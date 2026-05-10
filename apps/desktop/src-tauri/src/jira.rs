//! Jira (Atlassian Cloud) API client.
//!
//! Auth: email + API Token via Basic Auth.
//! Token creation: https://id.atlassian.com/manage-profile/security/api-tokens

use serde::{Deserialize, Serialize};

const USER_AGENT: &str = concat!("Woom-Desktop/", env!("CARGO_PKG_VERSION"));

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct JiraCredentials {
    /// Cloud workspace, e.g. `acme.atlassian.net` (no scheme, no trailing slash).
    pub workspace: String,
    pub email: String,
    pub token: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct JiraUser {
    pub account_id: String,
    pub email_address: Option<String>,
    pub display_name: String,
    pub avatar_url: String,
    pub workspace: String,
}

#[derive(Debug, thiserror::Error)]
pub enum JiraError {
    #[error("invalid credentials")]
    InvalidCredentials,
    #[error("workspace not found")]
    WorkspaceNotFound,
    #[error("Jira returned {status}")]
    Api { status: u16 },
    #[error(transparent)]
    Reqwest(#[from] reqwest::Error),
}

#[derive(Debug, Deserialize)]
struct RawMyself {
    #[serde(rename = "accountId")]
    account_id: String,
    #[serde(default, rename = "emailAddress")]
    email_address: Option<String>,
    #[serde(rename = "displayName")]
    display_name: String,
    #[serde(rename = "avatarUrls")]
    avatar_urls: AvatarUrls,
}

#[derive(Debug, Deserialize)]
struct AvatarUrls {
    #[serde(rename = "48x48")]
    x48: String,
}

pub fn normalize_workspace(input: &str) -> String {
    input
        .trim()
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_end_matches('/')
        .to_string()
}

pub async fn fetch_myself(creds: &JiraCredentials) -> Result<JiraUser, JiraError> {
    let ws = normalize_workspace(&creds.workspace);
    if ws.is_empty() {
        return Err(JiraError::WorkspaceNotFound);
    }
    let client = reqwest::Client::builder().user_agent(USER_AGENT).timeout(std::time::Duration::from_secs(30)).build()?;
    let resp = client
        .get(format!("https://{ws}/rest/api/3/myself"))
        .basic_auth(&creds.email, Some(&creds.token))
        .header("Accept", "application/json")
        .send()
        .await?;

    let status = resp.status();
    if status.is_success() {
        let raw: RawMyself = resp.json().await?;
        return Ok(JiraUser {
            account_id: raw.account_id,
            email_address: raw.email_address,
            display_name: raw.display_name,
            avatar_url: raw.avatar_urls.x48,
            workspace: ws,
        });
    }
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Err(JiraError::InvalidCredentials);
    }
    if status == reqwest::StatusCode::NOT_FOUND {
        return Err(JiraError::WorkspaceNotFound);
    }
    Err(JiraError::Api { status: status.as_u16() })
}

// ---------- Issue listing ----------

#[derive(Debug, Serialize, Clone)]
pub struct JiraActor {
    pub display_name: String,
    pub avatar_url: Option<String>,
    pub account_id: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct JiraItem {
    pub id: String,
    pub key: String,
    pub summary: String,
    pub description: Option<String>,
    pub status: String,
    pub status_category: String,
    pub priority: Option<String>,
    pub issue_type: String,
    pub assignee: Option<JiraActor>,
    pub reporter: Option<JiraActor>,
    pub labels: Vec<String>,
    pub updated: String,
    pub created: String,
    pub url: String,
}

#[derive(Debug, Deserialize)]
struct RawSearchResp {
    issues: Vec<RawIssue>,
}

#[derive(Debug, Deserialize)]
struct RawIssue {
    id: String,
    key: String,
    fields: RawFields,
}

#[derive(Debug, Deserialize)]
struct RawFields {
    #[serde(default)]
    summary: String,
    #[serde(default)]
    description: Option<serde_json::Value>,
    status: RawStatus,
    priority: Option<RawPriority>,
    issuetype: RawIssueType,
    assignee: Option<RawJiraUser>,
    reporter: Option<RawJiraUser>,
    #[serde(default)]
    labels: Vec<String>,
    #[serde(default)]
    updated: String,
    #[serde(default)]
    created: String,
}

/// Walk an Atlassian Document Format (ADF) tree and extract a plain-text
/// approximation suitable for feeding to an LLM.
fn adf_to_text(value: &serde_json::Value) -> String {
    fn walk(v: &serde_json::Value, out: &mut String) {
        if let Some(arr) = v.as_array() {
            for item in arr {
                walk(item, out);
            }
            return;
        }
        if !v.is_object() {
            return;
        }
        let typ = v.get("type").and_then(|t| t.as_str()).unwrap_or("");
        match typ {
            "text" => {
                if let Some(t) = v.get("text").and_then(|t| t.as_str()) {
                    out.push_str(t);
                }
            }
            "hardBreak" => out.push('\n'),
            "paragraph" | "heading" => {
                if let Some(content) = v.get("content") {
                    walk(content, out);
                }
                out.push_str("\n\n");
            }
            "bulletList" | "orderedList" => {
                if let Some(content) = v.get("content").and_then(|c| c.as_array()) {
                    for (i, c) in content.iter().enumerate() {
                        let marker = if typ == "orderedList" {
                            format!("{}. ", i + 1)
                        } else {
                            "- ".to_string()
                        };
                        out.push_str(&marker);
                        walk(c, out);
                        out.push('\n');
                    }
                    out.push('\n');
                }
            }
            "listItem" | "blockquote" => {
                if let Some(content) = v.get("content") {
                    walk(content, out);
                }
            }
            "codeBlock" => {
                out.push_str("\n```\n");
                if let Some(content) = v.get("content") {
                    walk(content, out);
                }
                out.push_str("\n```\n");
            }
            "rule" => out.push_str("\n---\n"),
            _ => {
                if let Some(content) = v.get("content") {
                    walk(content, out);
                }
            }
        }
    }
    let mut out = String::new();
    walk(value, &mut out);
    out.trim().to_string()
}

#[derive(Debug, Deserialize)]
struct RawStatus {
    name: String,
    #[serde(default, rename = "statusCategory")]
    status_category: Option<RawStatusCategory>,
}

#[derive(Debug, Deserialize)]
struct RawStatusCategory {
    key: String,
}

#[derive(Debug, Deserialize)]
struct RawPriority {
    name: String,
}

#[derive(Debug, Deserialize)]
struct RawIssueType {
    name: String,
}

#[derive(Debug, Deserialize)]
struct RawJiraUser {
    #[serde(rename = "displayName")]
    display_name: String,
    #[serde(rename = "avatarUrls")]
    avatar_urls: Option<AvatarUrls>,
    #[serde(default, rename = "accountId")]
    account_id: Option<String>,
}

impl From<RawJiraUser> for JiraActor {
    fn from(u: RawJiraUser) -> Self {
        JiraActor {
            display_name: u.display_name,
            avatar_url: u.avatar_urls.map(|a| a.x48),
            account_id: u.account_id,
        }
    }
}

pub async fn list_my_issues(creds: &JiraCredentials) -> Result<Vec<JiraItem>, JiraError> {
    list_issues_for(creds, None).await
}

// ---------- Field write ops (assignee / priority / labels) ----------

pub async fn set_assignee(
    creds: &JiraCredentials,
    key: &str,
    account_id: Option<&str>,
) -> Result<(), JiraError> {
    let ws = normalize_workspace(&creds.workspace);
    let client = reqwest::Client::builder().user_agent(USER_AGENT).timeout(std::time::Duration::from_secs(30)).build()?;
    let body = serde_json::json!({ "accountId": account_id });
    let resp = client
        .put(format!("https://{ws}/rest/api/3/issue/{key}/assignee"))
        .basic_auth(&creds.email, Some(&creds.token))
        .header("Accept", "application/json")
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await?;
    let status = resp.status();
    if !status.is_success() {
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(JiraError::InvalidCredentials);
        }
        return Err(JiraError::Api { status: status.as_u16() });
    }
    Ok(())
}

pub async fn set_priority(
    creds: &JiraCredentials,
    key: &str,
    priority: &str,
) -> Result<(), JiraError> {
    let ws = normalize_workspace(&creds.workspace);
    let client = reqwest::Client::builder().user_agent(USER_AGENT).timeout(std::time::Duration::from_secs(30)).build()?;
    let body = serde_json::json!({
        "fields": { "priority": { "name": priority } }
    });
    let resp = client
        .put(format!("https://{ws}/rest/api/3/issue/{key}"))
        .basic_auth(&creds.email, Some(&creds.token))
        .header("Accept", "application/json")
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await?;
    let status = resp.status();
    if !status.is_success() {
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(JiraError::InvalidCredentials);
        }
        return Err(JiraError::Api { status: status.as_u16() });
    }
    Ok(())
}

