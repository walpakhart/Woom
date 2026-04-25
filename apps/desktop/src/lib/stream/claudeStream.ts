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

import { appendToLastAssistant, appendToLastThinking, appendToLastTrace, addAction, genId } from '$lib/state/sessions.svelte';
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
  /** Called with one tool-use trace segment per call (already formatted
   *  via `formatToolUse`). The default handler appends to the message's
   *  `trace` field; AgentColumn collapses the trace into a "✓ N steps"
   *  pill above the answer body so the chat doesn't drown in
   *  read/edit/bash hints. Routed separately from `onAssistantDelta`
   *  so the assistant's actual reply text stays clean. */
  onTraceDelta?: (sessionId: string, segment: string) => void;
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
  },
  onTraceDelta(sessionId, segment) {
    appendToLastTrace(sessionId, segment);
  }
};

/** Dispatch a single parsed stream event for `sessionId`. Caller-supplied
 *  handlers are merged on top of `defaultStreamHandlers`, so a caller
 *  that only overrides `onAssistantDelta` (typical: chat column adds a
 *  scroll-on-append) still gets the default `onThinkingDelta` (writes
 *  to the session's `thinking` field).
 *
 *  Important: `{...defaults, ...handlers}` with `handlers.onThinkingDelta
 *  = undefined` would overwrite the default with undefined and silently
 *  drop thinking + trace blocks (the classic JS spread-undefined bug —
 *  bit us once already when callers passed an object literal with
 *  `onThinkingDelta: req.onThinkingDelta` and req didn't supply it).
 *  Build the override map from defined values only. */
export function handleStreamEvent(
  sessionId: string,
  parsed: unknown,
  handlers: ClaudeStreamHandlers = defaultStreamHandlers
): void {
  let merged: ClaudeStreamHandlers;
  if (handlers === defaultStreamHandlers) {
    merged = defaultStreamHandlers;
  } else {
    const overrides: Partial<ClaudeStreamHandlers> = {};
    if (handlers.onAssistantDelta) overrides.onAssistantDelta = handlers.onAssistantDelta;
    if (handlers.onThinkingDelta) overrides.onThinkingDelta = handlers.onThinkingDelta;
    if (handlers.onTraceDelta) overrides.onTraceDelta = handlers.onTraceDelta;
    if (handlers.onAppNavigation) overrides.onAppNavigation = handlers.onAppNavigation;
    merged = { ...defaultStreamHandlers, ...overrides };
  }
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
    if (block.type === 'redacted_thinking') {
      // Thinking-models occasionally produce blocks the API redacts
      // (signed/encrypted, can't be displayed). Surface a placeholder
      // so the pill expansion still tells the user *something* was
      // there — without it the thinking pill might show only partial
      // content and feel buggy.
      merged.onThinkingDelta?.(sessionId, '\n\n[redacted thinking — content not available]\n\n');
      continue;
    }
    if (block.type !== 'tool_use') {
      // Anything we don't recognize (image, server_tool_use, future
      // block types) — log once per unknown shape so future drops
      // don't go unnoticed. Kept silent in production-build console
      // (warn level only fires in DevTools).
      if (typeof block.type === 'string') {
        console.warn('[claudeStream] dropped unknown content block:', block.type, block);
      }
      continue;
    }
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
          if (hint) merged.onTraceDelta?.(sessionId, hint);
          continue;
        }
        const formatted = formatToolUse(name, input);
        if (formatted) merged.onTraceDelta?.(sessionId, formatted);
      }
    }
  }
}
