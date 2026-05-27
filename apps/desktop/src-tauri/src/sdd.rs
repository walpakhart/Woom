//! Spec-Driven Development orchestrator.
//!
//! Workflow: user kicks off SDD ↦ agent writes a `spec.md` skeleton then a
//! `plan.md` with N phases ↦ user approves each gate ↦ agent executes
//! phases one-by-one ↦ each phase writes a `results/NN-result.md` summary
//! and flips its own frontmatter `status: completed`.
//!
//! All files live in a TEMP workspace at
//! `<app_data>/sdd-workspaces/<workspace_id>/` so the user's repo stays
//! clean (no `.planning/` pollution). One workspace per /sdd invocation;
//! workspaces survive app restarts and can be resumed later (planned —
//! v1 wipes on session change).
//!
//! State is filesystem-backed exactly like brain's orchestrator: each
//! file's YAML frontmatter carries its own status, so an external reader
//! (or a crashed-and-restarted Woom) can rebuild the workspace state by
//! reading the files. No SQLite, no manifest.
//!
//! Frontend interaction model: orchestrator (TS) calls `sdd_start`,
//! `sdd_refresh_state`, `sdd_approve`, etc. Each call returns a fresh
//! `SddWorkspace` snapshot the UI renders as an inline chat card.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
// `SystemTime`/`UNIX_EPOCH` moved with `now_ms` to sdd_time.rs.

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};

const WORKSPACE_DIR: &str = "sdd-workspaces";

// now_ms + short_id moved to ./sdd_time.rs (wave-1 phase-10 split).
use crate::sdd_time::{now_ms, short_id};

// ---------------------------------------------------------------------------
// Public types — serialized to the frontend over Tauri IPC.
// ---------------------------------------------------------------------------

/// Coarse state of the workspace — drives which card variant the UI shows.
/// `tag` discriminator + `data` payload keeps JSON ergonomic for the
/// Svelte side (`{ kind: "phase_running", phase: 2 }`).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum SddStage {
    /// User just kicked off — agent is gathering requirements / writing spec.
    Drafting,
    /// `spec.md` exists; awaiting user approve.
    SpecReady,
    /// Agent is writing `plan.md` with phase files.
    Planning,
    /// `plan.md` + phases exist; awaiting user approve to start execution.
    PlanReady,
    /// V2 only — plan approved; the next phase is gated on per-phase user
    /// approval. The card shows the phase body + a "Start phase N"
    /// button. Cleared by `sdd_approve_phase` writing
    /// `control/phase-N-approved`.
    PhasePendingApproval { phase: u32 },
    /// Phase `phase` is being executed by the agent. Three-call mode
    /// emits the finer-grained `PhasePlanning` / `PhaseImplementing` /
    /// `PhaseVerifying` variants instead; this variant stays for
    /// single-call mode + as the fallback when `substep-state.json`
    /// is absent (legacy hydrate path).
    PhaseRunning { phase: u32 },
    /// Three-call mode only — agent is producing the plan-pass output
    /// (read-only analysis written verbatim to
    /// `phases/<slug>/plan.md`). Drives the "Phase N — planning" badge
    /// in the SddCard. Cleared by `sdd_save_phase_plan` advancing
    /// substep-state to `Implement` (or `Plan` review gate).
    /// See `spec-1` FR-1 and `plan-1` §State machine.
    PhasePlanning { phase: u32 },
    /// Three-call mode only — plan.md exists, plan-gate is enabled
    /// (`phase_execution.plan_gate = true`), and
    /// `control/phase-<N>-plan-approved` is NOT yet present. SddCard
    /// surfaces the plan body + Approve / Amend / Discard buttons.
    /// `sdd_approve_phase_plan` writes the gate marker to advance.
    /// See `spec-1` FR-7.
    PhasePlanReview { phase: u32 },
    /// Three-call mode only — agent is executing the plan against the
    /// repo (edits land here). Same in-flight semantics as
    /// `PhaseRunning` but distinguished so the SddCard badge can
    /// announce "implementing" specifically and the action_log can
    /// tag rows with `sub_step: implement`.
    /// See `spec-1` FR-3.
    PhaseImplementing { phase: u32 },
    /// Three-call mode — agent is running the verify-pass (self-review
    /// producing structured JSON written to `phases/<slug>/verify.json`).
    /// In single-call mode this variant stays unused (placeholder kept
    /// for v2 roadmap continuity); three-call wires it via
    /// `sdd_save_phase_verify`.
    /// See `spec-1` FR-4.
    PhaseVerifying { phase: u32 },
    /// Phase `phase` finished; about to advance or awaiting approval.
    PhaseDone { phase: u32 },
    /// All phases done; final summary card up.
    Complete,
    /// User pressed pause — orchestrator stops scheduling new phases.
    Paused,
    /// User pressed stop — workspace is dead, no more execution.
    Stopped,
    /// Unrecoverable error. Structured payload — `reason` retained for
    /// backward-compat with the original v1 `Failed { reason }` shape so
    /// existing UI code reading `stage.reason` keeps working; new fields
    /// (`failed_phase`, `trigger`, `failed_checks`, `action_log_tail`)
    /// drive the v2 failure card. All new fields default to safe empty
    /// values so callers that don't populate them (e.g. tests) still
    /// produce a valid Failed.
    Failed {
        reason: String,
        #[serde(default)]
        failed_phase: Option<u32>,
        #[serde(default)]
        trigger: Option<FailureTrigger>,
        #[serde(default)]
        failed_checks: Vec<u32>,
        #[serde(default)]
        action_log_tail: Vec<ActionLogEntry>,
    },
}

/// Coarse classification of why a phase entered `failed`. Drives the
/// human-readable header + which inline-action makes sense (`retry` for
/// `Timeout`/`CheckFailed`/`Crash`, `skip` for `UserStopped`, …). The
/// verifier (sdd_verify.rs) emits `CheckFailed`; future fault paths
/// (e.g. agent crash detection) will fill the others.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FailureTrigger {
    /// Acceptance check ran, exit-code or stdout-match didn't satisfy.
    CheckFailed,
    /// Verifier shell command exceeded its `timeout_ms`.
    Timeout,
    /// Agent threw / runtime error during phase execution.
    Exception,
    /// Phase entered `running` and never produced a result — detected on
    /// app boot via `recovery_state = OrphanPhase`.
    Crash,
    /// User explicitly stopped the workspace.
    UserStopped,
    /// Three-call mode — the plan-pass agent mutated files on disk
    /// despite the read-only contract. Detected by the pre/post
    /// `git status --porcelain` sentinel around the plan call.
    /// See `spec-1` NFR-rel-3 + `plan-1` §Prompt + agent flow.
    PlanMutatedDisk,
    /// Three-call mode — verify-pass produced a JSON payload with
    /// non-empty `deviations`. The phase is flipped to `failed` so
    /// the existing Retry / Edit & retry / Skip cards can act on it.
    /// See `spec-1` FR-5.
    VerifyFailed,
    /// Three-call mode — verify-pass output failed to parse as the
    /// expected JSON schema AND no hard-gate acceptance check rescued
    /// the phase. Only surfaces when there's NOTHING more specific
    /// to report; `VerifyOutput::parse_or_fallback` masks most
    /// parse errors with a fallback deviation row.
    /// See `spec-1` NFR-rel-2.
    VerifyParseFail,
    /// Three-call mode — user clicked Discard during the plan-review
    /// gate (`PhasePlanReview`). The phase is failed so the standard
    /// failure-card recovery flow (Retry / Edit & retry / Skip) is
    /// available.
    /// See `spec-1` FR-7.
    PlanDiscarded,
}

/// A single phase as parsed from `phases/NN-*.md`. Both frontmatter and
/// body are kept so the UI can render a full preview without re-reading
/// the file.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SddPhase {
    pub number: u32,
    pub slug: String,
    pub title: String,
    pub depends_on: Vec<u32>,
    /// Mirror of the file's frontmatter `status:` — `pending | running |
    /// done | failed | skipped`. Free-form string because the agent may
    /// invent statuses we don't anticipate; UI handles unknowns gracefully.
    pub status: String,
    pub tasks_total: u32,
    pub tasks_done: u32,
    /// Markdown body — the actual instructions for the agent. Held in
    /// memory only because UI may want to preview it before approving.
    pub body: String,
    /// Absolute path on disk — handy for the UI's "open in editor" button.
    pub path: String,
    /// Optional summary appearing after the phase finishes (read from
    /// `results/NN-result.md`, populated by `refresh_state`).
    pub summary: Option<String>,
    /// Three-call mode artifact — `phases/<slug>/plan.md`. None when
    /// missing (single-call mode or three-call still pre-plan-pass).
    /// Surfaces in the SddCard Plan tab and plan-review pane.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_body: Option<String>,
    /// Three-call mode artifact — `phases/<slug>/verify.json` parsed
    /// into the structured `VerifyOutput`. None when missing.
    /// Surfaces in the SddCard Verify tab.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verify: Option<VerifyOutput>,
}

