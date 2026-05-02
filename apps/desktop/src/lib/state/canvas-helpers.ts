/* Pure helper functions for the Canvas (whiteboard) feature.
 *
 * No Svelte runtime: factories, geometry, hydration. Lives in a plain
 * `.ts` module so the screenshot service, the agent system-prompt
 * builder, and the renderer can `import { makeShape }` without
 * pulling the reactive store. The `canvas.svelte.ts` module re-
 * exports these so legacy callers keep working.
 *
 * NB: these helpers ALL accept their `Canvas` / `Shape` etc as
 * parameters — they never read or mutate the global `canvasState`.
 * That's the rule that makes them safe to extract here. The
 * mutating operations (addShape, deleteShapes, …) live in
 * `canvas.svelte.ts` next to the reactive store. */

import type {
  Canvas,
  CanvasIndexEntry,
  Edge,
  EdgeAnchor,
  Shape,
  ShapeKind
} from './canvas-types';

// ---- IDs ------------------------------------------------------------------

/** Generate a UUIDv4 string. Falls back to a manual implementation
 *  when `crypto.randomUUID` is unavailable (older WKWebView slices,
 *  test runners). */
export function genId(): string {
  if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
    return crypto.randomUUID();
  }
  return 'xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx'.replace(/[xy]/g, (c) => {
    const r = (Math.random() * 16) | 0;
    const v = c === 'x' ? r : (r & 0x3) | 0x8;
    return v.toString(16);
  });
}

/** Distinct alias for shape ids — same generator, but the call site
 *  reads more clearly. Edges use `genEdgeId` for the same reason. */
export function genShapeId(): string { return genId(); }
export function genEdgeId(): string { return genId(); }

// ---- Canvas factories -----------------------------------------------------

/** A blank canvas at the origin with a 8 px grid and no shapes. Used
 *  by `createCanvas` and the import / restore paths. */
export function blankCanvas(name: string): Canvas {
  const now = Date.now();
  return {
    id: genId(),
    name,
    createdAt: now,
    updatedAt: now,
    archivedAt: null,
    thumbnail: null,
    background: 'dot',
    gridSize: 8,
    viewport: { x: 0, y: 0, zoom: 1 },
    shapes: [],
    edges: [],
    version: 0,
    schemaVersion: 1
  };
}

/** Hydrate a Canvas record with sensible defaults when older
 *  serializations are missing optional fields. Used by both the
 *  localStorage read path and the on-disk read path so additions to
 *  the Canvas type only need one back-fill site. */
export function hydrateCanvas(parsed: Partial<Canvas>): Canvas | null {
  if (typeof parsed.id !== 'string' || typeof parsed.name !== 'string') return null;
  return {
    id: parsed.id,
    name: parsed.name,
    createdAt: parsed.createdAt ?? Date.now(),
    updatedAt: parsed.updatedAt ?? Date.now(),
    archivedAt: parsed.archivedAt ?? null,
    thumbnail: parsed.thumbnail ?? null,
    background: parsed.background ?? 'dot',
    gridSize: parsed.gridSize ?? 8,
    viewport: parsed.viewport ?? { x: 0, y: 0, zoom: 1 },
    shapes: Array.isArray(parsed.shapes) ? parsed.shapes : [],
    edges: Array.isArray(parsed.edges) ? parsed.edges : [],
    version: parsed.version ?? 0,
    schemaVersion: 1
  };
}

/** Project a full canvas down to the index entry stored in the
 *  library overlay. Kept identical between every persistence path
 *  so the index and the per-canvas JSON never disagree. */
export function indexEntryFor(canvas: Canvas): CanvasIndexEntry {
  return {
    id: canvas.id,
    name: canvas.name,
    updatedAt: canvas.updatedAt,
    archivedAt: canvas.archivedAt,
    shapeCount: canvas.shapes.length,
    thumbnail: canvas.thumbnail
  };
}

// ---- Shape factories ------------------------------------------------------

/** Default props per shape kind. The renderer is the source of truth
 *  for `props` schema — these defaults intentionally lean minimal so
 *  a freshly drawn shape is "blank but legible". Themed colors come
 *  from CSS vars at render time, not from props (so theme switches
 *  re-tint everything). */
