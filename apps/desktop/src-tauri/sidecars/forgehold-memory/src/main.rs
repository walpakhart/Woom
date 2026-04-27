//! forgehold-memory — MCP sidecar exposing a local SQLite-backed note store.
//!
//! Ships as a Rust binary next to the main forgehold-desktop executable. The
//! desktop app sets `FORGEHOLD_MEMORY_DB` to the path the db should live at
//! (typically `~/Library/Application Support/Forgehold/memory.db` on macOS); we
//! create the file on first run. FTS5 indexes `content` for fast keyword
//! lookup without embeddings or external services.

use std::sync::{Arc, Mutex};

use anyhow::Context;
use rmcp::{
    ErrorData, ServerHandler, ServiceExt,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, Content, ServerCapabilities, ServerInfo},
    schemars, tool, tool_handler, tool_router,
    transport::stdio,
};
use rusqlite::{Connection, params};
use serde::Deserialize;

#[derive(Clone)]
struct Memory {
    db: Arc<Mutex<Connection>>,
    #[allow(dead_code)] // read by `#[tool_handler]` macro expansion
    tool_router: ToolRouter<Self>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SaveParams {
    /// The note text to remember. Write it as a full sentence — this string
    /// is what search queries match against. Good: "User prefers terse
    /// responses with no trailing summaries." Bad: "terse responses".
    content: String,
    /// Optional tags for filtering (comma-joined, e.g. ["user", "style"]).
    #[serde(default)]
    tags: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SearchParams {
    /// FTS5 query string. Supports prefix (`foo*`), phrase (`"foo bar"`),
    /// AND/OR/NOT. Plain words are treated as an AND match.
    query: String,
    /// Max results to return (default 10, cap 50).
    #[serde(default)]
    limit: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ListParams {
    /// Max notes to return, newest first (default 20, cap 100).
    #[serde(default)]
    limit: Option<u32>,
    /// If set, only return notes whose tags include this one.
    #[serde(default)]
    tag: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct DeleteParams {
    /// Row id returned by `memory_save` / `memory_search` / `memory_list`.
    id: i64,
}

#[tool_router]
impl Memory {
    fn new(db_path: &str) -> anyhow::Result<Self> {
        let conn = Connection::open(db_path)
            .with_context(|| format!("open sqlite at {}", db_path))?;
        init_schema(&conn).context("init schema")?;
        Ok(Self {
            db: Arc::new(Mutex::new(conn)),
            tool_router: Self::tool_router(),
        })
    }

    #[tool(
        description = "Save a fact, preference, or note to long-term memory. Use this when the user asks you to remember something, or when you discover a durable preference / project fact worth keeping. Returns the new row id."
    )]
    async fn memory_save(
        &self,
        Parameters(SaveParams { content, tags }): Parameters<SaveParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let content = content.trim().to_string();
        if content.is_empty() {
            return Err(ErrorData::invalid_params("content must not be empty", None));
        }
        let tags_str = tags
            .as_ref()
            .map(|ts| ts.iter().map(|t| t.trim()).filter(|t| !t.is_empty()).collect::<Vec<_>>().join(","))
            .unwrap_or_default();
        let now = unix_now();
        let id = {
            let conn = self.db.lock().map_err(lock_err)?;
            conn.execute(
                "INSERT INTO memories (content, tags, created_at) VALUES (?1, ?2, ?3)",
                params![content, tags_str, now],
            )
            .map_err(sql_err)?;
            conn.last_insert_rowid()
        };
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Saved memory #{id}"
        ))]))
    }

    #[tool(
        description = "Full-text search of stored memories. Returns matches with id, content, and tags. Use this before answering questions about the user's preferences or past context — if a relevant memory exists, quote it back.\n\nIMPORTANT — make ONE broad query, not multiple narrow ones. SQLite FTS handles multi-word queries (`auth login session`) and matches any of them — you don't need separate calls for synonyms. If nothing relevant returns on the first call, the memory genuinely isn't there; iterating with different keywords mostly just costs context tokens for the same null result."
    )]
    async fn memory_search(
        &self,
        Parameters(SearchParams { query, limit }): Parameters<SearchParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let limit = limit.unwrap_or(10).min(50) as i64;
        let query = query.trim();
        if query.is_empty() {
            return Err(ErrorData::invalid_params("query must not be empty", None));
        }
        let rows = {
            let conn = self.db.lock().map_err(lock_err)?;
            let mut stmt = conn
                .prepare(
                    "SELECT m.id, m.content, m.tags, m.created_at \
                     FROM memories m \
                     JOIN memories_fts fts ON fts.rowid = m.id \
                     WHERE memories_fts MATCH ?1 \
                     ORDER BY bm25(memories_fts) \
                     LIMIT ?2",
                )
                .map_err(sql_err)?;
            let rows = stmt
                .query_map(params![query, limit], row_to_tuple)
                .map_err(sql_err)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(sql_err)?;
            rows
        };
        Ok(CallToolResult::success(vec![Content::text(format_rows(&rows))]))
    }

    #[tool(
        description = "List memories newest-first, optionally filtered by tag. Use for browsing / audit, not for answering specific questions (use memory_search for that)."
    )]
    async fn memory_list(
        &self,
        Parameters(ListParams { limit, tag }): Parameters<ListParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let limit = limit.unwrap_or(20).min(100) as i64;
        let rows: Vec<(i64, String, String, i64)> = {
            let conn = self.db.lock().map_err(lock_err)?;
            if let Some(t) = tag.as_ref().map(|s| s.trim()).filter(|s| !s.is_empty()) {
                // Tag column is comma-joined — match the target surrounded by
                // word boundaries (commas or string ends) to avoid substring
                // hits (e.g. `ui` matching `fluid`).
                let pattern = format!("%,{},%", t);
                let mut stmt = conn
                    .prepare(
                        "SELECT id, content, tags, created_at FROM memories \
                         WHERE (',' || tags || ',') LIKE ?1 \
                         ORDER BY created_at DESC LIMIT ?2",
                    )
                    .map_err(sql_err)?;
                let collected: Vec<_> = stmt
                    .query_map(params![pattern, limit], row_to_tuple)
                    .map_err(sql_err)?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(sql_err)?;
                collected
            } else {
                let mut stmt = conn
                    .prepare(
                        "SELECT id, content, tags, created_at FROM memories \
                         ORDER BY created_at DESC LIMIT ?1",
                    )
                    .map_err(sql_err)?;
                let collected: Vec<_> = stmt
                    .query_map(params![limit], row_to_tuple)
                    .map_err(sql_err)?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(sql_err)?;
                collected
            }
        };
        Ok(CallToolResult::success(vec![Content::text(format_rows(&rows))]))
    }

    #[tool(
        description = "Delete a memory by id. Use only when the user asks to forget something specific or when the stored note is no longer accurate."
    )]
    async fn memory_delete(
        &self,
        Parameters(DeleteParams { id }): Parameters<DeleteParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let affected = {
            let conn = self.db.lock().map_err(lock_err)?;
            conn.execute("DELETE FROM memories WHERE id = ?1", params![id])
                .map_err(sql_err)?
        };
        if affected == 0 {
            Ok(CallToolResult::success(vec![Content::text(format!(
                "No memory with id {id}"
            ))]))
        } else {
            Ok(CallToolResult::success(vec![Content::text(format!(
                "Deleted memory #{id}"
            ))]))
        }
    }
}

