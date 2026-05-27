//! Background-task registry — long-running processes the agent (or user)
//! spawns and wants to watch over time. The companion of `terminal_bridge`
//! / `terminal.rs`: terminal is short-lived `bash -c` per call; this module
//! keeps a process ALIVE, tails its output, scans for `http://localhost:PORT`
//! URLs, and lets callers `wait_line` on the next stdout line (the
//! line-streaming primitive the agent uses to react to a build/test/dev
//! server without polling).
//!
//! Lifecycle: `spawn` returns a stable id; the child runs detached from
//! any UI. Stdout+stderr are piped to a rolling file under
//! `~/Library/Application Support/Woom/bg-tasks/<id>.log` (~10 MB cap,
//! rotated to `.1`). Every line is also broadcast to in-process
//! subscribers (`bg:line:<id>` Tauri event + a tokio broadcast channel
//! for the synchronous `wait_line` Tauri command).
//!
//! Process tree death: child handle stored on the registry; `Drop` and
//! `kill` send SIGTERM then SIGKILL after 3s. App restart marks all
//! stored task metadata as `Killed { reason: "app-restart" }` on init —
//! we don't try to reap orphans across crashes (would need a separate
//! supervisor process; out of scope for phase 1).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{broadcast, mpsc, Mutex as AsyncMutex};

/// One line of output. Sent via tokio broadcast for `wait_line`
/// long-poll consumers AND emitted as a `bg:line:<id>` Tauri event for
/// the frontend's reactive log tail.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BgLine {
    pub id: String,
    pub at: u64,
    pub stream: BgStream,
    pub line: String,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum BgStream {
    Stdout,
    Stderr,
}

/// Public-facing task metadata. Mirrors what the frontend store keeps
/// per task. Subset of the internal `TaskState` (no live handles).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BgTask {
    pub id: String,
    pub label: String,
    pub cmd: String,
    pub cwd: String,
    pub session_id: Option<String>,
    pub pid: Option<u32>,
    pub started_at: u64,
    pub status: BgStatus,
    pub log_path: String,
    /// URLs scraped from output, in order of first appearance. First match
    /// becomes the "primary" URL the preview surface points at.
    pub detected_urls: Vec<String>,
    /// Ports parsed from those URLs. Same order. De-duplicated.
    pub detected_ports: Vec<u16>,
    /// Most recent ~30 lines of stdout/stderr — used by the frontend
    /// task list to render a tiny inline preview without hitting
    /// `bg_logs` for every task. Trimmed on the fly.
    #[serde(default)]
    pub recent_lines: Vec<BgLine>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BgStatus {
    Running,
    /// Child exited on its own. `code` is the OS exit code (None if
    /// signalled — Unix only reports signal via wait_status; we collapse
    /// to a sentinel `-1` to keep the JSON shape simple).
    Exited { code: i32 },
    /// User / programmatic kill via `bg_kill`.
    Killed { reason: String },
}

/// Tunables — easier to find here than scattered.
const LOG_CAP_BYTES: u64 = 10 * 1024 * 1024;
const RECENT_LINES_CAP: usize = 30;
const KILL_GRACE_MS: u64 = 3000;
const BROADCAST_CAPACITY: usize = 256;
/// Coalescer cadence — at most one `bg:lines:<id>` IPC event per this
/// many ms, regardless of how many lines streamed in. Tuned for a smooth
/// 60Hz UI without saturating Tauri's main thread on bursty output.
const COALESCE_WINDOW_MS: u64 = 80;
/// Hard burst cap — flush early if we accumulate this many lines before
/// the cadence elapses. Prevents 10K-line dumps from blowing JSON encode.
const COALESCE_BURST_CAP: usize = 100;

/// Live state for one tracked process. The registry holds these in
/// memory; the public `BgTask` is a serialisable snapshot.
struct TaskState {
    task: BgTask,
    child: Option<Arc<AsyncMutex<Child>>>,
    /// Sender side of the per-line broadcast — subscribers consume via
    /// `broadcast::Receiver` for `wait_line`. Dropped when the task is
    /// torn down so receivers see `RecvError::Closed`.
    line_tx: broadcast::Sender<BgLine>,
    /// Sender side for piping bytes into the child's stdin (interactive
    /// dev servers). `None` if stdin wasn't requested at spawn (we still
    /// want it usually — many dev servers read keyboard input).
    stdin_tx: Option<mpsc::Sender<Vec<u8>>>,
}

