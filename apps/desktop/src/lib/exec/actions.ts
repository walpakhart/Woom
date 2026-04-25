// Action executors — one per `ClaudeAction` kind. Pulled out of +page.svelte
// so each function is independently testable and the god component shrinks
// by ~150 LOC. Errors land back on the action card via `updateAction(...,
// status: 'error')` — no global toast — so the chat row stays the visible
// transcript of what happened.
//
// `appendToTranscript` is supplied by the caller because rendering bash
// output inline in the assistant turn touches DOM-scroll state that lives
// in the component layer.

import { invoke } from '@tauri-apps/api/core';
import {
  sessionsState,
  updateSession,
  updateAction,
  removeAction
} from '$lib/state/sessions.svelte';
import { activeInstances } from '$lib/state/layout.svelte';
import { truncInline } from '$lib/format';
import type { ClaudeAction, ClaudeSession } from '$lib/types';

/** Resolve the working directory the action should run in. Order:
 *  worktree → explicit cwd → first editor's open repo → null. */
export function effectiveCwd(s: ClaudeSession): string | null {
  if (s.worktreePath) return s.worktreePath;
  if (s.cwd) return s.cwd;
  const first = activeInstances().find((i) => i.kind === 'editor');
  const editor = first ? sessionsState.editorInstanceState[first.id]?.repoPath : null;
  return editor && editor.length > 0 ? editor : null;
}

/** Convert any thrown value into a string fit for the action card.
 *  Tauri rejections are usually plain strings; Errors are stringified. */
function asMessage(e: unknown): string {
  return typeof e === 'string' ? e : e instanceof Error ? e.message : String(e);
}

interface GitStatusFile { path: string; unstaged: boolean; staged: boolean }

export async function executeCommit(sessionId: string, actionId: string): Promise<void> {
  const sess = sessionsState.list.find((x) => x.id === sessionId);
  const action = sess?.actions.find((a) => a.id === actionId && a.kind === 'commit');
  if (!sess || !action || action.kind !== 'commit') return;
  const cwd = effectiveCwd(sess);
  if (!cwd) {
    updateAction(sessionId, actionId, {
      status: 'error',
      result: 'No working directory — pick a folder or enable worktree first.'
    });
    return;
  }
  updateAction(sessionId, actionId, { status: 'executing' });
  try {
    // Stage every dirty path before commit (Forgehold-style: full stage).
    const status = await invoke<{ files: GitStatusFile[] }>('git_status', { repo: cwd });
    const toStage = status.files.filter((f) => f.unstaged).map((f) => f.path);
    if (toStage.length) {
      await invoke('git_stage', { repo: cwd, paths: toStage });
    }
    const fullMsg = action.body ? `${action.message}\n\n${action.body}` : action.message;
    const cmd = action.push ? 'git_commit_and_push' : 'git_commit';
    const res = await invoke<string>(cmd, { repo: cwd, message: fullMsg });
    updateAction(sessionId, actionId, { status: 'done', result: res });
    // Auto-dismiss successful commits so the chat stays tidy.
    setTimeout(() => removeAction(sessionId, actionId), 4000);
  } catch (e) {
    updateAction(sessionId, actionId, { status: 'error', result: asMessage(e) });
  }
}

export async function executeBash(
  sessionId: string,
  actionId: string,
  appendToTranscript: (sessionId: string, delta: string) => void
): Promise<void> {
  const sess = sessionsState.list.find((x) => x.id === sessionId);
  const action = sess?.actions.find((a) => a.id === actionId && a.kind === 'bash');
  if (!sess || !action || action.kind !== 'bash') return;
  const cwd = effectiveCwd(sess);
  if (!cwd) {
    updateAction(sessionId, actionId, {
      status: 'error',
      result: 'No working directory — pick a folder first.'
    });
    return;
  }
  updateAction(sessionId, actionId, { status: 'executing' });
  try {
    const res = await invoke<{ stdout: string; stderr: string; code: number; ok: boolean }>(
      'fs_bash_run',
      { cwd, command: action.command }
    );
    const combined = [res.stdout, res.stderr].filter(Boolean).join('\n').trim();
    updateAction(sessionId, actionId, {
      status: res.ok ? 'done' : 'error',
      result: combined || '(no output)',
      exitCode: res.code
    });
    // Render `$ command` + output inline in Claude's last assistant turn so
    // the transcript reads as a continuous flow.
    const output = combined || '(no output)';
    const exitNote = res.ok ? '' : ` _(exit ${res.code})_`;
    appendToTranscript(
      sessionId,
      `\n\n\`$ ${truncInline(action.command, 400)}\`${exitNote}\n\n\`\`\`\n${truncInline(output, 4000)}\n\`\`\`\n\n`
    );
    if (res.ok) {
      setTimeout(() => removeAction(sessionId, actionId), 4000);
    }
  } catch (e) {
    updateAction(sessionId, actionId, { status: 'error', result: asMessage(e) });
  }
}

export async function executeSwitchCwd(sessionId: string, actionId: string): Promise<void> {
  const sess = sessionsState.list.find((x) => x.id === sessionId);
  const action = sess?.actions.find((a) => a.id === actionId && a.kind === 'switch_cwd');
  if (!sess || !action || action.kind !== 'switch_cwd') return;
  updateAction(sessionId, actionId, { status: 'executing' });
  try {
    const exists = await invoke<boolean>('fs_path_exists', { path: action.path });
    if (!exists) {
      updateAction(sessionId, actionId, { status: 'error', result: `Path does not exist: ${action.path}` });
      return;
    }
    // Drop any worktree override — the user is switching to a new location.
    updateSession(sessionId, {
      cwd: action.path,
      worktreePath: null,
      worktreeBranch: null,
      worktreeRepo: null
    });
    updateAction(sessionId, actionId, { status: 'done', result: `Switched to ${action.path}` });
    setTimeout(() => removeAction(sessionId, actionId), 3000);
  } catch (e) {
    updateAction(sessionId, actionId, { status: 'error', result: asMessage(e) });
  }
}

export async function executePr(sessionId: string, actionId: string): Promise<void> {
  const sess = sessionsState.list.find((x) => x.id === sessionId);
  const action = sess?.actions.find((a) => a.id === actionId && a.kind === 'pr');
  if (!sess || !action || action.kind !== 'pr') return;
  const cwd = effectiveCwd(sess);
  if (!cwd) {
    updateAction(sessionId, actionId, {
      status: 'error',
      result: 'No working directory — pick a folder first.'
    });
    return;
  }
  updateAction(sessionId, actionId, { status: 'executing' });
  try {
    const url = await invoke<string>('git_create_pr', {
      repo: cwd,
      title: action.title,
      body: action.body,
      draft: action.draft,
      base: action.base.trim() || null
    });
    updateAction(sessionId, actionId, { status: 'done', result: url });
  } catch (e) {
    updateAction(sessionId, actionId, { status: 'error', result: asMessage(e) });
  }
}

/** Dispatch by kind. Each ClaudeActionCard's approve button funnels here so
 *  the column component doesn't have to know which backend function runs
 *  for each action kind. */
export function dispatchAction(
  sessionId: string,
  action: ClaudeAction,
  appendToTranscript: (sessionId: string, delta: string) => void
): void {
  if (action.kind === 'commit') void executeCommit(sessionId, action.id);
  else if (action.kind === 'pr') void executePr(sessionId, action.id);
  else if (action.kind === 'switch_cwd') void executeSwitchCwd(sessionId, action.id);
  else if (action.kind === 'bash') void executeBash(sessionId, action.id, appendToTranscript);
}
