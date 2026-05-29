//! Dynamic Workflows (SDD `sdd-98a42f3bdb` Phase 4). Planner → fan-out
//! → verifier pipeline replicating Anthropic's research-preview feature
//! locally so it works on any Max plan (not just Enterprise / Team).
//!
//! ## V1 status (Phase 4)
//!
//! This module ships:
//!   - Types (`PlannerOutput`, `SubagentSpec`, `DynamicWorkflow`,
//!     `DwSubagentState`) shared with the frontend via serde.
//!   - `validate_planner_output` — schema-light validator (count cap,
//!     unique ids, non-empty prompts).
//!   - `estimate_workflow_cost` — pure helper used by the preflight
//!     modal to show pre-flight $ estimate before user approves.
//!   - Four Tauri commands (`dw_plan`, `dw_approve`, `dw_cancel`,
//!     `dw_status`) + in-memory workflow registry with locking.
//!
//! Deferred to Phase 4.5 (callout in `result.md`):
//!   - Real `run_planner` claude-spawn (currently returns canned
//!     planner output for UI smoke).
//!   - Real `run_fanout` parallel subagent execution (currently emits
//!     synthetic `dw:subagent_done` events with echo-prompt results).
//!   - Real `run_verifier` synthesis turn.
//!
//! The frontend works end-to-end against the V1 mock — preflight modal
//! shows real cost estimates, the DW card grid renders, cancel path
//! cleans worktrees. Real claude-spawn integration takes one focused
//! follow-up phase touching only `dw::run_planner` / `run_fanout` /
//! `run_verifier` internals; the Tauri surface + UI stay frozen.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tokio::sync::Semaphore;
// `Manager` required for `app.state::<DwRegistry>()` — idiomatic Tauri 2
// trait import.
use tauri::{AppHandle, Emitter, Manager};

use crate::claude_quota;
use crate::worktree;

/// Hard cap on subagent count per workflow — matches the spec.
pub const MAX_SUBAGENTS: usize = 20;
/// Default budget cap in USD before the workflow pauses + asks the
/// user to raise (or cancel).
pub const DEFAULT_BUDGET_CAP_USD: f64 = 5.0;

/// Hard ceiling for a single planner/verifier/subagent oneshot turn.
/// Without it a stalled `claude` subprocess hangs the whole `/dw` flow.
const ONESHOT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

// ---- Serde types (mirror the TS shapes in `lib/types.ts`) -----------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubagentSpec {
    pub id: String,
    pub prompt: String,
    pub cwd_strategy: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd_subpath: Option<String>,
    #[serde(default)]
    pub expected_artifacts: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannerOutput {
    pub rationale: String,
    pub subagents: Vec<SubagentSpec>,
    pub verifier_prompt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DwSubagentState {
    pub id: String,
    pub prompt: String,
    pub cwd_strategy: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd_subpath: Option<String>,
    pub expected_artifacts: Vec<String>,
    pub status: String, // 'queued' | 'streaming' | 'done' | 'failed' | 'cancelled'
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claude_uuid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub tokens_in: u64,
    pub tokens_out: u64,
    pub cost_usd: f64,
    /// Unified diff this subagent produced in its worktree (staged vs the
    /// parent HEAD it branched from). Empty / absent = research-only run.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff: Option<String>,
    /// Whether the user has applied this subagent's diff to the parent repo.
    #[serde(default)]
    pub applied: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DynamicWorkflow {
    pub id: String,
    pub session_id: String,
    pub user_prompt: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_rationale: Option<String>,
    pub subagents: Vec<DwSubagentState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verifier_prompt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verifier_result: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub final_answer: Option<String>,
    pub budget_cap_usd: f64,
    pub total_cost_usd: f64,
    /// Account-wide quota utilization (%) this workflow's fan-out pushed
    /// each rolling bucket up by (end − start, clamped ≥0). Lets the
    /// budget popover fold DW spend into the per-session limits line —
    /// DW runs no chat turns, so without this its quota burn was invisible
    /// there. Approximate (bucket is shared with other clients).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quota_delta_5h: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quota_delta_7d: Option<f64>,
    pub created_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<i64>,
    /// Parent session's cwd (repo root) — threaded through `dw_plan`
    /// so the fan-out can create per-subagent worktrees + run
    /// planner/verifier claude turns against the right tree.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_cwd: Option<String>,
}

// ---- In-memory workflow registry ------------------------------------------

#[derive(Default)]
pub struct DwRegistry {
    workflows: Mutex<HashMap<String, DynamicWorkflow>>,
}

impl DwRegistry {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }
    fn upsert(&self, wf: DynamicWorkflow) {
        if let Ok(mut g) = self.workflows.lock() {
            g.insert(wf.id.clone(), wf);
        }
    }
    fn get(&self, id: &str) -> Option<DynamicWorkflow> {
        self.workflows.lock().ok().and_then(|g| g.get(id).cloned())
    }
    fn mutate<F: FnOnce(&mut DynamicWorkflow)>(&self, id: &str, f: F) -> Option<DynamicWorkflow> {
        let mut g = self.workflows.lock().ok()?;
        let wf = g.get_mut(id)?;
        f(wf);
        Some(wf.clone())
    }
    /// Mutate + persist in one shot (Phase 5). Every state transition
    /// flows through here so the disk JSON stays in lockstep with the
    /// in-memory snapshot. Disk failure is best-effort logged inside
    /// `persist_workflow` — never blocks the mutation.
    pub(crate) fn mutate_persist<F: FnOnce(&mut DynamicWorkflow)>(
        &self,
        app: &AppHandle,
        id: &str,
        f: F,
    ) -> Option<DynamicWorkflow> {
        let updated = self.mutate(id, f);
        if let Some(w) = &updated {
            persist_workflow(app, w);
        }
        updated
    }
    /// Snapshot all live workflows for `dw_list` / shutdown probe.
    pub(crate) fn list_ids_by_status(&self, statuses: &[&str]) -> Vec<String> {
        let g = match self.workflows.lock() {
            Ok(g) => g,
            Err(_) => return Vec::new(),
        };
        g.values()
            .filter(|w| statuses.iter().any(|s| *s == w.status))
            .map(|w| w.id.clone())
            .collect()
    }
}

// ---- Persistence (Phase 5) ------------------------------------------------

/// `<app_data>/workflows/` — directory holding one JSON per workflow.
/// `None` when the OS app-data dir can't be resolved (rare; degrades
/// to in-memory-only mode).
pub(crate) fn workflow_storage_root(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_data_dir()
        .ok()
        .map(|p| p.join("workflows"))
}

pub(crate) fn workflow_path(root: &Path, workflow_id: &str) -> PathBuf {
    let safe = workflow_id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect::<String>();
    root.join(format!("{}.json", safe))
}

/// Temp-file + rename pattern mirroring `rtk.rs::atomic_write`. Inlined
/// (rather than crate-shared) until a third caller needs it.
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

/// Best-effort write: serialize + atomic rename. Disk failure is
/// logged via `eprintln!` and otherwise swallowed — never crash a
/// running workflow just because the OS couldn't write.
pub(crate) fn persist_workflow(app: &AppHandle, wf: &DynamicWorkflow) {
    let root = match workflow_storage_root(app) {
        Some(r) => r,
        None => return,
    };
    let path = workflow_path(&root, &wf.id);
    let bytes = match serde_json::to_vec_pretty(wf) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("[dw] serialize workflow {} failed: {}", wf.id, e);
            return;
        }
    };
    if let Err(e) = atomic_write(&path, &bytes) {
        eprintln!("[dw] persist workflow {} failed: {}", wf.id, e);
    }
}

