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
  file: 'rgba(232, 163, 58, 0.95)',
  dir: 'rgba(232, 163, 58, 0.95)',
  jira: 'rgba(96, 165, 250, 0.95)',
  github: 'rgba(177, 153, 246, 0.95)',
  sentry: 'rgba(248, 143, 116, 0.95)',
  cursor: 'rgba(177, 153, 246, 0.95)',
  claude: 'rgba(232, 163, 58, 0.95)',
  editor: 'rgba(232, 163, 58, 0.95)',
  canvas: 'rgba(232, 163, 58, 0.95)',
  terminal: 'rgba(232, 163, 58, 0.95)'
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
    'box-shadow: 0 8px 20px rgba(0, 0, 0, 0.45), 0 0 0 4px rgba(232, 163, 58, 0.08)',
    'color: #f5f0eb',
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
    'background: rgba(255, 255, 255, 0.08)',
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
