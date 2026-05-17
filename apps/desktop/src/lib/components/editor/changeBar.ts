import { Decoration, type DecorationSet, EditorView } from '@codemirror/view';
import {
  StateField,
  StateEffect,
  RangeSetBuilder,
  type Extension,
  type Transaction
} from '@codemirror/state';

export type LineChangeKind = 'add' | 'mod' | 'del';
export type LineChanges = Map<number, LineChangeKind>;

const ADD_DECO = Decoration.line({ attributes: { class: 'cm-line-changebar cm-line-changebar--add' } });
const MOD_DECO = Decoration.line({ attributes: { class: 'cm-line-changebar cm-line-changebar--mod' } });
const DEL_DECO = Decoration.line({ attributes: { class: 'cm-line-changebar cm-line-changebar--del' } });

export const setChangeBar = StateEffect.define<LineChanges>();

function buildDecorations(map: LineChanges, doc: EditorView['state']['doc']): DecorationSet {
  const builder = new RangeSetBuilder<Decoration>();
  if (map.size === 0) return builder.finish();
  const total = doc.lines;
  const keys = [...map.keys()].sort((a, b) => a - b);
  for (const ln of keys) {
    if (ln < 1 || ln > total) continue;
    const kind = map.get(ln)!;
    const line = doc.line(ln);
    const deco = kind === 'add' ? ADD_DECO : kind === 'mod' ? MOD_DECO : DEL_DECO;
    builder.add(line.from, line.from, deco);
  }
  return builder.finish();
}

const changeBarField = StateField.define<DecorationSet>({
  create: () => Decoration.none,
  update(value: DecorationSet, tr: Transaction): DecorationSet {
    for (const e of tr.effects) {
      if (e.is(setChangeBar)) return buildDecorations(e.value, tr.state.doc);
    }
    if (tr.docChanged) return value.map(tr.changes);
    return value;
  },
  provide: (f) => EditorView.decorations.from(f)
});

export function changeBarExtension(): Extension {
  return [changeBarField];
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
