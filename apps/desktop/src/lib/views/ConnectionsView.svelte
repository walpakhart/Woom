<script lang="ts">
  import { openUrl } from '@tauri-apps/plugin-opener';
  import type {
    ClaudeStatus,
    ConnectionMeta,
    ConnectionStatus,
    CursorStatus,
    JiraStatus
  } from '../data';

  interface Props {
    sourceConns: ConnectionMeta[];
    agentConns: ConnectionMeta[];
    connectedIds: Set<string>;
    githubStatus: ConnectionStatus;
    jiraStatus: JiraStatus;
    claudeStatus: ClaudeStatus | null;
    cursorStatus: CursorStatus | null;
    onDisconnectGithub: () => void;
    onDisconnectJira: () => void;
    onOpenConnectModal: (conn: ConnectionMeta) => void;
  }

  let {
    sourceConns,
    agentConns,
    connectedIds,
    githubStatus,
    jiraStatus,
    claudeStatus,
    cursorStatus,
    onDisconnectGithub,
    onDisconnectJira,
    onOpenConnectModal
  }: Props = $props();
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
            <div class="conn-card" class:connected class:disabled={!conn.implemented}>
              <div class="conn-head">
                <span class="conn-icon {conn.iconClass}" class:conn-icon--svg={!!conn.iconSvg}>
                  {#if conn.iconSvg}
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
              <div class="conn-footer">
                <span class="conn-type mono">{conn.kind}</span>
                {#if connected && conn.id === 'github'}
                  <button class="conn-btn conn-btn--configure" onclick={onDisconnectGithub}>Disconnect</button>
                {:else if connected && conn.id === 'jira'}
                  <button class="conn-btn conn-btn--configure" onclick={onDisconnectJira}>Disconnect</button>
                {:else if connected && conn.id === 'claude'}
                  <button class="conn-btn conn-btn--configure" onclick={() => onOpenConnectModal(conn)}>Manage</button>
                {:else if connected && conn.id === 'cursor'}
                  <button class="conn-btn conn-btn--configure" onclick={() => void openUrl('https://cursor.com/docs/cli')}>Docs</button>
                {:else if conn.id === 'cursor'}
                  <button class="conn-btn conn-btn--connect" onclick={() => void openUrl('https://cursor.com/docs/cli/installation')}>Install</button>
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
    color: #0a111e;
    background: linear-gradient(135deg, #34d399, #10b981);
    box-shadow: 0 2px 8px rgba(16, 185, 129, 0.2), inset 0 1px 0 rgba(255, 255, 255, 0.2);
    font-weight: 600;
  }
  .conn-btn--connect:hover { box-shadow: 0 4px 14px rgba(16, 185, 129, 0.3), inset 0 1px 0 rgba(255, 255, 255, 0.25); transform: translateY(-1px); }
  .conn-btn--configure { background: transparent; color: var(--text-1); border: 1px solid var(--border-neutral-hi); }
  .conn-btn--configure:hover { background: var(--bg-3); color: var(--text-0); border-color: var(--border-hi); }

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
