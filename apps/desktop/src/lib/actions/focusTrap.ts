/* Reusable focus-trap Svelte action.
 *
 * Wrap any modal / dialog container with `use:focusTrap` and Tab /
 * Shift-Tab will cycle focus through the focusable descendants of
 * that container instead of escaping back to the page underneath.
 * The action also restores focus to whatever was focused before the
 * trap mounted, so dismissing a modal doesn't dump the user at the
 * top of the document.
 *
 * Used primarily for keyboard-only users — alongside the
 * `aria-modal="true"` already set on Woom's dialogs, this
 * satisfies the §1.6 (`docs/ROADMAP_1.0.md`) a11y requirement that
 * modals not leak keyboard focus to background content.
 */

const FOCUSABLE_SELECTORS = [
  'a[href]',
  'area[href]',
  'input:not([disabled]):not([type="hidden"])',
  'select:not([disabled])',
  'textarea:not([disabled])',
  'button:not([disabled])',
  'iframe',
  'object',
  'embed',
  '[contenteditable]:not([contenteditable="false"])',
  '[tabindex]:not([tabindex="-1"])'
].join(',');

/** True when the element is visible (display/visibility) AND not
 *  visually hidden via CSS. We don't try to detect off-screen
 *  positioning — that's a deliberate UX choice (off-screen with
 *  `tabindex` is still focusable per spec). */
function isVisible(el: HTMLElement): boolean {
  if (el.hidden) return false;
  const style = window.getComputedStyle(el);
  if (style.display === 'none' || style.visibility === 'hidden') return false;
  return true;
}

function focusableWithin(container: HTMLElement): HTMLElement[] {
  const all = Array.from(
    container.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTORS)
  );
  return all.filter(isVisible);
}

export function focusTrap(node: HTMLElement) {
  /* Stash the previously-focused element so we can return focus to
   * it on unmount. Falls back to `document.body` so unmount never
   * throws when nothing was focused. */
  const previouslyFocused =
    (document.activeElement as HTMLElement | null) ?? document.body;

  /* Move focus into the container on mount. Prefer the first
   * focusable child; fall back to the container itself (callers
   * already set `tabindex="-1"` on dialog containers). Wrapped in
   * a microtask so Svelte has a chance to attach children before we
   * try to query them. */
  queueMicrotask(() => {
    const focusables = focusableWithin(node);
    const target = focusables[0] ?? node;
    if (target && typeof target.focus === 'function') {
      try {
        target.focus({ preventScroll: false });
      } catch {
        /* Some elements (eg <object>) reject .focus() — ignore. */
      }
    }
  });

  function onKeydown(e: KeyboardEvent) {
    if (e.key !== 'Tab') return;
    const focusables = focusableWithin(node);
    if (focusables.length === 0) {
      e.preventDefault();
      return;
    }
    const first = focusables[0];
    const last = focusables[focusables.length - 1];
    const active = document.activeElement as HTMLElement | null;

    if (e.shiftKey) {
      /* Shift-Tab on the first element wraps to the last. Also
       * triggers when focus is OUTSIDE the container entirely (e.g.
       * focus moved to body via a programmatic blur). */
      if (active === first || !node.contains(active)) {
        e.preventDefault();
        last.focus();
      }
    } else if (active === last || !node.contains(active)) {
      e.preventDefault();
      first.focus();
    }
  }

  node.addEventListener('keydown', onKeydown);

  return {
    destroy() {
      node.removeEventListener('keydown', onKeydown);
      /* Restore focus to whoever had it before the trap mounted.
       * Skip when that element is no longer in the document
       * (parent unmounted while modal was open) so we don't throw. */
      if (previouslyFocused && document.body.contains(previouslyFocused)) {
        try {
          previouslyFocused.focus({ preventScroll: false });
        } catch { /* ignore */ }
      }
    }
  };
}
