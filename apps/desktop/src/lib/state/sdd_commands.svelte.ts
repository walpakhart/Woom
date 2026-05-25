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
  upsertWorkspace,
  type AuditEntry,
  type SddPhaseDiff,
  type SddWorkspace,
} from './sdd.svelte';

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
