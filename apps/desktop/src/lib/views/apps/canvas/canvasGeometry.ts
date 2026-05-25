// Pure geometric + blob helpers for CanvasSurface. Extracted in
// wave-1 phase-5 refactor. No reactive store, no DOM observers (the
// FileReader / Image calls are one-shot async helpers).

export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

/** AABB intersection test — used by the marquee-selection sweep to
 *  decide which shapes the rubber-band rectangle touches. */
export function rectIntersects(a: Rect, b: Rect): boolean {
  return !(a.x > b.x + b.w || a.x + a.w < b.x || a.y > b.y + b.h || a.y + a.h < b.y);
}

/** Normalised marquee rect from two raw corner points (either order). */
export function marqueeFromPoints(
  ax: number, ay: number,
  bx: number, by: number
): Rect {
  return {
    x: Math.min(ax, bx),
    y: Math.min(ay, by),
    w: Math.abs(bx - ax),
    h: Math.abs(by - ay),
  };
}

/** MIME / extension check for paste / drop of image content onto the
 *  canvas. Lower-cased name compared against the common bitmap+SVG set. */
export function looksLikeImage(file: { type: string; name: string }): boolean {
  if (file.type.startsWith('image/')) return true;
  const lower = file.name.toLowerCase();
  return /\.(png|jpg|jpeg|gif|webp|svg)$/.test(lower);
}

/** Read a Blob/File as a base64 dataURL. Resolves with `null` on
 *  error so callers can branch silently. */
export function readAsDataUrl(blob: Blob): Promise<string | null> {
  return new Promise((resolve) => {
    const reader = new FileReader();
    reader.onerror = () => resolve(null);
    reader.onload = () => {
      const r = reader.result;
      resolve(typeof r === 'string' ? r : null);
    };
    reader.readAsDataURL(blob);
  });
}

/** Decode a dataURL into intrinsic dimensions so the placed shape
 *  starts at native size (the caller caps to a max so a giant
 *  screenshot doesn't dominate the canvas). Falls back to a sane
 *  default size on decode failure. */
export function intrinsicFromDataUrl(dataUrl: string): Promise<{ w: number; h: number }> {
  return new Promise((resolve) => {
    const img = new Image();
    img.onerror = () => resolve({ w: 320, h: 200 });
    img.onload = () => resolve({ w: img.naturalWidth || 320, h: img.naturalHeight || 200 });
    img.src = dataUrl;
  });
}

/** Smart-guide alignment computation. Given a "lead" rect (the
 *  shape currently being translated) and the rest of the canvas's
 *  shapes ("others"), find the smallest delta needed to snap any
 *  of the lead's anchors (left / center / right horizontally; top /
 *  middle / bottom vertically) to any of the others' anchors. Lines
 *  are post-snap so the renderer doesn't have to recompute. The
 *  picker chooses the BEST match per axis (smallest |delta|) so a
 *  near-tie between two equally good alignments resolves
 *  predictably. */
export function computeAlignment(
  lead: { x: number; y: number; w: number; h: number },
  others: { x: number; y: number; w: number; h: number }[],
  tol: number
): {
  snapDx: number | null;
  snapDy: number | null;
  lines: { vertical: number[]; horizontal: number[] };
} {
  const leadXs = [lead.x, lead.x + lead.w / 2, lead.x + lead.w];
  const leadYs = [lead.y, lead.y + lead.h / 2, lead.y + lead.h];
  let bestDx: number | null = null; let bestDxAbs = Infinity;
  let bestDxLine: number | null = null;
  let bestDy: number | null = null; let bestDyAbs = Infinity;
  let bestDyLine: number | null = null;
  const linesV: number[] = [];
  const linesH: number[] = [];
  for (const o of others) {
    const oXs = [o.x, o.x + o.w / 2, o.x + o.w];
    const oYs = [o.y, o.y + o.h / 2, o.y + o.h];
    for (const lx of leadXs) {
      for (const ox of oXs) {
        const d = ox - lx;
        if (Math.abs(d) > tol) continue;
        if (Math.abs(d) < bestDxAbs) {
          bestDxAbs = Math.abs(d);
          bestDx = d;
          bestDxLine = ox;
        }
      }
    }
    for (const ly of leadYs) {
      for (const oy of oYs) {
        const d = oy - ly;
        if (Math.abs(d) > tol) continue;
        if (Math.abs(d) < bestDyAbs) {
          bestDyAbs = Math.abs(d);
          bestDy = d;
          bestDyLine = oy;
        }
      }
    }
  }
  if (bestDxLine !== null) linesV.push(bestDxLine);
  if (bestDyLine !== null) linesH.push(bestDyLine);
  return {
    snapDx: bestDx,
    snapDy: bestDy,
    lines: { vertical: linesV, horizontal: linesH },
  };
}

