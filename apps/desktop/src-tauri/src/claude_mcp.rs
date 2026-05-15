//! MCP-config plumbing for the Claude CLI adapter. Owns:
//!   - `build_mcp_config`: writes the temp `--mcp-config` JSON and the
//!     `--allowedTools` list for all available sidecars AND any
//!     third-party MCP servers the user installed via `claude mcp add`.
//!   - per-server builders that pull creds from Keychain and locate
//!     the bundled sidecar binaries.
//!   - `user_mcp_servers`: scans `~/.claude.json` so the merged config
//!     stays consistent with the Claude CLI's own view of what's
//!     installed, even though we pass `--strict-mcp-config` and the CLI
//!     wouldn't load that file on its own.
//!
//! Lives in its own module because `claude.rs` was getting unwieldy
//! and the MCP wiring is independent of the spawn / streaming / one-
//! shot paths in `claude.rs` — it just feeds them a finished config
//! path + allowed-tool list. Kept `pub(crate)` (not `pub`) so the
//! surface stays internal to the desktop binary.

use std::path::{Path, PathBuf};

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

/// Build an MCP config file for this session by pulling creds from Keychain,
/// wiring up available bundled sidecars, AND merging in any third-party MCP
/// servers the user registered via `claude mcp add` (see `user_mcp_servers`).
///
/// Returns the temp config path and the list of tool names to allow. For
/// Woom's own sidecars we enumerate every tool explicitly so the surface is
/// stable across CLI versions; for user-added servers we use the server-wide
/// `mcp__<name>` form (Claude CLI accepts this as "every tool from this
/// server") to avoid having to probe each third-party server's schema.
///
/// Returns `None` when no servers can be configured at all (e.g. fresh
/// install, nothing connected, no user MCPs); in that case we run `claude`
/// without MCP, preserving the old behavior.
pub(crate) fn build_mcp_config(
    session_id: &str,
    ipc_socket: Option<&Path>,
) -> Option<(PathBuf, Vec<String>)> {
    let mut servers = serde_json::Map::new();
    let mut allowed: Vec<String> = Vec::new();
    let ipc_str = ipc_socket
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();

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

    if let Some(gh) = build_github_server(session_id, &ipc_str) {
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
    }

    if let Some(mem) = build_memory_server() {
        servers.insert("memory".into(), mem);
        allowed.push("mcp__memory__memory_save".into());
        allowed.push("mcp__memory__memory_search".into());
        allowed.push("mcp__memory__memory_list".into());
        allowed.push("mcp__memory__memory_get".into());
        allowed.push("mcp__memory__memory_update".into());
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

    // woom-app: in-app navigation. Tool calls are intercepted by
    // the frontend's stream parser to drive the UI (open detail panes,
    // switch views, add editor instances, surface connect modals).
    // Always wired — no creds needed.
    // IPC socket + session id are forwarded so propose_bash /
    // propose_switch_cwd can block on user approval the same way
    // propose_commit / propose_pr do in woom-github.
    if let Some(app) = build_app_server(session_id, &ipc_str) {
        servers.insert("app".into(), app);
        allowed.push("mcp__app__open_github_pr".into());
        allowed.push("mcp__app__open_github_issue".into());
        allowed.push("mcp__app__open_jira_issue".into());
        allowed.push("mcp__app__open_sentry_issue".into());
        allowed.push("mcp__app__switch_view".into());
        allowed.push("mcp__app__open_connect_modal".into());
        allowed.push("mcp__app__add_app_instance".into());
        allowed.push("mcp__app__open_github_repo".into());
        allowed.push("mcp__app__open_jira_tab".into());
        allowed.push("mcp__app__open_sentry_tab".into());
        allowed.push("mcp__app__set_github_instance".into());
        allowed.push("mcp__app__set_jira_instance".into());
        allowed.push("mcp__app__set_sentry_instance".into());
        allowed.push("mcp__app__set_editor_repo_path".into());
        allowed.push("mcp__app__set_agent_cwd".into());
        allowed.push("mcp__app__list_instances".into());
        allowed.push("mcp__app__open_sentry_event".into());
        // Canvas (whiteboard) tools — every linked-canvas mutation goes
        // through one of these. Without explicit allow-listing the
        // agent sees them in the MCP catalog but `--allowedTools`
        // strips them, so it answers "I have no canvas tools" even
        // though the sidecar exposes them. (The `Coding` profile
        // lets all `mcp__app__*` through — see `Profile::allows`.)
        allowed.push("mcp__app__canvas_add_shape".into());
        allowed.push("mcp__app__canvas_add_shapes".into());
        allowed.push("mcp__app__canvas_update_shape".into());
        allowed.push("mcp__app__canvas_delete_shape".into());
        allowed.push("mcp__app__canvas_add_edge".into());
        allowed.push("mcp__app__canvas_add_edges".into());
        allowed.push("mcp__app__canvas_delete_edge".into());
        allowed.push("mcp__app__canvas_arrange".into());
        allowed.push("mcp__app__canvas_focus".into());
        allowed.push("mcp__app__canvas_set_z".into());
        allowed.push("mcp__app__canvas_duplicate".into());
        allowed.push("mcp__app__canvas_find".into());
        allowed.push("mcp__app__canvas_group".into());
        allowed.push("mcp__app__canvas_ungroup".into());
        allowed.push("mcp__app__canvas_lock".into());
        allowed.push("mcp__app__canvas_align".into());
        allowed.push("mcp__app__canvas_distribute".into());
        allowed.push("mcp__app__canvas_set_viewport".into());
        allowed.push("mcp__app__canvas_upload_image".into());
        // Terminal tools — drive the user-visible PTY column. These
        // are no more privileged than the Bash tool (same shell, same
        // env) so they belong in the always-allowed set; without
        // them Claude CLI prompts the user on every `terminal_run`,
        // which is exactly the "endless permission prompts" symptom.
        allowed.push("mcp__app__ensure_terminal".into());
        allowed.push("mcp__app__terminal_list".into());
        allowed.push("mcp__app__terminal_run".into());
        allowed.push("mcp__app__terminal_write".into());
        allowed.push("mcp__app__terminal_buffer".into());
        allowed.push("mcp__app__propose_bash".into());
        allowed.push("mcp__app__propose_switch_cwd".into());
    }

    // Merge in any third-party MCP servers the user has installed via
    // `claude mcp add ...` — they live in `~/.claude.json` under top-level
    // `mcpServers` and per-project `projects.<path>.mcpServers`. Woom's
    // built-ins win on name collisions so we never instantiate two of the
    // same server; everything else (godot, brave-search, filesystem, …)
    // "just works" without per-server plumbing in this module.
    //
    // The allowed-tools entry is the server-wide form `mcp__<name>`, which
    // Claude CLI accepts as "every tool from this server". That avoids
    // having to enumerate a third-party server's tool list at config time.
    for (name, def) in user_mcp_servers() {
        if servers.contains_key(&name) {
            continue;
        }
        servers.insert(name.clone(), def);
        allowed.push(format!("mcp__{name}"));
    }

    if servers.is_empty() {
        return None;
    }

    let config = serde_json::json!({ "mcpServers": servers });
    let body = serde_json::to_string(&config).ok()?;
    let path = std::env::temp_dir().join(format!("woom-mcp-{}.json", session_id));
    std::fs::write(&path, body).ok()?;
    Some((path, allowed))
}

fn build_jira_server() -> Option<serde_json::Value> {
    let stored = keychain::get(JIRA_KEYCHAIN_KEY).ok().flatten()?;
    let creds: JiraCredentials = serde_json::from_str(&stored).ok()?;
    let sidecar = find_sidecar("woom-jira")?;
    Some(serde_json::json!({
        "command": sidecar.to_string_lossy(),
        "env": {
            "JIRA_WORKSPACE": normalize_workspace(&creds.workspace),
            "JIRA_EMAIL": creds.email,
            "JIRA_TOKEN": creds.token,
        }
    }))
}

fn build_github_server(session_id: &str, ipc_socket: &str) -> Option<serde_json::Value> {
    let token = keychain::get(GITHUB_KEYCHAIN_KEY).ok().flatten()?;
    if token.trim().is_empty() {
        return None;
    }
    let sidecar = find_sidecar("woom-github")?;
    // Plumb the action-IPC socket path + session id so the sidecar's
    // `propose_*` tools can reach the Tauri shell to BLOCK on user
    // approval — without these, propose_bash et al. fall back to the
    // legacy "card created, end turn" stub. Empty socket string is
    // a sentinel meaning "IPC unavailable" and triggers that fallback.
    Some(serde_json::json!({
        "command": sidecar.to_string_lossy(),
        "env": {
            "GITHUB_TOKEN": token,
            "WOOM_IPC_SOCKET": ipc_socket,
            "WOOM_SESSION_ID": session_id,
        }
    }))
}

fn build_sentry_server() -> Option<serde_json::Value> {
    let stored = keychain::get(SENTRY_KEYCHAIN_KEY).ok().flatten()?;
    let creds: SentryCredentials = serde_json::from_str(&stored).ok()?;
    let sidecar = find_sidecar("woom-sentry")?;
    Some(serde_json::json!({
        "command": sidecar.to_string_lossy(),
        "env": {
            "SENTRY_HOST": creds.host,
            "SENTRY_ORG": creds.organization_slug,
            "SENTRY_TOKEN": creds.token,
        }
    }))
}

/// Wire up the bundled `woom-memory` sidecar — a SQLite-backed notes store
/// exposed via MCP. Ships with the .app, no external install. Persists under
/// the app's data dir (`~/Library/Application Support/Woom/memory.db` on
/// macOS) so notes survive across sessions.
fn build_memory_server() -> Option<serde_json::Value> {
    let sidecar = find_sidecar("woom-memory")?;
    let db_path = app_data_dir().map(|d| d.join("memory.db"));
    let db_str = db_path
        .as_ref()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    Some(serde_json::json!({
        "command": sidecar.to_string_lossy(),
        "env": {
            "WOOM_MEMORY_DB": db_str,
        }
    }))
}

/// Wire up the bundled `woom-app` sidecar — exposes UI navigation
/// tools (open detail panes, switch views, add editor columns). Tool
/// calls are intercepted by the frontend's stream parser; the sidecar
/// is intentionally thin (just registers the schemas).
/// `session_id` and `ipc_socket` are forwarded so propose_bash /
/// propose_switch_cwd can reach the Tauri action-IPC socket and block
/// on user approval.
fn build_app_server(session_id: &str, ipc_socket: &str) -> Option<serde_json::Value> {
    let sidecar = find_sidecar("woom-app")?;
    Some(serde_json::json!({
        "command": sidecar.to_string_lossy(),
        "env": {
            "WOOM_IPC_SOCKET": ipc_socket,
            "WOOM_SESSION_ID": session_id,
        }
    }))
}

/// Per-platform app data directory. Mirrors Tauri's default app-config dir
/// but keeps this module self-contained (no `AppHandle` threaded through).
fn app_data_dir() -> Option<PathBuf> {
    let home = claude::home_dir()?;
    #[cfg(target_os = "macos")]
    let dir = home.join("Library/Application Support/Woom");
    #[cfg(target_os = "linux")]
    let dir = home.join(".local/share/woom");
    #[cfg(target_os = "windows")]
    let dir = home.join("AppData/Roaming/Woom");
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

/// Scan `~/.claude.json` for MCP servers the user has registered via
/// `claude mcp add ...`. Pulls from both the top-level `mcpServers` map
/// (user-scope) and any per-project map under `projects.<path>.mcpServers`
/// (local-scope, which is the `claude mcp add` default). Server definitions
/// are returned as raw JSON so they can be inserted straight into our temp
/// config alongside Woom's bundled sidecars.
///
/// Deduplicated by name across both scopes — a server registered in both
/// places only appears once. Caller is responsible for skipping names that
/// already exist in Woom's own server map.
fn user_mcp_servers() -> Vec<(String, serde_json::Value)> {
    let Some(home) = claude::home_dir() else {
        return Vec::new();
    };
    let path = home.join(".claude.json");
    let Ok(body) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&body) else {
        return Vec::new();
    };

    let mut out: Vec<(String, serde_json::Value)> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    if let Some(map) = json.get("mcpServers").and_then(|v| v.as_object()) {
        for (k, v) in map {
            if seen.insert(k.clone()) {
                out.push((k.clone(), v.clone()));
            }
        }
    }
    if let Some(projects) = json.get("projects").and_then(|v| v.as_object()) {
        for pdata in projects.values() {
            if let Some(map) = pdata.get("mcpServers").and_then(|v| v.as_object()) {
                for (k, v) in map {
                    if seen.insert(k.clone()) {
                        out.push((k.clone(), v.clone()));
                    }
                }
            }
        }
    }

    out
}
