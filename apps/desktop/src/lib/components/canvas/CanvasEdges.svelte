<script lang="ts">
  // CanvasEdges — one SVG layer that renders every edge of a canvas in
  // canvas-pixel coordinates. Sits inside the stage transform so pan/zoom
  // moves edges with shapes.
  //
  // Per edge we render TWO paths:
  //   1. A wide invisible "hit" path with `pointer-events: stroke` so
  //      clicking anywhere within ~10 canvas px of the edge selects it.
  //   2. A visible thin path with `pointer-events: none` for the actual
  //      stroke + arrowhead.
  //
  // Endpoint resolution and routing math live in helpers below. Edges
  // whose endpoints can't be resolved (one shape was deleted) skip
  // rendering — keeping them around in the data so undo restores them
  // alongside the shape, but they don't litter the surface.

  import type { Canvas, Edge, EdgeAnchor, Shape } from '$lib/state/canvas.svelte';
  import { getEdgeEndpoint } from '$lib/state/canvas.svelte';

  interface Props {
    canvas: Canvas;
    selectedEdgeIds: string[];
    /** Camera zoom for counter-zoomed stroke widths so edges stay
     *  legible regardless of how far in/out the user is. */
    zoom: number;
  }

  let { canvas, selectedEdgeIds, zoom }: Props = $props();

  /* Counter-zoom factor so the visible stroke stays close to N CSS px
     on screen (with vector-effect=non-scaling-stroke as backup). */
  const cz = $derived(1 / Math.max(zoom, 0.0001));

  /* Render-time decoration: which side of the bbox an anchor faces.
     Used to seed the orthogonal/curved routes so the line leaves the
     shape perpendicular to the edge it touches. */
  function anchorDirection(a: EdgeAnchor):
    'l' | 'r' | 't' | 'b' | 'c' {
    if ('offset' in a) return 'c';
    switch (a.anchor) {
      case 'tl': case 'ml': case 'bl': return 'l';
      case 'tr': case 'mr': case 'br': return 'r';
      case 'tc':                       return 't';
      case 'bc':                       return 'b';
      default:                         return 'c';
    }
  }

  function straightPath(a: { x: number; y: number }, b: { x: number; y: number }): string {
    return `M ${a.x.toFixed(2)} ${a.y.toFixed(2)} L ${b.x.toFixed(2)} ${b.y.toFixed(2)}`;
  }

  /** Manhattan routing — two-segment elbow when the anchor directions
   *  agree on an axis, three-segment Z when they conflict. Not a full
   *  A* with bbox avoidance (saves the implementation cost) but produces
   *  clean elbows for the common cases (LR / TB flowcharts) which is
   *  90% of what users want. */
  function orthogonalPath(
    a: { x: number; y: number; dir: 'l' | 'r' | 't' | 'b' | 'c' },
    b: { x: number; y: number; dir: 'l' | 'r' | 't' | 'b' | 'c' }
  ): string {
    const ax = a.x, ay = a.y, bx = b.x, by = b.y;
    /* Pick the bend axis based on the source's exit direction. */
    const verticalSource = a.dir === 't' || a.dir === 'b';
    const horizontalSource = a.dir === 'l' || a.dir === 'r';
    if (horizontalSource) {
      const midX = (ax + bx) / 2;
      return `M ${ax.toFixed(2)} ${ay.toFixed(2)} L ${midX.toFixed(2)} ${ay.toFixed(2)} L ${midX.toFixed(2)} ${by.toFixed(2)} L ${bx.toFixed(2)} ${by.toFixed(2)}`;
    }
    if (verticalSource) {
      const midY = (ay + by) / 2;
      return `M ${ax.toFixed(2)} ${ay.toFixed(2)} L ${ax.toFixed(2)} ${midY.toFixed(2)} L ${bx.toFixed(2)} ${midY.toFixed(2)} L ${bx.toFixed(2)} ${by.toFixed(2)}`;
    }
    /* Center / unknown — fall back to a midpoint elbow on the dominant
       axis. */
    const dx = Math.abs(bx - ax);
    const dy = Math.abs(by - ay);
    if (dx > dy) {
      const midX = (ax + bx) / 2;
      return `M ${ax.toFixed(2)} ${ay.toFixed(2)} L ${midX.toFixed(2)} ${ay.toFixed(2)} L ${midX.toFixed(2)} ${by.toFixed(2)} L ${bx.toFixed(2)} ${by.toFixed(2)}`;
    } else {
      const midY = (ay + by) / 2;
      return `M ${ax.toFixed(2)} ${ay.toFixed(2)} L ${ax.toFixed(2)} ${midY.toFixed(2)} L ${bx.toFixed(2)} ${midY.toFixed(2)} L ${bx.toFixed(2)} ${by.toFixed(2)}`;
    }
  }

  /** Cubic bezier whose control points push out perpendicular to each
   *  endpoint's anchor direction. The result reads as a smooth, oriented
   *  flow even when the endpoints are close. */
  function curvedPath(
    a: { x: number; y: number; dir: 'l' | 'r' | 't' | 'b' | 'c' },
    b: { x: number; y: number; dir: 'l' | 'r' | 't' | 'b' | 'c' }
  ): string {
    const dx = b.x - a.x;
    const dy = b.y - a.y;
    const dist = Math.sqrt(dx * dx + dy * dy);
    /* Control-point reach scales with distance, capped at 120 so very
       long edges don't S-curve into the next county. */
    const reach = Math.min(120, Math.max(40, dist * 0.4));

    function offset(d: 'l' | 'r' | 't' | 'b' | 'c'): { dx: number; dy: number } {
      switch (d) {
        case 'l': return { dx: -reach, dy: 0 };
        case 'r': return { dx:  reach, dy: 0 };
        case 't': return { dx: 0, dy: -reach };
        case 'b': return { dx: 0, dy:  reach };
        case 'c': return { dx: dx * 0.4, dy: dy * 0.4 };
      }
    }
    const oa = offset(a.dir);
    /* Target direction is the OPPOSITE of `b.dir` so the curve
       arrives "into" the anchor, not past it. */
    const ob = offset(b.dir);
    const c1x = a.x + oa.dx;
    const c1y = a.y + oa.dy;
    const c2x = b.x - ob.dx;
    const c2y = b.y - ob.dy;
    return `M ${a.x.toFixed(2)} ${a.y.toFixed(2)} C ${c1x.toFixed(2)} ${c1y.toFixed(2)}, ${c2x.toFixed(2)} ${c2y.toFixed(2)}, ${b.x.toFixed(2)} ${b.y.toFixed(2)}`;
  }

  /** Resolve an edge to a render record. Returns null when an endpoint's
   *  shape is gone — the edge is held in storage but skipped here. */
  type EdgeRender = {
    edge: Edge;
    a: { x: number; y: number; dir: 'l' | 'r' | 't' | 'b' | 'c' };
    b: { x: number; y: number; dir: 'l' | 'r' | 't' | 'b' | 'c' };
    pathD: string;
    selected: boolean;
  };

  function buildRender(canvas: Canvas, edge: Edge, selected: boolean): EdgeRender | null {
    const aPt = getEdgeEndpoint(canvas, edge.from);
    const bPt = getEdgeEndpoint(canvas, edge.to);
    if (!aPt || !bPt) return null;
    const a = { ...aPt, dir: anchorDirection(edge.from) };
    const b = { ...bPt, dir: anchorDirection(edge.to) };
    let pathD: string;
    switch (edge.routing) {
      case 'straight':    pathD = straightPath(a, b); break;
      case 'curved':      pathD = curvedPath(a, b); break;
      case 'orthogonal':
      default:            pathD = orthogonalPath(a, b); break;
    }
    return { edge, a, b, pathD, selected };
  }

  const renders = $derived.by<EdgeRender[]>(() => {
    const out: EdgeRender[] = [];
    for (const e of canvas.edges) {
      const r = buildRender(canvas, e, selectedEdgeIds.includes(e.id));
      if (r) out.push(r);
    }
    return out;
  });

  /* Theme — tied to the dark palette. We deliberately don't read
     edge.color here so users get consistent default visuals; per-edge
     color override is post-v1. */
  const STROKE_COLOR = 'var(--text-1)';
  const STROKE_COLOR_SELECTED = 'var(--accent)';