#[tool_handler]
impl ServerHandler for Memory {
    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::default();
        info.capabilities = ServerCapabilities::builder().enable_tools().build();
        info.instructions = Some(
            "Persistent memory store. Survives across chat sessions. Tools:\n\
             - memory_save: store a fact or preference the user wants remembered\n\
             - memory_search: look up stored notes by keyword before answering personal / project questions\n\
             - memory_list: browse recent entries\n\
             - memory_delete: remove a specific entry by id\n\n\
             RULE: when the user says \"remember X\" or shares a durable preference, call memory_save. When the user asks about something personal / project-specific, call memory_search first to check if you already know."
                .to_string(),
        );
        info
    }
}

fn init_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS memories (\
             id INTEGER PRIMARY KEY AUTOINCREMENT,\
             content TEXT NOT NULL,\
             tags TEXT NOT NULL DEFAULT '',\
             created_at INTEGER NOT NULL\
         );\
         CREATE VIRTUAL TABLE IF NOT EXISTS memories_fts USING fts5(\
             content, tags, content='memories', content_rowid='id', tokenize='porter'\
         );\
         CREATE TRIGGER IF NOT EXISTS memories_ai AFTER INSERT ON memories BEGIN\
             INSERT INTO memories_fts(rowid, content, tags) VALUES (new.id, new.content, new.tags);\
         END;\
         CREATE TRIGGER IF NOT EXISTS memories_ad AFTER DELETE ON memories BEGIN\
             INSERT INTO memories_fts(memories_fts, rowid, content, tags) VALUES('delete', old.id, old.content, old.tags);\
         END;\
         CREATE TRIGGER IF NOT EXISTS memories_au AFTER UPDATE ON memories BEGIN\
             INSERT INTO memories_fts(memories_fts, rowid, content, tags) VALUES('delete', old.id, old.content, old.tags);\
             INSERT INTO memories_fts(rowid, content, tags) VALUES (new.id, new.content, new.tags);\
         END;",
    )?;
    Ok(())
}

fn row_to_tuple(row: &rusqlite::Row) -> rusqlite::Result<(i64, String, String, i64)> {
    Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
}

fn format_rows(rows: &[(i64, String, String, i64)]) -> String {
    if rows.is_empty() {
        return "No matching memories.".to_string();
    }
    rows.iter()
        .map(|(id, content, tags, created_at)| {
            let tag_suffix = if tags.is_empty() {
                String::new()
            } else {
                format!(" [{tags}]")
            };
            format!("#{id} ({created_at}){tag_suffix}\n{content}")
        })
        .collect::<Vec<_>>()
        .join("\n\n---\n\n")
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn lock_err<T>(_: std::sync::PoisonError<T>) -> ErrorData {
    ErrorData::internal_error("memory lock poisoned", None)
}

fn sql_err(e: rusqlite::Error) -> ErrorData {
    ErrorData::internal_error(e.to_string(), None)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let db_path = std::env::var("FORGEHOLD_MEMORY_DB")
        .context("FORGEHOLD_MEMORY_DB env var is required")?;
    if db_path.trim().is_empty() {
        anyhow::bail!("FORGEHOLD_MEMORY_DB must be non-empty");
    }
    if let Some(parent) = std::path::Path::new(&db_path).parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let memory = Memory::new(&db_path)?;
    let service = memory
        .serve(stdio())
        .await
        .context("start MCP service over stdio")?;
    service.waiting().await?;
    Ok(())
}
