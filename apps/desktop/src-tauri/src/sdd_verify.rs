//! SDD acceptance-criteria verifier.
//!
//! Each phase in `plan.json` may carry an `acceptance: []` array of
//! machine-verifiable checks. This module runs them and persists per-
//! phase results into `<workspace>/results/phase-N-acceptance.json`
//! so the SddCard can render per-criterion pass/fail dots without a
//! re-run.
//!
//! Three check shapes:
//!   - `shell` — run a command, compare exit code, optionally regex-
//!     match stdout. Streams via `tokio::process::Command`. Caps the
//!     captured output at `LOG_TAIL_BYTES` so we never blow up the
//!     JSON file with multi-MB compiler logs.
//!   - `file_exists` — synchronous existence check on a list of repo-
//!     relative paths. One missing entry fails the whole check; the
//!     log_tail enumerates the missing paths so the UI can surface
//!     them in the "▸ files: 3 paths" expander.
//!   - `manual` — the verifier records `manual_unmarked` and does NOT
//!     block phase advancement on its own. A separate Tauri command
//!     (`sdd_mark_manual_check`) lets the user flip it to passed/failed
//!     after eyeballing whatever it asked them to check.
//!
//! Aggregate decision (`overall_status`):
//!   - any non-manual `failed` → `failed`
//!   - all non-manual `passed` AND any `manual_unmarked` → `manual_pending`
//!   - all `passed` (incl. resolved manuals) → `passed`
//!   - empty acceptance list → `passed` (nothing to check, trivially OK)
//!
//! The orchestrator (sdd.rs) consumes `overall_status` to decide
//! whether to flip phase frontmatter to `done` or `failed`.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::sdd::{read_plan_json, SddPhaseAcceptance};

/// Cap on captured stdout/stderr per shell check. Keeps the JSON
/// payload tractable — the UI's expandable log shows the tail anyway.
const LOG_TAIL_BYTES: usize = 4096;

