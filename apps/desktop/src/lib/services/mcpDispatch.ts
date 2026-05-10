/* MCP-tool dispatch for the `woom-app` sidecar.
 *
 * The agent calls `mcp__app__open_jira_issue` / `switch_view` /
 * `canvas_add_shape` / etc. — the stream parser sees the
 * `tool_use` event, and we drive Woom's reactive state directly
 * here. Same outcome as if the user had clicked through the UI.
 *
 * No approval card: these are read-only navigations / state writes
 * the agent is allowed to make without user confirmation. Bad
 * inputs (unknown view name, blank id) silently no-op rather than
 * throw — the chat still shows the inline `> *Tool* …` hint so the
 * user can see what the agent tried.
 *
 * The dispatcher needs ~7 component-local helpers ("findInstance",
 * "openConnectModal", etc.) that can't be safely held by a module
 * (they close over `+page.svelte`'s `view` setter and per-page
 * state). Those go through a `McpDispatchContext` deps bag passed
 * to `createMcpDispatcher(deps)`, which returns the dispatch
 * function. Module-level state (inboxState, canvasState, layout
 * helpers, the canvas op aliases) is imported directly here.
 *
 * Keep this file in lock-step with `woom-app/src/main.rs` —
 * each `case` here mirrors a `#[tool]` on the sidecar. */

import type { ConnectionMeta } from '$lib/data';
import { connectionsMeta } from '$lib/data';
import type {
  Edge,
  Shape,
  ShapeKind
} from '$lib/state/canvas-types';
import {
  ensureCanvasLoaded,
  makeShape,
  requestCanvasFocus,
  addShape as canvasAddShape,
  addShapes as canvasAddShapes,
  addEdge as canvasAddEdge,
  deleteShapes as canvasDeleteShapes,
  deleteEdges as canvasDeleteEdges,
  patchShape as canvasPatchShape,
  setShapeZ as canvasSetShapeZ,
  duplicateShapes as canvasDuplicateShapes,
  findShapesByQuery as canvasFindShapes,
  setSelection as canvasSetSelection,
  groupShapes as canvasGroupShapes,
  ungroupShapes as canvasUngroupShapes,
  setShapesLocked as canvasSetShapesLocked,
  alignShapes as canvasAlignShapes,
  distributeShapes as canvasDistributeShapes,
  setViewport as canvasSetViewport,
  canvasState,
  type AlignAxis,
  type DistributeAxis
} from '$lib/state/canvas.svelte';
import { applyLayout as canvasApplyLayout, type LayoutAlgorithm } from '$lib/services/canvasLayout';
import { applySessionCwd } from '$lib/services/sessionCwd';
import {
  inboxState,
  openSentryFocus,
  scheduleSentryTabFilterRefresh,
  setSentryFilters,
  updateGithubFilters,
  updateJiraFilters,
  updateJiraTabFilters,
  type GithubFilters,
  type GithubFilterMode,
  type JiraFilters
} from '$lib/state/inbox.svelte';
import {
  layoutState,
  APP_INSTANCE_IDS,
  kindForInstanceId
} from '$lib/state/layout.svelte';
import { notify } from '$lib/state/toaster.svelte';

/** Surface a "the agent tried to do X but Y was wrong" warning when
 *  an MCP tool call lands in a silent-no-op branch. Without a toast,
 *  the user has no idea the agent's tool call failed — the only
 *  signal in the chat is the bare `> *Tool* …` hint, which doesn't
 *  carry "and it didn't work" semantics. The agent's NEXT turn also
 *  doesn't see it (these tool dispatches don't go through the chat
 *  message pipeline), so the agent often charges ahead assuming the
 *  switch / focus / open succeeded. Toasting at least lets the user
 *  notice and correct the agent on the next prompt. */
