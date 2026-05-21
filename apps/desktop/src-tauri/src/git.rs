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

// ---------------------------------------------------------------------------
// SDD-specific git helpers — used by the Spec-Driven-Development orchestrator
// to commit pre/post-phase snapshots, roll a phase back, and detect crash-
// orphaned phases on app boot. Kept here (vs in sdd.rs) so the existing
// `git()` / `run()` plumbing is reused and there's only one shell-out path
// for the whole crate.
// ---------------------------------------------------------------------------

/// Cheap "is this directory inside a git work tree?" probe. Any non-zero
/// exit (including "not a repo") returns false. Used to flip the SDD
/// workspace's `git_enabled` flag — non-git working dirs degrade
/// gracefully (no branch / commit / rollback, but the rest of SDD works).
pub fn is_git_repo(repo_cwd: &str) -> bool {
    if repo_cwd.is_empty() {
        return false;
    }
    let mut c = git(repo_cwd);
    c.args(["rev-parse", "--git-dir"]);
    c.output().map(|o| o.status.success()).unwrap_or(false)
}

/// True when `git status --porcelain` is empty — no untracked, no
/// staged, no modified. Used by the SDD pre-phase snapshot path to
/// decide whether to safety-stash before committing the pre-phase
/// marker.
pub fn working_dir_clean(repo_cwd: &str) -> bool {
    let mut c = git(repo_cwd);
    c.args(["status", "--porcelain"]);
    match c.output() {
        Ok(o) if o.status.success() => o.stdout.iter().all(|b| b.is_ascii_whitespace()),
        _ => false,
    }
}

/// Resolve `HEAD` to a full sha. Returns Err on detached HEAD with no
/// commits at all (fresh repo) so callers can short-circuit instead
/// of cargo-culting a broken ref.
pub fn head_sha(repo_cwd: &str) -> Result<String, String> {
    let mut c = git(repo_cwd);
    c.args(["rev-parse", "HEAD"]);
    Ok(run(c)?.trim().to_string())
}

/// Create a new branch at HEAD without checking it out. Used to mint
/// the per-workspace `sdd/<id>` branch on `sdd_start` so all SDD
/// commits land on a dedicated ref the user can review (or discard)
/// later. Idempotent — `git branch <name>` exits non-zero if the
/// branch already exists, which we treat as a no-op success since
/// the workspace already had this branch from a prior boot.
pub fn create_branch_at_head(repo_cwd: &str, name: &str) -> Result<(), String> {
    let mut c = git(repo_cwd);
    c.args(["branch", name]);
    match run(c) {
        Ok(_) => Ok(()),
        Err(e) if e.to_lowercase().contains("already exists") => Ok(()),
        Err(e) => Err(e),
    }
}

/// Commit ALL working-tree changes (`git add -A && git commit`) on the
/// CURRENT branch with the given message. `--allow-empty` so it
/// works for "no-op phase" or pre-phase markers when the tree was
/// already clean. Returns the new HEAD sha.
pub fn commit_all_allow_empty(repo_cwd: &str, message: &str) -> Result<String, String> {
    // Stage everything first so the commit captures untracked files too.
    let mut add = git(repo_cwd);
    add.args(["add", "-A"]);
    let _ = run(add); // tolerate "nothing to add" on a clean tree
    let mut commit = git(repo_cwd);
    commit.args(["commit", "--allow-empty", "-m", message]);
    run(commit)?;
    head_sha(repo_cwd)
}

/// Hard-reset to a known sha. Destructive; callers MUST safety-stash
/// first if there's any uncommitted work they want to preserve.
pub fn reset_hard(repo_cwd: &str, sha: &str) -> Result<(), String> {
    let mut c = git(repo_cwd);
    c.args(["reset", "--hard", sha]);
    run(c).map(|_| ())
}

