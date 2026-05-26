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
import {
  attachActionLogListener,
  rehydrateActionLogs,
} from './sdd_action_log_store.svelte';

// --- Mirrored types from `sdd.rs` ------------------------------------

export type SddStage =
  | { kind: 'drafting' }
  | { kind: 'spec_ready' }
  | { kind: 'planning' }
  | { kind: 'plan_ready' }
  | { kind: 'phase_pending_approval'; phase: number }
  | { kind: 'phase_running'; phase: number }
  /** Three-call mode — agent is producing the plan-pass output
   *  (read-only analysis written to `phases/<slug>/plan.md`).
   *  See `spec-1` FR-1. */
  | { kind: 'phase_planning'; phase: number }
  /** Three-call mode — plan.md exists, plan-gate is enabled, user
   *  must Approve / Amend / Discard before implement fires.
   *  See `spec-1` FR-7. */
  | { kind: 'phase_plan_review'; phase: number }
  /** Three-call mode — agent is executing the plan (edits land
   *  here). See `spec-1` FR-3. */
  | { kind: 'phase_implementing'; phase: number }
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
  | 'user_stopped'
  /** Three-call plan-pass mutated files despite the read-only contract.
   *  Detected via pre/post `git status --porcelain` sentinel. */
  | 'plan_mutated_disk'
  /** Three-call verify-pass produced non-empty `deviations`. */
  | 'verify_failed'
  /** Three-call verify-pass JSON failed to parse AND no hard gate
   *  rescued the phase. Rare — `VerifyOutput::parse_or_fallback`
   *  masks most parse errors. */
  | 'verify_parse_fail'
  /** User clicked Discard during the plan-review gate. */
  | 'plan_discarded';

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
  /** Three-call mode artifact — `phases/<slug>/plan.md`. Null when
   *  missing (single-call mode or three-call pre-plan-pass). See
   *  `spec-1` FR-2. */
  plan_body?: string | null;
  /** Three-call mode artifact — `phases/<slug>/verify.json` parsed.
   *  Null when missing. See `spec-1` FR-4. */
  verify?: import('./sdd_commands.svelte').VerifyOutput | null;
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
  /** Three-call execution mode config mirrored from
   *  `meta.json#phase_execution`. SddCard + buildPromptForStage read
   *  this to decide which sub-step pipeline to follow. See
   *  `spec-1` FR-11. */
  phase_execution: PhaseExecutionConfig;
}

/** Which sub-step a three-call phase is currently running. Lives on
 *  `<workspace>/control/phase-<N>-substep-state.json` and tags action
 *  log rows so the SddCard can group events per pass. */
export type SddPhaseSubstep = 'plan' | 'implement' | 'verify';

export type PhaseExecutionMode = 'single_call' | 'three_call';

/** Mirror of `crate::sdd::PhaseExecutionConfig`. Every field has a
 *  default on the Rust side so legacy meta.json (lacking the block)
 *  deserializes; this TS type matches the populated shape after
 *  hydrate. See `spec-1` FR-11 / FR-12. */
export interface PhaseExecutionConfig {
  mode: PhaseExecutionMode;
  plan_gate: boolean;
  plan_budget_pct: number;
  implement_budget_pct: number;
  verify_budget_pct: number;
}

/** Default values matching `PhaseExecutionConfig::default()` on the
 *  Rust side. Exposed so the SddCard / Settings can short-circuit
 *  when a workspace's config is absent (e.g. during the first paint
 *  before hydrate lands). */
export const DEFAULT_PHASE_EXECUTION_CONFIG: PhaseExecutionConfig = {
  mode: 'single_call',
  plan_gate: false,
  plan_budget_pct: 0.25,
  implement_budget_pct: 0.70,
  verify_budget_pct: 0.05,
};

/** Tagged-union mirror of Rust's `SddRecoveryState`. Currently only one
 *  variant — `OrphanPhase` — but we keep the discriminator so adding
 *  e.g. `OrphanPlan` later is non-breaking. */
