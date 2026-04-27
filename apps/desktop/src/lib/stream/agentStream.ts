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
  appendEditEvent,
  updateEditEvent,
  addAction,
  updateLastAssistantUsage,
  genId
} from '$lib/state/sessions.svelte';
import { formatToolUse } from '$lib/format';
import type { ClaudeUsage } from '$lib/types';

export interface AgentStreamHandlers {
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
        console.warn('[agentStream] dropped unknown content block:', block.type, block);
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

