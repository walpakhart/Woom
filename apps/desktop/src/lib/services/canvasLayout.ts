// Canvas layout helpers — see [docs/CANVAS.md §7](../../../../docs/CANVAS.md).
// One layout call = one history entry: we collect every shape's before/after
// (x, y) and commit through `commitTransientPatches` so undo restores the
// pre-layout positions in a single ⌘Z.

import {
  ensureCanvasLoaded,
  commitTransientPatches,
  snapToGrid,
  type Canvas,
  type Shape
} from '$lib/state/canvas.svelte';

export type LayoutAlgorithm = 'grid' | 'row' | 'column' | 'dagre';

export type GridOpts = { cols?: number; gap?: number };
export type RowOpts = { gap?: number; align?: 'top' | 'center' | 'bottom' };
export type ColumnOpts = { gap?: number; align?: 'left' | 'center' | 'right' };
export type DagreOpts = {
  /** Top-to-bottom or left-to-right. LR makes flowcharts read like English;
   *  TB is what most architecture diagrams want. */
  rankdir?: 'TB' | 'LR' | 'BT' | 'RL';
  /** Px between adjacent layers. */
  ranksep?: number;
  /** Px between siblings in the same layer. */
  nodesep?: number;
};

/** Pick the subset of canvas shapes the layout will operate on. Empty
 *  `ids` means "every root-level shape". Falsy / unknown ids are filtered
 *  out so a stale agent call doesn't crash. */
function resolveTargets(canvas: Canvas, ids: string[] | undefined): Shape[] {
  if (!ids || ids.length === 0) {
    return canvas.shapes.filter((s) => s.parentId === null);
  }
  const set = new Set(ids);
  return canvas.shapes.filter((s) => set.has(s.id));
}

/** Compute the AABB of `shapes`. Used so layouts anchor to "where the
 *  selection currently is" instead of teleporting to the canvas origin. */
function aabb(shapes: Shape[]): { x: number; y: number; w: number; h: number } {
  if (shapes.length === 0) return { x: 0, y: 0, w: 0, h: 0 };
  let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
  for (const s of shapes) {
    if (s.x < minX) minX = s.x;
    if (s.y < minY) minY = s.y;
    if (s.x + s.w > maxX) maxX = s.x + s.w;
    if (s.y + s.h > maxY) maxY = s.y + s.h;
  }
  return { x: minX, y: minY, w: maxX - minX, h: maxY - minY };
}

/** Snap final positions to the canvas grid (so layouts stay aligned with
 *  the dot pattern). Op-batches every change as one atomic history entry. */
function commitMoves(
  canvas: Canvas,
  moves: Array<{ shape: Shape; x: number; y: number }>
) {
  const g = canvas.gridSize;
  const changes = moves
    .map(({ shape, x, y }) => ({
      shapeId: shape.id,
      before: { x: shape.x, y: shape.y },
      after: { x: snapToGrid(x, g), y: snapToGrid(y, g) }
    }))
    /* Skip identity moves so a re-run of the same layout doesn't
       pollute history. */
    .filter((ch) => ch.before.x !== ch.after.x || ch.before.y !== ch.after.y);
  if (changes.length === 0) return;
  /* Apply the new positions in-memory immediately; commitTransientPatches
     handles the undo recording. */
  for (const ch of changes) {
    const s = canvas.shapes.find((x) => x.id === ch.shapeId);
    if (s) { s.x = ch.after.x as number; s.y = ch.after.y as number; }
  }
  commitTransientPatches(canvas.id, changes);
}

/** Lay shapes in a `cols`-wide grid, ordered by their current `z` (so
 *  the earliest-created shape is top-left). Cell size is the largest
 *  shape; each shape sits centered inside its cell. */
export function applyGrid(canvasId: string, ids?: string[], opts?: GridOpts) {
  const canvas = ensureCanvasLoaded(canvasId);
  if (!canvas) return;
  const targets = resolveTargets(canvas, ids).slice().sort((a, b) => a.z - b.z);
  if (targets.length === 0) return;
  const gap = opts?.gap ?? 24;
  const cols = Math.max(1, opts?.cols ?? Math.ceil(Math.sqrt(targets.length)));
  const cellW = Math.max(...targets.map((s) => s.w));
  const cellH = Math.max(...targets.map((s) => s.h));
  const start = aabb(targets);
  const moves: Array<{ shape: Shape; x: number; y: number }> = [];
  for (let i = 0; i < targets.length; i++) {
    const r = Math.floor(i / cols);
    const c = i % cols;
    const cx = start.x + c * (cellW + gap) + (cellW - targets[i].w) / 2;
    const cy = start.y + r * (cellH + gap) + (cellH - targets[i].h) / 2;
    moves.push({ shape: targets[i], x: cx, y: cy });
  }
  commitMoves(canvas, moves);
}

/** One horizontal line, sorted by current x. `align` controls where each
 *  shape's bbox sits vertically relative to the row's y-baseline. */
export function applyRow(canvasId: string, ids?: string[], opts?: RowOpts) {
  const canvas = ensureCanvasLoaded(canvasId);
  if (!canvas) return;
  const targets = resolveTargets(canvas, ids).slice().sort((a, b) => a.x - b.x);
  if (targets.length === 0) return;
  const gap = opts?.gap ?? 24;
  const align = opts?.align ?? 'center';
  const start = aabb(targets);
  const rowMaxH = Math.max(...targets.map((s) => s.h));
  const moves: Array<{ shape: Shape; x: number; y: number }> = [];
  let cursor = start.x;
  for (const s of targets) {
    let y = start.y;
    if (align === 'center') y = start.y + (rowMaxH - s.h) / 2;
    if (align === 'bottom') y = start.y + (rowMaxH - s.h);
    moves.push({ shape: s, x: cursor, y });
    cursor += s.w + gap;
  }
  commitMoves(canvas, moves);
}

