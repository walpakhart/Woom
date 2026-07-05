<script lang="ts">
  /* Unified model + effort picker — one quiet paper menu, per the
     redesign grammar: printed card (bg-2, hairline, engraved step
     shadow), two labelled sections rendered as plain rows with a
     check on the active one. Replaces the old canvas "reactor"
     (matrix glyph header + gradient slider) wholesale. */

  interface Props {
    model: string;
    modelOptions: { value: string; label: string }[];
    effort: string;
    effortOptions: { value: string; label: string }[];
    onModelChange: (value: string) => void;
    onEffortChange: (value: string) => void;
  }
  let { model, modelOptions, effort, effortOptions, onModelChange, onEffortChange }: Props =
    $props();

  function shortEffort(raw: string): string {
    const s = raw.replace(/^effort\s*[·:-]\s*/i, '').trim();
    return s.charAt(0).toUpperCase() + s.slice(1);
  }

  const curModel = $derived(modelOptions.find((m) => m.value === model) ?? modelOptions[0]);
  const curEffort = $derived(
    effortOptions.find((o) => o.value === effort) ?? effortOptions[0]
  );
  const chipLabel = $derived(
    `${curModel?.label ?? 'model'}${curEffort ? ` · ${shortEffort(curEffort.label)}` : ''}`
  );

  let open = $state(false);
  let triggerEl: HTMLButtonElement | null = $state(null);
  let panelEl: HTMLDivElement | null = $state(null);
  let coords = $state<{ left: number; top: number } | null>(null);

  function openPanel() {
    open = true;
    requestAnimationFrame(align);
  }
  function close() {
    open = false;
    coords = null;
  }
  function toggle() {
    if (open) close();
    else openPanel();
  }
  function align() {
    if (!triggerEl || !panelEl) return;
    const t = triggerEl.getBoundingClientRect();
    const p = panelEl.getBoundingClientRect();
    const margin = 10;
    let left = t.left;
    if (left + p.width > window.innerWidth - margin)
      left = Math.max(margin, window.innerWidth - margin - p.width);
    coords = { left, top: Math.max(margin, t.top - p.height - 8) };
  }

  function pickModel(v: string) {
    if (v !== model) onModelChange(v);
  }
  function pickEffort(v: string) {
    if (v !== effort) onEffortChange(v);
  }

  /* ---- effort slider ---- */
  const ei = $derived(Math.max(0, effortOptions.findIndex((o) => o.value === effort)));
  const efRatio = $derived(effortOptions.length <= 1 ? 0 : ei / (effortOptions.length - 1));
  let trackEl: HTMLDivElement | null = $state(null);
  let dragging = $state(false);
  function setEffortIdx(i: number) {
    const c = Math.max(0, Math.min(effortOptions.length - 1, i));
    const o = effortOptions[c];
    if (o && o.value !== effort) onEffortChange(o.value);
  }
  function idxFromClientX(clientX: number): number {
    if (!trackEl || effortOptions.length <= 1) return ei;
    const r = trackEl.getBoundingClientRect();
    const ratio = Math.max(0, Math.min(1, (clientX - r.left) / r.width));
    return Math.round(ratio * (effortOptions.length - 1));
  }
  function onTrackDown(e: PointerEvent) {
    dragging = true;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    setEffortIdx(idxFromClientX(e.clientX));
  }
  function onTrackMove(e: PointerEvent) {
    if (dragging) setEffortIdx(idxFromClientX(e.clientX));
  }
  function onTrackUp(e: PointerEvent) {
    dragging = false;
    (e.currentTarget as HTMLElement).releasePointerCapture?.(e.pointerId);
  }
  function onSliderKey(e: KeyboardEvent) {
    if (e.key === 'ArrowRight') { e.preventDefault(); setEffortIdx(ei + 1); }
    else if (e.key === 'ArrowLeft') { e.preventDefault(); setEffortIdx(ei - 1); }
    else if (e.key === 'Home') { e.preventDefault(); setEffortIdx(0); }
    else if (e.key === 'End') { e.preventDefault(); setEffortIdx(effortOptions.length - 1); }
  }

  function onKey(e: KeyboardEvent) {
    if (!open) {
      if (e.key === 'Enter' || e.key === ' ' || e.key === 'ArrowUp') {
        e.preventDefault();
        openPanel();
      }
      return;
    }
    if (e.key === 'Escape') {
      e.preventDefault();
      close();
      triggerEl?.focus();
    }
  }

  function onDocClick(e: MouseEvent) {
    if (!open) return;
    const target = e.target as Node;
    if (triggerEl?.contains(target) || panelEl?.contains(target)) return;
    close();
  }
  $effect(() => {
    if (!open) return;
    document.addEventListener('mousedown', onDocClick);
    const onSR = () => align();
    window.addEventListener('scroll', onSR, true);
    window.addEventListener('resize', onSR);
    return () => {
      document.removeEventListener('mousedown', onDocClick);
      window.removeEventListener('scroll', onSR, true);
      window.removeEventListener('resize', onSR);
    };
  });
</script>

<button
  bind:this={triggerEl}
  class="me-chip"
  onclick={toggle}
  onkeydown={onKey}
  aria-haspopup="menu"
  aria-expanded={open}
  title="Model · effort"
>
  <span class="me-chip-label">{chipLabel}</span>
  <svg class="me-chip-caret" class:open viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M6 9l6 6 6-6"/></svg>
</button>

