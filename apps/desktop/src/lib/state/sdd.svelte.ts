/* Spec-Driven Development orchestrator — frontend state machine.
 *
 * Wraps the Rust `sdd_*` Tauri commands in a reactive store. Each chat
 * session can have AT MOST one active SDD workspace; `workspaceBySession`
 * is the per-session index. We refresh the workspace from disk after
 * every agent turn that touches the workspace, so the UI cards stay in
 * sync with what the agent has written.
 *
 * This file owns:
 *   - the reactive store + per-session index
 *   - listener for `sdd:changed:*` Tauri events
 *   - prompt interpolation (spec / plan / phase templates)
 *   - high-level button-handler helpers the SDD card calls
 *
 * It does NOT own the chat send pipeline — to push a prompt to the
 * agent, callers `setSessionInput(...)` + `sendClaudeMessage()` from
 * the existing chat code path. Keeps SDD a thin shell over the same
 * machinery a manual user-typed prompt uses (no second send code path
 * to maintain).
 */

import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { subscribeToolEvent, type ToolStreamEvent } from '$lib/stream/agentStream';
import { formatToolUse } from '$lib/format';

// --- Mirrored types from `sdd.rs` ------------------------------------

export type SddStage =
  | { kind: 'drafting' }
  | { kind: 'spec_ready' }
  | { kind: 'planning' }
  | { kind: 'plan_ready' }
  | { kind: 'phase_pending_approval'; phase: number }
  | { kind: 'phase_running'; phase: number }
  | { kind: 'phase_verifying'; phase: number }
  | { kind: 'phase_done'; phase: number }
  | { kind: 'complete' }
  | { kind: 'paused' }
  | { kind: 'stopped' }
  | {
      kind: 'failed';
      reason: string;
      /** Phase number that failed. Optional for backward-compat with v1
       *  workspaces whose `Failed` payload is still the legacy
       *  reason-only shape. */
      failed_phase?: number | null;
      trigger?: SddFailureTrigger | null;
      /** Indices into the phase's acceptance array that ended in
       *  `failed`. Empty when trigger isn't `check_failed`. */
      failed_checks?: number[];
      /** Last ~10 action-log entries before failure, used to give the
       *  user context inside the failure card without an extra IPC. */
      action_log_tail?: ActionLogEntry[];
    };

/** Coarse classification of why a phase failed. Drives the failure
 *  card's header copy + which inline action makes sense. Mirrors
 *  `enum FailureTrigger` in `sdd.rs`. */
export type SddFailureTrigger =
  | 'check_failed'
  | 'timeout'
  | 'exception'
  | 'crash'
  | 'user_stopped';

// --- Phase diff (file changes between pre/post phase commits) --------

export interface DiffFile {
  path: string;
  /** Single-letter status: A=added, M=modified, D=deleted, R=renamed,
   *  C=copied, T=typechange. Renames carry `from_path`. */
  status: string;
  insertions: number;
  deletions: number;
  is_binary: boolean;
  from_path?: string | null;
}

export interface SddPhaseDiff {
  files: DiffFile[];
  total_insertions: number;
  total_deletions: number;
  /** True when the workspace doesn't have git, the phase had no
   *  pre-phase snapshot (dirty tree at approve time), or the phase
   *  hasn't completed. UI renders a placeholder row in that case. */
  skipped: boolean;
}

// --- Audit log (phase 6: self-driving MCP) ----------------------------

/** Source of a workspace mutation. `agent` = MCP tool call from the
 *  CLI; `user` = UI click; `system` = orchestrator-internal event
 *  (e.g. recovery on boot). Free-form on the wire so we can grow into
 *  e.g. `cli` without breaking the schema. */
export type AuditSource = 'agent' | 'user' | 'system' | string;

/** Single row of the workspace audit log. Mirrors `sdd::audit::AuditEntry`
 *  on the Rust side. `before` / `after` are free-form JSON snapshots of
 *  whatever fields the action changed — kept compact on purpose. */
export interface AuditEntry {
  ts: number;
  source: AuditSource;
  action: string;
  phase?: number | null;
  reason?: string | null;
  before?: unknown;
  after?: unknown;
}

/** Per-phase machine-verifiable acceptance criterion. Mirrors the Rust
 *  `SddPhaseAcceptance` enum (serde tag = "type"). Plan v2 sources the
 *  list from `plan.json`; phase 1 ships the schema only — phase 2 will
 *  start populating it from the agent's plan output. */
export type SddPhaseAcceptance =
  | { type: 'shell'; cmd: string; expect_exit: number; stdout_match?: string | null; timeout_ms?: number | null }
  | { type: 'file_exists'; paths: string[] }
  | { type: 'manual'; description: string };

export interface SddPlanPhase {
  number: number;
  slug: string;
  title: string;
  depends_on: number[];
  complexity: string | null;
  acceptance: SddPhaseAcceptance[];
}

export interface SddPlanFile {
  version: number;
  phases: SddPlanPhase[];
}

// --- Acceptance / verifier types (mirrors `sdd_verify.rs`) -----------

/** Per-check verdict written by the verifier. `manual_unmarked` is
 *  the initial state for `manual` checks until the user resolves
 *  via `markSddManualCheck`. */
export type AcceptanceCheckStatus =
  | 'pending'
  | 'passed'
  | 'failed'
  | 'skipped'
  | 'manual_unmarked';

export interface AcceptanceResult {
  check_index: number;
  /** Snake-cased discriminant of the source check shape. */
  kind: 'shell' | 'file_exists' | 'manual';
  status: AcceptanceCheckStatus;
  started_at: number;
  finished_at: number;
  exit_code?: number | null;
  log_tail?: string;
  note?: string;
}

/** Aggregate verdict the orchestrator uses to flip phase frontmatter.
 *  `manual_pending` keeps the phase in `running` so the user must
 *  resolve manuals before advancement. */
export type AcceptanceOverallStatus = 'passed' | 'failed' | 'manual_pending';