/// Stash including untracked files under a custom label so it's
/// identifiable later (`git stash list`). Returns Ok(true) if a
/// stash was actually created, Ok(false) if there was nothing to
/// stash (avoiding a confusing empty-stash entry). The label travels
/// in the stash message so the user can see "sdd-pre-phase-2-<wsid>"
/// in their `git stash list` output.
pub fn stash_with_label(repo_cwd: &str, label: &str) -> Result<bool, String> {
    if working_dir_clean(repo_cwd) {
        return Ok(false);
    }
    let mut c = git(repo_cwd);
    c.args(["stash", "push", "-u", "-m", label]);
    run(c).map(|_| true)
}

/// SDD-flavoured working-tree summary used by SddCard's git-row.
/// Cheaper than `repo_info` (skips remote-url parsing); aligns the
/// shape of what the SDD frontend wants to render.
#[derive(Debug, Serialize, Clone)]
pub struct SddGitState {
    pub enabled: bool,
    pub branch: Option<String>,
    pub on_sdd_branch: bool,
    pub ahead: u32,
    pub behind: u32,
    pub dirty: bool,
}

pub fn sdd_git_state(repo_cwd: &str, sdd_branch: &str) -> SddGitState {
    if !is_git_repo(repo_cwd) {
        return SddGitState {
            enabled: false,
            branch: None,
            on_sdd_branch: false,
            ahead: 0,
            behind: 0,
            dirty: false,
        };
    }
    let st = status(repo_cwd).ok();
    let branch = st.as_ref().and_then(|s| s.branch.clone());
    let on_sdd_branch = branch.as_deref() == Some(sdd_branch);
    SddGitState {
        enabled: true,
        branch,
        on_sdd_branch,
        ahead: st.as_ref().map(|s| s.ahead).unwrap_or(0),
        behind: st.as_ref().map(|s| s.behind).unwrap_or(0),
        dirty: st.as_ref().map(|s| !s.files.is_empty()).unwrap_or(false),
    }
}

/// Per-file entry in a phase diff. `status` mirrors `git diff
/// --name-status` letters: A = added, M = modified, D = deleted,
/// R = renamed, C = copied, T = type-change. `insertions`/`deletions`
/// from `--numstat`. Binary files have `is_binary: true` and zero
/// counts (numstat reports `-`).
#[derive(Debug, Serialize, Clone, PartialEq)]
pub struct DiffFile {
    pub path: String,
    pub status: String,
    pub insertions: u32,
    pub deletions: u32,
    pub is_binary: bool,
    /// Set on `R` / `C` so the UI can display "old → new".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_path: Option<String>,
}

/// Aggregate result of `compute_phase_diff`. `skipped: true` signals
/// that the phase had no `pre_phase_sha` (snapshot intentionally
/// skipped in phase 3 — clean-tree gate failed) so there's nothing to
/// diff. UI renders a "git snapshot was skipped for this phase"
/// placeholder in that case.
#[derive(Debug, Serialize, Clone, PartialEq)]
pub struct SddPhaseDiff {
    pub files: Vec<DiffFile>,
    pub total_insertions: u32,
    pub total_deletions: u32,
    pub skipped: bool,
}

