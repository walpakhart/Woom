//! Localhost HTTP bridge for the terminal MCP tools.
//!
//! Why HTTP at all? The `forgehold-app` MCP sidecar lives in its own
//! process — spawned by Claude / Cursor, not by Forgehold-desktop —
//! so it can't reach the in-memory `TerminalRegistry` directly. The
//! cleanest IPC is a tiny axum server bound to `127.0.0.1:<random>`
//! that exposes the registry as a small REST surface. Sidecar reads
//! the port out of `<app_data>/bridge.port` at startup.
//!
//! Endpoints (all under `/v1/terminals`):
//!   GET  /                — list every live session id
//!   POST /:id/write       — body { data_b64 }, raw bytes to stdin
//!   POST /:id/run         — body { cmd, timeout_ms? }, run subprocess
//!   GET  /:id/buffer?lines=N
//!                         — last N lines of accumulated output
//!
//! ## Architecture
//!
//! The terminal column hosts TWO independent execution surfaces:
//!
//!   1. **User PTY** — `/bin/zsh` running on the slave side of a
//!      pseudo-terminal pair. The user types here, sees their normal
//!      shell with their normal env, aliases, prompt, etc. Untouched
//!      by anything the agent does.
//!
//!   2. **Agent subprocess executor** (`run` handler below) — when
//!      the agent calls `terminal_run`, we spawn a fresh `bash -c
//!      "<cmd>"` subprocess SEPARATELY from the user shell. cwd =
//!      session's spawn cwd. env = hardened (PAGER=cat, NO_COLOR=1,
//!      CI=1, etc.) so pagers can't trap, ANSI doesn't leak, CLIs
//!      use non-tty defaults. stdout/stderr piped, stdin = null.
//!
//! Output of agent subprocesses is INJECTED into the same xterm.js
//! display the user sees, framed by header/footer ANSI lines so
//! it's visually distinct from user-shell output. The injection
//! goes via the same `terminal:output:<id>` Tauri event the PTY
//! reader uses, plus the rolling buffer, so terminal_buffer reads
//! see it too.
//!
//! Why this beats the old "inject into PTY shell + sentinel" model:
//!   - No wrapper boilerplate echoed in the user's scrollback.
//!   - No stty/echo timing races.
//!   - Exit code from `Child::wait()` — no temp files.
//!   - Streaming output naturally via line-by-line read of pipes.
//!   - User shell stays at its prompt, undisturbed.
//!
//! Bound to 127.0.0.1 only — never listens on a public interface.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::io::Write as _;
use tauri::{AppHandle, Emitter, Manager};
use uuid::Uuid;

use crate::terminal::TerminalRegistry;

#[derive(Clone)]
struct BridgeState {
    app: AppHandle,
}

#[derive(Serialize)]
struct ListResp {
    instances: Vec<InstanceLite>,
}

#[derive(Serialize)]
struct InstanceLite {
    /// Human-readable column name from the workbench (`Vermeer`,
    /// `Notre-Dame`). This is what should be passed as `id` to all
    /// terminal_* tools. First field on purpose so the agent picks
    /// it as the canonical reference. Optional only because legacy
    /// spawns predating the column-naming feature might have null.
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    /// Per-spawn uuid. Useful only for disambiguation if two columns
    /// share a name (rare). Renamed from `id` so the agent doesn't
    /// reflexively grab the uuid as the canonical identifier.
    uuid: String,
}

#[derive(Deserialize)]
struct WriteReq {
    /// Base64-encoded raw bytes. Plain UTF-8 strings work too — the
    /// sidecar encodes for us.
    data_b64: String,
}

#[derive(Deserialize)]
struct RunReq {
    cmd: String,
    /// Idle timeout: how long we wait WITHOUT new output before
    /// declaring the command stuck. Default 60s. As long as the
    /// command keeps streaming bytes (build logs, test progress),
    /// the deadline rolls forward — a 10-minute test run with
    /// continuous output will NOT trip this. Only commands that
    /// genuinely hang (no stdout for 60s+) hit this timeout.
    timeout_ms: Option<u64>,
    /// Absolute upper bound on total run time. Default 30 minutes.
    /// A pathological command that loops forever printing junk
    /// (and thus never trips the idle timeout) still terminates
    /// at this cap.
    total_timeout_ms: Option<u64>,
}

