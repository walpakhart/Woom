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

const VALID_INSTANCE_KINDS: &[&str] = &[
    "github", "jira", "sentry", "claude", "cursor", "editor",
];

const VALID_REPO_SECTIONS: &[&str] = &[
    "code", "pulls", "issues", "actions", "releases",
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

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AddWorkbenchInstanceParams {
    /// Kind of instance to add. One of: github, jira, sentry, claude,
    /// cursor, editor. Singleton kinds (github/jira/sentry) are no-ops if
    /// already present in the active workbench.
    kind: String,
    /// Only meaningful when `kind = "editor"`. Absolute folder path to
    /// open in the new editor column. Omit for an empty editor.
    #[serde(default)]
    repo_path: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct NewWorkbenchParams {
    /// Display name for the new workbench tab. Optional — defaults to
    /// "Workbench".
    #[serde(default)]
    name: Option<String>,
    /// If true, switch the active workbench to the new one immediately
    /// after creation. Defaults to true — usually what you want when
    /// the user says "make a new workbench for X".
    #[serde(default = "default_true")]
    activate: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SwitchWorkbenchParams {
    /// Workbench name to switch to. Case-insensitive match. Either
    /// `name` or `index` must be provided.
    #[serde(default)]
    name: Option<String>,
    /// 0-based index of the workbench tab to switch to. Either `name`
    /// or `index` must be provided.
    #[serde(default)]
    index: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct FocusWorkbenchInstanceParams {
    /// Kind of instance to scroll/focus in the active workbench. One of:
    /// github, jira, sentry, claude, cursor, editor. If no instance of
    /// this kind exists in the active workbench, one is created.
    kind: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct OpenRepoParams {
    /// Repository owner (user or org).
    owner: String,
    /// Repository name.
    repo: String,
    /// Which section of the repo to land on. One of: code (file
    /// browser), pulls (PR list), issues (issue list), actions (workflow
    /// runs), releases. Defaults to "pulls".
    #[serde(default)]
    section: Option<String>,
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

    #[tool(
        description = "Add a column / instance of any kind to the active workbench. Replaces `add_editor_instance` with a single tool that handles all kinds. Kinds: `github` (PR/issue inbox), `jira` (Jira inbox), `sentry` (Sentry issues inbox), `claude` (Claude chat column), `cursor` (Cursor chat column), `editor` (file browser + editor). For `editor`, optionally pass `repo_path` to open a folder immediately. Singleton kinds (github/jira/sentry) are no-ops if already present in the active workbench. Use whenever the user asks to \"add a Claude column\" / \"open editor for /Users/me/Repos/foo\" / \"give me another agent\"."
    )]
    async fn add_workbench_instance(
        &self,
        Parameters(AddWorkbenchInstanceParams { kind, repo_path }): Parameters<AddWorkbenchInstanceParams>,
    ) -> Result<CallToolResult, ErrorData> {
        validate_one_of(&kind, VALID_INSTANCE_KINDS, "kind")?;
        let path = repo_path.as_deref().map(str::trim).filter(|s| !s.is_empty());
        match (kind.as_str(), path) {
            ("editor", Some(p)) => Ok(CallToolResult::success(vec![Content::text(format!(
                "Adding new Editor column with folder `{}`.",
                p
            ))])),
            ("editor", None) => Ok(CallToolResult::success(vec![Content::text(
                "Adding new empty Editor column.".to_string(),
            )])),
            (k, _) => Ok(CallToolResult::success(vec![Content::text(format!(
                "Adding new `{}` column to the active workbench.",
                k
            ))])),
        }
    }

    #[tool(
        description = "Create a new workbench tab with optional name. By default the new workbench becomes active. Pass `activate=false` to create-without-switch (rare). Use when the user says \"make a new workbench for the X feature\" — workbenches are independent column sets, so a new one means a clean slate."
    )]
    async fn new_workbench(
        &self,
        Parameters(NewWorkbenchParams { name, activate }): Parameters<NewWorkbenchParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let label = name
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("Workbench")
            .to_string();
        let suffix = if activate { " and switching to it" } else { " in the background" };
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Creating workbench `{}`{}.",
            label, suffix
        ))]))
    }

    #[tool(
        description = "Switch the active workbench to one identified by `name` (case-insensitive) or 0-based `index`. Provide exactly one of the two. Use when the user references a workbench by label (\"go to the Manage workbench\") or position (\"switch to the second workbench\")."
    )]
    async fn switch_workbench(
        &self,
        Parameters(SwitchWorkbenchParams { name, index }): Parameters<SwitchWorkbenchParams>,
    ) -> Result<CallToolResult, ErrorData> {
        match (name.as_deref().map(str::trim).filter(|s| !s.is_empty()), index) {
            (Some(n), _) => Ok(CallToolResult::success(vec![Content::text(format!(
                "Switching to workbench `{}`.",
                n
            ))])),
            (None, Some(i)) => Ok(CallToolResult::success(vec![Content::text(format!(
                "Switching to workbench #{}.",
                i
            ))])),
            (None, None) => Err(ErrorData::invalid_params(
                "either `name` or `index` must be provided",
                None,
            )),
        }
    }

    #[tool(
        description = "Scroll the existing column of `kind` in the active workbench into view (and highlight it). If no column of that kind exists, one is created. Useful when the user says \"focus the GitHub column\" / \"show me the editor\" / \"jump to claude\" — saves them a horizontal scroll."
    )]
    async fn focus_workbench_instance(
        &self,
        Parameters(FocusWorkbenchInstanceParams { kind }): Parameters<FocusWorkbenchInstanceParams>,
    ) -> Result<CallToolResult, ErrorData> {
        validate_one_of(&kind, VALID_INSTANCE_KINDS, "kind")?;
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Focusing `{}` column in the active workbench.",
            kind
        ))]))
    }

    #[tool(
        description = "Open a specific repository in the Repositories view (full GitHub browser). Switches the top-level view to `repositories`, picks the repo from the list, and lands on `section` (default: `pulls`). Sections: `code` (file browser + README), `pulls` (PR list), `issues` (issue list), `actions` (workflow runs), `releases`. Use whenever the user wants to drill into a repo (\"show me the actions for efficiently\", \"open the releases tab on forge\")."
    )]
    async fn open_repo(
        &self,
        Parameters(OpenRepoParams { owner, repo, section }): Parameters<OpenRepoParams>,
    ) -> Result<CallToolResult, ErrorData> {
        if owner.trim().is_empty() || repo.trim().is_empty() {
            return Err(ErrorData::invalid_params("owner and repo are required", None));
        }
        if let Some(s) = section.as_deref() {
            validate_one_of(s, VALID_REPO_SECTIONS, "section")?;
        }
        let s = section.as_deref().unwrap_or("pulls");
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Opening repository {}/{} → {} section.",
            owner.trim(),
            repo.trim(),
            s
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
             \n\
             ## Detail panes (slide-over from any view)\n\
             - open_github_pr / open_github_issue — show a PR or issue in the detail pane (with optional tab focus for PRs).\n\
             - open_jira_issue — open a Jira ticket's slide-over.\n\
             - open_sentry_issue — open a Sentry issue's slide-over.\n\
             \n\
             ## Top-level navigation\n\
             - switch_view — change the top-level tab (workbench / repositories / tasks / issues / rules / connections / settings).\n\
             - open_repo — switch to Repositories view AND open a specific repo on a section (code/pulls/issues/actions/releases). Use this instead of switch_view when the user names a repo.\n\
             \n\
             ## Workbench manipulation\n\
             - new_workbench — create a fresh workbench tab (with optional name). Activates it by default.\n\
             - switch_workbench — switch active workbench by name or index.\n\
             - add_workbench_instance — add any kind of column (github/jira/sentry/claude/cursor/editor) to the active workbench. For `editor`, optional `repo_path` opens a folder.\n\
             - focus_workbench_instance — scroll-to + highlight an existing column (creates one if none exists).\n\
             - add_editor_instance — DEPRECATED, use add_workbench_instance with kind=`editor`. Kept for back-compat.\n\
             \n\
             ## Sources\n\
             - open_connect_modal — surface the connect/status modal for any source/agent (use when the user asks about an integration that isn't connected yet — e.g. they mention Slack and you can see it's not in their connected list).\n\
             \n\
             # When to chain calls\n\
             These tools compose. \"Open the actions tab for forge\" → switch_view (workbench is fine) + open_repo with section=actions. \"Make a new workbench called Hotfix and add a Claude column there\" → new_workbench + add_workbench_instance(kind=claude). Don't ask for confirmation — these are harmless navigation that gives the user the same view they'd get clicking through manually."
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
