//! MCP-handler helpers — shared validation between the woom-app
//! sidecar's tool stubs and the Tauri-side audit-aware variants.
//! Extracted from `sdd.rs` in wave-1 phase-10 refactor.
//!
//! Pure functions, no IO — the sidecar can pull `MUTATING_ACTIONS` +
//! `validate_mutation` without dragging in the whole `sdd` module.
//! `cargo test sdd_mcp_handlers` runs the validation tests in
//! isolation.

use crate::sdd_audit::validate_reason;

/// Mutation actions accepted by the audit log + MCP tool surface.
/// Listed in the same order as the phase 6 spec for grep-ability.
pub const MUTATING_ACTIONS: &[&str] = &[
    "advance_phase",
    "retry_phase",
    "skip_phase",
    "approve_spec",
    "approve_plan",
    "rollback_phase",
    "pause",
    "resume",
    "stop",
    "discard",
    "edit_body",
    "insert_phase",
    "delete_phase",
    "manual_check_marked",
];

/// Read-only tools that DON'T require a `reason` and never emit
/// audit entries. Kept here so the sidecar + frontend can agree on
/// the split without duplicating the list.
#[allow(dead_code)] // Used by tests + future frontend feature-detection.
pub const READ_ONLY_TOOLS: &[&str] = &[
    "sdd_get",
    "sdd_list_phases",
    "sdd_get_phase",
    "sdd_get_action_log",
    "sdd_get_results",
];

/// Validate a mutating MCP request. Today the only check is the
/// shared reason-length gate, but the function gives us a single
/// place to grow into per-action validation (e.g. refusing
/// `advance_phase` when the previous phase isn't `done`).
pub fn validate_mutation(action: &str, reason: &str) -> Result<String, String> {
    if !MUTATING_ACTIONS.contains(&action) {
        return Err(format!(
            "unknown mutating action `{action}` — see MUTATING_ACTIONS"
        ));
    }
    validate_reason(reason)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_mutation_rejects_short_reason() {
        let err = validate_mutation("advance_phase", "ok").unwrap_err();
        assert!(err.contains("reason too short"));
    }

    #[test]
    fn validate_mutation_accepts_known_action_with_long_reason() {
        let r = validate_mutation("retry_phase", "verifier flaked, retrying").unwrap();
        assert_eq!(r, "verifier flaked, retrying");
    }

    #[test]
    fn validate_mutation_rejects_unknown_action() {
        let err = validate_mutation("delete_universe", "burn it all down").unwrap_err();
        assert!(err.contains("unknown mutating action"));
    }

    #[test]
    fn read_only_tools_disjoint_from_mutating_actions() {
        for r in READ_ONLY_TOOLS {
            let stripped = r.strip_prefix("sdd_").unwrap_or(r);
            assert!(
                !MUTATING_ACTIONS.contains(&stripped),
                "{stripped} is in both lists"
            );
        }
    }
}
