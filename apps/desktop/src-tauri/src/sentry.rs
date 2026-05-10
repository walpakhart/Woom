//! Sentry source — read-only HTTP client to a Sentry org's issues feed.
//!
//! Auth is a single user-pasted Auth Token (Sentry's name for a PAT).
//! Tokens are minted at `<host>/settings/account/api/auth-tokens/` with
//! `org:read`, `project:read`, `event:read` scopes. We store the token,
//! the org slug, and the base host together so cloud and self-hosted
//! installs both work. Credentials live in macOS Keychain — same path
//! as github/jira, single-blob JSON entry.
//!
//! No write paths in v0.1: the user reads issues here, drags them onto
//! Claude, and the agent does the actual work via the MCP sidecar.

use serde::{Deserialize, Serialize};

/// Persisted credentials. Serialized to JSON and stored as a single
/// keychain entry under `SENTRY_KEY` (see `lib.rs`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SentryCredentials {
    /// Auth token from `<host>/settings/account/api/auth-tokens/`.
    pub token: String,
    /// Org slug — the URL-safe handle (`acme-co`), not the display name.
    pub organization_slug: String,
    /// Base host, e.g. `https://sentry.io` or `https://sentry.acme.co`.
    /// Trailing slash and any path are stripped.
    pub host: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SentryUser {
    pub id: String,
    pub email: Option<String>,
    pub username: Option<String>,
    pub name: Option<String>,
    pub organization_slug: String,
    pub organization_name: String,
    pub host: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SentryIssue {
    pub id: String,
    pub short_id: String,
    pub title: String,
    pub culprit: Option<String>,
    pub level: String,
    pub status: String,
    pub platform: Option<String>,
    pub project_slug: String,
    pub project_name: String,
    pub count: String,
    pub user_count: u64,
    pub first_seen: String,
    pub last_seen: String,
    pub permalink: String,
    pub metadata_type: Option<String>,
    pub metadata_value: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SentryEvent {
    pub event_id: String,
    pub date_created: String,
    pub message: Option<String>,
    pub platform: Option<String>,
    /// First-line summary of the exception type + value, when present.
    pub exception_summary: Option<String>,
    /// Permalink to the event on Sentry — opens the rich frame view.
    pub permalink: Option<String>,
}

/// Project metadata — the slug is what query strings need (`project:foo`).
#[derive(Debug, Clone, Serialize)]
pub struct SentryProject {
    pub id: String,
    pub slug: String,
    pub name: String,
    pub platform: Option<String>,
    pub is_member: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SentryEnvironment {
    pub id: String,
    pub name: String,
}

/// Stack frame as it appears in `entries.exception.values[].stacktrace.frames[]`.
/// Innermost first when reversed; we keep API order (outer → innermost).
#[derive(Debug, Clone, Serialize)]
pub struct SentryStackFrame {
    pub function: Option<String>,
    pub filename: Option<String>,
    pub abs_path: Option<String>,
    pub lineno: Option<u64>,
    pub colno: Option<u64>,
    pub in_app: bool,
    pub context: Vec<SentryContextLine>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SentryContextLine {
    pub line: u64,
    pub source: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SentryException {
    pub r#type: Option<String>,
    pub value: Option<String>,
    pub module: Option<String>,
    pub frames: Vec<SentryStackFrame>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SentryEventDetail {
    pub event_id: String,
    pub date_created: String,
    pub platform: Option<String>,
    pub message: Option<String>,
    pub culprit: Option<String>,
    pub user_email: Option<String>,
    pub user_id: Option<String>,
    pub user_ip: Option<String>,
    pub release: Option<String>,
    pub environment: Option<String>,
    pub tags: Vec<(String, String)>,
    pub exceptions: Vec<SentryException>,
    /// Newline-joined breadcrumb messages, capped — too noisy as structured.
    pub breadcrumbs_summary: Option<String>,
    pub permalink: Option<String>,
}

const USER_AGENT: &str = concat!("woom-desktop/", env!("CARGO_PKG_VERSION"));

/// Strip protocol-trailing-slash slop from whatever the user pasted into
/// the host field. Returns `https://sentry.io` for empty input.
pub fn normalize_host(raw: &str) -> String {
    let trimmed = raw.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return "https://sentry.io".to_string();
    }
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        trimmed.to_string()
    } else {
        format!("https://{}", trimmed)
    }
}

fn http() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .expect("reqwest client")
}

/// Validate the credentials by hitting `/api/0/` (auth + base) and
/// `/api/0/organizations/{slug}/` (org access). Returns the populated
/// `SentryUser` ready to push into the connection store.
pub async fn validate(creds: &SentryCredentials) -> Result<SentryUser, String> {
    let client = http();
    // Step 1: identity.
    let me_url = format!("{}/api/0/", creds.host);
    let resp = client
        .get(&me_url)
        .bearer_auth(&creds.token)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| format!("Sentry unreachable: {}", e))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Sentry auth failed ({}): {}", status, truncate(&body, 240)));
    }
    let me_json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Sentry returned non-JSON: {}", e))?;
    // /api/0/ returns either { user: {...} } when authed, or a public payload.
    let user = me_json.get("user").cloned().unwrap_or(serde_json::Value::Null);
    let user_id = user
        .get("id")
        .and_then(|v| v.as_str())
        .or_else(|| user.get("id").and_then(|v| v.as_u64()).map(|_| ""))
        .unwrap_or("")
        .to_string();
    if user_id.is_empty() {
        return Err("Sentry didn't return a user — token may be missing scopes (need org:read).".into());
    }
    let email = user
        .get("email")
        .and_then(|v| v.as_str())
        .map(String::from);
    let username = user
        .get("username")
        .and_then(|v| v.as_str())
        .map(String::from);
    let name = user
        .get("name")
        .and_then(|v| v.as_str())
        .map(String::from);

    // Step 2: org access.
    let org_url = format!("{}/api/0/organizations/{}/", creds.host, creds.organization_slug);
    let org_resp = client
        .get(&org_url)
        .bearer_auth(&creds.token)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| format!("org lookup failed: {}", e))?;
    if !org_resp.status().is_success() {
        let status = org_resp.status();
        let body = org_resp.text().await.unwrap_or_default();
        return Err(format!(
            "Org '{}' not accessible ({}): {}",
            creds.organization_slug,
            status,
            truncate(&body, 200)
        ));
    }
    let org_json: serde_json::Value = org_resp
        .json()
        .await
        .map_err(|e| format!("org returned non-JSON: {}", e))?;
    let org_name = org_json
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or(&creds.organization_slug)
        .to_string();

    Ok(SentryUser {
        id: user_id,
        email,
        username,
        name,
        organization_slug: creds.organization_slug.clone(),
        organization_name: org_name,
        host: creds.host.clone(),
    })
}

