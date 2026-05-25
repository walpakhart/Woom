//! Three-call phase execution config — extracted from `sdd.rs` in
//! wave-1 phase-10 refactor. The config lives in
//! `<workspace>/meta.json#phase_execution`; this module owns its
//! shape + serde defaults + validation. Pure types — no Tauri, no
//! filesystem.

use serde::{Deserialize, Serialize};

/// Per-workspace toggle for the three-call execution mode (plan →
/// implement → verify) introduced in `spec-1`. `single_call` is the
/// historical Woom behaviour where each phase fires one agent turn;
/// `three_call` fans the phase out into three discrete passes with
/// structured verify output. Migration hook in `sdd_hydrate` sets
/// legacy workspaces to `SingleCall` so in-flight workflows don't
/// shift behaviour mid-execution.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PhaseExecutionMode {
    SingleCall,
    ThreeCall,
}

impl Default for PhaseExecutionMode {
    fn default() -> Self {
        // Legacy workspaces lacking the `phase_execution` block
        // deserialize with this default — the migration hook
        // explicitly writes the same value to disk so future reads
        // are deterministic. See `plan-1` §Compatibility + migration.
        Self::SingleCall
    }
}

pub fn default_plan_budget_pct() -> f32 {
    0.25
}
pub fn default_implement_budget_pct() -> f32 {
    0.70
}
pub fn default_verify_budget_pct() -> f32 {
    0.05
}

/// `<workspace>/meta.json#phase_execution` — orchestrator config for
/// the three-call phase execution mode. Every field has a `serde`
/// default so a meta.json lacking the block (legacy workspace) still
/// deserializes cleanly via `#[serde(default)]` on the parent field.
/// Budget percentages are informational this release — persisted now
/// so a future budget-enforcement spec can consume them without a
/// schema migration. See `spec-1` FR-11 / FR-12.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PhaseExecutionConfig {
    #[serde(default)]
    pub mode: PhaseExecutionMode,
    /// When true, three-call mode halts between `PhasePlanning` and
    /// `PhaseImplementing` so the user can Approve / Amend / Discard
    /// the generated plan.md. Default false — auto-advance to
    /// implement when the plan-pass finishes. See `spec-1` FR-7.
    #[serde(default)]
    pub plan_gate: bool,
    /// Share of the per-phase budget reserved for the plan-pass call.
    /// Sum of `plan_budget_pct + implement_budget_pct +
    /// verify_budget_pct` MUST be ≤ 1.0 (enforced by `validate()`).
    #[serde(default = "default_plan_budget_pct")]
    pub plan_budget_pct: f32,
    #[serde(default = "default_implement_budget_pct")]
    pub implement_budget_pct: f32,
    #[serde(default = "default_verify_budget_pct")]
    pub verify_budget_pct: f32,
}

impl Default for PhaseExecutionConfig {
    fn default() -> Self {
        Self {
            mode: PhaseExecutionMode::default(),
            plan_gate: false,
            plan_budget_pct: default_plan_budget_pct(),
            implement_budget_pct: default_implement_budget_pct(),
            verify_budget_pct: default_verify_budget_pct(),
        }
    }
}

impl PhaseExecutionConfig {
    /// Reject configs whose budget percentages sum past 1.0. The
    /// `sdd_set_phase_execution_config` Tauri command (lands in
    /// phase 6) calls this before persisting to surface a typed
    /// error instead of writing garbage that later math relies on.
    /// Per-percentage clamping (e.g. negative values) is not enforced
    /// — `f32` is the wire type and we trust validators on the UI
    /// side to keep inputs in `[0.0, 1.0]`. See `spec-1` FR-12.
    #[allow(dead_code)] // wired by sdd_set_phase_execution_config in phase 6
    pub fn validate(&self) -> Result<(), String> {
        let sum = self.plan_budget_pct + self.implement_budget_pct + self.verify_budget_pct;
        // Allow a small float slop so 0.25 + 0.70 + 0.05 = 1.0 + ε
        // doesn't false-positive.
        if sum > 1.0 + f32::EPSILON * 4.0 {
            return Err(format!(
                "budget percentages exceed 1.0 (got {sum:.3})"
            ));
        }
        Ok(())
    }
}
