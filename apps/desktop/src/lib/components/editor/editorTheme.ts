/* CodeMirror theme picker that follows the app's `themeState`.
 *
 * Returns the CM extension to wrap into a `Compartment` so the
 * editor surface (gutter / line bg / syntax colours) flips alongside
 * the rest of the UI when the user switches palette.
 *
 *   Iconic / Dark → `oneDark` (the existing extension; fits the
 *                   graphite + cream-on-chocolate surfaces).
 *   Light          → empty extension array, which falls back to
 *                   CodeMirror's default light styling so the editor
 *                   reads on a cream background.
 *
 * If you need finer-grained light styling later, swap the empty `[]`
 * for a proper light theme extension (e.g. `@uiw/codemirror-theme-…`)
 * — the rest of the wiring stays unchanged.
 */

import { oneDark } from '@codemirror/theme-one-dark';
import type { Extension } from '@codemirror/state';
import type { ThemeName } from '$lib/state/theme.svelte';

export function editorThemeExtension(name: ThemeName): Extension {
  if (name === 'light') return [];
  return oneDark;
}
