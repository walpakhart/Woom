//! Markdown-file mutators for SDD phase / spec / plan files.
//! Extracted from `sdd.rs` in wave-1 phase-10 refactor.
//!
//! Every function in this module touches a single .md file: read,
//! split frontmatter, mutate the YAML map (or body), write back via
//! atomic `.tmp` + rename so a reader can never observe a torn file.
//! Frontend never touches the markdown directly — these helpers are
//! the only path that writes to phase / spec / plan files outside
//! the agent's tool calls.

use std::path::Path;

use crate::sdd_frontmatter::split_frontmatter_raw;
use crate::sdd_time::{format_iso, now_ms};

/// Update the `status:` key inside a markdown file's YAML frontmatter,
/// preserving every other field. We deliberately re-parse the raw
/// YAML into a generic `serde_yaml::Mapping`, patch the `status` key,
/// and re-serialize — round-tripping preserves any custom fields
/// (verification commands, etc) that our typed FrontMatter doesn't
/// know about.
pub(crate) fn set_status_on(path: &Path, new_status: &str) -> Result<(), String> {
    let raw = std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let (yaml_str, body) = split_frontmatter_raw(&raw);
    let mut map: serde_yaml::Mapping = if yaml_str.trim().is_empty() {
        serde_yaml::Mapping::new()
    } else {
        serde_yaml::from_str(&yaml_str).map_err(|e| format!("parse frontmatter: {e}"))?
    };
    map.insert(
        serde_yaml::Value::String("status".into()),
        serde_yaml::Value::String(new_status.into()),
    );
    map.insert(
        serde_yaml::Value::String("updated".into()),
        serde_yaml::Value::String(format_iso(now_ms())),
    );
    let new_yaml = serde_yaml::to_string(&map).map_err(|e| format!("yaml: {e}"))?;
    let content = format!("---\n{new_yaml}---\n\n{}\n", body.trim_end());
    let tmp = path.with_extension("md.tmp");
    std::fs::write(&tmp, content).map_err(|e| format!("write tmp: {e}"))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("rename: {e}"))?;
    Ok(())
}

/// Replace the body of a markdown file while preserving its YAML
/// frontmatter verbatim. Used by `sdd_save_body` so the user can edit
/// spec.md / plan.md / phase.md content inline in the card without
/// blowing away the agent's frontmatter (status, depends_on, etc).
pub(crate) fn replace_body_on(path: &Path, new_body: &str) -> Result<(), String> {
    let raw = std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let (yaml_str, _) = split_frontmatter_raw(&raw);
    let content = if yaml_str.is_empty() {
        format!("{}\n", new_body.trim_end())
    } else {
        format!("---\n{yaml_str}\n---\n\n{}\n", new_body.trim_end())
    };
    let tmp = path.with_extension("md.tmp");
    std::fs::write(&tmp, content).map_err(|e| format!("write tmp: {e}"))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("rename: {e}"))?;
    Ok(())
}

/// Set both `status:` AND `summary:` in one round-trip. Used by
/// `sdd_save_phase_verify` so the phase frontmatter advances from
/// `running` → `done`/`failed` and the verify-pass summary lands in
/// the `summary:` field in a single atomic write. See `spec-1` FR-6.
pub(crate) fn set_status_and_summary_on(
    path: &Path,
    new_status: &str,
    new_summary: Option<&str>,
) -> Result<(), String> {
    let raw = std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let (yaml_str, body) = split_frontmatter_raw(&raw);
    let mut map: serde_yaml::Mapping = if yaml_str.trim().is_empty() {
        serde_yaml::Mapping::new()
    } else {
        serde_yaml::from_str(&yaml_str).map_err(|e| format!("parse frontmatter: {e}"))?
    };
    map.insert(
        serde_yaml::Value::String("status".into()),
        serde_yaml::Value::String(new_status.into()),
    );
    map.insert(
        serde_yaml::Value::String("updated".into()),
        serde_yaml::Value::String(format_iso(now_ms())),
    );
    // Only write `summary:` when the verify pass produced a real
    // string. Empty / missing summary leaves the field untouched —
    // never write garbage per FR-6.
    if let Some(summary) = new_summary {
        if !summary.trim().is_empty() {
            map.insert(
                serde_yaml::Value::String("summary".into()),
                serde_yaml::Value::String(summary.trim().into()),
            );
        }
    }
    let new_yaml = serde_yaml::to_string(&map).map_err(|e| format!("yaml: {e}"))?;
    let content = format!("---\n{new_yaml}---\n\n{}\n", body.trim_end());
    let tmp = path.with_extension("md.tmp");
    std::fs::write(&tmp, content).map_err(|e| format!("write tmp: {e}"))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("rename: {e}"))?;
    Ok(())
}

/// Reset a phase file's status back to `pending` so the orchestrator
/// will re-issue it on the next advance. Used by the card's "Retry"
/// button on a failed phase. Also clears any tasks_completed counter
/// so the agent re-attempts from the top.
pub(crate) fn reset_phase_status(path: &Path) -> Result<(), String> {
    let raw = std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let (yaml_str, body) = split_frontmatter_raw(&raw);
    let mut map: serde_yaml::Mapping = if yaml_str.trim().is_empty() {
        serde_yaml::Mapping::new()
    } else {
        serde_yaml::from_str(&yaml_str).map_err(|e| format!("parse frontmatter: {e}"))?
    };
    map.insert(
        serde_yaml::Value::String("status".into()),
        serde_yaml::Value::String("pending".into()),
    );
    map.insert(
        serde_yaml::Value::String("tasks_completed".into()),
        serde_yaml::Value::Number(serde_yaml::Number::from(0)),
    );
    let new_yaml = serde_yaml::to_string(&map).map_err(|e| format!("yaml: {e}"))?;
    let content = format!("---\n{new_yaml}---\n\n{}\n", body.trim_end());
    let tmp = path.with_extension("md.tmp");
    std::fs::write(&tmp, content).map_err(|e| format!("write tmp: {e}"))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("rename: {e}"))?;
    Ok(())
}
