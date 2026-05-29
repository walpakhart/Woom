// Token / cost / context-window helpers for the Claude usage UI. Pure
// functions only — the chat column and header chip both need the same
// math so we centralise it here.

import type { ClaudeSession, ClaudeUsage } from '$lib/types';

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
  /* Opus 4.8 (launched 2026-05-28). Cheaper than 4.7 — Anthropic's
   * press release frames the headline number as "3× cheaper than 4.7";
   * cache write/read follow the standard 1.25× / 0.1× of base.
   * Fast mode = base × 2 across all four buckets (2.5× faster output,
   * dedicated endpoint). 1M-context variant = base × 2 too. The four
   * possible combinations are encoded flat (no derived multipliers at
   * lookup) so debugging a wrong-rate diff in production is one grep. */
  'claude-opus-4-8':            { input: 5,  output: 25,  cacheWrite: 6.25, cacheRead: 0.5 },
  'claude-opus-4-8[1m]':        { input: 10, output: 50,  cacheWrite: 12.5, cacheRead: 1.0 },
  'claude-opus-4-8:fast':       { input: 10, output: 50,  cacheWrite: 12.5, cacheRead: 1.0 },
  'claude-opus-4-8[1m]:fast':   { input: 20, output: 100, cacheWrite: 25.0, cacheRead: 2.0 },
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
  /* Opus 4.8 default tier dropped to 200K; the dedicated 1M variant
   * carries an explicit `[1m]` suffix in the model id. Order matters —
   * check the 1M variant first because `startsWith('claude-opus-4-8')`
   * matches both. */
  if (model.startsWith('claude-opus-4-8[1m]')) return 1_000_000;
  if (model.startsWith('claude-opus-4-8')) return 200_000;
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
  if (!usage.model) return 0;
  /* Fast-mode keying: append `:fast` to the model id when the session
   * stamped `fastMode: true`. The RATE_TABLE has explicit `:fast`
   * entries for the variants where Fast is supported (Opus 4.8 +
   * 4.8[1m] today). If the composite key misses, fall back to the
   * base entry — defence against a future model getting fastMode-set
   * but no `:fast` rate listed yet (cost reports under-bill rather
   * than zero). */
  const fastKey = usage.fastMode === true ? `${usage.model}:fast` : null;
  const r = (fastKey && RATE_TABLE[fastKey]) || RATE_TABLE[usage.model];
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
 *  ring indicator in AgentApp's header. Returns 0..1, clamped.
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

/** Aggregate token + USD totals across every assistant-message
 *  `usage` snapshot in a session. Each `ClaudeUsage` lives on the
 *  message it was stamped onto (see `ClaudeMessage.usage` in
 *  `types.ts`) — we sum the buckets directly and dollar-cost each
 *  snapshot individually so per-message model swaps cost correctly
 *  (e.g. user jumped from Sonnet to Opus mid-chat).
 *
 *  Returns a fully-defaulted shape so the ChatHeader chip never has
 *  to guard against partial data; new sessions with zero turns just
 *  render as "0 tok · $0". */
export function sessionUsageTotals(sess: ClaudeSession | null): {
  input: number;
  output: number;
  cacheRead: number;
  cacheCreation: number;
  costUsd: number;
  turns: number;
} {
  const acc = {
    input: 0,
    output: 0,
    cacheRead: 0,
    cacheCreation: 0,
    costUsd: 0,
    turns: 0,
  };
  if (!sess) return acc;
  for (const m of sess.messages) {
    if (m.role !== 'assistant') continue;
    const u = m.usage;
    if (!u) continue;
    acc.input += u.inputTokens || 0;
    acc.output += u.outputTokens || 0;
    acc.cacheRead += u.cacheReadTokens || 0;
    acc.cacheCreation += u.cacheCreationTokens || 0;
    acc.costUsd += costForUsage(u);
    acc.turns += 1;
  }
  return acc;
}

/** Heuristic estimate of tokens + USD saved by RTK output-compression
 *  in this session. Source signal: bash tool-use traces (Claude CLI
 *  emits each Bash invocation as a segment starting with `Bash(` in
 *  the message's `trace` event). Each rewritten bash call saves
 *  roughly 70% of an average 2K-token output, so:
 *
 *      tokensSaved ≈ bashCalls × 0.7 × 2000
 *
 *  USD cost is approximated via the session's output rate (worst-case;
 *  bash output goes into the agent's input on the NEXT turn, but rate
 *  parity makes the rough number reasonable). Below 3 bash calls we
 *  return zeros — the heuristic is too noisy for tiny sessions and
 *  the popover hides the line. See SDD `sdd-98a42f3bdb` Phase 3 plan
 *  for the rationale + open question on instrumented telemetry. */
export function estimateRtkSavings(sess: ClaudeSession | null): {
  tokensSaved: number;
  usdSaved: number;
  bashCalls: number;
} {
  if (!sess) return { tokensSaved: 0, usdSaved: 0, bashCalls: 0 };
  let bashCalls = 0;
  for (const m of sess.messages) {
    if (m.role !== 'assistant') continue;
    if (!m.events) continue;
    for (const ev of m.events) {
      if (ev.kind !== 'trace') continue;
      for (const seg of ev.segments) {
        /* Claude CLI's trace segment for a Bash tool call looks like
         * `Bash(git status)` / `bash(cargo test)`. Match the prefix
         * tolerantly (case-insensitive, leading whitespace OK). */
        if (/^\s*bash\s*\(/i.test(seg)) bashCalls += 1;
      }
    }
  }
  if (bashCalls < 3) return { tokensSaved: 0, usdSaved: 0, bashCalls };
  const tokensSaved = bashCalls * 1400; // 0.7 × 2K rounded
  /* Cost via output-rate of the session's current model. Use a fake
   * usage envelope so `costForUsage` runs the same lookup path the
   * budget chip uses (including Fast-mode keying). */
  const probe: ClaudeUsage = {
    inputTokens: 0,
    cacheCreationTokens: 0,
    cacheReadTokens: 0,
    outputTokens: tokensSaved,
    contextSize: 0,
    model: sess.claudeModel ?? null,
    fastMode: sess.fastMode === true
  };
  const usdSaved = costForUsage(probe);
  return { tokensSaved, usdSaved, bashCalls };
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
