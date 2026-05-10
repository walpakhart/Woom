<script lang="ts">
  /* ChatHeader — top row of AgentApp center pane.
     v8: italic-accent agent name + neutral session title + running
     pulse + stop. The model picker moved into the composer's stats
     row in v8 — keeping the header focused on identity. */
  import { sessionsState } from '$lib/state/sessions.svelte';

  type Kind = 'claude' | 'cursor';

  interface Props {
    kind: Kind;
    instanceId: string;
    thinkingStartedAt: number | null;
    thinkingTick: number;
    onStop: () => void;
  }

  let p: Props = $props();

  const sess = $derived(
    sessionsState.list.find((s) => s.id === sessionsState.activeIds[p.kind]) ?? null
  );

  const elapsed = $derived.by(() => {
    if (!p.thinkingStartedAt || !sess?.sending) return '';
    void p.thinkingTick;
    const ms = Date.now() - p.thinkingStartedAt;
    const s = Math.floor(ms / 1000);
    if (s < 60) return `${s}s`;
    const m = Math.floor(s / 60);
    const r = s % 60;
    return `${m}m ${String(r).padStart(2, '0')}s`;
  });

  const kindLabel = $derived(p.kind === 'claude' ? 'Claude' : 'Cursor');
</script>

<header class="ch">
  <div class="ch-title">
    <span class="ch-agent">{kindLabel}</span>
    <span class="ch-dot">·</span>
    {#if sess}
      <span class="ch-sess">{sess.title || 'Untitled chat'}</span>
    {:else}
      <span class="ch-sess ch-sess--empty">No session</span>
    {/if}
  </div>

  {#if sess?.sending}
    <span class="ch-running">
      <span class="ch-pip"></span>
      <span class="mono">{elapsed || 'thinking'}</span>
    </span>

    <button class="ch-stop" onclick={p.onStop} title="Stop generation">
      <svg viewBox="0 0 24 24" fill="currentColor"><rect x="6" y="6" width="12" height="12" rx="1.5"/></svg>
    </button>
  {/if}
</header>

<style>
  .ch {
    flex: 0 0 56px;
    display: flex; align-items: center; gap: 12px;
    padding: 0 22px;
    border-bottom: 1px solid var(--border);
    background: linear-gradient(180deg, var(--bg-2), var(--bg-1));
    min-height: 0;
  }
  .ch-title {
    flex: 1; min-width: 0;
    display: flex; align-items: baseline; gap: 8px;
    font-family: 'Instrument Serif', 'New York', Georgia, serif;
    font-size: 22px; font-weight: 400;
    letter-spacing: -0.02em;
    white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
  }
  .ch-agent {
    color: var(--app-tone, var(--src-claude));
    font-style: italic;
  }
  .ch-dot { color: var(--text-mute); font-style: normal; }
  .ch-sess { color: var(--text-0); }
  .ch-sess--empty { color: var(--text-mute); }

  .ch-running {
    display: inline-flex; align-items: center; gap: 6px;
    font-size: 11px; color: var(--text-mute);
  }
  .ch-pip {
    width: 6px; height: 6px; border-radius: 50%;
    background: var(--app-tone, var(--accent));
    box-shadow: 0 0 6px var(--app-glow, var(--accent-glow));
    animation: ch-pulse 1.4s infinite;
  }
  @keyframes ch-pulse {
    0%, 100% { opacity: 0.45; transform: scale(0.9); }
    50%      { opacity: 1;    transform: scale(1.1); }
  }

  .ch-stop {
    width: 28px; height: 28px;
    display: grid; place-items: center;
    border-radius: 7px;
    background: rgba(232, 130, 100, 0.10);
    border: 1px solid rgba(232, 130, 100, 0.32);
    color: var(--error);
    cursor: pointer;
    transition: background 140ms;
  }
  .ch-stop:hover { background: rgba(232, 130, 100, 0.18); }
  .ch-stop svg { width: 12px; height: 12px; }
</style>
