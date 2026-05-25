<script lang="ts">
  // Canvas (whiteboard) solo app — through M-canvas-2.
  //
  // M1 baseline (already shipped):
  //   - Infinite plane with dot/grid background.
  //   - Pan (Space-drag, middle-mouse), zoom around cursor (⌘-wheel).
  //   - Empty-state, viewport persistence, library index.
  //
  // M2 additions (this file):
  //   - Toolbar (V/R/O/L/A/T/S) overlaid on the surface.
  //   - Six primitive shape kinds (rect, ellipse, line, arrow, text, sticky)
  //     drawn by click-and-drag with the matching tool.
  //   - Selection model: click → select; ⇧-click → toggle add; click empty
  //     → clear; drag empty → marquee select.
  //   - Drag-translate selected shapes (single or multi-select).
  //   - Resize handles (8 cardinal/diagonal) for single-shape selection.
  //   - Grid snap (default 8 px) on drawing / translate / resize, with
  //     ⌘ as an opt-out modifier.
  //   - Undo / redo (⌘Z / ⇧⌘Z) backed by the op log in canvas.svelte.ts.
  //   - Delete (⌫), duplicate (⌘D).
  //   - Per-instance tool persistence — switch tabs and the tool you
  //     left in is still active.
  //
  // Deferred to later milestones:
  //   - Edge / connector tool (M-canvas-4).
  //   - Mermaid / DOT / code / image / freehand shapes (M-canvas-3).
  //   - Edge-snap with smart guides (M-canvas-4).
  //   - Library overlay with thumbnails (M-canvas-5).
  //   - Forge live cards via drag-from-other-columns (M-canvas-6).
  //   - Agent link + MCP tools (M-canvas-7+).

  import { onMount, onDestroy, untrack } from 'svelte';
  import {
    anchorWorld,
    computeAlignment,
    intrinsicFromDataUrl,
    looksLikeImage,
    marqueeFromPoints,
    readAsDataUrl,
    rectIntersects,
    type CanonicalAnchor,
    anchorStyleCss,
    computeResize,
    handleCursor,
    handleStyleCss,
    HANDLE_IDS,
    isTypingTarget,
    rescaleShapePoints,
    type HandleId,
  } from './canvasGeometry';
  import { buildLiveCardShape } from './canvasLiveCard';
  import CanvasShape from '$lib/components/canvas/CanvasShape.svelte';
  import CanvasToolbar from '$lib/components/canvas/CanvasToolbar.svelte';
  import CanvasEdges from '$lib/components/canvas/CanvasEdges.svelte';
  import CanvasLibrary from '$lib/components/canvas/CanvasLibrary.svelte';
  import CanvasMinimap from '$lib/components/canvas/CanvasMinimap.svelte';
  import { applyLayout, type LayoutAlgorithm } from '$lib/services/canvasLayout';
  import { dragState, setDragPayload, type DragPayload } from '$lib/state/drag.svelte';
  import { sessionsState } from '$lib/state/sessions.svelte';
  import { layoutState } from '$lib/state/layout.svelte';
  import {
    canvasState,
    ensureCanvasLoaded,
    createAndOpenInInstance,
    setActiveCanvasTab,
    closeCanvasTab,
    setViewport,
    renameCanvas,
    setTool,
    snapToGrid,
    makeShape,
    addShape,
    getDescendants,
    patchShape,
    commitTransientPatches,
    deleteShapes,
    duplicateShapes,
    undo,
    redo,
    canUndo as canUndoFor,
    canRedo as canRedoFor,
    setSelection,
    toggleInSelection,
    clearSelection,
    makeEdge,
    addEdge,
    deleteEdges,
    nearestAnchor,
    groupShapes,
    ungroupShapes,
    type CanvasTool,
    type Shape,
    type ShapeKind,
    type EdgeAnchor
  } from '$lib/state/canvas.svelte';

  interface Props {
    instanceId: string;
    /** Forwarded to live-card shapes — see CanvasShape's `onCardOpen`. */
    onCardOpen?: (shape: Shape) => void;
  }

  let { instanceId, onCardOpen }: Props = $props();

  const instState = $derived(canvasState.byInstance[instanceId]);
  const activeCanvasId = $derived(instState?.activeId ?? null);
  const activeCanvas = $derived(activeCanvasId ? canvasState.open[activeCanvasId] : null);

  /** Sessions whose `linkedCanvasId === activeCanvasId`. Drives the
   *  "Linked: <session>" chip in the header (parity with the
   *  TerminalSurface chip) so the user sees from the canvas side
   *  which agent has the canvas-tools wired in. */
  const linkedSessions = $derived.by(() => {
    if (!activeCanvasId) return [];
    const out: { sessionId: string; title: string; kind: 'claude' | 'cursor' }[] = [];
    for (const s of sessionsState.list) {
      if (s.linkedCanvasId !== activeCanvasId) continue;
      out.push({ sessionId: s.id, title: s.title, kind: s.agentKind });
    }
    return out;
  });
  const tool = $derived<CanvasTool>(instState?.tool ?? 'select');

  // ---- Camera (local mirror of viewport) -------------------------------

  let camX = $state(0);
  let camY = $state(0);
  let camZoom = $state(1);

  let lastSyncedCanvasId: string | null = null;
  $effect(() => {
    if (!activeCanvas) {
      lastSyncedCanvasId = null;
      return;
    }
    if (activeCanvas.id !== lastSyncedCanvasId) {
      lastSyncedCanvasId = activeCanvas.id;
      untrack(() => {
        camX = activeCanvas.viewport.x;
        camY = activeCanvas.viewport.y;
        camZoom = activeCanvas.viewport.zoom;
      });
    }
  });

  function pushViewport() {
    if (!activeCanvas) return;
    setViewport(activeCanvas.id, { x: camX, y: camY, zoom: camZoom });
  }

  $effect(() => {
    if (!instState) return;
    for (const id of instState.tabs) ensureCanvasLoaded(id);
  });

  /* Agent-driven viewport focus. The `canvas_focus` MCP tool sets a
     `pendingFocus` field on ephemeral state; we watch it, recenter the
     camera on the requested shape, and clear the field. The TS dep is
     `pendingFocus` itself so back-to-back focuses on the same shape
     re-fire (different `ts`). */
  $effect(() => {
    if (!activeCanvas || !surfaceEl) return;
    const eph = canvasState.ephemeral[activeCanvas.id];
    const pending = eph?.pendingFocus;
    if (!pending) return;
    const shape = activeCanvas.shapes.find((s) => s.id === pending.shapeId);
    if (shape) {
      const rect = surfaceEl.getBoundingClientRect();
      const cx = shape.x + shape.w / 2;
      const cy = shape.y + shape.h / 2;
      camX = cx - rect.width / (2 * camZoom);
      camY = cy - rect.height / (2 * camZoom);
      pushViewport();
    }
    /* Clear the request so the next focus on the same shape (different
       `ts`) re-triggers this effect. */
    if (eph) eph.pendingFocus = null;
  });

  // ---- Tab actions -----------------------------------------------------

  function onCreateNew() { createAndOpenInInstance(instanceId); }
  function onSelectTab(id: string) { setActiveCanvasTab(instanceId, id); }
  function onCloseTab(e: MouseEvent, id: string) {
    e.stopPropagation();
    closeCanvasTab(instanceId, id);
  }

  let editingTabId = $state<string | null>(null);
  let editingTabDraft = $state('');
  /** Library overlay — shown above the canvas surface; clicks outside
   *  the panel close it. Rendered inside this column so opening a
   *  library on column A doesn't blank column B. */
  let libraryOpen = $state(false);
  function openLibrary() { libraryOpen = true; }
  function closeLibrary() { libraryOpen = false; }

  /** Minimap toggle — defaults to ON for new canvas columns since the
   *  glyph in the bottom-right is small and adds navigability for any
   *  canvas bigger than the viewport. Toggled via the `M` shortcut or
   *  the X on the minimap itself. */
  let minimapVisible = $state(true);
  function toggleMinimap() { minimapVisible = !minimapVisible; }
  function startTabRename(id: string, currentName: string) {
    editingTabId = id;
    editingTabDraft = currentName;
  }
  function commitTabRename() {
    if (editingTabId && editingTabDraft.trim()) renameCanvas(editingTabId, editingTabDraft.trim());
    editingTabId = null;
    editingTabDraft = '';
  }
  function cancelTabRename() {
    editingTabId = null;
    editingTabDraft = '';
  }

  // ---- Selection -------------------------------------------------------

  const selection = $derived<string[]>(
    activeCanvasId ? canvasState.ephemeral[activeCanvasId]?.selection ?? [] : []
  );
  /** Split the selection list — same id namespace covers shapes + edges
   *  (both are UUIDs), so we resolve membership at consumption time
   *  rather than carry two parallel lists in state. */
  const selectedShapes = $derived<Shape[]>(
    activeCanvas
      ? activeCanvas.shapes.filter((s) => selection.includes(s.id))
      : []
  );
  const selectedEdgeIds = $derived<string[]>(
    activeCanvas
      ? activeCanvas.edges.filter((e) => selection.includes(e.id)).map((e) => e.id)
      : []
  );
  /** AABB of all selected shapes — drives the multi-select transform box.
   *  Single-select also uses this; it's the same math. */
  const selectionBox = $derived.by(() => {
    if (selectedShapes.length === 0) return null;
    let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
    for (const s of selectedShapes) {
      if (s.x < minX) minX = s.x;
      if (s.y < minY) minY = s.y;
      if (s.x + s.w > maxX) maxX = s.x + s.w;
      if (s.y + s.h > maxY) maxY = s.y + s.h;
    }
    return { x: minX, y: minY, w: maxX - minX, h: maxY - minY };
  });

  // ---- Surface gestures -------------------------------------------------

  let surfaceEl = $state<HTMLDivElement | null>(null);
  /** Hold Space → pan-mode cursor. Pan also fires on middle-button down. */
  let spaceHeld = $state(false);
  let panActive = $state(false);

  /** Marquee — when set, a translucent rect is drawn between these
   *  anchor + current points (in canvas coords). Cleared on release. */
  let marquee = $state<{ ax: number; ay: number; bx: number; by: number } | null>(null);

  /** Smart-guide alignment lines. While the user drag-translates a
   *  selection, we compute which edges / centers of the lead shape
   *  align (within ~4 screen px) with edges / centers of any OTHER
   *  shape. Each match produces both a snap and a guide line.
   *
   *  `vertical` holds canvas-x coordinates for full-height vertical
   *  guides; `horizontal` holds canvas-y for full-width horizontal
   *  guides. Cleared the moment a translate gesture ends or any
   *  other gesture starts. */
  let activeGuides = $state<{ vertical: number[]; horizontal: number[] }>({
    vertical: [], horizontal: []
  });

  /** While drawing a fresh shape, an in-progress preview lives here.
   *  Same shape model as a committed shape, just not in the canvas yet. */
  let drawingPreview = $state<Shape | null>(null);

  /** Freehand-specific live preview — derived from the in-flight
   *  gesture's point list so the stroke renders on every move tick. We
   *  build the bbox from the world-coord points and translate them
   *  into bbox-local for the renderer (same shape the committed
   *  freehand will end up with). */
  const freehandPreview = $derived.by<Shape | null>(() => {
    if (!gesture || gesture.kind !== 'freehand') return null;
    const pts = gesture.points;
    if (pts.length < 1) return null;
    let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
    for (const [x, y] of pts) {
      if (x < minX) minX = x;
      if (y < minY) minY = y;
      if (x > maxX) maxX = x;
      if (y > maxY) maxY = y;
    }
    const pad = 6;
    const bx = minX - pad;
    const by = minY - pad;
    const bw = Math.max(1, maxX - minX + pad * 2);
    const bh = Math.max(1, maxY - minY + pad * 2);
    const localPoints = pts.map(([x, y, p]) => [x - bx, y - by, p] as [number, number, number]);
    return {
      id: '__freehand_preview',
      kind: 'freehand',
      x: bx, y: by, w: bw, h: bh,
      rot: 0, z: 0, parentId: null, locked: false, hidden: false, label: null,
      props: { points: localPoints, color: 'text-0', thickness: 2, smoothing: 0.5 },
      createdAt: 0, createdBy: 'user', updatedAt: 0
    };
  });

  /** Active gesture: any non-pan, non-pan-keyboard interaction sets this
   *  so global handlers know what's in flight. Used to dispatch
   *  pointer-move / pointer-up to the right routine. */
  type Gesture =
    | { kind: 'translate'; startCanvas: { x: number; y: number };
        startSnapshots: Array<{ id: string; x: number; y: number }> }
    | { kind: 'resize'; handle: HandleId; shapeId: string;
        before: { x: number; y: number; w: number; h: number };
        startCanvas: { x: number; y: number } }
    | { kind: 'draw'; tool: ShapeKind; startCanvas: { x: number; y: number } }
    | { kind: 'freehand'; points: Array<[number, number, number]> }
    | { kind: 'edge';
        from: EdgeAnchor;
        anchorWorld: { x: number; y: number };
        cursorWorld: { x: number; y: number };
        hoverShapeId: string | null }
    | { kind: 'marquee'; startCanvas: { x: number; y: number };
        additive: boolean; baseSelection: string[] };
  let gesture = $state<Gesture | null>(null);

  /** Pointer id whose `setPointerCapture` we postponed at translate-start.
   *  We defer capture for click-on-shape gestures so that:
   *    - a clean click → click → release → click sequence is NOT
   *      redirected to `surfaceEl` by capture, which would steal the
   *      `dblclick` event from the shape's own `ondblclick` handler
   *      (used to enter inline edit mode for text / sticky / code /
   *      mermaid / frame).
   *  Capture is acquired lazily once the user pushes the pointer past
   *  a small dead-zone (`DRAG_DEADZONE_PX` screen px) — at that point
   *  it's a real drag and we want lossless tracking even if the
   *  pointer leaves the surface mid-translate. Pure dblclick stays
   *  inside the dead-zone, so capture never activates and the event
   *  reaches the shape. */
  let pendingCaptureId = $state<number | null>(null);
  const DRAG_DEADZONE_PX = 4;

  // HandleId moved to ./canvasGeometry (wave-29).
  // ---- Coord conversion -----------------------------------------------

  function cssToCanvas(cssX: number, cssY: number): { x: number; y: number } {
    return { x: camX + cssX / camZoom, y: camY + cssY / camZoom };
  }
  function eventToCanvas(e: PointerEvent | MouseEvent): { x: number; y: number } {
    if (!surfaceEl) return { x: 0, y: 0 };
    const rect = surfaceEl.getBoundingClientRect();
    return cssToCanvas(e.clientX - rect.left, e.clientY - rect.top);
  }

  /** Snap a (x, y) point to the canvas grid. ⌘ / ⌃ held bypasses. */
  function maybeSnap(p: { x: number; y: number }, e: PointerEvent | MouseEvent | null): { x: number; y: number } {
    const noSnap = !!e && (e.metaKey || e.ctrlKey);
    if (noSnap || !activeCanvas) return p;
    const g = activeCanvas.gridSize;
    return { x: snapToGrid(p.x, g), y: snapToGrid(p.y, g) };
  }

  // ---- Wheel: pan / zoom ----------------------------------------------

  function onWheel(e: WheelEvent) {
    if (!activeCanvas || !surfaceEl) return;
    const rect = surfaceEl.getBoundingClientRect();
    const cx = e.clientX - rect.left;
    const cy = e.clientY - rect.top;
    if (e.ctrlKey || e.metaKey) {
      e.preventDefault();
      const before = cssToCanvas(cx, cy);
      const factor = Math.exp(-e.deltaY * 0.0015);
      camZoom = Math.max(0.1, Math.min(4, camZoom * factor));
      const after = cssToCanvas(cx, cy);
      camX += before.x - after.x;
      camY += before.y - after.y;
    } else {
      e.preventDefault();
      camX += e.deltaX / camZoom;
      camY += e.deltaY / camZoom;
    }
    pushViewport();
  }

  // ---- Pointer-down router --------------------------------------------

  function onPointerDown(e: PointerEvent) {
    if (!surfaceEl || !activeCanvas) return;

    /* Pan gesture wins over everything: middle button, or Space-held +
       left button. We keep it isolated so it never starts a translate or
       a draw by accident. */
    const isPanGesture = e.button === 1 || (e.button === 0 && spaceHeld);
    if (isPanGesture) { startPan(e); return; }

    if (e.button !== 0) return;

    /* Climb the DOM to find what kind of thing was clicked. Order matters:
         1. anchor dot → edge-creation gesture (highest priority — anchors
            are tiny and live ON TOP of selection rings & shape bodies)
         2. resize handle → bbox resize
         3. edge hit-path → edge selection
         4. shape body → translate / select
         5. empty → marquee
       We don't use elementsFromPoint() because the stage transform makes
       hit-test coords a function of the camera; data attributes are
       simpler and zoom-independent. */
    const target = e.target as Element | null;
    const anchorEl = target?.closest('[data-cv-anchor]') as HTMLElement | null;
    if (anchorEl && tool === 'select') {
      const anchorId = anchorEl.dataset.cvAnchor as 'tc'|'mr'|'bc'|'ml'|'tl'|'tr'|'bl'|'br';
      const shapeId = anchorEl.dataset.cvAnchorShape;
      if (shapeId) { startEdge(e, shapeId, anchorId); return; }
    }
    const handle = target?.closest('[data-cv-handle]') as HTMLElement | null;
    if (handle && tool === 'select') {
      const id = handle.dataset.cvHandle as HandleId;
      startResize(e, id);
      return;
    }
    const edgeEl = target?.closest('[data-edge-id]') as Element | null;
    if (edgeEl && tool === 'select') {
      const edgeId = (edgeEl as HTMLElement).dataset.edgeId
        ?? (edgeEl as SVGElement).getAttribute('data-edge-id');
      if (edgeId && activeCanvas) {
        if (e.shiftKey) toggleInSelection(activeCanvas.id, edgeId);
        else            setSelection(activeCanvas.id, [edgeId]);
        return;
      }
    }
    const shapeEl = target?.closest('[data-shape-id]') as HTMLElement | null;
    const shapeId = shapeEl?.dataset.shapeId ?? null;

    if (tool === 'select') {
      if (shapeId) {
        startSelectAndTranslate(e, shapeId);
      } else {
        startMarquee(e);
      }
      return;
    }

    /* Freehand: dedicated point-trace gesture, not a bbox drag. */
    if (tool === 'freehand') {
      startFreehand(e);
      return;
    }

    /* Image tool is a one-click action — open the file picker, place
       the resulting image at the click point. We don't let it become
       a drag-to-bbox gesture because the dimensions come from the
       intrinsic image size (snapped to a sensible max). */
    if (tool === 'image') {
      const at = eventToCanvas(e);
      void pickAndInsertImage(at);
      return;
    }

    /* Drawing tools: every tool draws by drag. Click-without-drag still
       creates a default-sized shape (handled in finishDraw via min size
       fallback). */
    const kind = toolToShapeKind(tool);
    if (kind) startDraw(e, kind);
  }

  function toolToShapeKind(t: CanvasTool): ShapeKind | null {
    switch (t) {
      case 'rect':     return 'rect';
      case 'ellipse':  return 'ellipse';
      case 'line':     return 'line';
      case 'arrow':    return 'arrow-shape';
      case 'text':     return 'text';
      case 'sticky':   return 'sticky';
      case 'mermaid':  return 'mermaid';
      case 'code':     return 'code';
      case 'image':    return 'image';
      case 'frame':    return 'frame';
      /* `freehand` is dispatched separately — it needs a per-point
         drawing gesture that's not a bbox-style click+drag. */
      default:         return null;
    }
  }

  // ---- Pan -------------------------------------------------------------

  function startPan(e: PointerEvent) {
    if (!surfaceEl) return;
    e.preventDefault();
    panActive = true;
    surfaceEl.setPointerCapture(e.pointerId);
    let lastX = e.clientX;
    let lastY = e.clientY;
    const onMove = (m: PointerEvent) => {
      const dx = m.clientX - lastX;
      const dy = m.clientY - lastY;
      lastX = m.clientX; lastY = m.clientY;
      camX -= dx / camZoom;
      camY -= dy / camZoom;
    };
    // Pull cleanup into its own scope so it runs on EVERY exit path
    // — onUp's normal flow OR a thrown releasePointerCapture (e.g.
    // when the pointer was stolen by a parent and is no longer
    // captured by surfaceEl). Without this guard a failed release
    // would skip removeEventListener and the move/up handlers
    // would dangle past the gesture, accumulating per pan attempt.
    const cleanup = () => {
      try {
        surfaceEl?.removeEventListener('pointermove', onMove);
        surfaceEl?.removeEventListener('pointerup', onUp);
        surfaceEl?.removeEventListener('pointercancel', onUp);
      } catch {
        // removeEventListener never throws in spec, but be defensive.
      }
      panActive = false;
    };
    const onUp = (u: PointerEvent) => {
      try {
        surfaceEl?.releasePointerCapture(u.pointerId);
      } catch {
        // releasePointerCapture throws InvalidStateError if the pointer
        // is no longer captured (e.g. parent stole capture mid-gesture).
        // Don't let that swallow our cleanup.
      }
      cleanup();
      pushViewport();
    };
    surfaceEl.addEventListener('pointermove', onMove);
    surfaceEl.addEventListener('pointerup', onUp);
    surfaceEl.addEventListener('pointercancel', onUp);
  }

  // ---- Translate -------------------------------------------------------

  function startSelectAndTranslate(e: PointerEvent, shapeId: string) {
    if (!activeCanvas) return;
    e.preventDefault();
    /* Select before translating. Behavior matches Figma:
         - plain click on a shape: selection becomes [shapeId]
         - ⇧-click on an unselected shape: add to selection
         - ⇧-click on a selected shape: remove (toggle)
         - plain click on already-selected shape: keep selection (so
           user can drag a multi-select group without losing it). */
    const alreadyIn = selection.includes(shapeId);
    if (e.shiftKey) {
      toggleInSelection(activeCanvas.id, shapeId);
    } else if (!alreadyIn) {
      setSelection(activeCanvas.id, [shapeId]);
    }
    /* Snapshot starting positions of every selected shape AND every
       descendant of any selected frame / group. The descendant
       expansion makes frames behave as containers — drag a frame and
       its children follow. We re-derive selection from the store
       here because `setSelection` may have just changed it. */
    const liveSelection = canvasState.ephemeral[activeCanvas.id]?.selection ?? [];
    const expandedIds = new Set<string>();
    for (const id of liveSelection) {
      expandedIds.add(id);
      const sh = activeCanvas.shapes.find((s) => s.id === id);
      if (sh && (sh.kind === 'frame' || sh.kind === 'group')) {
        for (const desc of getDescendants(activeCanvas, sh.id)) {
          expandedIds.add(desc.id);
        }
      }
    }
    const snapshot = Array.from(expandedIds)
      .map((id) => activeCanvas!.shapes.find((s) => s.id === id))
      .filter((s): s is Shape => !!s)
      .map((s) => ({ id: s.id, x: s.x, y: s.y }));
    if (snapshot.length === 0) return;
    const startCanvas = eventToCanvas(e);
    gesture = { kind: 'translate', startCanvas, startSnapshots: snapshot };
    /* Defer setPointerCapture(e.pointerId) until the pointer leaves
       the dead-zone (see `pendingCaptureId` declaration). A pure
       click-without-drag must NOT capture, otherwise the follow-up
       `click` and `dblclick` get retargeted to surfaceEl and the
       shape's inline-edit handler never fires. */
    pendingCaptureId = e.pointerId;
  }

  function applyTranslate(currentCanvas: { x: number; y: number }, e: PointerEvent) {
    if (!activeCanvas || !gesture || gesture.kind !== 'translate') return;
    const dx = currentCanvas.x - gesture.startCanvas.x;
    const dy = currentCanvas.y - gesture.startCanvas.y;
    /* Pick a "lead" shape — the first in the snapshot list — and
       compute its destination. The delta from start→destination is
       then applied to every other selected shape so multi-select
       moves stay rigid (only the lead's edges/centers get aligned;
       relative offsets between selected shapes are preserved). */
    const lead = gesture.startSnapshots[0];
    let nx = lead.x + dx;
    let ny = lead.y + dy;
    const noSnap = e.metaKey || e.ctrlKey;

    /* Smart guides — alignment with edges / centers of un-selected
       shapes. Threshold is ~4 screen px (counter-zoomed to canvas
       px) so guides feel just-magnetic regardless of zoom. */
    const guideX: number[] = [];
    const guideY: number[] = [];
    if (!noSnap && activeCanvas) {
      /* Lead bbox at the unsnapped destination — w/h come from the
         live shape since the snapshot only stores x/y. */
      const leadShape = activeCanvas.shapes.find((s) => s.id === lead.id);
      const lw = leadShape?.w ?? 0;
      const lh = leadShape?.h ?? 0;
      const selectedSet = new Set(gesture.startSnapshots.map((s) => s.id));
      const others = activeCanvas.shapes.filter((s) => !selectedSet.has(s.id));
      const tol = 4 / Math.max(camZoom, 0.0001);
      const align = computeAlignment({ x: nx, y: ny, w: lw, h: lh }, others, tol);
      if (align.snapDx !== null) { nx += align.snapDx; guideX.push(align.snapDx + nx - align.snapDx /* placeholder, replaced below */); }
      if (align.snapDy !== null) { ny += align.snapDy; }
      /* `align.lines.{vertical,horizontal}` are final canvas-coord
         lines AFTER the snap, so render them directly — no need to
         recompute. */
      guideX.length = 0;
      guideX.push(...align.lines.vertical);
      guideY.push(...align.lines.horizontal);
    }

    /* Grid-snap on each axis only if smart-guides didn't find a hit
       on that axis. Otherwise grid would fight the guide and the
       shape would visually jitter mid-drag. */
    if (!noSnap && activeCanvas) {
      if (guideX.length === 0) nx = snapToGrid(nx, activeCanvas.gridSize);
      if (guideY.length === 0) ny = snapToGrid(ny, activeCanvas.gridSize);
    }

    activeGuides = { vertical: guideX, horizontal: guideY };

    const adx = nx - lead.x;
    const ady = ny - lead.y;
    for (const snap of gesture.startSnapshots) {
      patchShape(activeCanvas.id, snap.id,
        { x: snap.x + adx, y: snap.y + ady },
        { transient: true });
    }
  }

  function finishTranslate() {
    if (!activeCanvas || !gesture || gesture.kind !== 'translate') return;
    const changes = gesture.startSnapshots.map((snap) => {
      const shape = activeCanvas!.shapes.find((s) => s.id === snap.id);
      if (!shape) return null;
      return {
        shapeId: snap.id,
        before: { x: snap.x, y: snap.y },
        after: { x: shape.x, y: shape.y }
      };
    }).filter((c): c is NonNullable<typeof c> => !!c);
    commitTransientPatches(activeCanvas.id, changes);
    /* Always clear guides on release — even if the gesture didn't
       actually move (a click-on-shape that never dragged). */
    activeGuides = { vertical: [], horizontal: [] };
  }

  /** For a moving bbox `lead` against a list of stationary `others`,
   *  return:
   *    - the smallest delta that snaps any of {leftEdge, centerX,
   *      rightEdge} of `lead` onto any of {leftEdge, centerX,
   *      rightEdge} of any other shape (within tolerance), and the
   *      same for the Y axis.
   *    - the canvas-coord lines that visualise the alignment.
   *  Lines are post-snap so the renderer doesn't have to recompute.
   *
   *  The picker chooses the BEST match per axis (smallest |delta|)
   *  rather than the first one, so a near-tie between two equally
   *  good alignments resolves predictably. */
  /* computeAlignment moved to ./canvasGeometry.ts (wave-15 split). */

  // ---- Marquee ---------------------------------------------------------

  function startMarquee(e: PointerEvent) {
    if (!activeCanvas) return;
    e.preventDefault();
    surfaceEl?.setPointerCapture(e.pointerId);
    const start = eventToCanvas(e);
    const additive = e.shiftKey;
    const base = additive ? selection.slice() : [];
    if (!additive) clearSelection(activeCanvas.id);
    gesture = { kind: 'marquee', startCanvas: start, additive, baseSelection: base };
    marquee = { ax: start.x, ay: start.y, bx: start.x, by: start.y };
  }

  function applyMarquee(currentCanvas: { x: number; y: number }) {
    if (!activeCanvas || !gesture || gesture.kind !== 'marquee') return;
    if (!marquee) return;
    marquee.bx = currentCanvas.x;
    marquee.by = currentCanvas.y;
    const rect = marqueeRect();
    if (!rect) return;
    /* Live-update selection so shapes light up under the marquee as you
       drag. Combine with the base selection for additive (⇧-drag) mode. */
    const inside = activeCanvas.shapes.filter((s) =>
      rectIntersects(rect, { x: s.x, y: s.y, w: s.w, h: s.h })
    ).map((s) => s.id);
    const next = gesture.additive
      ? Array.from(new Set([...gesture.baseSelection, ...inside]))
      : inside;
    setSelection(activeCanvas.id, next);
  }

  function finishMarquee() {
    marquee = null;
  }

  function marqueeRect() {
    if (!marquee) return null;
    return marqueeFromPoints(marquee.ax, marquee.ay, marquee.bx, marquee.by);
  }
  /* rectIntersects moved to ./canvasGeometry.ts */

  // ---- Draw ------------------------------------------------------------

  function startDraw(e: PointerEvent, kind: ShapeKind) {
    if (!activeCanvas) return;
    e.preventDefault();
    surfaceEl?.setPointerCapture(e.pointerId);
    const start = maybeSnap(eventToCanvas(e), e);
    /* Seed a 0-sized preview so the first move expands it. The preview
       isn't in canvas.shapes yet — it's drawn by a separate template
       block. Commit happens on pointer-up. */
    drawingPreview = makeShape({ kind, x: start.x, y: start.y, w: 1, h: 1 });
    gesture = { kind: 'draw', tool: kind, startCanvas: start };
  }

  function applyDraw(current: { x: number; y: number }, e: PointerEvent) {
    if (!gesture || gesture.kind !== 'draw' || !drawingPreview) return;
    const snapped = maybeSnap(current, e);
    const x = Math.min(gesture.startCanvas.x, snapped.x);
    const y = Math.min(gesture.startCanvas.y, snapped.y);
    const w = Math.abs(snapped.x - gesture.startCanvas.x);
    const h = Math.abs(snapped.y - gesture.startCanvas.y);
    drawingPreview.x = x;
    drawingPreview.y = y;
    drawingPreview.w = Math.max(1, w);
    drawingPreview.h = Math.max(1, h);
    /* For line / arrow, store endpoints in bbox-local coords mirroring
       the actual drag direction. So a top-right→bottom-left drag draws
       a line going from local (w, 0) to (0, h), not always TL→BR. */
    if (drawingPreview.kind === 'line' || drawingPreview.kind === 'arrow-shape') {
      const ax = gesture.startCanvas.x - x;
      const ay = gesture.startCanvas.y - y;
      const bx = snapped.x - x;
      const by = snapped.y - y;
      drawingPreview.props = {
        ...drawingPreview.props,
        from: { x: ax, y: ay },
        to: { x: bx, y: by }
      };
    }
  }

  /** Finalize the in-progress drawing. Tiny / zero-sized rects from a
   *  click-without-drag get a default size so the user still gets a
   *  shape (Figma does the same). Text and sticky default larger
   *  because their content needs room. */
  function finishDraw(e: PointerEvent) {
    if (!activeCanvas || !drawingPreview || !gesture || gesture.kind !== 'draw') return;
    let { x, y, w, h, kind } = drawingPreview;
    const MIN_DRAG = 4; // canvas px — below this we treat as a "click"
    const wasClick = w < MIN_DRAG && h < MIN_DRAG;
    if (wasClick) {
      switch (kind) {
        case 'rect':
        case 'ellipse':       w = 120; h = 80;  break;
        case 'line':
        case 'arrow-shape':   w = 120; h = 0.0001; break; // horizontal
        case 'text':          w = 200; h = 32;  break;
        case 'sticky':        w = 200; h = 120; break;
        case 'mermaid':       w = 320; h = 220; break;
        case 'code':          w = 320; h = 140; break;
        case 'frame':         w = 320; h = 200; break;
      }
      /* Re-center on the click point so the default-sized shape lands
         where the user clicked, not down-and-right of it. Snap to grid
         here too unless ⌘ held. */
      const cx = drawingPreview.x;
      const cy = drawingPreview.y;
      x = cx - w / 2;
      y = cy - h / 2;
      if (!(e.metaKey || e.ctrlKey)) {
        x = snapToGrid(x, activeCanvas.gridSize);
        y = snapToGrid(y, activeCanvas.gridSize);
      }
      /* Re-establish line / arrow endpoints for the click case (a
         left-to-right horizontal line). */
      if (kind === 'line' || kind === 'arrow-shape') {
        drawingPreview.props = {
          ...drawingPreview.props,
          from: { x: 0, y: h / 2 },
          to: { x: w, y: h / 2 }
        };
      }
    }
    const finalShape: Shape = {
      ...drawingPreview,
      x, y, w: Math.max(1, w), h: Math.max(0.001, h)
    };
    drawingPreview = null;
    addShape(activeCanvas.id, finalShape);
    /* Auto-select the freshly drawn shape so the user can immediately
       move / delete / resize it. Stays in select tool unless we're on
       text / sticky, where common UX is "draw, type" — but inline text
       editing isn't in M2 so we just select for now. */
    setSelection(activeCanvas.id, [finalShape.id]);
    /* Pop back to select tool after drawing — matches Figma's "draw
       once and continue iterating" rhythm. */
    setTool(instanceId, 'select');
  }

  // ---- Freehand --------------------------------------------------------

  function startFreehand(e: PointerEvent) {
    if (!activeCanvas) return;
    e.preventDefault();
    surfaceEl?.setPointerCapture(e.pointerId);
    const p = eventToCanvas(e);
    /* perfect-freehand expects (x, y, pressure). PointerEvent.pressure
       is 0 for trackpads / mice that don't report it; we substitute 0.5
       so the stroke gets a uniform-but-natural baseline thickness. */
    const pressure = e.pressure > 0 ? e.pressure : 0.5;
    gesture = { kind: 'freehand', points: [[p.x, p.y, pressure]] };
  }

  function applyFreehand(current: { x: number; y: number }, e: PointerEvent) {
    if (!gesture || gesture.kind !== 'freehand') return;
    const pressure = e.pressure > 0 ? e.pressure : 0.5;
    /* Skip near-duplicate samples — 0.5 px threshold removes browser
       jitter without losing real drawing detail. Cuts the stored point
       count by ~30% on a typical doodle. */
    const last = gesture.points[gesture.points.length - 1];
    if (last) {
      const dx = current.x - last[0];
      const dy = current.y - last[1];
      if (dx * dx + dy * dy < 0.25) return;
    }
    gesture.points = [...gesture.points, [current.x, current.y, pressure]];
  }

  function finishFreehand() {
    if (!activeCanvas || !gesture || gesture.kind !== 'freehand') return;
    const pts = gesture.points;
    if (pts.length < 2) return; /* a stray click doesn't become a stroke */

    /* Compute world AABB and pad by stroke thickness so the stroke
       outline isn't clipped at the edges of the bbox. */
    let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
    for (const [x, y] of pts) {
      if (x < minX) minX = x;
      if (y < minY) minY = y;
      if (x > maxX) maxX = x;
      if (y > maxY) maxY = y;
    }
    const pad = 6;
    const bbox = {
      x: minX - pad,
      y: minY - pad,
      w: maxX - minX + pad * 2,
      h: maxY - minY + pad * 2
    };
    /* Translate points into bbox-local coords so the shape is
       self-contained (resizing scales them as a unit). */
    const localPoints: Array<[number, number, number]> = pts.map(
      ([x, y, p]) => [x - bbox.x, y - bbox.y, p]
    );
    const shape = makeShape({
      kind: 'freehand',
      x: bbox.x, y: bbox.y, w: bbox.w, h: bbox.h,
      props: { points: localPoints, color: 'text-0', thickness: 2, smoothing: 0.5 }
    });
    addShape(activeCanvas.id, shape);
    setSelection(activeCanvas.id, [shape.id]);
    setTool(instanceId, 'select');
  }

  // ---- Edge drawing ----------------------------------------------------

  /* CanonicalAnchor + anchorWorld moved to ./canvasGeometry.ts (wave-15 split). */

  function startEdge(e: PointerEvent, shapeId: string, anchor: CanonicalAnchor) {
    if (!activeCanvas) return;
    const shape = activeCanvas.shapes.find((s) => s.id === shapeId);
    if (!shape) return;
    e.preventDefault();
    e.stopPropagation();
    surfaceEl?.setPointerCapture(e.pointerId);
    const a = anchorWorld(shape, anchor);
    gesture = {
      kind: 'edge',
      from: { shapeId, anchor },
      anchorWorld: a,
      cursorWorld: a,
      hoverShapeId: null
    };
  }

  function applyEdge(current: { x: number; y: number }, e: PointerEvent) {
    if (!gesture || gesture.kind !== 'edge') return;
    gesture.cursorWorld = current;
    /* Hit-test the cursor against shapes. For overlapping shapes we
       pick the topmost (highest z), matching what the user sees. The
       shape currently being dragged-from is excluded so we don't
       create a self-loop on the same anchor. */
    const target = e.target as Element | null;
    const shapeEl = target?.closest('[data-shape-id]') as HTMLElement | null;
    const id = shapeEl?.dataset.shapeId ?? null;
    gesture.hoverShapeId = id && id !== gesture.from.shapeId ? id : null;
  }

  function finishEdge() {
    if (!activeCanvas || !gesture || gesture.kind !== 'edge') return;
    const targetId = gesture.hoverShapeId;
    if (!targetId) return;
    const target = activeCanvas.shapes.find((s) => s.id === targetId);
    if (!target) return;
    /* Snap to the target's nearest anchor — gives the user "magnetic"
       attachment regardless of where they released inside the shape. */
    const snap = nearestAnchor(target, gesture.cursorWorld);
    const edge = makeEdge({
      from: gesture.from,
      to: { shapeId: target.id, anchor: snap }
    });
    const newId = addEdge(activeCanvas.id, edge);
    if (newId) setSelection(activeCanvas.id, [newId]);
  }

  // ---- Image (paste / pick / drop) -------------------------------------

  /** Max accepted image bytes when pasted / dropped / picked. localStorage
   *  has a ~5–10 MB hard limit per origin; a single 4 MB image would
   *  blow it instantly. We reject large pastes early with a toast and
   *  document the move-to-disk migration in CANVAS.md §11.1. */
  const MAX_IMAGE_BYTES = 1_500_000;

  /* looksLikeImage / readAsDataUrl / intrinsicFromDataUrl moved to
   * ./canvasGeometry.ts (wave-1 phase-5 split). */

  /** Drop the image on the canvas at `at` (canvas px). `at` is the
   *  desired *center* of the inserted shape, so the image lands under
   *  the cursor / drop point regardless of its size. */
  async function insertImageBlob(blob: Blob, at: { x: number; y: number }) {
    if (!activeCanvas) return;
    if (blob.size > MAX_IMAGE_BYTES) {
      // Rather than pull a toast import here we use a console hint —
      // the user's not in the right mental model to read a toast right
      // before pasting anyway. Move-to-disk migration removes this cap.
      console.warn(`[canvas] image too large (${(blob.size / 1024).toFixed(0)} KB > ${MAX_IMAGE_BYTES / 1024} KB) — paste rejected`);
      return;
    }
    const dataUrl = await readAsDataUrl(blob);
    if (!dataUrl) return;
    const { w, h } = await intrinsicFromDataUrl(dataUrl);
    const MAX_DIM = 480;
    let outW = w, outH = h;
    if (w > MAX_DIM || h > MAX_DIM) {
      const k = Math.min(MAX_DIM / w, MAX_DIM / h);
      outW = Math.round(w * k);
      outH = Math.round(h * k);
    }
    const shape = makeShape({
      kind: 'image',
      x: at.x - outW / 2,
      y: at.y - outH / 2,
      w: outW,
      h: outH,
      props: { dataUrl, intrinsicWidth: w, intrinsicHeight: h, alt: null }
    });
    addShape(activeCanvas.id, shape);
    setSelection(activeCanvas.id, [shape.id]);
  }

  /** Open the OS file picker, return the chosen image as a Blob. We use
   *  a hidden `<input type="file">` rather than tauri-plugin-dialog so
   *  this works in `pnpm dev` (a plain browser) too. */
  async function pickAndInsertImage(at: { x: number; y: number }) {
    const input = document.createElement('input');
    input.type = 'file';
    input.accept = 'image/png,image/jpeg,image/gif,image/webp,image/svg+xml';
    input.style.position = 'fixed';
    input.style.left = '-1000px';
    document.body.appendChild(input);
    await new Promise<void>((resolve) => {
      input.onchange = () => resolve();
      input.oncancel = () => resolve();
      input.click();
    });
    const file = input.files?.[0] ?? null;
    input.remove();
    if (!file) return;
    if (!looksLikeImage(file)) return;
    await insertImageBlob(file, at);
    /* Drop back to select tool so the user can drag the new image
       around without re-clicking. */
    setTool(instanceId, 'select');
  }

  function viewportCenterCanvas(): { x: number; y: number } {
    if (!surfaceEl) return { x: camX, y: camY };
    const rect = surfaceEl.getBoundingClientRect();
    return cssToCanvas(rect.width / 2, rect.height / 2);
  }

  /* Drain rail-icon drop. The Rail's Canvas icon accepts drag payloads
     outside the canvas DOM — there's no DragEvent we can use to compute
     coords, so the rail handler queues the payload and we insert here
     at viewport center once the surface mounts (or while it's already
     mounted). `seq` ensures dropping the same payload object twice
     still fires the effect. Clears via `setDragPayload(null)` + nulling
     `pendingRailDrop` so the global safety net stays consistent. */
  let lastDrainedSeq = -1;
  $effect(() => {
    const req = dragState.pendingRailDrop;
    if (!req) return;
    if (req.seq === lastDrainedSeq) return;
    lastDrainedSeq = req.seq;
    if (!activeCanvas) {
      /* No canvas to insert into — drop the request so a future valid
         drop isn't blocked behind a stale pending one. */
      dragState.pendingRailDrop = null;
      return;
    }
    /* Wait a frame so freshly-mounted surfaceEl has dimensions before
       viewportCenterCanvas() reads its bounding rect. */
    requestAnimationFrame(() => {
      void insertLiveCard(req.payload, viewportCenterCanvas());
      dragState.pendingRailDrop = null;
      setDragPayload(null);
    });
  });

  function onPaste(e: ClipboardEvent) {
    if (!activeCanvas) return;
    if (isTypingTarget()) return;
    const hovered = surfaceEl?.matches(':hover');
    if (!hovered) return;
    const items = e.clipboardData?.items;
    if (!items) return;
    for (const item of Array.from(items)) {
      if (item.kind !== 'file') continue;
      const blob = item.getAsFile();
      if (!blob || !looksLikeImage(blob)) continue;
      e.preventDefault();
      void insertImageBlob(blob, viewportCenterCanvas());
      return;
    }
  }

  /* Drop affordance — surface glows when a draggable payload is over
     the canvas (matches rail-drop behaviour). Cleared on dragleave +
     drop + dragend (window listener) so a cancelled drop doesn't keep
     glow stuck. */
  let dropActive = $state(false);
  let outerEl: HTMLElement | null = $state(null);
  function onDragEnterSurface(e: DragEvent) {
    if (!canAccept(e)) return;
    e.preventDefault();
    dropActive = true;
  }
  function onDragLeaveSurface(e: DragEvent) {
    /* Only clear when leaving the OUTER section itself, not a descendant.
       `relatedTarget` is the element the pointer enters next; if it's
       still inside `outerEl`, the leave was just a child boundary cross. */
    const related = e.relatedTarget as Node | null;
    if (related && outerEl && outerEl.contains(related)) return;
    dropActive = false;
  }
  function canAccept(e: DragEvent): boolean {
    /* Forge payloads live in module state because WKWebView hides
       custom mimes during dragover. Always accept if a payload is
       active — that's what the inbox / chat-msg drag installed. */
    if (dragState.payload) return true;
    if (!e.dataTransfer) return false;
    const types = Array.from(e.dataTransfer.types ?? []);
    if (types.includes('Files')) return true;
    if (types.includes('application/x-woom-file')) return true;
    if (types.includes('text/uri-list')) return true;
    return false;
  }
  function onDragOver(e: DragEvent) {
    /* Accept on dragover when payload is something we handle:
        file drop (image) OR Forge inbox payload (jira/github/sentry/file/chat-message). */
    if (!canAccept(e)) return;
    e.preventDefault();
    if (e.dataTransfer) e.dataTransfer.dropEffect = 'copy';
    if (!dropActive) dropActive = true;
  }

  function onDrop(e: DragEvent) {
    dropActive = false;
    if (!activeCanvas) return;

    /* Forge inbox payload first — it lives in module state because
       WebKit hides custom mimes during dragover. If a Forge payload is
       active AND files are also attached, the Forge payload wins
       (matches user intent: dragging a card from the inbox shouldn't
       silently fall through to "treat as file"). */
    const payload = dragState.payload;
    if (payload) {
      e.preventDefault();
      const at = eventToCanvas(e);
      void insertLiveCard(payload, at);
      return;
    }

    const files = Array.from(e.dataTransfer?.files ?? []);
    const imgs = files.filter(looksLikeImage);
    if (imgs.length === 0) return;
    e.preventDefault();
    const at = eventToCanvas(e);
    let i = 0;
    for (const f of imgs) {
      // Stagger multiple drops so they don't overlap exactly.
      void insertImageBlob(f, { x: at.x + i * 12, y: at.y + i * 12 });
      i++;
    }
  }

  // ---- Live-card insertion --------------------------------------------

  /** Build the right shape kind for an inbox drag payload. The card's
   *  bbox is sized to the source-column card dimensions so the visual
   *  feels like the card "lifted off" the inbox onto the canvas;
   *  CARD_W × CARD_H matches the typical inbox-card height. */
  function insertLiveCard(payload: DragPayload, at: { x: number; y: number }) {
    if (!activeCanvas) return;
    const shape = buildLiveCardShape(payload, at);
    if (!shape) return;
    addShape(activeCanvas.id, shape);
    setSelection(activeCanvas.id, [shape.id]);
  }

  // ---- Resize handles --------------------------------------------------

  /** A handle drags the shape's bbox. We compute the new (x, y, w, h)
   *  from the handle's anchor — e.g. dragging the top-left moves x and
   *  y, and reduces w / h by the same delta. */
  function startResize(e: PointerEvent, handle: HandleId) {
    if (!activeCanvas) return;
    if (selection.length !== 1) return;
    const shapeId = selection[0];
    const shape = activeCanvas.shapes.find((s) => s.id === shapeId);
    if (!shape || shape.locked) return;
    e.preventDefault();
    e.stopPropagation();
    surfaceEl?.setPointerCapture(e.pointerId);
    const before = { x: shape.x, y: shape.y, w: shape.w, h: shape.h };
    const startCanvas = eventToCanvas(e);
    gesture = { kind: 'resize', handle, shapeId, before, startCanvas };
  }

  function applyResize(current: { x: number; y: number }, e: PointerEvent) {
    if (!activeCanvas || !gesture || gesture.kind !== 'resize') return;
    const { handle, shapeId, before, startCanvas } = gesture;
    const dx = current.x - startCanvas.x;
    const dy = current.y - startCanvas.y;
    const grid = (e.metaKey || e.ctrlKey) ? 0 : activeCanvas.gridSize;
    const { x: nx, y: ny, w: nw, h: nh } = computeResize(handle, before, dx, dy, grid);

    /* Scale shape-internal point sets proportionally so they follow
       the bbox. Line / arrow have endpoint props; freehand has a list
       of stroke samples. All other kinds (rect, ellipse, text, sticky,
       mermaid, code, image) just resize the bbox — their renderers
       handle the scaling internally. */
    const shape = activeCanvas.shapes.find((s) => s.id === shapeId);
    const extra = rescaleShapePoints(shape, before, { w: nw, h: nh });

    patchShape(activeCanvas.id, shapeId,
      { x: nx, y: ny, w: nw, h: nh, ...extra },
      { transient: true });
  }

  function finishResize() {
    if (!activeCanvas || !gesture || gesture.kind !== 'resize') return;
    /* Narrow to a local const so the type stays a `resize` gesture
       inside the `find` callback (Svelte's reactive `gesture` is mutable
       and TS widens it across closures otherwise). */
    const g = gesture;
    const shape = activeCanvas.shapes.find((s) => s.id === g.shapeId);
    if (!shape) return;
    commitTransientPatches(activeCanvas.id, [{
      shapeId: shape.id,
      before: g.before,
      after: { x: shape.x, y: shape.y, w: shape.w, h: shape.h, props: shape.props }
    }]);
  }

  // ---- Pointer move / up dispatch -------------------------------------

  function onPointerMove(e: PointerEvent) {
    if (!gesture || !activeCanvas) return;
    const cur = eventToCanvas(e);
    /* For translate gestures, hold off on dispatching to applyTranslate
       (and on acquiring pointer capture) until the pointer has clearly
       escaped a dead-zone around the press point. This lets a still
       click+release fire `click` / `dblclick` on the shape (capture
       never activates, target stays at the shape DOM), while a real
       drag still gets lossless tracking the moment it crosses the
       threshold. Only translate uses this — resize / draw / freehand /
       marquee / edge are all started by intentional gestures so they
       capture immediately at start-time. */
    if (gesture.kind === 'translate' && pendingCaptureId === e.pointerId) {
      const dx = cur.x - gesture.startCanvas.x;
      const dy = cur.y - gesture.startCanvas.y;
      /* Threshold is in canvas (world) units, but the deadzone is
         expressed in screen px so it feels constant regardless of
         zoom — divide by camZoom. */
      const tol = DRAG_DEADZONE_PX / Math.max(camZoom, 0.0001);
      if (Math.hypot(dx, dy) <= tol) return;
      try { surfaceEl?.setPointerCapture(e.pointerId); } catch { /* ignore */ }
      pendingCaptureId = null;
    }
    switch (gesture.kind) {
      case 'translate': applyTranslate(cur, e); break;
      case 'resize':    applyResize(cur, e); break;
      case 'draw':      applyDraw(cur, e); break;
      case 'freehand':  applyFreehand(cur, e); break;
      case 'edge':      applyEdge(cur, e); break;
      case 'marquee':   applyMarquee(cur); break;
    }
  }

  function onPointerUp(e: PointerEvent) {
    if (!gesture) return;
    /* Drop the deferred-capture marker — gesture is over either way
       (click-without-drag OR drag completed), so there's nothing left
       to defer. */
    if (pendingCaptureId === e.pointerId) pendingCaptureId = null;
    try { surfaceEl?.releasePointerCapture(e.pointerId); } catch { /* ignore */ }
    switch (gesture.kind) {
      case 'translate': finishTranslate(); break;
      case 'resize':    finishResize(); break;
      case 'draw':      finishDraw(e); break;
      case 'freehand':  finishFreehand(); break;
      case 'edge':      finishEdge(); break;
      case 'marquee':   finishMarquee(); break;
    }
    gesture = null;
  }

  // ---- Keyboard --------------------------------------------------------

  // isTypingTarget moved to ./canvasGeometry (wave-29).

  function onKeyDown(e: KeyboardEvent) {
    /* Tool shortcuts: only fire when the canvas surface is hovered or
       focused. Otherwise typing 'r' in another input would switch
       tools and lose the keystroke. */
    const hovered = surfaceEl?.matches(':hover');
    const targetTyping = isTypingTarget();

    /* Space: pan-mode toggle. Suppressed inside text inputs so typing
       a space character doesn't fight the pan gesture. */
    if (e.code === 'Space' && !spaceHeld) {
      if (targetTyping) return;
      if (!hovered) return;
      spaceHeld = true;
      e.preventDefault();
      return;
    }

    if (targetTyping) return;

    /* Undo / redo — fire whenever a canvas is open, regardless of
       hover, so the user can ⌘Z right after dropping a shape without
       worrying about the cursor position. */
    if ((e.metaKey || e.ctrlKey) && (e.key === 'z' || e.key === 'Z')) {
      if (!activeCanvas) return;
      e.preventDefault();
      if (e.shiftKey) redo(activeCanvas.id); else undo(activeCanvas.id);
      return;
    }
    if ((e.metaKey || e.ctrlKey) && (e.key === 'y' || e.key === 'Y')) {
      if (!activeCanvas) return;
      e.preventDefault();
      redo(activeCanvas.id);
      return;
    }

    /* ⌘P opens the library overlay when the canvas surface is hovered.
       Gated on hover so two canvas columns don't both pop their
       library at the same time, and so it doesn't fight ⌘P in other
       host contexts (Print). */
    if ((e.metaKey || e.ctrlKey) && (e.key === 'p' || e.key === 'P')) {
      if (!hovered) return;
      e.preventDefault();
      libraryOpen = !libraryOpen;
      return;
    }

    if (!hovered) return;

    /* Single-key tool toggles. Skip when ⌘/⌃ is held — ⌘R reloads,
       ⌘T opens new tab in the host shell, etc. We never want to steal
       those from the OS. */
    if (e.metaKey || e.ctrlKey || e.altKey) return;

    if (e.key === 'v' || e.key === 'V') { setTool(instanceId, 'select'); return; }
    if (e.key === 'r' || e.key === 'R') { setTool(instanceId, 'rect'); return; }
    if (e.key === 'o' || e.key === 'O') { setTool(instanceId, 'ellipse'); return; }
    if (e.key === 'l' || e.key === 'L') { setTool(instanceId, 'line'); return; }
    if (e.key === 'a' || e.key === 'A') { setTool(instanceId, 'arrow'); return; }
    if (e.key === 't' || e.key === 'T') { setTool(instanceId, 'text'); return; }
    if (e.key === 's' || e.key === 'S') { setTool(instanceId, 'sticky'); return; }
    if (e.key === 'p' || e.key === 'P') { setTool(instanceId, 'freehand'); return; }
    if (e.key === 'f' || e.key === 'F') { setTool(instanceId, 'frame'); return; }
    if (e.key === 'm' || e.key === 'M') {
      /* `m` toggles the minimap on a quick tap (no Shift); pair with
         a tool letter only when ⇧M is held — this keeps `m` muscle
         memory ("toggle minimap") aligned with how Figma uses it,
         while preserving the Mermaid tool shortcut behind ⇧M.  */
      if (e.shiftKey) { setTool(instanceId, 'mermaid'); return; }
      toggleMinimap();
      return;
    }
    if (e.key === 'c' || e.key === 'C') { setTool(instanceId, 'code'); return; }
    if (e.key === 'i' || e.key === 'I') { setTool(instanceId, 'image'); return; }

    if ((e.key === 'Delete' || e.key === 'Backspace') && activeCanvas && selection.length > 0) {
      e.preventDefault();
      /* Selection holds both shape ids and edge ids; dispatch to the
         right deleter for each. Edge ids first so we don't double-delete
         edges that would otherwise cascade from the shape removal. */
      const edgeIds = selectedEdgeIds.slice();
      const shapeIds = selectedShapes.map((s) => s.id);
      if (edgeIds.length > 0) deleteEdges(activeCanvas.id, edgeIds);
      if (shapeIds.length > 0) deleteShapes(activeCanvas.id, shapeIds);
      return;
    }
    if (e.key === 'Escape') {
      if (gesture?.kind === 'draw') {
        drawingPreview = null;
        gesture = null;
        return;
      }
      if (gesture?.kind === 'freehand') {
        gesture = null;
        return;
      }
      if (gesture?.kind === 'edge') {
        gesture = null;
        return;
      }
      if (activeCanvas && selection.length > 0) clearSelection(activeCanvas.id);
      return;
    }
  }

  function onKeyUp(e: KeyboardEvent) {
    if (e.code === 'Space') spaceHeld = false;
  }

  /* ⌘D — duplicate selection. Separate from the main key dispatcher
     because it needs to fire on cmd-key combos which are filtered out
     above to avoid stealing OS shortcuts. */
  function onKeyDownDuplicate(e: KeyboardEvent) {
    if (!(e.metaKey || e.ctrlKey)) return;
    if (e.key !== 'd' && e.key !== 'D') return;
    const hovered = surfaceEl?.matches(':hover');
    if (!hovered) return;
    if (isTypingTarget()) return;
    if (!activeCanvas || selection.length === 0) return;
    e.preventDefault();
    duplicateShapes(activeCanvas.id, selection);
  }

  /* ⌘G / ⇧⌘G — group / ungroup selection. Same gating as duplicate
     (hover + not typing). Group needs ≥1 shape; ungroup operates on
     the first selected frame/group. */
  function onKeyDownGroup(e: KeyboardEvent) {
    if (!(e.metaKey || e.ctrlKey)) return;
    if (e.key !== 'g' && e.key !== 'G') return;
    const hovered = surfaceEl?.matches(':hover');
    if (!hovered) return;
    if (isTypingTarget()) return;
    if (!activeCanvas) return;
    e.preventDefault();
    if (e.shiftKey) {
      /* Ungroup — find the first selected shape that's a frame/group
         and unwrap it. Multi-select ungroup is not needed often;
         users normally have one container selected. */
      const target = selectedShapes.find((s) => s.kind === 'frame' || s.kind === 'group');
      if (target) ungroupShapes(activeCanvas.id, target.id);
      return;
    }
    /* Group: need at least 2 shapes to make grouping meaningful (a
       single shape in a frame is weird). Locked shapes pass through. */
    const ids = selectedShapes.map((s) => s.id);
    if (ids.length < 2) return;
    groupShapes(activeCanvas.id, ids);
  }

  /* Tick for the auto-save indicator. We can't use the global
   * `now` from +page.svelte because it's parent-owned; rolling our
   * own keeps this column self-contained. 200 ms tick is plenty for
   * a 1.2 s pulse window. */
  let nowTick = $state(Date.now());
  let saveTickTimer: ReturnType<typeof setInterval> | null = null;

  onMount(() => {
    saveTickTimer = setInterval(() => (nowTick = Date.now()), 200);
    window.addEventListener('keydown', onKeyDown);
    window.addEventListener('keydown', onKeyDownDuplicate);
    window.addEventListener('keydown', onKeyDownGroup);
    window.addEventListener('keyup', onKeyUp);
    /* Paste fires at the document level — listen there and bail unless
       the canvas surface is hovered (mirrors the keyboard-shortcut
       gate so multiple canvas columns don't fight over the same
       paste). */
    window.addEventListener('paste', onPaste);
    const clearDropAffordance = () => { dropActive = false; };
    window.addEventListener('dragend', clearDropAffordance);
    window.addEventListener('drop', clearDropAffordance);
    return () => {
      if (saveTickTimer) clearInterval(saveTickTimer);
      window.removeEventListener('keydown', onKeyDown);
      window.removeEventListener('keydown', onKeyDownDuplicate);
      window.removeEventListener('keydown', onKeyDownGroup);
      window.removeEventListener('keyup', onKeyUp);
      window.removeEventListener('paste', onPaste);
      window.removeEventListener('dragend', clearDropAffordance);
      window.removeEventListener('drop', clearDropAffordance);
    };
  });

  // ---- Camera CSS ------------------------------------------------------

  const stageTransform = $derived(
    `translate(${-camX * camZoom}px, ${-camY * camZoom}px) scale(${camZoom})`
  );
  const bgSize = $derived(((activeCanvas?.gridSize ?? 8) * camZoom));
  const bgPosX = $derived(-camX * camZoom);
  const bgPosY = $derived(-camY * camZoom);
  const bgKind = $derived(activeCanvas?.background ?? 'dot');
  const showOrigin = $derived(camZoom >= 0.4);
  const zoomPct = $derived(Math.round(camZoom * 100));
  const shapeCount = $derived(activeCanvas?.shapes.length ?? 0);

  /* Counter-zoom factor for handles / marquee borders. */
  const cz = $derived(1 / Math.max(camZoom, 0.0001));

  /* Whether resize handles render. Single-shape selection only in M2 —
     multi-select group resize is more subtle and lands later. */
  const showResizeHandles = $derived(
    selectedShapes.length === 1 && tool === 'select'
  );
  /* Edge anchors render in the same circumstance as resize handles —
     a single shape selected in select mode. Drag from a cardinal anchor
     to another shape to draw an edge (see `startEdge`). */
  const showEdgeAnchors = $derived(showResizeHandles);
  const singleSelectedShape = $derived(
    selectedShapes.length === 1 ? selectedShapes[0] : null
  );

  const anchorStyle = (a: 'tc' | 'mr' | 'bc' | 'ml', shape: Shape) =>
    anchorStyleCss(a, shape, cz);

  /** Edge-preview state in render-friendly shape. Keeps the template
   *  free of nested closures over the reactive `gesture` (which TS
   *  refuses to narrow across find callbacks). Returns null when no
   *  edge gesture is in flight. */
  const edgePreview = $derived.by<
    | null
    | {
        anchor: { x: number; y: number };
        cursor: { x: number; y: number };
        hovered: Shape | null;
      }
  >(() => {
    if (!gesture || gesture.kind !== 'edge' || !activeCanvas) return null;
    const hoverId = gesture.hoverShapeId;
    const hovered = hoverId
      ? activeCanvas.shapes.find((s) => s.id === hoverId) ?? null
      : null;
    return {
      anchor: gesture.anchorWorld,
      cursor: gesture.cursorWorld,
      hovered
    };
  });

  // ---- Minimap helpers --------------------------------------------------

  /** Viewport rect in canvas-pixel coordinates. The minimap reads the
   *  surface size on every render (computed lazily because it can change
   *  when the user resizes the column or the window). Falls back to a
   *  reasonable default when the surface ref isn't bound yet. */
  const minimapViewport = $derived.by(() => {
    if (!surfaceEl) return { x: camX, y: camY, w: 600, h: 400 };
    const rect = surfaceEl.getBoundingClientRect();
    return {
      x: camX,
      y: camY,
      w: rect.width / camZoom,
      h: rect.height / camZoom
    };
  });

  /** Center the camera on `(canvasX, canvasY)` — invoked by minimap
   *  click and drag. Preserves the current zoom so users don't lose
   *  context after teleporting. */
  function onMinimapTeleport(canvasX: number, canvasY: number) {
    if (!surfaceEl) return;
    const rect = surfaceEl.getBoundingClientRect();
    camX = canvasX - rect.width / (2 * camZoom);
    camY = canvasY - rect.height / (2 * camZoom);
    pushViewport();
  }

  // ---- Layout actions --------------------------------------------------

  /** Run a layout algorithm on the current selection (shape ids only —
   *  edges aren't laid out). Falls back to "all root shapes" when the
   *  selection has nothing or only edges in it. */
  function runLayout(algo: LayoutAlgorithm) {
    if (!activeCanvas) return;
    const ids = selectedShapes.length > 0
      ? selectedShapes.map((s) => s.id)
      : undefined;
    void applyLayout(activeCanvas.id, algo, ids);
  }

  /* Cursor for the surface — pan beats everything, then per-tool. */
  const surfaceCursor = $derived.by(() => {
    if (panActive || spaceHeld) return 'grab';
    if (gesture?.kind === 'translate') return 'move';
    if (gesture?.kind === 'resize') return 'crosshair';
    switch (tool) {
      case 'select':  return 'default';
      case 'text':
      case 'sticky':  return 'text';
      default:        return 'crosshair';
    }
  });

  const handleIds = HANDLE_IDS;

  const handleStyle = (handle: HandleId, box: { x: number; y: number; w: number; h: number }) =>
    handleStyleCss(handle, box, cz);