/// List issues for the org. `query` follows Sentry's search syntax — empty
/// string = "everything unresolved sorted by last seen". `project_slugs`
/// scopes to specific projects (multi-select; empty = all). `environment`
/// filters to a single env name. `sort` is one of `date|new|priority|freq|user`.
pub async fn list_issues(
    creds: &SentryCredentials,
    query: Option<&str>,
    project_slugs: &[String],
    environment: Option<&str>,
    sort: &str,
    limit: u32,
) -> Result<Vec<SentryIssue>, String> {
    let client = http();
    let q = query.unwrap_or("is:unresolved");
    // Sentry expects numeric project IDs. We resolve from slug → id via a
    // single projects-list lookup so the user-facing UI stays slug-based.
    // Cached on each call (cheap; org rarely has hundreds of projects).
    let mut project_param = String::new();
    if !project_slugs.is_empty() {
        let projects = list_projects_inner(&client, creds).await.unwrap_or_default();
        let by_slug: std::collections::HashMap<&str, &str> = projects
            .iter()
            .map(|p| (p.slug.as_str(), p.id.as_str()))
            .collect();
        for slug in project_slugs {
            if let Some(id) = by_slug.get(slug.as_str()) {
                project_param.push_str(&format!("&project={}", id));
            }
        }
    }
    let env_param = environment
        .map(|e| format!("&environment={}", urlencoding::encode(e)))
        .unwrap_or_default();
    let sort_safe = match sort {
        "new" | "priority" | "freq" | "user" | "date" => sort,
        _ => "date",
    };
    let url = format!(
        "{}/api/0/organizations/{}/issues/?query={}&limit={}&sort={}{}{}",
        creds.host,
        creds.organization_slug,
        urlencoding::encode(q),
        limit.min(100),
        sort_safe,
        project_param,
        env_param,
    );
    let resp = client
        .get(&url)
        .bearer_auth(&creds.token)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| format!("network: {}", e))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("issues {}: {}", status, truncate(&body, 240)));
    }
    let arr: Vec<serde_json::Value> = resp
        .json()
        .await
        .map_err(|e| format!("malformed JSON: {}", e))?;
    Ok(arr.into_iter().map(parse_issue).collect())
}