pub async fn set_labels(
    creds: &JiraCredentials,
    key: &str,
    labels: Vec<String>,
) -> Result<(), JiraError> {
    let ws = normalize_workspace(&creds.workspace);
    let client = reqwest::Client::builder().user_agent(USER_AGENT).timeout(std::time::Duration::from_secs(30)).build()?;
    let body = serde_json::json!({ "fields": { "labels": labels } });
    let resp = client
        .put(format!("https://{ws}/rest/api/3/issue/{key}"))
        .basic_auth(&creds.email, Some(&creds.token))
        .header("Accept", "application/json")
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await?;
    let status = resp.status();
    if !status.is_success() {
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(JiraError::InvalidCredentials);
        }
        return Err(JiraError::Api { status: status.as_u16() });
    }
    Ok(())
}

#[derive(Debug, Serialize, Clone)]
pub struct JiraUserSummary {
    pub account_id: String,
    pub display_name: String,
    pub email_address: Option<String>,
    pub avatar_url: String,
    pub active: bool,
}

#[derive(Debug, Deserialize)]
struct RawUser {
    #[serde(rename = "accountId")]
    account_id: String,
    #[serde(rename = "displayName")]
    display_name: String,
    #[serde(default, rename = "emailAddress")]
    email_address: Option<String>,
    #[serde(rename = "avatarUrls")]
    avatar_urls: AvatarUrls,
    #[serde(default)]
    active: bool,
}

impl From<RawUser> for JiraUserSummary {
    fn from(u: RawUser) -> Self {
        JiraUserSummary {
            account_id: u.account_id,
            display_name: u.display_name,
            email_address: u.email_address,
            avatar_url: u.avatar_urls.x48,
            active: u.active,
        }
    }
}

pub async fn search_users(
    creds: &JiraCredentials,
    query: &str,
) -> Result<Vec<JiraUserSummary>, JiraError> {
    let ws = normalize_workspace(&creds.workspace);
    let client = reqwest::Client::builder().user_agent(USER_AGENT).timeout(std::time::Duration::from_secs(30)).build()?;
    let resp = client
        .get(format!("https://{ws}/rest/api/3/user/search"))
        .basic_auth(&creds.email, Some(&creds.token))
        .header("Accept", "application/json")
        .query(&[("query", query), ("maxResults", "30")])
        .send()
        .await?;
    let status = resp.status();
    if !status.is_success() {
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(JiraError::InvalidCredentials);
        }
        return Err(JiraError::Api { status: status.as_u16() });
    }
    let raw: Vec<RawUser> = resp.json().await?;
    Ok(raw.into_iter().filter(|u| u.active).map(JiraUserSummary::from).collect())
}

