//! forgehold-sentry — MCP sidecar exposing Sentry as tools to Claude Code.
//!
//! Reads credentials from env:
//!   SENTRY_HOST    e.g. "https://sentry.io" (or self-hosted base URL)
//!   SENTRY_ORG     organization slug, e.g. "efficiently-dev"
//!   SENTRY_TOKEN   personal auth token (event:read + project:read scopes)
//!
//! Speaks MCP over stdio. Spawned by Forgehold's main Tauri process, with creds
//! pulled from Keychain and passed as env vars. Never touches disk for creds.

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
        let host = std::env::var("SENTRY_HOST")
            .unwrap_or_else(|_| "https://sentry.io".to_string())
            .trim()
            .trim_end_matches('/')
            .to_string();
        let org = std::env::var("SENTRY_ORG")
            .context("SENTRY_ORG env var is required")?
            .trim()
            .to_string();
        let token = std::env::var("SENTRY_TOKEN").context("SENTRY_TOKEN env var is required")?;
        if host.is_empty() || org.is_empty() || token.is_empty() {
            anyhow::bail!("SENTRY_HOST/SENTRY_ORG/SENTRY_TOKEN must be non-empty");
        }
        Ok(Self { host, org, token })
    }
}

#[derive(Clone)]
struct Sentry {
    creds: Creds,
    http: reqwest::Client,
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct GetIssueParams {
    /// Sentry short id (e.g. "CATALOG-API-76") or numeric issue id.
    issue_id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SearchIssuesParams {
    /// Sentry search query, e.g. `is:unresolved level:error project:catalog-api`.
    /// Empty string returns the default unresolved feed for the org.
    query: String,
    /// Max issues to return (default 25, cap 100).
    #[serde(default)]
    limit: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct GetEventParams {
    /// Issue id or short id (e.g. "CATALOG-API-76") whose event to fetch.
    issue_id: String,
    /// Event id, or one of the special tokens "latest"/"oldest"/"recommended".
    /// Defaults to "latest" (the most recent occurrence).
    #[serde(default)]
    event_id: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct GetIssueTagsParams {
    /// Issue id or short id (e.g. "CATALOG-API-76") whose tag distribution to
    /// fetch. Tag distributions tell you which browser/OS/release/environment
    /// the error tends to fire in — gold for "is this OS-specific?".
    issue_id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ListEventsParams {
    /// Issue id or short id (e.g. "CATALOG-API-76") whose events to list.
    issue_id: String,
    /// Max events to return (default 10, cap 100).
    #[serde(default)]
    limit: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct UpdateIssueParams {
    /// Issue id or short id to mutate.
    issue_id: String,
    /// New status: `resolved` / `unresolved` / `ignored`. Omit to leave unchanged.
    #[serde(default)]
    status: Option<String>,
    /// Username or "me" to assign to. Omit to leave unchanged. Pass empty
    /// string `""` to explicitly unassign.
    #[serde(default)]
    assigned_to: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AddCommentParams {
    /// Issue id or short id to comment on.
    issue_id: String,
    /// Comment body in plain text or Markdown.
    body: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ListReleasesParams {
    /// Optional project slug (e.g. `catalog-api`). Omit/empty for org-wide
    /// release feed.
    #[serde(default)]
    project: Option<String>,
    /// Max releases to return (default 25, cap 100).
    #[serde(default)]
    limit: Option<u32>,
}

#[tool_router]
impl Sentry {
    fn new(creds: Creds) -> anyhow::Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .context("build reqwest client")?;
        Ok(Self { creds, http, tool_router: Self::tool_router() })
    }

    #[tool(
        description = "Fetch a Sentry issue by its short id (e.g. CATALOG-API-76) or numeric id. Returns title, level, status, project, culprit, type/value, event count, first/last seen, and permalink."
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
        description = "Search Sentry issues with the standard Sentry search syntax (e.g. `is:unresolved level:error project:foo`). Returns a compact list (short_id, title, level, status, project, last_seen).\n\nIMPORTANT — make ONE focused query, do not iterate. Sentry's search returns matching issues across the org in a single call. Do NOT re-run with different `project:` / `level:` / `is:` scopes — that re-pays the entire conversation context for the same answer. Examples:\n  - \"recent errors\" → ONE call: `is:unresolved level:error sort:date`.\n  - \"crashes mentioning auth\" → ONE call: `auth is:unresolved`.\n  - \"errors in project X this week\" → ONE call: `project:X is:unresolved age:-7d`.\nOnly broaden / narrow if the first result was empty or clearly missed the user's intent."
    )]
    async fn search_issues(
        &self,
        Parameters(SearchIssuesParams { query, limit }): Parameters<SearchIssuesParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let max = limit.unwrap_or(25).min(100);
        match self.fetch_search(&query, max).await {
            Ok(text) => Ok(CallToolResult::success(vec![Content::text(text)])),
            Err(e) => Err(ErrorData::internal_error(e.to_string(), None)),
        }
    }

    #[tool(
        description = "Fetch a single occurrence (event) for a Sentry issue, including its stack trace, breadcrumbs (the action sequence that led to the error), source context around each frame, request details, and user. `event_id` defaults to \"latest\" — the most recent occurrence. Use this when the user wants to see what actually happened."
    )]
    async fn get_event(
        &self,
        Parameters(GetEventParams { issue_id, event_id }): Parameters<GetEventParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let ev = event_id.unwrap_or_else(|| "latest".to_string());
        match self.fetch_event(&issue_id, &ev).await {
            Ok(text) => Ok(CallToolResult::success(vec![Content::text(text)])),
            Err(e) => Err(ErrorData::internal_error(e.to_string(), None)),
        }
    }

    #[tool(
        description = "Fetch tag distributions for a Sentry issue (browser, OS, release, environment, transaction, …). Each tag entry shows the top values + counts so you can answer 'is this only in Chrome on iOS?' or 'did this regress after release X?'."
    )]
    async fn get_issue_tags(
        &self,
        Parameters(GetIssueTagsParams { issue_id }): Parameters<GetIssueTagsParams>,
    ) -> Result<CallToolResult, ErrorData> {
        match self.fetch_issue_tags(&issue_id).await {
            Ok(text) => Ok(CallToolResult::success(vec![Content::text(text)])),
            Err(e) => Err(ErrorData::internal_error(e.to_string(), None)),
        }
    }

    #[tool(
        description = "List occurrences (events) for a Sentry issue. Returns event_id, timestamp, environment, release, user, message — useful for spotting a stable repro pattern across multiple events instead of relying on a single (possibly flaky) occurrence. Call once per issue — the default 10 events is enough to spot a pattern. Don't iterate with growing limits unless the first batch was clearly insufficient."
    )]
    async fn list_events(
        &self,
        Parameters(ListEventsParams { issue_id, limit }): Parameters<ListEventsParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let max = limit.unwrap_or(10).min(100);
        match self.fetch_events_list(&issue_id, max).await {
            Ok(text) => Ok(CallToolResult::success(vec![Content::text(text)])),
            Err(e) => Err(ErrorData::internal_error(e.to_string(), None)),
        }
    }

    #[tool(
        description = "Update a Sentry issue: change `status` (resolved/unresolved/ignored) and/or `assigned_to` (username or \"me\"; pass \"\" to unassign). Use this to close the loop after fixing — or to claim an issue before working on it."
    )]
    async fn update_issue(
        &self,
        Parameters(UpdateIssueParams { issue_id, status, assigned_to }): Parameters<UpdateIssueParams>,
    ) -> Result<CallToolResult, ErrorData> {
        match self.do_update_issue(&issue_id, status.as_deref(), assigned_to.as_deref()).await {
            Ok(text) => Ok(CallToolResult::success(vec![Content::text(text)])),
            Err(e) => Err(ErrorData::internal_error(e.to_string(), None)),
        }
    }

    #[tool(
        description = "Post a comment on a Sentry issue. Use this for handoff to a teammate, or to leave a note on what you found / did. Body accepts plain text or Markdown."
    )]
    async fn add_comment(
        &self,
        Parameters(AddCommentParams { issue_id, body }): Parameters<AddCommentParams>,
    ) -> Result<CallToolResult, ErrorData> {
        match self.do_add_comment(&issue_id, &body).await {
            Ok(text) => Ok(CallToolResult::success(vec![Content::text(text)])),
            Err(e) => Err(ErrorData::internal_error(e.to_string(), None)),
        }
    }

    #[tool(
        description = "List projects in the Sentry organization. Returns slug, name, platform, team. Use this to orient yourself when the org is unfamiliar or when scoping a search by `project:<slug>`."
    )]
    async fn list_projects(&self) -> Result<CallToolResult, ErrorData> {
        match self.fetch_projects().await {
            Ok(text) => Ok(CallToolResult::success(vec![Content::text(text)])),
            Err(e) => Err(ErrorData::internal_error(e.to_string(), None)),
        }
    }

    #[tool(
        description = "List recent releases for a project (or org-wide if `project` omitted). Returns version, dateCreated, lastDeploy, commit count. Use this to correlate 'when did this regress?' with the release timeline."
    )]
    async fn list_releases(
        &self,
        Parameters(ListReleasesParams { project, limit }): Parameters<ListReleasesParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let max = limit.unwrap_or(25).min(100);
        match self.fetch_releases(project.as_deref(), max).await {
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
        info.instructions = Some(
            "Access the user's Sentry organization. Triage flow:\n\
             - get_issue(short_id) for a quick summary\n\
             - get_event(short_id) for the latest stack trace + breadcrumbs + source context\n\
             - get_issue_tags(short_id) to see browser/OS/release/env distributions\n\
             - list_events(short_id) when one occurrence isn't enough\n\
             - search_issues(query) with Sentry syntax (e.g. `is:unresolved level:error project:foo`)\n\
             - list_projects / list_releases for orientation\n\
             - update_issue(short_id, status='resolved') / add_comment(short_id, body) to close the loop"
                .to_string(),
        );
        info
    }
}

