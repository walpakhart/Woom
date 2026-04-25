<script lang="ts">
  import { externalId } from '$lib/data';
  import { inboxState } from '$lib/state/inbox.svelte';

  interface Props {
    open: boolean;
    /** Called when the user picks an item. Parent closes the palette, focuses
        the object in the workbench, and switches the active view as needed. */
    onSelect: (id: number) => void;
  }

  let {
    open = $bindable(),
    onSelect
  }: Props = $props();
</script>

{#if open}
  <div
    class="palette-backdrop"
    onclick={(e) => { if (e.target === e.currentTarget) open = false; }}
    onkeydown={(e) => { if (e.key === 'Escape') open = false; }}
    role="dialog"
    tabindex="-1"
  >
    <div class="palette">
      <!-- svelte-ignore a11y_autofocus -->
      <input class="palette-input" placeholder="Search inbox…" autofocus />
      {#if inboxState.items.length === 0}
        <div class="palette-empty">Nothing to search yet.</div>
      {:else}
        <div class="palette-section">
          <div class="palette-section-title">Objects</div>
          {#each inboxState.items.slice(0, 8) as obj (obj.id)}
            <button
              class="palette-item"
              class:highlight={obj.id === inboxState.focusItem?.id}
              onclick={() => onSelect(obj.id)}
            >
              <span class="source-mark" style="width:18px; height:18px; font-size:10px;">GH</span>
              <span class="mono" style="color: var(--text-2); font-size:12px;">{externalId(obj)}</span>
              <span>{obj.title}</span>
            </button>
          {/each}
        </div>
      {/if}
    </div>
  </div>
{/if}

<style>
  .palette-backdrop {
    position: fixed; inset: 0;
    background: rgba(10, 17, 30, 0.78);
    backdrop-filter: blur(20px);
    display: flex; align-items: flex-start; justify-content: center;
    padding-top: 15vh; z-index: 200;
    animation: fadeIn 180ms ease-out;
  }
  .palette {
    width: 640px; max-width: 90vw;
    background: rgba(15, 24, 40, 0.94);
    backdrop-filter: blur(24px);
    border: 1px solid var(--border-hi2); border-radius: 14px;
    overflow: hidden;
    box-shadow: 0 30px 80px rgba(0, 0, 0, 0.6), inset 0 1px 0 rgba(255, 255, 255, 0.04);
    animation: slideDown 220ms cubic-bezier(0.34, 1.56, 0.64, 1);
  }
  .palette-input {
    width: 100%; padding: 18px 22px;
    font-size: 15px; color: var(--text-0);
    border-bottom: 1px solid var(--border-neutral);
    background: transparent; border-left: none; border-right: none; border-top: none;
  }
  .palette-input:focus { outline: none; }
  .palette-input::placeholder { color: var(--text-2); }
  .palette-section { padding: 8px 10px; }
  .palette-section-title {
    padding: 6px 12px; font-size: 10.5px; font-weight: 600;
    color: var(--text-mute); text-transform: uppercase; letter-spacing: 0.08em;
  }
  .palette-empty { padding: 24px 22px; font-size: 13px; color: var(--text-2); text-align: center; }
  .palette-item {
    display: flex; align-items: center; gap: 12px;
    padding: 9px 12px; border-radius: 7px;
    width: 100%; text-align: left;
    font-size: 13px; color: var(--text-1); cursor: pointer;
    background: none; border: none;
  }
  .palette-item:hover, .palette-item.highlight { background: var(--bg-2); color: var(--text-0); }
  .palette-item.highlight { box-shadow: inset 0 0 0 1px var(--border-hi); }

  @keyframes fadeIn { from { opacity: 0; } to { opacity: 1; } }
  @keyframes slideDown {
    from { transform: translateY(-10px); opacity: 0; }
    to { transform: translateY(0); opacity: 1; }
  }
  .source-mark {
    display: inline-flex; align-items: center; justify-content: center;
    border-radius: 4px; font-weight: 700;
    background: rgba(139, 92, 246, 0.14); color: #b099f6; border: 1px solid rgba(139, 92, 246, 0.35);
  }
</style>
