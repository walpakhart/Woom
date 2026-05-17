// Per-turn system-prompt suffix builder for Claude / Cursor agent runs.
// Pure function over `layoutState` + `sessionsState`. No DOM, no events.
//
// Ordering rule (matters for prompt-cache): everything static lives at
// the top, the variable solo-layout block comes LAST. Claude's cache
// keys off a prefix, so a stable header + tool guide on top means the
// kilobytes of instructions get cached across turns and only the
// trailing layout snapshot is fresh on each call. Same logic applies
// to cursor-agent's backend caching.

import { layoutState, APP_INSTANCE_IDS, DEFAULT_PANEL_ORDER, kindForInstanceId } from '$lib/state/layout.svelte';
import { sessionsState } from '$lib/state/sessions.svelte';
import { canvasState, ensureCanvasLoaded, type Shape, type Edge } from '$lib/state/canvas.svelte';
import { getCachedClaudeMd } from '$lib/state/claudemd.svelte';
import { getCachedAutoMemoryBlock } from '$lib/state/autoMemory.svelte';

/** Build the per-turn app-context string we hand the agent as a
 *  system-prompt suffix. Lists each solo singleton (kind + id), the
 *  editor's open path, the active agent session per kind, and any
 *  editor↔agent / terminal links. Re-derived on every turn so it's
 *  always current. */