#[derive(Serialize)]
struct RunResp {
    stdout: String,
    exit_code: i32,
    timed_out: bool,
    /// When `timed_out: true` and the trailing stdout looks like an
    /// interactive prompt waiting on user input — `Y/n`, `Press Enter`,
    /// `Password:`, `Authenticate ...?`, `Continue?`, `Username for ...:`
    /// — this field carries the detected prompt line so the agent can
    /// react with `terminal_write` directly without having to scan the
    /// raw stdout. None when no prompt was detected (genuine hang or
    /// silent command).
    #[serde(skip_serializing_if = "Option::is_none")]
    interactive_prompt: Option<String>,
}

#[derive(Deserialize)]
struct BufferQuery {
    lines: Option<usize>,
}

#[derive(Serialize)]
struct BufferResp {
    text: String,
    /// Total bytes the session has emitted since spawn (mod buffer
    /// rotation). Lets the agent detect "did anything new happen
    /// since I last looked?".
    total_bytes: u64,
}

/// Anyhow-flavoured error → JSON 400/404/500. Keeps handlers terse.
struct BridgeErr(StatusCode, String);
impl IntoResponse for BridgeErr {
    fn into_response(self) -> Response {
        (self.0, self.1).into_response()
    }
}
impl From<String> for BridgeErr {
    fn from(s: String) -> Self {
        BridgeErr(StatusCode::INTERNAL_SERVER_ERROR, s)
    }
}

async fn list(State(s): State<BridgeState>) -> Json<ListResp> {
    let reg = s.app.state::<TerminalRegistry>();
    let instances = reg
        .list()
        .into_iter()
        .map(|(id, name)| InstanceLite { name, uuid: id })
        .collect();
    Json(ListResp { instances })
}

async fn write(
    State(s): State<BridgeState>,
    Path(id): Path<String>,
    Json(req): Json<WriteReq>,
) -> Result<(), BridgeErr> {
    let bytes = STANDARD
        .decode(req.data_b64.as_bytes())
        .map_err(|e| BridgeErr(StatusCode::BAD_REQUEST, format!("base64: {e}")))?;
    let session = s
        .app
        .state::<TerminalRegistry>()
        .get_by_id_or_name(&id)
        .ok_or_else(|| BridgeErr(StatusCode::NOT_FOUND, format!("unknown terminal id-or-name: {id}")))?;
    let mut w = session.writer.lock();
    w.write_all(&bytes).map_err(|e| format!("write: {e}"))?;
    w.flush().map_err(|e| format!("flush: {e}"))?;
    Ok(())
}