pub struct BgRegistry {
    tasks: RwLock<HashMap<String, Arc<RwLock<TaskState>>>>,
    /// Where rolling log files live. Created at first spawn.
    log_dir: RwLock<Option<PathBuf>>,
}

impl BgRegistry {
    pub fn new() -> Self {
        Self {
            tasks: RwLock::new(HashMap::new()),
            log_dir: RwLock::new(None),
        }
    }

    fn ensure_log_dir(&self, app: &AppHandle) -> Result<PathBuf, String> {
        if let Some(p) = self.log_dir.read().as_ref() {
            return Ok(p.clone());
        }
        let base = app
            .path()
            .app_data_dir()
            .map_err(|e| format!("app_data_dir: {e}"))?
            .join("bg-tasks");
        std::fs::create_dir_all(&base).map_err(|e| format!("mkdir {}: {e}", base.display()))?;
        *self.log_dir.write() = Some(base.clone());
        Ok(base)
    }

    pub fn list(&self) -> Vec<BgTask> {
        let mut out: Vec<BgTask> = self
            .tasks
            .read()
            .values()
            .map(|s| s.read().task.clone())
            .collect();
        // Newest first — matches the task list visual order.
        out.sort_by(|a, b| b.started_at.cmp(&a.started_at));
        out
    }

