//! Cursor Agent CLI adapter — parallel to `claude.rs`.
//!
//! Forge spawns `cursor-agent -p <prompt> --output-format stream-json
//! --resume <chat_id>` and normalizes Cursor's native event stream into the
//! same Claude-style shape the frontend already parses (`type: "assistant"`
//! with a `content[]` array of `{type: "text"|"tool_use", …}` blocks). That
//! way `handleStreamEvent` in `+page.svelte` doesn't need a second code path.
//!
//! Key differences from the Claude adapter:
//!   - Cursor won't accept a caller-chosen session UUID. On the first turn we
//!     call `cursor-agent create-chat` to mint one; subsequent turns use
//!     `--resume <id>`. The generated ID is returned to the frontend so it
//!     can store it on the session for future calls.
//!   - No `--append-system-prompt` — user-authored Rules are prepended to
//!     the prompt text as a fenced preamble.
//!   - No `--mcp-config` — Cursor reads `~/.cursor/mcp.json` and
//!     `<workspace>/.cursor/mcp.json`. Forge's sidecars aren't wired in by
//!     default in v1; users can add them manually via `cursor-agent mcp
//!     enable` or a project-local mcp.json.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;
use serde_json::json;

use crate::claude::Runners;

#[derive(Debug, Serialize, Clone)]
pub struct CursorStatus {
    pub detected: bool,
    pub path: Option<String>,
    pub version: Option<String>,
    pub ready: bool,
}

