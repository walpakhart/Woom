//! Phase-structure mutation commands: insert, reorder, delete, upgrade.
//! Extracted from `sdd.rs` in wave-22. All four go through the same
//! sequence:
//!   1. Validate the requested change against the in-memory snapshot.
//!   2. Shuffle the underlying phase markdown files on disk.
//!   3. `rebuild_plan_json` so plan.json mirrors the new on-disk layout.
//!   4. `rebuild_from_disk` to refresh the in-memory snapshot.
//!   5. `emit_changed` so the SddCard re-renders.
//!
//! Heavy file-shuffling lives in `sdd_plan_helpers.rs`; this module is
//! just the Tauri orchestration glue.

use std::collections::HashMap;
use std::path::PathBuf;

use tauri::{AppHandle, State};

use crate::sdd::{SddRegistry, SddWorkspace};
use crate::sdd_hydrate::rebuild_from_disk;
use crate::sdd_plan_helpers::{
    rebuild_plan_json, rename_phase_file, slugify, update_phase_number_in_frontmatter,
};
use crate::sdd_watcher::emit_changed;

#[tauri::command]
pub async fn sdd_insert_phase(
    app: AppHandle,
    registry: State<'_, SddRegistry>,
    id: String,
    after_number: u32,
    title: String,
    depends_on: Option<Vec<u32>>,
) -> Result<SddWorkspace, String> {
    let cell = registry
        .workspaces
        .read()
        .get(&id)
        .cloned()
        .ok_or_else(|| format!("unknown workspace {id}"))?;
    let root = PathBuf::from(cell.read().root.clone());
    if !root.join("plan.json").exists() {
        return Err("workspace is not v2 — call sdd_upgrade_workspace first".into());
    }
    let phases_dir = root.join("phases");
    std::fs::create_dir_all(&phases_dir).map_err(|e| format!("mkdir phases: {e}"))?;
    /* Existing phases — sort descending so we rename higher numbers
     * first and never overwrite an in-use file. */
    let mut existing: Vec<(u32, String)> = cell
        .read()
        .phases
        .iter()
        .map(|p| (p.number, p.slug.clone()))
        .collect();
    existing.sort_by(|a, b| b.0.cmp(&a.0));
    for (num, slug) in &existing {
        if *num > after_number {
            rename_phase_file(&phases_dir, slug, num + 1)?;
        }
    }
    let new_number = after_number + 1;
    let slug_title = slugify(&title);
    let new_slug = format!("{:02}-{}", new_number, slug_title);
    let new_path = phases_dir.join(format!("{new_slug}.md"));
    let depends_repr = match depends_on.as_ref() {
        Some(v) if !v.is_empty() => format!(
            "[{}]",
            v.iter().map(|n| n.to_string()).collect::<Vec<_>>().join(", ")
        ),
        _ => "[]".into(),
    };
    let body = format!(
        "---\nphase: {new_number}\ntitle: {title}\ndepends_on: {depends_repr}\nstatus: pending\ntasks_total: 0\ntasks_completed: 0\n---\n\n## Goal\n\n_(inserted phase — fill in)_\n\n## Tasks\n\n## Verification\n\n```bash\n```\n"
    );
    std::fs::write(&new_path, body).map_err(|e| format!("write phase: {e}"))?;
    rebuild_plan_json(&root)?;
    let snapshot = {
        let mut w = cell.write();
        rebuild_from_disk(&mut w)?;
        w.clone()
    };
    emit_changed(&app, &id);
    Ok(snapshot)
}

