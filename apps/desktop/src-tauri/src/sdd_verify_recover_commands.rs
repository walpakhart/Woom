//! Verification + git rollback + crash-recovery Tauri commands.
//! Extracted from `sdd.rs` in wave-25.
//!
//! `sdd_run_verification` evaluates the per-phase acceptance list and
//! flips the phase frontmatter (`done`/`failed`/stays-running for
//! manual checks). On a `done` flip it also drops a post-phase git
//! commit so the SddCard's "Files changed" drawer has something to
//! diff against.
//!
//! `sdd_rollback_phase` does the symmetric inverse — safety-stash +
//! `git reset --hard` back to the recorded `pre_phase_sha`, then
//! flips the phase to `pending` so the next approve cycle starts
//! clean. `sdd_recover_workspace` is the crash-recovery wrapper that
//! either calls rollback or marks the orphan as `failed` for manual
//! triage.

use std::path::PathBuf;

use tauri::{AppHandle, State};

use crate::sdd::{SddRecoveryState, SddRegistry, SddWorkspace};
use crate::sdd_hydrate::rebuild_from_disk;
use crate::sdd_md_mutators::set_status_on;
use crate::sdd_meta::{read_phase_meta, read_workspace_meta, write_phase_meta};
use crate::sdd_plan_helpers::set_status_with_extras;
use crate::sdd_watcher::emit_changed;

/// Run acceptance checks for `phase` and persist the result file.
///
/// Verdict mapping (matches the spec's "trigger flip" rules):
///   - `Passed` (incl. empty acceptance list) → phase status flips to
///     `done`. Subsequent gate-derivation moves the workflow forward.
///   - `Failed` → phase status flips to `failed`. Orchestrator surfaces
///     the failure via `derive_stage`'s `Failed` arm.
///   - `ManualPending` → phase status STAYS `running` so the SddCard
///     keeps the user's attention until they mark the manuals via
///     `sdd_mark_manual_check`.
#[tauri::command]
pub async fn sdd_run_verification(
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
    let file = crate::sdd_verify::verify_phase(&root, phase).await?;
    /* Flip phase frontmatter based on verdict. We compute the new
     *  status string under a closure so the borrow checker stays
     *  happy; no async work happens between read and write. */
    let phases_dir = root.join("phases");
    let target_status: Option<&str> = match file.overall_status {
        crate::sdd_verify::OverallStatus::Passed => Some("done"),
        crate::sdd_verify::OverallStatus::Failed => Some("failed"),
        crate::sdd_verify::OverallStatus::ManualPending => None,
    };
    if let Some(new_status) = target_status {
        /* Find the phase file by number in the (now-cached) workspace
         *  snapshot so we don't have to re-glob. */
        let phase_info = {
            let w = cell.read();
            w.phases
                .iter()
                .find(|p| p.number == phase)
                .map(|p| (p.slug.clone(), p.title.clone()))
        };
        if let Some((slug, title)) = phase_info {
            let phase_path = phases_dir.join(format!("{slug}.md"));
            set_status_on(&phase_path, new_status)?;
            /* Post-phase git commit on a successful flip to `done`.
             *  Failures are logged + recorded as `snapshot_skipped`
             *  but never block the status flip — the user's intent
             *  to advance trumps git plumbing. */
            if new_status == "done" {
                let meta = read_workspace_meta(&root);
                if meta.git_enabled {
                    if let Some(repo) = meta.repo_cwd.as_deref() {
                        let body = post_phase_commit_body(&file);
                        let msg = format!("phase-{phase}: {title}\n\n{body}");
                        match crate::git::commit_all_allow_empty(repo, &msg) {
                            Ok(sha) => {
                                let mut pm = read_phase_meta(&root, phase);
                                pm.post_phase_sha = Some(sha);
                                let _ = write_phase_meta(&root, phase, &pm);
                            }
                            Err(e) => {
                                eprintln!(
                                    "[sdd] workspace={id} phase={phase}: post-phase commit failed: {e}"
                                );
                            }
                        }
                    }
                }
            }
        }
    }
    let snapshot = {
        let mut w = cell.write();
        rebuild_from_disk(&mut w)?;
        w.clone()
    };
    emit_changed(&app, &id);
    Ok(snapshot)
}

