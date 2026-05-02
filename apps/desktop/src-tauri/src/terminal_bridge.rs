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
//!   POST /:id/run         — body { cmd, timeout_ms? }, sentinel-await
//!   GET  /:id/buffer?lines=N
//!                         — last N lines of accumulated output
//!
//! `run` writes `{ <cmd>; }; printf '\n__FGH_DONE_<uuid>__%d\n' $?\n`
//! into the PTY and polls the rolling output buffer for the marker.
//! On match it returns the captured stdout (ANSI-stripped) + exit
//! code; on timeout it returns whatever it had + a `timed_out` flag.
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
    id: String,
    /// Human-readable column name from the workbench (e.g.
    /// "Notre-Dame"). Optional because legacy spawns may not have
    /// passed it. Surfaced in the MCP `terminal_list` reply so the
    /// agent can address terminals by name in its response text.
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
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
    /// Hard deadline for sentinel detection. Default 60s — long
    /// enough for `cargo build` on a clean cache, short enough that
    /// a wedged command surfaces as a clear timeout in the agent's
    /// tool result rather than blocking the chat indefinitely.
    timeout_ms: Option<u64>,
}

#[derive(Serialize)]
struct RunResp {
    stdout: String,
    exit_code: i32,
    timed_out: bool,
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
        .map(|(id, name)| InstanceLite { id, name })
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
        .get(&id)
        .ok_or_else(|| BridgeErr(StatusCode::NOT_FOUND, "unknown id".into()))?;
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
    let session = s
        .app
        .state::<TerminalRegistry>()
        .get(&id)
        .ok_or_else(|| BridgeErr(StatusCode::NOT_FOUND, "unknown id".into()))?;
    let timeout = Duration::from_millis(req.timeout_ms.unwrap_or(60_000));
    let sentinel_uuid = Uuid::new_v4().simple().to_string();
    let sentinel_marker = format!("__FGH_DONE_{sentinel_uuid}__");

    // Snapshot the buffer length *before* we write — we'll only scan
    // the bytes that arrive after our command. Scanning the whole
    // buffer would risk matching prior output or echoes.
    let scan_start = session.output_buf.lock().len();

    // Wrap the user's command in a brace block so `$?` reflects the
    // last command run inside, not `printf`. Append a sentinel line
    // with the exit code; the marker contains a per-call uuid so it
    // can never collide with a prior run's wait.
    let wrapped = format!(
        "{{ {cmd}\n}}\nprintf '\\n{sentinel}%d\\n' \"$?\"\n",
        cmd = req.cmd,
        sentinel = sentinel_marker
    );
    {
        let mut w = session.writer.lock();
        w.write_all(wrapped.as_bytes())
            .map_err(|e| format!("write: {e}"))?;
        w.flush().map_err(|e| format!("flush: {e}"))?;
    }

    // Wait for the sentinel. We mostly notify-wait but fall through
    // every 200 ms anyway so a stuck pipe doesn't leave us blocked
    // past the deadline.
    let deadline = Instant::now() + timeout;
    loop {
        let now = Instant::now();
        if now >= deadline {
            // Timed out — return whatever we have so far.
            let buf = session.output_buf.lock();
            let new_bytes = if scan_start < buf.len() {
                buf[scan_start..].to_vec()
            } else {
                Vec::new()
            };
            drop(buf);
            return Ok(Json(RunResp {
                stdout: clean_output(&new_bytes, &sentinel_marker),
                exit_code: -1,
                timed_out: true,
            }));
        }
        // Try to find the sentinel.
        {
            let buf = session.output_buf.lock();
            if scan_start < buf.len() {
                if let Some((stdout_bytes, exit_code)) =
                    extract_sentinel(&buf[scan_start..], &sentinel_marker)
                {
                    return Ok(Json(RunResp {
                        stdout: clean_output(&stdout_bytes, &sentinel_marker),
                        exit_code,
                        timed_out: false,
                    }));
                }
            }
        }
        // Wait either for a new chunk or a small heartbeat tick.
        let remaining = deadline.saturating_duration_since(now);
        let _ = tokio::time::timeout(
            remaining.min(Duration::from_millis(200)),
            session.output_notify.notified(),
        )
        .await;
    }
}

/// Locate the sentinel `__FGH_DONE_<uuid>__<digits>\n` in `chunk` and
/// split off the bytes that came before it as stdout. Returns the
/// stdout slice + parsed exit code. Caller is responsible for
/// stripping ANSI from stdout.
fn extract_sentinel(chunk: &[u8], marker: &str) -> Option<(Vec<u8>, i32)> {
    let needle = marker.as_bytes();
    let pos = chunk.windows(needle.len()).position(|w| w == needle)?;
    // Read digits after the marker, stop at first non-digit.
    let digits_start = pos + needle.len();
    let mut digits_end = digits_start;
    while digits_end < chunk.len() && chunk[digits_end].is_ascii_digit() {
        digits_end += 1;
    }
    let exit_code: i32 = std::str::from_utf8(&chunk[digits_start..digits_end])
        .ok()?
        .parse()
        .ok()?;
    Some((chunk[..pos].to_vec(), exit_code))
}

/// Strip ANSI escape sequences + the sentinel marker, then decode
/// as utf-8 (lossy — terminal output isn't strictly valid utf-8 if
/// there's binary in the middle).
fn clean_output(bytes: &[u8], sentinel_marker: &str) -> String {
    let stripped = strip_ansi_escapes::strip(bytes);
    let mut s = String::from_utf8_lossy(&stripped).into_owned();
    // Belt-and-suspenders: in case the sentinel appeared in echo'd
    // command text (some shells echo input back), drop any line
    // containing the marker.
    s = s
        .lines()
        .filter(|l| !l.contains(sentinel_marker))
        .collect::<Vec<_>>()
        .join("\n");
    s
}

async fn buffer(
    State(s): State<BridgeState>,
    Path(id): Path<String>,
    Query(q): Query<BufferQuery>,
) -> Result<Json<BufferResp>, BridgeErr> {
    let session = s
        .app
        .state::<TerminalRegistry>()
        .get(&id)
        .ok_or_else(|| BridgeErr(StatusCode::NOT_FOUND, "unknown id".into()))?;
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
