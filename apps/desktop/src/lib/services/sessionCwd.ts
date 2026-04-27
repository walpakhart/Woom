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
import type { ClaudeSession } from '$lib/types';

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
    const map = { ...sess.cwdUuids };
    if (oldCwd && sess.claudeResumable) {
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
    patch.cwdUuids = map;
  }
  if (opts.breakLink) {
    patch.linkedToEditor = false;
    patch.linkedToEditorInstanceId = null;
  }
  updateSession(sessionId, patch);
}

/** Snapshot the last few user/assistant exchanges into a self-contained
 *  prose block, injected into the next turn's system prompt. Two
 *  flavours, both feeding off the same unified `sess.messages` history
 *  (which spans every cwd the session has visited):
 *    - `resumed: false` — fresh CLI conversation in a brand-new
 *      project. The CLI side has zero memory; the recap primes it
 *      with what was just being discussed.
 *    - `resumed: true` — returning to a project we've been in before.
 *      The CLI's resumed conversation already remembers THIS
 *      project's prior turns, but knows nothing of what was
 *      discussed elsewhere since we left. Recap fills that gap so
 *      cross-project work bleeds over (e.g. "we figured X in repo B
 *      that affects A").
 *  Each message is truncated to ~800 chars to bound the token cost.
 *  Returns null when there's nothing meaningful to recap. */
export function buildCwdSwitchRecap(
  sess: ClaudeSession,
  oldCwd: string | null,
  newCwd: string | null,
  opts: { resumed: boolean }
): string | null {
  const meaningful = sess.messages.filter(
    (m) => (m.role === 'user' || m.role === 'assistant') && m.content.trim().length > 0
  );
  if (meaningful.length === 0) return null;
  const recent = meaningful.slice(-6);
  const lines: string[] = [];
  if (opts.resumed) {
    lines.push("You're returning to a project you've been in before. Your CLI session here resumes with full memory of this project's prior chat. While you were elsewhere, the user had these other exchanges — they may relate to work here, or not:");
  } else {
    lines.push('Your cwd just changed mid-conversation. The CLI you run on uses a fresh session in the new project, so you have no memory of prior turns from its perspective. Forgehold preserved the last few exchanges below for continuity:');
  }
  if (oldCwd) lines.push(`- Previous cwd: ${oldCwd}`);
  if (newCwd) lines.push(`- ${opts.resumed ? 'Now back in' : 'New cwd'}: ${newCwd}`);
  lines.push('');
  lines.push('Recent exchanges (oldest → newest):');
  for (const m of recent) {
    const role = m.role === 'user' ? 'User' : 'You (assistant)';
    const text = m.content.trim();
    const trimmed = text.length > 800 ? `${text.slice(0, 799)}…` : text;
    lines.push(`${role}: ${trimmed}`);
  }
  lines.push('');
  if (opts.resumed) {
    lines.push("Continue from your remembered context. If anything from the cross-project chatter above touches the work in this repo, weave it in.");
  } else {
    lines.push("Continue from there with the new cwd in mind. The old project's files are no longer your working tree — if the user asks to keep working on the prior thread, your tools (Read/Write/Bash) are now scoped to the new project.");
  }
  return lines.join('\n');
}
