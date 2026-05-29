// Claude + Cursor chat session state. Owns the session list, per-app-instance
// active-session pointers, the cross-instance "currently focused" pointer,
// per-instance scroll containers, and user-authored rules. Persists sessions
// to disk (~/Library/Application Support/Woom/sessions/) via Tauri
// fs commands; rules stay in localStorage (small + frequently mutated).

import { invoke } from '@tauri-apps/api/core';
import type {
  ClaudeAction,
  ClaudeMessage,
  ClaudeSession,
  ClaudeUsage,
  Mention,
  MessageEvent,
  PendingActionResult
} from '$lib/types';
import { notify } from '$lib/state/toaster.svelte';
import { isImagePath } from '$lib/format';
import { contextWindowFor } from '$lib/usage';
import { buildContinuationRecap } from '$lib/services/sessionCwd';
import {
  applyOpsToSession,
  drainStreamQueue,
  dropStreamQueueFor,
  enqueueStream,
  flushStreamQueueNow as _flushStreamQueueImpl,
  setStreamFlushHandler,
} from './sessions_stream';
import { capMessageEvents, serializeSession } from './sessions_serialize';
import {
  flushAllNow,
  flushNow,
  getDiskDir,
  resetLastWrittenSnapshot,
  scheduleDiskWrite,
  sessionFilePathFor,
  sessionIndexFilePath,
  setDiskDir,
} from './sessions_disk.svelte';
import { persistError as _sharedPersistError } from './sessions_persist_error.svelte';

// Re-export so existing imports (`import { persistError } from
// '$lib/state/sessions.svelte'`) keep working post-split. New code
// should import directly from `sessions_persist_error.svelte`.
export const persistError = _sharedPersistError;

export const SESSIONS_STORAGE_KEY = 'woom:claude-sessions:v1';
export const RULES_STORAGE_KEY = 'woom:claude-rules:v1';
export const EDITOR_STATE_STORAGE_KEY = 'woom:editor-state:v1';

// ---- Disk persistence ----
// Internals (_diskDir, _diskWriteTimer, _lastWrittenRef, debounce
// timing, flushToDisk) moved to `./sessions_disk.svelte.ts` (wave-1
// phase-7 split). The host owns the reactive store + the $effect
// that calls `scheduleDiskWrite` on every change; the module
// handles the actual write logic + ceiling debounce.
// `initSessionsFromDisk` is called from +page.svelte onMount and
// migrates sessions from localStorage → disk on first run, then
// keeps disk as the source of truth. Pre-migration, we fall back
// to localStorage so the app still works on the very first run.

// ---- Streaming-delta batch queue ----
// Mechanics moved to `./sessions_stream.ts` (wave-1 phase-7 split).
// The host-side glue below registers a flush handler that drains
// the module-level queue and writes results back into the reactive
// `sessionsState.list` — keeping the actual store mutation inside
// this file so `sessions_stream.ts` stays free of $state imports
// (avoids a circular dep, and the queue stays unit-testable).

setStreamFlushHandler(() => {
  const work = drainStreamQueue();
  if (work.size === 0) return;
  /* Per-index mutation instead of full-list rebuild. The old
   * `.list = .list.map(...)` reassigned the array reference, which
   * Svelte 5 treats as a coarse-grained dirty on EVERY consumer of
   * `sessionsState.list` (ChatThread, ChatHeader, sidebar lists,
   * SDD cards, palette derivations, …). When 1 of 8 sessions
   * receives a delta we don't want the other 7's reactive subscribers
   * to re-run — fine-grained mutation of `.list[i]` only dirties
   * readers of that index. Cuts streaming-token reactive cost
   * proportional to parallel-session count. */
  const list = sessionsState.list;
  for (let i = 0; i < list.length; i++) {
    const ops = work.get(list[i].id);
    if (!ops || ops.length === 0) continue;
    const next = applyOpsToSession(list[i], ops);
    if (next !== list[i]) list[i] = next;
  }
});

/** Drain any queued streaming deltas synchronously. Re-exported from
 *  `sessions_stream` so existing callers in `+page.svelte`,
 *  `agentStream.ts`, etc. keep working unchanged. */
export function flushStreamQueueNow(): void {
  _flushStreamQueueImpl();
}

/* Per-event byte cap + `serializeSession` / `capMessageEvents` moved
 * to `./sessions_serialize.ts` (wave-1 phase-7 split). They're pure
 * functions with no reactive store dependency, so isolating them
 * makes the store file smaller and the utilities trivially unit-
 * testable. The cap value lives next to its only callers. */

/** Force an immediate, awaitable flush. Bypasses the debounce timer
 *  so callers (window-close hook, manual "Save now" affordance, …)
 *  can guarantee the on-disk copy reflects the in-memory state
 *  before the next browser tick / quit. Thin wrapper around the
 *  disk module's `flushNow` so the existing public symbol survives
 *  the split. */
