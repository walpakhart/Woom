<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';

  /* Compact commit-log panel for the sidebar's "Git" tab. Uses the
     existing `git_log` Tauri command (returns short_sha / author /
     date / subject) so this is a pure UI add — no backend work. */

  interface CommitEntry {
    sha: string;
    short_sha: string;
    author: string;
    date: string;
    subject: string;
  }

  interface Props {
    repo: string;
    /** Bumps from the parent whenever something changed in git
     *  (commit landed, branch switched, fs watcher fired). Used as a
     *  reactive dep so the log re-fetches without a manual refresh
     *  click. */
    refreshKey?: number;
  }
  let { repo, refreshKey = 0 }: Props = $props();

  const PAGE = 30;

  let commits = $state<CommitEntry[]>([]);
  let loading = $state(false);
  let error = $state<string | null>(null);

  async function load() {
    if (!repo) {
      commits = [];
      return;
    }
    loading = true;
    error = null;
    try {
      commits = await invoke<CommitEntry[]>('git_log', { repo, limit: PAGE });
    } catch (e) {
      error = typeof e === 'string' ? e : String(e);
    } finally {
      loading = false;
    }
  }

  /* Re-fetch on repo change OR when the parent bumps refreshKey
     (e.g. after a successful commit / push / pull). */
  $effect(() => {
    repo;
    refreshKey;
    void load();
  });

  /* Local relative-time formatter — keeps the panel self-contained
     instead of importing the inbox utility (which expects a unix
     timestamp; git's ISO string is friendlier to parse here). */
  function rel(iso: string): string {
    const t = new Date(iso).getTime();
    if (!Number.isFinite(t)) return iso;
    const d = (Date.now() - t) / 1000;
    if (d < 60) return 'just now';
    if (d < 3600) return `${Math.floor(d / 60)}m ago`;
    if (d < 86400) return `${Math.floor(d / 3600)}h ago`;
    if (d < 86400 * 30) return `${Math.floor(d / 86400)}d ago`;
    if (d < 86400 * 365) return `${Math.floor(d / (86400 * 30))}mo ago`;
    return `${Math.floor(d / (86400 * 365))}y ago`;
  }

  async function copySha(sha: string) {
    try {
      await navigator.clipboard.writeText(sha);
    } catch {/* ignore */}
  }
</script>

<div class="hp">
  <div class="hp-head">
    <span class="hp-title">History</span>
    <button class="hp-refresh" onclick={() => void load()} disabled={loading} title="Refresh">
      <svg class="i i-sm" viewBox="0 0 24 24" style="transform: rotate({loading ? 360 : 0}deg); transition: transform 0.6s;">
        <path d="M21 12a9 9 0 0 1-9 9 9 9 0 0 1-8.5-6"/>
        <path d="M3 12a9 9 0 0 1 9-9 9 9 0 0 1 8.5 6"/>
        <polyline points="21 3 21 9 15 9"/>
        <polyline points="3 21 3 15 9 15"/>
      </svg>
    </button>
  </div>
  {#if error}
    <div class="hp-state hp-state--err">{error}</div>
  {:else if loading && commits.length === 0}
    <div class="hp-state">Loading…</div>
  {:else if commits.length === 0}
    <div class="hp-state">No commits yet on this branch.</div>
  {:else}
    <div class="hp-list">
      {#each commits as c (c.sha)}
        <div class="hp-row" title={`${c.sha}\n${c.author}\n${c.date}`}>
          <button class="hp-sha mono" onclick={() => void copySha(c.sha)} title="Copy full SHA">
            {c.short_sha}
          </button>
          <div class="hp-body">
            <div class="hp-subject">{c.subject}</div>
            <div class="hp-meta mono">
              <span class="hp-author">{c.author}</span>
              <span class="hp-time">{rel(c.date)}</span>
            </div>
          </div>
        </div>
      {/each}
    </div>
  {/if}
</div>

<style>
  .hp { display: flex; flex-direction: column; height: 100%; min-height: 0; }
  .hp-head {
    display: flex; align-items: center; justify-content: space-between;
    padding: 6px 10px;
    border-bottom: 1px solid var(--border-neutral);
    flex-shrink: 0;
  }
  .hp-title {
    font-size: 10.5px; font-weight: 600; letter-spacing: 0.06em;
    text-transform: uppercase; color: var(--text-mute);
  }
  .hp-refresh {
    width: 22px; height: 22px; border-radius: 4px;
    color: var(--text-2); display: inline-flex; align-items: center; justify-content: center;
  }
  .hp-refresh:hover:not(:disabled) { background: var(--bg-3); color: var(--text-0); }
  .hp-refresh:disabled { opacity: 0.4; cursor: not-allowed; }
  .hp-refresh :global(svg) { width: 12px; height: 12px; }

  .hp-state { padding: 14px 12px; font-size: 11.5px; color: var(--text-2); text-align: center; }
  .hp-state--err { color: var(--error); }

  .hp-list { flex: 1; overflow-y: auto; padding: 4px 0; }
  .hp-row {
    display: flex; align-items: flex-start; gap: 8px;
    padding: 6px 10px;
    border-bottom: 1px solid rgba(255, 255, 255, 0.03);
    transition: background 100ms;
  }
  .hp-row:hover { background: var(--bg-2); }
  .hp-sha {
    flex-shrink: 0;
    font-size: 10.5px; color: var(--accent-bright);
    background: var(--accent-soft);
    border: 1px solid rgba(232, 163, 58, 0.18);
    border-radius: 3px;
    padding: 1px 5px;
    cursor: pointer;
    line-height: 1.4;
  }
  .hp-sha:hover { background: rgba(232, 163, 58, 0.18); }
  .hp-body { flex: 1; min-width: 0; }
  .hp-subject {
    font-size: 12px; color: var(--text-0); line-height: 1.35;
    overflow: hidden; text-overflow: ellipsis;
    display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical;
  }
  .hp-meta {
    display: flex; gap: 8px;
    font-size: 10px; color: var(--text-mute);
    margin-top: 2px;
  }
  .hp-author { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; max-width: 140px; }
  .hp-time { flex-shrink: 0; }
</style>