    pub fn get(&self, id: &str) -> Option<BgTask> {
        self.tasks.read().get(id).map(|s| s.read().task.clone())
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn short_id() -> String {
    use uuid::Uuid;
    let s = Uuid::new_v4().simple().to_string();
    format!("bg-{}", &s[..10])
}

/// Manual URL scanner — cheap regex-free scan for `http(s)://(localhost|
/// 127.0.0.1|0.0.0.0)[:port][/path]`. Returns (url, port?). We avoid
/// pulling in `regex` for this one site.
fn scan_url(line: &str) -> Option<(String, Option<u16>)> {
    let lower = line.to_ascii_lowercase();
    let mut start = 0usize;
    while let Some(i) = lower[start..].find("http") {
        let p = start + i;
        let rest = &line[p..];
        let scheme_len = if rest.to_ascii_lowercase().starts_with("https://") {
            8
        } else if rest.to_ascii_lowercase().starts_with("http://") {
            7
        } else {
            start = p + 4;
            continue;
        };
        let after_scheme = &rest[scheme_len..];
        let host_end = after_scheme
            .find(|c: char| {
                c.is_whitespace() || c == ',' || c == ';' || c == ')' || c == ']' || c == '"'
            })
            .unwrap_or(after_scheme.len());
        let host_and_path = &after_scheme[..host_end];
        // Host portion stops at `/` or `?`.
        let stop = host_and_path
            .find(|c: char| c == '/' || c == '?' || c == '#')
            .unwrap_or(host_and_path.len());
        let host_with_port = &host_and_path[..stop];
        let host_lower = host_with_port.to_ascii_lowercase();
        let (host_only, port) = if let Some(c) = host_with_port.find(':') {
            let host = &host_with_port[..c];
            let port_str = &host_with_port[c + 1..];
            (host.to_ascii_lowercase(), port_str.parse::<u16>().ok())
        } else {
            (host_lower, None)
        };
        if matches!(host_only.as_str(), "localhost" | "127.0.0.1" | "0.0.0.0") {
            let url = format!("{}{}", &rest[..scheme_len], host_and_path);
            return Some((url, port));
        }
        start = p + scheme_len;
    }
    None
}

/// Drain a per-task line channel and emit batched `bg:lines:<id>` Tauri
/// events. ONE event per ≤COALESCE_WINDOW_MS (80ms) or per
/// COALESCE_BURST_CAP (100) lines — whichever fires first. Replaces the
/// previous per-line emit pattern that froze the app on Vite/Next dev
/// startup (~hundreds of lines/sec → IPC saturation + Svelte
/// reactivity storm).
async fn run_coalescer(
    app: AppHandle,
    id: String,
    mut rx: mpsc::UnboundedReceiver<BgLine>,
) {
    let event_name = format!("bg:lines:{id}");
    let mut buf: Vec<BgLine> = Vec::with_capacity(COALESCE_BURST_CAP);
    let window = Duration::from_millis(COALESCE_WINDOW_MS);
    loop {
        /* Wait for ANY line — if the channel closes (both readers
         *  EOF'd, original sender dropped) we exit cleanly. */
        let first = match rx.recv().await {
            Some(line) => line,
            None => break,
        };
        buf.push(first);
        /* Accumulate more lines for up to `window` ms OR until burst
         *  cap, whichever first. `try_recv` is a non-blocking poll,
         *  `recv` with timeout gives us "either a new line within X ms
         *  or flush". */
        let deadline = tokio::time::Instant::now() + window;
        while buf.len() < COALESCE_BURST_CAP {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                break;
            }
            match tokio::time::timeout(remaining, rx.recv()).await {
                Ok(Some(line)) => buf.push(line),
                Ok(None) => {
                    // Channel closed mid-window — flush remaining lines and exit.
                    if !buf.is_empty() {
                        let _ = app.emit(&event_name, &buf);
                    }
                    return;
                }
                Err(_) => break, // timeout — flush
            }
        }
        let _ = app.emit(&event_name, &buf);
        buf.clear();
    }
}

/// Append a line to the task's recent_lines + url/port detections.
/// Called inline from the reader loop.
fn ingest_line(state: &Arc<RwLock<TaskState>>, line: BgLine) {
    let mut s = state.write();
    // URL/port detection — only when not already known.
    if let Some((url, port)) = scan_url(&line.line) {
        if !s.task.detected_urls.iter().any(|u| u == &url) {
            s.task.detected_urls.push(url);
        }
        if let Some(p) = port {
            if !s.task.detected_ports.contains(&p) {
                s.task.detected_ports.push(p);
            }
        }
    }
    s.task.recent_lines.push(line.clone());
    if s.task.recent_lines.len() > RECENT_LINES_CAP {
        let drop = s.task.recent_lines.len() - RECENT_LINES_CAP;
        s.task.recent_lines.drain(0..drop);
    }
    // Broadcast — best-effort. Failure = no live subscribers, that's fine.
    let _ = s.line_tx.send(line);
}

fn rotate_if_oversized(log_path: &std::path::Path) -> std::io::Result<()> {
    let md = match std::fs::metadata(log_path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    if md.len() < LOG_CAP_BYTES {
        return Ok(());
    }
    let rotated = log_path.with_extension(format!(
        "{}.1",
        log_path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("log")
    ));
    let _ = std::fs::remove_file(&rotated);
    std::fs::rename(log_path, &rotated)?;
    Ok(())
}

#[derive(Debug, Deserialize)]
pub struct SpawnArgs {
    pub cmd: String,
    pub cwd: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    /// Optional env overrides to layer on top of the parent's env.
    #[serde(default)]
    pub env: Option<HashMap<String, String>>,
}

/// Public spawn entry point. Returns the freshly-tracked task. The
/// child is detached — caller doesn't need to poll for the result.
pub async fn spawn(
    app: AppHandle,
    registry: &BgRegistry,
    args: SpawnArgs,
) -> Result<BgTask, String> {
    let log_dir = registry.ensure_log_dir(&app)?;
    let id = short_id();
    let log_path = log_dir.join(format!("{id}.log"));
    let _ = rotate_if_oversized(&log_path);

    // Open the log file in append mode — both stdout + stderr forwarders
    // share the handle so interleaved lines write atomically per-line.
    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .map_err(|e| format!("open log {}: {e}", log_path.display()))?;
    drop(log_file); // re-open via tokio later; verify writeable first

    let mut cmd = Command::new("sh");
    cmd.arg("-c")
        .arg(&args.cmd)
        .current_dir(&args.cwd)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(false);
    /* Put the shell in its own process group so `kill(-pid, SIGKILL)`
     *  reaps both the shell AND its forked children (sleep, node, etc).
     *  Without this, killing only the shell leaves long-running children
     *  orphaned (re-parented to init) and the BgTask UI thinks it's dead
     *  while ports stay bound. process_group(0) → new pgrp with the
     *  child as leader; pgid == pid for our purposes. */
    #[cfg(unix)]
    cmd.process_group(0);

    if let Some(env) = &args.env {
        for (k, v) in env {
            cmd.env(k, v);
        }
    }

    // Hardened defaults — same set the terminal bridge uses for agent
    // subprocesses. NO_COLOR helps log scrapers; CI=1 suppresses TTY
    // probes some dev servers do. PAGER=cat keeps git from forking less.
    cmd.env("NO_COLOR", "1")
        .env("PAGER", "cat")
        // Don't claim CI — many dev servers tweak behaviour and the user
        // probably wants the local dev experience. Leave as-is unless
        // the caller overrides via `env`.
        ;

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("spawn `{}`: {e}", args.cmd))?;

    let pid = child.id();
    let label = args
        .label
        .clone()
        .unwrap_or_else(|| derive_label(&args.cmd));

    let task = BgTask {
        id: id.clone(),
        label,
        cmd: args.cmd.clone(),
        cwd: args.cwd.clone(),
        session_id: args.session_id.clone(),
        pid,
        started_at: now_ms(),
        status: BgStatus::Running,
        log_path: log_path.to_string_lossy().into_owned(),
        detected_urls: Vec::new(),
        detected_ports: Vec::new(),
        recent_lines: Vec::new(),
    };

    let (line_tx, _) = broadcast::channel::<BgLine>(BROADCAST_CAPACITY);
    let (stdin_tx, mut stdin_rx) = mpsc::channel::<Vec<u8>>(32);

    /* Coalescer channel — reader loops dump each parsed line here; a
     *  single coalescer task drains it every COALESCE_WINDOW_MS (or when
     *  the buffer reaches COALESCE_BURST_CAP) and emits ONE Tauri event
     *  `bg:lines:<id>` carrying a `Vec<BgLine>`. Without this, a fast
     *  dev server (Vite/Next/turbo) firing hundreds of lines/sec did
     *  one IPC roundtrip per line + one Svelte reactivity rebuild per
     *  line — main thread saturated, whole app froze. Batching cuts
     *  IPC pressure by ~30× on bursty output. */
    let (lines_tx, lines_rx) = mpsc::unbounded_channel::<BgLine>();

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let stdin = child.stdin.take();

    let child_arc = Arc::new(AsyncMutex::new(child));

    let state = Arc::new(RwLock::new(TaskState {
        task: task.clone(),
        child: Some(child_arc.clone()),
        line_tx: line_tx.clone(),
        stdin_tx: Some(stdin_tx),
    }));

    registry.tasks.write().insert(id.clone(), state.clone());

    // Spawn the coalescer — flushes batched lines to the UI.
    {
        let app2 = app.clone();
        let id2 = id.clone();
        tokio::spawn(run_coalescer(app2, id2, lines_rx));
    }

    // Stdin forwarder — bytes-in via `bg_send_stdin` get pushed here.
    if let Some(mut stdin) = stdin {
        tokio::spawn(async move {
            while let Some(chunk) = stdin_rx.recv().await {
                if stdin.write_all(&chunk).await.is_err() {
                    break;
                }
                let _ = stdin.flush().await;
            }
        });
    } else {
        // Drop the receiver so the sender hangs up cleanly on next send.
        drop(stdin_rx);
    }

    // Stdout reader.
    if let Some(stdout) = stdout {
        let id2 = id.clone();
        let state2 = state.clone();
        let log_path2 = log_path.clone();
        let lines_tx2 = lines_tx.clone();
        tokio::spawn(async move {
            let mut reader = BufReader::new(stdout);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) => break, // EOF
                    Ok(_) => {
                        let trimmed = line.trim_end_matches(['\r', '\n']).to_string();
                        let evt = BgLine {
                            id: id2.clone(),
                            at: now_ms(),
                            stream: BgStream::Stdout,
                            line: trimmed,
                        };
                        ingest_line(&state2, evt.clone());
                        let _ = lines_tx2.send(evt);
                        // Best-effort log file append.
                        if let Ok(mut f) = std::fs::OpenOptions::new()
                            .append(true)
                            .create(true)
                            .open(&log_path2)
                        {
                            use std::io::Write;
                            let _ = f.write_all(line.as_bytes());
                        }
                    }
                    Err(_) => break,
                }
            }
        });
    }

    // Stderr reader — same as stdout, separate stream tag.
    if let Some(stderr) = stderr {
        let id2 = id.clone();
        let state2 = state.clone();
        let log_path2 = log_path.clone();
        let lines_tx2 = lines_tx.clone();
        tokio::spawn(async move {
            let mut reader = BufReader::new(stderr);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) => break,
                    Ok(_) => {
                        let trimmed = line.trim_end_matches(['\r', '\n']).to_string();
                        let evt = BgLine {
                            id: id2.clone(),
                            at: now_ms(),
                            stream: BgStream::Stderr,
                            line: trimmed,
                        };
                        ingest_line(&state2, evt.clone());
                        let _ = lines_tx2.send(evt);
                        if let Ok(mut f) = std::fs::OpenOptions::new()
                            .append(true)
                            .create(true)
                            .open(&log_path2)
                        {
                            use std::io::Write;
                            let _ = f.write_all(line.as_bytes());
                        }
                    }
                    Err(_) => break,
                }
            }
        });
    }
    /* Drop the original sender so the coalescer exits when BOTH reader
     *  loops terminate (each owns a clone). The original `lines_tx`
     *  belongs to this scope; without dropping it the channel would
     *  stay open even after stdout/stderr EOF. */
    drop(lines_tx);

    // Waiter — flips status to Exited when the child terminates.
    {
        let app2 = app.clone();
        let id2 = id.clone();
        let state2 = state.clone();
        tokio::spawn(async move {
            let exit_code: i32 = {
                let mut guard = child_arc.lock().await;
                match guard.wait().await {
                    Ok(es) => es.code().unwrap_or(-1),
                    Err(_) => -1,
                }
            };
            {
                let mut s = state2.write();
                if matches!(s.task.status, BgStatus::Running) {
                    s.task.status = BgStatus::Exited { code: exit_code };
                }
                s.child = None;
                s.task.pid = None;
            }
            let snapshot = state2.read().task.clone();
            let _ = app2.emit(&format!("bg:status:{id2}"), &snapshot);
            // Generic event for "any task changed" — lets the store
            // re-fetch list without subscribing per id.
            let _ = app2.emit("bg:tasks-changed", &id2);
        });
    }

    let _ = app.emit("bg:tasks-changed", &id);
    Ok(task)
}

