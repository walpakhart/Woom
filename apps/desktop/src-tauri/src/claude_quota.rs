//! Plan-usage / rate-limit reporting for Claude Code subscription
//! users. Backs the same data the interactive `/usage` command shows
//! in the CLI: 5-hour limit, weekly all-models, weekly Sonnet-only,
//! weekly Opus-only, and the Claude Design ("omelette") bucket.
//!
//! Source is an undocumented but stable OAuth-scoped endpoint:
//!   GET https://api.anthropic.com/api/oauth/usage
//!   Authorization: Bearer <oauth_access_token>
//!   anthropic-beta: oauth-2025-04-20
//!
//! The access token lives in the user's macOS Keychain under service
//! `Claude Code-credentials` (the same place the `claude` CLI stores
//! it after `claude login`). We shell out to `security
//! find-generic-password -s ... -w` because the existing
//! `crate::keychain` module is scoped to Forge-owned services.
//!
//! Why undocumented endpoint and not a CLI subcommand:
//!   - `claude` CLI has no `--json` flag for `/usage`; the slash
//!     command is interactive-only (no stdout JSON in `-p` mode).
//!   - There is an open feature request for `claude usage --json`
//!     (anthropics/claude-code#40793) but it isn't shipped.
//!   - The endpoint is what Claude Code itself calls; same auth,
//!     same shape — we read it directly so we get the same numbers.
//!
//! Security: the access token is read into a local `String`, sent
//! over TLS to api.anthropic.com, and dropped at function end. Never
//! logged, never written to disk, never returned to the frontend.

use std::process::Command;
use std::time::Duration;

use serde::{Deserialize, Serialize};

const KEYCHAIN_SERVICE: &str = "Claude Code-credentials";
const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
const USAGE_BETA: &str = "oauth-2025-04-20";

/// A single quota bucket. `utilization` is a percentage 0–100 (the
/// API returns floats); `resets_at` is an ISO-8601 timestamp string.
/// Some buckets (e.g. `seven_day_sonnet` when never used) come back
/// with `resets_at: null` — we keep both fields optional so the
/// frontend can decide whether to render the reset-time tail.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanUsageBucket {
    #[serde(default)]
    pub utilization: Option<f64>,
    #[serde(default)]
    pub resets_at: Option<String>,
}

/// Subset of `/api/oauth/usage` response we surface. The endpoint
/// returns more buckets (`iguana_necktie`, `omelette_promotional`,
/// `seven_day_oauth_apps`, `seven_day_cowork`) which appear to be
/// experimental / promotional / org-specific — we ignore them rather
/// than expose internals that may rename.
///
/// `seven_day_omelette` is the internal codename for the "Claude
/// Design" weekly bucket (matches what the Claude Code CLI renders
/// as "Weekly · Claude Design").
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PlanUsage {
    #[serde(default)]
    pub five_hour: Option<PlanUsageBucket>,
    #[serde(default)]
    pub seven_day: Option<PlanUsageBucket>,
    #[serde(default)]
    pub seven_day_sonnet: Option<PlanUsageBucket>,
    #[serde(default)]
    pub seven_day_opus: Option<PlanUsageBucket>,
    #[serde(default)]
    pub seven_day_omelette: Option<PlanUsageBucket>,
}

#[derive(Debug, thiserror::Error)]
pub enum QuotaError {
    #[error("Claude Code OAuth token not found in keychain — log in via `claude login` first")]
    NoToken,
    #[error("Claude Code keychain entry was malformed (expected JSON with `.claudeAiOauth.accessToken`)")]
    MalformedToken,
    #[error("usage endpoint returned HTTP {0}")]
    Http(reqwest::StatusCode),
    #[error("network error: {0}")]
    Network(String),
}

/// Read the OAuth access token from the macOS Keychain entry the
/// Claude Code CLI created at `claude login`. Returns `None` (not an
/// error) when the entry is missing — the caller surfaces a friendlier
/// "log in first" message via `QuotaError::NoToken`.
fn read_oauth_token() -> Option<String> {
    let out = Command::new("security")
        .args(["find-generic-password", "-s", KEYCHAIN_SERVICE, "-w"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let raw = String::from_utf8(out.stdout).ok()?;
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let parsed: serde_json::Value = serde_json::from_str(raw).ok()?;
    parsed
        .get("claudeAiOauth")
        .and_then(|c| c.get("accessToken"))
        .and_then(|t| t.as_str())
        .map(String::from)
}

/// Fetch the current plan-usage snapshot from the OAuth `/usage`
/// endpoint. Caller is responsible for caching — the endpoint
/// 429s aggressively when polled tightly.
pub async fn fetch_plan_usage() -> Result<PlanUsage, QuotaError> {
    let Some(token) = read_oauth_token() else {
        return Err(QuotaError::NoToken);
    };
    if token.trim().is_empty() {
        return Err(QuotaError::MalformedToken);
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| QuotaError::Network(e.to_string()))?;
    let resp = client
        .get(USAGE_URL)
        .bearer_auth(token)
        .header("anthropic-beta", USAGE_BETA)
        .header("user-agent", "Forgehold/0.1")
        .send()
        .await
        .map_err(|e| QuotaError::Network(e.to_string()))?;
    if !resp.status().is_success() {
        return Err(QuotaError::Http(resp.status()));
    }
    resp.json::<PlanUsage>()
        .await
        .map_err(|e| QuotaError::Network(e.to_string()))
}
