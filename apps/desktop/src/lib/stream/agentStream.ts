// Unified agent streaming-event handler. Handles both Claude and Cursor
// — Cursor's native event shape is normalized into Claude-style by
// `cursor.rs::normalize_event` before it reaches us, so a single switch
// here drives both agents' UI. The CLIs emit `--output-format
// stream-json` (one JSON object per line); the Tauri backend parses
// each line and re-emits via `claude:stream:<sessionId>` (channel name
// is historical — it carries normalized events for both agents); we
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

import { invoke } from '@tauri-apps/api/core';
import {
  sessionsState,
  appendToLastAssistant,
  appendToLastThinking,
  appendToLastTrace,
  attachOutputToLastTrace,
  appendEditEvent,
  updateEditEvent,
  addAction,
  updateLastAssistantUsage,
  genId
} from '$lib/state/sessions.svelte';
import { formatToolUse } from '$lib/format';
import type { ClaudeAction, ClaudeUsage } from '$lib/types';

// ---------------------------------------------------------------------------
// Tool-event pub/sub — a tiny side-channel that mirrors every `tool_use` and
// `tool_result` block coming through `handleStreamEvent`, in normalized form.
// Used by the SDD action-log store (`sdd.svelte.ts::attachActionLogListener`)
// to populate the per-phase live-activity feed without re-parsing stream
// events. Stays opt-in: subscribers only see the firehose if they explicitly
// `subscribeToolEvent(...)` — nobody else pays a perf cost.
//
// We don't reuse `AgentStreamHandlers` for this because the existing handler
// fields are coupled to chat-thread / action-card concerns; the SDD log
// wants raw "what tool fired, with what args, with what result" without the
// formatting that `formatToolUse` already applies for the trace pill.
// ---------------------------------------------------------------------------

export type ToolEventKind = 'tool_use' | 'tool_result';

export interface ToolStreamEvent {
  kind: ToolEventKind;
  /** Owning agent session id (Claude / Cursor session). */
  sessionId: string;
  /** Stable id from the CLI — same value on the matching `tool_use` and
   *  `tool_result` so subscribers can correlate. May be empty for
   *  legacy events or pre-MCP tools. */
  toolUseId: string;
  /** Tool name as the CLI emits it: "Read", "Edit", "Bash",
   *  "mcp__github__get_pr", … For tool_result, mirrors the originating
   *  tool's name when known (we resolve via the cached id→name map). */
  toolName: string;
  /** Raw input arguments for `tool_use`. Empty object on `tool_result`. */
  input?: Record<string, unknown>;
  /** Trimmed text payload of `tool_result`. Empty on `tool_use`. */
  resultText?: string;
  /** True when the originating `tool_result` block carried `is_error: true`. */
  isError?: boolean;
}

type ToolEventListener = (event: ToolStreamEvent) => void;
const toolEventListeners = new Set<ToolEventListener>();

/** Subscribe to every tool_use / tool_result event flowing through
 *  `handleStreamEvent`. Returns an unsubscribe function. Listeners that
 *  throw are caught and logged so one bad subscriber can't break the
 *  stream pipeline. */
export function subscribeToolEvent(listener: ToolEventListener): () => void {
  toolEventListeners.add(listener);
  return () => toolEventListeners.delete(listener);
}

function emitToolEvent(event: ToolStreamEvent): void {
  for (const l of toolEventListeners) {
    try {
      l(event);
    } catch (e) {
      console.warn('[agentStream] tool-event listener threw:', e);
    }
  }
}

/** Map of tool_use id → tool_name, kept so tool_result events
 *  (which only carry the id) can resolve their originating tool name
 *  for the action log. Keyed per session so two parallel sessions
 *  don't see each other's ids. Pruned to last 256 entries to keep
 *  memory bounded across long-running turns. */
const toolNameById: Map<string, Map<string, string>> = new Map();
const TOOL_NAME_CAP = 256;

/** Per-session map of Claude CLI background-task ids → output paths.
 *  Populated when we sniff a `Command running in background with ID:`
 *  line in a `tool_result`. Exposed via `bgTasksForSession` /
 *  `unwatchBgTasksForSession` so +page.svelte can drop the registry
 *  when the user moves on (types into the chat, deletes the session)
 *  — otherwise the auto-fire on `claude:bg_done` would arrive late
 *  and surprise the agent mid-other-conversation. */
const claudeBgTasksBySession: Map<string, Map<string, string>> = new Map();

const BG_ID_RE = /Command running in background with ID:\s*(\S+)/;
const BG_OUTPUT_RE = /Output is being written to:\s*(\S+)/;

/** Parse a Bash tool_result for the bg-task spawn markers and, if
 *  found, register a Tauri-side watcher. Idempotent on `task_id` —
 *  the Rust side replaces a prior watcher with the same id. */
export function maybeRegisterClaudeBgTask(sessionId: string, rawResultText: string): void {
  const idMatch = BG_ID_RE.exec(rawResultText);
  const pathMatch = BG_OUTPUT_RE.exec(rawResultText);
  if (!idMatch || !pathMatch) return;
  const taskId = idMatch[1];
  const outputPath = pathMatch[1];
  let bySession = claudeBgTasksBySession.get(sessionId);
  if (!bySession) {
    bySession = new Map();
    claudeBgTasksBySession.set(sessionId, bySession);
  }
  bySession.set(taskId, outputPath);
  void invoke('claude_bg_watch', { sessionId, taskId, outputPath }).catch((e) => {
    console.warn('[claude_bg] watch failed', taskId, e);
  });
}

/** Drop the watch for a specific task id. Called from +page.svelte's
 *  `claude:bg_done` listener after it fires the silent continuation,
 *  so a second mtime-stale tick (Rust-side races) doesn't double-fire. */
