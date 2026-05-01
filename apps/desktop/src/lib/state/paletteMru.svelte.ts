/* Most-recently-used picks for the command palette
 * (`docs/ROADMAP_1.0.md §2.8.3`).
 *
 * We track up to 50 keys; on each pick the key is bumped to the top
 * of the list. The palette consumes `mruRank(key)` to compute a
 * small score boost so a row the user has just touched is more
 * likely to surface for the next abbreviated query.
 *
 * Stored in localStorage under `forgehold:palette-mru:v1` so the
 * boost survives app restarts. Bounded list keeps the tracker O(1)
 * lookup via the in-memory Map regardless of history depth.
 */

const STORAGE_KEY = 'forgehold:palette-mru:v1';
const MAX_ENTRIES = 50;

const initial = loadMru();

export const paletteMruState = $state<{ entries: string[] }>({
  entries: initial
});

/** Bump `key` to the top of the MRU list. Idempotent re-bumps. */
export function recordPalettePick(key: string): void {
  const current = paletteMruState.entries;
  const filtered = current.filter((k) => k !== key);
  filtered.unshift(key);
  paletteMruState.entries = filtered.slice(0, MAX_ENTRIES);
  persist();
}

/** Where this key sits in the MRU list. Returns:
 *   0 → most recent
 *   1..N → progressively older
 *   -1 → never picked
 *
 * Callers translate to a score boost (e.g. `Math.max(0, 10 - rank)`). */
export function mruRank(key: string): number {
  return paletteMruState.entries.indexOf(key);
}

function persist(): void {
  if (typeof localStorage === 'undefined') return;
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(paletteMruState.entries));
  } catch {
    /* SSR / quota — non-critical */
  }
}

function loadMru(): string[] {
  if (typeof localStorage === 'undefined') return [];
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    return parsed
      .filter((k): k is string => typeof k === 'string')
      .slice(0, MAX_ENTRIES);
  } catch {
    return [];
  }
}
