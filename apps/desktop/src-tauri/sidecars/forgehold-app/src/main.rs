//! forgehold-app — MCP sidecar that lets Claude / Cursor drive the
//! Forgehold UI itself: open detail panes, switch top-level views, add
//! editor instances, prompt the user to connect a missing source.
//!
//! How it works
//!   The sidecar's tools don't actually mutate the UI from here — they
//!   can't reach the running Forgehold process. Instead, each tool
//!   validates its arguments and returns a confirmation string. The
//!   Forgehold frontend's stream parser (`src/lib/stream/agentStream.ts`)
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

/// Top-level view identifiers exposed to the agent. Named after the
/// platform (`github` / `jira` / `sentry`) rather than the generic noun
/// the UI used to show ("Repositories" / "Tasks" / "Issues") because:
///   1. We're going to grow more sources of the same shape — a future
///      GitLab tab would have its own "repositories" page, and the
///      agent would have no way to disambiguate.
///   2. The internal `View` enum on the frontend is `githubTab` /
///      `jiraTab` / `sentryTab` — so this also matches the UI rail
///      tooltips and the platform pills the user sees.
const VALID_VIEWS: &[&str] = &[
    "workbench",
    "github",
    "jira",
    "sentry",
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

const VALID_GH_FILTER_MODES: &[&str] = &[
    "involving", "authored", "review_requested", "assigned", "user", "all",
];

const VALID_SENTRY_STATUSES: &[&str] = &[
    "unresolved", "resolved", "ignored", "all",
];

const VALID_SENTRY_LEVELS: &[&str] = &[
    "all", "fatal", "error", "warning", "info", "debug",
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
struct OpenSentryEventParams {
    /// Sentry issue id (numeric or short id).
    issue_id: String,
    /// Specific event id to open. Omit (or pass "latest") to load the
    /// most recent occurrence — same as plain `open_sentry_issue`.
    /// Use a real event id to surface a particular occurrence (e.g. the
    /// one with the user's email or the failing release) instead of
    /// the latest. Pair with `mcp__sentry__list_events` to pick one.
    #[serde(default)]
    event_id: Option<String>,
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
    /// Optional repo-relative path to open inside `section=code`. When
    /// set, Forgehold drills into the code browser to that file (or
    /// folder) — e.g. `src/lib/auth.ts`. Ignored for non-code sections.
    /// Use this when the user says "open <file> in <repo>" instead of
    /// just "open <repo>".
    #[serde(default)]
    path: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct OpenJiraTabParams {
    /// Project key to filter by (e.g. `DEVOPS`). Null/omitted = all
    /// projects. Resolve human names via `mcp__jira__list_projects`.
    #[serde(default)]
    project_key: Option<String>,
    /// Free-text JQL fragment appended to the search query (e.g.
    /// `"login flow"`). The tab's existing UI builds the full JQL —
    /// just pass the keyword.
    #[serde(default)]
    search: Option<String>,
    /// Status name like "In Review", "Done", "Blocked". Use the human
    /// label, the tab handles canonicalisation.
    #[serde(default)]
    status_name: Option<String>,
    /// Numeric Jira board ids to scope to. Multi-select — multiple
    /// boards OR-merge their projects. Resolve via the JQL search +
    /// board metadata; usually omit unless the user explicitly
    /// mentioned a board.
    #[serde(default)]
    board_ids: Option<Vec<u64>>,
    /// Numeric sprint ids (or `"backlog"`). Resolve via
    /// `mcp__jira__list_sprints`. Only meaningful with exactly one
    /// board selected.
    #[serde(default)]
    sprint_ids: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct OpenSentryTabParams {
    /// Sentry project slugs to filter by (e.g. `["catalog-api",
    /// "checkout-web"]`). Empty/omitted = all projects. Resolve via
    /// `mcp__sentry__list_projects`.
    #[serde(default)]
    projects: Option<Vec<String>>,
    /// Free-text search appended to Sentry's search query (e.g.
    /// `"login flow"`). Combined with the structured filters below.
    #[serde(default)]
    search: Option<String>,
    /// Issue status filter. One of: unresolved, resolved, ignored, all.
    /// Defaults to keeping the user's current selection.
    #[serde(default)]
    status: Option<String>,
    /// Severity level filter. One of: all, fatal, error, warning, info,
    /// debug.
    #[serde(default)]
    level: Option<String>,
    /// Environment slug (e.g. `production`, `staging`). Null = all envs.
    #[serde(default)]
    environment: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SetGithubColumnParams {
    /// Art-name of the github workbench column (e.g. "Petra"). Either
    /// `instance_name` or `instance_id` must be provided.
    #[serde(default)]
    instance_name: Option<String>,
    /// UUID of the github column.
    #[serde(default)]
    instance_id: Option<String>,
    /// `owner/name` to scope the column to (e.g. `Efficiently-Dev/efficiently`).
    /// Pass empty string `""` to clear the repo filter (= all repos).
    #[serde(default)]
    repo: Option<String>,
    /// Filter mode. One of: involving, authored, review_requested,
    /// assigned, user, all. Mirrors the dropdown in the column header.
    #[serde(default)]
    mode: Option<String>,
    /// Free-text search applied on top of `mode` + `repo` filters.
    #[serde(default)]
    search: Option<String>,
    /// GitHub login when `mode = "user"`. Ignored otherwise.
    #[serde(default)]
    custom_user: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SetJiraColumnParams {
    /// Art-name of the jira workbench column (e.g. "Mona-Lisa").
    /// Either `instance_name` or `instance_id` must be provided.
    #[serde(default)]
    instance_name: Option<String>,
    /// UUID of the jira column.
    #[serde(default)]
    instance_id: Option<String>,
    /// Project key (e.g. `DEVOPS`). Empty string clears the filter.
    #[serde(default)]
    project_key: Option<String>,
    /// Status name ("In Review", "Done"). Empty clears.
    #[serde(default)]
    status_name: Option<String>,
    /// Free-text search.
    #[serde(default)]
    search: Option<String>,
    /// Numeric board ids — see OpenJiraTabParams.
    #[serde(default)]
    board_ids: Option<Vec<u64>>,
    /// Sprint ids — see OpenJiraTabParams.
    #[serde(default)]
    sprint_ids: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SetSentryColumnParams {
    /// Art-name of the sentry workbench column. Either `instance_name`
    /// or `instance_id` must be provided.
    #[serde(default)]
    instance_name: Option<String>,
    /// UUID of the sentry column.
    #[serde(default)]
    instance_id: Option<String>,
    /// Sentry project slugs.
    #[serde(default)]
    projects: Option<Vec<String>>,
    /// Free-text search.
    #[serde(default)]
    search: Option<String>,
    /// Status filter. unresolved/resolved/ignored/all.
    #[serde(default)]
    status: Option<String>,
    /// Severity level. all/fatal/error/warning/info/debug.
    #[serde(default)]
    level: Option<String>,
    /// Environment slug.
    #[serde(default)]
    environment: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SetEditorRepoPathParams {
    /// Art-name of the editor instance (e.g. "Sagrada-Familia"). Either
    /// `instance_name` or `instance_id` must be provided. Names are
    /// matched case-insensitively.
    #[serde(default)]
    instance_name: Option<String>,
    /// UUID of the editor instance. Either `instance_name` or
    /// `instance_id` must be provided.
    #[serde(default)]
    instance_id: Option<String>,
    /// Absolute folder path to open in the editor. If the editor has
    /// linked agent sessions, their cwd is auto-updated to match.
    repo_path: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SetAgentCwdParams {
    /// Use `target = "self"` to point at the calling session (most
    /// common when the user says "switch myself"). Otherwise pass
    /// `instance_name` or `instance_id` of the target agent column.
    #[serde(default)]
    target: Option<String>,
    /// Art-name of the agent instance. Optional — only used when
    /// `target` is omitted or != "self".
    #[serde(default)]
    instance_name: Option<String>,
    /// UUID of the agent instance.
    #[serde(default)]
    instance_id: Option<String>,
    /// Absolute folder path to use as cwd. The change takes effect on
    /// the agent session's NEXT turn (the current turn keeps the old
    /// cwd it spawned with).
    repo_path: String,
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

/// Pull a non-empty `instance_name` first, fall back to `instance_id`,
/// or fail with the same error every column-mutating tool wants. Centralised
/// because every `set_*_column` tool needs the exact same dispatch and we
/// don't want to copy 6 lines of `.trim().filter().or()` six times.
fn require_instance_label<'a>(
    name: Option<&'a str>,
    id: Option<&'a str>,
) -> Result<&'a str, ErrorData> {
    let by_name = name.map(str::trim).filter(|s| !s.is_empty());
    let by_id = id.map(str::trim).filter(|s| !s.is_empty());
    by_name.or(by_id).ok_or_else(|| {
        ErrorData::invalid_params(
            "either `instance_name` or `instance_id` must be provided",
            None,
        )
    })
}

/// Tiny accumulator that builds a "k=v, k=v" trace string for the human-
/// readable confirmation a tool returns. The agent already sees the
/// structured `Parameters(…)` it sent, but the user sees this string in
/// the trace pill — so we want it readable, comma-separated, and skip
/// keys the caller didn't touch. Keeping it as a helper because the
/// three column-mutator tools (github/jira/sentry) follow the same
/// "patch only the keys you want" semantics and would otherwise each
/// reimplement the same Vec push + join.
struct FilterBits(Vec<String>);

fn changed_filter_bits() -> FilterBits {
    FilterBits(Vec::new())
}

impl FilterBits {
    fn push_kv(&mut self, key: &str, value: &str) {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            self.0.push(format!("{}=<cleared>", key));
        } else {
            self.0.push(format!("{}=\"{}\"", key, trimmed));
        }
    }

    fn summary(&self, empty: &str) -> String {
        if self.0.is_empty() {
            empty.to_string()
        } else {
            self.0.join(", ")
        }
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
        description = "Open a SPECIFIC event of a Sentry issue in Forgehold's detail pane (instead of just the latest). Use this when you've called mcp__sentry__list_events and want to surface one particular occurrence — e.g. \"the one in production after release 2.4.1\" or \"the one where user X hit it\". Pass `issue_id` (numeric or short id) and `event_id` (the real event id from list_events). Omit event_id to fall back to latest, equivalent to open_sentry_issue."
    )]
    async fn open_sentry_event(
        &self,
        Parameters(OpenSentryEventParams { issue_id, event_id }): Parameters<OpenSentryEventParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let issue = issue_id.trim();
        if issue.is_empty() {
            return Err(ErrorData::invalid_params("issue_id is empty", None));
        }
        let event = event_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("latest");
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Opening Sentry issue {} on event {} in Forgehold's detail pane.",
            issue, event
        ))]))
    }

    #[tool(
        description = "Switch Forgehold's top-level view tab. Available tabs: `workbench` (default — columns), `github` (GitHub browser with code/branches/releases/PRs/issues), `jira` (Jira board / inbox), `sentry` (Sentry issues browser), `rules` (user-rules editor), `connections` (sources + agents configure), `settings`. Use when the user wants to navigate (\"open github\", \"go to jira\", \"show me sentry issues\"). For SCOPED navigation (specific repo / project / sprint / sentry filter), prefer `open_github_repo` / `open_jira_tab` / `open_sentry_tab` instead — they switch the view AND apply filters in one call."
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
        description = "Open a specific GitHub repository inside Forgehold's GitHub tab. Switches the top-level view to `github`, picks the repo from the list, and lands on `section` (default: `pulls`). Sections: `code` (file browser + README), `pulls` (PR list), `issues` (issue list), `actions` (workflow runs), `releases`. Pass `path` together with `section=code` to drill into a specific file or folder (e.g. `src/lib/auth.ts`) — the file viewer opens with the contents preloaded. Named `open_github_repo` rather than `open_repo` because we'll grow into other VCS sources (GitLab, Bitbucket, etc.) where \"repository\" lookups need their own resolver. Use whenever the user wants to drill into a GitHub repo (\"show me the actions for efficiently\", \"open src/lib/auth.ts in efficiently\", \"open the releases tab on forge\")."
    )]
    async fn open_github_repo(
        &self,
        Parameters(OpenRepoParams { owner, repo, section, path }): Parameters<OpenRepoParams>,
    ) -> Result<CallToolResult, ErrorData> {
        if owner.trim().is_empty() || repo.trim().is_empty() {
            return Err(ErrorData::invalid_params("owner and repo are required", None));
        }
        if let Some(s) = section.as_deref() {
            validate_one_of(s, VALID_REPO_SECTIONS, "section")?;
        }
        let s = section.as_deref().unwrap_or("pulls");
        let trimmed_path = path.as_deref().map(str::trim).filter(|p| !p.is_empty());
        if trimmed_path.is_some() && s != "code" {
            return Err(ErrorData::invalid_params(
                "`path` is only valid when `section` is `code`",
                None,
            ));
        }
        let suffix = match trimmed_path {
            Some(p) => format!(" → file `{}`", p),
            None => String::new(),
        };
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Opening repository {}/{} → {} section{}.",
            owner.trim(),
            repo.trim(),
            s,
            suffix
        ))]))
    }

    #[tool(
        description = "Open the Jira top-level tab with optional filters applied — same controls as the tab's UI dropdowns. `project_key` scopes to one project, `status_name` filters by workflow state (\"In Review\", \"Done\"), `search` is a free-text fragment. `board_ids` and `sprint_ids` use Jira's numeric ids — resolve them via `mcp__jira__list_sprints` first if the user mentions a sprint by name. Omit a parameter to leave that filter as the user last set it. Use this instead of `switch_view view=jira` whenever the user asks for a SPECIFIC slice (\"show me my open Jira tickets in DEVOPS\", \"sprint 160 tickets\", \"in-review tickets\")."
    )]
    async fn open_jira_tab(
        &self,
        Parameters(OpenJiraTabParams { project_key, search, status_name, board_ids, sprint_ids }): Parameters<OpenJiraTabParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let mut bits: Vec<String> = Vec::new();
        if let Some(p) = project_key.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            bits.push(format!("project={}", p));
        }
        if let Some(s) = status_name.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            bits.push(format!("status=\"{}\"", s));
        }
        if let Some(q) = search.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            bits.push(format!("search=\"{}\"", q));
        }
        if let Some(b) = board_ids.as_ref().filter(|v| !v.is_empty()) {
            bits.push(format!(
                "boards=[{}]",
                b.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(",")
            ));
        }
        if let Some(s) = sprint_ids.as_ref().filter(|v| !v.is_empty()) {
            bits.push(format!("sprints=[{}]", s.join(",")));
        }
        let summary = if bits.is_empty() {
            "no filters (showing whatever the tab last had)".to_string()
        } else {
            bits.join(", ")
        };
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Opening Jira tab with {}.",
            summary
        ))]))
    }

    #[tool(
        description = "Open the Sentry top-level tab with optional filters — same controls as the tab's filter bar. `projects` scopes to one or more Sentry project slugs, `status` is unresolved/resolved/ignored/all, `level` is fatal/error/warning/info/debug/all, `environment` is the env slug (e.g. `production`), `search` is free-text appended to Sentry's query. Omit a parameter to leave it untouched. Use whenever the user asks for a SPECIFIC slice of Sentry issues (\"production crashes\", \"unresolved errors in checkout-web\") instead of just \"open Sentry\"."
    )]
    async fn open_sentry_tab(
        &self,
        Parameters(OpenSentryTabParams { projects, search, status, level, environment }): Parameters<OpenSentryTabParams>,
    ) -> Result<CallToolResult, ErrorData> {
        if let Some(s) = status.as_deref() {
            validate_one_of(s, VALID_SENTRY_STATUSES, "status")?;
        }
        if let Some(l) = level.as_deref() {
            validate_one_of(l, VALID_SENTRY_LEVELS, "level")?;
        }
        let mut bits: Vec<String> = Vec::new();
        if let Some(p) = projects.as_ref().filter(|v| !v.is_empty()) {
            bits.push(format!("projects=[{}]", p.join(",")));
        }
        if let Some(q) = search.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            bits.push(format!("search=\"{}\"", q));
        }
        if let Some(s) = status.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            bits.push(format!("status={}", s));
        }
        if let Some(l) = level.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            bits.push(format!("level={}", l));
        }
        if let Some(e) = environment.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            bits.push(format!("env={}", e));
        }
        let summary = if bits.is_empty() {
            "no filters (showing whatever the tab last had)".to_string()
        } else {
            bits.join(", ")
        };
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Opening Sentry tab with {}.",
            summary
        ))]))
    }

    #[tool(
        description = "Apply filters to a GitHub workbench column (kind=`github`). Identify the column by `instance_name` (art-name like \"Petra\") or `instance_id`. `repo` is `owner/name` (or empty string `\"\"` to clear → all repos), `mode` is involving/authored/review_requested/assigned/user/all, `search` is free-text, `custom_user` is a GitHub login (only used when `mode=user`). Pass only the keys you want to change; omitted keys keep their current value. Use this instead of `add_workbench_instance kind=github` when a column already exists — \"show authored PRs in efficiently in the github column\", \"filter Petra to only review-requested\"."
    )]
    async fn set_github_column(
        &self,
        Parameters(SetGithubColumnParams { instance_name, instance_id, repo, mode, search, custom_user }): Parameters<SetGithubColumnParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let label = require_instance_label(instance_name.as_deref(), instance_id.as_deref())?;
        if let Some(m) = mode.as_deref() {
            validate_one_of(m, VALID_GH_FILTER_MODES, "mode")?;
        }
        let mut bits = changed_filter_bits();
        if let Some(r) = repo {
            bits.push_kv("repo", &r);
        }
        if let Some(m) = mode {
            bits.push_kv("mode", &m);
        }
        if let Some(s) = search {
            bits.push_kv("search", &s);
        }
        if let Some(u) = custom_user {
            bits.push_kv("custom_user", &u);
        }
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Updating github column `{}`: {}.",
            label,
            bits.summary("no filter changes")
        ))]))
    }

    #[tool(
        description = "Apply filters to a Jira workbench column (kind=`jira`). Identify by `instance_name` or `instance_id`. `project_key` (empty string clears), `status_name`, `search`, `board_ids`, `sprint_ids` — same semantics as `open_jira_tab`. Pass only what you want to change. Use when a Jira column already exists and the user asks for a different scope (\"narrow Mona-Lisa to DEVOPS\", \"show in-review only in the jira column\")."
    )]
    async fn set_jira_column(
        &self,
        Parameters(SetJiraColumnParams { instance_name, instance_id, project_key, status_name, search, board_ids, sprint_ids }): Parameters<SetJiraColumnParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let label = require_instance_label(instance_name.as_deref(), instance_id.as_deref())?;
        let mut bits = changed_filter_bits();
        if let Some(p) = project_key {
            bits.push_kv("project", &p);
        }
        if let Some(s) = status_name {
            bits.push_kv("status", &s);
        }
        if let Some(q) = search {
            bits.push_kv("search", &q);
        }
        if let Some(b) = board_ids {
            bits.push_kv(
                "boards",
                &format!("[{}]", b.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(",")),
            );
        }
        if let Some(s) = sprint_ids {
            bits.push_kv("sprints", &format!("[{}]", s.join(",")));
        }
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Updating jira column `{}`: {}.",
            label,
            bits.summary("no filter changes")
        ))]))
    }

    #[tool(
        description = "Apply filters to a Sentry workbench column (kind=`sentry`). Identify by `instance_name` or `instance_id`. `projects`, `search`, `status`, `level`, `environment` — same semantics as `open_sentry_tab`. Pass only what you want to change. Use to retarget an existing Sentry column (\"only show production fatals in the sentry column\")."
    )]
    async fn set_sentry_column(
        &self,
        Parameters(SetSentryColumnParams { instance_name, instance_id, projects, search, status, level, environment }): Parameters<SetSentryColumnParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let label = require_instance_label(instance_name.as_deref(), instance_id.as_deref())?;
        if let Some(s) = status.as_deref() {
            validate_one_of(s, VALID_SENTRY_STATUSES, "status")?;
        }
        if let Some(l) = level.as_deref() {
            validate_one_of(l, VALID_SENTRY_LEVELS, "level")?;
        }
        let mut bits = changed_filter_bits();
        if let Some(p) = projects {
            bits.push_kv("projects", &format!("[{}]", p.join(",")));
        }
        if let Some(q) = search {
            bits.push_kv("search", &q);
        }
        if let Some(s) = status {
            bits.push_kv("status", &s);
        }
        if let Some(l) = level {
            bits.push_kv("level", &l);
        }
        if let Some(e) = environment {
            bits.push_kv("env", &e);
        }
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Updating sentry column `{}`: {}.",
            label,
            bits.summary("no filter changes")
        ))]))
    }

    #[tool(
        description = "Change the open folder of an EXISTING editor column. Use this when the user says \"switch the editor to /path\" — do NOT use add_workbench_instance, which creates a new column. Identify the editor by `instance_name` (the art-name like \"Sagrada-Familia\" shown in the workbench bar) or `instance_id`. If the editor has linked agent sessions, their cwd is auto-updated to match (no separate set_agent_cwd call needed)."
    )]
    async fn set_editor_repo_path(
        &self,
        Parameters(SetEditorRepoPathParams { instance_name, instance_id, repo_path }): Parameters<SetEditorRepoPathParams>,
    ) -> Result<CallToolResult, ErrorData> {
        if repo_path.trim().is_empty() {
            return Err(ErrorData::invalid_params("repo_path is required", None));
        }
        let by_name = instance_name.as_deref().map(str::trim).filter(|s| !s.is_empty());
        let by_id = instance_id.as_deref().map(str::trim).filter(|s| !s.is_empty());
        if by_name.is_none() && by_id.is_none() {
            return Err(ErrorData::invalid_params(
                "either `instance_name` or `instance_id` must be provided",
                None,
            ));
        }
        let label = by_name.or(by_id).unwrap_or("editor");
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Setting editor `{}` repo path → `{}`. Linked agent sessions (if any) update too.",
            label,
            repo_path.trim()
        ))]))
    }

    #[tool(
        description = "Change an agent session's cwd (working directory). Use when the user says \"switch yourself to /path\" / \"point Claude at /repo\" / \"have the cursor agent work on X\". For yourself, pass `target=\"self\"` — the change takes effect on your NEXT turn (the current one keeps the old cwd it spawned with). For another agent column, pass `instance_name` (e.g. \"Mona-Lisa\") or `instance_id`. Do NOT use this to create a new agent — use add_workbench_instance for that."
    )]
    async fn set_agent_cwd(
        &self,
        Parameters(SetAgentCwdParams { target, instance_name, instance_id, repo_path }): Parameters<SetAgentCwdParams>,
    ) -> Result<CallToolResult, ErrorData> {
        if repo_path.trim().is_empty() {
            return Err(ErrorData::invalid_params("repo_path is required", None));
        }
        let is_self = target.as_deref().map(str::trim).map(|s| s.eq_ignore_ascii_case("self")).unwrap_or(false);
        if !is_self {
            let by_name = instance_name.as_deref().map(str::trim).filter(|s| !s.is_empty());
            let by_id = instance_id.as_deref().map(str::trim).filter(|s| !s.is_empty());
            if by_name.is_none() && by_id.is_none() {
                return Err(ErrorData::invalid_params(
                    "for non-self target, either `instance_name` or `instance_id` must be provided",
                    None,
                ));
            }
            let label = by_name.or(by_id).unwrap_or("agent");
            return Ok(CallToolResult::success(vec![Content::text(format!(
                "Setting agent `{}` cwd → `{}` (effective from its next turn).",
                label,
                repo_path.trim()
            ))]));
        }
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Setting MY (self) cwd → `{}`. Effective from my next turn — the current one is already running with the old cwd.",
            repo_path.trim()
        ))]))
    }

    #[tool(
        description = "Re-list the workbenches and their column instances. The Forgehold runtime injects this state into your system prompt at the start of every turn, so you usually already know it. Call this only if you suspect the preamble is stale (e.g. you just added a column and want to confirm its name/id), or if the user references something that wasn't in the preamble."
    )]
    async fn list_instances(&self) -> Result<CallToolResult, ErrorData> {
        Ok(CallToolResult::success(vec![Content::text(
            "Forgehold injects the current workbench / instance map into your system prompt at the start of every turn. Re-read it for the latest state. (This tool is a placeholder — the frontend interceptor doesn't need to mutate anything for it; the actual data lives in the system prompt preamble.)".to_string(),
        )]))
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
             - switch_view — change the top-level tab (workbench / github / jira / sentry / rules / connections / settings). Use ONLY when the user wants the bare tab without any specific scope; if they name a repo / project / sprint / Sentry filter, prefer the targeted opener below.\n\
             - open_github_repo — GitHub tab + specific repo on a section (code/pulls/issues/actions/releases). Pass `path` with `section=code` to drill into a file (\"open src/lib/auth.ts in efficiently\").\n\
             - open_jira_tab — Jira tab + Jira filters (project_key / status_name / search / board_ids / sprint_ids). Use for \"my Jira tickets in DEVOPS\", \"sprint 160 in Jira\".\n\
             - open_sentry_tab — Sentry tab + Sentry filters (projects / search / status / level / environment). Use for \"production crashes\", \"unresolved errors in checkout-web\".\n\
             \n\
             ## Workbench manipulation\n\
             - new_workbench — create a fresh workbench tab (with optional name). Activates it by default.\n\
             - switch_workbench — switch active workbench by name or index.\n\
             - add_workbench_instance — add a NEW column (github/jira/sentry/claude/cursor/editor). Use ONLY when the user explicitly asks for a new/another column. Do NOT use for \"switch the editor to /path\" — that's set_editor_repo_path.\n\
             - set_editor_repo_path — change an EXISTING editor's open folder. Linked agents auto-follow.\n\
             - set_agent_cwd — change an agent session's cwd. `target=self` for yourself, or `instance_name` for another column. Effective from the next turn.\n\
             - focus_workbench_instance — scroll-to + highlight an existing column (creates one if none exists).\n\
             - list_instances — re-read the workbench layout if you think your preamble is stale.\n\
             - add_editor_instance — DEPRECATED, use add_workbench_instance with kind=`editor`. Kept for back-compat.\n\
             - set_github_column / set_jira_column / set_sentry_column — patch filters on an EXISTING workbench column (identify by `instance_name` or `instance_id`). Pass only the keys you want to change; omitted keys are preserved. Pass empty string `\"\"` to clear a single-value filter (e.g. `repo=\"\"` = all repos). Use these to retarget a column the user already has open.\n\
             \n\
             ## Sources\n\
             - open_connect_modal — surface the connect/status modal for any source/agent (use when the user asks about an integration that isn't connected yet — e.g. they mention Slack and you can see it's not in their connected list).\n\
             \n\
             # When to chain calls\n\
             These tools compose. \"Open the actions tab for forge\" → open_github_repo(owner=…, repo=forge, section=actions) — one call, no need to switch_view first. \"Make a new workbench called Hotfix and add a Claude column there\" → new_workbench + add_workbench_instance(kind=claude). Don't ask for confirmation — these are harmless navigation that gives the user the same view they'd get clicking through manually."
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
