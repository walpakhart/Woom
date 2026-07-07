/* Interface direction — the Cabin/Quiet redesign's second axis
 * (orthogonal to the colour theme).
 *
 *   cabin — persistent chrome: IconRail + list panes + Context dock.
 *   quiet — empty stage: no rail/titlebar, one centred column, a
 *           charcoal bottom Dock, context lives in a popover.
 *
 * Both colour themes work in both directions (4 combinations). Like
 * `theme.svelte.ts`, this is just a `data-layout` attribute on `<html>`
 * plus `[data-layout='quiet']` CSS; components read the attribute (or
 * this store) rather than being rebuilt per direction.
 *
 * Toggled instantly with ⌘. and from Settings → Appearance. */

const KEY = 'woom:layout-mode:v1';

export type LayoutMode = 'cabin' | 'quiet';

const VALID: LayoutMode[] = ['cabin', 'quiet'];

export const layoutModeState = $state<{ mode: LayoutMode }>({
  mode: readPersisted()
});

function readPersisted(): LayoutMode {
  try {
    const raw = localStorage.getItem(KEY);
    if (raw && (VALID as string[]).includes(raw)) return raw as LayoutMode;
  } catch {
    /* SSR / privacy mode */
  }
  return 'cabin';
}

/** Push the direction to `<html data-layout="…">` and persist. Safe
 *  from any component / boot effect. */
export function applyLayoutMode(mode: LayoutMode) {
  layoutModeState.mode = mode;
  if (typeof document !== 'undefined') {
    document.documentElement.dataset.layout = mode;
  }
  try {
    localStorage.setItem(KEY, mode);
  } catch {
    /* ignore */
  }
}

/** ⌘. flips between the two directions. */
export function toggleLayoutMode() {
  applyLayoutMode(layoutModeState.mode === 'cabin' ? 'quiet' : 'cabin');
}

/** Boot-time apply — run once from +page.svelte so the saved direction
 *  is on before first paint. */
export function initLayoutMode() {
  applyLayoutMode(layoutModeState.mode);
}
