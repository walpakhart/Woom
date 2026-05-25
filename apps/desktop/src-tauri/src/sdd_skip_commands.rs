//! Skip / accept-failed phase commands. Extracted from `sdd.rs` in
//! wave-23. All three share the same shape:
//!   1. Look up the workspace + target phase.
//!   2. Refuse if the phase is in the wrong state (running / not-failed).
//!   3. Patch the phase frontmatter via `set_status_with_extras` (one
//!      atomic round-trip that flips `status` + adds the extra keys).
//!   4. Mirror the reason into `phase-N-meta.json` so audit/recovery
//!      flows can read it without re-parsing the markdown.
//!   5. Append an audit-log entry, refresh snapshot, emit_changed.

use std::path::PathBuf;

use tauri::{AppHandle, State};

use crate::sdd::{SddRegistry, SddWorkspace};
use crate::sdd_audit as audit;
use crate::sdd_hydrate::rebuild_from_disk;
use crate::sdd_meta::{read_phase_meta, write_phase_meta};
use crate::sdd_plan_helpers::set_status_with_extras;
use crate::sdd_watcher::emit_changed;

/// Mark phase `phase` as `skipped` so the gate releases and the next
/// pending phase becomes the candidate. Optional `reason` is appended
/// to the phase frontmatter as a `skip_reason` key. Refuses to skip
/// `running` phases — pause first, then skip from a settled state.
#[tauri::command]
pub async fn sdd_skip_phase(
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
    let path: PathBuf = {
        let w = cell.read();
        let p = w
            .phases
            .iter()
            .find(|p| p.number == phase)
            .ok_or_else(|| format!("phase {phase} not found"))?;
        if p.status == "running" {
            return Err(format!(
                "phase {phase} is currently running — pause first, then skip"
            ));
        }
        PathBuf::from(&p.path)
    };
    set_status_with_extras(
        &path,
        "skipped",
        reason
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .map(|r| ("skip_reason".to_string(), r.to_string()))
            .into_iter()
            .collect(),
    )?;
    let snapshot = {
        let mut w = cell.write();
        rebuild_from_disk(&mut w)?;
        w.clone()
    };
    emit_changed(&app, &id);
    Ok(snapshot)
}

/// Skip a phase with a mandatory reason — used by the failure card's
/// `Skip phase` button to bypass a verifier-flagged failure. Differs
/// from `sdd_skip_phase` in two ways: (a) `reason` is REQUIRED (min 5
/// chars after trim), so the audit trail always has a "why"; (b) it
/// also persists `skip_reason` to `phase-N-meta.json` so the audit
/// log (phase 6) and any future analytics can pick it up without
/// re-parsing the markdown frontmatter.
///
/// Failed phases ARE skippable (the whole point of this command).
/// `running` phases are refused — same as `sdd_skip_phase` — to avoid
/// racing with an active executor.
#[tauri::command]
pub async fn sdd_skip_phase_with_reason(
    app: AppHandle,
    registry: State<'_, SddRegistry>,
    id: String,
    phase: u32,
    reason: String,
) -> Result<SddWorkspace, String> {
    let trimmed = reason.trim();
    if trimmed.len() < 5 {
        return Err(
            "skip reason must be at least 5 characters — explain why this phase is being skipped"
                .into(),
        );
    }
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
        if p.status == "running" {
            return Err(format!(
                "phase {phase} is currently running — pause first, then skip"
            ));
        }
        (PathBuf::from(&p.path), PathBuf::from(&w.root))
    };
    set_status_with_extras(
        &path,
        "skipped",
        vec![("skip_reason".to_string(), trimmed.to_string())],
    )?;
    /* Mirror the reason into phase-meta.json so the audit log and
     *  recovery flows can read it without re-parsing the markdown
     *  frontmatter. */
    let mut pm = read_phase_meta(&root, phase);
    pm.skip_reason = Some(trimmed.to_string());
    let _ = write_phase_meta(&root, phase, &pm);
    audit::append(
        &root,
        &audit::AuditEntry::new("user", "skip_phase")
            .with_phase(phase)
            .with_reason(trimmed)
            .with_after(serde_json::json!({"status": "skipped"})),
    );
    let snapshot = {
        let mut w = cell.write();
        rebuild_from_disk(&mut w)?;
        w.clone()
    };
    emit_changed(&app, &id);
    Ok(snapshot)
}

/// Accept a `failed` phase as-is — flips `status: failed` → `done`
/// and records `accepted_reason` in the phase frontmatter + meta.json.
/// Used by the failure card's "Accept anyway" button when the verifier
/// flagged deviations the user has reviewed and decided are tolerable
/// (e.g. acknowledged trade-offs documented in the verify summary).
/// Requires a non-empty reason (min 5 chars) so the audit trail always
/// has a why. Refuses to accept anything other than a `failed` phase —
/// running / pending / done phases stay untouched.
#[tauri::command]
pub async fn sdd_accept_phase_failed(
    app: AppHandle,
    registry: State<'_, SddRegistry>,
    id: String,
    phase: u32,
    reason: String,
) -> Result<SddWorkspace, String> {
    let trimmed = reason.trim();
    if trimmed.len() < 5 {
        return Err(
            "accept reason must be at least 5 characters — explain why this failure is being accepted"
                .into(),
        );
    }
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
        if p.status != "failed" {
            return Err(format!(
                "phase {phase} is `{}` — Accept only applies to failed phases",
                p.status
            ));
        }
        (PathBuf::from(&p.path), PathBuf::from(&w.root))
    };
    set_status_with_extras(
        &path,
        "done",
        vec![("accepted_reason".to_string(), trimmed.to_string())],
    )?;
    let mut pm = read_phase_meta(&root, phase);
    pm.accepted_reason = Some(trimmed.to_string());
    let _ = write_phase_meta(&root, phase, &pm);
    audit::append(
        &root,
        &audit::AuditEntry::new("user", "accept_phase_failed")
            .with_phase(phase)
            .with_reason(trimmed)
            .with_after(serde_json::json!({"status": "done"})),
    );
    let snapshot = {
        let mut w = cell.write();
        rebuild_from_disk(&mut w)?;
        w.clone()
    };
    emit_changed(&app, &id);
    Ok(snapshot)
}
