//! Cursor Agent CLI adapter — parallel to `claude.rs`.
//!
//! Forgehold spawns `cursor-agent -p <prompt> --output-format stream-json
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
//!     `<workspace>/.cursor/mcp.json`. Forgehold's sidecars aren't wired in by
//!     default in v1; users can add them manually via `cursor-agent mcp
//!     enable` or a project-local mcp.json.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;
use serde_json::json;

use crate::claude::Runners;

#[derive(Debug, Serialize, Clone)]
pub struct CursorStatus {
    /// `cursor-agent` binary found on PATH.
    pub detected: bool,
    /// Path to the binary if detected.
    pub path: Option<String>,
    /// Output of `cursor-agent --version`, trimmed.
    pub version: Option<String>,
    /// `~/.cursor` dir exists — usually means `cursor-agent login` has
    /// been run (the CLI persists its session there).
    pub has_config_dir: bool,
    /// `CURSOR_API_KEY` env var set in our process env.
    pub has_api_key_env: bool,
    /// High-level bool for the UI: detected + (config dir or API key env).
    pub ready: bool,
}

pub fn detect() -> CursorStatus {
    let path = which("cursor-agent");
    let detected = path.is_some();
    let version = path.as_ref().and_then(|p| read_version(p));
    // cursor-agent stores credentials under ~/.cursor; presence of the dir
    // is our cheap proxy for "authenticated". For API-key auth the env var
    // CURSOR_API_KEY works too.
    let has_config_dir = home_dir()
        .map(|h| h.join(".cursor").is_dir())
        .unwrap_or(false);
    let has_api_key_env = std::env::var("CURSOR_API_KEY").is_ok();
    CursorStatus {
        detected,
        path: path.map(|p| p.display().to_string()),
        version,
        has_config_dir,
        has_api_key_env,
        ready: detected && (has_config_dir || has_api_key_env),
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
    app_context: Option<&str>,
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

    // Prepend rules + app context to the prompt — cursor-agent has no
    // `--append-system-prompt`. Order matters for cursor-agent's backend
    // prompt cache (Anthropic-flavour, prefix-keyed): static-est blocks
    // first so the cache hit survives layout changes between turns.
    //   1. User rules — only changes when the user edits the Rules tab.
    //   2. App context — workbench layout snapshot, mutates per turn;
    //      its OWN structure is also static-first / variable-last (see
    //      `buildAgentAppContext`).
    //   3. The user message itself.
    let mut preamble = String::new();
    if let Some(r) = rules.map(|s| s.trim()).filter(|s| !s.is_empty()) {
        preamble.push_str("User rules (follow these on every turn):\n\n");
        preamble.push_str(r);
        preamble.push_str("\n\n---\n\n");
    }
    if let Some(ctx) = app_context.map(str::trim).filter(|s| !s.is_empty()) {
        preamble.push_str(ctx);
        preamble.push_str("\n\n---\n\n");
    }
    let effective_prompt = if preamble.is_empty() {
        prompt.to_string()
    } else {
        format!("{}{}", preamble, prompt)
    };

    let mut cmd = tokio::process::Command::new(bin);
    cmd.arg("--print")
        .arg("--output-format")
        .arg("stream-json")
        // Without this, cursor-agent buffers the entire model response and
        // emits one big `assistant` event at the end. The Forgehold spinner
        // would just tick for 30-90s with no UI feedback. Turn it on so
        // text deltas stream as the model writes them.
        .arg("--stream-partial-output")
        .arg("--resume")
        .arg(&chat_id)
        // Let the agent run tools non-interactively — equivalent to Claude's
        // `auto` permission mode. `--approve-mcps` lets MCP servers answer
        // without the tty prompt they'd otherwise emit. `--trust` skips the
        // first-time workspace-trust prompt that otherwise hangs the spawn
        // forever (no tty to confirm on).
        .arg("--force")
        .arg("--approve-mcps")
        .arg("--trust");
    if let Some(m) = model.map(str::trim).filter(|m| !m.is_empty()) {
        cmd.arg("--model").arg(m);
    }
    if let Some(dir) = cwd {
        cmd.arg("--workspace").arg(dir);
        cmd.current_dir(dir);
    }
    // Prompt is the positional argument — pass it last with `--` so any
    // `-`/`--`-leading content inside the prompt isn't reinterpreted as
    // a flag by the CLI parser.
    cmd.arg("--").arg(&effective_prompt);
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
    // cursor-agent occasionally exits 0 yet writes nothing to stdout — typical
    // auth failures (token expired, "Authentication required" — keychain was
    // ignored, network error during auth refresh). Without surfacing the
    // stderr text the chat just shows "(empty response)" and the user thinks
    // the app silently swallowed their turn. Treat empty stdout + non-empty
    // stderr as an error so they at least see what cursor-agent told us.
    if final_text.trim().is_empty() && !stderr_text.is_empty() {
        return Err(CursorRunError::Failed(format!(
            "cursor-agent finished with no output. Diagnostic: {}",
            truncate_str(&stderr_text, 600)
        )));
    }
    Ok((final_text, chat_id))
}

fn truncate_str(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max).collect();
    out.push_str("…");
    out
}