impl Sentry {
    /// Resolve a short-id to its numeric Sentry issue id. Numeric inputs are
    /// returned as-is. The shortids endpoint is the canonical mapping —
    /// returns a 404 for unknown short-ids, which we surface verbatim.
    async fn resolve_issue_id(&self, id: &str) -> anyhow::Result<String> {
        let trimmed = id.trim();
        if trimmed.chars().all(|c| c.is_ascii_digit()) && !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
        let url = format!(
            "{}/api/0/organizations/{}/shortids/{}/",
            self.creds.host,
            urlencoding::encode(&self.creds.org),
            urlencoding::encode(trimmed),
        );
        let resp = self.authed_get(&url).send().await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!(
                "Sentry {} resolving short-id {}: {}",
                status,
                trimmed,
                truncate(&body, 500)
            );
        }
        let v: serde_json::Value = resp.json().await?;
        // The endpoint returns either { groupId: "...", group: { id: "..." } }
        // or { group: { id: "..." } } depending on Sentry version. Try both.
        let group_id = v
            .get("groupId")
            .and_then(|x| x.as_str())
            .map(|s| s.to_string())
            .or_else(|| {
                v.get("group")
                    .and_then(|g| g.get("id"))
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string())
            });
        group_id.ok_or_else(|| anyhow::anyhow!("Sentry returned no groupId for {}", trimmed))
    }

    fn authed_get(&self, url: &str) -> reqwest::RequestBuilder {
        self.http
            .get(url)
            .bearer_auth(&self.creds.token)
            .header("Accept", "application/json")
    }

    fn authed_put(&self, url: &str) -> reqwest::RequestBuilder {
        self.http
            .put(url)
            .bearer_auth(&self.creds.token)
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
    }

    fn authed_post(&self, url: &str) -> reqwest::RequestBuilder {
        self.http
            .post(url)
            .bearer_auth(&self.creds.token)
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
    }

    async fn fetch_issue(&self, id: &str) -> anyhow::Result<String> {
        // Two-step: short-id → numeric id → /api/0/issues/{id}/
        let numeric = self.resolve_issue_id(id).await?;
        let url = format!("{}/api/0/issues/{}/", self.creds.host, numeric);
        let resp = self.authed_get(&url).send().await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Sentry {} fetching issue {}: {}", status, id, truncate(&body, 500));
        }
        let v: serde_json::Value = resp.json().await?;
        Ok(format_issue(&v))
    }

    async fn fetch_search(&self, query: &str, limit: u32) -> anyhow::Result<String> {
        let q = if query.trim().is_empty() { "is:unresolved" } else { query };
        let url = format!(
            "{}/api/0/organizations/{}/issues/?query={}&limit={}",
            self.creds.host,
            urlencoding::encode(&self.creds.org),
            urlencoding::encode(q),
            limit
        );
        let resp = self.authed_get(&url).send().await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Sentry {} on search: {}", status, truncate(&body, 500));
        }
        let v: serde_json::Value = resp.json().await?;
        Ok(format_search(&v))
    }

    async fn fetch_event(&self, issue_id: &str, event_id: &str) -> anyhow::Result<String> {
        let numeric = self.resolve_issue_id(issue_id).await?;
        let ev = event_id.trim();
        let safe_ev = if ev.is_empty() { "latest" } else { ev };
        let url = format!(
            "{}/api/0/issues/{}/events/{}/",
            self.creds.host, numeric, safe_ev
        );
        let resp = self.authed_get(&url).send().await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!(
                "Sentry {} fetching event {} of issue {}: {}",
                status,
                safe_ev,
                issue_id,
                truncate(&body, 500)
            );
        }
        let v: serde_json::Value = resp.json().await?;
        Ok(format_event(&v))
    }

    async fn fetch_issue_tags(&self, issue_id: &str) -> anyhow::Result<String> {
        let numeric = self.resolve_issue_id(issue_id).await?;
        let url = format!("{}/api/0/issues/{}/tags/", self.creds.host, numeric);
        let resp = self.authed_get(&url).send().await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!(
                "Sentry {} fetching tags for {}: {}",
                status,
                issue_id,
                truncate(&body, 500)
            );
        }
        let v: serde_json::Value = resp.json().await?;
        Ok(format_issue_tags(&v))
    }

    async fn fetch_events_list(&self, issue_id: &str, limit: u32) -> anyhow::Result<String> {
        let numeric = self.resolve_issue_id(issue_id).await?;
        let url = format!(
            "{}/api/0/issues/{}/events/?limit={}",
            self.creds.host, numeric, limit
        );
        let resp = self.authed_get(&url).send().await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!(
                "Sentry {} listing events for {}: {}",
                status,
                issue_id,
                truncate(&body, 500)
            );
        }
        let v: serde_json::Value = resp.json().await?;
        Ok(format_events_list(&v))
    }

    async fn do_update_issue(
        &self,
        issue_id: &str,
        status: Option<&str>,
        assigned_to: Option<&str>,
    ) -> anyhow::Result<String> {
        let numeric = self.resolve_issue_id(issue_id).await?;
        let mut payload = serde_json::Map::new();
        if let Some(s) = status.map(|x| x.trim()).filter(|x| !x.is_empty()) {
            payload.insert("status".into(), serde_json::Value::String(s.to_string()));
        }
        if let Some(a) = assigned_to {
            // Empty string explicitly unassigns; non-empty becomes the value.
            payload.insert(
                "assignedTo".into(),
                serde_json::Value::String(a.trim().to_string()),
            );
        }
        if payload.is_empty() {
            anyhow::bail!("nothing to update — pass `status` and/or `assigned_to`");
        }
        let url = format!("{}/api/0/issues/{}/", self.creds.host, numeric);
        let resp = self
            .authed_put(&url)
            .json(&serde_json::Value::Object(payload.clone()))
            .send()
            .await?;
        let st = resp.status();
        if !st.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!(
                "Sentry {} updating issue {}: {}",
                st,
                issue_id,
                truncate(&body, 500)
            );
        }
        let v: serde_json::Value = resp.json().await?;
        let mut out = format!("Updated issue {}.\n", issue_id);
        if let Some(s) = v.get("status").and_then(|x| x.as_str()) {
            out.push_str(&format!("Status: {}\n", s));
        }
        if let Some(a) = v.get("assignedTo") {
            if a.is_null() {
                out.push_str("Assignee: —\n");
            } else if let Some(name) = a
                .get("name")
                .and_then(|x| x.as_str())
                .or_else(|| a.get("username").and_then(|x| x.as_str()))
            {
                out.push_str(&format!("Assignee: {}\n", name));
            }
        }
        Ok(out)
    }

    async fn do_add_comment(&self, issue_id: &str, body: &str) -> anyhow::Result<String> {
        if body.trim().is_empty() {
            anyhow::bail!("comment body is empty");
        }
        let numeric = self.resolve_issue_id(issue_id).await?;
        let url = format!("{}/api/0/issues/{}/comments/", self.creds.host, numeric);
        // Sentry's comments API wants `{ data: { text } }` per recent versions
        // (some self-hosted older versions accept top-level `text`); we send
        // the nested shape since it works on both.
        let payload = serde_json::json!({ "data": { "text": body } });
        let resp = self.authed_post(&url).json(&payload).send().await?;
        let st = resp.status();
        if !st.is_success() {
            let raw = resp.text().await.unwrap_or_default();
            anyhow::bail!(
                "Sentry {} adding comment on {}: {}",
                st,
                issue_id,
                truncate(&raw, 500)
            );
        }
        Ok(format!("Comment posted on {}.", issue_id))
    }

    async fn fetch_projects(&self) -> anyhow::Result<String> {
        let url = format!(
            "{}/api/0/organizations/{}/projects/",
            self.creds.host,
            urlencoding::encode(&self.creds.org)
        );
        let resp = self.authed_get(&url).send().await?;
        let st = resp.status();
        if !st.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Sentry {} listing projects: {}", st, truncate(&body, 500));
        }
        let v: serde_json::Value = resp.json().await?;
        Ok(format_projects(&v))
    }

    async fn fetch_releases(&self, project: Option<&str>, limit: u32) -> anyhow::Result<String> {
        let mut url = format!(
            "{}/api/0/organizations/{}/releases/?per_page={}",
            self.creds.host,
            urlencoding::encode(&self.creds.org),
            limit
        );
        if let Some(p) = project.map(|s| s.trim()).filter(|s| !s.is_empty()) {
            // The releases endpoint accepts `project=<id|slug>` as a filter.
            url.push_str(&format!("&project={}", urlencoding::encode(p)));
        }
        let resp = self.authed_get(&url).send().await?;
        let st = resp.status();
        if !st.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Sentry {} listing releases: {}", st, truncate(&body, 500));
        }
        let v: serde_json::Value = resp.json().await?;
        Ok(format_releases(&v))
    }
}

