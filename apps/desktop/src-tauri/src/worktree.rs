//! Per-session git worktrees for Claude runs.
//!
//! Each Claude session can optionally run in its own isolated git worktree,
//! which lives under `~/Library/Application Support/Forgehold/worktrees/<session>/`.
//! This lets multiple agents work in parallel on the same repo without
//! trampling each other's in-progress changes, and keeps the user's main
//! working tree untouched — exactly what SPEC §worktrees mandates.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;

/// Storage root for all Forgehold-managed worktrees. We use macOS's Application
/// Support dir so the data is treated as app state (backed up, excluded from
/// Spotlight by default, stable across app updates).
pub fn storage_root() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    let root = Path::new(&home)
        .join("Library")
        .join("Application Support")
        .join("Forgehold")
        .join("worktrees");
    let _ = std::fs::create_dir_all(&root);
    Some(root)
}

#[derive(Debug, Serialize, Clone)]
pub struct Worktree {
    /// Filesystem path to the worktree.
    pub path: String,
    /// Current branch checked out in the worktree, if any.
    pub branch: Option<String>,
    /// Head SHA of the worktree.
    pub head: Option<String>,
    /// True if this is the repo's main working tree (not a secondary worktree).
    pub is_main: bool,
    /// If this worktree was created by Forgehold, the session id we attached to
    /// its branch name (`forgehold/<session>`). None for user-created worktrees.
    pub forgehold_session: Option<String>,
}

fn git(cwd: &str) -> Command {
    let mut c = Command::new("git");
    c.current_dir(cwd);
    c
}

fn run(mut cmd: Command) -> Result<String, String> {
    let out = cmd.output().map_err(|e| format!("git failed to spawn: {}", e))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        let code = out.status.code().unwrap_or(-1);
        return Err(if stderr.is_empty() {
            format!("git exited with code {}", code)
        } else {
            stderr
        });
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

/// Create a new worktree for a Claude session. The worktree branches off
/// `base_ref` (defaults to HEAD of the repo) and checks out a new branch
/// called `forgehold/<session_id>` inside a fresh directory under `storage_root`.
///
/// Idempotent: if a worktree for this session already exists, returns its
/// path without re-creating.
pub fn create(
    repo_path: &str,
    session_id: &str,
    base_ref: Option<&str>,
) -> Result<Worktree, String> {
    let root = storage_root().ok_or_else(|| "could not resolve $HOME".to_string())?;
    let path = root.join(session_id);
    let branch = forgehold_branch(session_id);

    // Already exists? Treat as success.
    if path.exists() {
        return inspect(&path).ok_or_else(|| "worktree dir exists but git doesn't know it".into());
    }

    let base = base_ref.unwrap_or("HEAD");
    let mut cmd = git(repo_path);
    cmd.args([
        "worktree",
        "add",
        "-b",
        &branch,
        path.to_string_lossy().as_ref(),
        base,
    ]);
    run(cmd)?;

    inspect(&path).ok_or_else(|| "git worktree add succeeded but inspect failed".into())
}

/// Remove a worktree and its branch. Force-remove is used so local edits
/// get discarded — callers must confirm with the user before invoking.
pub fn remove(repo_path: &str, session_id: &str) -> Result<(), String> {
    let root = storage_root().ok_or_else(|| "could not resolve $HOME".to_string())?;
    let path = root.join(session_id);
    let branch = forgehold_branch(session_id);

    if path.exists() {
        let mut cmd = git(repo_path);
        cmd.args([
            "worktree",
            "remove",
            "--force",
            path.to_string_lossy().as_ref(),
        ]);
        // Ignore not-a-worktree errors — we try to delete dir manually below.
        let _ = run(cmd);
        if path.exists() {
            std::fs::remove_dir_all(&path).map_err(|e| format!("rm -rf worktree: {}", e))?;
        }
    }

    // Delete the branch too (best-effort — may fail if merged/protected).
    let mut bcmd = git(repo_path);
    bcmd.args(["branch", "-D", &branch]);
    let _ = run(bcmd);

    Ok(())
}

/// List every worktree git knows about for a repo, annotated with our
/// `forgehold_session` tag if the branch name matches `forgehold/<session>`.
pub fn list(repo_path: &str) -> Result<Vec<Worktree>, String> {
    let mut cmd = git(repo_path);
    cmd.args(["worktree", "list", "--porcelain"]);
    let raw = run(cmd)?;
    let mut out: Vec<Worktree> = Vec::new();
    let mut path: Option<String> = None;
    let mut head: Option<String> = None;
    let mut branch: Option<String> = None;
    let mut is_main = false;
    let mut first = true;

    for line in raw.lines() {
        if line.is_empty() {
            if let Some(p) = path.take() {
                out.push(Worktree {
                    path: p,
                    branch: branch.take(),
                    head: head.take(),
                    is_main: std::mem::replace(&mut is_main, false),
                    forgehold_session: None,
                });
            }
            continue;
        }
        if let Some(p) = line.strip_prefix("worktree ") {
            path = Some(p.to_string());
            // The first block is the main working tree.
            if first {
                is_main = true;
                first = false;
            }
        } else if let Some(h) = line.strip_prefix("HEAD ") {
            head = Some(h.to_string());
        } else if let Some(b) = line.strip_prefix("branch ") {
            // `branch refs/heads/foo` → "foo"
            branch = Some(
                b.strip_prefix("refs/heads/").unwrap_or(b).to_string(),
            );
        }
    }
    // Tail flush if the trailing block wasn't followed by a blank line.
    if let Some(p) = path.take() {
        out.push(Worktree {
            path: p,
            branch: branch.take(),
            head: head.take(),
            is_main,
            forgehold_session: None,
        });
    }

    // Annotate forgehold_session from branch names.
    for w in out.iter_mut() {
        if let Some(b) = &w.branch {
            if let Some(session) = b.strip_prefix("forgehold/") {
                w.forgehold_session = Some(session.to_string());
            }
        }
    }

    Ok(out)
}

/// Re-inspect a worktree dir and return its state.
fn inspect(path: &Path) -> Option<Worktree> {
    let mut cmd = git(path.to_string_lossy().as_ref());
    cmd.args(["rev-parse", "HEAD"]);
    let head = run(cmd).ok().map(|s| s.trim().to_string());

    let mut branch_cmd = git(path.to_string_lossy().as_ref());
    branch_cmd.args(["branch", "--show-current"]);
    let branch = run(branch_cmd).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());

    let forgehold_session = branch
        .as_ref()
        .and_then(|b| b.strip_prefix("forgehold/").map(|s| s.to_string()));

    Some(Worktree {
        path: path.to_string_lossy().to_string(),
        branch,
        head,
        is_main: false,
        forgehold_session,
    })
}

