//! Local reader / writer for the `woom-memory` SQLite store.
//!
//! Lets the Woom UI talk to long-term memory without an MCP round-
//! trip on the agent side. Two commands:
//!   - `memory_save_local`: insert a row (used by paste-trap + the
//!     right-click "Save as note" on chat bubbles, see
//!     `docs/ROADMAP_1.0.md §2.2.11`).
//!   - `memory_search_local`: FTS5 query, returns top-N hits (used
//!     by the first-turn preamble to auto-recall memories scoped to
//!     the current cwd / repo before the agent even reads its
//!     question).
//!
//! The schema is assumed to already exist — created by the
//! `woom-memory` sidecar on first launch. We only INSERT / SELECT
//! here; never CREATE / ALTER. If the file is missing we let the
//! error propagate so the toast surfaces it (the user can launch
//! any memory-aware agent column to bootstrap the DB).

use std::path::PathBuf;

use rusqlite::{params, Connection};
use serde::Serialize;

fn db_path() -> Result<PathBuf, String> {
    let home = std::env::var("HOME").map_err(|_| "HOME not set".to_string())?;
    Ok(PathBuf::from(home)
        .join("Library")
        .join("Application Support")
        .join("Woom")
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
    /* Match the sidecar's per-connection pragmas. WAL mode itself is
     * a DB-file attribute set by whichever process opened first
     * (typically the woom-memory sidecar), so we inherit it; busy_timeout
     * is per-connection though — without it concurrent writes from
     * the sidecar AND this command race on SQLITE_BUSY instead of
     * waiting. 2s is plenty for any healthy contention scenario. */
    conn.busy_timeout(std::time::Duration::from_millis(2_000))
        .map_err(|e| format!("set busy_timeout: {e}"))?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    /* Substring dedup. If a memory entry already contains this exact
     * content (or vice versa, the incoming content is a superset),
     * we'd otherwise stack near-duplicates over time — same fact
     * saved on session 1, on session 2, on session 3 with two extra
     * words. The agent (and the user) reading memory_search results
     * would see noise. Promote duplicates by updating the existing
     * row's content + updated_at instead, returning the SAME id.
     *
     * Scoped to entries created in the last 90 days so old, possibly
     * stale rows aren't silently overwritten by tangentially-similar
     * new content. Same-kind comparison only — a `note` mentioning a
     * project key shouldn't subsume a `user` preference that happens
     * to mention the same key. */
    let ninety_days = 90 * 24 * 60 * 60;
    let cutoff = now - ninety_days;
    let candidates: Vec<(i64, String)> = {
        let mut stmt = conn
            .prepare(
                "SELECT id, content FROM memories \
                 WHERE kind = ?1 AND created_at >= ?2 \
                 ORDER BY created_at DESC LIMIT 200",
            )
            .map_err(|e| format!("prepare dedup: {e}"))?;
        let rows = stmt
            .query_map(params![kind, cutoff], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
            })
            .map_err(|e| format!("query dedup: {e}"))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("collect dedup: {e}"))?
    };
    let trimmed_str = trimmed.to_string();
    for (existing_id, existing_content) in &candidates {
        if is_substring_dup(&trimmed_str, existing_content) {
            /* Take the longer of the two as the canonical text.
             * Keeps whichever variant has more context. */
            let winning = if trimmed_str.len() > existing_content.len() {
                trimmed_str.as_str()
            } else {
                existing_content.as_str()
            };
            conn.execute(
                "UPDATE memories SET content = ?1, tags = ?2, updated_at = ?3 \
                 WHERE id = ?4",
                params![winning, tags_str, now, existing_id],
            )
            .map_err(|e| format!("dedup update: {e}"))?;
            return Ok(*existing_id);
        }
    }
    conn.execute(
        "INSERT INTO memories (content, tags, kind, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?4)",
        params![trimmed, tags_str, kind, now],
    )
    .map_err(|e| format!("insert: {e}"))?;
    Ok(conn.last_insert_rowid())
}

/// Whitespace-normalized substring check. Two memory contents count as
/// "duplicate" when one's normalized form contains the other AND the
/// shorter is at least 40 chars — short strings (paths, ids) are too
/// generic to dedupe on substring alone. Normalization collapses
/// internal whitespace + lowercases so a re-saved sentence with one
/// extra newline still matches.
fn is_substring_dup(a: &str, b: &str) -> bool {
    fn norm(s: &str) -> String {
        s.to_lowercase()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }
    let na = norm(a);
    let nb = norm(b);
    let short_len = na.len().min(nb.len());
    if short_len < 40 {
        return false;
    }
    na.contains(&nb) || nb.contains(&na)
}

