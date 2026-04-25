//! forgehold-app — MCP sidecar that lets Claude / Cursor drive the
//! Forgehold UI itself: open detail panes, switch top-level views, add
//! editor instances, prompt the user to connect a missing source.
//!
//! How it works
//!   The sidecar's tools don't actually mutate the UI from here — they
//!   can't reach the running Forgehold process. Instead, each tool
//!   validates its arguments and returns a confirmation string. The
//!   Forgehold frontend's stream parser (`src/lib/stream/claudeStream.ts`)
//!   sees the corresponding `mcp__app__*` tool_use event in Claude's
//!   stream-json output and performs the navigation directly via its
//!   already-reactive Svelte state. So the round-trip is:
//!
//!     Claude → tool_use {name: "mcp__app__open_jira_issue", input: …}
//!       → forgehold-app validates + replies "Opened DEVOPS-414."
//!       → frontend's stream handler also sees the tool_use, sets
//!         `inboxState.jira.focusKey = "DEVOPS-414"`, slide-over appears.
//!
//! The sidecar is intentionally thin — its main job is making the tools
//! callable so they show up in `--allowedTools` and the LLM has a
//! schema to fill in.

use rmcp::{
    ErrorData, ServerHandler, ServiceExt,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, Content, ServerCapabilities, ServerInfo},
    schemars, tool, tool_handler, tool_router,
    transport::stdio,
};
use serde::Deserialize;

const VALID_VIEWS: &[&str] = &[
    "workbench",
    "repositories",
    "tasks",
    "issues",
    "rules",
    "connections",
    "settings",
];

const VALID_SOURCES: &[&str] = &[
    "github", "jira", "sentry", "claude", "cursor",
    "slack", "linear", "notion", "gitlab", "teams", "asana",
    "codex", "aider", "copilot",
];

const VALID_PR_TABS: &[&str] = &[
    "conversation", "commits", "files", "reviews", "checks",
];

#[derive(Clone)]
struct App {
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct OpenGithubPrParams {
    /// Repository owner (user or org), e.g. "Efficiently-Dev".
    owner: String,
    /// Repository name, e.g. "efficiently".
    repo: String,
    /// Pull request number.
    number: u64,
    /// Optional tab to focus inside the PR detail pane.
    /// One of: conversation | commits | files | reviews | checks.
    /// Defaults to "conversation".
    #[serde(default)]
    tab: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct OpenGithubIssueParams {
    /// Repository owner.
    owner: String,
    /// Repository name.
    repo: String,
    /// Issue number.
    number: u64,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct OpenJiraIssueParams {
    /// Issue key (e.g. "DEVOPS-414"). Opens Forgehold's Jira slide-over
    /// pane on this issue.
    key: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct OpenSentryIssueParams {
    /// Sentry issue id (numeric, e.g. "5728934712") OR short id
    /// (e.g. "BMS-API-J6"). Forgehold resolves short-ids when needed.
    id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SwitchViewParams {
    /// Which top-level tab to switch to. One of:
    /// `workbench` (default columns view), `repositories` (full GitHub
    /// browser), `tasks` (Jira board), `issues` (Sentry issues),
    /// `rules` (user rules editor), `connections` (sources + agents
    /// configure), `settings`.
    view: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AddEditorInstanceParams {
    /// Optional absolute path to the folder/repo the new editor column
    /// should open. Omit to create an empty editor — the user can pick
    /// a folder afterwards.
    #[serde(default)]
    repo_path: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct OpenConnectModalParams {
    /// Source/agent id whose connect modal to open. One of:
    /// github, jira, sentry, claude, cursor, slack, linear, notion,
    /// gitlab, teams, asana, codex, aider, copilot. The modal renders
    /// install / token instructions appropriate for the source.
    source: String,
}

fn validate_one_of(value: &str, choices: &[&str], label: &str) -> Result<(), ErrorData> {
    if choices.contains(&value) {
        Ok(())
    } else {
        Err(ErrorData::invalid_params(
            format!(
                "invalid {label} `{value}`. expected one of: {}",
                choices.join(", ")
            ),
            None,
        ))
    }
}

#[tool_router]
impl App {
    fn new() -> Self {
        Self { tool_router: Self::tool_router() }
    }

    #[tool(
        description = "Open a GitHub pull request in Forgehold's PR detail pane (the slide-over Forgehold normally opens when the user clicks a PR card). Optional `tab` selects which sub-tab is focused on open: `conversation` (default — comments + reviews), `commits`, `files` (diff browser), `reviews`, `checks` (CI). Use this when the user says \"open PR #X\" or asks to look at a specific PR — it gives them the same view they'd get clicking through the inbox themselves."
    )]
    async fn open_github_pr(
        &self,
        Parameters(OpenGithubPrParams { owner, repo, number, tab }): Parameters<OpenGithubPrParams>,
    ) -> Result<CallToolResult, ErrorData> {
        if let Some(t) = tab.as_deref() {
            validate_one_of(t, VALID_PR_TABS, "tab")?;
        }
        let tab_label = tab.as_deref().unwrap_or("conversation");
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Opening {}/{}#{} ({} tab) in Forgehold's PR detail pane.",
            owner, repo, number, tab_label
        ))]))
    }

