/* UI density mode — `comfortable` (default) or `compact`. Linear /
 * Slack-style "fit more rows in the same window" toggle. Distinct from
 * `scale.svelte.ts` (which is a uniform zoom on every pixel) — density
 * only trims the padding around list rows, pane headers, and the inbox
 * cards while leaving fonts and chrome at their original sizes. Net
 * effect on a 13" laptop: ~22% more inbox rows visible above the fold.
 *
 * Apply mechanism: sets `data-density="compact"` on `<html>` and lets
 * the CSS overrides in `app.css` (selectors keyed on
 * `:root[data-density="compact"]`) do the work. No JS per-frame math
 * and no per-component prop drilling — the toggle flips one attribute
 * and the cascade handles the rest.
 *
 * Why an attribute instead of a class: CSS specificity is one notch
 * higher for an attribute selector than a class without us having to
 * write `:root.density-compact`, which collides with theme classes
 * elsewhere. Symbolic difference, identical render, fewer foot-guns.
 */

const KEY = 'woom:density:v1';

export type Density = 'comfortable' | 'compact';

function readPersistedDensity(): Density {
  try {
    const raw = localStorage.getItem(KEY);
    if (raw === 'compact' || raw === 'comfortable') return raw;
  } catch {/* privacy mode — fall through */}
  return 'comfortable';
}

export const densityState = $state<{ value: Density }>({
  value: readPersistedDensity()
});

/** Apply the chosen density to `<html data-density>` and persist. Safe
 *  to call from any component / boot effect. SSR-safe — guards on
 *  `document` so the server-rendered HTML doesn't crash. */
export function applyDensity(value: Density) {
  densityState.value = value;
  if (typeof document !== 'undefined') {
    // `comfortable` is the implicit default — we strip the attribute
    // rather than write `data-density="comfortable"` so the cascade
    // doesn't need a `[data-density="comfortable"]` rule to match
    // (and so the DOM stays clean for the default case).
    if (value === 'compact') {
      document.documentElement.setAttribute('data-density', 'compact');
    } else {
      document.documentElement.removeAttribute('data-density');
    }
  }
  try {
    localStorage.setItem(KEY, value);
  } catch {/* ignore — out of quota / privacy mode */}
}

/** Flip between the two modes. Bound to ⌘\ in `+page.svelte`'s
 *  global keydown so power users can toggle without leaving the
 *  keyboard. */
export function toggleDensity() {
  applyDensity(densityState.value === 'compact' ? 'comfortable' : 'compact');
}

/** Boot-time apply — run once from +page.svelte's onMount so the
 *  saved value lands before the first paint. */
export function initDensity() {
  applyDensity(densityState.value);
}
