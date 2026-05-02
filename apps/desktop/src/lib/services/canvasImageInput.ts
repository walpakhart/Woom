/* Canvas image-input primitives.
 *
 * The canvas accepts images via paste, drop, and a file picker.
 * Common to all three paths is reading the bytes as a base64 dataURL
 * and decoding the intrinsic dimensions so the placed shape lands at
 * native size (capped to a max). These helpers are pure DOM reads
 * (FileReader, HTMLImageElement) — no Svelte runes, no canvas state.
 *
 * The 1.5MB cap is a stop-gap until canvas storage moves to disk
 * (see CANVAS.md §11.1). After that lands these helpers can be
 * untouched; the upstream caller in CanvasColumn just stops checking
 * the size. */

/** Soft ceiling for an inline-base64 image. Above this we reject the
 *  paste rather than blow up localStorage with a single screenshot. */
export const MAX_IMAGE_BYTES = 1_500_000;

/** Heuristic check: does this File / drop entry look like an image we
 *  can render? MIME-type first (most reliable), with an extension
 *  fallback for screenshots dragged from Finder where MIME comes
 *  through as `application/octet-stream`. */
export function looksLikeImage(file: { type: string; name: string }): boolean {
  if (file.type.startsWith('image/')) return true;
  const lower = file.name.toLowerCase();
  return /\.(png|jpg|jpeg|gif|webp|svg)$/.test(lower);
}

/** Read a Blob/File as base64 dataURL. Resolves with `null` on error
 *  so callers can branch silently. */
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
 *  starts at native size. Falls back to 320×200 if decoding fails
 *  (broken SVG / corrupt data). */
export function intrinsicFromDataUrl(dataUrl: string): Promise<{ w: number; h: number }> {
  return new Promise((resolve) => {
    const img = new Image();
    img.onerror = () => resolve({ w: 320, h: 200 });
    img.onload = () => resolve({ w: img.naturalWidth || 320, h: img.naturalHeight || 200 });
    img.src = dataUrl;
  });
}

/** Compute the on-canvas dimensions for an image with intrinsic
 *  size `w × h`, capped at `maxDim` on the longer axis. Aspect ratio
 *  is preserved. Used by drop / paste / file-picker insertion paths. */
export function fitImageDimensions(
  w: number,
  h: number,
  maxDim: number = 480
): { w: number; h: number } {
  if (w <= maxDim && h <= maxDim) return { w, h };
  const k = Math.min(maxDim / w, maxDim / h);
  return { w: Math.round(w * k), h: Math.round(h * k) };
}