/// Best-effort label derivation: first word of the command, capped at 40 chars.
fn derive_label(cmd: &str) -> String {
    let s: String = cmd
        .split_whitespace()
        .next()
        .unwrap_or("task")
        .chars()
        .take(40)
        .collect();
    if s.is_empty() {
        "task".to_string()
    } else {
        s
    }
}

pub async fn kill(app: AppHandle, registry: &BgRegistry, id: &str) -> Result<(), String> {
    let state = registry
        .tasks
        .read()
        .get(id)
        .cloned()
        .ok_or_else(|| format!("no such task: {id}"))?;
    /* Snapshot the PID *and* whether we have a live child handle. We
     *  must NOT call `child_arc.lock().await` here — the waiter task in
     *  `spawn()` holds that mutex for the entire lifetime of the child
     *  (it awaits `child.wait()` while holding the guard). Acquiring it
     *  again deadlocks the kill call indefinitely — which is the bug the
     *  UI surfaces as "kill button does nothing".
     *
     *  Instead we send SIGKILL via libc directly using the stored PID.
     *  The waiter wakes up, sets status, releases the mutex normally. */
    let (pid_opt, had_child) = {
        let s = state.read();
        (s.task.pid, s.child.is_some())
    };
    if !had_child {
        // Already reaped — UI just hasn't caught up. Idempotent OK.
        return Ok(());
    }
    let Some(pid) = pid_opt else {
        return Ok(());
    };
    #[cfg(unix)]
    {
        /* Negative pid → kill the whole process group (`spawn()` sets
         *  `process_group(0)` so pgid == pid). Reaps shell + every child
         *  it forked (sleep / node / etc) in one syscall. */
        let gpid = -(pid as i32);
        unsafe {
            // SIGTERM first — gives clean shutdown a brief window. The
            // waiter task will flip status on wait() completion below.
            let _ = libc::kill(gpid, libc::SIGTERM);
        }
        // Spawn a grace timer that escalates to SIGKILL if the process
        // group is still alive after KILL_GRACE_MS. Detached — kill()
        // returns immediately.
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(KILL_GRACE_MS)).await;
            unsafe {
                // kill(pid, 0) probes liveness; ESRCH = already dead.
                if libc::kill(pid as i32, 0) == 0 {
                    let _ = libc::kill(gpid, libc::SIGKILL);
                }
            }
        });
    }
    #[cfg(windows)]
    {
        /* Windows has no process groups in the Unix sense. Best-effort:
         *  fall back to tokio's start_kill on the stored Child. We avoid
         *  the deadlock by using try_lock — if the waiter holds it,
         *  TerminateProcess via PID-less path isn't available, but the
         *  waiter's wait() will return ERROR_INVALID_HANDLE once we
         *  TerminateProcess from another path. For now: skip on Windows
         *  if we can't get the lock; status flip below still happens. */
        if let Some(child_arc) = state.read().child.clone() {
            if let Ok(mut guard) = child_arc.try_lock() {
                let _ = guard.start_kill();
            }
        }
    }
    // Flip status immediately so the UI reflects the user's intent. The
    // waiter task will overwrite this with Exited on wait() return, but
    // our `matches!(_, Running)` guard there prevents that — once we
    // write Killed, the Exited path becomes a no-op.
    {
        let mut s = state.write();
        s.task.status = BgStatus::Killed {
            reason: "user".to_string(),
        };
    }
    let snapshot = state.read().task.clone();
    let _ = app.emit(&format!("bg:status:{id}"), &snapshot);
    let _ = app.emit("bg:tasks-changed", id);
    Ok(())
}

