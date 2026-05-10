// Live-card lookup — maps a canvas live-card shape's lookup key to the
// freshest data we have in the app. Cards prefer live data (currently
// loaded by some inbox column) and fall back to the snapshot captured
// at drop-time when no column has the object loaded right now.
//
// Inbox state is **per-instance** — two Jira columns can browse two
// different boards on the same solo. We scan every Jira / GH /
// Sentry instance and return the first match. That matches user intent:
// if any column has the ticket loaded, the card is live; if none does,
// it's a stale snapshot.

import { inboxState } from '$lib/state/inbox.svelte';
import { sessionsState } from '$lib/state/sessions.svelte';
import type { InboxItem, JiraItem, SentryIssue } from '$lib/data';
import type { ClaudeMessage, ClaudeSession } from '$lib/types';

/** Live JiraItem for the given ticket key, or null if no column has it. */
export function findJiraItem(ticketKey: string): JiraItem | null {
  if (!ticketKey) return null;
  for (const items of Object.values(inboxState.jiraItemsByInstance)) {
    const hit = items.find((i) => i.key === ticketKey);
    if (hit) return hit;
  }
  return null;
}

/** Live GitHub InboxItem (PR or issue) matching the owner/repo/number tuple.
 *  GitHub items in the inbox don't carry a stable `is_pull_request` filter on
 *  every fetch path, so we rely on the caller to know which kind they want
 *  and just match by repo + number. */
export function findGithubItem(
  owner: string,
  repo: string,
  number: number
): InboxItem | null {
  if (!owner || !repo || !number) return null;
  for (const items of Object.values(inboxState.itemsByInstance)) {
    const hit = items.find(
      (i) =>
        i.number === number &&
        i.repo?.owner === owner &&
        i.repo?.name === repo
    );
    if (hit) return hit;
  }
  return null;
}

/** Live SentryIssue matching the issue id (Sentry's stable internal id,
 *  not the short_id). */
export function findSentryIssue(issueId: string): SentryIssue | null {
  if (!issueId) return null;
  for (const items of Object.values(inboxState.sentryItemsByInstance)) {
    const hit = items.find((i) => i.id === issueId);
    if (hit) return hit;
  }
  return null;
}

/** Live chat message lookup. Returns the session + message at the given
 *  index, or null if the session is gone or the index has scrolled off
 *  history. The renderer uses this to keep cards in sync as the agent
 *  edits / replaces messages mid-session. */
export function findChatMessage(
  sessionId: string,
  messageIndex: number
): { session: ClaudeSession; message: ClaudeMessage } | null {
  if (!sessionId) return null;
  const session = sessionsState.list.find((s) => s.id === sessionId);
  if (!session) return null;
  const message = session.messages[messageIndex];
  if (!message) return null;
  return { session, message };
}
