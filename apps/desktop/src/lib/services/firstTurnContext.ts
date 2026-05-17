// First-turn context preamble. On the very first user message of a
// session we don't yet have ANY conversation history for the agent to
// rely on — no prior tool calls, no recap, just the user's question
// landing on a cold CLI. This builder hands the agent a structured
// snapshot of the working directory (git branch + status + recent
// commits, presence of CLAUDE.md / AGENTS.md, basic file count) so
// it doesn't open the turn with `git status` / `pwd` / `ls` calls of
// its own — which it currently does, costing a tool-use round-trip
// before the user's question is answered.
//
// Wired in `+page.svelte` send-message flow: when
// `sess.messages.filter(role==='user').length === 1` (we're about to
// dispatch the first user turn), the preamble is built and stamped
// onto `cwdSwitchRecap` so `buildAgentAppContext` folds it into the
// system prompt suffix on this exact turn. cwdSwitchRecap is a
// one-shot channel (cleared after the turn ships), so subsequent
// turns are unaffected.

import { invoke } from '@tauri-apps/api/core';

/** Top-N memory rows returned by `memory_search_local`. Kept structural
 *  here so the helper doesn't pull in `serde`-generated TS shapes. */
interface MemoryHit {
  id: number;
  kind: string;
  content: string;
  tags: string;
  created_at: number;
}

/** Stable localStorage key the Editor view uses to mirror its active
 *  file path. Mirror is one-way (Editor → localStorage) — we only
 *  read here; never write. Drift in this format would break the
 *  preamble's "open in editor" line, not anything critical. */
function activeEditorFileKey(instanceId: string): string {
  return `woom:editor:active:${instanceId}`;
}

export function getActiveEditorFile(instanceId: string): string | null {
  try {
    const v = localStorage.getItem(activeEditorFileKey(instanceId));
    return v && v.trim() ? v : null;
  } catch {
    return null;
  }
}

/** Parallel-friendly bash result wrapper. Mirrors the shape returned
 *  by the `bash_run` Tauri command but only carries the bits this
 *  module cares about. */
interface BashOut {
  ok: boolean;
  stdout: string;
  stderr: string;
}

async function bash(cwd: string, command: string, timeoutTagForErrors: string): Promise<BashOut> {
  try {
    const r = await invoke<{ ok: boolean; stdout: string; stderr: string }>(
      'bash_run',
      { cwd, command }
    );
    return r;
  } catch (e) {
    /* Tauri command timeout / spawn failure. Return a non-fatal
     * empty result so the preamble degrades gracefully — better to
     * ship without a section than to abort the whole user turn just
     * because `git status` choked. Tag carries the section name into
     * the dev console for debugging. */
    console.warn(`firstTurnContext.${timeoutTagForErrors}:`, e);
    return { ok: false, stdout: '', stderr: '' };
  }
}

/** Build the per-turn preamble. Returns null when:
 *    - `cwd` is null (no project to inspect)
 *    - cwd isn't a git repo (no meaningful output to surface)
 *
 *  Callers prepend the result to any existing recap. Empty string is
 *  never returned — callers can null-check and skip the stamp.
 *
 *  `editorFile` — absolute path of the file currently focused in the
 *  linked Editor instance, or null when no link / no open file. Gets
 *  surfaced as a "currently open" line so the agent doesn't have to
 *  guess what the user is looking at.
 *
 *  `recallTerms` — extra keywords appended to the cwd-basename memory
 *  query. Pass first-turn user-message excerpt or @-mention titles to
 *  bias recall toward the topic of the question. Empty is fine. */
