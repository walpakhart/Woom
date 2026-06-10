<script lang="ts">
  /* Settings → Workflows. Home for SDD workspace history + Dynamic
   *  Workflow history, relocated out of the chat header (which now
   *  carries only session vitals). SDD rows reopen the standalone
   *  view / discard; DW rows are a read-only run log with per-run cost. */
  import {
    sddState,
    openStandaloneView,
    discardSdd,
    showSddCard,
    type SddWorkspace,
    type SddPhase,
  } from '$lib/state/sdd.svelte';
  import { dwState } from '$lib/state/dw.svelte';
  import { formatCostUsd } from '$lib/usage';

  function sddStageLabel(w: SddWorkspace): string {
    const s = w.stage;
    switch (s.kind) {
      case 'drafting': return 'drafting spec';
      case 'spec_ready': return 'spec ready';
      case 'planning': return 'drafting plan';
      case 'plan_ready': return 'plan ready';
      case 'phase_pending_approval': return `phase ${s.phase} pending`;
      case 'phase_running': return `phase ${s.phase} running`;
      case 'phase_planning': return `phase ${s.phase} planning`;
      case 'phase_plan_review': return `phase ${s.phase} plan review`;
      case 'phase_implementing': return `phase ${s.phase} implementing`;
      case 'phase_verifying': return `phase ${s.phase} verifying`;
      case 'phase_done': return `phase ${s.phase} done`;
      case 'complete': return 'complete';
      case 'paused': return 'paused';
      case 'stopped': return 'stopped';
      case 'failed': return 'failed';
    }
  }
  function sddStageTone(w: SddWorkspace): 'live' | 'ok' | 'warn' | 'dim' {
    const k = w.stage.kind;
    if (
      k === 'drafting' || k === 'planning' || k === 'phase_running' ||
      k === 'phase_planning' || k === 'phase_implementing' || k === 'phase_verifying'
    ) return 'live';
    if (k === 'phase_plan_review' || k === 'failed' || k === 'stopped') return 'warn';
    if (k === 'complete') return 'ok';
    return 'dim';
  }
  function sddPhaseProgress(w: SddWorkspace): string {
    if (w.phases.length === 0) return '';
    const done = w.phases.filter((ph: SddPhase) => ph.status === 'done').length;
    return `${done}/${w.phases.length}`;
  }
  function sddOpen(id: string) {
    showSddCard(id);
    openStandaloneView(id);
  }

  const dwHistory = $derived([...dwState.workflows].sort((a, b) => b.createdAt - a.createdAt));
</script>

<div class="card">
  <header class="card-head">
    <h2 class="card-title">SDD workspaces</h2>
    <p class="card-sub">
      Spec-driven runs on disk. Click a row to reopen it in the standalone view; trash to discard.
    </p>
  </header>
  {#if sddState.workspaces.length === 0}
    <p class="card-sub mono">No specs yet. Type <span class="mono">/sdd &lt;ask&gt;</span> in a chat to start one.</p>
  {:else}
    <div class="wf-rows">
      {#each sddState.workspaces as w (w.id)}
        <div class="wf-row" data-tone={sddStageTone(w)}>
          <button class="wf-row-main" type="button" onclick={() => sddOpen(w.id)} title="Reopen this workspace">
            <span class="wf-stage mono">{sddStageLabel(w)}</span>
            <span class="wf-ask">{w.user_prompt || '(no ask)'}</span>
            {#if sddPhaseProgress(w)}
              <span class="wf-prog mono">{sddPhaseProgress(w)}</span>
            {/if}
          </button>
          <button class="wf-discard" type="button" onclick={() => void discardSdd(w.id)} title="Delete this workspace" aria-label="Delete workspace">
            <svg viewBox="0 0 24 24" width="12" height="12" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
              <polyline points="3 6 5 6 21 6"/><path d="M19 6l-2 14a2 2 0 0 1-2 2H9a2 2 0 0 1-2-2L5 6"/>
            </svg>
          </button>
        </div>
      {/each}
    </div>
  {/if}
</div>

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
  .wf-row-main {
    flex: 1; min-width: 0;
    display: inline-flex; align-items: baseline; gap: 12px;
    background: transparent; border: 0; padding: 0;
    color: var(--text-1); cursor: pointer; text-align: left; font: inherit;
  }
  .wf-stage {
    flex-shrink: 0; font-size: 10.5px; color: var(--text-mute); min-width: 110px;
  }
  .wf-row[data-tone="live"] .wf-stage { color: #66d39a; }
  .wf-row[data-tone="warn"] .wf-stage { color: var(--warm, #e0b16c); }
  .wf-row[data-tone="ok"] .wf-stage { color: var(--accent-bright); }
  .wf-ask {
    flex: 1; min-width: 0; font-size: 12.5px; color: var(--text-1);
    white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
  }
  .wf-prog { flex-shrink: 0; font-size: 10.5px; color: var(--text-mute); }
  .wf-discard {
    width: 24px; height: 24px; flex-shrink: 0;
    display: grid; place-items: center;
    background: transparent; border: 0; border-radius: 5px;
    color: var(--text-mute); cursor: pointer;
    transition: color 120ms, background 120ms;
  }
  .wf-discard:hover { color: var(--error, #e88264); background: var(--bg-3); }

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