/// Default per-check shell timeout when the plan didn't specify one.
const DEFAULT_TIMEOUT_MS: u64 = 120_000;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    /// Verifier hasn't reached this check yet (only seen if a partial
    /// run wrote results — current code runs sequentially-then-flush
    /// so this is reserved for future incremental writes).
    Pending,
    /// Shell / file_exists check passed. Manual checks land here only
    /// after `sdd_mark_manual_check(..., passed = true)`.
    Passed,
    /// Shell / file_exists check failed. Manual checks land here when
    /// the user explicitly marks them failed.
    Failed,
    /// User opted to skip this check (reserved — not wired in this
    /// phase, but the discriminant is stable so future runs don't
    /// break the JSON).
    Skipped,
    /// Manual check, no user verdict yet. Doesn't block phase
    /// advancement under the orchestrator's "manual_pending" branch.
    ManualUnmarked,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct AcceptanceResult {
    /// Index into the phase's `acceptance` array in plan.json. Stable
    /// across reruns as long as the plan structure doesn't change.
    pub check_index: usize,
    /// Snake-cased discriminant of the source check (`shell` /
    /// `file_exists` / `manual`). Lets the UI render the right kind
    /// without re-reading plan.json.
    pub kind: String,
    pub status: CheckStatus,
    /// Unix-ms when this check started. Zero for not-yet-run checks.
    pub started_at: u64,
    /// Unix-ms when this check finished. Zero for not-yet-run.
    pub finished_at: u64,
    /// Shell exit code; None for non-shell or for checks that timed
    /// out / were killed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    /// Up to LOG_TAIL_BYTES of merged stdout/stderr. For file_exists
    /// failures, contains a `\n`-joined list of missing paths.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub log_tail: String,
    /// Free-form note (timeout reason, regex mismatch detail, etc).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum OverallStatus {
    /// Empty acceptance list, OR every non-manual check passed AND
    /// every manual check was explicitly marked passed.
    Passed,
    /// At least one non-manual check failed (or a manual check was
    /// explicitly marked failed). The orchestrator treats this as a
    /// blocker for phase `done`.
    Failed,
    /// All non-manual checks passed but at least one manual check is
    /// still `manual_unmarked`. UI shows yellow indicator; phase does
    /// NOT auto-advance.
    ManualPending,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct PhaseAcceptanceFile {
    pub phase: u32,
    pub overall_status: OverallStatus,
    /// Unix-ms timestamps spanning the full verifier run. `started_at`
    /// is when the orchestrator kicked off the first check; `finished_at`
    /// when the last sequential check returned.
    pub started_at: u64,
    pub finished_at: u64,
    pub results: Vec<AcceptanceResult>,
}

fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn results_path(workspace_root: &Path, phase_number: u32) -> PathBuf {
    workspace_root
        .join("results")
        .join(format!("phase-{phase_number}-acceptance.json"))
}

/// Read the previously-persisted verifier output for a phase, if any.
/// Used by the UI to render last-run state without rerunning, and by
/// the orchestrator's race-guard to skip a redundant verify.
pub fn read_phase_acceptance(
    workspace_root: &Path,
    phase_number: u32,
) -> Option<PhaseAcceptanceFile> {
    let raw = std::fs::read_to_string(results_path(workspace_root, phase_number)).ok()?;
    serde_json::from_str(&raw).ok()
}

/// Atomic write through `<file>.tmp` + rename — never leaves the
/// reader observing a torn JSON.
fn write_phase_acceptance(
    workspace_root: &Path,
    file: &PhaseAcceptanceFile,
) -> Result<(), String> {
    let path = results_path(workspace_root, file.phase);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
    }
    let body = serde_json::to_string_pretty(file).map_err(|e| format!("serialize: {e}"))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, body).map_err(|e| format!("write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("rename: {e}"))?;
    Ok(())
}

/// Trim a string to the LAST `LOG_TAIL_BYTES` bytes, snapping to a
/// UTF-8 boundary. Used so the agent's pretty unicode-bordered log
/// frames don't bleed mid-codepoint into the JSON.
fn tail_utf8(buf: &str) -> String {
    if buf.len() <= LOG_TAIL_BYTES {
        return buf.to_string();
    }
    let start = buf.len() - LOG_TAIL_BYTES;
    let mut idx = start;
    while idx < buf.len() && !buf.is_char_boundary(idx) {
        idx += 1;
    }
    buf[idx..].to_string()
}

/// Run a single shell check. Inheritable env, runs through `sh -c`
/// (same launcher pattern as bg_tasks::spawn) so the agent can write
/// pipe-y commands like `cargo test 2>&1 | tee log.txt`.
///
/// Returns the merged stdout+stderr tail and the final exit code.
/// Timeout: per-check `timeout_ms` if specified, else DEFAULT_TIMEOUT_MS.
pub async fn run_shell_check(
    cwd: &Path,
    cmd: &str,
    expect_exit: i32,
    stdout_match: Option<&str>,
    timeout_ms: Option<u64>,
    check_index: usize,
) -> AcceptanceResult {
    use tokio::io::AsyncReadExt;
    use tokio::process::Command;

    let started_at = now_ms();
    let timeout = Duration::from_millis(timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS));

    let mut command = Command::new("sh");
    command
        .arg("-c")
        .arg(cmd)
        .current_dir(cwd)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .env("NO_COLOR", "1")
        .env("PAGER", "cat")
        .env("CI", "1");

    let mut child = match command.spawn() {
        Ok(c) => c,
        Err(e) => {
            return AcceptanceResult {
                check_index,
                kind: "shell".into(),
                status: CheckStatus::Failed,
                started_at,
                finished_at: now_ms(),
                exit_code: None,
                log_tail: String::new(),
                note: format!("spawn failed: {e}"),
            };
        }
    };

    let mut stdout = child.stdout.take();
    let mut stderr = child.stderr.take();

    // Read both streams concurrently so a chatty stderr can't deadlock
    // the wait by filling the OS pipe buffer.
    let read_streams = async {
        let mut out = String::new();
        let mut err = String::new();
        let stdout_fut = async {
            if let Some(s) = stdout.as_mut() {
                let _ = s.read_to_string(&mut out).await;
            }
        };
        let stderr_fut = async {
            if let Some(s) = stderr.as_mut() {
                let _ = s.read_to_string(&mut err).await;
            }
        };
        tokio::join!(stdout_fut, stderr_fut);
        let merged = if err.is_empty() {
            out
        } else if out.is_empty() {
            err
        } else {
            format!("{out}\n--- stderr ---\n{err}")
        };
        merged
    };

    let waited = tokio::time::timeout(timeout, async {
        let merged = read_streams.await;
        let status = child.wait().await;
        (merged, status)
    })
    .await;

    let (merged, status) = match waited {
        Ok((m, Ok(s))) => (m, Some(s)),
        Ok((m, Err(_))) => (m, None),
        Err(_) => {
            // Timeout — kill child best-effort. tokio's child.kill() is
            // async but we don't care about the outcome.
            let _ = child.start_kill();
            return AcceptanceResult {
                check_index,
                kind: "shell".into(),
                status: CheckStatus::Failed,
                started_at,
                finished_at: now_ms(),
                exit_code: None,
                log_tail: String::new(),
                note: format!("timeout after {} ms", timeout.as_millis()),
            };
        }
    };

    let exit_code = status.as_ref().and_then(|s| s.code());
    let log_tail = tail_utf8(&merged);

    let exit_match = exit_code == Some(expect_exit);
    // Substring match on the merged output. We deliberately avoid a
    // regex dep — substring is enough for the common "test output
    // contained the success word" check, and avoids pulling `regex`
    // into Cargo.toml. If a future check truly needs regex, swap this
    // branch + add the dep.
    let stdout_ok = match stdout_match {
        None => true,
        Some(needle) => merged.contains(needle),
    };

    let (status, note) = match (exit_match, stdout_ok) {
        (true, true) => (CheckStatus::Passed, String::new()),
        (false, _) => (
            CheckStatus::Failed,
            format!(
                "exit {} != expected {expect_exit}",
                exit_code.map(|c| c.to_string()).unwrap_or_else(|| "?".into())
            ),
        ),
        (_, false) => (
            CheckStatus::Failed,
            format!("stdout did not contain {:?}", stdout_match.unwrap_or("")),
        ),
    };

    AcceptanceResult {
        check_index,
        kind: "shell".into(),
        status,
        started_at,
        finished_at: now_ms(),
        exit_code,
        log_tail,
        note,
    }
}