/// Compose the body of a `phase-N: <slug>` commit message from the
/// verifier's aggregate output. Plain-text, two lines: a one-line
/// "Verified by N checks (M passed, K manual)" summary followed by
/// per-check status lines for the audit trail.
fn post_phase_commit_body(file: &crate::sdd_verify::PhaseAcceptanceFile) -> String {
    use crate::sdd_verify::CheckStatus;
    let total = file.results.len();
    let passed = file
        .results
        .iter()
        .filter(|r| matches!(r.status, CheckStatus::Passed))
        .count();
    // "Manual" here = original kind == "manual", regardless of verdict —
    // tells reviewers how many of the green check-marks were human-
    // approved vs autonomous.
    let manual = file.results.iter().filter(|r| r.kind == "manual").count();
    let mut s = format!("Verified by {total} checks ({passed} passed, {manual} manual)\n");
    for r in &file.results {
        let mark = match r.status {
            CheckStatus::Passed => "✓",
            CheckStatus::Failed => "✗",
            CheckStatus::ManualUnmarked => "?",
            CheckStatus::Skipped => "·",
            CheckStatus::Pending => "…",
        };
        s.push_str(&format!("- {mark} {}\n", r.kind));
    }
    s
}

/// Resolve a manual acceptance check by user verdict. Recomputes the
/// aggregate verdict + (if it now flips to `Passed` or `Failed`) the
/// phase frontmatter, just like `sdd_run_verification` does at first
/// run. Idempotent — a second call with the same args is a no-op
/// past the JSON write.
#[tauri::command]
pub async fn sdd_mark_manual_check(
    app: AppHandle,
    registry: State<'_, SddRegistry>,
    id: String,
    phase: u32,
    check_index: usize,
    passed: bool,
) -> Result<SddWorkspace, String> {
    let cell = registry
        .workspaces
        .read()
        .get(&id)
        .cloned()
        .ok_or_else(|| format!("unknown workspace {id}"))?;
    let root = PathBuf::from(cell.read().root.clone());
    let file = crate::sdd_verify::mark_manual_check(&root, phase, check_index, passed)?;
    let target_status: Option<&str> = match file.overall_status {
        crate::sdd_verify::OverallStatus::Passed => Some("done"),
        crate::sdd_verify::OverallStatus::Failed => Some("failed"),
        crate::sdd_verify::OverallStatus::ManualPending => None,
    };
    if let Some(new_status) = target_status {
        let phase_slug = {
            let w = cell.read();
            w.phases
                .iter()
                .find(|p| p.number == phase)
                .map(|p| p.slug.clone())
        };
        if let Some(slug) = phase_slug {
            let phase_path = root.join("phases").join(format!("{slug}.md"));
            set_status_on(&phase_path, new_status)?;
        }
    }
    let snapshot = {
        let mut w = cell.write();
        rebuild_from_disk(&mut w)?;
        w.clone()
    };
    emit_changed(&app, &id);
    Ok(snapshot)
}

