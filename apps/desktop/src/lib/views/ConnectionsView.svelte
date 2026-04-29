<script lang="ts">
  import {
    relativeTime,
    type ClaudeStatus,
    type ConnectionMeta,
    type ConnectionStatus,
    type CursorStatus,
    type JiraStatus,
    type SentryStatus
  } from '../data';
  import { connectionsState, testConnection } from '$lib/state/connections.svelte';
  import {
    connectionEventsState,
    type ConnectionEvent,
    type ConnectionEventSource
  } from '$lib/state/connectionEvents.svelte';

  interface Props {
    sourceConns: ConnectionMeta[];
    agentConns: ConnectionMeta[];
    connectedIds: Set<string>;
    githubStatus: ConnectionStatus;
    jiraStatus: JiraStatus;
    sentryStatus: SentryStatus;
    claudeStatus: ClaudeStatus | null;
    cursorStatus: CursorStatus | null;
    onDisconnectGithub: () => void;
    onDisconnectJira: () => void;
    onDisconnectSentry: () => void;
    onOpenConnectModal: (conn: ConnectionMeta) => void;
  }

  let {
    sourceConns,
    agentConns,
    connectedIds,
    githubStatus,
    jiraStatus,
    sentryStatus,
    claudeStatus,
    cursorStatus,
    onDisconnectGithub,
    onDisconnectJira,
    onDisconnectSentry,
    onOpenConnectModal
  }: Props = $props();

  /* Map source-id → most recent event. The `events` list is already
   *  sorted newest-first, so the first hit per source wins. */
  const lastEventBySource = $derived.by(() => {
    const map: Record<ConnectionEventSource, ConnectionEvent | null> = {
      github: null,
      jira: null,
      sentry: null,
      claude: null,
      cursor: null
    };
    for (const ev of connectionEventsState.events) {
      if (map[ev.source] === null) map[ev.source] = ev;
    }
    return map;
  });

  /* `conn.id` → matching event-log key. The catalogue of source ids is
   *  larger than the set we record events for (Slack / Linear / etc.
   *  are placeholders); this narrows the union safely. */
  const TESTABLE_SOURCES: Record<string, ConnectionEventSource> = {
    github: 'github',
    jira: 'jira',
    sentry: 'sentry',
    claude: 'claude',
    cursor: 'cursor'
  };

  function eventKindLabel(kind: ConnectionEvent['kind']): string {
    switch (kind) {
      case 'connected':
        return 'OK';
      case 'disconnected':
        return 'no token';
      case 'rate_limited':
        return 'rate-limited';
      case 'error':
        return 'error';
    }
  }

  /** Render the GitHub rate-limit reset window as a friendly relative
   *  string. The Unix epoch comes from `x-ratelimit-reset`; if the
   *  window has already elapsed (clock skew / stale data) we say
   *  "now" rather than "−2m". */
  function rateLimitResetLabel(unixSec: number): string {
    const diffMs = unixSec * 1000 - Date.now();
    if (diffMs < 30_000) return 'momentarily';
    const minutes = Math.round(diffMs / 60_000);
    if (minutes < 60) return `${minutes}m`;
    const hours = Math.round(minutes / 60);
    return `${hours}h`;
  }

  /** Tighter quota string for the small per-card test-row. Prefer
   *  `4.8k/5k` over the verbose form. */
  function shortQuota(remaining: number, limit: number): string {
    return `${formatThousands(remaining)}/${formatThousands(limit)}`;
  }

  function formatThousands(n: number): string {
    if (n < 1000) return String(n);
    const k = n / 1000;
    return Number.isInteger(k) ? `${k}k` : `${k.toFixed(1)}k`;
  }
</script>

