//! Three-call mode (plan → implement → verify) Tauri commands +
//! plan-review gates + phase-execution config. Extracted from `sdd.rs`
//! in wave-24.
//!
//! Each command advances the per-phase substep state machine
//! (`SddPhaseSubstep`) so the orchestrator's next agent turn picks
//! the right pass prompt. Audit log entries mirror the substep
//! transitions so the SddCard audit overlay shows a clean lifecycle
//! (plan_started → plan_completed → implement_started → ... ).
//!
//! See `spec-1` FR-3 through FR-7 and FR-11/FR-12 for the contract
//! these commands fulfill.

use std::path::PathBuf;

use tauri::{AppHandle, State};

use crate::sdd::{SddRegistry, SddWorkspace};
use crate::sdd_action_log::append_substep_started_event;
use crate::sdd_audit as audit;
use crate::sdd_hydrate::rebuild_from_disk;
use crate::sdd_lifecycle_commands::set_control_file;
use crate::sdd_md_mutators::set_status_and_summary_on;
use crate::sdd_meta::{read_phase_meta, read_workspace_meta, write_phase_meta, write_workspace_meta};
use crate::sdd_phase_config::PhaseExecutionConfig;
use crate::sdd_phase_io::{write_phase_plan_md, write_verify_json, VerifyOutput};
use crate::sdd_plan_helpers::set_status_with_extras;
use crate::sdd_substep::{
    clear_substep_state, write_substep_state, SddPhaseSubstep, SddPhaseSubstepState,
};
use crate::sdd_time::now_ms;
use crate::sdd_watcher::emit_changed;