/// Scan `<app_data>/workflows/` for every `*.json` and parse what we
/// can. Corrupt files are logged + skipped, not propagated.
pub(crate) fn load_workflows(root: &Path) -> Vec<DynamicWorkflow> {
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
        // Skip the hidden temp files atomic_write briefly creates.
        if path
            .file_name()
            .and_then(|s| s.to_str())
            .map(|s| s.starts_with('.'))
            .unwrap_or(false)
        {
            continue;
        }
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("[dw] read workflow {:?} failed: {}", path, e);
                continue;
            }
        };
        match serde_json::from_slice::<DynamicWorkflow>(&bytes) {
            Ok(wf) => out.push(wf),
            Err(e) => eprintln!("[dw] parse workflow {:?} failed: {}", path, e),
        }
    }
    out
}

/// Sweep workflows whose terminal `completed_at` is older than
/// `retain_days`. Best-effort worktree cleanup + JSON delete.
/// Returns the count of files removed (logged by callers).
pub(crate) fn cleanup_stale_workflows(app: &AppHandle, retain_days: u32) -> usize {
    let root = match workflow_storage_root(app) {
        Some(r) => r,
        None => return 0,
    };
    let now = unix_ms();
    let cutoff = now - (retain_days as i64) * 86_400_000;
    let mut removed = 0;
    for wf in load_workflows(&root) {
        let terminal = matches!(
            wf.status.as_str(),
            "done" | "failed" | "cancelled"
        );
        if !terminal {
            continue;
        }
        let stale = wf.completed_at.map(|t| t < cutoff).unwrap_or(false);
        if !stale {
            continue;
        }
        if let Some(parent) = &wf.parent_cwd {
            let _ = worktree::cleanup_workflow_worktrees(parent, &wf.id);
        }
        let path = workflow_path(&root, &wf.id);
        if let Err(e) = std::fs::remove_file(&path) {
            eprintln!("[dw] remove stale workflow {:?} failed: {}", path, e);
            continue;
        }
        removed += 1;
    }
    removed
}

/// Startup-time recovery. Walks the storage dir, marks any workflow
/// in a non-terminal status as `failed` with a synthetic interrupted
/// final_answer, and upserts each parsed workflow into the live
/// registry so the frontend's `dw_list` sees them. Emits a single
/// `dw:recovered_interrupted` event with the count of interrupted
/// workflows so the UI can surface one banner (not N).
pub(crate) async fn recover_on_startup(app: AppHandle) {
    let root = match workflow_storage_root(&app) {
        Some(r) => r,
        None => return,
    };
    let registry: tauri::State<'_, Arc<DwRegistry>> = app.state();
    let reg = registry.inner().clone();
    let workflows = load_workflows(&root);
    let mut interrupted = 0usize;
    for mut wf in workflows {
        let non_terminal = matches!(
            wf.status.as_str(),
            "planning" | "running" | "verifying" | "paused_quota" | "awaiting_approval"
        );
        if non_terminal {
            wf.status = "failed".to_string();
            wf.completed_at = Some(unix_ms());
            wf.final_answer = Some(
                "_Workflow interrupted on app shutdown — partial results below._".to_string(),
            );
            interrupted += 1;
            // Persist the flipped state so next launch doesn't re-flip.
            persist_workflow(&app, &wf);
        }
        reg.upsert(wf);
    }
    if interrupted > 0 {
        let _ = app.emit(
            "dw:recovered_interrupted",
            serde_json::json!({ "count": interrupted }),
        );
    }
    // Sweep stale workflows once on startup.
    let removed = cleanup_stale_workflows(&app, 7);
    if removed > 0 {
        eprintln!("[dw] startup sweep removed {} stale workflows", removed);
    }
}

// ---- Validator + cost estimator -------------------------------------------

pub fn validate_planner_output(p: &PlannerOutput) -> Result<(), String> {
    if p.subagents.is_empty() {
        return Err("planner produced zero subagents".into());
    }
    if p.subagents.len() > MAX_SUBAGENTS {
        return Err(format!(
            "planner produced {} subagents — cap is {}",
            p.subagents.len(),
            MAX_SUBAGENTS
        ));
    }
    let mut seen = std::collections::HashSet::new();
    for s in &p.subagents {
        if s.prompt.trim().is_empty() {
            return Err(format!("subagent {} has empty prompt", s.id));
        }
        if !seen.insert(s.id.clone()) {
            return Err(format!("duplicate subagent id: {}", s.id));
        }
        if s.cwd_strategy != "inherit" && s.cwd_strategy != "subpath" {
            return Err(format!(
                "subagent {} has invalid cwdStrategy: {}",
                s.id, s.cwd_strategy
            ));
        }
    }
    if p.verifier_prompt.trim().is_empty() {
        return Err("verifier prompt is empty".into());
    }
    Ok(())
}

/// USD per 1M tokens, mirroring `apps/desktop/src/lib/usage.ts`
/// RATE_TABLE. Kept in sync manually — both files are versioned
/// alongside the model release cadence. Returns `(input_rate,
/// output_rate)`; unknown model id → Sonnet defaults so estimate is
/// at least plausible.
fn rate_for(model: &str, fast: bool) -> (f64, f64) {
    let key_fast = if fast { ":fast" } else { "" };
    match format!("{}{}", model, key_fast).as_str() {
        "claude-opus-4-8" => (5.0, 25.0),
        "claude-opus-4-8[1m]" => (10.0, 50.0),
        "claude-opus-4-8:fast" => (10.0, 50.0),
        "claude-opus-4-8[1m]:fast" => (20.0, 100.0),
        "claude-opus-4-7" => (15.0, 75.0),
        "claude-sonnet-4-6" => (3.0, 15.0),
        _ => (3.0, 15.0),
    }
}

pub fn estimate_workflow_cost(plan: &PlannerOutput, model: &str, fast: bool) -> f64 {
    let (r_in, r_out) = rate_for(model, fast);
    let n = plan.subagents.len() as f64;
    // 3K avg in + 3K avg out per subagent + 5K total for verifier.
    let cost = (n * 3_000.0 * r_in
        + n * 3_000.0 * r_out
        + 5_000.0 * r_in
        + 0.0_f64.max(5_000.0 * r_out))
        / 1_000_000.0;
    // ×1.2 safety margin so the user isn't surprised when actual lands
    // above estimate.
    cost * 1.2
}

// ---- Real planner / fanout / verifier --------------------------------------

const PLANNER_SCHEMA: &str = r#"{
  "rationale": "string — 1-2 sentences justifying the decomposition",
  "subagents": [
    {
      "id": "string — 'sub-1', 'sub-2', ...",
      "prompt": "string — self-contained instructions, no cross-subagent dependencies",
      "cwdStrategy": "inherit" | "subpath",
      "cwdSubpath": "string (only when cwdStrategy=subpath, e.g. 'src/auth/')",
      "expectedArtifacts": ["string", ...]
    }
  ],
  "verifierPrompt": "string — instructions for the verifier turn"
}"#;

