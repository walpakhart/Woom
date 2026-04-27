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
import {
  findInstanceAnywhere,
  firstInstanceOfKind,
  addPanelInstance,
  scrollInstanceIntoView
} from '$lib/state/layout.svelte';

export interface OpenFileInEditorOpts {
  /** Preferred editor column id — usually the session's
   *  `linkedToEditorInstanceId`. We check it still exists; if it was
   *  closed we fall back to the first editor in the active workbench
   *  (and, last resort, spawn a new editor). */
  preferInstanceId?: string | null;
}

/** Open `filePath` in an editor column, creating one if needed. Steps:
 *    1. Pick the target editor:
 *         a. `preferInstanceId` if it still exists.
 *         b. else first editor in the active workbench.
 *         c. else spawn a fresh editor in the active workbench.
 *    2. Make sure the editor's repoPath covers `filePath`. If the
 *       current repoPath doesn't have it as a descendant, ask Tauri
 *       for `git_repo_root(filePath)`. If that fails (file isn't in a
 *       repo), fall back to the file's parent directory. Either way
 *       the editor's tree will scope to something that contains the
 *       file, which is what FileTree needs to surface it.
 *    3. Stash `filePath` in the editor's `pendingOpenFile` slot —
 *       EditorView's $effect picks it up and runs the local
 *       `openFile` (which adds a tab + activates it).
 *    4. Scroll the column into view so the user sees the result.
 *
 *  Errors are swallowed — the worst case is "the editor opens its
 *  folder but doesn't focus the file" or "no editor opens at all"; we
 *  prefer that to a noisy notify when the user just wanted to peek at
 *  a path and the lookup hit a transient git failure. */
export async function openFileInEditor(
  filePath: string,
  opts: OpenFileInEditorOpts = {}
): Promise<void> {
  if (!filePath) return;

  let instanceId: string | null = null;
  if (opts.preferInstanceId) {
    const found = findInstanceAnywhere(opts.preferInstanceId);
    if (found && found.inst.kind === 'editor') instanceId = found.inst.id;
  }
  if (!instanceId) {
    const first = firstInstanceOfKind('editor');
    if (first) instanceId = first.id;
  }
  if (!instanceId) {
    instanceId = addPanelInstance('editor');
  }
  if (!instanceId) return;

  const currentRepo = sessionsState.editorInstanceState[instanceId]?.repoPath ?? '';
  const covers = currentRepo
    ? filePath === currentRepo || filePath.startsWith(currentRepo.replace(/\/$/, '') + '/')
    : false;
  if (!covers) {
    // Try git first — preferred because users typically work at the
    // repo root, not in a single-file directory. Falls through to
    // parent-dir if the file isn't tracked / cwd isn't a repo.
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
  void scrollInstanceIntoView(instanceId);
}
