// Agent-turn helpers extracted from `+page.svelte` in wave-38.
// Covers the executeAction / dismissAction / onActionResolved /
// continueAgentTurn cluster — every approval-card outcome flows
// through here. `continuationInFlight` is a module-local guard so
// two cards finishing in the same microtask don't both fire the
// next turn (would produce duplicate agent recaps).

import { invoke } from '@tauri-apps/api/core';
import {
  appendSessionMessage,
  drainPendingActionResultsForAgent,
  flushActionResultsToUI,
  formatActionResultsForPrompt,
  removeAction,
  replaceLastAssistant,
  sessionsState,
  updateSession,
} from '$lib/state/sessions.svelte';
import { buildAgentAppContext } from '$lib/services/agentContext';
import { runAgentRequest } from '$lib/exec/claude';
import { dispatchAction } from '$lib/exec/actions';
import { notifyError } from '$lib/state/toaster.svelte';
import { appHasFocus, notifyClaudeRunComplete } from '$lib/notifications';
import type { ClaudeAction, ClaudeSession } from '$lib/types';

/** Per-session re-entry guard. Two cards finishing in the same
 *  microtask both pass the `stillBusy=false` check and would each
 *  fire `continueAgentTurn`, so the agent gets the same recap twice
 *  and produces a duplicate turn. The Set tracks "continuation
 *  already fired for this batch" — entries are cleared when
 *  continueAgentTurn finishes (in finally) so the next user-initiated
 *  batch can fire. */
const continuationInFlight = new Set<string>();

export interface AgentTurnDeps {
  /** Current editor repo path — fallback cwd when the session lacks one. */
  getEditorRepoPath(): string;
  /** Route-local thinking-timer hooks. */
  startThinkingTimer(sessionId: string): void;
  stopThinkingTimer(sessionId: string): void;
  /** Route-local streaming delta + scroll helpers. */
  appendAssistantDelta(sessionId: string, delta: string): void;
  scrollChatBottom(): Promise<void> | void;
  /** Route-local app-navigation dispatcher (MCP `mcp__app__*` cases). */
  handleAppNavigation(sessionId: string, name: string, input: Record<string, unknown>): void;
}

export function executeAction(
  sessionId: string,
  action: ClaudeAction,
  deps: AgentTurnDeps,
): void {
  dispatchAction(sessionId, action, (sid, a, r) => onActionResolved(sid, a, r, deps));
}

/** Drop an action card AND tell the sidecar's IPC waiter that the
 *  user dismissed (so its blocking MCP call returns and the agent
 *  can react to "user said no" in the same turn instead of hanging
 *  the CLI on a never-arriving response). Cards without a waitId
 *  (legacy fire-and-forget path) just get removed locally. */
export function dismissAction(sessionId: string, actionId: string): void {
  const sess = sessionsState.list.find((s) => s.id === sessionId);
  const a = sess?.actions.find((x) => x.id === actionId);
  const waitId = a?.waitId;
  removeAction(sessionId, actionId);
  if (waitId) {
    void invoke('resolve_action_wait', {
      waitId,
      ok: false,
      summary:
        'User dismissed the card. The action was not run. Decide whether to propose a different approach or stop and ask the user what they want.',
    }).catch((e) => console.warn('[action-ipc] dismiss resolve failed', e));
  }
}

/** Called by every executeXxx after the action ran. The executor
 *  already pushed the outcome onto the pending-action-results
 *  queue. All this hook does is decide whether to AUTO-FIRE the
 *  next agent turn so the user doesn't have to type "continue" by
 *  hand. */
export function onActionResolved(
  sessionId: string,
  _action: ClaudeAction,
  _result: { ok: boolean; summary: string },
  deps: AgentTurnDeps,
): void {
  const sess = sessionsState.list.find((s) => s.id === sessionId);
  if (!sess) return;
  const stillBusy = sess.actions.some(
    (a) => a.status === 'pending' || a.status === 'executing',
  );
  if (stillBusy) return;
  if (continuationInFlight.has(sessionId)) return;
  const lastMsg = sess.messages[sess.messages.length - 1];
  const lastErrored =
    lastMsg?.role === 'assistant' &&
    (lastMsg.content.startsWith('**Claude failed:') ||
      lastMsg.content.startsWith('**Cursor failed:'));
  if (lastErrored) return;
  continuationInFlight.add(sessionId);
  if (sess.awaitingApproval) {
    updateSession(sessionId, { awaitingApproval: false });
  }
  void continueAgentTurn(sessionId, deps);
}

/** Re-enter `runAgentRequest` for an auto-continuation. The prompt
 *  is built from the pending-action-results queue, drained inside
 *  this function — same code path the manual `sendClaudeMessage`
 *  uses, so the agent sees identical "since-last-turn outcomes"
 *  formatting whether it picks up automatically or after the user
 *  types something. */
