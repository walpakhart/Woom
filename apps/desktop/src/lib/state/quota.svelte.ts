// Subscription plan-usage state. Backs the chip that mirrors what
// Claude Code's `/usage` shows: 5-hour rolling, weekly all-models,
// weekly Sonnet-only, weekly Opus-only, weekly Claude Design.
//
// One reactive singleton — every column reads the same numbers.
// `refreshPlanUsage()` is the only mutator; it dedupes concurrent
// in-flight fetches and skips refetching when the cached snapshot
// is fresh (< MIN_REFRESH_MS old). Callers should fire-and-forget.

import { invoke } from '@tauri-apps/api/core';

/** One quota bucket. Shape mirrors `PlanUsageBucket` in claude_quota.rs.
 *  `utilization` is a percentage 0–100 (the OAuth endpoint returns
 *  floats); `resets_at` is an ISO-8601 timestamp string from the API.
 *  Some buckets come back with both fields null (e.g. weekly Sonnet
 *  when never used) — treat null as "no data, don't render". */
export interface PlanUsageBucket {
  utilization: number | null;
  resets_at: string | null;
}

/** Subset of `/api/oauth/usage` we render. Field names match the API
 *  response (snake_case) so serde on the Rust side and the frontend
 *  speak the same wire format. `seven_day_omelette` is the internal
 *  codename for "Claude Design" — Claude Code's UI labels it that way. */
export interface PlanUsage {
  five_hour: PlanUsageBucket | null;
  seven_day: PlanUsageBucket | null;
  seven_day_sonnet: PlanUsageBucket | null;
  seven_day_opus: PlanUsageBucket | null;
  seven_day_omelette: PlanUsageBucket | null;
}

/** Min interval between refresh attempts. The endpoint 429s under
 *  tight polling and the data only changes on a per-turn cadence
 *  anyway, so 60s is a comfortable floor. Callers can call
 *  `refreshPlanUsage()` after every chat send and within this window
 *  it'll just no-op. */
const MIN_REFRESH_MS = 60_000;

export const quotaState = $state<{
  usage: PlanUsage | null;
  /** epoch ms of the last successful fetch. 0 = never fetched. */
  fetchedAt: number;
  /** Last error message, surfaced as a tooltip on the chip. Null on
   *  success; reset on the next successful fetch. Common values:
   *  "log in via `claude login`", "HTTP 429", "network error". */
  error: string | null;
  loading: boolean;
}>({
  usage: null,
  fetchedAt: 0,
  error: null,
  loading: false
});

/** Pull a fresh plan-usage snapshot from the backend. Dedupes
 *  concurrent calls (a second invocation while one is in-flight
 *  returns the existing promise) and skips when the cache is
 *  fresher than `MIN_REFRESH_MS`. Pass `force: true` to bypass the
 *  freshness gate (e.g. user clicked a refresh button). */
let inflight: Promise<void> | null = null;
export function refreshPlanUsage(opts: { force?: boolean } = {}): Promise<void> {
  if (inflight) return inflight;
  if (!opts.force && Date.now() - quotaState.fetchedAt < MIN_REFRESH_MS) {
    return Promise.resolve();
  }
  quotaState.loading = true;
  inflight = (async () => {
    try {
      const usage = await invoke<PlanUsage>('claude_plan_usage');
      quotaState.usage = usage;
      quotaState.fetchedAt = Date.now();
      quotaState.error = null;
    } catch (e) {
      quotaState.error = e instanceof Error ? e.message : String(e);
    } finally {
      quotaState.loading = false;
      inflight = null;
    }
  })();
  return inflight;
}

/** Format a "resets in Xh / Yd" hint for the tooltip. Returns null
 *  when the timestamp is missing or already in the past — we don't
 *  surface "0s ago" oddities. */
export function formatResetsIn(iso: string | null, now: number = Date.now()): string | null {
  if (!iso) return null;
  const t = Date.parse(iso);
  if (!Number.isFinite(t)) return null;
  const diffMs = t - now;
  if (diffMs <= 0) return null;
  const mins = Math.round(diffMs / 60_000);
  if (mins < 60) return `${mins}m`;
  const hours = Math.round(mins / 60);
  if (hours < 48) return `${hours}h`;
  const days = Math.round(hours / 24);
  return `${days}d`;
}
