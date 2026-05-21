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
use std::time::{SystemTime, UNIX_EPOCH};

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

const WORKSPACE_DIR: &str = "sdd-workspaces";

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn short_id() -> String {
    use uuid::Uuid;
    let s = Uuid::new_v4().simple().to_string();
    format!("sdd-{}", &s[..10])
}

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
    /// Phase `phase` is being executed by the agent.
    PhaseRunning { phase: u32 },
    /// V2 only — phase body status is `done` but acceptance criteria are
    /// still being evaluated by the verifier (phase 2 of the SDD v2
    /// roadmap). Placeholder in this phase; `derive_stage` does not yet
    /// emit it.
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
    },
}

// ---------------------------------------------------------------------------
// Plan-as-data — v2 schema. Lives alongside `plan.md` as `plan.json`.
// Markdown stays the human-readable view; JSON is the structured source of
// truth for verifier acceptance criteria + plan-editor mutations.
// ---------------------------------------------------------------------------

/// Top-level shape of `<workspace>/plan.json`. `version` is bumped if the
/// schema ever changes incompatibly; `phases` mirrors the
/// `phases/NN-*.md` files but adds metadata the markdown can't carry
/// (acceptance criteria, complexity hint).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SddPlanFile {
    pub version: u32,
    pub phases: Vec<SddPlanPhase>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SddPlanPhase {
    pub number: u32,
    pub slug: String,
    pub title: String,
    #[serde(default)]
    pub depends_on: Vec<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub complexity: Option<String>,
    /// Verifier criteria. Empty in phase 1 of the v2 roadmap; populated
    /// by phase 2 of the roadmap when the agent learns to write them.
    #[serde(default)]
    pub acceptance: Vec<SddPhaseAcceptance>,
}

/// Verifier-check spec. `serde(tag = "type")` matches the JSON shape
/// described in the SDD spec (`{ "type": "shell", "cmd": "…" }`).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SddPhaseAcceptance {
    Shell {
        cmd: String,
        #[serde(default)]
        expect_exit: i32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        stdout_match: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timeout_ms: Option<u64>,
    },
    FileExists {
        paths: Vec<String>,
    },
    Manual {
        description: String,
    },
}

/// Read `<workspace>/plan.json`. `None` for legacy workspaces (file
/// absent) AND for files that exist but parse-fail — we tolerate the
/// latter rather than poison the workspace state. The agent (or user
/// via the inline editor) will rewrite a healthy version on the next
/// turn.
pub(crate) fn read_plan_json(root: &Path) -> Option<SddPlanFile> {
    let path = root.join("plan.json");
    let raw = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&raw).ok()
}

/// Write `<workspace>/plan.json`. Atomic via `.tmp` + rename so a
/// concurrent reader can never observe a torn file. Uses the same
/// pattern as `set_status_on` on the markdown side.
fn write_plan_json(root: &Path, plan: &SddPlanFile) -> Result<(), String> {
    let path = root.join("plan.json");
    let content = serde_json::to_string_pretty(plan)
        .map_err(|e| format!("serialize plan.json: {e}"))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, content).map_err(|e| format!("write plan.json.tmp: {e}"))?;
    std::fs::rename(&tmp, &path)
        .map_err(|e| format!("rename plan.json.tmp -> plan.json: {e}"))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Frontmatter parsing — small, self-contained, no `gray-matter` crate.
// ---------------------------------------------------------------------------

/// YAML frontmatter shape for spec/plan/phase files. All fields optional —
/// we read what's there and fall back to defaults for missing keys.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct FrontMatter {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    phase: Option<serde_yaml::Value>, // can be u32 or "01-foo"
    #[serde(default)]
    depends_on: Vec<u32>,
    #[serde(default)]
    tasks_total: Option<u32>,
    #[serde(default)]
    tasks_completed: Option<u32>,
    #[serde(default)]
    total_phases: Option<u32>,
    #[serde(default)]
    created: Option<String>,
    #[serde(default)]
    updated: Option<String>,
}

/// Split a markdown file with leading `---\n…\n---\n` YAML frontmatter
/// into (parsed_yaml, body). Files with no frontmatter return
/// `(default, full_content)` — we tolerate sloppy agent output rather
/// than 500-erroring.
fn parse_frontmatter(content: &str) -> (FrontMatter, String) {
    let trimmed = content.trim_start_matches('\u{feff}'); // strip BOM
    if !trimmed.starts_with("---") {
        return (FrontMatter::default(), content.to_string());
    }
    // Find the closing `---` on its own line.
    let after_open = &trimmed[3..];
    // Skip the trailing newline of the opening fence.
    let after_open = after_open.strip_prefix('\n').unwrap_or(after_open);
    // Search for `\n---` followed by newline-or-EOF.
    let close_idx = after_open
        .find("\n---")
        .or_else(|| if after_open.starts_with("---") { Some(0) } else { None });
    let Some(idx) = close_idx else {
        return (FrontMatter::default(), content.to_string());
    };
    let yaml_str = &after_open[..idx];
    let rest = &after_open[idx..];
    // Strip the closing fence + optional newline.
    let body = rest
        .strip_prefix("\n---")
        .or_else(|| rest.strip_prefix("---"))
        .unwrap_or(rest)
        .trim_start_matches('\n')
        .to_string();
    let fm = serde_yaml::from_str::<FrontMatter>(yaml_str).unwrap_or_default();
    (fm, body)
}

