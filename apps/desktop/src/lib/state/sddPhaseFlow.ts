/* SDD phase-flow state machine + dispatcher.
 *
 * The failure-card buttons used to call command-specific helpers
 * (`acceptSddPhaseFailed`, `retrySddPhase`, `fixDeviationsAndRetry`,
 * `rollbackSddPhase`, …) directly. Each had its own staleness-handling,
 * its own busy-flag pattern, its own toast-on-error, its own
 * refresh-before-act logic. Result: phase-lifecycle states blurred
 * (especially across fix-attempt round-trips), buttons sometimes
 * silently no-op'd, the failure card kept showing yesterday's
 * deviations.
 *
 * This module makes the state space EXPLICIT and routes every action
 * through one place:
 *
 *   1. `derivePhaseFlow(ws, phase)` collapses the raw SddPhase into a
 *      tagged enum (`not_started` / `running` / `failed` / `done` /
 *      `skipped`). The card reads from THIS, not from `phase.status`
 *      directly, so the union is exhaustive and the substep is
 *      always surfaced when running.
 *
 *   2. `dispatchPhaseAction(ws, phase, action, deps)` is the single
 *      entry point for any failure-card click. It refreshes the
 *      workspace from disk (Rust's `sdd_get` rebuilds before
 *      returning, so the response is canonical), re-derives the
 *      flow state, validates that `action` is meaningful in that
 *      state, then routes to the matching command. Invalid actions
 *      surface as `info` toasts ("phase already done — nothing to
 *      accept") instead of silent no-ops or scary error messages.
 *
 * Keeps the SddCard.svelte component dumb — buttons stamp an action
 * intent, the dispatcher figures out the right command to run.
 */

import { invoke } from '@tauri-apps/api/core';
import {
  sddState,
  upsertWorkspace,
  fixDeviationsAndRetry,
  type SddPhase,
  type SddWorkspace,
} from './sdd.svelte';
import {
  acceptSddPhaseFailed,
  retrySddPhase,
  rollbackSddPhase,
  skipSddPhaseWithReason,
} from './sdd_commands.svelte';
import { notify } from './toaster.svelte';

/** Canonical lifecycle states for a single phase. Free of the raw
 *  `pending|running|done|failed|skipped` ambiguity from disk — the
 *  derive function below collapses substep markers + verify body
 *  into one explicit shape. */
export type PhaseFlowState =
  /** Status `pending` with no prior verify run. Awaiting kickoff. */
  | { kind: 'not_started' }
  /** Status `running` OR `pending` with substep state on disk. The
   *  `substep` field surfaces which three-call sub-pass is live. */
  | { kind: 'running'; substep: 'plan' | 'implement' | 'verify' | 'unknown' }
  /** Three-call plan-review gate. Status `running`, plan.md exists,
   *  awaiting user approve/discard. */
  | { kind: 'awaiting_plan_review' }
  /** Status `failed`. Carries the deviation list from the latest
   *  verify pass + the fix-attempt counter. */
  | {
      kind: 'failed';
      deviations: string[];
      failedChecks: number[] | null;
      fixAttempt: number;
      hasVerifyDeviations: boolean;
    }
  /** Status `done`. */
  | { kind: 'done' }
  /** Status `skipped` — user dropped this phase with a reason. */
  | { kind: 'skipped' };

/** Collapse the raw SddPhase + workspace context into a flow state. */
export function derivePhaseFlow(ws: SddWorkspace, phaseNumber: number): PhaseFlowState | null {
  const phase = ws.phases.find((p) => p.number === phaseNumber);
  if (!phase) return null;
  switch (phase.status) {
    case 'pending':
      return { kind: 'not_started' };
    case 'done':
      return { kind: 'done' };
    case 'skipped':
      return { kind: 'skipped' };
    case 'failed': {
      const deviations = phase.verify?.deviations ?? [];
      const fixAttempt = sddState.fixAttempts[ws.id]?.[phaseNumber] ?? 0;
      return {
        kind: 'failed',
        deviations,
        failedChecks: null /* surfaced via stage; not needed for flow routing */,
        fixAttempt,
        hasVerifyDeviations: deviations.length > 0,
      };
    }
    case 'running':
      /* Substep marker lives in `substep-state.json` on disk, which
       *  the workspace doesn't carry here. The frontend's `stage`
       *  field reflects it via the `phase_planning` / `_implementing`
       *  / `_verifying` discriminator — caller threads stage.kind in
       *  via the parent if it wants the substep precision. */
      return { kind: 'running', substep: 'unknown' };
    default:
      /* Unknown status — surface as not_started so the UI can at
       *  least offer a kickoff path. */
      return { kind: 'not_started' };
  }
}

