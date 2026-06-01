/* Auto-memory capture (on-demand `/remember`). Runs ONE hidden distill
 * turn over the current chat, asks the agent for durable prefs/feedback as
 * strict JSON, and surfaces each proposal as a `memory` approval card. The
 * memory store is written ONLY when the user approves a card (see
 * executeMemory in exec/actions.ts) — the distill turn never writes. */

import { runAgentRequest } from '$lib/exec/claude';
import {
  addAction,
  appendSessionMessage,
  genUuid,
} from '$lib/state/sessions.svelte';
import type { ClaudeSession } from '$lib/types';

type Proposal = { kind: string; content: string };
const VALID_KINDS = new Set(['user', 'feedback', 'project', 'reference']);
const MIN_USER_TURNS = 3;

const DISTILL_PROMPT =
  'Review THIS conversation and extract durable, reusable memories about the ' +
  'user or how to work with them — preferences, conventions, feedback, project ' +
  'facts, external references. Output ONLY a JSON array, no prose, no code ' +
  'fence. Each item: {"kind": "user"|"feedback"|"project"|"reference", ' +
  '"content": "<one or two sentences>"}. Skip anything ephemeral or already ' +
  'obvious. If nothing durable, output [].';

function parseProposals(reply: string): Proposal[] {
  let s = reply.trim();
  // Strip a ```json … ``` (or ``` … ```) fence if present.
  const fence = s.match(/```(?:json)?\s*([\s\S]*?)```/i);
  if (fence) s = fence[1].trim();
  // Grab the first [...] block so trailing prose can't break the parse.
  const start = s.indexOf('[');
  const end = s.lastIndexOf(']');
  if (start < 0 || end < start) return [];
  try {
    const arr = JSON.parse(s.slice(start, end + 1));
    if (!Array.isArray(arr)) return [];
    return arr
      .filter(
        (x): x is Proposal =>
          x && typeof x.content === 'string' && typeof x.kind === 'string'
      )
      .map((x) => ({ kind: x.kind.toLowerCase().trim(), content: x.content.trim() }))
      .filter((x) => x.content.length > 0 && VALID_KINDS.has(x.kind));
  } catch {
    return [];
  }
}

/** Run the distill turn + emit approval cards. Returns the proposal count. */
export async function distillMemories(session: ClaudeSession): Promise<number> {
  const userTurns = session.messages.filter((m) => m.role === 'user').length;
  if (userTurns < MIN_USER_TURNS || session.claudeResumable !== true) {
    appendSessionMessage(session.id, {
      role: 'system',
      content:
        '_Nothing to distill yet — have a more substantial conversation first, then run `/remember`._',
      at: new Date().toISOString(),
    });
    return 0;
  }

  let reply = '';
  try {
    const result = await runAgentRequest({
      sessionId: session.id,
      prompt: DISTILL_PROMPT,
      cwd: session.worktreePath ?? session.cwd ?? null,
      claudeUuid: session.claudeUuid,
      resume: true,
      rules: null,
      agentKind: 'claude',
      cursorModel: null,
      claudeModel: session.claudeModel ?? null,
      appContext: null,
      imagePaths: [],
      rtkDisabled: session.rtkEnabled === false,
      fastMode: session.fastMode === true,
      thinkingEffort: session.thinkingEffort ?? null,
      // Hidden turn — swallow stream deltas, don't touch the visible thread.
      onAssistantDelta: () => {},
    });
    reply = result.reply;
  } catch (e) {
    appendSessionMessage(session.id, {
      role: 'system',
      content: `_Memory distill failed: ${String(e)}_`,
      at: new Date().toISOString(),
    });
    return 0;
  }

  const proposals = parseProposals(reply);
  if (proposals.length === 0) {
    appendSessionMessage(session.id, {
      role: 'system',
      content: '_No durable memories found in this conversation._',
      at: new Date().toISOString(),
    });
    return 0;
  }

  for (const p of proposals) {
    addAction(session.id, {
      id: genUuid(),
      kind: 'memory',
      memKind: p.kind as 'user' | 'feedback' | 'project' | 'reference',
      content: p.content,
      status: 'pending',
    });
  }
  appendSessionMessage(session.id, {
    role: 'system',
    content: `_Proposed ${proposals.length} mem-save card${proposals.length === 1 ? '' : 's'} — approve the ones worth keeping._`,
    at: new Date().toISOString(),
  });
  return proposals.length;
}
