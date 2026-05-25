<script lang="ts">
  /* Floating tooltip — JS-positioned, fixed sibling of `.rail-scroll`.
     The CSS pseudo-element approach lived inside the scrollable middle
     column, and CSS can't do `overflow-y: scroll` + `overflow-x: visible`
     (both axes are forced clipped as soon as one is). That clipped every
     tooltip on a rail-btn inside `.rail-scroll`. A `position: fixed`
     element escapes ALL ancestor clip contexts — cleanest way to keep
     hover affordance without breaking scroll. */
  let text = $state('');
  let x = $state(0);
  let y = $state(0);
  let visible = $state(false);

  export function show(target: HTMLElement): void {
    const t = target.getAttribute('data-tooltip');
    if (!t) return;
    const rect = target.getBoundingClientRect();
    text = t;
    /* 8px to right of button, vertically centred. The `.rail-tooltip`
       uses `transform: translateY(-50%)` so we anchor on midpoint. */
    x = rect.right + 8;
    y = rect.top + rect.height / 2;
    visible = true;
  }

  export function hide(): void {
    visible = false;
  }
</script>

{#if visible}
  <!-- MUST render OUTSIDE `<aside class="rail">` (parent does that) so
       it isn't trapped by the rail's containing block — the rail has
       `backdrop-filter: blur(12px)` which (per CSS spec) creates a new
       containing block for fixed-positioned descendants. Placed at
       component root → containing block is the viewport. -->
  <div class="rail-tooltip" role="tooltip" style="left: {x}px; top: {y}px;">
    {text}
  </div>
{/if}

<style>
  .rail-tooltip {
    position: fixed;
    padding: 4px 10px;
    background: var(--bg-3);
    border: 1px solid var(--border-neutral-hi);
    border-radius: 6px;
    font-size: 11.5px;
    color: var(--text-0);
    white-space: nowrap;
    pointer-events: none;
    z-index: 9999;
    box-shadow: var(--shadow-1);
    transform: translateY(-50%);
    animation: rail-tooltip-in 120ms var(--ease-out, ease-out);
  }
  @keyframes rail-tooltip-in {
    from { opacity: 0; transform: translate(-2px, -50%); }
    to   { opacity: 1; transform: translate(0, -50%); }
  }
  @media (prefers-reduced-motion: reduce) {
    .rail-tooltip { animation: none; }
  }
</style>
