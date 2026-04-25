<script lang="ts">
  import { modalsState, closeModal, patchModal } from '$lib/state/modals.svelte';

  interface Props {
    openBrowser: (url: string) => void | Promise<void>;
    jiraTokenUrl: () => string;
    onSubmit: () => Promise<void> | void;
  }
  let { openBrowser, jiraTokenUrl, onSubmit }: Props = $props();

  const m = $derived(modalsState.jiraConnect);

  function canSubmit(): boolean {
    return !!m && !m.busy && m.workspace.trim().length > 0 && m.email.trim().length > 0 && m.token.trim().length > 0;
  }
</script>

{#if m}
  <div
    class="modal-backdrop"
    onclick={(e) => { if (e.target === e.currentTarget && !m.busy) closeModal('jiraConnect'); }}
    onkeydown={(e) => { if (e.key === 'Escape' && !m.busy) closeModal('jiraConnect'); }}
    role="dialog"
    aria-modal="true"
    tabindex="-1"
  >
    <div class="modal modal-wide">
      <header class="modal-head">
        <span class="conn-icon conn-icon--jira">J</span>
        <div>
          <div class="modal-title">Connect Jira</div>
          <div class="modal-sub">Atlassian Cloud · email + API token</div>
        </div>
        <button class="modal-close" onclick={() => closeModal('jiraConnect')} disabled={m.busy} aria-label="Close">
          <svg class="i" viewBox="0 0 24 24"><path d="M18 6 6 18M6 6l12 12" /></svg>
        </button>
      </header>
      <div class="modal-body">
        <p class="modal-copy">
          Basic auth with an Atlassian API token. The token inherits your account's
          permissions — you'll need <em>browse projects</em> for reads, plus <em>edit issues</em>
          / <em>transition issues</em> / <em>add comments</em> on the projects you want Claude
          to mutate. Token is stored in your macOS Keychain, not on disk.
        </p>
        <button class="link-btn" onclick={() => openBrowser(jiraTokenUrl())}>
          <svg class="i i-sm" viewBox="0 0 24 24"><path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"/><path d="M15 3h6v6M10 14 21 3"/></svg>
          Create an API token on id.atlassian.com
        </button>

        <label class="field">
          <span class="field-label">Workspace</span>
          <!-- svelte-ignore a11y_autofocus -->
          <input
            class="field-input mono"
            type="text"
            value={m.workspace}
            oninput={(e) => patchModal('jiraConnect', { workspace: (e.currentTarget as HTMLInputElement).value })}
            placeholder="acme.atlassian.net"
            autofocus
            disabled={m.busy}
          />
        </label>
        <label class="field">
          <span class="field-label">Email</span>
          <input
            class="field-input"
            type="email"
            value={m.email}
            oninput={(e) => patchModal('jiraConnect', { email: (e.currentTarget as HTMLInputElement).value })}
            placeholder="you@acme.com"
            disabled={m.busy}
          />
        </label>
        <label class="field">
          <span class="field-label">API Token</span>
          <input
            class="field-input mono"
            type="password"
            value={m.token}
            oninput={(e) => patchModal('jiraConnect', { token: (e.currentTarget as HTMLInputElement).value })}
            placeholder="ATATT3x…"
            disabled={m.busy}
            onkeydown={(e) => { if (e.key === 'Enter' && canSubmit()) void onSubmit(); }}
          />
        </label>

        {#if m.error}<div class="modal-error">{m.error}</div>{/if}
      </div>
      <footer class="modal-foot">
        <button class="btn btn--ghost" onclick={() => closeModal('jiraConnect')} disabled={m.busy}>Cancel</button>
        <button class="btn btn--primary" onclick={() => void onSubmit()} disabled={!canSubmit()}>
          {m.busy ? 'Verifying…' : 'Connect'}
        </button>
      </footer>
    </div>
  </div>
{/if}