/** World position of one of the 9 canonical anchors on a rect. Used
 *  by the edge-drawing tool to pin the edge's source point at the
 *  exact pixel the user grabbed (rather than the shape's center). */
export type CanonicalAnchor = 'tl'|'tc'|'tr'|'ml'|'mc'|'mr'|'bl'|'bc'|'br';
export function anchorWorld(
  shape: { x: number; y: number; w: number; h: number },
  a: CanonicalAnchor
): { x: number; y: number } {
  const cx = shape.x + shape.w / 2;
  const cy = shape.y + shape.h / 2;
  const lx = shape.x;
  const rx = shape.x + shape.w;
  const ty = shape.y;
  const by = shape.y + shape.h;
  switch (a) {
    case 'tl': return { x: lx, y: ty };
    case 'tc': return { x: cx, y: ty };
    case 'tr': return { x: rx, y: ty };
    case 'ml': return { x: lx, y: cy };
    case 'mc': return { x: cx, y: cy };
    case 'mr': return { x: rx, y: cy };
    case 'bl': return { x: lx, y: by };
    case 'bc': return { x: cx, y: by };
    case 'br': return { x: rx, y: by };
  }
}

/** The 8 resize handle ids on a single-shape selection. Order matches
 *  the historical render order in CanvasSurface so existing CSS keeps
 *  matching the right corners. */
export type HandleId = 'tl' | 'tc' | 'tr' | 'ml' | 'mr' | 'bl' | 'bc' | 'br';
export const HANDLE_IDS: HandleId[] = ['tl', 'tc', 'tr', 'ml', 'mr', 'bl', 'bc', 'br'];

/** CSS for one of the 4 edge anchors (`tc`/`mr`/`bc`/`ml`). Counter-
 *  zooms width/height/border so the hit target stays a reasonable
 *  screen size at any camera zoom (`cz`). Used by CanvasSurface to
 *  render the edge-anchor dots on a single-selection box. */
export function anchorStyleCss(
  a: 'tc' | 'mr' | 'bc' | 'ml',
  shape: { x: number; y: number; w: number; h: number },
  cz: number
): string {
  const size = 12 * cz;
  let cx = shape.x;
  let cy = shape.y;
  if (a === 'tc') { cx = shape.x + shape.w / 2; cy = shape.y; }
  if (a === 'mr') { cx = shape.x + shape.w;     cy = shape.y + shape.h / 2; }
  if (a === 'bc') { cx = shape.x + shape.w / 2; cy = shape.y + shape.h; }
  if (a === 'ml') { cx = shape.x;               cy = shape.y + shape.h / 2; }
  return `
      left: ${cx}px;
      top: ${cy}px;
      width: ${size}px;
      height: ${size}px;
      margin-left: ${-size / 2}px;
      margin-top: ${-size / 2}px;
      border-width: ${1.5 * cz}px;
    `;
}

/** CSS for one of the 8 resize handles. Same counter-zoom trick as
 *  `anchorStyleCss`. `box` is the shape's canvas-space bbox. */
export function handleStyleCss(
  handle: HandleId,
  box: { x: number; y: number; w: number; h: number },
  cz: number
): string {
  /* Each handle is positioned in canvas coords; the stage transform
     scales them with the camera. We counter-zoom width/height so the
     hit target stays a reasonable screen size at any zoom. */
  const size = 10 * cz;
  let cx = box.x;
  let cy = box.y;
  if (handle.includes('r')) cx = box.x + box.w;
  if (handle.includes('c') && handle.startsWith('t')) cx = box.x + box.w / 2;
  if (handle.includes('c') && handle.startsWith('b')) cx = box.x + box.w / 2;
  if (handle === 'ml' || handle === 'mr') cy = box.y + box.h / 2;
  if (handle.startsWith('b')) cy = box.y + box.h;
  /* Special-case 'tc' and 'bc' which were already handled above. */
  if (handle === 'tc') { cx = box.x + box.w / 2; cy = box.y; }
  if (handle === 'bc') { cx = box.x + box.w / 2; cy = box.y + box.h; }
  return `
      left: ${cx}px;
      top: ${cy}px;
      width: ${size}px;
      height: ${size}px;
      margin-left: ${-size / 2}px;
      margin-top: ${-size / 2}px;
      border-width: ${1.5 * cz}px;
    `;
}

/** OS cursor name for the given handle. Stays in sync with the
 *  pointer the user expects when grabbing a corner / edge handle. */
export function handleCursor(h: HandleId): string {
  switch (h) {
    case 'tl': case 'br': return 'nwse-resize';
    case 'tr': case 'bl': return 'nesw-resize';
    case 'tc': case 'bc': return 'ns-resize';
    case 'ml': case 'mr': return 'ew-resize';
  }
}

