// Live-card factory extracted from CanvasSurface.svelte in wave-29
// split. Maps a `DragPayload` from the inbox/file-tree/chat-thread
// onto the right Shape kind + initial props snapshot so the card
// still renders correctly when no source pane has the data
// loaded later (e.g. user closed the Jira column after dragging).
//
// `at` is the canvas-space click point — we center the standard
// 280x96 card on it. The chat-message variant gets a taller box
// (200) because its body is a markdown excerpt.
//
// Returns `null` for unsupported payload kinds so CanvasSurface's
// drop handler can bail out without throwing.

import { makeShape, type Shape } from '$lib/state/canvas.svelte';
import type { DragPayload } from '$lib/state/drag.svelte';

export function buildLiveCardShape(
  payload: DragPayload,
  at: { x: number; y: number }
): Shape | null {
  const CARD_W = 280;
  const CARD_H = 96;
  const x = at.x - CARD_W / 2;
  const y = at.y - CARD_H / 2;

  if (payload.source === 'jira') {
    const item = payload.item;
    return makeShape({
      kind: 'jira-card',
      x, y, w: CARD_W, h: CARD_H,
      props: {
        ticketKey: item.key,
        /* Snapshot the fields we render so the card still looks right
           when no Jira column has it loaded later. */
        snapshot: {
          key: item.key,
          summary: item.summary,
          status: item.status,
          priority: item.priority,
          issueType: item.issue_type,
          assignee: item.assignee?.display_name ?? null,
          updated: item.updated
        }
      }
    });
  }
  if (payload.source === 'github') {
    const item = payload.item;
    const owner = item.repo?.owner ?? '';
    const repoName = item.repo?.name ?? '';
    return makeShape({
      kind: item.is_pull_request ? 'github-pr-card' : 'github-issue-card',
      x, y, w: CARD_W, h: CARD_H,
      props: {
        owner,
        repo: repoName,
        number: item.number,
        snapshot: {
          title: item.title,
          state: item.state,
          merged: item.merged,
          draft: item.draft,
          author: item.author?.login ?? null,
          comments: item.comments,
          updated: item.updated_at
        }
      }
    });
  }
  if (payload.source === 'sentry') {
    const item = payload.item;
    return makeShape({
      kind: 'sentry-event-card',
      x, y, w: CARD_W, h: CARD_H,
      props: {
        issueId: item.id,
        shortId: item.short_id,
        snapshot: {
          title: item.title,
          level: item.level,
          status: item.status,
          count: item.count,
          culprit: item.culprit,
          project: item.project_slug
        }
      }
    });
  }
  if (payload.source === 'file') {
    return makeShape({
      kind: 'file-card',
      x, y, w: CARD_W, h: 70,
      props: {
        repoRoot: null,
        relPath: payload.path,
        isDir: payload.isDir
      }
    });
  }
  if (payload.source === 'chat-message') {
    const snap = payload.snapshot;
    /* Chat messages are taller because the excerpt body needs room
       (sticky-style markdown render). Cap at a sensible height — the
       user can resize. */
    return makeShape({
      kind: 'chat-message-card',
      x: at.x - CARD_W / 2,
      y: at.y - 100,
      w: CARD_W,
      h: 200,
      props: {
        sessionId: payload.sessionId,
        messageIndex: payload.messageIndex,
        snapshot: {
          role: snap.role,
          agentKind: snap.agentKind,
          sessionTitle: snap.sessionTitle,
          excerpt: snap.excerpt,
          at: snap.at
        }
      }
    });
  }
  return null;
}