/// Return users assignable on issues of `project_key`. Powers the assignee
/// dropdown in the create-issue modal — Jira's `/user/search` returns
/// site-wide users (incl. ones who can't actually be assigned), so we hit
/// the project-scoped endpoint instead.
pub async fn list_assignable_users(
    creds: &JiraCredentials,
    project_key: &str,
) -> Result<Vec<JiraUserSummary>, JiraError> {
    let ws = normalize_workspace(&creds.workspace);
    let client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(std::time::Duration::from_secs(30))
        .build()?;
    let resp = client
        .get(format!("https://{ws}/rest/api/3/user/assignable/search"))
        .basic_auth(&creds.email, Some(&creds.token))
        .header("Accept", "application/json")
        .query(&[("project", project_key), ("maxResults", "200")])
        .send()
        .await?;
    let status = resp.status();
    if !status.is_success() {
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(JiraError::InvalidCredentials);
        }
        return Err(JiraError::Api { status: status.as_u16() });
    }
    let raw: Vec<RawUser> = resp.json().await?;
    Ok(raw.into_iter().filter(|u| u.active).map(JiraUserSummary::from).collect())
}

// ---------- Issue detail, comments, transitions ----------

#[derive(Debug, Serialize, Clone)]
pub struct JiraComment {
    pub id: String,
    pub author: Option<JiraActor>,
    pub body: String,
    pub created: String,
    pub updated: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct JiraTransition {
    pub id: String,
    pub name: String,
    /// Target status category key (e.g. "indeterminate", "done", "new").
    pub to_status: String,
    pub to_status_category: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct JiraDetail {
    pub id: String,
    pub key: String,
    pub summary: String,
    /// Plain-text version of the description (ADF flattened). Suitable for
    /// display/editing in a simple textarea.
    pub description: String,
    pub status: String,
    pub status_category: String,
    pub priority: Option<String>,
    pub issue_type: String,
    pub assignee: Option<JiraActor>,
    pub reporter: Option<JiraActor>,
    pub labels: Vec<String>,
    pub updated: String,
    pub created: String,
    pub url: String,
    pub comments: Vec<JiraComment>,
    pub transitions: Vec<JiraTransition>,
}

#[derive(Debug, Deserialize)]
struct RawIssueDetail {
    id: String,
    key: String,
    fields: RawFields,
    #[serde(default)]
    transitions: Vec<RawTransition>,
}

#[derive(Debug, Deserialize)]
struct RawTransition {
    id: String,
    name: String,
    #[serde(default)]
    to: Option<RawTransitionTo>,
}

#[derive(Debug, Deserialize)]
struct RawTransitionTo {
    #[serde(default)]
    name: String,
    #[serde(default, rename = "statusCategory")]
    status_category: Option<RawStatusCategory>,
}

#[derive(Debug, Deserialize)]
struct RawCommentList {
    #[serde(default)]
    comments: Vec<RawComment>,
}

#[derive(Debug, Deserialize)]
struct RawComment {
    id: String,
    author: Option<RawJiraUser>,
    body: Option<serde_json::Value>, // ADF
    #[serde(default)]
    created: String,
    #[serde(default)]
    updated: String,
}

pub async fn get_issue_detail(
    creds: &JiraCredentials,
    key: &str,
) -> Result<JiraDetail, JiraError> {
    let ws = normalize_workspace(&creds.workspace);
    let client = reqwest::Client::builder().user_agent(USER_AGENT).timeout(std::time::Duration::from_secs(30)).build()?;

    // Pull issue with transitions in one call.
    let issue_url = format!(
        "https://{ws}/rest/api/3/issue/{key}?expand=transitions&fields=summary,description,status,priority,issuetype,assignee,reporter,labels,updated,created"
    );
    let resp = client
        .get(&issue_url)
        .basic_auth(&creds.email, Some(&creds.token))
        .header("Accept", "application/json")
        .send()
        .await?;
    let status = resp.status();
    if !status.is_success() {
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(JiraError::InvalidCredentials);
        }
        return Err(JiraError::Api { status: status.as_u16() });
    }
    let raw: RawIssueDetail = resp.json().await?;

    // Comments (separate endpoint for cleaner pagination).
    let cmt_url = format!("https://{ws}/rest/api/3/issue/{key}/comment?orderBy=created&maxResults=100");
    let cmt_resp = client
        .get(&cmt_url)
        .basic_auth(&creds.email, Some(&creds.token))
        .header("Accept", "application/json")
        .send()
        .await?;
    let comments_list: RawCommentList = if cmt_resp.status().is_success() {
        cmt_resp.json().await.unwrap_or(RawCommentList { comments: vec![] })
    } else {
        RawCommentList { comments: vec![] }
    };

    let status_category = raw
        .fields
        .status
        .status_category
        .as_ref()
        .map(|s| s.key.clone())
        .unwrap_or_else(|| "new".into());
    let description = raw
        .fields
        .description
        .as_ref()
        .map(adf_to_text)
        .unwrap_or_default();

    let transitions = raw
        .transitions
        .into_iter()
        .map(|t| {
            let (to_status, to_cat) = match t.to {
                Some(to) => (
                    to.name,
                    to.status_category.map(|c| c.key).unwrap_or_default(),
                ),
                None => (String::new(), String::new()),
            };
            JiraTransition {
                id: t.id,
                name: t.name,
                to_status,
                to_status_category: to_cat,
            }
        })
        .collect();

    let comments = comments_list
        .comments
        .into_iter()
        .map(|c| JiraComment {
            id: c.id,
            author: c.author.map(JiraActor::from),
            body: c.body.as_ref().map(adf_to_text).unwrap_or_default(),
            created: c.created,
            updated: c.updated,
        })
        .collect();

    Ok(JiraDetail {
        id: raw.id,
        key: raw.key.clone(),
        summary: raw.fields.summary,
        description,
        status: raw.fields.status.name,
        status_category,
        priority: raw.fields.priority.map(|p| p.name),
        issue_type: raw.fields.issuetype.name,
        assignee: raw.fields.assignee.map(JiraActor::from),
        reporter: raw.fields.reporter.map(JiraActor::from),
        labels: raw.fields.labels,
        updated: raw.fields.updated,
        created: raw.fields.created,
        url: format!("https://{ws}/browse/{}", raw.key),
        comments,
        transitions,
    })
}

/// Build a minimal ADF document from plain text. Splits on blank lines into
/// separate paragraphs; single newlines become `hardBreak` nodes.
fn text_to_adf(text: &str) -> serde_json::Value {
    if text.trim().is_empty() {
        return serde_json::json!({ "type": "doc", "version": 1, "content": [] });
    }
    let mut paragraphs: Vec<serde_json::Value> = Vec::new();
    for block in text.split("\n\n") {
        let mut content: Vec<serde_json::Value> = Vec::new();
        let lines: Vec<&str> = block.split('\n').collect();
        for (i, line) in lines.iter().enumerate() {
            if !line.is_empty() {
                content.push(serde_json::json!({ "type": "text", "text": line }));
            }
            if i + 1 < lines.len() {
                content.push(serde_json::json!({ "type": "hardBreak" }));
            }
        }
        if content.is_empty() {
            continue;
        }
        paragraphs.push(serde_json::json!({
            "type": "paragraph",
            "content": content,
        }));
    }
    serde_json::json!({ "type": "doc", "version": 1, "content": paragraphs })
}

pub async fn update_issue(
    creds: &JiraCredentials,
    key: &str,
    summary: Option<&str>,
    description: Option<&str>,
) -> Result<(), JiraError> {
    if summary.is_none() && description.is_none() {
        return Ok(());
    }
    let ws = normalize_workspace(&creds.workspace);
    let client = reqwest::Client::builder().user_agent(USER_AGENT).timeout(std::time::Duration::from_secs(30)).build()?;

    let mut fields = serde_json::Map::new();
    if let Some(s) = summary {
        fields.insert("summary".into(), serde_json::json!(s));
    }
    if let Some(d) = description {
        fields.insert("description".into(), text_to_adf(d));
    }
    let body = serde_json::json!({ "fields": fields });

    let resp = client
        .put(format!("https://{ws}/rest/api/3/issue/{key}"))
        .basic_auth(&creds.email, Some(&creds.token))
        .header("Accept", "application/json")
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await?;
    let status = resp.status();
    if !status.is_success() {
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(JiraError::InvalidCredentials);
        }
        return Err(JiraError::Api { status: status.as_u16() });
    }
    Ok(())
}

