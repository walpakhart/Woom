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
}

#[tool_handler]
impl ServerHandler for Jira {
    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::default();
        info.capabilities = ServerCapabilities::builder().enable_tools().build();
        info.instructions = Some(
            "Access the user's Jira (Atlassian Cloud) workspace. Use get_issue when the user references a ticket (e.g. 'DEVOPS-396'); use search with JQL for lists."
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
