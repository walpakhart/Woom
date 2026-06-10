<script lang="ts">
  // Unified model + effort control ("engine"). MODEL = its own signature
  // canvas animation (identity): Fable→rain, Mythos→glitch, Opus→orbit,
  // Sonnet→waves, Haiku→pulse. EFFORT never changes the KIND — it modulates
  // temperament: low = fast + sparse (faster, shallower), high = slow + dense
  // (deeper, deliberate). The chip carries a tiny live signature; the panel
  // hosts a full reactor + model pills + effort track. Canvas freezes to a
  // single static frame under prefers-reduced-motion (the CSS-only reset can't
  // touch a <canvas> draw loop, so we gate the rAF in JS).
  import { fade } from 'svelte/transition';

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

  type Kind = 'rain' | 'glitch' | 'orbit' | 'waves' | 'pulse';
  function sigKind(m: string): Kind {
    if (m.startsWith('claude-fable-5')) return 'rain';
    if (m.startsWith('claude-mythos-5')) return 'glitch';
    if (m.startsWith('claude-opus')) return 'orbit';
    if (m.startsWith('claude-sonnet')) return 'waves';
    if (m.startsWith('claude-haiku')) return 'pulse';
    return 'waves';
  }

  function shortEffort(raw: string): string {
    const s = raw.replace(/^effort\s*[·:-]\s*/i, '').trim();
    return s.charAt(0).toUpperCase() + s.slice(1);
  }

  const efTiers = $derived(
    effortOptions.map((o) => ({ value: o.value, label: shortEffort(o.label) }))
  );
  const ei = $derived(Math.max(0, efTiers.findIndex((t) => t.value === effort)));
  const efRatio = $derived(efTiers.length <= 1 ? 0 : ei / (efTiers.length - 1));
  const isHot = $derived(efRatio >= 0.9);
  const fillPct = $derived(efRatio * 100);

  const curModel = $derived(modelOptions.find((m) => m.value === model) ?? modelOptions[0]);
  const modelLabel = $derived(curModel?.label ?? 'model');
  const effortLabel = $derived(efTiers[ei]?.label ?? '—');
  const kind = $derived(sigKind(model));

  function vibe(): string {
    const e = efRatio;
    if (e <= 0.2) return 'fastest · shallow';
    if (e < 0.45) return 'quick · light';
    if (e < 0.7) return 'balanced';
    if (e < 0.92) return 'deep · careful';
    return 'deepest · deliberate';
  }

  // --- popover open/position (fixed + align above, matches EffortSlider) ---
  let open = $state(false);
  let triggerEl: HTMLButtonElement | null = $state(null);
  let panelEl: HTMLDivElement | null = $state(null);
  let trackEl: HTMLDivElement | null = $state(null);
  let coords = $state<{ left: number; top: number } | null>(null);
  let measured = $state(false);

  function openPanel() {
    open = true;
    if (triggerEl) {
      const r = triggerEl.getBoundingClientRect();
      coords = { left: r.left, top: r.top };
    }
    requestAnimationFrame(align);
  }
  function close() {
    open = false;
    coords = null;
    measured = false;
  }
  function toggle() {
    if (open) close();
    else openPanel();
  }
  function align() {
    if (!triggerEl || !panelEl) return;
    const t = triggerEl.getBoundingClientRect();
    const p = panelEl.getBoundingClientRect();
    const margin = 12;
    let left = t.left + t.width / 2 - p.width / 2;
    if (left + p.width > window.innerWidth - margin)
      left = Math.max(margin, window.innerWidth - margin - p.width);
    if (left < margin) left = margin;
    coords = { left, top: t.top - p.height - 8 };
    measured = true;
  }

  function setEffortIdx(i: number) {
    const clamped = Math.max(0, Math.min(efTiers.length - 1, i));
    const t = efTiers[clamped];
    if (t && t.value !== effort) onEffortChange(t.value);
  }
  function idxFromClientX(clientX: number): number {
    if (!trackEl || efTiers.length <= 1) return ei;
    const r = trackEl.getBoundingClientRect();
    const ratio = Math.max(0, Math.min(1, (clientX - r.left) / r.width));
    return Math.round(ratio * (efTiers.length - 1));
  }
  let dragging = $state(false);
  function onTrackPointerDown(e: PointerEvent) {
    dragging = true;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    setEffortIdx(idxFromClientX(e.clientX));
  }
  function onTrackPointerMove(e: PointerEvent) {
    if (!dragging) return;
    setEffortIdx(idxFromClientX(e.clientX));
  }
  function onTrackPointerUp(e: PointerEvent) {
    dragging = false;
    (e.currentTarget as HTMLElement).releasePointerCapture?.(e.pointerId);
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
    } else if (e.key === 'ArrowRight') {
      e.preventDefault();
      setEffortIdx(ei + 1);
    } else if (e.key === 'ArrowLeft') {
      e.preventDefault();
      setEffortIdx(ei - 1);
    } else if (e.key === 'Home') {
      e.preventDefault();
      setEffortIdx(0);
    } else if (e.key === 'End') {
      e.preventDefault();
      setEffortIdx(efTiers.length - 1);
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

  // ── signature canvas engine ──────────────────────────────────────────────
  // Two canvases share one paint(): the always-mounted chip core (24px) and
  // the panel reactor (only mounted while open). A single rAF advances a shared
  // clock + per-kind state. speed()/depth() come from the effort ratio.
  let chipCanvas: HTMLCanvasElement | null = $state(null);
  let reactorCanvas: HTMLCanvasElement | null = $state(null);

  const GLYPHS = 'ｱｲｳｴｵｶｷｸｹｺ0123456789<>=/$';
  function accent(el: HTMLElement): string {
    return getComputedStyle(el).getPropertyValue('--accent-bright').trim() || '#B0DCC8';
  }

  type CtxBox = { ctx: CanvasRenderingContext2D; w: number; h: number; a: string };
  function fit(canvas: HTMLCanvasElement, dpr: number): CtxBox | null {
    const ctx = canvas.getContext('2d');
    if (!ctx) return null;
    const w = canvas.clientWidth || 24;
    const h = canvas.clientHeight || 24;
    canvas.width = Math.round(w * dpr);
    canvas.height = Math.round(h * dpr);
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    return { ctx, w, h, a: accent(canvas) };
  }

  let drops: number[] = [];
  let orbits: { a: number; r: number; sp: number }[] = [];
  let clock = 0;

  function reseed(w: number, h: number) {
    drops = new Array(Math.max(1, Math.floor(w / 9))).fill(0).map(() => Math.random() * -h);
    orbits = new Array(14)
      .fill(0)
      .map((_, i) => ({ a: Math.random() * 7, r: 8 + i * 2.4, sp: 0.4 + Math.random() * 0.5 }));
  }

  function paint(box: CtxBox, k: Kind, d: number, sm: boolean) {
    const { ctx, w, h, a } = box;
    ctx.clearRect(0, 0, w, h);
    if (k === 'rain') {
      ctx.fillStyle = 'rgba(0,0,0,.2)';
      ctx.fillRect(0, 0, w, h);
      ctx.font = (sm ? 8 : 11) + 'px ui-monospace, monospace';
      const step = 9;
      const cols = Math.max(1, Math.floor(w / step));
      const vis = Math.ceil(cols * (0.3 + 0.7 * d));
      for (let i = 0; i < vis; i++) {
        const x = i * step + 1;
        const y = drops[i % drops.length] ?? 0;
        ctx.globalAlpha = 0.95;
        ctx.fillStyle = a;
        ctx.fillText(GLYPHS[(Math.random() * GLYPHS.length) | 0], x, y);
        ctx.globalAlpha = 0.3;
        ctx.fillText(GLYPHS[(Math.random() * GLYPHS.length) | 0], x, y - (sm ? 8 : 11));
      }
      ctx.globalAlpha = 1;
    } else if (k === 'glitch') {
      ctx.fillStyle = 'rgba(0,0,0,.28)';
      ctx.fillRect(0, 0, w, h);
      const n = Math.round((sm ? 4 : 26) * (0.3 + 0.7 * d));
      ctx.fillStyle = a;
      for (let i = 0; i < n; i++) {
        if (Math.random() > 0.45) continue;
        ctx.globalAlpha = 0.2 + Math.random() * 0.7;
        ctx.fillRect(Math.random() * w, Math.random() * h, 2 + Math.random() * 8, 2);
      }
      ctx.globalAlpha = 1;
    } else if (k === 'orbit') {
      ctx.save();
      ctx.translate(w / 2, h / 2);
      const cnt = Math.round((sm ? 5 : 14) * (0.35 + 0.65 * d));
      const pts: [number, number][] = [];
      for (let i = 0; i < cnt; i++) {
        const o = orbits[i % orbits.length];
        const r = sm ? o.r * 0.5 : o.r;
        const x = Math.cos(o.a) * r;
        const y = Math.sin(o.a) * r * 0.7;
        pts.push([x, y]);
        ctx.globalAlpha = 0.85;
        ctx.fillStyle = a;
        ctx.beginPath();
        ctx.arc(x, y, sm ? 1 : 1.8, 0, 7);
        ctx.fill();
      }
      if (d > 0.6 && !sm) {
        ctx.globalAlpha = (0.18 * (d - 0.6)) / 0.4;
        ctx.strokeStyle = a;
        ctx.lineWidth = 0.8;
        for (let i = 0; i < pts.length; i++)
          for (let j = i + 1; j < pts.length; j++) {
            const dx = pts[i][0] - pts[j][0];
            const dy = pts[i][1] - pts[j][1];
            if (dx * dx + dy * dy < 420) {
              ctx.beginPath();
              ctx.moveTo(pts[i][0], pts[i][1]);
              ctx.lineTo(pts[j][0], pts[j][1]);
              ctx.stroke();
            }
          }
      }
      ctx.restore();
      ctx.globalAlpha = 1;
    } else if (k === 'waves') {
      const layers = Math.round((sm ? 2 : 4) * (0.5 + 0.5 * d));
      for (let l = 0; l < layers; l++) {
        ctx.beginPath();
        ctx.strokeStyle = a;
        ctx.globalAlpha = 0.14 + l * 0.07;
        ctx.lineWidth = sm ? 1 : 1.4;
        const amp = (sm ? 3 : 14) * (0.5 + 0.5 * d) - l * (sm ? 0.6 : 2.5);
        for (let x = 0; x <= w; x += sm ? 3 : 6) {
          const y = h / 2 + Math.sin(x / (sm ? 9 : 26) + clock / 420 + l) * amp;
          x ? ctx.lineTo(x, y) : ctx.moveTo(x, y);
        }
        ctx.stroke();
      }
      ctx.globalAlpha = 1;
    } else {
      ctx.save();
      ctx.translate(w / 2, h / 2);
      const rings = sm ? 2 : 3;
      for (let l = 0; l < rings; l++) {
        const ph = (clock / 600 + l / rings) % 1;
        const r = ph * (sm ? 9 : 34) * (0.6 + 0.4 * d);
        ctx.globalAlpha = (1 - ph) * 0.6;
        ctx.strokeStyle = a;
        ctx.lineWidth = sm ? 1 : 1.6;
        ctx.beginPath();
        ctx.arc(0, 0, r, 0, 7);
        ctx.stroke();
      }
      ctx.restore();
      ctx.globalAlpha = 1;
    }
  }

  // Drive the engine. Re-runs when chip/reactor mount or kind/effort change.
  $effect(() => {
    const chip = chipCanvas;
    const reactor = reactorCanvas;
    if (!chip) return;
    // reactive deps — re-fit + (for static path) repaint on change
    const k = kind;
    const ef = efRatio;
    void open;

    const reduce =
      typeof window !== 'undefined' &&
      window.matchMedia?.('(prefers-reduced-motion: reduce)').matches;
    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    const speed = 1.7 - 1.25 * ef; // 1.7 → 0.45
    const depth = 0.22 + 0.78 * ef; // 0.22 → 1.0

    let chipBox = fit(chip, dpr);
    let reactorBox = reactor ? fit(reactor, dpr) : null;
    // Reseed each run to the WIDEST live canvas so neither the chip
    // (collapsed) nor the reactor (open) renders sparse.
    const seedW = Math.max(reactorBox?.w ?? 0, chipBox?.w ?? 0, 24);
    const seedH = Math.max(reactorBox?.h ?? 0, chipBox?.h ?? 0, 24);
    // Reseed only when a wider canvas needs more columns (e.g. the panel
    // opened) — NOT on every effort tick, else the rain/orbits jump.
    const needCols = Math.max(1, Math.floor(seedW / 9));
    if (drops.length < needCols || !orbits.length) reseed(seedW, seedH);
    const fallH = Math.max(reactorBox?.h ?? 0, chipBox?.h ?? 0, 24);

    if (reduce) {
      // single deterministic frame, no loop. Chip = full-bleed signature
      // (sm=false), same density as the reactor.
      if (chipBox) paint(chipBox, k, depth, false);
      if (reactorBox) paint(reactorBox, k, depth, false);
      return;
    }

    let raf = 0;
    let last = 0;
    const fall = 4 + 8 * speed;
    const tick = (ts: number) => {
      raf = requestAnimationFrame(tick);
      const dt = ts - last;
      if (dt < 50) return; // ~20fps — cheap, plenty for a signature
      last = ts;
      clock += dt * speed;
      for (let i = 0; i < drops.length; i++)
        drops[i] = drops[i] > fallH + 20 ? -11 : drops[i] + fall;
      orbits.forEach((o) => (o.a += 0.02 * o.sp * speed * 2));
      if (chipBox) paint(chipBox, k, depth, false);
      if (reactorBox) paint(reactorBox, k, depth, false);
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  });
</script>

<div class="me" class:me--open={open}>
  <button
    type="button"
    class="me-trigger"
    class:me-trigger--hot={isHot}
    bind:this={triggerEl}
    aria-haspopup="dialog"
    aria-expanded={open}
    aria-label="Model and thinking effort"
    onclick={toggle}
    onkeydown={onKey}
  >
    <canvas class="me-bg" bind:this={chipCanvas} aria-hidden="true"></canvas>
    <span class="me-scrim" aria-hidden="true"></span>
    <span class="me-meta">
      <span class="me-model">{modelLabel}</span>
      <span class="me-eff">{effortLabel}</span>
    </span>
    <svg class="me-cay" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><path d="m6 9 6 6 6-6" /></svg>
  </button>

  {#if open && coords}
    <div
      class="me-panel"
      bind:this={panelEl}
      role="dialog"
      aria-label="Model and thinking effort"
      style:left="{coords.left}px"
      style:top="{coords.top}px"
      style:visibility={measured ? 'visible' : 'hidden'}
      transition:fade={{ duration: 120 }}
    >
      <div class="me-reactor">
        <canvas bind:this={reactorCanvas}></canvas>
        <span class="me-r-name">{modelLabel}</span>
        <span class="me-r-vibe"><b>{effortLabel}</b> · {vibe()}</span>
      </div>

      <div class="me-models">
        {#each modelOptions as m (m.value)}
          <button
            type="button"
            class="me-mp"
            class:me-mp--on={m.value === model}
            onclick={(e) => {
              e.stopPropagation();
              if (m.value !== model) onModelChange(m.value);
            }}
          >
            <span class="me-sig" aria-hidden="true"></span>{m.label}
          </button>
        {/each}
      </div>

      <div class="me-effort">
        <div class="me-ends"><span>Faster</span><span>Deeper</span></div>
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="me-track"
          class:me-track--hot={isHot}
          bind:this={trackEl}
          onpointerdown={onTrackPointerDown}
          onpointermove={onTrackPointerMove}
          onpointerup={onTrackPointerUp}
          role="slider"
          tabindex="0"
          aria-valuemin={0}
          aria-valuemax={efTiers.length - 1}
          aria-valuenow={ei}
          aria-valuetext={effortLabel}
          onkeydown={onKey}
        >
          <div class="me-fill" style:width="{fillPct}%"></div>
          <div class="me-stops">
            {#each efTiers as t, i (t.value)}
              <button
                type="button"
                class="me-stop"
                class:me-stop--p={i <= ei}
                class:me-stop--a={i === ei}
                title={t.label}
                aria-label={t.label}
                onclick={(e) => {
                  e.stopPropagation();
                  setEffortIdx(i);
                }}
              ></button>
            {/each}
          </div>
          <div class="me-knob" style:left="{fillPct}%" class:me-knob--hot={isHot}></div>
        </div>
      </div>
    </div>
  {/if}
</div>

<style>
  .me {
    position: relative;
    display: inline-flex;
    min-width: 0;
  }

  .me-trigger {
    position: relative;
    overflow: hidden;
    display: inline-flex;
    align-items: center;
    gap: 8px;
    height: 28px;
    padding: 0 9px;
    background: var(--bg-0);
    border: 1px solid var(--border-neutral);
    border-radius: 8px;
    color: var(--text-1);
    font-family: inherit;
    cursor: pointer;
    transition: border-color 120ms, box-shadow 280ms, color 120ms;
  }
  .me-trigger:hover {
    border-color: var(--border-neutral-hi);
    color: var(--text-0);
  }
  .me-trigger:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }
  .me--open .me-trigger {
    border-color: var(--border-hi);
    color: var(--text-0);
  }
  .me-trigger--hot {
    border-color: color-mix(in srgb, var(--accent-bright) 48%, var(--border-neutral));
    box-shadow: 0 0 16px -4px var(--accent-bright);
  }

  /* Live signature fills the whole chip; scrim keeps text legible. */
  .me-bg {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    z-index: 0;
  }
  .me-scrim {
    position: absolute;
    inset: 0;
    z-index: 1;
    pointer-events: none;
    background: radial-gradient(
      120% 140% at 50% 50%,
      color-mix(in srgb, var(--bg-0) 20%, transparent),
      color-mix(in srgb, var(--bg-0) 55%, transparent)
    );
  }
  .me-meta {
    position: relative;
    z-index: 2;
    flex: 0 0 auto;
    display: inline-flex;
    align-items: baseline;
    gap: 0;
  }
  .me-model {
    font-size: 12px;
    font-weight: 600;
    letter-spacing: -0.01em;
    white-space: nowrap;
    text-shadow: 0 1px 6px var(--bg-0);
  }
  .me-eff {
    display: inline-flex;
    align-items: baseline;
    font-size: 11px;
    color: var(--text-2);
    white-space: nowrap;
    font-variant-numeric: tabular-nums;
    text-shadow: 0 1px 6px var(--bg-0);
  }
  .me-eff::before {
    content: "·";
    margin: 0 5px;
    color: var(--text-mute);
  }
  .me-trigger--hot .me-eff {
    color: var(--accent-bright);
  }
  .me-cay {
    position: relative;
    z-index: 2;
    width: 11px;
    height: 11px;
    margin-left: 1px;
    stroke: var(--text-2);
    transition: transform 180ms;
    flex-shrink: 0;
  }
  .me--open .me-cay {
    transform: rotate(180deg);
  }

  .me-panel {
    position: fixed;
    z-index: 220;
    width: 300px;
    background: color-mix(in srgb, var(--bg-2) 97%, transparent);
    backdrop-filter: blur(16px);
    -webkit-backdrop-filter: blur(16px);
    border: 1px solid var(--border-hi);
    border-radius: 14px;
    box-shadow: var(--shadow-2), inset 0 1px 0 rgba(255, 255, 255, 0.03);
    overflow: hidden;
  }

  .me-reactor {
    position: relative;
    height: 78px;
    background: var(--bg-0);
    border-bottom: 1px solid var(--border-neutral);
  }
  .me-reactor canvas {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
  }
  .me-r-name {
    position: absolute;
    left: 12px;
    top: 10px;
    z-index: 2;
    font-size: 11px;
    font-weight: 600;
    color: var(--text-1);
  }
  .me-r-vibe {
    position: absolute;
    right: 12px;
    bottom: 9px;
    z-index: 2;
    font-size: 10px;
    color: var(--text-2);
    letter-spacing: 0.04em;
  }
  .me-r-vibe b {
    color: var(--accent-bright);
    font-weight: 600;
  }

  .me-models {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    padding: 12px 13px;
  }
  .me-mp {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 5px 10px;
    border-radius: 8px;
    cursor: pointer;
    font-size: 12px;
    font-family: inherit;
    color: var(--text-2);
    background: var(--bg-1);
    border: 1px solid var(--border-neutral);
    transition: color 140ms, background 140ms, border-color 140ms;
  }
  .me-mp:hover {
    color: var(--text-0);
    border-color: var(--border-neutral-hi);
  }
  .me-mp--on {
    color: var(--accent-bright);
    background: color-mix(in srgb, var(--accent) 14%, transparent);
    border-color: color-mix(in srgb, var(--accent-bright) 40%, var(--border-neutral));
  }
  .me-sig {
    width: 6px;
    height: 6px;
    border-radius: 1.5px;
    background: currentColor;
    opacity: 0.7;
  }

  .me-effort {
    padding: 4px 13px 14px;
  }
  .me-ends {
    display: flex;
    justify-content: space-between;
    font-size: 9.5px;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--text-mute);
    margin-bottom: 6px;
  }
  .me-track {
    position: relative;
    height: 24px;
    border-radius: 8px;
    cursor: pointer;
    touch-action: none;
    background:
      repeating-linear-gradient(90deg, transparent 0 6px, color-mix(in srgb, var(--text-mute) 15%, transparent) 6px 7px),
      var(--bg-0);
    border: 1px solid var(--border-neutral-hi, var(--border-hi));
  }
  .me-track--hot {
    border-color: color-mix(in srgb, var(--accent-bright) 45%, var(--border-neutral));
  }
  .me-fill {
    position: absolute;
    inset: 0 auto 0 0;
    border-radius: 8px 0 0 8px;
    pointer-events: none;
    background: linear-gradient(
      90deg,
      color-mix(in srgb, var(--accent) 28%, transparent),
      color-mix(in srgb, var(--accent-bright) 52%, transparent)
    );
    transition: width 250ms cubic-bezier(0.22, 1, 0.36, 1);
  }
  .me-stops {
    position: absolute;
    inset: 0;
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 0 9px;
    pointer-events: none;
  }
  .me-stop {
    width: 4px;
    height: 4px;
    border-radius: 50%;
    background: var(--text-mute);
    border: none;
    padding: 0;
    pointer-events: auto;
    cursor: pointer;
    transition: transform 140ms, background 140ms, box-shadow 140ms;
  }
  .me-stop--p {
    background: color-mix(in srgb, var(--accent-bright) 85%, white);
  }
  .me-stop--a {
    transform: scale(2.1);
    box-shadow: 0 0 6px -1px var(--accent-bright);
  }
  .me-knob {
    position: absolute;
    top: 50%;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    transform: translate(-50%, -50%);
    background: var(--text-0);
    box-shadow: 0 1px 4px rgba(0, 0, 0, 0.45), inset 0 0 0 2px var(--bg-2);
    transition: left 250ms cubic-bezier(0.22, 1, 0.36, 1), box-shadow 200ms;
    pointer-events: none;
  }
  .me-knob--hot {
    background: var(--accent-bright);
    box-shadow: 0 0 0 2px var(--bg-2), 0 0 12px -1px var(--accent-bright);
  }

  @media (prefers-reduced-motion: reduce) {
    .me-trigger,
    .me-fill,
    .me-knob,
    .me-cay {
      transition: none;
    }
  }
</style>
