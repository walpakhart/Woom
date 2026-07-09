//! Ledger workflows — sequential, machine-checked task execution.
//!
//! A Ledger runs a CHECKLIST of discrete requirements sequentially in
//! ONE shared worktree:
//!
//!   - every item carries a *verification command* (shell, exit 0 =
//!     pass) — "done" is an objective machine verdict, not LLM opinion;
//!     items without a command fall back to a fresh-context LLM grader
//!     that never shares context with the worker ("the agent doing the
//!     work isn't the one grading it");
//!   - each item is executed by a FRESH `claude -p` context (state
//!     lives in the worktree + the ledger file, not in a long chat) so
//!     context rot can't accumulate across items;
//!   - a passed item is committed in the worktree (`ledger: <title>`),
//!     giving per-item diffs + a clean retry boundary;
//!   - a failed check re-runs the worker with the check output as
//!     feedback, up to `max_attempts`; exhaustion stops the line and
//!     parks the workflow for the user (retry / skip / cancel);
//!   - when every item passes the workflow parks in `awaiting_review`
//!     with the full branch diff — the approval gate is BEHAVIOR
//!     (diff + green checks), not a markdown document.
//!
//! Storage: in-memory registry + one JSON per workflow under
//! `<app_data>/ledgers/`, every transition flowing through
//! `mutate_persist` and emitted as `ledger:*` events.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::claude_quota;
use crate::worktree;

/// Hard cap on checklist length — past this the task should be split
/// into several ledgers.
pub const MAX_ITEMS: usize = 30;
/// Per-item worker retry ceiling (first run + retries).
pub const DEFAULT_MAX_ATTEMPTS: u32 = 3;
/// Ceiling for one shell check run. Test suites routinely take
/// minutes; anything past this is a hung check, not a slow one.
const CHECK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);
/// Default hard wall-clock cap for one worker attempt.
const ITEM_TIMEOUT_SECS: u64 = 600;
/// Default no-stream-activity window before a worker attempt is stalled.
const STALL_TIMEOUT_SECS: u64 = 120;
/// Tail of check output kept on the item (feedback prompt + card UI).
const CHECK_OUTPUT_TAIL: usize = 4_000;
/// Per-item budget headroom. The whole-run cap scales with checklist
/// length (`budget_for`) so a healthy multi-item run NEVER nags — the
/// cap only trips on genuine pathology (a single item looping, or a
/// wild overrun). Opus items routinely run $20-35 each, so this sits
/// well above that. When the cap IS crossed the run PAUSES
/// (`paused_budget`), not dies: raise the cap and resume, committed
/// items kept.
pub const PER_ITEM_BUDGET_USD: f64 = 60.0;

/// Whole-run cap for a checklist of `n` items (min one item's worth).
fn budget_for(n: usize) -> f64 {
    (n.max(1) as f64) * PER_ITEM_BUDGET_USD
}

fn default_budget_cap() -> f64 {
    PER_ITEM_BUDGET_USD
}

fn default_model() -> String {
    "claude-opus-4-8".to_string()
}

fn default_max_attempts() -> u32 {
    DEFAULT_MAX_ATTEMPTS
}

/// Apply as ONE clean commit by default — a user reviewing the branch
/// doesn't want a dozen `ledger: <item>` commits. Opt out per-workflow
/// (`squash = false`) to keep the granular per-item history.
fn default_squash() -> bool {
    true
}
fn default_true() -> bool {
    true
}

// ---- Serde types (mirror the TS shapes in `lib/types.ts`) -----------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LedgerItem {
    pub id: String,
    /// One-line requirement ("what must become true").
    pub title: String,
    /// Optional expanded description handed to the worker verbatim.
    #[serde(default)]
    pub detail: Option<String>,
    /// Shell verification command run in the worktree; exit 0 = pass.
    /// `None` → fresh-context LLM grader.
    #[serde(default)]
    pub check_cmd: Option<String>,
    /// 'queued' | 'working' | 'checking' | 'passed' | 'failed' | 'skipped'
    pub status: String,
    #[serde(default)]
    pub attempts: u32,
    #[serde(default = "default_max_attempts")]
    pub max_attempts: u32,
    /// Incremental diff of this item's commit alone.
    #[serde(default)]
    pub diff: Option<String>,
    #[serde(default)]
    pub commit_sha: Option<String>,
    /// Tail of the failing/passing check output (or grader reason).
    #[serde(default)]
    pub check_output: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub tokens_in: u64,
    #[serde(default)]
    pub tokens_out: u64,
    #[serde(default)]
    pub cost_usd: f64,
    /// Durable learnings the worker left for subsequent items ("NOTES:"
    /// tail of its reply) — injected into every later worker prompt so
    /// knowledge chains across fresh contexts.
    #[serde(default)]
    pub notes: Option<String>,
    /// Marked parallel-safe by the builder: consecutive parallel items
    /// form a wave executed concurrently in isolated worktrees, diffs
    /// merged back in item order. Failures degrade to sequential retry.
    #[serde(default)]
    pub parallel: bool,
    /// Explicit dependency edges — ids of items that must be `passed` or
    /// `skipped` before this one becomes eligible. Empty (the default) =
    /// the item is gated only by its position in the list, so an all-
    /// empty checklist runs exactly like the classic linear ledger. Deps
    /// let the builder express a DAG: fan-out items sharing a prerequisite,
    /// a join item that waits on several branches, etc.
    #[serde(default)]
    pub deps: Vec<String>,
    /// Parent item id for sub-items. An item that is the `parent_id` of
    /// ≥1 other item is a CONTAINER (a grouping header): it is never
    /// executed by a worker — its "work" is its children — and its status
    /// is rolled up from them (passed once all children settle). Leaf
    /// items (the common case) have `None`. Enables arbitrary-depth
    /// nesting via the parent chain without a tree data structure.
    #[serde(default)]
    pub parent_id: Option<String>,
    /// Live action feed for the current attempt (tool calls streamed
    /// from the worker CLI). Capped; reset per attempt.
    #[serde(default)]
    pub feed: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LedgerWorkflow {
    pub id: String,
    pub session_id: String,
    /// The overall task this checklist decomposes.
    pub task: String,
    /// Durable plan / contract (markdown) the agent authors while
    /// building — the "what we're doing and why" that survives context
    /// resets. Rendered at the top of the card, editable pre-run. Empty
    /// until the agent calls `ledger_set_plan`.
    #[serde(default)]
    pub plan: String,
    /// Integration "Janitor" gate — a shell command (build + test) run
    /// ONCE against the whole branch after every item passes, before the
    /// review gate lets you apply. `None` = no gate. Set via
    /// `ledger_set_final_check`.
    #[serde(default)]
    pub final_check: Option<String>,
    /// Result of the last final-check run. Defaults true so a workflow
    /// with NO gate applies freely; flipped by the run loop / recheck.
    #[serde(default = "default_true")]
    pub final_check_ok: bool,
    /// Captured stdout/stderr tail of the last final-check run.
    #[serde(default)]
    pub final_check_output: Option<String>,
    /// 'building' | 'awaiting_launch' | 'running' | 'paused_quota' |
    /// 'paused' (user) | 'paused_budget' (cap hit) | 'awaiting_review' |
    /// 'done' | 'failed' | 'cancelled'
    pub status: String,
    pub items: Vec<LedgerItem>,
    #[serde(default)]
    pub current_item: Option<String>,
    #[serde(default)]
    pub worktree_path: Option<String>,
    #[serde(default)]
    pub branch: Option<String>,
    /// Parent HEAD sha the worktree branched from — full-diff base.
    #[serde(default)]
    pub base_sha: Option<String>,
    /// Cumulative branch diff, captured when all items pass.
    #[serde(default)]
    pub full_diff: Option<String>,
    #[serde(default)]
    pub applied: bool,
    /// Apply as one squashed commit (true) vs preserving per-item
    /// commits + a merge commit (false). Surfaced as a toggle on the
    /// review gate.
    #[serde(default = "default_squash")]
    pub squash: bool,
    /// User steering notes queued mid-run. Drained into the next worker
    /// prompt (next attempt or next item) so the human can nudge a
    /// running ledger without editing items or restarting.
    #[serde(default)]
    pub injections: Vec<String>,
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default)]
    pub total_cost_usd: f64,
    /// Spend ceiling; run pauses (`paused_budget`) when crossed. Raised
    /// on resume. Per-workflow so a big redesign can opt into more.
    #[serde(default = "default_budget_cap")]
    pub budget_cap_usd: f64,
    /// Optional per-workflow worker wall-clock cap (seconds). None → ITEM_TIMEOUT_SECS.
    #[serde(default)]
    pub item_timeout_secs: Option<u64>,
    pub created_at: i64,
    #[serde(default)]
    pub started_at: Option<i64>,
    #[serde(default)]
    pub completed_at: Option<i64>,
    #[serde(default)]
    pub parent_cwd: Option<String>,
}

// ---- Registry + persistence ------------------------------------------------

#[derive(Default)]
pub struct LedgerRegistry {
    workflows: Mutex<HashMap<String, LedgerWorkflow>>,
}

impl LedgerRegistry {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }
    fn upsert(&self, wf: LedgerWorkflow) {
        if let Ok(mut g) = self.workflows.lock() {
            g.insert(wf.id.clone(), wf);
        }
    }
    fn get(&self, id: &str) -> Option<LedgerWorkflow> {
        self.workflows.lock().ok().and_then(|g| g.get(id).cloned())
    }
    fn mutate<F: FnOnce(&mut LedgerWorkflow)>(&self, id: &str, f: F) -> Option<LedgerWorkflow> {
        let mut g = self.workflows.lock().ok()?;
        let wf = g.get_mut(id)?;
        f(wf);
        Some(wf.clone())
    }
    pub(crate) fn mutate_persist<F: FnOnce(&mut LedgerWorkflow)>(
        &self,
        app: &AppHandle,
        id: &str,
        f: F,
    ) -> Option<LedgerWorkflow> {
        let updated = self.mutate(id, f);
        if let Some(w) = &updated {
            persist_workflow(app, w);
        }
        updated
    }
    fn all(&self) -> Vec<LedgerWorkflow> {
        self.workflows
            .lock()
            .map(|g| g.values().cloned().collect())
            .unwrap_or_default()
    }
    /// Mutate + emit WITHOUT the disk write — for high-frequency feed
    /// entries where persisting every line would thrash the disk. The
    /// next `mutate_persist` (item transition) captures the state.
    fn mutate_emit<F: FnOnce(&mut LedgerWorkflow)>(&self, app: &AppHandle, id: &str, f: F) {
        if let Some(w) = self.mutate(id, f) {
            let _ = app.emit("ledger:updated", &w);
        }
    }
}

pub(crate) fn ledger_storage_root(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_data_dir().ok().map(|p| p.join("ledgers"))
}

fn ledger_path(root: &Path, id: &str) -> PathBuf {
    let safe = id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect::<String>();
    root.join(format!("{}.json", safe))
}

fn atomic_write(target: &Path, data: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let parent = target
        .parent()
        .ok_or_else(|| std::io::Error::other("target has no parent"))?;
    std::fs::create_dir_all(parent)?;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp = parent.join(format!(
        ".{}.woom-tmp.{}.{}",
        target.file_name().and_then(|s| s.to_str()).unwrap_or("woom"),
        std::process::id(),
        nanos
    ));
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(data)?;
        f.sync_all().ok();
    }
    std::fs::rename(&tmp, target)?;
    Ok(())
}

pub(crate) fn persist_workflow(app: &AppHandle, wf: &LedgerWorkflow) {
    let root = match ledger_storage_root(app) {
        Some(r) => r,
        None => return,
    };
    let bytes = match serde_json::to_vec_pretty(wf) {
        Ok(b) => b,
        Err(e) => {
            crate::logging::log_line(
                "error",
                "ledger",
                &format!("serialize {} failed: {}", wf.id, e),
            );
            return;
        }
    };
    if let Err(e) = atomic_write(&ledger_path(&root, &wf.id), &bytes) {
        crate::logging::log_line(
            "error",
            "ledger",
            &format!("persist {} failed: {}", wf.id, e),
        );
    }
}

