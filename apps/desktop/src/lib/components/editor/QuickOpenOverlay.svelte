<script lang="ts">
  /* QuickOpenOverlay — ⌘P quick-open. Same modal shape as the
     CommandPalette / SearchInFilesOverlay so the muscle memory carries
     over (debounced input, ↑/↓, Enter to open, Esc to dismiss).
     Backend is `fs_walk_files` — the same Tauri command the @-mention
     picker leans on; bounded to ~2,000 files / depth 8 so a giant
     monorepo doesn't freeze the call. Ranking goes through
     `fuzzyScoreAny` so "shvelt" / "EdtrApp" still find what you mean.

     Why a separate overlay and not a CommandPalette section: the
     palette already paginates 6 results per section and weighs every
     match against MRU + pinned — overloading it with 200 file paths
     would push every other section off the screen. ⌘P users want one
     thing — open a file fast — so a focused modal reads cleaner. */
  import { invoke } from '@tauri-apps/api/core';
  import { sessionsState, requestEditorOpenFile } from '$lib/state/sessions.svelte';
  import { layoutState, setActiveInstance } from '$lib/state/layout.svelte';
  import { fuzzyScoreAny } from '$lib/services/fuzzyMatch';
  import { focusTrap } from '$lib/actions/focusTrap';
  import type { View } from '$lib/state/view.svelte';

  interface Props {
    open: boolean;
    setView: (v: View) => void;
  }
  let { open = $bindable(), setView }: Props = $props();

  type FileHit = { name: string; path: string; is_dir: boolean };
  type Row = { hit: FileHit; rel: string; score: number };

  let query = $state('');
  let selectedIdx = $state(0);
  let hits = $state<FileHit[]>([]);
  let busy = $state(false);
  let errMsg = $state<string | null>(null);
  let listEl: HTMLDivElement | null = $state(null);

  /* ── Editor scope picker (same vocabulary as SearchInFilesOverlay).
     We let users cycle between editor instances with non-empty
     repoPaths; the open file lands in the chosen one. */
  const editorInstances = $derived(layoutState.instances.editor ?? []);
  let targetEditorId = $state<string | null>(null);
  $effect(() => {
    if (!open) return;
    if (targetEditorId === null) targetEditorId = layoutState.activeInstance.editor;
  });
  const currentEditorId = $derived(targetEditorId ?? layoutState.activeInstance.editor);
  const currentEditor = $derived(
    editorInstances.find((i) => i.id === currentEditorId) ?? null
  );
  const repoPath = $derived(
    sessionsState.editorInstanceState[currentEditorId]?.repoPath ?? ''
  );
  const hasMultipleEditors = $derived(
    editorInstances.filter(
      (i) => (sessionsState.editorInstanceState[i.id]?.repoPath ?? '') !== ''
    ).length > 1
  );

  function cycleEditor() {
    const candidates = editorInstances.filter(
      (i) => (sessionsState.editorInstanceState[i.id]?.repoPath ?? '') !== ''
    );
    if (candidates.length <= 1) return;
    const idx = candidates.findIndex((i) => i.id === currentEditorId);
    const next = candidates[(idx + 1) % candidates.length];
    targetEditorId = next.id;
    setActiveInstance('editor', next.id);
    /* The walked-file cache is repo-scoped — invalidate so we re-load
       the new repo's tree on the next keystroke. */
    hits = [];
    void runWalk();
  }

  /* ── File walk. Cached per repo so re-opening the modal in the same
     repo is instant. The cached set is invalidated on every open
     (overlay can sit closed long enough that files churn under us). */
  let cachedRepo = '';
  async function runWalk() {
    if (!repoPath) {
      hits = [];
      return;
    }
    busy = true;
    errMsg = null;
    try {
      /* Use a generous cap — quick-open is the one place users expect
         the WHOLE repo to be reachable. The Rust side already skips
         node_modules / .git / dist by default. */
      const r = await invoke<FileHit[]>('fs_walk_files', {
        root: repoPath,
        query: null,
        maxFiles: 5000,
        maxDepth: 12
      });
      /* Filter directories — quick-open is for files. The same call
         is used by MentionPicker which keeps directories for repo
         root selection; there we filter on the consumer side too. */
      hits = r.filter((h) => !h.is_dir);
      cachedRepo = repoPath;
    } catch (e) {
      errMsg = e instanceof Error ? e.message : String(e);
      hits = [];
    } finally {
      busy = false;
    }
  }

  /* Re-walk on open + on repo change. */
  $effect(() => {
    if (!open) return;
    if (cachedRepo !== repoPath) void runWalk();
  });

  /* Pre-strip the repo prefix once so fuzzy matching scores against
     the relative path (what the user types). Memoized via $derived so
     a typing storm doesn't re-allocate. */
  const indexed = $derived.by(() => {
    const root = repoPath.endsWith('/') ? repoPath : repoPath + '/';
    return hits.map((h) => ({
      hit: h,
      rel: h.path.startsWith(root) ? h.path.slice(root.length) : h.path
    }));
  });

  const rows = $derived.by<Row[]>(() => {
    const q = query.trim();
    if (!q) {
      /* Empty query → show top 80 alphabetically by relative path so
         the modal is useful even before the user types. The 80 cap
         keeps the DOM cheap — typing immediately cuts to fuzzy hits. */
      return indexed
        .slice()
        .sort((a, b) => a.rel.localeCompare(b.rel))
        .slice(0, 80)
        .map((e) => ({ hit: e.hit, rel: e.rel, score: 0 }));
    }
    const out: Row[] = [];
    for (const e of indexed) {
      /* Score against name first (typically what users type), then
         the relative path for paths-only queries like "src/lib". */
      const score = fuzzyScoreAny(q, [e.hit.name, e.rel]);
      if (score === null) continue;
      out.push({ hit: e.hit, rel: e.rel, score });
    }
    out.sort((a, b) => b.score - a.score || a.rel.localeCompare(b.rel));
    /* 200 rows is enough — past that, the user should refine. */
    return out.slice(0, 200);
  });

  $effect(() => {
    void rows.length;
    selectedIdx = 0;
  });

  function close() {
    open = false;
    query = '';
    selectedIdx = 0;
    errMsg = null;
    targetEditorId = null;
  }

  function pickRow(row: Row) {
    requestEditorOpenFile(currentEditorId, row.hit.path);
    setView('editorApp');
    close();
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === 'Escape') { close(); return; }
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      selectedIdx = Math.min(selectedIdx + 1, Math.max(rows.length - 1, 0));
      return;
    }
    if (e.key === 'ArrowUp') {
      e.preventDefault();
      selectedIdx = Math.max(selectedIdx - 1, 0);
      return;
    }
    if (e.key === 'Enter') {
      e.preventDefault();
      const r = rows[selectedIdx];
      if (r) pickRow(r);
    }
  }

  /* Ensure the selected row scrolls into view when navigating with
     the keyboard past the viewport edge — same pattern as SFO. */
  $effect(() => {
    selectedIdx;
    queueMicrotask(() => {
      if (!listEl) return;
      const node = listEl.querySelector(`[data-idx="${selectedIdx}"]`);
      if (node && 'scrollIntoView' in node) {
        (node as HTMLElement).scrollIntoView({ block: 'nearest' });
      }
    });
  });

  /* Highlight matched chars inside the file name. Cheap subsequence
     walker (the fuzzy scorer doesn't expose its own positions). When
     a char doesn't match, we drop the highlight start anchor and try
     to resync — that catches typos / out-of-order chars without a
     second-pass DP. */
  function highlightFileName(name: string, q: string): Array<{ t: string; hit: boolean }> {
    if (!q) return [{ t: name, hit: false }];
    const out: Array<{ t: string; hit: boolean }> = [];
    const lowerName = name.toLowerCase();
    const lowerQ = q.toLowerCase();
    let qi = 0;
    let buf = '';
    let matchBuf = '';
    let inMatch = false;
    for (let i = 0; i < name.length; i++) {
      if (qi < lowerQ.length && lowerName[i] === lowerQ[qi]) {
        if (!inMatch) {
          if (buf) out.push({ t: buf, hit: false });
          buf = '';
          inMatch = true;
          matchBuf = '';
        }
        matchBuf += name[i];
        qi++;
      } else {
        if (inMatch) {
          out.push({ t: matchBuf, hit: true });
          matchBuf = '';
          inMatch = false;
          buf = '';
        }
        buf += name[i];
      }
    }
    if (inMatch && matchBuf) out.push({ t: matchBuf, hit: true });
    if (buf) out.push({ t: buf, hit: false });
    return out;
  }
