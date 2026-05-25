//! Audit log — append-only JSONL of every workspace mutation,
//! regardless of whether the agent (via MCP tools), the user (via UI
//! clicks), or the system (orchestrator-internal events) triggered
//! it. Lives at `<workspace>/audit-log.jsonl` and is read by the
//! SddCard's audit overlay.
//!
//! Schema is intentionally compact: ts + source + action + phase +
//! reason + before/after JSON snapshots of whatever fields the action
//! changed. Forward-compatible — readers must skip lines that don't
//! deserialize, since we'll grow the schema over time.
//!
//! Extracted from `sdd.rs` in wave-1 phase-10 refactor.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::sdd_time::now_ms;

/// Single row of the audit log. `before` / `after` are free-form
/// JSON snapshots of the relevant fields for the action — e.g. for
/// `advance_phase`, `before = {"status": "pending_approval"}`,
/// `after = {"status": "running"}`. Kept compact on purpose; the
/// goal is "did the user override our refusal?", not full diffs.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct AuditEntry {
    pub ts: u64,
    /// One of: `agent`, `user`, `system`. Free-form to allow future
    /// sources (e.g. `cli`) without breaking the schema.
    pub source: String,
    /// Mutation action verb. Must match one of the values in the
    /// SDD phase 6 spec (`advance_phase`, `retry_phase`, …) so the
    /// UI's filter dropdown stays predictable.
    pub action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub before: serde_json::Value,
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub after: serde_json::Value,
}

impl AuditEntry {
    pub fn new(source: &str, action: &str) -> Self {
        Self {
            ts: now_ms(),
            source: source.into(),
            action: action.into(),
            phase: None,
            reason: None,
            before: serde_json::Value::Null,
            after: serde_json::Value::Null,
        }
    }
    pub fn with_phase(mut self, phase: u32) -> Self {
        self.phase = Some(phase);
        self
    }
    pub fn with_reason(mut self, reason: impl Into<String>) -> Self {
        let r = reason.into();
        if !r.trim().is_empty() {
            self.reason = Some(r);
        }
        self
    }
    pub fn with_before(mut self, v: serde_json::Value) -> Self {
        self.before = v;
        self
    }
    pub fn with_after(mut self, v: serde_json::Value) -> Self {
        self.after = v;
        self
    }
}

pub fn audit_log_path(workspace_root: &Path) -> PathBuf {
    workspace_root.join("audit-log.jsonl")
}

/// Append one entry to the workspace audit log. Best-effort —
/// failures log to stderr but never bubble. Mirrors the
/// action-log append pattern: POSIX append-mode + a single
/// `writeln!` is atomic for lines smaller than `PIPE_BUF`.
pub fn append(workspace_root: &Path, entry: &AuditEntry) {
    let path = audit_log_path(workspace_root);
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            eprintln!("[sdd:audit] mkdir {}: {e}", parent.display());
            return;
        }
    }
    let line = match serde_json::to_string(entry) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[sdd:audit] serialize: {e}");
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
        Err(e) => eprintln!("[sdd:audit] open {}: {e}", path.display()),
    }
}

/// Read every audit entry, oldest-first. Lines that fail to parse
/// are silently skipped (forward-compat). Missing file → empty
/// vector.
pub fn read_all(workspace_root: &Path) -> Vec<AuditEntry> {
    let path = audit_log_path(workspace_root);
    let raw = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    raw.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<AuditEntry>(l).ok())
        .collect()
}

/// Validate a mutation reason. Phase 6's contract: every mutating
/// MCP tool requires `reason` ≥ 5 chars after trim, so the audit
/// trail always carries a "why". Empty / whitespace-only / "ok" /
/// "yes" all reject. Returns the trimmed reason on success.
pub fn validate_reason(reason: &str) -> Result<String, String> {
    let trimmed = reason.trim();
    if trimmed.len() < 5 {
        return Err("reason too short — explain why you're advancing".into());
    }
    Ok(trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn td() -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static C: AtomicU64 = AtomicU64::new(0);
        let n = C.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let dir = std::env::temp_dir().join(format!("woom-sdd-audit-{pid}-{n}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn append_then_read_round_trip() {
        let dir = td();
        let e1 = AuditEntry::new("user", "advance_phase")
            .with_phase(1)
            .with_reason("approved manually")
            .with_before(serde_json::json!({"status":"pending_approval"}))
            .with_after(serde_json::json!({"status":"running"}));
        let e2 = AuditEntry::new("agent", "retry_phase")
            .with_phase(2)
            .with_reason("verifier failed once, trying again");
        append(&dir, &e1);
        append(&dir, &e2);
        let got = read_all(&dir);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].action, "advance_phase");
        assert_eq!(got[0].source, "user");
        assert_eq!(got[0].phase, Some(1));
        assert_eq!(got[1].action, "retry_phase");
        assert_eq!(got[1].source, "agent");
    }

    #[test]
    fn read_missing_returns_empty() {
        let dir = td();
        assert!(read_all(&dir).is_empty());
    }

    #[test]
    fn append_creates_workspace_dir_if_missing() {
        let dir = td();
        let e = AuditEntry::new("system", "boot");
        append(&dir, &e);
        assert!(audit_log_path(&dir).exists());
    }

    #[test]
    fn read_skips_corrupted_lines() {
        let dir = td();
        let good = AuditEntry::new("user", "pause");
        let body = format!(
            "{}\n{}\n{}\n",
            serde_json::to_string(&good).unwrap(),
            "{ not valid json …",
            serde_json::to_string(&good).unwrap(),
        );
        std::fs::write(audit_log_path(&dir), body).unwrap();
        let got = read_all(&dir);
        assert_eq!(got.len(), 2, "good lines kept, bad line dropped");
    }

    #[test]
    fn validate_reason_rejects_short() {
        assert!(validate_reason("").is_err());
        assert!(validate_reason("   ").is_err());
        assert!(validate_reason("ok").is_err());
        assert!(validate_reason("yes!").is_err());
    }

    #[test]
    fn validate_reason_accepts_long_enough() {
        assert_eq!(
            validate_reason("approved").unwrap(),
            "approved".to_string()
        );
        assert_eq!(
            validate_reason("  approved manually  ").unwrap(),
            "approved manually".to_string()
        );
    }

    #[test]
    fn entry_skip_serializes_nulls() {
        let e = AuditEntry::new("user", "pause");
        let s = serde_json::to_string(&e).unwrap();
        assert!(!s.contains("\"before\""), "before should be omitted");
        assert!(!s.contains("\"after\""), "after should be omitted");
        assert!(!s.contains("\"phase\""), "phase should be omitted");
        assert!(!s.contains("\"reason\""), "reason should be omitted");
    }
}
