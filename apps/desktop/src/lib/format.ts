// Pure presentation helpers — no Svelte, no reactivity, no I/O. Imported by
// the solo UI to format paths, tool calls, byte counts, etc. Kept off
// the +page.svelte shell so the same helpers can be reused across extracted
// components without a circular import.

/** Tool names we never render — pure plumbing that would pollute the transcript. */
export const HIDDEN_TOOLS = new Set(['ToolSearch', 'ScheduleWakeup', 'AskUserQuestion']);

export function shortPath(p: string | null): string {
  if (!p) return 'No folder';
  return shortenFsPath(p);
}

export function shortRemote(url: string): string {
  // git@github.com:acme/repo.git → acme/repo
  // https://github.com/acme/repo.git → acme/repo
  const m = url.match(/[:/]([^:/]+\/[^/]+?)(?:\.git)?\/?$/);
  return m ? m[1] : url;
}

export function shortenFsPath(p: string): string {
  const home = '/Users/';
  if (p.startsWith(home)) {
    const rest = p.slice(home.length);
    const slash = rest.indexOf('/');
    if (slash > 0) return '~/' + rest.slice(slash + 1);
  }
  const parts = p.split('/').filter(Boolean);
  if (parts.length > 2) return '…/' + parts.slice(-2).join('/');
  return p;
}

/** Trim a long absolute path down to its tail — strips the user's home dir
    and keeps at most the last 2 segments, e.g. `e2e/README.md`. Preserves
    plain filenames and already-short relatives untouched. Distinct from
    `shortenFsPath` above, which keeps a full `~/...` path (used by the
    session/worktree labels where more context matters). */
