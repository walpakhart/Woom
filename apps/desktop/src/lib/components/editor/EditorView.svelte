<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import { open as openDialog } from '@tauri-apps/plugin-dialog';
  import Editor from '$lib/components/editor/Editor.svelte';
  import FileTree from '$lib/components/editor/FileTree.svelte';
  import GitPanel from '$lib/components/editor/GitPanel.svelte';
  import HistoryPanel from '$lib/components/editor/HistoryPanel.svelte';
  import DiffView from '$lib/components/editor/DiffView.svelte';
  import Splitter from '$lib/components/ui/Splitter.svelte';
  import { notifyError } from '$lib/state/toaster.svelte';
  import { applyRangeToAgent } from '$lib/services/applyToAgent';

  const ROOT_STORAGE_KEY = 'forgehold:editor:root';
  const TABS_STORAGE_KEY = 'forgehold:editor:tabs';
  const SIDEBAR_TAB_KEY = 'forgehold:editor:sidebar-tab';

  /* Sidebar mode — VSCode-style tabs at the bottom of the explorer.
     "Work tree" shows just the file browser; "Git" shows the staging /
     commit panel + history of recent commits. Persisted across reloads
     so the user lands back on whichever pane they had open. */
  type SidebarTab = 'workTree' | 'git';
  let sidebarTab = $state<SidebarTab>(
    (localStorage.getItem(SIDEBAR_TAB_KEY) as SidebarTab) || 'workTree'
  );
  $effect(() => {
    localStorage.setItem(SIDEBAR_TAB_KEY, sidebarTab);
  });

  /* Bumped after every commit / push / pull / branch switch so the
     HistoryPanel inside the Git tab re-fetches automatically. */
  let gitChangeCount = $state(0);

  interface Props {
    /** Two-way bound to the parent so Claude sessions can pick up the repo
        as their default cwd. */
    repoPath?: string;
    /** Agent columns currently in the workbench. Drives the Link button /
        dropdown — shown only when there's at least one AI column to link to. */
    agentInstances?: { id: string; kind: 'claude' | 'cursor'; name: string }[];
    /** Sessions currently linked TO this editor — rendered as chips in the
        header so the link is visible from the editor side too (matches the
        "Linked to Editor" pill on the AI column). */
    linkedAgents?: { sessionId: string; agentInstanceId: string; kind: 'claude' | 'cursor'; name: string }[];
    /** Invoked when the user picks an AI column to link this editor to. The
        parent will reuse-or-spawn a session in that column pointing at this
        editor's folder and flag it linked. */
    onLinkToAgent?: (agentInstanceId: string) => void;
    /** Break the link for a specific session. Called from the X on each
        "Linked to" chip. */
    onUnlinkAgent?: (sessionId: string) => void;
  }
  let {
    repoPath = $bindable(''),
    agentInstances = [],
    linkedAgents = [],
    onLinkToAgent,
    onUnlinkAgent
  }: Props = $props();

  // Pick which linked agent the AI commit-message button routes to. Claude
  // wins over Cursor when both are linked — not a user preference, just a
  // stable tiebreaker so the UI is deterministic. Either one uses the
  // same headless one-off path on the backend.
  const linkedAiKind = $derived<'claude' | 'cursor' | null>(
    linkedAgents.find((a) => a.kind === 'claude')?.kind
      ?? linkedAgents.find((a) => a.kind === 'cursor')?.kind
      ?? null
  );

  let showLinkPicker = $state(false);

  let tabs = $state<string[]>([]);
  let activePath = $state<string>('');
  let dirtyByPath = $state<Record<string, boolean>>({});
  let editor: ReturnType<typeof Editor> | null = $state(null);
  let gitPanel = $state<{ refresh: () => Promise<void> } | null>(null);
  let error = $state<string | null>(null);
  let watchUnlisten: UnlistenFn | null = null;
  let gitStatusByPath = $state<Record<string, string>>({});
  let diffTarget = $state<{ path: string; staged: boolean } | null>(null);

  /* Live line range + viewport anchor of the user's selection in
     CodeMirror, mirrored up from <Editor> via `onSelectionChange`.
     `null` for the whole object means the selection collapsed to a
     caret — nothing to "apply to" yet. `anchor === null` means the
     selection is real but its end is currently scrolled out of the
     CodeMirror viewport; we keep the selection state so re-scrolling
     re-pops the popover, but render nothing in the meantime. Reset
     whenever the active file or diff mode changes — the new <Editor>
     instance starts with a fresh selection but doesn't fire
     `onSelectionChange` for the initial state, hence the explicit
     reset below. */
  let selection = $state<{
    startLine: number;
    endLine: number;
    anchor: { x: number; y: number } | null;
  } | null>(null);

  $effect(() => {
    activePath;
    diffTarget;
    selection = null;
  });

  /* Resolve "Apply to <agent>" buttons for the current selection.
     - 0 linked agents → empty (the bar still shows the selection
       range with a hint to link an agent).
     - 1 agent of a given kind → label is just the kind ("Claude" /
       "Cursor"). Two distinct kinds yields two short buttons.
     - 2+ agents of the same kind → suffix the column name so the
       user can tell e.g. Claude · Mona-Lisa apart from Claude ·
       Da-Vinci. We never drop the kind label even when one column
       is unique within its kind, because the user might mentally
       group "all Claudes" / "all Cursors" and expect consistent
       prefixing. */
  type ApplyBtn = {
    sessionId: string;
    agentInstanceId: string;
    label: string;
    kind: 'claude' | 'cursor';
  };
  const applyButtons = $derived.by<ApplyBtn[]>(() => {
    if (linkedAgents.length === 0) return [];
    const byKind: Record<'claude' | 'cursor', typeof linkedAgents> = {
      claude: [],
      cursor: []
    };
    for (const a of linkedAgents) byKind[a.kind].push(a);
    const out: ApplyBtn[] = [];
    for (const k of ['claude', 'cursor'] as const) {
      const group = byKind[k];
      if (group.length === 0) continue;
      const kindLabel = k === 'claude' ? 'Claude' : 'Cursor';
      if (group.length === 1) {
        out.push({
          sessionId: group[0].sessionId,
          agentInstanceId: group[0].agentInstanceId,
          kind: k,
          label: kindLabel
        });
      } else {
        for (const a of group) {
          out.push({
            sessionId: a.sessionId,
            agentInstanceId: a.agentInstanceId,
            kind: k,
            label: `${kindLabel} · ${a.name}`
          });
        }
      }
    }
    return out;
  });

  function selectionRangeText(): string {
    if (!selection) return '';
    return selection.startLine === selection.endLine
      ? `${selection.startLine}`
      : `${selection.startLine}-${selection.endLine}`;
  }

  /* No success/error toasts — the user gets the same intent
     conveyed by the agent column flipping its active session and
     the @-token appearing in the composer (which is in their
     direct line of sight when they click an Apply button). Toasts
     just add visual noise on every selection click. Errors here
     would only fire if the session was concurrently destroyed,
     which is rare enough to swallow silently rather than disrupt
     the flow with a popup. */
  function handleApplyTo(btn: ApplyBtn) {
    if (!selection || !activePath) return;
    applyRangeToAgent({
      sessionId: btn.sessionId,
      agentInstanceId: btn.agentInstanceId,
      filePath: activePath,
      startLine: selection.startLine,
      endLine: selection.endLine
    });
  }

  interface FileStatus { path: string; code: string; staged: boolean; unstaged: boolean; }
  interface GitStatusPayload {
    branch: string | null; upstream: string | null; ahead: number; behind: number; files: FileStatus[];
  }

  function onGitStatusChange(files: FileStatus[]) {
    const root = repoPath.replace(/\/$/, '');
    const map: Record<string, string> = {};
    for (const f of files) {
      // `code` is 2 chars: index + worktree. Pick the stronger indicator.
      const idx = f.code.charAt(0);
      const wt = f.code.charAt(1);
      let c = ' ';
      if (idx !== ' ' && idx !== '?') c = idx;
      else if (wt !== ' ') c = wt;
      if (c === ' ') c = 'M';
      map[`${root}/${f.path}`] = c;
    }
    gitStatusByPath = map;
  }

  /** Called after a successful ⌘S. Optimistic M + immediate refresh. */
  async function onFileSaved(savedPath: string) {
    gitStatusByPath = { ...gitStatusByPath, [savedPath]: 'M' };
    await refreshGitStatus(); // authoritative, shows real M or ? or A
    void gitPanel?.refresh();
  }

  let pollTimer: ReturnType<typeof setInterval> | null = null;
  let statusDebounce: ReturnType<typeof setTimeout> | null = null;
  let statusInFlight = false;
  let lastStatusAt = 0;

  /** Authoritative git-status refresh. Guarded against overlapping calls —
      if one is in flight we just skip (the next scheduleGitStatus will catch
      up). Called from: save hook, fs watcher (debounced), branch switch,
      polling timer. */
  async function refreshGitStatus() {
    if (!repoPath || statusInFlight) return;
    statusInFlight = true;
    try {
      const s = await invoke<GitStatusPayload>('git_status', { repo: repoPath });
      onGitStatusChange(s.files);
      lastStatusAt = Date.now();
    } catch (e) {
      console.warn('git_status failed:', e);
    } finally {
      statusInFlight = false;
    }
  }

  /** Coalesce a burst of events (Vite HMR, Claude multi-file edits, git
      internal writes) into a single `git status` call. */
  function scheduleGitStatus(delayMs = 250) {
    if (statusDebounce) clearTimeout(statusDebounce);
    statusDebounce = setTimeout(() => { void refreshGitStatus(); }, delayMs);
  }

  onMount(async () => {
    // Restore last-opened repo + tabs. The parent may have already set
    // `repoPath` (it reads the same localStorage key for its Claude cwd
    // fallback); in that case we just honor it and skip the restore.
    let rootToLoad = repoPath || localStorage.getItem(ROOT_STORAGE_KEY) || '';
    if (rootToLoad) {
      try {
        const exists = await invoke<boolean>('fs_path_exists', { path: rootToLoad });
        if (exists) {
          if (!repoPath) await setRoot(rootToLoad);
          else await startWatch();
          const savedTabs = JSON.parse(localStorage.getItem(TABS_STORAGE_KEY) || '[]');
          if (Array.isArray(savedTabs)) {
            for (const p of savedTabs) {
              const ok = await invoke<boolean>('fs_path_exists', { path: p });
              if (ok) tabs = [...tabs, p];
            }
            if (tabs.length) activePath = tabs[0];
          }
        }
      } catch {/* ignore */}
    }
    // Subscribe to file-change events — this is how we detect Claude's edits
    // and terminal edits. Debounced so a burst (e.g. Claude writing 5 files)
    // fires a single `git status` call, not 5.
    watchUnlisten = await listen<{ path: string; kind: string }>('fs:changed', (e) => {
      const p = e.payload.path;
      if (p === activePath && !dirtyByPath[activePath] && editor) {
        void editor.reload();
      }
      scheduleGitStatus(250);
    });

    // Safety-net polling every 3s, but only if we haven't refreshed recently.
    // Covers cases where the fs watcher misses events (network drives, Docker
    // mounts, some macOS fsevents quirks).
    pollTimer = setInterval(() => {
      if (document.hidden) return;
      if (Date.now() - lastStatusAt < 2500) return; // recent refresh, skip
      void refreshGitStatus();
    }, 3000);
  });

  onDestroy(() => {
    watchUnlisten?.();
    if (pollTimer) clearInterval(pollTimer);
    if (statusDebounce) clearTimeout(statusDebounce);
    if (repoPath) void invoke('fs_watch_stop').catch(() => {});
  });

  async function pickFolder() {
    let picked: string | string[] | null;
    try {
      picked = await openDialog({ directory: true, multiple: false, title: 'Open folder' });
    } catch (e) {
      notifyError(e, { title: "Couldn't open folder picker" });
      return;
    }
    if (!picked || Array.isArray(picked)) return;
    try {
      await setRoot(picked);
    } catch (e) {
      notifyError(e, { title: "Couldn't open folder" });
    }
  }

  async function setRoot(path: string) {
    error = null;
    try {
      const root = await invoke<string>('git_repo_root', { path });
      repoPath = (root || path).trim();
    } catch {
      repoPath = path;
    }
    localStorage.setItem(ROOT_STORAGE_KEY, repoPath);
    await startWatch();
  }

  async function startWatch() {
    try {
      await invoke('fs_watch_stop').catch(() => {});
      await invoke('fs_watch_start', { path: repoPath });
    } catch (e: unknown) {
      // Non-fatal: editor still works without auto-reload.
      console.warn('fs_watch_start failed:', e);
    }
  }

  /** Add `path` as a tab and activate it. Exported so EditorColumn
   *  can drive the editor from outside in response to
   *  `editorInstanceState.pendingOpenFile` signals (the diff card's
   *  clickable file path, future "go to file" UIs). Idempotent —
   *  re-clicking on an already-open tab just re-focuses it. */
  export function openFile(path: string) {
    diffTarget = null; // leaving diff mode
    if (!tabs.includes(path)) tabs = [...tabs, path];
    activePath = path;
    persistTabs();
  }

  function openDiff(relPath: string, staged: boolean) {
    diffTarget = { path: relPath, staged };
  }

  function closeDiff() {
    diffTarget = null;
  }

  async function switchTab(path: string) {
    if (path === activePath) return;
    if (dirtyByPath[activePath]) {
      const choice = confirm(
        `"${fileName(activePath)}" has unsaved changes. Save before switching?\n\nOK = save, Cancel = discard.`
      );
      if (choice) {
        await editor?.saveNow();
      } else {
        dirtyByPath = { ...dirtyByPath, [activePath]: false };
      }
    }
    activePath = path;
  }

  async function closeTab(path: string, ev?: MouseEvent) {
    ev?.stopPropagation();
    if (dirtyByPath[path]) {
      const keep = !confirm(`Discard unsaved changes in "${fileName(path)}"?`);
      if (keep) return;
    }
    const wasActive = activePath === path;
    const idx = tabs.indexOf(path);
    tabs = tabs.filter((p) => p !== path);
    const { [path]: _, ...rest } = dirtyByPath;
    dirtyByPath = rest;
    if (wasActive) {
      activePath = tabs[Math.max(0, Math.min(idx, tabs.length - 1))] ?? '';
    }
    persistTabs();
  }

  function persistTabs() {
    localStorage.setItem(TABS_STORAGE_KEY, JSON.stringify(tabs));
  }

  function onDirty(d: boolean) {
    if (!activePath) return;
    if (dirtyByPath[activePath] !== d) {
      dirtyByPath = { ...dirtyByPath, [activePath]: d };
    }
  }

  function fileName(p: string) {
    return p ? p.split('/').pop() ?? p : '';
  }

  function relToRepo(p: string) {
    if (!p || !repoPath) return p;
    return p.startsWith(repoPath + '/') ? p.slice(repoPath.length + 1) : p;
  }

  async function onTabMiddleClick(path: string, ev: MouseEvent) {
    if (ev.button === 1) {
      ev.preventDefault();
      await closeTab(path);
    }
  }
