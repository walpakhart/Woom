// Pure type definitions for the Canvas (whiteboard) feature. Lives in
// a plain `.ts` module — no Svelte runes here — so consumers that
// only need the type contract (renderer components, the agent
// system-prompt builder, the screenshot service) can `import type`
// without dragging the reactive runtime into their bundle slice.
//
// The reactive state + mutation API still lives in
// `canvas.svelte.ts`, which re-exports these types verbatim so legacy
// `import { Shape } from '$lib/state/canvas.svelte'` callers keep
// working without changes.

/** Closed catalog of shape kinds. Adding one = code change in this
 *  file + the renderer + the agent tool catalog. Never extend at
 *  runtime. */
export type ShapeKind =
  | 'rect' | 'ellipse' | 'arrow-shape' | 'line' | 'text' | 'sticky'
  | 'mermaid' | 'dot' | 'plantuml' | 'code' | 'image' | 'freehand'
  | 'frame' | 'group'
  | 'jira-card' | 'github-pr-card' | 'github-issue-card'
  | 'sentry-event-card' | 'file-card' | 'chat-message-card';

/** Shape envelope — every shape has a bounding box, a kind, optional
 *  parent (frame / group), and kind-specific `props` whose schema is
 *  enforced by the renderer rather than at type level (we'd need a
 *  discriminated union per kind for that — deferred until shapes
 *  actually render). */
export type Shape = {
  id: string;
  kind: ShapeKind;
  x: number;
  y: number;
  w: number;
  h: number;
  rot: number;
  z: number;
  parentId: string | null;
  locked: boolean;
  hidden: boolean;
  label: string | null;
  props: Record<string, unknown>;
  createdAt: number;
  createdBy: 'user' | 'agent';
  updatedAt: number;
};

/** Endpoint of an Edge. Either snaps to one of nine canonical
 *  anchors on a shape's bbox, or is a free offset (in canvas px)
 *  from the shape's top-left. The free offset is used for arrows
 *  that point at *part* of an image (e.g. "this button in the
 *  screenshot"). */
export type EdgeAnchor =
  | { shapeId: string; anchor: 'tl'|'tc'|'tr'|'ml'|'mc'|'mr'|'bl'|'bc'|'br' }
  | { shapeId: string; offset: { dx: number; dy: number } };

export type Edge = {
  id: string;
  from: EdgeAnchor;
  to: EdgeAnchor;
  kind: 'arrow' | 'line' | 'dashed';
  routing: 'straight' | 'orthogonal' | 'curved';
  label: string | null;
  color: string | null;
  thickness: 1 | 2 | 3;
  z: number;
};

export type CanvasViewport = { x: number; y: number; zoom: number };

export type Canvas = {
  id: string;
  name: string;
  createdAt: number;
  updatedAt: number;
  archivedAt: number | null;
  /** PNG data URL, regenerated on autosave. Null until first
   *  thumbnail pass runs. M-canvas-1 leaves this null since nothing
   *  is drawable. */
  thumbnail: string | null;
  background: 'dot' | 'grid' | 'plain';
  /** Canvas pixels between grid lines (and the snap step). Default
   *  8 to match the design system's 4/8 px scale. */
  gridSize: number;
  viewport: CanvasViewport;
  shapes: Shape[];
  edges: Edge[];
  /** Bumped on every applied operation so the agent can detect
   *  stale reads via `canvas.diff_since(version)`. Starts at 0. */
  version: number;
  /** Schema version of the persisted record. Bumped on breaking
   *  changes; M-canvas-1 ships v1. */
  schemaVersion: 1;
};

/** Compact entry the library overlay reads. Full canvas JSON lives
 *  under the `STORAGE_CANVAS_PREFIX + id` key (or, in a later
 *  milestone, on disk) and is loaded lazily on open. */
export type CanvasIndexEntry = {
  id: string;
  name: string;
  updatedAt: number;
  archivedAt: number | null;
  shapeCount: number;
  thumbnail: string | null;
};

/** Active tool for a Canvas column. Drives pointer-down behavior on
 *  the surface — `select` lets the user pick / move shapes, the rest
 *  start drawing a fresh shape on click+drag. Persisted per
 *  column-instance so switching tabs and coming back keeps the
 *  active tool. */
export type CanvasTool =
  | 'select'
  | 'rect'
  | 'ellipse'
  | 'line'
  | 'arrow'
  | 'text'
  | 'sticky'
  | 'mermaid'
  | 'code'
  | 'image'
  | 'freehand'
  | 'frame';

/** Single mutation. Inverse is computed from the op itself: `add` ⇄
 *  `remove`, `patch.before` undoes `patch.after`. Kept tiny so
 *  history is cheap to hold in memory; one HistoryEntry can carry
 *  many ops (e.g., a multi-select drag commits one entry holding N
 *  patch ops). */
export type Op =
  | { kind: 'add'; shape: Shape }
  | { kind: 'remove'; shape: Shape }
  | {
      kind: 'patch';
      shapeId: string;
      before: Partial<Shape>;
      after: Partial<Shape>;
    }
  | { kind: 'edge-add'; edge: Edge }
  | { kind: 'edge-remove'; edge: Edge }
  | {
      kind: 'edge-patch';
      edgeId: string;
      before: Partial<Edge>;
      after: Partial<Edge>;
    };

export type HistoryEntry = { ops: Op[]; ts: number };

/** Per-column-instance UI state — which canvases the column has
 *  open as tabs, which one is active, and which tool the user
 *  last had selected. Independent from `canvasState.open` so the
 *  same canvas can appear in multiple columns simultaneously. */
export type CanvasInstanceState = {
  tabs: string[];
  activeId: string | null;
  tool: CanvasTool;
};

/** Per-canvas ephemeral state (selection, history). NOT persisted —
 *  a reload starts fresh on selection / undo stack. Lives in a
 *  parallel store rather than on `Canvas` so the persisted JSON
 *  stays small and serialization round-trips cleanly. */
export type CanvasEphemeral = {
  selection: string[];
  history: HistoryEntry[];
  /** Index of the next entry to apply on redo. Entries
   *  `[0..historyIndex)` are "applied"; entries `[historyIndex..)`
   *  are pending redo. A new op truncates everything from
   *  `historyIndex` onward. */
  historyIndex: number;
  /** Shape id the agent's `canvas_focus` tool wants the viewport to
   *  center on. The CanvasColumn watches this via `$effect`,
   *  animates the camera, and clears the field. Null = no pending
   *  focus. `ts` lets the column distinguish back-to-back focus
   *  calls onto the same shape (which would otherwise look like a
   *  no-op). */
  pendingFocus: { shapeId: string; ts: number } | null;
};