pub async fn transition_issue(
    creds: &JiraCredentials,
    key: &str,
    transition_id: &str,
) -> Result<(), JiraError> {
    let ws = normalize_workspace(&creds.workspace);
    let client = reqwest::Client::builder().user_agent(USER_AGENT).timeout(std::time::Duration::from_secs(30)).build()?;
    let body = serde_json::json!({ "transition": { "id": transition_id } });
    let resp = client
        .post(format!("https://{ws}/rest/api/3/issue/{key}/transitions"))
        .basic_auth(&creds.email, Some(&creds.token))
        .header("Accept", "application/json")
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await?;
    let status = resp.status();
    if !status.is_success() {
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(JiraError::InvalidCredentials);
        }
        return Err(JiraError::Api { status: status.as_u16() });
    }
    Ok(())
}

pub async fn add_comment(
    creds: &JiraCredentials,
    key: &str,
    body: &str,
) -> Result<JiraComment, JiraError> {
    let ws = normalize_workspace(&creds.workspace);
    let client = reqwest::Client::builder().user_agent(USER_AGENT).timeout(std::time::Duration::from_secs(30)).build()?;
    let payload = serde_json::json!({ "body": text_to_adf(body) });
    let resp = client
        .post(format!("https://{ws}/rest/api/3/issue/{key}/comment"))
        .basic_auth(&creds.email, Some(&creds.token))
        .header("Accept", "application/json")
        .header("Content-Type", "application/json")
        .json(&payload)
        .send()
        .await?;
    let status = resp.status();
    if !status.is_success() {
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(JiraError::InvalidCredentials);
        }
        return Err(JiraError::Api { status: status.as_u16() });
    }
    let raw: RawComment = resp.json().await?;
    Ok(JiraComment {
        id: raw.id,
        author: raw.author.map(JiraActor::from),
        body: raw.body.as_ref().map(adf_to_text).unwrap_or_default(),
        created: raw.created,
        updated: raw.updated,
    })
}

// ---------- Worklogs (native Jira time tracking) ----------

#[derive(Debug, Serialize, Clone)]
pub struct JiraWorklog {
    pub id: String,
    pub author: Option<JiraActor>,
    pub comment: String,
    pub created: String,
    pub updated: String,
    /// When the work started (ISO-8601 with offset, as returned by Jira).
    pub started: String,
    pub time_spent_seconds: i64,
    /// Human label Jira computed, e.g. `"1h 30m"`. Shown verbatim so the
    /// app's label matches what the user sees in the Jira UI.
    pub time_spent: String,
}

#[derive(Debug, Deserialize)]
struct RawWorklogList {
    #[serde(default)]
    worklogs: Vec<RawWorklog>,
}

