// Unit tests for the quota-state pure helpers (`nextResetAt`,
// `formatResumeIn`). These back the Phase-2 watchdog + ResumePill +
// QuotaPauseModal countdown — getting them wrong shows up as wrong
// time-to-resume strings (a "0s" pill that won't fire, or a 12-hour
// countdown on a fresh reset).

import { describe, expect, it } from 'vitest';

import { formatResumeIn, nextResetAt } from './quota.svelte';
import type { PlanUsage } from './quota.svelte';

function bucket(iso: string | null) {
  return iso === null ? null : { utilization: 50, resets_at: iso };
}

function usage(overrides: Partial<PlanUsage> = {}): PlanUsage {
  return {
    five_hour: null,
    seven_day: null,
    seven_day_sonnet: null,
    seven_day_opus: null,
    seven_day_omelette: null,
    ...overrides
  };
}

describe('formatResumeIn', () => {
  it('returns "0s" for zero / negative / non-finite', () => {
    expect(formatResumeIn(0)).toBe('0s');
    expect(formatResumeIn(-1000)).toBe('0s');
    expect(formatResumeIn(NaN)).toBe('0s');
    expect(formatResumeIn(Infinity)).toBe('0s');
  });

  it('renders sub-minute durations as raw seconds', () => {
    expect(formatResumeIn(1000)).toBe('1s');
    expect(formatResumeIn(59_000)).toBe('59s');
  });

  it('renders sub-hour as "Xm YYs"', () => {
    // 12m 34s
    expect(formatResumeIn(12 * 60_000 + 34_000)).toBe('12m 34s');
    // Leading-zero seconds — the second pad keeps the layout stable.
    expect(formatResumeIn(5 * 60_000 + 3_000)).toBe('5m 03s');
  });

  it('renders hours-plus as "Xh YYm"', () => {
    // 3h 02m
    expect(formatResumeIn(3 * 3_600_000 + 2 * 60_000)).toBe('3h 02m');
    // 1h exactly
    expect(formatResumeIn(3_600_000)).toBe('1h 00m');
  });
});

describe('nextResetAt', () => {
  const NOW = 1_700_000_000_000; // fixed test clock — Nov 2023

  it('returns null for null usage', () => {
    expect(nextResetAt(null, NOW)).toBeNull();
  });

  it('returns null when both buckets are null', () => {
    expect(nextResetAt(usage(), NOW)).toBeNull();
  });

  it('returns null when both buckets are in the past', () => {
    const past = new Date(NOW - 60_000).toISOString();
    expect(
      nextResetAt(usage({ five_hour: bucket(past), seven_day: bucket(past) }), NOW)
    ).toBeNull();
  });

  it('uses the only non-null bucket', () => {
    const future = new Date(NOW + 5 * 60_000).toISOString();
    expect(
      nextResetAt(usage({ five_hour: bucket(future) }), NOW)
    ).toBe(Date.parse(future));
  });

  it('returns the earlier of two future resets', () => {
    const sooner = new Date(NOW + 5 * 60_000).toISOString();
    const later = new Date(NOW + 60 * 60_000).toISOString();
    expect(
      nextResetAt(usage({
        five_hour: bucket(sooner),
        seven_day: bucket(later)
      }), NOW)
    ).toBe(Date.parse(sooner));
  });

  it('skips past timestamps and uses the future-side bucket', () => {
    const past = new Date(NOW - 60_000).toISOString();
    const future = new Date(NOW + 10 * 60_000).toISOString();
    expect(
      nextResetAt(usage({
        five_hour: bucket(past),
        seven_day: bucket(future)
      }), NOW)
    ).toBe(Date.parse(future));
  });
});
