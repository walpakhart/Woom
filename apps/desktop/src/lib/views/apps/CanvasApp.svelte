<script lang="ts">
  import CanvasSurface from './canvas/CanvasSurface.svelte';
  import InlineClaude from './editor/InlineClaude.svelte';
  import Splitter from '$lib/components/ui/Splitter.svelte';
  import SidePaneRail from '$lib/components/ui/SidePaneRail.svelte';
  import { canvasState, setActiveCanvasTab, closeCanvasTab, createAndOpenInInstance } from '$lib/state/canvas.svelte';
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

  /* Redesign v2 §2.7 — canvases list column. Lists this instance's open
     canvases (tabs); click switches, "+" creates, hover × closes. */
  const canvasTabs = $derived.by(() => {
    const ids = canvasState.byInstance[p.instanceId]?.tabs ?? [];
    return ids.map((id) => {
      const c = canvasState.open[id];
      return { id, name: c?.name ?? 'Untitled', shapes: c?.shapes.length ?? 0 };
    });
  });
  function removeCanvas(id: string, e: MouseEvent) {
    e.stopPropagation();
    closeCanvasTab(p.instanceId, id);
  }

  function handleLinkSession(sessionId: string) {
    if (!activeCanvasId) return;
    const sess = sessionsState.list.find((s) => s.id === sessionId);
    if (!sess) return;
    const patch: Partial<typeof sess> = { linkedCanvasId: activeCanvasId };
    if (!sess.agentInstanceId) patch.agentInstanceId = APP_INSTANCE_IDS.claude;
    updateSession(sessionId, patch);
  }

  function handleUnlinkSession(sessionId: string) {
    updateSession(sessionId, { linkedCanvasId: null });
  }

  /** Sessions linked to the active canvas — feeds the collapsed
   *  rail-mini so the user sees which agents are attached even
   *  with the side pane closed. */
  const linkedAgents = $derived.by(() => {
    const out: { sessionId: string; agentInstanceId: string; kind: 'claude'; title: string }[] = [];
    if (!activeCanvasId) return out;
    for (const s of sessionsState.list) {
      if (s.archived) continue;
      if (s.linkedCanvasId !== activeCanvasId) continue;
      const aid = s.agentInstanceId ?? APP_INSTANCE_IDS.claude;
      if (kindForInstanceId(aid) !== 'claude') continue;
      out.push({ sessionId: s.id, agentInstanceId: aid, kind: 'claude', title: s.title || 'Untitled chat' });
    }
    return out;
  });
</script>

<section
  class="app-shell sc-shell"
  class:sc-shell--rail={!sideOpen}
  style="--app-tone: var(--src-canvas); --app-glow: rgba(125,201,176,0.40);"
>
  <aside class="lp sc-list">
    <header class="lp-head">
      <span class="lp-title">Canvases</span>
      <span class="lp-count">{canvasTabs.length}</span>
      <span class="lp-head-spring"></span>
      <button class="lp-add" onclick={() => createAndOpenInInstance(p.instanceId)} title="New canvas" aria-label="New canvas">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round"><path d="M12 5v14M5 12h14"/></svg>
      </button>
    </header>
    <div class="lp-list">
      {#each canvasTabs as c (c.id)}
        <button
          class="lp-row sc-list-row"
          class:active={c.id === activeCanvasId}
          onclick={() => setActiveCanvasTab(p.instanceId, c.id)}
        >
          <span class="lp-row-title">{c.name}</span>
          <span class="lp-row-meta">{c.shapes} card{c.shapes === 1 ? '' : 's'}</span>
          {#if canvasTabs.length > 1}
            <span
              class="sc-list-x"
              role="button"
              tabindex="-1"
              title="Close {c.name}"
              aria-label="Close {c.name}"
              onclick={(e) => removeCanvas(c.id, e)}
              onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); removeCanvas(c.id, e as unknown as MouseEvent); } }}
            >
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"><line x1="6" y1="6" x2="18" y2="18"/><line x1="6" y1="18" x2="18" y2="6"/></svg>
            </span>
          {/if}
        </button>
      {/each}
    </div>
  </aside>
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
  /* Redesign v2 §2.7 — [list 264][surface(+side)]; flush, no shell pad. */
  .sc-shell {
    display: grid;
    grid-template-columns: 264px minmax(0, 1fr);
    grid-template-rows: minmax(0, 1fr);
    padding: 0;
  }
  .sc-shell.sc-shell--rail {
    grid-template-columns: 264px minmax(0, 1fr) 44px;
    transition: grid-template-columns var(--dur-base) var(--ease-out);
  }
  /* Canvases list column. */
  .sc-list { min-height: 0; }
  .sc-list-row { display: flex; align-items: center; gap: 8px; }
  .sc-list-row .lp-row-title { flex: 1; min-width: 0; }
  .sc-list-row .lp-row-meta { flex: none; }
  .sc-list-x {
    flex: none; width: 18px; height: 18px;
    display: grid; place-items: center;
    border-radius: 4px; color: var(--text-mute); cursor: pointer;
    opacity: 0; transition: opacity 120ms, color 120ms, background 120ms;
  }
  .sc-list-x svg { width: 11px; height: 11px; }
  .sc-list-row:hover .sc-list-x { opacity: 0.8; }
  .sc-list-x:hover { opacity: 1; color: var(--err); background: var(--bg-3); }
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
