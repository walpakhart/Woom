//! Filesystem ops exposed to the frontend editor.
//!
//! These are intentionally thin wrappers around `std::fs`. The frontend
//! editor drives path choice (file picker dialog, file tree clicks), so we
//! trust absolute paths passed in. If we add untrusted callers later,
//! constrain paths to registered Repository roots from REPOS.md §2.

use std::path::PathBuf;

use serde::Serialize;

#[derive(Debug, Serialize, Clone)]
pub struct DirEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
}

pub fn read_file(path: &str) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("read {}: {}", path, e))
}

pub fn write_file(path: &str, contents: &str) -> Result<(), String> {
    if let Some(parent) = PathBuf::from(path).parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("mkdir -p {}: {}", parent.display(), e))?;
        }
    }
    write_atomic(std::path::Path::new(path), contents.as_bytes())
        .map_err(|e| format!("write {}: {}", path, e))
}

/// Binary write — for chat image attachments dropped from clipboard / Cmd+Shift+5
/// floating preview where we have only the byte buffer (no source file path).
/// Caller picks the destination; we just create parent dirs and dump the bytes.
pub fn write_bytes(path: &str, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = PathBuf::from(path).parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("mkdir -p {}: {}", parent.display(), e))?;
        }
    }
    write_atomic(std::path::Path::new(path), bytes)
        .map_err(|e| format!("write {}: {}", path, e))
}

/// Write `bytes` to `target` durably: tempfile in the same directory,
/// `fsync`, then rename over the destination. On crash before rename
/// the destination keeps its previous contents (or stays absent);
/// after rename, the new contents are guaranteed to be on disk.
///
/// Same-directory tempfile is intentional — `rename(2)` is atomic only
/// when source and destination share a filesystem. Using `/tmp` would
/// silently degrade to copy+unlink on a different mount.
///
/// On rename failure the tempfile is cleaned up so retries don't leave
/// litter. If the parent directory is read-only or full this still
/// surfaces the error to the caller; we don't fall back to a non-atomic
/// write because that would defeat the durability promise.
fn write_atomic(target: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let parent = target
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| std::path::Path::new("."));
    let file_name = target
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("woom-tmp");
    /* PID + nanos suffix is collision-proof enough — only this process
     * writes here, and within a process nanos monotonically advances
     * between two `SystemTime::now()` calls on every platform Tauri
     * targets. We don't need crypto-grade randomness. */
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp_name = format!(".{}.{}.{}.tmp", file_name, std::process::id(), nanos);
    let tmp_path = parent.join(tmp_name);
    {
        let mut f = std::fs::File::create(&tmp_path)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    if let Err(e) = std::fs::rename(&tmp_path, target) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(e);
    }
    Ok(())
}

pub fn list_dir(path: &str) -> Result<Vec<DirEntry>, String> {
    let rd = std::fs::read_dir(path).map_err(|e| format!("read_dir {}: {}", path, e))?;
    let mut out: Vec<DirEntry> = Vec::new();
    for entry in rd.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        // Skip .git and other VCS internals from the tree; users can still
        // open individual files via the file picker if they need to.
        if name == ".git" || name == ".DS_Store" {
            continue;
        }
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        out.push(DirEntry {
            name,
            path: entry.path().to_string_lossy().to_string(),
            is_dir: meta.is_dir(),
            size: if meta.is_file() { meta.len() } else { 0 },
        });
    }
    // Directories first, then alphabetical within each group.
    out.sort_by(|a, b| match (a.is_dir, b.is_dir) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
    });
    Ok(out)
}

pub fn path_exists(path: &str) -> bool {
    std::path::Path::new(path).exists()
}

