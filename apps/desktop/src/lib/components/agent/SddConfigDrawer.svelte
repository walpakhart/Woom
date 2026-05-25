<script lang="ts">
  /* SddConfigDrawer — inline workspace-config panel that opens
     beneath the SddCard header on cog click. Extracted from SddCard
     in wave-13 split. Mirrors the Settings card controls but scoped
     to this workspace (`meta.json#phase_execution`).

     Self-contained: the change handlers fire the
     `setSddPhaseExecutionConfig` Tauri command directly — parent
     doesn't have to thread setters through. */
  import {
    DEFAULT_PHASE_EXECUTION_CONFIG,
    setSddPhaseExecutionConfig,
    type PhaseExecutionConfig,
    type SddWorkspace,
  } from '$lib/state/sdd.svelte';

  interface Props {
    workspace: SddWorkspace;
  }
  let { workspace }: Props = $props();
</script>

<div class="sdd-config-drawer">
  <label class="sdd-config-row">
    <span class="sdd-config-label">Execution mode</span>
    <select
      class="sdd-config-select mono"
      value={workspace.phase_execution?.mode ?? 'single_call'}
      onchange={(e) => {
        const mode = (e.currentTarget as HTMLSelectElement).value as 'single_call' | 'three_call';
        void setSddPhaseExecutionConfig(workspace.id, {
          ...(workspace.phase_execution ?? DEFAULT_PHASE_EXECUTION_CONFIG),
          mode,
        } satisfies PhaseExecutionConfig);
      }}
    >
      <option value="single_call">single-call (legacy)</option>
      <option value="three_call">three-call (plan → implement → verify)</option>
    </select>
  </label>
  <label class="sdd-config-row sdd-config-row--toggle">
    <input
      type="checkbox"
      checked={workspace.phase_execution?.plan_gate ?? false}
      disabled={(workspace.phase_execution?.mode ?? 'single_call') !== 'three_call'}
      onchange={(e) => {
        const plan_gate = (e.currentTarget as HTMLInputElement).checked;
        void setSddPhaseExecutionConfig(workspace.id, {
          ...(workspace.phase_execution ?? DEFAULT_PHASE_EXECUTION_CONFIG),
          plan_gate,
        } satisfies PhaseExecutionConfig);
      }}
    />
    <span class="sdd-config-label">Pause between plan and implement (plan-review gate)</span>
  </label>
  <p class="sdd-config-hint">
    Three-call mode runs each phase as three discrete agent passes — adds ~5–15% cost per phase, improves auditability. Config persists in <span class="mono">meta.json#phase_execution</span>.
  </p>
</div>

<style>
  .sdd-config-drawer {
    margin: 6px 0 8px 0;
    padding: 8px 12px;
    border: 1px solid color-mix(in srgb, var(--accent) 22%, transparent);
    border-radius: 6px;
    background: color-mix(in srgb, var(--accent) 4%, transparent);
    display: flex;
    flex-direction: column;
    gap: 6px;
    font-size: 12px;
  }
  .sdd-config-row {
    display: inline-flex;
    align-items: center;
    gap: 8px;
  }
  .sdd-config-row--toggle { font-size: 11.5px; color: var(--text-1); }
  .sdd-config-row--toggle input[disabled] { opacity: 0.4; }
  .sdd-config-label { color: var(--text-1); }
  .sdd-config-select {
    padding: 3px 6px;
    border-radius: 4px;
    border: 1px solid var(--border);
    background: var(--bg-1);
    color: var(--text-0);
    font-size: 11px;
  }
  .sdd-config-hint {
    margin: 2px 0 0 0;
    font-size: 11px;
    color: var(--text-mute);
    line-height: 1.4;
  }
</style>
