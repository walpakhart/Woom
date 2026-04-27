//! MCP-config plumbing for the Claude CLI adapter. Owns:
//!   - `ToolProfile`: which subset of MCP tools to expose this session
//!   - `build_mcp_config`: writes the temp `--mcp-config` JSON, picks
//!     the `--allowedTools` list, and skips spawning sidecars whose
//!     entire tool surface was filtered out by the profile.
//!   - per-server builders that pull creds from Keychain and locate
//!     the bundled sidecar binaries.
//!
//! Lives in its own module because `claude.rs` was getting unwieldy
//! and the MCP wiring is independent of the spawn / streaming / one-
//! shot paths in `claude.rs` — it just feeds them a finished config
//! path + allowed-tool list. Kept `pub(crate)` (not `pub`) so the
//! surface stays internal to the desktop binary.

use std::path::PathBuf;

use crate::claude;
use crate::jira::{JiraCredentials, normalize_workspace};
use crate::keychain;
use crate::sentry::SentryCredentials;

const JIRA_KEYCHAIN_KEY: &str = "jira";
const GITHUB_KEYCHAIN_KEY: &str = "github";
const SENTRY_KEYCHAIN_KEY: &str = "sentry";

/// Removes its path on drop. Used to clean up the temp MCP config file
/// `build_mcp_config` writes — the caller (`claude::ask`) holds one of
/// these as a guard so the file disappears on every exit path,
/// including panics.
pub(crate) struct TempFile(pub(crate) PathBuf);

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Which subset of MCP tools to expose for a given chat session. Each
/// MCP tool schema costs ~150-300 tokens of system-prompt overhead, so
/// trimming the wired set is the cheapest startup-cost win we have.
/// `from_str` is intentionally lenient — unknown / null falls back to
/// `All` (legacy behavior, every tool wired).
///
/// Six profiles cover the recurring chat shapes:
///  - Coding: just code (no external integrations)
///  - GitHub / Jira / Sentry: single-source focus — that integration
///    full-access plus base navigation
///  - Triage: read-only cross-tool — "what's the state of X" without edits
///  - All: legacy fallback when nothing's been picked
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToolProfile {
    All,
    Coding,
    Github,
    Jira,
    Sentry,
    Triage,
}

impl ToolProfile {
    pub(crate) fn from_str(s: Option<&str>) -> Self {
        match s.map(str::trim) {
            Some("coding") => Self::Coding,
            Some("github") => Self::Github,
            Some("jira") => Self::Jira,
            Some("sentry") => Self::Sentry,
            Some("triage") => Self::Triage,
            _ => Self::All,
        }
    }

