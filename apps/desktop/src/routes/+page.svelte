<script lang="ts">
  import { onMount, onDestroy, tick } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import { open as openDialog } from '@tauri-apps/plugin-dialog';
  import Sigil from '$lib/components/ui/Sigil.svelte';
  import WorktreeDiffModal from '$lib/components/editor/WorktreeDiffModal.svelte';
  import JiraDetailPane from '$lib/components/inbox/JiraDetailPane.svelte';
  import SentryDetailPane from '$lib/components/inbox/SentryDetailPane.svelte';
  import GithubFocusOverlay from '$lib/components/inbox/GithubFocusOverlay.svelte';
  import Rail from '$lib/components/ui/Rail.svelte';
  import RulesView from '$lib/views/RulesView.svelte';
  import ConnectionsView from '$lib/views/ConnectionsView.svelte';
  import SettingsView from '$lib/views/SettingsView.svelte';
  import RepositoriesView from '$lib/views/RepositoriesView.svelte';
  import TasksView from '$lib/views/TasksView.svelte';
  import IssuesView from '$lib/views/IssuesView.svelte';
  import CommandPalette from '$lib/components/ui/CommandPalette.svelte';
  import ModalsRoot from '$lib/components/modals/ModalsRoot.svelte';
  import GithubColumn from '$lib/components/workbench/GithubColumn.svelte';
  import JiraColumn from '$lib/components/workbench/JiraColumn.svelte';
  import SentryColumn from '$lib/components/workbench/SentryColumn.svelte';
  import AgentColumn from '$lib/components/workbench/AgentColumn.svelte';
  import EditorColumn from '$lib/components/workbench/EditorColumn.svelte';
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
    listInstancesOfKind,
    goToInstance,
    moveInstanceToWorkbench,
    registerInstanceRemovedHook
  } from '$lib/state/layout.svelte';
  import {
    sessionsState,
    persistSessionsEffect,
    persistRulesEffect,
    persistEditorInstanceStateEffect,
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
    refreshAllStatus
  } from '$lib/state/connections.svelte';
  import {
    inboxState,
    refreshInbox,
    refreshJiraInbox,
    refreshSentryInbox,
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
    setGithubMeLogin
  } from '$lib/state/inbox.svelte';
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

  type View = 'workbench' | 'repositories' | 'tasks' | 'issues' | 'rules' | 'connections' | 'settings';
  type DetailTab = 'conversation' | 'commits' | 'files' | 'reviews' | 'checks';

  // View & layout state
  let view = $state<View>('workbench');
  let paletteOpen = $state(false);
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
      await refreshAllStatus();
      if (connectedGithub) void refreshInbox();
      if (connectedJira) void refreshJiraInbox();
      if (connectedSentry) void refreshSentryInbox();
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

  // Repositories state lives in RepositoriesView now; parent keeps a handle
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

  /** Update an agent session's cwd safely. Both Claude CLI and cursor-agent
   *  scope conversations by *project directory* (cwd-derived), so resuming
   *  an old session id under a new cwd fails ("No conversation found with
   *  session ID …"). When the cwd actually changes:
   *    - Stash the current uuid under the old cwd in `cwdUuids` (only if
   *      it has a real CLI-side conversation, i.e. `claudeResumable`).
   *      This lets a future return to that cwd resume the original chat.
   *    - Look up the new cwd in the map. If we've been there before,
   *      restore that uuid and resume — full CLI memory of the prior
   *      thread, no recap needed.
   *    - Otherwise mint a fresh uuid for the new project AND snapshot
   *      the recent UI conversation into `cwdSwitchRecap` so the next
   *      turn's system prompt can prime the fresh CLI session with what
   *      was just being discussed. Without that the agent would lose
   *      conversational context every time the user moves between repos.
   *  `breakLink: true` also unlinks from the editor (used by manual user
   *  actions like `pickCwd`). */
  function applySessionCwd(
    sessionId: string,
    newCwd: string | null,
    opts: { breakLink?: boolean } = {}
  ) {
    const sess = sessionsState.list.find((s) => s.id === sessionId);
    if (!sess) return;
    const oldCwd = sess.cwd ?? null;
    const cwdChanged = (oldCwd ?? '') !== (newCwd ?? '');
    const patch: Partial<ClaudeSession> = { cwd: newCwd };
    if (cwdChanged) {
      // Stash departing cwd's uuid (only if it's a live CLI conversation).
      const map = { ...sess.cwdUuids };
      if (oldCwd && sess.claudeResumable) {
        map[oldCwd] = sess.claudeUuid;
      }
      const restoreUuid = newCwd ? map[newCwd] : null;
      if (restoreUuid) {
        // Returning to a project we've been in before — resume its
        // conversation so the CLI side has full memory of THIS project's
        // history. But still inject a recap describing what was discussed
        // in OTHER projects since we left this one — the user may have
        // worked on things in B that are relevant when returning to A.
        // The CLI's resumed conversation has no knowledge of B's turns;
        // the recap fills that gap.
        patch.claudeUuid = restoreUuid;
        patch.claudeResumable = true;
        patch.cwdSwitchRecap = buildCwdSwitchRecap(sess, oldCwd, newCwd, { resumed: true });
      } else {
        // Fresh project. New uuid, prime with recap of recent chatter so
        // the brand-new CLI conversation has continuity.
        patch.claudeUuid = genUuid();
        patch.claudeResumable = false;
        patch.cwdSwitchRecap = buildCwdSwitchRecap(sess, oldCwd, newCwd, { resumed: false });
      }
      patch.cwdUuids = map;
    }
    if (opts.breakLink) {
      patch.linkedToEditor = false;
      patch.linkedToEditorInstanceId = null;
    }
    updateSession(sessionId, patch);
  }

  /** Snapshot the last few user/assistant exchanges into a self-contained
   *  prose block, injected into the next turn's system prompt. Two
   *  flavours, both feeding off the same unified `sess.messages` history
   *  (which spans every cwd the session has visited):
   *    - `resumed: false` — fresh CLI conversation in a brand-new
   *      project. The CLI side has zero memory; the recap primes it
   *      with what was just being discussed.
   *    - `resumed: true` — returning to a project we've been in before.
   *      The CLI's resumed conversation already remembers THIS
   *      project's prior turns, but knows nothing of what was
   *      discussed elsewhere since we left. Recap fills that gap so
   *      cross-project work bleeds over (e.g. "we figured X in repo B
   *      that affects A").
   *  Each message is truncated to ~800 chars to bound the token cost.
   *  Returns null when there's nothing meaningful to recap. */
  function buildCwdSwitchRecap(
    sess: ClaudeSession,
    oldCwd: string | null,
    newCwd: string | null,
    opts: { resumed: boolean }
  ): string | null {
    const meaningful = sess.messages.filter(
      (m) => (m.role === 'user' || m.role === 'assistant') && m.content.trim().length > 0
    );
    if (meaningful.length === 0) return null;
    const recent = meaningful.slice(-6);
    const lines: string[] = [];
    if (opts.resumed) {
      lines.push("You're returning to a project you've been in before. Your CLI session here resumes with full memory of this project's prior chat. While you were elsewhere, the user had these other exchanges — they may relate to work here, or not:");
    } else {
      lines.push('Your cwd just changed mid-conversation. The CLI you run on uses a fresh session in the new project, so you have no memory of prior turns from its perspective. Forgehold preserved the last few exchanges below for continuity:');
    }
    if (oldCwd) lines.push(`- Previous cwd: ${oldCwd}`);
    if (newCwd) lines.push(`- ${opts.resumed ? 'Now back in' : 'New cwd'}: ${newCwd}`);
    lines.push('');
    lines.push('Recent exchanges (oldest → newest):');
    for (const m of recent) {
      const role = m.role === 'user' ? 'User' : 'You (assistant)';
      const text = m.content.trim();
      const trimmed = text.length > 800 ? `${text.slice(0, 799)}…` : text;
      lines.push(`${role}: ${trimmed}`);
    }
    lines.push('');
    if (opts.resumed) {
      lines.push("Continue from your remembered context. If anything from the cross-project chatter above touches the work in this repo, weave it in.");
    } else {
      lines.push("Continue from there with the new cwd in mind. The old project's files are no longer your working tree — if the user asks to keep working on the prior thread, your tools (Read/Write/Bash) are now scoped to the new project.");
    }
    return lines.join('\n');
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

  let refreshInterval: ReturnType<typeof setInterval> | null = null;
  let tickInterval: ReturnType<typeof setInterval> | null = null;

  // Wire the layout→sessions hook once. Any closed panel instance (via the X
  // button or workbench deletion) orphans its pinned sessions back to the
  // floating pool so they reattach elsewhere instead of vanishing.
  registerInstanceRemovedHook((id) => orphanSessionsForInstance(id));

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
    restorePanelState();
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
    if (refreshInterval) clearInterval(refreshInterval);
    if (tickInterval) clearInterval(tickInterval);
  });

  $effect(() => {
    // Feed the authed GitHub login to the inbox store so query-builders can
    // substitute it into `author:`, `assignee:` etc. (@me only works in
    // *some* contexts on the search API — using the literal login is the
    // safe path).
    setGithubMeLogin(githubStatus.kind === 'connected' ? githubStatus.user.login : null);
  });

  $effect(() => {
    if (connectedGithub) {
      if (!refreshInterval) {
        refreshInterval = setInterval(() => {
          void refreshInbox({ silent: true });
          if (connectedJira) void refreshJiraInbox({ silent: true });
        }, 60_000);
      }
    } else {
      if (refreshInterval) {
        clearInterval(refreshInterval);
        refreshInterval = null;
      }
      resetGithubInbox();
    }
  });

  $effect(() => {
    if (connectedJira && inboxState.jiraItems.length === 0 && !inboxState.jiraItemsLoading) {
      void refreshJiraInbox({ silent: true });
    } else if (!connectedJira) {
      // Only wipe the issue list on transient disconnects — keep the
      // user-picked assignee so reconnecting doesn't silently jump back to
      // "me". `resetJiraInbox` below is used by the explicit disconnect
      // button which *does* clear the assignee.
      inboxState.jiraItems = [];
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
      } else {
        e.dataTransfer.setData('text/plain', payload.path);
        attachDragChip(e, payload.isDir ? 'dir' : 'file', payload.name);
      }
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

    // 3) Ticket / Sentry drop from inbox. The file branch above already
    //    returned, so `internal` is one of the issue-shaped variants here.
    if (!internal || internal.source === 'file') {
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
    const aiPath = activeSession.worktreePath || activeSession.cwd || '';
    const editorPath =
      sessionsState.editorInstanceState[editorInstanceId]?.repoPath ?? '';
    const sharedPath = aiPath || editorPath;
    // Push AI's folder to the Editor when AI has one — this is the new
    // symmetry the user asked for.
    if (aiPath && aiPath !== editorPath) {
      setEditorRepoPath(aiPath, editorInstanceId);
    }
    updateSession(activeSession.id, {
      linkedToEditor: true,
      linkedToEditorInstanceId: editorInstanceId,
      cwd: sharedPath || null
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
      editorRepoPath). Opens the Editor column if hidden. */
  function openSessionFolderInEditor() {
    const path = activeSession?.worktreePath || activeSession?.cwd || editorRepoPath;
    if (!path) return;
    ensureEditorShowing(path);
  }

  /** Handle a click on a @file/@dir mention inside a rendered chat bubble.
      `path` is whatever the mention's @token resolved to — usually a path
      relative to the session's cwd/worktree/editor. We try each of those
      three roots, in priority order, until something exists on disk. */
  async function openMentionPath(path: string) {
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
    for (const abs of candidates) {
      try {
        const ok = await invoke<boolean>('fs_path_exists', { path: abs });
        if (ok) {
          ensureEditorShowing(abs);
          return;
        }
      } catch {
        // keep trying the next candidate
      }
    }
    // Last-ditch: open the first candidate anyway — the Editor will surface
    // its own "file not found" state if the path is wrong.
    if (candidates[0]) ensureEditorShowing(candidates[0]);
  }

  /** Open (or scroll to) a column of the given kind. Singleton kinds only
      ever get one instance. Multi-instance kinds open their first matching
      instance (or create one if none exist). Close via the X on the column
      itself. */
  function openColumn(kind: PanelKind) {
    void scrollKindIntoView(kind);
  }

  /** Spawn a new column instance of the given kind. For singletons it's a
      no-op (scrolls to the existing one). Bound to the "+" button next to
      multi-instance agent/editor pills. */
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
      const boundState = boundId ? sessionsState.editorInstanceState[boundId] : null;
      const fallback = firstInstanceOfKind('editor');
      const path = boundState?.repoPath
        ?? (fallback ? sessionsState.editorInstanceState[fallback.id]?.repoPath ?? null : null)
        ?? null;
      if (s.cwd !== path) {
        updateSession(s.id, { cwd: path });
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

  async function sendClaudeMessage() {
    const s = activeSession;
    if (!s || !s.input.trim() || s.sending) return;
    const text = s.input.trim();
    const id = s.id;
    appendSessionMessage(id, { role: 'user', content: text, at: new Date().toISOString() });
    // Auto-title from first user message when chat had no mentions
    const curr = sessionsState.list.find((x) => x.id === id);
    // Snapshot the mentions BEFORE clearing so we can still bake them into
    // the prompt below. The attachments strip disappears immediately after
    // send — that's what the user asked for.
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
    let prompt = text;
    if (mentionsSnapshot.length) {
      const ctx = mentionsSnapshot
        .map((m) => {
          if (m.source === 'file') {
            const abs = m.body ?? m.externalId;
            const kind = m.isDir ? 'directory' : isImagePath(abs) ? 'image' : 'file';
            const hint = kind === 'image'
              ? `This is an image — use the Read tool with its absolute path to view it inline.`
              : `You have Read / Glob / Grep tools — use them to inspect this ${kind} when relevant.`;
            return `Referenced ${kind}: @${m.externalId}\nAbsolute path: ${abs}\n${hint}`;
          }
          return `@${m.externalId} — ${m.title}` + (m.body ? `\n\n${m.body}` : '');
        })
        .join('\n\n----\n\n');
      prompt = `Referenced items:\n\n${ctx}\n\n----\n\nUser message:\n${text}`;
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
    const appContext = buildAgentAppContext(id);

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
        appContext,
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

  // Streaming-event dispatch lives in `$lib/stream/claudeStream.ts`. The
  // caller here just forwards assistant text deltas to the chat (session
  // store + scroll-to-bottom is the only DOM-coupled bit).
  function appendAssistantDelta(sessionId: string, delta: string) {
    appendToLastAssistant(sessionId, delta);
    void scrollChatBottom();
  }

  /** Build the per-turn app-context string we hand the agent as a system-
   *  prompt suffix. Lists every workbench's instances by name + id, with
   *  the editor's open path / agent's cwd, and editor↔agent links. Tells
   *  the agent which instance/session it's running in so "switch myself"
   *  has a meaning, and which sibling instances exist so it knows whether
   *  to add a NEW one or just change an existing one's path.
   *
   *  Re-derived on every turn from `layoutState` + `sessionsState` so it's
   *  always current. */
  function buildAgentAppContext(callingSessionId: string): string {
    const lines: string[] = [];
    lines.push(
      'You are running inside Forgehold, a desktop app where the user has '
        + 'organised work into workbenches (tabs of side-by-side columns). '
        + 'You can navigate the UI directly via the `mcp__app__*` tools.'
    );

    const calling = sessionsState.list.find((s) => s.id === callingSessionId);
    const callingInstanceId = calling?.columnInstanceId ?? null;

    // One-shot recap if the user just switched the agent's cwd. Cleared
    // after the turn ships (in sendClaudeMessage's success path).
    if (calling?.cwdSwitchRecap) {
      lines.push('');
      lines.push('---');
      lines.push(calling.cwdSwitchRecap);
      lines.push('---');
    }

    for (const wb of layoutState.workbenches) {
      const isActive = wb.id === layoutState.activeWorkbenchId;
      lines.push('');
      lines.push(`Workbench "${wb.name}"${isActive ? ' (ACTIVE)' : ''} — id ${wb.id}:`);
      if (wb.instances.length === 0) {
        lines.push('  (no columns)');
        continue;
      }
      for (const inst of wb.instances) {
        const meta: string[] = [`kind=${inst.kind}`, `name=${inst.name}`, `id=${inst.id}`];
        if (inst.kind === 'editor') {
          const path = sessionsState.editorInstanceState[inst.id]?.repoPath ?? '';
          meta.push(`repo_path=${path || '(none)'}`);
          // Show what agent sessions are linked to this editor.
          const linked = sessionsState.list
            .filter((s) => s.linkedToEditor && s.linkedToEditorInstanceId === inst.id)
            .map((s) => s.title || s.id.slice(0, 6));
          if (linked.length) meta.push(`linked_agents=[${linked.join(', ')}]`);
        }
        if (inst.kind === 'claude' || inst.kind === 'cursor') {
          // Find the active session bound to this column.
          const sessId = sessionsState.activeByInstance[inst.id] ?? null;
          const sess = sessId ? sessionsState.list.find((s) => s.id === sessId) : null;
          if (sess) {
            const effCwd = sess.worktreePath || sess.cwd
              || (sess.linkedToEditor && sess.linkedToEditorInstanceId
                ? sessionsState.editorInstanceState[sess.linkedToEditorInstanceId]?.repoPath
                : null)
              || '(inherits from editor or no cwd)';
            meta.push(`session=${sess.title || sess.id.slice(0, 6)}`);
            meta.push(`cwd=${effCwd}`);
            if (sess.linkedToEditor && sess.linkedToEditorInstanceId) {
              const link = wb.instances.find((i) => i.id === sess.linkedToEditorInstanceId);
              if (link) meta.push(`linked_to_editor=${link.name}`);
            }
          }
        }
        const isYou = inst.id === callingInstanceId;
        lines.push(`  - ${meta.join(', ')}${isYou ? '  ← THIS IS YOU' : ''}`);
      }
    }

    lines.push('');
    lines.push(
      'When the user asks to "switch the editor and claude", "open this '
        + 'repo in editor", "switch myself to /path", etc — DO NOT add a new '
        + 'column. Use these tools on existing instances:'
    );
    lines.push(
      '  - `mcp__app__set_editor_repo_path` — change an editor\'s open '
        + 'folder. Pass `instance_name` (the art-name like "Sagrada-Familia") '
        + 'or `instance_id`. If the editor has linked agents, their cwd '
        + 'auto-follows — see the `linked_agents=[…]` field on each editor '
        + 'in the preamble above. So if your column is in `linked_agents` of '
        + 'the editor you\'re moving, you DON\'T need a separate set_agent_cwd '
        + 'for yourself — the link handles it.'
    );
    lines.push(
      '  - `mcp__app__set_agent_cwd` — change an agent session\'s cwd. '
        + 'Pass `instance_name`/`instance_id`, or `target=self` for yourself. '
        + 'For yourself, the change takes effect on your NEXT turn. The '
        + 'editor↔agent link is NEVER broken by this call — only by the '
        + 'user clicking "Unlink" in the UI.'
    );
    lines.push(
      '  - `mcp__app__list_instances` — re-list the current state if you '
        + 'think this preamble is stale.'
    );
    lines.push(
      'Only use `mcp__app__add_workbench_instance` when the user explicitly '
        + 'says "add", "new", "another" — not for "switch" / "open in".'
    );
    lines.push('');
    lines.push(
      'Approval cards: `set_editor_repo_path` and `set_agent_cwd` execute '
        + 'immediately when the USER asked you to switch — no approval card. '
        + 'If you want to PROACTIVELY suggest a switch (the user didn\'t '
        + 'ask but you think they should), use `mcp__github__propose_switch_cwd` '
        + 'instead — that one queues an approval card.'
    );
    return lines.join('\n');
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
        // Type-safe: view is the View union, validated server-side too.
        if (v) view = v as View;
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
        const validKinds: PanelKind[] = ['github', 'jira', 'sentry', 'claude', 'cursor', 'editor'];
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
        const validKinds: PanelKind[] = ['github', 'jira', 'sentry', 'claude', 'cursor', 'editor'];
        if (!validKinds.includes(kind as PanelKind)) return;
        view = 'workbench';
        void scrollKindIntoView(kind as PanelKind);
        return;
      }
      case 'mcp__app__open_repo': {
        const owner = str('owner');
        const repo = str('repo');
        const section = str('section') || 'pulls';
        if (!owner || !repo) return;
        view = 'repositories';
        // RepositoriesView watches this slot and clears it after opening.
        inboxState.pendingRepoNav = { owner, repo, section };
        return;
      }
      case 'mcp__app__set_editor_repo_path': {
        const repoPath = str('repo_path');
        const instName = str('instance_name');
        const instId = str('instance_id');
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
        const repoPath = str('repo_path');
        if (!repoPath) return;
        const target = str('target').toLowerCase();
        let sessId: string | null = null;
        if (target === 'self') {
          sessId = _sessionId;
        } else {
          const instName = str('instance_name');
          const instId = str('instance_id');
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
    }
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
        body: `${target.name} already has a column of that kind, or the source disappeared.`
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
    // Truncate everything from this message onward, then re-send with the
    // new text. That way mentions stay in sync with what was truly asked.
    // Also make sure the session is active so sendClaudeMessage picks it up.
    sessionsState.activeClaudeId = sessionId;
    truncateSessionAt(sessionId, index);
    setSessionInput(sessionId, trimmed);
    await sendClaudeMessage();
  }

  async function resendMessage(sessionId: string, index: number, content: string) {
    const ok = confirm(
      'Resend this message?\n\nEverything after it (Claude\'s replies, your later messages, pending action cards) will be erased.'
    );
    if (!ok) return;
    sessionsState.activeClaudeId = sessionId;
    truncateSessionAt(sessionId, index);
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
    if (!sess || sess.sending) return;
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
      // Mark awaitingApproval again if the continuation also added
      // pending action cards — chains of commit → PR are common.
      const sessAfter2 = sessionsState.list.find((s) => s.id === sessionId);
      const stillPending = sessAfter2?.actions.some((a) => a.status === 'pending') ?? false;
      if (stillPending) patch.awaitingApproval = true;
      updateSession(sessionId, patch);
    } catch (e) {
      const msg = typeof e === 'string' ? e : String(e);
      replaceLastAssistant(
        sessionId,
        `**${sess.agentKind === 'cursor' ? 'Cursor' : 'Claude'} failed:** ${msg}`
      );
    }
    stopThinkingTimer();
    updateSession(sessionId, { sending: false });
    void scrollChatBottom();
    // Best-effort — keeps the session-store invariant consistent if
    // an error message-bubble was just stamped.
    void runStartedAt;
  }

  // ---- Claude stub flow ----
  // Real agent execution is the next milestone. For now we simulate the
  // drop → run → commit → open-PR pipeline so the UX is testable.

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
    // Ask RepositoriesView to refresh its list if a repo is currently open
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
      closeModal('jiraConnect');
      await refreshJiraStatus();
    } catch (e) {
      patchModal('jiraConnect', { busy: false, error: typeof e === 'string' ? e : String(e) });
    }
  }

  async function disconnectJira() {
    await invoke('jira_disconnect');
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
      closeModal('sentryConnect');
      await refreshSentryStatus();
      void refreshSentryInbox();
    } catch (e) {
      patchModal('sentryConnect', { busy: false, error: typeof e === 'string' ? e : String(e) });
    }
  }

  async function disconnectSentryAll() {
    try {
      await invoke('sentry_disconnect');
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
      closeModal('pat');
      await refreshInbox();
      view = 'workbench';
    } catch (e) {
      patchModal('pat', { busy: false, error: typeof e === 'string' ? e : String(e) });
    }
  }

  async function disconnectGithub() {
    try {
      await invoke('github_disconnect');
      await refreshGithubStatus();
      resetGithubInbox();
      notify({ kind: 'success', title: 'Disconnected from GitHub' });
    } catch (e) {
      notifyError(e, { title: 'GitHub disconnect failed' });
    }
    // Repo state is owned by RepositoriesView — it wipes itself via its
    // `$effect` on `connectedGithub` becoming false.
  }

  async function disconnectJiraAll() {
    try {
      await invoke('jira_disconnect');
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
    } else if (e.key === 'j' && view === 'workbench' && !anyModalOpen()) {
      moveSelection(1);
    } else if (e.key === 'k' && view === 'workbench' && !(e.metaKey || e.ctrlKey) && !anyModalOpen()) {
      moveSelection(-1);
    }
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
      paletteOpen
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
    const active = inboxState.jiraFilters;
    openModal('jiraCreate', {
      projectKey: active.projectKey ?? '',
      projects: inboxState.jiraProjectOptions,
      projectsLoading: false,
      issueTypes: [],
      issueTypeName: 'Task',
      summary: '',
      description: '',
      assigneeAccountId: '',
      sprints: inboxState.jiraSprintOptions,
      sprintId: typeof active.sprintId === 'number' ? active.sprintId : null,
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
    patchModal('jiraCreate', { projectKey: key, issueTypes: [] });
    if (!key) return;
    try {
      const types = await invoke<JiraIssueType[]>('jira_list_issue_types', { projectKey: key });
      const m = modalsState.jiraCreate;
      if (!m) return;
      // Keep a sensible default issue type name — prefer whatever the user
      // already had picked if it's still valid, otherwise first type from
      // the API, otherwise hard-coded "Task".
      const preserved = types.find((t) => t.name === m.issueTypeName);
      const nextName = preserved ? preserved.name : types[0]?.name ?? 'Task';
      patchModal('jiraCreate', { issueTypes: types, issueTypeName: nextName });
    } catch {
      // ignore — modal falls back to hardcoded Task/Bug/Story
    }
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
      // Optimistically push the new issue onto the current list, then refresh
      // to pick up server-side ordering.
      inboxState.jiraItems = [created, ...inboxState.jiraItems];
      closeModal('jiraCreate');
      void refreshJiraInbox({ silent: true });
    } catch (e) {
      patchModal('jiraCreate', { busy: false, error: typeof e === 'string' ? e : String(e) });
    }
  }

  // ---- GitHub Create PR ----

  async function openGithubCreatePr() {
    const activeRepo = inboxState.githubFilters.repo;
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
      // Optimistically push onto inbox and open focus pane.
      inboxState.items = [created, ...inboxState.items];
      openFocusItem(created);
      view = 'workbench';
      void refreshInbox({ silent: true });
    } catch (e) {
      patchModal('githubCreatePr', { busy: false, error: typeof e === 'string' ? e : String(e) });
    }
  }
</script>

<svelte:window onkeydown={onKey} />

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
    inboxCount={inboxState.items.length}
    {anythingConnected}
    {statusLoading}
    {githubStatus}
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
            {@const count = insts.length}
            {@const inCurrent = insts.some((i) => i.workbenchId === layoutState.activeWorkbenchId)}
            <div
              class="pill-group"
              class:active={inCurrent}
              class:dim={count === 0}
              class:has-menu={count > 0}
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
                {#if meta?.iconSvg}
                  <span class="pill-icon {meta.iconClass}"><svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">{@html meta.iconSvg}</svg></span>
                {:else if kind === 'editor'}
                  <span class="pill-icon pill-icon--editor"><svg viewBox="0 0 24 24"><path d="M4 4h7v7H4zM13 4h7v7h-7zM4 13h7v7H4zM13 13h7v7h-7z"/></svg></span>
                {/if}
                <span class="pill-label">{label}</span>
                {#if count > 0}
                  <span class="pill-count mono">{count}</span>
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
              {#if count > 0}
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

          <div style="flex:1"></div>
          <button class="icon-btn" title="Search" aria-label="Search" onclick={() => (paletteOpen = true)}>
            <svg class="i i-sm" viewBox="0 0 24 24"><circle cx="11" cy="11" r="7" /><path d="m20 20-3-3" /></svg>
          </button>
        </div>
        <div class="wb-columns">
          {#each instances as inst (inst.id)}
            {#if inst.kind === 'github' && connectedGithub}
              <GithubColumn
                instanceId={inst.id}
                {githubStatus}
                {now}
                {tab}
                {actionBusy}
                onSelectInboxItem={selectInboxItem}
                onRefreshInbox={() => refreshInbox()}
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
                onRefreshJiraInbox={() => refreshJiraInbox()}
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
              />
            {:else if inst.kind === 'editor'}
              <EditorColumn
                instanceId={inst.id}
                onLinkToAgent={(agentId) => linkEditorToAgent(inst.id, agentId)}
              />
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

    {:else if view === 'repositories'}
      <RepositoriesView
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

    {:else if view === 'tasks'}
      <TasksView
        {jiraStatus}
        bind:view
        {now}
        onOpenCreateIssue={openJiraCreateIssue}
      />

    {:else if view === 'issues'}
      <IssuesView
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
     view — workbench, repositories, tasks, issues, etc. The earlier
     per-view mounts only rendered when their owning component was on
     screen, which broke the `mcp__app__open_*` navigation tools (PR
     opened in the focus state but no overlay rendered until the user
     manually flipped to Repositories). -->
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
        onStatusChange={() => void refreshJiraInbox({ silent: true })}
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
  onSelect={(id) => { selectInboxItem(id); paletteOpen = false; view = 'workbench'; }}
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
    background: rgba(12, 20, 34, 0.5);
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
    background: transparent;
    cursor: pointer;
    border: 1px solid transparent;
    border-bottom: none;
    transition: all 120ms;
    flex-shrink: 0;
    max-width: 200px;
  }
  .wb-tab:hover { color: var(--text-0); background: var(--bg-1); }
  .wb-tab.active {
    color: var(--text-0);
    background: rgba(15, 24, 40, 0.35);
    border-color: var(--border-neutral);
  }
  /* Drag-over highlight when the user is moving a column onto this tab. */
  .wb-tab.drag-over {
    color: #1a0a04;
    background: var(--accent);
    border-color: var(--accent);
    box-shadow: 0 0 0 2px var(--accent), 0 0 12px var(--accent-glow);
  }
  .wb-tab.drag-over .wb-tab-count { background: rgba(26, 10, 4, 0.18); color: #1a0a04; }
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
    background: rgba(15, 24, 40, 0.35);
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
  .pill-group:hover { border-color: var(--border-neutral-hi); background: var(--bg-2); }
  .pill-group.active {
    border-color: var(--border-hi);
    background: var(--bg-2);
    box-shadow: 0 0 0 1px var(--border-hi), 0 1px 8px rgba(232, 163, 58, 0.08);
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
    color: #1a0a04;
  }
  .pill-menu-item.drag-over .pill-menu-dot {
    background: #1a0a04; box-shadow: none;
  }
  .pill-menu-item.drag-over .pill-menu-wb {
    background: rgba(26, 10, 4, 0.2); color: #1a0a04; border-color: transparent;
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
</style>
