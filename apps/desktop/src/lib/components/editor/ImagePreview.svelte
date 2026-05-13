<script lang="ts">
  /* ImagePreview — opens an image file (PNG/JPEG/GIF/WebP/BMP/SVG)
     in the editor surface as an actual image, not a binary blob in
     CodeMirror. Without this, opening a screenshot from a tab dumps
     a wall of garbled UTF-8 — disorienting and useless.

     Tauri's asset:// protocol is already wired up via
     convertFileSrc; we just hand it the absolute path. The webview
     reads bytes through the asset scope (configured in
     tauri.conf.json), so we don't have to base64-encode and don't
     blow the IPC channel. SVG falls into the same path — it's just
     text, but the asset URL works for `<img>` regardless.

     Controls:
     - Fit / Actual size toggle (button + double-click image).
     - Wheel-zoom on the image when not fit-mode.
     - Reveal in Finder, Copy path — shared with the editor status
       bar but useful here too since the user is staring at an image
       with no obvious next move. */

  import { invoke, convertFileSrc } from '@tauri-apps/api/core';

  interface Props {
    /** Absolute path of the image file. */
    path: string;
  }
  let { path }: Props = $props();

  /* `fit` keeps the image scaled to viewport (the default — most
     images you open are screenshots and you want to see the whole
     thing). `actual` swaps to 1:1 + pan; useful for inspecting a
     pixel-detailed asset without external tools. */
  let mode = $state<'fit' | 'actual'>('fit');
  /* Pan offset in `actual` mode; reset when switching back to fit
     so the user doesn't return to a randomly-scrolled state. */
  let panX = $state(0);
  let panY = $state(0);
  /* Custom zoom on top of `actual` (1.0 = native). Wheel adjusts;
     reset to 1 on `actual ↔ fit` swap. Clamped so a stray scroll
     doesn't zoom into oblivion. */
  let zoom = $state(1);

  let dims = $state<{ w: number; h: number } | null>(null);
  let loaded = $state(false);
  let error = $state<string | null>(null);

  const src = $derived(path ? convertFileSrc(path) : '');
  const baseName = $derived(path.split('/').pop() || path);
  const ext = $derived(() => {
    const dot = baseName.lastIndexOf('.');
    return dot < 0 ? '' : baseName.slice(dot + 1).toUpperCase();
  });

  /* Reset state on path change so swapping screenshot ↔ logo doesn't
     leak the previous file's pan or "actual" zoom into the new view. */
  $effect(() => {
    void path;
    mode = 'fit';
    panX = 0;
    panY = 0;
    zoom = 1;
    dims = null;
    loaded = false;
    error = null;
  });

  function onLoad(ev: Event) {
    const img = ev.currentTarget as HTMLImageElement;
    /* SVGs may report 0 (intrinsic-less). Fall back to the rendered
       client size — best we can do without parsing the SVG ourselves. */
    const w = img.naturalWidth || img.clientWidth || 0;
    const h = img.naturalHeight || img.clientHeight || 0;
    dims = { w, h };
    loaded = true;
  }
  function onError() { error = 'Failed to load image. The file may be corrupted or unsupported.'; }

  function toggleMode() {
    mode = mode === 'fit' ? 'actual' : 'fit';
    panX = 0;
    panY = 0;
    zoom = 1;
  }

  /* Wheel-zoom only in actual mode — in fit mode, scroll should
     scroll the surrounding pane, not zoom (which would feel jumpy
     since the pane is already auto-fitting). */
  function onWheel(e: WheelEvent) {
    if (mode !== 'actual') return;
    e.preventDefault();
    /* Smooth-feeling zoom — 0.1 step per notch, clamped to a sane
       0.1×–8× range. */
    const next = Math.max(0.1, Math.min(8, zoom * (e.deltaY < 0 ? 1.1 : 1 / 1.1)));
    zoom = Number(next.toFixed(3));
  }

  /* Click-and-drag pan in actual mode. Pointer events because they
     unify mouse + trackpad + pen and let us release on
     pointer-out-of-window cleanly. */
  let dragging = $state(false);
  let dragStart = { x: 0, y: 0, panX: 0, panY: 0 };
  function onPointerDown(e: PointerEvent) {
    if (mode !== 'actual') return;
    if (e.button !== 0) return;
    dragging = true;
    dragStart = { x: e.clientX, y: e.clientY, panX, panY };
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  }
  function onPointerMove(e: PointerEvent) {
    if (!dragging) return;
    panX = dragStart.panX + (e.clientX - dragStart.x);
    panY = dragStart.panY + (e.clientY - dragStart.y);
  }
  function onPointerUp(e: PointerEvent) {
    dragging = false;
    try { (e.currentTarget as HTMLElement).releasePointerCapture(e.pointerId); } catch { /* already released */ }
  }

  async function reveal() {
    try { await invoke('fs_reveal_in_finder', { path }); } catch { /* no-op — best-effort */ }
  }
  async function copyPath() {
    try { await navigator.clipboard.writeText(path); } catch { /* clipboard may be denied — silent */ }
  }
