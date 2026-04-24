//! Claude Code CLI detection.
//!
//! Forgehold does NOT own Claude Code authentication — the `claude` CLI manages
//! its own auth (subscription via `claude login`, or API key via
//! `ANTHROPIC_API_KEY`). We just detect whether the CLI is installed and
//! whether it appears to be configured.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

use serde::Serialize;

use crate::jira::{JiraCredentials, normalize_workspace};
use crate::keychain;

const JIRA_KEYCHAIN_KEY: &str = "jira";
const GITHUB_KEYCHAIN_KEY: &str = "github";

/// Session id → pid of the running `claude` process. Lets us kill it later.
pub type Runners = Arc<Mutex<HashMap<String, u32>>>;

pub fn new_runners() -> Runners {
    Arc::new(Mutex::new(HashMap::new()))
}

#[derive(Debug, Serialize, Clone)]
pub struct ClaudeStatus {
    /// `claude` binary found on PATH.
    pub detected: bool,
    /// Path to the binary if detected.
    pub path: Option<String>,
    /// Output of `claude --version`, trimmed.
    pub version: Option<String>,
    /// `~/.claude` dir exists — usually means `claude login` has been run,
    /// i.e. a Claude subscription session is configured.
    pub has_config_dir: bool,
    /// `ANTHROPIC_API_KEY` env var is set in our process env.
    pub has_api_key_env: bool,
    /// High-level bool for the UI: detected + (config dir or API key env var).
    pub ready: bool,
}

pub fn detect() -> ClaudeStatus {
    let path = which("claude");
    let detected = path.is_some();

    let version = path.as_ref().and_then(|p| read_version(p));
    let has_config_dir = home_dir()
        .map(|h| h.join(".claude").is_dir())
        .unwrap_or(false);
    let has_api_key_env = std::env::var("ANTHROPIC_API_KEY").is_ok();

    ClaudeStatus {
        detected,
        path: path.map(|p| p.display().to_string()),
        version,
        has_config_dir,
        has_api_key_env,
        ready: detected && (has_config_dir || has_api_key_env),
    }
}

fn which(name: &str) -> Option<PathBuf> {
    // Start with whatever PATH the process has (hydrated from the login shell
    // in `lib::run`), then fall back to the usual suspects in case the shell
    // hydration didn't cover them.
    let mut candidates: Vec<String> = Vec::new();
    if let Ok(p) = std::env::var("PATH") {
        for dir in p.split(':') {
            if !dir.is_empty() {
                candidates.push(dir.to_string());
            }
        }
    }
    let fallbacks = ["/opt/homebrew/bin", "/usr/local/bin"];
    for extra in fallbacks {
        if !candidates.iter().any(|d| d == extra) {
            candidates.push(extra.into());
        }
    }
    if let Some(h) = home_dir() {
        for sub in [".local/bin", ".claude/local/bin", ".claude/local", ".bun/bin", ".volta/bin"] {
            let full = h.join(sub).to_string_lossy().into_owned();
            if !candidates.iter().any(|d| d == &full) {
                candidates.push(full);
            }
        }
    }
    for dir in candidates {
        let candidate = Path::new(&dir).join(name);
        if is_executable(&candidate) {
            return Some(candidate);
        }
    }
    None
}

fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    match std::fs::metadata(path) {
        Ok(m) => m.is_file() && (m.permissions().mode() & 0o111 != 0),
        Err(_) => false,
    }
}

fn read_version(path: &Path) -> Option<String> {
    let output = Command::new(path).arg("--version").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&output.stdout).to_string();
    let trimmed = s.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn home_dir() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(PathBuf::from)
}