/// Fetch one issue by id (numeric or short-id). For short-ids
/// (`AUDIT-30`, `BMS-API-J6`) the `/api/0/issues/{id}/` endpoint
/// returns 404 — Sentry routes those by numeric id only. Resolve via
/// the org-scoped `shortids` endpoint first when the input looks like
/// a short id (i.e. contains a non-digit character).
pub async fn get_issue(
    creds: &SentryCredentials,
    issue_id: &str,
) -> Result<SentryIssue, String> {
    let resolved = resolve_to_numeric_id(creds, issue_id).await?;
    let client = http();
    let url = format!("{}/api/0/issues/{}/", creds.host, resolved);
    let resp = client
        .get(&url)
        .bearer_auth(&creds.token)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| format!("network: {}", e))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("issue {}: {}", status, truncate(&body, 240)));
    }
    let v: serde_json::Value = resp.json().await.map_err(|e| format!("JSON: {}", e))?;
    Ok(parse_issue(v))
}

/// Map a Sentry short-id (`AUDIT-30`, `BMS-API-J6`) to its numeric
/// issue id. Pure pass-through when the input is already numeric.
/// Used by every endpoint that takes an issue id since the agent
/// often hands us short-ids straight from search results.
///
/// Edge cases we handle:
/// - empty / whitespace → fast-fail, don't hit `…/shortids//`
/// - already-numeric → pass through, no network round-trip
/// - the `"latest"` event-id alias mistakenly passed as an issue id
///   (Sentry's REST API uses `latest` as an alias for the most-recent
///   *event*, not an issue) → fast-fail with a readable message
/// - URL-encode both the org slug and the short id so a slug with
///   non-ASCII or a malformed id with a `/` doesn't inject a bonus
///   path segment.
pub async fn resolve_to_numeric_id(
    creds: &SentryCredentials,
    issue_id: &str,
) -> Result<String, String> {
    let trimmed = issue_id.trim();
    if trimmed.is_empty() {
        return Err("empty issue id".to_string());
    }
    if trimmed.eq_ignore_ascii_case("latest") {
        return Err("\"latest\" is an event-id alias, not an issue id".to_string());
    }
    // Already numeric → no lookup needed.
    if trimmed.chars().all(|c| c.is_ascii_digit()) {
        return Ok(trimmed.to_string());
    }
    let client = http();
    // `shortids/{short_id}/` returns the group object with `id` (numeric).
    let url = format!(
        "{}/api/0/organizations/{}/shortids/{}/",
        creds.host,
        urlencoding::encode(&creds.organization_slug),
        urlencoding::encode(trimmed)
    );
    let resp = client
        .get(&url)
        .bearer_auth(&creds.token)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| format!("network resolving short id: {}", e))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!(
            "resolve short id {} {}: {}",
            trimmed,
            status,
            truncate(&body, 200)
        ));
    }
    let v: serde_json::Value = resp.json().await.map_err(|e| format!("JSON: {}", e))?;
    // The shortids endpoint returns `{ "shortId": "...", "group": { "id": "...", ... } }`.
    let group_id = v
        .get("group")
        .and_then(|g| g.get("id"))
        .and_then(|i| i.as_str())
        .or_else(|| v.get("groupId").and_then(|i| i.as_str()))
        .ok_or_else(|| format!("short id {} resolved but no numeric id in response", trimmed))?
        .to_string();
    Ok(group_id)
}

/// List recent events (occurrences) for an issue. The `latest` event
/// alias is special-cased by Sentry — it returns the most recent.
pub async fn list_events(
    creds: &SentryCredentials,
    issue_id: &str,
    limit: u32,
) -> Result<Vec<SentryEvent>, String> {
    let resolved = resolve_to_numeric_id(creds, issue_id).await?;
    let client = http();
    let url = format!(
        "{}/api/0/issues/{}/events/?limit={}",
        creds.host,
        resolved,
        limit.min(50)
    );
    let resp = client
        .get(&url)
        .bearer_auth(&creds.token)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| format!("network: {}", e))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("events {}: {}", status, truncate(&body, 240)));
    }
    let arr: Vec<serde_json::Value> = resp.json().await.map_err(|e| format!("JSON: {}", e))?;
    Ok(arr.into_iter().map(parse_event).collect())
}

