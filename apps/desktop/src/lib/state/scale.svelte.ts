/* App-wide UI scale.
 *
 * One global multiplier (0.8–1.5) that grows or shrinks every glyph,
 * border and spacing in the window — same UX as Cursor's
 * "Window: Zoom Level" or VSCode's `window.zoomLevel`. Useful on
 * external monitors where the host OS scaling is too tight or too
 * loose for chat reading.
 *
 * Implementation: native WebKit zoom via Tauri's
 * `WebviewWindow::set_zoom`, exposed through the `set_window_zoom`
 * Rust command. This is the same primitive Chrome/Cursor/VSCode use
 * for Cmd+/- — the entire pixel grid is dilated uniformly, so
 * `position: fixed`, viewport units (`100vw`), scroll containers,
 * and `getBoundingClientRect` all stay consistent.
 *
 * We tried CSS `zoom: <factor>` on `<html>` first. It paints
 * correctly on a fresh load but silently breaks layout in WebKit
 * (Tauri 2's macOS engine): fixed-position overlays drift,
 * 100vw-wide bars undersize at <1.0× and overflow at >1.0×, and
 * `getBoundingClientRect` returns post-zoom values while
 * `clientX/Y` events are still in CSS pixels — so popovers anchor
 * to the wrong place and drag-handles miss their targets. Switching
 * to the native primitive eliminates all of this in one go.
 */

import { invoke } from '@tauri-apps/api/core';

const KEY = 'woom:scale:v1';

export const SCALE_OPTIONS = [
  { value: 0.8, label: '80%' },
  { value: 0.9, label: '90%' },
  { value: 1.0, label: '100%' },
  { value: 1.1, label: '110%' },
  { value: 1.25, label: '125%' },
  { value: 1.5, label: '150%' }
] as const;

export type ScaleValue = (typeof SCALE_OPTIONS)[number]['value'];

const VALID_VALUES: number[] = SCALE_OPTIONS.map((o) => o.value);

export const scaleState = $state<{ value: ScaleValue }>({
  value: readPersistedScale()
});

function readPersistedScale(): ScaleValue {
  try {
    const raw = localStorage.getItem(KEY);
    if (raw !== null) {
      const n = Number(raw);
      if (Number.isFinite(n) && VALID_VALUES.includes(n)) {
        return n as ScaleValue;
      }
    }
  } catch {
    /* SSR / privacy mode — default to 1.0 below. */
  }
  return 1.0;
}

/** Apply the chosen scale to the active webview window and persist
 *  to localStorage. Safe to call from any component / boot-time
 *  effect — the Rust command clamps to a sane range, and persistence
 *  failure is swallowed (privacy-mode tabs still get the in-session
 *  apply).
 *
 *  We deliberately don't await the invoke: Tauri returns immediately
 *  once the dispatcher posts the message, and the UI shouldn't block
 *  on the round-trip (it's a fire-and-forget visual change). If the
 *  call fails (e.g. window destroyed during shutdown), we don't have
 *  a recovery anyway. */
export function applyScale(value: ScaleValue) {
  scaleState.value = value;
  // Clean up any leftover CSS `zoom` from the previous implementation
  // — older builds wrote `zoom: <n>` directly to the `<html>` style
  // attribute. If it's still there it would compound on top of the
  // native webview zoom (e.g. user picks 110%, we'd actually paint at
  // 1.1 × the stale 0.9 = 0.99) and shift everything by a few px. One
  // unconditional removeProperty per call costs nothing.
  if (typeof document !== 'undefined') {
    document.documentElement.style.removeProperty('zoom');
  }
  void invoke('set_window_zoom', { factor: value }).catch(() => {
    /* Window gone or backend unreachable — nothing actionable. */
  });
  try {
    localStorage.setItem(KEY, String(value));
  } catch {
    /* ignore — quota / privacy mode. The state still applies in the
     * current session, just won't survive a restart. */
  }
}

/** Boot-time apply — run once from +page.svelte so the saved scale
 *  flips on before the user notices a default-100% flash. Component-
 *  level setting is done via `applyScale()`. */
export function initScale() {
  applyScale(scaleState.value);
}