async fn run(
    State(s): State<BridgeState>,
    Path(id): Path<String>,
    Json(req): Json<RunReq>,
) -> Result<Json<RunResp>, BridgeErr> {
    use std::process::Stdio;
    use tokio::io::AsyncReadExt;
    use tokio::process::Command;

    let session = s
        .app
        .state::<TerminalRegistry>()
        .get_by_id_or_name(&id)
        .ok_or_else(|| BridgeErr(StatusCode::NOT_FOUND, format!("unknown terminal id-or-name: {id}")))?;

    // Idle = no output for this long → kill. Default 60s.
    let idle_timeout = Duration::from_millis(req.timeout_ms.unwrap_or(60_000));
    // Hard ceiling regardless of output activity. Default 30 min.
    let total_timeout = Duration::from_millis(req.total_timeout_ms.unwrap_or(30 * 60 * 1000));
    let started_at = Instant::now();

    // ─── Header — visually frame the agent's command in xterm ───
    //
    // Cyan dim "$ <cmd>" line. The user sees this above the actual
    // command output, clearly distinguishing agent-initiated runs
    // from their own typing in the user shell.
    let cmd_for_display = req.cmd.replace('\n', " ");
    let header = format!("\r\n\x1b[36m\x1b[1m$ \x1b[22m{}\x1b[0m\r\n", cmd_for_display);
    inject_to_display(&s.app, &session, header.as_bytes());

    // ─── Spawn agent subprocess (NOT through user shell PTY) ───
    //
    // `bash -l -c "<cmd>"` runs the command in a fresh login shell.
    // Login shell so the user's PATH from `.zprofile` / `.bash_profile`
    // is loaded — without `-l` the subprocess gets only the bare env
    // we passed, missing tool installs in `/opt/homebrew/bin`,
    // `~/.local/bin`, etc. Login shell also reads `.bashrc`-equivalent
    // setups, so things like nvm-managed node, asdf, mise, etc. work.
    //
    // We use bash (not the user's shell) for predictability — agent
    // commands are bash syntax. Users can have fish or other shells
    // for their interactive use; agent commands won't break.
    let mut cmd = Command::new("/bin/bash");
    cmd.arg("-l").arg("-c").arg(&req.cmd);

    // Hardened env. These override whatever the login shell loads.
    // No more pager/color/CI surprises in agent output.
    cmd.env("PAGER", "cat");
    cmd.env("GIT_PAGER", "cat");
    cmd.env("GH_PAGER", "cat");
    cmd.env("SYSTEMD_PAGER", "cat");
    cmd.env("LESS", "-FRX");
    cmd.env("NO_COLOR", "1");
    cmd.env("FORCE_COLOR", "0");
    cmd.env("CLICOLOR", "0");
    cmd.env("CLICOLOR_FORCE", "0");
    cmd.env("CI", "1");
    cmd.env("DEBIAN_FRONTEND", "noninteractive");
    cmd.env("TERM", "xterm-256color");

    if let Some(cwd) = session.spawn_cwd.as_ref() {
        cmd.current_dir(cwd);
    }
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    cmd.stdin(Stdio::null()); // No interactive stdin yet — TODO

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            let err_line = format!("\r\n\x1b[31m✗ spawn failed: {}\x1b[0m\r\n", e);
            inject_to_display(&s.app, &session, err_line.as_bytes());
            return Err(BridgeErr(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("spawn bash: {e}"),
            ));
        }
    };

    let stdout = child.stdout.take().expect("stdout piped");
    let stderr = child.stderr.take().expect("stderr piped");

    // ─── Stream stdout/stderr → display + collected buffer ───
    //
    // Two reader tasks, one per pipe. Each chunk is:
    //   1. Forwarded to the xterm display (so user sees output live)
    //   2. Appended to a shared buffer (returned to the agent at the
    //      end as the `stdout` field of RunResp)
    //
    // We don't separate stdout vs stderr in the agent's response —
    // most agent-facing CLIs emit useful info on both streams and
    // the agent reasons better about the merged transcript anyway.
    use std::sync::Arc as StdArc;
    use parking_lot::Mutex as PlMutex;
    let collected: StdArc<PlMutex<Vec<u8>>> = StdArc::new(PlMutex::new(Vec::with_capacity(4096)));
    // Last-output-at: shared timestamp for idle-timeout detection.
    // Each reader task updates this when bytes arrive.
    let last_output: StdArc<PlMutex<Instant>> = StdArc::new(PlMutex::new(Instant::now()));

    // Spawn one drain task per pipe. Inline (not via closure) because
    // tokio::process::ChildStdout and ChildStderr are different types
    // and a single closure would monomorphize to the first one and
    // refuse the second. A generic fn would also work but inline reads
    // cleaner here.
    let stdout_task = {
        let app = s.app.clone();
        let session = session.clone();
        let collected = collected.clone();
        let last_output = last_output.clone();
        tokio::spawn(async move {
            let mut reader = tokio::io::BufReader::new(stdout);
            let mut buf = vec![0u8; 4096];
            loop {
                match reader.read(&mut buf).await {
                    Ok(0) => break,
                    Ok(n) => {
                        let chunk = &buf[..n];
                        inject_to_display(&app, &session, chunk);
                        collected.lock().extend_from_slice(chunk);
                        *last_output.lock() = Instant::now();
                    }
                    Err(_) => break,
                }
            }
        })
    };
    let stderr_task = {
        let app = s.app.clone();
        let session = session.clone();
        let collected = collected.clone();
        let last_output = last_output.clone();
        tokio::spawn(async move {
            let mut reader = tokio::io::BufReader::new(stderr);
            let mut buf = vec![0u8; 4096];
            loop {
                match reader.read(&mut buf).await {
                    Ok(0) => break,
                    Ok(n) => {
                        let chunk = &buf[..n];
                        inject_to_display(&app, &session, chunk);
                        collected.lock().extend_from_slice(chunk);
                        *last_output.lock() = Instant::now();
                    }
                    Err(_) => break,
                }
            }
        })
    };

    // ─── Wait loop with idle + total timeout ───
    //
    // We can't use `tokio::time::timeout(total, child.wait())` alone
    // because we ALSO want idle detection (no new bytes for `idle_timeout`).
    // Poll loop: every 100ms, check (a) child exit, (b) idle, (c) total.
    let exit_status = loop {
        let now = Instant::now();
        if now.saturating_duration_since(started_at) >= total_timeout {
            // Total cap hit. Kill child.
            let _ = child.kill().await;
            break None;
        }
        let last = *last_output.lock();
        if now.saturating_duration_since(last) >= idle_timeout {
            // No output for idle window. Kill child.
            let _ = child.kill().await;
            break None;
        }
        // Try to reap. `try_wait` returns Ok(Some(_)) only when child
        // has exited. Otherwise we sleep briefly and re-poll.
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            Err(_) => break None,
        }
    };

    // Drain pipes after process exit (the reader tasks finish on
    // EOF). Awaiting ensures any final bytes land in `collected`
    // before we read it.
    let _ = stdout_task.await;
    let _ = stderr_task.await;

    let elapsed_ms = started_at.elapsed().as_millis();
    let (exit_code, timed_out) = match exit_status {
        Some(s) => (s.code().unwrap_or(-1), false),
        None => (-1, true),
    };

    // ─── Footer — exit code + duration, color-coded ───
    let footer = if timed_out {
        format!("\r\n\x1b[31m✗ TIMED OUT\x1b[0m \x1b[2m({} ms)\x1b[0m\r\n", elapsed_ms)
    } else if exit_code == 0 {
        format!("\r\n\x1b[32m✓\x1b[0m \x1b[2mexit 0  ({} ms)\x1b[0m\r\n", elapsed_ms)
    } else {
        format!(
            "\r\n\x1b[33m✗ exit {}\x1b[0m \x1b[2m({} ms)\x1b[0m\r\n",
            exit_code, elapsed_ms
        )
    };
    inject_to_display(&s.app, &session, footer.as_bytes());

    // ─── Refresh the user shell's prompt under our footer ───
    //
    // The user's PTY shell didn't participate in the agent's run —
    // its perceived cursor is still at the top of the column, where
    // its last prompt was printed. Visually that prompt is now
    // scrolled up above all the agent output we just streamed, so
    // the user sees `✓ exit 0` with no fresh prompt below it and
    // assumes the terminal is "hung". They press Ctrl+C, the shell
    // re-prints its prompt at the current cursor, and only then
    // the next interaction works.
    //
    // Send a bare newline to the shell — it reads "" as an empty
    // command, executes nothing, and prints a fresh prompt at the
    // current cursor position (below our footer). The reader thread
    // captures that prompt and emits via terminal:output, so xterm
    // renders it where the user expects.
    {
        let mut w = session.writer.lock();
        let _ = w.write_all(b"\n");
        let _ = w.flush();
    }

    // Strip ANSI from collected output for the agent's response —
    // tools may STILL emit color despite NO_COLOR (some don't honor
    // the env var). The agent reasons better about plain text.
    let collected_bytes = collected.lock().clone();
    let stdout = clean_output(&collected_bytes);
    let interactive_prompt = if timed_out {
        detect_interactive_prompt(&stdout)
    } else {
        None
    };

    Ok(Json(RunResp {
        stdout,
        exit_code,
        timed_out,
        interactive_prompt,
    }))
}

