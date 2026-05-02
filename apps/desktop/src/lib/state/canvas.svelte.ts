// Canvas (whiteboard) reactive state + mutation API. Type definitions
// live in `canvas-types.ts` (no Svelte runtime); this module re-exports
// them so legacy `import { Shape } from '$lib/state/canvas.svelte'`
// callers keep working without a churn.
//
// Persistence: per-canvas JSON files at
// `~/Library/Application Support/Forgehold/canvases/<id>.json` plus a
// thin `index.json`. localStorage stays as fallback when disk init
// fails (e.g. SSR build, sandboxing weirdness). Migration from the
// legacy `forgehold:canvas:v1:*` localStorage layout happens once on
// first disk init (`initCanvasFromDisk`); the legacy keys are then
// cleared to free the ~5 MB browser quota that big canvases would
// otherwise blow.

// ---- Types ---------------------------------------------------------------

export type {
  ShapeKind,
  Shape,
  EdgeAnchor,
  Edge,
  CanvasViewport,
  Canvas,
  CanvasIndexEntry,
  CanvasTool,
  Op,
  HistoryEntry,
  CanvasInstanceState,
  CanvasEphemeral
} from './canvas-types';

import type {
  Canvas,
  CanvasEphemeral,
  CanvasIndexEntry,
  CanvasInstanceState,
  CanvasTool,
  CanvasViewport,
  Edge,
  EdgeAnchor,
  HistoryEntry,
  Op,
  Shape,
  ShapeKind
} from './canvas-types';

/** History bound — older entries discarded so a long-running session
 *  doesn't accumulate unbounded memory. Matches the 50-op cap mentioned
 *  in [docs/CANVAS.md §11.3](../../../../docs/CANVAS.md). */
const HISTORY_CAP = 50;

// ---- Storage keys --------------------------------------------------------

const STORAGE_INDEX = 'forgehold:canvas:index:v1';
const STORAGE_CANVAS_PREFIX = 'forgehold:canvas:v1:';
const STORAGE_INSTANCE_STATE = 'forgehold:canvas:instances:v1';

// ---- Disk persistence (M1 §1.1) -----------------------------------------
// `_diskDir` is the resolved canvas folder once `initCanvasFromDisk` runs.
// Until it's set, the legacy localStorage path is the source of truth — so
// SSR / unit tests and the very-first launch keep working. After init, all
// `persistCanvas` / `persistIndex` writes go to disk and localStorage stays
// silent. Mirrors the same `_diskDir` pattern in `sessions.svelte.ts`
// (commit 36eecee), so the failure modes (write quota, sandbox refusal)
// surface consistently across both stores.

import { invoke } from '@tauri-apps/api/core';
import { notify } from '$lib/state/toaster.svelte';
import {
  blankCanvas,
  defaultPropsFor,
  genEdgeId,
  genId,
  genShapeId,
  getDescendants,
  getEdgeEndpoint,
  hydrateCanvas,
  indexEntryFor,
  makeEdge,
  makeShape,
  nearestAnchor,
  snapToGrid
} from './canvas-helpers';

/* Re-export the pure helpers so legacy callers that did
 * `import { makeShape } from '$lib/state/canvas.svelte'` keep
 * working. New callers can import directly from `canvas-helpers`. */
export {
  blankCanvas,
  defaultPropsFor,
  genEdgeId,
  genId,
  genShapeId,
  getDescendants,
  getEdgeEndpoint,
  hydrateCanvas,
  indexEntryFor,
  makeEdge,
  makeShape,
  nearestAnchor,
  snapToGrid
};

/** Throttle for the "shape is locked" hint toast. Without this, a
 *  drag against a locked shape fires `patchShape` per pointermove
 *  and would carpet the screen with N toasts. We emit one toast per
 *  second per shape, rate-limited globally. */
let lastLockedHintAt = 0;

let _diskDir: string | null = null;

function canvasFilePath(id: string): string {
  return `${_diskDir}/${id}.json`;
}

function canvasIndexFilePath(): string {
  return `${_diskDir}/index.json`;
}