/// Bounded recursive file walk for the @-mention picker. We BFS the
/// repo, skip the usual VCS/build noise (`.git`, `node_modules`,
/// `target`, `dist`, …), respect a hard cap on returned files, and
/// optionally filter by case-insensitive substring on the leaf
/// filename. Returns repo-relative paths so the caller can render
/// them straight into the picker without any local stripping.
///
/// Caps default to depth=8, files=2000 — enough for any reasonable
/// project, fast enough to feel instant. The caller is expected to
/// debounce calls per-keystroke.
pub fn walk_files(
    root: &str,
    query: Option<&str>,
    max_files: usize,
    max_depth: usize,
) -> Result<Vec<DirEntry>, String> {
    use std::collections::VecDeque;
    let root_path = std::path::Path::new(root);
    if !root_path.is_dir() {
        return Err(format!("not a directory: {}", root));
    }
    /* Common build / VCS noise. We skip these aggressively rather
       than respect .gitignore — the picker needs sub-50ms latency
       on multi-thousand-file repos. */
    const SKIP_DIRS: &[&str] = &[
        ".git", "node_modules", ".next", ".nuxt", "dist", "build",
        "out", "target", ".turbo", ".cache", ".parcel-cache",
        ".svelte-kit", ".vercel", ".pnpm-store", "vendor", ".idea",
        ".vscode", ".angular", ".nx", ".localstack", "coverage", ".DS_Store"
    ];
    let q = query.map(|s| s.to_lowercase());
    let mut out: Vec<DirEntry> = Vec::new();
    let mut queue: VecDeque<(std::path::PathBuf, usize)> = VecDeque::new();
    queue.push_back((root_path.to_path_buf(), 0));

    while let Some((dir, depth)) = queue.pop_front() {
        if out.len() >= max_files {
            break;
        }
        let rd = match std::fs::read_dir(&dir) {
            Ok(r) => r,
            Err(_) => continue,
        };
        for entry in rd.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') && SKIP_DIRS.contains(&name.as_str()) {
                continue;
            }
            if SKIP_DIRS.contains(&name.as_str()) {
                continue;
            }
            let path = entry.path();
            let meta = match entry.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };
            if meta.is_dir() {
                if depth + 1 < max_depth {
                    queue.push_back((path, depth + 1));
                }
                continue;
            }
            /* Filter by query against the leaf filename. Empty query
               means "list everything up to the cap" — the picker uses
               that to populate a project-wide "Files" header. */
            if let Some(ref needle) = q {
                if !name.to_lowercase().contains(needle) {
                    continue;
                }
            }
            out.push(DirEntry {
                name,
                path: path.to_string_lossy().to_string(),
                is_dir: false,
                size: meta.len(),
            });
            if out.len() >= max_files {
                break;
            }
        }
    }
    /* Shorter paths first, then alphabetical — matches what users
       expect from a fuzzy file picker (root README beats some
       deeply-nested test fixture with the same prefix). */
    out.sort_by(|a, b| {
        let depth_a = a.path.matches('/').count();
        let depth_b = b.path.matches('/').count();
        depth_a
            .cmp(&depth_b)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(out)
}

/// Single hit produced by `search_text` — one match within a single
/// line of a single text file. `byte_offset` is the absolute byte
/// offset of the match's first byte within the file (CodeMirror's
/// `from`); `line_byte_offset` is the byte offset of the line's first
/// byte (used by the caller to compute approximate scroll target).
/// `preview` is the matching line's content, trimmed if very long.
#[derive(Debug, Serialize, Clone)]
pub struct TextMatch {
    pub path: String,
    pub line: u32,
    pub col: u32,
    pub byte_offset: u64,
    pub line_byte_offset: u64,
    pub match_len: u32,
    pub preview: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct SearchTextResult {
    /// Up to `max_results` matches, ordered by walk order (BFS by dir
    /// depth, alphabetical within a dir). Hitting the cap is reported
    /// via `truncated = true` so the UI can render "+ more" hints.
    pub matches: Vec<TextMatch>,
    pub truncated: bool,
    /// Files actually scanned — useful for "Searched N files" footer.
    pub files_scanned: u32,
}

/// Project-wide grep used by the Editor's ⌘⇧F overlay. Plain
/// case-insensitive substring search; no regex / glob filters in v1
/// (matches the cut from `ROADMAP_1.0.md §2.1`).
///
/// Heuristics:
///   * Skip noisy dirs (same list as `walk_files` — node_modules,
///     target, .git, etc.) so a search in a monorepo doesn't read
///     50k vendor files.
///   * Skip files > 1 MiB and files whose first 8 KiB contain a null
///     byte (rough binary heuristic).
///   * Cap matches per file (50) so one mega-line-count file doesn't
///     dominate the result list.
///   * Cap total matches (`max_results`); set `truncated` on overflow.
///   * Depth-limit at 12 — deep enough for real projects, shallow
///     enough to keep stalls bounded.
///
/// The query is treated as a literal string (lowercased on both
/// sides). Empty / whitespace-only queries return an empty result.
pub fn search_text(
    root: &str,
    query: &str,
    max_results: usize,
) -> Result<SearchTextResult, String> {
    use std::collections::VecDeque;
    use std::io::Read;
    let q = query.trim();
    if q.is_empty() {
        return Ok(SearchTextResult {
            matches: Vec::new(),
            truncated: false,
            files_scanned: 0,
        });
    }
    let root_path = std::path::Path::new(root);
    if !root_path.is_dir() {
        return Err(format!("not a directory: {}", root));
    }
    const SKIP_DIRS: &[&str] = &[
        ".git", "node_modules", ".next", ".nuxt", "dist", "build", "out",
        "target", ".turbo", ".cache", ".parcel-cache", ".svelte-kit",
        ".vercel", ".pnpm-store", "vendor", ".idea", ".vscode", ".angular",
        ".nx", ".localstack", "coverage", ".DS_Store",
    ];
    const MAX_FILE_BYTES: u64 = 1024 * 1024; // 1 MiB
    const BINARY_PROBE_BYTES: usize = 8 * 1024;
    const MAX_MATCHES_PER_FILE: usize = 50;
    const MAX_DEPTH: usize = 12;
    const PREVIEW_MAX_LEN: usize = 240;

    let needle = q.to_lowercase();
    let mut matches: Vec<TextMatch> = Vec::new();
    let mut files_scanned: u32 = 0;
    let mut truncated = false;
    let mut queue: VecDeque<(std::path::PathBuf, usize)> = VecDeque::new();
    queue.push_back((root_path.to_path_buf(), 0));

    'walk: while let Some((dir, depth)) = queue.pop_front() {
        let rd = match std::fs::read_dir(&dir) {
            Ok(r) => r,
            Err(_) => continue,
        };
        // Collect entries first so we can sort within a directory —
        // gives the user stable, alphabetical output for grouped
        // results.
        let mut entries: Vec<std::fs::DirEntry> = rd.flatten().collect();
        entries.sort_by_key(|e| e.file_name());

        for entry in entries {
            if matches.len() >= max_results {
                truncated = true;
                break 'walk;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if SKIP_DIRS.contains(&name.as_str()) {
                continue;
            }
            let path = entry.path();
            let meta = match entry.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };
            if meta.is_dir() {
                if depth + 1 < MAX_DEPTH {
                    queue.push_back((path, depth + 1));
                }
                continue;
            }
            if !meta.is_file() {
                continue;
            }
            if meta.len() > MAX_FILE_BYTES {
                continue;
            }

            // Binary heuristic: peek at the first 8 KiB for a NUL byte.
            let mut file = match std::fs::File::open(&path) {
                Ok(f) => f,
                Err(_) => continue,
            };
            let mut probe = vec![0u8; BINARY_PROBE_BYTES.min(meta.len() as usize)];
            let probed = match file.read(&mut probe) {
                Ok(n) => n,
                Err(_) => continue,
            };
            if probe[..probed].contains(&0u8) {
                continue;
            }
            // Re-read full contents as UTF-8 (lossy — keeps weirdly-
            // encoded but mostly-ASCII files searchable instead of
            // dropping them entirely).
            let raw = match std::fs::read(&path) {
                Ok(b) => b,
                Err(_) => continue,
            };
            let contents = String::from_utf8_lossy(&raw);
            files_scanned += 1;

            let mut per_file = 0usize;
            let mut line_byte_offset: u64 = 0;
            for (line_idx, line) in contents.split('\n').enumerate() {
                let lower = line.to_lowercase();
                let mut search_from = 0usize;
                while let Some(rel) = lower[search_from..].find(&needle) {
                    let col_bytes = search_from + rel;
                    let abs_offset = line_byte_offset + col_bytes as u64;
                    // Trim the preview around the match — long minified
                    // lines would otherwise blow up the response size.
                    let preview = trim_preview(line, col_bytes, PREVIEW_MAX_LEN);
                    matches.push(TextMatch {
                        path: path.to_string_lossy().to_string(),
                        line: (line_idx + 1) as u32,
                        col: (col_bytes + 1) as u32,
                        byte_offset: abs_offset,
                        line_byte_offset,
                        match_len: needle.len() as u32,
                        preview,
                    });
                    per_file += 1;
                    if matches.len() >= max_results {
                        truncated = true;
                        break 'walk;
                    }
                    if per_file >= MAX_MATCHES_PER_FILE {
                        break;
                    }
                    search_from = col_bytes + needle.len();
                    if search_from >= line.len() {
                        break;
                    }
                }
                if per_file >= MAX_MATCHES_PER_FILE {
                    break;
                }
                // +1 for the newline byte we split on.
                line_byte_offset += line.len() as u64 + 1;
            }
        }
    }

    Ok(SearchTextResult {
        matches,
        truncated,
        files_scanned,
    })
}

