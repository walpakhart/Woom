<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { ask } from '@tauri-apps/plugin-dialog';
  import { openUrl } from '@tauri-apps/plugin-opener';

  interface FileStatus { path: string; code: string; staged: boolean; unstaged: boolean; }
  interface GitStatus {
    branch: string | null;
    upstream: string | null;
    ahead: number;
    behind: number;
    files: FileStatus[];
  }
  interface Branch { name: string; is_current: boolean; is_remote: boolean; upstream: string | null; }

  interface Props {
    repo: string;
    onStatusChange?: (files: FileStatus[]) => void;
    onOpenDiff?: (path: string, staged: boolean) => void;
    /** Which agent the ✨-button should route to, or null to grey it out.
        Parent picks this from the first AI session linked to *this* editor
        (either Claude or Cursor). Both adapters ship a headless
        commit-message generator, so the button works with whichever
        agent the user has bridged. */
    aiKind?: 'claude' | 'cursor' | null;
  }
  let { repo, onStatusChange, onOpenDiff, aiKind = null }: Props = $props();

  let status = $state<GitStatus | null>(null);
  let branches = $state<Branch[]>([]);
  let loading = $state(false);
  let error = $state<string | null>(null);
  let commitMsg = $state('');
  let busy = $state<string | null>(null);
  let showBranches = $state(false);
  let newBranchName = $state('');
  let creating = $state(false);

  // PR creation
  let ghAvailable = $state(false);
  let prOpen = $state(false);
  let prTitle = $state('');
  let prBody = $state('');
  let prDraft = $state(false);
  let prBase = $state('');
  let lastPrUrl = $state<string | null>(null);

  onMount(async () => {
    ghAvailable = await invoke<boolean>('pr_create_available').catch(() => false);
    // NOTE: we intentionally do NOT listen to `fs:changed` here. EditorView
    // owns the single subscription, debounces bursts, and hands us results
    // via `onStatusChange`. Double-listening caused overlapping `git status`
    // calls under Vite HMR / Claude multi-file edits / git's own index-lock
    // writes — a feedback loop that hung the UI.
  });

  export async function refresh() {
    if (!repo) { status = null; branches = []; onStatusChange?.([]); return; }
    loading = true;
    error = null;
    try {
      const [s, b] = await Promise.all([
        invoke<GitStatus>('git_status', { repo }),
        invoke<Branch[]>('git_branches', { repo })
      ]);
      status = s;
      branches = b;
      onStatusChange?.(s.files);
    } catch (e: unknown) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      loading = false;
    }
  }

  async function withBusy(label: string, fn: () => Promise<unknown>) {
    busy = label;
    error = null;
    try {
      await fn();
      await refresh();
    } catch (e: unknown) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      busy = null;
    }
  }

  function stageFile(f: FileStatus) {
    if (f.staged && !f.unstaged) {
      void withBusy('unstage', () => invoke('git_unstage', { repo, paths: [f.path] }));
    } else {
      void withBusy('stage', () => invoke('git_stage', { repo, paths: [f.path] }));
    }
  }

  function stageAll() {
    const paths = (status?.files ?? []).filter((f) => f.unstaged).map((f) => f.path);
    if (paths.length) void withBusy('stage-all', () => invoke('git_stage', { repo, paths }));
  }

  function discardFile(f: FileStatus) {
    // Per-file discard is a single-click, no-confirm action. "Discard all"
    // still confirms because that's a bulk destructive op; single files
    // are easily re-typed / re-added if mistaken.
    void withBusy('discard', () => invoke('git_discard', { repo, paths: [f.path] }));
  }

  /** Discard only UNSTAGED changes — intentionally does NOT touch staged
      files so the user can't accidentally wipe a whole commit's worth of
      work with one click. For staged files the UI offers "unstage all"
      instead (below), which is non-destructive. */
  async function discardAllUnstaged() {
    const files = (status?.files ?? []).filter((f) => f.unstaged && !f.staged);
    if (files.length === 0) return;
    const untrackedCount = files.filter((f) => f.code.startsWith('?')).length;
    const modifiedCount = files.length - untrackedCount;
    const parts: string[] = [];
    if (modifiedCount > 0) parts.push(`${modifiedCount} modified file${modifiedCount === 1 ? '' : 's'}`);
    if (untrackedCount > 0) parts.push(`${untrackedCount} untracked file${untrackedCount === 1 ? '' : 's'}`);
    // `window.confirm` gets swallowed by Tauri's WebKit — use the dialog
    // plugin's native `ask` so the modal actually appears.
    const ok = await ask(
      `This will revert ${parts.join(' and ')}.\n\nStaged files are left alone. This cannot be undone.`,
      {
        title: 'Discard unstaged changes?',
        kind: 'warning',
        okLabel: 'Discard',
        cancelLabel: 'Cancel'
      }
    );
    if (!ok) return;
    const paths = files.map((f) => f.path);
    void withBusy('discard-all', () => invoke('git_discard', { repo, paths }));
  }

  /** Unstage every staged file (safe — just `git reset HEAD --`, no file
      content is touched). Moves them back into the Changes section so the
      user can review / stage selectively / discard if needed. */
  function unstageAll() {
    const paths = (status?.files ?? []).filter((f) => f.staged).map((f) => f.path);
    if (paths.length === 0) return;
    void withBusy('unstage-all', () => invoke('git_unstage', { repo, paths }));
  }

  async function doCommit() {
    if (!commitMsg.trim()) return;
    await withBusy('commit', () => invoke<string>('git_commit', { repo, message: commitMsg.trim() }));
    commitMsg = '';
  }

  let aiGenerating = $state(false);
  let aiError = $state<string | null>(null);
  async function generateAiCommitMessage() {
    if (!aiKind || aiGenerating) return;
    if (stagedFiles.length === 0) {
      aiError = 'Stage changes first — AI needs a staged diff to summarize.';
      return;
    }
    aiGenerating = true;
    aiError = null;
    try {
      const msg = await invoke<string>('agent_generate_commit_message', {
        repo,
        agentKind: aiKind
      });
      commitMsg = msg;
    } catch (e) {
      aiError = typeof e === 'string' ? e : String(e);
    } finally {
      aiGenerating = false;
    }
  }

  async function doCommitAndPush() {
    if (!commitMsg.trim()) return;
    await withBusy('commit + push', () => invoke<string>('git_commit_and_push', { repo, message: commitMsg.trim() }));
    commitMsg = '';
  }

  async function switchBranch(name: string) {
    showBranches = false;
    await withBusy(`checkout ${name}`, () => invoke('git_checkout', { repo, branch: name }));
  }

  async function createBranch() {
    if (!newBranchName.trim()) return;
    const name = newBranchName.trim();
    await withBusy(`create ${name}`, () => invoke('git_create_branch', { repo, name, checkout: true }));
    newBranchName = '';
    creating = false;
  }

  function openPrDialog() {
    if (!ghAvailable) {
      error = 'GitHub is not connected — open the Connections tab and connect GitHub.';
      return;
    }
    // Suggest a PR title from the most recent commit subject.
    prTitle = '';
    prBody = '';
    prDraft = false;
    prBase = '';
    prOpen = true;
    invoke<{ sha: string; short_sha: string; author: string; date: string; subject: string }[]>(
      'git_log', { repo, limit: 1 }
    ).then((entries) => {
      if (entries[0]) prTitle = entries[0].subject;
    }).catch(() => {});
  }

  async function createPr() {
    if (!prTitle.trim()) return;
    await withBusy('create PR', async () => {
      const url = await invoke<string>('git_create_pr', {
        repo,
        title: prTitle.trim(),
        body: prBody,
        draft: prDraft,
        base: prBase.trim() || null
      });
      lastPrUrl = url;
      prOpen = false;
    });
  }

  $effect(() => { void refresh(); });

  const stagedFiles = $derived((status?.files ?? []).filter((f) => f.staged));
  const unstagedFiles = $derived((status?.files ?? []).filter((f) => f.unstaged && !f.staged));
  const localBranches = $derived(branches.filter((b) => !b.is_remote));
  const canOpenPr = $derived(!!status?.upstream && !!status?.branch && ghAvailable);
