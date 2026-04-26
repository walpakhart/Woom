<script lang="ts" generics="T">
  // Custom dropdown that replaces native `<select>` so popups match the
  // Forgehold dark palette. Keyboard: ArrowUp/Down navigate, Enter selects,
  // Escape closes, letter keys type-to-search within visible labels. Opens
  // below by default and flips above (or right-aligns) when it would spill
  // out of the viewport.
  //
  // Sized to auto-fit the trigger by default. The list is portal'd via
  // absolute positioning inside the component itself (not a true portal —
  // keeps things simple and avoids z-index wrestling with modals).
  import type { Snippet } from 'svelte';
  import { slide, fade } from 'svelte/transition';
  import { cubicOut } from 'svelte/easing';

  export interface DropdownOption<V = unknown> {
    value: V;
    label: string;
    /** Leading color dot. Hex or CSS color. Optional. */
    color?: string;
    /** Subtext below the label. Optional. */
    hint?: string;
    /** Disables clicking this option. */
    disabled?: boolean;
  }

  interface Props<V> {
    value: V;
    options: DropdownOption<V>[];
    onChange: (value: V) => void;
    placeholder?: string;
    disabled?: boolean;
    /** CSS width override. Default: auto-size based on trigger content. */
    width?: string;
    /** Small visual variant — `chip` is rounded-rect pill, `ghost` is
     *  transparent (for inline use inside other chips). */
    variant?: 'chip' | 'ghost';
    /** Optional leading icon snippet, rendered left of the label. */
    icon?: Snippet;
    /** Called when the panel opens. Use this to lazy-load options. */
    onOpen?: () => void;
    /** ARIA label, for the trigger button. */
    ariaLabel?: string;
    /** Optional compact mode — smaller padding/font. */
    compact?: boolean;
    /** When provided, the dropdown enters multi-select mode:
     *   - trigger shows a comma-joined list of selected option labels
     *     (CSS truncates with ellipsis when it doesn't fit)
     *   - each option in the panel renders with a checkbox; selected
     *     ones are checked
     *   - clicking an option fires `onChange(option.value)` (toggle
     *     semantics expected on the caller) and the panel STAYS OPEN
     *     so you can pick several in a row
     *   - `value` is ignored for label resolution; the placeholder
     *     shows iff `selectedValues` is empty
     *  Use `null` / undefined to keep the legacy single-select shape. */
    selectedValues?: V[] | null;
  }

  let {
    value,
    options,
    onChange,
    placeholder = 'Select…',
    disabled = false,
    width,
    variant = 'chip',
    icon,
    onOpen,
    ariaLabel,
    compact = false,
    selectedValues = null
  }: Props<T> = $props();

  const isMulti = $derived(selectedValues !== null && selectedValues !== undefined);

  let open = $state(false);
  /** Index of the "focused" option for keyboard navigation. -1 = nothing. */
  let activeIndex = $state(-1);
  /** Right-align the panel if the trigger is too close to the viewport edge. */
  let alignRight = $state(false);
  /** Open upward if there's not enough space below. */
  let alignUp = $state(false);
  let triggerEl: HTMLButtonElement | null = $state(null);
  let panelEl: HTMLDivElement | null = $state(null);
  /** Accumulated type-to-search buffer, cleared 650 ms after the last key. */
  let typeahead = $state('');
  let typeaheadTimer: ReturnType<typeof setTimeout> | null = null;

  const selected = $derived(options.find((o) => o.value === value) ?? null);

  /** Multi-select trigger label — joined names of every option whose
   *  value is in `selectedValues`. CSS handles overflow ellipsis on
   *  the trigger, so a long join naturally truncates with `…`. */
  const multiLabel = $derived.by(() => {
    if (!selectedValues || selectedValues.length === 0) return '';
    const labels: string[] = [];
    for (const v of selectedValues) {
      const opt = options.find((o) => o.value === v);
      if (opt) labels.push(opt.label);
    }
    return labels.join(', ');
  });

  /** Quick lookup: is `v` in the multi-select selection? Used to
   *  render checkbox state on each option. Stable — recomputes only
   *  when `selectedValues` changes. */
  const selectedSet = $derived.by(() => {
    if (!selectedValues) return new Set<T>();
    return new Set(selectedValues);
  });

  function toggle() {
    if (disabled) return;
    if (open) close();
    else openPanel();
  }

  function openPanel() {
    open = true;
    // Reset keyboard cursor to the currently-selected option so ArrowDown
    // from the closed state lands somewhere useful.
    const idx = options.findIndex((o) => o.value === value);
    activeIndex = idx;
    requestAnimationFrame(computeAlignment);
    onOpen?.();
  }

  function close() {
    open = false;
    activeIndex = -1;
    typeahead = '';
    if (typeaheadTimer) {
      clearTimeout(typeaheadTimer);
      typeaheadTimer = null;
    }
  }

  function pick(opt: DropdownOption<T>) {
    if (opt.disabled) return;
    onChange(opt.value);
    if (isMulti) {
      // Stay open so the user can toggle several entries in a row.
      // Keep keyboard focus inside the panel — re-focus the trigger
      // would close the panel via blur.
      return;
    }
    close();
    triggerEl?.focus();
  }

  function computeAlignment() {
    if (!triggerEl || !panelEl) return;
    const tRect = triggerEl.getBoundingClientRect();
    const pRect = panelEl.getBoundingClientRect();
    const margin = 12;
    alignRight = tRect.left + pRect.width > window.innerWidth - margin;
    alignUp =
      tRect.bottom + pRect.height + margin > window.innerHeight &&
      tRect.top > pRect.height + margin;
  }

  function onDocClick(e: MouseEvent) {
    if (!open) return;
    const target = e.target as Node;
    if (triggerEl?.contains(target)) return;
    if (panelEl?.contains(target)) return;
    close();
  }

  function onKey(e: KeyboardEvent) {
    if (disabled) return;
    if (!open) {
      if (e.key === 'ArrowDown' || e.key === 'Enter' || e.key === ' ') {
        e.preventDefault();
        openPanel();
      }
      return;
    }
    if (e.key === 'Escape') {
      e.preventDefault();
      close();
      triggerEl?.focus();
      return;
    }
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      activeIndex = nextEnabled(activeIndex, 1);
      scrollActiveIntoView();
      return;
    }
    if (e.key === 'ArrowUp') {
      e.preventDefault();
      activeIndex = nextEnabled(activeIndex, -1);
      scrollActiveIntoView();
      return;
    }
    if (e.key === 'Home') {
      e.preventDefault();
      activeIndex = nextEnabled(-1, 1);
      scrollActiveIntoView();
      return;
    }
    if (e.key === 'End') {
      e.preventDefault();
      activeIndex = nextEnabled(options.length, -1);
      scrollActiveIntoView();
      return;
    }
    if (e.key === 'Enter') {
      e.preventDefault();
      if (activeIndex >= 0 && activeIndex < options.length) {
        pick(options[activeIndex]);
      }
      return;
    }
    // Single-character typeahead — accumulates until 650 ms idle, then jumps
    // to the first option whose label (case-insensitive) starts with the
    // buffer. Keeps the UX fast for users who know their target by name.
    if (e.key.length === 1 && !e.metaKey && !e.ctrlKey && !e.altKey) {
      typeahead += e.key.toLowerCase();
      if (typeaheadTimer) clearTimeout(typeaheadTimer);
      typeaheadTimer = setTimeout(() => {
        typeahead = '';
      }, 650);
      const idx = options.findIndex(
        (o) => !o.disabled && o.label.toLowerCase().startsWith(typeahead)
      );
      if (idx >= 0) {
        activeIndex = idx;
        scrollActiveIntoView();
      }
    }
  }

  function nextEnabled(from: number, delta: 1 | -1): number {
    if (!options.length) return -1;
    let i = from;
    for (let step = 0; step < options.length; step++) {
      i = (i + delta + options.length) % options.length;
      if (!options[i].disabled) return i;
    }
    return from;
  }

  function scrollActiveIntoView() {
    if (activeIndex < 0 || !panelEl) return;
    const el = panelEl.querySelector<HTMLElement>(
      `[data-idx="${activeIndex}"]`
    );
    el?.scrollIntoView({ block: 'nearest' });
  }

  $effect(() => {
    if (!open) return;
    document.addEventListener('mousedown', onDocClick);
    return () => document.removeEventListener('mousedown', onDocClick);
  });
