/* User pins for repos and palette items.
 *
 * `docs/ROADMAP_1.0.md §2.3.12` (repo pinning) + `§2.8.11` (palette
 * pinned items). Both are bare key sets stored in localStorage —
 * tiny enough to share a module. Order doesn't matter; consumers
 * use the set membership and sort their own lists.
 */

const REPO_KEY = 'woom:pinned-repos:v1';
const PALETTE_KEY = 'woom:pinned-palette:v1';

export const pinnedState = $state<{
  /** GitHub repo full names (`owner/repo`). */
  repos: Set<string>;
  /** Palette result keys (e.g. `view:settings`, `repo:foo/bar`). */
  palette: Set<string>;
}>({
  repos: loadSet(REPO_KEY),
  palette: loadSet(PALETTE_KEY)
});

export function isRepoPinned(fullName: string): boolean {
  return pinnedState.repos.has(fullName);
}

export function toggleRepoPin(fullName: string): void {
  if (pinnedState.repos.has(fullName)) {
    pinnedState.repos.delete(fullName);
  } else {
    pinnedState.repos.add(fullName);
  }
  /* Reactivity: Svelte 5 fine-grained reactivity tracks the Set
   * itself, but mutations on the existing Set don't trigger a
   * re-derive in some places — reassign to force a notification. */
  pinnedState.repos = new Set(pinnedState.repos);
  saveSet(REPO_KEY, pinnedState.repos);
}

export function isPalettePinned(key: string): boolean {
  return pinnedState.palette.has(key);
}

export function togglePalettePin(key: string): void {
  if (pinnedState.palette.has(key)) {
    pinnedState.palette.delete(key);
  } else {
    pinnedState.palette.add(key);
  }
  pinnedState.palette = new Set(pinnedState.palette);
  saveSet(PALETTE_KEY, pinnedState.palette);
}

function loadSet(key: string): Set<string> {
  if (typeof localStorage === 'undefined') return new Set();
  try {
    const raw = localStorage.getItem(key);
    if (!raw) return new Set();
    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed)) return new Set();
    return new Set(parsed.filter((v): v is string => typeof v === 'string'));
  } catch {
    return new Set();
  }
}

function saveSet(key: string, set: Set<string>): void {
  if (typeof localStorage === 'undefined') return;
  try {
    localStorage.setItem(key, JSON.stringify([...set]));
  } catch {
    /* ignore */
  }
}
