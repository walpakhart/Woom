<script lang="ts">
  /* SidePaneRail — 44px vertical strip that mirrors the editor's
     ActivityBar on the right edge when a side pane is collapsed.
     Single source of truth across EditorApp / CanvasApp /
     TerminalApp / AgentApp.

       - top: expand-button («‹»)
       - middle: one square per linked agent (BrandIcon)
       - empty state: a muted dash so the rail doesn't look broken

     Visual identity matches `.eab` ActivityBar — same width, same
     hover treatment, same plain background. NOT a glass / floating
     pane — sits flush as a column in the parent grid. */
  import BrandIcon from './BrandIcon.svelte';
  import { setActiveSessionInInstance } from '$lib/state/sessions.svelte';

  interface LinkedAgent {
    sessionId: string;
    agentInstanceId: string;
    kind: 'claude' | 'cursor';
    name?: string;
    title?: string;
  }

  interface Props {
    linkedAgents: LinkedAgent[];
    reviewCount?: number;
    onExpand: () => void;
    expandOnAgentClick?: boolean;
    onAgentClick?: (a: LinkedAgent) => void;
  }
  let p: Props = $props();

  function defaultAgentClick(a: LinkedAgent) {
    setActiveSessionInInstance(a.agentInstanceId, a.sessionId);
    if (p.expandOnAgentClick !== false) p.onExpand();
  }

  function handleAgentClick(a: LinkedAgent) {
    if (p.onAgentClick) p.onAgentClick(a);
    else defaultAgentClick(a);
  }
</script>

<aside class="spr app-pane">
  <button
    class="spr-btn spr-btn--expand"
    aria-label="Expand pane"
    onclick={p.onExpand}
  >
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M14 6l-6 6 6 6"/></svg>
    {#if p.reviewCount && p.reviewCount > 0}
      <span class="spr-badge">{p.reviewCount}</span>
    {/if}
  </button>
  <div class="spr-divider" aria-hidden="true"></div>
  {#each p.linkedAgents as la (la.sessionId)}
    <button
      class="spr-btn spr-btn--agent"
      data-agent={la.kind}
      aria-label="Open {la.kind} chat: {la.title ?? la.name ?? 'chat'}"
      onclick={() => handleAgentClick(la)}
    >
      <BrandIcon kind={la.kind} size={18} />
    </button>
  {/each}
  {#if p.linkedAgents.length === 0}
    <span class="spr-empty mono" aria-hidden="true">—</span>
  {/if}
</aside>

<style>
  /* 44px-wide rail that mirrors `.eab` (editor ActivityBar) but
     wears the standard `.app-pane` chrome (border + radius + shadow)
     so it floats as a proper rounded panel — same chassis as every
     other side panel in the app. */
  .spr {
    display: flex; flex-direction: column;
    align-items: center;
    gap: 4px;
    padding: 8px 0;
    width: 44px;
    height: 100%;
  }
  .spr-btn {
    position: relative;
    width: 32px; height: 32px;
    display: grid; place-items: center;
    border-radius: 8px;
    color: var(--text-2);
    background: transparent;
    border: 1px solid transparent;
    cursor: pointer;
    flex-shrink: 0;
    transition:
      color var(--dur-quick) var(--ease-out),
      background var(--dur-quick) var(--ease-out),
      border-color var(--dur-quick) var(--ease-out);
  }
  .spr-btn:hover {
    color: var(--text-0);
    background: var(--bg-2);
    border-color: var(--border-hi);
  }
  .spr-btn--expand svg { width: 14px; height: 14px; }
  .spr-btn--agent { padding: 4px; }
  .spr-btn--agent[data-agent='claude']:hover {
    background: color-mix(in srgb, var(--src-claude) 14%, var(--bg-2));
    border-color: color-mix(in srgb, var(--src-claude) 32%, var(--border));
  }
  .spr-btn--agent[data-agent='cursor']:hover {
    background: color-mix(in srgb, var(--src-cursor) 14%, var(--bg-2));
    border-color: color-mix(in srgb, var(--src-cursor) 32%, var(--border));
  }
  .spr-divider {
    width: 22px; height: 1px;
    background: var(--border);
    margin: 2px 0;
    flex-shrink: 0;
  }
  .spr-empty {
    color: var(--text-mute);
    font-size: 12px;
    margin-top: 2px;
  }
  .spr-badge {
    position: absolute; top: -3px; right: -3px;
    min-width: 14px; height: 14px; padding: 0 3px;
    border-radius: 7px;
    font-family: 'JetBrains Mono', monospace;
    font-size: 9px; font-weight: 700;
    background: var(--accent-bright); color: var(--accent-fg);
    display: grid; place-items: center;
    box-shadow: 0 0 0 2px var(--bg-1);
  }
</style>
