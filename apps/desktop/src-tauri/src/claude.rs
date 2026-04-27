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
use crate::sentry::SentryCredentials;

const JIRA_KEYCHAIN_KEY: &str = "jira";
const GITHUB_KEYCHAIN_KEY: &str = "github";
const SENTRY_KEYCHAIN_KEY: &str = "sentry";

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

/// Minimal extension → media-type mapping for the four formats Anthropic's
/// vision endpoint accepts. Anything else falls through to `image/png` —
/// the API will reject unsupported types with a clear error rather than
/// silently corrupting the upload.
fn guess_image_media_type(path: &str) -> &'static str {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        "image/jpeg"
    } else if lower.ends_with(".gif") {
        "image/gif"
    } else if lower.ends_with(".webp") {
        "image/webp"
    } else {
        "image/png"
    }
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
/// `claude_uuid` is the Claude CLI session UUID (independent of Forgehold's own
/// `session_id`). When `resume` is false we pass `--session-id <uuid>` so the
/// CLI creates a new persisted session; when true we pass `--resume <uuid>`
/// so it continues the existing one with full history. This gives Forgehold
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
    // Forwarded as `--model <id>` to claude CLI. None means no flag → CLI
    // picks its default (Opus 4.7 on Max plans). Frontend defaults new
    // sessions to `claude-sonnet-4-6` so the typical case doesn't burn
    // the 5h quota at Opus rates.
    model: Option<&str>,
    // Tool profile name: 'coding' / 'triage' / 'pr-review' / 'all'.
    // Filters which MCP servers are wired and which tools end up in
    // `--allowedTools`. Unrecognised/None falls back to 'all' (legacy
    // behavior — every tool exposed).
    tool_profile: Option<&str>,
    app_context: Option<&str>,
    image_paths: &[String],
) -> Result<String, ClaudeRunError> {
    use tauri::Emitter;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

    let status = detect();
    if !status.detected {
        return Err(ClaudeRunError::NotInstalled);
    }
    if !status.ready {
        return Err(ClaudeRunError::NotAuthed);
    }
    let bin = status.path.as_deref().unwrap_or("claude");

    let mcp = build_mcp_config(session_id, ToolProfile::from_str(tool_profile));
    // Drop guard ensures the temp config file is removed on every exit path.
    let _mcp_guard = mcp.as_ref().map(|(p, _)| TempFile(p.clone()));

    // When the user attached images, switch to `--input-format stream-json` so
    // we can embed proper `image` content blocks alongside the text. The model
    // sees the bytes via vision — no Read tool call, no path-encoding pitfalls,
    // works for any filename including em-dash + Cyrillic. Plain text turns
    // (no images) keep the simpler `-p <prompt>` arg path.
    let stream_input = !image_paths.is_empty();

    let mut cmd = tokio::process::Command::new(bin);
    if stream_input {
        cmd.arg("-p")
            .arg("--input-format")
            .arg("stream-json")
            .arg("--output-format")
            .arg("stream-json")
            .arg("--verbose");
    } else {
        cmd.arg("-p")
            .arg(prompt)
            .arg("--output-format")
            .arg("stream-json")
            .arg("--verbose");
    }
    if resume {
        cmd.arg("--resume").arg(claude_uuid);
    } else {
        cmd.arg("--session-id").arg(claude_uuid);
    }
    // Forward selected model. The flag is per-turn, so users can swap
    // mid-session — Claude CLI accepts a different `--model` on each
    // `--resume` call. None = no flag, CLI picks its default.
    if let Some(m) = model.map(str::trim).filter(|s| !s.is_empty()) {
        cmd.arg("--model").arg(m);
    }
    // Compose the system-prompt suffix in three parts. Order matters for
    // Claude's prompt-cache: the cache key is a prefix of the appended
    // text, so anything that's mostly-stable should come BEFORE anything
    // that mutates per turn. Otherwise the variable bytes invalidate the
    // cache for the kilobytes that follow them.
    //
    //   1. Memory hint — fully static (only depends on whether the
    //      memory sidecar is wired in this session, which is constant
    //      for the session's lifetime).
    //   2. User rules — changes only when the user edits the Rules tab,
    //      and that's a manual save, so effectively static between
    //      turns.
    //   3. Per-turn UI context — workbench layout, cwds, linked agents,
    //      one-shot cwd-switch recap. Mutates every turn that adds a
    //      column or changes an editor's open path. Goes LAST so the
    //      preceding block stays a stable prefix.
    let mut system_parts: Vec<String> = Vec::new();
    let memory_available = mcp
        .as_ref()
        .map(|(_, allowed)| {
            allowed.iter().any(|t| t.starts_with("mcp__memory__"))
        })
        .unwrap_or(false);
    if memory_available {
        system_parts.push(
            "Forgehold memory (persistent across sessions): you have \
             `mcp__memory__memory_search`, `memory_save`, `memory_list`, \
             `memory_delete`. Behavior:\n\
             - At the START of a non-trivial turn, run `memory_search` with \
               the user's keywords (or a paraphrase). If it returns relevant \
               facts/preferences/context, lean on them — don't ask the user \
               to repeat themselves.\n\
             - When the user states a preference, gives feedback on your \
               approach, or shares persistent context (their role, project, \
               tools, conventions), call `memory_save` to record it. Tag \
               with category like `preference`, `project`, `feedback`, \
               `reference`. Keep entries terse (1-3 sentences).\n\
             - Don't save ephemeral task state, code, or anything already in \
               git/the codebase. Only save what would be lost across restarts.\n\
             - If a recalled memory conflicts with current state, trust what \
               you see now and update or delete the stale memory."
                .to_string(),
        );
    }
    if let Some(r) = rules.map(|s| s.trim()).filter(|s| !s.is_empty()) {
        system_parts.push(format!("User rules (follow these on every turn):\n\n{}", r));
    }
    if let Some(ctx) = app_context.map(str::trim).filter(|s| !s.is_empty()) {
        system_parts.push(ctx.to_string());
    }
    if !system_parts.is_empty() {
        cmd.arg("--append-system-prompt").arg(system_parts.join("\n\n---\n\n"));
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
    if stream_input {
        cmd.stdin(std::process::Stdio::piped());
    }

    let mut child = cmd.spawn()?;
    let pid = child.id().unwrap_or(0);
    if pid != 0 {
        runners.lock().unwrap().insert(session_id.to_string(), pid);
    }

    if stream_input {
        // Build one user message with text + image content blocks. We resolve
        // each image to base64 + media_type up front; if a path can't be read
        // (deleted, permission), skip it silently — the user still gets their
        // text turn rather than a hard failure.
        use base64::Engine;
        let mut content: Vec<serde_json::Value> = Vec::new();
        // The text block goes first so the prompt context (workbench app
        // context, mention bodies) stays at the head of the message — same
        // reading order Claude sees in interactive mode.
        content.push(serde_json::json!({ "type": "text", "text": prompt }));
        for p in image_paths {
            let bytes = match tokio::fs::read(p).await {
                Ok(b) => b,
                Err(_) => continue,
            };
            let media_type = guess_image_media_type(p);
            let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
            content.push(serde_json::json!({
                "type": "image",
                "source": {
                    "type": "base64",
                    "media_type": media_type,
                    "data": b64,
                }
            }));
        }
        let msg = serde_json::json!({
            "type": "user",
            "message": { "role": "user", "content": content }
        });
        let line = msg.to_string();
        if let Some(mut stdin) = child.stdin.take() {
            // Best-effort: failures to write are surfaced via the CLI's
            // exit code path below (it'll error out if the input is empty).
            let _ = stdin.write_all(line.as_bytes()).await;
            let _ = stdin.write_all(b"\n").await;
            let _ = stdin.shutdown().await;
        }
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

/// Result of a successful compact-fork. The frontend swaps the
/// session's `claudeUuid` to `new_uuid` and appends a system message
/// to the chat carrying `summary` so the user can read what the new
/// session was seeded with.
#[derive(Debug, serde::Serialize, Clone)]
pub struct CompactResult {
    pub new_uuid: String,
    pub summary: String,
}

/// Two-shot fork-compact: ask the existing CLI session to summarise
/// itself, then seed a brand-new session with that summary as the
/// first turn. The summary call uses `--resume <old_uuid>` so the
/// model has full prior context; the seed call uses `--session-id
/// <new_uuid>` and a short ack-prompt so the new session has stored
/// history starting from the summary, ready for `--resume <new_uuid>`
/// on the user's very next normal turn.
///
/// We bypass `ask()` here so the calls don't emit on the
/// `claude:stream:<session_id>` channel (the chat would briefly show
/// a wall of summary text mid-compact otherwise). Instead each call
/// runs as a plain `claude -p <prompt> --output-format json`,
/// blocking until done.
pub async fn compact_session(
    old_uuid: &str,
    new_uuid: &str,
    cwd: Option<&Path>,
    model: Option<&str>,
) -> Result<CompactResult, ClaudeRunError> {
    let status = detect();
    if !status.detected {
        return Err(ClaudeRunError::NotInstalled);
    }
    if !status.ready {
        return Err(ClaudeRunError::NotAuthed);
    }
    let bin = status.path.as_deref().unwrap_or("claude");

    // Step 1: summary. Tight prompt so the response is focused — the
    // sentinel header lets us strip any pre-amble Claude sometimes
    // emits anyway despite "no preamble" instructions.
    let summary_prompt = "Output ONLY a concise (300-500 word) summary of this conversation \
        so far — what was asked, what was decided, what was done, what's still in flight, \
        and any code/config decisions worth remembering. No preamble, no sign-off, no \
        meta-commentary about \"summarising\" — just the summary content.";
    let summary = run_claude_oneshot(bin, summary_prompt, Some(old_uuid), None, cwd, model).await?;
    let summary = summary.trim();
    if summary.is_empty() {
        return Err(ClaudeRunError::Failed(
            "claude returned an empty summary — old session may not be resumable".into(),
        ));
    }

    // Step 2: seed new session. Keep the prompt small (no tool use, no
    // navigation) — it just needs to land enough turn history that the
    // next `--resume <new_uuid>` works. Ack body is intentionally
    // discarded; we keep `summary` as the user-facing artifact.
    let seed_prompt = format!(
        "This is a continuation of an earlier Claude Code session. \
         The prior conversation has been compacted into the summary below. \
         Reply with a brief one-sentence acknowledgement (e.g. \"Ready to continue.\") \
         — the user's next message will pick up from here.\n\n\
         === PRIOR-SESSION SUMMARY ===\n{summary}\n=== END SUMMARY ==="
    );
    let _ack = run_claude_oneshot(bin, &seed_prompt, None, Some(new_uuid), cwd, model).await?;

    Ok(CompactResult {
        new_uuid: new_uuid.to_string(),
        summary: summary.to_string(),
    })
}

/// Run a single non-streaming `claude -p` call and return the
/// `result` field from the JSON output. Used by `compact_session` for
/// both the summary call and the seed call. No MCP, no
/// `--append-system-prompt`, no images — just the prompt text and
/// (optionally) `--resume` / `--session-id` / `--model`.
async fn run_claude_oneshot(
    bin: &str,
    prompt: &str,
    resume_uuid: Option<&str>,
    new_session_uuid: Option<&str>,
    cwd: Option<&Path>,
    model: Option<&str>,
) -> Result<String, ClaudeRunError> {
    let mut cmd = tokio::process::Command::new(bin);
    cmd.arg("-p").arg(prompt).arg("--output-format").arg("json");
    if let Some(u) = resume_uuid {
        cmd.arg("--resume").arg(u);
    } else if let Some(u) = new_session_uuid {
        cmd.arg("--session-id").arg(u);
    }
    if let Some(m) = model.map(str::trim).filter(|s| !s.is_empty()) {
        cmd.arg("--model").arg(m);
    }
    if let Some(d) = cwd {
        cmd.current_dir(d);
    }
    // Same PATH augmentation as `ask()` / `generate_commit_message` —
    // Tauri-launched processes inherit a skinny PATH, and `claude` may
    // live in `~/.claude/local/bin` etc. depending on install method.
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

    let out = cmd.output().await?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        let code = out.status.code().unwrap_or(-1);
        return Err(ClaudeRunError::Failed(format!(
            "claude exited {code}{}",
            if stderr.is_empty() { String::new() } else { format!(" — {stderr}") }
        )));
    }
    // `--output-format json` returns one envelope: `{"result": "...",
    // "session_id": "...", "is_error": false, ...}`. Pull `result`,
    // tolerate missing-field as an empty string so the caller surfaces
    // a "claude returned empty" instead of a JSON-parse error.
    let parsed: serde_json::Value = serde_json::from_slice(&out.stdout)
        .map_err(|e| ClaudeRunError::Failed(format!("claude json parse failed: {e}")))?;
    let result = parsed
        .get("result")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    Ok(result)
}

/// Removes its path on drop. Used to clean up the temp MCP config.
struct TempFile(PathBuf);

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
enum ToolProfile {
    All,
    Coding,
    Github,
    Jira,
    Sentry,
    Triage,
}

impl ToolProfile {
    fn from_str(s: Option<&str>) -> Self {
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
    fn allows(self, tool: &str) -> bool {
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
fn build_mcp_config(session_id: &str, profile: ToolProfile) -> Option<(PathBuf, Vec<String>)> {
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
