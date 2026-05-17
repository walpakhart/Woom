//! Statusline-as-script — small Claude-Code-parity feature. The user
//! drops a shell command into `<app_data>/statusline.json`; on every
//! turn end and on a configurable interval we pipe the current
//! session state JSON to stdin of that command, read stdout, and
//! render it as a thin strip below the composer.
//!
//! Failure modes are non-fatal: a missing config = no statusline; a
//! command that exits non-zero = statusline shows stderr in a muted
//! tone so the user knows their script is broken; a timeout = stale
//! output stays visible while the next call retries.
//!
//! The JSON contract mirrors Claude Code's `statusLine` payload subset
//! that's actually wired here today (more fields can be added without
//! breaking older user scripts — they ignore unknown fields).

use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct StatusLineConfig {
    /// Shell command to run. None = no statusline configured.
    pub command: Option<String>,
    /// Optional args. Receives state JSON on stdin in addition.
    pub args: Vec<String>,
    /// Periodic re-run interval, seconds. None or 0 = no timer; run
    /// only on turn end. Frontend clamps to [5, 300].
    pub refresh_interval: Option<u64>,
    /// Per-call timeout, milliseconds. Default 5_000.
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
    /// Optional max bytes of stdout to display. Default 4_000 — keeps
    /// a runaway script from blowing the UI. Stdout above this gets
    /// truncated with `[…truncated]`.
    #[serde(default = "default_max_output")]
    pub max_output_bytes: usize,
}

fn default_timeout_ms() -> u64 { 5_000 }
fn default_max_output() -> usize { 4_000 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusLineResult {
    pub ok: bool,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
}

pub struct StatusLineState {
    pub config_path: PathBuf,
    pub config: parking_lot::Mutex<StatusLineConfig>,
}

impl StatusLineState {
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
    dir.join("statusline.json")
}

fn read_or_default(path: &std::path::Path) -> StatusLineConfig {
    let raw = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(_) => return StatusLineConfig::default(),
    };
    serde_json::from_str::<StatusLineConfig>(&raw).unwrap_or_default()
}

// ---- Execution -----------------------------------------------------------

pub async fn run(
    state: &StatusLineState,
    payload: serde_json::Value,
) -> StatusLineResult {
    let cfg = state.config.lock().clone();
    let Some(command) = cfg.command.clone() else {
        return StatusLineResult {
            ok: false,
            stdout: String::new(),
            stderr: "no statusline configured".into(),
            exit_code: None,
            duration_ms: 0,
        };
    };

    let started = std::time::Instant::now();
    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg(&command);
    for a in &cfg.args {
        cmd.arg(a);
    }
    cmd.stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return StatusLineResult {
                ok: false,
                stdout: String::new(),
                stderr: format!("spawn: {e}"),
                exit_code: None,
                duration_ms: started.elapsed().as_millis() as u64,
            };
        }
    };

    if let Some(mut stdin) = child.stdin.take() {
        let body = serde_json::to_vec(&payload).unwrap_or_default();
        let _ = stdin.write_all(&body).await;
        drop(stdin);
    }

    let timeout = Duration::from_millis(cfg.timeout_ms.max(100));
    let output = match tokio::time::timeout(timeout, child.wait_with_output()).await {
        Ok(Ok(out)) => out,
        Ok(Err(e)) => {
            return StatusLineResult {
                ok: false,
                stdout: String::new(),
                stderr: format!("wait: {e}"),
                exit_code: None,
                duration_ms: started.elapsed().as_millis() as u64,
            };
        }
        Err(_) => {
            return StatusLineResult {
                ok: false,
                stdout: String::new(),
                stderr: format!("timed out after {timeout:?}"),
                exit_code: None,
                duration_ms: started.elapsed().as_millis() as u64,
            };
        }
    };

    let mut stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    if stdout.len() > cfg.max_output_bytes {
        stdout.truncate(cfg.max_output_bytes);
        stdout.push_str("\n[…truncated]");
    }
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    StatusLineResult {
        ok: output.status.success(),
        stdout,
        stderr,
        exit_code: output.status.code(),
        duration_ms: started.elapsed().as_millis() as u64,
    }
}

// ---- Tauri commands ------------------------------------------------------

#[tauri::command]
pub fn statusline_load_config(state: State<'_, StatusLineState>) -> StatusLineConfig {
    state.config.lock().clone()
}

#[tauri::command]
pub fn statusline_save_config(
    state: State<'_, StatusLineState>,
    config: StatusLineConfig,
) -> Result<(), String> {
    let raw = serde_json::to_string_pretty(&config).map_err(|e| format!("serialize: {e}"))?;
    std::fs::write(&state.config_path, raw)
        .map_err(|e| format!("write: {e}"))?;
    *state.config.lock() = config;
    Ok(())
}

#[tauri::command]
pub async fn statusline_run(
    state: State<'_, StatusLineState>,
    payload: serde_json::Value,
) -> Result<StatusLineResult, String> {
    Ok(run(&state, payload).await)
}