</script>

<section
  bind:this={outerEl}
  class="canvas-surface canvas-surface--outer"
  class:canvas-surface--drop={dropActive}
  data-instance-id={instanceId}
  data-kind="canvas"
  ondragenter={onDragEnterSurface}
  ondragleave={onDragLeaveSurface}
  ondragover={onDragOver}
  ondrop={onDrop}
>

  {#if !activeCanvas}
    <div class="empty-state">
      <div class="empty-glyph" aria-hidden="true">
        <svg viewBox="0 0 64 64"><rect x="6" y="10" width="52" height="36" rx="4"/><circle cx="20" cy="26" r="3"/><circle cx="32" cy="26" r="3"/><circle cx="44" cy="26" r="3"/><circle cx="20" cy="38" r="3"/><circle cx="32" cy="38" r="3"/><circle cx="44" cy="38" r="3"/></svg>
      </div>
      <h3 class="empty-title">No canvas open</h3>
      <p class="empty-sub">A canvas is an infinite plane for diagrams, sketches, and live cards. Create one and your linked agent can draw on it with you.</p>
      <button class="btn btn--primary" onclick={onCreateNew}>+ New canvas</button>
    </div>
  {:else}
    <div
      class="canvas-surface"
      bind:this={surfaceEl}
      onwheel={onWheel}
      onpointerdown={onPointerDown}
      onpointermove={onPointerMove}
      onpointerup={onPointerUp}
      onpointercancel={onPointerUp}
      role="presentation"
      style="
        --cv-bg-size: {bgSize}px;
        --cv-bg-x: {bgPosX}px;
        --cv-bg-y: {bgPosY}px;
        cursor: {surfaceCursor};
      "
      data-bg={bgKind}
    >
      <CanvasToolbar
        {tool}
        onSelect={(t) => setTool(instanceId, t)}
        onUndo={() => activeCanvas && undo(activeCanvas.id)}
        onRedo={() => activeCanvas && redo(activeCanvas.id)}
        canUndo={activeCanvas ? canUndoFor(activeCanvas.id) : false}
        canRedo={activeCanvas ? canRedoFor(activeCanvas.id) : false}
        onLayout={runLayout}
      />
      <div class="stage" style="transform: {stageTransform}">
        {#if showOrigin}
          <div class="origin-mark" aria-hidden="true">
            <span class="origin-x"></span>
            <span class="origin-y"></span>
            <span class="origin-label mono">0,0</span>
          </div>
        {/if}
        <!-- Edges render BENEATH shapes so a card visually sits "on top
             of" its connectors. The hit paths inside CanvasEdges still
             accept clicks because they live in their own SVG with
             `pointer-events: stroke` per path (the wrapper SVG itself
             is `pointer-events: none`). -->
        <CanvasEdges canvas={activeCanvas} {selectedEdgeIds} zoom={camZoom} />

        {#each activeCanvas.shapes as shape (shape.id)}
          <CanvasShape
            {shape}
            selected={selection.includes(shape.id)}
            dim={false}
            zoom={camZoom}
            canvasId={activeCanvas.id}
            {onCardOpen}
          />
        {/each}
        {#if drawingPreview}
          <CanvasShape
            shape={drawingPreview}
            selected={false}
            dim={true}
            zoom={camZoom}
            canvasId={null}
          />
        {/if}
        {#if freehandPreview}
          <CanvasShape
            shape={freehandPreview}
            selected={false}
            dim={false}
            zoom={camZoom}
            canvasId={null}
          />
        {/if}
        {#if marquee}
          {@const r = marqueeRect()}
          {#if r}
            <div
              class="cv-marquee"
              style="
                left: {r.x}px;
                top: {r.y}px;
                width: {r.w}px;
                height: {r.h}px;
                border-width: {1 * cz}px;
              "
              aria-hidden="true"
            ></div>
          {/if}
        {/if}
        {#if selectionBox}
          <div
            class="cv-selection-box"
            style="
              left: {selectionBox.x}px;
              top: {selectionBox.y}px;
              width: {selectionBox.w}px;
              height: {selectionBox.h}px;
              border-width: {1 * cz}px;
            "
            aria-hidden="true"
          ></div>
          {#if showResizeHandles}
            {#each handleIds as h (h)}
              <button
                class="cv-handle"
                data-cv-handle={h}
                style="{handleStyle(h, selectionBox)} cursor: {handleCursor(h)};"
                aria-label={`Resize ${h}`}
              ></button>
            {/each}
          {/if}

          {#if showEdgeAnchors && singleSelectedShape}
            <!-- Four cardinal anchors around the single-selected shape's
                 bbox. Drag one to the next shape to draw an edge. We
                 render only N/E/S/W (not the corners) to keep the
                 visual quiet — the corners are still reachable through
                 the agent's MCP tools when needed. -->
            {#each (['tc', 'mr', 'bc', 'ml'] as const) as a (a)}
              <button
                class="cv-anchor"
                data-cv-anchor={a}
                data-cv-anchor-shape={singleSelectedShape.id}
                style={anchorStyle(a, singleSelectedShape)}
                aria-label={`Drag to connect from ${a}`}
                title="Drag to another shape to draw an edge"
              ></button>
            {/each}
          {/if}
        {/if}

        {#if activeGuides.vertical.length > 0 || activeGuides.horizontal.length > 0}
          <!-- Smart-guide alignment lines. Rendered just before the
               edge-preview so they sit ABOVE shapes (visible) but
               BELOW the edge preview / handles. Counter-zoomed to
               stay 1px on screen. The lines are nominally infinite —
               we just paint them long enough (~1e5 canvas px each
               way from the guide's anchor) that the user never sees
               them end. -->
          <svg class="cv-guides" overflow="visible">
            {#each activeGuides.vertical as gx (gx)}
              <line
                x1={gx} y1={-100000}
                x2={gx} y2={100000}
                stroke="var(--accent)"
                stroke-width={1 * cz}
                stroke-dasharray={`${4 * cz} ${4 * cz}`}
                vector-effect="non-scaling-stroke"
              />
            {/each}
            {#each activeGuides.horizontal as gy (gy)}
              <line
                x1={-100000} y1={gy}
                x2={100000} y2={gy}
                stroke="var(--accent)"
                stroke-width={1 * cz}
                stroke-dasharray={`${4 * cz} ${4 * cz}`}
                vector-effect="non-scaling-stroke"
              />
            {/each}
          </svg>
        {/if}

        {#if edgePreview}
          <!-- Live preview of the in-flight edge. Renders as a dashed
               line from the source anchor to the cursor; if a target
               shape is hovered, we tint the preview accent so the user
               sees the drop will land there. -->
          <svg class="cv-edge-preview" overflow="visible">
            <path
              d={`M ${edgePreview.anchor.x.toFixed(2)} ${edgePreview.anchor.y.toFixed(2)} L ${edgePreview.cursor.x.toFixed(2)} ${edgePreview.cursor.y.toFixed(2)}`}
              stroke="var(--accent)"
              stroke-width={2 * cz}
              stroke-dasharray={`${5 * cz} ${4 * cz}`}
              fill="none"
              vector-effect="non-scaling-stroke"
              pointer-events="none"
            />
          </svg>
          {#if edgePreview.hovered}
            <div
              class="cv-edge-target-ring"
              style="
                left: {edgePreview.hovered.x}px;
                top: {edgePreview.hovered.y}px;
                width: {edgePreview.hovered.w}px;
                height: {edgePreview.hovered.h}px;
                outline-width: {2 * cz}px;
                outline-offset: {3 * cz}px;
              "
              aria-hidden="true"
            ></div>
          {/if}
        {/if}
      </div>

      {#if minimapVisible && activeCanvas}
        <CanvasMinimap
          shapes={activeCanvas.shapes}
          viewport={minimapViewport}
          onTeleport={onMinimapTeleport}
          onClose={() => (minimapVisible = false)}
        />
      {/if}
    </div>

    <footer class="canvas-status mono">
      <span>{shapeCount} shape{shapeCount === 1 ? '' : 's'}</span>
      <span class="dot">·</span>
      <span>{zoomPct}%</span>
      <span class="dot">·</span>
      <span class="hint">Space drag pan · ⌘ scroll zoom · M minimap · ⌘P library · ⌘Z undo</span>
    </footer>
  {/if}

  {#if libraryOpen}
    <CanvasLibrary
      {instanceId}
      {activeCanvasId}
      onClose={closeLibrary}
    />
  {/if}
</section>

<style>
  .canvas-surface {
    background: var(--bg-0);
    width: 100%; height: 100%;
    flex: 1 1 auto; min-width: 0; min-height: 0;
    display: flex;
    flex-direction: column;
    /* Library overlay (`.cv-library`) is `position: absolute; inset: 0`
       so a positioned ancestor anchors it to this app's box. */
    position: relative;
    overflow: hidden;
  }

  /* ---- Empty state -------------------------------------------------- */

  .empty-state { flex: 1; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 12px; padding: 48px 24px; text-align: center; color: var(--text-1); }
  .empty-glyph svg { width: 56px; height: 56px; stroke: var(--text-2); fill: none; stroke-width: 1.4; }
  .empty-glyph circle { fill: var(--text-2); stroke: none; }
  .empty-title { margin: 0; font-size: 16px; color: var(--text-0); font-weight: 600; }
  .empty-sub { margin: 0; max-width: 320px; font-size: 13px; line-height: 1.5; color: var(--text-1); }

  /* ---- Drawing surface --------------------------------------------- */

  .canvas-surface {
    flex: 1;
    position: relative;
    overflow: hidden;
    background: var(--bg-0);
    touch-action: none;
  }
  /* Dot / grid backgrounds. Color is dimmed via color-mix so the
     pattern is perceptible but doesn't compete with shape outlines —
     earlier the raw `--text-mute` was too contrasty against the dark
     surface, especially around freshly drawn cards. The .35-ish
     opacity matches what tldraw / Figma use for similar grids. */
  .canvas-surface[data-bg='dot'] {
    background-image: radial-gradient(
      circle at 1px 1px,
      color-mix(in srgb, var(--text-mute) 28%, transparent) 1px,
      transparent 1.5px
    );
    background-size: var(--cv-bg-size) var(--cv-bg-size);
    background-position: var(--cv-bg-x) var(--cv-bg-y);
  }
  .canvas-surface[data-bg='grid'] {
    background-image:
      linear-gradient(to right, color-mix(in srgb, var(--border-neutral) 55%, transparent) 1px, transparent 1px),
      linear-gradient(to bottom, color-mix(in srgb, var(--border-neutral) 55%, transparent) 1px, transparent 1px);
    background-size: var(--cv-bg-size) var(--cv-bg-size);
    background-position: var(--cv-bg-x) var(--cv-bg-y);
  }
  .canvas-surface[data-bg='plain'] { background-image: none; }

  /* Drop affordance — accent ring + inner glow when draggable
     payload over canvas. Matches rail-drop visual so interaction
     feels uniform across solos. */
  .canvas-surface--outer.canvas-surface--drop {
    box-shadow:
      inset 0 0 0 3px var(--accent),
      inset 0 0 60px color-mix(in srgb, var(--accent) 28%, transparent);
    transition: box-shadow 120ms ease;
  }
  .canvas-surface--outer.canvas-surface--drop::after {
    content: 'Drop to add to canvas';
    position: absolute;
    top: 14px; left: 50%; transform: translateX(-50%);
    padding: 6px 14px;
    border-radius: 999px;
    background: var(--accent);
    color: #fff;
    font-size: 12px; font-weight: 600;
    pointer-events: none;
    z-index: 2000;
    box-shadow: 0 6px 18px rgba(0, 0, 0, 0.4);
  }

  .stage {
    position: absolute;
    top: 0;
    left: 0;
    width: 0;
    height: 0;
    transform-origin: 0 0;
    will-change: transform;
  }

  .origin-mark { position: absolute; top: 0; left: 0; width: 0; height: 0; pointer-events: none; color: var(--text-mute); }
  .origin-x, .origin-y { position: absolute; background: currentColor; opacity: 0.6; }
  .origin-x { left: -8px; top: -0.5px; width: 16px; height: 1px; }
  .origin-y { top: -8px; left: -0.5px; height: 16px; width: 1px; }
  .origin-label { position: absolute; top: 4px; left: 4px; font-size: 9px; color: var(--text-mute); opacity: 0.7; }

  /* ---- Marquee + selection box + handles --------------------------- */

  .cv-marquee {
    position: absolute;
    border-style: dashed;
    border-color: var(--accent);
    background: rgba(232, 130, 100, 0.08);
    pointer-events: none;
    box-sizing: border-box;
  }

  .cv-selection-box {
    position: absolute;
    border-style: solid;
    border-color: var(--accent);
    pointer-events: none;
    box-sizing: border-box;
  }

  .cv-handle {
    position: absolute;
    background: var(--bg-0);
    border-style: solid;
    border-color: var(--accent);
    border-radius: 2px;
    box-sizing: border-box;
    padding: 0;
    /* Hit target slightly bigger via padding-box logic — keep border
       crisp via box-shadow inset, expand actual click area a touch. */
  }
  .cv-handle:hover { background: var(--accent); }

  /* Edge anchors — round, semi-translucent. Hover pops them so the
     drag affordance is unmistakable. Sit on top of resize handles
     (via z-index) when both render at the same anchor position. */
  .cv-anchor {
    position: absolute;
    background: var(--bg-1);
    border-style: solid;
    border-color: var(--accent);
    border-radius: 50%;
    cursor: crosshair;
    padding: 0;
    box-sizing: border-box;
    z-index: 5;
    transition: background 80ms, transform 80ms;
  }
  .cv-anchor:hover {
    background: var(--accent);
    transform: scale(1.18);
  }

  /* Preview line shown while the user drags from an anchor. SVG itself
     is 0×0 and overflow:visible — same trick we use for CanvasEdges so
     the path can paint anywhere in canvas-space. */
  .cv-edge-preview {
    position: absolute;
    top: 0;
    left: 0;
    width: 0;
    height: 0;
    pointer-events: none;
  }
  /* Smart-guide layer — same 0×0 trick. Sits above shapes via the
     stacking context's natural order; below resize handles because
     the handles render later in the template. */
  .cv-guides {
    position: absolute;
    top: 0;
    left: 0;
    width: 0;
    height: 0;
    pointer-events: none;
  }

  /* Highlight ring around the target shape while an edge is being
     drawn over it. Same outline trick as the selection ring. */
  .cv-edge-target-ring {
    position: absolute;
    pointer-events: none;
    outline-style: dashed;
    outline-color: var(--accent);
    border-radius: 8px;
    box-sizing: border-box;
  }

  /* ---- Status bar -------------------------------------------------- */

  .canvas-status {
    flex-shrink: 0;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 6px 14px;
    border-top: 1px solid var(--border-neutral);
    background: var(--bg-1);
    font-size: 11px;
    color: var(--text-2);
  }
  .canvas-status .dot { color: var(--text-mute); }
  .canvas-status .hint { margin-left: auto; color: var(--text-mute); font-family: inherit; }
</style>
