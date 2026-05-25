<script lang="ts">
  import { onMount } from 'svelte';
  import { getVersion } from '@tauri-apps/api/app';
  import {
    updateState as updatesStore,
    checkNow as updatesCheckNow,
    installNow as updatesInstallNow,
    setAutoCheck as updatesSetAutoCheck,
    clearSkip as updatesClearSkip,
  } from '$lib/state/updates.svelte';
  import { notifyError } from '$lib/state/toaster.svelte';
  import { relativeTime } from '$lib/data';

  let appVersionLabel = $state('Woom');
  onMount(async () => {
    try {
      appVersionLabel = `Woom ${await getVersion()}`;
    } catch {
      /* leave fallback */
    }
  });

  const updateCheckBusy = $derived(updatesStore.phase.kind === 'checking');
  const updateInstallBusy = $derived(
    updatesStore.phase.kind === 'downloading' ||
      updatesStore.phase.kind === 'verifying' ||
      updatesStore.phase.kind === 'installing'
  );
  const updateLastCheckedAt = $derived(
    updatesStore.settings.last_checked_at_ms
      ? new Date(updatesStore.settings.last_checked_at_ms).toISOString()
      : null
  );
  const updateAvailable = $derived.by(() => {
    const p = updatesStore.phase;
    if (p.kind !== 'available') return null;
    return { version: p.version, body: p.notes };
  });
  const updateInstalledOk = $derived(updatesStore.phase.kind === 'installed_pending_quit');
  const updateDownloadProgress = $derived.by(() => {
    const p = updatesStore.phase;
    if (p.kind !== 'downloading') return null;
    if (!p.total || p.total <= 0) return null;
    return Math.min(1, p.downloaded / p.total);
  });
  const updateDownloadTotal = $derived(
    updatesStore.phase.kind === 'downloading' ? updatesStore.phase.total : null
  );
  const updateStatusMessage = $derived.by(() => {
    const p = updatesStore.phase;
    switch (p.kind) {
      case 'idle': return null;
      case 'checking': return 'Checking for updates…';
      case 'up_to_date': return 'You’re on the latest version.';
      case 'available': return `Update ${p.version} ready to install.`;
      case 'snoozed': return `Update ${p.version} snoozed.`;
      case 'skipped': return `Update ${p.version} skipped.`;
      case 'downloading': return p.total
        ? `Downloading ${p.version}… ${Math.round((p.downloaded / p.total) * 100)}%`
        : `Downloading ${p.version}…`;
      case 'verifying': return `Verifying ${p.version}…`;
      case 'installing': return `Installing ${p.version}…`;
      case 'installed_pending_quit':
        return 'Update installed. Quit and reopen Woom to finish.';
      case 'failed': return `Couldn’t check for updates: ${p.reason}`;
    }
  });

  async function checkForUpdates() {
    try { await updatesCheckNow(); }
    catch (e) { notifyError(e, { title: 'Update check failed' }); }
  }

  async function installUpdate() {
    if (updatesStore.phase.kind !== 'available') return;
    try { await updatesInstallNow(); }
    catch (e) { notifyError(e, { title: 'Install failed' }); }
  }

  async function toggleAutoCheck(enabled: boolean) {
    try { await updatesSetAutoCheck(enabled); }
    catch (e) { notifyError(e, { title: 'Settings update failed' }); }
  }

  async function clearSkippedVersion() {
    try { await updatesClearSkip(); }
    catch (e) { notifyError(e, { title: 'Clear skip failed' }); }
  }

  function fmtSize(bytes: number): string {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`;
    return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
  }
</script>

<!-- Updates -->
<div class="card">
  <header class="card-head">
    <h2 class="card-title">Updates</h2>
    <p class="card-sub">
      Auto-update channel for new Woom builds. Releases pull from the public manifest configured in <span class="mono">tauri.conf.json</span> and are verified against an ed25519 public key.
    </p>
  </header>
  <div class="grid">
    <div class="stat">
      <div class="stat-label">Current version</div>
      <div class="stat-value mono">{appVersionLabel}</div>
    </div>
    <div class="stat">
      <div class="stat-label">Last checked</div>
      <div class="stat-value mono">
        {updateLastCheckedAt ? relativeTime(updateLastCheckedAt) : 'never'}
      </div>
    </div>
  </div>
  <div class="update-actions">
    <button
      class="btn btn--ghost"
      onclick={checkForUpdates}
      disabled={updateCheckBusy || updateInstallBusy}
    >
      {updateCheckBusy ? 'Checking…' : 'Check for updates'}
    </button>
    <label class="update-toggle">
      <input
        type="checkbox"
        checked={updatesStore.settings.auto_check_enabled}
        onchange={(e) => void toggleAutoCheck((e.currentTarget as HTMLInputElement).checked)}
      />
      Auto-check on launch + every 6h
    </label>
  </div>
  {#if updatesStore.settings.skipped_version || updatesStore.phase.kind === 'skipped'}
    <div class="update-skip-row">
      <span class="update-skip-label">Skipped version</span>
      <span class="mono">{updatesStore.settings.skipped_version ?? (updatesStore.phase.kind === 'skipped' ? updatesStore.phase.version : '')}</span>
      <button class="btn-link" onclick={clearSkippedVersion}>clear skip & re-check</button>
    </div>
  {/if}
  {#if updateStatusMessage}
    <div class="update-status" class:update-status--ready={updateAvailable !== null} class:update-status--installed={updateInstalledOk}>
      {updateStatusMessage}
    </div>
  {/if}
  {#if updateAvailable && !updateInstalledOk}
    <div class="update-detail">
      <div class="update-version mono">v{updateAvailable.version}</div>
      {#if updateAvailable.body}
        <pre class="update-notes">{updateAvailable.body}</pre>
      {/if}
      <div class="update-actions">
        <button
          class="btn btn--primary"
          onclick={installUpdate}
          disabled={updateInstallBusy}
        >
          {updateInstallBusy ? 'Installing…' : 'Download & install'}
        </button>
      </div>
      {#if updateDownloadProgress !== null}
        <div class="update-progress" role="progressbar" aria-valuemin="0" aria-valuemax="100"
             aria-valuenow={Math.round(updateDownloadProgress * 100)}>
          <div class="update-progress-bar" style="width: {Math.round(updateDownloadProgress * 100)}%"></div>
          <div class="update-progress-label mono">
            {updateDownloadTotal ? `${fmtSize(Math.round(updateDownloadProgress * updateDownloadTotal))} / ${fmtSize(updateDownloadTotal)}` : `${Math.round(updateDownloadProgress * 100)}%`}
          </div>
        </div>
      {/if}
    </div>
  {/if}
</div>