export async function buildFirstTurnPreamble(
  cwd: string | null,
  editorFile: string | null = null,
  recallTerms: string[] = []
): Promise<string | null> {
  if (!cwd) return null;

  /* Single bash call that emits multiple sections separated by sentinel
   * markers. Doing 5 separate `bash_run` invocations would cost 5 IPC
   * round-trips + 5 sh-spawns on every first turn (~150-300ms on warm
   * disk); one shelled-out script with sentinels is one IPC and one
   * fork. Sentinels are improbable strings that won't appear in any
   * normal git/ls output. */
  const script = `
set +e
cd ${shellEscape(cwd)} || exit 0
echo "::WOOM::IS_GIT::"
git rev-parse --is-inside-work-tree 2>/dev/null
echo "::WOOM::BRANCH::"
git rev-parse --abbrev-ref HEAD 2>/dev/null
echo "::WOOM::STATUS::"
git status --porcelain=v1 --untracked-files=no 2>/dev/null | head -30
echo "::WOOM::UNTRACKED::"
git ls-files --others --exclude-standard 2>/dev/null | head -10
echo "::WOOM::LOG::"
git log --oneline -n 5 2>/dev/null
echo "::WOOM::REMOTE::"
git config --get remote.origin.url 2>/dev/null
echo "::WOOM::CLAUDE_MD::"
test -f CLAUDE.md && echo "yes" || echo "no"
echo "::WOOM::AGENTS_MD::"
test -f AGENTS.md && echo "yes" || echo "no"
echo "::WOOM::TOP_FILES::"
ls -1 2>/dev/null | head -20
echo "::WOOM::END::"
`.trim();

  /* Run shell snapshot + memory recall in parallel. Memory query is
   * built from cwd basename + caller-supplied recall terms (truncated
   * + de-duped). bash takes ~50-150ms; memory_search_local is
   * sub-millisecond on a sane store size, so the wall time is bounded
   * by bash. Both are wrapped so a failure on either side doesn't
   * abort the other. */
  const cwdBase = cwd.split('/').filter((s) => s.length > 0).pop() ?? '';
  const recallQuery = uniqueTerms([cwdBase, ...recallTerms])
    .slice(0, 5)
    .join(' ');
  const [r, hits] = await Promise.all([
    bash(cwd, script, 'gather'),
    recallQuery
      ? invoke<MemoryHit[]>('memory_search_local', { query: recallQuery, limit: 5 })
          .catch((e) => {
            console.warn('firstTurnContext.memory_search_local:', e);
            return [] as MemoryHit[];
          })
      : Promise.resolve([] as MemoryHit[])
  ]);
  if (!r.ok) return null;

  const sections = parseSections(r.stdout);
  const isGit = (sections.IS_GIT ?? '').trim() === 'true';
  if (!isGit) {
    /* Non-repo cwd: only surface top-level files + CLAUDE.md presence.
     * Skipping the git-only sections keeps the preamble tight. */
    const topFiles = (sections.TOP_FILES ?? '').trim();
    const hasClaude = (sections.CLAUDE_MD ?? '').trim() === 'yes';
    const hasAgents = (sections.AGENTS_MD ?? '').trim() === 'yes';
    if (!topFiles && !hasClaude && !hasAgents && hits.length === 0) return null;
    const out: string[] = ['Repository snapshot at session start:'];
    out.push(`- cwd: ${cwd} (NOT a git repo)`);
    if (editorFile) out.push(`- currently open in editor: ${editorFile}`);
    if (hasClaude) out.push('- CLAUDE.md present — READ IT before answering');
    if (hasAgents) out.push('- AGENTS.md present — READ IT before answering');
    if (topFiles) out.push(`- Top-level entries: ${topFiles.split('\n').join(', ')}`);
    appendMemoryRecallBlock(out, hits);
    return out.join('\n');
  }

  const branch = (sections.BRANCH ?? '').trim() || '(detached)';
  const status = (sections.STATUS ?? '').trim();
  const untracked = (sections.UNTRACKED ?? '').trim();
  const log = (sections.LOG ?? '').trim();
  const remote = (sections.REMOTE ?? '').trim();
  const hasClaude = (sections.CLAUDE_MD ?? '').trim() === 'yes';
  const hasAgents = (sections.AGENTS_MD ?? '').trim() === 'yes';

  const out: string[] = ['Repository snapshot at session start (use this — do NOT re-run `git status` / `pwd` / `ls` as the first tool calls):'];
  out.push(`- cwd: ${cwd}`);
  out.push(`- branch: ${branch}`);
  if (remote) out.push(`- remote: ${remote}`);
  if (editorFile) out.push(`- currently open in editor: ${editorFile}`);
  if (status) {
    out.push('- modified files:');
    for (const line of status.split('\n').slice(0, 30)) {
      out.push(`    ${line}`);
    }
  } else {
    out.push('- working tree: clean');
  }
  if (untracked) {
    const lines = untracked.split('\n').slice(0, 10);
    out.push(`- untracked (first ${lines.length}): ${lines.join(', ')}`);
  }
  if (log) {
    out.push('- recent commits:');
    for (const line of log.split('\n').slice(0, 5)) {
      out.push(`    ${line}`);
    }
  }
  if (hasClaude || hasAgents) {
    const files = [hasClaude ? 'CLAUDE.md' : null, hasAgents ? 'AGENTS.md' : null]
      .filter((x): x is string => x !== null);
    out.push(`- project rules present: ${files.join(', ')} — READ THESE before answering`);
  }
  appendMemoryRecallBlock(out, hits);
  return out.join('\n');
}

