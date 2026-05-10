<script lang="ts">
  /* TerminalApp — full-screen workspace для терминала.
     Layout: [TerminalSurface (flex)] [side quick-commands (300)]

     Right side — quick commands rail. На MVP статичный список pinned
     команд + "Hand to Claude" CTA. История + drop-в-Claude — следующий
     milestone. */

  import TerminalSurface from './terminal/TerminalSurface.svelte';
  import Splitter from '$lib/components/ui/Splitter.svelte';
  import { layoutState } from '$lib/state/layout.svelte';

  interface Props {
    instanceId: string;
    cwd?: string | null;
    onOpenClaude: () => void;
  }
  let p: Props = $props();

  let sideOpen = $state(true);

  /** Curated mark of the active Terminal instance — surfaces in the
   *  side panel head so users always see which Hopper / Hokusai
   *  terminal they're inside. */
  const instanceLabel = $derived(
    layoutState.instances.terminal.find((i) => i.id === p.instanceId)?.name ?? 'Terminal'
  );
</script>

<section
  class="app-shell st-shell"
  style="--app-tone: var(--src-term); --app-glow: rgba(245,240,234,0.30);"
>
  {#if sideOpen}
    <Splitter
      direction="horizontal"
      fixedSide="end"
      persistKey="terminal-side"
      initial={300}
      min={240}
      max={520}
    >
      {#snippet start()}
        <section class="app-pane st-main">
          <TerminalSurface instanceId={p.instanceId} cwd={p.cwd ?? null} />
        </section>
      {/snippet}
      {#snippet end()}
        <aside class="app-pane st-side">
          <header class="app-pane-head">
            <span class="app-pane-head-h">{instanceLabel}</span>
            <span class="st-kind-tag mono">Terminal</span>
            <button class="app-iconbtn" title="Hide" onclick={() => (sideOpen = false)}>
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7"><path d="M18 6 6 18M6 6l12 12"/></svg>
            </button>
          </header>
      <div class="st-side-body">
        <div class="st-group mono">Pinned</div>
        <button class="qc-row">
          <span class="qc-icon"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7"><polyline points="9 11 12 14 22 4"/></svg></span>
          <span class="qc-body">
            <span class="qc-name mono">pnpm test --filter=desktop</span>
            <span class="qc-desc">run vitest suite</span>
          </span>
        </button>
        <button class="qc-row">
          <span class="qc-icon"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7"><path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/></svg></span>
          <span class="qc-body">
            <span class="qc-name mono">pnpm typecheck</span>
            <span class="qc-desc">tsc --noEmit</span>
          </span>
        </button>
        <button class="qc-row">
          <span class="qc-icon"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7"><circle cx="12" cy="12" r="9"/><polyline points="12 6 12 12 16 14"/></svg></span>
          <span class="qc-body">
            <span class="qc-name mono">scripts/build-dmg.sh</span>
            <span class="qc-desc">notarized universal dmg</span>
          </span>
        </button>
        <button class="qc-row">
          <span class="qc-icon"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7"><circle cx="6" cy="6" r="2.5"/><circle cx="18" cy="18" r="2.5"/><path d="M6 8.5V14a4 4 0 0 0 4 4h6"/></svg></span>
          <span class="qc-body">
            <span class="qc-name mono">git status -sb</span>
            <span class="qc-desc">short branch status</span>
          </span>
        </button>

        <div class="st-group mono">Hand to Claude</div>
        <button class="qc-row qc-row--accent" onclick={p.onOpenClaude}>
          <span class="qc-icon qc-icon--claude"><svg viewBox="0 0 24 24" fill="currentColor" stroke="none"><path d="M12 2 L14.5 9.5 L22 12 L14.5 14.5 L12 22 L9.5 14.5 L2 12 L9.5 9.5 Z"/></svg></span>
          <span class="qc-body">
            <span class="qc-name">Open Claude</span>
            <span class="qc-desc">switch to a focused agent workspace</span>
          </span>
        </button>
          </div>
        </aside>
      {/snippet}
    </Splitter>
  {:else}
    <section class="app-pane st-main st-main--full">
      <TerminalSurface instanceId={p.instanceId} cwd={p.cwd ?? null} />
      <button class="st-show-side" title="Show quick commands" onclick={() => (sideOpen = true)}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7"><path d="M14 6l-6 6 6 6"/></svg>
      </button>
    </section>
  {/if}
</section>

<style>
  .st-shell { display: block; padding: var(--app-pad, 14px); }
  .st-shell :global(.s-start),
  .st-shell :global(.s-end) {
    height: 100%;
    display: flex;
    min-width: 0;
  }
  .st-shell :global(.s-start) > :global(*),
  .st-shell :global(.s-end) > :global(*) {
    flex: 1 1 auto;
    width: 100%;
    min-width: 0;
  }

  .st-main {
    display: flex;
    overflow: hidden;
    background: #1a1610;
    position: relative;
  }
  .st-main--full { height: 100%; }
  .st-main :global(.terminal-surface) {
    background: #1a1610 !important;
    flex: 1 1 auto;
  }
  .st-show-side {
    position: absolute;
    top: 14px; right: 14px;
    width: 26px; height: 26px;
    display: grid; place-items: center;
    border-radius: 6px;
    background: rgba(34,28,23,0.85);
    border: 1px solid var(--border);
    color: var(--text-2);
    cursor: pointer;
    backdrop-filter: blur(8px);
  }
  .st-show-side:hover { color: var(--text-0); border-color: var(--border-hi); }
  .st-show-side svg { width: 13px; height: 13px; }

  /* Mono kind tag next to the editorial instance name. */
  .st-kind-tag {
    font-size: 9.5px; font-weight: 700;
    letter-spacing: 0.10em;
    text-transform: uppercase;
    padding: 2px 7px;
    border-radius: 4px;
    background: color-mix(in srgb, var(--src-term) 10%, var(--bg-3));
    color: var(--text-1);
    border: 1px solid color-mix(in srgb, var(--src-term) 18%, transparent);
  }

  .st-side-body {
    flex: 1;
    overflow-y: auto;
    padding: 8px;
  }
  .st-group {
    padding: 14px 8px 6px;
    font-size: 9.5px; font-weight: 700;
    letter-spacing: 0.10em;
    text-transform: uppercase;
    color: var(--text-mute);
  }

  .qc-row {
    display: flex; align-items: center; gap: 10px;
    padding: 9px 11px;
    border-radius: 9px;
    cursor: pointer;
    width: 100%;
    text-align: left;
    background: transparent;
    border: 1px solid transparent;
    transition: background 120ms, border-color 120ms;
    margin-bottom: 2px;
  }
  .qc-row:hover { background: var(--bg-2); border-color: var(--border); }
  .qc-row--accent {
    background: color-mix(in srgb, var(--accent) 4%, transparent);
    border-color: var(--border-accent-2);
  }
  .qc-row--accent:hover {
    background: color-mix(in srgb, var(--accent) 8%, transparent);
    border-color: var(--border-accent);
  }
  .qc-icon {
    width: 28px; height: 28px;
    display: grid; place-items: center;
    border-radius: 7px;
    background: var(--bg-3); color: var(--accent-bright);
    flex-shrink: 0;
    box-shadow: inset 0 0 0 1px var(--border);
  }
  .qc-icon--claude {
    background: var(--accent-soft); color: var(--src-claude);
    box-shadow: inset 0 0 0 1px var(--border-accent-2);
  }
  .qc-icon svg { width: 14px; height: 14px; }
  .qc-body { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 2px; }
  .qc-name {
    font-size: 11.5px; color: var(--text-0); font-weight: 500;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .qc-desc {
    font-size: 10.5px; color: var(--text-mute);
    line-height: 1.35;
  }
</style>
