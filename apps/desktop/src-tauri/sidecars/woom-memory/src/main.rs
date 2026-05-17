//! woom-memory — MCP sidecar exposing a local SQLite-backed note store.
//!
//! Ships as a Rust binary next to the main woom-desktop executable. The
//! desktop app sets `WOOM_MEMORY_DB` to the path the db should live at
//! (typically `~/Library/Application Support/Woom/memory.db` on macOS); we
//! create the file on first run.
//!
//! Schema (see `init_schema` for the migration ladder):
//!   memories(id, content, tags, kind, created_at, updated_at)
//!   memories_fts (FTS5, tokenize='unicode61 remove_diacritics 2')
//!
//! Why `unicode61` and not `porter`: the user writes in Russian and English
//! interchangeably. `porter` only stems English; Cyrillic words pass through
//! it un-normalised so a search for "память" doesn't match "память." (with a
//! period) or "Память" (capitalised). `unicode61` does case-folding and
//! diacritic-stripping over the full Unicode range, so both languages work
//! out-of-the-box.

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

/// Allowed memory kinds. Mirrors the taxonomy Claude Code's auto-memory
/// system uses so agents can shuttle entries between the two stores
/// without translating types. `note` is the catch-all default for
/// anything that doesn't fit the four buckets.
const KINDS: &[&str] = &["user", "feedback", "project", "reference", "note"];

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SaveParams {
    /// The note text to remember. Write it as a full sentence — this string
    /// is what search queries match against. Good: "User prefers terse
    /// responses with no trailing summaries." Bad: "terse responses".
    content: String,
    /// Optional tags for filtering (comma-joined, e.g. ["user", "style"]).
    #[serde(default)]
    tags: Option<Vec<String>>,
    /// Memory kind — drives how this entry should be applied later.
    /// One of: `user` (about the user — role, preferences),
    /// `feedback` (how to approach work — corrections, validated patterns),
    /// `project` (ongoing initiatives, decisions, deadlines),
    /// `reference` (pointers to external systems / dashboards),
    /// `note` (catch-all). Defaults to `note` when omitted.
    #[serde(default)]
    kind: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SearchParams {
    /// FTS5 query string. Supports prefix (`foo*`), phrase (`"foo bar"`),
    /// AND/OR/NOT. Plain words are treated as an AND match. Tokenizer
    /// is `unicode61 remove_diacritics 2`, so case + diacritics +
    /// Cyrillic-vs-Latin variants normalize automatically.
    query: String,
    /// Max results to return (default 10, cap 50).
    #[serde(default)]
    limit: Option<u32>,
    /// If set, only search within memories of this kind.
    #[serde(default)]
    kind: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ListParams {
    /// Max notes to return, newest first (default 20, cap 100).
    #[serde(default)]
    limit: Option<u32>,
    /// If set, only return notes whose tags include this one.
    #[serde(default)]
    tag: Option<String>,
    /// If set, only return notes of this kind.
    #[serde(default)]
    kind: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct GetParams {
    /// Row id returned by `memory_save` / `memory_search` / `memory_list`.
    id: i64,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct UpdateParams {
    /// Row id to edit.
    id: i64,
    /// New content. Omit to leave content unchanged.
    #[serde(default)]
    content: Option<String>,
    /// Replacement tag set. Omit to leave tags unchanged. Pass
    /// an empty array to clear all tags.
    #[serde(default)]
    tags: Option<Vec<String>>,
    /// New kind. Omit to leave unchanged.
    #[serde(default)]
    kind: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct DeleteParams {
    /// Row id returned by `memory_save` / `memory_search` / `memory_list`.
    id: i64,
}

/// Snapshot of a memory row used for output formatting. Avoids
/// passing tuple fields around once we have more than three columns.
#[derive(Debug, Clone)]
struct MemoryRow {
    id: i64,
    content: String,
    tags: String,
    kind: String,
    created_at: i64,
    updated_at: i64,
}

#[tool_router]
impl Memory {
    fn new(db_path: &str) -> anyhow::Result<Self> {
        let conn = Connection::open(db_path)
            .with_context(|| format!("open sqlite at {}", db_path))?;
        /* Durability + concurrency pragmas. WAL lets readers and a
         * single writer coexist (the sidecar is single-threaded but
         * memory_local.rs writes from the desktop process — two
         * writers on the same file). synchronous=NORMAL gives one
         * fsync per checkpoint instead of per transaction, which on
         * the modest memory write rate is fine: durable enough to
         * survive process crashes, fast enough that bursts don't
         * stall on disk. busy_timeout retries on lock contention
         * instead of immediately returning SQLITE_BUSY. */
        conn.pragma_update(None, "journal_mode", "WAL")
            .context("set journal_mode=WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")
            .context("set synchronous=NORMAL")?;
        conn.pragma_update(None, "busy_timeout", 2_000)
            .context("set busy_timeout=2000")?;
        init_schema(&conn).context("init schema")?;
        Ok(Self {
            db: Arc::new(Mutex::new(conn)),
            tool_router: Self::tool_router(),
        })
    }

    #[tool(
        description = "Save a fact, preference, or note to long-term memory. Use this when the user asks you to remember something, or when you discover a durable preference / project fact worth keeping. Pass `kind` to tag the bucket (user / feedback / project / reference / note) so future searches can filter. Returns the new row id."
    )]
    async fn memory_save(
        &self,
        Parameters(SaveParams { content, tags, kind }): Parameters<SaveParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let content = content.trim().to_string();
        if content.is_empty() {
            return Err(ErrorData::invalid_params("content must not be empty", None));
        }
        let kind = normalize_kind(kind.as_deref())?;
        let tags_str = serialize_tags(tags.as_ref());
        let now = unix_now();
        let id = {
            let conn = self.db.lock().map_err(lock_err)?;
            conn.execute(
                "INSERT INTO memories (content, tags, kind, created_at, updated_at) \
                 VALUES (?1, ?2, ?3, ?4, ?4)",
                params![content, tags_str, kind, now],
            )
            .map_err(sql_err)?;
            conn.last_insert_rowid()
        };
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Saved memory #{id} (kind={kind})"
        ))]))
    }

    #[tool(
        description = "Full-text search of stored memories. Returns matches with id, kind, content, and tags. SQLite FTS handles multi-word queries with unicode-aware case-folding, so Russian and English work the same way. Pass `kind` to scope the search."
    )]
    async fn memory_search(
        &self,
        Parameters(SearchParams { query, limit, kind }): Parameters<SearchParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let limit = limit.unwrap_or(10).min(50) as i64;
        let query = query.trim();
        if query.is_empty() {
            return Err(ErrorData::invalid_params("query must not be empty", None));
        }
        let kind_filter = match kind.as_deref() {
            Some(k) => Some(normalize_kind(Some(k))?),
            None => None,
        };
        let safe_query = sanitize_fts5_query(query);
        if safe_query.is_empty() {
            /* All tokens were stripped (e.g. query was pure punctuation
             * like ":::"). Return empty result rather than letting an
             * empty MATCH string error out at SQLite level. */
            return Ok(CallToolResult::success(vec![Content::text(
                "No matching memories.".to_string(),
            )]));
        }
        let rows = {
            let conn = self.db.lock().map_err(lock_err)?;
            let sql = if kind_filter.is_some() {
                "SELECT m.id, m.content, m.tags, m.kind, m.created_at, m.updated_at \
                 FROM memories m \
                 JOIN memories_fts fts ON fts.rowid = m.id \
                 WHERE memories_fts MATCH ?1 AND m.kind = ?2 \
                 ORDER BY bm25(memories_fts) \
                 LIMIT ?3"
            } else {
                "SELECT m.id, m.content, m.tags, m.kind, m.created_at, m.updated_at \
                 FROM memories m \
                 JOIN memories_fts fts ON fts.rowid = m.id \
                 WHERE memories_fts MATCH ?1 \
                 ORDER BY bm25(memories_fts) \
                 LIMIT ?2"
            };
            /* Bind `stmt` and the iterator separately so the
             * MappedRows borrow on `stmt` is unambiguously contained
             * in the block scope. The chained-`?` form `stmt.query_map
             * (...)?.collect(...)?` confuses NLL (Rust 1.75) into
             * thinking `stmt` is dropped before the temporary
             * `Result` finishes. */
            let mut stmt = conn.prepare(sql).map_err(sql_err)?;
            let iter = if let Some(k) = &kind_filter {
                stmt.query_map(params![safe_query, k, limit], row_to_memory)
                    .map_err(sql_err)?
            } else {
                stmt.query_map(params![safe_query, limit], row_to_memory)
                    .map_err(sql_err)?
            };
            iter.collect::<Result<Vec<_>, _>>().map_err(sql_err)?
        };
        Ok(CallToolResult::success(vec![Content::text(format_rows(&rows))]))
    }

    #[tool(
        description = "List memories newest-first, optionally filtered by tag and/or kind. Use for browsing / audit, not for answering specific questions (use memory_search for that)."
    )]
    async fn memory_list(
        &self,
        Parameters(ListParams { limit, tag, kind }): Parameters<ListParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let limit = limit.unwrap_or(20).min(100) as i64;
        let kind_filter = match kind.as_deref() {
            Some(k) => Some(normalize_kind(Some(k))?),
            None => None,
        };
        let tag_clean = tag
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from);
        let rows: Vec<MemoryRow> = {
            let conn = self.db.lock().map_err(lock_err)?;
            /* Same NLL-friendly split as `memory_search`: keep the
             * `stmt` binding in scope until the iterator is fully
             * collected. */
            match (tag_clean, kind_filter) {
                (Some(t), Some(k)) => {
                    let pattern = like_tag_pattern(&t);
                    let mut stmt = conn
                        .prepare(
                            "SELECT id, content, tags, kind, created_at, updated_at \
                             FROM memories \
                             WHERE (',' || tags || ',') LIKE ?1 ESCAPE '\\' \
                               AND kind = ?2 \
                             ORDER BY created_at DESC LIMIT ?3",
                        )
                        .map_err(sql_err)?;
                    let iter = stmt
                        .query_map(params![pattern, k, limit], row_to_memory)
                        .map_err(sql_err)?;
                    iter.collect::<Result<Vec<_>, _>>().map_err(sql_err)?
                }
                (Some(t), None) => {
                    let pattern = like_tag_pattern(&t);
                    let mut stmt = conn
                        .prepare(
                            "SELECT id, content, tags, kind, created_at, updated_at \
                             FROM memories \
                             WHERE (',' || tags || ',') LIKE ?1 ESCAPE '\\' \
                             ORDER BY created_at DESC LIMIT ?2",
                        )
                        .map_err(sql_err)?;
                    let iter = stmt
                        .query_map(params![pattern, limit], row_to_memory)
                        .map_err(sql_err)?;
                    iter.collect::<Result<Vec<_>, _>>().map_err(sql_err)?
                }
                (None, Some(k)) => {
                    let mut stmt = conn
                        .prepare(
                            "SELECT id, content, tags, kind, created_at, updated_at \
                             FROM memories \
                             WHERE kind = ?1 \
                             ORDER BY created_at DESC LIMIT ?2",
                        )
                        .map_err(sql_err)?;
                    let iter = stmt
                        .query_map(params![k, limit], row_to_memory)
                        .map_err(sql_err)?;
                    iter.collect::<Result<Vec<_>, _>>().map_err(sql_err)?
                }
                (None, None) => {
                    let mut stmt = conn
                        .prepare(
                            "SELECT id, content, tags, kind, created_at, updated_at \
                             FROM memories \
                             ORDER BY created_at DESC LIMIT ?1",
                        )
                        .map_err(sql_err)?;
                    let iter = stmt
                        .query_map(params![limit], row_to_memory)
                        .map_err(sql_err)?;
                    iter.collect::<Result<Vec<_>, _>>().map_err(sql_err)?
                }
            }
        };
        Ok(CallToolResult::success(vec![Content::text(format_rows(&rows))]))
    }

    #[tool(
        description = "Fetch a single memory by id. Useful right after `memory_update` to confirm the new state, or to re-read a row referenced from a chat."
    )]
    async fn memory_get(
        &self,
        Parameters(GetParams { id }): Parameters<GetParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let row: Option<MemoryRow> = {
            let conn = self.db.lock().map_err(lock_err)?;
            let mut stmt = conn
                .prepare(
                    "SELECT id, content, tags, kind, created_at, updated_at \
                     FROM memories WHERE id = ?1",
                )
                .map_err(sql_err)?;
            stmt.query_row(params![id], row_to_memory)
                .map(Some)
                .or_else(|e| match e {
                    rusqlite::Error::QueryReturnedNoRows => Ok(None),
                    other => Err(other),
                })
                .map_err(sql_err)?
        };
        match row {
            Some(r) => Ok(CallToolResult::success(vec![Content::text(
                format_rows(&[r]),
            )])),
            None => Ok(CallToolResult::success(vec![Content::text(format!(
                "No memory with id {id}"
            ))])),
        }
    }

    #[tool(
        description = "Edit an existing memory in place. Pass any subset of `content`, `tags`, `kind` — omitted fields stay as-is. Pass an empty `tags` array to clear all tags. Use this when a stored fact changes (e.g. preference shifts, project status moves) instead of delete+save."
    )]
    async fn memory_update(
        &self,
        Parameters(UpdateParams { id, content, tags, kind }): Parameters<UpdateParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let kind_norm = match kind.as_deref() {
            Some(k) => Some(normalize_kind(Some(k))?),
            None => None,
        };
        let new_content = match content {
            Some(c) => {
                let trimmed = c.trim().to_string();
                if trimmed.is_empty() {
                    return Err(ErrorData::invalid_params(
                        "content cannot be empty if provided; pass null to keep existing",
                        None,
                    ));
                }
                Some(trimmed)
            }
            None => None,
        };
        let new_tags_str = tags.as_ref().map(|t| serialize_tags(Some(t)));
        let now = unix_now();

        let updated = {
            let conn = self.db.lock().map_err(lock_err)?;
            /* Build the UPDATE dynamically based on which fields the
             * caller supplied. Always touch `updated_at` so callers
             * know the row was rewritten. */
            let mut sets: Vec<&str> = Vec::new();
            let mut bound: Vec<rusqlite::types::Value> = Vec::new();
            if let Some(c) = &new_content {
                sets.push("content = ?");
                bound.push(c.clone().into());
            }
            if let Some(t) = &new_tags_str {
                sets.push("tags = ?");
                bound.push(t.clone().into());
            }
            if let Some(k) = &kind_norm {
                sets.push("kind = ?");
                bound.push(k.clone().into());
            }
            if sets.is_empty() {
                return Err(ErrorData::invalid_params(
                    "memory_update requires at least one of content / tags / kind",
                    None,
                ));
            }
            sets.push("updated_at = ?");
            bound.push(now.into());
            bound.push(id.into());
            let sql = format!(
                "UPDATE memories SET {} WHERE id = ?",
                sets.join(", ")
            );
            let bound_refs: Vec<&dyn rusqlite::ToSql> =
                bound.iter().map(|v| v as &dyn rusqlite::ToSql).collect();
            conn.execute(&sql, &bound_refs[..]).map_err(sql_err)?
        };

        if updated == 0 {
            Ok(CallToolResult::success(vec![Content::text(format!(
                "No memory with id {id}"
            ))]))
        } else {
            Ok(CallToolResult::success(vec![Content::text(format!(
                "Updated memory #{id}"
            ))]))
        }
    }

    #[tool(
        description = "Delete a memory by id. Use only when the user asks to forget something specific or when the stored note is no longer accurate (prefer `memory_update` if part of the note is still useful)."
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
             - memory_save: store a fact (kind: user / feedback / project / reference / note)\n\
             - memory_search: FTS5 lookup, optionally scoped by kind (works for Russian + English)\n\
             - memory_list: browse newest-first, optionally filtered by tag and/or kind\n\
             - memory_get: fetch one row by id\n\
             - memory_update: edit content / tags / kind in place\n\
             - memory_delete: remove by id\n\n\
             Kind taxonomy mirrors Claude Code's auto-memory:\n\
             - user: about the user (role, preferences, knowledge)\n\
             - feedback: how to approach work (corrections, validated patterns)\n\
             - project: ongoing initiatives, decisions, deadlines\n\
             - reference: pointers to external systems / dashboards\n\
             - note: catch-all\n\n\
             RULE: when the user says \"remember X\" or shares a durable preference, call memory_save with the right kind. When the user asks about something personal / project-specific, call memory_search first. When a stored fact changes, prefer memory_update over delete+save so id stability is preserved."
                .to_string(),
        );
        info
    }
}

