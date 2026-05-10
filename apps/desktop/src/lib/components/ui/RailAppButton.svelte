<script lang="ts">
  /* RailAppButton — a rail icon for a multi-instance app (Editor /
     Canvas / Terminal). Single click = switch view to the kind's
     active instance. Long-press (or right-click) opens a popover
     showing every open instance + an "Add" button + per-instance ×
     to remove non-primary entries.

     Why this lives in its own file rather than inline in Rail.svelte:
     the long-press timer + outside-click handling + per-instance
     popover would triple the size of Rail.svelte. Keeping it
     here lets Rail.svelte stay a flat list of icons. */

  import {
    layoutState,
    addInstance,
    removeInstance,
    setActiveInstance,
    type AppKind
  } from '$lib/state/layout.svelte';
  import type { Snippet } from 'svelte';

  interface Props {
    kind: AppKind;
    /** Pretty label used in the popover title and tooltip. */
    label: string;
    /** Tooltip + cmd-N hint, e.g. "Editor · ⌘6". */
    tooltip: string;
    /** True when this kind is the currently active view in +page.svelte.
     *  Used to light the rail-btn with the brand glow + accent dot. */
    active: boolean;
    /** rail-btn brand tone CSS — same vars the existing buttons use. */
    tone: string;
    glow: string;
    /** Inline `<svg>` icon snippet (so each kind keeps its own glyph). */
    icon: Snippet;
    /** Click handler — switches the top-level view to this kind. The
     *  parent +page.svelte already routes to layoutState.activeInstance[kind]. */
    onActivate: () => void;
  }
  let p: Props = $props();

  const instances = $derived(layoutState.instances[p.kind] ?? []);
  const activeId = $derived(layoutState.activeInstance[p.kind]);

  let menuOpen = $state(false);
  let pressTimer: ReturnType<typeof setTimeout> | null = null;
  let dragHoverTimer: ReturnType<typeof setTimeout> | null = null;
  /** Refs used by the outside-click handler so we only suppress the
   *  close when the click landed on THIS instance's button or menu —
   *  not on another rail button that happens to share the same class. */
  let rootEl: HTMLDivElement | null = $state(null);
  /** Block the synthetic click that fires after a long-press release —
   *  otherwise opening the menu would also navigate to the kind. */
  let suppressClick = $state(false);

  function startPress(e: PointerEvent) {
    if (e.button !== 0) return;
    pressTimer = setTimeout(() => {
      menuOpen = true;
      suppressClick = true;
      pressTimer = null;
    }, 380);
  }
  function cancelPress() {
    if (pressTimer) {
      clearTimeout(pressTimer);
      pressTimer = null;
    }
  }

  /** Drag-hover expand. Hovering a payload over the rail button for
   *  ~450 ms opens the instance popover, so the user can pick which
   *  Editor / Canvas / Terminal instance to drop into. Without this
   *  the only way to target a non-default instance is to first
   *  switch to it manually. Cancelled on dragleave / drop. */
  function onDragEnterBtn(e: DragEvent) {
    if (!e.dataTransfer) return;
    if (dragHoverTimer) clearTimeout(dragHoverTimer);
    dragHoverTimer = setTimeout(() => {
      menuOpen = true;
      dragHoverTimer = null;
    }, 450);
  }
  function onDragLeaveBtn() {
    if (dragHoverTimer) {
      clearTimeout(dragHoverTimer);
      dragHoverTimer = null;
    }
  }
  function onClick() {
    if (suppressClick) {
      suppressClick = false;
      return;
    }
    p.onActivate();
  }
  function onContextMenu(e: MouseEvent) {
    /* Right-click is a faster alternative to long-press. */
    e.preventDefault();
    menuOpen = true;
  }
  function onWindowClick(e: MouseEvent) {
    if (!menuOpen) return;
    /* Close on any click that lands OUTSIDE this specific component's
       root. Earlier we exempted any `.rab-btn` (so clicking another
       app's rail button left this menu open) — that's wrong: clicking
       anywhere away from the active popover should dismiss it. The
       button's own onClick handler short-circuits via `suppressClick`
       when needed, so toggling on the same button still works. */
    const t = e.target as Node | null;
    if (rootEl && t && rootEl.contains(t)) return;
    menuOpen = false;
  }
  function onKey(e: KeyboardEvent) {
    if (e.key === 'Escape' && menuOpen) menuOpen = false;
  }

  function pickInstance(id: string) {
    setActiveInstance(p.kind, id);
    menuOpen = false;
    p.onActivate();
  }
  function spawnInstance() {
    const inst = addInstance(p.kind);
    if (inst) {
      menuOpen = false;
      p.onActivate();
    }
  }
  function discardInstance(id: string, name: string, e: MouseEvent) {
    e.stopPropagation();
    if (!confirm(`Close ${p.label} · ${name}? Any unsaved per-instance state is dropped.`)) return;
    removeInstance(p.kind, id);
  }