/// Inject bytes into the terminal column's display + rolling buffer
/// AS IF the shell printed them. Bytes go three places:
///
///   1. The xterm.js view via `terminal:output:<id>` Tauri event —
///      user sees it immediately.
///   2. The session's rolling output buffer — `terminal_buffer` MCP
///      reads and `terminal_run`'s timed_out fallback reads.
///   3. `output_notify` — wakes any consumers polling for new bytes.
///
/// Used by the bridge's `run` handler to stream agent-subprocess
/// output into the user's terminal view without ever touching the
/// user's actual shell process. The shell stays at its prompt,
/// undisturbed; the user can keep typing while agent output
/// streams above their input.
fn inject_to_display(
    app: &AppHandle,
    session: &std::sync::Arc<crate::terminal::Session>,
    bytes: &[u8],
) {
    // Append to rolling buffer + bound size.
    {
        let mut buf = session.output_buf.lock();
        buf.extend_from_slice(bytes);
        const BUFFER_CAP: usize = 64 * 1024;
        if buf.len() > BUFFER_CAP {
            let excess = buf.len() - BUFFER_CAP;
            buf.drain(..excess);
        }
    }
    // Emit to xterm.js for live display.
    let payload = STANDARD.encode(bytes);
    let _ = app.emit(&format!("terminal:output:{}", session.id), payload);
    // Wake terminal_buffer waiters, if any.
    session.output_notify.notify_one();
}