/// Initialize / migrate the schema. Forward-only: every change here
/// must be safe to re-run on an already-migrated DB. We use
/// `IF NOT EXISTS` for tables and `PRAGMA table_info` lookups for
/// column adds (`ALTER TABLE ADD COLUMN` errors on duplicates).
fn init_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS memories (\
             id INTEGER PRIMARY KEY AUTOINCREMENT,\
             content TEXT NOT NULL,\
             tags TEXT NOT NULL DEFAULT '',\
             created_at INTEGER NOT NULL\
         );",
    )?;

    /* Column adds — guarded by a `table_info` check so re-running
     * doesn't error on an already-migrated DB. SQLite's `ALTER TABLE
     * ADD COLUMN IF NOT EXISTS` is 3.35+; we don't pin a min version
     * so the manual check is safer. */
    let existing_cols = list_columns(conn, "memories")?;
    if !existing_cols.iter().any(|c| c == "kind") {
        conn.execute_batch(
            "ALTER TABLE memories ADD COLUMN kind TEXT NOT NULL DEFAULT 'note';",
        )?;
    }
    if !existing_cols.iter().any(|c| c == "updated_at") {
        /* Default 0 for legacy rows so we can distinguish never-edited
         * from "edited at unix epoch" if a future feature cares. */
        conn.execute_batch(
            "ALTER TABLE memories ADD COLUMN updated_at INTEGER NOT NULL DEFAULT 0;",
        )?;
    }

    /* FTS5 — drop+rebuild if the existing virtual table was created
     * with the old `porter` tokenizer. We detect by inspecting the
     * stored CREATE statement; for fresh DBs we just create the new
     * one. The rebuild is cheap (only as expensive as re-tokenizing
     * every row, which is sub-millisecond per memory). */
    let needs_rebuild = match fts_tokenizer(conn)? {
        Some(t) => !t.contains("unicode61"),
        None => false,
    };
    if needs_rebuild {
        conn.execute_batch("DROP TABLE IF EXISTS memories_fts;")?;
    }

    /* Each statement on its own concatenated line so Rust's `\<newline>`
     * (which strips ALL leading whitespace on the next line) doesn't
     * collapse `BEGIN\n  INSERT` into `BEGININSERT` — the bug that
     * silently broke the original 0.1 schema; nothing detected it
     * because `init_schema` had no tests then. */
    conn.execute_batch(concat!(
        "CREATE VIRTUAL TABLE IF NOT EXISTS memories_fts USING fts5(",
        "content, tags, content='memories', content_rowid='id',",
        " tokenize='unicode61 remove_diacritics 2');",
        "CREATE TRIGGER IF NOT EXISTS memories_ai AFTER INSERT ON memories",
        " BEGIN INSERT INTO memories_fts(rowid, content, tags)",
        " VALUES (new.id, new.content, new.tags); END;",
        "CREATE TRIGGER IF NOT EXISTS memories_ad AFTER DELETE ON memories",
        " BEGIN INSERT INTO memories_fts(memories_fts, rowid, content, tags)",
        " VALUES('delete', old.id, old.content, old.tags); END;",
        "CREATE TRIGGER IF NOT EXISTS memories_au AFTER UPDATE ON memories",
        " BEGIN INSERT INTO memories_fts(memories_fts, rowid, content, tags)",
        " VALUES('delete', old.id, old.content, old.tags);",
        " INSERT INTO memories_fts(rowid, content, tags)",
        " VALUES (new.id, new.content, new.tags); END;",
    ))?;

    if needs_rebuild {
        /* Re-populate from the source table after the drop. */
        conn.execute_batch(
            "INSERT INTO memories_fts(rowid, content, tags) \
             SELECT id, content, tags FROM memories;",
        )?;
    }

    Ok(())
}

