//! SDD prompt templates — embedded markdown blobs the orchestrator
//! sends to the agent for each workflow stage. Extracted from
//! `sdd.rs` in wave-1 phase-10 refactor so the main file stays focused
//! on state + commands. Templates live next to the Rust enum that
//! references them so adding a new stage = one file edit + one match
//! arm, not "hunt around the 5KLoC god file".

use serde::{Deserialize, Serialize};

const SPEC_TEMPLATE_PROMPT: &str = include_str!("./sdd_prompts/spec.md");
const PLAN_TEMPLATE_PROMPT: &str = include_str!("./sdd_prompts/plan.md");
const PHASE_TEMPLATE_PROMPT: &str = include_str!("./sdd_prompts/phase.md");
const SUMMARY_TEMPLATE_PROMPT: &str = include_str!("./sdd_prompts/summary.md");
const AMEND_TEMPLATE_PROMPT: &str = include_str!("./sdd_prompts/amend.md");
/// Three-call mode plan pass — read-only analysis producing a plan
/// markdown body. See `spec-1` FR-1 / `plan-1` §Prompt + agent flow.
const PHASE_PLAN_TEMPLATE_PROMPT: &str = include_str!("./sdd_prompts/phase_plan.md");
/// Three-call mode implement pass — executes the plan with edits.
const PHASE_IMPLEMENT_TEMPLATE_PROMPT: &str = include_str!("./sdd_prompts/phase_implement.md");
/// Three-call mode verify pass — produces structured JSON verdict.
const PHASE_VERIFY_TEMPLATE_PROMPT: &str = include_str!("./sdd_prompts/phase_verify.md");

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SddPromptKind {
    Spec,
    Plan,
    Phase,
    Summary,
    /// In-place correction of the active spec / plan / phase. Used
    /// when the user types a delta mid-workflow instead of approving
    /// the current artifact — the agent edits files in place rather
    /// than scaffolding a fresh workspace.
    Amend,
    /// Three-call mode — plan pass (read-only analysis producing
    /// `phases/<slug>/plan.md`). See `spec-1` FR-1.
    PhasePlan,
    /// Three-call mode — implement pass (executes the plan against
    /// the repo). See `spec-1` FR-3.
    PhaseImplement,
    /// Three-call mode — verify pass (structured JSON self-review).
    /// See `spec-1` FR-4.
    PhaseVerify,
}

/// Return the canned prompt template the orchestrator should send to
/// the agent for a given stage. The orchestrator interpolates
/// `{{workspace_root}}` and `{{user_prompt}}` placeholders before
/// sending.
#[tauri::command]
pub async fn sdd_prompt(kind: SddPromptKind) -> Result<String, String> {
    let s = match kind {
        SddPromptKind::Spec => SPEC_TEMPLATE_PROMPT,
        SddPromptKind::Plan => PLAN_TEMPLATE_PROMPT,
        SddPromptKind::Phase => PHASE_TEMPLATE_PROMPT,
        SddPromptKind::Summary => SUMMARY_TEMPLATE_PROMPT,
        SddPromptKind::Amend => AMEND_TEMPLATE_PROMPT,
        SddPromptKind::PhasePlan => PHASE_PLAN_TEMPLATE_PROMPT,
        SddPromptKind::PhaseImplement => PHASE_IMPLEMENT_TEMPLATE_PROMPT,
        SddPromptKind::PhaseVerify => PHASE_VERIFY_TEMPLATE_PROMPT,
    };
    Ok(s.to_string())
}
