//! woom-github — MCP sidecar exposing GitHub as tools to Claude Code.
//!
//! Auth: personal access token via env var GITHUB_TOKEN.
//!
//! Exposes read tools (search, get_pr, diff, files, comments, commits,
//! releases, workflow runs) and write tools (add_comment, submit_review,
//! merge_pr) that surface approval cards in the Woom UI.

use anyhow::Context;
use rmcp::{
    ErrorData, ServerHandler, ServiceExt,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, Content, ServerCapabilities, ServerInfo},
    schemars, tool, tool_handler, tool_router,
    transport::stdio,
};
use serde::Deserialize;
use urlencoding;

const USER_AGENT: &str = concat!("woom-github/", env!("CARGO_PKG_VERSION"));
const API_BASE: &str = "https://api.github.com";

#[derive(Clone)]
struct GhCreds {
    token: String,
}

impl GhCreds {
    fn from_env() -> anyhow::Result<Self> {
        let token = std::env::var("GITHUB_TOKEN")
            .context("GITHUB_TOKEN env var is required")?
            .trim()
            .to_string();
        if token.is_empty() {
            anyhow::bail!("GITHUB_TOKEN must be non-empty");
        }
        Ok(Self { token })
    }
}

#[derive(Clone)]
struct Gh {
    creds: GhCreds,
    http: reqwest::Client,
    #[allow(dead_code)] // read by `#[tool_handler]` macro expansion
    tool_router: ToolRouter<Self>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct PrRef {
    /// Repository owner (user or org), e.g. "anthropics".
    owner: String,
    /// Repository name, e.g. "claude-code".
    repo: String,
    /// Pull request number.
    number: u64,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct RepoRef {
    /// Repository owner (user or org), e.g. "anthropics".
    owner: String,
    /// Repository name, e.g. "claude-code".
    repo: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct RepoTreeParams {
    owner: String,
    repo: String,
    /// Git ref: branch name, tag, or commit SHA. Defaults to HEAD (the default branch).
    #[serde(default)]
    reference: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct GetFileParams {
    owner: String,
    repo: String,
    /// Path relative to the repo root, e.g. "src/lib.rs".
    path: String,
    /// Git ref — branch / tag / SHA. Defaults to HEAD.
    #[serde(default)]
    reference: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ListCommitsParams {
    owner: String,
    repo: String,
    #[serde(default)]
    reference: Option<String>,
    /// Max commits to return (default 20, cap 100).
    #[serde(default)]
    limit: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ListLimitParams {
    owner: String,
    repo: String,
    /// Max items to return (default 30, cap 100).
    #[serde(default)]
    limit: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AddCommentParams {
    owner: String,
    repo: String,
    /// Issue or PR number (GitHub issue-comments endpoint handles both).
    number: u64,
    /// Markdown body.
    body: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SubmitReviewParams {
    owner: String,
    repo: String,
    number: u64,
    /// One of: APPROVE, REQUEST_CHANGES, COMMENT.
    event: String,
    /// Required for REQUEST_CHANGES; optional otherwise.
    #[serde(default)]
    body: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct MergePrParams {
    owner: String,
    repo: String,
    number: u64,
    /// One of: merge, squash, rebase. Defaults to "merge".
    #[serde(default)]
    method: Option<String>,
    /// Optional commit title for the merge commit.
    #[serde(default)]
    commit_title: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ProposeCommitParams {
    /// The commit subject line. Keep it concise (imperative, ≤72 chars).
    message: String,
    /// Optional extended commit body (markdown allowed).
    #[serde(default)]
    body: Option<String>,
    /// Whether to push the branch after the commit. Default: true.
    #[serde(default)]
    push: Option<bool>,
    /// A short free-form note for the user explaining what the commit contains.
    #[serde(default)]
    note: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ProposeBashParams {
    /// The exact shell command you want to run (single line, will be run via `sh -c`).
    command: String,
    /// Short free-form explanation for the user of why this command is needed.
    #[serde(default)]
    reason: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ProposeSwitchCwdParams {
    /// Absolute local path to switch the Claude session's working directory to.
    path: String,
    /// A short free-form note for the user explaining why you want to switch.
    #[serde(default)]
    reason: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SearchParams {
    /// GitHub search syntax forwarded verbatim to `/search/issues`. Examples:
    /// `is:pr is:merged DEVOPS-414 org:Efficiently-Dev`,
    /// `is:open author:nik repo:acme/api`,
    /// `is:issue label:bug created:>2025-01-01 org:acme`.
    /// See https://docs.github.com/en/search-github/searching-on-github/searching-issues-and-pull-requests
    query: String,
    /// Max items to return (default 25, cap 100).
    #[serde(default)]
    limit: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ListReposParams {
    /// Max repos to return (default 30, cap 100).
    #[serde(default)]
    limit: Option<u32>,
    /// Filter by name substring (case-insensitive). Omit for all.
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ProposePrParams {
    /// Proposed PR title.
    title: String,
    /// Proposed PR body (markdown).
    #[serde(default)]
    body: Option<String>,
    /// Base branch to target. Omit to use repo default (usually `main`).
    #[serde(default)]
    base: Option<String>,
    /// Open as draft PR. Default: false.
    #[serde(default)]
    draft: Option<bool>,
    /// Optional free-form note for the user.
    #[serde(default)]
    note: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct EditPrParams {
    owner: String,
    repo: String,
    number: u64,
    /// New title. Omit to leave unchanged.
    #[serde(default)]
    title: Option<String>,
    /// New body (markdown). Omit to leave unchanged. Pass an empty
    /// string to clear the body.
    #[serde(default)]
    body: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ReviewersParams {
    owner: String,
    repo: String,
    number: u64,
    /// User logins to add or remove (without leading @).
    #[serde(default)]
    user_logins: Option<Vec<String>>,
    /// Team slugs (without org prefix). Common case is empty.
    #[serde(default)]
    team_slugs: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct LabelsParams {
    owner: String,
    repo: String,
    /// Issue OR PR number (GitHub treats them the same on labels).
    number: u64,
    /// Label names. For add: case-sensitive, exact match required;
    /// non-existent labels are auto-created on add. For remove:
    /// missing labels are silently skipped.
    labels: Vec<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AssigneesParams {
    owner: String,
    repo: String,
    number: u64,
    /// User logins (no leading @). Both PRs and issues accepted.
    logins: Vec<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SetPrDraftParams {
    owner: String,
    repo: String,
    number: u64,
    /// `true` → convert to draft. `false` → mark ready for review.
    draft: bool,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SetPrStateParams {
    owner: String,
    repo: String,
    number: u64,
    /// `open` to reopen a closed PR/issue, `closed` to close.
    state: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ListCheckRunsParams {
    owner: String,
    repo: String,
    /// Branch name, tag, or commit SHA. For PR checks pass the PR's
    /// head SHA (visible on `get_pr` as `head_sha`).
    #[serde(rename = "ref", alias = "ref_name", alias = "sha")]
    ref_name: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct WorkflowRunParams {
    owner: String,
    repo: String,
    /// The workflow run id (NOT the workflow id itself). Visible on
    /// `list_workflow_runs` results.
    run_id: u64,
}

#[tool_router]
impl Gh {
    fn new(creds: GhCreds) -> anyhow::Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .context("build reqwest client")?;
        Ok(Self { creds, http, tool_router: Self::tool_router() })
    }

    #[tool(
        description = "Fetch GitHub PR detail (title, author, state, mergeable, branches, body). Use this as the SINGLE call for most PR questions — title/author/state/branches/body are usually all you need. Call get_pr_diff / get_pr_files / get_pr_comments only when the user explicitly asks about diff content, file list, or discussion respectively. Do NOT pre-emptively call all four for the same PR — that's 4× the context cost for one answer."
    )]
    async fn get_pr(
        &self,
        Parameters(PrRef { owner, repo, number }): Parameters<PrRef>,
    ) -> Result<CallToolResult, ErrorData> {
        match self.fetch_pr(&owner, &repo, number).await {
            Ok(t) => Ok(CallToolResult::success(vec![Content::text(t)])),
            Err(e) => Err(ErrorData::internal_error(e.to_string(), None)),
        }
    }

    #[tool(description = "Fetch the full unified diff for a PR (raw patch text). LARGE response — call only when the user explicitly asks to see the diff or you need to reason about specific code changes. For 'what does this PR do' questions, get_pr's body field is usually sufficient.")]
    async fn get_pr_diff(
        &self,
        Parameters(PrRef { owner, repo, number }): Parameters<PrRef>,
    ) -> Result<CallToolResult, ErrorData> {
        match self.fetch_pr_diff(&owner, &repo, number).await {
            Ok(t) => Ok(CallToolResult::success(vec![Content::text(t)])),
            Err(e) => Err(ErrorData::internal_error(e.to_string(), None)),
        }
    }

    #[tool(
        description = "List files changed in a PR with status (added/modified/removed), additions, deletions, and a patch per file. Prefer this over get_pr_diff when you only need 'which files changed' — it returns one row per file instead of the full patch. If the user wants only file names without per-file patches, mention that in the reply rather than calling this AND get_pr_diff."
    )]
    async fn get_pr_files(
        &self,
        Parameters(PrRef { owner, repo, number }): Parameters<PrRef>,
    ) -> Result<CallToolResult, ErrorData> {
        match self.fetch_pr_files(&owner, &repo, number).await {
            Ok(t) => Ok(CallToolResult::success(vec![Content::text(t)])),
            Err(e) => Err(ErrorData::internal_error(e.to_string(), None)),
        }
    }

    #[tool(
        description = "Fetch the discussion on a PR: both issue-level comments (general conversation) and review comments (inline on specific lines). Call only when the user asks about review feedback, blockers, or thread context — not as a default companion to get_pr."
    )]
    async fn get_pr_comments(
        &self,
        Parameters(PrRef { owner, repo, number }): Parameters<PrRef>,
    ) -> Result<CallToolResult, ErrorData> {
        match self.fetch_pr_comments(&owner, &repo, number).await {
            Ok(t) => Ok(CallToolResult::success(vec![Content::text(t)])),
            Err(e) => Err(ErrorData::internal_error(e.to_string(), None)),
        }
    }

    #[tool(
        description = "List the complete file tree of a GitHub repository at a given ref. Returns paths with type (blob/tree/commit) and sizes. Use this to explore unfamiliar repos before reading specific files."
    )]
    async fn list_tree(
        &self,
        Parameters(RepoTreeParams { owner, repo, reference }): Parameters<RepoTreeParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let r = reference.unwrap_or_else(|| "HEAD".into());
        match self.fetch_tree(&owner, &repo, &r).await {
            Ok(t) => Ok(CallToolResult::success(vec![Content::text(t)])),
            Err(e) => Err(ErrorData::internal_error(e.to_string(), None)),
        }
    }

    #[tool(
        description = "Fetch the contents of a single file in a GitHub repository at a given ref. Returns the decoded text (or a note for binary files). Use after list_tree to zoom in."
    )]
    async fn get_file(
        &self,
        Parameters(GetFileParams { owner, repo, path, reference }): Parameters<GetFileParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let r = reference.unwrap_or_else(|| "HEAD".into());
        match self.fetch_file(&owner, &repo, &path, &r).await {
            Ok(t) => Ok(CallToolResult::success(vec![Content::text(t)])),
            Err(e) => Err(ErrorData::internal_error(e.to_string(), None)),
        }
    }

    #[tool(
        description = "List recent commits on a branch of a GitHub repository. Each entry includes sha, author, date, and subject line."
    )]
    async fn list_commits(
        &self,
        Parameters(ListCommitsParams { owner, repo, reference, limit }): Parameters<ListCommitsParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let r = reference.unwrap_or_else(|| "HEAD".into());
        let n = limit.unwrap_or(20).min(100);
        match self.fetch_commits(&owner, &repo, &r, n).await {
            Ok(t) => Ok(CallToolResult::success(vec![Content::text(t)])),
            Err(e) => Err(ErrorData::internal_error(e.to_string(), None)),
        }
    }

    #[tool(
        description = "List published GitHub releases (tag, title, date, body). Cap on `limit` is 100."
    )]
    async fn list_releases(
        &self,
        Parameters(ListLimitParams { owner, repo, limit }): Parameters<ListLimitParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let n = limit.unwrap_or(30).min(100);
        match self.fetch_releases(&owner, &repo, n).await {
            Ok(t) => Ok(CallToolResult::success(vec![Content::text(t)])),
            Err(e) => Err(ErrorData::internal_error(e.to_string(), None)),
        }
    }

    #[tool(
        description = "List recent GitHub Actions workflow runs (name, status, conclusion, branch, actor, time)."
    )]
    async fn list_workflow_runs(
        &self,
        Parameters(ListLimitParams { owner, repo, limit }): Parameters<ListLimitParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let n = limit.unwrap_or(30).min(100);
        match self.fetch_workflow_runs(&owner, &repo, n).await {
            Ok(t) => Ok(CallToolResult::success(vec![Content::text(t)])),
            Err(e) => Err(ErrorData::internal_error(e.to_string(), None)),
        }
    }

    #[tool(description = "Fetch the repository README as markdown text.")]
    async fn get_readme(
        &self,
        Parameters(RepoRef { owner, repo }): Parameters<RepoRef>,
    ) -> Result<CallToolResult, ErrorData> {
        match self.fetch_readme(&owner, &repo).await {
            Ok(t) => Ok(CallToolResult::success(vec![Content::text(t)])),
            Err(e) => Err(ErrorData::internal_error(e.to_string(), None)),
        }
    }

    #[tool(
        description = "Post a new issue / PR comment on behalf of the user. Returns the created comment's URL."
    )]
    async fn add_comment(
        &self,
        Parameters(AddCommentParams { owner, repo, number, body }): Parameters<AddCommentParams>,
    ) -> Result<CallToolResult, ErrorData> {
        match self.post_comment(&owner, &repo, number, &body).await {
            Ok(t) => Ok(CallToolResult::success(vec![Content::text(t)])),
            Err(e) => Err(ErrorData::internal_error(e.to_string(), None)),
        }
    }

    #[tool(
        description = "Submit a PR review. `event` = APPROVE | REQUEST_CHANGES | COMMENT. `body` is optional for APPROVE/COMMENT, required for REQUEST_CHANGES."
    )]
    async fn submit_review(
        &self,
        Parameters(SubmitReviewParams { owner, repo, number, event, body }): Parameters<SubmitReviewParams>,
    ) -> Result<CallToolResult, ErrorData> {
        match self.post_review(&owner, &repo, number, &event, body.as_deref()).await {
            Ok(t) => Ok(CallToolResult::success(vec![Content::text(t)])),
            Err(e) => Err(ErrorData::internal_error(e.to_string(), None)),
        }
    }

    #[tool(
        description = "Merge a pull request. `method` = merge | squash | rebase (default merge). Fails if the PR isn't mergeable."
    )]
    async fn merge_pr(
        &self,
        Parameters(MergePrParams { owner, repo, number, method, commit_title }): Parameters<MergePrParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let m = method.unwrap_or_else(|| "merge".into());
        match self
            .do_merge(&owner, &repo, number, &m, commit_title.as_deref())
            .await
        {
            Ok(t) => Ok(CallToolResult::success(vec![Content::text(t)])),
            Err(e) => Err(ErrorData::internal_error(e.to_string(), None)),
        }
    }

