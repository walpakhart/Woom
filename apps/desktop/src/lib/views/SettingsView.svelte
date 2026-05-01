<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { openPath, openUrl } from '@tauri-apps/plugin-opener';
  import { check, type Update } from '@tauri-apps/plugin-updater';
  import { marked } from 'marked';
  import { buildBugReport, bugReportGithubIssueUrl } from '$lib/services/bugReport';
  import { resetWelcome, welcomeState } from '$lib/state/welcome.svelte';
  import { sessionsState, persistError, SESSIONS_STORAGE_KEY, RULES_STORAGE_KEY } from '$lib/state/sessions.svelte';
  import { layoutState } from '$lib/state/layout.svelte';
  import { notify, notifyError } from '$lib/state/toaster.svelte';
  import { themeState, applyTheme, type ThemeName } from '$lib/state/theme.svelte';
  import { scaleState, applyScale, SCALE_OPTIONS } from '$lib/state/scale.svelte';
  import {
    connectionEventsState,
    clearConnectionEvents,
    type ConnectionEvent,
    type ConnectionEventSource
  } from '$lib/state/connectionEvents.svelte';
  import { relativeTime } from '$lib/data';

  /* Theme picker. Each entry encodes a tiny preview swatch (bg, text,
     accent) so the user can eyeball the palette without applying. */
  const THEMES: { name: ThemeName; label: string; sub: string; bg: string; fg: string; accent: string }[] = [
    { name: 'iconic', label: 'Iconic', sub: 'Molten gold on graphite', bg: '#0C1117', fg: '#EDE5D1', accent: '#E8A33A' },
    { name: 'light',  label: 'Light',  sub: 'Tint cream + Shade chocolate', bg: '#FAEEE0', fg: '#2A1208', accent: '#4E2812' },
    { name: 'dark',   label: 'Dark',   sub: 'Shade chocolate + Tint cream', bg: '#1A0E07', fg: '#FAEEE0', accent: '#FAEEE0' }
  ];

  /** 14 days in seconds — matches the SPEC §worktrees retention rule. */
  const ORPHAN_AGE_SECS = 14 * 24 * 60 * 60;

  // ---- Live counters (synchronously derived from existing stores) -------

  const claudeCount = $derived(sessionsState.list.filter((s) => s.agentKind === 'claude').length);
  const cursorCount = $derived(sessionsState.list.filter((s) => s.agentKind === 'cursor').length);
  const totalMessages = $derived(
    sessionsState.list.reduce((acc, s) => acc + s.messages.length, 0)
  );
  const workbenchCount = $derived(layoutState.workbenches.length);
  const totalColumns = $derived(
    layoutState.workbenches.reduce((acc, w) => acc + w.instances.length, 0)
  );

  // localStorage usage estimate. UTF-16 chars × 2 bytes is the conservative
  // upper bound; what the browser actually charges depends on the engine,
  // but this matches user-facing disk-usage charts in DevTools.
  const lsBytes = $derived.by(() => {
    if (typeof localStorage === 'undefined') return 0;
    let total = 0;
    for (let i = 0; i < localStorage.length; i++) {
      const key = localStorage.key(i);
      if (!key) continue;
      const v = localStorage.getItem(key) ?? '';
      total += (key.length + v.length) * 2;
    }
    return total;
  });
  const sessionsBytes = $derived.by(() => {
    if (typeof localStorage === 'undefined') return 0;
    const v = localStorage.getItem(SESSIONS_STORAGE_KEY) ?? '';
    return (SESSIONS_STORAGE_KEY.length + v.length) * 2;
  });

  // ---- Worktree stats — Tauri-driven, refreshed on demand --------------

  let worktreeBytes = $state<number | null>(null);
  let worktreeDir = $state<string | null>(null);
  let worktreeStatLoading = $state(false);
  let cleanupBusy = $state(false);

  async function refreshDiskStats() {
    worktreeStatLoading = true;
    try {
      const [b, d] = await Promise.all([
        invoke<number>('worktree_disk_usage'),
        invoke<string | null>('worktree_storage_dir')
      ]);
      worktreeBytes = b;
      worktreeDir = d;
    } catch (e) {
      notifyError(e, { title: 'Disk-usage probe failed' });
    } finally {
      worktreeStatLoading = false;
    }
  }

  async function runCleanup() {
    if (cleanupBusy) return;
    cleanupBusy = true;
    try {
      const activeIds = sessionsState.list.map((s) => s.id);
      const summary = await invoke<{
        removed: number;
        bytes_freed: number;
        kept: number;
        failed: string[];
      }>('worktree_cleanup_orphans', {
        activeSessionIds: activeIds,
        maxAgeSecs: ORPHAN_AGE_SECS
      });
      if (summary.removed === 0 && summary.failed.length === 0) {
        notify({
          kind: 'info',
          title: 'Nothing to clean',
          body: `${summary.kept} worktree${summary.kept === 1 ? '' : 's'} on disk, all linked to live chats or younger than 14 days.`
        });
      } else {
        notify({
          kind: 'success',
          title: `Removed ${summary.removed} orphan worktree${summary.removed === 1 ? '' : 's'}`,
          body: `${formatBytes(summary.bytes_freed)} freed${summary.failed.length ? ` · ${summary.failed.length} failed` : ''}`
        });
      }
      await refreshDiskStats();
    } catch (e) {
      notifyError(e, { title: 'Cleanup failed' });
    } finally {
      cleanupBusy = false;
    }
  }

  async function openWorktreeDir() {
    if (!worktreeDir) return;
    try {
      /* `openPath` takes a raw path — no `file://` prefix, no URL
         encoding required. Previous `openUrl(`file://${worktreeDir}`)`
         broke on macOS because `Application Support` has a space in
         it, which `file://…` only accepts URL-encoded. */
      await openPath(worktreeDir);
    } catch (e) {
      notifyError(e, { title: 'Could not open folder' });
    }
  }

  function formatBytes(b: number): string {
    if (b < 1024) return `${b} B`;
    if (b < 1024 * 1024) return `${(b / 1024).toFixed(1)} KB`;
    if (b < 1024 * 1024 * 1024) return `${(b / 1024 / 1024).toFixed(1)} MB`;
    return `${(b / 1024 / 1024 / 1024).toFixed(2)} GB`;
  }

  // Probe on mount so the page lands populated.
  $effect(() => { void refreshDiskStats(); });

  // ---- Connection diagnostics ----------------------------------------

  /** Filter chip selection. `all` is sentinel for the unfiltered view. */
  let eventFilter = $state<ConnectionEventSource | 'all'>('all');

  const FILTER_TABS: ReadonlyArray<{ id: ConnectionEventSource | 'all'; label: string }> = [
    { id: 'all', label: 'All' },
    { id: 'github', label: 'GitHub' },
    { id: 'jira', label: 'Jira' },
    { id: 'sentry', label: 'Sentry' },
    { id: 'claude', label: 'Claude' },
    { id: 'cursor', label: 'Cursor' }
  ];

  const filteredEvents = $derived(
    eventFilter === 'all'
      ? connectionEventsState.events
      : connectionEventsState.events.filter((e) => e.source === eventFilter)
  );

  /** Per-source counts shown in the filter chips. Lets the user spot a
   *  noisy source ("Jira: 47") without scrolling the timeline. */
  const counts = $derived.by(() => {
    const c: Record<ConnectionEventSource, number> = {
      github: 0,
      jira: 0,
      sentry: 0,
      claude: 0,
      cursor: 0
    };
    for (const ev of connectionEventsState.events) c[ev.source]++;
    return c;
  });

  function eventLabel(ev: ConnectionEvent): string {
    switch (ev.kind) {
      case 'connected':
        return 'OK';
      case 'disconnected':
        return 'no token';
      case 'rate_limited':
        return 'rate-limited';
      case 'error':
        return 'error';
    }
  }

  function confirmClear() {
    if (connectionEventsState.events.length === 0) return;
    const ok = confirm(
      `Clear ${connectionEventsState.events.length} connection events? This is local diagnostics only — your tokens stay connected.`
    );
    if (ok) clearConnectionEvents();
  }

  /* ------------------------------------------------------------------ */
  /* Auto-updater (`docs/ROADMAP_1.0.md §1.3`).                         */
  /* The plugin reads its endpoints + pubkey from `tauri.conf.json     */
  /* > plugins > updater`. Until a real signing key is wired up, the   */
  /* manifest URL doesn't resolve and `check()` either returns null or */
  /* throws — both surfaced here as a friendly status.                 */
  /* ------------------------------------------------------------------ */

  let updateCheckBusy = $state(false);
  let updateInstallBusy = $state(false);
  let updateLastCheckedAt = $state<string | null>(null);
  let updateAvailable = $state<Update | null>(null);
  let updateStatusMessage = $state<string | null>(null);
  /* 0..1 progress when a download is in flight; null when idle. */
  let updateDownloadProgress = $state<number | null>(null);
  let updateDownloadTotal = $state<number | null>(null);
  let updateInstalledOk = $state(false);

  async function checkForUpdates() {
    updateCheckBusy = true;
    updateStatusMessage = null;
    try {
      const u = await check();
      updateLastCheckedAt = new Date().toISOString();
      if (u) {
        updateAvailable = u;
        updateStatusMessage = `Update ${u.version} ready to install.`;
      } else {
        updateAvailable = null;
        updateStatusMessage = 'You’re on the latest version.';
      }
    } catch (e) {
      /* The PLACEHOLDER pubkey + unreachable endpoint will land here
       * until a real signing key is wired up; surface as a hint
       * rather than a scary error. */
      updateAvailable = null;
      updateStatusMessage = `Couldn’t check for updates: ${
        typeof e === 'string' ? e : (e as Error).message ?? 'unknown error'
      }`;
    } finally {
      updateCheckBusy = false;
    }
  }

  async function installUpdate() {
    if (!updateAvailable) return;
    updateInstallBusy = true;
    updateDownloadProgress = 0;
    updateDownloadTotal = null;
    try {
      let downloaded = 0;
      await updateAvailable.downloadAndInstall((event) => {
        if (event.event === 'Started') {
          updateDownloadTotal =
            typeof event.data.contentLength === 'number'
              ? event.data.contentLength
              : null;
          downloaded = 0;
          updateDownloadProgress = 0;
        } else if (event.event === 'Progress') {
          downloaded += event.data.chunkLength;
          if (updateDownloadTotal && updateDownloadTotal > 0) {
            updateDownloadProgress = Math.min(1, downloaded / updateDownloadTotal);
          }
        } else if (event.event === 'Finished') {
          updateDownloadProgress = 1;
        }
      });
      updateInstalledOk = true;
      updateStatusMessage = 'Update installed. Quit and reopen Forgehold to finish.';
    } catch (e) {
      updateStatusMessage = `Install failed: ${
        typeof e === 'string' ? e : (e as Error).message ?? 'unknown error'
      }`;
    } finally {
      updateInstallBusy = false;
    }
  }

  function fmtSize(bytes: number): string {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`;
    return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
  }

  /* MCP sidecar health (M4 §2.9.8). One row per bundled sidecar with
   * a live "running / not running" indicator. Sidecars are spawned
   * lazily by Claude / Cursor on first MCP handshake — a
   * `not running` state just means no agent has called any of that
   * sidecar's tools yet in this Forgehold launch (or the sidecar
   * crashed; either way the next agent call will respawn it). */
  type SidecarHealth = { name: string; running: boolean; pid_count: number };
  let sidecarHealth = $state<SidecarHealth[] | null>(null);
  let sidecarHealthLoading = $state(false);

  async function refreshSidecarHealth() {
    sidecarHealthLoading = true;
    try {
      sidecarHealth = await invoke<SidecarHealth[]>('mcp_sidecar_health');
    } catch (e) {
      notifyError(e, { title: 'Probe failed' });
    } finally {
      sidecarHealthLoading = false;
    }
  }
  /* Fire once on mount so the section isn't empty on first view. */
  $effect(() => {
    if (sidecarHealth === null) void refreshSidecarHealth();
  });

  /* ------------------------------------------------------------------ */
  /* Crash-report opt-out (`docs/ROADMAP_1.0.md §1.3`).                  */
  /* The opt-out is file-backed (presence == opted out); see             */
  /* `crash_reporting.rs` for the path. Initial fetch happens on mount   */
  /* below so the toggle reflects the live filesystem state.             */
  /* ------------------------------------------------------------------ */
  let telemetryOptOut = $state(false);
  let telemetryBusy = $state(false);

  async function loadTelemetryPref() {
    try {
      telemetryOptOut = await invoke<boolean>('get_telemetry_opt_out');
    } catch {
      /* Pre-update binary doesn't expose the command; treat as
       * "default off". */
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

  /* Pull the persisted preference on first mount. Effect runs once
   * because we don't depend on any reactive read inside. */
  $effect(() => {
    void loadTelemetryPref();
  });

  /* ------------------------------------------------------------------ */
  /* Report-bug form (`docs/ROADMAP_1.0.md §1.3`).                       */
  /* Builds a Markdown bundle locally — connection state, last 50       */
  /* status events, layout snapshot, user description — for the user    */
  /* to copy or paste into a GitHub issue. Nothing leaves the machine   */
  /* unless they explicitly click "Open issue" / paste the bundle       */
  /* themselves.                                                        */
  /* ------------------------------------------------------------------ */

  /* Repo target for the "Open GitHub issue" button. Configured at
   * build time (or runtime via env when desired); we deliberately
   * don't autodetect from the current working repo — the user might
   * have multiple Forgehold checkouts. Empty string disables the
   * "Open issue" path; the user can still copy or download. */
  const BUG_REPORT_GITHUB_REPO = ''; /* e.g. "forgehold/forgehold" — wire up at 1.0 */

  let bugReportDescription = $state('');
  let bugReportPreview = $state<string | null>(null);
  let bugReportCopiedAt = $state<number | null>(null);

  function refreshBugReportPreview() {
    bugReportPreview = buildBugReport({
      description: bugReportDescription,
      appVersion: 'Forgehold 1.0.0'
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

  /* ------------------------------------------------------------------ */
  /* In-app docs viewer (`docs/ROADMAP_1.0.md §1.10`).                   */
  /* Reads bundled Markdown specs via the `list_bundled_docs` /          */
  /* `read_bundled_doc` Tauri commands and renders with `marked`. The    */
  /* Rust side handles path-traversal sanitization and falls back to     */
  /* the repo's `docs/` folder during dev so `pnpm tauri dev` works      */
  /* without re-bundling.                                                */
  /* ------------------------------------------------------------------ */

  let docsList = $state<string[] | null>(null);
  let docsListError = $state<string | null>(null);
  let activeDoc = $state<string | null>(null);
  let activeDocBody = $state<string | null>(null);
  let activeDocLoading = $state(false);
  let activeDocError = $state<string | null>(null);

  async function loadDocsList() {
    if (docsList !== null) return;
    try {
      docsList = await invoke<string[]>('list_bundled_docs');
    } catch (e) {
      docsListError = typeof e === 'string' ? e : (e as Error).message ?? 'unknown error';
    }
  }

  async function openDoc(name: string) {
    activeDoc = name;
    activeDocBody = null;
    activeDocError = null;
    activeDocLoading = true;
    try {
      const md = await invoke<string>('read_bundled_doc', { name });
      /* `marked.parse` is sync when no async extensions registered;
       * cast to string explicitly to satisfy TS now that the
       * declared return is `string | Promise<string>`. */
      activeDocBody = await Promise.resolve(marked.parse(md) as string | Promise<string>);
    } catch (e) {
      activeDocError = typeof e === 'string' ? e : (e as Error).message ?? 'unknown error';
    } finally {
      activeDocLoading = false;
    }
  }

  function closeDoc() {
    activeDoc = null;
    activeDocBody = null;
    activeDocError = null;
  }

  $effect(() => {
    void loadDocsList();
  });
</script>

<section class="settings-view">
  <div class="settings-header">
    <h1 class="view-title">Settings</h1>
    <p class="view-sub">Storage usage, cleanup, and app diagnostics.</p>
  </div>

  <div class="settings-body">
    <!-- Storage card -->
    <div class="card">
      <header class="card-head">
        <h2 class="card-title">Storage</h2>
        <p class="card-sub">
          Chat history, rules, and layout live in Tauri's localStorage. Worktrees are real git
          checkouts on disk under <span class="mono">{worktreeDir ?? '~/Library/Application Support/Forgehold/worktrees'}</span>.
        </p>
      </header>
      <div class="grid">
        <div class="stat">
          <div class="stat-label">Claude chats</div>
          <div class="stat-value mono">{claudeCount}</div>
        </div>
        <div class="stat">
          <div class="stat-label">Cursor chats</div>
          <div class="stat-value mono">{cursorCount}</div>
        </div>
        <div class="stat">
          <div class="stat-label">Total messages</div>
          <div class="stat-value mono">{totalMessages}</div>
        </div>
        <div class="stat">
          <div class="stat-label">Workbenches · columns</div>
          <div class="stat-value mono">{workbenchCount} · {totalColumns}</div>
        </div>
        <div class="stat">
          <div class="stat-label">Sessions size</div>
          <div class="stat-value mono">{formatBytes(sessionsBytes)}</div>
          <div class="stat-hint">localStorage cap is ~5–10 MB total</div>
        </div>
        <div class="stat">
          <div class="stat-label">All localStorage</div>
          <div class="stat-value mono">{formatBytes(lsBytes)}</div>
          <div class="stat-hint">includes layout, rules, filter prefs</div>
        </div>
        <div class="stat">
          <div class="stat-label">Worktrees on disk</div>
          <div class="stat-value mono">
            {#if worktreeStatLoading}…{:else}{formatBytes(worktreeBytes ?? 0)}{/if}
          </div>
          <div class="stat-hint">isolated branches per Claude/Cursor run</div>
        </div>
      </div>

      {#if persistError.sessions || persistError.rules}
        <div class="alert alert--error">
          <strong>Persistence is failing.</strong>
          {#if persistError.sessions}<div>Sessions: <span class="mono">{persistError.sessions}</span></div>{/if}
          {#if persistError.rules}<div>Rules: <span class="mono">{persistError.rules}</span></div>{/if}
          <div class="alert-hint">
            Quota is full — clean up orphan worktrees, or delete old chats. New messages stay in
            memory but won't survive a restart until storage frees up.
          </div>
        </div>
      {/if}

      <div class="card-actions">
        <button class="btn btn--ghost" onclick={() => void refreshDiskStats()} disabled={worktreeStatLoading}>
          {worktreeStatLoading ? 'Refreshing…' : 'Refresh'}
        </button>
        <button class="btn btn--ghost" onclick={openWorktreeDir} disabled={!worktreeDir}>
          Open worktrees folder
        </button>
        <div style="flex:1"></div>
        <button class="btn btn--primary" onclick={runCleanup} disabled={cleanupBusy}>
          {cleanupBusy ? 'Cleaning…' : 'Clean orphan worktrees > 14 days'}
        </button>
      </div>
    </div>

    <!-- Connection diagnostics. Driven by `connectionEventsState`,
         which records every status round-trip with timing + outcome.
         Useful when debugging "why did my Jira go dark at 3 AM" — the
         timeline shows whether the source actually disconnected, hit a
         429, or just returned a network error. -->
    <div class="card">
      <header class="card-head">
        <h2 class="card-title">Connection diagnostics</h2>
        <p class="card-sub">
          Last {connectionEventsState.events.length} status checks across every connected source. Click a row's source to filter. Local-only — nothing is sent anywhere.
        </p>
      </header>

      <div class="diag-tabs">
        {#each FILTER_TABS as tab (tab.id)}
          {@const count = tab.id === 'all' ? connectionEventsState.events.length : counts[tab.id]}
          <button
            class="diag-tab"
            class:active={eventFilter === tab.id}
            onclick={() => (eventFilter = tab.id)}
          >
            {tab.label}
            <span class="diag-tab-count mono">{count}</span>
          </button>
        {/each}
      </div>

      {#if filteredEvents.length === 0}
        <div class="diag-empty">
          {#if connectionEventsState.events.length === 0}
            No events recorded yet. Hit "Test" on a connection card to populate this log.
          {:else}
            No events for this source.
          {/if}
        </div>
      {:else}
        <ol class="diag-list">
          {#each filteredEvents as ev (ev.id)}
            <li class="diag-row diag-row--{ev.kind}">
              <span class="diag-source mono">{ev.source}</span>
              <span class="diag-kind">{eventLabel(ev)}</span>
              <span class="diag-detail" title={ev.message ?? ''}>
                {ev.message ?? ''}
              </span>
              <span class="diag-latency mono">
                {ev.latencyMs !== null ? `${ev.latencyMs}ms` : '—'}
              </span>
              <span class="diag-time mono">{relativeTime(ev.at)}</span>
            </li>
          {/each}
        </ol>
      {/if}

      <div class="card-actions">
        <button class="btn btn--ghost" onclick={confirmClear} disabled={connectionEventsState.events.length === 0}>
          Clear log
        </button>
      </div>
    </div>

    <!-- Theme picker -->
    <div class="card">
      <header class="card-head">
        <h2 class="card-title">Theme</h2>
        <p class="card-sub">
          Pick a colour palette. Layout, fonts and spacing stay the same — only colours flip.
        </p>
      </header>
      <div class="theme-grid">
        {#each THEMES as t (t.name)}
          <button
            class="theme-card"
            class:active={themeState.name === t.name}
            onclick={() => applyTheme(t.name)}
            title={t.sub}
            aria-pressed={themeState.name === t.name}
          >
            <span class="theme-swatch" style="background: {t.bg}; color: {t.fg};">
              <span class="theme-swatch-dot" style="background: {t.accent};"></span>
              <span class="theme-swatch-text">Aa</span>
            </span>
            <span class="theme-label">{t.label}</span>
            <span class="theme-sub">{t.sub}</span>
          </button>
        {/each}
      </div>
    </div>

    <!-- UI scale — global zoom multiplier on <html>. Same paradigm as
         Cursor's "Window: Zoom Level". See lib/state/scale.svelte.ts
         for the rationale on CSS `zoom` over Tauri's setZoom() API. -->
    <div class="card">
      <header class="card-head">
        <h2 class="card-title">UI scale</h2>
        <p class="card-sub">
          Zoom every glyph, border and spacing in the window. Useful on external monitors where
          the OS scaling feels too tight or too loose for chat reading.
        </p>
      </header>
      <div class="scale-grid">
        {#each SCALE_OPTIONS as opt (opt.value)}
          <button
            class="scale-card"
            class:active={scaleState.value === opt.value}
            onclick={() => applyScale(opt.value)}
            aria-pressed={scaleState.value === opt.value}
          >
            <span class="scale-label">{opt.label}</span>
          </button>
        {/each}
      </div>
    </div>

    <!-- Updates -->
    <div class="card">
      <header class="card-head">
        <h2 class="card-title">Updates</h2>
        <p class="card-sub">
          Auto-update channel for new Forgehold builds. Signed and notarized releases pull from the public manifest configured in <span class="mono">tauri.conf.json</span>.
        </p>
      </header>
      <div class="grid">
        <div class="stat">
          <div class="stat-label">Current version</div>
          <div class="stat-value mono">Forgehold 1.0.0</div>
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
      </div>
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

    <!-- MCP servers diagnostic (M4 §2.9.8) -->
    <div class="card">
      <header class="card-head">
        <h2 class="card-title">MCP servers</h2>
        <p class="card-sub">
          Forgehold's bundled sidecars. Spawned by Claude / Cursor on first MCP handshake; "not running" means no agent has talked to that sidecar yet this launch.
        </p>
      </header>
      <div class="update-actions">
        <button
          class="btn btn--ghost"
          onclick={() => void refreshSidecarHealth()}
          disabled={sidecarHealthLoading}
        >
          {sidecarHealthLoading ? 'Checking…' : 'Refresh'}
        </button>
      </div>
      {#if sidecarHealth}
        <ul class="sidecar-list">
          {#each sidecarHealth as s (s.name)}
            <li class="sidecar-row" class:running={s.running}>
              <span class="sidecar-dot" aria-hidden="true"></span>
              <span class="sidecar-name mono">{s.name}</span>
              <span class="sidecar-status">
                {s.running ? `running · ${s.pid_count} pid${s.pid_count === 1 ? '' : 's'}` : 'not running'}
              </span>
            </li>
          {/each}
        </ul>
      {/if}
    </div>

    <!-- Documentation -->
    <div class="card">
      <header class="card-head">
        <h2 class="card-title">Documentation</h2>
        <p class="card-sub">
          The full Forgehold spec set, bundled with the app. Each entry is rendered from <span class="mono">docs/*.md</span> in the repo. Pick one to read inline, or open the file in Finder.
        </p>
      </header>
      {#if docsListError}
        <div class="alert alert--error">{docsListError}</div>
      {:else if docsList === null}
        <div class="docs-loading">Loading…</div>
      {:else if docsList.length === 0}
        <div class="docs-empty">No bundled docs found.</div>
      {:else if activeDoc === null}
        <ul class="docs-list">
          {#each docsList as name (name)}
            <li>
              <button class="docs-link" onclick={() => void openDoc(name)}>
                <span class="mono">{name}.md</span>
              </button>
            </li>
          {/each}
        </ul>
      {:else}
        <div class="docs-active">
          <div class="docs-active-bar">
            <button class="btn btn--ghost" onclick={closeDoc}>← Back</button>
            <span class="docs-active-name mono">{activeDoc}.md</span>
          </div>
          {#if activeDocLoading}
            <div class="docs-loading">Loading…</div>
          {:else if activeDocError}
            <div class="alert alert--error">{activeDocError}</div>
          {:else if activeDocBody}
            <article class="docs-md">
              {@html activeDocBody}
            </article>
          {/if}
        </div>
      {/if}
    </div>

    <!-- Build / app info -->
    <div class="card">
      <header class="card-head">
        <h2 class="card-title">App</h2>
      </header>
      <div class="grid">
        <div class="stat">
          <div class="stat-label">Build</div>
          <div class="stat-value mono">Forgehold 1.0.0 · aarch64</div>
        </div>
        <div class="stat">
          <div class="stat-label">Storage keys</div>
          <div class="stat-value mono">{SESSIONS_STORAGE_KEY}, {RULES_STORAGE_KEY}</div>
        </div>
      </div>
      <div class="update-actions">
        <button
          class="btn btn--ghost"
          onclick={() => resetWelcome()}
          disabled={!welcomeState.completed}
          title="Re-open the first-launch welcome flow"
        >
          {welcomeState.completed ? 'Show welcome flow again' : 'Welcome flow active'}
        </button>
      </div>
    </div>
  </div>
</section>

<style>
  .settings-view { overflow-y: auto; flex: 1; display: flex; flex-direction: column; }
  .settings-header { padding: 48px 56px 12px; max-width: 980px; margin: 0 auto; width: 100%; }
  .view-title { font-size: 28px; font-weight: 600; letter-spacing: -0.025em; color: var(--text-0); margin-bottom: 6px; }
  .view-sub { font-size: 13.5px; color: var(--text-2); margin: 0; }

  .settings-body {
    padding: 8px 56px 48px; max-width: 980px; margin: 0 auto; width: 100%;
    display: flex; flex-direction: column; gap: 18px;
  }

  .card {
    background: var(--bg-1);
    border: 1px solid var(--border-neutral);
    border-radius: 12px;
    padding: 20px 22px;
    display: flex; flex-direction: column; gap: 14px;
    box-shadow: var(--shadow-1);
  }
  .card-head { display: flex; flex-direction: column; gap: 4px; }
  .card-title { font-size: 15px; font-weight: 600; color: var(--text-0); margin: 0; }
  .card-sub { font-size: 12.5px; color: var(--text-2); margin: 0; line-height: 1.55; }
  .card-sub .mono { background: var(--bg-2); padding: 1px 6px; border-radius: 4px; font-size: 11.5px; color: var(--text-1); }

  .grid {
    display: grid; grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
    gap: 12px 20px;
  }
  .stat {
    display: flex; flex-direction: column; gap: 3px;
    padding: 10px 12px;
    background: var(--bg-0);
    border: 1px solid var(--border-neutral);
    border-radius: 8px;
  }
  .stat-label { font-size: 11px; font-weight: 600; color: var(--text-mute); letter-spacing: 0.04em; text-transform: uppercase; }
  .stat-value { font-size: 14px; color: var(--text-0); }
  .stat-hint { font-size: 11px; color: var(--text-mute); margin-top: 2px; }

  .alert {
    padding: 12px 14px;
    border-radius: 8px;
    border: 1px solid;
    font-size: 12.5px;
    line-height: 1.5;
    display: flex; flex-direction: column; gap: 4px;
  }
  .alert--error {
    background: rgba(214, 72, 44, 0.08);
    border-color: rgba(214, 72, 44, 0.4);
    color: var(--error);
  }
  .alert .mono { color: var(--text-1); }
  .alert-hint { font-size: 11.5px; color: var(--text-2); }

  .card-actions {
    display: flex; gap: 8px; align-items: center; flex-wrap: wrap;
  }

  /* Theme picker — three side-by-side cards. The swatch shows the
     palette literally so the user can compare at a glance: surface
     colour as background, text colour for the "Aa" preview, accent
     colour as a dot. */
  .theme-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
    gap: 10px;
  }
  .theme-card {
    display: flex; flex-direction: column; gap: 6px;
    padding: 14px;
    background: var(--bg-2);
    border: 1px solid var(--border-neutral);
    border-radius: 10px;
    text-align: left;
    cursor: pointer;
    transition: all 140ms;
  }
  .theme-card:hover {
    border-color: var(--border-neutral-hi);
    background: var(--bg-3);
    transform: translateY(-1px);
  }
  .theme-card.active {
    border-color: var(--accent);
    box-shadow: 0 0 0 1px var(--accent), 0 4px 14px var(--accent-glow);
  }
  .theme-swatch {
    display: flex; align-items: center; justify-content: space-between;
    padding: 14px 16px;
    border-radius: 7px;
    font-size: 22px; font-weight: 700;
    border: 1px solid rgba(0, 0, 0, 0.12);
  }
  .theme-swatch-dot {
    width: 14px; height: 14px;
    border-radius: 50%;
    flex-shrink: 0;
    box-shadow: 0 0 0 2px rgba(255, 255, 255, 0.08);
  }
  .theme-swatch-text { letter-spacing: -0.02em; }
  .theme-label {
    font-size: 13px; font-weight: 600; color: var(--text-0);
    margin-top: 4px;
  }
  .theme-sub { font-size: 11.5px; color: var(--text-2); }
  .theme-card.active .theme-label { color: var(--accent-bright); }

  /* Scale picker — a row of compact percentage chips. Smaller than the
     theme cards because the only thing to preview is a number. */
  .scale-grid {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }
  .scale-card {
    min-width: 72px;
    padding: 10px 14px;
    background: var(--bg-2);
    border: 1px solid var(--border-neutral);
    border-radius: 8px;
    text-align: center;
    cursor: pointer;
    transition: all 140ms;
  }
  .scale-card:hover {
    border-color: var(--border-neutral-hi);
    background: var(--bg-3);
    transform: translateY(-1px);
  }
  .scale-card.active {
    border-color: var(--accent);
    box-shadow: 0 0 0 1px var(--accent), 0 4px 14px var(--accent-glow);
  }
  .scale-label {
    font-size: 13px;
    font-weight: 600;
    color: var(--text-0);
  }
  .scale-card.active .scale-label { color: var(--accent-bright); }

  /* Connection diagnostics — filter chips + chronological list. */
  .diag-tabs {
    display: flex; flex-wrap: wrap; gap: 6px;
  }
  .diag-tab {
    display: inline-flex; align-items: center; gap: 6px;
    padding: 5px 10px;
    background: var(--bg-2);
    border: 1px solid var(--border-neutral);
    border-radius: 999px;
    font-size: 11.5px; font-weight: 500; color: var(--text-1);
    cursor: pointer; transition: all 140ms;
  }
  .diag-tab:hover { background: var(--bg-3); color: var(--text-0); }
  .diag-tab.active {
    background: var(--bg-3);
    border-color: var(--accent);
    color: var(--accent-bright);
  }
  .diag-tab-count {
    font-size: 10px; color: var(--text-mute);
    background: var(--bg-0);
    padding: 1px 6px; border-radius: 999px;
  }
  .diag-tab.active .diag-tab-count { color: var(--accent-bright); }

  .diag-list {
    list-style: none; padding: 0; margin: 0;
    display: flex; flex-direction: column; gap: 2px;
    max-height: 320px; overflow-y: auto;
    border: 1px solid var(--border-neutral);
    border-radius: 8px;
    background: var(--bg-0);
  }
  .diag-row {
    display: grid;
    grid-template-columns: 60px 80px 1fr auto auto;
    gap: 12px; align-items: center;
    padding: 6px 12px;
    font-size: 11.5px; color: var(--text-1);
    border-bottom: 1px solid var(--border-neutral);
  }
  .diag-row:last-child { border-bottom: none; }
  .diag-source { font-size: 10.5px; color: var(--text-mute); text-transform: uppercase; letter-spacing: 0.04em; }
  .diag-kind { font-weight: 600; }
  .diag-detail {
    color: var(--text-2);
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
    min-width: 0;
  }
  .diag-latency, .diag-time { font-size: 10.5px; color: var(--text-mute); }
  .diag-row--connected .diag-kind { color: var(--accent-bright); }
  .diag-row--disconnected .diag-kind { color: var(--text-2); }
  .diag-row--rate_limited .diag-kind { color: #f59e0b; }
  .diag-row--error .diag-kind { color: #f87171; }
  .diag-empty {
    padding: 16px; border: 1px dashed var(--border-neutral);
    border-radius: 8px; text-align: center;
    font-size: 12px; color: var(--text-mute);
  }

  /* Updates card */
  .update-actions { display: flex; gap: 8px; flex-wrap: wrap; }
  .update-status {
    padding: 10px 12px;
    border-radius: 8px;
    background: var(--bg-2);
    border: 1px solid var(--border-neutral);
    color: var(--text-1);
    font-size: 12.5px; line-height: 1.5;
  }
  .update-status--ready {
    background: rgba(16, 185, 129, 0.08);
    border-color: rgba(16, 185, 129, 0.35);
    color: var(--accent-bright);
  }
  .update-status--installed {
    background: rgba(16, 185, 129, 0.14);
    border-color: rgba(16, 185, 129, 0.5);
    color: var(--accent-bright);
    font-weight: 500;
  }
  .update-detail {
    display: flex; flex-direction: column; gap: 10px;
    padding: 12px; border-radius: 10px;
    background: var(--bg-0); border: 1px solid var(--border-neutral);
  }
  .update-version { font-size: 13px; color: var(--text-0); }
  .update-notes {
    margin: 0; padding: 10px 12px;
    background: var(--bg-2); border-radius: 6px;
    font-size: 12px; color: var(--text-1); line-height: 1.55;
    white-space: pre-wrap;
  }
  .update-progress {
    position: relative; height: 18px; border-radius: 4px;
    background: var(--bg-2); overflow: hidden;
  }
  .update-progress-bar {
    position: absolute; left: 0; top: 0; bottom: 0;
    background: linear-gradient(90deg, var(--accent), var(--accent-bright));
    transition: width 200ms ease;
  }
  .update-progress-label {
    position: relative; z-index: 1; font-size: 10.5px;
    text-align: center; line-height: 18px; color: var(--text-0);
    text-shadow: 0 0 2px var(--bg-0);
  }

  /* Bug-report card */
  .bug-report-textarea {
    width: 100%;
    padding: 10px 12px;
    background: var(--bg-0);
    border: 1px solid var(--border-neutral);
    border-radius: 8px;
    color: var(--text-0);
    font-family: inherit;
    font-size: 12.5px;
    line-height: 1.5;
    resize: vertical;
  }
  .bug-report-textarea:focus {
    outline: none;
    border-color: var(--border-hi2);
    box-shadow: 0 0 0 2px var(--accent-glow);
  }
  .bug-report-preview {
    background: var(--bg-0); border: 1px solid var(--border-neutral);
    border-radius: 8px; padding: 6px 12px; font-size: 12px;
  }
  .bug-report-preview > summary {
    cursor: pointer; color: var(--text-2);
    padding: 4px 0; user-select: none;
  }
  .bug-report-preview-pre {
    margin: 6px 0 4px;
    padding: 10px 12px;
    background: var(--bg-2);
    border-radius: 6px;
    font-size: 11px; line-height: 1.5;
    color: var(--text-1);
    max-height: 320px; overflow: auto;
    white-space: pre-wrap; word-break: break-word;
  }

  /* Documentation card */
  .docs-loading, .docs-empty {
    padding: 14px; text-align: center;
    color: var(--text-mute); font-size: 12.5px;
    border: 1px dashed var(--border-neutral); border-radius: 8px;
  }
  .docs-list { list-style: none; padding: 0; margin: 0; display: grid; grid-template-columns: repeat(auto-fit, minmax(180px, 1fr)); gap: 6px; }
  .docs-link {
    display: inline-flex; width: 100%;
    padding: 8px 10px;
    background: var(--bg-0);
    border: 1px solid var(--border-neutral);
    border-radius: 6px;
    color: var(--text-1);
    cursor: pointer;
    text-align: left;
    transition: all 120ms ease;
  }
  .docs-link:hover {
    background: var(--bg-2);
    border-color: var(--border-hi2);
    color: var(--text-0);
  }
  .docs-active { display: flex; flex-direction: column; gap: 12px; }
  .docs-active-bar {
    display: flex; align-items: center; gap: 12px;
    padding-bottom: 8px;
    border-bottom: 1px solid var(--border-neutral);
  }
  .docs-active-name { color: var(--text-mute); font-size: 11.5px; }
  .docs-md {
    max-height: 70vh; overflow-y: auto;
    padding: 4px 8px;
    font-size: 13px; line-height: 1.55;
    color: var(--text-1);
  }
  .docs-md :global(h1) { font-size: 20px; margin: 18px 0 10px; color: var(--text-0); }
  .docs-md :global(h2) { font-size: 16px; margin: 16px 0 8px; color: var(--text-0); border-bottom: 1px solid var(--border-neutral); padding-bottom: 4px; }
  .docs-md :global(h3) { font-size: 14px; margin: 14px 0 6px; color: var(--text-0); }
  .docs-md :global(h4) { font-size: 13px; margin: 12px 0 6px; color: var(--text-0); }
  .docs-md :global(p) { margin: 0 0 10px; }
  .docs-md :global(ul), .docs-md :global(ol) { margin: 0 0 10px; padding-left: 22px; }
  .docs-md :global(li) { margin: 2px 0; }
  .docs-md :global(code) {
    background: var(--bg-2); padding: 1px 5px; border-radius: 4px;
    font-size: 11.5px; color: var(--text-0);
  }
  .docs-md :global(pre) {
    background: var(--bg-2); padding: 10px 12px; border-radius: 8px;
    overflow-x: auto; font-size: 11.5px; line-height: 1.5;
    margin: 0 0 12px;
  }
  .docs-md :global(pre code) { background: none; padding: 0; }
  .docs-md :global(blockquote) {
    margin: 0 0 10px; padding: 6px 14px;
    border-left: 3px solid var(--accent);
    color: var(--text-2);
    background: var(--bg-2); border-radius: 0 6px 6px 0;
  }
  .docs-md :global(table) { border-collapse: collapse; margin: 0 0 12px; font-size: 12px; }
  .docs-md :global(th), .docs-md :global(td) {
    border: 1px solid var(--border-neutral);
    padding: 4px 8px;
  }
  .docs-md :global(a) { color: var(--accent-bright); }

  /* MCP sidecar list */
  .sidecar-list { list-style: none; padding: 0; margin: 0; display: flex; flex-direction: column; gap: 4px; }
  .sidecar-row {
    display: grid;
    grid-template-columns: auto 1fr auto;
    gap: 10px; align-items: center;
    padding: 6px 12px;
    background: var(--bg-0);
    border: 1px solid var(--border-neutral);
    border-radius: 7px;
    font-size: 12px;
  }
  .sidecar-dot {
    width: 8px; height: 8px; border-radius: 50%;
    background: var(--text-mute);
  }
  .sidecar-row.running .sidecar-dot {
    background: var(--accent-bright);
    box-shadow: 0 0 6px var(--accent-glow);
  }
  .sidecar-name { color: var(--text-0); }
  .sidecar-status { color: var(--text-mute); font-size: 10.5px; }
  .sidecar-row.running .sidecar-status { color: var(--accent-bright); }

  /* Privacy toggle */
  .telemetry-row {
    display: grid;
    grid-template-columns: auto 1fr;
    grid-row-gap: 2px;
    column-gap: 10px;
    align-items: center;
    padding: 8px 12px;
    border-radius: 8px;
    background: var(--bg-0);
    border: 1px solid var(--border-neutral);
    cursor: pointer;
  }
  .telemetry-row input[type='checkbox'] {
    grid-row: 1 / span 2;
    width: 16px; height: 16px;
    accent-color: var(--accent);
  }
  .telemetry-row-label { font-size: 12.5px; color: var(--text-0); font-weight: 500; }
  .telemetry-row-hint  { font-size: 11px; color: var(--text-mute); }
</style>
