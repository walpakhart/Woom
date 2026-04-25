<script lang="ts">
  import { connectionsState } from '$lib/state/connections.svelte';
  import { modalsState, closeModal, patchModal } from '$lib/state/modals.svelte';

  interface Props {
    onSubmit: () => Promise<void> | void;
  }
  let { onSubmit }: Props = $props();

  const m = $derived(modalsState.comment);
  const githubStatus = $derived(connectionsState.github);
</script>

{#if m}
  <div
    class="modal-backdrop"
    onclick={(e) => { if (e.target === e.currentTarget && !m.busy) closeModal('comment'); }}
    onkeydown={(e) => { if (e.key === 'Escape' && !m.busy) closeModal('comment'); }}
    role="dialog"
    aria-modal="true"
    tabindex="-1"
  >
    <div class="modal modal-wide">
      <header class="modal-head">
        <svg class="i" viewBox="0 0 24 24" style="color: var(--text-1)"><path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z" /></svg>
        <div>
          <div class="modal-title">Add comment</div>
          <div class="modal-sub">posts as @{githubStatus.kind === 'connected' ? githubStatus.user.login : ''}</div>
        </div>
        <button class="modal-close" onclick={() => closeModal('comment')} disabled={m.busy} aria-label="Close">
          <svg class="i" viewBox="0 0 24 24"><path d="M18 6 6 18M6 6l12 12" /></svg>
        </button>
      </header>
      <div class="modal-body">
        <!-- svelte-ignore a11y_autofocus -->
        <textarea
          class="field-textarea"
          value={m.body}
          oninput={(e) => patchModal('comment', { body: (e.currentTarget as HTMLTextAreaElement).value })}
          placeholder="Write your comment (markdown supported)…"
          autofocus
          disabled={m.busy}
        ></textarea>
        {#if m.error}<div class="modal-error">{m.error}</div>{/if}
      </div>
      <footer class="modal-foot">
        <button class="btn btn--ghost" onclick={() => closeModal('comment')} disabled={m.busy}>Cancel</button>
        <button class="btn btn--primary" onclick={() => void onSubmit()} disabled={m.busy || !m.body.trim()}>
          {m.busy ? 'Posting…' : 'Comment'}
        </button>
      </footer>
    </div>
  </div>
{/if}
