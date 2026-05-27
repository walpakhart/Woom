//! Workspace hydration helpers — pure functions that walk a workspace
//! directory and rebuild the typed `SddWorkspace` snapshot from
//! whatever's on disk. Extracted from `sdd.rs` in wave-6 split.
//!
//! `derive_stage` is the heart of the orchestrator — it consumes the
//! reconstructed workspace + the spec/plan frontmatter status fields
//! and produces the `SddStage` discriminant that drives every UI
//! decision (which card to show, which buttons to enable, whether to
//! auto-fire the next phase prompt). Kept on its own so the table
//! of transitions is easy to find + the unit tests can hit it without
//! standing up a full registry.

use std::path::{Path, PathBuf};

use crate::sdd::{FailureTrigger, SddPhase, SddRecoveryState, SddStage, SddWorkspace};
use crate::sdd_action_log::ActionLogEntry;
use crate::sdd_frontmatter::{parse_frontmatter, FrontMatter};
use crate::sdd_meta::{read_phase_meta, SddMeta};
use crate::sdd_phase_config::PhaseExecutionMode;
use crate::sdd_phase_io::{read_phase_plan_md, read_verify_json};
use crate::sdd_substep::{read_substep_state, SddPhaseSubstep};
use crate::sdd_time::now_ms;

/// Phase number derivation — supports `phase: 1` and
/// `phase: "01-foundation"` and falls back to filename prefix like
/// `01-foundation.md`.
pub(crate) fn phase_number_from(slug: &str, fm: &FrontMatter) -> u32 {
    if let Some(v) = &fm.phase {
        if let Some(n) = v.as_u64() {
            return n as u32;
        }
        if let Some(s) = v.as_str() {
            if let Some(num) = s.split('-').next().and_then(|p| p.parse::<u32>().ok()) {
                return num;
            }
        }
    }
    slug.split('-').next().and_then(|p| p.parse::<u32>().ok()).unwrap_or(0)
}

/// Pull the indices of acceptance checks that ended in `Failed` for the
/// given phase, by reading `<workspace>/results/phase-N-acceptance.json`
/// (written by the verifier in phase 2). Returns empty when the file
/// is absent or all checks passed — both are valid "no failed checks
/// to surface" states.
pub(crate) fn read_failed_check_indices(workspace_root: &Path, phase: u32) -> Vec<u32> {
    let path = workspace_root
        .join("results")
        .join(format!("phase-{phase}-acceptance.json"));
    let raw = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let parsed: Result<crate::sdd_verify::PhaseAcceptanceFile, _> = serde_json::from_str(&raw);
    let Ok(file) = parsed else {
        return Vec::new();
    };
    file.results
        .iter()
        .enumerate()
        .filter_map(|(i, r)| {
            matches!(r.status, crate::sdd_verify::CheckStatus::Failed).then_some(i as u32)
        })
        .collect()
}

/// Read the last `tail` ActionLogEntry rows from
/// `<workspace>/phases/phase-N.log.jsonl`. Used by `derive_stage` to
/// pre-populate the failure card's "last actions" tail without forcing
/// the frontend to make a second IPC call. Failures here are silent
/// (returns empty vec) — surfacing them on disk-read errors would mask
/// the actual failure trigger.
pub(crate) fn read_action_log_tail(
    workspace_root: &Path,
    phase: u32,
    tail: usize,
) -> Vec<ActionLogEntry> {
    let path = workspace_root
        .join("phases")
        .join(format!("phase-{phase}.log.jsonl"));
    let raw = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let mut all: Vec<ActionLogEntry> = raw
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<ActionLogEntry>(l).ok())
        .collect();
    if all.len() > tail {
        let drop = all.len() - tail;
        all.drain(..drop);
    }
    all
}