#[derive(Debug, Serialize)]
pub struct MemoryHit {
    pub id: i64,
    pub kind: String,
    pub content: String,
    pub tags: String,
    pub created_at: i64,
}

/// Aggregate counters used by the Settings → Memory panel.
/// `total` is the row count across all kinds; `by_kind` breaks that
/// down per `user / feedback / project / reference / note`. `db_bytes`
/// is the on-disk size of the main DB file (excludes WAL / SHM since
/// those are transient checkpoints). Best-effort: if any sub-query
/// fails we return zero for that field rather than erroring the whole
/// stats call, so the UI can still render partial info.
#[derive(Debug, Serialize, Default)]
pub struct MemoryStats {
    pub total: i64,
    pub by_kind: std::collections::BTreeMap<String, i64>,
    pub db_bytes: u64,
}

/// FTS5 search against the same memory.db the sidecar serves.
///
/// Sanitization matches the sidecar's strategy (`sanitize_fts5_query`
/// in `woom-memory/src/main.rs`): strip FTS5 metachars, split on
/// whitespace, re-emit each token as a `"phrase"` so a stray `:` or
/// `*` in the user query can't trip "no such column" errors. Returns
/// an empty Vec on null/empty post-sanitization input — caller can
/// treat that as "no recall" without special-casing.
///
/// `limit` is clamped to [1, 50] to match the sidecar's behaviour
/// and prevent unbounded scans on large stores.
#[tauri::command]
pub fn memory_search_local(
    query: String,
    limit: Option<u32>,
) -> Result<Vec<MemoryHit>, String> {
    let safe = sanitize_fts5_query(query.trim());
    if safe.is_empty() {
        return Ok(Vec::new());
    }
    let n = limit.unwrap_or(5).clamp(1, 50) as i64;
    let path = db_path()?;
    let conn = Connection::open(&path)
        .map_err(|e| format!("open {}: {}", path.display(), e))?;
    conn.busy_timeout(std::time::Duration::from_millis(2_000))
        .map_err(|e| format!("set busy_timeout: {e}"))?;
    let mut stmt = conn
        .prepare(
            "SELECT m.id, m.kind, m.content, m.tags, m.created_at \
             FROM memories m \
             JOIN memories_fts fts ON fts.rowid = m.id \
             WHERE memories_fts MATCH ?1 \
             ORDER BY bm25(memories_fts) \
             LIMIT ?2",
        )
        .map_err(|e| format!("prepare: {e}"))?;
    let rows = stmt
        .query_map(params![safe, n], |r| {
            Ok(MemoryHit {
                id: r.get(0)?,
                kind: r.get(1)?,
                content: r.get(2)?,
                tags: r.get(3)?,
                created_at: r.get(4)?,
            })
        })
        .map_err(|e| format!("query: {e}"))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("collect: {e}"))
}

/// List recent memories newest-first. Used by the Settings → Memory
/// browser to surface what's accumulated without forcing the user to
/// run a search query. Caps at 100 rows per call (matches the sidecar
/// limit) — pagination via `offset` for older entries.
#[tauri::command]
pub fn memory_list_local(
    kind: Option<String>,
    limit: Option<u32>,
    offset: Option<u32>,
) -> Result<Vec<MemoryHit>, String> {
    let n = limit.unwrap_or(50).clamp(1, 100) as i64;
    let off = offset.unwrap_or(0) as i64;
    let path = db_path()?;
    let conn = match Connection::open(&path) {
        Ok(c) => c,
        Err(_) => return Ok(Vec::new()),
    };
    let _ = conn.busy_timeout(std::time::Duration::from_millis(2_000));
    let kind_norm = match kind.as_deref().map(str::trim) {
        Some(k) if !k.is_empty() => {
            let lower = k.to_ascii_lowercase();
            match lower.as_str() {
                "user" | "feedback" | "project" | "reference" | "note" => Some(lower),
                _ => return Err(format!("invalid kind: {k}")),
            }
        }
        _ => None,
    };
    let rows = if let Some(k) = kind_norm {
        let mut stmt = conn
            .prepare(
                "SELECT id, kind, content, tags, created_at \
                 FROM memories WHERE kind = ?1 \
                 ORDER BY created_at DESC LIMIT ?2 OFFSET ?3",
            )
            .map_err(|e| format!("prepare list: {e}"))?;
        let iter = stmt
            .query_map(params![k, n, off], |r| {
                Ok(MemoryHit {
                    id: r.get(0)?,
                    kind: r.get(1)?,
                    content: r.get(2)?,
                    tags: r.get(3)?,
                    created_at: r.get(4)?,
                })
            })
            .map_err(|e| format!("query list: {e}"))?;
        iter.collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("collect list: {e}"))?
    } else {
        let mut stmt = conn
            .prepare(
                "SELECT id, kind, content, tags, created_at \
                 FROM memories ORDER BY created_at DESC LIMIT ?1 OFFSET ?2",
            )
            .map_err(|e| format!("prepare list: {e}"))?;
        let iter = stmt
            .query_map(params![n, off], |r| {
                Ok(MemoryHit {
                    id: r.get(0)?,
                    kind: r.get(1)?,
                    content: r.get(2)?,
                    tags: r.get(3)?,
                    created_at: r.get(4)?,
                })
            })
            .map_err(|e| format!("query list: {e}"))?;
        iter.collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("collect list: {e}"))?
    };
    Ok(rows)
}

