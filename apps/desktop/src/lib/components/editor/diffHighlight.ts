// Standalone syntax-highlight helper for the (CodeMirror-less) DiffView.
//
// We can't reuse `defaultHighlightStyle` from `@codemirror/language`
// because its CSS classes are obfuscated (`ͼo`, `ͼp`, …) and bound to
// an EditorView's injected stylesheet — pulling them out into our
// grid would require shipping an off-screen CM view per side. Instead
// we walk the syntax tree with `classHighlighter` from
// `@lezer/highlight`, which emits stable `tok-*` class names. DiffView
// styles them with its own per-token palette (defined in app.css).
//
// `ensureSyntaxTree(state, len, 200ms)` force-parses synchronously up
// to `len` so we get a complete tree even for lang-javascript (whose
// parser is normally incremental and would return a stub on the first
// tick). Falls back to `syntaxTree` if the deadline is missed.

import { EditorState, type Extension } from '@codemirror/state';
import { ensureSyntaxTree, syntaxTree } from '@codemirror/language';
import { highlightTree, classHighlighter } from '@lezer/highlight';

/** A single syntax token in document-absolute coordinates. */
export type SynTok = { from: number; to: number; cls: string };

/** Per-line syntax tokens, with positions translated to be relative
 *  to the start of the line (0..line.length). Lines are indexed
 *  1-based to match the rest of the diff pipeline. */
export type LineTokens = Map<number, SynTok[]>;

/** Tokenize `text` against `lang` and return tokens grouped by line.
 *  Line numbers are 1-based; missing lines (no tokens) are simply
 *  absent from the map and should be rendered as plain text. */
export function tokenizeByLine(text: string, lang: Extension): LineTokens {
  const byLine: LineTokens = new Map();
  if (!text) return byLine;

  let tree;
  try {
    const state = EditorState.create({ doc: text, extensions: [lang] });
    tree = ensureSyntaxTree(state, text.length, 200) ?? syntaxTree(state);
  } catch {
    return byLine; // unknown language / parser error → plain text
  }
  if (!tree) return byLine;

  /* Walk the source ONCE to build absolute-pos → line/col lookup so
   * we can split tokens that span newlines into per-line pieces.
   * `lineStart[k]` is the absolute offset of the start of line k+1
   * (i.e. 0-based array of 1-based line starts). */
  const lineStart: number[] = [0];
  for (let i = 0; i < text.length; i++) {
    if (text.charCodeAt(i) === 10 /* \n */) lineStart.push(i + 1);
  }
  const totalLines = lineStart.length;

  const lineOf = (pos: number): number => {
    /* Binary search — tokens are dispatched in source order, but the
     * cost is amortized ~O(log L) per token which is negligible. */
    let lo = 0, hi = totalLines - 1;
    while (lo < hi) {
      const mid = (lo + hi + 1) >> 1;
      if (lineStart[mid] <= pos) lo = mid;
      else hi = mid - 1;
    }
    return lo + 1; // 1-based
  };

  highlightTree(tree, classHighlighter, (from, to, classes) => {
    if (from >= to) return;
    let pos = from;
    while (pos < to) {
      const ln = lineOf(pos);
      const lineStartPos = lineStart[ln - 1];
      const lineEndPos = ln < totalLines ? lineStart[ln] - 1 : text.length;
      const segEnd = Math.min(to, lineEndPos);
      const localFrom = pos - lineStartPos;
      const localTo = segEnd - lineStartPos;
      if (localTo > localFrom) {
        let arr = byLine.get(ln);
        if (!arr) { arr = []; byLine.set(ln, arr); }
        arr.push({ from: localFrom, to: localTo, cls: classes });
      }
      pos = lineEndPos + 1;
    }
  });

  return byLine;
}

/** Merge per-line syntax tokens with optional diff-word parts into a
 *  flat segment list where each segment has a uniform (syntaxCls,
 *  diffHl) pair. Used both for plain rows (`parts = null`) and for
 *  paired change rows (where word-diff highlights must layer on top
 *  of syntax color). The output is consumed directly by the Svelte
 *  template via `{#each segments as seg}`. */
export type DiffPart = { text: string; hl?: 'add' | 'rem' };
export type Segment = { text: string; cls: string | null; hl: 'add' | 'rem' | null };

export function buildSegments(
  line: string,
  tokens: SynTok[] | undefined,
  parts: DiffPart[] | null
): Segment[] {
  const len = line.length;
  if (len === 0) return [];
  /* Char-aligned label arrays — one slot per char of the line. Sparse
   * via `null` so we can detect "no class here / no diff highlight
   * here" without a sentinel. */
  const sxArr: (string | null)[] = new Array(len).fill(null);
  if (tokens) {
    for (const t of tokens) {
      const a = Math.max(0, t.from), b = Math.min(len, t.to);
      for (let i = a; i < b; i++) sxArr[i] = t.cls;
    }
  }
  const dxArr: ('add' | 'rem' | null)[] = new Array(len).fill(null);
  if (parts) {
    let p = 0;
    for (const part of parts) {
      const end = p + part.text.length;
      if (part.hl) for (let i = p; i < end && i < len; i++) dxArr[i] = part.hl;
      p = end;
    }
  }
  const out: Segment[] = [];
  let i = 0;
  while (i < len) {
    const cls = sxArr[i];
    const hl = dxArr[i];
    let j = i + 1;
    while (j < len && sxArr[j] === cls && dxArr[j] === hl) j++;
    out.push({ text: line.slice(i, j), cls, hl });
    i = j;
  }
  return out;
}