fn build_planner_prompt(user_prompt: &str) -> String {
    format!(
        "You are a workflow planner. Decompose the user's task into INDEPENDENT subagents \
         (max 20). Each subagent's prompt MUST be self-contained — no cross-subagent \
         dependencies (the verifier resolves any conflicts).\n\n\
         Output ONLY a single JSON object matching the schema below. NO markdown fences, \
         NO prose before or after the JSON, NO explanation.\n\nSchema:\n{}\n\nUser request:\n{}",
        PLANNER_SCHEMA, user_prompt
    )
}

fn strip_json_fence(s: &str) -> &str {
    let s = s.trim();
    let inner = s
        .strip_prefix("```json")
        .or_else(|| s.strip_prefix("```"))
        .map(|r| r.trim_start_matches('\n'))
        .unwrap_or(s);
    inner.strip_suffix("```").unwrap_or(inner).trim()
}

fn parse_planner_json(raw: &str) -> Result<PlannerOutput, String> {
    let stripped = strip_json_fence(raw.trim());
    let p: PlannerOutput = serde_json::from_str(stripped).map_err(|e| {
        format!(
            "planner JSON parse: {} — raw first 200 chars: {}",
            e,
            raw.chars().take(200).collect::<String>()
        )
    })?;
    validate_planner_output(&p)?;
    Ok(p)
}

/// Snapshot account-wide 5H / 7D quota utilization (%). Returns (0, 0) on
/// any failure so callers can diff start/end without error handling.
async fn fetch_quota_util() -> (f64, f64) {
    match claude_quota::fetch_plan_usage().await {
        Ok(snap) => (
            snap.five_hour.as_ref().and_then(|b| b.utilization).unwrap_or(0.0),
            snap.seven_day.as_ref().and_then(|b| b.utilization).unwrap_or(0.0),
        ),
        Err(_) => (0.0, 0.0),
    }
}

async fn call_oneshot(
    prompt: &str,
    cwd: Option<&Path>,
    model: &str,
    timeout: Option<std::time::Duration>,
) -> Result<(String, Option<crate::claude::OneshotUsage>), String> {
    let status = crate::claude::detect();
    if !status.detected {
        return Err("claude CLI not installed".into());
    }
    if !status.ready {
        return Err("claude CLI not authenticated".into());
    }
    let bin = status.path.as_deref().unwrap_or("claude");
    let fut = crate::claude::run_claude_oneshot(bin, prompt, None, None, cwd, Some(model));
    // The timeout only guards the PLANNER (a quick decompose turn whose
    // hang originally froze `/dw` with no UI feedback). Subagents +
    // verifier do real, open-ended work — surveying a codebase across
    // many tool calls routinely runs minutes — so they pass `None` and
    // run unbounded, gated instead by the budget cap, quota-pause, and
    // user cancel. A short shared ceiling here was killing legit
    // subagent turns (3/6 timing out).
    let resp = match timeout {
        Some(dur) => match tokio::time::timeout(dur, fut).await {
            Ok(r) => r.map_err(|e| format!("claude oneshot: {}", e))?,
            Err(_) => {
                return Err(format!("claude oneshot timed out after {}s", dur.as_secs()))
            }
        },
        None => fut.await.map_err(|e| format!("claude oneshot: {}", e))?,
    };
    Ok((resp.text, resp.usage))
}

/// One real Opus 4.8 turn that decomposes the user's task into a planner
/// JSON envelope. Strict-JSON system prompt + single retry on parse /
/// validation failure (`"your previous output was invalid…"` follow-up).
async fn run_planner(
    user_prompt: &str,
    cwd: Option<&Path>,
    model: &str,
) -> Result<PlannerOutput, String> {
    let prompt = build_planner_prompt(user_prompt);
    let (raw, _usage) = call_oneshot(&prompt, cwd, model, Some(ONESHOT_TIMEOUT)).await?;
    match parse_planner_json(&raw) {
        Ok(p) => Ok(p),
        Err(parse_err) => {
            let retry_prompt = format!(
                "Your previous output failed JSON parsing ({}). Regenerate, strictly \
                 matching the schema. NO markdown fences. NO prose. JUST the JSON object.\n\n\
                 Schema:\n{}\n\nOriginal request:\n{}",
                parse_err, PLANNER_SCHEMA, user_prompt
            );
            let (raw2, _usage2) =
                call_oneshot(&retry_prompt, cwd, model, Some(ONESHOT_TIMEOUT)).await?;
            parse_planner_json(&raw2)
        }
    }
}

/// Real fan-out: sequentially create per-subagent worktrees for every
/// queued subagent, then hand the id list to `run_subagents_subset`.
/// The subset helper holds the per-subagent spawn body (quota
/// poll-pause + auto-resume, real-usage cost, budget mid-run check)
/// and is reused by the verifier-retry path.
async fn run_fanout(
    app: AppHandle,
    reg: Arc<DwRegistry>,
    workflow_id: String,
    parent_cwd: PathBuf,
    model: String,
) -> Result<(), String> {
    let wf = reg.get(&workflow_id).ok_or("workflow disappeared")?;
    let cap = wf.budget_cap_usd;
    let parent_cwd_str = parent_cwd.to_string_lossy().to_string();

    // Stage 1: sequential worktree creation. Parallel `git worktree add`
    // contends on `.git/index.lock`, so we serialise this phase.
    let mut to_run: Vec<String> = Vec::new();
    for sub in wf.subagents.iter() {
        if sub.status != "queued" {
            continue;
        }
        match worktree::create_for_subagent(&parent_cwd_str, &workflow_id, &sub.id) {
            Ok(w) => {
                let path = w.path.clone();
                reg.mutate_persist(&app, &workflow_id, |x| {
                    for s in x.subagents.iter_mut() {
                        if s.id == sub.id {
                            s.worktree_path = Some(path.clone());
                        }
                    }
                });
                to_run.push(sub.id.clone());
            }
            Err(e) => {
                reg.mutate_persist(&app, &workflow_id, |x| {
                    for s in x.subagents.iter_mut() {
                        if s.id == sub.id {
                            s.status = "failed".to_string();
                            s.error = Some(format!("worktree: {}", e));
                        }
                    }
                });
                let _ = app.emit(
                    "dw:subagent_done",
                    serde_json::json!({
                        "workflowId": workflow_id,
                        "subagentId": sub.id,
                        "result": "",
                        "tokensIn": 0,
                        "tokensOut": 0,
                        "costUsd": 0.0,
                        "error": format!("worktree: {}", e),
                    }),
                );
            }
        }
    }

    let sem = Arc::new(Semaphore::new(4));
    let cancel = Arc::new(AtomicBool::new(false));
    run_subagents_subset(&app, &reg, &workflow_id, &model, cap, to_run, sem, cancel).await;
    Ok(())
}

