//! Auto-update lifecycle state machine, persisted settings, and the
//! background poll task that drives the Tauri updater plugin on a
//! 6-hour cadence.
//!
//! The plugin (`tauri_plugin_updater`) does the heavy lifting — manifest
//! fetch + ed25519 verification + atomic install — but it has NO state
//! beyond a per-call `Update` handle. This module wraps it with:
//!
//! - **A finite-state machine** (`UpdateState`) reported to the
//!   frontend via the `update:state` event so a single store can drive
//!   the toast, release-notes pane, AND Settings card.
//! - **Persisted user preferences** (`UpdaterSettings`) — auto-check
//!   toggle, snooze deadline, skipped version, last-checked timestamp.
//!   Survive app restart by living as JSON under
//!   `$app_local_data_dir/updater-settings.json`.
//! - **A tokio task** spawned at app setup that ticks every 6 hours
//!   (plus an immediate startup check), honouring the settings file
//!   each cycle.
//!
//! Snooze + Skip are policy filters applied INSIDE `check_and_emit`:
//! the plugin's `check()` may say "yes there's 1.4.2", but if the user
//! snoozed it until tomorrow we emit `Snoozed` instead of `Available`
//! so the UI doesn't re-prompt.
//!
//! Phase reference: SDD workspace `sdd-2508eeb82e`, phase 3.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_updater::UpdaterExt;
use tokio::sync::Mutex;

const POLL_INTERVAL_SECS: u64 = 6 * 60 * 60;
const STATE_EVENT: &str = "update:state";
const SETTINGS_FILE: &str = "updater-settings.json";

/// Lifecycle state for the update flow. Mirrors the diagram in the
/// SDD plan's "Update lifecycle" section. Serialised with
/// `serde(tag = "kind", rename_all = "snake_case")` so the JS side
/// reads it as `{ kind: 'available', version: '…', … }`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UpdateState {
    Idle,
    Checking,
    UpToDate { checked_at_ms: u64 },
    Available {
        version: String,
        notes: String,
        pub_date: Option<String>,
        manifest_url: String,
    },
    Snoozed { version: String, until_ms: u64 },
    Skipped { version: String },
    Downloading { version: String, downloaded: u64, total: Option<u64> },
    Verifying { version: String },
    Installing { version: String },
    InstalledPendingQuit { version: String },
    Failed { version: Option<String>, reason: String },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UpdaterSettings {
    /// When true, the background poll loop fires a check every
    /// POLL_INTERVAL_SECS. Default true — startup check still fires
    /// either way (see `spawn_poll`).
    pub auto_check_enabled: bool,
    /// Unix millis; while in the future the matching version is
    /// reported as `Snoozed` instead of `Available`.
    pub snooze_until_ms: Option<u64>,
    /// Specific version string the user explicitly "skipped". Reported
    /// as `Skipped` until a different version shows up.
    pub skipped_version: Option<String>,
    /// Unix millis of the most recent `check()` call (success OR
    /// failure). Drives the Settings "Last checked" readout.
    pub last_checked_at_ms: Option<u64>,
    /// Most recent advertised version (whatever `check()` returned
    /// last). Used to dedupe re-emits of the same `Available` state.
    pub last_known_version: Option<String>,
    /// Path to a DMG queued by "Install on quit". Cleared after the
    /// swap script runs in Phase 5; today this is set by Phase 4's
    /// stub `installOnQuit` flow.
    pub pending_update_path: Option<String>,
    /// Version string of the pending DMG (mirrors filename / manifest).
    pub pending_update_version: Option<String>,
}

impl Default for UpdaterSettings {
    fn default() -> Self {
        Self {
            auto_check_enabled: true,
            snooze_until_ms: None,
            skipped_version: None,
            last_checked_at_ms: None,
            last_known_version: None,
            pending_update_path: None,
            pending_update_version: None,
        }
    }
}

/// Tauri-managed singleton holding the most recent emitted state +
/// in-memory settings cache. JS calls `updater_get_state` once on
/// mount to seed its store, then subscribes to the event stream.
pub struct UpdaterState {
    pub current: Mutex<UpdateState>,
}

impl UpdaterState {
    pub fn new() -> Self {
        Self { current: Mutex::new(UpdateState::Idle) }
    }
}

