<script lang="ts">
  /* Avatar at the bottom of the rail + identity popover under it. The
     popover is CSS-only — hover/focus on `.rail-avatar` reveals the
     identity list (`.rail-identity`). Identity rows render the
     connected user for each source (or "—" when disconnected). */
  import type {
    ClaudeStatus,
    ConnectionStatus,
    JiraStatus,
    SentryStatus
  } from '$lib/data';

  interface Props {
    githubStatus: ConnectionStatus;
    jiraStatus?: JiraStatus;
    sentryStatus?: SentryStatus;
    claudeStatus?: ClaudeStatus | null;
    /** 'rail' — 30px, popover opens to the right (rail foot).
     *  'titlebar' — 22px, popover drops below, right-aligned. */
    placement?: 'rail' | 'titlebar';
  }
  let { placement = 'rail', ...p }: Props = $props();

  interface IdentityRow {
    label: string;
    value: string;
    sub?: string;
    connected: boolean;
  }

  function agentRow(
    label: string,
    s: ClaudeStatus | null | undefined
  ): IdentityRow {
    if (!s || !s.ready) {
      return { label, value: '—', connected: false };
    }
    return {
      label,
      value: 'unknown',
      sub: s.version ? `v${s.version}` : undefined,
      connected: true
    };
  }

  const identityRows = $derived.by((): IdentityRow[] => {
    const rows: IdentityRow[] = [];
    if (p.githubStatus.kind === 'connected') {
      rows.push({
        label: 'GitHub',
        value: `@${p.githubStatus.user.login}`,
        sub: p.githubStatus.user.name ?? undefined,
        connected: true
      });
    } else {
      rows.push({ label: 'GitHub', value: '—', connected: false });
    }
    if (p.jiraStatus?.kind === 'connected') {
      const u = p.jiraStatus.user;
      rows.push({
        label: 'Jira',
        value: u.display_name,
        sub: `${u.workspace}${u.email_address ? ' · ' + u.email_address : ''}`,
        connected: true
      });
    } else {
      rows.push({ label: 'Jira', value: '—', connected: false });
    }
    if (p.sentryStatus?.kind === 'connected') {
      const u = p.sentryStatus.user;
      rows.push({
        label: 'Sentry',
        value: u.organization_slug,
        sub: u.host.replace(/^https?:\/\//, ''),
        connected: true
      });
    } else {
      rows.push({ label: 'Sentry', value: '—', connected: false });
    }
    rows.push(agentRow('Claude', p.claudeStatus));
    return rows;
  });
</script>

<button class="rail-avatar" class:tb={placement === 'titlebar'} type="button" aria-label="Workspace identity" tabindex="0">
  {#if p.githubStatus.kind === 'connected'}
    <img src={p.githubStatus.user.avatar_url} alt={p.githubStatus.user.login} />
  {:else}—{/if}
  <div class="rail-identity" role="dialog" aria-label="Connected identities">
    <div class="rail-identity-head">Logged in as</div>
    <ul class="rail-identity-list">
      {#each identityRows as row, idx (row.label + '|' + idx)}
        <li class="rail-identity-row" class:connected={row.connected}>
          <span class="rail-identity-label">{row.label}</span>
          <span class="rail-identity-value mono" title={row.sub ?? ''}>
            {row.value}
            {#if row.sub}<span class="rail-identity-sub">{row.sub}</span>{/if}
          </span>
        </li>
      {/each}
    </ul>
  </div>
</button>

<style>
  .rail-avatar {
    width: 30px; height: 30px;
    margin-top: 4px;
    border-radius: 50%;
    background: linear-gradient(135deg, #4F8EFF, #7DC9B0);
    display: inline-flex; align-items: center; justify-content: center;
    color: #fff; font-weight: 600; font-size: 11px;
    box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.15);
    position: relative;
    border: 0; padding: 0;
    cursor: pointer;
  }
  .rail-avatar > img {
    width: 100%; height: 100%; object-fit: cover; border-radius: 50%;
  }
  /* Titlebar placement — smaller, popover drops below right-aligned. */
  .rail-avatar.tb { width: 22px; height: 22px; margin-top: 0; font-size: 9px; }

  .rail-identity {
    position: absolute;
    left: calc(100% + 12px); bottom: 0;
    min-width: 220px;
    padding: 10px 12px;
    background: var(--bg-3);
    border: 1px solid var(--border-neutral-hi);
    border-radius: 10px;
    color: var(--text-0);
    font-size: 11.5px;
    box-shadow: var(--shadow-3);
    opacity: 0;
    transform: translateY(4px) scale(0.98);
    transition:
      opacity var(--dur-quick) var(--ease-out),
      transform var(--dur-quick) var(--ease-out);
    pointer-events: none;
    z-index: 20;
    text-align: left;
  }
  .rail-avatar:hover .rail-identity,
  .rail-avatar:focus-visible .rail-identity {
    opacity: 1;
    transform: translateY(0) scale(1);
    pointer-events: auto;
  }
  /* Titlebar: drop below the avatar, aligned to the window's right edge. */
  .rail-avatar.tb .rail-identity {
    left: auto; right: 0; bottom: auto;
    top: calc(100% + 8px);
  }
  @media (prefers-reduced-motion: reduce) {
    .rail-identity { transition: opacity 80ms var(--ease-out); transform: none; }
    .rail-avatar:hover .rail-identity,
    .rail-avatar:focus-visible .rail-identity { transform: none; }
  }
  .rail-identity-head {
    font-size: 10.5px; font-weight: 600;
    letter-spacing: 0.04em; text-transform: uppercase;
    color: var(--text-mute);
    margin-bottom: 6px;
  }
  .rail-identity-list { list-style: none; padding: 0; margin: 0; display: flex; flex-direction: column; gap: 4px; }
  .rail-identity-row {
    display: flex; align-items: baseline; gap: 8px;
    color: var(--text-2);
  }
  .rail-identity-row.connected { color: var(--text-0); }
  .rail-identity-label { min-width: 56px; color: var(--text-mute); font-size: 10.5px; }
  .rail-identity-value {
    flex: 1 1 auto;
    font-size: 11.5px;
    word-break: break-word;
    display: inline-flex; flex-direction: column; gap: 1px;
  }
  .rail-identity-sub { color: var(--text-mute); font-size: 10.5px; }
</style>