/// One workspace = one SDD session. Everything orchestrator-side hangs
/// off this snapshot. Identity is `id`; the directory at `root` is the
/// source of truth (we mirror state into Rust on each `refresh_state`).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SddWorkspace {
    pub id: String,
    /// Owning Woom session — workspaces are scoped per chat session so
    /// switching sessions doesn't bleed SDD state across contexts.
    pub session_id: Option<String>,
    /// Absolute path of the workspace dir on disk.
    pub root: String,
    pub stage: SddStage,
    /// Original user ask — pinned at workspace creation so we always
    /// have the "north star" even after later turns rewrite spec.
    pub user_prompt: String,
    pub spec_path: Option<String>,
    pub spec_body: Option<String>,
    pub plan_path: Option<String>,
    pub plan_body: Option<String>,
    /// Final wrap-up written by the agent after every phase is done.
    /// Source: `<workspace>/SUMMARY.md`. Optional — older workspaces
    /// (or workflows the user discarded mid-flight) don't have one.
    pub summary_path: Option<String>,
    pub summary_body: Option<String>,
    pub phases: Vec<SddPhase>,
    pub created_at: u64,
    pub updated_at: u64,
    /// V2 workspace flag — true iff `plan.json` exists at the workspace
    /// root. v2 workspaces honour the per-phase approval gate
    /// (`PhasePendingApproval`); v1 workspaces fall through to the legacy
    /// auto-advance flow (`PlanReady` / `PhaseDone` directly fire the
    /// next phase prompt).
    #[serde(default)]
    pub is_v2: bool,
    /// Crash-recovery hint computed on hydrate / refresh. `None` in the
    /// happy path. `Some(OrphanPhase{..})` when a phase status is
    /// `running` or `verifying` but its `phase-N-meta.json` lacks a
    /// `post_phase_sha` — almost always means the agent process was
    /// killed mid-phase and the user should be offered "rollback" or
    /// "keep state, mark failed".
    #[serde(default)]
    pub recovery_state: Option<SddRecoveryState>,
    /// Three-call execution config mirrored from `meta.json#phase_execution`
    /// on every refresh. Cached on the snapshot so `derive_stage`, the
    /// SddCard, and the prompt-builder all read the same value without
    /// each having to re-read meta.json. See `spec-1` FR-11.
    #[serde(default)]
    pub phase_execution: PhaseExecutionConfig,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SddRecoveryState {
    /// Phase `phase` was interrupted; `pre_phase_sha` is where we'd
    /// roll back to, or None if git was disabled when the phase
    /// approved (then "rollback" is unavailable and the recover path
    /// only offers "keep state, mark failed").
    OrphanPhase {
        phase: u32,
        pre_phase_sha: Option<String>,
        /// Three-call mode — sub-step that was in flight when the
        /// app crashed. Read from `substep-state.json` during the
        /// hydrate scan. Drives the recovery banner copy ("Phase N
        /// interrupted during implement"). None for single-call
        /// workspaces or workspaces without a checkpoint. See
        /// `spec-1` NFR-rel-1.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sub_step: Option<SddPhaseSubstep>,
    },
}

// Plan-as-data types (SddPlanFile/SddPlanPhase/SddPhaseAcceptance) +
// read_plan_json + write_plan_json moved to ./sdd_plan.rs (wave-1
// phase-10 split). Re-exported here so existing call sites keep
// using `crate::sdd::SddPlanFile` etc.
#[allow(unused_imports)]
pub use crate::sdd_plan::{SddPhaseAcceptance, SddPlanFile, SddPlanPhase};
#[allow(unused_imports)]
pub(crate) use crate::sdd_plan::{read_plan_json, write_plan_json};

// Frontmatter parsing moved to `sdd_frontmatter.rs` (wave-1 phase-10
// split). Re-export the types here so existing call sites don't have
// to update their `use` lines.
#[allow(unused_imports)]
use crate::sdd_frontmatter::parse_frontmatter;
#[allow(unused_imports)]
use crate::sdd_frontmatter::FrontMatter;
#[allow(unused_imports)]
use crate::sdd_frontmatter::write_with_frontmatter;

// ---------------------------------------------------------------------------
// Registry + lifecycle.
// ---------------------------------------------------------------------------

/// Inner shared state — held inside an `Arc` so the file-watcher
/// thread can keep its own clone and refresh workspaces without going
/// through Tauri's `State<'_, SddRegistry>` (which is bound to the
/// command's lifetime).
pub(crate) type SharedWorkspaces = Arc<RwLock<HashMap<String, Arc<RwLock<SddWorkspace>>>>>;

pub struct SddRegistry {
    pub(crate) workspaces: SharedWorkspaces,
    pub(crate) base_dir: RwLock<Option<PathBuf>>,
    /// File-system watcher — one `notify::RecommendedWatcher` for the
    /// whole base dir. Lazy-initialized on first `sdd_start`. Held in
    /// the struct so it doesn't get dropped (drop = stop watching).
    pub(crate) watcher: parking_lot::Mutex<Option<notify::RecommendedWatcher>>,
}

impl SddRegistry {
    pub fn new() -> Self {
        Self {
            workspaces: Arc::new(RwLock::new(HashMap::new())),
            base_dir: RwLock::new(None),
            watcher: parking_lot::Mutex::new(None),
        }
    }

    fn ensure_base_dir(&self, app: &AppHandle) -> Result<PathBuf, String> {
        if let Some(p) = self.base_dir.read().as_ref() {
            return Ok(p.clone());
        }
        let base = app
            .path()
            .app_data_dir()
            .map_err(|e| format!("app_data_dir: {e}"))?
            .join(WORKSPACE_DIR);
        std::fs::create_dir_all(&base).map_err(|e| format!("mkdir {}: {e}", base.display()))?;
        *self.base_dir.write() = Some(base.clone());
        Ok(base)
    }
}

impl Default for SddRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Compute the absolute path for a workspace, no I/O.
fn workspace_root(base: &Path, id: &str) -> PathBuf {
    base.join(id)
}

// rebuild_from_disk moved to ./sdd_hydrate.rs (wave-6 split).
use crate::sdd_hydrate::rebuild_from_disk;

// phase_number_from moved to ./sdd_hydrate.rs (wave-6 split).
#[allow(unused_imports)]
use crate::sdd_hydrate::phase_number_from;

// derive_stage moved to ./sdd_hydrate.rs (wave-6 split). Used via
// rebuild_from_disk; no direct caller left in this file, but kept
// as a re-export for callers outside the module that still reach
// through `crate::sdd::derive_stage`.
#[allow(unused_imports)]
pub(crate) use crate::sdd_hydrate::derive_stage;

/// Mutate the `status:` field of a markdown file's frontmatter on disk
/// WITHOUT losing other fields the agent might have added. We read the
/// raw YAML into a generic `serde_yaml::Mapping`, patch the `status`
/// key, and re-serialize — round-tripping preserves any custom fields
/// (verification commands, etc) that our typed FrontMatter doesn't
/// know about.
// Markdown file mutators moved to ./sdd_md_mutators.rs (wave-5 split).
#[allow(unused_imports)]
use crate::sdd_md_mutators::{
    replace_body_on, reset_phase_status, set_status_and_summary_on, set_status_on,
};

/// Split markdown content into raw YAML frontmatter string + body, no
/// parsing. Used by `set_status_on` to preserve unknown fields.
#[allow(unused_imports)]
use crate::sdd_frontmatter::split_frontmatter_raw;

// format_iso + days_to_ymd moved to ./sdd_time.rs (wave-1 phase-10 split).
#[allow(unused_imports)]
use crate::sdd_time::{days_to_ymd, format_iso};

// Prompt templates + `sdd_prompt` command moved to ./sdd_prompts.rs
// (wave-1 phase-10 split). SddPromptKind re-exported here for any
// remaining call site in this file.
#[allow(unused_imports)]
pub use crate::sdd_prompts::SddPromptKind;