export function forgetBgTask(sessionId: string, taskId: string): void {
  const bySession = claudeBgTasksBySession.get(sessionId);
  if (!bySession) return;
  bySession.delete(taskId);
  if (bySession.size === 0) claudeBgTasksBySession.delete(sessionId);
  void invoke('claude_bg_unwatch', { taskId }).catch(() => { /* noop */ });
}

/** Bulk-cancel every active watch for a session. Use when the user
 *  deletes the session or starts typing a fresh message into a chat
 *  that still has live bg tasks (manual interaction = "I'm driving,
 *  don't surprise me with a silent auto-reply"). */
export function unwatchBgTasksForSession(sessionId: string): void {
  const bySession = claudeBgTasksBySession.get(sessionId);
  if (!bySession) return;
  for (const taskId of bySession.keys()) {
    void invoke('claude_bg_unwatch', { taskId }).catch(() => { /* noop */ });
  }
  claudeBgTasksBySession.delete(sessionId);
}

function rememberToolName(sessionId: string, toolUseId: string, toolName: string) {
  if (!toolUseId) return;
  let inner = toolNameById.get(sessionId);
  if (!inner) {
    inner = new Map();
    toolNameById.set(sessionId, inner);
  }
  inner.set(toolUseId, toolName);
  if (inner.size > TOOL_NAME_CAP) {
    // Drop the oldest entry — Map preserves insertion order.
    const firstKey = inner.keys().next().value;
    if (firstKey !== undefined) inner.delete(firstKey);
  }
}

function lookupToolName(sessionId: string, toolUseId: string): string {
  return toolNameById.get(sessionId)?.get(toolUseId) ?? '';
}

export interface AgentStreamHandlers {
  /** Called with raw text deltas for the assistant turn. Implementations
   *  typically forward to `appendToLastAssistant` and scroll the chat. */
  onAssistantDelta: (sessionId: string, delta: string) => void;
  /** Called with `thinking` content blocks emitted by reasoning models
   *  (Claude `*-thinking-*`, Cursor reasoning models). The default
   *  handler routes these into the assistant message's `thinking`
   *  field — AgentApp collapses them into an expandable pill so
   *  the user can inspect the chain-of-thought after the answer
   *  lands. Without this they'd be silently dropped. */
  onThinkingDelta?: (sessionId: string, delta: string) => void;
  /** Called with one tool-use trace segment per call (already formatted
   *  via `formatToolUse`). The default handler appends to the message's
   *  `trace` field; AgentApp collapses the trace into a "✓ N steps"
   *  pill above the answer body so the chat doesn't drown in
   *  read/edit/bash hints. Routed separately from `onAssistantDelta`
   *  so the assistant's actual reply text stays clean. */
  onTraceDelta?: (sessionId: string, segment: string) => void;
  /** Called when Claude invokes a `mcp__app__*` tool — Woom-app's
   *  navigation surface (open detail pane, switch view, add editor
   *  column, surface connect modal). The caller has access to all the
   *  reactive state slices and decides what to mutate. Optional — if
   *  omitted the call is rendered like any other tool_use. */
  onAppNavigation?: (
    sessionId: string,
    name: string,
    input: Record<string, unknown>
  ) => void;
  /** Called once per assistant API call with the `usage` block from
   *  stream-json. Multi-step turns produce several of these (one per
   *  sub-step); the default handler keeps overwriting so the latest
   *  sub-step wins — that sub-step's `cache_read` tokens reflect the
   *  full prior conversation, which is the cheapest informative
   *  single-number summary for the per-message badge. Optional. */
  onUsage?: (sessionId: string, usage: ClaudeUsage) => void;
}

/** Per-session "step usage seen this turn" flag. Set when an
 *  `assistant` event with a usage block arrives; read on the
 *  terminating `result` event to decide whether the result's usage
 *  is a redundant cumulative artifact (skip) or the only signal
 *  (use). Cleared on the result event so the next turn starts
 *  fresh. Module-scoped Map because the dispatcher is otherwise
 *  stateless per-line, and tracking this on the session model
 *  itself would balloon the persisted shape for ephemeral state.
 *  Untracked sessions (no entry) are treated as not-yet-seen. */
const perTurnStepUsageSeen = new Map<string, boolean>();

/** Reset the per-turn dispatcher flags for a session. Call at the
 *  start of every new agent turn (manual send / auto-continuation /
 *  orphan retry) so a previous turn's lingering state can't poison
 *  the new one. The result-event handler also clears these on a
 *  clean turn-end, but turns that error out without emitting a
 *  result event (CLI crash, JSON parse failure on the result line,
 *  forced kill mid-stream) leave flags set — calling this at turn
 *  start guarantees every turn starts from a known-clean baseline. */
export function resetTurnDispatcherState(sessionId: string): void {
  perTurnStepUsageSeen.delete(sessionId);
}

/** Default handler: write to the sessions store. UIs that want to also
 *  scroll the chat should pass their own. */