</script>

{#if open}
  <div
    class="qo-backdrop"
    onclick={(e) => { if (e.target === e.currentTarget) close(); }}
    onkeydown={onKey}
    role="dialog"
    aria-modal="true"
    aria-label="Quick open file"
    tabindex="-1"
    use:focusTrap
  >
    <div class="qo">
      <div class="qo-head">
        <!-- svelte-ignore a11y_autofocus -->
        <input
          class="qo-input"
          bind:value={query}
          placeholder={repoPath ? 'Type a file name…' : 'Open a folder in the editor first…'}
          autofocus
          disabled={!repoPath}
        />
        {#if currentEditor}
          <button
            class="qo-scope"
            class:qo-scope-cyclable={hasMultipleEditors}
            onclick={cycleEditor}
            disabled={!hasMultipleEditors}
            title={hasMultipleEditors ? 'Click to cycle editor' : 'Only one editor open'}
            type="button"
          >
            <span class="qo-scope-name">{currentEditor.name}</span>
            <span class="qo-scope-sep">·</span>
            <span class="qo-scope-folder mono">
              {repoPath ? (repoPath.split('/').filter(Boolean).pop() ?? '') : '(no folder)'}
            </span>
            {#if hasMultipleEditors}<span class="qo-scope-cycle">⇄</span>{/if}
          </button>
        {/if}
        <span class="qo-meta mono">
          {#if busy}
            indexing…
          {:else if errMsg}
            <span class="qo-err">{errMsg}</span>
          {:else if hits.length === 0 && repoPath}
            no files
          {:else if rows.length === 0 && query.trim()}
            no matches
          {:else}
            {rows.length} of {hits.length}
          {/if}
        </span>
      </div>

      {#if !repoPath}
        <div class="qo-empty">
          The editor has no folder open. Choose one from the Editor view, then
          try ⌘P again.
        </div>
      {:else if rows.length === 0 && !busy && !query.trim()}
        <div class="qo-empty">
          Indexing <span class="mono">{repoPath.split('/').filter(Boolean).pop()}</span>…
        </div>
      {:else}
        <div class="qo-scroll" bind:this={listEl}>
          {#each rows as row, i (row.hit.path)}
            {@const dirPart = row.rel.slice(0, Math.max(0, row.rel.length - row.hit.name.length - 1))}
            <button
              class="qo-row"
              class:highlight={i === selectedIdx}
              data-idx={i}
              onmouseenter={() => (selectedIdx = i)}
              onclick={() => pickRow(row)}
              type="button"
            >
              <span class="qo-row-name mono">
                {#each highlightFileName(row.hit.name, query.trim()) as seg}
                  {#if seg.hit}<mark>{seg.t}</mark>{:else}{seg.t}{/if}
                {/each}
              </span>
              <span class="qo-row-dir mono">{dirPart || '/'}</span>
            </button>
          {/each}
        </div>
      {/if}

      <div class="qo-foot">
        <span class="grp"><span class="kbd">↑</span><span class="kbd">↓</span><span>navigate</span></span>
        <span class="grp"><span class="kbd">⏎</span><span>open</span></span>
        <span class="grp" style="margin-left: auto;"><span class="kbd">esc</span><span>close</span></span>
      </div>
    </div>
  </div>
{/if}

<style>
  /* Modal frame mirrors SearchInFilesOverlay exactly so the two
     surfaces feel like sister palettes — `qo-` prefix avoids style
     bleed when both render at once (e.g. user hits ⌘P then changes
     mind and hits ⌘⇧F). */
  .qo-backdrop {
    position: fixed; inset: 0;
    background: var(--backdrop);
    backdrop-filter: blur(22px) saturate(1.1);
    -webkit-backdrop-filter: blur(22px) saturate(1.1);
    display: flex; align-items: flex-start; justify-content: center;
    padding-top: 10vh; z-index: 200;
    animation: qoFade var(--dur-base) var(--ease-out);
  }
  .qo {
    width: 720px; max-width: 92vw;
    max-height: 70vh;
    display: flex; flex-direction: column;
    background: linear-gradient(180deg, var(--bg-2), var(--bg-1));
    border: 1px solid var(--border-hi);
    border-radius: var(--r-modal, 16px);
    overflow: hidden;
    box-shadow: var(--shadow-3), 0 0 0 1px var(--border-accent-2);
    animation: qoSlide var(--dur-slow) var(--ease-spring);
  }
  .qo-head {
    display: flex; align-items: center; gap: 14px;
    padding: 14px 22px;
    border-bottom: 1px solid var(--border);
    flex-shrink: 0;
  }
  .qo-input {
    flex: 1;
    font-size: 15px; color: var(--text-0);
    background: transparent; border: none;
    letter-spacing: -0.005em;
  }
  .qo-input:focus { outline: none; }
  .qo-input::placeholder { color: var(--text-mute); }
  .qo-input:disabled { opacity: 0.5; }
  .qo-scope {
    display: inline-flex; align-items: center; gap: 5px;
    flex-shrink: 0;
    padding: 4px 8px;
    background: var(--bg-3);
    border: 1px solid var(--border);
    border-radius: 6px;
    font-size: 11px;
    color: var(--text-1);
    cursor: default;
  }
  .qo-scope:disabled { opacity: 0.7; }
  .qo-scope-cyclable { cursor: pointer; }
  .qo-scope-cyclable:hover { background: var(--bg-2); border-color: var(--border-hi); color: var(--text-0); }
  .qo-scope-name { font-weight: 600; letter-spacing: -0.005em; }
  .qo-scope-sep { color: var(--text-mute); opacity: 0.6; }
  .qo-scope-folder {
    color: var(--text-mute);
    font-size: 10.5px;
    max-width: 160px;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .qo-scope-cycle { color: var(--accent); font-size: 11px; margin-left: 2px; }
  .qo-meta { font-size: 11px; color: var(--text-mute); flex-shrink: 0; }
  .qo-err { color: var(--accent-warn, #e89b7d); }

  .qo-empty {
    padding: 28px 22px;
    color: var(--text-mute);
    font-size: 12.5px;
    text-align: center;
  }

  .qo-scroll {
    flex: 1; min-height: 0;
    overflow-y: auto;
    padding: 4px 0;
  }
  .qo-row {
    width: 100%;
    display: grid;
    grid-template-columns: 1fr auto;
    gap: 12px;
    align-items: baseline;
    padding: 6px 22px;
    background: transparent;
    border: 0;
    color: var(--text-0);
    text-align: left;
    cursor: pointer;
    font-size: 12.5px;
    line-height: 1.4;
  }
  .qo-row:hover, .qo-row.highlight {
    background: var(--accent-soft);
    color: var(--text-0);
  }
  .qo-row-name {
    font-size: 13px;
    color: var(--text-0);
    white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
  }
  .qo-row-name mark {
    background: transparent;
    color: var(--accent-bright);
    font-weight: 600;
  }
  .qo-row-dir {
    color: var(--text-mute);
    font-size: 11px;
    max-width: 60%;
    white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
    direction: rtl; text-align: left;
  }

  .qo-foot {
    display: flex; align-items: center; gap: 14px;
    padding: 8px 18px;
    border-top: 1px solid var(--border);
    color: var(--text-mute);
    font-size: 11px;
    flex-shrink: 0;
  }
  .qo-foot .grp { display: inline-flex; align-items: center; gap: 6px; }
  .qo-foot .kbd {
    background: var(--bg-2);
    border: 1px solid var(--border);
    border-radius: 3px;
    padding: 0 5px;
    color: var(--text-1);
    font-family: 'JetBrains Mono', monospace;
    font-size: 10px;
  }

  @keyframes qoFade { from { opacity: 0; } to { opacity: 1; } }
  @keyframes qoSlide {
    from { opacity: 0; transform: translateY(-12px) scale(0.985); }
    to   { opacity: 1; transform: translateY(0)    scale(1); }
  }
</style>
