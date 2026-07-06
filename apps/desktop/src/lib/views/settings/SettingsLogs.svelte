<script lang="ts">
  /* Settings → Logs. Companion to the backend `logging.rs` module:
   * the app appends everything (Rust error paths, panics, webview
   * console.error/warn, uncaught JS errors) to
   * `<app_data>/logs/woom.log` (5 MB rotation → `woom.log.1`).
   * This card lets the user view the tail inline, reveal the file in
   * Finder, copy the path, and clear the log — everything needed to
   * attach it to a bug report. */
  import { invoke } from '@tauri-apps/api/core';
  import { notify, notifyError } from '$lib/state/toaster.svelte';

  const TAIL_LINES = 400;

  let logPath = $state('');
  let tail = $state<string | null>(null);
  let busy = $state(false);
  let copiedAt = $state<number | null>(null);

  $effect(() => {
    void (async () => {
      try {
        logPath = await invoke<string>('log_path');
      } catch {
        /* command missing — leave path empty, buttons still render */
      }
    })();
  });

  async function refreshTail() {
    busy = true;
    try {
      tail = await invoke<string>('log_tail', { lines: TAIL_LINES });
    } catch (e) {
      notifyError(e, { title: 'Read log failed' });
    } finally {
      busy = false;
    }
  }

  function toggleView() {
    if (tail === null) void refreshTail();
    else tail = null;
  }

  async function copyPath() {
    if (!logPath) return;
    try {
      await navigator.clipboard.writeText(logPath);
      copiedAt = Date.now();
      notify({ kind: 'success', title: 'Log path copied', body: logPath });
    } catch (e) {
      notifyError(e, { title: 'Copy failed' });
    }
  }

  async function reveal() {
    try {
      await invoke('log_reveal');
    } catch (e) {
      notifyError(e, { title: 'Reveal failed' });
    }
  }

  async function clearLog() {
    busy = true;
    try {
      await invoke('log_clear');
      notify({ kind: 'success', title: 'Log cleared' });
      if (tail !== null) await refreshTail();
    } catch (e) {
      notifyError(e, { title: 'Clear failed' });
    } finally {
      busy = false;
    }
  }
</script>

<!-- App log -->
<div class="card">
  <header class="card-head">
    <h2 class="card-title">Logs</h2>
    <p class="card-sub">
      Everything noteworthy — backend errors, panics, and webview console errors — lands in one
      file. Attach it to a bug report or send it our way when something misbehaves. Rotates at
      5&nbsp;MB, older generation kept as <span class="mono">woom.log.1</span>.
    </p>
  </header>

  <button
    class="log-path"
    type="button"
    title="Click to copy path"
    onclick={copyPath}
    disabled={!logPath}
  >
    <span class="log-path-text">{logPath || 'resolving…'}</span>
    <span class="log-path-hint">
      {copiedAt && Date.now() - copiedAt < 4000 ? 'copied!' : 'copy'}
    </span>
  </button>

  <div class="card-actions">
    <button class="btn btn--ghost" onclick={toggleView} disabled={busy}>
      {tail === null ? 'View' : 'Hide'}
    </button>
    <button class="btn btn--ghost" onclick={reveal}>Reveal in Finder</button>
    {#if tail !== null}
      <button class="btn btn--ghost" onclick={() => void refreshTail()} disabled={busy}>
        Refresh
      </button>
    {/if}
    <button class="btn btn--ghost log-clear" onclick={clearLog} disabled={busy}>Clear</button>
  </div>

  {#if tail !== null}
    <pre class="log-pre">{tail.trim() ? tail : 'Log is empty.'}</pre>
  {/if}
</div>

<style>
  .log-path {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    padding: 8px 12px;
    background: var(--bg-0);
    border: 1px solid var(--border-neutral);
    border-radius: 8px;
    cursor: pointer;
    text-align: left;
    transition: border-color 120ms, background 120ms;
  }
  .log-path:hover:not(:disabled) {
    background: var(--bg-2);
    border-color: var(--border-hi2, var(--border-neutral));
  }
  .log-path:disabled {
    cursor: default;
    opacity: 0.6;
  }
  .log-path-text {
    flex: 1;
    min-width: 0;
    font-family: var(--font-mono, ui-monospace, monospace);
    font-size: 11.5px;
    color: var(--text-1);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .log-path-hint {
    font-size: 10.5px;
    color: var(--text-mute);
    text-transform: uppercase;
    letter-spacing: 0.05em;
    flex-shrink: 0;
  }
  .log-clear:hover {
    color: var(--error, #e88264);
  }
  .log-pre {
    margin: 0;
    padding: 10px 12px;
    background: var(--bg-0);
    border: 1px solid var(--border-neutral);
    border-radius: 8px;
    font-family: var(--font-mono, ui-monospace, monospace);
    font-size: 11px;
    line-height: 1.5;
    color: var(--text-1);
    max-height: 360px;
    overflow: auto;
    white-space: pre-wrap;
    word-break: break-word;
  }
</style>
