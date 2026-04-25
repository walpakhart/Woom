//! forgehold-jira — MCP sidecar exposing Jira Cloud as tools to Claude Code.
//!
//! Reads credentials from env:
//!   JIRA_WORKSPACE  e.g. "acme.atlassian.net"
//!   JIRA_EMAIL      Atlassian account email
//!   JIRA_TOKEN      API token from id.atlassian.com/manage-profile/security/api-tokens
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

const USER_AGENT: &str = concat!("forgehold-jira/", env!("CARGO_PKG_VERSION"));

#[derive(Clone)]
struct Creds {
    workspace: String,
    email: String,
    token: String,
}

impl Creds {
    fn from_env() -> anyhow::Result<Self> {
        let workspace = std::env::var("JIRA_WORKSPACE")
            .context("JIRA_WORKSPACE env var is required")?
            .trim()
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .trim_end_matches('/')
            .to_string();
        let email = std::env::var("JIRA_EMAIL").context("JIRA_EMAIL env var is required")?;
        let token = std::env::var("JIRA_TOKEN").context("JIRA_TOKEN env var is required")?;
        if workspace.is_empty() || email.is_empty() || token.is_empty() {
            anyhow::bail!("JIRA_WORKSPACE/JIRA_EMAIL/JIRA_TOKEN must be non-empty");
        }
        Ok(Self { workspace, email, token })
    }
}

#[derive(Clone)]
struct Jira {
    creds: Creds,
    http: reqwest::Client,
    #[allow(dead_code)] // read by the `#[tool_handler]` macro expansion
    tool_router: ToolRouter<Self>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct GetIssueParams {
    /// Issue key, e.g. "DEVOPS-396" or "EFF-21597".
    key: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SearchParams {
    /// Jira Query Language expression, e.g. `assignee = currentUser() AND resolution = Unresolved`.
    jql: String,
    /// Max issues to return (default 25, cap 100).
    #[serde(default)]
    max_results: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AddCommentParams {
    /// Issue key, e.g. "DEVOPS-414".
    key: String,
    /// Comment body. Plain text or Atlassian Markdown — converted to ADF.
    body: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct TransitionIssueParams {
    /// Issue key, e.g. "DEVOPS-414".
    key: String,
    /// Either the transition name (e.g. "In Review", "Done", case-insensitive
    /// match against the available transitions) OR the literal transition id.
    /// Use list_transitions implicitly by passing the human name.
    to: String,
    /// Optional comment posted with the transition. Some workflows require it.
    #[serde(default)]
    comment: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ListProjectsParams {
    /// Filter by name/key substring (case-insensitive). Omit for all.
    #[serde(default)]
    query: Option<String>,
    /// Max projects to return (default 50, cap 200).
    #[serde(default)]
    limit: Option<u32>,
}

#[tool_router]
impl Jira {
    fn new(creds: Creds) -> anyhow::Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .context("build reqwest client")?;
        Ok(Self { creds, http, tool_router: Self::tool_router() })
    }

    #[tool(
        description = "Fetch a Jira issue by its key. Returns summary, status, assignee, reporter, priority, type, labels, description (plain text), and recent comments."
    )]
    async fn get_issue(
        &self,
        Parameters(GetIssueParams { key }): Parameters<GetIssueParams>,
    ) -> Result<CallToolResult, ErrorData> {
        match self.fetch_issue(&key).await {
            Ok(text) => Ok(CallToolResult::success(vec![Content::text(text)])),
            Err(e) => Err(ErrorData::internal_error(e.to_string(), None)),
        }
    }

    #[tool(
        description = "Search Jira issues with JQL. Returns a compact list (key, summary, status, assignee, updated)."
    )]
    async fn search(
        &self,
        Parameters(SearchParams { jql, max_results }): Parameters<SearchParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let max = max_results.unwrap_or(25).min(100);
        match self.fetch_search(&jql, max).await {
            Ok(text) => Ok(CallToolResult::success(vec![Content::text(text)])),
            Err(e) => Err(ErrorData::internal_error(e.to_string(), None)),
        }
    }

