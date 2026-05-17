// Headless Claude / Cursor invocation. Wraps the `claude_ask` Tauri
// command + the `claude:stream:<sessionId>` event subscription, dispatching
// each parsed line through `handleStreamEvent`.
//
// Pure exec layer: no DOM, no scroll, no toasts. The caller (chat column /
// component) decides what to do with errors and the final reply, and
// supplies an `onAssistantDelta` callback if it wants the streaming text
// to appear in the UI.

import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { handleStreamEvent, resetTurnDispatcherState } from '$lib/stream/agentStream';
import { markTurnStart, markTurnEnd } from '$lib/state/sessions.svelte';

export interface AgentRunRequest {
  sessionId: string;
  prompt: string;
  cwd: string | null;
  /** Stable session UUID. For Claude this round-trips back; for Cursor
   *  the CLI may mint a new chat id on first turn — always read the
   *  returned `session_uuid` from the result. */
  claudeUuid: string;
  resume: boolean;
  /** User-authored rules from the Rules tab. Appended via
   *  `--append-system-prompt` so they apply on every turn. */
  rules: string | null;
  agentKind: 'claude' | 'cursor';
  cursorModel: string | null;
  /** Model id forwarded to `claude --model`. Null = no flag passed (CLI
   *  picks its default). */
  claudeModel: string | null;
  /** Per-turn UI context: a description of the active solo, sibling
   *  instances + names + cwds, and which instance the calling session is
   *  bound to. Lets the agent address specific columns by name (e.g.
   *  "switch the editor Sagrada-Familia") and know what already exists
   *  so it doesn't blindly add a NEW column when the user said "switch".
   *  Built fresh on every turn. */
  appContext: string | null;
  /** Absolute paths of image attachments. For Claude they get base64-
   *  embedded as `image` content blocks via the CLI's stream-json input;
   *  for Cursor (no equivalent flag) the backend ignores this and the
   *  caller should fall back to the path-mention flow. */
  imagePaths?: string[];
  /** Called with every assistant-text delta as it streams in. */
  onAssistantDelta: (sessionId: string, delta: string) => void;
  /** Called with `thinking` deltas from reasoning models. Optional —
   *  when omitted the stream handler falls back to its default
   *  (`appendToLastThinking` on the session). */
  onThinkingDelta?: (sessionId: string, delta: string) => void;
  /** Called once per tool-use trace segment. Optional — defaults to
   *  `appendToLastTrace` which feeds the "✓ N steps" pill. */
  onTraceDelta?: (sessionId: string, segment: string) => void;
  /** Called when the agent invokes a `mcp__app__*` UI-navigation tool.
   *  Threaded through to the stream handler — see
   *  `AgentStreamHandlers.onAppNavigation`. Optional. */
  onAppNavigation?: (
    sessionId: string,
    name: string,
    input: Record<string, unknown>
  ) => void;
}

export interface AgentRunResult {
  reply: string;
  /** Effective session uuid as returned by the backend. May differ from the
   *  one we sent for Cursor. */
  sessionUuid: string;
}

/** Run one turn against the agent. Subscribes to `claude:stream:<id>`,
 *  dispatches events through `handleStreamEvent`, then awaits the final
 *  reply. The subscription is always cleaned up — even on throw. */
