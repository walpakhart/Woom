//! Cursor MCP bridge — exposes Woom's Jira/GitHub/Sentry/Memory/App
//! sidecars to `cursor-agent` so Cursor sees the same tools as Claude.
//!
//! Cursor reads its MCP config from `~/.cursor/mcp.json` (and from
//! `<workspace>/.cursor/mcp.json`). It does NOT accept a `--mcp-config` flag
//! the way `claude` does. To give Cursor parity we merge our own server
//! entries into `~/.cursor/mcp.json` whenever the user's connection state
//! changes (connect/disconnect → keychain mutation → re-sync).
//!
//! ## Ownership = prefix, not marker
//!
//! Anything in `mcpServers` whose key starts with one of `OWNED_PREFIXES`
//! (`woom-`) is treated as ours: dropped on every sync, then re-added
//! from current Keychain state. Entries the user added by hand via
//! `cursor-agent mcp add` (any other name) survive. We also strip the
//! marker key we used to write (`_woom_managed`) — it's advisory and
//! re-emitted below, but the cleanup loop never reads it. This makes
//! sync self-correcting: rename a sidecar / drop a server / ship a
//! rebrand → next start of Woom converges the file to whatever the
//! current code declares, no migration code required.
//!
//! Security note: the resulting `~/.cursor/mcp.json` contains plaintext
//! tokens (the `env` block is not Keychain-resolved by cursor-agent). This
//! matches the threat model for `~/.aws/credentials` and the manual
//! `cursor-agent mcp add --env` flow — but is weaker than Woom's own
//! Keychain-only storage. We accept the trade-off because users opting into
//! Cursor want parity; if they don't, they can simply not use Cursor.

use std::path::PathBuf;

use serde_json::{Map, Value};

use crate::action_ipc;
use crate::keychain;

/// Sentinel `WOOM_SESSION_ID` written into `~/.cursor/mcp.json`. The
/// file is global (single config across all Cursor sessions) so we
/// can't bake a per-session id — instead we ship this constant and
/// the frontend's action-IPC handler routes any card carrying it to
/// the user's currently-focused Cursor session. Single-Cursor-session
/// flows (the common case) get correct routing for free; parallel
/// sessions all post to the focused one, which is the least-surprising
/// fallback short of dynamic per-session config (cursor-agent has no
/// `--mcp-config` flag — see `cursor.rs` module docs).
const CURSOR_SENTINEL_SESSION_ID: &str = "cursor";

/// Server-name prefixes that identify an entry as ours. Entries matching
/// any of these are blown away on every sync and re-derived from current
/// Keychain state. Add new prefixes here on a rebrand if the historical
/// install base ever needs cleanup again.
const OWNED_PREFIXES: &[&str] = &["woom-"];

/// Top-level marker key we write into the file. Stripped on every sync
/// and re-emitted below, so the file always has at most one. Cleanup
/// logic doesn't read it — it exists purely so a human glancing at
/// `~/.cursor/mcp.json` can see what Woom thinks it currently owns.
const MARKER_KEY: &str = "_woom_managed";
const LEGACY_MARKER_KEYS: &[&str] = &[];

const JIRA_KEYCHAIN_KEY: &str = "jira";
const GITHUB_KEYCHAIN_KEY: &str = "github";
const SENTRY_KEYCHAIN_KEY: &str = "sentry";
const SIDECAR_JIRA: &str = "woom-jira";
const SIDECAR_GITHUB: &str = "woom-github";
const SIDECAR_SENTRY: &str = "woom-sentry";
const SIDECAR_MEMORY: &str = "woom-memory";
const SIDECAR_APP: &str = "woom-app";