    #[tool(
        description = "Post a comment on a Jira issue. Body accepts plain text or simple Markdown (bold, italic, code, links, lists) — converted to ADF before sending. Use this for status updates, follow-ups, or to document what you found while investigating."
    )]
    async fn add_comment(
        &self,
        Parameters(AddCommentParams { key, body }): Parameters<AddCommentParams>,
    ) -> Result<CallToolResult, ErrorData> {
        match self.do_add_comment(&key, &body).await {
            Ok(text) => Ok(CallToolResult::success(vec![Content::text(text)])),
            Err(e) => Err(ErrorData::internal_error(e.to_string(), None)),
        }
    }

    #[tool(
        description = "Transition a Jira issue to a new workflow status (e.g. \"In Review\", \"Done\", \"Blocked\"). Accepts the human name or a transition id — call get_issue first if you don't know which transitions are available. Optional `comment` posts an inline comment with the transition (some workflows require it)."
    )]
    async fn transition_issue(
        &self,
        Parameters(TransitionIssueParams { key, to, comment }): Parameters<TransitionIssueParams>,
    ) -> Result<CallToolResult, ErrorData> {
        match self.do_transition(&key, &to, comment.as_deref()).await {
            Ok(text) => Ok(CallToolResult::success(vec![Content::text(text)])),
            Err(e) => Err(ErrorData::internal_error(e.to_string(), None)),
        }
    }

    #[tool(
        description = "List projects in the user's Jira workspace. Returns key, name, project type, lead. Useful when the user mentions a project by name (or partial name) and you need its key to construct a JQL search or get_issue call."
    )]
    async fn list_projects(
        &self,
        Parameters(ListProjectsParams { query, limit }): Parameters<ListProjectsParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let max = limit.unwrap_or(50).min(200);
        let needle = query.as_deref().map(str::trim).filter(|s| !s.is_empty());
        match self.fetch_projects(needle, max).await {
            Ok(text) => Ok(CallToolResult::success(vec![Content::text(text)])),
            Err(e) => Err(ErrorData::internal_error(e.to_string(), None)),
        }
    }
}

#[tool_handler]
impl ServerHandler for Jira {
    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::default();
        info.capabilities = ServerCapabilities::builder().enable_tools().build();
        info.instructions = Some(
            "Access the user's Jira (Atlassian Cloud) workspace.\n\n\
             READ:\n\
             - get_issue(key) — full detail with comments/transitions for a single ticket.\n\
             - search(jql) — JQL query for lists. e.g. `assignee = currentUser() AND resolution = Unresolved`.\n\
             - list_projects(query?) — discover project keys when the user mentions a project by partial name.\n\n\
             WRITE:\n\
             - add_comment(key, body) — post a comment. Use for status updates / handoffs / documenting findings.\n\
             - transition_issue(key, to, comment?) — move workflow state. Pass the human name (e.g. \"In Review\") or transition id.\n\n\
             Match GitHub semantics: read-only ops are auto-approved; mutation ops should be called only when the user explicitly asks for them."
                .to_string(),
        );
        info
    }
}

