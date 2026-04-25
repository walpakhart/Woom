//! forgehold-github — MCP sidecar exposing GitHub as tools to Claude Code.
//!
//! Auth: personal access token via env var GITHUB_TOKEN.
//!
//! Read-only tool surface (phase 2). Write tools (comment/review) come later
//! once the read flow is validated.

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

const USER_AGENT: &str = concat!("forgehold-github/", env!("CARGO_PKG_VERSION"));
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
        description = "Fetch GitHub PR detail (title, author, state, mergeable, branches, body)."
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

    #[tool(description = "Fetch the full unified diff for a PR (raw patch text).")]
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
        description = "List files changed in a PR with status (added/modified/removed), additions, deletions, and a patch per file."
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
        description = "Fetch the discussion on a PR: both issue-level comments (general conversation) and review comments (inline on specific lines)."
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
        description = "Propose a commit for the user to review. Use this after you've finished making code changes and want to suggest a commit. Does NOT perform the commit — it surfaces an editable commit card in the Forgehold UI so the user can review, tweak the message, and approve with one click. Only call this when the user asked you to commit."
    )]
    async fn propose_commit(
        &self,
        Parameters(ProposeCommitParams { message, body, push, note }): Parameters<ProposeCommitParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let push_s = push.unwrap_or(true);
        let summary = format!(
            "Commit proposal queued for user approval.\n\n\
             subject: {}\n\
             push:    {}\n\
             body:    {}\n\
             note:    {}\n\n\
             The user will see an editable card with [Commit & Push] / [Dismiss] in the Forgehold chat. \
             Do not call git-write tools yourself — wait for the user.",
            message,
            push_s,
            body.as_deref().unwrap_or("(none)"),
            note.as_deref().unwrap_or("(none)"),
        );
        Ok(CallToolResult::success(vec![Content::text(summary)]))
    }

    #[tool(
        description = "Propose a shell command for the user to approve. Use this for ANY command that changes state: `git checkout/switch/merge/push/pull/reset/rebase`, `rm`, `mv`, `cp`, `npm install`, `yarn`, database migrations, deployment scripts, etc. Does NOT run the command — the user sees an approval card with [Run] / [Dismiss] buttons and can edit the command first. Read-only commands like `git status`, `ls`, `cat`, `grep` should use the regular Bash tool; this one is only for mutations."
    )]
    async fn propose_bash(
        &self,
        Parameters(ProposeBashParams { command, reason }): Parameters<ProposeBashParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let summary = format!(
            "Bash command queued for user approval.\n\n\
             command: {}\n\
             reason:  {}\n\n\
             The user will see an editable [Run] / [Dismiss] card in the Forgehold chat. \
             When the user approves and the command finishes, they'll paste the output \
             back to you so you can continue. Do not call other state-changing tools \
             until they respond.",
            command,
            reason.as_deref().unwrap_or("(none)"),
        );
        Ok(CallToolResult::success(vec![Content::text(summary)]))
    }

    #[tool(
        description = "Propose switching the current Claude session's working directory to a different local path. Use when the user asks you to work on a different repo or folder. Does NOT switch — the user sees an approval card. Only call when the user asked you to switch."
    )]
    async fn propose_switch_cwd(
        &self,
        Parameters(ProposeSwitchCwdParams { path, reason }): Parameters<ProposeSwitchCwdParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let summary = format!(
            "Working-directory switch proposed for user approval.\n\n\
             path:   {}\n\
             reason: {}\n\n\
             The user will see an approval card in the Forgehold chat. Do not call \
             other tools until they accept or dismiss.",
            path,
            reason.as_deref().unwrap_or("(none)"),
        );
        Ok(CallToolResult::success(vec![Content::text(summary)]))
    }

    #[tool(
        description = "Search GitHub pull requests across one or more repos / orgs. Returns id, number, repo, title, state (open/closed/merged), author, draft flag, created/updated, url. Use this when the user wants to find PRs by keyword, ticket id, author, label, or status — e.g. \"find merged PRs that mention DEVOPS-414\" → `is:pr is:merged DEVOPS-414`. Pass full GitHub search syntax."
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
        description = "Search GitHub issues across one or more repos / orgs. Same syntax as search_prs but defaults to `is:issue`. Use to find tickets by label, milestone, mention, or full-text — e.g. \"open issues with label:bug touched in the last week\" → `is:issue is:open label:bug updated:>2025-04-18`."
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
        description = "Propose a pull request for the user to review. Use after the commit is made (or will be made) and the user asked to open a PR. Does NOT create the PR — it surfaces an editable PR card in the Forgehold UI. Only call when the user asked you to open a PR."
    )]
    async fn propose_pr(
        &self,
        Parameters(ProposePrParams { title, body, base, draft, note }): Parameters<ProposePrParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let summary = format!(
            "PR proposal queued for user approval.\n\n\
             title: {}\n\
             base:  {}\n\
             draft: {}\n\
             body:  {}\n\
             note:  {}\n\n\
             The user will see an editable card with [Create PR] / [Dismiss] in the Forgehold chat. \
             Wait for the user before doing anything else.",
            title,
            base.as_deref().unwrap_or("(repo default)"),
            draft.unwrap_or(false),
            body.as_deref().unwrap_or("(none)"),
            note.as_deref().unwrap_or("(none)"),
        );
        Ok(CallToolResult::success(vec![Content::text(summary)]))
    }
}

#[tool_handler]
impl ServerHandler for Gh {
    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::default();
        info.capabilities = ServerCapabilities::builder().enable_tools().build();
        info.instructions = Some(
            "Access GitHub and propose local actions on behalf of the user.\n\n\
             READ: get_pr, get_pr_diff, get_pr_files, get_pr_comments, list_tree, get_file, list_commits, list_releases, list_workflow_runs, get_readme.\n\n\
             WRITE (needs user confirmation in their chat client): add_comment, submit_review, merge_pr.\n\n\
             PROPOSE (queues an approval card in Forgehold UI, does nothing itself — use when the user asked you to commit/open-pr/switch-repo/run-a-command): propose_commit, propose_pr, propose_switch_cwd, propose_bash.\n\n\
             RULE: for any command that modifies state (git checkout/switch/merge/push/pull/reset/rebase, rm, mv, npm install, migrations, deploys, etc.) call propose_bash instead of the regular Bash tool. Read-only commands (git status, ls, cat, grep, find, rg) can use Bash directly. After proposing, STOP and wait for the user's approval before doing anything else."
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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let creds = GhCreds::from_env()?;
    let gh = Gh::new(creds)?;
    let service = gh.serve(stdio()).await.context("start MCP service over stdio")?;
    service.waiting().await?;
    Ok(())
}
