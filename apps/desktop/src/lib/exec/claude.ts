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
import { handleStreamEvent } from '$lib/stream/claudeStream';

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
  /** Per-turn UI context: a description of the active workbench, sibling
   *  instances + names + cwds, and which instance the calling session is
   *  bound to. Lets the agent address specific columns by name (e.g.
   *  "switch the editor Sagrada-Familia") and know what already exists
   *  so it doesn't blindly add a NEW column when the user said "switch".
   *  Built fresh on every turn. */
  appContext: string | null;
  /** Called with every assistant-text delta as it streams in. */
  onAssistantDelta: (sessionId: string, delta: string) => void;
  /** Called when the agent invokes a `mcp__app__*` UI-navigation tool.
   *  Threaded through to the stream handler — see
   *  `ClaudeStreamHandlers.onAppNavigation`. Optional. */
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
  let unlisten: UnlistenFn | null = null;
  try {
    unlisten = await listen<string>(`claude:stream:${req.sessionId}`, (event) => {
      try {
        const parsed = JSON.parse(event.payload);
        handleStreamEvent(req.sessionId, parsed, {
          onAssistantDelta: req.onAssistantDelta,
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
      appContext: req.appContext
    });
    return { reply: result.reply, sessionUuid: result.session_uuid };
  } finally {
    unlisten?.();
  }
}

/** Stop the running agent process for `sessionId`. No-op if nothing's
 *  running — the backend swallows that case. Errors are surfaced so the
 *  caller can toast them. */
export async function stopAgentRequest(sessionId: string): Promise<void> {
  await invoke('claude_stop', { sessionId });
}