/// Inverse of `parse_frontmatter` — write `<--- yaml ---> body`. Writes
/// atomically (`.tmp` + rename) so a reader can never observe a half-
/// written file. Borrowed straight from brain's `ProgressWriter`
/// pattern — same reason: the orchestrator (TS) is racing with the
/// agent's file writes and a torn read would crash the YAML parser.
///
/// NOTE: currently unused — F1 only modifies existing frontmatter via
/// `set_status_on` (which preserves unknown fields). Kept for v2 when
/// we'll write skeleton spec.md / plan.md placeholders into a fresh
/// workspace before the agent fills them in (helps agents that struggle
/// with "the file doesn't exist yet" framing).
#[allow(dead_code)]
fn write_with_frontmatter(path: &Path, fm: &FrontMatter, body: &str) -> Result<(), String> {
    let yaml = serde_yaml::to_string(fm).map_err(|e| format!("yaml: {e}"))?;
    let content = format!("---\n{yaml}---\n\n{}\n", body.trim_end());
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
    }
    let tmp = path.with_extension("md.tmp");
    std::fs::write(&tmp, content).map_err(|e| format!("write tmp {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("rename {} → {}: {e}", tmp.display(), path.display()))
}

// ---------------------------------------------------------------------------
// Registry + lifecycle.
// ---------------------------------------------------------------------------

/// Inner shared state — held inside an `Arc` so the file-watcher
/// thread can keep its own clone and refresh workspaces without going
/// through Tauri's `State<'_, SddRegistry>` (which is bound to the
/// command's lifetime).
type SharedWorkspaces = Arc<RwLock<HashMap<String, Arc<RwLock<SddWorkspace>>>>>;

pub struct SddRegistry {
    workspaces: SharedWorkspaces,
    base_dir: RwLock<Option<PathBuf>>,
    /// File-system watcher — one `notify::RecommendedWatcher` for the
    /// whole base dir. Lazy-initialized on first `sdd_start`. Held in
    /// the struct so it doesn't get dropped (drop = stop watching).
    watcher: parking_lot::Mutex<Option<notify::RecommendedWatcher>>,
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

/// Scan a workspace directory and rebuild the in-memory `SddWorkspace`
/// from disk. Idempotent — call this whenever we suspect the agent has
/// modified files. The stage is derived from which files exist + their
/// frontmatter statuses, so the agent doesn't have to keep a parallel
/// state machine in its head — it just writes files.
fn rebuild_from_disk(workspace: &mut SddWorkspace) -> Result<(), String> {
    let root = PathBuf::from(&workspace.root);
    workspace.updated_at = now_ms();

    // --- spec ---
    let spec_path = root.join("spec.md");
    let mut spec_status: Option<String> = None;
    if spec_path.exists() {
        let raw = std::fs::read_to_string(&spec_path)
            .map_err(|e| format!("read spec: {e}"))?;
        let (fm, body) = parse_frontmatter(&raw);
        spec_status = fm.status.clone();
        workspace.spec_path = Some(spec_path.to_string_lossy().into_owned());
        workspace.spec_body = Some(body);
    } else {
        workspace.spec_path = None;
        workspace.spec_body = None;
    }

    // --- plan ---
    let plan_path = root.join("plan.md");
    let mut plan_status: Option<String> = None;
    if plan_path.exists() {
        let raw = std::fs::read_to_string(&plan_path)
            .map_err(|e| format!("read plan: {e}"))?;
        let (fm, body) = parse_frontmatter(&raw);
        plan_status = fm.status.clone();
        workspace.plan_path = Some(plan_path.to_string_lossy().into_owned());
        workspace.plan_body = Some(body);
    } else {
        workspace.plan_path = None;
        workspace.plan_body = None;
    }

    // --- summary (post-completion wrap-up) ---
    let summary_path = root.join("SUMMARY.md");
    if summary_path.exists() {
        let raw = std::fs::read_to_string(&summary_path)
            .map_err(|e| format!("read summary: {e}"))?;
        let (_, body) = parse_frontmatter(&raw);
        workspace.summary_path = Some(summary_path.to_string_lossy().into_owned());
        workspace.summary_body = Some(body);
    } else {
        workspace.summary_path = None;
        workspace.summary_body = None;
    }

    // --- phases ---
    workspace.phases.clear();
    let phases_dir = root.join("phases");
    if phases_dir.is_dir() {
        let mut entries: Vec<PathBuf> = std::fs::read_dir(&phases_dir)
            .map_err(|e| format!("read phases: {e}"))?
            .filter_map(|r| r.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|e| e == "md"))
            .collect();
        entries.sort();
        let results_dir = root.join("results");
        for path in entries {
            let raw = std::fs::read_to_string(&path)
                .map_err(|e| format!("read phase {}: {e}", path.display()))?;
            let (fm, body) = parse_frontmatter(&raw);
            let slug = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("00-unknown")
                .to_string();
            let number = phase_number_from(&slug, &fm);
            // Result summary (best-effort)
            let summary = {
                let p = results_dir.join(format!("{slug}-result.md"));
                if p.exists() {
                    std::fs::read_to_string(&p).ok().map(|r| {
                        let (_, body) = parse_frontmatter(&r);
                        body
                    })
                } else {
                    None
                }
            };
            workspace.phases.push(SddPhase {
                number,
                slug,
                title: fm.title.clone().unwrap_or_else(|| "Untitled phase".into()),
                depends_on: fm.depends_on.clone(),
                status: fm.status.clone().unwrap_or_else(|| "pending".into()),
                tasks_total: fm.tasks_total.unwrap_or(0),
                tasks_done: fm.tasks_completed.unwrap_or(0),
                body,
                path: path.to_string_lossy().into_owned(),
                summary,
            });
        }
        workspace.phases.sort_by_key(|p| p.number);
    }

    // --- v2 detection ---
    /* `plan.json` is the v2 marker. Its presence flips `is_v2` to true,
     * which `derive_stage` reads to decide whether to insert the
     * per-phase approval gate. Legacy workspaces (no plan.json) keep
     * the v1 auto-advance flow byte-for-byte. */
    workspace.is_v2 = root.join("plan.json").exists();

    // --- crash-recovery probe ---
    /* A phase whose status is still `running` or `verifying` BUT whose
     *  per-phase meta has no `post_phase_sha` is almost certainly an
     *  orphan — the agent was killed mid-phase. Surface it so the
     *  card can offer "Rollback" / "Mark failed". Cheap probe: only
     *  reads phase-N-meta.json side-cars, never shells out to git. */
    workspace.recovery_state = workspace
        .phases
        .iter()
        .find(|p| p.status == "running" || p.status == "verifying")
        .and_then(|p| {
            let meta = read_phase_meta(&root, p.number);
            if meta.post_phase_sha.is_some() {
                None
            } else {
                Some(SddRecoveryState::OrphanPhase {
                    phase: p.number,
                    pre_phase_sha: meta.pre_phase_sha.clone(),
                })
            }
        });

    // --- derive stage ---
    /* Stage transition table — based on what files exist + their statuses.
     *  We DON'T blindly trust agent's `status: completed` in plan.md until
     *  the user has approved it via `sdd_approve`, because the agent's
     *  free to mark anything done; the user is the gate. */
    workspace.stage = derive_stage(workspace, spec_status, plan_status);

    Ok(())
}

/// Phase number derivation — supports `phase: 1` and `phase: "01-foundation"`
/// and falls back to filename prefix like `01-foundation.md`.
fn phase_number_from(slug: &str, fm: &FrontMatter) -> u32 {
    if let Some(v) = &fm.phase {
        if let Some(n) = v.as_u64() {
            return n as u32;
        }
        if let Some(s) = v.as_str() {
            if let Some(num) = s.split('-').next().and_then(|p| p.parse::<u32>().ok()) {
                return num;
            }
        }
    }
    slug.split('-').next().and_then(|p| p.parse::<u32>().ok()).unwrap_or(0)
}

/// Pure function — stage from files. Kept separate so it's easy to unit-test.
fn derive_stage(
    w: &SddWorkspace,
    spec_status: Option<String>,
    plan_status: Option<String>,
) -> SddStage {
    /* Filesystem-backed pause/stop signals — same convention as
     *  brain's orchestrator. The card writes `control/stop` /
     *  `control/pause` on user click, which lets state survive an
     *  app restart (in-memory `w.stage` is reset to Drafting on
     *  hydrate, so we have to recover the user's intent from disk). */
    let root = PathBuf::from(&w.root);
    if root.join("control/stop").exists() {
        return SddStage::Stopped;
    }
    if root.join("control/pause").exists() {
        return SddStage::Paused;
    }
    // Honour in-memory overrides too (the control file may not have
    // landed yet between flip_stage + the immediate rebuild).
    if matches!(w.stage, SddStage::Paused | SddStage::Stopped) {
        return w.stage.clone();
    }
    if let SddStage::Failed { .. } = &w.stage {
        return w.stage.clone();
    }
    // No spec yet -> Drafting.
    if w.spec_body.is_none() {
        return SddStage::Drafting;
    }
    let spec_approved = matches!(spec_status.as_deref(), Some("approved"));
    if !spec_approved {
        return SddStage::SpecReady;
    }
    // Spec approved — agent should now be working on the plan.
    if w.plan_body.is_none() || w.phases.is_empty() {
        return SddStage::Planning;
    }
    let plan_approved = matches!(plan_status.as_deref(), Some("approved"));
    if !plan_approved {
        return SddStage::PlanReady;
    }
    // Plan approved — check for failure first so a failed phase
    // doesn't get masked by a later "done" sibling. (Sequential exec
    // means this is the latest phase touched.)
    if let Some(p) = w.phases.iter().find(|p| p.status == "failed") {
        /* Hydrate the structured failure payload from on-disk artefacts.
         *  - `failed_checks` comes from `phase-N-acceptance.json` (written
         *    by the verifier in phase 2). When the file is absent or all
         *    checks pass somehow, we leave it empty.
         *  - `action_log_tail` is the last 10 entries from
         *    `phase-N.log.jsonl` (written by phase 4's tool-event
         *    listener). Best-effort: a missing file just means the agent
         *    didn't emit any tool events (or the user is on an old
         *    workspace pre-phase-4) — we surface an empty tail.
         *  - `trigger` defaults to `CheckFailed` when there are failed
         *    checks; absent that signal we fall back to `Exception` since
         *    the only other ways into `status: failed` today are the
         *    agent setting it after exhausting retries (Exception-ish).
         *    Future code paths (timeout, crash) will set the tag
         *    explicitly. */
        let root = PathBuf::from(&w.root);
        let failed_checks = read_failed_check_indices(&root, p.number);
        let action_log_tail = read_action_log_tail(&root, p.number, 10);
        let trigger = if !failed_checks.is_empty() {
            Some(FailureTrigger::CheckFailed)
        } else {
            Some(FailureTrigger::Exception)
        };
        return SddStage::Failed {
            reason: format!("Phase {} ({}) failed — see result file", p.number, p.title),
            failed_phase: Some(p.number),
            trigger,
            failed_checks,
            action_log_tail,
        };
    }
    let running_phase = w.phases.iter().find(|p| p.status == "running");
    if let Some(p) = running_phase {
        return SddStage::PhaseRunning { phase: p.number };
    }
    /* `skipped` phases are treated like `done` for advancement purposes —
     * they don't block the next gate. We treat any phase that is `done`
     * OR `skipped` as "completed" for the all-done check + last-done
     * lookup. */
    let is_completed = |p: &SddPhase| p.status == "done" || p.status == "skipped";
    let all_done = !w.phases.is_empty() && w.phases.iter().all(is_completed);
    if all_done {
        return SddStage::Complete;
    }
    let last_done = w.phases.iter().rev().find(|p| is_completed(p));
    /* V2 gate insertion. Before falling through to PlanReady / PhaseDone
     * (which the frontend uses as "auto-fire next phase prompt"), check
     * whether the next pending phase needs a per-phase approval. The
     * gate is satisfied by writing `<workspace>/control/phase-N-approved`
     * (done by `sdd_approve_phase`).
     *
     * Legacy v1 workspaces (no plan.json) skip this entire block —
     * their auto-advance behaviour is byte-for-byte preserved. */
    if w.is_v2 {
        let prev = last_done.map(|p| p.number).unwrap_or(0);
        let next = w
            .phases
            .iter()
            .find(|p| p.number > prev && p.status == "pending");
        if let Some(p) = next {
            let approved = Path::new(&w.root)
                .join(format!("control/phase-{}-approved", p.number))
                .exists();
            if !approved {
                return SddStage::PhasePendingApproval { phase: p.number };
            }
        }
    }
    if let Some(p) = last_done {
        return SddStage::PhaseDone { phase: p.number };
    }
    // No phase running, no phase done — execution hasn't started but plan
    // is approved. UI shows the "start phase 1" affordance under PlanReady.
    SddStage::PlanReady
}

/// Mutate the `status:` field of a markdown file's frontmatter on disk
/// WITHOUT losing other fields the agent might have added. We read the
/// raw YAML into a generic `serde_yaml::Mapping`, patch the `status`
/// key, and re-serialize — round-tripping preserves any custom fields
/// (verification commands, etc) that our typed FrontMatter doesn't
/// know about.
fn set_status_on(path: &Path, new_status: &str) -> Result<(), String> {
    let raw = std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let (yaml_str, body) = split_frontmatter_raw(&raw);
    let mut map: serde_yaml::Mapping = if yaml_str.trim().is_empty() {
        serde_yaml::Mapping::new()
    } else {
        serde_yaml::from_str(&yaml_str).map_err(|e| format!("parse frontmatter: {e}"))?
    };
    map.insert(
        serde_yaml::Value::String("status".into()),
        serde_yaml::Value::String(new_status.into()),
    );
    map.insert(
        serde_yaml::Value::String("updated".into()),
        serde_yaml::Value::String(format_iso(now_ms())),
    );
    let new_yaml = serde_yaml::to_string(&map).map_err(|e| format!("yaml: {e}"))?;
    let content = format!("---\n{new_yaml}---\n\n{}\n", body.trim_end());
    let tmp = path.with_extension("md.tmp");
    std::fs::write(&tmp, content).map_err(|e| format!("write tmp: {e}"))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("rename: {e}"))?;
    Ok(())
}

