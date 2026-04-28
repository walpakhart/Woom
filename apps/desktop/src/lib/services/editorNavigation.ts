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
 *    2. ONLY set the editor's repoPath when the editor has none yet —
 *       i.e. it was just spawned, or has never had a folder opened.
 *       In that case we resolve `git_repo_root(filePath)` (or fall
 *       back to the file's parent directory) and use that as the
 *       initial root.
 *
 *       We deliberately do NOT touch the repoPath when the editor
 *       already has one, even if it doesn't cover `filePath`. Users
 *       click file paths from agent output expecting to *peek* at the
 *       file in a tab — clobbering FileTree's root would lose their
 *       navigation context (and was a real bug — clicking a path from
 *       another repo dragged the editor into that repo and left the
 *       user looking at the wrong file tree).
 *    3. Stash `filePath` in the editor's `pendingOpenFile` slot —
 *       EditorView's $effect picks it up and runs the local
 *       `openFile` (which adds a tab + activates it). Tabs are
 *       repoPath-independent, so files outside the current root just
 *       show up as standalone tabs without affecting the tree.
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
  // Bootstrap repoPath only when the editor is empty. If the user
  // already has a folder open, leave it alone — files outside the root
  // open as orphan tabs, which is what users expect ("I'm browsing repo
  // A, let me peek at one file from repo B without losing my tree").
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
  void scrollInstanceIntoView(instanceId);
}