export function defaultPropsFor(kind: ShapeKind): Record<string, unknown> {
  switch (kind) {
    case 'rect':         return { fill: null, stroke: 'border-hi', strokeWidth: 2, radius: 6 };
    case 'ellipse':      return { fill: null, stroke: 'border-hi', strokeWidth: 2 };
    case 'line':         return { thickness: 2, dash: 'solid' };
    case 'arrow-shape':  return { thickness: 2, head: 'filled' };
    case 'text':         return { text: 'Text', fontSize: 16, fontWeight: 500, align: 'left', color: null };
    case 'sticky':       return { markdown: 'Note…', tint: 'forge', fontSize: 13 };
    /* `theme: 'forge-dark'` is implemented as a custom mermaid theme
       inside CanvasShape so the diagram tints match the warm dark
       palette. Default source is a tiny flowchart so the freshly
       inserted shape isn't blank. */
    case 'mermaid':      return { source: 'flowchart LR\n  A[Start] --> B(Step) --> C{Done?}\n  C -->|yes| D[Ship]\n  C -->|no| B', theme: 'forge-dark' };
    case 'dot':          return { source: 'digraph G { rankdir=LR; A -> B -> C; }', engine: 'dot' };
    /* Code shapes render via CodeMirror 6 (read-only by default;
       M-canvas-3 leaves edit-on-double-click for a follow-up).
       `language` is matched against the lang autoload table in
       `codemirrorLang.ts`. */
    case 'code':         return { source: '// Click to edit\nfunction hello() {\n  console.log("hi");\n}', language: 'javascript', theme: 'one-dark', lineNumbers: true, highlight: [] };
    /* Images carry the asset payload as a base64 data URL inline in
       v0.1 (localStorage-backed canvas state means we have no
       filesystem to point at yet). Kept under a 1 MB pasted-size
       cap to avoid blowing the localStorage quota. Migration to
       `<canvas-id>.assets/<asset-id>.png` lives at the same time as
       canvases move from localStorage to disk JSON files. */
    case 'image':        return { dataUrl: '', intrinsicWidth: 0, intrinsicHeight: 0, alt: null };
    /* `points` is a list of `[x, y, pressure]` in **bbox-local**
       canvas px. Bbox is the AABB of the points, recomputed on
       commit. The renderer turns this into an SVG path via
       `perfect-freehand`. */
    case 'freehand':     return { points: [], color: 'text-0', thickness: 2, smoothing: 0.5 };
    /* Frame: a labeled rectangle that acts as a container. `title`
       renders in the top-bar; `tint` (null for now) overrides the
       border color when set. Children get their `parentId` set to
       the frame's id via `canvas_group` or by drag-into-frame. */
    case 'frame':        return { title: 'Frame', tint: null };
    case 'group':        return {};
    /* Forge live cards: store the LOOKUP KEY (ticketKey, repo+number,
       etc.) + a SNAPSHOT of the source object captured at drop time.
       Renderer prefers live data from inbox state when available;
       falls back to the snapshot when no column has the object loaded
       (e.g., user closed the source column). Snapshot is intentionally
       small — title + status, not full body — so a 50-card canvas
       doesn't bloat localStorage. */
    case 'jira-card':           return { ticketKey: '', snapshot: null };
    case 'github-pr-card':      return { owner: '', repo: '', number: 0, snapshot: null };
    case 'github-issue-card':   return { owner: '', repo: '', number: 0, snapshot: null };
    case 'sentry-event-card':   return { projectSlug: '', issueId: '', shortId: null, snapshot: null };
    case 'file-card':           return { repoRoot: null, relPath: '', isDir: false };
    case 'chat-message-card':   return { sessionId: '', messageIndex: 0, snapshot: null };
    default:             return {};
  }
}

/** Build a fresh shape at `(x, y)` with `(w, h)` size. `kind` picks
 *  the default-props recipe; caller can patch fields after via
 *  `updateShape`. */
export function makeShape(args: {
  kind: ShapeKind;
  x: number;
  y: number;
  w: number;
  h: number;
  createdBy?: 'user' | 'agent';
  label?: string | null;
  props?: Record<string, unknown>;
}): Shape {
  const now = Date.now();
  return {
    id: genShapeId(),
    kind: args.kind,
    x: args.x,
    y: args.y,
    w: Math.max(1, args.w),
    h: Math.max(1, args.h),
    rot: 0,
    z: now,
    parentId: null,
    locked: false,
    hidden: false,
    label: args.label ?? null,
    props: { ...defaultPropsFor(args.kind), ...(args.props ?? {}) },
    createdAt: now,
    createdBy: args.createdBy ?? 'user',
    updatedAt: now
  };
}

// ---- Edge factories -------------------------------------------------------

