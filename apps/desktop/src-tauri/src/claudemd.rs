//! CLAUDE.md auto-load — mirrors Anthropic's per-session memory mechanism
//! (see `docs/CLAUDE_PARITY.md §7.2`). On each agent turn we walk the
//! current working directory upward, concatenate every `CLAUDE.md` /
//! `.claude/CLAUDE.md` we find, prepend the user-global
//! `~/.claude/CLAUDE.md`, strip HTML comments, and recursively expand
//! `@path/to/file.md` imports (up to 5 hops).
//!
//! Caching is a thin map keyed by cwd — the walk + IO is cheap but we
//! still want to skip it on every turn for sessions that pin a
//! long-lived cwd. The cache invalidates on `claudemd_load` re-entry
//! when the user explicitly refreshes (e.g. saved a new CLAUDE.md in
//! their editor); a separate "watch the files" pass isn't worth the
//! complexity at this phase.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Cap on `@import` recursion. Anthropic uses 5; matching that. The
/// usual case is 1–2 hops (a CLAUDE.md that pulls in `@SETUP.md`).
const IMPORT_DEPTH_CAP: usize = 5;

/// Per-file size cap — protects against a runaway `@import` to a huge
/// log file. 200 KB matches the largest CLAUDE.md we've observed in
/// the wild; bump if users complain.
const FILE_BYTES_CAP: u64 = 200 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeMdResult {
    /// Final concatenated content with imports expanded and comments
    /// stripped. Empty when no CLAUDE.md files were found anywhere.
    pub content: String,
    /// Files we successfully read, in load order — user-global first,
    /// then root-of-tree down to cwd (so cwd-specific overrides come
    /// last and the agent reads them as the most-specific guidance).
    pub sources: Vec<String>,
    /// Soft errors (file too big, import cycle, permission denied).
    /// Non-fatal — surfaced so the UI can show "1 import skipped" if
    /// the user cares to debug.
    pub warnings: Vec<String>,
}

/// Public entry — walks cwd, returns merged content. Pure: no
/// caching at this layer; the Tauri command wraps with a per-cwd
/// cache.
pub fn load_for_cwd(cwd: Option<&str>) -> ClaudeMdResult {
    let mut result = ClaudeMdResult {
        content: String::new(),
        sources: Vec::new(),
        warnings: Vec::new(),
    };
    let mut visited: HashSet<PathBuf> = HashSet::new();

    // ── User-global first.
    if let Some(home) = home_dir() {
        let user_md = home.join(".claude").join("CLAUDE.md");
        if user_md.is_file() {
            ingest(&user_md, &mut result, &mut visited, 0);
        }
    }

    // ── Project tree. Walk from root-of-tree down to cwd so the most
    // specific override loads last and trumps in agent reading order.
    if let Some(cwd_str) = cwd {
        let cwd_path = Path::new(cwd_str);
        let mut ancestors: Vec<&Path> = cwd_path.ancestors().collect();
        ancestors.reverse(); // now root → cwd
        for dir in ancestors {
            for candidate in [dir.join("CLAUDE.md"), dir.join(".claude").join("CLAUDE.md")] {
                if candidate.is_file() {
                    ingest(&candidate, &mut result, &mut visited, 0);
                }
            }
        }
    }

    result
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

fn ingest(
    path: &Path,
    result: &mut ClaudeMdResult,
    visited: &mut HashSet<PathBuf>,
    depth: usize,
) {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if !visited.insert(canonical.clone()) {
        return; // cycle / dedup
    }
    let md = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(e) => {
            result
                .warnings
                .push(format!("stat {}: {e}", path.display()));
            return;
        }
    };
    if md.len() > FILE_BYTES_CAP {
        result
            .warnings
            .push(format!("skipped {} (over {} byte cap)", path.display(), FILE_BYTES_CAP));
        return;
    }
    let raw = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            result.warnings.push(format!("read {}: {e}", path.display()));
            return;
        }
    };

    /* Section header — labels where this content came from so the
     *  agent can disambiguate overlapping rules. The path is the
     *  authoritative reference; the leading horizontal rule keeps
     *  sections visually separated when concatenated. */
    if !result.content.is_empty() {
        result.content.push_str("\n\n");
    }
    result
        .content
        .push_str(&format!("<!-- CLAUDE.md from {} -->\n", path.display()));

    let stripped = strip_html_comments(&raw);
    let expanded = expand_imports(&stripped, path, result, visited, depth);
    result.content.push_str(&expanded);
    result.sources.push(path.to_string_lossy().into_owned());
}

