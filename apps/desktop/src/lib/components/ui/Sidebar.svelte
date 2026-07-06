<script lang="ts">
  /* Paper redesign sidebar — labeled navigation column that replaces
     the icon rail. Groups per the mockup: OVERVIEW / SOURCES /
     SURFACES / SYSTEM; footer hosts the update card + settings row +
     identity avatar. Collapsible to a 56px icon-only strip (chevron
     in the brand row; persisted). Icons are the app-brand SVG/PNG
     marks carried over from the old rail, not abstract glyphs. */
  import { onMount } from 'svelte';
  import { getVersion } from '@tauri-apps/api/app';
  import RailIdentityAvatar from './rail/RailIdentityAvatar.svelte';
  import { SVG_GITHUB, SVG_JIRA, SVG_SENTRY } from '$lib/data';
  import {
    layoutState,
    setActiveInstance,
    addInstance,
    removeInstance,
    MULTI_INSTANCE_KINDS,
    type AppKind
  } from '$lib/state/layout.svelte';
  import { sessionsState } from '$lib/state/sessions.svelte';
  import { updateState, installNow, snooze } from '$lib/state/updates.svelte';
  import { dragState, requestCanvasRailDrop } from '$lib/state/drag.svelte';
  import type {
    ClaudeStatus,
    ConnectionStatus,
    JiraStatus,
    SentryStatus
  } from '$lib/data';

  type View =
    | 'home'
    | 'jiraApp'
    | 'githubApp'
    | 'sentryApp'
    | 'claudeApp'
    | 'editorApp'
    | 'canvasApp'
    | 'terminalApp'
    | 'rules'
    | 'library'
    | 'connections'
    | 'settings';

  interface Props {
    view: View;
    anythingConnected: boolean;
    statusLoading: boolean;
    anyRetrying?: boolean;
    githubStatus: ConnectionStatus;
    jiraStatus?: JiraStatus;
    sentryStatus?: SentryStatus;
    claudeStatus?: ClaudeStatus | null;
    githubBadge?: number;
    jiraBadge?: number;
    sentryBadge?: number;
    dragActive?: boolean;
    claudeBusy?: boolean;
    onAgentDrop?: (e: DragEvent) => void;
  }

  let {
    view = $bindable(),
    anythingConnected: _anythingConnected,
    statusLoading: _statusLoading,
    anyRetrying: _anyRetrying = false,
    githubStatus,
    jiraStatus,
    sentryStatus,
    claudeStatus,
    githubBadge = 0,
    jiraBadge = 0,
    sentryBadge = 0,
    dragActive = false,
    claudeBusy = false,
    onAgentDrop
  }: Props = $props();

  let version = $state('');
  onMount(async () => {
    try { version = await getVersion(); } catch { /* browser preview */ }
  });

  /* ----- collapse (persisted) ----- */
  const COLLAPSE_KEY = 'woom:sidebar-collapsed:v1';
  let collapsed = $state(false);
  onMount(() => {
    try { collapsed = localStorage.getItem(COLLAPSE_KEY) === '1'; } catch { /* ignore */ }
  });
  function toggleCollapsed() {
    collapsed = !collapsed;
    try { localStorage.setItem(COLLAPSE_KEY, collapsed ? '1' : '0'); } catch { /* ignore */ }
  }

  /* ----- nav model ----- */
  type IconKind = 'brand' | 'stroke' | 'claude';
  interface NavItem {
    view: View;
    label: string;
    icon: IconKind;
    /** Inner SVG markup (paths). Unused for `claude` (PNG mark). */
    svg?: string;
    tone?: string;
    count?: number;
    kind?: AppKind;
    busy?: boolean;
  }

  const ICON_HOME =
    '<path d="M3 9l9-7 9 7v11a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/><polyline points="9 22 9 12 15 12 15 22"/>';
  const ICON_EDITOR =
    '<rect x="3" y="3" width="18" height="18" rx="2"/><line x1="3" y1="9" x2="21" y2="9"/><line x1="9" y1="9" x2="9" y2="21"/>';
  const ICON_CANVAS =
    '<rect x="3" y="3" width="18" height="14" rx="2"/><rect x="6" y="6" width="9" height="6" rx="1"/><rect x="13" y="13" width="5" height="3" rx="0.5"/>';
  const ICON_TERMINAL =
    '<polyline points="4 17 10 11 4 5"/><line x1="12" y1="19" x2="20" y2="19"/>';
  const ICON_CONNECTIONS =
    '<path d="M21 11V7a2 2 0 0 0-2-2H5a2 2 0 0 0-2 2v10a2 2 0 0 0 2 2h6"/><circle cx="17" cy="17" r="3"/><path d="M19 17h2"/>';
  const ICON_RULES =
    '<path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7"/><path d="M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4z"/>';
  const ICON_LIBRARY =
    '<path d="M4 4h6v16H4z"/><path d="M14 4h2v16h-2z"/><path d="M18 5l2 .5L22 19l-2 .5z"/>';
  const ICON_SETTINGS =
    '<circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09a1.65 1.65 0 0 0-1-1.51 1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09a1.65 1.65 0 0 0 1.51-1 1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33h.01a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"/>';

  const overview: NavItem[] = [
    { view: 'home', label: 'home', icon: 'stroke', svg: ICON_HOME }
  ];

  const sources = $derived<NavItem[]>([
    { view: 'jiraApp', label: 'jira', icon: 'brand', svg: SVG_JIRA, tone: 'var(--src-jira)', count: jiraBadge },
    { view: 'githubApp', label: 'github', icon: 'brand', svg: SVG_GITHUB, tone: 'var(--src-github)', count: githubBadge },
    { view: 'sentryApp', label: 'sentry', icon: 'brand', svg: SVG_SENTRY, tone: 'var(--src-sentry)', count: sentryBadge }
  ]);

  const surfaces = $derived<NavItem[]>([
    { view: 'claudeApp', label: 'claude', icon: 'claude', tone: 'var(--src-claude)', busy: claudeBusy },
    { view: 'editorApp', label: 'editor', icon: 'stroke', svg: ICON_EDITOR, tone: 'var(--src-editor)', kind: 'editor' },
    { view: 'canvasApp', label: 'canvas', icon: 'stroke', svg: ICON_CANVAS, tone: 'var(--src-canvas)', kind: 'canvas' },
    { view: 'terminalApp', label: 'terminal', icon: 'stroke', svg: ICON_TERMINAL, tone: 'var(--src-term)', kind: 'terminal' }
  ]);

  const system: NavItem[] = [
    { view: 'connections', label: 'connections', icon: 'stroke', svg: ICON_CONNECTIONS },
    { view: 'rules', label: 'rules', icon: 'stroke', svg: ICON_RULES },
    { view: 'library', label: 'library', icon: 'stroke', svg: ICON_LIBRARY }
  ];

  function instancesOf(kind: AppKind | undefined) {
    if (!kind) return [];
    return layoutState.instances[kind] ?? [];
  }

  /** Editor instances get the open repo's folder name next to the
   *  curated mark — five "Vermeer / Hokusai / …" rows say nothing
   *  about WHICH repo each one holds. */
  function instanceHint(kind: AppKind, id: string): string {
    if (kind !== 'editor') return '';
    const slot = sessionsState.editorInstanceState[id];
    const roots = slot?.repoPaths?.length
      ? slot.repoPaths
      : slot?.repoPath ? [slot.repoPath] : [];
    return roots
      .map((r) => r.split('/').filter(Boolean).pop() ?? '')
      .filter(Boolean)
      .join(' + ');
  }

  /* ----- update card ----- */
  const updateReady = $derived.by(() => {
    const p = updateState.phase;
    return p.kind === 'installed_pending_quit' || p.kind === 'available' ? p : null;
  });

  /* ----- claude drag-drop (same contract as the old rail) ----- */
  let dropOver = $state(false);
  function hasDropPayload(e: DragEvent): boolean {
    const t = e.dataTransfer?.types;
    if (!t) return false;
    return (
      t.indexOf('Files') !== -1 ||
      t.indexOf('text/uri-list') !== -1 ||
      t.indexOf('text/plain') !== -1
    );
  }
  function claudeDragOver(e: DragEvent) {
    if (!hasDropPayload(e)) return;
    e.preventDefault();
    if (e.dataTransfer) e.dataTransfer.dropEffect = 'copy';
    dropOver = true;
  }
  function claudeDrop(e: DragEvent) {
    dropOver = false;
    if (!onAgentDrop) return;
    e.preventDefault();
    view = 'claudeApp';
    onAgentDrop(e);
  }
  function canvasDrop(_e: DragEvent) {
    const payload = dragState.payload;
    if (!payload) return;
    requestCanvasRailDrop(payload);
  }
