//! Claude CLI background-task watcher.
//!
//! Claude Code CLI's `Bash` tool with `run_in_background: true` spawns the
//! child detached and writes stdout/stderr to
//! `/tmp/claude-<uid>/<cwd-encoded>/<session-uuid>/tasks/<taskId>.output`.
//! There is NO sibling `.status` / `.pid` file, so we can't detect
//! completion from a marker file. Inside Claude Code's own harness the
//! agent runs `BashOutput` on demand AND the agent loop is itself
//! reactive enough that the user can poke "status?" without losing
//! context.
//!
//! In Woom the agent's turn ends right after the Bash tool returns
//! "Command running in background with ID: <id>", so the agent never
//! auto-resumes when the build finishes. This module fills the gap:
//! the frontend registers `(session_id, task_id, output_path)` when it
//! sees that line in a `tool_result`, we spawn a polling task per
//! registered file, and emit `claude:bg_done` with a tail of the
//! output when the file's mtime has been stable long enough for the
//! task to be considered idle.
//!
//! Done-detection is a heuristic — Claude doesn't expose the child
//! process's exit code anywhere we can read. We treat "no output for
//! N seconds" as "probably done; check it". The frontend then fires a
//! silent agent prompt with the tail; the agent can call `BashOutput`
//! itself if it needs more.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use parking_lot::Mutex;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::task::JoinHandle;

/// How often the poll loop re-stats the output file.
const POLL_INTERVAL: Duration = Duration::from_secs(2);
/// Output file's mtime must be unchanged for this long before the task
/// is considered "done". Long enough that a slow build (cargo, sqlx
/// macros) doesn't false-positive between two compilation phases; short
/// enough that a finished `psql` script doesn't sit idle for a minute.
const IDLE_THRESHOLD: Duration = Duration::from_secs(20);
/// Hard ceiling per watcher. Past this we give up — better than
/// leaking a tokio task forever if something keeps the file open.
const MAX_WATCH_LIFETIME: Duration = Duration::from_secs(60 * 30);
/// Tail bytes captured for the `claude:bg_done` event payload. The
/// frontend trims further; we cap here to keep the Tauri IPC small.
const TAIL_BYTES: usize = 8 * 1024;

#[derive(Default)]
pub struct ClaudeBgRegistry {
    inner: Mutex<HashMap<String, JoinHandle<()>>>,
}