/// All projects accessible to the auth token in the org.
pub async fn list_projects(creds: &SentryCredentials) -> Result<Vec<SentryProject>, String> {
    list_projects_inner(&http(), creds).await
}

async fn list_projects_inner(
    client: &reqwest::Client,
    creds: &SentryCredentials,
) -> Result<Vec<SentryProject>, String> {
    let url = format!(
        "{}/api/0/organizations/{}/projects/",
        creds.host, creds.organization_slug
    );
    let resp = client
        .get(&url)
        .bearer_auth(&creds.token)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| format!("network: {}", e))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("projects {}: {}", status, truncate(&body, 240)));
    }
    let arr: Vec<serde_json::Value> = resp.json().await.map_err(|e| format!("JSON: {}", e))?;
    let mut out: Vec<SentryProject> = arr
        .into_iter()
        .map(|v| SentryProject {
            id: v.get("id").and_then(|s| s.as_str()).unwrap_or("").to_string(),
            slug: v.get("slug").and_then(|s| s.as_str()).unwrap_or("").to_string(),
            name: v.get("name").and_then(|s| s.as_str()).unwrap_or("").to_string(),
            platform: v.get("platform").and_then(|s| s.as_str()).map(String::from),
            is_member: v.get("isMember").and_then(|b| b.as_bool()).unwrap_or(true),
        })
        .collect();
    // Member projects first, then alphabetical by slug — matches Sentry's UX.
    out.sort_by(|a, b| {
        b.is_member
            .cmp(&a.is_member)
            .then_with(|| a.slug.cmp(&b.slug))
    });
    Ok(out)
}

/// Environments declared on a single project. Sentry's environment filter
/// is org-wide in queries (`environment:prod`) but the dropdown sources
/// these per-project to keep the list short.
pub async fn list_environments(
    creds: &SentryCredentials,
    project_slug: &str,
) -> Result<Vec<SentryEnvironment>, String> {
    let url = format!(
        "{}/api/0/projects/{}/{}/environments/",
        creds.host, creds.organization_slug, project_slug
    );
    let resp = http()
        .get(&url)
        .bearer_auth(&creds.token)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| format!("network: {}", e))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("envs {}: {}", status, truncate(&body, 240)));
    }
    let arr: Vec<serde_json::Value> = resp.json().await.map_err(|e| format!("JSON: {}", e))?;
    Ok(arr
        .into_iter()
        .filter_map(|v| {
            let id = v.get("id").and_then(|s| s.as_str())?.to_string();
            let name = v.get("name").and_then(|s| s.as_str())?.to_string();
            if name.is_empty() {
                return None;
            }
            Some(SentryEnvironment { id, name })
        })
        .collect())
}

/// Detail-pane fetch — single event with full stacktrace + tags + breadcrumbs.
pub async fn get_event_detail(
    creds: &SentryCredentials,
    issue_id: &str,
    event_id: &str,
) -> Result<SentryEventDetail, String> {
    let resolved_issue = resolve_to_numeric_id(creds, issue_id).await?;
    let target = if event_id.is_empty() { "latest" } else { event_id };
    let url = format!("{}/api/0/issues/{}/events/{}/", creds.host, resolved_issue, target);
    let resp = http()
        .get(&url)
        .bearer_auth(&creds.token)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| format!("network: {}", e))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("event {}: {}", status, truncate(&body, 240)));
    }
    let v: serde_json::Value = resp.json().await.map_err(|e| format!("JSON: {}", e))?;
    Ok(parse_event_detail(v, &resolved_issue, &creds.host))
}

/// Resolve / unresolve / ignore an issue. `status` ∈ unresolved | resolved |
/// ignored. Sentry returns the updated issue payload — we just check status.
pub async fn set_issue_status(
    creds: &SentryCredentials,
    issue_id: &str,
    status: &str,
) -> Result<SentryIssue, String> {
    let resolved = resolve_to_numeric_id(creds, issue_id).await?;
    let body = serde_json::json!({ "status": status });
    let url = format!("{}/api/0/issues/{}/", creds.host, resolved);
    let resp = http()
        .put(&url)
        .bearer_auth(&creds.token)
        .header("Accept", "application/json")
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("network: {}", e))?;
    if !resp.status().is_success() {
        let s = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("update {}: {}", s, truncate(&body, 240)));
    }
    let v: serde_json::Value = resp.json().await.map_err(|e| format!("JSON: {}", e))?;
    Ok(parse_issue(v))
}