// ---------------------------------------------------------------------------
// Tauri commands.
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Deserialize)]
pub struct SddStartArgs {
    pub session_id: Option<String>,
    pub user_prompt: String,
    /// Absolute path the agent / editor session is working in. When set
    /// AND the path is a git repo, the orchestrator mints `sdd/<id>` and
    /// snapshots / commits per phase against that repo. Optional — when
    /// omitted (or pointing at a non-git dir), the workspace runs in
    /// degraded mode (`git_enabled: false`) and skips snapshot / commit
    /// / rollback. The frontend wires this to `session.cwd` of the
    /// linked agent at SDD-start time.
    #[serde(default)]
    pub repo_cwd: Option<String>,
}

#[tauri::command]
pub async fn sdd_start(
    app: AppHandle,
    registry: State<'_, SddRegistry>,
    args: SddStartArgs,
) -> Result<SddWorkspace, String> {
    let base = registry.ensure_base_dir(&app)?;
    let id = short_id();
    let root = workspace_root(&base, &id);
    std::fs::create_dir_all(root.join("phases"))
        .map_err(|e| format!("mkdir phases: {e}"))?;
    std::fs::create_dir_all(root.join("results"))
        .map_err(|e| format!("mkdir results: {e}"))?;
    std::fs::create_dir_all(root.join("control"))
        .map_err(|e| format!("mkdir control: {e}"))?;

    let ws = SddWorkspace {
        id: id.clone(),
        session_id: args.session_id,
        root: root.to_string_lossy().into_owned(),
        stage: SddStage::Drafting,
        user_prompt: args.user_prompt,
        spec_path: None,
        spec_body: None,
        plan_path: None,
        plan_body: None,
        summary_path: None,
        summary_body: None,
        phases: Vec::new(),
        created_at: now_ms(),
        updated_at: now_ms(),
        is_v2: false,
        recovery_state: None,
        // New workspaces default to three-call per spec FR-11; legacy
        // workspaces hydrated from disk override this in `sdd_hydrate`
        // (sets `SingleCall` so in-flight workflows don't change
        // behaviour mid-execution).
        phase_execution: PhaseExecutionConfig {
            mode: PhaseExecutionMode::ThreeCall,
            ..PhaseExecutionConfig::default()
        },
    };
    let cell = Arc::new(RwLock::new(ws.clone()));
    registry.workspaces.write().insert(id.clone(), cell);
    /* Git startup — only meaningful when the caller passed a
     *  `repo_cwd` AND it's actually a git work tree. We swallow
     *  errors here (best-effort): a failure to mint the SDD branch
     *  shouldn't abort the whole `sdd_start` flow, the workspace
     *  is still usable in degraded mode. */
    let mut git_enabled = false;
    let mut sdd_branch: Option<String> = None;
    let mut parent_sha: Option<String> = None;
    if let Some(cwd) = args.repo_cwd.as_deref().filter(|s| !s.is_empty()) {
        if crate::git::is_git_repo(cwd) {
            git_enabled = true;
            let branch_name = format!("sdd/{}", id);
            // Capture HEAD before branching so we keep a "where did
            // this workspace start" pointer even if HEAD moves later.
            parent_sha = crate::git::head_sha(cwd).ok();
            if let Err(e) = crate::git::create_branch_at_head(cwd, &branch_name) {
                eprintln!(
                    "[sdd] workspace={id} repo={cwd}: sdd_branch create failed: {e}"
                );
            } else {
                sdd_branch = Some(branch_name);
            }
        }
    }
    /* Persist side-car metadata so the workspace can be restored after
     *  app restart with full context (session binding, original ask,
     *  git provenance). */
    let meta = SddMeta {
        user_prompt: Some(ws.user_prompt.clone()),
        session_id: ws.session_id.clone(),
        created_at: Some(ws.created_at),
        repo_cwd: args.repo_cwd.clone(),
        git_enabled,
        sdd_branch,
        parent_sha,
        // Mirror the workspace's runtime config onto meta.json so a
        // subsequent hydrate / restart recovers the same mode without
        // a separate save step. See `plan-1` §Compatibility + migration.
        phase_execution: ws.phase_execution.clone(),
    };
    let _ = write_workspace_meta(&root, &meta);
    /* Boot the FS watcher if this is the first workspace. Failures here
     *  are non-fatal — we fall back to the explicit `sdd_refresh` poll
     *  that the orchestrator calls after each agent turn. */
    let _ = ensure_watcher(&app, registry.workspaces.clone(), &registry.watcher, &base);
    emit_changed(&app, &id);
    Ok(ws)
}

/// Scan `<app_data>/sdd-workspaces/*` and rebuild the registry from
/// what's on disk. Called once from the frontend on app boot so
/// previously-created workspaces survive a restart and re-attach to
/// their session id.
///
/// We DON'T try to be clever about associating workspaces to currently-
/// open sessions — the workspace stores its own `session_id` in the
/// frontmatter of `spec.md` (well, in v2 it will; for v1 we just trust
/// the dir name). The frontend store uses `workspaceBySession` to
/// surface the right one in each chat's card.
#[tauri::command]
pub async fn sdd_hydrate(
    app: AppHandle,
    registry: State<'_, SddRegistry>,
) -> Result<Vec<SddWorkspace>, String> {
    let base = registry.ensure_base_dir(&app)?;
    let mut out: Vec<SddWorkspace> = Vec::new();
    let entries = match std::fs::read_dir(&base) {
        Ok(e) => e,
        Err(_) => return Ok(out),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let id = match path.file_name().and_then(|s| s.to_str()) {
            Some(s) if s.starts_with("sdd-") => s.to_string(),
            _ => continue,
        };
        // Already in memory from the previous session? Webview reloads
        // re-fire `sdd_hydrate` but the Tauri backend state survives —
        // every in-memory workspace must still flow into `out` or the
        // frontend store wipes itself ("0 SDD workspaces") even though
        // the workspace dir on disk is intact. Push the current
        // snapshot under a short-lived read lock, then move on.
        if let Some(cell) = registry.workspaces.read().get(&id).cloned() {
            out.push(cell.read().clone());
            continue;
        }
        /* Bootstrap with placeholder values; rebuild_from_disk fills
         *  spec/plan/phases and recomputes the stage. session_id and
         *  user_prompt are recovered from a `meta.json` if present
         *  (we'll write that in a follow-up); for now they stay null. */
        let mut ws = SddWorkspace {
            id: id.clone(),
            session_id: None,
            root: path.to_string_lossy().into_owned(),
            stage: SddStage::Drafting,
            user_prompt: String::new(),
            spec_path: None,
            spec_body: None,
            plan_path: None,
            plan_body: None,
            summary_path: None,
            summary_body: None,
            phases: Vec::new(),
            created_at: now_ms(),
            updated_at: now_ms(),
            is_v2: false,
            recovery_state: None,
            // Hydrate sets this from `meta.json#phase_execution` below.
            // Legacy workspaces lacking the block get the default
            // (SingleCall) so they keep their existing behaviour
            // mid-execution. See `plan-1` §Compatibility + migration.
            phase_execution: PhaseExecutionConfig::default(),
        };
        let _ = rebuild_from_disk(&mut ws);
        // Recover user_prompt + session_id + phase_execution from optional meta.json side-car.
        // Migration: legacy meta.json files written before `spec-1` lack
        // the `phase_execution` block entirely. We detect this via a raw
        // JSON probe (cheap), set the default block to single_call so
        // in-flight workflows don't shift mid-execution, persist the
        // migrated meta.json back to disk, and emit an audit row so
        // the orchestrator's trail captures the one-shot rewrite.
        let meta_path = path.join("meta.json");
        if let Ok(meta_raw) = std::fs::read_to_string(&meta_path) {
            let needs_migration = serde_json::from_str::<serde_json::Value>(&meta_raw)
                .map(|v| v.get("phase_execution").is_none())
                .unwrap_or(false);
            if let Ok(meta) = serde_json::from_str::<SddMeta>(&meta_raw) {
                ws.user_prompt = meta.user_prompt.clone().unwrap_or_default();
                ws.session_id = meta.session_id.clone();
                ws.created_at = meta.created_at.unwrap_or(ws.created_at);
                ws.phase_execution = meta.phase_execution.clone();
                if needs_migration {
                    let migrated = SddMeta {
                        phase_execution: PhaseExecutionConfig::default(),
                        ..meta
                    };
                    let _ = write_workspace_meta(&path, &migrated);
                    audit::append(
                        &path,
                        &audit::AuditEntry::new("system", "phase_execution_migrated")
                            .with_after(serde_json::json!({"mode": "single_call"})),
                    );
                }
            }
        }
        let cell = Arc::new(RwLock::new(ws.clone()));
        registry.workspaces.write().insert(id.clone(), cell);
        out.push(ws);
    }
    /* Boot the watcher once we have workspaces on disk to observe. */
    let _ = ensure_watcher(&app, registry.workspaces.clone(), &registry.watcher, &base);
    Ok(out)
}

