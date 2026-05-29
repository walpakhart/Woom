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
    blobToBase64,
    deriveCwd,
    formatBytesShort,
    groupAgentSessions as _groupAgentSessions,
    guessExt,
    imageFilesFromEvent,
  } from './page_helpers';
  import {
    coerceString,
    INSTANCE_ID_KEYS_DEEP,
    INSTANCE_NAME_KEYS_DEEP,
    num as _mcpNum,
    pickDeep,
    pickFrom,
    REPO_PATH_KEYS_DEEP,
    str as _mcpStr,
  } from './mcpInputParse';
  import { handleCanvasOrSddMcp } from './appNavigationCanvas';
  import { handleInboxOrViewMcp } from './appNavigationInbox';
  import { handleSlashCommand as handleSlashCommandImpl } from './handleSlashCommand';
  import * as _modalActions from './modalActions';
  import * as _agentDrop from './agentDrop';
  import * as _worktree from './worktreeActions';
  import * as _sessionLinks from './sessionLinks';
  import * as _agentTurn from './agentTurn';
  import { createSendClaudeMessage as _createSendClaudeMessage } from './sendClaudeMessage';
  import * as _kbd from './keyboardShortcuts';
  import { buildPaletteActions } from './paletteActions';
  import {
    isGithubFilterMode,
    isSentryStatus,
    isSentryLevel,
    mapAgentViewToInternal,
    parseSprintScopes,
    type SentryStatus,
    type SentryLevel,
    type SentryFilterPatch,
  } from './mcpTypeGuards';
  import {
    actionMatchesIpcParams,
    buildActionFromIpcRequest,
    type ActionRequestPayload,
  } from './actionIpcConverters';
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
  import { quotaState, refreshPlanUsage, nextResetAt } from '$lib/state/quota.svelte';
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
    genUuid,
    setAwaitingResume,
    clearResumeState
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
    refreshAllStatusOnBoot,
    refreshAgentsOnBoot
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
  import {
    initSdd,
    workspaceForSession,
    refreshSdd,
    sddState,
    closeStandaloneView,
    saveSddPhasePlan,
    completeSddPhaseImplement,
    saveSddPhaseVerify,
    approveSddPhasePlan,
    discardSddPhasePlan,
    setSddAutoFireDispatcher
  } from '$lib/state/sdd.svelte';
  import { dwState, updateWorkflow, loadPersistedWorkflows } from '$lib/state/dw.svelte';
  import type { DynamicWorkflow } from '$lib/types';
  import SddCard from '$lib/components/agent/SddCard.svelte';
  import {
    initUpdatesStore,
    updateState as updatesPhaseStore,
    installNow as updatesInstallNow,
    installOnQuit as updatesInstallOnQuit,
    snooze as updatesSnooze,
    skipVersion as updatesSkipVersion,
  } from '$lib/state/updates.svelte';
  import UpdateNotesPane from '$lib/components/ui/UpdateNotesPane.svelte';
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

  /* groupAgentSessions moved to ./page_helpers.ts (wave-15 split). */
  function groupAgentSessions(kind: 'claude' | 'cursor', nowMs: number) {
    return _groupAgentSessions(sessionsState.list, kind, nowMs);
  }

  // Thinking-time label for the typing indicator — keyed by session id
  // so two chats of the same kind (or one Claude + one Cursor) each get
  // an independent timer. Earlier the maps were keyed by `kind` which
  // meant starting one Claude chat's run made every other Claude chat
  // show the same elapsed-seconds counter.
  let thinkingStartedAt = $state<Record<string, number | null>>({});
  let thinkingTick = $state<Record<string, number>>({});
  const thinkingTimers: Record<string, ReturnType<typeof setInterval> | null> = {};

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

  /* Top-level palette actions — extracted to ./paletteActions.ts
   * (phase-9 split). The derived re-runs whenever `connectionsState`
   * changes (live read inside `buildPaletteActions`), so the
   * connect/disconnect verb flips correctly. */
  const paletteActions = $derived.by(() =>
    buildPaletteActions({
      connectionsState,
      sourceConns,
      agentConns,
      openConnectModal,
      disconnectGithub,
      disconnectJiraAll,
      disconnectSentryAll,
      openCheatsheet: () => (cheatsheetOpen = true),
      setView: (v) => (view = v),
    })
  );
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
  /** Unlisten for the `claude:bg_done` event fired by `claude_bg.rs`
   *  when a Claude CLI background task's output file goes idle. The
   *  handler fires a silent continuation prompt so the agent picks up
   *  the bg task's tail output and continues working. */
  let claudeBgUnlisten: UnlistenFn | null = null;
  /* Window-close lifecycle handles. Both are unlisten-style — see
     the close-flush hook inside onMount for what they catch. */
  let tauriCloseUnlisten: UnlistenFn | null = null;
  let dwDoneUnlistenRef: UnlistenFn | null = null;
  let dwRecoverUnlistenRef: UnlistenFn | null = null;
  let beforeUnloadHandler: (() => void) | null = null;
  let closeFlushInProgress = false;

  /* ActionRequestPayload moved to ./actionIpcConverters.ts */

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

  /* actionMatchesIpcParams + buildActionFromIpcRequest moved to
   * ./actionIpcConverters.ts (wave-3 phase-9 split). */

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

  /* Auto-update UX wiring — drives the sticky toast + release-notes
   * pane off the `updateState.phase` store from `$lib/state/updates`.
   * Sticky toast appears when a new version is available; clicking
   * "View" opens the UpdateNotesPane lightbox. Phase 4 of the SDD
   * update-system workspace (`sdd-2508eeb82e`). */
  let showUpdateNotesPane = $state(false);
  let lastToastedVersion = $state<string | null>(null);
  let lastFailedReason = $state<string | null>(null);
  $effect(() => {
    const phase = updatesPhaseStore.phase;

    // Failed state — surface a red toast with a "Open releases page"
    // recovery action. Dedup by reason so a poll loop emitting the
    // same failure twice doesn't stack toasts. Phase 5 task 5.
    if (phase.kind === 'failed') {
      if (lastFailedReason === phase.reason) return;
      lastFailedReason = phase.reason;
      notify({
        kind: 'error',
        title: 'Update failed',
        body: phase.reason,
        ttlMs: null,
        actions: [
          {
            label: 'Open releases page',
            onClick: () => {
              const url = phase.version
                ? `https://github.com/walpakhart/Woom/releases/tag/v${phase.version}`
                : 'https://github.com/walpakhart/Woom/releases';
              void invoke('plugin:opener|open_url', { url }).catch(() => {});
            },
          },
        ],
      });
      return;
    }

    if (phase.kind !== 'available') {
      // Reset the dedup gates when the live state goes back to
      // idle/up-to-date so a future re-emit of the same version
      // (or the same failure after a manual retry) re-arms cleanly.
      if (phase.kind !== 'snoozed' && phase.kind !== 'skipped') {
        lastToastedVersion = null;
        lastFailedReason = null;
        showUpdateNotesPane = false;
      }
      return;
    }
    // Dedup at the version level — same version emitted twice by the
    // poll loop reuses the existing toast (toaster.svelte's own dedup
    // refreshes the TTL, but actions captured the OLD closure so we
    // bail entirely here when we already toasted this version).
    if (lastToastedVersion === phase.version) return;
    lastToastedVersion = phase.version;
    notify({
      kind: 'info',
      title: `Woom ${phase.version} available`,
      body: 'New release ready to install.',
      ttlMs: null,
      actions: [
        { label: 'View', onClick: () => { showUpdateNotesPane = true; } },
        { label: 'Snooze 24h', onClick: () => { void updatesSnooze(24); } },
        { label: 'Skip', onClick: () => { void updatesSkipVersion(phase.version); } },
      ],
    });
  });

  /* Quota guard watchdog (SDD `sdd-98a42f3bdb` Phase 2). 30s polling
   * gated on "any session is sending OR awaiting resume". On tick:
   * (a) refresh quota snapshot, (b) for each session, trip the SIGTERM
   * path if utilization crossed 95% mid-stream, or clear the resume
   * state + auto-fire the queued prompt if it dropped back under 95%.
   * Cleanup on app unmount. */
  let quotaWatchdog: ReturnType<typeof setInterval> | null = null;
  /* Always-on idle refresh for the 5H/7D quota pills. The watchdog
   * above only polls while a turn is in-flight, so when the app sits
   * idle the pills froze at their last value and never self-updated.
   * This keeps them live (skips when the tab is hidden; refreshPlanUsage
   * still honours the 60s freshness gate + 429 backoff, so it can't
   * hammer the endpoint). */
  let quotaIdleInterval: ReturnType<typeof setInterval> | null = null;
  $effect(() => {
    const anyActive = sessionsState.list.some(
      (s) => s.sending || s.awaitingResume
    );
    if (anyActive) {
      if (!quotaWatchdog) {
        quotaWatchdog = setInterval(() => {
          void refreshPlanUsage().then(() => checkQuotaWatchdog());
        }, 30_000);
      }
    } else if (quotaWatchdog) {
      clearInterval(quotaWatchdog);
      quotaWatchdog = null;
    }
    return () => {
      if (quotaWatchdog) {
        clearInterval(quotaWatchdog);
        quotaWatchdog = null;
      }
    };
  });

  async function checkQuotaWatchdog() {
    const pct5h = quotaState.usage?.five_hour?.utilization ?? 0;
    const pct7d = quotaState.usage?.seven_day?.utilization ?? 0;
    const hot = pct5h >= 95 || pct7d >= 95;
    /* Snapshot ids first — auto-fire mutates sessionsState.list under
     * us during the loop. Iterate by id, not by index. */
    const snapshot = sessionsState.list.map((s) => s.id);
    for (const id of snapshot) {
      const s = sessionsState.list.find((x) => x.id === id);
      if (!s || s.agentKind !== 'claude') continue;
      if (s.sending && hot && !s.awaitingResume) {
        const fallback = Date.now() + 30 * 60_000;
        const resumeAt = nextResetAt(quotaState.usage) ?? fallback;
        /* Trip: SIGTERM the claude CLI, mark the last assistant
         * message as interrupted-by-quota, flip session to
         * awaitingResume. The next message's `interrupted` field
         * is what ChatThread reads to render ResumePill. */
        try {
          await invoke('claude_stop', { sessionId: s.id });
        } catch {
          /* claude_stop returns false if no runner — no-op */
        }
        const last = s.messages[s.messages.length - 1];
        if (last && last.role === 'assistant') {
          const msgs = [...s.messages];
          msgs[msgs.length - 1] = { ...last, interrupted: 'quota' as const };
          updateSession(s.id, { messages: msgs });
        }
        setAwaitingResume(s.id, resumeAt, 'quota');
        continue;
      }
      if (s.awaitingResume && !hot) {
        /* Quota recovered. Clear resume state; if a prompt is queued,
         * auto-fire it as a Claude-kind send. The send pipeline takes
         * `pendingQueue[0]` automatically on its next `s.sending` flip
         * — clearing `sending: false` plus the queued entry triggers
         * the existing queue-while-sending drain in `sendClaudeMessage`'s
         * finally block. */
        clearResumeState(s.id);
        if ((s.pendingQueue ?? []).length > 0) {
          /* Pop queue head into composer input + fire send. Same
           * shape as the ResumePill click path. */
          const entry = s.pendingQueue![0];
          const rest = s.pendingQueue!.slice(1);
          updateSession(s.id, {
            input: entry.text,
            mentions: entry.mentions ?? [],
            pendingQueue: rest
          });
          /* Sequential await — multiple sessions auto-resuming on the
           * same tick fan out one at a time so we don't thrash the
           * spawn pool. */
          try {
            await sendClaudeMessage({ kind: 'claude' });
          } catch (e) {
            console.warn('quota auto-resume send failed', e);
          }
        } else if (s.pendingActionResults.length > 0) {
          /* Paused mid-turn by the continuation quota-guard. No queued
           * user prompt — re-fire the auto-continuation to drain the
           * waiting action-results. */
          try {
            await continueAgentTurn(s.id);
          } catch (e) {
            console.warn('quota auto-resume continuation failed', e);
          }
        }
      }
    }
  }

  /** ResumePill click handler — same drain semantics as the auto-fire
   *  branch in `checkQuotaWatchdog`, manually triggered. */
  function onResumeAfterQuota(sessionId: string) {
    const s = sessionsState.list.find((x) => x.id === sessionId);
    if (!s) return;
    clearResumeState(s.id);
    if ((s.pendingQueue ?? []).length > 0) {
      const entry = s.pendingQueue![0];
      const rest = s.pendingQueue!.slice(1);
      updateSession(s.id, {
        input: entry.text,
        mentions: entry.mentions ?? [],
        pendingQueue: rest
      });
      void sendClaudeMessage({ kind: 'claude' });
    } else if (s.pendingActionResults.length > 0) {
      /* Paused mid-turn by the continuation quota-guard — no queued
       * user prompt, but undrained action-results are waiting. Re-fire
       * the auto-continuation so the turn picks up where it stopped. */
      void continueAgentTurn(s.id);
    }
  }

  onMount(async () => {
    /* Re-apply the persisted theme on boot — the SSR shell rendered
       with default `:root` vars, this flips `<html data-theme="…">`
       so the saved palette wins on first paint. */
    initTheme();
    /* Updater auto-check default — OFF for now. Phase 1 of the SDD
     * update-system workspace (`sdd-2508eeb82e`) lands the manifest +
     * pubkey scaffolding; Phase 3 reads this key from a real Rust
     * settings store. Until then we set a localStorage default so any
     * Phase 4 UI that gates on the flag finds an explicit value
     * instead of `null`. NEVER overwrite an existing setting — the
     * user may have flipped it on after we ship. */
    try {
      if (typeof localStorage !== 'undefined' &&
          localStorage.getItem('woom.updates.auto_check') === null) {
        localStorage.setItem('woom.updates.auto_check', 'false');
      }
    } catch { /* localStorage can throw in some sandboxed contexts; safe to ignore */ }
    // Install a window-level dragend/drop listener that clears the
    // drag payload on any cancel — defense in depth against future
    // drag sources forgetting their own ondragend wiring.
    installGlobalDragSafetyNet();
    // Background-task store — wires the global `bg:tasks-changed`
    // listener so the Preview pane (right side of Claude/Cursor solo)
    // refreshes when a process spawns / exits anywhere in the app.
    void initBgTasks();
    /* Register the auto-fire dispatcher BEFORE initSdd so hydrate-time
     *  catch-up (workspace left mid-substep across app restart) can
     *  reach the chat send pipeline immediately. */
    setSddAutoFireDispatcher(async (sessionId, prompt) => {
      const s = sessionsState.list.find((x) => x.id === sessionId);
      if (!s) return;
      if (s.sending) {
        /* Active turn in flight — park in the silent slot and let the
         *  end-of-turn drain fire it. Same lane SddCard's Approve
         *  click uses while a turn is running. */
        const { setPendingSilent } = await import('$lib/state/sdd.svelte');
        setPendingSilent(sessionId, prompt);
        return;
      }
      await onSddAdvance(sessionId, prompt);
    });
    void initSdd();
    /* Updater state store — subscribes to the `update:state` Tauri
     * event so the Settings card + the Phase 4 toast read live state
     * from the Rust-side background poll. No-op if init has already
     * fired (HMR re-mount safety). */
    void initUpdatesStore();
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
    /* Claude CLI bg-task auto-resume. When the agent ran `Bash` with
     *  `run_in_background:true`, its turn ends immediately after the
     *  "Command running in background…" message — without a kick, the
     *  agent would never wake up when the build/test finishes.
     *  `claude_bg.rs` polls the output file's mtime and emits when it
     *  goes idle; we fold the tail into a silent continuation prompt
     *  so the agent picks up and decides whether the task is truly
     *  done or needs another `BashOutput` call. */
    claudeBgUnlisten = await listen<{
      session_id: string;
      task_id: string;
      output_path: string;
      tail: string;
      timed_out: boolean;
    }>('claude:bg_done', async (event) => {
      const { forgetBgTask } = await import('$lib/stream/agentStream');
      const { session_id, task_id, tail, timed_out } = event.payload;
      // Drop the registry entry first so a second mtime tick (or a
      // refire on rapid re-watch) doesn't double-fire.
      forgetBgTask(session_id, task_id);
      const sess = sessionsState.list.find((x) => x.id === session_id);
      if (!sess) return;
      // Cap the tail we paste into the prompt — Rust already capped
      // at 8 KB, this is a defensive second cap so a runaway log
      // can't blow the agent's context window.
      const capped = tail.length > 4000 ? `…${tail.slice(tail.length - 4000)}` : tail;
      const header = timed_out
        ? `[Woom: bg task ${task_id} hit the 30-minute watcher cap — may still be running]`
        : `[Woom: bg task ${task_id} output went idle — likely complete]`;
      const promptText = [
        header,
        '',
        'Output tail:',
        '```',
        capped.trim() || '(empty)',
        '```',
        '',
        'Continue from where you were. Run `BashOutput` on the task if you need more.',
      ].join('\n');
      // Sequence: set input → focus session → fire silent. The send
      // path reads session.input. If the session is busy, the silent
      // branch parks the prompt in `pendingSilentBySession` and the
      // post-turn drain picks it up.
      updateSession(session_id, { input: promptText });
      sessionsState.activeIds[sess.agentKind] = session_id;
      sessionsState.activeClaudeId = session_id;
      await sendClaudeMessage({ silent: true, kind: sess.agentKind });
    });
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
        /* DW running-workflow guard (Phase 5). Probe the registry
           BEFORE flushing — if a /dw workflow is in flight, surface
           the styled ConfirmModal. Cancel keeps the window open
           (reset the flush flag so a second close-request can
           re-enter); Confirm SIGTERMs every in-flight workflow then
           proceeds with the normal flush + destroy inside onConfirm.
           Probe failure is non-fatal — fall through to the flush as
           if no workflows were running. */
        let probe: { count: number; ids: string[] } | null = null;
        try {
          probe = await invoke<{ count: number; ids: string[] }>('dw_has_running');
        } catch (e) {
          console.warn('dw_has_running probe failed', e);
        }
        if (probe && probe.count > 0) {
          const flushDeadlineInner = () =>
            new Promise<void>((resolve) => setTimeout(resolve, 2_500));
          openModal('confirm', {
            title: `${probe.count} Dynamic Workflow${probe.count === 1 ? '' : 's'} still running`,
            body: 'Close anyway? Subagents will be cancelled and partial transcripts saved.',
            confirmText: 'Close anyway',
            danger: true,
            busy: false,
            onConfirm: async () => {
              try {
                await invoke<number>('dw_cancel_all');
              } catch (e) {
                console.warn('dw_cancel_all failed', e);
              }
              try {
                await Promise.race([flushSessionsNow(), flushDeadlineInner()]);
              } catch { /* best effort */ }
              try {
                await win.destroy();
              } catch {
                try { await win.close(); } catch { /* tearing down */ }
              }
            },
            onCancel: () => {
              closeFlushInProgress = false;
            }
          });
          return;
        }
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
    quotaIdleInterval = setInterval(() => {
      if (typeof document !== 'undefined' && document.hidden) return;
      void refreshPlanUsage();
    }, 90_000);
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
    _agentDrop.setCachedAppDataDir(appDataDir);
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
    // Pre-warm agent detection BEFORE biometric unlock — Claude /
    // Cursor `--version` is local-only (no keychain, no network) so
    // it's safe to run during the lock screen. Cold-launch on macOS
    // routinely needs 1–3 s for the first child spawn to actually
    // return; running this in parallel with the Touch ID prompt
    // means by the time the user unlocks, `connectionsState.claude`
    // is already populated instead of `null`, and the agent pane
    // renders straight into the AgentApp instead of flashing the
    // "Connect Claude Code first" empty state. Wrap in the same
    // boot-retry loop so the first slow spawn still recovers.
    void refreshAgentsOnBoot().catch(() => {});
    // Biometric gate runs second — refreshAllStatus + inbox fetches live
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

    /* DW persistence (Phase 5) — hydrate workflows from disk before
     * wiring the live event listeners so backend-emitted updates land
     * on already-mounted entries instead of triggering a no-op. */
    void loadPersistedWorkflows();
    /* Recovery banner — backend emits once on startup with the count
     * of workflows that died non-terminally on the previous shutdown. */
    const dwRecoverUnlisten = await listen<{ count: number }>(
      'dw:recovered_interrupted',
      (e) => {
        const n = e.payload?.count ?? 0;
        if (n <= 0) return;
        notify({
          kind: 'info',
          title: `${n} workflow${n === 1 ? '' : 's'} interrupted on last close`,
          body: 'Reload the chat to inspect partial transcripts.'
        });
      }
    );
    dwRecoverUnlistenRef = dwRecoverUnlisten;

    /* Dynamic Workflows — workflow-done listener. Folds the verifier
     * synthesis back into the parent chat as a SEPARATE assistant
     * ClaudeMessage (per Phase 4 spec — synthesis behaves like any
     * normal claude reply: copy / drag / context-menu / quoted into
     * the next turn). The DW card stays focused on per-subagent
     * progress; this listener mirrors `final_answer` from the workflow
     * payload into the chat transcript. */
    const dwDoneUnlisten = await listen<DynamicWorkflow>('dw:workflow_done', (e) => {
      const wf = e.payload as DynamicWorkflow | (Partial<DynamicWorkflow> & { workflowId?: string; error?: string });
      if (!wf) return;
      // Error path: backend emits `{ workflowId, error }` when fanout or
      // verifier fails. Surface as a system message so the user sees
      // why no synthesis arrived.
      if ((wf as { error?: string }).error) {
        const wfId = (wf as { workflowId?: string }).workflowId;
        const target = wfId
          ? dwState.workflows.find((w) => w.id === wfId)
          : null;
        if (target) {
          appendSessionMessage(target.sessionId, {
            role: 'system',
            content: `_Dynamic Workflow failed: ${(wf as { error: string }).error}_`,
            at: new Date().toISOString()
          });
        }
        return;
      }
      const full = wf as DynamicWorkflow;
      // Keep reactive state in lockstep with the backend snapshot.
      updateWorkflow(full.id, {
        status: full.status,
        verifierResult: full.verifierResult,
        finalAnswer: full.finalAnswer,
        completedAt: full.completedAt
      });
      if (full.status === 'done' && full.finalAnswer && full.finalAnswer.trim().length > 0) {
        appendSessionMessage(full.sessionId, {
          role: 'assistant',
          content: full.finalAnswer,
          at: new Date().toISOString()
        });
      }
    });
    dwDoneUnlistenRef = dwDoneUnlisten;
  });

  /* formatBytesShort moved to ./page_helpers.ts */

  onDestroy(() => {
    if (githubPollInterval) clearInterval(githubPollInterval);
    if (jiraPollInterval) clearInterval(jiraPollInterval);
    if (sentryPollInterval) clearInterval(sentryPollInterval);
    if (tickInterval) clearInterval(tickInterval);
    if (connectionRefreshInterval) clearInterval(connectionRefreshInterval);
    if (quotaIdleInterval) clearInterval(quotaIdleInterval);
    removeFocusListener?.();
    actionIpcUnlisten?.();
    claudeBgUnlisten?.();
    tauriCloseUnlisten?.();
    dwDoneUnlistenRef?.();
    dwRecoverUnlistenRef?.();
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
  // attachBlobsToSession + pasteImagesIntoColumn moved to ./agentDrop.ts
  // (wave-35 split).
  const attachBlobsToSession = (sessionId: string, blobs: { name: string; type: string; blob: Blob }[]) =>
    _agentDrop.attachBlobsToSession(sessionId, blobs);
  const pasteImagesIntoColumn = (instanceId: string, kind: 'claude' | 'cursor', blobs: { name: string; type: string; blob: Blob }[]) =>
    _agentDrop.pasteImagesIntoColumn(instanceId, kind, blobs);

  /* imageFilesFromEvent moved to ./page_helpers.ts */

  // onAgentDrop moved to ./agentDrop.ts (wave-35 split). Deps inject
  // route-local justDragged setter + clearAgentDragState closure.
  const onAgentDrop = (instanceId: string, kind: 'claude' | 'cursor', e: DragEvent) =>
    _agentDrop.onAgentDrop(instanceId, kind, e, {
      setJustDragged: (v) => { justDragged = v; },
      clearAgentDragState,
    });

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
  /* deriveCwd moved to ./page_helpers.ts */

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
    clearContinuationInFlight(sessionId);
    deleteClaudeSession(sessionId);
  }

  // Session-link helpers moved to ./sessionLinks.ts (wave-37 split).
  const _linkDeps = (): import('./sessionLinks').SessionLinkDeps => ({
    getActiveSession: () => activeSession,
    setEditorRepoPath,
  });
  const linkActiveSessionToEditor = (id: string) => _sessionLinks.linkActiveSessionToEditor(id, _linkDeps());
  const syncAgentToLinkedEditor = () => _sessionLinks.syncAgentToLinkedEditor(_linkDeps());
  const syncLinkedEditorToAgent = () => _sessionLinks.syncLinkedEditorToAgent(_linkDeps());
  const toggleSessionEditorLink = () => _sessionLinks.toggleSessionEditorLink(_linkDeps());
  const linkActiveSessionToTerminal = (id: string) => _sessionLinks.linkActiveSessionToTerminal(id, _linkDeps());
  const toggleSessionTerminalLink = () => _sessionLinks.toggleSessionTerminalLink(_linkDeps());
  const linkSessionToTerminal = (tid: string, sid: string) => _sessionLinks.linkSessionToTerminal(tid, sid);
  const unlinkSessionFromTerminal = (sid: string) => _sessionLinks.unlinkSessionFromTerminal(sid);
  const linkActiveSessionToCanvas = (cid: string) => _sessionLinks.linkActiveSessionToCanvas(cid, _linkDeps());
  const toggleSessionCanvasLink = () => _sessionLinks.toggleSessionCanvasLink(_linkDeps());
  const linkEditorToAgent = (eid: string, aid: string, sid?: string) => _sessionLinks.linkEditorToAgent(eid, aid, sid);

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

  async function createWorktree(): Promise<void> {
    return _worktree.createWorktree(_worktreeDeps());
  }

  async function removeWorktree(): Promise<void> {
    return _worktree.removeWorktree(_worktreeDeps());
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

  async function copyWorktreeBranch(): Promise<void> {
    return _worktree.copyWorktreeBranch(_worktreeDeps());
  }

  let worktreeDiffOpen = $state(false);
  function openWorktreeDiff() {
    worktreeMenuOpen = false;
    worktreeDiffOpen = true;
  }

  async function applyWorktree(): Promise<void> {
    return _worktree.applyWorktree(_worktreeDeps());
  }
  const _worktreeDeps = (): import('./worktreeActions').WorktreeDeps => ({
    getActiveSession: () => activeSession,
    getEditorRepoPath: () => editorRepoPath,
    setWorktreeBusy: (s) => { worktreeBusy = s; },
    setWorktreeMenuOpen: (v) => { worktreeMenuOpen = v; },
  });

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

  function startThinkingTimer(sessionId: string) {
    thinkingStartedAt[sessionId] = Date.now();
    thinkingTick[sessionId] = 0;
    if (thinkingTimers[sessionId]) clearInterval(thinkingTimers[sessionId]!);
    thinkingTimers[sessionId] = setInterval(() => {
      thinkingTick[sessionId] = (thinkingTick[sessionId] ?? 0) + 1;
    }, 1000);
  }

  function stopThinkingTimer(sessionId: string) {
    if (thinkingTimers[sessionId]) {
      clearInterval(thinkingTimers[sessionId]!);
      thinkingTimers[sessionId] = null;
    }
    thinkingStartedAt[sessionId] = null;
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
  // handleSlashCommand moved to ./handleSlashCommand.ts (wave-33 split).
  const handleSlashCommand = (text: string, session: ClaudeSession) =>
    handleSlashCommandImpl(text, session, {
      sendClaudeMessage,
      scrollChatBottom,
      runCompactSession,
    });

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

  // queueSavedDrafts moved into the sendClaudeMessage factory deps
  // (wave-39); shim below declares + passes it.

  /** SDD card "Approve & continue" / "Next phase" click handler.
   *  Pushes the next-stage prompt into the agent's CLI session
   *  SILENTLY — the giant phase/plan/spec template is recorded in the
   *  agent's `--resume` transcript so context inheritance works, but
   *  the visible chat thread skips it via `hidden: true`. Agent's
   *  reply stays visible (usually a short "Phase N done.").
   *
   *  We still go through `sendClaudeMessage` (with `silent: true`) so
   *  all the post-turn plumbing — sending=false, statusline refresh,
   *  SDD refresh, queue drain — runs identically to a manual user
   *  turn. SDD shouldn't fork a parallel pipeline; it just gets to
   *  borrow the existing one. */
  async function onSddAdvance(sessionId: string, prompt: string): Promise<void> {
    const s = sessionsState.list.find((x) => x.id === sessionId);
    if (!s) return;
    /* Pin the active-session pointers to the SDD session BEFORE the
     *  send fires. `sendClaudeMessage` resolves its target via
     *  `activeSession` (= `activeClaudeId`); without this, if the
     *  user switched chats during the streaming phase the silent
     *  send would target the wrong session and leave the SDD
     *  session's `input` populated with the orchestrator prompt
     *  while the spinner stays up on no actual run. */
    sessionsState.activeClaudeId = sessionId;
    sessionsState.activeIds[s.agentKind] = sessionId;
    updateSession(sessionId, { input: prompt });
    await Promise.resolve();
    await sendClaudeMessage({ silent: true, kind: s.agentKind });
  }

  /** Options for `sendClaudeMessage`. `silent` is used by the SDD
   *  orchestrator to push phase prompts into the agent's CLI session
   *  WITHOUT polluting the visible chat with the giant template — the
   *  user-message bubble is marked `hidden: true` so ChatThread skips
   *  it. Agent's reply stays visible (it's short and useful — usually
   *  "Phase N done."). Skips slash-command parsing, the UserPromptSubmit
   *  hook, and mention baking — SDD prompts are internal and shouldn't
   *  go through user-prompt-shaped middleware. */
  type SendOpts = { silent?: boolean; kind?: 'claude' | 'cursor' };
  // sendClaudeMessage moved to ./sendClaudeMessage.ts (wave-39 split).
  // The factory returns a self-aware closure so recursive retries
  // + post-turn queue drain resolve to the same fn reference.
  const queueSavedDrafts = new Map<string, { text: string; mentions: import('$lib/types').Mention[] }>();
  const sendClaudeMessage = _createSendClaudeMessage({
    getActiveSession: () => activeSession,
    getEditorRepoPath: () => editorRepoPath,
    startThinkingTimer,
    stopThinkingTimer,
    scrollChatBottom,
    appendAssistantDelta,
    handleAppNavigation,
    handleSlashCommand,
    buildStatusLinePayload,
    queueSavedDrafts,
    getAttachmentDir: () => _agentDrop.getAttachmentDir(),
  });

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
  /* MCP type guards + parsers moved to ./mcpTypeGuards.ts (phase-9 split). */
  function handleAppNavigation(
    _sessionId: string,
    name: string,
    input: Record<string, unknown>
  ) {
    /* Input-parser shims — input-bound thin wrappers around the
     * shared helpers in `./mcpInputParse.ts` (wave-1 phase-9 split).
     * Kept as local closures so every existing `case 'foo':` block
     * can keep calling `str('key')` / `pick('a','b')` without a
     * mechanical rewrite. */
    const str = (k: string): string => _mcpStr(input, k);
    const num = (k: string): number => _mcpNum(input, k);
    const pick = (...keys: string[]): string => pickFrom(input, ...keys);
    // parseEdgeSpec moved to ./mcpInputParse.ts (wave-30 split).
    // Inbox/view/instance cases moved to ./appNavigationInbox.ts
    // (wave-32 split). Canvas + SDD cases moved to
    // ./appNavigationCanvas.ts (wave-31 split).
    if (handleInboxOrViewMcp(_sessionId, name, input, {
      setView: (v) => { view = v as View; },
      setTab: (t) => { tab = t as DetailTab; },
      mapAgentViewToInternal,
      findInstanceByNameOrId,
      setEditorRepoPath,
      resolveGithubFocus,
      openConnectModal,
      isGithubFilterMode,
      isSentryStatus,
      isSentryLevel,
      parseSprintScopes,
    })) return;
    if (handleCanvasOrSddMcp(_sessionId, name, input, { linkedCanvasIdFor })) return;
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
  // executeAction / dismissAction / onActionResolved / continueAgentTurn
  // moved to ./agentTurn.ts (wave-38 split). continuationInFlight Set
  // moved with them — kept module-local in agentTurn so the route
  // file doesn't import it directly.
  const _turnDeps = (): import('./agentTurn').AgentTurnDeps => ({
    getEditorRepoPath: () => editorRepoPath,
    startThinkingTimer,
    stopThinkingTimer,
    appendAssistantDelta,
    scrollChatBottom,
    handleAppNavigation,
  });
  const executeAction = (sid: string, action: ClaudeAction) =>
    _agentTurn.executeAction(sid, action, _turnDeps());
  const dismissAction = (sid: string, aid: string) =>
    _agentTurn.dismissAction(sid, aid);
  const onActionResolved = (sid: string, action: ClaudeAction, result: { ok: boolean; summary: string }) =>
    _agentTurn.onActionResolved(sid, action, result, _turnDeps());
  const continueAgentTurn = (sid: string) =>
    _agentTurn.continueAgentTurn(sid, _turnDeps());
  const clearContinuationInFlight = (sid: string) =>
    _agentTurn.clearContinuationInFlight(sid);
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
      cwdSwitchRecap: recap,
      /* Force-clear `sending` so the composer leaves the "thinking…"
       *  state even when the CLI was SIGKILLed without emitting a
       *  stream-end event. Without this the thinking dots stick
       *  forever and the next Send falls into the queue-while-busy
       *  branch instead of firing. */
      sending: false
    });
    stopThinkingTimer(kind);
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
  // Modal action handlers moved to ./modalActions.ts (wave-34 split).
  // The route file keeps thin shims that wire route-local setters
  // (view, actionBusy) + derived agent statuses via a deps object.
  const _modalDeps = (): import('./modalActions').ModalActionDeps => ({
    setView: (v) => { view = v as View; },
    setActionBusy: (s) => { actionBusy = s; },
    reloadDetailAndLists,
    getClaudeStatus: () => claudeStatus,
    getCursorStatus: () => cursorStatus,
  });
  const submitComment = () => _modalActions.submitComment();
  const submitReview = () => _modalActions.submitReview();
  const submitMerge = () => _modalActions.submitMerge();
  const setState = (s: 'closed' | 'open') => _modalActions.setIssueState(s, _modalDeps());
  const askClose = () => _modalActions.askClose(_modalDeps());
  const openConnectModal = (conn: ConnectionMeta) => _modalActions.openConnectModal(conn, _modalDeps());
  const refreshClaudeModal = () => _modalActions.refreshClaudeModal(_modalDeps());
  const refreshCursorModal = () => _modalActions.refreshCursorModal(_modalDeps());
  const cursorInstallUrl = () => _modalActions.cursorInstallUrl();
  const claudeInstallUrl = () => _modalActions.claudeInstallUrl();
  const jiraTokenUrl = () => _modalActions.jiraTokenUrl();
  const sentryTokenUrl = () => _modalActions.sentryTokenUrl();
  const submitJira = () => _modalActions.submitJira();
  const disconnectJira = () => _modalActions.disconnectJira();
  const submitSentry = () => _modalActions.submitSentry();
  const disconnectSentryAll = () => _modalActions.disconnectSentryAll();
  const submitPat = () => _modalActions.submitPat(_modalDeps());
  const disconnectGithub = () => _modalActions.disconnectGithub();
  const disconnectJiraAll = () => _modalActions.disconnectJiraAll();
  const openBrowser = (u: string) => _modalActions.openBrowser(u);

  // onKey + focusedRowUrl + isTextInput + anyModalOpen + mergeDisabled
  // moved to ./keyboardShortcuts.ts (wave-40 split). Big deps interface
  // because every binding flips a different `let`-state local.
  const onKey = _kbd.createOnKey({
    getView: () => view,
    setView: (v) => { view = v as View; },
    setPaletteMode: (m) => { paletteMode = m; },
    togglePaletteOpen: () => { paletteOpen = !paletteOpen; },
    setPaletteOpen: (open) => { paletteOpen = open; },
    SOLO_BY_DIGIT,
    toggleAgentDashboard: () => { agentDashboardOpen = !agentDashboardOpen; },
    toggleSearchInFiles: () => { searchInFilesOpen = !searchInFilesOpen; },
    toggleQuickOpen: () => { quickOpenOpen = !quickOpenOpen; },
    toggleSymbolPicker: () => { symbolPickerOpen = !symbolPickerOpen; },
    toggleCheatsheet: () => { cheatsheetOpen = !cheatsheetOpen; },
    setCheatsheet: (o) => { cheatsheetOpen = o; },
    toggleWelcome: () => { welcomeOpen = !welcomeOpen; },
    setWelcome: (o) => { welcomeOpen = o; },
    setSearchInFiles: (o) => { searchInFilesOpen = o; },
    setQuickOpen: (o) => { quickOpenOpen = o; },
    setSymbolPicker: (o) => { symbolPickerOpen = o; },
    isSourceApp: () => isSourceApp,
    isWelcomeOpen: () => welcomeOpen,
    isCheatsheetOpen: () => cheatsheetOpen,
    isPaletteOpen: () => paletteOpen,
    isSearchInFilesOpen: () => searchInFilesOpen,
    isQuickOpenOpen: () => quickOpenOpen,
    isSymbolPickerOpen: () => symbolPickerOpen,
    navBack,
    navForward,
    stopAgentForKind,
    getPatModal: () => patModal,
    getJiraModal: () => jiraModal,
    getClaudeModal: () => claudeModal,
    getCommentModal: () => commentModal,
    getReviewModal: () => reviewModal,
    getMergeModal: () => mergeModal,
    getCommitModal: () => commitModal,
    getConfirmModal: () => confirmModal,
    getJiraCreateModal: () => jiraCreateModal,
    getGithubCreatePrModal: () => githubCreatePrModal,
  });
  const focusedRowUrl = _kbd.focusedRowUrl;
  const isTextInput = _kbd.isTextInput;
  const mergeDisabled = _kbd.mergeDisabled;

  // ---- Jira Create Issue ----

  // --- Create issue / PR (modalActions) ---
  const openJiraCreateIssue = () => _modalActions.openJiraCreateIssue();
  const onJiraCreateProjectChange = (k: string) => _modalActions.onJiraCreateProjectChange(k);
  const submitJiraCreate = () => _modalActions.submitJiraCreate();
  const openGithubCreatePr = () => _modalActions.openGithubCreatePr();
  const onGithubPrRepoChange = (f: string) => _modalActions.onGithubPrRepoChange(f);
  const onGithubPrBranchesChange = () => _modalActions.onGithubPrBranchesChange();
  const submitGithubPr = () => _modalActions.submitGithubPr(_modalDeps());
  const githubTokenUrl = () => _modalActions.githubTokenUrl();
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
        {#if claudeStatus === null || connectionsState.retrying.claude}
          <!-- Boot detection still in flight — don't flash the "Connect"
               empty state. Cold-launch on macOS routinely needs 1–3 s
               for the first `claude --version` to return; the boot
               retry covers up to 22 s. Show a passive "detecting"
               cue so the user doesn't think the app is broken. -->
          <section class="full-center app-stub-shell" style="--app-tone: var(--src-claude); --app-glow: rgba(232,155,125,0.42);">
            <div class="app-stub">
              <div class="app-stub-icon">
                <BrandIcon kind="claude" size={36} />
              </div>
              <h2 class="app-stub-title">Claude</h2>
              <p class="app-stub-sub">Detecting Claude Code CLI…</p>
            </div>
          </section>
        {:else}
          {@render soloEmpty('Claude', 'var(--src-claude)', 'rgba(232,155,125,0.42)', 'Connect Claude Code first — the agent needs a working CLI.', 'claude')}
        {/if}
      {:else}
        <AgentApp
          kind="claude"
          instanceId={APP_INSTANCE_IDS.claude}
          {now}
          thinkingStartedAt={thinkingStartedAt}
          thinkingTick={thinkingTick}
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
          onSend={() => void sendClaudeMessage({ kind: 'claude' })}
          onStop={() => void stopAgentForKind('claude')}
          onPasteImages={(k, blobs) => pasteImagesIntoColumn(APP_INSTANCE_IDS.claude, k, blobs)}
          onDragOver={(e) => onAgentDragOver(APP_INSTANCE_IDS.claude, 'claude', e)}
          onDrop={(e) => onAgentDrop(APP_INSTANCE_IDS.claude, 'claude', e)}
          onDragLeave={() => onAgentDragLeave(APP_INSTANCE_IDS.claude)}
          onSddAdvance={onSddAdvance}
          onResumeAfterQuota={onResumeAfterQuota}
        />
      {/if}

    {:else if view === 'cursorApp'}
      {#if !connectedCursor}
        {#if cursorStatus === null || connectionsState.retrying.cursor}
          <section class="full-center app-stub-shell" style="--app-tone: var(--src-cursor); --app-glow: rgba(220,220,220,0.30);">
            <div class="app-stub">
              <div class="app-stub-icon">
                <BrandIcon kind="cursor" size={36} />
              </div>
              <h2 class="app-stub-title">Cursor</h2>
              <p class="app-stub-sub">Detecting Cursor CLI…</p>
            </div>
          </section>
        {:else}
          {@render soloEmpty('Cursor', 'var(--src-cursor)', 'rgba(220,220,220,0.30)', 'Cursor CLI not detected. Install Cursor and re-check connections.', 'cursor')}
        {/if}
      {:else}
        <AgentApp
          kind="cursor"
          instanceId={APP_INSTANCE_IDS.cursor}
          {now}
          thinkingStartedAt={thinkingStartedAt}
          thinkingTick={thinkingTick}
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
          onSend={() => void sendClaudeMessage({ kind: 'cursor' })}
          onStop={() => void stopAgentForKind('cursor')}
          onPasteImages={(k, blobs) => pasteImagesIntoColumn(APP_INSTANCE_IDS.cursor, k, blobs)}
          onDragOver={(e) => onAgentDragOver(APP_INSTANCE_IDS.cursor, 'cursor', e)}
          onDrop={(e) => onAgentDrop(APP_INSTANCE_IDS.cursor, 'cursor', e)}
          onDragLeave={() => onAgentDragLeave(APP_INSTANCE_IDS.cursor)}
          onSddAdvance={onSddAdvance}
          onResumeAfterQuota={onResumeAfterQuota}
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

{#if showUpdateNotesPane && updatesPhaseStore.phase.kind === 'available'}
  <UpdateNotesPane
    version={updatesPhaseStore.phase.version}
    notes={updatesPhaseStore.phase.notes}
    pubDate={updatesPhaseStore.phase.pub_date}
    onInstallNow={() => {
      showUpdateNotesPane = false;
      void updatesInstallNow();
    }}
    onInstallOnQuit={() => {
      showUpdateNotesPane = false;
      void updatesInstallOnQuit();
    }}
    onClose={() => { showUpdateNotesPane = false; }}
  />
{/if}

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

<!-- Standalone read-only SDD overlay — opened from the header history
     popover. Renders as a top-level fullscreen card with only a Close
     button (no Discard / Approve), so the user can read past specs
     without touching the active chat. -->
{#if sddState.standaloneViewWorkspaceId}
  {@const standaloneWs = sddState.workspaces.find((w) => w.id === sddState.standaloneViewWorkspaceId)}
  {#if standaloneWs}
    <SddCard
      workspace={standaloneWs}
      viewOnly={true}
      onClose={closeStandaloneView}
      onAdvance={() => {}}
    />
  {/if}
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