/// Replace the body of a markdown file while preserving its YAML
/// frontmatter verbatim. Used by `sdd_save_body` so the user can edit
/// spec.md / plan.md / phase.md content inline in the card without
/// blowing away the agent's frontmatter (status, depends_on, etc).
fn replace_body_on(path: &Path, new_body: &str) -> Result<(), String> {
    let raw = std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let (yaml_str, _) = split_frontmatter_raw(&raw);
    let content = if yaml_str.is_empty() {
        format!("{}\n", new_body.trim_end())
    } else {
        format!("---\n{yaml_str}\n---\n\n{}\n", new_body.trim_end())
    };
    let tmp = path.with_extension("md.tmp");
    std::fs::write(&tmp, content).map_err(|e| format!("write tmp: {e}"))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("rename: {e}"))?;
    Ok(())
}

/// Reset a phase file's status back to `pending` so the orchestrator
/// will re-issue it on the next advance. Used by the card's "Retry"
/// button on a failed phase. Also clears any tasks_completed counter
/// so the agent re-attempts from the top.
fn reset_phase_status(path: &Path) -> Result<(), String> {
    let raw = std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let (yaml_str, body) = split_frontmatter_raw(&raw);
    let mut map: serde_yaml::Mapping = if yaml_str.trim().is_empty() {
        serde_yaml::Mapping::new()
    } else {
        serde_yaml::from_str(&yaml_str).map_err(|e| format!("parse frontmatter: {e}"))?
    };
    map.insert(
        serde_yaml::Value::String("status".into()),
        serde_yaml::Value::String("pending".into()),
    );
    map.insert(
        serde_yaml::Value::String("tasks_completed".into()),
        serde_yaml::Value::Number(serde_yaml::Number::from(0)),
    );
    let new_yaml = serde_yaml::to_string(&map).map_err(|e| format!("yaml: {e}"))?;
    let content = format!("---\n{new_yaml}---\n\n{}\n", body.trim_end());
    let tmp = path.with_extension("md.tmp");
    std::fs::write(&tmp, content).map_err(|e| format!("write tmp: {e}"))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("rename: {e}"))?;
    Ok(())
}

/// Split markdown content into raw YAML frontmatter string + body, no
/// parsing. Used by `set_status_on` to preserve unknown fields.
fn split_frontmatter_raw(content: &str) -> (String, String) {
    let trimmed = content.trim_start_matches('\u{feff}');
    if !trimmed.starts_with("---") {
        return (String::new(), content.to_string());
    }
    let after_open = &trimmed[3..];
    let after_open = after_open.strip_prefix('\n').unwrap_or(after_open);
    let Some(idx) = after_open.find("\n---") else {
        return (String::new(), content.to_string());
    };
    let yaml_str = &after_open[..idx];
    let rest = &after_open[idx..];
    let body = rest
        .strip_prefix("\n---")
        .unwrap_or(rest)
        .trim_start_matches('\n')
        .to_string();
    (yaml_str.to_string(), body)
}

fn format_iso(_ms: u64) -> String {
    // chrono not in deps; use a coarse YYYY-MM-DD via SystemTime.
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Naive: days since epoch → date. Good enough for "created/updated"
    // tracking; we don't need precision down to the second here.
    let days = secs / 86_400;
    let (y, m, d) = days_to_ymd(days as i64);
    format!("{y:04}-{m:02}-{d:02}")
}