/// Compute per-file diff stats between `pre_sha` and `post_sha`.
/// Combines `git diff --name-status` (letter status) with `git diff
/// --numstat` (line counts) into a single `DiffFile` per path.
///
/// Both git commands are run on the same SHA range so order is
/// stable; `--name-status` is parsed first to seed the file list +
/// pick up renames, then `--numstat` fills in counts. A file that
/// appears only in numstat (shouldn't happen in practice) is appended
/// with status "M" as a fallback.
pub fn compute_phase_diff(repo: &str, pre_sha: &str, post_sha: &str) -> Result<SddPhaseDiff, String> {
    let range = format!("{}..{}", pre_sha, post_sha);
    let mut name_status_cmd = git(repo);
    name_status_cmd.args(["diff", "--name-status", "-M", "-C", &range]);
    let name_out = run(name_status_cmd)?;

    let mut numstat_cmd = git(repo);
    numstat_cmd.args(["diff", "--numstat", "-M", "-C", &range]);
    let numstat_out = run(numstat_cmd)?;

    /* Build path → (status, from_path) from `--name-status`. The
     *  rename/copy lines have THREE tab-fields: `R<score>\told\tnew`,
     *  the rest have TWO: `M\tpath`. We key on the *new* path so
     *  numstat (which always uses the new path) lines up. */
    use std::collections::HashMap;
    let mut status_by_path: HashMap<String, (String, Option<String>)> = HashMap::new();
    for line in name_out.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        match parts.as_slice() {
            [s, p] if !s.is_empty() => {
                status_by_path.insert((*p).to_string(), (status_letter(s).to_string(), None));
            }
            [s, from, to] if !s.is_empty() => {
                status_by_path.insert(
                    (*to).to_string(),
                    (status_letter(s).to_string(), Some((*from).to_string())),
                );
            }
            _ => {}
        }
    }

    let mut files: Vec<DiffFile> = Vec::new();
    let mut total_insertions: u32 = 0;
    let mut total_deletions: u32 = 0;
    for line in numstat_out.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 3 {
            continue;
        }
        let ins_raw = parts[0];
        let del_raw = parts[1];
        let path = parts[2].to_string();
        let is_binary = ins_raw == "-" || del_raw == "-";
        let insertions = ins_raw.parse::<u32>().unwrap_or(0);
        let deletions = del_raw.parse::<u32>().unwrap_or(0);
        total_insertions = total_insertions.saturating_add(insertions);
        total_deletions = total_deletions.saturating_add(deletions);
        let (status, from_path) = status_by_path
            .remove(&path)
            .unwrap_or_else(|| ("M".to_string(), None));
        files.push(DiffFile {
            path,
            status,
            insertions,
            deletions,
            is_binary,
            from_path,
        });
    }
    /* Any name-status entries without a numstat row (e.g. pure
     *  rename with no edits — git reports it with `0\t0` in numstat
     *  so this shouldn't fire, but defensive). */
    for (path, (status, from_path)) in status_by_path.into_iter() {
        files.push(DiffFile {
            path,
            status,
            insertions: 0,
            deletions: 0,
            is_binary: false,
            from_path,
        });
    }
    Ok(SddPhaseDiff {
        files,
        total_insertions,
        total_deletions,
        skipped: false,
    })
}

/// Get the unified-format patch for a single file between two SHAs.
/// Used by the lazy file-diff fetch from the UI when the user clicks
/// to expand a row. Returns the raw `git diff` output (already includes
/// `diff --git`/`index`/hunk headers); empty string if the file
/// matches between SHAs (shouldn't happen given we only call this for
/// listed files but harmless).
pub fn compute_file_diff(repo: &str, pre_sha: &str, post_sha: &str, path: &str) -> Result<String, String> {
    let range = format!("{}..{}", pre_sha, post_sha);
    let mut c = git(repo);
    c.args(["diff", "-M", "-C", &range, "--", path]);
    run(c)
}

