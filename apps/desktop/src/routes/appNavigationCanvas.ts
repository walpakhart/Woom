// Canvas + SDD MCP-tool dispatchers extracted from
// `handleAppNavigation` in `+page.svelte` (wave-31 split). Every case
// resolves through `linkedCanvasIdFor` (passed in via deps) and
// delegates to the matching canvas-store operation. SDD cases fan
// out to the same SDD store helpers the SddCard buttons call.
//
// The caller hands in a `deps` object built once at page-script
// setup; this module never imports from `+page.svelte` so the
// dependency graph stays clean. Returns `true` when a case matched
// (caller skips the rest of the switch), `false` otherwise.

import {
  num as _mcpNum,
  parseEdgeSpec,
  pickFrom,
  str as _mcpStr,
} from './mcpInputParse';
import {
  addEdge as canvasAddEdge,
  addShape as canvasAddShape,
  addShapes as canvasAddShapes,
  alignShapes as canvasAlignShapes,
  deleteEdges as canvasDeleteEdges,
  deleteShapes as canvasDeleteShapes,
  distributeShapes as canvasDistributeShapes,
  duplicateShapes as canvasDuplicateShapes,
  ensureCanvasLoaded,
  findShapesByQuery as canvasFindShapes,
  groupShapes as canvasGroupShapes,
  makeShape,
  patchShape as canvasPatchShape,
  requestCanvasFocus,
  setSelection as canvasSetSelection,
  setShapesLocked as canvasSetShapesLocked,
  setShapeZ as canvasSetShapeZ,
  setViewport as canvasSetViewport,
  ungroupShapes as canvasUngroupShapes,
  type AlignAxis,
  type DistributeAxis,
  type Shape,
  type ShapeKind,
} from '$lib/state/canvas.svelte';
import { applyLayout as canvasApplyLayout, type LayoutAlgorithm } from '$lib/services/canvasLayout';
import {
  approveSddPhasePlan,
  completeSddPhaseImplement,
  discardSddPhasePlan,
  saveSddPhasePlan,
  saveSddPhaseVerify,
} from '$lib/state/sdd.svelte';

export interface CanvasMcpDeps {
  /** Returns the canvas id the session is linked to, or `null` when
   *  no link exists (case becomes a no-op). */
  linkedCanvasIdFor(sessionId: string): string | null;
}

/** Try to handle a canvas-prefixed or SDD-prefixed MCP tool call.
 *  Returns `true` when a case matched. The original
 *  `handleAppNavigation` switch keeps its other cases above this
 *  call and only delegates here for the canvas/SDD branches. */