#[derive(Debug, thiserror::Error)]
pub enum ClaudeRunError {
    #[error("claude CLI is not installed — run `curl -fsSL https://claude.ai/install.sh | bash` or see https://docs.claude.com/en/docs/claude-code/overview")]
    NotInstalled,
    #[error("claude is not authenticated — run `claude login` in your terminal or set ANTHROPIC_API_KEY")]
    NotAuthed,
    #[error("claude failed: {0}")]
    Failed(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Run `claude -p <prompt> --output-format stream-json --verbose` and stream
/// JSONL events to the frontend via `claude:stream:{session_id}` events.
/// Returns the final result text (from the `result` event) when done.
///
/// `claude_uuid` is the Claude CLI session UUID (independent of forge's own
/// `session_id`). When `resume` is false we pass `--session-id <uuid>` so the
/// CLI creates a new persisted session; when true we pass `--resume <uuid>`
/// so it continues the existing one with full history. This gives forge
/// chat-level memory equivalent to native `claude` interactive sessions.
pub async fn ask(
    app: tauri::AppHandle,
    runners: Runners,
    session_id: &str,
    prompt: &str,
    cwd: Option<&Path>,
    claude_uuid: &str,
    resume: bool,
    rules: Option<&str>,
) -> Result<String, ClaudeRunError> {
    use tauri::Emitter;
    use tokio::io::AsyncBufReadExt;

    let status = detect();
    if !status.detected {
        return Err(ClaudeRunError::NotInstalled);
    }
    if !status.ready {
        return Err(ClaudeRunError::NotAuthed);
    }
    let bin = status.path.as_deref().unwrap_or("claude");

    let mcp = build_mcp_config(session_id);
    // Drop guard ensures the temp config file is removed on every exit path.
    let _mcp_guard = mcp.as_ref().map(|(p, _)| TempFile(p.clone()));

    let mut cmd = tokio::process::Command::new(bin);
    cmd.arg("-p")
        .arg(prompt)
        .arg("--output-format")
        .arg("stream-json")
        .arg("--verbose");
    if resume {
        cmd.arg("--resume").arg(claude_uuid);
    } else {
        cmd.arg("--session-id").arg(claude_uuid);
    }
    if let Some(r) = rules.map(|s| s.trim()).filter(|s| !s.is_empty()) {
        // User-authored rules from the Rules tab — appended to the system
        // prompt so Claude respects them on every turn without us having to
        // bake them into each user message.
        let wrapped = format!("User rules (follow these on every turn):\n\n{}", r);
        cmd.arg("--append-system-prompt").arg(wrapped);
    }
    if let Some((path, allowed)) = &mcp {
        cmd.arg("--mcp-config").arg(path);
        if !allowed.is_empty() {
            cmd.arg("--allowedTools").arg(allowed.join(","));
        }
    }
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    // Tauri-launched apps don't inherit shell PATH. Augment with common paths
    // beyond what `hydrate_path_from_login_shell` already set — belt & braces
    // for users with unusual shell configs or shells that don't export PATH.
    if let Ok(p) = std::env::var("PATH") {
        let mut parts: Vec<String> = p.split(':').map(String::from).collect();
        for e in ["/opt/homebrew/bin", "/usr/local/bin"] {
            if !parts.iter().any(|d| d == e) {
                parts.push(e.into());
            }
        }
        if let Some(h) = home_dir() {
            for sub in [".local/bin", ".claude/local/bin", ".claude/local", ".bun/bin", ".volta/bin"] {
                let full = h.join(sub).to_string_lossy().into_owned();
                if !parts.iter().any(|d| d == &full) {
                    parts.push(full);
                }
            }
        }
        cmd.env("PATH", parts.join(":"));
    }
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let mut child = cmd.spawn()?;
    let pid = child.id().unwrap_or(0);
    if pid != 0 {
        runners.lock().unwrap().insert(session_id.to_string(), pid);
    }

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| ClaudeRunError::Failed("no stdout".into()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| ClaudeRunError::Failed("no stderr".into()))?;
    let reader = tokio::io::BufReader::new(stdout);
    let mut lines = reader.lines();

    // Drain stderr concurrently so the pipe buffer doesn't fill up and block
    // the child (older behavior: stderr was piped but never read). Keep a
    // capped tail so a runaway CLI can't blow up our memory.
    let stderr_buf = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let stderr_buf_task = stderr_buf.clone();
    let stderr_task = tokio::spawn(async move {
        use tokio::io::AsyncReadExt;
        const MAX_STDERR: usize = 16 * 1024;
        let mut reader = tokio::io::BufReader::new(stderr);
        let mut chunk = [0u8; 2048];
        loop {
            match reader.read(&mut chunk).await {
                Ok(0) => break,
                Ok(n) => {
                    let s = String::from_utf8_lossy(&chunk[..n]);
                    let mut buf = stderr_buf_task.lock().unwrap();
                    buf.push_str(&s);
                    if buf.len() > MAX_STDERR {
                        // Keep only the tail — that's where the actual error
                        // typically lives when the CLI fails.
                        let start = buf.len() - MAX_STDERR;
                        *buf = buf[start..].to_string();
                    }
                }
                Err(_) => break,
            }
        }
    });

    let event_name = format!("claude:stream:{}", session_id);
    let mut final_text = String::new();

    while let Ok(Some(line)) = lines.next_line().await {
        let _ = app.emit(&event_name, &line);
        // Capture final result if the CLI emits one
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) {
            if v.get("type").and_then(|t| t.as_str()) == Some("result") {
                if let Some(r) = v.get("result").and_then(|r| r.as_str()) {
                    final_text = r.to_string();
                }
            }
        }
    }

    let out = child.wait().await;
    let _ = stderr_task.await;
    runners.lock().unwrap().remove(session_id);
    let out = out?;

    if let Some(code) = out.code() {
        if code == 143 {
            return Err(ClaudeRunError::Failed("cancelled".into()));
        }
    }
    if !out.success() {
        let stderr_tail = stderr_buf.lock().unwrap().trim().to_string();
        let code = out.code().unwrap_or(-1);
        let msg = if stderr_tail.is_empty() {
            format!("exit code {code}")
        } else {
            format!("exit code {code} — {stderr_tail}")
        };
        return Err(ClaudeRunError::Failed(msg));
    }
    Ok(final_text)
}

/// One-off headless Claude call: feed it the staged git diff from `repo`
/// and return a single-line commit message. Separate from the chat-session
/// `ask()` path — uses a throwaway session so it never lands in the user's
/// chat history or the active linked-column transcript.
pub async fn generate_commit_message(repo: &std::path::Path) -> Result<String, ClaudeRunError> {
    let status = detect();
    if !status.detected {
        return Err(ClaudeRunError::NotInstalled);
    }
    if !status.ready {
        return Err(ClaudeRunError::NotAuthed);
    }
    let bin = status.path.as_deref().unwrap_or("claude");

    // Pull the staged diff. If nothing's staged, caller asked for a commit
    // message out of nothing — surface that explicitly.
    let diff_out = tokio::process::Command::new("git")
        .current_dir(repo)
        .args(["diff", "--cached"])
        .output()
        .await?;
    if !diff_out.status.success() {
        let stderr = String::from_utf8_lossy(&diff_out.stderr).trim().to_string();
        return Err(ClaudeRunError::Failed(format!(
            "git diff --cached failed: {}",
            if stderr.is_empty() { "unknown error".into() } else { stderr }
        )));
    }
    let diff = String::from_utf8_lossy(&diff_out.stdout);
    if diff.trim().is_empty() {
        return Err(ClaudeRunError::Failed(
            "Nothing is staged — stage at least one change before asking for a commit message.".into(),
        ));
    }

    // Tight prompt — we want just the subject line, no chatter.
    let prompt = format!(
        "Write a single git commit message for the following staged diff.\n\n\
         Rules:\n\
         - Imperative mood (\"Add X\", \"Fix Y\", not \"Added X\").\n\
         - Under 72 characters.\n\
         - No quotes, no markdown, no preamble, no explanation.\n\
         - Output ONLY the commit message on a single line.\n\n\
         ```diff\n{diff}```"
    );

    let mut cmd = tokio::process::Command::new(bin);
    cmd.arg("-p").arg(&prompt);
    cmd.current_dir(repo);
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    // Mirror the PATH augmentation from `ask()` — Tauri-launched processes
    // inherit a skinny PATH, and the same `claude` binary lookup pattern
    // applies here.
    if let Ok(p) = std::env::var("PATH") {
        let mut parts: Vec<String> = p.split(':').map(String::from).collect();
        for e in ["/opt/homebrew/bin", "/usr/local/bin"] {
            if !parts.iter().any(|d| d == e) {
                parts.push(e.into());
            }
        }
        if let Some(h) = home_dir() {
            for sub in [".local/bin", ".claude/local/bin", ".claude/local", ".bun/bin", ".volta/bin"] {
                let full = h.join(sub).to_string_lossy().into_owned();
                if !parts.iter().any(|d| d == &full) {
                    parts.push(full);
                }
            }
        }
        cmd.env("PATH", parts.join(":"));
    }

    let out = cmd.output().await?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        let code = out.status.code().unwrap_or(-1);
        return Err(ClaudeRunError::Failed(format!(
            "claude exited {code}{}",
            if stderr.is_empty() { String::new() } else { format!(" — {stderr}") }
        )));
    }
    let raw = String::from_utf8_lossy(&out.stdout).trim().to_string();
    // Strip common wrappers the model sometimes adds even when instructed
    // otherwise (backticks, quotes, a trailing period run-on). Keep it
    // single-line — the commit-message field is one-line.
    let cleaned = raw
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .trim_matches(|c: char| c == '"' || c == '\'' || c == '`')
        .to_string();
    if cleaned.is_empty() {
        return Err(ClaudeRunError::Failed("claude returned an empty response".into()));
    }
    Ok(cleaned)
}

