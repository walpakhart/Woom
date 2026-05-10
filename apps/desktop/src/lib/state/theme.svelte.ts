/* App-wide colour theme.
 *
 * Two palettes:
 *   iconic — sage/mint on cool noir (the brand look, default dark).
 *   light  — sage/mint on cream-mint (the brand look, light variant).
 *
 * Implementation is just a `data-theme` attribute on `<html>` plus
 * matching `:root[data-theme="…"]` blocks in app.css that override
 * the existing colour custom-properties. Components don't need to
 * know which theme is active — they keep reading `var(--bg-0)` /
 * `var(--accent)` / etc.
 *
 * The previous separate "dark" theme has been retired — `iconic` is
 * already the dark variant under the new W-mark palette, so a
 * separate "dark" was redundant. Persisted `'dark'` values from
 * older builds are migrated to `iconic` at boot. */

const KEY = 'woom:theme:v1';

export type ThemeName = 'iconic' | 'light';

const VALID: ThemeName[] = ['iconic', 'light'];

export const themeState = $state<{ name: ThemeName }>({
  name: readPersistedTheme()
});

function readPersistedTheme(): ThemeName {
  try {
    const raw = localStorage.getItem(KEY);
    if (raw && (VALID as string[]).includes(raw)) return raw as ThemeName;
    /* Migrate old 'dark' → 'iconic' (Iconic IS the dark theme under
       the new palette; keeping a separate 'dark' was redundant). */
    if (raw === 'dark') {
      try { localStorage.setItem(KEY, 'iconic'); } catch { /* ignore */ }
      return 'iconic';
    }
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
