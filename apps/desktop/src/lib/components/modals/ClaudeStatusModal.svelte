<script lang="ts">
  import { modalsState, closeModal } from '$lib/state/modals.svelte';
  import { connectionsMeta } from '$lib/data';

  interface Props {
    openBrowser: (url: string) => void | Promise<void>;
    claudeInstallUrl: () => string;
    onRefresh: () => Promise<void> | void;
  }
  let { openBrowser, claudeInstallUrl, onRefresh }: Props = $props();

  const m = $derived(modalsState.claudeStatus);
  const claudeMeta = connectionsMeta.find((c) => c.id === 'claude')!;

  /* Per-cmd copy state. Keyed by the command string itself so the
     "Copied" flash on one command doesn't bleed into another. */
  let copiedKey = $state<string | null>(null);
  let copyTimer: ReturnType<typeof setTimeout> | null = null;
  async function copy(cmd: string) {
    try {
      await navigator.clipboard.writeText(cmd);
      copiedKey = cmd;
      if (copyTimer) clearTimeout(copyTimer);
      copyTimer = setTimeout(() => (copiedKey = null), 1400);
    } catch {/* ignore */}
  }
</script>

{#if m}
  {@const s = m.status}
  <div
    class="modal-backdrop"
    onclick={(e) => { if (e.target === e.currentTarget && !m.loading) closeModal('claudeStatus'); }}
    onkeydown={(e) => { if (e.key === 'Escape' && !m.loading) closeModal('claudeStatus'); }}
    role="dialog"
    aria-modal="true"
    tabindex="-1"
  >
    <div class="modal modal-wide">
      <header class="modal-head">
        <span class="conn-icon conn-icon--claude conn-icon--img">
          <img src={claudeMeta.iconImg} alt="" class="conn-icon-img" />
        </span>
        <div>
          <div class="modal-title">Claude Code</div>
          <div class="modal-sub">Authentication managed by the <code class="mono">claude</code> CLI</div>
        </div>
        <button class="modal-close" onclick={() => closeModal('claudeStatus')} disabled={m.loading} aria-label="Close">
          <svg class="i" viewBox="0 0 24 24"><path d="M18 6 6 18M6 6l12 12" /></svg>
        </button>
      </header>
      <div class="modal-body">
        {#if m.loading || !s}
          <div class="tab-state">Detecting…</div>
        {:else}
          <div class="claude-detect">
            <div class="detect-row" class:ok={s.detected}>
              <span class="detect-dot"></span>
              <div class="detect-main">
                <div class="detect-title">claude CLI</div>
                <div class="detect-sub mono">
                  {#if s.detected}
                    {s.path}{#if s.version} · {s.version}{/if}
                  {:else}
                    not found on PATH
                  {/if}
                </div>
              </div>
            </div>
            <div class="detect-row" class:ok={s.has_config_dir || s.has_api_key_env}>
              <span class="detect-dot"></span>
              <div class="detect-main">
                <div class="detect-title">Authentication</div>
                <div class="detect-sub">
                  {#if s.has_api_key_env}
                    <code class="mono">ANTHROPIC_API_KEY</code> env var set — using API key billing
                  {:else if s.has_config_dir}
                    <code class="mono">~/.claude</code> exists — signed in via subscription (Claude Max / Pro)
                  {:else}
                    not authenticated yet
                  {/if}
                </div>
              </div>
            </div>
          </div>

          {#if !s.detected}
            {@const installCmd = 'curl -fsSL https://claude.ai/install.sh | bash'}
            <div class="claude-hint">
              <div class="claude-hint-title">Install the CLI first</div>
              <div class="claude-hint-body">
                <p>Run this once in your terminal:</p>
                <div class="cmd-line">
                  <code class="cmd-text">{installCmd}</code>
                  <button class="cmd-copy" class:copied={copiedKey === installCmd} onclick={() => void copy(installCmd)}>
                    {#if copiedKey === installCmd}
                      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><path d="M20 6 9 17l-5-5"/></svg>
                      Copied
                    {:else}
                      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><rect x="9" y="9" width="11" height="11" rx="2"/><path d="M5 15V5a2 2 0 0 1 2-2h10"/></svg>
                      Copy
                    {/if}
                  </button>
                </div>
                <button class="link-btn" onclick={() => openBrowser(claudeInstallUrl())}>
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"/><path d="M15 3h6v6M10 14 21 3"/></svg>
                  Claude Code documentation
                </button>
              </div>
            </div>
          {:else if !s.ready}
            {@const loginCmd = 'claude login'}
            <div class="claude-hint">
              <div class="claude-hint-title">Sign in to your Claude subscription</div>
              <div class="claude-hint-body">
                <p>Run this once — it opens a browser, you sign in with your Max / Pro plan, done.</p>
                <div class="cmd-line">
                  <code class="cmd-text">{loginCmd}</code>
                  <button class="cmd-copy" class:copied={copiedKey === loginCmd} onclick={() => void copy(loginCmd)}>
                    {#if copiedKey === loginCmd}
                      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><path d="M20 6 9 17l-5-5"/></svg>
                      Copied
                    {:else}
                      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><rect x="9" y="9" width="11" height="11" rx="2"/><path d="M5 15V5a2 2 0 0 1 2-2h10"/></svg>
                      Copy
                    {/if}
                  </button>
                </div>
                <p style="color: var(--text-2);">
                  Prefer to bill via the API instead? Export <code class="mono">ANTHROPIC_API_KEY</code>.
                </p>
              </div>
            </div>
          {:else}
            <div class="claude-ok">
              <svg class="i" viewBox="0 0 24 24" style="width: 22px; height: 22px;" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><path d="M20 6 9 17l-5-5"/></svg>
              <div>
                <div class="detect-title">Ready to run agents.</div>
                <div class="detect-sub">Forgehold will use this CLI for Claude Code runs. Re-check any time.</div>
              </div>
            </div>
          {/if}
        {/if}
      </div>
      <footer class="modal-foot">
        <button class="btn btn--ghost" onclick={() => void onRefresh()} disabled={m.loading}>
          {m.loading ? 'Checking…' : 'Re-check'}
        </button>
        <div style="flex:1"></div>
        <button class="btn btn--primary" onclick={() => closeModal('claudeStatus')} disabled={m.loading}>Close</button>
      </footer>
    </div>
  </div>
{/if}