export const defaultStreamHandlers: AgentStreamHandlers = {
  onAssistantDelta(sessionId, delta) {
    appendToLastAssistant(sessionId, delta);
  },
  onThinkingDelta(sessionId, delta) {
    appendToLastThinking(sessionId, delta);
  },
  onTraceDelta(sessionId, segment) {
    appendToLastTrace(sessionId, segment);
  },
  onUsage(sessionId, usage) {
    updateLastAssistantUsage(sessionId, usage);
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
  handlers: AgentStreamHandlers = defaultStreamHandlers
): void {
  let merged: AgentStreamHandlers;
  if (handlers === defaultStreamHandlers) {
    merged = defaultStreamHandlers;
  } else {
    const overrides: Partial<AgentStreamHandlers> = {};
    if (handlers.onAssistantDelta) overrides.onAssistantDelta = handlers.onAssistantDelta;
    if (handlers.onThinkingDelta) overrides.onThinkingDelta = handlers.onThinkingDelta;
    if (handlers.onTraceDelta) overrides.onTraceDelta = handlers.onTraceDelta;
    if (handlers.onAppNavigation) overrides.onAppNavigation = handlers.onAppNavigation;
    if (handlers.onUsage) overrides.onUsage = handlers.onUsage;
    merged = { ...defaultStreamHandlers, ...overrides };
  }
  if (!parsed || typeof parsed !== 'object') return;
  const msg = parsed as Record<string, unknown>;

  // `result` events terminate every cursor-agent turn AND every
  // claude-code turn. Cursor names the fields camelCase
  // (`inputTokens`, `cacheReadTokens`, `cacheWriteTokens`,
  // `outputTokens`); Claude's CLI shape uses snake_case
  // (`input_tokens`, `cache_read_input_tokens`,
  // `cache_creation_input_tokens`, `output_tokens`) — same names it
  // uses on the per-message `assistant` events. We try both naming
  // conventions for every field so a Claude `result` event doesn't
  // stamp a phantom `↑ 0 ↓ 0` onto the bubble (which is what the
  // camelCase-only path was doing before) and a Cursor `result`
  // doesn't accidentally fall through to the assistant branch
  // below. Skip the stamp if every count came back 0 — the CLI
  // sometimes emits an empty `result.usage` envelope before the
  // real one and we don't want that wiping a real usage already
  // stamped from the assistant event right before. */
  if (msg.type === 'result') {
    // Clear the per-turn step-usage flag UNCONDITIONALLY on any
    // result event, before doing anything else. If we waited until
    // after the usage-extraction block, an empty/zero usage envelope
    // (which the CLI sometimes emits as a placeholder) would leave
    // the flag set and poison the NEXT turn's usage update — the
    // result event from turn N+1 would see `stepSeen=true` from
    // turn N and skip its own usage stamp. Clearing at the top of
    // every result handler tracks "turn ended" cleanly regardless
    // of whether the usage envelope was meaningful.
    perTurnStepUsageSeen.delete(sessionId);
  }
  if (msg.type === 'result' && msg.usage && typeof msg.usage === 'object') {
    const u = msg.usage as Record<string, unknown>;
    const inp = numField(u, 'inputTokens') || numField(u, 'input_tokens');
    const cacheRead =
      numField(u, 'cacheReadTokens') || numField(u, 'cache_read_input_tokens');
    const cacheWrite =
      numField(u, 'cacheWriteTokens') || numField(u, 'cache_creation_input_tokens');
    const out = numField(u, 'outputTokens') || numField(u, 'output_tokens');
    if (inp + out + cacheRead + cacheWrite > 0) {
      // Cumulative-usage guard. Claude CLI's `result.usage` sums input
      // tokens across every internal tool-call sub-step in a turn,
      // not just the final step's snapshot. For a turn with N tool
      // calls, that's N× inflation: we've seen 946k reported as the
      // "context size" for a chat whose actual conversation length
      // was ~150k. The earlier per-step `assistant` events already
      // carry the correct final-step usage; if we've seen any of
      // those for this session, prefer them and ignore the result
      // event's number entirely. Falls back to using `result.usage`
      // when no per-step usage was seen (cursor flow + edge cases).
      const stepSeen = perTurnStepUsageSeen.get(sessionId) === true;
      if (!stepSeen) {
        merged.onUsage?.(sessionId, {
          inputTokens: inp,
          cacheCreationTokens: cacheWrite,
          cacheReadTokens: cacheRead,
          outputTokens: out,
          contextSize: inp + cacheWrite + cacheRead,
          // Cursor doesn't surface a stable model id on the result
          // event; Claude's `result` may carry one but we already
          // pull the model from the in-stream assistant events above
          // so either way leaving null is safe — the per-message
          // badge falls back gracefully and the context-ring chip
          // uses the session's `cursorModel` / `claudeModel` field.
          model: null
        });
      }
      // Turn ended — clear the flag for the next turn so we can pick
      // up `result.usage` again if it's the only source available.
      perTurnStepUsageSeen.delete(sessionId);
    }
    return;
  }

  // User-role events with `tool_result` content blocks. Claude CLI
  // emits these AFTER each tool_use — they carry the tool's output
  // back to the model for its next reasoning step. Pre-fix the
  // stream parser bailed on every non-assistant event, so tool
  // results never landed in `sess.messages`. Result: when CLI's
  // session uuid rotated (user pressed Stop, orphan recovery),
  // the recap built from sess.messages had only agent-side text —
  // no `supabase projects list` output, no `gh api` JSON, no
  // `git log` lines. Agent then re-asked the user for facts it
  // had already discovered, on the same chat. Persisting tool
  // results inline at the end of the corresponding assistant
  // message (the one that emitted the tool_use) means:
  //   1. recap (from sess.messages) carries the data forward
  //   2. user sees what the agent saw, scrolling chat history
  //   3. on --resume the duplication is harmless (CLI's own
  //      session JSONL has the canonical copy; ours is mirror).
  if (msg.type === 'user') {
    const inner = msg.message as { content?: Array<Record<string, unknown>> } | undefined;
    if (Array.isArray(inner?.content)) {
      for (const block of inner.content) {
        if (block.type !== 'tool_result') continue;
        const raw = extractToolResultText(block);
        const toolUseId =
          typeof block.tool_use_id === 'string' ? (block.tool_use_id as string) : '';
        const isError = block.is_error === true;
        // Mirror to the tool-event channel even for empty results —
        // SDD action log uses these to flip a `running` entry to
        // `done`/`failed`. Skip only when there's literally nothing
        // we can correlate.
        if (toolUseId) {
          emitToolEvent({
            kind: 'tool_result',
            sessionId,
            toolUseId,
            toolName: lookupToolName(sessionId, toolUseId),
            resultText: raw ?? '',
            isError,
          });
        }
        if (!raw) continue;
        // Detect Claude CLI background-task spawn: the Bash tool with
        // `run_in_background:true` returns a tool_result whose text
        // includes both an ID and the output-file path. Without this
        // hook the agent's turn ends right after the spawn message
        // (next streaming event is end-of-turn) and the bg task runs
        // forever with no one to wake the agent when it finishes.
        // `claude_bg_watch` polls the output file and emits
        // `claude:bg_done` on idle; +page.svelte listens for that
        // event and fires a silent continuation prompt.
        maybeRegisterClaudeBgTask(sessionId, raw);
        // Attach to the last trace event's last segment — visually
        // pairs the command with its output inside the same "✓ N
        // steps" pill (so expanding the pill shows: command →
        // output card). The capping + ‹output› wrapping happens
        // inside attachOutputToLastTrace.
        attachOutputToLastTrace(sessionId, raw);
      }
    }
    return;
  }
  if (msg.type !== 'assistant') return;
  const inner = msg.message as {
    content?: Array<Record<string, unknown>>;
    usage?: Record<string, unknown>;
    model?: string;
  } | undefined;
  if (!inner?.content || !Array.isArray(inner.content)) return;

  // Claude shape: usage on every assistant API call (multi-step turns
  // produce several). Pull up front so even tool-only sub-steps
  // refresh the badge, and a model swap mid-session is reflected on
  // the very next reply. Skip empty (all-zero) envelopes — Claude
  // CLI occasionally emits a placeholder usage block at the start
  // of a step before the real counts land, and we don't want that
  // wiping the previous turn's stamp until we have real numbers.
  if (inner.usage && typeof inner.usage === 'object') {
    const u = inner.usage as Record<string, unknown>;
    /* Try BOTH naming conventions on every field — Claude CLI uses
     * snake_case (`input_tokens`, `cache_creation_input_tokens`,
     * `cache_read_input_tokens`, `output_tokens`) but cursor-agent
     * uses camelCase (`inputTokens`, `cacheWriteTokens`,
     * `cacheReadTokens`, `outputTokens`) on its per-step `assistant`
     * envelopes. The result-event handler upstream already does this
     * fallback; without doing it here too, cursor's per-step events
     * fell through with `inp=0/out=0/cache=0`, the contextSize stamp
     * never fired, and the context ring sat at 0% forever. */
    const inp = numField(u, 'input_tokens') || numField(u, 'inputTokens');
    const cacheCreate =
      numField(u, 'cache_creation_input_tokens') ||
      numField(u, 'cacheWriteTokens');
    const cacheRead =
      numField(u, 'cache_read_input_tokens') ||
      numField(u, 'cacheReadTokens');
    const out = numField(u, 'output_tokens') || numField(u, 'outputTokens');
    if (inp + out + cacheCreate + cacheRead > 0) {
      // Mark this turn as having received per-step usage. The result
      // event handler reads this to decide whether to fall back on
      // its own (cumulative-prone) usage block or skip it.
      perTurnStepUsageSeen.set(sessionId, true);
      merged.onUsage?.(sessionId, {
        inputTokens: inp,
        cacheCreationTokens: cacheCreate,
        cacheReadTokens: cacheRead,
        outputTokens: out,
        contextSize: inp + cacheCreate + cacheRead,
        model: typeof inner.model === 'string' ? inner.model : null
      });
    }
  }

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
        console.warn('[agentStream] dropped unknown content block:', block.type, block);
      }
      continue;
    }
    const name = typeof block.name === 'string' ? block.name : 'tool';
    const input = (block.input ?? {}) as Record<string, unknown>;
    const id = typeof block.id === 'string' ? block.id : genId();
    /* Mirror to the tool-event channel so subscribers (SDD action log)
     *  see every tool_use uniformly — including propose_*, mcp__app__*,
     *  Edit/MultiEdit/Write — in raw form, before any UI-shaping
     *  branches below mutate state or emit trace pills. */
    rememberToolName(sessionId, id, name);
    emitToolEvent({
      kind: 'tool_use',
      sessionId,
      toolUseId: id,
      toolName: name,
      input,
    });
    // Intercept propose_* tools: they surface action cards in the chat
    // so the user can approve them before anything runs. Suppress the
    // generic tool-use line — the card carries the message.
    //
    // Race with action-IPC: the sidecar's MCP tool also emits a
    // `woom:action_request` event when it makes its blocking IPC
    // call to Tauri. Whichever path arrives first creates the card;
    // the other should NOT duplicate. The IPC path fingerprints the
    // card with a `waitId`, so this stream parser checks for an
    // existing pending card matching the same params and skips
    // creation when one's already there.
    const sess = sessionsState.list.find((s) => s.id === sessionId);
    const hasMatchingPending = (
      kind: 'bash' | 'commit' | 'pr' | 'switch_cwd',
      probe: (a: ClaudeAction) => boolean
    ) =>
      sess?.actions.some((a) => a.status === 'pending' && a.kind === kind && probe(a)) ?? false;
    switch (name) {
      case 'mcp__github__propose_commit': {
        const message = String(input.message ?? '');
        if (!hasMatchingPending('commit', (a) => a.kind === 'commit' && a.message === message)) {
          addAction(sessionId, {
            id,
            kind: 'commit',
            message,
            body: typeof input.body === 'string' ? input.body : '',
            push: input.push !== false,
            note: typeof input.note === 'string' ? input.note : '',
            status: 'pending'
          });
        }
        const commitHint = formatToolUse(name, input);
        if (commitHint) merged.onTraceDelta?.(sessionId, commitHint);
        continue;
      }
      case 'mcp__github__propose_pr': {
        const title = String(input.title ?? '');
        if (!hasMatchingPending('pr', (a) => a.kind === 'pr' && a.title === title)) {
          addAction(sessionId, {
            id,
            kind: 'pr',
            title,
            body: typeof input.body === 'string' ? input.body : '',
            base: typeof input.base === 'string' ? input.base : '',
            draft: input.draft === true,
            note: typeof input.note === 'string' ? input.note : '',
            status: 'pending'
          });
        }
        const prHint = formatToolUse(name, input);
        if (prHint) merged.onTraceDelta?.(sessionId, prHint);
        continue;
      }
      case 'mcp__github__propose_switch_cwd':
      case 'mcp__app__propose_switch_cwd': {
        const path = String(input.path ?? '');
        if (!hasMatchingPending('switch_cwd', (a) => a.kind === 'switch_cwd' && a.path === path)) {
          addAction(sessionId, {
            id,
            kind: 'switch_cwd',
            path,
            reason: typeof input.reason === 'string' ? input.reason : '',
            status: 'pending'
          });
        }
        const switchHint = formatToolUse(name, input);
        if (switchHint) merged.onTraceDelta?.(sessionId, switchHint);
        continue;
      }
      case 'mcp__github__propose_bash':
      case 'mcp__app__propose_bash': {
        const command = String(input.command ?? '');
        if (!hasMatchingPending('bash', (a) => a.kind === 'bash' && a.command === command)) {
          addAction(sessionId, {
            id,
            kind: 'bash',
            command,
            reason: typeof input.reason === 'string' ? input.reason : '',
            status: 'pending'
          });
        }
        // Always emit a trace segment so attachOutputToLastTrace has
        // somewhere to attach the tool_result output after the command runs.
        const bashHint = formatToolUse(name, input);
        if (bashHint) merged.onTraceDelta?.(sessionId, bashHint);
        continue;
      }
      default: {
        // woom-app navigation tools — drive the UI directly. We
        // also surface a one-line "navigated to X" hint into the chat
        // so the user has a record of what happened.
        if (name.startsWith('mcp__app__')) {
          merged.onAppNavigation?.(sessionId, name, input);
          const hint = formatToolUse(name, input);
          if (hint) merged.onTraceDelta?.(sessionId, hint);
          continue;
        }
        // File-mutation tools: surface them as inline diff cards
        // (Cursor-style "apply / revert" UX). We REPLACE the trace
        // pill instead of duplicating: each Edit/MultiEdit/Write
        // already produces a visible card with file path + line
        // counts, and adding a "_edit foo.ts_" trace line on top
        // means every modification shows up twice — once as an
        // inline card, once buried inside "✓ N steps". The card is
        // the more useful anchor (it's expandable and revertable),
        // so we drop the trace via `continue`.
        if (name === 'Edit') {
          const fp = typeof input.file_path === 'string' ? input.file_path : '';
          const oldStr = typeof input.old_string === 'string' ? input.old_string : '';
          const newStr = typeof input.new_string === 'string' ? input.new_string : '';
          if (fp) {
            appendEditEvent(sessionId, {
              toolId: id,
              filePath: fp,
              oldText: oldStr,
              newText: newStr,
              isCreate: false
            });
          }
          // Skip the trace line — the diff card already says "edited X".
          continue;
        }
        if (name === 'MultiEdit') {
          // MultiEdit packs several `{old_string,new_string}` edits onto
          // one file. Emit one diff card per edit so each chunk gets its
          // own Keep / Revert pair — that matches Cursor's behavior and
          // keeps reverts surgical (one bad edit doesn't force the user
          // to revert the whole sequence).
          const fp = typeof input.file_path === 'string' ? input.file_path : '';
          const edits = Array.isArray(input.edits)
            ? (input.edits as Array<Record<string, unknown>>)
            : [];
          if (fp && edits.length) {
            for (let i = 0; i < edits.length; i++) {
              const e = edits[i] ?? {};
              const oldStr = typeof e.old_string === 'string' ? e.old_string : '';
              const newStr = typeof e.new_string === 'string' ? e.new_string : '';
              if (!oldStr && !newStr) continue;
              appendEditEvent(sessionId, {
                // Synthesize a stable per-edit id so updateEditEvent can
                // find the right card. `id` is the tool_use id; appending
                // the index keeps it unique within the call.
                toolId: `${id}#${i}`,
                filePath: fp,
                oldText: oldStr,
                newText: newStr,
                isCreate: false
              });
            }
          }
          continue;
        }
        if (name === 'Write') {
          // Write is a full-file overwrite. The payload carries the *new*
          // contents and (for cursor-agent edits) optionally the exact
          // pre-state via `prev_content` — see cursor.rs::extract_tool_shape.
          //
          // Two paths:
          //   1. `prev_content` arrived inline (cursor-agent supplied
          //      `beforeFullFileContent` on the completed event). Best
          //      case: we have the *exact* moment-of-edit baseline, no
          //      git round-trip needed, and we can correctly distinguish
          //      "modified" from "created" by looking at whether
          //      `prev_content` is empty.
          //   2. `prev_content` is missing (Claude's Write, or older
          //      cursor-agent builds): ship a loading placeholder, then
          //      ask Tauri for `git show HEAD:<path>` in the background.
          //      Three sub-outcomes (see backfillWriteOldText):
          //        • git_show finds it → real diff against HEAD.
          //        • file isn't tracked at HEAD but the parent is a repo
          //          → leave isCreate=true so Revert deletes (with a
          //          Rust-side guard against deleting tracked files).
          //        • path isn't in any repo → same as above; user can
          //          Revert to delete.
          //
          // Why we can't just read pre-state at tool_use time: by the
          // time the assistant block reaches us claude/cursor-agent has
          // already executed Write — disk already holds the new content.
          // The inline `prev_content` from cursor.rs and the HEAD
          // backfill are the only post-hoc baselines we can recover.
          const fp = typeof input.file_path === 'string' ? input.file_path : '';
          const content = typeof input.content === 'string' ? input.content : '';
          const inlinePrev =
            typeof input.prev_content === 'string' ? input.prev_content : '';
          if (fp) {
            // Did cursor-agent (or anyone) ship us an exact pre-state?
            // We treat any present `prev_content` field as authoritative —
            // empty string means "file genuinely didn't exist before",
            // non-empty means "this is the literal pre-edit content".
            // We can only tell those two apart from the `prev_content`
            // shape coming through Tauri (a missing `prev_content` key
            // would deserialize to undefined, but our Rust normalizer
            // always inserts the key so the value carries the signal).
            const havePrev = typeof input.prev_content === 'string';
            if (havePrev) {
              appendEditEvent(sessionId, {
                toolId: id,
                filePath: fp,
                oldText: inlinePrev,
                newText: content,
                isCreate: inlinePrev.length === 0,
                wholeFile: true,
                status: 'applied'
              });
            } else {
              appendEditEvent(sessionId, {
                toolId: id,
                filePath: fp,
                oldText: '',
                newText: content,
                // Optimistic default — backfill flips it to false if
                // git_show finds the file in HEAD. Erring on "new"
                // rather than "modified" so the early-render card
                // doesn't promise a diff that's actually missing. Rust
                // `revert_write` has a guardrail to refuse deleting
                // tracked files even when isCreate=true, so this
                // optimism can't cost the user committed content.
                isCreate: true,
                wholeFile: true,
                status: 'loading'
              });
              void backfillWriteOldText(sessionId, id, fp);
            }
          }
          continue;
        }
        if (name === 'Delete') {
          // Cursor's `deleteToolCall` — the agent removed a file. Rust
          // normalizer (cursor.rs::extract_tool_shape) already pulled
          // `prev_content` out of `result.success.prevContent` when
          // available; we ship it as `oldText` so the diff card can
          // render the deletion as a sea of red lines + offer Restore.
          //
          // Two sub-cases for missing prev_content:
          //   • cursor-agent didn't include it (older builds, or shape
          //     changed) → fall back to `git show HEAD:<file>`. Same
          //     backfill machinery as Write uses; if the file was
          //     tracked we recover the pre-deletion contents. If it
          //     wasn't tracked there's no recoverable content and the
          //     card's Restore button will create an empty file (which
          //     is at least better than the current trace-pill "no
          //     UX" state).
          //   • the empty-prev-content case (binary file the CLI
          //     refused to capture, or really an empty file) — Restore
          //     creates an empty file at the path; user can re-delete
          //     manually if that's not what they wanted.
          const fp = typeof input.file_path === 'string' ? input.file_path : '';
          const prev = typeof input.prev_content === 'string' ? input.prev_content : '';
          if (fp) {
            appendEditEvent(sessionId, {
              toolId: id,
              filePath: fp,
              oldText: prev,
              newText: '',
              isCreate: false,
              isDelete: true,
              wholeFile: true,
              // If we already have prevContent inline, the card is
              // ready immediately. Otherwise wait on the git_show
              // backfill, same as Write.
              status: prev ? 'applied' : 'loading'
            });
            if (!prev) void backfillDeleteOldText(sessionId, id, fp);
          }
          continue;
        }
        if (name === 'Bash') {
          // Claude has no dedicated Delete tool — file removals come
          // through the generic `Bash` tool with shapes like
          // `rm path`, `rm -f a b`, `unlink path`, etc. We sniff the
          // command for those simple shapes and synthesize a Delete
          // diff card per matched path so the user gets a Restore
          // button (same UX as Cursor's deleteToolCall).
          //
          // Why not `continue` afterwards: bash commands are often
          // composite (`rm tmp && build`). Letting the generic
          // formatToolUse trace pill render alongside the cards
          // preserves the full command for the user; the cards are
          // additive surface for the destructive sub-action.
          //
          // Out of scope (falls through to plain trace pill):
          //   • `-r` / `-R` / `--recursive` — directory deletes need a
          //     "restore tree" UX we don't have, and naively listing
          //     N files in a tree would balloon the chat.
          //   • Paths with shell expansion (`$VAR`, `~/foo`,
          //     globs) — we don't run a shell, can't resolve them.
          //   • `find ... -delete`, `xargs rm`, heredocs — not worth
          //     parsing for the long tail.
          const cmd = typeof input.command === 'string' ? input.command : '';
          const cwd = resolveSessionCwd(sessionId);
          const deletes = extractBashDeletes(cmd, cwd);
          for (let i = 0; i < deletes.length; i++) {
            const fp = deletes[i];
            // Synthesize a per-path tool id so updateEditEvent finds
            // the right card. Multiple `rm a b c` paths map to
            // multiple cards under the same Bash invocation id.
            const cardId = deletes.length === 1 ? id : `${id}#del${i}`;
            appendEditEvent(sessionId, {
              toolId: cardId,
              filePath: fp,
              oldText: '',
              newText: '',
              isCreate: false,
              isDelete: true,
              wholeFile: true,
              // Always loading until git_show resolves — Claude's bash
              // tool doesn't carry pre-deletion content, so HEAD
              // backfill is the only recovery path.
              status: 'loading'
            });
            void backfillDeleteOldText(sessionId, cardId, fp);
          }
          // intentional fall-through to formatToolUse below
        }
        const formatted = formatToolUse(name, input);
        if (formatted) merged.onTraceDelta?.(sessionId, formatted);
      }
    }
  }
}

