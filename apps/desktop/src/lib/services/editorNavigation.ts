// Cross-component "open this file in an editor column" plumbing. The
// diff card's clickable file path, MCP-driven `open_file` calls (future),
// and any other "go to file" surface should funnel through this single
// helper so they all pick the same target editor + use the same "create
// the column if needed" / "set repo root" / "scroll into view" sequence.
//
// Why a helper and not a direct write to sessionsState: choosing WHICH
// editor instance handles the request needs three pieces of state
// (sessions, layout, and current Tauri-known git repo root for the
// path), all of which are awkward to read together inline in a Svelte
// component. Bundling the resolution here keeps callers to a single
// async call.

import { invoke } from '@tauri-apps/api/core';
import {
  sessionsState,
  requestEditorOpenFile
} from '$lib/state/sessions.svelte';
import { APP_INSTANCE_IDS } from '$lib/state/layout.svelte';

export interface OpenFileInEditorOpts {
  /** Kept for API compat with callers that used to pass a preferred
   *  editor instance id. App mode has only one editor — the value is
   *  ignored when it doesn't match the singleton id. */
  preferInstanceId?: string | null;
}

/** Open `filePath` in the editor solo singleton.
 *
 *  When the editor has no repoPath yet, we resolve `git_repo_root` for
 *  the file and use it as the initial root. If the editor already has
 *  a repoPath, we leave it alone — files outside the root open as
 *  orphan tabs, which is what users want ("peek at one file from repo
 *  B without losing my tree in repo A").
 *
 *  Errors are swallowed: the worst case is "no folder gets bootstrapped"
 *  or "the file just opens as an orphan tab" — preferable to a noisy
 *  toast when the user just wanted to peek at a path. */
export async function openFileInEditor(
  filePath: string,
  _opts: OpenFileInEditorOpts = {}
): Promise<void> {
  if (!filePath) return;

  const instanceId = APP_INSTANCE_IDS.editor;

  const currentRepo = sessionsState.editorInstanceState[instanceId]?.repoPath ?? '';
  if (!currentRepo) {
    let nextRoot = '';
    try {
      nextRoot = await invoke<string>('git_repo_root', { path: filePath });
    } catch {
      /* not a repo — handled below */
    }
    if (!nextRoot) {
      const slash = filePath.lastIndexOf('/');
      nextRoot = slash > 0 ? filePath.slice(0, slash) : filePath;
    }
    const slot = sessionsState.editorInstanceState[instanceId];
    if (!slot) {
      sessionsState.editorInstanceState[instanceId] = { repoPath: nextRoot };
    } else {
      sessionsState.editorInstanceState = {
        ...sessionsState.editorInstanceState,
        [instanceId]: { ...slot, repoPath: nextRoot }
      };
    }
  }

  requestEditorOpenFile(instanceId, filePath);
}
