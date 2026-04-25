//! forgehold-sentry — MCP sidecar exposing the user's Sentry org as tools
//! available to Claude Code (and Cursor).
//!
//! Reads credentials from env at launch:
//!   SENTRY_HOST          base URL, e.g. https://sentry.io
//!   SENTRY_ORG           org slug, e.g. acme-co
//!   SENTRY_TOKEN         Auth token from <host>/settings/account/api/auth-tokens/
//!                        (needs org:read, project:read, event:read scopes)
//!
//! Speaks MCP over stdio. Spawned by Forgehold's main Tauri process with
//! creds pulled from Keychain and passed as env. Never reads disk for
//! creds. Read-only — exposes get_issue / search_issues / get_event /
//! get_latest_event tools.

use anyhow::Context;
use rmcp::{
    ErrorData, ServerHandler, ServiceExt,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, Content, ServerCapabilities, ServerInfo},
    schemars, tool, tool_handler, tool_router,
    transport::stdio,
};
use serde::Deserialize;

const USER_AGENT: &str = concat!("forgehold-sentry/", env!("CARGO_PKG_VERSION"));

#[derive(Clone)]
struct Creds {
    host: String,
    org: String,
    token: String,
}

impl Creds {
    fn from_env() -> anyhow::Result<Self> {
        let host_raw = std::env::var("SENTRY_HOST").unwrap_or_else(|_| "https://sentry.io".into());
        let host = normalize_host(&host_raw);
        let org = std::env::var("SENTRY_ORG").context("SENTRY_ORG env var is required")?;
        let token = std::env::var("SENTRY_TOKEN").context("SENTRY_TOKEN env var is required")?;
        if org.is_empty() || token.is_empty() {
            anyhow::bail!("SENTRY_ORG and SENTRY_TOKEN must be non-empty");
        }
        Ok(Self { host, org, token })
    }
}

fn normalize_host(raw: &str) -> String {
    let trimmed = raw.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return "https://sentry.io".to_string();
    }
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        trimmed.to_string()
    } else {
        format!("https://{}", trimmed)
    }
}

#[derive(Clone)]
struct Sentry {
    creds: Creds,
    http: reqwest::Client,
    #[allow(dead_code)] // read by `#[tool_handler]` macro expansion
    tool_router: ToolRouter<Self>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct GetIssueParams {
    /// Issue id (numeric "123456") or short id ("PROJ-1").
    issue_id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SearchParams {
    /// Sentry search syntax. Examples: `is:unresolved`,
    /// `is:unresolved level:error project:web`, `event.type:error
    /// timesSeen:>10`. Empty = `is:unresolved`.
    #[serde(default)]
    query: Option<String>,
    /// Max issues (default 25, cap 100).
    #[serde(default)]
    max_results: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct GetEventParams {
    /// Issue id or short id — same as `get_issue`.
    issue_id: String,
    /// `latest` (default), `oldest`, or a specific eventID.
    #[serde(default)]
    event_id: Option<String>,
}

#[tool_router]
impl Sentry {
    fn new(creds: Creds) -> anyhow::Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .build()
            .context("build reqwest client")?;
        Ok(Self { creds, http, tool_router: Self::tool_router() })
    }

    #[tool(
        description = "Fetch a Sentry issue (an error group) by id or short id. Returns level, status, project, last/first seen, occurrence + user counts, culprit/title/metadata, and the permalink."
    )]
    async fn get_issue(
        &self,
        Parameters(GetIssueParams { issue_id }): Parameters<GetIssueParams>,
    ) -> Result<CallToolResult, ErrorData> {
        match self.fetch_issue(&issue_id).await {
            Ok(text) => Ok(CallToolResult::success(vec![Content::text(text)])),
            Err(e) => Err(ErrorData::internal_error(e.to_string(), None)),
        }
    }

    #[tool(
        description = "Search Sentry issues. Default = unresolved sorted by recency. Supports Sentry's search syntax (level:, project:, is:, etc.). Returns id, short_id, title, level, status, count, last seen."
    )]
    async fn search_issues(
        &self,
        Parameters(SearchParams { query, max_results }): Parameters<SearchParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let q = query.as_deref().unwrap_or("is:unresolved");
        let max = max_results.unwrap_or(25).min(100);
        match self.fetch_issues(q, max).await {
            Ok(text) => Ok(CallToolResult::success(vec![Content::text(text)])),
            Err(e) => Err(ErrorData::internal_error(e.to_string(), None)),
        }
    }

    #[tool(
        description = "Fetch a single event (one occurrence of an error). Pass event_id='latest' to get the most recent. Returns the event metadata, message, exception type/value, and the first stack frame summary when available."
    )]
    async fn get_event(
        &self,
        Parameters(GetEventParams { issue_id, event_id }): Parameters<GetEventParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let target = event_id.unwrap_or_else(|| "latest".into());
        match self.fetch_event(&issue_id, &target).await {
            Ok(text) => Ok(CallToolResult::success(vec![Content::text(text)])),
            Err(e) => Err(ErrorData::internal_error(e.to_string(), None)),
        }
    }
}

