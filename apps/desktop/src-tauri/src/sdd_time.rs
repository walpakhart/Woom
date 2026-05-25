//! Time + id helpers for SDD. Extracted from `sdd.rs` in wave-1
//! phase-10 refactor. Pure — no Tauri, no FS, no global state — so
//! these are trivially unit-testable and shaved off ~50 LoC from the
//! main file.

use std::time::{SystemTime, UNIX_EPOCH};

use uuid::Uuid;

/// Wall-clock millis since epoch. Used for `created_at`/`updated_at`
/// timestamps on workspaces + audit log entries.
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Short, sortable workspace id with a fixed `sdd-` prefix so a
/// directory listing under `<app_data>/sdd-workspaces/` is unambiguous
/// and the user can pick them out of a generic `ls` without guessing.
pub fn short_id() -> String {
    let s = Uuid::new_v4().simple().to_string();
    format!("sdd-{}", &s[..10])
}

/// Coarse YYYY-MM-DD formatter — we don't take a `chrono` dep just
/// for "created today" tracking. Ignores the input ms entirely and
/// stamps `now` because every caller is asking for the current date
/// anyway (the param exists for a later refactor that wants per-ms
/// dates without a signature change).
#[allow(dead_code)]
pub fn format_iso(_ms: u64) -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = secs / 86_400;
    let (y, m, d) = days_to_ymd(days as i64);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Civil date arithmetic from days-since-epoch. Algorithm by
/// Howard Hinnant — handles negative inputs (pre-1970) and
/// gregorian-correct leap years. We keep it inline because pulling
/// chrono adds a meaningful binary-size cost for one formatter.
pub fn days_to_ymd(z: i64) -> (i32, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m, d)
}