pub async fn send_stdin(registry: &BgRegistry, id: &str, data: Vec<u8>) -> Result<(), String> {
    let state = registry
        .tasks
        .read()
        .get(id)
        .cloned()
        .ok_or_else(|| format!("no such task: {id}"))?;
    let tx = state.read().stdin_tx.clone();
    let Some(tx) = tx else {
        return Err("stdin closed".into());
    };
    tx.send(data).await.map_err(|e| format!("stdin send: {e}"))
}

pub fn logs(registry: &BgRegistry, id: &str, tail: Option<usize>) -> Result<String, String> {
    let path = {
        let state = registry
            .tasks
            .read()
            .get(id)
            .cloned()
            .ok_or_else(|| format!("no such task: {id}"))?;
        let p = state.read().task.log_path.clone();
        p
    };
    let raw = std::fs::read_to_string(&path).map_err(|e| format!("read log: {e}"))?;
    if let Some(n) = tail {
        let lines: Vec<&str> = raw.lines().collect();
        let start = lines.len().saturating_sub(n);
        Ok(lines[start..].join("\n"))
    } else {
        Ok(raw)
    }
}

/// Subscribe and wait for next line matching `contains` (case-insensitive
/// substring; `None` matches the next line of any content), up to
/// `timeout_ms` ms. Returns the line OR `None` if the timeout elapsed.
/// Used by the `bg_wait_line` MCP tool for "react when the dev server
/// prints `Ready`" / "loop until tests pass".
pub async fn wait_line(
    registry: &BgRegistry,
    id: &str,
    contains: Option<String>,
    timeout_ms: u64,
) -> Result<Option<BgLine>, String> {
    let state = registry
        .tasks
        .read()
        .get(id)
        .cloned()
        .ok_or_else(|| format!("no such task: {id}"))?;
    /* CRITICAL ORDER: subscribe to the broadcast BEFORE we scan
     *  history. The opposite order has a race where a line that
     *  landed between "scan history" and "subscribe" disappears
     *  forever — and broadcast's `Receiver` has no replay buffer
     *  for past sends. Subscribing first guarantees any line
     *  emitted from this moment on is queued for us; then we walk
     *  `recent_lines` so we also catch matches that already fired
     *  before the call. Without this combination, a bg task that
     *  printed `Ready\n` then exited (e.g. a `until curl ...; done;
     *  echo CLICKHOUSE_READY` poll) would hang every consumer that
     *  called `wait_line` after the echo — exactly the symptom the
     *  agent hit on the trawl-clickhouse smoke test. */
    let mut rx = state.read().line_tx.subscribe();
    let needle = contains.map(|s| s.to_ascii_lowercase());
    /* History sweep — only when caller specified a needle. With
     *  `contains=None` callers explicitly want the NEXT line (e.g.
     *  "drain until the next stdout event"), so returning a stale
     *  recent_lines entry would break that contract. With a needle,
     *  callers are looking for a specific marker — they DON'T care
     *  whether it's already arrived or arrives next, just that it's
     *  there. recent_lines is capped at RECENT_LINES_CAP (30) so the
     *  walk is cheap. */
    if let Some(n) = &needle {
        let s = state.read();
        for line in s.task.recent_lines.iter() {
            if line.line.to_ascii_lowercase().contains(n) {
                return Ok(Some(line.clone()));
            }
        }
    }
    let deadline = tokio::time::Instant::now() + Duration::from_millis(timeout_ms);
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return Ok(None);
        }
        match tokio::time::timeout(remaining, rx.recv()).await {
            Ok(Ok(line)) => {
                if let Some(n) = &needle {
                    if !line.line.to_ascii_lowercase().contains(n) {
                        continue;
                    }
                }
                return Ok(Some(line));
            }
            Ok(Err(broadcast::error::RecvError::Lagged(_))) => continue,
            Ok(Err(broadcast::error::RecvError::Closed)) => return Ok(None),
            Err(_) => return Ok(None),
        }
    }
}