/// Two-shot fork-compact for cursor-agent. Mirrors `claude::compact_session`:
/// ask the existing chat to summarise itself, then start a brand-new chat
/// seeded with that summary as the first user turn. The new chat_id is
/// minted by cursor-agent (it doesn't accept a `--session-id` flag the way
/// claude does — `--resume` is the only id-control surface) and we read it
/// back from the seed-call's `result.session_id`.
///
/// Both calls go through `--output-format json` so we don't fan out
/// stream events to the frontend mid-compact (the chat would briefly show
/// the summary text and ack as if they were normal turns otherwise).
pub async fn compact_session(
    old_chat_id: &str,
    cwd: Option<&Path>,
    model: Option<&str>,
) -> Result<crate::claude::CompactResult, CursorRunError> {
    let status = detect();
    if !status.detected {
        return Err(CursorRunError::NotInstalled);
    }
    if !status.ready {
        return Err(CursorRunError::NotAuthed);
    }
    let bin = status.path.as_deref().unwrap_or("cursor-agent");

    // Step 1: summary against the live chat. Same prompt shape as the
    // claude-side compact so summaries read consistently regardless of
    // which agent produced them.
    let summary_prompt = "Output ONLY a concise (300-500 word) summary of this conversation \
        so far — what was asked, what was decided, what was done, what's still in flight, \
        and any code/config decisions worth remembering. No preamble, no sign-off, no \
        meta-commentary about \"summarising\" — just the summary content.";
    let (summary, _) =
        run_cursor_oneshot(bin, summary_prompt, Some(old_chat_id), cwd, model).await?;
    let summary = summary.trim();
    if summary.is_empty() {
        return Err(CursorRunError::Failed(
            "cursor-agent returned an empty summary — old chat may not be resumable".into(),
        ));
    }

    // Step 2: seed a fresh chat with the summary. cursor-agent mints the
    // chat_id; we capture it from the result event so the frontend can
    // swap it onto the session for the user's next normal turn (which
    // will then `--resume <new>`).
    let seed_prompt = format!(
        "This is a continuation of an earlier cursor-agent session. \
         The prior conversation has been compacted into the summary below. \
         Reply with a brief one-sentence acknowledgement (e.g. \"Ready to continue.\") \
         — the user's next message will pick up from here.\n\n\
         === PRIOR-SESSION SUMMARY ===\n{summary}\n=== END SUMMARY ==="
    );
    let (_ack, new_chat_id) = run_cursor_oneshot(bin, &seed_prompt, None, cwd, model).await?;
    if new_chat_id.is_empty() {
        return Err(CursorRunError::Failed(
            "cursor-agent didn't return a session_id for the seed turn".into(),
        ));
    }

    Ok(crate::claude::CompactResult {
        new_uuid: new_chat_id,
        summary: summary.to_string(),
    })
}

