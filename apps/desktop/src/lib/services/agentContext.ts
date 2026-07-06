// Per-turn system-prompt suffix builder for Claude agent runs.
// Pure function over `layoutState` + `sessionsState`. No DOM, no events.
//
// Prompt-cache rule: this builder returns TWO strings.
//   `system` — stable across turns (header, nav guide, discipline
//      blocks, auto-memory, CLAUDE.md). The caller
//      passes it as `--append-system-prompt`, so it caches as a byte-
//      stable prefix and the whole conversation reads from cache.
//   `turn` — volatile every turn (solo-layout snapshot, one-shot
//      cwd recap, canvas summary). The caller appends it to the USER
//      MESSAGE. It used to live at the tail of the system prompt,
//      which broke the cache prefix whenever the layout changed and
//      forced the entire conversation to re-write at cache-write rate
//      (measured ~$15–47 wasted on a heavy session). In the user
//      message it's fresh content either way, so zero cache loss.

import { layoutState, APP_INSTANCE_IDS, DEFAULT_PANEL_ORDER, MULTI_INSTANCE_KINDS, kindForInstanceId } from '$lib/state/layout.svelte';
import { sessionsState, editorRoots } from '$lib/state/sessions.svelte';
import { canvasState, ensureCanvasLoaded, type Shape, type Edge } from '$lib/state/canvas.svelte';
import { getCachedClaudeMd } from '$lib/state/claudemd.svelte';
import { getCachedAutoMemoryBlock } from '$lib/state/autoMemory.svelte';

/** Build the per-turn app-context string we hand the agent as a
 *  system-prompt suffix. Lists each solo singleton (kind + id), the
 *  editor's open path, the active agent session per kind, and any
 *  editor↔agent / terminal links. Re-derived on every turn so it's
 *  always current. */
