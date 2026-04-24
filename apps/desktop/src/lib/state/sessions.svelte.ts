// Claude + Cursor chat session state. Owns the session list, per-column
// active-session pointers, the cross-column "currently focused" pointer,
// per-column scroll containers, and user-authored rules. Persists sessions
// and rules to localStorage via $effect.

import type { ClaudeAction, ClaudeMessage, ClaudeSession, Mention } from '$lib/types';

export const SESSIONS_STORAGE_KEY = 'forgehold:claude-sessions:v1';
export const RULES_STORAGE_KEY = 'forgehold:claude-rules:v1';

export function genId() {
  return Math.random().toString(36).slice(2) + Date.now().toString(36);
}

export function genUuid(): string {
  if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
    return crypto.randomUUID();
  }
  // Fallback for ancient webviews — manual RFC4122 v4.
  return 'xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx'.replace(/[xy]/g, (c) => {
    const r = (Math.random() * 16) | 0;
    const v = c === 'x' ? r : (r & 0x3) | 0x8;
    return v.toString(16);
  });
}

function loadStoredRules(): string {
  if (typeof localStorage === 'undefined') return '';
  try {
    return localStorage.getItem(RULES_STORAGE_KEY) ?? '';
  } catch {
    return '';
  }
}

function loadStoredSessions(): {
  sessions: ClaudeSession[];
  activeId: string | null;
} {
  if (typeof localStorage === 'undefined') return { sessions: [], activeId: null };
  try {
    const raw = localStorage.getItem(SESSIONS_STORAGE_KEY);
    if (!raw) return { sessions: [], activeId: null };
    const data = JSON.parse(raw) as {
      sessions?: ClaudeSession[];
      activeId?: string | null;
    };
    const sessions = (data.sessions || []).map((s) => ({
      ...s,
      input: s.input || '',
      sending: false,
      cwd: s.cwd || null,
      mentions: s.mentions || [],
      messages: s.messages || [],
      worktreePath: s.worktreePath ?? null,
      worktreeBranch: s.worktreeBranch ?? null,
      worktreeRepo: s.worktreeRepo ?? null,
      actions: (s.actions || []).filter(
        (a: ClaudeAction) => a.status === 'pending' || a.status === 'error'
      ),
      // Backfill for sessions persisted before Claude-CLI session resume
      // landed. No UUID → fresh session on next send (Claude won't remember
      // old turns, but UI history stays visible).
      claudeUuid: (s as { claudeUuid?: string }).claudeUuid || genUuid(),
      claudeResumable: Boolean((s as { claudeResumable?: boolean }).claudeResumable),
      agentKind: ((s as { agentKind?: 'claude' | 'cursor' }).agentKind ?? 'claude'),
      cursorModel: (s as { cursorModel?: string | null }).cursorModel ?? null,
      linkedToEditor: Boolean((s as { linkedToEditor?: boolean }).linkedToEditor),
      linkedToEditorInstanceId:
        (s as { linkedToEditorInstanceId?: string | null }).linkedToEditorInstanceId ?? null,
      // Sessions persisted before multi-instance landed have no column binding
      // — null means "float and attach to the first matching-kind column".
      columnInstanceId:
        (s as { columnInstanceId?: string | null }).columnInstanceId ?? null
    }));
    return {
      sessions,
      activeId: data.activeId ?? sessions[0]?.id ?? null
    };
  } catch {
    return { sessions: [], activeId: null };
  }
}

const __initial = loadStoredSessions();
const __legacy = __initial.sessions.find((s) => s.id === __initial.activeId);

/** Reactive singleton. Imported by +page.svelte and any chat-aware
 *  component. Per-column-instance active pointers live in `activeByInstance`
 *  now; legacy `activeIds` (per-kind) is kept as a fallback for floating
 *  sessions that haven't been pinned to a column yet. */