    #[tool(
        description = "Open a GitHub issue in Forgehold's detail pane — same as open_github_pr but for plain issues (not PRs). Use when the user references an issue by repo + number."
    )]
    async fn open_github_issue(
        &self,
        Parameters(OpenGithubIssueParams { owner, repo, number }): Parameters<OpenGithubIssueParams>,
    ) -> Result<CallToolResult, ErrorData> {
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Opening {}/{}#{} (issue) in Forgehold's detail pane.",
            owner, repo, number
        ))]))
    }

    #[tool(
        description = "Open a Jira issue in Forgehold's slide-over pane. Same as the user clicking the ticket from the Jira column — shows description, comments, transitions, worklog. Use when the user says \"show DEVOPS-414\" or wants to look at a specific ticket."
    )]
    async fn open_jira_issue(
        &self,
        Parameters(OpenJiraIssueParams { key }): Parameters<OpenJiraIssueParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let trimmed = key.trim();
        if trimmed.is_empty() {
            return Err(ErrorData::invalid_params("issue key is empty", None));
        }
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Opening Jira {} in Forgehold's detail pane.",
            trimmed
        ))]))
    }

    #[tool(
        description = "Open a Sentry issue in Forgehold's slide-over detail pane. Accepts either the numeric id or the short id (e.g. `BMS-API-J6` — Forgehold resolves it). Use when the user says \"show BMS-API-J6\" or wants to drill into a specific Sentry issue."
    )]
    async fn open_sentry_issue(
        &self,
        Parameters(OpenSentryIssueParams { id }): Parameters<OpenSentryIssueParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let trimmed = id.trim();
        if trimmed.is_empty() {
            return Err(ErrorData::invalid_params("issue id is empty", None));
        }
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Opening Sentry issue {} in Forgehold's detail pane.",
            trimmed
        ))]))
    }

    #[tool(
        description = "Switch Forgehold's top-level view tab. Available tabs: `workbench` (default — columns), `repositories` (full GitHub browser with code/branches/releases/PRs), `tasks` (Jira board), `issues` (Sentry issues browser), `rules` (user-rules editor), `connections` (sources + agents configure), `settings`. Use when the user wants to navigate (\"open repos\", \"go to tasks\", \"show me sentry issues\")."
    )]
    async fn switch_view(
        &self,
        Parameters(SwitchViewParams { view }): Parameters<SwitchViewParams>,
    ) -> Result<CallToolResult, ErrorData> {
        validate_one_of(&view, VALID_VIEWS, "view")?;
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Switching Forgehold view → {}.",
            view
        ))]))
    }

    #[tool(
        description = "Add a new Editor column to the active workbench. If `repo_path` is provided, the new editor opens that folder immediately. Omit `repo_path` to create an empty editor the user can fill in. Use when the user says \"open the X folder in editor\" or asks to look at code from a different repo than the one already open."
    )]
    async fn add_editor_instance(
        &self,
        Parameters(AddEditorInstanceParams { repo_path }): Parameters<AddEditorInstanceParams>,
    ) -> Result<CallToolResult, ErrorData> {
        match repo_path.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            Some(p) => Ok(CallToolResult::success(vec![Content::text(format!(
                "Adding new Editor column with folder `{}`.",
                p
            ))])),
            None => Ok(CallToolResult::success(vec![Content::text(
                "Adding new empty Editor column.".to_string(),
            )])),
        }
    }

    #[tool(
        description = "Open the connect / status modal for a source or agent. Use this when the user mentions an integration that isn't connected yet (\"do I have Slack hooked up?\", \"connect Sentry\") — surface the modal so they can finish setup. Source ids: github, jira, sentry, claude, cursor, slack, linear, notion, gitlab, teams, asana, codex, aider, copilot."
    )]
    async fn open_connect_modal(
        &self,
        Parameters(OpenConnectModalParams { source }): Parameters<OpenConnectModalParams>,
    ) -> Result<CallToolResult, ErrorData> {
        validate_one_of(&source, VALID_SOURCES, "source")?;
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Opening connect modal for `{}`.",
            source
        ))]))
    }
}

#[tool_handler]
impl ServerHandler for App {
    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::default();
        info.capabilities = ServerCapabilities::builder().enable_tools().build();
        info.instructions = Some(
            "Drive Forgehold's UI directly. Use these tools when the user wants you to NAVIGATE the app on their behalf:\n\
             - open_github_pr / open_github_issue — show a PR or issue in the detail pane (with optional tab focus for PRs).\n\
             - open_jira_issue — open a Jira ticket's slide-over.\n\
             - open_sentry_issue — open a Sentry issue's slide-over.\n\
             - switch_view — change the top-level tab (workbench / repositories / tasks / issues / rules / connections / settings).\n\
             - add_editor_instance — spin up a new Editor column for a folder.\n\
             - open_connect_modal — surface the connect/status modal for any source/agent (use when the user asks about an integration that isn't connected yet — e.g. they mention Slack and you can see it's not in their connected list).\n\n\
             These don't need approval cards — they're harmless navigation that gives the user the same view they'd get clicking through manually. Call them whenever they save the user a step."
                .to_string(),
        );
        info
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let app = App::new();
    let service = app.serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
