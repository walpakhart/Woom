//! User-defined hooks — small scripts the user wires into agent lifecycle
//! events. Inspired by Claude Code's `settings.json > hooks` surface (see
//! `docs/CLAUDE_PARITY.md §2`). Each hook is a shell command that receives
//! the event payload as JSON on stdin and replies with either:
//!   - exit 0 + JSON on stdout = success, optional structured control
//!   - exit 2 + stderr text     = block, feedback fed back to caller
//!   - any other code           = non-blocking warning shown to the user
//!
//! Phase 1 events implemented:
//!   - SessionStart      (session created or resumed)
//!   - UserPromptSubmit  (before user msg is sent to the CLI)
//!   - Stop              (after assistant turn ends)
//!
//! Pre/PostToolUse are NOT in Phase 1 because intercepting CLI-internal
//! tool calls requires us to own tool dispatch (today the CLI runs Bash/
//! Edit/Write itself). Tracked in `CLAUDE_PARITY.md §2.1.3`.
//!
//! Hook config lives at `<app_data>/hooks.json`. Schema:
//!
//! ```jsonc
//! {
//!   "SessionStart": [
//!     {
//!       "matcher": "*",                  // optional regex on event-specific match field
//!       "handler": { "type": "command", "command": "/path/to/script", "args": ["--foo"] },
//!       "timeout_ms": 5000,               // default 15_000
//!       "disabled": false                 // default false
//!     }
//!   ],
//!   "UserPromptSubmit": [...],
//!   "Stop": [...]
//! }
//! ```
//!
//! Failure modes: missing file → empty config. Malformed JSON → empty
//! config + console warning (we don't crash the app on bad user config).
//! Hook script not found / non-executable → that hook fails, others
//! still run. One handler exception never stops the agent — at worst
//! the agent gets a "hook X warned: …" injected into the next message
//! via `additional_context` (future enhancement).

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

// ---- Event names ---------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HookEvent {
    SessionStart,
    UserPromptSubmit,
    Stop,
}

impl HookEvent {
    fn as_key(self) -> &'static str {
        match self {
            HookEvent::SessionStart => "SessionStart",
            HookEvent::UserPromptSubmit => "UserPromptSubmit",
            HookEvent::Stop => "Stop",
        }
    }
}

// ---- Config shape --------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct HookConfig {
    /// Map event name → list of hook entries. Unknown event names are
    /// preserved on round-trip (so users can hand-edit the file ahead
    /// of new event support) but ignored by `run_for_event`.
    pub hooks: HashMap<String, Vec<HookEntry>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookEntry {
    /// Glob/regex against an event-specific match field. Currently we
    /// only support `"*"` (match all) and plain substring match — full
    /// regex deferred until we have a real use case.
    #[serde(default = "default_matcher")]
    pub matcher: String,
    pub handler: HookHandler,
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
    #[serde(default)]
    pub disabled: bool,
}

fn default_matcher() -> String { "*".into() }
fn default_timeout_ms() -> u64 { 15_000 }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HookHandler {
    /// External shell command. `command` is the executable; `args` are
    /// passed verbatim (no shell expansion — we use `tokio::process::
    /// Command` directly). Env inherits from the parent; per-hook env
    /// overrides can be layered in a future field.
    Command {
        command: String,
        #[serde(default)]
        args: Vec<String>,
    },
}

// ---- Outcome shape -------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookOutcome {
    /// Aggregate result across all hooks that ran for this event.
    /// Blocking = any hook exited with code 2; the caller should NOT
    /// proceed. `feedback` collects the stderr / reason texts.
    pub blocked: bool,
    pub feedback: Vec<String>,
    /// PrePromptSubmit hooks can rewrite the prompt by returning JSON
    /// `{ "updatedPrompt": "..." }` on stdout. The first non-null
    /// rewrite wins; subsequent hooks see the rewritten version.
    pub updated_prompt: Option<String>,
    /// Any hook can attach `{ "additionalContext": "…" }` to its
    /// stdout JSON; we concatenate all of them into a single field
    /// the caller appends to the next system prompt section.
    pub additional_context: Option<String>,
    /// Per-hook diagnostic — useful for the settings "test hooks"
    /// button. Indexed by handler-command path.
    pub per_hook: Vec<PerHookResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerHookResult {
    pub command: String,
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
    pub stdout: String,
    pub stderr: String,
    pub error: Option<String>,
}

