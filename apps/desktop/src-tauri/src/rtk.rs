//! RTK integration — bundled-sidecar discovery, version probing, and the
//! `rtk_status` Tauri command.
//!
//! Phase-1 scope (see SDD workspace `sdd-e1817d13c6`):
//!   - locate the bundled `rtk-<triple>` binary placed by Tauri's
//!     externalBin layout next to the main app binary,
//!   - fall back to a system-PATH `rtk` so users who installed RTK
//!     themselves keep their working setup,
//!   - probe `--version` (host-arch only) and surface a `RtkStatus`
//!     shape to the frontend for the composer pill + welcome screens,
//!   - detect whether the bundled binary supports `rtk hook claude`
//!     (RTK ≥ 0.42 ships a native JSON hook handler — lets us drop the
//!     jq dependency for the wrapper script in Phase 2).
//!
//! Hook installation, env-wiring into `spawn_claude_armed`, and the
//! wrapper-script copy live in Phase 2 (this module only exposes the
//! primitives those need: `resolve_bundled_bin`, `resolve_system_bin`,
//! `probe_version`).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

/// Target triple this binary was compiled for. Set by `build.rs` from
/// the cargo `TARGET` env var. Tauri's externalBin layout keeps the
/// triple suffix on disk in dev (`target/debug/rtk-<triple>`) and in
/// the bundled .app (`Contents/MacOS/rtk-<triple>` alongside the host
/// binary), so we can resolve by appending the triple to a candidate
/// directory.
const TARGET_TRIPLE: &str = env!("WOOM_TARGET_TRIPLE");

/// Snapshot of RTK availability — what the frontend needs to render the
/// composer pill and the welcome-screen explainer.
#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RtkStatus {
    /// Bundled sidecar present + executable.
    pub bundled_available: bool,
    /// Absolute path to the bundled binary if found.
    pub bundled_path: Option<String>,
    /// Version reported by `<bundled> --version`; only set on the host
    /// arch (cross-arch bundles can't be exec'd to probe).
    pub bundled_version: Option<String>,
    /// User-installed `rtk` somewhere on PATH (independent of Woom's
    /// bundle). Lets Phase 2 detect "user already ran `rtk init -g`"
    /// and avoid duplicating their hook config.
    pub system_path: Option<String>,
    pub system_version: Option<String>,
    /// `jq` available on PATH. Phase 2 wrapper script wants it for the
    /// legacy hook shape; native `rtk hook claude` (≥0.42) avoids the
    /// dependency entirely — see `uses_native_hook`.
    pub jq_available: bool,
    /// `rtk hook claude --help` returned cleanly — the bundled rtk has
    /// the native JSON-envelope handler so the wrapper script can call
    /// `rtk hook claude` directly without jq plumbing.
    pub uses_native_hook: bool,
    /// `false` on native Windows (the PreToolUse hook itself requires a
    /// Unix shell — see upstream RTK README "Windows" section). The
    /// composer pill hides itself when this is false.
    pub platform_supported: bool,
}

/// Resolve the bundled `rtk-<triple>` next to the main binary.
///
/// Layout in practice:
///   - dev (`pnpm tauri dev`): `target/debug/rtk-<triple>` next to
///     `target/debug/woom-desktop`.
///   - release `.app` (macOS): `Contents/MacOS/rtk-<triple>` next to
///     `Contents/MacOS/Woom`.
///   - release `.AppImage` / debian package (Linux): same dir as the
///     main binary.
///
/// `app` is accepted for forward-compat (Tauri 2's resource-dir lookup
/// might become the canonical path later) but the actual resolution
/// today is `current_exe().parent() / rtk-<triple>` because that's what
/// Tauri's externalBin produces.
pub fn resolve_bundled_bin(_app: &AppHandle) -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    resolve_bundled_in_dir(dir)
}

/// Pure helper that the unit-tests drive directly — keeps the resolver
/// independent of an `AppHandle` (Tauri's test harness is heavy and
/// the only thing we actually use it for is `current_exe()`'s parent
/// dir, which we can pass in).
pub(crate) fn resolve_bundled_in_dir(dir: &Path) -> Option<PathBuf> {
    let candidates = [
        dir.join(format!("rtk-{TARGET_TRIPLE}")),
        // Tauri sometimes strips the triple suffix on release bundles
        // (the docs go back and forth on this). Try the alias too so a
        // future tooling shift doesn't break resolution silently.
        dir.join("rtk"),
    ];
    for c in candidates {
        if is_executable(&c) {
            return Some(c);
        }
    }
    None
}

