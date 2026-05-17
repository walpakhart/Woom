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
import {
  bgTasksState,
  findBgTaskByToken,
  killBgTask,
  spawnBgTask
} from '$lib/state/bgTasks.svelte';

export type SlashCommand =
  | 'compact'
  | 'clear'
  | 'usage'
  | 'help'
  | 'preview'
  | 'kill'
  | 'ps'
  | 'loop'
  | 'unloop'
  | 'sdd';

export const KNOWN_SLASH_COMMANDS: SlashCommand[] = [
  'compact',
  'clear',
  'usage',
  'help',
  'preview',
  'kill',
  'ps',
  'loop',
  'unloop',
  'sdd'
];

/** Commands that accept inline arguments after the slash (`/preview pnpm dev`).
 *  Arg-less commands stay in the strict-exact-match path so they remain
 *  unambiguous. */
const ARG_BEARING: Set<SlashCommand> = new Set(['preview', 'kill', 'loop', 'sdd']);

/** Display-shape for the inline slash-picker. Picker lives in
 *  Composer.svelte and filters this list by prefix as the user types
 *  past the leading `/`. Description renders right of the command in
 *  the dropdown row — keep each line short. */
export const SLASH_COMMAND_DESCRIPTIONS: Record<SlashCommand, string> = {
  compact: 'Summarize the chat into a shorter prefix and start fresh',
  clear:   'Drop all messages from this chat (keeps the session id)',
  usage:   'Append a token / cost breakdown for this session',
  help:    'Show the list of supported slash commands inline',
  preview: 'Spawn a background task (dev server, watcher) in the Preview pane',
  kill:    'Kill a tracked background task by id or label substring',
  ps:      'List running background tasks inline',
  loop:    'Re-send a prompt on a fixed cadence: /loop 5m check the deploy',
  unloop:  'Stop the active loop on this chat',
  sdd:     'Spec-Driven Development — agent writes spec/plan/phases into a temp workspace'
};

/** Parse a composer message; returns the matched command or null.
 *  Strict-exact-match for arg-less commands (compact / clear / usage /
 *  help / ps) — a trailing argument hides the command from this path.
 *  Use `parseSlashCommandWithArgs` for the arg-bearing commands. */
export function parseSlashCommand(input: string): SlashCommand | null {
  const trimmed = input.trim();
  if (!trimmed.startsWith('/')) return null;
  const word = trimmed.slice(1);
  if (!/^[a-z]+$/i.test(word)) return null;
  const lower = word.toLowerCase() as SlashCommand;
  if (!KNOWN_SLASH_COMMANDS.includes(lower)) return null;
  // Arg-bearing commands MUST have args (and so don't match the
  // strict-exact path). `/preview` alone is still useful — it opens
  // the pane composer — so we let that through.
  if (ARG_BEARING.has(lower)) {
    return lower === 'preview' ? lower : null;
  }
  return lower;
}

/** Args-bearing parse — matches `/cmd <rest of line>` where `cmd` is a
 *  known arg-bearing command and `<rest>` is at least one character.
 *  Returns null if the input is not arg-bearing OR doesn't match a
 *  known command — caller falls back to the regular agent send. */
export function parseSlashCommandWithArgs(
  input: string
): { name: SlashCommand; args: string } | null {
  const trimmed = input.trim();
  if (!trimmed.startsWith('/')) return null;
  const m = /^\/([a-z]+)\s+(.+)$/i.exec(trimmed);
  if (!m) return null;
  const name = m[1].toLowerCase() as SlashCommand;
  if (!KNOWN_SLASH_COMMANDS.includes(name)) return null;
  if (!ARG_BEARING.has(name)) return null;
  return { name, args: m[2].trim() };
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
    '- `/preview [cmd]` — open the Preview pane; with args, spawn a background task',
    '- `/kill <id|label>` — kill a tracked background task',
    '- `/ps` — list running background tasks',
    '- `/sdd <ask>` — spec → plan → phases workflow in a temp workspace (no git pollution)',
    '- `/loop <duration> <prompt>` / `/unloop` — re-send a prompt on a cadence',
    '- `/help` — show this list'
  ];
  appendSessionMessage(session.id, {
    role: 'assistant',
    content: lines.join('\n'),
    at: new Date().toISOString()
  });
}

/** Spawn a background task and append a confirmation line. Uses the
 *  session's worktree (or cwd) as the working dir. */
export async function spawnPreviewFromSlash(
  session: ClaudeSession,
  cmd: string
): Promise<void> {
  const cwd = session.worktreePath ?? session.cwd;
  if (!cwd) {
    appendSessionMessage(session.id, {
      role: 'assistant',
      content: '_Cannot spawn: this session has no cwd. Pick a folder first._',
      at: new Date().toISOString()
    });
    return;
  }
  const task = await spawnBgTask({ cmd, cwd, session_id: session.id });
  if (!task) {
    appendSessionMessage(session.id, {
      role: 'assistant',
      content: `_Failed to spawn \`${cmd}\` — check the Preview pane for diagnostics._`,
      at: new Date().toISOString()
    });
    return;
  }
  appendSessionMessage(session.id, {
    role: 'assistant',
    content: `_Spawned background task \`${task.label}\` (id \`${task.id}\`). Output streams in the Preview pane._`,
    at: new Date().toISOString()
  });
}

