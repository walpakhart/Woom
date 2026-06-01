/* Pure git-status decoration helpers for the explorer tree. No Svelte
   deps so the precedence logic is unit-testable in isolation. */

/** Severity ranking for folder rollup — highest wins when a folder's
 *  descendants carry mixed statuses. Codes match the single-char values
 *  in EditorView's gitStatusByPath map (index|worktree folded to one). */
export const STATUS_RANK: Record<string, number> = {
  U: 6, // conflict / unmerged
  D: 5, // deleted
  M: 4, // modified
  R: 3, // renamed
  A: 2, // added / staged-new
  '?': 1 // untracked
};

/** Fold a set of git status codes to the single highest-severity one.
 *  Returns '' when there's nothing to show. Unknown codes rank 0 and
 *  only win if nothing else is present. */
export function foldStatus(codes: string[]): string {
  let best = '';
  let bestRank = 0;
  for (const c of codes) {
    const r = STATUS_RANK[c] ?? 0;
    if (r > bestRank) {
      bestRank = r;
      best = c;
    }
  }
  return best;
}