export async function flushSessionsNow(): Promise<void> {
  await flushNow(sessionsState.list, sessionsState.activeClaudeId);
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
 *  component. Per-app-instance active pointers live in `activeByInstance`;
 *  legacy `activeIds` (per-kind) is kept as a fallback for floating
 *  sessions that haven't been bound to an instance yet. */
export const sessionsState = $state<{
  list: ClaudeSession[];
  // Per-agent-kind active session — fallback pointer for floating sessions
  // (agentInstanceId === null). Usually shadowed by per-instance pointers.
  activeIds: Record<'claude' | 'cursor', string | null>;
  // Per-app-instance active session. Key = PanelInstance.id. Each agent
  // app instance owns one entry; two Claude instances can focus different
  // sessions at the same time without stepping on each other.
  activeByInstance: Record<string, string | null>;
  // Cross-instance focus — whatever the user last clicked. Used by legacy
  // single-instance code paths (sendClaudeMessage, pickCwd, createWorktree, …)
  // that take no agent-kind argument.
  activeClaudeId: string | null;
  // Per-app-instance scroll containers. Each agent app registers its own
  // scroll element; we scroll each independently when its active session
  // streams.
  scrollEls: Record<string, HTMLDivElement | null>;
  // User-authored rules/preferences appended to Claude's system prompt on
  // every turn via `--append-system-prompt`. Edited in the Rules view.
  userRules: string;
  // Per-app-instance editor state (repoPath shown in that Editor instance,
  // plus a transient `pendingOpenFile` that any source — diff card, MCP
  // tool, future "go to file" UI — can set to ask EditorView to focus a
  // specific file. Keyed by PanelInstance.id.
  editorInstanceState: Record<
    string,
    { repoPath: string; pendingOpenFile?: string | null }
  >;
  /** Transient "expand the InlineClaude row for this session" signal.
   *  Set by `applyRangeToAgent` after a user clicks "Apply to <agent>"
   *  in the editor selection bar; consumed by InlineClaude's effect
   *  which auto-expands the matching row so the user can immediately
   *  type a follow-up alongside the freshly-pinned `@path:line-range`
   *  mention. Cleared back to `null` after consumption. */
  requestInlineExpandFor: string | null;
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
  editorInstanceState: loadStoredEditorState(),
  requestInlineExpandFor: null
});

// ---- Persistence ----
// `initSessionsFromDisk` is called from +page.svelte onMount and migrates
// sessions from localStorage → disk on first run, then keeps disk as the
// source of truth. `persistSessionsEffect` wires the reactive $effect that
// schedules debounced disk writes on every state change; pre-migration it
// falls back to localStorage so the app works on the very first run.
// `persistError` is re-exported above; sessions-write toast lives in the
// disk module, rules-write toast stays here.

let rulesToastFired = false;
/* localStorage-fallback toast — separate from the disk-write toast
 * inside `sessions_disk` because the two paths can fail independently
 * (disk write succeeds but localStorage migration cleanup fails, or
 * vice versa pre-migration). */
let sessionsLocalToastFired = false;

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
    /* Run the same per-event cap on load: existing on-disk sessions
     * that pre-date the serialize-side cap still have giant edit
     * payloads; capping here means a single launch heals them, and
     * the next persist round writes the trimmed shape back to disk. */
    messages: capMessageEvents(s.messages || []),
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
    lastContextSize: (s as { lastContextSize?: number }).lastContextSize ?? 0,
    linkedToEditor: Boolean((s as { linkedToEditor?: boolean }).linkedToEditor),
    linkedToEditorInstanceId:
      (s as { linkedToEditorInstanceId?: string | null }).linkedToEditorInstanceId ?? null,
    linkedCanvasId:
      (s as { linkedCanvasId?: string | null }).linkedCanvasId ?? null,
    linkedTerminalInstanceId:
      (s as { linkedTerminalInstanceId?: string | null }).linkedTerminalInstanceId ?? null,
    /* Read both the new (`agentInstanceId`) and legacy (`columnInstanceId`)
       fields so sessions saved before the v8 column-rename rehydrate
       cleanly. The legacy fallback can be dropped after a migration
       window — every save path now writes the new key, so a single
       reopen rewrites the file. */
    agentInstanceId:
      (s as { agentInstanceId?: string | null }).agentInstanceId
      ?? (s as { columnInstanceId?: string | null }).columnInstanceId
      ?? null,
    cwdSwitchRecap:
      (s as { cwdSwitchRecap?: string | null }).cwdSwitchRecap ?? null,
    cwdUuids:
      (s as { cwdUuids?: Record<string, string> }).cwdUuids ?? {},
    awaitingApproval:
      Boolean((s as { awaitingApproval?: boolean }).awaitingApproval),
    pendingActionResults:
      (s as { pendingActionResults?: PendingActionResult[] }).pendingActionResults ?? [],
    /* pendingTurn from disk means the prior process died mid-turn.
       Clear the marker (so a clean reboot doesn't keep flagging) and
       set the derived `interrupted` so the UI / next-turn flow can
       offer recovery. Mid-process crash signal is one-shot — we
       don't want to keep re-triggering recovery if the user just
       leaves the chat sitting there with the banner visible. */
    pendingTurn: null,
    interrupted:
      (s as { pendingTurn?: { startedAt: number } | null }).pendingTurn != null
  };
}

/** Called from +page.svelte onMount with the resolved app-data dir.
 *  Tries to load sessions from disk; if no disk sessions exist yet,
 *  migrates whatever is in localStorage to disk, then clears localStorage. */
