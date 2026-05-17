/* Loops — `/loop <duration> <prompt>` schedules a recurring user
 * message in the active session (Claude Code parity §6). On each
 * fire we drop the prompt into `pendingQueue` so the existing turn-
 * end drain picks it up as if the user had typed it.
 *
 * Scope choices (MVP):
 *   - Fixed cadence only. Self-paced (agent picks next interval) is
 *     deferred — needs a tool the agent calls back into.
 *   - Per-session. One loop per session id; starting a new loop
 *     replaces any existing one (with a console hint).
 *   - In-memory. Loops don't survive an app restart. Worth persisting
 *     once the surface matures; not worth it for the MVP.
 *   - 7-day hard cap (Claude Code parity). Auto-stops at expiry.
 *   - Tick granularity: 5 s. Shorter intervals are clamped up to 5 s
 *     to avoid burning CPU on a tight loop. */

import type { ClaudeSession } from '$lib/types';
import { appendSessionMessage, sessionsState, updateSession } from '$lib/state/sessions.svelte';

export interface LoopTask {
  sessionId: string;
  prompt: string;
  intervalMs: number;
  /** Wall-clock ms — next fire time. Compared every tick. */
  nextFireAt: number;
  /** Wall-clock ms — when the loop auto-stops. Default = now + 7d. */
  expiresAt: number;
  /** Total fires so far — surfaces in the UI indicator. */
  fires: number;
  /** Pause flag — `/loop pause <session>` flips this without dropping
   *  the task. UI toggles it via a button. */
  paused: boolean;
}

const MIN_INTERVAL_MS = 5_000;
const SEVEN_DAYS_MS = 7 * 24 * 3600 * 1000;
const TICK_MS = 5_000;

export const loopsState = $state<{
  /** One loop per session. Map preserves insertion order which is
   *  nice for the future "all loops" panel; we read by sessionId. */
  bySession: Record<string, LoopTask>;
}>({ bySession: {} });

/** Public — start (or replace) a loop on a session. Returns the
 *  task or null if the duration / prompt is invalid. */
export function startLoop(
  sessionId: string,
  intervalMs: number,
  prompt: string
): LoopTask | null {
  const interval = Math.max(MIN_INTERVAL_MS, Math.floor(intervalMs));
  const trimmed = prompt.trim();
  if (trimmed.length === 0) return null;
  const now = Date.now();
  const task: LoopTask = {
    sessionId,
    prompt: trimmed,
    intervalMs: interval,
    nextFireAt: now + interval,
    expiresAt: now + SEVEN_DAYS_MS,
    fires: 0,
    paused: false
  };
  loopsState.bySession[sessionId] = task;
  ensureTicking();
  return task;
}

export function stopLoop(sessionId: string): boolean {
  if (loopsState.bySession[sessionId]) {
    delete loopsState.bySession[sessionId];
    return true;
  }
  return false;
}

export function pauseLoop(sessionId: string, paused: boolean): void {
  const t = loopsState.bySession[sessionId];
  if (t) t.paused = paused;
}

export function getLoop(sessionId: string): LoopTask | null {
  return loopsState.bySession[sessionId] ?? null;
}

/** Parse a duration string like `5m`, `30s`, `1h`, `2h30m`. Returns
 *  null for malformed input. Whole-number coefficients only. */
export function parseDuration(s: string): number | null {
  const trimmed = s.trim().toLowerCase();
  if (!trimmed) return null;
  /* Simple multi-token parser: tokens of `<digits><unit>` where
   *  unit ∈ {ms,s,m,h,d}. `5m` = 300_000 ms. */
  const re = /(\d+)(ms|s|m|h|d)/g;
  let total = 0;
  let matched = 0;
  let m: RegExpExecArray | null;
  while ((m = re.exec(trimmed)) !== null) {
    const n = Number(m[1]);
    if (!Number.isFinite(n)) return null;
    matched += m[0].length;
    switch (m[2]) {
      case 'ms': total += n; break;
      case 's': total += n * 1000; break;
      case 'm': total += n * 60_000; break;
      case 'h': total += n * 3_600_000; break;
      case 'd': total += n * 86_400_000; break;
    }
  }
  /* Reject extra garbage characters — if the regex didn't consume
   *  every char of `trimmed`, the user typed something we don't
   *  recognise and we should fail loud. */
  if (matched === 0 || matched < trimmed.replace(/\s+/g, '').length) return null;
  return total;
}

// ---- Scheduler -----------------------------------------------------------

let tickHandle: ReturnType<typeof setInterval> | null = null;

function ensureTicking(): void {
  if (tickHandle !== null) return;
  if (typeof window === 'undefined') return;
  tickHandle = setInterval(tick, TICK_MS);
}

function tick(): void {
  const now = Date.now();
  const due: LoopTask[] = [];
  for (const t of Object.values(loopsState.bySession)) {
    if (now >= t.expiresAt) {
      // Auto-stop expired loops.
      delete loopsState.bySession[t.sessionId];
      continue;
    }
    if (t.paused) continue;
    if (now >= t.nextFireAt) due.push(t);
  }
  for (const t of due) {
    fireLoop(t);
  }
}

function fireLoop(t: LoopTask): void {
  const sess = sessionsState.list.find((s) => s.id === t.sessionId);
  if (!sess) {
    // Session was deleted — drop the loop.
    delete loopsState.bySession[t.sessionId];
    return;
  }
  t.fires += 1;
  t.nextFireAt = Date.now() + t.intervalMs;
  /* Push into the session's pendingQueue (existing field — drained
   *  at end-of-turn or fired immediately if the session is idle).
   *  Append-only; the user can still type something else into the
   *  composer and it'll go ahead of the queued loop tick. */
  enqueueLoopPrompt(sess, t.prompt);
}

function enqueueLoopPrompt(sess: ClaudeSession, prompt: string): void {
  const queue = [...(sess.pendingQueue ?? []), { text: prompt, mentions: [] }];
  updateSession(sess.id, { pendingQueue: queue });
  /* If the session is idle right now, trigger the existing send
   *  pipeline by writing the prompt to `input` and dispatching a
   *  custom event the +page.svelte layer listens for. We avoid
   *  importing sendClaudeMessage here to dodge a circular dep. */
  if (!sess.sending && (sess.pendingQueue?.length ?? 0) === 0) {
    /* The queue update above already added the item — read back the
     *  latest session state and pop the head. */
    const fresh = sessionsState.list.find((s) => s.id === sess.id);
    const head = fresh?.pendingQueue?.[0];
    if (head && fresh) {
      updateSession(sess.id, {
        input: head.text,
        mentions: head.mentions,
        pendingQueue: fresh.pendingQueue?.slice(1) ?? []
      });
      try {
        window.dispatchEvent(new CustomEvent('woom:loop-fire', {
          detail: { sessionId: sess.id }
        }));
      } catch { /* noop */ }
    }
  }

  /* Surface a tiny system-style note so the user sees the loop
   *  fired. We append rather than overwriting the conversation. */
  appendSessionMessage(sess.id, {
    role: 'assistant',
    content: `_/loop fire #${(loopsState.bySession[sess.id]?.fires ?? 0)} — re-running: "${prompt.length > 80 ? prompt.slice(0, 77) + '…' : prompt}"_`,
    at: new Date().toISOString()
  });
}
