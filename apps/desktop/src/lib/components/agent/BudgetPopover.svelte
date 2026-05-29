<script lang="ts">
  /* Budget popover (SDD `sdd-98a42f3bdb` Phase 3). Renders a per-bucket
   * cost breakdown for the current session, plus RTK savings estimate
   * (when ≥3 bash calls have run), a per-turn cost sparkline, and a
   * CSV-export footer link.
   *
   * Owned by ChatHeader.svelte — parent controls open/close + outside-
   * click + ESC dismissal. This component is render-only. */
  import type { ClaudeSession, ClaudeUsage } from '$lib/types';
  import {
    costForUsage,
    estimateRtkSavings,
    formatCostUsd,
    formatTokens,
    sessionUsageTotals,
  } from '$lib/usage';

  interface Props {
    session: ClaudeSession;
    onClose: () => void;
  }
  const { session, onClose }: Props = $props();

  const totals = $derived(sessionUsageTotals(session));
  const rtkSavings = $derived(estimateRtkSavings(session));

  /* Cost contribution per bucket. MUST cost each turn at the model it
   * actually ran on (`m.usage.model`), NOT the session's CURRENT model
   * — otherwise switching the model mid-chat (e.g. Haiku turns, then
   * flip to Opus) re-prices every past token at the new rate and the
   * four bucket lines no longer sum to the header total (which
   * `sessionUsageTotals` computes per-message). We mirror that: walk
   * messages, isolate one bucket per probe, re-use `costForUsage` so
   * rate + Fast-mode keying stay consistent. Σ buckets === totals.costUsd. */
  function bucketCost(bucket: 'input' | 'output' | 'cacheRead' | 'cacheCreation'): number {
    let sum = 0;
    for (const m of session.messages) {
      if (m.role !== 'assistant' || !m.usage) continue;
      const u = m.usage;
      const probe: ClaudeUsage = {
        inputTokens: bucket === 'input' ? (u.inputTokens || 0) : 0,
        cacheCreationTokens: bucket === 'cacheCreation' ? (u.cacheCreationTokens || 0) : 0,
        cacheReadTokens: bucket === 'cacheRead' ? (u.cacheReadTokens || 0) : 0,
        outputTokens: bucket === 'output' ? (u.outputTokens || 0) : 0,
        contextSize: 0,
        model: u.model ?? null,
        fastMode: u.fastMode === true,
      };
      sum += costForUsage(probe);
    }
    return sum;
  }

  /* Per-turn cost series for the sparkline. Walks assistant messages
   * in chronological order; each `usage` snapshot is one turn. */
  const series = $derived.by(() => {
    const out: { i: number; cost: number }[] = [];
    let i = 0;
    for (const m of session.messages) {
      if (m.role !== 'assistant' || !m.usage) continue;
      out.push({ i, cost: costForUsage(m.usage) });
      i += 1;
    }
    return out;
  });

  /* Inline-SVG sparkline path. 200×28 viewbox. Single peak dominates
   * the y-axis (linear scale) — documented limitation; log-scale is
   * a V2 follow-up. */
  const sparklinePath = $derived.by(() => {
    if (series.length === 0) return '';
    const maxCost = Math.max(...series.map((p) => p.cost), 0.0001);
    const stepX = series.length > 1 ? 200 / (series.length - 1) : 0;
    return series
      .map((p, idx) => {
        const x = (idx * stepX).toFixed(2);
        const y = (28 - (p.cost / maxCost) * 24 - 2).toFixed(2);
        return `${idx === 0 ? 'M' : 'L'} ${x} ${y}`;
      })
      .join(' ');
  });

  function exportCsv() {
    const lines: string[] = [];
    lines.push('section,label,tokens,usd');
    lines.push(`totals,input,${totals.input},${bucketCost('input').toFixed(6)}`);
    lines.push(`totals,output,${totals.output},${bucketCost('output').toFixed(6)}`);
    lines.push(`totals,cache_read,${totals.cacheRead},${bucketCost('cacheRead').toFixed(6)}`);
    lines.push(`totals,cache_creation,${totals.cacheCreation},${bucketCost('cacheCreation').toFixed(6)}`);
    lines.push(`totals,total,${totals.input + totals.output},${totals.costUsd.toFixed(6)}`);
    if (rtkSavings.bashCalls >= 3) {
      lines.push(`rtk,saved,${rtkSavings.tokensSaved},${rtkSavings.usdSaved.toFixed(6)}`);
    }
    let turnIdx = 0;
    for (const m of session.messages) {
      if (m.role !== 'assistant' || !m.usage) continue;
      const u = m.usage;
      const cost = costForUsage(u);
      lines.push(
        `turn,${turnIdx},${u.inputTokens + u.outputTokens + u.cacheReadTokens + u.cacheCreationTokens},${cost.toFixed(6)}`
      );
      turnIdx += 1;
    }
    const csv = lines.join('\n') + '\n';
    const blob = new Blob([csv], { type: 'text/csv;charset=utf-8' });
    const url = URL.createObjectURL(blob);
    const yyyy = new Date().toISOString().slice(0, 10);
    const a = document.createElement('a');
    a.href = url;
    a.download = `session-${session.id}-budget-${yyyy}.csv`;
    document.body.appendChild(a);
    a.click();
    document.body.removeChild(a);
    /* Revoke after a short delay so the synthetic-click download
     * settles before the blob URL invalidates. */
    setTimeout(() => URL.revokeObjectURL(url), 10_000);
  }