    #[tool(
        description = "Propose a commit. Surfaces an editable approval card in Woom and BLOCKS until the user approves (commit + push runs) or dismisses. The tool's response is the actual outcome (commit hash, push status, or error) — react to it directly in this same turn."
    )]
    async fn propose_commit(
        &self,
        Parameters(ProposeCommitParams { message, body, push, note }): Parameters<ProposeCommitParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let push_s = push.unwrap_or(true);
        let params = serde_json::json!({
            "message": message,
            "body": body,
            "push": push_s,
            "note": note,
        });
        let fallback = format!(
            "Commit proposal queued.\nsubject: {}\npush: {}\nbody: {}\nnote: {}",
            message,
            push_s,
            body.as_deref().unwrap_or("(none)"),
            note.as_deref().unwrap_or("(none)"),
        );
        run_or_fallback("commit", params, fallback).await
    }

    #[tool(
        description = "Propose a state-changing shell command (`git switch/merge/push/pull/reset/rebase`, `rm`, `mv`, `npm install`, migrations, deploys, etc.). Surfaces an editable approval card in Woom and BLOCKS until the user approves (command runs) or dismisses. The tool's response is the actual stdout/stderr + exit code — read it and continue this same turn. Read-only commands (git status, ls, cat, grep) use the regular Bash tool, not this one."
    )]
    async fn propose_bash(
        &self,
        Parameters(ProposeBashParams { command, reason }): Parameters<ProposeBashParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let params = serde_json::json!({
            "command": command,
            "reason": reason,
        });
        let fallback = format!(
            "Bash command queued.\ncommand: {}\nreason: {}",
            command,
            reason.as_deref().unwrap_or("(none)"),
        );
        run_or_fallback("bash", params, fallback).await
    }

    #[tool(
        description = "Propose switching the current session's working directory. Surfaces an approval card in Woom and BLOCKS until the user approves (cwd switches) or dismisses. The tool's response is the actual outcome — react and continue in this same turn."
    )]
    async fn propose_switch_cwd(
        &self,
        Parameters(ProposeSwitchCwdParams { path, reason }): Parameters<ProposeSwitchCwdParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let params = serde_json::json!({
            "path": path,
            "reason": reason,
        });
        let fallback = format!(
            "cwd switch proposal queued.\npath: {}\nreason: {}",
            path,
            reason.as_deref().unwrap_or("(none)"),
        );
        run_or_fallback("switch_cwd", params, fallback).await
    }

    #[tool(
        description = "Search GitHub pull requests across repos / orgs. Returns id, number, repo, title, state, author, draft flag, dates, url. Pass full GitHub search syntax (e.g. `is:pr is:merged DEVOPS-414`). One call covers all matches across orgs/repos — see the search-discipline block in your system context for canonical patterns."
    )]
    async fn search_prs(
        &self,
        Parameters(SearchParams { query, limit }): Parameters<SearchParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let max = limit.unwrap_or(25).min(100);
        let q = format!("is:pr {}", query.trim());
        match self.fetch_search(&q, max).await {
            Ok(t) => Ok(CallToolResult::success(vec![Content::text(t)])),
            Err(e) => Err(ErrorData::internal_error(e.to_string(), None)),
        }
    }

    #[tool(
        description = "Search GitHub issues across repos / orgs. Same syntax as search_prs but defaults to `is:issue` (e.g. `is:issue is:open label:bug updated:>2025-04-18`). One call covers all matches — see the search-discipline block in your system context."
    )]
    async fn search_issues(
        &self,
        Parameters(SearchParams { query, limit }): Parameters<SearchParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let max = limit.unwrap_or(25).min(100);
        let q = format!("is:issue {}", query.trim());
        match self.fetch_search(&q, max).await {
            Ok(t) => Ok(CallToolResult::success(vec![Content::text(t)])),
            Err(e) => Err(ErrorData::internal_error(e.to_string(), None)),
        }
    }

    #[tool(
        description = "List GitHub repos accessible by the authenticated user (own + collaborator + org). Returns owner/name, default branch, private flag, language, last push. Useful when the user says \"the repo I'm working on\" or asks to scope a search to a particular repo and you don't already know its slug."
    )]
    async fn list_repos(
        &self,
        Parameters(ListReposParams { limit, name }): Parameters<ListReposParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let max = limit.unwrap_or(30).min(100);
        let needle = name.as_deref().map(str::trim).filter(|s| !s.is_empty());
        match self.fetch_repos(max, needle).await {
            Ok(t) => Ok(CallToolResult::success(vec![Content::text(t)])),
            Err(e) => Err(ErrorData::internal_error(e.to_string(), None)),
        }
    }

    #[tool(
        description = "Propose a pull request for the user to review. Use after the commit is made (or will be made) and the user asked to open a PR. Does NOT create the PR — it surfaces an editable PR card in the Woom UI. Only call when the user asked you to open a PR."
    )]
    async fn propose_pr(
        &self,
        Parameters(ProposePrParams { title, body, base, draft, note }): Parameters<ProposePrParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let params = serde_json::json!({
            "title": title,
            "body": body,
            "base": base,
            "draft": draft.unwrap_or(false),
            "note": note,
        });
        let fallback = format!(
            "PR proposal queued.\ntitle: {}\nbase: {}\ndraft: {}",
            title,
            base.as_deref().unwrap_or("(repo default)"),
            draft.unwrap_or(false),
        );
        run_or_fallback("pr", params, fallback).await
    }

    #[tool(
        description = "Edit an existing PR's title and/or body in place. Either field can be omitted to leave it unchanged. Use when the user asks to rename a PR or rewrite its description (e.g. \"update PR #1234 to say Part 1 + Part 2\"). For closing/reopening, use set_pr_state instead. For draft toggling, use set_pr_draft."
    )]
    async fn edit_pr(
        &self,
        Parameters(EditPrParams { owner, repo, number, title, body }): Parameters<EditPrParams>,
    ) -> Result<CallToolResult, ErrorData> {
        if title.is_none() && body.is_none() {
            return Err(ErrorData::invalid_params(
                "edit_pr: at least one of `title` or `body` must be provided".to_string(),
                None,
            ));
        }
        match self.do_edit_pr(&owner, &repo, number, title.as_deref(), body.as_deref()).await {
            Ok(t) => Ok(CallToolResult::success(vec![Content::text(t)])),
            Err(e) => Err(ErrorData::internal_error(e.to_string(), None)),
        }
    }

    #[tool(
        description = "Add reviewers to a PR. Pass `user_logins` (array of GitHub usernames, no leading @) and/or `team_slugs` (array of team slugs without the org prefix). Additive — existing reviewers stay. GitHub silently de-dupes already-requested reviewers."
    )]
    async fn request_reviewers(
        &self,
        Parameters(ReviewersParams { owner, repo, number, user_logins, team_slugs }): Parameters<ReviewersParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let users = user_logins.unwrap_or_default();
        let teams = team_slugs.unwrap_or_default();
        if users.is_empty() && teams.is_empty() {
            return Err(ErrorData::invalid_params(
                "request_reviewers: provide at least one of `user_logins` or `team_slugs`".to_string(),
                None,
            ));
        }
        match self.do_request_reviewers(&owner, &repo, number, &users, &teams).await {
            Ok(t) => Ok(CallToolResult::success(vec![Content::text(t)])),
            Err(e) => Err(ErrorData::internal_error(e.to_string(), None)),
        }
    }

    #[tool(
        description = "Remove reviewers from a PR. Mirror of request_reviewers — same `user_logins` / `team_slugs` shape."
    )]
    async fn remove_reviewers(
        &self,
        Parameters(ReviewersParams { owner, repo, number, user_logins, team_slugs }): Parameters<ReviewersParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let users = user_logins.unwrap_or_default();
        let teams = team_slugs.unwrap_or_default();
        if users.is_empty() && teams.is_empty() {
            return Err(ErrorData::invalid_params(
                "remove_reviewers: provide at least one of `user_logins` or `team_slugs`".to_string(),
                None,
            ));
        }
        match self.do_remove_reviewers(&owner, &repo, number, &users, &teams).await {
            Ok(t) => Ok(CallToolResult::success(vec![Content::text(t)])),
            Err(e) => Err(ErrorData::internal_error(e.to_string(), None)),
        }
    }

    #[tool(
        description = "Add labels to an issue or PR. Labels are auto-created if they don't exist (GitHub's default). Additive — existing labels stay. Pass `labels: [\"bug\", \"high-priority\"]`. PRs and issues use the same numbering for labels."
    )]
    async fn add_labels(
        &self,
        Parameters(LabelsParams { owner, repo, number, labels }): Parameters<LabelsParams>,
    ) -> Result<CallToolResult, ErrorData> {
        if labels.is_empty() {
            return Err(ErrorData::invalid_params(
                "add_labels: `labels` must be a non-empty array".to_string(),
                None,
            ));
        }
        match self.do_add_labels(&owner, &repo, number, &labels).await {
            Ok(t) => Ok(CallToolResult::success(vec![Content::text(t)])),
            Err(e) => Err(ErrorData::internal_error(e.to_string(), None)),
        }
    }

    #[tool(
        description = "Remove labels from an issue or PR. Per-label loop — labels not present on the issue are silently skipped (no error). Returns the labels that were actually removed."
    )]
    async fn remove_labels(
        &self,
        Parameters(LabelsParams { owner, repo, number, labels }): Parameters<LabelsParams>,
    ) -> Result<CallToolResult, ErrorData> {
        if labels.is_empty() {
            return Err(ErrorData::invalid_params(
                "remove_labels: `labels` must be a non-empty array".to_string(),
                None,
            ));
        }
        match self.do_remove_labels(&owner, &repo, number, &labels).await {
            Ok(t) => Ok(CallToolResult::success(vec![Content::text(t)])),
            Err(e) => Err(ErrorData::internal_error(e.to_string(), None)),
        }
    }

    #[tool(
        description = "Add assignees (users responsible for the issue/PR) by login. Same numbering applies to PRs. Additive — existing assignees stay. GitHub silently ignores users that already are assignees."
    )]
    async fn add_assignees(
        &self,
        Parameters(AssigneesParams { owner, repo, number, logins }): Parameters<AssigneesParams>,
    ) -> Result<CallToolResult, ErrorData> {
        if logins.is_empty() {
            return Err(ErrorData::invalid_params(
                "add_assignees: `logins` must be a non-empty array".to_string(),
                None,
            ));
        }
        match self.do_add_assignees(&owner, &repo, number, &logins).await {
            Ok(t) => Ok(CallToolResult::success(vec![Content::text(t)])),
            Err(e) => Err(ErrorData::internal_error(e.to_string(), None)),
        }
    }

    #[tool(
        description = "Remove assignees from an issue/PR. Mirror of add_assignees."
    )]
    async fn remove_assignees(
        &self,
        Parameters(AssigneesParams { owner, repo, number, logins }): Parameters<AssigneesParams>,
    ) -> Result<CallToolResult, ErrorData> {
        if logins.is_empty() {
            return Err(ErrorData::invalid_params(
                "remove_assignees: `logins` must be a non-empty array".to_string(),
                None,
            ));
        }
        match self.do_remove_assignees(&owner, &repo, number, &logins).await {
            Ok(t) => Ok(CallToolResult::success(vec![Content::text(t)])),
            Err(e) => Err(ErrorData::internal_error(e.to_string(), None)),
        }
    }

    #[tool(
        description = "Convert a PR to draft (`draft: true`) or mark it ready for review (`draft: false`). Implemented via GitHub's GraphQL mutation since REST doesn't expose draft toggling."
    )]
    async fn set_pr_draft(
        &self,
        Parameters(SetPrDraftParams { owner, repo, number, draft }): Parameters<SetPrDraftParams>,
    ) -> Result<CallToolResult, ErrorData> {
        match self.do_set_pr_draft(&owner, &repo, number, draft).await {
            Ok(t) => Ok(CallToolResult::success(vec![Content::text(t)])),
            Err(e) => Err(ErrorData::internal_error(e.to_string(), None)),
        }
    }

    #[tool(
        description = "Open or close a PR/issue. `state: \"open\"` reopens, `state: \"closed\"` closes. For PRs, closing without merging discards them; use merge_pr if the intent is to merge."
    )]
    async fn set_pr_state(
        &self,
        Parameters(SetPrStateParams { owner, repo, number, state }): Parameters<SetPrStateParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let s = state.trim().to_ascii_lowercase();
        if s != "open" && s != "closed" {
            return Err(ErrorData::invalid_params(
                format!("set_pr_state: `state` must be \"open\" or \"closed\", got \"{state}\""),
                None,
            ));
        }
        match self.do_set_pr_state(&owner, &repo, number, &s).await {
            Ok(t) => Ok(CallToolResult::success(vec![Content::text(t)])),
            Err(e) => Err(ErrorData::internal_error(e.to_string(), None)),
        }
    }

    #[tool(
        description = "List CI/check runs for a commit ref (typically a PR's head SHA from get_pr). Returns each check's name, status (queued/in_progress/completed), conclusion (success/failure/etc), and details URL. Use to answer \"is the PR green?\" without paging through GitHub's UI."
    )]
    async fn list_check_runs(
        &self,
        Parameters(ListCheckRunsParams { owner, repo, ref_name }): Parameters<ListCheckRunsParams>,
    ) -> Result<CallToolResult, ErrorData> {
        match self.do_list_check_runs(&owner, &repo, &ref_name).await {
            Ok(t) => Ok(CallToolResult::success(vec![Content::text(t)])),
            Err(e) => Err(ErrorData::internal_error(e.to_string(), None)),
        }
    }

    #[tool(
        description = "Re-run a GitHub Actions workflow run by id. Triggers a fresh run with the same inputs; doesn't cancel the old one. Useful for flaky CI."
    )]
    async fn rerun_workflow(
        &self,
        Parameters(WorkflowRunParams { owner, repo, run_id }): Parameters<WorkflowRunParams>,
    ) -> Result<CallToolResult, ErrorData> {
        match self.do_rerun_workflow(&owner, &repo, run_id).await {
            Ok(t) => Ok(CallToolResult::success(vec![Content::text(t)])),
            Err(e) => Err(ErrorData::internal_error(e.to_string(), None)),
        }
    }

    #[tool(
        description = "Cancel an in-progress GitHub Actions workflow run by id. No-op if the run already finished."
    )]
    async fn cancel_workflow(
        &self,
        Parameters(WorkflowRunParams { owner, repo, run_id }): Parameters<WorkflowRunParams>,
    ) -> Result<CallToolResult, ErrorData> {
        match self.do_cancel_workflow(&owner, &repo, run_id).await {
            Ok(t) => Ok(CallToolResult::success(vec![Content::text(t)])),
            Err(e) => Err(ErrorData::internal_error(e.to_string(), None)),
        }
    }
}

