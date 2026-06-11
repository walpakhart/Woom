// Fork-compact for chat sessions. Asks the live CLI session to
// summarise itself, mints a fresh session UUID, seeds it with the
// summary, swaps the session over so the next turn resumes the new
// compacted thread. See `agent::compact_session` (Rust) for the
// two-shot details. We always trust the returned `new_uuid` and
// stamp THAT on the session.
//
// The only component-local pieces the caller threads through are
// `editorRepoPath` (a $derived reactive value over editor instances)
// and `scrollChatBottom` (DOM-coupled — needs the chat container
// ref). Everything else is module state.

import { invoke } from '@tauri-apps/api/core';

import {
  appendSessionMessage,
  genUuid,
  sessionsState,
  updateSession
} from '$lib/state/sessions.svelte';
import { COMPACT_SUMMARY_MARKER } from '$lib/services/sessionCwd';

export interface RunCompactOpts {
  /** Fallback cwd when the session has no worktree / no explicit cwd
   *  / no linked editor's repoPath. Component-local in +page.svelte
   *  (the user's currently-focused editor's path). */
  editorRepoPath: string | null;
  /** Scroll the chat thread to the latest message after the system-
   *  message gets appended. DOM-coupled, so callers must supply. */
  scrollChatBottom: () => void | Promise<void>;
}

/** Surface the compact summary in chat as a system message so the user
 *  can audit what the new session was seeded with. */
export async function runCompactSession(
  sessionId: string,
  opts: RunCompactOpts
): Promise<void> {
  const s = sessionsState.list.find((x) => x.id === sessionId);
  if (!s) return;
  const proposedNewUuid = genUuid();
  const cwd = s.worktreePath || s.cwd
    || (s.linkedToEditor && s.linkedToEditorInstanceId
      ? sessionsState.editorInstanceState[s.linkedToEditorInstanceId]?.repoPath ?? null
      : null)
    || opts.editorRepoPath
    || null;
  // Compact runs on the same model the user picked for normal turns.
  const model = s.claudeModel;
  const result = await invoke<{ new_uuid: string; summary: string }>(
    'agent_compact_session',
    {
      oldUuid: s.claudeUuid,
      proposedNewUuid,
      cwd,
      model
    }
  );
  // Swap the session over to whatever uuid the backend returned +
  // reset context-window counter (the new session starts with just
  // the summary, so its first turn's context size will be small).
  updateSession(sessionId, {
    claudeUuid: result.new_uuid,
    claudeResumable: true,
    lastContextSize: 0
  });
  // Marker prefix is shared with `extractCompactSummary` in sessionCwd
  // — keeping them on a single constant lets cwd-switch recap and
  // orphan-recovery recap pick up this summary as the older-history
  // layer when the user later switches projects or the CLI loses its
  // store. Cheap insurance against the two strings drifting apart.
  appendSessionMessage(sessionId, {
    role: 'system',
    content: `${COMPACT_SUMMARY_MARKER}\n\n${result.summary}`,
    at: new Date().toISOString()
  });
  void opts.scrollChatBottom();
}
