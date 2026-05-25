//! V2 plan-as-data helpers — pure file-shuffling + `plan.json` regen.
//! Extracted from `sdd.rs` in wave-21 so the heavy phase mutation
//! commands (insert / reorder / delete / skip / upgrade) all share one
//! place for the tricky rename + frontmatter-patch logic.
//!
//! All functions are filesystem-only (no Tauri state). Tests live with
//! `sdd.rs`'s integration suite because they want a temp dir + the
//! same phase file fixtures.

use std::collections::HashMap;
use std::path::Path;

use crate::sdd_frontmatter::{parse_frontmatter, split_frontmatter_raw};
use crate::sdd_hydrate::phase_number_from;
use crate::sdd_plan::{read_plan_json, write_plan_json, SddPhaseAcceptance, SddPlanFile, SddPlanPhase};
use crate::sdd_time::{format_iso, now_ms};

/// Rename `<phases_dir>/<old_slug>.md` to use `new_number` as its
/// numeric prefix and update the frontmatter `phase:` value to match.
/// Returns the new slug.
pub(crate) fn rename_phase_file(
    phases_dir: &Path,
    old_slug: &str,
    new_number: u32,
) -> Result<String, String> {
    let suffix = old_slug.split_once('-').map(|(_, s)| s).unwrap_or(old_slug);
    let new_slug = format!("{:02}-{}", new_number, suffix);
    let old_path = phases_dir.join(format!("{old_slug}.md"));
    let new_path = phases_dir.join(format!("{new_slug}.md"));
    if old_path != new_path {
        std::fs::rename(&old_path, &new_path)
            .map_err(|e| format!("rename {} -> {}: {e}", old_path.display(), new_path.display()))?;
    }
    update_phase_number_in_frontmatter(&new_path, new_number)?;
    Ok(new_slug)
}

/// Patch a phase markdown file's frontmatter `phase:` value. Preserves
/// every other field via the same round-trip `serde_yaml` Mapping
/// trick `set_status_on` uses.
pub(crate) fn update_phase_number_in_frontmatter(
    path: &Path,
    new_number: u32,
) -> Result<(), String> {
    let raw = std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let (yaml_str, body) = split_frontmatter_raw(&raw);
    let mut map: serde_yaml::Mapping = if yaml_str.trim().is_empty() {
        serde_yaml::Mapping::new()
    } else {
        serde_yaml::from_str(&yaml_str).map_err(|e| format!("parse frontmatter: {e}"))?
    };
    map.insert(
        serde_yaml::Value::String("phase".into()),
        serde_yaml::Value::Number(serde_yaml::Number::from(new_number as u64)),
    );
    let new_yaml = serde_yaml::to_string(&map).map_err(|e| format!("yaml: {e}"))?;
    let content = format!("---\n{new_yaml}---\n\n{}\n", body.trim_end());
    let tmp = path.with_extension("md.tmp");
    std::fs::write(&tmp, content).map_err(|e| format!("write tmp: {e}"))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("rename: {e}"))?;
    Ok(())
}

/// Like `set_status_on` but also writes additional frontmatter keys
/// in the same atomic round-trip. Used by `sdd_skip_phase` to set
/// both `status: skipped` and `skip_reason: "<text>"` together.
pub(crate) fn set_status_with_extras(
    path: &Path,
    new_status: &str,
    extras: Vec<(String, String)>,
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
    for (k, v) in extras {
        map.insert(
            serde_yaml::Value::String(k),
            serde_yaml::Value::String(v),
        );
    }
    let new_yaml = serde_yaml::to_string(&map).map_err(|e| format!("yaml: {e}"))?;
    let content = format!("---\n{new_yaml}---\n\n{}\n", body.trim_end());
    let tmp = path.with_extension("md.tmp");
    std::fs::write(&tmp, content).map_err(|e| format!("write tmp: {e}"))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("rename: {e}"))?;
    Ok(())
}

/// Re-derive `<workspace>/plan.json` from the phase markdown files.
/// Preserves any existing `acceptance` arrays (matched by phase
/// number) so a regen doesn't blow away verifier criteria the agent
/// wrote previously.
pub(crate) fn rebuild_plan_json(root: &Path) -> Result<(), String> {
    let phases_dir = root.join("phases");
    let mut entries: Vec<(u32, String, String, Vec<u32>)> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&phases_dir) {
        for entry in rd.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("md") {
                continue;
            }
            let stem = match path.file_stem().and_then(|s| s.to_str()) {
                Some(s) => s.to_string(),
                None => continue,
            };
            let raw = std::fs::read_to_string(&path).unwrap_or_default();
            let (fm, _body) = parse_frontmatter(&raw);
            let number = phase_number_from(&stem, &fm);
            if number == 0 {
                continue;
            }
            let title = fm.title.clone().unwrap_or_default();
            let depends = fm.depends_on.clone();
            entries.push((number, stem, title, depends));
        }
    }
    entries.sort_by_key(|(n, _, _, _)| *n);
    /* Carry over acceptance from the existing plan.json so a regen
     * doesn't drop the verifier criteria. */
    let prior = read_plan_json(root);
    let prior_by_num: HashMap<u32, Vec<SddPhaseAcceptance>> = prior
        .map(|p| p.phases.into_iter().map(|x| (x.number, x.acceptance)).collect())
        .unwrap_or_default();
    let phases: Vec<SddPlanPhase> = entries
        .into_iter()
        .map(|(number, slug, title, depends_on)| SddPlanPhase {
            number,
            slug,
            title,
            depends_on,
            complexity: None,
            acceptance: prior_by_num.get(&number).cloned().unwrap_or_default(),
        })
        .collect();
    write_plan_json(root, &SddPlanFile { version: 1, phases })
}

/// Lower-case + spaces-to-dashes + strip non-alphanumeric. Used to
/// derive a filename slug from a human title (`"Foundation Phase"` →
/// `"foundation-phase"`).
pub(crate) fn slugify(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last_dash = false;
    for ch in s.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash && !out.is_empty() {
            out.push('-');
            last_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        "phase".into()
    } else {
        out
    }
}
