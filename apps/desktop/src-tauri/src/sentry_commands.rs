//! Sentry Tauri commands. Extracted from `lib.rs` in wave-28 split.
//! Each command pulls Sentry creds (host + org + token) via the shared
//! `sentry_creds()` helper, calls into `crate::sentry::*`, and maps
//! errors to the `String` shape Tauri serializes back to the frontend.
//!
//! `sentry_connect` / `sentry_status` / `sentry_disconnect` are the
//! connection-lifecycle trio — they also fan out to
//! `crate::cursor_mcp::sync()` so `~/.cursor/mcp.json` mirrors the
//! current keychain state.

use crate::{cursor_mcp, keychain, sentry, SentryConnectionStatus, SENTRY_KEY};
use sentry::{
    SentryCredentials, SentryEnvironment, SentryEvent, SentryEventDetail, SentryIssue,
    SentryProject, SentryUser,
};

#[tauri::command]
pub async fn sentry_connect(
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
pub async fn sentry_status() -> Result<SentryConnectionStatus, String> {
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
pub fn sentry_disconnect() -> Result<(), String> {
    keychain::delete(SENTRY_KEY).map_err(|e| e.to_string())?;
    let _ = cursor_mcp::sync();
    Ok(())
}

pub(crate) async fn sentry_creds() -> Result<SentryCredentials, String> {
    let stored = keychain::get(SENTRY_KEY)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Sentry is not connected".to_string())?;
    serde_json::from_str(&stored).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn sentry_list_issues(
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
pub async fn sentry_get_issue(issue_id: String) -> Result<SentryIssue, String> {
    let creds = sentry_creds().await?;
    sentry::get_issue(&creds, &issue_id).await
}

#[tauri::command]
pub async fn sentry_list_events(issue_id: String, limit: Option<u32>) -> Result<Vec<SentryEvent>, String> {
    let creds = sentry_creds().await?;
    sentry::list_events(&creds, &issue_id, limit.unwrap_or(20)).await
}

#[tauri::command]
pub async fn sentry_list_projects() -> Result<Vec<SentryProject>, String> {
    let creds = sentry_creds().await?;
    sentry::list_projects(&creds).await
}

#[tauri::command]
pub async fn sentry_list_environments(project_slug: String) -> Result<Vec<SentryEnvironment>, String> {
    let creds = sentry_creds().await?;
    sentry::list_environments(&creds, &project_slug).await
}

#[tauri::command]
pub async fn sentry_get_event_detail(
    issue_id: String,
    event_id: Option<String>,
) -> Result<SentryEventDetail, String> {
    let creds = sentry_creds().await?;
    sentry::get_event_detail(&creds, &issue_id, event_id.as_deref().unwrap_or("latest")).await
}

#[tauri::command]
pub async fn sentry_set_status(issue_id: String, status: String) -> Result<SentryIssue, String> {
    let creds = sentry_creds().await?;
    sentry::set_issue_status(&creds, &issue_id, &status).await
}