export interface PhaseAcceptanceFile {
  phase: number;
  overall_status: AcceptanceOverallStatus;
  started_at: number;
  finished_at: number;
  results: AcceptanceResult[];
}

export interface SddPhase {
  number: number;
  slug: string;
  title: string;
  depends_on: number[];
  status: string; // pending | running | done | failed | skipped
  tasks_total: number;
  tasks_done: number;
  body: string;
  path: string;
  summary: string | null;
}

export interface SddWorkspace {
  id: string;
  session_id: string | null;
  root: string;
  stage: SddStage;
  user_prompt: string;
  spec_path: string | null;
  spec_body: string | null;
  plan_path: string | null;
  plan_body: string | null;
  /** Final wrap-up at `<workspace>/SUMMARY.md` — populated after every
   *  phase is done. Null on running / paused / failed workflows. */
  summary_path: string | null;
  summary_body: string | null;
  phases: SddPhase[];
  created_at: number;
  updated_at: number;
  /** True when this workspace has a `plan.json` on disk — i.e. it was
   *  scaffolded under the v2 SDD flow (plan-as-data + per-phase gates).
   *  Legacy v1 workspaces stay on the old auto-advance path so older
   *  in-flight SDD sessions don't suddenly require manual approves. */
  is_v2: boolean;
  /** Crash-recovery hint computed on hydrate / refresh. Null in the
   *  happy path. When non-null, the SddCard surfaces a "Phase N
   *  interrupted" banner asking the user to rollback or keep state.
   *  Mirrors `enum SddRecoveryState` in `apps/desktop/src-tauri/src/sdd.rs`. */
  recovery_state: SddRecoveryState | null;
}

/** Tagged-union mirror of Rust's `SddRecoveryState`. Currently only one
 *  variant — `OrphanPhase` — but we keep the discriminator so adding
 *  e.g. `OrphanPlan` later is non-breaking. */
export type SddRecoveryState = {
  kind: "orphan_phase";
  phase: number;
  /** Sha to roll back to. Null when git was disabled at approve-time. */
  pre_phase_sha: string | null;
};

/** Compact git-row state returned by `getSddGitState`. Mirrors
 *  `crate::git::SddGitState`. UI hides the row entirely when
 *  `enabled = false` (workspace runs in degraded / no-git mode). */
export interface SddGitState {
  enabled: boolean;
  branch: string | null;
  on_sdd_branch: boolean;
  ahead: number;
  behind: number;
  dirty: boolean;
}

// --- Store -----------------------------------------------------------

interface SddStoreShape {
  /** All known workspaces — newest-first. */
  workspaces: SddWorkspace[];
  /** Session id → workspace id mapping. One SDD workspace per session
   *  at a time; if the user starts a second SDD in the same session
   *  the previous one is dropped (`/sdd` overwrites). */
  workspaceBySession: Record<string, string>;
  /** Per-workspace targeted listener handles — we tear these down on
   *  discard so dead workspaces don't keep waking the renderer. */
  unlistenByWorkspace: Record<string, UnlistenFn>;
  /** Global broadcast listener — one per app lifecycle. */
  globalUnlisten: UnlistenFn | null;
  /** One-level undo target per (workspace, target). Target key is
   *  `spec` / `plan` / `phase-NN`. `body` is the pre-save markdown;
   *  `savedAt` is the ms-since-epoch at which the save fired (used
   *  to time out the affordance after 30s). In-memory only — wiped
   *  on app restart by design (the spec is explicit on this). */
  undoByWorkspace: Record<string, Record<string, { body: string; savedAt: number }>>;
  /** Self-save debounce — `lastSelfSaveAt[wsId]` = ms timestamp of the
   *  most recent `saveSddBody` call from THIS app instance. The
   *  agent-rewrite-detection branch in the `sdd:changed` listener
   *  skips its auto-clear when the incoming change landed within
   *  ~500ms of our own save — otherwise our own write round-trip
   *  would false-positive as "agent rewrote it". */
  lastSelfSaveAt: Record<string, number>;
  /** Per-session toggle for the library panel. When true, ChatThread
   *  renders a `SddLibraryCard` listing every workspace on disk so
   *  the user can re-open / discard / inspect past specs. Pure UI
   *  state — not persisted, resets on app restart. */
  libraryOpenBySession: Record<string, boolean>;
  /** Deferred silent SDD prompts, keyed by SESSION id. Populated when
   *  the user clicks Approve/Continue while the chat session is still
   *  mid-turn (e.g. agent's previous reply hasn't fully streamed in).
   *  Drained by the post-turn cleanup in `+page.svelte` — fires the
   *  pending prompt silently as soon as the session frees up. Avoids
   *  the stuck-spinner bug where a clicked Approve would silently
   *  bail because the session was busy at click-time. */
  pendingSilentBySession: Record<string, string>;
  /** Workspace id rendered as a top-level read-only fullscreen overlay
   *  (opened from the header history popover). Independent from
   *  `workspaceBySession` — does NOT bind the workspace to the active
   *  chat, so the SddCard does NOT appear inline in the thread. */
  standaloneViewWorkspaceId: string | null;
  /** Hide-without-discard registry. When a workspace id lands here,
   *  ChatThread's inlineActions skips rendering the matching SddCard;
   *  the workspace files stay on disk + the header popover still
   *  lists it. User unhides by clicking the row in the SDD history
   *  popover OR by typing in the chat (SDD card re-opens itself on
   *  the next agent turn that touches the workspace). Pure UI state
   *  — resets on app restart. */
  hiddenWorkspaceIds: Record<string, true>;
  /** Per-(workspace, phase) live-feed of `tool_use` / `tool_result` /
   *  agent-message events. Indexed `[wsId][phase]` -> ring-buffered
   *  array (cap 100). Mirrors the JSONL on disk under
   *  `<workspace>/phases/phase-<N>.log.jsonl`; the in-memory copy is
   *  the live fast path, disk is for crash-recovery rehydration. */
  actionLogByWorkspace: Record<string, Record<number, ActionLogEntry[]>>;
}

