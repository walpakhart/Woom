//! User-driven gate + edit + retry + discard Tauri commands. Extracted
//! from `sdd.rs` in wave-26.
//!
//! Covers the commands a human triggers from the SddCard:
//!   - `sdd_approve` — flip spec/plan `status: draft` → `approved`.
//!   - `sdd_save_body` — replace spec/plan/phase markdown body (frontmatter
//!     preserved verbatim).
//!   - `sdd_retry_phase` — reset a failed/done phase to `pending` so the
//!     next advance re-issues it. Bumps `retry_count` on phase-meta.
//!   - `sdd_approve_phase` — open the per-phase approval gate by writing
//!     `control/phase-N-approved`. Captures a pre-phase git snapshot so
//!     `sdd_rollback_phase` has a target later, and in three-call mode
//!     seeds the substep state with `Plan` so the orchestrator fires the
//!     plan-pass prompt.
//!   - `sdd_discard` — wipe workspace dir + drop from registry.

use std::path::PathBuf;

use serde::Deserialize;
use tauri::{AppHandle, Emitter, State};

use crate::sdd::{SddRegistry, SddWorkspace};
use crate::sdd_action_log::append_substep_started_event;
use crate::sdd_audit as audit;
use crate::sdd_hydrate::rebuild_from_disk;
use crate::sdd_lifecycle_commands::set_control_file;
use crate::sdd_md_mutators::{replace_body_on, reset_phase_status, set_status_on};
use crate::sdd_meta::{read_phase_meta, read_workspace_meta, write_phase_meta};
use crate::sdd_phase_config::PhaseExecutionMode;
use crate::sdd_substep::{write_substep_state, SddPhaseSubstep, SddPhaseSubstepState};
use crate::sdd_time::now_ms;
use crate::sdd_watcher::emit_changed;

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SddApproveTarget {
    Spec,
    Plan,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SddEditTarget {
    Spec,
    Plan,
    Phase { number: u32 },
}

#[tauri::command]
pub async fn sdd_approve(
    app: AppHandle,
    registry: State<'_, SddRegistry>,
    id: String,
    target: SddApproveTarget,
) -> Result<SddWorkspace, String> {
    let cell = registry
        .workspaces
        .read()
        .get(&id)
        .cloned()
        .ok_or_else(|| format!("unknown workspace {id}"))?;
    // Read paths under the lock, then drop the guard before disk I/O.
    let path: PathBuf = {
        let w = cell.read();
        match target {
            SddApproveTarget::Spec => w
                .spec_path
                .clone()
                .map(PathBuf::from)
                .ok_or_else(|| "no spec yet".to_string())?,
            SddApproveTarget::Plan => w
                .plan_path
                .clone()
                .map(PathBuf::from)
                .ok_or_else(|| "no plan yet".to_string())?,
        }
    };
    set_status_on(&path, "approved")?;
    {
        let root = PathBuf::from(cell.read().root.clone());
        let action = match target {
            SddApproveTarget::Spec => "approve_spec",
            SddApproveTarget::Plan => "approve_plan",
        };
        audit::append(
            &root,
            &audit::AuditEntry::new("user", action)
                .with_after(serde_json::json!({"status": "approved"})),
        );
    }
    let snapshot = {
        let mut w = cell.write();
        rebuild_from_disk(&mut w)?;
        w.clone()
    };
    emit_changed(&app, &id);
    Ok(snapshot)
}

/// Save user-edited body for spec / plan / a specific phase. The YAML
/// frontmatter is preserved verbatim — we only swap the markdown
/// content. Used by the card's inline "edit" affordance so the user
/// can tweak the agent's spec/plan before approving without round-
/// tripping through a text editor.
#[tauri::command]
pub async fn sdd_save_body(
    app: AppHandle,
    registry: State<'_, SddRegistry>,
    id: String,
    target: SddEditTarget,
    body: String,
) -> Result<SddWorkspace, String> {
    let cell = registry
        .workspaces
        .read()
        .get(&id)
        .cloned()
        .ok_or_else(|| format!("unknown workspace {id}"))?;
    let path: PathBuf = {
        let w = cell.read();
        match target {
            SddEditTarget::Spec => w
                .spec_path
                .clone()
                .map(PathBuf::from)
                .ok_or_else(|| "no spec yet".to_string())?,
            SddEditTarget::Plan => w
                .plan_path
                .clone()
                .map(PathBuf::from)
                .ok_or_else(|| "no plan yet".to_string())?,
            SddEditTarget::Phase { number } => w
                .phases
                .iter()
                .find(|p| p.number == number)
                .map(|p| PathBuf::from(&p.path))
                .ok_or_else(|| format!("phase {number} not found"))?,
        }
    };
    replace_body_on(&path, &body)?;
    let snapshot = {
        let mut w = cell.write();
        rebuild_from_disk(&mut w)?;
        w.clone()
    };
    emit_changed(&app, &id);
    Ok(snapshot)
}

/// Reset a failed (or done) phase back to `pending` so the next
/// advance re-issues it. Used by the card's "Retry" button on a
/// failed phase. The user is the gate — we don't auto-retry inside
/// the agent prompt (that's the per-prompt `{{retries_max}}` budget);
/// THIS reset is for the case where verification failed AFTER the
/// in-prompt retries were exhausted.
#[tauri::command]
pub async fn sdd_retry_phase(
    app: AppHandle,
    registry: State<'_, SddRegistry>,
    id: String,
    phase: u32,
) -> Result<SddWorkspace, String> {
    let cell = registry
        .workspaces
        .read()
        .get(&id)
        .cloned()
        .ok_or_else(|| format!("unknown workspace {id}"))?;
    let (path, root): (PathBuf, PathBuf) = {
        let w = cell.read();
        let p = w
            .phases
            .iter()
            .find(|p| p.number == phase)
            .ok_or_else(|| format!("phase {phase} not found"))?;
        (PathBuf::from(&p.path), PathBuf::from(&w.root))
    };
    reset_phase_status(&path)?;
    /* Bump `retry_count` so the UI can warn at the 3rd attempt. We
     *  do this even if reset_phase_status was a no-op (re-clicking
     *  retry on a phase that's already pending is rare but harmless
     *  to count). */
    let mut pm = read_phase_meta(&root, phase);
    let prev_retries = pm.retry_count;
    pm.retry_count = pm.retry_count.saturating_add(1);
    let _ = write_phase_meta(&root, phase, &pm);
    audit::append(
        &root,
        &audit::AuditEntry::new("user", "retry_phase")
            .with_phase(phase)
            .with_before(serde_json::json!({"retry_count": prev_retries}))
            .with_after(serde_json::json!({"retry_count": pm.retry_count})),
    );
    let snapshot = {
        let mut w = cell.write();
        rebuild_from_disk(&mut w)?;
        w.clone()
    };
    emit_changed(&app, &id);
    Ok(snapshot)
}

/// Open the gate for phase `phase` on a v2 workspace by writing
/// `<workspace>/control/phase-<phase>-approved`. `derive_stage` reads
/// this file and falls through from `PhasePendingApproval` to the
/// usual `PlanReady` / `PhaseDone` path, which the frontend uses to
/// fire the phase prompt. Idempotent — a second call on an already-
/// approved phase is fine. No-op for legacy workspaces (no plan.json):
/// the stage was never `PhasePendingApproval` to begin with.
#[tauri::command]
pub async fn sdd_approve_phase(
    app: AppHandle,
    registry: State<'_, SddRegistry>,
    id: String,
    phase: u32,
) -> Result<SddWorkspace, String> {
    let cell = registry
        .workspaces
        .read()
        .get(&id)
        .cloned()
        .ok_or_else(|| format!("unknown workspace {id}"))?;
    let root = PathBuf::from(cell.read().root.clone());
    /* Pre-phase git snapshot. Captured BEFORE writing the gate file so
     *  if the snapshot path errors out, the user hasn't moved the
     *  workflow forward. Failures are logged but non-fatal — the phase
     *  still gets approved (degraded: rollback unavailable). */
    let meta = read_workspace_meta(&root);
    let mut phase_meta = read_phase_meta(&root, phase);
    if meta.git_enabled {
        if let Some(repo) = meta.repo_cwd.as_deref() {
            // Stash any non-SDD work so the pre-phase commit doesn't
            // sweep up unrelated edits. Safety-stash carries a per-
            // workspace label so the user can find it back via
            // `git stash list`.
            let stash_label = format!("sdd-pre-phase-{}-{}", phase, id);
            let _ = crate::git::stash_with_label(repo, &stash_label);
            let msg = format!("[sdd:pre-phase-{}] {}", phase, id);
            match crate::git::commit_all_allow_empty(repo, &msg) {
                Ok(sha) => {
                    phase_meta.pre_phase_sha = Some(sha);
                    phase_meta.snapshot_skipped = false;
                    let _ = write_phase_meta(&root, phase, &phase_meta);
                }
                Err(e) => {
                    eprintln!(
                        "[sdd] workspace={id} phase={phase}: pre-phase snapshot failed: {e}"
                    );
                    phase_meta.snapshot_skipped = true;
                    let _ = write_phase_meta(&root, phase, &phase_meta);
                }
            }
        }
    } else {
        phase_meta.snapshot_skipped = true;
        let _ = write_phase_meta(&root, phase, &phase_meta);
    }
    set_control_file(&root, &format!("phase-{phase}-approved"))?;
    audit::append(
        &root,
        &audit::AuditEntry::new("user", "advance_phase")
            .with_phase(phase)
            .with_after(serde_json::json!({"approved": true})),
    );
    // Three-call mode bootstrap — flip the phase to `running` and
    // seed substep-state with `Plan` so derive_stage emits
    // `PhasePlanning` on the next refresh. The plan-pass prompt fires
    // through the standard `buildPromptForStage` path. Single-call
    // mode does NOT touch the substep file; phase status stays
    // `pending` until the agent flips it via `sdd_log_phase_done`.
    let three_call = matches!(
        cell.read().phase_execution.mode,
        PhaseExecutionMode::ThreeCall
    );
    if three_call {
        let phase_path = cell
            .read()
            .phases
            .iter()
            .find(|p| p.number == phase)
            .map(|p| PathBuf::from(&p.path));
        if let Some(path) = phase_path {
            let _ = set_status_on(&path, "running");
        }
        let _ = write_substep_state(
            &root,
            &SddPhaseSubstepState {
                phase,
                sub_step: Some(SddPhaseSubstep::Plan),
                started_at: now_ms(),
            },
        );
        append_substep_started_event(&root, phase, SddPhaseSubstep::Plan);
        audit::append(
            &root,
            &audit::AuditEntry::new("system", "phase_plan_started")
                .with_phase(phase)
                .with_after(serde_json::json!({"sub_step": "plan"})),
        );
    }
    let snapshot = {
        let mut w = cell.write();
        rebuild_from_disk(&mut w)?;
        w.clone()
    };
    emit_changed(&app, &id);
    Ok(snapshot)
}

/// Optional convenience — wipe a workspace dir from disk + drop it from
/// memory. Used by the UI's "discard" button on a stopped workspace.
#[tauri::command]
pub async fn sdd_discard(
    app: AppHandle,
    registry: State<'_, SddRegistry>,
    id: String,
) -> Result<(), String> {
    let cell = registry
        .workspaces
        .read()
        .get(&id)
        .cloned()
        .ok_or_else(|| format!("unknown workspace {id}"))?;
    let root = PathBuf::from(cell.read().root.clone());
    let _ = std::fs::remove_dir_all(&root);
    registry.workspaces.write().remove(&id);
    let _ = app.emit("sdd:discarded", &id);
    Ok(())
}
