import { describe, expect, it } from 'vitest';
import { fuzzyScore, fuzzyScoreAny } from './fuzzyMatch';

describe('fuzzyScore', () => {
  it('returns 0 for empty query (matches everything)', () => {
    expect(fuzzyScore('', 'anything')).toBe(0);
  });

  it('returns null when query is not a subsequence', () => {
    expect(fuzzyScore('xyz', 'GitHub PR review')).toBeNull();
    expect(fuzzyScore('zzz', 'Sentry')).toBeNull();
  });

  it('matches a clean subsequence', () => {
    /* "ghp" → "GitHub PR" — case-insensitive subseq */
    expect(fuzzyScore('ghp', 'GitHub PR')).toBeGreaterThan(0);
  });

  it('rewards consecutive characters more than scattered ones', () => {
    const consec = fuzzyScore('open', 'open settings')!;
    const scatter = fuzzyScore('open', 'opal pen')!;
    expect(consec).toBeGreaterThan(scatter);
  });

  it('rewards word-boundary matches', () => {
    /* "set" should score higher in "Open Settings" (start of word)
     * than in "Reset" (mid-word). */
    const boundary = fuzzyScore('set', 'Open Settings')!;
    const midWord = fuzzyScore('set', 'Reset')!;
    expect(boundary).toBeGreaterThan(midWord);
  });

  it('rewards exact case match', () => {
    const exact = fuzzyScore('PR', 'New PR')!;
    const cased = fuzzyScore('pr', 'New PR')!;
    expect(exact).toBeGreaterThan(cased);
  });

  it('detects camelCase boundaries', () => {
    expect(fuzzyScore('jdc', 'JsDocComment')).not.toBeNull();
  });

  it('handles common command-palette typos like "githb"', () => {
    expect(fuzzyScore('githb', 'GitHub')).toBeGreaterThan(0);
    expect(fuzzyScore('githb', 'GitHub PR review')).toBeGreaterThan(0);
  });
});

describe('fuzzyScoreAny', () => {
  it('returns the best score across multiple fields', () => {
    const score = fuzzyScoreAny('settings', ['Settings', 'app config']);
    /* Direct match on the first field should win. */
    expect(score).toBeGreaterThan(20);
  });

  it('returns null only when no field matches', () => {
    expect(fuzzyScoreAny('xyz', ['Open', 'Settings'])).toBeNull();
  });

  it('skips null/undefined fields', () => {
    expect(fuzzyScoreAny('set', ['Settings', null, undefined])).toBeGreaterThan(0);
  });
});