export async function continueAgentTurn(sessionId: string, deps: AgentTurnDeps): Promise<void> {
  const sess = sessionsState.list.find((s) => s.id === sessionId);
  if (!sess || sess.sending) {
    continuationInFlight.delete(sessionId);
    return;
  }
  const drained = drainPendingActionResultsForAgent(sessionId);
  if (drained.length === 0) {
    continuationInFlight.delete(sessionId);
    return;
  }
  const prompt = formatActionResultsForPrompt(drained);
  const kind = (sess.agentKind ?? 'claude') as 'claude' | 'cursor';
  updateSession(sessionId, { sending: true });
  appendSessionMessage(sessionId, {
    role: 'assistant',
    content: '',
    at: new Date().toISOString(),
  });
  deps.startThinkingTimer(sessionId);
  const runStartedAt = Date.now();
  void deps.scrollChatBottom();

  const cwd = sess.worktreePath || sess.cwd || deps.getEditorRepoPath() || null;
  const claudeUuid = sess.claudeUuid;
  const resume = Boolean(sess.claudeResumable);
  const rules = sessionsState.userRules.trim();
  const agentKind = sess.agentKind;
  const cursorModel = agentKind === 'cursor' ? sess.cursorModel : null;
  const claudeModel = agentKind === 'claude' ? sess.claudeModel : null;
  const appContext = buildAgentAppContext(sessionId);

  try {
    const result = await runAgentRequest({
      sessionId,
      prompt,
      cwd,
      claudeUuid,
      resume,
      rules: rules || null,
      agentKind,
      cursorModel,
      claudeModel,
      appContext,
      // See `sendClaudeMessage.ts` for the RTK wiring rationale.
      // The auto-follow-up turn must honour the same per-session
      // toggle as the user-initiated turn.
      rtkDisabled: sess.rtkEnabled === false,
      onAssistantDelta: deps.appendAssistantDelta,
      onAppNavigation: deps.handleAppNavigation,
    });
    const sessAfter = sessionsState.list.find((s) => s.id === sessionId);
    const lastMsg = sessAfter?.messages[sessAfter.messages.length - 1];
    const streamed = lastMsg?.role === 'assistant' ? lastMsg.content.trim() : '';
    const finalReply = result.reply.trim();
    const uuidStable = !!sessAfter && sessAfter.claudeUuid === claudeUuid;
    const patch: Partial<ClaudeSession> = {};
    if (uuidStable) {
      patch.claudeResumable = true;
      if (result.sessionUuid && result.sessionUuid !== claudeUuid) {
        patch.claudeUuid = result.sessionUuid;
      }
    }
    if (!streamed) {
      replaceLastAssistant(sessionId, finalReply || '(empty response)');
    }
    if (sess.cwdSwitchRecap) {
      patch.cwdSwitchRecap = null;
    }
    const sessAfter2 = sessionsState.list.find((s) => s.id === sessionId);
    const stillPending = sessAfter2?.actions.some((a) => a.status === 'pending') ?? false;
    if (stillPending) patch.awaitingApproval = true;
    updateSession(sessionId, patch);
  } catch (e) {
    const msg = typeof e === 'string' ? e : String(e);
    const cancelled = msg.toLowerCase().includes('cancelled');
    const agentLabel = sess.agentKind === 'cursor' ? 'Cursor' : 'Claude';
    if (cancelled) {
      appendSessionMessage(sessionId, {
        role: 'system',
        content: 'Cancelled.',
        at: new Date().toISOString(),
      });
    } else {
      replaceLastAssistant(sessionId, `**${agentLabel} failed:** ${msg}`);
      if (appHasFocus()) {
        notifyError(e, { title: `${agentLabel} run failed` });
      } else {
        notifyClaudeRunComplete({
          agentLabel,
          sessionTitle: sess.title || 'Untitled chat',
          ok: false,
          durationMs: Date.now() - runStartedAt,
        });
      }
    }
  }
  deps.stopThinkingTimer(sessionId);
  flushActionResultsToUI(sessionId);
  updateSession(sessionId, { sending: false });
  continuationInFlight.delete(sessionId);
  void deps.scrollChatBottom();
  const sessNow = sessionsState.list.find((s) => s.id === sessionId);
  if (sessNow?.sending) {
    updateSession(sessionId, { sending: false });
  }
  continuationInFlight.delete(sessionId);
}

/** Expose the guard for sendClaudeMessage cleanup paths (route file
 *  clears it when a turn errors out outside this module). */
export function clearContinuationInFlight(sessionId: string): void {
  continuationInFlight.delete(sessionId);
}