fn parse_event_detail(v: serde_json::Value, issue_id: &str, host: &str) -> SentryEventDetail {
    let event_id = v
        .get("eventID")
        .and_then(|s| s.as_str())
        .unwrap_or("")
        .to_string();
    let date_created = v
        .get("dateCreated")
        .and_then(|s| s.as_str())
        .unwrap_or("")
        .to_string();
    let platform = v.get("platform").and_then(|s| s.as_str()).map(String::from);
    let message = v.get("message").and_then(|s| s.as_str()).map(String::from);
    let culprit = v.get("culprit").and_then(|s| s.as_str()).map(String::from);

    // tags: [ { key, value }, … ] — flatten to (k,v) pairs for the UI.
    let tags = v
        .get("tags")
        .and_then(|t| t.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| {
                    let k = item.get("key").and_then(|s| s.as_str())?.to_string();
                    let val = item.get("value").and_then(|s| s.as_str())?.to_string();
                    Some((k, val))
                })
                .collect()
        })
        .unwrap_or_default();

    let user = v.get("user").cloned().unwrap_or(serde_json::Value::Null);
    let user_email = user.get("email").and_then(|s| s.as_str()).map(String::from);
    let user_id = user.get("id").and_then(|s| s.as_str()).map(String::from);
    let user_ip = user.get("ip_address").and_then(|s| s.as_str()).map(String::from);

    // Walk entries to pull exception + breadcrumbs.
    let mut exceptions: Vec<SentryException> = Vec::new();
    let mut breadcrumbs_summary: Option<String> = None;
    if let Some(entries) = v.get("entries").and_then(|e| e.as_array()) {
        for entry in entries {
            let typ = entry.get("type").and_then(|s| s.as_str()).unwrap_or("");
            let data = entry.get("data");
            match typ {
                "exception" => {
                    if let Some(values) = data.and_then(|d| d.get("values")).and_then(|v| v.as_array()) {
                        for val in values {
                            exceptions.push(parse_exception(val));
                        }
                    }
                }
                "breadcrumbs" => {
                    if let Some(values) = data.and_then(|d| d.get("values")).and_then(|v| v.as_array()) {
                        // Tail (most recent) — Sentry returns oldest first.
                        let tail: Vec<&serde_json::Value> = values.iter().rev().take(8).collect();
                        let lines: Vec<String> = tail
                            .into_iter()
                            .rev()
                            .map(|b| {
                                let cat = b.get("category").and_then(|s| s.as_str()).unwrap_or("");
                                let msg = b.get("message").and_then(|s| s.as_str()).unwrap_or("");
                                let level = b.get("level").and_then(|s| s.as_str()).unwrap_or("");
                                format!(
                                    "[{}] {}{}",
                                    if level.is_empty() { cat } else { level },
                                    if cat.is_empty() || level.is_empty() {
                                        String::new()
                                    } else {
                                        format!("{} · ", cat)
                                    },
                                    msg
                                )
                            })
                            .collect();
                        if !lines.is_empty() {
                            breadcrumbs_summary = Some(lines.join("\n"));
                        }
                    }
                }
                _ => {}
            }
        }
    }

    let permalink = if !issue_id.is_empty() && !event_id.is_empty() {
        Some(format!("{}/issues/{}/events/{}/", host, issue_id, event_id))
    } else {
        None
    };

    SentryEventDetail {
        event_id,
        date_created,
        platform,
        message,
        culprit,
        user_email,
        user_id,
        user_ip,
        release: v.get("release").and_then(|r| r.get("version")).and_then(|s| s.as_str()).map(String::from),
        environment: v.get("environment").and_then(|s| s.as_str()).map(String::from),
        tags,
        exceptions,
        breadcrumbs_summary,
        permalink,
    }
}