// ---- Reactive state ------------------------------------------------------

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
  /** Last persistCanvas() timestamp per canvas id. Drives the
   *  auto-save indicator on CanvasColumn (M4 §2.10.1). UI flashes
   *  for ~1 s after the value bumps. */
  lastSavedAt: Record<string, number>;
}>({
  index: [],
  open: {},
  byInstance: {},
  ephemeral: {},
  lastSavedAt: {}
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
  /* Disk path: serialize once, fire-and-forget through the Tauri IPC.
   * Any failure (sandbox / fs error) silently falls through to the
   * localStorage write below — keeps the user's data accessible on
   * the next launch even if the disk write tier is broken. */
  const json = JSON.stringify({ entries: canvasState.index });
  if (_diskDir) {
    void invoke('fs_write_file', {
      path: canvasIndexFilePath(),
      contents: json
    }).catch(() => { /* ignore — localStorage acts as the safety net */ });
    return;
  }
  try {
    localStorage.setItem(STORAGE_INDEX, json);
  } catch {
    /* quota / SSR — ignore */
  }
}

function persistInstanceState() {
  /* Per-instance UI state (active tab, current tool) is small and
   * frequently mutated — keep on localStorage even after the disk
   * migration. Quota concerns are about big canvases, not which tab
   * is open in column #2. */
  try {
    localStorage.setItem(STORAGE_INSTANCE_STATE, JSON.stringify(canvasState.byInstance));
  } catch { /* ignore */ }
}

function persistCanvas(canvas: Canvas) {
  const json = JSON.stringify(canvas);
  /* Stamp the auto-save indicator (M4 §2.10.1). UI watches
   * `canvasState.lastSavedAt[canvas.id]` to flash a "saved" pulse
   * on the column header. Stamp on the local-mem path too so the
   * indicator works pre-disk-migration. */
  canvasState.lastSavedAt[canvas.id] = Date.now();
  if (_diskDir) {
    void invoke('fs_write_file', {
      path: canvasFilePath(canvas.id),
      contents: json
    }).catch(() => { /* ignore — see persistIndex */ });
    return;
  }
  try {
    localStorage.setItem(STORAGE_CANVAS_PREFIX + canvas.id, json);
  } catch { /* ignore */ }
}

function readCanvasFromStorage(id: string): Canvas | null {
  try {
    const raw = localStorage.getItem(STORAGE_CANVAS_PREFIX + id);
    if (!raw) return null;
    return hydrateCanvas(JSON.parse(raw) as Partial<Canvas>);
  } catch {
    return null;
  }
}

/** Disk-backed analogue of `readCanvasFromStorage`. Used by
 *  `loadCanvas` once `initCanvasFromDisk` has resolved the canvas
 *  directory; falls back through to the localStorage path when the
 *  read fails or returns no body. */
async function readCanvasFromDisk(id: string): Promise<Canvas | null> {
  if (!_diskDir) return null;
  try {
    const exists = await invoke<boolean>('fs_path_exists', {
      path: canvasFilePath(id)
    });
    if (!exists) return null;
    const raw = await invoke<string>('fs_read_file', {
      path: canvasFilePath(id)
    });
    return hydrateCanvas(JSON.parse(raw) as Partial<Canvas>);
  } catch {
    return null;
  }
}

/** Initialize disk persistence. Called from +page.svelte onMount with
 *  the resolved app-data dir (`~/Library/Application Support/Forgehold`
 *  on macOS).
 *
 *  - First launch with no `index.json` on disk: migrate every
 *    `forgehold:canvas:v1:*` localStorage entry to disk, write
 *    `index.json`, then clear those localStorage keys to free quota.
 *  - Subsequent launches: read `index.json` (lightweight — just the
 *    library entries) and let `loadCanvas` lazy-fetch the per-canvas
 *    JSON files on demand.
 *
 *  Failures fall back transparently to localStorage so the app keeps
 *  working on first run if the disk write tier is broken (sandbox
 *  refusal, missing parent dir we can't create, etc.). */
export async function initCanvasFromDisk(appDataDir: string): Promise<void> {
  _diskDir = `${appDataDir}/canvases`;
  try {
    const indexExists = await invoke<boolean>('fs_path_exists', {
      path: canvasIndexFilePath()
    });
    if (indexExists) {
      const raw = await invoke<string>('fs_read_file', {
        path: canvasIndexFilePath()
      });
      const parsed = JSON.parse(raw) as { entries?: CanvasIndexEntry[] };
      if (Array.isArray(parsed.entries)) {
        canvasState.index = parsed.entries.filter(
          (e): e is CanvasIndexEntry =>
            !!e && typeof e.id === 'string' && typeof e.name === 'string'
        );
      }
      /* Eager-load every canvas referenced by the index so the sync
       * `ensureCanvasLoaded` callers (the renderer, the agent system
       * prompt) keep working without a refactor to async. Mirrors
       * what `initSessionsFromDisk` does for session bodies. Bounded
       * by the user's library size (typically <50 canvases × <100KB),
       * well within the renderer's memory budget. */
      for (const entry of canvasState.index) {
        const c = await readCanvasFromDisk(entry.id);
        if (c) canvasState.open[entry.id] = c;
      }
    } else {
      /* Fresh-on-disk install. Pull whatever's in localStorage right
       * now (already loaded by `restoreCanvasState` earlier in onMount)
       * and write each canvas + the index file to disk. */
      for (const entry of canvasState.index) {
        const c = readCanvasFromStorage(entry.id);
        if (c) {
          await invoke('fs_write_file', {
            path: canvasFilePath(c.id),
            contents: JSON.stringify(c)
          });
        }
      }
      await invoke('fs_write_file', {
        path: canvasIndexFilePath(),
        contents: JSON.stringify({ entries: canvasState.index })
      });
    }
    /* localStorage is no longer the source of truth. Clear the bulk
     * of it so the ~5 MB origin quota doesn't get squeezed by old
     * snapshots. The lighter index/instance keys stay so the legacy
     * boot path still works if the disk init ever fails on a future
     * launch. */
    try {
      const toRemove: string[] = [];
      for (let i = 0; i < localStorage.length; i++) {
        const k = localStorage.key(i);
        if (k && k.startsWith(STORAGE_CANVAS_PREFIX)) toRemove.push(k);
      }
      for (const k of toRemove) localStorage.removeItem(k);
    } catch { /* ignore */ }
  } catch (e) {
    console.error('[canvas] disk init failed, falling back to localStorage:', e);
    _diskDir = null;
  }
}

/** True once the disk persistence tier is active. Mostly for tests +
 *  Settings diagnostics. */
export function canvasUsesDisk(): boolean {
  return _diskDir !== null;
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
  /* Belt + braces: clear from BOTH stores even when only one is
   * active. The disk path is the live one in production, but when
   * the legacy localStorage entry is still there from a pre-migration
   * launch it'd silently re-appear next boot if we don't sweep it. */
  try { localStorage.removeItem(STORAGE_CANVAS_PREFIX + id); } catch { /* ignore */ }
  if (_diskDir) {
    void invoke('fs_remove_file', { path: canvasFilePath(id) }).catch(() => {
      /* file already gone on disk — nothing to do */
    });
  }
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

/* Snap, geometry, and shape factories all live in `canvas-helpers.ts`
 * (re-exported above). The persistence helper below stays here
 * because it bumps `canvasState.lastSavedAt[id]` via `persistCanvas`. */

/** Persist the canvas's `updatedAt` + index entry. Debounced so a
 *  chatty drag-resize doesn't write to localStorage on every frame. */
let canvasPersistTimer: ReturnType<typeof setTimeout> | null = null;
function schedulePersist(canvas: Canvas) {
  canvas.updatedAt = Date.now();
  if (canvasPersistTimer) clearTimeout(canvasPersistTimer);
  canvasPersistTimer = setTimeout(() => {
    persistCanvas(canvas);
    upsertIndexEntry(indexEntryFor(canvas));
  }, 600);
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
  if (!shape) return;
  if (shape.locked) {
    /* Surface a hint so the user understands why their drag /
     * edit no-op'd (M4 §2.10.8). Throttled because a real drag
     * fires this 60+ times a second. */
    const now = Date.now();
    if (now - lastLockedHintAt > 1500 && !opts?.transient) {
      lastLockedHintAt = now;
      notify({
        kind: 'info',
        title: 'Shape is locked',
        body: 'Right-click → Unlock or hit ⇧⌘L to edit.'
      });
    }
    return;
  }
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