export function buildAgentAppContext(callingSessionId: string): string {
  const lines: string[] = [];

  // ── Static section: header + navigation tool guide. Same bytes on
  // every turn (modulo a Woom deploy) so prompt caches eat it.
  lines.push(
    'You are running inside Woom, a desktop app organised as solo '
      + 'modes — one full-screen surface per source (Jira / GitHub / '
      + 'Sentry / Claude / Cursor / Editor / Canvas / Terminal). Each '
      + 'kind has exactly one singleton you can address by id (see the '
      + 'layout snapshot below). Navigate the UI via `mcp__app__*` tools.'
  );
  lines.push('');
  lines.push(
    'When the user asks to "switch the editor", "open this repo", '
      + '"switch myself to /path", etc:'
  );
  lines.push(
    '  - `mcp__app__set_editor_repo_path` — change the editor\'s open '
      + 'folder. Pass `repo_path`. Linked agent sessions auto-follow — '
      + 'their cwd updates in lockstep (see `linked_agents=[…]` on the '
      + 'editor row below). If your session is in that list, you DON\'T '
      + 'need a separate set_agent_cwd for yourself.'
  );
  lines.push(
    '  - `mcp__app__set_agent_cwd` — change an agent session\'s cwd. '
      + 'Pass `target=self` to switch yourself; takes effect on your NEXT '
      + 'turn. The editor↔agent link is NEVER broken by this call — only '
      + 'by the user clicking "Unlink" in the UI.'
  );
  lines.push(
    '  - `mcp__app__focus_solo` — set the rail to a specific solo '
      + '(`kind=editor` etc.). Use this to bring the user\'s attention '
      + 'somewhere; not needed when you only want to read state.'
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
  // see on Woom isn't the system prompt — it's the agent
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

  // ── Variable section: solo layout snapshot + one-shot
  // cwd-switch recap. Re-derived every turn so cache-busting bytes
  // live here exclusively. Keep the section delimiter so the agent
  // can visually parse where the current state begins.
  const calling = sessionsState.list.find((s) => s.id === callingSessionId);
  const callingInstanceId = calling?.agentInstanceId ?? null;

  /* Auto-memory — long-term `user` + `feedback` entries from the
     local SQLite store. Cheap, lives once per session prefix so
     prompt-cache eats it. Refreshed on Settings save + app boot. */
  const autoMem = getCachedAutoMemoryBlock();
  if (autoMem.trim().length > 0) {
    lines.push('');
    lines.push('---');
    lines.push(autoMem);
  }

  /* CLAUDE.md auto-load (mirrors Claude Code's session-memory pattern).
     Pulls from the per-cwd cache populated by `loadClaudeMd` before
     this builder runs — sync access here keeps the function pure.
     Stamped BEFORE the layout snapshot so it reads as durable
     project rules while the layout reads as live state. */
  const cwd = calling?.worktreePath ?? calling?.cwd ?? null;
  const claudemd = getCachedClaudeMd(cwd);
  if (claudemd.content.trim().length > 0) {
    lines.push('');
    lines.push('---');
    lines.push('Project memory (CLAUDE.md, auto-loaded):');
    lines.push('');
    lines.push(claudemd.content.trim());
  }

  lines.push('');
  lines.push('---');
  lines.push('Current solo-mode layout (refreshed on every turn):');

  // One-shot recap if the user just switched the agent's cwd. Cleared
  // after the turn ships (in sendClaudeMessage's success path).
  if (calling?.cwdSwitchRecap) {
    lines.push('');
    lines.push(calling.cwdSwitchRecap);
  }

  // Plan-mode discipline. When the user toggled the session into plan
  // mode (⇧⇥), the agent should READ + INVESTIGATE only — no edits,
  // no mutating bash, no MCP calls that change remote state. End the
  // turn with a structured plan; the user reviews and clicks
  // "Approve & switch to default" to flip mode and continue.
  if (calling?.permissionMode === 'plan') {
    lines.push('');
    lines.push('---');
    lines.push('Plan mode ACTIVE. Do NOT call tools that:');
    lines.push('  - write or edit files (Edit / Write / NotebookEdit)');
    lines.push('  - run mutating bash commands (rm, mv, git commit/push, npm install, sed -i, etc.)');
    lines.push('  - mutate remote state via MCP (mcp__github__add_comment, mcp__jira__transition_issue, mcp__sentry__update_issue, propose_*, etc.)');
    lines.push('Read tools (Read, Grep, Glob, terminal_buffer, mcp__*__get_*, mcp__*__search_*, mcp__*__list_*) ARE allowed.');
    lines.push('');
    lines.push(
      'End the turn with a clear, ordered plan the user can review. '
        + 'The user will switch you out of plan mode to begin execution.'
    );
  }

  for (const kind of DEFAULT_PANEL_ORDER) {
    const id = APP_INSTANCE_IDS[kind];
    const meta: string[] = [`kind=${kind}`, `id=${id}`];
    /* Multi-instance kinds (editor/canvas/terminal) carry a curated
       display name (e.g. "Vermeer") that MCP tools use as the
       agent-facing handle. Surface it next to the id so the agent
       picks the readable form. */
    if (kind === 'editor' || kind === 'canvas' || kind === 'terminal') {
      const primaryName = layoutState.instances[kind].find((i) => i.id === id)?.name;
      if (primaryName) meta.push(`name=${primaryName}`);
    }
    if (kind === 'editor') {
      const path = sessionsState.editorInstanceState[id]?.repoPath ?? layoutState.active.editor.repoPath ?? '';
      meta.push(`repo_path=${path || '(none)'}`);
      /* Currently-open file in this editor instance. EditorView mirrors
         `activePath` into localStorage under `woom:editor:active:<id>`
         on every change. Reading that here means the agent's per-turn
         layout snapshot always reflects what the user is actually
         looking at, so requests like "fix this" can be grounded
         without a separate question / tool call. */
      try {
        const openFile = localStorage.getItem(`woom:editor:active:${id}`);
        if (openFile && openFile.trim()) meta.push(`open_file=${openFile}`);
      } catch { /* localStorage access denied — non-essential */ }
      const linked = sessionsState.list
        .filter((s) => s.linkedToEditor && s.linkedToEditorInstanceId === id)
        .map((s) => s.title || s.id.slice(0, 6));
      if (linked.length) meta.push(`linked_agents=[${linked.join(', ')}]`);
    }
    if (kind === 'claude' || kind === 'cursor') {
      const sessId = sessionsState.activeByInstance[id] ?? null;
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
          const linkKind = kindForInstanceId(sess.linkedToEditorInstanceId);
          if (linkKind) meta.push(`linked_to_editor=${linkKind}`);
        }
        if (sess.linkedTerminalInstanceId) {
          /* Surface the linked terminal's instance ID + display name so
             the agent can call `terminal_run` / `terminal_buffer` with
             the column's art-name directly (e.g. "Vermeer") instead
             of paying a round-trip to `terminal_list`. */
          const termInst = layoutState.instances.terminal.find(
            (i) => i.id === sess.linkedTerminalInstanceId
          );
          if (termInst) {
            meta.push(`linked_to_terminal=${termInst.name} (id=${termInst.id})`);
          }
        }
      }
    }
    const isYou = id === callingInstanceId;
    lines.push(`  - ${meta.join(', ')}${isYou ? '  ← THIS IS YOU' : ''}`);
  }

  /* ── Canvas summary — only when this session is linked to a canvas.
     Gives the agent the inventory of shapes and edges plus stable ids
     it can reference in `canvas_*` tool calls without a round-trip. */
  if (calling?.linkedCanvasId) {
    const summary = buildCanvasSummary(calling.linkedCanvasId);
    if (summary) {
      lines.push('');
      lines.push('---');
      lines.push(summary);
    }
  }

  return lines.join('\n');
}