<section class="connections-view">
  <div class="connections-header">
    <h1 class="view-title">Connections</h1>
    <p class="view-sub">Connect the tools that serve your work. Tokens live in your macOS Keychain.</p>
  </div>
  <div class="connections-body">
    {#each [['sources', sourceConns, 'Work sources'], ['agents', agentConns, 'AI agents']] as [key, list, label] (key)}
      {@const items = list as ConnectionMeta[]}
      <div class="conn-category">
        <div class="conn-category-head">
          <span>{label}</span>
          <span class="conn-category-count mono">{items.filter((c) => connectedIds.has(c.id)).length} of {items.length}</span>
        </div>
        <div class="conn-grid">
          {#each items as conn (conn.id)}
            {@const connected = connectedIds.has(conn.id)}
            {@const testKey = TESTABLE_SOURCES[conn.id] ?? null}
            {@const lastEv = testKey ? lastEventBySource[testKey] : null}
            {@const testing = testKey ? connectionsState.testing[testKey] : false}
            <div class="conn-card" class:connected class:disabled={!conn.implemented}>
              <div class="conn-head">
                <span class="conn-icon {conn.iconClass}" class:conn-icon--svg={!!(conn.iconSvg && !conn.iconImg)} class:conn-icon--img={!!conn.iconImg}>
                  {#if conn.iconImg}
                    <img src={conn.iconImg} alt="" class="conn-icon-img" />
                  {:else if conn.iconSvg}
                    <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">{@html conn.iconSvg}</svg>
                  {:else}
                    {conn.iconLetters}
                  {/if}
                </span>
                <span class="conn-name">{conn.name}</span>
                <span class="conn-status" class:connected>
                  {#if connected}live{:else if !conn.implemented}soon{:else}not connected{/if}
                </span>
              </div>
              <div class="conn-desc">{conn.desc}</div>
              {#if connected && testKey}
                {@const ghRate =
                  testKey === 'github' && githubStatus.kind === 'connected'
                    ? githubStatus.rate_limit
                    : undefined}
                {@const lowQuota =
                  ghRate && ghRate.remaining < ghRate.limit * 0.1}
                <div class="conn-test-row">
                  <button
                    class="conn-test-btn"
                    onclick={() => void testConnection(testKey)}
                    disabled={testing}
                    title="Re-run {conn.name} status check"
                  >
                    {testing ? 'Testing…' : 'Test'}
                  </button>
                  {#if lastEv}
                    <span class="conn-event conn-event--{lastEv.kind}" title={lastEv.message ?? ''}>
                      <span class="conn-event-kind">{eventKindLabel(lastEv.kind)}</span>
                      {#if lastEv.latencyMs !== null}
                        <span class="conn-event-sep">·</span>
                        <span class="mono">{lastEv.latencyMs}ms</span>
                      {/if}
                      <span class="conn-event-sep">·</span>
                      <span>{relativeTime(lastEv.at)}</span>
                    </span>
                  {/if}
                  {#if ghRate}
                    <span
                      class="conn-quota"
                      class:conn-quota--low={lowQuota}
                      title="Rate limit: {ghRate.remaining} of {ghRate.limit} left ({ghRate.resource ?? 'core'} bucket); resets in {rateLimitResetLabel(ghRate.reset)}"
                    >
                      <span class="mono">{shortQuota(ghRate.remaining, ghRate.limit)}</span>
                    </span>
                  {/if}
                </div>
              {/if}
              <div class="conn-footer">
                <span class="conn-type mono">{conn.kind}</span>
                {#if connected && conn.id === 'github'}
                  <button class="conn-btn conn-btn--configure" onclick={onDisconnectGithub}>Disconnect</button>
                {:else if connected && conn.id === 'jira'}
                  <button class="conn-btn conn-btn--configure" onclick={onDisconnectJira}>Disconnect</button>
                {:else if connected && conn.id === 'sentry'}
                  <button class="conn-btn conn-btn--configure" onclick={onDisconnectSentry}>Disconnect</button>
                {:else if connected && conn.id === 'claude'}
                  <button class="conn-btn conn-btn--configure" onclick={() => onOpenConnectModal(conn)}>Manage</button>
                {:else if connected && conn.id === 'cursor'}
                  <button class="conn-btn conn-btn--configure" onclick={() => onOpenConnectModal(conn)}>Manage</button>
                {:else if conn.implemented}
                  <button class="conn-btn conn-btn--connect" onclick={() => onOpenConnectModal(conn)}>Connect</button>
                {:else}
                  <span class="conn-soon">coming soon</span>
                {/if}
              </div>
            </div>
          {/each}
        </div>
      </div>
    {/each}
    {#if githubStatus.kind === 'connected'}
      <div class="you-are">
        GitHub as
        <img src={githubStatus.user.avatar_url} alt="" class="you-avatar" />
        <span class="mono">@{githubStatus.user.login}</span>
        {#if githubStatus.user.name}<span class="you-name">· {githubStatus.user.name}</span>{/if}
        {#if githubStatus.rate_limit}
          {@const rl = githubStatus.rate_limit}
          {@const usedPct = Math.round((rl.used / rl.limit) * 100)}
          {@const lowQuota = rl.remaining < rl.limit * 0.1}
          <span class="you-name you-quota" class:you-quota--low={lowQuota}>
            · API <span class="mono">{rl.remaining}/{rl.limit}</span>
            <span class="you-quota-pct">({usedPct}% used)</span>
            · resets in <span class="mono">{rateLimitResetLabel(rl.reset)}</span>
          </span>
        {/if}
      </div>
    {/if}
    {#if jiraStatus.kind === 'connected'}
      <div class="you-are">
        Jira as
        <img src={jiraStatus.user.avatar_url} alt="" class="you-avatar" />
        <span>{jiraStatus.user.display_name}</span>
        <span class="you-name mono">· {jiraStatus.user.workspace}</span>
      </div>
    {/if}
    {#if sentryStatus.kind === 'connected'}
      <div class="you-are">
        Sentry as
        <span>{sentryStatus.user.name ?? sentryStatus.user.username ?? sentryStatus.user.email ?? sentryStatus.user.id}</span>
        <span class="you-name mono">· {sentryStatus.user.organization_slug} on {sentryStatus.user.host.replace(/^https?:\/\//, '')}</span>
      </div>
    {/if}
    {#if claudeStatus?.ready}
      <div class="you-are">
        Claude Code ready
        {#if claudeStatus.version}<span class="you-name mono">· {claudeStatus.version}</span>{/if}
        {#if claudeStatus.has_api_key_env}
          <span class="you-name">· via API key</span>
        {:else if claudeStatus.has_config_dir}
          <span class="you-name">· via <code class="mono">claude login</code></span>
        {/if}
      </div>
    {/if}
    {#if cursorStatus?.ready}
      <div class="you-are">
        Cursor ready
        {#if cursorStatus.version}<span class="you-name mono">· {cursorStatus.version}</span>{/if}
      </div>
    {/if}
  </div>
</section>

<style>
  .connections-view { overflow-y: auto; flex: 1; }
  .connections-header { padding: 48px 56px 20px; text-align: center; }
  .view-title { font-size: 28px; font-weight: 600; letter-spacing: -0.025em; color: var(--text-0); margin-bottom: 10px; }
  .view-sub { font-size: 14px; color: var(--text-2); max-width: 520px; margin: 0 auto; line-height: 1.5; }
  .connections-body { padding: 0 56px 100px; max-width: 980px; margin: 0 auto; width: 100%; }

  .conn-category { margin-top: 36px; }
  .conn-category-head {
    display: flex; align-items: center; gap: 12px; margin-bottom: 16px;
    font-size: 11px; font-weight: 600; letter-spacing: 0.08em;
    color: var(--text-2); text-transform: uppercase;
  }
  .conn-category-head::after { content: ''; flex: 1; height: 1px; background: var(--border-neutral); }
  .conn-category-count { font-family: 'JetBrains Mono', monospace; color: var(--text-mute); font-size: 10.5px; letter-spacing: 0; }

  .conn-grid { display: grid; grid-template-columns: repeat(3, 1fr); gap: 10px; }
  .conn-card {
    padding: 18px 18px 14px;
    background: var(--bg-1); border: 1px solid var(--border-neutral);
    border-radius: 11px;
    display: flex; flex-direction: column; gap: 12px;
    transition: all 180ms;
  }
  .conn-card:hover { border-color: var(--border-neutral-hi); background: var(--bg-2); transform: translateY(-2px); box-shadow: 0 8px 24px rgba(0, 0, 0, 0.3); }
  .conn-card.connected { border-color: rgba(16, 185, 129, 0.18); }
  .conn-card.connected:hover { border-color: rgba(16, 185, 129, 0.32); box-shadow: 0 8px 24px rgba(0, 0, 0, 0.3), 0 0 20px rgba(16, 185, 129, 0.05); }
  .conn-card.disabled { opacity: 0.55; }
  .conn-card.disabled:hover { transform: none; }

  .conn-head { display: flex; align-items: center; gap: 12px; }
  .conn-icon {
    width: 36px; height: 36px; border-radius: 9px;
    display: inline-flex; align-items: center; justify-content: center;
    font-size: 13px; font-weight: 700; letter-spacing: -0.02em; flex-shrink: 0;
  }
  .conn-icon--svg svg {
    width: 20px; height: 20px;
    color: currentColor;
    display: block;
  }

  .conn-name { font-size: 14px; font-weight: 600; color: var(--text-0); }
  .conn-status { font-size: 10.5px; color: var(--text-mute); margin-left: auto; font-weight: 500; }
  .conn-status.connected { color: var(--accent-bright); }
  .conn-status.connected::before {
    content: ''; display: inline-block;
    width: 5px; height: 5px; background: var(--accent-bright); border-radius: 50%;
    box-shadow: 0 0 6px var(--accent-glow); margin-right: 6px; vertical-align: middle;
  }
  .conn-desc { font-size: 12.5px; color: var(--text-1); line-height: 1.5; min-height: 36px; }
  .conn-footer { display: flex; align-items: center; justify-content: space-between; gap: 10px; margin-top: auto; }
  .conn-type { font-size: 10.5px; color: var(--text-mute); }
  .conn-soon { font-size: 10.5px; color: var(--text-mute); font-style: italic; }
  .conn-btn { padding: 6px 14px; border-radius: 6px; font-size: 11.5px; font-weight: 500; transition: all 140ms; background: none; border: none; cursor: pointer; }
  .conn-btn--connect {
    color: var(--accent-fg);
    background: linear-gradient(135deg, #34d399, #10b981);
    box-shadow: 0 2px 8px rgba(16, 185, 129, 0.2), inset 0 1px 0 rgba(255, 255, 255, 0.2);
    font-weight: 600;
  }
  .conn-btn--connect:hover { box-shadow: 0 4px 14px rgba(16, 185, 129, 0.3), inset 0 1px 0 rgba(255, 255, 255, 0.25); transform: translateY(-1px); }
  .conn-btn--configure { background: transparent; color: var(--text-1); border: 1px solid var(--border-neutral-hi); }
  .conn-btn--configure:hover { background: var(--bg-3); color: var(--text-0); border-color: var(--border-hi); }

  .conn-test-row {
    display: flex; align-items: center; gap: 10px;
    padding: 8px 0 0;
    border-top: 1px dashed var(--border-neutral);
    font-size: 11px;
  }
  .conn-test-btn {
    padding: 3px 9px; border-radius: 5px;
    background: transparent; color: var(--text-1);
    border: 1px solid var(--border-neutral-hi);
    font-size: 11px; font-weight: 500; cursor: pointer;
    transition: all 140ms;
  }
  .conn-test-btn:hover:not(:disabled) { background: var(--bg-3); color: var(--text-0); border-color: var(--border-hi); }
  .conn-test-btn:disabled { opacity: 0.55; cursor: progress; }

  .conn-event {
    display: inline-flex; align-items: center; gap: 4px;
    color: var(--text-mute);
    font-size: 11px;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .conn-event-kind { font-weight: 600; }
  .conn-event-sep { opacity: 0.6; }
  .conn-event--connected .conn-event-kind { color: var(--accent-bright); }
  .conn-event--disconnected .conn-event-kind { color: var(--text-2); }
  .conn-event--rate_limited .conn-event-kind { color: #f59e0b; }
  .conn-event--error .conn-event-kind { color: #f87171; }

  /* GitHub rate-limit chip in the per-card test-row. Pushed to the
     right with margin-left:auto so the most-relevant info (test
     button + last event) stays anchored on the left. Turns amber
     once <10% of the window is left so users notice before the
     hard 429. */
  .conn-quota {
    margin-left: auto;
    padding: 2px 7px;
    border-radius: 999px;
    background: var(--bg-2);
    border: 1px solid var(--border-neutral);
    color: var(--text-2);
    font-size: 10.5px;
    cursor: help;
  }
  .conn-quota--low {
    background: rgba(245, 158, 11, 0.10);
    border-color: rgba(245, 158, 11, 0.45);
    color: #f59e0b;
  }

  .you-quota { display: inline-flex; align-items: center; gap: 4px; flex-wrap: wrap; }
  .you-quota--low { color: #f59e0b; }
  .you-quota-pct { color: var(--text-mute); font-size: 11.5px; }

  .you-are {
    margin-top: 28px; padding: 14px 16px;
    background: var(--bg-1); border: 1px solid rgba(16, 185, 129, 0.16);
    border-radius: 10px;
    display: flex; align-items: center; gap: 10px;
    font-size: 12.5px; color: var(--text-1);
  }
  .you-avatar { width: 22px; height: 22px; border-radius: 50%; }
  .you-name { color: var(--text-2); }
</style>