#[derive(Debug, Deserialize)]
struct RawWorklog {
    id: String,
    author: Option<RawJiraUser>,
    #[serde(default)]
    comment: Option<serde_json::Value>,
    #[serde(default)]
    created: String,
    #[serde(default)]
    updated: String,
    #[serde(default)]
    started: String,
    #[serde(default, rename = "timeSpentSeconds")]
    time_spent_seconds: i64,
    #[serde(default, rename = "timeSpent")]
    time_spent: String,
}

impl From<RawWorklog> for JiraWorklog {
    fn from(w: RawWorklog) -> Self {
        JiraWorklog {
            id: w.id,
            author: w.author.map(JiraActor::from),
            comment: w.comment.as_ref().map(adf_to_text).unwrap_or_default(),
            created: w.created,
            updated: w.updated,
            started: w.started,
            time_spent_seconds: w.time_spent_seconds,
            time_spent: w.time_spent,
        }
    }
}

pub async fn list_worklogs(
    creds: &JiraCredentials,
    key: &str,
) -> Result<Vec<JiraWorklog>, JiraError> {
    let ws = normalize_workspace(&creds.workspace);
    let client = reqwest::Client::builder().user_agent(USER_AGENT).timeout(std::time::Duration::from_secs(30)).build()?;
    let resp = client
        .get(format!(
            "https://{ws}/rest/api/3/issue/{key}/worklog?maxResults=100"
        ))
        .basic_auth(&creds.email, Some(&creds.token))
        .header("Accept", "application/json")
        .send()
        .await?;
    let status = resp.status();
    if !status.is_success() {
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(JiraError::InvalidCredentials);
        }
        return Err(JiraError::Api { status: status.as_u16() });
    }
    let list: RawWorklogList = resp.json().await?;
    Ok(list.worklogs.into_iter().map(JiraWorklog::from).collect())
}

pub async fn add_worklog(
    creds: &JiraCredentials,
    key: &str,
    time_spent_seconds: i64,
    started: Option<&str>,
    comment: Option<&str>,
) -> Result<JiraWorklog, JiraError> {
    let ws = normalize_workspace(&creds.workspace);
    let client = reqwest::Client::builder().user_agent(USER_AGENT).timeout(std::time::Duration::from_secs(30)).build()?;
    let mut payload = serde_json::json!({
        "timeSpentSeconds": time_spent_seconds,
    });
    if let Some(s) = started {
        payload["started"] = serde_json::Value::String(s.to_string());
    }
    if let Some(body) = comment {
        if !body.trim().is_empty() {
            payload["comment"] = text_to_adf(body);
        }
    }
    let resp = client
        .post(format!("https://{ws}/rest/api/3/issue/{key}/worklog"))
        .basic_auth(&creds.email, Some(&creds.token))
        .header("Accept", "application/json")
        .header("Content-Type", "application/json")
        .json(&payload)
        .send()
        .await?;
    let status = resp.status();
    if !status.is_success() {
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(JiraError::InvalidCredentials);
        }
        return Err(JiraError::Api { status: status.as_u16() });
    }
    let raw: RawWorklog = resp.json().await?;
    Ok(JiraWorklog::from(raw))
}

pub async fn delete_worklog(
    creds: &JiraCredentials,
    key: &str,
    worklog_id: &str,
) -> Result<(), JiraError> {
    let ws = normalize_workspace(&creds.workspace);
    let client = reqwest::Client::builder().user_agent(USER_AGENT).timeout(std::time::Duration::from_secs(30)).build()?;
    let resp = client
        .delete(format!(
            "https://{ws}/rest/api/3/issue/{key}/worklog/{worklog_id}"
        ))
        .basic_auth(&creds.email, Some(&creds.token))
        .send()
        .await?;
    let status = resp.status();
    if !status.is_success() {
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(JiraError::InvalidCredentials);
        }
        return Err(JiraError::Api { status: status.as_u16() });
    }
    Ok(())
}

pub async fn list_issues_for(
    creds: &JiraCredentials,
    assignee_account_id: Option<&str>,
) -> Result<Vec<JiraItem>, JiraError> {
    let jql = match assignee_account_id {
        Some(id) => format!(
            "assignee = \"{}\" AND resolution = Unresolved ORDER BY updated DESC",
            id.replace('"', "")
        ),
        None => {
            "assignee = currentUser() AND resolution = Unresolved ORDER BY updated DESC".to_string()
        }
    };
    search_issues(creds, &jql).await
}

/// Run an arbitrary JQL search. Returns up to 50 issues.
pub async fn search_issues(
    creds: &JiraCredentials,
    jql: &str,
) -> Result<Vec<JiraItem>, JiraError> {
    let ws = normalize_workspace(&creds.workspace);
    let client = reqwest::Client::builder().user_agent(USER_AGENT).timeout(std::time::Duration::from_secs(30)).build()?;
    let body = serde_json::json!({
        "jql": jql,
        "fields": [
            "summary", "description", "status", "priority", "issuetype",
            "assignee", "reporter", "labels", "updated", "created"
        ],
        "maxResults": 50
    });
    let resp = client
        .post(format!("https://{ws}/rest/api/3/search/jql"))
        .basic_auth(&creds.email, Some(&creds.token))
        .header("Accept", "application/json")
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await?;

    let status = resp.status();
    if !status.is_success() {
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(JiraError::InvalidCredentials);
        }
        return Err(JiraError::Api { status: status.as_u16() });
    }
    let parsed: RawSearchResp = resp.json().await?;
    let items: Vec<JiraItem> = parsed
        .issues
        .into_iter()
        .map(|raw| {
            let status_category = raw
                .fields
                .status
                .status_category
                .as_ref()
                .map(|s| s.key.clone())
                .unwrap_or_else(|| "new".into());
            let url = format!("https://{ws}/browse/{}", raw.key);
            let description = raw
                .fields
                .description
                .as_ref()
                .map(adf_to_text)
                .filter(|s| !s.is_empty());
            JiraItem {
                id: raw.id,
                key: raw.key,
                summary: raw.fields.summary,
                description,
                status: raw.fields.status.name,
                status_category,
                priority: raw.fields.priority.map(|p| p.name),
                issue_type: raw.fields.issuetype.name,
                assignee: raw.fields.assignee.map(JiraActor::from),
                reporter: raw.fields.reporter.map(JiraActor::from),
                labels: raw.fields.labels,
                updated: raw.fields.updated,
                created: raw.fields.created,
                url,
            }
        })
        .collect();
    Ok(items)
}