/** Cap on the number of shapes / edges we list inline. Past this we
 *  truncate with a marker — the agent can still mutate the missing
 *  entries by id (via tool calls), it just can't browse them in the
 *  preamble. Picked to keep the section under ~3 KB even on a busy
 *  canvas so it doesn't dominate cache. */
const MAX_SHAPES_IN_SUMMARY = 80;
const MAX_EDGES_IN_SUMMARY = 80;

/** Compact canvas-state preamble. Returns an empty string if the canvas
 *  was deleted between linking and now (callers skip the section). */
function buildCanvasSummary(canvasId: string): string {
  const c = ensureCanvasLoaded(canvasId);
  if (!c) return '';
  const lines: string[] = [];
  const bounds = computeCanvasBounds(c.shapes);
  lines.push(
    `Linked canvas: "${c.name}" (id ${c.id}, ${c.shapes.length} shape${c.shapes.length === 1 ? '' : 's'}, `
      + `${c.edges.length} edge${c.edges.length === 1 ? '' : 's'}, version ${c.version}).`
  );
  if (bounds) {
    lines.push(
      `Content AABB in canvas px: [${Math.round(bounds.x)},${Math.round(bounds.y)}]..`
        + `[${Math.round(bounds.x + bounds.w)},${Math.round(bounds.y + bounds.h)}].`
    );
  } else {
    lines.push('Canvas is empty.');
  }
  lines.push(
    'Use the `mcp__app__canvas_*` tools to draw, patch, or delete on this canvas. '
      + 'Shape ids below are STABLE — reuse them in `canvas_add_edge`, '
      + '`canvas_update_shape`, `canvas_delete_shape`, `canvas_focus`. '
      + 'When wiring up multiple connectors at once, prefer the batch '
      + '`canvas_add_edges` tool ({"edges":[{from, to}, …]}) over calling '
      + '`canvas_add_edge` N times — it lands as one ⌘Z step and saves '
      + 'round-trips. Edge specs accept short field names too: '
      + '`from`/`to`/`source`/`target` are aliases for `from_shape_id`/'
      + '`to_shape_id`.'
  );
  lines.push(
    'A PNG snapshot of this canvas is attached to the user\'s current '
      + 'message — read it as a visual companion to the JSON inventory '
      + 'below. The PNG is regenerated every turn so it always reflects '
      + 'the live state. The inventory is the SOURCE OF TRUTH for ids '
      + 'and exact coordinates; the PNG is what helps with layout '
      + 'aesthetics, freehand strokes, image content, and visual '
      + 'reasoning ("does this read as balanced?", "is the arrow '
      + 'pointing at the right node?").'
  );
  lines.push(
    'Color guidance: DO NOT set `props.color`, `props.fill`, or '
      + '`props.stroke` on text / sticky / rect / ellipse shapes unless '
      + 'the user explicitly asked for a color. The renderer\'s defaults '
      + 'are theme-aware (work on dark + light); your custom colors '
      + 'usually break contrast. If you want to GROUP related shapes '
      + 'visually, prefer `canvas_group` (a frame around them) over '
      + 'colored fills. If you really want a hint of color, use sticky '
      + 'shapes with `props.tint = "yellow" | "pink" | "blue" | "green" | '
      + '"gray" | "forge"` — those tints are translucent and stay '
      + 'readable.'
  );

  if (c.shapes.length > 0) {
    lines.push('');
    lines.push('Shapes:');
    const shown = c.shapes.slice(0, MAX_SHAPES_IN_SUMMARY);
    for (const s of shown) {
      lines.push(`  - ${formatShapeForSummary(s)}`);
    }
    if (c.shapes.length > MAX_SHAPES_IN_SUMMARY) {
      lines.push(`  - … ${c.shapes.length - MAX_SHAPES_IN_SUMMARY} more shape(s) omitted`);
    }
  }

  if (c.edges.length > 0) {
    lines.push('');
    lines.push('Edges:');
    const shown = c.edges.slice(0, MAX_EDGES_IN_SUMMARY);
    for (const e of shown) {
      lines.push(`  - ${formatEdgeForSummary(e)}`);
    }
    if (c.edges.length > MAX_EDGES_IN_SUMMARY) {
      lines.push(`  - … ${c.edges.length - MAX_EDGES_IN_SUMMARY} more edge(s) omitted`);
    }
  }

  return lines.join('\n');
}

