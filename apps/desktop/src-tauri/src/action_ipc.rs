//! Unix-socket IPC between Forgehold MCP sidecars (forgehold-github)
//! and the Tauri shell. Used to make `propose_*` MCP tools BLOCKING.
//!
//! Why: each propose_* tool needs to hold its MCP response open until
//! the user approves the action card and the underlying operation
//! (bash / git commit / PR open / cwd switch) runs to completion. The
//! agent's CLI stays parked waiting for the MCP response, so the
//! eventual result lands as the *tool_result* in the SAME turn — the
//! agent can react to the bash output / commit hash / PR url without
//! the turn ending.
//!
//! Pre-refactor flow returned "card created" to the agent immediately,
//! ended the turn, and tried to feed the eventual outcome back as a
//! synthesised next-turn user message. That broke on every edge: the
//! CLI's idle timeout fired during legitimate waits, the session-id
//! got locked when a forced-kill cascaded, and the user constantly
//! had to type "продолжи" to nudge the agent forward.
//!
//! Protocol: line-delimited JSON. Sidecar opens connection, writes
//! one CardRequest line, reads one CardResolution line, closes. Each
//! request carries a fresh `wait_id` (uuid) so the Tauri side can
//! match the eventual frontend `resolve_action_wait` invocation back
//! to the right pending socket connection.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tokio::sync::{oneshot, Mutex};

/// Sidecar → Tauri: "create an approval card with these params and
/// hold the connection open until the frontend resolves it."
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CardRequest {
    /// Forgehold session id (passed via FORGEHOLD_SESSION_ID env when
    /// the sidecar was spawned). Tells the frontend WHICH chat the
    /// card belongs to — important when multiple agents run in
    /// parallel and the same MCP-server-name is shared.
    pub session_id: String,
    /// Per-request uuid — the frontend echoes it back when resolving
    /// so we can route the response to the right waiting socket.
    pub wait_id: String,
    /// "bash" | "commit" | "pr" | "switch_cwd"
    pub kind: String,
    /// Kind-specific payload. Frontend interprets as needed:
    /// - bash:        { command, reason }
    /// - commit:      { message, body, push, note }
    /// - pr:          { title, body, base, draft, note }
    /// - switch_cwd:  { path, reason }
    pub params: serde_json::Value,
}

/// Tauri → Sidecar: "card resolved, here's the outcome."
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CardResolution {
    pub wait_id: String,
    /// True if the user approved AND the action ran without error.
    /// False on dismiss, error, or any failure path.
    pub ok: bool,
    /// Human-readable single-line summary (returned as the MCP tool
    /// result text). Includes both success info ("commit abc1234
    /// pushed") and error detail ("bash exited 128: branch already
    /// exists\n…stderr…") so the agent can react in the same turn.
    pub summary: String,
}

/// Per-session list of currently-waiting sidecar connections. We
/// store oneshot senders keyed by wait_id; the frontend's
/// `resolve_action_wait(wait_id, …)` finds the right one and shoots
/// the resolution down the channel. The connection task on the other
/// end of the channel writes the response to the socket and closes.
pub type WaitMap = Arc<Mutex<HashMap<String, oneshot::Sender<CardResolution>>>>;

/// IPC server state, parked in Tauri app state so commands can reach
/// it. Cheap to clone (everything inside is Arc).
pub struct ActionIpc {
    socket_path: PathBuf,
    waits: WaitMap,
}

impl ActionIpc {
    pub fn new(socket_path: PathBuf) -> Self {
        Self {
            socket_path,
            waits: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    pub fn waits(&self) -> WaitMap {
        self.waits.clone()
    }
}

/// Bind the Unix listener and spawn an accept loop. Each accepted
/// connection runs in its own task, reads one CardRequest, registers
/// a wait, emits a Tauri event so the frontend can show the card,
/// then awaits the oneshot and writes the resolution back.
pub fn start_ipc_server(ipc: Arc<ActionIpc>, app: AppHandle) -> std::io::Result<()> {
    // Best-effort cleanup of stale socket from a prior crash. If the
    // bind below succeeds, this matters; if a real process is bound,
    // bind() will fail and we'll surface that error.
    let _ = std::fs::remove_file(ipc.socket_path());
    let listener = UnixListener::bind(ipc.socket_path())?;

    tokio::spawn(async move {
        loop {
            let stream = match listener.accept().await {
                Ok((s, _)) => s,
                Err(e) => {
                    eprintln!("[action_ipc] accept error: {}", e);
                    // Brief backoff so a flapping listener can't burn
                    // CPU. accept() failures here are usually fd
                    // exhaustion or bind-broken; either way nothing
                    // smart we can do.
                    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                    continue;
                }
            };
            let waits = ipc.waits();
            let app = app.clone();
            tokio::spawn(async move {
                handle_connection(stream, waits, app).await;
            });
        }
    });

    Ok(())
}

async fn handle_connection(
    stream: tokio::net::UnixStream,
    waits: WaitMap,
    app: AppHandle,
) {
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);
    let mut line = String::new();
    if let Err(e) = reader.read_line(&mut line).await {
        eprintln!("[action_ipc] read error: {}", e);
        return;
    }
    let req: CardRequest = match serde_json::from_str(line.trim()) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[action_ipc] parse error: {} — line: {:?}", e, line);
            return;
        }
    };

    // Register the wait BEFORE emitting so a vanishingly fast
    // frontend response has somewhere to land.
    let (tx, rx) = oneshot::channel::<CardResolution>();
    {
        let mut w = waits.lock().await;
        w.insert(req.wait_id.clone(), tx);
    }

    // Emit to frontend — `forgehold:action_request` carries the full
    // request payload. The frontend matches by session_id, creates a
    // pending action card with `wait_id`, and on user approve runs
    // the action and invokes `resolve_action_wait(wait_id, …)`.
    let _ = app.emit("forgehold:action_request", &req);

    // Block waiting for resolution. If the channel is dropped without
    // a send (frontend gone, session deleted, app shutting down) we
    // bail without writing — the sidecar will see EOF, its MCP tool
    // returns an error, and the agent gets a useful error result.
    let resolution = match rx.await {
        Ok(r) => r,
        Err(_) => {
            // Clean up the now-orphaned wait entry just in case the
            // sender dropped without removal.
            waits.lock().await.remove(&req.wait_id);
            return;
        }
    };

    let mut body = match serde_json::to_string(&resolution) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[action_ipc] serialize error: {}", e);
            return;
        }
    };
    body.push('\n');
    let _ = write_half.write_all(body.as_bytes()).await;
    let _ = write_half.shutdown().await;
}

/// Called by the frontend (via Tauri command) once an approval card
/// has run to completion. Looks up the registered oneshot by wait_id
/// and shoots the resolution down it; the socket task picks it up,
/// writes it to the sidecar, and the sidecar's MCP tool returns.
/// Returns false if the wait_id is unknown — usually means the
/// connection died before resolution (rare; frontend just no-ops).
pub async fn resolve_wait(waits: &WaitMap, resolution: CardResolution) -> bool {
    let mut w = waits.lock().await;
    if let Some(tx) = w.remove(&resolution.wait_id) {
        let _ = tx.send(resolution);
        true
    } else {
        false
    }
}