/** Coerce a possibly-missing JSON number field to a finite integer.
 *  Stream-json usage blocks usually have all four token counters but
 *  occasionally drop fields when the value would be 0 — return 0 in
 *  that case so the math downstream doesn't NaN. */
function numField(obj: Record<string, unknown>, key: string): number {
  const v = obj[key];
  return typeof v === 'number' && Number.isFinite(v) ? v : 0;
}

/** Pull plain text out of a `tool_result` content block. Claude API
 *  shape allows the result's `content` field to be EITHER a string
 *  (the simple case) OR an array of content blocks (so a tool can
 *  return text + image + text etc). We flatten both into a single
 *  string for persistence; image blocks become a placeholder note. */
function extractToolResultText(block: Record<string, unknown>): string {
  const content = block.content;
  if (typeof content === 'string') return content;
  if (Array.isArray(content)) {
    const parts: string[] = [];
    for (const c of content) {
      if (c && typeof c === 'object') {
        const item = c as Record<string, unknown>;
        if (item.type === 'text' && typeof item.text === 'string') {
          parts.push(item.text);
        } else if (item.type === 'image') {
          parts.push('[image]');
        }
      }
    }
    return parts.join('\n').trim();
  }
  return '';
}

/** Resolve the pre-agent contents of a file the agent just `Write`'d, by
 *  asking Tauri for the HEAD-version + tracked-status in one round-trip.
 *  Runs out-of-band (called via `void`) because `handleStreamEvent` is
 *  synchronous and we don't want to block the next stream line.
 *
 *  Replaces the old `git_repo_root + git_show` pair, which had two
 *  silent failure modes that combined into "Revert deletes a tracked
 *  file":
 *    • `git_repo_root` was called with the *file* path, not a directory
 *      — git rejected it, the call threw, and the catch left the card
 *      `isCreate=true`. Revert then `remove_file()`'d committed paths.
 *    • An empty `git_show` result (file exists in worktree but not at
 *      HEAD — e.g. just-staged, or a fresh repo with no commits) was
 *      indistinguishable from "untracked, brand-new file"; both ended
 *      up as `isCreate=true` and Revert deleted them.
 *
 *  `pre_write_baseline` (Rust side) does the parent-dir resolve, the
 *  HEAD lookup, AND a `git ls-files --error-unmatch` tracked check, and
 *  returns all three so we make one informed decision here:
 *
 *    • `repo_root === ""` → not in any git repo. Genuinely no baseline.
 *      Stay on the optimistic isCreate=true path; user can Revert to
 *      delete (Rust `revert_write` will then succeed since file isn't
 *      tracked anywhere).
 *    • `tracked === true` → file IS in git. Either as HEAD or just
 *      staged. This is a modify, not a create. We populate `oldText`
 *      with whatever `git show HEAD:` returned (may be empty for
 *      newly-staged-but-not-committed; that's fine, Revert will then
 *      truncate the file rather than delete it — recoverable in any
 *      editor's undo).
 *    • `tracked === false` but `repo_root !== ""` → file is in a repo
 *      but git doesn't know about it. Genuine new-file create. Leave
 *      isCreate=true so Revert deletes.
 *
 *  Tauri command never throws — Rust returns a struct with empty
 *  fields on any error. Try/catch is here only to defend against
 *  the IPC layer itself failing. */
