<script lang="ts">
  import RailAppButton from '$lib/components/ui/RailAppButton.svelte';
  /* Reuse the brand SVGs that ConnectionsView renders so the rail's
     vocabulary matches Connections — same Octocat for GitHub, same
     Atlassian Jira mark, same Sentry crown. Claude / Cursor still
     use the curated PNGs (`/brand-claude.png` / `/brand-cursor.png`)
     because their official marks are rich gradients that don't
     distill cleanly to a single mono <path>. */
  import { SVG_GITHUB, SVG_JIRA, SVG_SENTRY } from '$lib/data';
  import type {
    ClaudeStatus,
    ConnectionStatus,
    CursorStatus,
    JiraStatus,
    SentryStatus
  } from '$lib/data';

  type View =
    | 'home'
    | 'jiraApp'
    | 'githubApp'
    | 'sentryApp'
    | 'claudeApp'
    | 'cursorApp'
    | 'editorApp'
    | 'canvasApp'
    | 'terminalApp'
    | 'rules'
    | 'connections'
    | 'settings';

  interface Props {
    view: View;
    inboxCount: number;
    anythingConnected: boolean;
    statusLoading: boolean;
    /** Boot retry/backoff loop is mid-attempt for any source. Renders
     *  a pulsing dot in place of the warning dot so the user reads
     *  "we're still trying" instead of a flat "nothing's connected". */
    anyRetrying?: boolean;
    githubStatus: ConnectionStatus;
    /** Identity inputs for the bottom-rail badge popover. */
    jiraStatus?: JiraStatus;
    sentryStatus?: SentryStatus;
    claudeStatus?: ClaudeStatus | null;
    cursorStatus?: CursorStatus | null;
    /** Drop landed on the Claude / Cursor rail icon. Parent owns the
     *  routing — switches the view and forwards the DragEvent to the
     *  same `onAgentDrop` pipeline a column-level drop would hit. The
     *  Rail just decides "this is a payload we want" via
     *  `hasDropPayload`. */
    onAgentDrop?: (kind: 'claude' | 'cursor', e: DragEvent) => void;
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
    cursorStatus,
    onAgentDrop
  }: Props = $props();

  /** Highlight the Claude / Cursor rail button while a payload is
   *  dragging over it. Cleared on `dragleave` (when the cursor truly
   *  leaves the button) and on `drop`. */
  let dropOverKind = $state<'claude' | 'cursor' | null>(null);

  /** WebKit hides custom `application/x-woom-*` mimes during dragover
   *  (only standard mimes are exposed until `drop`), so we accept
   *  anything that looks like a sane payload — files, uri-list, plain
   *  text — and let the column-level drop decide if it's actually
   *  routable. */
  function hasDropPayload(e: DragEvent): boolean {
    const types = e.dataTransfer?.types;
    if (!types) return false;
    return (
      types.indexOf('Files') !== -1 ||
      types.indexOf('text/uri-list') !== -1 ||
      types.indexOf('text/plain') !== -1 ||
      types.indexOf('application/x-woom-file') !== -1 ||
      types.indexOf('application/x-woom-jira') !== -1 ||
      types.indexOf('application/x-woom-github') !== -1 ||
      types.indexOf('application/x-woom-sentry') !== -1
    );
  }

  function railDragEnter(kind: 'claude' | 'cursor', e: DragEvent) {
    if (!hasDropPayload(e)) return;
    e.preventDefault();
    dropOverKind = kind;
  }
  function railDragOver(kind: 'claude' | 'cursor', e: DragEvent) {
    if (!hasDropPayload(e)) return;
    e.preventDefault();
    if (e.dataTransfer) e.dataTransfer.dropEffect = 'copy';
    dropOverKind = kind;
  }
  function railDragLeave() {
    /* dragleave fires when the cursor enters child elements too;
       relatedTarget can be null. We rely on the next dragenter to
       re-set the highlight, so a brief flicker on inner elements is
       acceptable. The `drop` handler does the final cleanup. */
    dropOverKind = null;
  }
  function railDrop(kind: 'claude' | 'cursor', e: DragEvent) {
    dropOverKind = null;
    if (!onAgentDrop) return;
    e.preventDefault();
    view = kind === 'claude' ? 'claudeApp' : 'cursorApp';
    onAgentDrop(kind, e);
  }

  interface IdentityRow {
    label: string;
    value: string;
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
    return {
      label,
      value: 'unknown',
      sub: s.version ? `v${s.version}` : undefined,
      connected: true
    };
  }
