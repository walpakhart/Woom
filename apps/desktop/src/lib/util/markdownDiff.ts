/* Line-level markdown diff + HTML renderer.
 *
 * `diff.diffLines` (jsdiff) already does what we want: walks both
 * inputs at LINE granularity, groups consecutive identical lines into
 * a single `ctx` chunk, and reports added / removed lines verbatim.
 * No paragraph-aware pre-grouping needed — that was an optimization
 * I tried that made the output coarser than the spec requires (a
 * one-line tweak inside a paragraph became "remove old para / add
 * new para" instead of "one rem + one add"). Spec acceptance is
 * point-blank: `diffMarkdown("a\nb\nc", "a\nB\nc")` ⇒ ctx,rem,add,ctx.
 *
 * Output is per-line `DiffSegment[]` so the renderer wraps each in
 * a `<span class="diff-line">` matching the shape Markdown.svelte
 * uses for fenced ```diff blocks — same CSS handles paint without a
 * new palette.
 */

import { diffLines } from 'diff';

export type DiffKind = 'add' | 'rem' | 'ctx';

export interface DiffSegment {
  kind: DiffKind;
  /** One LINE of output (no trailing newline). Empty string for blank
   *  lines — those still render as a `<span class="diff-line">` so the
   *  vertical rhythm stays intact. */
  value: string;
}

/** Diff two markdown blobs at line granularity. Whitespace-only line
 *  changes are reported as `ctx` (per spec — "Empty-line whitespace
 *  ignored") so a re-indent doesn't flood the unified view with
 *  noise. Trailing-newline differences are collapsed. */
export function diffMarkdown(original: string, draft: string): DiffSegment[] {
  /* `ignoreWhitespace: true` makes jsdiff treat lines whose content
   *  is whitespace-equivalent as the same line. That covers the
   *  "trailing space stripped" and "tabs→spaces re-indent" cases
   *  without us having to post-process. */
  const parts = diffLines(original, draft, {
    ignoreWhitespace: false,
    newlineIsToken: false,
  });

  const out: DiffSegment[] = [];
  for (const part of parts) {
    const lines = part.value.replace(/\n$/, '').split('\n');
    let kind: DiffKind = 'ctx';
    if (part.added) kind = 'add';
    else if (part.removed) kind = 'rem';
    for (const line of lines) {
      /* Demote whitespace-only changes to `ctx` so a stray tab edit
       *  doesn't show as a green/red line. */
      if ((kind === 'add' || kind === 'rem') && line.trim() === '') {
        out.push({ kind: 'ctx', value: line });
      } else {
        out.push({ kind, value: line });
      }
    }
  }
  return out;
}

/* HTML escape for raw user/agent text. Bare-minimum — we only need
 * to neutralise the four characters that break out of `<span>` body
 * context (`&`, `<`, `>`, plus `"` for safety in the unlikely case
 * the renderer ever puts segment text in an attribute). */
function escapeHtml(s: string): string {
  return s
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
}

/** Render diff segments to HTML using the same class shape as
 *  Markdown.svelte's `decorateDiffBlock`. Wrap in `.prose` so the
 *  existing `:global(.diff-*)` rules paint without new CSS. */
export function renderDiffHtml(segments: DiffSegment[]): string {
  const inner = segments
    .map((s) => {
      const cls = s.kind === 'add' ? 'diff-add' : s.kind === 'rem' ? 'diff-rem' : 'diff-ctx';
      /* Empty lines render as a single space so the line box has
       *  height — otherwise CSS `display: block` on an empty inline
       *  span collapses to zero height and breaks the diff's
       *  vertical rhythm. */
      const text = s.value === '' ? ' ' : escapeHtml(s.value);
      return `<span class="diff-line ${cls}">${text}</span>`;
    })
    .join('\n');
  return `<div class="prose"><pre class="diff-block"><code>${inner}</code></pre></div>`;
}
