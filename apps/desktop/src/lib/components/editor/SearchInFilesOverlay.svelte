<script lang="ts">
  /* Project-wide find — the long-missing ⌘⇧F. Same modal shape as
   * CommandPalette so the muscle memory carries over: debounced input,
   * keyboard-navigable result list, Esc to close. Backend is
   * `fs_search_text` (case-insensitive substring; binary / oversized
   * files skipped).
   *
   * Click / Enter on a result primes the cursor cache for the target
   * file (so the editor's existing cursor-restore path lands the
   * caret on the match) and requests the file open via the standard
   * `requestEditorOpenFile` signal. No new editor APIs needed.
   *
   * Scope: searches the editor singleton's `repoPath`. If the editor
   * has no folder open the overlay surfaces a one-line hint and a
   * "Pick a folder" affordance routed back to the editor view. */

  import { invoke } from '@tauri-apps/api/core';
  import {
    sessionsState,
    requestEditorOpenFile
  } from '$lib/state/sessions.svelte';
  import { recordCursor } from '$lib/state/editorCursors.svelte';
  import { layoutState, setActiveInstance } from '$lib/state/layout.svelte';
  import { focusTrap } from '$lib/actions/focusTrap';
  import type { View } from '$lib/state/view.svelte';

  interface TextMatch {
    path: string;
    line: number;
    col: number;
    byte_offset: number;
    line_byte_offset: number;
    match_len: number;
    preview: string;
  }
  interface SearchResult {
    matches: TextMatch[];
    truncated: boolean;
    files_scanned: number;
  }

  interface Props {
    open: boolean;
    setView: (v: View) => void;
  }

  let { open = $bindable(), setView }: Props = $props();

  let query = $state('');
  let selectedIdx = $state(0);
  let results = $state<TextMatch[]>([]);
  let truncated = $state(false);
  let filesScanned = $state(0);
  let busy = $state(false);
  let errMsg = $state<string | null>(null);
  let debounceTimer: ReturnType<typeof setTimeout> | null = null;

  /* Which editor solo to search. Defaults to the rail's currently
   * active editor instance (`layoutState.activeInstance.editor`) so
   * ⌘⇧F follows the user's current focus when there are multiple
   * editors open with different repos. The chip in the header lets
   * them flip targets without leaving the overlay. */
  const editorInstances = $derived(layoutState.instances.editor ?? []);
  let targetEditorId = $state<string | null>(null);
  /* On open, sync `targetEditorId` to the layout's active editor.
   * Keeping it as local $state (vs. a $derived) means switching
   * editors mid-search inside the overlay doesn't snap back when
   * the layout state ticks elsewhere. */
  $effect(() => {
    if (!open) return;
    if (targetEditorId === null) {
      targetEditorId = layoutState.activeInstance.editor;
    }
  });
  const currentEditorId = $derived(
    targetEditorId ?? layoutState.activeInstance.editor
  );
  const currentEditor = $derived(
    editorInstances.find((i) => i.id === currentEditorId) ?? null
  );
  const repoPath = $derived(
    sessionsState.editorInstanceState[currentEditorId]?.repoPath ?? ''
  );

  /* Cycle through editor instances that have a non-empty repoPath —
   * lets the user pick which editor to search when several are open
   * on different repos. Skips empty-folder editors so the cycle
   * doesn't park on a useless instance. */
  function cycleEditor() {
    const candidates = editorInstances.filter(
      (i) => (sessionsState.editorInstanceState[i.id]?.repoPath ?? '') !== ''
    );
    if (candidates.length <= 1) return;
    const idx = candidates.findIndex((i) => i.id === currentEditorId);
    const next = candidates[(idx + 1) % candidates.length];
    targetEditorId = next.id;
    /* Also bump the rail's active pointer so a subsequent ⌘⇧F (or any
     * other editor-targeted action) lines up with the user's choice. */
    setActiveInstance('editor', next.id);
    /* Reset results — they're scoped to the previous repo. */
    results = [];
    selectedIdx = 0;
    if (query.trim()) scheduleSearch();
  }
  const hasMultipleEditors = $derived(
    editorInstances.filter(
      (i) => (sessionsState.editorInstanceState[i.id]?.repoPath ?? '') !== ''
    ).length > 1
  );

  /* Group matches by file so a long result list stays scannable.
   * Insertion order from the backend is BFS by dir depth, alphabetical
   * within a dir — preserving that here keeps the same layout users
   * see in the file tree. */
  type FileGroup = { path: string; matches: TextMatch[] };
  const groups = $derived.by((): FileGroup[] => {
    const map = new Map<string, TextMatch[]>();
    for (const m of results) {
      const arr = map.get(m.path);
      if (arr) arr.push(m);
      else map.set(m.path, [m]);
    }
    return [...map.entries()].map(([path, matches]) => ({ path, matches }));
  });

  /* Flat list for keyboard navigation — `selectedIdx` indexes into
   * this. Re-derived alongside groups so the indices stay aligned. */
  const flatMatches = $derived(results);

  function close() {
    open = false;
    /* Persist the last query string per-session-feel — feels natural to
     * re-open and pick up where you left off — but clear the result
     * cache so the modal opens snappy. */
    results = [];
    selectedIdx = 0;
    errMsg = null;
    /* Reset the per-overlay editor target so the next ⌘⇧F syncs from
     * the rail's active editor again. Otherwise switching editors via
     * the rail wouldn't propagate. The cycle path already calls
     * `setActiveInstance`, so the user's sticky choice is preserved. */
    targetEditorId = null;
  }

  /* Strip the repo prefix from the absolute path for display. Falls
   * back to the absolute path when the path somehow doesn't sit under
   * the current root (orphan tab from a previous search session). */
  function relPath(abs: string): string {
    if (!repoPath) return abs;
    const root = repoPath.endsWith('/') ? repoPath : repoPath + '/';
    return abs.startsWith(root) ? abs.slice(root.length) : abs;
  }

  /* Hand-rolled debounce — Tauri command latency on a multi-thousand
   * file repo can be 80-200 ms; debouncing at 180 ms means the search
   * doesn't fight the user's typing speed. */
  function scheduleSearch() {
    if (debounceTimer) clearTimeout(debounceTimer);
    debounceTimer = setTimeout(runSearch, 180);
  }

  async function runSearch() {
    debounceTimer = null;
    const q = query.trim();
    if (!q || !repoPath) {
      results = [];
      truncated = false;
      filesScanned = 0;
      errMsg = null;
      return;
    }
    busy = true;
    errMsg = null;
    try {
      const r = await invoke<SearchResult>('fs_search_text', {
        root: repoPath,
        query: q,
        maxResults: 500
      });
      results = r.matches;
      truncated = r.truncated;
      filesScanned = r.files_scanned;
      selectedIdx = 0;
    } catch (e) {
      errMsg = e instanceof Error ? e.message : String(e);
      results = [];
    } finally {
      busy = false;
    }
  }

  /* Pick a result: prime the cursor cache so the editor's existing
   * load-time restore lands the caret on the match, then request the
   * file open through the singleton signal. Approximate `scrollTop`
   * by line number × an 18 px guess — close enough that the match
   * lands in the viewport on real files (one or two lines off in
   * pathological cases). */
  function pickMatch(m: TextMatch) {
    /* Route the open through whichever editor the user picked in the
     * scope chip — same instance the search was scoped to, so the
     * match opens in the right window when several editors are
     * juggled. */
    const editorId = currentEditorId;
    recordCursor(m.path, {
      from: m.byte_offset,
      to: m.byte_offset + m.match_len,
      scrollTop: Math.max(0, (m.line - 6) * 18)
    });
    requestEditorOpenFile(editorId, m.path);
    setView('editorApp');
    close();
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      close();
      return;
    }
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      selectedIdx = Math.min(selectedIdx + 1, Math.max(flatMatches.length - 1, 0));
      return;
    }
    if (e.key === 'ArrowUp') {
      e.preventDefault();
      selectedIdx = Math.max(selectedIdx - 1, 0);
      return;
    }
    if (e.key === 'Enter') {
      e.preventDefault();
      const m = flatMatches[selectedIdx];
      if (m) pickMatch(m);
    }
  }

  /* Highlight the matched substring inside the preview line — same
   * case-insensitive logic as the backend, plus a guard for the case
   * where `…`-trimmed previews shifted the position so the offset
   * doesn't line up cleanly. */
  function highlightPreview(preview: string, q: string): Array<{ t: string; hit: boolean }> {
    if (!q) return [{ t: preview, hit: false }];
    const out: Array<{ t: string; hit: boolean }> = [];
    const lowerP = preview.toLowerCase();
    const lowerQ = q.toLowerCase();
    let i = 0;
    while (i < preview.length) {
      const hit = lowerP.indexOf(lowerQ, i);
      if (hit < 0) {
        out.push({ t: preview.slice(i), hit: false });
        break;
      }
      if (hit > i) out.push({ t: preview.slice(i, hit), hit: false });
      out.push({ t: preview.slice(hit, hit + q.length), hit: true });
      i = hit + q.length;
    }
    return out;
  }

  /* Ensure the selected row scrolls into view when navigating with
   * the keyboard past the viewport edge. */
  let listEl: HTMLDivElement | null = $state(null);
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
</script>

