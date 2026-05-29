// Pure session-serialization helpers — extracted from sessions.svelte.ts
// in wave-1 phase-7 refactor. Everything here is a free function with
// no reactive store dependency, no Tauri invoke, no localStorage. The
// disk-persistence layer (`flushToDisk`) imports `serializeSession`;
// the hydrate path imports `capMessageEvents` to heal oversize events
// on load. Keeping these pure makes them trivial to unit-test (input
// → output, no environment) and removes ~110 LoC of mechanical
// boilerplate from the god file.

import type { ClaudeMessage, ClaudeSession, MessageEvent } from '$lib/types';

/* Per-event byte cap. Tool-use traces (especially Cursor's `edit` events
 * with full `oldText`/`newText` payloads) used to balloon to 240KB+ each;
 * a long agent run accumulated 100MB+ across the session list and froze
 * the app at boot (JSON.parse + Svelte reactivity choked on the deep
 * tree). 64KB is plenty for a sane Edit diff — anything bigger is a
 * file-dump that's not useful to keep in the chat transcript. */
export const EVENT_BYTE_CAP = 64 * 1024;

/** Cheap size estimator for a MessageEvent — sums string-length of the
 *  fields that dominate JSON size, no allocation. Skips the per-event
 *  JSON.stringify the old check ran on every hydrate — that was the
 *  dominant cost when loading a 4 MB session at boot. */
export function estimateEventSize(e: MessageEvent): number {
  switch (e.kind) {
    case 'text':
      return e.body?.length ?? 0;
    case 'trace': {
      let n = 0;
      for (const s of e.segments) n += s.length;
      return n;
    }
    case 'edit':
      return (e.oldText?.length ?? 0) + (e.newText?.length ?? 0);
    default:
      return 0;
  }
}

/** Replace events whose payload exceeds `EVENT_BYTE_CAP` with a small
 *  trace stub. Applied symmetrically on serialize (so disk stays lean)
 *  and on hydrate (so existing oversize files self-heal at boot).
 *  Pure — never mutates the input; returns the input reference unchanged
 *  when no events were touched (cheap fast-path for the common case). */
export function capMessageEvents(messages: ClaudeMessage[]): ClaudeMessage[] {
  let touchedAny = false;
  const next = messages.map((m) => {
    if (m.role !== 'assistant' || !m.events || m.events.length === 0) return m;
    let touched = false;
    const evs: MessageEvent[] = m.events.map((e) => {
      const size = estimateEventSize(e);
      if (size <= EVENT_BYTE_CAP) return e;
      touched = true;
      const hint = e.kind === 'edit' ? ` path=${e.filePath}` : '';
      return {
        kind: 'trace',
        segments: [
          `‹toolcall›\n[event truncated: kind=${e.kind}${hint}, ${Math.round(size / 1024)}KB > ${EVENT_BYTE_CAP / 1024}KB cap]\n‹/toolcall›`,
        ],
      };
    });
    if (!touched) return m;
    touchedAny = true;
    return { ...m, events: evs };
  });
  return touchedAny ? next : messages;
}

/** Convert a runtime `ClaudeSession` (reactive Proxy via Svelte $state)
 *  into a plain object suitable for JSON.stringify → disk. Drops the
 *  ephemeral `sending` flag (recomputed at runtime) and runs
 *  `capMessageEvents` so oversize tool traces never make it to disk. */
export function serializeSession(s: ClaudeSession): object {
  return {
    id: s.id,
    title: s.title,
    mentions: s.mentions,
    messages: capMessageEvents(s.messages),
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
    lastContextSize: s.lastContextSize,
    linkedToEditor: s.linkedToEditor,
    linkedToEditorInstanceId: s.linkedToEditorInstanceId,
    linkedCanvasId: s.linkedCanvasId,
    linkedTerminalInstanceId: s.linkedTerminalInstanceId,
    agentInstanceId: s.agentInstanceId,
    cwdSwitchRecap: s.cwdSwitchRecap,
    cwdUuids: s.cwdUuids,
    awaitingApproval: s.awaitingApproval,
    pendingActionResults: s.pendingActionResults,
    pendingTurn: s.pendingTurn ?? null,
    // Persist RTK toggle so a "RTK off for diagnostics" decision
    // survives a window close. Default-on semantics live in the
    // `newClaudeSession` factory + Composer pill render path —
    // serialising the explicit boolean keeps hydration deterministic.
    rtkEnabled: s.rtkEnabled ?? true,
    /* Fast mode default-off — RATE_TABLE keeps standard rates for
     * any persisted session without the field. Explicit boolean
     * keeps hydration deterministic. */
    fastMode: s.fastMode ?? false,
    /* Quota-pause state — survives reload so a paused chat doesn't
     * silently lose its countdown badge across app restarts. The
     * `resumeAt` unix-ms remains valid (or already-past, which
     * ResumePill treats as «click to resume»). */
    awaitingResume: s.awaitingResume ?? false,
    resumeAt: s.resumeAt ?? null,
    interruptedReason: s.interruptedReason ?? null,
  };
}
