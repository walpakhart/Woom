//! Action-log entry types for SDD phase JSONL feeds. Extracted from
//! `sdd.rs` in wave-1 phase-10 refactor.
//!
//! Each phase writes `<workspace>/phases/phase-<N>.log.jsonl` with one
//! `ActionLogEntry` per line so the SddCard's live feed survives an
//! app restart. The schema mirrors `interface ActionLogEntry` in
//! `apps/desktop/src/lib/state/sdd.svelte.ts`. This module only owns
//! the data shapes — the Tauri commands that READ / APPEND / TAIL the
//! log stay in `sdd.rs` because they need access to the workspace
//! registry.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::sdd_substep::SddPhaseSubstep;
use crate::sdd_time::now_ms;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ActionLogKind {
    ToolUse,
    ToolResult,
    AgentMessage,
    SddEvent,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ActionLogEntry {
    /// Unix-ms when this entry was produced. Frontend supplies it so
    /// the wall-clock matches the chat-stream events the user sees.
    pub ts: u64,
    /// Owning phase. Required so cross-phase log files don't bleed
    /// into the wrong feed; we use it to pick the JSONL filename.
    pub phase: u32,
    pub kind: ActionLogKind,
    /// Tool name verbatim from the CLI. None for `agent_message` /
    /// `sdd_event`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    /// One-line summary, ≤80 chars expected. The frontend builds this
    /// via the same `formatToolUse` helper the chat thread uses, so
    /// the live feed reads consistently with the trace pills.
    pub summary: String,
    /// Optional expandable detail (full bash command, full mcp args).
    /// Stored verbatim — frontend handles truncation in the lightbox.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// Lifecycle: `running` (after tool_use, before tool_result),
    /// `done` (tool_result, no error), `failed` (tool_result with
    /// `is_error: true`). None for events that don't have a lifecycle
    /// (agent_message, sdd_event).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    /// Stable id from the CLI's `tool_use_id`; lets the UI match a
    /// `running` entry to its terminal `done` / `failed` and keep one
    /// pill per call instead of two stacked rows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
    /// Three-call mode — which sub-step was in flight when this entry
    /// was logged (`plan` / `implement` / `verify`). None for
    /// single-call workspaces and for legacy JSONL written before
    /// phase 5. Surfaces as group dividers in the SddCard live feed.
    /// See `spec-1` FR-9.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sub_step: Option<SddPhaseSubstep>,
}

/// Path to the per-phase JSONL feed under `<workspace>/phases/`. Used
/// by the orchestrator's substep divider + the
/// `sdd_append_action_log` / `sdd_read_action_log` Tauri commands
/// (which stay in `sdd.rs` because they need the workspace registry).
pub(crate) fn action_log_path(workspace_root: &Path, phase: u32) -> PathBuf {
    workspace_root
        .join("phases")
        .join(format!("phase-{phase}.log.jsonl"))
}

/// Append a synthetic `sdd_event` row marking the start of a
/// three-call sub-step. Used by the orchestrator (NOT the agent) to
/// drop a divider into the JSONL so SddCard's live feed can group
/// tool rows by pass. Best-effort — IO failures are logged but never
/// block the caller. See `spec-1` FR-9.
pub(crate) fn append_substep_started_event(
    workspace_root: &Path,
    phase: u32,
    sub_step: SddPhaseSubstep,
) {
    let summary = match sub_step {
        SddPhaseSubstep::Plan => "— plan —",
        SddPhaseSubstep::Implement => "— implement —",
        SddPhaseSubstep::Verify => "— verify —",
    };
    let entry = ActionLogEntry {
        ts: now_ms(),
        phase,
        kind: ActionLogKind::SddEvent,
        tool: None,
        summary: summary.into(),
        detail: None,
        status: None,
        correlation_id: None,
        sub_step: Some(sub_step),
    };
    let path = action_log_path(workspace_root, phase);
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            eprintln!("[sdd] substep_event mkdir {}: {e}", parent.display());
            return;
        }
    }
    let line = match serde_json::to_string(&entry) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[sdd] substep_event serialize: {e}");
            return;
        }
    };
    use std::io::Write;
    match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        Ok(mut f) => {
            let _ = writeln!(f, "{line}");
        }
        Err(e) => eprintln!("[sdd] substep_event open {}: {e}", path.display()),
    }
}
