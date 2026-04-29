// Canvas (whiteboard) state — see [docs/CANVAS.md](../../../../docs/CANVAS.md).
// M-canvas-1 scope: types, library index, per-instance active canvas + tabs,
// viewport. Shapes / edges are typed but the renderer doesn't draw them yet
// (that's M-canvas-2 onward). Persistence is localStorage for now; filesystem
// JSON files arrive in M-canvas-3 alongside asset storage.

// ---- Types ---------------------------------------------------------------

/** Closed catalog of shape kinds. Adding one = code change in this file +
 *  the renderer + the agent tool catalog. Never extend at runtime. */
export type ShapeKind =
  | 'rect' | 'ellipse' | 'arrow-shape' | 'line' | 'text' | 'sticky'
  | 'mermaid' | 'dot' | 'plantuml' | 'code' | 'image' | 'freehand'
  | 'frame' | 'group'
  | 'jira-card' | 'github-pr-card' | 'github-issue-card'
  | 'sentry-event-card' | 'file-card' | 'chat-message-card';

/** Shape envelope — every shape has a bounding box, a kind, optional parent
 *  (frame / group), and kind-specific `props` whose schema is enforced by the
 *  renderer rather than at type level (we'd need a discriminated union per
 *  kind for that — deferred until shapes actually render). */
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

/** Endpoint of an Edge. Either snaps to one of nine canonical anchors on a
 *  shape's bbox, or is a free offset (in canvas px) from the shape's
 *  top-left. The free offset is used for arrows that point at *part* of an
 *  image (e.g. "this button in the screenshot"). */
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
  /** PNG data URL, regenerated on autosave. Null until first thumbnail
   *  pass runs. M-canvas-1 leaves this null since nothing is drawable. */
  thumbnail: string | null;
  background: 'dot' | 'grid' | 'plain';
  /** Canvas pixels between grid lines (and the snap step). Default 8 to
   *  match the design system's 4/8 px scale. */
  gridSize: number;
  viewport: CanvasViewport;
  shapes: Shape[];
  edges: Edge[];
  /** Bumped on every applied operation so the agent can detect stale reads
   *  via `canvas.diff_since(version)`. Starts at 0. */
  version: number;
  /** Schema version of the persisted record. Bumped on breaking changes;
   *  M-canvas-1 ships v1. */
  schemaVersion: 1;
};

/** Compact entry the library overlay reads. Full canvas JSON lives under
 *  the `STORAGE_CANVAS_PREFIX + id` key (or, in a later milestone, on disk)
 *  and is loaded lazily on open. */
export type CanvasIndexEntry = {
  id: string;
  name: string;
  updatedAt: number;
  archivedAt: number | null;
  shapeCount: number;
  thumbnail: string | null;
};

// ---- Tool kinds ----------------------------------------------------------

/** Active tool for a Canvas column. Drives pointer-down behavior on the
 *  surface — `select` lets the user pick / move shapes, the rest start
 *  drawing a fresh shape on click+drag. Persisted per column-instance so
 *  switching tabs and coming back keeps the active tool. */
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

// ---- Op log / history ----------------------------------------------------

/** Single mutation. Inverse is computed from the op itself: `add` ⇄ `remove`,
 *  `patch.before` undoes `patch.after`. Kept tiny so history is cheap to
 *  hold in memory; one HistoryEntry can carry many ops (e.g., a multi-select
 *  drag commits one entry holding N patch ops). */
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

/** History bound — older entries discarded so a long-running session
 *  doesn't accumulate unbounded memory. Matches the 50-op cap mentioned
 *  in [docs/CANVAS.md §11.3](../../../../docs/CANVAS.md). */
const HISTORY_CAP = 50;

// ---- Storage keys --------------------------------------------------------

const STORAGE_INDEX = 'forgehold:canvas:index:v1';
const STORAGE_CANVAS_PREFIX = 'forgehold:canvas:v1:';
const STORAGE_INSTANCE_STATE = 'forgehold:canvas:instances:v1';

// ---- Helpers -------------------------------------------------------------

function genId(): string {
  if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
    return crypto.randomUUID();
  }
  return 'xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx'.replace(/[xy]/g, (c) => {
    const r = (Math.random() * 16) | 0;
    const v = c === 'x' ? r : (r & 0x3) | 0x8;
    return v.toString(16);
  });
}