function warnAgentToolMissed(tool: string, reason: string) {
  notify({
    kind: 'info',
    title: `Agent tool "${tool}" was a no-op`,
    body: reason,
    ttlMs: 5000
  });
}
import { sessionsState } from '$lib/state/sessions.svelte';
import type { PanelKind } from '$lib/types';
type PanelInstance = { id: string; kind: PanelKind; name: string; width: number };
import type { AgentInternalView } from './mcpAlias';
import type { DetailTab } from '$lib/state/view.svelte';
import {
  INSTANCE_ID_KEYS,
  INSTANCE_NAME_KEYS,
  mapAgentViewToInternal,
  parseEdgeSpec,
  parseSprintScopes,
  pickDeep,
  pickFrom,
  readNum,
  readStr,
  REPO_PATH_KEYS
} from './mcpAlias';

/** Component-local dependencies the dispatcher needs. The `+page.svelte`
 *  page wires these once via `createMcpDispatcher(deps)` and reuses the
 *  returned dispatch function for every tool call. */
export interface McpDispatchContext {
  /** Set the top-level view (rail tab). */
  setView: (v: AgentInternalView) => void;
  /** Set an editor instance's repo root, propagating to linked agent
   *  sessions. The component owns the per-instance state map this
   *  writes to. */
  setEditorRepoPath: (value: string, instanceId?: string) => void;
  /** Open the source-specific connect modal. */
  openConnectModal: (conn: ConnectionMeta) => void;
  /** Resolve the canvas id a session is linked to (or null if no
   *  link / canvas missing). */
  linkedCanvasIdFor: (sessionId: string) => string | null;
  /** Resolve the singleton record for a kind. App mode keeps this
   *  shape (instead of a literal `APP_INSTANCE_IDS[kind]` lookup) so
   *  `findInstanceByNameOrId` callers don't need to know the concrete
   *  id format. */
  findInstanceByNameOrId: (
    kind: PanelKind,
    name: string,
    id: string
  ) => PanelInstance | null;
  /** Resolve a focused GitHub PR / issue overlay, fetching the
   *  upstream item on demand. Returns void; resolves async. */
  resolveGithubFocus: (
    owner: string,
    repo: string,
    n: number,
    tabHint: DetailTab | null
  ) => Promise<void>;
}

const VALID_PANEL_KINDS: PanelKind[] =
  ['github', 'jira', 'sentry', 'claude', 'cursor', 'editor', 'canvas', 'terminal'];

const Z_MODES = ['to-front', 'to-back', 'forward', 'backward'] as const;
const ALIGN_AXES: AlignAxis[] =
  ['left', 'center-x', 'right', 'top', 'center-y', 'bottom'];

/** Closed-set narrowing for the GitHub-column `mode` filter. The
 *  agent ships free strings; the column rejects anything outside
 *  this list. Kept here rather than in `inbox.svelte.ts` so the
 *  dispatcher is the canonical user. Mirrors `GithubFilterMode`
 *  from `inbox.svelte.ts`. */
function isGithubFilterMode(s: string): s is GithubFilterMode {
  return (
    s === 'involving' || s === 'authored' || s === 'review_requested' ||
    s === 'assigned' || s === 'user' || s === 'all'
  );
}

type SentryStatus = 'unresolved' | 'resolved' | 'ignored' | 'all';
function isSentryStatus(s: string): s is SentryStatus {
  return s === 'unresolved' || s === 'resolved' || s === 'ignored' || s === 'all';
}

type SentryLevel = 'all' | 'fatal' | 'error' | 'warning' | 'info' | 'debug';
function isSentryLevel(s: string): s is SentryLevel {
  return (
    s === 'all' || s === 'fatal' || s === 'error' ||
    s === 'warning' || s === 'info' || s === 'debug'
  );
}

/** SentryFilters in the column store carries 7 fields including
 *  `sort` (which the agent doesn't expose). The dispatcher accepts
 *  any subset matching what the tool advertises. */
export type SentryFilterPatch = {
  projects?: string[];
  search?: string;
  status?: SentryStatus;
  level?: SentryLevel;
  environment?: string | null;
};

