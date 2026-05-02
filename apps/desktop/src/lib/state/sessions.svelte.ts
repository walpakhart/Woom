// Claude + Cursor chat session state. Owns the session list, per-column
// active-session pointers, the cross-column "currently focused" pointer,
// per-column scroll containers, and user-authored rules. Persists sessions
// to disk (~/Library/Application Support/Forgehold/sessions/) via Tauri
// fs commands; rules stay in localStorage (small + frequently mutated).

import { invoke } from '@tauri-apps/api/core';
import type { ClaudeAction, ClaudeMessage, ClaudeSession, ClaudeUsage, Mention, MessageEvent } from '$lib/types';
import { notify } from '$lib/state/toaster.svelte';
import { isImagePath } from '$lib/format';

export const SESSIONS_STORAGE_KEY = 'forgehold:claude-sessions:v1';
export const RULES_STORAGE_KEY = 'forgehold:claude-rules:v1';
export const EDITOR_STATE_STORAGE_KEY = 'forgehold:editor-state:v1';

// ---- Disk persistence internals ----
// After `initSessionsFromDisk()` runs, `_diskDir` is set and disk is the
// source of truth. Pre-migration, we fall back to localStorage so the app
// still works on the very first run with existing localStorage data.

let _diskDir: string | null = null; // e.g. "…/Forgehold/sessions"
let _diskWriteTimer: ReturnType<typeof setTimeout> | null = null;

function sessionIndexPath(): string { return `${_diskDir}/index.json`; }
function sessionFilePath(id: string): string { return `${_diskDir}/${id}.json`; }

function serializeSession(s: ClaudeSession): object {
  return {
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
    claudeModel: s.claudeModel,
    claudeToolProfile: s.claudeToolProfile,
    lastContextSize: s.lastContextSize,
    linkedToEditor: s.linkedToEditor,
    linkedToEditorInstanceId: s.linkedToEditorInstanceId,
    linkedCanvasId: s.linkedCanvasId,
    columnInstanceId: s.columnInstanceId,
    cwdSwitchRecap: s.cwdSwitchRecap,
    cwdUuids: s.cwdUuids,
    awaitingApproval: s.awaitingApproval
  };
}

async function flushToDisk(sessions: ClaudeSession[], activeId: string | null) {
  if (!_diskDir) return;
  try {
    const ids: string[] = [];
    for (const s of sessions) {
      await invoke('fs_write_file', {
        path: sessionFilePath(s.id),
        contents: JSON.stringify(serializeSession(s))
      });
      ids.push(s.id);
    }
    await invoke('fs_write_file', {
      path: sessionIndexPath(),
      contents: JSON.stringify({ activeId, ids })
    });
    if (persistError.sessions) persistError.sessions = null;
  } catch (e) {
    const msg = asMessage(e);
    persistError.sessions = msg;
    if (!sessionsToastFired) {
      sessionsToastFired = true;
      notify({
        kind: 'error',
        title: "Couldn't save chats",
        body: `${msg}. New messages stay in memory but won't survive a restart. See Settings → Storage.`,
        ttlMs: null
      });
    }
  }
}

function scheduleDiskWrite() {
  if (_diskWriteTimer) clearTimeout(_diskWriteTimer);
  _diskWriteTimer = setTimeout(() => {
    _diskWriteTimer = null;
    void flushToDisk(sessionsState.list, sessionsState.activeClaudeId);
  }, 1500);
}

function loadStoredEditorState(): Record<
  string,
  { repoPath: string; pendingOpenFile?: string | null }
> {
  if (typeof localStorage === 'undefined') return {};
  try {
    const raw = localStorage.getItem(EDITOR_STATE_STORAGE_KEY);
    if (!raw) return {};
    const parsed = JSON.parse(raw) as Record<string, unknown>;
    const out: Record<string, { repoPath: string; pendingOpenFile?: string | null }> = {};
    for (const [k, v] of Object.entries(parsed)) {
      if (v && typeof v === 'object' && typeof (v as { repoPath?: unknown }).repoPath === 'string') {
        // Intentionally drop `pendingOpenFile` on rehydrate. It's a
        // one-shot signal from the diff card to the editor; persisting
        // it would cause the editor to silently re-open a stale file
        // every reload.
        out[k] = { repoPath: (v as { repoPath: string }).repoPath };
      }
    }
    return out;
  } catch {
    return {};
  }
}

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
    const sessions = (data.sessions || []).map(hydrateSession);
    return { sessions, activeId: data.activeId ?? sessions[0]?.id ?? null };
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
  // Per-column-instance editor state (repoPath shown in that Editor column,
  // plus a transient `pendingOpenFile` that any source — diff card, MCP
  // tool, future "go to file" UI — can set to ask EditorView to focus a
  // specific file. Keyed by PanelInstance.id.
  editorInstanceState: Record<
    string,
    { repoPath: string; pendingOpenFile?: string | null }
  >;
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
  editorInstanceState: loadStoredEditorState()
});

