<script lang="ts">
  /* Foot-cluster rail button — Connections / Rules / Library / Settings.
     Same chassis as RailSourceButton but lighter contract (no badge,
     no drag-drop, no busy pulse). The optional `dot` slot draws the
     connections warning / retry indicator. */
  import type { Snippet } from 'svelte';

  interface Props {
    active: boolean;
    label: string;
    tooltip: string;
    view: string;
    /** When set, renders the connections status indicator at the
     *  top-right. `retrying` pulses; `disconnected` is static. */
    dot?: 'retrying' | 'disconnected' | null;
    onclick: () => void;
    icon: Snippet;
  }
  let p: Props = $props();
</script>

<button
  class="rail-btn"
  class:active={p.active}
  data-tooltip={p.tooltip}
  data-view={p.view}
  onclick={p.onclick}
  aria-label={p.label}
>
  {@render p.icon()}
  {#if p.dot === 'retrying'}
    <span class="rail-dot rail-dot--retrying" aria-label="retrying"></span>
  {:else if p.dot === 'disconnected'}
    <span class="rail-dot"></span>
  {/if}
</button>