/// Run a single file_exists check. Synchronous; returns instantly.
pub fn run_file_check(
    workspace_root: &Path,
    paths: &[String],
    check_index: usize,
) -> AcceptanceResult {
    let started_at = now_ms();
    let mut missing: Vec<String> = Vec::new();
    for p in paths {
        let full = workspace_root.join(p);
        if !full.exists() {
            missing.push(p.clone());
        }
    }
    let (status, note, log_tail) = if missing.is_empty() {
        (CheckStatus::Passed, String::new(), String::new())
    } else {
        let count = missing.len();
        (
            CheckStatus::Failed,
            format!("{count} missing path(s)"),
            missing.join("\n"),
        )
    };
    AcceptanceResult {
        check_index,
        kind: "file_exists".into(),
        status,
        started_at,
        finished_at: now_ms(),
        exit_code: None,
        log_tail,
        note,
    }
}

/// "Run" a manual check — i.e. record it as `manual_unmarked` so the
/// UI knows to surface a Mark-passed/Mark-failed pair of buttons.
pub fn run_manual_check(description: &str, check_index: usize) -> AcceptanceResult {
    AcceptanceResult {
        check_index,
        kind: "manual".into(),
        status: CheckStatus::ManualUnmarked,
        started_at: now_ms(),
        finished_at: now_ms(),
        exit_code: None,
        log_tail: String::new(),
        note: description.to_string(),
    }
}

