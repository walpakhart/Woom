// Custom drag image helper. The browser's default drag preview is the
// element being dragged — for tree rows that's a wide ugly stripe, for
// inbox cards it's the full card. We want a compact labeled chip that
// reads as "this thing is being moved", with a per-kind accent.
//
// `dataTransfer.setDragImage` requires the image element to exist in the
// DOM at the moment of `dragstart`. We append it off-screen then remove
// on the next tick — by then the browser has already snapshotted it.

export type DragChipKind = 'file' | 'dir' | 'jira' | 'github' | 'sentry' | 'cursor' | 'claude' | 'editor' | 'canvas' | 'terminal';

const KIND_GLYPH: Record<DragChipKind, string> = {
  file: '📄',
  dir: '📁',
  jira: 'J',
  github: 'GH',
  sentry: 'St',
  cursor: 'Cr',
  claude: 'C',
  editor: 'E',
  canvas: 'Cv',
  terminal: 'T'
};

const KIND_ACCENT: Record<DragChipKind, string> = {
  file: 'rgba(204, 120, 92, 0.95)',     // clay
  dir: 'rgba(204, 120, 92, 0.95)',
  jira: 'rgba(79, 142, 255, 0.95)',     // src-jira
  github: 'rgba(181, 132, 255, 0.95)',  // src-github
  sentry: 'rgba(232, 130, 100, 0.95)',  // src-sentry
  cursor: 'rgba(220, 220, 220, 0.85)',  // src-cursor
  claude: 'rgba(232, 155, 125, 0.95)',  // src-claude
  editor: 'rgba(204, 120, 92, 0.95)',
  canvas: 'rgba(125, 201, 176, 0.95)',  // src-canvas
  terminal: 'rgba(245, 240, 234, 0.85)' // src-term
};

/** Attach a styled drag chip to the event. Call inside an `ondragstart`
 *  handler. The chip is created, set as the drag image, and removed on
 *  the next tick (browsers cache it as a bitmap during dragstart). */
export function attachDragChip(
  e: DragEvent,
  kind: DragChipKind,
  label: string
): void {
  if (!e.dataTransfer || typeof e.dataTransfer.setDragImage !== 'function') return;
  const chip = document.createElement('div');
  chip.style.cssText = [
    'position: fixed',
    'top: -1000px',
    'left: -1000px',
    'display: inline-flex',
    'align-items: center',
    'gap: 8px',
    'padding: 6px 12px 6px 8px',
    'border-radius: 999px',
    'background: rgba(28, 22, 18, 0.96)',
    'border: 1px solid ' + KIND_ACCENT[kind],
    'box-shadow: 0 8px 20px rgba(0, 0, 0, 0.45), 0 0 0 4px rgba(204, 120, 92, 0.10)',
    'color: #f5f0ea',
    'font: 500 12.5px -apple-system, system-ui, sans-serif',
    'pointer-events: none',
    'white-space: nowrap',
    'max-width: 280px',
    'overflow: hidden',
    'text-overflow: ellipsis'
  ].join(';');
  const glyph = document.createElement('span');
  glyph.style.cssText = [
    'display: inline-flex',
    'align-items: center',
    'justify-content: center',
    'width: 18px',
    'height: 18px',
    'border-radius: 4px',
    'background: rgba(245, 240, 234, 0.08)',
    'color: ' + KIND_ACCENT[kind],
    'font: 700 10.5px -apple-system, system-ui, sans-serif'
  ].join(';');
  glyph.textContent = KIND_GLYPH[kind];
  const text = document.createElement('span');
  text.style.cssText = 'overflow: hidden; text-overflow: ellipsis; max-width: 220px;';
  text.textContent = label;
  chip.appendChild(glyph);
  chip.appendChild(text);
  document.body.appendChild(chip);
  e.dataTransfer.setDragImage(chip, 12, 14);
  // Cleanup on next tick — the browser has already snapshotted the bitmap.
  setTimeout(() => chip.remove(), 0);
}
