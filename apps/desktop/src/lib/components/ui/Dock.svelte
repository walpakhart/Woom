<script lang="ts">
  /* Quiet-direction bottom Dock (redesign v2 §3.1). Replaces the
     IconRail + Titlebar chrome when `data-layout='quiet'`: a 42px
     charcoal strip (same in both colour themes) with the woom mark,
     word-sections that switch solos, and a spend/quota readout on the
     right. Status dots ride on the words (source badge counts, claude
     running, editor review count as a superscript). */
  import { quotaState } from '$lib/state/quota.svelte';

  /* Matches +page.svelte's local View union (superset of the exported
     one — it also carries the system solos: library, rules, …). */
  type View =
    | 'home' | 'jiraApp' | 'githubApp' | 'sentryApp' | 'claudeApp'
    | 'editorApp' | 'canvasApp' | 'terminalApp'
    | 'rules' | 'library' | 'connections' | 'settings';

  interface Props {
    view: View;
    onNav: (v: View) => void;
    jiraBadge?: number;
    githubBadge?: number;
    sentryBadge?: number;
    claudeBusy?: boolean;
    editorReview?: number;
  }
  let p: Props = $props();

  type Word = {
    view: View;
    label: string;
    badge?: number;
    running?: boolean;
    sup?: number;
  };

  const words = $derived<Word[]>([
    { view: 'home', label: 'home' },
    { view: 'jiraApp', label: 'jira', badge: p.jiraBadge },
    { view: 'githubApp', label: 'github', badge: p.githubBadge },
    { view: 'sentryApp', label: 'sentry', badge: p.sentryBadge },
    { view: 'claudeApp', label: 'claude', running: p.claudeBusy },
    { view: 'editorApp', label: 'editor', sup: p.editorReview },
    { view: 'canvasApp', label: 'canvas' },
    { view: 'terminalApp', label: 'terminal' },
    { view: 'library', label: 'library' },
    { view: 'rules', label: 'rules' },
    { view: 'connections', label: 'connections' },
    { view: 'settings', label: 'settings' }
  ]);

  const fiveHourPct = $derived.by(() => {
    const u = quotaState.usage?.five_hour?.utilization;
    return typeof u === 'number' ? Math.max(0, Math.min(100, Math.round(u))) : null;
  });
  const weekPct = $derived.by(() => {
    const u = quotaState.usage?.seven_day?.utilization;
    return typeof u === 'number' ? Math.max(0, Math.min(100, Math.round(u))) : null;
  });
</script>

<footer class="dock" aria-label="Quiet dock">
  <span class="dock-mark" aria-hidden="true"></span>

  <nav class="dock-nav">
    {#each words as w (w.view)}
      <button
        class="dock-word"
        class:active={p.view === w.view}
        onclick={() => p.onNav(w.view)}
        aria-current={p.view === w.view ? 'page' : undefined}
      >
        {w.label}{#if w.sup && w.sup > 0}<sup class="dock-sup">{w.sup}</sup>{/if}
        {#if w.running}
          <span class="dock-dot dock-dot--run" aria-label="running"></span>
        {:else if w.badge && w.badge > 0}
          <span class="dock-dot" class:dock-dot--err={w.view === 'sentryApp'} aria-hidden="true"></span>
        {/if}
      </button>
    {/each}
  </nav>

  <span class="dock-spring"></span>

  {#if fiveHourPct !== null}
    <span class="dock-quota" title="Claude — 5h quota window">
      <span class="dock-quota-label mono">5h</span>
      <span class="dock-quota-track"><span class="dock-quota-fill" class:full={fiveHourPct >= 100} style="width:{fiveHourPct}%"></span></span>
      <span class="dock-quota-pct mono">{fiveHourPct}%</span>
    </span>
  {/if}
  {#if p.view === 'claudeApp' && weekPct !== null}
    <span class="dock-quota" title="Claude — 7-day quota window">
      <span class="dock-quota-label mono">week</span>
      <span class="dock-quota-track"><span class="dock-quota-fill" class:full={weekPct >= 100} style="width:{weekPct}%"></span></span>
      <span class="dock-quota-pct mono">{weekPct}%</span>
    </span>
  {/if}
</footer>

<style>
  /* Charcoal in BOTH themes (§3.1). */
  .dock {
    flex: none;
    display: flex; align-items: center; gap: 14px;
    height: 42px;
    padding: 0 18px;
    background: var(--dark-0);
    border-top: 1px solid rgba(0, 0, 0, 0.35);
  }
  :global(:root[data-theme='light']) .dock { background: var(--dark-1); }

  /* Real engraved W — alpha mask re-inked (matches Titlebar + mockup
     footer). Mockup 4j: 24×12 mark, ink #98A0A8 (= --dark-text-2). */
  .dock-mark {
    flex: none;
    display: block;
    width: 24px; height: 12px;
    background: var(--dark-text-2);
    -webkit-mask: url('/woom-mark-ink.png') center / contain no-repeat;
    mask: url('/woom-mark-ink.png') center / contain no-repeat;
  }

  .dock-nav {
    display: flex; align-items: center; gap: 14px;
    min-width: 0; overflow: hidden;
  }
  .dock-word {
    position: relative;
    display: inline-flex; align-items: center; gap: 4px;
    padding: 0; border: 0; background: transparent;
    font-size: 12.5px; color: var(--dark-mute);
    cursor: pointer;
    white-space: nowrap;
    transition: color 120ms;
  }
  .dock-word:hover { color: var(--dark-text-2); }
  .dock-word.active { color: var(--dark-text); font-weight: 600; }
  .dock-sup { font-size: 8px; color: var(--dark-text-2); line-height: 1; }
  .dock-dot {
    width: 4px; height: 4px; border-radius: 50%;
    background: var(--dark-text-2);
    flex: none;
  }
  .dock-dot--err { background: var(--term-err); }
  .dock-dot--run {
    width: 5px; height: 5px;
    background: var(--term-ok);
    animation: dock-pulse 1.6s ease-in-out infinite;
  }
  @keyframes dock-pulse { 0%,100% { opacity: 0.4; } 50% { opacity: 1; } }
  @media (prefers-reduced-motion: reduce) { .dock-dot--run { animation: none; } }

  .dock-spring { flex: 1; }

  .dock-quota { display: inline-flex; align-items: center; gap: 6px; flex: none; }
  .dock-quota-label { font-size: 10.5px; color: var(--dark-mute); }
  .dock-quota-track {
    width: 44px; height: 4px; border-radius: 2px;
    background: #2A2F35; overflow: hidden;
  }
  .dock-quota-fill { display: block; height: 100%; background: var(--dark-text-2); border-radius: 2px; }
  .dock-quota-fill.full { background: var(--term-warn); }
  .dock-quota-pct { font-size: 11px; color: var(--dark-mute); min-width: 30px; }
</style>