</script>

<svg
  class="cv-edges"
  width="100%" height="100%"
  overflow="visible"
  preserveAspectRatio="none"
>
  <defs>
    <!-- One marker per arrow color so selected vs unselected arrows
         keep their head color in sync with the stroke. -->
    <marker
      id="cv-arrow-default"
      viewBox="0 0 10 10"
      refX="8" refY="5"
      markerWidth="6" markerHeight="6"
      orient="auto-start-reverse"
    >
      <path d="M 0 0 L 10 5 L 0 10 z" fill={STROKE_COLOR} />
    </marker>
    <marker
      id="cv-arrow-selected"
      viewBox="0 0 10 10"
      refX="8" refY="5"
      markerWidth="6" markerHeight="6"
      orient="auto-start-reverse"
    >
      <path d="M 0 0 L 10 5 L 0 10 z" fill={STROKE_COLOR_SELECTED} />
    </marker>
  </defs>

  {#each renders as r (r.edge.id)}
    <!-- Wide invisible hit path. Gives the user a generous click target
         while the visible path can stay 1.5 px thin. -->
    <path
      class="cv-edge-hit"
      data-edge-id={r.edge.id}
      d={r.pathD}
      stroke="transparent"
      stroke-width={Math.max(12, 12 * cz)}
      fill="none"
      pointer-events="stroke"
    />
    <path
      class="cv-edge"
      d={r.pathD}
      stroke={r.selected ? STROKE_COLOR_SELECTED : STROKE_COLOR}
      stroke-width={(r.selected ? 2 : r.edge.thickness) * cz}
      stroke-dasharray={r.edge.kind === 'dashed' ? `${6 * cz} ${4 * cz}` : undefined}
      fill="none"
      stroke-linecap="round"
      stroke-linejoin="round"
      marker-end={r.edge.kind === 'arrow'
        ? (r.selected ? 'url(#cv-arrow-selected)' : 'url(#cv-arrow-default)')
        : undefined}
      pointer-events="none"
      vector-effect="non-scaling-stroke"
    />
    {#if r.edge.label}
      <text
        x={(r.a.x + r.b.x) / 2}
        y={(r.a.y + r.b.y) / 2}
        text-anchor="middle"
        dominant-baseline="middle"
        fill="var(--text-0)"
        font-size={11 * cz}
        style="paint-order: stroke; stroke: var(--bg-0); stroke-width: {3 * cz}px; pointer-events: none;"
      >{r.edge.label}</text>
    {/if}
  {/each}
</svg>

<style>
  .cv-edges {
    position: absolute;
    top: 0;
    left: 0;
    /* SVG itself is "0 size" — children paint outside via overflow:visible.
       Keeps it from intercepting pointer events outside the actual paths. */
    width: 0;
    height: 0;
    pointer-events: none;
  }
  .cv-edge-hit { cursor: pointer; pointer-events: stroke; }
  .cv-edge { transition: stroke-width 80ms; }
</style>