export function handleCanvasOrSddMcp(
  sessionId: string,
  name: string,
  input: Record<string, unknown>,
  deps: CanvasMcpDeps
): boolean {
  const str = (k: string): string => _mcpStr(input, k);
  const num = (k: string): number => _mcpNum(input, k);

  switch (name) {
    /* ---- Canvas (whiteboard) ---- */
    case 'mcp__app__canvas_add_shape': {
      const canvasId = deps.linkedCanvasIdFor(sessionId);
      if (!canvasId) return true;
      const kind = str('kind') as ShapeKind;
      if (!kind) return true;
      const x = num('x'); const y = num('y');
      const w = num('w'); const h = num('h');
      if (!Number.isFinite(x) || !Number.isFinite(y) || !(w > 0) || !(h > 0)) return true;
      const props = (input.props && typeof input.props === 'object')
        ? (input.props as Record<string, unknown>)
        : undefined;
      const label = typeof input.label === 'string' ? (input.label as string) : null;
      const desiredId = str('shape_id');
      const shape = makeShape({
        kind, x, y, w, h, props, label, createdBy: 'agent'
      });
      if (desiredId) shape.id = desiredId;
      canvasAddShape(canvasId, shape);
      return true;
    }
    case 'mcp__app__canvas_add_shapes': {
      const canvasId = deps.linkedCanvasIdFor(sessionId);
      if (!canvasId) return true;
      const arr = Array.isArray(input.shapes) ? input.shapes : [];
      const shapes: Shape[] = [];
      for (const raw of arr) {
        if (!raw || typeof raw !== 'object') continue;
        const s = raw as Record<string, unknown>;
        const kind = typeof s.kind === 'string' ? s.kind as ShapeKind : null;
        if (!kind) continue;
        const x = Number(s.x); const y = Number(s.y);
        const w = Number(s.w); const h = Number(s.h);
        if (!Number.isFinite(x) || !Number.isFinite(y) || !(w > 0) || !(h > 0)) continue;
        const sh = makeShape({
          kind, x, y, w, h,
          props: (s.props && typeof s.props === 'object') ? (s.props as Record<string, unknown>) : undefined,
          label: typeof s.label === 'string' ? s.label : null,
          createdBy: 'agent'
        });
        if (typeof s.shape_id === 'string' && s.shape_id) sh.id = s.shape_id;
        shapes.push(sh);
      }
      if (shapes.length > 0) canvasAddShapes(canvasId, shapes);
      return true;
    }
    case 'mcp__app__canvas_update_shape': {
      const canvasId = deps.linkedCanvasIdFor(sessionId);
      if (!canvasId) return true;
      const shapeId = str('shape_id');
      if (!shapeId) return true;
      const patch: Partial<Shape> = {};
      if (typeof input.x === 'number') patch.x = input.x as number;
      if (typeof input.y === 'number') patch.y = input.y as number;
      if (typeof input.w === 'number' && (input.w as number) > 0) patch.w = input.w as number;
      if (typeof input.h === 'number' && (input.h as number) > 0) patch.h = input.h as number;
      if (typeof input.rot === 'number') patch.rot = input.rot as number;
      if (typeof input.label === 'string') patch.label = input.label as string;
      if (input.props && typeof input.props === 'object') {
        /* Merge with the shape's existing props rather than replacing,
           so callers can patch a single field (`{props:{source:"..."}}`)
           without losing tint / theme / etc. */
        const c = ensureCanvasLoaded(canvasId);
        const cur = c?.shapes.find((s) => s.id === shapeId);
        patch.props = { ...(cur?.props ?? {}), ...(input.props as Record<string, unknown>) };
      }
      if (Object.keys(patch).length === 0) return true;
      canvasPatchShape(canvasId, shapeId, patch);
      return true;
    }
    case 'mcp__app__canvas_delete_shape': {
      const canvasId = deps.linkedCanvasIdFor(sessionId);
      if (!canvasId) return true;
      const ids: string[] = [];
      const single = str('shape_id');
      if (single) ids.push(single);
      if (Array.isArray(input.shape_ids)) {
        for (const v of input.shape_ids) if (typeof v === 'string' && v) ids.push(v);
      }
      if (ids.length > 0) canvasDeleteShapes(canvasId, ids);
      return true;
    }
    case 'mcp__app__canvas_add_edge': {
      const canvasId = deps.linkedCanvasIdFor(sessionId);
      if (!canvasId) return true;
      const edge = parseEdgeSpec(input);
      if (edge) canvasAddEdge(canvasId, edge);
      return true;
    }
    case 'mcp__app__canvas_add_edges': {
      const canvasId = deps.linkedCanvasIdFor(sessionId);
      if (!canvasId) return true;
      /* Accept the canonical `edges` plus the same aliases the
         sidecar declares (`connections` / `links` / `arrows`). */
      const arr = (input.edges ?? input.connections ?? input.links ?? input.arrows);
      if (!Array.isArray(arr)) return true;
      for (const raw of arr) {
        if (!raw || typeof raw !== 'object') continue;
        const edge = parseEdgeSpec(raw as Record<string, unknown>);
        if (edge) canvasAddEdge(canvasId, edge);
      }
      return true;
    }
    case 'mcp__app__canvas_delete_edge': {
      const canvasId = deps.linkedCanvasIdFor(sessionId);
      if (!canvasId) return true;
      const ids: string[] = [];
      const single = str('edge_id');
      if (single) ids.push(single);
      if (Array.isArray(input.edge_ids)) {
        for (const v of input.edge_ids) if (typeof v === 'string' && v) ids.push(v);
      }
      if (ids.length > 0) canvasDeleteEdges(canvasId, ids);
      return true;
    }
    case 'mcp__app__canvas_arrange': {
      const canvasId = deps.linkedCanvasIdFor(sessionId);
      if (!canvasId) return true;
      const algo = str('algorithm') as LayoutAlgorithm;
      if (!['grid', 'row', 'column', 'dagre'].includes(algo)) return true;
      const ids = Array.isArray(input.shape_ids)
        ? (input.shape_ids as unknown[]).filter((v): v is string => typeof v === 'string')
        : undefined;
      const opts: Record<string, unknown> = {};
      if (typeof input.rankdir === 'string') opts.rankdir = input.rankdir;
      if (typeof input.gap === 'number') opts.gap = input.gap;
      void canvasApplyLayout(canvasId, algo, ids, opts);
      return true;
    }
    case 'mcp__app__canvas_focus': {
      const canvasId = deps.linkedCanvasIdFor(sessionId);
      if (!canvasId) return true;
      const shapeId = str('shape_id');
      if (!shapeId) return true;
      requestCanvasFocus(canvasId, shapeId);
      return true;
    }
    case 'mcp__app__canvas_set_z': {
      const canvasId = deps.linkedCanvasIdFor(sessionId);
      if (!canvasId) return true;
      const shapeId = str('shape_id');
      const mode = str('mode');
      if (!shapeId) return true;
      if (!['to-front', 'to-back', 'forward', 'backward'].includes(mode)) return true;
      canvasSetShapeZ(canvasId, shapeId, mode as 'to-front' | 'to-back' | 'forward' | 'backward');
      return true;
    }
    case 'mcp__app__canvas_duplicate': {
      const canvasId = deps.linkedCanvasIdFor(sessionId);
      if (!canvasId) return true;
      const ids = Array.isArray(input.shape_ids)
        ? (input.shape_ids as unknown[]).filter((v): v is string => typeof v === 'string' && v.length > 0)
        : [];
      if (ids.length === 0) return true;
      const dx = typeof input.dx === 'number' ? input.dx : 12;
      const dy = typeof input.dy === 'number' ? input.dy : 12;
      canvasDuplicateShapes(canvasId, ids, dx, dy);
      return true;
    }
    case 'mcp__app__canvas_find': {
      const canvasId = deps.linkedCanvasIdFor(sessionId);
      if (!canvasId) return true;
      const query = str('query');
      if (!query) return true;
      const ids = canvasFindShapes(canvasId, query);
      /* `find` is a read — but our sidecar reply is just a
         confirmation, so returning data through the agent would
         require either an IPC bridge or a follow-up message. We
         DO change UI state: select the matches so the user can
         visually see what the agent found. The agent's next-turn
         system-prompt preamble will reflect the new selection
         context (selection is ephemeral so it doesn't pollute
         saved canvas state). */
      if (ids.length > 0) canvasSetSelection(canvasId, ids);
      return true;
    }
    case 'mcp__app__canvas_group': {
      const canvasId = deps.linkedCanvasIdFor(sessionId);
      if (!canvasId) return true;
      const ids = Array.isArray(input.shape_ids)
        ? (input.shape_ids as unknown[]).filter((v): v is string => typeof v === 'string' && v.length > 0)
        : [];
      if (ids.length === 0) return true;
      const kind = input.kind === 'group' ? 'group' : 'frame';
      const title = typeof input.title === 'string' ? input.title : undefined;
      canvasGroupShapes(canvasId, ids, { kind, title });
      return true;
    }
    case 'mcp__app__canvas_ungroup': {
      const canvasId = deps.linkedCanvasIdFor(sessionId);
      if (!canvasId) return true;
      const shapeId = str('shape_id');
      if (!shapeId) return true;
      canvasUngroupShapes(canvasId, shapeId);
      return true;
    }
    case 'mcp__app__canvas_lock': {
      const canvasId = deps.linkedCanvasIdFor(sessionId);
      if (!canvasId) return true;
      const ids = Array.isArray(input.shape_ids)
        ? (input.shape_ids as unknown[]).filter((v): v is string => typeof v === 'string' && v.length > 0)
        : [];
      if (ids.length === 0) return true;
      const locked = input.locked === true;
      canvasSetShapesLocked(canvasId, ids, locked);
      return true;
    }
    case 'mcp__app__canvas_align': {
      const canvasId = deps.linkedCanvasIdFor(sessionId);
      if (!canvasId) return true;
      const ids = Array.isArray(input.shape_ids)
        ? (input.shape_ids as unknown[]).filter((v): v is string => typeof v === 'string' && v.length > 0)
        : [];
      const axis = str('axis');
      const validAxes: AlignAxis[] = ['left', 'center-x', 'right', 'top', 'center-y', 'bottom'];
      if (ids.length < 2 || !(validAxes as string[]).includes(axis)) return true;
      canvasAlignShapes(canvasId, ids, axis as AlignAxis);
      return true;
    }
    case 'mcp__app__canvas_distribute': {
      const canvasId = deps.linkedCanvasIdFor(sessionId);
      if (!canvasId) return true;
      const ids = Array.isArray(input.shape_ids)
        ? (input.shape_ids as unknown[]).filter((v): v is string => typeof v === 'string' && v.length > 0)
        : [];
      const axis = str('axis');
      if (ids.length < 3 || (axis !== 'horizontal' && axis !== 'vertical')) return true;
      canvasDistributeShapes(canvasId, ids, axis as DistributeAxis);
      return true;
    }
    case 'mcp__app__canvas_set_viewport': {
      const canvasId = deps.linkedCanvasIdFor(sessionId);
      if (!canvasId) return true;
      const x = num('x'); const y = num('y');
      if (!Number.isFinite(x) || !Number.isFinite(y)) return true;
      const c = ensureCanvasLoaded(canvasId);
      if (!c) return true;
      const z = typeof input.zoom === 'number' && input.zoom > 0
        ? Math.max(0.1, Math.min(4, input.zoom))
        : c.viewport.zoom;
      canvasSetViewport(canvasId, { x, y, zoom: z });
      return true;
    }
    case 'mcp__app__canvas_upload_image': {
      const canvasId = deps.linkedCanvasIdFor(sessionId);
      if (!canvasId) return true;
      const b64 = str('base64');
      if (!b64) return true;
      const mime = str('mime_type') || 'image/png';
      const dataUrl = `data:${mime};base64,${b64}`;
      /* Use Image() to read intrinsic dimensions; fall back to a
         default size if decode fails. We can't await inside this
         switch elegantly, so this branch fires off an async task
         that creates the shape once dimensions resolve. */
      const c = ensureCanvasLoaded(canvasId);
      if (!c) return true;
      const desiredX = typeof input.x === 'number' ? input.x : (c.viewport.x + 100);
      const desiredY = typeof input.y === 'number' ? input.y : (c.viewport.y + 100);
      const desiredId = str('shape_id');
      const alt = typeof input.alt === 'string' ? input.alt : null;
      void (async () => {
        const dim = await new Promise<{ w: number; h: number }>((resolve) => {
          const img = new Image();
          img.onerror = () => resolve({ w: 320, h: 200 });
          img.onload = () => resolve({ w: img.naturalWidth || 320, h: img.naturalHeight || 200 });
          img.src = dataUrl;
        });
        const MAX_DIM = 480;
        let outW = dim.w, outH = dim.h;
        if (dim.w > MAX_DIM || dim.h > MAX_DIM) {
          const k = Math.min(MAX_DIM / dim.w, MAX_DIM / dim.h);
          outW = Math.round(dim.w * k);
          outH = Math.round(dim.h * k);
        }
        const shape = makeShape({
          kind: 'image',
          x: desiredX,
          y: desiredY,
          w: outW,
          h: outH,
          props: { dataUrl, intrinsicWidth: dim.w, intrinsicHeight: dim.h, alt }
        });
        if (desiredId) shape.id = desiredId;
        canvasAddShape(canvasId, shape);
      })();
      return true;
    }
    /* Three-call SDD close-out tools. The sidecar tool returns
       text instantly so the agent can keep streaming; the real
       mutation (write plan.md / verify.json, advance substep
       state, flip phase frontmatter) happens here via the same
       Tauri commands the SddCard buttons call. File-watcher
       detects the change and the orchestrator schedules the
       next pass on the next tick. */
    case 'mcp__app__sdd_save_phase_plan': {
      const id = str('id');
      const phase = num('phase');
      const body = str('body');
      if (!id || !Number.isFinite(phase) || !body) return true;
      void saveSddPhasePlan(id, phase, body);
      return true;
    }
    case 'mcp__app__sdd_complete_phase_implement': {
      const id = str('id');
      const phase = num('phase');
      const summary = str('summary');
      if (!id || !Number.isFinite(phase) || !summary) return true;
      const fc = Array.isArray(input.files_changed)
        ? (input.files_changed as unknown[]).filter((v): v is string => typeof v === 'string')
        : [];
      void completeSddPhaseImplement(id, phase, summary, fc);
      return true;
    }
    case 'mcp__app__sdd_log_phase_done': {
      /* Single-call mode close-out. The woom-app sidecar stub
       * returns a success message but doesn't actually flip phase
       * status — bug surfaced as "agent says 'Phase 4 done.' in
       * chat but UI still shows phase 3 done". Route to the
       * existing verify-save command with an auto-built JSON that
       * carries the agent's summary + files_changed and an empty
       * deviations array (single-call agents don't run a verify
       * pass — their "done" claim IS the assertion that the phase
       * is clean). Flipping through verify-save means the same
       * derive_stage / audit path runs and the chips advance. */
      const id = str('id');
      const phase = num('phase');
      const summary = str('summary');
      if (!id || !Number.isFinite(phase) || !summary) return true;
      const filesChanged = Array.isArray(input.files_changed)
        ? (input.files_changed as unknown[]).filter((v): v is string => typeof v === 'string')
        : [];
      const rawJson = JSON.stringify({
        summary,
        files_changed: filesChanged,
        task_compliance: [],
        deviations: [],
        notes: '',
      });
      void saveSddPhaseVerify(id, phase, rawJson);
      return true;
    }
    case 'mcp__app__sdd_save_phase_verify': {
      const id = str('id');
      const phase = num('phase');
      const rawJson = str('raw_json');
      if (!id || !Number.isFinite(phase) || !rawJson) return true;
      void saveSddPhaseVerify(id, phase, rawJson);
      return true;
    }
    case 'mcp__app__sdd_approve_phase_plan': {
      const id = str('id');
      const phase = num('phase');
      if (!id || !Number.isFinite(phase)) return true;
      void approveSddPhasePlan(id, phase);
      return true;
    }
    case 'mcp__app__sdd_discard_phase_plan': {
      const id = str('id');
      const phase = num('phase');
      const reason = str('reason');
      if (!id || !Number.isFinite(phase)) return true;
      void discardSddPhasePlan(id, phase, reason || undefined);
      return true;
    }
  }
  return false;
}

// Re-export pickFrom for the wave-30 imports to keep working.
export { pickFrom };