export async function runAgentRequest(req: AgentRunRequest): Promise<AgentRunResult> {
  // Wipe any leftover per-turn dispatcher flags before this turn
  // starts. Normally cleared by the previous turn's `result` event;
  // turns that errored out without emitting `result` (CLI crash,
  // forced kill, malformed final line) would otherwise leak their
  // flags to here and skew this turn's usage stamping.
  resetTurnDispatcherState(req.sessionId);
  /* Stamp pendingTurn so that if the app dies before this function
   * returns (force-quit mid-stream, OS crash), the next boot's
   * hydrateSession can flag the session as interrupted. The Rust
   * side's stop() already cleans up its own process state; this is
   * the JS-side counterpart that survives JS-process death. */
  markTurnStart(req.sessionId);
  let unlisten: UnlistenFn | null = null;
  try {
    unlisten = await listen<string>(`claude:stream:${req.sessionId}`, (event) => {
      try {
        const parsed = JSON.parse(event.payload);
        handleStreamEvent(req.sessionId, parsed, {
          onAssistantDelta: req.onAssistantDelta,
          onThinkingDelta: req.onThinkingDelta,
          onTraceDelta: req.onTraceDelta,
          onAppNavigation: req.onAppNavigation
        });
      } catch {
        // Malformed line — drop it. The CLI sometimes interleaves a stray
        // log line; we'd rather skip than crash the stream.
      }
    });
  } catch {
    // Couldn't subscribe — proceed without streaming, the final reply
    // still comes back via the invoke result below.
  }

  try {
    const result = await invoke<{ reply: string; session_uuid: string }>('claude_ask', {
      sessionId: req.sessionId,
      prompt: req.prompt,
      cwd: req.cwd,
      claudeUuid: req.claudeUuid,
      resume: req.resume,
      rules: req.rules,
      agentKind: req.agentKind,
      cursorModel: req.cursorModel,
      claudeModel: req.claudeModel,
      appContext: req.appContext,
      imagePaths: req.imagePaths ?? []
    });
    return { reply: result.reply, sessionUuid: result.session_uuid };
  } finally {
    unlisten?.();
    /* Clear pendingTurn whether the call succeeded, errored, or
     * threw via resume-orphan etc. — the marker is meant to survive
     * only abrupt process death, not normal control flow exits.
     * Failure path callers still get the error; they handle recovery
     * via the existing resume-orphan / cwd-recap channels. */
    markTurnEnd(req.sessionId);
  }
}

/** Stop the running agent process for `sessionId`. No-op if nothing's
 *  running — the backend swallows that case. Errors are surfaced so the
 *  caller can toast them. */
export async function stopAgentRequest(sessionId: string): Promise<void> {
  await invoke('claude_stop', { sessionId });
}

/** Args needed to prewarm a Claude CLI for a session. Same shape as
 *  the spawn-relevant subset of `AgentRunRequest` — they have to match
 *  for the backend's pool-match check to find the prewarmed process
 *  on the next `runAgentRequest`. */
export interface PrewarmRequest {
  sessionId: string;
  cwd: string | null;
  claudeUuid: string;
  resume: boolean;
  rules: string | null;
  agentKind: 'claude' | 'cursor';
  claudeModel: string | null;
  appContext: string | null;
}

/** Pre-spawn a Claude CLI for `sessionId` so the cold-start cost
 *  (binary load + `--resume` history hydration) overlaps with the
 *  user typing their prompt. Idempotent for matching args (cheap to
 *  call on every keystroke / focus event). No-op for cursor sessions —
 *  cursor-agent takes its prompt as a positional CLI arg, so the
 *  backend has nothing to spawn until the user hits Send. */
export async function prewarmAgent(req: PrewarmRequest): Promise<void> {
  try {
    await invoke('claude_prewarm', {
      sessionId: req.sessionId,
      cwd: req.cwd,
      claudeUuid: req.claudeUuid,
      resume: req.resume,
      rules: req.rules,
      agentKind: req.agentKind,
      claudeModel: req.claudeModel,
      appContext: req.appContext
    });
  } catch {
    // Prewarm is purely an optimization — failing to spawn (binary
    // missing, auth lost) is fine; the actual `runAgentRequest` will
    // surface the real error to the user.
  }
}

/** Drop any prewarmed CLI parked for `sessionId`. Called on tab
 *  switch / cwd change / chat deletion / blur-after-empty-input —
 *  anywhere the parked process can no longer match the next ask. */
export async function dropPrewarm(sessionId: string): Promise<void> {
  try {
    await invoke('claude_drop_prewarm', { sessionId });
  } catch {
    // Best-effort cleanup; if the call fails, the TTL sweeper will
    // pick up the abandoned process within ~30s.
  }
}

/** Stable wire-format prefix the Rust side stamps on `claude_ask`
 *  errors when the CLI couldn't find the resume target uuid in its
 *  on-disk store. Frontend uses this to distinguish "session was
 *  pruned, recover by minting a new uuid + injecting recap" from
 *  ordinary CLI failures (auth, network, model error, etc.). */
export const RESUME_ORPHAN_PREFIX = 'RESUME_ORPHAN: ';

export function isResumeOrphanError(err: unknown): boolean {
  const msg = typeof err === 'string' ? err : err instanceof Error ? err.message : String(err);
  return msg.startsWith(RESUME_ORPHAN_PREFIX);
}
