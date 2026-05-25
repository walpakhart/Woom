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
