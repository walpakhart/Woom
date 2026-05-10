//! Crash reporting — Sentry scaffolding + opt-out plumbing.
//!
//! `docs/ROADMAP_1.0.md §1.3` calls for "a dogfood Sentry project for
//! the Forge app itself" with a user-visible opt-out. This module
//! provides everything that doesn't depend on a live Sentry DSN:
//!
//! - File-backed opt-out flag at
//!   `~/Library/Application Support/Woom/telemetry-opt-out.flag`.
//!   Plain "presence == opted out" semantics — no JSON to corrupt,
//!   readable by every other tool.
//! - Tauri commands `get_telemetry_opt_out` / `set_telemetry_opt_out`
//!   that the Settings view binds to its toggle.
//! - `init_if_enabled()` hook fired from `lib::run()` before the
//!   event loop. Currently a no-op; once the deployment carries a
//!   real DSN (env var `WOOM_SENTRY_DSN`), uncomment the
//!   marked block to wire `sentry::init()` (the SDK can be added
//!   to `Cargo.toml` then — see the comment below).
//!
//! Why scaffold the opt-out before the SDK lands:
//!   1. The toggle's behaviour is already correct (we never collect),
//!      so users who flip it during the alpha don't need re-toggling
//!      after we ship telemetry — their preference is honoured.
//!   2. Settings UI parity with the 1.0 spec is a small step now;
//!      retro-fitting the toggle later when crash reports are already
//!      live invites a "sent reports for a few hours before the toggle
//!      worked" bug.

use std::path::PathBuf;

const OPT_OUT_FLAG: &str = "telemetry-opt-out.flag";

/// Resolve the per-user Woom app-support directory. Mirrors the
/// path used by `worktree::storage_dir()` and `claude::sessions_dir()`
/// so all Woom state ends up under one root the user can poke at
/// via Finder.
fn app_support_dir() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    let path = PathBuf::from(home)
        .join("Library")
        .join("Application Support")
        .join("Woom");
    std::fs::create_dir_all(&path).ok()?;
    Some(path)
}

fn opt_out_flag_path() -> Option<PathBuf> {
    app_support_dir().map(|p| p.join(OPT_OUT_FLAG))
}

/// Whether the user has opted out of crash reporting.
pub fn is_opted_out() -> bool {
    opt_out_flag_path()
        .map(|p| p.exists())
        .unwrap_or(false)
}

/// Persist the opt-out preference. `true` writes the flag file;
/// `false` removes it.
pub fn set_opted_out(opted_out: bool) -> Result<(), String> {
    let Some(path) = opt_out_flag_path() else {
        return Err("could not resolve app-support directory".into());
    };
    if opted_out {
        std::fs::write(&path, b"opted out\n").map_err(|e| e.to_string())?;
    } else if path.exists() {
        std::fs::remove_file(&path).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Initialize Sentry if a DSN is configured AND the user hasn't
/// opted out. Currently a no-op until the SDK is added; the
/// commented block below shows the intended shape.
///
/// To enable:
///   1. Add to `Cargo.toml`:
///      ```toml
///      sentry-sdk = { package = "sentry", version = "0.32", default-features = false,
///                     features = ["backtrace", "contexts", "panic", "rustls"] }
///      ```
///      (Renamed because our local `mod sentry` shadows the crate
///      name.)
///   2. Uncomment the `let _guard = ...` block.
///   3. Hold `_guard` for the lifetime of `run()` so the background
///      submission thread keeps draining the queue.
pub fn init_if_enabled() {
    if is_opted_out() {
        return;
    }
    let dsn = match std::env::var("WOOM_SENTRY_DSN") {
        Ok(v) if !v.is_empty() => v,
        _ => return, /* no DSN → nothing to wire up */
    };
    /* Once `sentry-sdk` is in Cargo.toml, replace this `_ = dsn;` no-
     * op with the real init. Keeping the variable consumed silences
     * the unused-warning until then. */
    let _ = dsn;
    /*
    let _guard = sentry_sdk::init((
        dsn,
        sentry_sdk::ClientOptions {
            release: sentry_sdk::release_name!(),
            send_default_pii: false,
            attach_stacktrace: true,
            ..Default::default()
        },
    ));
    /* `_guard` must live for the program lifetime. Move it onto
     * a `static` slot or hold it on the stack of `run()`. */
    */
}

#[tauri::command]
pub fn get_telemetry_opt_out() -> bool {
    is_opted_out()
}

#[tauri::command]
pub fn set_telemetry_opt_out(opt_out: bool) -> Result<(), String> {
    set_opted_out(opt_out)
}