/// Roll a phase back to its `pre_phase_sha` snapshot. The user's
/// current changes are first safety-stashed under
/// `sdd-rollback-safety-<phase>-<id>`; then `git reset --hard` jumps
/// to the snapshot; then the phase frontmatter resets to `pending` and
/// any per-phase acceptance JSON is wiped so the next approve cycle
/// starts clean. Returns the refreshed workspace snapshot.
///
/// Errors when:
///   - workspace is not git-enabled (no snapshot to roll back to);
///   - the phase has no recorded `pre_phase_sha` (snapshot was skipped);
///   - the phase is currently `running` (must pause / stop first).
#[tauri::command]
pub async fn sdd_rollback_phase(
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
    let meta = read_workspace_meta(&root);
    if !meta.git_enabled {
        return Err("workspace is not git-enabled — no rollback available".into());
    }
    let repo = meta
        .repo_cwd
        .as_deref()
        .ok_or_else(|| "workspace meta missing repo_cwd".to_string())?;
    let phase_meta = read_phase_meta(&root, phase);
    let pre_sha = phase_meta
        .pre_phase_sha
        .as_deref()
        .ok_or_else(|| format!("phase {phase} has no pre_phase_sha — snapshot was skipped"))?;
    /* Pull phase slug + sanity-check status before destructive ops.
     *  We refuse to roll a `running` phase — the user must pause /
     *  stop first so we don't reset under a live agent. */
    let (phase_path, current_status) = {
        let w = cell.read();
        let p = w
            .phases
            .iter()
            .find(|p| p.number == phase)
            .ok_or_else(|| format!("phase {phase} not found"))?;
        (PathBuf::from(&p.path), p.status.clone())
    };
    if current_status == "running" {
        return Err(format!(
            "phase {phase} is running — pause or stop the workflow before rolling back"
        ));
    }
    let safety_label = format!("sdd-rollback-safety-{}-{}", phase, id);
    let _ = crate::git::stash_with_label(repo, &safety_label);
    crate::git::reset_hard(repo, pre_sha).map_err(|e| format!("reset --hard: {e}"))?;
    set_status_on(&phase_path, "pending")?;
    /* Wipe per-phase acceptance results + post_phase_sha so the next
     *  approve cycle starts from zero. We keep `pre_phase_sha` so the
     *  user can roll forward / back repeatedly without losing the
     *  rollback target. */
    let acceptance_path = root.join("results").join(format!("phase-{phase}-acceptance.json"));
    let _ = std::fs::remove_file(acceptance_path);
    let mut pm = phase_meta;
    pm.post_phase_sha = None;
    let _ = write_phase_meta(&root, phase, &pm);
    /* Drop the `phase-N-approved` gate too, so the user re-approves
     *  before the orchestrator advances. */
    let _ = std::fs::remove_file(root.join("control").join(format!("phase-{phase}-approved")));
    let snapshot = {
        let mut w = cell.write();
        rebuild_from_disk(&mut w)?;
        w.clone()
    };
    emit_changed(&app, &id);
    Ok(snapshot)
}

/// Resolve a detected crash-recovery situation. `action`:
///   - `"rollback"` — same effect as `sdd_rollback_phase` on the
///     orphan phase (fast-path so the UI doesn't have to look up
///     the phase number itself).
///   - `"keep"` — flip the orphan phase from `running`/`verifying`
///     to `failed` so the user can decide whether to retry / skip
///     manually. No git side-effects.
#[tauri::command]
pub async fn sdd_recover_workspace(
    app: AppHandle,
    registry: State<'_, SddRegistry>,
    id: String,
    action: String,
) -> Result<SddWorkspace, String> {
    let cell = registry
        .workspaces
        .read()
        .get(&id)
        .cloned()
        .ok_or_else(|| format!("unknown workspace {id}"))?;
    /* Snapshot the orphan phase under the read lock so we don't hold
     *  it across async work. */
    let orphan = {
        let w = cell.read();
        match w.recovery_state.as_ref() {
            Some(SddRecoveryState::OrphanPhase { phase, .. }) => Some(*phase),
            None => None,
        }
    };
    let phase = orphan.ok_or_else(|| "no recovery_state on workspace".to_string())?;
    match action.as_str() {
        "rollback" => sdd_rollback_phase(app, registry, id, phase).await,
        "keep" => {
            let path: PathBuf = {
                let w = cell.read();
                w.phases
                    .iter()
                    .find(|p| p.number == phase)
                    .map(|p| PathBuf::from(&p.path))
                    .ok_or_else(|| format!("phase {phase} not found"))?
            };
            set_status_with_extras(
                &path,
                "failed",
                vec![("recovery_action".into(), "keep".into())],
            )?;
            let snapshot = {
                let mut w = cell.write();
                rebuild_from_disk(&mut w)?;
                w.clone()
            };
            emit_changed(&app, &id);
            Ok(snapshot)
        }
        other => Err(format!(
            "unknown recovery action `{other}` (expected `rollback` or `keep`)"
        )),
    }
}
