<script lang="ts">
  import { onDestroy, tick } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import { EditorView, basicSetup } from 'codemirror';
  import { EditorState } from '@codemirror/state';
  import { oneDark } from '@codemirror/theme-one-dark';
  import type { FileBlob, RepoBranch, RepoCommit, TreeEntry } from './data';
  import { languageFor } from './codemirrorLang';
  import Markdown from './Markdown.svelte';

  interface Props {
    owner: string;
    repo: string;
    defaultBranch: string;
    htmlUrl: string;
    now: number;
  }
  let { owner, repo, defaultBranch, htmlUrl, now }: Props = $props();

  type SubView = 'files' | 'commits';

  let branch = $state<string>('');
  $effect(() => {
    if (!branch && defaultBranch) branch = defaultBranch;
  });
  let subView = $state<SubView>('files');
  let branches = $state<RepoBranch[]>([]);
  let showBranchPicker = $state(false);
  let branchFilter = $state<string>('');
  let branchesLoading = $state(false);

  let tree = $state<TreeEntry[]>([]);
  let treeLoading = $state(false);
  let treeError = $state<string | null>(null);
  let truncated = $state(false);

  // Expand/collapse per directory path.
  let expanded = $state<Set<string>>(new Set());

  let selectedPath = $state<string>('');
  let blob = $state<FileBlob | null>(null);
  let blobLoading = $state(false);
  let blobError = $state<string | null>(null);

  let commits = $state<RepoCommit[]>([]);
  let commitsLoading = $state(false);
  let commitsError = $state<string | null>(null);

  // CodeMirror viewer instance.
  let viewerEl: HTMLDivElement;
  let viewer: EditorView | null = null;

  $effect(() => {
    void loadTree();
    // eslint-disable-next-line @typescript-eslint/no-unused-expressions
    owner; repo; branch;
  });

  $effect(() => {
    if (subView === 'commits' && !commits.length && !commitsLoading) void loadCommits();
  });

  async function loadBranches() {
    if (branches.length) { showBranchPicker = !showBranchPicker; return; }
    branchesLoading = true;
    try {
      branches = await invoke<RepoBranch[]>('github_list_repo_branches', { owner, repo });
    } catch {/* ignore */}
    finally { branchesLoading = false; showBranchPicker = true; }
  }

  function pickBranch(name: string) {
    showBranchPicker = false;
    if (name === branch) return;
    branch = name;
    // Reset view.
    selectedPath = '';
    blob = null;
    commits = [];
  }

  async function loadTree() {
    if (!owner || !repo || !branch) return;
    treeLoading = true;
    treeError = null;
    try {
      const res = await invoke<TreeEntry[]>('github_list_tree', {
        owner, repo, reference: branch
      });
      truncated = res.some((e) => e.kind === 'notice');
      tree = res.filter((e) => e.kind !== 'notice');
    } catch (e) {
      treeError = typeof e === 'string' ? e : String(e);
    } finally {
      treeLoading = false;
    }
  }

  async function loadCommits() {
    if (!owner || !repo || !branch) return;
    commitsLoading = true;
    commitsError = null;
    try {
      commits = await invoke<RepoCommit[]>('github_list_repo_commits', {
        owner, repo, reference: branch, limit: 30
      });
    } catch (e) {
      commitsError = typeof e === 'string' ? e : String(e);
    } finally {
      commitsLoading = false;
    }
  }

  async function openFile(path: string) {
    if (path === selectedPath) return;
    selectedPath = path;
    blob = null;
    blobError = null;
    blobLoading = true;
    try {
      blob = await invoke<FileBlob>('github_get_file_content', {
        owner, repo, path, reference: branch
      });
      await tick();
      renderBlob();
    } catch (e) {
      blobError = typeof e === 'string' ? e : String(e);
    } finally {
      blobLoading = false;
    }
  }

  function renderBlob() {
    if (!blob || !viewerEl) return;
    viewer?.destroy();
    if (!blob.is_text) {
      viewer = null;
      return;
    }
    viewer = new EditorView({
      parent: viewerEl,
      state: EditorState.create({
        doc: blob.content,
        extensions: [
          basicSetup,
          oneDark,
          languageFor(blob.path),
          EditorState.readOnly.of(true),
          EditorView.editable.of(false),
          EditorView.lineWrapping
        ]
      })
    });
  }

  onDestroy(() => viewer?.destroy());

  // Build a flat, expansion-aware view from the full recursive tree list.
  // `tree` is already flat; we build visible rows based on `expanded` dirs.
  interface Row { path: string; name: string; kind: string; depth: number; }
  const visibleRows = $derived.by<Row[]>(() => {
    if (!tree.length) return [];
    // Index entries by path; determine parent dir presence.
    const byPath = new Map(tree.map((e) => [e.path, e] as const));
    // Ensure intermediate directories exist (GitHub only lists `tree` entries
    // for actual directory refs, which is fine, but be safe).
    const out: Row[] = [];
    // Sort: directories first within siblings, alphabetical.
    const sorted = [...tree].sort((a, b) => {
      if (a.kind !== b.kind) {
        if (a.kind === 'tree') return -1;
        if (b.kind === 'tree') return 1;
      }
      return a.path.localeCompare(b.path);
    });
    for (const entry of sorted) {
      const parts = entry.path.split('/');
      const depth = parts.length - 1;
      // Check every ancestor is expanded.
      let visible = true;
      for (let i = 1; i < parts.length; i++) {
        const parentPath = parts.slice(0, i).join('/');
        if (!expanded.has(parentPath)) { visible = false; break; }
      }
      if (!visible) continue;
      out.push({
        path: entry.path,
        name: parts[parts.length - 1],
        kind: entry.kind,
        depth
      });
    }
    // Hide byPath unused (silence lint) — keeping for future use.
    void byPath;
    return out;
  });

  function toggle(row: Row) {
    if (row.kind === 'tree') {
      if (expanded.has(row.path)) expanded.delete(row.path);
      else expanded.add(row.path);
      expanded = new Set(expanded);
    } else if (row.kind === 'blob') {
      void openFile(row.path);
    } else if (row.kind === 'commit') {
      // submodule: open on GitHub
      void openUrl(`${htmlUrl}/tree/${branch}/${row.path}`);
    }
  }

  function relTime(iso: string): string {
    const t = new Date(iso).getTime();
    const diff = Math.max(0, now - t);
    const m = 60_000, h = 3_600_000, d = 86_400_000, w = 604_800_000;
    if (diff < m) return 'now';
    if (diff < h) return `${Math.round(diff / m)}m`;
    if (diff < d) return `${Math.round(diff / h)}h`;
    if (diff < w) return `${Math.round(diff / d)}d`;
    return `${Math.round(diff / w)}w`;
  }

  const filteredBranches = $derived(
    branches.filter((b) => b.name.toLowerCase().includes(branchFilter.toLowerCase())).slice(0, 200)
  );
