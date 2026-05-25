//! Action-log Tauri commands. Extracted from `sdd.rs` in wave-19 split.
//! Thin wrappers around `crate::sdd_action_log` that read/write the
//! per-phase `action-log.jsonl` file. Frontend stream parser calls
//! `sdd_append_action_log` after every successful tool event; the
//! `sdd_append_action_log_batch` variant takes a vector for the
//! debounce-flush path that bursts a queue into one syscall.
//!
//! Schema is intentionally compact: `ts + phase + kind + payload` so
//! readers can forward-skip lines that don't deserialize (we'll grow
//! the schema over time).

use std::path::PathBuf;

use tauri::State;

use crate::sdd::SddRegistry;
use crate::sdd_action_log::{action_log_path, ActionLogEntry};

#[tauri::command]
pub async fn sdd_append_action_log(
    registry: State<'_, SddRegistry>,
    id: String,
    entry: ActionLogEntry,
) -> Result<(), String> {
    let cell = registry
        .workspaces
        .read()
        .get(&id)
        .cloned()
        .ok_or_else(|| format!("unknown workspace {id}"))?;
    let root = PathBuf::from(cell.read().root.clone());
    let path = action_log_path(&root, entry.phase);
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            eprintln!("[sdd] action_log mkdir {}: {e}", parent.display());
            return Ok(());
        }
    }
    let line = match serde_json::to_string(&entry) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[sdd] action_log serialize: {e}");
            return Ok(());
        }
    };
    use std::io::Write;
    match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        Ok(mut f) => {
            // Single-shot write so an interleaved write from another
            // turn doesn't splice mid-line. POSIX append-mode + a
            // single `write_all` smaller than PIPE_BUF is atomic.
            let _ = writeln!(f, "{line}");
        }
        Err(e) => eprintln!("[sdd] action_log open {}: {e}", path.display()),
    }
    Ok(())
}

/// Same as `sdd_append_action_log` but takes a vector — used by the
/// frontend's debounce-flush path so a burst of tool events writes in
/// one syscall instead of N. Each entry must already carry its own
/// `phase`; we group them by phase internally so a single batch can
/// span phases (rare, but cheap to handle).
#[tauri::command]
pub async fn sdd_append_action_log_batch(
    registry: State<'_, SddRegistry>,
    id: String,
    entries: Vec<ActionLogEntry>,
) -> Result<(), String> {
    if entries.is_empty() {
        return Ok(());
    }
    let cell = registry
        .workspaces
        .read()
        .get(&id)
        .cloned()
        .ok_or_else(|| format!("unknown workspace {id}"))?;
    let root = PathBuf::from(cell.read().root.clone());
    use std::collections::HashMap;
    use std::io::Write;
    let mut by_phase: HashMap<u32, String> = HashMap::new();
    for e in &entries {
        let line = match serde_json::to_string(e) {
            Ok(s) => s,
            Err(err) => {
                eprintln!("[sdd] action_log batch serialize: {err}");
                continue;
            }
        };
        let buf = by_phase.entry(e.phase).or_default();
        buf.push_str(&line);
        buf.push('\n');
    }
    let phases_dir = root.join("phases");
    let _ = std::fs::create_dir_all(&phases_dir);
    for (phase, body) in by_phase {
        let path = action_log_path(&root, phase);
        match std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        {
            Ok(mut f) => {
                let _ = f.write_all(body.as_bytes());
            }
            Err(e) => eprintln!("[sdd] action_log_batch open {}: {e}", path.display()),
        }
    }
    Ok(())
}

/// Read up to `tail` most-recent entries from the phase's JSONL log.
/// Used on app boot to rehydrate the live feed for any phase still
/// `running`. Lines that fail to parse are skipped (forward-compat
/// for future schema additions).
#[tauri::command]
pub async fn sdd_read_action_log(
    registry: State<'_, SddRegistry>,
    id: String,
    phase: u32,
    tail: Option<u32>,
) -> Result<Vec<ActionLogEntry>, String> {
    let cell = registry
        .workspaces
        .read()
        .get(&id)
        .cloned()
        .ok_or_else(|| format!("unknown workspace {id}"))?;
    let root = PathBuf::from(cell.read().root.clone());
    let path = action_log_path(&root, phase);
    let raw = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("read action log {}: {e}", path.display())),
    };
    let mut out: Vec<ActionLogEntry> = raw
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<ActionLogEntry>(l).ok())
        .collect();
    if let Some(n) = tail {
        let n = n as usize;
        if out.len() > n {
            let drop = out.len() - n;
            out.drain(..drop);
        }
    }
    Ok(out)
}