export function toolPathLabel(p: string): string {
  if (!p) return '';
  const homeMatch = p.match(/^\/Users\/[^/]+\//);
  const stripped = homeMatch ? p.slice(homeMatch[0].length) : p.replace(/^\//, '');
  const segs = stripped.split('/').filter(Boolean);
  if (segs.length <= 2) return segs.join('/');
  return segs.slice(-2).join('/');
}

/** Turn a `tool_use` block into a compact human-readable line of markdown.
    Return `''` to hide the tool call from the transcript entirely. */
export function formatToolUse(name: string, input: Record<string, unknown>): string {
  if (HIDDEN_TOOLS.has(name)) return '';
  const s = (k: string) => (typeof input[k] === 'string' ? (input[k] as string) : '');

  if (name === 'Bash') {
    const cmd = s('command');
    return cmd ? mdInlineCode(`$ ${truncInline(cmd, 400)}`) : `_using Bash…_`;
  }
  if (name === 'Read') {
    // Accept Claude's `file_path` AND cursor-agent's raw `path` /
    // `target_file` aliases. Earlier we'd render `_read_` (no target)
    // when the cursor-side flattening in `cursor.rs` missed a freshly
    // shipped variant — surfacing a row labeled "READ" with NO file
    // beside it. Falling through to the alternate keys + a last-ditch
    // string scan keeps the row informative until the canonical key
    // wins again.
    const fp = s('file_path') || s('path') || s('target_file') || s('filePath');
    const range = input.offset || input.limit ? ` (L${input.offset ?? 1}–)` : '';
    return fp ? `_read_ ${mdInlineCode(toolPathLabel(fp))}${range}` : `_read_ \`(args pending)\``;
  }
  if (name === 'Edit' || name === 'Write') {
    const fp = s('file_path') || s('path') || s('target_file') || s('filePath');
    return fp
      ? `_${name.toLowerCase()}_ ${mdInlineCode(toolPathLabel(fp))}`
      : `_${name.toLowerCase()}_ \`(args pending)\``;
  }
  if (name === 'Grep') {
    // Cursor's grep variant has shipped both `query` and `regex` over
    // time; accept either alongside the canonical `pattern`.
    const pattern = s('pattern') || s('query') || s('regex');
    const path = s('path') || s('include') || s('glob');
    const inPath = path ? ` in ${mdInlineCode(toolPathLabel(path))}` : '';
    return pattern
      ? `_grep_ ${mdInlineCode(truncInline(pattern, 120))}${inPath}`
      : `_grep_ \`(args pending)\``;
  }
  if (name === 'Glob') {
    // Same defensive aliasing — different cursor builds have used
    // `pattern`, `glob`, and `glob_pattern` interchangeably.
    const pattern = s('pattern') || s('glob') || s('glob_pattern');
    return pattern ? `_glob_ ${mdInlineCode(pattern)}` : `_glob_ \`(args pending)\``;
  }
  if (name === 'WebFetch' || name === 'WebSearch') {
    const q = s('url') || s('query') || s('prompt');
    return q
      ? `_${name.toLowerCase()}_ ${mdInlineCode(truncInline(q, 120))}`
      : `_${name.toLowerCase()}_`;
  }
  if (name === 'TodoWrite') {
    return formatTodos(input);
  }
  if (name === 'NotebookEdit') {
    const fp = s('notebook_path') || s('file_path');
    return fp ? `_notebook edit_ ${mdInlineCode(toolPathLabel(fp))}` : `_notebook edit_`;
  }
  if (name === 'mcp__app__ask_user_question') {
    /* Special hint for the interactive question card. The renderer
     * recognises `_ask_` and swaps the trace row for a QuestionCard
     * inline, matching it back to the pending action by question
     * text. We carry the FULL question text (untruncated) so the
     * match is robust — the renderer truncates for display. */
    const q = s('question');
    return q ? `_ask_ ${mdInlineCode(q)}` : `_ask…_`;
  }
  if (name === 'mcp__app__propose_bash' || name === 'mcp__github__propose_bash') {
    const cmd = s('command');
    return cmd ? mdInlineCode(`$ ${truncInline(cmd, 400)}`) : `_propose bash…_`;
  }
  if (name === 'mcp__app__propose_switch_cwd') {
    const path = s('path');
    return path ? `_switch cwd_ ${mdInlineCode(truncInline(path, 160))}` : `_switch cwd…_`;
  }
  if (name === 'mcp__github__propose_commit') {
    const msg = s('message') || s('subject');
    return msg ? `_commit_ ${mdInlineCode(truncInline(msg, 160))}` : `_propose commit…_`;
  }
  if (name === 'mcp__github__propose_pr') {
    const title = s('title');
    return title ? `_open pr_ ${mdInlineCode(truncInline(title, 160))}` : `_propose pr…_`;
  }
  if (name.startsWith('mcp__')) {
    // e.g. mcp__jira__get_issue → jira.get_issue
    const parts = name.split('__').slice(1);
    const label = parts.join('.');
    const keys = Object.keys(input);
    if (keys.length === 1 && typeof input[keys[0]] === 'string') {
      return `_${label}_ ${mdInlineCode(truncInline(input[keys[0]] as string, 160))}`;
    }
    const json = keys.length ? ` ${mdInlineCode(truncInline(JSON.stringify(input), 200))}` : '';
    return `_${label}_${json}`;
  }
  // Fallback: try to pick a single-string argument, otherwise just name it.
  const stringArgs = Object.entries(input).filter(([, v]) => typeof v === 'string');
  if (stringArgs.length === 1) {
    return `_${name}_ ${mdInlineCode(truncInline(stringArgs[0][1] as string, 200))}`;
  }
  return `_using ${name}…_`;
}

/** Render a TodoWrite / UpdateTodos call as a single compact summary
 *  line — "Update todos · 5 items · 2 done · 1 in progress". Earlier
 *  this dumped the whole bullet list into the trace, which pushed every
 *  meaningful step below the fold every time the agent rebalanced its
 *  todo list. The label that ends up rendered by `parseToolHint` (verb
 *  = "todos") shows the count + the active-form of the currently
 *  in-progress item so the user still gets a hint of "what is the
 *  agent doing right now" without scrolling past the entire plan. */
export function formatTodos(input: Record<string, unknown>): string {
  const todos = Array.isArray(input.todos) ? (input.todos as Array<Record<string, unknown>>) : [];
  if (todos.length === 0) return `_todos_`;
  let done = 0;
  let active = 0;
  let cancelled = 0;
  let activeLabel = '';
  for (const t of todos) {
    const status = typeof t.status === 'string' ? t.status : 'pending';
    if (status === 'completed') done += 1;
    else if (status === 'in_progress') {
      active += 1;
      if (!activeLabel) {
        const af = typeof t.activeForm === 'string' ? t.activeForm : '';
        const c = typeof t.content === 'string' ? t.content : '';
        activeLabel = af || c;
      }
    } else if (status === 'cancelled') cancelled += 1;
  }
  const parts: string[] = [`${todos.length} item${todos.length === 1 ? '' : 's'}`];
  if (done) parts.push(`${done} done`);
  if (active) parts.push(`${active} in progress`);
  if (cancelled) parts.push(`${cancelled} cancelled`);
  // Compact, single-line trace pill. `parseToolHint`'s `todos` branch
  // (added in ChatThread.svelte) gives this a checklist icon + brand
  // tint so the summary still reads as a todo step at a glance.
  const summary = parts.join(' · ');
  return activeLabel
    ? `_todos_ ${mdInlineCode(truncInline(summary, 80))} — ${truncInline(activeLabel, 80)}`
    : `_todos_ ${mdInlineCode(truncInline(summary, 80))}`;
}

export function truncInline(s: string, max: number): string {
  if (s.length <= max) return s;
  return s.slice(0, max) + '…';
}

export function formatBytes(n: number | null | undefined): string {
  if (!n || n <= 0) return '';
  if (n < 1024) return `${n}B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)}K`;
  return `${(n / 1024 / 1024).toFixed(1)}M`;
}

/** Wrap text as markdown inline code, safely handling backticks in content
    by choosing a delimiter longer than the longest run in `s` (CommonMark
    §6.1 rule). Without this, a pattern like `` DEVOPS-396`|branch`` `` breaks
    the surrounding `` `…` `` pair and renders as broken empty boxes. */
export function mdInlineCode(s: string): string {
  if (!s.includes('`')) return `\`${s}\``;
  let maxRun = 0;
  let cur = 0;
  for (const ch of s) {
    if (ch === '`') {
      cur += 1;
      if (cur > maxRun) maxRun = cur;
    } else {
      cur = 0;
    }
  }
  const delim = '`'.repeat(maxRun + 1);
  const padL = s.startsWith('`') ? ' ' : '';
  const padR = s.endsWith('`') ? ' ' : '';
  return `${delim}${padL}${s}${padR}${delim}`;
}

export function labelColorStyle(color: string): string {
  const hex = color.replace(/^#/, '').padStart(6, '0');
  return `--label-color: #${hex};`;
}

export function firstLine(text: string): string {
  return text.split('\n')[0];
}

export function restLines(text: string): string {
  return text.split('\n').slice(1).join('\n').trim();
}

const IMAGE_EXTS = new Set([
  'png', 'jpg', 'jpeg', 'webp', 'gif', 'bmp', 'svg', 'heic', 'heif', 'avif', 'ico', 'tiff', 'tif'
]);

export function isImagePath(path: string): boolean {
  const dot = path.lastIndexOf('.');
  if (dot < 0) return false;
  return IMAGE_EXTS.has(path.slice(dot + 1).toLowerCase());
}

/** Extract the last path segment (filename or dir name) from an absolute path.
    Strips trailing slash on directories so `/foo/bar/` → `bar`. */
export function basename(path: string): string {
  const trimmed = path.endsWith('/') ? path.slice(0, -1) : path;
  const slash = trimmed.lastIndexOf('/');
  return slash >= 0 ? trimmed.slice(slash + 1) : trimmed;
}

/** Parse a Jira-style duration string (`"1h 30m"`, `"45m"`, `"2h"`, `"1.5h"`,
 *  `"1d 2h"`, `"1w"`) into seconds. Jira's own convention: `w`=5d, `d`=8h,
 *  `h`=60m, `m`=60s. A bare number is interpreted as minutes. Returns `null`
 *  when the string can't be parsed. */
export function parseDuration(input: string): number | null {
  const s = input.trim().toLowerCase();
  if (!s) return null;
  if (/^\d+(\.\d+)?$/.test(s)) {
    const mins = Number(s);
    return Math.round(mins * 60);
  }
  const re = /(\d+(?:\.\d+)?)\s*([wdhms])/g;
  let total = 0;
  let matched = false;
  let m: RegExpExecArray | null;
  while ((m = re.exec(s)) !== null) {
    matched = true;
    const n = parseFloat(m[1]);
    switch (m[2]) {
      case 'w': total += n * 5 * 8 * 3600; break;
      case 'd': total += n * 8 * 3600; break;
      case 'h': total += n * 3600; break;
      case 'm': total += n * 60; break;
      case 's': total += n; break;
    }
  }
  if (!matched) return null;
  return Math.round(total);
}

/** Format a positive seconds count into the same `"1h 30m"` shape Jira
 *  surfaces in its UI. Returns `"0m"` for zero/negative inputs. */
export function formatDuration(seconds: number): string {
  if (!seconds || seconds <= 0) return '0m';
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  const parts: string[] = [];
  if (h) parts.push(`${h}h`);
  if (m) parts.push(`${m}m`);
  if (parts.length === 0) parts.push(`${seconds}s`);
  return parts.join(' ');
}

/** Format a Date into Jira's `started` field shape
 *  (`yyyy-MM-ddTHH:mm:ss.SSSZZZZ`, e.g. `2026-04-24T14:05:00.000+0000`).
 *  Jira rejects plain `Z` suffixes — it expects `+hhmm` / `-hhmm`. */
export function jiraStartedString(d: Date): string {
  const pad = (n: number, w = 2) => String(n).padStart(w, '0');
  const offsetMin = -d.getTimezoneOffset();
  const sign = offsetMin >= 0 ? '+' : '-';
  const oh = pad(Math.floor(Math.abs(offsetMin) / 60));
  const om = pad(Math.abs(offsetMin) % 60);
  return (
    `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}` +
    `T${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}` +
    `.${pad(d.getMilliseconds(), 3)}${sign}${oh}${om}`
  );
}
