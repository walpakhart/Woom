// Session-link helpers extracted from `+page.svelte` in wave-37.
// Each helper either reads/mutates `activeSession` (via deps) or
// links a specific sessionId; the route file keeps thin shims that
// reach into the reactive derived value + the layout-state ref.
//
// `linkSessionToTerminal` includes the eager PTY spawn so the agent's
// `mcp__app__terminal_*` calls hit a live session immediately — this
// is the historically-buggy code path the comment at the call site
// explains.

import {
  newClaudeSession,
  sessionsState,
  setActiveSessionInInstance,
  updateSession,
} from '$lib/state/sessions.svelte';
import { applySessionCwd } from '$lib/services/sessionCwd';
import { dropPrewarm } from '$lib/exec/claude';
import { ensureTerminalSession } from '$lib/state/terminals.svelte';
import {
  APP_INSTANCE_IDS,
  kindForInstanceId,
  layoutState,
} from '$lib/state/layout.svelte';
import type { ClaudeSession } from '$lib/types';

export interface SessionLinkDeps {
  /** Route-local reactive `activeSession` value (may be null). */
  getActiveSession(): ClaudeSession | null;
  /** Route-local editor-repo-path setter — needed by syncLinkedEditorToAgent. */
  setEditorRepoPath(path: string, instanceId?: string): void;
}

/** Link the active chat session to an editor instance WITHOUT touching
 *  either side's folder. If the agent's cwd and the editor's repoPath
 *  diverge, the WorktreeBar shows an orange mismatch button — the user
 *  explicitly chooses which side wins. This avoids the old "auto-push"
 *  surprise where picking an editor silently overwrote either side.
 *  When the agent has no folder of its own (first link, no worktree),
 *  we still pull the editor's folder in so the session has something
 *  to work with. */
export function linkActiveSessionToEditor(editorInstanceId: string, deps: SessionLinkDeps): void {
  const activeSession = deps.getActiveSession();
  if (!activeSession) return;
  const editorPath =
    sessionsState.editorInstanceState[editorInstanceId]?.repoPath ?? '';
  const hasOwnFolder = !!(activeSession.worktreePath || activeSession.cwd);
  const patch: Partial<typeof activeSession> = {
    linkedToEditor: true,
    linkedToEditorInstanceId: editorInstanceId,
  };
  if (!hasOwnFolder && editorPath) {
    patch.cwd = editorPath;
  }
  updateSession(activeSession.id, patch);
}

/** Sync the active session's cwd to its linked editor's repoPath.
 *  Wired to the "Use editor folder" choice in WorktreeBar's mismatch
 *  menu. Uses `applySessionCwd` (not raw updateSession) so the CLI
 *  uuid rotates and the next turn's prompt gets a cwd-switch recap. */
export function syncAgentToLinkedEditor(deps: SessionLinkDeps): void {
  const activeSession = deps.getActiveSession();
  if (!activeSession?.linkedToEditorInstanceId) return;
  const editorPath =
    sessionsState.editorInstanceState[activeSession.linkedToEditorInstanceId]?.repoPath ?? '';
  if (!editorPath) return;
  applySessionCwd(activeSession.id, editorPath);
  void dropPrewarm(activeSession.id);
}

/** Sync the linked editor's repoPath to the active session's cwd /
 *  worktree. Wired to the "Use agent folder" choice in WorktreeBar's
 *  mismatch menu. */
export function syncLinkedEditorToAgent(deps: SessionLinkDeps): void {
  const activeSession = deps.getActiveSession();
  if (!activeSession?.linkedToEditorInstanceId) return;
  const agentPath = activeSession.worktreePath || activeSession.cwd || '';
  if (!agentPath) return;
  deps.setEditorRepoPath(agentPath, activeSession.linkedToEditorInstanceId);
}

export function toggleSessionEditorLink(deps: SessionLinkDeps): void {
  const activeSession = deps.getActiveSession();
  if (!activeSession) return;
  if (activeSession.linkedToEditor) {
    updateSession(activeSession.id, {
      linkedToEditor: false,
      linkedToEditorInstanceId: null,
    });
  } else {
    linkActiveSessionToEditor(layoutState.activeInstance.editor, deps);
  }
}

/** Bind the active session to a specific terminal instance from the
 *  cwd-bar's "Link terminal…" picker. */
export function linkActiveSessionToTerminal(terminalInstanceId: string, deps: SessionLinkDeps): void {
  const activeSession = deps.getActiveSession();
  if (!activeSession) return;
  linkSessionToTerminal(terminalInstanceId, activeSession.id);
}

