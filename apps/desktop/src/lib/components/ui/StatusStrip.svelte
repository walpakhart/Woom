<script lang="ts">
  /* Paper redesign status strip — 34px charcoal band across the
     bottom (same --dark-0 in both themes, per the mockup's "dark
     insets don't theme" rule). Left: running background task +
     busy-agent chip. Right: source counters. */
  import { bgTasksState } from '$lib/state/bgTasks.svelte';

  interface Props {
    claudeBusy?: boolean;
    claudeLabel?: string | null;
    githubBadge?: number;
    sentryBadge?: number;
    onGoClaude?: () => void;
  }
  let {
    claudeBusy = false,
    claudeLabel = null,
    githubBadge = 0,
    sentryBadge = 0,
    onGoClaude
  }: Props = $props();

  const runningTask = $derived(
    bgTasksState.tasks.find((t) => t.status.kind === 'running') ?? null
  );
  const runningCount = $derived(
    bgTasksState.tasks.filter((t) => t.status.kind === 'running').length
  );
</script>

{#if runningTask || claudeBusy}
<footer class="ss">
  {#if runningTask}
    <button class="ss-item" onclick={() => onGoClaude?.()}>
      <span class="ss-spin" aria-hidden="true"></span>
      <span class="ss-strong">$ {runningTask.cmd}</span>
      {#if runningCount > 1}<span>+{runningCount - 1} more</span>{/if}
    </button>
  {/if}
  {#if claudeBusy}
    <button class="ss-item" onclick={() => onGoClaude?.()}>
      <span class="ss-pulse"></span>
      <span>claude{claudeLabel ? ` — ${claudeLabel}` : ' — working'}</span>
    </button>
  {/if}
  <div class="ss-spring"></div>
  {#if sentryBadge}<span class="ss-meta">{sentryBadge} issues</span>{/if}
  {#if githubBadge}<span class="ss-meta">↑{githubBadge}</span>{/if}
</footer>
{/if}

<style>
  .ss {
    flex: none;
    display: flex; align-items: center; gap: 18px;
    height: 34px;
    padding: 0 16px;
    background: var(--dark-0);
    color: var(--dark-text-2);
    font-size: 11px;
    border-top: 1px solid rgba(0, 0, 0, 0.2);
    user-select: none;
    position: relative;
    z-index: 6;
  }
  .ss-item {
    display: flex; align-items: center; gap: 8px;
    background: none; border: 0; padding: 0;
    color: var(--dark-text-2);
    font-size: 11px;
    cursor: pointer;
    white-space: nowrap;
    transition: color 120ms;
  }
  .ss-item:hover { color: var(--dark-text); }
  .ss-strong { color: var(--dark-text); }
  .ss-spring { flex: 1; }
  .ss-meta { color: var(--dark-text-2); white-space: nowrap; }
  .ss-spin {
    display: inline-block;
    width: 9px; height: 9px;
    border: 1.5px solid var(--dark-mute);
    border-top-color: var(--dark-text);
    border-radius: 50%;
    animation: ss-spin 0.9s linear infinite;
  }
  .ss-pulse {
    width: 6px; height: 6px; border-radius: 50%;
    background: var(--src-claude);
    animation: ss-pulsedot 1.6s infinite;
  }
  @keyframes ss-spin { to { transform: rotate(360deg); } }
  @keyframes ss-pulsedot {
    0%, 100% { opacity: 0.35; }
    50% { opacity: 1; }
  }
</style>