/// Update an existing memory row's content (and optionally kind /
/// tags). Used by the Settings → Memory inline editor. Returns the
/// number of rows updated (0 if the id no longer exists). Bumps
/// `updated_at` to now so the row floats to the top of recency
/// orderings the next time the browser refreshes.
#[tauri::command]
pub fn memory_update_local(
    id: i64,
    content: String,
    kind: Option<String>,
    tags: Option<Vec<String>>,
) -> Result<usize, String> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return Err("content must not be empty".into());
    }
    let kind_norm = match kind.as_deref().map(str::trim) {
        Some(k) if !k.is_empty() => {
            let lower = k.to_ascii_lowercase();
            match lower.as_str() {
                "user" | "feedback" | "project" | "reference" | "note" => Some(lower),
                _ => return Err(format!("invalid kind: {k}")),
            }
        }
        _ => None,
    };
    let path = db_path()?;
    let conn = Connection::open(&path)
        .map_err(|e| format!("open {}: {}", path.display(), e))?;
    conn.busy_timeout(std::time::Duration::from_millis(2_000))
        .map_err(|e| format!("set busy_timeout: {e}"))?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    /* Build the UPDATE column list dynamically so omitted optional
     * fields don't get clobbered to defaults. Always update content +
     * updated_at; conditionally update kind / tags. */
    let n = match (kind_norm, tags) {
        (Some(k), Some(ts)) => {
            let tags_str = ts
                .into_iter()
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty())
                .collect::<Vec<_>>()
                .join(",");
            conn.execute(
                "UPDATE memories SET content = ?1, kind = ?2, tags = ?3, updated_at = ?4 WHERE id = ?5",
                params![trimmed, k, tags_str, now, id],
            )
        }
        (Some(k), None) => conn.execute(
            "UPDATE memories SET content = ?1, kind = ?2, updated_at = ?3 WHERE id = ?4",
            params![trimmed, k, now, id],
        ),
        (None, Some(ts)) => {
            let tags_str = ts
                .into_iter()
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty())
                .collect::<Vec<_>>()
                .join(",");
            conn.execute(
                "UPDATE memories SET content = ?1, tags = ?2, updated_at = ?3 WHERE id = ?4",
                params![trimmed, tags_str, now, id],
            )
        }
        (None, None) => conn.execute(
            "UPDATE memories SET content = ?1, updated_at = ?2 WHERE id = ?3",
            params![trimmed, now, id],
        ),
    }
    .map_err(|e| format!("update: {e}"))?;
    Ok(n)
}

/// Delete a single memory row by id. Used by Settings → Memory
/// browser's per-row delete button. Returns the number of rows
/// affected (0 when the id didn't exist; the UI can treat that as a
/// no-op rather than an error since the user-visible state is the
/// same as a successful delete).
#[tauri::command]
pub fn memory_delete_local(id: i64) -> Result<usize, String> {
    let path = db_path()?;
    let conn = Connection::open(&path)
        .map_err(|e| format!("open {}: {}", path.display(), e))?;
    conn.busy_timeout(std::time::Duration::from_millis(2_000))
        .map_err(|e| format!("set busy_timeout: {e}"))?;
    let n = conn
        .execute("DELETE FROM memories WHERE id = ?1", params![id])
        .map_err(|e| format!("delete: {e}"))?;
    Ok(n)
}

