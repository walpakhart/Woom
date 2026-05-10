<script lang="ts">
  // Minimap — small overview rendered in the bottom-right corner of the
  // canvas surface. Shows every shape as a tiny rect (kind-tinted) plus
  // a brighter rect for the current camera viewport. Click anywhere to
  // teleport the camera; click+drag the viewport rect (or any point)
  // to pan continuously.
  //
  // The map rescales to fit the union of:
  //   - the AABB of all shapes,
  //   - the current viewport rect.
  // Including the viewport rect in the bounds means the minimap always
  // shows the user "where am I" even when the camera is panned far off
  // every shape (otherwise the view rect would clip out of frame).
  //
  // Coords:
  //   - Canvas px in the parent stage's coordinate system.
  //   - We compute a single uniform scale factor that fits the
  //     content+viewport into the minimap's size.
  //   - Mouse interactions translate minimap-px → canvas-px and call
  //     the parent's `onTeleport` callback so the camera can sync.

  import type { Shape } from '$lib/state/canvas.svelte';

  interface Props {
    shapes: Shape[];
    /** Canvas-px viewport rect: top-left + width/height. Comes from
     *  the parent's camera derivation (camX, camY, surface size /
     *  zoom). The minimap renders this rect as a brighter outline so
     *  the user can see what they're looking at. */
    viewport: { x: number; y: number; w: number; h: number };
    /** Called when the user clicks/drags inside the minimap. The
     *  caller centers the camera on `(canvasX, canvasY)`. */
    onTeleport: (canvasX: number, canvasY: number) => void;
    /** Click-X handler so the user can collapse the minimap when it
     *  occludes content they want to see. */
    onClose: () => void;
  }

  let { shapes, viewport, onTeleport, onClose }: Props = $props();

  /* Map dimensions in CSS px — picked so the map is informative but
     compact. Aspect ratio 5:3 fits most solo layouts. */
  const MAP_W = 200;
  const MAP_H = 120;

  /* Computed bounding box (canvas px) we map onto the visible area.
     Pad to avoid the content sitting flush against the edges, and
     fall back to the viewport rect when there are zero shapes (so
     the user still sees a meaningful map on a fresh canvas). */
  const bounds = $derived.by(() => {
    let minX = viewport.x, minY = viewport.y;
    let maxX = viewport.x + viewport.w, maxY = viewport.y + viewport.h;
    for (const s of shapes) {
      if (s.x < minX) minX = s.x;
      if (s.y < minY) minY = s.y;
      if (s.x + s.w > maxX) maxX = s.x + s.w;
      if (s.y + s.h > maxY) maxY = s.y + s.h;
    }
    const pad = 40;
    return {
      x: minX - pad,
      y: minY - pad,
      w: Math.max(1, maxX - minX + pad * 2),
      h: Math.max(1, maxY - minY + pad * 2)
    };
  });

  /* Single uniform scale — pick the smaller of width/height ratios so
     content fits in both axes. Centering offsets fill the unused
     space along the other axis. */
  const layout = $derived.by(() => {
    const sx = MAP_W / bounds.w;
    const sy = MAP_H / bounds.h;
    const scale = Math.min(sx, sy);
    const drawW = bounds.w * scale;
    const drawH = bounds.h * scale;
    const offX = (MAP_W - drawW) / 2;
    const offY = (MAP_H - drawH) / 2;
    return { scale, offX, offY };
  });

  function canvasToMap(cx: number, cy: number): { x: number; y: number } {
    return {
      x: layout.offX + (cx - bounds.x) * layout.scale,
      y: layout.offY + (cy - bounds.y) * layout.scale
    };
  }

  function mapToCanvas(mx: number, my: number): { x: number; y: number } {
    return {
      x: bounds.x + (mx - layout.offX) / layout.scale,
      y: bounds.y + (my - layout.offY) / layout.scale
    };
  }

  /* Mouse → teleport + drag. We capture pointermove on the surface
     while a drag is active so the user can scrub continuously. */
  let mapEl = $state<HTMLDivElement | null>(null);
  let dragging = $state(false);

  function teleportFromEvent(e: PointerEvent) {
    if (!mapEl) return;
    const rect = mapEl.getBoundingClientRect();
    const mx = e.clientX - rect.left;
    const my = e.clientY - rect.top;
    const { x, y } = mapToCanvas(mx, my);
    onTeleport(x, y);
  }

  function onPointerDown(e: PointerEvent) {
    if (e.button !== 0) return;
    e.preventDefault();
    e.stopPropagation();
    mapEl?.setPointerCapture(e.pointerId);
    dragging = true;
    teleportFromEvent(e);
  }
  function onPointerMove(e: PointerEvent) {
    if (!dragging) return;
    e.preventDefault();
    e.stopPropagation();
    teleportFromEvent(e);
  }
  function onPointerUp(e: PointerEvent) {
    if (!dragging) return;
    try { mapEl?.releasePointerCapture(e.pointerId); } catch { /* ignore */ }
    dragging = false;
  }

  /* Per-kind tints for shape dots. Mirrors the live-card stripe colors
     so the minimap reads as "where the orange / violet things live". */
  function tintFor(kind: string): string {
    switch (kind) {
      case 'jira-card':           return '#2684FF';
      case 'github-pr-card':
      case 'github-issue-card':   return '#8B5CF6';
      case 'sentry-event-card':   return '#F88F74';
      case 'file-card':           return '#E8A33A';
      case 'chat-message-card':   return '#D97757';
      case 'mermaid':
      case 'dot':                 return '#0ea5e9';
      case 'code':                return '#a855f7';
      case 'image':               return '#A8D9B8';
      case 'sticky':              return 'rgba(232, 130, 100, 0.85)';
      case 'frame':
      case 'group':               return 'rgba(255, 255, 255, 0.18)';
      default:                    return 'rgba(255, 255, 255, 0.45)';
    }
  }

  /* Render-time computation of every shape's minimap rect. Cheap —
     O(N) per shape, runs only when shapes / bounds / viewport change.
     For a 2k-shape canvas this is still <1ms per render. */
  const dots = $derived.by(() => {
    return shapes.map((s) => {
      const tl = canvasToMap(s.x, s.y);
      const w = Math.max(1, s.w * layout.scale);
      const h = Math.max(1, s.h * layout.scale);
      return { id: s.id, x: tl.x, y: tl.y, w, h, color: tintFor(s.kind) };
    });
  });

  const vpRect = $derived.by(() => {
    const tl = canvasToMap(viewport.x, viewport.y);
    return {
      x: tl.x,
      y: tl.y,
      w: Math.max(2, viewport.w * layout.scale),
      h: Math.max(2, viewport.h * layout.scale)
    };
  });
