// Rail badge counts — "how many new items has each source surfaced
// since the user last visited it". Drives the small numeric pill that
// appears on the GitHub / Jira / Sentry rail icons, mirroring the
// pattern inbox apps (Slack / Linear / Things) use to telegraph
// awaiting work without making the user open the solo.
//
// Source of truth: per-source inbox item count in the primary instance.
// We store a "last seen count" per source in localStorage; the rendered
// badge is `max(0, current - lastSeen)`. When the user opens a solo
// we snapshot the current count as the new baseline — badge clears.
//
// Why count-delta instead of timestamp + per-item `updated_at`: the
// inbox items currently expose stable ids but not a uniform `updated_at`
// across all three sources, and the goal is "did something new arrive
// while I wasn't looking" — count-delta answers that without forcing
// every InboxItem / JiraItem / SentryIssue to carry the same shape.
//
// Caveat: if items are removed from the inbox between visits, the count
// can briefly under-report. That's preferable to over-reporting (false
// red dots erode trust). A user-visible refresh always re-aligns.

const LS_KEY = 'woom:rail-badges-seen:v1';

type SourceKind = 'github' | 'jira' | 'sentry';

interface SeenMap {
  github: number;
  jira: number;
  sentry: number;
}

function readSeen(): SeenMap {
  try {
    const raw = localStorage.getItem(LS_KEY);
    if (!raw) return { github: 0, jira: 0, sentry: 0 };
    const parsed = JSON.parse(raw);
    return {
      github: typeof parsed.github === 'number' ? parsed.github : 0,
      jira: typeof parsed.jira === 'number' ? parsed.jira : 0,
      sentry: typeof parsed.sentry === 'number' ? parsed.sentry : 0
    };
  } catch {
    return { github: 0, jira: 0, sentry: 0 };
  }
}

function persistSeen(seen: SeenMap) {
  try {
    localStorage.setItem(LS_KEY, JSON.stringify(seen));
  } catch {/* ignore */}
}

export const railBadgeState = $state<{ seen: SeenMap }>({ seen: readSeen() });

/** Snapshot the current count as the new baseline. Called when the
 *  user navigates INTO the solo for this source — clears the badge
 *  even if items remain on the list. */
export function markSourceSeen(kind: SourceKind, currentCount: number) {
  railBadgeState.seen[kind] = currentCount;
  persistSeen(railBadgeState.seen);
}

/** How many "unseen" items to render on the badge. Clamped to ≥0 so
 *  a refresh that removes items doesn't flash a negative. */
export function badgeCount(kind: SourceKind, currentCount: number): number {
  const seen = railBadgeState.seen[kind];
  const delta = currentCount - seen;
  return delta > 0 ? delta : 0;
}
