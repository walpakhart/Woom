<script lang="ts">
  import type { Snippet } from 'svelte';
  import { onMount } from 'svelte';

  interface Props {
    /** `horizontal` = side-by-side (vertical divider), `vertical` = stacked (horizontal divider). */
    direction?: 'horizontal' | 'vertical';
    /** Initial size in px of the *start* pane. */
    initial?: number;
    min?: number;
    max?: number;
    /** If set, the chosen size is persisted in localStorage under `forgehold:splitter:<persistKey>`. */
    persistKey?: string;
    start: Snippet;
    end: Snippet;
  }

  let {
    direction = 'horizontal',
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
      const saved = localStorage.getItem(`forgehold:splitter:${persistKey}`);
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
      const delta = cur - startPos;
      let next = startSize + delta;
      next = Math.max(min, Math.min(max, next));
      next = Math.min(next, containerSize - minEnd);
      size = next;
    };
    const onUp = () => {
      dragging = false;
      if (persistKey) localStorage.setItem(`forgehold:splitter:${persistKey}`, String(size));
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
    let delta = 0;
    if (direction === 'horizontal') {
      if (e.key === 'ArrowLeft') delta = -step;
      else if (e.key === 'ArrowRight') delta = step;
    } else {
      if (e.key === 'ArrowUp') delta = -step;
      else if (e.key === 'ArrowDown') delta = step;
    }
    if (delta !== 0) {
      size = Math.max(min, Math.min(max, size + delta));
      if (persistKey) localStorage.setItem(`forgehold:splitter:${persistKey}`, String(size));
      e.preventDefault();
    }
  }
</script>

<div
  class="splitter"
  class:vertical={direction === 'vertical'}
  class:dragging
  bind:this={containerEl}
>
  <div
    class="s-start"
    style={direction === 'horizontal' ? `width: ${size}px` : `height: ${size}px`}
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
  <div class="s-end">
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
  .s-start { flex-shrink: 0; min-width: 0; min-height: 0; overflow: hidden; }
  .s-end { flex: 1; min-width: 0; min-height: 0; overflow: hidden; }
  .s-divider {
    position: relative;
    flex-shrink: 0;
    width: 5px;
    cursor: col-resize;
    user-select: none;
    background: transparent;
    z-index: 5;
    transition: background 100ms ease;
  }
  .s-divider.vert { width: 100%; height: 5px; cursor: row-resize; }
  .s-divider::before {
    content: '';
    position: absolute;
    inset: 0;
    margin: 0 2px;
    background: var(--border-neutral);
    transition: background 100ms ease;
  }
  .s-divider.vert::before { margin: 2px 0; }
  .s-divider:hover::before,
  .s-divider:focus-visible::before,
  .splitter.dragging .s-divider::before { background: var(--accent); }
  .s-divider:focus-visible { outline: none; }
  .splitter.dragging { cursor: inherit; }
</style>
