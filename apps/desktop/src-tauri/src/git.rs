//! Git operations — thin wrappers around the `git` CLI on PATH.
//!
//! We shell out rather than using libgit2 because (a) `git` is universally
//! available on macOS dev machines, (b) it handles credential-helper /
//! SSH-agent / signing transparently, and (c) bug-for-bug compat with what
//! the user sees in their terminal.
//!
//! All commands take an absolute `repo` path (the repository working tree)
//! and set `current_dir` accordingly. They never touch paths outside the
//! repo.

use std::process::Command;

use serde::Serialize;

#[derive(Debug, Serialize, Clone)]
pub struct FileStatus {
    /// Path relative to the repo root.
    pub path: String,
    /// Two-char `git status --porcelain` code: index + worktree.
    pub code: String,
    /// True iff at least one of the two status chars is a staged modification
    /// (anything other than ' ' or '?').
    pub staged: bool,
    /// True iff the worktree side indicates a change.
    pub unstaged: bool,
}

#[derive(Debug, Serialize, Clone)]
pub struct GitStatus {
    pub branch: Option<String>,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub files: Vec<FileStatus>,
}

#[derive(Debug, Serialize, Clone)]
pub struct Branch {
    pub name: String,
    pub is_current: bool,
    pub is_remote: bool,
    pub upstream: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct CommitEntry {
    pub sha: String,
    pub short_sha: String,
    pub author: String,
    pub date: String,
    pub subject: String,
}

fn git(repo: &str) -> Command {
    let mut c = Command::new("git");
    c.current_dir(repo);
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

pub fn status(repo: &str) -> Result<GitStatus, String> {
    // `--porcelain=v1 --branch` without `-z`: LF-separated entries, stable
    // format, one line per file. We deliberately skip `-z` because with it
    // the branch header is also NUL-terminated (no trailing \n), which
    // trips up simple line-based parsers — and LF-separated parsing handles
    // 99.9% of real paths; only files with literal newlines in their names
    // would misparse, and those aren't a reality we need to support in MVP.
    //
    // `-uall` (≡ `--untracked-files=all`): without it git COLLAPSES an
    // untracked directory into a single line `?? dir/`, so the CHANGES
    // panel showed e.g. `?? internal-scripts/` instead of the seven new
    // files inside it. That made stage-all / discard-all act on the
    // whole tree blindly and hid file-level diffs. With `-uall` git
    // emits one entry per file like `?? internal-scripts/foo.ts`, which
    // is what the panel needs to render and stage individually.
    let mut cmd = git(repo);
    cmd.args(["status", "--porcelain=v1", "--branch", "-uall"]);
    let raw = run(cmd)?;

    let mut branch: Option<String> = None;
    let mut upstream: Option<String> = None;
    let mut ahead: u32 = 0;
    let mut behind: u32 = 0;
    let mut files: Vec<FileStatus> = Vec::new();

    for line in raw.lines() {
        if let Some(stripped) = line.strip_prefix("## ") {
            // "branch...origin/branch [ahead 2, behind 1]" or "HEAD (no branch)".
            let (refs, counters) = match stripped.find(" [") {
                Some(i) => (
                    &stripped[..i],
                    Some(stripped[i + 2..].trim_end_matches(']')),
                ),
                None => (stripped, None),
            };
            if let Some((b, u)) = refs.split_once("...") {
                branch = Some(b.to_string());
                upstream = Some(u.to_string());
            } else {
                branch = Some(refs.to_string());
            }
            if let Some(c) = counters {
                for part in c.split(", ") {
                    if let Some(n) = part.strip_prefix("ahead ") {
                        ahead = n.parse().unwrap_or(0);
                    } else if let Some(n) = part.strip_prefix("behind ") {
                        behind = n.parse().unwrap_or(0);
                    }
                }
            }
            continue;
        }
        if line.len() < 3 {
            continue;
        }
        // Porcelain v1 format: "XY path" (X=index, Y=worktree, then space,
        // then path). Rename shows "R  old -> new" — we grab the NEW name.
        let code: String = line.chars().take(2).collect();
        let rest = &line[3..];
        let path = match rest.split_once(" -> ") {
            Some((_old, new)) => new.to_string(),
            None => rest.to_string(),
        };
        if path.is_empty() {
            continue;
        }
        let staged = !matches!(code.chars().next().unwrap_or(' '), ' ' | '?');
        let unstaged = !matches!(code.chars().nth(1).unwrap_or(' '), ' ');
        files.push(FileStatus { path, code, staged, unstaged });
    }

    Ok(GitStatus { branch, upstream, ahead, behind, files })
}

/// Flat list of files in this repo — tracked + untracked minus ignored.
/// Used by the chat-composer's `@`-autocomplete popover. Falls back to
/// an empty list on any error so the popover just shows no files instead
/// of the whole UI breaking.
pub fn ls_files(repo: &str) -> Vec<String> {
    let mut cmd = git(repo);
    cmd.args(["ls-files", "-z", "--cached", "--others", "--exclude-standard"]);
    let out = match cmd.output() {
        Ok(o) => o,
        Err(_) => return Vec::new(),
    };
    if !out.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&out.stdout)
        .split('\0')
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

/// True iff `rel_path` (repo-relative) is tracked in the index of `repo`.
/// Used as a safety guardrail before destructive operations (e.g. `Revert`
/// on a Write card with `isCreate=true`): if the front-end thinks "agent
/// just created this", but git knows about the path, the FE's backfill
/// silently failed and removing the file would clobber committed work.
///
/// Implemented via `git ls-files --error-unmatch -- <path>`: exits 0 iff
/// the path is in the index, otherwise non-zero. Cheaper than `cat-file`
/// or `log -1` because it doesn't touch object storage. Returns false on
/// any error (not-a-repo, git missing, …) so callers see "not tracked"
/// and fall through to their default branch — the caller decides how
/// strict it wants to be about ambiguous results.
pub fn is_tracked(repo: &str, rel_path: &str) -> bool {
    if repo.is_empty() || rel_path.is_empty() {
        return false;
    }
    let mut cmd = git(repo);
    cmd.args(["ls-files", "--error-unmatch", "--", rel_path]);
    cmd.output().map(|o| o.status.success()).unwrap_or(false)
}

/// Return the subset of `paths` that are gitignored by this repo. Uses
/// `git check-ignore --stdin -z` so behavior matches exactly what `git add`
/// would skip (nested .gitignore + .git/info/exclude + global ignore all
/// honored). The `-z` flag only works with `--stdin`, so paths are piped
/// in NUL-separated. Non-fatal on error — bad repo or transient failure
/// just returns an empty list rather than blocking the file tree render.
pub fn check_ignore(repo: &str, paths: &[String]) -> Vec<String> {
    use std::io::Write;
    if paths.is_empty() {
        return Vec::new();
    }
    let mut cmd = git(repo);
    cmd.args(["check-ignore", "-z", "--stdin"]);
    cmd.stdin(std::process::Stdio::piped());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    if let Some(mut stdin) = child.stdin.take() {
        let mut payload: Vec<u8> = Vec::with_capacity(
            paths.iter().map(|p| p.len() + 1).sum(),
        );
        for (i, p) in paths.iter().enumerate() {
            if i > 0 {
                payload.push(0);
            }
            payload.extend_from_slice(p.as_bytes());
        }
        let _ = stdin.write_all(&payload);
        // Dropping `stdin` here closes the pipe so git knows input is done.
    }

    let out = match child.wait_with_output() {
        Ok(o) => o,
        Err(_) => return Vec::new(),
    };
    let code = out.status.code().unwrap_or(-1);
    // `git check-ignore` exit codes: 0 = one or more paths ignored,
    // 1 = none ignored (expected; empty list), 128 = fatal (e.g. not a repo).
    if code == 1 || code == 128 {
        return Vec::new();
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    stdout
        .split('\0')
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

pub fn branches(repo: &str) -> Result<Vec<Branch>, String> {
    let mut cmd = git(repo);
    cmd.args([
        "for-each-ref",
        "--format=%(refname:short)%00%(HEAD)%00%(upstream:short)",
        "refs/heads/",
        "refs/remotes/",
    ]);
    let raw = run(cmd)?;
    let mut out: Vec<Branch> = Vec::new();
    for line in raw.lines() {
        let parts: Vec<&str> = line.split('\0').collect();
        if parts.len() < 3 {
            continue;
        }
        let name = parts[0].to_string();
        if name.ends_with("/HEAD") {
            continue; // skip remote HEAD pointer
        }
        let is_remote = name.starts_with("origin/") || name.contains('/') && !is_local_name(&name);
        let is_current = parts[1] == "*";
        let upstream = if parts[2].is_empty() { None } else { Some(parts[2].to_string()) };
        out.push(Branch { name, is_current, is_remote, upstream });
    }
    Ok(out)
}

fn is_local_name(s: &str) -> bool {
    // A ref like `refs/heads/feat/foo` comes back as `feat/foo`, which has a
    // slash. So we can't just check for a slash — for-each-ref only lists the
    // refs we asked for, so anything starting with "origin/" or a known
    // remote prefix is remote. Conservative: treat unknown-prefix/slash refs
    // as local (they'll be local branches with slashes in the name).
    !s.starts_with("origin/")
}

pub fn current_branch(repo: &str) -> Result<String, String> {
    let mut cmd = git(repo);
    cmd.args(["branch", "--show-current"]);
    Ok(run(cmd)?.trim().to_string())
}

pub fn checkout(repo: &str, branch: &str) -> Result<(), String> {
    let mut cmd = git(repo);
    cmd.args(["checkout", branch]);
    run(cmd).map(|_| ())
}

pub fn create_branch(
    repo: &str,
    name: &str,
    checkout: bool,
    start_point: Option<&str>,
) -> Result<(), String> {
    let mut cmd = git(repo);
    if checkout {
        cmd.args(["checkout", "-b", name]);
    } else {
        cmd.args(["branch", name]);
    }
    // `start_point` is optional. When the user picks "from <other branch>"
    // (local OR `origin/foo`), git will fork the new branch off that ref.
    // Omitting it preserves the original behaviour (fork off HEAD).
    if let Some(sp) = start_point.filter(|s| !s.is_empty()) {
        cmd.arg(sp);
    }
    run(cmd).map(|_| ())
}

/// Fetch all remotes with `--prune` so deleted remote branches disappear
/// from `for-each-ref` output. Used to refresh the branch picker before
/// listing — otherwise `origin/feat-old` lingers for hours after another
/// dev deletes it server-side.
pub fn fetch(repo: &str) -> Result<String, String> {
    let mut cmd = git(repo);
    cmd.args(["fetch", "--all", "--prune"]);
    run(cmd)
}

pub fn stage(repo: &str, paths: &[String]) -> Result<(), String> {
    if paths.is_empty() {
        return Ok(());
    }
    let mut cmd = git(repo);
    cmd.arg("add").arg("--").args(paths);
    run(cmd).map(|_| ())
}

pub fn unstage(repo: &str, paths: &[String]) -> Result<(), String> {
    if paths.is_empty() {
        return Ok(());
    }
    let mut cmd = git(repo);
    cmd.args(["restore", "--staged", "--"]).args(paths);
    run(cmd).map(|_| ())
}

/// Discard local changes for a set of paths. Bucketed by their relationship
/// to HEAD, because the right revert op differs:
///   - in HEAD        → `git checkout HEAD -- <path>` (full revert incl. staged diff)
///   - staged, new    → `git reset HEAD -- <path>` then `rm` from disk
///                       (added to index but doesn't exist in HEAD — `checkout
///                        HEAD --` fails with `pathspec did not match any files`)
///   - untracked      → just `rm` from disk
///
/// Destructive: callers must confirm with the user before invoking this.
pub fn discard(repo: &str, paths: &[String]) -> Result<(), String> {
    if paths.is_empty() {
        return Ok(());
    }
    let mut in_head: Vec<&String> = Vec::new();
    let mut staged_added: Vec<&String> = Vec::new();
    let mut untracked: Vec<&String> = Vec::new();
    for p in paths {
        // `ls-tree HEAD` checks membership in the HEAD tree — prints the
        // blob line on hit, nothing on miss.
        let head_hit = {
            let mut c = git(repo);
            c.args(["ls-tree", "HEAD", "--", p]);
            c.output()
                .map(|o| o.status.success() && !o.stdout.is_empty())
                .unwrap_or(false)
        };
        if head_hit {
            in_head.push(p);
            continue;
        }
        // Not in HEAD but maybe staged-added — `ls-files --cached` checks
        // the index specifically (no worktree).
        let in_index = {
            let mut c = git(repo);
            c.args(["ls-files", "--cached", "--error-unmatch", "--", p]);
            c.output().map(|o| o.status.success()).unwrap_or(false)
        };
        if in_index {
            staged_added.push(p);
        } else {
            untracked.push(p);
        }
    }

    if !in_head.is_empty() {
        let mut cmd = git(repo);
        cmd.args(["checkout", "HEAD", "--"]).args(&in_head);
        run(cmd).map(|_| ())?;
    }

    // Unstage added-to-index-but-new-to-HEAD files so `rm` below doesn't
    // leave orphan index entries pointing at deleted paths.
    if !staged_added.is_empty() {
        let mut cmd = git(repo);
        cmd.args(["reset", "HEAD", "--"]).args(&staged_added);
        run(cmd).map(|_| ())?;
    }

    for p in staged_added.iter().chain(untracked.iter()) {
        let full = std::path::Path::new(repo).join(p);
        match std::fs::remove_file(&full) {
            Ok(_) => {}
            // Ignore "already gone" as an idempotent success.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("rm {}: {}", full.display(), e)),
        }
    }
    Ok(())
}

pub fn commit(repo: &str, message: &str) -> Result<String, String> {
    let mut cmd = git(repo);
    cmd.args(["commit", "-m", message]);
    let out = run(cmd)?;
    // Return the new HEAD sha so the UI can show confirmation.
    let mut sha_cmd = git(repo);
    sha_cmd.args(["rev-parse", "HEAD"]);
    let sha = run(sha_cmd)?.trim().to_string();
    let _ = out; // commit's stdout is not especially useful
    Ok(sha)
}

pub fn push(repo: &str) -> Result<String, String> {
    let mut cmd = git(repo);
    cmd.arg("push");
    match run(cmd) {
        Ok(out) => Ok(out),
        Err(err) => {
            // Fresh local branch (no upstream yet) → git's first push fails
            // with "fatal: The current branch <name> has no upstream branch.
            // To push the current branch and set the remote as upstream, use
            // git push --set-upstream origin <name>". Retrying with
            // `--set-upstream origin HEAD` does exactly that and is the
            // behaviour every editor (VS Code, Cursor, GitHub Desktop)
            // ships by default. We do it transparently so the propose_commit
            // action card and the GitPanel push button "just work" on a
            // brand-new branch instead of bouncing back the raw fatal.
            //
            // Match on git's wording (case-insensitive — older git localises
            // some messages but the english phrase is stable since 2.x):
            // "no upstream branch" OR "--set-upstream" both appear in the
            // exact failure mode we care about, and nowhere else.
            let lower = err.to_lowercase();
            if !(lower.contains("no upstream branch") || lower.contains("--set-upstream")) {
                return Err(err);
            }
            let branch = current_branch(repo).unwrap_or_default();
            if branch.is_empty() {
                // Detached HEAD — `--set-upstream` would be meaningless. Bubble up
                // the original error so the user knows to checkout a branch first.
                return Err(err);
            }
            let mut retry = git(repo);
            retry.args(["push", "--set-upstream", "origin", &branch]);
            let retry_out = run(retry)?;
            Ok(format!(
                "Set upstream to origin/{branch} and pushed.\n{}",
                retry_out.trim()
            ))
        }
    }
}

pub fn pull(repo: &str) -> Result<String, String> {
    let mut cmd = git(repo);
    cmd.arg("pull");
    run(cmd)
}

pub fn log(repo: &str, limit: u32) -> Result<Vec<CommitEntry>, String> {
    let mut cmd = git(repo);
    cmd.args([
        "log",
        &format!("-{}", limit.max(1)),
        "--pretty=format:%H%x00%h%x00%an%x00%ad%x00%s",
        "--date=iso-strict",
    ]);
    let raw = run(cmd)?;
    let mut out: Vec<CommitEntry> = Vec::new();
    for line in raw.lines() {
        let parts: Vec<&str> = line.split('\0').collect();
        if parts.len() < 5 {
            continue;
        }
        out.push(CommitEntry {
            sha: parts[0].to_string(),
            short_sha: parts[1].to_string(),
            author: parts[2].to_string(),
            date: parts[3].to_string(),
            subject: parts[4].to_string(),
        });
    }
    Ok(out)
}

pub fn repo_root(path: &str) -> Result<String, String> {
    // Find the enclosing git work tree. Useful when the user picks a file
    // inside a repo subdirectory.
    let mut cmd = git(path);
    cmd.args(["rev-parse", "--show-toplevel"]);
    Ok(run(cmd)?.trim().to_string())
}

#[derive(Debug, Serialize, Clone)]
pub struct RepoInfo {
    /// True when `path` (or any parent) is a git repo.
    pub is_git: bool,
    /// Resolved repo root — the top of the working tree.
    pub root: Option<String>,
    /// Currently checked-out branch (None = detached HEAD).
    pub current_branch: Option<String>,
    /// `origin` remote URL, or any remote if `origin` is missing.
    pub remote_url: Option<String>,
    pub remote_name: Option<String>,
    /// Total number of non-staged / non-committed changes.
    pub dirty_count: u32,
    /// Untracked files (a subset of dirty_count).
    pub untracked_count: u32,
    pub ahead: u32,
    pub behind: u32,
    /// True when the path isn't a directory we could even stat.
    pub missing: bool,
}

/// One-shot combined summary of a local path — is it a git repo, on what
/// branch, with what remote and how dirty? Everything the UI needs to show
/// a compact "repo status" chip.
///
/// Returns a best-effort result — if git commands fail for a specific slice
/// (e.g. no remote configured), those fields are None but the rest is
/// still populated.
pub fn repo_info(path: &str) -> RepoInfo {
    let p = std::path::Path::new(path);
    if !p.exists() {
        return RepoInfo {
            is_git: false,
            root: None,
            current_branch: None,
            remote_url: None,
            remote_name: None,
            dirty_count: 0,
            untracked_count: 0,
            ahead: 0,
            behind: 0,
            missing: true,
        };
    }

    // Resolve repo root. If this fails, the path isn't a git repo — that's a
    // legitimate state, not an error.
    let root = {
        let mut c = git(path);
        c.args(["rev-parse", "--show-toplevel"]);
        run(c).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
    };
    if root.is_none() {
        return RepoInfo {
            is_git: false,
            root: None,
            current_branch: None,
            remote_url: None,
            remote_name: None,
            dirty_count: 0,
            untracked_count: 0,
            ahead: 0,
            behind: 0,
            missing: false,
        };
    }
    let root_path = root.clone().unwrap();

    // Branch — None if detached.
    let current_branch = {
        let mut c = git(&root_path);
        c.args(["branch", "--show-current"]);
        run(c).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
    };

    // Remote — prefer origin, then first listed.
    let (remote_name, remote_url) = {
        let mut c = git(&root_path);
        c.args(["remote", "-v"]);
        let raw = run(c).unwrap_or_default();
        let mut origin: Option<(String, String)> = None;
        let mut first: Option<(String, String)> = None;
        for line in raw.lines() {
            // format: "origin\tgit@github.com:acme/repo.git (fetch)"
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 2 { continue; }
            let name = parts[0].to_string();
            let url = parts[1].to_string();
            if first.is_none() { first = Some((name.clone(), url.clone())); }
            if name == "origin" { origin = Some((name, url)); break; }
        }
        match origin.or(first) {
            Some((n, u)) => (Some(n), Some(u)),
            None => (None, None),
        }
    };

    // Status — ahead/behind + counts. Same `-uall` reasoning as
    // `status()` above: without it, an untracked directory with N
    // new files counts as ONE untracked entry, so the repo badge
    // showed "1" instead of the real file count and felt wrong
    // next to a CHANGES panel that exposes per-file rows.
    let (ahead, behind, dirty_count, untracked_count) = {
        let mut c = git(&root_path);
        c.args(["status", "--porcelain=v1", "--branch", "-uall"]);
        let raw = run(c).unwrap_or_default();
        let mut ahead: u32 = 0;
        let mut behind: u32 = 0;
        let mut dirty: u32 = 0;
        let mut untracked: u32 = 0;
        for line in raw.lines() {
            if let Some(h) = line.strip_prefix("## ") {
                if let Some(i) = h.find(" [") {
                    let counters = &h[i + 2..h.len() - 1];
                    for part in counters.split(", ") {
                        if let Some(n) = part.strip_prefix("ahead ") {
                            ahead = n.parse().unwrap_or(0);
                        } else if let Some(n) = part.strip_prefix("behind ") {
                            behind = n.parse().unwrap_or(0);
                        }
                    }
                }
                continue;
            }
            if line.starts_with("?? ") {
                untracked += 1;
                dirty += 1;
            } else if !line.is_empty() {
                dirty += 1;
            }
        }
        (ahead, behind, dirty, untracked)
    };

    RepoInfo {
        is_git: true,
        root,
        current_branch,
        remote_url,
        remote_name,
        dirty_count,
        untracked_count,
        ahead,
        behind,
        missing: false,
    }
}

/// Return the contents of a file at a given git revision. `revision` can
/// be `"HEAD"`, `":"` (the index / staging area), any branch name, a SHA,
/// etc. Empty revision means "worktree" — we read from disk.
///
/// Returns an empty string if the path doesn't exist at that revision —
/// this mirrors what a side-by-side diff of a new file should show
/// (empty left side, contents on right).
pub fn show(repo: &str, revision: &str, path: &str) -> Result<String, String> {
    if revision.is_empty() {
        // Worktree: just read from disk.
        let full = std::path::Path::new(repo).join(path);
        return std::fs::read_to_string(&full).or_else(|e| {
            // Deleted-in-worktree case: return empty, not an error.
            if e.kind() == std::io::ErrorKind::NotFound {
                Ok(String::new())
            } else {
                Err(format!("read {}: {}", full.display(), e))
            }
        });
    }
    // Strip a trailing `:` from revision because we always add one below.
    // Caller may pass `":"` (shorthand for index) or `"HEAD"` — both produce
    // the right spec after normalization:
    //   ":"    → ""     + ":" + path  = ":path"       (index stage 0)
    //   "HEAD" → "HEAD" + ":" + path  = "HEAD:path"   (last commit)
    let clean_rev = revision.trim_end_matches(':');
    let rev_spec = format!("{}:{}", clean_rev, path);
    let mut cmd = git(repo);
    cmd.args(["show", &rev_spec]);
    match run(cmd) {
        Ok(s) => Ok(s),
        // `git show HEAD:new-file` fails with exit 128 — that's a newly-added
        // file with no HEAD version. Return empty so the diff shows "added".
        Err(_) => Ok(String::new()),
    }
}

/// Unified diff for a single file. `staged=true` compares index vs HEAD
/// (i.e. what will go in the next commit); `staged=false` compares
/// worktree vs index (i.e. local edits not yet staged).
///
/// For untracked files, `git diff` returns nothing, so we fall back to a
/// synthetic "added" diff by rendering the whole file as +lines.
pub fn diff(repo: &str, path: &str, staged: bool) -> Result<String, String> {
    let mut cmd = git(repo);
    cmd.args(["diff", "--no-color", "--no-ext-diff", "-U3"]);
    if staged {
        cmd.arg("--cached");
    }
    cmd.arg("--").arg(path);
    let out = run(cmd)?;
    if !out.trim().is_empty() {
        return Ok(out);
    }
    // Untracked file path: `git diff` produces empty. Synthesize an "added"
    // diff so the UI still has something to render.
    if !staged {
        let full = std::path::Path::new(repo).join(path);
        if let Ok(contents) = std::fs::read_to_string(&full) {
            let line_count = contents.lines().count();
            let mut out = String::new();
            out.push_str(&format!("diff --git a/{} b/{}\n", path, path));
            out.push_str("new file mode 100644\n");
            out.push_str("--- /dev/null\n");
            out.push_str(&format!("+++ b/{}\n", path));
            out.push_str(&format!("@@ -0,0 +1,{} @@\n", line_count));
            for line in contents.lines() {
                out.push('+');
                out.push_str(line);
                out.push('\n');
            }
            return Ok(out);
        }
    }
    Ok(out)
}

/// Whether the `gh` CLI is on PATH. We use it for PR creation because it
/// already handles GitHub auth (via `gh auth login` or the GITHUB_TOKEN
/// env var). If not available, the frontend should fall back to a manual
/// browser-based flow.
pub fn gh_cli_available() -> bool {
    Command::new("gh")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Create a PR via GitHub's REST API using the Keychain-stored token. Fully
/// replaces the old `gh pr create` shell-out so Woom needs no extra CLI.
///
/// The caller must push the head branch first — this function does NOT push.
/// Orchestration: read the current branch + origin URL from the local repo,
/// parse `owner/repo`, resolve the base (default branch if blank), then call
/// into `github::create_pr` for the HTTP POST.
pub async fn create_pr(
    repo: &str,
    title: &str,
    body: &str,
    draft: bool,
    base: Option<&str>,
    token: &str,
) -> Result<String, String> {
    let head = current_branch(repo)?;
    let remote_url = origin_url(repo)?;
    let (owner, repo_name) = parse_github_slug(&remote_url)?;
    let base_branch = match base.map(|s| s.trim()).filter(|s| !s.is_empty()) {
        Some(b) => b.to_string(),
        None => crate::github::fetch_default_branch(token, &owner, &repo_name)
            .await
            .map_err(|e| e.to_string())?,
    };
    crate::github::create_pr(
        token,
        &owner,
        &repo_name,
        title,
        body,
        &head,
        &base_branch,
        draft,
    )
    .await
    .map_err(|e| e.to_string())
}

fn origin_url(repo: &str) -> Result<String, String> {
    let mut c = Command::new("git");
    c.current_dir(repo).args(["remote", "get-url", "origin"]);
    run(c).map(|s| s.trim().to_string())
}

/// Parse `owner/repo` out of a GitHub remote URL. Accepts:
///   - `git@github.com:owner/repo.git`
///   - `git@github.com-work:owner/repo.git` (SSH config alias, common for
///     multi-account setups — `Host github.com-work` in `~/.ssh/config`)
///   - `https://github.com/owner/repo(.git)?`
///   - `ssh://git@github.com[:22]/owner/repo(.git)?`
///   - `git://github.com/owner/repo(.git)?`
fn parse_github_slug(url: &str) -> Result<(String, String), String> {
    let trimmed = url.trim().trim_end_matches('/').trim_end_matches(".git");

    // Split off transport → extract (host, path).
    let (host_raw, path) = if let Some(rest) = trimmed.strip_prefix("ssh://") {
        // `user@host[:port]/owner/repo`
        let after_user = rest.split_once('@').map(|(_, r)| r).unwrap_or(rest);
        after_user
            .split_once('/')
            .ok_or_else(|| format!("could not parse ssh:// remote: {}", url))?
    } else if let Some(rest) = trimmed.strip_prefix("git@") {
        // `host:owner/repo`
        rest.split_once(':')
            .ok_or_else(|| format!("could not parse SSH remote: {}", url))?
    } else if let Some(rest) = trimmed.strip_prefix("https://") {
        rest.split_once('/')
            .ok_or_else(|| format!("could not parse HTTPS remote: {}", url))?
    } else if let Some(rest) = trimmed.strip_prefix("http://") {
        rest.split_once('/')
            .ok_or_else(|| format!("could not parse HTTP remote: {}", url))?
    } else if let Some(rest) = trimmed.strip_prefix("git://") {
        rest.split_once('/')
            .ok_or_else(|| format!("could not parse git:// remote: {}", url))?
    } else {
        return Err(format!("unsupported remote URL: {}", url));
    };

    // Strip optional port on SSH/HTTPS hosts (`github.com:22` → `github.com`).
    let host_bare = host_raw.split(':').next().unwrap_or(host_raw);
    // Accept the canonical host and any `github.com-...` alias.
    let is_github = host_bare == "github.com"
        || host_bare.starts_with("github.com-")
        || host_bare.starts_with("github.com.");
    if !is_github {
        return Err(format!(
            "unsupported remote host `{}` — PR creation needs github.com (SSH aliases like `github.com-work` are fine; other providers aren't supported yet)",
            host_bare
        ));
    }

    let parts: Vec<&str> = path.splitn(2, '/').collect();
    if parts.len() != 2 || parts[0].is_empty() || parts[1].is_empty() {
        return Err(format!("could not parse owner/repo from: {}", url));
    }
    Ok((parts[0].to_string(), parts[1].to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ssh_canonical() {
        let (o, r) = parse_github_slug("git@github.com:foo/bar.git").unwrap();
        assert_eq!((o.as_str(), r.as_str()), ("foo", "bar"));
    }

    #[test]
    fn parses_ssh_alias() {
        let (o, r) = parse_github_slug("git@github.com-work:Efficiently-Dev/efficiently.git").unwrap();
        assert_eq!((o.as_str(), r.as_str()), ("Efficiently-Dev", "efficiently"));
    }

    #[test]
    fn parses_https() {
        let (o, r) = parse_github_slug("https://github.com/foo/bar").unwrap();
        assert_eq!((o.as_str(), r.as_str()), ("foo", "bar"));
    }

    #[test]
    fn parses_ssh_with_scheme() {
        let (o, r) = parse_github_slug("ssh://git@github.com/foo/bar.git").unwrap();
        assert_eq!((o.as_str(), r.as_str()), ("foo", "bar"));
    }

    #[test]
    fn rejects_non_github_host() {
        assert!(parse_github_slug("git@gitlab.com:foo/bar.git").is_err());
    }
}
