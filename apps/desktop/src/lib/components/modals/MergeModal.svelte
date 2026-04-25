<script lang="ts">
  import { externalId } from '$lib/data';
  import { inboxState } from '$lib/state/inbox.svelte';
  import { modalsState, closeModal, patchModal, type MergeMethod } from '$lib/state/modals.svelte';

  interface Props {
    onSubmit: () => Promise<void> | void;
  }
  let { onSubmit }: Props = $props();

  const m = $derived(modalsState.merge);

  const METHODS: { value: MergeMethod; title: string; desc: string }[] = [
    { value: 'squash', title: 'Squash and merge', desc: 'All commits combined into one' },
    { value: 'merge', title: 'Create a merge commit', desc: 'All commits added with a merge commit' },
    { value: 'rebase', title: 'Rebase and merge', desc: 'Apply commits on top of base' }
  ];
</script>

{#if m}
  <div
    class="modal-backdrop"
    onclick={(e) => { if (e.target === e.currentTarget && !m.busy) closeModal('merge'); }}
    onkeydown={(e) => { if (e.key === 'Escape' && !m.busy) closeModal('merge'); }}
    role="dialog"
    aria-modal="true"
    tabindex="-1"
  >
    <div class="modal">
      <header class="modal-head">
        <svg class="i" viewBox="0 0 24 24" style="color: var(--accent-bright)">
          <circle cx="18" cy="18" r="3" /><circle cx="6" cy="6" r="3" />
          <path d="M6 9v6a6 6 0 0 0 6 6h2" />
        </svg>
        <div>
          <div class="modal-title">Merge pull request</div>
          <div class="modal-sub">
            {inboxState.focusItem ? externalId(inboxState.focusItem) : ''}{inboxState.prDetail ? ` · ${inboxState.prDetail.base_ref} ← ${inboxState.prDetail.head_ref}` : ''}
          </div>
        </div>
        <button class="modal-close" onclick={() => closeModal('merge')} disabled={m.busy} aria-label="Close">
          <svg class="i" viewBox="0 0 24 24"><path d="M18 6 6 18M6 6l12 12" /></svg>
        </button>
      </header>
      <div class="modal-body">
        <div class="radio-group">
          {#each METHODS as opt (opt.value)}
            <label class="radio" class:selected={m.method === opt.value}>
              <input
                type="radio"
                name="merge-method"
                checked={m.method === opt.value}
                onchange={() => patchModal('merge', { method: opt.value })}
                disabled={m.busy}
              />
              <div>
                <div class="radio-title">{opt.title}</div>
                <div class="radio-desc">{opt.desc}</div>
              </div>
            </label>
          {/each}
        </div>
        {#if m.error}<div class="modal-error">{m.error}</div>{/if}
      </div>
      <footer class="modal-foot">
        <button class="btn btn--ghost" onclick={() => closeModal('merge')} disabled={m.busy}>Cancel</button>
        <button class="btn btn--primary" onclick={() => void onSubmit()} disabled={m.busy}>
          {m.busy ? 'Merging…' : 'Confirm merge'}
        </button>
      </footer>
    </div>
  </div>
{/if}
