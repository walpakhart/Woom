mod agent;
mod biometry;
mod claude;
mod cursor;
mod cursor_mcp;
mod fs;
mod git;
mod github;
mod jira;
mod keychain;
mod sentry;
mod watch;
mod worktree;

use agent::{AgentAskResult, AgentKind, AgentStatus};
use claude::{ClaudeStatus, Runners};
use fs::{BashResult, DirEntry};
use git::{Branch, CommitEntry as GitCommitEntry, GitStatus, RepoInfo};
use tauri::State;
use github::{
    ChangedFile, CheckRun, Comment, CommitDetail, CommitEntry, CompareResult, FileBlob,
    GithubUser, InboxItem, PrDetail, Release, RepoBranch, RepoCommit, RepoReadme, Repository,
    Review, ReviewComment, TreeEntry, WorkflowRun,
};
use jira::{
    JiraBoard, JiraComment, JiraCredentials, JiraDetail, JiraIssueType, JiraItem, JiraProject,
    JiraSprint, JiraStatus as JiraWorkflowStatus, JiraUser, JiraUserSummary, JiraWorklog,
};
use sentry::{
    SentryCredentials, SentryEnvironment, SentryEvent, SentryEventDetail, SentryIssue,
    SentryProject, SentryUser,
};
use serde::Serialize;
use watch::WatcherState;
use worktree::{Worktree, WorktreeChangedFile};

const GITHUB_KEY: &str = "github";
const JIRA_KEY: &str = "jira";
const SENTRY_KEY: &str = "sentry";

#[derive(Debug, Serialize, Clone)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ConnectionStatus {
    Disconnected,
    Connected { user: GithubUser },
}

#[derive(Debug, Serialize, Clone)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum JiraStatus {
    Disconnected,
    Connected { user: JiraUser },
}

#[derive(Debug, Serialize, Clone)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SentryConnectionStatus {
    Disconnected,
    Connected { user: SentryUser },
}