</script>

<div class="bp-popover" role="dialog" aria-label="Session token budget breakdown">
  <header class="bp-head">
    <span class="bp-turns mono">{totals.turns} turn{totals.turns === 1 ? '' : 's'}</span>
    <span class="bp-total mono">
      {formatTokens(totals.input + totals.output)} · {formatCostUsd(totals.costUsd)}
    </span>
    <button class="bp-close" onclick={onClose} aria-label="Close">
      <svg viewBox="0 0 24 24" width="13" height="13" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" aria-hidden="true">
        <line x1="18" y1="6" x2="6" y2="18" /><line x1="6" y1="6" x2="18" y2="18" />
      </svg>
    </button>
  </header>

  <ul class="bp-buckets">
    <li>
      <span class="bp-bucket-label">Input</span>
      <span class="bp-bucket-vals mono">{formatTokens(totals.input)} · {formatCostUsd(bucketCost('input'))}</span>
    </li>
    <li>
      <span class="bp-bucket-label">Output</span>
      <span class="bp-bucket-vals mono">{formatTokens(totals.output)} · {formatCostUsd(bucketCost('output'))}</span>
    </li>
    <li>
      <span class="bp-bucket-label">Cache read</span>
      <span class="bp-bucket-vals mono" title="~10× cheaper than fresh input">{formatTokens(totals.cacheRead)} · {formatCostUsd(bucketCost('cacheRead'))}</span>
    </li>
    <li>
      <span class="bp-bucket-label">Cache write</span>
      <span class="bp-bucket-vals mono">{formatTokens(totals.cacheCreation)} · {formatCostUsd(bucketCost('cacheCreation'))}</span>
    </li>
  </ul>

  {#if rtkSavings.bashCalls >= 3}
    <div class="bp-rtk">
      <span class="bp-rtk-label">RTK saved</span>
      <strong class="bp-rtk-vals mono">
        ~{formatTokens(rtkSavings.tokensSaved)} tokens · {formatCostUsd(rtkSavings.usdSaved)}
      </strong>
      <small class="bp-rtk-note">{rtkSavings.bashCalls} bash calls rewritten · heuristic</small>
    </div>
  {/if}

  {#if series.length > 1}
    <div class="bp-spark-wrap">
      <span class="bp-spark-label">Per-turn cost</span>
      <svg class="bp-sparkline" viewBox="0 0 200 28" preserveAspectRatio="none" aria-hidden="true">
        <path d={sparklinePath} fill="none" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" stroke-linecap="round" />
      </svg>
    </div>
  {/if}

  <footer class="bp-foot">
    <button class="bp-csv" type="button" onclick={exportCsv}>CSV export ↗</button>
  </footer>
</div>

<style>
  .bp-popover {
    position: absolute;
    top: calc(100% + 6px);
    right: 0;
    z-index: 40;
    width: 280px;
    padding: 10px 12px 8px;
    background: var(--bg-1);
    border: 1px solid var(--border-hi);
    border-radius: 8px;
    box-shadow: 0 6px 22px rgba(0, 0, 0, 0.22);
    color: var(--text-1);
    font-size: 12px;
  }
  .bp-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding-bottom: 8px;
    border-bottom: 1px solid var(--border);
    gap: 8px;
  }
  .bp-turns {
    color: var(--text-mute);
    font-size: 11px;
  }
  .bp-total {
    flex: 1;
    text-align: right;
    color: var(--text-0);
    font-weight: 600;
  }
  .bp-close {
    background: transparent;
    border: none;
    color: var(--text-mute);
    cursor: pointer;
    padding: 2px;
    line-height: 0;
  }
  .bp-close:hover { color: var(--text-1); }
  .bp-buckets {
    margin: 8px 0 0;
    padding: 0;
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .bp-buckets li {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    font-size: 12px;
  }
  .bp-bucket-label { color: var(--text-mute); }
  .bp-bucket-vals { color: var(--text-1); font-size: 11.5px; }
  .bp-rtk {
    margin-top: 10px;
    padding: 7px 9px;
    border-radius: 6px;
    background: color-mix(in srgb, var(--accent) 10%, transparent);
    border: 1px solid color-mix(in srgb, var(--accent) 28%, var(--border));
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .bp-rtk-label {
    font-size: 10.5px;
    color: var(--text-mute);
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }
  .bp-rtk-vals { color: var(--accent-bright, var(--accent)); }
  .bp-rtk-note { font-size: 10px; color: var(--text-mute); }
  .bp-spark-wrap {
    margin-top: 10px;
    padding-top: 8px;
    border-top: 1px solid var(--border);
    color: var(--accent-bright, var(--accent));
  }
  .bp-spark-label {
    display: block;
    font-size: 10.5px;
    color: var(--text-mute);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    margin-bottom: 4px;
  }
  .bp-sparkline {
    display: block;
    width: 100%;
    height: 28px;
  }
  .bp-foot {
    margin-top: 8px;
    padding-top: 6px;
    border-top: 1px solid var(--border);
    display: flex;
    justify-content: flex-end;
  }
  .bp-csv {
    background: transparent;
    border: none;
    color: var(--text-mute);
    font-size: 11px;
    cursor: pointer;
    padding: 0;
  }
  .bp-csv:hover { color: var(--text-1); text-decoration: underline; }
  .mono { font-family: 'JetBrains Mono', monospace; }
</style>
