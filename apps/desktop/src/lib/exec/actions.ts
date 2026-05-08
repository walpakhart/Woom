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
  removeAction,
  enqueuePendingActionResult
} from '$lib/state/sessions.svelte';
import { activeInstances } from '$lib/state/layout.svelte';
import { truncInline } from '$lib/format';
import type { ClaudeAction, ClaudeSession } from '$lib/types';

/** Deliver an action outcome to its destination.
 *
 *  Two paths, picked by whether the card has a `waitId`:
 *
 *  1. **Synchronous (waitId set)** — the action card was created in
 *     response to a sidecar IPC request that's still BLOCKING in the
 *     sidecar's `propose_*` MCP tool. We invoke `resolve_action_wait`
 *     so Tauri shoots the resolution back over the Unix socket, the
 *     sidecar's MCP call returns the summary as the tool result, and
 *     the agent reacts to it IN THE SAME TURN. No queue, no
 *     next-turn drain — the agent is still mid-think.
 *
 *  2. **Legacy (no waitId)** — fallback for sessions where IPC
 *     wasn't available (older forge build, socket binding failed,
 *     etc.). Push to `pendingActionResults`; consumers (UI flush +
 *     agent drain) deliver on next quiescent point. This is the old
 *     pre-refactor flow, kept for graceful degradation.
 *
 *  Older versions also called `appendSessionMessage` directly here
 *  which broke mid-stream — see git history. The queue pattern
 *  fixed that; the IPC path supersedes it for the common case. */
function recordActionOutcome(
  sessionId: string,
  kind: 'commit' | 'pr' | 'bash' | 'switch_cwd',
  ok: boolean,
  summary: string,
  waitId?: string
) {
  if (waitId) {
    void invoke('resolve_action_wait', { waitId, ok, summary }).catch((e) => {
      console.warn('[action-ipc] resolve failed for', waitId, e);
    });
    return;
  }
  enqueuePendingActionResult(sessionId, { ok, kind, summary });
}

/** Fired after an action card runs (success or failure). The caller in
 *  +page.svelte uses this to auto-continue the agent's turn — when an
 *  agent proposes a commit / PR / bash, its turn ENDS waiting for the
 *  user's approval; this callback feeds the result back so the agent
 *  can pick up where it left off without the user having to type
 *  "now make the PR" by hand. `summary` is a short prose recap suited
 *  to inject as a follow-up user prompt. */
export type ActionResolvedCallback = (
  sessionId: string,
  action: ClaudeAction,
  result: { ok: boolean; summary: string }
) => void;

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

export async function executeCommit(
  sessionId: string,
  actionId: string,
  onResolved?: ActionResolvedCallback
): Promise<void> {
  const sess = sessionsState.list.find((x) => x.id === sessionId);
  const action = sess?.actions.find((a) => a.id === actionId && a.kind === 'commit');
  if (!sess || !action || action.kind !== 'commit') return;
  const cwd = effectiveCwd(sess);
  if (!cwd) {
    const msg = 'No working directory — pick a folder or enable worktree first.';
    updateAction(sessionId, actionId, { status: 'error', result: msg });
    recordActionOutcome(sessionId, 'commit', false, `commit (${truncInline(action.message, 80)}) skipped: ${msg}`, action.waitId);
    onResolved?.(sessionId, action, { ok: false, summary: msg });
    return;
  }
  updateAction(sessionId, actionId, { status: 'executing' });
  try {
    const status = await invoke<{ files: GitStatusFile[] }>('git_status', { repo: cwd });
    const toStage = status.files.filter((f) => f.unstaged).map((f) => f.path);
    if (toStage.length) {
      await invoke('git_stage', { repo: cwd, paths: toStage });
    }
    const fullMsg = action.body ? `${action.message}\n\n${action.body}` : action.message;
    const cmd = action.push ? 'git_commit_and_push' : 'git_commit';
    const res = await invoke<string>(cmd, { repo: cwd, message: fullMsg });
    updateAction(sessionId, actionId, { status: 'done', result: res });
    const okSummary = `Commit landed (${action.push ? 'pushed' : 'local'}): ${action.message}\n${res}`;
    recordActionOutcome(sessionId, 'commit', true, okSummary, action.waitId);
    onResolved?.(sessionId, action, { ok: true, summary: okSummary });
    setTimeout(() => removeAction(sessionId, actionId), 4000);
  } catch (e) {
    const msg = asMessage(e);
    updateAction(sessionId, actionId, { status: 'error', result: msg });
    const failSummary = `Commit failed (${truncInline(action.message, 80)}): ${msg}`;
    recordActionOutcome(sessionId, 'commit', false, failSummary, action.waitId);
    onResolved?.(sessionId, action, { ok: false, summary: failSummary });
  }
}

