<script lang="ts">
  /* Cabin titlebar — 40px strip (redesign v2, §2.1). Native macOS
     traffic lights overlay the top-left, so we reserve a 58px well.
     Layout: mark → breadcrumb (woom / {solo} · {instance meta}) →
     spring → ⌘K chip → 5h quota meter (bar only, no percent digit) →
     identity avatar. Theme toggle + window title + percent-as-number
     were removed (theme lives in Settings / ⌘K). */
  import { quotaState } from '$lib/state/quota.svelte';
  import RailIdentityAvatar from './rail/RailIdentityAvatar.svelte';
  import type {
    ClaudeStatus,
    ConnectionStatus,
    JiraStatus,
    SentryStatus
  } from '$lib/data';

  interface Props {
    soloTitle: string;
    instanceMeta?: string;
    onPalette?: () => void;
    githubStatus: ConnectionStatus;
    jiraStatus?: JiraStatus;
    sentryStatus?: SentryStatus;
    claudeStatus?: ClaudeStatus | null;
  }
  let {
    soloTitle,
    instanceMeta,
    onPalette,
    githubStatus,
    jiraStatus,
    sentryStatus,
    claudeStatus
  }: Props = $props();

  const fiveHourPct = $derived.by(() => {
    const u = quotaState.usage?.five_hour?.utilization;
    return typeof u === 'number' ? Math.max(0, Math.min(100, Math.round(u))) : null;
  });
</script>

<header class="tb" data-tauri-drag-region>
  <div class="tb-lights" aria-hidden="true"></div>
  <span class="tb-mark" role="img" aria-label="Woom"></span>

  <div class="tb-crumb" data-tauri-drag-region>
    <span class="tb-crumb-root">woom</span>
    <span class="tb-crumb-sep">/</span>
    <span class="tb-crumb-solo">{soloTitle}</span>
    {#if instanceMeta}
      <span class="tb-crumb-sep">·</span>
      <span class="tb-crumb-meta mono">{instanceMeta}</span>
    {/if}
  </div>

  <div class="tb-spring" data-tauri-drag-region></div>

  <button class="tb-cmdk" onclick={() => onPalette?.()}>
    <span class="tb-cmdk-k">⌘K</span><span class="tb-cmdk-label">search · commands</span>
  </button>

  {#if fiveHourPct !== null}
    <div class="tb-5h" title="Claude — 5h quota window">
      <span class="tb-5h-label mono">5h</span>
      <span class="tb-5h-track"><span class="tb-5h-fill" style="width:{fiveHourPct}%"></span></span>
    </div>
  {/if}

  <RailIdentityAvatar
    placement="titlebar"
    {githubStatus}
    {jiraStatus}
    {sentryStatus}
    {claudeStatus}
  />
</header>

<style>
  .tb {
    flex: none;
    display: flex; align-items: center; gap: 14px;
    height: 40px;
    padding: 0 16px;
    background: var(--bg-0);
    border-bottom: 1px solid var(--border-lo);
    user-select: none;
    position: relative;
    z-index: 6;
  }
  /* Native traffic lights overlay this well. */
  .tb-lights { width: 58px; flex: none; }

  /* Engraved W — alpha mask re-inked per theme. */
  .tb-mark {
    display: block; flex: none;
    width: 24px; height: 12px;
    background: var(--text-0);
    -webkit-mask: url('/woom-mark-ink.png') center / contain no-repeat;
    mask: url('/woom-mark-ink.png') center / contain no-repeat;
  }

  .tb-crumb {
    display: flex; align-items: baseline; gap: 6px;
    font-size: 12.5px;
    white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
    min-width: 0;
  }
  .tb-crumb-root { color: var(--text-faint); }
  .tb-crumb-sep { color: var(--text-faint); }
  .tb-crumb-solo { color: var(--text-1); font-weight: 500; }
  .tb-crumb-meta {
    font-size: 11px; color: var(--text-faint);
    overflow: hidden; text-overflow: ellipsis;
  }

  .tb-spring { flex: 1; height: 100%; }

  .tb-cmdk {
    display: flex; align-items: center; gap: 6px;
    font-size: 11.5px;
    border: 1px solid var(--border);
    border-radius: 7px;
    padding: 3px 10px;
    cursor: pointer;
    background: transparent;
    white-space: nowrap;
    transition: border-color 120ms;
  }
  .tb-cmdk:hover { border-color: var(--border-hi2, var(--border-hi)); }
  .tb-cmdk-k { font-weight: 600; color: var(--text-1); }
  .tb-cmdk-label { color: var(--text-mute); }

  .tb-5h {
    display: flex; align-items: center; gap: 7px;
  }
  .tb-5h-label { font-size: 10.5px; color: var(--text-faint); }
  .tb-5h-track {
    display: inline-block;
    width: 40px; height: 4px;
    border-radius: 2px;
    background: var(--bg-4);
    overflow: hidden;
  }
  .tb-5h-fill {
    display: block; height: 100%;
    background: var(--text-1);
    border-radius: 2px;
  }
</style>
