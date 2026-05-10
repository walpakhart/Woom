<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';

  interface ChangedFile { path: string; status: string; additions: number; deletions: number; }
  interface Payload { files: ChangedFile[]; raw: string; }

  interface Props {
    repo: string;
    sessionId: string;
    branch: string;
    onClose: () => void;
  }
  let { repo, sessionId, branch, onClose }: Props = $props();

  let files = $state<ChangedFile[]>([]);
  let raw = $state<string>('');
  let loading = $state(true);
  let error = $state<string | null>(null);

  async function load() {
    loading = true;
    error = null;
    try {
      const p = await invoke<Payload>('worktree_diff', {
        repo,
        sessionId,
        baseRef: null
      });
      files = p.files;
      raw = p.raw;
    } catch (e) {
      error = typeof e === 'string' ? e : String(e);
    } finally {
      loading = false;
    }
  }

  $effect(() => { void load(); });

  const totals = $derived.by(() => {
    let add = 0, del = 0;
    for (const f of files) { add += f.additions; del += f.deletions; }
    return { add, del };
  });

  function statusLabel(s: string): { short: string; cls: string; title: string } {
    switch (s) {
      case 'A': return { short: 'A', cls: 'wdf-s-add', title: 'Added' };
      case 'M': return { short: 'M', cls: 'wdf-s-mod', title: 'Modified' };
      case 'D': return { short: 'D', cls: 'wdf-s-del', title: 'Deleted' };
      case 'R': return { short: 'R', cls: 'wdf-s-ren', title: 'Renamed' };
      case 'T': return { short: 'T', cls: 'wdf-s-mod', title: 'Type changed' };
      case 'C': return { short: 'C', cls: 'wdf-s-add', title: 'Copied' };
      default: return { short: s || '?', cls: 'wdf-s-mod', title: s };
    }
  }

  interface Line { kind: 'add' | 'del' | 'hunk' | 'meta' | 'ctx'; text: string; }
  const lines = $derived.by<Line[]>(() => {
    if (!raw) return [];
    const out: Line[] = [];
    for (const r of raw.split('\n')) {
      if (r.startsWith('@@')) out.push({ kind: 'hunk', text: r });
      else if (r.startsWith('+') && !r.startsWith('+++')) out.push({ kind: 'add', text: r.slice(1) });
      else if (r.startsWith('-') && !r.startsWith('---')) out.push({ kind: 'del', text: r.slice(1) });
      else if (r.startsWith('diff ') || r.startsWith('index ') || r.startsWith('+++ ') || r.startsWith('--- ') || r.startsWith('new file') || r.startsWith('deleted file') || r.startsWith('similarity') || r.startsWith('rename ') || r.startsWith('\\')) out.push({ kind: 'meta', text: r });
      else out.push({ kind: 'ctx', text: r.startsWith(' ') ? r.slice(1) : r });
    }
    return out;
  });
</script>

