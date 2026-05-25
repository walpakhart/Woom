// `sendClaudeMessage` extracted from `+page.svelte` in wave-39. This
// is the route's largest single function (~620 LoC) — every Composer
// "Send" routes through it. Heavy state coupling: ~15 reactive locals
// + 8 imported services + recursive self-call on resume-orphan retry
// + post-turn queue drain. The route file keeps a thin shim that
// passes a deps object built once + a self-reference so the
// recursive retry / queue-drain re-entry resolves correctly.
//
// The function preserves a lot of subtle behaviour:
//   - Queue-while-sending (visible text vs. silent SDD prompt slot).
//   - Slash-command short-circuit (delegates to handleSlashCommand).
//   - UserPromptSubmit hook gating.
//   - Crash-recovery recap + uuid rotation when `interrupted=true`.
//   - First-turn preamble (cwd snapshot + recall terms).
//   - Mention bake-in (file refs → image vision blocks for Claude,
//     path-pointer text for Cursor).
//   - Resume-orphan self-heal with a single recursive retry.
//   - Stop hook + statusline refresh + native completion notify.
//   - SDD silent-prompt drain + queue drain in the finally block.

import { invoke } from '@tauri-apps/api/core';
import {
  appendSessionMessage,
  drainPendingActionResultsForAgent,
  flushActionResultsToUI,
  formatActionResultsForPrompt,
  genUuid,
  replaceLastAssistant,
  sessionsState,
  updateSession,
} from '$lib/state/sessions.svelte';
import { applySessionCwd, buildContinuationRecap } from '$lib/services/sessionCwd';
import { buildFirstTurnPreamble, getActiveEditorFile } from '$lib/services/firstTurnContext';
import { buildAgentAppContext } from '$lib/services/agentContext';
import { saveCanvasScreenshot } from '$lib/services/canvasScreenshot';
import {
  isResumeOrphanError,
  RESUME_ORPHAN_PREFIX,
  runAgentRequest,
} from '$lib/exec/claude';
import { ensureCanvasLoaded } from '$lib/state/canvas.svelte';
import { loadClaudeMd } from '$lib/state/claudemd.svelte';
import { refreshPlanUsage } from '$lib/state/quota.svelte';
import { runHook } from '$lib/state/hooks.svelte';
import { runStatusLine, type StatusLinePayload } from '$lib/state/statusline.svelte';
import { notify, notifyError } from '$lib/state/toaster.svelte';
import { appHasFocus, notifyClaudeRunComplete } from '$lib/notifications';
import { refreshSdd, workspaceForSession } from '$lib/state/sdd.svelte';
import { isImagePath } from '$lib/format';
import type { ClaudeSession, Mention } from '$lib/types';

export interface SendOpts {
  silent?: boolean;
  kind?: 'claude' | 'cursor';
}

export interface SendClaudeMessageDeps {
  /** Route-local reactive derived for "the chat the user is looking at". */
  getActiveSession(): ClaudeSession | null;
  /** Editor's open-folder fallback when the session has no cwd. */
  getEditorRepoPath(): string;
  startThinkingTimer(kind: 'claude' | 'cursor'): void;
  stopThinkingTimer(kind: 'claude' | 'cursor'): void;
  scrollChatBottom(): Promise<void> | void;
  appendAssistantDelta(sessionId: string, delta: string): void;
  handleAppNavigation(sessionId: string, name: string, input: Record<string, unknown>): void;
  handleSlashCommand(text: string, session: ClaudeSession): Promise<boolean>;
  buildStatusLinePayload(): StatusLinePayload | null;
  /** Per-session saved-drafts map (preserves the user's typed text
   *  across a multi-message queue drain). */
  queueSavedDrafts: Map<string, { text: string; mentions: Mention[] }>;
  /** Cached app-data attachment dir — used for the canvas screenshot
   *  channel that only Claude sessions get. */
  getAttachmentDir(): Promise<string>;
}

/** Build a self-aware sendClaudeMessage closure. The recursive
 *  self-call (resume-orphan retry, post-turn queue drain) needs a
 *  reference to the same function — we return the closure so its
 *  inner references resolve to itself, not a stale snapshot. */