/// PATH lookup for a user-installed `rtk` — same shape as
/// `claude::resolve_bin` (see `claude.rs`). Kept independent so a
/// missing/dev-only bundled sidecar still surfaces the user's own
/// install.
pub fn resolve_system_bin() -> Option<PathBuf> {
    which("rtk")
}

fn which(name: &str) -> Option<PathBuf> {
    let mut candidates: Vec<String> = Vec::new();
    if let Ok(p) = std::env::var("PATH") {
        for dir in p.split(':') {
            if !dir.is_empty() {
                candidates.push(dir.to_string());
            }
        }
    }
    for extra in ["/opt/homebrew/bin", "/usr/local/bin"] {
        if !candidates.iter().any(|d| d == extra) {
            candidates.push(extra.into());
        }
    }
    if let Some(h) = home_dir() {
        for sub in [".local/bin", ".cargo/bin"] {
            let full = h.join(sub).to_string_lossy().into_owned();
            if !candidates.iter().any(|d| d == &full) {
                candidates.push(full);
            }
        }
    }
    for dir in candidates {
        let candidate = Path::new(&dir).join(name);
        if is_executable(&candidate) {
            return Some(candidate);
        }
    }
    None
}

fn home_dir() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(PathBuf::from)
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    match std::fs::metadata(path) {
        Ok(m) => m.is_file() && (m.permissions().mode() & 0o111 != 0),
        Err(_) => false,
    }
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

/// Probe `<bin> --version`. Mirrors the hard-deadline pattern in
/// `claude.rs::read_version`: RTK's version-probe is supposed to be
/// instant but a corrupt binary can hang indefinitely, so we cap at 2s
/// and kill on overshoot.
pub fn probe_version(bin: &Path) -> Option<String> {
    use std::io::Read;
    let mut child = std::process::Command::new(bin)
        .arg("--version")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    return None;
                }
                let mut buf = String::new();
                if let Some(mut out) = child.stdout.take() {
                    out.read_to_string(&mut buf).ok()?;
                }
                let trimmed = buf.trim();
                // RTK prints `rtk 0.42.0` (the binary name is
                // `rtk-<triple>` in dev, so the prefix varies). Pull
                // the second whitespace-delimited token if present.
                let version = trimmed
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or(trimmed)
                    .to_string();
                return if version.is_empty() {
                    None
                } else {
                    Some(version)
                };
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(_) => return None,
        }
    }
}

