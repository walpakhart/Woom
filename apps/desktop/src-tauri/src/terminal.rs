//! Terminal column — one PTY-backed shell per instance.
//!
//! `terminal_spawn(opts)` allocates a fresh PTY pair, kicks off the
//! user's login shell (zsh by default), and returns a stable id the
//! frontend keeps in `<TerminalColumn instanceId>`. Output is streamed
//! out-of-band as Tauri events `terminal:output:<id>` carrying base64
//! chunks (xterm.js handles the ANSI). `terminal_write` /
//! `terminal_resize` / `terminal_kill` close the loop.
//!
//! Phase 2 (MCP) — the same Session is exposed via a localhost HTTP
//! bridge in `terminal_bridge.rs`. Agents (Claude / Cursor) call MCP
//! tools in `forgehold-app` that POST to the bridge and read/write
//! the same master fd the user is staring at. Each Session keeps a
//! ring buffer of its last ~64 KB of output so `run_command` can
//! sentinel-detect "command finished" without re-spawning a shell.
//!
//! Cleanup: instances drop on `terminal_kill` OR when the Tauri app
//! exits (the OS reaps PTY children automatically).

use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::Arc;
use std::thread;

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use parking_lot::Mutex;
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};
use tokio::sync::Notify;
use uuid::Uuid;

/// Cap on the per-session ring buffer. 64 KB is plenty for the
/// sentinel-detection scan (`run_command` pumps chunks every ~50 ms,
/// so 64 KB ≈ multiple seconds of dense output). Drain-from-front
/// on overflow — Vec drain is O(n) but n is bounded.
const BUFFER_CAP: usize = 64 * 1024;

/// One live PTY session. The master fd survives until either the
/// frontend kills it or the app exits. The reader thread owns the
/// reader half and emits events; `master` is shared (Mutex) so write
/// + resize calls from the Tauri command handlers can poke it.
pub struct Session {
    pub master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
    pub writer: Arc<Mutex<Box<dyn Write + Send>>>,
    /// Kept so `Drop` can `kill()` the child if the user forcibly
    /// closes the column. Without this the shell lingers as an
    /// orphan until the parent app exits.
    pub child: Arc<Mutex<Box<dyn portable_pty::Child + Send + Sync>>>,
    /// Rolling buffer of the last [`BUFFER_CAP`] bytes the PTY has
    /// emitted. The MCP `run_command` waiter scans this for its
    /// sentinel; `read_buffer` peeks the tail.
    pub output_buf: Arc<Mutex<Vec<u8>>>,
    /// Wakes anyone waiting on new output (the run-command sentinel
    /// loop). Reader thread calls `notify_waiters` after each chunk.
    pub output_notify: Arc<Notify>,
    /// Human-readable column name (e.g. "Notre-Dame") the frontend
    /// passed at spawn. Surfaced in `terminal_list` so the MCP agent
    /// (and the user reading the trace) sees readable column names
    /// alongside the opaque uuids.
    pub name: Option<String>,
}

#[derive(Default)]
pub struct TerminalRegistry {
    pub sessions: Mutex<HashMap<String, Arc<Session>>>,
}

impl TerminalRegistry {
    /// Snapshot of every live session as `(id, name)`. Used by the
    /// bridge `list` endpoint so the lock isn't held across `await`.
    pub fn list(&self) -> Vec<(String, Option<String>)> {
        self.sessions
            .lock()
            .iter()
            .map(|(id, s)| (id.clone(), s.name.clone()))
            .collect()
    }

    /// Resolve an id-or-name to a Session. Tries direct uuid lookup
    /// first; if that misses, scans for a session whose `name`
    /// matches the input (case-insensitive). Lets MCP callers pass
    /// readable column names (e.g. "Notre-Dame") instead of forcing
    /// every tool call to drag a uuid through chat history.
    pub fn get_by_id_or_name(&self, key: &str) -> Option<Arc<Session>> {
        let map = self.sessions.lock();
        if let Some(s) = map.get(key) {
            return Some(s.clone());
        }
        let lower = key.to_ascii_lowercase();
        for s in map.values() {
            if let Some(n) = s.name.as_deref() {
                if n.eq_ignore_ascii_case(&lower) || n.to_ascii_lowercase() == lower {
                    return Some(s.clone());
                }
            }
        }
        None
    }
}

#[derive(Debug, Serialize)]
pub struct SpawnResult {
    pub id: String,
}

#[derive(Debug, Deserialize, Default)]
pub struct SpawnOpts {
    /// Working directory the shell should `cd` into. Falls back to
    /// `$HOME` then `/` when missing — matches Terminal.app behaviour
    /// for a fresh tab without an attached folder.
    pub cwd: Option<String>,
    /// Shell binary path. Defaults to the user's `$SHELL` env, then
    /// `/bin/zsh` (macOS default), then `/bin/sh` as last resort.
    pub shell: Option<String>,
    pub cols: Option<u16>,
    pub rows: Option<u16>,
    /// Human-readable column name (art-name like "Notre-Dame"). Stored
    /// on the Session and returned by `terminal_list` so MCP agents
    /// see readable names rather than just uuids.
    pub name: Option<String>,
}

