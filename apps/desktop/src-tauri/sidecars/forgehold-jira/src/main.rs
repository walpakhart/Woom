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

mod md_to_adf;
use md_to_adf::markdown_to_adf;

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

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct CreateIssueParams {
    /// Project key, e.g. "DEVOPS". Call list_projects first if you need to
    /// translate a human project name into its key.
    project_key: String,
    /// Issue type name as it appears in Jira (case-insensitive: "Task",
    /// "Bug", "Story", "Sub-task", "Epic", or any custom type the project
    /// has). Defaults to "Task" when omitted.
    #[serde(default)]
    issue_type: Option<String>,
    /// One-line summary (the ticket title).
    summary: String,
    /// Optional plain-text description. Wrapped as a single ADF paragraph;
    /// keep formatting simple — line breaks become hard breaks.
    #[serde(default)]
    description: Option<String>,
    /// Optional assignee Atlassian accountId. Resolve a name → accountId
    /// via list_assignable_users(project_key, query?). Omit to leave
    /// unassigned.
    #[serde(default)]
    assignee_account_id: Option<String>,
    /// Optional sprint id (numeric). Resolve a sprint name → id via
    /// list_sprints(project_key). The issue is created first, then added
    /// to the sprint via a follow-up Agile API call.
    #[serde(default)]
    sprint_id: Option<u64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ListAssignableUsersParams {
    /// Project key — assignability is project-scoped, so this is required.
    project_key: String,
    /// Optional substring to filter by displayName / email (case-insensitive).
    /// Omit to list all assignable users (capped at 200).
    #[serde(default)]
    query: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct UpdateIssueParams {
    /// Issue key, e.g. "DEVOPS-452".
    key: String,
    /// New summary (title). Omit / null to leave unchanged.
    #[serde(default)]
    summary: Option<String>,
    /// New description in Markdown. Omit to leave unchanged. Pass empty
    /// string to clear the field.
    #[serde(default)]
    description: Option<String>,
    /// New assignee accountId. Omit to leave unchanged. Pass `"unassign"`
    /// (literal string) to remove the assignee.
    #[serde(default)]
    assignee_account_id: Option<String>,
    /// Sprint to move the issue into. Omit to leave unchanged. Pass 0 or
    /// `null`-ish to remove from sprint (best-effort: Jira sometimes
    /// requires moving to backlog explicitly).
    #[serde(default)]
    sprint_id: Option<u64>,
    /// Replace the issue's labels with this list. Omit to leave labels
    /// unchanged. Pass `[]` to clear.
    #[serde(default)]
    labels: Option<Vec<String>>,
    /// Issue priority by name (e.g. "Highest", "High", "Medium", "Low",
    /// "Lowest"). Project-specific — the names usually match Jira's
    /// defaults but can be customised. Omit to leave unchanged.
    #[serde(default)]
    priority: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SetPriorityParams {
    /// Issue key, e.g. "DEVOPS-452".
    key: String,
    /// Priority name. Pass an empty string or omit to clear (sets to
    /// the project default).
    priority: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct WorklogParams {
    /// Issue key, e.g. "DEVOPS-452".
    key: String,
    /// Time spent in Jira's shorthand: "1h 30m", "45m", "2d 4h". The
    /// Jira API parses this server-side using the workspace's standard
    /// hours-per-day setting.
    time_spent: String,
    /// Optional comment in markdown — converted to ADF for the worklog.
    #[serde(default)]
    comment: Option<String>,
    /// ISO 8601 start time (e.g. "2026-05-04T10:00:00.000+0300"). Omit
    /// to use "now" on Jira's side.
    #[serde(default)]
    started: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ListWorklogsParams {
    key: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ListBoardsParams {
    /// Project key to filter by. Omit to list all accessible boards
    /// (capped at 50 — Agile API page size).
    #[serde(default)]
    project_key: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ListStatusesParams {
    /// Project key. Statuses are project-scoped — the same workflow
    /// can have different status sets across projects.
    project_key: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ListIssueTypesParams {
    project_key: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ListSprintsParams {
    /// Project key. The sidecar resolves the project's first scrum board
    /// and returns its sprints — covers the common single-board case
    /// without forcing the agent to know board ids.
    project_key: String,
    /// Sprint state filter: `active`, `future`, `closed`, or `all`. Defaults
    /// to `active,future` so the agent gets the sprints worth filing into.
    #[serde(default)]
    state: Option<String>,
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
        description = "Search Jira issues with JQL. Returns a compact list (key, summary, status, assignee, updated). JQL combines project / status / assignee / sprint / full-text in one call — see the search-discipline block in your system context for canonical patterns."
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

    #[tool(
        description = "List users assignable on issues in a given project. Returns one line per user: accountId, displayName, email. Use this to resolve a human name (\"@Nikolay\", \"assignee=me\", \"give it to Petya\") into the accountId that create_issue / set_assignee require. `query` filters by name/email substring."
    )]
    async fn list_assignable_users(
        &self,
        Parameters(ListAssignableUsersParams { project_key, query }): Parameters<ListAssignableUsersParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let needle = query.as_deref().map(str::trim).filter(|s| !s.is_empty());
        match self.fetch_assignable_users(&project_key, needle).await {
            Ok(text) => Ok(CallToolResult::success(vec![Content::text(text)])),
            Err(e) => Err(ErrorData::internal_error(e.to_string(), None)),
        }
    }

    #[tool(
        description = "List sprints on a project's first scrum board. Returns one line per sprint: id, name, state (active/future/closed). Use this to translate a human sprint name (\"Sprint 160\", \"current sprint\") into the numeric id that create_issue's sprint_id parameter accepts. `state` filters: `active`, `future`, `closed`, `all` (default = active+future)."
    )]
    async fn list_sprints(
        &self,
        Parameters(ListSprintsParams { project_key, state }): Parameters<ListSprintsParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let s = state.as_deref().map(str::trim).filter(|s| !s.is_empty()).unwrap_or("active,future");
        match self.fetch_sprints(&project_key, s).await {
            Ok(text) => Ok(CallToolResult::success(vec![Content::text(text)])),
            Err(e) => Err(ErrorData::internal_error(e.to_string(), None)),
        }
    }

    #[tool(
        description = "Create a new Jira issue. Required: project_key + summary. Optional: issue_type (default Task), description (Markdown — converted to ADF: headings, lists, code blocks, links, bold/italic/strike all preserved), assignee_account_id (resolve via list_assignable_users), sprint_id (resolve via list_sprints — the issue is added to the sprint via a follow-up Agile API call after the create succeeds). Returns the new issue key + browse URL. Use this whenever the user asks to file/open/create a ticket."
    )]
    async fn create_issue(
        &self,
        Parameters(CreateIssueParams {
            project_key,
            issue_type,
            summary,
            description,
            assignee_account_id,
            sprint_id,
        }): Parameters<CreateIssueParams>,
    ) -> Result<CallToolResult, ErrorData> {
        match self
            .do_create_issue(
                &project_key,
                issue_type.as_deref().unwrap_or("Task"),
                &summary,
                description.as_deref(),
                assignee_account_id.as_deref(),
                sprint_id,
            )
            .await
        {
            Ok(text) => Ok(CallToolResult::success(vec![Content::text(text)])),
            Err(e) => Err(ErrorData::internal_error(e.to_string(), None)),
        }
    }

    #[tool(
        description = "Update an existing Jira issue. Pass `key` plus any subset of: summary, description (Markdown — replaces existing body, full ADF conversion), assignee_account_id (or the literal string \"unassign\" to clear), sprint_id (numeric — moves issue into that sprint), labels (full replace; pass [] to clear), priority (name like \"High\"). Omitted fields are left unchanged. Use this when the user asks to fix the description, reassign, change a title, or move between sprints."
    )]
    async fn update_issue(
        &self,
        Parameters(UpdateIssueParams {
            key,
            summary,
            description,
            assignee_account_id,
            sprint_id,
            labels,
            priority,
        }): Parameters<UpdateIssueParams>,
    ) -> Result<CallToolResult, ErrorData> {
        match self
            .do_update_issue(
                &key,
                summary.as_deref(),
                description.as_deref(),
                assignee_account_id.as_deref(),
                sprint_id,
                labels.as_deref(),
                priority.as_deref(),
            )
            .await
        {
            Ok(text) => Ok(CallToolResult::success(vec![Content::text(text)])),
            Err(e) => Err(ErrorData::internal_error(e.to_string(), None)),
        }
    }

    #[tool(
        description = "Set an issue's priority by name (e.g. \"High\", \"Medium\"). Thin wrapper around update_issue's priority field — exists separately because \"set this to high\" is a common one-shot the agent shouldn't need to plumb through update_issue's larger param surface."
    )]
    async fn set_priority(
        &self,
        Parameters(SetPriorityParams { key, priority }): Parameters<SetPriorityParams>,
    ) -> Result<CallToolResult, ErrorData> {
        match self
            .do_update_issue(&key, None, None, None, None, None, Some(&priority))
            .await
        {
            Ok(text) => Ok(CallToolResult::success(vec![Content::text(text)])),
            Err(e) => Err(ErrorData::internal_error(e.to_string(), None)),
        }
    }

    #[tool(
        description = "Log time on a Jira issue. `time_spent` uses Jira's shorthand (\"1h 30m\", \"45m\", \"2d 4h\"). `comment` is optional markdown — converted to ADF for the worklog. `started` is optional ISO-8601 (defaults to \"now\")."
    )]
    async fn add_worklog(
        &self,
        Parameters(WorklogParams { key, time_spent, comment, started }): Parameters<WorklogParams>,
    ) -> Result<CallToolResult, ErrorData> {
        match self
            .do_add_worklog(&key, &time_spent, comment.as_deref(), started.as_deref())
            .await
        {
            Ok(text) => Ok(CallToolResult::success(vec![Content::text(text)])),
            Err(e) => Err(ErrorData::internal_error(e.to_string(), None)),
        }
    }

    #[tool(
        description = "List worklogs (time entries) on a Jira issue. Returns each entry's author, time-spent, started timestamp, and comment text."
    )]
    async fn list_worklogs(
        &self,
        Parameters(ListWorklogsParams { key }): Parameters<ListWorklogsParams>,
    ) -> Result<CallToolResult, ErrorData> {
        match self.do_list_worklogs(&key).await {
            Ok(text) => Ok(CallToolResult::success(vec![Content::text(text)])),
            Err(e) => Err(ErrorData::internal_error(e.to_string(), None)),
        }
    }

    #[tool(
        description = "List Agile boards. Optional `project_key` to filter to one project; omit to get all accessible (Agile API caps at 50). Useful for resolving board ids before driving sprint creation/start/close (Forgehold's UI has these but the MCP tool surface is read-only on boards for now)."
    )]
    async fn list_boards(
        &self,
        Parameters(ListBoardsParams { project_key }): Parameters<ListBoardsParams>,
    ) -> Result<CallToolResult, ErrorData> {
        match self.do_list_boards(project_key.as_deref()).await {
            Ok(text) => Ok(CallToolResult::success(vec![Content::text(text)])),
            Err(e) => Err(ErrorData::internal_error(e.to_string(), None)),
        }
    }

    #[tool(
        description = "List workflow statuses available on a project. Status sets are project-scoped (the same workflow can have different statuses across projects). Use to discover valid `status` values before issuing transitions or filtering JQL."
    )]
    async fn list_statuses(
        &self,
        Parameters(ListStatusesParams { project_key }): Parameters<ListStatusesParams>,
    ) -> Result<CallToolResult, ErrorData> {
        match self.do_list_statuses(&project_key).await {
            Ok(text) => Ok(CallToolResult::success(vec![Content::text(text)])),
            Err(e) => Err(ErrorData::internal_error(e.to_string(), None)),
        }
    }

    #[tool(
        description = "List issue types available on a project (Task, Bug, Story, Epic, Sub-task, …). Use to validate `issue_type` before create_issue."
    )]
    async fn list_issue_types(
        &self,
        Parameters(ListIssueTypesParams { project_key }): Parameters<ListIssueTypesParams>,
    ) -> Result<CallToolResult, ErrorData> {
        match self.do_list_issue_types(&project_key).await {
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
             - list_projects(query?) — discover project keys when the user mentions a project by partial name.\n\
             - list_assignable_users(project_key, query?) — translate a human name (\"give it to Petya\", \"@Nikolay\") into the accountId that create_issue / set_assignee need. Filter by name/email substring.\n\
             - list_sprints(project_key, state?) — translate a sprint name (\"Sprint 160\", \"current sprint\") into the numeric id that create_issue's sprint_id parameter accepts. state defaults to active+future; pass `all` if the user references a closed one.\n\
             - list_boards(project_key?) — Agile board ids; useful for project-scoped sprint operations.\n\
             - list_statuses(project_key) — workflow status names available on a project; use to validate transitions before calling them.\n\
             - list_issue_types(project_key) — issue types valid for create_issue on this project (Task / Bug / Story / Epic / sub-types).\n\
             - list_worklogs(key) — time entries on an issue.\n\n\
             WRITE:\n\
             - add_comment(key, body) — post a comment. Body is Markdown; full ADF conversion (headings, lists, code blocks, links, bold/italic).\n\
             - transition_issue(key, to, comment?) — move workflow state. Pass the human name (e.g. \"In Review\") or transition id.\n\
             - create_issue(project_key, summary, issue_type?, description?, assignee_account_id?, sprint_id?) — file a new ticket. Default issue_type is Task. ALWAYS call list_projects → list_assignable_users + list_sprints up front when the user asks for a ticket with assignee/sprint, so you can pass real ids instead of bouncing back to the user. Returns the new issue key + URL.\n\
             - update_issue(key, summary?, description?, assignee_account_id?, sprint_id?, labels?, priority?) — patch existing ticket fields. Pass only the keys you want to change. assignee_account_id=\"unassign\" clears the assignee. description is Markdown.\n\
             - set_priority(key, priority) — quick wrapper when the user just asks to bump priority (\"set DEVOPS-416 to High\").\n\
             - add_worklog(key, time_spent, comment?, started?) — log time. time_spent uses Jira shorthand (\"1h 30m\", \"45m\", \"2d\").\n\n\
             All write payloads accept Markdown for rich-text fields (description, comment body, worklog comment) and translate to ADF: headings, bullet/ordered lists, fenced code blocks, links, bold/italic/strike all preserved.\n\n\
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
        // Real markdown → ADF translation now (was plain-text wrapped before,
        // which silently dropped headings / lists / code blocks even though
        // the docstring promised "Atlassian Markdown" support).
        let url = format!(
            "https://{}/rest/api/3/issue/{}/comment",
            self.creds.workspace, key
        );
        let payload = serde_json::json!({ "body": markdown_to_adf(trimmed) });
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
                "comment": [{ "add": { "body": markdown_to_adf(c) } }]
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

    async fn do_create_issue(
        &self,
        project_key: &str,
        issue_type: &str,
        summary: &str,
        description: Option<&str>,
        assignee_account_id: Option<&str>,
        sprint_id: Option<u64>,
    ) -> anyhow::Result<String> {
        let project_key = project_key.trim();
        let summary = summary.trim();
        if project_key.is_empty() {
            anyhow::bail!("project_key is required");
        }
        if summary.is_empty() {
            anyhow::bail!("summary is required");
        }
        // Description goes in as a minimal ADF doc — same shape we use for
        // comments (no marks; one paragraph). Newlines turn into hard breaks
        // so a multi-line description renders sensibly in Jira UI.
        let mut fields = serde_json::json!({
            "project": { "key": project_key },
            "issuetype": { "name": issue_type },
            "summary": summary,
        });
        if let Some(d) = description.map(str::trim).filter(|s| !s.is_empty()) {
            // Full markdown → ADF: headings, lists, code blocks, links, etc.
            // Was previously a single paragraph with hard-breaks, which
            // turned `## Section` into literal `## Section` text in the UI.
            fields["description"] = markdown_to_adf(d);
        }
        if let Some(acct) = assignee_account_id.map(str::trim).filter(|s| !s.is_empty()) {
            fields["assignee"] = serde_json::json!({ "accountId": acct });
        }
        let payload = serde_json::json!({ "fields": fields });
        let url = format!("https://{}/rest/api/3/issue", self.creds.workspace);
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
            anyhow::bail!("Jira {} creating issue: {}", status, truncate(&body, 600));
        }
        let v: serde_json::Value = resp.json().await?;
        let key = v
            .get("key")
            .and_then(|k| k.as_str())
            .ok_or_else(|| anyhow::anyhow!("create response missing `key`"))?
            .to_string();
        let url = format!("https://{}/browse/{}", self.creds.workspace, key);

        // Optional follow-up: drop the new issue into a sprint via the
        // Agile API. Failures here are surfaced as a soft warning in the
        // returned text — the issue itself was created successfully.
        let mut sprint_note = String::new();
        if let Some(sid) = sprint_id {
            let agile_url = format!(
                "https://{}/rest/agile/1.0/sprint/{}/issue",
                self.creds.workspace, sid
            );
            let agile_payload = serde_json::json!({ "issues": [&key] });
            let agile_resp = self
                .http
                .post(&agile_url)
                .basic_auth(&self.creds.email, Some(&self.creds.token))
                .header("Accept", "application/json")
                .header("Content-Type", "application/json")
                .json(&agile_payload)
                .send()
                .await;
            match agile_resp {
                Ok(r) if r.status().is_success() => {
                    sprint_note = format!(" Added to sprint {}.", sid);
                }
                Ok(r) => {
                    let s = r.status();
                    let b = r.text().await.unwrap_or_default();
                    sprint_note = format!(
                        " (sprint {} assignment failed: {} {})",
                        sid,
                        s,
                        truncate(&b, 200)
                    );
                }
                Err(e) => {
                    sprint_note = format!(" (sprint {} assignment failed: {})", sid, e);
                }
            }
        }

        Ok(format!("Created {} — {}.{}", key, url, sprint_note))
    }

    async fn do_update_issue(
        &self,
        key: &str,
        summary: Option<&str>,
        description: Option<&str>,
        assignee_account_id: Option<&str>,
        sprint_id: Option<u64>,
        labels: Option<&[String]>,
        priority: Option<&str>,
    ) -> anyhow::Result<String> {
        let key = key.trim();
        if key.is_empty() {
            anyhow::bail!("issue key is required");
        }

        let mut fields = serde_json::Map::new();
        if let Some(s) = summary.map(str::trim) {
            if s.is_empty() {
                anyhow::bail!("summary cannot be empty (omit the field to leave unchanged)");
            }
            fields.insert("summary".into(), serde_json::Value::String(s.to_string()));
        }
        if let Some(d) = description {
            // Empty string clears the field — render as an empty ADF doc.
            // Non-empty: full markdown → ADF translation.
            let adf = if d.trim().is_empty() {
                serde_json::json!({
                    "type": "doc",
                    "version": 1,
                    "content": [{ "type": "paragraph" }],
                })
            } else {
                markdown_to_adf(d)
            };
            fields.insert("description".into(), adf);
        }
        if let Some(acct) = assignee_account_id {
            let trimmed = acct.trim();
            if trimmed.eq_ignore_ascii_case("unassign") || trimmed.is_empty() {
                fields.insert("assignee".into(), serde_json::Value::Null);
            } else {
                fields.insert(
                    "assignee".into(),
                    serde_json::json!({ "accountId": trimmed }),
                );
            }
        }
        if let Some(ls) = labels {
            // Jira's PUT replaces the labels array verbatim; pass through.
            fields.insert(
                "labels".into(),
                serde_json::Value::Array(
                    ls.iter()
                        .map(|s| serde_json::Value::String(s.trim().to_string()))
                        .filter(|v| match v {
                            serde_json::Value::String(s) => !s.is_empty(),
                            _ => false,
                        })
                        .collect(),
                ),
            );
        }
        if let Some(p) = priority {
            let trimmed = p.trim();
            if trimmed.is_empty() {
                // Empty string → null clears it (Jira falls back to
                // project default).
                fields.insert("priority".into(), serde_json::Value::Null);
            } else {
                fields.insert(
                    "priority".into(),
                    serde_json::json!({ "name": trimmed }),
                );
            }
        }

        let mut updated_parts: Vec<&str> = Vec::new();
        if !fields.is_empty() {
            let url = format!("https://{}/rest/api/3/issue/{}", self.creds.workspace, key);
            let payload = serde_json::json!({ "fields": serde_json::Value::Object(fields.clone()) });
            let resp = self
                .http
                .put(&url)
                .basic_auth(&self.creds.email, Some(&self.creds.token))
                .header("Accept", "application/json")
                .header("Content-Type", "application/json")
                .json(&payload)
                .send()
                .await?;
            let status = resp.status();
            if !status.is_success() {
                let body = resp.text().await.unwrap_or_default();
                anyhow::bail!("Jira {} updating {}: {}", status, key, truncate(&body, 600));
            }
            for k in fields.keys() {
                updated_parts.push(k.as_str());
            }
        }

        // Sprint moves go through the Agile API — separate from the field PUT.
        let mut sprint_note = String::new();
        if let Some(sid) = sprint_id {
            let agile_url = format!(
                "https://{}/rest/agile/1.0/sprint/{}/issue",
                self.creds.workspace, sid
            );
            let payload = serde_json::json!({ "issues": [key] });
            let resp = self
                .http
                .post(&agile_url)
                .basic_auth(&self.creds.email, Some(&self.creds.token))
                .header("Accept", "application/json")
                .header("Content-Type", "application/json")
                .json(&payload)
                .send()
                .await;
            match resp {
                Ok(r) if r.status().is_success() => {
                    sprint_note = format!(" Moved to sprint {}.", sid);
                }
                Ok(r) => {
                    let s = r.status();
                    let b = r.text().await.unwrap_or_default();
                    sprint_note = format!(
                        " (sprint {} move failed: {} {})",
                        sid,
                        s,
                        truncate(&b, 200)
                    );
                }
                Err(e) => {
                    sprint_note = format!(" (sprint {} move failed: {})", sid, e);
                }
            }
        }

        if updated_parts.is_empty() && sprint_note.is_empty() {
            return Ok(format!("No changes specified for {}.", key));
        }
        let url = format!("https://{}/browse/{}", self.creds.workspace, key);
        let touched = if updated_parts.is_empty() {
            String::new()
        } else {
            format!(" Updated fields: {}.", updated_parts.join(", "))
        };
        Ok(format!("Updated {} — {}.{}{}", key, url, touched, sprint_note))
    }

    async fn fetch_assignable_users(
        &self,
        project_key: &str,
        needle: Option<&str>,
    ) -> anyhow::Result<String> {
        let url = format!(
            "https://{}/rest/api/3/user/assignable/search?project={}&maxResults=200",
            self.creds.workspace,
            urlencoding::encode(project_key.trim())
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
            anyhow::bail!(
                "Jira {} listing assignable users for {}: {}",
                status,
                project_key,
                truncate(&body, 500)
            );
        }
        let raw: serde_json::Value = resp.json().await?;
        let users = raw.as_array().cloned().unwrap_or_default();
        let needle_lc = needle.map(|s| s.to_lowercase());
        let mut lines: Vec<String> = Vec::new();
        for u in &users {
            let active = u.get("active").and_then(|x| x.as_bool()).unwrap_or(false);
            if !active {
                continue;
            }
            let name = u.get("displayName").and_then(|x| x.as_str()).unwrap_or("");
            let email = u.get("emailAddress").and_then(|x| x.as_str()).unwrap_or("");
            let acct = u.get("accountId").and_then(|x| x.as_str()).unwrap_or("");
            if let Some(n) = needle_lc.as_deref() {
                let hay = format!("{} {}", name, email).to_lowercase();
                if !hay.contains(n) {
                    continue;
                }
            }
            lines.push(format!(
                "- accountId={}  displayName={}  email={}",
                acct,
                name,
                if email.is_empty() { "(hidden)" } else { email }
            ));
        }
        if lines.is_empty() {
            return Ok(format!("No assignable users found for project `{}`{}.", project_key,
                needle.map(|n| format!(" matching `{}`", n)).unwrap_or_default()));
        }
        Ok(format!(
            "Assignable users for `{}` ({} total):\n{}",
            project_key,
            lines.len(),
            lines.join("\n")
        ))
    }

    async fn fetch_sprints(
        &self,
        project_key: &str,
        state: &str,
    ) -> anyhow::Result<String> {
        // 1) Resolve project's first scrum board. Most teams have one — covers
        //    the common case without forcing the agent to know board ids.
        let boards_url = format!(
            "https://{}/rest/agile/1.0/board?projectKeyOrId={}&type=scrum&maxResults=50",
            self.creds.workspace,
            urlencoding::encode(project_key.trim())
        );
        let boards_resp = self
            .http
            .get(&boards_url)
            .basic_auth(&self.creds.email, Some(&self.creds.token))
            .header("Accept", "application/json")
            .send()
            .await?;
        let boards_status = boards_resp.status();
        if !boards_status.is_success() {
            let body = boards_resp.text().await.unwrap_or_default();
            anyhow::bail!(
                "Jira {} listing boards for {}: {}",
                boards_status,
                project_key,
                truncate(&body, 500)
            );
        }
        let boards_v: serde_json::Value = boards_resp.json().await?;
        let board_id = boards_v
            .get("values")
            .and_then(|x| x.as_array())
            .and_then(|a| a.first())
            .and_then(|b| b.get("id"))
            .and_then(|x| x.as_u64())
            .ok_or_else(|| anyhow::anyhow!(
                "Project `{}` has no scrum board — sprints aren't applicable here.",
                project_key
            ))?;

        // 2) Pull sprints, optionally filtered by state. Jira's Agile API
        //    accepts comma-separated states; "all" means "no filter".
        let mut url = format!(
            "https://{}/rest/agile/1.0/board/{}/sprint?maxResults=50",
            self.creds.workspace, board_id
        );
        if state != "all" {
            url.push_str(&format!("&state={}", urlencoding::encode(state)));
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
            anyhow::bail!(
                "Jira {} listing sprints for board {}: {}",
                status,
                board_id,
                truncate(&body, 500)
            );
        }
        let v: serde_json::Value = resp.json().await?;
        let sprints = v.get("values").and_then(|x| x.as_array()).cloned().unwrap_or_default();
        if sprints.is_empty() {
            return Ok(format!(
                "No `{}` sprints on board {} (project `{}`).",
                state, board_id, project_key
            ));
        }
        let mut lines: Vec<String> = Vec::new();
        for s in &sprints {
            let id = s.get("id").and_then(|x| x.as_u64()).unwrap_or(0);
            let name = s.get("name").and_then(|x| x.as_str()).unwrap_or("");
            let st = s.get("state").and_then(|x| x.as_str()).unwrap_or("");
            lines.push(format!("- id={}  name={}  state={}", id, name, st));
        }
        Ok(format!(
            "Sprints on board {} (project `{}`, state={}):\n{}",
            board_id,
            project_key,
            state,
            lines.join("\n")
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

    async fn do_add_worklog(
        &self,
        key: &str,
        time_spent: &str,
        comment: Option<&str>,
        started: Option<&str>,
    ) -> anyhow::Result<String> {
        let key = key.trim();
        if key.is_empty() {
            anyhow::bail!("issue key is required");
        }
        if time_spent.trim().is_empty() {
            anyhow::bail!("time_spent is required (e.g. \"1h 30m\")");
        }
        let url = format!(
            "https://{}/rest/api/3/issue/{}/worklog",
            self.creds.workspace, key
        );
        let mut payload = serde_json::Map::new();
        payload.insert(
            "timeSpent".into(),
            serde_json::Value::String(time_spent.trim().to_string()),
        );
        if let Some(c) = comment.map(str::trim).filter(|s| !s.is_empty()) {
            payload.insert("comment".into(), markdown_to_adf(c));
        }
        if let Some(s) = started.map(str::trim).filter(|s| !s.is_empty()) {
            payload.insert("started".into(), serde_json::Value::String(s.to_string()));
        }
        let resp = self
            .http
            .post(&url)
            .basic_auth(&self.creds.email, Some(&self.creds.token))
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .json(&serde_json::Value::Object(payload))
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Jira {} on worklog/{}: {}", status, key, truncate(&body, 600));
        }
        let v: serde_json::Value = resp.json().await?;
        let id = v.get("id").and_then(|x| x.as_str()).unwrap_or("?");
        Ok(format!("Worklog #{} added to {} ({}).", id, key, time_spent))
    }

    async fn do_list_worklogs(&self, key: &str) -> anyhow::Result<String> {
        let key = key.trim();
        if key.is_empty() {
            anyhow::bail!("issue key is required");
        }
        let url = format!(
            "https://{}/rest/api/3/issue/{}/worklog?maxResults=100",
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
            anyhow::bail!("Jira {} on worklog/{}: {}", status, key, truncate(&body, 600));
        }
        let v: serde_json::Value = resp.json().await?;
        let logs = v
            .get("worklogs")
            .and_then(|x| x.as_array())
            .cloned()
            .unwrap_or_default();
        if logs.is_empty() {
            return Ok(format!("No worklogs on {}.", key));
        }
        let mut out = format!("Worklogs on {} ({} total):\n", key, logs.len());
        for w in logs.iter().take(50) {
            let author = w
                .get("author")
                .and_then(|a| a.get("displayName"))
                .and_then(|x| x.as_str())
                .unwrap_or("?");
            let time = w.get("timeSpent").and_then(|x| x.as_str()).unwrap_or("?");
            let started = w.get("started").and_then(|x| x.as_str()).unwrap_or("");
            out.push_str(&format!("- {} · {} · {}\n", started, author, time));
        }
        if logs.len() > 50 {
            out.push_str(&format!("… and {} more\n", logs.len() - 50));
        }
        Ok(out)
    }

    async fn do_list_boards(&self, project_key: Option<&str>) -> anyhow::Result<String> {
        let mut url = format!(
            "https://{}/rest/agile/1.0/board?maxResults=50",
            self.creds.workspace
        );
        if let Some(pk) = project_key.map(str::trim).filter(|s| !s.is_empty()) {
            use std::fmt::Write as _;
            let _ = write!(&mut url, "&projectKeyOrId={}", urlencoding::encode(pk));
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
            anyhow::bail!("Jira {} on /board: {}", status, truncate(&body, 600));
        }
        let v: serde_json::Value = resp.json().await?;
        let boards = v
            .get("values")
            .and_then(|x| x.as_array())
            .cloned()
            .unwrap_or_default();
        if boards.is_empty() {
            return Ok("No boards found.".to_string());
        }
        let mut out = format!("Boards ({} total):\n", boards.len());
        for b in &boards {
            let id = b.get("id").and_then(|x| x.as_u64()).unwrap_or(0);
            let name = b.get("name").and_then(|x| x.as_str()).unwrap_or("?");
            let kind = b.get("type").and_then(|x| x.as_str()).unwrap_or("?");
            out.push_str(&format!("- id={} · {} ({})\n", id, name, kind));
        }
        Ok(out)
    }

    async fn do_list_statuses(&self, project_key: &str) -> anyhow::Result<String> {
        let key = project_key.trim();
        if key.is_empty() {
            anyhow::bail!("project_key is required");
        }
        let url = format!(
            "https://{}/rest/api/3/project/{}/statuses",
            self.creds.workspace,
            urlencoding::encode(key)
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
            anyhow::bail!("Jira {} on /project/{}/statuses: {}", status, key, truncate(&body, 600));
        }
        // Response is `[{name: <issuetype>, statuses: [{ name, ... }]}]`.
        // Flatten and dedupe by status name so the agent gets a clean
        // "valid statuses on this project" list.
        let v: serde_json::Value = resp.json().await?;
        let arr = v.as_array().cloned().unwrap_or_default();
        let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for it in &arr {
            if let Some(ss) = it.get("statuses").and_then(|x| x.as_array()) {
                for s in ss {
                    if let Some(n) = s.get("name").and_then(|x| x.as_str()) {
                        seen.insert(n.to_string());
                    }
                }
            }
        }
        if seen.is_empty() {
            return Ok(format!("No statuses found for project {}.", key));
        }
        let mut out = format!("Statuses on {} ({} unique):\n", key, seen.len());
        for s in &seen {
            out.push_str(&format!("- {}\n", s));
        }
        Ok(out)
    }

    async fn do_list_issue_types(&self, project_key: &str) -> anyhow::Result<String> {
        let key = project_key.trim();
        if key.is_empty() {
            anyhow::bail!("project_key is required");
        }
        // Jira's REST API shape: `/issue/createmeta` with project filter
        // gives back the valid issue types for that project (project-
        // scoped because admins can hide types per-project).
        let url = format!(
            "https://{}/rest/api/3/issue/createmeta?projectKeys={}",
            self.creds.workspace,
            urlencoding::encode(key)
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
            anyhow::bail!("Jira {} on createmeta: {}", status, truncate(&body, 600));
        }
        let v: serde_json::Value = resp.json().await?;
        let projects = v
            .get("projects")
            .and_then(|x| x.as_array())
            .cloned()
            .unwrap_or_default();
        let mut out = format!("Issue types on {}:\n", key);
        for p in &projects {
            if let Some(types) = p.get("issuetypes").and_then(|x| x.as_array()) {
                for t in types {
                    let name = t.get("name").and_then(|x| x.as_str()).unwrap_or("?");
                    let subtask = t.get("subtask").and_then(|x| x.as_bool()).unwrap_or(false);
                    out.push_str(&format!(
                        "- {}{}\n",
                        name,
                        if subtask { " (sub-task)" } else { "" }
                    ));
                }
            }
        }
        if out.lines().count() <= 1 {
            return Ok(format!("No issue types found for project {}.", key));
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
    // Per-row browse URL stays inline (same shape as Sentry's permalink) —
    // a `<KEY>` template in the header is technically smaller but bets the
    // agent will splice the key correctly when generating Markdown links,
    // and that bet has cost more in fix-up turns than the saved tokens are
    // worth. The only thing we trim is the ISO timestamp: the milliseconds
    // and timezone offset on `updated` aren't useful for triage and the
    // per-row line repeats N times in every subsequent turn.
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
        let updated = fields
            .get("updated")
            .and_then(|s| s.as_str())
            .map(trim_iso_to_minute)
            .unwrap_or_default();
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

/// `2026-04-25T14:23:11.123+0300` → `2026-04-25 14:23`. Anything that
/// doesn't look like ISO-8601 (or a partial fragment) is returned as-is so
/// we never silently mangle a value Jira hands us.
fn trim_iso_to_minute(s: &str) -> String {
    let (date, rest) = match s.split_once('T') {
        Some(parts) => parts,
        None => return s.to_string(),
    };
    // `rest` looks like `14:23:11.123+0300` — keep just hours and minutes.
    let mut hm = rest.split(':');
    let h = hm.next().unwrap_or("");
    let m = hm.next().unwrap_or("");
    if h.is_empty() || m.is_empty() {
        return s.to_string();
    }
    format!("{} {}:{}", date, h, m)
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