    /// Decide whether `tool` (full mcp__server__name) is in this profile.
    /// Built-in tools (Edit/Read/Bash/Grep/etc) are unaffected — Claude
    /// always exposes them; we only filter MCP-side schemas.
    pub(crate) fn allows(self, tool: &str) -> bool {
        // Memory + base App nav are always in. Memory persists across
        // chats so it's universally useful; App nav has no token cost
        // beyond the base set we always want (open detail panes,
        // switch views, list instances).
        let base = tool.starts_with("mcp__memory__")
            || matches!(tool,
                "mcp__app__list_instances"
                | "mcp__app__switch_view"
                | "mcp__app__open_connect_modal"
            );
        if base {
            return true;
        }
        match self {
            Self::All => true,
            Self::Coding => {
                // Code-focused: full App-nav (so the agent can spawn
                // editor columns / switch cwds), nothing external.
                tool.starts_with("mcp__app__")
            }
            Self::Github => {
                // GitHub full-access + the App-nav surface that opens
                // PRs / repos / pins the GH column. PR review and PR
                // authoring both live here.
                tool.starts_with("mcp__github__")
                    || matches!(tool,
                        "mcp__app__open_github_pr"
                        | "mcp__app__open_github_issue"
                        | "mcp__app__open_github_repo"
                        | "mcp__app__set_github_column"
                    )
            }
            Self::Jira => {
                tool.starts_with("mcp__jira__")
                    || matches!(tool,
                        "mcp__app__open_jira_issue"
                        | "mcp__app__open_jira_tab"
                        | "mcp__app__set_jira_column"
                    )
            }
            Self::Sentry => {
                tool.starts_with("mcp__sentry__")
                    || matches!(tool,
                        "mcp__app__open_sentry_issue"
                        | "mcp__app__open_sentry_event"
                        | "mcp__app__open_sentry_tab"
                        | "mcp__app__set_sentry_column"
                    )
            }
            Self::Triage => {
                // Read-only cross-tool. No write paths (no merge_pr,
                // no transition_issue, no add_comment, no propose_*,
                // no update_issue) — triage is "show me", not edit.
                matches!(tool,
                    "mcp__jira__get_issue"
                    | "mcp__jira__search"
                    | "mcp__jira__list_projects"
                    | "mcp__jira__list_assignable_users"
                    | "mcp__jira__list_sprints"
                    | "mcp__github__get_pr"
                    | "mcp__github__get_pr_diff"
                    | "mcp__github__get_pr_files"
                    | "mcp__github__get_pr_comments"
                    | "mcp__github__list_tree"
                    | "mcp__github__get_file"
                    | "mcp__github__list_commits"
                    | "mcp__github__list_releases"
                    | "mcp__github__list_workflow_runs"
                    | "mcp__github__get_readme"
                    | "mcp__github__search_prs"
                    | "mcp__github__search_issues"
                    | "mcp__github__list_repos"
                    | "mcp__sentry__get_issue"
                    | "mcp__sentry__search_issues"
                    | "mcp__sentry__get_event"
                    | "mcp__sentry__get_issue_tags"
                    | "mcp__sentry__list_events"
                    | "mcp__sentry__list_projects"
                    | "mcp__sentry__list_releases"
                    | "mcp__app__open_github_pr"
                    | "mcp__app__open_github_issue"
                    | "mcp__app__open_jira_issue"
                    | "mcp__app__open_sentry_issue"
                    | "mcp__app__open_sentry_event"
                    | "mcp__app__open_github_repo"
                    | "mcp__app__open_jira_tab"
                    | "mcp__app__open_sentry_tab"
                )
            }
        }
    }
}

