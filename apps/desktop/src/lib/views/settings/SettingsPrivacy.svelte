<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { getVersion } from '@tauri-apps/api/app';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import { notify, notifyError } from '$lib/state/toaster.svelte';
  import { buildBugReport, bugReportGithubIssueUrl } from '$lib/services/bugReport';

  // ---- Crash-report opt-out --------------------------------------------

  let telemetryOptOut = $state(false);
  let telemetryBusy = $state(false);

  async function loadTelemetryPref() {
    try {
      telemetryOptOut = await invoke<boolean>('get_telemetry_opt_out');
    } catch {
      telemetryOptOut = false;
    }
  }

  async function toggleTelemetry(next: boolean) {
    telemetryBusy = true;
    try {
      await invoke('set_telemetry_opt_out', { optOut: next });
      telemetryOptOut = next;
      notify({
        kind: 'success',
        title: next ? 'Crash reports disabled' : 'Crash reports enabled',
        body: 'Takes effect on next app launch.'
      });
    } catch (e) {
      notifyError(e, { title: 'Toggle failed' });
    } finally {
      telemetryBusy = false;
    }
  }

  $effect(() => {
    void loadTelemetryPref();
  });

  // ---- Bug report ------------------------------------------------------

  const BUG_REPORT_GITHUB_REPO = '';

  let bugReportDescription = $state('');
  let bugReportPreview = $state<string | null>(null);
  let bugReportCopiedAt = $state<number | null>(null);

  let appVersionLabel = $state('Woom');
  onMount(async () => {
    try {
      appVersionLabel = `Woom ${await getVersion()}`;
    } catch {
      /* leave fallback */
    }
  });

  function refreshBugReportPreview() {
    bugReportPreview = buildBugReport({
      description: bugReportDescription,
      appVersion: appVersionLabel
    });
  }

  async function copyBugReport() {
    refreshBugReportPreview();
    if (!bugReportPreview) return;
    try {
      await navigator.clipboard.writeText(bugReportPreview);
      bugReportCopiedAt = Date.now();
      notify({ kind: 'success', title: 'Bug report copied', body: 'Paste it into an issue or email.' });
    } catch (e) {
      notifyError(e, { title: 'Copy failed' });
    }
  }

  function openBugReportIssue() {
    refreshBugReportPreview();
    const url =
      bugReportPreview && BUG_REPORT_GITHUB_REPO
        ? bugReportGithubIssueUrl(bugReportPreview, BUG_REPORT_GITHUB_REPO)
        : null;
    if (!url) {
      notify({ kind: 'info', title: 'No issue tracker configured', body: 'Copy the bundle and paste it into your tracker manually.' });
      return;
    }
    void openUrl(url);
  }
</script>

<!-- Privacy / crash reports -->
<div class="card">
  <header class="card-head">
    <h2 class="card-title">Privacy</h2>
    <p class="card-sub">
      Crash reports help us spot panics that wouldn’t otherwise reach you. Reports never include token bodies or chat content; user can opt out at any time.
    </p>
  </header>
  <label class="telemetry-row">
    <input
      type="checkbox"
      checked={telemetryOptOut}
      disabled={telemetryBusy}
      onchange={(e) => void toggleTelemetry((e.currentTarget as HTMLInputElement).checked)}
    />
    <span class="telemetry-row-label">Opt out of crash reports</span>
    <span class="telemetry-row-hint">Takes effect on next app launch.</span>
  </label>
</div>

<!-- Report a bug -->
<div class="card">
  <header class="card-head">
    <h2 class="card-title">Report a bug</h2>
    <p class="card-sub">
      Bundles your description with current connection state, the last 50 status events, and a snapshot of your layout. Generated locally — nothing leaves the machine until you copy or open it yourself.
    </p>
  </header>
  <textarea
    class="bug-report-textarea"
    rows="6"
    placeholder="What happened? What did you expect to happen?"
    bind:value={bugReportDescription}
    oninput={refreshBugReportPreview}
  ></textarea>
  <div class="update-actions">
    <button class="btn btn--ghost" onclick={copyBugReport}>
      {bugReportCopiedAt && Date.now() - bugReportCopiedAt < 4000 ? 'Copied!' : 'Copy bundle to clipboard'}
    </button>
    <button class="btn btn--primary" onclick={openBugReportIssue} disabled={!BUG_REPORT_GITHUB_REPO}>
      Open GitHub issue
    </button>
  </div>
  {#if bugReportPreview}
    <details class="bug-report-preview">
      <summary>Preview bundle</summary>
      <pre class="bug-report-preview-pre">{bugReportPreview}</pre>
    </details>
  {/if}
</div>