/** Drop the active session's terminal link. */
export function toggleSessionTerminalLink(deps: SessionLinkDeps): void {
  const activeSession = deps.getActiveSession();
  if (!activeSession) return;
  if (activeSession.linkedTerminalInstanceId) {
    unlinkSessionFromTerminal(activeSession.id);
  } else {
    linkSessionToTerminal(layoutState.activeInstance.terminal, activeSession.id);
  }
}

/** Link a chat session to this terminal instance — mirror of
 *  `linkEditorToAgent` but for the terminal side. After linking, the
 *  session's MCP `terminal_run` / `terminal_write` default to this
 *  terminal id, AND selecting text in this terminal will surface
 *  an "Apply to <agent>" chip wired to the same session. */
export function linkSessionToTerminal(terminalInstanceId: string, sessionId: string): void {
  const sess = sessionsState.list.find((s) => s.id === sessionId);
  if (!sess) return;
  /* Floating sessions (agentInstanceId === null) need a canonical
     agent-app id so the terminal's inline-agents pane can render
     their card AND surface "Apply to <agent>" — both consumers
     require a non-null id to resolve which app to route into. */
  const patch: Partial<typeof sess> = { linkedTerminalInstanceId: terminalInstanceId };
  if (!sess.agentInstanceId) patch.agentInstanceId = APP_INSTANCE_IDS[sess.agentKind];
  updateSession(sessionId, patch);
  /* Eager-spawn the PTY so the agent's `mcp__app__terminal_*` calls
     hit a live session immediately — previously the PTY only spawned
     on first surface mount, so an agent linked through the cwd-bar
     (without the user opening the Terminal solo) saw an empty
     `terminal_list` and bounced off. */
  const layoutName =
    layoutState.instances.terminal.find((i) => i.id === terminalInstanceId)?.name ?? null;
  const editorCwd =
    sess.linkedToEditor && sess.linkedToEditorInstanceId
      ? sessionsState.editorInstanceState[sess.linkedToEditorInstanceId]?.repoPath ?? null
      : null;
  const spawnCwd = editorCwd ?? layoutState.active.terminal.cwd ?? null;
  void ensureTerminalSession(terminalInstanceId, spawnCwd, 120, 32, layoutName);
}

export function unlinkSessionFromTerminal(sessionId: string): void {
  updateSession(sessionId, { linkedTerminalInstanceId: null });
}

export function linkActiveSessionToCanvas(canvasId: string, deps: SessionLinkDeps): void {
  const activeSession = deps.getActiveSession();
  if (!activeSession) return;
  updateSession(activeSession.id, { linkedCanvasId: canvasId });
}

export function toggleSessionCanvasLink(deps: SessionLinkDeps): void {
  const activeSession = deps.getActiveSession();
  if (!activeSession) return;
  updateSession(activeSession.id, { linkedCanvasId: null });
}

/** Initiate a link from the Editor side. Always links the *currently
 *  active* session in the target agent column — never spawns a new chat. */
export function linkEditorToAgent(
  editorInstanceId: string,
  agentInstanceId: string,
  sessionId?: string,
): void {
  const editorPath = sessionsState.editorInstanceState[editorInstanceId]?.repoPath || '';
  if (!editorPath) return;
  const kind = kindForInstanceId(agentInstanceId);
  if (kind !== 'claude' && kind !== 'cursor') return;
  const explicit = sessionId
    ? sessionsState.list.find((s) => s.id === sessionId) ?? null
    : null;
  function patchForLink(sess: { worktreePath?: string | null; cwd?: string | null }) {
    const hasOwn = !!(sess.worktreePath || sess.cwd);
    const base = {
      linkedToEditor: true,
      linkedToEditorInstanceId: editorInstanceId,
      agentInstanceId,
    } as const;
    return hasOwn ? base : { ...base, cwd: editorPath };
  }
  if (explicit) {
    setActiveSessionInInstance(agentInstanceId, explicit.id);
    updateSession(explicit.id, patchForLink(explicit));
    return;
  }
  const currentId = sessionsState.activeByInstance[agentInstanceId] ?? null;
  const current = currentId
    ? sessionsState.list.find((s) => s.id === currentId) ?? null
    : null;
  if (current) {
    updateSession(current.id, patchForLink(current));
  } else {
    newClaudeSession({
      agentKind: kind,
      cwd: editorPath,
      linkedToEditor: true,
      linkedToEditorInstanceId: editorInstanceId,
      agentInstanceId: agentInstanceId,
    });
  }
}