// PhaseExecutionMode + PhaseExecutionConfig + budget defaults moved
// to ./sdd_phase_config.rs (wave-1 phase-10 split).
pub use crate::sdd_phase_config::{PhaseExecutionConfig, PhaseExecutionMode};

// SddMeta + SddPhaseMeta moved to ./sdd_meta.rs (wave-1 phase-10 split).
#[allow(unused_imports)]
pub(crate) use crate::sdd_meta::{SddMeta, SddPhaseMeta};

/// Structured output produced by the three-call verify pass.
/// Mirrors the JSON schema instructed in
// VerifyOutput + strip_json_fences + phases/<slug>/{plan.md, verify.json}
// I/O helpers moved to ./sdd_phase_io.rs (wave-1 phase-10 split).
pub use crate::sdd_phase_io::VerifyOutput;
#[allow(unused_imports)]
pub(crate) use crate::sdd_phase_io::{
    read_phase_plan_md, read_verify_json, write_phase_plan_md, write_verify_json,
};

// SddPhaseSubstep + SddPhaseSubstepState moved to ./sdd_substep.rs
// (wave-1 phase-10 split).
#[allow(unused_imports)]
pub use crate::sdd_substep::{SddPhaseSubstep, SddPhaseSubstepState};

// substep-state R/W helpers moved to ./sdd_substep.rs (wave-2 follow-up).
#[allow(unused_imports)]
pub(crate) use crate::sdd_substep::{clear_substep_state, read_substep_state, write_substep_state};

// phase-meta R/W helpers moved to ./sdd_meta.rs (wave-2 follow-up).
#[allow(unused_imports)]
pub(crate) use crate::sdd_meta::{read_phase_meta, write_phase_meta};

// read_failed_check_indices + read_action_log_tail moved to
// ./sdd_hydrate.rs (wave-6 split).
#[allow(unused_imports)]
pub(crate) use crate::sdd_hydrate::{read_action_log_tail, read_failed_check_indices};

// workspace_meta R/W moved to ./sdd_meta.rs (wave-6 follow-up).
#[allow(unused_imports)]
use crate::sdd_meta::{read_workspace_meta, write_workspace_meta};

#[tauri::command]
pub async fn sdd_get(
    registry: State<'_, SddRegistry>,
    id: String,
) -> Result<SddWorkspace, String> {
    let cell = registry
        .workspaces
        .read()
        .get(&id)
        .cloned()
        .ok_or_else(|| format!("unknown workspace {id}"))?;
    /* Re-derive from disk before returning. The in-memory cell can
     * drift between commands — e.g. a write through one path bumps a
     * phase status but doesn't re-run `derive_stage`, so the cached
     * `stage` field references a phase that's no longer failed. The
     * frontend's failure-card auto-sync depends on `sdd_get` being
     * the source of truth; without this rebuild the card kept
     * rendering "Phase 3 failed" after Rust had already flipped
     * phase 3 to `running` (toast surfaced this as "phase 3 no
     * longer failed" but the UI didn't follow because the returned
     * snapshot's stage was still `Failed { failed_phase: Some(3) }`).
     * `rebuild_from_disk` is cheap — it walks the per-phase
     * frontmatter, no IPC. */
    {
        let mut w = cell.write();
        crate::sdd_hydrate::rebuild_from_disk(&mut w)?;
    }
    let snapshot = cell.read().clone();
    Ok(snapshot)
}

#[tauri::command]
pub async fn sdd_list(
    registry: State<'_, SddRegistry>,
) -> Result<Vec<SddWorkspace>, String> {
    let mut out: Vec<SddWorkspace> = registry
        .workspaces
        .read()
        .values()
        .map(|c| c.read().clone())
        .collect();
    out.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(out)
}

/// Re-read all files in the workspace dir + recompute stage. Called by
/// the orchestrator AFTER it sends a prompt that should have caused the
/// agent to write files (or AFTER `notify::watch` fires — that's v2).
#[tauri::command]
pub async fn sdd_refresh(
    app: AppHandle,
    registry: State<'_, SddRegistry>,
    id: String,
) -> Result<SddWorkspace, String> {
    let cell = registry
        .workspaces
        .read()
        .get(&id)
        .cloned()
        .ok_or_else(|| format!("unknown workspace {id}"))?;
    let snapshot = {
        let mut w = cell.write();
        rebuild_from_disk(&mut w)?;
        w.clone()
    };
    emit_changed(&app, &id);
    Ok(snapshot)
}

// User-driven gate + edit + retry + discard commands moved
// to ./sdd_user_commands.rs (wave-26 split). lib.rs invoke_handler
// registers them via the new module path.


// Lifecycle commands (pause / resume / stop) + control-file helpers
// moved to ./sdd_lifecycle_commands.rs (wave-18 split). Re-exported so
// existing call-sites compile unchanged.
#[allow(unused_imports)]
pub(crate) use crate::sdd_lifecycle_commands::{
    flip_stage, set_control_file, unset_control_files,
};

/* `sdd_prompt` command moved to ./sdd_prompts.rs */


// ---------------------------------------------------------------------------
// V2 plan-as-data commands — per-phase approval gate + plan editor.
// Legacy v1 workspaces are unaffected (these commands either become no-ops
// or refuse to run when `is_v2 = false`, depending on the call).
// ---------------------------------------------------------------------------

// Three-call mode + phase-execution-config commands moved to
// ./sdd_phase_commands.rs (wave-24 split). lib.rs invoke_handler
// registers them via the new module path.

// Skip / accept-failed commands moved to ./sdd_skip_commands.rs
// (wave-23 split). lib.rs invoke_handler registers them via the new
// module path.

/// Insert a new phase after the given number. Shifts every existing
/// phase with `number > after_number` up by one (file renamed, frontmatter
/// `phase:` rewritten). Updates `plan.json` to match. Atomic from the
/// user's perspective: a partial failure half-way through leaves the
/// workspace in a broken state — the user should `sdd_discard` and
/// start over. (Acceptable for now since the file surface is small;
/// proper transactional rename would need a stage dir + commit phase.)
// Phase-structure commands (sdd_insert_phase / sdd_reorder_phases /
// sdd_delete_phase / sdd_upgrade_workspace) moved to
// ./sdd_structure_commands.rs (wave-22 split). lib.rs invoke_handler
// registers them via the new module path.

// ---------------------------------------------------------------------------
// V2 verifier — Tauri-command surface around `sdd_verify` module.
// `sdd_run_verification` runs the acceptance checks for a phase and
// flips the phase frontmatter to `done` / `failed` based on the verdict.
// `sdd_mark_manual_check` lets the user resolve a manual check
// post-verification.
// ---------------------------------------------------------------------------

// Verification + rollback + recovery commands moved to
// ./sdd_verify_recover_commands.rs (wave-25 split). lib.rs invoke_handler
// registers them via the new module path.


/// Compact git-row state for the SddCard header. Returns
/// `enabled: false` for non-git workspaces — UI should hide the row
/// entirely rather than show "no branch".
// Git-state + per-phase diff Tauri commands moved to
// ./sdd_git_commands.rs (wave-20 split). lib.rs invoke_handler
// registers them via the new module path.

// ---------------------------------------------------------------------------
// Live action log — append-only JSONL of every tool_use / tool_result the
// agent emits during a phase, persisted at
// `<workspace>/phases/phase-<N>.log.jsonl` so the SddCard's live-feed
// survives an app restart. Schema mirrors `interface ActionLogEntry` in
// `apps/desktop/src/lib/state/sdd.svelte.ts`. Best-effort: failures are
// logged to stderr and never bubble — losing log lines should never break
// phase execution.
// ---------------------------------------------------------------------------

// ActionLogKind + ActionLogEntry moved to ./sdd_action_log.rs
// (wave-1 phase-10 split). The JSONL-append / tail Tauri commands
// stay in this file because they need the workspace registry.
#[allow(unused_imports)]
pub use crate::sdd_action_log::{ActionLogEntry, ActionLogKind};

// action_log_path + append_substep_started_event moved to
// ./sdd_action_log.rs (wave-7 follow-up).
#[allow(unused_imports)]
pub(crate) use crate::sdd_action_log::{action_log_path, append_substep_started_event};

