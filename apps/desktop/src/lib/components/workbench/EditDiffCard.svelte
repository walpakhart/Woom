<script lang="ts">
  /* Inline diff card for an Edit / MultiEdit chunk. Same UX shape as
     Cursor's chat: collapsed file pill ("a/b/c.ts ±N"), click to expand
     into a unified diff with red/green line gutters. Two buttons:
       - Revert: writes `oldText` back over `newText` in the file. We
         refuse the write if `newText` is no longer in the file or is
         multi-matched (see `revert_edit` Tauri command for the safety
         model). On success, the card flips to a `reverted` state and
         the button becomes "Reapply" — symmetrical, so the user can
         oscillate without losing the change.
       - Keep: simply collapses the card and dismisses Revert. Doesn't
         touch the file (the Edit was already applied — keeping is a
         no-op semantically).

     We deliberately don't use CodeMirror's MergeView here even though
     it sits in the same lib (used by the Editor's git-diff tab). That
     view spins up a full editor per side, which is fine for one open
     diff but would put 10× CodeMirror instances in the DOM if the
     agent did 10 edits this turn — measurable scroll lag. The
     hand-rolled LCS line diff below is ~50 lines and renders as cheap
     <div> rows. */

  import { invoke } from '@tauri-apps/api/core';
  import { updateEditEvent } from '$lib/state/sessions.svelte';
  import { notifyError } from '$lib/state/toaster.svelte';

  interface Props {
    sessionId: string;
    toolId: string;
    filePath: string;
    oldText: string;
    newText: string;
    isCreate: boolean;
    /** True for `Write` (full-file overwrite) — picks `revert_write`
     *  semantics + a "Wrote" verb. False for Edit / MultiEdit. */
    wholeFile: boolean;
    status: 'loading' | 'applied' | 'reverted' | 'error';
    note?: string;
  }
  let {
    sessionId,
    toolId,
    filePath,
    oldText,
    newText,
    isCreate,
    wholeFile,
    status,
    note
  }: Props = $props();

  let expanded = $state(false);
  let busy = $state(false);

  type DiffLine = { kind: 'same' | 'add' | 'del'; text: string };

  /* Line-level LCS so unchanged context lines stay grey and only
     true edits flash red/green. O(mn) DP — fine for typical Edit
     payloads (~tens of lines), and the agent never sends megabyte
     hunks because Edit's `old_string` requirement caps it. */
  function lineDiff(oldT: string, newT: string): DiffLine[] {
    const a = oldT.split('\n');
    const b = newT.split('\n');
    const m = a.length, n = b.length;
    const dp: number[][] = Array.from({ length: m + 1 }, () => new Array(n + 1).fill(0));
    for (let i = 1; i <= m; i++) {
      for (let j = 1; j <= n; j++) {
        dp[i][j] = a[i - 1] === b[j - 1] ? dp[i - 1][j - 1] + 1 : Math.max(dp[i - 1][j], dp[i][j - 1]);
      }
    }
    const out: DiffLine[] = [];
    let i = m, j = n;
    while (i > 0 || j > 0) {
      if (i > 0 && j > 0 && a[i - 1] === b[j - 1]) {
        out.unshift({ kind: 'same', text: a[i - 1] });
        i--; j--;
      } else if (j > 0 && (i === 0 || dp[i][j - 1] >= dp[i - 1][j])) {
        out.unshift({ kind: 'add', text: b[j - 1] });
        j--;
      } else {
        out.unshift({ kind: 'del', text: a[i - 1] });
        i--;
      }
    }
    return out;
  }

  const diff = $derived(lineDiff(oldText, newText));
  const stats = $derived.by(() => {
    let add = 0, del = 0;
    for (const d of diff) {
      if (d.kind === 'add') add++;
      else if (d.kind === 'del') del++;
    }
    return { add, del };
  });

  /** Last 2 path segments — same convention `formatToolUse` uses for
   *  read/write hints. Keeps long absolute paths from blowing out the
   *  pill width while still anchoring the user to "which file". */
  const shortPath = $derived.by(() => {
    const segs = filePath.split('/').filter(Boolean);
    return segs.length <= 2 ? filePath : `…/${segs.slice(-2).join('/')}`;
  });

  async function handleRevert() {
    if (busy) return;
    busy = true;
    try {
      if (wholeFile) {
        // `Write`: rewrite the full file (or delete it if the agent
        // created it from nothing). The Tauri side validates current
        // content matches `newText` so we don't trample post-Write
        // edits.
        await invoke('revert_write', {
          filePath,
          oldText,
          newText,
          isCreate
        });
      } else {
        await invoke('revert_edit', {
          filePath,
          oldText,
          newText
        });
      }
      updateEditEvent(sessionId, toolId, { status: 'reverted', note: undefined });
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : String(e);
      updateEditEvent(sessionId, toolId, { status: 'error', note: msg });
      notifyError(e, { title: `Revert failed for ${shortPath}` });
    } finally {
      busy = false;
    }
  }

  /** Reapply after a Revert: undo the undo. For Edit, swap the args and
   *  reuse `revert_edit`'s safety checks (it only knows "replace one
   *  literal occurrence with another", which is exactly what we need
   *  either direction). For Write, rewrite the file with `newText`
   *  (re-creating it if Revert deleted it). */
  async function handleReapply() {
    if (busy) return;
    busy = true;
    try {
      if (wholeFile) {
        if (isCreate) {
          // Revert deleted the file. Reapply re-creates it with the
          // original Write content. Bypass `revert_write` since its
          // pre-flight check expects the file to already match
          // `newText` — here it's missing entirely.
          await invoke('fs_write_file', { path: filePath, contents: newText });
        } else {
          // File still exists with `oldText`; flip back to `newText`
          // by reusing revert_write with swapped args. isCreate=false
          // because the file is on disk, not absent.
          await invoke('revert_write', {
            filePath,
            oldText: newText,
            newText: oldText,
            isCreate: false
          });
        }
      } else {
        await invoke('revert_edit', {
          filePath,
          oldText: newText,
          newText: oldText
        });
      }
      updateEditEvent(sessionId, toolId, { status: 'applied', note: undefined });
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : String(e);
      updateEditEvent(sessionId, toolId, { status: 'error', note: msg });
      notifyError(e, { title: `Reapply failed for ${shortPath}` });
    } finally {
      busy = false;
    }
  }
