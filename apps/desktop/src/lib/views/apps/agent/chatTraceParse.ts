// Pure trace-segment parsing utilities — extracted from
// ChatThread.svelte in wave-1 phase-6 refactor. The renderer uses
// these to turn the agent CLI's wire format ("‹toolcall›…‹/toolcall›"
// segments with optional spliced-in "‹output›…‹/output›") into a
// structured ToolHint that the trace-step template can render with
// a per-kind icon + label.
//
// No DOM, no Svelte state, no IPC — just string-in / object-out, so
// these are trivially unit-testable in isolation and can be reused
// by other surfaces (e.g. a future "agent activity feed" view that
// shares the same wire format).

/** Tool kinds we render with a dedicated icon + colour. Anything else
 *  falls through to the neutral `unknown` style — still renders, just
 *  without the per-tool flair. */
export type ToolKind =
  | 'read' | 'edit' | 'write' | 'create' | 'delete'
  | 'bash' | 'grep' | 'glob' | 'webfetch' | 'websearch'
  | 'todo' | 'todos' | 'switch_cwd' | 'commit' | 'pr'
  | 'ask'
  | 'mcp' | 'unknown';

export type ToolHint = {
  kind: ToolKind;
  /** Human-readable verb shown on the chip ("Read", "Bash", "Grep"…). */
  label: string;
  /** Primary subject — usually a path or command body. Rendered mono. */
  target: string;
  /** Optional secondary qualifier ("in <path>" for grep, "(L12–)" for read). */
  scope: string;
};

export type ParsedTraceSegment =
  | { kind: 'tool'; cmd: string; output: string }
  | { kind: 'text' };

/** Parse a single trace segment. `appendToLastTrace` /
 *  `attachOutputToLastTrace` (in `sessions.svelte.ts`) wrap each tool
 *  invocation in unicode guillemet markers (U+2039/U+203A —
 *  `‹toolcall›…‹/toolcall›` and `‹output›…‹/output›`) so they don't
 *  collide with literal HTML the agent might emit in its prose. We
 *  forgive minor layout variation (inline vs newline, missing close
 *  on a partial stream) and also accept legacy plain-angle
 *  `<toolcall>…` for any pre-migration message that survived in the
 *  persisted log. Plain segments (already markdown-formatted by
 *  `formatToolUse`) fall through to `{ kind: 'text' }`. */