/// Per-subagent spawn body extracted out so the verifier-retry path
/// can re-run a subset of subagents through the same plumbing. Each
/// call: (a) loops a 60s `fetch_plan_usage` poll until both buckets
/// drop below 95% (or cancel trips), emitting `dw:paused_quota` /
/// `dw:resumed_quota` around the wait so the card reflects the live
/// state; (b) spawns a `tokio::spawn` claude turn under the shared
/// `Semaphore(20)`; (c) reads real `usage` from the envelope when the
/// CLI emits it, falling back to a 4-char-per-token estimate when
/// `usage` is absent; (d) short-circuits via `cancel` + `dw:budget_exceeded`
/// when running total crosses the cap.
async fn run_subagents_subset(
    app: &AppHandle,
    reg: &Arc<DwRegistry>,
    wf_id: &str,
    model: &str,
    cap: f64,
    ids: Vec<String>,
    sem: Arc<Semaphore>,
    cancel: Arc<AtomicBool>,
) {
    let mut handles = Vec::with_capacity(ids.len());
    for sub_id in ids {
        if cancel.load(Ordering::SeqCst) {
            break;
        }

        // Quota poll-pause loop. Re-checks every 60s until both buckets
        // drop below 95% OR cancel trips. Emits `dw:paused_quota` once
        // on entry + `dw:resumed_quota` on exit so the card flips.
        let mut paused_emitted = false;
        loop {
            let hot = match claude_quota::fetch_plan_usage().await {
                Ok(snap) => {
                    let p5 = snap
                        .five_hour
                        .as_ref()
                        .and_then(|b| b.utilization)
                        .unwrap_or(0.0);
                    let p7 = snap
                        .seven_day
                        .as_ref()
                        .and_then(|b| b.utilization)
                        .unwrap_or(0.0);
                    if p5 >= 95.0 || p7 >= 95.0 {
                        Some((p5, p7))
                    } else {
                        None
                    }
                }
                Err(_) => None,
            };
            match hot {
                None => break,
                Some((p5, p7)) => {
                    if !paused_emitted {
                        reg.mutate_persist(app, wf_id, |w| {
                            w.status = "paused_quota".to_string();
                        });
                        let _ = app.emit(
                            "dw:paused_quota",
                            serde_json::json!({
                                "workflowId": wf_id,
                                "pct5h": p5,
                                "pct7d": p7,
                            }),
                        );
                        paused_emitted = true;
                    }
                    tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                    if cancel.load(Ordering::SeqCst) {
                        return;
                    }
                }
            }
        }
        if paused_emitted {
            reg.mutate_persist(app, wf_id, |w| {
                w.status = "running".to_string();
            });
            let _ = app.emit(
                "dw:resumed_quota",
                serde_json::json!({ "workflowId": wf_id }),
            );
        }

        // Snapshot the prompt + worktree path before the move into the
        // spawned task — we can't borrow across the await boundary.
        let snap = reg.get(wf_id);
        let (prompt, worktree_path) = match snap.and_then(|w| {
            w.subagents.iter().find(|s| s.id == sub_id).cloned()
        }) {
            Some(s) => (s.prompt, s.worktree_path.unwrap_or_default()),
            None => continue,
        };

        // Throttle SPAWNING here, not inside the task. Block until a
        // permit frees so at most `Semaphore` subagents are launched
        // before we re-check `cancel`. Previously every task was spawned
        // instantly (spawn doesn't block) and acquired its permit
        // internally, so the budget-cap `cancel` set by a finishing task
        // couldn't stop the already-spawned rest — the cap overshot
        // badly ($8 on a $5 cap). With the permit gate, once the cap
        // trips the loop bails on its next acquire, bounding overshoot
        // to the in-flight batch.
        let permit = match sem.clone().acquire_owned().await {
            Ok(p) => p,
            Err(_) => break,
        };
        // A finishing task may have tripped the cap / cancel while we
        // waited for the permit — don't launch another.
        if cancel.load(Ordering::SeqCst) {
            drop(permit);
            break;
        }

        let cancel_t = cancel.clone();
        let app_t = app.clone();
        let reg_t = reg.clone();
        let wf_id_t = wf_id.to_string();
        let sub_id_t = sub_id.clone();
        let model_t = model.to_string();
        let worktree_buf = PathBuf::from(worktree_path);

        let h = tauri::async_runtime::spawn(async move {
            // Permit moved in — held for the turn, released on drop.
            let _permit = permit;
            if cancel_t.load(Ordering::SeqCst) {
                return;
            }
            reg_t.mutate_persist(&app_t, &wf_id_t, |w| {
                for s in w.subagents.iter_mut() {
                    if s.id == sub_id_t {
                        s.status = "streaming".to_string();
                    }
                }
            });
            let result = call_oneshot(&prompt, Some(&worktree_buf), &model_t, None).await;
            let (text, usage, error) = match result {
                Ok((t, u)) => (t, u, None),
                Err(e) => (String::new(), None, Some(e)),
            };
            // Cost the four token buckets at their REAL rates. The old
            // code summed cache-read + cache-creation into `input` and
            // charged the whole lot at the fresh-input rate — but cache
            // read is 10× cheaper and cache write is 1.25×, so a turn
            // that re-read a big cached prompt was billed ~10× too high
            // (single subagents showing $11). Standard Anthropic ratios:
            // cacheRead = input×0.1, cacheWrite = input×1.25.
            let (input_t, cache_read_t, cache_write_t, out_tokens) = match &usage {
                Some(u) => (
                    u.input_tokens,
                    u.cache_read_input_tokens,
                    u.cache_creation_input_tokens,
                    u.output_tokens,
                ),
                None => ((prompt.len() / 4) as u64, 0u64, 0u64, (text.len() / 4) as u64),
            };
            // `tokens_in` stays the full input footprint for the card's
            // token display; only the COST weights the buckets.
            let in_tokens = input_t + cache_read_t + cache_write_t;
            let (r_in, r_out) = (5.0_f64, 25.0_f64);
            let cost = (input_t as f64 * r_in
                + cache_read_t as f64 * r_in * 0.1
                + cache_write_t as f64 * r_in * 1.25
                + out_tokens as f64 * r_out)
                / 1_000_000.0;
            let new_status = if error.is_some() { "failed" } else { "done" };
            // Capture what the subagent changed in its worktree (staged vs
            // the branch base) so the card can show a diff + offer Apply.
            // Empty = research-only run. Best-effort; a capture failure just
            // leaves `diff` None.
            let diff = if error.is_none() {
                match worktree::capture_diff(&worktree_buf.to_string_lossy()) {
                    Ok(d) if !d.trim().is_empty() => Some(d),
                    _ => None,
                }
            } else {
                None
            };
            let updated = reg_t.mutate_persist(&app_t, &wf_id_t, |w| {
                for s in w.subagents.iter_mut() {
                    if s.id == sub_id_t {
                        s.status = new_status.to_string();
                        s.result = if error.is_none() {
                            Some(text.clone())
                        } else {
                            None
                        };
                        s.error = error.clone();
                        s.tokens_in = in_tokens;
                        s.tokens_out = out_tokens;
                        s.cost_usd = cost;
                        s.diff = diff.clone();
                    }
                }
                w.total_cost_usd = w.subagents.iter().map(|s| s.cost_usd).sum();
            });
            let _ = app_t.emit(
                "dw:subagent_done",
                serde_json::json!({
                    "workflowId": wf_id_t,
                    "subagentId": sub_id_t,
                    "result": text,
                    "tokensIn": in_tokens,
                    "tokensOut": out_tokens,
                    "costUsd": cost,
                    "error": error,
                    "diff": diff,
                }),
            );
            // Budget cap enforcement removed — it overshot wildly under
            // parallel fan-out and the numbers misled more than helped.
            // `total_cost_usd` is now purely informational; the only
            // stop controls are the user's Cancel button + quota-pause.
            let _ = updated;
        });
        handles.push(h);
    }

    for h in handles {
        let _ = h.await;
    }
}