/// Append a single ActionLogEntry to the phase's JSONL file. Creates the
/// file (and parent dir) on first write. Best-effort: an IO failure
/// logs to stderr and returns Ok(()) — the live feed lives in memory
/// anyway; persistence is for crash-recovery continuity, not the
/// happy path.
// Action-log Tauri commands moved to ./sdd_action_log_commands.rs
// (wave-19 split). lib.rs invoke_handler registers them via the new
// module path.

// ---------------------------------------------------------------------------
// V2 plan-as-data helpers (file-shuffling, plan.json regen).
// ---------------------------------------------------------------------------

// Plan-helpers (rename_phase_file, update_phase_number_in_frontmatter,
// set_status_with_extras, rebuild_plan_json, slugify) moved to
// ./sdd_plan_helpers.rs (wave-21 split). Re-exported so command bodies
// + the tests below can keep calling them unqualified.
#[allow(unused_imports)]
pub(crate) use crate::sdd_plan_helpers::{
    rebuild_plan_json, rename_phase_file, set_status_with_extras, slugify,
    update_phase_number_in_frontmatter,
};


// ensure_watcher + emit_changed moved to ./sdd_watcher.rs (wave-7 split).
use crate::sdd_watcher::{emit_changed, ensure_watcher};

// ---------------------------------------------------------------------------
// Audit log — append-only JSONL of every workspace mutation, regardless of
// whether the agent (via MCP tools), the user (via UI clicks), or the
// system (orchestrator-internal events) triggered it. Lives at
// `<workspace>/audit-log.jsonl` and is read by the SddCard's audit
// overlay. Schema is intentionally compact: ts + source + action + phase
// + reason + before/after JSON snapshots of whatever fields the action
// changed. Forward-compatible — readers must skip lines that don't
// deserialize, since we'll grow the schema over time.
// ---------------------------------------------------------------------------

// Audit log moved to ./sdd_audit.rs (wave-1 phase-10 split). Aliased
// as `audit::*` here so all `audit::AuditEntry::new(...)` / `audit::append`
// call sites stay unchanged.
pub(crate) use crate::sdd_audit as audit;

// ---------------------------------------------------------------------------
// MCP-handler helpers — shared validation between the woom-app sidecar's
// tool stubs and the Tauri-side audit-aware variants. Lives in its own
// module so the verification command `cargo test sdd::mcp_handlers` has
// a clear target. Pure functions, no IO.
// ---------------------------------------------------------------------------

// MCP-handler helpers moved to ./sdd_mcp_handlers.rs (wave-1 phase-10
// split). Aliased here so existing `mcp_handlers::*` call sites stay
// unchanged.
#[allow(unused_imports)]
pub(crate) use crate::sdd_mcp_handlers as mcp_handlers;

// ---------------------------------------------------------------------------
// Tauri commands — audit log read/append + a tiny convenience wrapper that
// performs reason-validation server-side. Frontend stream-parser calls
// `sdd_audit_append` after every successful mutation; the SddCard's
// audit overlay calls `sdd_audit_read`.
// ---------------------------------------------------------------------------

// Audit + validate Tauri commands moved to ./sdd_audit_commands.rs
// (wave-17 split). lib.rs invoke_handler registers them via the
// new module path.

