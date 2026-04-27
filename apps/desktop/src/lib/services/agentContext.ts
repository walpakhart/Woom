// Per-turn system-prompt suffix builder for Claude / Cursor agent runs.
// Pulled out of +page.svelte as the first piece of the god-file split.
// No DOM, no events — pure function over `layoutState` + `sessionsState`.
//
// Ordering rule (matters for prompt-cache): everything static lives at
// the top, the variable workbench-layout block comes LAST. Claude's
// cache keys off a prefix, so a stable header + tool guide on top
// means the kilobytes of instructions get cached across turns and
// only the trailing layout snapshot is fresh on each call. Same logic
// applies to cursor-agent's backend caching.

import { layoutState } from '$lib/state/layout.svelte';
import { sessionsState } from '$lib/state/sessions.svelte';

/** Build the per-turn app-context string we hand the agent as a system-
 *  prompt suffix. Lists every workbench's instances by name + id, with
 *  the editor's open path / agent's cwd, and editor↔agent links. Tells
 *  the agent which instance/session it's running in so "switch myself"
 *  has a meaning, and which sibling instances exist so it knows whether
 *  to add a NEW one or just change an existing one's path.
 *
 *  Re-derived on every turn so it's always current. */
export function buildAgentAppContext(callingSessionId: string): string {
  const lines: string[] = [];

  // ── Static section: header + navigation tool guide. Same bytes on
  // every turn (modulo a Forgehold deploy) so prompt caches eat it.
  lines.push(
    'You are running inside Forgehold, a desktop app where the user has '
      + 'organised work into workbenches (tabs of side-by-side columns). '
      + 'You can navigate the UI directly via the `mcp__app__*` tools.'
  );
  lines.push('');
  lines.push(
    'When the user asks to "switch the editor and claude", "open this '
      + 'repo in editor", "switch myself to /path", etc — DO NOT add a new '
      + 'column. Use these tools on existing instances:'
  );
  lines.push(
    '  - `mcp__app__set_editor_repo_path` — change an editor\'s open '
      + 'folder. Pass `instance_name` (the art-name like "Sagrada-Familia") '
      + 'or `instance_id`. If the editor has linked agents, their cwd '
      + 'auto-follows — see the `linked_agents=[…]` field on each editor '
      + 'in the workbench layout below. So if your column is in '
      + '`linked_agents` of the editor you\'re moving, you DON\'T need a '
      + 'separate set_agent_cwd for yourself — the link handles it.'
  );
  lines.push(
    '  - `mcp__app__set_agent_cwd` — change an agent session\'s cwd. '
      + 'Pass `instance_name`/`instance_id`, or `target=self` for yourself. '
      + 'For yourself, the change takes effect on your NEXT turn. The '
      + 'editor↔agent link is NEVER broken by this call — only by the '
      + 'user clicking "Unlink" in the UI.'
  );
  lines.push(
    '  - `mcp__app__list_instances` — re-list the current state if you '
      + 'think this preamble is stale.'
  );
  lines.push(
    'Only use `mcp__app__add_workbench_instance` when the user explicitly '
      + 'says "add", "new", "another" — not for "switch" / "open in".'
  );
  lines.push('');
  lines.push(
    'Approval cards: `set_editor_repo_path` and `set_agent_cwd` execute '
      + 'immediately when the USER asked you to switch — no approval card. '
      + 'If you want to PROACTIVELY suggest a switch (the user didn\'t '
      + 'ask but you think they should), use `mcp__github__propose_switch_cwd` '
      + 'instead — that one queues an approval card.'
  );

  // Tool-iteration discipline. Empirically the biggest token-burn we
  // see on Forgehold isn't the system prompt — it's the agent
  // re-running near-identical search queries 5–10 times across
  // GitHub/Jira/Sentry/memory to "be thorough", then re-paying the
  // entire conversation history on every round-trip. One focused
  // query returns the same data and costs 1/Nth of the limit. This
  // block lives in the static cached prefix, so it costs ~140 tokens
  // once per session and saves multiple thousand tokens per
  // "list my PRs" / "find issues mentioning X" / "show recent
  // errors" turn.
  lines.push('');
  lines.push(
    'Search/list discipline (applies to ALL data sources). When the '
      + 'user asks for a list, lookup, or "show me my X" — make ONE '
      + 'focused query, then narrow only if the result needs '
      + 'filtering. Do NOT iterate variations of the same intent '
      + '(running the same search with `org:` then without, with '
      + '`is:draft` then `state:open`, with different JQL scopes, '
      + 'etc.). The data sources already return all matches in one '
      + 'call; iterating just re-pays the entire conversation '
      + 'context for the same answer. Concrete patterns:\n'
      + '  - GitHub "my open PRs" → ONE `mcp__github__search_prs` '
      + 'with `is:pr author:<user> state:open sort:updated-desc`. '
      + 'Group by repo in your reply.\n'
      + '  - GitHub "PR #N details" → ONE `mcp__github__get_pr` '
      + '(it has title/state/branches/body). Add `get_pr_diff` / '
      + '`get_pr_files` / `get_pr_comments` ONLY if the user asks '
      + 'about diff/files/discussion respectively.\n'
      + '  - Jira "my tickets" / "open in DEVOPS" → ONE '
      + '`mcp__jira__search` with a single JQL: '
      + '`assignee = currentUser() AND resolution = Unresolved` '
      + 'or `project = DEVOPS AND status != Done`. JQL handles '
      + 'AND/OR/IN — combine, don\'t iterate.\n'
      + '  - Sentry "recent errors" / "crashes about X" → ONE '
      + '`mcp__sentry__search_issues` with combined filters '
      + '(`is:unresolved level:error project:foo`).\n'
      + '  - Memory recall → ONE `mcp__memory__memory_search` with '
      + 'multi-word query (FTS handles synonyms). If null on the '
      + 'first try, the memory genuinely isn\'t there.\n'
      + 'If the first call returns an empty result, narrowing then '
      + 'is free — but never broaden after a hit. Pagination > '
      + 're-querying.'
  );

  // ── Variable section: workbench layout snapshot + one-shot
  // cwd-switch recap. Re-derived every turn so cache-busting bytes
  // live here exclusively. Keep the section delimiter so the agent
  // can visually parse where the current state begins.
  const calling = sessionsState.list.find((s) => s.id === callingSessionId);
  const callingInstanceId = calling?.columnInstanceId ?? null;

  lines.push('');
  lines.push('---');
  lines.push('Current workbench layout (refreshed on every turn):');

  // One-shot recap if the user just switched the agent's cwd. Cleared
  // after the turn ships (in sendClaudeMessage's success path).
  if (calling?.cwdSwitchRecap) {
    lines.push('');
    lines.push(calling.cwdSwitchRecap);
  }

  for (const wb of layoutState.workbenches) {
    const isActive = wb.id === layoutState.activeWorkbenchId;
    lines.push('');
    lines.push(`Workbench "${wb.name}"${isActive ? ' (ACTIVE)' : ''} — id ${wb.id}:`);
    if (wb.instances.length === 0) {
      lines.push('  (no columns)');
      continue;
    }
    for (const inst of wb.instances) {
      const meta: string[] = [`kind=${inst.kind}`, `name=${inst.name}`, `id=${inst.id}`];
      if (inst.kind === 'editor') {
        const path = sessionsState.editorInstanceState[inst.id]?.repoPath ?? '';
        meta.push(`repo_path=${path || '(none)'}`);
        // Show what agent sessions are linked to this editor.
        const linked = sessionsState.list
          .filter((s) => s.linkedToEditor && s.linkedToEditorInstanceId === inst.id)
          .map((s) => s.title || s.id.slice(0, 6));
        if (linked.length) meta.push(`linked_agents=[${linked.join(', ')}]`);
      }
      if (inst.kind === 'claude' || inst.kind === 'cursor') {
        // Find the active session bound to this column.
        const sessId = sessionsState.activeByInstance[inst.id] ?? null;
        const sess = sessId ? sessionsState.list.find((s) => s.id === sessId) : null;
        if (sess) {
          const effCwd = sess.worktreePath || sess.cwd
            || (sess.linkedToEditor && sess.linkedToEditorInstanceId
              ? sessionsState.editorInstanceState[sess.linkedToEditorInstanceId]?.repoPath
              : null)
            || '(inherits from editor or no cwd)';
          meta.push(`session=${sess.title || sess.id.slice(0, 6)}`);
          meta.push(`cwd=${effCwd}`);
          if (sess.linkedToEditor && sess.linkedToEditorInstanceId) {
            const link = wb.instances.find((i) => i.id === sess.linkedToEditorInstanceId);
            if (link) meta.push(`linked_to_editor=${link.name}`);
          }
        }
      }
      const isYou = inst.id === callingInstanceId;
      lines.push(`  - ${meta.join(', ')}${isYou ? '  ← THIS IS YOU' : ''}`);
    }
  }

  return lines.join('\n');
}
