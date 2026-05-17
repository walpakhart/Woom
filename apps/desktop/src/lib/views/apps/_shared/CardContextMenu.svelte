<script lang="ts" module>
  /* Right-click context menu for inbox card rows (GitHub PR, Jira
   * ticket, Sentry issue). Positioned absolute at the user's click
   * coordinates, closes on Escape, on outside click/wheel/scroll, and
   * after any action runs. Modeled on FileTree's tree-ctx menu so the
   * keyboard / dismissal behaviour reads the same across the app.
   *
   * Each `items` entry runs its `onClick` then auto-closes. Pass
   * `danger: true` to flag destructive actions (resolves/closes etc.)
   * — they get a clay-red foreground until hovered.
   *
   * Caller owns the open/close state via `coords` + `onClose`. The
   * menu doesn't manage its own visibility because the inbox rows
   * need to keep the coords stable across re-renders without the
   * menu unmounting/remounting per keystroke. */
  export interface MenuItem {
    label: string;
    icon?: string; // SVG path 'd' attribute (24x24 viewBox)
    onClick: () => void | Promise<void>;
    danger?: boolean;
    /** Optional shortcut hint shown right-aligned (e.g. "⌘C"). Pure
     *  display — the parent owns actual keyboard wiring. */
    shortcut?: string;
  }
</script>

<script lang="ts">
  import { onDestroy } from 'svelte';

  interface Props {
    coords: { x: number; y: number } | null;
    items: MenuItem[];
    onClose: () => void;
  }
  let { coords, items, onClose }: Props = $props();

  let menuEl: HTMLDivElement | null = $state(null);

  /* Outside-click + Esc dismissal. Bound at the window level so even
     a click on an opaque solo header registers. We also listen for
     wheel/scroll because a menu that follows a scrolling viewport
     would drift away from its anchor point — easier to just close. */
  function onWindowDown(e: MouseEvent) {
    if (!menuEl) return;
    if (menuEl.contains(e.target as Node)) return;
    onClose();
  }
  function onWindowKey(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.preventDefault();
      onClose();
    }
  }
  function onScroll() {
    onClose();
  }

  $effect(() => {
    if (coords) {
      /* Use `capture: true` for mousedown so we win against any
         row-level click handler that would also fire on a click
         outside the menu but inside the same row. The menu should
         close FIRST, then the row click can proceed normally. */
      window.addEventListener('mousedown', onWindowDown, true);
      window.addEventListener('keydown', onWindowKey);
      window.addEventListener('wheel', onScroll, { passive: true });
      window.addEventListener('scroll', onScroll, { capture: true, passive: true });
      return () => {
        window.removeEventListener('mousedown', onWindowDown, true);
        window.removeEventListener('keydown', onWindowKey);
        window.removeEventListener('wheel', onScroll);
        window.removeEventListener('scroll', onScroll, { capture: true } as EventListenerOptions);
      };
    }
  });

  onDestroy(() => {
    window.removeEventListener('mousedown', onWindowDown, true);
    window.removeEventListener('keydown', onWindowKey);
    window.removeEventListener('wheel', onScroll);
    window.removeEventListener('scroll', onScroll, { capture: true } as EventListenerOptions);
  });

  async function runItem(it: MenuItem) {
    onClose();
    try {
      await it.onClick();
    } catch (e) {
      console.warn('CardContextMenu.runItem', e);
    }
  }

  /* Edge-aware positioning. If the menu would overflow the viewport's
     right edge we anchor to the right of the cursor; same for the
     bottom edge. Reads viewport on mount only — a menu that re-flowed
     on every keystroke would be jittery. 200px width / 220px height
     budget covers the worst-case items count we ship. */
  const style = $derived.by(() => {
    if (!coords) return '';
    const MENU_W = 220;
    const MENU_H = 220;
    const PAD = 8;
    const vw = typeof window !== 'undefined' ? window.innerWidth : 1024;
    const vh = typeof window !== 'undefined' ? window.innerHeight : 768;
    const left = coords.x + MENU_W + PAD > vw ? vw - MENU_W - PAD : coords.x;
    const top = coords.y + MENU_H + PAD > vh ? vh - MENU_H - PAD : coords.y;
    return `left: ${Math.max(PAD, left)}px; top: ${Math.max(PAD, top)}px;`;
  });
</script>

{#if coords}
  <div
    bind:this={menuEl}
    class="ctx-menu"
    style={style}
    role="menu"
    aria-label="Card actions"
  >
    {#each items as it, i (i)}
      <button
        class="ctx-item"
        class:ctx-item--danger={it.danger}
        onclick={() => void runItem(it)}
        role="menuitem"
      >
        {#if it.icon}
          <svg
            class="ctx-icon"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="1.7"
            stroke-linecap="round"
            stroke-linejoin="round"
            aria-hidden="true"
          >
            <path d={it.icon} />
          </svg>
        {/if}
        <span class="ctx-label">{it.label}</span>
        {#if it.shortcut}
          <span class="ctx-shortcut mono">{it.shortcut}</span>
        {/if}
      </button>
    {/each}
  </div>
{/if}

<style>
  .ctx-menu {
    position: fixed;
    z-index: 9999;
    /* Explicit width range. Without these the menu's natural shrink-
       to-fit defers to the child flex items' `flex: 1` label rule and
       the menu would stretch horizontally past its content (observed
       reaching ~1000px against a narrow inbox column). Lock the menu
       to a 220–260px band so it always reads as a popover, never a
       wall. */
    width: max-content;
    min-width: 220px;
    max-width: 260px;
    padding: 4px;
    background: var(--bg-3);
    border: 1px solid var(--border-neutral-hi, var(--border));
    border-radius: 8px;
    box-shadow: var(--shadow-2, 0 12px 32px rgba(0, 0, 0, 0.32));
    /* Subtle entrance — matches the rail tooltip animation so menus
       across the app feel consistent. Reduced-motion users get the
       static end state immediately via prefers-reduced-motion below. */
    animation: ctx-menu-in 120ms var(--ease-out, ease-out);
  }
  @keyframes ctx-menu-in {
    from { opacity: 0; transform: translateY(-2px); }
    to   { opacity: 1; transform: translateY(0); }
  }
  @media (prefers-reduced-motion: reduce) {
    .ctx-menu { animation: none; }
  }
  .ctx-item {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 6px 10px;
    background: transparent;
    border: 0;
    border-radius: 5px;
    color: var(--text-0);
    font-size: 12.5px;
    line-height: 1.3;
    text-align: left;
    cursor: pointer;
    transition: background 120ms;
  }
  .ctx-item:hover { background: var(--bg-2); }
  .ctx-item:focus-visible {
    outline: 1px solid var(--accent, var(--text-mute));
    outline-offset: -1px;
  }
  .ctx-icon {
    flex-shrink: 0;
    width: 14px;
    height: 14px;
    color: var(--text-mute);
  }
  .ctx-label { flex: 1; min-width: 0; }
  .ctx-shortcut {
    color: var(--text-mute);
    font-size: 11px;
  }
  .ctx-item--danger { color: var(--error, #e88264); }
  .ctx-item--danger:hover {
    background: color-mix(in srgb, var(--error, #e88264) 14%, transparent);
  }
  .ctx-item--danger .ctx-icon { color: var(--error, #e88264); }
</style>