/// Pure function — stage from files. Kept separate so it's easy to
/// unit-test. Inputs:
///   - `w`: the reconstructed workspace snapshot (every other field
///     already populated by `rebuild_from_disk`)
///   - `spec_status` / `plan_status`: the frontmatter `status:` values
///     pulled off spec.md / plan.md during rebuild — `None` when the
///     file doesn't exist or has no frontmatter
///
/// Returns the SddStage variant that drives every UI decision (which
/// card to show, which buttons to enable, whether to auto-fire the
/// next phase prompt). Honours filesystem-backed pause/stop signals,
/// three-call sub-step checkpoints, v2 per-phase approval gates, and
/// the structured failure payload (failed_checks + action_log_tail +
/// trigger inference) used by the failure card.
pub(crate) fn derive_stage(
    w: &SddWorkspace,
    spec_status: Option<String>,
    plan_status: Option<String>,
) -> SddStage {
    /* Filesystem-backed pause/stop signals — same convention as
     *  brain's orchestrator. The card writes `control/stop` /
     *  `control/pause` on user click, which lets state survive an
     *  app restart (in-memory `w.stage` is reset to Drafting on
     *  hydrate, so we have to recover the user's intent from disk). */
    let root = PathBuf::from(&w.root);
    if root.join("control/stop").exists() {
        return SddStage::Stopped;
    }
    if root.join("control/pause").exists() {
        return SddStage::Paused;
    }
    // Honour in-memory overrides too (the control file may not have
    // landed yet between flip_stage + the immediate rebuild).
    if matches!(w.stage, SddStage::Paused | SddStage::Stopped) {
        return w.stage.clone();
    }
    /* Sticky `Failed` — but only when the failure still exists on
     * disk. The previous unconditional return masked the case where
     * a retry / fix-reset had already flipped the failed phase off
     * `failed` (status now `pending` / `running` / `done`), but the
     * cached stage held us hostage to the old Failed view. With the
     * disk check the cached stage is honoured only when at least one
     * phase is genuinely failed; otherwise we fall through and let
     * the rest of derive_stage compute the actual current stage. */
    if let SddStage::Failed { .. } = &w.stage {
        let still_failed = w.phases.iter().any(|p| p.status == "failed");
        if still_failed {
            return w.stage.clone();
        }
        /* Stale Failed in cached stage. Drop through. The matching
         * block lower in this function will pick up any phase that
         * still actually IS failed, or compute the appropriate
         * non-failed stage from the live phases array. */
    }
    // No spec yet -> Drafting.
    if w.spec_body.is_none() {
        return SddStage::Drafting;
    }
    let spec_approved = matches!(spec_status.as_deref(), Some("approved"));
    if !spec_approved {
        return SddStage::SpecReady;
    }
    // Spec approved — agent should now be working on the plan.
    if w.plan_body.is_none() || w.phases.is_empty() {
        return SddStage::Planning;
    }
    let plan_approved = matches!(plan_status.as_deref(), Some("approved"));
    if !plan_approved {
        return SddStage::PlanReady;
    }
    // Plan approved — check for failure first so a failed phase
    // doesn't get masked by a later "done" sibling. (Sequential exec
    // means this is the latest phase touched.)
    if let Some(p) = w.phases.iter().find(|p| p.status == "failed") {
        let root = PathBuf::from(&w.root);
        let failed_checks = read_failed_check_indices(&root, p.number);
        let action_log_tail = read_action_log_tail(&root, p.number, 10);
        let verify = read_verify_json(&root, &p.slug);
        let verify_deviated = verify
            .as_ref()
            .is_some_and(|v| !v.deviations.is_empty());
        // The phase markdown frontmatter may carry an explicit
        // `trigger:` set by `sdd_discard_phase_plan` (plan_discarded)
        // or future failure paths. Read it once + parse into the
        // typed enum; falls back to inferring from acceptance.json +
        // verify.json. Manual write-trigger wins over inferred.
        let explicit_trigger = std::fs::read_to_string(&p.path)
            .ok()
            .and_then(|raw| {
                let (fm, _) = parse_frontmatter(&raw);
                fm.trigger.clone()
            })
            .and_then(|s| serde_json::from_value::<FailureTrigger>(serde_json::Value::String(s)).ok());
        let trigger = if let Some(t) = explicit_trigger {
            Some(t)
        } else if !failed_checks.is_empty() {
            Some(FailureTrigger::CheckFailed)
        } else if verify_deviated {
            Some(FailureTrigger::VerifyFailed)
        } else {
            Some(FailureTrigger::Exception)
        };
        let reason = if verify_deviated && failed_checks.is_empty() {
            let first = verify
                .as_ref()
                .and_then(|v| v.deviations.first())
                .cloned()
                .unwrap_or_else(|| "verify pass reported deviations".into());
            format!("Phase {} ({}) — verify deviations: {first}", p.number, p.title)
        } else {
            format!("Phase {} ({}) failed — see result file", p.number, p.title)
        };
        return SddStage::Failed {
            reason,
            failed_phase: Some(p.number),
            trigger,
            failed_checks,
            action_log_tail,
        };
    }
    let running_phase = w.phases.iter().find(|p| p.status == "running");
    if let Some(p) = running_phase {
        if w.phase_execution.mode == PhaseExecutionMode::ThreeCall {
            let root = PathBuf::from(&w.root);
            if let Some(state) = read_substep_state(&root, p.number) {
                match state.sub_step {
                    Some(SddPhaseSubstep::Plan) => {
                        let plan_md_exists = root
                            .join("phases")
                            .join(&p.slug)
                            .join("plan.md")
                            .exists();
                        let plan_approved = root
                            .join(format!("control/phase-{}-plan-approved", p.number))
                            .exists();
                        if w.phase_execution.plan_gate && plan_md_exists && !plan_approved {
                            return SddStage::PhasePlanReview { phase: p.number };
                        }
                        return SddStage::PhasePlanning { phase: p.number };
                    }
                    Some(SddPhaseSubstep::Implement) => {
                        return SddStage::PhaseImplementing { phase: p.number };
                    }
                    Some(SddPhaseSubstep::Verify) => {
                        return SddStage::PhaseVerifying { phase: p.number };
                    }
                    None => {}
                }
            }
        }
        return SddStage::PhaseRunning { phase: p.number };
    }
    // `skipped` phases are treated like `done` for advancement purposes —
    // they don't block the next gate.
    let is_completed = |p: &SddPhase| p.status == "done" || p.status == "skipped";
    let all_done = !w.phases.is_empty() && w.phases.iter().all(is_completed);
    if all_done {
        return SddStage::Complete;
    }
    let last_done = w.phases.iter().rev().find(|p| is_completed(p));
    // V2 gate insertion. Before falling through to PlanReady / PhaseDone
    // (which the frontend uses as "auto-fire next phase prompt"), check
    // whether the next pending phase needs a per-phase approval.
    if w.is_v2 {
        let prev = last_done.map(|p| p.number).unwrap_or(0);
        let next = w
            .phases
            .iter()
            .find(|p| p.number > prev && p.status == "pending");
        if let Some(p) = next {
            let approved = Path::new(&w.root)
                .join(format!("control/phase-{}-approved", p.number))
                .exists();
            if !approved {
                return SddStage::PhasePendingApproval { phase: p.number };
            }
        }
    }
    if let Some(p) = last_done {
        return SddStage::PhaseDone { phase: p.number };
    }
    // No phase running, no phase done — execution hasn't started but plan
    // is approved. UI shows the "start phase 1" affordance under PlanReady.
    SddStage::PlanReady
}