</script>

<aside class="rail">
  <!-- v8 brand mark = "go home" affordance. Click takes the user to
       the dashboard view (HomeApp.svelte) so the W is the always-
       reachable anchor of the workspace, mirroring how the system
       menu / dock icon traditionally navigates back to the app's
       summary page. -->
  <button
    class="rail-sigil"
    class:active={view === 'home'}
    onclick={() => (view = 'home')}
    aria-label="Home"
    data-tooltip="Home · ⌘0"
  >
    <img src="/woom-logo.svg" alt="Woom" />
  </button>

  <!-- Source solos -->
  <button
    class="rail-btn"
    class:active={view === 'jiraApp'}
    style="--rail-tone: var(--src-jira); --rail-glow: rgba(79,142,255,0.40);"
    data-tooltip="Jira · ⌘1"
    onclick={() => (view = 'jiraApp')}
    aria-label="Jira"
  >
    <svg viewBox="0 0 24 24" fill="currentColor" stroke="none" aria-hidden="true">{@html SVG_JIRA}</svg>
    {#if inboxCount > 0}
      <span class="rail-badge">{inboxCount > 99 ? '99+' : inboxCount}</span>
    {/if}
  </button>

  <button
    class="rail-btn"
    class:active={view === 'githubApp'}
    style="--rail-tone: var(--src-github); --rail-glow: rgba(181,132,255,0.40);"
    data-tooltip="GitHub · ⌘2"
    onclick={() => (view = 'githubApp')}
    aria-label="GitHub"
  >
    <svg viewBox="0 0 24 24" fill="currentColor" stroke="none" aria-hidden="true">{@html SVG_GITHUB}</svg>
  </button>

  <button
    class="rail-btn"
    class:active={view === 'sentryApp'}
    style="--rail-tone: var(--src-sentry); --rail-glow: rgba(110, 80, 155, 0.42);"
    data-tooltip="Sentry · ⌘3"
    onclick={() => (view = 'sentryApp')}
    aria-label="Sentry"
  >
    <svg viewBox="0 0 24 24" fill="currentColor" stroke="none" aria-hidden="true">{@html SVG_SENTRY}</svg>
  </button>

  <div class="rail-divider"></div>

  <!-- Agents -->
  <button
    class="rail-btn"
    class:active={view === 'claudeApp'}
    class:rail-dropping={dropOverKind === 'claude'}
    style="--rail-tone: var(--src-claude); --rail-glow: rgba(232,155,125,0.42);"
    data-tooltip="Claude · ⌘4 — drop to attach"
    onclick={() => (view = 'claudeApp')}
    ondragenter={(e) => railDragEnter('claude', e)}
    ondragover={(e) => railDragOver('claude', e)}
    ondragleave={railDragLeave}
    ondrop={(e) => railDrop('claude', e)}
    aria-label="Claude"
  >
    <!-- Use the curated brand PNG (the same asset Connections renders
         in its agent card) instead of redrawing the sunburst inline.
         The PNG ships with the right gradients and clay petals; the
         rail just scales it down to a 19px square. -->
    <img class="rail-brand-img" src="/brand-claude.png" alt="" aria-hidden="true" draggable="false" />
  </button>

  <button
    class="rail-btn"
    class:active={view === 'cursorApp'}
    class:rail-dropping={dropOverKind === 'cursor'}
    style="--rail-tone: var(--src-cursor); --rail-glow: rgba(220,220,220,0.32);"
    data-tooltip="Cursor · ⌘5 — drop to attach"
    onclick={() => (view = 'cursorApp')}
    ondragenter={(e) => railDragEnter('cursor', e)}
    ondragover={(e) => railDragOver('cursor', e)}
    ondragleave={railDragLeave}
    ondrop={(e) => railDrop('cursor', e)}
    aria-label="Cursor"
  >
    <!-- Same rationale as Claude above — Anysphere's mark is a faceted
         3D hex with subtle gradients, baked into the brand PNG. -->
    <img class="rail-brand-img" src="/brand-cursor.png" alt="" aria-hidden="true" draggable="false" />
  </button>

  <div class="rail-divider"></div>

  <!-- Tools — multi-instance via RailAppButton: long-press / right-click
       opens a popover with every open instance + an Add button + a
       per-instance × on hover for non-primary entries. -->
  <RailAppButton
    kind="editor"
    label="Editor"
    tooltip="Editor · ⌘6"
    active={view === 'editorApp'}
    tone="var(--src-editor)"
    glow="rgba(204,120,92,0.42)"
    onActivate={() => (view = 'editorApp')}
  >
    {#snippet icon()}
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="18" height="18" rx="2"/><line x1="3" y1="9" x2="21" y2="9"/><line x1="9" y1="9" x2="9" y2="21"/></svg>
    {/snippet}
  </RailAppButton>

  <RailAppButton
    kind="canvas"
    label="Canvas"
    tooltip="Canvas · ⌘7"
    active={view === 'canvasApp'}
    tone="var(--src-canvas)"
    glow="rgba(125,194,213,0.40)"
    onActivate={() => (view = 'canvasApp')}
  >
    {#snippet icon()}
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="18" height="14" rx="2"/><rect x="6" y="6" width="9" height="6" rx="1"/><rect x="13" y="13" width="5" height="3" rx="0.5"/></svg>
    {/snippet}
  </RailAppButton>

  <RailAppButton
    kind="terminal"
    label="Terminal"
    tooltip="Terminal · ⌘8"
    active={view === 'terminalApp'}
    tone="var(--src-term)"
    glow="rgba(229,234,232,0.30)"
    onActivate={() => (view = 'terminalApp')}
  >
    {#snippet icon()}
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><polyline points="4 17 10 11 4 5"/><line x1="12" y1="19" x2="20" y2="19"/></svg>
    {/snippet}
  </RailAppButton>

  <div class="rail-spacer"></div>

  <!-- System cluster -->
  <button
    class="rail-btn"
    class:active={view === 'connections'}
    data-tooltip={anyRetrying ? 'Connections — retrying…' : 'Connections'}
    onclick={() => (view = 'connections')}
    aria-label="Connections"
  >
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M21 11V7a2 2 0 0 0-2-2H5a2 2 0 0 0-2 2v10a2 2 0 0 0 2 2h6"/><circle cx="17" cy="17" r="3"/><path d="M19 17h2"/></svg>
    {#if anyRetrying}
      <span class="rail-dot rail-dot--retrying" aria-label="retrying"></span>
    {:else if !anythingConnected && !statusLoading}
      <span class="rail-dot"></span>
    {/if}
  </button>

  <button
    class="rail-btn"
    class:active={view === 'rules'}
    data-tooltip="Rules"
    onclick={() => (view = 'rules')}
    aria-label="Rules"
  >
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7"/><path d="M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4z"/></svg>
  </button>

  <button
    class="rail-btn"
    class:active={view === 'settings'}
    data-tooltip="Settings"
    onclick={() => (view = 'settings')}
    aria-label="Settings"
  >
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09a1.65 1.65 0 0 0-1-1.51 1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09a1.65 1.65 0 0 0 1.51-1 1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33h.01a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"/></svg>
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
              {#if row.sub}<span class="rail-identity-sub">{row.sub}</span>{/if}
            </span>
          </li>
        {/each}
      </ul>
    </div>
  </button>
</aside>

<style>
  .rail {
    display: flex; flex-direction: column; align-items: center;
    padding: 14px 0 18px;
    gap: 6px;
    background: rgba(20, 24, 26, 0.92);
    border-right: 1px solid var(--border-neutral);
    backdrop-filter: blur(12px);
    position: relative;
    z-index: 5;
  }

  /* Brand mark = home button in the same 44×44 footprint as the
     rail-btns below. The SVG viewBox is tight to the W bbox
     (655×268, ≈2.4:1) so rendering at a fixed height = the W's
     true proportions; locking height to 18px (similar to the 19px
     svg icons) plus width: auto gives a natural ~44×18 W shape
     instead of a stretched square. Active-glow uses the same
     accent ring all rail-btns share when their view is selected. */
  .rail-sigil {
    position: relative;
    display: grid; place-items: center;
    width: 44px; height: 44px;
    margin-bottom: 6px;
    border-radius: 11px;
    border: 0;
    background: transparent;
    cursor: pointer;
    overflow: hidden;
    transition: background 140ms;
  }
  .rail-sigil:hover {
    background: color-mix(in srgb, var(--accent) 10%, transparent);
  }
  .rail-sigil.active {
    background: color-mix(in srgb, var(--accent) 16%, transparent);
    box-shadow:
      inset 0 0 0 1px color-mix(in srgb, var(--accent) 40%, transparent),
      0 0 12px var(--accent-glow);
  }
  .rail-sigil img {
    /* 13px tall ≈ matches the visual weight of the 19px square nav
       icons below, since the W is a low-aspect glyph (244 tall vs
       655 wide in its tight viewBox) and would dominate the rail
       column at parity with their box heights. */
    height: 13px;
    width: auto;
    max-width: 100%;
    display: block;
    filter: drop-shadow(0 1px 0 rgba(0, 0, 0, 0.35));
  }

  .rail-divider {
    width: 28px; height: 1px;
    background: var(--border);
    margin: 4px 0;
  }
  .rail-spacer { flex: 1; }

  :global(.rail-btn) {
    position: relative;
    width: 44px; height: 44px;
    display: grid; place-items: center;
    color: var(--text-mute);
    background: transparent;
    border: 0;
    border-radius: 11px;
    cursor: pointer;
    padding: 0;
    transition: color 140ms, background 140ms, box-shadow 220ms;
    --rail-tone: var(--accent-bright);
    --rail-glow: var(--accent-glow);
  }
  :global(.rail-btn svg) { width: 19px; height: 19px; }
  /* Brand PNGs (Claude / Cursor) — match the inline SVG footprint and
     keep their ratio. `display: block` plus `pointer-events: none`
     prevents the image from intercepting clicks meant for the rail
     button.

     `currentColor` doesn't tint a raster image, so we lean on CSS
     filters to mirror how the SVG siblings respond to active /
     hover state: dimmed + desaturated when their view isn't
     selected, full-colour on hover or when active. Mirrors the
     `--text-mute → --rail-tone` swap the SVG buttons get. */
  :global(.rail-brand-img) {
    width: 22px; height: 22px;
    object-fit: contain;
    display: block;
    pointer-events: none;
    -webkit-user-drag: none;
    /* Match how SVG rail-btns read when inactive (`var(--text-mute)`)
       without losing the glyph to the dark surface: full grayscale
       so the colour vocabulary stays consistent, but only mild
       opacity reduction so the muted gray icon still reads — too low
       and the orange Claude burst flattens to nothing on `--bg-1`. */
    filter: grayscale(1) opacity(0.78);
    transition: filter 160ms;
  }
  :global(.rail-btn:hover .rail-brand-img),
  :global(.rail-btn.active .rail-brand-img) {
    filter: none;
  }

  :global(.rail-btn:hover) {
    color: var(--rail-tone);
    background: color-mix(in srgb, var(--rail-tone) 10%, transparent);
  }
  :global(.rail-btn.active) {
    color: var(--rail-tone);
    background: linear-gradient(180deg,
      color-mix(in srgb, var(--rail-tone) 20%, transparent),
      color-mix(in srgb, var(--rail-tone) 8%, transparent));
    box-shadow:
      inset 0 0 0 1px color-mix(in srgb, var(--rail-tone) 35%, transparent),
      0 0 22px color-mix(in srgb, var(--rail-glow) 60%, transparent);
  }
  :global(.rail-btn.active::after) {
    content: '';
    position: absolute;
    bottom: -3px; left: 50%; transform: translateX(-50%);
    width: 5px; height: 5px;
    border-radius: 50%;
    background: var(--rail-tone);
    box-shadow: 0 0 10px var(--rail-glow);
  }

  /* Drop highlight — terracotta dashed ring + halo while a payload is
     hovering the rail button. Mirrors the composer-shell drop hint so
     the user reads the same affordance everywhere. */
  :global(.rail-btn.rail-dropping) {
    color: var(--rail-tone);
    background: color-mix(in srgb, var(--rail-tone) 22%, transparent);
    box-shadow:
      inset 0 0 0 1.5px var(--rail-tone),
      0 0 0 4px color-mix(in srgb, var(--rail-glow) 40%, transparent),
      0 0 22px color-mix(in srgb, var(--rail-glow) 70%, transparent);
    transform: scale(1.04);
  }
  :global(.rail-btn.rail-dropping > svg) {
    transform: scale(1.05);
    filter: drop-shadow(0 0 8px var(--rail-glow));
  }

  /* Inbox-count badge */
  .rail-badge {
    position: absolute;
    top: 2px; right: 2px;
    min-width: 14px; height: 14px;
    padding: 0 3px;
    border-radius: 8px;
    background: var(--accent);
    color: var(--accent-fg);
    font-family: 'Inter Tight', system-ui, sans-serif;
    font-size: 9.5px; font-weight: 700;
    display: inline-flex; align-items: center; justify-content: center;
    box-shadow: 0 0 0 2px var(--bg-1), 0 0 10px var(--accent-glow);
  }

  /* Retry / disconnect dot */
  .rail-dot {
    position: absolute;
    top: 8px; right: 8px;
    width: 6px; height: 6px;
    border-radius: 50%;
    background: var(--warning);
    box-shadow: 0 0 0 2px var(--bg-1), 0 0 8px rgba(217, 184, 110, 0.5);
  }
  .rail-dot--retrying {
    background: var(--accent);
    box-shadow: 0 0 0 2px var(--bg-1), 0 0 8px var(--accent-glow);
    animation: rail-dot-pulse 1.4s ease-in-out infinite;
  }
  @keyframes rail-dot-pulse {
    0%, 100% { opacity: 0.55; }
    50%      { opacity: 1; }
  }
  @media (prefers-reduced-motion: reduce) {
    .rail-dot--retrying { animation: none; opacity: 0.85; }
  }

  /* Avatar at the bottom — popover with identity rows */
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

  /* Tooltip on hover (right side) — global so RailAppButton's nested
     button picks it up too. */
  :global(.rail-btn[data-tooltip]:hover::before) {
    content: attr(data-tooltip);
    position: absolute;
    left: 52px; top: 50%; transform: translateY(-50%);
    padding: 4px 10px;
    background: var(--bg-3);
    border: 1px solid var(--border-neutral-hi);
    border-radius: 6px;
    font-size: 11.5px;
    color: var(--text-0);
    white-space: nowrap;
    pointer-events: none;
    z-index: 10;
    box-shadow: var(--shadow-1);
  }
</style>
