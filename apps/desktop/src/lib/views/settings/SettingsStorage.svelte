<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { openPath } from '@tauri-apps/plugin-opener';
  import { sessionsState, persistError, SESSIONS_STORAGE_KEY } from '$lib/state/sessions.svelte';
  import { layoutState } from '$lib/state/layout.svelte';
  import { notify, notifyError } from '$lib/state/toaster.svelte';
  import {
    connectionEventsState,
    clearConnectionEvents,
    type ConnectionEvent,
    type ConnectionEventSource
  } from '$lib/state/connectionEvents.svelte';
  import { relativeTime } from '$lib/data';
  import { formatBytes } from './shared';

  /** 14 days in seconds — matches the SPEC §worktrees retention rule. */
  const ORPHAN_AGE_SECS = 14 * 24 * 60 * 60;

  const claudeCount = $derived(sessionsState.list.filter((s) => s.agentKind === 'claude').length);
  const cursorCount = $derived(sessionsState.list.filter((s) => s.agentKind === 'cursor').length);
  const totalMessages = $derived(
    sessionsState.list.reduce((acc, s) => acc + s.messages.length, 0)
  );
  const editorLinkCount = $derived(Object.keys(layoutState.links.editorToAgent).length);
  const canvasLinkCount = $derived(Object.keys(layoutState.links.canvasToAgent).length);

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
      await openPath(worktreeDir);
    } catch (e) {
      notifyError(e, { title: 'Could not open folder' });
    }
  }

  $effect(() => { void refreshDiskStats(); });

  // ---- Connection diagnostics ----------------------------------------

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
</script>

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

<!-- Connection diagnostics -->
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