export function buildAgentAppContext(callingSessionId: string): { system: string; turn: string } {
  const lines: string[] = [];

  const calling = sessionsState.list.find((s) => s.id === callingSessionId);

  // ── Static section: header + navigation tool guide. Same bytes on
  // every turn (modulo a Woom deploy) so prompt caches eat it.
  lines.push(
    'You are running inside Woom, a desktop app organised as solo '
      + 'modes — one full-screen surface per source (Jira / GitHub / '
      + 'Sentry / Claude / Editor / Canvas / Terminal). Source '
      + 'and agent kinds are singletons; editor / canvas / terminal can '
      + 'have MULTIPLE instances open at once — each is listed separately '
      + 'below with its own id + name (e.g. an editor named "Vermeer" '
      + 'open on repo A and "Klimt" on repo B are two distinct rows). '
      + 'Address any of them by id. Navigate the UI via `mcp__app__*` tools.'
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
  // Ask-user-question discipline. Default Claude behaviour is to
  // pose clarifications in prose ("Confirm? A / B / C"), which ends
  // the turn + forces a re-context on the next message AND skips
  // Woom's interactive card UI. Push the agent toward the MCP tool
  // for any branch-point that the user's preference will resolve.
  lines.push('');
  lines.push(
    'User-clarification discipline. When you need a binary choice '
      + '("proceed / abort"), a small option set ("which of A / B / '
      + 'C"), or a yes/no confirmation from the user — USE the '
      + '`mcp__app__ask_user_question` tool, NOT prose. Prose like '
      + '"Confirm?" ends your turn and loses the structured-card UX. '
      + 'The tool BLOCKS until the user picks (or types an "Other") '
      + 'and returns their answer in the SAME turn, so you reason '
      + 'about the choice immediately and continue. Reserve prose for '
      + 'open-ended questions and explanations. Don\'t use it for '
      + 'destructive-action confirmation (that\'s `propose_bash` / '
      + '`propose_commit` / `propose_pr` — they have approval semantics).'
  );

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
      + 'plain words (each matches as a prefix; rows matching all '
      + 'words rank first, partial matches fill the rest — extra '
      + 'words refine, not zero out). If empty on the first try, '
      + 'the memory genuinely isn\'t there.\n'
      + 'If the first call returns an empty result, narrowing then '
      + 'is free — but never broaden after a hit. Pagination > '
      + 're-querying.'
  );

  // Long-wait discipline. The single biggest UX bug we see today is
  // the agent firing a foreground bash polling loop (`until …; do
  // sleep 15; done`) to wait for a CI run / a notarize submit / an
  // RDS create to finish. The Bash tool blocks the turn until the
  // loop exits — which can be 5+ minutes — and the chat surface
  // visually freezes. Push the agent toward the bg-task family
  // (`mcp__app__bg_*`) for ANYTHING that's "wait for an external
  // thing to finish".
  lines.push('');
  lines.push(
    'Long-wait discipline. NEVER write a foreground bash polling '
      + 'loop (`until [...]; do sleep N; done`, `while true; sleep`, '
      + '`gh run watch`, etc.) — they block the turn for the entire '
      + 'wait + the chat surface visually freezes. Instead:\n'
      + '  - For "wait until CI / build / notarize / RDS provision '
      + 'finishes" → spawn the wait as a background task via '
      + '`mcp__app__bg_spawn` with the actual polling script as '
      + '`command`, then call `mcp__app__bg_wait_line(id, '
      + 'contains="<readiness signal>", timeout_ms=…)`. The task '
      + 'streams output into the Preview pane and the agent gets a '
      + 'discrete return value when the signal lands — no '
      + 'foreground blocking.\n'
      + '  - For "fire a one-shot wait of N seconds in the '
      + 'background and come back to it next turn" → run the wait '
      + 'inside `mcp__app__bg_spawn` (don\'t use Bash\'s `&` — that '
      + 'detaches WITH the Bash tool process and you lose the '
      + 'handle). The user sees the task in the Preview pane and '
      + 'gets a notification when it completes.\n'
      + '  - For "poll a remote until success without holding the '
      + 'chat" → schedule via the same bg pattern with `until` '
      + 'INSIDE the spawned script body, NOT in the Bash tool '
      + 'call.\n'
      + '  - If the user explicitly typed "wait for X and tell me" '
      + 'or "run rerun and tell me when it\'s done" → bg_spawn the '
      + 'poller, then immediately reply that the wait is running '
      + 'in the background and end the turn. The bg-task '
      + 'completion event re-arms the chat without you holding it '
      + 'hostage.'
  );

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

  // ── Volatile section: solo layout snapshot + one-shot cwd-switch
  // recap + canvas summary. Returned SEPARATELY (as `turn`) so the
  // caller appends it to the user-turn MESSAGE, not the cached
  // system prompt. These bytes change every turn; keeping them out
  // of `--append-system-prompt` makes the system prefix byte-stable,
  // so the whole conversation cache reads instead of re-writing when
  // the user moves around the UI. Verified ~$15–47/heavy-session of
  // cache-write waste from the old in-system placement.
  const turnLines: string[] = [];
  turnLines.push('Current solo-mode layout (refreshed on every turn):');

  // One-shot recap if the user just switched the agent's cwd. Cleared
  // after the turn ships (in sendClaudeMessage's success path).
  if (calling?.cwdSwitchRecap) {
    turnLines.push('');
    turnLines.push(calling.cwdSwitchRecap);
  }

  for (const kind of DEFAULT_PANEL_ORDER) {
    /* Multi-instance kinds (editor/canvas/terminal) can have several
       columns open at once — emit ONE row per instance so the agent
       sees every editor + every open repo, not just the primary. Source
       and agent kinds stay singletons (one row at the legacy id). */
    const instances = MULTI_INSTANCE_KINDS.has(kind)
      ? layoutState.instances[kind]
      : [{ id: APP_INSTANCE_IDS[kind], name: undefined as string | undefined }];

    for (const inst of instances) {
      const id = inst.id;
      const meta: string[] = [`kind=${kind}`, `id=${id}`];
      /* Multi-instance kinds carry a curated display name (e.g.
         "Vermeer") that MCP tools use as the agent-facing handle.
         Surface it next to the id so the agent picks the readable form. */
      if (MULTI_INSTANCE_KINDS.has(kind) && inst.name) meta.push(`name=${inst.name}`);

      if (kind === 'editor') {
        /* An editor instance can hold MULTIPLE open roots (multi-root
           workspace, like VS Code's "Open Folder" stacking). Surface
           ALL of them — `repo_roots=[a, b]` when >1, `repo_path=a` for
           the common single-root case. Falls back to the global
           active-editor repo only for the primary instance (legacy
           single-editor state with no per-instance root set). */
        const roots = editorRoots(id);
        if (roots.length > 1) {
          meta.push(`repo_roots=[${roots.join(', ')}]`);
        } else {
          const path = roots[0]
            ?? (id === APP_INSTANCE_IDS.editor ? layoutState.active.editor.repoPath : null)
            ?? '';
          meta.push(`repo_path=${path || '(none)'}`);
        }
        /* Currently-open file in THIS editor instance. EditorView mirrors
           `activePath` into localStorage under `woom:editor:active:<id>`
           on every change, keyed per instance — so each editor row
           reflects what the user is actually looking at in that column. */
        try {
          const openFile = localStorage.getItem(`woom:editor:active:${id}`);
          if (openFile && openFile.trim()) meta.push(`open_file=${openFile}`);
        } catch { /* localStorage access denied — non-essential */ }
        const linked = sessionsState.list
          .filter((s) => s.linkedToEditor && s.linkedToEditorInstanceId === id && !s.archived)
          .map((s) => s.title || s.id.slice(0, 6));
        if (linked.length) meta.push(`linked_agents=[${linked.join(', ')}]`);
      }

      if (kind === 'claude') {
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
            /* Name the SPECIFIC linked editor instance (id + name), not
               just its kind — with multiple editors open, "linked_to_editor=editor"
               alone was ambiguous and made the agent read the wrong column. */
            const linkInst = layoutState.instances.editor.find(
              (i) => i.id === sess.linkedToEditorInstanceId
            );
            if (linkInst) {
              meta.push(`linked_to_editor=${linkInst.name} (id=${linkInst.id})`);
            } else if (linkKind) {
              meta.push(`linked_to_editor=${linkKind} (id=${sess.linkedToEditorInstanceId})`);
            }
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
      turnLines.push(`  - ${meta.join(', ')}${isYou ? '  ← THIS IS YOU' : ''}`);
    }
  }

  /* ── Canvas summary — only when this session is linked to a canvas.
     Gives the agent the inventory of shapes and edges plus stable ids
     it can reference in `canvas_*` tool calls without a round-trip.
     Volatile (version bumps on every edit) so it rides the turn
     message alongside the canvas PNG, not the cached system prompt. */
  if (calling?.linkedCanvasId) {
    const summary = buildCanvasSummary(calling.linkedCanvasId);
    if (summary) {
      turnLines.push('');
      turnLines.push('---');
      turnLines.push(summary);
    }
  }

  return { system: lines.join('\n'), turn: turnLines.join('\n') };
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
