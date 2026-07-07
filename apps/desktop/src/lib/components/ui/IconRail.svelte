<script lang="ts">
  /* Cabin icon rail — 56px glyph column replacing the labelled Sidebar
     (redesign v2, §2.2). Groups: overview → sources → surfaces, then a
     spring, then system at the bottom. Badges are 5px dots (not
     numbers); the editor glyph carries a <sup> instance count.
     Multi-instance kinds (editor/canvas/terminal) open a right-click
     menu to switch / add / close instances — the sub-rows the old
     Sidebar rendered inline. Drag-n-drop onto the claude glyph is
     preserved. */
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
  import { dragState, requestCanvasRailDrop } from '$lib/state/drag.svelte';

  type View =
    | 'home' | 'jiraApp' | 'githubApp' | 'sentryApp' | 'claudeApp'
    | 'editorApp' | 'canvasApp' | 'terminalApp'
    | 'rules' | 'library' | 'connections' | 'settings';

  interface Props {
    view: View;
    githubBadge?: number;
    jiraBadge?: number;
    sentryBadge?: number;
    claudeBusy?: boolean;
    dragActive?: boolean;
    onAgentDrop?: (e: DragEvent) => void;
  }

  let {
    view = $bindable(),
    githubBadge = 0,
    jiraBadge = 0,
    sentryBadge = 0,
    claudeBusy = false,
    dragActive = false,
    onAgentDrop
  }: Props = $props();

  type IconKind = 'brand' | 'stroke' | 'claude';
  interface NavItem {
    view: View;
    label: string;
    icon: IconKind;
    svg?: string;
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
    { view: 'home', label: 'Home', icon: 'stroke', svg: ICON_HOME }
  ];
  const sources = $derived<NavItem[]>([
    { view: 'jiraApp', label: 'Jira', icon: 'brand', svg: SVG_JIRA, count: jiraBadge },
    { view: 'githubApp', label: 'GitHub', icon: 'brand', svg: SVG_GITHUB, count: githubBadge },
    { view: 'sentryApp', label: 'Sentry', icon: 'brand', svg: SVG_SENTRY, count: sentryBadge }
  ]);
  const surfaces = $derived<NavItem[]>([
    { view: 'claudeApp', label: 'Claude', icon: 'claude', busy: claudeBusy },
    { view: 'editorApp', label: 'Editor', icon: 'stroke', svg: ICON_EDITOR, kind: 'editor' },
    { view: 'canvasApp', label: 'Canvas', icon: 'stroke', svg: ICON_CANVAS, kind: 'canvas' },
    { view: 'terminalApp', label: 'Terminal', icon: 'stroke', svg: ICON_TERMINAL, kind: 'terminal' }
  ]);
  const system: NavItem[] = [
    { view: 'library', label: 'Library', icon: 'stroke', svg: ICON_LIBRARY },
    { view: 'rules', label: 'Rules', icon: 'stroke', svg: ICON_RULES },
    { view: 'connections', label: 'Connections', icon: 'stroke', svg: ICON_CONNECTIONS },
    { view: 'settings', label: 'Settings', icon: 'stroke', svg: ICON_SETTINGS }
  ];

  function instancesOf(kind: AppKind | undefined) {
    return kind ? layoutState.instances[kind] ?? [] : [];
  }
  function editorRepoHint(id: string): string {
    const slot = sessionsState.editorInstanceState[id];
    const roots = slot?.repoPaths?.length ? slot.repoPaths : slot?.repoPath ? [slot.repoPath] : [];
    return roots.map((r) => r.split('/').filter(Boolean).pop() ?? '').filter(Boolean).join(' + ');
  }

  /* ----- instance context menu ----- */
  let menu = $state<{ kind: AppKind; x: number; y: number } | null>(null);
  function openMenu(e: MouseEvent, it: NavItem) {
    if (!it.kind || !MULTI_INSTANCE_KINDS.has(it.kind)) return;
    e.preventDefault();
    view = it.view;
    menu = { kind: it.kind, x: 60, y: Math.min(e.clientY, window.innerHeight - 220) };
  }
  function closeMenu() { menu = null; }
  function pickInstance(kind: AppKind, id: string) { setActiveInstance(kind, id); closeMenu(); }
  function spawnInstance(kind: AppKind) { addInstance(kind); }
  function dropInstance(kind: AppKind, id: string, e: MouseEvent) { e.stopPropagation(); removeInstance(kind, id); }

  /* ----- claude drag-drop ----- */
  let dropOver = $state(false);
  function hasDropPayload(e: DragEvent): boolean {
    const t = e.dataTransfer?.types;
    return !!t && (t.indexOf('Files') !== -1 || t.indexOf('text/uri-list') !== -1 || t.indexOf('text/plain') !== -1);
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
    if (payload) requestCanvasRailDrop(payload);
  }
