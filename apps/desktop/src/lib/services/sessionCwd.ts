// Session cwd swap logic. When the user repoints a chat session at a
// new working directory mid-conversation, two things must happen
// atomically:
//   1. The CLI session uuid swaps (claude / cursor-agent scope chat
//      memory by project — we either resume an old uuid stashed for
//      this cwd, or mint a fresh one).
//   2. A "cwd switch recap" gets stamped on the session as a one-shot
//      system-prompt suffix, injected into the very next turn so the
//      agent has continuity across the swap (the new CLI session has
//      no memory of what was just being discussed in the prior cwd).
//
// Pulled out of +page.svelte verbatim — pure mutator over the sessions
// store, no DOM, no events. The only state these touch is
// `sessionsState`; callers pass everything else as args.

import { sessionsState, updateSession, genUuid } from '$lib/state/sessions.svelte';
import type { ClaudeMessage, ClaudeSession } from '$lib/types';

/** Marker at the head of the system message `runCompactSession` writes
 *  into the chat after a successful /compact. Used to rediscover the
 *  prior-conversation summary on later context-resets (cwd switch,
 *  resume-orphan recovery) so we can fold "older history (summary) +
 *  recent verbatim turns" into the next system prompt instead of
 *  dropping everything before the last six exchanges. The marker is a
 *  whole-prefix match: any drift here will silently degrade the recap
 *  back to verbatim-only, which is harmless. */
export const COMPACT_SUMMARY_MARKER =
  'Compacted earlier conversation. New session seeded with this summary:';

/** Find the most-recent `/compact` summary in a session's transcript
 *  and return just the summary body (marker stripped, trimmed). Walks
 *  backwards so newer summaries win when the user has compacted more
 *  than once over the chat's life. Returns null when nothing matches —
 *  callers fall back to verbatim-only recap. */
export function extractCompactSummary(messages: ClaudeMessage[]): string | null {
  for (let i = messages.length - 1; i >= 0; i--) {
    const m = messages[i];
    if (m.role !== 'system') continue;
    const trimmed = m.content.trim();
    if (trimmed.startsWith(COMPACT_SUMMARY_MARKER)) {
      return trimmed.slice(COMPACT_SUMMARY_MARKER.length).trim() || null;
    }
  }
  return null;
}

/** Bound the per-session cwd→uuid memory. Hopping between many projects in
 *  one chat would otherwise grow the map without limit, bloating the
 *  serialized session JSON on every flush. JS object property order is
 *  insertion order, so reinserting a key on touch makes it "newest" — we
 *  evict by walking from the front (oldest first). */
const MAX_CWD_UUIDS = 20;

function trimCwdUuids(map: Record<string, string>): Record<string, string> {
  const keys = Object.keys(map);
  if (keys.length <= MAX_CWD_UUIDS) return map;
  const trimmed: Record<string, string> = {};
  for (const k of keys.slice(keys.length - MAX_CWD_UUIDS)) {
    trimmed[k] = map[k];
  }
  return trimmed;
}

/** Repoint a session at `newCwd`, rotating the CLI uuid (or restoring
 *  a previously-stashed one for this cwd) and stamping a one-shot
 *  recap on the session for the next turn's system prompt. Pass
 *  `breakLink: true` when the cwd change should also unbind the
 *  session from any linked editor (typical for explicit user
 *  actions like `pickCwd`). */