// Civil date arithmetic from days-since-epoch. Algorithm by Howard Hinnant.
fn days_to_ymd(z: i64) -> (i32, u32, u32) {
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

// ---------------------------------------------------------------------------
// Prompt templates — embedded at compile time. The orchestrator (TS) reads
// these via the `sdd_prompt` Tauri command and injects them into the next
// agent message. Keeping templates in Rust (vs TS) means they live next
// to the schema they reference and survive a JS bundle reload.
// ---------------------------------------------------------------------------

const SPEC_TEMPLATE_PROMPT: &str = include_str!("./sdd_prompts/spec.md");
const PLAN_TEMPLATE_PROMPT: &str = include_str!("./sdd_prompts/plan.md");
const PHASE_TEMPLATE_PROMPT: &str = include_str!("./sdd_prompts/phase.md");
const SUMMARY_TEMPLATE_PROMPT: &str = include_str!("./sdd_prompts/summary.md");
const AMEND_TEMPLATE_PROMPT: &str = include_str!("./sdd_prompts/amend.md");

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SddPromptKind {
    Spec,
    Plan,
    Phase,
    Summary,
    /// In-place correction of the active spec / plan / phase. Used
    /// when the user types a delta mid-workflow instead of approving
    /// the current artifact — the agent edits files in place rather
    /// than scaffolding a fresh workspace.
    Amend,
}

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
        };
        let _ = rebuild_from_disk(&mut ws);
        // Recover user_prompt + session_id from optional meta.json side-car.
        if let Ok(meta_raw) = std::fs::read_to_string(path.join("meta.json")) {
            if let Ok(meta) = serde_json::from_str::<SddMeta>(&meta_raw) {
                ws.user_prompt = meta.user_prompt.unwrap_or_default();
                ws.session_id = meta.session_id;
                ws.created_at = meta.created_at.unwrap_or(ws.created_at);
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

/// Side-car metadata stored at `<workspace>/meta.json`. Currently
/// holds the bits that DON'T live in spec.md frontmatter (session id,
/// user's original ask) so they survive an app restart even when
/// the agent hasn't written spec.md yet.
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub(crate) struct SddMeta {
    user_prompt: Option<String>,
    session_id: Option<String>,
    created_at: Option<u64>,
    /// Absolute path of the repo cwd the SDD session is editing — typically
    /// the linked editor / agent session's cwd at start time. None when the
    /// caller didn't supply one (legacy v1 workspaces, or non-git callers).
    /// All git operations target this directory; if it isn't a git repo,
    /// `git_enabled` stays false and the orchestrator skips snapshots /
    /// commits / rollback (degraded mode per the spec).
    #[serde(default)]
    repo_cwd: Option<String>,
    #[serde(default)]
    git_enabled: bool,
    /// `sdd/<workspace-id>` — the per-workspace branch we mint at start.
    /// Stays None when `git_enabled = false`.
    #[serde(default)]
    sdd_branch: Option<String>,
    /// Sha of `repo_cwd`'s HEAD at workspace creation. Used as the
    /// "rollback to clean slate" target when the user wipes the whole
    /// workspace, separate from per-phase pre-snapshots.
    #[serde(default)]
    parent_sha: Option<String>,
}

/// Per-phase meta side-car at `<workspace>/phases/phase-N-meta.json`.
/// Holds the git shas captured around each phase so rollback / recovery
/// can return to a known state. Kept separate from the markdown
/// frontmatter because shas are noisy data the user shouldn't be
/// hand-editing.
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub(crate) struct SddPhaseMeta {
    /// Sha captured immediately before the phase ran (the rollback target).
    /// None when git_enabled was false at approve-time.
    #[serde(default)]
    pub(crate) pre_phase_sha: Option<String>,
    /// Sha of the commit that captured the phase's output. None when
    /// the phase hasn't completed verification yet, or git was off.
    #[serde(default)]
    pub(crate) post_phase_sha: Option<String>,
    /// True when the workspace was non-git at approve-time so we
    /// deliberately skipped the snapshot. Lets the UI label the
    /// phase as "no rollback available" instead of "missing".
    #[serde(default)]
    pub(crate) snapshot_skipped: bool,
    /// User-supplied reason when the phase was force-skipped via
    /// `sdd_skip_phase_with_reason` (failure card's Skip button).
    /// Mirrors the `skip_reason` frontmatter key but kept here too so
    /// the audit log can read it without re-parsing markdown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) skip_reason: Option<String>,
    /// Number of retry attempts made on this phase. Bumped by
    /// `sdd_retry_phase` (and the edit-then-retry flow). The UI shows
    /// a soft warning starting at 3 — "3rd retry — consider editing
    /// the spec or skipping". Defaults to 0 for fresh phases.
    #[serde(default)]
    pub(crate) retry_count: u32,
}

pub(crate) fn read_phase_meta(workspace_root: &Path, phase: u32) -> SddPhaseMeta {
    let path = workspace_root
        .join("phases")
        .join(format!("phase-{phase}-meta.json"));
    let raw = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => return SddPhaseMeta::default(),
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

pub(crate) fn write_phase_meta(
    workspace_root: &Path,
    phase: u32,
    meta: &SddPhaseMeta,
) -> Result<(), String> {
    let dir = workspace_root.join("phases");
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir phases: {e}"))?;
    let path = dir.join(format!("phase-{phase}-meta.json"));
    let body =
        serde_json::to_string_pretty(meta).map_err(|e| format!("serialize phase-meta: {e}"))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, body).map_err(|e| format!("write phase-meta tmp: {e}"))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("rename phase-meta: {e}"))?;
    Ok(())
}

/// Pull the indices of acceptance checks that ended in `Failed` for the
/// given phase, by reading `<workspace>/results/phase-N-acceptance.json`
/// (written by the verifier in phase 2). Returns empty when the file
/// is absent or all checks passed — both are valid "no failed checks
/// to surface" states.
fn read_failed_check_indices(workspace_root: &Path, phase: u32) -> Vec<u32> {
    let path = workspace_root
        .join("results")
        .join(format!("phase-{phase}-acceptance.json"));
    let raw = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let parsed: Result<crate::sdd_verify::PhaseAcceptanceFile, _> = serde_json::from_str(&raw);
    let Ok(file) = parsed else {
        return Vec::new();
    };
    file.results
        .iter()
        .enumerate()
        .filter_map(|(i, r)| {
            matches!(r.status, crate::sdd_verify::CheckStatus::Failed).then_some(i as u32)
        })
        .collect()
}

/// Read the last `tail` ActionLogEntry rows from
/// `<workspace>/phases/phase-N.log.jsonl`. Used by `derive_stage` to
/// pre-populate the failure card's "last actions" tail without forcing
/// the frontend to make a second IPC call. Failures here are silent
/// (returns empty vec) — surfacing them on disk-read errors would mask
/// the actual failure trigger.
fn read_action_log_tail(workspace_root: &Path, phase: u32, tail: usize) -> Vec<ActionLogEntry> {
    let path = workspace_root
        .join("phases")
        .join(format!("phase-{phase}.log.jsonl"));
    let raw = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let mut all: Vec<ActionLogEntry> = raw
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<ActionLogEntry>(l).ok())
        .collect();
    if all.len() > tail {
        let drop = all.len() - tail;
        all.drain(..drop);
    }
    all
}

pub(crate) fn read_workspace_meta(workspace_root: &Path) -> SddMeta {
    let raw = std::fs::read_to_string(workspace_root.join("meta.json")).unwrap_or_default();
    if raw.trim().is_empty() {
        return SddMeta::default();
    }
    serde_json::from_str(&raw).unwrap_or_default()
}

pub(crate) fn write_workspace_meta(
    workspace_root: &Path,
    meta: &SddMeta,
) -> Result<(), String> {
    let path = workspace_root.join("meta.json");
    let body = serde_json::to_string_pretty(meta).map_err(|e| format!("serialize meta: {e}"))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, body).map_err(|e| format!("write meta tmp: {e}"))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("rename meta: {e}"))?;
    Ok(())
}

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

/// Write a control file under `<workspace>/control/<name>`. Best-effort
/// — failures are reported but don't poison the stage flip (the in-memory
/// override still wins until the next process restart). Removing the
/// file is the inverse (`unset_control_file`).
fn set_control_file(root: &Path, name: &str) -> Result<(), String> {
    let dir = root.join("control");
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir control: {e}"))?;
    std::fs::write(dir.join(name), b"").map_err(|e| format!("write control/{name}: {e}"))
}