/// One real Opus 4.8 turn that synthesises subagent results into the
/// final answer. JSON-strict envelope `{synthesis, conflicts_found,
/// retry_subagents}`. When `retried == false` AND `retry_subagents` is
/// non-empty: reset those subagents' status to `queued`, re-run them
/// via `run_subagents_subset` (worktrees still exist — cleanup runs
/// AFTER verifier success), then recurse with `retried=true` so the
/// retry path is bounded to exactly one round.
async fn run_verifier(
    app: AppHandle,
    reg: Arc<DwRegistry>,
    workflow_id: String,
    parent_cwd: PathBuf,
    model: String,
    retried: bool,
) -> Result<String, String> {
    let wf = reg.get(&workflow_id).ok_or("workflow disappeared")?;
    let verifier_prompt = wf.verifier_prompt.clone().unwrap_or_else(|| {
        "Synthesise subagent outputs into a single coherent answer. Flag conflicts.".to_string()
    });
    let mut parts = String::new();
    for s in wf.subagents.iter() {
        if let Some(r) = &s.result {
            parts.push_str(&format!("## {}\n{}\n\n", s.id, r));
        } else if let Some(err) = &s.error {
            parts.push_str(&format!("## {} (FAILED: {})\n\n", s.id, err));
        }
    }
    let prompt = format!(
        "{}\n\nSubagent results:\n\n{}\n\nOutput ONLY a JSON object (no markdown fences, no prose):\n\
         {{\"synthesis\": \"<consolidated answer>\", \"conflicts_found\": [\"...\"], \"retry_subagents\": [\"sub-id\", ...]}}",
        verifier_prompt, parts
    );
    let (raw, _usage) = call_oneshot(&prompt, Some(&parent_cwd), &model, None).await?;
    let stripped = strip_json_fence(raw.trim());
    // Graceful degrade: the verifier sometimes answers in prose instead
    // of the requested JSON. Previously a parse failure errored the
    // WHOLE workflow ("Dynamic Workflow failed: verifier JSON parse"),
    // throwing away every subagent result. Now a non-JSON verifier reply
    // is used verbatim as the synthesis — the user still gets the
    // consolidated answer; only the structured retry/conflict fields are
    // skipped.
    let val_opt = serde_json::from_str::<serde_json::Value>(stripped).ok();
    let synthesis = match &val_opt {
        Some(v) => {
            let s = v
                .get("synthesis")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            if s.is_empty() {
                raw.trim().to_string()
            } else {
                s
            }
        }
        None => raw.trim().to_string(),
    };
    let retry_ids: Vec<String> = val_opt
        .as_ref()
        .and_then(|v| v.get("retry_subagents"))
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    if !retried && !retry_ids.is_empty() {
        // Reset those subagents' status to 'queued' + clear prior result
        // / error so `run_subagents_subset` picks them up fresh. Worktree
        // paths stay intact — cleanup happens after the verifier branch
        // finalises in `dw_approve`, so re-runs reuse the same trees.
        reg.mutate_persist(&app, &workflow_id, |w| {
            for s in w.subagents.iter_mut() {
                if retry_ids.contains(&s.id) {
                    s.status = "queued".to_string();
                    s.result = None;
                    s.error = None;
                }
            }
        });
        let cap = reg
            .get(&workflow_id)
            .map(|w| w.budget_cap_usd)
            .unwrap_or(DEFAULT_BUDGET_CAP_USD);
        let sem = Arc::new(Semaphore::new(4));
        let cancel = Arc::new(AtomicBool::new(false));
        run_subagents_subset(
            &app,
            &reg,
            &workflow_id,
            &model,
            cap,
            retry_ids.clone(),
            sem,
            cancel,
        )
        .await;
        // Recurse with retried=true so the retry branch can't loop.
        // `Box::pin` because `async fn` can't recurse directly.
        return Box::pin(run_verifier(
            app,
            reg,
            workflow_id,
            parent_cwd,
            model,
            true,
        ))
        .await;
    }

    if synthesis.is_empty() {
        return Err("verifier returned empty synthesis".into());
    }
    Ok(synthesis)
}

// ---- Tauri commands -------------------------------------------------------

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DwPlanResult {
    pub workflow_id: String,
    pub plan: PlannerOutput,
    pub estimate_usd: f64,
}

#[tauri::command]
pub async fn dw_plan(
    app: AppHandle,
    user_prompt: String,
    session_id: String,
    cwd: Option<String>,
) -> Result<DwPlanResult, String> {
    let registry: tauri::State<'_, Arc<DwRegistry>> = app.state();
    let model = "claude-opus-4-8";
    let cwd_path = cwd.as_deref().map(Path::new);
    let plan = run_planner(&user_prompt, cwd_path, model).await?;
    let estimate_usd = estimate_workflow_cost(&plan, model, false);
    let workflow_id = format!("dw-{}", uuid_v4());
    let now_ms = unix_ms();
    let subagents: Vec<DwSubagentState> = plan
        .subagents
        .iter()
        .map(|s| DwSubagentState {
            id: s.id.clone(),
            prompt: s.prompt.clone(),
            cwd_strategy: s.cwd_strategy.clone(),
            cwd_subpath: s.cwd_subpath.clone(),
            expected_artifacts: s.expected_artifacts.clone(),
            status: "queued".to_string(),
            claude_uuid: None,
            worktree_path: None,
            result: None,
            error: None,
            tokens_in: 0,
            tokens_out: 0,
            cost_usd: 0.0,
            diff: None,
            applied: false,
        })
        .collect();
    let wf = DynamicWorkflow {
        id: workflow_id.clone(),
        session_id,
        user_prompt,
        status: "awaiting_approval".to_string(),
        plan_rationale: Some(plan.rationale.clone()),
        subagents,
        verifier_prompt: Some(plan.verifier_prompt.clone()),
        verifier_result: None,
        final_answer: None,
        budget_cap_usd: DEFAULT_BUDGET_CAP_USD,
        total_cost_usd: 0.0,
        quota_delta_5h: None,
        quota_delta_7d: None,
        created_at: now_ms,
        started_at: None,
        completed_at: None,
        parent_cwd: cwd,
    };
    // Persist the planned workflow alongside the in-memory insert so a
    // crash between plan + approve doesn't leak an awaiting_approval
    // workflow as in-memory-only.
    persist_workflow(&app, &wf);
    registry.inner().upsert(wf);
    Ok(DwPlanResult {
        workflow_id,
        plan,
        estimate_usd,
    })
}