// ---- Persistence ----
// `initSessionsFromDisk` is called from +page.svelte onMount and migrates
// sessions from localStorage → disk on first run, then keeps disk as the
// source of truth. `persistSessionsEffect` wires the reactive $effect that
// schedules debounced disk writes on every state change; pre-migration it
// falls back to localStorage so the app works on the very first run.

export const persistError = $state<{ sessions: string | null; rules: string | null }>({
  sessions: null,
  rules: null
});

let sessionsToastFired = false;
let rulesToastFired = false;

function asMessage(e: unknown): string {
  if (e instanceof Error) return e.message || e.name;
  if (typeof e === 'string') return e;
  return String(e);
}

/** Load sessions from a raw parsed object — same hydration as loadStoredSessions. */
function hydrateSession(s: ClaudeSession): ClaudeSession {
  return {
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
    claudeUuid: (s as { claudeUuid?: string }).claudeUuid || genUuid(),
    claudeResumable: Boolean((s as { claudeResumable?: boolean }).claudeResumable),
    agentKind: ((s as { agentKind?: 'claude' | 'cursor' }).agentKind ?? 'claude'),
    cursorModel: (s as { cursorModel?: string | null }).cursorModel ?? null,
    claudeModel: (s as { claudeModel?: string | null }).claudeModel ?? null,
    claudeToolProfile:
      (s as { claudeToolProfile?: ClaudeSession['claudeToolProfile'] }).claudeToolProfile ?? null,
    lastContextSize: (s as { lastContextSize?: number }).lastContextSize ?? 0,
    linkedToEditor: Boolean((s as { linkedToEditor?: boolean }).linkedToEditor),
    linkedToEditorInstanceId:
      (s as { linkedToEditorInstanceId?: string | null }).linkedToEditorInstanceId ?? null,
    linkedCanvasId:
      (s as { linkedCanvasId?: string | null }).linkedCanvasId ?? null,
    columnInstanceId:
      (s as { columnInstanceId?: string | null }).columnInstanceId ?? null,
    cwdSwitchRecap:
      (s as { cwdSwitchRecap?: string | null }).cwdSwitchRecap ?? null,
    cwdUuids:
      (s as { cwdUuids?: Record<string, string> }).cwdUuids ?? {},
    awaitingApproval:
      Boolean((s as { awaitingApproval?: boolean }).awaitingApproval)
  };
}

/** Called from +page.svelte onMount with the resolved app-data dir.
 *  Tries to load sessions from disk; if no disk sessions exist yet,
 *  migrates whatever is in localStorage to disk, then clears localStorage. */
export async function initSessionsFromDisk(appDataDir: string): Promise<void> {
  _diskDir = `${appDataDir}/sessions`;
  try {
    const exists = await invoke<boolean>('fs_path_exists', { path: sessionIndexPath() });
    if (exists) {
      const raw = await invoke<string>('fs_read_file', { path: sessionIndexPath() });
      const index = JSON.parse(raw) as { activeId: string | null; ids: string[] };
      const sessions: ClaudeSession[] = [];
      for (const id of index.ids ?? []) {
        try {
          const sessionRaw = await invoke<string>('fs_read_file', { path: sessionFilePath(id) });
          sessions.push(hydrateSession(JSON.parse(sessionRaw) as ClaudeSession));
        } catch {
          // Skip corrupt or missing session files — they may have been manually
          // deleted or lost to a partial write.
        }
      }
      sessionsState.list = sessions;
      sessionsState.activeClaudeId = index.activeId ?? sessions[0]?.id ?? null;
      const active = sessions.find((s) => s.id === sessionsState.activeClaudeId);
      if (active) sessionsState.activeIds[active.agentKind] = active.id;
    } else {
      // First run with disk persistence — migrate localStorage to disk.
      await flushToDisk(sessionsState.list, sessionsState.activeClaudeId);
    }
    // localStorage is no longer the source of truth. Clear it to free quota.
    try { localStorage.removeItem(SESSIONS_STORAGE_KEY); } catch { /* ignore */ }
  } catch (e) {
    console.error('[sessions] disk init failed, falling back to localStorage:', e);
    _diskDir = null; // Keep localStorage path active.
  }
}

