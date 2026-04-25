<script lang="ts">
  import { externalId } from '$lib/data';
  import { inboxState } from '$lib/state/inbox.svelte';
  import { modalsState, closeModal, patchModal, type ReviewEvent } from '$lib/state/modals.svelte';

  interface Props {
    onSubmit: () => Promise<void> | void;
  }
  let { onSubmit }: Props = $props();

  const m = $derived(modalsState.review);
</script>

{#if m}
  <div
    class="modal-backdrop"
    onclick={(e) => { if (e.target === e.currentTarget && !m.busy) closeModal('review'); }}
    onkeydown={(e) => { if (e.key === 'Escape' && !m.busy) closeModal('review'); }}
    role="dialog"
    aria-modal="true"
    tabindex="-1"
  >
    <div class="modal modal-wide">
      <header class="modal-head">
        <svg class="i" viewBox="0 0 24 24" style="color: var(--text-1)"><path d="M20 6 9 17l-5-5" /></svg>
        <div>
          <div class="modal-title">Submit review</div>
          <div class="modal-sub">on {inboxState.focusItem ? externalId(inboxState.focusItem) : ''}</div>
        </div>
        <button class="modal-close" onclick={() => closeModal('review')} disabled={m.busy} aria-label="Close">
          <svg class="i" viewBox="0 0 24 24"><path d="M18 6 6 18M6 6l12 12" /></svg>
        </button>
      </header>
      <div class="modal-body">
        <div class="radio-group">
          {#each ['APPROVE', 'REQUEST_CHANGES', 'COMMENT'] as ev (ev)}
            {@const e = ev as ReviewEvent}
            <label class="radio" class:selected={m.event === e}>
              <input
                type="radio"
                name="review-event"
                checked={m.event === e}
                onchange={() => patchModal('review', { event: e })}
                disabled={m.busy}
              />
              <div>
                <div class="radio-title">
                  {e === 'APPROVE' ? 'Approve' : e === 'REQUEST_CHANGES' ? 'Request changes' : 'Comment'}
                </div>
                <div class="radio-desc">
                  {e === 'APPROVE' ? 'Submit approval' : e === 'REQUEST_CHANGES' ? 'Requires author updates' : 'General feedback without approval'}
                </div>
              </div>
            </label>
          {/each}
        </div>
        <!-- svelte-ignore a11y_autofocus -->
        <textarea
          class="field-textarea"
          value={m.body}
          oninput={(ev) => patchModal('review', { body: (ev.currentTarget as HTMLTextAreaElement).value })}
          placeholder={m.event === 'APPROVE' ? 'Optional: a word on why (markdown supported)' : 'Review body (markdown supported)'}
          autofocus
          disabled={m.busy}
        ></textarea>
        {#if m.error}<div class="modal-error">{m.error}</div>{/if}
      </div>
      <footer class="modal-foot">
        <button class="btn btn--ghost" onclick={() => closeModal('review')} disabled={m.busy}>Cancel</button>
        <button
          class="btn btn--primary"
          onclick={() => void onSubmit()}
          disabled={m.busy || (m.event !== 'APPROVE' && !m.body.trim())}
        >
          {m.busy ? 'Submitting…' : 'Submit review'}
        </button>
      </footer>
    </div>
  </div>
{/if}