</script>

<svelte:window onclick={() => menu && closeMenu()} />

{#snippet glyph(it: NavItem)}
  <button
    class="rail-item"
    class:active={view === it.view}
    class:drop-over={it.view === 'claudeApp' && (dropOver || dragActive)}
    title={it.label}
    aria-label={it.label}
    onclick={() => (view = it.view)}
    oncontextmenu={(e) => openMenu(e, it)}
    ondragover={it.view === 'claudeApp' ? claudeDragOver
      : it.view === 'canvasApp' ? (e) => { if (dragState.payload) e.preventDefault(); } : undefined}
    ondragleave={it.view === 'claudeApp' ? () => (dropOver = false) : undefined}
    ondrop={it.view === 'claudeApp' ? claudeDrop
      : it.view === 'canvasApp' ? canvasDrop : undefined}
  >
    <span class="rail-glyph" style:color={view === it.view ? 'var(--text-0)' : undefined}>
      {#if it.icon === 'claude'}
        <span class="rail-brand-mask" aria-hidden="true"></span>
      {:else if it.icon === 'brand'}
        <svg viewBox="0 0 24 24" fill="currentColor" stroke="none" aria-hidden="true">{@html it.svg}</svg>
      {:else}
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">{@html it.svg}</svg>
      {/if}
    </span>
    {#if it.busy}
      <span class="rail-dot rail-dot--pulse"></span>
    {:else if it.count}
      <span class="rail-dot"></span>
    {/if}
    {#if it.kind && instancesOf(it.kind).length > 1}
      <sup class="rail-sup">{instancesOf(it.kind).length}</sup>
    {/if}
  </button>
{/snippet}

<nav class="rail" class:is-drag-active={dragActive} aria-label="Primary">
  <div class="rail-group">
    {#each overview as it (it.view)}{@render glyph(it)}{/each}
  </div>
  <div class="rail-sep"></div>
  <div class="rail-group">
    {#each sources as it (it.view)}{@render glyph(it)}{/each}
  </div>
  <div class="rail-sep"></div>
  <div class="rail-group">
    {#each surfaces as it (it.view)}{@render glyph(it)}{/each}
  </div>

  <div class="rail-spring"></div>

  <div class="rail-group rail-group--foot">
    {#each system as it (it.view)}{@render glyph(it)}{/each}
  </div>
</nav>

{#if menu}
  <div class="rail-menu" style="left:{menu.x}px; top:{menu.y}px" role="menu">
    <div class="rail-menu-head">{menu.kind}s</div>
    {#each instancesOf(menu.kind) as inst (inst.id)}
      <div class="rail-menu-row" class:active={layoutState.activeInstance[menu.kind] === inst.id}>
        <button class="rail-menu-pick" onclick={() => pickInstance(menu!.kind, inst.id)}>
          <span class="rail-menu-name">{inst.name}</span>
          {#if menu.kind === 'editor' && editorRepoHint(inst.id)}
            <span class="rail-menu-hint mono">{editorRepoHint(inst.id)}</span>
          {/if}
        </button>
        {#if !inst.primary}
          <button class="rail-menu-close" title="Close {inst.name}" aria-label="Close {inst.name}" onclick={(e) => dropInstance(menu!.kind, inst.id, e)}>
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" aria-hidden="true"><line x1="6" y1="6" x2="18" y2="18"/><line x1="18" y1="6" x2="6" y2="18"/></svg>
          </button>
        {/if}
      </div>
    {/each}
    <button class="rail-menu-add" onclick={() => spawnInstance(menu!.kind)}>
      <span class="rail-menu-plus">+</span> new {menu.kind}
    </button>
  </div>
{/if}

<style>
  .rail {
    width: 56px; flex: none;
    display: flex; flex-direction: column; align-items: center;
    gap: 4px;
    padding: 10px 0;
    background: var(--bg-0);
    border-right: 1px solid var(--border-lo);
    min-height: 0;
    position: relative;
    z-index: 5;
  }
  .rail-group { display: flex; flex-direction: column; align-items: center; gap: 4px; width: 100%; }
  .rail-group--foot { gap: 6px; padding-bottom: 2px; }
  .rail-sep { width: 22px; height: 1px; background: var(--border-lo); margin: 6px 0; }
  .rail-spring { flex: 1; }

  .rail-item {
    position: relative;
    width: 36px; height: 36px;
    display: grid; place-items: center;
    border: 0; border-radius: var(--r-card);
    background: transparent;
    color: var(--text-faint);
    cursor: pointer;
    transition: background 120ms, color 120ms;
  }
  .rail-item:hover { background: var(--bg-hover); color: var(--text-1); }
  .rail-item.active { background: var(--bg-nav); color: var(--text-0); }
  .rail-item.drop-over { box-shadow: inset 0 0 0 1px var(--border-accent, var(--src-claude-border)); }

  .rail-glyph {
    display: inline-flex; align-items: center; justify-content: center;
    width: 16px; height: 16px; color: inherit;
  }
  .rail-glyph svg { width: 16px; height: 16px; }
  .rail-brand-mask {
    width: 15px; height: 15px; display: block;
    background: currentColor;
    -webkit-mask: url('/brand-claude.png') center / contain no-repeat;
    mask: url('/brand-claude.png') center / contain no-repeat;
  }

  .rail-dot {
    position: absolute; top: 6px; right: 6px;
    width: 5px; height: 5px; border-radius: 50%;
    background: var(--text-1);
  }
  .rail-dot--pulse { background: var(--ok); animation: rail-pulse 1.6s infinite; }
  @keyframes rail-pulse { 0%, 100% { opacity: 0.35; } 50% { opacity: 1; } }
  .rail-sup {
    position: absolute; top: 3px; right: 4px;
    font-size: 9px; line-height: 1; color: var(--text-faint);
    font-family: var(--font-mono);
  }

  /* Instance context menu */
  .rail-menu {
    position: fixed;
    min-width: 176px;
    padding: 6px;
    background: var(--bg-1);
    border: 1px solid var(--border-hi);
    border-radius: 10px;
    box-shadow: var(--shadow-2);
    z-index: 200;
  }
  .rail-menu-head {
    font-size: 9.5px; font-weight: 600; text-transform: uppercase;
    letter-spacing: 0.09em; color: var(--text-faint);
    padding: 3px 8px 6px;
  }
  .rail-menu-row { display: flex; align-items: center; border-radius: var(--r-item); }
  .rail-menu-row:hover { background: var(--bg-hover); }
  .rail-menu-row.active .rail-menu-name { color: var(--text-0); font-weight: 600; }
  .rail-menu-pick {
    flex: 1; min-width: 0;
    display: flex; align-items: center; gap: 7px;
    padding: 5px 8px; border: 0; background: transparent;
    text-align: left; cursor: pointer;
    font-size: 12px; color: var(--text-1);
  }
  .rail-menu-hint {
    margin-left: auto; font-size: 10px; color: var(--text-faint);
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .rail-menu-close {
    flex: none; width: 22px; height: 22px; margin-right: 4px;
    display: grid; place-items: center;
    border: 0; border-radius: 5px; background: transparent;
    color: var(--text-faint); cursor: pointer;
  }
  .rail-menu-close:hover { color: var(--err); background: var(--bg-sel); }
  .rail-menu-close svg { width: 11px; height: 11px; }
  .rail-menu-add {
    display: flex; align-items: center; gap: 6px;
    width: 100%; margin-top: 2px; padding: 5px 8px;
    border: 0; border-radius: var(--r-item); background: transparent;
    font-size: 11px; color: var(--text-faint); cursor: pointer; text-align: left;
  }
  .rail-menu-add:hover { background: var(--bg-hover); color: var(--text-1); }
  .rail-menu-plus { font-size: 13px; line-height: 1; color: var(--text-mute); }
</style>