/// Re-derive the set of Woom-owned MCP entries from current Keychain
/// state and merge them into `~/.cursor/mcp.json`. Idempotent — call this
/// after every connect/disconnect, or once at app startup.
///
/// Returns a list of server names we wrote (or empty when no creds were
/// available). Errors are returned as `String` so callers can log without
/// importing our error types.
pub fn sync() -> Result<Vec<String>, String> {
    let cfg_path = cursor_mcp_path().ok_or_else(|| "no $HOME for ~/.cursor/mcp.json".to_string())?;

    // Load existing config. Missing file or malformed JSON → start with an
    // empty doc rather than losing the user's other entries on the first
    // run; mid-flight corruption shouldn't take down a side feature.
    let mut doc: Map<String, Value> = match std::fs::read_to_string(&cfg_path) {
        Ok(s) if !s.trim().is_empty() => serde_json::from_str::<Value>(&s)
            .ok()
            .and_then(|v| v.as_object().cloned())
            .unwrap_or_default(),
        _ => Map::new(),
    };

    // Drop legacy + current marker keys. We re-emit the current one below;
    // never read them for cleanup logic — ownership is by prefix (see below).
    doc.remove(MARKER_KEY);
    for k in LEGACY_MARKER_KEYS {
        doc.remove(*k);
    }

    let servers = doc
        .entry("mcpServers".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    let servers_obj = match servers {
        Value::Object(o) => o,
        _ => {
            // Cursor itself would reject a non-object here, so reset rather
            // than crash. Safer than trying to coerce.
            *servers = Value::Object(Map::new());
            servers.as_object_mut().unwrap()
        }
    };

    // Prefix sweep: anything matching an owned prefix is ours, drop it.
    // Entries the user added by hand (any other name) stay untouched.
    // This is what makes sync self-correcting across renames, removals,
    // and rebrands — we don't trust a marker, we trust the namespace.
    servers_obj.retain(|name, _| !OWNED_PREFIXES.iter().any(|p| name.starts_with(p)));

    // Re-build current entries from Keychain.
    let sidecar_dir = sidecar_dir_for_running_app();
    let ipc_socket = action_ipc::current_socket_path()
        .to_string_lossy()
        .into_owned();
    let mut written: Vec<String> = Vec::new();

    if let Some(cfg) = build_jira_entry(&sidecar_dir) {
        servers_obj.insert(SIDECAR_JIRA.into(), cfg);
        written.push(SIDECAR_JIRA.into());
    }
    if let Some(cfg) = build_github_entry(&sidecar_dir, &ipc_socket) {
        servers_obj.insert(SIDECAR_GITHUB.into(), cfg);
        written.push(SIDECAR_GITHUB.into());
    }
    if let Some(cfg) = build_sentry_entry(&sidecar_dir) {
        servers_obj.insert(SIDECAR_SENTRY.into(), cfg);
        written.push(SIDECAR_SENTRY.into());
    }
    if let Some(cfg) = build_memory_entry(&sidecar_dir) {
        servers_obj.insert(SIDECAR_MEMORY.into(), cfg);
        written.push(SIDECAR_MEMORY.into());
    }
    // woom-app is unconditional — no creds, just exposes the
    // navigation tools (`mcp__app__open_jira_issue`, `switch_view`, …)
    // that Claude already gets via `--mcp-config`. Without this, Cursor
    // saw the tools mentioned in the system preamble but couldn't
    // actually call them; here we give it parity.
    if let Some(cfg) = build_app_entry(&sidecar_dir, &ipc_socket) {
        servers_obj.insert(SIDECAR_APP.into(), cfg);
        written.push(SIDECAR_APP.into());
    }

    doc.insert(
        MARKER_KEY.into(),
        Value::Array(written.iter().cloned().map(Value::String).collect()),
    );

    if let Some(parent) = cfg_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let serialized = serde_json::to_string_pretty(&Value::Object(doc)).map_err(|e| e.to_string())?;
    std::fs::write(&cfg_path, serialized).map_err(|e| e.to_string())?;
    Ok(written)
}

fn cursor_mcp_path() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok().map(PathBuf::from)?;
    Some(home.join(".cursor").join("mcp.json"))
}

/// Locate the sidecar directory the *currently running* Woom app uses.
/// In dev that's `target/{debug,release}/`, in a bundle it's the .app
/// `Contents/MacOS/`. We resolve relative to the executable rather than
/// hard-coding so dev + bundle both work.
fn sidecar_dir_for_running_app() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    exe.parent().map(|p| p.to_path_buf())
}

fn sidecar_path(dir: &Option<PathBuf>, name: &str) -> Option<String> {
    let p = dir.as_ref()?.join(name);
    if p.is_file() {
        Some(p.to_string_lossy().into_owned())
    } else {
        None
    }
}

fn build_jira_entry(dir: &Option<PathBuf>) -> Option<Value> {
    let stored = keychain::get(JIRA_KEYCHAIN_KEY).ok().flatten()?;
    #[derive(serde::Deserialize)]
    struct Creds {
        workspace: String,
        email: String,
        token: String,
    }
    let creds: Creds = serde_json::from_str(&stored).ok()?;
    let cmd = sidecar_path(dir, SIDECAR_JIRA)?;
    Some(serde_json::json!({
        "command": cmd,
        "env": {
            "JIRA_WORKSPACE": creds.workspace,
            "JIRA_EMAIL": creds.email,
            "JIRA_TOKEN": creds.token,
        }
    }))
}