/// Build a "timed out" response from whatever bytes have accumulated
/// in the session buffer since `scan_start`. Used by the legacy
/// PTY-injection path (no longer hit on the happy path now that run
/// uses subprocess directly, but kept as a defensive fallback).
#[allow(dead_code)]
fn timed_out_resp(
    session: &crate::terminal::Session,
    scan_start: usize,
) -> RunResp {
    let buf = session.output_buf.lock();
    let new_bytes = if scan_start < buf.len() {
        buf[scan_start..].to_vec()
    } else {
        Vec::new()
    };
    drop(buf);
    let stdout = clean_output(&new_bytes);
    let interactive_prompt = detect_interactive_prompt(&stdout);
    RunResp {
        stdout,
        exit_code: -1,
        timed_out: true,
        interactive_prompt,
    }
}

/// Heuristic prompt detection. When `terminal_run` times out, the
/// usual cause is the command is parked on an interactive prompt
/// waiting for input (gh auth login, ssh, sudo, npm init, db migrate
/// confirms, git rebase --interactive, etc.). The agent's pre-baked
/// path is to either give up or hallucinate "sandbox blocked" — both
/// useless. Surfacing the detected prompt directly tells the agent
/// "the command is alive, waiting for input X — use terminal_write".
///
/// Strategy: scan the LAST few non-empty lines of stdout for known
/// prompt patterns. Last lines because the prompt is the most recent
/// output before the read blocked. We err toward false positives —
/// if we surface a prompt and there isn't one, the agent will write
/// `\n` or whatever and likely move on; the cost is one wasted
/// terminal_write. Failing to detect a real prompt is the worse
/// failure (agent hallucinates instead).
fn detect_interactive_prompt(stdout: &str) -> Option<String> {
    let trimmed = stdout.trim_end();
    if trimmed.is_empty() {
        return None;
    }
    // Scan the last 5 non-empty lines.
    let lines: Vec<&str> = trimmed
        .lines()
        .rev()
        .filter(|l| !l.trim().is_empty())
        .take(5)
        .collect();
    for line in lines {
        let lower = line.to_ascii_lowercase();
        // Yes/no prompts.
        let yn_patterns = [
            "[y/n]", "(y/n)", "[yes/no]", "(yes/no)", "[y/n/?]",
            "y/n?", "yes/no?", "(yes)", "(y)", " y/n", "[y/n]:",
        ];
        if yn_patterns.iter().any(|p| lower.contains(p)) {
            return Some(line.trim().to_string());
        }
        // Question-mark prompts (broad — anything ending in `?` after
        // a non-trivial line that has prompt-flavored words).
        if line.trim_end().ends_with('?')
            && (lower.contains("authenticate")
                || lower.contains("continue")
                || lower.contains("proceed")
                || lower.contains("confirm")
                || lower.contains("are you sure")
                || lower.contains("overwrite")
                || lower.contains("delete")
                || lower.contains("install")
                || lower.contains("update")
                || lower.contains("ok to"))
        {
            return Some(line.trim().to_string());
        }
        // Colon-terminated prompts (Password:, Username for X:, etc).
        if line.trim_end().ends_with(':')
            && (lower.contains("password")
                || lower.contains("passphrase")
                || lower.contains("username")
                || lower.contains("login")
                || lower.contains("token")
                || lower.contains("email"))
        {
            return Some(line.trim().to_string());
        }
        // "Press Enter / any key" prompts.
        if lower.contains("press enter")
            || lower.contains("press any key")
            || lower.contains("press return")
            || lower.contains("hit enter")
        {
            return Some(line.trim().to_string());
        }
        // Selection lists (gh auth, vite menus, etc).
        if lower.starts_with("? ") || lower.contains("? select") || lower.contains("? choose") {
            return Some(line.trim().to_string());
        }
    }
    None
}

