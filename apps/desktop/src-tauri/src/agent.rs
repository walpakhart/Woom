//! Agent dispatcher — routes forge chat sessions to either the Claude Code
//! or Cursor Agent CLI. Both adapters (`claude.rs`, `cursor.rs`) emit the
//! same Claude-style stream-json events to the frontend so the UI never
//! branches on agent kind.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::claude::{self, Runners};
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
    session_id: &str,
    prompt: &str,
    cwd: Option<&Path>,
    agent_uuid: &str,
    resume: bool,
    rules: Option<&str>,
    cursor_model: Option<&str>,
) -> Result<AgentAskResult, AgentError> {
    match kind {
        AgentKind::Claude => {
            let reply = claude::ask(
                app,
                runners,
                session_id,
                prompt,
                cwd,
                agent_uuid,
                resume,
                rules,
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