/// Trim a preview line around the match column. We pad on both sides
/// up to `max_len`; if the line itself is short enough, return it
/// untouched. Adds Unicode ellipsis at the truncation boundary so the
/// UI can render the snippet without measuring overflow.
fn trim_preview(line: &str, match_col_bytes: usize, max_len: usize) -> String {
    let chars: Vec<char> = line.chars().collect();
    if chars.len() <= max_len {
        return line.to_string();
    }
    // Translate byte-col → char-col so the trim window centres on the
    // hit even when the line has multi-byte runs.
    let char_col = line[..match_col_bytes.min(line.len())].chars().count();
    let half = max_len / 2;
    let start = char_col.saturating_sub(half);
    let end = (start + max_len).min(chars.len());
    let mut out = String::new();
    if start > 0 {
        out.push('…');
    }
    out.extend(chars[start..end].iter());
    if end < chars.len() {
        out.push('…');
    }
    out
}

/// Delete a single file. Used for canvas-on-disk garbage collection
/// and similar straightforward removals. Idempotent on missing
/// files — no error when the path is already gone, since the
/// operational intent ("ensure the file isn't there") is satisfied.
pub fn remove_file_if_exists(path: &str) -> Result<(), String> {
    let p = std::path::Path::new(path);
    if !p.exists() {
        return Ok(());
    }
    std::fs::remove_file(p).map_err(|e| format!("remove {}: {}", path, e))
}

