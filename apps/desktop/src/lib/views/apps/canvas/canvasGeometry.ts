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