#[derive(Debug, Serialize, Clone)]
pub struct WorktreeChangedFile {
    pub path: String,
    pub status: String, // M / A / D / R / T / ??
    pub additions: u32,
    pub deletions: u32,
}

/// Compare the worktree's branch against a base (defaults to the main repo's
/// current HEAD) and return a per-file summary plus the raw unified diff.
///
/// Uses `<base>...<branch>` (three-dot) so the diff reflects only commits
/// that are on the session's branch since it forked from the base — not
/// everything that happened on the base branch in parallel.
pub fn diff(
    repo: &str,
    session_id: &str,
    base_ref: Option<&str>,
) -> Result<(Vec<WorktreeChangedFile>, String), String> {
    let branch = forgehold_branch(session_id);
    let base = base_ref.unwrap_or("HEAD").to_string();
    let spec = format!("{}...{}", base, branch);

    // Per-file stats from --numstat
    let mut num_cmd = git(repo);
    num_cmd.args(["diff", "--numstat", &spec]);
    let numstat = run(num_cmd)?;

    // Status char per file from --name-status
    let mut st_cmd = git(repo);
    st_cmd.args(["diff", "--name-status", &spec]);
    let name_status = run(st_cmd)?;

    let mut files: Vec<WorktreeChangedFile> = Vec::new();
    for line in numstat.lines() {
        let parts: Vec<&str> = line.splitn(3, '\t').collect();
        if parts.len() < 3 { continue; }
        files.push(WorktreeChangedFile {
            path: parts[2].to_string(),
            status: String::new(),
            additions: parts[0].parse().unwrap_or(0),
            deletions: parts[1].parse().unwrap_or(0),
        });
    }
    for line in name_status.lines() {
        let parts: Vec<&str> = line.splitn(2, '\t').collect();
        if parts.len() < 2 { continue; }
        let st_char = parts[0].chars().next().unwrap_or('?').to_string();
        let path = parts[1].to_string();
        if let Some(f) = files.iter_mut().find(|f| f.path == path) {
            f.status = st_char;
        }
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));

    // Raw unified diff (3 lines of context, no-color, no-ext-diff for sanity).
    let mut diff_cmd = git(repo);
    diff_cmd.args(["diff", "--no-color", "--no-ext-diff", "-U3", &spec]);
    let raw = run(diff_cmd)?;

    Ok((files, raw))
}

