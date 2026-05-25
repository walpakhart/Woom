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

use serde::{Deserialize, Serialize};

use crate::sdd_substep::SddPhaseSubstep;

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