pub(crate) fn load_workflows(root: &Path) -> Vec<LedgerWorkflow> {
    let entries = match std::fs::read_dir(root) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        if path
            .file_name()
            .and_then(|s| s.to_str())
            .map(|s| s.starts_with('.'))
            .unwrap_or(false)
        {
            continue;
        }
        match std::fs::read(&path).map_err(|e| e.to_string()).and_then(|b| {
            serde_json::from_slice::<LedgerWorkflow>(&b).map_err(|e| e.to_string())
        }) {
            Ok(wf) => out.push(wf),
            Err(e) => crate::logging::log_line(
                "error",
                "ledger",
                &format!("load {:?} failed: {}", path, e),
            ),
        }
    }
    out
}

/// Interrupted `running` workflows can't resume mid-oneshot after a
/// process death — mark them failed on boot so the card offers retry.
pub(crate) async fn recover_on_startup(app: AppHandle) {
    let root = match ledger_storage_root(&app) {
        Some(r) => r,
        None => return,
    };
    let registry: tauri::State<'_, Arc<LedgerRegistry>> = app.state();
    for mut wf in load_workflows(&root) {
        if matches!(wf.status.as_str(), "running" | "paused_quota" | "building") {
            wf.status = "failed".to_string();
            wf.current_item = None;
            for it in wf.items.iter_mut() {
                if matches!(it.status.as_str(), "working" | "checking") {
                    it.status = "failed".to_string();
                    it.error = Some("interrupted by app restart".into());
                }
            }
            wf.completed_at = Some(unix_ms());
            persist_workflow(&app, &wf);
        }
        registry.inner().upsert(wf);
    }
}

// ---- Git helpers (worktree-scoped) -----------------------------------------

fn git_in(dir: &str) -> std::process::Command {
    let mut c = std::process::Command::new("git");
    c.current_dir(dir);
    c.env("GIT_TERMINAL_PROMPT", "0");
    c
}

