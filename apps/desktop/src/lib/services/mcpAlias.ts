/* MCP payload alias parsing.
 *
 * cursor-agent and Claude both ship the same tool-call argument under
 * many different names: `repo_path` vs `path` vs `folder`, sometimes
 * wrapped under `args` / `arguments` / `params`, occasionally as a
 * single-element array, occasionally as `{path: "..."}` instead of
 * `"..."`. The Rust sidecars accept all of these via `#[serde(alias)]`
 * + recursive `coerce_to_string`. This module mirrors that logic on
 * the frontend so the dispatcher in `+page.svelte` accepts the exact
 * same shapes the sidecar does.
 *
 * Pure functions — no Svelte runes, no IPC. Each helper takes the
 * input as a parameter so unit-testable in isolation. */

import type { Edge } from '$lib/state/canvas-types';
import { makeEdge } from '$lib/state/canvas.svelte';

/** Pick the first non-empty string value found at any of the
 *  `keys` (in order) on `obj`. Trim before returning. */
export function pickFrom(
  obj: Record<string, unknown>,
  ...keys: string[]
): string {
  for (const k of keys) {
    const v = obj[k];
    if (typeof v === 'string' && v.trim()) return v.trim();
  }
  return '';
}

/** Coerce a Value into a non-empty trimmed string when possible.
 *  cursor-agent has shipped path-style fields as:
 *    - `"/Users/me/repo"`     (happy path)
 *    - `["/Users/me/repo"]`   (single-element array)
 *    - `{"path": "/Users/me/repo"}` (over-eager nesting)
 *    - `""`                   (empty placeholder)
 *  Any of these yields a valid string; everything else returns ''. */
export function coerceString(v: unknown): string {
  if (typeof v === 'string') return v.trim();
  if (Array.isArray(v)) {
    for (const x of v) {
      const s = coerceString(x);
      if (s) return s;
    }
    return '';
  }
  if (v && typeof v === 'object') {
    const obj = v as Record<string, unknown>;
    for (const k of [
      'repo_path', 'path', 'folder', 'directory', 'dir',
      'cwd', 'value', 'text', 'string'
    ]) {
      if (k in obj) {
        const s = coerceString(obj[k]);
        if (s) return s;
      }
    }
  }
  return '';
}

/** Alias-aware analogue of `pickFrom` that ALSO drills into the
 *  wrapper objects cursor-agent / Claude have been known to nest
 *  payloads under (`args` / `arguments` / `params` / `input`). Used
 *  by `set_editor_repo_path` / `set_agent_cwd` — both have been
 *  observed receiving fully-wrapped payloads where `repo_path` is
 *  two levels deep. Walks up to depth 4 to cover the
 *  `{"args":{"args":{...}}}` case we've seen in the wild. */
export function pickDeep(
  obj: Record<string, unknown> | null | undefined,
  keys: string[],
  depth = 4
): string {
  if (!obj || typeof obj !== 'object' || depth === 0) return '';
  for (const k of keys) {
    if (k in obj) {
      const s = coerceString(obj[k]);
      if (s) return s;
    }
  }
  for (const wrap of ['args', 'arguments', 'params', 'parameters', 'input', 'data', 'payload']) {
    const inner = obj[wrap];
    if (inner && typeof inner === 'object' && !Array.isArray(inner)) {
      const s = pickDeep(inner as Record<string, unknown>, keys, depth - 1);
      if (s) return s;
    }
  }
  return '';
}

/* Canonical alias lists — kept in sync with the sidecar's
 * `REPO_PATH_KEYS` / `INSTANCE_NAME_KEYS` / `INSTANCE_ID_KEYS` so both
 * halves of the round-trip recognise the same payload shapes. */
export const REPO_PATH_KEYS = [
  'repo_path', 'repoPath', 'path', 'folder', 'directory', 'dir',
  'cwd', 'repo', 'repository_path', 'folderPath', 'dirPath',
  'fullPath', 'absolutePath', 'target_path', 'target'
];

export const INSTANCE_NAME_KEYS = [
  'instance_name', 'instanceName', 'name', 'column_name', 'columnName',
  'editor_name', 'agent_name', 'label'
];

export const INSTANCE_ID_KEYS = [
  'instance_id', 'instanceId', 'id', 'column_id', 'columnId',
  'editor_id', 'agent_id', 'uuid'
];

/** Shared edge-spec parser used by both `canvas_add_edge` (single)
 *  and `canvas_add_edges` (batch). Mirrors the alias set on the
 *  sidecar's `CanvasAddEdgeParams`; returns null when required ids
 *  are missing so the caller can skip the entry instead of throwing. */