pub fn detect() -> CursorStatus {
    let path = which("cursor-agent");
    let detected = path.is_some();
    let version = path.as_ref().and_then(|p| read_version(p));
    // cursor-agent stores credentials under ~/.cursor; presence of the dir
    // is our cheap proxy for "authenticated". For API-key auth the env var
    // CURSOR_API_KEY works too.
    let has_cursor_dir = home_dir()
        .map(|h| h.join(".cursor").is_dir())
        .unwrap_or(false);
    let has_api_key = std::env::var("CURSOR_API_KEY").is_ok();
    CursorStatus {
        detected,
        path: path.map(|p| p.display().to_string()),
        version,
        ready: detected && (has_cursor_dir || has_api_key),
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CursorRunError {
    #[error("cursor-agent CLI is not installed — run `curl -fsS https://cursor.com/install -o- | bash` or see https://cursor.com/docs/cli/installation")]
    NotInstalled,
    #[error("cursor-agent is not authenticated — run `cursor-agent login` or set CURSOR_API_KEY")]
    NotAuthed,
    #[error("cursor-agent failed: {0}")]
    Failed(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Run cursor-agent and stream normalized Claude-style JSON events to the
/// frontend. Returns `(final_result_text, chat_id)` — chat_id is the session
/// identifier that the caller should persist and pass back as
/// `agent_uuid` with `resume=true` on subsequent turns.
pub async fn ask(
    app: tauri::AppHandle,
    runners: Runners,
    session_id: &str,
    prompt: &str,
    cwd: Option<&Path>,
    agent_uuid: &str,
    resume: bool,
    rules: Option<&str>,
    model: Option<&str>,
) -> Result<(String, String), CursorRunError> {
    use tauri::Emitter;
    use tokio::io::AsyncBufReadExt;

    let status = detect();
    if !status.detected {
        return Err(CursorRunError::NotInstalled);
    }
    if !status.ready {
        return Err(CursorRunError::NotAuthed);
    }
    let bin = status.path.as_deref().unwrap_or("cursor-agent");

    // Resolve the chat_id. For a resumed session we use what the caller gave
    // us; for a fresh turn we mint a new one via `create-chat` so the frontend
    // can store it stably. `create-chat` prints the UUID on stdout.
    let chat_id = if resume && !agent_uuid.is_empty() {
        agent_uuid.to_string()
    } else {
        let out = Command::new(bin)
            .arg("create-chat")
            .envs(extended_env())
            .output()
            .map_err(CursorRunError::Io)?;
        if !out.status.success() {
            return Err(CursorRunError::Failed(format!(
                "cursor-agent create-chat: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };

    // Prepend rules to the prompt — cursor-agent has no `--append-system-prompt`.
    let effective_prompt = match rules.map(|s| s.trim()).filter(|s| !s.is_empty()) {
        Some(r) => format!(
            "User rules (follow these on every turn):\n\n{}\n\n---\n\n{}",
            r, prompt
        ),
        None => prompt.to_string(),
    };

    let mut cmd = tokio::process::Command::new(bin);
    cmd.arg("-p")
        .arg(&effective_prompt)
        .arg("--output-format")
        .arg("stream-json")
        .arg("--resume")
        .arg(&chat_id)
        // Let the agent run tools non-interactively — equivalent to Claude's
        // `auto` permission mode. `--approve-mcps` lets MCP servers answer
        // without the tty prompt they'd otherwise emit.
        .arg("--force")
        .arg("--approve-mcps");
    if let Some(m) = model.map(str::trim).filter(|m| !m.is_empty()) {
        cmd.arg("--model").arg(m);
    }
    if let Some(dir) = cwd {
        cmd.arg("--workspace").arg(dir);
        cmd.current_dir(dir);
    }
    for (k, v) in extended_env() {
        cmd.env(k, v);
    }
    // macOS quirk: when Forgehold.app is unsigned (ad-hoc) and spawns
    // cursor-agent as a child, the child's Keychain Services calls via
    // node-keytar get silently denied — cursor-agent then reports
    // "Authentication required". Work around by reading the token ourselves
    // (the `security` CLI has its own trusted identity) and handing it to
    // cursor-agent via the env var it honors.
    if std::env::var("CURSOR_AUTH_TOKEN").is_err() {
        if let Some(tok) = read_cursor_access_token() {
            cmd.env("CURSOR_AUTH_TOKEN", tok);
        }
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
        .ok_or_else(|| CursorRunError::Failed("no stdout".into()))?;
    // Drain stderr concurrently so the pipe buffer never fills (which would
    // deadlock `child.wait`), and so we can surface cursor-agent's own error
    // text to the user on non-zero exit instead of a useless "status 1".
    let stderr_pipe = child.stderr.take();
    let stderr_handle = tokio::spawn(async move {
        use tokio::io::AsyncReadExt;
        let mut buf = Vec::new();
        if let Some(mut s) = stderr_pipe {
            let _ = s.read_to_end(&mut buf).await;
        }
        String::from_utf8_lossy(&buf).trim().to_string()
    });
    let reader = tokio::io::BufReader::new(stdout);
    let mut lines = reader.lines();

    // Same event channel the Claude adapter uses — frontend doesn't know (or
    // care) which CLI produced the line after normalization.
    let event_name = format!("claude:stream:{}", session_id);
    let mut final_text = String::new();

    while let Ok(Some(raw)) = lines.next_line().await {
        for out in normalize_event(&raw) {
            let serialized = out.to_string();
            let _ = app.emit(&event_name, &serialized);
            if let Some(t) = out.get("type").and_then(|t| t.as_str()) {
                if t == "result" {
                    if let Some(r) = out.get("result").and_then(|r| r.as_str()) {
                        final_text = r.to_string();
                    }
                }
            }
        }
    }

    let out = child.wait().await;
    runners.lock().unwrap().remove(session_id);
    let out = out?;
    let stderr_text = stderr_handle.await.unwrap_or_default();
    if let Some(code) = out.code() {
        if code == 143 {
            return Err(CursorRunError::Failed("cancelled".into()));
        }
    }
    if !out.success() {
        let code = out.code().unwrap_or(-1);
        let msg = if stderr_text.is_empty() {
            format!("cursor-agent exited with status {}", code)
        } else {
            format!("cursor-agent (status {}): {}", code, stderr_text)
        };
        return Err(CursorRunError::Failed(msg));
    }
    Ok((final_text, chat_id))
}

/// Translate one cursor-agent stream line into zero or more Claude-style
/// events. Most events pass through (cursor's `assistant`/`result` shape is
/// identical). `tool_call` → synthesized `assistant` event with a `tool_use`
/// content block so `formatToolUse` on the frontend picks it up unchanged.
fn normalize_event(raw: &str) -> Vec<serde_json::Value> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(raw) else {
        return Vec::new();
    };
    let Some(ty) = v.get("type").and_then(|t| t.as_str()) else {
        return Vec::new();
    };
    match ty {
        // Text from the model, and the final result: already match Claude's
        // shape closely enough — frontend only reads `type`, `message.content`,
        // and `result`.
        "assistant" | "result" | "system" | "user" => vec![v],
        "tool_call" => normalize_tool_call(&v).into_iter().collect(),
        _ => Vec::new(),
    }
}

/// Cursor's `tool_call` event carries a discriminated-union payload; pick out
/// the readable tool name + args and re-emit as a Claude-style `tool_use`
/// content block. We only handle the `started` subtype (tool *invocation*) —
/// `completed` carries the tool *result*, which forge already surfaces via
/// action cards or inline bash output, so rendering it again would duplicate.
fn normalize_tool_call(v: &serde_json::Value) -> Option<serde_json::Value> {
    let subtype = v.get("subtype").and_then(|s| s.as_str()).unwrap_or("");
    if subtype != "started" {
        return None;
    }
    let call_id = v
        .get("call_id")
        .and_then(|c| c.as_str())
        .unwrap_or("")
        .to_string();
    let session_id = v.get("session_id").cloned().unwrap_or(json!(null));
    let tool_call = v.get("tool_call")?;
    let (name, input) = extract_tool_shape(tool_call);
    Some(json!({
        "type": "assistant",
        "message": {
            "role": "assistant",
            "content": [{
                "type": "tool_use",
                "id": call_id,
                "name": name,
                "input": input,
            }]
        },
        "session_id": session_id,
    }))
}

/// Reach into cursor's discriminated tool_call union (readToolCall /
/// writeToolCall / function / …) and return a `(tool_name, input_object)`
/// pair. Best-effort — forge's `formatToolUse` gracefully falls back to a
/// compact generic render for unknown names/shapes.
fn extract_tool_shape(tc: &serde_json::Value) -> (String, serde_json::Value) {
    let Some(obj) = tc.as_object() else {
        return ("tool".into(), json!({}));
    };
    // Each known variant pairs a discriminator key with its payload:
    //   readToolCall   → Read / Grep / Glob equivalents
    //   writeToolCall  → Edit / Write / NotebookEdit equivalents
    //   function       → the MCP tool path (`function.name` + `function.args`)
    // The payload object already has a human-meaningful `name` in most cases;
    // when it doesn't we fall back to the discriminator itself.
    for (key, payload) in obj.iter() {
        if key == "function" {
            let name = payload
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or("function")
                .to_string();
            let input = payload
                .get("args")
                .cloned()
                .or_else(|| payload.get("arguments").cloned())
                .unwrap_or_else(|| json!({}));
            return (name, input);
        }
        if key.ends_with("ToolCall") {
            let name = payload
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or_else(|| humanize_discriminator(key))
                .to_string();
            // Flatten the payload as the "input" so formatToolUse can dig out
            // familiar keys (file_path, command, pattern, …).
            let input = json!(payload);
            return (name, input);
        }
    }
    ("tool".into(), json!({}))
}

fn humanize_discriminator(key: &str) -> &'static str {
    match key {
        "readToolCall" => "Read",
        "writeToolCall" => "Write",
        "bashToolCall" => "Bash",
        "grepToolCall" => "Grep",
        "globToolCall" => "Glob",
        _ => "tool",
    }
}

/// Send SIGTERM to a running cursor-agent spawn for the given forge session.
pub fn stop(runners: &Runners, session_id: &str) -> bool {
    let pid = runners.lock().unwrap().get(session_id).copied();
    match pid {
        Some(p) => unsafe { libc::kill(p as libc::pid_t, libc::SIGTERM) == 0 },
        None => false,
    }
}

fn which(name: &str) -> Option<PathBuf> {
    // Same PATH-augmentation pattern as claude.rs — Tauri-spawned processes
    // don't inherit login-shell PATH on macOS, so Homebrew / pipx / ~/.local
    // binaries are invisible without help.
    let mut candidates: Vec<String> = Vec::new();
    if let Ok(p) = std::env::var("PATH") {
        for dir in p.split(':') {
            if !dir.is_empty() {
                candidates.push(dir.to_string());
            }
        }
    }
    for extra in ["/opt/homebrew/bin", "/usr/local/bin"] {
        if !candidates.iter().any(|d| d == extra) {
            candidates.push(extra.into());
        }
    }
    if let Some(h) = home_dir() {
        candidates.push(h.join(".local/bin").to_string_lossy().into_owned());
    }
    for dir in candidates {
        let cand = Path::new(&dir).join(name);
        if is_executable(&cand) {
            return Some(cand);
        }
    }
    None
}

fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    p.metadata()
        .map(|m| m.is_file() && (m.permissions().mode() & 0o111) != 0)
        .unwrap_or(false)
}

fn read_version(path: &Path) -> Option<String> {
    let out = Command::new(path).arg("--version").output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() { None } else { Some(s) }
}

fn home_dir() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(PathBuf::from)
}

/// Read cursor-agent's session access token from the macOS login Keychain.
/// node-keytar (used by cursor-agent) stores it as a generic password under
/// service `cursor-access-token` / account `cursor-user`. We use the system
/// `security` CLI so Forgehold.app's own (possibly restricted) keychain
/// access path doesn't matter.
fn read_cursor_access_token() -> Option<String> {
    let out = std::process::Command::new("/usr/bin/security")
        .args([
            "find-generic-password",
            "-s",
            "cursor-access-token",
            "-a",
            "cursor-user",
            "-w",
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let tok = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if tok.is_empty() { None } else { Some(tok) }
}

/// PATH augmentation for spawned subprocesses — matches claude.rs.
fn extended_env() -> Vec<(String, String)> {
    let mut out = Vec::new();
    if let Ok(p) = std::env::var("PATH") {
        let extras = ["/opt/homebrew/bin", "/usr/local/bin"];
        let mut parts: Vec<&str> = p.split(':').collect();
        for e in extras {
            if !parts.contains(&e) {
                parts.push(e);
            }
        }
        let home_local = home_dir().map(|h| h.join(".local/bin").to_string_lossy().into_owned());
        if let Some(hl) = home_local.as_deref() {
            if !parts.contains(&hl) {
                parts.push(hl);
            }
        }
        out.push(("PATH".into(), parts.join(":")));
    }
    out
}