/// Aggregate per-check results into the orchestrator-facing verdict.
pub fn aggregate(results: &[AcceptanceResult]) -> OverallStatus {
    if results.is_empty() {
        return OverallStatus::Passed;
    }
    let mut any_manual_unmarked = false;
    for r in results {
        match (&r.kind[..], &r.status) {
            (_, CheckStatus::Failed) => return OverallStatus::Failed,
            ("manual", CheckStatus::ManualUnmarked) => any_manual_unmarked = true,
            _ => {}
        }
    }
    if any_manual_unmarked {
        OverallStatus::ManualPending
    } else {
        OverallStatus::Passed
    }
}

/// Top-level entry point. Reads plan.json, finds `phase_number`, runs
/// every acceptance check sequentially, persists `phase-N-acceptance.json`,
/// returns the structured file. Caller (sdd.rs Tauri command) decides
/// whether to flip phase frontmatter status based on the verdict.
pub async fn verify_phase(
    workspace_root: &Path,
    phase_number: u32,
) -> Result<PhaseAcceptanceFile, String> {
    let started_at = now_ms();
    let plan = read_plan_json(workspace_root)
        .ok_or_else(|| format!("plan.json missing for {}", workspace_root.display()))?;
    let phase = plan
        .phases
        .iter()
        .find(|p| p.number == phase_number)
        .ok_or_else(|| format!("phase {phase_number} not in plan.json"))?;

    let mut results: Vec<AcceptanceResult> = Vec::with_capacity(phase.acceptance.len());
    for (idx, check) in phase.acceptance.iter().enumerate() {
        let res = match check {
            SddPhaseAcceptance::Shell {
                cmd,
                expect_exit,
                stdout_match,
                timeout_ms,
            } => {
                run_shell_check(
                    workspace_root,
                    cmd,
                    *expect_exit,
                    stdout_match.as_deref(),
                    *timeout_ms,
                    idx,
                )
                .await
            }
            SddPhaseAcceptance::FileExists { paths } => {
                run_file_check(workspace_root, paths, idx)
            }
            SddPhaseAcceptance::Manual { description } => run_manual_check(description, idx),
        };
        results.push(res);
    }

    let overall_status = aggregate(&results);
    let file = PhaseAcceptanceFile {
        phase: phase_number,
        overall_status,
        started_at,
        finished_at: now_ms(),
        results,
    };
    write_phase_acceptance(workspace_root, &file)?;
    Ok(file)
}

/// Flip a single manual check's status to passed/failed and rewrite
/// the persisted JSON. Returns the updated file so the caller can
/// emit a `sdd:changed:<id>` event.
pub fn mark_manual_check(
    workspace_root: &Path,
    phase_number: u32,
    check_index: usize,
    passed: bool,
) -> Result<PhaseAcceptanceFile, String> {
    let mut file = read_phase_acceptance(workspace_root, phase_number)
        .ok_or_else(|| format!("no acceptance results for phase {phase_number}"))?;
    let result = file
        .results
        .iter_mut()
        .find(|r| r.check_index == check_index)
        .ok_or_else(|| format!("check_index {check_index} out of range"))?;
    if result.kind != "manual" {
        return Err(format!(
            "check {check_index} is `{}`, not manual",
            result.kind
        ));
    }
    result.status = if passed { CheckStatus::Passed } else { CheckStatus::Failed };
    result.finished_at = now_ms();
    file.overall_status = aggregate(&file.results);
    write_phase_acceptance(workspace_root, &file)?;
    Ok(file)
}