/// Build an MCP config file for this session by pulling creds from Keychain
/// and wiring up available sidecars. Returns the temp config path and the
/// list of tool names to allow (one entry per tool, explicit — wildcards are
/// not universally supported by `claude -p --allowedTools`).
///
/// `profile` filters the wired set after the per-server blocks below have
/// pushed everything they could connect to. Servers whose every tool got
/// filtered out are dropped from the config so we don't spawn sidecars for
/// nothing.
///
/// Returns `None` when no sidecars can be configured (e.g. Jira is not
/// connected or the sidecar binary isn't next to the main exe); in that case
/// we run `claude` without MCP, preserving the old behavior.
pub(crate) fn build_mcp_config(
    session_id: &str,
    profile: ToolProfile,
) -> Option<(PathBuf, Vec<String>)> {
    let mut servers = serde_json::Map::new();
    let mut allowed: Vec<String> = Vec::new();

    if let Some(jira) = build_jira_server() {
        servers.insert("jira".into(), jira);
        allowed.push("mcp__jira__get_issue".into());
        allowed.push("mcp__jira__search".into());
        allowed.push("mcp__jira__add_comment".into());
        allowed.push("mcp__jira__transition_issue".into());
        allowed.push("mcp__jira__list_projects".into());
        allowed.push("mcp__jira__create_issue".into());
        allowed.push("mcp__jira__update_issue".into());
        allowed.push("mcp__jira__list_assignable_users".into());
        allowed.push("mcp__jira__list_sprints".into());
    }

    if let Some(gh) = build_github_server() {
        servers.insert("github".into(), gh);
        allowed.push("mcp__github__get_pr".into());
        allowed.push("mcp__github__get_pr_diff".into());
        allowed.push("mcp__github__get_pr_files".into());
        allowed.push("mcp__github__get_pr_comments".into());
        allowed.push("mcp__github__list_tree".into());
        allowed.push("mcp__github__get_file".into());
        allowed.push("mcp__github__list_commits".into());
        allowed.push("mcp__github__list_releases".into());
        allowed.push("mcp__github__list_workflow_runs".into());
        allowed.push("mcp__github__get_readme".into());
        allowed.push("mcp__github__search_prs".into());
        allowed.push("mcp__github__search_issues".into());
        allowed.push("mcp__github__list_repos".into());
        allowed.push("mcp__github__add_comment".into());
        allowed.push("mcp__github__submit_review".into());
        allowed.push("mcp__github__merge_pr".into());
        allowed.push("mcp__github__propose_commit".into());
        allowed.push("mcp__github__propose_pr".into());
        allowed.push("mcp__github__propose_switch_cwd".into());
        allowed.push("mcp__github__propose_bash".into());
    }

    if let Some(mem) = build_memory_server() {
        servers.insert("memory".into(), mem);
        allowed.push("mcp__memory__memory_save".into());
        allowed.push("mcp__memory__memory_search".into());
        allowed.push("mcp__memory__memory_list".into());
        allowed.push("mcp__memory__memory_delete".into());
    }

    if let Some(s) = build_sentry_server() {
        servers.insert("sentry".into(), s);
        allowed.push("mcp__sentry__get_issue".into());
        allowed.push("mcp__sentry__search_issues".into());
        allowed.push("mcp__sentry__get_event".into());
        allowed.push("mcp__sentry__get_issue_tags".into());
        allowed.push("mcp__sentry__list_events".into());
        allowed.push("mcp__sentry__update_issue".into());
        allowed.push("mcp__sentry__add_comment".into());
        allowed.push("mcp__sentry__list_projects".into());
        allowed.push("mcp__sentry__list_releases".into());
    }

    // forgehold-app: in-app navigation. Tool calls are intercepted by
    // the frontend's stream parser to drive the UI (open detail panes,
    // switch views, add editor instances, surface connect modals).
    // Always wired — no creds needed.
    if let Some(app) = build_app_server() {
        servers.insert("app".into(), app);
        allowed.push("mcp__app__open_github_pr".into());
        allowed.push("mcp__app__open_github_issue".into());
        allowed.push("mcp__app__open_jira_issue".into());
        allowed.push("mcp__app__open_sentry_issue".into());
        allowed.push("mcp__app__switch_view".into());
        allowed.push("mcp__app__add_editor_instance".into());
        allowed.push("mcp__app__open_connect_modal".into());
        allowed.push("mcp__app__add_workbench_instance".into());
        allowed.push("mcp__app__new_workbench".into());
        allowed.push("mcp__app__switch_workbench".into());
        allowed.push("mcp__app__focus_workbench_instance".into());
        allowed.push("mcp__app__open_github_repo".into());
        allowed.push("mcp__app__open_jira_tab".into());
        allowed.push("mcp__app__open_sentry_tab".into());
        allowed.push("mcp__app__set_github_column".into());
        allowed.push("mcp__app__set_jira_column".into());
        allowed.push("mcp__app__set_sentry_column".into());
        allowed.push("mcp__app__set_editor_repo_path".into());
        allowed.push("mcp__app__set_agent_cwd".into());
        allowed.push("mcp__app__list_instances".into());
        allowed.push("mcp__app__open_sentry_event".into());
    }

    // Apply profile filter. Keep app-side & memory entries that profile
    // allows; drop tools outside the profile so they don't bloat the
    // system prompt.
    allowed.retain(|t| profile.allows(t));
    // Drop sidecars whose every tool was filtered out — no point
    // spawning a process Claude can't call into.
    for srv in ["jira", "github", "memory", "sentry", "app"] {
        let prefix = format!("mcp__{}__", srv);
        if !allowed.iter().any(|t| t.starts_with(&prefix)) {
            servers.remove(srv);
        }
    }

    if servers.is_empty() {
        return None;
    }

    let config = serde_json::json!({ "mcpServers": servers });
    let body = serde_json::to_string(&config).ok()?;
    let path = std::env::temp_dir().join(format!("forgehold-mcp-{}.json", session_id));
    std::fs::write(&path, body).ok()?;
    Some((path, allowed))
}

