/* Horizontal autoscroll for card drag-and-drop in the workbench.
 *
 * When the user grabs a card (Jira / GitHub / Sentry / file / canvas
 * shape) and drags it toward the workbench's off-screen edge, this
 * scrolls `.wb-columns` so the intended drop target comes into view.
 * The handlers are attached at the document level by `+page.svelte`
 * (see the drag-related event wiring); this module is the pointer-
 * driven step loop.
 *
 * Keeps no Svelte runes — a tiny class-style state machine. The
 * caller hands us the live drag payload via `payloadFn` so the
 * autoscroll only runs while a drag is in flight. */

const EDGE_BAND_PX = 100;
const MAX_STEP_PX = 26;
const MIN_STEP_PX = 4;

export class DragAutoscroll {
  private raf: number | null = null;
  private pointerX = 0;

  /** True while a drag is in progress. We use a callback instead of a
   *  reactive read because this module ships as plain TS (no `.svelte.ts`
   *  rune support), and the caller already owns the drag-state store. */
  private hasPayload: () => boolean;

  constructor(hasPayload: () => boolean) {
    this.hasPayload = hasPayload;
  }

  /** Bind to a `dragover` listener on `document` (or a parent). */
  track = (e: DragEvent): void => {
    this.pointerX = e.clientX;
    if (this.raf === null && this.hasPayload()) {
      this.raf = requestAnimationFrame(this.step);
    }
  };

  /** Cancel the running rAF loop on `dragend`. */
  stop = (): void => {
    if (this.raf !== null) cancelAnimationFrame(this.raf);
    this.raf = null;
  };

  private step = (): void => {
    if (!this.hasPayload()) {
      this.raf = null;
      return;
    }
    const wb = document.querySelector<HTMLElement>('.wb-columns');
    if (!wb) {
      this.raf = null;
      return;
    }
    const rect = wb.getBoundingClientRect();
    const vw = window.innerWidth || document.documentElement.clientWidth;
    /* Clamp the band's horizontal extent against the viewport edges
     * so a column that overflows the page (`.wb-columns` extends past
     * the visible area) still triggers scroll when the cursor is at
     * the visible edge, not the column's logical edge. */
    const effectiveRight = Math.min(rect.right, vw);
    const effectiveLeft = Math.max(rect.left, 0);

    let dx = 0;
    if (this.pointerX > effectiveRight - EDGE_BAND_PX) {
      const proximity = this.pointerX - (effectiveRight - EDGE_BAND_PX);
      dx = Math.min(MAX_STEP_PX, Math.max(MIN_STEP_PX, Math.round(proximity / 3)));
    } else if (this.pointerX > 0 && this.pointerX < effectiveLeft + EDGE_BAND_PX) {
      const proximity = (effectiveLeft + EDGE_BAND_PX) - this.pointerX;
      dx = -Math.min(MAX_STEP_PX, Math.max(MIN_STEP_PX, Math.round(proximity / 3)));
    }

    if (dx !== 0) {
      wb.scrollLeft = Math.max(
        0,
        Math.min(wb.scrollWidth - wb.clientWidth, wb.scrollLeft + dx)
      );
    }
    this.raf = requestAnimationFrame(this.step);
  };
}