fn unset_control_files(root: &Path) {
    let _ = std::fs::remove_file(root.join("control/pause"));
    let _ = std::fs::remove_file(root.join("control/stop"));
}

#[tauri::command]
pub async fn sdd_pause(
    app: AppHandle,
    registry: State<'_, SddRegistry>,
    id: String,
) -> Result<SddWorkspace, String> {
    /* Drop the control file FIRST so the next derive_stage picks it up
     *  regardless of in-memory state. */
    if let Some(cell) = registry.workspaces.read().get(&id).cloned() {
        let root = PathBuf::from(cell.read().root.clone());
        let _ = set_control_file(&root, "pause");
        audit::append(&root, &audit::AuditEntry::new("user", "pause"));
    }
    flip_stage(&app, &registry, &id, SddStage::Paused)
}

#[tauri::command]
pub async fn sdd_resume(
    app: AppHandle,
    registry: State<'_, SddRegistry>,
    id: String,
) -> Result<SddWorkspace, String> {
    // Resume = wipe control files + recompute stage from disk.
    let cell = registry
        .workspaces
        .read()
        .get(&id)
        .cloned()
        .ok_or_else(|| format!("unknown workspace {id}"))?;
    {
        let root = PathBuf::from(cell.read().root.clone());
        unset_control_files(&root);
        audit::append(&root, &audit::AuditEntry::new("user", "resume"));
    }
    let snapshot = {
        let mut w = cell.write();
        // Force stage out of Paused so derive_stage can compute fresh.
        w.stage = SddStage::Drafting;
        rebuild_from_disk(&mut w)?;
        w.clone()
    };
    emit_changed(&app, &id);
    Ok(snapshot)
}

#[tauri::command]
pub async fn sdd_stop(
    app: AppHandle,
    registry: State<'_, SddRegistry>,
    id: String,
) -> Result<SddWorkspace, String> {
    if let Some(cell) = registry.workspaces.read().get(&id).cloned() {
        let root = PathBuf::from(cell.read().root.clone());
        let _ = set_control_file(&root, "stop");
        audit::append(&root, &audit::AuditEntry::new("user", "stop"));
    }
    flip_stage(&app, &registry, &id, SddStage::Stopped)
}

fn flip_stage(
    app: &AppHandle,
    registry: &SddRegistry,
    id: &str,
    new_stage: SddStage,
) -> Result<SddWorkspace, String> {
    let cell = registry
        .workspaces
        .read()
        .get(id)
        .cloned()
        .ok_or_else(|| format!("unknown workspace {id}"))?;
    let snapshot = {
        let mut w = cell.write();
        w.stage = new_stage;
        w.updated_at = now_ms();
        w.clone()
    };
    emit_changed(app, id);
    Ok(snapshot)
}

/// Return the canned prompt template the orchestrator should send to the
/// agent for a given stage. The orchestrator interpolates `{{workspace_root}}`
/// and `{{user_prompt}}` placeholders before sending.
#[tauri::command]
pub async fn sdd_prompt(kind: SddPromptKind) -> Result<String, String> {
    let s = match kind {
        SddPromptKind::Spec => SPEC_TEMPLATE_PROMPT,
        SddPromptKind::Plan => PLAN_TEMPLATE_PROMPT,
        SddPromptKind::Phase => PHASE_TEMPLATE_PROMPT,
        SddPromptKind::Summary => SUMMARY_TEMPLATE_PROMPT,
        SddPromptKind::Amend => AMEND_TEMPLATE_PROMPT,
    };
    Ok(s.to_string())
}

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
#[tauri::command]
pub async fn sdd_get_git_state(
    registry: State<'_, SddRegistry>,
    id: String,
) -> Result<crate::git::SddGitState, String> {
    let cell = registry
        .workspaces
        .read()
        .get(&id)
        .cloned()
        .ok_or_else(|| format!("unknown workspace {id}"))?;
    let root = PathBuf::from(cell.read().root.clone());
    let meta = read_workspace_meta(&root);
    if !meta.git_enabled {
        return Ok(crate::git::SddGitState {
            enabled: false,
            branch: None,
            on_sdd_branch: false,
            ahead: 0,
            behind: 0,
            dirty: false,
        });
    }
    let repo = meta.repo_cwd.unwrap_or_default();
    let sdd_branch = meta.sdd_branch.unwrap_or_default();
    Ok(crate::git::sdd_git_state(&repo, &sdd_branch))
}

/// Compute file-level diff stats between a phase's pre-phase and
/// post-phase commits. Used by the SddCard `Files changed` drawer.
///
/// Returns `skipped: true` when the workspace doesn't have git
/// integration, when either SHA is missing (e.g. snapshot was skipped
/// because the working tree was dirty at approve time, see phase 3),
/// OR when the phase hasn't completed yet (no `post_phase_sha`). The UI
/// renders a "git snapshot was skipped" placeholder for that branch
/// and skips the file-list render.
#[tauri::command]
pub async fn sdd_get_phase_diff(
    registry: State<'_, SddRegistry>,
    id: String,
    phase: u32,
) -> Result<crate::git::SddPhaseDiff, String> {
    let cell = registry
        .workspaces
        .read()
        .get(&id)
        .cloned()
        .ok_or_else(|| format!("unknown workspace {id}"))?;
    let root = PathBuf::from(cell.read().root.clone());
    let meta = read_workspace_meta(&root);
    let phase_meta = read_phase_meta(&root, phase);
    let repo = meta.repo_cwd.unwrap_or_default();
    let pre = phase_meta.pre_phase_sha.unwrap_or_default();
    let post = phase_meta.post_phase_sha.unwrap_or_default();
    /* Three skip-paths converge on the same `skipped: true` shape — the
     *  UI doesn't need to know whether git was off, the snapshot was
     *  skipped (dirty tree), or the phase hasn't finished yet. */
    if !meta.git_enabled || repo.is_empty() || pre.is_empty() || post.is_empty() {
        return Ok(crate::git::SddPhaseDiff {
            files: Vec::new(),
            total_insertions: 0,
            total_deletions: 0,
            skipped: true,
        });
    }
    crate::git::compute_phase_diff(&repo, &pre, &post)
}

/// Lazy fetch the unified-diff patch for a single file in a phase.
/// Called when the user clicks a row in the `Files changed` drawer to
/// expand its body. Returns the raw `git diff` output (the UI runs it
/// through the existing `renderDiffHtml` helper). Empty string when
/// pre/post SHAs are missing — the UI hides the row body in that case.
#[tauri::command]
pub async fn sdd_get_file_diff(
    registry: State<'_, SddRegistry>,
    id: String,
    phase: u32,
    path: String,
) -> Result<String, String> {
    let cell = registry
        .workspaces
        .read()
        .get(&id)
        .cloned()
        .ok_or_else(|| format!("unknown workspace {id}"))?;
    let root = PathBuf::from(cell.read().root.clone());
    let meta = read_workspace_meta(&root);
    let phase_meta = read_phase_meta(&root, phase);
    let repo = meta.repo_cwd.unwrap_or_default();
    let pre = phase_meta.pre_phase_sha.unwrap_or_default();
    let post = phase_meta.post_phase_sha.unwrap_or_default();
    if !meta.git_enabled || repo.is_empty() || pre.is_empty() || post.is_empty() {
        return Ok(String::new());
    }
    crate::git::compute_file_diff(&repo, &pre, &post, &path)
}

