// `handleSlashCommand` extracted from `+page.svelte` (wave-33 split).
// The composer's Send routes plain user text through here first;
// matching slash / skill invocations are intercepted and dispatched
// to the corresponding action (skill render, /compact, /preview …).
// The caller-supplied `deps` carries the few `+page.svelte`-local
// functions (`sendClaudeMessage`, `scrollChatBottom`, `runCompactSession`)
// that can't be reached as imports.
//
// Returns `true` when a command was consumed — caller short-circuits
// the regular send path. `false` falls through to a normal user-text
// message.

import {
  appendBgTaskList,
  appendSlashHelp,
  appendUsageBreakdown,
  clearSessionHistory,
  killTaskFromSlash,
  KNOWN_SLASH_COMMANDS,
  parseSlashCommand,
  parseSlashCommandWithArgs,
  spawnPreviewFromSlash,
  startLoopFromSlash,
  stopLoopFromSlash,
} from '$lib/services/slashCommands';
import {
  appendSessionMessage,
  setSessionInput,
  updateSession,
} from '$lib/state/sessions.svelte';
import { skillsState, renderSkill } from '$lib/state/skills.svelte';
import type { ClaudeSession } from '$lib/types';

export interface SlashCommandDeps {
  sendClaudeMessage(opts?: { silent?: boolean; kind?: 'claude' | 'cursor' }): Promise<void>;
  scrollChatBottom(): Promise<void> | void;
  runCompactSession(sessionId: string): Promise<void>;
}