</script>

<div class="gp">
  <div class="gp-head">
    <button class="gp-branch" onclick={() => (showBranches = !showBranches)} disabled={loading || !repo}>
      <svg class="i i-sm" viewBox="0 0 24 24"><path d="M6 3v18M6 9a3 3 0 0 0 3 3h4a3 3 0 0 1 3 3v6M18 3a3 3 0 1 1 0 6 3 3 0 0 1 0-6z" /></svg>
      <span class="mono gp-branch-name">{status?.branch ?? '—'}</span>
      {#if status && (status.ahead || status.behind)}
        <span class="gp-counts">
          {#if status.ahead}↑{status.ahead}{/if}
          {#if status.behind}↓{status.behind}{/if}
        </span>
      {/if}
      <svg class="i i-sm" viewBox="0 0 24 24"><path d="M6 9l6 6 6-6" /></svg>
    </button>
    <button class="gp-btn" onclick={refresh} disabled={loading} title="Refresh status">
      <svg class="i i-sm" viewBox="0 0 24 24"><path d="M3 12a9 9 0 0 1 15-6.7L21 8M21 3v5h-5" /><path d="M21 12a9 9 0 0 1-15 6.7L3 16M3 21v-5h5" /></svg>
    </button>
    <button class="gp-btn" onclick={() => withBusy('pull', () => invoke('git_pull', { repo }))} disabled={!!busy || !repo} title="git pull">
      <svg class="i i-sm" viewBox="0 0 24 24"><path d="M12 4v12M6 10l6 6 6-6M5 20h14" /></svg>
    </button>
    <button class="gp-btn" onclick={() => withBusy('push', () => invoke('git_push', { repo }))} disabled={!!busy || !repo} title="git push">
      <svg class="i i-sm" viewBox="0 0 24 24"><path d="M12 20V8M6 14l6-6 6 6M5 4h14" /></svg>
    </button>
    <button class="gp-btn" onclick={openPrDialog} disabled={!canOpenPr || !!busy} title={canOpenPr ? 'Open PR' : 'Needs upstream + gh CLI'}>
      <svg class="i i-sm" viewBox="0 0 24 24"><circle cx="6" cy="6" r="2.5"/><circle cx="6" cy="18" r="2.5"/><circle cx="18" cy="18" r="2.5"/><path d="M6 8.5v7M8.5 6h7a3 3 0 0 1 3 3v6.5"/></svg>
    </button>
  </div>

  {#if showBranches}
    <div class="gp-branches">
      <div class="gp-section-title">Local branches</div>
      {#each localBranches as b (b.name)}
        <button class="gp-branch-row" class:current={b.is_current} onclick={() => switchBranch(b.name)} disabled={b.is_current || !!busy}>
          <span class="mono">{b.name}</span>
          {#if b.upstream}<span class="gp-upstream">→ {b.upstream}</span>{/if}
        </button>
      {/each}
      <div class="gp-new-branch">
        {#if creating}
          <input
            class="gp-input"
            placeholder="new-branch-name"
            bind:value={newBranchName}
            onkeydown={(e) => { if (e.key === 'Enter') void createBranch(); if (e.key === 'Escape') { creating = false; newBranchName = ''; } }}
            {@attach (node: HTMLInputElement) => node.focus()}
          />
          <button class="gp-btn-ghost" onclick={createBranch} disabled={!newBranchName.trim() || !!busy}>create</button>
          <button class="gp-btn-ghost" onclick={() => { creating = false; newBranchName = ''; }}>cancel</button>
        {:else}
          <button class="gp-btn-ghost" onclick={() => (creating = true)}>+ new branch</button>
        {/if}
      </div>
    </div>
  {/if}

  {#if error}<div class="gp-error">{error}</div>{/if}
  {#if busy}<div class="gp-busy">{busy}…</div>{/if}
  {#if lastPrUrl}
    <div class="gp-pr">
      <button class="gp-pr-link" onclick={() => openUrl(lastPrUrl!)}>{lastPrUrl}</button>
      <button class="gp-pr-dismiss" onclick={() => (lastPrUrl = null)}>✕</button>
    </div>
  {/if}

  {#if status}
    <div class="gp-body">
      {#if stagedFiles.length === 0 && unstagedFiles.length === 0}
        <div class="gp-empty">Working tree clean</div>
      {:else}
        {#if stagedFiles.length > 0}
          <div class="gp-section-head">
            <span class="gp-section-title">Staged ({stagedFiles.length})</span>
            <button class="gp-link" onclick={unstageAll} disabled={!!busy} title="Move every staged file back to Changes (non-destructive)">unstage all</button>
          </div>
          {#each stagedFiles as f (f.path)}
            <div class="gp-file-row gp-file-row--staged">
              <button class="gp-file" onclick={() => onOpenDiff?.(f.path, true)} title="Open diff">
                <span class="gp-code mono">{f.code}</span>
                <span class="gp-path mono">{f.path}</span>
              </button>
              <button class="gp-file-act" onclick={() => stageFile(f)} title="Unstage">
                <svg class="i i-sm" viewBox="0 0 24 24"><path d="M5 12h14" /></svg>
              </button>
            </div>
          {/each}
        {/if}
        {#if unstagedFiles.length > 0}
          <div class="gp-section-head">
            <span class="gp-section-title">Changes ({unstagedFiles.length})</span>
            <span class="gp-section-actions">
              <button class="gp-link" onclick={stageAll} disabled={!!busy}>stage all</button>
              <button class="gp-link gp-link--danger" onclick={discardAllUnstaged} disabled={!!busy} title="Revert every file in this list — staged files are NOT touched">discard all</button>
            </span>
          </div>
          {#each unstagedFiles as f (f.path)}
            <div class="gp-file-row">
              <button class="gp-file" onclick={() => onOpenDiff?.(f.path, false)} title="Open diff">
                <span class="gp-code mono">{f.code}</span>
                <span class="gp-path mono">{f.path}</span>
              </button>
              <button class="gp-file-act gp-file-act--discard" onclick={() => discardFile(f)} title={f.code.startsWith('?') ? 'Delete untracked' : 'Discard changes'}>
                <svg class="i i-sm" viewBox="0 0 24 24"><path d="M3 12a9 9 0 1 0 3-6.7M3 4v5h5" /></svg>
              </button>
              <button class="gp-file-act" onclick={() => stageFile(f)} title="Stage">
                <svg class="i i-sm" viewBox="0 0 24 24"><path d="M12 5v14M5 12h14" /></svg>
              </button>
            </div>
          {/each}
        {/if}
      {/if}
    </div>

    {#if stagedFiles.length > 0}
      <div class="gp-commit">
        <div class="gp-commit-input-wrap">
          <input
            class="gp-input gp-commit-input"
            placeholder={aiGenerating ? 'AI is writing…' : 'Commit message'}
            bind:value={commitMsg}
            disabled={aiGenerating}
            onkeydown={(e) => { if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') void (e.shiftKey ? doCommitAndPush() : doCommit()); }}
          />
          <button
            class="gp-ai-btn"
            onclick={generateAiCommitMessage}
            disabled={!aiKind || aiGenerating || !!busy}
            title={aiKind
              ? `Ask the linked ${aiKind === 'claude' ? 'Claude' : 'Cursor'} chat to write a commit message from the staged diff`
              : 'No Claude or Cursor chat is linked to this Editor — link one to enable AI commit messages'}
            aria-label="Write commit message with AI"
          >
            {#if aiGenerating}
              <span class="gp-ai-spinner"></span>
            {:else}
              <svg class="i i-sm" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M12 3l1.5 4.5L18 9l-4.5 1.5L12 15l-1.5-4.5L6 9l4.5-1.5L12 3z"/><path d="M19 14l0.8 2.4L22 17l-2.2 0.6L19 20l-0.8-2.4L16 17l2.2-0.6L19 14z"/></svg>
            {/if}
          </button>
        </div>
        <button class="gp-commit-btn" onclick={doCommit} disabled={!commitMsg.trim() || !!busy} title="⌘↵">Commit</button>
        <button class="gp-commit-btn gp-commit-btn--alt" onclick={doCommitAndPush} disabled={!commitMsg.trim() || !!busy} title="⇧⌘↵">&amp; push</button>
      </div>
      {#if aiError}
        <div class="gp-ai-error">{aiError}</div>
      {/if}
    {/if}
  {/if}

  {#if prOpen}
    <div class="gp-pr-dialog">
      <div class="gp-section-title" style="padding: 10px 12px 4px;">Open pull request</div>
      <input class="gp-input gp-pr-input" placeholder="Title" bind:value={prTitle} />
      <textarea class="gp-input gp-pr-textarea" placeholder="Body (optional, supports markdown)" rows="4" bind:value={prBody}></textarea>
      <input class="gp-input gp-pr-input" placeholder="Base branch (empty = repo default)" bind:value={prBase} />
      <label class="gp-pr-check">
        <input type="checkbox" bind:checked={prDraft} /> Draft
      </label>
      <div class="gp-pr-actions">
        <button class="gp-btn-ghost" onclick={() => (prOpen = false)}>Cancel</button>
        <button class="gp-commit-btn" onclick={createPr} disabled={!prTitle.trim() || !!busy}>Create</button>
      </div>
    </div>
  {/if}
</div>

<style>
  .gp { display: flex; flex-direction: column; height: 100%; min-height: 0; background: var(--bg-1); border-top: 1px solid var(--border-neutral); }
  .gp-head {
    display: flex; align-items: center; gap: 6px;
    padding: 8px 10px;
    border-bottom: 1px solid var(--border-neutral);
    background: var(--bg-2);
    flex-shrink: 0;
  }
  .gp-branch {
    display: inline-flex; align-items: center; gap: 6px;
    padding: 4px 8px;
    border-radius: 6px;
    background: var(--bg-3);
    color: var(--text-0);
    font-size: 12px;
    flex: 1;
    min-width: 0;
  }
  .gp-branch:hover:not(:disabled) { background: var(--bg-3); outline: 1px solid var(--border-hi); }
  .gp-branch-name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .gp-counts { font-size: 10.5px; color: var(--accent-bright); margin-left: 4px; }
  .gp-btn {
    display: inline-flex; align-items: center; justify-content: center;
    width: 26px; height: 26px; border-radius: 6px;
    color: var(--text-1);
    flex-shrink: 0;
  }
  .gp-btn:hover:not(:disabled) { background: var(--bg-3); color: var(--text-0); }
  .gp-btn:disabled { opacity: 0.4; cursor: default; }

  .gp-branches {
    border-bottom: 1px solid var(--border-neutral);
    padding: 6px 0;
    max-height: 250px; overflow-y: auto;
    flex-shrink: 0;
  }
  .gp-section-title { font-size: 10.5px; color: var(--text-2); padding: 6px 12px 4px; text-transform: uppercase; letter-spacing: 0.06em; }
  .gp-section-head { display: flex; align-items: baseline; justify-content: space-between; padding-right: 12px; }
  .gp-branch-row {
    display: flex; align-items: center; gap: 8px;
    width: 100%; padding: 4px 12px;
    font-size: 12px; color: var(--text-1);
    text-align: left;
  }
  .gp-branch-row:hover:not(:disabled) { background: var(--bg-2); color: var(--text-0); }
  .gp-branch-row.current { color: var(--accent-bright); }
  .gp-branch-row.current::before { content: '●'; font-size: 8px; }
  .gp-upstream { color: var(--text-mute); font-size: 11px; }
  .gp-new-branch { display: flex; gap: 6px; align-items: center; padding: 6px 12px 2px; }
  .gp-btn-ghost {
    font-size: 11.5px; color: var(--text-1);
    padding: 3px 8px; border-radius: 4px;
  }
  .gp-btn-ghost:hover:not(:disabled) { background: var(--bg-2); color: var(--text-0); }

  /* `scrollbar-gutter: stable` reserves the scrollbar track even when the
     list fits — without it, the scrollbar overlays the right-side action
     buttons (discard / stage) and covers them. */
  .gp-body { flex: 1; overflow: auto; padding: 4px 0; min-height: 0; scrollbar-gutter: stable; }
  .gp-empty { padding: 12px; font-size: 12px; color: var(--text-2); text-align: center; }
  .gp-file-row {
    display: flex; align-items: stretch;
    width: 100%;
    transition: background 80ms;
  }
  .gp-file-row:hover { background: var(--bg-2); }
  .gp-file-row:hover .gp-file-act { opacity: 1; }
  .gp-file {
    display: flex; align-items: center; gap: 8px;
    flex: 1; padding: 3px 12px;
    font-size: 12px; color: var(--text-1);
    text-align: left;
    min-width: 0;
  }
  .gp-file:hover { color: var(--text-0); }
  .gp-file-row--staged .gp-file { color: var(--accent-bright); }
  .gp-file-row--staged .gp-file:hover { color: var(--accent); }
  .gp-code { width: 24px; font-size: 11px; color: var(--text-2); flex-shrink: 0; }
  .gp-file-row--staged .gp-code { color: var(--accent); }
  .gp-path { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .gp-file-act {
    display: inline-flex; align-items: center; justify-content: center;
    width: 22px; flex-shrink: 0;
    color: var(--text-2);
    opacity: 0;
    transition: opacity 100ms, color 100ms;
  }
  .gp-file-act:hover { color: var(--accent-bright); }
  .gp-file-act--discard:hover { color: var(--error); }
  .gp-link { font-size: 11px; color: var(--accent-bright); }
  .gp-link:hover { text-decoration: underline; }
  .gp-link:disabled { opacity: 0.45; cursor: default; }
  .gp-link--danger { color: var(--error); }
  .gp-section-actions { display: inline-flex; gap: 10px; align-items: baseline; }

  .gp-commit {
    display: flex; flex-wrap: wrap; row-gap: 6px; gap: 6px;
    padding: 8px 10px;
    border-top: 1px solid var(--border-neutral);
    background: var(--bg-2);
    flex-shrink: 0;
  }
  /* In narrow columns, the commit-message field takes the whole first row
     and the Commit / & push buttons wrap below. Keeps the input usable
     instead of shrinking to a few characters wide. */
  .gp-commit-input-wrap { flex: 1 1 200px; min-width: 140px; }
  .gp-input {
    flex: 1;
    padding: 6px 10px;
    border-radius: 6px;
    background: var(--bg-0);
    border: 1px solid var(--border-neutral-hi);
    color: var(--text-0);
    font-size: 12.5px;
    font-family: inherit;
  }
  .gp-input:focus { border-color: var(--border-hi2); outline: none; }
  .gp-commit-btn {
    padding: 6px 12px;
    border-radius: 6px;
    background: var(--accent);
    color: var(--accent-fg);
    font-size: 12px; font-weight: 600;
    flex-shrink: 0;
  }
  .gp-commit-btn:hover:not(:disabled) { background: var(--accent-bright); }
  .gp-commit-btn:disabled { opacity: 0.4; cursor: default; }
  .gp-commit-btn--alt { background: var(--bg-3); color: var(--accent-bright); border: 1px solid var(--border-hi2); }
  .gp-commit-btn--alt:hover:not(:disabled) { background: var(--bg-2); color: var(--accent); }

  /* Composite commit-message field: text input + an embedded ✨ AI button
     on the right so the AI shortcut reads as part of the input rather than
     a floating action. */
  .gp-commit-input-wrap { position: relative; display: flex; }
  .gp-commit-input { padding-right: 34px; flex: 1; min-width: 0; }
  .gp-ai-btn {
    position: absolute; top: 50%; right: 4px; transform: translateY(-50%);
    width: 26px; height: 26px; border-radius: 5px;
    display: inline-flex; align-items: center; justify-content: center;
    color: var(--accent-bright); background: transparent;
    transition: all 120ms;
  }
  .gp-ai-btn:hover:not(:disabled) { background: var(--accent-soft); color: var(--accent); }
  .gp-ai-btn:disabled { color: var(--text-mute); opacity: 0.5; cursor: not-allowed; }
  .gp-ai-spinner {
    width: 12px; height: 12px; border-radius: 50%;
    border: 1.5px solid var(--border-neutral-hi);
    border-top-color: var(--accent-bright);
    animation: spin 640ms linear infinite;
  }
  @keyframes spin { to { transform: rotate(360deg); } }
  .gp-ai-error {
    padding: 4px 12px 8px;
    font-size: 11.5px; color: var(--error);
    background: var(--bg-2);
    border-top: 1px dashed rgba(232, 130, 100, 0.3);
    flex-shrink: 0;
  }

  .gp-pr {
    display: flex; align-items: center; gap: 6px;
    padding: 6px 10px;
    background: var(--accent-soft);
    border-top: 1px solid var(--border-hi);
    flex-shrink: 0;
  }
  .gp-pr-link {
    flex: 1;
    font-size: 11.5px; color: var(--accent-bright);
    text-align: left; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .gp-pr-link:hover { text-decoration: underline; }
  .gp-pr-dismiss { width: 18px; height: 18px; color: var(--text-2); border-radius: 3px; }
  .gp-pr-dismiss:hover { background: var(--bg-3); color: var(--text-0); }

  .gp-pr-dialog {
    border-top: 1px solid var(--border-neutral);
    padding: 0 12px 10px;
    background: var(--bg-2);
    display: flex; flex-direction: column; gap: 6px;
    flex-shrink: 0;
  }
  .gp-pr-input { width: 100%; }
  .gp-pr-textarea { width: 100%; resize: vertical; font-family: inherit; line-height: 1.5; }
  .gp-pr-check { display: flex; align-items: center; gap: 6px; font-size: 12px; color: var(--text-1); }
  .gp-pr-actions { display: flex; justify-content: flex-end; gap: 6px; margin-top: 4px; }

  .gp-error { padding: 8px 12px; font-size: 11.5px; color: var(--error); border-bottom: 1px solid rgba(232, 130, 100, 0.24); background: rgba(232, 130, 100, 0.1); }
  .gp-busy { padding: 6px 12px; font-size: 11px; color: var(--text-2);  }
</style>
