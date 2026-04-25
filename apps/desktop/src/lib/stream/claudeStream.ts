// Claude / Cursor streaming-event handler. The CLIs emit `--output-format
// stream-json`, which is one JSON object per line. The Tauri backend
// parses each line and re-emits via `claude:stream:<sessionId>`; we
// dispatch each one through `handleStreamEvent`.
//
// The handler turns:
//  - text blocks      → assistant message deltas
//  - propose_*  tool calls → ClaudeAction cards in the chat (commit / PR /
//                          bash / switch_cwd) so the user can approve them
//                          inline before they execute
//  - other tool_use   → an inline `> *Tool* …` line via `formatToolUse`
//
// `appendAssistantDelta` is supplied by the caller because the natural
// home for it (the chat column) wants to scroll on append. Keeping the
// scroll out of this module means the same handler can drive replays
// later (e.g. an artifact re-render) without DOM coupling.

import { appendToLastAssistant, appendToLastThinking, addAction, genId } from '$lib/state/sessions.svelte';
import { formatToolUse } from '$lib/format';

export interface ClaudeStreamHandlers {
  /** Called with raw text deltas for the assistant turn. Implementations
   *  typically forward to `appendToLastAssistant` and scroll the chat. */
  onAssistantDelta: (sessionId: string, delta: string) => void;
  /** Called with `thinking` content blocks emitted by reasoning models
   *  (Claude `*-thinking-*`, Cursor reasoning models). The default
   *  handler routes these into the assistant message's `thinking`
   *  field — AgentColumn collapses them into an expandable pill so
   *  the user can inspect the chain-of-thought after the answer
   *  lands. Without this they'd be silently dropped. */
  onThinkingDelta?: (sessionId: string, delta: string) => void;
  /** Called when Claude invokes a `mcp__app__*` tool — Forgehold-app's
   *  navigation surface (open detail pane, switch view, add editor
   *  column, surface connect modal). The caller has access to all the
   *  reactive state slices and decides what to mutate. Optional — if
   *  omitted the call is rendered like any other tool_use. */
  onAppNavigation?: (
    sessionId: string,
    name: string,
    input: Record<string, unknown>
  ) => void;
}

/** Default handler: write to the sessions store. UIs that want to also
 *  scroll the chat should pass their own. */
export const defaultStreamHandlers: ClaudeStreamHandlers = {
  onAssistantDelta(sessionId, delta) {
    appendToLastAssistant(sessionId, delta);
  },
  onThinkingDelta(sessionId, delta) {
    appendToLastThinking(sessionId, delta);
  }
};

/** Dispatch a single parsed stream event for `sessionId`. Caller-supplied
 *  handlers are merged on top of `defaultStreamHandlers`, so a caller
 *  that only overrides `onAssistantDelta` (typical: chat column adds a
 *  scroll-on-append) still gets the default `onThinkingDelta` (writes
 *  to the session's `thinking` field). */
export function handleStreamEvent(
  sessionId: string,
  parsed: unknown,
  handlers: ClaudeStreamHandlers = defaultStreamHandlers
): void {
  const merged = handlers === defaultStreamHandlers
    ? defaultStreamHandlers
    : { ...defaultStreamHandlers, ...handlers };
  if (!parsed || typeof parsed !== 'object') return;
  const msg = parsed as Record<string, unknown>;
  if (msg.type !== 'assistant') return;
  const inner = msg.message as { content?: Array<Record<string, unknown>> } | undefined;
  if (!inner?.content || !Array.isArray(inner.content)) return;

  for (const block of inner.content) {
    if (block.type === 'text' && typeof block.text === 'string') {
      merged.onAssistantDelta(sessionId, block.text);
      continue;
    }
    if (block.type === 'thinking' && typeof block.thinking === 'string') {
      merged.onThinkingDelta?.(sessionId, block.thinking);
      continue;
    }
    if (block.type !== 'tool_use') continue;
    const name = typeof block.name === 'string' ? block.name : 'tool';
    const input = (block.input ?? {}) as Record<string, unknown>;
    const id = typeof block.id === 'string' ? block.id : genId();
    // Intercept propose_* tools: they surface action cards in the chat
    // so the user can approve them before anything runs. Suppress the
    // generic tool-use line — the card carries the message.
    switch (name) {
      case 'mcp__github__propose_commit':
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
      case 'mcp__github__propose_pr':
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
      case 'mcp__github__propose_switch_cwd':
        addAction(sessionId, {
          id,
          kind: 'switch_cwd',
          path: String(input.path ?? ''),
          reason: typeof input.reason === 'string' ? input.reason : '',
          status: 'pending'
        });
        continue;
      case 'mcp__github__propose_bash':
        addAction(sessionId, {
          id,
          kind: 'bash',
          command: String(input.command ?? ''),
          reason: typeof input.reason === 'string' ? input.reason : '',
          status: 'pending'
        });
        continue;
      default: {
        // forgehold-app navigation tools — drive the UI directly. We
        // also surface a one-line "navigated to X" hint into the chat
        // so the user has a record of what happened.
        if (name.startsWith('mcp__app__')) {
          merged.onAppNavigation?.(sessionId, name, input);
          const hint = formatToolUse(name, input);
          if (hint) merged.onAssistantDelta(sessionId, `\n\n${hint}\n\n`);
          continue;
        }
        const formatted = formatToolUse(name, input);
        if (formatted) merged.onAssistantDelta(sessionId, `\n\n${formatted}\n\n`);
      }
    }
  }
}
