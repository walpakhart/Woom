//! GitHub Tauri commands. Extracted from `lib.rs` in wave-27 split.
//! Each command is a thin wrapper that pulls the stored PAT via the
//! shared `token()` helper, calls into `crate::github::*`, and maps
//! errors to the `String` shape Tauri serializes back to the
//! frontend.
//!
//! `github_connect_pat` / `github_status` / `github_disconnect` are
//! the connection-lifecycle trio — they also fan out to
//! `crate::cursor_mcp::sync()` so `~/.cursor/mcp.json` mirrors the
//! current keychain state.

use crate::github;
use crate::keychain;
use crate::{ConnectionStatus, GITHUB_KEY};
use github::{
    ChangedFile, CheckRun, Comment, CommitDetail, CommitEntry, CompareResult, FileBlob,
    GithubUser, InboxItem, PrDetail, Release, RepoBranch, RepoCommit, RepoReadme, Repository,
    Review, ReviewComment, TreeEntry, WorkflowRun,
};

/// Fetch the keychain-stored PAT or return the standard "not connected"
/// error string. Pulled out as `pub(crate)` so other command modules
/// can hop the same gate.
pub(crate) async fn token() -> Result<String, String> {
    keychain::get(GITHUB_KEY)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "GitHub is not connected".to_string())
}

#[tauri::command]
pub async fn github_connect_pat(token: String) -> Result<GithubUser, String> {
    let trimmed = token.trim().to_string();
    if trimmed.is_empty() {
        return Err("token is empty".into());
    }
    let (user, _rate_limit) = github::fetch_user(&trimmed).await.map_err(|e| e.to_string())?;
    keychain::set(GITHUB_KEY, &trimmed).map_err(|e| e.to_string())?;
    let _ = crate::cursor_mcp::sync();
    Ok(user)
}

#[tauri::command]
pub async fn github_status() -> Result<ConnectionStatus, String> {
    match keychain::get(GITHUB_KEY).map_err(|e| e.to_string())? {
        None => Ok(ConnectionStatus::Disconnected),
        Some(t) => match github::fetch_user(&t).await {
            Ok((user, rate_limit)) => Ok(ConnectionStatus::Connected { user, rate_limit }),
            Err(github::GithubError::InvalidToken) => {
                let _ = keychain::delete(GITHUB_KEY);
                Ok(ConnectionStatus::Disconnected)
            }
            Err(e) => Err(e.to_string()),
        },
    }
}

#[tauri::command]
pub fn github_disconnect() -> Result<(), String> {
    keychain::delete(GITHUB_KEY).map_err(|e| e.to_string())?;
    let _ = crate::cursor_mcp::sync();
    Ok(())
}

#[tauri::command]
pub async fn github_list_inbox() -> Result<Vec<InboxItem>, String> {
    let t = token().await?;
    github::search_involves_me(&t).await.map_err(|e| e.to_string())
}