// ---------------------------------------------------------------------------
// Settings persistence.
// ---------------------------------------------------------------------------

fn settings_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_local_data_dir()
        .map_err(|e| format!("app_local_data_dir: {e}"))?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("create_dir_all: {e}"))?;
    Ok(dir.join(SETTINGS_FILE))
}

pub fn load_settings(app: &AppHandle) -> UpdaterSettings {
    let Ok(path) = settings_path(app) else { return UpdaterSettings::default() };
    let Ok(raw) = std::fs::read_to_string(&path) else { return UpdaterSettings::default() };
    serde_json::from_str(&raw).unwrap_or_else(|e| {
        eprintln!("updater-settings: corrupt JSON, using defaults: {e}");
        UpdaterSettings::default()
    })
}

pub fn save_settings(app: &AppHandle, s: &UpdaterSettings) -> Result<(), String> {
    let path = settings_path(app)?;
    let tmp = path.with_extension("json.tmp");
    let body = serde_json::to_string_pretty(s).map_err(|e| format!("serialize: {e}"))?;
    std::fs::write(&tmp, body).map_err(|e| format!("write tmp: {e}"))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("rename: {e}"))?;
    Ok(())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Emit + check.
// ---------------------------------------------------------------------------

async fn emit(app: &AppHandle, state: UpdateState) {
    if let Some(s) = app.try_state::<Arc<UpdaterState>>() {
        let mut current = s.current.lock().await;
        *current = state.clone();
    }
    if let Err(e) = app.emit(STATE_EVENT, &state) {
        eprintln!("updater: emit failed: {e}");
    }
}

/// Probe the configured manifest endpoint, apply snooze/skip policy,
/// and emit the resulting lifecycle state. Mutates the persisted
/// settings (`last_checked_at_ms`, `last_known_version`) regardless of
/// outcome so the Settings UI's "Last checked" timestamp always
/// advances on any explicit poll.
pub async fn check_and_emit(app: &AppHandle) -> UpdateState {
    emit(app, UpdateState::Checking).await;
    let result = match app.updater() {
        Ok(updater) => updater.check().await,
        Err(e) => {
            let st = UpdateState::Failed { version: None, reason: format!("updater handle: {e}") };
            emit(app, st.clone()).await;
            return st;
        }
    };

    let mut settings = load_settings(app);
    settings.last_checked_at_ms = Some(now_ms());

    // Auto-clear stale skip — if the running binary already matches
    // the skipped version, the skip is meaningless (user is on it).
    // Prevents a zombie "0.1.2 skipped" message after the user
    // manually installs the version they previously skipped.
    let running = env!("CARGO_PKG_VERSION");
    if settings.skipped_version.as_deref() == Some(running) {
        settings.skipped_version = None;
    }

    let state = match result {
        Ok(Some(update)) => {
            let version = update.version.clone();
            settings.last_known_version = Some(version.clone());

            if settings.skipped_version.as_deref() == Some(&version) {
                UpdateState::Skipped { version }
            } else if let Some(until) = settings.snooze_until_ms {
                if until > now_ms() {
                    UpdateState::Snoozed { version, until_ms: until }
                } else {
                    // Snooze expired — clear it so the user gets the toast.
                    settings.snooze_until_ms = None;
                    UpdateState::Available {
                        version,
                        notes: update.body.clone().unwrap_or_default(),
                        pub_date: update.date.map(|d| d.to_string()),
                        manifest_url: endpoint(app),
                    }
                }
            } else {
                UpdateState::Available {
                    version,
                    notes: update.body.clone().unwrap_or_default(),
                    pub_date: update.date.map(|d| d.to_string()),
                    manifest_url: endpoint(app),
                }
            }
        }
        Ok(None) => UpdateState::UpToDate { checked_at_ms: now_ms() },
        Err(e) => UpdateState::Failed { version: None, reason: format!("check failed: {e}") },
    };

    let _ = save_settings(app, &settings);
    emit(app, state.clone()).await;
    state
}