<div class="wdf-backdrop" onclick={(e) => { if (e.target === e.currentTarget) onClose(); }} onkeydown={(e) => { if (e.key === 'Escape') onClose(); }} role="dialog" aria-modal="true" tabindex="-1">
  <div class="wdf">
    <header class="wdf-head">
      <div class="wdf-title-wrap">
        <svg class="i i-sm" viewBox="0 0 24 24"><path d="M6 3v18M6 9a3 3 0 0 0 3 3h4a3 3 0 0 1 3 3v6M18 3a3 3 0 1 1 0 6 3 3 0 0 1 0-6z"/></svg>
        <span class="wdf-title mono">{branch}</span>
        <span class="wdf-stats mono">
          <span class="wdf-add">+{totals.add}</span>
          <span class="wdf-del">−{totals.del}</span>
          <span class="wdf-count">{files.length} file{files.length === 1 ? '' : 's'}</span>
        </span>
      </div>
      <button class="wdf-close" onclick={onClose} aria-label="Close">
        <svg class="i" viewBox="0 0 24 24"><path d="M18 6 6 18M6 6l12 12"/></svg>
      </button>
    </header>

    {#if loading}
      <div class="wdf-state">Loading diff…</div>
    {:else if error}
      <div class="wdf-state wdf-err">{error}</div>
    {:else if files.length === 0}
      <div class="wdf-state">No changes between the base branch and this worktree yet.</div>
    {:else}
      <div class="wdf-body">
        <aside class="wdf-files">
          <div class="wdf-section-head">Files</div>
          {#each files as f (f.path)}
            {@const s = statusLabel(f.status)}
            <a class="wdf-file" href="#wdf-{f.path}" title={f.path}>
              <span class="wdf-status {s.cls}" title={s.title}>{s.short}</span>
              <span class="wdf-file-path mono">{f.path}</span>
              <span class="wdf-file-delta mono">
                <span class="wdf-add">+{f.additions}</span>
                <span class="wdf-del">−{f.deletions}</span>
              </span>
            </a>
          {/each}
        </aside>
        <div class="wdf-diff">
          {#each lines as line, i (i)}
            <div class="wdf-row wdf-row--{line.kind}">
              <span class="wdf-marker">
                {#if line.kind === 'add'}+{:else if line.kind === 'del'}−{:else if line.kind === 'hunk'}…{:else}&nbsp;{/if}
              </span>
              <span class="wdf-text mono">{line.text || ' '}</span>
            </div>
          {/each}
        </div>
      </div>
    {/if}
  </div>
</div>

<style>
  .wdf-backdrop {
    position: fixed; inset: 0;
    background: rgba(0, 0, 0, 0.55);
    backdrop-filter: blur(4px);
    z-index: 100;
    display: flex; align-items: stretch; justify-content: center;
    padding: 24px;
  }
  .wdf {
    width: 100%; max-width: 1200px;
    background: var(--bg-0);
    border: 1px solid var(--border-hi);
    border-radius: 12px;
    display: flex; flex-direction: column;
    box-shadow: 0 20px 60px rgba(0, 0, 0, 0.5);
    overflow: hidden;
  }
  .wdf-head {
    display: flex; align-items: center; justify-content: space-between;
    gap: 12px;
    padding: 12px 18px;
    background: var(--bg-1);
    border-bottom: 1px solid var(--border-neutral);
  }
  .wdf-title-wrap { display: inline-flex; align-items: center; gap: 10px; min-width: 0; }
  .wdf-title { color: var(--accent-bright); font-weight: 600; font-size: 13px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .wdf-stats { display: inline-flex; gap: 10px; font-size: 11.5px; color: var(--text-2); }
  .wdf-add { color: var(--success); }
  .wdf-del { color: var(--error); }
  .wdf-count { color: var(--text-2); margin-left: 4px; }
  .wdf-close {
    width: 28px; height: 28px; border-radius: 5px;
    display: inline-flex; align-items: center; justify-content: center;
    color: var(--text-1);
  }
  .wdf-close:hover { background: var(--bg-3); color: var(--text-0); }

  .wdf-state { padding: 40px; text-align: center; color: var(--text-2); font-size: 13px; }
  .wdf-err { color: var(--error); }

  .wdf-body { display: grid; grid-template-columns: 280px 1fr; flex: 1; min-height: 0; }

  .wdf-files {
    overflow-y: auto;
    border-right: 1px solid var(--border-neutral);
    background: var(--bg-1);
    padding: 6px 0 24px;
  }
  .wdf-section-head {
    padding: 8px 14px 6px;
    font-size: 10.5px; color: var(--text-2);
    text-transform: uppercase; letter-spacing: 0.06em;
  }
  .wdf-file {
    display: grid; grid-template-columns: 24px 1fr auto;
    align-items: center; gap: 8px;
    padding: 5px 12px;
    font-size: 12px; color: var(--text-1);
    text-decoration: none;
    border-left: 2px solid transparent;
    transition: background 100ms;
  }
  .wdf-file:hover { background: var(--bg-2); color: var(--text-0); border-left-color: var(--accent); }
  .wdf-status {
    width: 20px; height: 18px; border-radius: 3px;
    display: inline-flex; align-items: center; justify-content: center;
    font-size: 10.5px; font-weight: 600;
    flex-shrink: 0;
  }
  .wdf-s-add { color: var(--success); background: rgba(204, 120, 92, 0.15); }
  .wdf-s-mod { color: var(--warning); background: rgba(217, 184, 110, 0.15); }
  .wdf-s-del { color: var(--error); background: rgba(232, 130, 100, 0.18); }
  .wdf-s-ren { color: var(--accent); background: var(--accent-soft); }
  .wdf-file-path { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .wdf-file-delta { font-size: 10.5px; display: inline-flex; gap: 6px; flex-shrink: 0; }

  .wdf-diff {
    overflow: auto;
    font-family: 'JetBrains Mono', ui-monospace, 'SF Mono', monospace;
    font-size: 12.5px; line-height: 1.55;
    padding: 8px 0 40px;
  }
  .wdf-row { display: flex; white-space: pre; min-height: 18px; }
  .wdf-row--add { background: rgba(204, 120, 92, 0.10); }
  .wdf-row--add .wdf-marker, .wdf-row--add .wdf-text { color: var(--success); }
  .wdf-row--del { background: rgba(232, 130, 100, 0.10); }
  .wdf-row--del .wdf-marker, .wdf-row--del .wdf-text { color: var(--error); }
  .wdf-row--hunk { background: rgba(232, 130, 100, 0.08); color: var(--accent-bright);  }
  .wdf-row--meta { color: var(--text-2);  }
  .wdf-row--ctx { color: var(--text-1); }
  .wdf-marker { width: 18px; text-align: center; user-select: none; font-weight: 600; flex-shrink: 0; }
  .wdf-text { flex: 1; padding-right: 18px; }
</style>