// ---- Tauri commands ------------------------------------------------------

#[tauri::command]
pub async fn bg_spawn(
    app: AppHandle,
    registry: State<'_, BgRegistry>,
    args: SpawnArgs,
) -> Result<BgTask, String> {
    spawn(app, &registry, args).await
}

#[tauri::command]
pub fn bg_list(registry: State<'_, BgRegistry>) -> Vec<BgTask> {
    registry.list()
}

#[tauri::command]
pub fn bg_get(registry: State<'_, BgRegistry>, id: String) -> Option<BgTask> {
    registry.get(&id)
}

#[tauri::command]
pub async fn bg_kill(
    app: AppHandle,
    registry: State<'_, BgRegistry>,
    id: String,
) -> Result<(), String> {
    kill(app, &registry, &id).await
}

#[tauri::command]
pub async fn bg_send_stdin(
    registry: State<'_, BgRegistry>,
    id: String,
    data: String,
) -> Result<(), String> {
    send_stdin(&registry, &id, data.into_bytes()).await
}

#[tauri::command]
pub fn bg_logs(
    registry: State<'_, BgRegistry>,
    id: String,
    tail: Option<usize>,
) -> Result<String, String> {
    logs(&registry, &id, tail)
}

#[tauri::command]
pub async fn bg_wait_line(
    registry: State<'_, BgRegistry>,
    id: String,
    contains: Option<String>,
    timeout_ms: u64,
) -> Result<Option<BgLine>, String> {
    wait_line(&registry, &id, contains, timeout_ms).await
}

