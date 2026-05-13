<script lang="ts">
  /* EditorApp — VS Code-class workspace.
     Layout (mockup v6):
       [activity 44] [editor (flex)] [inline-claude 280]

     Center editor reuses the existing <EditorView> — a low-level
     CodeMirror wrapper (file tree + tabs + code) — for ~900 lines of
     editor plumbing. New peers: ActivityBar / InlineClaude as
     standalone components under lib/views/apps/editor/. */

  import EditorView from '$lib/components/editor/EditorView.svelte';
  import ActivityBar from './editor/ActivityBar.svelte';
  import InlineClaude from './editor/InlineClaude.svelte';
  import Splitter from '$lib/components/ui/Splitter.svelte';
  import SidePaneRail from '$lib/components/ui/SidePaneRail.svelte';
  import { sessionsState, getPendingEditEvents } from '$lib/state/sessions.svelte';
  import { kindForInstanceId, APP_INSTANCE_IDS, layoutState } from '$lib/state/layout.svelte';
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
  }
  let p: Props = $props();

  let activityTab = $state<ActivityTab>('explorer');

  /** Inline-Claude pane open state. Persisted per editor instance —
   *  Vermeer/Hopper/etc remember whether the user prefers the pane
   *  hidden (more chrome for code) or shown (one-glance to chat).
   *  Default = true so first-run users discover the pane exists. */
  // svelte-ignore state_referenced_locally
  const sideStorageKey = `editor-claude-side-open:${p.instanceId}`;
  let claudeSideOpen = $state(true);
  onMount(() => {
    const v = localStorage.getItem(sideStorageKey);
    if (v === '0' || v === '1') claudeSideOpen = v === '1';
  });
  $effect(() => {
    localStorage.setItem(sideStorageKey, claudeSideOpen ? '1' : '0');
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

  /** Link-picker entries — one row per Claude/Cursor session that is
   *  not already linked to this editor. The label is the session
   *  title so the user knows exactly which chat they're linking; if
   *  the agent has no sessions yet we still surface a single row with
   *  the kind name so the user can spawn-and-link in one click. */
  const agentInstances = $derived.by(() => {
    const out: { id: string; kind: 'claude' | 'cursor'; name: string; sessionId?: string }[] = [];
    /* Sessions, sorted: most-recently-active first per kind. */
    const sortByActivity = (a: typeof sessionsState.list[number], b: typeof sessionsState.list[number]) => {
      const ta = a.messages[a.messages.length - 1]?.at ?? '';
      const tb = b.messages[b.messages.length - 1]?.at ?? '';
      return tb.localeCompare(ta);
    };
    for (const kind of ['claude', 'cursor'] as const) {
      const colId = kind === 'claude' ? APP_INSTANCE_IDS.claude : APP_INSTANCE_IDS.cursor;
      const sessions = sessionsState.list
        .filter((s) => s.agentKind === kind)
        .sort(sortByActivity);
      if (sessions.length === 0) {
        out.push({ id: colId, kind, name: kind === 'claude' ? 'Claude' : 'Cursor' });
      } else {
        for (const s of sessions) {
          /* Skip sessions that already point at this editor — listing
             them would mean "link the linked", which is a no-op. */
          if (s.linkedToEditor && s.linkedToEditorInstanceId === p.instanceId) continue;
          out.push({ id: colId, kind, name: s.title || 'Untitled chat', sessionId: s.id });
        }
      }
    }
    return out;
  });

  /** Sessions linked TO this editor (для Link chips в EditorView header). */
  const linkedAgents = $derived.by(() => {
    const out: { sessionId: string; agentInstanceId: string; kind: 'claude' | 'cursor'; name: string }[] = [];
    for (const s of sessionsState.list) {
      if (!s.linkedToEditor) continue;
      if (s.linkedToEditorInstanceId !== p.instanceId) continue;
      if (!s.agentInstanceId) continue;
      const kind = kindForInstanceId(s.agentInstanceId);
      if (kind !== 'claude' && kind !== 'cursor') continue;
      out.push({ sessionId: s.id, agentInstanceId: s.agentInstanceId, kind, name: s.title });
    }
    return out;
  });

  function unlinkSession(sessionId: string) {
    const s = sessionsState.list.find((x) => x.id === sessionId);
    if (!s) return;
    s.linkedToEditor = false;
    s.linkedToEditorInstanceId = null;
  }

  /** Git change count → badge на activity-bar Git button. Будет реальным
   *  когда подключим git_status в EditorApp. На MVP — 0. */
  const gitCount = 0;
  /** Problems count → badge на activity-bar Tests + bottom Problems
   *  tab. На MVP — 0 (typecheck integration в следующем milestone). */
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
  class:se-shell--with-side={claudeSideOpen}
  style="--app-tone: var(--src-editor); --app-glow: rgba(204,120,92,0.42);"
>
  <div class="app-pane se-activity">
    <ActivityBar
      activeTab={activityTab}
      onPick={pickActivity}
      onOpenSettings={p.onOpenSettings}
      {gitCount}
      {problemsCount}
      {reviewCount}
    />
  </div>

  {#if claudeSideOpen}
    <!-- Splitter between the editor center and the InlineClaude pane.
         User-resizable; width persists per-instance under
         `editor-inline:<instanceId>` so each Vermeer / Rothko keeps
         its own preferred reading layout across reloads. -->
    <Splitter
      direction="horizontal"
      fixedSide="end"
      persistKey="editor-inline:{p.instanceId}"
      initial={320}
      min={240}
      max={560}
    >
      {#snippet start()}
        <section class="app-pane se-center">
          <div class="se-editor-area">
            <EditorView
              bind:repoPath
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
          <InlineClaude
            instanceId={p.instanceId}
            linkKind="editor"
            onClose={() => (claudeSideOpen = false)}
            onOpenClaude={p.onOpenClaude}
            onQuickSend={p.onQuickSend}
            onOpenSession={p.onOpenSession}
          />
        </aside>
      {/snippet}
    </Splitter>
  {:else}
    <section class="app-pane se-center">
      <div class="se-editor-area">
        <EditorView
          bind:repoPath
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
    <!-- Skinny rail (52px). Shows expand-button + one square per
         linked agent so the user always knows which chats are
         attached AND can pop straight to one without expanding the
         whole pane first. -->
    <div class="se-rail-slot" in:fly={{ x: 24, duration: 220, easing: cubicOut }}>
      <SidePaneRail
        linkedAgents={linkedAgents.map((la) => ({
          sessionId: la.sessionId,
          agentInstanceId: la.agentInstanceId,
          kind: la.kind,
          title: la.name
        }))}
        {reviewCount}
        onExpand={() => (claudeSideOpen = true)}
      />
    </div>
  {/if}
</section>

<style>
  /* Two grid layouts:
     - open: 44px ActivityBar + Splitter cell (editor + inline pane).
     - rail-collapsed: 44px ActivityBar + editor (1fr) + 52px rail-mini.
     The rail-mini holds linked-agent icons so the user can see at a
     glance WHO's attached even when the chat pane is hidden. */
  .se-shell {
    grid-template-columns: 44px 1fr 52px;
    transition: grid-template-columns var(--dur-base) var(--ease-out);
  }
  .se-shell--with-side {
    grid-template-columns: 44px minmax(0, 1fr);
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
  .se-activity :global(.eab) {
    width: 100%; height: 100%;
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
  }

  /* Slot for the shared SidePaneRail when the InlineClaude pane is
     collapsed. Width matches the rail itself (52px) — sized in the
     `.se-shell` grid template. */
  .se-rail-slot { height: 100%; min-width: 0; }
  .se-rail-slot :global(.spr) { width: 100%; }
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
