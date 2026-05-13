/* Most-recently-used picks for the command palette.
 *
 * Each pick is stored as a self-contained snapshot — display fields
 * + a `picker` descriptor that knows how to re-execute the jump.
 * That makes the dedicated "Recent" section in the palette work
 * even when the original source (e.g. a Jira project's cached issue
 * list) hasn't been re-fetched yet this session: we display from the
 * snapshot and re-issue the nav from the picker on click.
 *
 * Snapshot list is bounded at 50 entries; on each pick the entry is
 * bumped (or inserted) at the top. `mruRank(key)` keeps working as a
 * small fuzzy-score tie-breaker — call sites that just want
 * "did the user touch this recently?" don't change shape.
 *
 * Persisted as JSON in localStorage under `woom:palette-mru:v2`
 * (v1 was a plain string[] of keys; we don't migrate values, just
 * start from empty — the boost is recoverable in a session or two
 * of normal use).
 */

import type { View } from '$lib/state/view.svelte';

const STORAGE_KEY = 'woom:palette-mru:v2';
const MAX_ENTRIES = 50;

/** Discriminated picker — what to do when this MRU entry is chosen.
 * Each variant carries only primitives so the snapshot survives JSON
 * round-trip. Closures (e.g. action callbacks) are not embedded —
 * `action` carries the action id and the palette resolves it against
 * the currently-mounted `actions` prop. */
export type MruPicker =
  | { kind: 'view'; view: View }
  | { kind: 'app-editor' }
  | { kind: 'repo'; owner: string; repo: string }
  | { kind: 'jira-board'; boardId: number }
  | { kind: 'jira-project'; projectKey: string }
  | { kind: 'sentry-project'; slug: string }
  | { kind: 'github-item'; itemId: number }
  | { kind: 'jira-issue'; jiraKey: string }
  | { kind: 'sentry-issue'; issueId: string }
  | { kind: 'action'; actionId: string };

export type MruBadgeKind =
  | 'view'
  | 'editor'
  | 'canvas'
  | 'github'
  | 'jira'
  | 'sentry'
  | 'claude'
  | 'cursor'
  | 'action';

export type MruSnapshot = {
  /** Stable id (e.g. `gh:123`, `view:githubApp`, `action:connect-github`).
   *  Used for dedupe + `mruRank` lookups. */
  key: string;
  title: string;
  subtitle?: string;
  badge: string;
  badgeKind: MruBadgeKind;
  picker: MruPicker;
  /** ms epoch of last pick. Used to render "2m ago" style hints. */
  ts: number;
};

const initial = loadMru();

export const paletteMruState = $state<{ entries: MruSnapshot[] }>({
  entries: initial
});

/** Bump (or insert) `snapshot` at the top of the MRU list. Re-bumps are
 *  idempotent — the existing entry's `ts` is refreshed and display
 *  fields overwritten so a title rename surfaces next time. */
export function recordPalettePick(snapshot: Omit<MruSnapshot, 'ts'>): void {
  const next: MruSnapshot = { ...snapshot, ts: Date.now() };
  const filtered = paletteMruState.entries.filter((e) => e.key !== snapshot.key);
  filtered.unshift(next);
  paletteMruState.entries = filtered.slice(0, MAX_ENTRIES);
  persist();
}

/** Drop one entry (e.g. user clicks ✕ on a recent row). */
export function forgetPalettePick(key: string): void {
  const before = paletteMruState.entries.length;
  paletteMruState.entries = paletteMruState.entries.filter((e) => e.key !== key);
  if (paletteMruState.entries.length !== before) persist();
}

/** Where this key sits in the MRU list. Returns:
 *   0 → most recent
 *   1..N → progressively older
 *   -1 → never picked
 *
 * Callers translate to a score boost (e.g. `Math.max(0, 10 - rank)`). */
export function mruRank(key: string): number {
  return paletteMruState.entries.findIndex((e) => e.key === key);
}

function persist(): void {
  if (typeof localStorage === 'undefined') return;
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(paletteMruState.entries));
  } catch {
    /* SSR / quota — non-critical */
  }
}

function loadMru(): MruSnapshot[] {
  if (typeof localStorage === 'undefined') return [];
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    return parsed.filter(isValidSnapshot).slice(0, MAX_ENTRIES);
  } catch {
    return [];
  }
}

function isValidSnapshot(x: unknown): x is MruSnapshot {
  if (!x || typeof x !== 'object') return false;
  const o = x as Record<string, unknown>;
  return (
    typeof o.key === 'string' &&
    typeof o.title === 'string' &&
    typeof o.badge === 'string' &&
    typeof o.badgeKind === 'string' &&
    typeof o.ts === 'number' &&
    !!o.picker &&
    typeof o.picker === 'object' &&
    typeof (o.picker as { kind?: unknown }).kind === 'string'
  );
}
