//! GitHub API client.

use serde::{Deserialize, Serialize};
use serde_json::json;

const USER_AGENT: &str = concat!("Forgehold-Desktop/", env!("CARGO_PKG_VERSION"));
const API_BASE: &str = "https://api.github.com";

// ---------- Primitive types ----------

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GithubUser {
    pub login: String,
    pub id: u64,
    pub name: Option<String>,
    pub avatar_url: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Actor {
    pub login: String,
    pub avatar_url: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Label {
    pub name: String,
    pub color: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RepoRef {
    pub owner: String,
    pub name: String,
}

impl RepoRef {
    fn from_api_url(url: &str) -> Option<Self> {
        let suffix = url.strip_prefix("https://api.github.com/repos/")?;
        let mut parts = suffix.splitn(2, '/');
        let owner = parts.next()?.to_string();
        let name = parts.next()?.to_string();
        Some(RepoRef { owner, name })
    }
}

// ---------- Inbox / work item ----------

#[derive(Debug, Serialize, Clone)]
pub struct InboxItem {
    pub id: u64,
    pub number: u64,
    pub title: String,
    pub body: Option<String>,
    pub state: String,
    pub is_pull_request: bool,
    pub draft: bool,
    pub merged: bool,
    pub url: String,
    pub author: Option<Actor>,
    pub labels: Vec<Label>,
    pub assignees: Vec<Actor>,
    pub repo: Option<RepoRef>,
    pub comments: u64,
    pub created_at: String,
    pub updated_at: String,
}

// ---------- PR detail ----------

#[derive(Debug, Serialize, Clone)]
pub struct PrDetail {
    pub number: u64,
    pub state: String,
    pub draft: bool,
    pub merged: bool,
    pub mergeable: Option<bool>,
    pub mergeable_state: Option<String>,
    pub head_ref: String,
    pub base_ref: String,
    pub additions: u64,
    pub deletions: u64,
    pub changed_files: u64,
    pub commits: u64,
}

#[derive(Debug, Serialize, Clone)]
pub struct ChangedFile {
    pub filename: String,
    pub status: String,
    pub additions: u64,
    pub deletions: u64,
    pub changes: u64,
    pub patch: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct CommitEntry {
    pub sha: String,
    pub short_sha: String,
    pub message: String,
    pub author_name: String,
    pub author_login: Option<String>,
    pub author_avatar: Option<String>,
    pub author_date: String,
    pub url: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct CommitDetail {
    pub sha: String,
    pub short_sha: String,
    pub message: String,
    pub author_name: String,
    pub author_login: Option<String>,
    pub author_avatar: Option<String>,
    pub author_date: String,
    pub url: String,
    pub additions: u64,
    pub deletions: u64,
    pub total_changes: u64,
    pub files: Vec<ChangedFile>,
}

#[derive(Debug, Serialize, Clone)]
pub struct Review {
    pub id: u64,
    pub user: Option<Actor>,
    pub state: String,
    pub body: String,
    pub submitted_at: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct Comment {
    pub id: u64,
    pub user: Option<Actor>,
    pub body: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct ReviewComment {
    pub id: u64,
    pub user: Option<Actor>,
    pub body: String,
    pub path: String,
    pub line: Option<u64>,
    pub original_line: Option<u64>,
    pub side: Option<String>,
    pub commit_id: String,
    pub in_reply_to_id: Option<u64>,
    pub pull_request_review_id: Option<u64>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct Repository {
    pub id: u64,
    pub name: String,
    pub full_name: String,
    pub owner: String,
    pub description: Option<String>,
    pub private: bool,
    pub fork: bool,
    pub archived: bool,
    pub default_branch: String,
    pub stargazers_count: u64,
    pub open_issues_count: u64,
    pub language: Option<String>,
    pub updated_at: String,
    pub html_url: String,
}

// ---------- Errors ----------

#[derive(Debug, thiserror::Error)]
pub enum GithubError {
    #[error("invalid token")]
    InvalidToken,
    #[error("rate limited — try again later")]
    RateLimited,
    #[error("GitHub returned {status}")]
    Api { status: u16 },
    #[error(transparent)]
    Reqwest(#[from] reqwest::Error),
    #[error("{0}")]
    Message(String),
}

// ---------- Client ----------

fn client() -> reqwest::Result<reqwest::Client> {
    reqwest::Client::builder().user_agent(USER_AGENT).timeout(std::time::Duration::from_secs(30)).build()
}

fn request(
    client: &reqwest::Client,
    method: reqwest::Method,
    token: &str,
    url: String,
) -> reqwest::RequestBuilder {
    client
        .request(method, url)
        .bearer_auth(token)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
}

async fn handle<T: for<'de> serde::Deserialize<'de>>(
    resp: reqwest::Response,
) -> Result<T, GithubError> {
    check_status(&resp)?;
    Ok(resp.json().await?)
}

async fn handle_unit(resp: reqwest::Response) -> Result<(), GithubError> {
    check_status(&resp)?;
    Ok(())
}

fn check_status(resp: &reqwest::Response) -> Result<(), GithubError> {
    let status = resp.status();
    if status.is_success() {
        return Ok(());
    }
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return Err(GithubError::InvalidToken);
    }
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS
        || status == reqwest::StatusCode::FORBIDDEN
    {
        if resp
            .headers()
            .get("x-ratelimit-remaining")
            .and_then(|v| v.to_str().ok())
            == Some("0")
        {
            return Err(GithubError::RateLimited);
        }
    }
    Err(GithubError::Api { status: status.as_u16() })
}

// ---------- /user ----------

pub async fn fetch_user(token: &str) -> Result<GithubUser, GithubError> {
    let c = client()?;
    let resp = request(&c, reqwest::Method::GET, token, format!("{API_BASE}/user"))
        .send()
        .await?;
    handle(resp).await
}

// ---------- Inbox via /search/issues ----------

#[derive(Debug, Deserialize)]
struct SearchResponse {
    items: Vec<SearchItem>,
}

#[derive(Debug, Deserialize)]
struct SearchItem {
    id: u64,
    number: u64,
    title: String,
    body: Option<String>,
    state: String,
    html_url: String,
    user: Option<Actor>,
    labels: Vec<LabelRaw>,
    assignees: Vec<Actor>,
    repository_url: String,
    comments: u64,
    created_at: String,
    updated_at: String,
    draft: Option<bool>,
    pull_request: Option<PullRequestField>,
}

#[derive(Debug, Deserialize)]
struct LabelRaw {
    name: String,
    color: String,
}

#[derive(Debug, Deserialize)]
struct PullRequestField {
    merged_at: Option<String>,
}

impl From<SearchItem> for InboxItem {
    fn from(raw: SearchItem) -> Self {
        let is_pr = raw.pull_request.is_some();
        let merged = raw.pull_request.as_ref().and_then(|p| p.merged_at.as_ref()).is_some();
        InboxItem {
            id: raw.id,
            number: raw.number,
            title: raw.title,
            body: raw.body,
            state: raw.state,
            is_pull_request: is_pr,
            draft: raw.draft.unwrap_or(false),
            merged,
            url: raw.html_url,
            author: raw.user,
            labels: raw.labels.into_iter().map(|l| Label { name: l.name, color: l.color }).collect(),
            assignees: raw.assignees,
            repo: RepoRef::from_api_url(&raw.repository_url),
            comments: raw.comments,
            created_at: raw.created_at,
            updated_at: raw.updated_at,
        }
    }
}

pub async fn search_involves_me(token: &str) -> Result<Vec<InboxItem>, GithubError> {
    search_issues_with_query(token, "involves:@me is:open").await
}

/// Run a raw GitHub search-API query. The caller is responsible for composing
/// `q` — e.g. `involves:@me is:open` or
/// `author:foo repo:octocat/hello-world is:open`. Results come back sorted by
/// `updated desc`, capped at 50 items (one page).
pub async fn search_issues_with_query(
    token: &str,
    q: &str,
) -> Result<Vec<InboxItem>, GithubError> {
    let c = client()?;
    let resp = request(&c, reqwest::Method::GET, token, format!("{API_BASE}/search/issues"))
        .query(&[
            ("q", q),
            ("sort", "updated"),
            ("order", "desc"),
            ("per_page", "50"),
        ])
        .send()
        .await?;
    let parsed: SearchResponse = handle(resp).await?;
    Ok(parsed.items.into_iter().map(InboxItem::from).collect())
}

// ---------- /user/repos ----------

#[derive(Debug, Deserialize)]
struct RawRepo {
    id: u64,
    name: String,
    full_name: String,
    owner: OwnerRef,
    description: Option<String>,
    private: bool,
    fork: bool,
    archived: bool,
    default_branch: String,
    stargazers_count: u64,
    open_issues_count: u64,
    language: Option<String>,
    updated_at: String,
    html_url: String,
}

#[derive(Debug, Deserialize)]
struct OwnerRef {
    login: String,
}

impl From<RawRepo> for Repository {
    fn from(r: RawRepo) -> Self {
        Repository {
            id: r.id,
            name: r.name,
            full_name: r.full_name,
            owner: r.owner.login,
            description: r.description,
            private: r.private,
            fork: r.fork,
            archived: r.archived,
            default_branch: r.default_branch,
            stargazers_count: r.stargazers_count,
            open_issues_count: r.open_issues_count,
            language: r.language,
            updated_at: r.updated_at,
            html_url: r.html_url,
        }
    }
}

pub async fn list_repos(token: &str) -> Result<Vec<Repository>, GithubError> {
    let c = client()?;
    let mut all = Vec::new();
    // Up to 200 repos (4 pages of 50) — plenty for MVP
    for page in 1..=4u32 {
        let resp = request(&c, reqwest::Method::GET, token, format!("{API_BASE}/user/repos"))
            .query(&[
                ("per_page", "50"),
                ("sort", "updated"),
                ("affiliation", "owner,collaborator,organization_member"),
                ("page", &page.to_string()),
            ])
            .send()
            .await?;
        let raw: Vec<RawRepo> = handle(resp).await?;
        if raw.is_empty() {
            break;
        }
        let was_full = raw.len() == 50;
        all.extend(raw.into_iter().map(Repository::from));
        if !was_full {
            break;
        }
    }
    Ok(all)
}

// ---------- Per-repo listings ----------

/// Fetch a single PR/issue by number and adapt it into the same InboxItem
/// shape `list_repo_issues` returns. Used when the frontend needs to slot
/// an item into the focus pane without a prior list view (e.g. when the
/// agent calls `mcp__app__open_github_pr` to navigate the user there).
///
/// Routes through `/repos/{owner}/{repo}/issues/{number}` because that
/// endpoint covers both PRs and issues — issues unify under it server-side.
pub async fn fetch_inbox_item(
    token: &str,
    owner: &str,
    repo: &str,
    number: u64,
) -> Result<InboxItem, GithubError> {
    let c = client()?;
    let url = format!("{API_BASE}/repos/{owner}/{repo}/issues/{number}");
    let resp = request(&c, reqwest::Method::GET, token, url).send().await?;
    let raw: SearchItem = handle(resp).await?;
    Ok(InboxItem::from(raw))
}

pub async fn list_repo_issues(
    token: &str,
    owner: &str,
    repo: &str,
    state: &str,
) -> Result<Vec<InboxItem>, GithubError> {
    let c = client()?;
    // GitHub search works best for mixed PR+issue. Use it with repo filter.
    let q = format!("repo:{owner}/{repo} is:{}", state);
    let resp = request(&c, reqwest::Method::GET, token, format!("{API_BASE}/search/issues"))
        .query(&[
            ("q", q.as_str()),
            ("sort", "updated"),
            ("order", "desc"),
            ("per_page", "50"),
        ])
        .send()
        .await?;
    let parsed: SearchResponse = handle(resp).await?;
    Ok(parsed.items.into_iter().map(InboxItem::from).collect())
}

// ---------- Actions / workflow runs ----------

#[derive(Debug, Serialize, Clone)]
pub struct WorkflowRun {
    pub id: u64,
    pub name: String,
    pub display_title: String,
    pub head_branch: String,
    pub head_sha: String,
    pub event: String,
    pub status: String,
    pub conclusion: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub html_url: String,
    pub actor_login: Option<String>,
    pub actor_avatar: Option<String>,
    pub run_number: u64,
    pub workflow_id: u64,
    pub run_attempt: u32,
}

#[derive(Debug, Deserialize)]
struct RawRunsEnvelope {
    workflow_runs: Vec<RawRun>,
}

#[derive(Debug, Deserialize)]
struct RawRun {
    id: u64,
    #[serde(default)]
    name: Option<String>,
    #[serde(default, rename = "display_title")]
    display_title: String,
    head_branch: String,
    head_sha: String,
    event: String,
    status: String,
    conclusion: Option<String>,
    created_at: String,
    updated_at: String,
    html_url: String,
    actor: Option<RawRunActor>,
    run_number: u64,
    workflow_id: u64,
    #[serde(default = "default_attempt")]
    run_attempt: u32,
}

fn default_attempt() -> u32 { 1 }

#[derive(Debug, Deserialize)]
struct RawRunActor {
    login: String,
    avatar_url: String,
}

pub async fn list_workflow_runs(
    token: &str,
    owner: &str,
    repo: &str,
) -> Result<Vec<WorkflowRun>, GithubError> {
    let c = client()?;
    let url = format!("{API_BASE}/repos/{owner}/{repo}/actions/runs?per_page=30");
    let resp = request(&c, reqwest::Method::GET, token, url).send().await?;
    let parsed: RawRunsEnvelope = handle(resp).await?;
    Ok(parsed
        .workflow_runs
        .into_iter()
        .map(|r| WorkflowRun {
            id: r.id,
            name: r.name.unwrap_or_else(|| "workflow".into()),
            display_title: r.display_title,
            head_branch: r.head_branch,
            head_sha: r.head_sha,
            event: r.event,
            status: r.status,
            conclusion: r.conclusion,
            created_at: r.created_at,
            updated_at: r.updated_at,
            html_url: r.html_url,
            actor_login: r.actor.as_ref().map(|a| a.login.clone()),
            actor_avatar: r.actor.map(|a| a.avatar_url),
            run_number: r.run_number,
            workflow_id: r.workflow_id,
            run_attempt: r.run_attempt,
        })
        .collect())
}

// ---------- Rerun / cancel workflow ----------

pub async fn rerun_workflow(
    token: &str,
    owner: &str,
    repo: &str,
    run_id: u64,
) -> Result<(), GithubError> {
    let c = client()?;
    let url = format!("{API_BASE}/repos/{owner}/{repo}/actions/runs/{run_id}/rerun");
    let resp = request(&c, reqwest::Method::POST, token, url).send().await?;
    handle_unit(resp).await
}

pub async fn cancel_workflow(
    token: &str,
    owner: &str,
    repo: &str,
    run_id: u64,
) -> Result<(), GithubError> {
    let c = client()?;
    let url = format!("{API_BASE}/repos/{owner}/{repo}/actions/runs/{run_id}/cancel");
    let resp = request(&c, reqwest::Method::POST, token, url).send().await?;
    handle_unit(resp).await
}

// ---------- Git tree / file content ----------

#[derive(Debug, Serialize, Clone)]
pub struct TreeEntry {
    pub path: String,
    pub sha: String,
    /// "blob" | "tree" | "commit" (the last one for submodules)
    pub kind: String,
    pub size: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct RawTree {
    tree: Vec<RawTreeEntry>,
    truncated: bool,
}

#[derive(Debug, Deserialize)]
struct RawTreeEntry {
    path: String,
    #[serde(rename = "type")]
    kind: String,
    sha: String,
    #[serde(default)]
    size: Option<u64>,
}

/// List a repository's complete file tree at a given ref (branch or sha).
/// Uses the single recursive call. Note: GitHub caps at ~100k entries and
/// sets `truncated: true` past that; we surface a trailing entry to inform
/// the UI.
pub async fn list_tree(
    token: &str,
    owner: &str,
    repo: &str,
    ref_name: &str,
) -> Result<Vec<TreeEntry>, GithubError> {
    let c = client()?;
    let url = format!("{API_BASE}/repos/{owner}/{repo}/git/trees/{ref_name}?recursive=1");
    let resp = request(&c, reqwest::Method::GET, token, url).send().await?;
    let parsed: RawTree = handle(resp).await?;
    let mut out: Vec<TreeEntry> = parsed
        .tree
        .into_iter()
        .map(|e| TreeEntry { path: e.path, sha: e.sha, kind: e.kind, size: e.size })
        .collect();
    if parsed.truncated {
        out.push(TreeEntry {
            path: "__truncated__".into(),
            sha: String::new(),
            kind: "notice".into(),
            size: None,
        });
    }
    Ok(out)
}

#[derive(Debug, Serialize, Clone)]
pub struct FileBlob {
    pub path: String,
    pub content: String,
    pub size: u64,
    pub sha: String,
    pub encoding: String,
    /// `true` if content was base64-decoded successfully as UTF-8 text.
    /// Binary files set this to false and leave `content` empty.
    pub is_text: bool,
}

#[derive(Debug, Deserialize)]
struct RawContent {
    path: String,
    sha: String,
    size: u64,
    encoding: String,
    content: String,
}

pub async fn get_file_content(
    token: &str,
    owner: &str,
    repo: &str,
    path: &str,
    ref_name: &str,
) -> Result<FileBlob, GithubError> {
    let c = client()?;
    // Note: path needs URL encoding for segments with spaces / special chars.
    let enc_path: String = path
        .split('/')
        .map(|seg| urlencoding::encode(seg).into_owned())
        .collect::<Vec<_>>()
        .join("/");
    let url = format!("{API_BASE}/repos/{owner}/{repo}/contents/{enc_path}?ref={ref_name}");
    let resp = request(&c, reqwest::Method::GET, token, url).send().await?;
    let raw: RawContent = handle(resp).await?;
    let (content, is_text) = if raw.encoding == "base64" {
        use base64::Engine as _;
        match base64::engine::general_purpose::STANDARD
            .decode(raw.content.replace('\n', ""))
        {
            Ok(bytes) => match String::from_utf8(bytes) {
                Ok(s) => (s, true),
                Err(_) => (String::new(), false),
            },
            Err(_) => (String::new(), false),
        }
    } else {
        (raw.content, true)
    };
    Ok(FileBlob {
        path: raw.path,
        content,
        size: raw.size,
        sha: raw.sha,
        encoding: raw.encoding,
        is_text,
    })
}

// ---------- Releases ----------

#[derive(Debug, Serialize, Clone)]
pub struct Release {
    pub id: u64,
    pub tag_name: String,
    pub name: Option<String>,
    pub body: Option<String>,
    pub draft: bool,
    pub prerelease: bool,
    pub created_at: String,
    pub published_at: Option<String>,
    pub html_url: String,
    pub author_login: Option<String>,
    pub author_avatar: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawRelease {
    id: u64,
    tag_name: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    body: Option<String>,
    draft: bool,
    prerelease: bool,
    created_at: String,
    #[serde(default)]
    published_at: Option<String>,
    html_url: String,
    #[serde(default)]
    author: Option<RawReleaseAuthor>,
}

#[derive(Debug, Deserialize)]
struct RawReleaseAuthor {
    login: String,
    avatar_url: String,
}

pub async fn list_releases(
    token: &str,
    owner: &str,
    repo: &str,
) -> Result<Vec<Release>, GithubError> {
    let c = client()?;
    let url = format!("{API_BASE}/repos/{owner}/{repo}/releases?per_page=50");
    let resp = request(&c, reqwest::Method::GET, token, url).send().await?;
    let parsed: Vec<RawRelease> = handle(resp).await?;
    Ok(parsed
        .into_iter()
        .map(|r| Release {
            id: r.id,
            tag_name: r.tag_name,
            name: r.name,
            body: r.body,
            draft: r.draft,
            prerelease: r.prerelease,
            created_at: r.created_at,
            published_at: r.published_at,
            html_url: r.html_url,
            author_login: r.author.as_ref().map(|a| a.login.clone()),
            author_avatar: r.author.map(|a| a.avatar_url),
        })
        .collect())
}

// ---------- Recent commits (default branch) ----------

#[derive(Debug, Serialize, Clone)]
pub struct RepoCommit {
    pub sha: String,
    pub short_sha: String,
    pub message: String,
    pub author_name: String,
    pub author_login: Option<String>,
    pub author_avatar: Option<String>,
    pub date: String,
    pub html_url: String,
}

#[derive(Debug, Deserialize)]
struct RawRepoCommit {
    sha: String,
    html_url: String,
    commit: RawRepoCommitInner,
    #[serde(default)]
    author: Option<RawRepoCommitUser>,
}

#[derive(Debug, Deserialize)]
struct RawRepoCommitInner {
    message: String,
    author: RawRepoCommitAuthor,
}

#[derive(Debug, Deserialize)]
struct RawRepoCommitAuthor {
    name: String,
    date: String,
}

#[derive(Debug, Deserialize)]
struct RawRepoCommitUser {
    login: String,
    avatar_url: String,
}

pub async fn list_commits(
    token: &str,
    owner: &str,
    repo: &str,
    ref_name: &str,
    limit: u32,
) -> Result<Vec<RepoCommit>, GithubError> {
    let c = client()?;
    let per = limit.min(100).max(1);
    let url = format!(
        "{API_BASE}/repos/{owner}/{repo}/commits?sha={ref_name}&per_page={per}"
    );
    let resp = request(&c, reqwest::Method::GET, token, url).send().await?;
    let parsed: Vec<RawRepoCommit> = handle(resp).await?;
    Ok(parsed
        .into_iter()
        .map(|c| {
            let short = c.sha.chars().take(7).collect::<String>();
            let subject = c.commit.message.lines().next().unwrap_or("").to_string();
            RepoCommit {
                sha: c.sha,
                short_sha: short,
                message: subject,
                author_name: c.commit.author.name,
                author_login: c.author.as_ref().map(|a| a.login.clone()),
                author_avatar: c.author.map(|a| a.avatar_url),
                date: c.commit.author.date,
                html_url: c.html_url,
            }
        })
        .collect())
}

// ---------- Branches ----------

#[derive(Debug, Serialize, Clone)]
pub struct RepoBranch {
    pub name: String,
    pub sha: String,
    pub protected: bool,
}

#[derive(Debug, Deserialize)]
struct RawBranch {
    name: String,
    commit: RawBranchCommit,
    #[serde(default)]
    protected: bool,
}

#[derive(Debug, Deserialize)]
struct RawBranchCommit {
    sha: String,
}

pub async fn list_branches_api(
    token: &str,
    owner: &str,
    repo: &str,
) -> Result<Vec<RepoBranch>, GithubError> {
    let c = client()?;
    let url = format!("{API_BASE}/repos/{owner}/{repo}/branches?per_page=100");
    let resp = request(&c, reqwest::Method::GET, token, url).send().await?;
    let parsed: Vec<RawBranch> = handle(resp).await?;
    Ok(parsed
        .into_iter()
        .map(|b| RepoBranch { name: b.name, sha: b.commit.sha, protected: b.protected })
        .collect())
}

// ---------- README ----------

#[derive(Debug, Serialize, Clone)]
pub struct RepoReadme {
    pub name: String,
    pub content: String,
    pub html_url: String,
}

#[derive(Debug, Deserialize)]
struct RawReadme {
    name: String,
    content: String, // base64-encoded
    encoding: String,
    html_url: String,
}

pub async fn get_readme(
    token: &str,
    owner: &str,
    repo: &str,
) -> Result<Option<RepoReadme>, GithubError> {
    let c = client()?;
    let url = format!("{API_BASE}/repos/{owner}/{repo}/readme");
    let resp = request(&c, reqwest::Method::GET, token, url).send().await?;
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    let raw: RawReadme = handle(resp).await?;
    let decoded = if raw.encoding == "base64" {
        use base64::Engine as _;
        match base64::engine::general_purpose::STANDARD
            .decode(raw.content.replace('\n', ""))
        {
            Ok(bytes) => String::from_utf8_lossy(&bytes).to_string(),
            Err(_) => raw.content,
        }
    } else {
        raw.content
    };
    Ok(Some(RepoReadme {
        name: raw.name,
        content: decoded,
        html_url: raw.html_url,
    }))
}

// ---------- PR detail ----------

#[derive(Debug, Deserialize)]
struct RawPrDetail {
    number: u64,
    state: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    merged: bool,
    mergeable: Option<bool>,
    mergeable_state: Option<String>,
    head: BranchRef,
    base: BranchRef,
    #[serde(default)]
    additions: u64,
    #[serde(default)]
    deletions: u64,
    #[serde(default)]
    changed_files: u64,
    #[serde(default)]
    commits: u64,
}

#[derive(Debug, Deserialize)]
struct BranchRef {
    #[serde(rename = "ref")]
    ref_name: String,
}

pub async fn get_pr_detail(
    token: &str,
    owner: &str,
    repo: &str,
    number: u64,
) -> Result<PrDetail, GithubError> {
    let c = client()?;
    let url = format!("{API_BASE}/repos/{owner}/{repo}/pulls/{number}");
    let resp = request(&c, reqwest::Method::GET, token, url).send().await?;
    let raw: RawPrDetail = handle(resp).await?;
    Ok(PrDetail {
        number: raw.number,
        state: raw.state,
        draft: raw.draft,
        merged: raw.merged,
        mergeable: raw.mergeable,
        mergeable_state: raw.mergeable_state,
        head_ref: raw.head.ref_name,
        base_ref: raw.base.ref_name,
        additions: raw.additions,
        deletions: raw.deletions,
        changed_files: raw.changed_files,
        commits: raw.commits,
    })
}

// ---------- PR files ----------

#[derive(Debug, Deserialize)]
struct RawFile {
    filename: String,
    status: String,
    #[serde(default)]
    additions: u64,
    #[serde(default)]
    deletions: u64,
    #[serde(default)]
    changes: u64,
    patch: Option<String>,
}

impl From<RawFile> for ChangedFile {
    fn from(r: RawFile) -> Self {
        ChangedFile {
            filename: r.filename,
            status: r.status,
            additions: r.additions,
            deletions: r.deletions,
            changes: r.changes,
            patch: r.patch,
        }
    }
}

pub async fn list_pr_files(
    token: &str,
    owner: &str,
    repo: &str,
    number: u64,
) -> Result<Vec<ChangedFile>, GithubError> {
    let c = client()?;
    let url = format!("{API_BASE}/repos/{owner}/{repo}/pulls/{number}/files");
    let resp = request(&c, reqwest::Method::GET, token, url)
        .query(&[("per_page", "100")])
        .send()
        .await?;
    let raw: Vec<RawFile> = handle(resp).await?;
    Ok(raw.into_iter().map(ChangedFile::from).collect())
}

// ---------- PR commits ----------

#[derive(Debug, Deserialize)]
struct RawCommit {
    sha: String,
    html_url: String,
    author: Option<Actor>,
    commit: RawCommitInner,
}

#[derive(Debug, Deserialize)]
struct RawCommitInner {
    message: String,
    author: RawGitAuthor,
}

#[derive(Debug, Deserialize)]
struct RawGitAuthor {
    name: String,
    date: String,
}

impl From<RawCommit> for CommitEntry {
    fn from(c: RawCommit) -> Self {
        let short_sha: String = c.sha.chars().take(7).collect();
        CommitEntry {
            sha: c.sha,
            short_sha,
            message: c.commit.message,
            author_name: c.commit.author.name,
            author_login: c.author.as_ref().map(|a| a.login.clone()),
            author_avatar: c.author.as_ref().map(|a| a.avatar_url.clone()),
            author_date: c.commit.author.date,
            url: c.html_url,
        }
    }
}

pub async fn list_pr_commits(
    token: &str,
    owner: &str,
    repo: &str,
    number: u64,
) -> Result<Vec<CommitEntry>, GithubError> {
    let c = client()?;
    let url = format!("{API_BASE}/repos/{owner}/{repo}/pulls/{number}/commits");
    let resp = request(&c, reqwest::Method::GET, token, url)
        .query(&[("per_page", "100")])
        .send()
        .await?;
    let raw: Vec<RawCommit> = handle(resp).await?;
    Ok(raw.into_iter().map(CommitEntry::from).collect())
}

// ---------- Single commit with files ----------

#[derive(Debug, Deserialize)]
struct RawCommitFull {
    sha: String,
    html_url: String,
    author: Option<Actor>,
    commit: RawCommitInner,
    stats: Option<RawStats>,
    files: Option<Vec<RawFile>>,
}

#[derive(Debug, Deserialize)]
struct RawStats {
    #[serde(default)]
    additions: u64,
    #[serde(default)]
    deletions: u64,
    #[serde(default)]
    total: u64,
}

pub async fn get_commit(
    token: &str,
    owner: &str,
    repo: &str,
    sha: &str,
) -> Result<CommitDetail, GithubError> {
    let c = client()?;
    let url = format!("{API_BASE}/repos/{owner}/{repo}/commits/{sha}");
    let resp = request(&c, reqwest::Method::GET, token, url).send().await?;
    let raw: RawCommitFull = handle(resp).await?;
    let short_sha: String = raw.sha.chars().take(7).collect();
    Ok(CommitDetail {
        sha: raw.sha,
        short_sha,
        message: raw.commit.message,
        author_name: raw.commit.author.name,
        author_login: raw.author.as_ref().map(|a| a.login.clone()),
        author_avatar: raw.author.as_ref().map(|a| a.avatar_url.clone()),
        author_date: raw.commit.author.date,
        url: raw.html_url,
        additions: raw.stats.as_ref().map(|s| s.additions).unwrap_or(0),
        deletions: raw.stats.as_ref().map(|s| s.deletions).unwrap_or(0),
        total_changes: raw.stats.as_ref().map(|s| s.total).unwrap_or(0),
        files: raw.files.unwrap_or_default().into_iter().map(ChangedFile::from).collect(),
    })
}

// ---------- PR reviews ----------

#[derive(Debug, Deserialize)]
struct RawReview {
    id: u64,
    user: Option<Actor>,
    state: String,
    #[serde(default)]
    body: String,
    submitted_at: Option<String>,
}

impl From<RawReview> for Review {
    fn from(r: RawReview) -> Self {
        Review {
            id: r.id,
            user: r.user,
            state: r.state,
            body: r.body,
            submitted_at: r.submitted_at,
        }
    }
}

pub async fn list_pr_reviews(
    token: &str,
    owner: &str,
    repo: &str,
    number: u64,
) -> Result<Vec<Review>, GithubError> {
    let c = client()?;
    let url = format!("{API_BASE}/repos/{owner}/{repo}/pulls/{number}/reviews");
    let resp = request(&c, reqwest::Method::GET, token, url)
        .query(&[("per_page", "100")])
        .send()
        .await?;
    let raw: Vec<RawReview> = handle(resp).await?;
    Ok(raw.into_iter().map(Review::from).collect())
}

// ---------- PR review comments (inline) ----------

#[derive(Debug, Deserialize)]
struct RawReviewComment {
    #[serde(default)]
    id: u64,
    #[serde(default)]
    user: Option<Actor>,
    #[serde(default)]
    body: String,
    #[serde(default)]
    path: String,
    #[serde(default)]
    line: Option<u64>,
    #[serde(default)]
    original_line: Option<u64>,
    #[serde(default)]
    side: Option<String>,
    #[serde(default)]
    commit_id: String,
    #[serde(default)]
    in_reply_to_id: Option<u64>,
    #[serde(default)]
    pull_request_review_id: Option<u64>,
    #[serde(default)]
    created_at: String,
    #[serde(default)]
    updated_at: String,
}

impl From<RawReviewComment> for ReviewComment {
    fn from(r: RawReviewComment) -> Self {
        ReviewComment {
            id: r.id,
            user: r.user,
            body: r.body,
            path: r.path,
            line: r.line,
            original_line: r.original_line,
            side: r.side,
            commit_id: r.commit_id,
            in_reply_to_id: r.in_reply_to_id,
            pull_request_review_id: r.pull_request_review_id,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

pub async fn list_pr_review_comments(
    token: &str,
    owner: &str,
    repo: &str,
    number: u64,
) -> Result<Vec<ReviewComment>, GithubError> {
    let c = client()?;
    let url = format!("{API_BASE}/repos/{owner}/{repo}/pulls/{number}/comments");
    let resp = request(&c, reqwest::Method::GET, token, url)
        .query(&[("per_page", "100")])
        .send()
        .await?;
    let raw: Vec<RawReviewComment> = handle(resp).await?;
    Ok(raw.into_iter().map(ReviewComment::from).collect())
}

// ---------- Issue comments (works for PRs too) ----------

#[derive(Debug, Deserialize)]
struct RawComment {
    id: u64,
    user: Option<Actor>,
    body: String,
    created_at: String,
    updated_at: String,
}

impl From<RawComment> for Comment {
    fn from(c: RawComment) -> Self {
        Comment {
            id: c.id,
            user: c.user,
            body: c.body,
            created_at: c.created_at,
            updated_at: c.updated_at,
        }
    }
}

pub async fn list_issue_comments(
    token: &str,
    owner: &str,
    repo: &str,
    number: u64,
) -> Result<Vec<Comment>, GithubError> {
    let c = client()?;
    let url = format!("{API_BASE}/repos/{owner}/{repo}/issues/{number}/comments");
    let resp = request(&c, reqwest::Method::GET, token, url)
        .query(&[("per_page", "100")])
        .send()
        .await?;
    let raw: Vec<RawComment> = handle(resp).await?;
    Ok(raw.into_iter().map(Comment::from).collect())
}

pub async fn add_issue_comment(
    token: &str,
    owner: &str,
    repo: &str,
    number: u64,
    body: &str,
) -> Result<Comment, GithubError> {
    let c = client()?;
    let url = format!("{API_BASE}/repos/{owner}/{repo}/issues/{number}/comments");
    let resp = request(&c, reqwest::Method::POST, token, url)
        .json(&json!({ "body": body }))
        .send()
        .await?;
    let raw: RawComment = handle(resp).await?;
    Ok(Comment::from(raw))
}

pub async fn submit_review(
    token: &str,
    owner: &str,
    repo: &str,
    number: u64,
    event: &str,
    body: &str,
) -> Result<Review, GithubError> {
    let c = client()?;
    let url = format!("{API_BASE}/repos/{owner}/{repo}/pulls/{number}/reviews");
    let payload = if body.is_empty() {
        json!({ "event": event })
    } else {
        json!({ "event": event, "body": body })
    };
    let resp = request(&c, reqwest::Method::POST, token, url)
        .json(&payload)
        .send()
        .await?;
    let raw: RawReview = handle(resp).await?;
    Ok(Review::from(raw))
}

pub async fn set_issue_state(
    token: &str,
    owner: &str,
    repo: &str,
    number: u64,
    state: &str,
) -> Result<(), GithubError> {
    let c = client()?;
    let url = format!("{API_BASE}/repos/{owner}/{repo}/issues/{number}");
    let resp = request(&c, reqwest::Method::PATCH, token, url)
        .json(&json!({ "state": state }))
        .send()
        .await?;
    handle_unit(resp).await
}

pub async fn merge_pr(
    token: &str,
    owner: &str,
    repo: &str,
    number: u64,
    method: &str,
) -> Result<(), GithubError> {
    let c = client()?;
    let url = format!("{API_BASE}/repos/{owner}/{repo}/pulls/{number}/merge");
    let resp = request(&c, reqwest::Method::PUT, token, url)
        .json(&json!({ "merge_method": method }))
        .send()
        .await?;
    handle_unit(resp).await
}

#[derive(Debug, Serialize, Clone)]
pub struct CheckRun {
    /// GitHub's internal id — stable across refetches, useful for keying.
    pub id: u64,
    /// User-visible label, e.g. "lint" or "unit-tests (18.x)".
    pub name: String,
    /// "queued" | "in_progress" | "completed"
    pub status: String,
    /// Only set once `status == "completed"`: "success" | "failure" |
    /// "neutral" | "cancelled" | "skipped" | "timed_out" | "action_required".
    pub conclusion: Option<String>,
    /// UI link back to GitHub's check detail page (logs, annotations).
    pub details_url: Option<String>,
    /// App that produced the check (e.g. "GitHub Actions").
    pub app_name: Option<String>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawCheckRunsResponse {
    check_runs: Vec<RawCheckRun>,
}

#[derive(Debug, Deserialize)]
struct RawCheckRun {
    id: u64,
    name: String,
    status: String,
    conclusion: Option<String>,
    details_url: Option<String>,
    started_at: Option<String>,
    completed_at: Option<String>,
    app: Option<RawCheckRunApp>,
}

#[derive(Debug, Deserialize)]
struct RawCheckRunApp {
    name: Option<String>,
}

/// Check runs attached to a commit — the modern CI/CD status surface on
/// GitHub (Actions, third-party apps, branch protection). `ref_or_sha` is
/// typically a PR head SHA, but any ref the user can read works.
pub async fn list_check_runs(
    token: &str,
    owner: &str,
    repo: &str,
    ref_or_sha: &str,
) -> Result<Vec<CheckRun>, GithubError> {
    let c = client()?;
    let url = format!(
        "{API_BASE}/repos/{owner}/{repo}/commits/{ref_or_sha}/check-runs?per_page=100"
    );
    let resp = request(&c, reqwest::Method::GET, token, url).send().await?;
    let parsed: RawCheckRunsResponse = handle(resp).await?;
    Ok(parsed
        .check_runs
        .into_iter()
        .map(|c| CheckRun {
            id: c.id,
            name: c.name,
            status: c.status,
            conclusion: c.conclusion,
            details_url: c.details_url,
            app_name: c.app.and_then(|a| a.name),
            started_at: c.started_at,
            completed_at: c.completed_at,
        })
        .collect())
}

/// GET /repos/{owner}/{repo} and return the `default_branch` field. Used by
/// PR creation when the caller leaves the base branch blank.
pub async fn fetch_default_branch(
    token: &str,
    owner: &str,
    repo: &str,
) -> Result<String, GithubError> {
    let c = client()?;
    let url = format!("{API_BASE}/repos/{owner}/{repo}");
    let resp = request(&c, reqwest::Method::GET, token, url).send().await?;
    check_status(&resp)?;
    let v: serde_json::Value = resp.json().await?;
    Ok(v.get("default_branch")
        .and_then(|d| d.as_str())
        .unwrap_or("main")
        .to_string())
}

/// Create a PR via the REST API (`POST /repos/{owner}/{repo}/pulls`). Returns
/// the new PR's `html_url`. Replaces the legacy `gh pr create` shell-out so
/// Forgehold only needs the Keychain token — no `gh` CLI install required.
pub async fn create_pr(
    token: &str,
    owner: &str,
    repo: &str,
    title: &str,
    body: &str,
    head: &str,
    base: &str,
    draft: bool,
) -> Result<String, GithubError> {
    let c = client()?;
    let url = format!("{API_BASE}/repos/{owner}/{repo}/pulls");
    let resp = request(&c, reqwest::Method::POST, token, url)
        .json(&json!({
            "title": title,
            "body": body,
            "head": head,
            "base": base,
            "draft": draft,
        }))
        .send()
        .await?;
    let status = resp.status();
    let body_text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        // GitHub returns a structured error body with `.message` (e.g. "A pull
        // request already exists for…") — surface that to the user rather
        // than the bare HTTP status, which is useless for the common 422 case.
        let detail = serde_json::from_str::<serde_json::Value>(&body_text)
            .ok()
            .and_then(|v| v.get("message").and_then(|m| m.as_str()).map(|s| s.to_string()))
            .unwrap_or_else(|| body_text.chars().take(200).collect());
        return Err(GithubError::Message(format!(
            "GitHub {}: {}",
            status.as_u16(),
            detail
        )));
    }
    let v: serde_json::Value = serde_json::from_str(&body_text)
        .map_err(|e| GithubError::Message(format!("parse PR response: {e}")))?;
    v.get("html_url")
        .and_then(|h| h.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| GithubError::Message("PR created but GitHub response lacked html_url".into()))
}

// ---------- Branch compare (for PR creation preview) ----------

#[derive(Debug, Serialize, Clone)]
pub struct CompareResult {
    pub total_commits: u64,
    pub ahead_by: u64,
    pub behind_by: u64,
    pub additions: u64,
    pub deletions: u64,
    pub commits: Vec<CommitEntry>,
    pub files: Vec<ChangedFile>,
}

#[derive(Debug, Deserialize)]
struct RawCompareResp {
    #[serde(default)]
    total_commits: u64,
    #[serde(default)]
    ahead_by: u64,
    #[serde(default)]
    behind_by: u64,
    #[serde(default)]
    commits: Vec<RawCommit>,
    #[serde(default)]
    files: Vec<RawFile>,
}

/// Compare two branches via `GET /repos/{owner}/{repo}/compare/{base}...{head}`.
/// Returns commits + changed files + line totals, ready to show as a PR diff
/// preview.
pub async fn compare_branches(
    token: &str,
    owner: &str,
    repo: &str,
    base: &str,
    head: &str,
) -> Result<CompareResult, GithubError> {
    let c = client()?;
    // Branch names can contain slashes — percent-encode them for safety.
    let base_enc = urlencoding::encode(base);
    let head_enc = urlencoding::encode(head);
    let url = format!(
        "{API_BASE}/repos/{owner}/{repo}/compare/{base_enc}...{head_enc}?per_page=100"
    );
    let resp = request(&c, reqwest::Method::GET, token, url).send().await?;
    let raw: RawCompareResp = handle(resp).await?;
    let additions = raw.files.iter().map(|f| f.additions).sum();
    let deletions = raw.files.iter().map(|f| f.deletions).sum();
    Ok(CompareResult {
        total_commits: raw.total_commits,
        ahead_by: raw.ahead_by,
        behind_by: raw.behind_by,
        additions,
        deletions,
        commits: raw.commits.into_iter().map(CommitEntry::from).collect(),
        files: raw.files.into_iter().map(ChangedFile::from).collect(),
    })
}

// ---------- Create PR (returns an InboxItem) ----------

#[derive(Debug, Deserialize)]
struct RawCreatedPr {
    id: u64,
    number: u64,
    title: String,
    #[serde(default)]
    body: Option<String>,
    state: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    merged: bool,
    html_url: String,
    #[serde(default)]
    user: Option<Actor>,
    #[serde(default)]
    assignees: Vec<Actor>,
    #[serde(default)]
    labels: Vec<LabelRaw>,
    created_at: String,
    updated_at: String,
    #[serde(default)]
    comments: u64,
    #[serde(default)]
    merged_at: Option<String>,
}

/// Create a PR and return it shaped as an `InboxItem` so the caller can drop
/// it straight into the inbox list and onto the focus pane. Mirrors
/// `create_pr` but preserves the full response payload.
pub async fn create_pr_item(
    token: &str,
    owner: &str,
    repo: &str,
    title: &str,
    body: &str,
    head: &str,
    base: &str,
    draft: bool,
) -> Result<InboxItem, GithubError> {
    let c = client()?;
    let url = format!("{API_BASE}/repos/{owner}/{repo}/pulls");
    let resp = request(&c, reqwest::Method::POST, token, url)
        .json(&json!({
            "title": title,
            "body": body,
            "head": head,
            "base": base,
            "draft": draft,
        }))
        .send()
        .await?;
    let status = resp.status();
    let body_text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        let detail = serde_json::from_str::<serde_json::Value>(&body_text)
            .ok()
            .and_then(|v| v.get("message").and_then(|m| m.as_str()).map(|s| s.to_string()))
            .unwrap_or_else(|| body_text.chars().take(200).collect());
        return Err(GithubError::Message(format!(
            "GitHub {}: {}",
            status.as_u16(),
            detail
        )));
    }
    let raw: RawCreatedPr = serde_json::from_str(&body_text)
        .map_err(|e| GithubError::Message(format!("parse PR response: {e}")))?;
    let merged_at_present = raw.merged_at.is_some();
    Ok(InboxItem {
        id: raw.id,
        number: raw.number,
        title: raw.title,
        body: raw.body,
        state: raw.state,
        is_pull_request: true,
        draft: raw.draft,
        merged: raw.merged || merged_at_present,
        url: raw.html_url,
        author: raw.user,
        labels: raw
            .labels
            .into_iter()
            .map(|l| Label { name: l.name, color: l.color })
            .collect(),
        assignees: raw.assignees,
        repo: Some(RepoRef {
            owner: owner.to_string(),
            name: repo.to_string(),
        }),
        comments: raw.comments,
        created_at: raw.created_at,
        updated_at: raw.updated_at,
    })
}
