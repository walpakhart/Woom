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

import { invoke } from '@tauri-apps/api/core';
import {
  appendToLastAssistant,
  appendToLastThinking,
  appendToLastTrace,
  appendEditEvent,
  updateEditEvent,
  addAction,
  updateLastAssistantUsage,
  genId
} from '$lib/state/sessions.svelte';
import { formatToolUse } from '$lib/format';
import type { ClaudeUsage } from '$lib/types';

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
  /** Called once per assistant API call with the `usage` block from
   *  stream-json. Multi-step turns produce several of these (one per
   *  sub-step); the default handler keeps overwriting so the latest
   *  sub-step wins — that sub-step's `cache_read` tokens reflect the
   *  full prior conversation, which is the cheapest informative
   *  single-number summary for the per-message badge. Optional. */
  onUsage?: (sessionId: string, usage: ClaudeUsage) => void;
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
    if (handlers.onUsage) overrides.onUsage = handlers.onUsage;
    merged = { ...defaultStreamHandlers, ...overrides };
  }
  if (!parsed || typeof parsed !== 'object') return;
  const msg = parsed as Record<string, unknown>;

  // `result` events terminate every cursor-agent turn (and claude in
  // its own way) and carry a single `usage` block on the top-level
  // event. Cursor names the fields camelCase (`inputTokens`,
  // `cacheReadTokens`, `cacheWriteTokens`, `outputTokens`); Claude's
  // shape lives on `assistant` events with snake_case names handled
  // below. We surface cursor-style usage here so the chip + badge
  // light up for cursor sessions too.
  if (msg.type === 'result' && msg.usage && typeof msg.usage === 'object') {
    const u = msg.usage as Record<string, unknown>;
    const inp = numField(u, 'inputTokens');
    const cacheRead = numField(u, 'cacheReadTokens');
    const cacheWrite = numField(u, 'cacheWriteTokens');
    const out = numField(u, 'outputTokens');
    merged.onUsage?.(sessionId, {
      inputTokens: inp,
      cacheCreationTokens: cacheWrite,
      cacheReadTokens: cacheRead,
      outputTokens: out,
      contextSize: inp + cacheWrite + cacheRead,
      // Cursor doesn't surface a stable model id on the result event
      // (the system/init carries a display name like "Opus 4.7 1M
      // High Thinking" but the cli-config modelId is what we'd want).
      // Leave null — the per-message badge falls back gracefully and
      // the context-ring chip uses the session's `cursorModel` field.
      model: null
    });
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
  // the very next reply.
  if (inner.usage && typeof inner.usage === 'object') {
    const u = inner.usage as Record<string, unknown>;
    const inp = numField(u, 'input_tokens');
    const cacheCreate = numField(u, 'cache_creation_input_tokens');
    const cacheRead = numField(u, 'cache_read_input_tokens');
    const out = numField(u, 'output_tokens');
    merged.onUsage?.(sessionId, {
      inputTokens: inp,
      cacheCreationTokens: cacheCreate,
      cacheReadTokens: cacheRead,
      outputTokens: out,
      contextSize: inp + cacheCreate + cacheRead,
      model: typeof inner.model === 'string' ? inner.model : null
    });
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
          // Write is a full-file overwrite. The payload only carries the
          // *new* contents — there's no `old_string` to diff against.
          // Strategy: ship a placeholder card immediately (so the user
          // sees "wrote X" in chat order without delay), then in the
          // background ask Tauri for `git show HEAD:<path>` to recover
          // the pre-agent version. Three outcomes:
          //   • git_show succeeds → swap in `oldText`, flip to applied,
          //     real diff renders.
          //   • file isn't tracked / cwd isn't a repo → leave `oldText`
          //     empty, mark `isCreate=true`. Card shows as "new file".
          //   • the agent really created a new file → same as above.
          // Why fetch from HEAD instead of capturing pre-state at
          // tool_use time: by the time the assistant block reaches us,
          // claude/cursor-agent has already executed Write — the file
          // on disk is already the new content. HEAD is the closest
          // pre-agent baseline we can recover after the fact.
          const fp = typeof input.file_path === 'string' ? input.file_path : '';
          const content = typeof input.content === 'string' ? input.content : '';
          if (fp) {
            appendEditEvent(sessionId, {
              toolId: id,
              filePath: fp,
              oldText: '',
              newText: content,
              // Optimistic default — we'll flip this to false in the
              // backfill if git_show finds the file in HEAD. Erring on
              // "new" rather than "modified" so the early-render card
              // doesn't promise a diff that's actually missing.
              isCreate: true,
              wholeFile: true,
              status: 'loading'
            });
            void backfillWriteOldText(sessionId, id, fp);
          }
          continue;
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

/** Resolve the pre-agent contents of a file the agent just `Write`'d, by
 *  asking git for the HEAD-version of that path. Runs out-of-band
 *  (called via `void`) because `handleStreamEvent` is synchronous and
 *  we don't want to block the next stream line on a Tauri round-trip.
 *
 *  Lookup chain:
 *    1. `git_repo_root(filePath)` — if filePath isn't inside a git
 *       worktree this throws and we bail (file stays as `isCreate`).
 *    2. Compute the path relative to the repo root. `git show
 *       HEAD:<rel>` only accepts repo-relative paths.
 *    3. `git_show(repo, "HEAD", rel)` — returns the file as-of HEAD,
 *       or errors if the file is untracked / didn't exist at HEAD.
 *
 *  All errors land on the same branch: leave the card in `applied`
 *  state with `oldText=""` and `isCreate=true`, which renders as a
 *  pure-additions diff. We deliberately don't surface the git error
 *  on the card — it's noise for the common case (writing a brand new
 *  file). The Revert button still works because `revert_write`
 *  treats `isCreate=true` as "delete the file we created". */
async function backfillWriteOldText(
  sessionId: string,
  toolId: string,
  filePath: string
): Promise<void> {
  try {
    const repoRoot = await invoke<string>('git_repo_root', { path: filePath });
    if (!repoRoot) {
      updateEditEvent(sessionId, toolId, { status: 'applied' });
      return;
    }
    // Repo-relative path. git_show takes the slash-form even on macOS
    // (the underlying call is `git show <rev>:<path>`), and our
    // filePath already uses '/' on Tauri's macOS/Linux targets.
    const rel = filePath.startsWith(repoRoot + '/')
      ? filePath.slice(repoRoot.length + 1)
      : filePath;
    const oldText = await invoke<string>('git_show', {
      repo: repoRoot,
      revision: 'HEAD',
      path: rel
    });
    updateEditEvent(sessionId, toolId, {
      status: 'applied',
      oldText,
      isCreate: false
    });
  } catch {
    // File isn't tracked at HEAD (brand-new file, untracked, or cwd
    // isn't a repo). Stay on the optimistic isCreate=true path.
    updateEditEvent(sessionId, toolId, { status: 'applied' });
  }
}

