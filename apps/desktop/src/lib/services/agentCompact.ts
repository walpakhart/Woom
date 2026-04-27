// Fork-compact for Claude chat sessions. Asks the live CLI session to
// summarise itself, mints a fresh session UUID, seeds it with the
// summary, swaps the session over so the next turn resumes the new
// compacted thread. See `claude::compact_session` (Rust) for the
// two-shot details.
//
// Pulled out of +page.svelte. The only component-local pieces the
// caller has to thread through are `editorRepoPath` (a $derived
// reactive value over editor instances) and `scrollChatBottom` (DOM-
// coupled — needs the chat container ref). Everything else is module
// state.

import { invoke } from '@tauri-apps/api/core';

import {
  appendSessionMessage,
  genUuid,
  sessionsState,
  updateSession
} from '$lib/state/sessions.svelte';

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
  const newUuid = genUuid();
  const cwd = s.worktreePath || s.cwd
    || (s.linkedToEditor && s.linkedToEditorInstanceId
      ? sessionsState.editorInstanceState[s.linkedToEditorInstanceId]?.repoPath ?? null
      : null)
    || opts.editorRepoPath
    || null;
  const result = await invoke<{ new_uuid: string; summary: string }>(
    'claude_compact_session',
    {
      oldClaudeUuid: s.claudeUuid,
      newClaudeUuid: newUuid,
      cwd,
      claudeModel: s.claudeModel
    }
  );
  // Swap the session over to the new uuid + reset context-window
  // counter (the new session starts with just the summary, so its
  // first turn's context size will be small).
  updateSession(sessionId, {
    claudeUuid: result.new_uuid,
    claudeResumable: true,
    lastContextSize: 0
  });
  appendSessionMessage(sessionId, {
    role: 'system',
    content: `Compacted earlier conversation. New session seeded with this summary:\n\n${result.summary}`,
    at: new Date().toISOString()
  });
  void opts.scrollChatBottom();
}