export const sessionsState = $state<{
  list: ClaudeSession[];
  // Per-agent-kind active session — fallback pointer for floating sessions
  // (columnInstanceId === null). Usually shadowed by per-instance pointers.
  activeIds: Record<'claude' | 'cursor', string | null>;
  // Per-column-instance active session. Key = PanelInstance.id. Each chat
  // column owns one entry; two Claude columns can focus different sessions
  // at the same time without stepping on each other.
  activeByInstance: Record<string, string | null>;
  // Cross-column focus — whatever the user last clicked. Used by legacy
  // single-column code paths (sendClaudeMessage, pickCwd, createWorktree, …)
  // that take no agent-kind argument.
  activeClaudeId: string | null;
  // Per-column-instance scroll containers. Each chat column registers its own
  // scroll element; we scroll each independently when its active session
  // streams.
  scrollEls: Record<string, HTMLDivElement | null>;
  // User-authored rules/preferences appended to Claude's system prompt on
  // every turn via `--append-system-prompt`. Edited in the Rules view.
  userRules: string;
  // Per-column-instance editor state (repoPath shown in that Editor column).
  // Keyed by PanelInstance.id.
  editorInstanceState: Record<string, { repoPath: string }>;
}>({
  list: __initial.sessions,
  activeIds: {
    claude: __legacy?.agentKind === 'cursor' ? null : __initial.activeId,
    cursor: __legacy?.agentKind === 'cursor' ? __initial.activeId : null
  },
  activeByInstance: {},
  activeClaudeId: __initial.activeId,
  scrollEls: {},
  userRules: loadStoredRules(),
  editorInstanceState: {}
});

/** Currently focused session across both columns. */
export function getActiveSession(): ClaudeSession | null {
  return sessionsState.list.find((s) => s.id === sessionsState.activeClaudeId) ?? null;
}

// ---- Persistence ----
// Call once from a +page root $effect (Svelte 5 requires $effect to run
// inside a component / .svelte.ts effect root). We expose as a function so
// the page can wire both effects in one place.

export function persistSessionsEffect() {
  $effect(() => {
    if (typeof localStorage === 'undefined') return;
    try {
      const payload = {
        sessions: sessionsState.list.map((s) => ({
          id: s.id,
          title: s.title,
          mentions: s.mentions,
          messages: s.messages,
          cwd: s.cwd,
          input: s.input,
          worktreePath: s.worktreePath,
          worktreeBranch: s.worktreeBranch,
          worktreeRepo: s.worktreeRepo,
          actions: s.actions,
          claudeUuid: s.claudeUuid,
          claudeResumable: s.claudeResumable,
          agentKind: s.agentKind,
          cursorModel: s.cursorModel,
          linkedToEditor: s.linkedToEditor,
          linkedToEditorInstanceId: s.linkedToEditorInstanceId,
          columnInstanceId: s.columnInstanceId
        })),
        activeId: sessionsState.activeClaudeId
      };
      localStorage.setItem(SESSIONS_STORAGE_KEY, JSON.stringify(payload));
    } catch (e) {
      console.error('persist sessions', e);
    }
  });
}

export function persistRulesEffect() {
  $effect(() => {
    if (typeof localStorage === 'undefined') return;
    try {
      localStorage.setItem(RULES_STORAGE_KEY, sessionsState.userRules);
    } catch (e) {
      console.error('persist rules', e);
    }
  });
}

// ---- Handlers ----

export function newClaudeSession(
  opts: {
    title?: string;
    agentKind?: 'claude' | 'cursor';
    cwd?: string | null;
    linkedToEditor?: boolean;
    linkedToEditorInstanceId?: string | null;
    columnInstanceId?: string | null;
  } = {}
): string {
  const id = genId();
  const agentKind = opts.agentKind ?? 'claude';
  const n = sessionsState.list.filter((s) => s.agentKind === agentKind).length + 1;
  const title = opts.title ?? `Chat ${n}`;
  const columnInstanceId = opts.columnInstanceId ?? null;
  sessionsState.list = [
    {
      id, title, mentions: [], messages: [], input: '', sending: false,
      cwd: opts.cwd ?? null,
      worktreePath: null, worktreeBranch: null, worktreeRepo: null,
      actions: [],
      claudeUuid: genUuid(),
      claudeResumable: false,
      agentKind,
      cursorModel: null,
      linkedToEditor: !!opts.linkedToEditor,
      linkedToEditorInstanceId: opts.linkedToEditorInstanceId ?? null,
      columnInstanceId
    },
    ...sessionsState.list
  ];
  sessionsState.activeClaudeId = id;
  sessionsState.activeIds[agentKind] = id;
  if (columnInstanceId) {
    sessionsState.activeByInstance[columnInstanceId] = id;
  }
  return id;
}