export async function handleSlashCommand(
  text: string,
  session: ClaudeSession,
  deps: SlashCommandDeps
): Promise<boolean> {
  // Skill dispatch FIRST — `/<skill-name> [args]`. If the leading
  // slash token matches a discovered skill name, we render its body
  // (with $ARGUMENTS + `!`-shell injection) and stamp the resolved
  // markdown as the next user message instead of routing to a
  // built-in slash. Slash and skill names share a namespace; on
  // collision a built-in wins (so a user can't accidentally shadow
  // `/help` with a SKILL.md called `help`).
  /* Inline-skill detection — scan the WHOLE input for a
   *  `/<skillname>` token (at start, end, or anywhere preceded by
   *  whitespace) instead of requiring the whole input to BE the
   *  command. Lets the user write prose around a skill invocation
   *  the same way @-mentions splice into the text. The non-skill
   *  remainder of the input becomes the skill's $ARGUMENTS so
   *  SKILL.md templates can interpolate it. Built-in slash names
   *  (KNOWN_SLASH_COMMANDS) are skipped here so they fall through
   *  to the strict-start parser below. */
  const inlineSkillRe = /(^|\s)\/([A-Za-z][\w-]*)(?=\s|$)/g;
  let inlineSkill: { name: string; idx: number; full: string } | null = null;
  {
    const raw = text;
    let m: RegExpExecArray | null;
    while ((m = inlineSkillRe.exec(raw)) !== null) {
      const candidate = m[2].toLowerCase();
      if ((KNOWN_SLASH_COMMANDS as string[]).includes(candidate)) continue;
      const sk = skillsState.list.find((s) => s.name.toLowerCase() === candidate);
      if (!sk) continue;
      inlineSkill = {
        name: sk.name,
        idx: m.index + (m[1] ? m[1].length : 0),
        full: `/${sk.name}`,
      };
      break;
    }
  }
  if (inlineSkill) {
    const sk = skillsState.list.find((s) => s.name.toLowerCase() === inlineSkill!.name.toLowerCase());
    if (sk) {
      const tokenEnd = inlineSkill.idx + inlineSkill.full.length;
      const beforeToken = text.slice(0, inlineSkill.idx).replace(/\s+$/, '');
      const afterToken = text.slice(tokenEnd).replace(/^\s+/, '');
      const args = [beforeToken, afterToken].filter((s) => s.length > 0).join(' ');
      setSessionInput(session.id, '');
      const cwd = session.worktreePath ?? session.cwd ?? null;
      const rendered = await renderSkill(sk.id, args, cwd);
      if (!rendered) {
        appendSessionMessage(session.id, {
          role: 'assistant',
          content: `_Skill \`${sk.name}\` failed to render — check the file at \`${sk.path}\`._`,
          at: new Date().toISOString(),
        });
        return true;
      }
      /* Visible bubble: literal text the user typed (prose + skill
       *  token, e.g. "make me a hero section /frontend-design").
       *  Agent receives the expanded SKILL.md body silently — same
       *  pattern `/sdd` uses to avoid dumping the template into the
       *  visible transcript. */
      appendSessionMessage(session.id, {
        role: 'user',
        content: text,
        at: new Date().toISOString(),
      });
      updateSession(session.id, { input: rendered.rendered });
      await Promise.resolve();
      await deps.sendClaudeMessage({ silent: true });
      return true;
    }
  }
  // Args-bearing commands first — `/preview pnpm dev`, `/kill ID`.
  const withArgs = parseSlashCommandWithArgs(text);
  if (withArgs) {
    setSessionInput(session.id, '');
    if (withArgs.name === 'preview') {
      await spawnPreviewFromSlash(session, withArgs.args);
      void deps.scrollChatBottom();
    } else if (withArgs.name === 'kill') {
      await killTaskFromSlash(session, withArgs.args);
      void deps.scrollChatBottom();
    } else if (withArgs.name === 'loop') {
      await startLoopFromSlash(session, withArgs.args);
      void deps.scrollChatBottom();
    } else if (withArgs.name === 'sdd') {
      /* /sdd <prompt> — split the visible user-message from the
       *  agent-facing template. User's ASK appears in the chat as
       *  a normal user bubble (the thing they actually typed). The
       *  multi-paragraph spec-writer template is sent SILENTLY via
       *  `sendClaudeMessage({ silent: true })` — agent's CLI sees
       *  it through --resume history, the visible thread skips it.
       *  Card progress is the user-facing indicator from here on. */
      appendSessionMessage(session.id, {
        role: 'user',
        content: withArgs.args,
        at: new Date().toISOString(),
      });
      const { startSddFromSlash } = await import('$lib/services/slashCommands');
      const rendered = await startSddFromSlash(session, withArgs.args);
      if (rendered) {
        updateSession(session.id, { input: rendered });
        await Promise.resolve();
        await deps.sendClaudeMessage({ silent: true });
      }
      void deps.scrollChatBottom();
    }
    return true;
  }
  const cmd = parseSlashCommand(text);
  if (!cmd) return false;
  /* Clear the composer + capture an `at` for any follow-up. The
   * synthetic assistant messages we append below all carry their
   * own timestamps. */
  setSessionInput(session.id, '');
  if (cmd === 'compact') {
    await deps.runCompactSession(session.id);
  } else if (cmd === 'clear') {
    clearSessionHistory(session);
  } else if (cmd === 'usage') {
    appendUsageBreakdown(session);
    void deps.scrollChatBottom();
  } else if (cmd === 'help') {
    appendSlashHelp(session);
    void deps.scrollChatBottom();
  } else if (cmd === 'ps') {
    appendBgTaskList(session);
    void deps.scrollChatBottom();
  } else if (cmd === 'unloop') {
    await stopLoopFromSlash(session);
    void deps.scrollChatBottom();
  } else if (cmd === 'preview') {
    /* `/preview` with no args — just open the pane. The Composer
     * inside PreviewPane handles spawn. We rely on the AgentApp's
     * own `previewOpen` localStorage flag flipping by the time the
     * user gets here, but since this dispatch is at +page level we
     * can't directly poke that. Instead, fire a custom DOM event
     * the AgentApp listens for. */
    try {
      window.dispatchEvent(new CustomEvent('woom:open-preview', {
        detail: { kind: session.agentKind },
      }));
    } catch { /* noop */ }
  }
  return true;
}
