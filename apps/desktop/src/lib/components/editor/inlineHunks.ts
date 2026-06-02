import { EditorView, Decoration, WidgetType, type DecorationSet } from '@codemirror/view';
import {
  StateField,
  StateEffect,
  type Extension,
  type Transaction,
  type Text
} from '@codemirror/state';

/* Inline agentic-edit diff overlay. Sibling to `changeBar.ts` but renders
   IN the text (line backgrounds + ghost block widgets) rather than in a
   gutter, so a reviewer sees exactly what the agent added/removed without
   leaving the buffer. Hand-rolled line diff — no `@codemirror/merge`, same
   deliberate choice DiffView.svelte documents. */

export interface Hunk {
  id: string;
  /** 1-based first changed line on the OLD side. */
  oldStart: number;
  oldCount: number;
  /** 1-based line on the NEW side where the change anchors. Added lines
   *  occupy `newStart .. newStart + added.length - 1`; a pure-delete hunk
   *  anchors its ghost widget at `newStart` (the line that now sits where
   *  the removed lines used to be). */
  newStart: number;
  newCount: number;
  added: string[];
  removed: string[];
}

function splitLines(text: string): string[] {
  return text.split('\n');
}

/** Line-level LCS diff of two full-file strings, grouped into contiguous
 *  changed regions (hunks). Identical input → `[]`. */
export function computeHunks(oldText: string, newText: string): Hunk[] {
  const a = splitLines(oldText);
  const b = splitLines(newText);
  const n = a.length;
  const m = b.length;

  // dp[i][j] = LCS length of a[i:] and b[j:]. O(n*m) — fine for source files.
  const dp: Int32Array[] = Array.from({ length: n + 1 }, () => new Int32Array(m + 1));
  for (let i = n - 1; i >= 0; i--) {
    const row = dp[i];
    const next = dp[i + 1];
    for (let j = m - 1; j >= 0; j--) {
      row[j] = a[i] === b[j] ? next[j + 1] + 1 : Math.max(next[j], row[j + 1]);
    }
  }

  const hunks: Hunk[] = [];
  let cur: Hunk | null = null;
  let seq = 0;
  let i = 0;
  let j = 0;

  const open = () => {
    if (!cur) {
      cur = {
        id: `h${seq++}_${i + 1}_${j + 1}`,
        oldStart: i + 1,
        oldCount: 0,
        newStart: j + 1,
        newCount: 0,
        added: [],
        removed: []
      };
    }
  };
  const close = () => {
    if (cur) {
      hunks.push(cur);
      cur = null;
    }
  };

  while (i < n && j < m) {
    if (a[i] === b[j]) {
      close();
      i++;
      j++;
    } else if (dp[i + 1][j] >= dp[i][j + 1]) {
      open();
      cur!.removed.push(a[i]);
      cur!.oldCount++;
      i++;
    } else {
      open();
      cur!.added.push(b[j]);
      cur!.newCount++;
      j++;
    }
  }
  while (i < n) {
    open();
    cur!.removed.push(a[i]);
    cur!.oldCount++;
    i++;
  }
  while (j < m) {
    open();
    cur!.added.push(b[j]);
    cur!.newCount++;
    j++;
  }
  close();

  return hunks;
}

/* ── Hunk geometry + revert (P3) ─────────────────────────────────────── */

/** New-side line range a hunk occupies (1-based, inclusive). A hunk with
 *  added lines spans them; a pure-delete hunk collapses to its anchor line
 *  (the line that now sits where the removed lines were). */
export function hunkNewRange(h: Hunk): { fromLine: number; toLine: number } {
  if (h.added.length > 0) {
    return { fromLine: h.newStart, toLine: h.newStart + h.added.length - 1 };
  }
  return { fromLine: h.newStart, toLine: h.newStart };
}

/** First hunk whose new-side range covers `line` (1-based), or null. Used
 *  by the Tab/Esc keymap to find the hunk under the caret. */
