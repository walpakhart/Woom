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
use tauri::{AppHandle, Emitter, Manager, State};

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
        // Skip if already in memory.
        if registry.workspaces.read().contains_key(&id) {
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
pub use crate::sdd_substep::{SddPhaseSubstep, SddPhaseSubstepState};

// substep-state R/W helpers moved to ./sdd_substep.rs (wave-2 follow-up).
#[allow(unused_imports)]
pub(crate) use crate::sdd_substep::{clear_substep_state, read_substep_state, write_substep_state};

// phase-meta R/W helpers moved to ./sdd_meta.rs (wave-2 follow-up).
pub(crate) use crate::sdd_meta::{read_phase_meta, write_phase_meta};

// read_failed_check_indices + read_action_log_tail moved to
// ./sdd_hydrate.rs (wave-6 split).
#[allow(unused_imports)]
pub(crate) use crate::sdd_hydrate::{read_action_log_tail, read_failed_check_indices};

// workspace_meta R/W moved to ./sdd_meta.rs (wave-6 follow-up).
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

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SddApproveTarget {
    Spec,
    Plan,
}

#[tauri::command]
pub async fn sdd_approve(
    app: AppHandle,
    registry: State<'_, SddRegistry>,
    id: String,
    target: SddApproveTarget,
) -> Result<SddWorkspace, String> {
    let cell = registry
        .workspaces
        .read()
        .get(&id)
        .cloned()
        .ok_or_else(|| format!("unknown workspace {id}"))?;
    // Read paths under the lock, then drop the guard before disk I/O.
    let path: PathBuf = {
        let w = cell.read();
        match target {
            SddApproveTarget::Spec => w
                .spec_path
                .clone()
                .map(PathBuf::from)
                .ok_or_else(|| "no spec yet".to_string())?,
            SddApproveTarget::Plan => w
                .plan_path
                .clone()
                .map(PathBuf::from)
                .ok_or_else(|| "no plan yet".to_string())?,
        }
    };
    set_status_on(&path, "approved")?;
    {
        let root = PathBuf::from(cell.read().root.clone());
        let action = match target {
            SddApproveTarget::Spec => "approve_spec",
            SddApproveTarget::Plan => "approve_plan",
        };
        audit::append(
            &root,
            &audit::AuditEntry::new("user", action)
                .with_after(serde_json::json!({"status": "approved"})),
        );
    }
    let snapshot = {
        let mut w = cell.write();
        rebuild_from_disk(&mut w)?;
        w.clone()
    };
    emit_changed(&app, &id);
    Ok(snapshot)
}

// Lifecycle commands (pause / resume / stop) + control-file helpers
// moved to ./sdd_lifecycle_commands.rs (wave-18 split). Re-exported so
// existing call-sites compile unchanged.
#[allow(unused_imports)]
pub(crate) use crate::sdd_lifecycle_commands::{
    flip_stage, set_control_file, unset_control_files,
};

/* `sdd_prompt` command moved to ./sdd_prompts.rs */

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SddEditTarget {
    Spec,
    Plan,
    Phase { number: u32 },
}

/// Save user-edited body for spec / plan / a specific phase. The YAML
/// frontmatter is preserved verbatim — we only swap the markdown
/// content. Used by the card's inline "edit" affordance so the user
/// can tweak the agent's spec/plan before approving without round-
/// tripping through a text editor.
#[tauri::command]
pub async fn sdd_save_body(
    app: AppHandle,
    registry: State<'_, SddRegistry>,
    id: String,
    target: SddEditTarget,
    body: String,
) -> Result<SddWorkspace, String> {
    let cell = registry
        .workspaces
        .read()
        .get(&id)
        .cloned()
        .ok_or_else(|| format!("unknown workspace {id}"))?;
    let path: PathBuf = {
        let w = cell.read();
        match target {
            SddEditTarget::Spec => w
                .spec_path
                .clone()
                .map(PathBuf::from)
                .ok_or_else(|| "no spec yet".to_string())?,
            SddEditTarget::Plan => w
                .plan_path
                .clone()
                .map(PathBuf::from)
                .ok_or_else(|| "no plan yet".to_string())?,
            SddEditTarget::Phase { number } => w
                .phases
                .iter()
                .find(|p| p.number == number)
                .map(|p| PathBuf::from(&p.path))
                .ok_or_else(|| format!("phase {number} not found"))?,
        }
    };
    replace_body_on(&path, &body)?;
    let snapshot = {
        let mut w = cell.write();
        rebuild_from_disk(&mut w)?;
        w.clone()
    };
    emit_changed(&app, &id);
    Ok(snapshot)
}

/// Reset a failed (or done) phase back to `pending` so the next
/// advance re-issues it. Used by the card's "Retry" button on a
/// failed phase. The user is the gate — we don't auto-retry inside
/// the agent prompt (that's the per-prompt `{{retries_max}}` budget);
/// THIS reset is for the case where verification failed AFTER the
/// in-prompt retries were exhausted.
#[tauri::command]
pub async fn sdd_retry_phase(
    app: AppHandle,
    registry: State<'_, SddRegistry>,
    id: String,
    phase: u32,
) -> Result<SddWorkspace, String> {
    let cell = registry
        .workspaces
        .read()
        .get(&id)
        .cloned()
        .ok_or_else(|| format!("unknown workspace {id}"))?;
    let (path, root): (PathBuf, PathBuf) = {
        let w = cell.read();
        let p = w
            .phases
            .iter()
            .find(|p| p.number == phase)
            .ok_or_else(|| format!("phase {phase} not found"))?;
        (PathBuf::from(&p.path), PathBuf::from(&w.root))
    };
    reset_phase_status(&path)?;
    /* Bump `retry_count` so the UI can warn at the 3rd attempt. We
     *  do this even if reset_phase_status was a no-op (re-clicking
     *  retry on a phase that's already pending is rare but harmless
     *  to count). */
    let mut pm = read_phase_meta(&root, phase);
    let prev_retries = pm.retry_count;
    pm.retry_count = pm.retry_count.saturating_add(1);
    let _ = write_phase_meta(&root, phase, &pm);
    audit::append(
        &root,
        &audit::AuditEntry::new("user", "retry_phase")
            .with_phase(phase)
            .with_before(serde_json::json!({"retry_count": prev_retries}))
            .with_after(serde_json::json!({"retry_count": pm.retry_count})),
    );
    let snapshot = {
        let mut w = cell.write();
        rebuild_from_disk(&mut w)?;
        w.clone()
    };
    emit_changed(&app, &id);
    Ok(snapshot)
}

// ---------------------------------------------------------------------------
// V2 plan-as-data commands — per-phase approval gate + plan editor.
// Legacy v1 workspaces are unaffected (these commands either become no-ops
// or refuse to run when `is_v2 = false`, depending on the call).
// ---------------------------------------------------------------------------

/// Open the gate for phase `phase` on a v2 workspace by writing
/// `<workspace>/control/phase-<phase>-approved`. `derive_stage` reads
/// this file and falls through from `PhasePendingApproval` to the
/// usual `PlanReady` / `PhaseDone` path, which the frontend uses to
/// fire the phase prompt. Idempotent — a second call on an already-
/// approved phase is fine. No-op for legacy workspaces (no plan.json):
/// the stage was never `PhasePendingApproval` to begin with.
#[tauri::command]
pub async fn sdd_approve_phase(
    app: AppHandle,
    registry: State<'_, SddRegistry>,
    id: String,
    phase: u32,
) -> Result<SddWorkspace, String> {
    let cell = registry
        .workspaces
        .read()
        .get(&id)
        .cloned()
        .ok_or_else(|| format!("unknown workspace {id}"))?;
    let root = PathBuf::from(cell.read().root.clone());
    /* Pre-phase git snapshot. Captured BEFORE writing the gate file so
     *  if the snapshot path errors out, the user hasn't moved the
     *  workflow forward. Failures are logged but non-fatal — the phase
     *  still gets approved (degraded: rollback unavailable). */
    let meta = read_workspace_meta(&root);
    let mut phase_meta = read_phase_meta(&root, phase);
    if meta.git_enabled {
        if let Some(repo) = meta.repo_cwd.as_deref() {
            // Stash any non-SDD work so the pre-phase commit doesn't
            // sweep up unrelated edits. Safety-stash carries a per-
            // workspace label so the user can find it back via
            // `git stash list`.
            let stash_label = format!("sdd-pre-phase-{}-{}", phase, id);
            let _ = crate::git::stash_with_label(repo, &stash_label);
            let msg = format!("[sdd:pre-phase-{}] {}", phase, id);
            match crate::git::commit_all_allow_empty(repo, &msg) {
                Ok(sha) => {
                    phase_meta.pre_phase_sha = Some(sha);
                    phase_meta.snapshot_skipped = false;
                    let _ = write_phase_meta(&root, phase, &phase_meta);
                }
                Err(e) => {
                    eprintln!(
                        "[sdd] workspace={id} phase={phase}: pre-phase snapshot failed: {e}"
                    );
                    phase_meta.snapshot_skipped = true;
                    let _ = write_phase_meta(&root, phase, &phase_meta);
                }
            }
        }
    } else {
        phase_meta.snapshot_skipped = true;
        let _ = write_phase_meta(&root, phase, &phase_meta);
    }
    set_control_file(&root, &format!("phase-{phase}-approved"))?;
    audit::append(
        &root,
        &audit::AuditEntry::new("user", "advance_phase")
            .with_phase(phase)
            .with_after(serde_json::json!({"approved": true})),
    );
    // Three-call mode bootstrap — flip the phase to `running` and
    // seed substep-state with `Plan` so derive_stage emits
    // `PhasePlanning` on the next refresh. The plan-pass prompt fires
    // through the standard `buildPromptForStage` path. Single-call
    // mode does NOT touch the substep file; phase status stays
    // `pending` until the agent flips it via `sdd_log_phase_done`.
    let three_call = matches!(
        cell.read().phase_execution.mode,
        PhaseExecutionMode::ThreeCall
    );
    if three_call {
        let phase_path = cell
            .read()
            .phases
            .iter()
            .find(|p| p.number == phase)
            .map(|p| PathBuf::from(&p.path));
        if let Some(path) = phase_path {
            let _ = set_status_on(&path, "running");
        }
        let _ = write_substep_state(
            &root,
            &SddPhaseSubstepState {
                phase,
                sub_step: Some(SddPhaseSubstep::Plan),
                started_at: now_ms(),
            },
        );
        append_substep_started_event(&root, phase, SddPhaseSubstep::Plan);
        audit::append(
            &root,
            &audit::AuditEntry::new("system", "phase_plan_started")
                .with_phase(phase)
                .with_after(serde_json::json!({"sub_step": "plan"})),
        );
    }
    let snapshot = {
        let mut w = cell.write();
        rebuild_from_disk(&mut w)?;
        w.clone()
    };
    emit_changed(&app, &id);
    Ok(snapshot)
}

/// Three-call mode — persist the plan-pass output as
/// `phases/<slug>/plan.md`, advance `substep-state.json` to
/// `Implement` (or stay on `Plan` if `phase_execution.plan_gate` is
/// true), and emit `sdd:changed`. The body is the agent's verbatim
/// markdown — typically the final assistant message from the plan
/// pass. See `spec-1` FR-2.
#[tauri::command]
pub async fn sdd_save_phase_plan(
    app: AppHandle,
    registry: State<'_, SddRegistry>,
    id: String,
    phase: u32,
    body: String,
) -> Result<SddWorkspace, String> {
    let cell = registry
        .workspaces
        .read()
        .get(&id)
        .cloned()
        .ok_or_else(|| format!("unknown workspace {id}"))?;
    let (root, slug, plan_gate) = {
        let w = cell.read();
        let p = w
            .phases
            .iter()
            .find(|p| p.number == phase)
            .ok_or_else(|| format!("phase {phase} not found"))?;
        (
            PathBuf::from(&w.root),
            p.slug.clone(),
            w.phase_execution.plan_gate,
        )
    };
    write_phase_plan_md(&root, &slug, &body)?;
    // Advance the substep checkpoint. plan_gate=true → stay on `Plan`
    // (user needs to Approve via `sdd_approve_phase_plan`). Otherwise
    // jump straight to `Implement` so the next agent turn fires the
    // implement-pass prompt.
    let next = if plan_gate {
        SddPhaseSubstep::Plan
    } else {
        SddPhaseSubstep::Implement
    };
    write_substep_state(
        &root,
        &SddPhaseSubstepState {
            phase,
            sub_step: Some(next),
            started_at: now_ms(),
        },
    )?;
    // Only drop a substep divider when we actually advanced (not
    // when plan_gate parks us on Plan waiting for the user). The
    // plan-pass start event was already emitted by sdd_approve_phase.
    if !plan_gate {
        append_substep_started_event(&root, phase, SddPhaseSubstep::Implement);
        audit::append(
            &root,
            &audit::AuditEntry::new("agent", "phase_implement_started")
                .with_phase(phase)
                .with_after(serde_json::json!({"sub_step": "implement"})),
        );
        audit::append(
            &root,
            &audit::AuditEntry::new("agent", "phase_plan_completed")
                .with_phase(phase)
                .with_after(serde_json::json!({"sub_step": "plan"})),
        );
    } else {
        audit::append(
            &root,
            &audit::AuditEntry::new("agent", "phase_plan_completed")
                .with_phase(phase)
                .with_after(serde_json::json!({"sub_step": "plan", "gate": "review"})),
        );
    }
    let snapshot = {
        let mut w = cell.write();
        rebuild_from_disk(&mut w)?;
        w.clone()
    };
    emit_changed(&app, &id);
    Ok(snapshot)
}

/// Three-call mode — close out the implement pass: advance the
/// substep checkpoint from `Implement` → `Verify` so the orchestrator
/// fires the verify-pass prompt on the next agent turn. `summary` +
/// `files_changed` are persisted on the phase frontmatter (matching
/// `sdd_save_phase_verify`'s contract, so the verifier downstream
/// has the implement-pass-side narrative available). No status flip
/// happens here — that's the verify pass's job. See `spec-1` FR-3.
#[tauri::command]
pub async fn sdd_complete_phase_implement(
    app: AppHandle,
    registry: State<'_, SddRegistry>,
    id: String,
    phase: u32,
    summary: String,
    files_changed: Vec<String>,
) -> Result<SddWorkspace, String> {
    let cell = registry
        .workspaces
        .read()
        .get(&id)
        .cloned()
        .ok_or_else(|| format!("unknown workspace {id}"))?;
    let (root, phase_path) = {
        let w = cell.read();
        let p = w
            .phases
            .iter()
            .find(|p| p.number == phase)
            .ok_or_else(|| format!("phase {phase} not found"))?;
        (PathBuf::from(&w.root), PathBuf::from(&p.path))
    };
    let summary_opt: Option<&str> = if summary.trim().is_empty() {
        None
    } else {
        Some(summary.as_str())
    };
    // Carry the implement-pass summary onto the phase frontmatter so
    // the verify pass + result.md generator can quote it. Status stays
    // `running` — verify pass is the one that flips done/failed.
    if summary_opt.is_some() {
        set_status_and_summary_on(&phase_path, "running", summary_opt)?;
    }
    write_substep_state(
        &root,
        &SddPhaseSubstepState {
            phase,
            sub_step: Some(SddPhaseSubstep::Verify),
            started_at: now_ms(),
        },
    )?;
    append_substep_started_event(&root, phase, SddPhaseSubstep::Verify);
    audit::append(
        &root,
        &audit::AuditEntry::new("agent", "phase_implement_completed")
            .with_phase(phase)
            .with_after(serde_json::json!({
                "sub_step": "implement",
                "files_changed_count": files_changed.len(),
            })),
    );
    audit::append(
        &root,
        &audit::AuditEntry::new("agent", "phase_verify_started")
            .with_phase(phase)
            .with_after(serde_json::json!({"sub_step": "verify"})),
    );
    let snapshot = {
        let mut w = cell.write();
        rebuild_from_disk(&mut w)?;
        w.clone()
    };
    emit_changed(&app, &id);
    Ok(snapshot)
}

/// Three-call mode — persist the verify-pass JSON, auto-fill phase
/// frontmatter `summary`, flip phase status to `done` (no deviations)
/// or `failed { trigger: verify_failed }` (deviations present), clear
/// the substep checkpoint, emit `sdd:changed`. See `spec-1` FR-4
/// through FR-6.
#[tauri::command]
pub async fn sdd_save_phase_verify(
    app: AppHandle,
    registry: State<'_, SddRegistry>,
    id: String,
    phase: u32,
    raw_json: String,
) -> Result<SddWorkspace, String> {
    let cell = registry
        .workspaces
        .read()
        .get(&id)
        .cloned()
        .ok_or_else(|| format!("unknown workspace {id}"))?;
    let (root, slug, phase_path) = {
        let w = cell.read();
        let p = w
            .phases
            .iter()
            .find(|p| p.number == phase)
            .ok_or_else(|| format!("phase {phase} not found"))?;
        (
            PathBuf::from(&w.root),
            p.slug.clone(),
            PathBuf::from(&p.path),
        )
    };
    let verdict = VerifyOutput::parse_or_fallback(&raw_json);
    write_verify_json(&root, &slug, &verdict)?;
    let new_status = if verdict.deviations.is_empty() {
        "done"
    } else {
        "failed"
    };
    let summary_opt: Option<&str> = if verdict.summary.trim().is_empty() {
        None
    } else {
        Some(verdict.summary.as_str())
    };
    set_status_and_summary_on(&phase_path, new_status, summary_opt)?;
    // Phase complete — drop the checkpoint so a future hydrate
    // doesn't think the phase is still running.
    let _ = clear_substep_state(&root, phase);
    // Audit the verify-pass lifecycle. We emit both started+completed
    // here because the verify call kicks off from the same agent turn
    // that closes it — no intermediate orchestrator hook to insert
    // the "started" row earlier. See `spec-1` audit requirements.
    append_substep_started_event(&root, phase, SddPhaseSubstep::Verify);
    audit::append(
        &root,
        &audit::AuditEntry::new("agent", "phase_implement_completed")
            .with_phase(phase)
            .with_after(serde_json::json!({"sub_step": "implement"})),
    );
    audit::append(
        &root,
        &audit::AuditEntry::new("agent", "phase_verify_started")
            .with_phase(phase)
            .with_after(serde_json::json!({"sub_step": "verify"})),
    );
    audit::append(
        &root,
        &audit::AuditEntry::new("agent", "phase_verify_completed")
            .with_phase(phase)
            .with_after(serde_json::json!({
                "status": new_status,
                "deviations_count": verdict.deviations.len(),
                "files_changed_count": verdict.files_changed.len(),
            })),
    );
    let snapshot = {
        let mut w = cell.write();
        rebuild_from_disk(&mut w)?;
        w.clone()
    };
    emit_changed(&app, &id);
    Ok(snapshot)
}

/// Three-call mode — clear the plan-review gate by writing
/// `control/phase-<N>-plan-approved`, advance substep-state to
/// `Implement`, emit `sdd:changed`. See `spec-1` FR-7.
#[tauri::command]
pub async fn sdd_approve_phase_plan(
    app: AppHandle,
    registry: State<'_, SddRegistry>,
    id: String,
    phase: u32,
) -> Result<SddWorkspace, String> {
    let cell = registry
        .workspaces
        .read()
        .get(&id)
        .cloned()
        .ok_or_else(|| format!("unknown workspace {id}"))?;
    let root = PathBuf::from(cell.read().root.clone());
    set_control_file(&root, &format!("phase-{phase}-plan-approved"))?;
    write_substep_state(
        &root,
        &SddPhaseSubstepState {
            phase,
            sub_step: Some(SddPhaseSubstep::Implement),
            started_at: now_ms(),
        },
    )?;
    append_substep_started_event(&root, phase, SddPhaseSubstep::Implement);
    audit::append(
        &root,
        &audit::AuditEntry::new("user", "approve_phase_plan").with_phase(phase),
    );
    audit::append(
        &root,
        &audit::AuditEntry::new("agent", "phase_implement_started")
            .with_phase(phase)
            .with_after(serde_json::json!({"sub_step": "implement"})),
    );
    let snapshot = {
        let mut w = cell.write();
        rebuild_from_disk(&mut w)?;
        w.clone()
    };
    emit_changed(&app, &id);
    Ok(snapshot)
}

/// Three-call mode — discard the plan-pass output during plan-review
/// gate. Flips the phase to `failed` with `trigger: plan_discarded`
/// so the standard failure-card recovery flow (Retry / Edit & retry
/// / Skip) takes over. Different from `sdd_skip_phase_with_reason`
/// (which marks `skipped` and bypasses the failure card). See
/// `spec-1` FR-7.
#[tauri::command]
pub async fn sdd_discard_phase_plan(
    app: AppHandle,
    registry: State<'_, SddRegistry>,
    id: String,
    phase: u32,
    reason: Option<String>,
) -> Result<SddWorkspace, String> {
    let cell = registry
        .workspaces
        .read()
        .get(&id)
        .cloned()
        .ok_or_else(|| format!("unknown workspace {id}"))?;
    let (root, phase_path) = {
        let w = cell.read();
        let p = w
            .phases
            .iter()
            .find(|p| p.number == phase)
            .ok_or_else(|| format!("phase {phase} not found"))?;
        (PathBuf::from(&w.root), PathBuf::from(&p.path))
    };
    let trimmed_reason = reason
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("plan discarded by user during plan-review gate");
    set_status_with_extras(
        &phase_path,
        "failed",
        vec![
            ("trigger".into(), "plan_discarded".into()),
            ("discard_reason".into(), trimmed_reason.into()),
        ],
    )?;
    // Persist reason into phase-meta.json too so the audit log can
    // surface it without re-parsing markdown frontmatter.
    let mut phase_meta = read_phase_meta(&root, phase);
    phase_meta.skip_reason = Some(trimmed_reason.into());
    let _ = write_phase_meta(&root, phase, &phase_meta);
    // Clear the substep checkpoint so a future hydrate doesn't
    // re-trigger the recovery banner on this aborted phase.
    let _ = clear_substep_state(&root, phase);
    audit::append(
        &root,
        &audit::AuditEntry::new("user", "discard_phase_plan")
            .with_phase(phase)
            .with_after(serde_json::json!({
                "trigger": "plan_discarded",
                "reason": trimmed_reason,
            })),
    );
    let snapshot = {
        let mut w = cell.write();
        rebuild_from_disk(&mut w)?;
        w.clone()
    };
    emit_changed(&app, &id);
    Ok(snapshot)
}

/// Persist `meta.json#phase_execution` for the workspace. Validates
/// the budget-pct sum before writing so callers get a typed error
/// instead of a malformed file on disk. Emits an audit row + a
/// `sdd:changed` event so the SddCard reactive store refreshes
/// immediately. See `spec-1` FR-11 / FR-12.
#[tauri::command]
pub async fn sdd_set_phase_execution_config(
    app: AppHandle,
    registry: State<'_, SddRegistry>,
    id: String,
    config: PhaseExecutionConfig,
) -> Result<SddWorkspace, String> {
    config.validate()?;
    let cell = registry
        .workspaces
        .read()
        .get(&id)
        .cloned()
        .ok_or_else(|| format!("unknown workspace {id}"))?;
    let root = PathBuf::from(cell.read().root.clone());
    let mut meta = read_workspace_meta(&root);
    let before = meta.phase_execution.clone();
    meta.phase_execution = config.clone();
    write_workspace_meta(&root, &meta)?;
    audit::append(
        &root,
        &audit::AuditEntry::new("user", "set_phase_execution_config")
            .with_before(serde_json::to_value(&before).unwrap_or(serde_json::Value::Null))
            .with_after(serde_json::to_value(&config).unwrap_or(serde_json::Value::Null)),
    );
    let snapshot = {
        let mut w = cell.write();
        rebuild_from_disk(&mut w)?;
        w.clone()
    };
    emit_changed(&app, &id);
    Ok(snapshot)
}

/// Mark phase `phase` as `skipped` so the gate releases and the next
/// pending phase becomes the candidate. Optional `reason` is appended
/// to the phase frontmatter as a `skip_reason` key. Refuses to skip
/// `running` phases — pause first, then skip from a settled state.
#[tauri::command]
pub async fn sdd_skip_phase(
    app: AppHandle,
    registry: State<'_, SddRegistry>,
    id: String,
    phase: u32,
    reason: Option<String>,
) -> Result<SddWorkspace, String> {
    let cell = registry
        .workspaces
        .read()
        .get(&id)
        .cloned()
        .ok_or_else(|| format!("unknown workspace {id}"))?;
    let path: PathBuf = {
        let w = cell.read();
        let p = w
            .phases
            .iter()
            .find(|p| p.number == phase)
            .ok_or_else(|| format!("phase {phase} not found"))?;
        if p.status == "running" {
            return Err(format!(
                "phase {phase} is currently running — pause first, then skip"
            ));
        }
        PathBuf::from(&p.path)
    };
    set_status_with_extras(
        &path,
        "skipped",
        reason
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .map(|r| ("skip_reason".to_string(), r.to_string()))
            .into_iter()
            .collect(),
    )?;
    let snapshot = {
        let mut w = cell.write();
        rebuild_from_disk(&mut w)?;
        w.clone()
    };
    emit_changed(&app, &id);
    Ok(snapshot)
}

/// Skip a phase with a mandatory reason — used by the failure card's
/// `Skip phase` button to bypass a verifier-flagged failure. Differs
/// from `sdd_skip_phase` in two ways: (a) `reason` is REQUIRED (min 5
/// chars after trim), so the audit trail always has a "why"; (b) it
/// also persists `skip_reason` to `phase-N-meta.json` so the audit
/// log (phase 6) and any future analytics can pick it up without
/// re-parsing the markdown frontmatter.
///
/// Failed phases ARE skippable (the whole point of this command).
/// `running` phases are refused — same as `sdd_skip_phase` — to avoid
/// racing with an active executor.
#[tauri::command]
pub async fn sdd_skip_phase_with_reason(
    app: AppHandle,
    registry: State<'_, SddRegistry>,
    id: String,
    phase: u32,
    reason: String,
) -> Result<SddWorkspace, String> {
    let trimmed = reason.trim();
    if trimmed.len() < 5 {
        return Err(
            "skip reason must be at least 5 characters — explain why this phase is being skipped"
                .into(),
        );
    }
    let cell = registry
        .workspaces
        .read()
        .get(&id)
        .cloned()
        .ok_or_else(|| format!("unknown workspace {id}"))?;
    let (path, root): (PathBuf, PathBuf) = {
        let w = cell.read();
        let p = w
            .phases
            .iter()
            .find(|p| p.number == phase)
            .ok_or_else(|| format!("phase {phase} not found"))?;
        if p.status == "running" {
            return Err(format!(
                "phase {phase} is currently running — pause first, then skip"
            ));
        }
        (PathBuf::from(&p.path), PathBuf::from(&w.root))
    };
    set_status_with_extras(
        &path,
        "skipped",
        vec![("skip_reason".to_string(), trimmed.to_string())],
    )?;
    /* Mirror the reason into phase-meta.json so the audit log and
     *  recovery flows can read it without re-parsing the markdown
     *  frontmatter. */
    let mut pm = read_phase_meta(&root, phase);
    pm.skip_reason = Some(trimmed.to_string());
    let _ = write_phase_meta(&root, phase, &pm);
    audit::append(
        &root,
        &audit::AuditEntry::new("user", "skip_phase")
            .with_phase(phase)
            .with_reason(trimmed)
            .with_after(serde_json::json!({"status": "skipped"})),
    );
    let snapshot = {
        let mut w = cell.write();
        rebuild_from_disk(&mut w)?;
        w.clone()
    };
    emit_changed(&app, &id);
    Ok(snapshot)
}

/// Accept a `failed` phase as-is — flips `status: failed` → `done`
/// and records `accepted_reason` in the phase frontmatter + meta.json.
/// Used by the failure card's "Accept anyway" button when the verifier
/// flagged deviations the user has reviewed and decided are tolerable
/// (e.g. acknowledged trade-offs documented in the verify summary).
/// Requires a non-empty reason (min 5 chars) so the audit trail always
/// has a why. Refuses to accept anything other than a `failed` phase —
/// running / pending / done phases stay untouched.
#[tauri::command]
pub async fn sdd_accept_phase_failed(
    app: AppHandle,
    registry: State<'_, SddRegistry>,
    id: String,
    phase: u32,
    reason: String,
) -> Result<SddWorkspace, String> {
    let trimmed = reason.trim();
    if trimmed.len() < 5 {
        return Err(
            "accept reason must be at least 5 characters — explain why this failure is being accepted"
                .into(),
        );
    }
    let cell = registry
        .workspaces
        .read()
        .get(&id)
        .cloned()
        .ok_or_else(|| format!("unknown workspace {id}"))?;
    let (path, root): (PathBuf, PathBuf) = {
        let w = cell.read();
        let p = w
            .phases
            .iter()
            .find(|p| p.number == phase)
            .ok_or_else(|| format!("phase {phase} not found"))?;
        if p.status != "failed" {
            return Err(format!(
                "phase {phase} is `{}` — Accept only applies to failed phases",
                p.status
            ));
        }
        (PathBuf::from(&p.path), PathBuf::from(&w.root))
    };
    set_status_with_extras(
        &path,
        "done",
        vec![("accepted_reason".to_string(), trimmed.to_string())],
    )?;
    let mut pm = read_phase_meta(&root, phase);
    pm.accepted_reason = Some(trimmed.to_string());
    let _ = write_phase_meta(&root, phase, &pm);
    audit::append(
        &root,
        &audit::AuditEntry::new("user", "accept_phase_failed")
            .with_phase(phase)
            .with_reason(trimmed)
            .with_after(serde_json::json!({"status": "done"})),
    );
    let snapshot = {
        let mut w = cell.write();
        rebuild_from_disk(&mut w)?;
        w.clone()
    };
    emit_changed(&app, &id);
    Ok(snapshot)
}

/// Insert a new phase after the given number. Shifts every existing
/// phase with `number > after_number` up by one (file renamed, frontmatter
/// `phase:` rewritten). Updates `plan.json` to match. Atomic from the
/// user's perspective: a partial failure half-way through leaves the
/// workspace in a broken state — the user should `sdd_discard` and
/// start over. (Acceptable for now since the file surface is small;
/// proper transactional rename would need a stage dir + commit phase.)
#[tauri::command]
pub async fn sdd_insert_phase(
    app: AppHandle,
    registry: State<'_, SddRegistry>,
    id: String,
    after_number: u32,
    title: String,
    depends_on: Option<Vec<u32>>,
) -> Result<SddWorkspace, String> {
    let cell = registry
        .workspaces
        .read()
        .get(&id)
        .cloned()
        .ok_or_else(|| format!("unknown workspace {id}"))?;
    let root = PathBuf::from(cell.read().root.clone());
    if !root.join("plan.json").exists() {
        return Err("workspace is not v2 — call sdd_upgrade_workspace first".into());
    }
    let phases_dir = root.join("phases");
    std::fs::create_dir_all(&phases_dir).map_err(|e| format!("mkdir phases: {e}"))?;
    /* Existing phases — sort descending so we rename higher numbers
     * first and never overwrite an in-use file. */
    let mut existing: Vec<(u32, String)> = cell
        .read()
        .phases
        .iter()
        .map(|p| (p.number, p.slug.clone()))
        .collect();
    existing.sort_by(|a, b| b.0.cmp(&a.0));
    for (num, slug) in &existing {
        if *num > after_number {
            rename_phase_file(&phases_dir, slug, num + 1)?;
        }
    }
    let new_number = after_number + 1;
    let slug_title = slugify(&title);
    let new_slug = format!("{:02}-{}", new_number, slug_title);
    let new_path = phases_dir.join(format!("{new_slug}.md"));
    let depends_repr = match depends_on.as_ref() {
        Some(v) if !v.is_empty() => format!(
            "[{}]",
            v.iter().map(|n| n.to_string()).collect::<Vec<_>>().join(", ")
        ),
        _ => "[]".into(),
    };
    let body = format!(
        "---\nphase: {new_number}\ntitle: {title}\ndepends_on: {depends_repr}\nstatus: pending\ntasks_total: 0\ntasks_completed: 0\n---\n\n## Goal\n\n_(inserted phase — fill in)_\n\n## Tasks\n\n## Verification\n\n```bash\n```\n"
    );
    std::fs::write(&new_path, body).map_err(|e| format!("write phase: {e}"))?;
    rebuild_plan_json(&root)?;
    let snapshot = {
        let mut w = cell.write();
        rebuild_from_disk(&mut w)?;
        w.clone()
    };
    emit_changed(&app, &id);
    Ok(snapshot)
}

/// Reorder phases. `new_order` is a permutation of the current phase
/// numbers — `[3, 1, 2]` means "phase 3 becomes number 1, original 1
/// becomes 2, original 2 becomes 3". Validates the permutation, then
/// renames files via a 2-pass `.staged` shuffle to avoid collisions.
#[tauri::command]
pub async fn sdd_reorder_phases(
    app: AppHandle,
    registry: State<'_, SddRegistry>,
    id: String,
    new_order: Vec<u32>,
) -> Result<SddWorkspace, String> {
    let cell = registry
        .workspaces
        .read()
        .get(&id)
        .cloned()
        .ok_or_else(|| format!("unknown workspace {id}"))?;
    let root = PathBuf::from(cell.read().root.clone());
    if !root.join("plan.json").exists() {
        return Err("workspace is not v2 — call sdd_upgrade_workspace first".into());
    }
    let existing: Vec<(u32, String)> = cell
        .read()
        .phases
        .iter()
        .map(|p| (p.number, p.slug.clone()))
        .collect();
    if new_order.len() != existing.len() {
        return Err(format!(
            "new_order length {} != phase count {}",
            new_order.len(),
            existing.len()
        ));
    }
    {
        let mut sorted = new_order.clone();
        sorted.sort();
        let mut existing_nums: Vec<u32> = existing.iter().map(|(n, _)| *n).collect();
        existing_nums.sort();
        if sorted != existing_nums {
            return Err("new_order is not a permutation of existing phase numbers".into());
        }
    }
    let phases_dir = root.join("phases");
    /* Two-pass shuffle: first move every phase to a `.staged-<oldnum>.md`
     * intermediate name, then rename to the final `<newnum>-<slug>.md`.
     * This avoids the case where two renames target the same destination
     * mid-sequence. */
    let mut slug_by_old: HashMap<u32, String> = HashMap::new();
    for (num, slug) in &existing {
        slug_by_old.insert(*num, slug.clone());
        let from = phases_dir.join(format!("{slug}.md"));
        let staged = phases_dir.join(format!(".staged-{num}.md"));
        std::fs::rename(&from, &staged)
            .map_err(|e| format!("stage rename {}: {e}", from.display()))?;
    }
    for (idx, old_num) in new_order.iter().enumerate() {
        let new_num = (idx + 1) as u32;
        let slug = slug_by_old
            .get(old_num)
            .cloned()
            .ok_or_else(|| format!("internal: missing slug for {old_num}"))?;
        let suffix = slug.split_once('-').map(|(_, s)| s).unwrap_or(&slug);
        let staged = phases_dir.join(format!(".staged-{old_num}.md"));
        let dest = phases_dir.join(format!("{:02}-{suffix}.md", new_num));
        std::fs::rename(&staged, &dest)
            .map_err(|e| format!("final rename {}: {e}", staged.display()))?;
        update_phase_number_in_frontmatter(&dest, new_num)?;
    }
    rebuild_plan_json(&root)?;
    let snapshot = {
        let mut w = cell.write();
        rebuild_from_disk(&mut w)?;
        w.clone()
    };
    emit_changed(&app, &id);
    Ok(snapshot)
}

/// Delete a phase. Refuses if the phase is `running` (would orphan
/// agent's in-flight work). Renumbers subsequent phases down by one.
#[tauri::command]
pub async fn sdd_delete_phase(
    app: AppHandle,
    registry: State<'_, SddRegistry>,
    id: String,
    phase: u32,
) -> Result<SddWorkspace, String> {
    let cell = registry
        .workspaces
        .read()
        .get(&id)
        .cloned()
        .ok_or_else(|| format!("unknown workspace {id}"))?;
    let root = PathBuf::from(cell.read().root.clone());
    if !root.join("plan.json").exists() {
        return Err("workspace is not v2 — call sdd_upgrade_workspace first".into());
    }
    let (target_path, gate_path, existing): (PathBuf, PathBuf, Vec<(u32, String)>) = {
        let w = cell.read();
        let p = w
            .phases
            .iter()
            .find(|p| p.number == phase)
            .ok_or_else(|| format!("phase {phase} not found"))?;
        if p.status == "running" {
            return Err(format!("phase {phase} is running — pause first"));
        }
        (
            PathBuf::from(&p.path),
            root.join(format!("control/phase-{phase}-approved")),
            w.phases
                .iter()
                .map(|p| (p.number, p.slug.clone()))
                .collect(),
        )
    };
    std::fs::remove_file(&target_path).map_err(|e| format!("remove phase file: {e}"))?;
    let _ = std::fs::remove_file(&gate_path);
    let phases_dir = root.join("phases");
    let mut after: Vec<(u32, String)> = existing
        .into_iter()
        .filter(|(n, _)| *n > phase)
        .collect();
    after.sort_by_key(|(n, _)| *n);
    for (num, slug) in &after {
        rename_phase_file(&phases_dir, slug, num - 1)?;
    }
    rebuild_plan_json(&root)?;
    let snapshot = {
        let mut w = cell.write();
        rebuild_from_disk(&mut w)?;
        w.clone()
    };
    emit_changed(&app, &id);
    Ok(snapshot)
}

/// Generate a `plan.json` from the existing phase markdown files. Used
/// to upgrade a v1 workspace (no plan.json on disk) to the v2 schema
/// in-place. `acceptance` is empty for every phase — the agent fills
/// these in on the next plan rewrite (phase 2 of the v2 roadmap).
#[tauri::command]
pub async fn sdd_upgrade_workspace(
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
    let root = PathBuf::from(cell.read().root.clone());
    rebuild_plan_json(&root)?;
    let snapshot = {
        let mut w = cell.write();
        rebuild_from_disk(&mut w)?;
        w.clone()
    };
    emit_changed(&app, &id);
    Ok(snapshot)
}

// ---------------------------------------------------------------------------
// V2 verifier — Tauri-command surface around `sdd_verify` module.
// `sdd_run_verification` runs the acceptance checks for a phase and
// flips the phase frontmatter to `done` / `failed` based on the verdict.
// `sdd_mark_manual_check` lets the user resolve a manual check
// post-verification.
// ---------------------------------------------------------------------------

/// Run acceptance checks for `phase` and persist the result file.
///
/// Verdict mapping (matches the spec's "trigger flip" rules):
///   - `Passed` (incl. empty acceptance list) → phase status flips to
///     `done`. Subsequent gate-derivation moves the workflow forward.
///   - `Failed` → phase status flips to `failed`. Orchestrator surfaces
///     the failure via `derive_stage`'s `Failed` arm.
///   - `ManualPending` → phase status STAYS `running` so the SddCard
///     keeps the user's attention until they mark the manuals via
///     `sdd_mark_manual_check`.
#[tauri::command]
pub async fn sdd_run_verification(
    app: AppHandle,
    registry: State<'_, SddRegistry>,
    id: String,
    phase: u32,
) -> Result<SddWorkspace, String> {
    let cell = registry
        .workspaces
        .read()
        .get(&id)
        .cloned()
        .ok_or_else(|| format!("unknown workspace {id}"))?;
    let root = PathBuf::from(cell.read().root.clone());
    let file = crate::sdd_verify::verify_phase(&root, phase).await?;
    /* Flip phase frontmatter based on verdict. We compute the new
     *  status string under a closure so the borrow checker stays
     *  happy; no async work happens between read and write. */
    let phases_dir = root.join("phases");
    let target_status: Option<&str> = match file.overall_status {
        crate::sdd_verify::OverallStatus::Passed => Some("done"),
        crate::sdd_verify::OverallStatus::Failed => Some("failed"),
        crate::sdd_verify::OverallStatus::ManualPending => None,
    };
    if let Some(new_status) = target_status {
        /* Find the phase file by number in the (now-cached) workspace
         *  snapshot so we don't have to re-glob. */
        let phase_info = {
            let w = cell.read();
            w.phases
                .iter()
                .find(|p| p.number == phase)
                .map(|p| (p.slug.clone(), p.title.clone()))
        };
        if let Some((slug, title)) = phase_info {
            let phase_path = phases_dir.join(format!("{slug}.md"));
            set_status_on(&phase_path, new_status)?;
            /* Post-phase git commit on a successful flip to `done`.
             *  Failures are logged + recorded as `snapshot_skipped`
             *  but never block the status flip — the user's intent
             *  to advance trumps git plumbing. */
            if new_status == "done" {
                let meta = read_workspace_meta(&root);
                if meta.git_enabled {
                    if let Some(repo) = meta.repo_cwd.as_deref() {
                        let body = post_phase_commit_body(&file);
                        let msg = format!("phase-{phase}: {title}\n\n{body}");
                        match crate::git::commit_all_allow_empty(repo, &msg) {
                            Ok(sha) => {
                                let mut pm = read_phase_meta(&root, phase);
                                pm.post_phase_sha = Some(sha);
                                let _ = write_phase_meta(&root, phase, &pm);
                            }
                            Err(e) => {
                                eprintln!(
                                    "[sdd] workspace={id} phase={phase}: post-phase commit failed: {e}"
                                );
                            }
                        }
                    }
                }
            }
        }
    }
    let snapshot = {
        let mut w = cell.write();
        rebuild_from_disk(&mut w)?;
        w.clone()
    };
    emit_changed(&app, &id);
    Ok(snapshot)
}

/// Compose the body of a `phase-N: <slug>` commit message from the
/// verifier's aggregate output. Plain-text, two lines: a one-line
/// "Verified by N checks (M passed, K manual)" summary followed by
/// per-check status lines for the audit trail.
fn post_phase_commit_body(file: &crate::sdd_verify::PhaseAcceptanceFile) -> String {
    use crate::sdd_verify::CheckStatus;
    let total = file.results.len();
    let passed = file
        .results
        .iter()
        .filter(|r| matches!(r.status, CheckStatus::Passed))
        .count();
    // "Manual" here = original kind == "manual", regardless of verdict —
    // tells reviewers how many of the green check-marks were human-
    // approved vs autonomous.
    let manual = file.results.iter().filter(|r| r.kind == "manual").count();
    let mut s = format!("Verified by {total} checks ({passed} passed, {manual} manual)\n");
    for r in &file.results {
        let mark = match r.status {
            CheckStatus::Passed => "✓",
            CheckStatus::Failed => "✗",
            CheckStatus::ManualUnmarked => "?",
            CheckStatus::Skipped => "·",
            CheckStatus::Pending => "…",
        };
        s.push_str(&format!("- {mark} {}\n", r.kind));
    }
    s
}

/// Resolve a manual acceptance check by user verdict. Recomputes the
/// aggregate verdict + (if it now flips to `Passed` or `Failed`) the
/// phase frontmatter, just like `sdd_run_verification` does at first
/// run. Idempotent — a second call with the same args is a no-op
/// past the JSON write.
#[tauri::command]
pub async fn sdd_mark_manual_check(
    app: AppHandle,
    registry: State<'_, SddRegistry>,
    id: String,
    phase: u32,
    check_index: usize,
    passed: bool,
) -> Result<SddWorkspace, String> {
    let cell = registry
        .workspaces
        .read()
        .get(&id)
        .cloned()
        .ok_or_else(|| format!("unknown workspace {id}"))?;
    let root = PathBuf::from(cell.read().root.clone());
    let file = crate::sdd_verify::mark_manual_check(&root, phase, check_index, passed)?;
    let target_status: Option<&str> = match file.overall_status {
        crate::sdd_verify::OverallStatus::Passed => Some("done"),
        crate::sdd_verify::OverallStatus::Failed => Some("failed"),
        crate::sdd_verify::OverallStatus::ManualPending => None,
    };
    if let Some(new_status) = target_status {
        let phase_slug = {
            let w = cell.read();
            w.phases
                .iter()
                .find(|p| p.number == phase)
                .map(|p| p.slug.clone())
        };
        if let Some(slug) = phase_slug {
            let phase_path = root.join("phases").join(format!("{slug}.md"));
            set_status_on(&phase_path, new_status)?;
        }
    }
    let snapshot = {
        let mut w = cell.write();
        rebuild_from_disk(&mut w)?;
        w.clone()
    };
    emit_changed(&app, &id);
    Ok(snapshot)
}

// ---------------------------------------------------------------------------
// Git integration commands — phase rollback, crash recovery, and git-row
// state for the SddCard. Available on every workspace; calls on a
// non-git workspace return a `git_enabled = false` shape rather than an
// error so the UI can render a degraded badge instead of a popup.
// ---------------------------------------------------------------------------

/// Roll a phase back to its `pre_phase_sha` snapshot. The user's
/// current changes are first safety-stashed under
/// `sdd-rollback-safety-<phase>-<id>`; then `git reset --hard` jumps
/// to the snapshot; then the phase frontmatter resets to `pending` and
/// any per-phase acceptance JSON is wiped so the next approve cycle
/// starts clean. Returns the refreshed workspace snapshot.
///
/// Errors when:
///   - workspace is not git-enabled (no snapshot to roll back to);
///   - the phase has no recorded `pre_phase_sha` (snapshot was skipped);
///   - the phase is currently `running` (must pause / stop first).
#[tauri::command]
pub async fn sdd_rollback_phase(
    app: AppHandle,
    registry: State<'_, SddRegistry>,
    id: String,
    phase: u32,
) -> Result<SddWorkspace, String> {
    let cell = registry
        .workspaces
        .read()
        .get(&id)
        .cloned()
        .ok_or_else(|| format!("unknown workspace {id}"))?;
    let root = PathBuf::from(cell.read().root.clone());
    let meta = read_workspace_meta(&root);
    if !meta.git_enabled {
        return Err("workspace is not git-enabled — no rollback available".into());
    }
    let repo = meta
        .repo_cwd
        .as_deref()
        .ok_or_else(|| "workspace meta missing repo_cwd".to_string())?;
    let phase_meta = read_phase_meta(&root, phase);
    let pre_sha = phase_meta
        .pre_phase_sha
        .as_deref()
        .ok_or_else(|| format!("phase {phase} has no pre_phase_sha — snapshot was skipped"))?;
    /* Pull phase slug + sanity-check status before destructive ops.
     *  We refuse to roll a `running` phase — the user must pause /
     *  stop first so we don't reset under a live agent. */
    let (phase_path, current_status) = {
        let w = cell.read();
        let p = w
            .phases
            .iter()
            .find(|p| p.number == phase)
            .ok_or_else(|| format!("phase {phase} not found"))?;
        (PathBuf::from(&p.path), p.status.clone())
    };
    if current_status == "running" {
        return Err(format!(
            "phase {phase} is running — pause or stop the workflow before rolling back"
        ));
    }
    let safety_label = format!("sdd-rollback-safety-{}-{}", phase, id);
    let _ = crate::git::stash_with_label(repo, &safety_label);
    crate::git::reset_hard(repo, pre_sha).map_err(|e| format!("reset --hard: {e}"))?;
    set_status_on(&phase_path, "pending")?;
    /* Wipe per-phase acceptance results + post_phase_sha so the next
     *  approve cycle starts from zero. We keep `pre_phase_sha` so the
     *  user can roll forward / back repeatedly without losing the
     *  rollback target. */
    let acceptance_path = root.join("results").join(format!("phase-{phase}-acceptance.json"));
    let _ = std::fs::remove_file(acceptance_path);
    let mut pm = phase_meta;
    pm.post_phase_sha = None;
    let _ = write_phase_meta(&root, phase, &pm);
    /* Drop the `phase-N-approved` gate too, so the user re-approves
     *  before the orchestrator advances. */
    let _ = std::fs::remove_file(root.join("control").join(format!("phase-{phase}-approved")));
    let snapshot = {
        let mut w = cell.write();
        rebuild_from_disk(&mut w)?;
        w.clone()
    };
    emit_changed(&app, &id);
    Ok(snapshot)
}

/// Resolve a detected crash-recovery situation. `action`:
///   - `"rollback"` — same effect as `sdd_rollback_phase` on the
///     orphan phase (fast-path so the UI doesn't have to look up
///     the phase number itself).
///   - `"keep"` — flip the orphan phase from `running`/`verifying`
///     to `failed` so the user can decide whether to retry / skip
///     manually. No git side-effects.
#[tauri::command]
pub async fn sdd_recover_workspace(
    app: AppHandle,
    registry: State<'_, SddRegistry>,
    id: String,
    action: String,
) -> Result<SddWorkspace, String> {
    let cell = registry
        .workspaces
        .read()
        .get(&id)
        .cloned()
        .ok_or_else(|| format!("unknown workspace {id}"))?;
    /* Snapshot the orphan phase under the read lock so we don't hold
     *  it across async work. */
    let orphan = {
        let w = cell.read();
        match w.recovery_state.as_ref() {
            Some(SddRecoveryState::OrphanPhase { phase, .. }) => Some(*phase),
            None => None,
        }
    };
    let phase = orphan.ok_or_else(|| "no recovery_state on workspace".to_string())?;
    match action.as_str() {
        "rollback" => sdd_rollback_phase(app, registry, id, phase).await,
        "keep" => {
            let path: PathBuf = {
                let w = cell.read();
                w.phases
                    .iter()
                    .find(|p| p.number == phase)
                    .map(|p| PathBuf::from(&p.path))
                    .ok_or_else(|| format!("phase {phase} not found"))?
            };
            set_status_with_extras(
                &path,
                "failed",
                vec![("recovery_action".into(), "keep".into())],
            )?;
            let snapshot = {
                let mut w = cell.write();
                rebuild_from_disk(&mut w)?;
                w.clone()
            };
            emit_changed(&app, &id);
            Ok(snapshot)
        }
        other => Err(format!(
            "unknown recovery action `{other}` (expected `rollback` or `keep`)"
        )),
    }
}

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
pub(crate) use crate::sdd_plan_helpers::{
    rebuild_plan_json, rename_phase_file, set_status_with_extras, slugify,
    update_phase_number_in_frontmatter,
};

/// Optional convenience — wipe a workspace dir from disk + drop it from
/// memory. Used by the UI's "discard" button on a stopped workspace.
#[tauri::command]
pub async fn sdd_discard(
    app: AppHandle,
    registry: State<'_, SddRegistry>,
    id: String,
) -> Result<(), String> {
    let cell = registry
        .workspaces
        .read()
        .get(&id)
        .cloned()
        .ok_or_else(|| format!("unknown workspace {id}"))?;
    let root = PathBuf::from(cell.read().root.clone());
    let _ = std::fs::remove_dir_all(&root);
    registry.workspaces.write().remove(&id);
    let _ = app.emit("sdd:discarded", &id);
    Ok(())
}

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