/// Recursively delete a directory and everything inside it. Used by
/// the FileTree right-click "Delete folder" affordance. Refuses to:
///   - delete a missing path (caller bug — surface the error)
///   - delete a path that isn't a directory (use `remove_file_if_exists`)
///   - cross filesystem roots ("/", "/Users", "/Users/<name>") which
///     would be a catastrophe on a mistaken click
///
/// The depth-of-3 root guard is conservative — the user can still
/// delete `~/Repos/pers/woom` (4 deep) but not `~/` itself, which is
/// the right asymmetry for an explorer feature.
pub fn remove_dir_recursive(path: &str) -> Result<(), String> {
    let p = std::path::Path::new(path);
    if !p.exists() {
        return Err(format!("path {path} does not exist"));
    }
    if !p.is_dir() {
        return Err(format!("path {path} is not a directory"));
    }
    /* Refuse to delete the root or extremely shallow paths. Counting
       components on an absolute path: `/` → 1, `/Users` → 2,
       `/Users/foo` → 3. We require at least 4 (e.g. `/Users/foo/x`)
       so a stray click from the explorer can't wipe a home folder. */
    let depth = p.components().count();
    if depth < 4 {
        return Err(format!(
            "refusing to delete {path}: path is too shallow (depth {depth} < 4) — use Finder for system folders"
        ));
    }
    std::fs::remove_dir_all(p).map_err(|e| format!("remove_dir {path}: {e}"))
}

#[derive(Debug, Serialize, Clone)]
pub struct BashResult {
    pub stdout: String,
    pub stderr: String,
    pub code: i32,
    pub ok: bool,
}

/// Hard upper bound for `bash_run`. Approval-card commands (commit,
/// push, install, migration) can legitimately take minutes — 60s
/// was killing valid `git push` and `npm install` runs mid-flight,
/// which the user saw as "card hangs and freezes the UI" (the JS
/// invoke() future stays pending until the kill). 10 minutes covers
/// every realistic interactive workflow without letting a runaway
/// process eat a thread forever.
pub const BASH_RUN_TIMEOUT_SECS: u64 = 600;

