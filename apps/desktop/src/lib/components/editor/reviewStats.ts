import { computeHunks, hunkNewRange } from './inlineHunks';

/* Per-edit summary stats derived from the SAME line-diff engine the inline
   overlay uses (`computeHunks`), so the ReviewPane's `+N −M` counts and the
   editor overlay can never drift. No second diff implementation. */

/** Added / removed line counts for an edit, summed across all hunks. Identical
 *  input (no change) → `{ add: 0, rem: 0 }`. */
export function editStats(oldText: string, newText: string): { add: number; rem: number } {
  let add = 0;
  let rem = 0;
  for (const h of computeHunks(oldText ?? '', newText ?? '')) {
    add += h.added.length;
    rem += h.removed.length;
  }
  return { add, rem };
}

/** 1-based new-side line of the first change — the scroll target when opening
 *  an edit in the editor. `null` when there's no change. */
export function firstChangedLine(oldText: string, newText: string): number | null {
  const hunks = computeHunks(oldText ?? '', newText ?? '');
  return hunks.length ? hunkNewRange(hunks[0]).fromLine : null;
}