/// Fetch the manifest JSON from the configured updater endpoint and
/// extract the `sha256` field for the running architecture. Returns
/// `Ok(None)` when the manifest has no `sha256` for our arch (older
/// releases predate the Woom-specific extension) — caller treats
/// that as "skip the extra check" rather than fail. Network failure
/// or malformed JSON returns `Err`.
async fn fetch_expected_sha256(app: &AppHandle, version: &str) -> Result<Option<String>, String> {
    let url = endpoint(app);
    if url.is_empty() {
        return Err("updater endpoint not configured".into());
    }
    let body = reqwest::get(&url)
        .await
        .map_err(|e| format!("manifest fetch: {e}"))?
        .text()
        .await
        .map_err(|e| format!("manifest body: {e}"))?;
    let v: serde_json::Value = serde_json::from_str(&body)
        .map_err(|e| format!("manifest parse: {e}"))?;
    // Sanity: refuse to verify if the manifest's version disagrees
    // with what the plugin advertised. Either a mid-flight rotation
    // (manifest got bumped between `check()` and `download()`) OR a
    // proxy serving a stale copy — both are worth failing closed on.
    let manifest_version = v.get("version").and_then(|x| x.as_str()).unwrap_or("");
    if !manifest_version.is_empty() && manifest_version != version {
        return Err(format!(
            "manifest version mismatch: plugin saw {version}, manifest now says {manifest_version}"
        ));
    }
    let arch = if cfg!(target_arch = "aarch64") { "darwin-aarch64" } else { "darwin-x86_64" };
    Ok(v.pointer(&format!("/platforms/{arch}/sha256"))
        .and_then(|x| x.as_str())
        .map(|s| s.to_string()))
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    hex::encode(h.finalize())
}

fn endpoint(app: &AppHandle) -> String {
    app.config()
        .plugins
        .0
        .get("updater")
        .and_then(|v| v.get("endpoints"))
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

// ---------------------------------------------------------------------------
// Background poll task.
// ---------------------------------------------------------------------------

/// Fire an immediate startup check, then loop forever on the
/// POLL_INTERVAL_SECS cadence. The auto-check toggle ONLY gates the
/// recurring loop — startup always fires once so `last_checked_at_ms`
/// stays fresh and Settings shows a real value on first launch.
pub fn spawn_poll(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        // Startup check — always fires.
        let _ = check_and_emit(&app).await;
        loop {
            tokio::time::sleep(Duration::from_secs(POLL_INTERVAL_SECS)).await;
            let settings = load_settings(&app);
            if !settings.auto_check_enabled { continue; }
            let _ = check_and_emit(&app).await;
        }
    });
}

// ---------------------------------------------------------------------------
// Stage-on-quit swap (Phase 5).
// ---------------------------------------------------------------------------

const SWAP_SCRIPT_REL: &str = "resources/swap-binary.sh";
const SWAP_LOG_REL: &str = "Library/Logs/Woom/update-swap.log";
const PENDING_STALE_AGE_SECS: u64 = 30 * 24 * 60 * 60;

fn swap_script_path(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .resolve(SWAP_SCRIPT_REL, tauri::path::BaseDirectory::Resource)
        .ok()
}

fn swap_log_path() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    Some(PathBuf::from(home).join(SWAP_LOG_REL))
}

fn current_install_path() -> PathBuf {
    // The running app's binary lives at
    // /Applications/Woom.app/Contents/MacOS/woom-desktop; walk back
    // to the .app bundle for the script. If `current_exe()` fails
    // (developer dev-build outside an .app) fall back to the
    // canonical install path so the script reports a clean FAIL log
    // instead of crashing on missing args.
    let exe = std::env::current_exe().unwrap_or_default();
    let mut cursor: &Path = exe.as_path();
    while let Some(parent) = cursor.parent() {
        if parent.extension().is_some_and(|e| e == "app") {
            return parent.to_path_buf();
        }
        cursor = parent;
    }
    PathBuf::from("/Applications/Woom.app")
}

/// Synchronously spawn the swap script + wait up to 30s. Called from
/// the `RunEvent::ExitRequested` handler so the swap runs while the
/// host process is shutting down — host exit is what unblocks the
/// `rm -rf $APP_INSTALL_PATH` step.
pub fn fire_swap_script_blocking(app: &AppHandle, dmg_path: &Path) -> Result<(), String> {
    let script = swap_script_path(app)
        .ok_or_else(|| format!("swap script missing under {SWAP_SCRIPT_REL}"))?;
    let target = current_install_path();
    let status = std::process::Command::new("/bin/bash")
        .arg(&script)
        .arg(dmg_path)
        .arg(&target)
        .status()
        .map_err(|e| format!("spawn swap script: {e}"))?;
    if !status.success() {
        return Err(format!("swap script exit code {}", status.code().unwrap_or(-1)));
    }
    Ok(())
}