fn format_issue(v: &serde_json::Value) -> String {
    let short_id = v.get("shortId").and_then(|x| x.as_str()).unwrap_or("?");
    let title = v.get("title").and_then(|x| x.as_str()).unwrap_or("");
    let level = v.get("level").and_then(|x| x.as_str()).unwrap_or("?");
    let status = v.get("status").and_then(|x| x.as_str()).unwrap_or("?");
    let culprit = v.get("culprit").and_then(|x| x.as_str()).unwrap_or("");
    let permalink = v.get("permalink").and_then(|x| x.as_str()).unwrap_or("");
    let count = v.get("count").and_then(|x| x.as_str()).unwrap_or("?");
    let user_count = v
        .get("userCount")
        .and_then(|x| x.as_u64())
        .map(|n| n.to_string())
        .unwrap_or_else(|| "?".to_string());
    let first_seen = v.get("firstSeen").and_then(|x| x.as_str()).unwrap_or("");
    let last_seen = v.get("lastSeen").and_then(|x| x.as_str()).unwrap_or("");
    let project_slug = v
        .get("project")
        .and_then(|p| p.get("slug"))
        .and_then(|x| x.as_str())
        .unwrap_or("?");
    let project_name = v
        .get("project")
        .and_then(|p| p.get("name"))
        .and_then(|x| x.as_str())
        .unwrap_or("");
    let meta_type = v
        .get("metadata")
        .and_then(|m| m.get("type"))
        .and_then(|x| x.as_str())
        .unwrap_or("");
    let meta_value = v
        .get("metadata")
        .and_then(|m| m.get("value"))
        .and_then(|x| x.as_str())
        .unwrap_or("");

    let mut out = String::new();
    out.push_str(&format!("{} — {}\n", short_id, title));
    if !permalink.is_empty() {
        out.push_str(&format!("URL: {}\n", permalink));
    }
    out.push_str(&format!("Level: {} · Status: {}", level, status));
    out.push_str(&format!(" · Project: {}", project_slug));
    if !project_name.is_empty() && project_name != project_slug {
        out.push_str(&format!(" ({})", project_name));
    }
    out.push('\n');
    out.push_str(&format!("Events: {} · Users affected: {}\n", count, user_count));
    if !first_seen.is_empty() || !last_seen.is_empty() {
        out.push_str(&format!("First seen: {} · Last seen: {}\n", first_seen, last_seen));
    }
    if !culprit.is_empty() {
        out.push_str(&format!("Culprit: {}\n", culprit));
    }
    if !meta_type.is_empty() || !meta_value.is_empty() {
        out.push_str("\n--- Error ---\n");
        if !meta_type.is_empty() {
            out.push_str(&format!("{}: ", meta_type));
        }
        out.push_str(meta_value);
        out.push('\n');
    }
    out
}