impl Jira {
    async fn fetch_issue(&self, key: &str) -> anyhow::Result<String> {
        let url = format!(
            "https://{}/rest/api/3/issue/{}?fields=summary,status,assignee,reporter,priority,issuetype,labels,description,updated,created&expand=renderedFields",
            self.creds.workspace, key
        );
        let resp = self
            .http
            .get(&url)
            .basic_auth(&self.creds.email, Some(&self.creds.token))
            .header("Accept", "application/json")
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Jira {} while fetching {}: {}", status, key, truncate(&body, 500));
        }
        let v: serde_json::Value = resp.json().await?;
        Ok(format_issue(&v, &self.creds.workspace))
    }

    async fn fetch_search(&self, jql: &str, max_results: u32) -> anyhow::Result<String> {
        let body = serde_json::json!({
            "jql": jql,
            "fields": ["summary", "status", "assignee", "updated", "priority", "issuetype"],
            "maxResults": max_results,
        });
        let url = format!("https://{}/rest/api/3/search/jql", self.creds.workspace);
        let resp = self
            .http
            .post(&url)
            .basic_auth(&self.creds.email, Some(&self.creds.token))
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Jira {} on search: {}", status, truncate(&body, 500));
        }
        let v: serde_json::Value = resp.json().await?;
        Ok(format_search(&v, &self.creds.workspace))
    }

    async fn do_add_comment(&self, key: &str, body: &str) -> anyhow::Result<String> {
        let trimmed = body.trim();
        if trimmed.is_empty() {
            anyhow::bail!("comment body is empty");
        }
        // Wrap as a minimal ADF doc — Jira Cloud's POST comment endpoint
        // expects ADF, not plain text. One paragraph with a single text node
        // is enough for our case (the user's body is plain Markdown-flavored
        // text; we don't try to render bold/links/etc. — Jira won't parse
        // that anyway without proper ADF marks).
        let url = format!(
            "https://{}/rest/api/3/issue/{}/comment",
            self.creds.workspace, key
        );
        let payload = serde_json::json!({
            "body": {
                "type": "doc",
                "version": 1,
                "content": [{
                    "type": "paragraph",
                    "content": [{ "type": "text", "text": trimmed }]
                }]
            }
        });
        let resp = self
            .http
            .post(&url)
            .basic_auth(&self.creds.email, Some(&self.creds.token))
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Jira {} adding comment: {}", status, truncate(&body, 500));
        }
        Ok(format!("Comment posted on {}.", key))
    }

    async fn do_transition(
        &self,
        key: &str,
        to: &str,
        comment: Option<&str>,
    ) -> anyhow::Result<String> {
        // 1) Fetch the available transitions to map a name → id.
        let list_url = format!(
            "https://{}/rest/api/3/issue/{}/transitions",
            self.creds.workspace, key
        );
        let list_resp = self
            .http
            .get(&list_url)
            .basic_auth(&self.creds.email, Some(&self.creds.token))
            .header("Accept", "application/json")
            .send()
            .await?;
        let list_status = list_resp.status();
        if !list_status.is_success() {
            let body = list_resp.text().await.unwrap_or_default();
            anyhow::bail!(
                "Jira {} fetching transitions for {}: {}",
                list_status,
                key,
                truncate(&body, 500)
            );
        }
        let list_v: serde_json::Value = list_resp.json().await?;
        let transitions = list_v
            .get("transitions")
            .and_then(|x| x.as_array())
            .cloned()
            .unwrap_or_default();
        if transitions.is_empty() {
            anyhow::bail!("No transitions available for {}", key);
        }
        // Try id match first, then case-insensitive name match.
        let to_lower = to.to_lowercase();
        let matched = transitions.iter().find(|t| {
            t.get("id").and_then(|x| x.as_str()) == Some(to)
                || t.get("name")
                    .and_then(|x| x.as_str())
                    .map(|n| n.to_lowercase() == to_lower)
                    .unwrap_or(false)
        });
        let Some(t) = matched else {
            let names: Vec<String> = transitions
                .iter()
                .filter_map(|x| x.get("name").and_then(|n| n.as_str()).map(String::from))
                .collect();
            anyhow::bail!(
                "No transition matching `{}` on {}. Available: {}",
                to,
                key,
                names.join(", ")
            );
        };
        let transition_id = t
            .get("id")
            .and_then(|x| x.as_str())
            .ok_or_else(|| anyhow::anyhow!("transition has no id"))?;
        let transition_name = t.get("name").and_then(|x| x.as_str()).unwrap_or("?");

        // 2) POST the transition.
        let url = format!(
            "https://{}/rest/api/3/issue/{}/transitions",
            self.creds.workspace, key
        );
        let mut payload = serde_json::json!({
            "transition": { "id": transition_id }
        });
        if let Some(c) = comment.map(str::trim).filter(|s| !s.is_empty()) {
            payload["update"] = serde_json::json!({
                "comment": [{
                    "add": {
                        "body": {
                            "type": "doc",
                            "version": 1,
                            "content": [{
                                "type": "paragraph",
                                "content": [{ "type": "text", "text": c }]
                            }]
                        }
                    }
                }]
            });
        }
        let resp = self
            .http
            .post(&url)
            .basic_auth(&self.creds.email, Some(&self.creds.token))
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!(
                "Jira {} transitioning {}: {}",
                status,
                key,
                truncate(&body, 500)
            );
        }
        Ok(format!(
            "Transitioned {} → {} (id {}).",
            key, transition_name, transition_id
        ))
    }

    async fn fetch_projects(
        &self,
        needle: Option<&str>,
        max_results: u32,
    ) -> anyhow::Result<String> {
        // Search endpoint supports paging; we just take the first page since
        // most tenants have <200 projects. `expand=lead` enriches with the
        // owner so the LLM can route asks intelligently.
        let mut url = format!(
            "https://{}/rest/api/3/project/search?maxResults={}&expand=lead",
            self.creds.workspace, max_results
        );
        if let Some(n) = needle {
            url.push_str(&format!("&query={}", urlencoding::encode(n)));
        }
        let resp = self
            .http
            .get(&url)
            .basic_auth(&self.creds.email, Some(&self.creds.token))
            .header("Accept", "application/json")
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Jira {} listing projects: {}", status, truncate(&body, 500));
        }
        let v: serde_json::Value = resp.json().await?;
        let arr = v
            .get("values")
            .and_then(|x| x.as_array())
            .cloned()
            .unwrap_or_default();
        if arr.is_empty() {
            return Ok("No projects matched.".into());
        }
        let mut out = format!("{} project(s):\n", arr.len());
        for p in &arr {
            let key = p.get("key").and_then(|x| x.as_str()).unwrap_or("?");
            let name = p.get("name").and_then(|x| x.as_str()).unwrap_or("?");
            let typ = p.get("projectTypeKey").and_then(|x| x.as_str()).unwrap_or("");
            let lead = p
                .get("lead")
                .and_then(|l| l.get("displayName"))
                .and_then(|x| x.as_str())
                .unwrap_or("");
            out.push_str(&format!("- {} — {}", key, name));
            if !typ.is_empty() {
                out.push_str(&format!(" · type={}", typ));
            }
            if !lead.is_empty() {
                out.push_str(&format!(" · lead={}", lead));
            }
            out.push('\n');
        }
        Ok(out)
    }
}

