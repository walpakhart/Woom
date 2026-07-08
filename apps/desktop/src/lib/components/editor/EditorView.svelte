<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import { open as openDialog } from '@tauri-apps/plugin-dialog';
  import Editor from '$lib/components/editor/Editor.svelte';
  import FileTree from '$lib/components/editor/FileTree.svelte';
  import EditorFileSearch from '$lib/components/editor/EditorFileSearch.svelte';
  import EditorTabs from '$lib/components/editor/EditorTabs.svelte';
  import GitPanel from '$lib/components/editor/GitPanel.svelte';
  import HistoryPanel from '$lib/components/editor/HistoryPanel.svelte';
  import DiffView from '$lib/components/editor/DiffView.svelte';
  import ReviewPane from '$lib/components/editor/ReviewPane.svelte';
  import TerminalSurface from '$lib/views/apps/terminal/TerminalSurface.svelte';
  import MarkdownPreview from '$lib/components/editor/MarkdownPreview.svelte';
  import ImagePreview from '$lib/components/editor/ImagePreview.svelte';
  import Splitter from '$lib/components/ui/Splitter.svelte';
  import { notifyError } from '$lib/state/toaster.svelte';
  import { applyRangeToAgent } from '$lib/services/applyToAgent';
  import {
    sessionsState,
    consumeEditorOpenFile,
    getPendingEditEvents,
    updateEditEvent,
    setEditorRoots,
    removeEditorRoot
  } from '$lib/state/sessions.svelte';
  import { revertEditEvent } from '$lib/services/diffActions';

  /* Storage keys are computed per `instanceId` so every editor
     instance (Vermeer, Hokusai, …) gets its own tab list and root.
     Without the suffix, opening file X in Vermeer would also show it
     in Hokusai's tabs because both wrote to `woom:editor:tabs`. */
  function rootKey(id: string): string {
    return `woom:editor:root:${id}`;
  }
  /* Tabs persist PER-REPO (not per-instance) so the editor behaves like
     VS Code: each folder remembers which files were open. Opening repo X
     in any editor restores X's last tabs; a brand-new editor with no repo
     is empty; switching repos swaps the tab set instead of carrying stale
     files from the previous repo. Key = sorted root set joined by '|'. */
  function repoKeyFor(rs: string[]): string {
    return rs.length ? [...rs].sort().join('|') : '';
  }
  function tabsKey(repoKey: string): string {
    return `woom:editor:tabs:${repoKey}`;
  }
  /* The currently-focused buffer for this editor instance. Lives in
     localStorage (not in `editorInstanceState`) because we only read
     it from a sibling surface — the @-mention picker — that doesn't
     need the round-trip through reactive state. Keeping it here means
     the picker can read synchronously without subscribing to a slice
     of `sessionsState`. Updated by an effect on `activePath` below. */
  function activeKey(id: string): string {
    return `woom:editor:active:${id}`;
  }

  /* Sidebar mode is now driven from the parent's ActivityBar, not from
     a tab strip at the bottom of the explorer (v7). Seven tabs total —
     `explorer` is the file tree, `git` is the staging / history pane,
     `review` is the agent-edits review board (Multi-Agent Diff Review),
     and the remaining three (`search`, `debug`, `tests`) render their
     own focused panes inside the same sidebar slot. The parent passes
     the active tab in via `sidebarTab` and we keep our own fallback
     when running outside an EditorApp shell. */
  type SidebarTab = 'explorer' | 'search' | 'git' | 'review' | 'debug' | 'tests';

  /* Bumped after every commit / push / pull / branch switch so the
     HistoryPanel inside the Git tab re-fetches automatically. */
  let gitChangeCount = $state(0);

  interface Props {
    /** Two-way bound to the parent so Claude sessions can pick up the repo
        as their default cwd. */
    repoPath?: string;
    /** Ordered open-root set (multi-root workspace). Single-root callers
        leave it empty and the component falls back to `[repoPath]`. The
        primary root stays `repoPath` (=== repoPaths[0]). */
    repoPaths?: string[];
    /** Pickable rows for the link dropdown — one per Claude
        session (so the user knows exactly which chat will get linked).
        `name` is the session title, `id` is the agent column instance,
        `sessionId` is the specific session to activate before linking
        (omitted only when the agent has no sessions yet — click then
        spawns a fresh chat in that column). */
    agentInstances?: { id: string; kind: 'claude'; name: string; sessionId?: string }[];
    /** Sessions currently linked TO this editor — rendered as chips in the
        header so the link is visible from the editor side too (matches the
        "Linked to Editor" pill on the AI column). */
    linkedAgents?: { sessionId: string; agentInstanceId: string; kind: 'claude'; name: string }[];
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
    /** Switch the parent's ActivityBar to the Review tab. Wired up by
     *  EditorApp; legacy callers leave it undefined and the file-level
     *  "Open Review" affordance becomes a no-op (other Review entry
     *  points still work). */
    onRequestReviewTab?: () => void;
    /** Quick-send a message to a linked session. Used by the inline
     *  "Edit selection" composer popover so the user can write +
     *  send a prompt without leaving the editor. Mirrors the same
     *  contract InlineClaude has for its mini-composer; if not
     *  provided, the inline composer falls back to "pin mention,
     *  user finishes the prompt elsewhere". */
    onQuickSend?: (sessionId: string, text: string) => void;
    /** Quiet direction (README §3.4, mockup 4j). No activity bar exists
     *  in Quiet, so the explorer folds the git commit box under the tree
     *  (source control stays reachable) and the main pane swaps the tab
     *  strip for a path breadcrumb. Cabin (default) is untouched. */
    quiet?: boolean;
    /** Pending agent-edit count — shown in the Quiet breadcrumb's right
     *  readout (`review · N`, mockup 4j). Owned by EditorApp. */
    reviewCount?: number;
  }
  let {
    repoPath = $bindable(''),
    repoPaths = [],
    agentInstances = [],
    linkedAgents = [],
    onLinkToAgent,
    onUnlinkAgent,
    sidebarTab = 'explorer',
    instanceLabel,
    instanceId = 'default',
    onRequestReviewTab,
    onQuickSend,
    quiet = false,
    reviewCount = 0
  }: Props = $props();

  const linkedAiKind = $derived<'claude' | null>(
    linkedAgents.length > 0 ? 'claude' : null
  );


  /** Effective open-root set. Single-root callers pass no `repoPaths`, so we
      fall back to `[repoPath]` — every downstream loop then behaves exactly
      as the old single-root path. Empty when no folder is open. */
  const roots = $derived(
    repoPaths.length > 0 ? repoPaths : repoPath ? [repoPath] : []
  );

  /** Header label for the open-root set. One root ⇒ its basename; many ⇒
      "<first> +N" so two-or-more repos read at a glance without overflowing.
      Full list lives in the title attr. */
  const rootLabel = $derived(
    roots.length <= 1
      ? fileName(repoPath)
      : `${fileName(roots[0])} +${roots.length - 1}`
  );
  const rootTitle = $derived(roots.length > 1 ? roots.join('\n') : repoPath);

  /** Owning root for an absolute path — longest matching root prefix, else
      the primary root. Lets tabs / relative labels work across roots. */
  function rootForPath(abs: string): string {
    if (!abs) return repoPath;
    let best = '';
    for (const r of roots) {
      if ((abs === r || abs.startsWith(r + '/')) && r.length > best.length) best = r;
    }
    return best || repoPath;
  }

  /** Collapsed root nodes in the multi-root explorer (by root path). */
  let collapsedRoots = $state<Set<string>>(new Set());
  function toggleRootCollapse(root: string) {
    const next = new Set(collapsedRoots);
    if (next.has(root)) next.delete(root); else next.add(root);
    collapsedRoots = next;
  }

  /** Which root the SOURCE CONTROL panel targets. Defaults to the active
      tab's owning root, else the primary. Clamped into the current set so a
      removed root can't leave it dangling. */
  let activeGitRoot = $state('');
  $effect(() => {
    const fromTab = activePath ? rootForPath(activePath) : '';
    const candidate = fromTab || repoPath;
    if (roots.length > 0 && !roots.includes(activeGitRoot)) {
      activeGitRoot = roots.includes(candidate) ? candidate : roots[0];
    } else if (roots.length === 0) {
      activeGitRoot = '';
    }
  });

  /** Custom SCM repo-picker open state (replaces the native <select> with a
      styled popover matching the rest of the editor chrome). */
  let scmPickerOpen = $state(false);

  let tabs = $state<string[]>([]);
  let activePath = $state<string>('');
  let dirtyByPath = $state<Record<string, boolean>>({});
  let editor: ReturnType<typeof Editor> | null = $state(null);
  let gitPanel = $state<{ refresh: () => Promise<void> } | null>(null);
  let error = $state<string | null>(null);
  let watchUnlisten: UnlistenFn | null = null;
  let gitStatusByPath = $state<Record<string, string>>({});
  let diffTarget = $state<{ path: string; staged: boolean } | null>(null);

  /* ── Quiet breadcrumb data (mockup 4j) ─────────────────────────────
     The Quiet main pane shows a path breadcrumb instead of the tab
     strip. Compute the active file's repo-relative directory + owning
     repo label, plus the changed-file count for the right readout. */
  const crumbPath = $derived(diffTarget ? `${roots.length > 1 ? activeGitRoot : repoPath}/${diffTarget.path}` : activePath);
  const crumbName = $derived(crumbPath ? fileName(crumbPath) : '');
  const crumbDir = $derived.by(() => {
    if (!crumbPath) return '';
    const root = rootForPath(crumbPath);
    let rel = crumbPath;
    if (root && (rel === root || rel.startsWith(root + '/'))) rel = rel.slice(root.length);
    rel = rel.replace(/^\/+/, '');
    const parts = rel.split('/');
    parts.pop();
    return [fileName(root), ...parts].filter(Boolean).join('/');
  });
  const gitChangedCount = $derived(Object.keys(gitStatusByPath).length);

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
  let tabbarEl = $state<HTMLDivElement | null>(null);

  // Scroll the active tab into view whenever activePath changes.
  $effect(() => {
    activePath;
    if (!tabbarEl) return;
    const active = tabbarEl.querySelector<HTMLElement>('.etab-tab.active');
    active?.scrollIntoView({ block: 'nearest', inline: 'nearest', behavior: 'instant' });
  });

  /* File-name search state + effect moved to EditorFileSearch.svelte
   *  in the wave-1 phase-4 split. The search pane is self-contained
   *  (own query/include/exclude state, debounced fs walk, result
   *  list) and now mounts directly under the sidebar's "search" tab. */

  /** Lower-cased extension (without dot) of an absolute file path,
   *  empty string when the path has no extension. Used by the
   *  preview-routing logic below + a few other places. */
  function extOf(p: string): string {
    const dot = p.lastIndexOf('.');
    if (dot < 0) return '';
    return p.slice(dot + 1).toLowerCase();
  }

  /* Image vs Markdown vs text routing for the right-pane render.
     Bitmap formats land on ImagePreview (asset:// URL through
     convertFileSrc); .svg too — it renders the same way. Markdown
     opens in the text editor by default but the user can flip to a
     side-by-side or full preview via the toolbar / ⇧⌘V.
     Anything else falls through to CodeMirror. */
  const IMAGE_EXTS = new Set(['png', 'jpg', 'jpeg', 'gif', 'webp', 'bmp', 'ico', 'svg', 'avif']);
  const MARKDOWN_EXTS = new Set(['md', 'mdx', 'markdown']);
  const isImagePath = $derived(activePath ? IMAGE_EXTS.has(extOf(activePath)) : false);
  const isMarkdownPath = $derived(activePath ? MARKDOWN_EXTS.has(extOf(activePath)) : false);

  /* Markdown preview is per-instance state, not per-file — keeps the
     muscle memory consistent: hit ⇧⌘V on README, then open
     CONTRIBUTING, the preview is still on. `'off' | 'split' | 'only'`
     so users with a wide screen can keep both panes visible while
     users on a laptop can flip to preview-only mode. */
  let mdPreviewMode = $state<'off' | 'split' | 'only'>('off');
  function cycleMdPreview() {
    mdPreviewMode = mdPreviewMode === 'off' ? 'split'
      : mdPreviewMode === 'split' ? 'only'
      : 'off';
  }

  /* Word-wrap is also per-instance (same reasoning). Off by default —
     users opening logs / CSV-ish text don't want wrap helping them
     misread columns. */
  let wordWrap = $state(false);

  /* Integrated terminal (vscode-style bottom panel). One dedicated PTY
     per editor instance — `ensureTerminalSession` keeps it alive across
     open/close toggles so scrollback survives. Height is drag-resizable
     via the grip; both open-state and height persist per instance. */
  const termOpenKey = () => `woom:edterm-open:${instanceId}`;
  const termHKey = () => `woom:edterm-h:${instanceId}`;
  let termOpen = $state(false);
  let termH = $state(260);
  onMount(() => {
    termOpen = localStorage.getItem(termOpenKey()) === '1';
    const h = Number(localStorage.getItem(termHKey()));
    if (Number.isFinite(h) && h >= 120 && h <= 800) termH = h;
  });
  function toggleTerm(): void {
    termOpen = !termOpen;
    try { localStorage.setItem(termOpenKey(), termOpen ? '1' : '0'); } catch {/* private mode */}
  }
  function startTermDrag(e: PointerEvent): void {
    e.preventDefault();
    const startY = e.clientY;
    const startH = termH;
    const move = (ev: PointerEvent) => {
      termH = Math.min(800, Math.max(120, startH + (startY - ev.clientY)));
    };
    const up = () => {
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', up);
      try { localStorage.setItem(termHKey(), String(termH)); } catch {/* private mode */}
    };
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', up);
  }

  /* Live mirror of the editor's text — fed into MarkdownPreview when
     in split / only mode so the preview tracks edits without us
     re-reading the file from disk on every keystroke. Only the MD
     case actually consumes it; for non-MD files the callback is a
     no-op (we don't even attach it). */
  let liveBuffer = $state<string | null>(null);
  $effect(() => {
    activePath; diffTarget;
    /* Reset on file swap so a stale buffer doesn't bleed into the
       new file's preview before its first onTextChange fires. */
    liveBuffer = null;
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
     - 1 linked session → label is just "Claude".
     - 2+ → suffix the session name so the user can tell e.g.
       Claude · Mona-Lisa apart from Claude · Da-Vinci. */
  type ApplyBtn = {
    sessionId: string;
    agentInstanceId: string;
    label: string;
    kind: 'claude';
  };
  const applyButtons = $derived.by<ApplyBtn[]>(() => {
    if (linkedAgents.length === 0) return [];
    if (linkedAgents.length === 1) {
      const a = linkedAgents[0];
      return [{ sessionId: a.sessionId, agentInstanceId: a.agentInstanceId, kind: 'claude', label: 'Claude' }];
    }
    return linkedAgents.map((a) => ({
      sessionId: a.sessionId,
      agentInstanceId: a.agentInstanceId,
      kind: 'claude' as const,
      label: `Claude · ${a.name}`
    }));
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

  /* ── Inline "Composer here" — `compose` mode of the same selection
     popover. Click the ✨ chip on the regular pick popover and the
     same anchor expands into a textarea + agent switcher. Pressing
     Enter pins `@<file>:<start>-<end>` to the chosen session and fires
     `onQuickSend` so the agent picks up the request immediately —
     same plumbing the InlineClaude mini-composer uses. The popover
     position is frozen on entry (`composerAnchor`) so a CodeMirror
     dispatch that nulls `selection.anchor` (caret moved while typing
     inside the popover) doesn't yank the floater back home. */
  let composerMode = $state<{
    sessionId: string;
    agentInstanceId: string;
    kind: 'claude';
    label: string;
    /** Frozen popover position the moment the user opened compose
     *  mode. Keeping it pinned means the textarea doesn't drift if
     *  the editor briefly loses geometry / scrolls slightly. */
    anchor: { x: number; y: number };
    /** Frozen selection range — same reason as `anchor` above; we
     *  send the lines the user originally highlighted, not whatever
     *  CodeMirror reports when Enter fires. */
    range: { startLine: number; endLine: number };
    filePath: string;
  } | null>(null);
  let composerText = $state('');
  let composerEl: HTMLTextAreaElement | null = $state(null);

  function openComposer(btn?: ApplyBtn) {
    if (!selection || !selection.anchor || !activePath) return;
    /* Default to the first applyButton when called without an
       explicit target — gives the keyboard / icon-click flow a
       sensible single-tap entry. */
    const target = btn ?? applyButtons[0];
    if (!target) return;
    composerMode = {
      sessionId: target.sessionId,
      agentInstanceId: target.agentInstanceId,
      kind: target.kind,
      label: target.label,
      anchor: { x: selection.anchor.x, y: selection.anchor.y },
      range: { startLine: selection.startLine, endLine: selection.endLine },
      filePath: activePath
    };
    composerText = '';
    /* Focus the textarea after the DOM has it. queueMicrotask is
       enough — Svelte mounts the element in this same task. */
    queueMicrotask(() => composerEl?.focus());
  }

  function closeComposer() {
    composerMode = null;
    composerText = '';
  }

  function switchComposerTarget(btn: ApplyBtn) {
    if (!composerMode) return;
    composerMode = {
      ...composerMode,
      sessionId: btn.sessionId,
      agentInstanceId: btn.agentInstanceId,
      kind: btn.kind,
      label: btn.label
    };
  }

  function sendComposer() {
    if (!composerMode) return;
    const text = composerText.trim();
    if (!text) return;
    /* Pin the range mention first — this puts `@<file>:<a>-<b>` into
       the session's input. If a sender callback was passed, append
       our text right after and fire it immediately; otherwise the
       mention sits in the composer and the user finishes typing in
       the InlineClaude pane (the requestInlineExpandFor signal
       above expands that pane automatically). */
    const result = applyRangeToAgent({
      sessionId: composerMode.sessionId,
      agentInstanceId: composerMode.agentInstanceId,
      filePath: composerMode.filePath,
      startLine: composerMode.range.startLine,
      endLine: composerMode.range.endLine
    });
    if (!result.ok || !result.token) {
      closeComposer();
      return;
    }
    if (onQuickSend) {
      const stamped = `@${result.token} ${text}`;
      onQuickSend(composerMode.sessionId, stamped);
    }
    /* Drop selection so the popover dismisses; without this the
       `pick` popover would re-render under the composer until the
       user clicks elsewhere. */
    selection = null;
    closeComposer();
  }

  function onComposerKey(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.preventDefault();
      closeComposer();
      return;
    }
    if (e.key === 'Enter' && !e.shiftKey && !e.altKey && !e.isComposing) {
      e.preventDefault();
      sendComposer();
    }
  }

  /* ── Pending-edits banner for the current file.
     Drives the inline strip above CodeMirror that reads
     "N pending edits from <agent> · Keep · Revert · Open Review".
     The strip lets the user resolve a single file's worth of agent
     edits without leaving the buffer; bulk verdicts across files
     happen in the Review pane. */
  type PendingEditEvent = ReturnType<typeof getPendingEditEvents>[number];
  type PendingForFile = {
    sessionId: string;
    sessionTitle: string;
    sessionKind: 'claude';
    event: PendingEditEvent;
  };
  const pendingEditsForActiveFile = $derived.by<PendingForFile[]>(() => {
    void sessionsState.list;
    const path = activePath;
    if (!path) return [];
    const out: PendingForFile[] = [];
    for (const la of linkedAgents) {
      for (const ev of getPendingEditEvents(la.sessionId)) {
        if (ev.filePath !== path) continue;
        out.push({
          sessionId: la.sessionId,
          sessionTitle: la.name || 'Claude',
          sessionKind: la.kind,
          event: ev
        });
      }
    }
    return out;
  });

  /** Just the EditEvents for the active file — fed to <Editor> so it can
   *  diff each into inline hunks. Derived from the same source as the
   *  review banner so the inline overlay and the banner never disagree. */
  const pendingEditEventsForEditor = $derived(
    pendingEditsForActiveFile.map((p) => ({
      sessionId: p.sessionId,
      toolId: p.event.toolId,
      oldText: p.event.oldText,
      newText: p.event.newText,
      wholeFile: p.event.wholeFile ?? false
    }))
  );

  /** Aggregate label for the banner — "2 pending edits from <chat>".
   *  Hand-built rather than via a join because the user reads
   *  "from <agent>" as a hint for "whose changes am I about to keep /
   *  revert", and that voice changes shape with the count. */
  const pendingBannerLabel = $derived.by(() => {
    const list = pendingEditsForActiveFile;
    if (list.length === 0) return '';
    const titles = new Set<string>();
    for (const p of list) titles.add(p.sessionTitle);
    const sources = Array.from(titles).join(' · ');
    const count = list.length;
    return count === 1 ? `1 pending edit from ${sources}` : `${count} pending edits from ${sources}`;
  });

  let bannerBusy = $state(false);

  function keepActiveFileEdits() {
    for (const p of pendingEditsForActiveFile) {
      updateEditEvent(p.sessionId, p.event.toolId, { status: 'kept', note: undefined });
    }
  }

  async function revertActiveFileEdits() {
    if (bannerBusy) return;
    bannerBusy = true;
    try {
      /* Newest-first within the file — same dependency-ordering reason
         as `revertAllPendingEdits`: stacked edits on the same file
         only revert cleanly if the latest one goes back first. */
      const ordered = pendingEditsForActiveFile.slice().reverse();
      for (const p of ordered) {
        const r = await revertEditEvent(p.sessionId, p.event);
        if (!r.ok) {
          notifyError(r.error, { title: `Couldn't revert ${p.event.filePath}` });
          break;
        }
      }
    } finally {
      bannerBusy = false;
    }
  }

  interface FileStatus { path: string; code: string; staged: boolean; unstaged: boolean; }
  interface GitStatusPayload {
    branch: string | null; upstream: string | null; ahead: number; behind: number; files: FileStatus[];
  }

  /** Reduce a 2-char porcelain code (index + worktree) to the single
      stronger indicator the tree decorations consume. */
  function strongestCode(code: string): string {
    const idx = code.charAt(0);
    const wt = code.charAt(1);
    let c = ' ';
    if (idx !== ' ' && idx !== '?') c = idx;
    else if (wt !== ' ') c = wt;
    if (c === ' ') c = 'M';
    return c;
  }

  /** Merge ONE root's status slice into the union `gitStatusByPath`. Entries
      are keyed by absolute path, so we drop the prior slice for THIS root
      (prefix match) then add the fresh one — other roots' entries are left
      intact. Single-root callers just replace the whole (one-root) map. */
  function onGitStatusChange(files: FileStatus[], rootArg?: string) {
    const root = (rootArg ?? repoPath).replace(/\/$/, '');
    if (!root) return;
    const prefix = `${root}/`;
    const next: Record<string, string> = {};
    // Carry over entries that belong to OTHER roots.
    for (const [path, code] of Object.entries(gitStatusByPath)) {
      if (!path.startsWith(prefix)) next[path] = code;
    }
    for (const f of files) {
      next[`${root}/${f.path}`] = strongestCode(f.code);
    }
    gitStatusByPath = next;
  }

  /** Path+timestamp of our own most recent write. The fs watcher echoes
   *  our save back as an `fs:changed` event; without this guard the
   *  handler below would `reload()` the active buffer right after an
   *  autosave — recreating the CodeMirror view and dropping the caret /
   *  focus mid-edit. We skip the self-triggered reload within a short
   *  window since the buffer already holds exactly what we wrote. */
  let lastSelfSave: { path: string; at: number } | null = null;

  /** Called after a successful ⌘S / autosave. Optimistic M + immediate refresh. */
  async function onFileSaved(savedPath: string) {
    lastSelfSave = { path: savedPath, at: Date.now() };
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
    if (roots.length === 0 || statusInFlight || destroyed) return;
    statusInFlight = true;
    try {
      // One `git_status` per root; merge each slice into the union map.
      // The primary root (roots[0]) drives the header branch label.
      for (let i = 0; i < roots.length; i++) {
        const root = roots[i];
        try {
          const s = await invoke<GitStatusPayload>('git_status', { repo: root });
          // Destroy could have landed during the await above. Stop here
          // so we don't write to a parent that's no longer interested.
          if (destroyed) return;
          onGitStatusChange(s.files, root);
          if (i === 0) gitBranch = s.branch ?? '';
        } catch (e) {
          console.warn('git_status failed for', root, e);
        }
      }
      lastStatusAt = Date.now();
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
          if (!repoPath) {
            // setRoot → openRoots loads this repo's remembered tabs.
            await setRoot(rootToLoad);
          } else {
            await startWatch();
            await loadTabsForRoots(roots);
          }
        }
      } catch {/* ignore */}
    }
    // Subscribe to file-change events — this is how we detect Claude's edits
    // and terminal edits. Debounced so a burst (e.g. Claude writing 5 files)
    // fires a single `git status` call, not 5.
    watchUnlisten = await listen<{ path: string; kind: string }>('fs:changed', (e) => {
      const p = e.payload.path;
      // Skip the reload when this event is the echo of our OWN save —
      // reloading would recreate the editor view and steal focus / caret
      // mid-edit. Only reload for genuine external changes.
      const isSelfSave =
        !!lastSelfSave && lastSelfSave.path === p && Date.now() - lastSelfSave.at < 1500;
      if (p === activePath && !dirtyByPath[activePath] && editor && !isSelfSave) {
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

  /* ⇧⌘V — Markdown preview cycle. The shortcut is registered globally
     in +page.svelte (so it can be scoped to the editor solo), then
     fan-outs to every EditorView via a window event. We only react
     when the active file is actually Markdown — otherwise the
     keystroke is a harmless no-op. */
  function onTogglePreview() {
    if (!isMarkdownPath) return;
    cycleMdPreview();
  }
  onMount(() => {
    window.addEventListener('woom:editor:toggle-md-preview', onTogglePreview);
    return () => window.removeEventListener('woom:editor:toggle-md-preview', onTogglePreview);
  });

  /* The FileTree fires `woom:fs:path-deleted` after a successful
     rm — close any open tab that lived inside the deleted subtree
     so we don't keep a phantom buffer pointing at a missing file.
     The fs watcher would eventually surface the error on next save,
     but proactive closing matches what VSCode does (and avoids the
     "save failed: no such file" surprise minutes later). */
  function onFsDeleted(e: Event) {
    const detail = (e as CustomEvent<{ path: string; isDir: boolean }>).detail;
    if (!detail?.path) return;
    const dead = detail.path;
    const prefix = dead + '/';
    const survivors = tabs.filter((p) => p !== dead && !p.startsWith(prefix));
    if (survivors.length === tabs.length) return;
    const wasActiveGone = !survivors.includes(activePath);
    tabs = survivors;
    /* Drop dirty markers for closed tabs so we don't carry stale
       "unsaved" badges. */
    const nextDirty: Record<string, boolean> = {};
    for (const p of survivors) if (dirtyByPath[p]) nextDirty[p] = true;
    dirtyByPath = nextDirty;
    if (wasActiveGone) activePath = survivors[0] ?? '';
    persistTabs();
  }
  onMount(() => {
    window.addEventListener('woom:fs:path-deleted', onFsDeleted);
    return () => window.removeEventListener('woom:fs:path-deleted', onFsDeleted);
  });

  /* ⌘W — close the active tab, NOT the window. The Rust side rebinds
     the macOS menu's Cmd+W from "Close Window" to a custom item that
     fires `menu:close-tab`; only a mounted EditorView reacts (solos
     remount on view switch, so at most one is live = the visible one).
     Priority: diff overlay first (it sits on top of the buffer), then
     the active file tab. No tabs open → no-op, window stays. */
  onMount(() => {
    let dead = false;
    let unlisten: UnlistenFn | null = null;
    void listen('menu:close-tab', () => {
      if (diffTarget) {
        closeDiff();
        return;
      }
      if (activePath) void closeTab(activePath);
    }).then((u) => {
      if (dead) u();
      else unlisten = u;
    });
    return () => {
      dead = true;
      unlisten?.();
    };
  });

  async function pickFolder() {
    let picked: string | string[] | null;
    try {
      // multiple:true → the user can pick several folders at once and open
      // them all as a multi-root workspace (like VS Code / Cursor).
      picked = await openDialog({ directory: true, multiple: true, title: 'Open folder(s)' });
    } catch (e) {
      notifyError(e, { title: "Couldn't open folder picker" });
      return;
    }
    if (!picked) return;
    const list = (Array.isArray(picked) ? picked : [picked]).filter(Boolean);
    if (list.length === 0) return;
    try {
      // "Open folder(s)" REPLACES the current root set — opening a fresh
      // selection forgets the previous roots (it doesn't silently keep them).
      await openRoots(list);
    } catch (e) {
      notifyError(e, { title: "Couldn't open folder" });
    }
  }

  /** Resolve a picked folder to its git work-tree root (walk up), falling
      back to the folder itself when it isn't inside a repo. */
  async function resolveRoot(path: string): Promise<string> {
    try {
      const root = await invoke<string>('git_repo_root', { path });
      return (root || path).trim();
    } catch {
      return path;
    }
  }

  /** Open `paths` as THE workspace root set (replace), resolving each to its
      git root. Keeps the bindable `repoPath` synced to the primary. */
  async function openRoots(paths: string[]) {
    error = null;
    const resolved: string[] = [];
    for (const p of paths) resolved.push(await resolveRoot(p));
    /* Save the OUTGOING repo's tabs before switching so it keeps its
       memory, then swap to the incoming repo's remembered tabs. This is
       the "Open folder(s)" replace path — i.e. switching repos. */
    const prevRoots = roots;
    if (repoKeyFor(prevRoots) !== repoKeyFor(resolved)) saveTabsForRoots(prevRoots);
    setEditorRoots(instanceId, resolved);
    repoPath = resolved[0] ?? '';
    if (repoPath) localStorage.setItem(rootKey(instanceId), repoPath);
    await startWatch();
    if (repoKeyFor(prevRoots) !== repoKeyFor(resolved)) await loadTabsForRoots(resolved);
  }

  async function setRoot(path: string) {
    await openRoots([path]);
  }

  /** Remove a workspace root: prune its open tabs (by path prefix), drop it
      from the set, and restart the watcher on the new primary. */
  function removeRoot(root: string) {
    const prefix = root + '/';
    const survivors = tabs.filter((p) => p !== root && !p.startsWith(prefix));
    if (survivors.length !== tabs.length) {
      const wasActiveGone = !survivors.includes(activePath);
      tabs = survivors;
      const nextDirty: Record<string, boolean> = {};
      for (const p of survivors) if (dirtyByPath[p]) nextDirty[p] = true;
      dirtyByPath = nextDirty;
      if (wasActiveGone) activePath = survivors[0] ?? '';
      persistTabs();
    }
    removeEditorRoot(instanceId, root);
    collapsedRoots = new Set([...collapsedRoots].filter((r) => r !== root));
    void startWatch();
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

  /* The agent edit selected in the ReviewPane (`sessionId:toolId`), passed to
     <Editor> so the overlay scrolls to + emphasises exactly that edit's hunks
     — computed against the live buffer, so it's correct even when several
     edits stack on one file. */
  let selectedEditKey = $state<string | null>(null);

  /** Open an edit's file and mark it selected so the editor highlights that
   *  specific chunk. Reactive: when the file (re)opens, the Editor's overlay
   *  recompute repopulates and the focus effect scrolls there. */
  function selectReviewEdit(filePath: string, sessionId: string, toolId: string) {
    selectedEditKey = `${sessionId}:${toolId}`;
    openFile(filePath);
  }

  /** Pull `pendingOpenFile` off the instance's slot whenever it appears
   *  and route through `openFile`. Lets external code (mention pills,
   *  diff cards, MCP open requests) drive the editor without reaching
   *  in via `bind:this`. Consume in a microtask so reading and clearing
   *  the same reactive proxy don't trip a self-write warning. */
  $effect(() => {
    const pending = sessionsState.editorInstanceState[instanceId]?.pendingOpenFile;
    if (!pending) return;
    queueMicrotask(() => {
      const next = consumeEditorOpenFile(instanceId);
      if (next) openFile(next);
    });
  });

  function openDiff(relPath: string, staged: boolean) {
    diffTarget = { path: relPath, staged };
  }

  function closeDiff() {
    diffTarget = null;
  }

  /** Hop from a diff view to the real file, scrolling the caret to
   *  the first changed line. We add a tab + activate it (via the
   *  existing `openFile`), then fire the cross-component goto event
   *  the Editor component already listens for. RAF + microtask dance
   *  lets the new EditorView mount before we dispatch — otherwise the
   *  goto event fires before any Editor instance is attached and the
   *  jump is dropped. */
  function openDiffFileAtLine(relPath: string, line: number) {
    if (!repoPath || !relPath) return;
    const abs = `${repoPath}/${relPath}`;
    openFile(abs);
    queueMicrotask(() => {
      requestAnimationFrame(() => {
        window.dispatchEvent(
          new CustomEvent('woom:editor:goto', {
            detail: { editorId: instanceId, filePath: abs, line }
          })
        );
      });
    });
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
    saveTabsForRoots(roots);
  }

  /** Persist the current open-tab set under a given root set's key. Used
   *  both on every tab mutation (current roots) and right before a repo
   *  switch (outgoing roots) so the old repo keeps its tab memory. */
  function saveTabsForRoots(rs: string[]) {
    const key = repoKeyFor(rs);
    if (key) localStorage.setItem(tabsKey(key), JSON.stringify(tabs));
  }

  /** Replace the visible tab set with the one remembered for `rs`. Clears
   *  first (so stale files from the previous repo vanish), then restores
   *  only paths that still exist AND live under one of the given roots. */
  async function loadTabsForRoots(rs: string[]) {
    /* Capture the previously-active file BEFORE clearing: `activePath = ''`
       below triggers the mirror $effect, which removes `activeKey` — so
       read it first or we lose which tab was focused. Restoring it (vs.
       always next[0]) is what keeps the editor on the file you left open
       when you leave + return to the editor solo (EditorApp remounts). */
    let storedActive: string | null = null;
    try {
      storedActive = localStorage.getItem(activeKey(instanceId));
    } catch {
      storedActive = null;
    }
    tabs = [];
    activePath = '';
    dirtyByPath = {};
    const key = repoKeyFor(rs);
    if (!key) return;
    let saved: unknown = [];
    try {
      saved = JSON.parse(localStorage.getItem(tabsKey(key)) || '[]');
    } catch {
      saved = [];
    }
    if (!Array.isArray(saved)) return;
    const next: string[] = [];
    for (const p of saved) {
      if (typeof p !== 'string') continue;
      if (!rs.some((r) => p === r || p.startsWith(r + '/'))) continue;
      const ok = await invoke<boolean>('fs_path_exists', { path: p }).catch(() => false);
      if (ok) next.push(p);
    }
    tabs = next;
    activePath = storedActive && next.includes(storedActive) ? storedActive : (next[0] ?? '');
  }

  /* Mirror the active path into localStorage so the agent's @-mention
     picker can pin "current" first without subscribing to reactive
     state. Cleared when the editor has no open file so a stale path
     doesn't survive across "close all tabs".

     `armed` gate: the effect's first run happens while activePath is
     still '' — onMount's tab restore is parked on an await at that
     point and hasn't read activeKey yet. An unconditional removeItem
     here erased the stored value before loadTabsForRoots could use
     it, so returning to the editor always landed on tabs[0] instead
     of the last-focused file. Only allow removal after at least one
     real write, i.e. once a genuine "close all tabs" is possible. */
  let activeMirrorArmed = false;
  $effect(() => {
    try {
      if (activePath) {
        localStorage.setItem(activeKey(instanceId), activePath);
        activeMirrorArmed = true;
      } else if (activeMirrorArmed) {
        localStorage.removeItem(activeKey(instanceId));
      }
    } catch { /* ignore quota errors — non-essential */ }
  });

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
    if (!p) return p;
    const root = rootForPath(p);
    if (!root) return p;
    return p.startsWith(root + '/') ? p.slice(root.length + 1) : p;
  }

  /** Tab display name: just the basename, with the immediate parent
   *  folder prepended when two open tabs share the same filename.
   *  Avoids showing the full resolved pnpm/symlink path in the tab
   *  bar when the tree shows the logical short path. */
  function tabDisplayName(p: string): string {
    const parts = p.split('/');
    const name = parts.at(-1) ?? p;
    const hasDupe = tabs.some((t) => t !== p && (t.split('/').at(-1) ?? '') === name);
    if (hasDupe && parts.length >= 2) return `${parts.at(-2)}/${name}`;
    return name;
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
          <!-- Top row: repo name + actions. Linked-session pills sit in
               their own subtle row below to keep the head uncluttered. -->
          <div class="ev-left-head">
            <div class="ev-root-stack">
              <div class="ev-root-line">
                <span class="ev-root-name" title={rootTitle}>{rootLabel}</span>
                {#if instanceLabel}<span class="ev-root-instance">{instanceLabel}</span>{/if}
              </div>
              {#if gitBranch}
                <span class="ev-root-branch mono">wt <b>{gitBranch}</b></span>
              {/if}
            </div>
            <!-- Chat linking lives in the right-side AgentDock — the old
                 tree-head chain button duplicated it and confused. -->
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
                  title="Linked to Claude · {la.name}"
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
          <!-- Mockup: no caption strip — the tree speaks for itself.
               Non-explorer panels keep a caption so users know where
               they landed. -->
          {#if sidebarTab !== 'explorer'}
            <div class="ev-sidebar-label">
              {#if sidebarTab === 'search'}Search
              {:else if sidebarTab === 'git'}Source control
              {:else if sidebarTab === 'review'}Agent edits
              {:else if sidebarTab === 'debug'}Debug
              {:else if sidebarTab === 'tests'}Tests{/if}
            </div>
          {/if}

          <!-- Sidebar body: one of five panels picked by the activity bar. -->
          <div class="ev-sidebar-body">
            {#if sidebarTab === 'explorer'}
              {#if roots.length > 1}
                <!-- Multi-root: each root is a collapsible top-level node with
                     its own FileTree. The union gitStatusByPath decorates every
                     tree (Phase 4); each FileTree keeps its own expand cache. -->
                {#each roots as root (root)}
                  {@const collapsed = collapsedRoots.has(root)}
                  <div class="ev-root-group">
                    <div class="ev-root-bar">
                      <button class="ev-root-toggle" onclick={() => toggleRootCollapse(root)} title={root}>
                        <svg class="i i-sm" viewBox="0 0 24 24" style="transform: rotate({collapsed ? 0 : 90}deg)"><path d="M9 6l6 6-6 6" /></svg>
                        <span class="ev-root-label mono">{fileName(root)}</span>
                      </button>
                      <button class="ev-root-remove" onclick={() => removeRoot(root)} title="Remove folder from workspace" aria-label="Remove folder">
                        <svg class="i i-sm" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7"><path d="M6 6l12 12M6 18 18 6"/></svg>
                      </button>
                    </div>
                    {#if !collapsed}
                      <FileTree
                        rootPath={root}
                        selectedPath={diffTarget ? `${activeGitRoot}/${diffTarget.path}` : activePath}
                        onSelect={openFile}
                        {gitStatusByPath}
                      />
                    {/if}
                  </div>
                {/each}
              {:else}
                <FileTree
                  rootPath={repoPath}
                  selectedPath={diffTarget ? `${repoPath}/${diffTarget.path}` : activePath}
                  onSelect={openFile}
                  {gitStatusByPath}
                />
              {/if}
              {#if quiet && repoPath}
                <!-- Quiet 4j — the activity bar is gone, so fold the git
                     commit box under the tree; source control stays a
                     click away. Compact (no history splitter). -->
                <div class="ev-quiet-git">
                  <GitPanel
                    bind:this={gitPanel}
                    repo={roots.length > 1 ? activeGitRoot : repoPath}
                    onStatusChange={(files) => { onGitStatusChange(files, roots.length > 1 ? activeGitRoot : repoPath); gitChangeCount += 1; }}
                    onOpenDiff={openDiff}
                    aiKind={linkedAiKind}
                  />
                </div>
              {/if}
            {:else if sidebarTab === 'git'}
              {#if roots.length > 1}
                <!-- Per-root SOURCE CONTROL: switcher picks which repo the
                     panel + history target so commit/push is unambiguous. -->
                <div class="ev-scm-switcher">
                  <span class="ev-scm-label">Repo</span>
                  <div class="ev-scm-picker">
                    <button
                      class="ev-scm-trigger"
                      onclick={() => (scmPickerOpen = !scmPickerOpen)}
                      title="Switch source-control repo"
                    >
                      <svg class="i i-sm ev-scm-branch" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><circle cx="6" cy="6" r="2.5"/><circle cx="18" cy="18" r="2.5"/><path d="M6 8.5V14a4 4 0 0 0 4 4h6"/></svg>
                      <span class="ev-scm-current mono">{fileName(activeGitRoot)}</span>
                      <svg class="i i-sm ev-scm-caret" viewBox="0 0 24 24"><path d="M6 9l6 6 6-6" /></svg>
                    </button>
                    {#if scmPickerOpen}
                      <div class="ev-scm-backdrop" role="presentation" onclick={() => (scmPickerOpen = false)}></div>
                      <div class="ev-scm-menu" role="menu">
                        {#each roots as root (root)}
                          <button
                            class="ev-scm-item mono"
                            class:active={root === activeGitRoot}
                            role="menuitemradio"
                            aria-checked={root === activeGitRoot}
                            onclick={() => { activeGitRoot = root; scmPickerOpen = false; }}
                            title={root}
                          >
                            <span class="ev-scm-check">{#if root === activeGitRoot}✓{/if}</span>
                            <span class="ev-scm-item-name">{fileName(root)}</span>
                          </button>
                        {/each}
                      </div>
                    {/if}
                  </div>
                </div>
              {/if}
              <Splitter direction="vertical" persistKey="editor-git-tab" initial={300} min={140} max={900}>
                {#snippet start()}
                  <GitPanel
                    bind:this={gitPanel}
                    repo={roots.length > 1 ? activeGitRoot : repoPath}
                    onStatusChange={(files) => { onGitStatusChange(files, roots.length > 1 ? activeGitRoot : repoPath); gitChangeCount += 1; }}
                    onOpenDiff={openDiff}
                    aiKind={linkedAiKind}
                  />
                {/snippet}
                {#snippet end()}
                  <HistoryPanel repo={roots.length > 1 ? activeGitRoot : repoPath} refreshKey={gitChangeCount} />
                {/snippet}
              </Splitter>
            {:else if sidebarTab === 'search'}
              <EditorFileSearch rootPath={repoPath} onSelect={openFile} />
            {:else if sidebarTab === 'review'}
              <ReviewPane
                linkedAgents={linkedAgents}
                instanceId={instanceId}
                repoPath={repoPath}
                onSelectEdit={(filePath, sessionId, toolId) => selectReviewEdit(filePath, sessionId, toolId)}
              />
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
          <EditorTabs
            {tabs}
            {activePath}
            {dirtyByPath}
            {diffTarget}
            {tabDisplayName}
            {cursorInfo}
            readoutLang={extOf(diffTarget?.path ?? activePath)}
            {quiet}
            {reviewCount}
            crumbName={crumbName}
            crumbDir={crumbDir}
            repoLabel={rootLabel}
            {instanceLabel}
            gitCount={gitChangedCount}
            onSwitch={(p) => void switchTab(p)}
            onClose={(p, e) => void closeTab(p, e)}
            onMiddleClick={(p, e) => void onTabMiddleClick(p, e)}
            onCloseDiff={closeDiff}
            bind:tabbarEl
          />
          <div class="ev-editor-wrap">
            {#if diffTarget}
              {#key `${diffTarget.path}:${diffTarget.staged}`}
                <DiffView
                  repo={repoPath}
                  path={diffTarget.path}
                  staged={diffTarget.staged}
                  onOpenFile={(line) => openDiffFileAtLine(diffTarget!.path, line)}
                />
              {/key}
            {:else if activePath}
              <!-- Pending-edits banner removed: it duplicated the
                   sidebar Review tab which already lists pending edits
                   with the same Keep/Revert affordances. Per-line
                   change bar (left gutter) + Review tab cover the
                   same need without sitting on top of the editor. -->

              {#if isImagePath}
                <!-- Bitmap / vector image — render via the asset://
                     protocol instead of dumping bytes into CodeMirror.
                     Bypasses the text editor entirely for this file
                     since "edit a PNG" isn't a real workflow. -->
                {#key activePath}
                  <ImagePreview path={activePath} />
                {/key}
              {:else if isMarkdownPath}
                <!-- Markdown bar exposes the preview toggle. Three
                     modes (off / split / only) cycle on click; ⇧⌘V
                     does the same. -->
                <div class="ev-md-toolbar">
                  <span class="ev-md-toolbar-label mono">Markdown</span>
                  <span class="ev-md-toolbar-spacer"></span>
                  <div class="ev-md-toolbar-tabs" role="tablist" aria-label="Preview mode">
                    <button
                      class="ev-md-tab" class:ev-md-tab--active={mdPreviewMode === 'off'}
                      onclick={() => (mdPreviewMode = 'off')}
                      role="tab" aria-selected={mdPreviewMode === 'off'}
                      title="Editor only"
                    >Edit</button>
                    <button
                      class="ev-md-tab" class:ev-md-tab--active={mdPreviewMode === 'split'}
                      onclick={() => (mdPreviewMode = 'split')}
                      role="tab" aria-selected={mdPreviewMode === 'split'}
                      title="Editor + preview side by side"
                    >Split</button>
                    <button
                      class="ev-md-tab" class:ev-md-tab--active={mdPreviewMode === 'only'}
                      onclick={() => (mdPreviewMode = 'only')}
                      role="tab" aria-selected={mdPreviewMode === 'only'}
                      title="Preview only"
                    >Preview</button>
                  </div>
                  <span class="ev-md-toolbar-kbd mono" title="Cycle preview modes">⇧⌘V</span>
                </div>
                {#if mdPreviewMode === 'only'}
                  {#key activePath}
                    <MarkdownPreview path={activePath} />
                  {/key}
                {:else if mdPreviewMode === 'split'}
                  <div class="ev-md-split">
                    <div class="ev-md-split-pane">
                      {#key activePath}
                        <Editor
                          bind:this={editor}
                          path={activePath}
                          {instanceId}
                          {wordWrap}
                          {onDirty}
                          repoPath={repoPath ?? ''}
                          pendingEdits={pendingEditEventsForEditor}
                          onSaved={onFileSaved}
                          onSelectionChange={(sel) => (selection = sel)}
                          onCursorChange={(info) => (cursorInfo = info)}
                          onTextChange={(t) => (liveBuffer = t)}
                          selectedEditKey={selectedEditKey}
                        />
                      {/key}
                    </div>
                    <div class="ev-md-split-divider" aria-hidden="true"></div>
                    <div class="ev-md-split-pane ev-md-split-pane--preview">
                      {#key activePath}
                        <MarkdownPreview path={activePath} liveSource={liveBuffer ?? undefined} />
                      {/key}
                    </div>
                  </div>
                {:else}
                  {#key activePath}
                    <Editor
                      bind:this={editor}
                      path={activePath}
                      {instanceId}
                      {wordWrap}
                      {onDirty}
                      repoPath={repoPath ?? ''}
                      pendingEdits={pendingEditEventsForEditor}
                      onSaved={onFileSaved}
                      onSelectionChange={(sel) => (selection = sel)}
                      onCursorChange={(info) => (cursorInfo = info)}
                      selectedEditKey={selectedEditKey}
                    />
                  {/key}
                {/if}
              {:else}
                {#key activePath}
                  <Editor
                    bind:this={editor}
                    path={activePath}
                    {instanceId}
                    {wordWrap}
                    {onDirty}
                    repoPath={repoPath ?? ''}
                    pendingEdits={pendingEditEventsForEditor}
                    onSaved={onFileSaved}
                    onSelectionChange={(sel) => (selection = sel)}
                    onCursorChange={(info) => (cursorInfo = info)}
                    selectedEditKey={selectedEditKey}
                  />
                {/key}
              {/if}
              <!-- The "Apply to <agent>" floating popover sits ABOVE
                   the status bar in z-order; the status bar lives
                   inside `.ev-editor-wrap` so it's anchored to the
                   bottom of the right pane regardless of how the
                   user resizes the splitter or toggles the bottom
                   problems panel. -->
              {#if composerMode && !diffTarget}
                <!-- "Composer here" — Cursor-style inline composer
                     anchored to the highlighted lines. Same surface
                     the pick popover sits on; we use the frozen
                     `composerMode.anchor` so a CodeMirror dispatch
                     during typing doesn't drag the textarea away. -->
                <div
                  class="ev-apply-pop ev-apply-pop--compose"
                  style:left="{composerMode.anchor.x}px"
                  style:top="{composerMode.anchor.y}px"
                  role="dialog"
                  aria-label="Compose inline edit prompt"
                >
                  <div class="ev-compose-head">
                    <span class="ev-compose-tag mono">@{relToRepo(composerMode.filePath)}:{composerMode.range.startLine === composerMode.range.endLine ? composerMode.range.startLine : `${composerMode.range.startLine}-${composerMode.range.endLine}`}</span>
                    <span class="ev-compose-spacer"></span>
                    <button
                      class="ev-compose-x"
                      onmousedown={(e) => e.preventDefault()}
                      onclick={closeComposer}
                      title="Close (Esc)"
                      aria-label="Close composer"
                    >
                      <svg class="i i-sm" viewBox="0 0 24 24"><path d="M6 6l12 12M6 18L18 6" /></svg>
                    </button>
                  </div>
                  <textarea
                    class="ev-compose-input"
                    bind:this={composerEl}
                    bind:value={composerText}
                    onkeydown={onComposerKey}
                    placeholder="Edit selection… (⏎ to send, ⇧⏎ newline)"
                    rows="2"
                  ></textarea>
                  <div class="ev-compose-foot">
                    {#if applyButtons.length > 1}
                      <div class="ev-compose-targets">
                        {#each applyButtons as btn (btn.sessionId)}
                          <button
                            class="ev-compose-target"
                            class:active={btn.sessionId === composerMode.sessionId}
                            class:claude={btn.kind === 'claude'}
                            onmousedown={(e) => e.preventDefault()}
                            onclick={() => switchComposerTarget(btn)}
                            title="Send to {btn.label}"
                          >{btn.label}</button>
                        {/each}
                      </div>
                    {:else}
                      <span class="ev-compose-target-single">→ {composerMode.label}</span>
                    {/if}
                    <span class="ev-compose-spacer"></span>
                    <button
                      class="ev-compose-send"
                      onmousedown={(e) => e.preventDefault()}
                      disabled={!composerText.trim() || !onQuickSend}
                      onclick={sendComposer}
                      title={onQuickSend ? 'Send (⏎)' : 'No quick-send wired up — pin the mention only'}
                    >
                      {onQuickSend ? 'Send' : 'Pin only'}
                      <svg class="i i-sm" viewBox="0 0 24 24" aria-hidden="true"><path d="M5 12h12M13 6l6 6-6 6"/></svg>
                    </button>
                  </div>
                </div>
              {:else if selection && selection.anchor && !diffTarget && applyButtons.length > 0}
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
                  style:max-width="calc(100vw - {selection.anchor.x}px - 24px)"
                  role="toolbar"
                  aria-label="Apply selection to agent"
                >
                  <button
                    class="ev-apply-pop-btn ev-apply-pop-btn--edit"
                    onmousedown={(e) => e.preventDefault()}
                    onclick={() => openComposer()}
                    title="Open inline composer to write a prompt about this selection"
                  >
                    <svg class="i i-sm" viewBox="0 0 24 24" aria-hidden="true">
                      <path d="M12 20h9"/>
                      <path d="M16.5 3.5a2.121 2.121 0 0 1 3 3L7 19l-4 1 1-4 12.5-12.5z"/>
                    </svg>
                    <span>Edit…</span>
                  </button>
                  {#each applyButtons as btn (btn.sessionId)}
                    <button
                      class="ev-apply-pop-btn"
                      class:claude={btn.kind === 'claude'}
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

          <!-- Integrated terminal — vscode-style bottom panel. Dedicated
               PTY per editor instance, cwd = the open repo root. The
               surface keeps the PTY alive across toggles (scrollback
               survives close/open). -->
          {#if termOpen}
            <div class="ev-term" style="height: {termH}px">
              <div
                class="ev-term-grip"
                onpointerdown={startTermDrag}
                role="separator"
                aria-orientation="horizontal"
                aria-label="Resize terminal"
              ></div>
              <div class="ev-term-body">
                <TerminalSurface instanceId={`edterm:${instanceId}`} cwd={repoPath || null} />
              </div>
            </div>
          {/if}

          <!-- Status bar: language · cursor position · encoding · line
               endings · git branch. Single horizontal strip pinned to the
               bottom of the editor pane. Always visible — it hosts the
               terminal toggle; file segments render only with a file open. -->
            <div class="ev-statusbar mono">
              {#if activePath || diffTarget}
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
              {/if}
              <span class="ev-status-spacer"></span>
              <button
                class="ev-status-action"
                class:ev-status-action--on={termOpen}
                onclick={toggleTerm}
                title={termOpen ? 'Hide integrated terminal' : 'Open integrated terminal'}
                aria-pressed={termOpen}
              >
                <svg viewBox="0 0 24 24" width="11" height="11" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                  <path d="M4 17l6-5-6-5"/><path d="M12 19h8"/>
                </svg>
                Terminal
              </button>
              <span class="ev-status-sep">·</span>
              {#if activePath && !diffTarget && !isImagePath}
                <button
                  class="ev-status-action"
                  class:ev-status-action--on={wordWrap}
                  onclick={() => (wordWrap = !wordWrap)}
                  title={wordWrap ? 'Word wrap on — click to turn off' : 'Word wrap off — click to turn on'}
                  aria-pressed={wordWrap}
                >
                  <svg viewBox="0 0 24 24" width="11" height="11" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                    <path d="M3 6h18"/><path d="M3 12h13a3 3 0 0 1 0 6h-3"/><path d="M3 18h6"/><path d="M16 15l-3 3 3 3"/>
                  </svg>
                  Wrap
                </button>
                <span class="ev-status-sep">·</span>
              {/if}
              {#if activePath && !diffTarget}
                <button
                  class="ev-status-action"
                  onclick={async () => {
                    try { await navigator.clipboard.writeText(activePath); }
                    catch { /* clipboard may be denied — silent */ }
                  }}
                  title="Copy absolute path of the active file"
                >Copy path</button>
                <span class="ev-status-sep">·</span>
                <button
                  class="ev-status-action"
                  onclick={async () => {
                    try { await invoke('fs_reveal_in_finder', { path: activePath }); }
                    catch { /* reveal isn't critical — silent */ }
                  }}
                  title="Reveal active file in Finder"
                >Reveal</button>
                <span class="ev-status-sep">·</span>
              {/if}
              <span class="ev-status-seg ev-status-ok" title="No diagnostics">✓ no problems</span>
              <span class="ev-status-sep">·</span>
              <span class="ev-status-seg ev-status-branch" title="Current git branch">
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="6" cy="6" r="2.5"/><circle cx="18" cy="18" r="2.5"/><path d="M6 8.5V14a4 4 0 0 0 4 4h6"/></svg>
                {gitBranch || '—'}
              </span>
            </div>
        </main>
      {/snippet}
    </Splitter>
  {/if}
  {#if error}<div class="ev-error">{error}</div>{/if}
</div>

<style>
  .ev { position: relative; display: flex; height: 100%; min-height: 0; flex: 1; background: var(--bg-0); }

  /* Multi-root explorer — one collapsible group per workspace root. Only
     rendered when >1 root is open; single-root keeps the bare tree. */
  .ev-root-group { border-bottom: 1px solid var(--border-neutral); }
  .ev-root-bar {
    display: flex; align-items: center; gap: 4px;
    padding: 2px 6px 2px 4px;
    background: var(--bg-2);
  }
  .ev-root-toggle {
    flex: 1; min-width: 0;
    display: inline-flex; align-items: center; gap: 4px;
    padding: 3px 4px;
    color: var(--text-0); font-size: 11.5px; font-weight: 600;
    text-align: left;
  }
  .ev-root-toggle :global(svg) { width: 11px; height: 11px; color: var(--text-2); transition: transform var(--dur-base) var(--ease-spring); flex-shrink: 0; }
  .ev-root-label { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .ev-root-remove {
    width: 20px; height: 20px; flex-shrink: 0;
    display: inline-flex; align-items: center; justify-content: center;
    border-radius: 4px; color: var(--text-2); opacity: 0;
    transition: opacity 100ms, color 100ms, background 100ms;
  }
  .ev-root-bar:hover .ev-root-remove { opacity: 1; }
  .ev-root-remove:hover { color: var(--error); background: var(--bg-3); }
  .ev-root-remove :global(svg) { width: 12px; height: 12px; }

  /* Per-root SOURCE CONTROL switcher. */
  .ev-scm-switcher {
    display: flex; align-items: center; gap: 8px;
    padding: 6px 10px;
    border-bottom: 1px solid var(--border-neutral);
    background: var(--bg-2);
  }
  .ev-scm-label { font-size: 10.5px; text-transform: uppercase; letter-spacing: 0.06em; color: var(--text-mute); flex-shrink: 0; }
  /* Custom repo picker — styled button + popover (replaces the native
     <select> so it matches the editor chrome instead of the OS widget). */
  .ev-scm-picker { position: relative; flex: 1; min-width: 0; }
  .ev-scm-trigger {
    display: flex; align-items: center; gap: 6px;
    width: 100%; min-width: 0;
    padding: 5px 8px; border-radius: 6px;
    background: var(--bg-0); color: var(--text-0);
    border: 1px solid var(--border-neutral-hi);
    font-size: 12px; text-align: left;
  }
  .ev-scm-trigger:hover { border-color: var(--border-hi2); background: var(--bg-1); }
  .ev-scm-branch { width: 12px; height: 12px; color: var(--accent-bright); flex-shrink: 0; }
  .ev-scm-current { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .ev-scm-caret { width: 12px; height: 12px; color: var(--text-2); flex-shrink: 0; }

  .ev-scm-backdrop { position: fixed; inset: 0; z-index: 600; background: transparent; }
  .ev-scm-menu {
    position: absolute; z-index: 601;
    top: calc(100% + 4px); left: 0; right: 0;
    padding: 4px;
    background: var(--bg-3);
    border: 1px solid var(--border-neutral-hi);
    border-radius: 8px;
    box-shadow: var(--shadow-3);
    max-height: 280px; overflow-y: auto;
  }
  .ev-scm-item {
    display: flex; align-items: center; gap: 6px;
    width: 100%; padding: 6px 8px; border-radius: 5px;
    color: var(--text-1); font-size: 12px; text-align: left;
  }
  .ev-scm-item:hover { background: var(--bg-2); color: var(--text-0); }
  .ev-scm-item.active { color: var(--accent-bright); }
  .ev-scm-check { width: 12px; flex-shrink: 0; font-size: 11px; color: var(--accent-bright); }
  .ev-scm-item-name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }

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
    padding: 10px 12px 8px;
    background: var(--bg-0);
    flex-shrink: 0;
  }
  /* Two-line head stack: small italic-serif instance mark above the
     repo name. Lets users see which Vermeer / Rothko / Hokusai
     instance they're inside without having to open the rail menu. */
  /* Redesign v2 §2.7 — head stack: `woom  Vermeer` line + `wt <branch>`
     mono underneath. */
  .ev-root-stack {
    flex: 1 1 0; min-width: 0;
    display: flex; flex-direction: column;
    gap: 2px;
    overflow: hidden;
  }
  .ev-root-line {
    display: flex; align-items: baseline; gap: 8px;
    min-width: 0;
  }
  .ev-root-instance {
    font-style: italic;
    font-size: 11px; color: var(--text-faint);
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
    flex-shrink: 0;
  }
  .ev-root-name {
    min-width: 0;
    font-size: 13px; font-weight: 600;
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
  .ev-linked-dot {
    width: 6px; height: 6px; border-radius: 50%;
    background: var(--src-claude);
    box-shadow: var(--shadow-1);
    flex-shrink: 0;
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

  /* Quiet 4j — git commit box folded under the tree. `margin-top:auto`
     pins it to the bottom of the sidebar column (the tree keeps the top
     and scrolls on its own); a border + capped height keep it a compact
     source-control footer, not a second full pane. */
  .ev-quiet-git {
    flex: none;
    margin-top: auto;
    max-height: 48%;
    min-height: 0;
    display: flex;
    overflow: hidden;
    border-top: 1px solid var(--border);
  }
  .ev-quiet-git > :global(*) { flex: 1; min-height: 0; width: 100%; }

  /* Active-pane label — small uppercase caption above the body, in
     place of the old VSCode-style bottom tab strip. The activity bar
     on the left now drives which pane shows here. */
  .ev-sidebar-label {
    flex: 0 0 auto;
    padding: 8px 16px 6px;
    font-size: 9.5px; font-weight: 600;
    letter-spacing: 0.14em; text-transform: uppercase;
    color: var(--text-faint);
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
    font-family: var(--font-mono);
    font-size: 18px; font-weight: 600; letter-spacing: -0.01em;
    color: var(--text-0);
    margin: 0 0 8px;
  }
  .ev-sidebar-empty-p {
    font-size: 11.5px; color: var(--text-2);
    line-height: 1.5; margin: 0;
  }
  .ev-sidebar-empty-p .mono {
    font-family: var(--font-mono);
    font-size: 10.5px;
    padding: 1px 5px;
    background: var(--bg-2); border: 1px solid var(--border);
    border-radius: 4px;
    color: var(--accent-bright);
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
  /* Top tab strip styles moved to EditorTabs.svelte in the wave-1
   *  phase-4 split. The bar is rendered by `<EditorTabs />` directly
   *  under `.ev-main`; visual rhythm preserved 1:1. */

  .ev-editor-wrap { flex: 1; min-height: 0; position: relative; display: flex; flex-direction: column; }

  /* Pending agent-edits banner — sits between the tab strip and the
     CodeMirror surface for a file that has unresolved agent edits.
     Slim, brand-tinted, never absolute (so the editor surface
     shrinks to fit instead of having content go behind it). The
     `flex-direction: column` on `.ev-editor-wrap` above lets this
     row + the editor below stack cleanly. */
  /* Status bar — single horizontal strip pinned to the bottom of
     the editor pane. Mono throughout, brand-dot for the git branch
     readout, mint check for "no problems". */
  /* Whisper statusbar — mockup shows no status chrome, so this
     stays but fades to a hairline caption. */
  .ev-statusbar {
    display: flex; align-items: center; gap: 6px;
    padding: 4px 16px;
    border-top: 0;
    background: var(--bg-2);
    font-size: 10px;
    color: var(--text-faint);
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

  /* Status-bar action button — share the bar's monospace + small
     size, but feel clickable: subtle background on hover, an active
     tint when toggled on (used by Word-wrap). Doesn't try to look
     like a heavy CTA — the bar is dense and these need to read as
     "you can click this" without pulling focus. */
  .ev-status-action {
    display: inline-flex; align-items: center; gap: 4px;
    background: transparent;
    border: 0;
    padding: 0 5px;
    height: 18px;
    color: var(--text-2);
    font: inherit;
    font-size: 10.5px;
    border-radius: 3px;
    cursor: pointer;
    transition: background 120ms, color 120ms;
  }
  .ev-status-action:hover { background: var(--bg-3); color: var(--text-0); }
  .ev-status-action--on {
    color: var(--accent-bright);
    background: color-mix(in srgb, var(--accent) 14%, transparent);
  }
  .ev-status-action :global(svg) { width: 11px; height: 11px; }

  /* Integrated terminal panel — permanent dark inset (terminal is the
     one surface that stays night-side in both themes), hairline top
     border, drag grip on the upper edge. */
  .ev-term {
    position: relative;
    flex: none;
    display: flex;
    flex-direction: column;
    min-height: 120px;
    border-top: 1px solid var(--border);
    background: var(--dark-bg-0, #17191c);
  }
  .ev-term-grip {
    position: absolute;
    top: -3px;
    left: 0;
    right: 0;
    height: 7px;
    cursor: row-resize;
    z-index: 3;
  }
  .ev-term-grip:hover { background: color-mix(in srgb, var(--accent) 18%, transparent); }
  .ev-term-body {
    flex: 1;
    min-height: 0;
    display: grid;
  }

  /* ── Markdown preview toolbar + split layout ──────── */
  .ev-md-toolbar {
    display: flex; align-items: center; gap: 8px;
    padding: 5px 12px;
    height: 28px;
    background: linear-gradient(180deg, color-mix(in srgb, var(--accent) 5%, var(--bg-1)), var(--bg-1));
    border-bottom: 1px solid var(--border);
    flex-shrink: 0;
  }
  .ev-md-toolbar-label {
    font-size: 9.5px;
    text-transform: uppercase;
    letter-spacing: 0.1em;
    color: var(--text-mute);
    font-weight: 700;
  }
  .ev-md-toolbar-spacer { flex: 1; }
  .ev-md-toolbar-tabs {
    display: inline-flex;
    background: var(--bg-2);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 2px;
    gap: 2px;
  }
  .ev-md-tab {
    background: transparent;
    border: 0;
    padding: 2px 10px;
    border-radius: 4px;
    color: var(--text-2);
    font-size: 11px;
    font-weight: 500;
    cursor: pointer;
    transition: background 120ms, color 120ms;
  }
  .ev-md-tab:hover { color: var(--text-0); }
  .ev-md-tab--active {
    background: var(--bg-0);
    color: var(--accent-bright);
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--accent) 28%, var(--border));
  }
  .ev-md-toolbar-kbd {
    font-size: 9.5px;
    color: var(--text-mute);
    background: var(--bg-2);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 1px 5px;
  }

  .ev-md-split {
    flex: 1; min-height: 0;
    display: grid;
    grid-template-columns: 1fr 1px 1fr;
  }
  .ev-md-split-pane {
    min-width: 0; min-height: 0;
    overflow: hidden;
    display: flex; flex-direction: column;
  }
  .ev-md-split-pane--preview {
    background: var(--bg-0);
  }
  .ev-md-split-divider {
    background: var(--border);
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
    box-shadow: 0 1px 0 0 rgba(0, 0, 0, 0.1), var(--shadow-1);
    /* Inline `max-width` (computed against the anchor's viewport x)
     * keeps the popover from running past the editor's right edge
     * when the session list is long. Buttons inside ellipsize their
     * labels via `min-width: 0` + `text-overflow: ellipsis` below. */
    min-width: 0;
    overflow: hidden;
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
    /* Cap each pill so 5+ sessions don't all fight to display full
     * names. The leading icon + `Apply to ` prefix stays full-strength;
     * the trailing session label ellipses when needed. */
    min-width: 0;
    max-width: 220px;
  }
  .ev-apply-pop-btn > span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }
  .ev-apply-pop-btn:hover {
    background: var(--accent-soft);
    border-color: var(--accent);
  }
  .ev-apply-pop-btn :global(svg) {
    width: 12px; height: 12px; opacity: 0.85;
  }
  .ev-apply-pop-btn.claude { border-left: 2px solid var(--accent); padding-left: 8px; }
  .ev-apply-pop-btn--edit {
    color: var(--accent-bright);
  }
  .ev-apply-pop-btn--edit:hover {
    background: var(--accent-soft);
    border-color: var(--border-accent-2);
  }

  /* Compose mode — the same popover, expanded into a textarea + send
     row. Wider and not nowrap so the user can actually type into it.
     We keep the brand-tinted border + shadow so the surface still
     reads as the "selection action" widget, just bigger. */
  .ev-apply-pop--compose {
    flex-direction: column;
    align-items: stretch;
    width: 360px;
    max-width: 60vw;
    padding: 8px;
    gap: 6px;
    white-space: normal;
  }
  .ev-compose-head {
    display: flex; align-items: center; gap: 8px;
    color: var(--text-1);
    font-size: 11px;
  }
  .ev-compose-tag {
    color: var(--accent-bright);
    font-size: 10.5px;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .ev-compose-spacer { flex: 1; }
  .ev-compose-x {
    width: 18px; height: 18px;
    display: inline-grid; place-items: center;
    color: var(--text-mute);
    background: transparent; border: 0; border-radius: 4px;
    cursor: pointer;
  }
  .ev-compose-x:hover { color: var(--text-0); background: var(--bg-elev, var(--bg-2)); }
  .ev-compose-x :global(svg) { width: 11px; height: 11px; }
  .ev-compose-input {
    width: 100%;
    box-sizing: border-box;
    resize: vertical;
    min-height: 56px;
    max-height: 200px;
    padding: 8px 10px;
    background: var(--bg-1);
    border: 1px solid var(--border);
    border-radius: 6px;
    color: var(--text-0);
    font-family: inherit;
    font-size: 12.5px;
    line-height: 1.5;
    outline: none;
    transition: border-color 120ms;
  }
  .ev-compose-input:focus { border-color: var(--border-accent-2); }
  .ev-compose-input::placeholder { color: var(--text-mute); }
  .ev-compose-foot {
    display: flex; align-items: center; gap: 6px;
  }
  .ev-compose-targets {
    display: inline-flex; gap: 3px;
    padding: 2px;
    background: var(--bg-1);
    border: 1px solid var(--border);
    border-radius: 6px;
    flex-wrap: wrap;
  }
  .ev-compose-target {
    padding: 2px 8px;
    background: transparent;
    border: 0;
    color: var(--text-2);
    font-size: 11px;
    border-radius: 4px;
    cursor: pointer;
    transition: color 100ms, background 100ms;
  }
  .ev-compose-target:hover { color: var(--text-0); }
  .ev-compose-target.active { background: var(--bg-3); color: var(--text-0); }
  .ev-compose-target.claude.active { color: var(--src-claude); }
  .ev-compose-target-single {
    font-size: 11px;
    color: var(--text-2);
    padding: 2px 6px;
  }
  .ev-compose-send {
    display: inline-flex; align-items: center; gap: 6px;
    padding: 4px 12px;
    background: var(--accent);
    border: 1px solid var(--accent);
    color: var(--accent-fg);
    border-radius: 6px;
    font-size: 12px; font-weight: 600;
    cursor: pointer;
    transition: opacity 100ms, transform 100ms;
  }
  .ev-compose-send:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }
  .ev-compose-send:not(:disabled):hover { transform: translateY(-1px); }
  .ev-compose-send :global(svg) { width: 12px; height: 12px; }

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
  .ev-root-branch {
    font-size: 10px; color: var(--text-mute);
    white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
    min-width: 0;
  }
  .ev-root-branch b { color: var(--src-editor); font-weight: 500; }
</style>
