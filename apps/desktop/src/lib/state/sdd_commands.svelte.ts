// Tauri-command wrappers for SDD workspace operations. Extracted
// from sdd.svelte.ts in wave-11 split — every function here is a
// thin `invoke('sdd_xxx', args) → upsertWorkspace(ws)` shim with
// try/catch + console.warn on failure. Centralising them in their
// own module shrinks the host file dramatically.
//
// Re-exported from sdd.svelte.ts so existing call sites
// (SddCard, +page.svelte, etc.) keep their import paths.

import { invoke } from '@tauri-apps/api/core';

import {
  removeWorkspace,
  upsertWorkspace,
  type AuditEntry,
  type PhaseExecutionConfig,
  type SddGitState,
  type SddPhaseDiff,
  type SddWorkspace,
} from './sdd.svelte';

/** Mirror of `crate::sdd::VerifyOutput` — structured verify-pass
 *  output written to `<workspace>/phases/<slug>/verify.json`. */
export interface VerifyOutput {
  summary: string;
  files_changed: string[];
  task_compliance: string[];
  deviations: string[];
  notes: string;
}

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
 *  card's "Skip phase" button. Backend rejects reasons under 5 chars. */
export async function skipSddPhaseWithReason(
  id: string,
  phase: number,
  reason: string
): Promise<SddWorkspace | null> {
  try {
    const ws = await invoke<SddWorkspace>('sdd_skip_phase_with_reason', { id, phase, reason });
    upsertWorkspace(ws);
    return ws;
  } catch (e) {
    console.warn('sdd_skip_phase_with_reason failed', e);
    return null;
  }
}

/** Accept a failed phase as-is — flips status from `failed` to `done`
 *  with the user-supplied rationale persisted to phase frontmatter
 *  + meta.json. Backend rejects reasons under 5 chars. */
export async function acceptSddPhaseFailed(
  id: string,
  phase: number,
  reason: string
): Promise<SddWorkspace | null> {
  try {
    const ws = await invoke<SddWorkspace>('sdd_accept_phase_failed', { id, phase, reason });
    upsertWorkspace(ws);
    return ws;
  } catch (e) {
    console.warn('sdd_accept_phase_failed failed', e);
    return null;
  }
}

/** Phase-level diff (per-file stats) between pre/post phase SHAs.
 *  Returns `{ skipped: true }` for non-git workspaces, dirty-tree
 *  approves, or phases that haven't completed yet. */
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

/** Read the workspace audit log (every mutation across agent / user /
 *  system, oldest-first). Returns [] on missing file or unknown
 *  workspace — UI hides the indicator in that case. */
export async function loadAuditLog(id: string): Promise<AuditEntry[]> {
  try {
    return await invoke<AuditEntry[]>('sdd_audit_read', { id });
  } catch (e) {
    console.warn('sdd_audit_read failed', e);
    return [];
  }
}

/** Append a single audit entry. Used by the frontend stream parser
 *  when intercepting `mcp__app__sdd_*` mutating tool_use events. */
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
 *  phase. Empty string when pre/post SHAs are missing. */
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

/** v2 only — clear the per-phase approval gate. */
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

/** v2 only — mark a pending phase as `skipped`. */
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

/** v2 only — insert a new phase after `after_number`. */
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

export async function markSddManualCheck(
  id: string,
  phase: number,
  check_index: number,
  passed: boolean
): Promise<SddWorkspace | null> {
  try {
    const ws = await invoke<SddWorkspace>('sdd_mark_manual_check', {
      id, phase, check_index, passed,
    });
    upsertWorkspace(ws);
    return ws;
  } catch (e) {
    console.warn('sdd_mark_manual_check failed', e);
    return null;
  }
}

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

export async function getSddGitState(id: string): Promise<SddGitState | null> {
  try {
    return await invoke<SddGitState>('sdd_get_git_state', { id });
  } catch (e) {
    console.warn('sdd_get_git_state failed', e);
    return null;
  }
}

/** Persist the plan-pass output as `phases/<slug>/plan.md` and
 *  advance substep-state. */
export async function saveSddPhasePlan(
  id: string,
  phase: number,
  body: string,
): Promise<SddWorkspace | null> {
  try {
    const ws = await invoke<SddWorkspace>('sdd_save_phase_plan', { id, phase, body });
    upsertWorkspace(ws);
    return ws;
  } catch (e) {
    console.warn('sdd_save_phase_plan failed', e);
    return null;
  }
}

/** Three-call mode — close out the implement pass: advance the
 *  substep checkpoint from `Implement` → `Verify`. */
export async function completeSddPhaseImplement(
  id: string,
  phase: number,
  summary: string,
  filesChanged: string[],
): Promise<SddWorkspace | null> {
  try {
    const ws = await invoke<SddWorkspace>('sdd_complete_phase_implement', {
      id, phase, summary, filesChanged,
    });
    upsertWorkspace(ws);
    return ws;
  } catch (e) {
    console.warn('sdd_complete_phase_implement failed', e);
    return null;
  }
}

export async function saveSddPhaseVerify(
  id: string,
  phase: number,
  rawJson: string,
): Promise<SddWorkspace | null> {
  try {
    const ws = await invoke<SddWorkspace>('sdd_save_phase_verify', { id, phase, rawJson });
    upsertWorkspace(ws);
    return ws;
  } catch (e) {
    console.warn('sdd_save_phase_verify failed', e);
    return null;
  }
}

export async function setSddPhaseExecutionConfig(
  id: string,
  config: PhaseExecutionConfig,
): Promise<SddWorkspace | null> {
  try {
    const ws = await invoke<SddWorkspace>('sdd_set_phase_execution_config', { id, config });
    upsertWorkspace(ws);
    return ws;
  } catch (e) {
    console.warn('sdd_set_phase_execution_config failed', e);
    return null;
  }
}

export async function discardSddPhasePlan(
  id: string,
  phase: number,
  reason?: string,
): Promise<SddWorkspace | null> {
  try {
    const ws = await invoke<SddWorkspace>('sdd_discard_phase_plan', { id, phase, reason });
    upsertWorkspace(ws);
    return ws;
  } catch (e) {
    console.warn('sdd_discard_phase_plan failed', e);
    return null;
  }
}

export async function approveSddPhasePlan(
  id: string,
  phase: number,
): Promise<SddWorkspace | null> {
  try {
    const ws = await invoke<SddWorkspace>('sdd_approve_phase_plan', { id, phase });
    upsertWorkspace(ws);
    return ws;
  } catch (e) {
    console.warn('sdd_approve_phase_plan failed', e);
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
