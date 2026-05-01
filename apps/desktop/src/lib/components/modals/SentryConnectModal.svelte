<script lang="ts">
  import { modalsState, closeModal, patchModal } from '$lib/state/modals.svelte';
  import { focusTrap } from '$lib/actions/focusTrap';

  interface Props {
    openBrowser: (url: string) => void | Promise<void>;
    /** Resolve the host's auth-token settings page so the user can mint
     *  one in two clicks. */
    sentryTokenUrl: () => string;
    onSubmit: () => Promise<void> | void;
  }
  let { openBrowser, sentryTokenUrl, onSubmit }: Props = $props();

  const m = $derived(modalsState.sentryConnect);

  function canSubmit(): boolean {
    return !!m && !m.busy && m.organization_slug.trim().length > 0 && m.token.trim().length > 0;
  }
</script>

{#if m}
  <div
    class="modal-backdrop"
    onclick={(e) => { if (e.target === e.currentTarget && !m.busy) closeModal('sentryConnect'); }}
    onkeydown={(e) => { if (e.key === 'Escape' && !m.busy) closeModal('sentryConnect'); }}
    role="dialog"
    aria-modal="true"
    tabindex="-1"
    use:focusTrap
  >
    <div class="modal modal-wide">
      <header class="modal-head">
        <span class="conn-icon conn-icon--sentry">St</span>
        <div>
          <div class="modal-title">Connect Sentry</div>
          <div class="modal-sub">Cloud or self-hosted · Auth Token</div>
        </div>
        <button class="modal-close" onclick={() => closeModal('sentryConnect')} disabled={m.busy} aria-label="Close">
          <svg class="i" viewBox="0 0 24 24"><path d="M18 6 6 18M6 6l12 12" /></svg>
        </button>
      </header>
      <div class="modal-body">
        <p class="modal-copy">
          Mint a User Auth Token at <code class="mono">&lt;host&gt;/settings/account/api/auth-tokens/</code> with scopes
          <code class="mono">org:read</code>, <code class="mono">project:read</code>, <code class="mono">event:read</code>,
          <code class="mono">event:write</code> (the last is required so Claude can resolve / ignore / comment on issues).
          Self-hosted works too — point Host at your install. Token is stored in macOS Keychain, not on disk.
        </p>
        <button class="link-btn" onclick={() => openBrowser(sentryTokenUrl())}>
          <svg class="i i-sm" viewBox="0 0 24 24"><path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"/><path d="M15 3h6v6M10 14 21 3"/></svg>
          Open auth-tokens page
        </button>

        <label class="field">
          <span class="field-label">Host</span>
          <!-- svelte-ignore a11y_autofocus -->
          <input
            class="field-input mono"
            type="text"
            value={m.host}
            oninput={(e) => patchModal('sentryConnect', { host: (e.currentTarget as HTMLInputElement).value })}
            placeholder="https://sentry.io"
            autofocus
            disabled={m.busy}
          />
        </label>
        <label class="field">
          <span class="field-label">Organization slug</span>
          <input
            class="field-input mono"
            type="text"
            value={m.organization_slug}
            oninput={(e) => patchModal('sentryConnect', { organization_slug: (e.currentTarget as HTMLInputElement).value })}
            placeholder="acme-co"
            disabled={m.busy}
          />
        </label>
        <label class="field">
          <span class="field-label">Auth Token</span>
          <input
            class="field-input mono"
            type="password"
            value={m.token}
            oninput={(e) => patchModal('sentryConnect', { token: (e.currentTarget as HTMLInputElement).value })}
            placeholder="sntryu_…"
            disabled={m.busy}
            onkeydown={(e) => { if (e.key === 'Enter' && canSubmit()) void onSubmit(); }}
          />
        </label>

        {#if m.error}<div class="modal-error">{m.error}</div>{/if}
      </div>
      <footer class="modal-foot">
        <button class="btn btn--ghost" onclick={() => closeModal('sentryConnect')} disabled={m.busy}>Cancel</button>
        <button class="btn btn--primary" onclick={() => void onSubmit()} disabled={!canSubmit()}>
          {m.busy ? 'Verifying…' : 'Connect'}
        </button>
      </footer>
    </div>
  </div>
{/if}