export type ActionKind = 'tool_use' | 'tool_result' | 'agent_message' | 'sdd_event';

export interface ActionLogEntry {
  /** Unix-ms when the entry was produced. */
  ts: number;
  /** Owning phase (matches the SDD workspace's currently-running phase). */
  phase: number;
  kind: ActionKind;
  /** Tool name verbatim from the CLI: `"Read"`, `"Edit"`, `"Bash"`,
   *  `"mcp__github__get_pr"`, … Optional for `agent_message`/
   *  `sdd_event` rows. */
  tool?: string;
  /** ≤80-char one-liner used for the inline pill in SddCard. */
  summary: string;
  /** Optional expandable detail (full bash command, full mcp args). */
  detail?: string;
  /** Lifecycle for tool_use lifecycles: `running` (after tool_use,
   *  before tool_result), `done`, `failed`. */
  status?: 'running' | 'done' | 'failed';
  /** Stable id from the CLI's `tool_use_id` — lets the listener flip a
   *  `running` row to `done`/`failed` when the matching `tool_result`
   *  arrives instead of stacking two rows. */
  correlation_id?: string;
}

/** Soft cap for the in-memory feed per (workspace, phase). The full
 *  history lives in the JSONL on disk; this number governs only how
 *  much we keep hot for the inline SddCard pill list. 100 = cheap to
 *  render, large enough that a single phase rarely overflows. */
const ACTION_LOG_CAP = 100;

export const sddState = $state<SddStoreShape>({
  workspaces: [],
  workspaceBySession: {},
  unlistenByWorkspace: {},
  globalUnlisten: null,
  undoByWorkspace: {},
  lastSelfSaveAt: {},
  libraryOpenBySession: {},
  pendingSilentBySession: {},
  standaloneViewWorkspaceId: null,
  hiddenWorkspaceIds: {},
  actionLogByWorkspace: {},
});

/** Hide the inline SddCard for `workspaceId` without deleting the
 *  workspace. Files on disk untouched; the header popover still shows
 *  the row. Mirror of `showSddCard` — they form a pair. */
export function hideSddCard(workspaceId: string): void {
  sddState.hiddenWorkspaceIds = { ...sddState.hiddenWorkspaceIds, [workspaceId]: true };
}

/** Restore an inline SddCard previously hidden via `hideSddCard`. No-op
 *  when the workspace isn't currently hidden. */
export function showSddCard(workspaceId: string): void {
  if (!sddState.hiddenWorkspaceIds[workspaceId]) return;
  const next = { ...sddState.hiddenWorkspaceIds };
  delete next[workspaceId];
  sddState.hiddenWorkspaceIds = next;
}

/** Predicate. Used by ChatThread's inlineActions snippet to skip
 *  rendering. */
export function isSddCardHidden(workspaceId: string): boolean {
  return !!sddState.hiddenWorkspaceIds[workspaceId];
}

/** Open a workspace as a top-level read-only fullscreen overlay. Used
 *  by the header history popover. Does not bind the workspace to any
 *  session, so the chat thread stays clean. */
export function openStandaloneView(workspaceId: string): void {
  sddState.standaloneViewWorkspaceId = workspaceId;
}
export function closeStandaloneView(): void {
  sddState.standaloneViewWorkspaceId = null;
}

/** Toggle the library panel for a session. */
export function toggleSddLibrary(sessionId: string): void {
  sddState.libraryOpenBySession[sessionId] = !sddState.libraryOpenBySession[sessionId];
}

/** Bind an existing workspace to a session — used when the user
 *  re-opens a workspace from the library. Replaces whatever was
 *  active (if anything) without deleting it; the prior workspace
 *  stays on disk and remains discoverable via the library list. */
export function bindWorkspaceToSession(sessionId: string, workspaceId: string): void {
  sddState.workspaceBySession[sessionId] = workspaceId;
  sddState.libraryOpenBySession[sessionId] = false;
}

/** Park a silent SDD prompt that couldn't fire right now (session
 *  busy). The post-turn drain in `+page.svelte` picks it up + fires
 *  silently. One slot per session — a second click overwrites; the
 *  newest stage-derived prompt is always the right one to send. */
export function setPendingSilent(sessionId: string, prompt: string): void {
  sddState.pendingSilentBySession[sessionId] = prompt;
}
export function popPendingSilent(sessionId: string): string | null {
  const v = sddState.pendingSilentBySession[sessionId];
  if (v === undefined) return null;
  delete sddState.pendingSilentBySession[sessionId];
  return v;
}

/** Stable key for the (target) addressed by a save action. Keeps
 *  spec/plan/phase routes pulling from the same record without
 *  having to thread a discriminated union through every store op. */
export function targetKey(
  target: { kind: 'spec' } | { kind: 'plan' } | { kind: 'phase'; number: number }
): string {
  if (target.kind === 'spec') return 'spec';
  if (target.kind === 'plan') return 'plan';
  return `phase-${target.number}`;
}

/** Stash a pre-save snapshot so the SddCard's "Undo last edit"
 *  affordance can restore it within 30s. Also stamps `lastSelfSaveAt`
 *  so the listener's agent-rewrite-detection branch knows the
 *  upcoming `sdd:changed` event is ours, not the agent's. */
export function stashUndo(workspaceId: string, key: string, prevBody: string): void {
  const slots = sddState.undoByWorkspace[workspaceId] ?? {};
  slots[key] = { body: prevBody, savedAt: Date.now() };
  sddState.undoByWorkspace[workspaceId] = slots;
  sddState.lastSelfSaveAt[workspaceId] = Date.now();
}

/** Read + clear the undo slot. Returns the prior body, or null. */
export function popUndo(workspaceId: string, key: string): string | null {
  const slots = sddState.undoByWorkspace[workspaceId];
  if (!slots) return null;
  const slot = slots[key];
  if (!slot) return null;
  delete slots[key];
  /* Stamp lastSelfSaveAt again — the upcoming write-back from the
   *  caller's `saveSddBody` will also emit `sdd:changed`, which the
   *  listener would otherwise treat as an agent rewrite. */
  sddState.lastSelfSaveAt[workspaceId] = Date.now();
  return slot.body;
}