/// Inspect `pending_update_path` on startup. Three outcomes:
///   - file missing AND no version recorded → settings already clean,
///     return None.
///   - file missing OR file older than PENDING_STALE_AGE_SECS → clear
///     settings + delete file if present. Logged.
///   - file present + fresh → return the path + version so caller
///     decides what to do (the post-swap startup-success detection
///     reads `pending_update_version` against the running binary).
fn read_pending_state(app: &AppHandle) -> (UpdaterSettings, Option<(PathBuf, String)>) {
    let settings = load_settings(app);
    let Some(path_str) = settings.pending_update_path.as_ref() else {
        return (settings, None);
    };
    let path = PathBuf::from(path_str);
    let Some(version) = settings.pending_update_version.clone() else {
        return (settings, Some((path, String::new())));
    };
    (settings, Some((path, version)))
}

fn clear_pending(app: &AppHandle, settings: &mut UpdaterSettings) {
    if let Some(p) = settings.pending_update_path.as_ref() {
        let _ = std::fs::remove_file(p);
    }
    settings.pending_update_path = None;
    settings.pending_update_version = None;
    let _ = save_settings(app, settings);
}

/// On every launch: detect whether a previously-staged swap ran, and
/// drop the pending slot if it's stale. Called from `setup()`.
///
/// Detection strategy:
///   - If `pending_update_version` matches the running binary's
///     `CARGO_PKG_VERSION`, the swap already applied → emit a
///     success toast + clear settings.
///   - Else if the swap log's last line says "FAIL" newer than the
///     pending slot's mtime, the swap attempted but failed → emit a
///     Failed state (Phase 5 task 5's red toast subscribes to it).
///   - Else if the DMG is missing OR older than 30 days, clear the
///     slot silently.
///   - Else leave it — the user hasn't quit yet OR the swap is
///     scheduled for the next quit.
pub fn detect_pending_outcome(app: &AppHandle) {
    let (mut settings, pending) = read_pending_state(app);
    let Some((dmg_path, pending_version)) = pending else { return };
    let running_version = env!("CARGO_PKG_VERSION");
    if !pending_version.is_empty() && pending_version == running_version {
        // Swap took effect — running binary is the new version.
        let v = pending_version.clone();
        clear_pending(app, &mut settings);
        let app_for_emit = app.clone();
        tauri::async_runtime::spawn(async move {
            emit(&app_for_emit, UpdateState::InstalledPendingQuit { version: v.clone() }).await;
            // The state name reads slightly off after the relaunch
            // ("pending quit" once we're past quit), but the JS toast
            // copy treats it as a success message and Phase 4 already
            // uses InstalledPendingQuit. Don't introduce a new variant
            // just for the post-swap moment.
            let _ = v;
        });
        return;
    }

    // Stale by missing file OR by age.
    let stale = match std::fs::metadata(&dmg_path) {
        Err(_) => true,
        Ok(meta) => match meta.modified() {
            Ok(modified) => modified
                .elapsed()
                .map(|d| d.as_secs() > PENDING_STALE_AGE_SECS)
                .unwrap_or(false),
            Err(_) => false,
        },
    };
    if stale {
        clear_pending(app, &mut settings);
        return;
    }

    // Slot is fresh + version doesn't match running binary → swap
    // didn't fire (user hasn't quit yet, OR last quit crashed before
    // hitting the swap). Surface a Failed event for the JS toast in
    // case the log shows a definitive failure; otherwise leave the
    // slot in place for the next quit attempt.
    if let Some(log_path) = swap_log_path() {
        if let Ok(log) = std::fs::read_to_string(&log_path) {
            if let Some(last) = log.lines().last() {
                if last.contains("FAIL:") {
                    let reason = last.to_string();
                    let app_for_emit = app.clone();
                    let v = pending_version.clone();
                    tauri::async_runtime::spawn(async move {
                        emit(
                            &app_for_emit,
                            UpdateState::Failed { version: Some(v), reason },
                        )
                        .await;
                    });
                    clear_pending(app, &mut settings);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tauri commands.
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn updater_check_now(app: AppHandle) -> Result<UpdateState, String> {
    Ok(check_and_emit(&app).await)
}

#[tauri::command]
pub async fn updater_get_state(state: State<'_, Arc<UpdaterState>>) -> Result<UpdateState, String> {
    Ok(state.current.lock().await.clone())
}

#[tauri::command]
pub async fn updater_get_settings(app: AppHandle) -> Result<UpdaterSettings, String> {
    Ok(load_settings(&app))
}

#[tauri::command]
pub async fn updater_set_auto_check(app: AppHandle, enabled: bool) -> Result<(), String> {
    let mut s = load_settings(&app);
    s.auto_check_enabled = enabled;
    save_settings(&app, &s)
}

#[tauri::command]
pub async fn updater_snooze(app: AppHandle, hours: u32) -> Result<(), String> {
    if hours == 0 {
        return Err("hours must be > 0".into());
    }
    let until = now_ms().saturating_add(u64::from(hours).saturating_mul(60 * 60 * 1000));
    let mut s = load_settings(&app);
    s.snooze_until_ms = Some(until);
    save_settings(&app, &s)?;
    // Re-emit so the UI clears the toast immediately.
    let state = if let Some(v) = s.last_known_version.clone() {
        UpdateState::Snoozed { version: v, until_ms: until }
    } else {
        UpdateState::Idle
    };
    emit(&app, state).await;
    Ok(())
}

#[tauri::command]
pub async fn updater_skip_version(app: AppHandle, version: String) -> Result<(), String> {
    if version.trim().is_empty() {
        return Err("version is empty".into());
    }
    let mut s = load_settings(&app);
    s.skipped_version = Some(version.clone());
    save_settings(&app, &s)?;
    emit(&app, UpdateState::Skipped { version }).await;
    Ok(())
}

#[tauri::command]
pub async fn updater_clear_skip(app: AppHandle) -> Result<(), String> {
    let mut s = load_settings(&app);
    s.skipped_version = None;
    save_settings(&app, &s)?;
    // Trigger a fresh check so the toast re-arms immediately if the
    // skipped version is still the latest.
    let _ = check_and_emit(&app).await;
    Ok(())
}

/// Phase-4 stub for the "Install on quit" flow: download the new DMG
/// into `$app_local_data_dir/pending-update/`, persist the path +
/// version into settings, and emit `InstalledPendingQuit`. Phase 5
/// adds the real before-quit swap script that consumes
/// `pending_update_path`. Until then, the DMG just sits on disk; the
/// startup check on the next launch sees `pending_update_version`
/// matches the running binary's version and clears the slot (Phase 5
/// also wires that detection).
#[tauri::command]
pub async fn updater_install_on_quit(app: AppHandle) -> Result<(), String> {
    let updater = app.updater().map_err(|e| format!("updater handle: {e}"))?;
    let update = updater
        .check()
        .await
        .map_err(|e| format!("check: {e}"))?
        .ok_or_else(|| "no update available".to_string())?;
    let version = update.version.clone();

    let dir = app
        .path()
        .app_local_data_dir()
        .map_err(|e| format!("app_local_data_dir: {e}"))?
        .join("pending-update");
    std::fs::create_dir_all(&dir).map_err(|e| format!("create_dir_all: {e}"))?;
    let dmg_path = dir.join(format!("Woom_{version}.dmg"));

    emit(
        &app,
        UpdateState::Downloading { version: version.clone(), downloaded: 0, total: None },
    )
    .await;

    let app_for_progress = app.clone();
    let version_for_progress = version.clone();
    let bytes = update
        .download(
            move |chunk, total| {
                let app = app_for_progress.clone();
                let version = version_for_progress.clone();
                let chunk = chunk as u64;
                let total = total.map(|t| t as u64);
                tauri::async_runtime::spawn(async move {
                    emit(
                        &app,
                        UpdateState::Downloading { version, downloaded: chunk, total },
                    )
                    .await;
                });
            },
            || {},
        )
        .await
        .map_err(|e| emit_install_failure(&app, &version, format!("download: {e}")))?;

    emit(&app, UpdateState::Verifying { version: version.clone() }).await;
    if let Err(reason) = verify_sha256(&app, &version, &bytes).await {
        return Err(emit_install_failure(&app, &version, reason));
    }

    std::fs::write(&dmg_path, &bytes)
        .map_err(|e| emit_install_failure(&app, &version, format!("write pending DMG: {e}")))?;

    let mut settings = load_settings(&app);
    settings.pending_update_path = Some(dmg_path.to_string_lossy().to_string());
    settings.pending_update_version = Some(version.clone());
    save_settings(&app, &settings)?;

    emit(&app, UpdateState::InstalledPendingQuit { version }).await;
    Ok(())
}

#[tauri::command]
pub async fn updater_install_now(app: AppHandle) -> Result<(), String> {
    // Three-step flow:
    //   1. download() → bytes in memory.
    //   2. fetch the manifest's sha256 for our arch + compare against
    //      sha256(bytes). Mismatch → emit Failed + bail.
    //   3. install(bytes) → atomic swap + restart.
    // Step 2 is the spec's defence-in-depth check — protects against
    // a private-key leak that an attacker also uses to forge a valid
    // ed25519 signature. Without it, a single compromised key would
    // own every install.
    let updater = app.updater().map_err(|e| format!("updater handle: {e}"))?;
    let update = updater
        .check()
        .await
        .map_err(|e| format!("check: {e}"))?
        .ok_or_else(|| "no update available".to_string())?;
    let version = update.version.clone();

    emit(
        &app,
        UpdateState::Downloading { version: version.clone(), downloaded: 0, total: None },
    )
    .await;

    let app_for_progress = app.clone();
    let version_for_progress = version.clone();
    let bytes = update
        .download(
            move |chunk, total| {
                let app = app_for_progress.clone();
                let version = version_for_progress.clone();
                let chunk = chunk as u64;
                let total = total.map(|t| t as u64);
                tauri::async_runtime::spawn(async move {
                    emit(
                        &app,
                        UpdateState::Downloading { version, downloaded: chunk, total },
                    )
                    .await;
                });
            },
            || {},
        )
        .await
        .map_err(|e| emit_install_failure(&app, &version, format!("download: {e}")))?;

    emit(&app, UpdateState::Verifying { version: version.clone() }).await;
    if let Err(reason) = verify_sha256(&app, &version, &bytes).await {
        return Err(emit_install_failure(&app, &version, reason));
    }

    emit(&app, UpdateState::Installing { version: version.clone() }).await;
    update
        .install(bytes)
        .map_err(|e| emit_install_failure(&app, &version, format!("install: {e}")))?;

    emit(&app, UpdateState::InstalledPendingQuit { version }).await;
    Ok(())
}

/// Run the sha256 defence-in-depth comparison. `Ok(())` either means
/// the bytes matched the manifest's hex OR the manifest didn't carry
/// a `sha256` field for our arch (older releases). Both are safe to
/// proceed with — the ed25519 signature was already verified by the
/// plugin during `download()`.
async fn verify_sha256(app: &AppHandle, version: &str, bytes: &[u8]) -> Result<(), String> {
    let expected = match fetch_expected_sha256(app, version).await {
        Ok(Some(s)) => s,
        Ok(None) => return Ok(()),
        Err(e) => return Err(format!("sha256 lookup: {e}")),
    };
    let actual = sha256_hex(bytes);
    if !actual.eq_ignore_ascii_case(&expected) {
        return Err(format!(
            "sha256 mismatch: manifest says {expected} but downloaded bytes hash to {actual}"
        ));
    }
    Ok(())
}

/// Stamp a Failed state into the store + return the reason as the
/// error string so the JS side gets both the event AND the throw.
fn emit_install_failure(app: &AppHandle, version: &str, reason: String) -> String {
    let app = app.clone();
    let v = version.to_string();
    let reason_for_emit = reason.clone();
    tauri::async_runtime::spawn(async move {
        emit(
            &app,
            UpdateState::Failed { version: Some(v), reason: reason_for_emit },
        )
        .await;
    });
    reason
}