export function applySessionCwd(
  sessionId: string,
  newCwd: string | null,
  opts: { breakLink?: boolean } = {}
) {
  const sess = sessionsState.list.find((s) => s.id === sessionId);
  if (!sess) return;
  const oldCwd = sess.cwd ?? null;
  const cwdChanged = (oldCwd ?? '') !== (newCwd ?? '');
  const patch: Partial<ClaudeSession> = { cwd: newCwd };
  if (cwdChanged) {
    // Stash departing cwd's uuid (only if it's a live CLI conversation).
    // Re-insert pattern (`delete` then assign) so the touched key moves
    // to the end of the insertion order, making it the freshest entry
    // for the LRU trim below.
    const map = { ...sess.cwdUuids };
    if (oldCwd && sess.claudeResumable) {
      delete map[oldCwd];
      map[oldCwd] = sess.claudeUuid;
    }
    const restoreUuid = newCwd ? map[newCwd] : null;
    if (restoreUuid) {
      // Returning to a project we've been in before — resume its
      // conversation so the CLI side has full memory of THIS project's
      // history. But still inject a recap describing what was discussed
      // in OTHER projects since we left this one — the user may have
      // worked on things in B that are relevant when returning to A.
      // The CLI's resumed conversation has no knowledge of B's turns;
      // the recap fills that gap.
      // Touch newCwd → freshest in insertion order so it isn't evicted
      // on the very next hop.
      delete map[newCwd!];
      map[newCwd!] = restoreUuid;
      patch.claudeUuid = restoreUuid;
      patch.claudeResumable = true;
      patch.cwdSwitchRecap = buildCwdSwitchRecap(sess, oldCwd, newCwd, { resumed: true });
    } else {
      // Fresh project. New uuid, prime with recap of recent chatter so
      // the brand-new CLI conversation has continuity.
      patch.claudeUuid = genUuid();
      patch.claudeResumable = false;
      patch.cwdSwitchRecap = buildCwdSwitchRecap(sess, oldCwd, newCwd, { resumed: false });
    }
    patch.cwdUuids = trimCwdUuids(map);
  }
  if (opts.breakLink) {
    patch.linkedToEditor = false;
    patch.linkedToEditorInstanceId = null;
  }
  updateSession(sessionId, patch);
}

/** Why the CLI session lost (or never had) its conversation memory.
 *  Drives the recap's intro paragraph + which warnings the agent
 *  most needs to hear given how it got here. */
export type ContinuationReason =
  | 'cli_orphan'      // --resume target gone (CLI store pruned, reinstalled, etc.)
  | 'stop'            // User pressed Stop → force-killed CLI → uuid rotated
  | 'cwd_switch_fresh' // Moved to a project the session hasn't visited before
  | 'cwd_switch_resume' // Returned to a known project; CLI memory exists there but cross-project context lost
  | 'app_restart';    // Loaded from disk, CLI session likely stale

/** Optional metadata that fine-tunes the recap's framing. */
export interface ContinuationContext {
  /** Diagnostic from the CLI on `cli_orphan` (stderr tail). */
  detail?: string;
  /** For cwd-switch reasons — old/new paths shown in standing state. */
  oldCwd?: string | null;
  newCwd?: string | null;
}

/** Canonical recap builder for EVERY scenario where the CLI's
 *  conversation memory is lost or partial:
 *    - resume-orphan recovery (CLI session pruned)
 *    - user-initiated Stop (force-kill rotates uuid)
 *    - cwd switch (mid-chat repo hop, fresh CLI in new project)
 *    - cross-project return (CLI memory exists but missed work elsewhere)
 *    - app restart (session loaded from disk, CLI may be stale)
 *
 *  Single source of truth — replaces the prior two divergent builders
 *  (`buildCwdSwitchRecap` was 6 messages × 800 chars and missed
 *  tool-result detail; `buildResumeOrphanRecap` was 30 × 2500 with
 *  standing-state + first-user anchor). The unification matters
 *  because Stop and cwd-switch are common in the same flow — the
 *  user shouldn't see "agent forgot everything" depending on which
 *  trigger fired.
 *
 *  Always returns a non-null string (callers can stamp it directly
 *  onto `cwdSwitchRecap` without null-checking). Empty-history
 *  sessions get a minimal "fresh chat" placeholder. */