/// Run a shell command in `cwd` via `sh -c`. Captures stdout and stderr
/// separately and returns the exit code. Never panics; command failures
/// are reflected in `ok=false, code≠0`.
///
/// Async + tokio-based: we previously had a sync sibling that
/// busy-polled `try_wait()` every 50 ms on a Tauri worker thread. That
/// pinned a whole Tauri command thread for the duration of the call —
/// fire 4 approve-cards in parallel and the IPC queue stalled, which
/// users saw as the entire app lagging. Switching to tokio's process
/// API means the future yields cleanly between OS notifications and
/// the JS side's pending `invoke()` future never blocks anything.
/// `kill_on_drop` guarantees that timeout / cancellation leaves no
/// orphaned children behind.
pub async fn bash_run(cwd: &str, command: &str) -> Result<BashResult, String> {
    use std::process::Stdio;
    use std::time::Duration;
    use tokio::process::Command;
    use tokio::time::timeout;

    let child = Command::new("sh")
        .arg("-c")
        .arg(command)
        .current_dir(cwd)
        .env("PATH", enriched_path())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| format!("spawn sh: {}", e))?;

    match timeout(
        Duration::from_secs(BASH_RUN_TIMEOUT_SECS),
        child.wait_with_output(),
    )
    .await
    {
        Ok(Ok(out)) => Ok(BashResult {
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
            code: out.status.code().unwrap_or(-1),
            ok: out.status.success(),
        }),
        Ok(Err(e)) => Err(format!("wait: {}", e)),
        // Future drop → `kill_on_drop` reaps the child. Surface the
        // partial timeout fact so the agent / user know it wasn't a
        // success-with-empty-output.
        Err(_) => Err(format!(
            "command timed out after {}s",
            BASH_RUN_TIMEOUT_SECS
        )),
    }
}

fn enriched_path() -> String {
    // Same trick as claude.rs — Tauri apps launched from Finder don't
    // inherit the user's shell PATH, so gh/brew/pyenv binaries wouldn't
    // be found. Augment with the common locations.
    let base = std::env::var("PATH").unwrap_or_default();
    let extras = [
        "/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin",
    ];
    let mut parts: Vec<&str> = base.split(':').collect();
    for e in extras {
        if !parts.contains(&e) {
            parts.push(e);
        }
    }
    parts.join(":")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tempdir helper so tests stay isolated from each other and from
    /// real user files. Uses the process id + nanos so concurrent
    /// `cargo test` runs don't collide.
    fn tempdir() -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!(
            "woom-fs-test-{}-{}",
            std::process::id(),
            nanos
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn write_file_round_trips() {
        let dir = tempdir();
        let p = dir.join("a.json");
        write_file(p.to_str().unwrap(), "{\"x\":1}").unwrap();
        let got = std::fs::read_to_string(&p).unwrap();
        assert_eq!(got, "{\"x\":1}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn write_file_overwrites_existing() {
        let dir = tempdir();
        let p = dir.join("a.json");
        write_file(p.to_str().unwrap(), "first").unwrap();
        write_file(p.to_str().unwrap(), "second").unwrap();
        let got = std::fs::read_to_string(&p).unwrap();
        assert_eq!(got, "second");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn write_file_leaves_no_tempfile_on_success() {
        /* After a successful write the only file in the directory
         * should be the target. If the tempfile path leaks (rename
         * failure path forgot to clean up, or tempfile naming
         * collided with target) we'd see two files here. */
        let dir = tempdir();
        let p = dir.join("a.json");
        write_file(p.to_str().unwrap(), "hello").unwrap();
        let entries: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().collect();
        assert_eq!(entries.len(), 1);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn write_file_creates_missing_parent() {
        let dir = tempdir();
        let p = dir.join("nested/deep/a.json");
        write_file(p.to_str().unwrap(), "x").unwrap();
        assert!(p.exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn write_bytes_handles_binary() {
        let dir = tempdir();
        let p = dir.join("img.bin");
        let bytes: Vec<u8> = vec![0, 1, 2, 255, 0, 0, 13, 10];
        write_bytes(p.to_str().unwrap(), &bytes).unwrap();
        let got = std::fs::read(&p).unwrap();
        assert_eq!(got, bytes);
        std::fs::remove_dir_all(&dir).ok();
    }
}