export type SddRecoveryState = {
  kind: "orphan_phase";
  phase: number;
  /** Sha to roll back to. Null when git was disabled at approve-time. */
  pre_phase_sha: string | null;
  /** Three-call mode — sub-step that was in flight when Woom died.
   *  Surfaces in the recovery banner copy. See `spec-1` NFR-rel-1. */
  sub_step?: SddPhaseSubstep | null;
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
  /** Three-call mode — which sub-step was active when this row was
   *  written. Used by SddCard to render `— plan —` / `— implement —`
   *  / `— verify —` divider rows in the live feed. Absent on
   *  single-call workspaces and legacy JSONL. See `spec-1` FR-9. */
  sub_step?: SddPhaseSubstep | null;
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
 *  rendering. `failed` workspaces are ALWAYS surfaced even when
 *  previously hidden — failure carries user-actionable affordances
 *  (Retry / Accept / Skip / Rollback) and a hidden failure card is a
 *  footgun (workspace silently stuck with no entry point). */
export function isSddCardHidden(workspaceId: string): boolean {
  if (!sddState.hiddenWorkspaceIds[workspaceId]) return false;
  const ws = sddState.workspaces.find((w) => w.id === workspaceId);
  if (ws && ws.stage.kind === 'failed') return false;
  return true;
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

/** Auto-fire dispatcher registry. `+page.svelte` registers an
 *  implementation on mount that knows how to push a silent prompt
 *  through `sendClaudeMessage({silent: true})`. The store calls
 *  this whenever a workspace transitions into a stage that needs
 *  the next agent turn fired without a user gate
 *  (`phase_implementing` after `sdd_save_phase_plan`,
 *  `phase_verifying` after `sdd_complete_phase_implement`,
 *  `complete` after the last phase). Decouples store from chat
 *  send pipeline. */
type AutoFireDispatcher = (sessionId: string, prompt: string) => Promise<void> | void;
let autoFireDispatcher: AutoFireDispatcher | null = null;
export function setSddAutoFireDispatcher(fn: AutoFireDispatcher | null): void {
  autoFireDispatcher = fn;
}

/** Per-session re-fire guard. Tracks the (stage_kind, phase) tuple
 *  most recently dispatched for a session so we don't re-fire the
 *  same prompt twice on repeated `sdd:changed` events. Cleared
 *  whenever the stage changes naturally. */
const lastAutoFireKey: Record<string, string> = {};

async function maybeAutoFire(ws: SddWorkspace): Promise<void> {
  if (!autoFireDispatcher) return;
  if (!ws.session_id) return;
  const stage = ws.stage;
  let key = '';
  let shouldFire = false;
  if (stage.kind === 'phase_implementing') {
    const ph = ws.phases.find((p) => p.number === stage.phase);
    shouldFire = !!ph?.plan_body;
    key = `phase_implementing:${stage.phase}`;
  } else if (stage.kind === 'phase_verifying') {
    /* Fire verify-pass unconditionally on substep transition.
     * Earlier this gated on `ph.summary` truthy — but
     * `sdd_complete_phase_implement` only writes summary when the
     * agent's call carried a non-empty `summary` arg. Empty summary
     * (or an agent that dropped the field) left the verify pass
     * stuck behind a manual "Continue verify-pass" click. The verify
     * prompt is built from the phase doc anyway; the summary field
     * is decorative. */
    shouldFire = true;
    key = `phase_verifying:${stage.phase}`;
  } else if (stage.kind === 'complete') {
    shouldFire = !ws.summary_body;
    key = 'complete';
  }
  if (!shouldFire) return;
  if (lastAutoFireKey[ws.session_id] === key) return;
  lastAutoFireKey[ws.session_id] = key;
  const prompt = await buildPromptForStage(ws);
  if (!prompt) return;
  try {
    await autoFireDispatcher(ws.session_id, prompt);
  } catch (e) {
    console.warn('sdd autoFire dispatcher failed', e);
    delete lastAutoFireKey[ws.session_id];
  }
}

/** User-initiated re-fire for the current substep prompt. UI calls
 *  this when an agent-turn ended without the auto-fire dispatcher
 *  picking up — e.g. production bundle predates the auto-fire wiring,
 *  or the agent silently dropped the turn. Bypasses the
 *  `lastAutoFireKey` dedupe (the whole point is to retry) and supports
 *  `phase_planning` in addition to the auto-fire stages, since the
 *  initial plan-pass fire happens in `+page.svelte` and can fail there
 *  too. Caller is expected to gate by `!sessionSending` so we don't
 *  step on an in-flight turn. */
export async function manualContinueSdd(id: string): Promise<void> {
  if (!autoFireDispatcher) {
    console.warn('sdd manualContinue: auto-fire dispatcher not registered');
    return;
  }
  const ws = sddState.workspaces.find((w) => w.id === id);
  if (!ws || !ws.session_id) return;
  const stage = ws.stage;
  if (
    stage.kind !== 'phase_planning' &&
    stage.kind !== 'phase_implementing' &&
    stage.kind !== 'phase_verifying'
  ) {
    return;
  }
  const prompt = await buildPromptForStage(ws);
  if (!prompt) return;
  lastAutoFireKey[ws.session_id] = `${stage.kind}:${stage.phase}`;
  try {
    await autoFireDispatcher(ws.session_id, prompt);
  } catch (e) {
    console.warn('sdd manualContinue dispatch failed', e);
    delete lastAutoFireKey[ws.session_id];
  }
}

/** Fix-deviations-and-retry: combines `sdd_retry_phase` (resets the
 *  failed phase back to `pending`) with a custom follow-up prompt that
 *  lists the verify deviations + asks the agent to address each one
 *  before re-running implement/verify. Used by the "Fix deviations"
 *  button on the failure card when the user wants the workflow to
 *  self-heal rather than skip/accept the deviations as-is.
 *
 *  Dispatches via the same auto-fire channel so the agent picks the
 *  next turn up automatically — no manual Send required. */
export async function fixDeviationsAndRetry(id: string, phaseNumber: number): Promise<void> {
  const ws = sddState.workspaces.find((w) => w.id === id);
  if (!ws || !ws.session_id) return;
  const phase = ws.phases.find((p) => p.number === phaseNumber);
  if (!phase) return;
  const deviations = phase.verify?.deviations ?? [];
  const summary = phase.verify?.summary ?? '';
  // Reset phase to pending so the standard flow re-fires it.
  const { retrySddPhase } = await import('./sdd_commands.svelte');
  await retrySddPhase(id, phaseNumber);
  if (!autoFireDispatcher) {
    console.warn('sdd fixDeviationsAndRetry: auto-fire dispatcher not registered');
    return;
  }
  const deviationList = deviations.length > 0
    ? deviations.map((d, i) => `${i + 1}. ${d}`).join('\n')
    : '(no parseable deviation list on the verify.json — re-read the file before starting)';
  const prompt = [
    `# Phase ${phaseNumber} — fix deviations`,
    '',
    'The previous verify pass flagged the following deviations from the phase plan:',
    '',
    deviationList,
    '',
    summary ? `Verify summary:\n${summary}\n` : '',
    'Address each deviation. Re-run the phase: plan adjustments → implement the fixes → verify again.',
    'Call `sdd_save_phase_plan` → `sdd_complete_phase_implement` → `sdd_save_phase_verify` as usual; the workflow will advance automatically when verify passes.',
  ].filter(Boolean).join('\n');
  lastAutoFireKey[ws.session_id] = `fix_deviations:${phaseNumber}`;
  try {
    await autoFireDispatcher(ws.session_id, prompt);
  } catch (e) {
    console.warn('sdd fixDeviationsAndRetry dispatch failed', e);
    delete lastAutoFireKey[ws.session_id];
  }
}

/** Quick-skip a failed phase with a default reason. Bypasses the
 *  inline textarea — useful when the user has already reviewed the
 *  deviations in the verify pane and just wants to move on. The
 *  reason stamped here ("user clicked Skip & continue without
 *  comment") still satisfies the audit-trail minimum so the action
 *  is reversible later. */
export async function quickSkipFailedPhase(id: string, phaseNumber: number): Promise<void> {
  const { skipSddPhaseWithReason } = await import('./sdd_commands.svelte');
  await skipSddPhaseWithReason(
    id,
    phaseNumber,
    'Skipped from the failure-card quick action — user reviewed deviations and chose to advance without an explicit reason.',
  );
}

/** Initialise the global `sdd:changed` listener AND hydrate from disk.
 *  Idempotent — safe to call from `+page.svelte` onMount more than
 *  once. Hydration rebuilds any workspaces that existed before app
 *  restart, so the user's previous SDD session resumes seamlessly. */
export async function initSdd(): Promise<void> {
  console.log('[sdd] initSdd called, globalUnlisten:', !!sddState.globalUnlisten);
  if (sddState.globalUnlisten) return;
  sddState.globalUnlisten = await listen<string>('sdd:changed', async (evt) => {
    /* Broadcast carries the workspace id. Re-pull just that one — much
     *  cheaper than `sdd_list` on every event. */
    const id = evt.payload;
    try {
      const ws = await invoke<SddWorkspace>('sdd_get', { id });
      upsertWorkspace(ws);
      /* After every state change check if the new stage needs the
       *  agent fired (three-call substep transitions / workflow
       *  complete). Idempotent via `lastAutoFireKey`. */
      void maybeAutoFire(ws);
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
    console.log('[sdd] hydrated', ws.length, 'workspaces:', ws.map((w) => w.id));
    for (const w of ws) upsertWorkspace(w);
    /* Hydrate-time auto-fire — if a workspace was left mid-substep
     *  (e.g. agent ended the plan-pass turn but the implement-pass
     *  never fired because the JS bundle predated the auto-fire fix),
     *  catching up here means the user's next app launch unsticks
     *  the workflow without manual intervention. The dispatcher
     *  registers AFTER initSdd in `+page.svelte` onMount, so we
     *  defer to the next microtask to give it a chance to land. */
    queueMicrotask(() => {
      for (const w of ws) void maybeAutoFire(w);
    });
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

export function upsertWorkspace(ws: SddWorkspace): void {
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

export function removeWorkspace(id: string): void {
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

// Tauri command wrappers moved to ./sdd_commands.svelte.ts (wave-11
// split). Re-exported below so external callers keep their imports
// unchanged.
export {
  acceptSddPhaseFailed,
  appendAuditLog,
  approveSdd,
  approveSddPhase,
  getSddFileDiff,
  getSddPhaseDiff,
  insertSddPhase,
  loadAuditLog,
  refreshSdd,
  retrySddPhase,
  saveSddBody,
  skipSddPhase,
  skipSddPhaseWithReason,
} from './sdd_commands.svelte';

/** v2 only — reorder phases. `new_order` is a list of current phase
 *  numbers in the order they should end up (1-indexed final positions).
 *  Rust renumbers and renames the markdown files via a two-pass
 *  staging strategy. */
// Wave-11/12 — the remaining 18 Tauri command wrappers live in
// ./sdd_commands.svelte.ts. Re-exported below so external callers
// keep their imports.
export {
  approveSddPhasePlan,
  attachSddToSession,
  completeSddPhaseImplement,
  deleteSddPhase,
  discardSdd,
  discardSddPhasePlan,
  getSddGitState,
  markSddManualCheck,
  pauseSdd,
  recoverSddWorkspace,
  reorderSddPhases,
  resumeSdd,
  rollbackSddPhase,
  runSddVerification,
  saveSddPhasePlan,
  saveSddPhaseVerify,
  setSddPhaseExecutionConfig,
  stopSdd,
  upgradeSddWorkspace,
  type VerifyOutput,
} from './sdd_commands.svelte';

// --- Prompt assembly -------------------------------------------------
// Templates + interpolation moved to ./sdd_prompts.ts (wave-1 phase-8
// split). Re-exported below so existing callers (SddCard etc.) keep
// importing from `$lib/state/sdd.svelte`.
import {
  buildAmendPrompt as _buildAmendPrompt,
  buildKickoffPrompt as _buildKickoffPrompt,
  buildPromptForStage as _buildPromptForStage,
} from './sdd_prompts';
export const buildPromptForStage = _buildPromptForStage;
export const buildKickoffPrompt = _buildKickoffPrompt;
export const buildAmendPrompt = _buildAmendPrompt;

// --- Live action log -------------------------------------------------------
// Per-(workspace, phase) feed of tool_use / tool_result events the agent
// emits. Hot path: tool event → push into in-memory buffer → debounced
// flush to disk. Cold path: app boot → read JSONL for any phase still
// `running` → seed the buffer so the SddCard live feed isn't empty after
// restart.

// Action-log live buffer + agent-stream listener + rehydrate path
// moved to ./sdd_action_log_store.svelte.ts (wave-10 split).
// Re-exported here so existing call sites (SddCard, initSdd) compile
// unchanged.
export {
  actionLogFor,
  appendSddActionLog,
  attachActionLogListener,
  getSddPhaseForSession,
  rehydrateActionLogs,
} from './sdd_action_log_store.svelte';