/** Compare a stashed undo body against the workspace's current body
 *  for the same target key. Returns true when they DIFFER, meaning
 *  the agent has rewritten the file since we stashed. Used by the
 *  listener to drop stale undo slots. */
function bodyForKey(ws: SddWorkspace, key: string): string | null {
  if (key === 'spec') return ws.spec_body;
  if (key === 'plan') return ws.plan_body;
  if (key.startsWith('phase-')) {
    const n = Number(key.slice('phase-'.length));
    const ph = ws.phases.find((p) => p.number === n);
    return ph?.body ?? null;
  }
  return null;
}

/** Initialise the global `sdd:changed` listener AND hydrate from disk.
 *  Idempotent — safe to call from `+page.svelte` onMount more than
 *  once. Hydration rebuilds any workspaces that existed before app
 *  restart, so the user's previous SDD session resumes seamlessly. */
export async function initSdd(): Promise<void> {
  if (sddState.globalUnlisten) return;
  sddState.globalUnlisten = await listen<string>('sdd:changed', async (evt) => {
    /* Broadcast carries the workspace id. Re-pull just that one — much
     *  cheaper than `sdd_list` on every event. */
    const id = evt.payload;
    try {
      const ws = await invoke<SddWorkspace>('sdd_get', { id });
      upsertWorkspace(ws);
      /* Agent-rewrite detection. For every stashed undo slot whose
       *  underlying body now DIFFERS from the snapshot we stashed at
       *  save time AND the change DIDN'T land within ~500ms of our
       *  own save (which would be the round-trip from our own
       *  saveSddBody emit), drop the slot — the user undo'ing back
       *  to an old body the agent has since rewritten would be a
       *  footgun. */
      const slots = sddState.undoByWorkspace[id];
      if (slots) {
        const lastSelf = sddState.lastSelfSaveAt[id] ?? 0;
        const now = Date.now();
        if (now - lastSelf > 500) {
          for (const key of Object.keys(slots)) {
            const current = bodyForKey(ws, key);
            if (current !== null && current !== slots[key].body) {
              delete slots[key];
            }
          }
        }
      }
    } catch {
      /* Workspace was discarded between emit + receive — fine, drop. */
      removeWorkspace(id);
    }
  });
  /* Hydrate any workspaces left on disk from a prior run. The Rust
   *  side scans `<app_data>/sdd-workspaces/*` and rebuilds in-memory
   *  state from the files. Failures here are non-fatal — worst case
   *  the user has to /sdd again. */
  try {
    const ws = await invoke<SddWorkspace[]>('sdd_hydrate');
    for (const w of ws) upsertWorkspace(w);
  } catch (e) {
    console.warn('sdd_hydrate failed', e);
  }
  /* Wire the agent-stream tool-event listener once and seed the live
   *  feed for any workspace already in `phase_running` from the
   *  previous app run — keeps the SddCard's "live activity" pane
   *  populated immediately on boot instead of waiting for the next
   *  tool_use to land. */
  attachActionLogListener();
  await rehydrateActionLogs();
}

function upsertWorkspace(ws: SddWorkspace): void {
  const idx = sddState.workspaces.findIndex((w) => w.id === ws.id);
  if (idx === -1) {
    sddState.workspaces = [ws, ...sddState.workspaces];
  } else {
    const copy = [...sddState.workspaces];
    copy[idx] = ws;
    sddState.workspaces = copy;
  }
  if (ws.session_id) {
    sddState.workspaceBySession[ws.session_id] = ws.id;
  }
}

function removeWorkspace(id: string): void {
  sddState.workspaces = sddState.workspaces.filter((w) => w.id !== id);
  for (const [sid, wid] of Object.entries(sddState.workspaceBySession)) {
    if (wid === id) delete sddState.workspaceBySession[sid];
  }
  const un = sddState.unlistenByWorkspace[id];
  if (un) {
    try { un(); } catch { /* noop */ }
    delete sddState.unlistenByWorkspace[id];
  }
}

/** Active workspace for a session, or null. */
export function workspaceForSession(sessionId: string): SddWorkspace | null {
  const wid = sddState.workspaceBySession[sessionId];
  if (!wid) return null;
  return sddState.workspaces.find((w) => w.id === wid) ?? null;
}

// --- Commands --------------------------------------------------------

/** Kick off a new SDD workspace bound to the chat session. If a
 *  workspace is already attached to this session, it's discarded first
 *  (one SDD per session, simplest model). */
export async function startSdd(
  sessionId: string,
  userPrompt: string,
  /** Absolute repo cwd of the linked agent / editor. Optional — when
   *  passed AND it's actually a git repo, the orchestrator mints
   *  `sdd/<id>` and snapshots / commits per phase against it. Null
   *  → workspace runs in degraded "no-git" mode (no rollback). */
  repoCwd?: string | null
): Promise<SddWorkspace | null> {
  // Discard any existing workspace tied to this session.
  const existing = sddState.workspaceBySession[sessionId];
  if (existing) {
    try { await invoke('sdd_discard', { id: existing }); } catch { /* noop */ }
    removeWorkspace(existing);
  }
  try {
    const ws = await invoke<SddWorkspace>('sdd_start', {
      args: {
        session_id: sessionId,
        user_prompt: userPrompt,
        repo_cwd: repoCwd ?? null,
      },
    });
    upsertWorkspace(ws);
    return ws;
  } catch (e) {
    console.warn('sdd_start failed', e);
    return null;
  }
}

/** Re-read workspace state from disk. Call after the agent finishes a
 *  turn that should have written files (spec.md, plan.md, phase
 *  files). The Rust side derives the stage from what's on disk. */
export async function refreshSdd(id: string): Promise<SddWorkspace | null> {
  try {
    const ws = await invoke<SddWorkspace>('sdd_refresh', { id });
    upsertWorkspace(ws);
    return ws;
  } catch (e) {
    console.warn('sdd_refresh failed', e);
    return null;
  }
}

