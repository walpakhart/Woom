// Worktree management helpers extracted from `+page.svelte` in
// wave-36. Each function takes a `WorktreeDeps` ctx because the
// route file owns the reactive `activeSession` derived + the
// busy/menu setters; this module just does the actual Tauri calls
// + `updateSession` patches.

import { invoke } from '@tauri-apps/api/core';
import { notify, notifyError } from '$lib/state/toaster.svelte';
import { updateSession } from '$lib/state/sessions.svelte';
import type { ClaudeSession } from '$lib/types';

interface WorktreeInfo {
  path: string;
  branch: string | null;
  head: string | null;
  is_main: boolean;
  woom_session: string | null;
}

export interface WorktreeDeps {
  /** Current active session (may be null). */
  getActiveSession(): ClaudeSession | null;
  /** Inherited editor repo path used as a fallback when the session has no cwd. */
  getEditorRepoPath(): string;
  setWorktreeBusy(s: 'creating' | 'removing' | null): void;
  setWorktreeMenuOpen(v: boolean): void;
}

export async function createWorktree(deps: WorktreeDeps): Promise<void> {
  const activeSession = deps.getActiveSession();
  if (!activeSession) return;
  const repo = activeSession.cwd || deps.getEditorRepoPath();
  if (!repo) {
    notify({
      kind: 'warning',
      title: 'No repository picked',
      body: 'Worktrees need a git repo to branch off — open a folder in the Editor or pick one as cwd.',
    });
    return;
  }
  const ok = confirm(
    `Isolate this Claude session in its own git worktree?\n\n` +
      `Woom will create a fresh branch "woom/${activeSession.id.slice(0, 8)}" ` +
      `off your current HEAD and check it out into a private directory.\n\n` +
      `Your main working tree stays untouched. Claude will only write there.`,
  );
  if (!ok) return;
  deps.setWorktreeBusy('creating');
  try {
    const info = await invoke<WorktreeInfo>('worktree_create', {
      repo,
      sessionId: activeSession.id,
      baseRef: null,
    });
    updateSession(activeSession.id, {
      worktreePath: info.path,
      worktreeBranch: info.branch,
      worktreeRepo: repo,
    });
  } catch (e) {
    notifyError(e, { title: 'Failed to create worktree' });
  } finally {
    deps.setWorktreeBusy(null);
  }
}

export async function removeWorktree(deps: WorktreeDeps): Promise<void> {
  const activeSession = deps.getActiveSession();
  if (!activeSession || !activeSession.worktreePath || !activeSession.worktreeRepo) return;
  const branch = activeSession.worktreeBranch ?? '(unknown branch)';
  const ok = confirm(
    `Remove the isolated worktree for this session?\n\n` +
      `Branch ${branch} will be force-deleted along with any uncommitted work ` +
      `inside it. If you want to keep Claude's changes, merge or push the ` +
      `branch first.`,
  );
  if (!ok) return;
  deps.setWorktreeBusy('removing');
  deps.setWorktreeMenuOpen(false);
  try {
    await invoke('worktree_remove', {
      repo: activeSession.worktreeRepo,
      sessionId: activeSession.id,
    });
    updateSession(activeSession.id, {
      worktreePath: null,
      worktreeBranch: null,
      worktreeRepo: null,
    });
  } catch (e) {
    notifyError(e, { title: 'Failed to remove worktree' });
  } finally {
    deps.setWorktreeBusy(null);
  }
}

export async function copyWorktreeBranch(deps: WorktreeDeps): Promise<void> {
  const activeSession = deps.getActiveSession();
  if (!activeSession?.worktreeBranch) return;
  try {
    await navigator.clipboard.writeText(activeSession.worktreeBranch);
  } catch {/* ignore */}
  deps.setWorktreeMenuOpen(false);
}

export async function applyWorktree(deps: WorktreeDeps): Promise<void> {
  const activeSession = deps.getActiveSession();
  if (
    !activeSession ||
    !activeSession.worktreePath ||
    !activeSession.worktreeRepo ||
    !activeSession.worktreeBranch
  ) return;
  const ok = confirm(
    `Apply Claude's work to your current branch?\n\n` +
      `Woom will run \`git merge --no-ff ${activeSession.worktreeBranch}\` in ${activeSession.worktreeRepo} ` +
      `and then remove the isolated worktree.\n\n` +
      `Make sure your main repo is checked out to the branch you want to merge into, ` +
      `and that its working tree is clean. If the merge has conflicts, the worktree stays — resolve conflicts in the main repo, commit, then discard the worktree manually.`,
  );
  if (!ok) return;
  deps.setWorktreeBusy('removing');
  deps.setWorktreeMenuOpen(false);
  try {
    const msg = await invoke<string>('worktree_apply', {
      repo: activeSession.worktreeRepo,
      sessionId: activeSession.id,
    });
    updateSession(activeSession.id, {
      worktreePath: null,
      worktreeBranch: null,
      worktreeRepo: null,
    });
    notify({ kind: 'success', title: 'Worktree applied', body: msg });
  } catch (e) {
    notifyError(e, {
      title: 'Apply failed',
      body: 'Worktree is preserved — resolve conflicts in the main repo, then retry.',
    });
  } finally {
    deps.setWorktreeBusy(null);
  }
}