/// Strip ANSI escape sequences and decode as utf-8 (lossy — terminal
/// output isn't strictly valid utf-8 if there's binary in the middle).
/// Pre-refactor this also stripped a `__FGH_DONE_*__` sentinel marker;
/// that's gone now — completion is detected via temp-file, so the
/// PTY buffer holds only the user's command output.
fn clean_output(bytes: &[u8]) -> String {
    let stripped = strip_ansi_escapes::strip(bytes);
    String::from_utf8_lossy(&stripped).into_owned()
}

async fn buffer(
    State(s): State<BridgeState>,
    Path(id): Path<String>,
    Query(q): Query<BufferQuery>,
) -> Result<Json<BufferResp>, BridgeErr> {
    let session = s
        .app
        .state::<TerminalRegistry>()
        .get_by_id_or_name(&id)
        .ok_or_else(|| BridgeErr(StatusCode::NOT_FOUND, format!("unknown terminal id-or-name: {id}")))?;
    let buf = session.output_buf.lock();
    let total = buf.len() as u64;
    let stripped = strip_ansi_escapes::strip(&*buf);
    drop(buf);
    let text = String::from_utf8_lossy(&stripped).into_owned();
    let lines = q.lines.unwrap_or(200);
    let trimmed: String = if lines == 0 {
        text
    } else {
        let v: Vec<&str> = text.lines().collect();
        let start = v.len().saturating_sub(lines);
        v[start..].join("\n")
    };
    Ok(Json(BufferResp {
        text: trimmed,
        total_bytes: total,
    }))
}

/// Path of the port-discovery file. Sidecars read this on launch to
/// learn where to POST. We use Tauri's resolved app-data directory
/// so it's the same path the app uses for everything else.
pub fn port_file_path(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_data_dir().ok().map(|d| d.join("bridge.port"))
}

/// Spin up the bridge on a random localhost port. The port is
/// written to `bridge.port` under the app's data dir so spawned
/// sidecars can find it. Returns the port for log surfacing.
///
/// Errors are non-fatal — the desktop app keeps running, just
/// without MCP terminal access. Without this bridge, the agent
/// loses `terminal.run_command` etc., but the user's terminal
/// column still works (it talks to Tauri commands directly).
pub async fn start(app: AppHandle) -> Result<u16, String> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| format!("bind: {e}"))?;
    let port = listener
        .local_addr()
        .map_err(|e| format!("local_addr: {e}"))?
        .port();

    if let Some(path) = port_file_path(&app) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&path, format!("{port}\n"));
    }

    let state = BridgeState { app: app.clone() };
    let router = Router::new()
        .route("/v1/terminals", get(list))
        .route("/v1/terminals/{id}/write", post(write))
        .route("/v1/terminals/{id}/run", post(run))
        .route("/v1/terminals/{id}/buffer", get(buffer))
        .with_state(state);

    let app_for_handle = app.clone();
    tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, router).await {
            // Surface to the frontend log so we know if the bridge
            // dies mid-session — agent calls would start failing.
            let _ = app_for_handle.emit("terminal:bridge_error", format!("axum: {e}"));
        }
    });

    Ok(port)
}

/// Cleanup helper — removes the port file when the app shuts down so
/// a stale port doesn't persist across launches.
pub fn clear_port_file(app: &AppHandle) {
    if let Some(path) = port_file_path(app) {
        let _ = std::fs::remove_file(path);
    }
}
