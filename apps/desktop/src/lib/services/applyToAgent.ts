/** "Apply to <agent>" plumbing for the editor's selection bar.
 *
 *  When the user highlights lines in the editor and clicks "Apply to
 *  Claude · Mona-Lisa", three things must happen in lockstep so the
 *  affordance feels like one action:
 *
 *    1. Pin the range as a `@path:start-end` mention on the target
 *       session's composer (so the agent sees what the user is
 *       referring to).
 *    2. Make that session the active tab in its column (otherwise
 *       the mention lands on a session the user can't currently see
 *       in the solo).
 *    3. Scroll the agent column into view in the solo so the
 *       composer is visible after the click — the editor and the
 *       chat are usually side-by-side, but the user might have a
 *       multi-column setup where the target column is offscreen.
 *
 *  Bundling those three calls here keeps the editor's selection-bar
 *  component a thin view layer, mirrors how `editorNavigation.ts`
 *  centralises "open this file" so the diff card and selection bar
 *  don't drift, and gives a single place to extend later (e.g.
 *  auto-focus the composer textarea, append a templated prompt). */

import {
  attachLineRangeMention,
  attachTerminalSelectionMention,
  setActiveSessionInInstance,
  sessionsState
} from '$lib/state/sessions.svelte';

export interface ApplyRangeArgs {
  sessionId: string;
  agentInstanceId: string;
  filePath: string;
  startLine: number;
  endLine: number;
}

/** Result is returned (instead of toast'd here) so the caller can
 *  decide between an in-line confirmation and a global toast — the
 *  selection bar uses the latter, but a future "Apply to all" command
 *  could batch results and toast once. */
export interface ApplyRangeResult {
  ok: boolean;
  /** The bare token without the `@` (e.g. `src/foo.ts:45-67`) so the
   *  caller can include it in a confirmation toast. */
  token: string | null;
}

export function applyRangeToAgent(args: ApplyRangeArgs): ApplyRangeResult {
  const token = attachLineRangeMention(
    args.sessionId,
    args.filePath,
    args.startLine,
    args.endLine
  );
  if (!token) return { ok: false, token: null };
  setActiveSessionInInstance(args.agentInstanceId, args.sessionId);
  /* Tell the InlineClaude pane to auto-expand the row for this
     session. Both composers (the agent app's main one and the inline
     mini one) read from the same `sess.input`, so the freshly-pinned
     `@path:line-range` mention shows up everywhere automatically;
     the expand signal just makes the inline row visible-by-default
     so the user doesn't have to click to find it. Consumed (cleared
     back to null) by InlineClaude's effect. */
  sessionsState.requestInlineExpandFor = args.sessionId;
  return { ok: true, token };
}

export interface ApplyTerminalArgs {
  sessionId: string;
  agentInstanceId: string;
  /** Human-friendly terminal name (e.g. "Hopper") — surfaces in the
   *  resulting `@token` so the agent (and the user) can tell which
   *  terminal the paste came from. */
  terminalLabel: string;
  /** Captured selection text, exactly as `xterm.getSelection()` returned
   *  it. Trimmed and whitespace-normalised inside the helper. */
  content: string;
}

/** Terminal twin of `applyRangeToAgent`: pins the captured shell output
 *  as a `@terminal/<label>:<hash>` mention onto the target session's
 *  composer, makes that session active in its column, and signals the
 *  inline-agents pane to auto-expand the row. The agent's prompt
 *  receives the literal selection bytes inline (the prompt-builder's
 *  non-`file` branch renders `@<id> — <title>\n\n<body>`), so it can
 *  reason about an error message, log line, or command output the user
 *  highlighted without needing a tool call. */
export function applyTerminalSelectionToAgent(args: ApplyTerminalArgs): ApplyRangeResult {
  const token = attachTerminalSelectionMention(
    args.sessionId,
    args.terminalLabel,
    args.content
  );
  if (!token) return { ok: false, token: null };
  setActiveSessionInInstance(args.agentInstanceId, args.sessionId);
  sessionsState.requestInlineExpandFor = args.sessionId;
  return { ok: true, token };
}