export function persistSessionsEffect() {
  $effect(() => {
    if (_diskDir) {
      // Disk is active — debounce writes to avoid hammering disk during streaming.
      const list = sessionsState.list;
      const activeId = sessionsState.activeClaudeId;
      void list; void activeId; // ensure svelte tracks these reactive reads
      scheduleDiskWrite();
      return;
    }
    // Pre-migration localStorage fallback.
    if (typeof localStorage === 'undefined') return;
    try {
      const payload = {
        sessions: sessionsState.list.map(serializeSession),
        activeId: sessionsState.activeClaudeId
      };
      localStorage.setItem(SESSIONS_STORAGE_KEY, JSON.stringify(payload));
      if (persistError.sessions) persistError.sessions = null;
    } catch (e) {
      const msg = asMessage(e);
      persistError.sessions = msg;
      if (!sessionsToastFired) {
        sessionsToastFired = true;
        notify({
          kind: 'error',
          title: "Couldn't save chats",
          body: `${msg}. New messages stay in memory but won't survive a restart. See Settings → Storage.`,
          ttlMs: null
        });
      }
    }
  });
}

export function persistRulesEffect() {
  $effect(() => {
    if (typeof localStorage === 'undefined') return;
    try {
      localStorage.setItem(RULES_STORAGE_KEY, sessionsState.userRules);
      if (persistError.rules) persistError.rules = null;
    } catch (e) {
      const msg = asMessage(e);
      persistError.rules = msg;
      if (!rulesToastFired) {
        rulesToastFired = true;
        notify({ kind: 'warning', title: "Couldn't save rules", body: msg, ttlMs: null });
      }
    }
  });
}

/** Persist `sessionsState.editorInstanceState` so each editor column
 *  remembers its open folder across reloads. Without this, the
 *  agent-driven `set_editor_repo_path` (and the manual user-side path
 *  picker) would visually revert on every restart even though sessions
 *  themselves persist their cwd.
 *
 *  Strips the transient `pendingOpenFile` field on save — that's a
 *  one-shot signal between the diff card and EditorView; persisting it
 *  would cause the editor to silently re-open a stale file on every
 *  reload. The serialized shape stays {repoPath} only. */
export function persistEditorInstanceStateEffect() {
  $effect(() => {
    if (typeof localStorage === 'undefined') return;
    try {
      const stripped: Record<string, { repoPath: string }> = {};
      for (const [k, v] of Object.entries(sessionsState.editorInstanceState)) {
        stripped[k] = { repoPath: v.repoPath };
      }
      localStorage.setItem(EDITOR_STATE_STORAGE_KEY, JSON.stringify(stripped));
    } catch {
      // Quota / SSR / private mode: silent. Editor path won't survive a
      // restart in that environment, but it's recoverable by re-picking.
    }
  });
}

/** Ask the editor instance `instanceId` to open `filePath` as its active
 *  tab. EditorView watches this slot via $effect and consumes it on the
 *  next reactive tick (calling `consumeEditorOpenFile` to clear the
 *  signal so subsequent identical clicks still re-trigger).
 *
 *  Idempotent if the slot is missing — we lazily create it (matches the
 *  pattern EditorColumn uses on mount). The caller is responsible for
 *  ensuring `instanceId` actually points to an editor column; if it
 *  doesn't, the signal sits in state forever. In practice the diff card
 *  resolves a real instanceId via `findInstanceAnywhere` before
 *  calling. */
export function requestEditorOpenFile(instanceId: string, filePath: string): void {
  if (!filePath) return;
  const slot = sessionsState.editorInstanceState[instanceId];
  if (!slot) {
    sessionsState.editorInstanceState[instanceId] = { repoPath: '', pendingOpenFile: filePath };
    return;
  }
  // Reassign the slot so Svelte's reactive proxy catches the mutation —
  // direct field assignment on a deeply-nested $state object can miss
  // when the object reference is preserved across writes. The spread
  // also ensures consume sees a fresh write even when the same path is
  // requested twice in a row (otherwise the value-equality check would
  // skip the second click).
  sessionsState.editorInstanceState = {
    ...sessionsState.editorInstanceState,
    [instanceId]: { ...slot, pendingOpenFile: filePath }
  };
}

/** Pop the pending file path off the editor's slot. Returns the path
 *  that was set (or undefined if there's no signal). Intended to be
 *  called from EditorView's $effect right after it forwards the value
 *  into its internal `openFile`. Always clears the slot so the next
 *  request re-triggers, even if it's the same path. */