/// Run a single non-streaming `cursor-agent --print --output-format json`
/// call and return `(result_text, session_id)`. `resume_chat_id`
/// controls whether we attach to an existing chat (Some = `--resume
/// <id>`) or let cursor-agent mint a fresh one (None = no flag).
/// `--force --approve-mcps --trust` mirrors the headless flags that
/// `ask()` uses for the streaming path.
async fn run_cursor_oneshot(
    bin: &str,
    prompt: &str,
    resume_chat_id: Option<&str>,
    cwd: Option<&Path>,
    model: Option<&str>,
) -> Result<(String, String), CursorRunError> {
    let mut cmd = tokio::process::Command::new(bin);
    cmd.arg("--print")
        .arg("--output-format")
        .arg("json")
        .arg("--force")
        .arg("--approve-mcps")
        .arg("--trust")
        .arg("-p")
        .arg(prompt);
    if let Some(id) = resume_chat_id {
        cmd.arg("--resume").arg(id);
    }
    if let Some(m) = model.map(str::trim).filter(|m| !m.is_empty()) {
        cmd.arg("--model").arg(m);
    }
    if let Some(d) = cwd {
        cmd.current_dir(d);
    }
    for (k, v) in extended_env() {
        cmd.env(k, v);
    }
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let out = cmd.output().await.map_err(CursorRunError::Io)?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(CursorRunError::Failed(format!(
            "cursor-agent exited {}{}",
            out.status.code().unwrap_or(-1),
            if stderr.is_empty() { String::new() } else { format!(" — {stderr}") }
        )));
    }
    let parsed: serde_json::Value = serde_json::from_slice(&out.stdout)
        .map_err(|e| CursorRunError::Failed(format!("cursor-agent json parse failed: {e}")))?;
    let result = parsed
        .get("result")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let session_id = parsed
        .get("session_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    Ok((result, session_id))
}

/// Headless one-off: feed the staged diff and return a one-line commit
/// message. Minted on a throwaway cursor-agent chat so the agent's real
/// conversation history stays clean. Same prompt shape as claude's version
/// so commit messages feel consistent regardless of which agent wrote them.
pub async fn generate_commit_message(
    repo: &std::path::Path,
) -> Result<String, CursorRunError> {
    let status = detect();
    if !status.detected {
        return Err(CursorRunError::NotInstalled);
    }
    if !status.ready {
        return Err(CursorRunError::NotAuthed);
    }
    let bin = status.path.as_deref().unwrap_or("cursor-agent");

    // Staged diff — the thing we want summarized.
    let diff_out = tokio::process::Command::new("git")
        .current_dir(repo)
        .args(["diff", "--cached"])
        .output()
        .await
        .map_err(CursorRunError::Io)?;
    if !diff_out.status.success() {
        let stderr = String::from_utf8_lossy(&diff_out.stderr).trim().to_string();
        return Err(CursorRunError::Failed(format!(
            "git diff --cached failed: {}",
            if stderr.is_empty() { "unknown error".into() } else { stderr }
        )));
    }
    let diff = String::from_utf8_lossy(&diff_out.stdout);
    if diff.trim().is_empty() {
        return Err(CursorRunError::Failed(
            "Nothing is staged — stage at least one change before asking for a commit message.".into(),
        ));
    }

    let prompt = format!(
        "Write a single git commit message for the following staged diff.\n\n\
         Rules:\n\
         - Imperative mood (\"Add X\", \"Fix Y\", not \"Added X\").\n\
         - Under 72 characters.\n\
         - No quotes, no markdown, no preamble, no explanation.\n\
         - Output ONLY the commit message on a single line.\n\n\
         ```diff\n{diff}```"
    );

    // cursor-agent needs an active chat to `-p` into; mint a throwaway one.
    let create_out = Command::new(bin)
        .arg("create-chat")
        .envs(extended_env())
        .output()
        .map_err(CursorRunError::Io)?;
    if !create_out.status.success() {
        return Err(CursorRunError::Failed(format!(
            "cursor-agent create-chat: {}",
            String::from_utf8_lossy(&create_out.stderr).trim()
        )));
    }
    let chat_id = String::from_utf8_lossy(&create_out.stdout).trim().to_string();

    let mut cmd = tokio::process::Command::new(bin);
    cmd.arg("--print")
        .arg("--output-format")
        .arg("stream-json")
        .arg("--stream-partial-output")
        .arg("--resume")
        .arg(&chat_id)
        .arg("--force")
        .arg("--approve-mcps")
        .arg("--trust")
        .arg("--workspace")
        .arg(repo)
        .arg("--")
        .arg(&prompt);
    cmd.current_dir(repo);
    for (k, v) in extended_env() {
        cmd.env(k, v);
    }
    if std::env::var("CURSOR_AUTH_TOKEN").is_err() {
        if let Some(tok) = read_cursor_access_token() {
            cmd.env("CURSOR_AUTH_TOKEN", tok);
        }
    }
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let out = cmd.output().await.map_err(CursorRunError::Io)?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        let code = out.status.code().unwrap_or(-1);
        return Err(CursorRunError::Failed(format!(
            "cursor-agent exited {code}{}",
            if stderr.is_empty() { String::new() } else { format!(" — {stderr}") }
        )));
    }

    // Parse stream-json events for the final `result` payload. Same shape
    // as claude's; we already parse it in `ask()`.
    let stdout = String::from_utf8_lossy(&out.stdout);
    let mut final_text = String::new();
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
            if v.get("type").and_then(|t| t.as_str()) == Some("result") {
                if let Some(r) = v.get("result").and_then(|r| r.as_str()) {
                    final_text = r.to_string();
                }
            }
        }
    }

    let cleaned = final_text
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .trim_matches(|c: char| c == '"' || c == '\'' || c == '`')
        .to_string();
    if cleaned.is_empty() {
        return Err(CursorRunError::Failed(
            "cursor returned an empty response".into(),
        ));
    }
    Ok(cleaned)
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
        // Text deltas, the final result, the init system event, and the user
        // echo: already match Claude's shape closely enough — frontend only
        // reads `type`, `message.content`, and `result`.
        //
        // BUT: cursor-agent emits two flavours of `assistant` events when
        // `--stream-partial-output` is on:
        //   1. partial deltas — have `timestamp_ms` — each carrying one chunk
        //   2. one final summary — no `timestamp_ms` — carries the FULL text
        // If we let both through, the frontend's append-delta path runs the
        // full text on top of the already-streamed chunks, doubling it. Drop
        // the summary; the final reply still arrives via the `result` event,
        // which `+page.svelte`'s `replaceLastAssistant` uses for the clean
        // post-stream text anyway.
        "assistant" => {
            if v.get("timestamp_ms").is_some() {
                vec![v]
            } else {
                Vec::new()
            }
        }
        "result" | "system" | "user" => vec![v],
        // tool_call: cursor-agent fires `started` (immediately on
        // dispatch) and `completed` (with the result) for every tool.
        //
        //   - For edit/write tools we want the COMPLETED event so we
        //     can read `result.success.afterFullFileContent` — the
        //     actual final file contents. The `args.streamContent` on
        //     a `started` event is a partial / streamed edit-spec
        //     (cursor's edit tool isn't a plain Write — it can patch
        //     surgically based on a chunk that doesn't equal the full
        //     file). Rendering a diff card on started would show the
        //     wrong content.
        //
        //   - For all other tools (read, grep, bash, mcp, …) we keep
        //     the original behavior: emit on STARTED so the user gets
        //     immediate "_using …_" feedback. The completed event is
        //     dropped to avoid double-pills.
        "tool_call" => {
            let subtype = v.get("subtype").and_then(|s| s.as_str()).unwrap_or("");
            let is_edit_write = v
                .get("tool_call")
                .and_then(|tc| tc.as_object())
                .is_some_and(|o| {
                    o.contains_key("editToolCall") || o.contains_key("writeToolCall")
                });
            let want = if is_edit_write { "completed" } else { "started" };
            if subtype == want {
                normalize_tool_call(&v).into_iter().collect()
            } else {
                Vec::new()
            }
        }
        _ => Vec::new(),
    }
}