fn run_git(mut cmd: std::process::Command) -> Result<String, String> {
    let out = cmd.output().map_err(|e| format!("git spawn: {}", e))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

fn head_sha(dir: &str) -> Result<String, String> {
    let mut c = git_in(dir);
    c.args(["rev-parse", "HEAD"]);
    run_git(c).map(|s| s.trim().to_string())
}

/// Stage + commit everything in the worktree as one item commit.
/// `--allow-empty` keeps research-only items (no file changes) green.
fn commit_item(dir: &str, title: &str) -> Result<String, String> {
    let mut add = git_in(dir);
    add.args(["add", "-A"]);
    run_git(add)?;
    let mut commit = git_in(dir);
    commit.args([
        "-c",
        "user.name=woom-ledger",
        "-c",
        "user.email=ledger@woom.local",
        "commit",
        "--allow-empty",
        "--no-verify",
        "-m",
        &format!("ledger: {}", title),
    ]);
    run_git(commit)?;
    head_sha(dir)
}

fn commit_diff(dir: &str, sha: &str) -> Option<String> {
    let mut c = git_in(dir);
    c.args(["show", "--format=", sha]);
    match run_git(c) {
        Ok(d) if !d.trim().is_empty() => Some(d),
        _ => None,
    }
}

fn range_diff(dir: &str, base: &str) -> Option<String> {
    let mut c = git_in(dir);
    c.args(["diff", &format!("{}..HEAD", base)]);
    match run_git(c) {
        Ok(d) if !d.trim().is_empty() => Some(d),
        _ => None,
    }
}

// ---- Oneshot + cost ---------------------------------------------------------

async fn call_oneshot(
    prompt: &str,
    cwd: Option<&Path>,
    model: &str,
) -> Result<(String, Option<crate::claude::OneshotUsage>), String> {
    let status = crate::claude::detect();
    if !status.detected {
        return Err("claude CLI not installed".into());
    }
    if !status.ready {
        return Err("claude CLI not authenticated".into());
    }
    let bin = status.path.as_deref().unwrap_or("claude");
    // Unbounded: real work runs minutes; the stop controls are
    // cancel + quota-pause, not a turn timer.
    crate::claude::run_claude_oneshot(bin, prompt, None, None, cwd, Some(model))
        .await
        .map(|r| (r.text, r.usage))
        .map_err(|e| format!("claude oneshot: {}", e))
}

/// Cap on live-feed entries kept per attempt.
const FEED_CAP: usize = 120;

/// One-line summary of a tool_use block for the live feed.
fn feed_line(name: &str, input: &serde_json::Value) -> String {
    let detail = input
        .get("file_path")
        .or_else(|| input.get("command"))
        .or_else(|| input.get("pattern"))
        .or_else(|| input.get("description"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let mut d = detail.replace('\n', " ");
    if d.len() > 90 {
        d.truncate(90);
        d.push('…');
    }
    if d.is_empty() {
        name.to_string()
    } else {
        format!("{} {}", name, d)
    }
}

/// Worker turn with a LIVE action feed: `claude -p --output-format
/// stream-json` parsed line-by-line, every tool_use pushed onto the
/// item's `feed` (mutate_emit — no disk write per line). Returns the
/// final result text + usage like `call_oneshot`.
async fn run_worker_streaming(
    app: &AppHandle,
    reg: &Arc<LedgerRegistry>,
    wf_id: &str,
    item_id: &str,
    prompt: &str,
    cwd: &Path,
    model: &str,
    item_timeout_secs: u64,
) -> Result<(String, Option<crate::claude::OneshotUsage>), String> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let status = crate::claude::detect();
    if !status.detected {
        return Err("claude CLI not installed".into());
    }
    if !status.ready {
        return Err("claude CLI not authenticated".into());
    }
    let bin = status.path.as_deref().unwrap_or("claude");

    let mut cmd = tokio::process::Command::new(bin);
    cmd.arg("-p")
        .arg(prompt)
        .arg("--output-format")
        .arg("stream-json")
        .arg("--verbose")
        // Headless worker: no TTY to approve tool calls. Without this the
        // CLI silently DENIES every Write/Edit/Bash, so the worker can
        // never touch the worktree — it just loops probing for write
        // access, burning millions of tokens without ever producing a
        // diff (observed: one item ran to 3M tokens / $12 before manual
        // cancel). Matches the interactive session's `spawn_claude_armed`;
        // Woom is the trust boundary, not the CLI.
        .arg("--dangerously-skip-permissions")
        .arg("--model")
        .arg(model)
        .current_dir(cwd)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    crate::claude::augment_cli_path(&mut cmd);

    let mut child = cmd.spawn().map_err(|e| format!("worker spawn: {}", e))?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.shutdown().await;
    }
    let stdout = child.stdout.take().ok_or("worker stdout unavailable")?;
    let mut lines = BufReader::new(stdout).lines();

    let mut result_text = String::new();
    let mut usage: Option<crate::claude::OneshotUsage> = None;

    // Bound the worker turn: race each stream line against a 5s tick that
    // consults the pure watchdog. A blocked child (hung `pnpm install`
    // emitting no tokens) trips the stall window; a slow-but-alive worker
    // trips the wall-clock cap. On either, kill the child and return a
    // structured reason — never hang the run.
    let started = std::time::Instant::now();
    let mut last_activity = std::time::Instant::now();
    let lim = WorkerLimits {
        item_secs: item_timeout_secs,
        stall_secs: STALL_TIMEOUT_SECS,
    };
    loop {
        let tick = tokio::time::sleep(std::time::Duration::from_secs(5));
        tokio::select! {
            line = lines.next_line() => {
                match line {
                    Ok(Some(l)) => {
                        last_activity = std::time::Instant::now();
                        let v: serde_json::Value = match serde_json::from_str(&l) {
                            Ok(v) => v,
                            Err(_) => continue,
                        };
                        match v.get("type").and_then(|t| t.as_str()) {
                            Some("assistant") => {
                                let blocks = v
                                    .get("message")
                                    .and_then(|m| m.get("content"))
                                    .and_then(|c| c.as_array())
                                    .cloned()
                                    .unwrap_or_default();
                                for b in blocks {
                                    if b.get("type").and_then(|t| t.as_str()) != Some("tool_use") {
                                        continue;
                                    }
                                    let name = b.get("name").and_then(|n| n.as_str()).unwrap_or("tool");
                                    let entry = feed_line(
                                        name,
                                        b.get("input").unwrap_or(&serde_json::Value::Null),
                                    );
                                    reg.mutate_emit(app, wf_id, |w| {
                                        for it in w.items.iter_mut() {
                                            if it.id == item_id {
                                                it.feed.push(entry.clone());
                                                if it.feed.len() > FEED_CAP {
                                                    let drop = it.feed.len() - FEED_CAP;
                                                    it.feed.drain(0..drop);
                                                }
                                            }
                                        }
                                    });
                                }
                            }
                            Some("result") => {
                                result_text = v
                                    .get("result")
                                    .and_then(|r| r.as_str())
                                    .unwrap_or_default()
                                    .to_string();
                                usage = v
                                    .get("usage")
                                    .and_then(|u| serde_json::from_value(u.clone()).ok());
                            }
                            _ => {}
                        }
                    }
                    Ok(None) | Err(_) => break,
                }
            }
            _ = tick => {
                let elapsed = started.elapsed().as_secs();
                let idle = last_activity.elapsed().as_secs();
                match worker_watchdog(elapsed, idle, lim) {
                    WorkerTick::Continue => {}
                    WorkerTick::Stall => {
                        let _ = child.start_kill();
                        return Err(format!("worker stalled: no output for {idle}s"));
                    }
                    WorkerTick::Timeout => {
                        let _ = child.start_kill();
                        return Err(format!("worker timed out after {elapsed}s"));
                    }
                }
            }
        }
    }
    let exit = child.wait().await.map_err(|e| format!("worker wait: {}", e))?;
    if !exit.success() && result_text.is_empty() {
        return Err(format!("worker CLI exited {}", exit));
    }
    Ok((result_text, usage))
}

fn rate_for(model: &str) -> (f64, f64) {
    let m = model.to_ascii_lowercase();
    if m.contains("haiku") {
        (1.0, 5.0)
    } else if m.contains("sonnet") {
        (3.0, 15.0)
    } else {
        (15.0, 75.0)
    }
}

fn turn_cost(
    usage: &Option<crate::claude::OneshotUsage>,
    prompt_len: usize,
    text_len: usize,
    model: &str,
) -> (u64, u64, f64) {
    let (input_t, cache_read_t, cache_write_t, out_t) = match usage {
        Some(u) => (
            u.input_tokens,
            u.cache_read_input_tokens,
            u.cache_creation_input_tokens,
            u.output_tokens,
        ),
        None => ((prompt_len / 4) as u64, 0, 0, (text_len / 4) as u64),
    };
    let in_tokens = input_t + cache_read_t + cache_write_t;
    let (r_in, r_out) = rate_for(model);
    let cost = (input_t as f64 * r_in
        + cache_read_t as f64 * r_in * 0.1
        + cache_write_t as f64 * r_in * 1.25
        + out_t as f64 * r_out)
        / 1_000_000.0;
    (in_tokens, out_t, cost)
}

/// Clean commit message for a squash-applied ledger: the task as the
/// subject, then the passed items as a bullet body. Used only when
/// `squash = true` (the single-commit path).
fn apply_commit_message(wf: &LedgerWorkflow) -> String {
    let mut body = String::new();
    for it in wf.items.iter().filter(|i| i.status == "passed") {
        body.push_str(&format!("- {}\n", it.title));
    }
    let subject = wf.task.trim();
    if body.is_empty() {
        subject.to_string()
    } else {
        format!("{}\n\nLedger workflow:\n{}", subject, body.trim_end())
    }
}

// ---- Prompts ----------------------------------------------------------------

fn items_overview(wf: &LedgerWorkflow) -> String {
    wf.items
        .iter()
        .map(|it| {
            let glyph = match it.status.as_str() {
                "passed" => "[x]",
                "failed" => "[!]",
                "skipped" => "[-]",
                "working" | "checking" => "[>]",
                _ => "[ ]",
            };
            format!("{} {} — {}", glyph, it.id, it.title)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Accumulated NOTES from every passed item — the knowledge chain
/// across fresh worker contexts (Anthropic harness "progress file").
fn learnings(wf: &LedgerWorkflow) -> String {
    let notes: Vec<String> = wf
        .items
        .iter()
        .filter(|i| i.status == "passed")
        .filter_map(|i| i.notes.as_ref().map(|n| format!("[{}] {}", i.id, n.trim())))
        .collect();
    notes.join("\n")
}

const NOTES_MARKER: &str = "NOTES:";

/// Pull the worker's `NOTES:` tail out of its final reply.
fn extract_notes(text: &str) -> Option<String> {
    let idx = text.rfind(NOTES_MARKER)?;
    let tail = text[idx + NOTES_MARKER.len()..].trim();
    if tail.is_empty() {
        None
    } else {
        Some(truncate_tail(tail, 1_200))
    }
}

fn build_worker_prompt(
    wf: &LedgerWorkflow,
    item: &LedgerItem,
    feedback: Option<&str>,
    in_wave: bool,
) -> String {
    let mut p = format!(
        "You are one worker in a sequential, machine-checked workflow (a \"ledger\"). \
         The overall task:\n\n{task}\n\nFull checklist (x = already done by previous \
         workers, their changes are committed in this working tree):\n{overview}\n\n\
         YOUR item — work on this and NOTHING else:\n{id}: {title}\n{detail}\n",
        task = wf.task,
        overview = items_overview(wf),
        id = item.id,
        title = item.title,
        detail = item.detail.as_deref().unwrap_or(""),
    );
    // The durable plan/contract, when set, anchors every worker to the
    // same intent — the "why + approach" behind the flat checklist.
    if !wf.plan.trim().is_empty() {
        p.push_str(&format!(
            "\nPLAN / CONTRACT for the whole workflow (honor it; your item is one step \
             toward this):\n{}\n",
            wf.plan.trim()
        ));
    }
    let learned = learnings(wf);
    if !learned.is_empty() {
        p.push_str(&format!(
            "\nLearnings left by previous workers (trust these — they already \
             explored the repo):\n{}\n",
            learned
        ));
    }
    if in_wave {
        p.push_str(
            "\nYou run IN PARALLEL with workers on other items. Touch ONLY files \
             within your item's scope — overlapping edits will conflict on merge.\n",
        );
    }
    match &item.check_cmd {
        Some(cmd) => {
            p.push_str(&format!(
                "\nYour work is verified by this exact command (run from the repo root; \
                 exit 0 = pass):\n  {}\nRun it yourself before finishing.\n",
                cmd
            ));
        }
        None => {
            p.push_str(
                "\nYour work is graded by an independent reviewer that sees the diff \
                 and the requirement — make the change complete and self-evident.\n",
            );
        }
    }
    if let Some(fb) = feedback {
        p.push_str(&format!(
            "\nA PREVIOUS ATTEMPT at this item FAILED verification. Do not repeat it. \
             Verification output:\n---\n{}\n---\nFix the root cause.\n",
            fb
        ));
    }
    p.push_str(
        "\nRules: do NOT run `git commit` (the orchestrator commits for you). \
         Do NOT touch items other than yours. Keep changes minimal and production-grade.\n\
         End your reply with a `NOTES:` section — 1-5 terse bullet lines of durable \
         learnings for the NEXT workers (key file paths, commands, gotchas, decisions). \
         No prose, no recap of what you did.",
    );
    p
}

fn build_grader_prompt(wf: &LedgerWorkflow, item: &LedgerItem, diff: &str) -> String {
    let plan_ctx = if wf.plan.trim().is_empty() {
        String::new()
    } else {
        format!("\n\nPlan / contract (judge against this intent):\n{}", wf.plan.trim())
    };
    format!(
        "You are an independent verifier. You did NOT write this change. Requirement:\n\
         {title}\n{detail}\n\nOverall task context:\n{task}{plan_ctx}\n\nThe diff produced for \
         this requirement:\n```diff\n{diff}\n```\n\nVerdict: does the diff fully satisfy \
         the requirement? Reply with STRICT JSON only, no fences, no prose:\n\
         {{\"pass\": true|false, \"reason\": \"one short sentence\"}}",
        title = item.title,
        detail = item.detail.as_deref().unwrap_or(""),
        task = wf.task,
        plan_ctx = plan_ctx,
        diff = truncate_tail(diff, 60_000),
    )
}

fn truncate_tail(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let start = s.len() - max;
    let boundary = s
        .char_indices()
        .map(|(i, _)| i)
        .find(|&i| i >= start)
        .unwrap_or(start);
    format!("…(truncated)…\n{}", &s[boundary..])
}

fn strip_json_fence(s: &str) -> &str {
    let t = s.trim();
    let t = t.strip_prefix("```json").or_else(|| t.strip_prefix("```")).unwrap_or(t);
    let t = t.strip_suffix("```").unwrap_or(t);
    t.trim()
}

#[derive(Deserialize)]
struct GraderVerdict {
    pass: bool,
    #[serde(default)]
    reason: String,
}

// ---- Check execution ---------------------------------------------------------

/// Run a shell check in the worktree. Ok(true) = exit 0. The String is
/// the combined output tail (stdout + stderr) for feedback/UI.
async fn run_shell_check(dir: &str, cmd: &str) -> (bool, String) {
    let mut c = tokio::process::Command::new("/bin/sh");
    c.arg("-lc").arg(cmd).current_dir(dir);
    c.env("GIT_TERMINAL_PROMPT", "0");
    let fut = c.output();
    match tokio::time::timeout(CHECK_TIMEOUT, fut).await {
        Ok(Ok(out)) => {
            let mut text = String::from_utf8_lossy(&out.stdout).to_string();
            let err = String::from_utf8_lossy(&out.stderr);
            if !err.trim().is_empty() {
                text.push_str("\n--- stderr ---\n");
                text.push_str(&err);
            }
            (out.status.success(), truncate_tail(&text, CHECK_OUTPUT_TAIL))
        }
        Ok(Err(e)) => (false, format!("check spawn failed: {}", e)),
        Err(_) => (
            false,
            format!("check timed out after {}s", CHECK_TIMEOUT.as_secs()),
        ),
    }
}

// ---- Quota pause -------------------------------------------------------------

async fn wait_quota(app: &AppHandle, reg: &Arc<LedgerRegistry>, wf_id: &str) -> bool {
    let mut paused = false;
    loop {
        if reg.get(wf_id).map(|w| w.status == "cancelled").unwrap_or(true) {
            return false;
        }
        let hot = match claude_quota::fetch_plan_usage().await {
            Ok(snap) => {
                let p5 = snap.five_hour.as_ref().and_then(|b| b.utilization).unwrap_or(0.0);
                let p7 = snap.seven_day.as_ref().and_then(|b| b.utilization).unwrap_or(0.0);
                p5 >= 95.0 || p7 >= 95.0
            }
            Err(_) => false,
        };
        if !hot {
            break;
        }
        if !paused {
            if let Some(w) = reg.mutate_persist(app, wf_id, |w| {
                w.status = "paused_quota".to_string();
            }) {
                let _ = app.emit("ledger:updated", &w);
            }
            paused = true;
        }
        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
    }
    if paused {
        if let Some(w) = reg.mutate_persist(app, wf_id, |w| {
            w.status = "running".to_string();
        }) {
            let _ = app.emit("ledger:updated", &w);
        }
    }
    true
}

// ---- Run loop -----------------------------------------------------------------

fn emit_updated(app: &AppHandle, reg: &Arc<LedgerRegistry>, wf_id: &str) {
    if let Some(w) = reg.get(wf_id) {
        let _ = app.emit("ledger:updated", &w);
    }
}

/// A queued item is eligible only once every dependency it names has
/// settled (`passed` or `skipped`). No deps → always ready, so an
/// all-empty-deps checklist schedules exactly like the classic linear
/// ledger. An unknown dep id is treated as satisfied (deps are filtered
/// to real, earlier ids at add time, so this is only belt-and-braces —
/// a stale id must never wedge the run).
fn deps_ready(wf: &LedgerWorkflow, item: &LedgerItem) -> bool {
    item.deps.iter().all(|dep| {
        wf.items
            .iter()
            .find(|i| &i.id == dep)
            .map(|i| matches!(i.status.as_str(), "passed" | "skipped"))
            .unwrap_or(true)
    })
}

/// A CONTAINER is any item that is the declared parent of ≥1 other item —
/// a grouping header for sub-items. Containers are never handed to a
/// worker; their status is rolled up from their children.
fn is_container(wf: &LedgerWorkflow, id: &str) -> bool {
    wf.items.iter().any(|i| i.parent_id.as_deref() == Some(id))
}

/// Outcome of scanning for the next runnable item — pure, so the run
/// loop's termination is unit-testable without the async machinery.
enum ItemPick {
    /// A queued leaf with satisfied deps — run it.
    Run(LedgerItem),
    /// Queued leaves remain but none is ready (dangling/cyclic deps that
    /// slipped past add-time validation) — hard error, never spin.
    Deadlock,
    /// Nothing left to run: every leaf is passed/skipped/blocked → done.
    /// `blocked` items are NOT queued, so a skipped-prereq subtree parks
    /// the workflow at the review gate instead of deadlocking it.
    Done,
}

fn next_ready_item(wf: &LedgerWorkflow) -> ItemPick {
    if let Some(i) = wf
        .items
        .iter()
        .find(|i| i.status == "queued" && !is_container(wf, &i.id) && deps_ready(wf, i))
    {
        return ItemPick::Run(i.clone());
    }
    if wf
        .items
        .iter()
        .any(|i| i.status == "queued" && !is_container(wf, &i.id))
    {
        return ItemPick::Deadlock;
    }
    ItemPick::Done
}

/// Derive every container's status from its children. `passed` once all
/// children settle (passed/skipped), `failed` if any child failed,
/// `working` while children are in flight, else `queued`. Reverse order
/// so a container that is itself a child is rolled up before its parent
/// reads it (children are always added after their parent). Idempotent.
fn rollup_containers(w: &mut LedgerWorkflow) {
    let ids: Vec<String> = w.items.iter().map(|i| i.id.clone()).collect();
    for id in ids.iter().rev() {
        let children: Vec<&str> = w
            .items
            .iter()
            .filter(|i| i.parent_id.as_deref() == Some(id.as_str()))
            .map(|i| i.status.as_str())
            .collect();
        if children.is_empty() {
            continue;
        }
        let next = if children.iter().any(|s| *s == "failed") {
            "failed"
        } else if children.iter().all(|s| matches!(*s, "passed" | "skipped")) {
            "passed"
        } else if children
            .iter()
            .any(|s| matches!(*s, "working" | "checking" | "passed" | "skipped"))
        {
            "working"
        } else {
            "queued"
        }
        .to_string();
        if let Some(it) = w.items.iter_mut().find(|i| &i.id == id) {
            it.status = next;
        }
    }
}

/// Propagate `blocked` down dependency edges: a not-yet-run item whose
/// dependency was skipped or failed (or is itself blocked) can't run
/// honestly, so it parks as `blocked` instead of executing against an
/// unmet prerequisite. Fixpoint so a skip cascades through a chain; fully
/// reversible — retrying the culprit until it passes clears the taint and
/// re-queues the subtree on the next call. Only ever moves items between
/// `queued` and `blocked`; never touches passed/working/failed/skipped.
fn recompute_blocked(w: &mut LedgerWorkflow) {
    loop {
        let mut changed = false;
        let ids: Vec<String> = w.items.iter().map(|i| i.id.clone()).collect();
        for id in ids {
            let (status, deps) = match w.items.iter().find(|i| i.id == id) {
                Some(i) => (i.status.clone(), i.deps.clone()),
                None => continue,
            };
            if status != "queued" && status != "blocked" {
                continue;
            }
            let tainted = deps.iter().any(|d| {
                w.items
                    .iter()
                    .find(|i| &i.id == d)
                    .map(|i| matches!(i.status.as_str(), "skipped" | "failed" | "blocked"))
                    .unwrap_or(false)
            });
            let next = if tainted { "blocked" } else { "queued" };
            if next != status {
                if let Some(it) = w.items.iter_mut().find(|i| i.id == id) {
                    it.status = next.to_string();
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
}

pub(crate) fn spawn_ledger_run(app: AppHandle, reg: Arc<LedgerRegistry>, wf_id: String) {
    tauri::async_runtime::spawn(async move {
        if let Err(e) = run_ledger(&app, &reg, &wf_id).await {
            // A cancel/discard mid-run tears the worktree out from under
            // the loop — that surfaces here as an error, but the user's
            // `cancelled` verdict must not be overwritten with `failed`.
            if reg.get(&wf_id).map(|w| w.status == "cancelled").unwrap_or(true) {
                return;
            }
            let wf = reg.mutate_persist(&app, &wf_id, |w| {
                w.status = "failed".to_string();
                w.current_item = None;
                w.completed_at = Some(unix_ms());
            });
            let _ = app.emit(
                "ledger:workflow_done",
                serde_json::json!({ "workflowId": wf_id, "error": e }),
            );
            let _ = wf;
        }
    });
}

async fn run_ledger(app: &AppHandle, reg: &Arc<LedgerRegistry>, wf_id: &str) -> Result<(), String> {
    let wf = reg.get(wf_id).ok_or("workflow disappeared")?;
    let parent_cwd = wf.parent_cwd.clone().ok_or("workflow has no parent cwd")?;
    let model = wf.model.clone();

    // ONE shared worktree for the whole ledger — sequential items build
    // on each other's committed changes. (Uses the shared `worktree`
    // module; its on-disk layout roots the dir at `dw/<wf-id>/work`.)
    let (wt_path, base) = match (&wf.worktree_path, &wf.base_sha) {
        (Some(p), Some(b)) if Path::new(p).exists() => (p.clone(), b.clone()),
        _ => {
            let wt = worktree::create_for_subagent(&parent_cwd, wf_id, "work")
                .map_err(|e| format!("worktree: {}", e))?;
            let base = head_sha(&wt.path)?;
            let path = wt.path.clone();
            let branch = wt.branch.clone();
            reg.mutate_persist(app, wf_id, |w| {
                w.worktree_path = Some(path.clone());
                w.branch = branch.clone();
                w.base_sha = Some(base.clone());
            });
            (path, base)
        }
    };
    emit_updated(app, reg, wf_id);

    loop {
        // Re-read fresh state every iteration — retry/skip/cancel/pause
        // commands mutate the registry under us.
        let wf = reg.get(wf_id).ok_or("workflow disappeared")?;
        if wf.status == "cancelled" {
            return Ok(());
        }
        // User pause takes effect at the item boundary — the in-flight
        // item finishes, then the loop winds down and resume re-enters.
        if matches!(wf.status.as_str(), "paused" | "paused_budget") {
            return Ok(());
        }
        // First queued LEAF item whose dependencies have all settled.
        // Containers (grouping headers) are never executed — their status
        // rolls up from children. With no deps/parents this is just "first
        // queued" (classic order). If queued leaves remain but none is
        // ready, the only cause is a bad dep graph (dangling/cyclic) —
        // filtered at add time, so treat it as a hard error, not a spin.
        let item = match next_ready_item(&wf) {
            ItemPick::Run(i) => i,
            ItemPick::Deadlock => {
                return Err(
                    "ledger deadlocked: remaining items have unsatisfiable dependencies".into(),
                );
            }
            ItemPick::Done => break,
        };

        if !wait_quota(app, reg, wf_id).await {
            return Ok(()); // cancelled during pause
        }

        // Wave: contiguous run of queued+parallel items starting at the
        // first queued one → optimistic concurrent execution in isolated
        // worktrees. Failures degrade those items back to the sequential
        // path (parallel stripped, feedback + attempt carried).
        if item.parallel {
            let mut wave: Vec<LedgerItem> = Vec::new();
            let mut seen_first = false;
            for it in wf.items.iter() {
                if it.id == item.id {
                    seen_first = true;
                }
                if !seen_first {
                    continue;
                }
                match it.status.as_str() {
                    // A wave member must also have its deps satisfied. Any
                    // member depending on another still-queued member is
                    // NOT ready (deps require passed/skipped), so it drops
                    // out of the wave and runs later — waves never contain
                    // an intra-wave dependency edge.
                    "queued"
                        if it.parallel
                            && !is_container(&wf, &it.id)
                            && deps_ready(&wf, it)
                            && wave.len() < 4 =>
                    {
                        wave.push(it.clone())
                    }
                    "queued" => break,
                    "passed" | "skipped" => continue,
                    _ => break,
                }
            }
            if wave.len() >= 2 {
                run_wave(app, reg, wf_id, &wt_path, &model, wave).await?;
                continue;
            }
        }

        // Attempt loop for this item.
        let mut attempt = reg
            .get(wf_id)
            .and_then(|w| w.items.iter().find(|i| i.id == item.id).map(|i| i.attempts))
            .unwrap_or(0);
        let mut feedback: Option<String> = item.check_output.clone();
        loop {
            attempt += 1;
            let wf_now = reg.get(wf_id).ok_or("workflow disappeared")?;
            if wf_now.status == "cancelled" {
                return Ok(());
            }
            reg.mutate_persist(app, wf_id, |w| {
                w.current_item = Some(item.id.clone());
                for it in w.items.iter_mut() {
                    if it.id == item.id {
                        it.status = "working".to_string();
                        it.attempts = attempt;
                        it.feed = Vec::new();
                    }
                }
            });
            emit_updated(app, reg, wf_id);

            // Drain any steering the user queued mid-run and fold it into
            // this turn's feedback, so a running ledger can be nudged
            // without editing items or restarting. Consumed once.
            let mut injected: Vec<String> = Vec::new();
            reg.mutate_persist(app, wf_id, |w| {
                if !w.injections.is_empty() {
                    injected = std::mem::take(&mut w.injections);
                }
            });
            let feedback_now: Option<String> = if injected.is_empty() {
                feedback.clone()
            } else {
                let steer = format!("USER STEERING (mid-run):\n{}", injected.join("\n"));
                Some(match feedback.as_deref() {
                    Some(f) if !f.trim().is_empty() => {
                        format!("{}\n\nPrevious check output:\n{}", steer, f)
                    }
                    _ => steer,
                })
            };

            // 1. Fresh-context worker turn (streamed — live feed).
            let prompt = build_worker_prompt(&wf_now, &item, feedback_now.as_deref(), false);
            let (text, usage) = match run_worker_streaming(
                app,
                reg,
                wf_id,
                &item.id,
                &prompt,
                Path::new(&wt_path),
                &model,
                wf_now.item_timeout_secs.unwrap_or(ITEM_TIMEOUT_SECS),
            )
            .await
            {
                Ok(v) => v,
                Err(reason) => {
                    // Bounded worker failure (timeout / stall / spawn) is a
                    // NORMAL attempt failure, never a run abort. Reset the
                    // worktree debris and route it through the SAME retry /
                    // settle path a failed check uses: feed the reason back
                    // to the next attempt, and after `max_attempts` settle
                    // the item to `failed` with the reason surfaced in
                    // `check_output` so the run reaches the review gate.
                    let mut reset = git_in(&wt_path);
                    reset.args(["reset", "--hard", "HEAD"]);
                    let _ = run_git(reset);
                    let mut clean = git_in(&wt_path);
                    clean.args(["clean", "-fd"]);
                    let _ = run_git(clean);

                    if attempt >= item.max_attempts {
                        reg.mutate_persist(app, wf_id, |w| {
                            for it in w.items.iter_mut() {
                                if it.id == item.id {
                                    it.status = "failed".to_string();
                                    it.check_output = Some(reason.clone());
                                    it.error = Some(format!(
                                        "worker failed after {} attempts: {}",
                                        attempt, reason
                                    ));
                                }
                            }
                            w.status = "failed".to_string();
                            w.current_item = None;
                            w.completed_at = Some(unix_ms());
                        });
                        if let Some(w) = reg.get(wf_id) {
                            let _ = app.emit(
                                "ledger:item_done",
                                serde_json::json!({
                                    "workflowId": wf_id,
                                    "itemId": item.id,
                                    "status": "failed",
                                }),
                            );
                            let _ = app.emit("ledger:workflow_done", &w);
                        }
                        return Ok(());
                    }
                    feedback = Some(reason);
                    continue;
                }
            };
            let (tin, tout, cost) = turn_cost(&usage, prompt.len(), text.len(), &model);
            let spent = reg
                .mutate_persist(app, wf_id, |w| {
                    for it in w.items.iter_mut() {
                        if it.id == item.id {
                            it.tokens_in += tin;
                            it.tokens_out += tout;
                            it.cost_usd += cost;
                            it.status = "checking".to_string();
                        }
                    }
                    w.total_cost_usd = w.items.iter().map(|i| i.cost_usd).sum();
                })
                .map(|w| w.total_cost_usd)
                .unwrap_or(0.0);
            emit_updated(app, reg, wf_id);

            // Budget brake: a single turn can blow the whole budget (a
            // permission-starved worker once hit $12 on one attempt).
            // PAUSE rather than fail — reset this item to queued so a
            // resume (with a raised cap) re-runs it cleanly, and keep
            // every already-committed item. The user raises the cap and
            // presses resume; no progress lost.
            if spent > wf_now.budget_cap_usd {
                let cap = wf_now.budget_cap_usd;
                reg.mutate_persist(app, wf_id, |w| {
                    for it in w.items.iter_mut() {
                        if it.id == item.id {
                            it.status = "queued".to_string();
                            it.attempts = attempt.saturating_sub(1);
                            it.error = Some(format!(
                                "paused: spend ${:.2} hit the ${:.0} budget cap — raise it and resume",
                                spent, cap
                            ));
                        }
                    }
                    w.status = "paused_budget".to_string();
                    w.current_item = None;
                });
                emit_updated(app, reg, wf_id);
                return Ok(());
            }

            // 2. Verification — objective shell check, or independent grader.
            let (passed, check_out) = match &item.check_cmd {
                Some(cmd) => run_shell_check(&wt_path, cmd).await,
                None => {
                    let diff = worktree::capture_diff(&wt_path).unwrap_or_default();
                    let gp = build_grader_prompt(&wf_now, &item, &diff);
                    match call_oneshot(&gp, Some(Path::new(&wt_path)), &model).await {
                        Ok((raw, gusage)) => {
                            let (gin, gout, gcost) =
                                turn_cost(&gusage, gp.len(), raw.len(), &model);
                            reg.mutate_persist(app, wf_id, |w| {
                                for it in w.items.iter_mut() {
                                    if it.id == item.id {
                                        it.tokens_in += gin;
                                        it.tokens_out += gout;
                                        it.cost_usd += gcost;
                                    }
                                }
                                w.total_cost_usd = w.items.iter().map(|i| i.cost_usd).sum();
                            });
                            match serde_json::from_str::<GraderVerdict>(strip_json_fence(&raw)) {
                                Ok(v) => (v.pass, v.reason),
                                Err(_) => (false, format!("grader verdict unparseable: {}", truncate_tail(&raw, 400))),
                            }
                        }
                        Err(e) => (false, format!("grader turn failed: {}", e)),
                    }
                }
            };

            if passed {
                let sha = commit_item(&wt_path, &item.title)?;
                let idiff = commit_diff(&wt_path, &sha);
                let notes = extract_notes(&text);
                reg.mutate_persist(app, wf_id, |w| {
                    for it in w.items.iter_mut() {
                        if it.id == item.id {
                            it.status = "passed".to_string();
                            it.commit_sha = Some(sha.clone());
                            it.diff = idiff.clone();
                            it.check_output = Some(check_out.clone());
                            it.notes = notes.clone();
                            it.error = None;
                        }
                    }
                    // A pass may clear a skip/fail taint — re-queue any
                    // subtree that was blocked waiting on this prereq.
                    recompute_blocked(w);
                    rollup_containers(w);
                    w.current_item = None;
                });
                if let Some(w) = reg.get(wf_id) {
                    let _ = app.emit(
                        "ledger:item_done",
                        serde_json::json!({
                            "workflowId": wf_id,
                            "itemId": item.id,
                            "status": "passed",
                        }),
                    );
                    let _ = app.emit("ledger:updated", &w);
                }
                break;
            }

            // Failed check — reset the worktree's UNCOMMITTED debris so
            // the retry starts from the last good commit, then retry or
            // stop the line.
            let mut reset = git_in(&wt_path);
            reset.args(["reset", "--hard", "HEAD"]);
            let _ = run_git(reset);
            let mut clean = git_in(&wt_path);
            clean.args(["clean", "-fd"]);
            let _ = run_git(clean);

            if attempt >= item.max_attempts {
                reg.mutate_persist(app, wf_id, |w| {
                    for it in w.items.iter_mut() {
                        if it.id == item.id {
                            it.status = "failed".to_string();
                            it.check_output = Some(check_out.clone());
                            it.error =
                                Some(format!("check failed after {} attempts", attempt));
                        }
                    }
                    w.status = "failed".to_string();
                    w.current_item = None;
                    w.completed_at = Some(unix_ms());
                });
                if let Some(w) = reg.get(wf_id) {
                    let _ = app.emit(
                        "ledger:item_done",
                        serde_json::json!({
                            "workflowId": wf_id,
                            "itemId": item.id,
                            "status": "failed",
                        }),
                    );
                    let _ = app.emit("ledger:workflow_done", &w);
                }
                return Ok(());
            }
            feedback = Some(check_out);
        }
    }

    // Integration "Janitor" gate — one shell run against the WHOLE branch
    // after every item passed, catching cross-item regressions a per-item
    // check can't. On red, the workflow still parks at the review gate (so
    // the user sees the diff + the failure), but apply is blocked until a
    // green re-check. No gate configured → ok stays true.
    let (fc_ok, fc_out) = match reg.get(wf_id).and_then(|w| w.final_check.clone()) {
        Some(cmd) if !cmd.trim().is_empty() => {
            let (ok, out) = run_shell_check(&wt_path, &cmd).await;
            (ok, Some(out))
        }
        _ => (true, None),
    };

    // Every item passed (or was skipped) — capture the branch diff and
    // park for the behavioral review gate.
    let full = range_diff(&wt_path, &base);
    let wf_done = reg.mutate_persist(app, wf_id, |w| {
        // Final rollup so grouping headers read `passed` at the review
        // gate once every child settled.
        rollup_containers(w);
        w.full_diff = full.clone();
        w.final_check_ok = fc_ok;
        w.final_check_output = fc_out.clone();
        w.status = "awaiting_review".to_string();
        w.current_item = None;
        w.completed_at = Some(unix_ms());
    });
    if let Some(w) = wf_done {
        let _ = app.emit("ledger:awaiting_review", &w);
    }
    Ok(())
}

/// Best-effort removal of a wave's isolated worktree + branch.
fn remove_wave_worktree(shared: &str, path: &str, branch: Option<&str>) {
    let mut rm = git_in(shared);
    rm.args(["worktree", "remove", "--force", path]);
    let _ = run_git(rm);
    if let Some(b) = branch {
        let mut br = git_in(shared);
        br.args(["branch", "-D", b]);
        let _ = run_git(br);
    }
}

/// Optimistic parallel execution of a wave (2-4 parallel-marked items).
/// Each worker runs in an ISOLATED worktree branched from the shared
/// branch HEAD; diffs merge back IN ITEM ORDER, each followed by its
/// check + commit in the shared tree. Any failure (worker error, merge
/// conflict, red check) degrades that item to the sequential path:
/// status back to `queued`, `parallel` stripped, feedback + attempt
/// carried — the outer loop re-runs it alone with full context.
async fn run_wave(
    app: &AppHandle,
    reg: &Arc<LedgerRegistry>,
    wf_id: &str,
    shared: &str,
    model: &str,
    wave: Vec<LedgerItem>,
) -> Result<(), String> {
    let wf_now = reg.get(wf_id).ok_or("workflow disappeared")?;

    // Mark the whole wave working up front so the card shows the burst.
    reg.mutate_persist(app, wf_id, |w| {
        for it in w.items.iter_mut() {
            if wave.iter().any(|x| x.id == it.id) {
                it.status = "working".to_string();
                it.attempts += 1;
                it.feed = Vec::new();
            }
        }
    });
    emit_updated(app, reg, wf_id);

    struct WaveResult {
        item: LedgerItem,
        wt: Option<worktree::Worktree>,
        text: String,
        diff: Option<String>,
        error: Option<String>,
    }

    let mut handles = Vec::new();
    for item in wave {
        let wt = match worktree::create_for_subagent(shared, wf_id, &item.id) {
            Ok(w) => w,
            Err(e) => {
                handles.push(tauri::async_runtime::spawn(async move {
                    WaveResult {
                        item,
                        wt: None,
                        text: String::new(),
                        diff: None,
                        error: Some(format!("worktree: {}", e)),
                    }
                }));
                continue;
            }
        };
        let prompt = build_worker_prompt(&wf_now, &item, item.check_output.as_deref(), true);
        let (app_t, reg_t, wf_t, model_t) =
            (app.clone(), reg.clone(), wf_id.to_string(), model.to_string());
        let timeout_t = wf_now.item_timeout_secs.unwrap_or(ITEM_TIMEOUT_SECS);
        handles.push(tauri::async_runtime::spawn(async move {
            let run = run_worker_streaming(
                &app_t,
                &reg_t,
                &wf_t,
                &item.id,
                &prompt,
                Path::new(&wt.path),
                &model_t,
                timeout_t,
            )
            .await;
            match run {
                Ok((text, usage)) => {
                    let (tin, tout, cost) =
                        turn_cost(&usage, prompt.len(), text.len(), &model_t);
                    reg_t.mutate_emit(&app_t, &wf_t, |w| {
                        for it in w.items.iter_mut() {
                            if it.id == item.id {
                                it.tokens_in += tin;
                                it.tokens_out += tout;
                                it.cost_usd += cost;
                            }
                        }
                        w.total_cost_usd = w.items.iter().map(|i| i.cost_usd).sum();
                    });
                    let diff = worktree::capture_diff(&wt.path).ok().filter(|d| !d.trim().is_empty());
                    WaveResult { item, wt: Some(wt), text, diff, error: None }
                }
                Err(e) => WaveResult {
                    item,
                    wt: Some(wt),
                    text: String::new(),
                    diff: None,
                    error: Some(e),
                },
            }
        }));
    }

    let mut results: Vec<WaveResult> = Vec::new();
    for h in handles {
        if let Ok(r) = h.await {
            results.push(r);
        }
    }
    // Merge back in the checklist's order, not completion order.
    results.sort_by_key(|r| {
        wf_now.items.iter().position(|i| i.id == r.item.id).unwrap_or(usize::MAX)
    });

    for r in results {
        let degrade = |reason: String, reg: &Arc<LedgerRegistry>| {
            reg.mutate_persist(app, wf_id, |w| {
                for it in w.items.iter_mut() {
                    if it.id == r.item.id {
                        it.status = "queued".to_string();
                        it.parallel = false;
                        it.check_output = Some(reason.clone());
                    }
                }
            });
        };
        if let Some(e) = &r.error {
            degrade(format!("parallel attempt failed: {}", e), reg);
        } else {
            let apply_ok = match &r.diff {
                Some(d) => worktree::apply_patch(shared, d).map(|_| true).unwrap_or(false),
                None => true, // research-only item — nothing to merge
            };
            if !apply_ok {
                degrade(
                    "parallel attempt produced a diff that no longer merges \
                     (another wave item touched the same files) — redo \
                     sequentially on the current tree"
                        .to_string(),
                    reg,
                );
            } else {
                let (passed, check_out) = match &r.item.check_cmd {
                    Some(cmd) => run_shell_check(shared, cmd).await,
                    None => {
                        let d = r.diff.clone().unwrap_or_default();
                        let gp = build_grader_prompt(&wf_now, &r.item, &d);
                        match call_oneshot(&gp, Some(Path::new(shared)), model).await {
                            Ok((raw, _)) => match serde_json::from_str::<GraderVerdict>(
                                strip_json_fence(&raw),
                            ) {
                                Ok(v) => (v.pass, v.reason),
                                Err(_) => (false, "grader verdict unparseable".to_string()),
                            },
                            Err(e) => (false, format!("grader turn failed: {}", e)),
                        }
                    }
                };
                if passed {
                    let sha = commit_item(shared, &r.item.title)?;
                    let idiff = commit_diff(shared, &sha);
                    let notes = extract_notes(&r.text);
                    reg.mutate_persist(app, wf_id, |w| {
                        for it in w.items.iter_mut() {
                            if it.id == r.item.id {
                                it.status = "passed".to_string();
                                it.commit_sha = Some(sha.clone());
                                it.diff = idiff.clone();
                                it.check_output = Some(check_out.clone());
                                it.notes = notes.clone();
                                it.error = None;
                            }
                        }
                        // Mirror the sequential pass path: a wave member
                        // passing can clear a taint and re-queue a blocked
                        // subtree.
                        recompute_blocked(w);
                        rollup_containers(w);
                    });
                    let _ = app.emit(
                        "ledger:item_done",
                        serde_json::json!({
                            "workflowId": wf_id,
                            "itemId": r.item.id,
                            "status": "passed",
                        }),
                    );
                } else {
                    // Roll the failed merge's debris out of the shared tree.
                    let mut reset = git_in(shared);
                    reset.args(["reset", "--hard", "HEAD"]);
                    let _ = run_git(reset);
                    let mut clean = git_in(shared);
                    clean.args(["clean", "-fd"]);
                    let _ = run_git(clean);
                    degrade(check_out, reg);
                }
            }
        }
        if let Some(wt) = &r.wt {
            remove_wave_worktree(shared, &wt.path, wt.branch.as_deref());
        }
    }
    emit_updated(app, reg, wf_id);
    Ok(())
}

// ---- Tauri commands ------------------------------------------------------------

#[tauri::command]
pub async fn ledger_create(
    app: AppHandle,
    session_id: String,
    task: String,
    cwd: Option<String>,
    model: Option<String>,
) -> Result<String, String> {
    let registry: tauri::State<'_, Arc<LedgerRegistry>> = app.state();
    let id = format!("ledger-{}", uuid_v4());
    let wf = LedgerWorkflow {
        id: id.clone(),
        session_id,
        task,
        plan: String::new(),
        final_check: None,
        final_check_ok: true,
        final_check_output: None,
        status: "building".to_string(),
        items: vec![],
        current_item: None,
        worktree_path: None,
        branch: None,
        base_sha: None,
        full_diff: None,
        applied: false,
        squash: default_squash(),
        injections: Vec::new(),
        model: model.filter(|m| !m.trim().is_empty()).unwrap_or_else(default_model),
        total_cost_usd: 0.0,
        budget_cap_usd: default_budget_cap(),
        item_timeout_secs: None,
        created_at: unix_ms(),
        started_at: None,
        completed_at: None,
        parent_cwd: cwd,
    };
    persist_workflow(&app, &wf);
    registry.inner().upsert(wf.clone());
    let _ = app.emit("ledger:created", &wf);
    Ok(id)
}

#[tauri::command]
pub async fn ledger_set_task(
    app: AppHandle,
    workflow_id: String,
    task: String,
) -> Result<(), String> {
    let registry: tauri::State<'_, Arc<LedgerRegistry>> = app.state();
    let wf = registry
        .inner()
        .mutate_persist(&app, &workflow_id, |w| w.task = task.clone())
        .ok_or_else(|| format!("ledger not found: {}", workflow_id))?;
    let _ = app.emit("ledger:updated", &wf);
    Ok(())
}

#[tauri::command]
pub async fn ledger_set_plan(
    app: AppHandle,
    workflow_id: String,
    plan: String,
) -> Result<(), String> {
    let registry: tauri::State<'_, Arc<LedgerRegistry>> = app.state();
    let wf = registry
        .inner()
        .mutate_persist(&app, &workflow_id, |w| w.plan = plan.clone())
        .ok_or_else(|| format!("ledger not found: {}", workflow_id))?;
    let _ = app.emit("ledger:updated", &wf);
    Ok(())
}

#[tauri::command]
pub async fn ledger_set_final_check(
    app: AppHandle,
    workflow_id: String,
    cmd: Option<String>,
) -> Result<(), String> {
    let registry: tauri::State<'_, Arc<LedgerRegistry>> = app.state();
    let wf = registry
        .inner()
        .mutate_persist(&app, &workflow_id, |w| {
            w.final_check = cmd.as_ref().map(|c| c.trim().to_string()).filter(|c| !c.is_empty());
        })
        .ok_or_else(|| format!("ledger not found: {}", workflow_id))?;
    let _ = app.emit("ledger:updated", &wf);
    Ok(())
}

/// Re-run the integration final-check against the current worktree (used
/// from the review gate after a failed janitor run). Updates
/// `final_check_ok` / `final_check_output` and re-emits.
#[tauri::command]
pub async fn ledger_recheck(app: AppHandle, workflow_id: String) -> Result<bool, String> {
    let registry: tauri::State<'_, Arc<LedgerRegistry>> = app.state();
    let wf = registry
        .inner()
        .get(&workflow_id)
        .ok_or_else(|| format!("ledger not found: {}", workflow_id))?;
    let cmd = match wf.final_check.as_deref() {
        Some(c) if !c.trim().is_empty() => c.to_string(),
        _ => return Ok(true), // no gate → nothing to check
    };
    let wt = wf
        .worktree_path
        .clone()
        .ok_or_else(|| "ledger has no worktree".to_string())?;
    let (ok, out) = run_shell_check(&wt, &cmd).await;
    let updated = registry.inner().mutate_persist(&app, &workflow_id, |w| {
        w.final_check_ok = ok;
        w.final_check_output = Some(out.clone());
    });
    if let Some(w) = updated {
        let _ = app.emit("ledger:updated", &w);
    }
    Ok(ok)
}

#[tauri::command]
pub async fn ledger_add_item(
    app: AppHandle,
    workflow_id: String,
    title: String,
    detail: Option<String>,
    check_cmd: Option<String>,
    max_attempts: Option<u32>,
    parallel: Option<bool>,
    deps: Option<Vec<String>>,
    parent_id: Option<String>,
) -> Result<String, String> {
    if title.trim().is_empty() {
        return Err("item title is empty".into());
    }
    let registry: tauri::State<'_, Arc<LedgerRegistry>> = app.state();
    let mut new_id = String::new();
    let wf = registry
        .inner()
        .mutate_persist(&app, &workflow_id, |w| {
            if w.items.len() >= MAX_ITEMS {
                return;
            }
            let id = format!("item-{}", w.items.len() + 1);
            new_id = id.clone();
            // Keep only deps that name a real, earlier item — a dangling
            // or forward ref would deadlock the run loop. Builders add
            // items in order, so a valid dep always already exists.
            let existing: std::collections::HashSet<&str> =
                w.items.iter().map(|i| i.id.as_str()).collect();
            let deps = deps
                .clone()
                .unwrap_or_default()
                .into_iter()
                .filter(|d| existing.contains(d.as_str()))
                .collect::<Vec<_>>();
            // parent_id must name a real, earlier item (same order rule as
            // deps); an unknown ref is dropped so it degrades to a normal
            // top-level item rather than an orphan the card can't place.
            let parent_id = parent_id
                .clone()
                .filter(|p| existing.contains(p.as_str()));
            w.items.push(LedgerItem {
                id,
                title: title.clone(),
                detail: detail.clone().filter(|s| !s.trim().is_empty()),
                check_cmd: check_cmd.clone().filter(|s| !s.trim().is_empty()),
                status: "queued".to_string(),
                attempts: 0,
                max_attempts: max_attempts
                    .filter(|&m| (1..=10).contains(&m))
                    .unwrap_or(DEFAULT_MAX_ATTEMPTS),
                diff: None,
                commit_sha: None,
                check_output: None,
                error: None,
                tokens_in: 0,
                tokens_out: 0,
                cost_usd: 0.0,
                notes: None,
                parallel: parallel.unwrap_or(false),
                deps,
                parent_id,
                feed: Vec::new(),
            });
        })
        .ok_or_else(|| format!("ledger not found: {}", workflow_id))?;
    if new_id.is_empty() {
        return Err(format!("item cap ({}) reached", MAX_ITEMS));
    }
    let _ = app.emit("ledger:updated", &wf);
    Ok(new_id)
}

/// Statuses in which the user may still reshape the checklist.
fn editable(wf_status: &str, item_status: &str) -> bool {
    matches!(wf_status, "building" | "awaiting_launch" | "failed")
        && matches!(item_status, "queued" | "failed" | "skipped")
}

/// Edit a not-yet-run item (title / detail / check_cmd / max_attempts /
/// parallel / deps / parent_id). The user's pre-run review gate is only
/// as strong as its ability to FIX the checklist, not just approve it.
#[tauri::command]
pub async fn ledger_update_item(
    app: AppHandle,
    workflow_id: String,
    item_id: String,
    title: Option<String>,
    detail: Option<String>,
    check_cmd: Option<String>,
    max_attempts: Option<u32>,
    parallel: Option<bool>,
    deps: Option<Vec<String>>,
    parent_id: Option<String>,
) -> Result<(), String> {
    let registry: tauri::State<'_, Arc<LedgerRegistry>> = app.state();
    let mut touched = false;
    let wf = registry
        .inner()
        .mutate_persist(&app, &workflow_id, |w| {
            let wf_status = w.status.clone();
            // Valid targets for deps / parent_id: any OTHER item. Filter
            // here (not against "earlier only") since edit can point at
            // any existing item; a self-ref or unknown id is dropped so
            // the graph can't be wedged from the card.
            let valid: std::collections::HashSet<String> = w
                .items
                .iter()
                .map(|i| i.id.clone())
                .filter(|id| id != &item_id)
                .collect();
            for it in w.items.iter_mut() {
                if it.id != item_id || !editable(&wf_status, &it.status) {
                    continue;
                }
                touched = true;
                if let Some(t) = &title {
                    if !t.trim().is_empty() {
                        it.title = t.clone();
                    }
                }
                if let Some(d) = &detail {
                    it.detail = Some(d.clone()).filter(|s| !s.trim().is_empty());
                }
                if let Some(c) = &check_cmd {
                    it.check_cmd = Some(c.clone()).filter(|s| !s.trim().is_empty());
                }
                if let Some(m) = max_attempts {
                    if (1..=10).contains(&m) {
                        it.max_attempts = m;
                    }
                }
                if let Some(p) = parallel {
                    it.parallel = p;
                }
                if let Some(d) = &deps {
                    it.deps = d.iter().filter(|x| valid.contains(*x)).cloned().collect();
                }
                if let Some(p) = &parent_id {
                    // Empty string clears the parent (item → top level).
                    it.parent_id = if p.trim().is_empty() {
                        None
                    } else if valid.contains(p) {
                        Some(p.clone())
                    } else {
                        it.parent_id.clone()
                    };
                }
                // Edited items get a clean slate — stale failure feedback
                // must not leak into the next attempt's prompt.
                if it.status != "queued" {
                    it.status = "queued".to_string();
                    it.attempts = 0;
                    it.check_output = None;
                    it.error = None;
                }
            }
        })
        .ok_or_else(|| format!("ledger not found: {}", workflow_id))?;
    if !touched {
        return Err("item not editable in its current state".into());
    }
    let _ = app.emit("ledger:updated", &wf);
    Ok(())
}

#[tauri::command]
pub async fn ledger_remove_item(
    app: AppHandle,
    workflow_id: String,
    item_id: String,
) -> Result<(), String> {
    let registry: tauri::State<'_, Arc<LedgerRegistry>> = app.state();
    let mut touched = false;
    let wf = registry
        .inner()
        .mutate_persist(&app, &workflow_id, |w| {
            let wf_status = w.status.clone();
            let before = w.items.len();
            w.items
                .retain(|it| !(it.id == item_id && editable(&wf_status, &it.status)));
            touched = w.items.len() != before;
        })
        .ok_or_else(|| format!("ledger not found: {}", workflow_id))?;
    if !touched {
        return Err("item not removable in its current state".into());
    }
    let _ = app.emit("ledger:updated", &wf);
    Ok(())
}

/// Move an item one slot up or down among the not-yet-run tail.
#[tauri::command]
pub async fn ledger_move_item(
    app: AppHandle,
    workflow_id: String,
    item_id: String,
    direction: String,
) -> Result<(), String> {
    let registry: tauri::State<'_, Arc<LedgerRegistry>> = app.state();
    let up = direction == "up";
    let mut touched = false;
    let wf = registry
        .inner()
        .mutate_persist(&app, &workflow_id, |w| {
            let wf_status = w.status.clone();
            let idx = match w.items.iter().position(|i| i.id == item_id) {
                Some(i) => i,
                None => return,
            };
            let swap_with = if up { idx.checked_sub(1) } else { idx.checked_add(1) };
            let j = match swap_with {
                Some(j) if j < w.items.len() => j,
                _ => return,
            };
            if editable(&wf_status, &w.items[idx].status)
                && editable(&wf_status, &w.items[j].status)
            {
                w.items.swap(idx, j);
                touched = true;
            }
        })
        .ok_or_else(|| format!("ledger not found: {}", workflow_id))?;
    if !touched {
        return Err("item not movable".into());
    }
    let _ = app.emit("ledger:updated", &wf);
    Ok(())
}

/// Agent finished building the checklist — park in `awaiting_launch`
/// for the user's approval gate.
#[tauri::command]
pub async fn ledger_launch(app: AppHandle, workflow_id: String) -> Result<(), String> {
    let registry: tauri::State<'_, Arc<LedgerRegistry>> = app.state();
    let wf = registry
        .inner()
        .get(&workflow_id)
        .ok_or_else(|| format!("ledger not found: {}", workflow_id))?;
    if wf.items.is_empty() {
        return Err("no items added — call ledger_add_item first".into());
    }
    let wf = registry
        .inner()
        .mutate_persist(&app, &workflow_id, |w| {
            w.status = "awaiting_launch".to_string();
        })
        .unwrap();
    let _ = app.emit("ledger:updated", &wf);
    Ok(())
}

/// User approved the checklist — start the sequential run.
#[tauri::command]
pub async fn ledger_run(app: AppHandle, workflow_id: String) -> Result<(), String> {
    let registry: tauri::State<'_, Arc<LedgerRegistry>> = app.state();
    let reg = registry.inner().clone();
    let wf = reg
        .get(&workflow_id)
        .ok_or_else(|| format!("ledger not found: {}", workflow_id))?;
    if !matches!(wf.status.as_str(), "awaiting_launch" | "building") {
        return Err(format!("ledger not launchable from status {}", wf.status));
    }
    if wf.parent_cwd.is_none() {
        return Err("ledger has no working directory".into());
    }
    let wf = reg
        .mutate_persist(&app, &workflow_id, |w| {
            // Scale the cap to the checklist size so a healthy run never
            // nags — only a genuine overrun trips it. Respect a higher
            // user-set cap.
            w.budget_cap_usd = w.budget_cap_usd.max(budget_for(w.items.len()));
            w.status = "running".to_string();
            w.started_at = Some(unix_ms());
        })
        .unwrap();
    let _ = app.emit("ledger:updated", &wf);
    spawn_ledger_run(app, reg, workflow_id);
    Ok(())
}

#[tauri::command]
pub async fn ledger_cancel(app: AppHandle, workflow_id: String) -> Result<(), String> {
    let registry: tauri::State<'_, Arc<LedgerRegistry>> = app.state();
    let wf = registry
        .inner()
        .mutate_persist(&app, &workflow_id, |w| {
            w.status = "cancelled".to_string();
            w.current_item = None;
            w.completed_at = Some(unix_ms());
        })
        .ok_or_else(|| format!("ledger not found: {}", workflow_id))?;
    let _ = app.emit("ledger:workflow_done", &wf);
    Ok(())
}

/// User pause. Takes effect at the next item boundary — the in-flight
/// item completes, the run loop then winds down. Resume re-enters.
#[tauri::command]
pub async fn ledger_pause(app: AppHandle, workflow_id: String) -> Result<(), String> {
    let registry: tauri::State<'_, Arc<LedgerRegistry>> = app.state();
    let wf = registry
        .inner()
        .get(&workflow_id)
        .ok_or_else(|| format!("ledger not found: {}", workflow_id))?;
    if !matches!(wf.status.as_str(), "running" | "paused_quota") {
        return Err(format!("ledger not pausable from status {}", wf.status));
    }
    let wf = registry
        .inner()
        .mutate_persist(&app, &workflow_id, |w| {
            w.status = "paused".to_string();
        })
        .unwrap();
    let _ = app.emit("ledger:updated", &wf);
    Ok(())
}

/// Resume a paused run (user pause or budget cap). Optionally raise the
/// budget cap; a bare resume from a budget pause auto-bumps the cap by
/// the default increment so the run can actually make progress.
#[tauri::command]
pub async fn ledger_resume(
    app: AppHandle,
    workflow_id: String,
    budget_cap_usd: Option<f64>,
) -> Result<(), String> {
    let registry: tauri::State<'_, Arc<LedgerRegistry>> = app.state();
    let reg = registry.inner().clone();
    let wf = reg
        .get(&workflow_id)
        .ok_or_else(|| format!("ledger not found: {}", workflow_id))?;
    if !matches!(wf.status.as_str(), "paused" | "paused_budget") {
        return Err(format!("ledger not resumable from status {}", wf.status));
    }
    let wf = reg
        .mutate_persist(&app, &workflow_id, |w| {
            if let Some(c) = budget_cap_usd {
                if c > 0.0 {
                    w.budget_cap_usd = c;
                }
            }
            // Guarantee real headroom above what's already spent, else
            // the next turn would instantly re-pause at the same cap.
            // A checklist-sized bump so one resume finishes the run
            // instead of nagging item-by-item.
            if w.budget_cap_usd <= w.total_cost_usd {
                w.budget_cap_usd = w.total_cost_usd + budget_for(w.items.len());
            }
            w.status = "running".to_string();
            w.completed_at = None;
        })
        .unwrap();
    let _ = app.emit("ledger:updated", &wf);
    spawn_ledger_run(app, reg, workflow_id);
    Ok(())
}

/// Reset a failed item to `queued` and resume the sequential run.
#[tauri::command]
pub async fn ledger_retry_item(
    app: AppHandle,
    workflow_id: String,
    item_id: String,
) -> Result<(), String> {
    let registry: tauri::State<'_, Arc<LedgerRegistry>> = app.state();
    let reg = registry.inner().clone();
    // Only a parked (failed) workflow restarts the loop here — spawning
    // while a run is live would race a second loop over the same items.
    let was_failed = reg
        .get(&workflow_id)
        .map(|w| w.status == "failed")
        .ok_or_else(|| format!("ledger not found: {}", workflow_id))?;
    let wf = reg
        .mutate_persist(&app, &workflow_id, |w| {
            for it in w.items.iter_mut() {
                if it.id == item_id && it.status == "failed" {
                    it.status = "queued".to_string();
                    it.attempts = 0;
                    it.error = None;
                }
            }
            if w.status == "failed" {
                w.status = "running".to_string();
                w.completed_at = None;
            }
        })
        .ok_or_else(|| format!("ledger not found: {}", workflow_id))?;
    let _ = app.emit("ledger:updated", &wf);
    if was_failed {
        spawn_ledger_run(app, reg, workflow_id);
    }
    Ok(())
}

/// Skip a failed item and resume the run past it.
#[tauri::command]
pub async fn ledger_skip_item(
    app: AppHandle,
    workflow_id: String,
    item_id: String,
) -> Result<(), String> {
    let registry: tauri::State<'_, Arc<LedgerRegistry>> = app.state();
    let reg = registry.inner().clone();
    // Same live-loop guard as retry: a running loop re-reads item state
    // every iteration, so skipping mid-run needs no new spawn.
    let was_failed = reg
        .get(&workflow_id)
        .map(|w| w.status == "failed")
        .ok_or_else(|| format!("ledger not found: {}", workflow_id))?;
    let wf = reg
        .mutate_persist(&app, &workflow_id, |w| {
            for it in w.items.iter_mut() {
                if it.id == item_id && matches!(it.status.as_str(), "failed" | "queued") {
                    it.status = "skipped".to_string();
                }
            }
            // Skipping a prerequisite blocks its dependent subtree — those
            // items won't run against an unmet dep (reversible if the user
            // later retries the skipped item to green).
            recompute_blocked(w);
            if w.status == "failed" {
                w.status = "running".to_string();
                w.completed_at = None;
            }
        })
        .ok_or_else(|| format!("ledger not found: {}", workflow_id))?;
    let _ = app.emit("ledger:updated", &wf);
    if was_failed {
        spawn_ledger_run(app, reg, workflow_id);
    }
    Ok(())
}

/// The behavioral gate's "Apply" — land the branch diff onto the
/// parent checkout, then reap the worktree.
#[tauri::command]
pub async fn ledger_apply(app: AppHandle, workflow_id: String) -> Result<(), String> {
    let registry: tauri::State<'_, Arc<LedgerRegistry>> = app.state();
    let reg = registry.inner().clone();
    let wf = reg
        .get(&workflow_id)
        .ok_or_else(|| format!("ledger not found: {}", workflow_id))?;
    if wf.status != "awaiting_review" {
        return Err(format!("ledger not reviewable from status {}", wf.status));
    }
    // Janitor gate: a configured final-check must be green before the
    // branch can land — blocks applying a diff that passes every item but
    // breaks the integrated build.
    if wf.final_check.as_deref().map(|c| !c.trim().is_empty()).unwrap_or(false) && !wf.final_check_ok {
        return Err("final check is red — fix the branch and re-check before applying".into());
    }
    let parent = wf.parent_cwd.clone().ok_or("ledger has no parent cwd")?;
    let diff = wf.full_diff.clone().unwrap_or_default();
    if !diff.trim().is_empty() {
        match wf.branch.as_deref() {
            // Real git merge of the workflow's branch — carries binary
            // files (icons, images) a text patch would drop, and honours
            // the squash toggle (one clean commit vs per-item history).
            Some(branch) => {
                worktree::apply_branch(&parent, branch, wf.squash, &apply_commit_message(&wf))?;
            }
            // Legacy workflows persisted before the branch was recorded:
            // fall back to the text patch (binaries still won't ride, but
            // nothing else can be done without the branch).
            None => worktree::apply_patch(&parent, &diff)?,
        }
    }
    let wf = reg
        .mutate_persist(&app, &workflow_id, |w| {
            w.applied = true;
            w.status = "done".to_string();
        })
        .unwrap();
    let _ = worktree::cleanup_workflow_worktrees(&parent, &workflow_id);
    let _ = app.emit("ledger:workflow_done", &wf);
    // Delta-spec: distill what became TRUE of the project into memory so
    // future sessions inherit it (OpenSpec living-spec pattern). Cheap
    // haiku turn, best-effort, detached from the apply path.
    tauri::async_runtime::spawn(async move {
        let items: Vec<String> = wf
            .items
            .iter()
            .filter(|i| i.status == "passed")
            .map(|i| {
                let n = i.notes.as_deref().unwrap_or("");
                format!("- {} {}", i.title, if n.is_empty() { String::new() } else { format!("({})", n) })
            })
            .collect();
        if items.is_empty() {
            return;
        }
        let prompt = format!(
            "A checklist workflow just landed in repo {repo}. Task: {task}\nCompleted \
             items:\n{items}\n\nWrite 1-3 sentences of FORWARD-LOOKING truth about the \
             project after this change (what now exists / how it works), suitable as a \
             long-term memory. No narrative, no 'we did X'. Reply with the sentences only.",
            repo = parent,
            task = wf.task,
            items = items.join("\n"),
        );
        if let Ok((text, _)) =
            call_oneshot(&prompt, None, "claude-haiku-4-5-20251001").await
        {
            let t = text.trim();
            if !t.is_empty() {
                let _ = crate::memory_local::memory_save_local(
                    format!("[ledger delta-spec, {}] {}", wf.task, t),
                    Some("project".into()),
                    Some(vec!["ledger".into(), "delta-spec".into()]),
                );
            }
        }
    });
    Ok(())
}

/// Discard the branch without applying.
#[tauri::command]
pub async fn ledger_discard(app: AppHandle, workflow_id: String) -> Result<(), String> {
    let registry: tauri::State<'_, Arc<LedgerRegistry>> = app.state();
    let reg = registry.inner().clone();
    let wf = reg
        .mutate_persist(&app, &workflow_id, |w| {
            w.status = "cancelled".to_string();
            w.completed_at = Some(unix_ms());
        })
        .ok_or_else(|| format!("ledger not found: {}", workflow_id))?;
    if let Some(parent) = &wf.parent_cwd {
        let _ = worktree::cleanup_workflow_worktrees(parent, &workflow_id);
    }
    let _ = app.emit("ledger:workflow_done", &wf);
    Ok(())
}

/// Toggle whether applying squashes into one commit (default) or keeps
/// the per-item commits. Honoured by `ledger_apply`.
#[tauri::command]
pub async fn ledger_set_squash(
    app: AppHandle,
    workflow_id: String,
    squash: bool,
) -> Result<(), String> {
    let registry: tauri::State<'_, Arc<LedgerRegistry>> = app.state();
    let reg = registry.inner().clone();
    let wf = reg
        .mutate_persist(&app, &workflow_id, |w| w.squash = squash)
        .ok_or_else(|| format!("ledger not found: {}", workflow_id))?;
    emit_updated(&app, &reg, &workflow_id);
    let _ = wf;
    Ok(())
}

/// Queue a mid-run steering note. Drained into the next worker turn
/// (next attempt or next item) — no-op with an error if the workflow
/// isn't actively running.
#[tauri::command]
pub async fn ledger_inject(
    app: AppHandle,
    workflow_id: String,
    note: String,
) -> Result<(), String> {
    let note = note.trim().to_string();
    if note.is_empty() {
        return Err("empty steering note".into());
    }
    let registry: tauri::State<'_, Arc<LedgerRegistry>> = app.state();
    let reg = registry.inner().clone();
    let wf = reg
        .get(&workflow_id)
        .ok_or_else(|| format!("ledger not found: {}", workflow_id))?;
    if wf.status != "running" {
        return Err(format!("ledger is not running (status {})", wf.status));
    }
    reg.mutate_persist(&app, &workflow_id, |w| w.injections.push(note));
    emit_updated(&app, &reg, &workflow_id);
    Ok(())
}

#[tauri::command]
pub async fn ledger_get(
    app: AppHandle,
    workflow_id: String,
) -> Result<LedgerWorkflow, String> {
    let registry: tauri::State<'_, Arc<LedgerRegistry>> = app.state();
    registry
        .inner()
        .get(&workflow_id)
        .ok_or_else(|| format!("ledger not found: {}", workflow_id))
}

#[tauri::command]
pub async fn ledger_list(app: AppHandle) -> Result<Vec<LedgerWorkflow>, String> {
    let registry: tauri::State<'_, Arc<LedgerRegistry>> = app.state();
    let mut all = registry.inner().all();
    all.sort_by_key(|w| std::cmp::Reverse(w.created_at));
    Ok(all)
}

fn unix_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn uuid_v4() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct WorkerLimits {
    /// Hard wall-clock cap for one worker attempt, seconds.
    pub item_secs: u64,
    /// Max seconds with no stream activity before the attempt is stalled.
    pub stall_secs: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WorkerTick {
    Continue,
    Stall,
    Timeout,
}

/// Pure watchdog decision. `elapsed_secs` = seconds since the attempt
/// started; `idle_secs` = seconds since the last stream event. Timeout
/// wins over stall so the surfaced reason names the harder bound.
pub(crate) fn worker_watchdog(elapsed_secs: u64, idle_secs: u64, lim: WorkerLimits) -> WorkerTick {
    if elapsed_secs >= lim.item_secs {
        WorkerTick::Timeout
    } else if idle_secs >= lim.stall_secs {
        WorkerTick::Stall
    } else {
        WorkerTick::Continue
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, status: &str) -> LedgerItem {
        LedgerItem {
            id: id.into(),
            title: format!("do {}", id),
            detail: None,
            check_cmd: None,
            status: status.into(),
            attempts: 0,
            max_attempts: DEFAULT_MAX_ATTEMPTS,
            diff: None,
            commit_sha: None,
            check_output: None,
            error: None,
            tokens_in: 0,
            tokens_out: 0,
            cost_usd: 0.0,
            notes: None,
            parallel: false,
            deps: Vec::new(),
            parent_id: None,
            feed: Vec::new(),
        }
    }

    fn wf(items: Vec<LedgerItem>) -> LedgerWorkflow {
        LedgerWorkflow {
            id: "ledger-test".into(),
            session_id: "s".into(),
            task: "task".into(),
            plan: String::new(),
            final_check: None,
            final_check_ok: true,
            final_check_output: None,
            status: "running".into(),
            items,
            current_item: None,
            worktree_path: None,
            branch: None,
            base_sha: None,
            full_diff: None,
            applied: false,
            squash: default_squash(),
            injections: Vec::new(),
            model: default_model(),
            total_cost_usd: 0.0,
            budget_cap_usd: default_budget_cap(),
            item_timeout_secs: None,
            created_at: 0,
            started_at: None,
            completed_at: None,
            parent_cwd: None,
        }
    }

    #[test]
    fn overview_glyphs_track_status() {
        let w = wf(vec![item("item-1", "passed"), item("item-2", "queued")]);
        let o = items_overview(&w);
        assert!(o.contains("[x] item-1"));
        assert!(o.contains("[ ] item-2"));
    }

    #[test]
    fn worker_prompt_carries_check_and_feedback() {
        let mut it = item("item-1", "queued");
        it.check_cmd = Some("pnpm test".into());
        let w = wf(vec![it.clone()]);
        let p = build_worker_prompt(&w, &it, Some("2 tests failed"), false);
        assert!(p.contains("pnpm test"));
        assert!(p.contains("2 tests failed"));
        assert!(p.contains("do NOT run `git commit`"));
        assert!(p.contains("NOTES:"));
    }

    #[test]
    fn notes_chain_extract_and_inject() {
        let reply = "did the thing.\n\nNOTES:\n- build cmd is `pnpm check`\n- config in src/lib.rs";
        let n = extract_notes(reply).unwrap();
        assert!(n.contains("pnpm check"));
        let mut done = item("item-1", "passed");
        done.notes = Some(n);
        let next = item("item-2", "queued");
        let w = wf(vec![done, next.clone()]);
        let p = build_worker_prompt(&w, &next, None, false);
        assert!(p.contains("Learnings left by previous workers"));
        assert!(p.contains("pnpm check"));
    }

    #[test]
    fn wave_prompt_warns_about_scope() {
        let it = item("item-1", "queued");
        let w = wf(vec![it.clone()]);
        let p = build_worker_prompt(&w, &it, None, true);
        assert!(p.contains("IN PARALLEL"));
    }

    #[test]
    fn grader_verdict_parses_with_fence() {
        let raw = "```json\n{\"pass\": true, \"reason\": \"ok\"}\n```";
        let v: GraderVerdict = serde_json::from_str(strip_json_fence(raw)).unwrap();
        assert!(v.pass);
    }

    #[test]
    fn truncate_tail_keeps_end() {
        let s = "a".repeat(100) + "TAIL";
        let t = truncate_tail(&s, 10);
        assert!(t.ends_with("TAIL"));
        assert!(t.starts_with("…(truncated)…"));
    }

    #[test]
    fn deps_gate_readiness() {
        let mut b = item("item-2", "queued");
        b.deps = vec!["item-1".into()];
        // No-dep item is always ready.
        let a = item("item-1", "queued");
        let w = wf(vec![a, b.clone()]);
        assert!(deps_ready(&w, &w.items[0]));
        // item-2 waits until item-1 settles.
        assert!(!deps_ready(&w, &b));
        let w2 = wf(vec![item("item-1", "passed"), b.clone()]);
        assert!(deps_ready(&w2, &b));
        // Unknown dep id never wedges the run.
        let mut c = item("item-3", "queued");
        c.deps = vec!["item-nope".into()];
        assert!(deps_ready(&w2, &c));
    }

    #[test]
    fn container_detection_and_rollup() {
        let parent = item("item-1", "queued");
        let mut child = item("item-2", "queued");
        child.parent_id = Some("item-1".into());
        let mut w = wf(vec![parent, child]);
        assert!(is_container(&w, "item-1"));
        assert!(!is_container(&w, "item-2"));
        // Children still queued → container not yet passed.
        rollup_containers(&mut w);
        assert_eq!(w.items[0].status, "queued");
        // Child passes → container rolls up to passed.
        w.items[1].status = "passed".into();
        rollup_containers(&mut w);
        assert_eq!(w.items[0].status, "passed");
    }

    #[test]
    fn rollup_marks_working_and_failed() {
        let parent = item("item-1", "queued");
        let mut c1 = item("item-2", "working");
        c1.parent_id = Some("item-1".into());
        let mut c2 = item("item-3", "queued");
        c2.parent_id = Some("item-1".into());
        let mut w = wf(vec![parent, c1, c2]);
        rollup_containers(&mut w);
        assert_eq!(w.items[0].status, "working");
        // A failed child fails the group.
        w.items[1].status = "failed".into();
        rollup_containers(&mut w);
        assert_eq!(w.items[0].status, "failed");
    }

    fn dep_item(id: &str, status: &str, deps: &[&str]) -> LedgerItem {
        let mut i = item(id, status);
        i.deps = deps.iter().map(|d| d.to_string()).collect();
        i
    }
    fn status_of<'a>(w: &'a LedgerWorkflow, id: &str) -> &'a str {
        w.items.iter().find(|i| i.id == id).unwrap().status.as_str()
    }

    #[test]
    fn skip_blocks_dependent_subtree() {
        // item-1 skipped → item-2 (deps [1]) and item-3 (deps [2]) block.
        let mut w = wf(vec![
            item("item-1", "skipped"),
            dep_item("item-2", "queued", &["item-1"]),
            dep_item("item-3", "queued", &["item-2"]),
        ]);
        recompute_blocked(&mut w);
        assert_eq!(status_of(&w, "item-2"), "blocked");
        assert_eq!(status_of(&w, "item-3"), "blocked", "block cascades down the chain");
    }

    #[test]
    fn failed_dep_also_blocks() {
        let mut w = wf(vec![
            item("item-1", "failed"),
            dep_item("item-2", "queued", &["item-1"]),
        ]);
        recompute_blocked(&mut w);
        assert_eq!(status_of(&w, "item-2"), "blocked");
    }

    #[test]
    fn pass_unblocks_subtree() {
        // The culprit was retried to green — its blocked subtree re-queues.
        let mut w = wf(vec![
            item("item-1", "passed"),
            dep_item("item-2", "blocked", &["item-1"]),
            dep_item("item-3", "blocked", &["item-2"]),
        ]);
        recompute_blocked(&mut w);
        assert_eq!(status_of(&w, "item-2"), "queued");
        assert_eq!(status_of(&w, "item-3"), "queued", "unblock cascades once the taint clears");
    }

    #[test]
    fn independent_items_never_block() {
        let mut w = wf(vec![
            item("item-1", "skipped"),
            item("item-2", "queued"), // no deps
        ]);
        recompute_blocked(&mut w);
        assert_eq!(status_of(&w, "item-2"), "queued");
    }

    #[test]
    fn recompute_blocked_is_idempotent() {
        let mut w = wf(vec![
            item("item-1", "skipped"),
            dep_item("item-2", "queued", &["item-1"]),
        ]);
        recompute_blocked(&mut w);
        let once = status_of(&w, "item-2").to_string();
        recompute_blocked(&mut w);
        assert_eq!(status_of(&w, "item-2"), once);
    }

    #[test]
    fn picker_runs_first_ready_leaf() {
        let w = wf(vec![item("item-1", "queued"), item("item-2", "queued")]);
        match next_ready_item(&w) {
            ItemPick::Run(i) => assert_eq!(i.id, "item-1"),
            _ => panic!("expected Run(item-1)"),
        }
    }

    #[test]
    fn picker_skips_unready_dep_but_runs_available() {
        // item-2 gated on item-1; picker still finds item-1.
        let w = wf(vec![
            item("item-1", "queued"),
            dep_item("item-2", "queued", &["item-1"]),
        ]);
        match next_ready_item(&w) {
            ItemPick::Run(i) => assert_eq!(i.id, "item-1"),
            _ => panic!("expected Run(item-1)"),
        }
    }

    #[test]
    fn picker_done_when_all_settled() {
        let w = wf(vec![item("item-1", "passed"), item("item-2", "skipped")]);
        assert!(matches!(next_ready_item(&w), ItemPick::Done));
    }

    #[test]
    fn picker_done_not_deadlock_with_blocked_subtree() {
        // THE deadlock-freedom guarantee: a skipped prereq parks its
        // dependent as `blocked`; nothing is queued → Done, never spin.
        let w = wf(vec![
            item("item-1", "skipped"),
            dep_item("item-2", "blocked", &["item-1"]),
        ]);
        assert!(
            matches!(next_ready_item(&w), ItemPick::Done),
            "blocked items must not be treated as queued"
        );
    }

    #[test]
    fn picker_deadlock_on_unsatisfiable_cycle() {
        // Both queued, each waiting on the other → nothing ready but
        // queued leaves remain → hard Deadlock (never an infinite spin).
        let w = wf(vec![
            dep_item("item-1", "queued", &["item-2"]),
            dep_item("item-2", "queued", &["item-1"]),
        ]);
        assert!(matches!(next_ready_item(&w), ItemPick::Deadlock));
    }

    #[test]
    fn deps_ready_gates_on_settled_deps() {
        let w = wf(vec![
            item("item-1", "queued"),
            dep_item("item-2", "queued", &["item-1"]),
        ]);
        assert!(!deps_ready(&w, &w.items[1]), "queued dep not ready");
        let w2 = wf(vec![
            item("item-1", "passed"),
            dep_item("item-2", "queued", &["item-1"]),
        ]);
        assert!(deps_ready(&w2, &w2.items[1]), "passed dep is ready");
        let w3 = wf(vec![
            item("item-1", "skipped"),
            dep_item("item-2", "queued", &["item-1"]),
        ]);
        assert!(deps_ready(&w3, &w3.items[1]), "skipped dep satisfies deps_ready (blocking is separate)");
    }

    #[test]
    fn watchdog_flags_stall_and_timeout() {
        // limits: item wall-clock 600s, stall 120s
        let lim = WorkerLimits { item_secs: 600, stall_secs: 120 };
        // healthy: recent activity, well under wall-clock
        assert_eq!(worker_watchdog(10, 5, lim), WorkerTick::Continue);
        // stalled: no activity for >= stall window
        assert_eq!(worker_watchdog(200, 130, lim), WorkerTick::Stall);
        // timed out: total elapsed >= item window (even if just active)
        assert_eq!(worker_watchdog(601, 1, lim), WorkerTick::Timeout);
        // timeout takes precedence over stall when both trip
        assert_eq!(worker_watchdog(700, 300, lim), WorkerTick::Timeout);
    }
}