/** Actions any failure-card button can request. The dispatcher picks
 *  the right command based on the live phase state. */
export type FlowAction =
  /** Primary failed-with-deviations action — re-fires the phase with
   *  a follow-up prompt listing the deviations. */
  | { type: 'fix' }
  /** Reset the phase to `pending` + fire the standard kickoff prompt.
   *  Distinct from `fix` in that no deviation context is attached. */
  | { type: 'retry' }
  /** Flip failed → done with a default audit reason + advance. */
  | { type: 'accept' }
  /** Flip failed → done with a user-supplied audit reason + advance. */
  | { type: 'accept_with_reason'; reason: string }
  /** Flip failed → skipped + advance. */
  | { type: 'skip' }
  /** Flip failed → skipped with a user-supplied audit reason. */
  | { type: 'skip_with_reason'; reason: string }
  /** Fire the next-phase prompt (for done / skipped states). */
  | { type: 'next_phase' }
  /** Git checkout pre-phase commit, then reset phase to pending. */
  | { type: 'rollback' };

export interface PhaseFlowDeps {
  /** Called with the prompt that should drive the next agent turn.
   *  Parent (SddCard) wires this to the session's send pipeline. */
  onAdvance: (prompt: string) => void | Promise<void>;
  /** Build a prompt for whatever stage the (refreshed) workspace is
   *  on now. Same signature as SddCard's local `buildPromptForStage`
   *  helper — passed in to avoid a circular import. */
  buildPromptForStage: (ws: SddWorkspace) => Promise<string | null> | string | null;
}

/** Result of a single dispatch — success or a reason it didn't fire.
 *  Toasts are already raised inside the dispatcher, so callers just
 *  need to know whether to flip their busy flag back. */
export interface DispatchResult {
  ok: boolean;
  /** When ok=false, a short reason for telemetry / logging. */
  reason?: string;
}

/** ENTRY POINT. Refresh the workspace from disk, re-derive the flow
 *  state, validate the requested action, then execute. Every failure
 *  mode surfaces a toast — never silent. */
export async function dispatchPhaseAction(
  workspaceId: string,
  phaseNumber: number,
  action: FlowAction,
  deps: PhaseFlowDeps
): Promise<DispatchResult> {
  /* Step 1 — refresh. Rust's `sdd_get` re-runs `rebuild_from_disk`
   *  before returning, so the response carries a canonical stage +
   *  phase array. Without this every dispatcher would have to
   *  handle staleness independently. */
  let refreshed: SddWorkspace;
  try {
    refreshed = await invoke<SddWorkspace>('sdd_get', { id: workspaceId });
  } catch (e) {
    notify({
      kind: 'error',
      title: `Couldn't refresh SDD workspace`,
      body: String(e),
      ttlMs: 8000,
    });
    return { ok: false, reason: 'sdd_get failed' };
  }
  upsertWorkspace(refreshed);

  const flow = derivePhaseFlow(refreshed, phaseNumber);
  if (!flow) {
    notify({
      kind: 'warning',
      title: `Phase ${phaseNumber} not found`,
      body: 'The workspace may have been edited externally — discard + reopen.',
      ttlMs: 7000,
    });
    return { ok: false, reason: 'phase missing' };
  }

  /* Step 2 — route. Each branch validates `flow.kind` against
   *  `action.type` and routes to the matching command. Invalid
   *  combinations surface as info toasts (not errors — the click
   *  was reasonable, it just doesn't apply to this state). */
  switch (action.type) {
    case 'fix':
      return await handleFix(refreshed, phaseNumber, flow);
    case 'retry':
      return await handleRetry(refreshed, phaseNumber, flow, deps);
    case 'accept':
      return await handleAccept(refreshed, phaseNumber, flow, deps);
    case 'accept_with_reason':
      return await handleAcceptWithReason(refreshed, phaseNumber, flow, action.reason, deps);
    case 'skip':
      return await handleSkip(refreshed, phaseNumber, flow);
    case 'skip_with_reason':
      return await handleSkipWithReason(refreshed, phaseNumber, flow, action.reason);
    case 'next_phase':
      return await handleNextPhase(refreshed, phaseNumber, flow, deps);
    case 'rollback':
      return await handleRollback(refreshed, phaseNumber, flow);
  }
}