/// When Forgehold is launched from Finder/Dock (not a terminal) the process
/// inherits macOS's minimal PATH (`/usr/bin:/bin:/usr/sbin:/sbin`) — the
/// user's `~/.zshrc`/`.bash_profile` never runs. Tools like `claude`,
/// `cursor-agent`, and anything installed via Homebrew/nvm/bun/volta live
/// outside that PATH, so the app can't find them.
///
/// Fix: spawn the user's login shell once at startup, ask it to print its
/// fully-resolved PATH, and overwrite our own. Every `std::env::var("PATH")`
/// read (including the `which()` lookups in `claude.rs` / `cursor.rs`) then
/// sees the right values, as do all spawned subprocesses.
fn hydrate_path_from_login_shell() {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
    let out = std::process::Command::new(&shell)
        .arg("-l")
        .arg("-c")
        .arg("printf %s \"$PATH\"")
        .output();
    if let Ok(out) = out {
        if out.status.success() {
            let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !path.is_empty() {
                // SAFETY: Rust 2024 marks `set_var` unsafe because mutating
                // process env is unsound when other threads may read it
                // concurrently. We're called once, synchronously, at the
                // top of `run()` — before Tauri spawns its event loop or
                // any tokio worker. No reader thread exists yet.
                unsafe { std::env::set_var("PATH", path) };
            }
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    hydrate_path_from_login_shell();
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .manage(claude::new_runners())
        .manage(watch::new_state())
        .invoke_handler(tauri::generate_handler![
            github_connect_pat,
            github_status,
            github_disconnect,
            github_list_inbox,
            github_search_inbox,
            github_list_repos,
            github_list_repo_items,
            github_get_inbox_item,
            github_list_workflow_runs,
            github_rerun_workflow,
            github_cancel_workflow,
            github_list_tree,
            github_get_file_content,
            github_list_releases,
            github_list_repo_commits,
            github_list_repo_branches,
            github_get_readme,
            github_get_pr,
            github_list_pr_files,
            github_list_pr_commits,
            github_list_check_runs,
            github_list_pr_reviews,
            github_list_review_comments,
            github_list_comments,
            github_get_commit,
            github_add_comment,
            github_submit_review,
            github_set_state,
            github_merge_pr,
            github_compare,
            github_create_pr,
            jira_connect,
            jira_status,
            jira_disconnect,
            sentry_connect,
            sentry_status,
            sentry_disconnect,
            sentry_list_issues,
            sentry_get_issue,
            sentry_list_events,
            sentry_list_projects,
            sentry_list_environments,
            sentry_get_event_detail,
            sentry_set_status,
            cursor_mcp_sync,
            jira_list_inbox,
            jira_list_inbox_for,
            jira_search,
            jira_list_projects,
            jira_list_boards,
            jira_list_sprints,
            jira_list_statuses,
            jira_list_issue_types,
            jira_create_issue,
            jira_search_users,
            jira_list_assignable_users,
            jira_get_issue_detail,
            jira_update_issue,
            jira_transition_issue,
            jira_set_assignee,
            jira_set_priority,
            jira_set_labels,
            jira_add_comment,
            jira_list_worklogs,
            jira_add_worklog,
            jira_delete_worklog,
            claude_status,
            claude_ask,
            claude_stop,
            agent_generate_commit_message,
            agent_status,
            fs_read_file,
            fs_write_file,
            fs_write_bytes,
            app_data_dir,
            fs_list_dir,
            fs_path_exists,
            fs_bash_run,
            git_status,
            git_check_ignore,
            git_ls_files,
            git_branches,
            git_current_branch,
            git_checkout,
            git_create_branch,
            git_stage,
            git_unstage,
            git_discard,
            git_commit,
            git_push,
            git_pull,
            git_log,
            git_repo_root,
            git_repo_info,
            git_diff,
            git_show,
            git_commit_and_push,
            git_create_pr,
            git_gh_cli_available,
            pr_create_available,
            fs_watch_start,
            fs_watch_stop,
            worktree_create,
            worktree_remove,
            worktree_list,
            worktree_disk_usage,
            worktree_storage_dir,
            worktree_cleanup_orphans,
            worktree_diff,
            worktree_apply,
            biometric_unlock,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

async fn token() -> Result<String, String> {
    keychain::get(GITHUB_KEY)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "GitHub is not connected".to_string())
}

#[tauri::command]
async fn github_connect_pat(token: String) -> Result<GithubUser, String> {
    let trimmed = token.trim().to_string();
    if trimmed.is_empty() {
        return Err("token is empty".into());
    }
    let user = github::fetch_user(&trimmed).await.map_err(|e| e.to_string())?;
    keychain::set(GITHUB_KEY, &trimmed).map_err(|e| e.to_string())?;
    let _ = cursor_mcp::sync();
    Ok(user)
}

#[tauri::command]
async fn github_status() -> Result<ConnectionStatus, String> {
    match keychain::get(GITHUB_KEY).map_err(|e| e.to_string())? {
        None => Ok(ConnectionStatus::Disconnected),
        Some(t) => match github::fetch_user(&t).await {
            Ok(user) => Ok(ConnectionStatus::Connected { user }),
            Err(github::GithubError::InvalidToken) => {
                let _ = keychain::delete(GITHUB_KEY);
                Ok(ConnectionStatus::Disconnected)
            }
            Err(e) => Err(e.to_string()),
        },
    }
}

#[tauri::command]
fn github_disconnect() -> Result<(), String> {
    keychain::delete(GITHUB_KEY).map_err(|e| e.to_string())?;
    let _ = cursor_mcp::sync();
    Ok(())
}

/// Idempotent re-sync of `~/.cursor/mcp.json` with whatever creds are
/// currently in Keychain. Wired into every connect/disconnect so Cursor
/// sees the same Jira/GitHub/Sentry/Memory tools as Claude with no
/// manual `cursor-agent mcp add` step. Best-effort — a failure here
/// just means Cursor stays out of sync; doesn't break the connect flow.
#[tauri::command]
fn cursor_mcp_sync() -> Result<Vec<String>, String> {
    cursor_mcp::sync()
}

#[tauri::command]
async fn github_list_inbox() -> Result<Vec<InboxItem>, String> {
    let t = token().await?;
    github::search_involves_me(&t).await.map_err(|e| e.to_string())
}

/// Run a pre-built GitHub search query (`q=` as composed by the frontend).
/// Caller is responsible for combining filters — e.g. `involves:@me is:open`.
#[tauri::command]
async fn github_search_inbox(query: String) -> Result<Vec<InboxItem>, String> {
    let t = token().await?;
    github::search_issues_with_query(&t, &query)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn github_list_repos() -> Result<Vec<Repository>, String> {
    let t = token().await?;
    github::list_repos(&t).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn github_list_repo_items(
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
async fn github_get_inbox_item(
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
async fn github_list_workflow_runs(
    owner: String,
    repo: String,
) -> Result<Vec<WorkflowRun>, String> {
    let t = token().await?;
    github::list_workflow_runs(&t, &owner, &repo).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn github_rerun_workflow(owner: String, repo: String, run_id: u64) -> Result<(), String> {
    let t = token().await?;
    github::rerun_workflow(&t, &owner, &repo, run_id).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn github_cancel_workflow(owner: String, repo: String, run_id: u64) -> Result<(), String> {
    let t = token().await?;
    github::cancel_workflow(&t, &owner, &repo, run_id).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn github_list_tree(
    owner: String,
    repo: String,
    reference: String,
) -> Result<Vec<TreeEntry>, String> {
    let t = token().await?;
    github::list_tree(&t, &owner, &repo, &reference).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn github_get_file_content(
    owner: String,
    repo: String,
    path: String,
    reference: String,
) -> Result<FileBlob, String> {
    let t = token().await?;
    github::get_file_content(&t, &owner, &repo, &path, &reference).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn github_list_releases(owner: String, repo: String) -> Result<Vec<Release>, String> {
    let t = token().await?;
    github::list_releases(&t, &owner, &repo).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn github_list_repo_commits(
    owner: String,
    repo: String,
    reference: String,
    limit: u32,
) -> Result<Vec<RepoCommit>, String> {
    let t = token().await?;
    github::list_commits(&t, &owner, &repo, &reference, limit).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn github_list_repo_branches(owner: String, repo: String) -> Result<Vec<RepoBranch>, String> {
    let t = token().await?;
    github::list_branches_api(&t, &owner, &repo).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn github_get_readme(
    owner: String,
    repo: String,
) -> Result<Option<RepoReadme>, String> {
    let t = token().await?;
    github::get_readme(&t, &owner, &repo).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn github_get_pr(owner: String, repo: String, number: u64) -> Result<PrDetail, String> {
    let t = token().await?;
    github::get_pr_detail(&t, &owner, &repo, number).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn github_list_pr_files(
    owner: String,
    repo: String,
    number: u64,
) -> Result<Vec<ChangedFile>, String> {
    let t = token().await?;
    github::list_pr_files(&t, &owner, &repo, number).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn github_list_pr_commits(
    owner: String,
    repo: String,
    number: u64,
) -> Result<Vec<CommitEntry>, String> {
    let t = token().await?;
    github::list_pr_commits(&t, &owner, &repo, number).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn github_list_check_runs(
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
async fn github_list_pr_reviews(
    owner: String,
    repo: String,
    number: u64,
) -> Result<Vec<Review>, String> {
    let t = token().await?;
    github::list_pr_reviews(&t, &owner, &repo, number).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn github_list_review_comments(
    owner: String,
    repo: String,
    number: u64,
) -> Result<Vec<ReviewComment>, String> {
    let t = token().await?;
    github::list_pr_review_comments(&t, &owner, &repo, number).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn github_list_comments(
    owner: String,
    repo: String,
    number: u64,
) -> Result<Vec<Comment>, String> {
    let t = token().await?;
    github::list_issue_comments(&t, &owner, &repo, number).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn github_get_commit(
    owner: String,
    repo: String,
    sha: String,
) -> Result<CommitDetail, String> {
    let t = token().await?;
    github::get_commit(&t, &owner, &repo, &sha).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn github_add_comment(
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
async fn github_submit_review(
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
async fn github_set_state(
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
async fn github_merge_pr(
    owner: String,
    repo: String,
    number: u64,
    method: String,
) -> Result<(), String> {
    let t = token().await?;
    github::merge_pr(&t, &owner, &repo, number, &method).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn github_compare(
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
async fn github_create_pr(
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

// ---------- Jira ----------

#[tauri::command]
async fn jira_connect(
    workspace: String,
    email: String,
    token: String,
) -> Result<JiraUser, String> {
    let creds = JiraCredentials {
        workspace: jira::normalize_workspace(&workspace),
        email: email.trim().to_string(),
        token: token.trim().to_string(),
    };
    if creds.workspace.is_empty() {
        return Err("workspace URL is required".into());
    }
    if creds.email.is_empty() {
        return Err("email is required".into());
    }
    if creds.token.is_empty() {
        return Err("API token is required".into());
    }
    let user = jira::fetch_myself(&creds).await.map_err(|e| e.to_string())?;
    let payload = serde_json::to_string(&creds).map_err(|e| e.to_string())?;
    keychain::set(JIRA_KEY, &payload).map_err(|e| e.to_string())?;
    let _ = cursor_mcp::sync();
    Ok(user)
}

#[tauri::command]
async fn jira_status() -> Result<JiraStatus, String> {
    let Some(stored) = keychain::get(JIRA_KEY).map_err(|e| e.to_string())? else {
        return Ok(JiraStatus::Disconnected);
    };
    let creds: JiraCredentials = match serde_json::from_str(&stored) {
        Ok(c) => c,
        Err(_) => {
            let _ = keychain::delete(JIRA_KEY);
            return Ok(JiraStatus::Disconnected);
        }
    };
    match jira::fetch_myself(&creds).await {
        Ok(user) => Ok(JiraStatus::Connected { user }),
        Err(jira::JiraError::InvalidCredentials) => {
            let _ = keychain::delete(JIRA_KEY);
            Ok(JiraStatus::Disconnected)
        }
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
fn jira_disconnect() -> Result<(), String> {
    keychain::delete(JIRA_KEY).map_err(|e| e.to_string())?;
    let _ = cursor_mcp::sync();
    Ok(())
}

async fn jira_creds() -> Result<JiraCredentials, String> {
    let stored = keychain::get(JIRA_KEY)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Jira is not connected".to_string())?;
    serde_json::from_str(&stored).map_err(|e| e.to_string())
}

// ---------- Sentry ----------

#[tauri::command]
async fn sentry_connect(
    host: String,
    organization_slug: String,
    token: String,
) -> Result<SentryUser, String> {
    let creds = SentryCredentials {
        host: sentry::normalize_host(&host),
        organization_slug: organization_slug.trim().to_string(),
        token: token.trim().to_string(),
    };
    if creds.organization_slug.is_empty() {
        return Err("organization slug is required (e.g. 'acme-co')".into());
    }
    if creds.token.is_empty() {
        return Err("auth token is required".into());
    }
    let user = sentry::validate(&creds).await?;
    let payload = serde_json::to_string(&creds).map_err(|e| e.to_string())?;
    keychain::set(SENTRY_KEY, &payload).map_err(|e| e.to_string())?;
    let _ = cursor_mcp::sync();
    Ok(user)
}

#[tauri::command]
async fn sentry_status() -> Result<SentryConnectionStatus, String> {
    let Some(stored) = keychain::get(SENTRY_KEY).map_err(|e| e.to_string())? else {
        return Ok(SentryConnectionStatus::Disconnected);
    };
    let creds: SentryCredentials = match serde_json::from_str(&stored) {
        Ok(c) => c,
        Err(_) => {
            let _ = keychain::delete(SENTRY_KEY);
            return Ok(SentryConnectionStatus::Disconnected);
        }
    };
    match sentry::validate(&creds).await {
        Ok(user) => Ok(SentryConnectionStatus::Connected { user }),
        Err(_) => {
            // Token revoked / network blip — leave creds in keychain so the
            // user can retry; surface as disconnected for UX.
            Ok(SentryConnectionStatus::Disconnected)
        }
    }
}

#[tauri::command]
fn sentry_disconnect() -> Result<(), String> {
    keychain::delete(SENTRY_KEY).map_err(|e| e.to_string())?;
    let _ = cursor_mcp::sync();
    Ok(())
}

async fn sentry_creds() -> Result<SentryCredentials, String> {
    let stored = keychain::get(SENTRY_KEY)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Sentry is not connected".to_string())?;
    serde_json::from_str(&stored).map_err(|e| e.to_string())
}

#[tauri::command]
async fn sentry_list_issues(
    query: Option<String>,
    project_slugs: Option<Vec<String>>,
    environment: Option<String>,
    sort: Option<String>,
    limit: Option<u32>,
) -> Result<Vec<SentryIssue>, String> {
    let creds = sentry_creds().await?;
    sentry::list_issues(
        &creds,
        query.as_deref(),
        project_slugs.as_deref().unwrap_or(&[]),
        environment.as_deref(),
        sort.as_deref().unwrap_or("date"),
        limit.unwrap_or(50),
    )
    .await
}

#[tauri::command]
async fn sentry_get_issue(issue_id: String) -> Result<SentryIssue, String> {
    let creds = sentry_creds().await?;
    sentry::get_issue(&creds, &issue_id).await
}

#[tauri::command]
async fn sentry_list_events(issue_id: String, limit: Option<u32>) -> Result<Vec<SentryEvent>, String> {
    let creds = sentry_creds().await?;
    sentry::list_events(&creds, &issue_id, limit.unwrap_or(20)).await
}

#[tauri::command]
async fn sentry_list_projects() -> Result<Vec<SentryProject>, String> {
    let creds = sentry_creds().await?;
    sentry::list_projects(&creds).await
}

#[tauri::command]
async fn sentry_list_environments(project_slug: String) -> Result<Vec<SentryEnvironment>, String> {
    let creds = sentry_creds().await?;
    sentry::list_environments(&creds, &project_slug).await
}

#[tauri::command]
async fn sentry_get_event_detail(
    issue_id: String,
    event_id: Option<String>,
) -> Result<SentryEventDetail, String> {
    let creds = sentry_creds().await?;
    sentry::get_event_detail(&creds, &issue_id, event_id.as_deref().unwrap_or("latest")).await
}

#[tauri::command]
async fn sentry_set_status(issue_id: String, status: String) -> Result<SentryIssue, String> {
    let creds = sentry_creds().await?;
    sentry::set_issue_status(&creds, &issue_id, &status).await
}

#[tauri::command]
async fn jira_list_inbox() -> Result<Vec<JiraItem>, String> {
    let creds = jira_creds().await?;
    jira::list_my_issues(&creds).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn jira_list_inbox_for(
    assignee_account_id: Option<String>,
) -> Result<Vec<JiraItem>, String> {
    let creds = jira_creds().await?;
    jira::list_issues_for(&creds, assignee_account_id.as_deref())
        .await
        .map_err(|e| e.to_string())
}

/// Run a pre-built JQL query (composed by the frontend from its filter state).
#[tauri::command]
async fn jira_search(jql: String) -> Result<Vec<JiraItem>, String> {
    let creds = jira_creds().await?;
    jira::search_issues(&creds, &jql).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn jira_list_projects() -> Result<Vec<JiraProject>, String> {
    let creds = jira_creds().await?;
    jira::list_projects(&creds).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn jira_list_boards(project_key: Option<String>) -> Result<Vec<JiraBoard>, String> {
    let creds = jira_creds().await?;
    jira::list_boards(&creds, project_key.as_deref())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn jira_list_sprints(board_id: u64) -> Result<Vec<JiraSprint>, String> {
    let creds = jira_creds().await?;
    jira::list_sprints(&creds, board_id).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn jira_list_statuses(
    project_key: Option<String>,
) -> Result<Vec<JiraWorkflowStatus>, String> {
    let creds = jira_creds().await?;
    jira::list_statuses(&creds, project_key.as_deref())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn jira_list_issue_types(project_key: String) -> Result<Vec<JiraIssueType>, String> {
    let creds = jira_creds().await?;
    jira::list_issue_types(&creds, &project_key).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn jira_create_issue(
    project_key: String,
    issue_type: String,
    summary: String,
    description: String,
    assignee_account_id: Option<String>,
    sprint_id: Option<u64>,
) -> Result<JiraItem, String> {
    let creds = jira_creds().await?;
    jira::create_issue(
        &creds,
        &project_key,
        &issue_type,
        &summary,
        &description,
        assignee_account_id.as_deref(),
        sprint_id,
    )
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn jira_search_users(query: String) -> Result<Vec<JiraUserSummary>, String> {
    let creds = jira_creds().await?;
    jira::search_users(&creds, &query).await.map_err(|e| e.to_string())
}

#[tauri::command]
#[allow(non_snake_case)]
async fn jira_list_assignable_users(projectKey: String) -> Result<Vec<JiraUserSummary>, String> {
    let creds = jira_creds().await?;
    jira::list_assignable_users(&creds, &projectKey).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn jira_set_assignee(key: String, account_id: Option<String>) -> Result<(), String> {
    let creds = jira_creds().await?;
    jira::set_assignee(&creds, &key, account_id.as_deref())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn jira_set_priority(key: String, priority: String) -> Result<(), String> {
    let creds = jira_creds().await?;
    jira::set_priority(&creds, &key, &priority).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn jira_set_labels(key: String, labels: Vec<String>) -> Result<(), String> {
    let creds = jira_creds().await?;
    jira::set_labels(&creds, &key, labels).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn jira_get_issue_detail(key: String) -> Result<JiraDetail, String> {
    let creds = jira_creds().await?;
    jira::get_issue_detail(&creds, &key).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn jira_update_issue(
    key: String,
    summary: Option<String>,
    description: Option<String>,
) -> Result<(), String> {
    let creds = jira_creds().await?;
    jira::update_issue(&creds, &key, summary.as_deref(), description.as_deref())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn jira_transition_issue(key: String, transition_id: String) -> Result<(), String> {
    let creds = jira_creds().await?;
    jira::transition_issue(&creds, &key, &transition_id).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn jira_add_comment(key: String, body: String) -> Result<JiraComment, String> {
    let creds = jira_creds().await?;
    jira::add_comment(&creds, &key, &body).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn jira_list_worklogs(key: String) -> Result<Vec<JiraWorklog>, String> {
    let creds = jira_creds().await?;
    jira::list_worklogs(&creds, &key).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn jira_add_worklog(
    key: String,
    #[allow(non_snake_case)] timeSpentSeconds: i64,
    started: Option<String>,
    comment: Option<String>,
) -> Result<JiraWorklog, String> {
    let creds = jira_creds().await?;
    jira::add_worklog(
        &creds,
        &key,
        timeSpentSeconds,
        started.as_deref(),
        comment.as_deref(),
    )
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn jira_delete_worklog(
    key: String,
    #[allow(non_snake_case)] worklogId: String,
) -> Result<(), String> {
    let creds = jira_creds().await?;
    jira::delete_worklog(&creds, &key, &worklogId).await.map_err(|e| e.to_string())
}

// ---------- Claude Code ----------

#[tauri::command]
fn claude_status() -> ClaudeStatus {
    claude::detect()
}

#[tauri::command]
async fn claude_ask(
    app: tauri::AppHandle,
    runners: State<'_, Runners>,
    session_id: String,
    prompt: String,
    cwd: Option<String>,
    claude_uuid: String,
    resume: bool,
    rules: Option<String>,
    // camelCase on the wire (Svelte invokes pass JS-style keys); snake_case
    // is what Tauri's codegen expects, so spell the param out explicitly to
    // keep the frontend contract stable.
    #[allow(non_snake_case)] agentKind: Option<AgentKind>,
    #[allow(non_snake_case)] cursorModel: Option<String>,
    // Per-turn dynamic context describing the agent's UI surroundings —
    // active workbench, sibling instances + their names + cwds, and which
    // instance the calling session is bound to. Built fresh on the
    // frontend before every turn (workbench layout changes, sessions
    // change cwd) so the agent always sees current state. Prepended to
    // the system prompt for Claude / to the prompt itself for Cursor.
    #[allow(non_snake_case)] appContext: Option<String>,
    // Absolute paths of image files attached to this turn. For Claude these
    // get base64-embedded as `image` content blocks via stream-json input
    // (the model sees the bytes directly via vision). Empty for Cursor —
    // its CLI has no equivalent input format, so the frontend falls back
    // to the path-mention flow there.
    #[allow(non_snake_case)] imagePaths: Option<Vec<String>>,
) -> Result<AgentAskResult, String> {
    let cwd_path = cwd.as_deref().map(std::path::Path::new);
    let kind = agentKind.unwrap_or_default();
    let images = imagePaths.unwrap_or_default();
    agent::ask(
        kind,
        app,
        runners.inner().clone(),
        &session_id,
        &prompt,
        cwd_path,
        &claude_uuid,
        resume,
        rules.as_deref(),
        cursorModel.as_deref(),
        appContext.as_deref(),
        &images,
    )
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
fn agent_status() -> AgentStatus {
    agent::detect_all()
}

#[tauri::command]
fn claude_stop(runners: State<'_, Runners>, session_id: String) -> Result<bool, String> {
    // Try both adapters — whichever one owns the PID for this session does
    // the kill. Keeps the command name backwards-compatible even though it
    // no longer targets only Claude.
    Ok(agent::stop(&runners, &session_id))
}

#[tauri::command]
async fn agent_generate_commit_message(
    repo: String,
    #[allow(non_snake_case)] agentKind: AgentKind,
) -> Result<String, String> {
    let path = std::path::PathBuf::from(&repo);
    agent::generate_commit_message(agentKind, &path)
        .await
        .map_err(|e| e.to_string())
}

// ---------- FS ----------

#[tauri::command]
fn fs_read_file(path: String) -> Result<String, String> {
    fs::read_file(&path)
}

#[tauri::command]
fn fs_write_file(path: String, contents: String) -> Result<(), String> {
    fs::write_file(&path, &contents)
}

#[tauri::command]
fn fs_write_bytes(path: String, base64: String) -> Result<(), String> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(base64.as_bytes())
        .map_err(|e| format!("invalid base64: {}", e))?;
    fs::write_bytes(&path, &bytes)
}

/// Resolve the OS-level app data dir (`~/Library/Application Support/<id>` on
/// macOS) for the frontend. Used as a stable cwd-independent home for chat
/// image attachments saved from clipboard or Cmd+Shift+5 drag, where we have
/// only the bytes (no source path).
#[tauri::command]
fn app_data_dir(app: tauri::AppHandle) -> Result<String, String> {
    use tauri::Manager;
    app.path()
        .app_data_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn fs_list_dir(path: String) -> Result<Vec<DirEntry>, String> {
    fs::list_dir(&path)
}

#[tauri::command]
fn fs_path_exists(path: String) -> bool {
    fs::path_exists(&path)
}

#[tauri::command]
fn fs_bash_run(cwd: String, command: String) -> Result<BashResult, String> {
    fs::bash_run(&cwd, &command)
}

// ---------- Git ----------

#[tauri::command]
fn git_status(repo: String) -> Result<GitStatus, String> {
    git::status(&repo)
}

#[tauri::command]
fn git_check_ignore(repo: String, paths: Vec<String>) -> Vec<String> {
    git::check_ignore(&repo, &paths)
}

#[tauri::command]
fn git_ls_files(repo: String) -> Vec<String> {
    git::ls_files(&repo)
}

#[tauri::command]
fn git_branches(repo: String) -> Result<Vec<Branch>, String> {
    git::branches(&repo)
}

#[tauri::command]
fn git_current_branch(repo: String) -> Result<String, String> {
    git::current_branch(&repo)
}

#[tauri::command]
fn git_checkout(repo: String, branch: String) -> Result<(), String> {
    git::checkout(&repo, &branch)
}

#[tauri::command]
fn git_create_branch(repo: String, name: String, checkout: bool) -> Result<(), String> {
    git::create_branch(&repo, &name, checkout)
}

#[tauri::command]
fn git_stage(repo: String, paths: Vec<String>) -> Result<(), String> {
    git::stage(&repo, &paths)
}

#[tauri::command]
fn git_unstage(repo: String, paths: Vec<String>) -> Result<(), String> {
    git::unstage(&repo, &paths)
}

#[tauri::command]
fn git_discard(repo: String, paths: Vec<String>) -> Result<(), String> {
    git::discard(&repo, &paths)
}

#[tauri::command]
fn git_commit(repo: String, message: String) -> Result<String, String> {
    git::commit(&repo, &message)
}

#[tauri::command]
fn git_push(repo: String) -> Result<String, String> {
    git::push(&repo)
}

#[tauri::command]
fn git_pull(repo: String) -> Result<String, String> {
    git::pull(&repo)
}

#[tauri::command]
fn git_log(repo: String, limit: u32) -> Result<Vec<GitCommitEntry>, String> {
    git::log(&repo, limit)
}

#[tauri::command]
fn git_repo_root(path: String) -> Result<String, String> {
    git::repo_root(&path)
}

#[tauri::command]
fn git_repo_info(path: String) -> RepoInfo {
    git::repo_info(&path)
}

#[tauri::command]
fn git_diff(repo: String, path: String, staged: bool) -> Result<String, String> {
    git::diff(&repo, &path, staged)
}

#[tauri::command]
fn git_show(repo: String, revision: String, path: String) -> Result<String, String> {
    git::show(&repo, &revision, &path)
}

#[tauri::command]
fn git_commit_and_push(repo: String, message: String) -> Result<String, String> {
    let sha = git::commit(&repo, &message)?;
    let push_out = git::push(&repo)?;
    Ok(format!("{}\n{}", sha, push_out.trim()))
}

#[tauri::command]
async fn git_create_pr(
    repo: String,
    title: String,
    body: String,
    draft: bool,
    base: Option<String>,
) -> Result<String, String> {
    let token = token().await?;
    git::create_pr(&repo, &title, &body, draft, base.as_deref(), &token).await
}

#[tauri::command]
fn git_gh_cli_available() -> bool {
    // Kept for any legacy callers; modern PR creation no longer needs the
    // `gh` CLI — it hits the GitHub REST API with the Keychain token.
    git::gh_cli_available()
}

/// True iff a GitHub token is stored in Keychain — i.e. PR creation through
/// `git_create_pr` will work. Cheaper than `github_status` (no API call).
#[tauri::command]
fn pr_create_available() -> bool {
    keychain::get(GITHUB_KEY)
        .ok()
        .flatten()
        .map(|t| !t.trim().is_empty())
        .unwrap_or(false)
}

#[tauri::command]
fn fs_watch_start(
    state: State<'_, WatcherState>,
    app: tauri::AppHandle,
    path: String,
) -> Result<(), String> {
    watch::start(state.inner(), app, &path)
}

#[tauri::command]
fn fs_watch_stop(state: State<'_, WatcherState>) -> Result<(), String> {
    watch::stop(state.inner());
    Ok(())
}

// ---------- Worktree (per-session isolated git worktrees) ----------

#[tauri::command]
fn worktree_create(
    repo: String,
    session_id: String,
    base_ref: Option<String>,
) -> Result<Worktree, String> {
    worktree::create(&repo, &session_id, base_ref.as_deref())
}

#[tauri::command]
fn worktree_remove(repo: String, session_id: String) -> Result<(), String> {
    worktree::remove(&repo, &session_id)
}

#[tauri::command]
fn worktree_list(repo: String) -> Result<Vec<Worktree>, String> {
    worktree::list(&repo)
}

#[derive(Debug, Serialize, Clone)]
struct WorktreeDiff {
    files: Vec<WorktreeChangedFile>,
    raw: String,
}

#[tauri::command]
fn worktree_diff(
    repo: String,
    session_id: String,
    base_ref: Option<String>,
) -> Result<WorktreeDiff, String> {
    let (files, raw) = worktree::diff(&repo, &session_id, base_ref.as_deref())?;
    Ok(WorktreeDiff { files, raw })
}

#[tauri::command]
fn worktree_apply(repo: String, session_id: String) -> Result<String, String> {
    worktree::apply(&repo, &session_id)
}

#[tauri::command]
fn worktree_disk_usage() -> u64 {
    worktree::disk_usage_bytes()
}

#[tauri::command]
fn worktree_storage_dir() -> Option<String> {
    worktree::storage_root().map(|p| p.to_string_lossy().to_string())
}

#[tauri::command]
fn worktree_cleanup_orphans(
    active_session_ids: Vec<String>,
    max_age_secs: u64,
) -> worktree::CleanupSummary {
    worktree::cleanup_orphans(&active_session_ids, max_age_secs)
}

// ---------- Biometry (Touch ID) ----------

#[tauri::command]
async fn biometric_unlock(reason: Option<String>) -> Result<(), String> {
    let reason = reason.unwrap_or_else(|| "Unlock Forgehold".to_string());
    biometry::authenticate(&reason).await
}
