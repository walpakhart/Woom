<script lang="ts">
  /* Settings → Workflows. Home for Dynamic Workflow history, relocated
   *  out of the chat header (which now carries only session vitals).
   *  DW rows are a read-only run log with per-run cost. */
  import { dwState } from '$lib/state/dw.svelte';
  import { formatCostUsd } from '$lib/usage';

  const dwHistory = $derived([...dwState.workflows].sort((a, b) => b.createdAt - a.createdAt));
</script>

<div class="card">
  <header class="card-head">
    <h2 class="card-title">Dynamic Workflows</h2>
    <p class="card-sub">
      Fan-out workflows run across sessions. Newest first, with per-run status and cost.
    </p>
  </header>
  {#if dwHistory.length === 0}
    <p class="card-sub mono">No Dynamic Workflows run yet. Start one with <span class="mono">/dw &lt;ask&gt;</span> in a chat.</p>
  {:else}
    <div class="wf-rows">
      {#each dwHistory as wf (wf.id)}
        <div class="wf-row wf-row--dw">
          <span class="wf-dw-status wf-dw-status--{wf.status} mono">{wf.status}</span>
          <span class="wf-ask">{wf.userPrompt || '(no ask)'}</span>
          <span class="wf-cost mono">{formatCostUsd(wf.totalCostUsd)}</span>
        </div>
      {/each}
    </div>
  {/if}
</div>

<style>
  .wf-rows { display: flex; flex-direction: column; gap: 1px; }
  .wf-row {
    display: flex; align-items: center; gap: 8px;
    padding: 7px 10px; border-radius: 8px;
    transition: background 120ms;
  }
  .wf-row:hover { background: var(--bg-2); }
  .wf-row--dw { gap: 12px; }
  .wf-ask {
    flex: 1; min-width: 0; font-size: 12.5px; color: var(--text-1);
    white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
  }

  .wf-dw-status {
    flex-shrink: 0; min-width: 72px;
    font-size: 9.5px; text-transform: uppercase; letter-spacing: 0.05em;
    color: var(--text-mute);
  }
  .wf-dw-status--done { color: #66d39a; }
  .wf-dw-status--failed, .wf-dw-status--cancelled { color: var(--error, #e88264); }
  .wf-dw-status--running, .wf-dw-status--building, .wf-dw-status--awaiting_verify,
  .wf-dw-status--awaiting_launch, .wf-dw-status--verifying { color: var(--accent-bright); }
  .wf-cost { flex-shrink: 0; font-size: 11.5px; color: var(--accent-bright); }
</style>
