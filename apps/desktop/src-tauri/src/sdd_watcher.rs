//! Filesystem watcher + event-emit helpers for SDD workspaces.
//! Extracted from `sdd.rs` in wave-7 split.
//!
//! `ensure_watcher` boots one OS thread per base directory; the thread
//! drains `notify` events, debounces them per workspace (250 ms),
//! rebuilds the touched workspace from disk, and emits
//! `sdd:changed:<id>` (per-workspace) + `sdd:changed` (broad). The
//! SddCard subscribes to the targeted event, the future "all
//! workspaces" library view will subscribe to the broad one.
//!
//! Watching the BASE dir recursively (vs per-workspace) lets us handle
//! "new workspace created" + "old workspace removed" without re-arming
//! the watcher each time — the base dir is single-purpose so there
//! are no other write sources to filter against.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tauri::{AppHandle, Emitter};

use crate::sdd::SharedWorkspaces;
use crate::sdd_hydrate::rebuild_from_disk;

/// Per-workspace targeted event + a broad "something changed" event
/// for any "list of all workspaces" UI we add later (none right now).
pub(crate) fn emit_changed(app: &AppHandle, id: &str) {
    let _ = app.emit(&format!("sdd:changed:{id}"), &id);
    let _ = app.emit("sdd:changed", &id);
}

/// Boot the filesystem watcher on first workspace creation. Spawns
/// one background OS thread that drains `notify` events, debounces
/// them per workspace, rebuilds the workspace from disk, and emits
/// `sdd:changed:<id>`.
///
/// Debounce: 250 ms per workspace. The agent often writes a file then
/// updates frontmatter immediately after; without debouncing we'd
/// rebuild twice in quick succession and the UI would flicker.
pub(crate) fn ensure_watcher(
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
