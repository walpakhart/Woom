import { describe, it, expect } from 'vitest';
import { editStats, firstChangedLine } from './reviewStats';

describe('editStats', () => {
  it('identical text → no changes', () => {
    const t = 'a\nb\nc';
    expect(editStats(t, t)).toEqual({ add: 0, rem: 0 });
  });

  it('pure addition counts added lines only', () => {
    const oldT = 'a\nb\nc';
    const newT = 'a\nx\ny\nb\nc';
    expect(editStats(oldT, newT)).toEqual({ add: 2, rem: 0 });
  });

  it('pure deletion counts removed lines only', () => {
    const oldT = 'a\nb\nc\nd';
    const newT = 'a\nd';
    expect(editStats(oldT, newT)).toEqual({ add: 0, rem: 2 });
  });

  it('replacement counts both sides', () => {
    const oldT = 'a\nb\nc';
    const newT = 'a\nB\nc';
    expect(editStats(oldT, newT)).toEqual({ add: 1, rem: 1 });
  });

  it('multi-hunk sums across hunks', () => {
    const oldT = 'a\nb\nc\nd\ne';
    const newT = 'a\nB\nc\nD\ne';
    expect(editStats(oldT, newT)).toEqual({ add: 2, rem: 2 });
  });

  it('empty / undefined inputs do not throw', () => {
    expect(editStats('', '')).toEqual({ add: 0, rem: 0 });
    expect(editStats(undefined as unknown as string, undefined as unknown as string)).toEqual({
      add: 0,
      rem: 0
    });
  });
});

describe('firstChangedLine', () => {
  it('identical text → null', () => {
    const t = 'a\nb\nc';
    expect(firstChangedLine(t, t)).toBeNull();
  });

  it('addition → new-side line of first inserted line', () => {
    const oldT = 'a\nb\nc';
    const newT = 'a\nx\ny\nb\nc';
    expect(firstChangedLine(oldT, newT)).toBe(2);
  });

  it('deletion → anchor line on new side', () => {
    const oldT = 'a\nb\nc\nd';
    const newT = 'a\nd';
    expect(firstChangedLine(oldT, newT)).toBe(2);
  });

  it('replacement → line of the change', () => {
    const oldT = 'a\nb\nc';
    const newT = 'a\nB\nc';
    expect(firstChangedLine(oldT, newT)).toBe(2);
  });

  it('multi-hunk → first hunk wins', () => {
    const oldT = 'a\nb\nc\nd\ne';
    const newT = 'a\nB\nc\nD\ne';
    expect(firstChangedLine(oldT, newT)).toBe(2);
  });

  it('empty / undefined inputs → null', () => {
    expect(firstChangedLine('', '')).toBeNull();
    expect(firstChangedLine(undefined as unknown as string, undefined as unknown as string)).toBeNull();
  });
});