/// Reorder phases. `new_order` is a permutation of the current phase
/// numbers — `[3, 1, 2]` means "phase 3 becomes number 1, original 1
/// becomes 2, original 2 becomes 3". Validates the permutation, then
/// renames files via a 2-pass `.staged` shuffle to avoid collisions.
#[tauri::command]
pub async fn sdd_reorder_phases(
    app: AppHandle,
    registry: State<'_, SddRegistry>,
    id: String,
    new_order: Vec<u32>,
) -> Result<SddWorkspace, String> {
    let cell = registry
        .workspaces
        .read()
        .get(&id)
        .cloned()
        .ok_or_else(|| format!("unknown workspace {id}"))?;
    let root = PathBuf::from(cell.read().root.clone());
    if !root.join("plan.json").exists() {
        return Err("workspace is not v2 — call sdd_upgrade_workspace first".into());
    }
    let existing: Vec<(u32, String)> = cell
        .read()
        .phases
        .iter()
        .map(|p| (p.number, p.slug.clone()))
        .collect();
    if new_order.len() != existing.len() {
        return Err(format!(
            "new_order length {} != phase count {}",
            new_order.len(),
            existing.len()
        ));
    }
    {
        let mut sorted = new_order.clone();
        sorted.sort();
        let mut existing_nums: Vec<u32> = existing.iter().map(|(n, _)| *n).collect();
        existing_nums.sort();
        if sorted != existing_nums {
            return Err("new_order is not a permutation of existing phase numbers".into());
        }
    }
    let phases_dir = root.join("phases");
    /* Two-pass shuffle: first move every phase to a `.staged-<oldnum>.md`
     * intermediate name, then rename to the final `<newnum>-<slug>.md`.
     * This avoids the case where two renames target the same destination
     * mid-sequence. */
    let mut slug_by_old: HashMap<u32, String> = HashMap::new();
    for (num, slug) in &existing {
        slug_by_old.insert(*num, slug.clone());
        let from = phases_dir.join(format!("{slug}.md"));
        let staged = phases_dir.join(format!(".staged-{num}.md"));
        std::fs::rename(&from, &staged)
            .map_err(|e| format!("stage rename {}: {e}", from.display()))?;
    }
    for (idx, old_num) in new_order.iter().enumerate() {
        let new_num = (idx + 1) as u32;
        let slug = slug_by_old
            .get(old_num)
            .cloned()
            .ok_or_else(|| format!("internal: missing slug for {old_num}"))?;
        let suffix = slug.split_once('-').map(|(_, s)| s).unwrap_or(&slug);
        let staged = phases_dir.join(format!(".staged-{old_num}.md"));
        let dest = phases_dir.join(format!("{:02}-{suffix}.md", new_num));
        std::fs::rename(&staged, &dest)
            .map_err(|e| format!("final rename {}: {e}", staged.display()))?;
        update_phase_number_in_frontmatter(&dest, new_num)?;
    }
    rebuild_plan_json(&root)?;
    let snapshot = {
        let mut w = cell.write();
        rebuild_from_disk(&mut w)?;
        w.clone()
    };
    emit_changed(&app, &id);
    Ok(snapshot)
}

/// Delete a phase. Refuses if the phase is `running` (would orphan
/// agent's in-flight work). Renumbers subsequent phases down by one.
#[tauri::command]
pub async fn sdd_delete_phase(
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
    if !root.join("plan.json").exists() {
        return Err("workspace is not v2 — call sdd_upgrade_workspace first".into());
    }
    let (target_path, gate_path, existing): (PathBuf, PathBuf, Vec<(u32, String)>) = {
        let w = cell.read();
        let p = w
            .phases
            .iter()
            .find(|p| p.number == phase)
            .ok_or_else(|| format!("phase {phase} not found"))?;
        if p.status == "running" {
            return Err(format!("phase {phase} is running — pause first"));
        }
        (
            PathBuf::from(&p.path),
            root.join(format!("control/phase-{phase}-approved")),
            w.phases
                .iter()
                .map(|p| (p.number, p.slug.clone()))
                .collect(),
        )
    };
    std::fs::remove_file(&target_path).map_err(|e| format!("remove phase file: {e}"))?;
    let _ = std::fs::remove_file(&gate_path);
    let phases_dir = root.join("phases");
    let mut after: Vec<(u32, String)> = existing
        .into_iter()
        .filter(|(n, _)| *n > phase)
        .collect();
    after.sort_by_key(|(n, _)| *n);
    for (num, slug) in &after {
        rename_phase_file(&phases_dir, slug, num - 1)?;
    }
    rebuild_plan_json(&root)?;
    let snapshot = {
        let mut w = cell.write();
        rebuild_from_disk(&mut w)?;
        w.clone()
    };
    emit_changed(&app, &id);
    Ok(snapshot)
}

/// Generate a `plan.json` from the existing phase markdown files. Used
/// to upgrade a v1 workspace (no plan.json on disk) to the v2 schema
/// in-place. `acceptance` is empty for every phase — the agent fills
/// these in on the next plan rewrite (phase 2 of the v2 roadmap).
#[tauri::command]
pub async fn sdd_upgrade_workspace(
    app: AppHandle,
    registry: State<'_, SddRegistry>,
    id: String,
) -> Result<SddWorkspace, String> {
    let cell = registry
        .workspaces
        .read()
        .get(&id)
        .cloned()
        .ok_or_else(|| format!("unknown workspace {id}"))?;
    let root = PathBuf::from(cell.read().root.clone());
    rebuild_plan_json(&root)?;
    let snapshot = {
        let mut w = cell.write();
        rebuild_from_disk(&mut w)?;
        w.clone()
    };
    emit_changed(&app, &id);
    Ok(snapshot)
}