/// Open a separate Tauri WebviewWindow pointed at a localhost URL —
/// gives the user a full-fidelity preview window (real cursor, real
/// scrolling, real keyboard) instead of the cramped inline iframe.
/// Reuses an existing window with the same label so reopening focuses
/// rather than spawning duplicates.
///
/// Why this beats `tauri-plugin-opener` (default browser): the Tauri
/// window stays bound to the Woom app's lifecycle (closes when the
/// task dies / when the user exits Woom), runs in the same process
/// (future: `WebviewWindow::eval` for agent automation — Phase 1d),
/// and avoids the cold-start of spawning Chrome.
#[tauri::command]
pub async fn preview_open_window(
    app: AppHandle,
    task_id: String,
    url: String,
    title: Option<String>,
) -> Result<(), String> {
    let parsed = tauri::Url::parse(&url).map_err(|e| format!("bad url `{url}`: {e}"))?;
    /* Sanitise to alnum + dash + underscore — Tauri rejects window
     *  labels containing `:` / spaces / dots. `task_id` is already
     *  safe (`bg-` + 10 hex chars) but be defensive. */
    let label = format!("preview-{}", task_id.replace(|c: char| !c.is_alphanumeric() && c != '-' && c != '_', "_"));

    if let Some(existing) = app.get_webview_window(&label) {
        let _ = existing.show();
        let _ = existing.unminimize();
        let _ = existing.set_focus();
        return Ok(());
    }

    let win_title = title.unwrap_or_else(|| format!("Preview · {task_id}"));
    let _w = tauri::WebviewWindowBuilder::new(
        &app,
        &label,
        tauri::WebviewUrl::External(parsed),
    )
    .title(win_title)
    .inner_size(1100.0, 800.0)
    .min_inner_size(420.0, 320.0)
    .resizable(true)
    .build()
    .map_err(|e| format!("build window: {e}"))?;
    Ok(())
}