fn format_search(v: &serde_json::Value) -> String {
    let arr = match v.as_array() {
        Some(a) => a,
        // Some Sentry endpoints wrap in `{ data: [...] }` — handle that too.
        None => match v.get("data").and_then(|d| d.as_array()) {
            Some(a) => a,
            None => return "No issues matched.".into(),
        },
    };
    if arr.is_empty() {
        return "No issues matched.".into();
    }
    let mut out = String::new();
    out.push_str(&format!("{} issue(s):\n", arr.len()));
    for i in arr {
        let short_id = i.get("shortId").and_then(|x| x.as_str()).unwrap_or("?");
        let title = i.get("title").and_then(|x| x.as_str()).unwrap_or("");
        let level = i.get("level").and_then(|x| x.as_str()).unwrap_or("?");
        let status = i.get("status").and_then(|x| x.as_str()).unwrap_or("?");
        let project = i
            .get("project")
            .and_then(|p| p.get("slug"))
            .and_then(|x| x.as_str())
            .unwrap_or("?");
        let last_seen = i.get("lastSeen").and_then(|x| x.as_str()).unwrap_or("");
        let count = i.get("count").and_then(|x| x.as_str()).unwrap_or("?");
        let permalink = i.get("permalink").and_then(|x| x.as_str()).unwrap_or("");
        out.push_str(&format!(
            "- {} [{}/{}] {} · project={} · events={} · last={}\n",
            short_id, level, status, title, project, count, last_seen
        ));
        if !permalink.is_empty() {
            out.push_str(&format!("  {}\n", permalink));
        }
    }
    out
}

