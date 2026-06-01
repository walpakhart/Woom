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

export function changeBarExtension(): Extension {
  return [
    markerField,
    gutter({
      class: 'cm-changebar',
      markers: (view) => view.state.field(markerField)
    })
  ];
}

/** Parse unified-diff text → per-line markers on new (right) side. */
export function parseUnifiedDiffToLineChanges(diffText: string): LineChanges {
  const out: LineChanges = new Map();
  if (!diffText) return out;
  const lines = diffText.split('\n');
  let newLine = 0;
  let addsInHunk: number[] = [];
  let delsInHunk = 0;
  const flushHunk = () => {
    if (delsInHunk > 0 && addsInHunk.length > 0) {
      for (const ln of addsInHunk) out.set(ln, 'mod');
    }
    addsInHunk = [];
    delsInHunk = 0;
  };
  for (const raw of lines) {
    if (raw.startsWith('@@')) {
      flushHunk();
      const m = /\+([0-9]+)(?:,([0-9]+))?/.exec(raw);
      if (m) newLine = parseInt(m[1], 10);
      continue;
    }
    if (
      raw.startsWith('+++') || raw.startsWith('---') ||
      raw.startsWith('diff ') || raw.startsWith('index ') ||
      raw.startsWith('new file') || raw.startsWith('deleted file')
    ) continue;
    if (raw.startsWith('+')) {
      if (!out.has(newLine)) out.set(newLine, 'add');
      addsInHunk.push(newLine);
      newLine++;
    } else if (raw.startsWith('-')) {
      delsInHunk++;
      const prev = newLine - 1;
      if (prev >= 1 && !out.has(prev)) out.set(prev, 'del');
    } else {
      flushHunk();
      newLine++;
    }
  }
  flushHunk();
  return out;
}