export function hunkAtLine(hunks: Hunk[], line: number): Hunk | null {
  for (const h of hunks) {
    const r = hunkNewRange(h);
    if (line >= r.fromLine && line <= r.toLine) return h;
  }
  return null;
}

export interface DocChange {
  from: number;
  to: number;
  insert: string;
}

/** Build a whole-document replacement that reverts exactly one hunk:
 *  splice the hunk's added lines out and its removed lines back in, at the
 *  hunk's new-side position. Whole-doc replace (vs a tight range edit)
 *  sidesteps trailing-newline bookkeeping — correctness over minimal diff.
 *  NOTE: line numbers are relative to the CURRENT doc, so rejecting one
 *  hunk of a multi-hunk edit shifts the others (known MVP drift). */
export function buildHunkRevert(doc: Text, h: Hunk): DocChange {
  const lines = doc.toString().split('\n');
  const start = Math.max(0, h.newStart - 1); // 0-based index of first added line
  lines.splice(start, h.added.length, ...h.removed);
  return { from: 0, to: doc.length, insert: lines.join('\n') };
}

/* ── CodeMirror decoration layer ─────────────────────────────────────── */

class GhostLinesWidget extends WidgetType {
  constructor(readonly lines: string[], readonly hunkId: string) {
    super();
  }
  eq(other: GhostLinesWidget): boolean {
    return other.hunkId === this.hunkId && other.lines.length === this.lines.length;
  }
  toDOM(): HTMLElement {
    const wrap = document.createElement('div');
    wrap.className = 'cm-inline-hunk--del';
    wrap.setAttribute('data-hunk', this.hunkId);
    for (const ln of this.lines) {
      const row = document.createElement('div');
      row.className = 'cm-inline-hunk--del-line';
      // Render empty removed lines as a non-breaking space so the row
      // still has height and reads as "a line was here".
      row.textContent = ln.length ? ln : ' ';
      wrap.appendChild(row);
    }
    return wrap;
  }
  ignoreEvent(): boolean {
    return false;
  }
}

const addLineDeco = (id: string) =>
  Decoration.line({ class: 'cm-inline-hunk--add', attributes: { 'data-hunk': id } });

function buildDecorations(hunks: Hunk[], doc: Text): DecorationSet {
  if (hunks.length === 0) return Decoration.none;
  const ranges = [];
  const totalLines = doc.lines;

  for (const h of hunks) {
    // Added/modified lines — real lines in the current (post-edit) doc.
    for (let k = 0; k < h.added.length; k++) {
      const lineNo = h.newStart + k;
      if (lineNo < 1 || lineNo > totalLines) continue;
      const line = doc.line(lineNo);
      ranges.push(addLineDeco(h.id).range(line.from));
    }
    // Removed lines — gone from disk, shown as a ghost block above the
    // anchor line. Clamp; deletion at EOF anchors after the last line.
    if (h.removed.length > 0) {
      const widget = new GhostLinesWidget(h.removed, h.id);
      if (h.newStart > totalLines) {
        const last = doc.line(totalLines);
        ranges.push(
          Decoration.widget({ widget, block: true, side: 1 }).range(last.to)
        );
      } else {
        const anchor = doc.line(Math.max(1, h.newStart));
        ranges.push(
          Decoration.widget({ widget, block: true, side: -1 }).range(anchor.from)
        );
      }
    }
  }

  // `true` → let CodeMirror sort by (from, startSide); mixes line decos and
  // block widgets at the same position safely.
  return Decoration.set(ranges, true);
}

export const setHunks = StateEffect.define<Hunk[]>();

const pendingHunksField = StateField.define<DecorationSet>({
  create: () => Decoration.none,
  update(value: DecorationSet, tr: Transaction): DecorationSet {
    for (const e of tr.effects) {
      if (e.is(setHunks)) return buildDecorations(e.value, tr.state.doc);
    }
    if (tr.docChanged) return value.map(tr.changes);
    return value;
  },
  provide: (f) => EditorView.decorations.from(f)
});

export function inlineHunksExtension(): Extension {
  return [pendingHunksField];
}
