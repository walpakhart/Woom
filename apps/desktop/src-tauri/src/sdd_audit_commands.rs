//! Audit-log Tauri commands. Extracted from `sdd.rs` in wave-17
//! split. Thin wrappers around `crate::sdd_audit` + the
//! `mcp_handlers::validate_mutation` helper. Frontend stream parser
//! calls `sdd_audit_append` after every successful mutation; the
//! SddCard's audit overlay calls `sdd_audit_read`.

use std::path::PathBuf;

use tauri::State;

use crate::sdd::SddRegistry;
use crate::sdd_audit::{self as audit, AuditEntry};
use crate::sdd_mcp_handlers;

#[tauri::command]
pub async fn sdd_audit_append(
    registry: State<'_, SddRegistry>,
    id: String,
    entry: AuditEntry,
) -> Result<(), String> {
    let cell = registry
        .workspaces
        .read()
        .get(&id)
        .cloned()
        .ok_or_else(|| format!("unknown workspace {id}"))?;
    let root = PathBuf::from(cell.read().root.clone());
    audit::append(&root, &entry);
    Ok(())
}

#[tauri::command]
pub async fn sdd_audit_read(
    registry: State<'_, SddRegistry>,
    id: String,
) -> Result<Vec<AuditEntry>, String> {
    let cell = registry
        .workspaces
        .read()
        .get(&id)
        .cloned()
        .ok_or_else(|| format!("unknown workspace {id}"))?;
    let root = PathBuf::from(cell.read().root.clone());
    Ok(audit::read_all(&root))
}

/// Server-side reason gate the agent-facing MCP tool stubs share via
/// the `mcp_handlers::validate_mutation` helper. Exposed as a Tauri
/// command so the frontend stream parser can reject malformed agent
/// calls before invoking the underlying mutation.
#[tauri::command]
pub async fn sdd_validate_mutation(action: String, reason: String) -> Result<String, String> {
    sdd_mcp_handlers::validate_mutation(&action, &reason)
}
