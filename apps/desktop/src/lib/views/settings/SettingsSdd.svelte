<script lang="ts">
  import {
    sddState,
    setSddPhaseExecutionConfig,
    DEFAULT_PHASE_EXECUTION_CONFIG,
  } from '$lib/state/sdd.svelte';
</script>

<!-- SDD execution mode (spec-1) -->
<div class="card">
  <header class="card-head">
    <h2 class="card-title">SDD execution mode</h2>
    <p class="card-sub">
      Three-call mode runs each SDD phase as three discrete agent passes: <span class="mono">plan</span> (read-only analysis), <span class="mono">implement</span> (edits), <span class="mono">verify</span> (structured self-review). Adds ~5–15% cost per phase; improves auditability.
    </p>
  </header>
  {#if sddState.workspaces.length === 0}
    <p class="card-sub mono">No SDD workspaces yet. Start one with <span class="mono">/sdd &lt;ask&gt;</span> to configure execution mode.</p>
  {:else}
    <div class="grid">
      {#each sddState.workspaces as ws (ws.id)}
        <div class="sdd-mode-row">
          <span class="sdd-mode-row-id mono">{ws.id.replace(/^sdd-/, '')}</span>
          <span class="sdd-mode-row-ask">{ws.user_prompt || '(no ask)'}</span>
          <select
            class="sdd-mode-select mono"
            value={ws.phase_execution?.mode ?? 'single_call'}
            onchange={(e) => {
              const mode = (e.currentTarget as HTMLSelectElement).value as 'single_call' | 'three_call';
              void setSddPhaseExecutionConfig(ws.id, {
                ...(ws.phase_execution ?? DEFAULT_PHASE_EXECUTION_CONFIG),
                mode,
              });
            }}
          >
            <option value="single_call">single-call (legacy)</option>
            <option value="three_call">three-call (plan → implement → verify)</option>
          </select>
          <label class="sdd-mode-gate" title="Pause between plan and implement for user review">
            <input
              type="checkbox"
              checked={ws.phase_execution?.plan_gate ?? false}
              disabled={(ws.phase_execution?.mode ?? 'single_call') !== 'three_call'}
              onchange={(e) => {
                const plan_gate = (e.currentTarget as HTMLInputElement).checked;
                void setSddPhaseExecutionConfig(ws.id, {
                  ...(ws.phase_execution ?? DEFAULT_PHASE_EXECUTION_CONFIG),
                  plan_gate,
                });
              }}
            />
            plan-review gate
          </label>
        </div>
      {/each}
    </div>
  {/if}
</div>
