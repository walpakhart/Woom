<script lang="ts">
  /* Dynamic Workflow card (SDD `sdd-98a42f3bdb` Phase 4). Chat-inline
   * card rendered when an assistant message carries `dwWorkflowId`.
   * Surfaces:
   *   - Status badge (planning / awaiting_approval / running / verifying
   *     / done / failed / cancelled).
   *   - Per-subagent grid with status icon + token+$ counters.
   *   - Aggregate token + $ totals + budget bar.
   *   - Cancel button while running.
   *   - Expanded prompt + result preview on cell click. */
  import { onMount, onDestroy } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import { dwState, getWorkflow, loadWorkflow, updateSubagent, updateWorkflow } from '$lib/state/dw.svelte';
  import { formatCostUsd, formatTokens } from '$lib/usage';

  interface Props {
    workflowId: string;
  }
  const { workflowId }: Props = $props();

  const workflow = $derived(
    dwState.workflows.find((w) => w.id === workflowId) ?? null
  );

  let expandedId = $state<string | null>(null);

  /* Lazy-load full workflow JSON when the card mounts on a shell
   * entry (Phase 5 hydration — `loadPersistedWorkflows` populates
   * summaries only; subagent detail loads on demand here). */
  $effect(() => {
    const w = workflow;
    if (w && w.subagents.length === 0 && w.status !== 'planning' && w.status !== 'awaiting_approval') {
      void loadWorkflow(w.id);
    }
  });

  /* Interrupted state — Phase 5 recovery marks crashed-mid-flight
   * workflows as `failed` with a synthetic `finalAnswer` that begins
   * with this marker. The card surfaces them with an amber strip
   * instead of the default rust accent. */
  const isInterrupted = $derived(
    workflow?.status === 'failed' &&
    workflow.finalAnswer?.startsWith('_Workflow interrupted') === true
  );

  const totalTokens = $derived(
    workflow
      ? workflow.subagents.reduce((acc, s) => acc + s.tokensIn + s.tokensOut, 0)
      : 0
  );

  /* Listen for backend DW events and merge into state. The Tauri
   * commands emit `dw:subagent_done`, `dw:workflow_done`,
   * `dw:workflow_cancelled` after each lifecycle transition. */
  const unlistens: UnlistenFn[] = [];
  onMount(async () => {
    unlistens.push(
      await listen<{
        workflowId: string;
        subagentId: string;
        result: string;
        tokensIn: number;
        tokensOut: number;
        costUsd: number;
      }>('dw:subagent_done', (e) => {
        if (e.payload.workflowId !== workflowId) return;
        updateSubagent(workflowId, e.payload.subagentId, {
          status: 'done',
          result: e.payload.result,
          tokensIn: e.payload.tokensIn,
          tokensOut: e.payload.tokensOut,
          costUsd: e.payload.costUsd
        });
        const w = getWorkflow(workflowId);
        if (w) {
          updateWorkflow(workflowId, {
            totalCostUsd: w.subagents.reduce((acc, s) => acc + s.costUsd, 0)
          });
        }
      })
    );
    unlistens.push(
      await listen<typeof workflow>('dw:workflow_done', (e) => {
        const payload = e.payload as typeof workflow;
        if (!payload || payload.id !== workflowId) return;
        updateWorkflow(workflowId, {
          status: payload.status,
          verifierResult: payload.verifierResult ?? undefined,
          finalAnswer: payload.finalAnswer ?? undefined,
          completedAt: payload.completedAt ?? undefined
        });
      })
    );
    unlistens.push(
      await listen<typeof workflow>('dw:workflow_cancelled', (e) => {
        const payload = e.payload as typeof workflow;
        if (!payload || payload.id !== workflowId) return;
        updateWorkflow(workflowId, {
          status: 'cancelled',
          completedAt: payload?.completedAt ?? undefined
        });
      })
    );
  });
  onDestroy(() => {
    for (const u of unlistens) u();
  });

  async function cancel() {
    try {
      await invoke('dw_cancel', { workflowId });
    } catch (e) {
      console.warn('dw_cancel failed', e);
    }
  }

  function statusIcon(status: string): string {
    switch (status) {
      case 'queued': return '◌';
      case 'streaming': return '◐';
      case 'done': return '●';
      case 'failed': return '✕';
      case 'cancelled': return '⌀';
      default: return '·';
    }
  }
