<script lang="ts">
  import Sigil from '$lib/components/ui/Sigil.svelte';
  import type {
    ClaudeStatus,
    ConnectionStatus,
    CursorStatus,
    JiraStatus,
    SentryStatus
  } from '$lib/data';

  type View = 'workbench' | 'githubTab' | 'jiraTab' | 'sentryTab' | 'rules' | 'connections' | 'settings';

  interface Props {
    view: View;
    inboxCount: number;
    anythingConnected: boolean;
    statusLoading: boolean;
    /** True when the boot retry/backoff loop is mid-attempt for any
     *  source. Renders a pulsing dot in place of the warning dot so
     *  the user sees "we're still trying" instead of a flat
     *  "nothing's connected" — meaningful when the only failure was
     *  a transient network blip on launch. */
    anyRetrying?: boolean;
    githubStatus: ConnectionStatus;
    /** Identity inputs for the bottom-rail badge popover. The popover
     *  surfaces "whose creds are these?" per source — important when
     *  the user has more than one Atlassian / GitHub identity in
     *  flight and needs to confirm the right one is loaded. Agents
     *  (Claude / Cursor) don't expose a uniform identity API so they
     *  show ready/version + a soft "unknown" label per
     *  `docs/CONNECTIONS.md §11.4`. */
    jiraStatus?: JiraStatus;
    sentryStatus?: SentryStatus;
    claudeStatus?: ClaudeStatus | null;
    cursorStatus?: CursorStatus | null;
  }

  let {
    view = $bindable(),
    inboxCount,
    anythingConnected,
    statusLoading,
    anyRetrying = false,
    githubStatus,
    jiraStatus,
    sentryStatus,
    claudeStatus,
    cursorStatus
  }: Props = $props();

  /* Compact identity rows for the bottom-rail badge popover. Built
   * lazily ($derived) so the popover stays in sync with status
   * changes (token rotation, disconnect-then-reconnect-as-X). Disconnected
   * sources show "—" in their value column rather than being hidden
   * — the user often opens the popover specifically to confirm
   * "nothing is logged in here yet". */
  interface IdentityRow {
    label: string;
    value: string;
    /** Optional second-line detail (workspace, host, …). */
    sub?: string;
    connected: boolean;
  }

  const identityRows = $derived.by((): IdentityRow[] => {
    const rows: IdentityRow[] = [];
    if (githubStatus.kind === 'connected') {
      rows.push({
        label: 'GitHub',
        value: `@${githubStatus.user.login}`,
        sub: githubStatus.user.name ?? undefined,
        connected: true
      });
    } else {
      rows.push({ label: 'GitHub', value: '—', connected: false });
    }
    if (jiraStatus?.kind === 'connected') {
      const u = jiraStatus.user;
      rows.push({
        label: 'Jira',
        value: u.display_name,
        sub: `${u.workspace}${u.email_address ? ' · ' + u.email_address : ''}`,
        connected: true
      });
    } else {
      rows.push({ label: 'Jira', value: '—', connected: false });
    }
    if (sentryStatus?.kind === 'connected') {
      const u = sentryStatus.user;
      rows.push({
        label: 'Sentry',
        value: u.organization_slug,
        sub: u.host.replace(/^https?:\/\//, ''),
        connected: true
      });
    } else {
      rows.push({ label: 'Sentry', value: '—', connected: false });
    }
    rows.push(agentRow('Claude', claudeStatus));
    rows.push(agentRow('Cursor', cursorStatus));
    return rows;
  });

  function agentRow(
    label: string,
    s: ClaudeStatus | CursorStatus | null | undefined
  ): IdentityRow {
    if (!s || !s.ready) {
      return { label, value: '—', connected: false };
    }
    /* The CLIs don't expose a uniform "logged in as" affordance —
     * mark identity as unknown but show the version so it's clear
     * the binary is alive. */
    return {
      label,
      value: 'unknown',
      sub: s.version ? `v${s.version}` : undefined,
      connected: true
    };
  }
</script>

<aside class="rail">
  <div class="rail-top">
    <div class="rail-sigil"><Sigil size={36} /></div>
    <button class="rail-btn" class:active={view === 'workbench'} data-tooltip="Workbench" onclick={() => (view = 'workbench')} aria-label="Workbench">
      <svg class="i" viewBox="0 0 24 24"><path d="M4 6h16M4 12h10M4 18h16" /></svg>
      {#if inboxCount > 0}<span class="rail-badge">{inboxCount}</span>{/if}
    </button>
    <button class="rail-btn" class:active={view === 'githubTab'} data-tooltip="GitHub" onclick={() => (view = 'githubTab')} aria-label="GitHub">
      <svg class="i" viewBox="0 0 24 24"><path d="M3 7v11a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2V9a2 2 0 0 0-2-2h-7L10 5H5a2 2 0 0 0-2 2z" /></svg>
    </button>
    <button class="rail-btn" class:active={view === 'jiraTab'} data-tooltip="Jira" onclick={() => (view = 'jiraTab')} aria-label="Jira">
      <svg class="i" viewBox="0 0 24 24"><rect x="4" y="3" width="16" height="18" rx="2" /><path d="M9 3v2h6V3" /><path d="M8 11h8M8 15h6" /></svg>
    </button>
    <button class="rail-btn" class:active={view === 'sentryTab'} data-tooltip="Sentry" onclick={() => (view = 'sentryTab')} aria-label="Sentry">
      <svg class="i" viewBox="0 0 24 24"><circle cx="12" cy="12" r="9"/><path d="M12 8v4M12 16h.01"/></svg>
    </button>
    <button class="rail-btn" class:active={view === 'rules'} data-tooltip="Rules" onclick={() => (view = 'rules')} aria-label="Rules">
      <svg class="i" viewBox="0 0 24 24"><path d="M14 3v4a1 1 0 0 0 1 1h4M17 21H7a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h7l5 5v11a2 2 0 0 1-2 2z" /><path d="M9 12h6M9 16h6" /></svg>
    </button>
    <div class="rail-sep"></div>
    <button class="rail-btn" class:active={view === 'connections'} data-tooltip={anyRetrying ? 'Connections — retrying…' : 'Connections'} onclick={() => (view = 'connections')} aria-label="Connections">
      <svg class="i" viewBox="0 0 24 24"><path d="M10 13a5 5 0 0 0 7 0l3-3a5 5 0 0 0-7-7l-1 1" /><path d="M14 11a5 5 0 0 0-7 0l-3 3a5 5 0 0 0 7 7l1-1" /></svg>
      {#if anyRetrying}
        <span class="rail-dot rail-dot--retrying" aria-label="retrying"></span>
      {:else if !anythingConnected && !statusLoading}
        <span class="rail-dot"></span>
      {/if}
    </button>
  </div>
  <div class="rail-bottom">
    <button class="rail-btn" class:active={view === 'settings'} data-tooltip="Settings" onclick={() => (view = 'settings')} aria-label="Settings">
      <svg class="i" viewBox="0 0 24 24"><circle cx="12" cy="12" r="3" /><path d="M12 1v4M12 19v4M4.22 4.22l2.83 2.83M16.95 16.95l2.83 2.83M1 12h4M19 12h4M4.22 19.78l2.83-2.83M16.95 7.05l2.83-2.83" /></svg>
    </button>
    <button class="rail-avatar" type="button" aria-label="Workspace identity" tabindex="0">
      {#if githubStatus.kind === 'connected'}
        <img src={githubStatus.user.avatar_url} alt={githubStatus.user.login} />
      {:else}—{/if}
      <div class="rail-identity" role="dialog" aria-label="Connected identities">
        <div class="rail-identity-head">Logged in as</div>
        <ul class="rail-identity-list">
          {#each identityRows as row (row.label)}
            <li class="rail-identity-row" class:connected={row.connected}>
              <span class="rail-identity-label">{row.label}</span>
              <span class="rail-identity-value mono" title={row.sub ?? ''}>
                {row.value}
                {#if row.sub}
                  <span class="rail-identity-sub">{row.sub}</span>
                {/if}
              </span>
            </li>
          {/each}
        </ul>
      </div>
    </button>
  </div>
</aside>

<style>
  .rail {
    display: flex; flex-direction: column; align-items: center;
    padding: 14px 0; gap: 6px;
    border-right: 1px solid var(--border-neutral);
    background: var(--bg-1);
    backdrop-filter: blur(8px);
    /* Guarantee the rail renders above workbench tabs / pill hover menus
       so its badges and active-view indicator stay readable even when a
       neighbouring panel has its own stacking context. */
    position: relative;
    z-index: 5;
  }
  .rail-top, .rail-bottom { display: flex; flex-direction: column; align-items: center; gap: 6px; }
  .rail-bottom { margin-top: auto; }
  .rail-sigil { margin-bottom: 14px; }
  .rail-btn {
    width: 38px; height: 38px; border-radius: 9px;
    display: inline-flex; align-items: center; justify-content: center;
    color: var(--text-2); position: relative; transition: all 140ms ease;
    background: none; border: none; cursor: pointer; padding: 0;
  }
  .rail-btn:hover { color: var(--text-0); background: var(--bg-1); }
  .rail-btn.active { color: var(--accent-bright); background: var(--bg-2); box-shadow: inset 0 0 0 1px var(--border-hi); }
  .rail-btn.active::before {
    content: ''; position: absolute; left: -14px; top: 10px; bottom: 10px; width: 2px;
    background: var(--accent); border-radius: 2px; box-shadow: 0 0 8px var(--accent-glow);
  }
  .rail-badge {
    position: absolute; top: 2px; right: 2px;
    min-width: 14px; height: 14px; padding: 0 3px; border-radius: 7px;
    background: var(--accent); color: var(--accent-fg);
    font-size: 9.5px; font-weight: 700;
    display: inline-flex; align-items: center; justify-content: center;
    box-shadow: 0 0 0 2px var(--bg-0), 0 0 8px var(--accent-glow);
  }
  .rail-dot {
    position: absolute; top: 6px; right: 6px; width: 7px; height: 7px;
    border-radius: 50%; background: var(--warning);
    box-shadow: 0 0 0 2px var(--bg-0), 0 0 8px rgba(245, 158, 11, 0.5);
  }
  /* Retrying state: same position as `rail-dot` but a softer accent
     hue + a slow pulse so the user reads it as "ongoing work" rather
     than "permanent fault". Honours `prefers-reduced-motion` per the
     1.0 a11y bar. */
  .rail-dot--retrying {
    background: var(--accent);
    box-shadow: 0 0 0 2px var(--bg-0), 0 0 8px var(--accent-glow);
    animation: rail-dot-pulse 1.4s ease-in-out infinite;
  }
  @keyframes rail-dot-pulse {
    0%, 100% { opacity: 0.55; }
    50%      { opacity: 1; }
  }
  @media (prefers-reduced-motion: reduce) {
    .rail-dot--retrying { animation: none; opacity: 0.85; }
  }
  .rail-avatar {
    width: 30px; height: 30px; border-radius: 50%;
    background: linear-gradient(135deg, #3b82f6, #10b981);
    display: inline-flex; align-items: center; justify-content: center;
    color: #fff; font-weight: 600; font-size: 11px;
    box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.15);
    /* `overflow: hidden` clips the GitHub avatar into a circle, but
       it also clips the identity popover; let the popover sit
       outside via a relative wrapper on the button while keeping the
       avatar image clipped via its own border-radius. */
    position: relative;
    border: none; padding: 0; cursor: pointer;
  }
  .rail-avatar > img {
    width: 100%; height: 100%; object-fit: cover; border-radius: 50%;
    /* Clip the image so it stays circular even though the parent
       button no longer has `overflow: hidden`. */
  }
  /* Identity popover. Mirrors the data-tooltip styling used elsewhere
     in the rail but positioned to the right of the avatar (rail is on
     the left edge of the screen). Hidden by default; surfaced on
     hover OR keyboard focus so it's reachable without a pointer. */
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
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.35);
    opacity: 0;
    transform: translateY(4px) scale(0.98);
    transition: opacity 120ms ease, transform 120ms ease;
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
  @media (prefers-reduced-motion: reduce) {
    .rail-identity { transition: opacity 80ms linear; transform: none; }
    .rail-avatar:hover .rail-identity,
    .rail-avatar:focus-visible .rail-identity { transform: none; }
  }
  .rail-identity-head {
    font-size: 10.5px; font-weight: 600; letter-spacing: 0.04em;
    color: var(--text-mute); text-transform: uppercase; margin-bottom: 6px;
  }
  .rail-identity-list { list-style: none; padding: 0; margin: 0; display: flex; flex-direction: column; gap: 4px; }
  .rail-identity-row {
    display: flex; align-items: baseline; gap: 8px;
    color: var(--text-2);
  }
  .rail-identity-row.connected { color: var(--text-0); }
  .rail-identity-label { min-width: 56px; color: var(--text-mute); font-size: 10.5px; }
  .rail-identity-value {
    flex: 1 1 auto; font-size: 11.5px; word-break: break-word;
    display: inline-flex; flex-direction: column; gap: 1px;
  }
  .rail-identity-sub { color: var(--text-mute); font-size: 10.5px; }
  .rail-sep { width: 20px; height: 1px; background: var(--border-neutral); margin: 4px 0; }
  .rail-btn[data-tooltip]:hover::after {
    content: attr(data-tooltip);
    position: absolute; left: 46px; top: 50%; transform: translateY(-50%);
    padding: 4px 10px; background: var(--bg-3); border: 1px solid var(--border-neutral-hi);
    border-radius: 6px; font-size: 11.5px; color: var(--text-0);
    white-space: nowrap; pointer-events: none; z-index: 10;
  }
</style>
