// Pure stage-derivations for SddCard. Extracted in wave-1 phase-8
// refactor. Inputs in / strings out — no reactive state, no IPC —
// so the card's `$derived` blocks can call these without re-reading
// the workspace tree each cycle.

import type { SddWorkspace } from '$lib/state/sdd.svelte';

type Stage = SddWorkspace['stage'];
type EditTarget =
  | { kind: 'spec' }
  | { kind: 'plan' }
  | { kind: 'phase'; number: number }
  | null;

/** Header label for the workspace's current stage. Covers every
 *  variant of the SddStage union so adding a new stage on the Rust
 *  side surfaces here as a compile-time exhaustiveness miss.
 *
 *  `fixIteration` is the retry_count for the phase the stage refers
 *  to (when applicable). When > 0 the substep labels rewrite to
 *  `Phase N — fix #M planning|implementing|verifying` so the user
 *  can tell at a glance that a running pass is a fix iteration
 *  rather than the initial run. Without this rewrite the header
 *  said exactly the same "Phase N implementing" string on both the
 *  original pass AND every fix attempt — the "· fixing #N" badge
 *  shown by SddCard separately was easy to miss next to the stage
 *  label. */
export function stageLabel(stage: Stage, fixIteration: number = 0): string {
  const fixPrefix = fixIteration > 0 ? `fix #${fixIteration} ` : '';
  switch (stage.kind) {
    case 'drafting': return 'Drafting spec';
    case 'spec_ready': return 'Spec ready';
    case 'planning': return 'Drafting plan';
    case 'plan_ready': return 'Plan ready';
    case 'phase_pending_approval': return `Phase ${stage.phase} — review`;
    case 'phase_running': return `Phase ${stage.phase} — ${fixPrefix}running`;
    case 'phase_planning': return `Phase ${stage.phase} — ${fixPrefix}planning`;
    case 'phase_plan_review': return `Phase ${stage.phase} — plan review`;
    case 'phase_implementing': return `Phase ${stage.phase} — ${fixPrefix}implementing`;
    case 'phase_verifying': return `Phase ${stage.phase} — ${fixPrefix}verifying`;
    case 'phase_done': return `Phase ${stage.phase} done`;
    case 'complete': return 'All phases done';
    case 'paused': return 'Paused';
    case 'stopped': return 'Stopped';
    case 'failed':
      return fixIteration > 0
        ? `Phase ${stage.failed_phase ?? '?'} — fix #${fixIteration} failed`
        : 'Failed';
  }
}

/** Color hint for the header chip. `live` while a turn is in flight,
 *  `warn` on failure / stop / plan-review gate, `ok` on completion,
 *  `dim` for everything resting. */
export function stageTone(
  stage: Stage,
  isInFlight: boolean
): 'live' | 'ok' | 'warn' | 'dim' {
  if (isInFlight) return 'live';
  if (stage.kind === 'phase_plan_review') return 'warn';
  if (stage.kind === 'failed' || stage.kind === 'stopped') return 'warn';
  if (stage.kind === 'complete') return 'ok';
  return 'dim';
}

/** Label for the primary action button. Empty when no advance is
 *  available (turn in flight, terminal stage, etc.). */
export function actionLabel(
  stage: Stage,
  nextPhase: { number: number } | undefined
): string {
  if (stage.kind === 'spec_ready') return 'Approve spec · draft plan';
  if (stage.kind === 'plan_ready') {
    return nextPhase ? `Approve plan · start phase ${nextPhase.number}` : 'Approve plan';
  }
  if (stage.kind === 'phase_done') {
    return nextPhase ? `Continue · phase ${nextPhase.number}` : 'Done';
  }
  if (stage.kind === 'phase_pending_approval') return `Approve · start phase ${stage.phase}`;
  if (stage.kind === 'phase_plan_review') return `Approve plan · run phase ${stage.phase}`;
  return '';
}

/** Resolve the target the user's edit operation writes back to. The
 *  card branches on `spec_ready` / `plan_ready` / `phase_*` and saves
 *  to the right file via `saveSddBody`. Returns null when no edit
 *  surface is reachable from the current stage. */
