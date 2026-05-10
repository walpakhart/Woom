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

  /* Storage keys are computed per `instanceId` so every editor
     instance (Vermeer, Hokusai, …) gets its own tab list and root.
     Without the suffix, opening file X in Vermeer would also show it
     in Hokusai's tabs because both wrote to `woom:editor:tabs`. */
  function rootKey(id: string): string {
    return `woom:editor:root:${id}`;
  }
  function tabsKey(id: string): string {
    return `woom:editor:tabs:${id}`;
  }

  /* Sidebar mode is now driven from the parent's ActivityBar, not from
     a tab strip at the bottom of the explorer (v7). Six tabs total —
     `explorer` is the file tree, `git` is the staging / history pane,
     and the remaining three (`search`, `debug`, `tests`) render their
     own focused panes inside the same sidebar slot. The parent passes
     the active tab in via `sidebarTab` and we keep our own fallback
     when running outside an EditorApp shell. */
  type SidebarTab = 'explorer' | 'search' | 'git' | 'debug' | 'tests';

  /* Bumped after every commit / push / pull / branch switch so the
     HistoryPanel inside the Git tab re-fetches automatically. */
  let gitChangeCount = $state(0);

  interface Props {
    /** Two-way bound to the parent so Claude sessions can pick up the repo
        as their default cwd. */
    repoPath?: string;
    /** Pickable rows for the link dropdown — one per Claude/Cursor
        session (so the user knows exactly which chat will get linked).
        `name` is the session title, `id` is the agent column instance,
        `sessionId` is the specific session to activate before linking
        (omitted only when the agent has no sessions yet — click then
        spawns a fresh chat in that column). */
    agentInstances?: { id: string; kind: 'claude' | 'cursor'; name: string; sessionId?: string }[];
    /** Sessions currently linked TO this editor — rendered as chips in the
        header so the link is visible from the editor side too (matches the
        "Linked to Editor" pill on the AI column). */
    linkedAgents?: { sessionId: string; agentInstanceId: string; kind: 'claude' | 'cursor'; name: string }[];
    /** Invoked when the user picks an AI session to link this editor to.
        The parent activates the chosen session in its column and flags
        it linked. When no `sessionId` is passed (agent column was
        empty) the parent spawns a fresh chat instead. */
    onLinkToAgent?: (agentInstanceId: string, sessionId?: string) => void;
    /** Break the link for a specific session. Called from the X on each
        "Linked to" chip. */
    onUnlinkAgent?: (sessionId: string) => void;
    /** Driven from the parent's ActivityBar — controls which pane the
        sidebar shows. Default is `explorer` for legacy callers. */
    sidebarTab?: SidebarTab;
    /** Curated instance name (e.g. "Vermeer") — rendered as a small
        italic-serif label above the repo name so users always know
        which editor instance they're inside. Falls back to nothing
        when the parent doesn't pass one. */
    instanceLabel?: string;
    /** Editor instance id — used to scope the tab list / root path
        cache so two open editors don't share state. Required for
        multi-instance correctness; legacy callers can pass `default`. */
    instanceId?: string;
  }
  let {
    repoPath = $bindable(''),
    agentInstances = [],
    linkedAgents = [],
    onLinkToAgent,
    onUnlinkAgent,
    sidebarTab = 'explorer',
    instanceLabel,
    instanceId = 'default'
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

  /** Cursor-info readout for the status bar (line / col / line endings /
   *  byte count). Fed by Editor's `onCursorChange` callback; reset to
   *  null on file swap so the bar reads "—" until the new buffer's
   *  first selection event fires. */
  let cursorInfo = $state<{ line: number; col: number; lineEndings: 'lf' | 'crlf'; bytes: number } | null>(null);
  $effect(() => {
    activePath;
    diffTarget;
    cursorInfo = null;
  });

  /** Live git branch — used in the status bar's right edge. Updated by
   *  the GitPanel hook below; stays empty until the first
   *  `git_status` invoke succeeds. */
  let gitBranch = $state<string>('');

  // ---- File-name search ---------------------------------------------------
  let searchQuery = $state('');
  let searchResults = $state<{ path: string; name: string; rel: string }[]>([]);
  let searching = $state(false);
  let searchTimer: ReturnType<typeof setTimeout> | null = null;

  async function searchFiles(
    root: string,
    query: string
  ): Promise<{ path: string; name: string; rel: string }[]> {
    const q = query.toLowerCase();
    const results: { path: string; name: string; rel: string }[] = [];
    const queue: string[] = [root];
    while (queue.length > 0 && results.length < 80) {
      const dir = queue.shift()!;
      let entries: { name: string; path: string; is_dir: boolean }[] = [];
      try {
        entries = await invoke('fs_list_dir', { path: dir });
      } catch { continue; }
      for (const e of entries) {
        if (e.is_dir) {
          queue.push(e.path);
        } else {
          if (e.name.toLowerCase().includes(q)) {
            const rel = e.path.startsWith(root + '/') ? e.path.slice(root.length + 1) : e.path;
            results.push({ path: e.path, name: e.name, rel });
            if (results.length >= 80) break;
          }
        }
      }
    }
    return results;
  }

  $effect(() => {
    const q = searchQuery.trim();
    if (searchTimer) { clearTimeout(searchTimer); searchTimer = null; }
    if (q.length < 2) { searchResults = []; searching = false; return; }
    searching = true;
    searchTimer = setTimeout(async () => {
      if (!repoPath) { searching = false; return; }
      searchResults = await searchFiles(repoPath, q);
      searching = false;
    }, 280);
  });

  /** Map a file extension to a friendly language label for the status
   *  bar. Falls back to "Plain text" — keeping the bar honest about
   *  what CodeMirror can't syntax-highlight rather than guessing. */
  function languageLabel(p: string): string {
    if (!p) return 'Plain text';
    const dot = p.lastIndexOf('.');
    if (dot < 0) return 'Plain text';
    const ext = p.slice(dot + 1).toLowerCase();
    const map: Record<string, string> = {
      ts: 'TypeScript', tsx: 'TSX', js: 'JavaScript', jsx: 'JSX',
      svelte: 'Svelte', vue: 'Vue', html: 'HTML', css: 'CSS', scss: 'SCSS',
      json: 'JSON', md: 'Markdown', yaml: 'YAML', yml: 'YAML', toml: 'TOML',
      rs: 'Rust', go: 'Go', py: 'Python', rb: 'Ruby', java: 'Java',
      c: 'C', h: 'C', cc: 'C++', cpp: 'C++', hpp: 'C++',
      sh: 'Shell', bash: 'Shell', zsh: 'Shell', sql: 'SQL', php: 'PHP',
      lock: 'Lockfile'
    };
    return map[ext] ?? ext.toUpperCase();
  }

  /** Format a byte count compactly. The status bar can show this to
   *  remind the user how big the buffer is (1.4 KB, 124 KB, …). */
  function fmtBytes(n: number): string {
    if (n < 1024) return `${n} B`;
    if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
    return `${(n / (1024 * 1024)).toFixed(1)} MB`;
  }

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
    /* Drop the selection so the floating "Apply to …" popover dismisses
       itself. The token is now in the composer; staying selected would
       just leave the user staring at a stale popover until they click
       elsewhere. */
    selection = null;
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
  // True after `onDestroy` runs — tells in-flight async paths to bail
  // before writing to parent state. The `git_status` invoke can take
  // hundreds of ms; if the editor column is removed mid-call, the
  // promise still resolves and would otherwise call
  // `onGitStatusChange(...)` on a parent that's no longer interested.
  let destroyed = false;

  /** Authoritative git-status refresh. Guarded against overlapping calls —
      if one is in flight we just skip (the next scheduleGitStatus will catch
      up). Called from: save hook, fs watcher (debounced), branch switch,
      polling timer. */
  async function refreshGitStatus() {
    if (!repoPath || statusInFlight || destroyed) return;
    statusInFlight = true;
    try {
      const s = await invoke<GitStatusPayload>('git_status', { repo: repoPath });
      // Destroy could have landed during the await above. Stop here
      // so we don't invoke the parent callback with stale data.
      if (destroyed) return;
      onGitStatusChange(s.files);
      gitBranch = s.branch ?? '';
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
    let rootToLoad = repoPath || localStorage.getItem(rootKey(instanceId)) || '';
    if (rootToLoad) {
      try {
        const exists = await invoke<boolean>('fs_path_exists', { path: rootToLoad });
        if (exists) {
          if (!repoPath) await setRoot(rootToLoad);
          else await startWatch();
          const savedTabs = JSON.parse(localStorage.getItem(tabsKey(instanceId)) || '[]');
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
    destroyed = true;
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
    localStorage.setItem(rootKey(instanceId), repoPath);
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

  /** Add `path` as a tab and activate it. Exported so EditorView
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
    localStorage.setItem(tabsKey(instanceId), JSON.stringify(tabs));
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
        <p class="ev-empty-sub">Pick a folder — Woom detects the git root and gives you the tree, editor, and git controls.</p>
        <button class="ev-empty-cta" onclick={pickFolder}>Open folder…</button>
      </div>
    </section>
  {:else}
    <Splitter direction="horizontal" persistKey="editor-main" initial={300} min={180} max={520}>
      {#snippet start()}
        <aside class="ev-left">
          <!-- Top row: editorial repo name + actions. The "Chat 1 / Chat 2"
               linked-session pills that used to live here now sit in their
               own subtle row below — keeping the head uncluttered like the
               v7 mockup. -->
          <div class="ev-left-head">
            <div class="ev-root-stack">
              {#if instanceLabel}
                <span class="ev-instance-label" title="Editor instance · {instanceLabel}">{instanceLabel}</span>
              {/if}
              <span class="ev-root-name" title={repoPath}>{fileName(repoPath)}</span>
            </div>
            {#if onLinkToAgent && agentInstances.length > 0}
              <div class="ev-link-wrap">
                <button
                  class="ev-icon-btn"
                  class:has-links={linkedAgents.length > 0}
                  onclick={() => {
                    if (agentInstances.length === 1) {
                      onLinkToAgent?.(agentInstances[0].id, agentInstances[0].sessionId);
                    } else {
                      showLinkPicker = !showLinkPicker;
                    }
                  }}
                  title={linkedAgents.length > 0
                    ? `${linkedAgents.length} chat${linkedAgents.length === 1 ? '' : 's'} linked — click to add another`
                    : 'Link an AI chat to this folder'}
                  aria-label="Link to chat"
                >
                  <svg class="i i-sm" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7"><path d="M9 17H7A5 5 0 1 1 7 7h2M15 7h2a5 5 0 1 1 0 10h-2M8 12h8"/></svg>
                  {#if linkedAgents.length > 0}
                    <span class="ev-link-badge">{linkedAgents.length}</span>
                  {/if}
                </button>
                {#if showLinkPicker && agentInstances.length > 1}
                  <div class="ev-link-menu" role="menu">
                    <div class="ev-link-menu-head">Link this folder to…</div>
                    {#each agentInstances as a (a.sessionId ?? a.id + ':empty')}
                      <button
                        class="ev-link-menu-item"
                        role="menuitem"
                        onclick={() => { onLinkToAgent?.(a.id, a.sessionId); showLinkPicker = false; }}
                      >
                        <span class="ev-link-menu-kind" data-kind={a.kind}>{a.kind === 'claude' ? 'Claude' : 'Cursor'}</span>
                        <span class="ev-link-menu-name">{a.name}</span>
                      </button>
                    {/each}
                  </div>
                {/if}
              </div>
            {/if}
            <button class="ev-icon-btn" onclick={pickFolder} title="Open another folder">
              <svg class="i i-sm" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7"><path d="M3 7v11a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2V9a2 2 0 0 0-2-2h-7L10 5H5a2 2 0 0 0-2 2z" /></svg>
            </button>
          </div>

          <!-- Quiet "linked apps" row — only renders when something IS
               linked. Single row, brand dot per agent, hover-only ×.
               Modeled on the v7 worktree-side "Linked apps" pattern. -->
          {#if linkedAgents.length > 0}
            <div class="ev-linked-row">
              {#each linkedAgents as la (la.sessionId)}
                <span
                  class="ev-linked-chip"
                  data-kind={la.kind}
                  title="Linked to {la.kind === 'claude' ? 'Claude' : 'Cursor'} · {la.name}"
                >
                  <span class="ev-linked-dot"></span>
                  <span class="ev-linked-name mono">{la.name}</span>
                  {#if onUnlinkAgent}
                    <button
                      class="ev-linked-x"
                      onclick={() => onUnlinkAgent?.(la.sessionId)}
                      title="Unlink"
                      aria-label="Unlink"
                    >
                      <svg class="i i-sm" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7"><path d="M6 6l12 12M6 18L18 6"/></svg>
                    </button>
                  {/if}
                </span>
              {/each}
            </div>
          {/if}
          <!-- Sidebar pane label — small caption matching the active
               activity tab so users get a heading for the panel without
               needing the bottom tab strip. -->
          <div class="ev-sidebar-label">
            {#if sidebarTab === 'explorer'}Explorer
            {:else if sidebarTab === 'search'}Search
            {:else if sidebarTab === 'git'}Source control
            {:else if sidebarTab === 'debug'}Debug
            {:else if sidebarTab === 'tests'}Tests{/if}
          </div>

          <!-- Sidebar body: one of five panels picked by the activity bar. -->
          <div class="ev-sidebar-body">
            {#if sidebarTab === 'explorer'}
              <FileTree
                rootPath={repoPath}
                selectedPath={diffTarget ? `${repoPath}/${diffTarget.path}` : activePath}
                onSelect={openFile}
                {gitStatusByPath}
              />
            {:else if sidebarTab === 'git'}
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
            {:else if sidebarTab === 'search'}
              <div class="ev-sidebar-pane">
                <div class="ev-search-bar">
                  <input
                    class="ev-search-input mono"
                    placeholder="Search files by name…"
                    type="search"
                    bind:value={searchQuery}
                  />
                </div>
                {#if searchQuery.trim().length < 2}
                  <div class="ev-sidebar-empty">
                    <p class="ev-sidebar-empty-h serif">Find files</p>
                    <p class="ev-sidebar-empty-p">Type 2+ characters to search filenames in <span class="mono">{repoPath ? fileName(repoPath) : 'this repo'}</span>.</p>
                  </div>
                {:else if searching}
                  <div class="ev-sidebar-empty">
                    <p class="ev-sidebar-empty-p">Searching…</p>
                  </div>
                {:else if searchResults.length === 0}
                  <div class="ev-sidebar-empty">
                    <p class="ev-sidebar-empty-p">No files found for <span class="mono">"{searchQuery}"</span></p>
                  </div>
                {:else}
                  <div class="ev-search-results">
                    {#each searchResults as r (r.path)}
                      <button
                        class="ev-search-result"
                        onclick={() => openFile(r.path)}
                        title={r.path}
                      >
                        <span class="ev-search-result-name mono">{r.name}</span>
                        <span class="ev-search-result-dir mono">{r.rel.slice(0, r.rel.length - r.name.length - 1) || '/'}</span>
                      </button>
                    {/each}
                  </div>
                {/if}
              </div>
            {:else if sidebarTab === 'debug'}
              <div class="ev-sidebar-pane">
                <div class="ev-sidebar-empty">
                  <div class="ev-sidebar-empty-icon">
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6"><circle cx="12" cy="13" r="6"/><path d="M12 7v-3M9 4h6M5 11l-2 1M19 11l2 1M5 17l-2 1M19 17l2 1"/></svg>
                  </div>
                  <p class="ev-sidebar-empty-h serif">No debug session</p>
                  <p class="ev-sidebar-empty-p">Pick a launch config from <span class="mono">.vscode/launch.json</span> to start debugging. Breakpoints set in the editor will land here.</p>
                </div>
              </div>
            {:else if sidebarTab === 'tests'}
              <div class="ev-sidebar-pane">
                <div class="ev-sidebar-empty">
                  <div class="ev-sidebar-empty-icon">
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6"><polyline points="9 11 12 14 22 4"/><path d="M21 12v7a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11"/></svg>
                  </div>
                  <p class="ev-sidebar-empty-h serif">No test runner detected</p>
                  <p class="ev-sidebar-empty-p">Hand <span class="mono">pnpm test</span> to the terminal app, or ask Claude to run the suite for the current change.</p>
                </div>
              </div>
            {/if}
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
                  onCursorChange={(info) => (cursorInfo = info)}
                />
              {/key}
              <!-- The "Apply to <agent>" floating popover sits ABOVE
                   the status bar in z-order; the status bar lives
                   inside `.ev-editor-wrap` so it's anchored to the
                   bottom of the right pane regardless of how the
                   user resizes the splitter or toggles the bottom
                   problems panel. -->
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

          <!-- Status bar: language · cursor position · encoding · line
               endings   ✓ no problems · git branch. Same shape as the
               v8 mockup; wraps as a single horizontal strip pinned to
               the bottom of the editor pane. Hidden when nothing is
               open (empty state has its own card). -->
          {#if activePath || diffTarget}
            <div class="ev-statusbar mono">
              <span class="ev-status-seg">{languageLabel(diffTarget?.path ?? activePath)}</span>
              <span class="ev-status-sep">·</span>
              {#if cursorInfo}
                <span class="ev-status-seg">Ln {cursorInfo.line}, Col {cursorInfo.col}</span>
                <span class="ev-status-sep">·</span>
                <span class="ev-status-seg">UTF-8</span>
                <span class="ev-status-sep">·</span>
                <span class="ev-status-seg">{cursorInfo.lineEndings.toUpperCase()}</span>
                <span class="ev-status-sep">·</span>
                <span class="ev-status-seg">{fmtBytes(cursorInfo.bytes)}</span>
              {:else}
                <span class="ev-status-seg ev-status-dim">UTF-8</span>
              {/if}
              <span class="ev-status-spacer"></span>
              <span class="ev-status-seg ev-status-ok" title="No diagnostics">✓ no problems</span>
              <span class="ev-status-sep">·</span>
              <span class="ev-status-seg ev-status-branch" title="Current git branch">
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="6" cy="6" r="2.5"/><circle cx="18" cy="18" r="2.5"/><path d="M6 8.5V14a4 4 0 0 0 4 4h6"/></svg>
                {gitBranch || '—'}
              </span>
            </div>
          {/if}
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
    display: flex; align-items: center; gap: 6px;
    padding: 14px 16px 12px;
    border-bottom: 1px solid var(--border);
    background: linear-gradient(180deg, var(--bg-2), var(--bg-1));
    flex-shrink: 0;
  }
  /* Two-line head stack: small italic-serif instance mark above the
     repo name. Lets users see which Vermeer / Rothko / Hokusai
     instance they're inside without having to open the rail menu. */
  .ev-root-stack {
    flex: 1 1 0; min-width: 0;
    display: flex; flex-direction: column;
    gap: 1px;
    overflow: hidden;
  }
  .ev-instance-label {
    font-family: 'Geist', 'Inter', -apple-system, system-ui, sans-serif;
    font-size: 11px;
    
    line-height: 1;
    letter-spacing: 0.02em;
    color: var(--src-editor);
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  /* v7 — repo name reads as a small editorial heading. */
  .ev-root-name {
    min-width: 0;
    font-family: 'Geist', 'Inter', -apple-system, system-ui, sans-serif;
    font-size: 18px; font-weight: 600;
    letter-spacing: -0.01em;
    color: var(--text-0);
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .ev-icon-btn {
    position: relative;
    width: 26px; height: 26px; border-radius: 6px;
    display: inline-flex; align-items: center; justify-content: center;
    color: var(--text-2);
    background: transparent;
    border: 0;
    cursor: pointer;
    transition: background 120ms, color 120ms;
  }
  .ev-icon-btn:hover { background: var(--bg-3); color: var(--text-0); }
  .ev-icon-btn.has-links { color: var(--accent-bright); }
  .ev-link-badge {
    position: absolute;
    top: -2px; right: -2px;
    min-width: 12px; height: 12px;
    padding: 0 3px;
    border-radius: 7px;
    background: var(--accent);
    color: var(--accent-fg);
    font-family: 'Inter Tight', system-ui, sans-serif;
    font-size: 8.5px; font-weight: 700;
    display: inline-flex; align-items: center; justify-content: center;
    box-shadow: 0 0 0 2px var(--bg-2);
  }

  .ev-link-wrap { position: relative; display: inline-flex; }

  /* "Linked apps" row — one quiet line under the head. Brand dot per
     kind, mono session label, hover-only × to unlink. */
  .ev-linked-row {
    display: flex; flex-wrap: wrap; gap: 4px;
    padding: 6px 14px 8px;
    border-bottom: 1px solid var(--border);
    background: var(--bg-1);
  }
  .ev-linked-chip {
    display: inline-flex; align-items: center; gap: 6px;
    padding: 3px 4px 3px 7px;
    background: var(--bg-2);
    border: 1px solid var(--border);
    border-radius: 6px;
    font-size: 10.5px;
    color: var(--text-1);
    max-width: 160px;
  }
  .ev-linked-chip[data-kind="claude"] {
    border-color: color-mix(in srgb, var(--src-claude) 28%, var(--border));
  }
  .ev-linked-chip[data-kind="cursor"] {
    border-color: color-mix(in srgb, var(--src-cursor) 22%, var(--border));
  }
  .ev-linked-dot {
    width: 6px; height: 6px; border-radius: 50%;
    background: var(--src-claude);
    box-shadow: 0 0 6px color-mix(in srgb, var(--src-claude) 60%, transparent);
    flex-shrink: 0;
  }
  .ev-linked-chip[data-kind="cursor"] .ev-linked-dot {
    background: var(--src-cursor);
    box-shadow: 0 0 6px color-mix(in srgb, var(--src-cursor) 50%, transparent);
  }
  .ev-linked-name {
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
    font-size: 10.5px;
  }
  .ev-linked-x {
    width: 14px; height: 14px;
    display: inline-grid; place-items: center;
    border-radius: 3px;
    background: transparent; border: 0;
    color: var(--text-mute);
    opacity: 0;
    cursor: pointer;
    padding: 0;
    transition: opacity 100ms, color 100ms, background 100ms;
  }
  .ev-linked-chip:hover .ev-linked-x { opacity: 1; }
  .ev-linked-x:hover { color: var(--error); background: var(--bg-3); }
  .ev-linked-x svg { width: 10px; height: 10px; }

  .ev-link-menu {
    position: absolute; top: calc(100% + 6px); right: 0;
    min-width: 280px; max-width: 360px;
    background: var(--bg-1);
    border: 1px solid var(--border-hi);
    border-radius: 11px;
    box-shadow: var(--shadow-3);
    z-index: 20;
    padding: 6px;
    display: flex; flex-direction: column; gap: 1px;
  }
  .ev-link-menu-head {
    font-size: 9.5px; font-weight: 700;
    color: var(--text-mute);
    text-transform: uppercase; letter-spacing: 0.10em;
    padding: 8px 10px 6px;
    border-bottom: 1px solid var(--border);
    margin-bottom: 4px;
  }
  .ev-link-menu-item {
    display: flex; align-items: center; gap: 9px;
    padding: 8px 10px;
    border-radius: 7px;
    font-size: 12.5px; color: var(--text-1);
    text-align: left;
    transition: background 100ms, color 100ms;
    cursor: pointer;
    background: transparent;
    border: 0;
    width: 100%;
  }
  .ev-link-menu-item:hover { background: var(--bg-2); color: var(--text-0); }
  .ev-link-menu-kind {
    flex-shrink: 0;
    font-family: 'JetBrains Mono', monospace;
    font-size: 9.5px; font-weight: 700;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    padding: 2px 7px;
    border-radius: 4px;
    background: color-mix(in srgb, var(--src-claude) 14%, var(--bg-3));
    color: var(--src-claude);
    border: 1px solid color-mix(in srgb, var(--src-claude) 28%, transparent);
  }
  .ev-link-menu-kind[data-kind="cursor"] {
    background: color-mix(in srgb, var(--src-cursor) 12%, var(--bg-3));
    color: var(--src-cursor);
    border-color: color-mix(in srgb, var(--src-cursor) 22%, transparent);
  }
  .ev-link-menu-name {
    flex: 1; min-width: 0;
    font-size: 12.5px; color: var(--text-0);
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }

  /* Sidebar body fills the remaining vertical space — tabs sit pinned
     at the bottom under it so the active pane gets the maximum room. */
  .ev-sidebar-body { flex: 1; min-height: 0; min-width: 0; display: flex; flex-direction: column; overflow-x: hidden; }
  /* Belt-and-braces: any descendant scroll container that ends up
     showing a horizontal track (FileTree / GitPanel / HistoryPanel —
     all of which already clip+ellipsis their content) gets its
     horizontal bar hidden. Without this, narrow column widths
     produced a thin horizontal scrollbar wedged between the list
     and the bottom tab strip that read as visual noise. Vertical
     scrollbars stay intact. */
  .ev-sidebar-body :global(*) { scrollbar-width: thin; }
  .ev-sidebar-body :global(*::-webkit-scrollbar:horizontal) { height: 0; display: none; }

  /* Active-pane label — small uppercase caption above the body, in
     place of the old VSCode-style bottom tab strip. The activity bar
     on the left now drives which pane shows here. */
  .ev-sidebar-label {
    flex: 0 0 auto;
    padding: 8px 16px 6px;
    font-size: 9.5px; font-weight: 700;
    letter-spacing: 0.10em; text-transform: uppercase;
    color: var(--text-mute);
    background: var(--bg-1);
    border-bottom: 1px solid var(--border);
  }

  /* Generic pane shell for the search / debug / tests panels — they
     share an editorial empty state with the same shape as
     `.app-empty-card` from the chassis but inline. */
  .ev-sidebar-pane {
    flex: 1; min-height: 0;
    display: flex; flex-direction: column;
    padding: 14px;
    gap: 12px;
    overflow-y: auto;
  }
  .ev-search-input {
    width: 100%;
    padding: 8px 10px;
    background: var(--bg-2);
    border: 1px solid var(--border);
    border-radius: 8px;
    color: var(--text-0);
    font-size: 12px;
    transition: border-color 140ms, box-shadow 140ms;
  }
  .ev-search-input:focus {
    outline: none;
    border-color: var(--border-accent);
    box-shadow: 0 0 0 3px var(--accent-soft);
  }
  .ev-sidebar-empty {
    margin: auto;
    text-align: center;
    padding: 30px 16px;
  }
  .ev-sidebar-empty-icon {
    width: 44px; height: 44px;
    margin: 0 auto 12px;
    display: grid; place-items: center;
    border-radius: 12px;
    background: var(--bg-2);
    color: var(--accent-bright);
    box-shadow: inset 0 0 0 1px var(--border);
  }
  .ev-sidebar-empty-icon svg { width: 20px; height: 20px; }
  .ev-sidebar-empty-h {
    font-family: 'Geist', 'Inter', -apple-system, system-ui, sans-serif;
    font-size: 18px; font-weight: 600; letter-spacing: -0.01em;
    color: var(--text-0);
    margin: 0 0 8px;
  }
  .ev-sidebar-empty-p {
    font-size: 11.5px; color: var(--text-2);
    line-height: 1.5; margin: 0;
  }
  .ev-sidebar-empty-p .mono {
    font-family: 'JetBrains Mono', monospace;
    font-size: 10.5px;
    padding: 1px 5px;
    background: var(--bg-2); border: 1px solid var(--border);
    border-radius: 4px;
    color: var(--accent-bright);
  }

  .ev-search-bar { flex-shrink: 0; }
  .ev-search-results {
    flex: 1; min-height: 0;
    display: flex; flex-direction: column; gap: 2px;
    overflow-y: auto;
  }
  .ev-search-result {
    display: flex; flex-direction: column; align-items: flex-start; gap: 2px;
    padding: 6px 10px;
    border-radius: 6px;
    background: transparent;
    border: none;
    cursor: pointer;
    text-align: left;
    transition: background 120ms;
    width: 100%;
  }
  .ev-search-result:hover { background: var(--bg-2); }
  .ev-search-result-name {
    font-size: 12px;
    color: var(--text-0);
    font-weight: 500;
  }
  .ev-search-result-dir {
    font-size: 10.5px;
    color: var(--text-mute);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 100%;
  }

  .ev-main { flex: 1; display: flex; flex-direction: column; min-width: 0; height: 100%; min-height: 0; }
  /* Hide horizontal scrollbar on the editor content (CodeMirror's
     `.cm-scroller`) and any other descendant that would otherwise
     paint a thin track at the bottom of the right pane. Content stays
     horizontally scrollable via two-finger swipe / shift+scroll —
     this just removes the visible track which read as visual noise
     under the file tabs. Vertical scrollbars are untouched. */
  .ev-main :global(*::-webkit-scrollbar:horizontal) { height: 0; display: none; }
  .ev-main :global(.cm-scroller) { scrollbar-width: none; }
  .ev-main :global(.cm-scroller::-webkit-scrollbar:horizontal) { height: 0; display: none; }
  /* v8 — chip-style tabs floating on the editor surface, with a per-tab
     brand dot indicating dirty/saved state. The bar gets a soft top
     edge that fades into the file content below; no hard border. */
  .ev-tabbar {
    display: flex; align-items: center; gap: 6px;
    padding: 8px 10px 6px;
    min-height: 42px;
    background: var(--bg-1);
    overflow-x: auto;
    flex-shrink: 0;
    border-bottom: 1px solid var(--border);
  }
  .ev-tabbar::-webkit-scrollbar { height: 0; }
  .ev-tab-empty {
    padding: 6px 10px;
    font-size: 12px; color: var(--text-mute); 
    white-space: nowrap;
  }
  .ev-tab-wrap {
    display: inline-flex; align-items: center; gap: 0;
    height: 28px;
    padding: 0 4px 0 10px;
    background: var(--bg-2);
    border: 1px solid var(--border);
    border-radius: 7px;
    flex-shrink: 0;
    max-width: 260px;
    transition: background 120ms, border-color 120ms;
    cursor: pointer;
  }
  .ev-tab-wrap:hover { background: var(--bg-3); border-color: var(--border-hi); }
  .ev-tab-wrap.active {
    background: var(--bg-3);
    border-color: var(--border-hi);
    box-shadow: 0 0 0 1px color-mix(in srgb, var(--accent) 22%, transparent);
  }
  /* Leading brand dot — terracotta on active, muted on inactive,
     amber on dirty unsaved buffer. Matches the screenshot's bullet
     glyph next to the file name. */
  .ev-tab-wrap::before {
    content: '';
    flex-shrink: 0;
    width: 6px; height: 6px;
    border-radius: 50%;
    margin-right: 7px;
    background: var(--text-mute);
    transition: background 140ms, box-shadow 140ms;
  }
  .ev-tab-wrap.active::before {
    background: var(--accent-bright);
    box-shadow: 0 0 6px var(--accent-glow);
  }
  .ev-tab-wrap.dirty::before {
    background: var(--warning);
    box-shadow: 0 0 6px rgba(217, 184, 110, 0.45);
  }
  .ev-tab-btn {
    display: inline-flex; align-items: center; gap: 6px;
    padding: 0;
    font-size: 12.5px; color: var(--text-1);
    background: transparent; border: 0;
    min-width: 0;
    cursor: pointer;
  }
  .ev-tab-wrap.active .ev-tab-btn { color: var(--text-0); }
  .ev-tab-name {
    font-family: 'JetBrains Mono', monospace;
    font-size: 11.5px;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .ev-tab-x {
    display: inline-flex; align-items: center; justify-content: center;
    width: 18px; height: 18px; border-radius: 4px;
    margin-left: 6px;
    color: var(--text-mute);
    background: transparent; border: 0;
    align-self: center;
    flex-shrink: 0;
    cursor: pointer;
    transition: background 100ms, color 100ms;
  }
  .ev-tab-x:hover { background: rgba(232, 130, 100, 0.10); color: var(--error); }
  .ev-tab-x :global(svg) { width: 10px; height: 10px; }
  /* Inline dirty dot inside the close-button slot — only used when
     the buffer is unsaved and the user hasn't hovered the row yet. */
  .ev-tab-dot { width: 6px; height: 6px; border-radius: 50%; background: var(--warning); box-shadow: 0 0 6px rgba(217,184,110,0.4); }
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

  /* Status bar — single horizontal strip pinned to the bottom of
     the editor pane. Mono throughout, brand-dot for the git branch
     readout, mint check for "no problems". */
  .ev-statusbar {
    display: flex; align-items: center; gap: 6px;
    padding: 7px 18px;
    border-top: 1px solid var(--border);
    background: var(--bg-1);
    font-size: 11px;
    color: var(--text-2);
    flex-shrink: 0;
    overflow-x: auto;
    white-space: nowrap;
    scrollbar-width: none;
  }
  .ev-statusbar::-webkit-scrollbar { height: 0; }
  .ev-status-seg {
    display: inline-flex; align-items: center; gap: 5px;
    color: var(--text-1);
  }
  .ev-status-dim { color: var(--text-mute); }
  .ev-status-sep { color: var(--text-mute); opacity: 0.6; }
  .ev-status-spacer { flex: 1; }
  .ev-status-ok { color: var(--success); }
  .ev-status-branch { color: var(--accent-bright); }
  .ev-status-branch :global(svg) {
    width: 11px; height: 11px;
    color: var(--accent-bright);
  }

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
    background: rgba(232, 130, 100, 0.16);
    color: var(--error);
    border: 1px solid rgba(232, 130, 100, 0.3);
    border-radius: 6px;
    font-size: 12px;
  }
</style>