fn format_event(v: &serde_json::Value) -> String {
    let event_id = v.get("eventID").and_then(|x| x.as_str()).unwrap_or("?");
    let title = v.get("title").and_then(|x| x.as_str()).unwrap_or("");
    let date = v.get("dateCreated").and_then(|x| x.as_str()).unwrap_or("");
    let platform = v.get("platform").and_then(|x| x.as_str()).unwrap_or("");
    let culprit = v.get("culprit").and_then(|x| x.as_str()).unwrap_or("");
    let level = v.get("level").and_then(|x| x.as_str()).unwrap_or("?");
    let environment = v
        .get("environment")
        .and_then(|x| x.as_str())
        .unwrap_or("");
    let release = v
        .get("release")
        .and_then(|r| r.get("version"))
        .and_then(|x| x.as_str())
        .unwrap_or("");
    let user_email = v
        .get("user")
        .and_then(|u| u.get("email"))
        .and_then(|x| x.as_str())
        .unwrap_or("");
    let user_id = v
        .get("user")
        .and_then(|u| u.get("id"))
        .and_then(|x| x.as_str())
        .unwrap_or("");

    let mut out = String::new();
    out.push_str(&format!("Event {} — {}\n", event_id, title));
    out.push_str(&format!("Level: {} · Platform: {}\n", level, platform));
    if !date.is_empty() {
        out.push_str(&format!("When: {}\n", date));
    }
    if !environment.is_empty() {
        out.push_str(&format!("Environment: {}\n", environment));
    }
    if !release.is_empty() {
        out.push_str(&format!("Release: {}\n", release));
    }
    if !culprit.is_empty() {
        out.push_str(&format!("Culprit: {}\n", culprit));
    }
    if !user_email.is_empty() || !user_id.is_empty() {
        out.push_str(&format!(
            "User: {}\n",
            if !user_email.is_empty() { user_email } else { user_id }
        ));
    }

    // Walk `entries[]` for exception + message + request blocks. Sentry's
    // event payload is "entry" oriented — different platforms use different
    // entry types — so we just iterate and pull the ones useful for an LLM.
    if let Some(entries) = v.get("entries").and_then(|e| e.as_array()) {
        for entry in entries {
            let typ = entry.get("type").and_then(|x| x.as_str()).unwrap_or("");
            match typ {
                "message" => {
                    if let Some(msg) = entry
                        .get("data")
                        .and_then(|d| d.get("formatted"))
                        .and_then(|x| x.as_str())
                    {
                        out.push_str("\n--- Message ---\n");
                        out.push_str(msg);
                        out.push('\n');
                    }
                }
                "exception" => {
                    out.push_str("\n--- Exception ---\n");
                    let values = entry
                        .get("data")
                        .and_then(|d| d.get("values"))
                        .and_then(|x| x.as_array());
                    if let Some(values) = values {
                        for ex in values {
                            let etype = ex.get("type").and_then(|x| x.as_str()).unwrap_or("");
                            let evalue = ex.get("value").and_then(|x| x.as_str()).unwrap_or("");
                            out.push_str(&format!("{}: {}\n", etype, evalue));
                            // Stacktrace frames — show innermost first (Sentry
                            // returns them outer-first, so reverse).
                            if let Some(frames) = ex
                                .get("stacktrace")
                                .and_then(|s| s.get("frames"))
                                .and_then(|x| x.as_array())
                            {
                                let mut frames: Vec<&serde_json::Value> =
                                    frames.iter().collect();
                                frames.reverse();
                                let cap = frames.len().min(20);
                                for f in frames.iter().take(cap) {
                                    let func = f
                                        .get("function")
                                        .and_then(|x| x.as_str())
                                        .unwrap_or("?");
                                    let file = f
                                        .get("filename")
                                        .and_then(|x| x.as_str())
                                        .unwrap_or("?");
                                    let line = f
                                        .get("lineNo")
                                        .and_then(|x| x.as_u64())
                                        .map(|n| n.to_string())
                                        .unwrap_or_else(|| "?".to_string());
                                    let in_app =
                                        f.get("inApp").and_then(|x| x.as_bool()).unwrap_or(false);
                                    let mark = if in_app { "▸" } else { " " };
                                    out.push_str(&format!(
                                        "  {} {} at {}:{}\n",
                                        mark, func, file, line
                                    ));
                                    // Source context — pre/contextLine/post that
                                    // Sentry stores per-frame for in-app frames.
                                    // Skipping for non-in-app frames to keep the
                                    // output tight; the LLM rarely cares about
                                    // node_modules internals.
                                    if in_app {
                                        let line_n = f
                                            .get("lineNo")
                                            .and_then(|x| x.as_u64())
                                            .unwrap_or(0);
                                        let pre = f
                                            .get("preContext")
                                            .and_then(|x| x.as_array());
                                        let ctx = f
                                            .get("contextLine")
                                            .and_then(|x| x.as_str());
                                        let post = f
                                            .get("postContext")
                                            .and_then(|x| x.as_array());
                                        if pre.is_some() || ctx.is_some() || post.is_some() {
                                            if let Some(arr) = pre {
                                                let start = line_n.saturating_sub(arr.len() as u64);
                                                for (i, p) in arr.iter().enumerate() {
                                                    if let Some(s) = p.as_str() {
                                                        out.push_str(&format!(
                                                            "        {:>5} | {}\n",
                                                            start + i as u64,
                                                            s.trim_end()
                                                        ));
                                                    }
                                                }
                                            }
                                            if let Some(s) = ctx {
                                                out.push_str(&format!(
                                                    "        {:>5} > {}\n",
                                                    line_n,
                                                    s.trim_end()
                                                ));
                                            }
                                            if let Some(arr) = post {
                                                for (i, p) in arr.iter().enumerate() {
                                                    if let Some(s) = p.as_str() {
                                                        out.push_str(&format!(
                                                            "        {:>5} | {}\n",
                                                            line_n + 1 + i as u64,
                                                            s.trim_end()
                                                        ));
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                                if frames.len() > cap {
                                    out.push_str(&format!(
                                        "  … and {} more frame(s)\n",
                                        frames.len() - cap
                                    ));
                                }
                            }
                        }
                    }
                }
                "request" => {
                    let url = entry
                        .get("data")
                        .and_then(|d| d.get("url"))
                        .and_then(|x| x.as_str())
                        .unwrap_or("");
                    let method = entry
                        .get("data")
                        .and_then(|d| d.get("method"))
                        .and_then(|x| x.as_str())
                        .unwrap_or("");
                    if !url.is_empty() {
                        out.push_str(&format!("\n--- Request ---\n{} {}\n", method, url));
                    }
                }
                "breadcrumbs" => {
                    // Sentry orders breadcrumbs oldest → newest. Show the last
                    // ~20 (the path that led to the error). Each entry has
                    // `category`, `level`, `message`/`data`, `timestamp`.
                    let crumbs = entry
                        .get("data")
                        .and_then(|d| d.get("values"))
                        .and_then(|x| x.as_array());
                    if let Some(values) = crumbs {
                        if !values.is_empty() {
                            out.push_str("\n--- Breadcrumbs (most recent last) ---\n");
                            let cap = 20usize;
                            let start = values.len().saturating_sub(cap);
                            for c in &values[start..] {
                                let ts = c
                                    .get("timestamp")
                                    .and_then(|x| x.as_str())
                                    .unwrap_or("");
                                let cat = c
                                    .get("category")
                                    .and_then(|x| x.as_str())
                                    .unwrap_or("?");
                                let lvl = c
                                    .get("level")
                                    .and_then(|x| x.as_str())
                                    .unwrap_or("info");
                                let msg = c
                                    .get("message")
                                    .and_then(|x| x.as_str())
                                    .map(|s| s.to_string())
                                    .or_else(|| {
                                        c.get("data").map(|d| {
                                            // Compact one-line render of the
                                            // data dict — works for arbitrary
                                            // shapes (xhr, navigation, console).
                                            let pairs: Vec<String> = d
                                                .as_object()
                                                .map(|m| {
                                                    m.iter()
                                                        .map(|(k, v)| format!("{}={}", k, v))
                                                        .collect()
                                                })
                                                .unwrap_or_default();
                                            pairs.join(" ")
                                        })
                                    })
                                    .unwrap_or_default();
                                out.push_str(&format!(
                                    "  [{}] {} · {}: {}\n",
                                    lvl, ts, cat, msg
                                ));
                            }
                            if values.len() > cap {
                                out.push_str(&format!(
                                    "  … and {} earlier crumb(s) omitted\n",
                                    values.len() - cap
                                ));
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
    out
}

fn format_issue_tags(v: &serde_json::Value) -> String {
    let arr = match v.as_array() {
        Some(a) => a,
        None => return "No tag distributions returned.".into(),
    };
    if arr.is_empty() {
        return "No tag distributions for this issue.".into();
    }
    let mut out = String::new();
    out.push_str(&format!("{} tag dimension(s):\n", arr.len()));
    for tag in arr {
        let key = tag.get("key").and_then(|x| x.as_str()).unwrap_or("?");
        let total = tag
            .get("totalValues")
            .and_then(|x| x.as_u64())
            .map(|n| n.to_string())
            .unwrap_or_else(|| "?".to_string());
        let top = tag
            .get("topValues")
            .and_then(|x| x.as_array())
            .map(|arr| arr.iter().take(5).collect::<Vec<_>>())
            .unwrap_or_default();
        out.push_str(&format!("  {} (total: {})\n", key, total));
        for v in top {
            let name = v
                .get("name")
                .and_then(|x| x.as_str())
                .or_else(|| v.get("value").and_then(|x| x.as_str()))
                .unwrap_or("?");
            let count = v
                .get("count")
                .and_then(|x| x.as_u64())
                .map(|n| n.to_string())
                .unwrap_or_else(|| "?".to_string());
            out.push_str(&format!("    - {}  ({})\n", name, count));
        }
    }
    out
}

fn format_events_list(v: &serde_json::Value) -> String {
    let arr = match v.as_array() {
        Some(a) => a,
        None => return "No events.".into(),
    };
    if arr.is_empty() {
        return "No events.".into();
    }
    let mut out = String::new();
    out.push_str(&format!("{} event(s):\n", arr.len()));
    for e in arr {
        let id = e.get("eventID").and_then(|x| x.as_str()).unwrap_or("?");
        let date = e.get("dateCreated").and_then(|x| x.as_str()).unwrap_or("");
        let env = e.get("environment").and_then(|x| x.as_str()).unwrap_or("");
        let title = e.get("title").and_then(|x| x.as_str()).unwrap_or("");
        let release = e
            .get("release")
            .and_then(|r| r.get("version"))
            .and_then(|x| x.as_str())
            .unwrap_or("");
        let user = e
            .get("user")
            .and_then(|u| u.get("email"))
            .and_then(|x| x.as_str())
            .or_else(|| {
                e.get("user")
                    .and_then(|u| u.get("id"))
                    .and_then(|x| x.as_str())
            })
            .unwrap_or("");
        out.push_str(&format!("- {} · {}", id, date));
        if !env.is_empty() {
            out.push_str(&format!(" · env={}", env));
        }
        if !release.is_empty() {
            out.push_str(&format!(" · release={}", release));
        }
        if !user.is_empty() {
            out.push_str(&format!(" · user={}", user));
        }
        out.push('\n');
        if !title.is_empty() {
            out.push_str(&format!("  {}\n", title));
        }
    }
    out
}

fn format_projects(v: &serde_json::Value) -> String {
    let arr = match v.as_array() {
        Some(a) => a,
        None => return "No projects.".into(),
    };
    if arr.is_empty() {
        return "No projects in this organization.".into();
    }
    let mut out = String::new();
    out.push_str(&format!("{} project(s):\n", arr.len()));
    for p in arr {
        let slug = p.get("slug").and_then(|x| x.as_str()).unwrap_or("?");
        let name = p.get("name").and_then(|x| x.as_str()).unwrap_or("");
        let platform = p.get("platform").and_then(|x| x.as_str()).unwrap_or("");
        let teams = p
            .get("teams")
            .and_then(|x| x.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|t| t.get("slug").and_then(|x| x.as_str()))
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
        out.push_str(&format!("- {} ({})", slug, name));
        if !platform.is_empty() {
            out.push_str(&format!(" · {}", platform));
        }
        if !teams.is_empty() {
            out.push_str(&format!(" · teams: {}", teams));
        }
        out.push('\n');
    }
    out
}

fn format_releases(v: &serde_json::Value) -> String {
    let arr = match v.as_array() {
        Some(a) => a,
        None => return "No releases.".into(),
    };
    if arr.is_empty() {
        return "No releases.".into();
    }
    let mut out = String::new();
    out.push_str(&format!("{} release(s):\n", arr.len()));
    for r in arr {
        let version = r.get("version").and_then(|x| x.as_str()).unwrap_or("?");
        let short = r
            .get("shortVersion")
            .and_then(|x| x.as_str())
            .unwrap_or(version);
        let date = r.get("dateCreated").and_then(|x| x.as_str()).unwrap_or("");
        let last_deploy = r
            .get("lastDeploy")
            .and_then(|d| d.get("dateFinished"))
            .and_then(|x| x.as_str())
            .or_else(|| {
                r.get("lastDeploy")
                    .and_then(|d| d.get("dateStarted"))
                    .and_then(|x| x.as_str())
            })
            .unwrap_or("");
        let env = r
            .get("lastDeploy")
            .and_then(|d| d.get("environment"))
            .and_then(|x| x.as_str())
            .unwrap_or("");
        let commit_count = r
            .get("commitCount")
            .and_then(|x| x.as_u64())
            .map(|n| n.to_string())
            .unwrap_or_else(|| "?".to_string());
        out.push_str(&format!("- {} · created={}", short, date));
        if !last_deploy.is_empty() {
            out.push_str(&format!(" · deployed={}", last_deploy));
            if !env.is_empty() {
                out.push_str(&format!("({})", env));
            }
        }
        out.push_str(&format!(" · commits={}\n", commit_count));
    }
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
    let sentry = Sentry::new(creds)?;
    let service = sentry.serve(stdio()).await.context("start MCP service over stdio")?;
    service.waiting().await?;
    Ok(())
}
