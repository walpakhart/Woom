//! Local writer for the `forgehold-memory` SQLite store.
//!
//! Lets the Forgehold UI persist a chat message to long-term memory
//! without going through an MCP round-trip on the agent side. Used
//! by the "Save as note" right-click action on chat bubbles
//! (`docs/ROADMAP_1.0.md §2.2.11`).
//!
//! The schema is assumed to already exist — created by the
//! `forgehold-memory` sidecar on first launch. We only INSERT here;
//! never CREATE / ALTER. If the file is missing we let the error
//! propagate so the toast surfaces it (the user can launch any
//! memory-aware agent column to bootstrap the DB).

use std::path::PathBuf;

use rusqlite::{params, Connection};

fn db_path() -> Result<PathBuf, String> {
    let home = std::env::var("HOME").map_err(|_| "HOME not set".to_string())?;
    Ok(PathBuf::from(home)
        .join("Library")
        .join("Application Support")
        .join("Forgehold")
        .join("memory.db"))
}

#[tauri::command]
pub fn memory_save_local(
    content: String,
    kind: Option<String>,
    tags: Option<Vec<String>>,
) -> Result<i64, String> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return Err("content must not be empty".into());
    }
    /* Validate `kind` against the same closed set the sidecar
     * accepts. Avoids drift between the two writers. */
    let kind = match kind.as_deref().map(str::trim) {
        Some(k) if !k.is_empty() => {
            let lower = k.to_ascii_lowercase();
            match lower.as_str() {
                "user" | "feedback" | "project" | "reference" | "note" => lower,
                _ => return Err(format!("invalid kind: {k}")),
            }
        }
        _ => "note".to_string(),
    };
    let tags_str = tags
        .map(|ts| {
            ts.into_iter()
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty())
                .collect::<Vec<_>>()
                .join(",")
        })
        .unwrap_or_default();

    let path = db_path()?;
    let conn = Connection::open(&path)
        .map_err(|e| format!("open {}: {}", path.display(), e))?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    conn.execute(
        "INSERT INTO memories (content, tags, kind, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?4)",
        params![trimmed, tags_str, kind, now],
    )
    .map_err(|e| format!("insert: {e}"))?;
    Ok(conn.last_insert_rowid())
}