function blankCanvas(name: string): Canvas {
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

// ---- Reactive state ------------------------------------------------------

/** Per-column-instance state. `tabs` is the ordered list of canvas ids
 *  pinned to this column (the strip of tabs in the header). `activeId`
 *  is whichever tab is currently rendered. Both reset to empty when the
 *  column is created and survive workbench-tab switches because they're
 *  keyed on `instanceId`, not on the workbench. */
export type CanvasInstanceState = {
  tabs: string[];
  activeId: string | null;
  /** Currently selected drawing tool. Defaults to `'select'` on a fresh
   *  column. Persisted so re-opening a column lands in the tool the user
   *  left it in. */
  tool: CanvasTool;
};

/** Per-canvas ephemeral state (selection, history). NOT persisted — a
 *  reload starts fresh on selection / undo stack. Lives in a parallel
 *  store rather than on `Canvas` so the persisted JSON stays small and
 *  serialization round-trips cleanly. */
export type CanvasEphemeral = {
  selection: string[];
  history: HistoryEntry[];
  /** Index of the next entry to apply on redo. Entries `[0..historyIndex)`
   *  are "applied"; entries `[historyIndex..)` are pending redo. A new op
   *  truncates everything from `historyIndex` onward. */
  historyIndex: number;
  /** Shape id the agent's `canvas_focus` tool wants the viewport to
   *  center on. The CanvasColumn watches this via `$effect`, animates
   *  the camera, and clears the field. Null = no pending focus.
   *  `ts` lets the column distinguish back-to-back focus calls onto
   *  the same shape (which would otherwise look like a no-op). */
  pendingFocus: { shapeId: string; ts: number } | null;
};

export const canvasState = $state<{
  index: CanvasIndexEntry[];
  /** Loaded canvases, keyed by id. Lazily populated on open. */
  open: Record<string, Canvas>;
  /** Per-column-instance active canvas + tab strip. Keyed on column
   *  instance id so a Canvas column's tab state survives moving the
   *  column to another workbench (mirrors how `editorInstanceState`
   *  works for editor columns). */
  byInstance: Record<string, CanvasInstanceState>;
  /** Per-canvas ephemeral state (selection + undo stack). Created on
   *  open, dropped on close. */
  ephemeral: Record<string, CanvasEphemeral>;
}>({
  index: [],
  open: {},
  byInstance: {},
  ephemeral: {}
});

function ensureEphemeral(canvasId: string): CanvasEphemeral {
  let e = canvasState.ephemeral[canvasId];
  if (!e) {
    e = { selection: [], history: [], historyIndex: 0, pendingFocus: null };
    canvasState.ephemeral[canvasId] = e;
  } else if (!('pendingFocus' in e)) {
    /* Migration for ephemeral states created before the field landed. */
    (e as CanvasEphemeral).pendingFocus = null;
  }
  return e;
}

/** Ask the column rendering this canvas to pan the camera onto a shape.
 *  Set by the `canvas_focus` MCP tool dispatcher; consumed by the
 *  CanvasColumn's $effect, which animates the camera and clears the
 *  field. Safe to call when no column is currently rendering the
 *  canvas — the request just sits there until one mounts. */
export function requestCanvasFocus(canvasId: string, shapeId: string) {
  const eph = ensureEphemeral(canvasId);
  eph.pendingFocus = { shapeId, ts: Date.now() };
}

// ---- Persistence ---------------------------------------------------------

function persistIndex() {
  try {
    localStorage.setItem(STORAGE_INDEX, JSON.stringify({ entries: canvasState.index }));
  } catch {
    /* quota / SSR — ignore */
  }
}

function persistInstanceState() {
  try {
    localStorage.setItem(STORAGE_INSTANCE_STATE, JSON.stringify(canvasState.byInstance));
  } catch { /* ignore */ }
}

function persistCanvas(canvas: Canvas) {
  try {
    localStorage.setItem(STORAGE_CANVAS_PREFIX + canvas.id, JSON.stringify(canvas));
  } catch { /* ignore */ }
}

function readCanvasFromStorage(id: string): Canvas | null {
  try {
    const raw = localStorage.getItem(STORAGE_CANVAS_PREFIX + id);
    if (!raw) return null;
    const parsed = JSON.parse(raw) as Partial<Canvas>;
    // Basic shape check + back-fill defaults so older records hydrate safely
    // when we add fields later. We only validate the must-haves here.
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
  } catch {
    return null;
  }
}

/** One-shot: hydrate `canvasState` from localStorage. Called from the
 *  app root layout (alongside `restorePanelState`). Idempotent — calling
 *  twice is safe and replaces the in-memory state with whatever's on disk. */
export function restoreCanvasState() {
  try {
    const raw = localStorage.getItem(STORAGE_INDEX);
    if (raw) {
      const parsed = JSON.parse(raw) as { entries?: CanvasIndexEntry[] };
      if (Array.isArray(parsed.entries)) {
        canvasState.index = parsed.entries.filter(
          (e): e is CanvasIndexEntry =>
            !!e && typeof e.id === 'string' && typeof e.name === 'string'
        );
      }
    }
  } catch { /* ignore */ }

  try {
    const raw = localStorage.getItem(STORAGE_INSTANCE_STATE);
    if (raw) {
      const parsed = JSON.parse(raw) as Record<string, CanvasInstanceState>;
      const cleaned: Record<string, CanvasInstanceState> = {};
      const VALID_TOOLS: CanvasTool[] = [
        'select', 'rect', 'ellipse', 'line', 'arrow', 'text', 'sticky',
        'mermaid', 'code', 'image', 'freehand', 'frame'
      ];
      for (const [k, v] of Object.entries(parsed ?? {})) {
        if (!v || !Array.isArray(v.tabs)) continue;
        cleaned[k] = {
          tabs: v.tabs.filter((t) => typeof t === 'string'),
          activeId: typeof v.activeId === 'string' ? v.activeId : null,
          tool:
            typeof v.tool === 'string' && (VALID_TOOLS as string[]).includes(v.tool)
              ? (v.tool as CanvasTool)
              : 'select'
        };
      }
      canvasState.byInstance = cleaned;
    }
  } catch { /* ignore */ }
}

// ---- Library operations --------------------------------------------------

function indexEntryFor(canvas: Canvas): CanvasIndexEntry {
  return {
    id: canvas.id,
    name: canvas.name,
    updatedAt: canvas.updatedAt,
    archivedAt: canvas.archivedAt,
    shapeCount: canvas.shapes.length,
    thumbnail: canvas.thumbnail
  };
}

function upsertIndexEntry(entry: CanvasIndexEntry) {
  const i = canvasState.index.findIndex((e) => e.id === entry.id);
  if (i >= 0) canvasState.index[i] = entry;
  else canvasState.index = [...canvasState.index, entry];
  persistIndex();
}

/** Create a new blank canvas, persist, return the id. */
export function createCanvas(name = 'Untitled'): string {
  const c = blankCanvas(name);
  canvasState.open[c.id] = c;
  upsertIndexEntry(indexEntryFor(c));
  persistCanvas(c);
  return c.id;
}

/** Hydrate a canvas into the in-memory store if not already loaded.
 *  Returns the canvas, or null if the id is unknown / corrupt. */
export function ensureCanvasLoaded(id: string): Canvas | null {
  if (canvasState.open[id]) return canvasState.open[id];
  const fromStorage = readCanvasFromStorage(id);
  if (!fromStorage) return null;
  canvasState.open[id] = fromStorage;
  return fromStorage;
}

export function renameCanvas(id: string, name: string) {
  const c = ensureCanvasLoaded(id);
  if (!c) return;
  const trimmed = name.trim();
  if (!trimmed) return;
  c.name = trimmed;
  c.updatedAt = Date.now();
  upsertIndexEntry(indexEntryFor(c));
  persistCanvas(c);
}

export function archiveCanvas(id: string) {
  const c = ensureCanvasLoaded(id);
  if (!c) return;
  c.archivedAt = Date.now();
  c.updatedAt = c.archivedAt;
  upsertIndexEntry(indexEntryFor(c));
  persistCanvas(c);
  // Detach from any column tabs.
  for (const inst of Object.values(canvasState.byInstance)) {
    inst.tabs = inst.tabs.filter((t) => t !== id);
    if (inst.activeId === id) inst.activeId = inst.tabs[0] ?? null;
  }
  persistInstanceState();
}

export function unarchiveCanvas(id: string) {
  const c = ensureCanvasLoaded(id);
  if (!c) return;
  c.archivedAt = null;
  c.updatedAt = Date.now();
  upsertIndexEntry(indexEntryFor(c));
  persistCanvas(c);
}

/** Deep-clone an existing canvas — new uuid for the canvas AND every
 *  shape / edge inside it (so undo histories don't accidentally cross
 *  over). Suffixes the name with " copy" if not already. Persists, then
 *  returns the new id so callers can open it in a column. */
export function duplicateCanvas(id: string): string | null {
  const src = ensureCanvasLoaded(id);
  if (!src) return null;
  const newName = /\bcopy\b/i.test(src.name) ? `${src.name} copy` : `${src.name} copy`;
  /* genId via existing helper; can't reach `genId` here so we use the
     same UUID approach inline. */
  const cloneId = (typeof crypto !== 'undefined' && crypto.randomUUID)
    ? crypto.randomUUID()
    : `${Date.now()}-${Math.random().toString(16).slice(2)}`;
  /* Map old shape ids → new shape ids so we can rewrite edge endpoints. */
  const idMap = new Map<string, string>();
  const newShapes = src.shapes.map((s) => {
    const nid = (typeof crypto !== 'undefined' && crypto.randomUUID)
      ? crypto.randomUUID()
      : `${Date.now()}-${Math.random().toString(16).slice(2)}`;
    idMap.set(s.id, nid);
    return { ...s, id: nid, createdAt: Date.now(), updatedAt: Date.now() };
  });
  const newEdges = src.edges
    .map((e) => {
      const fromId = idMap.get(e.from.shapeId);
      const toId = idMap.get(e.to.shapeId);
      if (!fromId || !toId) return null;
      const newEid = (typeof crypto !== 'undefined' && crypto.randomUUID)
        ? crypto.randomUUID()
        : `${Date.now()}-${Math.random().toString(16).slice(2)}`;
      return {
        ...e,
        id: newEid,
        from: { ...e.from, shapeId: fromId },
        to: { ...e.to, shapeId: toId }
      };
    })
    .filter((e): e is NonNullable<typeof e> => !!e);
  const now = Date.now();
  const clone: Canvas = {
    ...src,
    id: cloneId,
    name: newName,
    createdAt: now,
    updatedAt: now,
    archivedAt: null,
    shapes: newShapes,
    edges: newEdges,
    version: 0
  };
  canvasState.open[clone.id] = clone;
  upsertIndexEntry(indexEntryFor(clone));
  persistCanvas(clone);
  return clone.id;
}

/** Serialize a canvas to a portable JSON blob. Strips persistence-only
 *  fields (`thumbnail`) to keep the export small and stable. */
export function exportCanvasJson(id: string): string | null {
  const c = ensureCanvasLoaded(id);
  if (!c) return null;
  const { thumbnail, ...rest } = c;
  return JSON.stringify(rest, null, 2);
}

/** Hydrate a canvas from a JSON blob produced by `exportCanvasJson`.
 *  Always mints a fresh id (avoids collisions when importing into the
 *  same workspace), preserves shapes/edges, persists, returns the new id.
 *  Returns null on parse error / shape mismatch. */
export function importCanvasJson(json: string): string | null {
  let parsed: Partial<Canvas>;
  try { parsed = JSON.parse(json); } catch { return null; }
  if (!parsed || typeof parsed !== 'object') return null;
  if (!Array.isArray(parsed.shapes) || !Array.isArray(parsed.edges)) return null;
  const newId = (typeof crypto !== 'undefined' && crypto.randomUUID)
    ? crypto.randomUUID()
    : `${Date.now()}-${Math.random().toString(16).slice(2)}`;
  const now = Date.now();
  const c: Canvas = {
    id: newId,
    name: typeof parsed.name === 'string' && parsed.name ? `${parsed.name} (imported)` : 'Imported',
    createdAt: now,
    updatedAt: now,
    archivedAt: null,
    thumbnail: null,
    background: parsed.background === 'grid' || parsed.background === 'plain' ? parsed.background : 'dot',
    gridSize: typeof parsed.gridSize === 'number' ? parsed.gridSize : 8,
    viewport: parsed.viewport ?? { x: 0, y: 0, zoom: 1 },
    shapes: parsed.shapes,
    edges: parsed.edges,
    version: 0,
    schemaVersion: 1
  };
  canvasState.open[c.id] = c;
  upsertIndexEntry(indexEntryFor(c));
  persistCanvas(c);
  return c.id;
}

export function deleteCanvas(id: string) {
  canvasState.index = canvasState.index.filter((e) => e.id !== id);
  delete canvasState.open[id];
  for (const inst of Object.values(canvasState.byInstance)) {
    inst.tabs = inst.tabs.filter((t) => t !== id);
    if (inst.activeId === id) inst.activeId = inst.tabs[0] ?? null;
  }
  try { localStorage.removeItem(STORAGE_CANVAS_PREFIX + id); } catch { /* ignore */ }
  persistIndex();
  persistInstanceState();
}

// ---- Per-column-instance ops --------------------------------------------

function ensureInstanceState(instanceId: string): CanvasInstanceState {
  let s = canvasState.byInstance[instanceId];
  if (!s) {
    s = { tabs: [], activeId: null, tool: 'select' };
    canvasState.byInstance[instanceId] = s;
  } else if (!s.tool) {
    /* Migration for users with v0 instance state (pre-tool field). */
    s.tool = 'select';
  }
  return s;
}

/** Set the active tool for a column instance. Persisted. */
export function setTool(instanceId: string, tool: CanvasTool) {
  const s = ensureInstanceState(instanceId);
  s.tool = tool;
  persistInstanceState();
}

/** Pin a canvas to a column's tab strip and activate it. Idempotent on the
 *  pin (won't add the same id twice) but always re-activates so clicking
 *  a library tile while it's already pinned still focuses it. */
export function openCanvasInInstance(instanceId: string, canvasId: string) {
  ensureCanvasLoaded(canvasId);
  const s = ensureInstanceState(instanceId);
  if (!s.tabs.includes(canvasId)) s.tabs = [...s.tabs, canvasId];
  s.activeId = canvasId;
  persistInstanceState();
}

/** Create a fresh canvas, pin it to the column, activate it. Returns the
 *  new canvas id. Used by the column header's "+" button. */
export function createAndOpenInInstance(instanceId: string, name?: string): string {
  const id = createCanvas(name ?? 'Untitled');
  openCanvasInInstance(instanceId, id);
  return id;
}

/** Detach a tab from a column without touching the library. The canvas
 *  itself stays available for re-opening from the library overlay. */
export function closeCanvasTab(instanceId: string, canvasId: string) {
  const s = canvasState.byInstance[instanceId];
  if (!s) return;
  s.tabs = s.tabs.filter((t) => t !== canvasId);
  if (s.activeId === canvasId) s.activeId = s.tabs[0] ?? null;
  persistInstanceState();
}

export function setActiveCanvasTab(instanceId: string, canvasId: string) {
  const s = ensureInstanceState(instanceId);
  if (!s.tabs.includes(canvasId)) s.tabs = [...s.tabs, canvasId];
  s.activeId = canvasId;
  ensureCanvasLoaded(canvasId);
  persistInstanceState();
}

// ---- Viewport ------------------------------------------------------------

/** Update a canvas's viewport. Debounced persistence happens here too;
 *  pan/zoom fire many times per second so we coalesce writes. */
let viewportPersistTimer: ReturnType<typeof setTimeout> | null = null;
export function setViewport(canvasId: string, vp: CanvasViewport) {
  const c = canvasState.open[canvasId];
  if (!c) return;
  c.viewport = vp;
  c.updatedAt = Date.now();
  if (viewportPersistTimer) clearTimeout(viewportPersistTimer);
  viewportPersistTimer = setTimeout(() => {
    persistCanvas(c);
    upsertIndexEntry(indexEntryFor(c));
  }, 600);
}

// ---- Cleanup hook --------------------------------------------------------

/** Drop a column instance's tab strip when the instance is removed. Wired
 *  in app bootstrap via `registerInstanceRemovedHook`. Does NOT delete the
 *  canvases themselves — they remain in the library so a later canvas
 *  column can reopen them. */
export function dropCanvasInstance(instanceId: string) {
  if (!canvasState.byInstance[instanceId]) return;
  delete canvasState.byInstance[instanceId];
  persistInstanceState();
}

// ---- Snap & geometry helpers --------------------------------------------

/** Round `v` to the nearest multiple of `step`. Used for grid-snap during
 *  translate / resize. `step <= 0` short-circuits to no-op so callers can
 *  flip snapping off without an extra branch. */
export function snapToGrid(v: number, step: number): number {
  if (step <= 0) return v;
  return Math.round(v / step) * step;
}

/** Persist the canvas's `updatedAt` + index entry. Debounced so a chatty
 *  drag-resize doesn't write to localStorage on every frame. */
let canvasPersistTimer: ReturnType<typeof setTimeout> | null = null;
function schedulePersist(canvas: Canvas) {
  canvas.updatedAt = Date.now();
  if (canvasPersistTimer) clearTimeout(canvasPersistTimer);
  canvasPersistTimer = setTimeout(() => {
    persistCanvas(canvas);
    upsertIndexEntry(indexEntryFor(canvas));
  }, 600);
}

// ---- Shape factories -----------------------------------------------------

function genShapeId(): string { return genId(); }

/** Default props per shape kind. The renderer is the source of truth for
 *  `props` schema — these defaults intentionally lean minimal so a freshly
 *  drawn shape is "blank but legible". Themed colors come from CSS vars at
 *  render time, not from props (so theme switches re-tint everything). */
function defaultPropsFor(kind: ShapeKind): Record<string, unknown> {
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
    /* Code shapes render via CodeMirror 6 (read-only by default; M-canvas-3
       leaves edit-on-double-click for a follow-up). `language` is matched
       against the lang autoload table in [`codemirrorLang.ts`](../components/editor/codemirrorLang.ts). */
    case 'code':         return { source: '// Click to edit\nfunction hello() {\n  console.log("hi");\n}', language: 'javascript', theme: 'one-dark', lineNumbers: true, highlight: [] };
    /* Images carry the asset payload as a base64 data URL inline in
       v0.1 (localStorage-backed canvas state means we have no
       filesystem to point at yet). Kept under a 1 MB pasted-size cap
       to avoid blowing the localStorage quota. Migration to
       `<canvas-id>.assets/<asset-id>.png` lives at the same time as
       canvases move from localStorage to disk JSON files. */
    case 'image':        return { dataUrl: '', intrinsicWidth: 0, intrinsicHeight: 0, alt: null };
    /* `points` is a list of `[x, y, pressure]` in **bbox-local** canvas
       px. Bbox is the AABB of the points, recomputed on commit. The
       renderer turns this into an SVG path via `perfect-freehand`. */
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

/** Build a fresh shape at `(x, y)` with `(w, h)` size. `kind` picks the
 *  default-props recipe; caller can patch fields after via `updateShape`. */
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

// ---- Op application ------------------------------------------------------

function applyOp(canvas: Canvas, op: Op) {
  switch (op.kind) {
    case 'add':
      canvas.shapes = [...canvas.shapes, op.shape];
      break;
    case 'remove':
      canvas.shapes = canvas.shapes.filter((s) => s.id !== op.shape.id);
      /* Cascade: any edge whose endpoint references the removed shape
         loses its anchor. Spec keeps such edges as dashed ghosts; our
         renderer skips them. We don't delete them here so undo restores
         both shape and its edges in one step. */
      break;
    case 'patch': {
      const i = canvas.shapes.findIndex((s) => s.id === op.shapeId);
      if (i < 0) break;
      canvas.shapes[i] = { ...canvas.shapes[i], ...op.after, updatedAt: Date.now() };
      break;
    }
    case 'edge-add':
      canvas.edges = [...canvas.edges, op.edge];
      break;
    case 'edge-remove':
      canvas.edges = canvas.edges.filter((e) => e.id !== op.edge.id);
      break;
    case 'edge-patch': {
      const i = canvas.edges.findIndex((e) => e.id === op.edgeId);
      if (i < 0) break;
      canvas.edges[i] = { ...canvas.edges[i], ...op.after };
      break;
    }
  }
}

function invertOp(op: Op): Op {
  switch (op.kind) {
    case 'add':         return { kind: 'remove', shape: op.shape };
    case 'remove':      return { kind: 'add', shape: op.shape };
    case 'patch':       return { kind: 'patch', shapeId: op.shapeId, before: op.after, after: op.before };
    case 'edge-add':    return { kind: 'edge-remove', edge: op.edge };
    case 'edge-remove': return { kind: 'edge-add', edge: op.edge };
    case 'edge-patch':  return { kind: 'edge-patch', edgeId: op.edgeId, before: op.after, after: op.before };
  }
}

/** Apply a batch of ops + push them as a single history entry. Truncates
 *  any pending redo. Called by every mutation in this module. Single
 *  source of truth for `version` and history bookkeeping. */
function commitOps(canvas: Canvas, ops: Op[]) {
  if (ops.length === 0) return;
  for (const op of ops) applyOp(canvas, op);
  canvas.version += 1;
  const eph = ensureEphemeral(canvas.id);
  /* Truncate redo branch — pushing a new entry past an undo discards the
     forward history, like every editor since 1985. */
  if (eph.historyIndex < eph.history.length) {
    eph.history = eph.history.slice(0, eph.historyIndex);
  }
  eph.history = [...eph.history, { ops, ts: Date.now() }];
  if (eph.history.length > HISTORY_CAP) {
    /* Trim from the front; older history is gone but the canvas state is
       still consistent (the trimmed ops are already applied). */
    eph.history = eph.history.slice(eph.history.length - HISTORY_CAP);
  }
  eph.historyIndex = eph.history.length;
  schedulePersist(canvas);
}

// ---- Public mutation API -------------------------------------------------

/** Add a shape to the canvas. Returns the shape id. */
export function addShape(canvasId: string, shape: Shape): string | null {
  const c = ensureCanvasLoaded(canvasId);
  if (!c) return null;
  commitOps(c, [{ kind: 'add', shape }]);
  return shape.id;
}

/** Add many shapes atomically (single history entry). Returns the new ids
 *  in order. */
export function addShapes(canvasId: string, shapes: Shape[]): string[] {
  const c = ensureCanvasLoaded(canvasId);
  if (!c) return [];
  commitOps(c, shapes.map((shape) => ({ kind: 'add', shape })));
  return shapes.map((s) => s.id);
}

/** Patch a shape with a partial. Records `before` from the live shape so
 *  undo works without the caller having to remember the prior values.
 *  Pass `transient: true` from gesture handlers (drag-translate, drag-resize)
 *  to mutate the shape in place WITHOUT pushing history; commit a single
 *  history entry on gesture-release via `commitTransientPatch`. */
export function patchShape(
  canvasId: string,
  shapeId: string,
  patch: Partial<Shape>,
  opts?: { transient?: boolean }
) {
  const c = ensureCanvasLoaded(canvasId);
  if (!c) return;
  const shape = c.shapes.find((s) => s.id === shapeId);
  if (!shape || shape.locked) return;
  if (opts?.transient) {
    Object.assign(shape, patch, { updatedAt: Date.now() });
    return;
  }
  const before: Partial<Shape> = {};
  for (const key of Object.keys(patch) as (keyof Shape)[]) {
    (before as Record<string, unknown>)[key] = (shape as Record<string, unknown>)[key];
  }
  commitOps(c, [{ kind: 'patch', shapeId, before, after: patch }]);
}

/** Push a single history entry covering many shape patches with their
 *  `before` snapshots. Designed for gesture-release: during the drag,
 *  shapes were mutated transiently with `patchShape({ transient: true })`;
 *  the caller remembers the original state and hands it here. */
export function commitTransientPatches(
  canvasId: string,
  changes: Array<{ shapeId: string; before: Partial<Shape>; after: Partial<Shape> }>
) {
  const c = ensureCanvasLoaded(canvasId);
  if (!c || changes.length === 0) return;
  /* Skip no-op gestures (click without drag). Compares each change's
     before/after on the same key set; if every value matches we don't
     pollute history. */
  const meaningful = changes.filter((ch) => {
    for (const key of Object.keys(ch.after)) {
      if ((ch.before as Record<string, unknown>)[key] !==
          (ch.after as Record<string, unknown>)[key]) {
        return true;
      }
    }
    return false;
  });
  if (meaningful.length === 0) return;
  commitOps(c, meaningful.map((ch) => ({
    kind: 'patch' as const,
    shapeId: ch.shapeId,
    before: ch.before,
    after: ch.after
  })));
}

/** Remove a shape (or many) atomically. Untouched ids skipped silently.
 *  Also removes any edges anchored to the deleted shapes — the alternative
 *  (orphan edges that point at nothing) is uglier than the inverse cost
 *  on undo (which restores both shape AND its edges in one go because
 *  they're committed in the same history entry). */
export function deleteShapes(canvasId: string, shapeIds: string[]) {
  const c = ensureCanvasLoaded(canvasId);
  if (!c || shapeIds.length === 0) return;
  const toRemove = c.shapes.filter((s) => shapeIds.includes(s.id) && !s.locked);
  if (toRemove.length === 0) return;
  const removeIds = new Set(toRemove.map((s) => s.id));
  const orphanEdges = c.edges.filter(
    (e) => removeIds.has(e.from.shapeId) || removeIds.has(e.to.shapeId)
  );
  const ops: Op[] = [
    ...orphanEdges.map((edge): Op => ({ kind: 'edge-remove', edge })),
    ...toRemove.map((shape): Op => ({ kind: 'remove', shape }))
  ];
  commitOps(c, ops);
  const eph = ensureEphemeral(c.id);
  eph.selection = eph.selection.filter((id) => !shapeIds.includes(id));
}

// ---- Edge ops ------------------------------------------------------------

function genEdgeId(): string { return genId(); }

/** Build a fresh Edge with sensible defaults. The agent's MCP tool and
 *  the user's drag-from-anchor gesture both go through this — keeping
 *  one place to change defaults (`'arrow'`, orthogonal routing, 2px). */
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

export function addEdge(canvasId: string, edge: Edge): string | null {
  const c = ensureCanvasLoaded(canvasId);
  if (!c) return null;
  /* Refuse self-loops where a shape edges to itself at the same anchor.
     Two anchors on the same shape (e.g. tc → bc) is fine. */
  if (
    edge.from.shapeId === edge.to.shapeId &&
    'anchor' in edge.from && 'anchor' in edge.to &&
    edge.from.anchor === edge.to.anchor
  ) return null;
  commitOps(c, [{ kind: 'edge-add', edge }]);
  return edge.id;
}

export function patchEdge(
  canvasId: string,
  edgeId: string,
  patch: Partial<Edge>
) {
  const c = ensureCanvasLoaded(canvasId);
  if (!c) return;
  const edge = c.edges.find((e) => e.id === edgeId);
  if (!edge) return;
  const before: Partial<Edge> = {};
  for (const key of Object.keys(patch) as (keyof Edge)[]) {
    (before as Record<string, unknown>)[key] = (edge as Record<string, unknown>)[key];
  }
  commitOps(c, [{ kind: 'edge-patch', edgeId, before, after: patch }]);
}

export function deleteEdges(canvasId: string, edgeIds: string[]) {
  const c = ensureCanvasLoaded(canvasId);
  if (!c || edgeIds.length === 0) return;
  const toRemove = c.edges.filter((e) => edgeIds.includes(e.id));
  if (toRemove.length === 0) return;
  commitOps(c, toRemove.map((edge): Op => ({ kind: 'edge-remove', edge })));
  const eph = ensureEphemeral(c.id);
  eph.selection = eph.selection.filter((id) => !edgeIds.includes(id));
}

/** Resolve an EdgeAnchor to the world-coord point it represents. Returns
 *  null when the anchored shape is missing — the renderer treats null as
 *  "ghost edge, draw with a warning glyph or skip". */
export function getEdgeEndpoint(canvas: Canvas, anchor: EdgeAnchor): { x: number; y: number } | null {
  const shape = canvas.shapes.find((s) => s.id === anchor.shapeId);
  if (!shape) return null;
  if ('offset' in anchor) {
    return { x: shape.x + anchor.offset.dx, y: shape.y + anchor.offset.dy };
  }
  /* Canonical anchors. Origin is the shape's top-left; left/top mid/right
     × top/middle/bottom = 9 anchor points. */
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

/** Pick the canonical anchor on `shape` whose world position is nearest
 *  to `point`. Used when an edge gesture lands on a shape body — we
 *  snap to the closest of its 9 anchors instead of the click point. */
export function nearestAnchor(shape: Shape, point: { x: number; y: number }): 'tl'|'tc'|'tr'|'ml'|'mc'|'mr'|'bl'|'bc'|'br' {
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

/** Duplicate the given shape ids by `(dx, dy)` canvas px. New shape ids
 *  are returned in the same order. Used by ⌘D and copy/paste. */
export function duplicateShapes(
  canvasId: string,
  shapeIds: string[],
  dx = 12,
  dy = 12
): string[] {
  const c = ensureCanvasLoaded(canvasId);
  if (!c) return [];
  const clones: Shape[] = [];
  for (const id of shapeIds) {
    const src = c.shapes.find((s) => s.id === id);
    if (!src) continue;
    clones.push({
      ...src,
      id: genShapeId(),
      x: src.x + dx,
      y: src.y + dy,
      z: Date.now() + clones.length,
      createdAt: Date.now(),
      updatedAt: Date.now()
    });
  }
  if (clones.length === 0) return [];
  commitOps(c, clones.map((shape) => ({ kind: 'add', shape })));
  /* Replace selection with the clones — matches Figma / Sketch UX. */
  const eph = ensureEphemeral(c.id);
  eph.selection = clones.map((s) => s.id);
  return eph.selection;
}

// ---- Hierarchy / parents ------------------------------------------------

/** All shapes whose `parentId` chain reaches `shapeId`. Used by the
 *  drag-translate gesture to make frames into proper containers — when
 *  the user drags a frame, its children follow. Transitive: a frame
 *  inside a frame collects children of children. Cycle-safe via a
 *  visited set so a corrupt save with a parent-cycle doesn't loop. */
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

/** Assign / clear a shape's parentId. Used by group / ungroup helpers
 *  and by future drop-into-frame UX. Refuses to introduce cycles
 *  (setting `shape.parentId = X` where X is already a descendant). */
export function setShapeParent(
  canvasId: string,
  shapeId: string,
  parentId: string | null
) {
  const c = ensureCanvasLoaded(canvasId);
  if (!c) return;
  if (parentId !== null) {
    /* Cycle check: walk up the proposed parent chain; if we land on
       the shape we're trying to reparent, the assignment would
       create a cycle. */
    let cursor: string | null = parentId;
    const seen = new Set<string>();
    while (cursor) {
      if (cursor === shapeId) return; /* cycle — abort silently */
      if (seen.has(cursor)) return;
      seen.add(cursor);
      const p: Shape | undefined = c.shapes.find((s) => s.id === cursor);
      cursor = p?.parentId ?? null;
    }
  }
  patchShape(canvasId, shapeId, { parentId });
}

// ---- Z-order ------------------------------------------------------------

/** Reorder shapes within the canvas's z-stack. Resolves to a `patch`
 *  op so undo restores the prior z. We use absolute z numbers (rather
 *  than swapping array positions) because the renderer reads `z` and
 *  passes it to CSS `z-index` — preserving stable values is simpler
 *  than re-deriving from array order on every render. */
export function setShapeZ(
  canvasId: string,
  shapeId: string,
  mode: 'to-front' | 'to-back' | 'forward' | 'backward'
) {
  const c = ensureCanvasLoaded(canvasId);
  if (!c) return;
  const shape = c.shapes.find((s) => s.id === shapeId);
  if (!shape) return;
  const others = c.shapes.filter((s) => s.id !== shapeId);
  const allZ = others.map((s) => s.z);
  let nextZ = shape.z;
  switch (mode) {
    case 'to-front':
      nextZ = (allZ.length === 0 ? shape.z : Math.max(...allZ)) + 1;
      break;
    case 'to-back':
      nextZ = (allZ.length === 0 ? shape.z : Math.min(...allZ)) - 1;
      break;
    case 'forward': {
      /* Find the smallest z among shapes currently above this one,
         then bump just above it. If nothing's above, no-op. */
      const above = allZ.filter((z) => z > shape.z);
      if (above.length === 0) return;
      nextZ = Math.min(...above) + 0.5;
      break;
    }
    case 'backward': {
      const below = allZ.filter((z) => z < shape.z);
      if (below.length === 0) return;
      nextZ = Math.max(...below) - 0.5;
      break;
    }
  }
  if (nextZ === shape.z) return;
  patchShape(canvasId, shapeId, { z: nextZ });
}

// ---- Group / ungroup -----------------------------------------------------

/** Wrap shape ids in a fresh `frame` (or `group`) shape, sized to their
 *  AABB with optional padding. Sets each child's `parentId` to the new
 *  container so subsequent translates carry them along. Returns the
 *  new container id, or null when the inputs were invalid.
 *
 *  The whole operation lands in ONE history entry (container add +
 *  child re-parent ops batched together) — undo restores the prior
 *  flat state in one step. */
export function groupShapes(
  canvasId: string,
  shapeIds: string[],
  opts?: { kind?: 'frame' | 'group'; title?: string; padding?: number }
): string | null {
  const c = ensureCanvasLoaded(canvasId);
  if (!c || shapeIds.length === 0) return null;
  const targets = c.shapes.filter((s) => shapeIds.includes(s.id));
  if (targets.length === 0) return null;
  /* AABB. Identical math to the layout helpers — duplicated here so
     this file stays self-contained (canvasLayout.ts is a higher-level
     consumer). */
  let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
  for (const s of targets) {
    if (s.x < minX) minX = s.x;
    if (s.y < minY) minY = s.y;
    if (s.x + s.w > maxX) maxX = s.x + s.w;
    if (s.y + s.h > maxY) maxY = s.y + s.h;
  }
  const pad = opts?.padding ?? 16;
  const kind = opts?.kind ?? 'frame';
  const container = makeShape({
    kind,
    x: minX - pad,
    y: minY - pad,
    w: (maxX - minX) + pad * 2,
    h: (maxY - minY) + pad * 2,
    /* Container z sits BELOW the children — so children render on top
       of the frame outline, like real container UX. */
    props: kind === 'frame' ? { title: opts?.title ?? 'Group', tint: null } : {}
  });
  /* Float container z under the lowest child — minus 1 keeps the
     ordering invariant strict. */
  const minZ = Math.min(...targets.map((s) => s.z));
  container.z = minZ - 1;
  /* Build batch: container add + each child's re-parent patch. */
  const ops: Op[] = [{ kind: 'add', shape: container }];
  for (const child of targets) {
    if (child.parentId === container.id) continue; /* shouldn't happen — fresh id */
    ops.push({
      kind: 'patch',
      shapeId: child.id,
      before: { parentId: child.parentId },
      after: { parentId: container.id }
    });
  }
  commitOps(c, ops);
  /* Replace selection with the container so the user can immediately
     drag the group as a unit. */
  const eph = ensureEphemeral(c.id);
  eph.selection = [container.id];
  return container.id;
}

/** Inverse of `groupShapes`. Clears children's `parentId` (back to
 *  null = root-level) and removes the container shape. One history
 *  entry. */
export function ungroupShapes(canvasId: string, containerId: string): string[] {
  const c = ensureCanvasLoaded(canvasId);
  if (!c) return [];
  const container = c.shapes.find((s) => s.id === containerId);
  if (!container) return [];
  if (container.kind !== 'frame' && container.kind !== 'group') return [];
  const children = c.shapes.filter((s) => s.parentId === containerId);
  if (children.length === 0) {
    /* No children — just delete the empty container. */
    commitOps(c, [{ kind: 'remove', shape: container }]);
    return [];
  }
  const ops: Op[] = [];
  for (const child of children) {
    ops.push({
      kind: 'patch',
      shapeId: child.id,
      before: { parentId: container.id },
      after: { parentId: null }
    });
  }
  ops.push({ kind: 'remove', shape: container });
  commitOps(c, ops);
  /* Select the freed children so the user can act on them again. */
  const eph = ensureEphemeral(c.id);
  eph.selection = children.map((s) => s.id);
  return eph.selection;
}

// ---- Lock / unlock -------------------------------------------------------

/** Toggle `locked` on shapes. Locked shapes ignore patches from
 *  agent and user — useful when the user wants to "freeze" reference
 *  cards while letting the agent rearrange the rest. */
export function setShapesLocked(canvasId: string, shapeIds: string[], locked: boolean) {
  const c = ensureCanvasLoaded(canvasId);
  if (!c || shapeIds.length === 0) return;
  /* Walk the targets. We bypass the regular `patchShape`-locked guard
     for THIS specific op (otherwise we'd never be able to unlock a
     locked shape — the lock blocks the patch).  */
  const ops: Op[] = [];
  for (const id of shapeIds) {
    const shape = c.shapes.find((s) => s.id === id);
    if (!shape || shape.locked === locked) continue;
    ops.push({
      kind: 'patch',
      shapeId: id,
      before: { locked: shape.locked },
      after: { locked }
    });
  }
  if (ops.length > 0) commitOps(c, ops);
}

// ---- Align / distribute --------------------------------------------------

export type AlignAxis = 'left' | 'center-x' | 'right' | 'top' | 'center-y' | 'bottom';
export type DistributeAxis = 'horizontal' | 'vertical';

/** Align selected shapes on an axis. The reference value (the "anchor"
 *  position to which others snap) is taken from the AABB of the
 *  selection — `left` means "snap every shape's left edge to the
 *  leftmost current left edge", etc. Matches Figma / Sketch. */
export function alignShapes(canvasId: string, shapeIds: string[], axis: AlignAxis) {
  const c = ensureCanvasLoaded(canvasId);
  if (!c || shapeIds.length < 2) return;
  const targets = c.shapes.filter((s) => shapeIds.includes(s.id));
  if (targets.length < 2) return;
  let anchor: number;
  switch (axis) {
    case 'left':       anchor = Math.min(...targets.map((s) => s.x)); break;
    case 'right':      anchor = Math.max(...targets.map((s) => s.x + s.w)); break;
    case 'center-x':   anchor = (Math.min(...targets.map((s) => s.x)) + Math.max(...targets.map((s) => s.x + s.w))) / 2; break;
    case 'top':        anchor = Math.min(...targets.map((s) => s.y)); break;
    case 'bottom':     anchor = Math.max(...targets.map((s) => s.y + s.h)); break;
    case 'center-y':   anchor = (Math.min(...targets.map((s) => s.y)) + Math.max(...targets.map((s) => s.y + s.h))) / 2; break;
  }
  const ops: Op[] = [];
  for (const s of targets) {
    let nx = s.x, ny = s.y;
    switch (axis) {
      case 'left':       nx = anchor; break;
      case 'right':      nx = anchor - s.w; break;
      case 'center-x':   nx = anchor - s.w / 2; break;
      case 'top':        ny = anchor; break;
      case 'bottom':     ny = anchor - s.h; break;
      case 'center-y':   ny = anchor - s.h / 2; break;
    }
    if (nx === s.x && ny === s.y) continue;
    ops.push({
      kind: 'patch',
      shapeId: s.id,
      before: { x: s.x, y: s.y },
      after: { x: nx, y: ny }
    });
  }
  if (ops.length > 0) commitOps(c, ops);
}

/** Equal-spacing distribution along an axis. Shapes are sorted by
 *  current position; the first and last stay put, middle ones are
 *  spaced so the GAPS between consecutive shapes are equal. */
export function distributeShapes(canvasId: string, shapeIds: string[], axis: DistributeAxis) {
  const c = ensureCanvasLoaded(canvasId);
  if (!c || shapeIds.length < 3) return;
  const targets = c.shapes.filter((s) => shapeIds.includes(s.id))
    .slice()
    .sort((a, b) => axis === 'horizontal' ? (a.x - b.x) : (a.y - b.y));
  if (targets.length < 3) return;
  const isHoriz = axis === 'horizontal';
  const first = targets[0];
  const last = targets[targets.length - 1];
  const span = isHoriz
    ? (last.x + last.w) - first.x
    : (last.y + last.h) - first.y;
  const totalShapes = targets.reduce((acc, s) => acc + (isHoriz ? s.w : s.h), 0);
  const totalGap = span - totalShapes;
  const gap = totalGap / (targets.length - 1);
  let cursor = isHoriz ? first.x + first.w + gap : first.y + first.h + gap;
  const ops: Op[] = [];
  for (let i = 1; i < targets.length - 1; i++) {
    const s = targets[i];
    const nx = isHoriz ? cursor : s.x;
    const ny = isHoriz ? s.y : cursor;
    if (nx !== s.x || ny !== s.y) {
      ops.push({
        kind: 'patch',
        shapeId: s.id,
        before: { x: s.x, y: s.y },
        after: { x: nx, y: ny }
      });
    }
    cursor += (isHoriz ? s.w : s.h) + gap;
  }
  if (ops.length > 0) commitOps(c, ops);
}

// ---- Find ---------------------------------------------------------------

/** Substring search over shape labels + text-bearing props. Used by the
 *  agent's `canvas_find` tool to locate "the mermaid where I wrote
 *  auth-flow" without re-reading the full state. Case-insensitive.
 *  Returns matched shape ids in canvas order (creation order, since
 *  shapes are appended). */
export function findShapesByQuery(canvasId: string, query: string): string[] {
  const c = canvasState.open[canvasId];
  if (!c) return [];
  const q = query.trim().toLowerCase();
  if (!q) return [];
  const matches: string[] = [];
  for (const s of c.shapes) {
    const haystacks: string[] = [];
    if (s.label) haystacks.push(s.label);
    const p = s.props as Record<string, unknown>;
    if (typeof p.text === 'string')     haystacks.push(p.text);
    if (typeof p.markdown === 'string') haystacks.push(p.markdown);
    if (typeof p.source === 'string')   haystacks.push(p.source);
    /* For live cards the agent might search by ticket key / repo. */
    if (typeof p.ticketKey === 'string') haystacks.push(p.ticketKey);
    if (typeof p.relPath === 'string')   haystacks.push(p.relPath);
    if (typeof p.shortId === 'string')   haystacks.push(p.shortId);
    if (typeof p.title === 'string')     haystacks.push(p.title);
    /* Snapshot fields on live cards. */
    const snap = p.snapshot;
    if (snap && typeof snap === 'object') {
      const snapObj = snap as Record<string, unknown>;
      if (typeof snapObj.title === 'string')   haystacks.push(snapObj.title);
      if (typeof snapObj.summary === 'string') haystacks.push(snapObj.summary);
      if (typeof snapObj.excerpt === 'string') haystacks.push(snapObj.excerpt);
    }
    for (const h of haystacks) {
      if (h.toLowerCase().includes(q)) {
        matches.push(s.id);
        break;
      }
    }
  }
  return matches;
}

// ---- Undo / redo ---------------------------------------------------------

export function undo(canvasId: string) {
  const c = ensureCanvasLoaded(canvasId);
  if (!c) return;
  const eph = ensureEphemeral(c.id);
  if (eph.historyIndex <= 0) return;
  const entry = eph.history[eph.historyIndex - 1];
  /* Apply inverses in reverse order. For pure add/remove order is moot,
     but for batched patches that touched overlapping fields, reversing
     keeps the round-trip exact. */
  for (let i = entry.ops.length - 1; i >= 0; i--) {
    applyOp(c, invertOp(entry.ops[i]));
  }
  eph.historyIndex -= 1;
  c.version += 1;
  schedulePersist(c);
}

export function redo(canvasId: string) {
  const c = ensureCanvasLoaded(canvasId);
  if (!c) return;
  const eph = ensureEphemeral(c.id);
  if (eph.historyIndex >= eph.history.length) return;
  const entry = eph.history[eph.historyIndex];
  for (const op of entry.ops) applyOp(c, op);
  eph.historyIndex += 1;
  c.version += 1;
  schedulePersist(c);
}

export function canUndo(canvasId: string): boolean {
  const eph = canvasState.ephemeral[canvasId];
  return !!eph && eph.historyIndex > 0;
}
export function canRedo(canvasId: string): boolean {
  const eph = canvasState.ephemeral[canvasId];
  return !!eph && eph.historyIndex < eph.history.length;
}

// ---- Selection -----------------------------------------------------------

export function setSelection(canvasId: string, ids: string[]) {
  const eph = ensureEphemeral(canvasId);
  eph.selection = ids;
}

export function toggleInSelection(canvasId: string, id: string) {
  const eph = ensureEphemeral(canvasId);
  if (eph.selection.includes(id)) {
    eph.selection = eph.selection.filter((s) => s !== id);
  } else {
    eph.selection = [...eph.selection, id];
  }
}

export function clearSelection(canvasId: string) {
  const eph = ensureEphemeral(canvasId);
  if (eph.selection.length > 0) eph.selection = [];
}

export function getSelection(canvasId: string): string[] {
  return canvasState.ephemeral[canvasId]?.selection ?? [];
}