/// What a hook script may emit on stdout (parsed as JSON; any other
/// stdout content is treated as plain text and surfaced as feedback).
#[derive(Debug, Default, Deserialize)]
struct HookStdoutJson {
    #[serde(default)]
    updated_prompt: Option<String>,
    #[serde(default)]
    additional_context: Option<String>,
    #[serde(default)]
    reason: Option<String>,
}

// ---- State ---------------------------------------------------------------

/// Loaded config + a path so save can write it back. Held in Tauri
/// state under `Mutex` because edits go through Tauri commands (one
/// writer at a time is fine; reads are cheap).
pub struct HookState {
    pub config_path: PathBuf,
    pub config: parking_lot::Mutex<HookConfig>,
}

impl HookState {
    pub fn new(app: &AppHandle) -> Self {
        let path = config_path(app);
        let cfg = read_or_default(&path);
        Self {
            config_path: path,
            config: parking_lot::Mutex::new(cfg),
        }
    }
}

fn config_path(app: &AppHandle) -> PathBuf {
    let dir = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::env::temp_dir());
    let _ = std::fs::create_dir_all(&dir);
    dir.join("hooks.json")
}

fn read_or_default(path: &std::path::Path) -> HookConfig {
    let raw = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return HookConfig::default(),
        Err(e) => {
            eprintln!("hooks: read {}: {e}", path.display());
            return HookConfig::default();
        }
    };
    match serde_json::from_str::<HookConfig>(&raw) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("hooks: malformed config — {e}");
            HookConfig::default()
        }
    }
}

// ---- Execution -----------------------------------------------------------

/// Public entry — runs every hook configured for `event` against the
/// supplied JSON payload. `match_field` is an optional event-specific
/// string to check against each entry's `matcher` (e.g. tool name for
/// PreToolUse later — for SessionStart / UserPromptSubmit / Stop we
/// pass `None` and the matcher just needs to be `"*"` or empty).
pub async fn run_for_event(
    state: &HookState,
    event: HookEvent,
    payload: serde_json::Value,
    match_field: Option<&str>,
) -> HookOutcome {
    let entries = {
        let cfg = state.config.lock();
        cfg.hooks
            .get(event.as_key())
            .cloned()
            .unwrap_or_default()
    };

    let mut outcome = HookOutcome {
        blocked: false,
        feedback: Vec::new(),
        updated_prompt: None,
        additional_context: None,
        per_hook: Vec::new(),
    };

    let mut current_payload = payload;
    for entry in entries {
        if entry.disabled {
            continue;
        }
        if !matches_entry(&entry.matcher, match_field) {
            continue;
        }

        // If a prior hook rewrote the prompt, the current payload should
        // reflect that so chained hooks see the updated version.
        if let Some(updated) = &outcome.updated_prompt {
            if let Some(obj) = current_payload.as_object_mut() {
                obj.insert("prompt".into(), serde_json::Value::String(updated.clone()));
            }
        }

        let per = run_single(&entry, &current_payload).await;
        let exit = per.exit_code;
        let parsed: HookStdoutJson = serde_json::from_str(&per.stdout).unwrap_or_default();

        if exit == Some(2) {
            outcome.blocked = true;
            let msg = if !per.stderr.trim().is_empty() {
                per.stderr.clone()
            } else {
                format!("hook `{}` blocked", per.command)
            };
            outcome.feedback.push(msg);
        } else if let Some(c) = exit {
            if c != 0 {
                // Non-blocking warning.
                outcome.feedback.push(format!(
                    "hook `{}` exited {c}: {}",
                    per.command,
                    per.stderr.trim()
                ));
            }
        }

        if let Some(p) = parsed.updated_prompt {
            outcome.updated_prompt = Some(p);
        }
        if let Some(ctx) = parsed.additional_context {
            let existing = outcome.additional_context.unwrap_or_default();
            let joined = if existing.is_empty() {
                ctx
            } else {
                format!("{existing}\n\n{ctx}")
            };
            outcome.additional_context = Some(joined);
        }
        if let Some(reason) = parsed.reason {
            outcome.feedback.push(reason);
        }

        outcome.per_hook.push(per);

        if outcome.blocked {
            break;
        }
    }

    outcome
}

