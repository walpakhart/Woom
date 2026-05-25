//! SDD meta side-car shapes — extracted from `sdd.rs` in wave-1
//! phase-10 refactor. Two structs that mirror the on-disk JSON files
//! `<workspace>/meta.json` and `<workspace>/phases/phase-N-meta.json`.
//! Both kept simple Serde-only types — no behaviour, no I/O — so they
//! compose cleanly into the main sdd.rs without pulling in the rest
//! of the orchestrator's plumbing.

use serde::{Deserialize, Serialize};

use crate::sdd_phase_config::PhaseExecutionConfig;

/// Side-car metadata stored at `<workspace>/meta.json`. Currently
/// holds the bits that DON'T live in spec.md frontmatter (session id,
/// user's original ask) so they survive an app restart even when
/// the agent hasn't written spec.md yet.
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub(crate) struct SddMeta {
    pub(crate) user_prompt: Option<String>,
    pub(crate) session_id: Option<String>,
    pub(crate) created_at: Option<u64>,
    /// Absolute path of the repo cwd the SDD session is editing — typically
    /// the linked editor / agent session's cwd at start time. None when the
    /// caller didn't supply one (legacy v1 workspaces, or non-git callers).
    /// All git operations target this directory; if it isn't a git repo,
    /// `git_enabled` stays false and the orchestrator skips snapshots /
    /// commits / rollback (degraded mode per the spec).
    #[serde(default)]
    pub(crate) repo_cwd: Option<String>,
    #[serde(default)]
    pub(crate) git_enabled: bool,
    /// `sdd/<workspace-id>` — the per-workspace branch we mint at start.
    /// Stays None when `git_enabled = false`.
    #[serde(default)]
    pub(crate) sdd_branch: Option<String>,
    /// Sha of `repo_cwd`'s HEAD at workspace creation. Used as the
    /// "rollback to clean slate" target when the user wipes the whole
    /// workspace, separate from per-phase pre-snapshots.
    #[serde(default)]
    pub(crate) parent_sha: Option<String>,
    /// Three-call execution config — see `PhaseExecutionConfig`. Missing
    /// on legacy workspaces; deserializes to `Default` (single_call).
    /// The hydrate-path migration writes the default block to disk so
    /// future reads have a populated value.
    #[serde(default)]
    pub(crate) phase_execution: PhaseExecutionConfig,
}

/// Per-phase meta side-car at `<workspace>/phases/phase-N-meta.json`.
/// Holds the git shas captured around each phase so rollback / recovery
/// can return to a known state. Kept separate from the markdown
/// frontmatter because shas are noisy data the user shouldn't be
/// hand-editing.
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub(crate) struct SddPhaseMeta {
    /// Sha captured immediately before the phase ran (the rollback target).
    /// None when git_enabled was false at approve-time.
    #[serde(default)]
    pub(crate) pre_phase_sha: Option<String>,
    /// Sha of the commit that captured the phase's output. None when
    /// the phase hasn't completed verification yet, or git was off.
    #[serde(default)]
    pub(crate) post_phase_sha: Option<String>,
    /// True when the workspace was non-git at approve-time so we
    /// deliberately skipped the snapshot. Lets the UI label the
    /// phase as "no rollback available" instead of "missing".
    #[serde(default)]
    pub(crate) snapshot_skipped: bool,
    /// User-supplied reason when the phase was force-skipped via
    /// `sdd_skip_phase_with_reason` (failure card's Skip button).
    /// Mirrors the `skip_reason` frontmatter key but kept here too so
    /// the audit log can read it without re-parsing markdown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) skip_reason: Option<String>,
    /// User-supplied reason when a failed phase was accepted as-is via
    /// `sdd_accept_phase_failed` (failure card's "Accept anyway"
    /// button). Mirrors the `accepted_reason` frontmatter key. Distinct
    /// from `skip_reason` — accept flips status to `done`, skip flips
    /// to `skipped`, so the audit semantics differ.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) accepted_reason: Option<String>,
    /// Number of retry attempts made on this phase. Bumped by
    /// `sdd_retry_phase` (and the edit-then-retry flow). The UI shows
    /// a soft warning starting at 3 — "3rd retry — consider editing
    /// the spec or skipping". Defaults to 0 for fresh phases.
    #[serde(default)]
    pub(crate) retry_count: u32,
}

use std::path::Path;

/// Read `<workspace>/phases/phase-<N>-meta.json`. Returns the
/// `Default` shape on missing OR parse-failed file — fail-open so a
/// corrupted side-car doesn't permanently block recovery flows.
pub(crate) fn read_phase_meta(workspace_root: &Path, phase: u32) -> SddPhaseMeta {
    let path = workspace_root
        .join("phases")
        .join(format!("phase-{phase}-meta.json"));
    let raw = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => return SddPhaseMeta::default(),
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

/// Atomically write phase meta (`.tmp` + rename so concurrent
/// readers never observe a torn file). Creates the `phases/` dir
/// lazily — the per-phase markdown file may not exist yet when a
/// snapshot lands.
pub(crate) fn write_phase_meta(
    workspace_root: &Path,
    phase: u32,
    meta: &SddPhaseMeta,
) -> Result<(), String> {
    let dir = workspace_root.join("phases");
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir phases: {e}"))?;
    let path = dir.join(format!("phase-{phase}-meta.json"));
    let body =
        serde_json::to_string_pretty(meta).map_err(|e| format!("serialize phase-meta: {e}"))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, body).map_err(|e| format!("write phase-meta tmp: {e}"))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("rename phase-meta: {e}"))?;
    Ok(())
}