#[tool_handler]
impl ServerHandler for Sentry {
    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::default();
        info.capabilities = ServerCapabilities::builder().enable_tools().build();
        info.instructions = Some(format!(
            "Read-only access to the user's Sentry organization '{}' on {}. \
             Use get_issue when the user references an error short-id (e.g. 'PROJ-123'); use search_issues for lists; \
             use get_event with event_id='latest' to fetch the most recent occurrence (includes stack frames + breadcrumbs).",
            self.creds.org, self.creds.host
        ));
        info
    }
}

impl Sentry {
    async fn fetch_issue(&self, issue_id: &str) -> anyhow::Result<String> {
        let url = format!("{}/api/0/issues/{}/", self.creds.host, issue_id);
        let v = self.get_json(&url).await?;
        Ok(format_issue(&v, &self.creds.host))
    }

    async fn fetch_issues(&self, query: &str, max_results: u32) -> anyhow::Result<String> {
        let url = format!(
            "{}/api/0/organizations/{}/issues/?query={}&limit={}&sort=date",
            self.creds.host,
            self.creds.org,
            urlencoding::encode(query),
            max_results
        );
        let v = self.get_json(&url).await?;
        Ok(format_issues(&v))
    }

    async fn fetch_event(&self, issue_id: &str, event_id: &str) -> anyhow::Result<String> {
        let url = format!(
            "{}/api/0/issues/{}/events/{}/",
            self.creds.host, issue_id, event_id
        );
        let v = self.get_json(&url).await?;
        Ok(format_event(&v, issue_id, &self.creds.host))
    }

    async fn get_json(&self, url: &str) -> anyhow::Result<serde_json::Value> {
        let resp = self
            .http
            .get(url)
            .bearer_auth(&self.creds.token)
            .header("Accept", "application/json")
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Sentry {} on {}: {}", status, url, truncate(&body, 400));
        }
        Ok(resp.json().await?)
    }
}

fn format_issue(v: &serde_json::Value, host: &str) -> String {
    let id = v.get("id").and_then(|s| s.as_str()).unwrap_or("?");
    let short_id = v.get("shortId").and_then(|s| s.as_str()).unwrap_or("");
    let title = v.get("title").and_then(|s| s.as_str()).unwrap_or("(untitled)");
    let level = v.get("level").and_then(|s| s.as_str()).unwrap_or("error");
    let status = v.get("status").and_then(|s| s.as_str()).unwrap_or("unresolved");
    let platform = v.get("platform").and_then(|s| s.as_str()).unwrap_or("");
    let project_slug = v
        .get("project")
        .and_then(|p| p.get("slug"))
        .and_then(|s| s.as_str())
        .unwrap_or("");
    let project_name = v
        .get("project")
        .and_then(|p| p.get("name"))
        .and_then(|s| s.as_str())
        .unwrap_or("");
    let count = v
        .get("count")
        .and_then(|s| s.as_str().map(String::from).or_else(|| s.as_u64().map(|n| n.to_string())))
        .unwrap_or_default();
    let user_count = v.get("userCount").and_then(|s| s.as_u64()).unwrap_or(0);
    let first_seen = v.get("firstSeen").and_then(|s| s.as_str()).unwrap_or("");
    let last_seen = v.get("lastSeen").and_then(|s| s.as_str()).unwrap_or("");
    let permalink = v.get("permalink").and_then(|s| s.as_str()).unwrap_or("");
    let culprit = v.get("culprit").and_then(|s| s.as_str()).unwrap_or("");
    let metadata = v.get("metadata").cloned().unwrap_or(serde_json::Value::Null);
    let meta_type = metadata.get("type").and_then(|s| s.as_str()).unwrap_or("");
    let meta_value = metadata.get("value").and_then(|s| s.as_str()).unwrap_or("");

    let mut out = String::new();
    out.push_str(&format!("{} — {}\n", short_id, title));
    if !meta_type.is_empty() || !meta_value.is_empty() {
        let exc = if meta_type.is_empty() {
            meta_value.to_string()
        } else if meta_value.is_empty() {
            meta_type.to_string()
        } else {
            format!("{}: {}", meta_type, meta_value)
        };
        out.push_str(&format!("Exception: {}\n", exc));
    }
    if !culprit.is_empty() {
        out.push_str(&format!("Culprit: {}\n", culprit));
    }
    out.push_str(&format!("Level: {} · Status: {}\n", level, status));
    if !project_slug.is_empty() {
        out.push_str(&format!(
            "Project: {} ({}{})\n",
            project_name,
            project_slug,
            if platform.is_empty() { String::new() } else { format!(", {}", platform) }
        ));
    }
    out.push_str(&format!(
        "Occurrences: {} · Users affected: {}\n",
        if count.is_empty() { "?".into() } else { count },
        user_count
    ));
    if !first_seen.is_empty() || !last_seen.is_empty() {
        out.push_str(&format!("First seen: {} · Last seen: {}\n", first_seen, last_seen));
    }
    if !permalink.is_empty() {
        out.push_str(&format!("URL: {}\n", permalink));
    } else if !id.is_empty() {
        out.push_str(&format!("URL: {}/issues/{}/\n", host, id));
    }
    out
}