/// `rtk hook claude --help` returning exit 0 means the binary has the
/// native JSON envelope handler — Phase 2 wrapper script can skip jq
/// entirely and pipe straight into `rtk hook claude`.
fn probe_native_hook(bin: &Path) -> bool {
    let out = std::process::Command::new(bin)
        .args(["hook", "claude", "--help"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .output();
    matches!(out, Ok(o) if o.status.success())
}

fn current_platform_supported() -> bool {
    // Native Windows can't run the upstream hook (requires Unix shell).
    // Tauri builds for Windows would still pass through the `rtk`
    // binary itself, but the hook never gets invoked, so we surface
    // that as "unsupported" and the UI hides the pill.
    !cfg!(target_os = "windows")
}

/// Snapshot RTK availability. Cheap enough to call on demand; the
/// frontend caches the result in a Svelte derive — no need for a
/// `tauri::State<...>` cache.
pub fn status(app: &AppHandle) -> RtkStatus {
    let bundled_path = resolve_bundled_bin(app);
    let system_path = resolve_system_bin();
    let bundled_version = bundled_path
        .as_deref()
        .and_then(probe_version);
    let system_version = system_path.as_deref().and_then(probe_version);
    let jq_available = which("jq").is_some();
    let uses_native_hook = bundled_path
        .as_deref()
        .map(probe_native_hook)
        .unwrap_or(false);

    RtkStatus {
        bundled_available: bundled_path.is_some(),
        bundled_path: bundled_path.map(|p| p.to_string_lossy().into_owned()),
        bundled_version,
        system_path: system_path.map(|p| p.to_string_lossy().into_owned()),
        system_version,
        jq_available,
        uses_native_hook,
        platform_supported: current_platform_supported(),
    }
}

#[tauri::command]
pub async fn rtk_status(app: AppHandle) -> RtkStatus {
    let handle = app.clone();
    tokio::task::spawn_blocking(move || status(&handle))
        .await
        .unwrap_or_default()
}

// ---- Hook installer ------------------------------------------------------
//
// Layout:
//   - bundled wrapper template lives at
//     `apps/desktop/src-tauri/resources/rtk-rewrite-woom.sh` (dev) or
//     `<app>.app/Contents/Resources/resources/rtk-rewrite-woom.sh`
//     (release; Tauri's `bundle.resources` keeps the relative path).
//   - copied wrapper lives at `<data_local_dir>/woom/hooks/
//     rtk-rewrite-woom.sh` with the `__WOOM_RTK_BIN__` placeholder
//     substituted for the absolute path to the bundled rtk binary.
//   - Claude Code settings live at `~/.claude/settings.json`. We
//     splice a PreToolUse block in with the markers `managed_by:
//     "woom"` + `hook_version` so the next install run can detect
//     whether to no-op (same version), update (newer wrapper), or
//     leave alone (user-managed entry).

/// Bumped whenever we change wrapper-script semantics in a way that
/// requires a re-copy of the file on disk (template changes, new env
/// flag, etc).
pub const WRAPPER_HOOK_VERSION: u32 = 1;

/// Token swapped at copy time so the on-disk script holds an absolute
/// path to the bundled rtk binary.
const WRAPPER_BIN_PLACEHOLDER: &str = "__WOOM_RTK_BIN__";

/// Outcome of a single `install_hook` call. The frontend doesn't see
/// this today — it's lifted out for diagnostics + unit-test assertions.
#[derive(Debug, PartialEq, Eq, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum InstallResult {
    /// Hook was missing — wrote the woom-managed block.
    Installed,
    /// Existing woom-managed block was already at the current version.
    NoChange,
    /// Existing woom-managed block was at an older version — bumped it.
    Upgraded { from: u32, to: u32 },
    /// The user already has their own PreToolUse hook on Bash. We
    /// don't touch it; the UI surfaces this via `hook_installed_status`
    /// → `InstalledByUser`.
    SkippedUserOwned,
    /// Multiple PreToolUse blocks on Bash present. Safer to bail out
    /// than to guess which one is canonical.
    SkippedConflicted,
}

/// Classification of the existing PreToolUse-on-Bash hook (if any) in
/// the user's `~/.claude/settings.json`. Used to decide whether to
/// install / upgrade / skip.
#[derive(Debug, PartialEq, Eq, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HookInstallState {
    NotInstalled,
    InstalledByWoom { command: String, hook_version: u32 },
    InstalledByUser { command: String },
    Conflicted,
}

/// Locate the bundled wrapper-script template. Mirrors the
/// `bundled_docs_dir` pattern in `lib.rs` — try Tauri's resource_dir
/// first, then fall back to the repo's `apps/desktop/src-tauri/resources/`
/// directory for dev builds (the dir walks up from `current_exe`).
pub fn bundled_wrapper_template(app: &AppHandle) -> Option<PathBuf> {
    if let Ok(rd) = app.path().resource_dir() {
        let p = rd.join("resources/rtk-rewrite-woom.sh");
        if p.is_file() {
            return Some(p);
        }
    }
    // Dev fallback: walk up from `current_exe` until we find the
    // `apps/desktop/src-tauri/resources/rtk-rewrite-woom.sh` shape.
    let exe = std::env::current_exe().ok()?;
    let mut cur = exe.parent().map(|p| p.to_path_buf());
    for _ in 0..8 {
        if let Some(p) = &cur {
            let cand = p.join("resources/rtk-rewrite-woom.sh");
            if cand.is_file() {
                return Some(cand);
            }
            let cand_nested = p.join("apps/desktop/src-tauri/resources/rtk-rewrite-woom.sh");
            if cand_nested.is_file() {
                return Some(cand_nested);
            }
            cur = p.parent().map(|p| p.to_path_buf());
        } else {
            break;
        }
    }
    None
}

/// Target location for the runtime copy of the wrapper script. macOS:
/// `~/Library/Application Support/woom/hooks/`. Linux: `~/.local/share/
/// woom/hooks/`. Falls back to `$HOME/.woom/hooks/` if neither standard
/// dir resolves (e.g. exotic HOME on a CI runner).
pub fn wrapper_target_path() -> Option<PathBuf> {
    let home = home_dir()?;
    let dir = if cfg!(target_os = "macos") {
        home.join("Library/Application Support/woom/hooks")
    } else {
        home.join(".local/share/woom/hooks")
    };
    Some(dir.join("rtk-rewrite-woom.sh"))
}

