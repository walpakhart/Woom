<script lang="ts">
  /* Dynamic Workflows preflight modal (SDD `sdd-98a42f3bdb` Phase 4).
   * Surfaces the planner output + cost estimate BEFORE the user
   * commits to firing fan-out. Editable budget cap so they can raise
   * above the $5 default if the workflow needs more headroom. */
  import { modalsState, closeModal } from '$lib/state/modals.svelte';
  import { formatCostUsd } from '$lib/usage';
  import { focusTrap } from '$lib/actions/focusTrap';

  const m = $derived(modalsState.dwPreflight);

  let expanded = $state<Record<string, boolean>>({});

  function finish(action: { kind: 'approve'; cap: number } | { kind: 'cancel' }) {
    const cur = m;
    closeModal('dwPreflight');
    cur?.resolve?.(action);
  }
</script>

{#if m}
  <div
    class="modal-backdrop"
    onclick={(e) => { if (e.target === e.currentTarget) finish({ kind: 'cancel' }); }}
    onkeydown={(e) => { if (e.key === 'Escape') finish({ kind: 'cancel' }); }}
    role="dialog"
    aria-modal="true"
    tabindex="-1"
    use:focusTrap
  >
    <div class="modal dwp-modal">
      <header class="modal-head">
        <div>
          <div class="modal-title">Dynamic Workflow — pre-flight</div>
          <div class="modal-sub">{m.plan.subagents.length} subagent{m.plan.subagents.length === 1 ? '' : 's'} · estimated cost <span class="mono">{formatCostUsd(m.estimateUsd)}</span></div>
        </div>
        <button class="modal-close" onclick={() => finish({ kind: 'cancel' })} aria-label="Cancel">
          <svg class="i" viewBox="0 0 24 24"><path d="M18 6 6 18M6 6l12 12" /></svg>
        </button>
      </header>
      <div class="modal-body">
        <div class="dwp-rationale">{m.plan.rationale}</div>
        <ul class="dwp-subs">
          {#each m.plan.subagents as sub (sub.id)}
            <li class="dwp-sub">
              <button
                class="dwp-sub-head"
                onclick={() => (expanded[sub.id] = !expanded[sub.id])}
                aria-expanded={!!expanded[sub.id]}
              >
                <span class="dwp-sub-id mono">{sub.id}</span>
                <span class="dwp-sub-preview">{sub.prompt.slice(0, 100)}{sub.prompt.length > 100 ? '…' : ''}</span>
              </button>
              {#if expanded[sub.id]}
                <pre class="dwp-sub-prompt">{sub.prompt}</pre>
              {/if}
            </li>
          {/each}
        </ul>
      </div>
      <footer class="modal-actions">
        <button class="btn-secondary" onclick={() => finish({ kind: 'cancel' })}>
          Cancel
        </button>
        <button
          class="btn-primary"
          onclick={() => finish({ kind: 'approve', cap: m.budgetCap })}
        >
          Approve workflow
        </button>
      </footer>
    </div>
  </div>
{/if}

<style>
  .dwp-modal {
    max-width: 560px;
    width: 90vw;
  }
  .dwp-rationale {
    padding: 8px 10px;
    background: var(--bg-2);
    border-left: 2px solid var(--accent);
    border-radius: 4px;
    font-size: 12.5px;
    color: var(--text-1);
    margin-bottom: 12px;
  }
  .dwp-subs {
    margin: 0 0 12px;
    padding: 0;
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: 4px;
    max-height: 260px;
    overflow-y: auto;
  }
  .dwp-sub-head {
    width: 100%;
    background: transparent;
    border: 1px solid var(--border);
    border-radius: 5px;
    padding: 6px 9px;
    text-align: left;
    cursor: pointer;
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--text-1);
    font-size: 12px;
    transition: background 120ms, border-color 120ms;
  }
  .dwp-sub-head:hover {
    background: var(--bg-2);
    border-color: var(--border-hi);
  }
  .dwp-sub-id {
    flex-shrink: 0;
    color: var(--accent-bright, var(--accent));
    font-size: 11px;
  }
  .dwp-sub-preview {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--text-mute);
  }
  .dwp-sub-prompt {
    margin: 4px 0 0;
    padding: 8px 10px;
    background: var(--bg-3);
    border-radius: 4px;
    font-size: 11.5px;
    white-space: pre-wrap;
    color: var(--text-1);
  }
  .modal-actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    padding: 12px 16px 14px;
    border-top: 1px solid var(--border);
  }
  .btn-primary,
  .btn-secondary {
    padding: 7px 14px;
    border-radius: 6px;
    font-size: 12.5px;
    font-weight: 600;
    cursor: pointer;
    transition: background 120ms, border-color 120ms;
  }
  .btn-primary {
    background: var(--accent);
    color: var(--accent-fg);
    border: 1px solid var(--accent);
  }
  .btn-primary:hover {
    background: var(--accent-bright, var(--accent));
  }
  .btn-secondary {
    background: var(--bg-2);
    color: var(--text-1);
    border: 1px solid var(--border);
  }
  .btn-secondary:hover {
    background: var(--bg-3);
    border-color: var(--border-hi);
  }
  .mono { font-family: 'JetBrains Mono', monospace; }
</style>