export async function initSessionsFromDisk(appDataDir: string): Promise<void> {
  setDiskDir(`${appDataDir}/sessions`);
  try {
    const indexPath = sessionIndexFilePath();
    if (!indexPath) throw new Error('disk dir not set');
    const exists = await invoke<boolean>('fs_path_exists', { path: indexPath });
    if (exists) {
      const raw = await invoke<string>('fs_read_file', { path: indexPath });
      const index = JSON.parse(raw) as { activeId: string | null; ids: string[] };
      // Read every session file in parallel — sequential `for ... await`
      // here used to scale boot time linearly with N sessions (one IPC
      // round-trip each). Promise.all collapses that to a single round-
      // trip's worth of wall clock. Settle-style filtering preserves the
      // skip-on-corrupt behavior: a single bad file shouldn't stop the
      // others from loading.
      const ids = index.ids ?? [];
      const settled = await Promise.all(
        ids.map(async (id): Promise<ClaudeSession | null> => {
          try {
            const filePath = sessionFilePathFor(id);
            if (!filePath) return null;
            const sessionRaw = await invoke<string>('fs_read_file', { path: filePath });
            return hydrateSession(JSON.parse(sessionRaw) as ClaudeSession);
          } catch {
            return null;
          }
        })
      );
      const sessions = settled.filter((s): s is ClaudeSession => s !== null);
      /* App-restart continuation recap.
       *
       * After Woom restarts the CLI's process pool is gone. Sometimes
       * `--resume <uuid>` rehydrates the prior conversation cleanly,
       * sometimes the CLI's session store has been auto-compacted /
       * pruned / reinstalled — in those cases the agent answers the
       * next turn as if it were a fresh chat ("В этом чате обсуждали
       * один вопрос…" pointing only at the most recent exchange).
       *
       * Cheap insurance: stamp a one-shot continuation recap on every
       * restored session that has prior turns. The next send picks it
       * up via the existing `cwdSwitchRecap` channel and clears it on
       * success, so it doesn't leak into subsequent turns. If the CLI
       * actually still remembers, the recap is harmless redundancy.
       *
       * Skipped when:
       *   - the session has no messages (nothing to remember)
       *   - the session already has a recap stamped (don't overwrite —
       *     a more specific reason was already set, e.g. cwd_switch)
       *   - the session was never sent (no claudeResumable + no msgs) */
      for (const s of sessions) {
        if (!s.messages || s.messages.length === 0) continue;
        if (s.cwdSwitchRecap) continue;
        try {
          s.cwdSwitchRecap = buildContinuationRecap(s, 'app_restart');
        } catch {
          /* recap build is best-effort — never block the restore */
        }
      }
      sessionsState.list = sessions;
      sessionsState.activeClaudeId = index.activeId ?? sessions[0]?.id ?? null;
      const active = sessions.find((s) => s.id === sessionsState.activeClaudeId);
      if (active) sessionsState.activeIds[active.agentKind] = active.id;
      /* Seed the disk module's "last written" snapshot so the first
       *  post-init scheduleDiskWrite doesn't think every session is
       *  new and redundantly rewrite all files. */
      resetLastWrittenSnapshot(sessions, sessionsState.activeClaudeId);
    } else {
      // First run with disk persistence — migrate localStorage to disk.
      await flushAllNow(sessionsState.list, sessionsState.activeClaudeId);
    }
    // localStorage is no longer the source of truth. Clear it to free quota.
    try { localStorage.removeItem(SESSIONS_STORAGE_KEY); } catch { /* ignore */ }
  } catch (e) {
    console.error('[sessions] disk init failed, falling back to localStorage:', e);
    setDiskDir(null); // Keep localStorage path active.
  }
}