fn format_issues(v: &serde_json::Value) -> String {
    let arr = match v.as_array() {
        Some(a) => a,
        None => return "No issues matched.".into(),
    };
    if arr.is_empty() {
        return "No issues matched.".into();
    }
    let mut out = String::new();
    out.push_str(&format!("{} issue(s):\n", arr.len()));
    for i in arr {
        let short_id = i.get("shortId").and_then(|s| s.as_str()).unwrap_or("?");
        let title = i.get("title").and_then(|s| s.as_str()).unwrap_or("(untitled)");
        let level = i.get("level").and_then(|s| s.as_str()).unwrap_or("");
        let status = i.get("status").and_then(|s| s.as_str()).unwrap_or("");
        let count = i
            .get("count")
            .and_then(|s| s.as_str().map(String::from).or_else(|| s.as_u64().map(|n| n.to_string())))
            .unwrap_or_default();
        let last_seen = i.get("lastSeen").and_then(|s| s.as_str()).unwrap_or("");
        let project = i
            .get("project")
            .and_then(|p| p.get("slug"))
            .and_then(|s| s.as_str())
            .unwrap_or("");
        let permalink = i.get("permalink").and_then(|s| s.as_str()).unwrap_or("");
        out.push_str(&format!(
            "- {} [{}] {} (project: {}, count: {}, last: {})\n  {}\n",
            short_id, level, title, project, count, last_seen, permalink
        ));
        if !status.is_empty() && status != "unresolved" {
            out.push_str(&format!("  status: {}\n", status));
        }
    }
    out
}

fn format_event(v: &serde_json::Value, issue_id: &str, host: &str) -> String {
    let event_id = v.get("eventID").and_then(|s| s.as_str()).unwrap_or("?");
    let date = v.get("dateCreated").and_then(|s| s.as_str()).unwrap_or("");
    let message = v.get("message").and_then(|s| s.as_str()).unwrap_or("");
    let platform = v.get("platform").and_then(|s| s.as_str()).unwrap_or("");
    // First exception value in entries → type + value + first frame.
    let exception_block = v
        .get("entries")
        .and_then(|e| e.as_array())
        .and_then(|arr| arr.iter().find(|x| x.get("type").and_then(|t| t.as_str()) == Some("exception")));
    let mut out = String::new();
    out.push_str(&format!("Event {} ({})\n", event_id, date));
    if !platform.is_empty() {
        out.push_str(&format!("Platform: {}\n", platform));
    }
    if !message.is_empty() {
        out.push_str(&format!("Message: {}\n", message));
    }
    if let Some(exc_block) = exception_block {
        if let Some(values) = exc_block
            .get("data")
            .and_then(|d| d.get("values"))
            .and_then(|v| v.as_array())
        {
            for (i, val) in values.iter().enumerate() {
                let typ = val.get("type").and_then(|s| s.as_str()).unwrap_or("");
                let valv = val.get("value").and_then(|s| s.as_str()).unwrap_or("");
                if !typ.is_empty() || !valv.is_empty() {
                    out.push_str(&format!(
                        "Exception {}: {}{}{}\n",
                        i + 1,
                        typ,
                        if valv.is_empty() { "" } else { ": " },
                        valv
                    ));
                }
                if let Some(frames) = val
                    .get("stacktrace")
                    .and_then(|s| s.get("frames"))
                    .and_then(|f| f.as_array())
                {
                    // Last 5 frames — most recent at the top per Sentry's convention.
                    let frames: Vec<&serde_json::Value> = frames.iter().rev().take(5).collect();
                    for f in frames.iter() {
                        let func = f.get("function").and_then(|s| s.as_str()).unwrap_or("?");
                        let file = f.get("filename").and_then(|s| s.as_str()).unwrap_or("?");
                        let line = f.get("lineNo").and_then(|s| s.as_u64()).unwrap_or(0);
                        out.push_str(&format!("  at {} ({}:{})\n", func, file, line));
                    }
                }
            }
        }
    }
    out.push_str(&format!("URL: {}/issues/{}/events/{}/\n", host, issue_id, event_id));
    out
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max])
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let creds = Creds::from_env()?;
    let server = Sentry::new(creds)?;
    let service = server.serve(stdio()).await.context("start MCP service over stdio")?;
    service.waiting().await?;
    Ok(())
}
