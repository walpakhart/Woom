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

import { invoke } from '@tauri-apps/api/core';
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
  sendClaudeMessage(opts?: { silent?: boolean; kind?: 'claude'; prompt?: string }): Promise<void>;
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
  /* Allow `/skill` followed by punctuation (`.`, `,`, `!`, `?`, `;`,
   * `:`) — not just whitespace or end-of-string. Without this a
   * sentence-final invocation like "build a page /frontend-design."
   * silently fell through to plain text because the trailing period
   * broke the lookahead and the user thought the skill was broken. */
  const inlineSkillRe = /(^|\s)\/([A-Za-z][\w-]*)(?=[\s.,!?;:]|$)/g;
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
       *  Agent receives the expanded SKILL.md body silently to avoid
       *  dumping the template into the visible transcript. */
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
    } else if (withArgs.name === 'dw') {
      await runDwFromSlash(session, withArgs.args, deps);
      void deps.scrollChatBottom();
    } else if (withArgs.name === 'ledger') {
      await runLedgerFromSlash(session, withArgs.args, deps);
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
  } else if (cmd === 'remember') {
    const { distillMemories } = await import('$lib/services/distillMemory');
    await distillMemories(session);
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
        detail: { kind: 'claude' },
      }));
    } catch { /* noop */ }
  }
  return true;
}

/** `/dw <ask>` runner. Calls backend planner, registers workflow in
 *  reactive state, opens the preflight modal with the planner output
 *  + cost estimate. On approve fires `dw_approve` (kicks off fan-out)
 *  and appends an assistant message carrying `dwWorkflowId` — ChatThread
 *  renders <DynamicWorkflowCard> after that message. On cancel drops
 *  the workflow from state (server-side will GC the orphan entry). */
export async function runDwFromSlash(
  session: ClaudeSession,
  userPrompt: string,
  deps: SlashCommandDeps,
): Promise<void> {
  appendSessionMessage(session.id, {
    role: 'user',
    content: `/dw ${userPrompt}`,
    at: new Date().toISOString(),
  });
  const cwd = session.worktreePath ?? session.cwd ?? null;
  // Phase 2a: create an EMPTY `building` workflow, then let the MAIN
  // chat agent construct it live (survey → dw_set_task → dw_add_subagent
  // ×N → dw_launch) — no hidden planner oneshot, no pre-flight modal.
  let workflowId: string;
  try {
    workflowId = await invoke('dw_create', {
      sessionId: session.id,
      task: userPrompt,
      cwd,
      model: session.claudeModel ?? null,
      fast: session.fastMode === true,
    });
  } catch (e) {
    appendSessionMessage(session.id, {
      role: 'assistant',
      content: `_DW create failed: ${String(e)}_`,
      at: new Date().toISOString(),
    });
    return;
  }
  // Assistant message that HOSTS the card once the workflow finishes
  // (terminal workflows render at their origin message; the active one
  // is shown in the pinned bottom slot meanwhile).
  appendSessionMessage(session.id, {
    role: 'assistant',
    content: '',
    at: new Date().toISOString(),
    dwWorkflowId: workflowId,
  });
  // Silent build brief — drives a normal (visible) agent turn that
  // populates the workflow via the dw_* tools.
  const brief =
    `You are building Dynamic Workflow \`${workflowId}\` for this task:\n\n${userPrompt}\n\n` +
    `Survey the repo just enough to split this into INDEPENDENT slices (no cross-slice deps). Then:\n` +
    `1. mcp__app__dw_set_task — workflowId "${workflowId}", a one-line task summary.\n` +
    `2. mcp__app__dw_add_subagent — workflowId "${workflowId}", one self-contained prompt per slice (call repeatedly). Spell out what to investigate/change + what to report.\n` +
    `3. mcp__app__dw_launch — workflowId "${workflowId}" once all slices are added.\n` +
    `Use read-only tools for the survey. Keep it tight — no long preamble, just build it.`;
  // Programmatic send via `prompt` (NOT updateSession({ input })): the
  // brief is hidden orchestration traffic and must not clobber whatever
  // the user is typing in the composer (architecture rule, commit 24ffc4c).
  await deps.sendClaudeMessage({ silent: true, prompt: brief });
}

/** `/ledger <ask>` runner — the sequential machine-checked sibling of
 *  `/dw`. Creates an empty `building` ledger, then a silent build brief
 *  drives the MAIN chat agent to construct the checklist live
 *  (ledger_set_task → ledger_add_item ×N → ledger_launch). The user
 *  approves the checklist on the card; execution runs items one by one
 *  in a shared worktree, each verified by its check command. */
export async function runLedgerFromSlash(
  session: ClaudeSession,
  userPrompt: string,
  deps: SlashCommandDeps,
): Promise<void> {
  appendSessionMessage(session.id, {
    role: 'user',
    content: `/ledger ${userPrompt}`,
    at: new Date().toISOString(),
  });
  const cwd = session.worktreePath ?? session.cwd ?? null;
  let workflowId: string;
  try {
    workflowId = await invoke('ledger_create', {
      sessionId: session.id,
      task: userPrompt,
      cwd,
      model: session.claudeModel ?? null,
    });
  } catch (e) {
    appendSessionMessage(session.id, {
      role: 'assistant',
      content: `_Ledger create failed: ${String(e)}_`,
      at: new Date().toISOString(),
    });
    return;
  }
  appendSessionMessage(session.id, {
    role: 'assistant',
    content: '',
    at: new Date().toISOString(),
    ledgerWorkflowId: workflowId,
  });
  const brief =
    `You are building Ledger \`${workflowId}\` for this task:\n\n${userPrompt}\n\n` +
    `A ledger is a machine-checked checklist: items run in order in one shared worktree, ` +
    `each executed by a fresh agent context and verified by a machine check; consecutive ` +
    `parallel-safe items run concurrently as a wave.\n\n` +
    `RESEARCH FIRST (read-only tools): read the key files the task touches, find the repo's ` +
    `real build/test/lint commands, and VERIFY each check command you plan to use actually ` +
    `runs here (you may execute it) — a checklist with broken checks is worthless. For large ` +
    `tasks spend several tool calls here; the checklist quality is decided in this phase.\n\n` +
    `Then build:\n` +
    `1. mcp__app__ledger_set_task — workflowId "${workflowId}", a one-line task summary.\n` +
    `2. mcp__app__ledger_add_item — workflowId "${workflowId}", once per item IN EXECUTION ORDER. ` +
    `Each item: \`title\` = one-line requirement (what must become true), \`detail\` = precise ` +
    `instructions incl. relevant file paths from your research, \`check_cmd\` = shell command ` +
    `from the repo root whose exit code proves the item (test run, build, grep — ALWAYS provide ` +
    `one when possible; omit only for judgment-call items, those get an LLM grader), ` +
    `\`parallel\` = true ONLY when the item touches files no other item touches (consecutive ` +
    `parallel items run concurrently; when unsure, false).\n` +
    `3. mcp__app__ledger_launch — workflowId "${workflowId}" once the checklist is complete.\n` +
    `Sizing: 2-8 items for most tasks, up to 15 for large features (cap 30) — split big items; ` +
    `each should be one coherent change a single focused turn can land. If the task is trivial ` +
    `(one obvious edit), SKIP the ledger — say so and just do it directly instead of calling ` +
    `the tools.\nKeep it tight — no long preamble, just build it.`;
  await deps.sendClaudeMessage({ silent: true, prompt: brief });
}
