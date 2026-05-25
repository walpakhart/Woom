//! Plan-as-data — v2 schema for `<workspace>/plan.json`. Extracted
//! from `sdd.rs` in wave-1 phase-10 refactor.
//!
//! Markdown (`plan.md`) stays the human-readable view; JSON is the
//! structured source of truth for verifier acceptance criteria +
//! plan-editor mutations. Reads tolerate missing or parse-broken
//! files (returns None) so a corrupted plan.json doesn't poison
//! workspace state — the agent or user rewrites a healthy version
//! on the next turn.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// Top-level shape of `<workspace>/plan.json`. `version` is bumped if
/// the schema ever changes incompatibly; `phases` mirrors the
/// `phases/NN-*.md` files but adds metadata the markdown can't carry
/// (acceptance criteria, complexity hint).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SddPlanFile {
    pub version: u32,
    pub phases: Vec<SddPlanPhase>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SddPlanPhase {
    pub number: u32,
    pub slug: String,
    pub title: String,
    #[serde(default)]
    pub depends_on: Vec<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub complexity: Option<String>,
    /// Verifier criteria. Empty in phase 1 of the v2 roadmap; populated
    /// by phase 2 of the roadmap when the agent learns to write them.
    #[serde(default)]
    pub acceptance: Vec<SddPhaseAcceptance>,
}

/// Verifier-check spec. `serde(tag = "type")` matches the JSON shape
/// described in the SDD spec (`{ "type": "shell", "cmd": "…" }`).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SddPhaseAcceptance {
    Shell {
        cmd: String,
        #[serde(default)]
        expect_exit: i32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        stdout_match: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timeout_ms: Option<u64>,
    },
    FileExists {
        paths: Vec<String>,
    },
    Manual {
        description: String,
    },
}

/// Read `<workspace>/plan.json`. `None` for legacy workspaces (file
/// absent) AND for files that exist but parse-fail — we tolerate the
/// latter rather than poison the workspace state. The agent (or user
/// via the inline editor) will rewrite a healthy version on the next
/// turn.
pub(crate) fn read_plan_json(root: &Path) -> Option<SddPlanFile> {
    let path = root.join("plan.json");
    let raw = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&raw).ok()
}

/// Write `<workspace>/plan.json`. Atomic via `.tmp` + rename so a
/// concurrent reader can never observe a torn file. Uses the same
/// pattern as `set_status_on` on the markdown side.
#[allow(dead_code)] // wired in phase 2 of the v2 plan-editor roadmap
pub(crate) fn write_plan_json(root: &Path, plan: &SddPlanFile) -> Result<(), String> {
    let path = root.join("plan.json");
    let content = serde_json::to_string_pretty(plan)
        .map_err(|e| format!("serialize plan.json: {e}"))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, content).map_err(|e| format!("write plan.json.tmp: {e}"))?;
    std::fs::rename(&tmp, &path)
        .map_err(|e| format!("rename plan.json.tmp -> plan.json: {e}"))?;
    Ok(())
}