export function createSendClaudeMessage(deps: SendClaudeMessageDeps) {
  const send = async (opts: SendOpts = {}): Promise<void> => {
    const s = opts.kind
      ? sessionsState.list.find((x) => x.id === sessionsState.activeIds[opts.kind!]) ?? null
      : deps.getActiveSession();
    if (!s) return;
    if (s.sending) {
      if (opts.silent) {
        const promptText = s.input.trim();
        if (promptText) {
          const { setPendingSilent } = await import('$lib/state/sdd.svelte');
          setPendingSilent(s.id, promptText);
        }
        updateSession(s.id, { input: '' });
        return;
      }
      const draft = s.input.trim();
      if (!draft && s.mentions.length === 0) return;
      const entry = { text: draft, mentions: [...s.mentions] };
      const nextQueue = [...(s.pendingQueue ?? []), entry];
      updateSession(s.id, { input: '', mentions: [], pendingQueue: nextQueue });
      return;
    }
    if (!s.input.trim() && s.mentions.length === 0) return;
    const text = s.input.trim();
    if (!opts.silent && (await deps.handleSlashCommand(text, s))) return;
    const id = s.id;
    const kind = (s.agentKind ?? 'claude') as 'claude' | 'cursor';
    if (!opts.silent) {
      try {
        const hookOut = await runHook('UserPromptSubmit', {
          session_id: id,
          agent_kind: kind,
          prompt: text,
          cwd: s.cwd ?? null,
          worktree_path: s.worktreePath ?? null,
        });
        if (hookOut.blocked) {
          appendSessionMessage(id, {
            role: 'assistant',
            content: `_Blocked by hook: ${hookOut.feedback.join('; ') || '(no reason given)'}_`,
            at: new Date().toISOString(),
          });
          return;
        }
      } catch (err) {
        console.warn('UserPromptSubmit hook failed', err);
      }
    }
    const mentionsSnapshotPre = s.mentions;
    const imageMentions = mentionsSnapshotPre.filter(
      (m) => m.source === 'file' && !m.isDir && !!m.body && isImagePath(m.body),
    );
    const userImages = imageMentions.map((m) => ({ path: m.body!, name: m.title }));
    appendSessionMessage(id, {
      role: 'user',
      content: text,
      at: new Date().toISOString(),
      ...(userImages.length ? { images: userImages } : {}),
      ...(opts.silent ? { hidden: true } : {}),
    });
    const curr = sessionsState.list.find((x) => x.id === id);
    const mentionsSnapshot = curr?.mentions ?? [];
    if (
      curr &&
      curr.messages.filter((m) => m.role === 'user').length === 1 &&
      curr.mentions.length === 0
    ) {
      const autoTitle = text.slice(0, 36) + (text.length > 36 ? '…' : '');
      updateSession(id, { title: autoTitle, input: '', sending: true, mentions: [], awaitingApproval: false });
    } else {
      updateSession(id, { input: '', sending: true, mentions: [], awaitingApproval: false });
    }
    const crashedSess = sessionsState.list.find((x) => x.id === id);
    if (crashedSess?.interrupted) {
      const recap = buildContinuationRecap(crashedSess, 'app_crash');
      updateSession(id, {
        interrupted: false,
        cwdSwitchRecap: recap,
        claudeUuid: genUuid(),
        claudeResumable: false,
      });
      appendSessionMessage(id, {
        role: 'system',
        content: '↩ Recovered from interrupted turn — prior transcript folded into the system prompt.',
        at: new Date().toISOString(),
      });
    }
    const sessForFirstTurn = sessionsState.list.find((x) => x.id === id);
    const priorAssistantTurns = (sessForFirstTurn?.messages ?? []).filter(
      (m) => m.role === 'assistant' && m.content.trim().length > 0,
    ).length;
    if (sessForFirstTurn && priorAssistantTurns === 0) {
      const preambleCwd =
        sessForFirstTurn.worktreePath
        || sessForFirstTurn.cwd
        || deps.getEditorRepoPath()
        || null;
      const linkedEditorId = sessForFirstTurn.linkedToEditor
        ? sessForFirstTurn.linkedToEditorInstanceId
        : null;
      const openFile = linkedEditorId ? getActiveEditorFile(linkedEditorId) : null;
      const recallTerms: string[] = [
        ...text.slice(0, 200).split(/\s+/).filter((w) => w.length >= 4),
        ...mentionsSnapshot.map((m) => m.title).filter(Boolean),
      ];
      const preamble = await buildFirstTurnPreamble(preambleCwd, openFile, recallTerms);
      if (preamble) {
        const existing = sessForFirstTurn.cwdSwitchRecap;
        const composed = existing
          ? `${preamble}\n\n---\n\n${existing}`
          : preamble;
        updateSession(id, { cwdSwitchRecap: composed });
      }
    }
    appendSessionMessage(id, {
      role: 'assistant',
      content: '',
      at: new Date().toISOString(),
    });
    deps.startThinkingTimer(kind);
    const runStartedAt = Date.now();
    void deps.scrollChatBottom();

    const sess = sessionsState.list.find((x) => x.id === id);
    const agentKindForPrompt = sess?.agentKind ?? 'claude';
    let prompt = text;
    if (mentionsSnapshot.length) {
      const ctx = mentionsSnapshot
        .map((m) => {
          if (m.source === 'file') {
            const abs = m.body ?? m.externalId;
            const kindLocal = m.isDir ? 'directory' : isImagePath(abs) ? 'image' : 'file';
            if (kindLocal === 'image' && agentKindForPrompt === 'claude') return null;
            const hint = kindLocal === 'image'
              ? `This is an image attached by the user — load it via its absolute path to view it inline.`
              : `You have Read / Glob / Grep tools — use them to inspect this ${kindLocal} when relevant.`;
            const label = kindLocal === 'image' ? `Attached ${kindLocal}: ${m.title}` : `Referenced ${kindLocal}: @${m.externalId}`;
            return `${label}\nAbsolute path: ${abs}\n${hint}`;
          }
          return `@${m.externalId} — ${m.title}` + (m.body ? `\n\n${m.body}` : '');
        })
        .filter((x): x is string => x !== null)
        .join('\n\n----\n\n');
      if (ctx) prompt = `Referenced items:\n\n${ctx}\n\n----\n\nUser message:\n${text}`;
    }

    const pendingForAgent = drainPendingActionResultsForAgent(id);
    if (pendingForAgent.length > 0) {
      const block = formatActionResultsForPrompt(pendingForAgent);
      prompt = `${block}\n\n---\n\n${prompt}`;
    }

    const cwd = sess?.worktreePath || sess?.cwd || deps.getEditorRepoPath() || null;
    const claudeUuid = sess?.claudeUuid ?? genUuid();
    const resume = Boolean(sess?.claudeResumable);
    const rules = sessionsState.userRules.trim();
    const agentKind = sess?.agentKind ?? 'claude';
    const cursorModel = agentKind === 'cursor' ? (sess?.cursorModel ?? null) : null;
    const claudeModel = agentKind === 'claude' ? (sess?.claudeModel ?? null) : null;
    await loadClaudeMd(sess?.worktreePath ?? sess?.cwd ?? null).catch(() => {});
    const appContext = buildAgentAppContext(id);
    const imagePaths = agentKind === 'claude' ? userImages.map((u) => u.path) : [];

    if (agentKind === 'claude' && sess?.linkedCanvasId) {
      const c = ensureCanvasLoaded(sess.linkedCanvasId);
      if (c && (c.shapes.length > 0 || c.edges.length > 0)) {
        try {
          const dir = await deps.getAttachmentDir();
          const path = await saveCanvasScreenshot(c, dir);
          if (path) imagePaths.push(path);
        } catch (err) {
          console.warn('canvas screenshot attach failed', err);
        }
      }
    }

    try {
      try {
        const result = await runAgentRequest({
          sessionId: id,
          prompt,
          cwd,
          claudeUuid,
          resume,
          rules: rules || null,
          agentKind,
          cursorModel,
          claudeModel,
          appContext,
          imagePaths,
          onAssistantDelta: deps.appendAssistantDelta,
          onAppNavigation: deps.handleAppNavigation,
        });
        const sessNowForReply = sessionsState.list.find((sx) => sx.id === id);
        const lastMsg = sessNowForReply?.messages[sessNowForReply.messages.length - 1];
        const streamed = lastMsg?.role === 'assistant' ? lastMsg.content.trim() : '';
        const finalReply = result.reply.trim();
        if (!streamed) {
          replaceLastAssistant(id, finalReply || '(empty response)');
        }
        const sessAfter = sessionsState.list.find((sx) => sx.id === id);
        const uuidStable = !!sessAfter && sessAfter.claudeUuid === claudeUuid;
        const patch: Partial<ClaudeSession> = {};
        if (uuidStable) {
          patch.claudeResumable = true;
          if (result.sessionUuid && result.sessionUuid !== claudeUuid) {
            patch.claudeUuid = result.sessionUuid;
          }
        }
        void refreshPlanUsage();
        if (sess?.cwdSwitchRecap) {
          patch.cwdSwitchRecap = null;
        }
        const stillPending = sessAfter?.actions.some((a) => a.status === 'pending') ?? false;
        if (stillPending) patch.awaitingApproval = true;
        updateSession(id, patch);
      } catch (e) {
        const msg = typeof e === 'string' ? e : String(e);
        const cancelled = msg.toLowerCase().includes('cancelled');
        if (isResumeOrphanError(e) && !cancelled) {
          const detail = msg.slice(RESUME_ORPHAN_PREFIX.length).trim();
          const sessNow = sessionsState.list.find((x) => x.id === id);
          if (sessNow) {
            const recap = buildContinuationRecap(sessNow, 'cli_orphan', { detail });
            updateSession(id, {
              claudeUuid: genUuid(),
              claudeResumable: false,
              cwdSwitchRecap: recap,
            });
            notify({
              kind: 'info',
              title: `${s.agentKind === 'cursor' ? 'Cursor' : 'Claude'} session refreshed`,
              body: 'Prior CLI history was unavailable; restarted with the in-app transcript baked into context. Continuing your turn.',
              ttlMs: 6000,
            });
            replaceLastAssistant(id, '');
            updateSession(id, { sending: false, input: text, mentions: mentionsSnapshotPre });
            deps.stopThinkingTimer(kind);
            await send();
            return;
          }
        }
        if (cancelled) {
          appendSessionMessage(id, {
            role: 'system',
            content: 'Cancelled.',
            at: new Date().toISOString(),
          });
        } else {
          replaceLastAssistant(id, `**${s.agentKind === 'cursor' ? 'Cursor' : 'Claude'} failed:** ${msg}`);
          if (appHasFocus()) {
            notifyError(e, { title: `${s.agentKind === 'cursor' ? 'Cursor' : 'Claude'} run failed` });
          }
        }
        if (!appHasFocus() && !cancelled) {
          notifyClaudeRunComplete({
            agentLabel: s.agentKind === 'cursor' ? 'Cursor' : 'Claude',
            sessionTitle: s.title || 'Untitled chat',
            ok: false,
            durationMs: Date.now() - runStartedAt,
          });
        }
      }
      deps.stopThinkingTimer(kind);
      flushActionResultsToUI(id);
      const finalSess = sessionsState.list.find((x) => x.id === id);
      const erroredOut = finalSess?.messages.some(
        (m, i) => i === finalSess.messages.length - 1 && m.role === 'assistant' && m.content.startsWith('**Claude failed:'),
      );
      updateSession(id, { sending: false });
      {
        const sddWs = workspaceForSession(id);
        if (sddWs) {
          void refreshSdd(sddWs.id);
        }
      }
      void runHook('Stop', {
        session_id: id,
        agent_kind: kind,
        errored: !!erroredOut,
        duration_ms: Date.now() - runStartedAt,
        message_count: finalSess?.messages.length ?? 0,
      }).catch(() => {});
      {
        const payload = deps.buildStatusLinePayload();
        if (payload) void runStatusLine(payload);
      }
      if (!appHasFocus() && !erroredOut) {
        notifyClaudeRunComplete({
          agentLabel: s.agentKind === 'cursor' ? 'Cursor' : 'Claude',
          sessionTitle: finalSess?.title || s.title || 'Untitled chat',
          ok: true,
          durationMs: Date.now() - runStartedAt,
        });
      }
      void deps.scrollChatBottom();
    } finally {
      const stillSending = sessionsState.list.find((sx) => sx.id === id)?.sending;
      if (stillSending) {
        deps.stopThinkingTimer(kind);
        updateSession(id, { sending: false });
      }
      const sessAfterDrain = sessionsState.list.find((x) => x.id === id);
      {
        const { popPendingSilent } = await import('$lib/state/sdd.svelte');
        const deferred = popPendingSilent(id);
        if (deferred && (sessAfterDrain?.pendingQueue?.length ?? 0) === 0) {
          updateSession(id, { input: deferred });
          const kindForDrain = sessAfterDrain?.agentKind ?? 'claude';
          sessionsState.activeClaudeId = id;
          sessionsState.activeIds[kindForDrain] = id;
          queueMicrotask(() => {
            void send({ silent: true, kind: kindForDrain });
          });
          return;
        }
      }
      const rawQueue = sessAfterDrain?.pendingQueue ?? [];
      const queue = rawQueue.filter((entry) => {
        const t = entry.text.trimStart();
        return !(
          t.startsWith('# SDD Phase 1 — Write the spec') ||
          t.startsWith('# SDD Phase 2 — Write the plan') ||
          t.startsWith('# SDD Phase 3+ — Execute phase')
        );
      });
      if (queue.length !== rawQueue.length) {
        updateSession(id, { pendingQueue: queue });
      }
      if (queue.length > 0) {
        const [nextEntry, ...rest] = queue;
        if (!deps.queueSavedDrafts.has(id)) {
          const cur = sessAfterDrain!;
          if (cur.input.trim() || cur.mentions.length > 0) {
            deps.queueSavedDrafts.set(id, { text: cur.input, mentions: [...cur.mentions] });
          }
        }
        updateSession(id, { pendingQueue: rest, input: nextEntry.text, mentions: nextEntry.mentions });
        sessionsState.activeClaudeId = id;
        sessionsState.activeIds[sessAfterDrain!.agentKind] = id;
        queueMicrotask(() => {
          void send();
        });
      } else {
        const saved = deps.queueSavedDrafts.get(id);
        if (saved) {
          deps.queueSavedDrafts.delete(id);
          if (saved.text.trim() || saved.mentions.length > 0) {
            updateSession(id, { input: saved.text, mentions: saved.mentions });
          }
        }
      }
    }
  };
  return send;
}

// `applySessionCwd` re-export so the route can drop its own import
// when it only needs the function for the resume-orphan retry path.
// (Module already imports it above.)
export { applySessionCwd };
