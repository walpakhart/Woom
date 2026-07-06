//! App-wide file logging — `<app_data_dir>/logs/woom.log`.
//!
//! Design goals (Settings → Logs card is the consumer):
//! - `log_line(level, source, msg)` is cheap, thread-safe, and NEVER
//!   panics — any I/O failure falls back to a plain `eprintln!` so
//!   dev consoles still see the message.
//! - Every line is mirrored to stderr regardless, so `pnpm tauri dev`
//!   output is unchanged when call sites migrate from `eprintln!`.
//! - Size-based rotation: when the file crosses 5 MB on a write we
//!   rename it to `woom.log.1` (clobbering any previous `.1`) and
//!   start fresh. Two generations ≈ 10 MB worst case on disk.
//! - The append handle is opened lazily and cached in a global mutex;
//!   `log_clear` / rotation drop it so the next write re-opens.
//!
//! No `log`/`tracing` crates — this is a deliberate zero-dependency
//! sink so the panic hook (which must not allocate a subscriber) and
//! frontend `log_write` share one dumb, reliable code path.

use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

/// Rotate once the live file crosses this size (checked per write).
const MAX_LOG_BYTES: u64 = 5 * 1024 * 1024;
/// Default tail length for `log_tail`.
const DEFAULT_TAIL_LINES: usize = 400;

struct LogState {
    /// Cached append handle; `None` until the first write and after a
    /// clear/rotation (re-opened lazily on the next write).
    file: Option<File>,
    /// Resolved once — either from `init` (Tauri's `app_data_dir`) or
    /// the deterministic macOS fallback when a write happens first.
    path: Option<PathBuf>,
}

static STATE: Mutex<LogState> = Mutex::new(LogState { file: None, path: None });

/// Record the Tauri-resolved app-data dir. Called once from `lib.rs`
/// setup; safe to skip (the fallback path below is identical on any
/// real macOS install).
pub(crate) fn init(app_data_dir: PathBuf) {
    let mut st = lock_state();
    if st.path.is_none() {
        st.path = Some(app_data_dir.join("logs").join("woom.log"));
    }
}

/// `~/Library/Application Support/com.woom.desktop/logs/woom.log` —
/// mirrors Tauri's `app_data_dir()` for our bundle id, so writes that
/// land before `init` (e.g. a panic during startup) go to the same
/// file the Settings card reads.
fn fallback_path() -> PathBuf {
    let base = std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|h| h.join("Library/Application Support/com.woom.desktop"))
        .unwrap_or_else(|| PathBuf::from("/tmp/woom"));
    base.join("logs").join("woom.log")
}

/// Poison-tolerant lock: a panic while holding the lock must not turn
/// every subsequent `log_line` into a second panic.
fn lock_state() -> std::sync::MutexGuard<'static, LogState> {
    STATE.lock().unwrap_or_else(|p| p.into_inner())
}

/// Append one line: `2026-07-06T12:34:56.789Z [LEVEL] [source] msg`.
/// Also mirrored to stderr. Never panics; on any file-system failure
/// the line still reaches stderr.
pub(crate) fn log_line(level: &str, source: &str, msg: &str) {
    let line = format!(
        "{} [{}] [{}] {}",
        iso8601_now_utc(),
        level.to_ascii_uppercase(),
        source,
        msg
    );
    // Mirror to stderr first — this must survive any file error.
    eprintln!("{line}");

    let mut st = lock_state();
    let path = st.path.get_or_insert_with(fallback_path).clone();

    // Rotate if the live file is already over budget.
    if let Ok(meta) = std::fs::metadata(&path) {
        if meta.len() > MAX_LOG_BYTES {
            st.file = None; // drop the handle before renaming under it
            let rotated = path.with_extension("log.1");
            let _ = std::fs::rename(&path, &rotated);
        }
    }

    if st.file.is_none() {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        match std::fs::OpenOptions::new().create(true).append(true).open(&path) {
            Ok(f) => st.file = Some(f),
            Err(e) => {
                eprintln!("[logging] open {path:?} failed: {e}");
                return;
            }
        }
    }
    if let Some(f) = st.file.as_mut() {
        if let Err(e) = writeln!(f, "{line}") {
            // Stale handle (file deleted / volume gone) — drop it so
            // the next write re-opens, and say so once on stderr.
            eprintln!("[logging] write failed: {e}");
            st.file = None;
        }
    }
}