{#if open}
  <div
    bind:this={panelEl}
    class="me-panel"
    role="menu"
    style={coords ? `left:${coords.left}px; top:${coords.top}px;` : 'visibility:hidden; left:0; top:0;'}
    onkeydown={onKey}
  >
    <div class="me-label">Model</div>
    {#each modelOptions as m (m.value)}
      <button
        class="me-row"
        class:active={m.value === model}
        role="menuitemradio"
        aria-checked={m.value === model}
        onclick={() => pickModel(m.value)}
      >
        <span class="me-row-label">{m.label}</span>
        {#if m.value === model}<span class="me-check" aria-hidden="true">✓</span>{/if}
      </button>
    {/each}

    <div class="me-rule" aria-hidden="true"></div>

    <div class="me-label me-label--row">
      <span>Effort</span>
      <span class="me-effort-cur">{shortEffort(curEffort?.label ?? '')}</span>
    </div>
    <div
      bind:this={trackEl}
      class="me-track"
      role="slider"
      tabindex="0"
      aria-valuemin={0}
      aria-valuemax={effortOptions.length - 1}
      aria-valuenow={ei}
      aria-valuetext={shortEffort(curEffort?.label ?? '')}
      onpointerdown={onTrackDown}
      onpointermove={onTrackMove}
      onpointerup={onTrackUp}
      onkeydown={onSliderKey}
    >
      <div class="me-track-rail"></div>
      <div class="me-track-fill" style:width="{efRatio * 100}%"></div>
      {#each effortOptions as o, i (o.value)}
        <button
          class="me-notch"
          class:passed={i <= ei}
          style:left="{effortOptions.length <= 1 ? 0 : (i / (effortOptions.length - 1)) * 100}%"
          title={shortEffort(o.label)}
          aria-label={shortEffort(o.label)}
          onclick={() => pickEffort(o.value)}
          tabindex="-1"
        ></button>
      {/each}
      <div class="me-thumb" class:dragging style:left="{efRatio * 100}%"></div>
    </div>
    <div class="me-track-ends" aria-hidden="true">
      <span>faster</span>
      <span>deeper</span>
    </div>
  </div>
{/if}

<style>
  .me-chip {
    display: inline-flex; align-items: center; gap: 6px;
    font-size: 10.5px;
    color: var(--text-1);
    border: 1px solid var(--border-hi);
    border-radius: var(--r-chip);
    padding: 2px 8px;
    background: transparent;
    cursor: pointer;
    white-space: nowrap;
    transition: color 120ms, border-color 120ms;
  }
  .me-chip:hover { color: var(--text-0); border-color: var(--border-hi2); }
  .me-chip-caret {
    width: 10px; height: 10px;
    transition: transform 140ms;
  }
  .me-chip-caret.open { transform: rotate(180deg); }

  /* Printed paper menu. */
  .me-panel {
    position: fixed;
    z-index: 300;
    min-width: 210px;
    padding: 6px;
    background: var(--bg-2);
    border: 1px solid var(--border-hi);
    border-radius: var(--r-card);
    box-shadow: var(--shadow-2);
  }
  .me-label {
    font-size: 9.5px; font-weight: 600;
    letter-spacing: 0.14em; text-transform: uppercase;
    color: var(--text-faint);
    padding: 6px 8px 4px;
  }
  .me-rule {
    height: 1px;
    background: var(--border-lo);
    margin: 6px 2px;
  }
  .me-row {
    display: flex; align-items: center; gap: 8px;
    width: 100%;
    padding: 5px 8px;
    border: 0;
    border-radius: var(--r-btn);
    background: transparent;
    font-size: 11.5px;
    color: var(--text-1);
    cursor: pointer;
    text-align: left;
    transition: background 100ms, color 100ms;
  }
  .me-row:hover { background: var(--bg-hover); color: var(--text-0); }
  .me-row.active { background: var(--bg-nav); color: var(--text-0); font-weight: 600; }
  .me-row-label { flex: 1; min-width: 0; white-space: nowrap; }
  .me-check { flex: none; font-size: 10px; color: var(--src-claude); }

  /* ---- effort slider — stepped ink line, engraved thumb ---- */
  .me-label--row {
    display: flex; align-items: baseline; justify-content: space-between;
  }
  .me-effort-cur {
    font-size: 10px; font-weight: 600;
    letter-spacing: 0.02em; text-transform: none;
    color: var(--text-0);
  }
  .me-track {
    position: relative;
    height: 24px;
    margin: 4px 10px 0;
    cursor: pointer;
    touch-action: none;
  }
  .me-track:focus-visible { outline: none; }
  .me-track-rail {
    position: absolute; left: 0; right: 0; top: 50%;
    height: 2px; transform: translateY(-50%);
    background: var(--bg-4);
    border-radius: 1px;
  }
  .me-track-fill {
    position: absolute; left: 0; top: 50%;
    height: 2px; transform: translateY(-50%);
    background: var(--text-0);
    border-radius: 1px;
    pointer-events: none;
  }
  .me-notch {
    position: absolute; top: 50%;
    width: 5px; height: 5px;
    transform: translate(-50%, -50%);
    border-radius: 50%;
    border: 0; padding: 0;
    background: var(--bg-4);
    box-shadow: 0 0 0 2px var(--bg-2);
    cursor: pointer;
  }
  .me-notch.passed { background: var(--text-0); }
  .me-thumb {
    position: absolute; top: 50%;
    width: 12px; height: 12px;
    transform: translate(-50%, -50%);
    border-radius: 50%;
    background: var(--text-0);
    box-shadow: var(--shadow-pill);
    pointer-events: none;
    transition: left 120ms var(--ease-out, ease-out);
  }
  .me-thumb.dragging { transition: none; }
  .me-track-ends {
    display: flex; justify-content: space-between;
    padding: 4px 10px 2px;
    font-size: 9px; font-weight: 600;
    letter-spacing: 0.12em; text-transform: uppercase;
    color: var(--text-faint);
  }
</style>