export function consumeEditorOpenFile(instanceId: string): string | undefined {
  const slot = sessionsState.editorInstanceState[instanceId];
  const pending = slot?.pendingOpenFile;
  if (!pending) return undefined;
  sessionsState.editorInstanceState = {
    ...sessionsState.editorInstanceState,
    [instanceId]: { ...slot, pendingOpenFile: null }
  };
  return pending;
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
      // Default new Claude sessions to Sonnet 4.6 — Opus 4.7 (the CLI
      // default on Max plans) burns ~5x more 5h quota for output and
      // most Forge tasks (UI edits, code reads, search, triage) don't
      // need Opus reasoning. Users opt in to Opus per session via the
      // model chip when they actually need it.
      claudeModel: agentKind === 'claude' ? 'claude-sonnet-4-6' : null,
      // Default new Claude sessions to 'coding' profile (App nav +
      // Memory only). Saves ~10-15k tokens of MCP schemas on every
      // turn vs wiring all 60+ tools. User flips to 'github' /
      // 'jira' / 'sentry' / 'triage' / 'all' via the profile chip
      // when needed.
      claudeToolProfile: agentKind === 'claude' ? 'coding' : null,
      lastContextSize: 0,
      linkedToEditor: !!opts.linkedToEditor,
      linkedToEditorInstanceId: opts.linkedToEditorInstanceId ?? null,
      linkedCanvasId: null,
      columnInstanceId,
      cwdSwitchRecap: null,
      cwdUuids: {},
      awaitingApproval: false
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
  // Every per-column pointer that was on this session jumps to the next
  // visible session of the same kind (or null if none remain).
  const fallback = rest.find((s) => s.agentKind === kind)?.id ?? null;
  for (const k of Object.keys(sessionsState.activeByInstance)) {
    if (sessionsState.activeByInstance[k] === id) {
      sessionsState.activeByInstance[k] = fallback;
    }
  }
  if (sessionsState.activeIds[kind] === id) sessionsState.activeIds[kind] = fallback;
  if (sessionsState.activeClaudeId === id) {
    sessionsState.activeClaudeId =
      fallback ?? sessionsState.activeIds[kind === 'claude' ? 'cursor' : 'claude'];
  }
  // Auto-create a fresh Claude chat if the user emptied the list — keeps
  // the chat column from sitting on a permanent empty state.
  if (rest.filter((s) => s.agentKind === 'claude').length === 0) {
    newClaudeSession({ agentKind: 'claude' });
  }
}

/** Clean up per-column pointers when a column instance is removed. The
 *  session list is global per-kind so nothing in `list` needs touching —
 *  only the per-instance bookkeeping. */
export function orphanSessionsForInstance(instanceId: string) {
  delete sessionsState.activeByInstance[instanceId];
  delete sessionsState.scrollEls[instanceId];
}

/** Set the active session pointer for one specific column instance. The
 *  session list is global per-kind, so this only affects which chat is
 *  shown when the user looks at this column — it doesn't move the chat
 *  out of any other column. */
export function setActiveSessionInColumn(columnId: string, sessionId: string) {
  sessionsState.activeByInstance[columnId] = sessionId;
  const sess = sessionsState.list.find((s) => s.id === sessionId);
  if (sess) {
    sessionsState.activeIds[sess.agentKind] = sessionId;
    sessionsState.activeClaudeId = sessionId;
    // Track "last shown in" for telemetry/UX-niceties only — does NOT
    // affect what's visible in other columns.
    if (sess.columnInstanceId !== columnId) {
      updateSession(sessionId, { columnInstanceId: columnId });
    }
  }
}

export function updateSession(id: string, patch: Partial<ClaudeSession>) {
  sessionsState.list = sessionsState.list.map((s) => (s.id === id ? { ...s, ...patch } : s));
}

/** Attach a specific line range of a file as a `@path:start-end` mention
 *  to the session's composer. Used by the editor's "Apply to <agent>"
 *  bar so the user can pin a selection to an agent without typing the
 *  range manually.
 *
 *  Token shape:
 *    • Range:  `@<rel-or-basename>:<startLine>-<endLine>`  (e.g. `@src/foo.ts:45-67`)
 *    • Single: `@<rel-or-basename>:<line>`                 (when start === end)
 *
 *  Storage:
 *    • One Mention per token: `externalId = <token>` (without the `@`),
 *      `body = absolute path`, `title = <basename>:<startLine>-<endLine>`.
 *      Different ranges of the same file produce distinct mentions
 *      (their tokens differ), so re-clicking "Apply to" on an
 *      adjusted selection adds a NEW pin instead of clobbering.
 *    • Token is appended to `input` with a leading-space guard so the
 *      backdrop's `@[^\s]+` regex tokenises it as a single highlight.
 *
 *  Returns the token (without `@`) so the caller can chain it (e.g.
 *  toast "Pinned src/foo.ts:45-67 to Claude · Mona-Lisa"). Returns
 *  null if the session doesn't exist. */
