//! Pause / Resume / Stop lifecycle commands. Extracted from `sdd.rs` in
//! wave-18. Writes the matching control file under `<workspace>/control/`
//! so the next `derive_stage` reads the same state regardless of in-memory
//! overrides, then flips the in-memory stage to the new value.
//!
//! Control files are best-effort: a failed write doesn't poison the
//! stage flip (the in-memory override still wins until the next process
//! restart). `unset_control_files` is the inverse — wipes both pause +
//! stop so `sdd_resume` can rebuild stage fresh from disk.
//!
//! Audit entries: every lifecycle command appends a `pause` / `resume` /
//! `stop` row with actor=`user` so reviewers can see who paused what.

use std::path::{Path, PathBuf};

use tauri::{AppHandle, State};

use crate::sdd::{SddRegistry, SddStage, SddWorkspace};
use crate::sdd_audit as audit;
use crate::sdd_hydrate::rebuild_from_disk;
use crate::sdd_time::now_ms;
use crate::sdd_watcher::emit_changed;

/// Write a control file under `<workspace>/control/<name>`. Best-effort
/// — failures are reported but don't poison the stage flip (the in-memory
/// override still wins until the next process restart). Removing the
/// file is the inverse (`unset_control_file`).
pub(crate) fn set_control_file(root: &Path, name: &str) -> Result<(), String> {
    let dir = root.join("control");
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir control: {e}"))?;
    std::fs::write(dir.join(name), b"").map_err(|e| format!("write control/{name}: {e}"))
}

pub(crate) fn unset_control_files(root: &Path) {
    let _ = std::fs::remove_file(root.join("control/pause"));
    let _ = std::fs::remove_file(root.join("control/stop"));
}

#[tauri::command]
pub async fn sdd_pause(
    app: AppHandle,
    registry: State<'_, SddRegistry>,
    id: String,
) -> Result<SddWorkspace, String> {
    /* Drop the control file FIRST so the next derive_stage picks it up
     *  regardless of in-memory state. */
    if let Some(cell) = registry.workspaces.read().get(&id).cloned() {
        let root = PathBuf::from(cell.read().root.clone());
        let _ = set_control_file(&root, "pause");
        audit::append(&root, &audit::AuditEntry::new("user", "pause"));
    }
    flip_stage(&app, &registry, &id, SddStage::Paused)
}

#[tauri::command]
pub async fn sdd_resume(
    app: AppHandle,
    registry: State<'_, SddRegistry>,
    id: String,
) -> Result<SddWorkspace, String> {
    // Resume = wipe control files + recompute stage from disk.
    let cell = registry
        .workspaces
        .read()
        .get(&id)
        .cloned()
        .ok_or_else(|| format!("unknown workspace {id}"))?;
    {
        let root = PathBuf::from(cell.read().root.clone());
        unset_control_files(&root);
        audit::append(&root, &audit::AuditEntry::new("user", "resume"));
    }
    let snapshot = {
        let mut w = cell.write();
        // Force stage out of Paused so derive_stage can compute fresh.
        w.stage = SddStage::Drafting;
        rebuild_from_disk(&mut w)?;
        w.clone()
    };
    emit_changed(&app, &id);
    Ok(snapshot)
}

#[tauri::command]
pub async fn sdd_stop(
    app: AppHandle,
    registry: State<'_, SddRegistry>,
    id: String,
) -> Result<SddWorkspace, String> {
    if let Some(cell) = registry.workspaces.read().get(&id).cloned() {
        let root = PathBuf::from(cell.read().root.clone());
        let _ = set_control_file(&root, "stop");
        audit::append(&root, &audit::AuditEntry::new("user", "stop"));
    }
    flip_stage(&app, &registry, &id, SddStage::Stopped)
}

pub(crate) fn flip_stage(
    app: &AppHandle,
    registry: &SddRegistry,
    id: &str,
    new_stage: SddStage,
) -> Result<SddWorkspace, String> {
    let cell = registry
        .workspaces
        .read()
        .get(id)
        .cloned()
        .ok_or_else(|| format!("unknown workspace {id}"))?;
    let snapshot = {
        let mut w = cell.write();
        w.stage = new_stage;
        w.updated_at = now_ms();
        w.clone()
    };
    emit_changed(app, id);
    Ok(snapshot)
}