#[tool_handler]
impl ServerHandler for Gh {
    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::default();
        info.capabilities = ServerCapabilities::builder().enable_tools().build();
        info.instructions = Some(
            "Access GitHub and propose local actions on behalf of the user.\n\n\
             READ: get_pr, get_pr_diff, get_pr_files, get_pr_comments, list_tree, get_file, list_commits, list_releases, list_workflow_runs, get_readme, list_check_runs.\n\n\
             WRITE (executes immediately — user has already given consent by asking): add_comment, submit_review, merge_pr, edit_pr (title/body), request_reviewers, remove_reviewers, add_labels, remove_labels, add_assignees, remove_assignees, set_pr_draft, set_pr_state (open/closed), rerun_workflow, cancel_workflow.\n\n\
             PROPOSE (queues an approval card in Woom UI, does nothing itself — use when the user asked you to commit/open-pr/switch-repo/run-a-command): propose_commit, propose_pr, propose_switch_cwd, propose_bash.\n\n\
             PR-EDIT GUIDE: rename PR → edit_pr; rewrite description → edit_pr; close/reopen → set_pr_state; convert to draft / mark ready → set_pr_draft; add CODEOWNERS / specific reviewers → request_reviewers. The agent should NOT push the user back to the GitHub UI for these — they're all wired here.\n\n\
             RULE: for any LOCAL command that modifies state (git switch/merge/push/pull/reset/rebase, rm, mv, npm install, migrations, deploys, etc.) call propose_bash instead of the regular Bash tool. Read-only commands (git status, ls, cat, grep, find, rg) can use Bash directly. propose_* calls are SYNCHRONOUS — they BLOCK until the user resolves the card and the action runs. The tool's response IS the actual outcome (commit hash, bash output, PR url, error stderr). React to the response in the SAME TURN — chain follow-up propose_* calls or finish, the agent's turn is fully under your control."
                .to_string(),
        );
        info
    }
}

