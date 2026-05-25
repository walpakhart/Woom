// SDD per-phase action-log store + agent-stream listener. Extracted
// from sdd.svelte.ts in wave-10 split. Owns:
//
//   - The in-memory ring buffer (`sddState.actionLogByWorkspace`)
//     keyed by (workspaceId, phase). 100-row cap per phase.
//   - Tool-event listener wired via `subscribeToolEvent` — converts
//     stream tool_use / tool_result events into ActionLogEntry rows,
//     correlates result back to its matching `running` use entry,
//     and persists each row to the per-phase JSONL via debounced
//     `sdd_append_action_log_batch` Tauri call (250 ms flush window).
//   - Rehydrate-from-disk path for workspaces left mid-phase on app
//     restart, so the SddCard live feed isn't empty after boot.
//
// Frontend re-exports `actionLogFor`, `appendSddActionLog`,
// `getSddPhaseForSession`, and the listener attach + rehydrate
// entries from sdd.svelte; this module is the implementation.

import { invoke } from '@tauri-apps/api/core';

import { formatToolUse } from '$lib/format';
import { subscribeToolEvent, type ToolStreamEvent } from '$lib/stream/agentStream';

import { sddState, workspaceForSession, type ActionLogEntry, type SddPhaseSubstep } from './sdd.svelte';

/* Per-phase live-buffer cap. Each phase keeps the last N tool rows
 * hot for SddCard's inline feed; older entries fall off but stay on
 * disk (the JSONL is the source of truth — buffer is just the
 * window the UI renders without an IPC round-trip). 100 = roughly
 * one "complex phase with lots of edits" worth of activity. */
export const ACTION_LOG_CAP = 100;

/** Resolve the (workspace, phase) live-feed should target for a session.
 *  Returns null when the session isn't bound to an SDD workspace OR the
 *  workspace isn't in `phase_running` (we deliberately drop tool events
 *  outside running phases — feeding a `phase_done` log would cross-pollute
 *  the next phase when it kicks off). */
export function getSddPhaseForSession(
  sessionId: string,
): { workspaceId: string; phase: number; subStep: SddPhaseSubstep | null } | null {
  const ws = workspaceForSession(sessionId);
  if (!ws) return null;
  const s = ws.stage;
  switch (s.kind) {
    case 'phase_running':
      return { workspaceId: ws.id, phase: s.phase, subStep: null };
    case 'phase_planning':
      return { workspaceId: ws.id, phase: s.phase, subStep: 'plan' };
    case 'phase_implementing':
      return { workspaceId: ws.id, phase: s.phase, subStep: 'implement' };
    case 'phase_verifying':
      return { workspaceId: ws.id, phase: s.phase, subStep: 'verify' };
    default:
      return null;
  }
}

/** Read-only view: returns the current in-memory buffer for the given
 *  (workspace, phase). Empty when nothing has been logged yet. */
export function actionLogFor(workspaceId: string, phase: number): ActionLogEntry[] {
  return sddState.actionLogByWorkspace[workspaceId]?.[phase] ?? [];
}

/** Append a single entry to the in-memory ring buffer + queue it for
 *  disk flush. Exposed so the SDD orchestrator itself can log
 *  `sdd_event` rows (phase started, verifier ran, …) — the agent
 *  stream listener uses the internal `pushActionEntry` directly. */
export function appendSddActionLog(workspaceId: string, entry: ActionLogEntry): void {
  pushActionEntry(workspaceId, entry);
  queueAppend(workspaceId, entry);
}

function pushActionEntry(workspaceId: string, entry: ActionLogEntry): void {
  const byWs = sddState.actionLogByWorkspace[workspaceId] ?? {};
  const prev = byWs[entry.phase] ?? [];
  /* Correlate tool_result back to its tool_use by replacing the running
   *  row instead of stacking a second one. The stable correlation_id
   *  (CLI's tool_use_id) lets us flip a `running` row to `done` /
   *  `failed` without losing the original summary. Falls back to a
   *  plain push when the result has no correlation_id (legacy events
   *  or a result for a dropped tool_use). */
  let next: ActionLogEntry[];
  if (entry.kind === 'tool_result' && entry.correlation_id) {
    const idx = prev.findIndex(
      (e) => e.kind === 'tool_use' && e.correlation_id === entry.correlation_id,
    );
    if (idx !== -1) {
      next = prev.slice();
      next[idx] = {
        ...prev[idx],
        ts: entry.ts,
        status: entry.status ?? prev[idx].status,
        detail: entry.detail ?? prev[idx].detail,
      };
    } else {
      next = [...prev, entry];
    }
  } else {
    next = [...prev, entry];
  }
  if (next.length > ACTION_LOG_CAP) {
    next = next.slice(next.length - ACTION_LOG_CAP);
  }
  sddState.actionLogByWorkspace = {
    ...sddState.actionLogByWorkspace,
    [workspaceId]: { ...byWs, [entry.phase]: next },
  };
}