#[tauri::command]
pub async fn dw_approve(
    app: AppHandle,
    workflow_id: String,
    budget_cap_usd: Option<f64>,
) -> Result<(), String> {
    let registry: tauri::State<'_, Arc<DwRegistry>> = app.state();
    let reg = registry.inner().clone();
    let cap = budget_cap_usd.unwrap_or(DEFAULT_BUDGET_CAP_USD);
    let wf = reg
        .mutate_persist(&app, &workflow_id, |w| {
            w.status = "running".to_string();
            w.budget_cap_usd = cap;
            w.started_at = Some(unix_ms());
        })
        .ok_or_else(|| format!("workflow not found: {}", workflow_id))?;
    let _ = app.emit("dw:workflow_started", &wf);

    let parent_cwd = wf
        .parent_cwd
        .as_deref()
        .map(PathBuf::from)
        .ok_or_else(|| "workflow has no parent cwd — pass `cwd` to dw_plan".to_string())?;
    let model = "claude-opus-4-8".to_string();
    // Quota utilization at fan-out start — diffed at done to attribute
    // this workflow's burn to the per-session limits line.
    let (q5_start, q7_start) = fetch_quota_util().await;

    let app_clone = app.clone();
    let reg_clone = reg.clone();
    let wf_id_clone = workflow_id.clone();
    tauri::async_runtime::spawn(async move {
        // 1. Fan-out — real parallel claude turns under Semaphore(20).
        if let Err(e) = run_fanout(
            app_clone.clone(),
            reg_clone.clone(),
            wf_id_clone.clone(),
            parent_cwd.clone(),
            model.clone(),
        )
        .await
        {
            let _ = reg_clone.mutate_persist(&app_clone, &wf_id_clone, |w| {
                w.status = "failed".to_string();
                w.completed_at = Some(unix_ms());
            });
            let _ = app_clone.emit(
                "dw:workflow_done",
                serde_json::json!({
                    "workflowId": wf_id_clone,
                    "error": format!("fanout: {}", e),
                }),
            );
            return;
        }

        // If fan-out paused on quota, leave the workflow in
        // `paused_quota` state — verifier won't run until the user
        // re-approves once quota recovers.
        let after_fanout = reg_clone.get(&wf_id_clone);
        if let Some(w) = &after_fanout {
            if w.status == "paused_quota" || w.status == "cancelled" {
                let _ = app_clone.emit("dw:workflow_done", w);
                return;
            }
        }

        // 2. Verifier.
        let _ = reg_clone.mutate_persist(&app_clone, &wf_id_clone, |w| {
            w.status = "verifying".to_string();
        });
        let synthesis = match run_verifier(
            app_clone.clone(),
            reg_clone.clone(),
            wf_id_clone.clone(),
            parent_cwd.clone(),
            model.clone(),
            false,
        )
        .await
        {
            Ok(s) => s,
            Err(e) => {
                let _ = reg_clone.mutate_persist(&app_clone, &wf_id_clone, |w| {
                    w.status = "failed".to_string();
                    w.completed_at = Some(unix_ms());
                });
                let _ = app_clone.emit(
                    "dw:workflow_done",
                    serde_json::json!({
                        "workflowId": wf_id_clone,
                        "error": format!("verifier: {}", e),
                    }),
                );
                return;
            }
        };

        // Attribute quota burn: end − start, clamped ≥0 (a window reset
        // mid-run would otherwise read negative).
        let (q5_end, q7_end) = fetch_quota_util().await;
        let final_wf = reg_clone
            .mutate_persist(&app_clone, &wf_id_clone, |w| {
                w.status = "done".to_string();
                w.verifier_result = Some(synthesis.clone());
                w.final_answer = Some(synthesis);
                w.completed_at = Some(unix_ms());
                w.quota_delta_5h = Some((q5_end - q5_start).max(0.0));
                w.quota_delta_7d = Some((q7_end - q7_start).max(0.0));
            })
            .unwrap();

        // 3. Best-effort worktree cleanup once we've harvested results.
        if let Some(p) = &final_wf.parent_cwd {
            let _ = worktree::cleanup_workflow_worktrees(p, &wf_id_clone);
        }

        let _ = app_clone.emit("dw:workflow_done", &final_wf);
    });
    Ok(())
}

#[tauri::command]
pub async fn dw_cancel(app: AppHandle, workflow_id: String) -> Result<(), String> {
    let registry: tauri::State<'_, Arc<DwRegistry>> = app.state();
    let wf = registry
        .inner()
        .mutate_persist(&app, &workflow_id, |w| {
            w.status = "cancelled".to_string();
            w.completed_at = Some(unix_ms());
        })
        .ok_or_else(|| format!("workflow not found: {}", workflow_id))?;
    // Best-effort worktree cleanup using the workflow's parent cwd
    // (the original repo root). Real fan-out populates parent_cwd
    // via dw_plan, so this call now actually reaps the dw worktrees.
    if let Some(p) = &wf.parent_cwd {
        let _ = worktree::cleanup_workflow_worktrees(p, &workflow_id);
    }
    let _ = app.emit("dw:workflow_cancelled", &wf);
    Ok(())
}

/// Apply one subagent's captured diff to the parent repo's working tree.
/// User-gated (the card's per-subagent Apply button). Parallel subagent
/// diffs can overlap, so this applies ONE at a time and surfaces a real
/// conflict as an Err — the user resolves / skips. Marks the subagent
/// `applied` on success so the card can show it's landed.
#[tauri::command]
pub async fn dw_apply_subagent(
    app: AppHandle,
    workflow_id: String,
    subagent_id: String,
) -> Result<(), String> {
    let registry: tauri::State<'_, Arc<DwRegistry>> = app.state();
    let wf = registry
        .inner()
        .get(&workflow_id)
        .ok_or_else(|| format!("workflow not found: {}", workflow_id))?;
    let parent = wf
        .parent_cwd
        .clone()
        .ok_or_else(|| "workflow has no parent cwd".to_string())?;
    let patch = wf
        .subagents
        .iter()
        .find(|s| s.id == subagent_id)
        .and_then(|s| s.diff.clone())
        .ok_or_else(|| format!("subagent {} has no diff to apply", subagent_id))?;
    worktree::apply_patch(&parent, &patch)?;
    registry.inner().mutate_persist(&app, &workflow_id, |w| {
        for s in w.subagents.iter_mut() {
            if s.id == subagent_id {
                s.applied = true;
            }
        }
    });
    Ok(())
}

#[tauri::command]
pub async fn dw_status(
    app: AppHandle,
    workflow_id: String,
) -> Result<Option<DynamicWorkflow>, String> {
    let registry: tauri::State<'_, Arc<DwRegistry>> = app.state();
    Ok(registry.inner().get(&workflow_id))
}

/// Light summary entry returned by `dw_list` — enough to render the
/// card chrome without paying the full workflow JSON parse cost when
/// the user has dozens of historical runs.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DwSummary {
    pub id: String,
    pub status: String,
    pub session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<i64>,
    pub total_cost_usd: f64,
    pub subagent_count: usize,
    pub user_prompt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_rationale: Option<String>,
    pub created_at: i64,
}

#[tauri::command]
pub async fn dw_list(app: AppHandle) -> Result<Vec<DwSummary>, String> {
    let root = match workflow_storage_root(&app) {
        Some(r) => r,
        None => return Ok(Vec::new()),
    };
    let mut out: Vec<DwSummary> = load_workflows(&root)
        .into_iter()
        .map(|w| DwSummary {
            id: w.id,
            status: w.status,
            session_id: w.session_id,
            completed_at: w.completed_at,
            total_cost_usd: w.total_cost_usd,
            subagent_count: w.subagents.len(),
            user_prompt: w.user_prompt,
            plan_rationale: w.plan_rationale,
            created_at: w.created_at,
        })
        .collect();
    // Newest first — created_at descending.
    out.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(out)
}

