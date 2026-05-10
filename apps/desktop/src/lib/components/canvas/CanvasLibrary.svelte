<script lang="ts">
  // Canvas library overlay — the full grid of every canvas in the
  // workspace. Triggered by ⌘P while a Canvas column is focused, or by
  // the column-header dropdown chevron. Renders ON TOP of the column
  // (not the whole app) so the user keeps their solo context.
  //
  // Each tile shows: name, last-edited time, shape count, and a hover
  // row of actions (rename, duplicate, archive / unarchive, export,
  // delete). Active canvases are listed first; archived go below in a
  // dimmed section. Search filters across both.
  //
  // Thumbnail rendering is deferred — a tile shows a stylized empty
  // preview based on background mode. Real bitmap thumbnails will land
  // when canvases move from localStorage to disk JSON files (where
  // sidecar PNG generation pays off).

  import {
    canvasState,
    createCanvas,
    renameCanvas,
    archiveCanvas,
    unarchiveCanvas,
    deleteCanvas,
    duplicateCanvas,
    exportCanvasJson,
    importCanvasJson,
    openCanvasInInstance,
    ensureCanvasLoaded,
    type CanvasIndexEntry
  } from '$lib/state/canvas.svelte';
  import type { Shape } from '$lib/state/canvas.svelte';
  import { notify } from '$lib/state/toaster.svelte';

  interface Props {
    instanceId: string;
    activeCanvasId: string | null;
    onClose: () => void;
  }

  let { instanceId, activeCanvasId, onClose }: Props = $props();

  let query = $state('');
  /** Inline-rename state: when set, the tile renders an input instead
   *  of the title. Clicking outside / Esc cancels; Enter / blur commits. */
  let editingId = $state<string | null>(null);
  let editingDraft = $state('');

  const filtered = $derived.by<CanvasIndexEntry[]>(() => {
    const q = query.trim().toLowerCase();
    const list = canvasState.index.slice();
    /* Newest activity first within a section. */
    list.sort((a, b) => b.updatedAt - a.updatedAt);
    if (!q) return list;
    return list.filter((e) => e.name.toLowerCase().includes(q));
  });
  const activeOnes = $derived(filtered.filter((e) => !e.archivedAt));
  const archivedOnes = $derived(filtered.filter((e) => !!e.archivedAt));

  function fmtRelative(ts: number): string {
    const dt = Date.now() - ts;
    if (dt < 60_000) return 'just now';
    if (dt < 3_600_000) return `${Math.floor(dt / 60_000)}m ago`;
    if (dt < 86_400_000) return `${Math.floor(dt / 3_600_000)}h ago`;
    if (dt < 7 * 86_400_000) return `${Math.floor(dt / 86_400_000)}d ago`;
    return new Date(ts).toLocaleDateString();
  }

  function handleOpen(id: string) {
    openCanvasInInstance(instanceId, id);
    onClose();
  }

  function handleNew() {
    const id = createCanvas('Untitled');
    openCanvasInInstance(instanceId, id);
    onClose();
  }

  function handleDuplicate(e: MouseEvent, id: string) {
    e.stopPropagation();
    const newId = duplicateCanvas(id);
    if (newId) {
      notify({ kind: 'success', title: 'Canvas duplicated', ttlMs: 1500 });
    }
  }

  function handleArchive(e: MouseEvent, id: string) {
    e.stopPropagation();
    archiveCanvas(id);
    notify({ kind: 'info', title: 'Archived', body: 'Open the library to restore.', ttlMs: 1800 });
  }

  function handleUnarchive(e: MouseEvent, id: string) {
    e.stopPropagation();
    unarchiveCanvas(id);
  }

  function handleDelete(e: MouseEvent, id: string, name: string) {
    e.stopPropagation();
    /* Confirm-before-destructive — uses the browser confirm to avoid
       wiring our app-level modal here. Two clicks beats an accidental
       deletion of a big canvas. */
    const ok = confirm(`Delete canvas "${name}"? This cannot be undone.`);
    if (!ok) return;
    deleteCanvas(id);
    notify({ kind: 'warning', title: 'Canvas deleted', ttlMs: 1500 });
  }

  function handleExport(e: MouseEvent, id: string, name: string) {
    e.stopPropagation();
    const json = exportCanvasJson(id);
    if (!json) return;
    /* Browser-side download via a transient anchor; works in dev (web
       view) without going through the tauri dialog plugin. The user
       gets a `<name>.canvas.json` saved through the OS download flow. */
    const blob = new Blob([json], { type: 'application/json' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = `${name.replace(/[^\w. -]+/g, '_') || 'canvas'}.canvas.json`;
    document.body.appendChild(a);
    a.click();
    a.remove();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  }

  function handleImport(e: MouseEvent) {
    e.stopPropagation();
    const input = document.createElement('input');
    input.type = 'file';
    input.accept = '.json,application/json';
    input.style.position = 'fixed';
    input.style.left = '-1000px';
    document.body.appendChild(input);
    input.onchange = async () => {
      const file = input.files?.[0];
      input.remove();
      if (!file) return;
      const text = await file.text();
      const id = importCanvasJson(text);
      if (id) {
        openCanvasInInstance(instanceId, id);
        notify({ kind: 'success', title: 'Imported', ttlMs: 1500 });
        onClose();
      } else {
        notify({ kind: 'error', title: 'Import failed', body: 'Not a valid Forge canvas JSON.' });
      }
    };
    input.click();
  }

  function startRename(id: string, current: string) {
    editingId = id;
    editingDraft = current;
  }
  function commitRename() {
    if (editingId && editingDraft.trim()) renameCanvas(editingId, editingDraft.trim());
    editingId = null;
    editingDraft = '';
  }
  function cancelRename() {
    editingId = null;
    editingDraft = '';
  }

  /** Esc closes the overlay (unless an inline rename is active — then
   *  Esc cancels just the rename). Bound globally; we listen on window
   *  during mount and clean up on destroy. */
  function onKeyDown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      if (editingId) { cancelRename(); return; }
      e.preventDefault();
      onClose();
    }
  }

  // No `onMount`/`return` because this component re-renders cheaply; we
  // attach the key handler via a `{@attach}` so it tracks the rendered
  // node lifetime exactly.
  function attachKey(_node: HTMLDivElement) {
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }

  /** Stylized gradient fallback when no shapes are available. */
  function previewGradient(entry: CanvasIndexEntry): string {
    let hash = 0;
    for (let i = 0; i < entry.name.length; i++) {
      hash = (hash * 31 + entry.name.charCodeAt(i)) | 0;
    }
    const hue = Math.abs(hash) % 360;
    return `linear-gradient(135deg, hsl(${hue}, 35%, 22%) 0%, hsl(${(hue + 40) % 360}, 30%, 14%) 100%)`;
  }

  /** Per-kind fill/stroke for the mini SVG preview. */
  function shapeColor(kind: string): { fill: string; stroke: string } {
    switch (kind) {
      case 'rect':              return { fill: 'rgba(255,255,255,0.05)', stroke: '#9094a0' };
      case 'ellipse':           return { fill: 'rgba(255,255,255,0.05)', stroke: '#9094a0' };
      case 'sticky':            return { fill: 'rgba(232,130,100,0.18)', stroke: 'rgba(232,130,100,0.6)' };
      case 'mermaid':
      case 'dot':               return { fill: 'rgba(14,165,233,0.12)', stroke: 'rgba(14,165,233,0.6)' };
      case 'code':              return { fill: 'rgba(168,85,247,0.12)', stroke: 'rgba(168,85,247,0.6)' };
      case 'jira-card':         return { fill: 'rgba(38,132,255,0.12)', stroke: '#2684FF' };
      case 'github-pr-card':
      case 'github-issue-card': return { fill: 'rgba(181,132,255,0.12)', stroke: '#8B5CF6' };
      case 'sentry-event-card': return { fill: 'rgba(232,130,100,0.12)', stroke: '#F88F74' };
      default:                  return { fill: 'rgba(125,201,176,0.10)', stroke: 'rgba(125,201,176,0.45)' };
    }
  }

  /** Render a compact SVG preview of the canvas shapes. Falls back to
   *  null when the canvas can't be loaded or has no shapes. */
  function makeSvgPreview(id: string): string | null {
    const c = ensureCanvasLoaded(id);
    if (!c || c.shapes.length === 0) return null;
    const visible = c.shapes.filter((s) => !s.hidden).slice(0, 60);
    if (visible.length === 0) return null;

    let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
    for (const s of visible) {
      if (s.x < minX) minX = s.x;
      if (s.y < minY) minY = s.y;
      if (s.x + s.w > maxX) maxX = s.x + s.w;
      if (s.y + s.h > maxY) maxY = s.y + s.h;
    }
    const PAD = Math.max((maxX - minX) * 0.08, 20);
    const vx = minX - PAD;
    const vy = minY - PAD;
    const vw = maxX - minX + PAD * 2;
    const vh = maxY - minY + PAD * 2;

    const els = visible.map((s: Shape) => {
      const { fill, stroke } = shapeColor(s.kind);
      if (s.kind === 'ellipse') {
        return `<ellipse cx="${s.x + s.w / 2}" cy="${s.y + s.h / 2}" rx="${s.w / 2}" ry="${s.h / 2}" fill="${fill}" stroke="${stroke}" stroke-width="1.5"/>`;
      }
      const r = s.kind === 'sticky' ? 6 : 3;
      return `<rect x="${s.x}" y="${s.y}" width="${s.w}" height="${s.h}" rx="${r}" fill="${fill}" stroke="${stroke}" stroke-width="1.5"/>`;
    }).join('');

    return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="${vx} ${vy} ${vw} ${vh}" width="100%" height="100%" preserveAspectRatio="xMidYMid meet">${els}</svg>`;
  }
</script>

<div
  class="cv-library"
  role="dialog"
  aria-label="Canvas library"
  aria-modal="true"
  {@attach attachKey}
>
  <!-- Backdrop button intercepts outside-clicks. Sized to fill via CSS;
       sits behind the panel so the panel's pointer-events still work. -->
  <button
    type="button"
    class="cv-library-backdrop"
    aria-label="Close library"
    onclick={onClose}
  ></button>
  <div
    class="cv-library-panel"
    role="document"
  >
    <header class="cv-library-head">
      <h2 class="cv-library-title">Canvases</h2>
      <input
        class="cv-library-search"
        placeholder="Search by name…"
        bind:value={query}
        {@attach (n: HTMLInputElement) => { n.focus(); }}
      />
      <div class="cv-library-actions">
        <button class="btn" onclick={handleImport} title="Import .canvas.json">Import</button>
        <button class="btn btn--primary" onclick={handleNew}>+ New</button>
        <button class="cv-library-close" onclick={onClose} aria-label="Close">
          <svg viewBox="0 0 24 24" width="14" height="14"><path d="M18 6 6 18M6 6l12 12" stroke="currentColor" stroke-width="2" stroke-linecap="round" fill="none"/></svg>
        </button>
      </div>
    </header>

    <div class="cv-library-body">
      {#if activeOnes.length === 0 && archivedOnes.length === 0}
        <div class="cv-library-empty">
          <p>No canvases yet.</p>
          <button class="btn btn--primary" onclick={handleNew}>+ New canvas</button>
        </div>
      {:else}
        {#if activeOnes.length > 0}
          <h3 class="cv-library-section">Active</h3>
          <div class="cv-library-grid">
            {#each activeOnes as entry (entry.id)}
              <div
                class="cv-tile"
                class:cv-tile--current={entry.id === activeCanvasId}
                role="button"
                tabindex="0"
                onclick={() => handleOpen(entry.id)}
                onkeydown={(e) => { if (e.key === 'Enter') { e.preventDefault(); handleOpen(entry.id); } }}
              >
                {@const svgPreview = makeSvgPreview(entry.id)}
                <div
                  class="cv-tile-preview"
                  style={svgPreview ? 'background: var(--bg-0)' : `background: ${previewGradient(entry)}`}
                >
                  {#if svgPreview}
                    {@html svgPreview}
                  {:else}
                    <span class="cv-tile-shape-count mono">{entry.shapeCount} shapes</span>
                  {/if}
                </div>
                <div class="cv-tile-info">
                  {#if editingId === entry.id}
                    <input
                      class="cv-tile-rename"
                      bind:value={editingDraft}
                      onblur={commitRename}
                      onkeydown={(e) => {
                        if (e.key === 'Enter') { e.preventDefault(); commitRename(); }
                        if (e.key === 'Escape') { e.preventDefault(); cancelRename(); }
                      }}
                      onclick={(e) => e.stopPropagation()}
                      {@attach (n: HTMLInputElement) => { n.focus(); n.select(); }}
                    />
                  {:else}
                    <button
                      class="cv-tile-name"
                      ondblclick={(e) => { e.stopPropagation(); startRename(entry.id, entry.name); }}
                      title="Double-click to rename"
                      onclick={(e) => e.stopPropagation()}
                    >{entry.name}</button>
                  {/if}
                  <span class="cv-tile-time mono">{fmtRelative(entry.updatedAt)}</span>
                </div>
                <div class="cv-tile-actions">
                  <button
                    class="cv-tile-act"
                    onclick={(e) => handleDuplicate(e, entry.id)}
                    title="Duplicate"
                    aria-label="Duplicate"
                  >
                    <svg viewBox="0 0 24 24" width="13" height="13" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><rect x="8" y="8" width="12" height="12" rx="2"/><path d="M16 8V6a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v8a2 2 0 0 0 2 2h2"/></svg>
                  </button>
                  <button
                    class="cv-tile-act"
                    onclick={(e) => handleExport(e, entry.id, entry.name)}
                    title="Export JSON"
                    aria-label="Export"
                  >
                    <svg viewBox="0 0 24 24" width="13" height="13" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M12 4v12M7 11l5 5 5-5M5 20h14"/></svg>
                  </button>
                  <button
                    class="cv-tile-act"
                    onclick={(e) => handleArchive(e, entry.id)}
                    title="Archive"
                    aria-label="Archive"
                  >
                    <svg viewBox="0 0 24 24" width="13" height="13" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M3 7h18M5 7v12a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V7M9 11h6"/></svg>
                  </button>
                  <button
                    class="cv-tile-act cv-tile-act--danger"
                    onclick={(e) => handleDelete(e, entry.id, entry.name)}
                    title="Delete (no undo)"
                    aria-label="Delete"
                  >
                    <svg viewBox="0 0 24 24" width="13" height="13" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M3 6h18M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2M6 6l1 14a2 2 0 0 0 2 2h6a2 2 0 0 0 2-2l1-14M10 11v6M14 11v6"/></svg>
                  </button>
                </div>
              </div>
            {/each}
          </div>
        {/if}

        {#if archivedOnes.length > 0}
          <h3 class="cv-library-section cv-library-section--muted">Archived</h3>
          <div class="cv-library-grid">
            {#each archivedOnes as entry (entry.id)}
              <div
                class="cv-tile cv-tile--archived"
                role="button"
                tabindex="0"
                onclick={() => handleOpen(entry.id)}
                onkeydown={(e) => { if (e.key === 'Enter') { e.preventDefault(); handleOpen(entry.id); } }}
              >
                {@const svgPreview = makeSvgPreview(entry.id)}
                <div
                  class="cv-tile-preview"
                  style={svgPreview ? 'background: var(--bg-0)' : `background: ${previewGradient(entry)}`}
                >
                  {#if svgPreview}
                    {@html svgPreview}
                  {:else}
                    <span class="cv-tile-shape-count mono">{entry.shapeCount} shapes</span>
                  {/if}
                </div>
                <div class="cv-tile-info">
                  <span class="cv-tile-name cv-tile-name--archived">{entry.name}</span>
                  <span class="cv-tile-time mono">archived {fmtRelative(entry.archivedAt ?? entry.updatedAt)}</span>
                </div>
                <div class="cv-tile-actions">
                  <button
                    class="cv-tile-act"
                    onclick={(e) => handleUnarchive(e, entry.id)}
                    title="Unarchive"
                    aria-label="Unarchive"
                  >
                    <svg viewBox="0 0 24 24" width="13" height="13" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M21 8v13H3V8M1 3h22v5H1zM10 12h4"/></svg>
                  </button>
                  <button
                    class="cv-tile-act cv-tile-act--danger"
                    onclick={(e) => handleDelete(e, entry.id, entry.name)}
                    title="Delete forever"
                    aria-label="Delete"
                  >
                    <svg viewBox="0 0 24 24" width="13" height="13" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M3 6h18M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2M6 6l1 14a2 2 0 0 0 2 2h6a2 2 0 0 0 2-2l1-14"/></svg>
                  </button>
                </div>
              </div>
            {/each}
          </div>
        {/if}
      {/if}
    </div>
  </div>
</div>

<style>
  .cv-library {
    position: absolute;
    inset: 0;
    background: rgba(0, 0, 0, 0.55);
    backdrop-filter: blur(4px);
    z-index: 10;
    display: flex;
    align-items: stretch;
    justify-content: stretch;
  }
  .cv-library-backdrop {
    position: absolute;
    inset: 0;
    background: transparent;
    border: 0;
    padding: 0;
    cursor: default;
  }
  .cv-library-panel {
    position: relative;
    flex: 1;
    margin: 14px;
    z-index: 1;
    background: var(--bg-1);
    border: 1px solid var(--border-neutral);
    border-radius: 12px;
    box-shadow: 0 24px 48px rgba(0, 0, 0, 0.4);
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }
  .cv-library-head {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 14px 18px;
    border-bottom: 1px solid var(--border-neutral);
    background: var(--bg-1);
  }
  .cv-library-title {
    font-size: 16px;
    font-weight: 600;
    color: var(--text-0);
    margin: 0;
    margin-right: 4px;
  }
  .cv-library-search {
    flex: 1;
    min-width: 0;
    background: var(--bg-2);
    border: 1px solid var(--border-neutral);
    border-radius: 6px;
    padding: 6px 10px;
    font-size: 13px;
    color: var(--text-0);
    outline: none;
  }
  .cv-library-search:focus { border-color: var(--accent); }
  .cv-library-actions { display: flex; align-items: center; gap: 6px; }
  .cv-library-close {
    width: 26px;
    height: 26px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background: transparent;
    border: 1px solid var(--border-neutral);
    color: var(--text-1);
    border-radius: 6px;
    cursor: pointer;
  }
  .cv-library-close:hover { background: var(--bg-2); color: var(--text-0); }

  .cv-library-body {
    flex: 1;
    overflow-y: auto;
    padding: 16px 18px 24px;
  }
  .cv-library-empty {
    height: 100%;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 12px;
    color: var(--text-1);
  }
  .cv-library-section {
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--text-2);
    margin: 8px 0 10px;
  }
  .cv-library-section--muted { color: var(--text-mute); }

  .cv-library-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
    gap: 12px;
  }

  .cv-tile {
    background: var(--bg-2);
    border: 1px solid var(--border-neutral);
    border-radius: 10px;
    overflow: hidden;
    cursor: pointer;
    transition: border-color 120ms, transform 120ms, box-shadow 120ms;
    display: flex;
    flex-direction: column;
  }
  .cv-tile:hover {
    border-color: var(--accent);
    transform: translateY(-1px);
    box-shadow: 0 6px 16px rgba(0, 0, 0, 0.35);
  }
  .cv-tile--current { outline: 2px solid var(--accent); outline-offset: 1px; }
  .cv-tile--archived { opacity: 0.7; }

  .cv-tile-preview {
    position: relative;
    aspect-ratio: 16 / 9;
    display: flex;
    align-items: flex-end;
    justify-content: flex-end;
    padding: 6px 8px;
  }
  .cv-tile-shape-count {
    font-size: 10px;
    color: rgba(255, 255, 255, 0.6);
    background: rgba(0, 0, 0, 0.35);
    padding: 1px 6px;
    border-radius: 3px;
  }

  .cv-tile-info {
    padding: 8px 10px 4px;
    display: flex;
    flex-direction: column;
    gap: 1px;
    min-width: 0;
  }
  .cv-tile-name {
    background: transparent;
    border: 0;
    padding: 0;
    margin: 0;
    text-align: left;
    font-size: 13px;
    font-weight: 500;
    color: var(--text-0);
    cursor: text;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .cv-tile-name--archived { color: var(--text-1); }
  .cv-tile-rename {
    background: var(--bg-0);
    border: 1px solid var(--accent);
    color: var(--text-0);
    border-radius: 4px;
    padding: 2px 6px;
    font-size: 13px;
    font-weight: 500;
    width: 100%;
    outline: none;
    box-sizing: border-box;
  }
  .cv-tile-time { font-size: 10px; color: var(--text-2); }

  .cv-tile-actions {
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 2px 6px 6px;
    margin-top: auto;
    opacity: 0;
    transition: opacity 100ms;
  }
  .cv-tile:hover .cv-tile-actions { opacity: 1; }
  .cv-tile-act {
    width: 22px;
    height: 22px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background: transparent;
    border: 1px solid transparent;
    color: var(--text-2);
    border-radius: 5px;
    cursor: pointer;
  }
  .cv-tile-act:hover { background: var(--bg-1); color: var(--text-0); border-color: var(--border-neutral); }
  .cv-tile-act--danger:hover { color: var(--error, #E88264); border-color: rgba(232, 130, 100, 0.4); }
</style>