/** Build the `mcp__app__*` dispatcher. Call once per page mount. */
export function createMcpDispatcher(ctx: McpDispatchContext) {
  return function handleAppNavigation(
    sessionId: string,
    name: string,
    input: Record<string, unknown>
  ): void {
    const str = (k: string): string => readStr(input, k);
    const num = (k: string): number => readNum(input, k);

    switch (name) {
      // ──────────────────────────────────────────────────────────
      // Inbox / focus pane
      // ──────────────────────────────────────────────────────────
      case 'mcp__app__open_jira_issue': {
        const key = str('key');
        if (key) inboxState.jiraFocusKey = key;
        return;
      }
      case 'mcp__app__open_sentry_issue': {
        // openSentryFocus(id) defaults eventId to null — equivalent to
        // "latest" so a stale event id from a previous open_sentry_event
        // call doesn't carry over.
        const id = str('id');
        if (id) openSentryFocus(id);
        return;
      }
      case 'mcp__app__open_sentry_event': {
        const id = str('issue_id');
        const eventId = str('event_id') || null;
        if (id) openSentryFocus(id, eventId);
        return;
      }
      case 'mcp__app__open_github_pr':
      case 'mcp__app__open_github_issue': {
        // GitHub focus pane wants a full InboxItem; fetch it on demand
        // through the same API call the inbox uses, then stash. The user
        // sees a brief flash before it lands — fine for a navigation.
        // The overlay is mounted at page root, so it appears over
        // whatever view the user is currently on.
        const owner = str('owner');
        const repo = str('repo');
        const n = num('number');
        if (!owner || !repo || !Number.isFinite(n)) return;
        const tabHint = str('tab') as DetailTab | '';
        void ctx.resolveGithubFocus(owner, repo, n, tabHint || null);
        return;
      }

      // ──────────────────────────────────────────────────────────
      // View / layout
      // ──────────────────────────────────────────────────────────
      case 'mcp__app__switch_view': {
        // The MCP tool exposes platform-named views (`github` / `jira`
        // / `sentry` / etc.); `mapAgentViewToInternal` translates them
        // to the matching `…App` internal view name.
        const mapped = mapAgentViewToInternal(str('view'));
        if (mapped) ctx.setView(mapped);
        return;
      }
      case 'mcp__app__open_repo': {
        const repoPath = str('repo_path');
        ctx.setView('editorApp');
        if (repoPath) ctx.setEditorRepoPath(repoPath, APP_INSTANCE_IDS.editor);
        return;
      }
      case 'mcp__app__open_connect_modal': {
        const sourceId = str('source');
        const conn = connectionsMeta.find((c) => c.id === sourceId);
        if (conn) ctx.openConnectModal(conn);
        return;
      }
      case 'mcp__app__focus_solo': {
        // App singletons always exist — this is just `setView` for the
        // kind's solo view.
        const kind = str('kind');
        if (!VALID_PANEL_KINDS.includes(kind as PanelKind)) {
          warnAgentToolMissed('focus_solo', `kind=${kind} is not a known panel kind`);
          return;
        }
        const map: Record<PanelKind, AgentInternalView> = {
          github: 'githubApp',
          jira: 'jiraApp',
          sentry: 'sentryApp',
          claude: 'claudeApp',
          cursor: 'cursorApp',
          editor: 'editorApp',
          canvas: 'canvasApp',
          terminal: 'terminalApp'
        };
        ctx.setView(map[kind as PanelKind]);
        return;
      }

      // ──────────────────────────────────────────────────────────
      // Source tabs (top-level views)
      // ──────────────────────────────────────────────────────────
      case 'mcp__app__open_github_repo': {
        const owner = str('owner');
        const repo = str('repo');
        const section = str('section') || 'pulls';
        const path = str('path');
        if (!owner || !repo) return;
        ctx.setView('githubApp');
        // GithubTab watches this slot and clears it after opening.
        // `path` only honoured for section=code (server validates too).
        inboxState.pendingRepoNav = {
          owner,
          repo,
          section,
          path: section === 'code' && path ? path : null
        };
        return;
      }
      case 'mcp__app__open_jira_tab': {
        // Build a Partial<JiraFilters> from only the keys the agent
        // actually sent. `updateJiraTabFilters` merges and persists
        // and triggers a debounced re-fetch. Skipping a key leaves
        // that filter alone (matches the tool's "omitted = unchanged"
        // contract).
        const patch: Partial<JiraFilters> = {};
        if ('project_key' in input) patch.projectKey = str('project_key') || null;
        if ('search' in input) patch.search = str('search');
        if ('status_name' in input) patch.statusName = str('status_name') || null;
        if (Array.isArray(input.board_ids)) {
          patch.boardIds = input.board_ids
            .map((x) => Number(x))
            .filter((x): x is number => Number.isFinite(x) && x > 0);
        }
        if (Array.isArray(input.sprint_ids)) {
          patch.sprintIds = parseSprintScopes(input.sprint_ids);
        }
        ctx.setView('jiraApp');
        updateJiraTabFilters(patch);
        return;
      }
      case 'mcp__app__open_sentry_tab': {
        // SentryTab fields are flat on `inboxState` (not under one
        // filter object), so we can't reuse a setSentryFilters-style
        // patch. Mutate field-by-field — the schedule call below
        // persists and re-runs the query.
        ctx.setView('sentryApp');
        if (Array.isArray(input.projects)) {
          inboxState.sentryTabProjects = input.projects
            .map((x) => String(x))
            .filter((s) => s.length > 0);
        }
        if ('search' in input) inboxState.sentryTabSearch = str('search');
        if ('status' in input) {
          const s = str('status');
          if (s) inboxState.sentryTabStatus = s as typeof inboxState.sentryTabStatus;
        }
        if ('level' in input) {
          const l = str('level');
          if (l) inboxState.sentryTabLevel = l as typeof inboxState.sentryTabLevel;
        }
        if ('environment' in input) {
          const e = str('environment');
          inboxState.sentryTabEnvironment = e ? e : null;
        }
        scheduleSentryTabFilterRefresh();
        return;
      }

      // ──────────────────────────────────────────────────────────
      // Per-column filter writes
      // ──────────────────────────────────────────────────────────
      case 'mcp__app__set_github_column': {
        const inst = ctx.findInstanceByNameOrId('github', str('instance_name'), str('instance_id'));
        if (!inst) {
          warnAgentToolMissed(
            'set_github_column',
            `No inbox matched "${str('instance_name') || str('instance_id') || '(unspecified)'}". Add the column first or pass a valid name/id.`
          );
          return;
        }
        const patch: Partial<GithubFilters> = {};
        if ('repo' in input) {
          // Empty string = "clear filter" (= all repos).
          const r = str('repo');
          patch.repo = r ? r : null;
        }
        if ('mode' in input) {
          const m = str('mode');
          if (isGithubFilterMode(m)) patch.mode = m;
        }
        if ('search' in input) patch.search = str('search');
        if ('custom_user' in input) patch.customUser = str('custom_user');
        ctx.setView('githubApp');
        updateGithubFilters(inst.id, patch);
        return;
      }
      case 'mcp__app__set_jira_column': {
        const inst = ctx.findInstanceByNameOrId('jira', str('instance_name'), str('instance_id'));
        if (!inst) {
          warnAgentToolMissed(
            'set_jira_column',
            `No Jira column matched "${str('instance_name') || str('instance_id') || '(unspecified)'}". Add the column first or pass a valid name/id.`
          );
          return;
        }
        const patch: Partial<JiraFilters> = {};
        if ('project_key' in input) {
          const p = str('project_key');
          patch.projectKey = p ? p : null;
        }
        if ('status_name' in input) {
          const s = str('status_name');
          patch.statusName = s ? s : null;
        }
        if ('search' in input) patch.search = str('search');
        if (Array.isArray(input.board_ids)) {
          patch.boardIds = input.board_ids
            .map((x) => Number(x))
            .filter((x): x is number => Number.isFinite(x) && x > 0);
        }
        if (Array.isArray(input.sprint_ids)) {
          patch.sprintIds = parseSprintScopes(input.sprint_ids);
        }
        ctx.setView('jiraApp');
        updateJiraFilters(inst.id, patch);
        return;
      }
      case 'mcp__app__set_sentry_column': {
        const inst = ctx.findInstanceByNameOrId('sentry', str('instance_name'), str('instance_id'));
        if (!inst) {
          warnAgentToolMissed(
            'set_sentry_column',
            `No Sentry column matched "${str('instance_name') || str('instance_id') || '(unspecified)'}". Add the column first or pass a valid name/id.`
          );
          return;
        }
        const patch: SentryFilterPatch = {};
        if (Array.isArray(input.projects)) {
          patch.projects = input.projects
            .map((x) => String(x))
            .filter((s) => s.length > 0);
        }
        if ('search' in input) patch.search = str('search');
        if ('status' in input) {
          const s = str('status');
          if (isSentryStatus(s)) patch.status = s;
        }
        if ('level' in input) {
          const l = str('level');
          if (isSentryLevel(l)) patch.level = l;
        }
        if ('environment' in input) {
          const e = str('environment');
          patch.environment = e ? e : null;
        }
        ctx.setView('sentryApp');
        setSentryFilters(inst.id, patch);
        return;
      }

      // ──────────────────────────────────────────────────────────
      // Editor / agent cwd
      // ──────────────────────────────────────────────────────────
      case 'mcp__app__set_editor_repo_path': {
        // Use `pickDeep` instead of plain `pick`: cursor-agent has
        // shipped this payload wrapped in `args` / `arguments`, with
        // `repo_path` as a single-element array, and with
        // non-canonical keys (`folderPath`, `fullPath`, …).
        // pickDeep mirrors the sidecar's recursive search.
        const repoPath = pickDeep(input, REPO_PATH_KEYS);
        const instName = pickDeep(input, INSTANCE_NAME_KEYS);
        const instId = pickDeep(input, INSTANCE_ID_KEYS);
        if (!repoPath) {
          warnAgentToolMissed(
            'set_editor_repo_path',
            'Agent did not provide a `repo_path`. Editor was not switched.'
          );
          return;
        }
        const editor = ctx.findInstanceByNameOrId('editor', instName, instId);
        if (!editor) {
          warnAgentToolMissed(
            'set_editor_repo_path',
            `No Editor column matched "${instName || instId || '(unspecified)'}". Repo not switched.`
          );
          return;
        }
        ctx.setView('editorApp');
        ctx.setEditorRepoPath(repoPath, editor.id);
        // Linked agents follow. `applySessionCwd` rotates the agent's
        // claudeUuid + resets `claudeResumable` when the new cwd
        // actually differs — necessary because Claude CLI scopes
        // conversations by project (cwd-derived); resuming an old
        // uuid in a new project fails with "No conversation found".
        for (const s of sessionsState.list) {
          if (s.linkedToEditor && s.linkedToEditorInstanceId === editor.id) {
            applySessionCwd(s.id, repoPath, { breakLink: false });
          }
        }
        return;
      }
      case 'mcp__app__set_agent_cwd': {
        // Same pickDeep contract as set_editor_repo_path — keep the
        // two in sync so the LLM doesn't need a different schema.
        const repoPath = pickDeep(input, REPO_PATH_KEYS);
        if (!repoPath) {
          warnAgentToolMissed(
            'set_agent_cwd',
            'Agent did not provide a `repo_path`. Agent cwd not switched.'
          );
          return;
        }
        const target = str('target').toLowerCase();
        let sessId: string | null = null;
        let targetLabel = '(self)';
        if (target === 'self') {
          sessId = sessionId;
        } else {
          const instName = pickDeep(input, INSTANCE_NAME_KEYS);
          const instId = pickDeep(input, INSTANCE_ID_KEYS);
          targetLabel = instName || instId || '(unspecified)';
          // Try claude first, then cursor — same pool from the
          // user's POV.
          const inst = ctx.findInstanceByNameOrId('claude', instName, instId)
            ?? ctx.findInstanceByNameOrId('cursor', instName, instId);
          if (inst) {
            ctx.setView(inst.kind === 'cursor' ? 'cursorApp' : 'claudeApp');
            sessId = sessionsState.activeByInstance[inst.id] ?? null;
          }
        }
        if (!sessId) {
          warnAgentToolMissed(
            'set_agent_cwd',
            `No agent column matched "${targetLabel}", or it has no active session. Cwd not switched.`
          );
          return;
        }
        applySessionCwd(sessId, repoPath, { breakLink: false });
        return;
      }
      case 'mcp__app__list_instances': {
        // No-op: the data lives in the system-prompt preamble and is
        // refreshed on every turn. The sidecar's tool reply explains.
        return;
      }

      // ──────────────────────────────────────────────────────────
      // Canvas (whiteboard)
      // ──────────────────────────────────────────────────────────
      case 'mcp__app__canvas_add_shape': {
        const canvasId = ctx.linkedCanvasIdFor(sessionId);
        if (!canvasId) return;
        const kind = str('kind') as ShapeKind;
        if (!kind) return;
        const x = num('x'); const y = num('y');
        const w = num('w'); const h = num('h');
        if (!Number.isFinite(x) || !Number.isFinite(y) || !(w > 0) || !(h > 0)) return;
        const props = (input.props && typeof input.props === 'object')
          ? (input.props as Record<string, unknown>)
          : undefined;
        const label = typeof input.label === 'string' ? (input.label as string) : null;
        const desiredId = str('shape_id');
        const shape = makeShape({ kind, x, y, w, h, props, label, createdBy: 'agent' });
        if (desiredId) shape.id = desiredId;
        canvasAddShape(canvasId, shape);
        return;
      }
      case 'mcp__app__canvas_add_shapes': {
        const canvasId = ctx.linkedCanvasIdFor(sessionId);
        if (!canvasId) return;
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
        return;
      }
      case 'mcp__app__canvas_update_shape': {
        const canvasId = ctx.linkedCanvasIdFor(sessionId);
        if (!canvasId) return;
        const shapeId = str('shape_id');
        if (!shapeId) return;
        const patch: Partial<Shape> = {};
        if (typeof input.x === 'number') patch.x = input.x as number;
        if (typeof input.y === 'number') patch.y = input.y as number;
        if (typeof input.w === 'number' && (input.w as number) > 0) patch.w = input.w as number;
        if (typeof input.h === 'number' && (input.h as number) > 0) patch.h = input.h as number;
        if (typeof input.rot === 'number') patch.rot = input.rot as number;
        if (typeof input.label === 'string') patch.label = input.label as string;
        if (input.props && typeof input.props === 'object') {
          /* Merge with the shape's existing props rather than replacing,
             so callers can patch a single field without losing tint /
             theme / etc. */
          const c = ensureCanvasLoaded(canvasId);
          const cur = c?.shapes.find((s) => s.id === shapeId);
          patch.props = { ...(cur?.props ?? {}), ...(input.props as Record<string, unknown>) };
        }
        if (Object.keys(patch).length === 0) return;
        canvasPatchShape(canvasId, shapeId, patch);
        return;
      }
      case 'mcp__app__canvas_delete_shape': {
        const canvasId = ctx.linkedCanvasIdFor(sessionId);
        if (!canvasId) return;
        const ids: string[] = [];
        const single = str('shape_id');
        if (single) ids.push(single);
        if (Array.isArray(input.shape_ids)) {
          for (const v of input.shape_ids) if (typeof v === 'string' && v) ids.push(v);
        }
        if (ids.length > 0) canvasDeleteShapes(canvasId, ids);
        return;
      }
      case 'mcp__app__canvas_add_edge': {
        const canvasId = ctx.linkedCanvasIdFor(sessionId);
        if (!canvasId) return;
        const edge = parseEdgeSpec(input);
        if (edge) canvasAddEdge(canvasId, edge);
        return;
      }
      case 'mcp__app__canvas_add_edges': {
        const canvasId = ctx.linkedCanvasIdFor(sessionId);
        if (!canvasId) return;
        /* Accept the canonical `edges` plus the same aliases the
           sidecar declares (`connections` / `links` / `arrows`). */
        const arr = (input.edges ?? input.connections ?? input.links ?? input.arrows);
        if (!Array.isArray(arr)) return;
        for (const raw of arr) {
          if (!raw || typeof raw !== 'object') continue;
          const edge = parseEdgeSpec(raw as Record<string, unknown>);
          if (edge) canvasAddEdge(canvasId, edge);
        }
        return;
      }
      case 'mcp__app__canvas_delete_edge': {
        const canvasId = ctx.linkedCanvasIdFor(sessionId);
        if (!canvasId) return;
        const ids: string[] = [];
        const single = str('edge_id');
        if (single) ids.push(single);
        if (Array.isArray(input.edge_ids)) {
          for (const v of input.edge_ids) if (typeof v === 'string' && v) ids.push(v);
        }
        if (ids.length > 0) canvasDeleteEdges(canvasId, ids);
        return;
      }
      case 'mcp__app__canvas_arrange': {
        const canvasId = ctx.linkedCanvasIdFor(sessionId);
        if (!canvasId) return;
        const algo = str('algorithm') as LayoutAlgorithm;
        if (!['grid', 'row', 'column', 'dagre'].includes(algo)) return;
        const ids = Array.isArray(input.shape_ids)
          ? (input.shape_ids as unknown[]).filter((v): v is string => typeof v === 'string')
          : undefined;
        const opts: Record<string, unknown> = {};
        if (typeof input.rankdir === 'string') opts.rankdir = input.rankdir;
        if (typeof input.gap === 'number') opts.gap = input.gap;
        void canvasApplyLayout(canvasId, algo, ids, opts);
        return;
      }
      case 'mcp__app__canvas_focus': {
        const canvasId = ctx.linkedCanvasIdFor(sessionId);
        if (!canvasId) return;
        const shapeId = str('shape_id');
        if (!shapeId) return;
        requestCanvasFocus(canvasId, shapeId);
        return;
      }
      case 'mcp__app__canvas_set_z': {
        const canvasId = ctx.linkedCanvasIdFor(sessionId);
        if (!canvasId) return;
        const shapeId = str('shape_id');
        const mode = str('mode');
        if (!shapeId) return;
        if (!(Z_MODES as readonly string[]).includes(mode)) return;
        canvasSetShapeZ(canvasId, shapeId, mode as typeof Z_MODES[number]);
        return;
      }
      case 'mcp__app__canvas_duplicate': {
        const canvasId = ctx.linkedCanvasIdFor(sessionId);
        if (!canvasId) return;
        const ids = Array.isArray(input.shape_ids)
          ? (input.shape_ids as unknown[]).filter((v): v is string => typeof v === 'string' && v.length > 0)
          : [];
        if (ids.length === 0) return;
        const dx = typeof input.dx === 'number' ? input.dx : 12;
        const dy = typeof input.dy === 'number' ? input.dy : 12;
        canvasDuplicateShapes(canvasId, ids, dx, dy);
        return;
      }
      case 'mcp__app__canvas_find': {
        const canvasId = ctx.linkedCanvasIdFor(sessionId);
        if (!canvasId) return;
        const query = str('query');
        if (!query) return;
        const ids = canvasFindShapes(canvasId, query);
        /* `find` is a read — but our sidecar reply is just a
           confirmation, so returning data through the agent would
           require either an IPC bridge or a follow-up message. We
           DO change UI state: select the matches so the user can
           visually see what the agent found. The agent's next-turn
           system-prompt preamble will reflect the new selection. */
        if (ids.length > 0) canvasSetSelection(canvasId, ids);
        return;
      }
      case 'mcp__app__canvas_group': {
        const canvasId = ctx.linkedCanvasIdFor(sessionId);
        if (!canvasId) return;
        const ids = Array.isArray(input.shape_ids)
          ? (input.shape_ids as unknown[]).filter((v): v is string => typeof v === 'string' && v.length > 0)
          : [];
        if (ids.length === 0) return;
        const kind = input.kind === 'group' ? 'group' : 'frame';
        const title = typeof input.title === 'string' ? input.title : undefined;
        canvasGroupShapes(canvasId, ids, { kind, title });
        return;
      }
      case 'mcp__app__canvas_ungroup': {
        const canvasId = ctx.linkedCanvasIdFor(sessionId);
        if (!canvasId) return;
        const shapeId = str('shape_id');
        if (!shapeId) return;
        canvasUngroupShapes(canvasId, shapeId);
        return;
      }
      case 'mcp__app__canvas_lock': {
        const canvasId = ctx.linkedCanvasIdFor(sessionId);
        if (!canvasId) return;
        const ids = Array.isArray(input.shape_ids)
          ? (input.shape_ids as unknown[]).filter((v): v is string => typeof v === 'string' && v.length > 0)
          : [];
        if (ids.length === 0) return;
        const locked = input.locked === true;
        canvasSetShapesLocked(canvasId, ids, locked);
        return;
      }
      case 'mcp__app__canvas_align': {
        const canvasId = ctx.linkedCanvasIdFor(sessionId);
        if (!canvasId) return;
        const ids = Array.isArray(input.shape_ids)
          ? (input.shape_ids as unknown[]).filter((v): v is string => typeof v === 'string' && v.length > 0)
          : [];
        const axis = str('axis');
        if (ids.length < 2 || !(ALIGN_AXES as string[]).includes(axis)) return;
        canvasAlignShapes(canvasId, ids, axis as AlignAxis);
        return;
      }
      case 'mcp__app__canvas_distribute': {
        const canvasId = ctx.linkedCanvasIdFor(sessionId);
        if (!canvasId) return;
        const ids = Array.isArray(input.shape_ids)
          ? (input.shape_ids as unknown[]).filter((v): v is string => typeof v === 'string' && v.length > 0)
          : [];
        const axis = str('axis');
        if (ids.length < 3 || (axis !== 'horizontal' && axis !== 'vertical')) return;
        canvasDistributeShapes(canvasId, ids, axis as DistributeAxis);
        return;
      }
      case 'mcp__app__canvas_set_viewport': {
        const canvasId = ctx.linkedCanvasIdFor(sessionId);
        if (!canvasId) return;
        const x = num('x'); const y = num('y');
        if (!Number.isFinite(x) || !Number.isFinite(y)) return;
        const c = ensureCanvasLoaded(canvasId);
        if (!c) return;
        const z = typeof input.zoom === 'number' && input.zoom > 0
          ? Math.max(0.1, Math.min(4, input.zoom))
          : c.viewport.zoom;
        canvasSetViewport(canvasId, { x, y, zoom: z });
        return;
      }
      case 'mcp__app__canvas_upload_image': {
        const canvasId = ctx.linkedCanvasIdFor(sessionId);
        if (!canvasId) return;
        const b64 = str('base64');
        if (!b64) return;
        const mime = str('mime_type') || 'image/png';
        const dataUrl = `data:${mime};base64,${b64}`;
        /* Use Image() to read intrinsic dimensions; fall back to a
           default size if decode fails. We can't await inside this
           switch elegantly, so this branch fires off an async task
           that creates the shape once dimensions resolve. */
        const c = ensureCanvasLoaded(canvasId);
        if (!c) return;
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
        return;
      }
    }
  };
}

/* `canvasState` is re-exported here so callers don't need a separate
 * import for the rare cross-check. Same for the canvas op aliases —
 * the dispatcher is the canonical user. */
export { canvasState };
