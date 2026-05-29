<script lang="ts">
  import { modalsState, closeModal, patchModal } from '$lib/state/modals.svelte';
  import { notifyError } from '$lib/state/toaster.svelte';

  const m = $derived(modalsState.confirm);

  async function run() {
    if (!m) return;
    patchModal('confirm', { busy: true });
    try {
      await m.onConfirm();
      closeModal('confirm');
    } catch (e) {
      patchModal('confirm', { busy: false });
      notifyError(e, { title: m.title });
    }
  }

  /** Run onCancel (if provided) then close the modal. Used by all
   *  three dismiss paths so re-armable callers (close-window guard,
   *  etc.) see the cancel signal. Synchronous fire-and-forget — the
   *  modal closes immediately even if the callback returns a Promise
   *  to avoid leaving the user staring at a hung dialog. */
  function dismiss() {
    if (!m || m.busy) return;
    try {
      const r = m.onCancel?.();
      if (r && typeof (r as Promise<void>).then === 'function') {
        void (r as Promise<void>).catch(() => { /* best effort */ });
      }
    } catch { /* swallow — modal still closes */ }
    closeModal('confirm');
  }
</script>

{#if m}
  <div
    class="modal-backdrop"
    onclick={(e) => { if (e.target === e.currentTarget) dismiss(); }}
    onkeydown={(e) => { if (e.key === 'Escape') dismiss(); }}
    role="dialog"
    aria-modal="true"
    tabindex="-1"
  >
    <div class="modal">
      <header class="modal-head">
        <svg class="i" viewBox="0 0 24 24" style="color: {m.danger ? '#F0A38A' : 'var(--accent-bright)'}">
          {#if m.danger}
            <circle cx="12" cy="12" r="10" /><line x1="12" y1="8" x2="12" y2="12" /><line x1="12" y1="16" x2="12.01" y2="16" />
          {:else}
            <path d="M20 6 9 17l-5-5" />
          {/if}
        </svg>
        <div>
          <div class="modal-title">{m.title}</div>
          <div class="modal-sub">{m.body}</div>
        </div>
        <button class="modal-close" onclick={dismiss} disabled={m.busy} aria-label="Close">
          <svg class="i" viewBox="0 0 24 24"><path d="M18 6 6 18M6 6l12 12" /></svg>
        </button>
      </header>
      <footer class="modal-foot">
        <button class="btn btn--ghost" onclick={dismiss} disabled={m.busy}>Cancel</button>
        <button class="btn {m.danger ? 'btn--danger' : 'btn--primary'}" onclick={run} disabled={m.busy}>
          {m.busy ? 'Working…' : m.confirmText}
        </button>
      </footer>
    </div>
  </div>
{/if}
