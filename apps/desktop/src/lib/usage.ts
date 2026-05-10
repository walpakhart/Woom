// Token / cost / context-window helpers for the Claude usage UI. Pure
// functions only — the chat column and header chip both need the same
// math so we centralise it here.

import type { ClaudeUsage } from '$lib/types';

/** USD per million tokens, by model id. Keys are the exact `model` strings
 *  Claude CLI returns in stream-json (e.g. `claude-sonnet-4-6`).
 *  Unknown / null models fall through to a 0-cost result rather than a
 *  guessed-rate one — Cursor turns don't carry a model id on their
 *  `result.usage` event and Cursor's pricing is subscription-credit
 *  based anyway, so showing a fabricated USD figure for those would
 *  mislead. The badge checks `cost > 0` to decide whether to render
 *  the cost span at all. */
const RATE_TABLE: Record<
  string,
  { input: number; output: number; cacheWrite: number; cacheRead: number }
> = {
  'claude-opus-4-7': { input: 15, output: 75, cacheWrite: 18.75, cacheRead: 1.5 },
  'claude-sonnet-4-6': { input: 3, output: 15, cacheWrite: 3.75, cacheRead: 0.3 },
  'claude-haiku-4-5-20251001': { input: 0.8, output: 4, cacheWrite: 1, cacheRead: 0.08 }
};

/** Effective context-window size in tokens. The cap depends on **both**
 *  the model id and the surface the session is running on, because:
 *
 *    • Claude Code (Anthropic SDK) on Max plan: Opus 4.7 ships a 1M
 *      variant; Sonnet/Haiku stay at 200k.
 *    • cursor-agent (Cursor CLI): even Opus 4.7 is capped at 200k under
 *      the standard subscription. Cursor's "Max mode" toggle is about
 *      tool/think budget, NOT context size — observed live as
 *      "70.4% · 140.9K / 200K context used" with Max enabled, so the
 *      Woom ring was over-reporting "23%" when the user was
 *      actually past 70% headed into auto-summary territory.
 *
 *  Defaults `agentKind = 'claude'` so existing call sites without the
 *  arg keep their 1M-for-Opus behavior; Cursor sessions explicitly
 *  pass `'cursor'` and get the safe 200k cap. If we later add a
 *  per-session "Cursor Ultra plan" toggle (or detect it from
 *  cursor-agent's status payload), this is the single switch. */
export function contextWindowFor(
  model: string | null,
  agentKind: 'claude' | 'cursor' = 'claude'
): number {
  if (agentKind === 'cursor') return 200_000;
  if (!model) return 200_000;
  if (model.startsWith('claude-opus-4-7')) return 1_000_000;
  return 200_000;
}

/** USD cost of one usage snapshot. We treat each token bucket separately
 *  because cache_read is ~10x cheaper than fresh input — averaging would
 *  hide the win from prompt caching that Forge specifically optimises
 *  for. Returns 0 when:
 *    - every counter is 0 (pre-usage messages)
 *    - or the model id isn't in the rate table (unknown / null —
 *      typical for Cursor turns; Cursor uses subscription credits,
 *      not per-token billing, so a guessed USD number would mislead). */
export function costForUsage(usage: ClaudeUsage): number {
  const r = usage.model ? RATE_TABLE[usage.model] : undefined;
  if (!r) return 0;
  return (
    (usage.inputTokens * r.input
      + usage.cacheCreationTokens * r.cacheWrite
      + usage.cacheReadTokens * r.cacheRead
      + usage.outputTokens * r.output) / 1_000_000
  );
}

/** "1.2k", "350k", "5.4M" — short token-count formatter for the badge.
 *  Designed to fit in a chip without wrapping; raw numbers like 348282
 *  blow up the layout. */
export function formatTokens(n: number): string {
  if (!Number.isFinite(n) || n <= 0) return '0';
  if (n < 1000) return String(n);
  if (n < 1_000_000) {
    const k = n / 1000;
    return k >= 100 ? `${Math.round(k)}k` : `${k.toFixed(1).replace(/\.0$/, '')}k`;
  }
  const m = n / 1_000_000;
  return m >= 100 ? `${Math.round(m)}M` : `${m.toFixed(1).replace(/\.0$/, '')}M`;
}

/** "$0.0042", "$0.18", "$2.40" — picks decimals that show meaningful
 *  precision without scientific notation. Sub-cent costs use 4
 *  decimals; cent-and-up use 2. */
export function formatCostUsd(usd: number): string {
  if (!Number.isFinite(usd) || usd <= 0) return '$0';
  if (usd < 0.01) return `$${usd.toFixed(4)}`;
  if (usd < 1) return `$${usd.toFixed(3)}`;
  return `$${usd.toFixed(2)}`;
}

/** What share of the context window did this turn fill? Used by the
 *  ring indicator in AgentColumn's header. Returns 0..1, clamped.
 *
 *  Pass `agentKind` so we use the correct surface-specific cap (Cursor
 *  sessions are 200k regardless of model — see `contextWindowFor`).
 *  Defaults to `'claude'` for back-compat with call sites that haven't
 *  been threaded through yet. */
export function contextPct(
  usage: ClaudeUsage,
  agentKind: 'claude' | 'cursor' = 'claude'
): number {
  const cap = contextWindowFor(usage.model, agentKind);
  if (cap <= 0) return 0;
  return Math.min(1, Math.max(0, usage.contextSize / cap));
}

/** Cache hit-rate for one snapshot — `cache_read / (input + cache_read +
 *  cache_creation)`. The closer to 1, the cheaper this turn was per
 *  token of input. Used in the per-message badge. Null when there's
 *  no input at all (defensive — shouldn't happen but the math would
 *  divide by zero). */
export function cacheHitRate(usage: ClaudeUsage): number | null {
  const total = usage.inputTokens + usage.cacheReadTokens + usage.cacheCreationTokens;
  if (total <= 0) return null;
  return usage.cacheReadTokens / total;
}