async function backfillWriteOldText(
  sessionId: string,
  toolId: string,
  filePath: string
): Promise<void> {
  try {
    const baseline = await invoke<{
      repo_root: string;
      old_text: string;
      tracked: boolean;
    }>('pre_write_baseline', { filePath });
    if (baseline.tracked) {
      updateEditEvent(sessionId, toolId, {
        status: 'applied',
        oldText: baseline.old_text,
        isCreate: false
      });
      return;
    }
    // Untracked path (or not in a repo). Optimistic create stays —
    // Rust's revert_write has a tracked-file guardrail anyway, so even
    // if isCreate=true is wrong we can't lose committed content.
    updateEditEvent(sessionId, toolId, { status: 'applied' });
  } catch {
    updateEditEvent(sessionId, toolId, { status: 'applied' });
  }
}

/** Mirror of `backfillWriteOldText` for the Delete fallback path. We
 *  only end up here when cursor-agent didn't ship `prevContent` on the
 *  `deleteToolCall` event — rare, but cheaper to handle than to bail.
 *
 *  Three outcomes:
 *    • file was tracked at HEAD → `oldText` is its HEAD-version
 *      content; Restore re-creates it with that body.
 *    • file wasn't tracked / repo missing → `oldText` stays empty.
 *      Restore creates an empty file at the path. Worse UX than the
 *      tracked case, but still better than the previous "deleted /path"
 *      trace pill with no Restore at all.
 *
 *  We never report `error` here even on git failure: the deletion card
 *  itself is fine, it just lacks recovery content. The user can still
 *  manually re-create the file via the editor; surfacing a red error
 *  banner would be alarmist for a missing prevContent that was never
 *  promised. */
