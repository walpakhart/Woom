<script lang="ts">
  /* SidePaneRail — shared 52px rail that replaces a full side pane
     when collapsed. Same shape across EditorApp / CanvasApp /
     TerminalApp / AgentApp's worktree slot:
       - top: expand-button («‹»)
       - middle: one square per linked agent (BrandIcon)
       - bottom: optional caller-supplied snippet (e.g. "+ link")

     Keeps a light visual identity to the pane it replaces — caller
     passes `tone` so the rail tints accent shadows / hover glow.
     Animates in via parent's `transition:fly`; the rail itself
     doesn't own a transition so the caller can choose the easing. */
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
    /** Auto-expand when an agent icon is clicked? Default true.
     *  The caller is responsible for actually showing the chat for
     *  the activated session — we just flip its activeId so the
     *  pane has something to show. */
    expandOnAgentClick?: boolean;
    /** Optional override — what to do when an agent icon is clicked.
     *  Default: setActiveSessionInInstance + onExpand. Use this when
     *  the caller wants to also focus a different solo (e.g. open
     *  the agent app instead of just expanding the pane). */
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

<aside class="spr">
  <button
    class="spr-btn spr-btn--expand"
    title="Expand pane · {p.linkedAgents.length} linked"
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
      title="{la.kind === 'claude' ? 'Claude' : 'Cursor'}: {la.title ?? la.name ?? 'chat'}"
      aria-label="Open {la.kind} chat: {la.title ?? la.name ?? 'chat'}"
      onclick={() => handleAgentClick(la)}
    >
      <BrandIcon kind={la.kind} size={20} />
    </button>
  {/each}
  {#if p.linkedAgents.length === 0}
    <span class="spr-empty mono" title="No agents linked here">—</span>
  {/if}
</aside>

<style>
  .spr {
    display: flex; flex-direction: column;
    align-items: center;
    gap: 6px;
    padding: 8px 6px;
    height: 100%;
    background: var(--bg-glass, rgba(20, 24, 26, 0.66));
    border-left: 1px solid var(--border);
    backdrop-filter: blur(14px);
    -webkit-backdrop-filter: blur(14px);
    overflow: hidden;
    box-sizing: border-box;
  }
  .spr-btn {
    position: relative;
    width: 36px; height: 36px;
    display: grid; place-items: center;
    border-radius: 8px;
    color: var(--text-2);
    background: transparent; border: 1px solid transparent;
    cursor: pointer;
    flex-shrink: 0;
    transition:
      color var(--dur-quick) var(--ease-out),
      background var(--dur-quick) var(--ease-out),
      border-color var(--dur-quick) var(--ease-out),
      transform var(--dur-quick) var(--ease-spring);
  }
  .spr-btn:hover {
    color: var(--text-0);
    background: var(--bg-elev, var(--bg-2));
    border-color: var(--border-hi);
    transform: scale(1.04);
  }
  .spr-btn--expand svg { width: 14px; height: 14px; }
  .spr-btn--agent { padding: 4px; }
  .spr-btn--agent[data-agent='claude'] { border-color: color-mix(in srgb, var(--src-claude) 28%, transparent); }
  .spr-btn--agent[data-agent='cursor'] { border-color: color-mix(in srgb, var(--src-cursor) 28%, transparent); }
  .spr-btn--agent[data-agent='claude']:hover { background: color-mix(in srgb, var(--src-claude) 14%, var(--bg-2)); }
  .spr-btn--agent[data-agent='cursor']:hover { background: color-mix(in srgb, var(--src-cursor) 14%, var(--bg-2)); }
  .spr-divider {
    width: 28px; height: 1px;
    background: var(--border);
    margin: 4px 0;
    flex-shrink: 0;
  }
  .spr-empty {
    color: var(--text-mute);
    font-size: 14px;
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