// ---------- Workflow statuses (for the inbox status filter) ----------

#[derive(Debug, Serialize, Clone)]
pub struct JiraStatus {
    pub id: String,
    pub name: String,
    /// One of `new`, `indeterminate`, `done`, `undefined`.
    pub category_key: String,
    /// Palette hint — taken from `statusCategory.colorName` when available,
    /// otherwise mapped from `category_key`. Values are low-level CSS color
    /// names as Jira uses them (`blue-gray`, `yellow`, `green`, `medium-gray`).
    pub color: String,
}

#[derive(Debug, Deserialize)]
struct RawStatusFull {
    id: String,
    name: String,
    #[serde(default, rename = "statusCategory")]
    status_category: Option<RawStatusCategoryFull>,
}

#[derive(Debug, Deserialize)]
struct RawStatusCategoryFull {
    #[serde(default)]
    key: String,
    #[serde(default, rename = "colorName")]
    color_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawProjectStatusesEntry {
    #[serde(default)]
    statuses: Vec<RawStatusFull>,
}

/// Map a category key to the palette name Jira would use if it didn't report
/// one. Mirrors what the Jira UI does for "new / indeterminate / done /
/// undefined".
fn category_color(cat: &str) -> &'static str {
    match cat {
        "new" => "blue-gray",
        "indeterminate" => "yellow",
        "done" => "green",
        _ => "medium-gray",
    }
}

fn status_from_raw(raw: RawStatusFull) -> JiraStatus {
    let (cat, color_from_api) = match raw.status_category {
        Some(c) => (c.key, c.color_name),
        None => (String::new(), None),
    };
    let category_key = if cat.is_empty() { "undefined".to_string() } else { cat };
    let color = color_from_api
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| category_color(&category_key).to_string());
    JiraStatus {
        id: raw.id,
        name: raw.name,
        category_key,
        color,
    }
}

/// List workflow statuses. When `project_key` is provided, flatten the
/// per-issue-type status lists into a deduped-by-name set (the shape the
/// inbox filter needs). Falls back to the global status list when the arg
/// is `None`.
pub async fn list_statuses(
    creds: &JiraCredentials,
    project_key: Option<&str>,
) -> Result<Vec<JiraStatus>, JiraError> {
    let ws = normalize_workspace(&creds.workspace);
    let client = reqwest::Client::builder().user_agent(USER_AGENT).timeout(std::time::Duration::from_secs(30)).build()?;

    if let Some(key) = project_key {
        let url = format!("https://{ws}/rest/api/3/project/{key}/statuses");
        let resp = client
            .get(&url)
            .basic_auth(&creds.email, Some(&creds.token))
            .header("Accept", "application/json")
            .send()
            .await?;
        let status = resp.status();
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(JiraError::InvalidCredentials);
        }
        if !status.is_success() {
            return Ok(vec![]);
        }
        let entries: Vec<RawProjectStatusesEntry> = resp.json().await.unwrap_or_default();
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut out: Vec<JiraStatus> = Vec::new();
        for entry in entries {
            for raw in entry.statuses {
                let s = status_from_raw(raw);
                if seen.insert(s.name.to_lowercase()) {
                    out.push(s);
                }
            }
        }
        return Ok(out);
    }

    let url = format!("https://{ws}/rest/api/3/status");
    let resp = client
        .get(&url)
        .basic_auth(&creds.email, Some(&creds.token))
        .header("Accept", "application/json")
        .send()
        .await?;
    let status = resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return Err(JiraError::InvalidCredentials);
    }
    if !status.is_success() {
        return Ok(vec![]);
    }
    let raw: Vec<RawStatusFull> = resp.json().await.unwrap_or_default();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut out: Vec<JiraStatus> = Vec::new();
    for r in raw {
        let s = status_from_raw(r);
        if seen.insert(s.name.to_lowercase()) {
            out.push(s);
        }
    }
    Ok(out)
}

// ---------- Projects / boards / sprints (for filter dropdowns) ----------

#[derive(Debug, Serialize, Clone)]
pub struct JiraProject {
    pub id: String,
    pub key: String,
    pub name: String,
    pub avatar_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawProjectList {
    #[serde(default)]
    values: Vec<RawProject>,
}

#[derive(Debug, Deserialize)]
struct RawProject {
    id: String,
    key: String,
    name: String,
    #[serde(default, rename = "avatarUrls")]
    avatar_urls: Option<AvatarUrls>,
}

pub async fn list_projects(creds: &JiraCredentials) -> Result<Vec<JiraProject>, JiraError> {
    let ws = normalize_workspace(&creds.workspace);
    let client = reqwest::Client::builder().user_agent(USER_AGENT).timeout(std::time::Duration::from_secs(30)).build()?;
    let resp = client
        .get(format!("https://{ws}/rest/api/3/project/search"))
        .basic_auth(&creds.email, Some(&creds.token))
        .header("Accept", "application/json")
        .query(&[("maxResults", "100"), ("orderBy", "name")])
        .send()
        .await?;
    let status = resp.status();
    if !status.is_success() {
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(JiraError::InvalidCredentials);
        }
        return Err(JiraError::Api { status: status.as_u16() });
    }
    let parsed: RawProjectList = resp.json().await?;
    Ok(parsed
        .values
        .into_iter()
        .map(|p| JiraProject {
            id: p.id,
            key: p.key,
            name: p.name,
            avatar_url: p.avatar_urls.map(|a| a.x48),
        })
        .collect())
}

