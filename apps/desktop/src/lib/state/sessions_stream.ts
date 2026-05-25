// Streaming-delta batch queue — extracted from sessions.svelte.ts in
// wave-1 phase-7 refactor. Owns the rAF-coalesced enqueue + flush
// pipeline and the per-session op applier; knows nothing about the
// reactive `sessionsState` store. The host (sessions.svelte.ts)
// registers a flush handler at module init that walks
// `sessionsState.list` and calls `applyOpsToSession` for every dirty
// id — keeping that mutation inside the store file means this module
// stays a pure utility (no circular import with the $state shell).
//
// `appendToLastAssistant` / `appendToLastThinking` get called once
// per token by the agent CLI's streaming pipeline (~50/s during a
// hot reply, times N parallel sessions). Each call used to do a
// full `sessionsState.list.map(...)` rebuild + spread of the last
// message's `events` array — O(sessions × messages) per token. On
// a long chat (300 msgs) with 4 sessions open this was the
// dominant cost during streaming and made the chat itself feel
// laggy. Coalescing into one flush per animation frame collapses
// 50 text tokens to a single list.map call.
//
// Any other mutator on `sessionsState.list` (`appendToLastTrace`,
// `attachOutputToLastTrace`, `replaceLastAssistant`,
// `updateLastAssistantUsage`, `flushSessionsNow`, …) MUST call
// `flushStreamQueueNow()` first so queued deltas land BEFORE the
// direct mutation — preserving causal order in the event log
// (text → trace → text shows up that way, not all-text-then-trace).

import type { ClaudeSession } from '$lib/types';

export type StreamOp =
  | { kind: 'text'; delta: string }
  | { kind: 'thinking'; delta: string };

const _streamQueue = new Map<string, StreamOp[]>();
let _streamRaf: number | null = null;
let _flushHandler: (() => void) | null = null;

/** Host hook — called by `sessions.svelte.ts` at module init.
 *  The handler is responsible for draining the queue (via
 *  `drainStreamQueue`) and writing back to whatever store owns
 *  the session list. */
export function setStreamFlushHandler(fn: () => void): void {
  _flushHandler = fn;
}

/** Enqueue a single text / thinking delta for the given session.
 *  Consecutive same-kind ops merge so 50 text tokens collapse to
 *  one op with a long delta — the flush walks the events array
 *  once per session instead of once per token. */
export function enqueueStream(sessionId: string, op: StreamOp): void {
  let ops = _streamQueue.get(sessionId);
  if (!ops) {
    ops = [];
    _streamQueue.set(sessionId, ops);
  }
  const last = ops[ops.length - 1];
  if (last && last.kind === op.kind) {
    last.delta += op.delta;
  } else {
    ops.push(op);
  }
  _scheduleStreamFlush();
}

function _scheduleStreamFlush(): void {
  if (_streamRaf !== null) return;
  if (typeof requestAnimationFrame === 'undefined') {
    queueMicrotask(_runFlush);
    return;
  }
  _streamRaf = requestAnimationFrame(() => {
    _streamRaf = null;
    _runFlush();
  });
}

function _runFlush(): void {
  if (_flushHandler) _flushHandler();
}

/** Snapshot the per-session ops then clear the live queue. Called
 *  inside the host's flush handler so re-entrant enqueues during
 *  the list.map (e.g. a Svelte effect kicked off by the rebuild)
 *  land in the NEXT frame's batch instead of being replayed in
 *  this pass. */
export function drainStreamQueue(): Map<string, StreamOp[]> {
  const work = new Map(_streamQueue);
  _streamQueue.clear();
  return work;
}

/** Drop any pending ops for a single session. Used by
 *  `replaceLastAssistant` to prevent queued deltas from re-applying
 *  on top of a replacement message after the synchronous flush. */
export function dropStreamQueueFor(sessionId: string): void {
  _streamQueue.delete(sessionId);
}

/** Apply a batch of ops to a session's LAST assistant message.
 *  Pure — never mutates `s`; returns a fresh ClaudeSession when
 *  ops applied, the input ref unchanged when ops are empty or
 *  the tail isn't an assistant. */
export function applyOpsToSession(
  s: ClaudeSession,
  ops: StreamOp[] | undefined
): ClaudeSession {
  if (!ops || ops.length === 0) return s;
  const msgs = [...s.messages];
  const last = msgs[msgs.length - 1];
  if (!last || last.role !== 'assistant') return s;
  let content = last.content;
  let thinking = last.thinking;
  const events = [...(last.events ?? [])];
  for (const op of ops) {
    if (op.kind === 'text') {
      content += op.delta;
      const lastEv = events[events.length - 1];
      if (lastEv && lastEv.kind === 'text') {
        events[events.length - 1] = { kind: 'text', body: lastEv.body + op.delta };
      } else {
        events.push({ kind: 'text', body: op.delta });
      }
    } else {
      thinking = (thinking ?? '') + op.delta;
    }
  }
  msgs[msgs.length - 1] = { ...last, content, thinking, events };
  return { ...s, messages: msgs };
}

/** Drain any queued streaming deltas synchronously. Called at the
 *  top of every direct mutator on `sessionsState.list` so trace
 *  pills, action cards, edit cards, replaceLastAssistant, …
 *  always see the latest text/thinking state instead of stomping
 *  over a stale snapshot that the rAF batch hadn't applied yet.
 *  Also called by `flushSessionsNow` so the on-disk copy is
 *  current before the app quits. */
export function flushStreamQueueNow(): void {
  if (_streamRaf !== null) {
    if (typeof cancelAnimationFrame === 'function') cancelAnimationFrame(_streamRaf);
    _streamRaf = null;
  }
  _runFlush();
}