#[tauri::command]
pub async fn dw_get(
    app: AppHandle,
    workflow_id: String,
) -> Result<Option<DynamicWorkflow>, String> {
    let registry: tauri::State<'_, Arc<DwRegistry>> = app.state();
    // Prefer in-memory snapshot (live workflows); fall back to disk for
    // historical / interrupted ones not yet hydrated into the registry.
    if let Some(w) = registry.inner().get(&workflow_id) {
        return Ok(Some(w));
    }
    let root = match workflow_storage_root(&app) {
        Some(r) => r,
        None => return Ok(None),
    };
    let path = workflow_path(&root, &workflow_id);
    if !path.exists() {
        return Ok(None);
    }
    let bytes = std::fs::read(&path).map_err(|e| format!("read workflow: {}", e))?;
    let wf: DynamicWorkflow = serde_json::from_slice(&bytes)
        .map_err(|e| format!("parse workflow: {}", e))?;
    // Cache the on-disk parse into the registry for future calls.
    registry.inner().upsert(wf.clone());
    Ok(Some(wf))
}

/// Lightweight close-window probe — returns id list of workflows the
/// frontend should warn the user about before tearing down.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DwRunningProbe {
    pub count: usize,
    pub ids: Vec<String>,
}

#[tauri::command]
pub async fn dw_has_running(app: AppHandle) -> Result<DwRunningProbe, String> {
    let registry: tauri::State<'_, Arc<DwRegistry>> = app.state();
    let ids = registry
        .inner()
        .list_ids_by_status(&["planning", "running", "verifying", "paused_quota"]);
    Ok(DwRunningProbe {
        count: ids.len(),
        ids,
    })
}

/// SIGTERM + cleanup every in-flight workflow. Called from the
/// close-window guard when the user confirms "Close anyway?".
#[tauri::command]
pub async fn dw_cancel_all(app: AppHandle) -> Result<usize, String> {
    let registry: tauri::State<'_, Arc<DwRegistry>> = app.state();
    let ids = registry
        .inner()
        .list_ids_by_status(&["planning", "running", "verifying", "paused_quota"]);
    let count = ids.len();
    for id in ids {
        let wf = registry.inner().mutate_persist(&app, &id, |w| {
            w.status = "cancelled".to_string();
            w.completed_at = Some(unix_ms());
        });
        if let Some(w) = wf {
            if let Some(p) = &w.parent_cwd {
                let _ = worktree::cleanup_workflow_worktrees(p, &id);
            }
            let _ = app.emit("dw:workflow_cancelled", &w);
        }
    }
    Ok(count)
}

// ---- helpers --------------------------------------------------------------

fn unix_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn uuid_v4() -> String {
    // Stripped-down v4 — relies on `rand`/`getrandom` if available;
    // otherwise falls back to time+pid hash. The Cargo workspace already
    // has `uuid` via the desktop crate; reuse it via the existing dep.
    uuid::Uuid::new_v4().to_string()
}

// ---- Tests ----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_plan(n: usize) -> PlannerOutput {
        let mut subs = Vec::new();
        for i in 0..n {
            subs.push(SubagentSpec {
                id: format!("sub-{}", i + 1),
                prompt: format!("prompt {}", i + 1),
                cwd_strategy: "inherit".to_string(),
                cwd_subpath: None,
                expected_artifacts: vec![],
            });
        }
        PlannerOutput {
            rationale: "r".to_string(),
            subagents: subs,
            verifier_prompt: "v".to_string(),
        }
    }

    #[test]
    fn validate_rejects_empty_subagents() {
        let p = sample_plan(0);
        assert!(validate_planner_output(&p).is_err());
    }

    #[test]
    fn validate_caps_at_max_subagents() {
        let p = sample_plan(MAX_SUBAGENTS + 1);
        assert!(validate_planner_output(&p).is_err());
    }

    #[test]
    fn validate_rejects_duplicate_ids() {
        let mut p = sample_plan(2);
        p.subagents[1].id = "sub-1".to_string();
        assert!(validate_planner_output(&p).is_err());
    }

    #[test]
    fn validate_rejects_empty_prompt() {
        let mut p = sample_plan(2);
        p.subagents[1].prompt = "   ".to_string();
        assert!(validate_planner_output(&p).is_err());
    }

    #[test]
    fn validate_rejects_empty_verifier_prompt() {
        let mut p = sample_plan(2);
        p.verifier_prompt = "".to_string();
        assert!(validate_planner_output(&p).is_err());
    }

    #[test]
    fn validate_accepts_well_formed_plan() {
        let p = sample_plan(5);
        assert!(validate_planner_output(&p).is_ok());
    }

    #[test]
    fn estimate_workflow_cost_opus48_3sub_reasonable() {
        let p = sample_plan(3);
        let c = estimate_workflow_cost(&p, "claude-opus-4-8", false);
        // n=3, (3 × 3K × $5/M + 3 × 3K × $25/M + 5K × $5/M + 5K × $25/M) × 1.2
        // = (0.045 + 0.225 + 0.025 + 0.125) × 1.2 = 0.42 × 1.2 = 0.504
        assert!((c - 0.504).abs() < 0.01, "expected ~0.504, got {}", c);
    }

    #[test]
    fn estimate_doubles_for_fast_mode() {
        let p = sample_plan(3);
        let base = estimate_workflow_cost(&p, "claude-opus-4-8", false);
        let fast = estimate_workflow_cost(&p, "claude-opus-4-8", true);
        assert!((fast - base * 2.0).abs() < 0.001);
    }

    #[test]
    fn strip_json_fence_handles_bare_object() {
        assert_eq!(strip_json_fence("{\"a\":1}"), "{\"a\":1}");
    }

    #[test]
    fn strip_json_fence_handles_json_fence() {
        let s = "```json\n{\"a\":1}\n```";
        assert_eq!(strip_json_fence(s), "{\"a\":1}");
    }

    #[test]
    fn strip_json_fence_handles_bare_fence() {
        let s = "```\n{\"a\":1}\n```";
        assert_eq!(strip_json_fence(s), "{\"a\":1}");
    }

    #[test]
    fn parse_planner_json_round_trip() {
        let raw = r#"{
          "rationale": "test",
          "subagents": [
            {"id": "sub-1", "prompt": "do X", "cwdStrategy": "inherit", "expectedArtifacts": ["diff"]}
          ],
          "verifierPrompt": "synthesise"
        }"#;
        let p = parse_planner_json(raw).unwrap();
        assert_eq!(p.subagents.len(), 1);
        assert_eq!(p.subagents[0].id, "sub-1");
        assert_eq!(p.rationale, "test");
    }

    #[test]
    fn parse_planner_json_strips_markdown_fence() {
        let raw = "```json\n{\"rationale\":\"r\",\"subagents\":[{\"id\":\"sub-1\",\"prompt\":\"p\",\"cwdStrategy\":\"inherit\",\"expectedArtifacts\":[]}],\"verifierPrompt\":\"v\"}\n```";
        let p = parse_planner_json(raw).unwrap();
        assert_eq!(p.subagents[0].id, "sub-1");
    }

    #[test]
    fn parse_planner_json_rejects_malformed() {
        assert!(parse_planner_json("not json").is_err());
    }

    #[test]
    fn oneshot_usage_round_trip_from_envelope() {
        // Mirror what `claude -p --output-format json` typically returns.
        let envelope = serde_json::json!({
            "result": "ok",
            "usage": {
                "input_tokens": 1200,
                "output_tokens": 450,
                "cache_read_input_tokens": 800,
                "cache_creation_input_tokens": 100,
            }
        });
        let parsed: crate::claude::OneshotUsage = serde_json::from_value(
            envelope.get("usage").cloned().unwrap()
        ).unwrap();
        assert_eq!(parsed.input_tokens, 1200);
        assert_eq!(parsed.output_tokens, 450);
        assert_eq!(parsed.cache_read_input_tokens, 800);
        assert_eq!(parsed.cache_creation_input_tokens, 100);
    }

    #[test]
    fn oneshot_usage_tolerates_missing_fields() {
        let v = serde_json::json!({});
        let parsed: crate::claude::OneshotUsage = serde_json::from_value(v).unwrap();
        assert_eq!(parsed.input_tokens, 0);
        assert_eq!(parsed.output_tokens, 0);
    }

    #[test]
    fn parse_planner_json_rejects_over_cap() {
        let mut subs = String::new();
        for i in 0..(MAX_SUBAGENTS + 1) {
            if i > 0 {
                subs.push(',');
            }
            subs.push_str(&format!(
                "{{\"id\":\"sub-{}\",\"prompt\":\"p\",\"cwdStrategy\":\"inherit\",\"expectedArtifacts\":[]}}",
                i + 1
            ));
        }
        let raw = format!(
            "{{\"rationale\":\"r\",\"subagents\":[{}],\"verifierPrompt\":\"v\"}}",
            subs
        );
        assert!(parse_planner_json(&raw).is_err());
    }
}