/// Read the bundled wrapper template, substitute the rtk binary path,
/// and write the result to `target` with executable permissions.
/// Idempotent — when the on-disk file already matches the rendered
/// content, the function leaves it untouched (and reports `false` so
/// the caller can skip downstream work).
///
/// Returns `Ok(true)` if a write occurred (new or changed), `Ok(false)`
/// if the file was already correct.
pub fn copy_wrapper_script(template: &Path, rtk_bin: &Path, target: &Path) -> std::io::Result<bool> {
    let raw = fs::read_to_string(template)?;
    let rtk_str = rtk_bin.to_string_lossy();
    let rendered = raw.replace(WRAPPER_BIN_PLACEHOLDER, &rtk_str);

    if let Ok(existing) = fs::read_to_string(target) {
        if existing == rendered {
            return Ok(false);
        }
    }

    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    atomic_write(target, rendered.as_bytes())?;
    set_executable(target)?;
    Ok(true)
}

#[cfg(unix)]
fn set_executable(p: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(p, fs::Permissions::from_mode(0o755))
}

#[cfg(not(unix))]
fn set_executable(_p: &Path) -> std::io::Result<()> {
    Ok(())
}

/// Atomically write `data` to `target`: write to `<target>.woom-tmp.<pid>`
/// then `rename` into place. Crash-safe — a partial write never replaces
/// the existing file.
fn atomic_write(target: &Path, data: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let parent = target
        .parent()
        .ok_or_else(|| std::io::Error::other("target has no parent"))?;
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
        let mut f = fs::File::create(&tmp)?;
        f.write_all(data)?;
        f.sync_all().ok();
    }
    fs::rename(&tmp, target)?;
    Ok(())
}

/// Resolve the Claude Code settings file. Lifted as a helper so unit
/// tests can swap in a temp path.
pub fn claude_settings_path() -> Option<PathBuf> {
    home_dir().map(|h| h.join(".claude/settings.json"))
}

/// Inspect `settings_path` and classify any existing PreToolUse-on-Bash
/// hook. Treats absent file / missing keys as `NotInstalled`. Multiple
/// matching entries → `Conflicted`. Recognises woom-managed entries
/// via the `managed_by: "woom"` marker plus reads back `hook_version`
/// so the caller can decide between no-op vs upgrade.
pub fn hook_installed_status(settings_path: &Path) -> HookInstallState {
    let raw = match fs::read_to_string(settings_path) {
        Ok(s) => s,
        Err(_) => return HookInstallState::NotInstalled,
    };
    let val: Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        // Malformed JSON in the user's settings — treat as "no hook"
        // so install_hook's atomic write replaces it cleanly. The
        // alternative (refusing to touch it) leaves the user without
        // RTK with no clear path to recovery.
        Err(_) => return HookInstallState::NotInstalled,
    };
    let blocks = val
        .pointer("/hooks/PreToolUse")
        .and_then(|v| v.as_array());
    let blocks = match blocks {
        Some(b) => b,
        None => return HookInstallState::NotInstalled,
    };

    let mut matches: Vec<&Value> = Vec::new();
    for b in blocks {
        let matcher = b
            .get("matcher")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        // RTK's upstream `rtk init -g` writes `matcher: "Bash"`. We do
        // the same. A bare-matcher block (empty/missing) is also
        // potentially a conflict if it has a hooks array, but the
        // Claude Code convention is to set matcher explicitly, so
        // we only flag explicit Bash entries.
        if matcher == "Bash" {
            matches.push(b);
        }
    }

    match matches.len() {
        0 => HookInstallState::NotInstalled,
        1 => {
            let b = matches[0];
            let command = extract_first_command(b).unwrap_or_default();
            if b.get("managed_by").and_then(|v| v.as_str()) == Some("woom") {
                let hv = b
                    .get("hook_version")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32;
                HookInstallState::InstalledByWoom {
                    command,
                    hook_version: hv,
                }
            } else {
                HookInstallState::InstalledByUser { command }
            }
        }
        _ => HookInstallState::Conflicted,
    }
}