/// Spawn a shell attached to a PTY. Returns a stable id the frontend
/// uses to address this terminal in subsequent calls.
#[tauri::command]
pub fn terminal_spawn(
    app: AppHandle,
    state: State<'_, TerminalRegistry>,
    opts: Option<SpawnOpts>,
) -> Result<SpawnResult, String> {
    let opts = opts.unwrap_or_default();
    let cols = opts.cols.unwrap_or(120);
    let rows = opts.rows.unwrap_or(32);

    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| format!("openpty: {e}"))?;

    // Resolve the shell binary. Order: explicit opt → $SHELL → zsh → sh.
    let shell = opts
        .shell
        .clone()
        .or_else(|| std::env::var("SHELL").ok())
        .unwrap_or_else(|| "/bin/zsh".into());

    let mut cmd = CommandBuilder::new(&shell);
    // Login shell so the user gets their dotfiles (PATH, prompt, etc.)
    // — without this, $PATH lives in cargo-test land and the user
    // can't find `git`. Matches Terminal.app's default.
    cmd.arg("-l");
    if let Some(cwd) = opts.cwd.as_deref() {
        cmd.cwd(cwd);
    } else if let Ok(home) = std::env::var("HOME") {
        cmd.cwd(home);
    }
    // Inherit the current process env. `portable-pty` doesn't do this
    // automatically — without it the child sees an empty env and most
    // CLIs misbehave (no $TERM, no $LANG, no $PATH).
    for (k, v) in std::env::vars() {
        cmd.env(k, v);
    }
    // Tell the child it's running inside a terminal capable of basic
    // ANSI / xterm sequences. xterm.js renders all of xterm-256color.
    cmd.env("TERM", "xterm-256color");

    let child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| format!("spawn shell: {e}"))?;
    drop(pair.slave); // we keep only the master fd

    let writer = pair
        .master
        .take_writer()
        .map_err(|e| format!("take_writer: {e}"))?;
    let reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| format!("clone_reader: {e}"))?;

    let id = Uuid::new_v4().to_string();
    let output_buf = Arc::new(Mutex::new(Vec::with_capacity(BUFFER_CAP)));
    let output_notify = Arc::new(Notify::new());
    let session = Arc::new(Session {
        master: Arc::new(Mutex::new(pair.master)),
        writer: Arc::new(Mutex::new(writer)),
        child: Arc::new(Mutex::new(child)),
        output_buf: output_buf.clone(),
        output_notify: output_notify.clone(),
        name: opts.name.clone(),
    });
    state.sessions.lock().insert(id.clone(), session);

    // Reader pump. Runs on its own OS thread because `portable-pty`'s
    // reader is blocking — putting it on the tokio runtime would tie
    // up a worker waiting on read. Each chunk is base64-encoded and
    // emitted on `terminal:output:<id>` so xterm.js can write it
    // verbatim. We also append to the rolling buffer + notify so
    // `run_command` waiters can sentinel-detect.
    let event_id = id.clone();
    thread::spawn(move || {
        let mut reader = reader;
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break, // EOF — child exited
                Ok(n) => {
                    let chunk = &buf[..n];
                    let payload = STANDARD.encode(chunk);
                    let _ = app.emit(&format!("terminal:output:{event_id}"), payload);
                    {
                        let mut b = output_buf.lock();
                        b.extend_from_slice(chunk);
                        if b.len() > BUFFER_CAP {
                            // Drain from the front to keep the buffer
                            // bounded. The drain itself is O(n) but n
                            // is bounded by BUFFER_CAP so each call is
                            // O(64 KB) max.
                            let excess = b.len() - BUFFER_CAP;
                            b.drain(..excess);
                        }
                    }
                    output_notify.notify_waiters();
                }
                Err(e) => {
                    let _ = app.emit(
                        &format!("terminal:error:{event_id}"),
                        format!("read: {e}"),
                    );
                    break;
                }
            }
        }
        let _ = app.emit(&format!("terminal:exit:{event_id}"), ());
    });

    Ok(SpawnResult { id })
}

/// Write base64-encoded bytes to the PTY. xterm.js sends keystrokes
/// as raw utf-8 → we accept base64 to stay binary-clean.
#[tauri::command]
pub fn terminal_write(
    state: State<'_, TerminalRegistry>,
    id: String,
    data: String,
) -> Result<(), String> {
    let bytes = STANDARD
        .decode(data.as_bytes())
        .map_err(|e| format!("base64: {e}"))?;
    let writer = {
        let map = state.sessions.lock();
        let s = map.get(&id).ok_or_else(|| "unknown id".to_string())?;
        s.writer.clone()
    };
    let mut w = writer.lock();
    w.write_all(&bytes).map_err(|e| format!("write: {e}"))?;
    w.flush().map_err(|e| format!("flush: {e}"))?;
    Ok(())
}

/// Resize the PTY. The frontend's `xterm-addon-fit` recomputes
/// `cols × rows` whenever the column resizes; ignore failures (the
/// child may already be dead).
#[tauri::command]
pub fn terminal_resize(
    state: State<'_, TerminalRegistry>,
    id: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    let master = {
        let map = state.sessions.lock();
        let s = map.get(&id).ok_or_else(|| "unknown id".to_string())?;
        s.master.clone()
    };
    master
        .lock()
        .resize(PtySize {
            cols,
            rows,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| format!("resize: {e}"))?;
    Ok(())
}

/// Tear down the session — kill the child + drop the master fd. The
/// reader thread will see EOF and exit on its own.
#[tauri::command]
pub fn terminal_kill(
    state: State<'_, TerminalRegistry>,
    id: String,
) -> Result<(), String> {
    let removed = state.sessions.lock().remove(&id);
    if let Some(s) = removed {
        let _ = s.child.lock().kill();
    }
    Ok(())
}
