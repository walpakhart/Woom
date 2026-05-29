<script lang="ts">
  /* Quota-pause modal (SDD `sdd-98a42f3bdb` Phase 2). Opens when
   * `sendClaudeMessage` detects 5H or 7D utilization ≥ 95%. The user
   * chooses between «wait» (queue the prompt + auto-fire on reset)
   * or «cancel» (drop the input). Modal owns a live 1s countdown so
   * the user can see exactly how much longer the bucket needs.
   *
   * Resolution flow: the modal NEVER closes itself silently — every
   * exit path calls `state.resolve(action)` before `closeModal`, so
   * the `openQuotaPauseModal(...)` promise always settles. */
  import { onDestroy } from 'svelte';
  import { modalsState, closeModal } from '$lib/state/modals.svelte';
  import { formatResumeIn } from '$lib/state/quota.svelte';
  import { focusTrap } from '$lib/actions/focusTrap';

  const m = $derived(modalsState.quotaPause);

  /** Live tick — drives the countdown. */
  let now = $state(Date.now());
  const tick = setInterval(() => { now = Date.now(); }, 1000);
  onDestroy(() => clearInterval(tick));

  const remaining = $derived(m ? Math.max(0, m.resumeAt - now) : 0);

  function finish(action: 'wait' | 'cancel') {
    const cur = m;
    closeModal('quotaPause');
    cur?.resolve?.(action);
  }
</script>

{#if m}
  <div
    class="modal-backdrop"
    onclick={(e) => { if (e.target === e.currentTarget) finish('cancel'); }}
    onkeydown={(e) => { if (e.key === 'Escape') finish('cancel'); }}
    role="dialog"
    aria-modal="true"
    tabindex="-1"
    use:focusTrap
  >
    <div class="modal qp-modal">
      <header class="modal-head">
        <div>
          <div class="modal-title">{m.bucketLabel} лимит {Math.round(m.pct)}% — пауза</div>
          <div class="modal-sub">Сброс через <span class="qp-countdown mono">{formatResumeIn(remaining)}</span></div>
        </div>
        <button class="modal-close" onclick={() => finish('cancel')} aria-label="Закрыть">
          <svg class="i" viewBox="0 0 24 24"><path d="M18 6 6 18M6 6l12 12" /></svg>
        </button>
      </header>
      <div class="modal-body">
        <p class="modal-copy">
          Claude Code 5H/7D квота почти исчерпана. Отправка сейчас прервёт твой turn
          mid-stream когда лимит пересечёт 100%. Можешь дождаться сброса — твоё
          сообщение уйдёт в очередь и автоматически отправится как только квота
          восстановится. Или отменить и решить вручную.
        </p>
      </div>
      <footer class="modal-actions">
        <button class="btn-secondary" onclick={() => finish('cancel')}>
          Отмена
        </button>
        <button class="btn-primary" onclick={() => finish('wait')}>
          Подождать сброса
        </button>
      </footer>
    </div>
  </div>
{/if}

<style>
  .qp-modal {
    max-width: 480px;
  }
  .qp-countdown {
    color: var(--accent-bright, var(--accent));
    font-weight: 600;
    letter-spacing: 0.04em;
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
</style>
