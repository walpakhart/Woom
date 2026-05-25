//! Jira Tauri commands. Extracted from `lib.rs` in wave-28 split. Each
//! command pulls Jira creds (workspace URL + email + token) via the
//! shared `jira_creds()` helper, calls into `crate::jira::*`, and maps
//! errors to the `String` shape Tauri serializes back to the frontend.
//!
//! `jira_connect` / `jira_status` / `jira_disconnect` are the
//! connection-lifecycle trio — they also fan out to
//! `crate::cursor_mcp::sync()` so `~/.cursor/mcp.json` mirrors the
//! current keychain state.

use crate::{cursor_mcp, jira, keychain, JiraStatus, JIRA_KEY};
use jira::{
    JiraBoard, JiraComment, JiraCredentials, JiraDetail, JiraIssueType, JiraItem, JiraProject,
    JiraSprint, JiraStatus as JiraWorkflowStatus, JiraUser, JiraUserSummary, JiraWorklog,
};

#[tauri::command]
pub async fn jira_connect(
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
pub async fn jira_status() -> Result<JiraStatus, String> {
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
pub fn jira_disconnect() -> Result<(), String> {
    keychain::delete(JIRA_KEY).map_err(|e| e.to_string())?;
    let _ = cursor_mcp::sync();
    Ok(())
}

pub(crate) async fn jira_creds() -> Result<JiraCredentials, String> {
    let stored = keychain::get(JIRA_KEY)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Jira is not connected".to_string())?;
    serde_json::from_str(&stored).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn jira_list_inbox() -> Result<Vec<JiraItem>, String> {
    let creds = jira_creds().await?;
    jira::list_my_issues(&creds).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn jira_list_inbox_for(
    assignee_account_id: Option<String>,
) -> Result<Vec<JiraItem>, String> {
    let creds = jira_creds().await?;
    jira::list_issues_for(&creds, assignee_account_id.as_deref())
        .await
        .map_err(|e| e.to_string())
}

/// Run a pre-built JQL query (composed by the frontend from its filter state).
#[tauri::command]
pub async fn jira_search(jql: String) -> Result<Vec<JiraItem>, String> {
    let creds = jira_creds().await?;
    jira::search_issues(&creds, &jql).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn jira_list_projects() -> Result<Vec<JiraProject>, String> {
    let creds = jira_creds().await?;
    jira::list_projects(&creds).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn jira_list_boards(project_key: Option<String>) -> Result<Vec<JiraBoard>, String> {
    let creds = jira_creds().await?;
    jira::list_boards(&creds, project_key.as_deref())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn jira_list_sprints(board_id: u64) -> Result<Vec<JiraSprint>, String> {
    let creds = jira_creds().await?;
    jira::list_sprints(&creds, board_id).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn jira_list_statuses(
    project_key: Option<String>,
) -> Result<Vec<JiraWorkflowStatus>, String> {
    let creds = jira_creds().await?;
    jira::list_statuses(&creds, project_key.as_deref())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn jira_list_issue_types(project_key: String) -> Result<Vec<JiraIssueType>, String> {
    let creds = jira_creds().await?;
    jira::list_issue_types(&creds, &project_key).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn jira_create_issue(
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
pub async fn jira_search_users(query: String) -> Result<Vec<JiraUserSummary>, String> {
    let creds = jira_creds().await?;
    jira::search_users(&creds, &query).await.map_err(|e| e.to_string())
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn jira_list_assignable_users(projectKey: String) -> Result<Vec<JiraUserSummary>, String> {
    let creds = jira_creds().await?;
    jira::list_assignable_users(&creds, &projectKey).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn jira_set_assignee(key: String, account_id: Option<String>) -> Result<(), String> {
    let creds = jira_creds().await?;
    jira::set_assignee(&creds, &key, account_id.as_deref())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn jira_set_priority(key: String, priority: String) -> Result<(), String> {
    let creds = jira_creds().await?;
    jira::set_priority(&creds, &key, &priority).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn jira_set_labels(key: String, labels: Vec<String>) -> Result<(), String> {
    let creds = jira_creds().await?;
    jira::set_labels(&creds, &key, labels).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn jira_get_issue_detail(key: String) -> Result<JiraDetail, String> {
    let creds = jira_creds().await?;
    jira::get_issue_detail(&creds, &key).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn jira_update_issue(
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
pub async fn jira_transition_issue(key: String, transition_id: String) -> Result<(), String> {
    let creds = jira_creds().await?;
    jira::transition_issue(&creds, &key, &transition_id).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn jira_add_comment(key: String, body: String) -> Result<JiraComment, String> {
    let creds = jira_creds().await?;
    jira::add_comment(&creds, &key, &body).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn jira_list_worklogs(key: String) -> Result<Vec<JiraWorklog>, String> {
    let creds = jira_creds().await?;
    jira::list_worklogs(&creds, &key).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn jira_add_worklog(
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
pub async fn jira_delete_worklog(
    key: String,
    #[allow(non_snake_case)] worklogId: String,
) -> Result<(), String> {
    let creds = jira_creds().await?;
    jira::delete_worklog(&creds, &key, &worklogId).await.map_err(|e| e.to_string())
}