/** Build a fresh Edge with sensible defaults. The agent's MCP tool
 *  and the user's drag-from-anchor gesture both go through this —
 *  keeping one place to change defaults (`'arrow'`, orthogonal
 *  routing, 2 px). */
export function makeEdge(args: {
  from: EdgeAnchor;
  to: EdgeAnchor;
  kind?: 'arrow' | 'line' | 'dashed';
  routing?: 'straight' | 'orthogonal' | 'curved';
  label?: string | null;
  color?: string | null;
  thickness?: 1 | 2 | 3;
}): Edge {
  return {
    id: genEdgeId(),
    from: args.from,
    to: args.to,
    kind: args.kind ?? 'arrow',
    routing: args.routing ?? 'orthogonal',
    label: args.label ?? null,
    color: args.color ?? null,
    thickness: args.thickness ?? 2,
    z: Date.now()
  };
}

// ---- Geometry -------------------------------------------------------------

/** Round `v` to the nearest multiple of `step`. Used for grid-snap
 *  during translate / resize. `step <= 0` short-circuits to no-op
 *  so callers can flip snapping off without an extra branch. */
export function snapToGrid(v: number, step: number): number {
  if (step <= 0) return v;
  return Math.round(v / step) * step;
}

/** Resolve an EdgeAnchor to the world-coord point it represents.
 *  Returns null when the anchored shape is missing — the renderer
 *  treats null as "ghost edge, draw with a warning glyph or
 *  skip". */
export function getEdgeEndpoint(canvas: Canvas, anchor: EdgeAnchor): { x: number; y: number } | null {
  const shape = canvas.shapes.find((s) => s.id === anchor.shapeId);
  if (!shape) return null;
  if ('offset' in anchor) {
    return { x: shape.x + anchor.offset.dx, y: shape.y + anchor.offset.dy };
  }
  /* Canonical anchors. Origin is the shape's top-left;
     left/top mid/right × top/middle/bottom = 9 anchor points. */
  const cx = shape.x + shape.w / 2;
  const cy = shape.y + shape.h / 2;
  const lx = shape.x;
  const rx = shape.x + shape.w;
  const ty = shape.y;
  const by = shape.y + shape.h;
  switch (anchor.anchor) {
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

/** Pick the canonical anchor on `shape` whose world position is
 *  nearest to `point`. Used when an edge gesture lands on a shape
 *  body — we snap to the closest of its 9 anchors instead of the
 *  click point. */
export function nearestAnchor(
  shape: Shape,
  point: { x: number; y: number }
): 'tl'|'tc'|'tr'|'ml'|'mc'|'mr'|'bl'|'bc'|'br' {
  const candidates: Array<['tl'|'tc'|'tr'|'ml'|'mc'|'mr'|'bl'|'bc'|'br', number, number]> = [
    ['tl', shape.x, shape.y],
    ['tc', shape.x + shape.w / 2, shape.y],
    ['tr', shape.x + shape.w, shape.y],
    ['ml', shape.x, shape.y + shape.h / 2],
    ['mc', shape.x + shape.w / 2, shape.y + shape.h / 2],
    ['mr', shape.x + shape.w, shape.y + shape.h / 2],
    ['bl', shape.x, shape.y + shape.h],
    ['bc', shape.x + shape.w / 2, shape.y + shape.h],
    ['br', shape.x + shape.w, shape.y + shape.h]
  ];
  let best: typeof candidates[number] = candidates[4];
  let bestDist = Infinity;
  for (const c of candidates) {
    const dx = c[1] - point.x;
    const dy = c[2] - point.y;
    const d = dx * dx + dy * dy;
    if (d < bestDist) { bestDist = d; best = c; }
  }
  return best[0];
}

// ---- Hierarchy ------------------------------------------------------------

/** All shapes whose `parentId` chain reaches `shapeId`. Used by the
 *  drag-translate gesture to make frames into proper containers —
 *  when the user drags a frame, its children follow. Transitive: a
 *  frame inside a frame collects children of children. Cycle-safe
 *  via a visited set so a corrupt save with a parent-cycle doesn't
 *  loop. */
export function getDescendants(canvas: Canvas, shapeId: string): Shape[] {
  const out: Shape[] = [];
  const visited = new Set<string>([shapeId]);
  const queue: string[] = [shapeId];
  while (queue.length > 0) {
    const id = queue.shift()!;
    for (const s of canvas.shapes) {
      if (s.parentId === id && !visited.has(s.id)) {
        visited.add(s.id);
        out.push(s);
        queue.push(s.id);
      }
    }
  }
  return out;
}