#[tauri::command]
pub async fn sdd_save_phase_plan(
    app: AppHandle,
    registry: State<'_, SddRegistry>,
    id: String,
    phase: u32,
    body: String,
) -> Result<SddWorkspace, String> {
    let cell = registry
        .workspaces
        .read()
        .get(&id)
        .cloned()
        .ok_or_else(|| format!("unknown workspace {id}"))?;
    let (root, slug, plan_gate) = {
        let w = cell.read();
        let p = w
            .phases
            .iter()
            .find(|p| p.number == phase)
            .ok_or_else(|| format!("phase {phase} not found"))?;
        (
            PathBuf::from(&w.root),
            p.slug.clone(),
            w.phase_execution.plan_gate,
        )
    };
    write_phase_plan_md(&root, &slug, &body)?;
    // Advance the substep checkpoint. plan_gate=true → stay on `Plan`
    // (user needs to Approve via `sdd_approve_phase_plan`). Otherwise
    // jump straight to `Implement` so the next agent turn fires the
    // implement-pass prompt.
    let next = if plan_gate {
        SddPhaseSubstep::Plan
    } else {
        SddPhaseSubstep::Implement
    };
    write_substep_state(
        &root,
        &SddPhaseSubstepState {
            phase,
            sub_step: Some(next),
            started_at: now_ms(),
        },
    )?;
    // Only drop a substep divider when we actually advanced (not
    // when plan_gate parks us on Plan waiting for the user). The
    // plan-pass start event was already emitted by sdd_approve_phase.
    if !plan_gate {
        append_substep_started_event(&root, phase, SddPhaseSubstep::Implement);
        audit::append(
            &root,
            &audit::AuditEntry::new("agent", "phase_implement_started")
                .with_phase(phase)
                .with_after(serde_json::json!({"sub_step": "implement"})),
        );
        audit::append(
            &root,
            &audit::AuditEntry::new("agent", "phase_plan_completed")
                .with_phase(phase)
                .with_after(serde_json::json!({"sub_step": "plan"})),
        );
    } else {
        audit::append(
            &root,
            &audit::AuditEntry::new("agent", "phase_plan_completed")
                .with_phase(phase)
                .with_after(serde_json::json!({"sub_step": "plan", "gate": "review"})),
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

/// Three-call mode — close out the implement pass: advance the
/// substep checkpoint from `Implement` → `Verify` so the orchestrator
/// fires the verify-pass prompt on the next agent turn. `summary` +
/// `files_changed` are persisted on the phase frontmatter (matching
/// `sdd_save_phase_verify`'s contract, so the verifier downstream
/// has the implement-pass-side narrative available). No status flip
/// happens here — that's the verify pass's job. See `spec-1` FR-3.
#[tauri::command]
pub async fn sdd_complete_phase_implement(
    app: AppHandle,
    registry: State<'_, SddRegistry>,
    id: String,
    phase: u32,
    summary: String,
    files_changed: Vec<String>,
) -> Result<SddWorkspace, String> {
    let cell = registry
        .workspaces
        .read()
        .get(&id)
        .cloned()
        .ok_or_else(|| format!("unknown workspace {id}"))?;
    let (root, phase_path) = {
        let w = cell.read();
        let p = w
            .phases
            .iter()
            .find(|p| p.number == phase)
            .ok_or_else(|| format!("phase {phase} not found"))?;
        (PathBuf::from(&w.root), PathBuf::from(&p.path))
    };
    let summary_opt: Option<&str> = if summary.trim().is_empty() {
        None
    } else {
        Some(summary.as_str())
    };
    // Carry the implement-pass summary onto the phase frontmatter so
    // the verify pass + result.md generator can quote it. Status stays
    // `running` — verify pass is the one that flips done/failed.
    if summary_opt.is_some() {
        set_status_and_summary_on(&phase_path, "running", summary_opt)?;
    }
    write_substep_state(
        &root,
        &SddPhaseSubstepState {
            phase,
            sub_step: Some(SddPhaseSubstep::Verify),
            started_at: now_ms(),
        },
    )?;
    append_substep_started_event(&root, phase, SddPhaseSubstep::Verify);
    audit::append(
        &root,
        &audit::AuditEntry::new("agent", "phase_implement_completed")
            .with_phase(phase)
            .with_after(serde_json::json!({
                "sub_step": "implement",
                "files_changed_count": files_changed.len(),
            })),
    );
    audit::append(
        &root,
        &audit::AuditEntry::new("agent", "phase_verify_started")
            .with_phase(phase)
            .with_after(serde_json::json!({"sub_step": "verify"})),
    );
    let snapshot = {
        let mut w = cell.write();
        rebuild_from_disk(&mut w)?;
        w.clone()
    };
    emit_changed(&app, &id);
    Ok(snapshot)
}

/// Three-call mode — persist the verify-pass JSON, auto-fill phase
/// frontmatter `summary`, flip phase status to `done` (no deviations)
/// or `failed { trigger: verify_failed }` (deviations present), clear
/// the substep checkpoint, emit `sdd:changed`. See `spec-1` FR-4
/// through FR-6.
#[tauri::command]
pub async fn sdd_save_phase_verify(
    app: AppHandle,
    registry: State<'_, SddRegistry>,
    id: String,
    phase: u32,
    raw_json: String,
) -> Result<SddWorkspace, String> {
    let cell = registry
        .workspaces
        .read()
        .get(&id)
        .cloned()
        .ok_or_else(|| format!("unknown workspace {id}"))?;
    let (root, slug, phase_path) = {
        let w = cell.read();
        let p = w
            .phases
            .iter()
            .find(|p| p.number == phase)
            .ok_or_else(|| format!("phase {phase} not found"))?;
        (
            PathBuf::from(&w.root),
            p.slug.clone(),
            PathBuf::from(&p.path),
        )
    };
    let verdict = VerifyOutput::parse_or_fallback(&raw_json);
    write_verify_json(&root, &slug, &verdict)?;
    let new_status = if verdict.deviations.is_empty() {
        "done"
    } else {
        "failed"
    };
    let summary_opt: Option<&str> = if verdict.summary.trim().is_empty() {
        None
    } else {
        Some(verdict.summary.as_str())
    };
    set_status_and_summary_on(&phase_path, new_status, summary_opt)?;
    // Phase complete — drop the checkpoint so a future hydrate
    // doesn't think the phase is still running.
    let _ = clear_substep_state(&root, phase);
    // Audit the verify-pass lifecycle. We emit both started+completed
    // here because the verify call kicks off from the same agent turn
    // that closes it — no intermediate orchestrator hook to insert
    // the "started" row earlier. See `spec-1` audit requirements.
    append_substep_started_event(&root, phase, SddPhaseSubstep::Verify);
    audit::append(
        &root,
        &audit::AuditEntry::new("agent", "phase_implement_completed")
            .with_phase(phase)
            .with_after(serde_json::json!({"sub_step": "implement"})),
    );
    audit::append(
        &root,
        &audit::AuditEntry::new("agent", "phase_verify_started")
            .with_phase(phase)
            .with_after(serde_json::json!({"sub_step": "verify"})),
    );
    audit::append(
        &root,
        &audit::AuditEntry::new("agent", "phase_verify_completed")
            .with_phase(phase)
            .with_after(serde_json::json!({
                "status": new_status,
                "deviations_count": verdict.deviations.len(),
                "files_changed_count": verdict.files_changed.len(),
            })),
    );
    let snapshot = {
        let mut w = cell.write();
        rebuild_from_disk(&mut w)?;
        w.clone()
    };
    emit_changed(&app, &id);
    Ok(snapshot)
}

/// Three-call mode — clear the plan-review gate by writing
/// `control/phase-<N>-plan-approved`, advance substep-state to
/// `Implement`, emit `sdd:changed`. See `spec-1` FR-7.
#[tauri::command]
pub async fn sdd_approve_phase_plan(
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
    set_control_file(&root, &format!("phase-{phase}-plan-approved"))?;
    write_substep_state(
        &root,
        &SddPhaseSubstepState {
            phase,
            sub_step: Some(SddPhaseSubstep::Implement),
            started_at: now_ms(),
        },
    )?;
    append_substep_started_event(&root, phase, SddPhaseSubstep::Implement);
    audit::append(
        &root,
        &audit::AuditEntry::new("user", "approve_phase_plan").with_phase(phase),
    );
    audit::append(
        &root,
        &audit::AuditEntry::new("agent", "phase_implement_started")
            .with_phase(phase)
            .with_after(serde_json::json!({"sub_step": "implement"})),
    );
    let snapshot = {
        let mut w = cell.write();
        rebuild_from_disk(&mut w)?;
        w.clone()
    };
    emit_changed(&app, &id);
    Ok(snapshot)
}

/// Three-call mode — discard the plan-pass output during plan-review
/// gate. Flips the phase to `failed` with `trigger: plan_discarded`
/// so the standard failure-card recovery flow (Retry / Edit & retry
/// / Skip) takes over. Different from `sdd_skip_phase_with_reason`
/// (which marks `skipped` and bypasses the failure card). See
/// `spec-1` FR-7.
#[tauri::command]
pub async fn sdd_discard_phase_plan(
    app: AppHandle,
    registry: State<'_, SddRegistry>,
    id: String,
    phase: u32,
    reason: Option<String>,
) -> Result<SddWorkspace, String> {
    let cell = registry
        .workspaces
        .read()
        .get(&id)
        .cloned()
        .ok_or_else(|| format!("unknown workspace {id}"))?;
    let (root, phase_path) = {
        let w = cell.read();
        let p = w
            .phases
            .iter()
            .find(|p| p.number == phase)
            .ok_or_else(|| format!("phase {phase} not found"))?;
        (PathBuf::from(&w.root), PathBuf::from(&p.path))
    };
    let trimmed_reason = reason
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("plan discarded by user during plan-review gate");
    set_status_with_extras(
        &phase_path,
        "failed",
        vec![
            ("trigger".into(), "plan_discarded".into()),
            ("discard_reason".into(), trimmed_reason.into()),
        ],
    )?;
    // Persist reason into phase-meta.json too so the audit log can
    // surface it without re-parsing markdown frontmatter.
    let mut phase_meta = read_phase_meta(&root, phase);
    phase_meta.skip_reason = Some(trimmed_reason.into());
    let _ = write_phase_meta(&root, phase, &phase_meta);
    // Clear the substep checkpoint so a future hydrate doesn't
    // re-trigger the recovery banner on this aborted phase.
    let _ = clear_substep_state(&root, phase);
    audit::append(
        &root,
        &audit::AuditEntry::new("user", "discard_phase_plan")
            .with_phase(phase)
            .with_after(serde_json::json!({
                "trigger": "plan_discarded",
                "reason": trimmed_reason,
            })),
    );
    let snapshot = {
        let mut w = cell.write();
        rebuild_from_disk(&mut w)?;
        w.clone()
    };
    emit_changed(&app, &id);
    Ok(snapshot)
}

/// Persist `meta.json#phase_execution` for the workspace. Validates
/// the budget-pct sum before writing so callers get a typed error
/// instead of a malformed file on disk. Emits an audit row + a
/// `sdd:changed` event so the SddCard reactive store refreshes
/// immediately. See `spec-1` FR-11 / FR-12.
#[tauri::command]
pub async fn sdd_set_phase_execution_config(
    app: AppHandle,
    registry: State<'_, SddRegistry>,
    id: String,
    config: PhaseExecutionConfig,
) -> Result<SddWorkspace, String> {
    config.validate()?;
    let cell = registry
        .workspaces
        .read()
        .get(&id)
        .cloned()
        .ok_or_else(|| format!("unknown workspace {id}"))?;
    let root = PathBuf::from(cell.read().root.clone());
    let mut meta = read_workspace_meta(&root);
    let before = meta.phase_execution.clone();
    meta.phase_execution = config.clone();
    write_workspace_meta(&root, &meta)?;
    audit::append(
        &root,
        &audit::AuditEntry::new("user", "set_phase_execution_config")
            .with_before(serde_json::to_value(&before).unwrap_or(serde_json::Value::Null))
            .with_after(serde_json::to_value(&config).unwrap_or(serde_json::Value::Null)),
    );
    let snapshot = {
        let mut w = cell.write();
        rebuild_from_disk(&mut w)?;
        w.clone()
    };
    emit_changed(&app, &id);
    Ok(snapshot)
}