export function deleteClaudeSession(id: string) {
  const doomed = sessionsState.list.find((s) => s.id === id);
  const rest = sessionsState.list.filter((s) => s.id !== id);
  sessionsState.list = rest;
  const kind = doomed?.agentKind ?? 'claude';
  const columnId = doomed?.columnInstanceId ?? null;
  // Per-column-instance active pointer: jump to next session bound to the
  // same instance (or floating of the same kind), or null if none.
  if (columnId && sessionsState.activeByInstance[columnId] === id) {
    const nextInInst = rest.find((s) => s.columnInstanceId === columnId) ?? null;
    sessionsState.activeByInstance[columnId] = nextInInst?.id ?? null;
  }
  // Legacy per-kind pointer — still used as a fallback for floating sessions.
  if (sessionsState.activeIds[kind] === id) {
    sessionsState.activeIds[kind] =
      rest.find((s) => s.agentKind === kind && !s.columnInstanceId)?.id ??
      rest.find((s) => s.agentKind === kind)?.id ??
      null;
  }
  if (sessionsState.activeClaudeId === id) {
    sessionsState.activeClaudeId =
      (columnId ? sessionsState.activeByInstance[columnId] : null) ??
      sessionsState.activeIds[kind] ??
      sessionsState.activeIds[kind === 'claude' ? 'cursor' : 'claude'];
  }
  // Only auto-create a chat for the Claude agent kind (keeps Cursor column
  // empty until the user asks for it) and only if no Claude chats remain.
  if (rest.filter((s) => s.agentKind === 'claude').length === 0) {
    newClaudeSession({ agentKind: 'claude', columnInstanceId: columnId });
  }
}

/** Remove a column's binding from every session it owned, so the sessions
 *  float back to the global pool. Called by the layout store when closing a
 *  column instance. */
export function orphanSessionsForInstance(instanceId: string) {
  sessionsState.list = sessionsState.list.map((s) =>
    s.columnInstanceId === instanceId ? { ...s, columnInstanceId: null } : s
  );
  delete sessionsState.activeByInstance[instanceId];
  delete sessionsState.scrollEls[instanceId];
}

/** Set the active session pointer for one specific column instance. */
export function setActiveSessionInColumn(columnId: string, sessionId: string) {
  sessionsState.activeByInstance[columnId] = sessionId;
  const sess = sessionsState.list.find((s) => s.id === sessionId);
  if (sess) {
    sessionsState.activeIds[sess.agentKind] = sessionId;
    sessionsState.activeClaudeId = sessionId;
    // Pin the session to this column if it was floating, so next time it's
    // only visible in this one.
    if (!sess.columnInstanceId || sess.columnInstanceId !== columnId) {
      updateSession(sessionId, { columnInstanceId: columnId });
    }
  }
}

export function updateSession(id: string, patch: Partial<ClaudeSession>) {
  sessionsState.list = sessionsState.list.map((s) => (s.id === id ? { ...s, ...patch } : s));
}

/** Attach an array of absolute filesystem paths as file-mentions to the given
    session. Appends `@<rel>` tokens to the input (mirrors the drop-from-FileTree
    flow) and skips paths already referenced by externalId. Called from the
    composer's + button (AgentColumn) and from the OS drag-drop listener
    (+page.svelte). */
export function attachPathsToSession(sessionId: string, paths: string[]): number {
  const s = sessionsState.list.find((x) => x.id === sessionId);
  if (!s || paths.length === 0) return 0;
  const existing = new Set(s.mentions.map((m) => m.externalId));
  const fresh: Mention[] = [];
  let input = s.input;
  for (const p of paths) {
    const rel = s.cwd && p.startsWith(s.cwd + '/') ? p.slice(s.cwd.length + 1) : p;
    if (existing.has(rel)) continue;
    existing.add(rel);
    const trimmed = p.endsWith('/') ? p.slice(0, -1) : p;
    const slash = trimmed.lastIndexOf('/');
    const name = slash >= 0 ? trimmed.slice(slash + 1) : trimmed;
    fresh.push({ source: 'file', externalId: rel, title: name, body: p, isDir: false });
    const sep = input && !input.endsWith(' ') ? ' ' : '';
    input = input + sep + '@' + rel + ' ';
  }
  if (fresh.length === 0) return 0;
  updateSession(sessionId, { input, mentions: [...s.mentions, ...fresh] });
  return fresh.length;
}

/** Route a click on a session tab through the per-kind pointer, the
    per-column-instance pointer (if bound), and the cross-column "currently
    focused" pointer. Legacy code paths that read the active session
    (sendClaudeMessage, pickCwd, worktree ops, …) keep working because
    `activeClaudeId` still points at whatever was last clicked. */
export function focusSession(id: string) {
  const sess = sessionsState.list.find((s) => s.id === id);
  if (!sess) return;
  sessionsState.activeIds[sess.agentKind] = id;
  sessionsState.activeClaudeId = id;
  if (sess.columnInstanceId) {
    sessionsState.activeByInstance[sess.columnInstanceId] = id;
  }
}

/** Sessions that should render in a given column instance. First instance of
 *  its kind in a workbench also adopts floating (unbound) sessions so
 *  pre-v2 persisted sessions don't disappear on upgrade. */