fn extract_first_command(block: &Value) -> Option<String> {
    let hooks = block.get("hooks")?.as_array()?;
    let h = hooks.first()?;
    h.get("command")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// Install (or upgrade) the woom-managed PreToolUse-on-Bash hook in
/// `settings_path`. Idempotent — running twice in a row at the current
/// version produces `NoChange` on the second call. If the user already
/// has their own hook, we leave it alone and return
/// `SkippedUserOwned`.
pub fn install_hook(settings_path: &Path, wrapper_path: &Path) -> std::io::Result<InstallResult> {
    let state = hook_installed_status(settings_path);
    match state {
        HookInstallState::InstalledByWoom { hook_version, .. }
            if hook_version == WRAPPER_HOOK_VERSION =>
        {
            return Ok(InstallResult::NoChange);
        }
        HookInstallState::InstalledByUser { .. } => return Ok(InstallResult::SkippedUserOwned),
        HookInstallState::Conflicted => return Ok(InstallResult::SkippedConflicted),
        _ => {}
    }

    // Load (or initialise) the settings document.
    let mut doc: Value = if settings_path.exists() {
        match fs::read_to_string(settings_path).ok().and_then(|s| serde_json::from_str(&s).ok()) {
            Some(v) => v,
            None => json!({}),
        }
    } else {
        json!({})
    };
    if !doc.is_object() {
        doc = json!({});
    }

    // Build the woom-managed block.
    let block = json!({
        "matcher": "Bash",
        "managed_by": "woom",
        "hook_version": WRAPPER_HOOK_VERSION,
        "hooks": [
            { "type": "command", "command": wrapper_path.to_string_lossy() }
        ]
    });

    // Walk into `.hooks.PreToolUse` (creating intermediate objects).
    let hooks = doc
        .as_object_mut()
        .expect("doc is object")
        .entry("hooks")
        .or_insert_with(|| json!({}));
    if !hooks.is_object() {
        *hooks = json!({});
    }
    let pre = hooks
        .as_object_mut()
        .expect("hooks is object")
        .entry("PreToolUse")
        .or_insert_with(|| json!([]));
    if !pre.is_array() {
        *pre = json!([]);
    }

    let arr = pre.as_array_mut().expect("PreToolUse is array");

    // Strip prior woom-managed Bash entries — the upgrade case removes
    // the stale one and re-inserts at the same logical position.
    let mut prior_version: Option<u32> = None;
    arr.retain(|b| {
        let is_woom_bash = b.get("matcher").and_then(|v| v.as_str()) == Some("Bash")
            && b.get("managed_by").and_then(|v| v.as_str()) == Some("woom");
        if is_woom_bash {
            if let Some(v) = b.get("hook_version").and_then(|v| v.as_u64()) {
                prior_version = Some(v as u32);
            }
            false
        } else {
            true
        }
    });
    arr.push(block);

    if let Some(parent) = settings_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let serialized = serde_json::to_string_pretty(&doc)
        .map_err(std::io::Error::other)?;
    let mut out = serialized;
    if !out.ends_with('\n') {
        out.push('\n');
    }
    atomic_write(settings_path, out.as_bytes())?;

    Ok(match prior_version {
        Some(v) => InstallResult::Upgraded {
            from: v,
            to: WRAPPER_HOOK_VERSION,
        },
        None => InstallResult::Installed,
    })
}

/// Remove only the woom-managed PreToolUse-on-Bash block (if present),
/// preserving any other PreToolUse entries the user may have added.
/// Returns whether a write occurred.
pub fn uninstall_hook(settings_path: &Path) -> std::io::Result<bool> {
    let raw = match fs::read_to_string(settings_path) {
        Ok(s) => s,
        Err(_) => return Ok(false),
    };
    let mut doc: Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(_) => return Ok(false),
    };

    let mut removed = false;
    if let Some(pre) = doc
        .pointer_mut("/hooks/PreToolUse")
        .and_then(|v| v.as_array_mut())
    {
        let before = pre.len();
        pre.retain(|b| {
            let is_woom = b.get("matcher").and_then(|v| v.as_str()) == Some("Bash")
                && b.get("managed_by").and_then(|v| v.as_str()) == Some("woom");
            !is_woom
        });
        removed = pre.len() < before;
    }

    if !removed {
        return Ok(false);
    }

    let serialized = serde_json::to_string_pretty(&doc)
        .map_err(std::io::Error::other)?;
    let mut out = serialized;
    if !out.ends_with('\n') {
        out.push('\n');
    }
    atomic_write(settings_path, out.as_bytes())?;
    Ok(true)
}

/// First-launch wiring — locate the bundled binary + wrapper template,
/// copy the wrapper into the user's data dir with the correct rtk path,
/// then patch `~/.claude/settings.json`. Each step degrades gracefully
/// (a missing template warns + returns, never panics the app).
pub async fn bootstrap(app: &AppHandle) -> Result<(), String> {
    if !current_platform_supported() {
        return Ok(());
    }
    let app_clone = app.clone();
    tokio::task::spawn_blocking(move || bootstrap_blocking(&app_clone))
        .await
        .map_err(|e| format!("bootstrap task panicked: {e}"))?
}