// ---------------------------------------------------------------------------
// Live action log — append-only JSONL of every tool_use / tool_result the
// agent emits during a phase, persisted at
// `<workspace>/phases/phase-<N>.log.jsonl` so the SddCard's live-feed
// survives an app restart. Schema mirrors `interface ActionLogEntry` in
// `apps/desktop/src/lib/state/sdd.svelte.ts`. Best-effort: failures are
// logged to stderr and never bubble — losing log lines should never break
// phase execution.
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ActionLogKind {
    ToolUse,
    ToolResult,
    AgentMessage,
    SddEvent,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ActionLogEntry {
    /// Unix-ms when this entry was produced. Frontend supplies it so
    /// the wall-clock matches the chat-stream events the user sees.
    pub ts: u64,
    /// Owning phase. Required so cross-phase log files don't bleed
    /// into the wrong feed; we use it to pick the JSONL filename.
    pub phase: u32,
    pub kind: ActionLogKind,
    /// Tool name verbatim from the CLI. None for `agent_message` /
    /// `sdd_event`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    /// One-line summary, ≤80 chars expected. The frontend builds this
    /// via the same `formatToolUse` helper the chat thread uses, so
    /// the live feed reads consistently with the trace pills.
    pub summary: String,
    /// Optional expandable detail (full bash command, full mcp args).
    /// Stored verbatim — frontend handles truncation in the lightbox.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// Lifecycle: `running` (after tool_use, before tool_result),
    /// `done` (tool_result, no error), `failed` (tool_result with
    /// `is_error: true`). None for events that don't have a lifecycle
    /// (agent_message, sdd_event).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    /// Stable id from the CLI's `tool_use_id`; lets the UI match a
    /// `running` entry to its terminal `done` / `failed` and keep one
    /// pill per call instead of two stacked rows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
}

fn action_log_path(workspace_root: &Path, phase: u32) -> PathBuf {
    workspace_root
        .join("phases")
        .join(format!("phase-{phase}.log.jsonl"))
}

/// Append a single ActionLogEntry to the phase's JSONL file. Creates the
/// file (and parent dir) on first write. Best-effort: an IO failure
/// logs to stderr and returns Ok(()) — the live feed lives in memory
/// anyway; persistence is for crash-recovery continuity, not the
/// happy path.
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

// ---------------------------------------------------------------------------
// V2 plan-as-data helpers (file-shuffling, plan.json regen).
// ---------------------------------------------------------------------------

/// Rename `<phases_dir>/<old_slug>.md` to use `new_number` as its
/// numeric prefix and update the frontmatter `phase:` value to match.
/// Returns the new slug.
fn rename_phase_file(
    phases_dir: &Path,
    old_slug: &str,
    new_number: u32,
) -> Result<String, String> {
    let suffix = old_slug.split_once('-').map(|(_, s)| s).unwrap_or(old_slug);
    let new_slug = format!("{:02}-{}", new_number, suffix);
    let old_path = phases_dir.join(format!("{old_slug}.md"));
    let new_path = phases_dir.join(format!("{new_slug}.md"));
    if old_path != new_path {
        std::fs::rename(&old_path, &new_path)
            .map_err(|e| format!("rename {} -> {}: {e}", old_path.display(), new_path.display()))?;
    }
    update_phase_number_in_frontmatter(&new_path, new_number)?;
    Ok(new_slug)
}

/// Patch a phase markdown file's frontmatter `phase:` value. Preserves
/// every other field via the same round-trip `serde_yaml` Mapping
/// trick `set_status_on` uses.
fn update_phase_number_in_frontmatter(path: &Path, new_number: u32) -> Result<(), String> {
    let raw = std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let (yaml_str, body) = split_frontmatter_raw(&raw);
    let mut map: serde_yaml::Mapping = if yaml_str.trim().is_empty() {
        serde_yaml::Mapping::new()
    } else {
        serde_yaml::from_str(&yaml_str).map_err(|e| format!("parse frontmatter: {e}"))?
    };
    map.insert(
        serde_yaml::Value::String("phase".into()),
        serde_yaml::Value::Number(serde_yaml::Number::from(new_number as u64)),
    );
    let new_yaml = serde_yaml::to_string(&map).map_err(|e| format!("yaml: {e}"))?;
    let content = format!("---\n{new_yaml}---\n\n{}\n", body.trim_end());
    let tmp = path.with_extension("md.tmp");
    std::fs::write(&tmp, content).map_err(|e| format!("write tmp: {e}"))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("rename: {e}"))?;
    Ok(())
}

/// Like `set_status_on` but also writes additional frontmatter keys
/// in the same atomic round-trip. Used by `sdd_skip_phase` to set
/// both `status: skipped` and `skip_reason: "<text>"` together.
fn set_status_with_extras(
    path: &Path,
    new_status: &str,
    extras: Vec<(String, String)>,
) -> Result<(), String> {
    let raw = std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let (yaml_str, body) = split_frontmatter_raw(&raw);
    let mut map: serde_yaml::Mapping = if yaml_str.trim().is_empty() {
        serde_yaml::Mapping::new()
    } else {
        serde_yaml::from_str(&yaml_str).map_err(|e| format!("parse frontmatter: {e}"))?
    };
    map.insert(
        serde_yaml::Value::String("status".into()),
        serde_yaml::Value::String(new_status.into()),
    );
    map.insert(
        serde_yaml::Value::String("updated".into()),
        serde_yaml::Value::String(format_iso(now_ms())),
    );
    for (k, v) in extras {
        map.insert(
            serde_yaml::Value::String(k),
            serde_yaml::Value::String(v),
        );
    }
    let new_yaml = serde_yaml::to_string(&map).map_err(|e| format!("yaml: {e}"))?;
    let content = format!("---\n{new_yaml}---\n\n{}\n", body.trim_end());
    let tmp = path.with_extension("md.tmp");
    std::fs::write(&tmp, content).map_err(|e| format!("write tmp: {e}"))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("rename: {e}"))?;
    Ok(())
}

/// Re-derive `<workspace>/plan.json` from the phase markdown files.
/// Preserves any existing `acceptance` arrays (matched by phase
/// number) so a regen doesn't blow away verifier criteria the agent
/// wrote previously.
fn rebuild_plan_json(root: &Path) -> Result<(), String> {
    let phases_dir = root.join("phases");
    let mut entries: Vec<(u32, String, String, Vec<u32>)> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&phases_dir) {
        for entry in rd.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("md") {
                continue;
            }
            let stem = match path.file_stem().and_then(|s| s.to_str()) {
                Some(s) => s.to_string(),
                None => continue,
            };
            let raw = std::fs::read_to_string(&path).unwrap_or_default();
            let (fm, _body) = parse_frontmatter(&raw);
            let number = phase_number_from(&stem, &fm);
            if number == 0 {
                continue;
            }
            let title = fm.title.clone().unwrap_or_default();
            let depends = fm.depends_on.clone();
            entries.push((number, stem, title, depends));
        }
    }
    entries.sort_by_key(|(n, _, _, _)| *n);
    /* Carry over acceptance from the existing plan.json so a regen
     * doesn't drop the verifier criteria. */
    let prior = read_plan_json(root);
    let prior_by_num: HashMap<u32, Vec<SddPhaseAcceptance>> = prior
        .map(|p| p.phases.into_iter().map(|x| (x.number, x.acceptance)).collect())
        .unwrap_or_default();
    let phases: Vec<SddPlanPhase> = entries
        .into_iter()
        .map(|(number, slug, title, depends_on)| SddPlanPhase {
            number,
            slug,
            title,
            depends_on,
            complexity: None,
            acceptance: prior_by_num.get(&number).cloned().unwrap_or_default(),
        })
        .collect();
    write_plan_json(root, &SddPlanFile { version: 1, phases })
}

/// Lower-case + spaces-to-dashes + strip non-alphanumeric. Used to
/// derive a filename slug from a human title (`"Foundation Phase"` →
/// `"foundation-phase"`).
fn slugify(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last_dash = false;
    for ch in s.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash && !out.is_empty() {
            out.push('-');
            last_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        "phase".into()
    } else {
        out
    }
}

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