export function attachLineRangeMention(
  sessionId: string,
  filePath: string,
  startLine: number,
  endLine: number
): string | null {
  const s = sessionsState.list.find((x) => x.id === sessionId);
  if (!s) return null;
  const lo = Math.min(startLine, endLine);
  const hi = Math.max(startLine, endLine);
  const rangeSuffix = lo === hi ? `:${lo}` : `:${lo}-${hi}`;

  const rel =
    s.cwd && filePath.startsWith(s.cwd + '/') ? filePath.slice(s.cwd.length + 1) : null;
  const trimmed = filePath.endsWith('/') ? filePath.slice(0, -1) : filePath;
  const slash = trimmed.lastIndexOf('/');
  const name = slash >= 0 ? trimmed.slice(slash + 1) : trimmed;
  const baseToken = rel ?? name;
  const token = `${baseToken}${rangeSuffix}`;

  const existing = new Set(s.mentions.map((m) => m.externalId));
  let nextInput = s.input;
  let nextMentions = s.mentions;
  if (!existing.has(token)) {
    const sep = nextInput && !nextInput.endsWith(' ') ? ' ' : '';
    nextInput = nextInput + sep + '@' + token + ' ';
    nextMentions = [
      ...s.mentions,
      {
        source: 'file',
        externalId: token,
        title: `${name}${rangeSuffix}`,
        body: filePath,
        isDir: false
      }
    ];
  }
  updateSession(sessionId, { input: nextInput, mentions: nextMentions });
  return token;
}

/** Drop file/Jira/etc. mentions whose `@<externalId>` is no longer
 *  present in `s.input`. Called from the textarea's `oninput` so
 *  that backspacing the `@token` actually unattaches the file —
 *  otherwise the mention silently survives in `s.mentions` and
 *  haunts the user in two visible ways:
 *
 *    1. The composer placeholder stays "Ask about the attached
 *       items …" forever even when the visible chip strip / textarea
 *       are empty (the placeholder is wired to `mentions.length`).
 *    2. The next `sendAgent` snapshots the still-present mentions
 *       and bakes them into the prompt as `Referenced file: @…`,
 *       so e.g. Cursor receives "Привет, what about resolve-
 *       components.js?" even though the user just typed "ку".
 *
 *  Image mentions are kept regardless: they live as thumbnail chips
 *  in the attach-row above the composer (no `@token` is ever written
 *  to the textarea for them — the path can have spaces and the
 *  user wouldn't see what the token resolved to anyway), so the
 *  textarea text is the wrong source of truth for them. The chip's
 *  X button (`removeMention`) is the user-facing remove path for
 *  images.
 *
 *  Same `(?:^|\s)@([^\s]+)` regex shape the backdrop highlighter
 *  uses, so we don't accidentally drop a mention because its `@`
 *  is part of an email address. */
export function pruneMentionsByInput(sessionId: string, input: string) {
  const s = sessionsState.list.find((x) => x.id === sessionId);
  if (!s || s.mentions.length === 0) return;
  const present = new Set<string>();
  const re = /(?:^|\s)@([^\s]+)/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(input)) !== null) {
    present.add(m[1]);
  }
  const next = s.mentions.filter((mention) => {
    if (
      mention.source === 'file' &&
      !mention.isDir &&
      mention.body &&
      isImagePath(mention.body)
    ) {
      return true;
    }
    return present.has(mention.externalId);
  });
  if (next.length === s.mentions.length) return;
  updateSession(sessionId, { mentions: next });
}

/** Attach an array of absolute filesystem paths as file-mentions to the given
    session. Called from the composer's + button (AgentColumn) and from the
    OS drag-drop listener (+page.svelte). Skips paths already referenced.

    Two attachment shapes depending on the file kind:
      • Image (png/jpg/…): mention only — no `@token` in input. Renders as a
        thumbnail chip strip above the composer (see AgentColumn). Path can
        contain spaces without breaking the inline @-parser.
      • Anything else: mention + `@token` in input. Token is the cwd-relative
        path when inside cwd, otherwise just the basename. */