/// Cursor's `tool_call` event carries a discriminated-union payload; pick out
/// the readable tool name + args and re-emit as a Claude-style `tool_use`
/// content block. We only handle the `started` subtype (tool *invocation*) —
/// `completed` carries the tool *result*, which Forgehold already surfaces via
/// action cards or inline bash output, so rendering it again would duplicate.
///
/// Two-pass extraction. cursor-agent has shipped at least three layouts for
/// where the tool name lives: inside `tool_call` as a discriminated union
/// (`function` / `*ToolCall`), inside `tool_call` as a flat `{name, input}`
/// (newer MCP form), or hoisted onto the EVENT itself (with `name`/`args`
/// siblings of `type`/`subtype`/`call_id`). We try the inner payload first
/// (it's the historic shape), then fall back to the event itself, then
/// drop a debug breadcrumb so a stuck `_using tool…_` pill always lets us
/// recover the raw event from the log file.
fn normalize_tool_call(v: &serde_json::Value) -> Option<serde_json::Value> {
    // Caller (`normalize_event`) already decided which subtype to
    // emit for which tool family — `started` for read/bash/grep/mcp/…,
    // `completed` for edit/write (because the full final file content
    // only lands on completed, in `result.success.afterFullFileContent`).
    // We accept whichever the caller passed through.
    let call_id = v
        .get("call_id")
        .and_then(|c| c.as_str())
        .unwrap_or("")
        .to_string();
    let session_id = v.get("session_id").cloned().unwrap_or(json!(null));

    // Pass 1: inner `tool_call` payload, if it's a JSON object. This is
    // the historical layout and still the correct one for cursor-agent's
    // built-in tools (Read/Write/Bash/Grep/Glob).
    let inner = v.get("tool_call");
    let (mut raw_name, mut input) = match inner.and_then(|tc| tc.as_object()) {
        Some(_) => extract_tool_shape(inner.unwrap()),
        None => ("tool".into(), json!({})),
    };

    // Pass 2: if the inner payload didn't yield a real tool name (i.e.
    // we landed on the generic "tool" fallback), retry against the
    // event's own object. cursor-agent's MCP layout in late-2025
    // builds hoists `name` and `args` to the event level and leaves
    // `tool_call` either absent, null, or a free-form string hint.
    // Without this retry the trace renders an empty "_using tool…_"
    // pill and the frontend dispatcher never matches `mcp__app__*`,
    // which is exactly the "Cursor opened the PR but nothing
    // happened" symptom from the bug repro.
    if raw_name == "tool" {
        let (n2, i2) = extract_tool_shape(v);
        if n2 != "tool" {
            raw_name = n2;
            input = i2;
        } else {
            // Still nothing. Stash the whole event under `_raw` so
            // formatToolUse falls into its single-string-arg branch
            // and surfaces the JSON inline — this is how we'll learn
            // about the next shape without another roundtrip.
            input = json!({
                "_raw": serde_json::to_string(v).unwrap_or_default(),
            });
            log_unknown_tool_call(v);
        }
    }

    let name = normalize_mcp_tool_name(&raw_name);
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

/// Append the raw event JSON to a per-machine log so the user can paste
/// it back when reporting "blank trace pill". Best-effort — failures
/// to write are silent (don't want diagnostics to break the agent).
/// The path is `~/Library/Logs/Forgehold/cursor-unknown-tool-calls.log`
/// on macOS, falling back to `$HOME/.forgehold-cursor-unknown.log`.
fn log_unknown_tool_call(v: &serde_json::Value) {
    let Ok(home) = std::env::var("HOME") else {
        return;
    };
    let dir = if cfg!(target_os = "macos") {
        PathBuf::from(&home).join("Library/Logs/Forgehold")
    } else {
        PathBuf::from(&home).join(".forgehold")
    };
    let _ = std::fs::create_dir_all(&dir);
    let file = dir.join("cursor-unknown-tool-calls.log");
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&file)
    {
        let line = format!(
            "{} {}\n",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            serde_json::to_string(v).unwrap_or_default()
        );
        let _ = f.write_all(line.as_bytes());
    }
}

