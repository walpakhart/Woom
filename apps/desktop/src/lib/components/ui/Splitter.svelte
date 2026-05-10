<script lang="ts">
  import type { Snippet } from 'svelte';
  import { onMount } from 'svelte';

  interface Props {
    /** `horizontal` = side-by-side (vertical divider), `vertical` = stacked (horizontal divider). */
    direction?: 'horizontal' | 'vertical';
    /** Which pane has the fixed/persisted dimension. The other pane
     *  flex-grows to fill what's left. Default `start` keeps the
     *  legacy "left sidebar fixed width" behaviour. Use `end` when
     *  the right-hand (or bottom) pane is the inspector and the
     *  left-hand (or top) pane is the main content. */
    fixedSide?: 'start' | 'end';
    /** Initial size in px of the *fixed* pane. */
    initial?: number;
    min?: number;
    max?: number;
    /** If set, the chosen size is persisted in localStorage under `woom:splitter:<persistKey>`. */
    persistKey?: string;
    start: Snippet;
    end: Snippet;
  }

  let {
    direction = 'horizontal',
    fixedSide = 'start',
    initial = 280,
    min = 120,
    max = 900,
    persistKey,
    start,
    end
  }: Props = $props();

  let size = $state<number>(0);
  let containerEl: HTMLDivElement;
  let dragging = $state(false);

  onMount(() => {
    size = initial;
    if (persistKey) {
      const saved = localStorage.getItem(`woom:splitter:${persistKey}`);
      if (saved) {
        const n = parseFloat(saved);
        if (!isNaN(n) && n >= min && n <= max) size = n;
      }
    }
  });

  function startDrag(e: PointerEvent) {
    if (e.button !== 0) return;
    dragging = true;
    const startPos = direction === 'horizontal' ? e.clientX : e.clientY;
    const startSize = size;
    const rect = containerEl.getBoundingClientRect();
    const containerSize = direction === 'horizontal' ? rect.width : rect.height;
    const minEnd = 120; // leave room for the end pane

    const onMove = (ev: PointerEvent) => {
      const cur = direction === 'horizontal' ? ev.clientX : ev.clientY;
      const rawDelta = cur - startPos;
      /* When the FIXED pane is on the END (right/bottom), dragging the
       *  divider toward it (i.e. positive delta in horizontal mode)
       *  SHRINKS that pane. Invert so the user always feels "drag the
       *  divider toward the pane to shrink it". */
      const delta = fixedSide === 'end' ? -rawDelta : rawDelta;
      let next = startSize + delta;
      next = Math.max(min, Math.min(max, next));
      /* Reserve room for the flex pane so the user can't collapse it
       *  to zero by overshooting the divider. */
      next = Math.min(next, containerSize - minEnd);
      size = next;
    };
    const onUp = () => {
      dragging = false;
      if (persistKey) localStorage.setItem(`woom:splitter:${persistKey}`, String(size));
      window.removeEventListener('pointermove', onMove);
      window.removeEventListener('pointerup', onUp);
    };
    window.addEventListener('pointermove', onMove);
    window.addEventListener('pointerup', onUp);
    e.preventDefault();
  }

  function onKeyDown(e: KeyboardEvent) {
    // Keyboard nudge: ←/→ for horizontal, ↑/↓ for vertical. 10px step, 40px with shift.
    const step = (e.shiftKey ? 40 : 10);
    let raw = 0;
    if (direction === 'horizontal') {
      if (e.key === 'ArrowLeft') raw = -step;
      else if (e.key === 'ArrowRight') raw = step;
    } else {
      if (e.key === 'ArrowUp') raw = -step;
      else if (e.key === 'ArrowDown') raw = step;
    }
    if (raw !== 0) {
      /* Same sign-flip the pointer drag uses — keyboard arrows are
       *  consistent with the visual divider direction. */
      const delta = fixedSide === 'end' ? -raw : raw;
      size = Math.max(min, Math.min(max, size + delta));
      if (persistKey) localStorage.setItem(`woom:splitter:${persistKey}`, String(size));
      e.preventDefault();
    }
  }
</script>

<div
  class="splitter"
  class:vertical={direction === 'vertical'}
  class:dragging
  class:fixed-end={fixedSide === 'end'}
  bind:this={containerEl}
>
  <div
    class="s-start"
    style={fixedSide === 'start'
      ? (direction === 'horizontal' ? `width: ${size}px` : `height: ${size}px`)
      : ''}
  >
    {@render start()}
  </div>
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div
    class="s-divider"
    class:vert={direction === 'vertical'}
    role="separator"
    aria-orientation={direction === 'horizontal' ? 'vertical' : 'horizontal'}
    aria-valuenow={size}
    aria-valuemin={min}
    aria-valuemax={max}
    tabindex="0"
    onpointerdown={startDrag}
    onkeydown={onKeyDown}
  ></div>
  <div
    class="s-end"
    style={fixedSide === 'end'
      ? (direction === 'horizontal' ? `width: ${size}px` : `height: ${size}px`)
      : ''}
  >
    {@render end()}
  </div>
</div>

<style>
  .splitter {
    display: flex; flex-direction: row;
    width: 100%; height: 100%;
    min-width: 0; min-height: 0;
  }
  .splitter.vertical { flex-direction: column; }
  .s-start { flex: 1; min-width: 0; min-height: 0; overflow: hidden; }
  .s-end { flex: 1; min-width: 0; min-height: 0; overflow: hidden; }
  /* Default mode: start is fixed (sidebar), end is flex. */
  .splitter:not(.fixed-end) > .s-start { flex: 0 0 auto; }
  /* Inverted mode: end is fixed (right inspector), start is flex. */
  .splitter.fixed-end > .s-end { flex: 0 0 auto; }
  .s-divider {
    position: relative;
    flex-shrink: 0;
    width: 10px;
    cursor: col-resize;
    user-select: none;
    background: transparent;
    z-index: 5;
    transition: background 100ms ease;
  }
  .s-divider.vert { width: 100%; height: 10px; cursor: row-resize; }
  /* Thin visible groove centered in a 10px hit-target. */
  .s-divider::before {
    content: '';
    position: absolute;
    inset: 0;
    margin: 0 4px;
    background: var(--border-neutral);
    border-radius: 1px;
    transition: background 100ms ease;
  }
  .s-divider.vert::before { margin: 4px 0; }
  .s-divider:hover::before,
  .s-divider:focus-visible::before,
  .splitter.dragging .s-divider::before { background: var(--accent); }
  .s-divider:focus-visible { outline: none; }
  .splitter.dragging { cursor: inherit; }
</style>