async function backfillDeleteOldText(
  sessionId: string,
  toolId: string,
  filePath: string
): Promise<void> {
  try {
    // Same Tauri command as Write's backfill — works for delete because
    // we just need the HEAD-version content; the `tracked`/`repo_root`
    // fields are unused here (Restore semantics don't depend on them).
    const baseline = await invoke<{
      repo_root: string;
      old_text: string;
      tracked: boolean;
    }>('pre_write_baseline', { filePath });
    updateEditEvent(sessionId, toolId, {
      status: 'applied',
      oldText: baseline.old_text
    });
  } catch {
    updateEditEvent(sessionId, toolId, { status: 'applied' });
  }
}

/** Resolve the effective cwd for a session — same priority order
 *  agentCompact / agentContext use:
 *    1. explicit worktree path (isolated worktree always wins)
 *    2. session.cwd (manually picked or inherited)
 *    3. linked editor's repoPath (link is "follow this editor's
 *       folder", so its repoPath is the natural fallback)
 *  Returns "" if none match — callers treat absolute paths as-is and
 *  leave relative paths unresolved (the backfill will then fail
 *  cleanly and the card stays empty-old-text).
 *
 *  Reading sessionsState from a non-reactive context is fine:
 *  $state proxies expose current values to plain reads; we just
 *  don't get reactivity, which we don't need for a one-shot lookup
 *  during stream-event handling. */