/// Strip the `forgehold-` prefix from MCP tool names so cursor-agent's
/// `mcp__forgehold-app__open_github_pr` lines up with the
/// `mcp__app__open_github_pr` shape the frontend's `handleStreamEvent` +
/// `handleAppNavigation` dispatcher matches on. Same fix for the other
/// sidecars (jira / github / sentry / memory).
///
/// Why the names diverge: Claude is invoked with `--mcp-config` listing
/// servers under bare keys (`app`, `jira`, …) so its tool names land at
/// `mcp__app__…`. Cursor reads `~/.cursor/mcp.json`, which uses the
/// public `forgehold-app` / `forgehold-jira` namespace (it's the
/// global MCP namespace shared with anything else the user wires in
/// via `cursor-agent mcp add`, so we don't squat on `app`). Without
/// this normalization the frontend silently drops every navigation
/// call — cursor-agent reports "tool succeeded" (the sidecar always
/// answers OK), but no UI mutation runs. That's the "Cursor says it
/// opened the PR but nothing happened" bug.
fn normalize_mcp_tool_name(name: &str) -> String {
    if let Some(rest) = name.strip_prefix("mcp__forgehold-") {
        // `rest` is now e.g. `app__open_github_pr`. Re-prefix to land at
        // the Claude-shaped `mcp__app__open_github_pr`.
        return format!("mcp__{}", rest);
    }
    name.to_string()
}