// ---------------------------------------------------------------------------
// Unit tests — exercise the v2 plan-as-data + per-phase gate logic without
// going through the Tauri command layer. The pure functions (derive_stage,
// rename_phase_file, plan.json round-trip, slugify) are easy to test;
// integration tests via temp dirs cover the file-shuffling commands.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_phase(num: u32, status: &str) -> SddPhase {
        SddPhase {
            number: num,
            slug: format!("{:02}-phase-{num}", num),
            title: format!("Phase {num}"),
            depends_on: if num > 1 { vec![num - 1] } else { vec![] },
            status: status.into(),
            tasks_total: 1,
            tasks_done: 0,
            body: String::new(),
            path: format!("/tmp/sdd-test/phases/{:02}-phase-{num}.md", num),
            summary: None,
            plan_body: None,
            verify: None,
        }
    }

    fn sample_ws(root: &Path, phases: Vec<SddPhase>, is_v2: bool) -> SddWorkspace {
        SddWorkspace {
            id: "sdd-test".into(),
            session_id: None,
            root: root.to_string_lossy().into_owned(),
            stage: SddStage::Drafting,
            user_prompt: String::new(),
            spec_path: Some(format!("{}/spec.md", root.to_string_lossy())),
            spec_body: Some("spec".into()),
            plan_path: Some(format!("{}/plan.md", root.to_string_lossy())),
            plan_body: Some("plan".into()),
            summary_path: None,
            summary_body: None,
            phases,
            created_at: 0,
            updated_at: 0,
            is_v2,
            recovery_state: None,
            // Default test workspace runs in single-call mode so the
            // existing derive_stage_v2_* tests keep their semantics
            // (no sub-step fan-out). Tests that need three-call mode
            // construct a workspace with `phase_execution.mode =
            // ThreeCall` explicitly.
            phase_execution: PhaseExecutionConfig::default(),
        }
    }

    fn sample_ws_three_call(root: &Path, phases: Vec<SddPhase>) -> SddWorkspace {
        let mut ws = sample_ws(root, phases, /* is_v2 */ true);
        ws.phase_execution.mode = PhaseExecutionMode::ThreeCall;
        ws
    }

    #[test]
    fn slugify_basic() {
        assert_eq!(slugify("Foundation"), "foundation");
        assert_eq!(slugify("Plan as data + per-phase gates"), "plan-as-data-per-phase-gates");
        assert_eq!(slugify("   "), "phase");
        assert_eq!(slugify("a__b"), "a-b");
    }

    #[test]
    fn plan_json_round_trip() {
        let dir = tempdir();
        let plan = SddPlanFile {
            version: 1,
            phases: vec![SddPlanPhase {
                number: 1,
                slug: "01-foundation".into(),
                title: "Foundation".into(),
                depends_on: vec![],
                complexity: Some("medium".into()),
                acceptance: vec![SddPhaseAcceptance::Shell {
                    cmd: "cargo build".into(),
                    expect_exit: 0,
                    stdout_match: None,
                    timeout_ms: None,
                }],
            }],
        };
        write_plan_json(&dir, &plan).expect("write");
        let back = read_plan_json(&dir).expect("read");
        assert_eq!(back, plan);
    }

    #[test]
    fn read_plan_json_missing_returns_none() {
        let dir = tempdir();
        // No plan.json on disk — must NOT error, just return None.
        assert!(read_plan_json(&dir).is_none());
    }

    #[test]
    fn derive_stage_v1_legacy_after_plan_approved_returns_plan_ready() {
        let dir = tempdir();
        let phases = vec![sample_phase(1, "pending"), sample_phase(2, "pending")];
        let ws = sample_ws(&dir, phases, /* is_v2 */ false);
        let stage = derive_stage(&ws, Some("approved".into()), Some("approved".into()));
        // Legacy: no per-phase gate, so we land on PlanReady (which the
        // frontend treats as "auto-fire phase 1 prompt").
        assert_eq!(stage, SddStage::PlanReady);
    }

    #[test]
    fn derive_stage_v2_after_plan_approved_returns_phase_pending_approval() {
        let dir = tempdir();
        let phases = vec![sample_phase(1, "pending"), sample_phase(2, "pending")];
        let ws = sample_ws(&dir, phases, /* is_v2 */ true);
        let stage = derive_stage(&ws, Some("approved".into()), Some("approved".into()));
        assert_eq!(stage, SddStage::PhasePendingApproval { phase: 1 });
    }

    #[test]
    fn derive_stage_v2_after_phase_done_returns_pending_approval_for_next() {
        let dir = tempdir();
        let phases = vec![sample_phase(1, "done"), sample_phase(2, "pending")];
        let ws = sample_ws(&dir, phases, true);
        let stage = derive_stage(&ws, Some("approved".into()), Some("approved".into()));
        assert_eq!(stage, SddStage::PhasePendingApproval { phase: 2 });
    }

    #[test]
    fn derive_stage_v2_with_control_file_falls_through_gate() {
        let dir = tempdir();
        std::fs::create_dir_all(dir.join("control")).unwrap();
        std::fs::write(dir.join("control/phase-1-approved"), "").unwrap();
        let phases = vec![sample_phase(1, "pending"), sample_phase(2, "pending")];
        let ws = sample_ws(&dir, phases, true);
        let stage = derive_stage(&ws, Some("approved".into()), Some("approved".into()));
        // Gate cleared — derive_stage falls through to the legacy
        // PlanReady so the frontend's auto-fire flow can pick up phase 1.
        assert_eq!(stage, SddStage::PlanReady);
    }

    #[test]
    fn derive_stage_v2_skipped_phase_does_not_block_next_gate() {
        let dir = tempdir();
        let phases = vec![sample_phase(1, "skipped"), sample_phase(2, "pending")];
        let ws = sample_ws(&dir, phases, true);
        let stage = derive_stage(&ws, Some("approved".into()), Some("approved".into()));
        // Skipped is treated like done for gate advancement — next gate
        // should be on phase 2.
        assert_eq!(stage, SddStage::PhasePendingApproval { phase: 2 });
    }

    #[test]
    fn derive_stage_v2_running_phase_takes_precedence_over_gate() {
        let dir = tempdir();
        let phases = vec![sample_phase(1, "running"), sample_phase(2, "pending")];
        let ws = sample_ws(&dir, phases, true);
        let stage = derive_stage(&ws, Some("approved".into()), Some("approved".into()));
        // Running phases skip the v2 gate entirely.
        assert_eq!(stage, SddStage::PhaseRunning { phase: 1 });
    }

    // ── three-call mode (spec-1) ─────────────────────────────────────

    #[test]
    fn phase_execution_mode_serializes_snake_case() {
        let s = serde_json::to_string(&PhaseExecutionMode::ThreeCall).unwrap();
        assert_eq!(s, "\"three_call\"");
        let s = serde_json::to_string(&PhaseExecutionMode::SingleCall).unwrap();
        assert_eq!(s, "\"single_call\"");
    }

    #[test]
    fn phase_execution_config_default_is_legacy_safe() {
        let cfg = PhaseExecutionConfig::default();
        assert_eq!(cfg.mode, PhaseExecutionMode::SingleCall);
        assert!(!cfg.plan_gate);
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn phase_execution_config_rejects_overflow_budget() {
        let cfg = PhaseExecutionConfig {
            mode: PhaseExecutionMode::ThreeCall,
            plan_gate: false,
            plan_budget_pct: 0.5,
            implement_budget_pct: 0.7,
            verify_budget_pct: 0.1,
        };
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn meta_json_round_trips_with_phase_execution_block() {
        let meta = SddMeta {
            user_prompt: Some("ask".into()),
            session_id: None,
            created_at: Some(1),
            repo_cwd: None,
            git_enabled: false,
            sdd_branch: None,
            parent_sha: None,
            phase_execution: PhaseExecutionConfig {
                mode: PhaseExecutionMode::ThreeCall,
                plan_gate: true,
                ..PhaseExecutionConfig::default()
            },
        };
        let json = serde_json::to_string(&meta).unwrap();
        let back: SddMeta = serde_json::from_str(&json).unwrap();
        assert_eq!(back.phase_execution.mode, PhaseExecutionMode::ThreeCall);
        assert!(back.phase_execution.plan_gate);
    }

    #[test]
    fn meta_json_legacy_missing_block_deserializes_to_single_call() {
        let legacy_json = r#"{"user_prompt":"ask","session_id":null,"created_at":1}"#;
        let meta: SddMeta = serde_json::from_str(legacy_json).unwrap();
        assert_eq!(meta.phase_execution.mode, PhaseExecutionMode::SingleCall);
    }

    #[test]
    fn substep_state_round_trip() {
        let dir = tempdir();
        let state = SddPhaseSubstepState {
            phase: 2,
            sub_step: Some(SddPhaseSubstep::Implement),
            started_at: 12345,
        };
        write_substep_state(&dir, &state).unwrap();
        let back = read_substep_state(&dir, 2).unwrap();
        assert_eq!(back, state);
        // Atomic write should not leave the `.tmp` sidecar behind.
        let tmp = dir.join("control/phase-2-substep-state.json.tmp");
        assert!(!tmp.exists(), "tmp file leaked after rename");
    }

    #[test]
    fn substep_state_missing_returns_none() {
        let dir = tempdir();
        assert!(read_substep_state(&dir, 1).is_none());
    }

    #[test]
    fn substep_state_clear_is_idempotent() {
        let dir = tempdir();
        // Removing a non-existent checkpoint is OK.
        clear_substep_state(&dir, 1).unwrap();
        // Write then clear leaves no file behind.
        let state = SddPhaseSubstepState {
            phase: 1,
            sub_step: Some(SddPhaseSubstep::Plan),
            started_at: 0,
        };
        write_substep_state(&dir, &state).unwrap();
        clear_substep_state(&dir, 1).unwrap();
        assert!(read_substep_state(&dir, 1).is_none());
    }

    #[test]
    fn derive_stage_three_call_substep_plan_emits_phase_planning() {
        let dir = tempdir();
        write_substep_state(
            &dir,
            &SddPhaseSubstepState {
                phase: 1,
                sub_step: Some(SddPhaseSubstep::Plan),
                started_at: 1,
            },
        )
        .unwrap();
        let phases = vec![sample_phase(1, "running"), sample_phase(2, "pending")];
        let ws = sample_ws_three_call(&dir, phases);
        let stage = derive_stage(&ws, Some("approved".into()), Some("approved".into()));
        assert_eq!(stage, SddStage::PhasePlanning { phase: 1 });
    }

    #[test]
    fn derive_stage_three_call_substep_implement_emits_phase_implementing() {
        let dir = tempdir();
        write_substep_state(
            &dir,
            &SddPhaseSubstepState {
                phase: 1,
                sub_step: Some(SddPhaseSubstep::Implement),
                started_at: 1,
            },
        )
        .unwrap();
        let phases = vec![sample_phase(1, "running")];
        let ws = sample_ws_three_call(&dir, phases);
        let stage = derive_stage(&ws, Some("approved".into()), Some("approved".into()));
        assert_eq!(stage, SddStage::PhaseImplementing { phase: 1 });
    }

    #[test]
    fn derive_stage_three_call_substep_verify_emits_phase_verifying() {
        let dir = tempdir();
        write_substep_state(
            &dir,
            &SddPhaseSubstepState {
                phase: 1,
                sub_step: Some(SddPhaseSubstep::Verify),
                started_at: 1,
            },
        )
        .unwrap();
        let phases = vec![sample_phase(1, "running")];
        let ws = sample_ws_three_call(&dir, phases);
        let stage = derive_stage(&ws, Some("approved".into()), Some("approved".into()));
        assert_eq!(stage, SddStage::PhaseVerifying { phase: 1 });
    }

    #[test]
    fn derive_stage_three_call_no_substep_falls_through_to_running() {
        let dir = tempdir();
        // No checkpoint on disk.
        let phases = vec![sample_phase(1, "running")];
        let ws = sample_ws_three_call(&dir, phases);
        let stage = derive_stage(&ws, Some("approved".into()), Some("approved".into()));
        assert_eq!(stage, SddStage::PhaseRunning { phase: 1 });
    }

    #[test]
    fn derive_stage_single_call_ignores_substep_state() {
        // Even with a checkpoint on disk, single-call mode never
        // emits sub-step variants — keeps legacy workspaces stable.
        let dir = tempdir();
        write_substep_state(
            &dir,
            &SddPhaseSubstepState {
                phase: 1,
                sub_step: Some(SddPhaseSubstep::Implement),
                started_at: 1,
            },
        )
        .unwrap();
        let phases = vec![sample_phase(1, "running")];
        let ws = sample_ws(&dir, phases, /* is_v2 */ true);
        // Default mode = SingleCall via PhaseExecutionConfig::default().
        let stage = derive_stage(&ws, Some("approved".into()), Some("approved".into()));
        assert_eq!(stage, SddStage::PhaseRunning { phase: 1 });
    }

    // ── artifact persistence (phase 3) ───────────────────────────────

    #[test]
    fn verify_output_parses_clean_json() {
        let raw = r#"{
            "summary": "ok",
            "files_changed": ["a.rs"],
            "task_compliance": ["t1 done"],
            "deviations": [],
            "notes": ""
        }"#;
        let v = VerifyOutput::parse_or_fallback(raw);
        assert_eq!(v.summary, "ok");
        assert_eq!(v.files_changed, vec!["a.rs"]);
        assert!(v.deviations.is_empty());
    }

    #[test]
    fn verify_output_strips_markdown_fences() {
        let raw = "```json\n{\"summary\":\"yo\",\"deviations\":[]}\n```";
        let v = VerifyOutput::parse_or_fallback(raw);
        assert_eq!(v.summary, "yo");
    }

    #[test]
    fn verify_output_falls_back_on_garbage() {
        let v = VerifyOutput::parse_or_fallback("not json at all");
        assert_eq!(
            v.deviations,
            vec!["Unable to parse verification output".to_string()]
        );
        assert!(v.summary.is_empty());
    }

    #[test]
    fn plan_md_round_trip() {
        let dir = tempdir();
        write_phase_plan_md(&dir, "01-foo", "# plan\nbody").unwrap();
        let back = read_phase_plan_md(&dir, "01-foo").unwrap();
        assert_eq!(back, "# plan\nbody");
    }

    #[test]
    fn verify_json_round_trip() {
        let dir = tempdir();
        let v = VerifyOutput {
            summary: "s".into(),
            files_changed: vec!["x".into()],
            task_compliance: vec![],
            deviations: vec![],
            notes: "n".into(),
        };
        write_verify_json(&dir, "01-foo", &v).unwrap();
        let back = read_verify_json(&dir, "01-foo").unwrap();
        assert_eq!(back, v);
    }

    #[test]
    fn verify_json_missing_returns_none() {
        let dir = tempdir();
        assert!(read_verify_json(&dir, "01-foo").is_none());
    }

    #[test]
    fn set_status_and_summary_writes_both_fields() {
        let dir = tempdir();
        std::fs::create_dir_all(dir.join("phases")).unwrap();
        let path = dir.join("phases/01-foo.md");
        std::fs::write(
            &path,
            "---\nphase: 1\ntitle: Foo\nstatus: running\n---\n\nbody\n",
        )
        .unwrap();
        set_status_and_summary_on(&path, "done", Some("Did the thing.")).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("status: done"));
        assert!(content.contains("summary: Did the thing."));
        assert!(content.contains("body"));
    }

    #[test]
    fn set_status_and_summary_skips_empty_summary() {
        let dir = tempdir();
        std::fs::create_dir_all(dir.join("phases")).unwrap();
        let path = dir.join("phases/01-foo.md");
        std::fs::write(
            &path,
            "---\nphase: 1\ntitle: Foo\nstatus: running\n---\n\nbody\n",
        )
        .unwrap();
        // None and "" both leave the field untouched (FR-6: never
        // write garbage).
        set_status_and_summary_on(&path, "done", None).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("status: done"));
        assert!(!content.contains("summary:"));
    }

    #[test]
    fn derive_stage_failure_reads_verify_json_deviations() {
        let dir = tempdir();
        let phases_dir = dir.join("phases");
        std::fs::create_dir_all(phases_dir.join("01-foundation")).unwrap();
        // Phase markdown — status failed.
        std::fs::write(
            phases_dir.join("01-foundation.md"),
            "---\nphase: 1\ntitle: Foundation\nstatus: failed\n---\n\nbody\n",
        )
        .unwrap();
        // verify.json with one deviation.
        write_verify_json(
            &dir,
            "01-foundation",
            &VerifyOutput {
                summary: "tried".into(),
                deviations: vec!["forgot the tests".into()],
                ..Default::default()
            },
        )
        .unwrap();
        let phase = SddPhase {
            number: 1,
            slug: "01-foundation".into(),
            title: "Foundation".into(),
            depends_on: vec![],
            status: "failed".into(),
            tasks_total: 0,
            tasks_done: 0,
            body: String::new(),
            path: phases_dir
                .join("01-foundation.md")
                .to_string_lossy()
                .into_owned(),
            summary: None,
            plan_body: None,
            verify: None,
        };
        let ws = sample_ws(&dir, vec![phase], /* is_v2 */ true);
        let stage = derive_stage(&ws, Some("approved".into()), Some("approved".into()));
        match stage {
            SddStage::Failed {
                trigger,
                failed_phase,
                reason,
                ..
            } => {
                assert_eq!(trigger, Some(FailureTrigger::VerifyFailed));
                assert_eq!(failed_phase, Some(1));
                assert!(reason.contains("forgot the tests"));
            }
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[test]
    fn derive_stage_failure_check_failed_wins_over_verify_failed() {
        // When BOTH hard-gate acceptance failures AND verify
        // deviations exist, the hard-gate trigger wins (more
        // specific). See `plan-1` §Failure-handling model.
        let dir = tempdir();
        let phases_dir = dir.join("phases");
        std::fs::create_dir_all(phases_dir.join("01-foundation")).unwrap();
        std::fs::write(
            phases_dir.join("01-foundation.md"),
            "---\nphase: 1\ntitle: Foundation\nstatus: failed\n---\n\nbody\n",
        )
        .unwrap();
        std::fs::create_dir_all(dir.join("results")).unwrap();
        // Synthesise an acceptance.json with a failed check.
        let accept = crate::sdd_verify::PhaseAcceptanceFile {
            phase: 1,
            overall_status: crate::sdd_verify::OverallStatus::Failed,
            started_at: 0,
            finished_at: 0,
            results: vec![crate::sdd_verify::AcceptanceResult {
                check_index: 0,
                kind: "shell".into(),
                status: crate::sdd_verify::CheckStatus::Failed,
                started_at: 0,
                finished_at: 0,
                exit_code: Some(1),
                log_tail: String::new(),
                note: String::new(),
            }],
        };
        std::fs::write(
            dir.join("results/phase-1-acceptance.json"),
            serde_json::to_string(&accept).unwrap(),
        )
        .unwrap();
        write_verify_json(
            &dir,
            "01-foundation",
            &VerifyOutput {
                deviations: vec!["soft verdict".into()],
                ..Default::default()
            },
        )
        .unwrap();
        let phase = SddPhase {
            number: 1,
            slug: "01-foundation".into(),
            title: "Foundation".into(),
            depends_on: vec![],
            status: "failed".into(),
            tasks_total: 0,
            tasks_done: 0,
            body: String::new(),
            path: phases_dir
                .join("01-foundation.md")
                .to_string_lossy()
                .into_owned(),
            summary: None,
            plan_body: None,
            verify: None,
        };
        let ws = sample_ws(&dir, vec![phase], /* is_v2 */ true);
        let stage = derive_stage(&ws, Some("approved".into()), Some("approved".into()));
        match stage {
            SddStage::Failed { trigger, .. } => {
                assert_eq!(trigger, Some(FailureTrigger::CheckFailed));
            }
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[test]
    fn derive_stage_three_call_plan_review_gate_emits_when_plan_md_exists() {
        let dir = tempdir();
        // Plan checkpoint + plan.md artifact on disk + plan_gate=true
        // + no approval marker → plan review.
        write_substep_state(
            &dir,
            &SddPhaseSubstepState {
                phase: 1,
                sub_step: Some(SddPhaseSubstep::Plan),
                started_at: 1,
            },
        )
        .unwrap();
        let slug_dir = dir.join("phases").join("01-foundation");
        std::fs::create_dir_all(&slug_dir).unwrap();
        std::fs::write(slug_dir.join("plan.md"), "plan body").unwrap();
        let phase = SddPhase {
            number: 1,
            slug: "01-foundation".into(),
            title: "Foundation".into(),
            depends_on: vec![],
            status: "running".into(),
            tasks_total: 0,
            tasks_done: 0,
            body: String::new(),
            path: format!("{}/phases/01-foundation.md", dir.to_string_lossy()),
            summary: None,
            plan_body: None,
            verify: None,
        };
        let mut ws = sample_ws_three_call(&dir, vec![phase]);
        ws.phase_execution.plan_gate = true;
        let stage = derive_stage(&ws, Some("approved".into()), Some("approved".into()));
        assert_eq!(stage, SddStage::PhasePlanReview { phase: 1 });
    }

    #[test]
    fn rename_phase_file_updates_filename_and_frontmatter() {
        let dir = tempdir();
        let phases_dir = dir.join("phases");
        std::fs::create_dir_all(&phases_dir).unwrap();
        let original = phases_dir.join("01-foo.md");
        std::fs::write(
            &original,
            "---\nphase: 1\ntitle: Foo\nstatus: pending\n---\n\nbody\n",
        )
        .unwrap();
        let new_slug = rename_phase_file(&phases_dir, "01-foo", 3).unwrap();
        assert_eq!(new_slug, "03-foo");
        assert!(!original.exists());
        let renamed = phases_dir.join("03-foo.md");
        assert!(renamed.exists());
        let content = std::fs::read_to_string(&renamed).unwrap();
        assert!(content.contains("phase: 3"), "frontmatter not updated: {content}");
        assert!(content.contains("body"));
    }

    #[test]
    fn rebuild_plan_json_collects_phases_in_order() {
        let dir = tempdir();
        let phases_dir = dir.join("phases");
        std::fs::create_dir_all(&phases_dir).unwrap();
        for n in [2u32, 1, 3] {
            std::fs::write(
                phases_dir.join(format!("{:02}-task-{n}.md", n)),
                format!(
                    "---\nphase: {n}\ntitle: Task {n}\nstatus: pending\n---\n\nbody\n"
                ),
            )
            .unwrap();
        }
        rebuild_plan_json(&dir).unwrap();
        let plan = read_plan_json(&dir).unwrap();
        assert_eq!(plan.version, 1);
        let nums: Vec<u32> = plan.phases.iter().map(|p| p.number).collect();
        assert_eq!(nums, vec![1, 2, 3]);
        assert_eq!(plan.phases[0].title, "Task 1");
        assert!(plan.phases[0].acceptance.is_empty());
    }

    #[test]
    fn rebuild_plan_json_preserves_acceptance_across_regen() {
        let dir = tempdir();
        let phases_dir = dir.join("phases");
        std::fs::create_dir_all(&phases_dir).unwrap();
        std::fs::write(
            phases_dir.join("01-a.md"),
            "---\nphase: 1\ntitle: A\nstatus: pending\n---\n\nbody\n",
        )
        .unwrap();
        // Seed an existing plan.json with acceptance for phase 1.
        let seed = SddPlanFile {
            version: 1,
            phases: vec![SddPlanPhase {
                number: 1,
                slug: "01-a".into(),
                title: "A".into(),
                depends_on: vec![],
                complexity: None,
                acceptance: vec![SddPhaseAcceptance::Manual {
                    description: "verify".into(),
                }],
            }],
        };
        write_plan_json(&dir, &seed).unwrap();
        rebuild_plan_json(&dir).unwrap();
        let plan = read_plan_json(&dir).unwrap();
        assert_eq!(plan.phases.len(), 1);
        assert_eq!(plan.phases[0].acceptance.len(), 1);
    }

    #[test]
    fn set_status_with_extras_writes_skip_reason() {
        let dir = tempdir();
        let path = dir.join("phase.md");
        std::fs::write(
            &path,
            "---\nphase: 1\ntitle: A\nstatus: pending\ntasks_total: 0\n---\n\nbody\n",
        )
        .unwrap();
        set_status_with_extras(
            &path,
            "skipped",
            vec![("skip_reason".into(), "user said so".into())],
        )
        .unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("status: skipped"));
        assert!(content.contains("skip_reason"));
        assert!(content.contains("user said so"));
        // Original fields preserved
        assert!(content.contains("title: A"));
        assert!(content.contains("body"));
    }

    // ---- action log -------------------------------------------------------

    #[test]
    fn action_log_path_targets_phases_subdir() {
        let dir = tempdir();
        let p = action_log_path(&dir, 7);
        assert_eq!(p.file_name().unwrap(), "phase-7.log.jsonl");
        assert!(p.starts_with(&dir.join("phases")));
    }

    #[test]
    fn action_log_jsonl_roundtrip() {
        // Direct file roundtrip — we don't go through the Tauri command
        // layer (would need a fake AppHandle) but exercise the same
        // serde shape + line-per-entry contract the commands write.
        let dir = tempdir();
        let phases_dir = dir.join("phases");
        std::fs::create_dir_all(&phases_dir).unwrap();
        let path = action_log_path(&dir, 1);
        let entries = vec![
            ActionLogEntry {
                ts: 1,
                phase: 1,
                kind: ActionLogKind::ToolUse,
                tool: Some("Read".into()),
                summary: "Read foo.rs".into(),
                detail: None,
                status: Some("running".into()),
                correlation_id: Some("abc".into()),
                sub_step: None,
            },
            ActionLogEntry {
                ts: 2,
                phase: 1,
                kind: ActionLogKind::ToolResult,
                tool: Some("Read".into()),
                summary: "Read foo.rs ✓".into(),
                detail: None,
                status: Some("done".into()),
                correlation_id: Some("abc".into()),
                sub_step: None,
            },
        ];
        let mut body = String::new();
        for e in &entries {
            body.push_str(&serde_json::to_string(e).unwrap());
            body.push('\n');
        }
        std::fs::write(&path, body).unwrap();

        let raw = std::fs::read_to_string(&path).unwrap();
        let parsed: Vec<ActionLogEntry> = raw
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].correlation_id.as_deref(), Some("abc"));
        assert!(matches!(parsed[1].kind, ActionLogKind::ToolResult));
    }

    #[test]
    fn action_log_skips_corrupted_lines_when_parsing_tail() {
        // Simulate a half-flushed line (truncated mid-write) — current
        // production behaviour: skip the bad line, keep the good ones.
        let dir = tempdir();
        std::fs::create_dir_all(dir.join("phases")).unwrap();
        let path = action_log_path(&dir, 2);
        let good = ActionLogEntry {
            ts: 1,
            phase: 2,
            kind: ActionLogKind::ToolUse,
            tool: Some("Bash".into()),
            summary: "Bash cargo build".into(),
            detail: None,
            status: Some("running".into()),
            correlation_id: None,
            sub_step: None,
        };
        let body = format!(
            "{}\n{}\n",
            serde_json::to_string(&good).unwrap(),
            "{not valid json…",
        );
        std::fs::write(&path, body).unwrap();
        let parsed: Vec<ActionLogEntry> = std::fs::read_to_string(&path)
            .unwrap()
            .lines()
            .filter(|l| !l.trim().is_empty())
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].summary, "Bash cargo build");
    }

    /* `derive_stage` failed_stage tests below.
     *  We exercise the path that hydrates `failed_checks` from
     *  `phase-N-acceptance.json` and the `action_log_tail` from the
     *  per-phase JSONL, so the failure card has structured data
     *  without an extra IPC round-trip. */
    #[test]
    fn failed_stage_hydrates_failed_checks_from_acceptance_file() {
        let dir = tempdir();
        std::fs::create_dir_all(dir.join("results")).unwrap();
        // Compose a PhaseAcceptanceFile with two checks, second failed.
        let acc = crate::sdd_verify::PhaseAcceptanceFile {
            phase: 1,
            overall_status: crate::sdd_verify::OverallStatus::Failed,
            started_at: 0,
            finished_at: 0,
            results: vec![
                crate::sdd_verify::AcceptanceResult {
                    check_index: 0,
                    kind: "shell".into(),
                    status: crate::sdd_verify::CheckStatus::Passed,
                    started_at: 0,
                    finished_at: 0,
                    exit_code: Some(0),
                    log_tail: String::new(),
                    note: String::new(),
                },
                crate::sdd_verify::AcceptanceResult {
                    check_index: 1,
                    kind: "shell".into(),
                    status: crate::sdd_verify::CheckStatus::Failed,
                    started_at: 0,
                    finished_at: 0,
                    exit_code: Some(1),
                    log_tail: "boom\n".into(),
                    note: String::new(),
                },
            ],
        };
        let path = dir.join("results/phase-1-acceptance.json");
        std::fs::write(path, serde_json::to_string_pretty(&acc).unwrap()).unwrap();
        let got = read_failed_check_indices(&dir, 1);
        assert_eq!(got, vec![1]);
    }

    #[test]
    fn failed_stage_action_log_tail_returns_last_n() {
        let dir = tempdir();
        std::fs::create_dir_all(dir.join("phases")).unwrap();
        let log_path = dir.join("phases/phase-1.log.jsonl");
        let mut body = String::new();
        for ts in 1..=12u64 {
            let e = ActionLogEntry {
                ts,
                phase: 1,
                kind: ActionLogKind::ToolUse,
                tool: Some("Read".into()),
                summary: format!("step {ts}"),
                detail: None,
                status: Some("done".into()),
                correlation_id: None,
                sub_step: None,
            };
            body.push_str(&serde_json::to_string(&e).unwrap());
            body.push('\n');
        }
        std::fs::write(log_path, body).unwrap();
        let tail = read_action_log_tail(&dir, 1, 5);
        assert_eq!(tail.len(), 5);
        // Should be the last 5 — ts 8..=12.
        assert_eq!(tail.first().unwrap().ts, 8);
        assert_eq!(tail.last().unwrap().ts, 12);
    }

    #[test]
    fn failed_stage_action_log_tail_handles_missing_file() {
        let dir = tempdir();
        let tail = read_action_log_tail(&dir, 99, 10);
        assert!(tail.is_empty());
    }

    /// Per-test scratch dir under the OS tempdir + a random suffix so
    /// concurrent `cargo test` workers don't collide. Cleaned up on
    /// process exit (good enough for unit tests; we don't leak between
    /// runs because the path is under $TMPDIR).
    fn tempdir() -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let dir = std::env::temp_dir().join(format!("woom-sdd-test-{pid}-{n}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("mkdir tempdir");
        dir
    }
}
