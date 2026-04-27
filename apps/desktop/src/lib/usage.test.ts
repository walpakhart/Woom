// Unit tests for the usage helpers (cost math, token formatting,
// context-window mapping, cache-hit ratios). These are the pieces
// surfaced in the per-message badge + the column-header context ring,
// so getting them wrong shows up as user-visible weirdness ("$NaN",
// "1000 tokens", a context ring that goes red on a fresh chat).

import { describe, expect, it } from 'vitest';

import {
  cacheHitRate,
  contextPct,
  contextWindowFor,
  costForUsage,
  formatCostUsd,
  formatTokens
} from './usage';
import type { ClaudeUsage } from './types';

function usage(partial: Partial<ClaudeUsage>): ClaudeUsage {
  return {
    inputTokens: 0,
    cacheCreationTokens: 0,
    cacheReadTokens: 0,
    outputTokens: 0,
    contextSize: 0,
    model: null,
    ...partial
  };
}

describe('formatTokens', () => {
  it('returns "0" for zero or negative or non-finite', () => {
    expect(formatTokens(0)).toBe('0');
    expect(formatTokens(-5)).toBe('0');
    expect(formatTokens(NaN)).toBe('0');
    expect(formatTokens(Infinity)).toBe('0');
  });

  it('renders 0–999 as raw integer', () => {
    expect(formatTokens(1)).toBe('1');
    expect(formatTokens(42)).toBe('42');
    expect(formatTokens(999)).toBe('999');
  });

  it('renders 1k–99.9k with one decimal', () => {
    expect(formatTokens(1000)).toBe('1k');
    expect(formatTokens(1200)).toBe('1.2k');
    expect(formatTokens(99500)).toBe('99.5k');
  });

  it('renders 100k–999k as integer k', () => {
    expect(formatTokens(100_000)).toBe('100k');
    expect(formatTokens(348_283)).toBe('348k');
  });

  it('renders millions with one decimal', () => {
    expect(formatTokens(1_000_000)).toBe('1M');
    expect(formatTokens(2_700_000)).toBe('2.7M');
    expect(formatTokens(99_900_000)).toBe('99.9M');
  });

  it('renders very large values as integer M', () => {
    expect(formatTokens(348_000_000)).toBe('348M');
  });
});

describe('formatCostUsd', () => {
  it('returns "$0" for zero / negative / non-finite', () => {
    expect(formatCostUsd(0)).toBe('$0');
    expect(formatCostUsd(-1)).toBe('$0');
    expect(formatCostUsd(NaN)).toBe('$0');
  });

  it('uses 4 decimals under one cent', () => {
    expect(formatCostUsd(0.0001)).toBe('$0.0001');
    expect(formatCostUsd(0.0099)).toBe('$0.0099');
  });

  it('uses 3 decimals under a dollar', () => {
    expect(formatCostUsd(0.05)).toBe('$0.050');
    expect(formatCostUsd(0.999)).toBe('$0.999');
  });

  it('uses 2 decimals from a dollar up', () => {
    expect(formatCostUsd(1)).toBe('$1.00');
    expect(formatCostUsd(2.4)).toBe('$2.40');
    expect(formatCostUsd(123.456)).toBe('$123.46');
  });
});

describe('contextWindowFor', () => {
  it('defaults to 200k for null / unknown models', () => {
    expect(contextWindowFor(null)).toBe(200_000);
    expect(contextWindowFor('claude-sonnet-4-6')).toBe(200_000);
    expect(contextWindowFor('claude-haiku-4-5-20251001')).toBe(200_000);
    expect(contextWindowFor('some-unknown-model-id')).toBe(200_000);
  });

  it('returns 1M for any opus-4-7 variant', () => {
    expect(contextWindowFor('claude-opus-4-7')).toBe(1_000_000);
    // Future-proofing: a longer suffix on opus-4-7 (e.g. a thinking
    // variant id) should still get the 1M window.
    expect(contextWindowFor('claude-opus-4-7-some-suffix')).toBe(1_000_000);
  });
});

describe('contextPct', () => {
  it('returns 0 when context size is 0', () => {
    expect(contextPct(usage({ contextSize: 0, model: 'claude-sonnet-4-6' }))).toBe(0);
  });

  it('reports the correct ratio for a half-full Sonnet window', () => {
    expect(contextPct(usage({ contextSize: 100_000, model: 'claude-sonnet-4-6' }))).toBeCloseTo(0.5);
  });

  it('clamps over-budget contexts to 1', () => {
    // 250k against Sonnet's 200k window — hard cap rather than 1.25.
    expect(contextPct(usage({ contextSize: 250_000, model: 'claude-sonnet-4-6' }))).toBe(1);
  });

  it('uses the 1M window for Opus when computing the ratio', () => {
    expect(contextPct(usage({ contextSize: 200_000, model: 'claude-opus-4-7' }))).toBeCloseTo(0.2);
  });
});

describe('cacheHitRate', () => {
  it('returns null when there is no input at all', () => {
    expect(cacheHitRate(usage({}))).toBeNull();
  });

  it('matches cache_read / total_input', () => {
    const u = usage({
      inputTokens: 100,
      cacheCreationTokens: 0,
      cacheReadTokens: 900
    });
    expect(cacheHitRate(u)).toBeCloseTo(0.9);
  });

  it('counts cache creation against the denominator (it is real input)', () => {
    const u = usage({
      inputTokens: 50,
      cacheCreationTokens: 50,
      cacheReadTokens: 0
    });
    // 0 cache_read / 100 total = 0% hit
    expect(cacheHitRate(u)).toBeCloseTo(0);
  });
});

describe('costForUsage', () => {
  it('returns 0 when every token bucket is 0', () => {
    expect(costForUsage(usage({}))).toBe(0);
  });

  it('uses Sonnet rates for unknown / null models (lower bound, no scary numbers)', () => {
    // 1M input @ $3 + 1M output @ $15 = $18
    const u = usage({ inputTokens: 1_000_000, outputTokens: 1_000_000, model: null });
    expect(costForUsage(u)).toBeCloseTo(18);
  });

  it('charges Opus rates correctly', () => {
    // 1M input @ $15 + 1M output @ $75 = $90
    const u = usage({
      inputTokens: 1_000_000,
      outputTokens: 1_000_000,
      model: 'claude-opus-4-7'
    });
    expect(costForUsage(u)).toBeCloseTo(90);
  });

  it('discounts cache_read 10x vs fresh input on Sonnet', () => {
    // 1M cache_read @ $0.30 = $0.30 (vs $3 fresh)
    const u = usage({ cacheReadTokens: 1_000_000, model: 'claude-sonnet-4-6' });
    expect(costForUsage(u)).toBeCloseTo(0.3);
  });

  it('charges cache_creation at the 1.25x premium on Sonnet', () => {
    // 1M cache_creation @ $3.75 = $3.75
    const u = usage({ cacheCreationTokens: 1_000_000, model: 'claude-sonnet-4-6' });
    expect(costForUsage(u)).toBeCloseTo(3.75);
  });

  it('charges Haiku at the 5x cheaper rate vs Sonnet for output', () => {
    // 1M output @ $4 vs Sonnet's $15
    const u = usage({ outputTokens: 1_000_000, model: 'claude-haiku-4-5-20251001' });
    expect(costForUsage(u)).toBeCloseTo(4);
  });
});