/// Reach into cursor's discriminated tool_call union (readToolCall /
/// writeToolCall / function / mcp / …) and return a `(tool_name,
/// input_object)` pair. Best-effort — Forgehold's `formatToolUse`
/// gracefully falls back to a compact generic render for unknown
/// names/shapes.
///
/// We probe in three layers, most-specific → most-general, so a cleaner
/// shape always wins over a fallback. cursor-agent has shipped four
/// distinct envelopes for tool_calls across versions; this used to only
/// handle two, which is why MCP calls (e.g. `mcp__forgehold-app__open_github_pr`)
/// were collapsing to `("tool", {})` — the agent reports "1 step" but
/// the trace says "_using tool…_" and the frontend dispatcher never
/// fires because `name` is empty. See the bug repro: "Открыл PR в
/// слайдовере" / "1 step ✓ … using tool…".
fn extract_tool_shape(tc: &serde_json::Value) -> (String, serde_json::Value) {
    let Some(obj) = tc.as_object() else {
        return ("tool".into(), json!({}));
    };

    // Layer 1: flat shape — `tool_call: { name, input | args | arguments }`.
    // cursor-agent ≥ Sept 2025 emits MCP calls this way: there's no
    // `function`/`fooToolCall` wrapper, just a top-level `name` and an
    // arg dict. Without this branch the loop below fell straight to
    // the generic fallback, which is what produced the empty "using
    // tool…" pill.
    if let Some(name) = obj.get("name").and_then(|n| n.as_str()) {
        let input = obj
            .get("input")
            .or_else(|| obj.get("args"))
            .or_else(|| obj.get("arguments"))
            .cloned()
            .unwrap_or_else(|| json!({}));
        return (name.to_string(), input);
    }

    // Layer 2: split MCP shape — `tool_call: { mcp: { server, tool,
    // args } }` or `tool_call: { server, tool, args }`. Some cursor
    // builds split the namespace and the tool name into two fields
    // instead of pre-joining as `mcp__<server>__<tool>`. Re-stitch so
    // downstream `normalize_mcp_tool_name` + the frontend dispatcher
    // see the same `mcp__forgehold-app__open_github_pr` shape Claude
    // produces directly.
    let mcp_obj = obj.get("mcp").and_then(|m| m.as_object()).unwrap_or(obj);
    if let (Some(server), Some(tool)) = (
        mcp_obj.get("server").and_then(|s| s.as_str()),
        mcp_obj.get("tool").or_else(|| mcp_obj.get("name")).and_then(|t| t.as_str()),
    ) {
        let input = mcp_obj
            .get("args")
            .or_else(|| mcp_obj.get("input"))
            .or_else(|| mcp_obj.get("arguments"))
            .cloned()
            .unwrap_or_else(|| json!({}));
        return (format!("mcp__{}__{}", server, tool), input);
    }

    // Layer 3a: cursor-agent's MCP shape (≥ Q2-2026). Real shape from
    // a captured event:
    //   tool_call: {
    //     mcpToolCall: {
    //       args: {
    //         args: { …actual tool params… },
    //         name: "forgehold-app-open_github_pr",
    //         providerIdentifier: "forgehold-app",
    //         toolCallId: "toolu_…",
    //         toolName: "open_github_pr"
    //       }
    //     }
    //   }
    // Two `args` levels: the outer is a metadata wrapper, the inner is
    // the actual params dict. `name` joins server+tool with a single
    // dash, which DOESN'T match the `mcp__<server>__<tool>` convention
    // the rest of the codebase (frontend dispatcher, formatToolUse,
    // claude.rs) expects. So we re-stitch from `providerIdentifier` +
    // `toolName` instead, stripping the `forgehold-` prefix to match
    // Claude's namespace (where servers live as bare keys `app`,
    // `jira`, …). For 3rd-party MCP servers the user wires in via
    // `cursor-agent mcp add`, the prefix won't be present, so the full
    // provider id stays in the namespace slot — `mcp__<provider>__<tool>`,
    // exactly what `formatToolUse`'s generic mcp branch already
    // formats nicely.
    if let Some(mcp) = obj.get("mcpToolCall").and_then(|m| m.as_object()) {
        let outer = mcp.get("args").and_then(|a| a.as_object());
        if let Some(outer) = outer {
            let provider = outer
                .get("providerIdentifier")
                .and_then(|s| s.as_str())
                .unwrap_or("");
            let tool_name = outer.get("toolName").and_then(|s| s.as_str()).unwrap_or("");
            let inner_args = outer.get("args").cloned().unwrap_or_else(|| json!({}));
            if !tool_name.is_empty() && !provider.is_empty() {
                let namespace = provider.strip_prefix("forgehold-").unwrap_or(provider);
                return (format!("mcp__{}__{}", namespace, tool_name), inner_args);
            }
        }
    }

    // Layer 3b: discriminated-union shapes we've seen historically.
    //   readToolCall   → Read
    //   editToolCall   → cursor's full-file overwrite/append; we
    //                    re-shape it as Claude's `Write` so the
    //                    frontend's existing diff-card path fires
    //                    (oldText backfilled from `git show HEAD:…`)
    //   writeToolCall  → also re-shape as Write
    //   bashToolCall   → Bash
    //   grepToolCall   → Grep
    //   globToolCall   → Glob
    //   function       → OpenAI-style `function.name` + `function.args`
    // The payload object already has a human-meaningful `name` in most
    // cases; when it doesn't we fall back to humanizing the
    // discriminator (any `fooBarToolCall` → `FooBar`) so unknown
    // future tools at least render with a readable label instead of
    // dumping the raw event JSON.
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
                .or_else(|| payload.get("input").cloned())
                .unwrap_or_else(|| json!({}));
            return (name, input);
        }
        // Cursor's file-mutation tools: `editToolCall` and `writeToolCall`.
        //
        // We re-shape into Claude's `Write` input (`{file_path, content}`)
        // so the frontend's Write handler in claudeStream.ts produces an
        // inline EditDiffCard with the same apply/revert UX users get
        // for Claude. Without this the card path never triggers and
        // Cursor edits show only as text.
        //
        // Source-of-truth ordering for `content`:
        //   1. `result.success.afterFullFileContent` — the actual final
        //      file contents after the edit. Only present on `completed`
        //      events. ALWAYS prefer this when available.
        //   2. `args.content` — present on cursor's bare-write tool path.
        //   3. `args.streamContent` — last resort; this is a partial
        //      streaming chunk / surgical-edit spec, NOT the full file.
        //      Will produce a wrong diff if rendered, but better than
        //      nothing if cursor ever emits a started-only event.
        // (normalize_event filters edit/write to completed-only, so
        // afterFullFileContent should always be available in practice.)
        if key == "editToolCall" || key == "writeToolCall" {
            let args = payload.get("args").and_then(|a| a.as_object());
            let success = payload
                .get("result")
                .and_then(|r| r.get("success"))
                .and_then(|s| s.as_object());
            let path = args
                .and_then(|a| a.get("path").or_else(|| a.get("file_path")))
                .or_else(|| success.and_then(|s| s.get("path")))
                .and_then(|p| p.as_str())
                .unwrap_or("");
            let content = success
                .and_then(|s| s.get("afterFullFileContent"))
                .or_else(|| args.and_then(|a| a.get("content")))
                .or_else(|| args.and_then(|a| a.get("streamContent")))
                .and_then(|c| c.as_str())
                .unwrap_or("");
            if !path.is_empty() {
                return (
                    "Write".into(),
                    json!({ "file_path": path, "content": content }),
                );
            }
        }
        // Cursor's deletion tool: `deleteToolCall`. There's no diff to
        // render (file's gone, nothing to apply/revert against), so we
        // surface it as a trace pill via formatToolUse — frontend's
        // generic mcp-shaped renderer hits the `path` field and shows
        // "_deleted /path/to/file_". A future revision could keep
        // `result.success.prevContent` to offer a one-click restore,
        // but EditDiffCard's current model doesn't have a "deleted"
        // variant; leaving as a pill for now.
        if key == "deleteToolCall" {
            let args = payload.get("args").and_then(|a| a.as_object());
            let path = args
                .and_then(|a| a.get("path"))
                .and_then(|p| p.as_str())
                .unwrap_or("");
            if !path.is_empty() {
                return ("Delete".into(), json!({ "file_path": path }));
            }
        }
        if key.ends_with("ToolCall") {
            let name = payload
                .get("name")
                .and_then(|n| n.as_str())
                .map(String::from)
                .unwrap_or_else(|| humanize_discriminator(key));
            // Flatten the payload as the "input" so formatToolUse can dig out
            // familiar keys (file_path, command, pattern, …).
            let input = json!(payload);
            return (name, input);
        }
    }

    // Layer 4: nothing matched. Stash the raw payload under `_raw` so a
    // user reporting "blank trace pill" can paste it back to us — the
    // frontend's generic renderer will inline-code it via
    // `formatToolUse`'s "single-string-arg" branch. We keep `name=tool`
    // (not "") so the assistant card still renders, even ugly.
    (
        "tool".into(),
        json!({
            "_raw": serde_json::to_string(tc).unwrap_or_default(),
        }),
    )
}

