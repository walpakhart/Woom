//! Three-call phase artifact I/O — extracted from `sdd.rs` in wave-1
//! phase-10 refactor. Owns:
//!
//!   - `VerifyOutput` — the structured verdict the verify pass writes
//!     as `phases/<slug>/verify.json`. Parse-tolerant + with a
//!     deterministic fallback so a malformed payload never crashes
//!     downstream rendering.
//!   - Read/write helpers for `phases/<slug>/plan.md` and
//!     `phases/<slug>/verify.json` — atomic via `.tmp` + rename so a
//!     reader can never observe a half-written file.
//!
//! These artifacts are the per-phase contract between the three
//! agent passes (plan / implement / verify). Centralising the I/O
//! keeps the path layout in one spot — any future relocation of
//! the `phases/<slug>/…` tree touches only this module.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// Structured verify-pass result. Schema matches the JSON the agent
/// emits per the verify-pass prompt (`sdd_prompts/phase_verify.md`,
/// FR-4 of `spec-1`). Every field has `#[serde(default)]` so a
/// malformed payload still produces a usable struct — `parse_or_fallback`
/// masks parse failures behind a fixed `deviations: ["Unable to parse
/// verification output"]` row so downstream code never sees `null`.
#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct VerifyOutput {
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub files_changed: Vec<String>,
    #[serde(default)]
    pub task_compliance: Vec<String>,
    #[serde(default)]
    pub deviations: Vec<String>,
    #[serde(default)]
    pub notes: String,
}

impl VerifyOutput {
    /// Parse the raw agent response into a `VerifyOutput`. Tolerates
    /// markdown code-fences around the JSON (the verify-pass prompt
    /// asks for raw JSON but agents sometimes wrap anyway) and a few
    /// common stripping cases. On hard parse failure returns a
    /// fallback with `deviations = ["Unable to parse verification
    /// output"]` so callers can persist a deterministic shape and
    /// the failure card has something to render. See `spec-1`
    /// NFR-rel-2.
    pub fn parse_or_fallback(raw: &str) -> Self {
        let stripped = strip_json_fences(raw);
        serde_json::from_str(&stripped).unwrap_or_else(|_| Self {
            deviations: vec!["Unable to parse verification output".into()],
            ..Default::default()
        })
    }
}

/// Trim leading/trailing whitespace plus an optional surrounding
/// triple-backtick fence (with or without a `json` language tag).
/// Idempotent on already-clean payloads.
pub fn strip_json_fences(raw: &str) -> String {
    let trimmed = raw.trim();
    let without_open = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .map(|s| s.trim_start())
        .unwrap_or(trimmed);
    let without_close = without_open.strip_suffix("```").unwrap_or(without_open);
    without_close.trim().to_string()
}

/// Read `<workspace>/phases/<slug>/plan.md`. `None` when the file is
/// missing — interpreted by callers as "plan pass hasn't landed yet"
/// (legacy single-call or three-call still in flight).
#[allow(dead_code)] // surfaced via SddCard Plan tab in phase 4
pub(crate) fn read_phase_plan_md(workspace_root: &Path, slug: &str) -> Option<String> {
    let path = workspace_root.join("phases").join(slug).join("plan.md");
    std::fs::read_to_string(&path).ok()
}

/// Atomically write the plan-pass output as `phases/<slug>/plan.md`.
/// Creates the per-phase directory lazily. Body is stored verbatim
/// (markdown, no frontmatter) — `spec-1` FR-2 captures the agent's
/// last assistant message as-is so the user sees the exact text the
/// implement pass will consume.
pub(crate) fn write_phase_plan_md(
    workspace_root: &Path,
    slug: &str,
    body: &str,
) -> Result<(), String> {
    let dir = workspace_root.join("phases").join(slug);
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir phase dir: {e}"))?;
    let path = dir.join("plan.md");
    let tmp = path.with_extension("md.tmp");
    std::fs::write(&tmp, body).map_err(|e| format!("write plan.md tmp: {e}"))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("rename plan.md: {e}"))?;
    Ok(())
}

/// Read `<workspace>/phases/<slug>/verify.json`. `None` when missing
/// (verify pass hasn't completed) OR when JSON parse fails (returning
/// `None` keeps callers honest — they should treat that as "no soft
/// verdict yet" rather than synthesising one). See `spec-1` FR-4.
#[allow(dead_code)] // surfaced via SddCard Verify tab in phase 4
pub(crate) fn read_verify_json(workspace_root: &Path, slug: &str) -> Option<VerifyOutput> {
    let path = workspace_root.join("phases").join(slug).join("verify.json");
    let raw = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&raw).ok()
}

/// Atomically write the verify-pass JSON. Pretty-printed for human
/// readability (the file is read by both the SddCard renderer and
/// the user inspecting on disk).
pub(crate) fn write_verify_json(
    workspace_root: &Path,
    slug: &str,
    verdict: &VerifyOutput,
) -> Result<(), String> {
    let dir = workspace_root.join("phases").join(slug);
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir phase dir: {e}"))?;
    let path = dir.join("verify.json");
    let body = serde_json::to_string_pretty(verdict)
        .map_err(|e| format!("serialize verify.json: {e}"))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, body).map_err(|e| format!("write verify.json tmp: {e}"))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("rename verify.json: {e}"))?;
    Ok(())
}