fn bootstrap_blocking(app: &AppHandle) -> Result<(), String> {
    let rtk_bin = match resolve_bundled_bin(app) {
        Some(p) => p,
        None => {
            // No bundled rtk (e.g. dev run without `fetch-rtk.sh`).
            // Stop quietly — composer pill will surface the
            // "unavailable" state on its own status probe.
            return Ok(());
        }
    };
    let template = bundled_wrapper_template(app)
        .ok_or_else(|| "wrapper template missing from bundle".to_string())?;
    let target = wrapper_target_path()
        .ok_or_else(|| "HOME not resolvable — cannot place wrapper".to_string())?;

    copy_wrapper_script(&template, &rtk_bin, &target)
        .map_err(|e| format!("copy wrapper: {e}"))?;

    let settings = claude_settings_path()
        .ok_or_else(|| "HOME not resolvable — cannot patch settings".to_string())?;
    install_hook(&settings, &target).map_err(|e| format!("install hook: {e}"))?;
    Ok(())
}

#[tauri::command]
pub async fn rtk_install_hook(app: AppHandle) -> Result<InstallResult, String> {
    let app_clone = app.clone();
    tokio::task::spawn_blocking(move || -> Result<InstallResult, String> {
        let rtk_bin = resolve_bundled_bin(&app_clone)
            .ok_or_else(|| "bundled rtk binary missing".to_string())?;
        let template = bundled_wrapper_template(&app_clone)
            .ok_or_else(|| "wrapper template missing".to_string())?;
        let target = wrapper_target_path()
            .ok_or_else(|| "HOME not resolvable".to_string())?;
        copy_wrapper_script(&template, &rtk_bin, &target)
            .map_err(|e| format!("copy wrapper: {e}"))?;
        let settings = claude_settings_path()
            .ok_or_else(|| "HOME not resolvable".to_string())?;
        install_hook(&settings, &target).map_err(|e| format!("install: {e}"))
    })
    .await
    .map_err(|e| format!("rtk_install_hook task panicked: {e}"))?
}

#[tauri::command]
pub async fn rtk_uninstall_hook() -> Result<bool, String> {
    tokio::task::spawn_blocking(|| -> Result<bool, String> {
        let settings = claude_settings_path()
            .ok_or_else(|| "HOME not resolvable".to_string())?;
        uninstall_hook(&settings).map_err(|e| format!("uninstall: {e}"))
    })
    .await
    .map_err(|e| format!("rtk_uninstall_hook task panicked: {e}"))?
}

#[tauri::command]
pub async fn rtk_hook_state() -> HookInstallState {
    tokio::task::spawn_blocking(|| {
        claude_settings_path()
            .map(|p| hook_installed_status(&p))
            .unwrap_or(HookInstallState::NotInstalled)
    })
    .await
    .unwrap_or(HookInstallState::NotInstalled)
}

