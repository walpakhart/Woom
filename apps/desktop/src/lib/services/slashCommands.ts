/* Slash-command interceptor for the agent composer
 * (`docs/ROADMAP_1.0.md §2.2.4`).
 *
 * These are Forge-side actions; they are NOT passed through to the
 * underlying CLI. The composer calls `parseSlashCommand` on the
 * trimmed input before sending. If the result is non-null, the
 * caller dispatches to the corresponding action helper instead of
 * issuing an agent turn.
 *
 * Only matches when the WHOLE message is the command — `/compact`
 * alone triggers; "/compact please" is a normal message. This keeps
 * users free to write a message that happens to start with a slash
 * (e.g. quoting a rule like "/^foo$/" in a code review).
 */

import type { ClaudeSession } from '$lib/types';
import { appendSessionMessage } from '$lib/state/sessions.svelte';
import { contextWindowFor, costForUsage, formatCostUsd, formatTokens } from '$lib/usage';

export type SlashCommand = 'compact' | 'clear' | 'usage' | 'help';

export const KNOWN_SLASH_COMMANDS: SlashCommand[] = [
  'compact',
  'clear',
  'usage',
  'help'
];

/** Parse a composer message; returns the matched command or null. */
export function parseSlashCommand(input: string): SlashCommand | null {
  const trimmed = input.trim();
  if (!trimmed.startsWith('/')) return null;
  /* Exact match — the slash plus a single word, no args, nothing
   * after. We may extend this to args later (`/checkout <branch>`)
   * but for v1 the closed-set strict-match keeps the surface small
   * and predictable. */
  const word = trimmed.slice(1);
  if (!/^[a-z]+$/i.test(word)) return null;
  const lower = word.toLowerCase() as SlashCommand;
  return KNOWN_SLASH_COMMANDS.includes(lower) ? lower : null;
}

/** Drop every existing message from a session, leaving the user with
 *  a blank thread under the same id. The Claude resume-uuid is left
 *  alone — the next send creates a fresh upstream thread by virtue
 *  of the empty history, and the user can keep editing rules / cwd
 *  without re-creating the column.
 *
 *  Used by `/clear`. Distinct from `archiveSession` (which archives
 *  the column entirely) — `/clear` keeps the session alive but
 *  drops its content. */
export function clearSessionHistory(session: ClaudeSession): void {
  session.messages = [];
  session.actions = [];
  session.lastContextSize = 0;
  session.cwdSwitchRecap = null;
  session.awaitingApproval = false;
}

/** Append a synthetic assistant message that summarises the
 *  session's cumulative usage. Helps the user spot when a session is
 *  burning through quota faster than they expected without leaving
 *  the chat to dig into the popover.
 *
 *  Synthetic = NOT a real agent turn — the message is appended
 *  locally and nothing hits the wire. */
export function appendUsageBreakdown(session: ClaudeSession): void {
  let inputTokens = 0;
  let outputTokens = 0;
  let cacheRead = 0;
  let cacheWrite = 0;
  let costUsd = 0;
  let turnCount = 0;
  for (const msg of session.messages) {
    if (msg.role !== 'assistant') continue;
    const u = msg.usage;
    if (!u) continue;
    turnCount += 1;
    inputTokens += u.inputTokens;
    outputTokens += u.outputTokens;
    cacheRead += u.cacheReadTokens;
    cacheWrite += u.cacheCreationTokens;
    /* Re-compute the cost from the usage snapshot rather than reading
     * a cached field — the message type doesn't carry one, and we'd
     * rather pay the few microseconds of arithmetic than maintain a
     * parallel persisted value. */
    costUsd += costForUsage(u);
  }
  const window = contextWindowFor(session.claudeModel, session.agentKind);
  const lines: string[] = [
    `**Usage so far** — ${turnCount} assistant turn${turnCount === 1 ? '' : 's'}.`,
    '',
    `- Input: \`${formatTokens(inputTokens)}\``,
    `- Output: \`${formatTokens(outputTokens)}\``,
    `- Cache read: \`${formatTokens(cacheRead)}\``,
    `- Cache write: \`${formatTokens(cacheWrite)}\``,
    `- Estimated cost: \`${formatCostUsd(costUsd)}\``,
    `- Context window: \`${formatTokens(window)}\``,
    '',
    `_Session id: \`${session.id}\` · agent: \`${session.agentKind}\` · model: \`${session.claudeModel ?? session.cursorModel ?? 'auto'}\`_`
  ];
  appendSessionMessage(session.id, {
    role: 'assistant',
    content: lines.join('\n'),
    at: new Date().toISOString()
  });
}

/** Append a synthetic assistant message listing the slash commands.
 *  Pure UX — no agent call. */
export function appendSlashHelp(session: ClaudeSession): void {
  const lines = [
    '**Slash commands** (Woom-side; not sent to the agent):',
    '',
    '- `/compact` — drop history, keep a summary in the next turn',
    '- `/clear` — wipe this session\'s messages and start fresh',
    '- `/usage` — print token + cost breakdown for this session',
    '- `/help` — show this list'
  ];
  appendSessionMessage(session.id, {
    role: 'assistant',
    content: lines.join('\n'),
    at: new Date().toISOString()
  });
}