export async function approveSdd(
  id: string,
  target: 'spec' | 'plan'
): Promise<SddWorkspace | null> {
  try {
    const ws = await invoke<SddWorkspace>('sdd_approve', { id, target });
    upsertWorkspace(ws);
    return ws;
  } catch (e) {
    console.warn('sdd_approve failed', e);
    return null;
  }
}

/** Save user-edited body for the spec, plan, or a specific phase. The
 *  YAML frontmatter is preserved verbatim on the Rust side. */
export async function saveSddBody(
  id: string,
  target: { kind: 'spec' } | { kind: 'plan' } | { kind: 'phase'; number: number },
  body: string
): Promise<SddWorkspace | null> {
  const args = target.kind === 'phase'
    ? { kind: 'phase', number: target.number }
    : { kind: target.kind };
  try {
    const ws = await invoke<SddWorkspace>('sdd_save_body', { id, target: args, body });
    upsertWorkspace(ws);
    return ws;
  } catch (e) {
    console.warn('sdd_save_body failed', e);
    return null;
  }
}

/** Reset a failed (or done) phase back to `pending`. The next
 *  advance fires the phase prompt again with a fresh status. */
export async function retrySddPhase(
  id: string,
  phase: number
): Promise<SddWorkspace | null> {
  try {
    const ws = await invoke<SddWorkspace>('sdd_retry_phase', { id, phase });
    upsertWorkspace(ws);
    return ws;
  } catch (e) {
    console.warn('sdd_retry_phase failed', e);
    return null;
  }
}

/** Force-skip a phase with a mandatory reason — used by the failure
 *  card's "Skip phase" button to bypass a verifier-flagged failure.
 *  Backend rejects reasons under 5 chars after trim, so callers should
 *  validate or surface the error. */
export async function skipSddPhaseWithReason(
  id: string,
  phase: number,
  reason: string
): Promise<SddWorkspace | null> {
  try {
    const ws = await invoke<SddWorkspace>('sdd_skip_phase_with_reason', {
      id,
      phase,
      reason,
    });
    upsertWorkspace(ws);
    return ws;
  } catch (e) {
    console.warn('sdd_skip_phase_with_reason failed', e);
    return null;
  }
}

/** Phase-level diff (per-file stats) between pre/post phase SHAs.
 *  Returns `{ skipped: true }` for non-git workspaces, dirty-tree
 *  approves, or phases that haven't completed yet — UI handles all
 *  three the same way. */
export async function getSddPhaseDiff(
  id: string,
  phase: number
): Promise<SddPhaseDiff | null> {
  try {
    return await invoke<SddPhaseDiff>('sdd_get_phase_diff', { id, phase });
  } catch (e) {
    console.warn('sdd_get_phase_diff failed', e);
    return null;
  }
}

/** Phase 6: read the workspace audit log (every mutation across
 *  agent / user / system, oldest-first). Returns [] on missing file or
 *  unknown workspace — UI hides the indicator in that case. */
export async function loadAuditLog(id: string): Promise<AuditEntry[]> {
  try {
    return await invoke<AuditEntry[]>('sdd_audit_read', { id });
  } catch (e) {
    console.warn('sdd_audit_read failed', e);
    return [];
  }
}

/** Phase 6: append a single audit entry. Used by the frontend stream
 *  parser when intercepting `mcp__app__sdd_*` mutating tool_use events,
 *  AND by user-flow handlers as an extra layer alongside the
 *  Rust-side audit hooks (single source of truth: dedupes are fine —
 *  worst case one mutation has two rows, which is benign). */
export async function appendAuditLog(
  id: string,
  entry: AuditEntry
): Promise<void> {
  try {
    await invoke('sdd_audit_append', { id, entry });
  } catch (e) {
    console.warn('sdd_audit_append failed', e);
  }
}

/** Lazy fetch of the unified-diff patch for a single file inside a
 *  phase. Empty string when pre/post SHAs are missing — callers
 *  should hide the row body in that case. */
export async function getSddFileDiff(
  id: string,
  phase: number,
  path: string
): Promise<string> {
  try {
    return await invoke<string>('sdd_get_file_diff', { id, phase, path });
  } catch (e) {
    console.warn('sdd_get_file_diff failed', e);
    return '';
  }
}

/** v2 only — clear the per-phase approval gate for `phase`. Writes
 *  `<root>/control/phase-N-approved` so derive_stage falls through and
 *  the phase prompt fires on the next advance. */
export async function approveSddPhase(
  id: string,
  phase: number
): Promise<SddWorkspace | null> {
  try {
    const ws = await invoke<SddWorkspace>('sdd_approve_phase', { id, phase });
    upsertWorkspace(ws);
    return ws;
  } catch (e) {
    console.warn('sdd_approve_phase failed', e);
    return null;
  }
}

/** v2 only — mark a pending phase as `skipped` so the workflow moves
 *  on to the next gate. `reason` is recorded in the phase frontmatter
 *  for audit. */
export async function skipSddPhase(
  id: string,
  phase: number,
  reason: string
): Promise<SddWorkspace | null> {
  try {
    const ws = await invoke<SddWorkspace>('sdd_skip_phase', { id, phase, reason });
    upsertWorkspace(ws);
    return ws;
  } catch (e) {
    console.warn('sdd_skip_phase failed', e);
    return null;
  }
}

/** v2 only — insert a new phase after `after_number`. The Rust side
 *  renumbers + renames subsequent phase files atomically and rewrites
 *  `plan.json`. Pass `after_number = 0` to insert at the very top. */
export async function insertSddPhase(
  id: string,
  after_number: number,
  title: string,
  depends_on: number[]
): Promise<SddWorkspace | null> {
  try {
    const ws = await invoke<SddWorkspace>('sdd_insert_phase', {
      id,
      after_number,
      title,
      depends_on,
    });
    upsertWorkspace(ws);
    return ws;
  } catch (e) {
    console.warn('sdd_insert_phase failed', e);
    return null;
  }
}