</script>

<div class="rcv">
  <div class="rcv-bar">
    <button class="rcv-branch" onclick={loadBranches} disabled={branchesLoading} title="Switch branch">
      <svg class="i i-sm" viewBox="0 0 24 24"><path d="M6 3v18M6 9a3 3 0 0 0 3 3h4a3 3 0 0 1 3 3v6M18 3a3 3 0 1 1 0 6 3 3 0 0 1 0-6z" /></svg>
      <span class="mono">{branch}</span>
      <svg class="i i-sm" viewBox="0 0 24 24"><path d="M6 9l6 6 6-6" /></svg>
    </button>
    <div class="rcv-sub">
      <button class="rcv-sub-tab" class:active={subView === 'files'} onclick={() => (subView = 'files')}>Files</button>
      <button class="rcv-sub-tab" class:active={subView === 'commits'} onclick={() => (subView = 'commits')}>Commits</button>
    </div>
    <div style="flex:1"></div>
    <button class="rcv-ghost" onclick={() => openUrl(`${htmlUrl}/tree/${branch}`)} title="Open on GitHub">
      <svg class="i i-sm" viewBox="0 0 24 24"><path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6" /><path d="M15 3h6v6M10 14 21 3" /></svg>
    </button>
  </div>

  {#if showBranchPicker}
    <div class="rcv-branches">
      <input class="rcv-branch-filter" placeholder="Filter branches…" bind:value={branchFilter} />
      <div class="rcv-branch-list">
        {#if branchesLoading && !branches.length}
          <div class="rcv-state">Loading branches…</div>
        {/if}
        {#each filteredBranches as b (b.name)}
          <button class="rcv-branch-row" class:current={b.name === branch} onclick={() => pickBranch(b.name)}>
            <span class="mono">{b.name}</span>
            {#if b.name === branch}<span class="rcv-tag">current</span>{/if}
            {#if b.protected}<span class="rcv-tag rcv-tag--protect">protected</span>{/if}
          </button>
        {/each}
      </div>
    </div>
  {/if}

  {#if subView === 'files'}
    <div class="rcv-files">
      <aside class="rcv-tree">
        {#if treeLoading}
          <div class="rcv-state">Loading tree…</div>
        {:else if treeError}
          <div class="rcv-state rcv-err">{treeError}</div>
        {:else}
          {#if truncated}
            <div class="rcv-state rcv-warn">Tree truncated by GitHub (very large repo). Showing up to 100k entries.</div>
          {/if}
          {#each visibleRows as row (row.path)}
            <button
              class="rcv-row"
              class:selected={row.path === selectedPath && row.kind === 'blob'}
              class:dir={row.kind === 'tree'}
              style="padding-left: {8 + row.depth * 12}px"
              onclick={() => toggle(row)}
              title={row.path}
            >
              <span class="rcv-chevron">
                {#if row.kind === 'tree'}
                  <svg class="i i-sm" viewBox="0 0 24 24" style="transform: rotate({expanded.has(row.path) ? 90 : 0}deg)"><path d="M9 6l6 6-6 6" /></svg>
                {:else if row.kind === 'commit'}
                  <span class="rcv-sub-icon">↗</span>
                {:else}
                  <span class="rcv-dot"></span>
                {/if}
              </span>
              <span class="rcv-name mono">{row.name}</span>
            </button>
          {/each}
        {/if}
      </aside>
      <main class="rcv-viewer">
        {#if blobLoading}
          <div class="rcv-state">Loading file…</div>
        {:else if blobError}
          <div class="rcv-state rcv-err">{blobError}</div>
        {:else if !selectedPath}
          <div class="rcv-state">Pick a file from the tree to view it here.</div>
        {:else if blob && !blob.is_text}
          {@const b = blob}
          <div class="rcv-state">Binary file — {b.size.toLocaleString()} bytes. <button class="rcv-link" onclick={() => openUrl(`${htmlUrl}/blob/${branch}/${b.path}`)}>Open on GitHub</button></div>
        {:else if blob && (blob.path.endsWith('.md') || blob.path.endsWith('.markdown'))}
          {@const b = blob}
          <div class="rcv-md">
            <div class="rcv-md-head mono">{b.path}</div>
            <div class="rcv-md-body"><Markdown source={b.content} /></div>
          </div>
        {:else}
          <div class="rcv-cm-head mono">{selectedPath}</div>
          <div class="rcv-cm" bind:this={viewerEl}></div>
        {/if}
      </main>
    </div>
  {:else}
    <div class="rcv-commits">
      {#if commitsLoading && !commits.length}
        <div class="rcv-state">Loading commits…</div>
      {:else if commitsError}
        <div class="rcv-state rcv-err">{commitsError}</div>
      {:else if commits.length === 0}
        <div class="rcv-state">No commits yet.</div>
      {:else}
        {#each commits as c (c.sha)}
          <button class="rcv-commit-row" onclick={() => openUrl(c.html_url)}>
            <span class="rcv-commit-sha mono">{c.short_sha}</span>
            <span class="rcv-commit-msg">{c.message}</span>
            {#if c.author_avatar}
              <img class="rcv-commit-avatar" src={c.author_avatar} alt={c.author_login ?? c.author_name} />
            {/if}
            <span class="rcv-commit-author mono">@{c.author_login ?? c.author_name}</span>
            <span class="rcv-commit-time mono">{relTime(c.date)}</span>
          </button>
        {/each}
      {/if}
    </div>
  {/if}
</div>

<style>
  .rcv { display: flex; flex-direction: column; height: 100%; min-height: 0; }
  .rcv-bar {
    display: flex; align-items: center; gap: 8px;
    padding: 8px 28px;
    border-bottom: 1px solid var(--border-neutral);
    background: var(--bg-1);
  }
  .rcv-branch {
    display: inline-flex; align-items: center; gap: 8px;
    padding: 6px 12px;
    border-radius: 7px;
    background: var(--bg-2);
    color: var(--text-0);
    font-size: 12px;
  }
  .rcv-branch:hover:not(:disabled) { background: var(--bg-3); }
  .rcv-sub { display: inline-flex; gap: 2px; background: var(--bg-2); border-radius: 7px; padding: 2px; }
  .rcv-sub-tab {
    padding: 5px 12px; border-radius: 5px;
    font-size: 11.5px; color: var(--text-1);
  }
  .rcv-sub-tab:hover { color: var(--text-0); }
  .rcv-sub-tab.active { background: var(--bg-0); color: var(--accent-bright); }
  .rcv-ghost {
    display: inline-flex; align-items: center; justify-content: center;
    width: 28px; height: 28px; border-radius: 6px;
    color: var(--text-2);
  }
  .rcv-ghost:hover { background: var(--bg-2); color: var(--text-0); }

  .rcv-branches {
    position: absolute; left: 28px; top: 56px;
    width: 340px; max-height: 420px;
    background: var(--bg-2);
    border: 1px solid var(--border-hi);
    border-radius: 8px;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.4);
    display: flex; flex-direction: column;
    z-index: 20;
  }
  .rcv-branch-filter {
    padding: 8px 12px; border: none; border-bottom: 1px solid var(--border-neutral);
    background: transparent; color: var(--text-0); font-size: 12px;
    font-family: inherit;
  }
  .rcv-branch-filter:focus { outline: none; background: var(--bg-3); }
  .rcv-branch-list { overflow-y: auto; padding: 4px 0; }
  .rcv-branch-row {
    display: flex; align-items: center; gap: 8px;
    width: 100%; padding: 4px 12px;
    font-size: 12px; color: var(--text-1);
    text-align: left;
  }
  .rcv-branch-row:hover { background: var(--bg-3); color: var(--text-0); }
  .rcv-branch-row.current { color: var(--accent-bright); }
  .rcv-tag {
    font-size: 9.5px; padding: 1px 6px; border-radius: 3px;
    background: var(--bg-3); color: var(--text-2);
  }
  .rcv-tag--protect { background: rgba(229, 162, 42, 0.15); color: var(--warning); }

  .rcv-files { flex: 1; display: flex; min-height: 0; }
  .rcv-tree {
    width: 320px; min-width: 240px; max-width: 480px;
    overflow-y: auto;
    border-right: 1px solid var(--border-neutral);
    background: var(--bg-1);
    padding: 6px 0;
  }
  .rcv-row {
    display: flex; align-items: center; gap: 6px;
    width: 100%; padding: 3px 10px;
    font-size: 12px; color: var(--text-1);
    text-align: left;
  }
  .rcv-row:hover { background: var(--bg-2); color: var(--text-0); }
  .rcv-row.selected { background: var(--accent-soft); color: var(--accent-bright); }
  .rcv-row.dir { color: var(--text-0); font-weight: 500; }
  .rcv-chevron {
    display: inline-flex; width: 14px; height: 14px;
    align-items: center; justify-content: center; flex-shrink: 0;
    color: var(--text-2);
  }
  .rcv-chevron :global(svg) { width: 11px; height: 11px; transition: transform 120ms; }
  .rcv-dot { width: 3px; height: 3px; border-radius: 50%; background: var(--text-mute); }
  .rcv-sub-icon { font-size: 10px; color: var(--text-2); }
  .rcv-name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }

  .rcv-viewer { flex: 1; min-width: 0; display: flex; flex-direction: column; overflow: hidden; }
  .rcv-cm-head,
  .rcv-md-head {
    padding: 8px 14px;
    border-bottom: 1px solid var(--border-neutral);
    background: var(--bg-1);
    font-size: 12px; color: var(--text-2);
  }
  .rcv-cm { flex: 1; min-height: 0; overflow: hidden; }
  .rcv-cm :global(.cm-editor) { height: 100%; font-family: 'JetBrains Mono', ui-monospace, 'SF Mono', monospace; font-size: 12.5px; }
  .rcv-cm :global(.cm-editor.cm-focused) { outline: none; }
  .rcv-md { flex: 1; display: flex; flex-direction: column; overflow: hidden; }
  .rcv-md-body { flex: 1; overflow: auto; padding: 18px 28px 40px; }

  .rcv-commits { flex: 1; overflow-y: auto; padding: 8px 28px 40px; }
  .rcv-commit-row {
    display: grid;
    grid-template-columns: 60px 1fr 20px auto auto;
    gap: 10px; align-items: center;
    width: 100%; padding: 8px 12px;
    border-radius: 6px;
    text-align: left;
    font-size: 12.5px; color: var(--text-1);
    transition: background 100ms;
  }
  .rcv-commit-row:hover { background: var(--bg-1); color: var(--text-0); }
  .rcv-commit-sha { color: var(--accent-bright); font-size: 11px; }
  .rcv-commit-msg { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--text-0); }
  .rcv-commit-avatar { width: 20px; height: 20px; border-radius: 50%; }
  .rcv-commit-author { font-size: 11px; color: var(--text-2); }
  .rcv-commit-time { font-size: 11px; color: var(--text-mute); }

  .rcv-state { padding: 14px 28px; color: var(--text-2); font-size: 12.5px; }
  .rcv-err { color: var(--error); }
  .rcv-warn { color: var(--warning); }
  .rcv-link { color: var(--accent-bright); text-decoration: underline; }
</style>
