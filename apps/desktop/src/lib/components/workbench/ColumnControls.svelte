<script lang="ts">
  import {
    layoutState,
    movePanelById,
    closePanelById,
    moveInstanceToWorkbench,
    archiveInstance
  } from '$lib/state/layout.svelte';
  import { notify } from '$lib/state/toaster.svelte';
  import { attachDragChip } from '$lib/dragImage';

  interface Props {
    instanceId: string;
    /** Used to keep singleton-kind move guards in sync with the layout
     *  store's logic — callers pass the panel kind so we can pre-disable
     *  workbenches that already host one of the same kind. */
    kind: 'github' | 'jira' | 'sentry' | 'claude' | 'cursor' | 'editor';
  }
  let { instanceId, kind }: Props = $props();

  const SINGLETON_KINDS = new Set(['github', 'jira', 'sentry']);

  let menuOpen = $state(false);
  /** When the trigger sits too close to the bottom of the viewport, the
   *  dropdown flips above the button so it doesn't get clipped. Computed
   *  on each open from the trigger's `getBoundingClientRect()`. */
  let flipUp = $state(false);
  let triggerEl = $state<HTMLButtonElement | null>(null);

  /** Approximate menu height for flip math. The actual menu varies with
   *  workbench count, but capping at ~280px (head + 5 rows + padding) is
   *  good enough for the flip threshold. */
  const ESTIMATED_MENU_HEIGHT = 280;

  function openMenu(force?: boolean) {
    const next = force !== undefined ? force : !menuOpen;
    if (next && triggerEl) {
      const rect = triggerEl.getBoundingClientRect();
      const spaceBelow = window.innerHeight - rect.bottom;
      // Flip if there's not enough room below AND the room above is bigger.
      flipUp = spaceBelow < ESTIMATED_MENU_HEIGHT && rect.top > spaceBelow;
    }
    menuOpen = next;
  }

  /** Close the popover when the user clicks anywhere outside. */
  function attachOutsideClose(node: HTMLDivElement) {
    function onClick(e: MouseEvent) {
      if (!menuOpen) return;
      if (!node.contains(e.target as Node)) menuOpen = false;
    }
    document.addEventListener('click', onClick, true);
    return () => document.removeEventListener('click', onClick, true);
  }

  function targetWorkbenches() {
    const sourceWb = layoutState.workbenches.find((w) =>
      w.instances.some((i) => i.id === instanceId)
    );
    return layoutState.workbenches
      .filter((w) => w.id !== sourceWb?.id)
      .map((w) => ({
        id: w.id,
        name: w.name,
        // Singleton kinds (github/jira) — flag if the target already has
        // one. We render that row disabled with a hint so the user
        // understands why it can't accept the move.
        blocked: SINGLETON_KINDS.has(kind) && w.instances.some((i) => i.kind === kind)
      }));
  }
  const candidates = $derived(targetWorkbenches());

  function moveTo(wbId: string, wbName: string) {
    const ok = moveInstanceToWorkbench(instanceId, wbId);
    menuOpen = false;
    if (ok) {
      notify({ kind: 'success', title: `Moved to ${wbName}`, ttlMs: 2000 });
    } else {
      notify({
        kind: 'warning',
        title: "Couldn't move column",
        body:
          SINGLETON_KINDS.has(kind)
            ? `${wbName} already has a ${kind} column — only one allowed per workbench.`
            : 'Source or target workbench not found.'
      });
    }
  }
</script>