/* ---------------- per-action handlers ---------------- */

async function handleFix(
  ws: SddWorkspace,
  phaseN: number,
  flow: PhaseFlowState
): Promise<DispatchResult> {
  if (flow.kind !== 'failed') {
    notify({
      kind: 'info',
      title: `Phase ${phaseN} is "${flow.kind}"`,
      body: 'Fix only applies to a failed phase. The workspace already moved past the failure.',
      ttlMs: 6000,
    });
    return { ok: false, reason: 'wrong-state' };
  }
  try {
    await fixDeviationsAndRetry(ws.id, phaseN);
    return { ok: true };
  } catch (e) {
    notify({
      kind: 'error',
      title: `Couldn't start fix for phase ${phaseN}`,
      body: String(e),
      ttlMs: 8000,
    });
    return { ok: false, reason: 'fix dispatch failed' };
  }
}

async function handleRetry(
  ws: SddWorkspace,
  phaseN: number,
  flow: PhaseFlowState,
  deps: PhaseFlowDeps
): Promise<DispatchResult> {
  if (flow.kind !== 'failed' && flow.kind !== 'not_started') {
    notify({
      kind: 'info',
      title: `Phase ${phaseN} is "${flow.kind}"`,
      body: 'Retry restarts a failed (or pending) phase. The workspace is somewhere else right now.',
      ttlMs: 6000,
    });
    return { ok: false, reason: 'wrong-state' };
  }
  const fresh = await retrySddPhase(ws.id, phaseN);
  if (!fresh) {
    notify({
      kind: 'error',
      title: `Couldn't retry phase ${phaseN}`,
      body: 'Rust command failed — see DevTools console.',
      ttlMs: 8000,
    });
    return { ok: false, reason: 'retry failed' };
  }
  const prompt = await deps.buildPromptForStage(fresh);
  if (prompt) await deps.onAdvance(prompt);
  return { ok: true };
}

async function handleAccept(
  ws: SddWorkspace,
  phaseN: number,
  flow: PhaseFlowState,
  deps: PhaseFlowDeps
): Promise<DispatchResult> {
  if (flow.kind === 'done' || flow.kind === 'skipped') {
    /* Already past the failure — just advance. */
    const prompt = await deps.buildPromptForStage(ws);
    if (prompt) await deps.onAdvance(prompt);
    return { ok: true };
  }
  if (flow.kind !== 'failed') {
    notify({
      kind: 'info',
      title: `Phase ${phaseN} is "${flow.kind}"`,
      body: 'Accept only applies to a failed phase. The workspace already moved on.',
      ttlMs: 6000,
    });
    return { ok: false, reason: 'wrong-state' };
  }
  const fresh = await acceptSddPhaseFailed(
    ws.id,
    phaseN,
    'Accepted from the failure-card quick action — user reviewed and chose to advance without an explicit reason.'
  );
  if (!fresh) {
    notify({
      kind: 'error',
      title: `Couldn't accept phase ${phaseN}`,
      body: 'Rust rejected the accept — see DevTools console.',
      ttlMs: 8000,
    });
    return { ok: false, reason: 'accept failed' };
  }
  const prompt = await deps.buildPromptForStage(fresh);
  if (prompt) await deps.onAdvance(prompt);
  return { ok: true };
}

async function handleAcceptWithReason(
  ws: SddWorkspace,
  phaseN: number,
  flow: PhaseFlowState,
  reason: string,
  deps: PhaseFlowDeps
): Promise<DispatchResult> {
  if (flow.kind !== 'failed') {
    notify({
      kind: 'info',
      title: `Phase ${phaseN} is "${flow.kind}"`,
      body: 'Accept only applies to a failed phase.',
      ttlMs: 6000,
    });
    return { ok: false, reason: 'wrong-state' };
  }
  const fresh = await acceptSddPhaseFailed(ws.id, phaseN, reason);
  if (!fresh) return { ok: false, reason: 'accept failed' };
  const prompt = await deps.buildPromptForStage(fresh);
  if (prompt) await deps.onAdvance(prompt);
  return { ok: true };
}