export function sessionsForInstance(
  instanceId: string,
  kind: 'claude' | 'cursor',
  isFirstOfKind: boolean
): ClaudeSession[] {
  return sessionsState.list.filter((s) => {
    if (s.agentKind !== kind) return false;
    if (s.columnInstanceId === instanceId) return true;
    if (isFirstOfKind && s.columnInstanceId === null) return true;
    return false;
  });
}

/** Return the session active in a given column instance, falling back to the
 *  first session visible in that column. */
export function activeSessionInInstance(
  instanceId: string,
  kind: 'claude' | 'cursor',
  isFirstOfKind: boolean
): ClaudeSession | null {
  const id = sessionsState.activeByInstance[instanceId];
  const visible = sessionsForInstance(instanceId, kind, isFirstOfKind);
  if (id) {
    const found = visible.find((s) => s.id === id);
    if (found) return found;
  }
  // Legacy per-kind pointer, for unbound sessions that drifted in.
  const legacy = sessionsState.activeIds[kind];
  if (legacy) {
    const found = visible.find((s) => s.id === legacy);
    if (found) return found;
  }
  return visible[0] ?? null;
}

export function appendSessionMessage(id: string, msg: ClaudeMessage) {
  sessionsState.list = sessionsState.list.map((s) =>
    s.id === id ? { ...s, messages: [...s.messages, msg] } : s
  );
}

/** Swap the agent CLI for a session. Rotates `claudeUuid` and resets the
    resumable flag because each CLI keeps its own session store — resuming
    a Claude id against cursor-agent (or vice versa) would fail. The UI
    history in Forgehold is retained but neither CLI will remember earlier
    turns on the new side. */
export function switchAgentKind(sessionId: string, kind: 'claude' | 'cursor') {
  const sess = sessionsState.list.find((s) => s.id === sessionId);
  if (!sess || sess.agentKind === kind) return;
  updateSession(sessionId, {
    agentKind: kind,
    claudeUuid: genUuid(),
    claudeResumable: false
  });
}

// ---- Low-level message / action mutators ----
// Live here because they only touch session state; +page.svelte calls them
// from the Claude streaming pipeline.

export function appendToLastAssistant(sessionId: string, delta: string) {
  sessionsState.list = sessionsState.list.map((s) => {
    if (s.id !== sessionId) return s;
    const msgs = [...s.messages];
    const last = msgs[msgs.length - 1];
    if (last && last.role === 'assistant') {
      msgs[msgs.length - 1] = { ...last, content: last.content + delta };
    }
    return { ...s, messages: msgs };
  });
}

export function replaceLastAssistant(sessionId: string, content: string) {
  sessionsState.list = sessionsState.list.map((s) => {
    if (s.id !== sessionId) return s;
    const msgs = [...s.messages];
    const last = msgs[msgs.length - 1];
    if (last && last.role === 'assistant') {
      msgs[msgs.length - 1] = { ...last, content };
    }
    return { ...s, messages: msgs };
  });
}

export function addAction(sessionId: string, action: ClaudeAction) {
  sessionsState.list = sessionsState.list.map((s) =>
    s.id === sessionId
      ? { ...s, actions: [...s.actions.filter((a) => a.id !== action.id), action] }
      : s
  );
}

export function updateAction(
  sessionId: string,
  actionId: string,
  patch: Partial<ClaudeAction>
) {
  sessionsState.list = sessionsState.list.map((s) => {
    if (s.id !== sessionId) return s;
    return {
      ...s,
      actions: s.actions.map((a) =>
        a.id === actionId ? ({ ...a, ...patch } as ClaudeAction) : a
      )
    };
  });
}

export function removeAction(sessionId: string, actionId: string) {
  sessionsState.list = sessionsState.list.map((s) =>
    s.id === sessionId ? { ...s, actions: s.actions.filter((a) => a.id !== actionId) } : s
  );
}

export function truncateSessionAt(sessionId: string, index: number) {
  sessionsState.list = sessionsState.list.map((s) => {
    if (s.id !== sessionId) return s;
    return {
      ...s,
      messages: s.messages.slice(0, index),
      // Drop any pending/errored action cards tied to the nuked turns.
      actions: [],
      // Claude CLI can't truncate mid-session — rotate to a fresh UUID so
      // the next send creates a new session without the old stale context.
      claudeUuid: genUuid(),
      claudeResumable: false
    };
  });
}

export function setSessionInput(sessionId: string, value: string) {
  sessionsState.list = sessionsState.list.map((s) =>
    s.id === sessionId ? { ...s, input: value } : s
  );
}
