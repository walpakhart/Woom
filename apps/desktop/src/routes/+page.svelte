<script lang="ts">
  import { onMount, onDestroy, tick } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import { open as openDialog } from '@tauri-apps/plugin-dialog';
  import Sigil from '$lib/Sigil.svelte';
  import WorktreeDiffModal from '$lib/WorktreeDiffModal.svelte';
  import JiraDetailPane from '$lib/JiraDetailPane.svelte';
  import Rail from '$lib/Rail.svelte';
  import RulesView from '$lib/views/RulesView.svelte';
  import ConnectionsView from '$lib/views/ConnectionsView.svelte';
  import RepositoriesView from '$lib/views/RepositoriesView.svelte';
  import TasksView from '$lib/views/TasksView.svelte';
  import CommandPalette from '$lib/CommandPalette.svelte';
  import Modals from '$lib/modals/Modals.svelte';
  import GithubColumn from '$lib/workbench/GithubColumn.svelte';
  import JiraColumn from '$lib/workbench/JiraColumn.svelte';
  import AgentColumn from '$lib/workbench/AgentColumn.svelte';
  import EditorColumn from '$lib/workbench/EditorColumn.svelte';
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
    registerInstanceRemovedHook
  } from '$lib/state/layout.svelte';
  import {
    sessionsState,
    persistSessionsEffect,
    persistRulesEffect,
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
    refreshClaudeStatus,
    refreshAllStatus
  } from '$lib/state/connections.svelte';
  import {
    inboxState,
    refreshInbox,
    refreshJiraInbox,
    loadDetail,
    reloadDetailAndLists as reloadDetailAndListsCore,
    selectInboxItem,
    openFocusItem,
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
    type RepoBranch,
    type Repository
  } from '$lib/data';
  import { basename, formatToolUse, isImagePath, truncInline } from '$lib/format';

  type View = 'workbench' | 'repositories' | 'tasks' | 'rules' | 'connections' | 'settings';
  type DetailTab = 'conversation' | 'commits' | 'files' | 'reviews' | 'checks';
  type ReviewEvent = 'APPROVE' | 'REQUEST_CHANGES' | 'COMMENT';
  type MergeMethod = 'merge' | 'squash' | 'rebase';

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

  // ---- Drag autoscroll for card DnD (Jira/GitHub → Claude column) ----
  // When the user grabs a card and drags it toward an off-screen column,
  // the workbench auto-scrolls so the drop target comes into view.
  let dragPointerX = $state(0);
  let dragAutoscrollRaf: number | null = null;

  function trackDragPointer(e: DragEvent) {
    dragPointerX = e.clientX;
    if (dragAutoscrollRaf === null && dragPayload) {
      dragAutoscrollRaf = requestAnimationFrame(dragAutoscrollStep);
    }
  }

  function dragAutoscrollStep() {
    if (!dragPayload) { dragAutoscrollRaf = null; return; }
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


  // Drag state
  type DragPayload =
    | { source: 'github'; item: InboxItem }
    | { source: 'jira'; item: JiraItem };
  let dragPayload = $state<DragPayload | null>(null);
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

  // Claude column state (stubbed agent flow)
  type ClaudeStaged = {
    source: 'github' | 'jira';
    title: string;
    externalId: string;
    body: string | null;
    repoLabel: string;
    status: 'idle' | 'thinking' | 'ready' | 'committing' | 'committed' | 'pr_opening';
    commitName: string | null;
    summary: string | null;
    error: string | null;
  };
  let claudeStaged = $state<ClaudeStaged | null>(null);

  // ClaudeMessage, Mention, ClaudeSession, ClaudeAction and RepoInfo
  // are imported from $lib/types so the workbench column components can
  // share the same shapes.

  let activeRepoInfo = $state<RepoInfo | null>(null);

  // Session persistence — sessions store handles the heavy lifting; we just
  // have to wire the $effect calls inside the component's effect scope.
  persistSessionsEffect();
  persistRulesEffect();

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
    if (!connectedGithub && !connectedJira && !connectedClaude) return;
    if (connectedGithub) addPanelInstance('github');
    if (connectedJira) addPanelInstance('jira');
    if (connectedClaude) addPanelInstance('claude');
  });

  // Modals
  let patModal = $state<{ conn: ConnectionMeta; token: string; error: string | null; busy: boolean } | null>(null);
  let jiraModal = $state<{
    workspace: string;
    email: string;
    token: string;
    error: string | null;
    busy: boolean;
  } | null>(null);
  let claudeModal = $state<{ status: ClaudeStatus | null; loading: boolean } | null>(null);
  let commentModal = $state<{ body: string; busy: boolean; error: string | null } | null>(null);
  let reviewModal = $state<{ event: ReviewEvent; body: string; busy: boolean; error: string | null } | null>(null);
  let mergeModal = $state<{ method: MergeMethod; busy: boolean; error: string | null } | null>(null);
  let commitModal = $state<{ commit: CommitEntry; detail: CommitDetail | null; loading: boolean; error: string | null; expanded: Set<string> } | null>(null);
  let confirmModal = $state<{
    title: string;
    body: string;
    confirmText: string;
    danger?: boolean;
    busy: boolean;
    onConfirm: () => Promise<void>;
  } | null>(null);
  let actionBusy = $state<string | null>(null);

  // Jira Create Issue modal
  type JiraCreateModalState = {
    projectKey: string;
    projects: JiraProject[];
    projectsLoading: boolean;
    issueTypes: JiraIssueType[];
    issueTypeName: string;
    summary: string;
    description: string;
    assigneeAccountId: string;
    sprints: JiraSprint[];
    sprintId: number | null;
    busy: boolean;
    error: string | null;
  };
  let jiraCreateModal = $state<JiraCreateModalState | null>(null);

  // GitHub Create PR modal
  type GithubCreatePrModalState = {
    repo: string;
    repos: { owner: string; name: string; full_name: string; default_branch?: string | null }[];
    reposLoading: boolean;
    branches: RepoBranch[];
    branchesLoading: boolean;
    base: string;
    head: string;
    title: string;
    body: string;
    draft: boolean;
    compare: {
      loading: boolean;
      error: string | null;
      total_commits: number;
      ahead_by: number;
      behind_by: number;
      additions: number;
      deletions: number;
      commits: CommitEntry[];
      files: import('$lib/data').ChangedFile[];
    } | null;
    filesExpanded: boolean;
    busy: boolean;
    error: string | null;
  };
  let githubCreatePrModal = $state<GithubCreatePrModalState | null>(null);

  // Derived — reading from the connections store. `$derived` re-runs when
  // reactive state inside its expression changes, so touching `connectionsState`
  // is enough to re-compute. Short aliases keep the template readable.
  const githubStatus = $derived(connectionsState.github);
  const jiraStatus = $derived(connectionsState.jira);
  const claudeStatus = $derived(connectionsState.claude);
  const cursorStatus = $derived(connectionsState.cursor);
  const statusLoading = $derived(connectionsState.statusLoading);
  const connectedGithub = $derived(githubStatus.kind === 'connected');
  const connectedJira = $derived(jiraStatus.kind === 'connected');
  const connectedClaude = $derived(claudeStatus?.ready ?? false);
  const connectedCursor = $derived(cursorStatus?.ready ?? false);
  const connectedIds = $derived.by(() => {
    const set = new Set<string>();
    if (connectedGithub) set.add('github');
    if (connectedJira) set.add('jira');
    if (connectedClaude) set.add('claude');
    if (connectedCursor) set.add('cursor');
    return set;
  });
  const anythingConnected = $derived(connectedIds.size > 0);

  let refreshInterval: ReturnType<typeof setInterval> | null = null;
  let tickInterval: ReturnType<typeof setInterval> | null = null;
  let tauriDropUnlisten: UnlistenFn | null = null;

  // Wire the layout→sessions hook once. Any closed panel instance (via the X
  // button or workbench deletion) orphans its pinned sessions back to the
  // floating pool so they reattach elsewhere instead of vanishing.
  registerInstanceRemovedHook((id) => orphanSessionsForInstance(id));

  onMount(async () => {
    restorePanelState();
    // Seed v1 editor root into the first editor instance, if any.
    try {
      const savedEditorRoot = localStorage.getItem('forgehold:editor:root');
      if (savedEditorRoot) {
        const ed = firstInstanceOfKind('editor');
        if (ed) setEditorRepoPath(savedEditorRoot, ed.id);
      }
    } catch {/* ignore */}
    tickInterval = setInterval(() => (now = Date.now()), 30_000);
    // Biometric gate runs first — refreshAllStatus + inbox fetches live
    // inside `biometricUnlock` so nothing hits the keychain before the
    // user authenticates.
    void biometricUnlock();

    // Tauri's native drag-drop channel — delivers Finder drops as native
    // absolute paths, bypassing the WebKit sandbox that sometimes hides
    // `text/uri-list` from DOM drop handlers. DOM handlers stay wired for
    // internal drags (tickets / file tree); this listener is the robust
    // path for OS files.
    tauriDropUnlisten = await listen<{ paths?: string[]; position?: { x: number; y: number } }>(
      'tauri://drag-drop',
      (e) => {
        const paths = Array.isArray(e.payload.paths) ? e.payload.paths : [];
        if (paths.length === 0) return;
        const pos = e.payload.position;
        if (!pos) return;
        // Hit-test the pointer: walk up from elementFromPoint to find a
        // `.wb-column`, then route only claude/cursor columns onward.
        const el = document.elementFromPoint(pos.x, pos.y) as HTMLElement | null;
        const col = el?.closest('.wb-column') as HTMLElement | null;
        const colKind = col?.dataset.kind;
        const colInstanceId = col?.dataset.instanceId;
        if (!colInstanceId || (colKind !== 'claude' && colKind !== 'cursor')) return;
        const kind = colKind as 'claude' | 'cursor';
        const activeId = sessionsState.activeByInstance[colInstanceId];
        let target = activeId
          ? sessionsState.list.find((s) => s.id === activeId) ?? null
          : null;
        if (!target) {
          target = sessionsState.list.find((s) => s.columnInstanceId === colInstanceId) ?? null;
        }
        if (!target) {
          const id = newClaudeSession({ agentKind: kind, columnInstanceId: colInstanceId });
          target = sessionsState.list.find((s) => s.id === id) ?? null;
        }
        if (!target) return;
        const n = attachPathsToSession(target.id, paths);
        if (n > 0) setActiveSessionInColumn(colInstanceId, target.id);
      }
    );
  });

  onDestroy(() => {
    if (refreshInterval) clearInterval(refreshInterval);
    if (tickInterval) clearInterval(tickInterval);
    tauriDropUnlisten?.();
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

  // Re-fetch detail when focus item changes. Kept in +page.svelte because it
  // also pokes `tab` (parent-owned UI state) back to 'conversation' on every
  // focus-change.
  let lastLoadedKey = $state<string | null>(null);
  $effect(() => {
    const key = inboxState.focusItem
      ? `${inboxState.focusItem.repo?.owner}/${inboxState.focusItem.repo?.name}#${inboxState.focusItem.number}`
      : null;
    if (key && key !== lastLoadedKey) {
      lastLoadedKey = key;
      tab = 'conversation';
      void loadDetail();
    }
  });

  // ---- Drag handlers ----

  function onDragStart(payload: DragPayload, e: DragEvent) {
    dragPayload = payload;
    if (e.dataTransfer) {
      e.dataTransfer.effectAllowed = 'copy';
      e.dataTransfer.setData(
        'text/plain',
        payload.source === 'github' ? `#${payload.item.number}` : payload.item.key
      );
    }
    // Track pointer globally so we can auto-scroll .wb-columns when the
    // user drags a card near either edge.
    document.addEventListener('dragover', trackDragPointer);
  }

  function onDragEnd() {
    dragPayload = null;
    dragOverInstanceId = null;
    justDragged = true;
    setTimeout(() => (justDragged = false), 120);
    document.removeEventListener('dragover', trackDragPointer);
    stopDragAutoscroll();
  }

  function onAgentDragOver(instanceId: string, _kind: 'claude' | 'cursor', e: DragEvent) {
    const types = e.dataTransfer?.types;
    const hasTicket = !!dragPayload;
    const hasInternalFile = types?.includes('application/x-forgehold-file') ?? false;
    // OS drag-drops from Finder set "Files" and "text/uri-list" on macOS.
    // We accept both so users can drop images/docs straight into the chat.
    const hasOsFile = (types?.includes('Files') || types?.includes('text/uri-list')) ?? false;
    if (!hasTicket && !hasInternalFile && !hasOsFile) return;
    e.preventDefault();
    if (e.dataTransfer) e.dataTransfer.dropEffect = 'copy';
    dragOverInstanceId = instanceId;
  }

  function onAgentDragLeave(instanceId: string) {
    if (dragOverInstanceId === instanceId) dragOverInstanceId = null;
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

    // 1) File / directory drop from the Editor tree.
    const fileRaw = e.dataTransfer?.getData('application/x-forgehold-file');
    if (fileRaw) {
      try {
        const { path, isDir, name } = JSON.parse(fileRaw) as {
          path: string; isDir: boolean; name: string;
        };
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
        dragOverInstanceId = null;
        justDragged = true;
        setTimeout(() => (justDragged = false), 200);
      } catch {
        dragOverInstanceId = null;
      }
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
        dragOverInstanceId = null;
        justDragged = true;
        setTimeout(() => (justDragged = false), 200);
        return;
      }
    }

    // 3) Ticket drop from Jira / GitHub inbox.
    if (!dragPayload) {
      dragOverInstanceId = null;
      return;
    }
    const p = dragPayload;
    const mention: Mention =
      p.source === 'github'
        ? {
            source: 'github',
            externalId: externalId(p.item),
            title: p.item.title,
            body: p.item.body
          }
        : {
            source: 'jira',
            externalId: p.item.key,
            title: p.item.summary,
            body: p.item.description
          };

    const target = pickTarget();
    if (target) {
      const sep = target.input && !target.input.endsWith(' ') ? ' ' : '';
      updateSession(target.id, {
        input: target.input + sep + `@${mention.externalId} `,
        mentions: [...target.mentions, mention]
      });
      setActiveSessionInColumn(instanceId, target.id);
    }

    dragOverInstanceId = null;
    dragPayload = null;
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
        updateSession(activeSession.id, { cwd: picked, linkedToEditor: false });
      }
    } catch (e) {
      console.error('pickCwd', e);
    }
  }

  function clearCwd() {
    if (!activeSession) return;
    updateSession(activeSession.id, { cwd: null, linkedToEditor: false });
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
      alert('Pick a repository folder first — worktrees need a git repo to branch off.');
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
      alert(`Failed to create worktree: ${typeof e === 'string' ? e : String(e)}`);
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
      alert(`Failed to remove worktree: ${typeof e === 'string' ? e : String(e)}`);
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
      alert(msg);
    } catch (e) {
      alert(`Apply failed: ${typeof e === 'string' ? e : String(e)}\n\nThe worktree is preserved — fix conflicts in the main repo (or via Editor), then try again.`);
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
      updateSession(id, { title: autoTitle, input: '', sending: true, mentions: [] });
    } else {
      updateSession(id, { input: '', sending: true, mentions: [] });
    }
    // Append empty assistant message that streaming will fill.
    appendSessionMessage(id, {
      role: 'assistant',
      content: '',
      at: new Date().toISOString()
    });
    startThinkingTimer();
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

    // Subscribe to streaming events for this session.
    let unlisten: UnlistenFn | null = null;
    try {
      unlisten = await listen<string>(`claude:stream:${id}`, (event) => {
        try {
          const parsed = JSON.parse(event.payload);
          handleStreamEvent(id, parsed);
        } catch {
          // ignore malformed lines
        }
      });
    } catch (e) {
      console.error('listen', e);
    }

    try {
      // Priority of working dir:
      //   1. Session has an isolated worktree → use it (SPEC: "every Claude run
      //      in a worktree, never touches main working tree").
      //   2. Explicit cwd set by user via pickCwd.
      //   3. Editor column's open repo (shared state).
      //   4. None → Claude inherits Forgehold's cwd (last-resort fallback).
      const cwd = sess?.worktreePath || sess?.cwd || editorRepoPath || null;
      const claudeUuid = sess?.claudeUuid ?? genUuid();
      const resume = Boolean(sess?.claudeResumable);
      const rules = sessionsState.userRules.trim();
      const agentKind = sess?.agentKind ?? 'claude';
      const cursorModel = agentKind === 'cursor' ? (sess?.cursorModel ?? null) : null;
      const result = await invoke<{ reply: string; session_uuid: string }>('claude_ask', {
        sessionId: id,
        prompt,
        cwd,
        claudeUuid,
        resume,
        rules: rules || null,
        agentKind,
        cursorModel
      });
      // Replace the streaming-accumulated body with the clean final text.
      replaceLastAssistant(id, result.reply.trim() || '(empty response)');
      // Persist the effective session uuid — for Cursor this may differ from
      // what we sent (CLI mints its own `chat_id` via `create-chat` on the
      // first turn); for Claude it round-trips unchanged.
      const patch: Partial<ClaudeSession> = { claudeResumable: true };
      if (result.session_uuid && result.session_uuid !== claudeUuid) {
        patch.claudeUuid = result.session_uuid;
      }
      updateSession(id, patch);
    } catch (e) {
      const msg = typeof e === 'string' ? e : String(e);
      if (msg.toLowerCase().includes('cancelled')) {
        // Keep the partial content; add a system note.
        appendSessionMessage(id, {
          role: 'system',
          content: 'Cancelled.',
          at: new Date().toISOString()
        });
      } else {
        replaceLastAssistant(id, `**Claude failed:** ${msg}`);
      }
    }
    if (unlisten) unlisten();
    stopThinkingTimer();
    updateSession(id, { sending: false });
    void scrollChatBottom();
  }

  // Append a streaming delta to the last assistant message, then scroll —
  // session mutation lives in the sessions store; the scroll lives here
  // because it reads DOM refs owned by the agent column.
  function appendAssistantDelta(sessionId: string, delta: string) {
    appendToLastAssistant(sessionId, delta);
    void scrollChatBottom();
  }

  function handleStreamEvent(sessionId: string, parsed: unknown) {
    if (!parsed || typeof parsed !== 'object') return;
    const msg = parsed as Record<string, unknown>;
    const type = msg.type;
    if (type === 'assistant') {
      const inner = msg.message as { content?: Array<Record<string, unknown>> } | undefined;
      if (inner?.content && Array.isArray(inner.content)) {
        for (const block of inner.content) {
          if (block.type === 'text' && typeof block.text === 'string') {
            appendAssistantDelta(sessionId, block.text);
          } else if (block.type === 'tool_use') {
            const name = typeof block.name === 'string' ? block.name : 'tool';
            const input = (block.input ?? {}) as Record<string, unknown>;
            // Intercept propose_commit / propose_pr: they don't execute, they
            // surface action cards in the chat. Suppress the generic tool-use
            // line so the card does the talking.
            if (name === 'mcp__github__propose_commit') {
              const id = typeof block.id === 'string' ? block.id : genId();
              addAction(sessionId, {
                id,
                kind: 'commit',
                message: String(input.message ?? ''),
                body: typeof input.body === 'string' ? input.body : '',
                push: input.push !== false,
                note: typeof input.note === 'string' ? input.note : '',
                status: 'pending'
              });
              continue;
            }
            if (name === 'mcp__github__propose_pr') {
              const id = typeof block.id === 'string' ? block.id : genId();
              addAction(sessionId, {
                id,
                kind: 'pr',
                title: String(input.title ?? ''),
                body: typeof input.body === 'string' ? input.body : '',
                base: typeof input.base === 'string' ? input.base : '',
                draft: input.draft === true,
                note: typeof input.note === 'string' ? input.note : '',
                status: 'pending'
              });
              continue;
            }
            if (name === 'mcp__github__propose_switch_cwd') {
              const id = typeof block.id === 'string' ? block.id : genId();
              addAction(sessionId, {
                id,
                kind: 'switch_cwd',
                path: String(input.path ?? ''),
                reason: typeof input.reason === 'string' ? input.reason : '',
                status: 'pending'
              });
              continue;
            }
            if (name === 'mcp__github__propose_bash') {
              const id = typeof block.id === 'string' ? block.id : genId();
              addAction(sessionId, {
                id,
                kind: 'bash',
                command: String(input.command ?? ''),
                reason: typeof input.reason === 'string' ? input.reason : '',
                status: 'pending'
              });
              continue;
            }
            const formatted = formatToolUse(name, input);
            if (formatted) appendAssistantDelta(sessionId, `\n\n${formatted}\n\n`);
          }
        }
      }
    }
  }

  function effectiveCwd(s: ClaudeSession): string | null {
    return s.worktreePath || s.cwd || editorRepoPath || null;
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

  /** Only agent pills accept drops — github/jira/editor don't host
      sessions that can take @mentions. Require something droppable:
      internal drag payload (ticket / file-tree) or OS files in the
      dataTransfer. */
  function pillCanAccept(e: DragEvent, kind: PanelKind): boolean {
    if (kind !== 'claude' && kind !== 'cursor') return false;
    const types = e.dataTransfer?.types;
    if (dragPayload) return true;
    if (types?.includes('application/x-forgehold-file')) return true;
    if (types && types.length > 0) return true;
    return false;
  }

  function onPillDragOver(e: DragEvent, kind: PanelKind, instanceId: string | null) {
    if (!pillCanAccept(e, kind)) return;
    e.preventDefault();
    e.stopPropagation();
    if (e.dataTransfer) e.dataTransfer.dropEffect = 'copy';
    pillDragOverKind = kind;
    pillDragOverInstance = instanceId;
  }

  function onPillDragLeave(kind: PanelKind, instanceId: string | null) {
    if (pillDragOverKind === kind && pillDragOverInstance === instanceId) {
      pillDragOverKind = null;
      pillDragOverInstance = null;
    }
  }

  function onPillDrop(e: DragEvent, kind: PanelKind, specificInstanceId: string | null) {
    if (!pillCanAccept(e, kind)) return;
    e.preventDefault();
    e.stopPropagation();
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

  async function executeCommit(sessionId: string, actionId: string) {
    const sess = sessionsState.list.find((x) => x.id === sessionId);
    const action = sess?.actions.find((a) => a.id === actionId && a.kind === 'commit');
    if (!sess || !action || action.kind !== 'commit') return;
    const cwd = effectiveCwd(sess);
    if (!cwd) {
      updateAction(sessionId, actionId, { status: 'error', result: 'No working directory — pick a folder or enable worktree first.' });
      return;
    }
    updateAction(sessionId, actionId, { status: 'executing' });
    try {
      // Stage all dirty files first. Forgehold-style: full-stage before commit.
      const status = await invoke<{ files: { path: string; unstaged: boolean; staged: boolean }[] }>(
        'git_status', { repo: cwd }
      );
      const toStage = status.files.filter((f) => f.unstaged).map((f) => f.path);
      if (toStage.length) {
        await invoke('git_stage', { repo: cwd, paths: toStage });
      }
      const fullMsg = action.body ? `${action.message}\n\n${action.body}` : action.message;
      let res: string;
      if (action.push) {
        res = await invoke<string>('git_commit_and_push', { repo: cwd, message: fullMsg });
      } else {
        res = await invoke<string>('git_commit', { repo: cwd, message: fullMsg });
      }
      updateAction(sessionId, actionId, { status: 'done', result: res });
      // Auto-dismiss successful commits after a short delay so the chat stays tidy.
      setTimeout(() => removeAction(sessionId, actionId), 4000);
    } catch (e) {
      updateAction(sessionId, actionId, {
        status: 'error',
        result: typeof e === 'string' ? e : String(e)
      });
    }
  }

  async function executeBash(sessionId: string, actionId: string) {
    const sess = sessionsState.list.find((x) => x.id === sessionId);
    const action = sess?.actions.find((a) => a.id === actionId && a.kind === 'bash');
    if (!sess || !action || action.kind !== 'bash') return;
    const cwd = effectiveCwd(sess);
    if (!cwd) {
      updateAction(sessionId, actionId, { status: 'error', result: 'No working directory — pick a folder first.' });
      return;
    }
    updateAction(sessionId, actionId, { status: 'executing' });
    try {
      const res = await invoke<{ stdout: string; stderr: string; code: number; ok: boolean }>(
        'fs_bash_run',
        { cwd, command: action.command }
      );
      const combined = [res.stdout, res.stderr].filter(Boolean).join('\n').trim();
      updateAction(sessionId, actionId, {
        status: res.ok ? 'done' : 'error',
        result: combined || '(no output)',
        exitCode: res.code
      });

      // Render `$ command` + output inline in Claude's last assistant turn so
      // the transcript reads as a continuous flow — same shape as direct Bash
      // tool calls (see formatToolUse → appendAssistantDelta path above).
      const output = combined || '(no output)';
      const exitNote = res.ok ? '' : ` _(exit ${res.code})_`;
      appendAssistantDelta(
        sessionId,
        `\n\n\`$ ${truncInline(action.command, 400)}\`${exitNote}\n\n\`\`\`\n${truncInline(output, 4000)}\n\`\`\`\n\n`
      );

      // Auto-dismiss on success so the transcript block is the primary record.
      if (res.ok) {
        setTimeout(() => removeAction(sessionId, actionId), 4000);
      }
    } catch (e) {
      updateAction(sessionId, actionId, {
        status: 'error',
        result: typeof e === 'string' ? e : String(e)
      });
    }
  }

  async function executeSwitchCwd(sessionId: string, actionId: string) {
    const sess = sessionsState.list.find((x) => x.id === sessionId);
    const action = sess?.actions.find((a) => a.id === actionId && a.kind === 'switch_cwd');
    if (!sess || !action || action.kind !== 'switch_cwd') return;
    updateAction(sessionId, actionId, { status: 'executing' });
    try {
      const exists = await invoke<boolean>('fs_path_exists', { path: action.path });
      if (!exists) {
        updateAction(sessionId, actionId, { status: 'error', result: `Path does not exist: ${action.path}` });
        return;
      }
      // Drop any worktree override — user is switching to a new location.
      updateSession(sessionId, {
        cwd: action.path,
        worktreePath: null,
        worktreeBranch: null,
        worktreeRepo: null
      });
      updateAction(sessionId, actionId, { status: 'done', result: `Switched to ${action.path}` });
      setTimeout(() => removeAction(sessionId, actionId), 3000);
    } catch (e) {
      updateAction(sessionId, actionId, {
        status: 'error',
        result: typeof e === 'string' ? e : String(e)
      });
    }
  }

  async function executePr(sessionId: string, actionId: string) {
    const sess = sessionsState.list.find((x) => x.id === sessionId);
    const action = sess?.actions.find((a) => a.id === actionId && a.kind === 'pr');
    if (!sess || !action || action.kind !== 'pr') return;
    const cwd = effectiveCwd(sess);
    if (!cwd) {
      updateAction(sessionId, actionId, { status: 'error', result: 'No working directory — pick a folder first.' });
      return;
    }
    updateAction(sessionId, actionId, { status: 'executing' });
    try {
      const url = await invoke<string>('git_create_pr', {
        repo: cwd,
        title: action.title,
        body: action.body,
        draft: action.draft,
        base: action.base.trim() || null
      });
      updateAction(sessionId, actionId, { status: 'done', result: url });
    } catch (e) {
      updateAction(sessionId, actionId, {
        status: 'error',
        result: typeof e === 'string' ? e : String(e)
      });
    }
  }

  // Dispatch helper — each ClaudeActionCard's approve button funnels through
  // here so the column component doesn't have to know which backend function
  // runs for each action kind.
  function executeAction(sessionId: string, action: ClaudeAction) {
    if (action.kind === 'commit') void executeCommit(sessionId, action.id);
    else if (action.kind === 'pr') void executePr(sessionId, action.id);
    else if (action.kind === 'switch_cwd') void executeSwitchCwd(sessionId, action.id);
    else if (action.kind === 'bash') void executeBash(sessionId, action.id);
  }

  // ---- Claude stub flow ----
  // Real agent execution is the next milestone. For now we simulate the
  // drop → run → commit → open-PR pipeline so the UX is testable.

  async function stopClaude() {
    const s = activeSession;
    if (!s) return;
    try {
      await invoke('claude_stop', { sessionId: s.id });
    } catch (e) {
      console.error('claude_stop', e);
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

  function toggleCommitFile(filename: string) {
    if (!commitModal) return;
    const next = new Set(commitModal.expanded);
    if (next.has(filename)) next.delete(filename);
    else next.add(filename);
    commitModal = { ...commitModal, expanded: next };
  }

  async function openCommit(c: CommitEntry) {
    if (!inboxState.focusItem?.repo) return;
    commitModal = { commit: c, detail: null, loading: true, error: null, expanded: new Set() };
    try {
      const detail = await invoke<CommitDetail>('github_get_commit', {
        owner: inboxState.focusItem.repo.owner,
        repo: inboxState.focusItem.repo.name,
        sha: c.sha
      });
      if (commitModal && commitModal.commit.sha === c.sha) {
        commitModal = { ...commitModal, detail, loading: false };
      }
    } catch (e) {
      if (commitModal && commitModal.commit.sha === c.sha) {
        commitModal = { ...commitModal, loading: false, error: typeof e === 'string' ? e : String(e) };
      }
    }
  }

  // --- Actions ---

  async function submitComment() {
    if (!commentModal || !inboxState.focusItem?.repo) return;
    const snap = commentModal;
    if (!snap.body.trim()) return;
    commentModal = { ...snap, busy: true, error: null };
    try {
      await invoke('github_add_comment', {
        owner: inboxState.focusItem.repo.owner,
        repo: inboxState.focusItem.repo.name,
        number: inboxState.focusItem.number,
        body: snap.body
      });
      commentModal = null;
      await reloadDetailAndLists();
    } catch (e) {
      commentModal = { ...snap, busy: false, error: typeof e === 'string' ? e : String(e) };
    }
  }

  async function submitReview() {
    if (!reviewModal || !inboxState.focusItem?.repo || !inboxState.focusItem.is_pull_request) return;
    const snap = reviewModal;
    reviewModal = { ...snap, busy: true, error: null };
    try {
      await invoke('github_submit_review', {
        owner: inboxState.focusItem.repo.owner,
        repo: inboxState.focusItem.repo.name,
        number: inboxState.focusItem.number,
        event: snap.event,
        body: snap.body
      });
      reviewModal = null;
      await reloadDetailAndLists();
    } catch (e) {
      reviewModal = { ...snap, busy: false, error: typeof e === 'string' ? e : String(e) };
    }
  }

  async function submitMerge() {
    if (!mergeModal || !inboxState.focusItem?.repo || !inboxState.focusItem.is_pull_request) return;
    const snap = mergeModal;
    mergeModal = { ...snap, busy: true, error: null };
    try {
      await invoke('github_merge_pr', {
        owner: inboxState.focusItem.repo.owner,
        repo: inboxState.focusItem.repo.name,
        number: inboxState.focusItem.number,
        method: snap.method
      });
      mergeModal = null;
      await reloadDetailAndLists();
    } catch (e) {
      mergeModal = { ...snap, busy: false, error: typeof e === 'string' ? e : String(e) };
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
    confirmModal = {
      title: `Close this ${kind}?`,
      body: `${externalId(inboxState.focusItem)} — ${inboxState.focusItem.title}`,
      confirmText: 'Close',
      danger: true,
      busy: false,
      onConfirm: async () => {
        await setState('closed');
      }
    };
  }

  async function runConfirm() {
    if (!confirmModal) return;
    const snap = confirmModal;
    confirmModal = { ...snap, busy: true };
    try {
      await snap.onConfirm();
    } finally {
      confirmModal = null;
    }
  }

  function openConnectModal(conn: ConnectionMeta) {
    if (!conn.implemented) return;
    if (conn.id === 'github') {
      patModal = { conn, token: '', error: null, busy: false };
    } else if (conn.id === 'jira') {
      jiraModal = { workspace: '', email: '', token: '', error: null, busy: false };
    } else if (conn.id === 'claude') {
      claudeModal = { status: claudeStatus, loading: false };
      void refreshClaudeModal();
    }
  }

  async function refreshClaudeModal() {
    if (!claudeModal) return;
    claudeModal = { ...claudeModal, loading: true };
    await refreshClaudeStatus();
    if (claudeModal) claudeModal = { status: claudeStatus, loading: false };
  }

  async function submitJira() {
    if (!jiraModal) return;
    const snap = jiraModal;
    jiraModal = { ...snap, busy: true, error: null };
    try {
      await invoke<JiraUser>('jira_connect', {
        workspace: snap.workspace,
        email: snap.email,
        token: snap.token
      });
      jiraModal = null;
      await refreshJiraStatus();
    } catch (e) {
      jiraModal = { ...snap, busy: false, error: typeof e === 'string' ? e : String(e) };
    }
  }

  async function disconnectJira() {
    await invoke('jira_disconnect');
    await refreshJiraStatus();
  }

  function jiraTokenUrl() {
    return 'https://id.atlassian.com/manage-profile/security/api-tokens';
  }

  function claudeInstallUrl() {
    return 'https://docs.claude.com/en/docs/claude-code/overview';
  }

  async function submitPat() {
    if (!patModal) return;
    const snap = patModal;
    patModal = { ...snap, busy: true, error: null };
    try {
      const user = await invoke<GithubUser>('github_connect_pat', { token: snap.token });
      connectionsState.github = { kind: 'connected', user };
      patModal = null;
      await refreshInbox();
      view = 'workbench';
    } catch (e) {
      patModal = { ...snap, busy: false, error: typeof e === 'string' ? e : String(e) };
    }
  }

  async function disconnectGithub() {
    await invoke('github_disconnect');
    await refreshGithubStatus();
    resetGithubInbox();
    // Repo state is owned by RepositoriesView — it wipes itself via its
    // `$effect` on `connectedGithub` becoming false.
  }

  async function disconnectJiraAll() {
    await invoke('jira_disconnect');
    await refreshJiraStatus();
    resetJiraInbox();
  }

  async function openBrowser(url: string) {
    try { await openUrl(url); } catch (e) { console.error(e); }
  }

  function onKey(e: KeyboardEvent) {
    if ((e.metaKey || e.ctrlKey) && e.key === 'k') {
      e.preventDefault();
      paletteOpen = !paletteOpen;
    } else if (e.key === 'Escape') {
      paletteOpen = false;
      if (patModal && !patModal.busy) patModal = null;
      if (jiraModal && !jiraModal.busy) jiraModal = null;
      if (claudeModal && !claudeModal.loading) claudeModal = null;
      if (inboxState.userPickerModal) inboxState.userPickerModal = null;
      if (commentModal && !commentModal.busy) commentModal = null;
      if (reviewModal && !reviewModal.busy) reviewModal = null;
      if (mergeModal && !mergeModal.busy) mergeModal = null;
      if (commitModal) commitModal = null;
      if (confirmModal && !confirmModal.busy) confirmModal = null;
      if (jiraCreateModal && !jiraCreateModal.busy) jiraCreateModal = null;
      if (githubCreatePrModal && !githubCreatePrModal.busy) githubCreatePrModal = null;
      if (inboxState.focusItem) closeFocusItem();
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
      inboxState.userPickerModal ||
      commentModal ||
      reviewModal ||
      mergeModal ||
      commitModal ||
      confirmModal ||
      jiraCreateModal ||
      githubCreatePrModal ||
      inboxState.focusItem ||
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
    jiraCreateModal = {
      projectKey: active.projectKey ?? '',
      projects: inboxState.jiraProjectOptions,
      projectsLoading: false,
      issueTypes: [],
      issueTypeName: 'Task',
      summary: '',
      description: '',
      assigneeAccountId: '',
      sprints: inboxState.jiraSprintOptions,
      sprintId:
        typeof active.sprintId === 'number' ? active.sprintId : null,
      busy: false,
      error: null
    };
    // Always refresh projects list (lazy — skips if already cached).
    if (!inboxState.jiraProjectOptions.length) {
      jiraCreateModal = { ...jiraCreateModal, projectsLoading: true };
      try {
        const projects = await invoke<JiraProject[]>('jira_list_projects');
        inboxState.jiraProjectOptions = projects;
        if (jiraCreateModal) {
          jiraCreateModal = { ...jiraCreateModal, projects, projectsLoading: false };
        }
      } catch {
        if (jiraCreateModal) {
          jiraCreateModal = { ...jiraCreateModal, projectsLoading: false };
        }
      }
    }
    // If a project is pre-selected, pull its issue types immediately.
    if (jiraCreateModal && jiraCreateModal.projectKey) {
      void onJiraCreateProjectChange(jiraCreateModal.projectKey);
    }
  }

  async function onJiraCreateProjectChange(key: string) {
    if (!jiraCreateModal) return;
    jiraCreateModal = { ...jiraCreateModal, projectKey: key, issueTypes: [] };
    if (!key) return;
    try {
      const types = await invoke<JiraIssueType[]>('jira_list_issue_types', {
        projectKey: key
      });
      if (jiraCreateModal) {
        // Keep a sensible default issue type name — prefer whatever the user
        // already had picked if it's still valid, otherwise first type from
        // the API, otherwise hard-coded "Task".
        const currentName = jiraCreateModal.issueTypeName;
        const preserved = types.find((t) => t.name === currentName);
        const nextName = preserved ? preserved.name : types[0]?.name ?? 'Task';
        jiraCreateModal = { ...jiraCreateModal, issueTypes: types, issueTypeName: nextName };
      }
    } catch {
      // ignore — modal falls back to hardcoded Task/Bug/Story
    }
  }

  async function submitJiraCreate() {
    if (!jiraCreateModal) return;
    const snap = jiraCreateModal;
    if (!snap.projectKey.trim() || !snap.summary.trim() || !snap.issueTypeName.trim()) {
      return;
    }
    jiraCreateModal = { ...snap, busy: true, error: null };
    try {
      const created = await invoke<JiraItem>('jira_create_issue', {
        projectKey: snap.projectKey.trim(),
        issueType: snap.issueTypeName,
        summary: snap.summary.trim(),
        description: snap.description,
        assigneeAccountId: snap.assigneeAccountId.trim() || null,
        sprintId: snap.sprintId
      });
      // Optimistically push the new issue onto the current list, then refresh
      // to pick up server-side ordering.
      inboxState.jiraItems = [created, ...inboxState.jiraItems];
      jiraCreateModal = null;
      void refreshJiraInbox({ silent: true });
    } catch (e) {
      jiraCreateModal = {
        ...snap,
        busy: false,
        error: typeof e === 'string' ? e : String(e)
      };
    }
  }

  // ---- GitHub Create PR ----

  async function openGithubCreatePr() {
    const activeRepo = inboxState.githubFilters.repo;
    githubCreatePrModal = {
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
    };
    if (!inboxState.githubRepoOptions.length) {
      githubCreatePrModal = { ...githubCreatePrModal, reposLoading: true };
      try {
        const repos = await invoke<Repository[]>('github_list_repos');
        inboxState.githubRepoOptions = repos.map((r) => ({
          owner: r.owner,
          name: r.name,
          full_name: r.full_name
        }));
        if (githubCreatePrModal) {
          githubCreatePrModal = {
            ...githubCreatePrModal,
            repos: repos.map((r) => ({
              owner: r.owner,
              name: r.name,
              full_name: r.full_name,
              default_branch: r.default_branch
            })),
            reposLoading: false
          };
        }
      } catch {
        if (githubCreatePrModal) {
          githubCreatePrModal = { ...githubCreatePrModal, reposLoading: false };
        }
      }
    }
    if (githubCreatePrModal && githubCreatePrModal.repo) {
      void onGithubPrRepoChange(githubCreatePrModal.repo);
    }
  }

  async function onGithubPrRepoChange(full: string) {
    if (!githubCreatePrModal) return;
    githubCreatePrModal = {
      ...githubCreatePrModal,
      repo: full,
      branches: [],
      base: '',
      head: '',
      compare: null,
      branchesLoading: !!full
    };
    if (!full) return;
    const [owner, name] = full.split('/');
    if (!owner || !name) return;
    try {
      const branches = await invoke<RepoBranch[]>('github_list_repo_branches', {
        owner,
        repo: name
      });
      // Look up the default branch — either from the cached repo list, or via
      // github_list_repos if we don't have it yet.
      let defaultBranch =
        githubCreatePrModal.repos.find((r) => r.full_name === full)?.default_branch ?? null;
      if (!defaultBranch) {
        try {
          const repos = await invoke<Repository[]>('github_list_repos');
          defaultBranch = repos.find((r) => r.full_name === full)?.default_branch ?? null;
        } catch { /* ignore */ }
      }
      if (githubCreatePrModal) {
        githubCreatePrModal = {
          ...githubCreatePrModal,
          branches,
          branchesLoading: false,
          base: defaultBranch ?? branches[0]?.name ?? ''
        };
      }
    } catch (e) {
      if (githubCreatePrModal) {
        githubCreatePrModal = {
          ...githubCreatePrModal,
          branchesLoading: false,
          error: typeof e === 'string' ? e : String(e)
        };
      }
    }
  }

  async function onGithubPrBranchesChange() {
    if (!githubCreatePrModal) return;
    const m = githubCreatePrModal;
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
      if (pretty) {
        githubCreatePrModal = { ...m, title: pretty };
      }
    }
    if (!m.repo || !m.base || !m.head || m.base === m.head) {
      if (m.compare) githubCreatePrModal = { ...githubCreatePrModal!, compare: null };
      return;
    }
    const [owner, name] = m.repo.split('/');
    if (!owner || !name) return;
    githubCreatePrModal = {
      ...githubCreatePrModal!,
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
    };
    try {
      const result = await invoke<CompareResult>('github_compare', {
        owner,
        repo: name,
        base: m.base,
        head: m.head
      });
      if (githubCreatePrModal) {
        githubCreatePrModal = {
          ...githubCreatePrModal,
          compare: { loading: false, error: null, ...result }
        };
      }
    } catch (e) {
      if (githubCreatePrModal) {
        githubCreatePrModal = {
          ...githubCreatePrModal,
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
        };
      }
    }
  }

  async function submitGithubPr() {
    if (!githubCreatePrModal) return;
    const snap = githubCreatePrModal;
    if (!snap.repo || !snap.base || !snap.head || snap.base === snap.head || !snap.title.trim()) {
      return;
    }
    const [owner, name] = snap.repo.split('/');
    if (!owner || !name) return;
    githubCreatePrModal = { ...snap, busy: true, error: null };
    try {
      const created = await invoke<InboxItem>('github_create_pr', {
        owner,
        repo: name,
        title: snap.title.trim(),
        body: snap.body,
        base: snap.base,
        head: snap.head,
        draft: snap.draft
      });
      githubCreatePrModal = null;
      // Optimistically push onto inbox and open focus pane.
      inboxState.items = [created, ...inboxState.items];
      openFocusItem(created);
      view = 'workbench';
      void refreshInbox({ silent: true });
    } catch (e) {
      githubCreatePrModal = {
        ...snap,
        busy: false,
        error: typeof e === 'string' ? e : String(e)
      };
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

<div id="app" class:is-dragging={dragPayload !== null}>
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
              role="button"
              tabindex="0"
              ondblclick={() => startWorkbenchRename(wb.id, wb.name)}
              onclick={() => setActiveWorkbench(wb.id)}
              onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); setActiveWorkbench(wb.id); } }}
              title="Click to switch · double-click to rename"
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
                onOpenComment={() => (commentModal = { body: '', busy: false, error: null })}
                onOpenReview={() => (reviewModal = { event: 'APPROVE', body: '', busy: false, error: null })}
                onOpenMerge={() => (mergeModal = { method: 'squash', busy: false, error: null })}
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
                onStopClaude={() => void stopClaude()}
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
                onStopClaude={() => void stopClaude()}
                onOpenMentionPath={(p) => void openMentionPath(p)}
              />
            {:else if inst.kind === 'editor'}
              <EditorColumn
                instanceId={inst.id}
                onLinkToAgent={(agentId) => linkEditorToAgent(inst.id, agentId)}
              />
            {/if}
          {/each}

          {#if inboxState.jiraFocusKey}
            <div class="slide-over" onclick={(e) => { if (e.target === e.currentTarget) inboxState.jiraFocusKey = null; }} role="dialog" aria-modal="true" tabindex="-1">
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
        onOpenComment={() => (commentModal = { body: '', busy: false, error: null })}
        onOpenReview={() => (reviewModal = { event: 'APPROVE', body: '', busy: false, error: null })}
        onOpenMerge={() => (mergeModal = { method: 'squash', busy: false, error: null })}
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

    {:else if view === 'rules'}
      <RulesView />

    {:else if view === 'connections'}
      <ConnectionsView
        {sourceConns}
        {agentConns}
        {connectedIds}
        {githubStatus}
        {jiraStatus}
        {claudeStatus}
        {cursorStatus}
        onDisconnectGithub={disconnectGithub}
        onDisconnectJira={disconnectJiraAll}
        onOpenConnectModal={openConnectModal}
      />
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

<Modals
  bind:commitModal
  bind:userPickerModal={inboxState.userPickerModal}
  bind:jiraModal
  bind:claudeModal
  bind:patModal
  bind:commentModal
  bind:reviewModal
  bind:mergeModal
  bind:confirmModal
  bind:jiraCreateModal
  bind:githubCreatePrModal
  {now}
  {githubStatus}
  {jiraStatus}
  {toggleCommitFile}
  {openBrowser}
  {onUserPickerInput}
  selectJiraUser={selectAssignee}
  selectAnyJiraUser={selectAnyAssignee}
  {submitJira}
  {jiraTokenUrl}
  {refreshClaudeModal}
  {claudeInstallUrl}
  {submitPat}
  {githubTokenUrl}
  {submitComment}
  {submitReview}
  {submitMerge}
  {runConfirm}
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
  .col-toggle {
    display: inline-flex; align-items: center; gap: 8px;
    padding: 6px 10px;
    border-radius: 8px;
    background: var(--bg-1);
    border: 1px solid var(--border-neutral);
    color: var(--text-2);
    font-size: 12.5px; font-weight: 500;
    transition: all 140ms;
  }
  .col-toggle:hover { color: var(--text-0); border-color: var(--border-neutral-hi); }
  .col-toggle.active {
    background: var(--bg-2);
    color: var(--text-0);
    border-color: var(--border-hi);
    box-shadow: inset 0 0 0 1px var(--border-hi);
  }
  .col-toggle-dot {
    width: 6px; height: 6px; border-radius: 50%;
    background: var(--text-mute);
  }
  .col-toggle.active .dot--github { background: #b199f6; box-shadow: 0 0 6px rgba(139, 92, 246, 0.5); }
  .col-toggle.active .dot--jira { background: #60a5fa; box-shadow: 0 0 6px rgba(59, 130, 246, 0.5); }
  .col-toggle.active .dot--claude { background: var(--accent-bright); box-shadow: 0 0 6px var(--accent-glow); }
  .col-toggle.active .dot--cursor { background: #b099f6; box-shadow: 0 0 6px rgba(176, 153, 246, 0.55); }
  .col-toggle.active .dot--editor { background: var(--warning); box-shadow: 0 0 6px rgba(229, 162, 42, 0.5); }
  .dot--cursor { background: rgba(176, 153, 246, 0.55); }
  .dot--editor { background: var(--warning); }
  .col-toggle-count {
    padding: 0 6px;
    min-width: 16px; height: 16px;
    border-radius: 8px;
    background: var(--bg-3);
    font-size: 10.5px; font-weight: 600;
    display: inline-flex; align-items: center; justify-content: center;
    color: var(--text-1);
  }
  .col-toggle.active .col-toggle-count { background: var(--accent-soft); color: var(--accent-bright); }

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
  .pill-group.has-menu:hover .pill-menu,
  .pill-group.has-menu.drag-over .pill-menu {
    display: flex;
    animation: fadeIn 120ms ease-out;
  }
  /* During any drag (ticket card, file, OS file), force-open every pill's
     menu so the user can aim at a specific column without having to first
     hover the pill without the menu. WebKit sometimes suppresses `:hover`
     mid-drag; the explicit class is the robust trigger. */
  #app.is-dragging .pill-group.has-menu .pill-menu {
    display: flex;
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