// Persistence tests in a top-level sub-module so
// `cargo test dw::persistence` (literally as the phase spec wrote it)
// matches all four cases. Helpers are inlined here to keep the module
// self-contained.
#[cfg(test)]
mod persistence {
    use super::*;

    /// Minimal tempdir for tests — `tempfile` isn't in our deps, so
    /// we roll our own with cleanup on drop.
    struct TestDir(PathBuf);
    impl TestDir {
        fn new() -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "woom-dw-test-{}-{}",
                std::process::id(),
                nanos
            ));
            std::fs::create_dir_all(&path).unwrap();
            TestDir(path)
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn sample_workflow(id: &str, status: &str, completed_ms_ago: Option<i64>) -> DynamicWorkflow {
        let now = unix_ms();
        DynamicWorkflow {
            id: id.to_string(),
            session_id: "sess-1".to_string(),
            user_prompt: "test prompt".to_string(),
            status: status.to_string(),
            plan_rationale: Some("r".to_string()),
            subagents: vec![DwSubagentState {
                id: "sub-1".to_string(),
                prompt: "p".to_string(),
                cwd_strategy: "inherit".to_string(),
                cwd_subpath: None,
                expected_artifacts: vec![],
                status: "done".to_string(),
                claude_uuid: None,
                worktree_path: None,
                result: Some("ok".to_string()),
                error: None,
                tokens_in: 100,
                tokens_out: 200,
                cost_usd: 0.005,
                diff: None,
                applied: false,
            }],
            verifier_prompt: Some("v".to_string()),
            verifier_result: None,
            final_answer: None,
            budget_cap_usd: 5.0,
            total_cost_usd: 0.005,
            quota_delta_5h: None,
            quota_delta_7d: None,
            created_at: now - 1000,
            started_at: Some(now - 500),
            completed_at: completed_ms_ago.map(|ago| now - ago),
            parent_cwd: None,
        }
    }

    #[test]
    fn persist_workflow_round_trip() {
        let tmp = TestDir::new();
        let wf = sample_workflow("dw-roundtrip", "done", Some(1000));
        let bytes = serde_json::to_vec_pretty(&wf).unwrap();
        let path = workflow_path(tmp.path(), &wf.id);
        atomic_write(&path, &bytes).unwrap();
        let loaded = load_workflows(tmp.path());
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, wf.id);
        assert_eq!(loaded[0].status, "done");
        assert_eq!(loaded[0].subagents.len(), 1);
        assert_eq!(loaded[0].subagents[0].cost_usd, 0.005);
    }

    #[test]
    fn load_workflows_skips_corrupt_files() {
        let tmp = TestDir::new();
        let wf = sample_workflow("dw-good", "done", Some(1000));
        atomic_write(
            &workflow_path(tmp.path(), &wf.id),
            &serde_json::to_vec_pretty(&wf).unwrap(),
        )
        .unwrap();
        std::fs::write(tmp.path().join("garbage.json"), b"not json {{").unwrap();
        std::fs::write(tmp.path().join("README.md"), b"# notes").unwrap();
        let loaded = load_workflows(tmp.path());
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, "dw-good");
    }

    #[test]
    fn cleanup_stale_drops_old_terminal_only() {
        let tmp = TestDir::new();
        let eight_days_ms = 8 * 86_400_000i64;
        let one_day_ms = 86_400_000i64;
        let old = sample_workflow("dw-old", "done", Some(eight_days_ms));
        let recent = sample_workflow("dw-recent", "done", Some(one_day_ms));
        let mut running = sample_workflow("dw-running", "running", Some(eight_days_ms));
        running.status = "running".to_string();
        for wf in [&old, &recent, &running] {
            atomic_write(
                &workflow_path(tmp.path(), &wf.id),
                &serde_json::to_vec_pretty(wf).unwrap(),
            )
            .unwrap();
        }
        let cutoff = unix_ms() - 7 * 86_400_000;
        let mut removed = 0;
        for wf in load_workflows(tmp.path()) {
            let terminal = matches!(wf.status.as_str(), "done" | "failed" | "cancelled");
            let stale = wf.completed_at.map(|t| t < cutoff).unwrap_or(false);
            if terminal && stale {
                std::fs::remove_file(workflow_path(tmp.path(), &wf.id)).unwrap();
                removed += 1;
            }
        }
        assert_eq!(removed, 1);
        let remaining: Vec<String> =
            load_workflows(tmp.path()).into_iter().map(|w| w.id).collect();
        assert!(remaining.contains(&"dw-recent".to_string()));
        assert!(remaining.contains(&"dw-running".to_string()));
        assert!(!remaining.contains(&"dw-old".to_string()));
    }

    #[test]
    fn recover_flips_running_to_failed_interrupted() {
        let mut wf = sample_workflow("dw-mid", "running", None);
        let non_terminal = matches!(
            wf.status.as_str(),
            "planning" | "running" | "verifying" | "paused_quota" | "awaiting_approval"
        );
        assert!(non_terminal);
        wf.status = "failed".to_string();
        wf.completed_at = Some(unix_ms());
        wf.final_answer = Some(
            "_Workflow interrupted on app shutdown — partial results below._".to_string(),
        );
        assert_eq!(wf.status, "failed");
        assert!(wf.final_answer.as_deref().unwrap().contains("interrupted"));
        assert!(wf.completed_at.is_some());
    }
}
