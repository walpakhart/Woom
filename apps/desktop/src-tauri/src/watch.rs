//! Recursive filesystem watcher for the open repo.
//!
//! Single global watcher per app. The frontend calls `fs_watch_start(path)`
//! when it opens a folder; we swap out any previous watcher and install a
//! new recursive one. File-change events are emitted to the frontend as
//! `fs:changed` events with `{ path, kind }`.
//!
//! `.git` and node_modules subtrees are filtered out because they churn
//! constantly and are never interesting to the editor.

use std::path::Path;
use std::sync::{Arc, Mutex};

use notify::{EventKind, RecursiveMode, Watcher};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

pub type WatcherState = Arc<Mutex<Option<notify::RecommendedWatcher>>>;

pub fn new_state() -> WatcherState {
    Arc::new(Mutex::new(None))
}

#[derive(Debug, Serialize, Clone)]
pub struct ChangePayload {
    pub path: String,
    pub kind: &'static str,
}

pub fn start(state: &WatcherState, app: AppHandle, root: &str) -> Result<(), String> {
    // Drop any previous watcher first so its background thread exits cleanly.
    {
        let mut slot = state.lock().map_err(|e| e.to_string())?;
        *slot = None;
    }

    let root_path = Path::new(root);
    if !root_path.exists() {
        return Err(format!("path does not exist: {}", root));
    }

    let root_owned = root.to_string();

    let mut watcher: notify::RecommendedWatcher = notify::recommended_watcher(
        move |res: notify::Result<notify::Event>| {
            let event = match res {
                Ok(e) => e,
                Err(_) => return,
            };
            // Map notify EventKind to a short label we're happy to expose.
            let kind: &'static str = match event.kind {
                EventKind::Create(_) => "created",
                EventKind::Modify(_) => "modified",
                EventKind::Remove(_) => "deleted",
                _ => return,
            };
            for p in event.paths {
                let path_str = p.to_string_lossy().to_string();
                if should_ignore(&path_str, &root_owned) {
                    continue;
                }
                let _ = app.emit("fs:changed", ChangePayload { path: path_str, kind });
            }
        },
    )
    .map_err(|e| format!("create watcher: {}", e))?;

    watcher
        .watch(root_path, RecursiveMode::Recursive)
        .map_err(|e| format!("watch {}: {}", root, e))?;

    let mut slot = state.lock().map_err(|e| e.to_string())?;
    *slot = Some(watcher);
    Ok(())
}

pub fn stop(state: &WatcherState) {
    if let Ok(mut slot) = state.lock() {
        *slot = None;
    }
}

fn should_ignore(path: &str, root: &str) -> bool {
    // Normalize: make the path relative to the repo root for quicker matching.
    let rel = path.strip_prefix(root).unwrap_or(path);
    for seg in rel.split('/') {
        // All of these churn constantly and are never useful to the editor:
        //   .git        — filtering ALL of it avoids a feedback loop where
        //                 our own `git status` creates `.git/index.lock`
        //                 which fires a notify event which triggers another
        //                 `git status` and so on.
        //   target      — Rust build output.
        //   node_modules — npm installs.
        //   .svelte-kit, .next, .nuxt, .cache, .turbo, dist, build — FE tooling.
        match seg {
            ".git" | ".DS_Store" | "target" | "node_modules"
            | ".svelte-kit" | ".next" | ".nuxt" | ".cache" | ".turbo"
            | "dist" | "build" | "out" | ".parcel-cache" | ".vite" => {
                return true;
            }
            _ => {}
        }
    }
    false
}