/// Removes its path on drop. Used to clean up the temp MCP config.
struct TempFile(PathBuf);

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Build an MCP config file for this session by pulling creds from Keychain
/// and wiring up available sidecars. Returns the temp config path and the
/// list of tool names to allow (one entry per tool, explicit — wildcards are
/// not universally supported by `claude -p --allowedTools`).
///
/// Returns `None` when no sidecars can be configured (e.g. Jira is not
/// connected or the sidecar binary isn't next to the main exe); in that case
/// we run `claude` without MCP, preserving the old behavior.
fn build_mcp_config(session_id: &str) -> Option<(PathBuf, Vec<String>)> {
    let mut servers = serde_json::Map::new();
    let mut allowed: Vec<String> = Vec::new();

    if let Some(jira) = build_jira_server() {
        servers.insert("jira".into(), jira);
        allowed.push("mcp__jira__get_issue".into());
        allowed.push("mcp__jira__search".into());
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

/// Per-platform app data directory. Mirrors Tauri's default app-config dir
/// but keeps this module self-contained (no `AppHandle` threaded through).
fn app_data_dir() -> Option<PathBuf> {
    let home = home_dir()?;
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

/// Send SIGTERM to a running claude spawn for the given session.
pub fn stop(runners: &Runners, session_id: &str) -> bool {
    let pid = runners.lock().unwrap().get(session_id).copied();
    match pid {
        Some(pid) if pid > 0 => {
            unsafe {
                libc::kill(pid as i32, libc::SIGTERM);
            }
            true
        }
        _ => false,
    }
}