</script>

{#snippet navIcon(it: NavItem, active: boolean)}
  <span
    class="sb-glyph"
    style:color={active ? (it.tone ?? 'var(--text-0)') : 'var(--text-mute)'}
  >
    {#if it.icon === 'claude'}
      <!-- Mono via CSS mask — the official PNG is orange; masking its
           silhouette with currentColor keeps the mark on the ink
           ladder like every other sidebar glyph. -->
      <span class="sb-brand-mask" aria-hidden="true"></span>
    {:else if it.icon === 'brand'}
      <svg viewBox="0 0 24 24" fill="currentColor" stroke="none" aria-hidden="true">{@html it.svg}</svg>
    {:else}
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">{@html it.svg}</svg>
    {/if}
  </span>
{/snippet}

{#snippet navRow(it: NavItem)}
  <button
    class="sb-item"
    class:active={view === it.view}
    class:drop-over={it.view === 'claudeApp' && (dropOver || dragActive)}
    title={collapsed ? it.label : undefined}
    onclick={() => (view = it.view)}
    ondragover={it.view === 'claudeApp' ? claudeDragOver
      : it.view === 'canvasApp' ? (e) => { if (dragState.payload) e.preventDefault(); } : undefined}
    ondragleave={it.view === 'claudeApp' ? () => (dropOver = false) : undefined}
    ondrop={it.view === 'claudeApp' ? claudeDrop
      : it.view === 'canvasApp' ? canvasDrop : undefined}
  >
    {@render navIcon(it, view === it.view)}
    {#if !collapsed}
      <span class="sb-label">{it.label}</span>
      {#if it.busy}
        <span class="sb-pulse" style:background={it.tone ?? 'var(--src-claude)'}></span>
      {/if}
      {#if it.count}
        <span class="sb-count">{it.count}</span>
      {:else if it.kind && instancesOf(it.kind).length > 1}
        <span class="sb-count">{instancesOf(it.kind).length}</span>
      {/if}
    {:else if it.busy}
      <span class="sb-pulse sb-pulse--dot" style:background={it.tone ?? 'var(--src-claude)'}></span>
    {/if}
  </button>
  {#if !collapsed && it.kind && view === it.view && MULTI_INSTANCE_KINDS.has(it.kind)}
    {#each instancesOf(it.kind) as inst (inst.id)}
      <div
        class="sb-sub"
        class:active={layoutState.activeInstance[it.kind] === inst.id}
      >
        <button
          class="sb-sub-main"
          onclick={() => setActiveInstance(it.kind!, inst.id)}
        >
          <span class="sb-sub-tick">·</span>{inst.name}
          {#if instanceHint(it.kind, inst.id)}
            <span class="sb-sub-hint" title={instanceHint(it.kind, inst.id)}>{instanceHint(it.kind, inst.id)}</span>
          {/if}
        </button>
        {#if !inst.primary}
          <button
            class="sb-sub-close"
            title="Close {inst.name}"
            aria-label="Close {inst.name}"
            onclick={() => removeInstance(it.kind!, inst.id)}
          >
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" aria-hidden="true"><line x1="6" y1="6" x2="18" y2="18"/><line x1="18" y1="6" x2="6" y2="18"/></svg>
          </button>
        {/if}
      </div>
    {/each}
    <button
      class="sb-add"
      title="New {it.label}"
      onclick={() => addInstance(it.kind!)}
    >
      <span class="sb-add-plus">+</span> new {it.label}
    </button>
  {/if}
{/snippet}

<aside class="sb" class:collapsed class:is-drag-active={dragActive} role="navigation">
  <div class="sb-brand">
    <span class="sb-mark" role="img" aria-label="Woom"></span>
    {#if !collapsed}
      <span class="sb-brand-name">woom</span>
      {#if version}<span class="sb-version">{version}</span>{/if}
    {/if}
    <button
      class="sb-collapse"
      onclick={toggleCollapsed}
      title={collapsed ? 'Expand sidebar' : 'Collapse sidebar'}
      aria-label={collapsed ? 'Expand sidebar' : 'Collapse sidebar'}
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        {#if collapsed}<polyline points="9 18 15 12 9 6" />{:else}<polyline points="15 18 9 12 15 6" />{/if}
      </svg>
    </button>
  </div>

  <div class="sb-scroll">
    <div class="sb-group">
      {#if !collapsed}<div class="sb-group-label">Overview</div>{/if}
      {#each overview as it (it.view)}{@render navRow(it)}{/each}
    </div>

    <div class="sb-group">
      {#if !collapsed}<div class="sb-group-label">Sources</div>{:else}<div class="sb-rule"></div>{/if}
      {#each sources as it (it.view)}{@render navRow(it)}{/each}
    </div>

    <div class="sb-group">
      {#if !collapsed}<div class="sb-group-label">Surfaces</div>{:else}<div class="sb-rule"></div>{/if}
      {#each surfaces as it (it.view)}{@render navRow(it)}{/each}
    </div>

    <div class="sb-group">
      {#if !collapsed}<div class="sb-group-label">System</div>{:else}<div class="sb-rule"></div>{/if}
      {#each system as it (it.view)}{@render navRow(it)}{/each}
    </div>
  </div>

  <div class="sb-foot">
    {#if updateReady && !collapsed}
      <div class="sb-update">
        <div class="sb-update-h">
          <span class="sb-update-dot"></span>
          {updateReady.version} {updateReady.kind === 'installed_pending_quit' ? 'ready' : 'available'}
        </div>
        <div class="sb-update-sub">ed25519 verified · installs on quit</div>
        <div class="sb-update-actions">
          <button class="sb-update-install" onclick={() => installNow()}>Install now</button>
          <button class="sb-update-snooze" onclick={() => snooze(4)}>Snooze</button>
        </div>
      </div>
    {/if}
    <div class="sb-foot-row">
      <button
        class="sb-item sb-settings"
        class:active={view === 'settings'}
        title={collapsed ? 'settings' : undefined}
        onclick={() => (view = 'settings')}
      >
        <span class="sb-glyph" style:color={view === 'settings' ? 'var(--text-0)' : 'var(--text-mute)'}>
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">{@html ICON_SETTINGS}</svg>
        </span>
        {#if !collapsed}<span class="sb-label">settings</span>{/if}
      </button>
      <RailIdentityAvatar
        {githubStatus}
        {jiraStatus}
        {sentryStatus}
        {claudeStatus}
      />
    </div>
  </div>
</aside>

<style>
  .sb {
    width: 212px; flex: none;
    display: flex; flex-direction: column;
    background: var(--bg-1);
    border-right: 1px solid var(--border);
    min-height: 0;
    position: relative;
    z-index: 5;
    transition: width 160ms var(--ease-out, ease-out);
  }
  .sb.collapsed { width: 56px; }

  .sb-brand {
    display: flex; align-items: center; gap: 9px;
    padding: 16px 12px 14px 16px;
  }
  .collapsed .sb-brand { padding: 16px 0 14px; justify-content: center; flex-direction: column; gap: 6px; }
  /* Engraved W — alpha mask re-inked per theme. */
  .sb-mark {
    display: block;
    width: 26px; height: 13px; flex: none;
    background: var(--text-0);
    -webkit-mask: url('/woom-mark-ink.png') center / contain no-repeat;
    mask: url('/woom-mark-ink.png') center / contain no-repeat;
  }
  .sb-brand-name { font-size: 13px; font-weight: 600; color: var(--text-0); }
  .sb-version { font-size: 10px; color: var(--text-faint); margin-left: auto; }
  .sb-collapse {
    display: grid; place-items: center;
    width: 18px; height: 18px;
    border: 0; border-radius: 5px;
    background: transparent;
    color: var(--text-faint);
    cursor: pointer;
    flex: none;
  }
  .sb-brand .sb-collapse { margin-left: auto; }
  .collapsed .sb-brand .sb-collapse { margin-left: 0; }
  .sb-collapse:hover { color: var(--text-0); background: var(--bg-hover); }
  .sb-collapse svg { width: 12px; height: 12px; }

  .sb-scroll { flex: 1; overflow-y: auto; overflow-x: hidden; padding: 0 10px 10px; min-height: 0; }
  .collapsed .sb-scroll { padding: 0 8px 10px; }
  .sb-group { margin-top: 10px; }
  .sb-group-label {
    font-size: 9.5px; font-weight: 600;
    letter-spacing: 0.14em; text-transform: uppercase;
    color: var(--text-faint);
    padding: 4px 8px 6px;
    white-space: nowrap;
  }
  .sb-rule { height: 1px; background: var(--border-lo); margin: 4px 6px 8px; }

  .sb-item {
    display: flex; align-items: center; gap: 9px;
    width: 100%;
    padding: 6px 8px;
    border-radius: var(--r-item);
    cursor: pointer;
    background: transparent;
    border: 0;
    margin-bottom: 1px;
    text-align: left;
    transition: background 120ms;
    position: relative;
  }
  .collapsed .sb-item { justify-content: center; padding: 7px 0; }
  .sb-item:hover { background: var(--bg-hover); }
  .sb-item.active { background: var(--bg-nav); }
  .sb-item.active .sb-label { color: var(--text-0); font-weight: 600; }
  .sb-item.drop-over {
    box-shadow: inset 0 0 0 1px var(--src-claude-border);
  }
  .sb-glyph {
    display: inline-flex; align-items: center; justify-content: center;
    width: 16px; height: 16px; flex: none;
    color: var(--text-mute);
  }
  .sb-glyph svg { width: 15px; height: 15px; }
  .sb-brand-mask {
    width: 15px; height: 15px; display: block;
    background: currentColor;
    -webkit-mask: url('/brand-claude.png') center / contain no-repeat;
    mask: url('/brand-claude.png') center / contain no-repeat;
  }
  .sb-label { font-size: 12.5px; color: var(--text-1); white-space: nowrap; }
  .sb-count { margin-left: auto; font-size: 10.5px; color: var(--text-faint); }
  .sb-pulse {
    margin-left: auto;
    width: 6px; height: 6px; border-radius: 50%;
    animation: sb-pulsedot 1.6s infinite;
  }
  .sb-pulse--dot {
    position: absolute;
    top: 4px; right: 6px;
    margin-left: 0;
  }
  .sb-sub {
    display: flex; align-items: center;
    width: calc(100% - 24px);
    margin: 0 0 1px 24px;
    border-radius: var(--r-item);
    color: var(--text-mute);
    min-width: 0;
  }
  .sb-sub:hover { background: var(--bg-hover); }
  .sb-sub:hover .sb-sub-main { color: var(--text-1); }
  .sb-sub.active .sb-sub-main { color: var(--text-0); }
  .sb-sub-main {
    display: flex; align-items: center; gap: 7px;
    flex: 1; min-width: 0;
    padding: 4px 8px;
    border: 0; background: transparent;
    font-size: 11.5px; color: inherit;
    cursor: pointer; text-align: left;
  }
  .sb-sub-tick { color: var(--text-faint); }
  .sb-sub-hint {
    margin-left: auto;
    padding-left: 8px;
    font-size: 10px;
    color: var(--text-faint);
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
    min-width: 0;
  }
  .sb-sub-close {
    display: grid; place-items: center;
    flex: none;
    width: 20px; height: 20px; margin-right: 4px;
    border: 0; border-radius: 5px;
    background: transparent;
    color: var(--text-faint);
    cursor: pointer;
    opacity: 0;
    transition: opacity 120ms, color 120ms, background 120ms;
  }
  .sb-sub:hover .sb-sub-close { opacity: 1; }
  .sb-sub-close:hover { color: var(--err); background: var(--bg-sel); }
  .sb-sub-close svg { width: 11px; height: 11px; }
  .sb-add {
    display: flex; align-items: center; gap: 6px;
    width: calc(100% - 24px);
    margin: 2px 0 2px 24px;
    padding: 4px 8px;
    border: 0; border-radius: var(--r-item);
    background: transparent;
    font-size: 11px; color: var(--text-faint);
    cursor: pointer; text-align: left;
  }
  .sb-add:hover { background: var(--bg-hover); color: var(--text-1); }
  .sb-add-plus { font-size: 13px; line-height: 1; color: var(--text-mute); }

  .sb-foot {
    flex: none;
    padding: 12px;
    border-top: 1px solid var(--border-lo);
  }
  .collapsed .sb-foot { padding: 10px 8px; }
  .sb-update {
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--bg-0);
    padding: 10px 11px;
    margin-bottom: 10px;
    box-shadow: var(--shadow-1);
  }
  .sb-update-h {
    display: flex; align-items: center; gap: 6px;
    font-size: 11px; font-weight: 600; color: var(--text-0);
  }
  .sb-update-dot {
    width: 6px; height: 6px; border-radius: 50%;
    background: var(--ok);
    animation: sb-pulsedot 2s infinite;
  }
  .sb-update-sub { font-size: 10.5px; color: var(--text-mute); margin: 4px 0 8px; }
  .sb-update-actions { display: flex; gap: 6px; }
  .sb-update-install {
    font-size: 10.5px; font-weight: 600;
    padding: 3px 9px; border-radius: var(--r-btn); border: 0;
    background: var(--text-0); color: var(--bg-0);
    cursor: pointer;
    box-shadow: var(--shadow-pill);
  }
  .sb-update-snooze {
    font-size: 10.5px;
    padding: 3px 9px; border-radius: var(--r-btn);
    border: 1px solid var(--border-hi);
    background: transparent; color: var(--text-1);
    cursor: pointer;
  }
  .sb-foot-row { display: flex; align-items: center; gap: 6px; }
  .collapsed .sb-foot-row { flex-direction: column; gap: 8px; }
  .sb-settings { flex: 1; margin-bottom: 0; }
  .collapsed .sb-settings { flex: none; width: 100%; }

  @keyframes sb-pulsedot {
    0%, 100% { opacity: 0.35; }
    50% { opacity: 1; }
  }
</style>