function computeCanvasBounds(shapes: Shape[]): { x: number; y: number; w: number; h: number } | null {
  if (shapes.length === 0) return null;
  let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
  for (const s of shapes) {
    if (s.x < minX) minX = s.x;
    if (s.y < minY) minY = s.y;
    if (s.x + s.w > maxX) maxX = s.x + s.w;
    if (s.y + s.h > maxY) maxY = s.y + s.h;
  }
  return { x: minX, y: minY, w: maxX - minX, h: maxY - minY };
}

/** One shape, one line — id, kind, bbox, the most descriptive prop
 *  per kind. We deliberately don't dump full props (mermaid sources,
 *  full freehand point lists) here — the agent can ask via a future
 *  read tool / inspect via individual updates. Goal is "give the
 *  agent enough to address the shape", not full state. */
function formatShapeForSummary(s: Shape): string {
  const bbox = `${Math.round(s.x)},${Math.round(s.y)} ${Math.round(s.w)}x${Math.round(s.h)}`;
  const meta = describeShapeProps(s);
  const label = s.label ? ` "${s.label}"` : '';
  return `${s.id} ${s.kind} (${bbox})${meta ? ' ' + meta : ''}${label}`;
}

function describeShapeProps(s: Shape): string {
  const p = s.props as Record<string, unknown>;
  switch (s.kind) {
    case 'text':
    case 'sticky': {
      const body = (typeof p.text === 'string' ? p.text : (typeof p.markdown === 'string' ? p.markdown : '')).trim();
      if (!body) return '';
      const oneline = body.replace(/\s+/g, ' ');
      return `text="${oneline.length > 40 ? oneline.slice(0, 37) + '…' : oneline}"`;
    }
    case 'mermaid':
    case 'dot':
    case 'plantuml': {
      const src = typeof p.source === 'string' ? p.source : '';
      const first = src.split('\n')[0]?.trim() ?? '';
      return `source.0="${first.length > 40 ? first.slice(0, 37) + '…' : first}"`;
    }
    case 'code': {
      const lang = typeof p.language === 'string' ? p.language : '';
      return lang ? `language=${lang}` : '';
    }
    case 'jira-card':         return `ticketKey=${typeof p.ticketKey === 'string' ? p.ticketKey : '?'}`;
    case 'github-pr-card':
    case 'github-issue-card': return `${p.owner}/${p.repo}#${p.number}`;
    case 'sentry-event-card': return `shortId=${p.shortId ?? p.issueId ?? '?'}`;
    case 'file-card':         return `path=${typeof p.relPath === 'string' ? p.relPath : ''}`;
    default:                  return '';
  }
}

function formatEdgeForSummary(e: Edge): string {
  const fromAnchor = 'anchor' in e.from ? e.from.anchor : 'offset';
  const toAnchor   = 'anchor' in e.to   ? e.to.anchor   : 'offset';
  const label = e.label ? ` "${e.label}"` : '';
  return `${e.id} ${e.from.shapeId}.${fromAnchor} → ${e.to.shapeId}.${toAnchor} [${e.kind}/${e.routing}]${label}`;
}

/* `canvasState` is referenced indirectly through `ensureCanvasLoaded`,
   but TS will complain it's imported and unused without this no-op
   reference. Re-exporting keeps the intent visible to anyone reading
   this file. */
void canvasState;