// ---- Tests ---------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn resolves_bundled_with_triple_suffix() {
        let tmp = tempdir();
        let bin = tmp.path().join(format!("rtk-{TARGET_TRIPLE}"));
        fs::write(&bin, b"#!/usr/bin/env sh\necho rtk 0.42.0\n").unwrap();
        chmod_exec(&bin);

        let resolved = resolve_bundled_in_dir(tmp.path()).expect("should find bundled");
        assert_eq!(resolved, bin);
    }

    #[test]
    fn resolves_bundled_alias_fallback() {
        let tmp = tempdir();
        let bin = tmp.path().join("rtk");
        fs::write(&bin, b"#!/usr/bin/env sh\necho rtk 0.42.0\n").unwrap();
        chmod_exec(&bin);

        let resolved = resolve_bundled_in_dir(tmp.path()).expect("should find alias");
        assert_eq!(resolved, bin);
    }

    #[test]
    fn missing_bundled_returns_none() {
        let tmp = tempdir();
        assert!(resolve_bundled_in_dir(tmp.path()).is_none());
    }

    #[test]
    fn probe_version_parses_rtk_format() {
        let tmp = tempdir();
        let bin = tmp.path().join("rtk-fake");
        fs::write(&bin, b"#!/usr/bin/env sh\necho 'rtk 0.42.0'\n").unwrap();
        chmod_exec(&bin);

        let v = probe_version(&bin).expect("version");
        assert_eq!(v, "0.42.0");
    }

    #[test]
    fn probe_version_handles_failure() {
        let tmp = tempdir();
        let bin = tmp.path().join("broken");
        fs::write(&bin, b"#!/usr/bin/env sh\nexit 1\n").unwrap();
        chmod_exec(&bin);

        assert!(probe_version(&bin).is_none());
    }

    // ----- tiny tempdir helper (avoids the `tempfile` crate dep for one
    // test module). Per-test unique path under the system temp dir +
    // a Drop that recursively removes it.

    struct TempDir(PathBuf);
    impl TempDir {
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn tempdir() -> TempDir {
        let mut p = std::env::temp_dir();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        p.push(format!("woom-rtk-test-{nanos}-{}", std::process::id()));
        fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }

    #[cfg(unix)]
    fn chmod_exec(p: &Path) {
        fs::set_permissions(p, fs::Permissions::from_mode(0o755)).unwrap();
    }
    #[cfg(not(unix))]
    fn chmod_exec(_p: &Path) {}

    // ----- Hook installer tests --------------------------------------------

    fn write_template(dir: &Path) -> PathBuf {
        let p = dir.join("rtk-rewrite-woom.sh");
        let body = "#!/usr/bin/env bash\nRTK_BIN=\"__WOOM_RTK_BIN__\"\nexec \"$RTK_BIN\" hook claude\n";
        fs::write(&p, body).unwrap();
        p
    }

    #[test]
    fn copy_wrapper_substitutes_path_and_chmods() {
        let tmp = tempdir();
        let template = write_template(tmp.path());
        let rtk_bin = tmp.path().join("rtk-fake");
        fs::write(&rtk_bin, "#!/bin/sh\necho ok\n").unwrap();
        chmod_exec(&rtk_bin);
        let target = tmp.path().join("nested/dir/rtk-rewrite-woom.sh");

        let wrote = copy_wrapper_script(&template, &rtk_bin, &target).unwrap();
        assert!(wrote, "first call must write the file");

        let content = fs::read_to_string(&target).unwrap();
        assert!(content.contains(rtk_bin.to_string_lossy().as_ref()));
        assert!(!content.contains(WRAPPER_BIN_PLACEHOLDER));

        #[cfg(unix)]
        {
            let mode = fs::metadata(&target).unwrap().permissions().mode();
            assert_eq!(mode & 0o111, 0o111, "must be executable");
        }
    }

    #[test]
    fn copy_wrapper_idempotent_on_second_call() {
        let tmp = tempdir();
        let template = write_template(tmp.path());
        let rtk_bin = tmp.path().join("rtk-fake");
        fs::write(&rtk_bin, "ok").unwrap();
        let target = tmp.path().join("rtk-rewrite-woom.sh");

        let first = copy_wrapper_script(&template, &rtk_bin, &target).unwrap();
        let second = copy_wrapper_script(&template, &rtk_bin, &target).unwrap();
        assert!(first);
        assert!(!second, "second call must be a no-op");
    }

    #[test]
    fn hook_status_not_installed_when_file_missing() {
        let tmp = tempdir();
        let p = tmp.path().join("does-not-exist.json");
        assert_eq!(hook_installed_status(&p), HookInstallState::NotInstalled);
    }

    #[test]
    fn hook_status_recognises_woom_block() {
        let tmp = tempdir();
        let p = tmp.path().join("settings.json");
        fs::write(
            &p,
            r#"{"hooks":{"PreToolUse":[{"matcher":"Bash","managed_by":"woom","hook_version":1,"hooks":[{"type":"command","command":"/x/y/rtk-rewrite-woom.sh"}]}]}}"#,
        )
        .unwrap();
        match hook_installed_status(&p) {
            HookInstallState::InstalledByWoom { command, hook_version } => {
                assert_eq!(command, "/x/y/rtk-rewrite-woom.sh");
                assert_eq!(hook_version, 1);
            }
            other => panic!("expected InstalledByWoom, got {other:?}"),
        }
    }

    #[test]
    fn hook_status_recognises_user_block() {
        let tmp = tempdir();
        let p = tmp.path().join("settings.json");
        fs::write(
            &p,
            r#"{"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"/usr/local/bin/rtk-rewrite.sh"}]}]}}"#,
        )
        .unwrap();
        match hook_installed_status(&p) {
            HookInstallState::InstalledByUser { command } => {
                assert_eq!(command, "/usr/local/bin/rtk-rewrite.sh");
            }
            other => panic!("expected InstalledByUser, got {other:?}"),
        }
    }

    #[test]
    fn hook_status_detects_conflict() {
        let tmp = tempdir();
        let p = tmp.path().join("settings.json");
        fs::write(
            &p,
            r#"{"hooks":{"PreToolUse":[
                {"matcher":"Bash","managed_by":"woom","hook_version":1,"hooks":[{"type":"command","command":"/a.sh"}]},
                {"matcher":"Bash","hooks":[{"type":"command","command":"/b.sh"}]}
            ]}}"#,
        )
        .unwrap();
        assert_eq!(hook_installed_status(&p), HookInstallState::Conflicted);
    }

    #[test]
    fn install_then_install_is_noop() {
        let tmp = tempdir();
        let settings = tmp.path().join(".claude/settings.json");
        let wrapper = tmp.path().join("rtk-rewrite-woom.sh");

        let r1 = install_hook(&settings, &wrapper).unwrap();
        assert_eq!(r1, InstallResult::Installed);
        let hash1 = fs::read_to_string(&settings).unwrap();

        let r2 = install_hook(&settings, &wrapper).unwrap();
        assert_eq!(r2, InstallResult::NoChange);
        let hash2 = fs::read_to_string(&settings).unwrap();
        assert_eq!(hash1, hash2, "second install must not rewrite settings.json");
    }

    #[test]
    fn install_with_user_hook_skips() {
        let tmp = tempdir();
        let settings = tmp.path().join("settings.json");
        let initial = r#"{"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"/u/sr.sh"}]}]}}"#;
        fs::write(&settings, initial).unwrap();
        let wrapper = tmp.path().join("rtk-rewrite-woom.sh");

        let r = install_hook(&settings, &wrapper).unwrap();
        assert_eq!(r, InstallResult::SkippedUserOwned);
        let after = fs::read_to_string(&settings).unwrap();
        assert_eq!(after, initial, "user's settings must remain untouched");
    }

    #[test]
    fn install_preserves_unrelated_hooks() {
        let tmp = tempdir();
        let settings = tmp.path().join("settings.json");
        // User has SessionStart + a non-Bash PreToolUse matcher (e.g.
        // Read). Our installer must leave both alone.
        let initial = r#"{
          "hooks": {
            "SessionStart": [
              { "hooks": [{ "type": "command", "command": "/u/start.sh" }] }
            ],
            "PreToolUse": [
              { "matcher": "Read", "hooks": [{ "type": "command", "command": "/u/read.sh" }] }
            ]
          }
        }"#;
        fs::write(&settings, initial).unwrap();
        let wrapper = tmp.path().join("rtk-rewrite-woom.sh");

        let r = install_hook(&settings, &wrapper).unwrap();
        assert_eq!(r, InstallResult::Installed);

        let after: Value =
            serde_json::from_str(&fs::read_to_string(&settings).unwrap()).unwrap();
        // SessionStart untouched.
        assert!(after.pointer("/hooks/SessionStart/0/hooks/0/command")
            .and_then(|v| v.as_str())
            == Some("/u/start.sh"));
        // Both PreToolUse blocks present (Read + Bash).
        let pre = after.pointer("/hooks/PreToolUse").unwrap().as_array().unwrap();
        assert_eq!(pre.len(), 2);
        let matchers: Vec<&str> = pre
            .iter()
            .map(|b| b.get("matcher").and_then(|v| v.as_str()).unwrap_or(""))
            .collect();
        assert!(matchers.contains(&"Read"));
        assert!(matchers.contains(&"Bash"));
    }

    #[test]
    fn uninstall_removes_only_woom_block() {
        let tmp = tempdir();
        let settings = tmp.path().join("settings.json");
        let wrapper = tmp.path().join("rtk-rewrite-woom.sh");

        // Seed with a user Read-hook + install ours.
        fs::write(
            &settings,
            r#"{"hooks":{"PreToolUse":[{"matcher":"Read","hooks":[{"type":"command","command":"/u/r.sh"}]}]}}"#,
        )
        .unwrap();
        install_hook(&settings, &wrapper).unwrap();
        assert!(matches!(
            hook_installed_status(&settings),
            HookInstallState::InstalledByWoom { .. }
        ));

        let did = uninstall_hook(&settings).unwrap();
        assert!(did);
        let after: Value =
            serde_json::from_str(&fs::read_to_string(&settings).unwrap()).unwrap();
        let pre = after.pointer("/hooks/PreToolUse").unwrap().as_array().unwrap();
        assert_eq!(pre.len(), 1);
        assert_eq!(
            pre[0].get("matcher").and_then(|v| v.as_str()),
            Some("Read"),
            "user Read-hook must survive"
        );
    }
}