fn matches_entry(matcher: &str, match_field: Option<&str>) -> bool {
    let m = matcher.trim();
    if m.is_empty() || m == "*" {
        return true;
    }
    match match_field {
        Some(s) => s.to_ascii_lowercase().contains(&m.to_ascii_lowercase()),
        None => false,
    }
}

async fn run_single(entry: &HookEntry, payload: &serde_json::Value) -> PerHookResult {
    let started = std::time::Instant::now();
    let HookHandler::Command { command, args } = &entry.handler;
    let mut cmd = Command::new(command);
    cmd.args(args);
    cmd.stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return PerHookResult {
                command: command.clone(),
                exit_code: None,
                duration_ms: started.elapsed().as_millis() as u64,
                stdout: String::new(),
                stderr: String::new(),
                error: Some(format!("spawn {command}: {e}")),
            };
        }
    };

    // Pipe payload into stdin then close.
    if let Some(mut stdin) = child.stdin.take() {
        let body = serde_json::to_vec(payload).unwrap_or_default();
        let _ = stdin.write_all(&body).await;
        // Drop closes the pipe so the child's read of stdin sees EOF.
        drop(stdin);
    }

    let timeout = Duration::from_millis(entry.timeout_ms.max(100));
    let output = match tokio::time::timeout(timeout, child.wait_with_output()).await {
        Ok(Ok(out)) => out,
        Ok(Err(e)) => {
            return PerHookResult {
                command: command.clone(),
                exit_code: None,
                duration_ms: started.elapsed().as_millis() as u64,
                stdout: String::new(),
                stderr: String::new(),
                error: Some(format!("wait: {e}")),
            };
        }
        Err(_) => {
            // Timeout — child still running. Best-effort kill.
            return PerHookResult {
                command: command.clone(),
                exit_code: None,
                duration_ms: started.elapsed().as_millis() as u64,
                stdout: String::new(),
                stderr: String::new(),
                error: Some(format!("hook timed out after {timeout:?}")),
            };
        }
    };

    PerHookResult {
        command: command.clone(),
        exit_code: output.status.code(),
        duration_ms: started.elapsed().as_millis() as u64,
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        error: None,
    }
}

// ---- Tauri commands ------------------------------------------------------

#[tauri::command]
pub fn hooks_load_config(state: State<'_, HookState>) -> HookConfig {
    state.config.lock().clone()
}

#[tauri::command]
pub fn hooks_save_config(
    state: State<'_, HookState>,
    config: HookConfig,
) -> Result<(), String> {
    let raw = serde_json::to_string_pretty(&config).map_err(|e| format!("serialize: {e}"))?;
    std::fs::write(&state.config_path, raw)
        .map_err(|e| format!("write {}: {e}", state.config_path.display()))?;
    *state.config.lock() = config;
    Ok(())
}

#[tauri::command]
pub async fn hooks_run(
    state: State<'_, HookState>,
    event: String,
    payload: serde_json::Value,
    match_field: Option<String>,
) -> Result<HookOutcome, String> {
    let evt = match event.as_str() {
        "SessionStart" => HookEvent::SessionStart,
        "UserPromptSubmit" => HookEvent::UserPromptSubmit,
        "Stop" => HookEvent::Stop,
        other => return Err(format!("unknown event: {other}")),
    };
    Ok(run_for_event(&state, evt, payload, match_field.as_deref()).await)
}
