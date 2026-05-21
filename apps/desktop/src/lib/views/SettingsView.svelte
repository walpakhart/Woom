<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { getVersion } from '@tauri-apps/api/app';
  import { openPath, openUrl } from '@tauri-apps/plugin-opener';
  import {
    updateState as updatesStore,
    checkNow as updatesCheckNow,
    installNow as updatesInstallNow,
    setAutoCheck as updatesSetAutoCheck,
    clearSkip as updatesClearSkip,
  } from '$lib/state/updates.svelte';
  import { marked } from 'marked';
  import { buildBugReport, bugReportGithubIssueUrl } from '$lib/services/bugReport';
  import { resetWelcome, welcomeState } from '$lib/state/welcome.svelte';
  import { sessionsState, persistError, SESSIONS_STORAGE_KEY, RULES_STORAGE_KEY } from '$lib/state/sessions.svelte';
  import { layoutState } from '$lib/state/layout.svelte';
  import { notify, notifyError } from '$lib/state/toaster.svelte';
  import { themeState, applyTheme, type ThemeName } from '$lib/state/theme.svelte';
  import { scaleState, applyScale, SCALE_OPTIONS } from '$lib/state/scale.svelte';
  import { densityState, applyDensity, type Density } from '$lib/state/density.svelte';
  import {
    connectionEventsState,
    clearConnectionEvents,
    type ConnectionEvent,
    type ConnectionEventSource
  } from '$lib/state/connectionEvents.svelte';
  import { relativeTime } from '$lib/data';
  import {
    hooksState,
    loadHookConfig,
    saveHookConfig,
    enabledHookCount,
    type HookConfig
  } from '$lib/state/hooks.svelte';

  /* Theme picker. Each entry encodes a tiny preview swatch (bg, text,
     accent) so the user can eyeball the palette without applying.
     Two palettes only — Iconic IS the dark variant under the W-mark
     palette, so a separate "Dark" was retired (it was redundant). */
  const THEMES: { name: ThemeName; label: string; sub: string; bg: string; fg: string; accent: string }[] = [
    { name: 'iconic', label: 'Iconic', sub: 'Sage + mint on cool noir', bg: '#0E1112', fg: '#EBEFEC', accent: '#B0DCC8' },
    { name: 'light',  label: 'Light',  sub: 'Sage + mint on cream',     bg: '#F1F5F2', fg: '#0E1B16', accent: '#2E5A4A' }
  ];

  /** 14 days in seconds — matches the SPEC §worktrees retention rule. */
  const ORPHAN_AGE_SECS = 14 * 24 * 60 * 60;

  // ---- Live counters (synchronously derived from existing stores) -------

  const claudeCount = $derived(sessionsState.list.filter((s) => s.agentKind === 'claude').length);
  const cursorCount = $derived(sessionsState.list.filter((s) => s.agentKind === 'cursor').length);
  const totalMessages = $derived(
    sessionsState.list.reduce((acc, s) => acc + s.messages.length, 0)
  );
  const editorLinkCount = $derived(Object.keys(layoutState.links.editorToAgent).length);
  const canvasLinkCount = $derived(Object.keys(layoutState.links.canvasToAgent).length);

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

  // ---- Hooks editor -----------------------------------------------------
  // Local draft so the user can edit + revert without writing every
  // keystroke through `saveHookConfig`. `pristine` is the last successful
  // load/save snapshot; the Save / Revert buttons disable when draft ==
  // pristine (no unsaved changes). Parse error is shown inline; Save is
  // gated on it.
  let hooksDraft = $state('');
  let hooksDraftPristine = $state('');
  let hooksParseError = $state<string | null>(null);
  let hooksLoading = $state(false);
  const hookCount = $derived(enabledHookCount());
  const hooksPlaceholder = `{
  "hooks": {
    "UserPromptSubmit": [
      {
        "matcher": "*",
        "handler": { "type": "command", "command": "/path/to/script.sh" },
        "timeout_ms": 5000,
        "disabled": false
      }
    ],
    "Stop": [],
    "SessionStart": []
  }
}`;

  /* Re-validate on every edit. Cheap (JSON.parse on a few KB). The
   *  parser error message is surfaced inline below the textarea. */
  $effect(() => {
    if (hooksDraft.trim().length === 0) {
      hooksParseError = null;
      return;
    }
    try {
      const parsed = JSON.parse(hooksDraft);
      if (parsed && typeof parsed === 'object' && parsed.hooks && typeof parsed.hooks !== 'object') {
        hooksParseError = '`hooks` must be an object keyed by event name';
        return;
      }
      hooksParseError = null;
    } catch (e) {
      hooksParseError = `JSON parse error: ${(e as Error).message}`;
    }
  });

  async function resetHooksDraft(): Promise<void> {
    hooksDraft = hooksDraftPristine;
  }

  async function saveHooksDraft(): Promise<void> {
    if (hooksParseError) return;
    hooksLoading = true;
    try {
      const parsed = hooksDraft.trim() === ''
        ? ({ hooks: {} } as HookConfig)
        : (JSON.parse(hooksDraft) as HookConfig);
      await saveHookConfig(parsed);
      hooksDraftPristine = hooksDraft;
      notify({ kind: 'success', title: 'Hooks saved', ttlMs: 2200 });
    } catch (e) {
      notifyError(e, { title: 'Hooks save failed' });
    } finally {
      hooksLoading = false;
    }
  }

  /* On mount: pull the on-disk config + populate the draft. We do it
   *  via the existing `loadHookConfig` so the reactive store stays in
   *  sync with any other view that reads it. */
  $effect(() => {
    void (async () => {
      hooksLoading = true;
      try {
        const cfg = hooksState.loaded ? hooksState.config : await loadHookConfig();
        hooksDraftPristine = JSON.stringify(cfg, null, 2);
        if (hooksDraft.length === 0) hooksDraft = hooksDraftPristine;
      } finally {
        hooksLoading = false;
      }
    })();
  });

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

  // ---- Long-term memory stats ------------------------------------------

  /** Tuple of counters returned by `memory_stats_local`. Null until the
   *  first refresh resolves — the panel renders "—" placeholders so
   *  the layout doesn't reflow once data lands. */
  interface MemStats {
    total: number;
    by_kind: Record<string, number>;
    db_bytes: number;
  }
  let memStats = $state<MemStats | null>(null);
  let memStatsBusy = $state(false);
  /** Display order for the kind chips. Matches the canonical order
   *  the sidecar's KINDS constant uses so the panel reads consistently
   *  regardless of insertion order in `by_kind`. */
  const memKinds = ['user', 'feedback', 'project', 'reference', 'note'] as const;

  async function refreshMemStats(): Promise<void> {
    if (memStatsBusy) return;
    memStatsBusy = true;
    try {
      const stats = await invoke<MemStats>('memory_stats_local');
      memStats = stats;
    } catch (e) {
      notifyError(e, { title: 'Could not read memory stats' });
    } finally {
      memStatsBusy = false;
    }
  }

  $effect(() => { void refreshMemStats(); });

  /* Memory browser. Shown beneath the stats panel when the user
     clicks "Browse". List loads on first open + on Refresh; search
     query debounced 250ms (matches inbox-search latency). Empty
     query lists newest-first via memory_list_local; non-empty query
     fans out to memory_search_local. */
  interface MemRow {
    id: number;
    kind: string;
    content: string;
    tags: string;
    created_at: number;
  }
  let memBrowserOpen = $state(false);
  let memBrowserQuery = $state('');
  let memBrowserKind = $state<string | null>(null);
  let memBrowserRows = $state<MemRow[]>([]);
  let memBrowserBusy = $state(false);
  let memBrowserDebounce: ReturnType<typeof setTimeout> | null = null;

  async function loadMemBrowser(): Promise<void> {
    if (memBrowserBusy) return;
    memBrowserBusy = true;
    try {
      const q = memBrowserQuery.trim();
      if (q) {
        memBrowserRows = await invoke<MemRow[]>('memory_search_local', {
          query: q,
          limit: 50
        });
      } else {
        memBrowserRows = await invoke<MemRow[]>('memory_list_local', {
          kind: memBrowserKind,
          limit: 50,
          offset: 0
        });
      }
    } catch (e) {
      notifyError(e, { title: 'Memory browser failed' });
    } finally {
      memBrowserBusy = false;
    }
  }

  function scheduleMemReload(): void {
    if (memBrowserDebounce) clearTimeout(memBrowserDebounce);
    memBrowserDebounce = setTimeout(() => void loadMemBrowser(), 250);
  }

  /* Auto-load when the browser opens for the first time. Subsequent
     opens reuse cached rows — Refresh button forces a re-fetch. */
  $effect(() => {
    if (memBrowserOpen && memBrowserRows.length === 0 && !memBrowserBusy) {
      void loadMemBrowser();
    }
  });

  /* Re-run query whenever the filter changes. Effect tracks the
     relevant fields; debounce smooths typing storms. */
  $effect(() => {
    void memBrowserQuery;
    void memBrowserKind;
    if (memBrowserOpen) scheduleMemReload();
  });

  /* In-place editor state. Tracks at most one open editor at a time
     keyed by row id; the textarea/kind/tags inputs bind to local
     drafts so the user can cancel without persisting. */
  let editingMemId = $state<number | null>(null);
  let editDraftContent = $state('');
  let editDraftKind = $state('note');
  let editDraftTags = $state('');
  let editBusy = $state(false);

  function startEditMemRow(row: MemRow): void {
    editingMemId = row.id;
    editDraftContent = row.content;
    editDraftKind = row.kind;
    editDraftTags = row.tags;
  }
  function cancelEditMemRow(): void {
    editingMemId = null;
    editDraftContent = '';
    editDraftKind = 'note';
    editDraftTags = '';
  }
  async function saveEditMemRow(): Promise<void> {
    if (editingMemId === null) return;
    const id = editingMemId;
    const content = editDraftContent.trim();
    if (!content) {
      notify({ kind: 'error', title: 'Content cannot be empty', ttlMs: 2200 });
      return;
    }
    editBusy = true;
    try {
      const tagsArr = editDraftTags
        .split(',')
        .map((t) => t.trim())
        .filter((t) => t.length > 0);
      await invoke<number>('memory_update_local', {
        id,
        content,
        kind: editDraftKind,
        tags: tagsArr
      });
      /* Mutate the local row in-place so the row preview updates
         without a full reload (full reload would re-fetch + lose
         scroll position). */
      memBrowserRows = memBrowserRows.map((r) =>
        r.id === id
          ? { ...r, content, kind: editDraftKind, tags: tagsArr.join(',') }
          : r
      );
      cancelEditMemRow();
      void refreshMemStats();
      notify({ kind: 'success', title: 'Memory updated', ttlMs: 1800 });
    } catch (e) {
      notifyError(e, { title: 'Update failed' });
    } finally {
      editBusy = false;
    }
  }

  async function deleteMemRow(id: number): Promise<void> {
    if (!window.confirm(`Delete memory #${id}? This can't be undone.`)) return;
    try {
      await invoke<number>('memory_delete_local', { id });
      memBrowserRows = memBrowserRows.filter((r) => r.id !== id);
      /* Stats become stale after delete. Refresh in the background
         so the kind chips + total count update without a full reload
         loop. */
      void refreshMemStats();
    } catch (e) {
      notifyError(e, { title: 'Delete failed' });
    }
  }

  function memRowDate(epoch: number): string {
    const d = new Date(epoch * 1000);
    const yyyy = d.getFullYear();
    const mm = String(d.getMonth() + 1).padStart(2, '0');
    const dd = String(d.getDate()).padStart(2, '0');
    return `${yyyy}-${mm}-${dd}`;
  }

  function memRowPreview(s: string): string {
    const collapsed = s.replace(/\s+/g, ' ').trim();
    return collapsed.length > 220 ? collapsed.slice(0, 217) + '…' : collapsed;
  }

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

  /* Updater UI is fully driven by the Rust-side state store now. Every
   * `$state` below this block migrated to `$derived` so the toast, the
   * Settings card, and any future release-notes overlay read from the
   * same source of truth (`updateState` from `$lib/state/updates.svelte`).
   * The store is initialised once on app mount in `+page.svelte` via
   * `initUpdatesStore()`. */
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
  /* Match the old `{ version, body }` shape so the template's
   * `updateAvailable.version` / `updateAvailable.body` bindings keep
   * working without a markup rewrite. */
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
    try {
      await updatesCheckNow();
    } catch (e) {
      notifyError(e, { title: 'Update check failed' });
    }
  }

  async function installUpdate() {
    if (updatesStore.phase.kind !== 'available') return;
    try {
      await updatesInstallNow();
    } catch (e) {
      notifyError(e, { title: 'Install failed' });
    }
  }

  async function toggleAutoCheck(enabled: boolean) {
    try {
      await updatesSetAutoCheck(enabled);
    } catch (e) {
      notifyError(e, { title: 'Settings update failed' });
    }
  }

  async function clearSkippedVersion() {
    try {
      await updatesClearSkip();
    } catch (e) {
      notifyError(e, { title: 'Clear skip failed' });
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
   * sidecar's tools yet in this Woom launch (or the sidecar
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
   * have multiple Woom checkouts. Empty string disables the
   * "Open issue" path; the user can still copy or download. */
  const BUG_REPORT_GITHUB_REPO = ''; /* e.g. "woom-app/woom" — wire up at 1.0 */

  let bugReportDescription = $state('');
  let bugReportPreview = $state<string | null>(null);
  let bugReportCopiedAt = $state<number | null>(null);

  /* Live app version pulled from Tauri's package_info() — single source of
   * truth so the Updates/Build/bug-report panels can't drift apart again.
   * Falls back to "Woom" until the Tauri call resolves (first paint). */
  let appVersionLabel = $state('Woom');
  onMount(async () => {
    try {
      appVersionLabel = `Woom ${await getVersion()}`;
    } catch {
      /* leave fallback in place */
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
          checkouts on disk under <span class="mono">{worktreeDir ?? '~/Library/Application Support/Woom/worktrees'}</span>.
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
          <div class="stat-label">Editor · canvas links</div>
          <div class="stat-value mono">{editorLinkCount} · {canvasLinkCount}</div>
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

    <!-- UI density — distinct from scale. Trims padding around list
         rows, pane headers, and chat messages so a single laptop screen
         fits ~20-25% more content. Keyboard: ⌘\ toggles between the two.
         See lib/state/density.svelte.ts for the apply mechanism. -->
    <div class="card">
      <header class="card-head">
        <h2 class="card-title">UI density</h2>
        <p class="card-sub">
          Trim padding around inbox cards and chat messages to fit more on screen. Distinct from UI scale — fonts stay the same size. Keyboard shortcut: <span class="mono">⌘ \</span>.
        </p>
      </header>
      <div class="scale-grid">
        {#each [{ value: 'comfortable' as Density, label: 'Comfortable' }, { value: 'compact' as Density, label: 'Compact' }] as opt (opt.value)}
          <button
            class="scale-card"
            class:active={densityState.value === opt.value}
            onclick={() => applyDensity(opt.value)}
            aria-pressed={densityState.value === opt.value}
          >
            <span class="scale-label">{opt.label}</span>
          </button>
        {/each}
      </div>
    </div>

    <!-- Long-term memory.
         Surfaces stats from the woom-memory SQLite store so the user
         can see what's accumulated. Numbers are read on mount + on
         "Refresh" — we don't subscribe to changes since the underlying
         store can be mutated from MCP / paste-trap / auto-distill at
         arbitrary times and a polling loop would be noise. -->
    <div class="card">
      <header class="card-head">
        <h2 class="card-title">Long-term memory</h2>
        <p class="card-sub">
          Durable notes that persist across chat sessions. The agent searches them at the start of every turn; the paste-trap and chat archive flows also write here. Stored at <span class="mono">~/Library/Application Support/Woom/memory.db</span>.
        </p>
      </header>
      <div class="grid">
        <div class="stat">
          <div class="stat-label">Total memories</div>
          <div class="stat-value mono">{memStats?.total.toLocaleString() ?? '—'}</div>
        </div>
        <div class="stat">
          <div class="stat-label">DB size</div>
          <div class="stat-value mono">{memStats ? formatBytes(memStats.db_bytes) : '—'}</div>
        </div>
      </div>
      {#if memStats && memStats.total > 0}
        <div class="mem-breakdown">
          {#each memKinds as kind (kind)}
            {@const n = memStats.by_kind[kind] ?? 0}
            {#if n > 0}
              <span class="mem-chip mono" title="{n.toLocaleString()} {kind} memories">
                <span class="mem-chip-kind">{kind}</span>
                <span class="mem-chip-n">{n.toLocaleString()}</span>
              </span>
            {/if}
          {/each}
        </div>
      {/if}
      <div class="update-actions">
        <button class="btn btn--ghost" onclick={refreshMemStats} disabled={memStatsBusy}>
          {memStatsBusy ? 'Refreshing…' : 'Refresh'}
        </button>
        <button class="btn btn--ghost" onclick={() => (memBrowserOpen = !memBrowserOpen)}>
          {memBrowserOpen ? 'Hide browser' : 'Browse'}
        </button>
      </div>
      {#if memBrowserOpen}
        <div class="mem-browser">
          <div class="mem-browser-controls">
            <input
              class="mem-browser-search mono"
              type="text"
              bind:value={memBrowserQuery}
              placeholder="Search memories — words, project names, tags…"
              spellcheck="false"
              autocomplete="off"
            />
            <select
              class="mem-browser-kind"
              bind:value={memBrowserKind}
              disabled={memBrowserQuery.trim().length > 0}
              title={memBrowserQuery.trim() ? 'Kind filter disabled while a search query is active' : 'Filter by kind'}
            >
              <option value={null}>All kinds</option>
              {#each memKinds as kind (kind)}
                <option value={kind}>{kind}</option>
              {/each}
            </select>
            <button class="btn btn--ghost btn--sm" onclick={loadMemBrowser} disabled={memBrowserBusy}>
              {memBrowserBusy ? '…' : 'Reload'}
            </button>
          </div>
          <div class="mem-browser-list">
            {#if memBrowserRows.length === 0 && !memBrowserBusy}
              <div class="mem-browser-empty">
                {memBrowserQuery.trim() ? 'No matches for that query.' : 'No memories yet.'}
              </div>
            {:else}
              {#each memBrowserRows as row (row.id)}
                <div class="mem-row" class:mem-row--editing={editingMemId === row.id}>
                  <div class="mem-row-head">
                    <span class="mem-row-id mono">#{row.id}</span>
                    <span class="mem-row-kind mono">{row.kind}</span>
                    <span class="mem-row-date mono">{memRowDate(row.created_at)}</span>
                    {#if row.tags}
                      <span class="mem-row-tags mono" title={row.tags}>{row.tags}</span>
                    {/if}
                    {#if editingMemId !== row.id}
                      <button
                        class="mem-row-edit"
                        onclick={() => startEditMemRow(row)}
                        title="Edit this memory"
                        aria-label="Edit memory #{row.id}"
                      >
                        <svg viewBox="0 0 24 24" width="13" height="13" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                          <path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7"/>
                          <path d="M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4z"/>
                        </svg>
                      </button>
                    {/if}
                    <button
                      class="mem-row-del"
                      onclick={() => void deleteMemRow(row.id)}
                      title="Delete this memory"
                      aria-label="Delete memory #{row.id}"
                    >
                      <svg viewBox="0 0 24 24" width="13" height="13" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                        <polyline points="3 6 5 6 21 6"/>
                        <path d="M19 6l-2 14a2 2 0 0 1-2 2H9a2 2 0 0 1-2-2L5 6"/>
                        <path d="M10 11v6 M14 11v6"/>
                      </svg>
                    </button>
                  </div>
                  {#if editingMemId === row.id}
                    <div class="mem-row-edit-form">
                      <textarea
                        class="mem-row-edit-content mono"
                        bind:value={editDraftContent}
                        rows="6"
                        spellcheck="false"
                        placeholder="Memory content…"
                      ></textarea>
                      <div class="mem-row-edit-row">
                        <label class="mem-row-edit-field">
                          <span class="mem-row-edit-label">Kind</span>
                          <select bind:value={editDraftKind} class="mem-row-edit-kind">
                            {#each memKinds as k (k)}
                              <option value={k}>{k}</option>
                            {/each}
                          </select>
                        </label>
                        <label class="mem-row-edit-field mem-row-edit-field--grow">
                          <span class="mem-row-edit-label">Tags (comma)</span>
                          <input
                            type="text"
                            class="mem-row-edit-tags mono"
                            bind:value={editDraftTags}
                            placeholder="comma,separated,tags"
                            spellcheck="false"
                          />
                        </label>
                      </div>
                      <div class="mem-row-edit-actions">
                        <button class="btn btn--ghost btn--sm" onclick={cancelEditMemRow} disabled={editBusy}>Cancel</button>
                        <button class="btn btn--primary btn--sm" onclick={saveEditMemRow} disabled={editBusy}>
                          {editBusy ? 'Saving…' : 'Save'}
                        </button>
                      </div>
                    </div>
                  {:else}
                    <div class="mem-row-body">{memRowPreview(row.content)}</div>
                  {/if}
                </div>
              {/each}
            {/if}
          </div>
        </div>
      {/if}
    </div>

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
      {#if updatesStore.settings.skipped_version}
        <div class="update-skip-row">
          <span class="update-skip-label">Skipped version</span>
          <span class="mono">{updatesStore.settings.skipped_version}</span>
          <button class="btn-link" onclick={clearSkippedVersion}>clear skip</button>
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

    <!-- Hooks (Claude Code parity §2). User-defined scripts wired
         into agent lifecycle events. Config is a single JSON file
         under <app_data>/hooks.json; this card edits it in place.
         Validation runs before save — malformed JSON is rejected
         with a inline error message so a hand-typed bug doesn't
         silently wipe the existing config. -->
    <div class="card">
      <header class="card-head">
        <h2 class="card-title">Hooks</h2>
        <p class="card-sub">
          Wire shell scripts into agent lifecycle events. Currently supported:
          <span class="mono">UserPromptSubmit</span>, <span class="mono">Stop</span>,
          <span class="mono">SessionStart</span>. Each handler reads JSON on stdin and may
          rewrite the prompt, attach context, or block the action via exit code 2.
        </p>
      </header>
      <div class="hooks-actions">
        <span class="card-sub">{hookCount} hook{hookCount === 1 ? '' : 's'} enabled</span>
        <button
          class="btn btn--ghost"
          onclick={() => void resetHooksDraft()}
          disabled={hooksLoading || hooksDraft === hooksDraftPristine}
          title="Revert unsaved edits"
        >Revert</button>
        <button
          class="btn"
          onclick={() => void saveHooksDraft()}
          disabled={hooksLoading || hooksDraft === hooksDraftPristine || !!hooksParseError}
        >{hooksLoading ? 'Saving…' : 'Save'}</button>
      </div>
      <textarea
        class="hooks-editor mono"
        bind:value={hooksDraft}
        placeholder={hooksPlaceholder}
        spellcheck="false"
        rows="12"
      ></textarea>
      {#if hooksParseError}
        <div class="hooks-error mono">{hooksParseError}</div>
      {/if}
      <p class="card-sub hooks-hint">
        Schema:
        <span class="mono">{`{ "hooks": { "<EventName>": [ { "matcher": "*", "handler": { "type": "command", "command": "/path/to/script" }, "timeout_ms": 5000, "disabled": false } ] } }`}</span>.
        Script stdin = event JSON. Stdout JSON keys honored:
        <span class="mono">updated_prompt</span> (UserPromptSubmit rewrite, deferred wiring),
        <span class="mono">additional_context</span>, <span class="mono">reason</span>.
        Exit 2 = block.
      </p>
    </div>

    <!-- MCP servers diagnostic (M4 §2.9.8) -->
    <div class="card">
      <header class="card-head">
        <h2 class="card-title">MCP servers</h2>
        <p class="card-sub">
          Woom's bundled sidecars. Spawned by Claude / Cursor on first MCP handshake; "not running" means no agent has talked to that sidecar yet this launch.
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
          The full Woom spec set, bundled with the app. Each entry is rendered from <span class="mono">docs/*.md</span> in the repo. Pick one to read inline, or open the file in Finder.
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
          <div class="stat-value mono">{appVersionLabel} · macOS</div>
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
  .settings-view {
    overflow-y: auto; flex: 1;
    display: flex; flex-direction: column;
    padding: 30px 60px 60px;
    background: var(--bg-0);
  }
  .settings-header { padding: 8px 0 28px; max-width: 880px; margin: 0 auto; width: 100%; }
  .view-title {
    font-family: 'Geist', 'Inter', -apple-system, system-ui, sans-serif;
    font-size: 38px; font-weight: 600;
    
    letter-spacing: -0.02em;
    color: var(--text-0);
    margin: 0 0 6px;
  }
  .view-sub { font-size: 14px; color: var(--text-2); margin: 0; line-height: 1.5; }

  .settings-body {
    padding: 0; max-width: 880px; margin: 0 auto; width: 100%;
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
    background: rgba(232, 130, 100, 0.08);
    border-color: rgba(232, 130, 100, 0.4);
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
    box-shadow: 0 0 0 2px rgba(245, 240, 234, 0.08);
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

  /* Memory panel — kind breakdown chips. Same vibe as theme swatches:
     small, hover-able, no border by default to keep the row visually
     calm. The number is dominant; the kind label is mute support. */
  .mem-breakdown {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin-top: 12px;
  }
  .mem-chip {
    display: inline-flex;
    align-items: baseline;
    gap: 6px;
    padding: 5px 10px;
    background: var(--bg-2);
    border: 1px solid var(--border-neutral);
    border-radius: 999px;
    font-size: 11px;
  }
  .mem-chip-kind { color: var(--text-mute); text-transform: capitalize; }
  .mem-chip-n { color: var(--text-0); font-weight: 600; font-variant-numeric: tabular-nums; }

  /* Memory browser. Toggle-revealed below the stats panel — a search
     row + scrollable list of rows. Each row is a thin card with
     metadata header + truncated content preview + delete affordance
     on the right. No backing-store contract beyond what the Tauri
     commands expose; the user can prune false-positives or stale
     entries without touching the DB directly. */
  .mem-browser {
    margin-top: 14px;
    padding-top: 14px;
    border-top: 1px dashed var(--border);
  }
  .mem-browser-controls {
    display: flex;
    gap: 8px;
    align-items: center;
  }
  .mem-browser-search {
    flex: 1;
    min-width: 0;
    height: 30px;
    padding: 0 10px;
    background: var(--bg-2);
    border: 1px solid var(--border-neutral);
    border-radius: 6px;
    color: var(--text-0);
    font-size: 12.5px;
  }
  .mem-browser-search:focus { outline: 1px solid var(--accent); outline-offset: -1px; }
  .mem-browser-kind {
    height: 30px;
    padding: 0 8px;
    background: var(--bg-2);
    border: 1px solid var(--border-neutral);
    border-radius: 6px;
    color: var(--text-0);
    font-size: 12px;
  }
  .mem-browser-kind:disabled { opacity: 0.5; cursor: not-allowed; }
  .mem-browser-list {
    margin-top: 10px;
    max-height: 420px;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .mem-browser-empty {
    padding: 32px 12px;
    text-align: center;
    color: var(--text-mute);
    font-size: 12px;
  }
  .mem-row {
    padding: 8px 10px;
    background: var(--bg-2);
    border: 1px solid var(--border);
    border-radius: 6px;
    font-size: 12px;
  }
  .mem-row-head {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
    margin-bottom: 4px;
  }
  .mem-row-id { color: var(--text-mute); font-size: 11px; }
  .mem-row-kind {
    font-size: 10.5px;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--accent-bright);
    background: color-mix(in srgb, var(--accent) 12%, transparent);
    padding: 1px 5px;
    border-radius: 3px;
  }
  .mem-row-date { color: var(--text-mute); font-size: 10.5px; }
  .mem-row-tags {
    flex: 1;
    min-width: 0;
    font-size: 10.5px;
    color: var(--text-2);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .mem-row-del,
  .mem-row-edit {
    width: 24px; height: 24px;
    display: grid; place-items: center;
    background: transparent;
    border: 0;
    border-radius: 4px;
    color: var(--text-mute);
    cursor: pointer;
    transition: color 120ms, background 120ms;
  }
  /* The first action button on a row pushes itself + every sibling
     button to the right edge — keeps the layout the same whether
     only Delete is visible or Edit + Delete are. */
  .mem-row-edit { margin-left: auto; }
  .mem-row-edit:hover {
    color: var(--accent-bright);
    background: color-mix(in srgb, var(--accent) 14%, transparent);
  }
  /* When only Delete is shown (editing in progress) it needs the same
     margin-left so the row layout stays stable. */
  .mem-row:not(.mem-row--editing) .mem-row-del {
    margin-left: 0;
  }
  .mem-row--editing .mem-row-del {
    margin-left: auto;
  }
  .mem-row-del:hover {
    color: var(--error, #e88264);
    background: color-mix(in srgb, var(--error, #e88264) 14%, transparent);
  }
  /* Inline edit form — only one row at a time. Textarea takes the
     full width; kind + tags share a flex row below. */
  .mem-row-edit-form {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin-top: 6px;
  }
  .mem-row-edit-content {
    width: 100%;
    min-height: 100px;
    padding: 8px 10px;
    background: var(--bg-1);
    border: 1px solid var(--border);
    border-radius: 6px;
    color: var(--text-0);
    font-size: 12.5px;
    line-height: 1.55;
    resize: vertical;
  }
  .mem-row-edit-content:focus {
    outline: 1px solid var(--accent);
    outline-offset: -1px;
  }
  .mem-row-edit-row { display: flex; gap: 8px; align-items: flex-end; }
  .mem-row-edit-field { display: flex; flex-direction: column; gap: 3px; }
  .mem-row-edit-field--grow { flex: 1; min-width: 0; }
  .mem-row-edit-label {
    font-size: 10.5px;
    color: var(--text-mute);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }
  .mem-row-edit-kind,
  .mem-row-edit-tags {
    height: 28px;
    padding: 0 8px;
    background: var(--bg-1);
    border: 1px solid var(--border);
    border-radius: 5px;
    color: var(--text-0);
    font-size: 12px;
  }
  .mem-row-edit-actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
  .mem-row--editing {
    border-color: var(--accent);
    box-shadow: 0 0 0 1px var(--accent);
  }
  .mem-row-body {
    color: var(--text-1);
    line-height: 1.55;
    word-break: break-word;
  }

  .btn--sm {
    height: 30px;
    padding: 0 10px;
    font-size: 11.5px;
  }

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
  .diag-row--rate_limited .diag-kind { color: #D9B86E; }
  .diag-row--error .diag-kind { color: #F0A38A; }
  .diag-empty {
    padding: 16px; border: 1px dashed var(--border-neutral);
    border-radius: 8px; text-align: center;
    font-size: 12px; color: var(--text-mute);
  }

  /* Updates card */
  .update-actions { display: flex; gap: 8px; flex-wrap: wrap; }

  /* Hooks editor — monospace textarea with inline validation. */
  .hooks-actions {
    display: flex; align-items: center;
    gap: 10px;
    margin-bottom: 8px;
  }
  .hooks-actions .card-sub { margin-right: auto; }
  .hooks-editor {
    width: 100%;
    padding: 10px 12px;
    border-radius: 8px;
    border: 1px solid var(--border);
    background: var(--bg-0);
    color: var(--text-0);
    font: 11.5px / 1.55 'JetBrains Mono', ui-monospace, monospace;
    resize: vertical;
    min-height: 180px;
    transition: border-color 120ms;
  }
  .hooks-editor:focus {
    outline: none;
    border-color: var(--accent);
  }
  .hooks-error {
    margin-top: 6px;
    padding: 6px 10px;
    background: color-mix(in srgb, var(--error) 10%, transparent);
    border: 1px solid color-mix(in srgb, var(--error) 50%, transparent);
    border-radius: 5px;
    color: var(--error);
    font-size: 11px;
    line-height: 1.4;
  }
  .hooks-hint {
    margin-top: 8px;
    line-height: 1.5;
  }
  .update-status {
    padding: 10px 12px;
    border-radius: 8px;
    background: var(--bg-2);
    border: 1px solid var(--border-neutral);
    color: var(--text-1);
    font-size: 12.5px; line-height: 1.5;
  }
  .update-status--ready {
    background: rgba(168, 217, 184, 0.08);
    border-color: rgba(168, 217, 184, 0.35);
    color: var(--accent-bright);
  }
  .update-status--installed {
    background: rgba(168, 217, 184, 0.14);
    border-color: rgba(168, 217, 184, 0.5);
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