/// `git diff --name-status` prefixes rename/copy with a similarity
/// score (e.g. `R100`, `C82`). We strip it for a stable single-letter
/// status the UI can switch on.
fn status_letter(s: &str) -> &str {
    if s.starts_with('R') {
        "R"
    } else if s.starts_with('C') {
        "C"
    } else {
        s
    }
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

    // ---- SDD helper tests --------------------------------------------------
    //
    // These bring up a real ephemeral git repo on disk so we exercise the
    // SAME shell-out path as production. Tempdir is hand-rolled (no extra
    // crate) — same approach as `sdd_verify::tests::td()`.

    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn td() -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let p = std::env::temp_dir().join(format!("woom-git-test-{pid}-{nanos}-{n}"));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn init_repo(dir: &std::path::Path) {
        // `git init -q -b main` so the default branch name is stable
        // across machines whose `init.defaultBranch` differs.
        let mut c = Command::new("git");
        c.current_dir(dir).args(["init", "-q", "-b", "main"]);
        c.output().unwrap();
        // Local user — required for commits to succeed in CI env.
        for kv in [("user.email", "test@woom"), ("user.name", "Woom Test")] {
            let mut c = Command::new("git");
            c.current_dir(dir).args(["config", kv.0, kv.1]);
            c.output().unwrap();
        }
    }

    #[test]
    fn is_git_repo_detects_init() {
        let dir = td();
        assert!(!is_git_repo(dir.to_str().unwrap()));
        init_repo(&dir);
        assert!(is_git_repo(dir.to_str().unwrap()));
    }

    #[test]
    fn working_dir_clean_tracks_changes() {
        let dir = td();
        init_repo(&dir);
        std::fs::write(dir.join("a.txt"), "hi").unwrap();
        // Untracked file → dirty.
        assert!(!working_dir_clean(dir.to_str().unwrap()));
        // After committing, clean again.
        let mut c = Command::new("git");
        c.current_dir(&dir).args(["add", "-A"]);
        c.output().unwrap();
        let mut c = Command::new("git");
        c.current_dir(&dir).args(["commit", "-q", "-m", "x"]);
        c.output().unwrap();
        assert!(working_dir_clean(dir.to_str().unwrap()));
    }

    #[test]
    fn create_branch_at_head_is_idempotent() {
        let dir = td();
        init_repo(&dir);
        std::fs::write(dir.join("seed.txt"), "x").unwrap();
        commit_all_allow_empty(dir.to_str().unwrap(), "seed").unwrap();
        let s = dir.to_str().unwrap();
        create_branch_at_head(s, "sdd/test").expect("first create");
        // Second call must not error — workspace re-init must be safe.
        create_branch_at_head(s, "sdd/test").expect("second create idempotent");
    }

    #[test]
    fn commit_all_allow_empty_returns_sha_and_handles_clean() {
        let dir = td();
        init_repo(&dir);
        let s = dir.to_str().unwrap();
        // Empty repo: --allow-empty still produces a commit.
        let sha1 = commit_all_allow_empty(s, "first").unwrap();
        assert_eq!(sha1.len(), 40);
        // Clean tree again: still works (allow-empty).
        let sha2 = commit_all_allow_empty(s, "still-empty").unwrap();
        assert_ne!(sha1, sha2);
    }

    #[test]
    fn reset_hard_rolls_back() {
        let dir = td();
        init_repo(&dir);
        let s = dir.to_str().unwrap();
        std::fs::write(dir.join("a.txt"), "v1").unwrap();
        let sha1 = commit_all_allow_empty(s, "v1").unwrap();
        std::fs::write(dir.join("a.txt"), "v2").unwrap();
        commit_all_allow_empty(s, "v2").unwrap();
        reset_hard(s, &sha1).unwrap();
        let cur = std::fs::read_to_string(dir.join("a.txt")).unwrap();
        assert_eq!(cur, "v1");
    }

    #[test]
    fn stash_with_label_skips_clean_tree() {
        let dir = td();
        init_repo(&dir);
        let s = dir.to_str().unwrap();
        commit_all_allow_empty(s, "seed").unwrap();
        // Clean → no stash created.
        assert_eq!(stash_with_label(s, "sdd-pre-1").unwrap(), false);
        // Dirty → stash created.
        std::fs::write(dir.join("a.txt"), "x").unwrap();
        assert_eq!(stash_with_label(s, "sdd-pre-1").unwrap(), true);
        // Working tree should now be clean again.
        assert!(working_dir_clean(s));
    }

    #[test]
    fn sdd_git_state_off_outside_repo() {
        let dir = td();
        let st = sdd_git_state(dir.to_str().unwrap(), "sdd/whatever");
        assert!(!st.enabled);
        assert!(!st.dirty);
    }

    #[test]
    fn sdd_git_state_inside_repo() {
        let dir = td();
        init_repo(&dir);
        let s = dir.to_str().unwrap();
        commit_all_allow_empty(s, "seed").unwrap();
        let st = sdd_git_state(s, "sdd/test-id");
        assert!(st.enabled);
        assert_eq!(st.branch.as_deref(), Some("main"));
        assert!(!st.on_sdd_branch);
        assert!(!st.dirty);
    }

    #[test]
    fn phase_diff_basic_modify_add_delete() {
        let dir = td();
        init_repo(&dir);
        let s = dir.to_str().unwrap();
        // Pre-state: a.txt with one line + b.txt to be deleted.
        std::fs::write(dir.join("a.txt"), "one\n").unwrap();
        std::fs::write(dir.join("b.txt"), "bye\n").unwrap();
        commit_all_allow_empty(s, "seed").unwrap();
        let pre = head_sha(s).unwrap();
        // Post-state: edit a.txt, add c.txt, delete b.txt.
        std::fs::write(dir.join("a.txt"), "one\ntwo\n").unwrap();
        std::fs::write(dir.join("c.txt"), "new\n").unwrap();
        std::fs::remove_file(dir.join("b.txt")).unwrap();
        commit_all_allow_empty(s, "phase").unwrap();
        let post = head_sha(s).unwrap();
        let diff = compute_phase_diff(s, &pre, &post).unwrap();
        assert!(!diff.skipped);
        assert_eq!(diff.files.len(), 3);
        let a = diff.files.iter().find(|f| f.path == "a.txt").unwrap();
        assert_eq!(a.status, "M");
        assert_eq!(a.insertions, 1);
        let c = diff.files.iter().find(|f| f.path == "c.txt").unwrap();
        assert_eq!(c.status, "A");
        let b = diff.files.iter().find(|f| f.path == "b.txt").unwrap();
        assert_eq!(b.status, "D");
        assert_eq!(diff.total_insertions, 2); // a.txt +1, c.txt +1
        assert_eq!(diff.total_deletions, 1); // b.txt -1
    }

    #[test]
    fn phase_diff_binary_file_marked_as_binary() {
        let dir = td();
        init_repo(&dir);
        let s = dir.to_str().unwrap();
        commit_all_allow_empty(s, "seed").unwrap();
        let pre = head_sha(s).unwrap();
        // Write file with NUL bytes — git treats it as binary, numstat
        // reports `-\t-`.
        std::fs::write(dir.join("blob.bin"), [0u8, 1, 2, 3, 0, 255]).unwrap();
        commit_all_allow_empty(s, "phase").unwrap();
        let post = head_sha(s).unwrap();
        let diff = compute_phase_diff(s, &pre, &post).unwrap();
        let f = diff.files.iter().find(|f| f.path == "blob.bin").unwrap();
        assert!(f.is_binary, "expected blob.bin marked binary, got {f:?}");
        assert_eq!(f.insertions, 0);
        assert_eq!(f.deletions, 0);
    }

    #[test]
    fn phase_diff_file_diff_returns_patch() {
        let dir = td();
        init_repo(&dir);
        let s = dir.to_str().unwrap();
        std::fs::write(dir.join("a.txt"), "one\n").unwrap();
        commit_all_allow_empty(s, "seed").unwrap();
        let pre = head_sha(s).unwrap();
        std::fs::write(dir.join("a.txt"), "one\ntwo\n").unwrap();
        commit_all_allow_empty(s, "phase").unwrap();
        let post = head_sha(s).unwrap();
        let patch = compute_file_diff(s, &pre, &post, "a.txt").unwrap();
        assert!(patch.contains("diff --git"), "patch missing header: {patch}");
        assert!(patch.contains("+two"), "patch missing addition: {patch}");
    }
}