fn emit_changed(app: &AppHandle, id: &str) {
    // Per-workspace targeted event + a broad "something changed" event
    // for any "list of all workspaces" UI we add later (none right now).
    let _ = app.emit(&format!("sdd:changed:{id}"), &id);
    let _ = app.emit("sdd:changed", &id);
}

/// Boot the filesystem watcher on first workspace creation. Spawns one
/// background OS thread that drains `notify` events, debounces them per
/// workspace, rebuilds the workspace from disk, and emits
/// `sdd:changed:<id>`.
///
/// Watching the BASE dir recursively (vs per-workspace) lets us handle
/// "new workspace created" + "old workspace removed" without re-arming
/// the watcher each time. Cheap — base dir is single-purpose so there
/// are no other write sources to filter against.
///
/// Debounce: 250 ms per workspace. The agent often writes a file then
/// updates frontmatter immediately after; without debouncing we'd
/// rebuild twice in quick succession and the UI would flicker.
fn ensure_watcher(
    app: &AppHandle,
    workspaces: SharedWorkspaces,
    watcher_slot: &parking_lot::Mutex<Option<notify::RecommendedWatcher>>,
    base_dir: &Path,
) -> Result<(), String> {
    use notify::{EventKind, RecursiveMode, Watcher};
    let mut guard = watcher_slot.lock();
    if guard.is_some() {
        return Ok(());
    }
    let (tx, rx) = std::sync::mpsc::channel::<notify::Result<notify::Event>>();
    let mut w = notify::recommended_watcher(tx).map_err(|e| format!("watcher init: {e}"))?;
    w.watch(base_dir, RecursiveMode::Recursive)
        .map_err(|e| format!("watch {}: {e}", base_dir.display()))?;
    *guard = Some(w);
    drop(guard);

    let workspaces2 = Arc::clone(&workspaces);
    let app2 = app.clone();
    std::thread::spawn(move || {
        use std::time::{Duration, Instant};
        let debounce = Duration::from_millis(250);
        let mut last_emit: HashMap<String, Instant> = HashMap::new();
        while let Ok(res) = rx.recv() {
            let Ok(evt) = res else { continue };
            /* Filter to mutation events only — `notify` also emits
             *  "Any" / metadata-only events on some platforms that
             *  don't carry useful info. We only care about content
             *  changes (file added / modified / removed). */
            if !matches!(
                evt.kind,
                EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
            ) {
                continue;
            }
            /* Ignore noise:
             *  - `.tmp` writes from our atomic rename pattern (we'd see
             *    both the tmp create AND the rename-target create —
             *    only the latter is real "content changed")
             *  - `meta.json` side-car (session_id / user_prompt — doesn't
             *    affect stage, would cause spurious rebuilds on init)
             *  - anything inside `control/` (pause/stop signal files;
             *    those are HANDLED by the agent reading them, not by
             *    the stage-derivation path). */
            let ignore = evt.paths.iter().any(|p| {
                if p.extension().is_some_and(|e| e == "tmp") { return true; }
                if p.file_name().is_some_and(|n| n == "meta.json") { return true; }
                p.components().any(|c| c.as_os_str() == "control")
            });
            if ignore { continue; }

            /* For each affected path, find the owning workspace
             *  (workspace whose root is an ancestor of the path). */
            let mut hit_workspaces: Vec<String> = Vec::new();
            {
                let map = workspaces2.read();
                for path in &evt.paths {
                    for (id, cell) in map.iter() {
                        let root = PathBuf::from(cell.read().root.clone());
                        if path.starts_with(&root) {
                            if !hit_workspaces.contains(id) {
                                hit_workspaces.push(id.clone());
                            }
                            break;
                        }
                    }
                }
            }
            for id in hit_workspaces {
                let now = Instant::now();
                if let Some(&last) = last_emit.get(&id) {
                    if now.duration_since(last) < debounce {
                        continue;
                    }
                }
                last_emit.insert(id.clone(), now);
                /* Rebuild from disk under write lock; drop the lock
                 *  before emitting the event to keep the lock window
                 *  short. */
                {
                    let map = workspaces2.read();
                    let Some(cell) = map.get(&id).cloned() else { continue };
                    drop(map);
                    let mut w = cell.write();
                    let _ = rebuild_from_disk(&mut w);
                }
                emit_changed(&app2, &id);
            }
        }
    });
    Ok(())
}

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

pub mod audit {
    use super::*;

    /// Single row of the audit log. `before` / `after` are free-form
    /// JSON snapshots of the relevant fields for the action — e.g. for
    /// `advance_phase`, `before = {"status": "pending_approval"}`,
    /// `after = {"status": "running"}`. Kept compact on purpose; the
    /// goal is "did the user override our refusal?", not full diffs.
    #[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
    pub struct AuditEntry {
        pub ts: u64,
        /// One of: `agent`, `user`, `system`. Free-form to allow future
        /// sources (e.g. `cli`) without breaking the schema.
        pub source: String,
        /// Mutation action verb. Must match one of the values in the
        /// SDD phase 6 spec (`advance_phase`, `retry_phase`, …) so the
        /// UI's filter dropdown stays predictable.
        pub action: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub phase: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub reason: Option<String>,
        #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
        pub before: serde_json::Value,
        #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
        pub after: serde_json::Value,
    }

    impl AuditEntry {
        pub fn new(source: &str, action: &str) -> Self {
            Self {
                ts: now_ms(),
                source: source.into(),
                action: action.into(),
                phase: None,
                reason: None,
                before: serde_json::Value::Null,
                after: serde_json::Value::Null,
            }
        }
        pub fn with_phase(mut self, phase: u32) -> Self {
            self.phase = Some(phase);
            self
        }
        pub fn with_reason(mut self, reason: impl Into<String>) -> Self {
            let r = reason.into();
            if !r.trim().is_empty() {
                self.reason = Some(r);
            }
            self
        }
        pub fn with_before(mut self, v: serde_json::Value) -> Self {
            self.before = v;
            self
        }
        pub fn with_after(mut self, v: serde_json::Value) -> Self {
            self.after = v;
            self
        }
    }

    pub fn audit_log_path(workspace_root: &Path) -> PathBuf {
        workspace_root.join("audit-log.jsonl")
    }