/// Per-session memory counts keyed by the 8-char session id prefix
/// (the tag form `from-session:<prefix>` written by autoDistillSession,
/// paste-trap, and the chat-message right-click "Save to memory"
/// action). Used by SessionsSidebar to render a "💾 N" badge next to
/// sessions that have associated long-term memory rows.
///
/// One Tauri call returns the full map so the sidebar doesn't pay
/// N queries when many sessions are visible. The map is small even
/// for power users (typically dozens of entries) — no pagination.
#[tauri::command]
pub fn memory_session_counts_local() -> Result<std::collections::BTreeMap<String, i64>, String> {
    let path = db_path()?;
    let mut out: std::collections::BTreeMap<String, i64> = Default::default();
    let conn = match Connection::open(&path) {
        Ok(c) => c,
        Err(_) => return Ok(out),
    };
    let _ = conn.busy_timeout(std::time::Duration::from_millis(2_000));
    /* Scan only rows whose tags string contains the marker — cheap
     * enough at this scale (memory table is typically <10k rows).
     * The exact prefix per row is extracted client-side below. */
    let mut stmt = match conn.prepare(
        "SELECT tags FROM memories WHERE tags LIKE '%from-session:%'",
    ) {
        Ok(s) => s,
        Err(_) => return Ok(out),
    };
    let rows = match stmt.query_map([], |r| r.get::<_, String>(0)) {
        Ok(it) => it,
        Err(_) => return Ok(out),
    };
    for row in rows {
        let tags = match row {
            Ok(s) => s,
            Err(_) => continue,
        };
        for tag in tags.split(',') {
            let t = tag.trim();
            if let Some(prefix) = t.strip_prefix("from-session:") {
                /* Guard against malformed tags that ate a trailing
                 * comma into the prefix. We only count alnum-ish
                 * prefixes (the session id-slice is base16-ish from
                 * genId) so noise doesn't bloat the map. */
                if !prefix.is_empty() && prefix.len() <= 32 {
                    *out.entry(prefix.to_string()).or_insert(0) += 1;
                }
            }
        }
    }
    Ok(out)
}

/// Read-only stats for the Settings → Memory panel.
#[tauri::command]
pub fn memory_stats_local() -> Result<MemoryStats, String> {
    let path = db_path()?;
    let db_bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    /* Open is fallible — DB may not exist yet on a brand-new install
     * (sidecar bootstraps it on first run). Return zeroes rather than
     * erroring so the panel renders "no memories yet" instead of a
     * scary error toast. */
    let conn = match Connection::open(&path) {
        Ok(c) => c,
        Err(_) => {
            return Ok(MemoryStats { total: 0, by_kind: Default::default(), db_bytes });
        }
    };
    let _ = conn.busy_timeout(std::time::Duration::from_millis(2_000));
    let total: i64 = conn
        .query_row("SELECT count(*) FROM memories", [], |r| r.get(0))
        .unwrap_or(0);
    let mut by_kind: std::collections::BTreeMap<String, i64> = Default::default();
    let _ = conn
        .prepare("SELECT kind, count(*) FROM memories GROUP BY kind")
        .and_then(|mut stmt| {
            let rows = stmt.query_map([], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
            })?;
            for row in rows {
                if let Ok((k, n)) = row {
                    by_kind.insert(k, n);
                }
            }
            Ok(())
        });
    Ok(MemoryStats { total, by_kind, db_bytes })
}

/// Copy of the sidecar's FTS5 sanitizer kept in sync by hand. Drift
/// here would resurrect the "no such column: 475" bug for whichever
/// surface uses the stale logic. If you change one, change the other
/// (woom-memory/src/main.rs).
fn sanitize_fts5_query(input: &str) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedup_finds_exact_repeat() {
        let s = "User prefers terse responses with no trailing summaries.";
        assert!(is_substring_dup(s, s));
    }

    #[test]
    fn dedup_finds_superset() {
        let a = "User prefers terse responses with no trailing summaries.";
        let b = "User prefers terse responses with no trailing summaries. Also no emojis.";
        assert!(is_substring_dup(a, b));
        assert!(is_substring_dup(b, a));
    }

    #[test]
    fn dedup_ignores_short_strings() {
        /* "/some/path" looks like a substring of "/some/path/sub" but
         * the shorter is under 40 chars — too generic to dedupe. */
        assert!(!is_substring_dup("/some/path", "/some/path/sub"));
    }

    #[test]
    fn dedup_normalizes_whitespace_and_case() {
        let a = "User PREFERS terse responses with NO trailing summaries.";
        let b = "user prefers terse  responses\nwith no trailing  summaries.";
        assert!(is_substring_dup(a, b));
    }

    #[test]
    fn dedup_rejects_unrelated_long_content() {
        let a = "User prefers terse responses with no trailing summaries.";
        let b = "Project FOO uses Postgres for the auth layer and Redis for caching.";
        assert!(!is_substring_dup(a, b));
    }

    #[test]
    fn sanitize_fts5_local_matches_sidecar() {
        /* Smoke-test the duplicated sanitizer to catch drift early. */
        assert_eq!(sanitize_fts5_query("foo:bar 475:"), "\"foobar\" \"475\"");
        assert_eq!(sanitize_fts5_query(":::"), "");
    }
}