function resolveSessionCwd(sessionId: string): string {
  const s = sessionsState.list.find((x) => x.id === sessionId);
  if (!s) return '';
  if (s.worktreePath) return s.worktreePath;
  if (s.cwd) return s.cwd;
  if (s.linkedToEditor && s.linkedToEditorInstanceId) {
    return sessionsState.editorInstanceState[s.linkedToEditorInstanceId]?.repoPath ?? '';
  }
  return '';
}

/** Sniff a Bash command string for `rm`/`unlink` invocations and
 *  return the absolute paths each one would delete.
 *
 *  Recognised shapes:
 *    rm path
 *    rm -f path
 *    rm -fv a "with spaces.ts" 'single-quoted'
 *    sudo rm path
 *    cmd1 ; rm path ; cmd2     (semicolon-separated subcommands)
 *    cmd1 && rm path           (&& and || boundaries split too)
 *    rm -- path                (end-of-options marker — skipped)
 *    unlink path
 *
 *  Skipped (returns [] for those subcommands):
 *    rm -r dir / -R / --recursive — directory deletes are out of
 *      scope; we'd need to enumerate the tree pre-execution to offer
 *      Restore, and the agent already removed it.
 *    paths starting with $ or ~ — shell expansion needed to resolve;
 *      we don't run a shell.
 *    paths containing globs (`*`, `?`, `[...]`) — same reason.
 *    `find ... -delete`, `xargs rm`, heredocs, loops — out of scope.
 *
 *  The result is best-effort. False negatives (missed deletes) just
 *  drop us back to the trace-pill UX, which is what we had before;
 *  false positives would create a Restore card for a file that
 *  wasn't actually removed, but the matchers are deliberately
 *  conservative to keep that risk low (we require `rm` / `unlink` at
 *  the start of a subcommand, no globs/expansions). */
