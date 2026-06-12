import { gutter, GutterMarker, EditorView } from '@codemirror/view';
import {
  StateField,
  StateEffect,
  RangeSetBuilder,
  RangeSet,
  type Extension,
  type Transaction
} from '@codemirror/state';

export type LineChangeKind = 'add' | 'mod' | 'del';
export type LineChanges = Map<number, LineChangeKind>;

/* Git change indicators rendered in a dedicated thin gutter column (like
   VS Code / Cursor) rather than as a border on the text line — so the stripe
   is crisp, full line-height, and never shifts the code. add = green stripe,
   mod = ochre stripe, del = red triangle on the line above the removed code. */
class ChangeMarker extends GutterMarker {
  constructor(readonly kind: LineChangeKind) {
    super();
  }
  eq(other: ChangeMarker): boolean {
    return other.kind === this.kind;
  }
  toDOM(): HTMLElement {
    const el = document.createElement('div');
    el.className = `cm-changebar-mark cm-changebar-mark--${this.kind}`;
    return el;
  }
}

const MARKERS: Record<LineChangeKind, ChangeMarker> = {
  add: new ChangeMarker('add'),
  mod: new ChangeMarker('mod'),
  del: new ChangeMarker('del')
};

export const setChangeBar = StateEffect.define<LineChanges>();

function buildMarkers(map: LineChanges, doc: EditorView['state']['doc']): RangeSet<GutterMarker> {
  const builder = new RangeSetBuilder<GutterMarker>();
  if (map.size === 0) return builder.finish();
  const total = doc.lines;
  const keys = [...map.keys()].sort((a, b) => a - b);
  for (const ln of keys) {
    if (ln < 1 || ln > total) continue;
    const line = doc.line(ln);
    builder.add(line.from, line.from, MARKERS[map.get(ln)!]);
  }
  return builder.finish();
}

const markerField = StateField.define<RangeSet<GutterMarker>>({
  create: () => RangeSet.empty as RangeSet<GutterMarker>,
  update(value: RangeSet<GutterMarker>, tr: Transaction): RangeSet<GutterMarker> {
    for (const e of tr.effects) {
      if (e.is(setChangeBar)) return buildMarkers(e.value, tr.state.doc);
    }
    if (tr.docChanged) return value.map(tr.changes);
    return value;
  }
});

export interface ChangeBarHandlers {
  /** Mousedown on a change mark. `lineNo` is the 1-based buffer line the
   *  mark sits on. Return true to swallow the event. */
  onMarkClick?: (lineNo: number, event: MouseEvent) => boolean;
}

export function changeBarExtension(handlers: ChangeBarHandlers = {}): Extension {
  return [
    markerField,
    gutter({
      class: 'cm-changebar',
      markers: (view) => view.state.field(markerField),
      domEventHandlers: {
        mousedown(view, line, event) {
          if (!handlers.onMarkClick) return false;
          const ln = view.state.doc.lineAt(line.from).number;
          return handlers.onMarkClick(ln, event as MouseEvent);
        }
      }
    })
  ];
}

/** One contiguous run of -/+ lines from a unified diff. Pure additions
 *  have empty `oldLines`; pure deletions have empty `newLines` and their
 *  gutter mark sits on `markerLine` (the surviving line above). */
export interface ChangeHunk {
  /** 1-based first old-side line of the run. */
  oldStart: number;
  /** 1-based first new-side line of the run (for pure deletions: the
   *  next surviving line, i.e. where the removed block used to start). */
  newStart: number;
  oldLines: string[];
  newLines: string[];
  /** Buffer line carrying the gutter mark for this hunk. */
  markerLine: number;
}

export interface ParsedFileDiff {
  map: LineChanges;
  hunks: ChangeHunk[];
  /** marked buffer line → index into `hunks`. */
  lineHunk: Map<number, number>;
}

/** Parse unified-diff text → per-line markers on the new (right) side
 *  plus the hunks behind them (for the click-to-peek diff popup). */
export function parseUnifiedDiff(diffText: string): ParsedFileDiff {
  const map: LineChanges = new Map();
  const hunks: ChangeHunk[] = [];
  const lineHunk = new Map<number, number>();
  if (!diffText) return { map, hunks, lineHunk };
  const lines = diffText.split('\n');
  let oldLine = 0;
  let newLine = 0;
  let curOld: string[] = [];
  let curNew: string[] = [];
  let runOldStart = 0;
  let runNewStart = 0;
  const flush = () => {
    if (curOld.length === 0 && curNew.length === 0) return;
    const idx = hunks.length;
    if (curNew.length > 0) {
      const kind: LineChangeKind = curOld.length > 0 ? 'mod' : 'add';
      for (let i = 0; i < curNew.length; i++) {
        const ln = runNewStart + i;
        map.set(ln, kind);
        lineHunk.set(ln, idx);
      }
      hunks.push({
        oldStart: runOldStart, newStart: runNewStart,
        oldLines: curOld, newLines: curNew, markerLine: runNewStart
      });
    } else {
      const prev = Math.max(1, runNewStart - 1);
      if (!map.has(prev)) map.set(prev, 'del');
      if (!lineHunk.has(prev)) lineHunk.set(prev, idx);
      hunks.push({
        oldStart: runOldStart, newStart: runNewStart,
        oldLines: curOld, newLines: [], markerLine: prev
      });
    }
    curOld = [];
    curNew = [];
  };
  for (const raw of lines) {
    if (raw.startsWith('@@')) {
      flush();
      const mo = /-([0-9]+)(?:,([0-9]+))?/.exec(raw);
      const mn = /\+([0-9]+)(?:,([0-9]+))?/.exec(raw);
      if (mo) oldLine = parseInt(mo[1], 10);
      if (mn) newLine = parseInt(mn[1], 10);
      continue;
    }
    if (
      raw.startsWith('+++') || raw.startsWith('---') ||
      raw.startsWith('diff ') || raw.startsWith('index ') ||
      raw.startsWith('new file') || raw.startsWith('deleted file') ||
      raw.startsWith('\\') // "\ No newline at end of file"
    ) continue;
    if (raw.startsWith('+')) {
      if (curOld.length === 0 && curNew.length === 0) {
        runOldStart = oldLine;
        runNewStart = newLine;
      }
      curNew.push(raw.slice(1));
      newLine++;
    } else if (raw.startsWith('-')) {
      if (curOld.length === 0 && curNew.length === 0) {
        runOldStart = oldLine;
        runNewStart = newLine;
      }
      curOld.push(raw.slice(1));
      oldLine++;
    } else {
      flush();
      oldLine++;
      newLine++;
    }
  }
  flush();
  return { map, hunks, lineHunk };
}