</script>

{#if workflow}
  <div class="dw-card" data-status={workflow.status} class:dw-card--interrupted={isInterrupted}>
    {#if isInterrupted}
      <div class="dw-stale-strip">
        Interrupted on previous shutdown · partial transcripts below
      </div>
    {/if}
    <header class="dw-head">
      <span class="dw-badge dw-badge--{workflow.status}">{workflow.status}</span>
      <span class="dw-rationale">{workflow.planRationale ?? workflow.userPrompt}</span>
      <span class="dw-totals mono">
        {formatTokens(totalTokens)} · {formatCostUsd(workflow.totalCostUsd)}
        <span class="dw-cap">/ {formatCostUsd(workflow.budgetCapUsd)}</span>
      </span>
      {#if workflow.status === 'running' || workflow.status === 'verifying' || workflow.status === 'planning'}
        <button class="dw-cancel" onclick={cancel} aria-label="Cancel workflow">cancel</button>
      {/if}
    </header>

    <ul class="dw-grid">
      {#each workflow.subagents as sub (sub.id)}
        <li class="dw-cell" data-status={sub.status} class:dw-cell--expanded={expandedId === sub.id}>
          <button
            class="dw-cell-head"
            onclick={() => (expandedId = expandedId === sub.id ? null : sub.id)}
          >
            <span class="dw-cell-icon">{statusIcon(sub.status)}</span>
            <span class="dw-cell-id mono">{sub.id}</span>
            <span class="dw-cell-cost mono">{formatCostUsd(sub.costUsd)}</span>
          </button>
          {#if expandedId === sub.id}
            <div class="dw-cell-body">
              <div class="dw-cell-section">
                <div class="dw-cell-label">Prompt</div>
                <pre class="dw-cell-text">{sub.prompt}</pre>
              </div>
              {#if sub.result}
                <div class="dw-cell-section">
                  <div class="dw-cell-label">Result</div>
                  <pre class="dw-cell-text">{sub.result}</pre>
                </div>
              {/if}
              {#if sub.error}
                <div class="dw-cell-section dw-cell-section--error">
                  <div class="dw-cell-label">Error</div>
                  <pre class="dw-cell-text">{sub.error}</pre>
                </div>
              {/if}
            </div>
          {/if}
        </li>
      {/each}
    </ul>

    <!-- Verifier synthesis is appended as a SEPARATE assistant
         ClaudeMessage in the parent session via `+page.svelte`'s
         `dw:workflow_done` listener. Card stays focused on
         per-subagent progress + totals; synthesis behaves like any
         normal claude reply (copy / drag / context-menu, etc.). -->
  </div>
{/if}

<style>
  .dw-card {
    margin: 8px 0;
    padding: 8px 0 10px 14px;
    border-left: 2px solid color-mix(in srgb, var(--accent) 75%, transparent);
    background: transparent;
    transition: border-color 140ms;
  }
  .dw-card:hover {
    background: color-mix(in srgb, var(--accent) 4%, transparent);
  }
  .dw-card[data-status='done'] {
    border-left-color: color-mix(in srgb, #6cb87a 60%, var(--border));
  }
  .dw-card[data-status='failed'],
  .dw-card[data-status='cancelled'] {
    border-left-color: color-mix(in srgb, var(--error, #e88264) 50%, var(--border));
  }
  .dw-card--interrupted {
    border-left-color: color-mix(in srgb, #e0b16c 60%, var(--border));
  }
  .dw-stale-strip {
    font-size: 11px;
    color: #c8923f;
    background: color-mix(in srgb, #e0b16c 10%, transparent);
    border-radius: 4px;
    padding: 3px 8px;
    margin-bottom: 6px;
    border: 1px solid color-mix(in srgb, #e0b16c 30%, var(--border));
  }
  .dw-head {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
    margin-bottom: 6px;
  }
  .dw-badge {
    padding: 1px 7px;
    border-radius: 3px;
    font-size: 9.5px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    background: var(--bg-3);
    color: var(--text-mute);
    border: 1px solid var(--border);
  }
  .dw-badge--running,
  .dw-badge--verifying,
  .dw-badge--planning {
    background: color-mix(in srgb, var(--accent) 12%, transparent);
    border-color: color-mix(in srgb, var(--accent) 35%, var(--border));
    color: var(--accent-bright, var(--accent));
  }
  .dw-badge--done {
    background: color-mix(in srgb, #6cb87a 14%, transparent);
    border-color: color-mix(in srgb, #6cb87a 40%, var(--border));
    color: #6cb87a;
  }
  .dw-badge--failed,
  .dw-badge--cancelled {
    background: color-mix(in srgb, var(--error, #e88264) 10%, transparent);
    border-color: color-mix(in srgb, var(--error, #e88264) 35%, var(--border));
    color: var(--error, #e88264);
  }
  .dw-rationale {
    flex: 1;
    min-width: 160px;
    color: var(--text-1);
    font-size: 12px;
  }
  .dw-totals {
    color: var(--text-mute);
    font-size: 11px;
  }
  .dw-cap { opacity: 0.6; }
  .dw-cancel {
    background: color-mix(in srgb, var(--error, #e88264) 10%, transparent);
    color: var(--error, #e88264);
    border: 1px solid color-mix(in srgb, var(--error, #e88264) 30%, var(--border));
    padding: 2px 8px;
    border-radius: 4px;
    font-size: 11px;
    cursor: pointer;
  }
  .dw-cancel:hover {
    background: color-mix(in srgb, var(--error, #e88264) 18%, transparent);
  }
  .dw-grid {
    margin: 0;
    padding: 0;
    list-style: none;
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(140px, 1fr));
    gap: 4px;
  }
  .dw-cell {
    border: 1px solid var(--border);
    border-radius: 5px;
    background: var(--bg-2);
    overflow: hidden;
    transition: border-color 120ms;
  }
  .dw-cell[data-status='done'] {
    border-color: color-mix(in srgb, #6cb87a 35%, var(--border));
  }
  .dw-cell[data-status='failed'] {
    border-color: color-mix(in srgb, var(--error, #e88264) 35%, var(--border));
  }
  .dw-cell--expanded {
    grid-column: 1 / -1;
  }
  .dw-cell-head {
    width: 100%;
    background: transparent;
    border: none;
    padding: 5px 8px;
    display: flex;
    align-items: center;
    gap: 6px;
    cursor: pointer;
    color: var(--text-1);
    font-size: 11.5px;
  }
  .dw-cell-head:hover { background: var(--bg-3); }
  .dw-cell-icon { color: var(--accent-bright, var(--accent)); }
  .dw-cell-id {
    color: var(--text-mute);
    flex: 1;
    text-align: left;
  }
  .dw-cell-cost { color: var(--text-mute); font-size: 10.5px; }
  .dw-cell-body {
    padding: 6px 10px 10px;
    border-top: 1px solid var(--border);
    background: var(--bg-1);
  }
  .dw-cell-section { margin-top: 6px; }
  .dw-cell-section--error pre { color: var(--error, #e88264); }
  .dw-cell-label {
    color: var(--text-mute);
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    margin-bottom: 3px;
  }
  .dw-cell-text {
    margin: 0;
    font-size: 11.5px;
    white-space: pre-wrap;
    color: var(--text-1);
    line-height: 1.4;
  }
  .dw-final {
    margin-top: 8px;
    padding: 7px 10px;
    border-radius: 5px;
    background: color-mix(in srgb, var(--accent) 6%, var(--bg-2));
    border: 1px solid color-mix(in srgb, var(--accent) 22%, var(--border));
  }
  .dw-final-label {
    color: var(--text-mute);
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    margin-bottom: 4px;
  }
  .dw-final-body {
    margin: 0;
    font-size: 12px;
    color: var(--text-1);
    white-space: pre-wrap;
    line-height: 1.5;
  }
  .mono { font-family: 'JetBrains Mono', monospace; }
</style>