export function persistSessionsEffect() {
  $effect(() => {
    if (getDiskDir()) {
      // Disk is active — debounce writes to avoid hammering disk during streaming.
      const list = sessionsState.list;
      const activeId = sessionsState.activeClaudeId;
      scheduleDiskWrite(list, activeId);
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
      if (!sessionsLocalToastFired) {
        sessionsLocalToastFired = true;
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

/** Persist `sessionsState.editorInstanceState` so each editor instance
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
 *  pattern EditorView uses on mount). The caller is responsible for
 *  ensuring `instanceId` actually points to an editor instance; if it
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
    agentInstanceId?: string | null;
  } = {}
): string {
  const id = genId();
  const agentKind = opts.agentKind ?? 'claude';
  const n = sessionsState.list.filter((s) => s.agentKind === agentKind).length + 1;
  const title = opts.title ?? `Chat ${n}`;
  const agentInstanceId = opts.agentInstanceId ?? null;
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
      // Default new Claude sessions to Opus 4.8. Anthropic dropped
      // Opus 4.8's standard rate to $5/$25 per 1M (was $15/$75 on
      // 4.7) on 2026-05-28 — now within striking distance of Sonnet
      // 4.6 ($3/$15) but with materially better reasoning. The user
      // opts down to Sonnet/Haiku per session via the model chip
      // when budget matters more than quality. NOTE: this only
      // affects NEW sessions; persisted sessions on the old default
      // keep their model — no auto-migration.
      claudeModel: agentKind === 'claude' ? 'claude-opus-4-8' : null,
      lastContextSize: 0,
      linkedToEditor: !!opts.linkedToEditor,
      linkedToEditorInstanceId: opts.linkedToEditorInstanceId ?? null,
      linkedCanvasId: null,
      linkedTerminalInstanceId: null,
      agentInstanceId,
      cwdSwitchRecap: null,
      cwdUuids: {},
      awaitingApproval: false,
      pendingActionResults: [],
      pendingTurn: null,
      // RTK output-compression is on by default for every new
      // Claude session. The composer pill (Phase 4) toggles this
      // flag; `sendClaudeMessage` forwards `!rtkEnabled` to the
      // backend as `rtkDisabled` so the spawned `claude` env gets
      // `WOOM_RTK_SESSION_DISABLED=1` and the wrapper passes Bash
      // output through unchanged.
      rtkEnabled: true,
      // Fast mode is opt-in per session. Default off; user toggles
      // via the FAST chip in Composer footer when the active model
      // is Opus 4.8-family. Persisted across reloads.
      fastMode: false
    },
    ...sessionsState.list
  ];
  sessionsState.activeClaudeId = id;
  sessionsState.activeIds[agentKind] = id;
  if (agentInstanceId) {
    sessionsState.activeByInstance[agentInstanceId] = id;
  }
  return id;
}

export function deleteClaudeSession(id: string) {
  const doomed = sessionsState.list.find((s) => s.id === id);
  /* Auto-distill: before the session disappears, snapshot the first
     user message + last assistant message into long-term memory so a
     future chat can find what this session was about via memory_search.
     Sessions with very little content (only the auto-created empty
     state) skip the distill — nothing meaningful to preserve. The
     save is fire-and-forget; deletion proceeds regardless of whether
     it succeeds (toast surfaces an error if it does fail). */
  if (doomed) {
    void autoDistillSession(doomed);
  }
  const rest = sessionsState.list.filter((s) => s.id !== id);
  sessionsState.list = rest;
  const kind = doomed?.agentKind ?? 'claude';
  // Every per-instance pointer that was on this session jumps to the next
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
  // the chat surface from sitting on a permanent empty state.
  if (rest.filter((s) => s.agentKind === 'claude').length === 0) {
    newClaudeSession({ agentKind: 'claude' });
  }
}

/** Snapshot a soon-to-be-deleted session into long-term memory. Picks
 *  the first user message (intent anchor) + the last assistant
 *  message (outcome anchor) + the cwd basename, formats as one note,
 *  and saves through `memory_save_local`. The dedup logic on the Rust
 *  side handles repeat saves cleanly — re-deleting a re-created chat
 *  with the same content updates the existing row rather than
 *  stacking duplicates.
 *
 *  Skip rules:
 *    - 0 user messages → empty placeholder session, nothing to learn.
 *    - First user message under 30 chars AND no assistant reply →
 *      probably "test" / "hi" — noise.
 *
 *  Fire-and-forget. Caller treats this as a side effect that runs in
 *  parallel with the actual list mutation. */
async function autoDistillSession(sess: ClaudeSession): Promise<void> {
  const users = sess.messages.filter((m) => m.role === 'user' && m.content.trim().length > 0);
  const assistants = sess.messages.filter(
    (m) => m.role === 'assistant' && m.content.trim().length > 0
  );
  if (users.length === 0) return;
  const firstUser = users[0].content.trim();
  const lastAssistant = assistants.length > 0
    ? assistants[assistants.length - 1].content.trim()
    : '';
  if (assistants.length === 0 && firstUser.length < 30) return;

  const cwd = sess.worktreePath || sess.cwd || '';
  const cwdBase = cwd
    ? cwd.split('/').filter((s) => s.length > 0).pop() ?? ''
    : '';
  const title = sess.title || 'Untitled chat';
  /* Keep each section bounded — full transcripts can be huge; the
     point of the distill is "enough to recall what this was", not
     "preserve the whole thing". 1200 chars per section + the wrapper
     stays under typical memory_search preview budget. */
  const trunc = (s: string, n: number): string =>
    s.length > n ? s.slice(0, n - 1) + '…' : s;
  const sections: string[] = [
    `Chat "${title}" archived${cwdBase ? ` (project: ${cwdBase})` : ''}.`,
    `First user prompt: ${trunc(firstUser, 1200)}`
  ];
  if (lastAssistant) {
    sections.push(`Last agent reply: ${trunc(lastAssistant, 1200)}`);
  }
  const content = sections.join('\n\n');
  const tags: string[] = ['auto-distilled', `from-session:${sess.id.slice(0, 8)}`];
  if (cwdBase) tags.push(`project:${cwdBase}`);
  try {
    await invoke<number>('memory_save_local', {
      content,
      kind: 'note',
      tags
    });
  } catch (e) {
    /* Surface the failure but don't block deletion — the user
       explicitly asked to delete. Sticky error toast so they know
       a memory snapshot wasn't captured if they care. */
    notify({
      kind: 'error',
      title: 'Could not distill chat to memory',
      body: String(e)
    });
  }
}

/** Clean up per-instance pointers when an app instance is removed. The
 *  session list is global per-kind so nothing in `list` needs touching —
 *  only the per-instance bookkeeping. */
export function orphanSessionsForInstance(instanceId: string) {
  delete sessionsState.activeByInstance[instanceId];
  delete sessionsState.scrollEls[instanceId];
}

/** Set the active session pointer for one specific app instance. The
 *  session list is global per-kind, so this only affects which chat is
 *  shown when the user looks at this instance — it doesn't move the
 *  chat out of any other instance. */
export function setActiveSessionInInstance(instanceId: string, sessionId: string) {
  sessionsState.activeByInstance[instanceId] = sessionId;
  const sess = sessionsState.list.find((s) => s.id === sessionId);
  if (sess) {
    sessionsState.activeIds[sess.agentKind] = sessionId;
    sessionsState.activeClaudeId = sessionId;
    // Track "last shown in" for telemetry/UX-niceties only — does NOT
    // affect what's visible in other instances.
    if (sess.agentInstanceId !== instanceId) {
      updateSession(sessionId, { agentInstanceId: instanceId });
    }
  }
}

export function updateSession(id: string, patch: Partial<ClaudeSession>) {
  // Drain any queued stream deltas before the patch so `sending: false`
  // (and similar one-shot toggles) can't land before the tail of the
  // stream's last few tokens. No-op when the queue is empty.
  flushStreamQueueNow();
  /* Index-based mutation — see the streaming flush handler at the top
   * of this file. Replacing `.list = .list.map(...)` with `.list[i] =
   * ...` so Svelte's fine-grained reactivity only dirties subscribers
   * of THIS session, not every component that reads `sessionsState.list`.
   * Matters under typical usage where 5-10 chats are open + visible
   * sidebar lists subscribe to the whole array. */
  const list = sessionsState.list;
  for (let i = 0; i < list.length; i++) {
    if (list[i].id === id) {
      list[i] = { ...list[i], ...patch };
      return;
    }
  }
}

/** Quota guard (Phase 2): mark a session as paused with a countdown
 *  to the relevant bucket's reset. Thin wrapper over `updateSession`
 *  so call-sites stay readable + a future `quota:reset` event-emit
 *  can hook here. */
export function setAwaitingResume(
  id: string,
  resetAt: number,
  reason: 'quota' | 'user' | 'crash'
) {
  updateSession(id, {
    awaitingResume: true,
    resumeAt: resetAt,
    interruptedReason: reason,
    sending: false
  });
}

/** Clear quota-pause state — used by the watchdog when the bucket
 *  drops under threshold and by the ResumePill after the user
 *  manually resumes. */
export function clearResumeState(id: string) {
  updateSession(id, {
    awaitingResume: false,
    resumeAt: undefined,
    interruptedReason: undefined
  });
}

/** Stamp `pendingTurn` on the session as a crash-detection marker.
 *  Call right before `runAgentRequest` invokes the CLI; pair with
 *  `markTurnEnd` in the matching `finally`. If the app dies between
 *  these two calls the marker survives to the next boot.
 *
 *  Synchronously triggers a disk write so the marker lands even if
 *  the user force-quits within the debounce window — the whole point
 *  of the marker is that it survives crashes that lose other in-flight
 *  state, so we can't rely on the regular debounced flush. */
export function markTurnStart(id: string) {
  const s = sessionsState.list.find((x) => x.id === id);
  if (!s) return;
  /* Index of the just-appended user message. Caller convention is
   * `appendSessionMessage(...)` immediately before `runAgentRequest`,
   * so messages.length - 1 is the prompt being answered. If no user
   * messages exist yet (defensive), record 0 — the recap path falls
   * back to "no specific message" framing. */
  const idx = Math.max(0, s.messages.length - 1);
  // Fine-grained per-index mutation — see `updateSession` for the
  // reactive-cost rationale.
  const list = sessionsState.list;
  for (let i = 0; i < list.length; i++) {
    if (list[i].id === id) {
      list[i] = { ...list[i], pendingTurn: { startedAt: Date.now(), userMessageIndex: idx } };
      break;
    }
  }
  void flushSessionsNow();
}

/** Clear `pendingTurn` once a turn has reached a terminal state. Safe
 *  to call when no marker is set (no-op). Does NOT clear `interrupted`
 *  — that flag is a UI signal owned by the recovery flow. */
export function markTurnEnd(id: string) {
  const s = sessionsState.list.find((x) => x.id === id);
  if (!s || s.pendingTurn == null) return;
  // Fine-grained per-index mutation — see `updateSession`.
  const list = sessionsState.list;
  for (let i = 0; i < list.length; i++) {
    if (list[i].id === id) {
      list[i] = { ...list[i], pendingTurn: null };
      break;
    }
  }
}

/** Acknowledge the interrupted-session banner without taking recovery
 *  action. Clears the derived flag; pendingTurn was already cleared on
 *  hydrate, so there's no on-disk state to touch. */
export function dismissInterrupted(id: string) {
  const s = sessionsState.list.find((x) => x.id === id);
  if (!s || !s.interrupted) return;
  // Fine-grained per-index mutation — see `updateSession`.
  const list = sessionsState.list;
  for (let i = 0; i < list.length; i++) {
    if (list[i].id === id) {
      list[i] = { ...list[i], interrupted: false };
      break;
    }
  }
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

/** Attach a captured terminal-selection block to the session's composer.
 *  Used by the terminal's "Apply to <agent>" floating chip — same role
 *  as `attachLineRangeMention` for the editor, but the @-token resolves
 *  to a chunk of literal shell output rather than a file range.
 *
 *  Token shape:
 *    `terminal/<safe-label>:<short-hash>`
 *    e.g. `terminal/Hopper:8f3a` — the slash makes the prefix unmistakable
 *    in chat, and `<short-hash>` derives from the captured text so two
 *    different selections from the same terminal land as two distinct
 *    chips instead of clobbering each other.
 *
 *  Storage:
 *    • `source: 'terminal'`, `body: <selected text>`, `title: <label> · "<preview…>"`.
 *    • The prompt-builder in +page.svelte's send path already has a
 *      generic non-`file` branch (`@<externalId> — <title>\n\n<body>`),
 *      so the agent sees the actual selected bytes inline without a
 *      "Referenced file" prefix that would make no sense for a paste.
 *
 *  Returns the token (without `@`) so the caller can chain a toast.
 *  Returns null if the session doesn't exist or the selection is empty. */
export function attachTerminalSelectionMention(
  sessionId: string,
  terminalLabel: string,
  content: string
): string | null {
  const s = sessionsState.list.find((x) => x.id === sessionId);
  if (!s) return null;
  const trimmed = content.replace(/\r\n/g, '\n').trim();
  if (!trimmed) return null;

  /* Slugify the human-friendly label ("Hopper", "Build watcher") into
     something safe for an @-token (no spaces, parsed as one word by
     the textarea's `@[^\s]+` highlight regex). */
  const safeLabel =
    terminalLabel
      .replace(/[^A-Za-z0-9_-]+/g, '-')
      .replace(/^-+|-+$/g, '')
      .slice(0, 40) || 'terminal';

  /* Tiny FNV-1a-ish hash over the trimmed body — not for security, just
     to give two different selections distinct externalIds so re-applying
     a different range doesn't collapse onto the previous chip. */
  let h = 2166136261;
  for (let i = 0; i < trimmed.length; i++) {
    h ^= trimmed.charCodeAt(i);
    h = (h * 16777619) >>> 0;
  }
  const shortHash = h.toString(16).padStart(8, '0').slice(0, 6);
  const token = `terminal/${safeLabel}:${shortHash}`;

  /* Compact preview for the chip title — first non-empty line, trimmed
     to ~28 chars. Keeps the @-pill scannable without dumping full
     output into a tooltip. */
  const firstLine = trimmed.split('\n').find((l) => l.trim().length > 0) ?? trimmed;
  const preview = firstLine.length > 28 ? firstLine.slice(0, 27) + '…' : firstLine;

  const existing = new Set(s.mentions.map((m) => m.externalId));
  let nextInput = s.input;
  let nextMentions = s.mentions;
  if (!existing.has(token)) {
    const sep = nextInput && !nextInput.endsWith(' ') ? ' ' : '';
    nextInput = nextInput + sep + '@' + token + ' ';
    nextMentions = [
      ...s.mentions,
      {
        source: 'terminal',
        externalId: token,
        title: `${terminalLabel} · "${preview}"`,
        body: trimmed,
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
    /* External attachments (OS drop / paste) — and all image mentions
       — are managed by the explicit chip strip, not by the inline
       `@token` text. Keep them regardless of what's in the textarea. */
    if (mention.attached) return true;
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
    session. Called from the composer's + button and from the OS drag-drop
    listener (+page.svelte). Skips paths already referenced.

    Two attachment shapes depending on the file kind:
      • Image (png/jpg/…): mention only — no `@token` in input. Renders as a
        thumbnail chip strip above the composer (see Composer). Path can
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
    fresh.push({ source: 'file', externalId: token, title: name, body: p, isDir: false, attached: true });
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
    per-app-instance pointer (if bound), and the cross-instance "currently
    focused" pointer. Legacy code paths that read the active session
    (sendClaudeMessage, pickCwd, worktree ops, …) keep working because
    `activeClaudeId` still points at whatever was last clicked. */
export function focusSession(id: string) {
  const sess = sessionsState.list.find((s) => s.id === id);
  if (!sess) return;
  sessionsState.activeIds[sess.agentKind] = id;
  sessionsState.activeClaudeId = id;
  if (sess.agentInstanceId) {
    sessionsState.activeByInstance[sess.agentInstanceId] = id;
  }
}

/** Sessions that should render in a given app instance. The chat list is
 *  **global per agent-kind** — every Claude instance sees every Claude
 *  chat, every Cursor instance sees every Cursor chat. The instance is
 *  just a viewing window with its own "currently open" pointer;
 *  `agentInstanceId` on a session is informational ("last shown here"). */
export function sessionsForInstance(
  _instanceId: string,
  kind: 'claude' | 'cursor'
): ClaudeSession[] {
  return sessionsState.list.filter((s) => s.agentKind === kind);
}

/** Return the session active in a given app instance. Each instance owns
 *  its own active pointer so two instances can show different chats; the
 *  list itself is shared. Falls back to the most-recent session of the
 *  same kind when the instance hasn't picked one yet (fresh instance =
 *  shows newest chat instead of an empty pane). */
export function activeSessionInInstance(
  instanceId: string,
  kind: 'claude' | 'cursor'
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

// ---- Pending action results queue ----
//
// Action cards (commit / PR / bash / switch_cwd) resolve asynchronously
// from the agent's turn, sometimes mid-stream. Their outcomes need to
// reach two consumers:
//
//   1. UI: an action-result chip in the chat transcript, parseable by
//      `parseActionResult` in the chat surface. This MUST NOT happen while
//      a turn is streaming — `appendSessionMessage` would shift the
//      "last message" position and silently drop subsequent assistant
//      deltas, cutting off the agent's reply mid-sentence.
//
//   2. Agent: a "since-last-turn outcomes" block prepended to the next
//      `runAgentRequest` prompt. The CLI's `--resume` history doesn't
//      include these (they're Woom annotations); inline prepend
//      is the only channel.
//
// The queue lets us decouple WHEN we record from WHEN each consumer
// reads. Action executors enqueue immediately on resolve. UI flush
// happens lazily when the session is quiescent (deferred until turn
// ends if streaming). Agent drain happens at the head of the next
// runAgentRequest call, manual or auto-fired.

/** Push an outcome onto the queue. If the session isn't currently
 *  streaming, the chip is flushed to the UI right away — same UX as
 *  the old direct-append path. Mid-stream resolves defer until the
 *  caller invokes `flushActionResultsToUI` (typically from the
 *  send-finally block). */
export function enqueuePendingActionResult(
  sessionId: string,
  result: { ok: boolean; kind: 'commit' | 'pr' | 'bash' | 'switch_cwd'; summary: string }
) {
  const sess = sessionsState.list.find((s) => s.id === sessionId);
  if (!sess) return;
  const entry: PendingActionResult = {
    ok: result.ok,
    kind: result.kind,
    summary: result.summary,
    at: new Date().toISOString(),
    flushedToUI: false
  };
  // Append to the queue first; the immediate-flush path below mirrors
  // it into messages[] and flips `flushedToUI` so the deferred-flush
  // path (turn-end) is a no-op for this entry.
  updateSession(sessionId, {
    pendingActionResults: [...sess.pendingActionResults, entry]
  });
  if (!sess.sending) {
    flushActionResultsToUI(sessionId);
  }
}

/** Append every queued result that hasn't yet hit the UI as a chat
 *  message. Safe to call repeatedly — items with `flushedToUI=true`
 *  are skipped. Caller invokes after a streaming turn ends so the
 *  chip lands in chronological order without breaking deltas. */
export function flushActionResultsToUI(sessionId: string) {
  const sess = sessionsState.list.find((s) => s.id === sessionId);
  if (!sess) return;
  const toFlush = sess.pendingActionResults.filter((r) => !r.flushedToUI);
  if (toFlush.length === 0) return;
  // One pass: build the new pendingActionResults (with flags flipped)
  // and append messages. Doing this in two state updates would mean
  // two reactive ticks and a brief window where the chip is appended
  // but the queue still flags it as unflushed.
  const updated = sess.pendingActionResults.map((r) =>
    r.flushedToUI ? r : { ...r, flushedToUI: true }
  );
  const newMessages = [
    ...sess.messages,
    ...toFlush.map((r) => ({
      role: 'system' as const,
      content: `[Woom action result] ${r.ok ? '✓' : '✗'} ${r.summary}`,
      at: r.at
    }))
  ];
  sessionsState.list = sessionsState.list.map((s) =>
    s.id === sessionId
      ? { ...s, messages: newMessages, pendingActionResults: updated }
      : s
  );
}

/** Take and clear the queue. Returns the items so the caller can
 *  build a prompt prefix from them. Called at the head of the next
 *  `runAgentRequest` (manual send or auto-continueAgentTurn) — the
 *  agent sees these results exactly once. UI-side flushing is
 *  independent: `flushedToUI=true` items have already been shown as
 *  chips and stay in `messages` regardless of this drain. */
export function drainPendingActionResultsForAgent(
  sessionId: string
): PendingActionResult[] {
  const sess = sessionsState.list.find((s) => s.id === sessionId);
  if (!sess || sess.pendingActionResults.length === 0) return [];
  const drained = sess.pendingActionResults;
  updateSession(sessionId, { pendingActionResults: [] });
  return drained;
}

/** Render the drained queue as the agent-facing prompt prefix. Empty
 *  string when there's nothing to report — caller can `${prefix}${prompt}`
 *  unconditionally. Format is deliberately structured (markdown bullet
 *  list with kind + ✓/✗ marker) so the agent can parse it without
 *  prose ambiguity. */
export function formatActionResultsForPrompt(results: PendingActionResult[]): string {
  if (results.length === 0) return '';
  const lines = ['[Woom: action card outcomes since your last turn]'];
  for (const r of results) {
    const marker = r.ok ? '✓' : '✗';
    // Indent the multi-line summary so it groups under the bullet
    // visually. Trim to keep prompt size bounded — full output is on
    // the chip in the UI when the user wants to inspect.
    const indented = r.summary
      .split('\n')
      .map((l) => `  ${l}`)
      .join('\n');
    lines.push(`- ${marker} ${r.kind}:\n${indented}`);
  }
  lines.push('');
  lines.push('Continue from where you were.');
  return lines.join('\n');
}

/** Swap the agent CLI for a session. Rotates `claudeUuid` and resets the
    resumable flag because each CLI keeps its own session store — resuming
    a Claude id against cursor-agent (or vice versa) would fail. Also
    drops the per-cwd uuid map: those ids are CLI-specific (a saved
    claudeUuid wouldn't resume in cursor-agent), so carrying the map
    across a CLI swap would only mislead future cwd switches. The UI
    history in Woom is retained but neither CLI will remember
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
  // Hot-path during streaming — enqueue into the rAF-coalesced batch
  // instead of doing a full sessionsState.list.map per token. The
  // queued op mirrors into both `content` (legacy concat) and
  // `events` (ordered array) on flush, identical to the previous
  // synchronous behavior.
  enqueueStream(sessionId, { kind: 'text', delta });
}

/** Mirror of `appendToLastAssistant` for `thinking` content blocks.
    Concatenates onto the last assistant message's `thinking` field
    (lazily initialised). The pill in the chat surface collapses these
    into a "Thinking ✓" button the user can expand to inspect after
    the final answer lands. Batched via the same rAF queue as text. */
export function appendToLastThinking(sessionId: string, delta: string) {
  enqueueStream(sessionId, { kind: 'thinking', delta });
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
  // Drain any queued text/thinking deltas so the trace event lands
  // AFTER them in `events[]` — preserves the text → trace → text
  // chronology in the rendered chat.
  flushStreamQueueNow();
  // Wrap in ‹toolcall›…‹/toolcall› markers so AssistantContent can
  // render the call (and any later-attached output, see
  // attachOutputToLastTrace) as a single unified card with INPUT
  // and OUTPUT sections — instead of two separate visual blocks
  // (a plain command block + a free-floating output card).
  const wrapped = `‹toolcall›\n${segment}\n‹/toolcall›`;
  sessionsState.list = sessionsState.list.map((s) => {
    if (s.id !== sessionId) return s;
    const msgs = [...s.messages];
    const last = msgs[msgs.length - 1];
    if (last && last.role === 'assistant') {
      const prev = last.trace ?? '';
      const nextTrace = prev ? `${prev}\n\n${wrapped}` : wrapped;
      const events = [...(last.events ?? [])];
      const lastEv = events[events.length - 1];
      if (lastEv && lastEv.kind === 'trace') {
        events[events.length - 1] = { kind: 'trace', segments: [...lastEv.segments, wrapped] };
      } else {
        events.push({ kind: 'trace', segments: [wrapped] });
      }
      msgs[msgs.length - 1] = { ...last, trace: nextTrace, events };
    }
    return { ...s, messages: msgs };
  });
}

/** Attach a tool's output to the LAST segment of the LAST trace event
    on the last assistant message. Visually pairs the tool call with
    its result inside the same "✓ N steps" pill — when the user expands
    the pill they see `<cmd>` and below it a collapsible output card,
    grouped as one logical unit instead of the call appearing in the
    pill and the output floating below as a separate text bubble.

    The output is wrapped in `‹output›…‹/output›` markers so the
    AssistantContent renderer can fold it into a collapsible card.
    Markers also keep recap parsing unambiguous if we ever want to
    extract them programmatically.

    No-op when there's no trace event yet (e.g. a tool_result fires
    for a non-trace tool like Edit, whose result lands on the diff
    card directly). */
export function attachOutputToLastTrace(sessionId: string, output: string) {
  const trimmed = output.trim();
  if (!trimmed) return;
  // Ensure any queued text deltas are flushed first so the splice
  // operates on the up-to-date trace segments.
  flushStreamQueueNow();
  // Cap to 1500 chars per result — file dumps and tree outputs would
  // otherwise blow the trace pill out of proportion. The agent's
  // CLI session has the full text anyway.
  const capped = trimmed.length > 1500 ? `${trimmed.slice(0, 1499)}…` : trimmed;
  // We splice the output INSIDE the last `‹toolcall›…‹/toolcall›`
  // wrapper of the last segment — right before its closing marker.
  // This way the call + its output stay co-located inside one
  // toolcall block, which AssistantContent renders as a single
  // bordered card with two sections (INPUT / OUTPUT).
  const closeMarker = '‹/toolcall›';
  const outputBlock = `\n‹output›\n${capped}\n‹/output›`;
  const splice = (s: string) => {
    const idx = s.lastIndexOf(closeMarker);
    if (idx < 0) {
      // Fallback for any stray legacy segment: just append the
      // output block at the end. Renders as a free-floating card,
      // same as the old behavior — better than dropping the data.
      return `${s}${outputBlock}`;
    }
    return `${s.slice(0, idx)}${outputBlock}\n${s.slice(idx)}`;
  };
  sessionsState.list = sessionsState.list.map((s) => {
    if (s.id !== sessionId) return s;
    const msgs = [...s.messages];
    const last = msgs[msgs.length - 1];
    if (!last || last.role !== 'assistant') return s;
    const events = [...(last.events ?? [])];
    let lastTraceIdx = -1;
    for (let i = events.length - 1; i >= 0; i--) {
      if (events[i].kind === 'trace') {
        lastTraceIdx = i;
        break;
      }
    }
    if (lastTraceIdx < 0) return s;
    const ev = events[lastTraceIdx];
    if (ev.kind !== 'trace') return s;
    const segs = [...ev.segments];
    if (segs.length === 0) return s;
    segs[segs.length - 1] = splice(segs[segs.length - 1]);
    events[lastTraceIdx] = { kind: 'trace', segments: segs };
    msgs[msgs.length - 1] = { ...last, trace: splice(last.trace ?? ''), events };
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
 *  last hop" — drives the context-window % indicator.
 *
 *  Cumulative-usage guard: some Claude CLI versions emit the final
 *  `result` event with a `usage` block that sums across every
 *  internal tool step in the turn, not just the final step. That
 *  inflates `contextSize` past what's physically possible for a
 *  single API call — we've seen 2M reported on a 200K-window Sonnet
 *  turn — and jumps the UI's context-window % indicator to 100% in
 *  one frame. A single API call CAN'T have more input tokens than
 *  the model's context window, so anything larger is the cumulative
 *  artifact and gets ignored: the prior assistant event already gave
 *  us the correct final-step snapshot. The `+ 8K` slack is for cache-
 *  creation chunks that nominally fit the window but get rounded up
 *  by tokenizer differences. */
export function updateLastAssistantUsage(sessionId: string, usage: ClaudeUsage) {
  // Stamp lands on whatever message the last queued delta belongs to,
  // not the one before it.
  flushStreamQueueNow();
  sessionsState.list = sessionsState.list.map((s) => {
    if (s.id !== sessionId) return s;
    const cap = contextWindowFor(
      usage.model ?? (s.agentKind === 'claude' ? s.claudeModel : s.cursorModel),
      s.agentKind
    );
    const looksCumulative = usage.contextSize > cap + 8_192;
    if (looksCumulative) {
      // Drop the inflated usage entirely — keep whatever the last
      // legitimate assistant event left on the message + session.
      return s;
    }
    const msgs = [...s.messages];
    const last = msgs[msgs.length - 1];
    if (last && last.role === 'assistant') {
      /* Stamp `fastMode` from the session onto the usage snapshot.
       * Claude CLI's per-step `usage` payload never carries Fast
       * mode — it's a billing-side property, not a token-counter
       * one. We copy it here so `costForUsage` reads the correct
       * RATE_TABLE row (`<model>:fast` vs `<model>`) without having
       * to look up the parent session at cost-calc time. Mid-session
       * Fast toggles therefore cost-stamp PER turn, not retroactively. */
      const stamped: ClaudeUsage = { ...usage, fastMode: s.fastMode === true };
      msgs[msgs.length - 1] = { ...last, usage: stamped };
    }
    return { ...s, messages: msgs, lastContextSize: usage.contextSize };
  });
}

export function replaceLastAssistant(sessionId: string, content: string) {
  // We're about to OVERWRITE the last assistant message — drop any
  // pending stream deltas for this session first so they don't
  // re-apply on top of the replacement on the next frame. Other
  // sessions' queued deltas are unaffected.
  dropStreamQueueFor(sessionId);
  flushStreamQueueNow();
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
  // Same ordering concern as appendToLastTrace — the edit card should
  // land after any pending stream deltas, not before them.
  flushStreamQueueNow();
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
  // Direct mutation instead of `list.map(...)`. We used to rebuild
  // the entire sessions array on EVERY keystroke, which invalidated
  // every $derived that reads `sessionsState.list` (and there are
  // dozens — active session, sidebar, badge counters, …) and
  // produced noticeable typing lag plus caret drift, because Svelte
  // then re-applied `textarea.value` on the re-render. The deep
  // $state proxy reacts to a single field change on a single item
  // without disturbing other derived state.
  const sess = sessionsState.list.find((s) => s.id === sessionId);
  if (sess) sess.input = value;
}