</script>

<div class="ip">
  <div
    class="ip-stage"
    class:ip-stage--actual={mode === 'actual'}
    class:ip-stage--dragging={dragging}
    onwheel={onWheel}
    onpointerdown={onPointerDown}
    onpointermove={onPointerMove}
    onpointerup={onPointerUp}
    onpointercancel={onPointerUp}
    role="img"
    aria-label={baseName}
  >
    {#if error}
      <div class="ip-error">{error}</div>
    {:else if src}
      <img
        class="ip-img"
        class:ip-img--fit={mode === 'fit'}
        class:ip-img--actual={mode === 'actual'}
        style:transform={mode === 'actual' ? `translate(${panX}px, ${panY}px) scale(${zoom})` : ''}
        {src}
        alt={baseName}
        onload={onLoad}
        onerror={onError}
        ondblclick={toggleMode}
        draggable="false"
      />
    {/if}
  </div>

  <div class="ip-bar">
    <span class="ip-name mono">{baseName}</span>
    <span class="ip-meta mono">
      {#if loaded && dims}
        {dims.w} × {dims.h}{ext() ? ` · ${ext()}` : ''}
      {:else if !error}
        loading…
      {/if}
    </span>
    <span class="ip-spacer"></span>
    <button class="ip-btn" onclick={toggleMode} title="Toggle fit / actual size (or double-click image)">
      {mode === 'fit' ? 'Actual size' : 'Fit'}
    </button>
    {#if mode === 'actual'}
      <span class="ip-zoom mono" title="Wheel to zoom · drag to pan">{Math.round(zoom * 100)}%</span>
    {/if}
    <button class="ip-btn" onclick={copyPath} title="Copy absolute path to clipboard">Copy path</button>
    <button class="ip-btn" onclick={reveal} title="Reveal in Finder">Reveal</button>
  </div>
</div>

<style>
  .ip {
    height: 100%;
    display: flex; flex-direction: column;
    background: var(--bg-0);
    min-height: 0;
  }
  .ip-stage {
    flex: 1; min-height: 0;
    overflow: hidden;
    display: grid; place-items: center;
    /* Soft checkerboard so transparent PNGs read as transparent
       instead of "looking like the background colour broke". 16px
       cells; tuned to feel like macOS Preview without being noisy. */
    background-color: var(--bg-1);
    background-image:
      linear-gradient(45deg, var(--bg-2) 25%, transparent 25%),
      linear-gradient(-45deg, var(--bg-2) 25%, transparent 25%),
      linear-gradient(45deg, transparent 75%, var(--bg-2) 75%),
      linear-gradient(-45deg, transparent 75%, var(--bg-2) 75%);
    background-size: 18px 18px;
    background-position: 0 0, 0 9px, 9px -9px, -9px 0;
    cursor: zoom-in;
    user-select: none;
  }
  .ip-stage--actual { cursor: grab; }
  .ip-stage--dragging { cursor: grabbing; }
  .ip-img {
    display: block;
    /* Soft shadow lifts the image off the checkerboard so the eye
       reads "the image" not "the surrounding noise". */
    box-shadow: 0 4px 24px rgba(0, 0, 0, 0.35);
    pointer-events: none;
    will-change: transform;
  }
  .ip-img--fit {
    max-width: calc(100% - 48px);
    max-height: calc(100% - 48px);
    object-fit: contain;
  }
  .ip-img--actual {
    transform-origin: center center;
    pointer-events: auto;
  }

  .ip-error {
    padding: 12px 16px;
    background: rgba(232, 130, 100, 0.12);
    color: var(--error);
    border-radius: 8px;
    font-size: 12.5px;
    border: 1px solid rgba(232, 130, 100, 0.3);
  }

  .ip-bar {
    display: flex; align-items: center; gap: 10px;
    padding: 6px 14px;
    border-top: 1px solid var(--border);
    background: var(--bg-1);
    flex-shrink: 0;
    height: 28px;
    font-size: 11px;
  }
  .ip-name {
    color: var(--text-0);
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
    max-width: 320px;
  }
  .ip-meta { color: var(--text-mute); font-size: 10.5px; }
  .ip-spacer { flex: 1; }
  .ip-zoom {
    color: var(--accent-bright);
    font-size: 10.5px;
    padding: 0 6px;
    border-radius: 3px;
    background: color-mix(in srgb, var(--accent) 12%, transparent);
  }
  .ip-btn {
    background: transparent;
    border: 1px solid var(--border);
    border-radius: 5px;
    padding: 2px 9px;
    color: var(--text-1);
    font-size: 11px;
    cursor: pointer;
    transition: background 120ms, color 120ms, border-color 120ms;
  }
  .ip-btn:hover {
    background: var(--bg-2);
    color: var(--text-0);
    border-color: var(--border-hi);
  }
</style>