    /// Append one entry to the workspace audit log. Best-effort —
    /// failures log to stderr but never bubble. Mirrors the
    /// action-log append pattern: POSIX append-mode + a single
    /// `writeln!` is atomic for lines smaller than `PIPE_BUF`.
    pub fn append(workspace_root: &Path, entry: &AuditEntry) {
        let path = audit_log_path(workspace_root);
        if let Some(parent) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                eprintln!("[sdd:audit] mkdir {}: {e}", parent.display());
                return;
            }
        }
        let line = match serde_json::to_string(entry) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[sdd:audit] serialize: {e}");
                return;
            }
        };
        use std::io::Write;
        match std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        {
            Ok(mut f) => {
                let _ = writeln!(f, "{line}");
            }
            Err(e) => eprintln!("[sdd:audit] open {}: {e}", path.display()),
        }
    }

    /// Read every audit entry, oldest-first. Lines that fail to parse
    /// are silently skipped (forward-compat). Missing file → empty
    /// vector.
    pub fn read_all(workspace_root: &Path) -> Vec<AuditEntry> {
        let path = audit_log_path(workspace_root);
        let raw = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        raw.lines()
            .filter(|l| !l.trim().is_empty())
            .filter_map(|l| serde_json::from_str::<AuditEntry>(l).ok())
            .collect()
    }

    /// Validate a mutation reason. Phase 6's contract: every mutating
    /// MCP tool requires `reason` ≥ 5 chars after trim, so the audit
    /// trail always carries a "why". Empty / whitespace-only / "ok" /
    /// "yes" all reject. Returns the trimmed reason on success.
    pub fn validate_reason(reason: &str) -> Result<String, String> {
        let trimmed = reason.trim();
        if trimmed.len() < 5 {
            return Err("reason too short — explain why you're advancing".into());
        }
        Ok(trimmed.to_string())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn td() -> PathBuf {
            use std::sync::atomic::{AtomicU64, Ordering};
            static C: AtomicU64 = AtomicU64::new(0);
            let n = C.fetch_add(1, Ordering::Relaxed);
            let pid = std::process::id();
            let dir = std::env::temp_dir().join(format!("woom-sdd-audit-{pid}-{n}"));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            dir
        }

        #[test]
        fn append_then_read_round_trip() {
            let dir = td();
            let e1 = AuditEntry::new("user", "advance_phase")
                .with_phase(1)
                .with_reason("approved manually")
                .with_before(serde_json::json!({"status":"pending_approval"}))
                .with_after(serde_json::json!({"status":"running"}));
            let e2 = AuditEntry::new("agent", "retry_phase")
                .with_phase(2)
                .with_reason("verifier failed once, trying again");
            append(&dir, &e1);
            append(&dir, &e2);
            let got = read_all(&dir);
            assert_eq!(got.len(), 2);
            assert_eq!(got[0].action, "advance_phase");
            assert_eq!(got[0].source, "user");
            assert_eq!(got[0].phase, Some(1));
            assert_eq!(got[1].action, "retry_phase");
            assert_eq!(got[1].source, "agent");
        }

        #[test]
        fn read_missing_returns_empty() {
            let dir = td();
            assert!(read_all(&dir).is_empty());
        }

        #[test]
        fn append_creates_workspace_dir_if_missing() {
            // The workspace dir itself exists (td creates it) but the
            // append path must work even when called on a fresh
            // workspace before any other file landed.
            let dir = td();
            let e = AuditEntry::new("system", "boot");
            append(&dir, &e);
            assert!(audit_log_path(&dir).exists());
        }

        #[test]
        fn read_skips_corrupted_lines() {
            let dir = td();
            let good = AuditEntry::new("user", "pause");
            let body = format!(
                "{}\n{}\n{}\n",
                serde_json::to_string(&good).unwrap(),
                "{ not valid json …",
                serde_json::to_string(&good).unwrap(),
            );
            std::fs::write(audit_log_path(&dir), body).unwrap();
            let got = read_all(&dir);
            assert_eq!(got.len(), 2, "good lines kept, bad line dropped");
        }

        #[test]
        fn validate_reason_rejects_short() {
            assert!(validate_reason("").is_err());
            assert!(validate_reason("   ").is_err());
            assert!(validate_reason("ok").is_err());
            assert!(validate_reason("yes!").is_err());
        }

        #[test]
        fn validate_reason_accepts_long_enough() {
            assert_eq!(
                validate_reason("approved").unwrap(),
                "approved".to_string()
            );
            assert_eq!(
                validate_reason("  approved manually  ").unwrap(),
                "approved manually".to_string()
            );
        }

        #[test]
        fn entry_skip_serializes_nulls() {
            // `before` / `after` default to Null and skip on serialize
            // — keeps the JSONL compact for actions that don't need a
            // payload (pause / resume).
            let e = AuditEntry::new("user", "pause");
            let s = serde_json::to_string(&e).unwrap();
            assert!(!s.contains("\"before\""), "before should be omitted");
            assert!(!s.contains("\"after\""), "after should be omitted");
            assert!(!s.contains("\"phase\""), "phase should be omitted");
            assert!(!s.contains("\"reason\""), "reason should be omitted");
        }
    }
}

// ---------------------------------------------------------------------------
// MCP-handler helpers — shared validation between the woom-app sidecar's
// tool stubs and the Tauri-side audit-aware variants. Lives in its own
// module so the verification command `cargo test sdd::mcp_handlers` has
// a clear target. Pure functions, no IO.
// ---------------------------------------------------------------------------

pub mod mcp_handlers {
    use super::audit::validate_reason;

    /// Mutation actions accepted by the audit log + MCP tool surface.
    /// Listed in the same order as the phase 6 spec for grep-ability.
    pub const MUTATING_ACTIONS: &[&str] = &[
        "advance_phase",
        "retry_phase",
        "skip_phase",
        "approve_spec",
        "approve_plan",
        "rollback_phase",
        "pause",
        "resume",
        "stop",
        "discard",
        "edit_body",
        "insert_phase",
        "delete_phase",
        "manual_check_marked",
    ];

    /// Read-only tools that DON'T require a `reason` and never emit
    /// audit entries. Kept here so the sidecar + frontend can agree on
    /// the split without duplicating the list.
    #[allow(dead_code)] // Used by tests + future frontend feature-detection.
    pub const READ_ONLY_TOOLS: &[&str] = &[
        "sdd_get",
        "sdd_list_phases",
        "sdd_get_phase",
        "sdd_get_action_log",
        "sdd_get_results",
    ];

    /// Validate a mutating MCP request. Today the only check is the
    /// shared reason-length gate, but the function gives us a single
    /// place to grow into per-action validation (e.g. refusing
    /// `advance_phase` when the previous phase isn't `done`).
    pub fn validate_mutation(action: &str, reason: &str) -> Result<String, String> {
        if !MUTATING_ACTIONS.contains(&action) {
            return Err(format!(
                "unknown mutating action `{action}` — see MUTATING_ACTIONS"
            ));
        }
        validate_reason(reason)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn validate_mutation_rejects_short_reason() {
            let err = validate_mutation("advance_phase", "ok").unwrap_err();
            assert!(err.contains("reason too short"));
        }

        #[test]
        fn validate_mutation_accepts_known_action_with_long_reason() {
            let r = validate_mutation("retry_phase", "verifier flaked, retrying").unwrap();
            assert_eq!(r, "verifier flaked, retrying");
        }

        #[test]
        fn validate_mutation_rejects_unknown_action() {
            let err = validate_mutation("delete_universe", "burn it all down").unwrap_err();
            assert!(err.contains("unknown mutating action"));
        }

        #[test]
        fn read_only_tools_disjoint_from_mutating_actions() {
            // Sanity: a read-only tool name should never appear in the
            // mutating-actions list (the two surfaces are explicitly
            // separated in the phase 6 spec).
            for r in READ_ONLY_TOOLS {
                let stripped = r.strip_prefix("sdd_").unwrap_or(r);
                assert!(
                    !MUTATING_ACTIONS.contains(&stripped),
                    "{stripped} is in both lists"
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tauri commands — audit log read/append + a tiny convenience wrapper that
// performs reason-validation server-side. Frontend stream-parser calls
// `sdd_audit_append` after every successful mutation; the SddCard's
// audit overlay calls `sdd_audit_read`.
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn sdd_audit_append(
    registry: State<'_, SddRegistry>,
    id: String,
    entry: audit::AuditEntry,
) -> Result<(), String> {
    let cell = registry
        .workspaces
        .read()
        .get(&id)
        .cloned()
        .ok_or_else(|| format!("unknown workspace {id}"))?;
    let root = PathBuf::from(cell.read().root.clone());
    audit::append(&root, &entry);
    Ok(())
}

#[tauri::command]
pub async fn sdd_audit_read(
    registry: State<'_, SddRegistry>,
    id: String,
) -> Result<Vec<audit::AuditEntry>, String> {
    let cell = registry
        .workspaces
        .read()
        .get(&id)
        .cloned()
        .ok_or_else(|| format!("unknown workspace {id}"))?;
    let root = PathBuf::from(cell.read().root.clone());
    Ok(audit::read_all(&root))
}

/// Server-side reason gate the agent-facing MCP tool stubs share via
/// the `mcp_handlers::validate_mutation` helper. Exposed as a Tauri
/// command so the frontend stream parser can reject malformed agent
/// calls before invoking the underlying mutation.
#[tauri::command]
pub async fn sdd_validate_mutation(action: String, reason: String) -> Result<String, String> {
    mcp_handlers::validate_mutation(&action, &reason)
}

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
        }
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