{#if open}
  <div
    class="sfo-backdrop"
    onclick={(e) => {
      if (e.target === e.currentTarget) close();
    }}
    onkeydown={onKey}
    role="dialog"
    aria-modal="true"
    aria-label="Search in files"
    tabindex="-1"
    use:focusTrap
  >
    <div class="sfo">
      <div class="sfo-head">
        <!-- svelte-ignore a11y_autofocus -->
        <input
          class="sfo-input"
          bind:value={query}
          oninput={scheduleSearch}
          placeholder={repoPath
            ? 'Type to grep…'
            : 'Open a folder in the editor first…'}
          autofocus
          disabled={!repoPath}
        />
        {#if currentEditor}
          <!-- Editor scope chip. Click to cycle through editor
               instances when several are open on different repos. The
               disabled state on the single-editor case prevents the
               row from looking interactive when it isn't. -->
          <button
            class="sfo-scope"
            class:sfo-scope-cyclable={hasMultipleEditors}
            onclick={cycleEditor}
            disabled={!hasMultipleEditors}
            title={hasMultipleEditors
              ? 'Click to cycle editor (or open ⌘⇧F again after switching the rail)'
              : 'Only one editor open'}
            type="button"
          >
            <span class="sfo-scope-name">{currentEditor.name}</span>
            <span class="sfo-scope-sep">·</span>
            <span class="sfo-scope-folder mono">
              {repoPath ? (repoPath.split('/').filter(Boolean).pop() ?? '') : '(no folder)'}
            </span>
            {#if hasMultipleEditors}<span class="sfo-scope-cycle">⇄</span>{/if}
          </button>
        {/if}
        <span class="sfo-meta">
          {#if busy}
            searching…
          {:else if errMsg}
            <span class="sfo-err">{errMsg}</span>
          {:else if query && results.length === 0 && repoPath}
            no matches
          {:else if results.length > 0}
            {results.length} match{results.length === 1 ? '' : 'es'} · {filesScanned} files{truncated ? ' (+ more)' : ''}
          {/if}
        </span>
      </div>

      {#if !repoPath}
        <div class="sfo-empty">
          The editor has no folder open. Choose one from the Editor view, then
          try ⌘⇧F again.
        </div>
      {:else if !query.trim()}
        <div class="sfo-empty">
          Type to grep <span class="mono">{repoPath.split('/').filter(Boolean).pop()}</span> — case-insensitive, skips
          binary &amp; vendor folders.
        </div>
      {:else if results.length > 0}
        <div class="sfo-scroll" bind:this={listEl}>
          {#each groups as g (g.path)}
            <div class="sfo-group">
              <div class="sfo-group-head">
                <span class="sfo-group-path mono">{relPath(g.path)}</span>
                <span class="sfo-group-count">{g.matches.length}</span>
              </div>
              {#each g.matches as m (m.byte_offset)}
                {@const idx = flatMatches.indexOf(m)}
                <button
                  class="sfo-row"
                  class:highlight={idx === selectedIdx}
                  data-idx={idx}
                  onmouseenter={() => (selectedIdx = idx)}
                  onclick={() => pickMatch(m)}
                  type="button"
                >
                  <span class="sfo-line mono">{m.line}:{m.col}</span>
                  <span class="sfo-preview mono">
                    {#each highlightPreview(m.preview, query.trim()) as seg}
                      {#if seg.hit}<mark>{seg.t}</mark>{:else}{seg.t}{/if}
                    {/each}
                  </span>
                </button>
              {/each}
            </div>
          {/each}
        </div>
      {/if}

      <div class="sfo-foot">
        <span class="grp">
          <span class="kbd">↑</span><span class="kbd">↓</span>
          <span>navigate</span>
        </span>
        <span class="grp">
          <span class="kbd">⏎</span>
          <span>open at line</span>
        </span>
        <span class="grp" style="margin-left: auto;">
          <span class="kbd">esc</span>
          <span>close</span>
        </span>
      </div>
    </div>
  </div>
{/if}

<style>
  /* Same modal shape as CommandPalette — different palette so the
     user knows at a glance "this is search, not navigation". */
  .sfo-backdrop {
    position: fixed; inset: 0;
    background: var(--backdrop);
    backdrop-filter: blur(22px) saturate(1.1);
    -webkit-backdrop-filter: blur(22px) saturate(1.1);
    display: flex; align-items: flex-start; justify-content: center;
    padding-top: 10vh; z-index: 200;
    animation: sfoFade var(--dur-base) var(--ease-out);
  }
  .sfo {
    width: 760px; max-width: 92vw;
    max-height: 76vh;
    display: flex; flex-direction: column;
    background: linear-gradient(180deg, var(--bg-2), var(--bg-1));
    border: 1px solid var(--border-hi);
    border-radius: var(--r-modal, 16px);
    overflow: hidden;
    box-shadow:
      var(--shadow-3),
      0 0 0 1px var(--border-accent-2),
      inset 0 1px 0 rgba(255, 240, 220, 0.04);
    animation: sfoSlide var(--dur-slow) var(--ease-spring);
  }
  .sfo-head {
    display: flex; align-items: center; gap: 14px;
    padding: 14px 22px;
    border-bottom: 1px solid var(--border);
    flex-shrink: 0;
  }
  .sfo-input {
    flex: 1;
    font-size: 15px; color: var(--text-0);
    background: transparent; border: none;
    letter-spacing: -0.005em;
  }
  .sfo-input:focus { outline: none; }
  .sfo-input::placeholder { color: var(--text-mute); }
  .sfo-input:disabled { opacity: 0.5; }
  /* Editor scope chip between the input and the result-count meta.
     Two-line layout would push the input down on narrow widths, so
     we keep it inline-flex and rely on the input flex:1 to absorb
     remaining space. */
  .sfo-scope {
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
  .sfo-scope:disabled { opacity: 0.7; }
  .sfo-scope-cyclable { cursor: pointer; }
  .sfo-scope-cyclable:hover {
    background: var(--bg-2);
    border-color: var(--border-hi);
    color: var(--text-0);
  }
  .sfo-scope-name { font-weight: 600; letter-spacing: -0.005em; }
  .sfo-scope-sep { color: var(--text-mute); opacity: 0.6; }
  .sfo-scope-folder {
    color: var(--text-mute);
    font-size: 10.5px;
    max-width: 160px;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .sfo-scope-cycle {
    color: var(--accent);
    font-size: 11px;
    margin-left: 2px;
  }
  .sfo-meta {
    font-family: 'JetBrains Mono', monospace;
    font-size: 11px;
    color: var(--text-mute);
    flex-shrink: 0;
  }
  .sfo-err { color: var(--accent-warn, #e89b7d); }
  .sfo-empty {
    padding: 32px 22px;
    font-size: 13px; color: var(--text-2);
    text-align: center;
  }
  .sfo-scroll {
    overflow-y: auto; flex: 1;
    padding: 6px 10px 10px;
  }
  .sfo-group { margin-top: 8px; }
  .sfo-group:first-child { margin-top: 2px; }
  .sfo-group-head {
    display: flex; align-items: baseline; justify-content: space-between;
    padding: 6px 12px 4px;
    border-bottom: 1px solid var(--border);
  }
  .sfo-group-path {
    color: var(--text-1);
    font-size: 11.5px;
    font-weight: 600;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .sfo-group-count {
    font-family: 'JetBrains Mono', monospace;
    font-size: 10px;
    color: var(--text-mute);
    background: var(--bg-3);
    padding: 1px 6px;
    border-radius: 4px;
  }
  .sfo-row {
    display: flex; align-items: baseline; gap: 12px;
    width: 100%;
    padding: 6px 14px;
    background: none; border: none;
    text-align: left;
    cursor: pointer;
    color: var(--text-1);
    border-radius: 6px;
    transition: background 80ms ease;
  }
  .sfo-row:hover { background: var(--bg-2); }
  .sfo-row.highlight {
    background: linear-gradient(90deg,
      color-mix(in srgb, var(--accent) 10%, transparent),
      color-mix(in srgb, var(--accent) 2%, transparent) 60%);
    box-shadow: inset 0 0 0 1px var(--border-accent-2);
    color: var(--text-0);
  }
  .sfo-line {
    color: var(--text-mute);
    font-size: 10.5px;
    flex-shrink: 0;
    min-width: 56px;
    font-feature-settings: 'tnum';
  }
  .sfo-preview {
    flex: 1; min-width: 0;
    font-size: 12px;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .sfo-preview mark {
    background: color-mix(in srgb, var(--accent) 28%, transparent);
    color: var(--text-0);
    padding: 0 2px;
    border-radius: 3px;
    font-weight: 600;
  }
  .mono {
    font-family: 'JetBrains Mono', monospace;
  }

  .sfo-foot {
    padding: 10px 18px;
    border-top: 1px solid var(--border);
    display: flex; align-items: center; gap: 14px;
    font-size: 11px;
    color: var(--text-mute);
    background: var(--bg-2);
    flex-shrink: 0;
  }
  .sfo-foot .grp { display: flex; align-items: center; gap: 5px; }
  .sfo-foot .kbd {
    display: inline-grid; place-items: center;
    height: 16px; min-width: 16px;
    padding: 0 4px;
    font-family: 'JetBrains Mono', monospace;
    font-size: 9.5px;
    background: var(--bg-3);
    border: 1px solid var(--border);
    border-radius: 4px;
    color: var(--text-1);
  }

  @keyframes sfoFade { from { opacity: 0; } to { opacity: 1; } }
  @keyframes sfoSlide {
    from { transform: translateY(-10px); opacity: 0; }
    to { transform: translateY(0); opacity: 1; }
  }
</style>