impl ClaudeBgRegistry {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct ClaudeBgDoneEvent {
    pub session_id: String,
    pub task_id: String,
    pub output_path: String,
    pub tail: String,
    /// True when the watcher stopped because of the lifetime cap rather
    /// than detecting idle output. Lets the frontend pick different
    /// copy ("bg task likely still running but watcher timed out…").
    pub timed_out: bool,
}

/// Register a watch for a Claude CLI background task. Idempotent on
/// `task_id` — calling twice with the same id replaces the prior
/// watcher (the second call usually means the agent re-emitted the
/// spawn message on `--resume` and we should track the latest path).
#[tauri::command]
pub fn claude_bg_watch(
    app: AppHandle,
    state: State<'_, Arc<ClaudeBgRegistry>>,
    session_id: String,
    task_id: String,
    output_path: String,
) -> Result<(), String> {
    let path = PathBuf::from(&output_path);
    let registry = state.inner_arc();
    let app_clone = app.clone();
    let session_clone = session_id.clone();
    let task_clone = task_id.clone();

    // Cancel any prior watcher on this task_id before installing the
    // replacement, so we don't end up with two pollers racing on the
    // same file.
    {
        let mut guard = registry.inner.lock();
        if let Some(prev) = guard.remove(&task_id) {
            prev.abort();
        }
    }

    let handle = tokio::spawn(async move {
        let start = std::time::Instant::now();
        let mut last_mtime: Option<SystemTime> = None;
        let mut last_change = std::time::Instant::now();

        loop {
            tokio::time::sleep(POLL_INTERVAL).await;

            // Lifetime cap. Emit a `timed_out` event so the frontend
            // can still nudge the agent (better to false-positive than
            // sit silent forever on a hung process).
            if start.elapsed() > MAX_WATCH_LIFETIME {
                let tail = read_tail(&path).unwrap_or_default();
                let _ = app_clone.emit(
                    "claude:bg_done",
                    ClaudeBgDoneEvent {
                        session_id: session_clone.clone(),
                        task_id: task_clone.clone(),
                        output_path: output_path.clone(),
                        tail,
                        timed_out: true,
                    },
                );
                break;
            }

            let mtime = match tokio::fs::metadata(&path).await {
                Ok(m) => m.modified().ok(),
                // File not yet created — keep waiting up to lifetime
                // cap. Claude CLI may delay creation for a second or
                // two after the tool_result text lands.
                Err(_) => continue,
            };

            let now = std::time::Instant::now();
            match (last_mtime, mtime) {
                (Some(prev), Some(cur)) if prev != cur => {
                    last_change = now;
                    last_mtime = Some(cur);
                }
                (None, Some(cur)) => {
                    last_change = now;
                    last_mtime = Some(cur);
                }
                _ => {}
            }

            if last_mtime.is_some() && now.duration_since(last_change) > IDLE_THRESHOLD {
                let tail = read_tail(&path).unwrap_or_default();
                let _ = app_clone.emit(
                    "claude:bg_done",
                    ClaudeBgDoneEvent {
                        session_id: session_clone.clone(),
                        task_id: task_clone.clone(),
                        output_path: output_path.clone(),
                        tail,
                        timed_out: false,
                    },
                );
                break;
            }
        }

        // Self-deregister so the registry doesn't accumulate dead
        // handles. Locking failure here is impossible (registry lives
        // for the app lifetime), but `if let` keeps the future
        // panic-free.
        let app_state: Option<State<'_, Arc<ClaudeBgRegistry>>> = app_clone.try_state();
        if let Some(state) = app_state {
            state.inner.lock().remove(&task_clone);
        }
    });

    registry.inner.lock().insert(task_id, handle);
    Ok(())
}

/// Cancel a watch before it naturally ends. Used when the session is
/// deleted or the user manually moves on (e.g. types into the chat
/// while the bg task is still running — we don't want a delayed silent
/// fire after they've already moved past).
#[tauri::command]
pub fn claude_bg_unwatch(
    state: State<'_, Arc<ClaudeBgRegistry>>,
    task_id: String,
) -> Result<(), String> {
    if let Some(handle) = state.inner.lock().remove(&task_id) {
        handle.abort();
    }
    Ok(())
}

/// Cancel every active watch for `session_id`. The registry doesn't
/// index by session today (would force a second map) so we walk the
/// few live tasks; in practice there are 0–2.
#[tauri::command]
pub fn claude_bg_unwatch_session(
    _state: State<'_, Arc<ClaudeBgRegistry>>,
    _session_id: String,
) -> Result<(), String> {
    // Currently a no-op stub — we don't track session_id → task_ids
    // server-side. Frontend tracks the mapping and calls
    // `claude_bg_unwatch` per task. Kept here for API symmetry so the
    // frontend can call this without a hard error if it later wants a
    // single bulk call.
    Ok(())
}

fn read_tail(path: &PathBuf) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    let start = bytes.len().saturating_sub(TAIL_BYTES);
    // Cut at the next UTF-8 boundary to avoid emitting invalid bytes
    // through the Tauri IPC. Worst case we drop the first few bytes of
    // the truncated tail.
    let slice = &bytes[start..];
    let safe_start = slice
        .iter()
        .position(|b| (*b as i8) >= -0x40)
        .unwrap_or(0);
    String::from_utf8(slice[safe_start..].to_vec()).ok()
}

trait InnerArc {
    fn inner_arc(&self) -> Arc<ClaudeBgRegistry>;
}

impl InnerArc for State<'_, Arc<ClaudeBgRegistry>> {
    fn inner_arc(&self) -> Arc<ClaudeBgRegistry> {
        Arc::clone(self.inner())
    }
}