</script>

<div class="ev">
  {#if !repoPath}
    <section class="ev-empty">
      <div class="ev-empty-card">
        <h2 class="ev-empty-title">Open a repository</h2>
        <p class="ev-empty-sub">Pick a folder — Forgehold detects the git root and gives you the tree, editor, and git controls.</p>
        <button class="ev-empty-cta" onclick={pickFolder}>Open folder…</button>
      </div>
    </section>
  {:else}
    <Splitter direction="horizontal" persistKey="editor-main" initial={300} min={180} max={520}>
      {#snippet start()}
        <aside class="ev-left">
          <div class="ev-left-head">
            <span class="ev-root-name mono" title={repoPath}>{fileName(repoPath)}</span>
            {#each linkedAgents as la (la.sessionId)}
              <!-- Same `.linked-pill` shape the chat side uses, just the
                   `--compact` modifier so multiple pills fit next to the
                   open-folder name. Dot animation, palette, and bench-name
                   chip are now visually identical across editor/chat. -->
              <span class="linked-pill linked-pill--compact" title="Linked to {la.kind === 'claude' ? 'Claude Code' : 'Cursor'} · {la.name} — folder syncs between editor and this chat">
                <span class="linked-pill-dot"></span>
                <svg class="i i-sm" viewBox="0 0 24 24"><path d="M9 17H7A5 5 0 1 1 7 7h2M15 7h2a5 5 0 1 1 0 10h-2M8 12h8"/></svg>
                <span class="linked-pill-bench mono">{la.name}</span>
                {#if onUnlinkAgent}
                  <button
                    class="linked-pill-x"
                    onclick={() => onUnlinkAgent?.(la.sessionId)}
                    title="Unlink"
                    aria-label="Unlink"
                  >
                    <svg class="i i-sm" viewBox="0 0 24 24"><path d="M6 6l12 12M6 18L18 6"/></svg>
                  </button>
                {/if}
              </span>
            {/each}
            {#if onLinkToAgent && agentInstances.length > 0}
              <div class="ev-link-wrap">
                <button
                  class="ev-icon-btn"
                  onclick={() => {
                    if (agentInstances.length === 1) {
                      onLinkToAgent?.(agentInstances[0].id);
                    } else {
                      showLinkPicker = !showLinkPicker;
                    }
                  }}
                  title={agentInstances.length === 1
                    ? `Link ${agentInstances[0].kind === 'claude' ? 'Claude Code' : 'Cursor'} chat (${agentInstances[0].name}) to this folder`
                    : 'Link an AI chat to this folder'}
                  aria-label="Link to agent"
                >
                  <svg class="i i-sm" viewBox="0 0 24 24"><path d="M9 17H7A5 5 0 1 1 7 7h2M15 7h2a5 5 0 1 1 0 10h-2M8 12h8"/></svg>
                </button>
                {#if showLinkPicker && agentInstances.length > 1}
                  <div class="ev-link-menu" role="menu">
                    <div class="ev-link-menu-head">Link this folder to…</div>
                    {#each agentInstances as a (a.id)}
                      <button
                        class="ev-link-menu-item"
                        role="menuitem"
                        onclick={() => { onLinkToAgent?.(a.id); showLinkPicker = false; }}
                      >
                        <span class="ev-link-menu-kind">{a.kind === 'claude' ? 'Claude' : 'Cursor'}</span>
                        <span class="ev-link-menu-name mono">{a.name}</span>
                      </button>
                    {/each}
                  </div>
                {/if}
              </div>
            {/if}
            <button class="ev-icon-btn" onclick={pickFolder} title="Open another folder">
              <svg class="i i-sm" viewBox="0 0 24 24"><path d="M3 7v11a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2V9a2 2 0 0 0-2-2h-7L10 5H5a2 2 0 0 0-2 2z" /></svg>
            </button>
          </div>
          <!-- Sidebar body: tabbed between the file tree and the git
               pane. Tabs sit at the bottom (VSCode panel style) so the
               content fills upward from there. -->
          <div class="ev-sidebar-body">
            {#if sidebarTab === 'workTree'}
              <FileTree
                rootPath={repoPath}
                selectedPath={diffTarget ? `${repoPath}/${diffTarget.path}` : activePath}
                onSelect={openFile}
                {gitStatusByPath}
              />
            {:else}
              <Splitter direction="vertical" persistKey="editor-git-tab" initial={300} min={140} max={900}>
                {#snippet start()}
                  <GitPanel
                    bind:this={gitPanel}
                    repo={repoPath}
                    onStatusChange={(files) => { onGitStatusChange(files); gitChangeCount += 1; }}
                    onOpenDiff={openDiff}
                    aiKind={linkedAiKind}
                  />
                {/snippet}
                {#snippet end()}
                  <HistoryPanel repo={repoPath} refreshKey={gitChangeCount} />
                {/snippet}
              </Splitter>
            {/if}
          </div>
          <div class="ev-sidebar-tabs" role="tablist">
            <button
              class="ev-sidebar-tab"
              class:active={sidebarTab === 'workTree'}
              role="tab"
              aria-selected={sidebarTab === 'workTree'}
              onclick={() => (sidebarTab = 'workTree')}
              title="File explorer"
            >
              <svg class="i i-sm" viewBox="0 0 24 24"><path d="M3 7v11a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2V9a2 2 0 0 0-2-2h-7L10 5H5a2 2 0 0 0-2 2z"/></svg>
              <span>Work tree</span>
            </button>
            <button
              class="ev-sidebar-tab"
              class:active={sidebarTab === 'git'}
              role="tab"
              aria-selected={sidebarTab === 'git'}
              onclick={() => (sidebarTab = 'git')}
              title="Staging, commit, and history"
            >
              <svg class="i i-sm" viewBox="0 0 24 24"><circle cx="6" cy="6" r="2.5"/><circle cx="6" cy="18" r="2.5"/><circle cx="18" cy="12" r="2.5"/><path d="M6 8.5v7M8.5 6h4a3 3 0 0 1 3 3v.5"/></svg>
              <span>Git</span>
            </button>
          </div>
        </aside>
      {/snippet}
      {#snippet end()}
        <main class="ev-main">
          <div class="ev-tabbar">
            {#if diffTarget}
              <div class="ev-tab-wrap active" title={diffTarget.path}>
                <button class="ev-tab-btn" onclick={closeDiff}>
                  <span class="ev-tab-diff-icon" title="Diff">Δ</span>
                  <span class="ev-tab-name mono">{diffTarget.path}</span>
                  <span class="ev-tab-side">{diffTarget.staged ? 'staged' : 'working'}</span>
                </button>
                <button class="ev-tab-x" onclick={closeDiff} title="Close diff">
                  <svg class="i i-sm" viewBox="0 0 24 24"><path d="M6 6l12 12M6 18L18 6" /></svg>
                </button>
              </div>
            {:else if tabs.length === 0}
              <div class="ev-tab-empty">Pick a file in the tree to open it here.</div>
            {:else}
              {#each tabs as path (path)}
                <div
                  class="ev-tab-wrap"
                  class:active={path === activePath}
                  class:dirty={dirtyByPath[path]}
                  title={path}
                >
                  <button
                    class="ev-tab-btn"
                    onclick={() => void switchTab(path)}
                    onauxclick={(e) => void onTabMiddleClick(path, e)}
                  >
                    <span class="ev-tab-name mono">{relToRepo(path)}</span>
                  </button>
                  <button class="ev-tab-x" onclick={(e) => void closeTab(path, e)} title={dirtyByPath[path] ? 'Close (unsaved)' : 'Close'}>
                    {#if dirtyByPath[path]}
                      <span class="ev-tab-dot"></span>
                    {:else}
                      <svg class="i i-sm" viewBox="0 0 24 24"><path d="M6 6l12 12M6 18L18 6" /></svg>
                    {/if}
                  </button>
                </div>
              {/each}
            {/if}
          </div>
          <div class="ev-editor-wrap">
            {#if diffTarget}
              {#key `${diffTarget.path}:${diffTarget.staged}`}
                <DiffView repo={repoPath} path={diffTarget.path} staged={diffTarget.staged} />
              {/key}
            {:else if activePath}
              {#key activePath}
                <Editor
                  bind:this={editor}
                  path={activePath}
                  {onDirty}
                  onSaved={onFileSaved}
                  onSelectionChange={(sel) => (selection = sel)}
                />
              {/key}
              {#if selection && selection.anchor && !diffTarget && applyButtons.length > 0}
                <!-- Floating popover anchored at the right edge of the
                     last selected line — same place Cursor/Copilot
                     drop their inline action chips, so the action
                     reads as "do this with the highlighted block".
                     `position: fixed` makes the coordinates we get
                     from CodeMirror's `coordsAtPos` (viewport-relative)
                     drop in directly without any rect math. The
                     anchor recomputes on scroll/resize via the
                     editor's `geometryChanged` signal, so the chip
                     follows the selection through scroll instead of
                     drifting off into space. `mousedown.preventDefault`
                     on each button keeps focus on CodeMirror, so the
                     native selection rectangle stays visible while
                     the user clicks an "Apply to" affordance. -->
                <div
                  class="ev-apply-pop"
                  style:left="{selection.anchor.x}px"
                  style:top="{selection.anchor.y}px"
                  role="toolbar"
                  aria-label="Apply selection to agent"
                >
                  {#each applyButtons as btn (btn.sessionId)}
                    <button
                      class="ev-apply-pop-btn"
                      class:claude={btn.kind === 'claude'}
                      class:cursor={btn.kind === 'cursor'}
                      onmousedown={(e) => e.preventDefault()}
                      onclick={() => handleApplyTo(btn)}
                      title={`Pin @${relToRepo(activePath)}:${selectionRangeText()} to ${btn.label}'s composer`}
                    >
                      <svg class="i i-sm" viewBox="0 0 24 24" aria-hidden="true">
                        <path d="M5 12h12M13 6l6 6-6 6" />
                      </svg>
                      <span>Apply to {btn.label}</span>
                    </button>
                  {/each}
                </div>
              {/if}
            {/if}
          </div>
        </main>
      {/snippet}
    </Splitter>
  {/if}
  {#if error}<div class="ev-error">{error}</div>{/if}
</div>

<style>
  .ev { position: relative; display: flex; height: 100%; min-height: 0; flex: 1; background: var(--bg-0); }

  .ev-empty { flex: 1; display: flex; align-items: center; justify-content: center; padding: 40px; }
  .ev-empty-card { max-width: 440px; text-align: center; }
  .ev-empty-title { font-size: 18px; margin: 0 0 10px; color: var(--text-0); }
  .ev-empty-sub { font-size: 13px; color: var(--text-1); margin: 0 0 24px; line-height: 1.6; }
  .ev-empty-cta {
    padding: 9px 22px;
    border-radius: 8px;
    background: var(--accent);
    color: var(--accent-fg);
    font-size: 13px; font-weight: 600;
  }
  .ev-empty-cta:hover { background: var(--accent-bright); }

  .ev-left {
    display: flex; flex-direction: column;
    height: 100%; min-height: 0;
    background: var(--bg-1);
    border-right: 1px solid var(--border-neutral);
  }
  .ev-left-head {
    display: flex; align-items: center; gap: 8px;
    row-gap: 6px;
    flex-wrap: wrap;
    padding: 10px 12px;
    border-bottom: 1px solid var(--border-neutral);
    background: var(--bg-2);
    flex-shrink: 0;
  }
  /* Root name claims the first row on its own when buttons would overflow —
     `min-width: 0` + `flex: 1 0 100%` on narrow columns makes the row
     wrap icons below instead of squishing the name. */
  .ev-root-name {
    flex: 1 1 120px; min-width: 0;
    font-size: 12.5px; color: var(--text-0); font-weight: 600;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .ev-icon-btn {
    width: 24px; height: 24px; border-radius: 4px;
    display: inline-flex; align-items: center; justify-content: center;
    color: var(--text-1);
  }
  .ev-icon-btn:hover { background: var(--bg-3); color: var(--text-0); }

  .ev-link-wrap { position: relative; display: inline-flex; }

  /* `.linked-pill` (+ `.linked-pill--compact`) styles live in app.css —
     shared with AgentColumn so the chat and editor render the same shape. */

  .ev-link-menu {
    position: absolute; top: calc(100% + 4px); right: 0;
    min-width: 220px;
    background: var(--bg-2);
    border: 1px solid var(--border-hi);
    border-radius: 8px;
    box-shadow: 0 10px 30px rgba(0, 0, 0, 0.5);
    z-index: 20;
    padding: 4px;
    display: flex; flex-direction: column; gap: 2px;
  }
  .ev-link-menu-head {
    font-size: 10.5px; color: var(--text-2);
    padding: 8px 10px 6px;
    border-bottom: 1px solid var(--border-neutral);
    margin-bottom: 4px;
  }
  .ev-link-menu-item {
    display: flex; align-items: center; gap: 8px;
    padding: 7px 10px;
    border-radius: 5px;
    font-size: 12.5px; color: var(--text-1);
    text-align: left;
    transition: all 100ms;
    cursor: pointer;
  }
  .ev-link-menu-item:hover { background: var(--bg-3); color: var(--text-0); }
  .ev-link-menu-kind {
    font-size: 10px; font-weight: 600; text-transform: uppercase;
    padding: 2px 6px; border-radius: 3px;
    background: var(--accent-soft); color: var(--accent-bright);
    border: 1px solid rgba(232, 163, 58, 0.22);
  }
  .ev-link-menu-name { font-size: 11.5px; color: var(--text-2); }

  /* Sidebar body fills the remaining vertical space — tabs sit pinned
     at the bottom under it so the active pane gets the maximum room. */
  .ev-sidebar-body { flex: 1; min-height: 0; display: flex; flex-direction: column; }
  .ev-sidebar-tabs {
    display: flex; align-items: stretch;
    border-top: 1px solid var(--border-neutral);
    background: var(--bg-2);
    flex-shrink: 0;
  }
  .ev-sidebar-tab {
    flex: 1;
    display: inline-flex; align-items: center; justify-content: center; gap: 6px;
    padding: 7px 8px;
    font-size: 11px; font-weight: 500;
    color: var(--text-2);
    background: transparent; border: none; cursor: pointer;
    border-top: 2px solid transparent;
    transition: all 120ms;
  }
  .ev-sidebar-tab:hover { color: var(--text-0); background: var(--bg-1); }
  .ev-sidebar-tab.active {
    color: var(--accent-bright);
    background: var(--bg-1);
    border-top-color: var(--accent);
  }
  .ev-sidebar-tab :global(svg) { width: 12px; height: 12px; }

  .ev-main { flex: 1; display: flex; flex-direction: column; min-width: 0; height: 100%; min-height: 0; }
  .ev-tabbar {
    display: flex; align-items: stretch; gap: 1px;
    padding: 0;
    min-height: 32px; max-height: 32px;
    border-bottom: 1px solid var(--border-neutral);
    background: var(--bg-1);
    overflow-x: auto;
    flex-shrink: 0;
  }
  .ev-tabbar::-webkit-scrollbar { height: 0; }
  .ev-tab-empty {
    padding: 8px 14px;
    font-size: 12px; color: var(--text-2); font-style: italic;
    white-space: nowrap;
  }
  .ev-tab-wrap {
    display: inline-flex; align-items: stretch; gap: 0;
    height: 100%;
    background: var(--bg-1);
    border-right: 1px solid var(--border-neutral);
    flex-shrink: 0;
    max-width: 260px;
    padding-right: 6px;
  }
  .ev-tab-wrap:hover { background: var(--bg-2); }
  .ev-tab-wrap.active {
    background: var(--bg-0);
    box-shadow: inset 0 2px 0 var(--accent);
  }
  .ev-tab-btn {
    display: inline-flex; align-items: center; gap: 6px;
    padding: 0 8px 0 12px;
    font-size: 12px; color: var(--text-1);
    min-width: 0;
  }
  .ev-tab-wrap.active .ev-tab-btn { color: var(--text-0); }
  .ev-tab-name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .ev-tab-x {
    display: inline-flex; align-items: center; justify-content: center;
    width: 16px; height: 16px; border-radius: 3px;
    color: var(--text-2);
    align-self: center;
    flex-shrink: 0;
  }
  .ev-tab-x:hover { background: var(--bg-3); color: var(--text-0); }
  .ev-tab-x :global(svg) { width: 10px; height: 10px; }
  .ev-tab-dot { width: 6px; height: 6px; border-radius: 50%; background: var(--accent-bright); }
  .ev-tab-diff-icon {
    color: var(--accent-bright); font-weight: 700;
    width: 14px; text-align: center;
    flex-shrink: 0;
  }
  .ev-tab-side {
    font-size: 10px; padding: 1px 5px;
    border-radius: 3px; background: var(--bg-3);
    color: var(--text-2);
    flex-shrink: 0;
  }

  .ev-editor-wrap { flex: 1; min-height: 0; position: relative; }

  /* Floating "Apply to <agent>" popover, anchored to the right end
     of the last selected line via fixed-position viewport
     coordinates from `coordsAtPos`. A small `translate` offset puts
     the chip just below + slightly past the right edge of the
     highlight so it doesn't overlap the selection or the next
     line's text. `pointer-events: auto` is implicit (default) so
     the chip is clickable; the empty space around it is
     `pointer-events: none` only because there's nothing else there
     — we don't wrap the chip in a transparent overlay that would
     intercept editor clicks. */
  .ev-apply-pop {
    position: fixed;
    z-index: 1000;
    transform: translate(8px, 6px);
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 3px;
    background: var(--bg-2);
    border: 1px solid var(--border-hi);
    border-radius: 7px;
    box-shadow: 0 6px 20px -6px rgba(0, 0, 0, 0.55), 0 1px 0 0 rgba(0, 0, 0, 0.1);
    white-space: nowrap;
  }
  .ev-apply-pop-btn {
    display: inline-flex; align-items: center; gap: 6px;
    padding: 4px 10px;
    border-radius: 5px;
    background: transparent;
    border: 1px solid transparent;
    color: var(--text-0);
    font-size: 12px; font-weight: 500;
    cursor: pointer;
    transition: background 100ms, border-color 100ms, color 100ms;
  }
  .ev-apply-pop-btn:hover {
    background: var(--accent-soft);
    border-color: var(--accent);
  }
  .ev-apply-pop-btn :global(svg) {
    width: 12px; height: 12px; opacity: 0.85;
  }
  /* Same family-colour accent we use elsewhere — claude == orange
     accent, cursor == subdued neutral — so the user can scan
     "which agent does this go to" without reading the label. */
  .ev-apply-pop-btn.claude { border-left: 2px solid var(--accent); padding-left: 8px; }
  .ev-apply-pop-btn.cursor { border-left: 2px solid var(--text-1); padding-left: 8px; }

  .ev-error {
    position: absolute;
    bottom: 10px; left: 50%; transform: translateX(-50%);
    padding: 8px 14px;
    background: rgba(214, 72, 44, 0.16);
    color: var(--error);
    border: 1px solid rgba(214, 72, 44, 0.3);
    border-radius: 6px;
    font-size: 12px;
  }
</style>
