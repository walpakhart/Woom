<script lang="ts">
  import CanvasSurface from './canvas/CanvasSurface.svelte';
  import InlineClaude from './editor/InlineClaude.svelte';
  import Splitter from '$lib/components/ui/Splitter.svelte';
  import { canvasState } from '$lib/state/canvas.svelte';
  import { layoutState, APP_INSTANCE_IDS } from '$lib/state/layout.svelte';
  import { sessionsState, updateSession } from '$lib/state/sessions.svelte';
  import type { Shape } from '$lib/state/canvas.svelte';

  interface Props {
    instanceId: string;
    onCardOpen?: (shape: Shape) => void;
    onOpenClaude?: () => void;
    onQuickSend?: (sessionId: string, text: string) => void;
    onOpenSession?: (sessionId: string, agentInstanceId: string) => void;
  }
  let p: Props = $props();

  let sideOpen = $state(true);

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

  const instanceLabel = $derived(
    layoutState.instances.canvas.find((i) => i.id === p.instanceId)?.name ?? 'Canvas'
  );

  const activeCanvasId = $derived(canvasState.byInstance[p.instanceId]?.activeId ?? null);

  function handleLinkSession(sessionId: string) {
    if (!activeCanvasId) return;
    const sess = sessionsState.list.find((s) => s.id === sessionId);
    if (!sess) return;
    const patch: Partial<typeof sess> = { linkedCanvasId: activeCanvasId };
    if (!sess.agentInstanceId && (sess.agentKind === 'claude' || sess.agentKind === 'cursor'))
      patch.agentInstanceId = APP_INSTANCE_IDS[sess.agentKind];
    updateSession(sessionId, patch);
  }

  function handleUnlinkSession(sessionId: string) {
    updateSession(sessionId, { linkedCanvasId: null });
  }
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
          <div class="sc-stats">
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
          </div>
          <InlineClaude
            instanceId={p.instanceId}
            linkKind="canvas"
            activeCanvasId={activeCanvasId}
            onClose={() => (sideOpen = false)}
            onOpenClaude={p.onOpenClaude ?? (() => {})}
            onQuickSend={p.onQuickSend ?? (() => {})}
            onOpenSession={p.onOpenSession ?? (() => {})}
            onLinkSession={handleLinkSession}
            onUnlinkSession={handleUnlinkSession}
          />
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
  /* InlineClaude fills the remaining side-panel height, overriding its
     own fixed 280px width so the splitter controls the column width. */
  .sc-shell :global(.ic) { width: 100%; flex: 1; border-left: none; min-height: 0; }

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

  .sc-side {
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

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

  .sc-stats {
    padding: 14px;
    flex-shrink: 0;
    border-bottom: 1px solid var(--border);
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
</style>