export async function executeBash(
  sessionId: string,
  actionId: string,
  appendToTranscript: (sessionId: string, delta: string) => void,
  onResolved?: ActionResolvedCallback
): Promise<void> {
  const sess = sessionsState.list.find((x) => x.id === sessionId);
  const action = sess?.actions.find((a) => a.id === actionId && a.kind === 'bash');
  if (!sess || !action || action.kind !== 'bash') return;
  const cwd = effectiveCwd(sess);
  if (!cwd) {
    const msg = 'No working directory — pick a folder first.';
    updateAction(sessionId, actionId, { status: 'error', result: msg });
    recordActionOutcome(sessionId, 'bash', false, `bash skipped: ${msg}`, action.waitId);
    onResolved?.(sessionId, action, { ok: false, summary: msg });
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
    const output = combined || '(no output)';
    const exitNote = res.ok ? '' : ` _(exit ${res.code})_`;
    appendToTranscript(
      sessionId,
      `\n\n\`$ ${truncInline(action.command, 400)}\`${exitNote}\n\n\`\`\`\n${truncInline(output, 4000)}\n\`\`\`\n\n`
    );
    const summary = `bash \`${truncInline(action.command, 200)}\` exited ${res.code}.\nOutput:\n${truncInline(output, 2000)}`;
    // The bash output already streams into the assistant transcript
    // above (`appendToTranscript`), so the agent sees it on its own
    // turn-end. We still emit a system-message marker so the agent
    // can spot the exit code when scanning the transcript on later
    // turns — without it the inline output is just text and the
    // agent has to infer success from the absence of "exited 1".
    recordActionOutcome(sessionId, 'bash', res.ok, summary, action.waitId);
    onResolved?.(sessionId, action, { ok: res.ok, summary });
    if (res.ok) {
      setTimeout(() => removeAction(sessionId, actionId), 4000);
    }
  } catch (e) {
    const msg = asMessage(e);
    updateAction(sessionId, actionId, { status: 'error', result: msg });
    const failSummary = `bash failed to start (\`${truncInline(action.command, 80)}\`): ${msg}`;
    recordActionOutcome(sessionId, 'bash', false, failSummary, action.waitId);
    onResolved?.(sessionId, action, { ok: false, summary: failSummary });
  }
}

export async function executeSwitchCwd(
  sessionId: string,
  actionId: string,
  onResolved?: ActionResolvedCallback
): Promise<void> {
  const sess = sessionsState.list.find((x) => x.id === sessionId);
  const action = sess?.actions.find((a) => a.id === actionId && a.kind === 'switch_cwd');
  if (!sess || !action || action.kind !== 'switch_cwd') return;
  updateAction(sessionId, actionId, { status: 'executing' });
  try {
    const exists = await invoke<boolean>('fs_path_exists', { path: action.path });
    if (!exists) {
      const msg = `Path does not exist: ${action.path}`;
      updateAction(sessionId, actionId, { status: 'error', result: msg });
      recordActionOutcome(sessionId, 'switch_cwd', false, `cwd switch failed: ${msg}`, action.waitId);
      onResolved?.(sessionId, action, { ok: false, summary: msg });
      return;
    }
    updateSession(sessionId, {
      cwd: action.path,
      worktreePath: null,
      worktreeBranch: null,
      worktreeRepo: null
    });
    updateAction(sessionId, actionId, { status: 'done', result: `Switched to ${action.path}` });
    const okSummary = `cwd switched to ${action.path}.`;
    recordActionOutcome(sessionId, 'switch_cwd', true, okSummary, action.waitId);
    onResolved?.(sessionId, action, { ok: true, summary: okSummary });
    setTimeout(() => removeAction(sessionId, actionId), 3000);
  } catch (e) {
    const msg = asMessage(e);
    updateAction(sessionId, actionId, { status: 'error', result: msg });
    const failSummary = `cwd switch failed (${action.path}): ${msg}`;
    recordActionOutcome(sessionId, 'switch_cwd', false, failSummary, action.waitId);
    onResolved?.(sessionId, action, { ok: false, summary: failSummary });
  }
}

export async function executePr(
  sessionId: string,
  actionId: string,
  onResolved?: ActionResolvedCallback
): Promise<void> {
  const sess = sessionsState.list.find((x) => x.id === sessionId);
  const action = sess?.actions.find((a) => a.id === actionId && a.kind === 'pr');
  if (!sess || !action || action.kind !== 'pr') return;
  const cwd = effectiveCwd(sess);
  if (!cwd) {
    const msg = 'No working directory — pick a folder first.';
    updateAction(sessionId, actionId, { status: 'error', result: msg });
    recordActionOutcome(sessionId, 'pr', false, `PR creation skipped: ${msg}`, action.waitId);
    onResolved?.(sessionId, action, { ok: false, summary: msg });
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
    const okSummary = `PR opened: ${action.title}\n${url}`;
    recordActionOutcome(sessionId, 'pr', true, okSummary, action.waitId);
    onResolved?.(sessionId, action, { ok: true, summary: okSummary });
    // Auto-dismiss successful PR cards too — without this, a chained
    // commit→PR continuation would re-include the done PR card in
    // every subsequent recap (recentActionSummaries filters by status,
    // not recency), giving the agent an ever-growing "things you
    // already did" list. Keep the URL visible briefly so the user
    // sees it before it clears.
    setTimeout(() => removeAction(sessionId, actionId), 6000);
  } catch (e) {
    const msg = asMessage(e);
    updateAction(sessionId, actionId, { status: 'error', result: msg });
    const failSummary = `PR creation failed (${truncInline(action.title, 80)}): ${msg}`;
    recordActionOutcome(sessionId, 'pr', false, failSummary, action.waitId);
    onResolved?.(sessionId, action, { ok: false, summary: failSummary });
  }
}

/** Dispatch by kind. Each ClaudeActionCard's approve button funnels here so
 *  the column component doesn't have to know which backend function runs
 *  for each action kind. `onResolved` fires once the underlying execute
 *  finishes — the page-level continuation logic uses it to auto-resume
 *  the agent's turn with the result. */
export function dispatchAction(
  sessionId: string,
  action: ClaudeAction,
  appendToTranscript: (sessionId: string, delta: string) => void,
  onResolved?: ActionResolvedCallback
): void {
  if (action.kind === 'commit') void executeCommit(sessionId, action.id, onResolved);
  else if (action.kind === 'pr') void executePr(sessionId, action.id, onResolved);
  else if (action.kind === 'switch_cwd') void executeSwitchCwd(sessionId, action.id, onResolved);
  else if (action.kind === 'bash') void executeBash(sessionId, action.id, appendToTranscript, onResolved);
}
