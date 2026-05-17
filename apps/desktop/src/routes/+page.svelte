<script lang="ts">
  import { onMount, onDestroy, tick } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import { open as openDialog } from '@tauri-apps/plugin-dialog';
  import Sigil from '$lib/components/ui/Sigil.svelte';
  import Cheatsheet from '$lib/components/ui/Cheatsheet.svelte';
  import WelcomeOverlay from '$lib/components/ui/WelcomeOverlay.svelte';
  import Welcome from '$lib/components/ui/Welcome.svelte';
  import { welcomeState } from '$lib/state/welcome.svelte';
  import WorktreeDiffModal from '$lib/components/editor/WorktreeDiffModal.svelte';
  import Rail from '$lib/components/ui/Rail.svelte';
  import RulesView from '$lib/views/RulesView.svelte';
  import LibraryApp from '$lib/views/apps/LibraryApp.svelte';
  import ConnectionsView from '$lib/views/ConnectionsView.svelte';
  import SettingsView from '$lib/views/SettingsView.svelte';
  import AgentApp from '$lib/views/apps/AgentApp.svelte';
  import JiraApp from '$lib/views/apps/JiraApp.svelte';
  import GithubApp from '$lib/views/apps/GithubApp.svelte';
  import SentryApp from '$lib/views/apps/SentryApp.svelte';
  import EditorApp from '$lib/views/apps/EditorApp.svelte';
  import CanvasApp from '$lib/views/apps/CanvasApp.svelte';
  import TerminalApp from '$lib/views/apps/TerminalApp.svelte';
  import HomeApp from '$lib/views/apps/HomeApp.svelte';
  import BrandIcon from '$lib/components/ui/BrandIcon.svelte';
  import CommandPalette from '$lib/components/ui/CommandPalette.svelte';
  import AgentDashboard from '$lib/views/AgentDashboard.svelte';
  import SearchInFilesOverlay from '$lib/components/editor/SearchInFilesOverlay.svelte';
  import QuickOpenOverlay from '$lib/components/editor/QuickOpenOverlay.svelte';
  import SymbolPickerOverlay from '$lib/components/editor/SymbolPickerOverlay.svelte';
  import ModalsRoot from '$lib/components/modals/ModalsRoot.svelte';
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
  import { applySessionCwd, extractCompactSummary, buildContinuationRecap } from '$lib/services/sessionCwd';
  import { buildFirstTurnPreamble, getActiveEditorFile } from '$lib/services/firstTurnContext';
  import { runCompactSession as runCompactSessionService } from '$lib/services/agentCompact';
  import { exportSessionMarkdown, exportSessionJson } from '$lib/services/sessionExport';
  import {
    parseSlashCommand,
    parseSlashCommandWithArgs,
    clearSessionHistory,
    appendUsageBreakdown,
    appendSlashHelp,
    appendBgTaskList,
    spawnPreviewFromSlash,
    killTaskFromSlash,
    startLoopFromSlash,
    stopLoopFromSlash,
    KNOWN_SLASH_COMMANDS
  } from '$lib/services/slashCommands';
  import { openFileInEditor } from '$lib/services/editorNavigation';
  import { refreshPlanUsage } from '$lib/state/quota.svelte';
  import {
    layoutState,
    persistPanelState,
    restorePanelState,
    registerInstanceRemovedHook,
    APP_INSTANCE_IDS,
    MULTI_INSTANCE_KINDS,
    addInstance as addLayoutInstance,
    kindForInstanceId
  } from '$lib/state/layout.svelte';
  import {
    sessionsState,
    persistSessionsEffect,
    persistRulesEffect,
    persistEditorInstanceStateEffect,
    initSessionsFromDisk,
    flushSessionsNow,
    newClaudeSession,
    deleteClaudeSession,
    updateSession,
    focusSession,
    appendSessionMessage,
    flushActionResultsToUI,
    drainPendingActionResultsForAgent,
    formatActionResultsForPrompt,
    appendToLastAssistant,
    replaceLastAssistant,
    addAction,
    updateAction,
    removeAction,
    truncateSessionAt,
    setSessionInput,
    attachPathsToSession,
    setActiveSessionInInstance,
    orphanSessionsForInstance,
    genId,
    genUuid
  } from '$lib/state/sessions.svelte';
  import {
    connectionsState,
    sourceConns,
    agentConns,
    refreshAllStatus,
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
  import { initDensity, toggleDensity } from '$lib/state/density.svelte';
  import { initBgTasks } from '$lib/state/bgTasks.svelte';
  import { initSdd, workspaceForSession, refreshSdd } from '$lib/state/sdd.svelte';
  import { loadHookConfig, runHook } from '$lib/state/hooks.svelte';
  import { skillsState, refreshSkills, renderSkill } from '$lib/state/skills.svelte';
  import { loadClaudeMd } from '$lib/state/claudemd.svelte';
  import { refreshAutoMemory } from '$lib/state/autoMemory.svelte';
  import {
    loadStatusLineConfig,
    runStatusLine,
    installStatusLineTimer,
    statuslineState,
    type StatusLinePayload
  } from '$lib/state/statusline.svelte';
  import { costForUsage, contextWindowFor } from '$lib/usage';
  import { badgeCount, markSourceSeen } from '$lib/state/railBadges.svelte';
  import {
    dragState,
    setDragPayload,
    installGlobalDragSafetyNet,
    type DragPayload
  } from '$lib/state/drag.svelte';
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
  import { ensureTerminalSession } from '$lib/state/terminals.svelte';
  import {
    runAgentRequest,
    stopAgentRequest,
    prewarmAgent,
    dropPrewarm,
    isResumeOrphanError,
    RESUME_ORPHAN_PREFIX
  } from '$lib/exec/claude';
  import type { ClaudeAction, ClaudeSession, Mention, PanelKind, RepoInfo } from '$lib/types';
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
    type SentryIssue,
    type SentryUser,
    type RepoBranch,
    type Repository,
    relativeTime
  } from '$lib/data';
  import { basename, formatToolUse, isImagePath, truncInline } from '$lib/format';

  type View =
    | 'home'
    | 'jiraApp'
    | 'githubApp'
    | 'sentryApp'
    | 'claudeApp'
    | 'cursorApp'
    | 'editorApp'
    | 'canvasApp'
    | 'terminalApp'
    | 'rules'
    | 'library'
    | 'connections'
    | 'settings';
  type DetailTab = 'conversation' | 'commits' | 'files' | 'reviews' | 'checks';

  /* Default view = Claude solo. Fresh installs land here (or get
   * redirected to Connections by the rail if nothing is set up). */
  let view = $state<View>('claudeApp');

  /* Browser-style back/forward stack for solo navigation. ⌘[ steps
   * back through the user's view history, ⌘] redoes it. We capture
   * the previous view via a `prevView` shadow + an effect that
   * watches `view`; user-initiated changes push the prev onto
   * `viewHistory` and clear `viewFuture`, while back/forward nav
   * sets `viewNavigatingHistory = true` so the effect skips the
   * push (otherwise back-stepping would itself add to history and
   * trap the user in a cycle). Cap at 50 entries — anything older
   * is realistically not useful and the unbounded list would slowly
   * leak memory in a long-lived desktop process. */
  let viewHistory = $state<View[]>([]);
  let viewFuture = $state<View[]>([]);
  let prevView: View = 'claudeApp';
  let viewNavigatingHistory = false;
  $effect(() => {
    const cur = view;
    if (cur === prevView) return;
    if (viewNavigatingHistory) {
      viewNavigatingHistory = false;
    } else {
      viewHistory.push(prevView);
      if (viewHistory.length > 50) viewHistory.shift();
      // User moved somewhere new — any "forward" path becomes stale,
      // same as a browser's address-bar nav after pressing Back.
      if (viewFuture.length) viewFuture = [];
    }
    prevView = cur;
  });
  function navBack() {
    if (!viewHistory.length) return;
    const target = viewHistory.pop()!;
    viewFuture.push(prevView);
    if (viewFuture.length > 50) viewFuture.shift();
    viewNavigatingHistory = true;
    view = target;
  }
  function navForward() {
    if (!viewFuture.length) return;
    const target = viewFuture.pop()!;
    viewHistory.push(prevView);
    if (viewHistory.length > 50) viewHistory.shift();
    viewNavigatingHistory = true;
    view = target;
  }

  /* ⌘0..⌘8 → solo. Order matches the rail top-to-bottom so the digit
   * reads as "the icon at row N". Rail tooltips have advertised these
   * since v1; the keyboard binding lives in onKey below. */
  const SOLO_BY_DIGIT: Record<string, View> = {
    '0': 'home',
    '1': 'jiraApp',
    '2': 'githubApp',
    '3': 'sentryApp',
    '4': 'claudeApp',
    '5': 'cursorApp',
    '6': 'editorApp',
    '7': 'canvasApp',
    '8': 'terminalApp'
  };

  /** True whenever the user is in a source-solo view (Jira / GitHub /
   *  Sentry). Used to gate keyboard shortcuts that only make sense
   *  inside an inbox view (j/k navigation). The detail pane is
   *  rendered inline in the right pane of the source app, so each
   *  app's focus state persists across navigation and the user can
   *  leave + return without losing the PR/ticket/issue they were
   *  reading. */
  const isSourceApp = $derived(
    view === 'jiraApp' || view === 'githubApp' || view === 'sentryApp'
  );

  /* Rail badge counts. Reads the primary-instance inbox list per source
   * and asks `badgeCount` how many of those exceed the user's last-seen
   * baseline (stored in localStorage by `railBadges`). The badge auto-
   * hides while the user is viewing that solo — see Rail.svelte's
   * `view !== '...App'` guard — and an effect below snapshots the
   * current count as the new baseline on view entry. */
  /* Per-agent-kind "is there an in-flight turn right now" signal.
   * Drives the ambient pulse on the rail's Claude / Cursor icons so
   * the user can tell at a glance that an agent is thinking even
   * when they've switched out of the solo. We read the active
   * session id per kind, then check its `sending` flag; an unknown
   * id (`activeIds[kind]` is null on a fresh install before the
   * first session) resolves to `false` cleanly. */
  const claudeBusy = $derived.by(() => {
    const id = sessionsState.activeIds['claude'];
    if (!id) return false;
    const s = sessionsState.list.find((x) => x.id === id);
    return !!s?.sending;
  });
  const cursorBusy = $derived.by(() => {
    const id = sessionsState.activeIds['cursor'];
    if (!id) return false;
    const s = sessionsState.list.find((x) => x.id === id);
    return !!s?.sending;
  });

  /* Source-accent CSS var matching the current solo. Drives the
   * brief brand-tinted flash painted across `.main` on every view
   * change so the bigger-picture context switch reads as more than
   * a content swap — the solo "lights up" in its own colour for a
   * moment. Falls back to the global accent for non-source views
   * (home / rules / library / connections / settings). */
  const viewFlashTone = $derived.by(() => {
    switch (view) {
      case 'jiraApp': return 'var(--src-jira)';
      case 'githubApp': return 'var(--src-github)';
      case 'sentryApp': return 'var(--src-sentry)';
      case 'claudeApp': return 'var(--src-claude)';
      case 'cursorApp': return 'var(--src-cursor)';
      case 'editorApp': return 'var(--src-editor)';
      case 'canvasApp': return 'var(--src-canvas)';
      case 'terminalApp': return 'var(--src-term)';
      default: return 'var(--accent)';
    }
  });

  const githubInboxCount = $derived(
    (inboxState.itemsByInstance[APP_INSTANCE_IDS.github] ?? []).length
  );
  const jiraInboxCount = $derived(
    (inboxState.jiraItemsByInstance[APP_INSTANCE_IDS.jira] ?? []).length
  );
  const sentryInboxCount = $derived(
    (inboxState.sentryItemsByInstance[APP_INSTANCE_IDS.sentry] ?? []).length
  );
  const githubBadge = $derived(badgeCount('github', githubInboxCount));
  const jiraBadge = $derived(badgeCount('jira', jiraInboxCount));
  const sentryBadge = $derived(badgeCount('sentry', sentryInboxCount));

  /* When the user opens a source solo, snapshot its current inbox
   * count as the new "seen" baseline so the badge clears. Subsequent
   * refreshes that add items will repopulate the delta. We watch the
   * view AND the count: re-entering the solo while items arrive in
   * the background should still clear, and arriving items while
   * already inside the solo should re-snapshot (the user is
   * actively looking — no "unread" is meaningful). */
  $effect(() => {
    if (view === 'githubApp') markSourceSeen('github', githubInboxCount);
  });
  $effect(() => {
    if (view === 'jiraApp') markSourceSeen('jira', jiraInboxCount);
  });
  $effect(() => {
    if (view === 'sentryApp') markSourceSeen('sentry', sentryInboxCount);
  });

  let paletteOpen = $state(false);
  /** Agent View dashboard — ⌘⇧A overlay listing every Claude/Cursor
   *  session grouped by state. Pure overlay, no view change underneath. */
  let agentDashboardOpen = $state(false);
  /* Which "flavour" of palette is currently open. `recents` is the
   * ⌘E quick-switcher mode (only Recent section, larger cap, Cmd-Tab
   * feel); `normal` is the full ⌘K palette. Bound to the palette so
   * it can settle back to `normal` on close. */
  let paletteMode = $state<'normal' | 'recents'>('normal');
  /* ⌘⇧F project-wide find — closes the long-standing cut from
   * EDITOR.md §1.3 + ROADMAP_1.0.md §2.1. Runs against the editor
   * singleton's repoPath via `fs_search_text`. */
  let searchInFilesOpen = $state(false);
  /* ⌘P quick-open overlay. Same scope as SearchInFilesOverlay (the
   * active editor's repo), but keys on the file's name + relative
   * path via `fs_walk_files` + `fuzzyScoreAny`. Lives next to the
   * search overlay so the keyboard handlers below can flip both with
   * the same Esc cascade. */
  let quickOpenOpen = $state(false);
  /* ⇧⌘O symbol-outline picker — companion to ⌘P but scoped to the
   * currently-active editor buffer. We extract symbols via a small
   * regex pass (services/symbolOutline.ts) instead of standing up a
   * tree-sitter pipeline; the picker reads `localStorage`'s
   * per-instance active-file key and runs the parse on demand. */
  let symbolPickerOpen = $state(false);
  /* Cheatsheet overlay (`?` toggles). Owned at +page level so any
   * shortcut, anywhere, can flip it without prop-drilling. */
  let cheatsheetOpen = $state(false);
  /* Welcome / "what is this app" overlay. ⇧⌘? toggles — Cheatsheet
   * stays on bare `?` because it's a quick lookup, while Welcome is
   * the longer-form orientation surface. We keep them separate so
   * users coming from "show me the keys" don't have to scroll past
   * an essay. */
  let welcomeOpen = $state(false);
  let tab = $state<DetailTab>('conversation');

  // Biometric gate. Before first unlock the UI shows a "Locked" overlay so
  // credentials in keychain can't be pulled until the user taps Touch ID
  // (or confirms with the Mac passcode). `biometryError` surfaces the last
  // LAContext failure so we can show "cancelled" / "not enrolled" etc.
  //
  // Skip the gate when not running inside Tauri (e.g. browser dev preview):
  // there's no Keychain, no biometric API — locking would just leave the
  // browser preview stuck on a screen that can never unlock.
  const inTauri =
    typeof window !== 'undefined' &&
    !!(window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
  let appLocked = $state(inTauri);
  let biometryInFlight = $state(false);
  let biometryError = $state<string | null>(null);

  async function biometricUnlock() {
    if (biometryInFlight) return;
    biometryInFlight = true;
    biometryError = null;
    try {
      await invoke('biometric_unlock', { reason: 'Unlock Woom to access your stored credentials' });
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
  // Legacy GithubTab repositories binding — kept as null since the
  // tab view is gone (GitHub app does both inbox + focus now).
  // Removing this would also remove the optional-chained refresh hook
  // below, so we keep the binding ref for backward compatibility.
  let repositoriesView: { refreshItems: () => void } | null = null;

  // Shared editor repo path. In multi-instance world we have one path per
  // editor column instance (see sessionsState.editorInstanceState). For
  // Claude sessions without an explicit cwd, we fall back to the FIRST editor
  // Editor singleton: there's exactly one editor surface in solo mode, so
  // the repo path is just the slot keyed on `APP_INSTANCE_IDS.editor`.
  const editorRepoPath = $derived(
    sessionsState.editorInstanceState[APP_INSTANCE_IDS.editor]?.repoPath ?? ''
  );

  function setEditorRepoPath(value: string, instanceId?: string) {
    const id = instanceId ?? APP_INSTANCE_IDS.editor;
    if (!sessionsState.editorInstanceState[id]) {
      sessionsState.editorInstanceState[id] = { repoPath: value };
    } else {
      sessionsState.editorInstanceState[id].repoPath = value;
    }
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
  // click doesn't open the PR detail pane.
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
  // are imported from $lib/types so the solo app components can
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

  /** Group agent sessions by relative date for the soloAgent sidebar.
   *  Returns four buckets with non-empty contents only — Today /
   *  Yesterday / This week / Older. The "now" arg makes the result
   *  reactive when the parent ticks. Sessions with no messages yet
   *  bucket into "Older" so they don't pollute Today. */
  function groupAgentSessions(
    kind: 'claude' | 'cursor',
    nowMs: number
  ): { label: string; items: typeof sessionsState.list }[] {
    const items = sessionsState.list.filter((s) => s.agentKind === kind);
    const dayMs = 24 * 60 * 60 * 1000;
    const sessTime = (s: typeof items[number]) => {
      const last = s.messages[s.messages.length - 1]?.at;
      return last ? new Date(last).getTime() : 0;
    };
    const sorted = [...items].sort((a, b) => sessTime(b) - sessTime(a));
    const today: typeof items = [];
    const yesterday: typeof items = [];
    const week: typeof items = [];
    const older: typeof items = [];
    for (const s of sorted) {
      const t = sessTime(s);
      if (t === 0) { older.push(s); continue; }
      const ageDays = Math.floor((nowMs - t) / dayMs);
      if (ageDays < 1) today.push(s);
      else if (ageDays < 2) yesterday.push(s);
      else if (ageDays < 7) week.push(s);
      else older.push(s);
    }
    return [
      { label: 'Today', items: today },
      { label: 'Yesterday', items: yesterday },
      { label: 'This week', items: week },
      { label: 'Older', items: older }
    ].filter((g) => g.items.length > 0);
  }

  // Thinking-time label for the typing indicator — per-kind so Claude and
  // Cursor timers don't interfere with each other.
  let thinkingStartedAt = $state<Record<'claude' | 'cursor', number | null>>({ claude: null, cursor: null });
  let thinkingTick = $state<Record<'claude' | 'cursor', number>>({ claude: 0, cursor: 0 });
  const thinkingTimers: Record<'claude' | 'cursor', ReturnType<typeof setInterval> | null> = { claude: null, cursor: null };

  // Auto-create initial chat in the Claude app singleton when Claude
  // connects for the first time and the user has no sessions yet. App
  // singletons always exist so there's nothing to spawn.
  $effect(() => {
    if (connectedClaude && sessionsState.list.length === 0) {
      newClaudeSession({ title: 'Chat 1', agentInstanceId: APP_INSTANCE_IDS.claude });
    }
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
  // In browser preview (non-Tauri) we pretend Claude/Cursor are ready so the
  // full agent UI renders instead of the "connect first" empty card. The
  // actual invoke calls will still no-op, which is fine for visual review.
  const connectedClaude = $derived((claudeStatus?.ready ?? false) || !inTauri);
  const connectedCursor = $derived((cursorStatus?.ready ?? false) || !inTauri);
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
  /* Heartbeat that periodically re-checks every connection's status.
     Without this, a transient network blip / token rotation / sleep-
     wake leaves the cards reading whatever the boot retry settled on,
     and the user has to click "Test connection" by hand to recover.
     5 minutes is a comfortable cadence — long enough to not hammer
     the GitHub/Jira/Sentry APIs (each call uses 1 quota unit per
     5-minute window per source), short enough that a flaky-network
     recovery is visible within a coffee break. */
  let connectionRefreshInterval: ReturnType<typeof setInterval> | null = null;
  /** Last time we re-ran `refreshAllStatus` from any trigger. Used
   *  by the focus listener to coalesce — a sequence of rapid focus
   *  events (e.g. macOS Mission Control switching apps) shouldn't
   *  fire more than one refresh per minute. */
  let lastConnectionRefreshAt = 0;
  /** Listener removal fn, populated on mount + called on destroy.
   *  Stored so the cleanup path can release the listener even if the
   *  app is torn down via Tauri rather than navigation. */
  let removeFocusListener: (() => void) | null = null;

  /** Unlisten for the action-IPC event from MCP sidecars. Each
   *  woom-github `propose_*` MCP call ends up emitting this
   *  event; the handler attaches the IPC `wait_id` to the matching
   *  pending action card so its eventual resolution routes back to
   *  the sidecar via `resolve_action_wait`. */
  let actionIpcUnlisten: UnlistenFn | null = null;
  /* Window-close lifecycle handles. Both are unlisten-style — see
     the close-flush hook inside onMount for what they catch. */
  let tauriCloseUnlisten: UnlistenFn | null = null;
  let beforeUnloadHandler: (() => void) | null = null;
  let closeFlushInProgress = false;

  type ActionRequestPayload = {
    session_id: string;
    wait_id: string;
    kind: 'bash' | 'commit' | 'pr' | 'switch_cwd' | 'question';
    params: Record<string, unknown>;
  };

  /** Match an IPC `propose_*` request to a pending action card and
   *  attach its `waitId`. Two arrival orderings need to work:
   *
   *  1. Stream parser fires first (sees the agent's tool_use block,
   *     creates the card with no waitId). Then the IPC request lands
   *     here moments later — we find the card by kind+command/title
   *     and stamp the waitId on it.
   *
   *  2. IPC fires first (race; not common but possible). No matching
   *     card yet — we create one ourselves from the IPC params with
   *     the waitId already set. The stream parser later sees the
   *     same tool_use; it'll create a SECOND card by id (different
   *     id, no waitId). To avoid that double-up, the stream parser
   *     also checks for an existing waitId-marked card with matching
   *     params and skips creation if one exists. (See agentStream.ts.)
   */
  /** Sentinel session id written by `cursor_mcp::sync` into the env
   *  block of every woom-* server in `~/.cursor/mcp.json`. The file is
   *  global so we can't bake a per-session id — instead we route any
   *  card carrying the sentinel to the currently-focused Cursor chat.
   *  Single-Cursor flows (the common case) get correct routing; parallel
   *  Cursor chats all post to whichever is focused. Keep in lock-step
   *  with `CURSOR_SENTINEL_SESSION_ID` in `cursor_mcp.rs`. */
  const CURSOR_SENTINEL_SESSION_ID = 'cursor';

  function resolveCursorSentinelSession(): string | null {
    /* First choice: the chat the user actively has open in the Cursor
       solo. Falls back to the most-recently-updated Cursor session so
       a freshly-bundled Cursor instance (no pin yet) still routes. */
    const pinned = sessionsState.activeByInstance[APP_INSTANCE_IDS.cursor];
    if (pinned) {
      const found = sessionsState.list.find((s) => s.id === pinned && s.agentKind === 'cursor');
      if (found) return found.id;
    }
    const cursorSessions = sessionsState.list.filter((s) => s.agentKind === 'cursor');
    if (!cursorSessions.length) return null;
    const sessTime = (s: typeof cursorSessions[number]) => {
      const last = s.messages[s.messages.length - 1]?.at;
      return last ? new Date(last).getTime() : 0;
    };
    cursorSessions.sort((a, b) => sessTime(b) - sessTime(a));
    return cursorSessions[0].id;
  }

  function handleActionRequest(payload: ActionRequestPayload) {
    /* Cursor's mcp.json is global, so its sidecars all stamp the same
       sentinel session id. Resolve it to the right live Cursor chat
       before the regular lookup runs. */
    if (payload.session_id === CURSOR_SENTINEL_SESSION_ID) {
      const resolved = resolveCursorSentinelSession();
      if (!resolved) return;
      payload = { ...payload, session_id: resolved };
    }
    const sess = sessionsState.list.find((s) => s.id === payload.session_id);
    if (!sess) return;
    const matching = sess.actions.find(
      (a) =>
        a.status === 'pending' && !a.waitId && actionMatchesIpcParams(a, payload.kind, payload.params)
    );
    if (matching) {
      updateAction(payload.session_id, matching.id, { waitId: payload.wait_id });
      return;
    }
    // No stream-parser-created card yet (or none matchable). Create
    // one from the IPC params directly; the stream parser will skip
    // its duplicate creation when it sees the matching waitId.
    const fresh = buildActionFromIpcRequest(payload);
    if (fresh) addAction(payload.session_id, fresh);
  }

  function actionMatchesIpcParams(
    a: ClaudeAction,
    kind: ActionRequestPayload['kind'],
    params: Record<string, unknown>
  ): boolean {
    if (a.kind !== kind) return false;
    if (kind === 'bash')
      return a.kind === 'bash' && a.command === String(params.command ?? '');
    if (kind === 'commit')
      return a.kind === 'commit' && a.message === String(params.message ?? '');
    if (kind === 'pr')
      return a.kind === 'pr' && a.title === String(params.title ?? '');
    if (kind === 'switch_cwd')
      return a.kind === 'switch_cwd' && a.path === String(params.path ?? '');
    return false;
  }

  function buildActionFromIpcRequest(p: ActionRequestPayload): ClaudeAction | null {
    const id = (typeof crypto !== 'undefined' && crypto.randomUUID)
      ? crypto.randomUUID()
      : `act-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
    const px = p.params;
    if (p.kind === 'bash') {
      return {
        id,
        kind: 'bash',
        command: String(px.command ?? ''),
        reason: typeof px.reason === 'string' ? px.reason : '',
        status: 'pending',
        waitId: p.wait_id
      };
    }
    if (p.kind === 'commit') {
      return {
        id,
        kind: 'commit',
        message: String(px.message ?? ''),
        body: typeof px.body === 'string' ? px.body : '',
        push: px.push !== false,
        note: typeof px.note === 'string' ? px.note : '',
        status: 'pending',
        waitId: p.wait_id
      };
    }
    if (p.kind === 'pr') {
      return {
        id,
        kind: 'pr',
        title: String(px.title ?? ''),
        body: typeof px.body === 'string' ? px.body : '',
        base: typeof px.base === 'string' ? px.base : '',
        draft: px.draft === true,
        note: typeof px.note === 'string' ? px.note : '',
        status: 'pending',
        waitId: p.wait_id
      };
    }
    if (p.kind === 'switch_cwd') {
      return {
        id,
        kind: 'switch_cwd',
        path: String(px.path ?? ''),
        reason: typeof px.reason === 'string' ? px.reason : '',
        status: 'pending',
        waitId: p.wait_id
      };
    }
    if (p.kind === 'question') {
      const opts = Array.isArray(px.options) ? px.options : [];
      return {
        id,
        kind: 'question',
        question: String(px.question ?? ''),
        header: typeof px.header === 'string' ? px.header : undefined,
        options: opts
          .map((o) => (typeof o === 'object' && o !== null
            ? { label: String((o as Record<string, unknown>).label ?? ''),
                description: typeof (o as Record<string, unknown>).description === 'string'
                  ? String((o as Record<string, unknown>).description)
                  : undefined }
            : { label: String(o) }))
          .filter((o) => o.label.length > 0),
        multiSelect: px.multi_select === true,
        status: 'pending',
        waitId: p.wait_id
      };
    }
    return null;
  }

  // Wire the layout→sessions hook once. If a singleton's data is ever
  // explicitly cleared, its pinned sessions float back to the pool so
  // they reattach elsewhere instead of vanishing.
  registerInstanceRemovedHook((id) => {
    orphanSessionsForInstance(id);
    /* Canvas owns per-instance tab state (which canvases are pinned,
       which one is active). Clearing it drops that map entry — but does
       NOT delete the canvases themselves; they stay in the library. */
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
    // Install a window-level dragend/drop listener that clears the
    // drag payload on any cancel — defense in depth against future
    // drag sources forgetting their own ondragend wiring.
    installGlobalDragSafetyNet();
    // Background-task store — wires the global `bg:tasks-changed`
    // listener so the Preview pane (right side of Claude/Cursor solo)
    // refreshes when a process spawns / exits anywhere in the app.
    void initBgTasks();
    void initSdd();
    // Hooks — load the user's `hooks.json` config so the lifecycle
    // call sites (UserPromptSubmit / Stop / SessionStart later) can
    // dispatch without an IPC stall on the first invocation.
    void loadHookConfig();
    // Auto-memory — pull the latest `user` + `feedback` entries from
    // the local SQLite store so the agent's system prompt suffix
    // includes them on the very first turn (no cold-start gap).
    void refreshAutoMemory();
    /* Skills — install bundled SKILL.md defaults into ~/.claude/skills
     * on first launch. Idempotent: never overwrites user edits. */
    void invoke('skills_install_bundled_defaults').catch(() => {});
    /* Statusline — load config + install refresh timer. The timer
       calls `buildStatusLinePayload` against the active session so
       script output reflects the currently-focused chat. */
    void (async () => {
      await loadStatusLineConfig();
      installStatusLineTimer(() => buildStatusLinePayload());
      const initial = buildStatusLinePayload();
      if (initial) void runStatusLine(initial);
    })();
    /* Loop-tick fire — `loops.svelte.ts` schedules recurring sends via
     * a setInterval; when one fires for an idle session it stamps the
     * input + dispatches this event. We re-enter the normal send path
     * so the loop's prompt goes through agentContext + mentions / cwd
     * recap exactly like a manually-typed message. */
    window.addEventListener('woom:loop-fire', (e: Event) => {
      const evt = e as CustomEvent<{ sessionId: string }>;
      const sid = evt.detail?.sessionId;
      if (!sid) return;
      const target = sessionsState.list.find((s) => s.id === sid);
      if (!target) return;
      const prev = sessionsState.activeIds[target.agentKind];
      if (prev !== sid) {
        sessionsState.activeIds[target.agentKind] = sid;
      }
      void sendClaudeMessage();
    });
    // Subscribe to action-IPC requests from MCP sidecars. Each event
    // fires when woom-github's `propose_*` MCP tool wants the
    // user to approve a card. We attach the IPC `wait_id` to the
    // matching pending action card (or create one if the stream
    // parser hasn't yet) so when the card resolves later in
    // executeAction, we can route the outcome back to the sidecar
    // via `resolve_action_wait` — and the agent receives it as the
    // tool_result IN THE SAME TURN (no end-turn / next-turn drain).
    actionIpcUnlisten = await listen<ActionRequestPayload>(
      'woom:action_request',
      (event) => handleActionRequest(event.payload)
    );
    /* Window close / dev reload safety net.
     *
     * The session persister debounces writes (see scheduleDiskWrite
     * in sessions.svelte.ts), which means a force-quit shortly
     * after a message lands — or in the middle of a streaming
     * answer — would drop the unflushed delta. We hook two
     * complementary lifecycle events:
     *
     *   1. Tauri's onCloseRequested — preventDefault, await an
     *      immediate flush, then explicitly destroy() the window.
     *      This catches normal quit (⌘Q) and red-button close.
     *   2. Browser beforeunload — best-effort sync flush trigger
     *      for dev-server HMR reloads where Tauri's hook doesn't
     *      run. We can't await async work here, but we can fire
     *      the immediate write request; modern Tauri/IPC tends
     *      to drain in-flight invoke calls before tear-down. */
    try {
      const { getCurrentWindow } = await import('@tauri-apps/api/window');
      const win = getCurrentWindow();
      const closeUnlisten = await win.onCloseRequested(async (event) => {
        /* Intercept-flush-destroy. preventDefault stops the natural
           close, we flush sessions, then `destroy()` tears the
           window down without re-firing onCloseRequested (`close()`
           would recurse). Both `allow-close` and `allow-destroy`
           are listed in capabilities/default.json — without them
           Tauri v2 silently refuses to close from JS and the
           window hangs in "intercepted" state. */
        if (closeFlushInProgress) return;
        closeFlushInProgress = true;
        event.preventDefault();
        /* Hard cap on the flush so a stuck disk write can never
           strand the user in a "won't quit" window. 2.5s is
           generous — `flushToDisk` is parallel Promise.all over
           N session files, in practice well under 100ms. */
        const flushDeadline = new Promise<void>((resolve) =>
          setTimeout(resolve, 2_500)
        );
        try {
          await Promise.race([flushSessionsNow(), flushDeadline]);
        } catch { /* best effort */ }
        try {
          await win.destroy();
        } catch {
          /* destroy() failed — fall back to close() so we don't
             leave the user stuck with a "won't quit" window. The
             flush already ran so persistence is safe either way. */
          try { await win.close(); } catch { /* runtime tearing down */ }
        }
      });
      tauriCloseUnlisten = closeUnlisten;
    } catch {
      /* Non-Tauri context (rare — only when /+page runs outside the
         shell, e.g. unit tests with jsdom). Fall through to the
         beforeunload listener which still gives best-effort cover. */
    }
    if (typeof window !== 'undefined') {
      const unloadHandler = () => { void flushSessionsNow(); };
      window.addEventListener('beforeunload', unloadHandler);
      beforeUnloadHandler = unloadHandler;
    }
    // Plan-usage snapshot for the chip. Fire-and-forget — the chip
    // shows "—" until the first response comes back, after which
    // refreshPlanUsage is debounced to MIN_REFRESH_MS (60s) so any
    // post-turn re-fetch is effectively free.
    void refreshPlanUsage();
    // Same boot-time pattern as theme: apply the saved zoom level to
    // <html> before first paint so the layout doesn't briefly flash
    // at 100% then jump.
    initScale();
    initDensity();
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
       `woom:canvas:v1:*` localStorage keys to free origin
       quota. Failures fall back to localStorage transparently. */
    await initCanvasFromDisk(appDataDir);
    // One-shot v1 → v2 migration: seed the legacy `woom:editor:root`
    // localStorage value into the first editor instance, ONLY if that
    // instance has no persisted v2 state yet. Without this guard the
    // migration would clobber the editor's open folder on every reload
    // (v2 persistence already restored the right path; the legacy key
    // would re-overwrite it back to the original v1 value).
    try {
      const savedEditorRoot = localStorage.getItem('woom:editor:root');
      if (savedEditorRoot) {
        const edId = APP_INSTANCE_IDS.editor;
        const alreadyHasV2 = !!sessionsState.editorInstanceState[edId]?.repoPath;
        if (!alreadyHasV2) {
          setEditorRepoPath(savedEditorRoot, edId);
        }
        // Drop the legacy key once we know v2 is in place — keeps
        // re-mounts cheap and prevents future regressions of this kind.
        if (alreadyHasV2) localStorage.removeItem('woom:editor:root');
      }
    } catch {/* ignore */}
    tickInterval = setInterval(() => (now = Date.now()), 30_000);
    // Periodic connection health-check. The boot retry only runs once
    // and gives up after ~22 s + per-attempt invoke timeouts; if the
    // user's network was down then or a token rotated since, the cards
    // would stay stuck on the boot result until manual "Test
    // connection". This heartbeat picks up automatically. We coalesce
    // with `lastConnectionRefreshAt` so the focus listener below doesn't
    // double-fire when the user app-switches mid-cadence.
    connectionRefreshInterval = setInterval(() => {
      if (appLocked) return;
      lastConnectionRefreshAt = Date.now();
      void refreshAllStatus();
    }, 5 * 60 * 1000);
    // Refresh on window focus too — the most common reason a user
    // notices a stale connection is when they come back to the app
    // after a meeting / sleep-wake. 60 s coalesce window so rapid
    // app-switching doesn't burn API quota.
    const focusHandler = () => {
      if (appLocked) return;
      if (Date.now() - lastConnectionRefreshAt < 60_000) return;
      lastConnectionRefreshAt = Date.now();
      void refreshAllStatus();
    };
    window.addEventListener('focus', focusHandler);
    removeFocusListener = () => window.removeEventListener('focus', focusHandler);
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
    if (connectionRefreshInterval) clearInterval(connectionRefreshInterval);
    removeFocusListener?.();
    actionIpcUnlisten?.();
    tauriCloseUnlisten?.();
    if (beforeUnloadHandler && typeof window !== 'undefined') {
      window.removeEventListener('beforeunload', beforeUnloadHandler);
      beforeUnloadHandler = null;
    }
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
      /* `chat-message` payloads are dragstart-handled inside AgentApp
         itself (which sets dragState directly). This `+page.svelte`
         path is for the inbox / file-tree drags; chat messages don't
         flow through `onDragStart` here. */
    }
  }

  function onDragEnd() {
    setDragPayload(null);
    clearAgentDragState();
    justDragged = true;
    setTimeout(() => (justDragged = false), 120);
  }

  /** Returns true if `e` carries a payload an agent column can accept —
   *  internal drag (ticket / file-tree row) or OS file drop. Used by
   *  dragenter, dragover, and drop alike so all three agree on accept. */
  function agentCanAccept(e: DragEvent): boolean {
    const types = e.dataTransfer?.types;
    if (dragState.payload) return true;
    if (types?.includes('application/x-woom-file')) return true;
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
    // Prefer `activeIds[kind]` since that's what ChatThread renders;
    // `activeByInstance[instanceId]` only updates when the focused
    // session has an `agentInstanceId`, which leaves floating sessions
    // out of sync.
    const activeId =
      sessionsState.activeIds[kind] ?? sessionsState.activeByInstance[instanceId];
    let target = activeId ? sessionsState.list.find((s) => s.id === activeId) ?? null : null;
    if (!target) target = sessionsState.list.find((s) => s.agentInstanceId === instanceId) ?? null;
    if (!target) {
      const id = newClaudeSession({ agentKind: kind, agentInstanceId: instanceId });
      target = sessionsState.list.find((s) => s.id === id) ?? null;
    }
    if (!target) return 0;
    const n = await attachBlobsToSession(target.id, blobs);
    if (n > 0) setActiveSessionInInstance(instanceId, target.id);
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
    //
    // Prefer `activeIds[kind]` over `activeByInstance[instanceId]` —
    // ChatThread reads the former, and `focusSession` only writes the
    // latter for sessions that already have an `agentInstanceId`. A
    // floating chat (agentInstanceId === null) appears in the column
    // and its messages render correctly, but the per-instance pointer
    // stays on whatever was previously bound — so drops landed in the
    // wrong session. Reading activeIds[kind] keeps drop target ==
    // visible chat regardless of binding state.
    const pickTarget = (): ClaudeSession | null => {
      const activeId =
        sessionsState.activeIds[kind] ?? sessionsState.activeByInstance[instanceId];
      let t = activeId ? sessionsState.list.find((s) => s.id === activeId) ?? null : null;
      if (!t) t = sessionsState.list.find((s) => s.agentInstanceId === instanceId) ?? null;
      if (!t) {
        // Adopt a floating session of the same kind if one exists.
        t = sessionsState.list.find(
          (s) => s.agentKind === kind && s.agentInstanceId === null
        ) ?? null;
        if (t) updateSession(t.id, { agentInstanceId: instanceId });
      }
      if (!t) {
        const id = newClaudeSession({ agentKind: kind, agentInstanceId: instanceId });
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
      const raw = e.dataTransfer?.getData('application/x-woom-file');
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
        setActiveSessionInInstance(instanceId, target.id);
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
          if (n > 0) setActiveSessionInInstance(instanceId, target.id);
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
          if (n > 0) setActiveSessionInInstance(instanceId, target.id);
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
      setActiveSessionInInstance(instanceId, target.id);
    }

    clearAgentDragState();
    setDragPayload(null);
    justDragged = true;
    setTimeout(() => (justDragged = false), 200);
  }

  /** Build a Mention from an inbox payload. Mirrors the shape the
   *  drag→drop pipeline produces in `onAgentDrop`, so the click-driven
   *  "Send to agent" buttons attach the exact same context the user
   *  would get by dragging the row. */
  function mentionFromInboxPayload(
    payload:
      | { kind: 'github'; item: InboxItem }
      | { kind: 'jira'; item: JiraItem }
      | { kind: 'sentry'; item: SentryIssue }
  ): Mention {
    if (payload.kind === 'github') {
      return {
        source: 'github',
        externalId: externalId(payload.item),
        title: payload.item.title,
        body: payload.item.body
      };
    }
    if (payload.kind === 'jira') {
      return {
        source: 'jira',
        externalId: payload.item.key,
        title: payload.item.summary,
        body: payload.item.description
      };
    }
    const issue = payload.item;
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
    return { source: 'sentry', externalId: ref, title: issue.title, body: summary };
  }

  /** Quick-send a free-form prompt into a specific session without
   *  the user having to switch view. Used by InlineClaude's per-row
   *  composer in the editor's right pane: write a question, click
   *  Send, response streams in the Claude app — user can switch over
   *  later to read it. If the session is currently mid-turn, the
   *  message goes into its `pendingQueue` and auto-fires when the
   *  current turn finishes. The active-session pointer flips to the
   *  target so `sendClaudeMessage` (which reads `activeSession`)
   *  picks it up; the rail view stays put — user is still in
   *  whatever app they were on. */
  function quickSendToSession(sessionId: string, text: string) {
    const trimmed = text.trim();
    if (!trimmed) return;
    const s = sessionsState.list.find((x) => x.id === sessionId);
    if (!s) return;
    if (s.sending) {
      const next = [...(s.pendingQueue ?? []), { text: trimmed, mentions: [] }];
      updateSession(sessionId, { pendingQueue: next });
      return;
    }
    /* Idle — flip activeClaudeId so sendClaudeMessage targets this
       session, set the input, fire. We do NOT change `view`, so the
       user stays in the editor / wherever they were. */
    sessionsState.activeClaudeId = sessionId;
    sessionsState.activeIds[s.agentKind] = sessionId;
    setSessionInput(sessionId, trimmed);
    void sendClaudeMessage();
  }

  /** Click-driven equivalent of the drag→drop pipeline for inbox
   *  items (GitHub PR / Jira ticket / Sentry issue). Used by every
   *  "Send to agent" affordance — per-row chips on each list, action
   *  buttons in detail panes, etc. Picks/creates a session in the
   *  agent column, splices `@<externalId>` into the input, and
   *  switches the top-level view so the user lands on the chat
   *  ready to type. */
  function sendInboxItemToAgent(
    payload:
      | { kind: 'github'; item: InboxItem }
      | { kind: 'jira'; item: JiraItem }
      | { kind: 'sentry'; item: SentryIssue },
    kind: 'claude' | 'cursor' = 'claude'
  ) {
    const instanceId = kind === 'claude' ? APP_INSTANCE_IDS.claude : APP_INSTANCE_IDS.cursor;
    const mention = mentionFromInboxPayload(payload);
    const activeId = sessionsState.activeByInstance[instanceId];
    let target = activeId ? sessionsState.list.find((s) => s.id === activeId) ?? null : null;
    if (!target) target = sessionsState.list.find((s) => s.agentInstanceId === instanceId) ?? null;
    if (!target) {
      target = sessionsState.list.find(
        (s) => s.agentKind === kind && s.agentInstanceId === null
      ) ?? null;
      if (target) updateSession(target.id, { agentInstanceId: instanceId });
    }
    if (!target) {
      const id = newClaudeSession({ agentKind: kind, agentInstanceId: instanceId });
      target = sessionsState.list.find((s) => s.id === id) ?? null;
    }
    if (!target) return;
    const dedup = target.mentions.filter(
      (m) => !(m.source === mention.source && m.externalId === mention.externalId)
    );
    const tokenAlreadyInInput = new RegExp(
      `(?:^|\\s)@${mention.externalId.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}(?:\\s|$)`
    ).test(target.input);
    const sep = target.input && !target.input.endsWith(' ') ? ' ' : '';
    updateSession(target.id, {
      input: tokenAlreadyInInput ? target.input : target.input + sep + `@${mention.externalId} `,
      mentions: [...dedup, mention]
    });
    setActiveSessionInInstance(instanceId, target.id);
    view = kind === 'claude' ? 'claudeApp' : 'cursorApp';
  }

  /** If user drops a file before setting cwd, infer the enclosing directory. */
  function deriveCwd(path: string, isDir: boolean): string | null {
    if (isDir) return path;
    const idx = path.lastIndexOf('/');
    return idx > 0 ? path.slice(0, idx) : null;
  }

  /** Builds the JSON payload piped to the user's statusline script.
   *  Reads from the currently-active session (across Claude / Cursor).
   *  Returns null if no session is active — caller skips the run. */
  function buildStatusLinePayload(): StatusLinePayload | null {
    const cur = activeSession;
    if (!cur) return null;
    const kind = (cur.agentKind ?? 'claude') as 'claude' | 'cursor';
    const model = kind === 'cursor' ? cur.cursorModel ?? null : cur.claudeModel ?? null;
    const window = contextWindowFor(cur.claudeModel, kind);
    const used = cur.lastContextSize ?? 0;
    /* Cumulative cost summed across every assistant turn's usage
     *  snapshot. costForUsage handles per-model rates. */
    let costUsd = 0;
    for (const m of cur.messages) {
      if (m.role === 'assistant' && m.usage) costUsd += costForUsage(m.usage);
    }
    return {
      model: { id: model, display_name: model },
      cwd: cur.cwd ?? null,
      session_id: cur.id,
      session_title: cur.title ?? 'Untitled chat',
      agent_kind: kind,
      permission_mode: cur.permissionMode ?? 'default',
      cost_usd: costUsd,
      context_window: {
        used_percentage: window > 0 ? Math.round((used / window) * 100) : 0,
        size: window
      },
      worktree: {
        path: cur.worktreePath ?? null,
        branch: cur.worktreeBranch ?? null
      }
    };
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
        // The cwd swap rotated the CLI session uuid, so any prewarmed
        // CLI for this session is now stale. The backend would respawn
        // on signature mismatch anyway, but dropping early avoids
        // burning a CLI process while the new prewarm fires.
        void dropPrewarm(activeSession.id);
      }
    } catch (e) {
      notifyError(e, { title: "Couldn't pick folder" });
    }
  }

  function clearCwd() {
    if (!activeSession) return;
    applySessionCwd(activeSession.id, null, { breakLink: true });
    void dropPrewarm(activeSession.id);
  }

  /** Wrapper around `deleteClaudeSession` that also drops any
   *  prewarmed CLI parked for this session — otherwise the
   *  pre-spawn lingers (holding a `claude` process + MCP sidecars)
   *  until the TTL sweeper picks it up. Also stops any in-flight
   *  agent run for this session and clears the auto-continuation
   *  guard set so a future session id reuse (vanishingly unlikely
   *  given uuid randomness, but still) wouldn't silently swallow
   *  the next continuation. */
  function deleteClaudeSessionWithCleanup(sessionId: string) {
    void dropPrewarm(sessionId);
    // If a turn is mid-stream for this session, stop it before we
    // pull the chat out from under it. Failures here are non-fatal —
    // the worst case is a CLI process that gets reaped by the TTL
    // sweeper a few minutes later.
    void stopAgentRequest(sessionId).catch(() => {});
    continuationInFlight.delete(sessionId);
    deleteClaudeSession(sessionId);
  }

  /** Link the active chat session to an editor instance WITHOUT touching
      either side's folder. If the agent's cwd and the editor's repoPath
      diverge, the WorktreeBar shows an orange mismatch button — the user
      explicitly chooses which side wins. This avoids the old "auto-push"
      surprise where picking an editor silently overwrote either side.
      When the agent has no folder of its own (first link, no worktree),
      we still pull the editor's folder in so the session has something
      to work with. */
  function linkActiveSessionToEditor(editorInstanceId: string) {
    if (!activeSession) return;
    const editorPath =
      sessionsState.editorInstanceState[editorInstanceId]?.repoPath ?? '';
    const hasOwnFolder = !!(activeSession.worktreePath || activeSession.cwd);
    const patch: Partial<typeof activeSession> = {
      linkedToEditor: true,
      linkedToEditorInstanceId: editorInstanceId
    };
    // First-time link with no agent cwd yet → adopt the editor's folder
    // (zero-config UX). Otherwise leave both sides alone; mismatch UI
    // surfaces a deliberate sync choice.
    if (!hasOwnFolder && editorPath) {
      patch.cwd = editorPath;
    }
    updateSession(activeSession.id, patch);
  }

  /** Sync the active session's cwd to its linked editor's repoPath.
      Wired to the "Use editor folder" choice in WorktreeBar's mismatch
      menu. Uses `applySessionCwd` (not raw updateSession) so the CLI
      uuid rotates and the next turn's prompt gets a cwd-switch recap. */
  function syncAgentToLinkedEditor() {
    if (!activeSession?.linkedToEditorInstanceId) return;
    const editorPath =
      sessionsState.editorInstanceState[activeSession.linkedToEditorInstanceId]?.repoPath ?? '';
    if (!editorPath) return;
    applySessionCwd(activeSession.id, editorPath);
    void dropPrewarm(activeSession.id);
  }

  /** Sync the linked editor's repoPath to the active session's cwd /
      worktree. Wired to the "Use agent folder" choice in WorktreeBar's
      mismatch menu. */
  function syncLinkedEditorToAgent() {
    if (!activeSession?.linkedToEditorInstanceId) return;
    const agentPath = activeSession.worktreePath || activeSession.cwd || '';
    if (!agentPath) return;
    setEditorRepoPath(agentPath, activeSession.linkedToEditorInstanceId);
  }

  function toggleSessionEditorLink() {
    if (!activeSession) return;
    if (activeSession.linkedToEditor) {
      updateSession(activeSession.id, {
        linkedToEditor: false,
        linkedToEditorInstanceId: null
      });
    } else {
      /* Link to whichever editor instance the rail currently has
         active (default = primary). Multi-instance aware. */
      linkActiveSessionToEditor(layoutState.activeInstance.editor);
    }
  }

  /** Bind the active session to a specific terminal instance from the
   *  cwd-bar's "Link terminal…" picker. Thin wrapper around
   *  `linkSessionToTerminal` that resolves the active session for the
   *  caller (the picker only knows which terminal the user picked). */
  function linkActiveSessionToTerminal(terminalInstanceId: string) {
    if (!activeSession) return;
    linkSessionToTerminal(terminalInstanceId, activeSession.id);
  }

  /** Drop the active session's terminal link. Wired to the cwd-bar's
   *  terminal chip × so the user can untap a chat without bouncing
   *  to the terminal app. */
  function toggleSessionTerminalLink() {
    if (!activeSession) return;
    if (activeSession.linkedTerminalInstanceId) {
      unlinkSessionFromTerminal(activeSession.id);
    } else {
      linkSessionToTerminal(layoutState.activeInstance.terminal, activeSession.id);
    }
  }

  /** Initiate a link from the Editor side. Always links the *currently
      active* session in the target agent column — never spawns a new chat.
      The chat's cwd just snaps to the editor's folder and the session
      becomes linked. If the column has no active session, we create one
      (empty column → there was nothing to link). */
  /** Link a chat session to this terminal instance — mirror of
   *  `linkEditorToAgent` but for the terminal side. After linking, the
   *  session's MCP `terminal_run` / `terminal_write` default to this
   *  terminal id, AND selecting text in this terminal will surface
   *  an "Apply to <agent>" chip wired to the same session. We don't
   *  touch cwd here — the terminal uses its session-derived
   *  `autoLinkedCwd` only when the chat ALSO links an editor; if the
   *  user just links a chat-to-terminal (no editor link), the
   *  terminal keeps whatever cwd was already set. */
  function linkSessionToTerminal(terminalInstanceId: string, sessionId: string) {
    const sess = sessionsState.list.find((s) => s.id === sessionId);
    if (!sess) return;
    /* Floating sessions (agentInstanceId === null) need a canonical
       agent-app id so the terminal's inline-agents pane can render
       their card AND surface "Apply to <agent>" — both consumers
       require a non-null id to resolve which app to route into. We
       use the singleton app id for the session's kind, which matches
       what `setActiveSessionInInstance` would set on first focus. */
    const patch: Partial<typeof sess> = { linkedTerminalInstanceId: terminalInstanceId };
    if (!sess.agentInstanceId) patch.agentInstanceId = APP_INSTANCE_IDS[sess.agentKind];
    updateSession(sessionId, patch);
    /* Eager-spawn the PTY so the agent's `mcp__app__terminal_*` calls
       hit a live session immediately — previously the PTY only spawned
       on first surface mount, so an agent linked through the cwd-bar
       (without the user opening the Terminal solo) saw an empty
       `terminal_list` and bounced off. Picks up the editor's repoPath
       when the session is also editor-linked; otherwise inherits the
       layout's last-used cwd / $HOME. Idempotent — second call is a
       no-op when the session already exists. */
    const layoutName =
      layoutState.instances.terminal.find((i) => i.id === terminalInstanceId)?.name ?? null;
    const editorCwd =
      sess.linkedToEditor && sess.linkedToEditorInstanceId
        ? sessionsState.editorInstanceState[sess.linkedToEditorInstanceId]?.repoPath ?? null
        : null;
    const spawnCwd = editorCwd ?? layoutState.active.terminal.cwd ?? null;
    void ensureTerminalSession(terminalInstanceId, spawnCwd, 120, 32, layoutName);
  }

  function unlinkSessionFromTerminal(sessionId: string) {
    updateSession(sessionId, { linkedTerminalInstanceId: null });
  }

  function linkActiveSessionToCanvas(canvasId: string) {
    if (!activeSession) return;
    updateSession(activeSession.id, { linkedCanvasId: canvasId });
  }

  function toggleSessionCanvasLink() {
    if (!activeSession) return;
    updateSession(activeSession.id, { linkedCanvasId: null });
  }

  function linkEditorToAgent(
    editorInstanceId: string,
    agentInstanceId: string,
    sessionId?: string
  ) {
    const editorPath = sessionsState.editorInstanceState[editorInstanceId]?.repoPath || '';
    if (!editorPath) return;
    const kind = kindForInstanceId(agentInstanceId);
    if (kind !== 'claude' && kind !== 'cursor') return;
    /* If the picker passed a specific session id, link that one (and
       activate it in its column so the editor's chat header switches
       to match). Otherwise fall back to whatever session is already
       active in the agent column — original behaviour. */
    const explicit = sessionId
      ? sessionsState.list.find((s) => s.id === sessionId) ?? null
      : null;
    // The session's folder must NOT change automatically. Only when
    // the session has no folder of its own (first-link) do we pull
    // editorPath in — otherwise we leave the divergence in place
    // and let WorktreeBar surface the mismatch chip.
    function patchForLink(sess: { worktreePath?: string | null; cwd?: string | null }) {
      const hasOwn = !!(sess.worktreePath || sess.cwd);
      const base = {
        linkedToEditor: true,
        linkedToEditorInstanceId: editorInstanceId,
        agentInstanceId
      } as const;
      return hasOwn ? base : { ...base, cwd: editorPath };
    }
    if (explicit) {
      setActiveSessionInInstance(agentInstanceId, explicit.id);
      updateSession(explicit.id, patchForLink(explicit));
      return;
    }
    const currentId = sessionsState.activeByInstance[agentInstanceId] ?? null;
    const current = currentId
      ? sessionsState.list.find((s) => s.id === currentId) ?? null
      : null;
    if (current) {
      updateSession(current.id, patchForLink(current));
    } else {
      newClaudeSession({
        agentKind: kind,
        cwd: editorPath,
        linkedToEditor: true,
        linkedToEditorInstanceId: editorInstanceId,
        agentInstanceId: agentInstanceId
      });
    }
  }

  // ---- Worktree management for the active Claude session ----
  let worktreeBusy = $state<'creating' | 'removing' | null>(null);
  let worktreeMenuOpen = $state(false);

  interface WorktreeInfo {
    path: string;
    branch: string | null;
    head: string | null;
    is_main: boolean;
    woom_session: string | null;
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
      `Woom will create a fresh branch "woom/${activeSession.id.slice(0, 8)}" ` +
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
      and switch the rail to it. Returns the editor singleton id. */
  function ensureEditorShowing(path: string): string {
    const id = APP_INSTANCE_IDS.editor;
    setEditorRepoPath(path, id);
    view = 'editorApp';
    return id;
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
    ensureEditorShowing(path);
  }

  /** Spawn a fresh chat in the Claude/Cursor singleton. */
  function spawnAgentChat(kind: 'claude' | 'cursor') {
    newClaudeSession({ agentKind: kind, agentInstanceId: APP_INSTANCE_IDS[kind] });
  }

  /** Backfill `linkedToEditorInstanceId` for legacy sessions that
   *  have `linkedToEditor=true` but no instance id stored (pre
   *  multi-instance-editor data). No cwd ↔ repoPath syncing happens
   *  here any more: previously this effect rewrote a session's `cwd`
   *  to the editor's `repoPath` every time they diverged, which is
   *  exactly why the orange "Folder mismatch" chip never appeared —
   *  it lit up for one tick and the effect snuffed it. All forced
   *  syncing is now opt-in: either the user picks a side from the
   *  mismatch menu (syncAgentToLinkedEditor / syncLinkedEditorToAgent)
   *  or the MCP `set_editor_repo_path` handler does it deliberately. */
  $effect(() => {
    for (const s of sessionsState.list) {
      if (!s.linkedToEditor) continue;
      if (!s.linkedToEditorInstanceId) {
        updateSession(s.id, { linkedToEditorInstanceId: APP_INSTANCE_IDS.editor });
      }
    }
  });

  function toggleWorktreeMenu() {
    worktreeMenuOpen = !worktreeMenuOpen;
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
      `Woom will run \`git merge --no-ff ${activeSession.worktreeBranch}\` in ${activeSession.worktreeRepo} ` +
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

  function startThinkingTimer(kind: 'claude' | 'cursor') {
    thinkingStartedAt[kind] = Date.now();
    thinkingTick[kind] = 0;
    if (thinkingTimers[kind]) clearInterval(thinkingTimers[kind]!);
    thinkingTimers[kind] = setInterval(() => {
      thinkingTick[kind] += 1;
    }, 1000);
  }

  function stopThinkingTimer(kind: 'claude' | 'cursor') {
    if (thinkingTimers[kind]) {
      clearInterval(thinkingTimers[kind]!);
      thinkingTimers[kind] = null;
    }
    thinkingStartedAt[kind] = null;
  }

  // Thin wrapper around `runCompactSessionService` so the AgentApp
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
   * the AgentApp export-chip click handler. Toast confirms so
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
    // Skill dispatch FIRST — `/<skill-name> [args]`. If the leading
    // slash token matches a discovered skill name, we render its body
    // (with $ARGUMENTS + `!`-shell injection) and stamp the resolved
    // markdown as the next user message instead of routing to a
    // built-in slash. Slash and skill names share a namespace; on
    // collision a built-in wins (so a user can't accidentally shadow
    // `/help` with a SKILL.md called `help`).
    const skillMatch = /^\/([A-Za-z][\w-]*)(?:\s+([\s\S]*))?$/.exec(text.trim());
    if (skillMatch) {
      const tokenName = skillMatch[1].toLowerCase();
      const args = skillMatch[2]?.trim() ?? '';
      const isBuiltin = (KNOWN_SLASH_COMMANDS as string[]).includes(tokenName);
      if (!isBuiltin) {
        const sk = skillsState.list.find((s) => s.name.toLowerCase() === tokenName);
        if (sk) {
          setSessionInput(session.id, '');
          const cwd = session.worktreePath ?? session.cwd ?? null;
          const rendered = await renderSkill(sk.id, args, cwd);
          if (!rendered) {
            appendSessionMessage(session.id, {
              role: 'assistant',
              content: `_Skill \`${sk.name}\` failed to render — check the file at \`${sk.path}\`._`,
              at: new Date().toISOString()
            });
            return true;
          }
          /* Stamp the rendered body into the composer + re-fire the
           *  normal send pipeline. The rendered text is what the agent
           *  sees; no further substitution happens. */
          updateSession(session.id, { input: rendered.rendered });
          /* Defer a microtask so the input update flushes before the
           *  recursive send picks it up. */
          await Promise.resolve();
          await sendClaudeMessage();
          return true;
        }
      }
    }
    // Args-bearing commands first — `/preview pnpm dev`, `/kill ID`.
    const withArgs = parseSlashCommandWithArgs(text);
    if (withArgs) {
      setSessionInput(session.id, '');
      if (withArgs.name === 'preview') {
        await spawnPreviewFromSlash(session, withArgs.args);
        void scrollChatBottom();
      } else if (withArgs.name === 'kill') {
        await killTaskFromSlash(session, withArgs.args);
        void scrollChatBottom();
      } else if (withArgs.name === 'loop') {
        await startLoopFromSlash(session, withArgs.args);
        void scrollChatBottom();
      } else if (withArgs.name === 'sdd') {
        /* /sdd <prompt> — create temp workspace + stamp the canonical
         *  spec-writer prompt into the composer input, then recursively
         *  fire `sendClaudeMessage()` so the agent picks it up via the
         *  same path as a manual user message (hooks, history, etc.).
         *  Identical to the skill-dispatch flow above. */
        const { startSddFromSlash } = await import('$lib/services/slashCommands');
        const rendered = await startSddFromSlash(session, withArgs.args);
        if (rendered) {
          updateSession(session.id, { input: rendered });
          await Promise.resolve();
          await sendClaudeMessage();
        }
        void scrollChatBottom();
      }
      return true;
    }
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
    } else if (cmd === 'ps') {
      appendBgTaskList(session);
      void scrollChatBottom();
    } else if (cmd === 'unloop') {
      await stopLoopFromSlash(session);
      void scrollChatBottom();
    } else if (cmd === 'preview') {
      /* `/preview` with no args — just open the pane. The Composer
       * inside PreviewPane handles spawn. We rely on the AgentApp's
       * own `previewOpen` localStorage flag flipping by the time the
       * user gets here, but since this dispatch is at +page level we
       * can't directly poke that. Instead, fire a custom DOM event
       * the AgentApp listens for. */
      try {
        window.dispatchEvent(new CustomEvent('woom:open-preview', {
          detail: { kind: session.agentKind }
        }));
      } catch { /* noop */ }
    }
    return true;
  }

  /** When the CLI orphans the resume target uuid, we mint a new uuid
   *  and prime the next turn's system prompt with a recap drawn from
   *  Woom's own in-memory transcript — the chat history the CLI
   *  has just lost. We piggyback on `cwdSwitchRecap` because the
   *  injection mechanism already exists (the field is read by
   *  `agentContext.ts` and stamped into `appendSystemPrompt`); the
   *  one-shot semantics are identical (consumed and cleared after
   *  next successful turn). The wording is tailored for the orphan
   *  case so the agent understands why memory feels fresh.
   *
   *  Mirrors `buildCwdSwitchRecap`'s "/compact summary + verbatim
   *  recent" structure: if the user has ever run /compact in this
   *  chat, fold its summary in as the older-context layer so we
   *  don't strand pre-compact decisions just because the CLI lost
   *  its store. */

  // Saved drafts while queue drains. Keyed by session id.
  // Set on first drain, cleared when the queue empties so the user's
  // in-progress text survives the queue firing.
  const queueSavedDrafts = new Map<string, { text: string; mentions: Mention[] }>();

  /** SDD card "Approve & continue" / "Next phase" click handler.
   *  Stamps the next-stage prompt into the composer + recursively fires
   *  the normal send pipeline — identical flow to the skill-dispatch
   *  path and the `/sdd` slash command kickoff. */
  async function onSddAdvance(sessionId: string, prompt: string): Promise<void> {
    const s = sessionsState.list.find((x) => x.id === sessionId);
    if (!s) return;
    updateSession(sessionId, { input: prompt });
    await Promise.resolve();
    await sendClaudeMessage();
  }

  async function sendClaudeMessage() {
    const s = activeSession;
    if (!s) return;
    /* Queue while a turn is in flight: instead of dropping the click,
       push the current input to `pendingQueue` and clear the composer.
       The drain in the `finally` block at end-of-turn pops the next
       queued message and re-fires sendClaudeMessage automatically.
       Mentions can't be queued reliably (they're snapshot at send-
       time and a queued turn wouldn't get a fair share of file
       refs / images), so we only queue plain-text drafts. If there
       are pending mentions we still queue but warn — the mentions
       stay attached for now and the next free moment uses them. */
    if (s.sending) {
      const draft = s.input.trim();
      if (!draft && s.mentions.length === 0) return;
      const entry = { text: draft, mentions: [...s.mentions] };
      const nextQueue = [...(s.pendingQueue ?? []), entry];
      updateSession(s.id, { input: '', mentions: [], pendingQueue: nextQueue });
      return;
    }
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
    const kind = (s.agentKind ?? 'claude') as 'claude' | 'cursor';
    /* UserPromptSubmit hook — runs *before* the message is stamped
       on the thread and sent upstream. Two outcomes we honor today:
         - `blocked: true` → abort the send; surface feedback as a
           muted system message so the user knows why.
         - any other → continue. (Phase 2.1 wires `updated_prompt`
           rewrite — would need to thread the rewritten text through
           the mention-baker and prompt assembler below.) */
    try {
      const hookOut = await runHook('UserPromptSubmit', {
        session_id: id,
        agent_kind: kind,
        prompt: text,
        cwd: s.cwd ?? null,
        worktree_path: s.worktreePath ?? null
      });
      if (hookOut.blocked) {
        appendSessionMessage(id, {
          role: 'assistant',
          content: `_Blocked by hook: ${hookOut.feedback.join('; ') || '(no reason given)'}_`,
          at: new Date().toISOString()
        });
        return;
      }
    } catch (err) {
      /* Hook IPC failed — log + continue. Better to send the message
         than to silently swallow it on a config glitch. */
      console.warn('UserPromptSubmit hook failed', err);
    }
    // Snapshot the mentions BEFORE clearing so we can still bake them into
    // the prompt below + stamp the image refs onto the user-message bubble
    // so the transcript still shows what was sent after the strip clears.
    const mentionsSnapshotPre = s.mentions;
    const imageMentions = mentionsSnapshotPre.filter(
      (m) => m.source === 'file' && !m.isDir && !!m.body && isImagePath(m.body)
    );
    const userImages = imageMentions.map((m) => ({ path: m.body!, name: m.title }));
    /* Only image attachments live on the user-message bubble. @-style
       mentions (Jira/GH/Sentry/file refs/chat) are fed into the prompt
       text below — they aren't "attachments" and shouldn't render as
       chips on the bubble. The user typed `@foo` inline; that's their
       reference, no extra surface needed. */
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
    /* Crash recovery: if the session was loaded from disk with an
       interrupted prior turn (pendingTurn was non-null at hydration
       time — a previous Woom process died mid-stream), stamp an
       app_crash recap onto cwdSwitchRecap and rotate the CLI uuid.
       buildAgentAppContext below folds the recap into the system
       prompt; the rotated uuid avoids a "Session ID already in use"
       error if the previous CLI's session-id lock survived the crash.
       Cleared in a single shot — subsequent turns on this session
       skip the branch because `interrupted` is now false. */
    const crashedSess = sessionsState.list.find((x) => x.id === id);
    if (crashedSess?.interrupted) {
      const recap = buildContinuationRecap(crashedSess, 'app_crash');
      updateSession(id, {
        interrupted: false,
        cwdSwitchRecap: recap,
        claudeUuid: genUuid(),
        claudeResumable: false
      });
      /* Visible breadcrumb in the chat thread so the user sees that a
         silent rotation + recap happened. Without this the next
         assistant reply seems to ignore the prior context, which
         reads as "agent forgot" even though the recap is in the
         system prompt. Inserted between the user's new message and
         the assistant placeholder so the chronology stays correct. */
      appendSessionMessage(id, {
        role: 'system',
        content: '↩ Recovered from interrupted turn — prior transcript folded into the system prompt.',
        at: new Date().toISOString()
      });
    }
    /* First-turn preamble: when no assistant turn has completed yet on
       this session, the CLI has zero context about the working dir.
       Without help the agent opens with `git status` / `pwd` / `ls`
       tool calls before getting to the user's question. Build a
       snapshot (branch / status / recent commits / CLAUDE.md presence)
       and prepend it to cwdSwitchRecap so it lands in the system
       prompt suffix on this turn. Composes cleanly with the crash
       recap above (which already set cwdSwitchRecap for crashed
       sessions). Runs async — adds ~50-150ms to the first send only.
       Worktree wins over plain cwd because that's the dir the agent
       actually operates in. */
    const sessForFirstTurn = sessionsState.list.find((x) => x.id === id);
    const priorAssistantTurns = (sessForFirstTurn?.messages ?? []).filter(
      (m) => m.role === 'assistant' && m.content.trim().length > 0
    ).length;
    if (sessForFirstTurn && priorAssistantTurns === 0) {
      const preambleCwd =
        sessForFirstTurn.worktreePath
        || sessForFirstTurn.cwd
        || editorRepoPath
        || null;
      /* Surface the file the user has open in the linked editor (if
         any) so the agent doesn't need a separate read just to know
         what they're looking at. Falls back to null when the session
         isn't linked — the preamble line drops out cleanly. */
      const linkedEditorId = sessForFirstTurn.linkedToEditor
        ? sessForFirstTurn.linkedToEditorInstanceId
        : null;
      const openFile = linkedEditorId ? getActiveEditorFile(linkedEditorId) : null;
      /* Recall-bias terms: the just-sent user text (first 200 chars,
         tokenized lazily) + @-mention titles. The Rust sanitizer
         strips operators, so we don't have to be careful about
         punctuation. Caps at 5 terms inside the helper. */
      const recallTerms: string[] = [
        ...text.slice(0, 200).split(/\s+/).filter((w) => w.length >= 4),
        ...mentionsSnapshot.map((m) => m.title).filter(Boolean)
      ];
      const preamble = await buildFirstTurnPreamble(preambleCwd, openFile, recallTerms);
      if (preamble) {
        const existing = sessForFirstTurn.cwdSwitchRecap;
        const composed = existing
          ? `${preamble}\n\n---\n\n${existing}`
          : preamble;
        updateSession(id, { cwdSwitchRecap: composed });
      }
    }
    // Append empty assistant message that streaming will fill.
    appendSessionMessage(id, {
      role: 'assistant',
      content: '',
      at: new Date().toISOString()
    });
    startThinkingTimer(kind);
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

    // Drain any action-card outcomes that landed since the agent's last
    // turn (commit/PR/bash/cwd-switch results) and prepend them as a
    // structured block. The CLI's `--resume` history doesn't include
    // these — they're Woom-side annotations — so this is the only
    // channel the agent has for learning whether its proposed commit
    // pushed cleanly, what stderr a bash card produced, etc. The drain
    // also clears the queue so a result is never delivered twice.
    const pendingForAgent = drainPendingActionResultsForAgent(id);
    if (pendingForAgent.length > 0) {
      const block = formatActionResultsForPrompt(pendingForAgent);
      prompt = `${block}\n\n---\n\n${prompt}`;
    }

    // Priority of working dir:
    //   1. Session has an isolated worktree → use it (SPEC: "every Claude run
    //      in a worktree, never touches main working tree").
    //   2. Explicit cwd set by user via pickCwd.
    //   3. Editor column's open repo (shared state).
    //   4. None → agent inherits Woom's cwd (last-resort fallback).
    const cwd = sess?.worktreePath || sess?.cwd || editorRepoPath || null;
    const claudeUuid = sess?.claudeUuid ?? genUuid();
    const resume = Boolean(sess?.claudeResumable);
    const rules = sessionsState.userRules.trim();
    const agentKind = sess?.agentKind ?? 'claude';
    const cursorModel = agentKind === 'cursor' ? (sess?.cursorModel ?? null) : null;
    const claudeModel = agentKind === 'claude' ? (sess?.claudeModel ?? null) : null;
    /* CLAUDE.md prefetch — populates the per-cwd cache so the sync
       `buildAgentAppContext` below can stamp the merged project memory
       into the system-prompt suffix. Cheap (Rust walker hits the same
       dirs and returns from in-memory cache on subsequent calls); only
       paying the IO once per cwd per session. */
    await loadClaudeMd(sess?.worktreePath ?? sess?.cwd ?? null).catch(() => {});
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

    // Outer try/finally ensures `sending: false` lands on every exit
    // path — exceptions escaping the inner catch, future code changes
    // forgetting the cleanup, etc. The body below already updates
    // `sending: false` in the normal cleanup; the finally is a
    // belt-and-braces no-op when that has already happened, and a
    // safety net when it hasn't.
    try {
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
      // Resume-orphan self-heal. The CLI lost the session uuid we tried to
      // resume against (pruned on disk, CLI reinstalled, etc.). Mint a
      // fresh uuid, clear the resumable flag, stamp a recap of the in-
      // memory chat into the next system prompt via cwdSwitchRecap (the
      // existing one-shot channel), then retry this same prompt once.
      // The retry never re-enters this branch on its own orphan because
      // we cleared `resume`.
      if (isResumeOrphanError(e) && !cancelled) {
        const detail = msg.slice(RESUME_ORPHAN_PREFIX.length).trim();
        const sessNow = sessionsState.list.find((x) => x.id === id);
        if (sessNow) {
          const recap = buildContinuationRecap(sessNow, 'cli_orphan', { detail });
          updateSession(id, {
            claudeUuid: genUuid(),
            claudeResumable: false,
            cwdSwitchRecap: recap
          });
          notify({
            kind: 'info',
            title: `${s.agentKind === 'cursor' ? 'Cursor' : 'Claude'} session refreshed`,
            body: 'Prior CLI history was unavailable; restarted with the in-app transcript baked into context. Continuing your turn.',
            ttlMs: 6000
          });
          // Re-enter sendClaudeMessage. We pre-stamped the new uuid and
          // cleared sending below would block, so we have to unmark it
          // first. The trim+empty-mention guard at the top is fine —
          // this is a true retry of the same prompt that already had
          // both. Pop the placeholder assistant bubble so the streaming
          // restart appends to a fresh one.
          replaceLastAssistant(id, '');
          updateSession(id, { sending: false, input: text, mentions: mentionsSnapshotPre });
          stopThinkingTimer(kind);
          await sendClaudeMessage();
          return;
        }
      }
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
    stopThinkingTimer(kind);
    // Flush any action-card outcomes that landed mid-turn into the
    // chat transcript NOW that the assistant message is no longer
    // streaming. Mid-stream flushing would shift the last-message
    // position and silently drop further deltas (the bug that cut
    // off responses mid-sentence). At this point sending is still
    // true but there's nothing more to stream into — the recv loop
    // ended, so it's safe to append system messages.
    flushActionResultsToUI(id);
    const finalSess = sessionsState.list.find((x) => x.id === id);
    const erroredOut = finalSess?.messages.some(
      (m, i) => i === finalSess.messages.length - 1 && m.role === 'assistant' && m.content.startsWith('**Claude failed:')
    );
    updateSession(id, { sending: false });
    /* SDD refresh — if the session has an active SDD workspace, the
     *  agent's just-completed turn likely wrote a spec/plan/phase file.
     *  Re-read the workspace from disk so the inline card reflects the
     *  new state (stage transition, new phases, status flips, etc.).
     *  Fire-and-forget; the resulting `sdd:changed:<id>` event will
     *  update the reactive store. */
    {
      const sddWs = workspaceForSession(id);
      if (sddWs) void refreshSdd(sddWs.id);
    }
    /* Stop hook — fires on every normal turn completion (errored OR
       not). Hooks can use this for notifications, log archival, or
       conditional `/loop`-style behavior. Failures swallow so a bad
       hook can't break end-of-turn cleanup. */
    void runHook('Stop', {
      session_id: id,
      agent_kind: kind,
      errored: !!erroredOut,
      duration_ms: Date.now() - runStartedAt,
      message_count: finalSess?.messages.length ?? 0
    }).catch(() => {});
    /* Statusline refresh on turn end — picks up cost / context-window
       deltas from the just-completed assistant turn. Cheap (one
       sh -c spawn) and capped server-side, so a slow script can't
       block the next turn. */
    {
      const payload = buildStatusLinePayload();
      if (payload) void runStatusLine(payload);
    }
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
    } finally {
      // Belt-and-braces sending=false. The body above already unsets
      // it on the normal happy/error paths; this catches exceptions
      // that escape the inner catch + any future code path that
      // returns early without cleanup. Idempotent — checked first to
      // avoid an extra reactive tick when nothing's wrong.
      const stillSending = sessionsState.list.find((s) => s.id === id)?.sending;
      if (stillSending) {
        stopThinkingTimer(kind);
        updateSession(id, { sending: false });
      }
      /* Drain the per-session queue. If the user typed more messages
         while the previous turn was running, they're parked here in
         FIFO order — fire the next one. Re-uses the session's normal
         send pipeline by setting it active + writing the queued text
         into `input`, then calling sendClaudeMessage(). The new
         active-session flip is a feature, not a bug: the user is
         intentionally driving this session forward. */
      const sessAfterDrain = sessionsState.list.find((x) => x.id === id);
      const queue = sessAfterDrain?.pendingQueue ?? [];
      if (queue.length > 0) {
        const [nextEntry, ...rest] = queue;
        // Save the user's current draft once (on first drain) so we can
        // restore it after the queue is fully consumed.
        if (!queueSavedDrafts.has(id)) {
          const cur = sessAfterDrain!;
          if (cur.input.trim() || cur.mentions.length > 0) {
            queueSavedDrafts.set(id, { text: cur.input, mentions: [...cur.mentions] });
          }
        }
        updateSession(id, { pendingQueue: rest, input: nextEntry.text, mentions: nextEntry.mentions });
        sessionsState.activeClaudeId = id;
        sessionsState.activeIds[sessAfterDrain!.agentKind] = id;
        /* queueMicrotask: let Svelte settle the input/pendingQueue
           updates before the recursive send reads them. Otherwise
           the early "no input" guard could fire on a stale read. */
        queueMicrotask(() => {
          void sendClaudeMessage();
        });
      } else {
        // Queue empty — restore whatever the user had typed before the
        // queue started firing.
        const saved = queueSavedDrafts.get(id);
        if (saved) {
          queueSavedDrafts.delete(id);
          if (saved.text.trim() || saved.mentions.length > 0) {
            updateSession(id, { input: saved.text, mentions: saved.mentions });
          }
        }
      }
    }
  }

  // Streaming-event dispatch lives in `$lib/stream/agentStream.ts`. The
  // caller here just forwards assistant text deltas to the chat (session
  // store + scroll-to-bottom is the only DOM-coupled bit).
  function appendAssistantDelta(sessionId: string, delta: string) {
    appendToLastAssistant(sessionId, delta);
    void scrollChatBottom();
  }

  /** Woom-app MCP navigation: the agent calls `mcp__app__open_jira_issue`
   *  / `switch_view` / `add_editor_instance` / etc., the stream parser sees
   *  the `tool_use` event, and we drive Woom's reactive state directly
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
   *  `sentry` / `claude` / `cursor` / `editor` / `canvas` / `terminal`)
   *  so a future GitLab/Bitbucket tab can claim its own slot. Translate
   *  to the internal `…App` view name. Returns `null` for unknown
   *  values so the handler can no-op cleanly. */
  function mapAgentViewToInternal(v: string): View | null {
    switch (v) {
      case 'github':       return 'githubApp';
      case 'jira':         return 'jiraApp';
      case 'sentry':       return 'sentryApp';
      case 'claude':       return 'claudeApp';
      case 'cursor':       return 'cursorApp';
      case 'editor':       return 'editorApp';
      case 'canvas':       return 'canvasApp';
      case 'terminal':     return 'terminalApp';
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
        if (key) {
          inboxState.jiraFocusKey = key;
          view = 'jiraApp';
        }
        return;
      }
      case 'mcp__app__open_sentry_issue': {
        // openSentryFocus(id) defaults eventId to null — equivalent to
        // "latest" so a stale event id from a previous open_sentry_event
        // call doesn't carry over.
        const id = str('id');
        if (id) {
          openSentryFocus(id);
          view = 'sentryApp';
        }
        return;
      }
      case 'mcp__app__open_sentry_event': {
        const id = str('issue_id');
        const eventId = str('event_id') || null;
        if (id) {
          openSentryFocus(id, eventId);
          view = 'sentryApp';
        }
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
        // Translate platform-named views (`github` / `jira` / `sentry` /
        // `claude` / `cursor` / `editor` / `canvas` / `terminal`) to the
        // matching `…App`. `rules` / `connections` / `settings` pass
        // through unchanged.
        const mapped = mapAgentViewToInternal(v);
        if (mapped) view = mapped;
        return;
      }
      case 'mcp__app__open_repo': {
        const repoPath = str('repo_path');
        view = 'editorApp';
        if (repoPath) setEditorRepoPath(repoPath, APP_INSTANCE_IDS.editor);
        return;
      }
      case 'mcp__app__open_connect_modal': {
        const sourceId = str('source');
        const conn = connectionsMeta.find((c) => c.id === sourceId);
        if (conn) openConnectModal(conn);
        return;
      }
      case 'mcp__app__open_github_repo': {
        const owner = str('owner');
        const repo = str('repo');
        const section = str('section') || 'pulls';
        const path = str('path');
        if (!owner || !repo) return;
        view = 'githubApp';
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
        view = 'jiraApp';
        updateJiraTabFilters(patch);
        return;
      }
      case 'mcp__app__open_sentry_tab': {
        // SentryTab fields are flat on `inboxState` (not under one
        // filter object), so we can't reuse a setSentryFilters-style
        // patch. Mutate field-by-field — `scheduleSentryTabFilterRefresh`
        // persists and re-runs the query the same way the dropdown
        // change handlers do.
        view = 'sentryApp';
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
      case 'mcp__app__set_github_instance': {
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
        view = 'githubApp';
        updateGithubFilters(inst.id, patch);
        return;
      }
      case 'mcp__app__set_jira_instance': {
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
        view = 'jiraApp';
        updateJiraFilters(inst.id, patch);
        return;
      }
      case 'mcp__app__set_sentry_instance': {
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
        view = 'sentryApp';
        setSentryFilters(inst.id, patch);
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
        view = 'editorApp';
        setEditorRepoPath(repoPath, editor.id);
        // Linked sessions are NO LONGER auto-synced from here. We
        // used to run `applySessionCwd(s.id, repoPath)` for every
        // linked session — that's exactly why changing the editor's
        // folder yanked the chat along and the mismatch chip never
        // got a chance to render. Now the pulsing orange "Folder
        // mismatch" button lights up in the agent's WorktreeBar and
        // the user explicitly picks which side wins through the
        // syncAgentToLinkedEditor / syncLinkedEditorToAgent menu.
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
            view = inst.kind === 'cursor' ? 'cursorApp' : 'claudeApp';
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
      case 'mcp__app__add_app_instance': {
        /* Spawn a new app instance. The sidecar shipped this tool but
         * the dispatcher had no handler — agent saw a "success" message,
         * UI didn't change, then on follow-up ("link them") it fell back
         * to set_editor_repo_path which mutated the EXISTING editor.
         *
         * Editor / canvas / terminal support multi-instance; the
         * singleton kinds (github / jira / sentry / claude / cursor)
         * just switch the rail to that solo. */
        const kindRaw = str('kind').toLowerCase();
        type AppKind = PanelKind;
        const VALID_KINDS: AppKind[] = [
          'github', 'jira', 'sentry', 'claude', 'cursor',
          'editor', 'canvas', 'terminal'
        ];
        if (!(VALID_KINDS as readonly string[]).includes(kindRaw)) return;
        const kind = kindRaw as AppKind;
        const VIEW_BY_KIND: Record<AppKind, View> = {
          github: 'githubApp',
          jira: 'jiraApp',
          sentry: 'sentryApp',
          claude: 'claudeApp',
          cursor: 'cursorApp',
          editor: 'editorApp',
          canvas: 'canvasApp',
          terminal: 'terminalApp'
        };
        if (!MULTI_INSTANCE_KINDS.has(kind)) {
          /* Singleton kinds: just switch view. The sidecar description
           * already promises "no-op if already present". */
          view = VIEW_BY_KIND[kind];
          return;
        }
        const inst = addLayoutInstance(kind);
        if (!inst) return;
        /* Editor-only: set the just-spawned instance's repoPath if
         * provided. We deliberately do NOT touch the calling session's
         * editor link — that's the user's call. If the session was
         * unlinked before, spawning an editor doesn't suddenly bind
         * them; if it was linked elsewhere, that link stays. The agent
         * can ask the user "want me to link?" or the user picks via the
         * cwd-bar's "Link editor…" dropdown. */
        if (kind === 'editor') {
          const repoPath = pickDeep(input as Record<string, unknown>, REPO_PATH_KEYS_DEEP);
          if (repoPath) setEditorRepoPath(repoPath, inst.id);
        }
        view = VIEW_BY_KIND[kind];
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

  /** Resolve the singleton record for a given kind. App mode keeps
   *  this shape so MCP-dispatch callers don't have to know the
   *  concrete id format — `name` is unused here (legacy art-names are
   *  gone), and `width` is a stub (`0`) since solo apps fill the
   *  whole window. */
  function findInstanceByNameOrId(
    kind: PanelKind,
    _name: string,
    _id: string
  ): { id: string; kind: PanelKind; name: string; width: number } | null {
    return { id: APP_INSTANCE_IDS[kind], kind, name: kind, width: 0 };
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
      /* Inline-only architecture — there's no global modal that can
         render the PR over Claude/Cursor. So when the agent triggers
         an open from another view, we have to switch the user TO the
         GitHub app or they wouldn't see anything. */
      view = 'githubApp';
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
        if (key) {
          inboxState.jiraFocusKey = key;
          view = 'jiraApp';
        }
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
        if (id) {
          openSentryFocus(id);
          view = 'sentryApp';
        }
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
        // Activate the session in the agent solo it lives on, then switch
        // the rail to that app. App singletons mean we can resolve the
        // kind from the session's `agentInstanceId` directly.
        const sessionId = typeof p.sessionId === 'string' ? p.sessionId : '';
        if (!sessionId) return;
        const sess = sessionsState.list.find((s) => s.id === sessionId);
        if (!sess?.agentInstanceId) return;
        const kind = kindForInstanceId(sess.agentInstanceId);
        if (kind !== 'claude' && kind !== 'cursor') return;
        setActiveSessionInInstance(sess.agentInstanceId, sessionId);
        view = kind === 'cursor' ? 'cursorApp' : 'claudeApp';
        return;
      }
    }
  }

  // ---- Message replay / edit ----
  /** Edit = lift the user's prior message back into the composer, drop
   *  everything after it from the transcript, and let the user tweak +
   *  re-send. Same truncate semantics as Resend but no confirm dialog
   *  and no immediate send — the user explicitly wanted to change
   *  something. The composer textarea gets focus + caret-at-end so they
   *  can start typing right away. */
  function startEditMessage(sessionId: string, index: number, content: string) {
    const orig = sessionsState.list.find((s) => s.id === sessionId)?.messages[index];
    const images = orig?.images ?? [];
    sessionsState.activeClaudeId = sessionId;
    truncateSessionAt(sessionId, index);
    if (images.length) {
      attachPathsToSession(sessionId, images.map((i) => i.path));
    }
    setSessionInput(sessionId, content);
    queueMicrotask(() => {
      const ta = document.querySelector<HTMLTextAreaElement>('.cmp-area');
      if (!ta) return;
      ta.focus();
      try { ta.setSelectionRange(content.length, content.length); } catch { /* ignore */ }
    });
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
    dispatchAction(sessionId, action, onActionResolved);
  }

  /** Drop an action card AND tell the sidecar's IPC waiter that the
   *  user dismissed (so its blocking MCP call returns and the agent
   *  can react to "user said no" in the same turn instead of hanging
   *  the CLI on a never-arriving response). Cards without a waitId
   *  (legacy fire-and-forget path) just get removed locally. */
  function dismissAction(sessionId: string, actionId: string) {
    const sess = sessionsState.list.find((s) => s.id === sessionId);
    const a = sess?.actions.find((x) => x.id === actionId);
    const waitId = a?.waitId;
    removeAction(sessionId, actionId);
    if (waitId) {
      void invoke('resolve_action_wait', {
        waitId,
        ok: false,
        summary: 'User dismissed the card. The action was not run. Decide whether to propose a different approach or stop and ask the user what they want.'
      }).catch((e) => console.warn('[action-ipc] dismiss resolve failed', e));
    }
  }

  /** Per-session re-entry guard. Two cards finishing in the same
   *  microtask both pass the `stillBusy=false` check above (Svelte
   *  state writes aren't synchronous gates) and would each fire
   *  `continueAgentTurn`, so the agent gets the same recap twice and
   *  produces a duplicate turn. The Set tracks "continuation already
   *  fired for this batch" — entries are cleared when continueAgentTurn
   *  finishes (in finally) so the next user-initiated batch can fire. */
  const continuationInFlight = new Set<string>();

  /** Called by every executeXxx after the action ran. The executor
   *  already pushed the outcome onto the pending-action-results
   *  queue (see `enqueuePendingActionResult`). All this hook does is
   *  decide whether to AUTO-FIRE the next agent turn so the user
   *  doesn't have to type "continue" by hand. The continuation
   *  itself drains the same queue inside `continueAgentTurn`, so
   *  the recap is built from one source of truth — no risk of the
   *  manual-send and auto-fire paths reporting different things. */
  function onActionResolved(
    sessionId: string,
    _action: ClaudeAction,
    _result: { ok: boolean; summary: string }
  ) {
    const sess = sessionsState.list.find((s) => s.id === sessionId);
    if (!sess) return;
    // Wait for ALL pending actions before continuing — agent may have
    // proposed a sequence (commit + PR) that we want to resolve in one
    // batch. `executing` counts as "still in flight".
    const stillBusy = sess.actions.some(
      (a) => a.status === 'pending' || a.status === 'executing'
    );
    if (stillBusy) return;
    if (continuationInFlight.has(sessionId)) return;
    // BLOCK auto-continue when the previous turn errored. Without this
    // guard, a CLI-stopped-responding / forced-shutdown cascades into
    // a second auto-resume that hits the now-locked session-id ("Session
    // ID … is already in use"), and so on. The user can hit the explicit
    // "Retry" button on the error chip if they want to try again — that
    // path goes through the recovery flow that handles uuid rotation.
    const lastMsg = sess.messages[sess.messages.length - 1];
    const lastErrored =
      lastMsg?.role === 'assistant' &&
      (lastMsg.content.startsWith('**Claude failed:') ||
        lastMsg.content.startsWith('**Cursor failed:'));
    if (lastErrored) return;
    // We DON'T require `sess.awaitingApproval` here. That flag is only
    // stamped at the END of `sendClaudeMessage`'s finally block, so
    // when the user approves a card before the flag lands (fast clicks,
    // streaming still settling) the continuation would silently no-op
    // and they'd have to type "продолжи" by hand. Downstream guards
    // (`sending`, drained-queue empty, in-flight Set above) cover the
    // real concurrency cases.
    continuationInFlight.add(sessionId);
    if (sess.awaitingApproval) {
      updateSession(sessionId, { awaitingApproval: false });
    }
    void continueAgentTurn(sessionId);
  }

  /** Re-enter `runAgentRequest` for an auto-continuation. The prompt
   *  is built from the pending-action-results queue, drained inside
   *  this function — same code path the manual `sendClaudeMessage`
   *  uses, so the agent sees identical "since-last-turn outcomes"
   *  formatting whether it picks up automatically or after the user
   *  types something. No `prompt` arg from the caller because the
   *  drained queue IS the prompt; an empty drain means the caller
   *  has nothing meaningful to send. */
  async function continueAgentTurn(sessionId: string) {
    const sess = sessionsState.list.find((s) => s.id === sessionId);
    if (!sess || sess.sending) {
      // Bail without firing the turn but still release the guard so
      // a future approval batch can continue. Otherwise a stuck
      // continuationInFlight entry would silently swallow the next
      // auto-resume.
      continuationInFlight.delete(sessionId);
      return;
    }
    // Drain the pending-action-results queue and turn it into the
    // prompt. If nothing's there (e.g. the user manually triggered
    // continuation but no actions are awaiting), bail — there's no
    // meaningful prompt to send and runAgentRequest with empty
    // content would just confuse the agent.
    const drained = drainPendingActionResultsForAgent(sessionId);
    if (drained.length === 0) {
      continuationInFlight.delete(sessionId);
      return;
    }
    const prompt = formatActionResultsForPrompt(drained);
    const kind = (sess.agentKind ?? 'claude') as 'claude' | 'cursor';
    updateSession(sessionId, { sending: true });
    appendSessionMessage(sessionId, {
      role: 'assistant',
      content: '',
      at: new Date().toISOString()
    });
    startThinkingTimer(kind);
    const runStartedAt = Date.now();
    void scrollChatBottom();

    const cwd = sess.worktreePath || sess.cwd || editorRepoPath || null;
    const claudeUuid = sess.claudeUuid;
    const resume = Boolean(sess.claudeResumable);
    const rules = sessionsState.userRules.trim();
    const agentKind = sess.agentKind;
    const cursorModel = agentKind === 'cursor' ? sess.cursorModel : null;
    const claudeModel = agentKind === 'claude' ? sess.claudeModel : null;
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
    stopThinkingTimer(kind);
    // Same flush hook as sendClaudeMessage's: any action-card outcomes
    // that landed during this continuation (e.g. the agent proposed a
    // PR and the user approved before this turn finished) get their
    // chips appended now, after streaming is done.
    flushActionResultsToUI(sessionId);
    updateSession(sessionId, { sending: false });
    continuationInFlight.delete(sessionId);
    void scrollChatBottom();
    // Belt-and-braces: even if an exception escapes the catch above,
    // make sure sending and the in-flight guard get cleared so a
    // future approval batch isn't blocked by stuck state. Idempotent.
    const sessNow = sessionsState.list.find((s) => s.id === sessionId);
    if (sessNow?.sending) {
      updateSession(sessionId, { sending: false });
    }
    continuationInFlight.delete(sessionId);
  }

  // ---- Agent execution ----

  async function stopAgentForKind(kind: 'claude' | 'cursor') {
    const activeId = sessionsState.activeIds[kind];
    const s = activeId ? sessionsState.list.find((x) => x.id === activeId) : null;
    if (!s) return;
    try {
      await stopAgentRequest(s.id);
    } catch (e) {
      notifyError(e, { title: 'Stop failed' });
    }
    // After SIGKILL the Claude CLI's session-id store can end up in a
    // wedged state — `--resume <uuid>` later either returns partial
    // context, refuses with "session in use", or silently restarts
    // fresh without telling us. The user-visible symptom is "agent
    // forgot what I just said". Force a clean restart on the NEXT
    // turn: rotate uuid, drop resumable, stamp a recap of the current
    // in-memory transcript so the fresh CLI starts with the full
    // context (anchored on the very FIRST user message — the original
    // task brief — plus the recent slice). The unified
    // `buildContinuationRecap` does this for every context-loss
    // scenario (Stop / cli_orphan / cwd_switch / app_restart) — same
    // strong recap shape, no more divergence between sources.
    const sessNow = sessionsState.list.find((x) => x.id === s.id);
    if (!sessNow) return;
    const recap = buildContinuationRecap(sessNow, 'stop', {
      detail: 'User pressed Stop — restarting CLI session with the in-app transcript baked in to avoid context loss.'
    });
    updateSession(s.id, {
      claudeUuid: genUuid(),
      claudeResumable: false,
      cwdSwitchRecap: recap
    });
  }


  /** After a mutating action (comment/review/merge/close) on the focused
      item, re-pull detail + inbox + repo-view items. Wraps the store's
      plain reload so the repo-view refresh (cross-cutting, not owned by the
      inbox store) fires alongside. */
  async function reloadDetailAndLists() {
    await reloadDetailAndListsCore();
    // Ask the open GithubApp repos view to refresh if one is open
    // (merge/close/comment flows need to see the new state reflected there).
    repositoriesView?.refreshItems();
  }

  /** Open a freshly-created PR (or any GitHub PR URL) inside Woom by
      synthesizing a minimal `InboxItem` and letting the `focusItem` effect
      hit the API for the full detail. Called from the action card's
      "Open in Woom" button after Claude creates a PR.

      URL shape: `https://github.com/<owner>/<repo>/pull/<number>` (trailing
      path segments like `/files` are ignored). Returns silently if the URL
      doesn't match — the card's raw link remains usable. */
  function openPrUrlInWoom(url: string, action: (ClaudeAction & { kind: 'pr' }) | null) {
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
    view = 'githubApp';
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
      view = 'githubApp';
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
      paletteMode = 'normal';
      paletteOpen = !paletteOpen;
    } else if ((e.metaKey || e.ctrlKey) && e.shiftKey && (e.key === 'a' || e.key === 'A')) {
      /* ⌘⇧A — Agent View dashboard. Mirrors Claude Code's `claude
       * agents` table — every Claude / Cursor session grouped by
       * lifecycle (Needs input / Working / Recent / Older + Pinned).
       * Toggle so a second press dismisses. */
      e.preventDefault();
      agentDashboardOpen = !agentDashboardOpen;
    } else if ((e.metaKey || e.ctrlKey) && e.shiftKey && (e.key === 'f' || e.key === 'F')) {
      /* ⌘⇧F — project-wide find. Standard IDE shortcut every
       * dev coming from VSCode / Cursor / JetBrains expects.
       * Toggle: pressing again while open closes (matches ⌘K). */
      e.preventDefault();
      searchInFilesOpen = !searchInFilesOpen;
    } else if ((e.metaKey || e.ctrlKey) && (e.key === 'p' || e.key === 'P') && !e.shiftKey && !e.altKey) {
      /* ⌘P — quick-open file picker. Mirror of VSCode / Cursor /
       * Sublime; the most-used IDE shortcut after Esc. Toggle so
       * a stray second press dismisses cleanly. */
      e.preventDefault();
      quickOpenOpen = !quickOpenOpen;
    } else if (
      (e.metaKey || e.ctrlKey) && e.shiftKey &&
      (e.key === '?' || e.key === '/' || e.code === 'Slash')
    ) {
      /* ⇧⌘? — toggle the Welcome / Help overlay. We accept both `?`
       * (shift-/ on US layouts surfaces the printable char) and `/`
       * + `e.code === 'Slash'` so non-US layouts where shift-/ resolves
       * to something else still hit the binding. Mutually exclusive
       * with the bare-`?` cheatsheet — closing one before flipping
       * the other keeps the modal stack legible. */
      e.preventDefault();
      cheatsheetOpen = false;
      welcomeOpen = !welcomeOpen;
    } else if (
      (e.metaKey || e.ctrlKey) && e.shiftKey &&
      (e.key === 'v' || e.key === 'V' || e.code === 'KeyV') &&
      view === 'editorApp'
    ) {
      /* ⇧⌘V — cycle Markdown preview mode (off → split → only → off).
       * Mirrors VSCode's "Open Preview to the Side" muscle memory.
       * Scoped to the editor solo so the binding doesn't fight Apple's
       * paste-without-formatting binding when the user is in another
       * surface. */
      e.preventDefault();
      window.dispatchEvent(new CustomEvent('woom:editor:toggle-md-preview'));
    } else if ((e.metaKey || e.ctrlKey) && e.shiftKey && (e.key === 'o' || e.key === 'O' || e.code === 'KeyO')) {
      /* ⇧⌘O — Go to symbol in file. Same shortcut VSCode / Cursor
       * already train people on, so muscle memory carries over.
       * `e.code` fallback because the shifted-O may report differently
       * on non-US layouts (Cmd-Shift remaps the printable char). */
      e.preventDefault();
      symbolPickerOpen = !symbolPickerOpen;
    } else if (
      (e.metaKey || e.ctrlKey) &&
      !e.shiftKey && !e.altKey &&
      (e.key === '[' || e.code === 'BracketLeft') &&
      !isTextInput(e.target)
    ) {
      /* ⌘[ — solo-history back. Browser-style nav across solos so
       * the user can jump GitHub PR → Jira ticket → Sentry issue
       * and walk back through the trail without re-clicking icons.
       * Skip when focus is in a text input (Composer / Editor / list
       * search) so the binding doesn't fight the system's outdent /
       * "previous word" Emacs binding. */
      e.preventDefault();
      navBack();
    } else if (
      (e.metaKey || e.ctrlKey) &&
      !e.shiftKey && !e.altKey &&
      (e.key === ']' || e.code === 'BracketRight') &&
      !isTextInput(e.target)
    ) {
      /* ⌘] — solo-history forward. Mirror of ⌘[. Only meaningful
       * after at least one ⌘[ press; idle no-op otherwise. */
      e.preventDefault();
      navForward();
    } else if (
      (e.metaKey || e.ctrlKey) &&
      !e.shiftKey && !e.altKey &&
      (e.key === '\\' || e.code === 'Backslash') &&
      !isTextInput(e.target)
    ) {
      /* ⌘\ — toggle UI density (comfortable ↔ compact). Mirrors
       * Slack's keyboard binding for the same toggle. Skipped when a
       * text input has focus so the binding doesn't fight a literal
       * `\` keystroke in the composer or editor. */
      e.preventDefault();
      toggleDensity();
    } else if (
      (e.metaKey || e.ctrlKey) &&
      !e.shiftKey && !e.altKey &&
      (e.key === '.' || e.code === 'Period')
    ) {
      /* ⌘. — interrupt the active agent run. Mirrors macOS's
       * "Cancel" muscle memory (used to abort everything from a
       * stuck Spotlight query to a print job). Scoped to whichever
       * agent solo the user is currently looking at so the binding
       * doesn't accidentally kill a Cursor turn while you're in
       * Claude's column. No-op when there's no in-flight session;
       * `stopAgentForKind` already guards with `if (!s) return`. */
      const kind: 'claude' | 'cursor' | null =
        view === 'claudeApp' ? 'claude'
        : view === 'cursorApp' ? 'cursor'
        : null;
      if (kind) {
        const activeId = sessionsState.activeIds[kind];
        const s = activeId ? sessionsState.list.find((x) => x.id === activeId) : null;
        if (s?.sending) {
          e.preventDefault();
          void stopAgentForKind(kind);
        }
      }
    } else if ((e.metaKey || e.ctrlKey) && e.key === 'e' && !e.shiftKey && !e.altKey) {
      /* ⌘E — quick switcher to most-recently-touched things. Open in
       * `recents` mode (other sections hidden until the typed query
       * stops matching any recent row). Mirrors JetBrains' Recent
       * Files / VS Code's "Quick Open Last" muscle memory. */
      e.preventDefault();
      paletteMode = 'recents';
      paletteOpen = true;
    } else if (
      (e.metaKey || e.ctrlKey) &&
      !e.shiftKey && !e.altKey &&
      e.key >= '0' && e.key <= '8'
    ) {
      /* ⌘0..⌘8 — solo switcher. The rail tooltips have promised
       * these shortcuts since the rail's first pass (`Home · ⌘0`,
       * `Jira · ⌘1`, …, `Terminal · ⌘8`) but the keyboard handler
       * never landed — this closes that gap. Order matches the rail
       * top-to-bottom so the digit reads as the icon's row. */
      e.preventDefault();
      const targetView = SOLO_BY_DIGIT[e.key];
      if (targetView) view = targetView;
    } else if (e.key === 'Escape') {
      /* Cheatsheet / Welcome win on their own Escape — neither
         lives in the modal stack the rest of the cascade walks, so
         we shortcut out before clearing inbox / palette state.
         Welcome takes priority because it's the bigger surface. */
      if (welcomeOpen) {
        welcomeOpen = false;
        return;
      }
      if (cheatsheetOpen) {
        cheatsheetOpen = false;
        return;
      }
      paletteOpen = false;
      searchInFilesOpen = false;
      quickOpenOpen = false;
      symbolPickerOpen = false;
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
    } else if (e.key === '?' && !isTextInput(e.target) && !anyModalOpen()) {
      /* `?` toggles the cheatsheet. Skip when an input/textarea has
         focus so it doesn't hijack a literal `?` the user is typing. */
      e.preventDefault();
      cheatsheetOpen = !cheatsheetOpen;
    } else if (e.key === 'j' && isSourceApp && !anyModalOpen()) {
      moveSelection(1);
    } else if (e.key === 'k' && isSourceApp && !(e.metaKey || e.ctrlKey) && !anyModalOpen()) {
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
      searchInFilesOpen ||
      quickOpenOpen ||
      symbolPickerOpen ||
      cheatsheetOpen ||
      welcomeOpen
    );
  }

  function githubTokenUrl() {
    const scopes = ['repo', 'read:user', 'read:org'].join(',');
    return `https://github.com/settings/tokens/new?scopes=${scopes}&description=Woom%20Desktop`;
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
    /* Pull repo default from the first inbox's filter, same
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
      view = 'githubApp';
      void refreshAllInboxes({ silent: true });
    } catch (e) {
      patchModal('githubCreatePr', { busy: false, error: typeof e === 'string' ? e : String(e) });
    }
  }
</script>

<svelte:window onkeydown={onKey} />

<Cheatsheet
  open={cheatsheetOpen}
  onClose={() => (cheatsheetOpen = false)}
  onOpenWelcome={() => {
    cheatsheetOpen = false;
    queueMicrotask(() => (welcomeOpen = true));
  }}
/>

<WelcomeOverlay
  open={welcomeOpen}
  onClose={() => (welcomeOpen = false)}
  setView={(v) => (view = v)}
  onOpenCheatsheet={() => {
    /* Hand off cleanly: Welcome closes itself, Cheatsheet opens
       on the next microtask so the focus-trap unmount happens
       before the new modal claims focus. */
    welcomeOpen = false;
    queueMicrotask(() => (cheatsheetOpen = true));
  }}
/>

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
      <h1 class="lock-title">Woom is locked</h1>
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

<!-- Global drag hint banner — appears top-center while any payload is
     in flight. The drag affordance into Claude / Cursor / Editor /
     Canvas was previously silent (user only learned by experimentation),
     so this banner names the valid drop targets up front. Source-tinted
     to match the drag chip and reinforce "what is being dragged".
     Auto-dismisses on dragend via the global safety net clearing
     `dragState.payload`. -->
{#if dragState.payload}
  {@const src = dragState.payload.source}
  {@const tone =
    src === 'github' ? 'var(--src-github)'
    : src === 'jira' ? 'var(--src-jira)'
    : src === 'sentry' ? 'var(--src-sentry)'
    : src === 'file' ? 'var(--src-editor)'
    : src === 'chat-message' ? 'var(--src-claude)'
    : 'var(--accent)'}
  {@const srcLabel =
    src === 'github' ? 'PR / issue'
    : src === 'jira' ? 'Jira ticket'
    : src === 'sentry' ? 'Sentry issue'
    : src === 'file' ? 'file'
    : src === 'chat-message' ? 'message'
    : 'item'}
  <div class="drag-hint" role="status" aria-live="polite" style="--hint-tone: {tone};">
    <span class="drag-hint-dot"></span>
    Dragging {srcLabel} — drop on
    <strong>Claude</strong>, <strong>Cursor</strong>, or <strong>Canvas</strong>
  </div>
{/if}

<div id="app" class:is-dragging={dragState.payload !== null}>
  <Rail
    bind:view
    {anythingConnected}
    {statusLoading}
    {anyRetrying}
    {githubStatus}
    {jiraStatus}
    {sentryStatus}
    {claudeStatus}
    {cursorStatus}
    {githubBadge}
    {jiraBadge}
    {sentryBadge}
    {claudeBusy}
    {cursorBusy}
    dragActive={dragState.payload !== null}
    onAgentDrop={(kind, e) => onAgentDrop(
      kind === 'claude' ? APP_INSTANCE_IDS.claude : APP_INSTANCE_IDS.cursor,
      kind,
      e
    )}
  />

  <div class="main">

    <!-- Solo-switch flash. A short brand-tinted radial gradient that
         sweeps across `.main` on every `view` change so the user reads
         the context-switch (GitHub → Jira → Claude → …) as more than
         a content swap. `{#key view}` re-mounts the node on every nav,
         which restarts the CSS animation; the node has `pointer-events:
         none` so it never interferes with clicks/drag on the underlying
         solo. Sits above content (`z-index: 50`) but under all modals
         (which use 500+). -->
    {#key view}
      <div class="solo-flash" style="--flash-tone: {viewFlashTone};" aria-hidden="true"></div>
    {/key}

    <!-- Themed empty card — shown when a solo view's source isn't
         connected yet. -->

    {#snippet soloEmpty(label: string, tone: string, glow: string, blurb: string, kind: 'github' | 'jira' | 'sentry' | 'claude' | 'cursor')}
      <section class="full-center app-stub-shell" style="--app-tone: {tone}; --app-glow: {glow};">
        <div class="app-stub">
          <div class="app-stub-icon">
            <BrandIcon {kind} size={36} />
          </div>
          <h2 class="app-stub-title">{label}</h2>
          <p class="app-stub-sub">{blurb}</p>
          <div class="app-stub-actions">
            <button class="btn btn--primary" onclick={() => (view = 'connections')}>Open connections</button>
          </div>
        </div>
      </section>
    {/snippet}

    {#if view === 'home'}
      <HomeApp
        {now}
        onNavigate={(v) => (view = v)}
        onOpenSession={(sessionId, agentInstanceId) => {
          const sess = sessionsState.list.find((x) => x.id === sessionId);
          if (!sess) return;
          setActiveSessionInInstance(agentInstanceId, sessionId);
          view = sess.agentKind === 'cursor' ? 'cursorApp' : 'claudeApp';
        }}
        onNewChat={(kind) => {
          newClaudeSession({ agentKind: kind, agentInstanceId: APP_INSTANCE_IDS[kind] });
          view = kind === 'cursor' ? 'cursorApp' : 'claudeApp';
        }}
        onOpenWelcome={() => (welcomeOpen = true)}
      />

    {:else if view === 'githubApp'}
      {#if !connectedGithub}
        {@render soloEmpty('GitHub', 'var(--src-github)', 'rgba(181,132,255,0.40)', 'Connect GitHub first — paste a PAT in Connections to see PRs and issues.', 'github')}
      {:else}
        <GithubApp
          instanceId={APP_INSTANCE_IDS.github}
          {githubStatus}
          {now}
          {tab}
          {actionBusy}
          onSelect={selectInboxItem}
          onRefresh={() => refreshInbox(APP_INSTANCE_IDS.github)}
          onOpenCreatePr={openGithubCreatePr}
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
          {onDragStart}
          {onDragEnd}
          {onCardMouseDown}
          {isClickNotDrag}
          onSendToClaude={(item) => sendInboxItemToAgent({ kind: 'github', item }, 'claude')}
          onSendToCursor={(item) => sendInboxItemToAgent({ kind: 'github', item }, 'cursor')}
        />
      {/if}

    {:else if view === 'jiraApp'}
      {#if !connectedJira}
        {@render soloEmpty('Jira', 'var(--src-jira)', 'rgba(79,142,255,0.40)', 'Connect Jira first — workspace URL + email + API token in Connections.', 'jira')}
      {:else}
        <JiraApp
          instanceId={APP_INSTANCE_IDS.jira}
          {jiraStatus}
          {now}
          onRefresh={() => refreshJiraInbox(APP_INSTANCE_IDS.jira)}
          onOpenCreateIssue={openJiraCreateIssue}
          onOpenBrowser={openBrowser}
          {onDragStart}
          {onDragEnd}
          {onCardMouseDown}
          {isClickNotDrag}
          {refreshAllJiraInboxes}
          onSendToClaude={(item) => sendInboxItemToAgent({ kind: 'jira', item }, 'claude')}
          onSendToCursor={(item) => sendInboxItemToAgent({ kind: 'jira', item }, 'cursor')}
        />
      {/if}

    {:else if view === 'sentryApp'}
      {#if !connectedSentry}
        {@render soloEmpty('Sentry', 'var(--src-sentry)', 'rgba(110,80,155,0.40)', 'Connect Sentry first — host + organization slug + API token in Connections.', 'sentry')}
      {:else}
        <SentryApp
          instanceId={APP_INSTANCE_IDS.sentry}
          {sentryStatus}
          {now}
          onOpenBrowser={openBrowser}
          {onDragStart}
          {onDragEnd}
          {onCardMouseDown}
          {isClickNotDrag}
          onSendToClaude={(item) => sendInboxItemToAgent({ kind: 'sentry', item }, 'claude')}
          onSendToCursor={(item) => sendInboxItemToAgent({ kind: 'sentry', item }, 'cursor')}
        />
      {/if}

    {:else if view === 'rules'}
      <RulesView />

    {:else if view === 'library'}
      <LibraryApp />

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

    {:else if view === 'claudeApp'}
      {#if !connectedClaude}
        {@render soloEmpty('Claude', 'var(--src-claude)', 'rgba(232,155,125,0.42)', 'Connect Claude Code first — the agent needs a working CLI.', 'claude')}
      {:else}
        <AgentApp
          kind="claude"
          instanceId={APP_INSTANCE_IDS.claude}
          {now}
          thinkingStartedAt={thinkingStartedAt.claude}
          thinkingTick={thinkingTick.claude}
          {worktreeBusy}
          {editorRepoPath}
          onPickCwd={pickCwd}
          onClearCwd={clearCwd}
          onToggleEditorLink={toggleSessionEditorLink}
          onLinkToEditorInstance={linkActiveSessionToEditor}
          onSyncAgentToEditor={syncAgentToLinkedEditor}
          onSyncEditorToAgent={syncLinkedEditorToAgent}
          onToggleTerminalLink={toggleSessionTerminalLink}
          onLinkToTerminalInstance={linkActiveSessionToTerminal}
          onToggleCanvasLink={toggleSessionCanvasLink}
          onLinkToCanvas={linkActiveSessionToCanvas}
          onCreateWorktree={createWorktree}
          onOpenWorktreeDiff={openWorktreeDiff}
          onOpenWorktreeInEditor={openWorktreeInEditor}
          onCopyWorktreeBranch={copyWorktreeBranch}
          onRemoveWorktree={removeWorktree}
          onStartEditMessage={startEditMessage}
          onResendMessage={resendMessage}
          onUpdateAction={updateAction}
          onRemoveAction={dismissAction}
          onExecuteAction={executeAction}
          onOpenPrInWoom={openPrUrlInWoom}
          onSend={() => void sendClaudeMessage()}
          onStop={() => void stopAgentForKind('claude')}
          onPasteImages={(k, blobs) => pasteImagesIntoColumn(APP_INSTANCE_IDS.claude, k, blobs)}
          onDragOver={(e) => onAgentDragOver(APP_INSTANCE_IDS.claude, 'claude', e)}
          onDrop={(e) => onAgentDrop(APP_INSTANCE_IDS.claude, 'claude', e)}
          onDragLeave={() => onAgentDragLeave(APP_INSTANCE_IDS.claude)}
          onSddAdvance={onSddAdvance}
        />
      {/if}

    {:else if view === 'cursorApp'}
      {#if !connectedCursor}
        {@render soloEmpty('Cursor', 'var(--src-cursor)', 'rgba(220,220,220,0.30)', 'Cursor CLI not detected. Install Cursor and re-check connections.', 'cursor')}
      {:else}
        <AgentApp
          kind="cursor"
          instanceId={APP_INSTANCE_IDS.cursor}
          {now}
          thinkingStartedAt={thinkingStartedAt.cursor}
          thinkingTick={thinkingTick.cursor}
          {worktreeBusy}
          {editorRepoPath}
          onPickCwd={pickCwd}
          onClearCwd={clearCwd}
          onToggleEditorLink={toggleSessionEditorLink}
          onLinkToEditorInstance={linkActiveSessionToEditor}
          onSyncAgentToEditor={syncAgentToLinkedEditor}
          onSyncEditorToAgent={syncLinkedEditorToAgent}
          onToggleTerminalLink={toggleSessionTerminalLink}
          onLinkToTerminalInstance={linkActiveSessionToTerminal}
          onToggleCanvasLink={toggleSessionCanvasLink}
          onLinkToCanvas={linkActiveSessionToCanvas}
          onCreateWorktree={createWorktree}
          onOpenWorktreeDiff={openWorktreeDiff}
          onOpenWorktreeInEditor={openWorktreeInEditor}
          onCopyWorktreeBranch={copyWorktreeBranch}
          onRemoveWorktree={removeWorktree}
          onStartEditMessage={startEditMessage}
          onResendMessage={resendMessage}
          onUpdateAction={updateAction}
          onRemoveAction={dismissAction}
          onExecuteAction={executeAction}
          onOpenPrInWoom={openPrUrlInWoom}
          onSend={() => void sendClaudeMessage()}
          onStop={() => void stopAgentForKind('cursor')}
          onPasteImages={(k, blobs) => pasteImagesIntoColumn(APP_INSTANCE_IDS.cursor, k, blobs)}
          onDragOver={(e) => onAgentDragOver(APP_INSTANCE_IDS.cursor, 'cursor', e)}
          onDrop={(e) => onAgentDrop(APP_INSTANCE_IDS.cursor, 'cursor', e)}
          onDragLeave={() => onAgentDragLeave(APP_INSTANCE_IDS.cursor)}
          onSddAdvance={onSddAdvance}
        />
      {/if}

    {:else if view === 'editorApp'}
      <!-- Multi-instance: re-key on the active editor instance id so
           Svelte re-mounts EditorApp when the user picks a different
           instance from the rail popover. Keeps per-instance state
           cleanly isolated. -->
      {#key layoutState.activeInstance.editor}
        <EditorApp
          instanceId={layoutState.activeInstance.editor}
          onLinkToAgent={(agentId, sessionId) => linkEditorToAgent(layoutState.activeInstance.editor, agentId, sessionId)}
          onOpenClaude={() => (view = 'claudeApp')}
          onOpenSettings={() => (view = 'settings')}
          onQuickSend={quickSendToSession}
          onOpenSession={(sessionId, agentInstanceId) => {
            const sess = sessionsState.list.find((x) => x.id === sessionId);
            if (!sess) return;
            setActiveSessionInInstance(agentInstanceId, sessionId);
            view = sess.agentKind === 'cursor' ? 'cursorApp' : 'claudeApp';
          }}
        />
      {/key}

    {:else if view === 'canvasApp'}
      {#key layoutState.activeInstance.canvas}
        <CanvasApp
          instanceId={layoutState.activeInstance.canvas}
          onCardOpen={openCanvasCardSource}
          onOpenClaude={() => (view = 'claudeApp')}
          onQuickSend={quickSendToSession}
          onOpenSession={(sessionId, agentInstanceId) => {
            const sess = sessionsState.list.find((x) => x.id === sessionId);
            if (!sess) return;
            setActiveSessionInInstance(agentInstanceId, sessionId);
            view = sess.agentKind === 'cursor' ? 'cursorApp' : 'claudeApp';
          }}
        />
      {/key}

    {:else if view === 'terminalApp'}
      {#key layoutState.activeInstance.terminal}
        <TerminalApp
          instanceId={layoutState.activeInstance.terminal}
          cwd={editorRepoPath || null}
          onOpenClaude={() => (view = 'claudeApp')}
          onOpenCursor={() => (view = 'cursorApp')}
          onQuickSend={quickSendToSession}
          onOpenSession={(sessionId, agentInstanceId) => {
            const sess = sessionsState.list.find((x) => x.id === sessionId);
            if (!sess) return;
            setActiveSessionInInstance(agentInstanceId, sessionId);
            view = sess.agentKind === 'cursor' ? 'cursorApp' : 'claudeApp';
          }}
          onLinkSession={(sessionId) =>
            linkSessionToTerminal(layoutState.activeInstance.terminal, sessionId)}
          onUnlinkSession={unlinkSessionFromTerminal}
        />
      {/key}

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

<!-- Inbox focus state (PR / ticket / issue) is persisted per app and
     rendered inline in the matching source app's right pane. Leaving
     GitHub for Claude no longer pops the PR over the chat, and
     returning to GitHub keeps the same PR open. Agent tools that
     target a source item (`mcp__app__open_github_pr`, etc.) switch
     the view to that source app on the way in, so the inline pane is
     the single render path. -->


{#if worktreeDiffOpen && activeSession?.worktreePath && activeSession.worktreeRepo && activeSession.worktreeBranch}
  <WorktreeDiffModal
    repo={activeSession.worktreeRepo}
    sessionId={activeSession.id}
    branch={activeSession.worktreeBranch}
    onClose={() => (worktreeDiffOpen = false)}
  />
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
  bind:mode={paletteMode}
  setView={(v) => (view = v)}
  actions={paletteActions}
/>

<SearchInFilesOverlay
  bind:open={searchInFilesOpen}
  setView={(v) => (view = v)}
/>

<QuickOpenOverlay
  bind:open={quickOpenOpen}
  setView={(v) => (view = v)}
/>

<SymbolPickerOverlay
  bind:open={symbolPickerOpen}
  setView={(v) => (view = v)}
/>

{#if agentDashboardOpen}
  <AgentDashboard
    onClose={() => (agentDashboardOpen = false)}
    onActivate={(s) => {
      /* Route to the agent solo (Claude / Cursor) for the activated
         session. The dashboard already set the active session id; we
         only switch the view here so the user lands inside the chat. */
      view = s.agentKind === 'cursor' ? 'cursorApp' : 'claudeApp';
    }}
  />
{/if}

<style>
  .bg {
    position: fixed; inset: 0; pointer-events: none; z-index: 0;
    background:
      radial-gradient(ellipse 1200px 600px at 10% 0%, rgba(30, 58, 107, 0.18), transparent 60%),
      radial-gradient(ellipse 900px 500px at 90% 100%, rgba(168, 217, 184, 0.06), transparent 60%);
  }
  #app { position: relative; z-index: 1; display: grid; grid-template-columns: 56px 1fr; height: 100vh; }

  /* Touch ID / device-owner-auth gate shown at launch. Sits over the app
     (z-index 500) so the solo doesn't flash through before unlock —
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
    background: rgba(232, 130, 100, 0.1);
    border: 1px solid rgba(232, 130, 100, 0.25);
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
    /* Anchor for the solo-flash overlay (absolute child). */
    position: relative;
  }

  /* Solo-switch flash — brand-tinted radial gradient that scales in,
     fades, and self-cleans. Keyed on `view` in the template so the
     animation re-runs every nav. Pointer-events disabled so the
     overlay can't interfere with clicks, drag handles, or text
     selection in the underlying solo. */
  .solo-flash {
    position: absolute;
    inset: 0;
    z-index: 50;
    pointer-events: none;
    background:
      radial-gradient(
        ellipse 80% 70% at 50% 0%,
        color-mix(in srgb, var(--flash-tone, var(--accent)) 22%, transparent),
        transparent 60%
      );
    opacity: 0;
    animation: solo-flash-anim 420ms var(--ease-out, cubic-bezier(0.2, 0.7, 0.3, 1));
  }
  @keyframes solo-flash-anim {
    0%   { opacity: 0; transform: scaleY(0.6); }
    35%  { opacity: 1; }
    100% { opacity: 0; transform: scaleY(1.05); }
  }
  @media (prefers-reduced-motion: reduce) {
    .solo-flash { display: none; }
  }
  .full-center { flex: 1; display: flex; align-items: center; justify-content: center; padding: 40px; }
  .empty { display: flex; flex-direction: column; align-items: center; gap: 16px; text-align: center; max-width: 420px; }
  .empty-title {
    font-family: 'Geist', 'Inter', -apple-system, system-ui, sans-serif;
    font-size: 28px; font-weight: 600;
    margin: 14px 0 0; color: var(--text-0);
    letter-spacing: -0.02em; line-height: 1.18;
  }
  .empty-sub { font-size: 13.5px; color: var(--text-1); margin: 0; line-height: 1.55; max-width: 380px; }


  /* App stubs — themed empty state shown when a rail app
     view (Claude / Cursor / Editor / Canvas / Terminal) is selected
     before its full implementation lands. The card adopts the rail
     button's brand tone via --app-tone / --app-glow. */
  .app-stub-shell {
    background:
      radial-gradient(ellipse 1100px 700px at 4% 100%, color-mix(in srgb, var(--app-tone, var(--accent)) 14%, transparent), transparent 65%),
      radial-gradient(ellipse 900px 600px at 100% 0%, rgba(110, 90, 130, 0.05), transparent 60%);
  }
  .app-stub {
    max-width: 560px;
    text-align: center;
    padding: 44px 40px 36px;
    background: var(--bg-1);
    border: 1px solid var(--border-hi);
    border-radius: 18px;
    box-shadow:
      0 0 0 1px color-mix(in srgb, var(--app-tone, var(--accent)) 14%, transparent),
      var(--shadow-3, 0 24px 64px rgba(0,0,0,0.55));
  }
  .app-stub-icon {
    width: 64px; height: 64px;
    margin: 0 auto 20px;
    display: grid; place-items: center;
    border-radius: 16px;
    background: color-mix(in srgb, var(--app-tone, var(--accent)) 12%, var(--bg-2));
    color: var(--app-tone, var(--accent-bright));
    box-shadow:
      inset 0 0 0 1px color-mix(in srgb, var(--app-tone, var(--accent)) 32%, transparent),
      0 0 28px var(--app-glow, var(--accent-glow));
  }
  /* BrandIcon sets its own width/height — no per-svg sizing needed. */
  .app-stub-title {
    font-family: 'Geist', 'Inter', -apple-system, system-ui, sans-serif;
    font-size: 30px; font-weight: 600; letter-spacing: -0.02em;
    color: var(--text-0);
    margin: 0 0 12px;
  }
  .app-stub-sub {
    font-size: 14px; line-height: 1.55;
    color: var(--text-1);
    margin: 0 0 22px;
  }
  .app-stub-actions { display: flex; gap: 8px; justify-content: center; }

  /* Global drag-affordance banner — sits above #app so it's never
     occluded by a solo's own content. Source-tinted (Jira blue / GitHub
     purple / Sentry plum / Editor terracotta / Claude rust) via the
     inline `--hint-tone` set on render so the colour matches the chip
     the user is dragging. */
  .drag-hint {
    position: fixed;
    top: 14px; left: 50%;
    transform: translateX(-50%);
    z-index: 600;
    display: inline-flex;
    align-items: center;
    gap: 10px;
    padding: 8px 16px;
    font-size: 12.5px;
    font-weight: 500;
    color: var(--text-0);
    background: color-mix(in srgb, var(--bg-1) 92%, transparent);
    border: 1px solid color-mix(in srgb, var(--hint-tone, var(--accent)) 40%, transparent);
    border-radius: 999px;
    backdrop-filter: blur(10px);
    -webkit-backdrop-filter: blur(10px);
    box-shadow:
      0 6px 24px rgba(0, 0, 0, 0.35),
      0 0 0 4px color-mix(in srgb, var(--hint-tone, var(--accent)) 12%, transparent);
    pointer-events: none;
    animation: drag-hint-in 220ms var(--ease-out, ease-out);
  }
  .drag-hint strong {
    color: var(--hint-tone, var(--accent-bright));
    font-weight: 700;
  }
  .drag-hint-dot {
    width: 8px; height: 8px;
    border-radius: 50%;
    background: var(--hint-tone, var(--accent));
    box-shadow: 0 0 12px var(--hint-tone, var(--accent));
    animation: drag-hint-pulse 1.4s ease-in-out infinite;
  }
  @keyframes drag-hint-in {
    from { opacity: 0; transform: translate(-50%, -8px); }
    to   { opacity: 1; transform: translate(-50%, 0); }
  }
  @keyframes drag-hint-pulse {
    0%, 100% { opacity: 0.55; }
    50%      { opacity: 1; }
  }
  @media (prefers-reduced-motion: reduce) {
    .drag-hint { animation: none; }
    .drag-hint-dot { animation: none; opacity: 0.9; }
  }
</style>
