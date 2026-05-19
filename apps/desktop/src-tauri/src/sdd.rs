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
    /// Phase `phase` is being executed by the agent.
    PhaseRunning { phase: u32 },
    /// Phase `phase` finished; about to advance or awaiting approval.
    PhaseDone { phase: u32 },
    /// All phases done; final summary card up.
    Complete,
    /// User pressed pause — orchestrator stops scheduling new phases.
    Paused,
    /// User pressed stop — workspace is dead, no more execution.
    Stopped,
    /// Unrecoverable error.
    Failed { reason: String },
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
        return SddStage::Failed {
            reason: format!("Phase {} ({}) failed — see result file", p.number, p.title),
        };
    }
    let running_phase = w.phases.iter().find(|p| p.status == "running");
    if let Some(p) = running_phase {
        return SddStage::PhaseRunning { phase: p.number };
    }
    let last_done = w.phases.iter().rev().find(|p| p.status == "done");
    let all_done = !w.phases.is_empty() && w.phases.iter().all(|p| p.status == "done");
    if all_done {
        return SddStage::Complete;
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
    };
    let cell = Arc::new(RwLock::new(ws.clone()));
    registry.workspaces.write().insert(id.clone(), cell);
    /* Persist side-car metadata so the workspace can be restored after
     *  app restart with full context (session binding, original ask). */
    let meta = SddMeta {
        user_prompt: Some(ws.user_prompt.clone()),
        session_id: ws.session_id.clone(),
        created_at: Some(ws.created_at),
    };
    if let Ok(meta_json) = serde_json::to_string_pretty(&meta) {
        let _ = std::fs::write(root.join("meta.json"), meta_json);
    }
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
struct SddMeta {
    user_prompt: Option<String>,
    session_id: Option<String>,
    created_at: Option<u64>,
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
    let path: PathBuf = {
        let w = cell.read();
        w.phases
            .iter()
            .find(|p| p.number == phase)
            .map(|p| PathBuf::from(&p.path))
            .ok_or_else(|| format!("phase {phase} not found"))?
    };
    reset_phase_status(&path)?;
    let snapshot = {
        let mut w = cell.write();
        rebuild_from_disk(&mut w)?;
        w.clone()
    };
    emit_changed(&app, &id);
    Ok(snapshot)
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