export function parseTraceSegment(seg: string): ParsedTraceSegment {
  /* Detect either marker style. The unicode guillemets are the
     canonical wrapping today; plain `<…>` survives only on old
     persisted messages. */
  if (!/[‹<](toolcall|output)\b/.test(seg)) return { kind: 'text' };
  function extract(tag: 'toolcall' | 'output'): string {
    /* Try unicode markers first, then plain. Closed pair preferred,
       falling back to "open + rest of segment" for partial streams. */
    const closedU = new RegExp(`‹${tag}›([\\s\\S]*?)‹\\/${tag}›`).exec(seg);
    if (closedU) return closedU[1];
    const closedA = new RegExp(`<${tag}>([\\s\\S]*?)<\\/${tag}>`).exec(seg);
    if (closedA) return closedA[1];
    const openU = new RegExp(`‹${tag}›([\\s\\S]*)`).exec(seg);
    if (openU) return openU[1];
    const openA = new RegExp(`<${tag}>([\\s\\S]*)`).exec(seg);
    return openA ? openA[1] : '';
  }
  /* Strip any inner output chunk first — output is spliced INSIDE
     the toolcall envelope by `attachOutputToLastTrace`, so the
     closing toolcall marker only matches AFTER the output. Without
     this strip, cmd would include the whole captured text. Then
     drop any stray leftover tag markers + the leading `$ ` shell
     prompt (the ▸ glyph already conveys "this is a command"). */
  function clean(s: string, dropOutput: boolean): string {
    let r = s;
    if (dropOutput) {
      r = r.replace(/‹output›[\s\S]*?‹\/output›/g, '');
      r = r.replace(/<output>[\s\S]*?<\/output>/g, '');
    }
    return r
      .replace(/‹\/?toolcall›/g, '')
      .replace(/‹\/?output›/g, '')
      .replace(/<\/?toolcall>/g, '')
      .replace(/<\/?output>/g, '')
      .trim();
  }
  let cmd = clean(extract('toolcall'), true);
  cmd = cmd
    .replace(/^[`'"]?\s*\$\s+/, '')
    .replace(/[`'"]$/, '')
    .trim();
  const output = clean(extract('output'), false);
  return { kind: 'tool', cmd, output };
}

/** Convert a `formatToolUse`-shaped hint string back into structure
 *  so the trace renderer can pick an icon/colour/label per tool kind
 *  instead of dumping every step as a same-looking `$ …` pill. Kept
 *  on the UI side because the over-the-wire format (`_read_ \`path\``)
 *  is markdown-stable and shared across both agents, so any
 *  structural decoration belongs in the renderer. */
export function parseToolHint(raw: string): ToolHint {
  const fallback = (k: ToolKind, label: string, target = ''): ToolHint => ({
    kind: k, label, target, scope: '',
  });
  const s = raw.trim();
  /* Bash carries no italics — it ships as `` `$ command` ``. The
     leading `$ ` was already stripped by `parseTraceSegment` so
     what's left is the bare command body. */
  if (!s.startsWith('_')) {
    /* Could still be a generic Markdown line; treat the whole thing
       as a Bash command if it looks like one (no leading `_kind_`
       marker and no markdown emphasis at all). */
    return fallback('bash', 'Bash', s.replace(/^`|`$/g, ''));
  }
  /* `_kind_ \`primary\`[ in \`secondary\`]` — italics + inline-code.
     We tolerate optional trailing parens like `(L12–)` from Read. */
  const m = /^_([a-zA-Z][\w. ]*?)_\s*(.*)$/.exec(s);
  if (!m) return fallback('unknown', 'Tool', s);
  const verb = m[1].toLowerCase().trim();
  const rest = m[2].trim();
  const codes = [...rest.matchAll(/`([^`]+)`/g)].map((mm) => mm[1]);
  const primary = codes[0] ?? '';
  const secondary = codes[1] ?? '';
  /* Pick out trailing parenthetical hint from Read (`(L12–)`). */
  const parenMatch = / \(([^)]+)\)\s*$/.exec(rest);
  const paren = parenMatch ? parenMatch[1] : '';
  const inMatch = / in $/.test(rest.split('`')[2] ?? '');
  const scope = secondary ? (inMatch ? `in ${secondary}` : secondary) : paren;

  /* Map verb → kind + nice label. The verb space includes mcp
     calls flattened by formatToolUse (`jira.get_issue`,
     `app.open_github_pr`, …) — we treat the whole `mcp.*`
     family as one kind but show the dotted name as the label. */
  if (verb === 'read') return { kind: 'read', label: 'Read', target: primary, scope };
  if (verb === 'edit') return { kind: 'edit', label: 'Edit', target: primary, scope };
  if (verb === 'write') return { kind: 'write', label: 'Write', target: primary, scope };
  if (verb === 'grep') return { kind: 'grep', label: 'Grep', target: primary, scope };
  if (verb === 'glob') return { kind: 'glob', label: 'Glob', target: primary, scope };
  if (verb === 'todos') {
    // `formatTodos` ships either "_todos_ \`N items · k done · …\`"
    // or "_todos_ \`…\` — Active label". The label-after-em-dash is
    // captured into `rest` past the first inline-code, so reconstruct
    // it here as the row's scope (rendered to the right of the
    // summary by the trace template).
    const afterCode = rest.replace(/`[^`]+`/, '').trim();
    const trailing = afterCode.startsWith('—') ? afterCode.slice(1).trim() : '';
    return {
      kind: 'todos',
      label: 'Update todos',
      target: primary,
      scope: trailing,
    };
  }
  if (verb === 'webfetch') return { kind: 'webfetch', label: 'Fetch', target: primary, scope };
  if (verb === 'websearch') return { kind: 'websearch', label: 'Search', target: primary, scope };
  if (verb === 'switch cwd') return { kind: 'switch_cwd', label: 'Switch cwd', target: primary, scope };
  if (verb === 'commit') return { kind: 'commit', label: 'Commit', target: primary, scope };
  if (verb === 'open pr') return { kind: 'pr', label: 'PR', target: primary, scope };
  if (verb === 'ask') return { kind: 'ask', label: 'Ask', target: primary, scope };
  if (verb === 'notebook edit') return { kind: 'edit', label: 'Notebook', target: primary, scope };
  if (verb === 'using bash…' || verb === 'propose bash…') {
    return { kind: 'bash', label: 'Bash', target: primary, scope };
  }
  /* mcp__server__tool gets flattened to `server.tool` by formatToolUse. */
  if (verb.includes('.')) {
    const segs = verb.split('.');
    const server = segs[0];
    const tool = segs.slice(1).join('.').replace(/_/g, ' ');
    return { kind: 'mcp', label: `${server} · ${tool}`, target: primary, scope };
  }
  /* Fallback: surface the verb as the label, keep its own
     capitalisation (without the underscores formatToolUse used). */
  return {
    kind: 'unknown',
    label: verb.replace(/_/g, ' ').replace(/^./, (c) => c.toUpperCase()),
    target: primary,
    scope,
  };
}
