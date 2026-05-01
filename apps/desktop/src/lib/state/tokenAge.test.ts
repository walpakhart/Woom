import { describe, expect, it, beforeEach } from 'vitest';
import {
  markTokenInstalled,
  clearTokenInstalled,
  tokenAgeInfo,
  tokenAgeState
} from './tokenAge.svelte';

describe('tokenAgeInfo', () => {
  beforeEach(() => {
    /* Reset module state between tests — same `tokenAgeState` is
     * imported across all of them, so leftovers from one test would
     * pollute the next. */
    tokenAgeState.installedAt.github = null;
    tokenAgeState.installedAt.jira = null;
    tokenAgeState.installedAt.sentry = null;
  });

  it('returns null when no install timestamp recorded', () => {
    expect(tokenAgeInfo('github')).toBeNull();
  });

  it('classifies fresh under 180 days', () => {
    const installed = new Date('2026-01-01T00:00:00Z').toISOString();
    tokenAgeState.installedAt.github = installed;
    const now = new Date('2026-04-01T00:00:00Z').getTime(); // 90 days later
    const info = tokenAgeInfo('github', now);
    expect(info?.severity).toBe('fresh');
    expect(info?.days).toBe(90);
  });

  it('warns at 180 days', () => {
    const installed = new Date('2026-01-01T00:00:00Z').toISOString();
    tokenAgeState.installedAt.github = installed;
    const now = new Date('2026-06-30T00:00:00Z').getTime(); // 180 days later
    const info = tokenAgeInfo('github', now);
    expect(info?.severity).toBe('warn');
  });

  it('strong-warns at 300 days', () => {
    const installed = new Date('2026-01-01T00:00:00Z').toISOString();
    tokenAgeState.installedAt.github = installed;
    const now = new Date('2026-10-28T00:00:00Z').getTime(); // 300 days later
    const info = tokenAgeInfo('github', now);
    expect(info?.severity).toBe('strong-warn');
  });

  it('expires at 365 days', () => {
    const installed = new Date('2026-01-01T00:00:00Z').toISOString();
    tokenAgeState.installedAt.github = installed;
    const now = new Date('2027-01-01T00:00:00Z').getTime(); // 365 days later
    const info = tokenAgeInfo('github', now);
    expect(info?.severity).toBe('expired');
  });

  it('floors days to a whole number', () => {
    const installed = new Date('2026-01-01T00:00:00Z').toISOString();
    tokenAgeState.installedAt.github = installed;
    /* 5.7 days later — `Math.floor` keeps it at 5. Without the floor
     * the UI would round to "6 days" the moment it crossed midnight,
     * which reads as if the token aged a full day overnight. */
    const now =
      new Date('2026-01-01T00:00:00Z').getTime() + 5.7 * 24 * 60 * 60 * 1000;
    expect(tokenAgeInfo('github', now)?.days).toBe(5);
  });

  it('clears install timestamp', () => {
    markTokenInstalled('github');
    expect(tokenAgeInfo('github')).not.toBeNull();
    clearTokenInstalled('github');
    expect(tokenAgeInfo('github')).toBeNull();
  });

  it('markTokenInstalled is idempotent on the same calendar day', () => {
    markTokenInstalled('github');
    const first = tokenAgeState.installedAt.github;
    expect(first).not.toBeNull();
    /* Calling again same-day must not push the timestamp forward —
     * the goal is to nag on token age, not on connect frequency. */
    markTokenInstalled('github');
    expect(tokenAgeState.installedAt.github).toBe(first);
  });

  it('returns null for invalid stored timestamp', () => {
    /* Defensive: if persist somehow lands a corrupt value, we treat
     * it as "no record" rather than a NaN-day banner. */
    tokenAgeState.installedAt.github = 'not-a-date';
    expect(tokenAgeInfo('github')).toBeNull();
  });
});