/// Map cursor's `*ToolCall` discriminator to a human label. Hand-
/// curated aliases keep the names friendly (Read, not "ReadTool"), and
/// any unknown discriminator falls back to PascalCasing whatever sits
/// before the `ToolCall` suffix — so a fresh cursor-agent build that
/// adds `webSearchToolCall` renders as "WebSearch" instead of dumping
/// the raw event JSON. Returns String so the dynamic fallback case
/// can return owned data.
fn humanize_discriminator(key: &str) -> String {
    match key {
        "readToolCall" => return "Read".into(),
        "writeToolCall" | "editToolCall" => return "Write".into(),
        "bashToolCall" => return "Bash".into(),
        "grepToolCall" => return "Grep".into(),
        "globToolCall" => return "Glob".into(),
        // Safety net: if Layer 3a (the dedicated `mcpToolCall` handler)
        // failed to extract a name, surface "MCP" so the trace pill
        // reads "_using MCP…_" instead of the generic "_using tool…_".
        "mcpToolCall" => return "MCP".into(),
        _ => {}
    }
    // Generic fallback: strip `ToolCall`, capitalise. `searchToolCall`
    // → "Search", `lsToolCall` → "Ls", `runTerminalCmdToolCall` →
    // "RunTerminalCmd". Better than the previous "tool" default which
    // collapsed every unknown variant onto the same label and then
    // leaked the raw event JSON through the Layer-4 fallback below.
    let stem = key.strip_suffix("ToolCall").unwrap_or(key);
    if stem.is_empty() {
        return "tool".into();
    }
    let mut chars = stem.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => "tool".into(),
    }
}

