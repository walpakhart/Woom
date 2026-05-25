// SDD prompt-template plumbing — extracted from sdd.svelte.ts in
// wave-1 phase-8 refactor. Templates live as `include_str!` blobs on
// the Rust side; the frontend fetches + interpolates them when it
// needs to drop the next-stage prompt into the chat composer. No
// reactive store, no UI — just `invoke` + string substitution.

import { invoke } from '@tauri-apps/api/core';
import type { SddWorkspace } from './sdd.svelte';

export type PromptKind =
  | 'spec'
  | 'plan'
  | 'phase'
  | 'summary'
  | 'amend'
  /** Three-call mode passes — see `spec-1` FR-1 / FR-3 / FR-4. */
  | 'phase_plan'
  | 'phase_implement'
  | 'phase_verify';

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
      const next = ws.phases.find((p: SddWorkspace['phases'][number]) => p.status === 'pending');
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
      return null; // already in flight (single-call mode)
    case 'phase_pending_approval':
      return null; // waiting on user — gate cleared via approveSddPhase
    case 'spec_ready':
      return null; // waiting on user approve
    case 'phase_planning': {
      /* Three-call mode — plan pass. Fires when the workspace
       * transitions out of phase_pending_approval AND
       * `phase_execution.mode === "three_call"`. */
      const stage = ws.stage;
      const ph = ws.phases.find((p: SddWorkspace['phases'][number]) => p.number === stage.phase);
      if (!ph) return null;
      const tpl = await fetchPrompt('phase_plan');
      return interpolate(tpl, {
        workspace_root: root,
        workspace_id: ws.id,
        user_prompt: ws.user_prompt,
        phase_number: String(ph.number),
        phase_slug: ph.slug,
        phase_file: `${ph.slug}.md`,
      });
    }
    case 'phase_plan_review':
      return null; // waiting on user — gate cleared via approveSddPhasePlan
    case 'phase_implementing': {
      const stage = ws.stage;
      const ph = ws.phases.find((p: SddWorkspace['phases'][number]) => p.number === stage.phase);
      if (!ph) return null;
      const tpl = await fetchPrompt('phase_implement');
      return interpolate(tpl, {
        workspace_root: root,
        workspace_id: ws.id,
        user_prompt: ws.user_prompt,
        phase_number: String(ph.number),
        phase_slug: ph.slug,
        phase_file: `${ph.slug}.md`,
      });
    }
    case 'phase_verifying': {
      const stage = ws.stage;
      const ph = ws.phases.find((p: SddWorkspace['phases'][number]) => p.number === stage.phase);
      if (!ph) return null;
      const tpl = await fetchPrompt('phase_verify');
      return interpolate(tpl, {
        workspace_root: root,
        workspace_id: ws.id,
        user_prompt: ws.user_prompt,
        phase_number: String(ph.number),
        phase_slug: ph.slug,
        phase_file: `${ph.slug}.md`,
      });
    }
    default:
      return null;
  }
}

/** Convenience for the slash command. Returns interpolated spec prompt
 *  ready to send. */
export async function buildKickoffPrompt(ws: SddWorkspace): Promise<string> {
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