<div class="wb-col-controls" {@attach attachOutsideClose}>
  <button
    class="wb-col-ctl"
    onclick={() => movePanelById(instanceId, -1)}
    aria-label="Move left"
    title="Move left"
  >
    <svg class="i i-sm" viewBox="0 0 24 24"><path d="M15 6l-6 6 6 6"/></svg>
  </button>
  <button
    class="wb-col-ctl"
    onclick={() => movePanelById(instanceId, 1)}
    aria-label="Move right"
    title="Move right"
  >
    <svg class="i i-sm" viewBox="0 0 24 24"><path d="M9 6l6 6-6 6"/></svg>
  </button>
  {#if layoutState.workbenches.length > 1}
    <div class="wb-col-move">
      <button
        bind:this={triggerEl}
        class="wb-col-ctl"
        class:active={menuOpen}
        onclick={(e) => { e.stopPropagation(); openMenu(); }}
        aria-label="Move to another workbench"
        aria-haspopup="menu"
        aria-expanded={menuOpen}
        title="Click to pick a workbench, or drag onto a tab"
        draggable="true"
        ondragstart={(e) => {
          if (!e.dataTransfer) return;
          // Custom mime read by the workbench-tab drop handler. The id is
          // all it needs — `moveInstanceToWorkbench` finds the source.
          e.dataTransfer.setData('application/x-forgehold-column', instanceId);
          e.dataTransfer.effectAllowed = 'move';
          attachDragChip(e, kind === 'editor' ? 'file' : kind, `Move column · ${kind}`);
          menuOpen = false;
        }}
      >
        <svg class="i i-sm" viewBox="0 0 24 24">
          <path d="M3 12h13M11 7l5 5-5 5M21 4v16"/>
        </svg>
      </button>
      {#if menuOpen}
        <div class="wb-col-move-menu" class:flip-up={flipUp} role="menu">
          <div class="wb-col-move-head">Move to workbench</div>
          {#each candidates as c (c.id)}
            <button
              class="wb-col-move-item"
              role="menuitem"
              disabled={c.blocked}
              title={c.blocked ? 'Already has a column of this kind' : `Move to ${c.name}`}
              onclick={() => moveTo(c.id, c.name)}
            >
              <span class="wb-col-move-name">{c.name}</span>
              {#if c.blocked}<span class="wb-col-move-hint">already has {kind}</span>{/if}
            </button>
          {/each}
        </div>
      {/if}
    </div>
  {/if}
  <button
    class="wb-col-ctl"
    onclick={() => archiveInstance(instanceId)}
    aria-label="Archive column"
    title="Archive — removes from workbench, keeps filters and chats. Restore from the kind pill."
  >
    <svg class="i i-sm" viewBox="0 0 24 24">
      <path d="M3 7h18M5 7v12a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V7M9 11h6"/>
    </svg>
  </button>
  <button
    class="wb-col-ctl wb-col-ctl--close"
    onclick={() => closePanelById(instanceId)}
    aria-label="Close column"
    title="Close — drops filters and orphans linked sessions"
  >
    <svg class="i i-sm" viewBox="0 0 24 24"><path d="M6 6l12 12M6 18L18 6"/></svg>
  </button>
</div>

<style>
  .wb-col-move { position: relative; display: inline-flex; }
  .wb-col-ctl.active { color: var(--accent-bright); background: var(--accent-soft); }
  .wb-col-move-menu {
    position: absolute; top: calc(100% + 6px); right: 0;
    min-width: 220px; max-width: 280px;
    background: var(--bg-2);
    border: 1px solid var(--border-hi);
    border-radius: 10px;
    box-shadow: 0 12px 32px rgba(0, 0, 0, 0.45);
    padding: 4px;
    display: flex; flex-direction: column; gap: 1px;
    z-index: 60;
    animation: fadeIn 120ms ease-out;
    transform-origin: top right;
  }
  /* Flip when the column is too close to the bottom of the viewport. The
     menu opens upward instead so it doesn't get clipped by the screen. */
  .wb-col-move-menu.flip-up {
    top: auto; bottom: calc(100% + 6px);
    transform-origin: bottom right;
    animation: fadeInUp 120ms ease-out;
  }
  .wb-col-move-head {
    font-size: 10px; font-weight: 600; letter-spacing: 0.05em;
    color: var(--text-mute); text-transform: uppercase;
    padding: 7px 10px 5px;
    border-bottom: 1px solid var(--border-neutral);
    margin-bottom: 3px;
  }
  .wb-col-move-item {
    display: flex; align-items: center; gap: 8px;
    padding: 7px 10px;
    border-radius: 6px;
    font-size: 12.5px; color: var(--text-1);
    text-align: left; cursor: pointer;
    background: transparent;
    border: none;
    transition: background 100ms;
  }
  .wb-col-move-item:hover:not(:disabled) { background: var(--bg-3); color: var(--text-0); }
  .wb-col-move-item:disabled { opacity: 0.45; cursor: not-allowed; }
  .wb-col-move-name { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .wb-col-move-hint {
    font-size: 10.5px; color: var(--text-mute); font-style: italic;
    flex-shrink: 0;
  }
  @keyframes fadeIn { from { opacity: 0; transform: translateY(-2px); } to { opacity: 1; transform: translateY(0); } }
  @keyframes fadeInUp { from { opacity: 0; transform: translateY(2px); } to { opacity: 1; transform: translateY(0); } }
</style>
