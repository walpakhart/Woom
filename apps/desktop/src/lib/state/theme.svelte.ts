/* App-wide colour theme.
 *
 * Three palettes:
 *   iconic — original molten-gold-on-graphite (the brand look).
 *   light  — Tint background + Shade accents (cream + chocolate).
 *   dark   — Shade background + Tint accents (the inverse).
 *
 * Implementation is just a `data-theme` attribute on `<html>` plus
 * matching `:root[data-theme="…"]` blocks in app.css that override
 * the existing colour custom-properties. Components don't need to
 * know which theme is active — they keep reading `var(--bg-0)` /
 * `var(--accent)` / etc.
 */

const KEY = 'forgehold:theme:v1';

export type ThemeName = 'iconic' | 'light' | 'dark';

const VALID: ThemeName[] = ['iconic', 'light', 'dark'];

export const themeState = $state<{ name: ThemeName }>({
  name: readPersistedTheme()
});

function readPersistedTheme(): ThemeName {
  try {
    const raw = localStorage.getItem(KEY);
    if (raw && (VALID as string[]).includes(raw)) return raw as ThemeName;
  } catch {
    /* SSR / privacy mode */
  }
  return 'iconic';
}

/** Push the chosen theme to `<html data-theme="…">` so the CSS
 *  overrides take effect, and persist the choice. Safe to call from
 *  any component / boot-time effect. */
export function applyTheme(name: ThemeName) {
  themeState.name = name;
  if (typeof document !== 'undefined') {
    document.documentElement.dataset.theme = name;
  }
  try {
    localStorage.setItem(KEY, name);
  } catch {
    /* ignore */
  }
}

/** Boot-time apply — run once from +page.svelte so the saved theme
 *  flips on before the first paint. Component-level setting is done
 *  via `applyTheme()`. */
export function initTheme() {
  applyTheme(themeState.name);
}