</script>

<div
  class="dd"
  class:dd--ghost={variant === 'ghost'}
  class:dd--chip={variant === 'chip'}
  class:dd--compact={compact}
  class:dd--disabled={disabled}
  class:dd--open={open}
  style:width={width}
>
  <button
    type="button"
    class="dd-trigger"
    bind:this={triggerEl}
    aria-haspopup="listbox"
    aria-expanded={open}
    aria-label={ariaLabel}
    {disabled}
    onclick={toggle}
    onkeydown={onKey}
  >
    {#if icon}{@render icon()}{/if}
    {#if isMulti}
      <span class="dd-label" class:dd-label--placeholder={!multiLabel}>
        {multiLabel || placeholder}
      </span>
    {:else}
      {#if selected?.color}
        <span class="dd-dot" style="background: {selected.color};"></span>
      {/if}
      <span class="dd-label" class:dd-label--placeholder={!selected}>
        {selected?.label ?? placeholder}
      </span>
    {/if}
    <svg class="dd-caret" viewBox="0 0 24 24" aria-hidden="true">
      <path d="m6 9 6 6 6-6" />
    </svg>
  </button>

  {#if open}
    <div
      class="dd-panel"
      class:dd-panel--right={alignRight}
      class:dd-panel--up={alignUp}
      bind:this={panelEl}
      role="listbox"
      tabindex="-1"
      transition:slide={{ duration: 140, easing: cubicOut, axis: 'y' }}
    >
      <div class="dd-panel-inner" in:fade={{ duration: 120 }}>
        {#if options.length === 0}
          <div class="dd-empty">No options</div>
        {/if}
        {#each options as opt, i (i)}
          {@const isSelected = isMulti ? selectedSet.has(opt.value) : opt.value === value}
          <button
            type="button"
            class="dd-opt"
            class:dd-opt--active={i === activeIndex}
            class:dd-opt--selected={isSelected}
            class:dd-opt--disabled={opt.disabled}
            data-idx={i}
            role="option"
            aria-selected={isSelected}
            aria-disabled={opt.disabled}
            disabled={opt.disabled}
            onclick={() => pick(opt)}
            onmouseenter={() => { if (!opt.disabled) activeIndex = i; }}
          >
            {#if opt.color}
              <span class="dd-dot" style="background: {opt.color};"></span>
            {/if}
            <span class="dd-opt-body">
              <span class="dd-opt-label">{opt.label}</span>
              {#if opt.hint}<span class="dd-opt-hint">{opt.hint}</span>{/if}
            </span>
            {#if isSelected}
              <svg class="dd-check" viewBox="0 0 24 24" aria-hidden="true">
                <path d="M20 6 9 17l-5-5" />
              </svg>
            {/if}
          </button>
        {/each}
      </div>
    </div>
  {/if}
</div>

<style>
  .dd {
    position: relative;
    display: inline-flex;
    min-width: 0;
  }

  .dd-trigger {
    display: inline-flex; align-items: center; gap: 8px;
    width: 100%;
    padding: 6px 8px 6px 10px;
    background: var(--bg-1);
    border: 1px solid var(--border-neutral);
    border-radius: 6px;
    color: var(--text-1);
    font-size: 12px;
    font-family: inherit;
    line-height: 1.2;
    min-width: 0;
    cursor: pointer;
    transition: border-color 120ms, color 120ms, background 120ms;
  }
  .dd-trigger:hover:not(:disabled) {
    border-color: var(--border-neutral-hi);
    color: var(--text-0);
  }
  .dd-trigger:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }
  .dd--open .dd-trigger {
    border-color: var(--border-hi);
    color: var(--text-0);
  }
  .dd--disabled .dd-trigger,
  .dd-trigger:disabled {
    opacity: 0.55;
    cursor: not-allowed;
  }

  .dd--ghost .dd-trigger {
    background: transparent;
    border-color: transparent;
    padding: 4px 4px 4px 6px;
  }
  .dd--ghost .dd-trigger:hover:not(:disabled) {
    background: var(--bg-1);
  }

  .dd--compact .dd-trigger {
    padding: 4px 6px 4px 8px;
    font-size: 11px;
  }

  .dd-label {
    flex: 1; min-width: 0;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
    text-align: left;
  }
  .dd-label--placeholder { color: var(--text-2); }

  .dd-dot {
    width: 6px; height: 6px; border-radius: 50%;
    flex-shrink: 0;
    box-shadow: 0 0 0 1px rgba(0, 0, 0, 0.25) inset;
  }

  .dd-caret {
    width: 12px; height: 12px;
    stroke: currentColor;
    fill: none;
    stroke-width: 2;
    stroke-linecap: round;
    stroke-linejoin: round;
    color: var(--text-2);
    flex-shrink: 0;
    transition: transform 160ms;
  }
  .dd--open .dd-caret { transform: rotate(180deg); }

  .dd-panel {
    position: absolute;
    top: calc(100% + 4px);
    left: 0;
    min-width: 100%;
    max-width: 380px;
    z-index: 220;
    background: color-mix(in srgb, var(--bg-2) 97%, transparent);
    backdrop-filter: blur(16px);
    -webkit-backdrop-filter: blur(16px);
    border: 1px solid var(--border-hi);
    border-radius: 8px;
    box-shadow: 0 10px 28px rgba(0, 0, 0, 0.45), inset 0 1px 0 rgba(255, 255, 255, 0.03);
    overflow: hidden;
  }
  .dd-panel--right { left: auto; right: 0; }
  .dd-panel--up {
    top: auto;
    bottom: calc(100% + 4px);
  }
  .dd-panel-inner {
    max-height: 280px;
    overflow-y: auto;
    padding: 4px;
    display: flex; flex-direction: column; gap: 1px;
  }
  .dd-empty {
    padding: 14px 12px;
    font-size: 12px; color: var(--text-mute);
    text-align: center;
  }
  .dd-opt {
    display: flex; align-items: center; gap: 8px;
    width: 100%;
    padding: 7px 9px;
    border-radius: 5px;
    font-size: 12.5px;
    color: var(--text-1);
    text-align: left;
    background: transparent;
    border: none;
    cursor: pointer;
    transition: background 90ms, color 90ms;
  }
  .dd-opt:disabled, .dd-opt--disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }
  .dd-opt--active:not(:disabled) {
    background: var(--bg-3);
    color: var(--text-0);
  }
  .dd-opt--selected:not(:disabled) {
    color: var(--accent-bright);
    background: var(--accent-soft);
  }
  .dd-opt--selected.dd-opt--active:not(:disabled) {
    background: color-mix(in srgb, var(--accent-bright) 14%, var(--bg-3));
  }
  .dd-opt-body {
    flex: 1; min-width: 0;
    display: flex; flex-direction: column; gap: 1px;
  }
  .dd-opt-label {
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .dd-opt-hint {
    font-size: 10.5px;
    color: var(--text-mute);
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .dd-check {
    width: 12px; height: 12px;
    stroke: currentColor;
    fill: none;
    stroke-width: 2.2;
    stroke-linecap: round;
    stroke-linejoin: round;
    color: var(--accent-bright);
    flex-shrink: 0;
  }
</style>
