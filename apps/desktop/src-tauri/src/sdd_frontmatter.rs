//! YAML frontmatter parser + writer for SDD workspace files.
//!
//! Extracted from `sdd.rs` in wave-1 phase-10 refactor. SDD's
//! state-on-disk model leans on every spec / plan / phase file
//! carrying a `---\nyaml\n---\nbody` envelope: the orchestrator
//! reads `status` / `phase` / `tasks_*` keys off the frontmatter to
//! decide the current stage, and writes flip the values inside
//! existing frontmatter without disturbing the body. Keeping the
//! envelope split out makes the parser unit-testable in isolation
//! and the main `sdd.rs` smaller.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// Parsed frontmatter shape — every field optional + `#[serde(default)]`
/// so we accept partial / sloppy agent output and fall back to defaults
/// for missing keys (rather than 500-erroring the whole hydrate).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct FrontMatter {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub phase: Option<serde_yaml::Value>, // can be u32 or "01-foo"
    #[serde(default)]
    pub depends_on: Vec<u32>,
    #[serde(default)]
    pub tasks_total: Option<u32>,
    #[serde(default)]
    pub tasks_completed: Option<u32>,
    #[serde(default)]
    pub total_phases: Option<u32>,
    #[serde(default)]
    pub created: Option<String>,
    #[serde(default)]
    pub updated: Option<String>,
    /// Explicit `FailureTrigger` discriminant written by commands like
    /// `sdd_discard_phase_plan`. Free-form string here so unknown
    /// variants don't fail deserialisation; `derive_stage` re-parses
    /// into the typed enum.
    #[serde(default)]
    pub trigger: Option<String>,
}

/// Split a markdown file with leading `---\n…\n---\n` YAML frontmatter
/// into (parsed_yaml, body). Files with no frontmatter return
/// `(default, full_content)` — we tolerate sloppy agent output rather
/// than 500-erroring.
pub fn parse_frontmatter(content: &str) -> (FrontMatter, String) {
    let trimmed = content.trim_start_matches('\u{feff}'); // strip BOM
    if !trimmed.starts_with("---") {
        return (FrontMatter::default(), content.to_string());
    }
    // Find the closing `---` on its own line.
    let after_open = &trimmed[3..];
    // Skip the trailing newline of the opening fence.
    let after_open = after_open.strip_prefix('\n').unwrap_or(after_open);
    // Search for `\n---` followed by newline-or-EOF.
    let close_idx = after_open
        .find("\n---")
        .or_else(|| if after_open.starts_with("---") { Some(0) } else { None });
    let Some(idx) = close_idx else {
        return (FrontMatter::default(), content.to_string());
    };
    let yaml_str = &after_open[..idx];
    let rest = &after_open[idx..];
    // Strip the closing fence + optional newline.
    let body = rest
        .strip_prefix("\n---")
        .or_else(|| rest.strip_prefix("---"))
        .unwrap_or(rest)
        .trim_start_matches('\n')
        .to_string();
    let fm = serde_yaml::from_str::<FrontMatter>(yaml_str).unwrap_or_default();
    (fm, body)
}

/// Inverse of `parse_frontmatter` — write `<--- yaml ---> body`. Writes
/// atomically (`.tmp` + rename) so a reader can never observe a half-
/// written file. Borrowed straight from brain's `ProgressWriter`
/// pattern — same reason: the orchestrator (TS) is racing with the
/// agent's file writes and a torn read would crash the YAML parser.
///
/// NOTE: currently unused — F1 only modifies existing frontmatter via
/// `set_status_on` (which preserves unknown fields). Kept for v2 when
/// we'll write skeleton spec.md / plan.md placeholders into a fresh
/// workspace before the agent fills them in.
#[allow(dead_code)]
pub fn write_with_frontmatter(path: &Path, fm: &FrontMatter, body: &str) -> Result<(), String> {
    let yaml = serde_yaml::to_string(fm).map_err(|e| format!("yaml: {e}"))?;
    let content = format!("---\n{yaml}---\n\n{}\n", body.trim_end());
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
    }
    let tmp = path.with_extension("md.tmp");
    std::fs::write(&tmp, content).map_err(|e| format!("write tmp {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("rename {} → {}: {e}", tmp.display(), path.display()))
}

/// Raw-yaml variant — returns the YAML chunk + body as separate strings
/// without parsing the YAML. Used by writers that want to preserve
/// unknown fields verbatim (`set_status_on`, `replace_body_on`) when
/// flipping a single key inside frontmatter without re-encoding the
/// whole structure.
pub fn split_frontmatter_raw(content: &str) -> (String, String) {
    let trimmed = content.trim_start_matches('\u{feff}');
    if !trimmed.starts_with("---") {
        return (String::new(), content.to_string());
    }
    let after_open = &trimmed[3..];
    let after_open = after_open.strip_prefix('\n').unwrap_or(after_open);
    let Some(idx) = after_open.find("\n---") else {
        return (String::new(), content.to_string());
    };
    let yaml_str = &after_open[..idx];
    let rest = &after_open[idx..];
    let body = rest
        .strip_prefix("\n---")
        .unwrap_or(rest)
        .trim_start_matches('\n')
        .to_string();
    (yaml_str.to_string(), body)
}