/** v2 only — reorder phases. `new_order` is a list of current phase
 *  numbers in the order they should end up (1-indexed final positions).
 *  Rust renumbers and renames the markdown files via a two-pass
 *  staging strategy. */
export async function reorderSddPhases(
  id: string,
  new_order: number[]
): Promise<SddWorkspace | null> {
  try {
    const ws = await invoke<SddWorkspace>('sdd_reorder_phases', { id, new_order });
    upsertWorkspace(ws);
    return ws;
  } catch (e) {
    console.warn('sdd_reorder_phases failed', e);
    return null;
  }
}

/** v2 only — delete a phase by number. Rejected with Err if the phase
 *  is currently `running`. Subsequent phases shift down by one. */
export async function deleteSddPhase(
  id: string,
  phase: number
): Promise<SddWorkspace | null> {
  try {
    const ws = await invoke<SddWorkspace>('sdd_delete_phase', { id, phase });
    upsertWorkspace(ws);
    return ws;
  } catch (e) {
    console.warn('sdd_delete_phase failed', e);
    return null;
  }
}

/** v2 only — run the acceptance checks for `phase` and persist the
 *  per-phase result file. Backend flips phase frontmatter to
 *  `done` / `failed` based on the aggregate verdict; on
 *  `manual_pending` the status stays `running` and the user clears
 *  the gate via `markSddManualCheck`.
 *
 *  Resolves with the fresh workspace snapshot. The result file
 *  itself is read separately via `readPhaseAcceptance`. */
export async function runSddVerification(
  id: string,
  phase: number
): Promise<SddWorkspace | null> {
  try {
    const ws = await invoke<SddWorkspace>('sdd_run_verification', { id, phase });
    upsertWorkspace(ws);
    return ws;
  } catch (e) {
    console.warn('sdd_run_verification failed', e);
    return null;
  }
}

/** v2 only — resolve a manual acceptance check. `passed = true` flips
 *  it to `passed`, `false` to `failed`. The backend recomputes the
 *  aggregate verdict and (if it now flips to a terminal state)
 *  updates phase frontmatter. */
export async function markSddManualCheck(
  id: string,
  phase: number,
  check_index: number,
  passed: boolean
): Promise<SddWorkspace | null> {
  try {
    const ws = await invoke<SddWorkspace>('sdd_mark_manual_check', {
      id,
      phase,
      check_index,
      passed,
    });
    upsertWorkspace(ws);
    return ws;
  } catch (e) {
    console.warn('sdd_mark_manual_check failed', e);
    return null;
  }
}

/** Generate a `plan.json` for a legacy v1 workspace from its existing
 *  phase markdown files. After this, `is_v2` flips to true and the
 *  per-phase approval gate kicks in for the next pending phase. */
export async function upgradeSddWorkspace(
  id: string
): Promise<SddWorkspace | null> {
  try {
    const ws = await invoke<SddWorkspace>('sdd_upgrade_workspace', { id });
    upsertWorkspace(ws);
    return ws;
  } catch (e) {
    console.warn('sdd_upgrade_workspace failed', e);
    return null;
  }
}

/** Roll a phase back to its `pre_phase_sha` snapshot. Errors when the
 *  phase has no recorded snapshot (git was off / phase running). The
 *  user's pre-rollback worktree is safety-stashed first under
 *  `sdd-rollback-safety-<phase>-<id>` so they can recover if they
 *  rolled back by accident. */
export async function rollbackSddPhase(
  id: string,
  phase: number
): Promise<SddWorkspace | null> {
  try {
    const ws = await invoke<SddWorkspace>('sdd_rollback_phase', { id, phase });
    upsertWorkspace(ws);
    return ws;
  } catch (e) {
    console.warn('sdd_rollback_phase failed', e);
    return null;
  }
}

/** Resolve a detected crash-recovery situation. `action` is either
 *  `"rollback"` (jumps to the orphan phase's pre-snapshot) or
 *  `"keep"` (flips the phase to `failed` so the user decides how to
 *  proceed). */
export async function recoverSddWorkspace(
  id: string,
  action: 'rollback' | 'keep'
): Promise<SddWorkspace | null> {
  try {
    const ws = await invoke<SddWorkspace>('sdd_recover_workspace', { id, action });
    upsertWorkspace(ws);
    return ws;
  } catch (e) {
    console.warn('sdd_recover_workspace failed', e);
    return null;
  }
}

/** Compact git-row state for the SddCard header. Returns
 *  `{ enabled: false, ... }` for non-git workspaces — UI hides the
 *  row in that case rather than showing an empty branch. */
export async function getSddGitState(id: string): Promise<SddGitState | null> {
  try {
    return await invoke<SddGitState>('sdd_get_git_state', { id });
  } catch (e) {
    console.warn('sdd_get_git_state failed', e);
    return null;
  }
}

export async function pauseSdd(id: string): Promise<void> {
  try { await invoke('sdd_pause', { id }); } catch (e) { console.warn(e); }
}
export async function resumeSdd(id: string): Promise<void> {
  try { await invoke('sdd_resume', { id }); } catch (e) { console.warn(e); }
}
export async function stopSdd(id: string): Promise<void> {
  try { await invoke('sdd_stop', { id }); } catch (e) { console.warn(e); }
}
export async function discardSdd(id: string): Promise<void> {
  try { await invoke('sdd_discard', { id }); } catch (e) { console.warn(e); }
  removeWorkspace(id);
}

// --- Prompt assembly -------------------------------------------------

type PromptKind = 'spec' | 'plan' | 'phase' | 'summary' | 'amend';

/** Fetch a prompt template from the Rust side. Templates are embedded
 *  via `include_str!` at build time so they ship with the binary. */
async function fetchPrompt(kind: PromptKind): Promise<string> {
  return await invoke<string>('sdd_prompt', { kind });
}

/** Substitute `{{key}}` placeholders. Keeps placeholders that weren't
 *  provided so an unfilled template is visible in the chat (easier
 *  debugging than a silently-blank prompt). */