</script>

<!-- Stop pointerdown / wheel from bubbling to the canvas surface.
     The minimap is an overlay; clicks on it shouldn't kick off a
     surface gesture (marquee / draw / pan), and scrolling it
     shouldn't zoom the canvas. -->
<div
  class="cv-minimap"
  role="region"
  aria-label="Canvas minimap"
  onpointerdown={(e) => e.stopPropagation()}
  onwheel={(e) => e.stopPropagation()}
>
  <button class="cv-minimap-close" onclick={onClose} aria-label="Hide minimap" title="Hide minimap (M)">
    <svg viewBox="0 0 24 24" width="10" height="10"><path d="M18 6 6 18M6 6l12 12" stroke="currentColor" stroke-width="2" stroke-linecap="round" fill="none"/></svg>
  </button>
  <div
    class="cv-minimap-surface"
    bind:this={mapEl}
    onpointerdown={onPointerDown}
    onpointermove={onPointerMove}
    onpointerup={onPointerUp}
    onpointercancel={onPointerUp}
    role="presentation"
    style="width: {MAP_W}px; height: {MAP_H}px;"
  >
    <svg width={MAP_W} height={MAP_H} class="cv-minimap-svg" overflow="hidden">
      {#each dots as d (d.id)}
        <rect
          x={d.x.toFixed(2)}
          y={d.y.toFixed(2)}
          width={d.w.toFixed(2)}
          height={d.h.toFixed(2)}
          fill={d.color}
          rx="1"
        />
      {/each}
      <rect
        class="cv-minimap-vp"
        x={vpRect.x.toFixed(2)}
        y={vpRect.y.toFixed(2)}
        width={vpRect.w.toFixed(2)}
        height={vpRect.h.toFixed(2)}
        fill="rgba(232, 130, 100, 0.10)"
        stroke="var(--accent)"
        stroke-width="1.2"
        rx="2"
      />
    </svg>
  </div>
</div>

<style>
  .cv-minimap {
    position: absolute;
    bottom: 38px; /* clear the canvas-status footer */
    right: 14px;
    background: rgba(10, 8, 6, 0.78);
    border: 1px solid var(--border-neutral);
    border-radius: 8px;
    padding: 4px;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.4);
    backdrop-filter: blur(6px);
    z-index: 5;
    user-select: none;
  }
  .cv-minimap-close {
    position: absolute;
    top: -8px;
    right: -8px;
    width: 18px;
    height: 18px;
    padding: 0;
    background: var(--bg-1);
    border: 1px solid var(--border-neutral);
    color: var(--text-2);
    border-radius: 50%;
    cursor: pointer;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    box-shadow: 0 2px 6px rgba(0, 0, 0, 0.45);
    z-index: 6;
  }
  .cv-minimap-close:hover {
    background: var(--bg-2);
    color: var(--text-0);
    border-color: var(--accent);
  }
  .cv-minimap-surface {
    cursor: crosshair;
    border-radius: 5px;
    overflow: hidden;
    background: rgba(255, 255, 255, 0.03);
    background-image:
      linear-gradient(to right, rgba(255,255,255,0.04) 1px, transparent 1px),
      linear-gradient(to bottom, rgba(255,255,255,0.04) 1px, transparent 1px);
    background-size: 20px 20px;
  }
  .cv-minimap-svg {
    display: block;
    pointer-events: none;
  }
  .cv-minimap-vp {
    pointer-events: none;
  }
</style>