impl Gh {
    fn req(&self, url: &str) -> reqwest::RequestBuilder {
        self.http
            .get(url)
            .bearer_auth(&self.creds.token)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
    }

    async fn fetch_search(&self, q: &str, per_page: u32) -> anyhow::Result<String> {
        // /search/issues handles both PRs and Issues; the `is:pr` /
        // `is:issue` qualifier already in `q` filters server-side.
        let url = format!(
            "{API_BASE}/search/issues?q={}&per_page={}&sort=updated",
            urlencoding::encode(q),
            per_page
        );
        let resp = self.req(&url).send().await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("GitHub search {} — {}", status, truncate(&body, 500));
        }
        let v: serde_json::Value = resp.json().await?;
        let items = v
            .get("items")
            .and_then(|x| x.as_array())
            .cloned()
            .unwrap_or_default();
        let total = v.get("total_count").and_then(|x| x.as_u64()).unwrap_or(0);
        if items.is_empty() {
            return Ok(format!("No results for `{}`.", q));
        }
        let mut out = format!(
            "{} of {} matches for `{}`:\n",
            items.len(),
            total,
            q
        );
        for it in &items {
            let number = it.get("number").and_then(|x| x.as_u64()).unwrap_or(0);
            let title = it.get("title").and_then(|x| x.as_str()).unwrap_or("");
            let state = it.get("state").and_then(|x| x.as_str()).unwrap_or("?");
            let url = it.get("html_url").and_then(|x| x.as_str()).unwrap_or("");
            // pull_request.merged_at distinguishes merged from just closed.
            let merged = it
                .get("pull_request")
                .and_then(|p| p.get("merged_at"))
                .map(|x| !x.is_null())
                .unwrap_or(false);
            let draft = it.get("draft").and_then(|x| x.as_bool()).unwrap_or(false);
            let user = it
                .get("user")
                .and_then(|u| u.get("login"))
                .and_then(|x| x.as_str())
                .unwrap_or("?");
            let updated = it
                .get("updated_at")
                .and_then(|x| x.as_str())
                .unwrap_or("");
            // Repo derived from html_url (`https://github.com/<owner>/<repo>/...`).
            let repo_slug = url
                .strip_prefix("https://github.com/")
                .and_then(|s| s.split('/').take(2).collect::<Vec<_>>().join("/").into())
                .unwrap_or_default();
            let kind = if it.get("pull_request").is_some() { "PR" } else { "Issue" };
            let state_label = if merged {
                "merged"
            } else if draft {
                "draft"
            } else {
                state
            };
            out.push_str(&format!(
                "- {} {}#{} [{}] {} (by @{}, updated {})\n  {}\n",
                kind, repo_slug, number, state_label, title, user, updated, url
            ));
        }
        Ok(out)
    }

    async fn fetch_repos(
        &self,
        per_page: u32,
        needle: Option<&str>,
    ) -> anyhow::Result<String> {
        // /user/repos returns repos the auth'd user owns OR collaborates on
        // OR can see via org membership. Sorted by recency by default.
        let url = format!(
            "{API_BASE}/user/repos?per_page={}&sort=pushed&affiliation=owner,collaborator,organization_member",
            per_page
        );
        let resp = self.req(&url).send().await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("GitHub /user/repos {} — {}", status, truncate(&body, 500));
        }
        let v: serde_json::Value = resp.json().await?;
        let items = v.as_array().cloned().unwrap_or_default();
        let needle_lower = needle.map(|s| s.to_lowercase());
        let mut out = String::new();
        let mut count = 0usize;
        for r in &items {
            let full = r.get("full_name").and_then(|x| x.as_str()).unwrap_or("?");
            if let Some(n) = &needle_lower {
                if !full.to_lowercase().contains(n) {
                    continue;
                }
            }
            let private = r.get("private").and_then(|x| x.as_bool()).unwrap_or(false);
            let lang = r.get("language").and_then(|x| x.as_str()).unwrap_or("");
            let default_branch = r
                .get("default_branch")
                .and_then(|x| x.as_str())
                .unwrap_or("");
            let pushed = r.get("pushed_at").and_then(|x| x.as_str()).unwrap_or("");
            out.push_str(&format!(
                "- {} ({}) · default={} · lang={} · pushed={}\n",
                full,
                if private { "private" } else { "public" },
                default_branch,
                if lang.is_empty() { "?" } else { lang },
                pushed
            ));
            count += 1;
        }
        if count == 0 {
            return Ok("No repositories matched.".into());
        }
        Ok(format!("{} repo(s):\n{}", count, out))
    }

    async fn fetch_pr(&self, owner: &str, repo: &str, number: u64) -> anyhow::Result<String> {
        let url = format!("{API_BASE}/repos/{owner}/{repo}/pulls/{number}");
        let resp = self.req(&url).send().await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("GitHub {} on {}: {}", status, url, truncate(&body, 500));
        }
        let v: serde_json::Value = resp.json().await?;
        Ok(format_pr(&v))
    }

    async fn fetch_pr_diff(&self, owner: &str, repo: &str, number: u64) -> anyhow::Result<String> {
        let url = format!("{API_BASE}/repos/{owner}/{repo}/pulls/{number}");
        let resp = self
            .http
            .get(&url)
            .bearer_auth(&self.creds.token)
            .header("Accept", "application/vnd.github.v3.diff")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .send()
            .await?;
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            anyhow::bail!("GitHub {} on {}: {}", status, url, truncate(&body, 500));
        }
        if body.len() > 200_000 {
            Ok(format!(
                "{}\n\n... [diff truncated at 200KB; use get_pr_files for a per-file view]",
                &body[..200_000]
            ))
        } else {
            Ok(body)
        }
    }

    async fn fetch_pr_files(
        &self,
        owner: &str,
        repo: &str,
        number: u64,
    ) -> anyhow::Result<String> {
        let mut out = String::new();
        let mut page = 1u32;
        loop {
            let url = format!(
                "{API_BASE}/repos/{owner}/{repo}/pulls/{number}/files?per_page=100&page={page}"
            );
            let resp = self.req(&url).send().await?;
            let status = resp.status();
            if !status.is_success() {
                let body = resp.text().await.unwrap_or_default();
                anyhow::bail!("GitHub {} on {}: {}", status, url, truncate(&body, 500));
            }
            let arr: Vec<serde_json::Value> = resp.json().await?;
            let count = arr.len();
            for f in &arr {
                let filename = f.get("filename").and_then(|v| v.as_str()).unwrap_or("?");
                let status = f.get("status").and_then(|v| v.as_str()).unwrap_or("?");
                let add = f.get("additions").and_then(|v| v.as_u64()).unwrap_or(0);
                let del = f.get("deletions").and_then(|v| v.as_u64()).unwrap_or(0);
                out.push_str(&format!("=== {} [{}, +{} -{}]\n", filename, status, add, del));
                if let Some(patch) = f.get("patch").and_then(|v| v.as_str()) {
                    out.push_str(patch);
                    out.push_str("\n\n");
                } else {
                    out.push_str("(no patch: binary or renamed without content change)\n\n");
                }
            }
            if count < 100 {
                break;
            }
            page += 1;
            if page > 10 {
                out.push_str("... [truncated at 1000 files]\n");
                break;
            }
        }
        Ok(out)
    }

    async fn fetch_tree(&self, owner: &str, repo: &str, reference: &str) -> anyhow::Result<String> {
        let url = format!("{API_BASE}/repos/{owner}/{repo}/git/trees/{reference}?recursive=1");
        let resp = self.req(&url).send().await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("GitHub {} on {}: {}", status, url, truncate(&body, 500));
        }
        let v: serde_json::Value = resp.json().await?;
        let truncated = v.get("truncated").and_then(|x| x.as_bool()).unwrap_or(false);
        let arr = v
            .get("tree")
            .and_then(|t| t.as_array())
            .cloned()
            .unwrap_or_default();
        let mut out = String::new();
        out.push_str(&format!(
            "Tree for {}/{} @ {} ({} entries{})\n\n",
            owner,
            repo,
            reference,
            arr.len(),
            if truncated { ", TRUNCATED by GitHub (>100k)" } else { "" }
        ));
        for entry in &arr {
            let path = entry.get("path").and_then(|x| x.as_str()).unwrap_or("?");
            let kind = entry.get("type").and_then(|x| x.as_str()).unwrap_or("?");
            let size = entry.get("size").and_then(|x| x.as_u64());
            let kind_label = match kind {
                "tree" => "DIR ",
                "blob" => "FILE",
                "commit" => "MOD ", // submodule
                _ => "?   ",
            };
            match size {
                Some(n) => out.push_str(&format!("{} {:>10}  {}\n", kind_label, n, path)),
                None => out.push_str(&format!("{}             {}\n", kind_label, path)),
            }
        }
        // Truncate if the response is too large for a chat context.
        Ok(truncate(&out, 200_000))
    }

    async fn fetch_file(
        &self,
        owner: &str,
        repo: &str,
        path: &str,
        reference: &str,
    ) -> anyhow::Result<String> {
        let enc: String = path
            .split('/')
            .map(|seg| urlencoding::encode(seg).into_owned())
            .collect::<Vec<_>>()
            .join("/");
        let url =
            format!("{API_BASE}/repos/{owner}/{repo}/contents/{enc}?ref={reference}");
        let resp = self.req(&url).send().await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("GitHub {} on {}: {}", status, url, truncate(&body, 500));
        }
        let v: serde_json::Value = resp.json().await?;
        let size = v.get("size").and_then(|x| x.as_u64()).unwrap_or(0);
        let encoding = v.get("encoding").and_then(|x| x.as_str()).unwrap_or("");
        let raw_content = v.get("content").and_then(|x| x.as_str()).unwrap_or("");
        if encoding == "base64" {
            use base64::Engine as _;
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(raw_content.replace('\n', ""))
                .unwrap_or_default();
            match String::from_utf8(bytes) {
                Ok(s) => {
                    let mut out = format!(
                        "{}/{} @ {}\nPath: {}\nSize: {} bytes\n\n",
                        owner, repo, reference, path, size
                    );
                    out.push_str(&truncate(&s, 250_000));
                    Ok(out)
                }
                Err(_) => Ok(format!(
                    "{}/{} @ {}\nPath: {}\nSize: {} bytes\n\n(binary file — not UTF-8)",
                    owner, repo, reference, path, size
                )),
            }
        } else {
            Ok(format!(
                "{}/{} @ {}\nPath: {}\nSize: {} bytes\n\n{}",
                owner,
                repo,
                reference,
                path,
                size,
                truncate(raw_content, 250_000)
            ))
        }
    }

    async fn fetch_commits(
        &self,
        owner: &str,
        repo: &str,
        reference: &str,
        limit: u32,
    ) -> anyhow::Result<String> {
        let url = format!(
            "{API_BASE}/repos/{owner}/{repo}/commits?sha={reference}&per_page={limit}"
        );
        let resp = self.req(&url).send().await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("GitHub {} on {}: {}", status, url, truncate(&body, 500));
        }
        let arr: Vec<serde_json::Value> = resp.json().await?;
        let mut out = format!(
            "Recent commits on {}/{} @ {} ({})\n\n",
            owner,
            repo,
            reference,
            arr.len()
        );
        for c in &arr {
            let sha = c.get("sha").and_then(|x| x.as_str()).unwrap_or("");
            let short = sha.chars().take(7).collect::<String>();
            let html_url = c.get("html_url").and_then(|x| x.as_str()).unwrap_or("");
            let commit = c.get("commit");
            let subject = commit
                .and_then(|x| x.get("message"))
                .and_then(|x| x.as_str())
                .and_then(|s| s.lines().next())
                .unwrap_or("");
            let author_name = commit
                .and_then(|x| x.get("author"))
                .and_then(|x| x.get("name"))
                .and_then(|x| x.as_str())
                .unwrap_or("?");
            let date = commit
                .and_then(|x| x.get("author"))
                .and_then(|x| x.get("date"))
                .and_then(|x| x.as_str())
                .unwrap_or("");
            let login = c
                .get("author")
                .and_then(|x| x.get("login"))
                .and_then(|x| x.as_str());
            out.push_str(&format!(
                "- {} {} (@{}, {})\n  {}\n  {}\n",
                short,
                subject,
                login.unwrap_or(author_name),
                date,
                html_url,
                "",
            ));
        }
        Ok(out)
    }

    async fn fetch_releases(
        &self,
        owner: &str,
        repo: &str,
        limit: u32,
    ) -> anyhow::Result<String> {
        let url = format!("{API_BASE}/repos/{owner}/{repo}/releases?per_page={limit}");
        let resp = self.req(&url).send().await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("GitHub {} on {}: {}", status, url, truncate(&body, 500));
        }
        let arr: Vec<serde_json::Value> = resp.json().await?;
        let mut out = format!("Releases in {}/{} ({})\n\n", owner, repo, arr.len());
        for r in &arr {
            let tag = r.get("tag_name").and_then(|x| x.as_str()).unwrap_or("");
            let name = r.get("name").and_then(|x| x.as_str()).unwrap_or("");
            let draft = r.get("draft").and_then(|x| x.as_bool()).unwrap_or(false);
            let prerelease = r.get("prerelease").and_then(|x| x.as_bool()).unwrap_or(false);
            let published = r.get("published_at").and_then(|x| x.as_str()).unwrap_or("");
            let url_ = r.get("html_url").and_then(|x| x.as_str()).unwrap_or("");
            let body = r.get("body").and_then(|x| x.as_str()).unwrap_or("");
            let flags = [
                if draft { "draft" } else { "" },
                if prerelease { "pre-release" } else { "" },
            ]
            .iter()
            .filter(|s| !s.is_empty())
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
            out.push_str(&format!(
                "### {} — {}{}\n{} · {}\n\n{}\n\n---\n\n",
                tag,
                if name.is_empty() { tag } else { name },
                if flags.is_empty() { String::new() } else { format!(" [{}]", flags) },
                published,
                url_,
                truncate(body, 4000)
            ));
        }
        Ok(out)
    }

    async fn fetch_workflow_runs(
        &self,
        owner: &str,
        repo: &str,
        limit: u32,
    ) -> anyhow::Result<String> {
        let url = format!(
            "{API_BASE}/repos/{owner}/{repo}/actions/runs?per_page={limit}"
        );
        let resp = self.req(&url).send().await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("GitHub {} on {}: {}", status, url, truncate(&body, 500));
        }
        let v: serde_json::Value = resp.json().await?;
        let arr = v
            .get("workflow_runs")
            .and_then(|x| x.as_array())
            .cloned()
            .unwrap_or_default();
        let mut out = format!(
            "Recent workflow runs in {}/{} ({})\n\n",
            owner,
            repo,
            arr.len()
        );
        for r in &arr {
            let name = r.get("name").and_then(|x| x.as_str()).unwrap_or("?");
            let title = r.get("display_title").and_then(|x| x.as_str()).unwrap_or("");
            let status_s = r.get("status").and_then(|x| x.as_str()).unwrap_or("?");
            let conclusion = r.get("conclusion").and_then(|x| x.as_str()).unwrap_or("-");
            let branch = r.get("head_branch").and_then(|x| x.as_str()).unwrap_or("");
            let number = r.get("run_number").and_then(|x| x.as_u64()).unwrap_or(0);
            let url_ = r.get("html_url").and_then(|x| x.as_str()).unwrap_or("");
            let login = r
                .get("actor")
                .and_then(|x| x.get("login"))
                .and_then(|x| x.as_str())
                .unwrap_or("?");
            let updated = r.get("updated_at").and_then(|x| x.as_str()).unwrap_or("");
            out.push_str(&format!(
                "- [{}/{}] {} #{} — {} (branch: {}, by @{}, {})\n  {}\n  {}\n",
                status_s, conclusion, name, number, title, branch, login, updated, url_, ""
            ));
        }
        Ok(out)
    }

    async fn post_comment(
        &self,
        owner: &str,
        repo: &str,
        number: u64,
        body: &str,
    ) -> anyhow::Result<String> {
        let url = format!("{API_BASE}/repos/{owner}/{repo}/issues/{number}/comments");
        let payload = serde_json::json!({ "body": body });
        let resp = self
            .http
            .post(&url)
            .bearer_auth(&self.creds.token)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .json(&payload)
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            let body_text = resp.text().await.unwrap_or_default();
            anyhow::bail!("GitHub {} on {}: {}", status, url, truncate(&body_text, 500));
        }
        let v: serde_json::Value = resp.json().await?;
        let html_url = v.get("html_url").and_then(|x| x.as_str()).unwrap_or("");
        Ok(format!("Comment posted: {}", html_url))
    }

    async fn post_review(
        &self,
        owner: &str,
        repo: &str,
        number: u64,
        event: &str,
        body: Option<&str>,
    ) -> anyhow::Result<String> {
        let url = format!("{API_BASE}/repos/{owner}/{repo}/pulls/{number}/reviews");
        let mut payload = serde_json::json!({ "event": event });
        if let Some(b) = body {
            payload["body"] = serde_json::Value::String(b.to_string());
        }
        let resp = self
            .http
            .post(&url)
            .bearer_auth(&self.creds.token)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .json(&payload)
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            let body_text = resp.text().await.unwrap_or_default();
            anyhow::bail!("GitHub {} on {}: {}", status, url, truncate(&body_text, 500));
        }
        let v: serde_json::Value = resp.json().await?;
        let html_url = v.get("html_url").and_then(|x| x.as_str()).unwrap_or("");
        let state = v.get("state").and_then(|x| x.as_str()).unwrap_or("");
        Ok(format!("Review submitted ({}): {}", state, html_url))
    }

    async fn do_merge(
        &self,
        owner: &str,
        repo: &str,
        number: u64,
        method: &str,
        commit_title: Option<&str>,
    ) -> anyhow::Result<String> {
        let url = format!("{API_BASE}/repos/{owner}/{repo}/pulls/{number}/merge");
        let mut payload = serde_json::json!({ "merge_method": method });
        if let Some(t) = commit_title {
            payload["commit_title"] = serde_json::Value::String(t.to_string());
        }
        let resp = self
            .http
            .put(&url)
            .bearer_auth(&self.creds.token)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .json(&payload)
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            let body_text = resp.text().await.unwrap_or_default();
            anyhow::bail!("GitHub {} on {}: {}", status, url, truncate(&body_text, 500));
        }
        let v: serde_json::Value = resp.json().await?;
        let sha = v.get("sha").and_then(|x| x.as_str()).unwrap_or("");
        let merged = v.get("merged").and_then(|x| x.as_bool()).unwrap_or(false);
        Ok(format!("PR #{} merged={} (sha: {})", number, merged, sha))
    }

    async fn do_edit_pr(
        &self,
        owner: &str,
        repo: &str,
        number: u64,
        title: Option<&str>,
        body: Option<&str>,
    ) -> anyhow::Result<String> {
        let url = format!("{API_BASE}/repos/{owner}/{repo}/pulls/{number}");
        let mut payload = serde_json::Map::new();
        if let Some(t) = title {
            payload.insert("title".into(), serde_json::Value::String(t.to_string()));
        }
        if let Some(b) = body {
            payload.insert("body".into(), serde_json::Value::String(b.to_string()));
        }
        let resp = self
            .http
            .patch(&url)
            .bearer_auth(&self.creds.token)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .json(&serde_json::Value::Object(payload))
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            let body_text = resp.text().await.unwrap_or_default();
            anyhow::bail!("GitHub {} on {}: {}", status, url, truncate(&body_text, 500));
        }
        let v: serde_json::Value = resp.json().await?;
        let html_url = v.get("html_url").and_then(|x| x.as_str()).unwrap_or("");
        let mut changed = Vec::new();
        if title.is_some() { changed.push("title"); }
        if body.is_some() { changed.push("body"); }
        Ok(format!("PR #{} updated ({}): {}", number, changed.join(" + "), html_url))
    }

    async fn do_request_reviewers(
        &self,
        owner: &str,
        repo: &str,
        number: u64,
        users: &[String],
        teams: &[String],
    ) -> anyhow::Result<String> {
        let url = format!(
            "{API_BASE}/repos/{owner}/{repo}/pulls/{number}/requested_reviewers"
        );
        let resp = self
            .http
            .post(&url)
            .bearer_auth(&self.creds.token)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .json(&serde_json::json!({
                "reviewers": users,
                "team_reviewers": teams,
            }))
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            let body_text = resp.text().await.unwrap_or_default();
            anyhow::bail!("GitHub {} on {}: {}", status, url, truncate(&body_text, 500));
        }
        Ok(format!(
            "Requested reviewers on PR #{}: users={:?}, teams={:?}",
            number, users, teams
        ))
    }

    async fn do_remove_reviewers(
        &self,
        owner: &str,
        repo: &str,
        number: u64,
        users: &[String],
        teams: &[String],
    ) -> anyhow::Result<String> {
        let url = format!(
            "{API_BASE}/repos/{owner}/{repo}/pulls/{number}/requested_reviewers"
        );
        let resp = self
            .http
            .delete(&url)
            .bearer_auth(&self.creds.token)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .json(&serde_json::json!({
                "reviewers": users,
                "team_reviewers": teams,
            }))
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            let body_text = resp.text().await.unwrap_or_default();
            anyhow::bail!("GitHub {} on {}: {}", status, url, truncate(&body_text, 500));
        }
        Ok(format!(
            "Removed reviewers from PR #{}: users={:?}, teams={:?}",
            number, users, teams
        ))
    }

    async fn do_add_labels(
        &self,
        owner: &str,
        repo: &str,
        number: u64,
        labels: &[String],
    ) -> anyhow::Result<String> {
        let url = format!("{API_BASE}/repos/{owner}/{repo}/issues/{number}/labels");
        let resp = self
            .http
            .post(&url)
            .bearer_auth(&self.creds.token)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .json(&serde_json::json!({ "labels": labels }))
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            let body_text = resp.text().await.unwrap_or_default();
            anyhow::bail!("GitHub {} on {}: {}", status, url, truncate(&body_text, 500));
        }
        Ok(format!("Added labels to #{}: {:?}", number, labels))
    }

    async fn do_remove_labels(
        &self,
        owner: &str,
        repo: &str,
        number: u64,
        labels: &[String],
    ) -> anyhow::Result<String> {
        let mut removed = Vec::new();
        let mut skipped = Vec::new();
        for label in labels {
            let enc = urlencoding::encode(label);
            let url = format!(
                "{API_BASE}/repos/{owner}/{repo}/issues/{number}/labels/{enc}"
            );
            let resp = self
                .http
                .delete(&url)
                .bearer_auth(&self.creds.token)
                .header("Accept", "application/vnd.github+json")
                .header("X-GitHub-Api-Version", "2022-11-28")
                .send()
                .await?;
            let status = resp.status();
            if status == reqwest::StatusCode::NOT_FOUND {
                // Label not present on this issue — fine, just skip.
                skipped.push(label.clone());
                continue;
            }
            if !status.is_success() {
                let body_text = resp.text().await.unwrap_or_default();
                anyhow::bail!("GitHub {} on {}: {}", status, url, truncate(&body_text, 500));
            }
            removed.push(label.clone());
        }
        Ok(format!(
            "Removed labels from #{}: {:?}{}",
            number,
            removed,
            if skipped.is_empty() {
                String::new()
            } else {
                format!(" (already absent: {:?})", skipped)
            }
        ))
    }

    async fn do_add_assignees(
        &self,
        owner: &str,
        repo: &str,
        number: u64,
        logins: &[String],
    ) -> anyhow::Result<String> {
        let url = format!("{API_BASE}/repos/{owner}/{repo}/issues/{number}/assignees");
        let resp = self
            .http
            .post(&url)
            .bearer_auth(&self.creds.token)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .json(&serde_json::json!({ "assignees": logins }))
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            let body_text = resp.text().await.unwrap_or_default();
            anyhow::bail!("GitHub {} on {}: {}", status, url, truncate(&body_text, 500));
        }
        Ok(format!("Added assignees on #{}: {:?}", number, logins))
    }

    async fn do_remove_assignees(
        &self,
        owner: &str,
        repo: &str,
        number: u64,
        logins: &[String],
    ) -> anyhow::Result<String> {
        let url = format!("{API_BASE}/repos/{owner}/{repo}/issues/{number}/assignees");
        let resp = self
            .http
            .delete(&url)
            .bearer_auth(&self.creds.token)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .json(&serde_json::json!({ "assignees": logins }))
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            let body_text = resp.text().await.unwrap_or_default();
            anyhow::bail!("GitHub {} on {}: {}", status, url, truncate(&body_text, 500));
        }
        Ok(format!("Removed assignees from #{}: {:?}", number, logins))
    }

    async fn do_set_pr_draft(
        &self,
        owner: &str,
        repo: &str,
        number: u64,
        draft: bool,
    ) -> anyhow::Result<String> {
        // Step 1: REST → fetch PR's `node_id` (GraphQL global id).
        let pr_url = format!("{API_BASE}/repos/{owner}/{repo}/pulls/{number}");
        let pr_resp = self.req(&pr_url).send().await?;
        let pr_status = pr_resp.status();
        if !pr_status.is_success() {
            let body = pr_resp.text().await.unwrap_or_default();
            anyhow::bail!("GitHub {} on {}: {}", pr_status, pr_url, truncate(&body, 500));
        }
        let pr_v: serde_json::Value = pr_resp.json().await?;
        let node_id = pr_v
            .get("node_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("missing node_id on PR fetch"))?;

        // Step 2: GraphQL mutation, picked by toggle direction.
        let mutation = if draft {
            "mutation($id:ID!) { convertPullRequestToDraft(input:{pullRequestId:$id}) { pullRequest { id isDraft } } }"
        } else {
            "mutation($id:ID!) { markPullRequestReadyForReview(input:{pullRequestId:$id}) { pullRequest { id isDraft } } }"
        };
        let gql_url = format!("{API_BASE}/graphql");
        let resp = self
            .http
            .post(&gql_url)
            .bearer_auth(&self.creds.token)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .json(&serde_json::json!({
                "query": mutation,
                "variables": { "id": node_id },
            }))
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            let body_text = resp.text().await.unwrap_or_default();
            anyhow::bail!("GitHub {} on {}: {}", status, gql_url, truncate(&body_text, 500));
        }
        let v: serde_json::Value = resp.json().await?;
        if let Some(errors) = v.get("errors").and_then(|e| e.as_array()) {
            if !errors.is_empty() {
                let msg = errors
                    .iter()
                    .filter_map(|e| e.get("message").and_then(|m| m.as_str()))
                    .collect::<Vec<_>>()
                    .join("; ");
                anyhow::bail!("GraphQL mutation failed: {msg}");
            }
        }
        Ok(format!(
            "PR #{} set to {}",
            number,
            if draft { "draft" } else { "ready for review" }
        ))
    }

    async fn do_set_pr_state(
        &self,
        owner: &str,
        repo: &str,
        number: u64,
        state: &str,
    ) -> anyhow::Result<String> {
        // PRs and issues share the /issues endpoint for state changes.
        let url = format!("{API_BASE}/repos/{owner}/{repo}/issues/{number}");
        let resp = self
            .http
            .patch(&url)
            .bearer_auth(&self.creds.token)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .json(&serde_json::json!({ "state": state }))
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            let body_text = resp.text().await.unwrap_or_default();
            anyhow::bail!("GitHub {} on {}: {}", status, url, truncate(&body_text, 500));
        }
        Ok(format!("#{} state set to {}", number, state))
    }

    async fn do_list_check_runs(
        &self,
        owner: &str,
        repo: &str,
        ref_name: &str,
    ) -> anyhow::Result<String> {
        let enc = urlencoding::encode(ref_name);
        let url = format!(
            "{API_BASE}/repos/{owner}/{repo}/commits/{enc}/check-runs?per_page=100"
        );
        let resp = self.req(&url).send().await?;
        let status = resp.status();
        if !status.is_success() {
            let body_text = resp.text().await.unwrap_or_default();
            anyhow::bail!("GitHub {} on {}: {}", status, url, truncate(&body_text, 500));
        }
        let v: serde_json::Value = resp.json().await?;
        let runs = v
            .get("check_runs")
            .and_then(|x| x.as_array())
            .cloned()
            .unwrap_or_default();
        if runs.is_empty() {
            return Ok(format!("No check runs found for {}/{}@{}.", owner, repo, ref_name));
        }
        let mut lines = Vec::with_capacity(runs.len() + 1);
        lines.push(format!("Check runs for {}/{}@{} ({} total):", owner, repo, ref_name, runs.len()));
        for r in runs.iter().take(50) {
            let name = r.get("name").and_then(|x| x.as_str()).unwrap_or("?");
            let st = r.get("status").and_then(|x| x.as_str()).unwrap_or("?");
            let conc = r.get("conclusion").and_then(|x| x.as_str()).unwrap_or("");
            let url = r.get("details_url").and_then(|x| x.as_str()).unwrap_or("");
            lines.push(format!(
                "- [{}{}] {} — {}",
                st,
                if conc.is_empty() { String::new() } else { format!("/{}", conc) },
                name,
                url
            ));
        }
        if runs.len() > 50 {
            lines.push(format!("… and {} more (truncated)", runs.len() - 50));
        }
        Ok(lines.join("\n"))
    }

    async fn do_rerun_workflow(
        &self,
        owner: &str,
        repo: &str,
        run_id: u64,
    ) -> anyhow::Result<String> {
        let url = format!("{API_BASE}/repos/{owner}/{repo}/actions/runs/{run_id}/rerun");
        let resp = self
            .http
            .post(&url)
            .bearer_auth(&self.creds.token)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            let body_text = resp.text().await.unwrap_or_default();
            anyhow::bail!("GitHub {} on {}: {}", status, url, truncate(&body_text, 500));
        }
        Ok(format!("Workflow run {} re-queued.", run_id))
    }

    async fn do_cancel_workflow(
        &self,
        owner: &str,
        repo: &str,
        run_id: u64,
    ) -> anyhow::Result<String> {
        let url = format!("{API_BASE}/repos/{owner}/{repo}/actions/runs/{run_id}/cancel");
        let resp = self
            .http
            .post(&url)
            .bearer_auth(&self.creds.token)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            let body_text = resp.text().await.unwrap_or_default();
            anyhow::bail!("GitHub {} on {}: {}", status, url, truncate(&body_text, 500));
        }
        Ok(format!("Workflow run {} cancellation requested.", run_id))
    }

    async fn fetch_readme(&self, owner: &str, repo: &str) -> anyhow::Result<String> {
        let url = format!("{API_BASE}/repos/{owner}/{repo}/readme");
        let resp = self.req(&url).send().await?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(format!("No README found in {}/{}.", owner, repo));
        }
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("GitHub {} on {}: {}", status, url, truncate(&body, 500));
        }
        let v: serde_json::Value = resp.json().await?;
        let encoding = v.get("encoding").and_then(|x| x.as_str()).unwrap_or("");
        let name = v.get("name").and_then(|x| x.as_str()).unwrap_or("README");
        let raw = v.get("content").and_then(|x| x.as_str()).unwrap_or("");
        if encoding == "base64" {
            use base64::Engine as _;
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(raw.replace('\n', ""))
                .unwrap_or_default();
            let content = String::from_utf8(bytes).unwrap_or_else(|_| "(non-UTF-8 README)".into());
            Ok(format!("# {} (README of {}/{})\n\n{}", name, owner, repo, content))
        } else {
            Ok(format!("# {} (README of {}/{})\n\n{}", name, owner, repo, raw))
        }
    }

    async fn fetch_pr_comments(
        &self,
        owner: &str,
        repo: &str,
        number: u64,
    ) -> anyhow::Result<String> {
        let issue_url = format!("{API_BASE}/repos/{owner}/{repo}/issues/{number}/comments?per_page=100");
        let review_url = format!("{API_BASE}/repos/{owner}/{repo}/pulls/{number}/comments?per_page=100");

        let (issue_resp, review_resp) = tokio::join!(
            self.req(&issue_url).send(),
            self.req(&review_url).send(),
        );

        let mut out = String::new();

        out.push_str("## Conversation comments\n\n");
        match issue_resp {
            Ok(r) if r.status().is_success() => {
                let arr: Vec<serde_json::Value> = r.json().await.unwrap_or_default();
                if arr.is_empty() {
                    out.push_str("(none)\n\n");
                } else {
                    for c in &arr {
                        let user = c
                            .get("user")
                            .and_then(|u| u.get("login"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("?");
                        let at = c.get("created_at").and_then(|v| v.as_str()).unwrap_or("");
                        let body = c.get("body").and_then(|v| v.as_str()).unwrap_or("");
                        out.push_str(&format!("- @{} at {}\n{}\n\n", user, at, body));
                    }
                }
            }
            Ok(r) => out.push_str(&format!("(error fetching issue comments: {})\n\n", r.status())),
            Err(e) => out.push_str(&format!("(error fetching issue comments: {})\n\n", e)),
        }

        out.push_str("## Review (inline) comments\n\n");
        match review_resp {
            Ok(r) if r.status().is_success() => {
                let arr: Vec<serde_json::Value> = r.json().await.unwrap_or_default();
                if arr.is_empty() {
                    out.push_str("(none)\n\n");
                } else {
                    for c in &arr {
                        let user = c
                            .get("user")
                            .and_then(|u| u.get("login"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("?");
                        let path = c.get("path").and_then(|v| v.as_str()).unwrap_or("?");
                        let line = c.get("line").and_then(|v| v.as_u64());
                        let body = c.get("body").and_then(|v| v.as_str()).unwrap_or("");
                        match line {
                            Some(n) => out.push_str(&format!("- @{} on {}:{}\n{}\n\n", user, path, n, body)),
                            None => out.push_str(&format!("- @{} on {}\n{}\n\n", user, path, body)),
                        }
                    }
                }
            }
            Ok(r) => out.push_str(&format!("(error fetching review comments: {})\n\n", r.status())),
            Err(e) => out.push_str(&format!("(error fetching review comments: {})\n\n", e)),
        }

        Ok(out)
    }
}

fn format_pr(v: &serde_json::Value) -> String {
    let number = v.get("number").and_then(|n| n.as_u64()).unwrap_or(0);
    let title = v.get("title").and_then(|s| s.as_str()).unwrap_or("");
    let state = v.get("state").and_then(|s| s.as_str()).unwrap_or("?");
    let draft = v.get("draft").and_then(|s| s.as_bool()).unwrap_or(false);
    let merged = v.get("merged").and_then(|s| s.as_bool()).unwrap_or(false);
    let mergeable = v.get("mergeable").and_then(|s| s.as_bool());
    let mergeable_state = v.get("mergeable_state").and_then(|s| s.as_str()).unwrap_or("");
    let author = v
        .get("user")
        .and_then(|u| u.get("login"))
        .and_then(|s| s.as_str())
        .unwrap_or("?");
    let head_ref = v
        .get("head")
        .and_then(|h| h.get("ref"))
        .and_then(|s| s.as_str())
        .unwrap_or("?");
    let head_sha = v
        .get("head")
        .and_then(|h| h.get("sha"))
        .and_then(|s| s.as_str())
        .unwrap_or("?");
    let base_ref = v
        .get("base")
        .and_then(|h| h.get("ref"))
        .and_then(|s| s.as_str())
        .unwrap_or("?");
    let body = v.get("body").and_then(|s| s.as_str()).unwrap_or("");
    let url = v.get("html_url").and_then(|s| s.as_str()).unwrap_or("");
    let additions = v.get("additions").and_then(|s| s.as_u64()).unwrap_or(0);
    let deletions = v.get("deletions").and_then(|s| s.as_u64()).unwrap_or(0);
    let changed_files = v.get("changed_files").and_then(|s| s.as_u64()).unwrap_or(0);

    let mut out = String::new();
    out.push_str(&format!("#{} — {}\n", number, title));
    out.push_str(&format!("URL: {}\n", url));
    out.push_str(&format!(
        "State: {}{}{} · Base: {} ← Head: {} ({})\n",
        state,
        if draft { " (draft)" } else { "" },
        if merged { " (merged)" } else { "" },
        base_ref,
        head_ref,
        &head_sha[..head_sha.len().min(7)],
    ));
    out.push_str(&format!(
        "Mergeable: {} ({}) · {} files, +{} -{}\n",
        mergeable.map(|b| b.to_string()).unwrap_or_else(|| "unknown".into()),
        mergeable_state,
        changed_files,
        additions,
        deletions,
    ));
    out.push_str(&format!("Author: @{}\n", author));
    if !body.is_empty() {
        out.push_str("\n--- Description ---\n");
        out.push_str(body);
        out.push('\n');
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

/// Send a CardRequest to the Tauri shell over the action-IPC Unix
/// socket and BLOCK until the user resolves the card. The MCP tool
/// caller (propose_bash et al.) holds its response open during this
/// await — that's exactly how the agent's CLI ends up waiting for
/// the card outcome IN THE SAME TURN.
///
/// Error path: if WOOM_IPC_SOCKET / WOOM_SESSION_ID env
/// vars are missing or the socket is unreachable, returns Err and
/// the caller falls back to the legacy "card created, end the turn"
/// stub (handing the agent a generic message). That keeps things
/// degrading gracefully if Tauri side isn't yet IPC-aware.
async fn ipc_request_card(
    kind: &str,
    params: serde_json::Value,
) -> anyhow::Result<(bool, String)> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixStream;

    let socket = std::env::var("WOOM_IPC_SOCKET")
        .context("WOOM_IPC_SOCKET not set")?;
    if socket.trim().is_empty() {
        anyhow::bail!("WOOM_IPC_SOCKET is empty");
    }
    let session_id = std::env::var("WOOM_SESSION_ID")
        .context("WOOM_SESSION_ID not set")?;
    let wait_id = uuid::Uuid::new_v4().to_string();

    let req = serde_json::json!({
        "session_id": session_id,
        "wait_id": wait_id,
        "kind": kind,
        "params": params,
    });
    let mut body = serde_json::to_string(&req)?;
    body.push('\n');

    let stream = UnixStream::connect(&socket)
        .await
        .with_context(|| format!("connect to action-IPC socket at {}", socket))?;
    let (read_half, mut write_half) = stream.into_split();
    write_half.write_all(body.as_bytes()).await?;
    write_half.flush().await?;
    // Don't close write half — keep it open, the Tauri side reads
    // a single line and then the connection stays open until it
    // writes the response. (Closing write half would let the OS
    // half-close the connection, which is fine, but explicit shutdown
    // would also send EOF that the read side treats as connection
    // gone; safer to just leave it.)
    let mut reader = BufReader::new(read_half);
    let mut response_line = String::new();
    // No timeout on this read — the agent is meant to wait
    // indefinitely on user approval, mirroring claude-code's bash
    // permission prompt UX. The user's CLI session itself can be
    // cancelled (kill -TERM); that propagates through the parent
    // claude process to us via stdio close.
    let _ = reader.read_line(&mut response_line).await?;
    let resp: serde_json::Value = serde_json::from_str(response_line.trim())
        .with_context(|| format!("parse IPC response: {:?}", response_line))?;
    let ok = resp.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
    let summary = resp
        .get("summary")
        .and_then(|v| v.as_str())
        .unwrap_or("(no summary returned)")
        .to_string();
    Ok((ok, summary))
}

/// Convenience wrapper: turn the IPC outcome (or fallback) into the
/// MCP tool's CallToolResult. We always return success at the MCP
/// level (the agent gets the `summary` text either way) — failures
/// of the underlying action are conveyed via the summary's content,
/// not via MCP error frames, because Anthropic's CLI surfaces MCP
/// errors as opaque "tool failed" red text instead of feeding the
/// detail back to the model. Keeping it in the success-text means
/// the agent sees and reasons about every outcome.
async fn run_or_fallback(
    kind: &str,
    params: serde_json::Value,
    fallback_summary: String,
) -> Result<CallToolResult, ErrorData> {
    match ipc_request_card(kind, params).await {
        Ok((_ok, summary)) => Ok(CallToolResult::success(vec![Content::text(summary)])),
        Err(e) => {
            // Surface the IPC failure to the agent as part of the
            // text so it can decide what to do (usually: tell the
            // user the propose flow degraded, fall back to a normal
            // Bash tool call, etc).
            let msg = format!(
                "{}\n\n(Action IPC unavailable: {}. The card was not registered with Woom's UI.)",
                fallback_summary, e
            );
            Ok(CallToolResult::success(vec![Content::text(msg)]))
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let creds = GhCreds::from_env()?;
    let gh = Gh::new(creds)?;
    let service = gh.serve(stdio()).await.context("start MCP service over stdio")?;
    service.waiting().await?;
    Ok(())
}