// ---------------------------------------------------------------------------
// Tests — exercise the runner functions and the aggregator without
// going through Tauri or the registry.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Per-test scratch dir under $TMPDIR + a (pid, counter) suffix so
    /// concurrent test runners don't collide. Mirrors the helper in
    /// `sdd.rs::tests::tempdir` — kept inline so the verifier module
    /// has no external test deps.
    fn td() -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let dir = std::env::temp_dir().join(format!("woom-sdd-verify-test-{pid}-{n}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("mkdir td");
        dir
    }

    #[tokio::test]
    async fn shell_check_passes_on_zero_exit() {
        let dir = td();
        let res = run_shell_check(&dir, "true", 0, None, None, 0).await;
        assert_eq!(res.status, CheckStatus::Passed, "{res:?}");
        assert_eq!(res.exit_code, Some(0));
        assert_eq!(res.kind, "shell");
    }

    #[tokio::test]
    async fn shell_check_fails_on_nonzero_exit() {
        let dir = td();
        let res = run_shell_check(&dir, "false", 0, None, None, 1).await;
        assert_eq!(res.status, CheckStatus::Failed);
        assert_eq!(res.exit_code, Some(1));
        assert!(res.note.contains("expected 0"));
    }

    #[tokio::test]
    async fn shell_check_substring_match() {
        let dir = td();
        let res = run_shell_check(
            &dir,
            "echo hello-world",
            0,
            Some("hello-"),
            None,
            0,
        )
        .await;
        assert_eq!(res.status, CheckStatus::Passed);
    }

    #[tokio::test]
    async fn shell_check_substring_mismatch() {
        let dir = td();
        let res = run_shell_check(&dir, "echo nope", 0, Some("yes"), None, 0).await;
        assert_eq!(res.status, CheckStatus::Failed);
        assert!(res.note.contains("did not contain"), "{}", res.note);
    }

    #[tokio::test]
    async fn shell_check_timeout() {
        let dir = td();
        let res = run_shell_check(&dir, "sleep 5", 0, None, Some(50), 0).await;
        assert_eq!(res.status, CheckStatus::Failed);
        assert!(res.note.contains("timeout"));
    }

    #[test]
    fn file_check_passes_when_paths_exist() {
        let dir = td();
        std::fs::write(&dir.join("a.txt"), "x").unwrap();
        std::fs::create_dir_all(&dir.join("sub")).unwrap();
        std::fs::write(&dir.join("sub/b.txt"), "y").unwrap();
        let res = run_file_check(
            &dir,
            &["a.txt".into(), "sub/b.txt".into()],
            0,
        );
        assert_eq!(res.status, CheckStatus::Passed);
    }

    #[test]
    fn file_check_fails_when_path_missing() {
        let dir = td();
        std::fs::write(&dir.join("a.txt"), "x").unwrap();
        let res = run_file_check(
            &dir,
            &["a.txt".into(), "missing.txt".into()],
            0,
        );
        assert_eq!(res.status, CheckStatus::Failed);
        assert!(res.log_tail.contains("missing.txt"));
    }

    #[test]
    fn manual_check_records_unmarked() {
        let res = run_manual_check("eyeball UI", 7);
        assert_eq!(res.status, CheckStatus::ManualUnmarked);
        assert_eq!(res.kind, "manual");
        assert_eq!(res.check_index, 7);
        assert_eq!(res.note, "eyeball UI");
    }

    #[test]
    fn aggregate_empty_is_passed() {
        assert_eq!(aggregate(&[]), OverallStatus::Passed);
    }

    #[test]
    fn aggregate_failure_dominates() {
        let r = [
            AcceptanceResult {
                check_index: 0,
                kind: "shell".into(),
                status: CheckStatus::Passed,
                started_at: 0,
                finished_at: 0,
                exit_code: Some(0),
                log_tail: String::new(),
                note: String::new(),
            },
            AcceptanceResult {
                check_index: 1,
                kind: "shell".into(),
                status: CheckStatus::Failed,
                started_at: 0,
                finished_at: 0,
                exit_code: Some(1),
                log_tail: String::new(),
                note: String::new(),
            },
        ];
        assert_eq!(aggregate(&r), OverallStatus::Failed);
    }

    #[test]
    fn aggregate_manual_pending_when_only_manual_unmarked() {
        let r = [
            AcceptanceResult {
                check_index: 0,
                kind: "shell".into(),
                status: CheckStatus::Passed,
                started_at: 0,
                finished_at: 0,
                exit_code: Some(0),
                log_tail: String::new(),
                note: String::new(),
            },
            AcceptanceResult {
                check_index: 1,
                kind: "manual".into(),
                status: CheckStatus::ManualUnmarked,
                started_at: 0,
                finished_at: 0,
                exit_code: None,
                log_tail: String::new(),
                note: "check it".into(),
            },
        ];
        assert_eq!(aggregate(&r), OverallStatus::ManualPending);
    }

    #[tokio::test]
    async fn verify_phase_writes_json_and_aggregates() {
        use crate::sdd::{SddPlanFile, SddPlanPhase};
        let dir = td();
        // Seed plan.json with a phase that has one passing shell check
        // and one file_exists check that will fail.
        let plan = SddPlanFile {
            version: 1,
            phases: vec![SddPlanPhase {
                number: 1,
                slug: "01-foo".into(),
                title: "Foo".into(),
                depends_on: vec![],
                complexity: None,
                acceptance: vec![
                    SddPhaseAcceptance::Shell {
                        cmd: "true".into(),
                        expect_exit: 0,
                        stdout_match: None,
                        timeout_ms: Some(2000),
                    },
                    SddPhaseAcceptance::FileExists {
                        paths: vec!["does-not-exist.txt".into()],
                    },
                ],
            }],
        };
        let plan_json = serde_json::to_string_pretty(&plan).unwrap();
        std::fs::write(&dir.join("plan.json"), plan_json).unwrap();

        let file = verify_phase(&dir, 1).await.expect("verify");
        assert_eq!(file.overall_status, OverallStatus::Failed);
        assert_eq!(file.results.len(), 2);
        assert_eq!(file.results[0].status, CheckStatus::Passed);
        assert_eq!(file.results[1].status, CheckStatus::Failed);
        // Persisted file matches.
        let persisted = read_phase_acceptance(&dir, 1).expect("persisted");
        assert_eq!(persisted.overall_status, OverallStatus::Failed);
    }

    #[tokio::test]
    async fn verify_phase_empty_acceptance_passes() {
        use crate::sdd::{SddPlanFile, SddPlanPhase};
        let dir = td();
        let plan = SddPlanFile {
            version: 1,
            phases: vec![SddPlanPhase {
                number: 1,
                slug: "01-foo".into(),
                title: "Foo".into(),
                depends_on: vec![],
                complexity: None,
                acceptance: vec![],
            }],
        };
        std::fs::write(
            &dir.join("plan.json"),
            serde_json::to_string_pretty(&plan).unwrap(),
        )
        .unwrap();
        let file = verify_phase(&dir, 1).await.expect("verify");
        assert_eq!(file.overall_status, OverallStatus::Passed);
        assert!(file.results.is_empty());
    }

    #[test]
    fn mark_manual_check_flips_status_and_aggregate() {
        use std::fs;
        let dir = td();
        fs::create_dir_all(&dir.join("results")).unwrap();
        let initial = PhaseAcceptanceFile {
            phase: 1,
            overall_status: OverallStatus::ManualPending,
            started_at: 0,
            finished_at: 0,
            results: vec![
                AcceptanceResult {
                    check_index: 0,
                    kind: "shell".into(),
                    status: CheckStatus::Passed,
                    started_at: 0,
                    finished_at: 0,
                    exit_code: Some(0),
                    log_tail: String::new(),
                    note: String::new(),
                },
                AcceptanceResult {
                    check_index: 1,
                    kind: "manual".into(),
                    status: CheckStatus::ManualUnmarked,
                    started_at: 0,
                    finished_at: 0,
                    exit_code: None,
                    log_tail: String::new(),
                    note: "verify visual".into(),
                },
            ],
        };
        write_phase_acceptance(&dir, &initial).unwrap();

        let updated = mark_manual_check(&dir, 1, 1, true).expect("mark");
        assert_eq!(updated.overall_status, OverallStatus::Passed);
        assert_eq!(updated.results[1].status, CheckStatus::Passed);
    }
}