export function parseEdgeSpec(obj: Record<string, unknown>): Edge | null {
  const fromId = pickFrom(
    obj,
    'from_shape_id', 'from', 'source', 'from_id', 'fromId',
    'fromShapeId', 'fromNode', 'fromBlock', 'start', 'start_id',
    'startId', 'src', 'sourceId'
  );
  const toId = pickFrom(
    obj,
    'to_shape_id', 'to', 'target', 'to_id', 'toId', 'toShapeId',
    'toNode', 'toBlock', 'end', 'end_id', 'endId', 'dest', 'dst',
    'targetId'
  );
  if (!fromId || !toId) return null;
  type AnchorName = 'tl'|'tc'|'tr'|'ml'|'mc'|'mr'|'bl'|'bc'|'br';
  const validAnchors: AnchorName[] = ['tl','tc','tr','ml','mc','mr','bl','bc','br'];
  const fromAnchorRaw = pickFrom(
    obj,
    'from_anchor', 'fromAnchor', 'source_anchor', 'sourceAnchor',
    'start_anchor', 'startAnchor', 'srcAnchor'
  ) || 'mr';
  const toAnchorRaw = pickFrom(
    obj,
    'to_anchor', 'toAnchor', 'target_anchor', 'targetAnchor',
    'end_anchor', 'endAnchor', 'destAnchor'
  ) || 'ml';
  const fromAnchor = (validAnchors as string[]).includes(fromAnchorRaw)
    ? (fromAnchorRaw as AnchorName) : 'mr';
  const toAnchor = (validAnchors as string[]).includes(toAnchorRaw)
    ? (toAnchorRaw as AnchorName) : 'ml';
  const kindRaw = pickFrom(obj, 'kind', 'style', 'edge_kind', 'edgeKind');
  const kind: 'arrow' | 'line' | 'dashed' =
    (kindRaw === 'line' || kindRaw === 'dashed') ? kindRaw : 'arrow';
  const routingRaw = pickFrom(obj, 'routing', 'route', 'path', 'pathing');
  const routing: 'straight' | 'orthogonal' | 'curved' =
    (routingRaw === 'straight' || routingRaw === 'curved')
      ? routingRaw : 'orthogonal';
  const labelRaw = pickFrom(obj, 'label', 'text', 'caption', 'title');
  const edge = makeEdge({
    from: { shapeId: fromId, anchor: fromAnchor },
    to: { shapeId: toId, anchor: toAnchor },
    kind, routing,
    label: labelRaw || null
  });
  const desiredId = pickFrom(obj, 'edge_id', 'id', 'edgeId');
  if (desiredId) edge.id = desiredId;
  return edge;
}

/** Read a typed string scalar by canonical key name. Wraps the raw
 *  `input[k]` access + trim. Used heavily by handleAppNavigation
 *  cases that take a single id / key / etc. */
export function readStr(input: Record<string, unknown>, k: string): string {
  return typeof input[k] === 'string' ? (input[k] as string).trim() : '';
}

/** Same as `readStr` but for numbers. Coerces strings ("12") because
 *  cursor-agent occasionally stringifies numeric args. */
export function readNum(input: Record<string, unknown>, k: string): number {
  const v = input[k];
  return typeof v === 'number' ? v : Number(v);
}

/** Map the platform-named view the agent ships (`github` /
 *  `jira` / `sentry`) to Forgehold's internal `View` key
 *  (`githubTab` / `jiraTab` / `sentryTab` — the `Tab` suffix
 *  disambiguates from the workbench column kind of the same
 *  name). Unrecognised names return `null`; pass-through values
 *  (`workbench` / `rules` / `connections` / `settings`) come back
 *  unchanged.
 *
 *  The return type uses the literal strings directly to avoid a
 *  cyclic import on the `View` alias from `view.svelte.ts`. */
export type AgentInternalView =
  | 'workbench' | 'githubTab' | 'jiraTab' | 'sentryTab'
  | 'rules' | 'connections' | 'settings';

export function mapAgentViewToInternal(v: string): AgentInternalView | null {
  switch (v) {
    case 'github':       return 'githubTab';
    case 'jira':         return 'jiraTab';
    case 'sentry':       return 'sentryTab';
    case 'workbench':    return 'workbench';
    case 'rules':        return 'rules';
    case 'connections':  return 'connections';
    case 'settings':     return 'settings';
    default:             return null;
  }
}

/** Coerce raw `sprint_ids` payload entries into the persisted
 *  `SprintScope[]` shape (numeric id or the literal `'backlog'`).
 *  The MCP tool's JSON schema accepts string|number; we accept either
 *  here too because cursor-agent and Claude have shipped both. The
 *  return type is repeated locally rather than imported from
 *  `inbox.svelte.ts` to keep this module reactive-store-free. */
export type SprintScopeAlias = number | 'backlog';

export function parseSprintScopes(raw: unknown[]): SprintScopeAlias[] {
  const out: SprintScopeAlias[] = [];
  for (const x of raw) {
    if (typeof x === 'number' && Number.isFinite(x) && x > 0) {
      out.push(x);
    } else if (typeof x === 'string') {
      if (x === 'backlog') {
        out.push('backlog');
      } else {
        const n = Number(x);
        if (Number.isFinite(n) && n > 0) out.push(n);
      }
    }
  }
  return out;
}
