//! HTTP client for the desktop app's terminal-MCP bridge.
//!
//! On launch the desktop app spins up an axum server on a random
//! 127.0.0.1 port and writes the port to `<app_data>/bridge.port`.
//! This client reads that file and POSTs into the bridge for every
//! `terminal.*` MCP tool call. Failure modes are user-facing — the
//! agent gets a clear "terminal bridge not reachable" rather than a
//! cryptic timeout — so the agent can suggest the user open a
//! terminal column or restart Woom.

use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

const APP_DATA_SUBDIR: &str = "com.woom.desktop";
const PORT_FILE: &str = "bridge.port";
/// Cap each HTTP call so a stuck bridge doesn't hang the agent's
/// turn. The /run endpoint has its own internal timeout that's
/// strictly longer (passed in `timeout_ms`) — this is just the
/// outer client-side guard.
const CLIENT_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, thiserror::Error)]
pub enum BridgeError {
    #[error("terminal bridge port file not found at {0:?} — is Woom running?")]
    PortFileMissing(PathBuf),
    #[error("terminal bridge port file is malformed: {0}")]
    PortFileMalformed(String),
    #[error("terminal bridge HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("terminal bridge returned {0}: {1}")]
    BadStatus(u16, String),
}

#[derive(Clone)]
pub struct BridgeClient {
    base_url: String,
    http: reqwest::Client,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ListResp {
    pub instances: Vec<InstanceLite>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct InstanceLite {
    /// Workbench column display name (e.g. "Notre-Dame"). This is
    /// what should be used to address terminals — first field on
    /// purpose so the agent picks it as canonical. Optional only
    /// because legacy sessions predating column-naming have no name.
    #[serde(default)]
    pub name: Option<String>,
    /// Per-spawn uuid. Useful only for disambiguation when two
    /// columns share a name (rare). Renamed from `id` so the agent
    /// doesn't reflexively grab the uuid.
    pub uuid: String,
}

#[derive(Debug, Serialize)]
pub struct WriteReq {
    pub data_b64: String,
}

#[derive(Debug, Serialize)]
pub struct RunReq {
    pub cmd: String,
    /// Idle timeout (ms) — bridge resets this on every chunk of
    /// output. Default 60_000. Long-running commands that stream
    /// progress don't need a higher value — pass-through 60s is
    /// fine because output keeps the deadline rolling.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    /// Absolute cap (ms). Default 30 minutes server-side.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_timeout_ms: Option<u64>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct RunResp {
    pub stdout: String,
    pub exit_code: i32,
    pub timed_out: bool,
    /// When `timed_out: true` and the bridge detected an interactive
    /// prompt waiting on user input (Y/N, password, "Press Enter",
    /// etc.), this carries the prompt line so the MCP tool can
    /// surface it directly to the agent.
    #[serde(default)]
    pub interactive_prompt: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct BufferResp {
    pub text: String,
    pub total_bytes: u64,
}

impl BridgeClient {
    /// Resolve the desktop app's port file → build a base URL. We
    /// re-resolve on every call rather than caching because the
    /// desktop could be restarted (new port) while the sidecar
    /// stays alive.
    pub fn discover() -> Result<Self, BridgeError> {
        let port_file = port_file_path();
        let raw = std::fs::read_to_string(&port_file)
            .map_err(|_| BridgeError::PortFileMissing(port_file.clone()))?;
        let port: u16 = raw
            .trim()
            .parse()
            .map_err(|e| BridgeError::PortFileMalformed(format!("{e}: {raw:?}")))?;
        let http = reqwest::Client::builder()
            .timeout(CLIENT_TIMEOUT)
            .build()?;
        Ok(Self {
            base_url: format!("http://127.0.0.1:{port}"),
            http,
        })
    }

    pub async fn list(&self) -> Result<ListResp, BridgeError> {
        let url = format!("{}/v1/terminals", self.base_url);
        let resp = self.http.get(url).send().await?;
        unpack(resp).await
    }

    pub async fn write(&self, id: &str, req: WriteReq) -> Result<(), BridgeError> {
        let url = format!("{}/v1/terminals/{id}/write", self.base_url);
        let resp = self.http.post(url).json(&req).send().await?;
        unpack_unit(resp).await
    }

    pub async fn run(&self, id: &str, req: RunReq) -> Result<RunResp, BridgeError> {
        let url = format!("{}/v1/terminals/{id}/run", self.base_url);
        let resp = self.http.post(url).json(&req).send().await?;
        unpack(resp).await
    }

    pub async fn buffer(
        &self,
        id: &str,
        lines: Option<usize>,
    ) -> Result<BufferResp, BridgeError> {
        let mut url = format!("{}/v1/terminals/{id}/buffer", self.base_url);
        if let Some(n) = lines {
            url.push_str(&format!("?lines={n}"));
        }
        let resp = self.http.get(url).send().await?;
        unpack(resp).await
    }
}

async fn unpack<T: for<'de> Deserialize<'de>>(
    resp: reqwest::Response,
) -> Result<T, BridgeError> {
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(BridgeError::BadStatus(status.as_u16(), body));
    }
    let parsed = resp.json::<T>().await?;
    Ok(parsed)
}

async fn unpack_unit(resp: reqwest::Response) -> Result<(), BridgeError> {
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(BridgeError::BadStatus(status.as_u16(), body));
    }
    Ok(())
}

fn port_file_path() -> PathBuf {
    // `dirs::data_dir()` on macOS resolves to `~/Library/Application
    // Support`. We append the bundle id so the path matches whatever
    // Tauri wrote into via `app.path().app_data_dir()`.
    dirs::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join(APP_DATA_SUBDIR)
        .join(PORT_FILE)
}