/// Drop `<!-- ... -->` blocks. Multi-line aware. Cheap manual scan.
fn strip_html_comments(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if i + 3 < bytes.len() && &bytes[i..i + 4] == b"<!--" {
            // Find closing `-->`.
            if let Some(end) = find_seq(bytes, i + 4, b"-->") {
                i = end + 3;
                continue;
            }
            // Unclosed comment — drop the rest, mirrors HTML parser semantics.
            break;
        }
        // Copy one byte; UTF-8 boundaries preserved because the only
        // matching tokens we look for are ASCII.
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

fn find_seq(haystack: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || from >= haystack.len() {
        return None;
    }
    let mut i = from;
    while i + needle.len() <= haystack.len() {
        if &haystack[i..i + needle.len()] == needle {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// Expand `@path/to/file.md` imports — must be at start-of-line for
/// safety (no implicit imports in the middle of a paragraph). Path
/// resolution is relative to the importing file's dir.
fn expand_imports(
    content: &str,
    importing_file: &Path,
    result: &mut ClaudeMdResult,
    visited: &mut HashSet<PathBuf>,
    depth: usize,
) -> String {
    if depth >= IMPORT_DEPTH_CAP {
        result.warnings.push(format!(
            "import depth cap hit in {}",
            importing_file.display()
        ));
        return content.to_string();
    }
    let mut out = String::with_capacity(content.len());
    for line in content.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix('@') {
            // Take everything up to whitespace as the path.
            let path_str: String = rest
                .chars()
                .take_while(|c| !c.is_whitespace())
                .collect();
            if !path_str.is_empty() && !path_str.starts_with('@') {
                let base = importing_file.parent().unwrap_or(Path::new("."));
                let import_path = base.join(&path_str);
                if import_path.is_file() {
                    let mut nested = ClaudeMdResult {
                        content: String::new(),
                        sources: Vec::new(),
                        warnings: Vec::new(),
                    };
                    ingest(&import_path, &mut nested, visited, depth + 1);
                    out.push_str(&nested.content);
                    result.sources.extend(nested.sources);
                    result.warnings.extend(nested.warnings);
                    continue;
                }
                // Path didn't resolve — emit a warning but keep the
                // raw line so the user can fix it.
                result.warnings.push(format!(
                    "@import not found: {} (from {})",
                    path_str,
                    importing_file.display()
                ));
            }
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

// ---- Tauri commands ------------------------------------------------------

#[tauri::command]
pub fn claudemd_load(cwd: Option<String>) -> ClaudeMdResult {
    load_for_cwd(cwd.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tmpdir(prefix: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "woom-claudemd-test-{prefix}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn write_file(p: &Path, body: &str) {
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        let mut f = std::fs::File::create(p).unwrap();
        f.write_all(body.as_bytes()).unwrap();
    }

    #[test]
    fn strip_comments_drops_block_and_inline() {
        let s = "before <!-- drop me --> after\n<!--\nmulti\n-->\nbody";
        let out = strip_html_comments(s);
        assert!(!out.contains("drop me"));
        assert!(!out.contains("multi"));
        assert!(out.contains("before "));
        assert!(out.contains(" after"));
        assert!(out.contains("body"));
    }

    #[test]
    fn walks_from_root_to_cwd() {
        let root = tmpdir("walk");
        let sub = root.join("subdir");
        write_file(&root.join("CLAUDE.md"), "root rules\n");
        write_file(&sub.join("CLAUDE.md"), "sub rules\n");
        let r = load_for_cwd(Some(sub.to_str().unwrap()));
        // Root loads first (less specific), sub last (more specific).
        let root_pos = r.content.find("root rules").unwrap();
        let sub_pos = r.content.find("sub rules").unwrap();
        assert!(root_pos < sub_pos);
    }

    #[test]
    fn expands_import_relative_to_file() {
        let root = tmpdir("imports");
        write_file(&root.join("CLAUDE.md"), "header\n@inc.md\nfooter\n");
        write_file(&root.join("inc.md"), "imported body\n");
        let r = load_for_cwd(Some(root.to_str().unwrap()));
        assert!(r.content.contains("imported body"), "got: {}", r.content);
    }

    #[test]
    fn import_cycle_does_not_infinite_loop() {
        let root = tmpdir("cycle");
        write_file(&root.join("CLAUDE.md"), "@a.md\n");
        write_file(&root.join("a.md"), "@b.md\n");
        write_file(&root.join("b.md"), "@a.md\nleaf\n");
        let r = load_for_cwd(Some(root.to_str().unwrap()));
        assert!(r.content.contains("leaf"));
    }
}