function interpolate(tpl: string, vars: Record<string, string>): string {
  return tpl.replace(/\{\{(\w+)\}\}/g, (_m, k) => (vars[k] !== undefined ? vars[k] : `{{${k}}}`));
}

/** Build the prompt the agent should receive for the workspace's
 *  CURRENT stage. Returns null when the stage doesn't have an agent-
 *  facing prompt (Complete / Stopped / Paused / Failed).
 *
 *  The caller (UI / slash handler) drops this into `session.input`
 *  and fires `sendClaudeMessage()` so the existing send pipeline
 *  handles hooks, history, etc. */
export async function buildPromptForStage(ws: SddWorkspace): Promise<string | null> {
  const root = ws.root;
  switch (ws.stage.kind) {
    case 'drafting': {
      const tpl = await fetchPrompt('spec');
      return interpolate(tpl, { workspace_root: root, user_prompt: ws.user_prompt });
    }
    case 'planning': {
      const tpl = await fetchPrompt('plan');
      return interpolate(tpl, { workspace_root: root, user_prompt: ws.user_prompt });
    }
    case 'plan_ready':
    case 'phase_done': {
      /* Plan approved (or previous phase done) — find next phase to
       *  execute. Sequential: lowest-numbered phase still in `pending`
       *  status. */
      const next = ws.phases.find((p) => p.status === 'pending');
      if (!next) return null;
      const tpl = await fetchPrompt('phase');
      return interpolate(tpl, {
        workspace_root: root,
        workspace_id: ws.id,
        user_prompt: ws.user_prompt,
        phase_number: String(next.number),
        phase_slug: next.slug,
        phase_file: `${next.slug}.md`,
        retries_max: '1',
      });
    }
    case 'complete': {
      /* Workflow finished — agent writes the final wrap-up.
       *  Returned ONLY if no SUMMARY.md exists yet, so the prompt
       *  fires once per workspace. Caller (orchestrator post-turn
       *  hook) detects this and silently sends it. */
      if (ws.summary_body) return null;
      const tpl = await fetchPrompt('summary');
      return interpolate(tpl, { workspace_root: root, user_prompt: ws.user_prompt });
    }
    case 'phase_running':
      return null; // already in flight
    case 'phase_verifying':
      return null; // verifier is running — agent has nothing to do
    case 'phase_pending_approval':
      return null; // waiting on user — gate cleared via approveSddPhase
    case 'spec_ready':
      return null; // waiting on user approve
    default:
      return null;
  }
}

/** Convenience for the slash command. Returns interpolated spec prompt
 *  ready to send. */
export async function buildKickoffPrompt(
  ws: SddWorkspace
): Promise<string> {
  const tpl = await fetchPrompt('spec');
  return interpolate(tpl, {
    workspace_root: ws.root,
    user_prompt: ws.user_prompt,
  });
}

/** Build the in-place amend prompt — used when the user wants to
 *  correct the current spec / plan / phase instead of approving it.
 *  The agent edits files under `ws.root` rather than scaffolding a
 *  fresh workspace. Caller drops the result into the composer and
 *  fires the normal send pipeline. */
export async function buildAmendPrompt(
  ws: SddWorkspace,
  userChange: string
): Promise<string> {
  const tpl = await fetchPrompt('amend');
  return interpolate(tpl, {
    workspace_root: ws.root,
    user_prompt: ws.user_prompt,
    stage_kind: ws.stage.kind,
    user_change: userChange.trim(),
  });
}

// --- Live action log -------------------------------------------------------
// Per-(workspace, phase) feed of tool_use / tool_result events the agent
// emits. Hot path: tool event → push into in-memory buffer → debounced
// flush to disk. Cold path: app boot → read JSONL for any phase still
// `running` → seed the buffer so the SddCard live feed isn't empty after
// restart.

/** Resolve the (workspace, phase) live-feed should target for a session.
 *  Returns null when the session isn't bound to an SDD workspace OR the
 *  workspace isn't in `phase_running` (we deliberately drop tool events
 *  outside running phases — feeding a `phase_done` log would cross-pollute
 *  the next phase when it kicks off). */
export function getSddPhaseForSession(
  sessionId: string,
): { workspaceId: string; phase: number } | null {
  const ws = workspaceForSession(sessionId);
  if (!ws) return null;
  if (ws.stage.kind !== 'phase_running') return null;
  return { workspaceId: ws.id, phase: ws.stage.phase };
}

/** Read-only view: returns the current in-memory buffer for the given
 *  (workspace, phase). Empty when nothing has been logged yet. */
export function actionLogFor(workspaceId: string, phase: number): ActionLogEntry[] {
  return sddState.actionLogByWorkspace[workspaceId]?.[phase] ?? [];
}

/** Append a single entry to the in-memory ring buffer + queue it for
 *  disk flush. Exposed so the SDD orchestrator itself can log
 *  `sdd_event` rows (phase started, verifier ran, …) — the agent
 *  stream listener uses the internal `pushActionEntry` directly. */
export function appendSddActionLog(workspaceId: string, entry: ActionLogEntry): void {
  pushActionEntry(workspaceId, entry);
  queueAppend(workspaceId, entry);
}

function pushActionEntry(workspaceId: string, entry: ActionLogEntry): void {
  const byWs = sddState.actionLogByWorkspace[workspaceId] ?? {};
  const prev = byWs[entry.phase] ?? [];
  /* Correlate tool_result back to its tool_use by replacing the running
   *  row instead of stacking a second one. The stable correlation_id
   *  (CLI's tool_use_id) lets us flip a `running` row to `done` /
   *  `failed` without losing the original summary. Falls back to a
   *  plain push when the result has no correlation_id (legacy events
   *  or a result for a dropped tool_use). */
  let next: ActionLogEntry[];
  if (entry.kind === 'tool_result' && entry.correlation_id) {
    const idx = prev.findIndex(
      (e) => e.kind === 'tool_use' && e.correlation_id === entry.correlation_id,
    );
    if (idx !== -1) {
      next = prev.slice();
      next[idx] = {
        ...prev[idx],
        ts: entry.ts,
        status: entry.status ?? prev[idx].status,
        detail: entry.detail ?? prev[idx].detail,
      };
    } else {
      next = [...prev, entry];
    }
  } else {
    next = [...prev, entry];
  }
  if (next.length > ACTION_LOG_CAP) {
    next = next.slice(next.length - ACTION_LOG_CAP);
  }
  sddState.actionLogByWorkspace = {
    ...sddState.actionLogByWorkspace,
    [workspaceId]: { ...byWs, [entry.phase]: next },
  };
}