export function buildContinuationRecap(
  sess: ClaudeSession,
  reason: ContinuationReason,
  ctx: ContinuationContext = {}
): string {
  const meaningful = sess.messages.filter(
    (m) => (m.role === 'user' || m.role === 'assistant') && m.content.trim().length > 0
  );
  // Last 30 turns + ALWAYS the first user message (north-star anchor)
  // when it's outside the slice. 30 was tuned during stop-recovery
  // bug-fixing; smaller windows lost tool-result detail (project
  // refs, API responses) the agent then re-asked the user for.
  const recent = meaningful.slice(-30);
  const firstUser = meaningful.find((m) => m.role === 'user');
  const includeFirstUserAnchor = !!firstUser && !recent.includes(firstUser);
  const compactSummary = extractCompactSummary(sess.messages);

  // Standing state — durable facts the user expects the agent to
  // know across context-loss boundaries (worktree, cwd, pending
  // approval cards). Without this, agent re-asks "are we on a
  // worktree" / "is there a PR open".
  const standingState: string[] = [];
  if (sess.worktreePath) {
    standingState.push(
      `Working on isolated worktree at ${sess.worktreePath}` +
        (sess.worktreeBranch ? ` (branch ${sess.worktreeBranch})` : '') +
        (sess.worktreeRepo ? `, derived from ${sess.worktreeRepo}` : '')
    );
  } else if (sess.cwd) {
    standingState.push(`Working directory: ${sess.cwd}`);
  }
  if (ctx.oldCwd && ctx.oldCwd !== ctx.newCwd) {
    standingState.push(`Previous cwd (just left): ${ctx.oldCwd}`);
  }
  if (ctx.newCwd) {
    standingState.push(`Current cwd: ${ctx.newCwd}`);
  }
  const livePending = sess.actions.filter(
    (a) => a.status === 'pending' || a.status === 'error'
  );
  if (livePending.length > 0) {
    const summarized = livePending
      .map((a) => {
        if (a.kind === 'commit') return `commit card: "${a.message}" (${a.status})`;
        if (a.kind === 'pr') return `PR card: "${a.title}" → ${a.base || 'main'} (${a.status})`;
        if (a.kind === 'bash') return `bash card: \`${a.command}\` (${a.status})`;
        if (a.kind === 'switch_cwd') return `cwd-switch card: ${a.path} (${a.status})`;
        return null;
      })
      .filter((x): x is string => x !== null);
    if (summarized.length) standingState.push(`Pending action cards: ${summarized.join('; ')}`);
  }

  // Reason-specific intro. The hard rules block is shared — every
  // context-loss scenario has the same "don't re-ask, treat as
  // continuation, north-star is below" obligation.
  let intro: string;
  switch (reason) {
    case 'cli_orphan':
      intro = "You are CONTINUING a chat that was already in progress. The Claude CLI's session for this conversation was pruned (CLI reinstall, manual cleanup, or the session-id store was wiped), so its short-term memory is gone — but Woom preserved the full transcript and feeds you the relevant slice below.";
      break;
    case 'stop':
      intro = "You are CONTINUING a chat the user just paused with Stop. Woom force-killed the prior CLI process, rotated to a fresh uuid (so a now-locked session-id can't trip up the next spawn), and hands you the prior transcript below as the canonical record of what you'd been doing.";
      break;
    case 'cwd_switch_fresh':
      intro = "You are CONTINUING a chat whose working directory just changed to a project the session has not visited before. The CLI gets a fresh session here (CLI memory is project-scoped), so its short-term memory of THIS chat is empty. Woom's transcript below is the only record of what the user has been working on — including from the prior project.";
      break;
    case 'cwd_switch_resume':
      intro = "You are CONTINUING a chat whose cwd just returned to a project you've worked in before. The CLI's resumed session here has full memory of THIS project's prior turns, but missed any work that happened in OTHER projects since you left. The transcript slice below covers ALL recent turns regardless of project — read it for cross-project context that may now apply here.";
      break;
    case 'app_restart':
      intro = "You are CONTINUING a chat that was loaded from disk after Woom restarted. The CLI's own session memory may or may not be intact; the transcript below is the authoritative record either way.";
      break;
  }

  const lines: string[] = [
    intro +
      "\n\nNON-NEGOTIABLE rules for this turn:" +
      "\n1. Treat this as a direct continuation. The user did not start a fresh chat. Their NEXT message refers to context that's in this recap — do NOT ask them to re-explain." +
      "\n2. The 'ORIGINAL TASK' block below is the north star. If the user's next message is a short directive (\"продолжи\", \"go ahead\", \"да\"), it refers back to that task. Re-read it before answering." +
      "\n3. Do NOT say \"first message in the chat\" / \"I don't see prior context\" / \"can you remind me what we were doing\". The recap below IS your prior context. Read it." +
      "\n4. DO NOT RE-ASK FOR DATA YOU ALREADY DISCOVERED. If a prior turn ran a tool and the result is in the recap — project refs, API responses, file paths, command outputs, config values — you HAVE that data. Use it; never say \"can you give me X\" when X is already shown above." +
      "\n5. Tools are wired identically (memory MCP, GitHub/Jira/Sentry sidecars, app-nav, action cards). Only the CLI's working memory was (potentially) the casualty.",
    ''
  ];
  if (ctx.detail) lines.push(`Diagnostic from CLI: ${ctx.detail.slice(0, 240)}`);
  if (standingState.length) {
    lines.push('');
    lines.push('Standing state at the moment of context transition:');
    for (const s of standingState) lines.push(`- ${s}`);
  }
  if (compactSummary) {
    lines.push('');
    lines.push('Earlier conversation (compact summary, captured by the user via /compact):');
    lines.push(compactSummary);
  }
  if (includeFirstUserAnchor && firstUser) {
    lines.push('');
    lines.push("ORIGINAL TASK (the user's very first message in this chat — the north star, do NOT lose sight of it):");
    const text = firstUser.content.trim();
    const trimmed = text.length > 2000 ? `${text.slice(0, 1999)}…` : text;
    lines.push(`User: ${trimmed}`);
  }
  if (recent.length === 0) {
    lines.push('');
    lines.push('(No prior exchanges to recap — this is effectively a new chat.)');
  } else {
    lines.push('');
    lines.push(`Last ${recent.length} exchanges (oldest → newest):`);
    recent.forEach((m, idx) => {
      const role = m.role === 'user' ? 'User' : 'You (assistant)';
      const text = m.content.trim();
      // Tail-context refinement: the last 2 messages of the recap
      // are the most recent thing the user/agent saw before the
      // context-loss boundary, so the user's NEXT message is most
      // likely referring to something IN them (a URL the agent
      // offered, a question it asked, a list of options). Truncating
      // those at the same 2500 cap as older messages dropped the
      // exact piece the user was replying to — e.g. a PR URL at
      // the end of "PR opened: <url>". 8000-char cap on the tail
      // keeps long agent replies whole; older messages stay capped
      // at 2500 to bound total token spend.
      const isTailContext = idx >= recent.length - 2;
      const cap = isTailContext ? 8000 : 2500;
      const trimmed = text.length > cap ? `${text.slice(0, cap - 1)}…` : text;
      lines.push(`${role}: ${trimmed}`);
    });
  }
  lines.push('');
  lines.push("Pick up the conversation. Tools are wired identically; only the CLI's working memory was (potentially) the casualty.");
  return lines.join('\n');
}

/** @deprecated Kept as a thin alias for callers that haven't migrated.
 *  Routes to `buildContinuationRecap` with the matching reason. */
export function buildCwdSwitchRecap(
  sess: ClaudeSession,
  oldCwd: string | null,
  newCwd: string | null,
  opts: { resumed: boolean }
): string {
  return buildContinuationRecap(
    sess,
    opts.resumed ? 'cwd_switch_resume' : 'cwd_switch_fresh',
    { oldCwd, newCwd }
  );
}
