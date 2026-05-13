// Minimal "open this file in an editor column" plumbing.
//
// Just dispatches an open request to a specific editor instance.
// Does NOT:
//   - bootstrap the editor's repoPath if it's empty (caller's job)
//   - switch the rail's active instance (caller's job)
//   - switch the top-level view (caller's job)
//
// Caller is responsible for picking the right instance, verifying the
// file lives under that instance's repoPath, and any UI side-effects.
// This keeps surprises out: clicking a file mention never silently
// re-roots the editor.

import { requestEditorOpenFile } from '$lib/state/sessions.svelte';
import { APP_INSTANCE_IDS } from '$lib/state/layout.svelte';

export interface OpenFileInEditorOpts {
  /** Editor instance to receive the open request. Defaults to the
   *  primary editor singleton. */
  preferInstanceId?: string | null;
}

export function openFileInEditor(
  filePath: string,
  opts: OpenFileInEditorOpts = {}
): void {
  if (!filePath) return;
  const instanceId = opts.preferInstanceId ?? APP_INSTANCE_IDS.editor;
  requestEditorOpenFile(instanceId, filePath);
}
