//! Three-call mode sub-step types + checkpoint state — extracted from
//! `sdd.rs` in wave-1 phase-10 refactor.
//!
//! The three-call pipeline (plan → implement → verify) needs to remember
//! which pass it's currently in across an app crash. We persist this
//! at `<workspace>/control/phase-<N>-substep-state.json`; on hydrate
//! the orchestrator reads it back to label the recovery banner with
//! "Phase N (during implement)" rather than the bare phase number.

use serde::{Deserialize, Serialize};

/// Which sub-step of a three-call phase is currently in flight. Drives
/// the `SddStage::Phase{Planning,Implementing,Verifying}` emission in
/// `derive_stage` + the `sub_step` tag on action_log rows so the
/// SddCard can render per-substep dividers without re-walking the
/// JSONL. See `spec-1` FR-1 / FR-3 / FR-4.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SddPhaseSubstep {
    Plan,
    Implement,
    Verify,
}

/// Crash-recovery checkpoint for the three-call phase pipeline. Lives
/// at `<workspace>/control/phase-<N>-substep-state.json`, written
/// atomically (`.tmp` + rename) at every transition. On hydrate the
/// orchestrator reads it; if `sub_step` is `Some(_)` AND no phase
/// result has landed since, the recovery banner labels the orphan as
/// "Phase N (during <sub_step>)" so the user knows where the run
/// died. See `spec-1` NFR-rel-1.
#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct SddPhaseSubstepState {
    pub phase: u32,
    /// `None` means no sub-step is in flight (cleared at phase end or
    /// after `clear_substep_state`). `Some(sub)` means the agent is
    /// running OR was running when Woom crashed.
    #[serde(default)]
    pub sub_step: Option<SddPhaseSubstep>,
    /// Unix-ms when the current sub_step transition was written. Zero
    /// when the state was constructed via `Default` (no transition
    /// yet recorded).
    #[serde(default)]
    pub started_at: u64,
}

use std::path::Path;

/// Read `<workspace>/control/phase-<N>-substep-state.json`. `None`
/// when the file is missing (legacy / single-call workspace) OR the
/// JSON fails to parse (treat as missing — fail open so a corrupted
/// checkpoint doesn't permanently block recovery). The caller's
/// expected behaviour for `None` is "fall back to `PhaseRunning`".
pub(crate) fn read_substep_state(
    workspace_root: &Path,
    phase: u32,
) -> Option<SddPhaseSubstepState> {
    let path = workspace_root
        .join("control")
        .join(format!("phase-{phase}-substep-state.json"));
    let raw = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&raw).ok()
}

/// Atomically write the checkpoint. Same write-tmp + rename pattern
/// as `write_phase_meta` so readers never see a partial file (POSIX
/// `rename` is atomic on the same filesystem). Creates the
/// `<root>/control/` directory lazily.
#[allow(dead_code)] // wired by sdd_save_phase_plan / sdd_save_phase_verify in phase 3
pub(crate) fn write_substep_state(
    workspace_root: &Path,
    state: &SddPhaseSubstepState,
) -> Result<(), String> {
    let dir = workspace_root.join("control");
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir control: {e}"))?;
    let path = dir.join(format!("phase-{}-substep-state.json", state.phase));
    let body = serde_json::to_string_pretty(state)
        .map_err(|e| format!("serialize substep-state: {e}"))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, body).map_err(|e| format!("write substep-state tmp: {e}"))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("rename substep-state: {e}"))?;
    Ok(())
}

/// Remove the checkpoint file. Called at phase end (verify-pass done)
/// so a successful phase doesn't leave a stale checkpoint that would
/// re-trigger the recovery banner on next boot. Missing file is not
/// an error — idempotent semantics match `write_substep_state`'s
/// "fail open" reader.
#[allow(dead_code)] // wired by sdd_save_phase_verify in phase 3
pub(crate) fn clear_substep_state(workspace_root: &Path, phase: u32) -> Result<(), String> {
    let path = workspace_root
        .join("control")
        .join(format!("phase-{phase}-substep-state.json"));
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("remove substep-state: {e}")),
    }
}
