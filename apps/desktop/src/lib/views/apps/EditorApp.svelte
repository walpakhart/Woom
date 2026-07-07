<script lang="ts">
  /* EditorApp — VS Code-class workspace.
     Layout: [activity 44] [editor (flex)] [inline-claude 280]

     Center editor reuses the existing <EditorView> — a low-level
     CodeMirror wrapper (file tree + tabs + code) — for ~900 lines of
     editor plumbing. New peers: ActivityBar / InlineClaude as
     standalone components under lib/views/apps/editor/. */

  import EditorView from '$lib/components/editor/EditorView.svelte';
  import ActivityBar from './editor/ActivityBar.svelte';
  import AgentDock, { type DockHandlers } from './editor/AgentDock.svelte';
  import Splitter from '$lib/components/ui/Splitter.svelte';
  import SidePaneRail from '$lib/components/ui/SidePaneRail.svelte';
  import { sessionsState, getPendingEditEvents, editorRoots, updateSession } from '$lib/state/sessions.svelte';
  import { kindForInstanceId, APP_INSTANCE_IDS, layoutState } from '$lib/state/layout.svelte';
  import { layoutModeState } from '$lib/state/layoutMode.svelte';
  import { onMount, untrack } from 'svelte';
  import { fly } from 'svelte/transition';
  import { cubicOut } from 'svelte/easing';

  type ActivityTab = 'explorer' | 'search' | 'git' | 'review' | 'debug' | 'tests';
  type SidebarTab = ActivityTab;

  interface Props {
    instanceId: string;
    /** Triggered when the user picks a chat from the editor's link
     *  picker. `sessionId` is optional — when present the parent
     *  activates that specific session before linking; when absent
     *  the parent links whatever's currently active in the agent
     *  app (or spawns a new chat if the agent has no sessions yet). */
    onLinkToAgent: (agentInstanceId: string, sessionId?: string) => void;
    onOpenClaude: () => void;
    /** Switches the top-level view to Settings (driven by +page.svelte
     *  via the rail). Lets the activity-bar gear act as a real shortcut. */
    onOpenSettings?: () => void;
    /** Quick-send to a linked session — fires immediately if idle,
     *  queues if a turn is in flight. Used by the per-row inline
     *  composer in the Inline Claude pane. */
    onQuickSend: (sessionId: string, text: string) => void;
    /** Activate a specific linked session AND switch the top-level
     *  view to its agent app. Per-row "Open" affordance. */
    onOpenSession: (sessionId: string, agentInstanceId: string) => void;
    /** Agent CLI connection flag — forwarded to AgentDock so it can
     *  render a "Connect first" stub instead of a dead chat body when
     *  Claude isn't connected. */
    connectedClaude?: boolean;
    /** Agent callback bundle for the docked ChatThread + Composer. */
    dock: DockHandlers;
  }
  let p: Props = $props();

  /* Interface direction. Quiet §3 mockup 4j strips the editor to a
     single centred document: the center EditorView only, a mono
     filename/repo header, and none of the Cabin chrome (ActivityBar,
     file-tree sidebar, AgentDock / Splitter). Cabin is untouched. */
  const quiet = $derived(layoutModeState.mode === 'quiet');

  let activityTab = $state<ActivityTab>('explorer');

  /** Agent-dock open state. Persisted per editor instance —
   *  Vermeer/Hokusai/etc remember whether the user prefers the dock
   *  hidden (more chrome for code) or shown (code + chat side by side).
   *  Default = true so first-run users discover the dock exists.
   *  Keeps the legacy `editor-claude-side-open:<id>` key so users who
   *  had the old Inline-agents pane open get the dock open too. */
  // svelte-ignore state_referenced_locally
  const sideStorageKey = `editor-claude-side-open:${p.instanceId}`;
  let dockOpen = $state(true);
  onMount(() => {
    const v = localStorage.getItem(sideStorageKey);
    if (v === '0' || v === '1') dockOpen = v === '1';
  });
  $effect(() => {
    localStorage.setItem(sideStorageKey, dockOpen ? '1' : '0');
  });

  const sidebarTab = $derived<SidebarTab>(activityTab);

  /** Fire when EditorView's pending-edits banner asks to jump to the
   *  Review tab. Toggling activityTab is enough — sidebarTab follows. */
  function focusReviewTab() {
    activityTab = 'review';
  }

  /* Editor-scoped keyboard shortcuts. Mounted on window only while
     EditorApp is in the DOM (i.e. the user is actually looking at
     the editor solo) so they don't leak into other surfaces.
       - ⇧⌘R → Review tab. Mirrors VS Code's "Show Source Control"
                rhythm; we picked R because Review starts with R and
                ⇧⌘G is already taken by Source Control. */
  onMount(() => {
    function handler(e: KeyboardEvent) {
      /* ⌘L toggles the agent dock. Checked FIRST and allowed from text
         inputs / CodeMirror — toggling the dock mid-typing is the
         Cursor muscle memory, unlike ⇧⌘R which bails on inputs. */
      if ((e.metaKey || e.ctrlKey) && (e.key === 'l' || e.key === 'L') && !e.shiftKey && !e.altKey) {
        e.preventDefault();
        dockOpen = !dockOpen;
        return;
      }
      const t = e.target as HTMLElement | null;
      if (t && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.isContentEditable)) return;
      if ((e.metaKey || e.ctrlKey) && e.shiftKey && (e.key === 'R' || e.key === 'r') && !e.altKey) {
        e.preventDefault();
        focusReviewTab();
      }
    }
    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  });

  /** Curated label of the currently-mounted editor instance — flows
   *  down to EditorView's sidebar head as a small italic-serif mark
   *  above the repo name. */
  const instanceLabel = $derived(
    layoutState.instances.editor.find((i) => i.id === p.instanceId)?.name ?? ''
  );

  function pickActivity(t: ActivityTab) {
    activityTab = t;
  }

  /** RepoPath for EditorView — read from the per-instance state slot
   *  on mount, written back on change. */
  let repoPath = $state(
    untrack(() => sessionsState.editorInstanceState[p.instanceId]?.repoPath ?? '')
  );
  $effect(() => {
    const slot = sessionsState.editorInstanceState[p.instanceId];
    if (!slot) {
      sessionsState.editorInstanceState[p.instanceId] = { repoPath };
    } else {
      slot.repoPath = repoPath;
    }
  });

  /** Ordered open-root set for this editor instance. Single-root ⇒ [repoPath].
   *  Read from the per-instance slot so it survives reload + tracks
   *  add/remove-root mutations made inside EditorView. */
  const repoPaths = $derived.by(() => {
    void sessionsState.editorInstanceState[p.instanceId]?.repoPaths;
    void repoPath;
    return editorRoots(p.instanceId);
  });

  /* Repo basename for the Quiet mono header — the "document location"
     eyebrow above the stripped EditorView (mockup 4j). EditorView owns
     the per-file tab strip, so this is the workspace-level mark. */
  const repoLabel = $derived(repoPath ? (repoPath.split('/').filter(Boolean).pop() ?? repoPath) : '');

  /** Link-picker entries — one row per Claude session that is
   *  not already linked to this editor. The label is the session
   *  title so the user knows exactly which chat they're linking; if
   *  the agent has no sessions yet we still surface a single row so
   *  the user can spawn-and-link in one click. */
  const agentInstances = $derived.by(() => {
    const out: { id: string; kind: 'claude'; name: string; sessionId?: string }[] = [];
    const sortByActivity = (a: typeof sessionsState.list[number], b: typeof sessionsState.list[number]) => {
      const ta = a.messages[a.messages.length - 1]?.at ?? '';
      const tb = b.messages[b.messages.length - 1]?.at ?? '';
      return tb.localeCompare(ta);
    };
    const colId = APP_INSTANCE_IDS.claude;
    /* Archived chats are hidden from the Claude sidebar — offering
       them as link targets here resurrected them into the dock. */
    const sessions = sessionsState.list.filter((s) => !s.archived).sort(sortByActivity);
    if (sessions.length === 0) {
      out.push({ id: colId, kind: 'claude', name: 'Claude' });
    } else {
      for (const s of sessions) {
        /* Skip sessions that already point at this editor — listing
           them would mean "link the linked", which is a no-op. */
        if (s.linkedToEditor && s.linkedToEditorInstanceId === p.instanceId) continue;
        out.push({ id: colId, kind: 'claude', name: s.title || 'Untitled chat', sessionId: s.id });
      }
    }
    return out;
  });

  /** Sessions linked TO this editor (for Link chips in the EditorView header). */
  const linkedAgents = $derived.by(() => {
    const out: { sessionId: string; agentInstanceId: string; kind: 'claude'; name: string }[] = [];
    for (const s of sessionsState.list) {
      if (s.archived) continue;
      if (!s.linkedToEditor) continue;
      if (s.linkedToEditorInstanceId !== p.instanceId) continue;
      if (!s.agentInstanceId) continue;
      if (kindForInstanceId(s.agentInstanceId) !== 'claude') continue;
      out.push({ sessionId: s.id, agentInstanceId: s.agentInstanceId, kind: 'claude', name: s.title });
    }
    return out;
  });

  function unlinkSession(sessionId: string) {
    /* Through updateSession, NOT direct field writes — a bare
       `s.linkedToEditor = false` skipped the persist scheduler, so the
       unlink evaporated on restart and the chat came back linked. */
    updateSession(sessionId, { linkedToEditor: false, linkedToEditorInstanceId: null });
  }

  /** Git change count → badge on the activity-bar Git button. Will be
   *  real once git_status is wired into EditorApp. MVP — 0. */
  const gitCount = 0;
  /** Problems count → badge on the activity-bar Tests + bottom Problems
   *  tab. MVP — 0 (typecheck integration in the next milestone). */
  const problemsCount = 0;

  /** Pending agent edits across every linked session — drives the
   *  Review tab's badge + pulse. We touch sessionsState.list inside
   *  the derived so $derived recomputes on any session-state mutation
   *  (new edit appended, status flipped, etc.). Cheap: one
   *  getPendingEditEvents call per linked agent, and the array length
   *  is the answer. */
  const reviewCount = $derived.by(() => {
    void sessionsState.list;
    let total = 0;
    for (const la of linkedAgents) {
      total += getPendingEditEvents(la.sessionId).length;
    }
    return total;
  });
