//! Git-state + per-phase diff Tauri commands. Extracted from `sdd.rs`
//! in wave-20. Thin wrappers around `crate::git::*` that read the
//! workspace + phase meta files to discover the right repo/SHAs, then
//! delegate to the pure git helpers. UI calls `sdd_get_git_state` once
//! on card mount, then `sdd_get_phase_diff` per completed phase for the
//! `Files changed` drawer; `sdd_get_file_diff` is lazy-fetched on row
//! expand.
//!
//! Skip-paths all converge on the same "snapshot was skipped" shape so
//! the UI never has to distinguish "git was off" from "working tree
//! was dirty at approve time" from "phase hasn't finished yet" — it
//! just renders a placeholder.

use std::path::PathBuf;

use tauri::State;

use crate::sdd::SddRegistry;
use crate::sdd_meta::{read_phase_meta, read_workspace_meta};

#[tauri::command]
pub async fn sdd_get_git_state(
    registry: State<'_, SddRegistry>,
    id: String,
) -> Result<crate::git::SddGitState, String> {
    let cell = registry
        .workspaces
        .read()
        .get(&id)
        .cloned()
        .ok_or_else(|| format!("unknown workspace {id}"))?;
    let root = PathBuf::from(cell.read().root.clone());
    let meta = read_workspace_meta(&root);
    if !meta.git_enabled {
        return Ok(crate::git::SddGitState {
            enabled: false,
            branch: None,
            on_sdd_branch: false,
            ahead: 0,
            behind: 0,
            dirty: false,
        });
    }
    let repo = meta.repo_cwd.unwrap_or_default();
    let sdd_branch = meta.sdd_branch.unwrap_or_default();
    Ok(crate::git::sdd_git_state(&repo, &sdd_branch))
}

/// Compute file-level diff stats between a phase's pre-phase and
/// post-phase commits. Used by the SddCard `Files changed` drawer.
///
/// Returns `skipped: true` when the workspace doesn't have git
/// integration, when either SHA is missing (e.g. snapshot was skipped
/// because the working tree was dirty at approve time, see phase 3),
/// OR when the phase hasn't completed yet (no `post_phase_sha`). The UI
/// renders a "git snapshot was skipped" placeholder for that branch
/// and skips the file-list render.
#[tauri::command]
pub async fn sdd_get_phase_diff(
    registry: State<'_, SddRegistry>,
    id: String,
    phase: u32,
) -> Result<crate::git::SddPhaseDiff, String> {
    let cell = registry
        .workspaces
        .read()
        .get(&id)
        .cloned()
        .ok_or_else(|| format!("unknown workspace {id}"))?;
    let root = PathBuf::from(cell.read().root.clone());
    let meta = read_workspace_meta(&root);
    let phase_meta = read_phase_meta(&root, phase);
    let repo = meta.repo_cwd.unwrap_or_default();
    let pre = phase_meta.pre_phase_sha.unwrap_or_default();
    let post = phase_meta.post_phase_sha.unwrap_or_default();
    /* Three skip-paths converge on the same `skipped: true` shape — the
     *  UI doesn't need to know whether git was off, the snapshot was
     *  skipped (dirty tree), or the phase hasn't finished yet. */
    if !meta.git_enabled || repo.is_empty() || pre.is_empty() || post.is_empty() {
        return Ok(crate::git::SddPhaseDiff {
            files: Vec::new(),
            total_insertions: 0,
            total_deletions: 0,
            skipped: true,
        });
    }
    crate::git::compute_phase_diff(&repo, &pre, &post)
}

/// Lazy fetch the unified-diff patch for a single file in a phase.
/// Called when the user clicks a row in the `Files changed` drawer to
/// expand its body. Returns the raw `git diff` output (the UI runs it
/// through the existing `renderDiffHtml` helper). Empty string when
/// pre/post SHAs are missing — the UI hides the row body in that case.
#[tauri::command]
pub async fn sdd_get_file_diff(
    registry: State<'_, SddRegistry>,
    id: String,
    phase: u32,
    path: String,
) -> Result<String, String> {
    let cell = registry
        .workspaces
        .read()
        .get(&id)
        .cloned()
        .ok_or_else(|| format!("unknown workspace {id}"))?;
    let root = PathBuf::from(cell.read().root.clone());
    let meta = read_workspace_meta(&root);
    let phase_meta = read_phase_meta(&root, phase);
    let repo = meta.repo_cwd.unwrap_or_default();
    let pre = phase_meta.pre_phase_sha.unwrap_or_default();
    let post = phase_meta.post_phase_sha.unwrap_or_default();
    if !meta.git_enabled || repo.is_empty() || pre.is_empty() || post.is_empty() {
        return Ok(String::new());
    }
    crate::git::compute_file_diff(&repo, &pre, &post, &path)
}
