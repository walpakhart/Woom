<script lang="ts">
  import CanvasSurface from './canvas/CanvasSurface.svelte';
  import InlineClaude from './editor/InlineClaude.svelte';
  import Splitter from '$lib/components/ui/Splitter.svelte';
  import SidePaneRail from '$lib/components/ui/SidePaneRail.svelte';
  import { canvasState } from '$lib/state/canvas.svelte';
  import { layoutState, APP_INSTANCE_IDS, kindForInstanceId } from '$lib/state/layout.svelte';
  import { sessionsState, updateSession } from '$lib/state/sessions.svelte';
  import { fly } from 'svelte/transition';
  import { cubicOut } from 'svelte/easing';
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

  /** Sessions linked to the active canvas — feeds the collapsed
   *  rail-mini so the user sees which agents are attached even
   *  with the side pane closed. Only Claude can link to canvases
   *  (Cursor doesn't have canvas-linking yet) — kindForInstanceId
   *  filters that for us. */
  const linkedAgents = $derived.by(() => {
    if (!activeCanvasId) return [] as { sessionId: string; agentInstanceId: string; kind: 'claude' | 'cursor'; title: string }[];
    const out: { sessionId: string; agentInstanceId: string; kind: 'claude' | 'cursor'; title: string }[] = [];
    for (const s of sessionsState.list) {
      if (s.linkedCanvasId !== activeCanvasId) continue;
      const aid = s.agentInstanceId
        ?? (s.agentKind === 'claude' || s.agentKind === 'cursor' ? APP_INSTANCE_IDS[s.agentKind] : null);
      if (!aid) continue;
      const k = kindForInstanceId(aid);
      if (k !== 'claude' && k !== 'cursor') continue;
      out.push({ sessionId: s.id, agentInstanceId: aid, kind: k, title: s.title || 'Untitled chat' });
    }
    return out;
  });
</script>

<section
  class="app-shell sc-shell"
  class:sc-shell--rail={!sideOpen}
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
        <aside class="app-pane sc-side" in:fly={{ x: 24, duration: 220, easing: cubicOut }}>
          <header class="app-pane-head">
            <span class="app-pane-head-h">{instanceLabel}</span>
            <span class="sc-kind-tag mono">Canvas</span>
            <button class="app-iconbtn" title="Collapse pane" aria-label="Collapse pane" onclick={() => (sideOpen = false)}>
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M10 6l6 6-6 6"/></svg>
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
    <section class="app-pane sc-canvas">
      <CanvasSurface instanceId={p.instanceId} onCardOpen={p.onCardOpen} />
    </section>
    <SidePaneRail
      {linkedAgents}
      onExpand={() => (sideOpen = true)}
    />
  {/if}
</section>

<style>
  .sc-shell { display: block; padding: var(--app-pad, 14px); }
  /* When the side pane is collapsed, switch to a 2-col grid:
     canvas pane (1fr) + 44px rail. Splitter mode keeps `display:
     block` so it can manage its own layout. */
  .sc-shell.sc-shell--rail {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 44px;
    transition: grid-template-columns var(--dur-base) var(--ease-out);
  }
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
    height: 100%;
  }
  .sc-canvas :global(.canvas-surface) {
    background: var(--bg-0) !important;
  }

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