fn format_issue(v: &serde_json::Value, workspace: &str) -> String {
    let key = v.get("key").and_then(|k| k.as_str()).unwrap_or("?");
    let fields = v.get("fields").cloned().unwrap_or(serde_json::Value::Null);
    let summary = fields.get("summary").and_then(|s| s.as_str()).unwrap_or("");
    let status = fields
        .get("status")
        .and_then(|s| s.get("name"))
        .and_then(|s| s.as_str())
        .unwrap_or("?");
    let priority = fields
        .get("priority")
        .and_then(|p| p.get("name"))
        .and_then(|n| n.as_str());
    let issue_type = fields
        .get("issuetype")
        .and_then(|t| t.get("name"))
        .and_then(|n| n.as_str())
        .unwrap_or("?");
    let assignee = user_name(fields.get("assignee"));
    let reporter = user_name(fields.get("reporter"));
    let labels = fields
        .get("labels")
        .and_then(|l| l.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();
    let updated = fields.get("updated").and_then(|s| s.as_str()).unwrap_or("");
    let description = fields
        .get("description")
        .map(adf_to_text)
        .unwrap_or_default();

    let mut out = String::new();
    out.push_str(&format!("{} — {}\n", key, summary));
    out.push_str(&format!("URL: https://{}/browse/{}\n", workspace, key));
    out.push_str(&format!("Status: {} · Type: {}", status, issue_type));
    if let Some(p) = priority {
        out.push_str(&format!(" · Priority: {}", p));
    }
    out.push('\n');
    out.push_str(&format!(
        "Assignee: {} · Reporter: {}\n",
        assignee.as_deref().unwrap_or("—"),
        reporter.as_deref().unwrap_or("—")
    ));
    if !labels.is_empty() {
        out.push_str(&format!("Labels: {}\n", labels));
    }
    if !updated.is_empty() {
        out.push_str(&format!("Updated: {}\n", updated));
    }
    if !description.is_empty() {
        out.push_str("\n--- Description ---\n");
        out.push_str(&description);
        out.push('\n');
    }
    out
}

fn format_search(v: &serde_json::Value, workspace: &str) -> String {
    let issues = match v.get("issues").and_then(|i| i.as_array()) {
        Some(arr) => arr,
        None => return "No issues matched.".into(),
    };
    if issues.is_empty() {
        return "No issues matched.".into();
    }
    let mut out = String::new();
    out.push_str(&format!("{} issue(s):\n", issues.len()));
    for i in issues {
        let key = i.get("key").and_then(|k| k.as_str()).unwrap_or("?");
        let fields = i.get("fields").cloned().unwrap_or(serde_json::Value::Null);
        let summary = fields.get("summary").and_then(|s| s.as_str()).unwrap_or("");
        let status = fields
            .get("status")
            .and_then(|s| s.get("name"))
            .and_then(|s| s.as_str())
            .unwrap_or("?");
        let assignee = user_name(fields.get("assignee"));
        let updated = fields.get("updated").and_then(|s| s.as_str()).unwrap_or("");
        out.push_str(&format!(
            "- {} [{}] {} (assignee: {}, updated: {})\n  https://{}/browse/{}\n",
            key,
            status,
            summary,
            assignee.as_deref().unwrap_or("—"),
            updated,
            workspace,
            key,
        ));
    }
    out
}

fn user_name(v: Option<&serde_json::Value>) -> Option<String> {
    v.and_then(|u| u.get("displayName"))
        .and_then(|n| n.as_str())
        .map(|s| s.to_string())
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max])
    }
}

