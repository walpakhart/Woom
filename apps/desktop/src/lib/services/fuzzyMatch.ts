/* Tiny fuzzy matcher for the command palette
 * (`docs/ROADMAP_1.0.md §2.8.1` + §2.8.2).
 *
 * Same idea as fzf-style scoring without the ~16 KB minified weight
 * of an external library. Returns null when the query letters don't
 * appear in order in the target, otherwise a positive score where
 * higher = better match.
 *
 * Bonuses:
 *   - consecutive matches inside the same word (typo-tolerant)
 *   - matches at a word boundary (start of string / after `_`, `-`,
 *     `/`, ` `, `.`, `:`)
 *   - exact case match (small bonus, mostly for things like "PR" vs "pr")
 *
 * Penalties:
 *   - per non-matching character traversed
 *
 * Designed for short, single-line palette rows (titles + subtitles),
 * not for prose. Big enough to be useful, small enough to inline in
 * a `.derived.by` without burning a frame on a 200-row palette.
 */

const BONUS_CONSEC = 4;
const BONUS_BOUNDARY = 6;
const BONUS_EXACT_CASE = 1;
const PENALTY_GAP = 1;

function isBoundaryBefore(text: string, idx: number): boolean {
  if (idx === 0) return true;
  const prev = text.charCodeAt(idx - 1);
  /* Treat any of these as a boundary so "open-pr" matches "OpenPR"
   * AND "open_pr" AND "open pr". CamelCase boundary detection
   * (lower → upper) is also folded in for stuff like "JsDocComment". */
  // Space / punctuation
  if (prev === 32 || prev === 95 || prev === 45 || prev === 47
    || prev === 46 || prev === 58 || prev === 44) return true;
  // CamelCase: previous lower, current upper
  const cur = text.charCodeAt(idx);
  if (cur >= 65 && cur <= 90 && prev >= 97 && prev <= 122) return true;
  return false;
}

/** Score `target` against `query`. Both inputs are taken as-is — no
 *  trim / lowercase done by the caller. Returns null when query is
 *  not a subsequence of target (case-insensitive). Score scales with
 *  the length of the query, not the target. */
export function fuzzyScore(query: string, target: string): number | null {
  if (!query) return 0; /* empty query matches everything with neutral score */
  if (!target) return null;
  const qLower = query.toLowerCase();
  const tLower = target.toLowerCase();
  let qi = 0;
  let lastMatchIdx = -1;
  let score = 0;
  let consecutive = 0;
  for (let ti = 0; ti < tLower.length && qi < qLower.length; ti++) {
    if (tLower.charCodeAt(ti) === qLower.charCodeAt(qi)) {
      score += 1; /* base hit */
      if (ti === lastMatchIdx + 1) {
        consecutive += 1;
        score += BONUS_CONSEC * consecutive;
      } else {
        consecutive = 0;
      }
      if (isBoundaryBefore(target, ti)) score += BONUS_BOUNDARY;
      if (target.charCodeAt(ti) === query.charCodeAt(qi)) score += BONUS_EXACT_CASE;
      lastMatchIdx = ti;
      qi += 1;
    } else if (lastMatchIdx >= 0) {
      score -= PENALTY_GAP;
      consecutive = 0;
    }
  }
  if (qi < qLower.length) return null; /* not all query chars matched */
  return Math.max(score, 1);
}

/** Score across multiple target fields and return the best score, or
 *  null when none of them match. The palette uses this to hit title
 *  AND subtitle in one call (`fuzzyScoreAny(query, [title, subtitle, key])`). */
export function fuzzyScoreAny(query: string, fields: (string | null | undefined)[]): number | null {
  let best: number | null = null;
  for (const f of fields) {
    if (!f) continue;
    const s = fuzzyScore(query, f);
    if (s !== null && (best === null || s > best)) best = s;
  }
  return best;
}