// Debounced batch flush: 250ms after the last queued event, ship all
// pending entries to disk in one Tauri call. Tracks pending per-workspace
// so two parallel SDDs don't share a flush window.
const pendingFlush: Record<string, ActionLogEntry[]> = {};
const flushTimer: Record<string, ReturnType<typeof setTimeout>> = {};
const FLUSH_DELAY_MS = 250;

function queueAppend(workspaceId: string, entry: ActionLogEntry): void {
  const buf = (pendingFlush[workspaceId] ??= []);
  buf.push(entry);
  if (flushTimer[workspaceId]) clearTimeout(flushTimer[workspaceId]);
  flushTimer[workspaceId] = setTimeout(() => flushPendingAppends(workspaceId), FLUSH_DELAY_MS);
}

async function flushPendingAppends(workspaceId: string): Promise<void> {
  const entries = pendingFlush[workspaceId];
  if (!entries || entries.length === 0) return;
  delete pendingFlush[workspaceId];
  delete flushTimer[workspaceId];
  try {
    await invoke('sdd_append_action_log_batch', { id: workspaceId, entries });
  } catch (e) {
    console.warn('[sdd] action_log flush failed', e);
  }
}

/** Build the human ≤80-char summary for the inline pill. Reuses
 *  `formatToolUse` from the chat-trace path so the wording matches. */
function summariseTool(toolName: string, input: Record<string, unknown>): string {
  const formatted = formatToolUse(toolName, input) ?? '';
  const stripped = formatted
    .replace(/^>\s*/, '')
    .replace(/\*([^*]+)\*/g, '$1')
    .replace(/`([^`]+)`/g, '$1')
    .trim();
  const fallback = `${toolName}`;
  const text = stripped || fallback;
  return text.length > 80 ? text.slice(0, 77) + '…' : text;
}

/** Compact a tool's raw input into a `detail` string for the lightbox.
 *  Just JSON.stringify with reasonable truncation. */
function detailFromInput(input: Record<string, unknown>): string | undefined {
  try {
    const s = JSON.stringify(input);
    if (!s || s === '{}') return undefined;
    return s.length > 4096 ? s.slice(0, 4093) + '…' : s;
  } catch {
    return undefined;
  }
}

let toolListenerAttached = false;

/** Subscribe the tool-stream listener so every agent tool_use /
 *  tool_result inside a running SDD phase gets logged. Idempotent —
 *  safe to call from `initSdd` more than once. */
export function attachActionLogListener(): void {
  if (toolListenerAttached) return;
  toolListenerAttached = true;
  subscribeToolEvent((evt: ToolStreamEvent) => {
    const target = getSddPhaseForSession(evt.sessionId);
    if (!target) return;
    const { workspaceId, phase, subStep } = target;
    if (evt.kind === 'tool_use') {
      const entry: ActionLogEntry = {
        ts: Date.now(),
        phase,
        kind: 'tool_use',
        tool: evt.toolName,
        summary: summariseTool(evt.toolName, evt.input ?? {}),
        detail: detailFromInput(evt.input ?? {}),
        status: 'running',
        correlation_id: evt.toolUseId || undefined,
        sub_step: subStep ?? undefined,
      };
      pushActionEntry(workspaceId, entry);
      queueAppend(workspaceId, entry);
      return;
    }
    const status = evt.isError ? 'failed' : 'done';
    const entry: ActionLogEntry = {
      ts: Date.now(),
      phase,
      kind: 'tool_result',
      tool: evt.toolName || undefined,
      summary: evt.toolName ? `${evt.toolName} ${status}` : status,
      status,
      correlation_id: evt.toolUseId || undefined,
      sub_step: subStep ?? undefined,
    };
    pushActionEntry(workspaceId, entry);
    queueAppend(workspaceId, entry);
  });
}

/** Pull the persisted JSONL for any workspace currently in
 *  `phase_running` and seed the in-memory buffer. Idempotent — if
 *  the buffer is already populated (e.g. the listener captured live
 *  events first) we skip the disk read. Called from `initSdd` after
 *  hydrate. */
export async function rehydrateActionLogs(): Promise<void> {
  for (const ws of sddState.workspaces) {
    const s = ws.stage;
    if (
      s.kind !== 'phase_running' &&
      s.kind !== 'phase_planning' &&
      s.kind !== 'phase_implementing' &&
      s.kind !== 'phase_verifying'
    ) {
      continue;
    }
    const phase = s.phase;
    if ((sddState.actionLogByWorkspace[ws.id]?.[phase]?.length ?? 0) > 0) continue;
    try {
      const entries = await invoke<ActionLogEntry[]>('sdd_read_action_log', {
        id: ws.id,
        phase,
        tail: ACTION_LOG_CAP,
      });
      if (entries.length > 0) {
        sddState.actionLogByWorkspace = {
          ...sddState.actionLogByWorkspace,
          [ws.id]: {
            ...(sddState.actionLogByWorkspace[ws.id] ?? {}),
            [phase]: entries,
          },
        };
      }
    } catch (e) {
      console.warn('[sdd] action_log rehydrate failed', ws.id, e);
    }
  }
}