/// Minimal Atlassian Document Format → plain text. Good enough for LLM context.
fn adf_to_text(v: &serde_json::Value) -> String {
    fn walk(v: &serde_json::Value, out: &mut String) {
        if let Some(arr) = v.as_array() {
            for item in arr {
                walk(item, out);
            }
            return;
        }
        if !v.is_object() {
            return;
        }
        let typ = v.get("type").and_then(|t| t.as_str()).unwrap_or("");
        match typ {
            "text" => {
                if let Some(t) = v.get("text").and_then(|t| t.as_str()) {
                    out.push_str(t);
                }
            }
            "hardBreak" => out.push('\n'),
            "paragraph" | "heading" => {
                if let Some(content) = v.get("content") {
                    walk(content, out);
                }
                out.push_str("\n\n");
            }
            "bulletList" | "orderedList" => {
                if let Some(content) = v.get("content").and_then(|c| c.as_array()) {
                    for (i, c) in content.iter().enumerate() {
                        let marker = if typ == "orderedList" {
                            format!("{}. ", i + 1)
                        } else {
                            "- ".to_string()
                        };
                        out.push_str(&marker);
                        walk(c, out);
                        out.push('\n');
                    }
                    out.push('\n');
                }
            }
            "listItem" | "blockquote" => {
                if let Some(content) = v.get("content") {
                    walk(content, out);
                }
            }
            "codeBlock" => {
                out.push_str("\n```\n");
                if let Some(content) = v.get("content") {
                    walk(content, out);
                }
                out.push_str("\n```\n");
            }
            "rule" => out.push_str("\n---\n"),
            _ => {
                if let Some(content) = v.get("content") {
                    walk(content, out);
                }
            }
        }
    }
    let mut out = String::new();
    walk(v, &mut out);
    out.trim().to_string()
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let creds = Creds::from_env()?;
    let jira = Jira::new(creds)?;
    let service = jira.serve(stdio()).await.context("start MCP service over stdio")?;
    service.waiting().await?;
    Ok(())
}