/// Current log-file path (resolving the fallback if `init` hasn't run).
fn current_path() -> PathBuf {
    let mut st = lock_state();
    st.path.get_or_insert_with(fallback_path).clone()
}

// ---- Tauri commands ------------------------------------------------------

/// Frontend sink — console/error hooks in `+page.svelte` funnel here.
#[tauri::command]
pub(crate) fn log_write(level: String, source: String, msg: String) {
    log_line(&level, &source, &msg);
}

#[tauri::command]
pub(crate) fn log_path() -> String {
    current_path().to_string_lossy().into_owned()
}

/// Last `lines` lines (default 400). The live file is capped at 5 MB
/// so a full read + split is cheap; no seek gymnastics needed.
#[tauri::command]
pub(crate) fn log_tail(lines: Option<u32>) -> Result<String, String> {
    let path = current_path();
    let n = lines.map(|x| x as usize).unwrap_or(DEFAULT_TAIL_LINES).max(1);
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(String::new()),
        Err(e) => return Err(format!("read {path:?}: {e}")),
    };
    let all: Vec<&str> = content.lines().collect();
    let start = all.len().saturating_sub(n);
    Ok(all[start..].join("\n"))
}

/// Truncate the live file and delete the rotated generation. The
/// cached handle is dropped so the next `log_line` re-opens fresh.
#[tauri::command]
pub(crate) fn log_clear() -> Result<(), String> {
    let path = {
        let mut st = lock_state();
        st.file = None;
        st.path.get_or_insert_with(fallback_path).clone()
    };
    let _ = std::fs::remove_file(path.with_extension("log.1"));
    match std::fs::remove_file(&path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("clear {path:?}: {e}")),
    }
    log_line("info", "app", "log cleared by user");
    Ok(())
}

/// Reveal the log file in Finder (macOS-only app — `open -R` selects
/// the file inside its enclosing folder).
#[tauri::command]
pub(crate) fn log_reveal() -> Result<(), String> {
    let path = current_path();
    // Make sure there is something to select — a fresh install may
    // not have written a line yet.
    if !path.exists() {
        log_line("info", "app", "log file created for reveal");
    }
    let status = std::process::Command::new("open")
        .arg("-R")
        .arg(&path)
        .status()
        .map_err(|e| format!("spawn open: {e}"))?;
    if !status.success() {
        return Err(format!("open -R exited with status {status}"));
    }
    Ok(())
}

// ---- Timestamp -----------------------------------------------------------

/// `2026-07-06T12:34:56.789Z` from `SystemTime`, no chrono. Uses the
/// standard civil-from-days algorithm (Howard Hinnant) for the date
/// part; proleptic Gregorian, UTC only.
fn iso8601_now_utc() -> String {
    let dur = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();
    let millis = dur.subsec_millis();
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02}T{h:02}:{m:02}:{s:02}.{millis:03}Z")
}

/// Days-since-epoch → (y, m, d). See
/// <https://howardhinnant.github.io/date_algorithms.html#civil_from_days>.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_from_days_epoch_and_leap() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19_723), (2024, 1, 1));
        // 2024-02-29 (leap day): 19723 + 31 + 28 = 19782
        assert_eq!(civil_from_days(19_782), (2024, 2, 29));
        // 2026-07-01: 20454 (2026-01-01) + 181 days (Jan..Jun)
        assert_eq!(civil_from_days(20_635), (2026, 7, 1));
    }

    #[test]
    fn timestamp_shape() {
        let ts = iso8601_now_utc();
        // 2026-07-06T12:34:56.789Z — fixed width 24.
        assert_eq!(ts.len(), 24);
        assert_eq!(&ts[4..5], "-");
        assert_eq!(&ts[10..11], "T");
        assert!(ts.ends_with('Z'));
    }
}