fn list_columns(conn: &Connection, table: &str) -> rusqlite::Result<Vec<String>> {
    let sql = format!("PRAGMA table_info({})", table);
    let mut stmt = conn.prepare(&sql)?;
    let cols = stmt
        .query_map([], |r| r.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(cols)
}

fn fts_tokenizer(conn: &Connection) -> rusqlite::Result<Option<String>> {
    let mut stmt =
        conn.prepare("SELECT sql FROM sqlite_master WHERE type='table' AND name='memories_fts'")?;
    let sql: Option<String> = stmt
        .query_row([], |r| r.get::<_, String>(0))
        .map(Some)
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(other),
        })?;
    Ok(sql)
}

fn row_to_memory(row: &rusqlite::Row) -> rusqlite::Result<MemoryRow> {
    Ok(MemoryRow {
        id: row.get(0)?,
        content: row.get(1)?,
        tags: row.get(2)?,
        kind: row.get(3)?,
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
    })
}

fn format_rows(rows: &[MemoryRow]) -> String {
    if rows.is_empty() {
        return "No matching memories.".to_string();
    }
    rows.iter()
        .map(|r| {
            let tag_suffix = if r.tags.is_empty() {
                String::new()
            } else {
                format!(" tags=[{}]", r.tags)
            };
            let edited = if r.updated_at > 0 && r.updated_at != r.created_at {
                format!(" · edited {}", iso_8601(r.updated_at))
            } else {
                String::new()
            };
            format!(
                "#{id} kind={kind} created={created}{edited}{tag_suffix}\n{content}",
                id = r.id,
                kind = r.kind,
                created = iso_8601(r.created_at),
                edited = edited,
                tag_suffix = tag_suffix,
                content = r.content
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n---\n\n")
}

/// Format unix epoch seconds as ISO 8601 UTC.
///
/// Avoids a `chrono` dependency — manual calendar math is fine because
/// agents only read the string for display. For epoch 0 ("never
/// edited" sentinel) returns "never".
fn iso_8601(epoch_secs: i64) -> String {
    if epoch_secs <= 0 {
        return "never".to_string();
    }
    let mut secs = epoch_secs;
    let s = (secs % 60) as u32;
    secs /= 60;
    let m = (secs % 60) as u32;
    secs /= 60;
    let h = (secs % 24) as u32;
    let mut days = secs / 24;

    /* Days since 1970-01-01. Convert to Y-M-D using the standard
     * civil-from-days algorithm (Howard Hinnant's date.h paper —
     * works for any Gregorian date in i64 range). */
    days += 719468;
    let era = if days >= 0 { days } else { days - 146096 } / 146097;
    let doe = (days - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m_civ = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m_civ <= 2 { y + 1 } else { y };

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        y, m_civ, d, h, m, s
    )
}

fn normalize_kind(input: Option<&str>) -> Result<String, ErrorData> {
    let raw = input.unwrap_or("note").trim();
    if raw.is_empty() {
        return Ok("note".to_string());
    }
    let lower = raw.to_ascii_lowercase();
    if KINDS.iter().any(|k| *k == lower) {
        Ok(lower)
    } else {
        Err(ErrorData::invalid_params(
            format!(
                "kind must be one of: {} (got '{}')",
                KINDS.join(", "),
                raw
            ),
            None,
        ))
    }
}

fn serialize_tags(tags: Option<&Vec<String>>) -> String {
    tags.map(|ts| {
        ts.iter()
            .map(|t| t.trim())
            .filter(|t| !t.is_empty())
            .collect::<Vec<_>>()
            .join(",")
    })
    .unwrap_or_default()
}

/// Build a LIKE pattern that matches a tag inside a comma-joined
/// `(',' || tags || ',')` haystack. Escapes the SQL wildcard chars
/// `%`, `_`, and `\` so a tag like `100%` doesn't match unrelated
/// rows. Use the matching `ESCAPE '\\'` clause in the LIKE.
fn like_tag_pattern(t: &str) -> String {
    let mut escaped = String::with_capacity(t.len() + 4);
    for c in t.chars() {
        if c == '%' || c == '_' || c == '\\' {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    format!("%,{escaped},%")
}

/// Sanitize a user-supplied FTS5 MATCH query.
///
/// FTS5 has its own mini-DSL on top of the user's words: `:` means
/// "column filter" (`tags:foo`), `*` is prefix, `"…"` is phrase,
/// `AND/OR/NOT/NEAR` are operators, `(` `)` group, and bare numbers
/// like `475:` are interpreted as column references — producing the
/// "no such column: 475" error from the 2026-05-16 incident.
///
/// Strategy: tokenize on whitespace, strip every FTS5 metachar from
/// each token (`"`, `:`, `(`, `)`, `*`, `^`), drop empty tokens, then
/// re-emit each as a quoted phrase so any remaining content (digits,
/// punctuation, Cyrillic) is treated as literal text. Multiple
/// phrases joined by whitespace yield FTS5's implicit AND-of-phrases,
/// which is exactly the recall semantics we want.
///
/// Trade-off: power users lose access to prefix-search (`foo*`) and
/// boolean operators. That's acceptable — the tool description never
/// promised them, and the cost of one mis-typed `:` killing search
/// for everyone is much higher than the upside of advanced syntax.
fn sanitize_fts5_query(input: &str) -> String {
    /* Characters that have syntactic meaning to FTS5. Stripping these
     * is safer than escaping because the FTS5 grammar treats some of
     * them (e.g. `*` in column ref position) ambiguously. */
    fn strip(c: char) -> bool {
        matches!(c, '"' | ':' | '(' | ')' | '*' | '^' | '\\' | '\0')
    }
    input
        .split_whitespace()
        .map(|tok| tok.chars().filter(|c| !strip(*c)).collect::<String>())
        .filter(|tok| !tok.is_empty())
        .map(|tok| format!("\"{}\"", tok))
        .collect::<Vec<_>>()
        .join(" ")
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
    let db_path = std::env::var("WOOM_MEMORY_DB")
        .context("WOOM_MEMORY_DB env var is required")?;
    if db_path.trim().is_empty() {
        anyhow::bail!("WOOM_MEMORY_DB must be non-empty");
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

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn fresh_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        conn
    }

    #[test]
    fn schema_initializes_clean_db() {
        let conn = fresh_db();
        let cols = list_columns(&conn, "memories").unwrap();
        assert!(cols.contains(&"kind".to_string()));
        assert!(cols.contains(&"updated_at".to_string()));
        let tok = fts_tokenizer(&conn).unwrap().expect("fts table exists");
        assert!(tok.contains("unicode61"));
    }

    #[test]
    fn schema_migrates_legacy_db() {
        /* Simulate the pre-migration shape: missing kind / updated_at,
         * porter tokenizer. init_schema must add the columns and
         * rebuild the FTS table. */
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE memories (\
                 id INTEGER PRIMARY KEY AUTOINCREMENT,\
                 content TEXT NOT NULL,\
                 tags TEXT NOT NULL DEFAULT '',\
                 created_at INTEGER NOT NULL\
             );\
             INSERT INTO memories(content, tags, created_at) \
                 VALUES ('legacy', 'a,b', 100);\
             CREATE VIRTUAL TABLE memories_fts USING fts5(\
                 content, tags, content='memories', content_rowid='id', tokenize='porter'\
             );\
             INSERT INTO memories_fts(rowid, content, tags) VALUES (1, 'legacy', 'a,b');",
        )
        .unwrap();
        init_schema(&conn).unwrap();
        let cols = list_columns(&conn, "memories").unwrap();
        assert!(cols.contains(&"kind".to_string()));
        assert!(cols.contains(&"updated_at".to_string()));
        let tok = fts_tokenizer(&conn).unwrap().expect("fts");
        assert!(tok.contains("unicode61"));
        /* Existing row survives + has default kind */
        let kind: String = conn
            .query_row("SELECT kind FROM memories WHERE id = 1", [], |r| r.get(0))
            .unwrap();
        assert_eq!(kind, "note");
        /* FTS still finds the row after rebuild */
        let count: i64 = conn
            .query_row(
                "SELECT count(*) FROM memories_fts WHERE memories_fts MATCH ?1",
                params!["legacy"],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn normalize_kind_accepts_canonical_values() {
        for k in KINDS {
            assert_eq!(normalize_kind(Some(k)).unwrap(), *k);
        }
    }

    #[test]
    fn normalize_kind_lowercases_and_trims() {
        assert_eq!(normalize_kind(Some("  USER ")).unwrap(), "user");
    }

    #[test]
    fn normalize_kind_rejects_garbage() {
        assert!(normalize_kind(Some("invalid")).is_err());
    }

    #[test]
    fn normalize_kind_defaults_to_note() {
        assert_eq!(normalize_kind(None).unwrap(), "note");
        assert_eq!(normalize_kind(Some("")).unwrap(), "note");
    }

    #[test]
    fn like_tag_pattern_escapes_wildcards() {
        assert_eq!(like_tag_pattern("ui"), "%,ui,%");
        assert_eq!(like_tag_pattern("100%"), "%,100\\%,%");
        assert_eq!(like_tag_pattern("a_b"), "%,a\\_b,%");
        assert_eq!(like_tag_pattern("c\\d"), "%,c\\\\d,%");
    }

    #[test]
    fn iso_8601_formats_known_epochs() {
        /* 2026-01-01T00:00:00Z = 1767225600 */
        assert_eq!(iso_8601(1767225600), "2026-01-01T00:00:00Z");
        /* 1970-01-01T00:00:01Z */
        assert_eq!(iso_8601(1), "1970-01-01T00:00:01Z");
        /* 0 / negative → "never" sentinel */
        assert_eq!(iso_8601(0), "never");
        assert_eq!(iso_8601(-1), "never");
    }

    #[test]
    fn iso_8601_handles_leap_year() {
        /* 2024-02-29T12:34:56Z = 1709210096 */
        assert_eq!(iso_8601(1709210096), "2024-02-29T12:34:56Z");
    }

    #[test]
    fn sanitize_fts5_strips_column_ref_syntax() {
        /* The 2026-05-16 incident: `475:` was read as a column
         * reference and SQLite errored with "no such column: 475". */
        assert_eq!(sanitize_fts5_query("client 475:"), "\"client\" \"475\"");
        assert_eq!(sanitize_fts5_query("foo:bar"), "\"foobar\"");
    }

    #[test]
    fn sanitize_fts5_strips_operators_and_quotes() {
        assert_eq!(
            sanitize_fts5_query("\"hello\" AND world*"),
            "\"hello\" \"AND\" \"world\""
        );
        assert_eq!(sanitize_fts5_query("(a OR b)"), "\"a\" \"OR\" \"b\"");
    }

    #[test]
    fn sanitize_fts5_keeps_unicode() {
        assert_eq!(
            sanitize_fts5_query("кнопка density"),
            "\"кнопка\" \"density\""
        );
    }

    #[test]
    fn sanitize_fts5_empty_when_all_punct() {
        assert_eq!(sanitize_fts5_query(":::"), "");
        assert_eq!(sanitize_fts5_query("   "), "");
    }

    #[test]
    fn fts_search_survives_colon_query() {
        /* Pre-fix this query crashed at SQLite level. Now it should
         * just return zero matches without erroring. */
        let conn = fresh_db();
        conn.execute(
            "INSERT INTO memories (content, tags, kind, created_at, updated_at) \
             VALUES (?1, '', 'note', 100, 0)",
            params!["client Acme had 475 widgets"],
        )
        .unwrap();
        /* Use the sanitizer the way memory_search does. */
        let safe = sanitize_fts5_query("client 475:");
        assert!(!safe.is_empty());
        let mut stmt = conn
            .prepare(
                "SELECT m.id FROM memories m \
                 JOIN memories_fts fts ON fts.rowid = m.id \
                 WHERE memories_fts MATCH ?1",
            )
            .unwrap();
        let ids: Vec<i64> = stmt
            .query_map(params![safe], |r| r.get::<_, i64>(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(ids, vec![1]);
    }

    #[test]
    fn serialize_tags_filters_blanks_and_trims() {
        let tags = vec!["  a  ".to_string(), "".to_string(), "b".to_string()];
        assert_eq!(serialize_tags(Some(&tags)), "a,b");
        assert_eq!(serialize_tags(None), "");
    }
}
