<script lang="ts">
  import { onMount, onDestroy, tick } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import { open as openDialog } from '@tauri-apps/plugin-dialog';
  import Sigil from '$lib/components/ui/Sigil.svelte';
  import Cheatsheet from '$lib/components/ui/Cheatsheet.svelte';
  import Welcome from '$lib/components/ui/Welcome.svelte';
  import { welcomeState } from '$lib/state/welcome.svelte';
  import WorktreeDiffModal from '$lib/components/editor/WorktreeDiffModal.svelte';
  import JiraDetailPane from '$lib/components/inbox/JiraDetailPane.svelte';
  import SentryDetailPane from '$lib/components/inbox/SentryDetailPane.svelte';
  import GithubFocusOverlay from '$lib/components/inbox/GithubFocusOverlay.svelte';
  import Rail from '$lib/components/ui/Rail.svelte';
  import RulesView from '$lib/views/RulesView.svelte';
  import ConnectionsView from '$lib/views/ConnectionsView.svelte';
  import SettingsView from '$lib/views/SettingsView.svelte';
  import GithubTab from '$lib/views/GithubTab.svelte';
  import JiraTab from '$lib/views/JiraTab.svelte';
  import SentryTab from '$lib/views/SentryTab.svelte';
  import CommandPalette from '$lib/components/ui/CommandPalette.svelte';
  import ModalsRoot from '$lib/components/modals/ModalsRoot.svelte';
  import GithubColumn from '$lib/components/workbench/GithubColumn.svelte';
  import JiraColumn from '$lib/components/workbench/JiraColumn.svelte';
  import SentryColumn from '$lib/components/workbench/SentryColumn.svelte';
  import AgentColumn from '$lib/components/workbench/AgentColumn.svelte';
  import EditorColumn from '$lib/components/workbench/EditorColumn.svelte';
  import CanvasColumn from '$lib/components/workbench/CanvasColumn.svelte';
  import TerminalColumn from '$lib/components/workbench/TerminalColumn.svelte';
  import {
    restoreCanvasState,
    dropCanvasInstance,
    ensureCanvasLoaded,
    addShape as canvasAddShape,
    addShapes as canvasAddShapes,
    addEdge as canvasAddEdge,
    deleteShapes as canvasDeleteShapes,
    deleteEdges as canvasDeleteEdges,
    patchShape as canvasPatchShape,
    requestCanvasFocus,
    setShapeZ as canvasSetShapeZ,
    duplicateShapes as canvasDuplicateShapes,
    findShapesByQuery as canvasFindShapes,
    setSelection as canvasSetSelection,
    groupShapes as canvasGroupShapes,
    ungroupShapes as canvasUngroupShapes,
    setShapesLocked as canvasSetShapesLocked,
    alignShapes as canvasAlignShapes,
    distributeShapes as canvasDistributeShapes,
    setViewport as canvasSetViewport,
    type AlignAxis,
    type DistributeAxis,
    makeShape,
    makeEdge,
    canvasState,
    initCanvasFromDisk,
    type ShapeKind,
    type EdgeAnchor,
    type Shape,
    type Edge
  } from '$lib/state/canvas.svelte';
  import { applyLayout as canvasApplyLayout, type LayoutAlgorithm } from '$lib/services/canvasLayout';
  import { saveCanvasScreenshot } from '$lib/services/canvasScreenshot';
  import { buildAgentAppContext } from '$lib/services/agentContext';
  import { applySessionCwd } from '$lib/services/sessionCwd';
  import { runCompactSession as runCompactSessionService } from '$lib/services/agentCompact';
  import { exportSessionMarkdown, exportSessionJson } from '$lib/services/sessionExport';
  import {
    parseSlashCommand,
    clearSessionHistory,
    appendUsageBreakdown,
    appendSlashHelp
  } from '$lib/services/slashCommands';
  import { openFileInEditor } from '$lib/services/editorNavigation';
  import { refreshPlanUsage } from '$lib/state/quota.svelte';
  import {
    layoutState,
    persistPanelState,
    restorePanelState,
    scrollInstanceIntoView,
    scrollKindIntoView,
    addPanelInstance,
    addWorkbench,
    removeWorkbench,
    renameWorkbench,
    setActiveWorkbench,
    activeInstances,
    activeWorkbench,
    firstInstanceOfKind,
    findInstanceAnywhere,
    listInstancesOfKind,
    listArchivedOfKind,
    unarchiveInstance,
    goToInstance,
    moveInstanceToWorkbench,
    registerInstanceRemovedHook,
    restoreMaximized,
    toggleMaximize
  } from '$lib/state/layout.svelte';
  import {
    sessionsState,
    persistSessionsEffect,
    persistRulesEffect,
    persistEditorInstanceStateEffect,
    initSessionsFromDisk,
    newClaudeSession,
    deleteClaudeSession,
    updateSession,
    focusSession,
    appendSessionMessage,
    appendToLastAssistant,
    replaceLastAssistant,
    addAction,
    updateAction,
    removeAction,
    truncateSessionAt,
    setSessionInput,
    attachPathsToSession,
    setActiveSessionInColumn,
    orphanSessionsForInstance,
    genId,
    genUuid
  } from '$lib/state/sessions.svelte';
  import {
    connectionsState,
    sourceConns,
    agentConns,
    refreshGithubStatus,
    refreshJiraStatus,
    refreshSentryStatus,
    refreshClaudeStatus,
    refreshAllStatusOnBoot
  } from '$lib/state/connections.svelte';
  import {
    markTokenInstalled,
    clearTokenInstalled
  } from '$lib/state/tokenAge.svelte';
  import {
    inboxState,
    refreshInbox,
    refreshJiraInbox,
    refreshSentryInbox,
    refreshAllInboxes,
    refreshAllJiraInboxes,
    refreshAllSentryInboxes,
    resetSentryInbox,
    loadDetail,
    reloadDetailAndLists as reloadDetailAndListsCore,
    selectInboxItem,
    openFocusItem,
    openSentryFocus,
    closeFocusItem,
    moveSelection,
    toggleFile,
    openUserPicker,
    onUserPickerInput,
    selectAssignee,
    selectAnyAssignee,
    resetGithubInbox,
    resetJiraInbox,
    setGithubMeLogin,
    updateGithubFilters,
    updateJiraFilters,
    setSentryFilters,
    updateJiraTabFilters,
    scheduleSentryTabFilterRefresh
  } from '$lib/state/inbox.svelte';
  import type {
    GithubFilters,
    GithubFilterMode,
    JiraFilters,
    SprintScope
  } from '$lib/state/inbox.svelte';
  import { initTheme } from '$lib/state/theme.svelte';
  import { initScale } from '$lib/state/scale.svelte';
  import { dragState, setDragPayload, type DragPayload } from '$lib/state/drag.svelte';
  import { attachDragChip } from '$lib/dragImage';
  import { notify, notifyError } from '$lib/state/toaster.svelte';
  import {
    modalsState,
    openModal,
    closeModal,
    patchModal,
    type ReviewEvent,
    type MergeMethod
  } from '$lib/state/modals.svelte';
  import { appHasFocus, notifyClaudeRunComplete } from '$lib/notifications';
  import { effectiveCwd, dispatchAction } from '$lib/exec/actions';
  import { runAgentRequest, stopAgentRequest } from '$lib/exec/claude';
  import type { ClaudeAction, ClaudeSession, Mention, PanelInstance, PanelKind, RepoInfo } from '$lib/types';
  import {
    connectionsMeta,
    externalId,
    type ClaudeStatus,
    type CommitDetail,
    type CommitEntry,
    type CompareResult,
    type ConnectionMeta,
    type GithubUser,
    type InboxItem,
    type JiraIssueType,
    type JiraItem,
    type JiraProject,
    type JiraSprint,
    type JiraUser,
    type JiraUserSummary,
    type SentryUser,
    type RepoBranch,
    type Repository
  } from '$lib/data';
  import { basename, formatToolUse, isImagePath, truncInline } from '$lib/format';

  type View = 'workbench' | 'githubTab' | 'jiraTab' | 'sentryTab' | 'rules' | 'connections' | 'settings';
  type DetailTab = 'conversation' | 'commits' | 'files' | 'reviews' | 'checks';

  // View & layout state
  let view = $state<View>('workbench');
  let paletteOpen = $state(false);
  /* Cheatsheet overlay (`?` toggles). Owned at +page level so any
   * shortcut, anywhere, can flip it without prop-drilling. */
  let cheatsheetOpen = $state(false);
  let tab = $state<DetailTab>('conversation');

  // Biometric gate. Before first unlock the UI shows a "Locked" overlay so
  // credentials in keychain can't be pulled until the user taps Touch ID
  // (or confirms with the Mac passcode). `biometryError` surfaces the last
  // LAContext failure so we can show "cancelled" / "not enrolled" etc.
  let appLocked = $state(true);
  let biometryInFlight = $state(false);
  let biometryError = $state<string | null>(null);

  async function biometricUnlock() {
    if (biometryInFlight) return;
    biometryInFlight = true;
    biometryError = null;
    try {
      await invoke('biometric_unlock', { reason: 'Unlock Forgehold to access your stored credentials' });
      appLocked = false;
      // Now that we're allowed through, pull connection status + inboxes.
      // `refreshAllStatusOnBoot` wraps each source's status call in an
      // exponential-backoff retry (0/2/6/14 s) so a single network
      // blip on launch doesn't leave a connected source reading as
      // disconnected. The first attempt fires immediately; we don't
      // wait for retries to start dependent fetches.
      await refreshAllStatusOnBoot();
      if (connectedGithub) void refreshAllInboxes();
      if (connectedJira) void refreshAllJiraInboxes();
      if (connectedSentry) void refreshAllSentryInboxes();
    } catch (e) {
      biometryError = typeof e === 'string' ? e : String(e);
    } finally {
      biometryInFlight = false;
    }
  }

  // Live clock — kept in +page.svelte because it's a cross-cutting timer that
  // drives every relative-time label in the app (inbox, jira, chat, detail,
  // commit modal, …) not just the inbox. The inbox store reads it as a prop.
  let now = $state(Date.now());

  // GitHub-tab state lives in GithubTab now; parent keeps a handle
  // via `bind:this` so cross-cutting actions (e.g. merging a PR) can refresh
  // the repo items list.
  let repositoriesView = $state<{ refreshItems: () => void } | null>(null);

  // Shared editor repo path. In multi-instance world we have one path per
  // editor column instance (see sessionsState.editorInstanceState). For
  // Claude sessions without an explicit cwd, we fall back to the FIRST editor
  // instance in panel order. Kept as a $derived so effects fire on change.
  // Migrated legacy single-editor-root localStorage key into the first editor
  // instance's slot on mount.
  const editorRepoPath = $derived.by(() => {
    const first = activeInstances().find((i) => i.kind === 'editor');
    return first ? sessionsState.editorInstanceState[first.id]?.repoPath ?? '' : '';
  });

  function setEditorRepoPath(value: string, instanceId?: string) {
    const id =
      instanceId ?? activeInstances().find((i) => i.kind === 'editor')?.id ?? null;
    if (!id) return;
    if (!sessionsState.editorInstanceState[id]) {
      sessionsState.editorInstanceState[id] = { repoPath: value };
    } else {
      sessionsState.editorInstanceState[id].repoPath = value;
    }
  }


  // ---- Drag autoscroll for card DnD (Jira/GitHub → Claude column) ----
  // When the user grabs a card and drags it toward an off-screen column,
  // the workbench auto-scrolls so the drop target comes into view.
  let dragPointerX = $state(0);
  let dragAutoscrollRaf: number | null = null;

  function trackDragPointer(e: DragEvent) {
    dragPointerX = e.clientX;
    if (dragAutoscrollRaf === null && dragState.payload) {
      dragAutoscrollRaf = requestAnimationFrame(dragAutoscrollStep);
    }
  }

  function dragAutoscrollStep() {
    if (!dragState.payload) { dragAutoscrollRaf = null; return; }
    const wb = document.querySelector('.wb-columns') as HTMLElement | null;
    if (!wb) { dragAutoscrollRaf = null; return; }
    const rect = wb.getBoundingClientRect();
    const vw = window.innerWidth || document.documentElement.clientWidth;
    const effectiveRight = Math.min(rect.right, vw);
    const effectiveLeft = Math.max(rect.left, 0);
    const edge = 100;
    const maxStep = 26;
    let dx = 0;
    if (dragPointerX > effectiveRight - edge) {
      dx = Math.min(maxStep, Math.max(4, Math.round((dragPointerX - (effectiveRight - edge)) / 3)));
    } else if (dragPointerX > 0 && dragPointerX < effectiveLeft + edge) {
      dx = -Math.min(maxStep, Math.max(4, Math.round(((effectiveLeft + edge) - dragPointerX) / 3)));
    }
    if (dx !== 0) {
      wb.scrollLeft = Math.max(0, Math.min(wb.scrollWidth - wb.clientWidth, wb.scrollLeft + dx));
    }
    dragAutoscrollRaf = requestAnimationFrame(dragAutoscrollStep);
  }

  function stopDragAutoscroll() {
    if (dragAutoscrollRaf !== null) cancelAnimationFrame(dragAutoscrollRaf);
    dragAutoscrollRaf = null;
  }


  // Drag state lives in `$lib/state/drag.svelte` so other components
  // (FileTree, etc.) can write into the same payload without prop-drilling.
  // Event handlers below read `dragState.payload` directly (not a $derived
  // alias) so the read is always against the live module state.
  // Per-instance drop highlight. Only one column at a time gets highlighted
  // while a card is hovered — two Claude columns could both accept the drop
  // but we track the *current* target, not "any Claude column".
  let dragOverInstanceId = $state<string | null>(null);
  // Set true briefly when a drag completes so the subsequent synthetic
  // click doesn't open the slide-over PR detail.
  let justDragged = $state(false);
  // Track mousedown position to distinguish a real click from a drag release.
  let mouseDownPt: { x: number; y: number } | null = null;
  function onCardMouseDown(e: MouseEvent) {
    mouseDownPt = { x: e.clientX, y: e.clientY };
  }
  function isClickNotDrag(e: MouseEvent): boolean {
    if (justDragged) return false;
    if (mouseDownPt) {
      const dx = e.clientX - mouseDownPt.x;
      const dy = e.clientY - mouseDownPt.y;
      mouseDownPt = null;
      if (Math.sqrt(dx * dx + dy * dy) > 6) return false;
    }
    return true;
  }

  // ClaudeMessage, Mention, ClaudeSession, ClaudeAction and RepoInfo
  // are imported from $lib/types so the workbench column components can
  // share the same shapes.

  let activeRepoInfo = $state<RepoInfo | null>(null);

  // Session persistence — sessions store handles the heavy lifting; we just
  // have to wire the $effect calls inside the component's effect scope.
  persistSessionsEffect();
  persistRulesEffect();
  persistEditorInstanceStateEffect();

  const activeSession = $derived(
    sessionsState.list.find((s) => s.id === sessionsState.activeClaudeId) ?? null
  );

  // Thinking-time label for the typing indicator
  let thinkingStartedAt = $state<number | null>(null);
  let thinkingTick = $state(0);
  let thinkingTimer: ReturnType<typeof setInterval> | null = null;

  // Auto-create initial chat + Claude column when Claude connects for the
  // first time and we have neither sessions nor a claude panel. Do NOT wipe
  // on disconnect — persisted sessions and panels should survive.
  $effect(() => {
    if (connectedClaude && sessionsState.list.length === 0) {
      let claude = firstInstanceOfKind('claude');
      if (!claude) {
        const id = addPanelInstance('claude');
        claude = activeInstances().find((i) => i.id === id) ?? null;
      }
      newClaudeSession({ title: 'Chat 1', columnInstanceId: claude?.id ?? null });
    }
  });

  // First-run: if a connected user's active workbench has zero instances,
  // seed reasonable defaults (github/jira/claude) so the workbench isn't
  // empty on a fresh install.
  $effect(() => {
    const inst = activeInstances();
    if (inst.length > 0) return;
    if (!connectedGithub && !connectedJira && !connectedSentry && !connectedClaude) return;
    if (connectedGithub) addPanelInstance('github');
    if (connectedJira) addPanelInstance('jira');
    if (connectedSentry) addPanelInstance('sentry');
    if (connectedClaude) addPanelInstance('claude');
  });

  // Modal payloads live in the registry — see `$lib/state/modals.svelte`.
  // Local aliases keep the template + handler code readable; mutations go
  // through `openModal` / `closeModal` / `patchModal`.
  const patModal = $derived(modalsState.pat);
  const jiraModal = $derived(modalsState.jiraConnect);
  const claudeModal = $derived(modalsState.claudeStatus);
  const commentModal = $derived(modalsState.comment);
  const reviewModal = $derived(modalsState.review);
  const mergeModal = $derived(modalsState.merge);
  const commitModal = $derived(modalsState.commit);
  const confirmModal = $derived(modalsState.confirm);
  const jiraCreateModal = $derived(modalsState.jiraCreate);
  const githubCreatePrModal = $derived(modalsState.githubCreatePr);
  let actionBusy = $state<string | null>(null);

  // Derived — reading from the connections store. `$derived` re-runs when
  // reactive state inside its expression changes, so touching `connectionsState`
  // is enough to re-compute. Short aliases keep the template readable.
  const githubStatus = $derived(connectionsState.github);
  const jiraStatus = $derived(connectionsState.jira);
  const sentryStatus = $derived(connectionsState.sentry);
  const claudeStatus = $derived(connectionsState.claude);
  const cursorStatus = $derived(connectionsState.cursor);
  const statusLoading = $derived(connectionsState.statusLoading);
  /* `anyRetrying` is true while the boot retry/backoff loop has at
   *  least one source mid-attempt after a transient failure. Rail
   *  uses it to render a pulsing "retrying" dot in place of the plain
   *  disconnected dot — distinguishes "nothing connected" from
   *  "trying to connect, network was flaky on launch". */
  const anyRetrying = $derived(
    connectionsState.retrying.github ||
      connectionsState.retrying.jira ||
      connectionsState.retrying.sentry ||
      connectionsState.retrying.claude ||
      connectionsState.retrying.cursor
  );

  /* Top-level palette actions (M4 §2.8.6 — action verbs). Built as a
   * derived so connect/disconnect labels flip based on the live
   * `connectionsState` — typing "connect github" surfaces the connect
   * verb when disconnected and the disconnect verb when already on. */
  const paletteActions = $derived.by(() => {
    type PA = { id: string; label: string; sub?: string; keywords?: string; pick: () => void };
    const a: PA[] = [];
    /* Source connect / disconnect. Use the connectionsMeta source list
     * so this stays in sync if a new source is added. */
    for (const conn of sourceConns) {
      if (!conn.implemented) continue;
      const status = connectionsState[conn.id as 'github' | 'jira' | 'sentry'];
      const isConnected = status?.kind === 'connected';
      a.push({
        id: `connect:${conn.id}`,
        label: isConnected ? `Reconnect ${conn.name}` : `Connect ${conn.name}`,
        sub: isConnected ? 'Re-enter token in the modal' : 'Open the connect modal',
        keywords: `${conn.id} pat token auth`,
        pick: () => openConnectModal(conn)
      });
      if (isConnected) {
        a.push({
          id: `disconnect:${conn.id}`,
          label: `Disconnect ${conn.name}`,
          sub: 'Drop the token from Keychain',
          keywords: `${conn.id} sign out logout`,
          pick: () => {
            if (conn.id === 'github') void disconnectGithub();
            else if (conn.id === 'jira') void disconnectJiraAll();
            else if (conn.id === 'sentry') void disconnectSentryAll();
          }
        });
      }
    }
    /* Agents — open status modals so the user can verify the binary
     * is detected. */
    for (const conn of agentConns) {
      if (!conn.implemented) continue;
      a.push({
        id: `status:${conn.id}`,
        label: `Check ${conn.name} status`,
        sub: 'Detect binary + version',
        keywords: `${conn.id} cli agent`,
        pick: () => openConnectModal(conn)
      });
    }
    a.push({
      id: 'cheatsheet',
      label: 'Show keyboard shortcuts',
      sub: 'Cheatsheet of every binding',
      keywords: 'help ? shortcuts hotkeys',
      pick: () => (cheatsheetOpen = true)
    });
    a.push({
      id: 'workbench:new',
      label: 'New workbench',
      sub: 'Create a fresh column tab',
      keywords: 'create add tab',
      pick: () => {
        const id = addWorkbench('Workbench ' + (layoutState.workbenches.length + 1));
        setActiveWorkbench(id);
        view = 'workbench';
      }
    });
    a.push({
      id: 'view:settings',
      label: 'Open settings',
      keywords: 'preferences config theme privacy updates docs',
      pick: () => (view = 'settings')
    });
    a.push({
      id: 'view:connections',
      label: 'Open connections',
      keywords: 'sources tokens auth',
      pick: () => (view = 'connections')
    });
    a.push({
      id: 'view:rules',
      label: 'Open rules',
      keywords: 'system prompt agent',
      pick: () => (view = 'rules')
    });
    return a;
  });
  const connectedGithub = $derived(githubStatus.kind === 'connected');
  const connectedJira = $derived(jiraStatus.kind === 'connected');
  const connectedSentry = $derived(sentryStatus.kind === 'connected');
  const connectedClaude = $derived(claudeStatus?.ready ?? false);
  const connectedCursor = $derived(cursorStatus?.ready ?? false);
  const connectedIds = $derived.by(() => {
    const set = new Set<string>();
    if (connectedGithub) set.add('github');
    if (connectedJira) set.add('jira');
    if (connectedSentry) set.add('sentry');
    if (connectedClaude) set.add('claude');
    if (connectedCursor) set.add('cursor');
    return set;
  });
  const anythingConnected = $derived(connectedIds.size > 0);

  let githubPollInterval: ReturnType<typeof setInterval> | null = null;
  let jiraPollInterval: ReturnType<typeof setInterval> | null = null;
  let sentryPollInterval: ReturnType<typeof setInterval> | null = null;
  let tickInterval: ReturnType<typeof setInterval> | null = null;

  // Wire the layout→sessions hook once. Any closed panel instance (via the X
  // button or workbench deletion) orphans its pinned sessions back to the
  // floating pool so they reattach elsewhere instead of vanishing.
  registerInstanceRemovedHook((id) => {
    orphanSessionsForInstance(id);
    /* Canvas columns own per-instance tab state (which canvases are pinned
       to this column, which one is active). Closing the column drops that
       map entry — but does NOT delete the canvases themselves; they stay
       in the library so a future canvas column can reopen them. */
    dropCanvasInstance(id);
  });

  // Auto-clear `awaitingApproval` when the user dismisses every pending
  // action card without approving any. Otherwise the "waiting for
  // approval" hint would stay visible forever and the next user message
  // would unnecessarily re-clear it. Only clears when there are no more
  // actions in any state (pending / executing / done / error). Keeps
  // the flag while done/error cards are still on screen so a chained
  // continuation can still see them in `onActionResolved`.
  $effect(() => {
    for (const sess of sessionsState.list) {
      if (sess.awaitingApproval && sess.actions.length === 0) {
        updateSession(sess.id, { awaitingApproval: false });
      }
    }
  });

  onMount(async () => {
    /* Re-apply the persisted theme on boot — the SSR shell rendered
       with default `:root` vars, this flips `<html data-theme="…">`
       so the saved palette wins on first paint. */
    initTheme();
    // Plan-usage snapshot for the chip. Fire-and-forget — the chip
    // shows "—" until the first response comes back, after which
    // refreshPlanUsage is debounced to MIN_REFRESH_MS (60s) so any
    // post-turn re-fetch is effectively free.
    void refreshPlanUsage();
    // Same boot-time pattern as theme: apply the saved zoom level to
    // <html> before first paint so the layout doesn't briefly flash
    // at 100% then jump.
    initScale();
    restorePanelState();
    /* Canvas state is persisted alongside layout state — index of canvases
       + per-column-instance tab strip. Hydrate after layout so instance ids
       in the canvas store still match live columns. */
    restoreCanvasState();
    // Migrate / load sessions from disk. Resolves app-data dir once and
    // passes it to initSessionsFromDisk which handles localStorage →
    // disk migration on first run, then switches persist to disk-only.
    // Runs before biometric unlock so sessions are ready when the lock
    // screen clears.
    const appDataDir = await invoke<string>('app_data_dir');
    cachedAppDataDir = appDataDir;
    await initSessionsFromDisk(appDataDir);
    /* Same migration shape for canvases (M1 §1.1). `restoreCanvasState`
       above already populated `canvasState.index` and `byInstance`
       from localStorage; `initCanvasFromDisk` either upgrades that
       layout to per-canvas JSON files on disk, or — when an
       `index.json` is already present — re-hydrates from disk
       (which IS now the source of truth) and clears the legacy
       `forgehold:canvas:v1:*` localStorage keys to free origin
       quota. Failures fall back to localStorage transparently. */
    await initCanvasFromDisk(appDataDir);
    // One-shot v1 → v2 migration: seed the legacy `forgehold:editor:root`
    // localStorage value into the first editor instance, ONLY if that
    // instance has no persisted v2 state yet. Without this guard the
    // migration would clobber the editor's open folder on every reload
    // (v2 persistence already restored the right path; the legacy key
    // would re-overwrite it back to the original v1 value).
    try {
      const savedEditorRoot = localStorage.getItem('forgehold:editor:root');
      if (savedEditorRoot) {
        const ed = firstInstanceOfKind('editor');
        const alreadyHasV2 = ed
          ? !!sessionsState.editorInstanceState[ed.id]?.repoPath
          : false;
        if (ed && !alreadyHasV2) {
          setEditorRepoPath(savedEditorRoot, ed.id);
        }
        // Drop the legacy key once we know v2 is in place — keeps
        // re-mounts cheap and prevents future regressions of this kind.
        if (alreadyHasV2) localStorage.removeItem('forgehold:editor:root');
      }
    } catch {/* ignore */}
    tickInterval = setInterval(() => (now = Date.now()), 30_000);
    // Biometric gate runs first — refreshAllStatus + inbox fetches live
    // inside `biometricUnlock` so nothing hits the keychain before the
    // user authenticates.
    void biometricUnlock();
    // OS-level file drops (Finder → app) come in as standard DOM `drop`
    // events because `dragDropEnabled: false` in tauri.conf.json — Tauri's
    // native drag handler is disabled so HTML5 drag-and-drop works
    // properly in WKWebView for both internal drags and external files.
    // `onAgentDrop` parses the `text/uri-list` mime for the OS path case.

    // Auto-clean orphan worktrees > 14 days old. Fire-and-forget — the
    // user sees results in Settings → Storage. We pass the live session
    // ID list so worktrees still attached to a chat are kept regardless
    // of age. Runs once per app launch, on a small delay so the more
    // important UI work (auth, inbox) gets the main-thread first.
    setTimeout(() => {
      const ids = sessionsState.list.map((s) => s.id);
      void invoke<{ removed: number; bytes_freed: number }>(
        'worktree_cleanup_orphans',
        { activeSessionIds: ids, maxAgeSecs: 14 * 24 * 60 * 60 }
      ).then((s) => {
        if (s.removed > 0) {
          notify({
            kind: 'info',
            title: `Cleaned ${s.removed} orphan worktree${s.removed === 1 ? '' : 's'}`,
            body: `Freed ${formatBytesShort(s.bytes_freed)} of disk · older than 14 days, no live chat.`
          });
        }
      }).catch(() => {/* silent — Settings has manual button */});
    }, 8000);
  });

  function formatBytesShort(b: number): string {
    if (b < 1024 * 1024) return `${(b / 1024).toFixed(0)} KB`;
    if (b < 1024 * 1024 * 1024) return `${(b / 1024 / 1024).toFixed(1)} MB`;
    return `${(b / 1024 / 1024 / 1024).toFixed(2)} GB`;
  }

  onDestroy(() => {
    if (githubPollInterval) clearInterval(githubPollInterval);
    if (jiraPollInterval) clearInterval(jiraPollInterval);
    if (sentryPollInterval) clearInterval(sentryPollInterval);
    if (tickInterval) clearInterval(tickInterval);
  });

  $effect(() => {
    // Feed the authed GitHub login to the inbox store so query-builders can
    // substitute it into `author:`, `assignee:` etc. (@me only works in
    // *some* contexts on the search API — using the literal login is the
    // safe path).
    setGithubMeLogin(githubStatus.kind === 'connected' ? githubStatus.user.login : null);
  });

  // ---- Per-source independent polling ----
  // Each source owns its own 60 s / 5 min scheduler. No source gates another.

  $effect(() => {
    if (connectedGithub) {
      if (!githubPollInterval) {
        githubPollInterval = setInterval(() => {
          void refreshAllInboxes({ silent: true });
        }, 60_000);
      }
    } else {
      if (githubPollInterval) {
        clearInterval(githubPollInterval);
        githubPollInterval = null;
      }
      resetGithubInbox();
    }
  });

  $effect(() => {
    /* Auto-load Jira on connect for any column that hasn't fetched yet
       (per-instance state means each column owns its own list). */
    if (connectedJira) {
      const empty = Object.values(inboxState.jiraItemsByInstance).every(
        (list) => list.length === 0
      );
      const idle = Object.values(inboxState.jiraItemsLoadingByInstance).every(
        (loading) => !loading
      );
      if (empty && idle) {
        void refreshAllJiraInboxes({ silent: true });
      }
      if (!jiraPollInterval) {
        jiraPollInterval = setInterval(() => {
          void refreshAllJiraInboxes({ silent: true });
        }, 60_000);
      }
    } else {
      if (jiraPollInterval) {
        clearInterval(jiraPollInterval);
        jiraPollInterval = null;
      }
      // Only wipe the issue list on transient disconnects — keep the
      // user-picked assignee so reconnecting doesn't silently jump back to
      // "me". `resetJiraInbox` below is used by the explicit disconnect
      // button which *does* clear the assignee.
      inboxState.jiraItemsByInstance = {};
    }
  });

  $effect(() => {
    if (connectedSentry) {
      if (!sentryPollInterval) {
        sentryPollInterval = setInterval(() => {
          void refreshAllSentryInboxes({ silent: true });
        }, 300_000); // 5-minute default
      }
    } else {
      if (sentryPollInterval) {
        clearInterval(sentryPollInterval);
        sentryPollInterval = null;
      }
    }
  });

  // Re-fetch detail every time the overlay opens (i.e. focusItem flips
  // null→set). Previously we cached by (owner/repo/number) and never
  // re-fetched on subsequent opens — a PR you came back to after pushing
  // commits would still show the stale snapshot until you hit "Retry".
  // Now: clear the cache key on close, so the next open re-loads.
  // Switching directly between two PRs already re-loaded (different
  // keys); only the close→reopen-same-PR case was stale.
  let lastLoadedKey = $state<string | null>(null);
  $effect(() => {
    const focused = inboxState.focusItem;
    const key = focused
      ? `${focused.repo?.owner}/${focused.repo?.name}#${focused.number}`
      : null;
    if (!key) {
      // Closed — reset so the next open (even of the same PR) refetches.
      lastLoadedKey = null;
      return;
    }
    if (key !== lastLoadedKey) {
      lastLoadedKey = key;
      tab = 'conversation';
      void loadDetail();
    }
  });

  // ---- Drag handlers ----

  function onDragStart(payload: DragPayload, e: DragEvent) {
    setDragPayload(payload);
    if (e.dataTransfer) {
      e.dataTransfer.effectAllowed = 'copy';
      // text/plain is one of the few mime types WKWebView reliably exposes
      // on `dataTransfer.types` during dragover, so it doubles as the "yes
      // there is something here" signal for non-internal drop targets.
      if (payload.source === 'github') {
        e.dataTransfer.setData('text/plain', `#${payload.item.number}`);
        attachDragChip(e, 'github', `#${payload.item.number} · ${payload.item.title}`);
      } else if (payload.source === 'jira') {
        e.dataTransfer.setData('text/plain', payload.item.key);
        attachDragChip(e, 'jira', `${payload.item.key} · ${payload.item.summary}`);
      } else if (payload.source === 'sentry') {
        const ref = payload.item.short_id || payload.item.id;
        e.dataTransfer.setData('text/plain', ref);
        attachDragChip(e, 'sentry', `${ref} · ${payload.item.title}`);
      } else if (payload.source === 'file') {
        e.dataTransfer.setData('text/plain', payload.path);
        attachDragChip(e, payload.isDir ? 'dir' : 'file', payload.name);
      }
      /* `chat-message` payloads are dragstart-handled inside AgentColumn
         itself (which sets dragState directly). This `+page.svelte`
         path is for the inbox / file-tree drags; chat messages don't
         flow through `onDragStart` here. */
    }
    // Track pointer globally so we can auto-scroll .wb-columns when the
    // user drags a card near either edge.
    document.addEventListener('dragover', trackDragPointer);
  }

  function onDragEnd() {
    setDragPayload(null);
    clearAgentDragState();
    pillDragOverKind = null;
    pillDragOverInstance = null;
    if (pillDragLeaveTimer) {
      clearTimeout(pillDragLeaveTimer);
      pillDragLeaveTimer = null;
    }
    justDragged = true;
    setTimeout(() => (justDragged = false), 120);
    document.removeEventListener('dragover', trackDragPointer);
    stopDragAutoscroll();
  }

  /** Returns true if `e` carries a payload an agent column can accept —
   *  internal drag (ticket / file-tree row) or OS file drop. Used by
   *  dragenter, dragover, and drop alike so all three agree on accept. */
  function agentCanAccept(e: DragEvent): boolean {
    const types = e.dataTransfer?.types;
    if (dragState.payload) return true;
    if (types?.includes('application/x-forgehold-file')) return true;
    if (types?.includes('Files')) return true;
    if (types?.includes('text/uri-list')) return true;
    return false;
  }

  // Counter-per-instance for dragenter / dragleave. Without this, dragleave
  // fires every time the cursor crosses a child element (textarea, message
  // bubble, etc.) and the highlight flickers. The counter increments on
  // every dragenter and decrements on every dragleave; the column is
  // "drag-over" while the count is > 0.
  const agentDragCounts = new Map<string, number>();

  function onAgentDragEnter(instanceId: string, _kind: 'claude' | 'cursor', e: DragEvent) {
    if (!agentCanAccept(e)) return;
    e.preventDefault();
    const cur = agentDragCounts.get(instanceId) ?? 0;
    agentDragCounts.set(instanceId, cur + 1);
    if (cur === 0) dragOverInstanceId = instanceId;
  }

  function onAgentDragOver(instanceId: string, _kind: 'claude' | 'cursor', e: DragEvent) {
    if (!agentCanAccept(e)) return;
    // preventDefault on dragover is what *enables* the drop. Without it the
    // OS thinks the target rejected the drag and the cursor reads "no-drop".
    e.preventDefault();
    if (e.dataTransfer) e.dataTransfer.dropEffect = 'copy';
    // dragenter already set dragOverInstanceId; keep it sticky in case
    // dragenter didn't fire (first drag of session, recovered state).
    if (dragOverInstanceId !== instanceId) dragOverInstanceId = instanceId;
  }

  function onAgentDragLeave(instanceId: string) {
    const cur = agentDragCounts.get(instanceId) ?? 0;
    if (cur <= 1) {
      agentDragCounts.delete(instanceId);
      if (dragOverInstanceId === instanceId) dragOverInstanceId = null;
    } else {
      agentDragCounts.set(instanceId, cur - 1);
    }
  }

  function clearAgentDragState() {
    agentDragCounts.clear();
    dragOverInstanceId = null;
  }

  /** App-data path for chat image attachments (clipboard / Cmd+Shift+5 floating
      preview / direct File blob drop). Resolved lazily once and cached — the
      OS path is stable for the install. Lives under $APPDATA which is in the
      `assetProtocol.scope` so `convertFileSrc` can render thumbnails. */
  let cachedAppDataDir: string | null = null;
  async function getAttachmentDir(): Promise<string> {
    if (!cachedAppDataDir) {
      cachedAppDataDir = await invoke<string>('app_data_dir');
    }
    return `${cachedAppDataDir}/chat-attachments`;
  }

  /** Read a Blob/File as base64 (without the `data:...;base64,` prefix). Uses
      FileReader to avoid the `String.fromCharCode.apply` stack-overflow that
      bites on multi-MB images. */
  function blobToBase64(blob: Blob): Promise<string> {
    return new Promise((resolve, reject) => {
      const r = new FileReader();
      r.onload = () => {
        const s = String(r.result ?? '');
        const i = s.indexOf(',');
        resolve(i >= 0 ? s.slice(i + 1) : s);
      };
      r.onerror = () => reject(r.error);
      r.readAsDataURL(blob);
    });
  }

  /** Save a list of in-memory image blobs to disk + attach them to a session.
      Used for Files drops (Cmd+Shift+5 floating preview, drag from another
      browser tab) and clipboard paste — anywhere we have bytes but no source
      path. Sanitises the filename and prefixes a timestamp so two screenshots
      from the same minute don't collide. */
  async function attachBlobsToSession(sessionId: string, blobs: { name: string; type: string; blob: Blob }[]): Promise<number> {
    if (blobs.length === 0) return 0;
    const dir = await getAttachmentDir();
    const savedPaths: string[] = [];
    for (const item of blobs) {
      try {
        const b64 = await blobToBase64(item.blob);
        // Sanitise: drop slashes (path traversal), collapse whitespace, keep
        // unicode. Falls back to a generic name when the blob has none.
        const safe = (item.name || `image.${guessExt(item.type)}`)
          .replace(/[/\\]+/g, '_')
          .replace(/\s+/g, ' ')
          .trim();
        const stamp = `${Date.now()}-${Math.random().toString(36).slice(2, 7)}`;
        const path = `${dir}/${stamp}-${safe}`;
        await invoke('fs_write_bytes', { path, base64: b64 });
        savedPaths.push(path);
      } catch (err) {
        console.warn('attach blob failed', err);
      }
    }
    if (savedPaths.length === 0) return 0;
    return attachPathsToSession(sessionId, savedPaths);
  }

  function guessExt(mime: string): string {
    if (mime.includes('jpeg')) return 'jpg';
    if (mime.includes('gif')) return 'gif';
    if (mime.includes('webp')) return 'webp';
    return 'png';
  }

  /** Cmd+V of one or more images in a chat composer. Routes through the same
      blob → on-disk → mention pipeline as drag-drop, so the resulting
      attachment chip strip + transcript thumbnail look identical. */
  async function pasteImagesIntoColumn(
    instanceId: string,
    kind: 'claude' | 'cursor',
    blobs: { name: string; type: string; blob: Blob }[]
  ): Promise<number> {
    if (blobs.length === 0) return 0;
    // Resolve target the same way `onAgentDrop` does: active session in this
    // column, then any session bound here, then a fresh one of this kind.
    const activeId = sessionsState.activeByInstance[instanceId];
    let target = activeId ? sessionsState.list.find((s) => s.id === activeId) ?? null : null;
    if (!target) target = sessionsState.list.find((s) => s.columnInstanceId === instanceId) ?? null;
    if (!target) {
      const id = newClaudeSession({ agentKind: kind, columnInstanceId: instanceId });
      target = sessionsState.list.find((s) => s.id === id) ?? null;
    }
    if (!target) return 0;
    const n = await attachBlobsToSession(target.id, blobs);
    if (n > 0) setActiveSessionInColumn(instanceId, target.id);
    return n;
  }

  /** Pull image File blobs out of a DragEvent's dataTransfer.files. Used as
      a fallback for the Cmd+Shift+5 floating preview drag (which exposes
      Files but NO text/uri-list, so the OS-path branch above misses it). */
  function imageFilesFromEvent(e: DragEvent): { name: string; type: string; blob: Blob }[] {
    const out: { name: string; type: string; blob: Blob }[] = [];
    const files = e.dataTransfer?.files;
    if (!files || files.length === 0) return out;
    for (let i = 0; i < files.length; i++) {
      const f = files[i];
      if (f && (f.type.startsWith('image/') || /\.(png|jpe?g|gif|webp|bmp|svg|heic|heif|avif)$/i.test(f.name))) {
        out.push({ name: f.name, type: f.type || 'image/png', blob: f });
      }
    }
    return out;
  }

  function onAgentDrop(instanceId: string, kind: 'claude' | 'cursor', e: DragEvent) {
    e.preventDefault();

    // Pick (or create) the drop target: the active session in THIS column
    // instance. Falls back to any session bound to this instance, then a
    // floating session of this kind (adopted), then a fresh one.
    const pickTarget = (): ClaudeSession | null => {
      const activeId = sessionsState.activeByInstance[instanceId];
      let t = activeId ? sessionsState.list.find((s) => s.id === activeId) ?? null : null;
      if (!t) t = sessionsState.list.find((s) => s.columnInstanceId === instanceId) ?? null;
      if (!t) {
        // Adopt a floating session of the same kind if one exists.
        t = sessionsState.list.find(
          (s) => s.agentKind === kind && s.columnInstanceId === null
        ) ?? null;
        if (t) updateSession(t.id, { columnInstanceId: instanceId });
      }
      if (!t) {
        const id = newClaudeSession({ agentKind: kind, columnInstanceId: instanceId });
        t = sessionsState.list.find((s) => s.id === id) ?? null;
      }
      return t;
    };

    // 1) Internal drag (file from Editor tree, or ticket from inbox). The
    //    module-state payload is the primary signal; we also read the
    //    custom mime as a fallback in case the dragend handler raced ahead
    //    of this drop in some WKWebView edge cases.
    const internal = dragState.payload;
    let filePayload: { path: string; isDir: boolean; name: string } | null = null;
    if (internal && internal.source === 'file') {
      filePayload = { path: internal.path, isDir: internal.isDir, name: internal.name };
    } else {
      const raw = e.dataTransfer?.getData('application/x-forgehold-file');
      if (raw) {
        try {
          const p = JSON.parse(raw) as { path: string; isDir: boolean; name: string };
          if (p && typeof p.path === 'string') filePayload = p;
        } catch { /* malformed mime payload — ignore */ }
      }
    }
    if (filePayload) {
      const { path, isDir, name } = filePayload;
      const target = pickTarget();
      if (target) {
        // Prefer the session's cwd (set by user or the tree root) for a clean
        // relative path; fall back to absolute if the file isn't inside cwd.
        const cwd = target.cwd ?? '';
        const rel = cwd && path.startsWith(cwd + '/') ? path.slice(cwd.length + 1) : path;
        const display = '@' + rel + (isDir ? '/' : '');
        const mention: Mention = {
          source: 'file',
          externalId: rel + (isDir ? '/' : ''),
          title: name,
          body: path,
          isDir
        };
        const sep = target.input && !target.input.endsWith(' ') ? ' ' : '';
        updateSession(target.id, {
          input: target.input + sep + display + ' ',
          mentions: [...target.mentions, mention],
          // Auto-bind cwd to the repo that file lives in, if not already set.
          cwd: target.cwd ?? deriveCwd(path, isDir)
        });
        setActiveSessionInColumn(instanceId, target.id);
      }
      clearAgentDragState();
      setDragPayload(null);
      justDragged = true;
      setTimeout(() => (justDragged = false), 200);
      return;
    }

    // 2) OS file drop from Finder / Downloads / other apps. macOS (and most
    //    platforms) expose absolute paths via `text/uri-list` — a newline-
    //    separated list of `file://` URIs. The browser's `.files` property
    //    gives File blobs but no paths, so we parse URIs ourselves.
    const uriList = e.dataTransfer?.getData('text/uri-list') || '';
    if (uriList) {
      const paths = uriList
        .split(/\r?\n/)
        .map((l) => l.trim())
        .filter((l) => l && !l.startsWith('#') && l.startsWith('file://'))
        .map((u) => {
          try {
            return decodeURIComponent(u.replace(/^file:\/\//, ''));
          } catch {
            return '';
          }
        })
        .filter(Boolean);
      if (paths.length > 0) {
        const target = pickTarget();
        if (target) {
          const n = attachPathsToSession(target.id, paths);
          if (n > 0) setActiveSessionInColumn(instanceId, target.id);
        }
        clearAgentDragState();
        justDragged = true;
        setTimeout(() => (justDragged = false), 200);
        return;
      }
    }

    // 2.5) In-memory image File blobs. macOS Cmd+Shift+5 floating preview
    //    drag exposes the screenshot as a `File` in `dataTransfer.files` but
    //    omits `text/uri-list` (the file isn't necessarily on disk yet). Same
    //    happens for cross-tab drags from a browser. Save the bytes ourselves
    //    under $APPDATA/chat-attachments/ so we have a stable absolute path
    //    that asset:// can serve and the agent CLI can read.
    const imageBlobs = imageFilesFromEvent(e);
    if (imageBlobs.length > 0) {
      const target = pickTarget();
      if (target) {
        void attachBlobsToSession(target.id, imageBlobs).then((n) => {
          if (n > 0) setActiveSessionInColumn(instanceId, target.id);
        });
      }
      clearAgentDragState();
      justDragged = true;
      setTimeout(() => (justDragged = false), 200);
      return;
    }

    // 3) Ticket / Sentry drop from inbox. The file branch above already
    //    returned, so `internal` is one of the issue-shaped variants
    //    here. Chat-message payloads are also rejected — agent columns
    //    don't accept "drop a message onto myself" (we'd just create a
    //    self-reference loop). Chat messages drop onto Canvas only.
    if (!internal || internal.source === 'file' || internal.source === 'chat-message') {
      clearAgentDragState();
      return;
    }
    let mention: Mention;
    if (internal.source === 'github') {
      mention = {
        source: 'github',
        externalId: externalId(internal.item),
        title: internal.item.title,
        body: internal.item.body
      };
    } else if (internal.source === 'jira') {
      mention = {
        source: 'jira',
        externalId: internal.item.key,
        title: internal.item.summary,
        body: internal.item.description
      };
    } else {
      // Sentry — encode the short_id (or numeric id fallback) so Claude
      // can hand it to `mcp__sentry__get_issue` without further parsing.
      const issue = internal.item;
      const ref = issue.short_id || issue.id;
      const summary = [
        issue.metadata_type && issue.metadata_value
          ? `${issue.metadata_type}: ${issue.metadata_value}`
          : issue.title,
        issue.culprit ? `culprit: ${issue.culprit}` : null,
        `level: ${issue.level} · status: ${issue.status}`,
        `project: ${issue.project_slug} · last seen: ${issue.last_seen}`,
        issue.permalink ? `url: ${issue.permalink}` : null
      ]
        .filter(Boolean)
        .join('\n');
      mention = {
        source: 'sentry',
        externalId: ref,
        title: issue.title,
        body: summary
      };
    }

    const target = pickTarget();
    if (target) {
      const sep = target.input && !target.input.endsWith(' ') ? ' ' : '';
      updateSession(target.id, {
        input: target.input + sep + `@${mention.externalId} `,
        mentions: [...target.mentions, mention]
      });
      setActiveSessionInColumn(instanceId, target.id);
    }

    clearAgentDragState();
    setDragPayload(null);
    justDragged = true;
    setTimeout(() => (justDragged = false), 200);
  }

  /** If user drops a file before setting cwd, infer the enclosing directory. */
  function deriveCwd(path: string, isDir: boolean): string | null {
    if (isDir) return path;
    const idx = path.lastIndexOf('/');
    return idx > 0 ? path.slice(0, idx) : null;
  }

  async function pickCwd() {
    if (!activeSession) return;
    try {
      const picked = await openDialog({
        directory: true,
        multiple: false,
        title: 'Pick working directory for Claude'
      });
      if (typeof picked === 'string') {
        applySessionCwd(activeSession.id, picked, { breakLink: true });
      }
    } catch (e) {
      notifyError(e, { title: "Couldn't pick folder" });
    }
  }

  function clearCwd() {
    if (!activeSession) return;
    applySessionCwd(activeSession.id, null, { breakLink: true });
  }

  /** Bidirectional link. When the AI side initiates and the session already
      has a concrete folder (cwd or worktreePath), the Editor adopts *its*
      folder — "whoever clicks Link pushes their folder to the other side".
      Only when the AI has no folder yet do we fall back to pulling the
      Editor's folder in (the old one-way behavior). */
  function linkActiveSessionToEditor(editorInstanceId: string) {
    if (!activeSession) return;
    const editorPath =
      sessionsState.editorInstanceState[editorInstanceId]?.repoPath ?? '';
    // Decide what counts as the chat's "owned" folder — the thing that
    // actually deserves to flow into the target editor:
    //   - worktreePath: explicit isolated worktree, always owned.
    //   - cwd while NOT currently linked to another editor: user picked
    //     this via pickCwd; treat as owned.
    //   - cwd while linked to a DIFFERENT editor: this is just a mirror
    //     of that other editor's folder, not a deliberate choice — do NOT
    //     push it onto the new target (that's the cross-overwrite bug).
    const aiWorktree = activeSession.worktreePath || '';
    const cwdIsMirror =
      activeSession.linkedToEditor &&
      !!activeSession.linkedToEditorInstanceId &&
      activeSession.linkedToEditorInstanceId !== editorInstanceId;
    const ownedCwd = cwdIsMirror ? '' : (activeSession.cwd || '');
    const ownedPath = aiWorktree || ownedCwd;
    if (ownedPath && ownedPath !== editorPath) {
      setEditorRepoPath(ownedPath, editorInstanceId);
    }
    updateSession(activeSession.id, {
      linkedToEditor: true,
      linkedToEditorInstanceId: editorInstanceId,
      cwd: ownedPath || editorPath || null
    });
  }

  function toggleSessionEditorLink() {
    if (!activeSession) return;
    if (activeSession.linkedToEditor) {
      updateSession(activeSession.id, {
        linkedToEditor: false,
        linkedToEditorInstanceId: null
      });
    } else {
      // Pick the first Editor instance in the active workbench. If none
      // exists, user-initiated link is a no-op (button is disabled in the UI
      // when no editor instance is available).
      const editorInst = firstInstanceOfKind('editor');
      if (!editorInst) return;
      linkActiveSessionToEditor(editorInst.id);
    }
  }

  /** Initiate a link from the Editor side. Always links the *currently
      active* session in the target agent column — never spawns a new chat.
      The chat's cwd just snaps to the editor's folder and the session
      becomes linked. If the column has no active session, we create one
      (empty column → there was nothing to link). */
  function linkEditorToAgent(
    editorInstanceId: string,
    agentInstanceId: string
  ) {
    const editorPath = sessionsState.editorInstanceState[editorInstanceId]?.repoPath || '';
    if (!editorPath) return;
    const inst = activeInstances().find((i) => i.id === agentInstanceId);
    if (!inst || (inst.kind !== 'claude' && inst.kind !== 'cursor')) return;
    const kind = inst.kind;
    const currentId = sessionsState.activeByInstance[inst.id] ?? null;
    const current = currentId
      ? sessionsState.list.find((s) => s.id === currentId) ?? null
      : null;
    if (current) {
      updateSession(current.id, {
        cwd: editorPath,
        linkedToEditor: true,
        linkedToEditorInstanceId: editorInstanceId,
        columnInstanceId: inst.id
      });
    } else {
      newClaudeSession({
        agentKind: kind,
        cwd: editorPath,
        linkedToEditor: true,
        linkedToEditorInstanceId: editorInstanceId,
        columnInstanceId: inst.id
      });
    }
    void scrollInstanceIntoView(inst.id);
  }

  // ---- Worktree management for the active Claude session ----
  let worktreeBusy = $state<'creating' | 'removing' | null>(null);
  let worktreeMenuOpen = $state(false);

  interface WorktreeInfo {
    path: string;
    branch: string | null;
    head: string | null;
    is_main: boolean;
    forgehold_session: string | null;
  }

  async function createWorktree() {
    if (!activeSession) return;
    const repo = activeSession.cwd || editorRepoPath;
    if (!repo) {
      notify({
        kind: 'warning',
        title: 'No repository picked',
        body: 'Worktrees need a git repo to branch off — open a folder in the Editor or pick one as cwd.'
      });
      return;
    }
    const ok = confirm(
      `Isolate this Claude session in its own git worktree?\n\n` +
      `Forgehold will create a fresh branch "forgehold/${activeSession.id.slice(0, 8)}" ` +
      `off your current HEAD and check it out into a private directory.\n\n` +
      `Your main working tree stays untouched. Claude will only write there.`
    );
    if (!ok) return;
    worktreeBusy = 'creating';
    try {
      const info = await invoke<WorktreeInfo>('worktree_create', {
        repo,
        sessionId: activeSession.id,
        baseRef: null
      });
      updateSession(activeSession.id, {
        worktreePath: info.path,
        worktreeBranch: info.branch,
        worktreeRepo: repo
      });
    } catch (e) {
      notifyError(e, { title: 'Failed to create worktree' });
    } finally {
      worktreeBusy = null;
    }
  }

  async function removeWorktree() {
    if (!activeSession || !activeSession.worktreePath || !activeSession.worktreeRepo) return;
    const branch = activeSession.worktreeBranch ?? '(unknown branch)';
    const ok = confirm(
      `Remove the isolated worktree for this session?\n\n` +
      `Branch ${branch} will be force-deleted along with any uncommitted work ` +
      `inside it. If you want to keep Claude's changes, merge or push the ` +
      `branch first.`
    );
    if (!ok) return;
    worktreeBusy = 'removing';
    worktreeMenuOpen = false;
    try {
      await invoke('worktree_remove', {
        repo: activeSession.worktreeRepo,
        sessionId: activeSession.id
      });
      updateSession(activeSession.id, {
        worktreePath: null,
        worktreeBranch: null,
        worktreeRepo: null
      });
    } catch (e) {
      notifyError(e, { title: 'Failed to remove worktree' });
    } finally {
      worktreeBusy = null;
    }
  }

  /** Ensure at least one editor column instance exists, set its repo path,
      and scroll it into view. Returns the editor instance id. */
  function ensureEditorShowing(path: string): string {
    let editor = firstInstanceOfKind('editor');
    if (!editor) {
      const id = addPanelInstance('editor');
      editor = activeInstances().find((i) => i.id === id) ?? null;
    }
    if (editor) {
      setEditorRepoPath(path, editor.id);
      void scrollInstanceIntoView(editor.id);
      return editor.id;
    }
    return '';
  }

  function openWorktreeInEditor() {
    if (!activeSession?.worktreePath) return;
    ensureEditorShowing(activeSession.worktreePath);
    worktreeMenuOpen = false;
  }

  /** Jump from the active Claude session to the Editor column, loading the
      same folder the session is using (worktree > session.cwd > inherited
      editorRepoPath). Opens the Editor column if hidden. Targets the editor
      this session is linked to when one exists — otherwise creates / scrolls
      the first editor instance. */
  function openSessionFolderInEditor() {
    const path = activeSession?.worktreePath || activeSession?.cwd || editorRepoPath;
    if (!path) return;
    const boundId = activeSession?.linkedToEditorInstanceId ?? null;
    const bound = boundId ? findInstanceAnywhere(boundId) : null;
    if (bound && bound.inst.kind === 'editor') {
      // Linked editor still exists — push the path there and scroll it
      // into view. Don't touch any OTHER editor (was the bug: clicking
      // the link pill clobbered the first editor's folder regardless of
      // which editor the session was actually linked to).
      setEditorRepoPath(path, bound.inst.id);
      void scrollInstanceIntoView(bound.inst.id);
      return;
    }
    ensureEditorShowing(path);
  }

  /** Handle a click on a @file/@dir mention inside a rendered chat bubble.
      `path` is whatever the mention's @token resolved to — usually a path
      relative to the session's cwd/worktree/editor. We try each of those
      three roots, in priority order, until something exists on disk.
      
      File mentions go through `openFileInEditor` which opens a tab without
      clobbering the editor's repoPath (was a real bug — clicking
      `@scripts/.../resolve-components.js` made FileTree treat the file
      itself as the repo root, scrambling the tree). Folder mentions
      (trailing `/`) keep the legacy `ensureEditorShowing` behaviour
      because that's what the user actually wants when they click a
      directory: scope the tree to it. */
  async function openMentionPath(path: string) {
    const isDir = path.endsWith('/');
    const candidates: string[] = [];
    if (path.startsWith('/')) {
      candidates.push(path);
    } else {
      const trimmed = path.replace(/\/$/, '');
      const roots = [
        activeSession?.worktreePath,
        activeSession?.cwd,
        editorRepoPath
      ].filter((r): r is string => !!r);
      for (const root of roots) {
        candidates.push(`${root.replace(/\/$/, '')}/${trimmed}`);
      }
    }
    const linkedEditorId = activeSession?.linkedToEditorInstanceId ?? null;
    for (const abs of candidates) {
      try {
        const ok = await invoke<boolean>('fs_path_exists', { path: abs });
        if (ok) {
          if (isDir) {
            ensureEditorShowing(abs);
          } else {
            await openFileInEditor(abs, { preferInstanceId: linkedEditorId });
          }
          return;
        }
      } catch {
        // keep trying the next candidate
      }
    }
    // Last-ditch: open the first candidate anyway — the Editor will surface
    // its own "file not found" state if the path is wrong.
    if (candidates[0]) {
      if (isDir) {
        ensureEditorShowing(candidates[0]);
      } else {
        await openFileInEditor(candidates[0], { preferInstanceId: linkedEditorId });
      }
    }
  }

  /** Open (or scroll to) a column of the given kind. Singleton kinds only
      ever get one instance. Multi-instance kinds open their first matching
      instance (or create one if none exist). Close via the X on the column
      itself. */
  function openColumn(kind: PanelKind) {
    void scrollKindIntoView(kind);
  }

  /** Spawn a new column instance of the given kind and scroll it into view.
      Bound to the "+" button in the pill bar. All kinds support multiple
      instances — see `addPanelInstance` in layout.svelte.ts. */
  function spawnColumnInstance(kind: PanelKind) {
    const id = addPanelInstance(kind);
    void scrollInstanceIntoView(id);
  }

  /** Spawn a fresh chat in the Claude/Cursor column and make sure the column
      is visible. Bound to the "+" button next to agent pills. */
  function spawnAgentChat(kind: 'claude' | 'cursor') {
    let inst = firstInstanceOfKind(kind);
    if (!inst) {
      const id = addPanelInstance(kind);
      inst = activeInstances().find((i) => i.id === id) ?? null;
    }
    newClaudeSession({ agentKind: kind, columnInstanceId: inst?.id ?? null });
    if (inst) void scrollInstanceIntoView(inst.id);
  }

  /** Keep every linked session's cwd in sync with the specific Editor
      instance it's bound to (`linkedToEditorInstanceId`). Fall back to the
      first editor in the active workbench when the binding is stale (editor
      was closed). Manually picking a cwd (via pickCwd / worktree ops)
      breaks the link elsewhere. */
  $effect(() => {
    for (const s of sessionsState.list) {
      if (!s.linkedToEditor) continue;
      const boundId = s.linkedToEditorInstanceId;
      const boundExists = boundId ? !!findInstanceAnywhere(boundId) : false;
      const fallback = firstInstanceOfKind('editor');
      const patch: Partial<ClaudeSession> = {};
      // Heal stale `linkedToEditorInstanceId` — the bound editor was closed.
      // If a fallback editor exists, re-bind silently so the pill shows the
      // current editor's name instead of "LINKED TO …" with no name. If no
      // editor remains, drop the link entirely so the pill disappears.
      if (!boundExists) {
        if (fallback) {
          patch.linkedToEditorInstanceId = fallback.id;
        } else {
          patch.linkedToEditor = false;
          patch.linkedToEditorInstanceId = null;
        }
      }
      const effectiveBoundId = patch.linkedToEditorInstanceId !== undefined
        ? patch.linkedToEditorInstanceId
        : boundId;
      const path = effectiveBoundId
        ? sessionsState.editorInstanceState[effectiveBoundId]?.repoPath ?? null
        : null;
      if (s.cwd !== path) {
        patch.cwd = path;
      }
      if (Object.keys(patch).length > 0) {
        updateSession(s.id, patch);
      }
    }
  });

  function toggleWorktreeMenu() {
    worktreeMenuOpen = !worktreeMenuOpen;
  }

  // Small helpers that keep AgentColumn's prop surface purely functional:
  // the component doesn't import editingMsg directly — it mutates via these.
  function cancelEditMessage() {
    editingMsg = null;
  }
  function setEditingMsgDraft(draft: string) {
    if (editingMsg) editingMsg = { ...editingMsg, draft };
  }

  async function copyWorktreeBranch() {
    if (!activeSession?.worktreeBranch) return;
    try {
      await navigator.clipboard.writeText(activeSession.worktreeBranch);
    } catch {/* ignore */}
    worktreeMenuOpen = false;
  }

  let worktreeDiffOpen = $state(false);
  function openWorktreeDiff() {
    worktreeMenuOpen = false;
    worktreeDiffOpen = true;
  }

  async function applyWorktree() {
    if (!activeSession || !activeSession.worktreePath || !activeSession.worktreeRepo || !activeSession.worktreeBranch) return;
    const ok = confirm(
      `Apply Claude's work to your current branch?\n\n` +
      `Forgehold will run \`git merge --no-ff ${activeSession.worktreeBranch}\` in ${activeSession.worktreeRepo} ` +
      `and then remove the isolated worktree.\n\n` +
      `Make sure your main repo is checked out to the branch you want to merge into, ` +
      `and that its working tree is clean. If the merge has conflicts, the worktree stays — resolve conflicts in the main repo, commit, then discard the worktree manually.`
    );
    if (!ok) return;
    worktreeBusy = 'removing';
    worktreeMenuOpen = false;
    try {
      const msg = await invoke<string>('worktree_apply', {
        repo: activeSession.worktreeRepo,
        sessionId: activeSession.id
      });
      updateSession(activeSession.id, {
        worktreePath: null,
        worktreeBranch: null,
        worktreeRepo: null
      });
      notify({ kind: 'success', title: 'Worktree applied', body: msg });
    } catch (e) {
      notifyError(e, {
        title: 'Apply failed',
        body: 'Worktree is preserved — resolve conflicts in the main repo, then retry.'
      });
    } finally {
      worktreeBusy = null;
    }
  }

  async function scrollChatBottom() {
    await tick();
    // Scroll every column instance whose active session is the currently-
    // focused one. In practice that's one column, but identical sessions
    // could theoretically be mirrored across columns in the future.
    const focused = sessionsState.activeClaudeId;
    if (!focused) return;
    for (const [instId, el] of Object.entries(sessionsState.scrollEls)) {
      if (!el) continue;
      if (sessionsState.activeByInstance[instId] === focused) {
        el.scrollTop = el.scrollHeight;
      }
    }
  }

  function startThinkingTimer() {
    thinkingStartedAt = Date.now();
    thinkingTick = 0;
    if (thinkingTimer) clearInterval(thinkingTimer);
    thinkingTimer = setInterval(() => {
      thinkingTick += 1;
    }, 1000);
  }

  function stopThinkingTimer() {
    if (thinkingTimer) {
      clearInterval(thinkingTimer);
      thinkingTimer = null;
    }
    thinkingStartedAt = null;
  }

  // Thin wrapper around `runCompactSessionService` so the AgentColumn
  // prop binding (`onCompactSession={runCompactSession}`) keeps the
  // same shape while the body lives in `lib/services/agentCompact.ts`.
  // Threads through the two component-local pieces: `editorRepoPath`
  // ($derived reactive) and `scrollChatBottom` (DOM-coupled).
  async function runCompactSession(sessionId: string): Promise<void> {
    await runCompactSessionService(sessionId, {
      editorRepoPath,
      scrollChatBottom
    });
  }

  /* Session transcript export (M4 §2.2.8). Copies the rendered
   * Markdown / JSON to the clipboard — caller picks the format via
   * the AgentColumn export-chip click handler. Toast confirms so
   * the user knows the clipboard now holds something. */
  async function exportSession(sessionId: string, format: 'markdown' | 'json') {
    const session = sessionsState.list.find((s) => s.id === sessionId);
    if (!session) return;
    const body = format === 'markdown'
      ? exportSessionMarkdown(session)
      : exportSessionJson(session);
    try {
      await navigator.clipboard.writeText(body);
      notify({
        kind: 'success',
        title: format === 'markdown' ? 'Markdown transcript copied' : 'JSON snapshot copied',
        body: 'Paste into an issue, doc, or wherever you need it.'
      });
    } catch (e) {
      notifyError(e, { title: 'Copy failed' });
    }
  }

  /* Slash-command interceptor for `/compact`, `/clear`, `/usage`,
   * `/help` (M4 §2.2.4). Returns `true` when the input was a known
   * command and dispatched locally; the caller then short-circuits
   * the agent send. */
  async function handleSlashCommand(
    text: string,
    session: ClaudeSession
  ): Promise<boolean> {
    const cmd = parseSlashCommand(text);
    if (!cmd) return false;
    /* Clear the composer + capture an `at` for any follow-up. The
     * synthetic assistant messages we append below all carry their
     * own timestamps. */
    setSessionInput(session.id, '');
    if (cmd === 'compact') {
      await runCompactSession(session.id);
    } else if (cmd === 'clear') {
      clearSessionHistory(session);
    } else if (cmd === 'usage') {
      appendUsageBreakdown(session);
      void scrollChatBottom();
    } else if (cmd === 'help') {
      appendSlashHelp(session);
      void scrollChatBottom();
    }
    return true;
  }

  async function sendClaudeMessage() {
    const s = activeSession;
    if (!s || s.sending) return;
    // Allow send with empty text as long as there's at least one attachment —
    // dropping just an image with no extra prompt is a valid "look at this"
    // turn (the image goes to the model as a vision content block).
    if (!s.input.trim() && s.mentions.length === 0) return;
    const text = s.input.trim();
    /* Slash-command interceptor — handle `/compact`, `/clear`,
     * `/usage`, `/help` locally and short-circuit before any agent
     * call. `parseSlashCommand` only matches when the WHOLE message
     * is the command, so a regular message that happens to start
     * with `/` falls through to the normal path. */
    if (await handleSlashCommand(text, s)) return;
    const id = s.id;
    // Snapshot the mentions BEFORE clearing so we can still bake them into
    // the prompt below + stamp the image refs onto the user-message bubble
    // so the transcript still shows what was sent after the strip clears.
    const mentionsSnapshotPre = s.mentions;
    const imageMentions = mentionsSnapshotPre.filter(
      (m) => m.source === 'file' && !m.isDir && !!m.body && isImagePath(m.body)
    );
    const userImages = imageMentions.map((m) => ({ path: m.body!, name: m.title }));
    appendSessionMessage(id, {
      role: 'user',
      content: text,
      at: new Date().toISOString(),
      ...(userImages.length ? { images: userImages } : {})
    });
    // Auto-title from first user message when chat had no mentions
    const curr = sessionsState.list.find((x) => x.id === id);
    const mentionsSnapshot = curr?.mentions ?? [];
    if (
      curr &&
      curr.messages.filter((m) => m.role === 'user').length === 1 &&
      curr.mentions.length === 0
    ) {
      const autoTitle = text.slice(0, 36) + (text.length > 36 ? '…' : '');
      updateSession(id, { title: autoTitle, input: '', sending: true, mentions: [], awaitingApproval: false });
    } else {
      updateSession(id, { input: '', sending: true, mentions: [], awaitingApproval: false });
    }
    // Append empty assistant message that streaming will fill.
    appendSessionMessage(id, {
      role: 'assistant',
      content: '',
      at: new Date().toISOString()
    });
    startThinkingTimer();
    const runStartedAt = Date.now();
    void scrollChatBottom();

    // Build prompt: include full context for each @mention. Uses the
    // snapshot taken just before we cleared `mentions` on the session,
    // so the CLI still gets the context even though the UI no longer
    // shows the chips.
    const sess = sessionsState.list.find((x) => x.id === id);
    const agentKindForPrompt = sess?.agentKind ?? 'claude';
    let prompt = text;
    if (mentionsSnapshot.length) {
      const ctx = mentionsSnapshot
        .map((m) => {
          if (m.source === 'file') {
            const abs = m.body ?? m.externalId;
            const kind = m.isDir ? 'directory' : isImagePath(abs) ? 'image' : 'file';
            // Image payload routing per agent:
            //  - Claude: already embedded as base64 content blocks via
            //    stream-json input below; no need to also describe in text.
            //  - Cursor: cursor-agent's headless mode actually DOES vision
            //    when the absolute path is in the prompt text (verified
            //    against cursor-agent CLI 2026-04). So we keep the path-
            //    pointer mention — it triggers the agent's native image read.
            if (kind === 'image' && agentKindForPrompt === 'claude') return null;
            const hint = kind === 'image'
              ? `This is an image attached by the user — load it via its absolute path to view it inline.`
              : `You have Read / Glob / Grep tools — use them to inspect this ${kind} when relevant.`;
            const label = kind === 'image' ? `Attached ${kind}: ${m.title}` : `Referenced ${kind}: @${m.externalId}`;
            return `${label}\nAbsolute path: ${abs}\n${hint}`;
          }
          return `@${m.externalId} — ${m.title}` + (m.body ? `\n\n${m.body}` : '');
        })
        .filter((x): x is string => x !== null)
        .join('\n\n----\n\n');
      if (ctx) prompt = `Referenced items:\n\n${ctx}\n\n----\n\nUser message:\n${text}`;
    }

    // Priority of working dir:
    //   1. Session has an isolated worktree → use it (SPEC: "every Claude run
    //      in a worktree, never touches main working tree").
    //   2. Explicit cwd set by user via pickCwd.
    //   3. Editor column's open repo (shared state).
    //   4. None → agent inherits Forgehold's cwd (last-resort fallback).
    const cwd = sess?.worktreePath || sess?.cwd || editorRepoPath || null;
    const claudeUuid = sess?.claudeUuid ?? genUuid();
    const resume = Boolean(sess?.claudeResumable);
    const rules = sessionsState.userRules.trim();
    const agentKind = sess?.agentKind ?? 'claude';
    const cursorModel = agentKind === 'cursor' ? (sess?.cursorModel ?? null) : null;
    const claudeModel = agentKind === 'claude' ? (sess?.claudeModel ?? null) : null;
    const claudeToolProfile = agentKind === 'claude' ? (sess?.claudeToolProfile ?? null) : null;
    const appContext = buildAgentAppContext(id);
    // Image vision blocks are a Claude-only path (cursor-agent has no
    // equivalent input-format flag). For Cursor we already wove the
    // "Read this image at <path>" hint into the prompt above.
    const imagePaths = agentKind === 'claude' ? userImages.map((u) => u.path) : [];

    /* Linked-canvas vision channel: rasterize the canvas to a PNG and
       append its path to imagePaths so the agent gets a visual snapshot
       alongside the JSON inventory in the system prompt. We don't add
       it to the user's message `images` array (the chip strip / chat
       transcript) — it's a "behind the scenes" attachment that
       belongs to the API call, not the user's perception of "what I
       sent". Skipped silently when:
         - the session isn't linked,
         - the canvas was deleted,
         - the canvas is empty (no shapes / edges),
         - rendering fails for any reason.
       Cursor-agent doesn't support image inputs the same way so we
       only attach for Claude sessions. */
    if (agentKind === 'claude' && sess?.linkedCanvasId) {
      const c = ensureCanvasLoaded(sess.linkedCanvasId);
      if (c && (c.shapes.length > 0 || c.edges.length > 0)) {
        try {
          const dir = await getAttachmentDir();
          const path = await saveCanvasScreenshot(c, dir);
          if (path) imagePaths.push(path);
        } catch (err) {
          console.warn('canvas screenshot attach failed', err);
        }
      }
    }

    try {
      const result = await runAgentRequest({
        sessionId: id,
        prompt,
        cwd,
        claudeUuid,
        resume,
        rules: rules || null,
        agentKind,
        cursorModel,
        claudeModel,
        claudeToolProfile,
        appContext,
        imagePaths,
        onAssistantDelta: appendAssistantDelta,
        onAppNavigation: handleAppNavigation
      });
      // Keep the streamed transcript intact — it includes intermediate
      // assistant text, `> *Tool* …` hint lines from `formatToolUse`,
      // navigation hints from `mcp__app__*` tools, and so on. The
      // earlier `replaceLastAssistant(id, result.reply)` wiped all of
      // that and left only the final answer; users lost the context
      // of what the agent did to get there.
      //
      // Two fallback cases where we DO need to write to the message:
      //   1. Streaming dropped everything (some agent backends emit no
      //      `text` blocks, only the final result event). In that case
      //      content is empty — fall back to `result.reply`.
      //   2. Empty reply with empty stream (rare — model returned nothing).
      //      Stamp a placeholder so the chat doesn't look broken.
      const sessNowForReply = sessionsState.list.find((s) => s.id === id);
      const lastMsg = sessNowForReply?.messages[sessNowForReply.messages.length - 1];
      const streamed = lastMsg?.role === 'assistant' ? lastMsg.content.trim() : '';
      const finalReply = result.reply.trim();
      if (!streamed) {
        replaceLastAssistant(id, finalReply || '(empty response)');
      }
      // Did `applySessionCwd` swap the session's claudeUuid mid-turn? That
      // happens when an in-turn tool call (`set_editor_repo_path` /
      // `set_agent_cwd`) changed cwd — the new uuid is fresh, not yet
      // used by the CLI. If we blindly set `claudeResumable=true` here we
      // overwrite the `false` applySessionCwd just set, and the *next*
      // turn does `--resume <new-uuid>` against a CLI that has never
      // heard of it ("No conversation found with session ID …"). So when
      // the uuid changed mid-turn, leave both fields alone — the new
      // uuid will be created via `--session-id` on its first ever use.
      const sessAfter = sessionsState.list.find((s) => s.id === id);
      const uuidStable = !!sessAfter && sessAfter.claudeUuid === claudeUuid;
      const patch: Partial<ClaudeSession> = {};
      if (uuidStable) {
        patch.claudeResumable = true;
        // Cursor mints a new chat_id via `create-chat` on the first turn;
        // round-trip it back so subsequent turns resume cleanly.
        if (result.sessionUuid && result.sessionUuid !== claudeUuid) {
          patch.claudeUuid = result.sessionUuid;
        }
      }
      // Fire-and-forget refresh — debounced to 60s by the state module,
      // so spamming the chat doesn't hammer the OAuth endpoint (which
      // 429s under tight polling). One real fetch per ~minute is plenty
      // for the chip to feel "live" since the 5h bucket only ticks up
      // once per turn anyway.
      void refreshPlanUsage();
      // One-shot recap consumed — clear so it doesn't re-inject on every
      // subsequent turn. We check `sessAfter` (post-turn) because
      // applySessionCwd may have just *set* the recap mid-turn; in that
      // case it's freshly minted for the NEXT turn, don't clear yet.
      // Only clear if the recap pre-dated this turn (i.e. it was the one
      // we already injected into appContext).
      if (sess?.cwdSwitchRecap) {
        patch.cwdSwitchRecap = null;
      }
      // Did the agent end its turn with pending approval cards? If so,
      // mark `awaitingApproval` so the UI shows a "waiting for your
      // approval" hint, AND so onActionResolved knows to auto-continue
      // the agent's turn once the user approves. Without this the user
      // has to manually type "now make the PR" after every commit.
      const stillPending = sessAfter?.actions.some((a) => a.status === 'pending') ?? false;
      if (stillPending) patch.awaitingApproval = true;
      updateSession(id, patch);
    } catch (e) {
      const msg = typeof e === 'string' ? e : String(e);
      const cancelled = msg.toLowerCase().includes('cancelled');
      if (cancelled) {
        // Keep the partial content; add a system note.
        appendSessionMessage(id, {
          role: 'system',
          content: 'Cancelled.',
          at: new Date().toISOString()
        });
      } else {
        replaceLastAssistant(id, `**${s.agentKind === 'cursor' ? 'Cursor' : 'Claude'} failed:** ${msg}`);
        // In-app toast — sticky, with full error text. Only when the user is
        // looking at the app (otherwise the macOS notification below covers
        // the off-app case).
        if (appHasFocus()) {
          notifyError(e, { title: `${s.agentKind === 'cursor' ? 'Cursor' : 'Claude'} run failed` });
        }
      }
      // macOS Notification Center: only when user has tabbed away. The
      // chat bubble + toast are enough when the app is in focus.
      if (!appHasFocus() && !cancelled) {
        notifyClaudeRunComplete({
          agentLabel: s.agentKind === 'cursor' ? 'Cursor' : 'Claude',
          sessionTitle: s.title || 'Untitled chat',
          ok: false,
          durationMs: Date.now() - runStartedAt
        });
      }
    }
    stopThinkingTimer();
    const finalSess = sessionsState.list.find((x) => x.id === id);
    const erroredOut = finalSess?.messages.some(
      (m, i) => i === finalSess.messages.length - 1 && m.role === 'assistant' && m.content.startsWith('**Claude failed:')
    );
    updateSession(id, { sending: false });
    // Native notification on success, but only if the user has tabbed away —
    // the streaming reply is its own feedback when they're watching.
    if (!appHasFocus() && !erroredOut) {
      notifyClaudeRunComplete({
        agentLabel: s.agentKind === 'cursor' ? 'Cursor' : 'Claude',
        sessionTitle: finalSess?.title || s.title || 'Untitled chat',
        ok: true,
        durationMs: Date.now() - runStartedAt
      });
    }
    void scrollChatBottom();
  }

  // Streaming-event dispatch lives in `$lib/stream/agentStream.ts`. The
  // caller here just forwards assistant text deltas to the chat (session
  // store + scroll-to-bottom is the only DOM-coupled bit).
  function appendAssistantDelta(sessionId: string, delta: string) {
    appendToLastAssistant(sessionId, delta);
    void scrollChatBottom();
  }

  /** Forgehold-app MCP navigation: the agent calls `mcp__app__open_jira_issue`
   *  / `switch_view` / `add_editor_instance` / etc., the stream parser sees
   *  the `tool_use` event, and we drive Forgehold's reactive state directly
   *  here — same outcome as if the user had clicked through the UI by hand.
   *  No approval card, since these are read-only navigations.
   *
   *  When inputs are bad (unknown view name, blank id) we silently no-op
   *  rather than throw — the chat still shows the inline `> *Tool* …` hint
   *  so the user can see what the agent tried. */
  /** Narrow a string to the GithubFilterMode union — anything else (typo
   *  from the agent, future mode the frontend doesn't know) silently no-
   *  ops, matching the rest of handleAppNavigation's "bad input = skip"
   *  contract. Defined here instead of inboxState because it's only
   *  needed for the agent-driven path; the UI dropdowns build the union
   *  by construction. */
  function isGithubFilterMode(s: string): s is GithubFilterMode {
    return (
      s === 'involving' ||
      s === 'authored' ||
      s === 'review_requested' ||
      s === 'assigned' ||
      s === 'user' ||
      s === 'all'
    );
  }
  type SentryStatus = 'unresolved' | 'resolved' | 'ignored' | 'all';
  type SentryLevel = 'all' | 'fatal' | 'error' | 'warning' | 'info' | 'debug';
  function isSentryStatus(s: string): s is SentryStatus {
    return s === 'unresolved' || s === 'resolved' || s === 'ignored' || s === 'all';
  }
  function isSentryLevel(s: string): s is SentryLevel {
    return (
      s === 'all' ||
      s === 'fatal' ||
      s === 'error' ||
      s === 'warning' ||
      s === 'info' ||
      s === 'debug'
    );
  }
  /** SentryFilters in the column store carries 7 fields including `sort`
   *  (which the agent doesn't expose) — we accept any subset matching
   *  what the tool advertises. Pulled out as its own type so the agent-
   *  driven setter doesn't reference the persisted-filter shape verbatim
   *  (its `sort` field would be a typed-`unknown` mismatch). */
  type SentryFilterPatch = {
    projects?: string[];
    search?: string;
    status?: SentryStatus;
    level?: SentryLevel;
    environment?: string | null;
  };
  /** MCP `switch_view` ships platform-named views (`github` / `jira` /
   *  `sentry`) so a future GitLab/Bitbucket tab can claim its own slot.
   *  The internal `View` enum kept the `Tab` suffix to avoid colliding
   *  with workbench column kinds (the kind `github` is a workbench
   *  column; `githubTab` is the top-level page). Translate here so the
   *  agent never sees the suffix. Returns `null` for unknown values so
   *  the handler can no-op cleanly instead of forcing an `as View`
   *  cast that would route to a blank screen. */
  function mapAgentViewToInternal(v: string): View | null {
    switch (v) {
      case 'github':
        return 'githubTab';
      case 'jira':
        return 'jiraTab';
      case 'sentry':
        return 'sentryTab';
      case 'workbench':
      case 'rules':
      case 'connections':
      case 'settings':
        return v;
      default:
        return null;
    }
  }
  /** Coerce raw `sprint_ids` payload entries into the persisted
   *  `SprintScope[]` shape (numeric id or the literal `'backlog'`).
   *  The MCP tool's JSON schema accepts string|number; we accept either
   *  here too because cursor-agent and Claude have shipped both. */
  function parseSprintScopes(raw: unknown[]): SprintScope[] {
    const out: SprintScope[] = [];
    for (const x of raw) {
      if (typeof x === 'number' && Number.isFinite(x) && x > 0) {
        out.push(x);
      } else if (typeof x === 'string') {
        if (x === 'backlog') {
          out.push('backlog');
        } else {
          const n = Number(x);
          if (Number.isFinite(n) && n > 0) out.push(n);
        }
      }
    }
    return out;
  }
  function handleAppNavigation(
    _sessionId: string,
    name: string,
    input: Record<string, unknown>
  ) {
    const str = (k: string): string =>
      typeof input[k] === 'string' ? (input[k] as string).trim() : '';
    const num = (k: string): number => {
      const v = input[k];
      return typeof v === 'number' ? v : Number(v);
    };
    /* `pick` accepts a canonical key plus a list of aliases and
       returns the first one that's a non-empty string. Mirrors the
       `#[serde(alias = "...")]` set on the sidecar's params struct
       so the frontend dispatcher accepts the same shapes the sidecar
       does — LLMs love shortening field names. */
    const pick = (...keys: string[]): string => pickFrom(input, ...keys);
    /* `pickFrom` is the same idea but works against any object — used
       by batch handlers (`canvas_add_edges`) that walk an array of
       sub-records, each of which may have its own alias-renamed
       fields. */
    const pickFrom = (obj: Record<string, unknown>, ...keys: string[]): string => {
      for (const k of keys) {
        const v = obj[k];
        if (typeof v === 'string' && v.trim()) return v.trim();
      }
      return '';
    };
    /* `coerceString` mirrors the sidecar's `coerce_to_string` —
       cursor-agent has shipped the same field as a string, a single-
       element array, or even a wrapped object with an inner `path`/
       `value` key. We accept any of those shapes and return the first
       plausible non-empty string (or empty string when nothing
       resolves). */
    const coerceString = (v: unknown): string => {
      if (typeof v === 'string') return v.trim();
      if (Array.isArray(v)) {
        for (const x of v) {
          const s = coerceString(x);
          if (s) return s;
        }
        return '';
      }
      if (v && typeof v === 'object') {
        const obj = v as Record<string, unknown>;
        for (const k of ['repo_path', 'path', 'folder', 'directory', 'dir', 'cwd', 'value', 'text', 'string']) {
          if (k in obj) {
            const s = coerceString(obj[k]);
            if (s) return s;
          }
        }
      }
      return '';
    };
    /* `pickDeep` is the alias-aware analogue of `pickFrom` that ALSO
       drills into the wrapper objects cursor-agent / claude have been
       known to nest payloads under (`args` / `arguments` / `params` /
       `input`). Used by `set_editor_repo_path` / `set_agent_cwd` —
       both have been observed receiving fully-wrapped payloads where
       `repo_path` is two levels deep. Walks up to depth 4 to cover
       the `{"args":{"args":{...}}}` case we've seen in the wild. */
    const pickDeep = (obj: Record<string, unknown> | null | undefined, keys: string[], depth = 4): string => {
      if (!obj || typeof obj !== 'object' || depth === 0) return '';
      for (const k of keys) {
        if (k in obj) {
          const s = coerceString(obj[k]);
          if (s) return s;
        }
      }
      for (const wrap of ['args', 'arguments', 'params', 'parameters', 'input', 'data', 'payload']) {
        const inner = obj[wrap];
        if (inner && typeof inner === 'object' && !Array.isArray(inner)) {
          const s = pickDeep(inner as Record<string, unknown>, keys, depth - 1);
          if (s) return s;
        }
      }
      return '';
    };
    /* Canonical alias lists for the deep extractors — kept in sync
       with the sidecar's `REPO_PATH_KEYS` / `INSTANCE_NAME_KEYS` /
       `INSTANCE_ID_KEYS` so both halves of the round-trip recognise
       the same payload shapes. */
    const REPO_PATH_KEYS_DEEP = [
      'repo_path', 'repoPath', 'path', 'folder', 'directory', 'dir',
      'cwd', 'repo', 'repository_path', 'folderPath', 'dirPath',
      'fullPath', 'absolutePath', 'target_path', 'target'
    ];
    const INSTANCE_NAME_KEYS_DEEP = [
      'instance_name', 'instanceName', 'name', 'column_name', 'columnName',
      'editor_name', 'agent_name', 'label'
    ];
    const INSTANCE_ID_KEYS_DEEP = [
      'instance_id', 'instanceId', 'id', 'column_id', 'columnId',
      'editor_id', 'agent_id', 'uuid'
    ];
    /* Shared edge-spec parser used by both `canvas_add_edge` (single)
       and `canvas_add_edges` (batch). Mirrors the alias set on the
       sidecar's CanvasAddEdgeParams; returns null when required ids
       are missing so the caller can skip the entry instead of throwing. */
    const parseEdgeSpec = (obj: Record<string, unknown>): Edge | null => {
      const fromId = pickFrom(
        obj,
        'from_shape_id', 'from', 'source', 'from_id', 'fromId',
        'fromShapeId', 'fromNode', 'fromBlock', 'start', 'start_id',
        'startId', 'src', 'sourceId'
      );
      const toId = pickFrom(
        obj,
        'to_shape_id', 'to', 'target', 'to_id', 'toId', 'toShapeId',
        'toNode', 'toBlock', 'end', 'end_id', 'endId', 'dest', 'dst',
        'targetId'
      );
      if (!fromId || !toId) return null;
      type AnchorName = 'tl'|'tc'|'tr'|'ml'|'mc'|'mr'|'bl'|'bc'|'br';
      const validAnchors: AnchorName[] = ['tl','tc','tr','ml','mc','mr','bl','bc','br'];
      const fromAnchorRaw = pickFrom(
        obj,
        'from_anchor', 'fromAnchor', 'source_anchor', 'sourceAnchor',
        'start_anchor', 'startAnchor', 'srcAnchor'
      ) || 'mr';
      const toAnchorRaw = pickFrom(
        obj,
        'to_anchor', 'toAnchor', 'target_anchor', 'targetAnchor',
        'end_anchor', 'endAnchor', 'destAnchor'
      ) || 'ml';
      const fromAnchor = (validAnchors as string[]).includes(fromAnchorRaw)
        ? (fromAnchorRaw as AnchorName) : 'mr';
      const toAnchor = (validAnchors as string[]).includes(toAnchorRaw)
        ? (toAnchorRaw as AnchorName) : 'ml';
      const kindRaw = pickFrom(obj, 'kind', 'style', 'edge_kind', 'edgeKind');
      const kind = (kindRaw === 'line' || kindRaw === 'dashed') ? kindRaw : 'arrow';
      const routingRaw = pickFrom(obj, 'routing', 'route', 'path', 'pathing');
      const routing = (routingRaw === 'straight' || routingRaw === 'curved')
        ? routingRaw : 'orthogonal';
      const labelRaw = pickFrom(obj, 'label', 'text', 'caption', 'title');
      const edge = makeEdge({
        from: { shapeId: fromId, anchor: fromAnchor },
        to: { shapeId: toId, anchor: toAnchor },
        kind, routing,
        label: labelRaw || null
      });
      const desiredId = pickFrom(obj, 'edge_id', 'id', 'edgeId');
      if (desiredId) edge.id = desiredId;
      return edge;
    };
    switch (name) {
      case 'mcp__app__open_jira_issue': {
        const key = str('key');
        if (key) inboxState.jiraFocusKey = key;
        return;
      }
      case 'mcp__app__open_sentry_issue': {
        // openSentryFocus(id) defaults eventId to null — equivalent to
        // "latest" so a stale event id from a previous open_sentry_event
        // call doesn't carry over.
        const id = str('id');
        if (id) openSentryFocus(id);
        return;
      }
      case 'mcp__app__open_sentry_event': {
        const id = str('issue_id');
        const eventId = str('event_id') || null;
        if (id) openSentryFocus(id, eventId);
        return;
      }
      case 'mcp__app__open_github_pr':
      case 'mcp__app__open_github_issue': {
        // GitHub focus pane wants a full InboxItem; fetch it on demand
        // through the same API call the inbox uses, then stash. The user
        // sees a brief flash before it lands — fine for a navigation.
        // The overlay is mounted at page root, so it appears over
        // whatever view the user is currently on.
        const owner = str('owner');
        const repo = str('repo');
        const n = num('number');
        if (!owner || !repo || !Number.isFinite(n)) return;
        const tabHint = str('tab') as DetailTab | '';
        void resolveGithubFocus(owner, repo, n, tabHint || null);
        return;
      }
      case 'mcp__app__switch_view': {
        const v = str('view');
        // The MCP tool exposes platform-named views (`github` / `jira`
        // / `sentry`) so a future GitLab tab can claim its own slot
        // without colliding with GitHub. Internal `View` keys still
        // carry the `Tab` suffix to disambiguate from workbench column
        // kinds (`github` the column kind vs `githubTab` the top-level
        // page) — translate here. Anything else passes through
        // unchanged (`workbench`, `rules`, `connections`, `settings`).
        const mapped = mapAgentViewToInternal(v);
        if (mapped) view = mapped;
        return;
      }
      case 'mcp__app__add_editor_instance': {
        const repoPath = str('repo_path');
        view = 'workbench';
        const newId = addPanelInstance('editor');
        if (repoPath && newId) setEditorRepoPath(repoPath, newId);
        return;
      }
      case 'mcp__app__open_connect_modal': {
        const sourceId = str('source');
        const conn = connectionsMeta.find((c) => c.id === sourceId);
        if (conn) openConnectModal(conn);
        return;
      }
      case 'mcp__app__add_workbench_instance': {
        const kind = str('kind');
        const validKinds: PanelKind[] = ['github', 'jira', 'sentry', 'claude', 'cursor', 'editor', 'canvas'];
        if (!validKinds.includes(kind as PanelKind)) return;
        view = 'workbench';
        const newId = addPanelInstance(kind as PanelKind);
        if (kind === 'editor') {
          const repoPath = str('repo_path');
          if (repoPath && newId) setEditorRepoPath(repoPath, newId);
        }
        // Scroll the new column into view so the user actually sees the
        // change — addPanelInstance can drop a column off-screen on a
        // wide layout.
        if (newId) void scrollInstanceIntoView(newId);
        return;
      }
      case 'mcp__app__new_workbench': {
        const name = str('name') || 'Workbench';
        const activate = input.activate !== false; // default true
        const newId = addWorkbench(name);
        if (activate && newId) setActiveWorkbench(newId);
        view = 'workbench';
        return;
      }
      case 'mcp__app__switch_workbench': {
        const name = str('name');
        const indexRaw = input.index;
        view = 'workbench';
        if (name) {
          const target = layoutState.workbenches.find(
            (w) => w.name.toLowerCase() === name.toLowerCase()
          );
          if (target) setActiveWorkbench(target.id);
          return;
        }
        if (typeof indexRaw === 'number' && Number.isInteger(indexRaw)) {
          const target = layoutState.workbenches[indexRaw];
          if (target) setActiveWorkbench(target.id);
        }
        return;
      }
      case 'mcp__app__focus_workbench_instance': {
        const kind = str('kind');
        const validKinds: PanelKind[] = ['github', 'jira', 'sentry', 'claude', 'cursor', 'editor', 'canvas'];
        if (!validKinds.includes(kind as PanelKind)) return;
        view = 'workbench';
        void scrollKindIntoView(kind as PanelKind);
        return;
      }
      case 'mcp__app__open_github_repo': {
        const owner = str('owner');
        const repo = str('repo');
        const section = str('section') || 'pulls';
        const path = str('path');
        if (!owner || !repo) return;
        view = 'githubTab';
        // GithubTab watches this slot and clears it after opening.
        // `path` only honoured for section=code (server validates too).
        inboxState.pendingRepoNav = {
          owner,
          repo,
          section,
          path: section === 'code' && path ? path : null
        };
        return;
      }
      case 'mcp__app__open_jira_tab': {
        // Build a Partial<JiraFilters> from only the keys the agent
        // actually sent. `updateJiraTabFilters` merges and persists
        // and triggers a debounced re-fetch — same code path JiraTab's
        // dropdowns use, so we get UI parity for free. Skipping a key
        // leaves that filter alone; matches the tool's "omitted =
        // unchanged" contract.
        const patch: Partial<JiraFilters> = {};
        if ('project_key' in input) patch.projectKey = str('project_key') || null;
        if ('search' in input) patch.search = str('search');
        if ('status_name' in input) patch.statusName = str('status_name') || null;
        if (Array.isArray(input.board_ids)) {
          patch.boardIds = input.board_ids
            .map((x) => Number(x))
            .filter((x): x is number => Number.isFinite(x) && x > 0);
        }
        if (Array.isArray(input.sprint_ids)) {
          patch.sprintIds = parseSprintScopes(input.sprint_ids);
        }
        view = 'jiraTab';
        updateJiraTabFilters(patch);
        return;
      }
      case 'mcp__app__open_sentry_tab': {
        // SentryTab fields are flat on `inboxState` (not under one
        // filter object), so we can't reuse a setSentryFilters-style
        // patch. Mutate field-by-field — `scheduleSentryTabFilterRefresh`
        // persists and re-runs the query the same way the dropdown
        // change handlers do.
        view = 'sentryTab';
        if (Array.isArray(input.projects)) {
          inboxState.sentryTabProjects = input.projects
            .map((x) => String(x))
            .filter((s) => s.length > 0);
        }
        if ('search' in input) inboxState.sentryTabSearch = str('search');
        if ('status' in input) {
          const s = str('status');
          if (s) inboxState.sentryTabStatus = s as typeof inboxState.sentryTabStatus;
        }
        if ('level' in input) {
          const l = str('level');
          if (l) inboxState.sentryTabLevel = l as typeof inboxState.sentryTabLevel;
        }
        if ('environment' in input) {
          const e = str('environment');
          inboxState.sentryTabEnvironment = e ? e : null;
        }
        scheduleSentryTabFilterRefresh();
        return;
      }
      case 'mcp__app__set_github_column': {
        const inst = findInstanceByNameOrId('github', str('instance_name'), str('instance_id'));
        if (!inst) return;
        const patch: Partial<GithubFilters> = {};
        if ('repo' in input) {
          // Empty string = "clear filter" (= all repos).
          const r = str('repo');
          patch.repo = r ? r : null;
        }
        if ('mode' in input) {
          const m = str('mode');
          if (isGithubFilterMode(m)) patch.mode = m;
        }
        if ('search' in input) patch.search = str('search');
        if ('custom_user' in input) patch.customUser = str('custom_user');
        view = 'workbench';
        updateGithubFilters(inst.id, patch);
        void scrollInstanceIntoView(inst.id);
        return;
      }
      case 'mcp__app__set_jira_column': {
        const inst = findInstanceByNameOrId('jira', str('instance_name'), str('instance_id'));
        if (!inst) return;
        const patch: Partial<JiraFilters> = {};
        if ('project_key' in input) {
          const p = str('project_key');
          patch.projectKey = p ? p : null;
        }
        if ('status_name' in input) {
          const s = str('status_name');
          patch.statusName = s ? s : null;
        }
        if ('search' in input) patch.search = str('search');
        if (Array.isArray(input.board_ids)) {
          patch.boardIds = input.board_ids
            .map((x) => Number(x))
            .filter((x): x is number => Number.isFinite(x) && x > 0);
        }
        if (Array.isArray(input.sprint_ids)) {
          patch.sprintIds = parseSprintScopes(input.sprint_ids);
        }
        view = 'workbench';
        updateJiraFilters(inst.id, patch);
        void scrollInstanceIntoView(inst.id);
        return;
      }
      case 'mcp__app__set_sentry_column': {
        const inst = findInstanceByNameOrId('sentry', str('instance_name'), str('instance_id'));
        if (!inst) return;
        const patch: SentryFilterPatch = {};
        if (Array.isArray(input.projects)) {
          patch.projects = input.projects
            .map((x) => String(x))
            .filter((s) => s.length > 0);
        }
        if ('search' in input) patch.search = str('search');
        if ('status' in input) {
          const s = str('status');
          if (isSentryStatus(s)) patch.status = s;
        }
        if ('level' in input) {
          const l = str('level');
          if (isSentryLevel(l)) patch.level = l;
        }
        if ('environment' in input) {
          const e = str('environment');
          patch.environment = e ? e : null;
        }
        view = 'workbench';
        setSentryFilters(inst.id, patch);
        void scrollInstanceIntoView(inst.id);
        return;
      }
      case 'mcp__app__set_editor_repo_path': {
        // Use `pickDeep` instead of `pick`: cursor-agent has shipped
        // this payload wrapped in `args` / `arguments`, with
        // `repo_path` as a single-element array, and with non-canonical
        // keys (`folderPath`, `fullPath`, …). pickDeep mirrors the
        // sidecar's recursive search so frontend and backend accept
        // the exact same shapes.
        const repoPath = pickDeep(input as Record<string, unknown>, REPO_PATH_KEYS_DEEP);
        const instName = pickDeep(input as Record<string, unknown>, INSTANCE_NAME_KEYS_DEEP);
        const instId = pickDeep(input as Record<string, unknown>, INSTANCE_ID_KEYS_DEEP);
        if (!repoPath) return;
        const editor = findInstanceByNameOrId('editor', instName, instId);
        if (!editor) return;
        view = 'workbench';
        setEditorRepoPath(repoPath, editor.id);
        // Linked agents follow. `applySessionCwd` rotates the agent's
        // claudeUuid + resets `claudeResumable` when the new cwd actually
        // differs — necessary because Claude CLI scopes conversations by
        // project (cwd-derived); resuming an old uuid in a new project
        // fails with "No conversation found".
        for (const s of sessionsState.list) {
          if (s.linkedToEditor && s.linkedToEditorInstanceId === editor.id) {
            applySessionCwd(s.id, repoPath, { breakLink: false });
          }
        }
        void scrollInstanceIntoView(editor.id);
        return;
      }
      case 'mcp__app__set_agent_cwd': {
        // Same pickDeep contract as set_editor_repo_path — keep the
        // two in sync so the LLM doesn't need a different schema for
        // each.
        const repoPath = pickDeep(input as Record<string, unknown>, REPO_PATH_KEYS_DEEP);
        if (!repoPath) return;
        const target = str('target').toLowerCase();
        let sessId: string | null = null;
        if (target === 'self') {
          sessId = _sessionId;
        } else {
          const instName = pickDeep(input as Record<string, unknown>, INSTANCE_NAME_KEYS_DEEP);
          const instId = pickDeep(input as Record<string, unknown>, INSTANCE_ID_KEYS_DEEP);
          // Try claude first, then cursor — same pool from the user's POV.
          const inst = findInstanceByNameOrId('claude', instName, instId)
            ?? findInstanceByNameOrId('cursor', instName, instId);
          if (inst) {
            view = 'workbench';
            void scrollInstanceIntoView(inst.id);
            sessId = sessionsState.activeByInstance[inst.id] ?? null;
          }
        }
        if (!sessId) return;
        applySessionCwd(sessId, repoPath, { breakLink: false });
        return;
      }
      case 'mcp__app__list_instances': {
        // No-op: the data lives in the system-prompt preamble and is
        // refreshed on every turn. The sidecar's tool reply explains.
        return;
      }
      /* ---- Canvas (whiteboard) ---- */
      case 'mcp__app__canvas_add_shape': {
        const canvasId = linkedCanvasIdFor(_sessionId);
        if (!canvasId) return;
        const kind = str('kind') as ShapeKind;
        if (!kind) return;
        const x = num('x'); const y = num('y');
        const w = num('w'); const h = num('h');
        if (!Number.isFinite(x) || !Number.isFinite(y) || !(w > 0) || !(h > 0)) return;
        const props = (input.props && typeof input.props === 'object')
          ? (input.props as Record<string, unknown>)
          : undefined;
        const label = typeof input.label === 'string' ? (input.label as string) : null;
        const desiredId = str('shape_id');
        const shape = makeShape({
          kind, x, y, w, h, props, label, createdBy: 'agent'
        });
        if (desiredId) shape.id = desiredId;
        canvasAddShape(canvasId, shape);
        return;
      }
      case 'mcp__app__canvas_add_shapes': {
        const canvasId = linkedCanvasIdFor(_sessionId);
        if (!canvasId) return;
        const arr = Array.isArray(input.shapes) ? input.shapes : [];
        const shapes: Shape[] = [];
        for (const raw of arr) {
          if (!raw || typeof raw !== 'object') continue;
          const s = raw as Record<string, unknown>;
          const kind = typeof s.kind === 'string' ? s.kind as ShapeKind : null;
          if (!kind) continue;
          const x = Number(s.x); const y = Number(s.y);
          const w = Number(s.w); const h = Number(s.h);
          if (!Number.isFinite(x) || !Number.isFinite(y) || !(w > 0) || !(h > 0)) continue;
          const sh = makeShape({
            kind, x, y, w, h,
            props: (s.props && typeof s.props === 'object') ? (s.props as Record<string, unknown>) : undefined,
            label: typeof s.label === 'string' ? s.label : null,
            createdBy: 'agent'
          });
          if (typeof s.shape_id === 'string' && s.shape_id) sh.id = s.shape_id;
          shapes.push(sh);
        }
        if (shapes.length > 0) canvasAddShapes(canvasId, shapes);
        return;
      }
      case 'mcp__app__canvas_update_shape': {
        const canvasId = linkedCanvasIdFor(_sessionId);
        if (!canvasId) return;
        const shapeId = str('shape_id');
        if (!shapeId) return;
        const patch: Partial<Shape> = {};
        if (typeof input.x === 'number') patch.x = input.x as number;
        if (typeof input.y === 'number') patch.y = input.y as number;
        if (typeof input.w === 'number' && (input.w as number) > 0) patch.w = input.w as number;
        if (typeof input.h === 'number' && (input.h as number) > 0) patch.h = input.h as number;
        if (typeof input.rot === 'number') patch.rot = input.rot as number;
        if (typeof input.label === 'string') patch.label = input.label as string;
        if (input.props && typeof input.props === 'object') {
          /* Merge with the shape's existing props rather than replacing,
             so callers can patch a single field (`{props:{source:"..."}}`)
             without losing tint / theme / etc. */
          const c = ensureCanvasLoaded(canvasId);
          const cur = c?.shapes.find((s) => s.id === shapeId);
          patch.props = { ...(cur?.props ?? {}), ...(input.props as Record<string, unknown>) };
        }
        if (Object.keys(patch).length === 0) return;
        canvasPatchShape(canvasId, shapeId, patch);
        return;
      }
      case 'mcp__app__canvas_delete_shape': {
        const canvasId = linkedCanvasIdFor(_sessionId);
        if (!canvasId) return;
        const ids: string[] = [];
        const single = str('shape_id');
        if (single) ids.push(single);
        if (Array.isArray(input.shape_ids)) {
          for (const v of input.shape_ids) if (typeof v === 'string' && v) ids.push(v);
        }
        if (ids.length > 0) canvasDeleteShapes(canvasId, ids);
        return;
      }
      case 'mcp__app__canvas_add_edge': {
        const canvasId = linkedCanvasIdFor(_sessionId);
        if (!canvasId) return;
        const edge = parseEdgeSpec(input);
        if (edge) canvasAddEdge(canvasId, edge);
        return;
      }
      case 'mcp__app__canvas_add_edges': {
        const canvasId = linkedCanvasIdFor(_sessionId);
        if (!canvasId) return;
        /* Accept the canonical `edges` plus the same aliases the
           sidecar declares (`connections` / `links` / `arrows`). */
        const arr = (input.edges ?? input.connections ?? input.links ?? input.arrows);
        if (!Array.isArray(arr)) return;
        for (const raw of arr) {
          if (!raw || typeof raw !== 'object') continue;
          const edge = parseEdgeSpec(raw as Record<string, unknown>);
          if (edge) canvasAddEdge(canvasId, edge);
        }
        return;
      }
      case 'mcp__app__canvas_delete_edge': {
        const canvasId = linkedCanvasIdFor(_sessionId);
        if (!canvasId) return;
        const ids: string[] = [];
        const single = str('edge_id');
        if (single) ids.push(single);
        if (Array.isArray(input.edge_ids)) {
          for (const v of input.edge_ids) if (typeof v === 'string' && v) ids.push(v);
        }
        if (ids.length > 0) canvasDeleteEdges(canvasId, ids);
        return;
      }
      case 'mcp__app__canvas_arrange': {
        const canvasId = linkedCanvasIdFor(_sessionId);
        if (!canvasId) return;
        const algo = str('algorithm') as LayoutAlgorithm;
        if (!['grid', 'row', 'column', 'dagre'].includes(algo)) return;
        const ids = Array.isArray(input.shape_ids)
          ? (input.shape_ids as unknown[]).filter((v): v is string => typeof v === 'string')
          : undefined;
        const opts: Record<string, unknown> = {};
        if (typeof input.rankdir === 'string') opts.rankdir = input.rankdir;
        if (typeof input.gap === 'number') opts.gap = input.gap;
        void canvasApplyLayout(canvasId, algo, ids, opts);
        return;
      }
      case 'mcp__app__canvas_focus': {
        const canvasId = linkedCanvasIdFor(_sessionId);
        if (!canvasId) return;
        const shapeId = str('shape_id');
        if (!shapeId) return;
        requestCanvasFocus(canvasId, shapeId);
        return;
      }
      case 'mcp__app__canvas_set_z': {
        const canvasId = linkedCanvasIdFor(_sessionId);
        if (!canvasId) return;
        const shapeId = str('shape_id');
        const mode = str('mode');
        if (!shapeId) return;
        if (!['to-front', 'to-back', 'forward', 'backward'].includes(mode)) return;
        canvasSetShapeZ(canvasId, shapeId, mode as 'to-front' | 'to-back' | 'forward' | 'backward');
        return;
      }
      case 'mcp__app__canvas_duplicate': {
        const canvasId = linkedCanvasIdFor(_sessionId);
        if (!canvasId) return;
        const ids = Array.isArray(input.shape_ids)
          ? (input.shape_ids as unknown[]).filter((v): v is string => typeof v === 'string' && v.length > 0)
          : [];
        if (ids.length === 0) return;
        const dx = typeof input.dx === 'number' ? input.dx : 12;
        const dy = typeof input.dy === 'number' ? input.dy : 12;
        canvasDuplicateShapes(canvasId, ids, dx, dy);
        return;
      }
      case 'mcp__app__canvas_find': {
        const canvasId = linkedCanvasIdFor(_sessionId);
        if (!canvasId) return;
        const query = str('query');
        if (!query) return;
        const ids = canvasFindShapes(canvasId, query);
        /* `find` is a read — but our sidecar reply is just a
           confirmation, so returning data through the agent would
           require either an IPC bridge or a follow-up message. We
           DO change UI state: select the matches so the user can
           visually see what the agent found. The agent's next-turn
           system-prompt preamble will reflect the new selection
           context (selection is ephemeral so it doesn't pollute
           saved canvas state). */
        if (ids.length > 0) canvasSetSelection(canvasId, ids);
        return;
      }
      case 'mcp__app__canvas_group': {
        const canvasId = linkedCanvasIdFor(_sessionId);
        if (!canvasId) return;
        const ids = Array.isArray(input.shape_ids)
          ? (input.shape_ids as unknown[]).filter((v): v is string => typeof v === 'string' && v.length > 0)
          : [];
        if (ids.length === 0) return;
        const kind = input.kind === 'group' ? 'group' : 'frame';
        const title = typeof input.title === 'string' ? input.title : undefined;
        canvasGroupShapes(canvasId, ids, { kind, title });
        return;
      }
      case 'mcp__app__canvas_ungroup': {
        const canvasId = linkedCanvasIdFor(_sessionId);
        if (!canvasId) return;
        const shapeId = str('shape_id');
        if (!shapeId) return;
        canvasUngroupShapes(canvasId, shapeId);
        return;
      }
      case 'mcp__app__canvas_lock': {
        const canvasId = linkedCanvasIdFor(_sessionId);
        if (!canvasId) return;
        const ids = Array.isArray(input.shape_ids)
          ? (input.shape_ids as unknown[]).filter((v): v is string => typeof v === 'string' && v.length > 0)
          : [];
        if (ids.length === 0) return;
        const locked = input.locked === true;
        canvasSetShapesLocked(canvasId, ids, locked);
        return;
      }
      case 'mcp__app__canvas_align': {
        const canvasId = linkedCanvasIdFor(_sessionId);
        if (!canvasId) return;
        const ids = Array.isArray(input.shape_ids)
          ? (input.shape_ids as unknown[]).filter((v): v is string => typeof v === 'string' && v.length > 0)
          : [];
        const axis = str('axis');
        const validAxes: AlignAxis[] = ['left', 'center-x', 'right', 'top', 'center-y', 'bottom'];
        if (ids.length < 2 || !(validAxes as string[]).includes(axis)) return;
        canvasAlignShapes(canvasId, ids, axis as AlignAxis);
        return;
      }
      case 'mcp__app__canvas_distribute': {
        const canvasId = linkedCanvasIdFor(_sessionId);
        if (!canvasId) return;
        const ids = Array.isArray(input.shape_ids)
          ? (input.shape_ids as unknown[]).filter((v): v is string => typeof v === 'string' && v.length > 0)
          : [];
        const axis = str('axis');
        if (ids.length < 3 || (axis !== 'horizontal' && axis !== 'vertical')) return;
        canvasDistributeShapes(canvasId, ids, axis as DistributeAxis);
        return;
      }
      case 'mcp__app__canvas_set_viewport': {
        const canvasId = linkedCanvasIdFor(_sessionId);
        if (!canvasId) return;
        const x = num('x'); const y = num('y');
        if (!Number.isFinite(x) || !Number.isFinite(y)) return;
        const c = ensureCanvasLoaded(canvasId);
        if (!c) return;
        const z = typeof input.zoom === 'number' && input.zoom > 0
          ? Math.max(0.1, Math.min(4, input.zoom))
          : c.viewport.zoom;
        canvasSetViewport(canvasId, { x, y, zoom: z });
        return;
      }
      case 'mcp__app__canvas_upload_image': {
        const canvasId = linkedCanvasIdFor(_sessionId);
        if (!canvasId) return;
        const b64 = str('base64');
        if (!b64) return;
        const mime = str('mime_type') || 'image/png';
        const dataUrl = `data:${mime};base64,${b64}`;
        /* Use Image() to read intrinsic dimensions; fall back to a
           default size if decode fails. We can't await inside this
           switch elegantly, so this branch fires off an async task
           that creates the shape once dimensions resolve. */
        const c = ensureCanvasLoaded(canvasId);
        if (!c) return;
        const desiredX = typeof input.x === 'number' ? input.x : (c.viewport.x + 100);
        const desiredY = typeof input.y === 'number' ? input.y : (c.viewport.y + 100);
        const desiredId = str('shape_id');
        const alt = typeof input.alt === 'string' ? input.alt : null;
        void (async () => {
          const dim = await new Promise<{ w: number; h: number }>((resolve) => {
            const img = new Image();
            img.onerror = () => resolve({ w: 320, h: 200 });
            img.onload = () => resolve({ w: img.naturalWidth || 320, h: img.naturalHeight || 200 });
            img.src = dataUrl;
          });
          const MAX_DIM = 480;
          let outW = dim.w, outH = dim.h;
          if (dim.w > MAX_DIM || dim.h > MAX_DIM) {
            const k = Math.min(MAX_DIM / dim.w, MAX_DIM / dim.h);
            outW = Math.round(dim.w * k);
            outH = Math.round(dim.h * k);
          }
          const shape = makeShape({
            kind: 'image',
            x: desiredX,
            y: desiredY,
            w: outW,
            h: outH,
            props: { dataUrl, intrinsicWidth: dim.w, intrinsicHeight: dim.h, alt }
          });
          if (desiredId) shape.id = desiredId;
          canvasAddShape(canvasId, shape);
        })();
        return;
      }
    }
  }

  /** Resolve which canvas a session is linked to. Returns null when the
   *  session has no link or its referenced canvas was deleted. The
   *  canvas tool dispatchers all gate on this so the agent's tool
   *  call is a no-op when the link is broken (rather than mutating an
   *  unrelated canvas). */
  function linkedCanvasIdFor(sessionId: string): string | null {
    const s = sessionsState.list.find((x) => x.id === sessionId);
    if (!s || !s.linkedCanvasId) return null;
    /* Validate the canvas still exists in the library. Index lookup
       is O(N) but the index is tiny (typically <50 entries). */
    const exists = canvasState.index.find((e) => e.id === s.linkedCanvasId);
    if (!exists) return null;
    /* Hydrate so subsequent ops find it under canvasState.open. */
    ensureCanvasLoaded(s.linkedCanvasId);
    return s.linkedCanvasId;
  }

  /** Look up a workbench instance by name or id within a given kind, across
   *  every workbench (not just the active one). Returns the first match —
   *  art-names are unique pool entries within a workbench, but the agent
   *  might reference one that's in a different workbench. */
  function findInstanceByNameOrId(
    kind: PanelKind,
    name: string,
    id: string
  ): PanelInstance | null {
    const wantName = name.trim().toLowerCase();
    const wantId = id.trim();
    for (const wb of layoutState.workbenches) {
      for (const inst of wb.instances) {
        if (inst.kind !== kind) continue;
        if (wantId && inst.id === wantId) return inst;
        if (wantName && inst.name.toLowerCase() === wantName) return inst;
      }
    }
    return null;
  }

  /** Pull a single GitHub item by `(owner, repo, number)` and slot it into
   *  the focus pane. Used by the `open_github_pr` / `open_github_issue`
   *  app-navigation tools. Best-effort — if the fetch fails we just
   *  swallow; the chat still shows what the agent tried.
   *
   *  `tabHint` is only used when the agent explicitly asked for a
   *  non-default tab (e.g. "open #123 on the files tab"); otherwise we
   *  reset to `conversation` so a fresh PR doesn't inherit the previous
   *  one's tab. */
  async function resolveGithubFocus(
    owner: string,
    repo: string,
    number: number,
    tabHint: DetailTab | null = null
  ) {
    try {
      const item = await invoke<InboxItem>('github_get_inbox_item', {
        owner,
        repo,
        number
      });
      openFocusItem(item);
      tab = tabHint ?? 'conversation';
    } catch (e) {
      console.warn('open_github_pr resolution failed:', e);
    }
  }

  /** Dispatch a double-click on a Canvas live-card to the right
   *  source-specific navigation. Uses the same nav primitives the
   *  agent's `mcp__app__open_*` tools and the inbox cards themselves
   *  call — so a click on a Jira card on the canvas behaves identically
   *  to a click on the Jira card in the column inbox. */
  function openCanvasCardSource(shape: Shape) {
    const p = shape.props as Record<string, unknown>;
    switch (shape.kind) {
      case 'jira-card': {
        const key = typeof p.ticketKey === 'string' ? p.ticketKey : '';
        if (key) inboxState.jiraFocusKey = key;
        return;
      }
      case 'github-pr-card':
      case 'github-issue-card': {
        const owner = typeof p.owner === 'string' ? p.owner : '';
        const repo = typeof p.repo === 'string' ? p.repo : '';
        const number = typeof p.number === 'number' ? p.number : 0;
        if (owner && repo && number > 0) {
          void resolveGithubFocus(owner, repo, number, null);
        }
        return;
      }
      case 'sentry-event-card': {
        const issueId = typeof p.issueId === 'string' ? p.issueId : '';
        const shortId = typeof p.shortId === 'string' ? p.shortId : '';
        const id = issueId || shortId;
        if (id) openSentryFocus(id);
        return;
      }
      case 'file-card': {
        const relPath = typeof p.relPath === 'string' ? p.relPath : '';
        if (!relPath) return;
        /* For file cards we let editorNavigation pick the right editor
           instance (linked editor wins, then first available, otherwise
           a new one is created). */
        void openFileInEditor(relPath, {});
        return;
      }
      case 'chat-message-card': {
        /* Locate the column the source session belongs to and scroll
           it into view, then activate the session there. The session
           may have been moved since the card was pinned; we look it up
           live from sessionsState to handle that. */
        const sessionId = typeof p.sessionId === 'string' ? p.sessionId : '';
        if (!sessionId) return;
        const sess = sessionsState.list.find((s) => s.id === sessionId);
        if (!sess?.columnInstanceId) return;
        const found = findInstanceAnywhere(sess.columnInstanceId);
        if (!found) return;
        void goToInstance(found.inst.id, found.wb.id);
        setActiveSessionInColumn(found.inst.id, sessionId);
        return;
      }
    }
  }

  // ---- Message replay / edit ----
  let editingMsg = $state<{ sessionId: string; index: number; draft: string } | null>(null);

  // Workbench-tab inline rename state.
  let editingWorkbench = $state<{ id: string; draft: string } | null>(null);

  // Workbench-bar pill hover/click state. A delayed leave lets the user
  // slide their mouse from the pill onto the menu without it snapping
  // shut mid-transit.
  let hoveredPill = $state<PanelKind | null>(null);
  let pillLeaveTimer: ReturnType<typeof setTimeout> | null = null;
  function onPillEnter(kind: PanelKind) {
    if (pillLeaveTimer) {
      clearTimeout(pillLeaveTimer);
      pillLeaveTimer = null;
    }
    hoveredPill = kind;
  }
  function onPillLeave() {
    if (pillLeaveTimer) clearTimeout(pillLeaveTimer);
    pillLeaveTimer = setTimeout(() => {
      hoveredPill = null;
      pillLeaveTimer = null;
    }, 140);
  }
  /** Click handler for the main pill body. Prefers an instance in the
      active workbench; otherwise jumps (and switches workbench) to the
      first one found. Creates nothing — the `+` button owns creation. */
  function navToKind(kind: PanelKind) {
    const insts = listInstancesOfKind(kind);
    if (insts.length === 0) return;
    const inCurrent = insts.find(
      (i) => i.workbenchId === layoutState.activeWorkbenchId
    );
    const target = inCurrent ?? insts[0];
    void goToInstance(target.id, target.workbenchId);
  }

  // ---- Pill drop targets ----
  // Drag a Jira/GH ticket (or a file from the Editor tree) onto a pill and
  // drop it on a specific column instance — even one living in a different
  // workbench. Cross-workbench "attach this to that chat" shortcut.
  let pillDragOverKind = $state<PanelKind | null>(null);
  let pillDragOverInstance = $state<string | null>(null);
  // Delay clearing the drag-over highlight by 140ms so the cursor can travel
  // from the pill body into the menu (or between sibling menu items) without
  // the menu snapping shut mid-transit. Mirrors the hover-leave behavior.
  let pillDragLeaveTimer: ReturnType<typeof setTimeout> | null = null;

  /** Only agent pills accept drops — github/jira/editor don't host
      sessions that can take @mentions. Require something droppable:
      internal drag payload (ticket / file-tree) or OS files in the
      dataTransfer. */
  function pillCanAccept(e: DragEvent, kind: PanelKind): boolean {
    if (kind !== 'claude' && kind !== 'cursor') return false;
    const types = e.dataTransfer?.types;
    if (dragState.payload) return true;
    if (types?.includes('application/x-forgehold-file')) return true;
    if (types && types.length > 0) return true;
    return false;
  }

  // Spring-loaded pill menu, macOS Finder-style. Hovering a pill mid-drag
  // opens its menu after a short delay (`PILL_OPEN_DELAY`); leaving keeps it
  // open for `PILL_CLOSE_DELAY` so the cursor can travel from pill body to
  // a menu-item or between sibling items without snapping shut.
  //
  // The counter (`pillDragCount`) collapses dragenter/dragleave from child
  // nodes — without it every dragenter on a menu item would count as a new
  // entry and dragleave on the previous would force-close the menu.
  const PILL_OPEN_DELAY = 220;
  const PILL_CLOSE_DELAY = 160;
  const pillDragCounts = new Map<PanelKind, number>();
  let pillOpenTimer: ReturnType<typeof setTimeout> | null = null;

  function clearPillTimers() {
    if (pillOpenTimer) { clearTimeout(pillOpenTimer); pillOpenTimer = null; }
    if (pillDragLeaveTimer) { clearTimeout(pillDragLeaveTimer); pillDragLeaveTimer = null; }
  }

  function onPillDragEnter(e: DragEvent, kind: PanelKind, instanceId: string | null) {
    if (!pillCanAccept(e, kind)) return;
    e.preventDefault();
    const cur = pillDragCounts.get(kind) ?? 0;
    pillDragCounts.set(kind, cur + 1);
    // Cancel any pending close — cursor came back.
    if (pillDragLeaveTimer) { clearTimeout(pillDragLeaveTimer); pillDragLeaveTimer = null; }
    if (pillDragOverKind === kind) {
      // Already over this pill — instance change is instant (no flash).
      if (instanceId !== null) pillDragOverInstance = instanceId;
      return;
    }
    // Spring-load: open after a short delay. Cancels if cursor leaves before.
    if (pillOpenTimer) clearTimeout(pillOpenTimer);
    pillOpenTimer = setTimeout(() => {
      pillDragOverKind = kind;
      pillDragOverInstance = instanceId;
      pillOpenTimer = null;
    }, PILL_OPEN_DELAY);
  }

  function onPillDragOver(e: DragEvent, kind: PanelKind, instanceId: string | null) {
    if (!pillCanAccept(e, kind)) return;
    e.preventDefault();
    e.stopPropagation();
    if (e.dataTransfer) e.dataTransfer.dropEffect = 'copy';
    // If the menu is already open and the cursor is over a specific
    // instance row, surface that instance immediately for the drop-target
    // accent — no debouncing here, just track the live target.
    if (pillDragOverKind === kind && instanceId !== null) {
      pillDragOverInstance = instanceId;
    }
  }

  function onPillDragLeave(kind: PanelKind, instanceId: string | null) {
    const cur = pillDragCounts.get(kind) ?? 0;
    if (cur > 1) {
      pillDragCounts.set(kind, cur - 1);
      return;
    }
    // Last leave for this pill — schedule close.
    pillDragCounts.delete(kind);
    if (pillOpenTimer) { clearTimeout(pillOpenTimer); pillOpenTimer = null; }
    if (pillDragOverKind !== kind) return;
    if (instanceId !== null && pillDragOverInstance !== instanceId) {
      // Just left the specific menu-item but still inside the pill area —
      // counter > 0 case is handled above; if we got here, fall through.
    }
    if (pillDragLeaveTimer) clearTimeout(pillDragLeaveTimer);
    pillDragLeaveTimer = setTimeout(() => {
      pillDragOverKind = null;
      pillDragOverInstance = null;
      pillDragLeaveTimer = null;
    }, PILL_CLOSE_DELAY);
  }

  // ---- Workbench-tab drop targets (move a column to another workbench) ----
  // The column header's "move" button (ColumnControls.svelte) sets a custom
  // mime `application/x-forgehold-column` with the instanceId. Workbench
  // tabs accept that mime and call `moveInstanceToWorkbench`. Same UX as
  // dragging tabs in iTerm/Chrome — drop the column onto a workbench name
  // and it relocates there.
  let tabDragOverId = $state<string | null>(null);

  function tabAcceptsDrop(e: DragEvent): boolean {
    return !!e.dataTransfer?.types.includes('application/x-forgehold-column');
  }

  function onTabDragEnter(e: DragEvent, wbId: string) {
    if (!tabAcceptsDrop(e)) return;
    e.preventDefault();
    tabDragOverId = wbId;
  }

  function onTabDragOver(e: DragEvent, wbId: string) {
    if (!tabAcceptsDrop(e)) return;
    e.preventDefault();
    if (e.dataTransfer) e.dataTransfer.dropEffect = 'move';
    tabDragOverId = wbId;
  }

  function onTabDragLeave(wbId: string) {
    if (tabDragOverId === wbId) tabDragOverId = null;
  }

  function onTabDrop(e: DragEvent, wbId: string) {
    if (!tabAcceptsDrop(e)) return;
    e.preventDefault();
    tabDragOverId = null;
    const instanceId = e.dataTransfer?.getData('application/x-forgehold-column');
    if (!instanceId) return;
    const target = layoutState.workbenches.find((w) => w.id === wbId);
    if (!target) return;
    const ok = moveInstanceToWorkbench(instanceId, wbId);
    if (ok) {
      // Switch to the destination so the user sees the result of their drag.
      setActiveWorkbench(wbId);
      notify({ kind: 'success', title: `Moved to ${target.name}`, ttlMs: 2000 });
    } else {
      notify({
        kind: 'warning',
        title: "Couldn't move column",
        body: 'Source or target workbench disappeared.'
      });
    }
  }

  function onPillDrop(e: DragEvent, kind: PanelKind, specificInstanceId: string | null) {
    if (!pillCanAccept(e, kind)) return;
    e.preventDefault();
    e.stopPropagation();
    clearPillTimers();
    pillDragCounts.clear();
    pillDragOverKind = null;
    pillDragOverInstance = null;

    const insts = listInstancesOfKind(kind);
    if (insts.length === 0) return;
    const target = specificInstanceId
      ? insts.find((i) => i.id === specificInstanceId)
      : insts.find((i) => i.workbenchId === layoutState.activeWorkbenchId) ?? insts[0];
    if (!target) return;

    // Switch workbench if the target column lives elsewhere, so the user
    // visibly "lands" on it.
    if (target.workbenchId !== layoutState.activeWorkbenchId) {
      setActiveWorkbench(target.workbenchId);
    }
    void scrollInstanceIntoView(target.id);

    // Reuse the per-column drop logic — same event, specific instanceId.
    onAgentDrop(target.id, kind as 'claude' | 'cursor', e);
  }

  function startWorkbenchRename(id: string, current: string) {
    editingWorkbench = { id, draft: current };
  }
  function commitWorkbenchRename() {
    if (!editingWorkbench) return;
    const { id, draft } = editingWorkbench;
    editingWorkbench = null;
    if (draft.trim()) renameWorkbench(id, draft.trim());
  }
  function askRemoveWorkbench(id: string) {
    const wb = layoutState.workbenches.find((w) => w.id === id);
    if (!wb) return;
    if (layoutState.workbenches.length <= 1) return;
    if (wb.instances.length > 0) {
      const ok = confirm(
        `Delete workbench "${wb.name}"?\n\n` +
        `It has ${wb.instances.length} column(s). Sessions pinned to those columns will float back and reattach to the first matching column in another workbench.`
      );
      if (!ok) return;
    }
    removeWorkbench(id);
  }

  function startEditMessage(sessionId: string, index: number, content: string) {
    editingMsg = { sessionId, index, draft: content };
  }

  async function commitEditMessage() {
    if (!editingMsg) return;
    const { sessionId, index, draft } = editingMsg;
    const trimmed = draft.trim();
    if (!trimmed) return;
    editingMsg = null;
    // Snapshot the images attached to the message we're editing BEFORE the
    // truncate drops it from the transcript — so resending still carries the
    // same vision payload.
    const orig = sessionsState.list.find((s) => s.id === sessionId)?.messages[index];
    const images = orig?.images ?? [];
    sessionsState.activeClaudeId = sessionId;
    truncateSessionAt(sessionId, index);
    if (images.length) {
      attachPathsToSession(sessionId, images.map((i) => i.path));
    }
    setSessionInput(sessionId, trimmed);
    await sendClaudeMessage();
  }

  async function resendMessage(sessionId: string, index: number, content: string) {
    const ok = confirm(
      'Resend this message?\n\nEverything after it (Claude\'s replies, your later messages, pending action cards) will be erased.'
    );
    if (!ok) return;
    const orig = sessionsState.list.find((s) => s.id === sessionId)?.messages[index];
    const images = orig?.images ?? [];
    sessionsState.activeClaudeId = sessionId;
    truncateSessionAt(sessionId, index);
    if (images.length) {
      attachPathsToSession(sessionId, images.map((i) => i.path));
    }
    setSessionInput(sessionId, content);
    await sendClaudeMessage();
  }

  let repoInfoTimer: ReturnType<typeof setTimeout> | null = null;
  function scheduleRepoInfoRefresh() {
    if (repoInfoTimer) clearTimeout(repoInfoTimer);
    repoInfoTimer = setTimeout(async () => {
      const target = activeSession ? effectiveCwd(activeSession) : null;
      if (!target) {
        activeRepoInfo = null;
        return;
      }
      try {
        activeRepoInfo = await invoke<RepoInfo>('git_repo_info', { path: target });
      } catch {
        activeRepoInfo = null;
      }
    }, 150);
  }

  $effect(() => {
    // Re-fetch repo info whenever the effective cwd for the active session changes.
    const target = activeSession ? effectiveCwd(activeSession) : null;
    // eslint-disable-next-line @typescript-eslint/no-unused-expressions
    target;
    scheduleRepoInfoRefresh();
  });

  // Action execution is in `$lib/exec/actions.ts`. The bash executor needs
  // `appendAssistantDelta` (DOM-coupled scroll) injected from here.
  // `onActionResolved` auto-continues the agent's turn — when the agent
  // ended its prior turn waiting on an approval card (commit / PR /
  // bash / switch_cwd), the result of running it is fed back as a
  // synthesised user message so the agent can continue from there
  // (e.g. propose_pr after the commit lands) without the user having
  // to manually type "now make the PR".
  function executeAction(sessionId: string, action: ClaudeAction) {
    dispatchAction(sessionId, action, appendAssistantDelta, onActionResolved);
  }

  /** Per-session re-entry guard. Two cards finishing in the same
   *  microtask both pass the `stillBusy=false` check above (Svelte
   *  state writes aren't synchronous gates) and would each fire
   *  `continueAgentTurn`, so the agent gets the same recap twice and
   *  produces a duplicate turn. The Set tracks "continuation already
   *  fired for this batch" — entries are cleared when continueAgentTurn
   *  finishes (in finally) so the next user-initiated batch can fire. */
  const continuationInFlight = new Set<string>();

  /** Called by every executeXxx after the action ran. If the session
   *  was awaiting approval AND no other actions are still pending,
   *  fire a follow-up agent turn with the result baked in. */
  function onActionResolved(
    sessionId: string,
    _action: ClaudeAction,
    result: { ok: boolean; summary: string }
  ) {
    const sess = sessionsState.list.find((s) => s.id === sessionId);
    if (!sess || !sess.awaitingApproval) return;
    // Wait for ALL pending actions before continuing — agent may have
    // proposed a sequence (commit + PR) that we want to resolve in one
    // batch. `executing` counts as "still in flight".
    const stillBusy = sess.actions.some(
      (a) => a.status === 'pending' || a.status === 'executing'
    );
    if (stillBusy) return;
    if (continuationInFlight.has(sessionId)) return;
    continuationInFlight.add(sessionId);
    updateSession(sessionId, { awaitingApproval: false });
    // Compose a recap of every action that ran since the last user turn.
    // For a single action it's just the one summary; for a batch the
    // agent gets the whole list in order.
    const recentActionSummaries = sess.actions
      .filter((a) => a.status === 'done' || a.status === 'error')
      .map((a) => `- ${a.kind}: ${a.status === 'done' ? '✓' : '✗'} ${actionShortSummary(a)}`)
      .join('\n');
    const continuation = recentActionSummaries
      ? `[Forgehold: action card resolved]\n${recentActionSummaries}\n\nLast result: ${result.ok ? '✓' : '✗'} ${result.summary}\n\nContinue with what you were doing.`
      : `[Forgehold: action resolved]\n${result.ok ? '✓' : '✗'} ${result.summary}\n\nContinue with what you were doing.`;
    void continueAgentTurn(sessionId, continuation);
  }

  /** Render an action card down to one line for the auto-continuation
   *  recap. Conservative — keeps url / hash / first line of cmd. */
  function actionShortSummary(a: ClaudeAction): string {
    if (a.kind === 'commit') return `${a.message}${a.result ? ` → ${a.result.split('\n')[0]}` : ''}`;
    if (a.kind === 'pr') return `${a.title}${a.result?.startsWith('http') ? ` → ${a.result}` : ''}`;
    if (a.kind === 'bash') return `\`${truncInline(a.command, 120)}\``;
    if (a.kind === 'switch_cwd') return a.path;
    return '(unknown)';
  }

  /** Re-enter `runAgentRequest` with a synthesised follow-up prompt.
   *  Doesn't append a user message to the chat — the recap from
   *  `onActionResolved` carries the context, and the Action Card
   *  results already render visibly in the transcript. */
  async function continueAgentTurn(sessionId: string, prompt: string) {
    const sess = sessionsState.list.find((s) => s.id === sessionId);
    if (!sess || sess.sending) {
      // Bail without firing the turn but still release the guard so
      // a future approval batch can continue. Otherwise a stuck
      // continuationInFlight entry would silently swallow the next
      // auto-resume.
      continuationInFlight.delete(sessionId);
      return;
    }
    updateSession(sessionId, { sending: true });
    appendSessionMessage(sessionId, {
      role: 'assistant',
      content: '',
      at: new Date().toISOString()
    });
    startThinkingTimer();
    const runStartedAt = Date.now();
    void scrollChatBottom();

    const cwd = sess.worktreePath || sess.cwd || editorRepoPath || null;
    const claudeUuid = sess.claudeUuid;
    const resume = Boolean(sess.claudeResumable);
    const rules = sessionsState.userRules.trim();
    const agentKind = sess.agentKind;
    const cursorModel = agentKind === 'cursor' ? sess.cursorModel : null;
    const claudeModel = agentKind === 'claude' ? sess.claudeModel : null;
    const claudeToolProfile = agentKind === 'claude' ? sess.claudeToolProfile : null;
    const appContext = buildAgentAppContext(sessionId);

    try {
      const result = await runAgentRequest({
        sessionId,
        prompt,
        cwd,
        claudeUuid,
        resume,
        rules: rules || null,
        agentKind,
        cursorModel,
        claudeModel,
        claudeToolProfile,
        appContext,
        onAssistantDelta: appendAssistantDelta,
        onAppNavigation: handleAppNavigation
      });
      const sessAfter = sessionsState.list.find((s) => s.id === sessionId);
      const lastMsg = sessAfter?.messages[sessAfter.messages.length - 1];
      const streamed = lastMsg?.role === 'assistant' ? lastMsg.content.trim() : '';
      const finalReply = result.reply.trim();
      const uuidStable = !!sessAfter && sessAfter.claudeUuid === claudeUuid;
      const patch: Partial<ClaudeSession> = {};
      if (uuidStable) {
        patch.claudeResumable = true;
        if (result.sessionUuid && result.sessionUuid !== claudeUuid) {
          patch.claudeUuid = result.sessionUuid;
        }
      }
      if (!streamed) {
        replaceLastAssistant(sessionId, finalReply || '(empty response)');
      }
      // One-shot recap consumed — clear so it doesn't re-inject on every
      // subsequent turn. (Same as sendClaudeMessage's success path —
      // missing this caused the recap to live forever after an
      // auto-continuation chain that included a propose_switch_cwd.)
      if (sess.cwdSwitchRecap) {
        patch.cwdSwitchRecap = null;
      }
      // Mark awaitingApproval again if the continuation also added
      // pending action cards — chains of commit → PR are common.
      const sessAfter2 = sessionsState.list.find((s) => s.id === sessionId);
      const stillPending = sessAfter2?.actions.some((a) => a.status === 'pending') ?? false;
      if (stillPending) patch.awaitingApproval = true;
      updateSession(sessionId, patch);
    } catch (e) {
      const msg = typeof e === 'string' ? e : String(e);
      const cancelled = msg.toLowerCase().includes('cancelled');
      const agentLabel = sess.agentKind === 'cursor' ? 'Cursor' : 'Claude';
      if (cancelled) {
        appendSessionMessage(sessionId, {
          role: 'system',
          content: 'Cancelled.',
          at: new Date().toISOString()
        });
      } else {
        replaceLastAssistant(sessionId, `**${agentLabel} failed:** ${msg}`);
        if (appHasFocus()) {
          notifyError(e, { title: `${agentLabel} run failed` });
        } else {
          notifyClaudeRunComplete({
            agentLabel,
            sessionTitle: sess.title || 'Untitled chat',
            ok: false,
            durationMs: Date.now() - runStartedAt
          });
        }
      }
    }
    stopThinkingTimer();
    updateSession(sessionId, { sending: false });
    continuationInFlight.delete(sessionId);
    void scrollChatBottom();
  }

  // ---- Agent execution ----

  async function stopActiveAgent() {
    const s = activeSession;
    if (!s) return;
    try {
      await stopAgentRequest(s.id);
    } catch (e) {
      notifyError(e, { title: 'Stop failed' });
    }
  }


  /** After a mutating action (comment/review/merge/close) on the focused
      item, re-pull detail + inbox + repo-view items. Wraps the store's
      plain reload so the repo-view refresh (cross-cutting, not owned by the
      inbox store) fires alongside. */
  async function reloadDetailAndLists() {
    await reloadDetailAndListsCore();
    // Ask GithubTab to refresh its list if a repo is currently open
    // (merge/close/comment flows need to see the new state reflected there).
    repositoriesView?.refreshItems();
  }

  /** Open a freshly-created PR (or any GitHub PR URL) inside Forgehold's workbench
      by synthesizing a minimal `InboxItem` and letting the `focusItem` effect
      hit the API for the full detail. Called from the action card's "Open in
      Forgehold" button after Claude creates a PR.

      URL shape: `https://github.com/<owner>/<repo>/pull/<number>` (trailing
      path segments like `/files` are ignored). Returns silently if the URL
      doesn't match — the card's raw link remains usable. */
  function openPrUrlInForgehold(url: string, action: (ClaudeAction & { kind: 'pr' }) | null) {
    const m = url.match(/^https:\/\/github\.com\/([^/]+)\/([^/]+)\/pull\/(\d+)/);
    if (!m) return;
    const [, owner, repo, numberStr] = m;
    const number = parseInt(numberStr, 10);
    const iso = new Date().toISOString();
    // We don't know `id` — use `number` (unique within a repo, enough for the
    // focusItem-change effect's key). Every other field is either defaulted or
    // pulled from the action we already have; loadDetail() overwrites with
    // real data on mount.
    const item: InboxItem = {
      id: number,
      number,
      title: action?.title || `#${number}`,
      body: action?.body ?? null,
      state: 'open',
      is_pull_request: true,
      draft: Boolean(action?.draft),
      merged: false,
      url,
      author: null,
      labels: [],
      assignees: [],
      repo: { owner, name: repo },
      comments: 0,
      created_at: iso,
      updated_at: iso
    };
    openFocusItem(item);
    view = 'workbench';
  }

  async function openCommit(c: CommitEntry) {
    if (!inboxState.focusItem?.repo) return;
    openModal('commit', { commit: c, detail: null, loading: true, error: null, expanded: new Set() });
    try {
      const detail = await invoke<CommitDetail>('github_get_commit', {
        owner: inboxState.focusItem.repo.owner,
        repo: inboxState.focusItem.repo.name,
        sha: c.sha
      });
      if (modalsState.commit && modalsState.commit.commit.sha === c.sha) {
        patchModal('commit', { detail, loading: false });
      }
    } catch (e) {
      if (modalsState.commit && modalsState.commit.commit.sha === c.sha) {
        patchModal('commit', { loading: false, error: typeof e === 'string' ? e : String(e) });
      }
    }
  }

  // --- Actions ---

  async function submitComment() {
    if (!commentModal || !inboxState.focusItem?.repo) return;
    const body = commentModal.body;
    if (!body.trim()) return;
    patchModal('comment', { busy: true, error: null });
    try {
      await invoke('github_add_comment', {
        owner: inboxState.focusItem.repo.owner,
        repo: inboxState.focusItem.repo.name,
        number: inboxState.focusItem.number,
        body
      });
      closeModal('comment');
      await reloadDetailAndLists();
    } catch (e) {
      patchModal('comment', { busy: false, error: typeof e === 'string' ? e : String(e) });
    }
  }

  async function submitReview() {
    if (!reviewModal || !inboxState.focusItem?.repo || !inboxState.focusItem.is_pull_request) return;
    const { event, body } = reviewModal;
    patchModal('review', { busy: true, error: null });
    try {
      await invoke('github_submit_review', {
        owner: inboxState.focusItem.repo.owner,
        repo: inboxState.focusItem.repo.name,
        number: inboxState.focusItem.number,
        event,
        body
      });
      closeModal('review');
      await reloadDetailAndLists();
    } catch (e) {
      patchModal('review', { busy: false, error: typeof e === 'string' ? e : String(e) });
    }
  }

  async function submitMerge() {
    if (!mergeModal || !inboxState.focusItem?.repo || !inboxState.focusItem.is_pull_request) return;
    const method = mergeModal.method;
    patchModal('merge', { busy: true, error: null });
    try {
      await invoke('github_merge_pr', {
        owner: inboxState.focusItem.repo.owner,
        repo: inboxState.focusItem.repo.name,
        number: inboxState.focusItem.number,
        method
      });
      closeModal('merge');
      await reloadDetailAndLists();
    } catch (e) {
      patchModal('merge', { busy: false, error: typeof e === 'string' ? e : String(e) });
    }
  }

  async function setState(state: 'closed' | 'open') {
    if (!inboxState.focusItem?.repo) return;
    actionBusy = state;
    try {
      await invoke('github_set_state', {
        owner: inboxState.focusItem.repo.owner,
        repo: inboxState.focusItem.repo.name,
        number: inboxState.focusItem.number,
        state
      });
      // Optimistically update focusItem so the UI flips Close→Reopen right away,
      // even though the inbox (filtered is:open) may drop the item on refresh.
      if (inboxState.focusItem) {
        inboxState.focusItem = { ...inboxState.focusItem, state };
      }
      await reloadDetailAndLists();
    } catch (e) {
      inboxState.detailError = typeof e === 'string' ? e : String(e);
    } finally {
      actionBusy = null;
    }
  }

  function askClose() {
    if (!inboxState.focusItem) return;
    const kind = inboxState.focusItem.is_pull_request ? 'pull request' : 'issue';
    openModal('confirm', {
      title: `Close this ${kind}?`,
      body: `${externalId(inboxState.focusItem)} — ${inboxState.focusItem.title}`,
      confirmText: 'Close',
      danger: true,
      busy: false,
      onConfirm: async () => {
        await setState('closed');
      }
    });
  }

  function openConnectModal(conn: ConnectionMeta) {
    if (!conn.implemented) return;
    if (conn.id === 'github') {
      openModal('pat', { conn, token: '', error: null, busy: false });
    } else if (conn.id === 'jira') {
      openModal('jiraConnect', { workspace: '', email: '', token: '', error: null, busy: false });
    } else if (conn.id === 'sentry') {
      openModal('sentryConnect', {
        host: 'https://sentry.io',
        organization_slug: '',
        token: '',
        error: null,
        busy: false
      });
    } else if (conn.id === 'claude') {
      openModal('claudeStatus', { status: claudeStatus, loading: false });
      void refreshClaudeModal();
    } else if (conn.id === 'cursor') {
      openModal('cursorStatus', { status: cursorStatus, loading: false });
      void refreshCursorModal();
    }
  }

  async function refreshClaudeModal() {
    if (!modalsState.claudeStatus) return;
    patchModal('claudeStatus', { loading: true });
    await refreshClaudeStatus();
    if (modalsState.claudeStatus) patchModal('claudeStatus', { status: claudeStatus, loading: false });
  }

  async function refreshCursorModal() {
    if (!modalsState.cursorStatus) return;
    patchModal('cursorStatus', { loading: true });
    // refreshClaudeStatus() actually refreshes BOTH agents (cursor + claude) —
    // see `agent_status` Tauri command. Reuse so we don't double-poll.
    await refreshClaudeStatus();
    if (modalsState.cursorStatus) patchModal('cursorStatus', { status: cursorStatus, loading: false });
  }

  function cursorInstallUrl() {
    return 'https://cursor.com/docs/cli/installation';
  }

  async function submitJira() {
    if (!jiraModal) return;
    const { workspace, email, token } = jiraModal;
    patchModal('jiraConnect', { busy: true, error: null });
    try {
      await invoke<JiraUser>('jira_connect', { workspace, email, token });
      markTokenInstalled('jira');
      closeModal('jiraConnect');
      await refreshJiraStatus();
    } catch (e) {
      patchModal('jiraConnect', { busy: false, error: typeof e === 'string' ? e : String(e) });
    }
  }

  async function disconnectJira() {
    await invoke('jira_disconnect');
    clearTokenInstalled('jira');
    await refreshJiraStatus();
  }

  function jiraTokenUrl() {
    return 'https://id.atlassian.com/manage-profile/security/api-tokens';
  }

  function sentryTokenUrl(): string {
    const host = modalsState.sentryConnect?.host?.trim() || 'https://sentry.io';
    return `${host.replace(/\/+$/, '')}/settings/account/api/auth-tokens/`;
  }

  async function submitSentry() {
    if (!modalsState.sentryConnect) return;
    const { host, organization_slug, token } = modalsState.sentryConnect;
    patchModal('sentryConnect', { busy: true, error: null });
    try {
      await invoke<SentryUser>('sentry_connect', {
        host,
        organizationSlug: organization_slug,
        token
      });
      markTokenInstalled('sentry');
      closeModal('sentryConnect');
      await refreshSentryStatus();
      void refreshAllSentryInboxes();
    } catch (e) {
      patchModal('sentryConnect', { busy: false, error: typeof e === 'string' ? e : String(e) });
    }
  }

  async function disconnectSentryAll() {
    try {
      await invoke('sentry_disconnect');
      clearTokenInstalled('sentry');
      await refreshSentryStatus();
      resetSentryInbox();
      notify({ kind: 'success', title: 'Disconnected from Sentry' });
    } catch (e) {
      notifyError(e, { title: 'Sentry disconnect failed' });
    }
  }

  function claudeInstallUrl() {
    return 'https://docs.claude.com/en/docs/claude-code/overview';
  }

  async function submitPat() {
    if (!patModal) return;
    const token = patModal.token;
    patchModal('pat', { busy: true, error: null });
    try {
      const user = await invoke<GithubUser>('github_connect_pat', { token });
      connectionsState.github = { kind: 'connected', user };
      markTokenInstalled('github');
      closeModal('pat');
      await refreshAllInboxes();
      view = 'workbench';
    } catch (e) {
      patchModal('pat', { busy: false, error: typeof e === 'string' ? e : String(e) });
    }
  }

  async function disconnectGithub() {
    try {
      await invoke('github_disconnect');
      clearTokenInstalled('github');
      await refreshGithubStatus();
      resetGithubInbox();
      notify({ kind: 'success', title: 'Disconnected from GitHub' });
    } catch (e) {
      notifyError(e, { title: 'GitHub disconnect failed' });
    }
    // Repo state is owned by GithubTab — it wipes itself via its
    // `$effect` on `connectedGithub` becoming false.
  }

  async function disconnectJiraAll() {
    try {
      await invoke('jira_disconnect');
      clearTokenInstalled('jira');
      await refreshJiraStatus();
      resetJiraInbox();
      notify({ kind: 'success', title: 'Disconnected from Jira' });
    } catch (e) {
      notifyError(e, { title: 'Jira disconnect failed' });
      return;
    }
  }

  async function openBrowser(url: string) {
    try { await openUrl(url); } catch (e) { notifyError(e, { title: 'Could not open browser' }); }
  }

  function onKey(e: KeyboardEvent) {
    if ((e.metaKey || e.ctrlKey) && e.key === 'k') {
      e.preventDefault();
      paletteOpen = !paletteOpen;
    } else if (e.key === 'Escape') {
      /* Cheatsheet wins on its own Escape — keep the existing modal
         cascade for everything else. */
      if (cheatsheetOpen) {
        cheatsheetOpen = false;
        return;
      }
      paletteOpen = false;
      if (patModal && !patModal.busy) closeModal('pat');
      if (jiraModal && !jiraModal.busy) closeModal('jiraConnect');
      if (claudeModal && !claudeModal.loading) closeModal('claudeStatus');
      if (modalsState.cursorStatus && !modalsState.cursorStatus.loading) closeModal('cursorStatus');
      if (modalsState.userPicker) closeModal('userPicker');
      if (commentModal && !commentModal.busy) closeModal('comment');
      if (reviewModal && !reviewModal.busy) closeModal('review');
      if (mergeModal && !mergeModal.busy) closeModal('merge');
      if (commitModal) closeModal('commit');
      if (confirmModal && !confirmModal.busy) closeModal('confirm');
      if (jiraCreateModal && !jiraCreateModal.busy) closeModal('jiraCreate');
      if (githubCreatePrModal && !githubCreatePrModal.busy) closeModal('githubCreatePr');
      if (inboxState.focusItem) closeFocusItem();
      if (inboxState.jiraFocusKey) inboxState.jiraFocusKey = null;
      if (inboxState.sentryFocusId) inboxState.sentryFocusId = null;
      /* Restore the maximized column if no other ESC-target was active.
         Defer one frame so single-press ESC closes the topmost overlay
         first (modals / focus panes), and only the *second* ESC drops
         the maximize. */
      if (
        layoutState.maximizedInstanceId !== null &&
        !anyModalOpen() &&
        !inboxState.focusItem &&
        !inboxState.jiraFocusKey &&
        !inboxState.sentryFocusId
      ) {
        restoreMaximized();
      }
    } else if (e.key === '?' && !isTextInput(e.target) && !anyModalOpen()) {
      /* `?` toggles the cheatsheet. Skip when an input/textarea has
         focus so it doesn't hijack a literal `?` the user is typing. */
      e.preventDefault();
      cheatsheetOpen = !cheatsheetOpen;
    } else if ((e.metaKey || e.ctrlKey) && e.shiftKey && (e.key === 'm' || e.key === 'M')) {
      /* ⌘⇧M — toggle maximize. If a column is already maximized
         (any column), this un-maximizes it. Otherwise we pick the
         first instance in the active workbench as the maximize
         target — there's no "focused column" concept in inboxState
         today, so going by panel order is the least-surprising
         fallback. Power users tend to hit the keyboard from a
         specific column anyway and can re-press to cycle. */
      e.preventDefault();
      const target =
        layoutState.maximizedInstanceId
          ?? activeInstances()[0]?.id
          ?? null;
      if (target) toggleMaximize(target);
    } else if ((e.metaKey || e.ctrlKey) && !e.shiftKey && !e.altKey && /^[1-9]$/.test(e.key)) {
      /* ⌘1..⌘9 — jump to the Nth workbench tab. Silently no-op when
         the index is out of range so an idle ⌘5 in a 2-workbench
         setup doesn't surprise the user. */
      const idx = Number(e.key) - 1;
      const target = layoutState.workbenches[idx];
      if (target) {
        e.preventDefault();
        setActiveWorkbench(target.id);
      }
    } else if (e.key === 'j' && view === 'workbench' && !anyModalOpen()) {
      moveSelection(1);
    } else if (e.key === 'k' && view === 'workbench' && !(e.metaKey || e.ctrlKey) && !anyModalOpen()) {
      moveSelection(-1);
    } else if (e.key === 'o' && !isTextInput(e.target) && !anyModalOpen() && !(e.metaKey || e.ctrlKey)) {
      /* Open the focused inbox row in the system browser
       * (M4 §2.3.6 — same shape as GitHub's `gh pr view --web`).
       * GitHub's focused PR/issue uses `focusItem.url`; Jira and
       * Sentry resolve the URL from the open detail object since
       * `jiraFocusKey` / `sentryFocusId` are bare ids. Silently
       * no-ops when nothing is focused — keyboard ergonomics, not
       * a destructive action. */
      const targetUrl = focusedRowUrl();
      if (targetUrl) {
        e.preventDefault();
        void openUrl(targetUrl);
      }
    }
  }

  function focusedRowUrl(): string | null {
    if (inboxState.focusItem?.url) return inboxState.focusItem.url;
    if (inboxState.jiraFocusKey) {
      /* Look up the focused Jira issue across both the tab slice and
       * every column slice — JiraItem carries the upstream `url`
       * straight from `/rest/api/3/issue/{key}`, so we don't need
       * to reconstruct it from the workspace + key. */
      for (const list of Object.values(inboxState.jiraItemsByInstance)) {
        const hit = list.find((it) => it.key === inboxState.jiraFocusKey);
        if (hit) return hit.url;
      }
      const hitTab = inboxState.jiraTabItems.find((it) => it.key === inboxState.jiraFocusKey);
      if (hitTab) return hitTab.url;
    }
    if (inboxState.sentryFocusId) {
      for (const list of Object.values(inboxState.sentryItemsByInstance)) {
        const hit = list.find((it) => it.id === inboxState.sentryFocusId);
        if (hit?.permalink) return hit.permalink;
      }
      const hitTab = inboxState.sentryTabItems.find((it) => it.id === inboxState.sentryFocusId);
      if (hitTab?.permalink) return hitTab.permalink;
    }
    return null;
  }

  /** Heuristic: is the keyboard event aimed at a text-entry surface
   *  (input / textarea / contenteditable)? Used to keep `?` from
   *  swallowing a literal question mark the user is typing. */
  function isTextInput(target: EventTarget | null): boolean {
    if (!(target instanceof HTMLElement)) return false;
    const tag = target.tagName;
    if (tag === 'INPUT' || tag === 'TEXTAREA') return true;
    if (target.isContentEditable) return true;
    return false;
  }

  function anyModalOpen() {
    return !!(
      patModal ||
      jiraModal ||
      claudeModal ||
      modalsState.userPicker ||
      commentModal ||
      reviewModal ||
      mergeModal ||
      commitModal ||
      confirmModal ||
      jiraCreateModal ||
      githubCreatePrModal ||
      inboxState.focusItem ||
      inboxState.jiraFocusKey ||
      inboxState.sentryFocusId ||
      paletteOpen ||
      cheatsheetOpen
    );
  }

  function githubTokenUrl() {
    const scopes = ['repo', 'read:user', 'read:org'].join(',');
    return `https://github.com/settings/tokens/new?scopes=${scopes}&description=Forgehold%20Desktop`;
  }

  function mergeDisabled(): boolean {
    if (!inboxState.focusItem?.is_pull_request) return true;
    if (!inboxState.prDetail) return true;
    if (inboxState.prDetail.merged) return true;
    if (inboxState.prDetail.state !== 'open') return true;
    if (inboxState.prDetail.draft) return true;
    return inboxState.prDetail.mergeable === false;
  }

  // ---- Jira Create Issue ----

  async function openJiraCreateIssue() {
    /* Pull defaults from the FIRST jira column's filter — no perfect
       answer with multiple columns, but most setups have one and the
       user expects the new-issue dialog to pre-fill from "the" column. */
    const firstId = Object.keys(inboxState.jiraFiltersByInstance)[0];
    const active = firstId
      ? inboxState.jiraFiltersByInstance[firstId]
      : { projectKey: null, sprintIds: [] as (number | 'backlog')[] };
    openModal('jiraCreate', {
      projectKey: active.projectKey ?? '',
      projects: inboxState.jiraProjectOptions,
      projectsLoading: false,
      issueTypes: [],
      issueTypeName: 'Task',
      summary: '',
      description: '',
      assigneeAccountId: '',
      assignees: [],
      assigneesLoading: false,
      sprints: inboxState.jiraSprintOptions,
      // Default the new-issue sprint to the first numeric sprint scope
      // selected in the filter (multi-select: a created issue can only
      // live in one sprint, so pick the first; null falls through if
      // the user only had backlog selected or nothing).
      sprintId: active.sprintIds.find((s): s is number => typeof s === 'number') ?? null,
      busy: false,
      error: null
    });
    // Always refresh projects list (lazy — skips if already cached).
    if (!inboxState.jiraProjectOptions.length) {
      patchModal('jiraCreate', { projectsLoading: true });
      try {
        const projects = await invoke<JiraProject[]>('jira_list_projects');
        inboxState.jiraProjectOptions = projects;
        patchModal('jiraCreate', { projects, projectsLoading: false });
      } catch {
        patchModal('jiraCreate', { projectsLoading: false });
      }
    }
    // If a project is pre-selected, pull its issue types immediately.
    if (modalsState.jiraCreate?.projectKey) {
      void onJiraCreateProjectChange(modalsState.jiraCreate.projectKey);
    }
  }

  async function onJiraCreateProjectChange(key: string) {
    if (!modalsState.jiraCreate) return;
    // Project change wipes assignee — accountId is project-scoped (a user
    // assignable in PROJECTA may not exist as an option in PROJECTB), so
    // resetting avoids carrying a stale id forward.
    patchModal('jiraCreate', {
      projectKey: key,
      issueTypes: [],
      assignees: [],
      assigneeAccountId: ''
    });
    if (!key) return;
    // Issue types + assignable users in parallel — both keyed off the
    // project. Failures are swallowed because the modal still works with
    // the hardcoded fallback (Task/Bug/Story) and an unassigned issue.
    void (async () => {
      try {
        const types = await invoke<JiraIssueType[]>('jira_list_issue_types', { projectKey: key });
        const m = modalsState.jiraCreate;
        if (!m) return;
        const preserved = types.find((t) => t.name === m.issueTypeName);
        const nextName = preserved ? preserved.name : types[0]?.name ?? 'Task';
        patchModal('jiraCreate', { issueTypes: types, issueTypeName: nextName });
      } catch {/* fallback to hardcoded list */}
    })();
    void (async () => {
      patchModal('jiraCreate', { assigneesLoading: true });
      try {
        const users = await invoke<JiraUserSummary[]>('jira_list_assignable_users', { projectKey: key });
        // Stable A→Z sort by displayName so the dropdown is scannable.
        users.sort((a, b) => a.display_name.localeCompare(b.display_name));
        patchModal('jiraCreate', { assignees: users, assigneesLoading: false });
      } catch {
        patchModal('jiraCreate', { assigneesLoading: false });
      }
    })();
  }

  async function submitJiraCreate() {
    if (!jiraCreateModal) return;
    const { projectKey, summary, description, issueTypeName, assigneeAccountId, sprintId } = jiraCreateModal;
    if (!projectKey.trim() || !summary.trim() || !issueTypeName.trim()) return;
    patchModal('jiraCreate', { busy: true, error: null });
    try {
      const created = await invoke<JiraItem>('jira_create_issue', {
        projectKey: projectKey.trim(),
        issueType: issueTypeName,
        summary: summary.trim(),
        description,
        assigneeAccountId: assigneeAccountId.trim() || null,
        sprintId
      });
      // Optimistically push the new issue onto every jira column's list,
      // then refresh to pick up server-side ordering. Each column will
      // re-fetch with its own filter — the optimistic prepend just hides
      // the round-trip latency.
      for (const id of Object.keys(inboxState.jiraItemsByInstance)) {
        inboxState.jiraItemsByInstance[id] = [
          created,
          ...inboxState.jiraItemsByInstance[id]
        ];
      }
      closeModal('jiraCreate');
      void refreshAllJiraInboxes({ silent: true });
    } catch (e) {
      patchModal('jiraCreate', { busy: false, error: typeof e === 'string' ? e : String(e) });
    }
  }

  // ---- GitHub Create PR ----

  async function openGithubCreatePr() {
    /* Pull repo default from the first GitHub column's filter, same
       trade-off as openJiraCreateIssue. */
    const firstId = Object.keys(inboxState.githubFiltersByInstance)[0];
    const activeRepo = firstId
      ? inboxState.githubFiltersByInstance[firstId].repo
      : null;
    openModal('githubCreatePr', {
      repo: activeRepo ?? '',
      repos: inboxState.githubRepoOptions.map((r) => ({
        owner: r.owner,
        name: r.name,
        full_name: r.full_name,
        default_branch: null
      })),
      reposLoading: false,
      branches: [],
      branchesLoading: false,
      base: '',
      head: '',
      title: '',
      body: '',
      draft: false,
      compare: null,
      filesExpanded: false,
      busy: false,
      error: null
    });
    if (!inboxState.githubRepoOptions.length) {
      patchModal('githubCreatePr', { reposLoading: true });
      try {
        const repos = await invoke<Repository[]>('github_list_repos');
        inboxState.githubRepoOptions = repos.map((r) => ({
          owner: r.owner,
          name: r.name,
          full_name: r.full_name
        }));
        patchModal('githubCreatePr', {
          repos: repos.map((r) => ({
            owner: r.owner,
            name: r.name,
            full_name: r.full_name,
            default_branch: r.default_branch
          })),
          reposLoading: false
        });
      } catch {
        patchModal('githubCreatePr', { reposLoading: false });
      }
    }
    if (modalsState.githubCreatePr?.repo) {
      void onGithubPrRepoChange(modalsState.githubCreatePr.repo);
    }
  }

  async function onGithubPrRepoChange(full: string) {
    if (!modalsState.githubCreatePr) return;
    patchModal('githubCreatePr', {
      repo: full,
      branches: [],
      base: '',
      head: '',
      compare: null,
      branchesLoading: !!full
    });
    if (!full) return;
    const [owner, name] = full.split('/');
    if (!owner || !name) return;
    try {
      const branches = await invoke<RepoBranch[]>('github_list_repo_branches', { owner, repo: name });
      // Look up the default branch — either from the cached repo list, or via
      // github_list_repos if we don't have it yet.
      let defaultBranch =
        modalsState.githubCreatePr?.repos.find((r) => r.full_name === full)?.default_branch ?? null;
      if (!defaultBranch) {
        try {
          const repos = await invoke<Repository[]>('github_list_repos');
          defaultBranch = repos.find((r) => r.full_name === full)?.default_branch ?? null;
        } catch { /* ignore */ }
      }
      patchModal('githubCreatePr', {
        branches,
        branchesLoading: false,
        base: defaultBranch ?? branches[0]?.name ?? ''
      });
    } catch (e) {
      patchModal('githubCreatePr', {
        branchesLoading: false,
        error: typeof e === 'string' ? e : String(e)
      });
    }
  }

  async function onGithubPrBranchesChange() {
    const m = modalsState.githubCreatePr;
    if (!m) return;
    // Autofill title from head branch name — Title Case sans separators —
    // only if the user hasn't typed a custom title yet.
    if (m.head && !m.title.trim()) {
      const pretty = m.head
        .replace(/^[a-zA-Z]+\//, '')
        .replace(/[-_/]+/g, ' ')
        .trim()
        .split(' ')
        .filter(Boolean)
        .map((w) => w.charAt(0).toUpperCase() + w.slice(1))
        .join(' ');
      if (pretty) patchModal('githubCreatePr', { title: pretty });
    }
    if (!m.repo || !m.base || !m.head || m.base === m.head) {
      if (m.compare) patchModal('githubCreatePr', { compare: null });
      return;
    }
    const [owner, name] = m.repo.split('/');
    if (!owner || !name) return;
    patchModal('githubCreatePr', {
      compare: {
        loading: true,
        error: null,
        total_commits: 0,
        ahead_by: 0,
        behind_by: 0,
        additions: 0,
        deletions: 0,
        commits: [],
        files: []
      }
    });
    try {
      const result = await invoke<CompareResult>('github_compare', {
        owner,
        repo: name,
        base: m.base,
        head: m.head
      });
      patchModal('githubCreatePr', { compare: { loading: false, error: null, ...result } });
    } catch (e) {
      patchModal('githubCreatePr', {
        compare: {
          loading: false,
          error: typeof e === 'string' ? e : String(e),
          total_commits: 0,
          ahead_by: 0,
          behind_by: 0,
          additions: 0,
          deletions: 0,
          commits: [],
          files: []
        }
      });
    }
  }

  async function submitGithubPr() {
    if (!githubCreatePrModal) return;
    const { repo, base, head, title, body, draft } = githubCreatePrModal;
    if (!repo || !base || !head || base === head || !title.trim()) return;
    const [owner, name] = repo.split('/');
    if (!owner || !name) return;
    patchModal('githubCreatePr', { busy: true, error: null });
    try {
      const created = await invoke<InboxItem>('github_create_pr', {
        owner,
        repo: name,
        title: title.trim(),
        body,
        base,
        head,
        draft
      });
      closeModal('githubCreatePr');
      // Optimistically push onto every github column and open focus pane.
      for (const id of Object.keys(inboxState.itemsByInstance)) {
        inboxState.itemsByInstance[id] = [created, ...inboxState.itemsByInstance[id]];
      }
      openFocusItem(created);
      view = 'workbench';
      void refreshAllInboxes({ silent: true });
    } catch (e) {
      patchModal('githubCreatePr', { busy: false, error: typeof e === 'string' ? e : String(e) });
    }
  }
</script>

<svelte:window onkeydown={onKey} />

<Cheatsheet open={cheatsheetOpen} onClose={() => (cheatsheetOpen = false)} />

<!-- First-launch welcome flow. Renders only when (a) the user has
     unlocked through the biometric gate (no point showing it under
     the lock screen) and (b) they haven't already completed it.
     Connecting a source from the welcome step opens the regular
     connect modal so the auth UX stays in one place. -->
{#if !appLocked && !welcomeState.completed}
  <Welcome
    sources={[...sourceConns, ...agentConns]}
    onConnect={(id) => {
      const conn = [...sourceConns, ...agentConns].find((c) => c.id === id);
      if (conn) openConnectModal(conn);
    }}
    onClose={() => { /* welcome state handles persistence */ }}
  />
{/if}

<div class="bg"></div>

{#if appLocked}
  <div class="lock-screen" role="dialog" aria-modal="true">
    <div class="lock-card">
      <Sigil size={72} />
      <h1 class="lock-title">Forgehold is locked</h1>
      <p class="lock-sub">
        Authenticate with Touch ID (or your Mac passcode) to unlock your stored
        credentials for Jira, GitHub, and Claude.
      </p>
      {#if biometryError}
        <div class="lock-err">{biometryError}</div>
      {/if}
      <button class="btn btn--primary" onclick={biometricUnlock} disabled={biometryInFlight}>
        {#if biometryInFlight}
          <span class="dot-pulse"></span><span class="dot-pulse"></span><span class="dot-pulse"></span>
        {:else}
          <svg class="i i-sm" viewBox="0 0 24 24"><path d="M12 11c-2 0-3.5 1.5-3.5 4v2c0 1.5.5 3 2 4M12 11c2 0 3.5 1.5 3.5 4v2c0 1.5-.5 3-2 4M12 11V3M12 3a4 4 0 0 0-4 4v4M12 3a4 4 0 0 1 4 4v4"/></svg>
          Unlock with Touch ID
        {/if}
      </button>
    </div>
  </div>
{/if}

<div id="app" class:is-dragging={dragState.payload !== null}>
  <Rail
    bind:view
    inboxCount={Object.values(inboxState.itemsByInstance).reduce((sum, list) => sum + list.length, 0)}
    {anythingConnected}
    {statusLoading}
    {anyRetrying}
    {githubStatus}
    {jiraStatus}
    {sentryStatus}
    {claudeStatus}
    {cursorStatus}
  />

  <div class="main">
    {#if view === 'workbench'}
      {#if !anythingConnected && !statusLoading}
        <section class="full-center">
          <div class="empty">
            <Sigil size={56} />
            <h2 class="empty-title">Connect a source</h2>
            <p class="empty-sub">Pick GitHub, Jira, or Claude Code — each lives in its own column. Drag any card onto the Claude column to hand it to the agent.</p>
            <button class="btn btn--primary" onclick={() => (view = 'connections')}>Set up connections</button>
          </div>
        </section>
      {:else}
        {@const instances = activeInstances()}
        {@const hasGithub = !!instances.find((i) => i.kind === 'github')}
        {@const hasJira = !!instances.find((i) => i.kind === 'jira')}
        {@const hasClaude = !!instances.find((i) => i.kind === 'claude')}
        {@const hasCursor = !!instances.find((i) => i.kind === 'cursor')}
        {@const hasEditor = !!instances.find((i) => i.kind === 'editor')}
        <div class="wb-tabs">
          {#each layoutState.workbenches as wb (wb.id)}
            <div
              class="wb-tab"
              class:active={wb.id === layoutState.activeWorkbenchId}
              class:drag-over={tabDragOverId === wb.id}
              role="button"
              tabindex="0"
              ondblclick={() => startWorkbenchRename(wb.id, wb.name)}
              onclick={() => setActiveWorkbench(wb.id)}
              onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); setActiveWorkbench(wb.id); } }}
              ondragenter={(e) => onTabDragEnter(e, wb.id)}
              ondragover={(e) => onTabDragOver(e, wb.id)}
              ondragleave={() => onTabDragLeave(wb.id)}
              ondrop={(e) => onTabDrop(e, wb.id)}
              title="Click to switch · double-click to rename · drop a column here to move it"
            >
              {#if editingWorkbench && editingWorkbench.id === wb.id}
                <input
                  class="wb-tab-rename"
                  value={editingWorkbench.draft}
                  oninput={(e) => { if (editingWorkbench) editingWorkbench = { ...editingWorkbench, draft: (e.currentTarget as HTMLInputElement).value }; }}
                  onblur={commitWorkbenchRename}
                  onkeydown={(e) => {
                    if (e.key === 'Enter') { e.preventDefault(); commitWorkbenchRename(); }
                    if (e.key === 'Escape') { e.preventDefault(); editingWorkbench = null; }
                  }}
                  {@attach (node: HTMLInputElement) => { node.focus(); node.select(); }}
                />
              {:else}
                <span class="wb-tab-name">{wb.name}</span>
                {#if wb.instances.length > 0}<span class="wb-tab-count mono">{wb.instances.length}</span>{/if}
                {#if layoutState.workbenches.length > 1 && wb.id !== layoutState.activeWorkbenchId}
                  <span
                    class="wb-tab-close"
                    role="button"
                    tabindex="0"
                    aria-label="Delete workbench"
                    onclick={(e) => { e.stopPropagation(); askRemoveWorkbench(wb.id); }}
                    onkeydown={(e) => { if (e.key === 'Enter') { e.stopPropagation(); askRemoveWorkbench(wb.id); } }}
                  >
                    <svg class="i i-sm" viewBox="0 0 24 24"><path d="M18 6 6 18M6 6l12 12"/></svg>
                  </span>
                {/if}
              {/if}
            </div>
          {/each}
          <button class="wb-tab-add" onclick={() => { const id = addWorkbench('Workbench ' + (layoutState.workbenches.length + 1)); setActiveWorkbench(id); startWorkbenchRename(id, 'Workbench ' + layoutState.workbenches.length); }} title="New workbench">
            <svg class="i i-sm" viewBox="0 0 24 24"><path d="M12 5v14M5 12h14"/></svg>
            <span>New workbench</span>
          </button>
        </div>
        <div class="wb-bar">
          {#snippet pill(kind: PanelKind, label: string, meta: typeof connectionsMeta[number] | undefined)}
            {@const insts = listInstancesOfKind(kind)}
            {@const archived = listArchivedOfKind(kind)}
            {@const count = insts.length}
            {@const inCurrent = insts.some((i) => i.workbenchId === layoutState.activeWorkbenchId)}
            <div
              class="pill-group"
              class:active={inCurrent}
              class:dim={count === 0 && archived.length === 0}
              class:has-menu={count > 0 || archived.length > 0}
              class:drag-over={pillDragOverKind === kind && pillDragOverInstance === null}
              class:drag-armed={pillDragOverKind === kind}
              ondragenter={(e) => onPillDragEnter(e, kind, null)}
              ondragover={(e) => onPillDragOver(e, kind, null)}
              ondragleave={() => onPillDragLeave(kind, null)}
              ondrop={(e) => onPillDrop(e, kind, null)}
              role="presentation"
            >
              <button
                class="pill"
                onclick={() => navToKind(kind)}
                disabled={count === 0}
                title={count === 0 ? `No ${label} columns yet — click + to create` : `Jump to ${label}`}
              >
                {#if meta?.iconImg}
                  <!-- Raster brand mark (Claude / Cursor) — true-colour
                       PNG instead of single-tone SVG so the official
                       gradient / palette comes through. -->
                  <span class="pill-icon pill-icon--img"><img src={meta.iconImg} alt="" class="pill-icon-img" /></span>
                {:else if meta?.iconSvg}
                  <span class="pill-icon {meta.iconClass}"><svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">{@html meta.iconSvg}</svg></span>
                {:else if kind === 'editor'}
                  <!-- Folder icon — matches the EditorView header so the
                       Editor's identity is the same in pill / column / tab. -->
                  <span class="pill-icon pill-icon--editor"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M3 7v11a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2V9a2 2 0 0 0-2-2h-7L10 5H5a2 2 0 0 0-2 2z"/></svg></span>
                {:else if kind === 'canvas'}
                  <!-- Stacked-frames icon — same glyph as the column's
                       brand mark so the Canvas identity is consistent
                       across pill / header / library tile. -->
                  <span class="pill-icon pill-icon--canvas"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="18" height="14" rx="2"/><rect x="6" y="6" width="9" height="6" rx="1"/><rect x="13" y="13" width="5" height="3" rx="0.5"/></svg></span>
                {:else if kind === 'terminal'}
                  <!-- Chevron-prompt icon — same as TerminalColumn brand
                       so the Terminal identity reads the same in pill +
                       column header. -->
                  <span class="pill-icon pill-icon--terminal"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><polyline points="4 17 10 11 4 5"/><line x1="12" y1="19" x2="20" y2="19"/></svg></span>
                {/if}
                <span class="pill-label">{label}</span>
                {#if count > 0}
                  <span class="pill-count mono">{count}</span>
                {/if}
                {#if archived.length > 0}
                  <span
                    class="pill-count pill-count--archived mono"
                    title={`${archived.length} archived — hover the menu to restore`}
                  >
                    {archived.length}
                  </span>
                {/if}
              </button>
              <button
                class="pill-add"
                onclick={() => spawnColumnInstance(kind)}
                title={`New ${label} column`}
                aria-label={`New ${label} column`}
              >
                <svg viewBox="0 0 24 24"><path d="M12 5v14M5 12h14"/></svg>
              </button>
              {#if count > 0 || archived.length > 0}
                <div class="pill-menu" role="menu">
                  <div class="pill-menu-head">{label} · {count} {count === 1 ? 'column' : 'columns'}</div>
                  {#each insts as inst (inst.id)}
                    {@const isCurrent = inst.workbenchId === layoutState.activeWorkbenchId}
                    <button
                      class="pill-menu-item"
                      class:is-current={isCurrent}
                      class:drag-over={pillDragOverKind === kind && pillDragOverInstance === inst.id}
                      role="menuitem"
                      ondragenter={(e) => onPillDragEnter(e, kind, inst.id)}
                      ondragover={(e) => onPillDragOver(e, kind, inst.id)}
                      ondragleave={() => onPillDragLeave(kind, inst.id)}
                      ondrop={(e) => onPillDrop(e, kind, inst.id)}
                      onclick={() => void goToInstance(inst.id, inst.workbenchId)}
                    >
                      <span class="pill-menu-dot" class:is-active={isCurrent}></span>
                      <span class="pill-menu-name mono">{inst.name}</span>
                      <span class="pill-menu-wb mono" title="Workbench">{inst.workbenchName}</span>
                    </button>
                  {/each}
                  {#if archived.length > 0}
                    <div class="pill-menu-head pill-menu-head--archive">Archived · {archived.length}</div>
                    {#each archived as a (a.id)}
                      <div class="pill-menu-item pill-menu-item--archived" role="presentation">
                        <span class="pill-menu-dot"></span>
                        <span class="pill-menu-name mono">{a.name}</span>
                        <span class="pill-menu-wb mono" title="Originally on this workbench">{a.originalWorkbenchName}</span>
                        <button
                          class="pill-menu-restore"
                          title="Restore — back to original workbench, or current if it was deleted"
                          aria-label="Restore"
                          onclick={(e) => {
                            e.stopPropagation();
                            unarchiveInstance(a.id, layoutState.activeWorkbenchId);
                          }}
                        >
                          <svg class="i i-sm" viewBox="0 0 24 24"><path d="M3 12a9 9 0 1 0 3-6.7M3 4v5h5"/></svg>
                        </button>
                      </div>
                    {/each}
                  {/if}
                </div>
              {/if}
            </div>
          {/snippet}

          {#if connectedGithub}
            {@render pill('github', 'GitHub', connectionsMeta.find((c) => c.id === 'github'))}
          {/if}
          {#if connectedJira}
            {@render pill('jira', 'Jira', connectionsMeta.find((c) => c.id === 'jira'))}
          {/if}
          {#if connectedSentry}
            {@render pill('sentry', 'Sentry', connectionsMeta.find((c) => c.id === 'sentry'))}
          {/if}
          {#if connectedClaude}
            {@render pill('claude', 'Claude', connectionsMeta.find((c) => c.id === 'claude'))}
          {/if}
          {#if connectedCursor}
            {@render pill('cursor', 'Cursor', connectionsMeta.find((c) => c.id === 'cursor'))}
          {/if}
          {@render pill('editor', 'Editor', undefined)}
          {@render pill('canvas', 'Canvas', undefined)}
          {@render pill('terminal', 'Terminal', undefined)}

          <div style="flex:1"></div>
          <button class="icon-btn" title="Search" aria-label="Search" onclick={() => (paletteOpen = true)}>
            <svg class="i i-sm" viewBox="0 0 24 24"><circle cx="11" cy="11" r="7" /><path d="m20 20-3-3" /></svg>
          </button>
        </div>
        <div class="wb-columns">
          {#if instances.length === 0}
            <!-- Empty workbench hint (M4 §2.6.8). Shown when the
                 active workbench has zero columns but at least one
                 source is connected — a fresh "Workbench 2" tab,
                 say. The pill bar is the canonical place to add
                 columns; this is a discoverability nudge, not a
                 button itself. -->
            <div class="wb-empty-hint" role="note">
              <div class="wb-empty-hint-title">This workbench is empty</div>
              <div class="wb-empty-hint-sub">
                Drag a chip from the bar above to add a column, or press <span class="mono">⌘K</span> and search "new column".
              </div>
            </div>
          {/if}
          {#each instances as inst (inst.id)}
            {#if inst.kind === 'github' && connectedGithub}
              <GithubColumn
                instanceId={inst.id}
                {githubStatus}
                {now}
                {tab}
                {actionBusy}
                onSelectInboxItem={selectInboxItem}
                onRefreshInbox={() => refreshInbox(inst.id)}
                onOpenPalette={() => (paletteOpen = true)}
                {onDragStart}
                {onDragEnd}
                {onCardMouseDown}
                {isClickNotDrag}
                onTabChange={(t) => (tab = t)}
                onToggleFile={toggleFile}
                onRetryLoadDetail={() => loadDetail()}
                onOpenCommit={openCommit}
                onOpenComment={() => openModal('comment', { body: '', busy: false, error: null })}
                onOpenReview={() => openModal('review', { event: 'APPROVE', body: '', busy: false, error: null })}
                onOpenMerge={() => openModal('merge', { method: 'squash', busy: false, error: null })}
                onAskClose={askClose}
                onReopen={() => setState('open')}
                onOpenBrowser={openBrowser}
                onOpenCheckDetails={(url) => void openUrl(url)}
                onCloseFocus={closeFocusItem}
                {mergeDisabled}
                onOpenCreatePr={openGithubCreatePr}
              />
            {:else if inst.kind === 'jira' && connectedJira}
              <JiraColumn
                instanceId={inst.id}
                {jiraStatus}
                {now}
                onOpenUserPicker={openUserPicker}
                onRefreshJiraInbox={() => refreshJiraInbox(inst.id)}
                {onDragStart}
                {onDragEnd}
                {onCardMouseDown}
                {isClickNotDrag}
                onOpenBrowser={openBrowser}
                onOpenCreateIssue={openJiraCreateIssue}
              />
            {:else if inst.kind === 'sentry' && connectedSentry}
              <SentryColumn
                instanceId={inst.id}
                {sentryStatus}
                {now}
                {onDragStart}
                {onDragEnd}
                {onCardMouseDown}
                {isClickNotDrag}
                onOpenBrowser={openBrowser}
              />
            {:else if inst.kind === 'claude' && connectedClaude}
              <AgentColumn
                kind="claude"
                instanceId={inst.id}
                {claudeStatus}
                {cursorStatus}
                {githubStatus}
                {editorRepoPath}
                {activeRepoInfo}
                {dragOverInstanceId}
                {worktreeBusy}
                {worktreeMenuOpen}
                {editingMsg}
                {thinkingStartedAt}
                {thinkingTick}
                {now}
                {onAgentDragEnter}
                {onAgentDragOver}
                {onAgentDragLeave}
                {onAgentDrop}
                onPickCwd={pickCwd}
                onClearCwd={clearCwd}
                onOpenSessionFolderInEditor={openSessionFolderInEditor}
                onToggleEditorLink={toggleSessionEditorLink}
                onLinkToEditorInstance={linkActiveSessionToEditor}
                onCreateWorktree={createWorktree}
                onToggleWorktreeMenu={toggleWorktreeMenu}
                onOpenWorktreeDiff={openWorktreeDiff}
                onOpenWorktreeInEditor={openWorktreeInEditor}
                onCopyWorktreeBranch={copyWorktreeBranch}
                onApplyWorktree={applyWorktree}
                onRemoveWorktree={removeWorktree}
                onUpdateSessionCursorModel={(id, model) => updateSession(id, { cursorModel: model })}
                onUpdateSessionClaudeModel={(id, model) => updateSession(id, { claudeModel: model })}
                onUpdateSessionClaudeToolProfile={(id, profile) => updateSession(id, { claudeToolProfile: profile })}
                onCompactSession={runCompactSession}
                onExportSession={exportSession}
                onDeleteClaudeSession={deleteClaudeSession}
                onNewClaudeSession={newClaudeSession}
                onStartEditMessage={startEditMessage}
                onResendMessage={resendMessage}
                onCancelEditMessage={cancelEditMessage}
                onCommitEditMessage={() => void commitEditMessage()}
                onSetEditingMsgDraft={setEditingMsgDraft}
                onUpdateAction={updateAction}
                onRemoveAction={removeAction}
                onExecuteAction={executeAction}
                onOpenPrInForgehold={openPrUrlInForgehold}
                onSetSessionInput={setSessionInput}
                onSendClaudeMessage={() => void sendClaudeMessage()}
                onStopClaude={() => void stopActiveAgent()}
                onOpenMentionPath={(p) => void openMentionPath(p)}
                onPasteImages={pasteImagesIntoColumn}
              />
            {:else if inst.kind === 'cursor' && connectedCursor}
              <AgentColumn
                kind="cursor"
                instanceId={inst.id}
                {claudeStatus}
                {cursorStatus}
                {githubStatus}
                {editorRepoPath}
                {activeRepoInfo}
                {dragOverInstanceId}
                {worktreeBusy}
                {worktreeMenuOpen}
                {editingMsg}
                {thinkingStartedAt}
                {thinkingTick}
                {now}
                {onAgentDragEnter}
                {onAgentDragOver}
                {onAgentDragLeave}
                {onAgentDrop}
                onPickCwd={pickCwd}
                onClearCwd={clearCwd}
                onOpenSessionFolderInEditor={openSessionFolderInEditor}
                onToggleEditorLink={toggleSessionEditorLink}
                onLinkToEditorInstance={linkActiveSessionToEditor}
                onCreateWorktree={createWorktree}
                onToggleWorktreeMenu={toggleWorktreeMenu}
                onOpenWorktreeDiff={openWorktreeDiff}
                onOpenWorktreeInEditor={openWorktreeInEditor}
                onCopyWorktreeBranch={copyWorktreeBranch}
                onApplyWorktree={applyWorktree}
                onRemoveWorktree={removeWorktree}
                onUpdateSessionCursorModel={(id, model) => updateSession(id, { cursorModel: model })}
                onUpdateSessionClaudeModel={(id, model) => updateSession(id, { claudeModel: model })}
                onUpdateSessionClaudeToolProfile={(id, profile) => updateSession(id, { claudeToolProfile: profile })}
                onCompactSession={runCompactSession}
                onExportSession={exportSession}
                onDeleteClaudeSession={deleteClaudeSession}
                onNewClaudeSession={newClaudeSession}
                onStartEditMessage={startEditMessage}
                onResendMessage={resendMessage}
                onCancelEditMessage={cancelEditMessage}
                onCommitEditMessage={() => void commitEditMessage()}
                onSetEditingMsgDraft={setEditingMsgDraft}
                onUpdateAction={updateAction}
                onRemoveAction={removeAction}
                onExecuteAction={executeAction}
                onOpenPrInForgehold={openPrUrlInForgehold}
                onSetSessionInput={setSessionInput}
                onSendClaudeMessage={() => void sendClaudeMessage()}
                onStopClaude={() => void stopActiveAgent()}
                onOpenMentionPath={(p) => void openMentionPath(p)}
                onPasteImages={pasteImagesIntoColumn}
              />
            {:else if inst.kind === 'editor'}
              <EditorColumn
                instanceId={inst.id}
                onLinkToAgent={(agentId) => linkEditorToAgent(inst.id, agentId)}
              />
            {:else if inst.kind === 'canvas'}
              <CanvasColumn instanceId={inst.id} onCardOpen={openCanvasCardSource} />
            {:else if inst.kind === 'terminal'}
              <TerminalColumn instanceId={inst.id} cwd={editorRepoPath || null} />
            {/if}
          {/each}

          {#if worktreeDiffOpen && activeSession?.worktreePath && activeSession.worktreeRepo && activeSession.worktreeBranch}
            <WorktreeDiffModal
              repo={activeSession.worktreeRepo}
              sessionId={activeSession.id}
              branch={activeSession.worktreeBranch}
              onClose={() => (worktreeDiffOpen = false)}
            />
          {/if}

        </div>
      {/if}

    {:else if view === 'githubTab'}
      <GithubTab
        bind:this={repositoriesView}
        {connectedGithub}
        {now}
        bind:view
        {tab}
        {actionBusy}
        onOpenFocusItem={openFocusItem}
        onRetryLoadDetail={() => loadDetail()}
        onTabChange={(t) => (tab = t)}
        onToggleFile={toggleFile}
        onOpenCommit={openCommit}
        onOpenComment={() => openModal('comment', { body: '', busy: false, error: null })}
        onOpenReview={() => openModal('review', { event: 'APPROVE', body: '', busy: false, error: null })}
        onOpenMerge={() => openModal('merge', { method: 'squash', busy: false, error: null })}
        onAskClose={askClose}
        onReopen={() => setState('open')}
        onOpenBrowser={openBrowser}
        onOpenCheckDetails={(url) => void openUrl(url)}
        {mergeDisabled}
      />

    {:else if view === 'jiraTab'}
      <JiraTab
        {jiraStatus}
        bind:view
        {now}
        onOpenCreateIssue={openJiraCreateIssue}
      />

    {:else if view === 'sentryTab'}
      <SentryTab
        {sentryStatus}
        bind:view
        {now}
      />

    {:else if view === 'rules'}
      <RulesView />

    {:else if view === 'connections'}
      <ConnectionsView
        {sourceConns}
        {agentConns}
        {connectedIds}
        {githubStatus}
        {jiraStatus}
        {sentryStatus}
        {claudeStatus}
        {cursorStatus}
        onDisconnectGithub={disconnectGithub}
        onDisconnectJira={disconnectJiraAll}
        onDisconnectSentry={disconnectSentryAll}
        onOpenConnectModal={openConnectModal}
      />
    {:else if view === 'settings'}
      <SettingsView />
    {:else}
      <section class="full-center">
        <div class="empty"><Sigil size={56} />
          <h2 class="empty-title" style="text-transform: capitalize;">{view}</h2>
          <p class="empty-sub">Lands in a later milestone.</p>
        </div>
      </section>
    {/if}
  </div>
</div>

<!-- Global focus overlays. Hoisted to page root so they appear in *any*
     view — workbench, github, jira, sentry, etc. The earlier per-view
     mounts only rendered when their owning component was on screen,
     which broke the `mcp__app__open_*` navigation tools (PR opened in
     the focus state but no overlay rendered until the user manually
     flipped to the GitHub tab). -->
{#if inboxState.focusItem}
  <GithubFocusOverlay
    {now}
    {tab}
    {actionBusy}
    onCloseFocus={closeFocusItem}
    onRetryLoadDetail={() => loadDetail()}
    onTabChange={(t) => (tab = t)}
    onToggleFile={toggleFile}
    onOpenCommit={openCommit}
    onOpenComment={() => openModal('comment', { body: '', busy: false, error: null })}
    onOpenReview={() => openModal('review', { event: 'APPROVE', body: '', busy: false, error: null })}
    onOpenMerge={() => openModal('merge', { method: 'squash', busy: false, error: null })}
    onAskClose={askClose}
    onReopen={() => setState('open')}
    onOpenBrowser={openBrowser}
    onOpenCheckDetails={(url) => void openUrl(url)}
    {mergeDisabled}
  />
{/if}

{#if inboxState.jiraFocusKey}
  <div class="slide-over" onclick={(e) => { if (e.target === e.currentTarget) inboxState.jiraFocusKey = null; }} onkeydown={(e) => { if (e.key === 'Escape') inboxState.jiraFocusKey = null; }} role="dialog" aria-modal="true" tabindex="-1">
    <div class="slide-panel">
      <JiraDetailPane
        issueKey={inboxState.jiraFocusKey}
        {now}
        onClose={() => (inboxState.jiraFocusKey = null)}
        onStatusChange={() => void refreshAllJiraInboxes({ silent: true })}
      />
    </div>
  </div>
{/if}

{#if inboxState.sentryFocusId}
  <div class="slide-over" onclick={(e) => { if (e.target === e.currentTarget) inboxState.sentryFocusId = null; }} onkeydown={(e) => { if (e.key === 'Escape') inboxState.sentryFocusId = null; }} role="dialog" aria-modal="true" tabindex="-1">
    <div class="slide-panel">
      <SentryDetailPane
        issueId={inboxState.sentryFocusId}
        {now}
        onClose={() => (inboxState.sentryFocusId = null)}
        onOpenBrowser={openBrowser}
      />
    </div>
  </div>
{/if}

<ModalsRoot
  {now}
  {openBrowser}
  {onUserPickerInput}
  selectJiraUser={selectAssignee}
  selectAnyJiraUser={selectAnyAssignee}
  submitJiraConnect={submitJira}
  {jiraTokenUrl}
  submitSentryConnect={submitSentry}
  {sentryTokenUrl}
  refreshClaudeStatus={refreshClaudeModal}
  {claudeInstallUrl}
  refreshCursorStatus={refreshCursorModal}
  {cursorInstallUrl}
  {submitPat}
  {githubTokenUrl}
  {submitComment}
  {submitReview}
  {submitMerge}
  {onJiraCreateProjectChange}
  {submitJiraCreate}
  {onGithubPrRepoChange}
  {onGithubPrBranchesChange}
  {submitGithubPr}
/>

<CommandPalette
  bind:open={paletteOpen}
  setView={(v) => (view = v)}
  actions={paletteActions}
/>

<style>
  .bg {
    position: fixed; inset: 0; pointer-events: none; z-index: 0;
    background:
      radial-gradient(ellipse 1200px 600px at 10% 0%, rgba(30, 58, 107, 0.18), transparent 60%),
      radial-gradient(ellipse 900px 500px at 90% 100%, rgba(16, 185, 129, 0.06), transparent 60%);
  }
  #app { position: relative; z-index: 1; display: grid; grid-template-columns: 56px 1fr; height: 100vh; }

  /* Touch ID / device-owner-auth gate shown at launch. Sits over the app
     (z-index 500) so the workbench doesn't flash through before unlock —
     the underlying app still mounts so the moment we flip `appLocked=false`
     the usual UI is already primed. */
  .lock-screen {
    position: fixed; inset: 0; z-index: 500;
    background: radial-gradient(ellipse at center, rgba(12, 17, 23, 0.92), rgba(12, 17, 23, 0.98));
    backdrop-filter: blur(20px);
    display: flex; align-items: center; justify-content: center;
    animation: fadeIn 200ms ease-out;
  }
  .lock-card {
    max-width: 420px;
    padding: 44px 40px 36px;
    text-align: center;
    display: flex; flex-direction: column; align-items: center; gap: 14px;
    background: var(--bg-1);
    border: 1px solid var(--border-neutral-hi);
    border-radius: 16px;
    box-shadow: 0 20px 60px rgba(0, 0, 0, 0.5);
  }
  .lock-title {
    font-size: 22px; font-weight: 600; color: var(--text-0);
    letter-spacing: -0.015em; margin: 8px 0 0;
  }
  .lock-sub {
    font-size: 13.5px; color: var(--text-1); margin: 0;
    line-height: 1.55; max-width: 340px;
  }
  .lock-err {
    font-size: 12px; color: var(--error);
    padding: 8px 12px; border-radius: 6px;
    background: rgba(214, 72, 44, 0.1);
    border: 1px solid rgba(214, 72, 44, 0.25);
  }
  .lock-card .btn {
    margin-top: 6px;
    min-width: 220px;
  }
  @keyframes fadeIn {
    from { opacity: 0; }
    to { opacity: 1; }
  }


  .main {
    min-height: 0; height: 100%;
    overflow: hidden; display: flex; flex-direction: column;
  }
  .full-center { flex: 1; display: flex; align-items: center; justify-content: center; padding: 40px; }
  .empty { display: flex; flex-direction: column; align-items: center; gap: 16px; text-align: center; max-width: 420px; }
  .empty-title { font-size: 22px; font-weight: 600; margin: 12px 0 0; color: var(--text-0); letter-spacing: -0.015em; }
  .empty-sub { font-size: 13.5px; color: var(--text-1); margin: 0; line-height: 1.55; max-width: 380px; }

  /* Workbench tabs — named presets of column layouts */
  .wb-tabs {
    display: flex; align-items: center; gap: 2px;
    padding: 6px 12px 4px;
    border-bottom: 1px solid var(--border-neutral);
    /* `bg-2` is the "elevated header" surface: lighter than bg-0 in
       dark themes, darker than bg-0 in light. Either way the strip
       reads as its own toolbar instead of melting into the page bg. */
    background: var(--bg-2);
    overflow-x: auto;
    scrollbar-width: none;
  }
  .wb-tabs::-webkit-scrollbar { display: none; }
  .wb-tab {
    display: inline-flex; align-items: center; gap: 6px;
    padding: 5px 10px;
    border-radius: 6px 6px 0 0;
    font-size: 12px; font-weight: 500;
    color: var(--text-2);
    background: var(--bg-1);
    cursor: pointer;
    /* Visible outline on every tab so the tab strip reads as a row of
       clickable pills, not text. Active state gets a stronger border +
       accent under-line; hover bumps the contrast a notch. */
    border: 1px solid var(--border-neutral);
    border-bottom: none;
    transition: all 120ms;
    flex-shrink: 0;
    max-width: 200px;
    position: relative;
  }
  .wb-tab:hover {
    color: var(--text-0);
    background: var(--bg-2);
    border-color: var(--border-hi);
  }
  .wb-tab.active {
    color: var(--text-0);
    background: var(--bg-0);
    border-color: var(--border-hi);
    box-shadow: var(--shadow-1);
  }
  /* Accent under-line on the active tab — sits flush with the tab-strip
     bottom border so the tab "owns" that section of the strip. */
  .wb-tab.active::after {
    content: '';
    position: absolute;
    left: 0; right: 0; bottom: -1px;
    height: 2px;
    background: var(--accent);
    border-radius: 1px 1px 0 0;
  }
  /* Drag-over highlight when the user is moving a column onto this tab. */
  .wb-tab.drag-over {
    color: var(--accent-fg);
    background: var(--accent);
    border-color: var(--accent);
    box-shadow: 0 0 0 2px var(--accent), 0 0 12px var(--accent-glow);
  }
  .wb-tab.drag-over .wb-tab-count { background: color-mix(in srgb, var(--accent-fg) 18%, transparent); color: var(--accent-fg); }
  .wb-tab-name {
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
    letter-spacing: -0.005em;
  }
  .wb-tab-count {
    padding: 0 5px;
    min-width: 16px; height: 15px;
    border-radius: 8px;
    background: var(--bg-3);
    font-size: 10px; font-weight: 600;
    display: inline-flex; align-items: center; justify-content: center;
    color: var(--text-1);
  }
  .wb-tab.active .wb-tab-count { background: var(--accent-soft); color: var(--accent-bright); }
  .wb-tab-close {
    display: inline-flex; align-items: center; justify-content: center;
    width: 16px; height: 16px;
    border-radius: 3px;
    color: var(--text-mute);
    opacity: 0; transition: all 120ms;
    cursor: pointer;
  }
  .wb-tab:hover .wb-tab-close { opacity: 1; }
  .wb-tab-close:hover { color: #fca5a5; background: var(--bg-3); }
  .wb-tab-close svg { width: 10px; height: 10px; }
  .wb-tab-rename {
    padding: 0 4px;
    background: var(--bg-0);
    border: 1px solid var(--border-hi);
    border-radius: 4px;
    color: var(--text-0);
    font: inherit;
    font-size: 12px;
    width: 140px;
    outline: none;
  }
  .wb-tab-add {
    display: inline-flex; align-items: center; gap: 4px;
    padding: 5px 10px;
    color: var(--text-2);
    background: transparent;
    border: 1px dashed var(--border-neutral-hi);
    border-radius: 6px;
    font-size: 11.5px;
    cursor: pointer;
    margin-left: 6px;
    transition: all 120ms;
    flex-shrink: 0;
  }
  .wb-tab-add:hover { color: var(--accent-bright); border-color: var(--border-hi); background: var(--accent-soft); }
  .wb-tab-add svg { width: 11px; height: 11px; stroke: currentColor; stroke-width: 2; fill: none; stroke-linecap: round; }

  /* Workbench — kanban columns */
  .wb-bar {
    display: flex; align-items: center; gap: 6px;
    padding: 10px 16px;
    border-bottom: 1px solid var(--border-neutral);
    background: var(--bg-1);
  }

  /* Pill-group — a single pill or a pill + plus-button compound. The whole
     group shares one border, so the icon + label + "+" read as one unit. */
  .pill-group {
    display: inline-flex; align-items: stretch;
    border: 1px solid var(--border-neutral);
    border-radius: 999px;
    background: var(--bg-1);
    /* No `overflow: hidden` — it would clip the absolute-positioned
       hover menu (and any focus ring). Rounded corners on inner buttons
       use border-radius directly so nothing pokes out of the pill shape. */
    transition: border-color 140ms, background 140ms, box-shadow 140ms;
    position: relative;
  }
  .pill-group:hover {
    border-color: var(--border-neutral-hi);
    background: var(--bg-2);
    box-shadow: var(--shadow-1);
  }
  .pill-group.active {
    border-color: var(--border-hi);
    background: var(--bg-2);
    box-shadow: 0 0 0 1px var(--border-hi), var(--shadow-1), 0 0 12px var(--accent-glow);
  }
  .pill-group.active::after {
    /* Little glow dot at the bottom-center when column is open. */
    content: ''; position: absolute; left: 50%; bottom: -5px;
    width: 4px; height: 4px; border-radius: 50%;
    background: var(--accent-bright);
    box-shadow: 0 0 6px var(--accent-glow);
    transform: translateX(-50%);
  }
  .pill {
    display: inline-flex; align-items: center; gap: 8px;
    padding: 5px 12px;
    color: var(--text-1);
    font-size: 12.5px; font-weight: 500;
    background: none; border: none; cursor: pointer;
    transition: color 140ms;
    border-top-left-radius: 999px;
    border-bottom-left-radius: 999px;
  }
  .pill:hover { color: var(--text-0); }
  .pill-group.active .pill { color: var(--text-0); }
  .pill-icon {
    display: inline-flex; align-items: center; justify-content: center;
    width: 16px; height: 16px; border-radius: 3px;
    flex-shrink: 0;
  }
  .pill-icon svg { width: 12px; height: 12px; display: block; color: currentColor; }
  .pill-icon--editor { color: var(--warning); }
  .pill-icon--terminal { color: var(--success); }
  .pill-label { letter-spacing: -0.005em; }
  .pill-count {
    padding: 0 6px;
    min-width: 16px; height: 16px;
    border-radius: 8px;
    background: var(--bg-3);
    font-size: 10.5px; font-weight: 600;
    display: inline-flex; align-items: center; justify-content: center;
    color: var(--text-1);
  }
  .pill-group.active .pill-count { background: var(--accent-soft); color: var(--accent-bright); }
  /* Archived counter — dotted border + transparent fill so it reads as
     "in storage, not running" next to the solid live counter. The
     non-active fallback for `.pill-count` is `var(--bg-3)`; we override
     it to transparent and replace with a dotted ring. */
  .pill-count--archived {
    background: transparent;
    border: 1px dotted var(--text-mute);
    color: var(--text-mute);
    /* `border` adds 2px to width/height — shrink min-width to keep the
       ring the same overall diameter as the live counter. */
    min-width: 14px; height: 14px;
    padding: 0 5px;
  }
  .pill-group.active .pill-count--archived {
    background: transparent;
    border-color: var(--accent-bright);
    color: var(--accent-bright);
  }
  .pill-add {
    display: inline-flex; align-items: center; justify-content: center;
    width: 26px;
    color: var(--text-2);
    background: none; border: none; cursor: pointer;
    border-left: 1px solid var(--border-neutral);
    border-top-right-radius: 999px;
    border-bottom-right-radius: 999px;
    transition: all 140ms;
  }
  .pill-add:hover { color: var(--accent-bright); background: var(--accent-soft); }
  .pill-add svg { width: 12px; height: 12px; stroke: currentColor; stroke-width: 2; stroke-linecap: round; fill: none; }

  /* "No columns yet" state — the pill body is disabled but the + next to
     it still pops to the foreground so the create path reads clearly. */
  .pill-group.dim { opacity: 0.55; }
  .pill-group.dim .pill { cursor: default; }
  .pill-group.dim:hover { opacity: 0.85; border-color: var(--border-neutral-hi); }
  .pill:disabled { cursor: default; }

  /* Hover-expand menu: lists every instance of this kind with its bench
     name + workbench it lives in. Clicking an item switches workbench (if
     needed) and scrolls the column into view.
     Kept in the DOM all the time (when count>0) but hidden — that way the
     CSS `:hover` chain covers both pill AND menu (since the menu is a DOM
     descendant of `.pill-group`), so sliding from pill to menu never
     triggers a close. `top: 100%` (no gap) keeps the hit area continuous. */
  .pill-menu {
    position: absolute; top: 100%; left: 0;
    margin-top: 4px;
    min-width: 240px; max-width: 320px;
    background: var(--bg-2);
    border: 1px solid var(--border-hi);
    border-radius: 10px;
    box-shadow: 0 12px 32px rgba(0, 0, 0, 0.45);
    padding: 4px;
    display: none;
    flex-direction: column; gap: 1px;
    z-index: 40;
  }
  /* Transparent "bridge" filling the 4px margin gap so the hover hit-area
     between pill and menu is continuous. */
  .pill-menu::before {
    content: '';
    position: absolute; top: -6px; left: 0; right: 0; height: 6px;
  }
  /* Menu open triggers:
     - `:hover` for normal mouse use,
     - `.drag-armed` while a drag is currently springing this pill open
       (set by `onPillDragEnter`, persists while cursor is anywhere over
       pill body OR any menu item, with a small close-delay so cursor
       can travel between them). */
  .pill-group.has-menu:hover .pill-menu,
  .pill-group.has-menu.drag-armed .pill-menu {
    display: flex;
    animation: fadeIn 120ms ease-out;
  }
  /* Drag-hover — accent outline so "here's the drop target" reads clearly,
     distinct from plain `:hover`. */
  .pill-group.drag-over {
    box-shadow: 0 0 0 2px var(--accent), 0 0 12px var(--accent-glow);
  }
  .pill-menu-item.drag-over {
    background: var(--accent);
    color: var(--accent-fg);
  }
  .pill-menu-item.drag-over .pill-menu-dot {
    background: var(--accent-fg); box-shadow: none;
  }
  .pill-menu-item.drag-over .pill-menu-wb {
    background: color-mix(in srgb, var(--accent-fg) 20%, transparent); color: var(--accent-fg); border-color: transparent;
  }
  .pill-menu-head {
    font-size: 10px; font-weight: 600; letter-spacing: 0.05em;
    color: var(--text-mute); text-transform: uppercase;
    padding: 7px 10px 5px;
    border-bottom: 1px solid var(--border-neutral);
    margin-bottom: 3px;
  }
  .pill-menu-item {
    display: flex; align-items: center; gap: 8px;
    padding: 7px 10px;
    border-radius: 6px;
    font-size: 12px; color: var(--text-1);
    text-align: left;
    transition: background 100ms;
    cursor: pointer;
  }
  .pill-menu-item:hover { background: var(--bg-3); color: var(--text-0); }
  .pill-menu-item.is-current { background: var(--accent-soft); color: var(--accent-bright); }
  .pill-menu-dot {
    width: 6px; height: 6px; border-radius: 50%;
    background: var(--text-mute);
    flex-shrink: 0;
  }
  .pill-menu-dot.is-active {
    background: var(--accent-bright);
    box-shadow: 0 0 6px var(--accent-glow);
  }
  .pill-menu-name {
    flex: 1; font-size: 11.5px; color: inherit;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .pill-menu-wb {
    font-size: 10px; color: var(--text-mute);
    padding: 1px 6px; border-radius: 3px;
    background: var(--bg-1);
    border: 1px solid var(--border-neutral);
    flex-shrink: 0;
  }

  /* Archived block — sits below the live columns, separator above. */
  .pill-menu-head--archive {
    margin-top: 6px;
    color: var(--text-mute);
  }
  .pill-menu-item--archived {
    /* Inactive look: dim text, no hover highlight on the row itself
       (only the restore button is interactive). */
    opacity: 0.55;
    cursor: default;
  }
  .pill-menu-item--archived:hover { background: transparent; color: var(--text-1); }
  .pill-menu-restore {
    display: inline-flex; align-items: center; justify-content: center;
    width: 22px; height: 22px; border-radius: 5px;
    color: var(--text-2); background: transparent; border: none; cursor: pointer;
    flex-shrink: 0;
    transition: all 120ms;
  }
  .pill-menu-restore:hover {
    background: var(--accent-soft);
    color: var(--accent-bright);
  }
  .pill-menu-item--archived:hover .pill-menu-restore { opacity: 1; }

  .wb-columns {
    flex: 1;
    display: flex;
    min-height: 0;
    overflow-x: auto;
    overflow-y: hidden;
    position: relative;
    /* Force a persistent, visible horizontal scrollbar — macOS's
       auto-hiding scrollbars made users think scroll was broken. */
    scrollbar-color: var(--accent-soft) transparent;
    scrollbar-gutter: stable;
  }
  .wb-columns::-webkit-scrollbar { height: 10px; }
  .wb-columns::-webkit-scrollbar-track { background: transparent; }
  .wb-columns::-webkit-scrollbar-thumb {
    background: var(--border-hi);
    border-radius: 5px;
    border: 2px solid transparent;
    background-clip: padding-box;
  }
  .wb-columns::-webkit-scrollbar-thumb:hover { background: var(--accent); background-clip: padding-box; }

  /* Empty-workbench discoverability nudge. Centered with generous
     padding so it reads as guidance rather than a permanent fixture. */
  :global(.wb-empty-hint) {
    margin: auto;
    padding: 28px 36px;
    max-width: 520px;
    text-align: center;
    color: var(--text-2);
    border: 1px dashed var(--border-neutral);
    border-radius: 14px;
    background: var(--bg-1);
  }
  :global(.wb-empty-hint-title) {
    font-size: 15px; font-weight: 600; color: var(--text-0);
    margin-bottom: 8px;
  }
  :global(.wb-empty-hint-sub) {
    font-size: 12.5px; line-height: 1.6;
  }
  :global(.wb-empty-hint-sub .mono) {
    background: var(--bg-2);
    border: 1px solid var(--border-neutral);
    padding: 1px 6px; border-radius: 5px;
    color: var(--text-1);
  }

  /* ======================================================================
     Maximize-overlay: one column expanded to fill the whole workbench area.
     Layered above its siblings, leaving them in the DOM (state untouched —
     scroll positions, in-flight requests, focused inputs all keep working).
     ESC / the toolbar button restore. .wb-columns is already
     `position: relative` so `inset: 0` anchors to it, not the viewport.
     `!important` overrides the inline `style="flex: 0 0 …px"` written by
     the resize handler — without it the column would still try to honour
     its width.
  ====================================================================== */
  .wb-columns:has(.wb-column--maximized) > :global(.wb-column:not(.wb-column--maximized)) {
    /* Visually hide siblings but keep them mounted. visibility:hidden over
       display:none so we don't tear down their internal state (chat scroll,
       editor selection, ongoing streams). */
    visibility: hidden;
    pointer-events: none;
  }
  :global(.wb-column.wb-column--maximized) {
    position: absolute !important;
    inset: 0 !important;
    z-index: 50 !important;
    flex: 1 1 100% !important;
    width: auto !important;
    box-shadow: 0 0 0 1px var(--border-hi), 0 8px 30px rgba(0, 0, 0, 0.55) !important;
    animation: wb-max-in 160ms cubic-bezier(0.2, 0.8, 0.2, 1);
  }
  /* Hide the resize handle of a maximized column — there's nothing to resize
     against. Don't drop the wb-col-controls bar; that's where Restore lives. */
  :global(.wb-column.wb-column--maximized > .wb-col-resize) { display: none !important; }
  @keyframes wb-max-in {
    from { opacity: 0.6; transform: scale(0.985); }
    to   { opacity: 1; transform: scale(1); }
  }
</style>
