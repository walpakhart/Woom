<script lang="ts">
  /* Paper redesign titlebar — 40px strip across the top. Native macOS
     traffic lights overlay the top-left (titleBarStyle: Transparent),
     so we only reserve space for them. Right cluster: ⌘K palette
     chip, theme toggle, Claude 5h quota meter. */
  import { themeState, applyTheme } from '$lib/state/theme.svelte';
  import { quotaState } from '$lib/state/quota.svelte';

  interface Props {
    soloTitle: string;
    onPalette?: () => void;
  }
  let { soloTitle, onPalette }: Props = $props();

  const fiveHourPct = $derived.by(() => {
    const u = quotaState.usage?.five_hour?.utilization;
    return typeof u === 'number' ? Math.max(0, Math.min(100, Math.round(u))) : null;
  });

  function toggleTheme() {
    applyTheme(themeState.name === 'light' ? 'iconic' : 'light');
  }
</script>

<header class="tb" data-tauri-drag-region>
  <div class="tb-lights" aria-hidden="true"></div>
  <div class="tb-title" data-tauri-drag-region>Woom — {soloTitle}</div>
  <div class="tb-spring" data-tauri-drag-region></div>

  <button class="tb-chip" onclick={() => onPalette?.()}>
    <span class="tb-chip-k">⌘K</span><span>search · run · connect</span>
  </button>

  <button class="tb-chip" onclick={toggleTheme} title="Toggle theme">
    {themeState.name === 'light' ? '☾ dark' : '☀ light'}
  </button>

  {#if fiveHourPct !== null}
    <div class="tb-quota" title="Claude quota — 5h window">
      <span>5h</span>
      <span class="tb-quota-track"><span class="tb-quota-fill" style="width:{fiveHourPct}%"></span></span>
      <span class="tb-quota-pct">{fiveHourPct}%</span>
    </div>
  {/if}
</header>

<style>
  .tb {
    flex: none;
    display: flex; align-items: center; gap: 14px;
    /* 34px lines the text centre (17px) up with macOS's NATIVE
       traffic-light position under titleBarStyle: Overlay — the
       trafficLightPosition config is ignored in Overlay mode, so
       the bar meets the buttons instead of the other way round. */
    height: 34px;
    padding: 0 16px;
    background: var(--bg-1);
    border-bottom: 1px solid var(--border);
    user-select: none;
    position: relative;
    z-index: 6;
  }
  /* Native traffic lights overlay this area (trafficLightPosition
     x:16 y:14 in tauri.conf centres them in the 40px bar). */
  .tb-lights { width: 66px; flex: none; }
  .tb-title {
    font-size: 12px; font-weight: 500;
    color: var(--text-1);
    letter-spacing: 0.02em;
    white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
  }
  .tb-spring { flex: 1; height: 100%; }
  .tb-chip {
    display: flex; align-items: center; gap: 6px;
    font-size: 11px;
    color: var(--text-mute);
    border: 1px solid var(--border-hi);
    border-radius: var(--r-btn);
    padding: 3px 9px;
    cursor: pointer;
    background: var(--bg-0);
    white-space: nowrap;
    transition: color 120ms, border-color 120ms;
  }
  .tb-chip:hover { color: var(--text-0); border-color: var(--border-hi2); }
  .tb-chip-k { font-weight: 600; }
  .tb-quota {
    display: flex; align-items: center; gap: 8px;
    font-size: 11px; color: var(--text-mute);
  }
  .tb-quota-track {
    display: inline-block;
    width: 52px; height: 5px;
    border-radius: 3px;
    background: var(--bg-4);
    overflow: hidden;
  }
  .tb-quota-fill {
    display: block; height: 100%;
    background: var(--src-claude);
    border-radius: 3px;
  }
  .tb-quota-pct { color: var(--text-1); }
</style>