export function attachPathsToSession(sessionId: string, paths: string[]): number {
  const s = sessionsState.list.find((x) => x.id === sessionId);
  if (!s || paths.length === 0) return 0;
  const existing = new Set(s.mentions.map((m) => m.externalId));
  const fresh: Mention[] = [];
  let input = s.input;
  for (const p of paths) {
    const rel =
      s.cwd && p.startsWith(s.cwd + '/') ? p.slice(s.cwd.length + 1) : null;
    const trimmed = p.endsWith('/') ? p.slice(0, -1) : p;
    const slash = trimmed.lastIndexOf('/');
    const name = slash >= 0 ? trimmed.slice(slash + 1) : trimmed;
    const isImage = isImagePath(p);
    // For images, externalId is the absolute path so two drops of the same
    // file dedupe even when basenames collide across folders. The chip
    // strip uses the path directly via convertFileSrc anyway.
    const token = isImage ? p : (rel ?? name);
    if (existing.has(token)) continue;
    existing.add(token);
    fresh.push({ source: 'file', externalId: token, title: name, body: p, isDir: false });
    if (!isImage) {
      const sep = input && !input.endsWith(' ') ? ' ' : '';
      input = input + sep + '@' + token + ' ';
    }
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

/** Sessions that should render in a given column instance. As of 2026-04-25
 *  the chat list is **global per agent-kind** — every Claude column sees
 *  every Claude chat, every Cursor column sees every Cursor chat. The
 *  column is just a viewing window with its own "currently open" pointer.
 *  `columnInstanceId` on a session is now informational ("last shown here").
 *
 *  `_isFirstOfKind` is kept for prop-shape compatibility with the column
 *  components but is unused — left in so we don't have to ripple a prop
 *  removal through three files for a refactor that's already this large. */
export function sessionsForInstance(
  _instanceId: string,
  kind: 'claude' | 'cursor',
  _isFirstOfKind: boolean
): ClaudeSession[] {
  return sessionsState.list.filter((s) => s.agentKind === kind);
}

/** Return the session active in a given column instance. Each column owns
 *  its own active pointer so two columns can show different chats; the
 *  list itself is shared. Falls back to the most-recent session of the
 *  same kind when the column hasn't picked one yet (fresh column = shows
 *  newest chat instead of an empty pane). */
export function activeSessionInInstance(
  instanceId: string,
  kind: 'claude' | 'cursor',
  _isFirstOfKind: boolean
): ClaudeSession | null {
  const visible = sessionsState.list.filter((s) => s.agentKind === kind);
  const pinned = sessionsState.activeByInstance[instanceId];
  if (pinned) {
    const found = visible.find((s) => s.id === pinned);
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
    a Claude id against cursor-agent (or vice versa) would fail. Also
    drops the per-cwd uuid map: those ids are CLI-specific (a saved
    claudeUuid wouldn't resume in cursor-agent), so carrying the map
    across a CLI swap would only mislead future cwd switches. The UI
    history in Forgehold is retained but neither CLI will remember
    earlier turns on the new side. */
export function switchAgentKind(sessionId: string, kind: 'claude' | 'cursor') {
  const sess = sessionsState.list.find((s) => s.id === sessionId);
  if (!sess || sess.agentKind === kind) return;
  updateSession(sessionId, {
    agentKind: kind,
    claudeUuid: genUuid(),
    claudeResumable: false,
    cwdUuids: {}
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
      // Mirror into both `content` (legacy concat for search /
      // replaceLastAssistant / back-compat with old persisted messages)
      // AND `events` (new ordered array — text deltas merge into the
      // last text event, or start a new one if the previous event was
      // a trace block). This preserves interleaving without breaking
      // any code that still reads `content`.
      const events = [...(last.events ?? [])];
      const lastEv = events[events.length - 1];
      if (lastEv && lastEv.kind === 'text') {
        events[events.length - 1] = { kind: 'text', body: lastEv.body + delta };
      } else {
        events.push({ kind: 'text', body: delta });
      }
      msgs[msgs.length - 1] = { ...last, content: last.content + delta, events };
    }
    return { ...s, messages: msgs };
  });
}

/** Mirror of `appendToLastAssistant` for `thinking` content blocks.
    Concatenates onto the last assistant message's `thinking` field
    (lazily initialised). The pill in AgentColumn collapses these into
    a "Thinking ✓" button the user can expand to inspect after the
    final answer lands. */
export function appendToLastThinking(sessionId: string, delta: string) {
  sessionsState.list = sessionsState.list.map((s) => {
    if (s.id !== sessionId) return s;
    const msgs = [...s.messages];
    const last = msgs[msgs.length - 1];
    if (last && last.role === 'assistant') {
      msgs[msgs.length - 1] = { ...last, thinking: (last.thinking ?? '') + delta };
    }
    return { ...s, messages: msgs };
  });
}

/** Append a tool-use trace line (already formatted by `formatToolUse`)
    onto the last assistant message. Mirrored into:
      - the legacy `trace: string` field (joined with `\n\n` between
        segments) — kept so older renderers / persisted-but-not-yet-
        re-rendered messages still work
      - the new `events` array (consecutive trace segments merge into
        one event with multiple segments — that one event renders as
        a single "✓ N steps" pill, where N = segments.length). When a
        trace event lands AFTER a text event, a new trace event opens
        so the chat shows: text → pill → text → pill in chronological
        order, instead of the old "all pills at top + all text below". */
export function appendToLastTrace(sessionId: string, segment: string) {
  if (!segment.trim()) return;
  sessionsState.list = sessionsState.list.map((s) => {
    if (s.id !== sessionId) return s;
    const msgs = [...s.messages];
    const last = msgs[msgs.length - 1];
    if (last && last.role === 'assistant') {
      const prev = last.trace ?? '';
      const nextTrace = prev ? `${prev}\n\n${segment}` : segment;
      const events = [...(last.events ?? [])];
      const lastEv = events[events.length - 1];
      if (lastEv && lastEv.kind === 'trace') {
        events[events.length - 1] = { kind: 'trace', segments: [...lastEv.segments, segment] };
      } else {
        events.push({ kind: 'trace', segments: [segment] });
      }
      msgs[msgs.length - 1] = { ...last, trace: nextTrace, events };
    }
    return { ...s, messages: msgs };
  });
}

/** Stamp a `usage` snapshot onto the last assistant message and update
 *  the session's `lastContextSize`. Called from the stream handler on
 *  every assistant event that carries a `usage` block; we keep
 *  overwriting (vs accumulating) so the stamp on the message reflects
 *  the FINAL sub-step of the turn — that step's `cache_read` tokens
 *  cover the entire prior conversation, which is the most useful single
 *  number to surface in the per-message badge. The session-level
 *  `lastContextSize` is similarly the most-recent value, since it
 *  represents "how much context did Claude actually look at on the
 *  last hop" — drives the context-window % indicator. */
export function updateLastAssistantUsage(sessionId: string, usage: ClaudeUsage) {
  sessionsState.list = sessionsState.list.map((s) => {
    if (s.id !== sessionId) return s;
    const msgs = [...s.messages];
    const last = msgs[msgs.length - 1];
    if (last && last.role === 'assistant') {
      msgs[msgs.length - 1] = { ...last, usage };
    }
    return { ...s, messages: msgs, lastContextSize: usage.contextSize };
  });
}

export function replaceLastAssistant(sessionId: string, content: string) {
  sessionsState.list = sessionsState.list.map((s) => {
    if (s.id !== sessionId) return s;
    const msgs = [...s.messages];
    const last = msgs[msgs.length - 1];
    if (last && last.role === 'assistant') {
      // Wipe events too — the renderer prefers events when present, so
      // leaving an empty events list with non-empty content would
      // render an empty bubble. Reset to a single text event matching
      // the new content.
      const events = content
        ? ([{ kind: 'text', body: content }] as ClaudeMessage['events'])
        : [];
      msgs[msgs.length - 1] = { ...last, content, events };
    }
    return { ...s, messages: msgs };
  });
}

/** Append a Cursor-style inline diff card into the LAST assistant message
 *  for `sessionId`. Same lifetime model as `appendToLastTrace` — we attach
 *  to whichever assistant turn is currently streaming, so the diff lands
 *  in the chat *exactly* where the agent ran the Edit, not bunched at the
 *  end. Idempotent on `toolId`: if the same tool_use id already produced
 *  an edit event (e.g. the stream replayed), we update the existing event
 *  in place rather than duplicating.
 *
 *  No-op when the last message isn't an assistant turn (defensive: stream
 *  events should only fire after the assistant message starts, but that
 *  ordering depends on cursor-agent / claude correctly emitting
 *  `assistant` before `tool_call`). */
export function appendEditEvent(
  sessionId: string,
  ev: {
    toolId: string;
    filePath: string;
    oldText: string;
    newText: string;
    isCreate: boolean;
    /** True for cursor's deletion tool. Mutually exclusive with
     *  `isCreate`. The card flips Revert→Restore semantics; sessions
     *  state stores it verbatim so EditDiffCard can branch on it. */
    isDelete?: boolean;
    /** True for `Write` — full-file overwrite. Picks `revert_write`
     *  semantics over `revert_edit` (full rewrite vs unique-substring
     *  replace), and changes the card's verb. Defaults to false. */
    wholeFile?: boolean;
    /** Initial status. Edit/MultiEdit emit fully-formed events so they
     *  start at `applied`. Write events arrive with a placeholder
     *  `oldText` and finish loading once `git show HEAD:<file>` returns
     *  — they should start at `loading`. Delete events normally start
     *  at `applied` (cursor hands us prevContent inline) but fall back
     *  to `loading` when prevContent is missing and we have to git-
     *  show-backfill, same as Write. */
    status?: 'loading' | 'applied';
  }
) {
  sessionsState.list = sessionsState.list.map((s) => {
    if (s.id !== sessionId) return s;
    const msgs = [...s.messages];
    const last = msgs[msgs.length - 1];
    if (!last || last.role !== 'assistant') return s;
    const events = [...(last.events ?? [])];
    const existingIdx = events.findIndex(
      (e) => e.kind === 'edit' && e.toolId === ev.toolId
    );
    const next: MessageEvent = {
      kind: 'edit',
      toolId: ev.toolId,
      filePath: ev.filePath,
      oldText: ev.oldText,
      newText: ev.newText,
      isCreate: ev.isCreate,
      isDelete: ev.isDelete ?? false,
      wholeFile: ev.wholeFile ?? false,
      status: ev.status ?? 'applied'
    };
    if (existingIdx >= 0) {
      events[existingIdx] = next;
    } else {
      events.push(next);
    }
    msgs[msgs.length - 1] = { ...last, events };
    return { ...s, messages: msgs };
  });
}

/** Patch an existing edit-event in place. Used in two scenarios:
 *  - Revert click → flip `status` to `reverted` / `error` (and set
 *    `note` on failure).
 *  - Async Write backfill → swap in the `oldText` we just fetched from
 *    `git show HEAD:<file>` and flip `status` from `loading` to
 *    `applied`. Optionally also patch `isCreate` (true if the file
 *    didn't exist in HEAD).
 *  Walks every assistant message in the session because the user might
 *  revert an edit from a turn far back in history; the async git
 *  fetch always lands on the most recent placeholder, but routing it
 *  through the same lookup keeps both cases on the same code path. */
export function updateEditEvent(
  sessionId: string,
  toolId: string,
  patch: {
    status?: 'loading' | 'applied' | 'kept' | 'reverted' | 'error';
    note?: string;
    oldText?: string;
    isCreate?: boolean;
  }
) {
  sessionsState.list = sessionsState.list.map((s) => {
    if (s.id !== sessionId) return s;
    let touched = false;
    const msgs = s.messages.map((m) => {
      if (m.role !== 'assistant' || !m.events) return m;
      let mTouched = false;
      const events = m.events.map((e) => {
        if (e.kind !== 'edit' || e.toolId !== toolId) return e;
        mTouched = true;
        return {
          ...e,
          status: patch.status ?? e.status,
          note: patch.note !== undefined ? patch.note : e.note,
          oldText: patch.oldText !== undefined ? patch.oldText : e.oldText,
          isCreate: patch.isCreate !== undefined ? patch.isCreate : e.isCreate
        };
      });
      if (!mTouched) return m;
      touched = true;
      return { ...m, events };
    });
    if (!touched) return s;
    return { ...s, messages: msgs };
  });
}

/** All edit events in `sessionId` whose status is still `applied` —
 *  the change is on disk and the user hasn't decided what to do with
 *  it yet (Keep moves to `kept`, Revert moves to `reverted`). Drives
 *  the bulk-action bar's count and its Keep all / Revert all targets.
 *
 *  Returns the events themselves (not just IDs) because the bulk
 *  handlers need `filePath`, `oldText`, `newText`, `wholeFile`,
 *  `isCreate`, and `isDelete` to dispatch the right Tauri command —
 *  same shape `EditDiffCard.svelte`'s individual handler uses. Order
 *  matches chat order. */
export function getPendingEditEvents(
  sessionId: string
): Array<
  Extract<MessageEvent, { kind: 'edit' }>
> {
  const s = sessionsState.list.find((x) => x.id === sessionId);
  if (!s) return [];
  const out: Array<Extract<MessageEvent, { kind: 'edit' }>> = [];
  for (const m of s.messages) {
    if (m.role !== 'assistant' || !m.events) continue;
    for (const e of m.events) {
      if (e.kind !== 'edit') continue;
      if (e.status !== 'applied') continue;
      out.push(e);
    }
  }
  return out;
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