export function editTarget(stage: Stage): EditTarget {
  if (stage.kind === 'spec_ready') return { kind: 'spec' };
  if (stage.kind === 'plan_ready') return { kind: 'plan' };
  if (stage.kind === 'phase_running' || stage.kind === 'phase_done') {
    return { kind: 'phase', number: stage.phase };
  }
  return null;
}

/** Body chunk to preview — show spec for spec_ready, plan for
 *  plan_ready, current phase's body for phase_running, prior phase
 *  summary for phase_done. Returns null when the stage has no
 *  natural body to render (drafting / waiting / paused / stopped /
 *  failed — those surfaces draw their own state instead). */
export function bodyForStage(
  workspace: SddWorkspace,
  stage: Stage
): { title: string; markdown: string } | null {
  if (stage.kind === 'spec_ready' && workspace.spec_body) {
    return { title: 'spec.md', markdown: workspace.spec_body };
  }
  if (stage.kind === 'plan_ready' && workspace.plan_body) {
    return { title: 'plan.md', markdown: workspace.plan_body };
  }
  if (stage.kind === 'phase_running') {
    const ph = workspace.phases.find((x) => x.number === stage.phase);
    if (ph) return { title: `phases/${ph.slug}.md`, markdown: ph.body };
  }
  if (stage.kind === 'phase_planning' || stage.kind === 'phase_implementing' || stage.kind === 'phase_verifying') {
    const ph = workspace.phases.find((x) => x.number === stage.phase);
    // During verify/implement we already have the plan.md — show it
    // so the user can scan the agent's intended approach while the
    // pass is running. During planning, plan.md may not exist yet.
    if (ph?.plan_body) return { title: `phases/${ph.slug}/plan.md`, markdown: ph.plan_body };
    if (ph) return { title: `phases/${ph.slug}.md`, markdown: ph.body };
  }
  if (stage.kind === 'phase_plan_review') {
    const ph = workspace.phases.find((x) => x.number === stage.phase);
    if (ph?.plan_body) return { title: `phases/${ph.slug}/plan.md`, markdown: ph.plan_body };
    if (ph) return { title: `phases/${ph.slug}.md`, markdown: ph.body };
  }
  if (stage.kind === 'phase_done') {
    const ph = workspace.phases.find((x) => x.number === stage.phase);
    if (ph?.summary) return { title: `results/${ph.slug}-result.md`, markdown: ph.summary };
    if (ph) return { title: `phases/${ph.slug}.md`, markdown: ph.body };
  }
  if (stage.kind === 'complete') {
    /* Prefer the agent's wrap-up (SUMMARY.md) when present — it's
     *  the curated digest. Fall back to concatenated phase
     *  summaries while the summary is still being written. */
    if (workspace.summary_body) {
      return { title: 'SUMMARY.md', markdown: workspace.summary_body };
    }
    const all = workspace.phases
      .map((ph) => `### Phase ${ph.number}: ${ph.title}\n\n${ph.summary ?? '_no summary written_'}\n`)
      .join('\n');
    return { title: 'all phases', markdown: all || '_no phase summaries — waiting for wrap-up…_' };
  }
  return null;
}

/** Compose a phase's body — plan section first, result/summary
 *  appended below when the phase has completed. Renders as a real
 *  document, so the lightbox view of a single phase shows the
 *  agent's intent + what shipped side-by-side. */
export function phaseBody(
  workspace: SddWorkspace,
  num: number
): { title: string; markdown: string } | null {
  const ph = workspace.phases.find((x) => x.number === num);
  if (!ph) return null;
  const parts: string[] = [];
  parts.push(`# Phase ${ph.number}: ${ph.title}`);
  parts.push(`_Status: **${ph.status}**_`);
  parts.push('');
  parts.push('## Plan');
  parts.push(ph.body?.trim() || '_no plan body yet_');
  if (ph.summary && ph.summary.trim()) {
    parts.push('');
    parts.push('## Result');
    parts.push(ph.summary.trim());
  }
  return { title: `phases/${ph.slug}.md`, markdown: parts.join('\n') };
}