/** One vertical line, sorted by current y. */
export function applyColumn(canvasId: string, ids?: string[], opts?: ColumnOpts) {
  const canvas = ensureCanvasLoaded(canvasId);
  if (!canvas) return;
  const targets = resolveTargets(canvas, ids).slice().sort((a, b) => a.y - b.y);
  if (targets.length === 0) return;
  const gap = opts?.gap ?? 24;
  const align = opts?.align ?? 'center';
  const start = aabb(targets);
  const colMaxW = Math.max(...targets.map((s) => s.w));
  const moves: Array<{ shape: Shape; x: number; y: number }> = [];
  let cursor = start.y;
  for (const s of targets) {
    let x = start.x;
    if (align === 'center') x = start.x + (colMaxW - s.w) / 2;
    if (align === 'right') x = start.x + (colMaxW - s.w);
    moves.push({ shape: s, x, y: cursor });
    cursor += s.h + gap;
  }
  commitMoves(canvas, moves);
}

/** DAG layered layout via d3-dag's `dagre` exposed graph. Targets are the
 *  union of the shapes in `ids` (or all root shapes when `ids` is empty)
 *  AND the shapes referenced by their connecting edges (so the DAG is
 *  closed under reachability inside the selection). Shapes with no
 *  connecting edge get appended in a final row at the bottom so they
 *  don't fall off the layout entirely. */
export async function applyDagre(
  canvasId: string,
  ids?: string[],
  opts?: DagreOpts
) {
  const canvas = ensureCanvasLoaded(canvasId);
  if (!canvas) return;
  const initialTargets = resolveTargets(canvas, ids);
  if (initialTargets.length === 0) return;
  const targetSet = new Set(initialTargets.map((s) => s.id));
  /* Pull in shapes that connect to the selection — keeps the DAG
     contiguous so the layout doesn't truncate edges mid-graph. */
  const subgraphEdges = canvas.edges.filter(
    (e) => targetSet.has(e.from.shapeId) && targetSet.has(e.to.shapeId)
  );
  const targets = canvas.shapes.filter((s) => targetSet.has(s.id));
  if (targets.length === 0) return;

  /* Lazy-import d3-dag — it's a 50KB chunk we don't need until a layout
     is actually requested. */
  const { dagre } = await import('d3-dag');

  const grf = new dagre.graphlib.Graph();
  grf.setGraph({
    rankdir: opts?.rankdir ?? 'LR',
    ranksep: opts?.ranksep ?? 60,
    nodesep: opts?.nodesep ?? 40
  });
  grf.setDefaultEdgeLabel(() => ({}));
  for (const s of targets) {
    grf.setNode(s.id, { width: s.w, height: s.h });
  }
  for (const e of subgraphEdges) {
    grf.setEdge(e.from.shapeId, e.to.shapeId);
  }
  dagre.layout(grf);

  const start = aabb(targets);
  const moves: Array<{ shape: Shape; x: number; y: number }> = [];
  /* d3-dag returns center coords; we anchor the layout at the original
     selection's top-left so the user doesn't lose the visual context.
     Disconnected shapes (no incoming/outgoing edges in the subgraph)
     get pushed to a row below the laid-out cluster. */
  let layoutMinX = Infinity, layoutMinY = Infinity;
  const positioned: Array<{ shape: Shape; x: number; y: number }> = [];
  const orphans: Shape[] = [];
  for (const s of targets) {
    const pos = grf.node(s.id);
    if (!pos || typeof pos.x !== 'number') {
      orphans.push(s);
      continue;
    }
    /* `pos.x`/`pos.y` are CENTER coords; convert to top-left. */
    const x = pos.x - s.w / 2;
    const y = pos.y - s.h / 2;
    if (x < layoutMinX) layoutMinX = x;
    if (y < layoutMinY) layoutMinY = y;
    positioned.push({ shape: s, x, y });
  }
  for (const p of positioned) {
    moves.push({
      shape: p.shape,
      x: p.x - layoutMinX + start.x,
      y: p.y - layoutMinY + start.y
    });
  }
  /* Orphans: drop them in a row at the bottom of the laid-out cluster. */
  if (orphans.length > 0) {
    const layoutH = Math.max(0, ...positioned.map((p) => p.y - layoutMinY + p.shape.h));
    let cursor = start.x;
    for (const s of orphans) {
      moves.push({ shape: s, x: cursor, y: start.y + layoutH + 24 });
      cursor += s.w + 24;
    }
  }
  commitMoves(canvas, moves);
}

/** Dispatcher that the toolbar / agent both call. The caller picks an
 *  algorithm; we call the right helper. Awaitable because dagre is
 *  lazy-imported; the others resolve synchronously. */
export async function applyLayout(
  canvasId: string,
  algorithm: LayoutAlgorithm,
  ids?: string[],
  opts?: GridOpts | RowOpts | ColumnOpts | DagreOpts
) {
  switch (algorithm) {
    case 'grid':   applyGrid(canvasId, ids, opts as GridOpts); break;
    case 'row':    applyRow(canvasId, ids, opts as RowOpts); break;
    case 'column': applyColumn(canvasId, ids, opts as ColumnOpts); break;
    case 'dagre':  await applyDagre(canvasId, ids, opts as DagreOpts); break;
  }
}