#[derive(Debug, Serialize, Clone)]
pub struct JiraBoard {
    pub id: u64,
    pub name: String,
    /// "scrum" | "kanban" | "simple"
    pub type_: String,
    pub project_key: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawBoardList {
    #[serde(default)]
    values: Vec<RawBoard>,
}

#[derive(Debug, Deserialize)]
struct RawBoard {
    id: u64,
    name: String,
    #[serde(rename = "type")]
    type_: String,
    #[serde(default)]
    location: Option<RawBoardLocation>,
}

#[derive(Debug, Deserialize)]
struct RawBoardLocation {
    #[serde(default, rename = "projectKey")]
    project_key: Option<String>,
}

/// List Jira Agile boards. Scopes to a project if `project_key` is provided.
/// Returns empty Vec gracefully if the workspace lacks Agile access (Free tier
/// or permission errors on `/rest/agile/*`).
pub async fn list_boards(
    creds: &JiraCredentials,
    project_key: Option<&str>,
) -> Result<Vec<JiraBoard>, JiraError> {
    let ws = normalize_workspace(&creds.workspace);
    let client = reqwest::Client::builder().user_agent(USER_AGENT).timeout(std::time::Duration::from_secs(30)).build()?;
    let mut req = client
        .get(format!("https://{ws}/rest/agile/1.0/board"))
        .basic_auth(&creds.email, Some(&creds.token))
        .header("Accept", "application/json")
        .query(&[("maxResults", "50")]);
    if let Some(k) = project_key {
        req = req.query(&[("projectKeyOrId", k)]);
    }
    let resp = req.send().await?;
    let status = resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return Err(JiraError::InvalidCredentials);
    }
    // Gracefully handle Free-tier workspaces that don't expose Agile.
    if !status.is_success() {
        return Ok(vec![]);
    }
    let parsed: RawBoardList = resp.json().await.unwrap_or(RawBoardList { values: vec![] });
    Ok(parsed
        .values
        .into_iter()
        .map(|b| JiraBoard {
            id: b.id,
            name: b.name,
            type_: b.type_,
            project_key: b.location.and_then(|l| l.project_key),
        })
        .collect())
}

#[derive(Debug, Serialize, Clone)]
pub struct JiraSprint {
    pub id: u64,
    pub name: String,
    /// "active" | "closed" | "future"
    pub state: String,
    pub board_id: u64,
}

#[derive(Debug, Deserialize)]
struct RawSprintList {
    #[serde(default)]
    values: Vec<RawSprint>,
}

#[derive(Debug, Deserialize)]
struct RawSprint {
    id: u64,
    name: String,
    #[serde(default)]
    state: Option<String>,
    #[serde(default, rename = "originBoardId")]
    origin_board_id: Option<u64>,
}

/// List sprints attached to a board. Returns empty Vec if the board is Kanban
/// (no sprints) or the workspace lacks Agile access.
pub async fn list_sprints(
    creds: &JiraCredentials,
    board_id: u64,
) -> Result<Vec<JiraSprint>, JiraError> {
    let ws = normalize_workspace(&creds.workspace);
    let client = reqwest::Client::builder().user_agent(USER_AGENT).timeout(std::time::Duration::from_secs(30)).build()?;
    let resp = client
        .get(format!(
            "https://{ws}/rest/agile/1.0/board/{board_id}/sprint"
        ))
        .basic_auth(&creds.email, Some(&creds.token))
        .header("Accept", "application/json")
        .query(&[("maxResults", "50"), ("state", "active,future")])
        .send()
        .await?;
    let status = resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return Err(JiraError::InvalidCredentials);
    }
    if !status.is_success() {
        // 400/404 = Kanban board / no sprint support — treat as "no sprints".
        return Ok(vec![]);
    }
    let parsed: RawSprintList = resp.json().await.unwrap_or(RawSprintList { values: vec![] });
    Ok(parsed
        .values
        .into_iter()
        .map(|s| JiraSprint {
            id: s.id,
            name: s.name,
            state: s.state.unwrap_or_else(|| "future".into()),
            board_id: s.origin_board_id.unwrap_or(board_id),
        })
        .collect())
}

// ---------- Issue creation ----------

#[derive(Debug, Serialize, Clone)]
pub struct JiraIssueType {
    pub id: String,
    pub name: String,
    /// `true` for sub-tasks — the UI filters these out since the modal has no
    /// parent-picker yet.
    pub subtask: bool,
    pub icon_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawIssueTypesEnvelope {
    #[serde(default)]
    issue_types: Vec<RawIssueTypeFull>,
}

#[derive(Debug, Deserialize)]
struct RawIssueTypeFull {
    id: String,
    name: String,
    #[serde(default)]
    subtask: bool,
    #[serde(default, rename = "iconUrl")]
    icon_url: Option<String>,
}

/// List issue types available for a project via `/rest/api/3/issue/createmeta/{key}/issuetypes`.
/// Returns an empty list if the endpoint is unavailable — the UI falls back to
/// a hardcoded `Task / Bug / Story` list in that case.
pub async fn list_issue_types(
    creds: &JiraCredentials,
    project_key: &str,
) -> Result<Vec<JiraIssueType>, JiraError> {
    let ws = normalize_workspace(&creds.workspace);
    let client = reqwest::Client::builder().user_agent(USER_AGENT).timeout(std::time::Duration::from_secs(30)).build()?;
    let resp = client
        .get(format!(
            "https://{ws}/rest/api/3/issue/createmeta/{project_key}/issuetypes"
        ))
        .basic_auth(&creds.email, Some(&creds.token))
        .header("Accept", "application/json")
        .send()
        .await?;
    let status = resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return Err(JiraError::InvalidCredentials);
    }
    if !status.is_success() {
        return Ok(vec![]);
    }
    let parsed: RawIssueTypesEnvelope = resp
        .json()
        .await
        .unwrap_or(RawIssueTypesEnvelope { issue_types: vec![] });
    Ok(parsed
        .issue_types
        .into_iter()
        .filter(|t| !t.subtask)
        .map(|t| JiraIssueType {
            id: t.id,
            name: t.name,
            subtask: t.subtask,
            icon_url: t.icon_url,
        })
        .collect())
}