/** Pure resize math — pick the new bbox + grid-snap + flip-negative.
 *  Takes the handle id, the bbox at gesture start, the cursor delta in
 *  canvas-space, the optional grid size (0 = no snap, e.g. ⌘ held).
 *  Returns the four-component bbox the caller patches onto the shape.
 *  The caller is responsible for any shape-internal point-set rescaling
 *  (line endpoints, freehand strokes); this helper only solves the
 *  rectangle math. */
export function computeResize(
  handle: HandleId,
  before: { x: number; y: number; w: number; h: number },
  dx: number,
  dy: number,
  gridSize: number
): { x: number; y: number; w: number; h: number } {
  let nx = before.x;
  let ny = before.y;
  let nw = before.w;
  let nh = before.h;
  /* For each handle, decide which sides move. Diagonal handles move
     two sides; cardinal handles move one. Negative widths are flipped
     at commit so a backward drag past the opposite edge still produces
     a valid bbox. */
  if (handle.includes('l')) { nx = before.x + dx; nw = before.w - dx; }
  if (handle.includes('r')) { nw = before.w + dx; }
  if (handle.startsWith('t')) { ny = before.y + dy; nh = before.h - dy; }
  if (handle.startsWith('b')) { nh = before.h + dy; }

  if (gridSize > 0) {
    /* Snap edges, not deltas. We compute the absolute world edge for
       each side that's moving and snap it. Keeps shapes hugging the
       grid even when the user dragged from a non-aligned start. */
    if (handle.includes('l')) {
      const right = before.x + before.w;
      nx = snap(before.x + dx, gridSize);
      nw = right - nx;
    }
    if (handle.includes('r')) {
      nw = snap(before.x + before.w + dx, gridSize) - before.x;
    }
    if (handle.startsWith('t')) {
      const bottom = before.y + before.h;
      ny = snap(before.y + dy, gridSize);
      nh = bottom - ny;
    }
    if (handle.startsWith('b')) {
      nh = snap(before.y + before.h + dy, gridSize) - before.y;
    }
  }

  /* Flip negative dimensions: dragging a left handle past the right
     edge swaps the rect, like Figma. */
  if (nw < 0) { nx += nw; nw = -nw; }
  if (nh < 0) { ny += nh; nh = -nh; }
  /* Clamp very small shapes to 1px so we don't lose them. */
  nw = Math.max(1, nw);
  nh = Math.max(1, nh);
  return { x: nx, y: ny, w: nw, h: nh };
}

function snap(value: number, grid: number): number {
  return Math.round(value / grid) * grid;
}

/** When the user resizes a shape whose props carry their own
 *  geometry (line/arrow endpoints, freehand stroke samples), the
 *  bbox alone isn't enough — we also need to scale those internal
 *  point sets so the rendered glyph keeps tracking the new box.
 *  Returns a `Partial<Shape>`-shaped patch the caller spreads into
 *  the main bbox patch, OR an empty object for kinds that don't
 *  need point-set rescaling. */
export function rescaleShapePoints(
  shape: { kind: string; props: unknown } | undefined,
  before: { w: number; h: number },
  after: { w: number; h: number }
): { props?: Record<string, unknown> } {
  if (!shape) return {};
  const sx = before.w === 0 ? 1 : after.w / before.w;
  const sy = before.h === 0 ? 1 : after.h / before.h;
  if (shape.kind === 'line' || shape.kind === 'arrow-shape') {
    const props = shape.props as Record<string, unknown>;
    const from = props.from as { x: number; y: number } | undefined;
    const to = props.to as { x: number; y: number } | undefined;
    if (from && to) {
      return {
        props: {
          ...props,
          from: { x: from.x * sx, y: from.y * sy },
          to:   { x: to.x   * sx, y: to.y   * sy },
        },
      };
    }
  } else if (shape.kind === 'freehand') {
    const props = shape.props as Record<string, unknown>;
    const points = props.points;
    if (Array.isArray(points)) {
      return {
        props: {
          ...props,
          points: points.map((pt) => {
            if (!Array.isArray(pt) || pt.length < 2) return pt;
            const [x, y, p] = pt as [number, number, number?];
            return [x * sx, y * sy, p ?? 0.5];
          }),
        },
      };
    }
  }
  return {};
}

/** Whether the active DOM element is something we should not steal
 *  key shortcuts from (text input, textarea, contentEditable). Used
 *  by canvas key handlers to bail out of shortcuts when typing in a
 *  composer field. */
export function isTypingTarget(): boolean {
  const el = document.activeElement;
  if (!el) return false;
  const tag = el.tagName;
  return tag === 'INPUT' || tag === 'TEXTAREA' || (el as HTMLElement).isContentEditable;
}