/// Send SIGTERM to a running cursor-agent spawn for the given Forgehold session.
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

/// Read cursor-agent's session access token. Tries (in order):
///   1. Login Keychain — `cursor-access-token` / `cursor-user`. Older
///      cursor-agent builds (node-keytar) wrote here.
///   2. `~/.cursor/cli-config.json` `accessToken` field. Newer builds
///      (post-Sept 2025) write to a config file instead of Keychain on
///      first install.
///   3. `~/.cursor/access-token` plain text. Some self-installed builds
///      use this layout.
/// Returns the first non-empty hit. We feed whatever we find via
/// `CURSOR_AUTH_TOKEN` so the spawned cursor-agent doesn't have to make
/// its own (possibly-failing) Keychain call from inside our unsigned app
/// sandbox.
fn read_cursor_access_token() -> Option<String> {
    if let Some(t) = read_cursor_token_keychain() { return Some(t); }
    if let Some(t) = read_cursor_token_config_json() { return Some(t); }
    if let Some(t) = read_cursor_token_plain_file() { return Some(t); }
    None
}

fn read_cursor_token_keychain() -> Option<String> {
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

fn read_cursor_token_config_json() -> Option<String> {
    let h = home_dir()?;
    // Try common json layouts in order; first hit wins.
    for name in ["cli-config.json", "config.json", "credentials.json", "auth.json"] {
        let path = h.join(".cursor").join(name);
        if let Ok(raw) = std::fs::read_to_string(&path) {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) {
                for key in ["accessToken", "access_token", "token", "apiKey", "api_key"] {
                    if let Some(t) = v.get(key).and_then(|x| x.as_str()) {
                        let t = t.trim();
                        if !t.is_empty() { return Some(t.to_string()); }
                    }
                }
                // Some builds nest under {auth:{accessToken}} or {credentials:{token}}.
                for outer in ["auth", "credentials", "session"] {
                    if let Some(inner) = v.get(outer) {
                        for key in ["accessToken", "access_token", "token"] {
                            if let Some(t) = inner.get(key).and_then(|x| x.as_str()) {
                                let t = t.trim();
                                if !t.is_empty() { return Some(t.to_string()); }
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

fn read_cursor_token_plain_file() -> Option<String> {
    let h = home_dir()?;
    for name in ["access-token", "access_token", "token"] {
        let path = h.join(".cursor").join(name);
        if let Ok(raw) = std::fs::read_to_string(&path) {
            let t = raw.trim();
            if !t.is_empty() { return Some(t.to_string()); }
        }
    }
    None
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
