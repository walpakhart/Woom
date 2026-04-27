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
    std::fs::write(path, contents).map_err(|e| format!("write {}: {}", path, e))
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
    std::fs::write(path, bytes).map_err(|e| format!("write {}: {}", path, e))
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

#[derive(Debug, Serialize, Clone)]
pub struct BashResult {
    pub stdout: String,
    pub stderr: String,
    pub code: i32,
    pub ok: bool,
}

/// Run a shell command in `cwd` via `sh -c`. Captures stdout and stderr
/// separately and returns the exit code. Never panics; command failures
/// are reflected in `ok=false, code≠0`.
///
/// The 60-second timeout is the hard ceiling; long-running commands
/// (e.g. `npm install`) will be killed. Revisit if needed.
pub fn bash_run(cwd: &str, command: &str) -> Result<BashResult, String> {
    use std::process::{Command, Stdio};
    use std::thread;
    use std::time::Duration;
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(command)
        .current_dir(cwd)
        .env("PATH", enriched_path())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("spawn sh: {}", e))?;

    let timeout = Duration::from_secs(60);
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut stdout = String::new();
                let mut stderr = String::new();
                if let Some(mut out) = child.stdout.take() {
                    use std::io::Read;
                    let _ = out.read_to_string(&mut stdout);
                }
                if let Some(mut err) = child.stderr.take() {
                    use std::io::Read;
                    let _ = err.read_to_string(&mut stderr);
                }
                let code = status.code().unwrap_or(-1);
                return Ok(BashResult { stdout, stderr, code, ok: status.success() });
            }
            Ok(None) => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    return Err("command timed out after 60s".into());
                }
                thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(format!("wait: {}", e)),
        }
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