/// Run a pre-built GitHub search query (`q=` as composed by the frontend).
/// Caller is responsible for combining filters — e.g. `involves:@me is:open`.
#[tauri::command]
pub async fn github_search_inbox(query: String) -> Result<Vec<InboxItem>, String> {
    let t = token().await?;
    github::search_issues_with_query(&t, &query)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn github_list_repos() -> Result<Vec<Repository>, String> {
    let t = token().await?;
    github::list_repos(&t).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn github_list_repo_items(
    owner: String,
    repo: String,
    state: String,
) -> Result<Vec<InboxItem>, String> {
    let t = token().await?;
    github::list_repo_issues(&t, &owner, &repo, &state).await.map_err(|e| e.to_string())
}

/// Single PR or issue lookup by `(owner, repo, number)`. Used by the
/// frontend's app-navigation handler when the agent calls
/// `mcp__app__open_github_pr` so we can slot the item into the focus
/// pane without first scrolling through an inbox.
#[tauri::command]
pub async fn github_get_inbox_item(
    owner: String,
    repo: String,
    number: u64,
) -> Result<InboxItem, String> {
    let t = token().await?;
    github::fetch_inbox_item(&t, &owner, &repo, number)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn github_list_workflow_runs(
    owner: String,
    repo: String,
) -> Result<Vec<WorkflowRun>, String> {
    let t = token().await?;
    github::list_workflow_runs(&t, &owner, &repo).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn github_rerun_workflow(owner: String, repo: String, run_id: u64) -> Result<(), String> {
    let t = token().await?;
    github::rerun_workflow(&t, &owner, &repo, run_id).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn github_cancel_workflow(owner: String, repo: String, run_id: u64) -> Result<(), String> {
    let t = token().await?;
    github::cancel_workflow(&t, &owner, &repo, run_id).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn github_list_tree(
    owner: String,
    repo: String,
    reference: String,
) -> Result<Vec<TreeEntry>, String> {
    let t = token().await?;
    github::list_tree(&t, &owner, &repo, &reference).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn github_get_file_content(
    owner: String,
    repo: String,
    path: String,
    reference: String,
) -> Result<FileBlob, String> {
    let t = token().await?;
    github::get_file_content(&t, &owner, &repo, &path, &reference).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn github_list_releases(owner: String, repo: String) -> Result<Vec<Release>, String> {
    let t = token().await?;
    github::list_releases(&t, &owner, &repo).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn github_list_repo_commits(
    owner: String,
    repo: String,
    reference: String,
    limit: u32,
) -> Result<Vec<RepoCommit>, String> {
    let t = token().await?;
    github::list_commits(&t, &owner, &repo, &reference, limit).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn github_list_repo_branches(owner: String, repo: String) -> Result<Vec<RepoBranch>, String> {
    let t = token().await?;
    github::list_branches_api(&t, &owner, &repo).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn github_get_readme(
    owner: String,
    repo: String,
) -> Result<Option<RepoReadme>, String> {
    let t = token().await?;
    github::get_readme(&t, &owner, &repo).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn github_get_pr(owner: String, repo: String, number: u64) -> Result<PrDetail, String> {
    let t = token().await?;
    github::get_pr_detail(&t, &owner, &repo, number).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn github_list_pr_files(
    owner: String,
    repo: String,
    number: u64,
) -> Result<Vec<ChangedFile>, String> {
    let t = token().await?;
    github::list_pr_files(&t, &owner, &repo, number).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn github_list_pr_commits(
    owner: String,
    repo: String,
    number: u64,
) -> Result<Vec<CommitEntry>, String> {
    let t = token().await?;
    github::list_pr_commits(&t, &owner, &repo, number).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn github_list_check_runs(
    owner: String,
    repo: String,
    reference: String,
) -> Result<Vec<CheckRun>, String> {
    let t = token().await?;
    github::list_check_runs(&t, &owner, &repo, &reference)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn github_list_pr_reviews(
    owner: String,
    repo: String,
    number: u64,
) -> Result<Vec<Review>, String> {
    let t = token().await?;
    github::list_pr_reviews(&t, &owner, &repo, number).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn github_list_review_comments(
    owner: String,
    repo: String,
    number: u64,
) -> Result<Vec<ReviewComment>, String> {
    let t = token().await?;
    github::list_pr_review_comments(&t, &owner, &repo, number).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn github_list_comments(
    owner: String,
    repo: String,
    number: u64,
) -> Result<Vec<Comment>, String> {
    let t = token().await?;
    github::list_issue_comments(&t, &owner, &repo, number).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn github_get_commit(
    owner: String,
    repo: String,
    sha: String,
) -> Result<CommitDetail, String> {
    let t = token().await?;
    github::get_commit(&t, &owner, &repo, &sha).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn github_add_comment(
    owner: String,
    repo: String,
    number: u64,
    body: String,
) -> Result<Comment, String> {
    let t = token().await?;
    github::add_issue_comment(&t, &owner, &repo, number, &body)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn github_submit_review(
    owner: String,
    repo: String,
    number: u64,
    event: String,
    body: String,
) -> Result<Review, String> {
    let t = token().await?;
    github::submit_review(&t, &owner, &repo, number, &event, &body)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn github_set_state(
    owner: String,
    repo: String,
    number: u64,
    state: String,
) -> Result<(), String> {
    let t = token().await?;
    github::set_issue_state(&t, &owner, &repo, number, &state)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn github_merge_pr(
    owner: String,
    repo: String,
    number: u64,
    method: String,
) -> Result<(), String> {
    let t = token().await?;
    github::merge_pr(&t, &owner, &repo, number, &method).await.map_err(|e| e.to_string())
}

/// PATCH a PR's title and/or body. Either or both can be provided;
/// missing keys leave the corresponding field unchanged on GitHub.
/// Empty strings DO clear the body — pass `null`/None to skip
/// updating that field instead.
#[tauri::command]
pub async fn github_edit_pr(
    owner: String,
    repo: String,
    number: u64,
    title: Option<String>,
    body: Option<String>,
) -> Result<(), String> {
    let t = token().await?;
    github::edit_pr(&t, &owner, &repo, number, title.as_deref(), body.as_deref())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn github_request_reviewers(
    owner: String,
    repo: String,
    number: u64,
    #[allow(non_snake_case)] userLogins: Option<Vec<String>>,
    #[allow(non_snake_case)] teamSlugs: Option<Vec<String>>,
) -> Result<(), String> {
    let t = token().await?;
    github::request_pr_reviewers(
        &t,
        &owner,
        &repo,
        number,
        &userLogins.unwrap_or_default(),
        &teamSlugs.unwrap_or_default(),
    )
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn github_remove_reviewers(
    owner: String,
    repo: String,
    number: u64,
    #[allow(non_snake_case)] userLogins: Option<Vec<String>>,
    #[allow(non_snake_case)] teamSlugs: Option<Vec<String>>,
) -> Result<(), String> {
    let t = token().await?;
    github::remove_pr_reviewers(
        &t,
        &owner,
        &repo,
        number,
        &userLogins.unwrap_or_default(),
        &teamSlugs.unwrap_or_default(),
    )
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn github_add_labels(
    owner: String,
    repo: String,
    number: u64,
    labels: Vec<String>,
) -> Result<(), String> {
    let t = token().await?;
    github::add_issue_labels(&t, &owner, &repo, number, &labels)
        .await
        .map_err(|e| e.to_string())
}

/// Remove labels from an issue/PR. Loops over the slice to call
/// `remove_issue_label` once per label so a missing-label 404 on one
/// doesn't abort the rest. Returns the labels that successfully
/// removed (caller can diff against the requested set to learn which
/// were already absent).
#[tauri::command]
pub async fn github_remove_labels(
    owner: String,
    repo: String,
    number: u64,
    labels: Vec<String>,
) -> Result<Vec<String>, String> {
    let t = token().await?;
    let mut removed = Vec::with_capacity(labels.len());
    for l in &labels {
        match github::remove_issue_label(&t, &owner, &repo, number, l).await {
            Ok(()) => removed.push(l.clone()),
            // Surface 404 / not-found as "absent, that's fine"; other
            // errors abort with diagnostic so the caller can act.
            Err(github::GithubError::Api { status: 404, .. }) => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(removed)
}

#[tauri::command]
pub async fn github_add_assignees(
    owner: String,
    repo: String,
    number: u64,
    logins: Vec<String>,
) -> Result<(), String> {
    let t = token().await?;
    github::add_issue_assignees(&t, &owner, &repo, number, &logins)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn github_remove_assignees(
    owner: String,
    repo: String,
    number: u64,
    logins: Vec<String>,
) -> Result<(), String> {
    let t = token().await?;
    github::remove_issue_assignees(&t, &owner, &repo, number, &logins)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn github_set_pr_draft(
    owner: String,
    repo: String,
    number: u64,
    draft: bool,
) -> Result<(), String> {
    let t = token().await?;
    github::set_pr_draft(&t, &owner, &repo, number, draft)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn github_compare(
    owner: String,
    repo: String,
    base: String,
    head: String,
) -> Result<CompareResult, String> {
    let t = token().await?;
    github::compare_branches(&t, &owner, &repo, &base, &head)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn github_create_pr(
    owner: String,
    repo: String,
    title: String,
    body: String,
    base: String,
    head: String,
    draft: bool,
) -> Result<InboxItem, String> {
    let t = token().await?;
    github::create_pr_item(&t, &owner, &repo, &title, &body, &head, &base, draft)
        .await
        .map_err(|e| e.to_string())
}