/// Merge the session's branch into the currently-checked-out branch of the
/// main repo, then remove the worktree. If the merge fails (conflicts),
/// returns an error without removing — the user can fix the conflicts by
/// hand in the main repo or via the editor.
pub fn apply(repo: &str, session_id: &str) -> Result<String, String> {
    let branch = forgehold_branch(session_id);
    // Refuse to merge into the same branch the worktree owns (no-op / would
    // corrupt the worktree's HEAD).
    let mut cur_cmd = git(repo);
    cur_cmd.args(["branch", "--show-current"]);
    let current = run(cur_cmd)?.trim().to_string();
    if current == branch {
        return Err(format!(
            "Main repo is already on {} — switch to another branch before applying.",
            branch
        ));
    }

    let msg = format!("Merge {}\n\nForgehold session {}", branch, session_id);
    let mut merge_cmd = git(repo);
    merge_cmd.args(["merge", "--no-ff", "-m", &msg, &branch]);
    let merge_out = run(merge_cmd)?;

    // Now safe to tear down the worktree.
    remove(repo, session_id)?;

    Ok(format!("Merged {} into {}.\n\n{}", branch, current, merge_out.trim()))
}

/// Total bytes used by everything under `storage_root`. Walks the tree
/// once so big worktrees with `target/` and `node_modules/` are honestly
/// counted. Returns 0 if the dir doesn't exist yet.
pub fn disk_usage_bytes() -> u64 {
    let Some(root) = storage_root() else { return 0 };
    fn walk(p: &Path) -> u64 {
        let mut total = 0u64;
        let Ok(entries) = std::fs::read_dir(p) else { return 0 };
        for entry in entries.flatten() {
            let Ok(meta) = entry.metadata() else { continue };
            if meta.is_dir() {
                total = total.saturating_add(walk(&entry.path()));
            } else {
                total = total.saturating_add(meta.len());
            }
        }
        total
    }
    walk(&root)
}

#[derive(Debug, Serialize, Clone)]
pub struct CleanupSummary {
    pub removed: u32,
    pub bytes_freed: u64,
    pub kept: u32,
    pub failed: Vec<String>,
}

/// Remove worktree directories under `storage_root` that:
///   1. Don't correspond to any session in `active_session_ids`, AND
///   2. Were last modified more than `max_age_secs` ago.
///
/// Older orphans are typical when the user deleted a chat without first
/// applying or removing its worktree. The 14-day default gives a safety
/// window in case they want to recover.
///
/// We delete via `rm -rf` of the directory because the parent repo path
/// is unknown to us at this point — `git worktree prune` in the parent
/// repo will reap the stale refs the next time the user works there.
pub fn cleanup_orphans(active_session_ids: &[String], max_age_secs: u64) -> CleanupSummary {
    let mut summary = CleanupSummary {
        removed: 0,
        bytes_freed: 0,
        kept: 0,
        failed: Vec::new(),
    };
    let Some(root) = storage_root() else { return summary };
    let Ok(entries) = std::fs::read_dir(&root) else { return summary };
    let active: std::collections::HashSet<&str> =
        active_session_ids.iter().map(|s| s.as_str()).collect();
    let now = std::time::SystemTime::now();

    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = entry.metadata() else { continue };
        if !meta.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else { continue };
        if active.contains(name) {
            summary.kept += 1;
            continue;
        }
        // Age check — use mtime as a proxy for "user touched this recently".
        let age_secs = meta
            .modified()
            .ok()
            .and_then(|m| now.duration_since(m).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        if age_secs < max_age_secs {
            summary.kept += 1;
            continue;
        }
        let size = walk_size(&path);
        match std::fs::remove_dir_all(&path) {
            Ok(()) => {
                summary.removed += 1;
                summary.bytes_freed = summary.bytes_freed.saturating_add(size);
            }
            Err(e) => {
                summary.failed.push(format!("{}: {}", name, e));
            }
        }
    }
    summary
}

fn walk_size(p: &Path) -> u64 {
    let mut total = 0u64;
    let Ok(entries) = std::fs::read_dir(p) else { return 0 };
    for entry in entries.flatten() {
        let Ok(meta) = entry.metadata() else { continue };
        if meta.is_dir() {
            total = total.saturating_add(walk_size(&entry.path()));
        } else {
            total = total.saturating_add(meta.len());
        }
    }
    total
}

fn forgehold_branch(session_id: &str) -> String {
    // Sanitize to a valid git ref. Git refs can't contain spaces, `..`, `~`,
    // `^`, `:`, `?`, `*`, `[`, `\`, or control chars.
    let safe: String = session_id
        .chars()
        .map(|c| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' | '/' => c,
            _ => '-',
        })
        .collect();
    format!("forgehold/{}", safe)
}