</script>

<div class="edit-card" class:edit-card--reverted={status === 'reverted'} class:edit-card--error={status === 'error'} class:edit-card--loading={status === 'loading'}>
  <button
    class="edit-head"
    onclick={() => (expanded = !expanded)}
    aria-expanded={expanded}
    title={expanded ? 'Collapse diff' : 'Show diff'}
  >
    <svg class="i i-sm edit-chevron" class:edit-chevron--open={expanded} viewBox="0 0 24 24"><path d="m9 18 6-6-6-6"/></svg>
    <span class="edit-icon" aria-hidden="true">
      {#if isCreate}
        <svg viewBox="0 0 24 24" width="14" height="14"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><polyline points="14 2 14 8 20 8"/><line x1="12" y1="18" x2="12" y2="12"/><line x1="9" y1="15" x2="15" y2="15"/></svg>
      {:else}
        <svg viewBox="0 0 24 24" width="14" height="14"><path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7"/><path d="m18.5 2.5 3 3L12 15l-4 1 1-4z"/></svg>
      {/if}
    </span>
    <span class="edit-path mono">{shortPath}</span>
    <span class="edit-stats mono">
      <span class="edit-add">+{stats.add}</span>
      <span class="edit-del">−{stats.del}</span>
    </span>
    <span class="edit-status">
      {#if status === 'loading'}loading…
      {:else if status === 'reverted'}reverted
      {:else if status === 'error'}error
      {:else if wholeFile && isCreate}created
      {:else if wholeFile}wrote
      {:else}applied{/if}
    </span>
  </button>

  {#if expanded}
    <div class="edit-body mono">
      {#each diff as line, i (i)}
        <div class="edit-line edit-line--{line.kind}">
          <span class="edit-line-marker">
            {#if line.kind === 'add'}+{:else if line.kind === 'del'}−{:else}{' '}{/if}
          </span>
          <span class="edit-line-text">{line.text || ' '}</span>
        </div>
      {/each}
    </div>
  {/if}

  {#if status === 'error' && note}
    <div class="edit-note">{note}</div>
  {/if}

  <div class="edit-actions">
    {#if status === 'loading'}
      <!-- Disabled placeholder — the git_show backfill is in flight,
           so the diff hasn't fully materialised. Keep the row in DOM
           rather than hiding it so the card height doesn't jump when
           backfill resolves. -->
      <button class="btn btn--ghost btn--small" disabled>Loading diff…</button>
    {:else if status === 'applied'}
      <button class="btn btn--ghost btn--small" onclick={handleRevert} disabled={busy}>
        {busy ? 'Reverting…' : 'Revert'}
      </button>
      <button class="btn btn--ghost btn--small" onclick={() => (expanded = false)} disabled={busy}>
        Keep
      </button>
    {:else if status === 'reverted'}
      <button class="btn btn--ghost btn--small" onclick={handleReapply} disabled={busy}>
        {busy ? 'Reapplying…' : 'Reapply'}
      </button>
    {:else if status === 'error'}
      <button class="btn btn--ghost btn--small" onclick={handleRevert} disabled={busy}>
        {busy ? 'Retrying…' : 'Retry revert'}
      </button>
    {/if}
  </div>
</div>

<style>
  .edit-card {
    border: 1px solid var(--border-neutral);
    border-radius: 8px;
    background: var(--bg-1);
    overflow: hidden;
    margin: 6px 0;
    font-size: 12.5px;
  }
  .edit-card--reverted { opacity: 0.72; }
  .edit-card--error { border-color: rgba(212, 102, 74, 0.5); }
  .edit-card--loading { opacity: 0.85; }

  .edit-head {
    display: flex; align-items: center; gap: 8px;
    width: 100%;
    padding: 7px 10px;
    background: transparent;
    border: 0;
    text-align: left;
    cursor: pointer;
    color: var(--text-1);
  }
  .edit-head:hover { background: var(--bg-2); }
  .edit-chevron {
    transition: transform 140ms;
    color: var(--text-2);
  }
  .edit-chevron--open { transform: rotate(90deg); }
  .edit-icon { display: inline-flex; align-items: center; color: var(--text-2); }
  .edit-icon svg { stroke: currentColor; fill: none; stroke-width: 2; stroke-linecap: round; stroke-linejoin: round; }
  .edit-path {
    flex: 1;
    color: var(--text-0);
    font-size: 12px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .edit-stats {
    color: var(--text-1);
    display: inline-flex;
    gap: 6px;
    font-size: 11px;
  }
  .edit-add { color: var(--success); }
  .edit-del { color: var(--error); }
  .edit-status {
    font-size: 10.5px;
    color: var(--text-mute);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    padding: 2px 6px;
    border-radius: 3px;
    background: var(--bg-2);
  }
  .edit-card--error .edit-status { color: var(--error); }
  .edit-card--reverted .edit-status { color: var(--text-2); }

  .edit-body {
    border-top: 1px solid var(--border-neutral);
    background: var(--bg-0);
    padding: 6px 0;
    max-height: 360px;
    overflow: auto;
    font-family: 'JetBrains Mono', ui-monospace, 'SF Mono', monospace;
    font-size: 11.5px;
    line-height: 1.55;
  }
  .edit-line {
    display: flex;
    padding: 0 10px;
    white-space: pre;
  }
  .edit-line-marker {
    width: 16px;
    flex-shrink: 0;
    color: var(--text-mute);
    user-select: none;
  }
  .edit-line-text {
    flex: 1;
    overflow: hidden;
  }
  .edit-line--add { background: rgba(111, 174, 136, 0.12); color: var(--text-0); }
  .edit-line--add .edit-line-marker { color: var(--success); }
  .edit-line--del { background: rgba(212, 102, 74, 0.12); color: var(--text-1); }
  .edit-line--del .edit-line-marker { color: var(--error); }
  .edit-line--same { color: var(--text-2); }

  .edit-note {
    padding: 6px 10px;
    font-size: 11.5px;
    color: var(--error);
    border-top: 1px solid var(--border-neutral);
    background: rgba(212, 102, 74, 0.06);
  }

  .edit-actions {
    display: flex;
    gap: 6px;
    padding: 6px 10px;
    border-top: 1px solid var(--border-neutral);
    background: var(--bg-1);
  }
</style>