#[derive(Debug, Deserialize)]
struct CreatedIssueResp {
    id: String,
    key: String,
}

/// Create a Jira issue. Returns the newly-created item shaped like the rest
/// of the inbox. If `sprint_id` is provided, a secondary best-effort call into
/// the Agile API moves the issue into that sprint (silently skipped on error
/// for Free-tier / non-Agile workspaces).
pub async fn create_issue(
    creds: &JiraCredentials,
    project_key: &str,
    issue_type: &str,
    summary: &str,
    description_md: &str,
    assignee_account_id: Option<&str>,
    sprint_id: Option<u64>,
) -> Result<JiraItem, JiraError> {
    let ws = normalize_workspace(&creds.workspace);
    let client = reqwest::Client::builder().user_agent(USER_AGENT).timeout(std::time::Duration::from_secs(30)).build()?;

    let mut fields = serde_json::Map::new();
    fields.insert(
        "project".into(),
        serde_json::json!({ "key": project_key }),
    );
    fields.insert(
        "issuetype".into(),
        serde_json::json!({ "name": issue_type }),
    );
    fields.insert("summary".into(), serde_json::json!(summary));
    if !description_md.trim().is_empty() {
        fields.insert("description".into(), text_to_adf(description_md));
    }
    if let Some(a) = assignee_account_id {
        if !a.is_empty() {
            fields.insert(
                "assignee".into(),
                serde_json::json!({ "accountId": a }),
            );
        }
    }
    // Best-effort attempt to set the sprint inline via the usual
    // `customfield_10020` (common default on Atlassian Cloud). The actual
    // custom field varies per workspace; if it's wrong the request still
    // succeeds and we fall back to the Agile `sprint/{id}/issue` call below.
    let sprint_field = sprint_id;
    let body = serde_json::json!({ "fields": fields });

    let resp = client
        .post(format!("https://{ws}/rest/api/3/issue"))
        .basic_auth(&creds.email, Some(&creds.token))
        .header("Accept", "application/json")
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await?;
    let status = resp.status();
    if !status.is_success() {
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(JiraError::InvalidCredentials);
        }
        return Err(JiraError::Api { status: status.as_u16() });
    }
    let created: CreatedIssueResp = resp.json().await?;

    // If requested, move into a sprint via the Agile API. Ignore all failures —
    // user can re-assign manually if workspace lacks Agile or the sprint is
    // closed.
    if let Some(sid) = sprint_field {
        let _ = client
            .post(format!(
                "https://{ws}/rest/agile/1.0/sprint/{sid}/issue"
            ))
            .basic_auth(&creds.email, Some(&creds.token))
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .json(&serde_json::json!({ "issues": [created.key.clone()] }))
            .send()
            .await;
    }

    // Re-fetch the issue to return a fully-populated `JiraItem` shape that the
    // frontend can drop straight into the inbox list.
    let detail_url = format!(
        "https://{ws}/rest/api/3/issue/{}?fields=summary,description,status,priority,issuetype,assignee,reporter,labels,updated,created",
        created.key
    );
    let detail_resp = client
        .get(&detail_url)
        .basic_auth(&creds.email, Some(&creds.token))
        .header("Accept", "application/json")
        .send()
        .await?;
    if !detail_resp.status().is_success() {
        // Creation succeeded — synthesize a minimal item rather than error out.
        return Ok(JiraItem {
            id: created.id,
            key: created.key.clone(),
            summary: summary.to_string(),
            description: if description_md.trim().is_empty() {
                None
            } else {
                Some(description_md.to_string())
            },
            status: "To Do".into(),
            status_category: "new".into(),
            priority: None,
            issue_type: issue_type.to_string(),
            assignee: None,
            reporter: None,
            labels: vec![],
            updated: String::new(),
            created: String::new(),
            url: format!("https://{ws}/browse/{}", created.key),
        });
    }
    let raw: RawIssue = detail_resp.json().await?;
    let status_category = raw
        .fields
        .status
        .status_category
        .as_ref()
        .map(|s| s.key.clone())
        .unwrap_or_else(|| "new".into());
    let description = raw
        .fields
        .description
        .as_ref()
        .map(adf_to_text)
        .filter(|s| !s.is_empty());
    Ok(JiraItem {
        id: raw.id,
        key: raw.key.clone(),
        summary: raw.fields.summary,
        description,
        status: raw.fields.status.name,
        status_category,
        priority: raw.fields.priority.map(|p| p.name),
        issue_type: raw.fields.issuetype.name,
        assignee: raw.fields.assignee.map(JiraActor::from),
        reporter: raw.fields.reporter.map(JiraActor::from),
        labels: raw.fields.labels,
        updated: raw.fields.updated,
        created: raw.fields.created,
        url: format!("https://{ws}/browse/{}", raw.key),
    })
}
