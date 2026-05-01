<script lang="ts">
  import { modalsState, closeModal, patchModal } from '$lib/state/modals.svelte';
  import { focusTrap } from '$lib/actions/focusTrap';

  interface Props {
    /** Open the user's browser to the relevant token-creation page. */
    openBrowser: (url: string) => void | Promise<void>;
    /** Compose the URL for the source's token settings page. */
    githubTokenUrl: () => string;
    /** Validate + persist the token (used by the Connect button + Enter). */
    onSubmit: () => Promise<void> | void;
  }
  let { openBrowser, githubTokenUrl, onSubmit }: Props = $props();

  const m = $derived(modalsState.pat);
</script>

{#if m}
  <div
    class="modal-backdrop"
    onclick={(e) => { if (e.target === e.currentTarget && !m.busy) closeModal('pat'); }}
    onkeydown={(e) => { if (e.key === 'Escape' && !m.busy) closeModal('pat'); }}
    role="dialog"
    aria-modal="true"
    tabindex="-1"
    use:focusTrap
  >
    <div class="modal">
      <header class="modal-head">
        <span class="conn-icon {m.conn.iconClass}" class:conn-icon--svg={!!m.conn.iconSvg}>
          {#if m.conn.iconSvg}
            <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">{@html m.conn.iconSvg}</svg>
          {:else}
            {m.conn.iconLetters}
          {/if}
        </span>
        <div>
          <div class="modal-title">Connect {m.conn.name}</div>
          <div class="modal-sub">Personal access token — stored in macOS Keychain</div>
        </div>
        <button class="modal-close" onclick={() => closeModal('pat')} disabled={m.busy} aria-label="Close">
          <svg class="i" viewBox="0 0 24 24"><path d="M18 6 6 18M6 6l12 12" /></svg>
        </button>
      </header>
      <div class="modal-body">
        {#if m.conn.id === 'github'}
          <p class="modal-copy">
            Classic personal access token with scopes <code class="mono">repo</code>,
            <code class="mono">read:user</code>, <code class="mono">read:org</code>. The link
            below pre-fills these. Fine-grained tokens also work — give them
            Pull requests / Issues / Contents / Metadata read+write on the repos
            you care about. Token is stored in your macOS Keychain, not on disk.
          </p>
          <button class="link-btn" onclick={() => openBrowser(githubTokenUrl())}>
            <svg class="i i-sm" viewBox="0 0 24 24"><path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"/><path d="M15 3h6v6M10 14 21 3"/></svg>
            Open token page on GitHub
          </button>
        {/if}
        <label class="field">
          <span class="field-label">Token</span>
          <!-- svelte-ignore a11y_autofocus -->
          <input
            class="field-input mono"
            type="password"
            value={m.token}
            oninput={(e) => patchModal('pat', { token: (e.currentTarget as HTMLInputElement).value })}
            placeholder="ghp_…"
            autofocus
            disabled={m.busy}
            onkeydown={(e) => { if (e.key === 'Enter' && !m.busy && m.token.trim()) void onSubmit(); }}
          />
        </label>
        {#if m.error}<div class="modal-error">{m.error}</div>{/if}
      </div>
      <footer class="modal-foot">
        <button class="btn btn--ghost" onclick={() => closeModal('pat')} disabled={m.busy}>Cancel</button>
        <button class="btn btn--primary" onclick={() => void onSubmit()} disabled={m.busy || !m.token.trim()}>
          {m.busy ? 'Verifying…' : 'Connect'}
        </button>
      </footer>
    </div>
  </div>
{/if}