fn parse_exception(val: &serde_json::Value) -> SentryException {
    let frames = val
        .get("stacktrace")
        .and_then(|s| s.get("frames"))
        .and_then(|f| f.as_array())
        .map(|arr| arr.iter().map(parse_frame).collect())
        .unwrap_or_default();
    SentryException {
        r#type: val.get("type").and_then(|s| s.as_str()).map(String::from),
        value: val.get("value").and_then(|s| s.as_str()).map(String::from),
        module: val.get("module").and_then(|s| s.as_str()).map(String::from),
        frames,
    }
}

fn parse_frame(f: &serde_json::Value) -> SentryStackFrame {
    let context = f
        .get("context")
        .and_then(|c| c.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|pair| {
                    let pair = pair.as_array()?;
                    let line = pair.first()?.as_u64()?;
                    let source = pair.get(1)?.as_str()?.to_string();
                    Some(SentryContextLine { line, source })
                })
                .collect()
        })
        .unwrap_or_default();
    SentryStackFrame {
        function: f.get("function").and_then(|s| s.as_str()).map(String::from),
        filename: f.get("filename").and_then(|s| s.as_str()).map(String::from),
        abs_path: f.get("absPath").and_then(|s| s.as_str()).map(String::from),
        lineno: f.get("lineNo").and_then(|s| s.as_u64()),
        colno: f.get("colNo").and_then(|s| s.as_u64()),
        in_app: f.get("inApp").and_then(|b| b.as_bool()).unwrap_or(false),
        context,
    }
}

fn parse_issue(v: serde_json::Value) -> SentryIssue {
    let project = v.get("project").cloned().unwrap_or(serde_json::Value::Null);
    let metadata = v.get("metadata").cloned().unwrap_or(serde_json::Value::Null);
    SentryIssue {
        id: v.get("id").and_then(|s| s.as_str()).unwrap_or("").to_string(),
        short_id: v
            .get("shortId")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string(),
        title: v
            .get("title")
            .and_then(|s| s.as_str())
            .unwrap_or("(untitled)")
            .to_string(),
        culprit: v.get("culprit").and_then(|s| s.as_str()).map(String::from),
        level: v
            .get("level")
            .and_then(|s| s.as_str())
            .unwrap_or("error")
            .to_string(),
        status: v
            .get("status")
            .and_then(|s| s.as_str())
            .unwrap_or("unresolved")
            .to_string(),
        platform: v.get("platform").and_then(|s| s.as_str()).map(String::from),
        project_slug: project
            .get("slug")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string(),
        project_name: project
            .get("name")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string(),
        count: v
            .get("count")
            .and_then(|s| s.as_str().map(String::from).or_else(|| s.as_u64().map(|n| n.to_string())))
            .unwrap_or_default(),
        user_count: v.get("userCount").and_then(|s| s.as_u64()).unwrap_or(0),
        first_seen: v
            .get("firstSeen")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string(),
        last_seen: v
            .get("lastSeen")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string(),
        permalink: v
            .get("permalink")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string(),
        metadata_type: metadata.get("type").and_then(|s| s.as_str()).map(String::from),
        metadata_value: metadata.get("value").and_then(|s| s.as_str()).map(String::from),
    }
}

fn parse_event(v: serde_json::Value) -> SentryEvent {
    let exception_summary = v
        .get("entries")
        .and_then(|e| e.as_array())
        .and_then(|arr| arr.iter().find(|x| x.get("type").and_then(|t| t.as_str()) == Some("exception")))
        .and_then(|exc| {
            exc.get("data")
                .and_then(|d| d.get("values"))
                .and_then(|vals| vals.as_array())
                .and_then(|arr| arr.first())
                .map(|first| {
                    let typ = first.get("type").and_then(|s| s.as_str()).unwrap_or("");
                    let val = first.get("value").and_then(|s| s.as_str()).unwrap_or("");
                    if typ.is_empty() && val.is_empty() {
                        String::new()
                    } else if val.is_empty() {
                        typ.to_string()
                    } else {
                        format!("{}: {}", typ, val)
                    }
                })
        })
        .filter(|s| !s.is_empty());
    SentryEvent {
        event_id: v
            .get("eventID")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string(),
        date_created: v
            .get("dateCreated")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string(),
        message: v.get("message").and_then(|s| s.as_str()).map(String::from),
        platform: v.get("platform").and_then(|s| s.as_str()).map(String::from),
        exception_summary,
        permalink: None,
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max])
    }
}
