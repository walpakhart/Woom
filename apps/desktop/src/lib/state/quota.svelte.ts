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

/** Default min interval between refresh attempts. The endpoint 429s
 *  under tight polling and the data only changes on a per-turn cadence
 *  anyway, so 60s is a comfortable floor. Callers can call
 *  `refreshPlanUsage()` after every chat send and within this window
 *  it'll just no-op. Exponential-backoff on 429 lifts this dynamically;
 *  see `quotaState.nextBackoffMs`. */
const DEFAULT_REFRESH_MS = 60_000;
/** Hard ceiling for the backoff ladder — 15 minutes. Past this we'd
 *  rather refresh once and accept another 429 than completely fall
 *  off the live-usage signal. */
const MAX_BACKOFF_MS = 15 * 60_000;

export const quotaState = $state<{
  usage: PlanUsage | null;
  /** epoch ms of the last successful fetch. 0 = never fetched. */
  fetchedAt: number;
  /** Last error message, surfaced as a tooltip on the chip. Null on
   *  success; reset on the next successful fetch. Common values:
   *  "log in via `claude login`", "HTTP 429", "network error". */
  error: string | null;
  loading: boolean;
  /** Live floor for the refresh-min-interval gate. Starts at
   *  `DEFAULT_REFRESH_MS` (60s); each 429 multiplies by 5 up to
   *  `MAX_BACKOFF_MS`; a successful fetch resets to default. The
   *  Phase-2 watchdog hits `refreshPlanUsage()` every 30s — without
   *  backoff a sustained 429 storm would never let the watchdog
   *  recover the live signal. */
  nextBackoffMs: number;
}>({
  usage: null,
  fetchedAt: 0,
  error: null,
  loading: false,
  nextBackoffMs: DEFAULT_REFRESH_MS
});

/** Pull a fresh plan-usage snapshot from the backend. Dedupes
 *  concurrent calls (a second invocation while one is in-flight
 *  returns the existing promise) and skips when the cache is
 *  fresher than `MIN_REFRESH_MS`. Pass `force: true` to bypass the
 *  freshness gate (e.g. user clicked a refresh button). */
let inflight: Promise<void> | null = null;
export function refreshPlanUsage(opts: { force?: boolean } = {}): Promise<void> {
  if (inflight) return inflight;
  /* `force: true` bypasses the freshness gate (click-to-refresh) but
   * still respects the backoff ladder — when the endpoint is clearly
   * rate-limiting, hammering it harder on user click doesn't help.
   * Per phase 2 plan + phase 3 force-refresh contract. */
  if (Date.now() - quotaState.fetchedAt < quotaState.nextBackoffMs) {
    if (!opts.force) return Promise.resolve();
    /* Force path: still gate, but only on backoff (not on default
     * freshness floor). If backoff is at default 60s, force always
     * passes; if 429 elevated it to 5min, force waits the 5min. */
    if (quotaState.nextBackoffMs > DEFAULT_REFRESH_MS) {
      return Promise.resolve();
    }
  }
  quotaState.loading = true;
  inflight = (async () => {
    try {
      const usage = await invoke<PlanUsage>('claude_plan_usage');
      quotaState.usage = usage;
      quotaState.fetchedAt = Date.now();
      quotaState.error = null;
      quotaState.nextBackoffMs = DEFAULT_REFRESH_MS;
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      quotaState.error = msg;
      /* Exponential backoff on 429-class errors. The Tauri command
       * surfaces upstream errors verbatim so we match on substring;
       * stable across HTTP message variants. */
      if (msg.includes('429') || /too many requests/i.test(msg)) {
        quotaState.nextBackoffMs = Math.min(
          MAX_BACKOFF_MS,
          quotaState.nextBackoffMs * 5
        );
      }
    } finally {
      quotaState.loading = false;
      inflight = null;
    }
  })();
  return inflight;
}

/** Earliest reset time across the 5H + 7D buckets, as unix-ms. Null
 *  when both buckets are absent or unparseable. Treats past timestamps
 *  as missing — they're stale data, not "reset just happened".
 *
 *  Used by the in-flight watchdog (`+page.svelte`) and pre-send guard
 *  (`sendClaudeMessage.ts`) to compute `sess.resumeAt`. */
export function nextResetAt(usage: PlanUsage | null, now: number = Date.now()): number | null {
  if (!usage) return null;
  const parsed: number[] = [];
  for (const bucket of [usage.five_hour, usage.seven_day]) {
    if (!bucket?.resets_at) continue;
    const t = Date.parse(bucket.resets_at);
    if (Number.isFinite(t) && t > now) parsed.push(t);
  }
  if (parsed.length === 0) return null;
  return Math.min(...parsed);
}

/** Countdown formatter — "12m 34s" / "3h 02m" / "0s". Used by the
 *  Resume pill and the quota-pause modal. Negative / 0 ms → "0s"
 *  (the caller should treat that as "reset already passed"). */
export function formatResumeIn(ms: number): string {
  if (!Number.isFinite(ms) || ms <= 0) return '0s';
  const totalSec = Math.floor(ms / 1000);
  if (totalSec < 60) return `${totalSec}s`;
  if (totalSec < 3600) {
    const m = Math.floor(totalSec / 60);
    const s = totalSec % 60;
    return `${m}m ${String(s).padStart(2, '0')}s`;
  }
  const h = Math.floor(totalSec / 3600);
  const m = Math.floor((totalSec % 3600) / 60);
  return `${h}h ${String(m).padStart(2, '0')}m`;
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