</script>

<svelte:window onclick={onWindowClick} onkeydown={onKey} />

<div class="rab" class:menu-open={menuOpen} bind:this={rootEl}>
  <button
    class="rail-btn rab-btn"
    class:active={p.active}
    style="--rail-tone: {p.tone}; --rail-glow: {p.glow};"
    data-tooltip={p.tooltip}
    aria-label={p.label}
    onpointerdown={startPress}
    onpointerup={cancelPress}
    onpointerleave={cancelPress}
    oncontextmenu={onContextMenu}
    onclick={onClick}
    ondragenter={onDragEnterBtn}
    ondragleave={onDragLeaveBtn}
  >
    {@render p.icon()}
    <!-- The instance count pill used to live here, but the number was
         visually noisy on the rail and the count is already discoverable
         via long-press on the icon (which opens the instance popover).
         Keep the popover dot indicator below as the sole "this kind has
         extras" affordance. -->
  </button>

  {#if menuOpen}
    <div class="rab-menu" role="menu" aria-label="{p.label} instances">
      <div class="rab-menu-head">
        <span class="rab-menu-h">{p.label}</span>
        <span class="rab-menu-sub mono">{instances.length} open</span>
      </div>
      <div class="rab-menu-list">
        {#each instances as inst (inst.id)}
          {@const isActive = inst.id === activeId}
          <div class="rab-menu-row" class:active={isActive}>
            <button
              type="button"
              class="rab-menu-pick"
              onclick={() => pickInstance(inst.id)}
              title={inst.primary
                ? `${p.label} (default)`
                : `${p.label} · ${inst.name}`}
            >
              <span class="rab-menu-dot" style="--rab-tone: {p.tone};"></span>
              <span class="rab-menu-name">{inst.name}</span>
              {#if inst.primary}<span class="rab-menu-tag mono">primary</span>{/if}
            </button>
            {#if !inst.primary}
              <button
                type="button"
                class="rab-menu-x"
                onclick={(e) => discardInstance(inst.id, inst.name, e)}
                title="Close this {p.label} instance"
                aria-label="Close instance"
              >
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"><path d="M6 6l12 12M6 18 18 6"/></svg>
              </button>
            {/if}
          </div>
        {/each}
      </div>
      <button class="rab-menu-add" type="button" onclick={spawnInstance}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"><path d="M12 5v14M5 12h14"/></svg>
        New {p.label.toLowerCase()}
      </button>
    </div>
  {/if}
</div>

<style>
  .rab { position: relative; display: inline-flex; }

  /* The button itself reuses the chassis `.rail-btn` class so it
     inherits hover/active/glow from app.css. We just add the
     stack-indicator on multi-instance kinds. */
  .rab-btn { position: relative; }


  /* Popover anchored to the right of the rail button. The rail is
     dark, so the popover lives on a slightly lighter glass surface
     to read clearly without competing with content panes. */
  .rab-menu {
    position: absolute;
    left: calc(100% + 8px);
    top: -4px;
    min-width: 220px; max-width: 280px;
    background: rgba(20, 24, 26, 0.96);
    border: 1px solid var(--border-hi);
    border-radius: 11px;
    box-shadow: var(--shadow-3);
    backdrop-filter: blur(14px);
    -webkit-backdrop-filter: blur(14px);
    z-index: 200;
    padding: 6px;
    animation: rab-pop 140ms ease-out;
  }
  @keyframes rab-pop {
    from { opacity: 0; transform: translateX(-4px) scale(0.96); }
    to   { opacity: 1; transform: translateX(0)    scale(1); }
  }

  .rab-menu-head {
    display: flex; align-items: baseline; gap: 8px;
    padding: 8px 10px 6px;
    border-bottom: 1px solid var(--border);
    margin-bottom: 4px;
  }
  .rab-menu-h {
    flex: 1;
    font-family: 'Geist', 'Inter', -apple-system, system-ui, sans-serif;
    font-size: 16px; font-weight: 600;
    color: var(--text-0);
    letter-spacing: -0.01em;
  }
  .rab-menu-sub {
    font-size: 9.5px;
    color: var(--text-mute);
    letter-spacing: 0.06em;
  }

  .rab-menu-list { display: flex; flex-direction: column; gap: 1px; }
  .rab-menu-row {
    display: flex; align-items: center;
    border-radius: 7px;
    transition: background 100ms;
  }
  .rab-menu-row:hover { background: var(--bg-2); }
  .rab-menu-row.active { background: var(--bg-2); }

  .rab-menu-pick {
    flex: 1;
    display: inline-flex; align-items: center; gap: 9px;
    padding: 7px 10px;
    text-align: left;
    background: transparent;
    border: 0;
    font-size: 12.5px;
    color: var(--text-1);
    cursor: pointer;
  }
  .rab-menu-row.active .rab-menu-pick { color: var(--text-0); }
  .rab-menu-dot {
    width: 7px; height: 7px;
    border-radius: 50%;
    background: var(--rab-tone, var(--accent));
    box-shadow: 0 0 6px color-mix(in srgb, var(--rab-tone, var(--accent)) 50%, transparent);
    flex-shrink: 0;
  }
  .rab-menu-name {
    flex: 1; min-width: 0;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .rab-menu-tag {
    font-size: 9px; font-weight: 600;
    letter-spacing: 0.08em; text-transform: uppercase;
    padding: 1px 5px; border-radius: 3px;
    background: var(--bg-3); color: var(--text-mute);
    border: 1px solid var(--border);
  }

  /* Per-row × button — only renders for non-primary entries. Sits
     muted next to the row, flares red on hover. */
  .rab-menu-x {
    flex-shrink: 0;
    width: 22px; height: 22px;
    display: grid; place-items: center;
    margin-right: 4px;
    border-radius: 5px;
    background: transparent;
    border: 0;
    color: var(--text-mute);
    cursor: pointer;
    opacity: 0;
    transition: opacity 100ms, color 100ms, background 100ms;
  }
  .rab-menu-row:hover .rab-menu-x { opacity: 0.85; }
  .rab-menu-x:hover {
    opacity: 1;
    color: var(--error);
    background: rgba(232, 130, 100, 0.10);
  }
  .rab-menu-x svg { width: 11px; height: 11px; }

  .rab-menu-add {
    width: 100%;
    display: inline-flex; align-items: center; gap: 8px; justify-content: center;
    margin-top: 6px;
    padding: 8px 10px;
    border: 1px dashed var(--border-neutral-hi);
    border-radius: 8px;
    background: transparent;
    color: var(--text-2);
    font-size: 12.5px; font-weight: 500;
    cursor: pointer;
    transition: all 140ms;
  }
  .rab-menu-add svg { width: 13px; height: 13px; }
  .rab-menu-add:hover {
    color: var(--accent-bright);
    border-color: var(--border-accent);
    background: var(--accent-soft);
  }
</style>