</script>

<section
  class="app-shell se-shell"
  class:se-shell--quiet={quiet}
  class:se-shell--with-side={dockOpen && !quiet}
  style="--app-tone: var(--src-editor); --app-glow: rgba(204,120,92,0.42);"
  ontransitionend={(e) => {
    /* Nudge CodeMirror to re-measure once the dock open/close column
       animation settles — without it a freshly-revealed editor can
       leave a ghost gutter until the next manual interaction. */
    if (e.propertyName === 'grid-template-columns') {
      window.dispatchEvent(new Event('resize'));
    }
  }}
>
  {#if quiet}
    <!-- Quiet §3 mockup 4j — single centred document. Just the center
         EditorView (tabs + code); the file-tree sidebar + internal
         divider are hidden via `:global` below, and the ActivityBar /
         AgentDock / outer Splitter are simply not rendered. -->
    <div class="qeditor">
      <div class="qeditor-head">
        {#if repoLabel}
          <span class="qeditor-name mono">{repoLabel}</span>
        {/if}
        {#if instanceLabel}
          <span class="qeditor-instance">{instanceLabel}</span>
        {/if}
      </div>
      <div class="qeditor-body">
        <EditorView
          bind:repoPath
          {repoPaths}
          {agentInstances}
          {linkedAgents}
          {sidebarTab}
          {instanceLabel}
          instanceId={p.instanceId}
          onLinkToAgent={p.onLinkToAgent}
          onUnlinkAgent={unlinkSession}
          onRequestReviewTab={focusReviewTab}
          onQuickSend={p.onQuickSend}
        />
      </div>
    </div>
  {:else}
  <div class="app-pane se-activity">
    <ActivityBar
      activeTab={activityTab}
      onPick={pickActivity}
      onOpenSettings={p.onOpenSettings}
      {gitCount}
      {problemsCount}
      {reviewCount}
      dockOpen={dockOpen}
      onToggleDock={() => (dockOpen = !dockOpen)}
    />
  </div>

  {#if dockOpen}
    <!-- Splitter between the editor center and the AgentDock.
         User-resizable; width persists per-instance under
         `editor-dock:<instanceId>` so each Vermeer / Hokusai keeps
         its own preferred split across reloads. Wider range than the
         old Inline-agents pane — a full chat needs room. -->
    <Splitter
      direction="horizontal"
      fixedSide="end"
      persistKey="editor-dock:{p.instanceId}"
      initial={440}
      min={340}
      max={760}
    >
      {#snippet start()}
        <section class="app-pane se-center">
          <div class="se-editor-area">
            <EditorView
              bind:repoPath
              {repoPaths}
              {agentInstances}
              {linkedAgents}
              {sidebarTab}
              {instanceLabel}
              instanceId={p.instanceId}
              onLinkToAgent={p.onLinkToAgent}
              onUnlinkAgent={unlinkSession}
              onRequestReviewTab={focusReviewTab}
              onQuickSend={p.onQuickSend}
            />
          </div>
        </section>
      {/snippet}
      {#snippet end()}
        <aside class="app-pane se-inline" in:fly={{ x: 24, duration: 220, easing: cubicOut }}>
          <AgentDock
            instanceId={p.instanceId}
            {linkedAgents}
            {agentInstances}
            connectedClaude={p.connectedClaude ?? true}
            onClose={() => (dockOpen = false)}
            onOpenSession={p.onOpenSession}
            onLinkToAgent={p.onLinkToAgent}
            dock={p.dock}
          />
        </aside>
      {/snippet}
    </Splitter>
  {:else}
    <section class="app-pane se-center">
      <div class="se-editor-area">
        <EditorView
          bind:repoPath
          {repoPaths}
          {agentInstances}
          {linkedAgents}
          {sidebarTab}
          {instanceLabel}
          instanceId={p.instanceId}
          onLinkToAgent={p.onLinkToAgent}
          onUnlinkAgent={unlinkSession}
          onRequestReviewTab={focusReviewTab}
          onQuickSend={p.onQuickSend}
        />
      </div>
    </section>
    <!-- Rail mirrors the left ActivityBar — same width (44px),
         same flat background, sits as a sibling column in the
         outer grid (NOT inside .se-center). So the editor pane
         keeps its own .app-pane chrome and tabs/scrollbars stop
         at its right edge instead of bleeding under the rail. -->
    <SidePaneRail
      linkedAgents={linkedAgents.map((la) => ({
        sessionId: la.sessionId,
        agentInstanceId: la.agentInstanceId,
        kind: la.kind,
        title: la.name
      }))}
      {reviewCount}
      onExpand={() => (dockOpen = true)}
      onAgentClick={(a) => {
        /* Rail icon → dock THAT session. The dock (AgentDock) reads
           `editor-dock-session:<instanceId>` on mount to pick its
           session, so seed it before expanding — otherwise the dock
           re-opens on its last/most-recent pick and ignores the rail
           click. */
        try {
          localStorage.setItem(`editor-dock-session:${p.instanceId}`, a.sessionId);
        } catch {
          /* private mode / quota — dock falls back to most-recent. */
        }
        dockOpen = true;
      }}
    />
  {/if}
  {/if}
</section>

<style>
  /* Two grid layouts:
     - open: 44px ActivityBar + Splitter cell (editor + InlineClaude).
     - rail-collapsed: 44px ActivityBar + editor pane (1fr) + 44px
       rail (mirror of the ActivityBar on the right edge). */
  .se-shell {
    grid-template-columns: 44px minmax(0, 1fr) 44px;
    transition: grid-template-columns var(--dur-base) var(--ease-out);
  }
  .se-shell--with-side {
    grid-template-columns: 44px minmax(0, 1fr);
  }

  /* Quiet §3 mockup 4j — drop the grid, centre a single document that
     fills the height (the code surface owns its own scroll, unlike the
     scrolling inbox docs). Only the center EditorView renders; the
     ActivityBar / AgentDock / rail are not in the DOM at all, and the
     EditorView's own file-tree pane + splitter divider are hidden. */
  .se-shell--quiet {
    display: block;
    padding: 0;
    overflow: hidden;
  }
  .qeditor {
    width: 100%;
    max-width: 900px;
    margin: 0 auto;
    height: 100%;
    display: flex;
    flex-direction: column;
    min-height: 0;
    padding: 0 20px;
  }
  .qeditor-head {
    display: flex;
    align-items: baseline;
    gap: 8px;
    padding: 14px 2px 10px;
    flex: 0 0 auto;
  }
  .qeditor-name {
    font-size: 12.5px;
    font-weight: 600;
    color: var(--text-1);
  }
  .qeditor-instance {
    font-size: 11px;
    font-style: italic;
    color: var(--text-mute);
  }
  .qeditor-body {
    flex: 1;
    min-height: 0;
    display: flex;
  }
  .qeditor-body :global(.ev) {
    flex: 1;
    min-height: 0;
    width: 100%;
  }
  /* Strip EditorView's left column (file tree) + the internal divider —
     the Quiet document is a single buffer, so `.ev-main` (tabs + code)
     fills the whole width. `.s-end` already flexes, no width override
     needed (fixedSide='start' only writes an inline width on s-start). */
  .qeditor-body :global(.ev > .splitter > .s-start),
  .qeditor-body :global(.ev > .splitter > .s-divider) {
    display: none;
  }
  /* Splitter snippets render bare into the panes — let them stretch
     to fill the available pixels in each side of the splitter. */
  .se-shell :global(.s-start),
  .se-shell :global(.s-end) {
    height: 100%;
    display: flex;
    min-width: 0;
  }
  .se-shell :global(.s-start) > :global(*),
  .se-shell :global(.s-end) > :global(*) {
    flex: 1 1 auto;
    width: 100%;
    min-width: 0;
  }

  /* Activity pane — narrow 44px column. The pane chrome (border + shadow)
     comes from `.app-pane`; this rule just lets ActivityBar fill it. */
  .se-activity {
    overflow: visible;
  }

  /* Center pane — editor area fills the column. Without `flex: 1`
     on `.se-editor-area`, it auto-sized to EditorView's content
     height (file tree + open buffer) and left a black gap below
     the status bar all the way to the window's bottom. */
  .se-center {
    display: flex; flex-direction: column;
    min-height: 0;
    height: 100%;
    position: relative;
    overflow: hidden;
  }
  .se-editor-area {
    flex: 1;
    display: flex;
    min-height: 0; min-width: 0;
    overflow: hidden;
  }
  /* EditorView root = .ev — fill the whole area. */
  .se-editor-area :global(.ev) {
    flex: 1; min-height: 0; width: 100%;
  }

  .se-inline {
    overflow: hidden;
  }
  .se-inline :global(.ic) {
    width: 100%; height: 100%;
  }
</style>
