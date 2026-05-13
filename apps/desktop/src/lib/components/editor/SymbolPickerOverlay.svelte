<script lang="ts">
  /* SymbolPickerOverlay — ⇧⌘O "Go to symbol in file". Same modal
     vocabulary as QuickOpenOverlay / SearchInFilesOverlay so the
     three pickers feel like one family.

     We extract symbols via `services/symbolOutline.ts` (regex-based,
     7 languages out of the box) so the picker stays self-contained —
     no tree-sitter / WASM / language server load on the boot path.
     Picking a row dispatches a `woom:editor:goto` window event with
     the target editor instance + line; Editor.svelte listens for it
     and runs `view.dispatch({selection, scrollIntoView})`. We use a
     window event instead of plumbing a ref through EditorApp →
     EditorView → Editor because the overlay can fire across an
     arbitrary editor instance (the user may have ⇧⌘O'd while
     focused on a different solo) and the event channel keeps the
     coupling at zero new shared types.

     If the active editor doesn't have a file open, the overlay shows
     a helpful empty state instead of silently going nowhere. */
  import { invoke } from '@tauri-apps/api/core';
  import { sessionsState, requestEditorOpenFile } from '$lib/state/sessions.svelte';
  import { layoutState } from '$lib/state/layout.svelte';
  import { fuzzyScoreAny } from '$lib/services/fuzzyMatch';
  import { extractSymbols, type SymbolEntry, type SymbolKind } from '$lib/services/symbolOutline';
  import { focusTrap } from '$lib/actions/focusTrap';
  import type { View } from '$lib/state/view.svelte';

  interface Props {
    open: boolean;
    setView: (v: View) => void;
  }
  let { open = $bindable(), setView }: Props = $props();

  type Row = { sym: SymbolEntry; score: number };

  let query = $state('');
  let selectedIdx = $state(0);
  let symbols = $state<SymbolEntry[]>([]);
  let busy = $state(false);
  let errMsg = $state<string | null>(null);
  let listEl: HTMLDivElement | null = $state(null);

  /* The editor we're outlining. We scope to whichever editor instance
     the rail currently has active, same as ⌘P / ⌘⇧F do — keeps the
     mental model consistent ("these pickers all act on the editor
     I'm looking at"). */
  const editorId = $derived(layoutState.activeInstance.editor);
  const repoPath = $derived(
    sessionsState.editorInstanceState[editorId]?.repoPath ?? ''
  );
  /* The currently-open buffer in that editor — read from the per-
     instance localStorage key Editor.svelte mirrors `activePath`
     into. Same hook the @-mention picker uses. */
  let activePath = $state('');
  function refreshActivePath() {
    try {
      activePath = localStorage.getItem(`woom:editor:active:${editorId}`) || '';
    } catch {
      activePath = '';
    }
  }

  $effect(() => {
    if (!open) return;
    refreshActivePath();
    void loadSymbols();
  });

  /* Re-extract whenever the editor or path change while the modal is
     open (rare — the user can swap editor instances mid-session). */
  $effect(() => {
    if (!open) return;
    void editorId;
    refreshActivePath();
    void loadSymbols();
  });

  async function loadSymbols() {
    symbols = [];
    selectedIdx = 0;
    if (!activePath) return;
    busy = true;
    errMsg = null;
    try {
      const text = await invoke<string>('fs_read_file', { path: activePath });
      symbols = extractSymbols(activePath, text);
    } catch (e) {
      errMsg = e instanceof Error ? e.message : String(e);
      symbols = [];
    } finally {
      busy = false;
    }
  }

  const rows = $derived.by<Row[]>(() => {
    const q = query.trim();
    if (!q) return symbols.map((s) => ({ sym: s, score: 0 }));
    const out: Row[] = [];
    for (const s of symbols) {
      const score = fuzzyScoreAny(q, [s.name, s.preview, s.kind]);
      if (score === null) continue;
      out.push({ sym: s, score });
    }
    /* Within equal score, file order wins so users get predictable
       results — fuzzy matches are listed in their declaration
       sequence, not alphabetised. */
    out.sort((a, b) => b.score - a.score || a.sym.line - b.sym.line);
    return out;
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
  }

  function pickRow(row: Row) {
    if (!activePath) return;
    /* Make sure the file is open + active in the target editor (it
       almost always is — the user just opened the picker — but a
       nice-to-have for the rare race where they ⇧⌘O'd via global
       shortcut from the agent solo). */
    requestEditorOpenFile(editorId, activePath);
    setView('editorApp');
    /* Custom event — Editor.svelte filters by editorId so the right
     * buffer takes the jump. */
    window.dispatchEvent(new CustomEvent('woom:editor:goto', {
      detail: { editorId, filePath: activePath, line: row.sym.line }
    }));
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

  function relPath(abs: string): string {
    if (!repoPath) return abs;
    const root = repoPath.endsWith('/') ? repoPath : repoPath + '/';
    return abs.startsWith(root) ? abs.slice(root.length) : abs;
  }

  /* Compact icon character per kind — keeps the row visually scannable
     without shipping an icon set. ASCII-friendly so any monospace
     font renders correctly. */
  function kindGlyph(k: SymbolKind): string {
    switch (k) {
      case 'function': return 'ƒ';
      case 'method':   return '⌘';
      case 'class':    return 'C';
      case 'interface':return 'I';
      case 'type':     return 'T';
      case 'enum':     return 'E';
      case 'struct':   return 'S';
      case 'trait':    return 'R';
      case 'mod':      return 'M';
      case 'macro':    return '!';
      case 'variable': return 'v';
      case 'section':  return '§';
      default:         return '·';
    }
  }
  function kindClass(k: SymbolKind): string { return `sp-kind sp-kind--${k}`; }
</script>

{#if open}
  <div
    class="sp-backdrop"
    onclick={(e) => { if (e.target === e.currentTarget) close(); }}
    onkeydown={onKey}
    role="dialog"
    aria-modal="true"
    aria-label="Go to symbol in file"
    tabindex="-1"
    use:focusTrap
  >
    <div class="sp">
      <div class="sp-head">
        <!-- svelte-ignore a11y_autofocus -->
        <input
          class="sp-input"
          bind:value={query}
          placeholder={activePath ? 'Type a symbol name…' : 'Open a file in the editor first…'}
          autofocus
          disabled={!activePath}
        />
        {#if activePath}
          <span class="sp-scope mono" title={activePath}>{relPath(activePath)}</span>
        {/if}
        <span class="sp-meta mono">
          {#if busy}
            scanning…
          {:else if errMsg}
            <span class="sp-err">{errMsg}</span>
          {:else if symbols.length === 0 && activePath}
            no symbols
          {:else if rows.length === 0 && query.trim()}
            no matches
          {:else}
            {rows.length} of {symbols.length}
          {/if}
        </span>
      </div>

      {#if !activePath}
        <div class="sp-empty">
          The active editor has no file open. Use ⌘P to open one and
          try ⇧⌘O again.
        </div>
      {:else if symbols.length === 0 && !busy}
        <div class="sp-empty">
          No symbols recognised in <span class="mono">{relPath(activePath)}</span>.
          Outline supports TS / JS / Svelte / Rust / Python / Go /
          Markdown today — other languages get the regex pass too but
          may surface fewer hits.
        </div>
      {:else}
        <div class="sp-scroll" bind:this={listEl}>
          {#each rows as r, i (r.sym.line)}
            <button
              class="sp-row"
              class:highlight={i === selectedIdx}
              data-idx={i}
              onmouseenter={() => (selectedIdx = i)}
              onclick={() => pickRow(r)}
              type="button"
              style:padding-left="{16 + r.sym.depth * 14}px"
            >
              <span class={kindClass(r.sym.kind)} aria-hidden="true">{kindGlyph(r.sym.kind)}</span>
              <span class="sp-name mono">{r.sym.name}</span>
              <span class="sp-preview mono">{r.sym.preview}</span>
              <span class="sp-line mono">{r.sym.line}</span>
            </button>
          {/each}
        </div>
      {/if}

      <div class="sp-foot">
        <span class="grp"><span class="kbd">↑</span><span class="kbd">↓</span><span>navigate</span></span>
        <span class="grp"><span class="kbd">⏎</span><span>jump</span></span>
        <span class="grp" style="margin-left: auto;"><span class="kbd">esc</span><span>close</span></span>
      </div>
    </div>
  </div>
{/if}

<style>
  .sp-backdrop {
    position: fixed; inset: 0;
    background: var(--backdrop);
    backdrop-filter: blur(22px) saturate(1.1);
    -webkit-backdrop-filter: blur(22px) saturate(1.1);
    display: flex; align-items: flex-start; justify-content: center;
    padding-top: 10vh; z-index: 200;
    animation: spFade var(--dur-base) var(--ease-out);
  }
  .sp {
    width: 720px; max-width: 92vw;
    max-height: 70vh;
    display: flex; flex-direction: column;
    background: linear-gradient(180deg, var(--bg-2), var(--bg-1));
    border: 1px solid var(--border-hi);
    border-radius: var(--r-modal, 16px);
    overflow: hidden;
    box-shadow: var(--shadow-3), 0 0 0 1px var(--border-accent-2);
    animation: spSlide var(--dur-slow) var(--ease-spring);
  }
  .sp-head {
    display: flex; align-items: center; gap: 14px;
    padding: 14px 22px;
    border-bottom: 1px solid var(--border);
    flex-shrink: 0;
  }
  .sp-input {
    flex: 1;
    font-size: 15px; color: var(--text-0);
    background: transparent; border: none;
    letter-spacing: -0.005em;
  }
  .sp-input:focus { outline: none; }
  .sp-input::placeholder { color: var(--text-mute); }
  .sp-input:disabled { opacity: 0.5; }
  .sp-scope {
    font-size: 11px;
    color: var(--text-mute);
    max-width: 240px;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .sp-meta { font-size: 11px; color: var(--text-mute); flex-shrink: 0; }
  .sp-err { color: var(--accent-warn, #e89b7d); }

  .sp-empty {
    padding: 28px 22px;
    color: var(--text-mute);
    font-size: 12.5px;
    text-align: center;
  }

  .sp-scroll {
    flex: 1; min-height: 0;
    overflow-y: auto;
    padding: 4px 0;
  }

  .sp-row {
    width: 100%;
    display: grid;
    grid-template-columns: 24px auto 1fr auto;
    gap: 10px;
    align-items: baseline;
    padding: 5px 22px 5px 16px;
    background: transparent;
    border: 0;
    color: var(--text-0);
    text-align: left;
    cursor: pointer;
    font-size: 12.5px;
    line-height: 1.4;
  }
  .sp-row:hover, .sp-row.highlight {
    background: var(--accent-soft);
  }
  .sp-name { font-size: 13px; color: var(--text-0); white-space: nowrap; }
  .sp-preview {
    color: var(--text-mute);
    font-size: 11px;
    white-space: nowrap;
    overflow: hidden; text-overflow: ellipsis;
  }
  .sp-line {
    color: var(--text-mute);
    font-size: 10.5px;
    text-align: right;
  }

  .sp-kind {
    display: inline-grid; place-items: center;
    width: 18px; height: 18px;
    border-radius: 4px;
    font-family: 'JetBrains Mono', monospace;
    font-size: 11px; font-weight: 700;
    background: var(--bg-3);
    color: var(--text-1);
  }
  /* Brand-tinted icons per kind so you can spot a class vs a function
     in your peripheral vision. The colours mirror the theme palette
     elsewhere — accent for callable things, secondary for shapes. */
  .sp-kind--function { background: color-mix(in srgb, var(--accent) 30%, var(--bg-3)); color: var(--accent-bright); }
  .sp-kind--method   { background: color-mix(in srgb, var(--accent) 22%, var(--bg-3)); color: var(--accent-bright); }
  .sp-kind--class    { background: color-mix(in srgb, var(--src-claude) 30%, var(--bg-3)); color: var(--src-claude); }
  .sp-kind--interface{ background: color-mix(in srgb, var(--src-cursor) 30%, var(--bg-3)); color: var(--text-0); }
  .sp-kind--struct   { background: color-mix(in srgb, var(--src-claude) 24%, var(--bg-3)); color: var(--src-claude); }
  .sp-kind--trait    { background: color-mix(in srgb, var(--accent) 22%, var(--bg-3)); color: var(--text-0); }
  .sp-kind--enum     { background: color-mix(in srgb, var(--diff-add) 24%, var(--bg-3)); color: var(--diff-add-stroke); }
  .sp-kind--type     { background: color-mix(in srgb, var(--src-jira) 24%, var(--bg-3)); color: var(--src-jira); }
  .sp-kind--mod      { background: var(--bg-3); color: var(--text-1); }
  .sp-kind--macro    { background: color-mix(in srgb, var(--warning) 30%, var(--bg-3)); color: var(--warning); }
  .sp-kind--variable { background: var(--bg-3); color: var(--text-2); }
  .sp-kind--section  { background: color-mix(in srgb, var(--src-editor) 24%, var(--bg-3)); color: var(--src-editor); }

  .sp-foot {
    display: flex; align-items: center; gap: 14px;
    padding: 8px 18px;
    border-top: 1px solid var(--border);
    color: var(--text-mute);
    font-size: 11px;
    flex-shrink: 0;
  }
  .sp-foot .grp { display: inline-flex; align-items: center; gap: 6px; }
  .sp-foot .kbd {
    background: var(--bg-2);
    border: 1px solid var(--border);
    border-radius: 3px;
    padding: 0 5px;
    color: var(--text-1);
    font-family: 'JetBrains Mono', monospace;
    font-size: 10px;
  }

  @keyframes spFade { from { opacity: 0; } to { opacity: 1; } }
  @keyframes spSlide {
    from { opacity: 0; transform: translateY(-12px) scale(0.985); }
    to   { opacity: 1; transform: translateY(0)    scale(1); }
  }
</style>