export function extractBashDeletes(command: string, cwd: string): string[] {
  if (!command) return [];
  const out: string[] = [];
  // Subcommand boundaries. Pipes are intentionally NOT a boundary —
  // a `rm` on the right side of `|` is suspicious (rm doesn't read
  // stdin for paths) and we don't want to be aggressive about
  // detecting deletes inside pipelines we can't reason about.
  const parts = command.split(/(?:&&|\|\||;|\n)/);
  for (const part of parts) {
    const trimmed = part.trim();
    if (!trimmed) continue;
    // Allow optional `sudo ` prefix; some agent-emitted commands use
    // it. Anything else before `rm`/`unlink` (cd, env vars, etc.)
    // means we'd need to track state across the chain — out of scope,
    // skip.
    const m = /^(?:sudo\s+)?(rm|unlink)\b(.*)$/.exec(trimmed);
    if (!m) continue;
    const argsStr = m[2];
    const tokens = tokenizeShell(argsStr);
    let rejected = false;
    const paths: string[] = [];
    for (const tok of tokens) {
      if (tok === '--') continue; // end-of-options marker
      if (tok === '--recursive' || tok === '--no-preserve-root') {
        rejected = true;
        break;
      }
      if (tok.startsWith('--')) continue; // unknown long flag — safe to ignore
      if (tok.startsWith('-') && tok.length > 1) {
        // Short-flag cluster. Reject the whole subcommand if
        // recursive is set — we don't restore directories.
        if (/[rR]/.test(tok)) {
          rejected = true;
          break;
        }
        continue;
      }
      // Positional path. Anything that needs shell expansion is
      // unsafe to interpret without running a shell.
      if (
        tok.startsWith('$') ||
        tok.startsWith('~') ||
        /[*?[\]]/.test(tok)
      ) {
        rejected = true;
        break;
      }
      paths.push(tok);
    }
    if (rejected) continue;
    for (const p of paths) {
      out.push(resolveAgainstCwd(p, cwd));
    }
  }
  return out;
}

/** Quote-aware tokenizer for the args portion of a shell command.
 *  Handles single + double quotes (no escape processing — agents
 *  rarely emit `\"` inside paths, and the failure mode is benign:
 *  a token boundary lands wrong and we drop or keep an extra char,
 *  which the eventual git_show backfill resolves to "file not
 *  tracked"). Whitespace inside quotes is preserved. */
function tokenizeShell(s: string): string[] {
  const tokens: string[] = [];
  let buf = '';
  let quote: '"' | "'" | null = null;
  for (let i = 0; i < s.length; i++) {
    const ch = s[i];
    if (quote) {
      if (ch === quote) quote = null;
      else buf += ch;
    } else if (ch === '"' || ch === "'") {
      quote = ch;
    } else if (ch === ' ' || ch === '\t') {
      if (buf) {
        tokens.push(buf);
        buf = '';
      }
    } else {
      buf += ch;
    }
  }
  if (buf) tokens.push(buf);
  return tokens;
}

/** Join `path` against `cwd` if `path` is relative. Strips a leading
 *  `./` so the resulting absolute path is canonical-ish. We don't
 *  collapse `..` segments — Tauri's git layer handles them, and a
 *  malformed path just produces an empty restore card on the
 *  backfill failure (acceptable). */
function resolveAgainstCwd(path: string, cwd: string): string {
  if (!path) return path;
  let p = path;
  if (p.startsWith('./')) p = p.slice(2);
  if (p.startsWith('/')) return p;
  if (!cwd) return p;
  return cwd.replace(/\/$/, '') + '/' + p;
}

