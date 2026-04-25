<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import { sessionsState, persistError, SESSIONS_STORAGE_KEY, RULES_STORAGE_KEY } from '$lib/state/sessions.svelte';
  import { layoutState } from '$lib/state/layout.svelte';
  import { notify, notifyError } from '$lib/state/toaster.svelte';

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
      await openUrl(`file://${worktreeDir}`);
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

    <!-- Build / app info -->
    <div class="card">
      <header class="card-head">
        <h2 class="card-title">App</h2>
      </header>
      <div class="grid">
        <div class="stat">
          <div class="stat-label">Build</div>
          <div class="stat-value mono">Forgehold 0.1.0 · aarch64</div>
        </div>
        <div class="stat">
          <div class="stat-label">Storage keys</div>
          <div class="stat-value mono">{SESSIONS_STORAGE_KEY}, {RULES_STORAGE_KEY}</div>
        </div>
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
</style>