/// Scan a workspace directory and rebuild the in-memory `SddWorkspace`
/// from disk. Idempotent — call this whenever we suspect the agent has
/// modified files. The stage is derived from which files exist + their
/// frontmatter statuses, so the agent doesn't have to keep a parallel
/// state machine in its head — it just writes files.
pub(crate) fn rebuild_from_disk(workspace: &mut SddWorkspace) -> Result<(), String> {
    let root = std::path::PathBuf::from(&workspace.root);
    workspace.updated_at = now_ms();

    // --- spec ---
    let spec_path = root.join("spec.md");
    let mut spec_status: Option<String> = None;
    if spec_path.exists() {
        let raw = std::fs::read_to_string(&spec_path)
            .map_err(|e| format!("read spec: {e}"))?;
        let (fm, body) = parse_frontmatter(&raw);
        spec_status = fm.status.clone();
        workspace.spec_path = Some(spec_path.to_string_lossy().into_owned());
        workspace.spec_body = Some(body);
    } else {
        workspace.spec_path = None;
        workspace.spec_body = None;
    }

    // --- plan ---
    let plan_path = root.join("plan.md");
    let mut plan_status: Option<String> = None;
    if plan_path.exists() {
        let raw = std::fs::read_to_string(&plan_path)
            .map_err(|e| format!("read plan: {e}"))?;
        let (fm, body) = parse_frontmatter(&raw);
        plan_status = fm.status.clone();
        workspace.plan_path = Some(plan_path.to_string_lossy().into_owned());
        workspace.plan_body = Some(body);
    } else {
        workspace.plan_path = None;
        workspace.plan_body = None;
    }

    // --- summary (post-completion wrap-up) ---
    let summary_path = root.join("SUMMARY.md");
    if summary_path.exists() {
        let raw = std::fs::read_to_string(&summary_path)
            .map_err(|e| format!("read summary: {e}"))?;
        let (_, body) = parse_frontmatter(&raw);
        workspace.summary_path = Some(summary_path.to_string_lossy().into_owned());
        workspace.summary_body = Some(body);
    } else {
        workspace.summary_path = None;
        workspace.summary_body = None;
    }

    // --- phases ---
    workspace.phases.clear();
    let phases_dir = root.join("phases");
    if phases_dir.is_dir() {
        let mut entries: Vec<std::path::PathBuf> = std::fs::read_dir(&phases_dir)
            .map_err(|e| format!("read phases: {e}"))?
            .filter_map(|r| r.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|e| e == "md"))
            .collect();
        entries.sort();
        let results_dir = root.join("results");
        for path in entries {
            let raw = std::fs::read_to_string(&path)
                .map_err(|e| format!("read phase {}: {e}", path.display()))?;
            let (fm, body) = parse_frontmatter(&raw);
            let slug = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("00-unknown")
                .to_string();
            let number = phase_number_from(&slug, &fm);
            // Result summary (best-effort)
            let summary = {
                let p = results_dir.join(format!("{slug}-result.md"));
                if p.exists() {
                    std::fs::read_to_string(&p).ok().map(|r| {
                        let (_, body) = parse_frontmatter(&r);
                        body
                    })
                } else {
                    None
                }
            };
            // Three-call mode artifacts — read each from disk, None
            // when absent. Single-call workspaces always end up with
            // both as None (no per-phase dir gets written).
            let plan_body = read_phase_plan_md(&root, &slug);
            let verify = read_verify_json(&root, &slug);
            workspace.phases.push(SddPhase {
                number,
                slug,
                title: fm.title.clone().unwrap_or_else(|| "Untitled phase".into()),
                depends_on: fm.depends_on.clone(),
                status: fm.status.clone().unwrap_or_else(|| "pending".into()),
                tasks_total: fm.tasks_total.unwrap_or(0),
                tasks_done: fm.tasks_completed.unwrap_or(0),
                body,
                path: path.to_string_lossy().into_owned(),
                summary,
                plan_body,
                verify,
            });
        }
        workspace.phases.sort_by_key(|p| p.number);
    }

    // --- v2 detection ---
    /* `plan.json` is the v2 marker. Its presence flips `is_v2` to true,
     * which `derive_stage` reads to decide whether to insert the
     * per-phase approval gate. Legacy workspaces (no plan.json) keep
     * the v1 auto-advance flow byte-for-byte. */
    workspace.is_v2 = root.join("plan.json").exists();

    // --- phase_execution mirror ---
    if let Ok(meta_raw) = std::fs::read_to_string(root.join("meta.json")) {
        if let Ok(meta) = serde_json::from_str::<SddMeta>(&meta_raw) {
            workspace.phase_execution = meta.phase_execution;
        }
    }

    // --- crash-recovery probe ---
    workspace.recovery_state = workspace
        .phases
        .iter()
        .find(|p| p.status == "running" || p.status == "verifying")
        .and_then(|p| {
            let meta = read_phase_meta(&root, p.number);
            if meta.post_phase_sha.is_some() {
                None
            } else {
                let sub_step = read_substep_state(&root, p.number)
                    .and_then(|s| s.sub_step);
                Some(SddRecoveryState::OrphanPhase {
                    phase: p.number,
                    pre_phase_sha: meta.pre_phase_sha.clone(),
                    sub_step,
                })
            }
        });

    // --- derive stage ---
    workspace.stage = derive_stage(workspace, spec_status, plan_status);

    Ok(())
}
