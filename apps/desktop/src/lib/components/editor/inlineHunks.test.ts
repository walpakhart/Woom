import { describe, it, expect } from 'vitest';
import { Text } from '@codemirror/state';
import { computeHunks, hunkNewRange, hunkAtLine, buildHunkRevert } from './inlineHunks';

/** Apply a DocChange to a Text and return the resulting string. */
function applyChange(doc: Text, change: { from: number; to: number; insert: string }): string {
  return doc.replace(change.from, change.to, Text.of(change.insert.split('\n'))).toString();
}

describe('computeHunks', () => {
  it('returns [] for identical text', () => {
    const t = 'a\nb\nc';
    expect(computeHunks(t, t)).toEqual([]);
  });

  it('detects a pure addition', () => {
    const oldT = 'a\nb\nc';
    const newT = 'a\nb\nX\nc';
    const hunks = computeHunks(oldT, newT);
    expect(hunks).toHaveLength(1);
    expect(hunks[0].added).toEqual(['X']);
    expect(hunks[0].removed).toEqual([]);
    expect(hunks[0].newStart).toBe(3); // X is the 3rd line in new
  });

  it('detects a pure deletion', () => {
    const oldT = 'a\nb\nc';
    const newT = 'a\nc';
    const hunks = computeHunks(oldT, newT);
    expect(hunks).toHaveLength(1);
    expect(hunks[0].removed).toEqual(['b']);
    expect(hunks[0].added).toEqual([]);
    expect(hunks[0].newStart).toBe(2); // anchors where 'b' used to be
  });

  it('detects a modification (del + add in one hunk)', () => {
    const oldT = 'a\nb\nc';
    const newT = 'a\nB\nc';
    const hunks = computeHunks(oldT, newT);
    expect(hunks).toHaveLength(1);
    expect(hunks[0].removed).toEqual(['b']);
    expect(hunks[0].added).toEqual(['B']);
    expect(hunks[0].newStart).toBe(2);
  });

  it('detects multiple non-contiguous regions as separate hunks', () => {
    const oldT = 'a\nb\nc\nd\ne';
    const newT = 'a\nB\nc\nd\nE';
    const hunks = computeHunks(oldT, newT);
    expect(hunks).toHaveLength(2);
    expect(hunks[0].added).toEqual(['B']);
    expect(hunks[1].added).toEqual(['E']);
  });

  it('handles addition at end of file', () => {
    const oldT = 'a\nb';
    const newT = 'a\nb\nc';
    const hunks = computeHunks(oldT, newT);
    expect(hunks).toHaveLength(1);
    expect(hunks[0].added).toEqual(['c']);
    expect(hunks[0].newStart).toBe(3);
  });

  it('handles full replace', () => {
    const hunks = computeHunks('x\ny', 'p\nq');
    expect(hunks).toHaveLength(1);
    expect(hunks[0].removed).toEqual(['x', 'y']);
    expect(hunks[0].added).toEqual(['p', 'q']);
  });
});

describe('hunkNewRange', () => {
  it('spans the added lines', () => {
    const [h] = computeHunks('a\nc', 'a\nX\nY\nc');
    expect(hunkNewRange(h)).toEqual({ fromLine: 2, toLine: 3 });
  });
  it('collapses a pure-delete to its anchor line', () => {
    const [h] = computeHunks('a\nb\nc', 'a\nc');
    expect(hunkNewRange(h)).toEqual({ fromLine: 2, toLine: 2 });
  });
});

describe('hunkAtLine', () => {
  it('finds the hunk covering a line, null otherwise', () => {
    const hunks = computeHunks('a\nb\nc\nd\ne', 'a\nB\nc\nd\nE');
    expect(hunkAtLine(hunks, 2)?.added).toEqual(['B']);
    expect(hunkAtLine(hunks, 5)?.added).toEqual(['E']);
    expect(hunkAtLine(hunks, 3)).toBeNull();
  });
});

describe('buildHunkRevert round-trips old↔new per hunk', () => {
  const cases: Array<[string, string, string]> = [
    ['addition', 'a\nb\nc', 'a\nb\nX\nc'],
    ['deletion', 'a\nb\nc', 'a\nc'],
    ['modification', 'a\nb\nc', 'a\nB\nc'],
    ['addition at end', 'a\nb', 'a\nb\nc'],
    ['full replace', 'x\ny', 'p\nq']
  ];
  for (const [name, oldT, newT] of cases) {
    it(`reverts a ${name} back to old`, () => {
      const hunks = computeHunks(oldT, newT);
      expect(hunks).toHaveLength(1);
      const doc = Text.of(newT.split('\n'));
      expect(applyChange(doc, buildHunkRevert(doc, hunks[0]))).toBe(oldT);
    });
  }
});