/** Resolve `<id|label-substr>` to a task and kill it. */
export async function killTaskFromSlash(
  session: ClaudeSession,
  token: string
): Promise<void> {
  const task = findBgTaskByToken(token);
  if (!task) {
    appendSessionMessage(session.id, {
      role: 'assistant',
      content: `_No task matches \`${token}\`. Use \`/ps\` to list._`,
      at: new Date().toISOString()
    });
    return;
  }
  await killBgTask(task.id);
  appendSessionMessage(session.id, {
    role: 'assistant',
    content: `_Killed \`${task.label}\` (id \`${task.id}\`)._`,
    at: new Date().toISOString()
  });
}

/** Start a loop. `args` shape: `<duration> <prompt>`. The duration
 *  parser accepts compound forms like `5m`, `2h30m`, `30s`. */
export async function startLoopFromSlash(
  session: ClaudeSession,
  args: string
): Promise<void> {
  const m = /^(\S+)\s+(.+)$/s.exec(args.trim());
  if (!m) {
    appendSessionMessage(session.id, {
      role: 'assistant',
      content: '_Usage: `/loop <duration> <prompt>` — e.g. `/loop 5m check the deploy`._',
      at: new Date().toISOString()
    });
    return;
  }
  const { parseDuration, startLoop } = await import('$lib/state/loops.svelte');
  const ms = parseDuration(m[1]);
  if (ms === null) {
    appendSessionMessage(session.id, {
      role: 'assistant',
      content: `_Bad duration \`${m[1]}\`. Try \`30s\`, \`5m\`, \`2h\`, \`1d\` (or combinations: \`2h30m\`)._`,
      at: new Date().toISOString()
    });
    return;
  }
  const task = startLoop(session.id, ms, m[2]);
  if (!task) {
    appendSessionMessage(session.id, {
      role: 'assistant',
      content: '_Failed to start loop — prompt was empty._',
      at: new Date().toISOString()
    });
    return;
  }
  appendSessionMessage(session.id, {
    role: 'assistant',
    content: `_/loop started — re-sending every ${m[1]}. \`/unloop\` to stop. Auto-expires in 7 days._`,
    at: new Date().toISOString()
  });
}

/** Kick off Spec-Driven Development for the user's ask.
 *
 *  1. Creates a temp workspace under `<app_data>/sdd-workspaces/<id>/`
 *     bound to this session.
 *  2. Drops the canonical spec-writer prompt into the composer input —
 *     the agent will use `ask_user_question` for any clarifications it
 *     needs, then write `spec.md` to the workspace.
 *  3. Caller fires `sendClaudeMessage()` right after this returns so
 *     the prompt actually leaves the composer. We DON'T fire the send
 *     ourselves because the chat closures live in +page.svelte and
 *     this helper is in a service module.
 *
 *  Returns the populated prompt the caller should send (already stamped
 *  into `session.input`), or null on failure. */
export async function startSddFromSlash(
  session: ClaudeSession,
  userPrompt: string
): Promise<string | null> {
  const { startSdd, buildKickoffPrompt } = await import('$lib/state/sdd.svelte');
  const ws = await startSdd(session.id, userPrompt);
  if (!ws) {
    appendSessionMessage(session.id, {
      role: 'assistant',
      content: '_Failed to create SDD workspace — check the app data directory permissions._',
      at: new Date().toISOString()
    });
    return null;
  }
  const prompt = await buildKickoffPrompt(ws);
  appendSessionMessage(session.id, {
    role: 'assistant',
    content: `_SDD started — workspace at \`${ws.root}\`. Agent will draft \`spec.md\` and stop. Approve via the inline card._`,
    at: new Date().toISOString()
  });
  return prompt;
}

export async function stopLoopFromSlash(session: ClaudeSession): Promise<void> {
  const { stopLoop } = await import('$lib/state/loops.svelte');
  const stopped = stopLoop(session.id);
  appendSessionMessage(session.id, {
    role: 'assistant',
    content: stopped ? '_/loop stopped._' : '_No active loop on this session._',
    at: new Date().toISOString()
  });
}

/** Inline `ps` — print the task list as a markdown table. */
export function appendBgTaskList(session: ClaudeSession): void {
  const tasks = bgTasksState.tasks;
  if (tasks.length === 0) {
    appendSessionMessage(session.id, {
      role: 'assistant',
      content: '_No background tasks. Spawn one with `/preview <cmd>`._',
      at: new Date().toISOString()
    });
    return;
  }
  const lines: string[] = [
    '**Background tasks** (`/preview` pane):',
    '',
    '| id | label | status | pid | url |',
    '| --- | --- | --- | --- | --- |'
  ];
  for (const t of tasks) {
    const statusStr =
      t.status.kind === 'running'
        ? 'running'
        : t.status.kind === 'exited'
          ? `exit ${t.status.code}`
          : 'killed';
    const url = t.detected_urls[0] ?? '—';
    lines.push(
      `| \`${t.id}\` | ${t.label} | ${statusStr} | ${t.pid ?? '—'} | ${url} |`
    );
  }
  appendSessionMessage(session.id, {
    role: 'assistant',
    content: lines.join('\n'),
    at: new Date().toISOString()
  });
}