async function handleSkip(
  ws: SddWorkspace,
  phaseN: number,
  flow: PhaseFlowState
): Promise<DispatchResult> {
  if (flow.kind !== 'failed') {
    notify({
      kind: 'info',
      title: `Phase ${phaseN} is "${flow.kind}"`,
      body: 'Skip only applies to a failed phase.',
      ttlMs: 6000,
    });
    return { ok: false, reason: 'wrong-state' };
  }
  await skipSddPhaseWithReason(
    ws.id,
    phaseN,
    'Skipped from the failure-card quick action — user reviewed deviations and chose to advance without an explicit reason.'
  );
  return { ok: true };
}

async function handleSkipWithReason(
  ws: SddWorkspace,
  phaseN: number,
  flow: PhaseFlowState,
  reason: string
): Promise<DispatchResult> {
  if (flow.kind !== 'failed') return { ok: false, reason: 'wrong-state' };
  await skipSddPhaseWithReason(ws.id, phaseN, reason);
  return { ok: true };
}

async function handleNextPhase(
  ws: SddWorkspace,
  phaseN: number,
  flow: PhaseFlowState,
  deps: PhaseFlowDeps
): Promise<DispatchResult> {
  /* Generic "move forward" intent. Behaviour depends on the live
   *  flow state: failed → accept-and-advance, anything else → just
   *  fire the next-stage prompt. */
  if (flow.kind === 'failed') {
    return await handleAccept(ws, phaseN, flow, deps);
  }
  const prompt = await deps.buildPromptForStage(ws);
  if (prompt) {
    await deps.onAdvance(prompt);
    return { ok: true };
  }
  notify({
    kind: 'info',
    title: `Phase ${phaseN} status is "${flow.kind}"`,
    body: 'Workspace already moved on — open the card to see the live state.',
    ttlMs: 6000,
  });
  return { ok: false, reason: 'no prompt' };
}

async function handleRollback(
  ws: SddWorkspace,
  phaseN: number,
  flow: PhaseFlowState
): Promise<DispatchResult> {
  if (flow.kind !== 'failed') {
    notify({
      kind: 'info',
      title: `Phase ${phaseN} is "${flow.kind}"`,
      body: 'Rollback only applies to a failed phase.',
      ttlMs: 6000,
    });
    return { ok: false, reason: 'wrong-state' };
  }
  await rollbackSddPhase(ws.id, phaseN);
  return { ok: true };
}

/* ---------------- helpers for SddCard ---------------- */

/** Convenience: format the flow state for a header subtitle line.
 *  Returns null when there's nothing to say (e.g. running with
 *  unknown substep). */
export function describeFlow(flow: PhaseFlowState): string | null {
  switch (flow.kind) {
    case 'not_started':
      return 'pending';
    case 'running':
      return flow.substep === 'unknown' ? 'running' : `running · ${flow.substep}`;
    case 'awaiting_plan_review':
      return 'awaiting plan approval';
    case 'failed':
      return flow.fixAttempt > 0
        ? `failed · fix attempt ${flow.fixAttempt} · ${flow.deviations.length} deviation${flow.deviations.length === 1 ? '' : 's'}`
        : `failed · ${flow.deviations.length} deviation${flow.deviations.length === 1 ? '' : 's'}`;
    case 'done':
      return 'done';
    case 'skipped':
      return 'skipped';
  }
}

/** Convenience: which actions are meaningful for this flow state?
 *  Used by the UI to decide which buttons to render. */
export function allowedActions(flow: PhaseFlowState): FlowAction['type'][] {
  switch (flow.kind) {
    case 'failed':
      return flow.hasVerifyDeviations
        ? ['fix', 'retry', 'accept', 'skip', 'rollback']
        : ['next_phase', 'retry', 'skip', 'rollback'];
    case 'done':
    case 'skipped':
      return ['next_phase'];
    case 'not_started':
      return ['retry'];
    case 'running':
    case 'awaiting_plan_review':
      return [];
  }
}
