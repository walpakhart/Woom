//! Cursor Agent CLI adapter — parallel to `claude.rs`.
//!
//! Woom spawns `cursor-agent -p <prompt> --output-format stream-json
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
//!     `<workspace>/.cursor/mcp.json`. Woom's sidecars aren't wired in by
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
    /// `--resume <chat_id>` referenced a chat the cursor-agent backend no
    /// longer has. Frontend self-heals by minting a fresh chat (next turn
    /// drops resume + stamps a recap of in-memory history into the prompt).
    /// Heuristic detection — Cursor doesn't publish stable error codes, so
    /// the match is loose and false positives just trigger the same recap-
    /// based recovery, which is benign.
    #[error("cursor-agent resume target is gone: {0}")]
    ResumeOrphan(String),
    #[error("cursor-agent failed: {0}")]
    Failed(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Heuristic check: did cursor-agent fail because the chat_id we tried
/// to resume no longer exists? Phrasings observed: "chat not found",
/// "no chat with id", "unknown chat", "chat ... does not exist".
/// Loose match by design — see the variant doc above.
fn looks_like_cursor_resume_orphan(stderr: &str) -> bool {
    let t = stderr.to_ascii_lowercase();
    let chat_y = t.contains("chat") || t.contains("conversation") || t.contains("session");
    let phrasings_y = t.contains("not found")
        || t.contains("no such")
        || t.contains("does not exist")
        || t.contains("doesn't exist")
        || t.contains("could not find")
        || t.contains("cannot find")
        || t.contains("unknown");
    (chat_y && phrasings_y) || t.contains("no resumable") || t.contains("invalid chat id")
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
    //   2. App context — app-instance map snapshot, mutates per turn;
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
        // emits one big `assistant` event at the end. The Woom spinner
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
    // macOS quirk: when Woom.app is unsigned (ad-hoc) and spawns
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
    // Stateful normalizer: dedupes assistant text deltas across the
    // stream (cursor-agent has shipped versions that re-emit the same
    // partial chunk and versions that emit cumulative-mode chunks
    // instead of incremental). Without dedupe the frontend's append-
    // delta path doubles the visible text. See `StreamNormalizer`.
    let mut normalizer = StreamNormalizer::new();

    while let Ok(Some(raw)) = lines.next_line().await {
        for out in normalizer.normalize(&raw) {
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
        // Surface resume-orphan distinctly so the frontend can self-heal.
        // Only check on a `--resume` attempt — a fresh `create-chat` turn
        // can't be orphaned (we just minted the id).
        if resume && looks_like_cursor_resume_orphan(&stderr_text) {
            return Err(CursorRunError::ResumeOrphan(stderr_text));
        }
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
        // Same orphan check on the exit-0-but-empty path — cursor-agent
        // sometimes prints "chat not found" to stderr and exits cleanly
        // when the resume id is unknown.
        if resume && looks_like_cursor_resume_orphan(&stderr_text) {
            return Err(CursorRunError::ResumeOrphan(stderr_text));
        }
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

/// Stream-scoped state for cursor-agent → Claude-shape translation. One
/// instance per `run_streaming` invocation; threads through the stdout
/// loop so dedupe state survives across stream lines.
///
/// Sole job today: dedupe assistant text deltas. cursor-agent has shipped
/// at least four flavours of `assistant` partial output across versions:
///
///   1. INCREMENTAL (the documented contract) — each event carries the
///      *new* chunk only ("Прове" → "рю..."). Frontend appends each
///      chunk and the visible text grows correctly.
///   2. CUMULATIVE — each event carries the entire assistant-message
///      text accumulated so far ("Прове" → "Проверю..."). If we forward
///      these as-is the frontend's append-delta path layers full state
///      on top of full state and the text quadruples.
///   3. INCREMENTAL with retransmits — same chunk emitted twice in a
///      row (likely a CLI flush / network-coalescing artefact). Same
///      doubling symptom in the chat.
///   4. SELF-DOUBLED — a single partial whose body is verbatim-doubled
///      (`"…редактор:…редактор:"`). Observed in resume-mode for short
///      replies. Without collapsing this we can't even compute a
///      sensible tail against later partials.
///
/// We can collapse all four into one rule by tracking the cumulative
/// emitted text (NOT the previous delta) and forwarding only the
/// strict-suffix tail / collapsing self-doubled bodies / dropping
/// on exact match. A non-`assistant` event (tool_call, system,
/// result) resets the *active* baseline — partials only stream
/// contiguously inside one assistant message; once anything else
/// lands the model has moved on (started a tool, ended the turn) and
/// the next assistant text is a fresh message.
///
/// BUT cursor-agent's LLM has been observed shipping a duplicate of
/// the *just-completed* paragraph as the first chunk of the new
/// message after a `tool_call` (it likes to "recap context" before
/// taking the next action). That doubles the visible bubble. So in
/// addition to clearing the active baseline we ALSO snapshot the
/// just-completed text into `prev_completed_text` and use it to drop
/// (or chop the overlap off) the very next assistant chunk.
struct StreamNormalizer {
    /// Accumulated visible text we've already streamed on the current
    /// assistant turn — i.e. the concatenation of every delta we've
    /// forwarded. Used as the baseline for dedupe / tail extraction
    /// on the next partial. Tracks the WHOLE message, not just the
    /// previous chunk: that's the bug from the previous revision
    /// where mixing incremental partials with a later cumulative
    /// chunk caused the cumulative chunk to fall through and get
    /// emitted in full, doubling the message.
    last_assistant_text: String,
    /// Snapshot of `last_assistant_text` at the moment a non-assistant
    /// event landed (tool_call / system / result / user). Cleared
    /// once the next assistant chunk has been processed against it,
    /// so we only suppress the immediate post-tool repeat — later
    /// genuine repetition by the LLM is allowed to show.
    prev_completed_text: String,
}

impl StreamNormalizer {
    fn new() -> Self {
        Self {
            last_assistant_text: String::new(),
            prev_completed_text: String::new(),
        }
    }

    /// Snapshot the current message into `prev_completed_text` and
    /// reset the active baseline. Called whenever a non-assistant
    /// event lands so the next assistant chunk can be checked for a
    /// verbatim recap and chopped down.
    fn close_message(&mut self) {
        if !self.last_assistant_text.is_empty() {
            self.prev_completed_text = std::mem::take(&mut self.last_assistant_text);
        } else {
            self.last_assistant_text.clear();
        }
    }

    /// Translate one cursor-agent stream line into zero or more Claude-style
    /// events. Most events pass through (cursor's `assistant`/`result` shape is
    /// identical). `tool_call` → synthesized `assistant` event with a `tool_use`
    /// content block so `formatToolUse` on the frontend picks it up unchanged.
    fn normalize(&mut self, raw: &str) -> Vec<serde_json::Value> {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(raw) else {
            return Vec::new();
        };
        let Some(ty) = v.get("type").and_then(|t| t.as_str()) else {
            return Vec::new();
        };
        match ty {
            // Text deltas already match Claude's shape closely enough —
            // frontend only reads `type`, `message.content`, and `result`.
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
            //
            // For partials: feed through the dedupe rule described in
            // `StreamNormalizer`.
            "assistant" => {
                if v.get("timestamp_ms").is_none() {
                    return Vec::new();
                }
                self.handle_assistant(v)
            }
            "result" | "system" | "user" => {
                self.close_message();
                vec![v]
            }
            // tool_call: cursor-agent fires `started` (immediately on
            // dispatch) and `completed` (with the result) for every tool.
            //
            //   - For edit/write/delete tools we want the COMPLETED event so we
            //     can read `result.success.afterFullFileContent` — the
            //     actual final file contents. The `args.streamContent` on
            //     a `started` event is a partial / streamed edit-spec
            //     (cursor's edit tool isn't a plain Write — it can patch
            //     surgically based on a chunk that doesn't equal the full
            //     file). Rendering a diff card on started would show the
            //     wrong content.
            //
            //   - For other tools (read, grep, bash, glob, mcp, …) we emit
            //     the synthesized `tool_use` on STARTED so the user gets
            //     immediate "_using …_" feedback. We THEN also emit a
            //     synthesized `user`-role event with a `tool_result` block
            //     on COMPLETED so the captured output (file body, grep
            //     matches, bash stdout, …) flows through Woom's existing
            //     `attachOutputToLastTrace` path — same UX Claude has
            //     where every step gets a collapsible `output · N lines`
            //     card. Pre-fix this completed event was dropped, which
            //     is why Cursor's steps drawer showed only commands and
            //     never any output.
            "tool_call" => {
                self.close_message();
                let subtype = v.get("subtype").and_then(|s| s.as_str()).unwrap_or("");
                // Same reasoning as edit/write: `started` doesn't have the
                // pre-deletion file body, only `completed` does (in
                // `result.success.prevContent`). Without that field we can't
                // offer Restore — we'd just have a path. So we wait for the
                // post-tool event and treat the missing-prevContent case as
                // a degraded "show pill, no Restore" path inside the diff
                // card layer.
                let is_file_mutation = v
                    .get("tool_call")
                    .and_then(|tc| tc.as_object())
                    .is_some_and(|o| {
                        o.contains_key("editToolCall")
                            || o.contains_key("writeToolCall")
                            || o.contains_key("deleteToolCall")
                    });
                if is_file_mutation {
                    if subtype == "completed" {
                        return normalize_tool_call(&v).into_iter().collect();
                    }
                    return Vec::new();
                }
                /* Read/grep/bash/glob/mcp/… : emit `started` as the
                   tool_use, then emit `completed` as a tool_result so
                   the output card lands beside the call in the trace
                   pill. Both events carry the same `call_id`, which
                   `attachOutputToLastTrace` matches on indirectly via
                   trace-segment ordering — the output goes onto the
                   last open `‹toolcall›` envelope, which is the one
                   we just synthesized for the same call. */
                if subtype == "started" {
                    return normalize_tool_call(&v).into_iter().collect();
                }
                if subtype == "completed" {
                    if let Some(result_event) = synth_tool_result(&v) {
                        return vec![result_event];
                    }
                }
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    fn handle_assistant(&mut self, v: serde_json::Value) -> Vec<serde_json::Value> {
        // Pull the text payload, if any. content[] may be empty (cursor
        // emits placeholder partials) or carry only tool-use blocks
        // (we synthesize those elsewhere). In both cases nothing to
        // dedupe — forward unchanged.
        let text_opt: Option<String> = v
            .get("message")
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_array())
            .and_then(|arr| {
                arr.iter()
                    .find(|b| b.get("type").and_then(|t| t.as_str()) == Some("text"))
            })
            .and_then(|b| b.get("text"))
            .and_then(|t| t.as_str())
            .map(|s| s.to_string());

        let Some(text) = text_opt else {
            return vec![v];
        };

        // Empty text — drop. cursor-agent occasionally sends a
        // placeholder before the first real chunk; forwarding it
        // creates an empty assistant message bubble for a frame.
        if text.is_empty() {
            return Vec::new();
        }

        // Paranoid: cursor-agent has been observed shipping an entire
        // assistant message with its body verbatim-doubled in a single
        // partial — e.g. text = "Hello world.Hello world." as the FIRST
        // chunk we ever see for that message. The starts-with branch
        // below can't catch that because there's no previous baseline
        // to subtract. Detect a pure 2x repeat (front half == back half)
        // and collapse before any further processing. We require an
        // exact char-boundary split so we never slice inside a multi-
        // byte codepoint.
        let text = collapse_doubled(&text);

        // Post-tool recap dedupe. cursor-agent's LLM has been observed
        // opening the very next assistant chunk after a `tool_call`
        // with a verbatim repeat of the paragraph it just shipped
        // ("Папки с именем `ops` рядом с `efficiently` нет; открываю
        // efficiently--operations" — said once before the tool call,
        // then said again right after as "context" before the next
        // action). Without trimming, the frontend renders both and
        // the user sees the same paragraph twice. `close_message`
        // snapshots the just-completed text into `prev_completed_text`
        // exactly for this; we look at it here, ONCE, against the
        // first chunk of the new message, then drop it so subsequent
        // chunks (or genuinely repetitive output later in the turn)
        // aren't accidentally suppressed.
        let text = if !self.prev_completed_text.is_empty()
            && self.last_assistant_text.is_empty()
        {
            let prev = std::mem::take(&mut self.prev_completed_text);
            if text == prev {
                return Vec::new();
            }
            if text.starts_with(&prev) {
                text[prev.len()..].to_string()
            } else if prev.starts_with(&text) {
                return Vec::new();
            } else {
                text
            }
        } else {
            self.prev_completed_text.clear();
            text
        };

        // Empty after recap chop — nothing genuinely new in this chunk.
        if text.is_empty() {
            return Vec::new();
        }

        // Exact repeat — same chunk we just shipped. Either a CLI
        // retransmit OR cumulative-mode reporting "no progress yet".
        // Either way the frontend already has it.
        if text == self.last_assistant_text {
            return Vec::new();
        }

        // Cumulative-mode chunk: the new text begins with everything we
        // already streamed. Forward only the strictly new tail so the
        // frontend's append-delta path produces the correct end state.
        // Empty `last_assistant_text` is handled by the explicit guard
        // (otherwise `starts_with("")` is trivially true and we'd
        // emit the entire text as a "tail", which is fine but doesn't
        // exercise this branch — handled by the fall-through below).
        if !self.last_assistant_text.is_empty()
            && text.starts_with(&self.last_assistant_text)
        {
            let tail = text[self.last_assistant_text.len()..].to_string();

            // Defensive: cursor-agent sometimes emits a glitched
            // cumulative state where the tail is itself a verbatim
            // repeat of what we already streamed (the new text is
            // `last + last`). Treat it as a CLI retransmit and drop
            // — keep the baseline pinned at the clean version so a
            // subsequent partial can extend it normally.
            if tail == self.last_assistant_text {
                return Vec::new();
            }
            // Same idea, less rigid: tail begins with everything we've
            // already streamed (then continues). That happens when
            // cursor sends `last + last + new`. Preserve the genuinely
            // new suffix only.
            let tail_emit = if tail.starts_with(&self.last_assistant_text) {
                tail[self.last_assistant_text.len()..].to_string()
            } else {
                tail
            };
            if tail_emit.is_empty() {
                return Vec::new();
            }
            // Baseline becomes the entire visible text we've streamed
            // — i.e. the cumulative end state of this partial. Future
            // partials' starts-with check runs against the WHOLE
            // visible reply, which is what we want.
            self.last_assistant_text = text;
            let mut clone = v;
            if let Some(content) = clone
                .get_mut("message")
                .and_then(|m| m.get_mut("content"))
                .and_then(|c| c.as_array_mut())
            {
                for block in content.iter_mut() {
                    if block.get("type").and_then(|t| t.as_str()) == Some("text") {
                        if let Some(t) = block.get_mut("text") {
                            *t = json!(tail_emit);
                        }
                    }
                }
            }
            return vec![clone];
        }

        // Incremental delta OR a genuinely new prefix (uncommon — model
        // produces unrelated continuation that doesn't share a prefix
        // with our baseline). Forward as-is, append onto the running
        // baseline so the next cumulative-style partial computes its
        // tail against the WHOLE emitted text. The previous revision
        // *replaced* the baseline with the current delta, which broke
        // the moment cursor-agent followed an incremental partial with
        // a cumulative one — `starts_with` only matched the last
        // delta, the cumulative chunk fell through to this branch,
        // and got re-emitted in full, doubling the visible message.
        //
        // Always restamp the cleaned `text` back into the JSON before
        // forwarding: `collapse_doubled` may have rewritten a self-
        // doubled glitch chunk and the frontend should see the clean
        // version, not the original.
        let mut clone = v;
        if let Some(content) = clone
            .get_mut("message")
            .and_then(|m| m.get_mut("content"))
            .and_then(|c| c.as_array_mut())
        {
            for block in content.iter_mut() {
                if block.get("type").and_then(|t| t.as_str()) == Some("text") {
                    if let Some(t) = block.get_mut("text") {
                        *t = json!(text.clone());
                    }
                }
            }
        }
        self.last_assistant_text.push_str(&text);
        vec![clone]
    }
}

/// Detect and collapse a payload whose front half equals its back half
/// — a glitch shape we've seen cursor-agent ship as a single partial.
/// Returns the collapsed half on a hit, the original text otherwise.
/// Only runs when the byte length is even AND the midpoint lands on a
/// UTF-8 char boundary (so we never split inside a multi-byte codepoint
/// — Cyrillic / emoji / CJK would silently corrupt otherwise).
fn collapse_doubled(text: &str) -> String {
    let len = text.len();
    if len < 2 || len % 2 != 0 {
        return text.to_string();
    }
    let mid = len / 2;
    if !text.is_char_boundary(mid) {
        return text.to_string();
    }
    let (front, back) = text.split_at(mid);
    if front == back {
        front.to_string()
    } else {
        text.to_string()
    }
}

/// Take a `tool_call.completed` event for a non-mutation tool and turn it
/// into a Claude-shaped `user`-role event whose content is a single
/// `tool_result` block. The frontend's `agentStream.ts` reads exactly this
/// shape (`extractToolResultText` + `attachOutputToLastTrace`) to glue
/// the output card onto the matching toolcall envelope, so going through
/// the same path means cursor-agent steps render with the same
/// "command + output · N lines" UX Claude already has.
///
/// Output extraction is best-effort across the shapes cursor-agent has
/// shipped:
///   - `result.success.contents` / `content` / `output` / `stdout` —
///     plain string body for read/grep/bash/glob/mcp.
///   - `result.success.matches[]` — grep can return structured matches;
///     we join them as `path:line: text` lines.
///   - `result.error.message` (or just `error`) — surface failures so
///     the user sees "command failed: …" instead of an empty drawer.
/// Returns `None` when no usable text could be salvaged — falls back to
/// the pre-fix behavior (no output card, just the tool_use pill).
fn synth_tool_result(v: &serde_json::Value) -> Option<serde_json::Value> {
    let call_id = v.get("call_id").and_then(|c| c.as_str()).unwrap_or("");
    let session_id = v.get("session_id").cloned().unwrap_or(json!(null));
    let tc = v.get("tool_call");
    let result = tc.and_then(|t| t.as_object()).and_then(|o| {
        // `result` lives one level deeper: tool_call: { fooToolCall: { result: { success | error } } }
        // Some shapes also bury it as tool_call: { result: … } directly.
        for (_, payload) in o.iter() {
            if let Some(r) = payload.get("result") {
                return Some(r.clone());
            }
        }
        o.get("result").cloned()
    });
    let result = result.unwrap_or_else(|| json!(null));
    let success = result.get("success");
    let error = result.get("error");

    // Pull a text body. Prefer the most explicit field cursor-agent
    // produces for the kind of tool, then fall back to anything
    // string-shaped on success.
    let mut body: String = String::new();
    if let Some(s) = success {
        let candidates = [
            "contents", "content", "output", "stdout", "result", "text",
            "matches",
        ];
        for key in candidates {
            if let Some(v) = s.get(key) {
                if let Some(s) = v.as_str() {
                    body = s.to_string();
                    break;
                }
                if let Some(arr) = v.as_array() {
                    /* Grep matches: try `{path, line, text}` records,
                       then fall back to flat strings, then JSON. */
                    let mut lines: Vec<String> = Vec::new();
                    for item in arr {
                        if let Some(obj) = item.as_object() {
                            let path = obj
                                .get("path")
                                .or_else(|| obj.get("file"))
                                .and_then(|p| p.as_str())
                                .unwrap_or("");
                            let line = obj
                                .get("line")
                                .or_else(|| obj.get("lineNumber"))
                                .and_then(|n| n.as_u64())
                                .unwrap_or(0);
                            let text = obj
                                .get("text")
                                .or_else(|| obj.get("match"))
                                .or_else(|| obj.get("preview"))
                                .and_then(|t| t.as_str())
                                .unwrap_or("");
                            if !path.is_empty() {
                                lines.push(format!("{path}:{line}: {text}"));
                            } else if !text.is_empty() {
                                lines.push(text.to_string());
                            } else {
                                lines.push(serde_json::to_string(item).unwrap_or_default());
                            }
                        } else if let Some(s) = item.as_str() {
                            lines.push(s.to_string());
                        } else {
                            lines.push(serde_json::to_string(item).unwrap_or_default());
                        }
                    }
                    if !lines.is_empty() {
                        body = lines.join("\n");
                        break;
                    }
                }
            }
        }
        // Bash: cursor-agent reports an `exitCode` we can surface alongside.
        if let (Some(code), false) = (s.get("exitCode").and_then(|n| n.as_i64()), body.is_empty()) {
            if code != 0 {
                body = format!("{body}\n(exit {code})");
            }
        }
    }
    if body.is_empty() {
        if let Some(e) = error {
            let msg = e
                .get("message")
                .or_else(|| e.get("text"))
                .and_then(|m| m.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| serde_json::to_string(e).unwrap_or_default());
            body = format!("error: {msg}");
        }
    }
    if body.is_empty() {
        return None;
    }
    /* Cap the length here — `attachOutputToLastTrace` does its own
       truncation but we shouldn't ship megabytes through the IPC
       channel just to immediately throw it away. 16 KiB is generous
       enough for typical reads/greps and small enough to keep the
       sidecar → renderer hop cheap. */
    const MAX: usize = 16 * 1024;
    if body.len() > MAX {
        let mut end = MAX;
        while !body.is_char_boundary(end) {
            end -= 1;
        }
        body.truncate(end);
        body.push_str("\n…(truncated)");
    }
    Some(json!({
        "type": "user",
        "message": {
            "role": "user",
            "content": [{
                "type": "tool_result",
                "tool_use_id": call_id,
                "content": [{ "type": "text", "text": body }],
            }],
        },
        "session_id": session_id,
    }))
}

/// Cursor's `tool_call` event carries a discriminated-union payload; pick out
/// the readable tool name + args and re-emit as a Claude-style `tool_use`
/// content block. We only handle the `started` subtype (tool *invocation*) —
/// `completed` carries the tool *result*, which `synth_tool_result` above
/// converts to a Claude `tool_result` envelope so the trace pill gets an
/// output card.
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
/// The path is `~/Library/Logs/Woom/cursor-unknown-tool-calls.log`
/// on macOS, falling back to `$HOME/.woom-cursor-unknown.log`.
fn log_unknown_tool_call(v: &serde_json::Value) {
    let Ok(home) = std::env::var("HOME") else {
        return;
    };
    let dir = if cfg!(target_os = "macos") {
        PathBuf::from(&home).join("Library/Logs/Woom")
    } else {
        PathBuf::from(&home).join(".woom")
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

/// Strip the `woom-` prefix from MCP tool names so cursor-agent's
/// `mcp__woom-app__open_github_pr` lines up with the
/// `mcp__app__open_github_pr` shape the frontend's `handleStreamEvent` +
/// `handleAppNavigation` dispatcher matches on. Same fix for the other
/// sidecars (jira / github / sentry / memory).
///
/// Why the names diverge: Claude is invoked with `--mcp-config` listing
/// servers under bare keys (`app`, `jira`, …) so its tool names land at
/// `mcp__app__…`. Cursor reads `~/.cursor/mcp.json`, which uses the
/// public `woom-app` / `woom-jira` namespace (it's the
/// global MCP namespace shared with anything else the user wires in
/// via `cursor-agent mcp add`, so we don't squat on `app`). Without
/// this normalization the frontend silently drops every navigation
/// call — cursor-agent reports "tool succeeded" (the sidecar always
/// answers OK), but no UI mutation runs. That's the "Cursor says it
/// opened the PR but nothing happened" bug.
fn normalize_mcp_tool_name(name: &str) -> String {
    if let Some(rest) = name.strip_prefix("mcp__woom-") {
        // `rest` is now e.g. `app__open_github_pr`. Re-prefix to land at
        // the Claude-shaped `mcp__app__open_github_pr`.
        return format!("mcp__{}", rest);
    }
    name.to_string()
}

/// Reach into cursor's discriminated tool_call union (readToolCall /
/// writeToolCall / function / mcp / …) and return a `(tool_name,
/// input_object)` pair. Best-effort — Woom's `formatToolUse`
/// gracefully falls back to a compact generic render for unknown
/// names/shapes.
///
/// We probe in three layers, most-specific → most-general, so a cleaner
/// shape always wins over a fallback. cursor-agent has shipped four
/// distinct envelopes for tool_calls across versions; this used to only
/// handle two, which is why MCP calls (e.g. `mcp__woom-app__open_github_pr`)
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
    //
    // We canonicalize the name (`bash` → `Bash`, `grep` → `Grep`, …) so
    // the frontend's `formatToolUse` per-tool branches match. cursor-
    // agent has been seen emitting both lowercase and PascalCase names
    // for the same tool depending on build, and a lowercase `bash` /
    // `grep` slipped through to the generic fallback before, which is
    // why the steps drawer rendered them argless.
    if let Some(name) = obj.get("name").and_then(|n| n.as_str()) {
        let input = obj
            .get("input")
            .or_else(|| obj.get("args"))
            .or_else(|| obj.get("arguments"))
            .cloned()
            .unwrap_or_else(|| json!({}));
        return (canonicalize_tool_name(name), input);
    }

    // Layer 2: split MCP shape — `tool_call: { mcp: { server, tool,
    // args } }` or `tool_call: { server, tool, args }`. Some cursor
    // builds split the namespace and the tool name into two fields
    // instead of pre-joining as `mcp__<server>__<tool>`. Re-stitch so
    // downstream `normalize_mcp_tool_name` + the frontend dispatcher
    // see the same `mcp__woom-app__open_github_pr` shape Claude
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
    //         name: "woom-app-open_github_pr",
    //         providerIdentifier: "woom-app",
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
    // `toolName` instead, stripping the `woom-` prefix to match
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
                let namespace = provider.strip_prefix("woom-").unwrap_or(provider);
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
        // so the frontend's Write handler in agentStream.ts produces an
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
        //
        // We also opportunistically pluck `beforeFullFileContent` (or any
        // pre-state shape cursor may add later) and forward it as
        // `prev_content`. agentStream.ts's Write handler uses it as the
        // exact pre-agent baseline — no `git show HEAD:` round-trip, no
        // misclassification of "modify" as "create" when the lookup
        // fails. Empty string ⇒ field absent, FE falls back to git
        // backfill exactly as before.
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
            let prev = success
                .and_then(|s| {
                    s.get("beforeFullFileContent")
                        .or_else(|| s.get("prevContent"))
                        .or_else(|| s.get("priorContent"))
                        .or_else(|| s.get("originalContent"))
                })
                .and_then(|c| c.as_str())
                .unwrap_or("");
            if !path.is_empty() {
                return (
                    "Write".into(),
                    json!({
                        "file_path": path,
                        "content": content,
                        "prev_content": prev,
                    }),
                );
            }
        }
        // Cursor's deletion tool: `deleteToolCall`. We surface it as a
        // dedicated `Delete` tool name with a `prev_content` payload so
        // agentStream.ts can ship an EditDiffCard with an "isDelete"
        // flag — same Keep/Restore UX users get for Edit/Write reverts.
        //
        // Source-of-truth for `prev_content`:
        //   1. `result.success.prevContent` — the pre-deletion file
        //      body. Only present on `completed` events. ALWAYS
        //      preferred when available (most accurate; reflects the
        //      exact moment-of-deletion state, including any uncommitted
        //      changes the user had).
        //   2. (none) — degrades the card to a "deleted" pill with
        //      empty content. The frontend then has the option to
        //      backfill via `git show HEAD:<file>` for tracked files,
        //      same fallback path Write uses.
        // (normalize_event filters delete to completed-only, so
        // `prevContent` should be available in practice; we still
        // tolerate its absence to keep the card path robust against
        // future cursor-agent shape changes.)
        if key == "deleteToolCall" {
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
            let prev = success
                .and_then(|s| {
                    s.get("prevContent")
                        .or_else(|| s.get("beforeFullFileContent"))
                        .or_else(|| s.get("content"))
                })
                .and_then(|c| c.as_str())
                .unwrap_or("");
            if !path.is_empty() {
                return (
                    "Delete".into(),
                    json!({ "file_path": path, "prev_content": prev }),
                );
            }
        }
        // Bash / Read / Grep / Glob: cursor-agent nests parameters one
        // level deep under `args` (`{shellToolCall: {args: {command:
        // …, description: …}}}`), and `formatToolUse` on the frontend
        // expects them at the *top* level of `input`. Forwarding the
        // whole payload means formatToolUse can't find `command` /
        // `pattern` / `file_path` / `path` and falls into its generic
        // `_using <name>…_` branch — which is the "grep, grep, grep,
        // read, read" symptom in the steps drawer (no args visible).
        //
        // We flatten `args` onto the input here so the per-tool
        // branches in formatToolUse pick up the actual parameters.
        // Read also re-keys `path` → `file_path` because cursor uses
        // `path` while Claude (and our formatter) uses `file_path`.
        if key == "shellToolCall" || key == "bashToolCall" {
            let mut input = payload
                .get("args")
                .cloned()
                .unwrap_or_else(|| json!({}));
            if !input.is_object() {
                input = json!({});
            }
            return ("Bash".into(), input);
        }
        if key == "readToolCall" {
            let args = payload.get("args").and_then(|a| a.as_object());
            let mut input = serde_json::Map::new();
            if let Some(a) = args {
                if let Some(p) = a
                    .get("path")
                    .or_else(|| a.get("file_path"))
                    .and_then(|s| s.as_str())
                {
                    input.insert("file_path".into(), json!(p));
                }
                for k in [
                    "offset",
                    "limit",
                    "startLine",
                    "endLine",
                    "start_line",
                    "end_line",
                ] {
                    if let Some(v) = a.get(k) {
                        input.insert(k.to_string(), v.clone());
                    }
                }
            }
            return ("Read".into(), serde_json::Value::Object(input));
        }
        if key == "grepToolCall" {
            let mut input = payload
                .get("args")
                .cloned()
                .unwrap_or_else(|| json!({}));
            if !input.is_object() {
                input = json!({});
            }
            return ("Grep".into(), input);
        }
        if key == "globToolCall" {
            let mut input = payload
                .get("args")
                .cloned()
                .unwrap_or_else(|| json!({}));
            if !input.is_object() {
                input = json!({});
            }
            return ("Glob".into(), input);
        }
        if key.ends_with("ToolCall") {
            let raw_name = payload
                .get("name")
                .and_then(|n| n.as_str())
                .map(String::from)
                .unwrap_or_else(|| humanize_discriminator(key));
            // Same flattening rationale as the named branches above:
            // formatToolUse looks for parameters at the top level of
            // `input`, so unwrap `args` if it's there and fall back to
            // the whole payload only when there's nothing else useful.
            let input = payload
                .get("args")
                .cloned()
                .filter(|v| v.is_object())
                .unwrap_or_else(|| json!(payload));
            return (canonicalize_tool_name(&raw_name), input);
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
        // cursor-agent ≥ April 2026 ships `shellToolCall` instead of
        // `bashToolCall`. Both map to Bash on the frontend so the same
        // `$ <command>` rendering and the Bash-rm interception in
        // agentStream.ts both fire regardless of CLI version.
        "bashToolCall" | "shellToolCall" => return "Bash".into(),
        "grepToolCall" => return "Grep".into(),
        "globToolCall" => return "Glob".into(),
        "deleteToolCall" => return "Delete".into(),
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

/// Map a tool name (whatever cursor-agent reported — `bash`, `Bash`,
/// `shell`, `grep`, `read_file`, …) to the canonical PascalCase label
/// that `formatToolUse` on the frontend recognizes. Names with the
/// `mcp__` prefix pass through unchanged (we never want to reshape
/// MCP namespaces). Unknown names also pass through, so a brand-new
/// cursor tool still renders by its CLI name instead of disappearing.
///
/// Why this matters: `formatToolUse('Grep', …)` → `_grep_ \`pattern\``
/// (with args) but `formatToolUse('grep', …)` lands in the generic
/// fallback that doesn't know about `pattern` / `path` and emits
/// `_using grep…_`. Same problem hit Bash/Read/Write/etc. Canonicalizing
/// up front means every downstream branch fires.
fn canonicalize_tool_name(name: &str) -> String {
    if name.starts_with("mcp__") {
        return name.to_string();
    }
    let lc = name.to_ascii_lowercase();
    match lc.as_str() {
        "bash" | "shell" | "terminal" | "exec" => "Bash".into(),
        "read" | "readfile" | "read_file" | "view" | "open_file" => "Read".into(),
        "write" | "writefile" | "write_file" | "create_file" => "Write".into(),
        "edit" | "editfile" | "edit_file" | "patch" => "Edit".into(),
        "delete" | "deletefile" | "delete_file" | "remove" => "Delete".into(),
        "grep" | "search_files" | "rg" => "Grep".into(),
        "glob" | "find_files" => "Glob".into(),
        "ls" | "list_dir" | "listdir" => "LS".into(),
        "webfetch" | "web_fetch" => "WebFetch".into(),
        "websearch" | "web_search" => "WebSearch".into(),
        "todowrite" | "todo_write" => "TodoWrite".into(),
        "notebookedit" | "notebook_edit" => "NotebookEdit".into(),
        _ => name.to_string(),
    }
}

/// Send SIGTERM to a running cursor-agent spawn for the given Woom session.
/// SIGTERM → wait 2s → SIGKILL fallback. Same rationale as `claude::stop`:
/// a cursor-agent stuck in a syscall (network, MCP RPC) ignores SIGTERM
/// and keeps its chat lock; without the kill the next `--resume` fails.
pub fn stop(runners: &Runners, session_id: &str) -> bool {
    let pid = runners.lock().unwrap().get(session_id).copied();
    match pid {
        Some(p) if p > 0 => {
            let sigterm_ok = unsafe { libc::kill(p as libc::pid_t, libc::SIGTERM) == 0 };
            // `agent::stop` is invoked from a sync Tauri command, so
            // `tokio::spawn` panics here. Use Tauri's runtime-agnostic
            // spawn to dispatch the SIGKILL escalation timer instead.
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(2000)).await;
                if unsafe { libc::kill(p as libc::pid_t, 0) } == 0 {
                    unsafe { libc::kill(p as libc::pid_t, libc::SIGKILL) };
                }
            });
            sigterm_ok
        }
        _ => false,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn assistant_text(v: &serde_json::Value) -> Option<&str> {
        v.get("message")?
            .get("content")?
            .as_array()?
            .iter()
            .find(|b| b.get("type").and_then(|t| t.as_str()) == Some("text"))?
            .get("text")?
            .as_str()
    }

    #[test]
    fn dedupe_passes_incremental_partials_through() {
        let mut n = StreamNormalizer::new();
        let line = |t: &str| {
            format!(
                r#"{{"type":"assistant","message":{{"role":"assistant","content":[{{"type":"text","text":"{}"}}]}},"timestamp_ms":1}}"#,
                t
            )
        };
        let p1 = n.normalize(&line("Прове"));
        let p2 = n.normalize(&line("рю..."));
        assert_eq!(p1.len(), 1);
        assert_eq!(assistant_text(&p1[0]), Some("Прове"));
        assert_eq!(p2.len(), 1);
        assert_eq!(assistant_text(&p2[0]), Some("рю..."));
    }

    #[test]
    fn dedupe_collapses_cumulative_partials_to_tail() {
        let mut n = StreamNormalizer::new();
        let line = |t: &str| {
            format!(
                r#"{{"type":"assistant","message":{{"role":"assistant","content":[{{"type":"text","text":"{}"}}]}},"timestamp_ms":1}}"#,
                t
            )
        };
        let p1 = n.normalize(&line("Привет"));
        let p2 = n.normalize(&line("Приветмир"));
        assert_eq!(assistant_text(&p1[0]), Some("Привет"));
        assert_eq!(assistant_text(&p2[0]), Some("мир"));
    }

    #[test]
    fn dedupe_drops_exact_repeat_partials() {
        let mut n = StreamNormalizer::new();
        let line = r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"Проверю..."}]},"timestamp_ms":1}"#;
        let p1 = n.normalize(line);
        let p2 = n.normalize(line);
        assert_eq!(p1.len(), 1);
        assert_eq!(assistant_text(&p1[0]), Some("Проверю..."));
        assert!(
            p2.is_empty(),
            "exact repeat must be dropped, got {:?}",
            p2
        );
    }

    #[test]
    fn dedupe_resets_baseline_after_tool_call_different_text() {
        // After a `tool_call` the active baseline must reset so a
        // genuinely-new continuation isn't accidentally treated as a
        // cumulative chunk of the prior message. Use *different* text
        // here — the verbatim-recap case is covered by
        // `dedupe_drops_post_tool_recap_repeat`, and that's a stronger
        // dedupe (one-shot, gated on `prev_completed_text`) that
        // co-exists with this baseline-reset behaviour.
        let mut n = StreamNormalizer::new();
        let assistant = |t: &str| {
            format!(
                r#"{{"type":"assistant","message":{{"role":"assistant","content":[{{"type":"text","text":"{}"}}]}},"timestamp_ms":1}}"#,
                t
            )
        };
        let _ = n.normalize(&assistant("Done."));
        let tool_line = r#"{"type":"tool_call","subtype":"started","call_id":"x","tool_call":{"shellToolCall":{"args":{"command":"ls"}}}}"#;
        let _ = n.normalize(tool_line);
        // Genuinely-new follow-up after the tool ran. Must pass.
        let p2 = n.normalize(&assistant("Now switching files."));
        assert_eq!(p2.len(), 1, "unrelated post-tool text must pass");
        assert_eq!(assistant_text(&p2[0]), Some("Now switching files."));
    }

    #[test]
    fn dedupe_drops_final_summary_assistant() {
        let mut n = StreamNormalizer::new();
        // No `timestamp_ms` → final summary, must be dropped (the `result`
        // event re-stamps the clean text via `replaceLastAssistant`).
        let line = r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"Final"}]}}"#;
        let out = n.normalize(line);
        assert!(out.is_empty(), "final summary must be dropped, got {:?}", out);
    }

    #[test]
    fn dedupe_collapses_self_doubled_first_partial() {
        let mut n = StreamNormalizer::new();
        // cursor-agent has been observed shipping the entire body
        // verbatim-doubled in a single first partial. Without
        // `collapse_doubled` this slipped through as-is and rendered
        // as "Hello world.Hello world." in the bubble.
        let line = r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"Поищу прямо отсюда:Поищу прямо отсюда:"}]},"timestamp_ms":1}"#;
        let out = n.normalize(line);
        assert_eq!(out.len(), 1);
        assert_eq!(
            assistant_text(&out[0]),
            Some("Поищу прямо отсюда:"),
            "self-doubled payload must be collapsed to one half"
        );
    }

    #[test]
    fn dedupe_drops_doubled_cumulative_tail() {
        let mut n = StreamNormalizer::new();
        let line = |t: &str| {
            format!(
                r#"{{"type":"assistant","message":{{"role":"assistant","content":[{{"type":"text","text":"{}"}}]}},"timestamp_ms":1}}"#,
                t
            )
        };
        // Stream: clean partial, then a corrupt cumulative state where
        // the body is repeated end-to-end. Tail equals baseline → drop.
        let _ = n.normalize(&line("Hello"));
        let p2 = n.normalize(&line("HelloHello"));
        assert!(
            p2.is_empty(),
            "cumulative chunk whose tail repeats baseline must be dropped, got {:?}",
            p2
        );
    }

    #[test]
    fn collapse_doubled_preserves_unique_text() {
        // Sanity: a non-doubled string passes through unchanged. We
        // never want to collapse legitimate "abab" patterns where the
        // halves only happen to look similar.
        assert_eq!(collapse_doubled("Hello world"), "Hello world");
        assert_eq!(collapse_doubled("abc"), "abc"); // odd length
        assert_eq!(collapse_doubled("aabb"), "aabb"); // halves differ
        assert_eq!(collapse_doubled(""), "");
    }

    #[test]
    fn dedupe_handles_mixed_incremental_then_cumulative() {
        let mut n = StreamNormalizer::new();
        let line = |t: &str| {
            format!(
                r#"{{"type":"assistant","message":{{"role":"assistant","content":[{{"type":"text","text":"{}"}}]}},"timestamp_ms":1}}"#,
                t
            )
        };

        // cursor-agent has been observed mixing INCREMENTAL partials
        // for the first chunks of a message with a CUMULATIVE chunk
        // at the end (typically the model's "final state" flush).
        // Without baseline-accumulation in handle_assistant, the
        // cumulative chunk fell through `starts_with` (because the
        // baseline was just the previous delta " world", not the
        // full " Hello world"), got emitted in full, and the bubble
        // ended up as "Hello world" + "Hello world more" =
        // doubled.
        let p1 = n.normalize(&line("Hello"));
        let p2 = n.normalize(&line(" world"));
        let p3 = n.normalize(&line("Hello world more"));

        assert_eq!(assistant_text(&p1[0]), Some("Hello"));
        assert_eq!(assistant_text(&p2[0]), Some(" world"));
        assert_eq!(
            assistant_text(&p3[0]),
            Some(" more"),
            "cumulative partial after incremental ones must be reduced to the new tail"
        );
    }

    #[test]
    fn dedupe_drops_self_doubled_cumulative_after_clean_partial() {
        // The screenshot scenario: cursor-agent ships a single
        // incremental partial with the full short reply, then a
        // self-doubled cumulative chunk (`prefix + prefix`) right
        // before the final summary. We must drop that doubled
        // chunk; the user can't see "Поищу...:Поищу...:" twice.
        let mut n = StreamNormalizer::new();
        let line = |t: &str| {
            format!(
                r#"{{"type":"assistant","message":{{"role":"assistant","content":[{{"type":"text","text":"{}"}}]}},"timestamp_ms":1}}"#,
                t
            )
        };
        let p1 = n.normalize(&line("Поищу прямо отсюда:"));
        let p2 = n.normalize(&line("Поищу прямо отсюда:Поищу прямо отсюда:"));

        assert_eq!(p1.len(), 1);
        assert_eq!(assistant_text(&p1[0]), Some("Поищу прямо отсюда:"));
        assert!(
            p2.is_empty(),
            "doubled cumulative chunk must be dropped; got {:?}",
            p2
        );
    }

    #[test]
    fn collapse_doubled_handles_multibyte() {
        // The midpoint of "ы:" is 3 bytes; splitting at byte 3 is on a
        // char boundary so it's safe. The doubled "ы:ы:" splits cleanly
        // and collapses to "ы:". The off-boundary case "ы" (2 bytes,
        // mid at 1, NOT a boundary) must pass through unchanged
        // instead of panicking.
        assert_eq!(collapse_doubled("ы:ы:"), "ы:");
        assert_eq!(collapse_doubled("ы"), "ы");
    }

    #[test]
    fn dedupe_drops_post_tool_recap_repeat() {
        // The exact scenario from the editor screenshot: cursor-agent
        // shipped a paragraph, fired a `tool_call` (read file), then
        // shipped the SAME paragraph again as the first chunk of the
        // next assistant message. Without `prev_completed_text` snap-
        // shot the second chunk fell through and rendered as a
        // duplicate bubble.
        let mut n = StreamNormalizer::new();
        let assistant = |t: &str| {
            format!(
                r#"{{"type":"assistant","message":{{"role":"assistant","content":[{{"type":"text","text":"{}"}}]}},"timestamp_ms":1}}"#,
                t
            )
        };
        // Minimal `tool_call` started event with a non-mutating tool
        // (read) so the normalizer keeps it; details don't matter for
        // this test, only that close_message() runs.
        let tool_call = r#"{"type":"tool_call","subtype":"started","tool_call":{"readToolCall":{"args":{"path":"foo"}}}}"#;

        let body = "Папки с именем `ops` рядом с `efficiently` нет; открываю efficiently--operations.";
        let p1 = n.normalize(&assistant(body));
        let _tc = n.normalize(tool_call);
        let p2 = n.normalize(&assistant(body));

        assert_eq!(assistant_text(&p1[0]), Some(body));
        assert!(
            p2.is_empty(),
            "post-tool verbatim recap must be dropped; got {:?}",
            p2
        );
    }

    #[test]
    fn dedupe_chops_post_tool_recap_prefix_keeps_continuation() {
        // Variant where the LLM doesn't just verbatim-recap — it
        // recaps THEN continues. We must chop the prefix so the bubble
        // shows only the genuinely new tail.
        let mut n = StreamNormalizer::new();
        let assistant = |t: &str| {
            format!(
                r#"{{"type":"assistant","message":{{"role":"assistant","content":[{{"type":"text","text":"{}"}}]}},"timestamp_ms":1}}"#,
                t
            )
        };
        let tool_call = r#"{"type":"tool_call","subtype":"started","tool_call":{"readToolCall":{"args":{"path":"foo"}}}}"#;

        let body = "Открываю efficiently--operations.";
        let body_extended = "Открываю efficiently--operations. Вот что я нашёл там.";
        let p1 = n.normalize(&assistant(body));
        let _tc = n.normalize(tool_call);
        let p2 = n.normalize(&assistant(body_extended));

        assert_eq!(assistant_text(&p1[0]), Some(body));
        assert_eq!(
            assistant_text(&p2[0]),
            Some(" Вот что я нашёл там."),
            "recap prefix must be chopped; only the continuation should reach the bubble"
        );
    }

    #[test]
    fn dedupe_lets_genuinely_different_post_tool_text_through() {
        // Sanity: a brand-new paragraph after a tool_call (no overlap
        // with the pre-tool text) must NOT be silenced. The recap
        // dedupe is a one-shot suppression — if the new chunk is
        // unrelated to `prev_completed_text` we forward it intact.
        let mut n = StreamNormalizer::new();
        let assistant = |t: &str| {
            format!(
                r#"{{"type":"assistant","message":{{"role":"assistant","content":[{{"type":"text","text":"{}"}}]}},"timestamp_ms":1}}"#,
                t
            )
        };
        let tool_call = r#"{"type":"tool_call","subtype":"started","tool_call":{"readToolCall":{"args":{"path":"foo"}}}}"#;

        let p1 = n.normalize(&assistant("Сначала ищу путь."));
        let _tc = n.normalize(tool_call);
        let p2 = n.normalize(&assistant("Готово, вот результат."));

        assert_eq!(assistant_text(&p1[0]), Some("Сначала ищу путь."));
        assert_eq!(
            assistant_text(&p2[0]),
            Some("Готово, вот результат."),
            "unrelated post-tool text must pass through unchanged"
        );
    }

    /// `extract_tool_shape` consumes the inner `tool_call` payload — the
    /// `{shellToolCall: {…}}` style object cursor-agent emits. Tests
    /// build that object directly and feed it in.
    fn shape_for(payload: serde_json::Value) -> (String, serde_json::Value) {
        extract_tool_shape(&payload)
    }

    #[test]
    fn shell_tool_call_maps_to_bash_with_command() {
        let payload = json!({
            "shellToolCall": {
                "args": {
                    "command": "ls -la",
                    "description": "List files in workspace",
                }
            }
        });
        let (name, input) = shape_for(payload);
        assert_eq!(name, "Bash");
        assert_eq!(input.get("command").and_then(|s| s.as_str()), Some("ls -la"));
    }

    #[test]
    fn grep_tool_call_flattens_args_with_pattern() {
        let payload = json!({
            "grepToolCall": {
                "args": {
                    "pattern": "hello",
                    "path": "/tmp/test.txt",
                }
            }
        });
        let (name, input) = shape_for(payload);
        assert_eq!(name, "Grep");
        assert_eq!(input.get("pattern").and_then(|s| s.as_str()), Some("hello"));
        assert_eq!(input.get("path").and_then(|s| s.as_str()), Some("/tmp/test.txt"));
    }

    #[test]
    fn read_tool_call_renames_path_to_file_path() {
        let payload = json!({
            "readToolCall": {
                "args": {
                    "path": "/tmp/test.txt",
                }
            }
        });
        let (name, input) = shape_for(payload);
        assert_eq!(name, "Read");
        // Cursor uses `path`; formatToolUse expects `file_path`.
        assert_eq!(
            input.get("file_path").and_then(|s| s.as_str()),
            Some("/tmp/test.txt")
        );
        assert!(input.get("path").is_none(), "raw `path` key must be remapped");
    }

    #[test]
    fn glob_tool_call_flattens_args() {
        let payload = json!({
            "globToolCall": {
                "args": {
                    "pattern": "**/*.rs"
                }
            }
        });
        let (name, input) = shape_for(payload);
        assert_eq!(name, "Glob");
        assert_eq!(input.get("pattern").and_then(|s| s.as_str()), Some("**/*.rs"));
    }

    #[test]
    fn canonicalize_tool_name_maps_lowercase_aliases() {
        assert_eq!(canonicalize_tool_name("bash"), "Bash");
        assert_eq!(canonicalize_tool_name("shell"), "Bash");
        assert_eq!(canonicalize_tool_name("grep"), "Grep");
        assert_eq!(canonicalize_tool_name("read"), "Read");
        assert_eq!(canonicalize_tool_name("write"), "Write");
        // PascalCase already-canonical names pass through.
        assert_eq!(canonicalize_tool_name("Bash"), "Bash");
        // MCP names are never reshaped.
        assert_eq!(
            canonicalize_tool_name("mcp__app__open_github_pr"),
            "mcp__app__open_github_pr"
        );
        // Unknown tools pass through (caller renders them as-is).
        assert_eq!(canonicalize_tool_name("MysteryTool"), "MysteryTool");
    }
}