fn build_jira_server() -> Option<serde_json::Value> {
    let stored = keychain::get(JIRA_KEYCHAIN_KEY).ok().flatten()?;
    let creds: JiraCredentials = serde_json::from_str(&stored).ok()?;
    let sidecar = find_sidecar("forgehold-jira")?;
    Some(serde_json::json!({
        "command": sidecar.to_string_lossy(),
        "env": {
            "JIRA_WORKSPACE": normalize_workspace(&creds.workspace),
            "JIRA_EMAIL": creds.email,
            "JIRA_TOKEN": creds.token,
        }
    }))
}

fn build_github_server() -> Option<serde_json::Value> {
    let token = keychain::get(GITHUB_KEYCHAIN_KEY).ok().flatten()?;
    if token.trim().is_empty() {
        return None;
    }
    let sidecar = find_sidecar("forgehold-github")?;
    Some(serde_json::json!({
        "command": sidecar.to_string_lossy(),
        "env": {
            "GITHUB_TOKEN": token,
        }
    }))
}

fn build_sentry_server() -> Option<serde_json::Value> {
    let stored = keychain::get(SENTRY_KEYCHAIN_KEY).ok().flatten()?;
    let creds: SentryCredentials = serde_json::from_str(&stored).ok()?;
    let sidecar = find_sidecar("forgehold-sentry")?;
    Some(serde_json::json!({
        "command": sidecar.to_string_lossy(),
        "env": {
            "SENTRY_HOST": creds.host,
            "SENTRY_ORG": creds.organization_slug,
            "SENTRY_TOKEN": creds.token,
        }
    }))
}

/// Wire up the bundled `forgehold-memory` sidecar — a SQLite-backed notes store
/// exposed via MCP. Ships with the .app, no external install. Persists under
/// the app's data dir (`~/Library/Application Support/Forgehold/memory.db` on
/// macOS) so notes survive across sessions.
fn build_memory_server() -> Option<serde_json::Value> {
    let sidecar = find_sidecar("forgehold-memory")?;
    let db_path = app_data_dir().map(|d| d.join("memory.db"));
    let db_str = db_path
        .as_ref()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    Some(serde_json::json!({
        "command": sidecar.to_string_lossy(),
        "env": {
            "FORGEHOLD_MEMORY_DB": db_str,
        }
    }))
}

/// Wire up the bundled `forgehold-app` sidecar — exposes UI navigation
/// tools (open detail panes, switch views, add editor columns). Tool
/// calls are intercepted by the frontend's stream parser; the sidecar
/// is intentionally thin (just registers the schemas).
fn build_app_server() -> Option<serde_json::Value> {
    let sidecar = find_sidecar("forgehold-app")?;
    Some(serde_json::json!({
        "command": sidecar.to_string_lossy(),
    }))
}

/// Per-platform app data directory. Mirrors Tauri's default app-config dir
/// but keeps this module self-contained (no `AppHandle` threaded through).
fn app_data_dir() -> Option<PathBuf> {
    let home = claude::home_dir()?;
    #[cfg(target_os = "macos")]
    let dir = home.join("Library/Application Support/Forgehold");
    #[cfg(target_os = "linux")]
    let dir = home.join(".local/share/forgehold");
    #[cfg(target_os = "windows")]
    let dir = home.join("AppData/Roaming/Forgehold");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

fn find_sidecar(name: &str) -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let candidate = dir.join(name);
    if candidate.is_file() {
        Some(candidate)
    } else {
        None
    }
}
