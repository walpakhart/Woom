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
