//! Agent dispatcher — routes Forgehold chat sessions to either the Claude Code
//! or Cursor Agent CLI. Both adapters (`claude.rs`, `cursor.rs`) emit the
//! same Claude-style stream-json events to the frontend so the UI never
//! branches on agent kind.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::claude::{self, Runners, WarmPool};
use crate::cursor;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AgentKind {
    #[default]
    Claude,
    Cursor,
}

#[derive(Debug, Serialize, Clone)]
pub struct AgentStatus {
    pub claude: claude::ClaudeStatus,
    pub cursor: cursor::CursorStatus,
}

pub fn detect_all() -> AgentStatus {
    AgentStatus {
        claude: claude::detect(),
        cursor: cursor::detect(),
    }
}

/// Outcome of one turn through either CLI. `session_uuid` is what the caller
/// should persist and pass back on the next turn with `resume=true`. For
/// Claude it mirrors what we sent in; for Cursor it's the `chat_id` the CLI
/// minted via `create-chat` when we had no prior id to resume.
#[derive(Debug, Serialize, Clone)]
pub struct AgentAskResult {
    pub reply: String,
    pub session_uuid: String,
}

#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    #[error(transparent)]
    Claude(#[from] claude::ClaudeRunError),
    #[error(transparent)]
    Cursor(#[from] cursor::CursorRunError),
}

impl AgentError {
    /// True when the underlying CLI told us the resume target uuid is gone
    /// from its on-disk store. The frontend uses this to self-heal: rotate
    /// the session uuid, clear the resumable flag, stamp a recap of the
    /// in-memory chat history into the next system prompt, and retry once.
    /// Both adapters surface this through their own error variants — this
    /// helper unifies the check so the frontend doesn't have to branch on
    /// agent kind.
    pub fn is_resume_orphan(&self) -> bool {
        match self {
            AgentError::Claude(claude::ClaudeRunError::ResumeOrphan(_)) => true,
            AgentError::Cursor(cursor::CursorRunError::ResumeOrphan(_)) => true,
            _ => false,
        }
    }
}

/// Kind-dispatched one-off commit-message generator. Both adapters ship
/// their own headless path that takes the staged diff and returns a
/// one-liner — this just picks the right one for the agent the user has
/// linked to the editor.
pub async fn generate_commit_message(
    kind: AgentKind,
    repo: &std::path::Path,
) -> Result<String, AgentError> {
    match kind {
        AgentKind::Claude => {
            crate::claude::generate_commit_message(repo).await.map_err(AgentError::from)
        }
        AgentKind::Cursor => {
            crate::cursor::generate_commit_message(repo).await.map_err(AgentError::from)
        }
    }
}

pub async fn ask(
    kind: AgentKind,
    app: tauri::AppHandle,
    runners: Runners,
    warm_pool: WarmPool,
    session_id: &str,
    prompt: &str,
    cwd: Option<&Path>,
    agent_uuid: &str,
    resume: bool,
    rules: Option<&str>,
    cursor_model: Option<&str>,
    claude_model: Option<&str>,
    claude_tool_profile: Option<&str>,
    app_context: Option<&str>,
    action_ipc_socket: Option<&Path>,
    image_paths: &[String],
) -> Result<AgentAskResult, AgentError> {
    match kind {
        AgentKind::Claude => {
            let reply = claude::ask(
                app,
                runners,
                warm_pool,
                session_id,
                prompt,
                cwd,
                agent_uuid,
                resume,
                rules,
                claude_model,
                claude_tool_profile,
                app_context,
                action_ipc_socket,
                image_paths,
            )
            .await?;
            // Claude reuses whatever UUID we handed it, so no change.
            Ok(AgentAskResult {
                reply,
                session_uuid: agent_uuid.to_string(),
            })
        }
        AgentKind::Cursor => {
            let (reply, chat_id) = cursor::ask(
                app,
                runners,
                session_id,
                prompt,
                cwd,
                agent_uuid,
                resume,
                rules,
                cursor_model,
                app_context,
            )
            .await?;
            Ok(AgentAskResult {
                reply,
                session_uuid: chat_id,
            })
        }
    }
}

/// Dispatch `stop` to whichever adapter owns the session. Both adapters track
/// their PIDs in the shared `Runners` map by `session_id`, so either call is
/// safe to try — whoever holds the PID sends the SIGTERM.
pub fn stop(runners: &Runners, session_id: &str) -> bool {
    claude::stop(runners, session_id) || cursor::stop(runners, session_id)
}

/// Kind-dispatched fork-compact. Each adapter runs a two-shot summary →
/// seed-new flow and reports back the new session UUID + summary text.
/// CompactResult shape is shared (same `{ new_uuid, summary }`) so the
/// Tauri command + frontend don't branch on agent kind.
///
/// `proposed_new_uuid` is honoured for Claude (its `--session-id <uuid>`
/// flag accepts a fixed id) and ignored for Cursor (cursor-agent has
/// no equivalent — it always mints its own chat_id, which we read back
/// from the seed-call's result and return as `new_uuid`). Frontend
/// rotates the session's stored uuid to whatever comes back, so both
/// paths converge from the caller's perspective.
pub async fn compact_session(
    kind: AgentKind,
    old_uuid: &str,
    proposed_new_uuid: &str,
    cwd: Option<&Path>,
    model: Option<&str>,
) -> Result<crate::claude::CompactResult, AgentError> {
    match kind {
        AgentKind::Claude => {
            crate::claude::compact_session(old_uuid, proposed_new_uuid, cwd, model)
                .await
                .map_err(AgentError::from)
        }
        AgentKind::Cursor => {
            crate::cursor::compact_session(old_uuid, cwd, model)
                .await
                .map_err(AgentError::from)
        }
    }
}