// Debounced batch flush: 250ms after the last queued event, ship all
// pending entries to disk in one Tauri call. Tracks pending per-workspace
// so two parallel SDDs don't share a flush window.
const pendingFlush: Record<string, ActionLogEntry[]> = {};
const flushTimer: Record<string, ReturnType<typeof setTimeout>> = {};
const FLUSH_DELAY_MS = 250;

function queueAppend(workspaceId: string, entry: ActionLogEntry): void {
  const buf = (pendingFlush[workspaceId] ??= []);
  buf.push(entry);
  if (flushTimer[workspaceId]) clearTimeout(flushTimer[workspaceId]);
  flushTimer[workspaceId] = setTimeout(() => flushPendingAppends(workspaceId), FLUSH_DELAY_MS);
}

async function flushPendingAppends(workspaceId: string): Promise<void> {
  const entries = pendingFlush[workspaceId];
  if (!entries || entries.length === 0) return;
  delete pendingFlush[workspaceId];
  delete flushTimer[workspaceId];
  try {
    await invoke('sdd_append_action_log_batch', { id: workspaceId, entries });
  } catch (e) {
    console.warn('[sdd] action_log flush failed', e);
  }
}

/** Build the human ≤80-char summary for the inline pill. Reuses
 *  `formatToolUse` from the chat-trace path so the wording matches. */
function summariseTool(toolName: string, input: Record<string, unknown>): string {
  // formatToolUse returns markdown like `> *Read* \`foo.rs\`` — strip the
  // trace decoration so the inline pill stays tight.
  const formatted = formatToolUse(toolName, input) ?? '';
  const stripped = formatted
    .replace(/^>\s*/, '')
    .replace(/\*([^*]+)\*/g, '$1')
    .replace(/`([^`]+)`/g, '$1')
    .trim();
  const fallback = `${toolName}`;
  const text = stripped || fallback;
  return text.length > 80 ? text.slice(0, 77) + '…' : text;
}

/** Compact a tool's raw input into a `detail` string for the lightbox.
 *  Just JSON.stringify with reasonable truncation — the lightbox can
 *  show long detail in a scrollable region. */
function detailFromInput(input: Record<string, unknown>): string | undefined {
  try {
    const s = JSON.stringify(input);
    if (!s || s === '{}') return undefined;
    return s.length > 4096 ? s.slice(0, 4093) + '…' : s;
  } catch {
    return undefined;
  }
}

let toolListenerAttached = false;
function attachActionLogListener(): void {
  if (toolListenerAttached) return;
  toolListenerAttached = true;
  subscribeToolEvent((evt: ToolStreamEvent) => {
    /* Resolve the owning SDD workspace + phase. Drop tool events from
     *  sessions not currently inside a running SDD phase — the feed
     *  is per-phase, and bleeding events from non-SDD chats would
     *  pollute the JSONL on disk. */
    const target = getSddPhaseForSession(evt.sessionId);
    if (!target) return;
    const { workspaceId, phase } = target;
    if (evt.kind === 'tool_use') {
      const entry: ActionLogEntry = {
        ts: Date.now(),
        phase,
        kind: 'tool_use',
        tool: evt.toolName,
        summary: summariseTool(evt.toolName, evt.input ?? {}),
        detail: detailFromInput(evt.input ?? {}),
        status: 'running',
        correlation_id: evt.toolUseId || undefined,
      };
      pushActionEntry(workspaceId, entry);
      queueAppend(workspaceId, entry);
      return;
    }
    /* tool_result: flip the matching `running` row to `done` /
     *  `failed`. We persist the result row too (so the JSONL retains
     *  the lifecycle pair); the in-memory push collapses them by
     *  correlation_id so the inline feed stays uncluttered. */
    const status = evt.isError ? 'failed' : 'done';
    const entry: ActionLogEntry = {
      ts: Date.now(),
      phase,
      kind: 'tool_result',
      tool: evt.toolName || undefined,
      summary: evt.toolName ? `${evt.toolName} ${status}` : status,
      status,
      correlation_id: evt.toolUseId || undefined,
    };
    pushActionEntry(workspaceId, entry);
    queueAppend(workspaceId, entry);
  });
}

/** Pull the persisted JSONL for any workspace currently in
 *  `phase_running` and seed the in-memory buffer. Idempotent — if
 *  the buffer is already populated (e.g. the listener captured live
 *  events first) we skip the disk read. Called from `initSdd` after
 *  hydrate. */
async function rehydrateActionLogs(): Promise<void> {
  for (const ws of sddState.workspaces) {
    if (ws.stage.kind !== 'phase_running') continue;
    const phase = ws.stage.phase;
    if ((sddState.actionLogByWorkspace[ws.id]?.[phase]?.length ?? 0) > 0) continue;
    try {
      const entries = await invoke<ActionLogEntry[]>('sdd_read_action_log', {
        id: ws.id,
        phase,
        tail: ACTION_LOG_CAP,
      });
      if (entries.length > 0) {
        sddState.actionLogByWorkspace = {
          ...sddState.actionLogByWorkspace,
          [ws.id]: {
            ...(sddState.actionLogByWorkspace[ws.id] ?? {}),
            [phase]: entries,
          },
        };
      }
    } catch (e) {
      console.warn('[sdd] action_log rehydrate failed', ws.id, e);
    }
  }
}