fn build_github_entry(dir: &Option<PathBuf>, ipc_socket: &str) -> Option<Value> {
    let token = keychain::get(GITHUB_KEYCHAIN_KEY).ok().flatten()?;
    if token.trim().is_empty() {
        return None;
    }
    let cmd = sidecar_path(dir, SIDECAR_GITHUB)?;
    // IPC socket + session-id sentinel let `propose_commit` / `propose_pr`
    // reach Woom's action-IPC and block on user approval — same shape
    // `claude_mcp::build_github_server` writes for Claude. Without these
    // the sidecar's `propose_*` tools error out with "WOOM_IPC_SOCKET not
    // set" and the agent gets a non-blocking stub response.
    Some(serde_json::json!({
        "command": cmd,
        "env": {
            "GITHUB_TOKEN": token,
            "WOOM_IPC_SOCKET": ipc_socket,
            "WOOM_SESSION_ID": CURSOR_SENTINEL_SESSION_ID,
        }
    }))
}

fn build_sentry_entry(dir: &Option<PathBuf>) -> Option<Value> {
    let stored = keychain::get(SENTRY_KEYCHAIN_KEY).ok().flatten()?;
    // Match the field names in `crate::sentry::SentryCredentials` exactly,
    // including `organization_slug` (vs the older `org` shorthand). Serde
    // is strict on field names, so a mismatch would silently return `None`
    // and Cursor would never see the Sentry entry.
    #[derive(serde::Deserialize)]
    struct Creds {
        host: String,
        #[serde(rename = "organization_slug")]
        organization_slug: String,
        token: String,
    }
    let creds: Creds = serde_json::from_str(&stored).ok()?;
    let cmd = sidecar_path(dir, SIDECAR_SENTRY)?;
    Some(serde_json::json!({
        "command": cmd,
        "env": {
            "SENTRY_HOST": creds.host,
            "SENTRY_ORG": creds.organization_slug,
            "SENTRY_TOKEN": creds.token,
        }
    }))
}

fn build_memory_entry(dir: &Option<PathBuf>) -> Option<Value> {
    let cmd = sidecar_path(dir, SIDECAR_MEMORY)?;
    let home = std::env::var("HOME").ok().map(PathBuf::from)?;
    #[cfg(target_os = "macos")]
    let app_data = home.join("Library/Application Support/Woom");
    #[cfg(target_os = "linux")]
    let app_data = home.join(".local/share/woom");
    #[cfg(target_os = "windows")]
    let app_data = home.join("AppData/Roaming/Woom");
    let _ = std::fs::create_dir_all(&app_data);
    Some(serde_json::json!({
        "command": cmd,
        "env": {
            "WOOM_MEMORY_DB": app_data.join("memory.db").to_string_lossy(),
        }
    }))
}

/// Wire up the bundled `woom-app` sidecar — exposes UI navigation
/// tools (`open_github_pr`, `open_jira_issue`, `switch_view`, …). The
/// frontend's stream parser intercepts the `mcp__app__*` tool_use events
/// and drives the Svelte state directly; the sidecar itself just
/// publishes the schemas so cursor-agent surfaces them as callable.
/// Always wired — no creds needed, mirrors `claude_mcp::build_app_server`.
///
/// `WOOM_IPC_SOCKET` + `WOOM_SESSION_ID` mirror Claude's setup so the
/// blocking `propose_bash` / `propose_switch_cwd` tools can reach the
/// approval-card pipeline. The session id is a sentinel (`CURSOR_SENTINEL_SESSION_ID`)
/// — the global mcp.json can't carry a per-session value, and the
/// frontend's IPC handler routes any card carrying it to the currently-
/// focused Cursor chat (see `routes/+page.svelte::handleActionRequest`).
fn build_app_entry(dir: &Option<PathBuf>, ipc_socket: &str) -> Option<Value> {
    let cmd = sidecar_path(dir, SIDECAR_APP)?;
    Some(serde_json::json!({
        "command": cmd,
        "env": {
            "WOOM_IPC_SOCKET": ipc_socket,
            "WOOM_SESSION_ID": CURSOR_SENTINEL_SESSION_ID,
        }
    }))
}