/** Format the recalled memory rows as a "Relevant prior memories"
 *  block and append into `out`. No-op when `hits` is empty so callers
 *  don't have to guard. Truncates each content line to keep the
 *  preamble bounded; the agent can call `mcp__memory__memory_get`
 *  for full content if it needs more. */
function appendMemoryRecallBlock(out: string[], hits: MemoryHit[]): void {
  if (hits.length === 0) return;
  out.push('');
  out.push(`Relevant prior memories (top ${hits.length}, oldest insights still apply):`);
  for (const h of hits) {
    const trimmed = h.content.length > 360
      ? h.content.slice(0, 357).replace(/\s+$/, '') + '…'
      : h.content;
    const tagSuffix = h.tags ? ` [${h.tags}]` : '';
    out.push(`  - #${h.id} kind=${h.kind}${tagSuffix}: ${trimmed.replace(/\n+/g, ' ')}`);
  }
  out.push('Read these BEFORE re-asking the user for context they\'ve already given.');
}

/** Deduplicate + clean a list of recall keywords. Lowercased, empty
 *  strings stripped, FTS5 metachars dropped (the Rust side does its
 *  own sanitization but this trims noise before that). Preserves
 *  first-occurrence order so the cwd basename keeps priority. */
function uniqueTerms(terms: string[]): string[] {
  const seen = new Set<string>();
  const out: string[] = [];
  for (const raw of terms) {
    const cleaned = raw
      .toLowerCase()
      .replace(/[":*()^\\\0]/g, '')
      .trim();
    if (!cleaned) continue;
    if (seen.has(cleaned)) continue;
    seen.add(cleaned);
    out.push(cleaned);
  }
  return out;
}

/** Split the bash output on `::WOOM::<NAME>::` sentinel lines into a
 *  name → body map. Sentinel rows are dropped; the body before the
 *  first sentinel is discarded. Tolerates missing sections (returns
 *  the empty string for any that didn't appear in the output). */
function parseSections(raw: string): Record<string, string> {
  const out: Record<string, string> = {};
  const lines = raw.split('\n');
  let current: string | null = null;
  let buf: string[] = [];
  for (const line of lines) {
    const m = line.match(/^::WOOM::([A-Z_]+)::$/);
    if (m) {
      if (current) out[current] = buf.join('\n').trim();
      current = m[1];
      buf = [];
    } else if (current) {
      buf.push(line);
    }
  }
  if (current) out[current] = buf.join('\n').trim();
  return out;
}

/** Conservative shell-quote — single-quote wrap with embedded `'`
 *  escaped via `'\''`. Used only on `cwd` paths which we control;
 *  caller responsibility to not pass attacker-controlled input. */
function shellEscape(s: string): string {
  return `'${s.replace(/'/g, "'\\''")}'`;
}
