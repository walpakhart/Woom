<script lang="ts">
  /* CanvasApp — full-screen workspace для холста.
     Layout: [CanvasSurface (flex)] [props side (280, optional)]

     Right side panel — info: shape count + drop hint. Полные
     properties / minimap — следующий milestone. */

  import CanvasSurface from './canvas/CanvasSurface.svelte';
  import Splitter from '$lib/components/ui/Splitter.svelte';
  import { canvasState } from '$lib/state/canvas.svelte';
  import { layoutState } from '$lib/state/layout.svelte';
  import type { Shape } from '$lib/state/canvas.svelte';

  interface Props {
    instanceId: string;
    onCardOpen?: (shape: Shape) => void;
  }
  let p: Props = $props();

  let sideOpen = $state(true);

  /** Live shape/edge counts из активного канваса. */
  const stats = $derived.by(() => {
    const inst = canvasState.byInstance[p.instanceId];
    const canvasId = inst?.activeId;
    if (!canvasId) return { shapes: 0, edges: 0 };
    const c = canvasState.open[canvasId];
    return {
      shapes: c?.shapes.length ?? 0,
      edges: c?.edges.length ?? 0
    };
  });

  /** Curated mark for the open canvas instance — shown as the side
   *  pane's editorial title so users always see which Rothko /
   *  Hokusai canvas they're inside. */
  const instanceLabel = $derived(
    layoutState.instances.canvas.find((i) => i.id === p.instanceId)?.name ?? 'Canvas'
  );
</script>

<section
  class="app-shell sc-shell"
  style="--app-tone: var(--src-canvas); --app-glow: rgba(125,201,176,0.40);"
>
  {#if sideOpen}
    <Splitter
      direction="horizontal"
      fixedSide="end"
      persistKey="canvas-side"
      initial={300}
      min={240}
      max={520}
    >
      {#snippet start()}
        <section class="app-pane sc-canvas">
          <CanvasSurface instanceId={p.instanceId} onCardOpen={p.onCardOpen} />
        </section>
      {/snippet}
      {#snippet end()}
        <aside class="app-pane sc-side">
          <header class="app-pane-head">
            <span class="app-pane-head-h">{instanceLabel}</span>
            <span class="sc-kind-tag mono">Canvas</span>
            <button class="app-iconbtn" title="Hide" onclick={() => (sideOpen = false)}>
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7"><path d="M18 6 6 18M6 6l12 12"/></svg>
            </button>
          </header>
          <div class="sc-side-body">
        <div class="sc-stat-row">
          <div class="sc-stat">
            <div class="sc-stat-num mono">{stats.shapes}</div>
            <div class="sc-stat-lbl mono">shapes</div>
          </div>
          <div class="sc-stat">
            <div class="sc-stat-num mono">{stats.edges}</div>
            <div class="sc-stat-lbl mono">edges</div>
          </div>
        </div>

        <div class="sc-section">
          <div class="sc-group-label mono">Agents · canvas tools</div>
          <p class="sc-empty-p">
            Any Claude / Cursor session can drive this canvas via the
            <span class="mono">canvas_*</span> MCP tools — drop a card or
            shape from the agent's chat to spawn it here.
          </p>
        </div>
          </div>
        </aside>
      {/snippet}
    </Splitter>
  {:else}
    <section class="app-pane sc-canvas sc-canvas--full">
      <CanvasSurface instanceId={p.instanceId} onCardOpen={p.onCardOpen} />
      <button class="sc-show-side" title="Show canvas info" onclick={() => (sideOpen = true)}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7"><path d="M14 6l-6 6 6 6"/></svg>
      </button>
    </section>
  {/if}
</section>

<style>
  .sc-shell { display: block; padding: var(--app-pad, 14px); }
  .sc-shell :global(.s-start),
  .sc-shell :global(.s-end) {
    height: 100%;
    display: flex;
    min-width: 0;
  }
  .sc-shell :global(.s-start) > :global(*),
  .sc-shell :global(.s-end) > :global(*) {
    flex: 1 1 auto;
    width: 100%;
    min-width: 0;
  }

  .sc-canvas {
    display: flex;
    overflow: hidden;
    background: var(--bg-0);
    position: relative;
  }
  .sc-canvas--full {
    height: 100%;
  }
  .sc-canvas :global(.canvas-surface) {
    background: var(--bg-0) !important;
  }
  .sc-show-side {
    position: absolute;
    top: 14px; right: 14px;
    width: 26px; height: 26px;
    display: grid; place-items: center;
    border-radius: 6px;
    background: var(--bg-2);
    border: 1px solid var(--border);
    color: var(--text-2);
    cursor: pointer;
  }
  .sc-show-side:hover { color: var(--text-0); border-color: var(--border-hi); }
  .sc-show-side svg { width: 13px; height: 13px; }

  /* Small mono tag next to the editorial instance name so users still
     see "Canvas" as a kind label even when the head shows a curated
     mark like "Rothko" or "Hokusai". */
  .sc-kind-tag {
    font-size: 9.5px; font-weight: 700;
    letter-spacing: 0.10em;
    text-transform: uppercase;
    padding: 2px 7px;
    border-radius: 4px;
    background: color-mix(in srgb, var(--src-canvas) 12%, var(--bg-3));
    color: var(--src-canvas);
    border: 1px solid color-mix(in srgb, var(--src-canvas) 22%, transparent);
  }

  .sc-side-body {
    overflow-y: auto;
    padding: 14px;
    display: flex; flex-direction: column; gap: 14px;
  }

  .sc-stat-row { display: grid; grid-template-columns: 1fr 1fr; gap: 10px; }
  .sc-stat {
    padding: 12px 14px;
    background: linear-gradient(180deg,
      color-mix(in srgb, var(--src-canvas) 8%, transparent), transparent);
    border: 1px solid color-mix(in srgb, var(--src-canvas) 22%, transparent);
    border-radius: 10px;
    text-align: center;
  }
  .sc-stat-num {
    font-size: 22px; font-weight: 600;
    color: var(--src-canvas);
    line-height: 1;
  }
  .sc-stat-lbl {
    font-size: 9.5px; color: var(--text-mute);
    margin-top: 4px; text-transform: uppercase; letter-spacing: 0.08em;
  }

  .sc-section { display: flex; flex-direction: column; gap: 6px; }
  .sc-group-label {
    font-size: 9.5px; font-weight: 700;
    letter-spacing: 0.10em;
    text-transform: uppercase;
    color: var(--text-mute);
  }
  .sc-empty-p {
    font-size: 12px; color: var(--text-2);
    line-height: 1.55; margin: 0;
  }
  .sc-empty-p .mono {
    font-family: 'JetBrains Mono', monospace; font-size: 11px;
    padding: 1px 5px; background: var(--bg-2); border: 1px solid var(--border);
    border-radius: 4px; color: var(--src-canvas);
  }
</style>
