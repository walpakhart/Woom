<script lang="ts">
  /* Left navigation rail — thin shell. Per-feature subsystems live in
   * sibling components under `rail/`:
   *   - RailTooltip       floating tooltip (escapes ancestor clip)
   *   - RailActiveHalo    active-button outer-glow overlay
   *   - RailIdentityAvatar avatar + identity popover
   *   - RailSourceButton  Jira / GitHub / Sentry / Claude / Cursor
   *   - RailSystemButton  Connections / Rules / Library / Settings
   *   - RailAppButton     editor / canvas / terminal multi-instance stacks
   * Shared chrome CSS (`.rail-btn` + states) imported globally from
   * `rail/chrome.css` so it reaches every child component without
   * crossing Svelte's per-component CSS scoping. */
  import './rail/chrome.css';
  import RailAppButton from '$lib/components/ui/RailAppButton.svelte';
  import RailTooltip from './rail/RailTooltip.svelte';
  import RailActiveHalo from './rail/RailActiveHalo.svelte';
  import RailIdentityAvatar from './rail/RailIdentityAvatar.svelte';
  import RailSourceButton from './rail/RailSourceButton.svelte';
  import RailSystemButton from './rail/RailSystemButton.svelte';
  /* Reuse brand SVGs that ConnectionsView renders so the rail's
   * vocabulary matches Connections. Claude / Cursor use curated PNGs
   * because their official marks are rich gradients that don't
   * distill cleanly to a single mono <path>. */
  import { SVG_GITHUB, SVG_JIRA, SVG_SENTRY } from '$lib/data';
  import { dragState, requestCanvasRailDrop } from '$lib/state/drag.svelte';
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
    cursorStatus?: CursorStatus | null;
    githubBadge?: number;
    jiraBadge?: number;
    sentryBadge?: number;
    dragActive?: boolean;
    claudeBusy?: boolean;
    cursorBusy?: boolean;
    onAgentDrop?: (kind: 'claude' | 'cursor', e: DragEvent) => void;
  }

  let {
    view = $bindable(),
    anythingConnected,
    statusLoading,
    anyRetrying = false,
    githubStatus,
    jiraStatus,
    sentryStatus,
    claudeStatus,
    cursorStatus,
    githubBadge = 0,
    jiraBadge = 0,
    sentryBadge = 0,
    dragActive = false,
    claudeBusy = false,
    cursorBusy = false,
    onAgentDrop
  }: Props = $props();

  /* Highlight Claude / Cursor rail button while a payload is dragging
   * over it. Cleared on `dragleave` (when cursor truly leaves the
   * button) and on `drop`. */
  let dropOverKind = $state<'claude' | 'cursor' | null>(null);

  /* WebKit hides custom `application/x-woom-*` mimes during dragover
   * (only standard mimes are exposed until `drop`), so we accept
   * anything that looks like a sane payload — files, uri-list, plain
   * text — and let the column-level drop decide if it's actually
   * routable. */
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
    /* dragleave fires when cursor enters child elements too;
     * relatedTarget can be null. We rely on next dragenter to re-set
     * highlight, so brief flicker on inner elements is acceptable. */
    dropOverKind = null;
  }
  function railDrop(kind: 'claude' | 'cursor', e: DragEvent) {
    dropOverKind = null;
    if (!onAgentDrop) return;
    e.preventDefault();
    view = kind === 'claude' ? 'claudeApp' : 'cursorApp';
    onAgentDrop(kind, e);
  }

  /* Canvas rail drop. The Canvas surface isn't in the DOM while user
   * is on another solo, so we can't forward the DragEvent — but
   * `dragState.payload` carries everything CanvasSurface needs. Queue
   * via `requestCanvasRailDrop`; the surface's $effect drains it at
   * viewport center after the view switch mounts it. */
  function onCanvasRailDrop(_e: DragEvent) {
    const payload = dragState.payload;
    if (!payload) return;
    requestCanvasRailDrop(payload);
  }

  /* Tooltip delegation. Any descendant button carrying `data-tooltip`
   * opts in. Mouseover/mouseout bubble up to the rail root unlike
   * mouseenter/mouseleave — exactly what we want for delegation. */
  let tooltip = $state<RailTooltip | undefined>();
  function onRailMouseOver(e: MouseEvent) {
    const t = (e.target as HTMLElement | null)?.closest?.('[data-tooltip]') as HTMLElement | null;
    if (t) tooltip?.show(t);
  }
  function onRailMouseOut(e: MouseEvent) {
    const t = (e.target as HTMLElement | null)?.closest?.('[data-tooltip]') as HTMLElement | null;
    const related = e.relatedTarget as HTMLElement | null;
    if (!t) return;
    if (related && t.contains(related)) return;
    tooltip?.hide();
  }

  /* Dynamic fade mask for `.rail-scroll`. Static gradient (always
   * fading top 14px) made the first icon (Jira) look perpetually
   * dimmed even when column wasn't actually overflowing. With
   * scroll-driven flags we paint top/bottom fade only when there's
   * content above/below the viewport. */
  let scrollEl = $state<HTMLDivElement | null>(null);
  let railEl = $state<HTMLElement | null>(null);
  let scrolledFromTop = $state(false);
  let moreBelow = $state(false);

  function recomputeFades() {
    const el = scrollEl;
    if (!el) return;
    /* `>= 1` instead of `> 0` because some engines emit fractional
     * scrollTop values (e.g. 0.6 on hidpi macbook trackpads). */
    scrolledFromTop = el.scrollTop >= 1;
    moreBelow = el.scrollTop + el.clientHeight < el.scrollHeight - 1;
  }

  $effect(() => {
    const el = scrollEl;
    if (!el) return;
    recomputeFades();
    el.addEventListener('scroll', recomputeFades, { passive: true });
    /* Recompute when column's children change height — new instances
     * added, popovers expanded — or when rail itself resizes. */
    const ro = new ResizeObserver(() => recomputeFades());
    ro.observe(el);
    for (const child of Array.from(el.children)) ro.observe(child as Element);
    return () => {
      el.removeEventListener('scroll', recomputeFades);
      ro.disconnect();
    };
  });
</script>

<aside
  class="rail"
  class:is-drag-active={dragActive}
  onmouseover={onRailMouseOver}
  onmouseout={onRailMouseOut}
  role="navigation"
  bind:this={railEl}
>
  <!-- v8 brand mark = "go home" affordance. -->
  <button
    class="rail-sigil"
    class:active={view === 'home'}
    onclick={() => (view = 'home')}
    aria-label="Home"
    data-tooltip="Home · ⌘0"
    data-view="home"
  >
    <img src="/woom-mark-transparent.png" alt="Woom" />
  </button>

  <RailActiveHalo
    {railEl}
    {scrollEl}
    {view}
    {scrolledFromTop}
    {moreBelow}
  />

  <!-- Scrollable middle column. Source / agent / tool buttons live
       here so the system cluster + avatar at the bottom stay anchored
       even when the user accumulates 10+ canvas / editor / terminal
       instances. -->
  <div
    class="rail-scroll"
    class:fade-top={scrolledFromTop}
    class:fade-bottom={moreBelow}
    bind:this={scrollEl}
  >
    <RailSourceButton
      view="jiraApp"
      label="Jira"
      tooltip="Jira · ⌘1"
      tone="var(--src-jira)"
      glow="rgba(79,142,255,0.40)"
      active={view === 'jiraApp'}
      badge={jiraBadge}
      onclick={() => (view = 'jiraApp')}
    >
      {#snippet icon()}
        <svg viewBox="0 0 24 24" fill="currentColor" stroke="none" aria-hidden="true">{@html SVG_JIRA}</svg>
      {/snippet}
    </RailSourceButton>

    <RailSourceButton
      view="githubApp"
      label="GitHub"
      tooltip="GitHub · ⌘2"
      tone="var(--src-github)"
      glow="rgba(181,132,255,0.40)"
      active={view === 'githubApp'}
      badge={githubBadge}
      onclick={() => (view = 'githubApp')}
    >
      {#snippet icon()}
        <svg viewBox="0 0 24 24" fill="currentColor" stroke="none" aria-hidden="true">{@html SVG_GITHUB}</svg>
      {/snippet}
    </RailSourceButton>

    <RailSourceButton
      view="sentryApp"
      label="Sentry"
      tooltip="Sentry · ⌘3"
      tone="var(--src-sentry)"
      glow="rgba(110, 80, 155, 0.42)"
      active={view === 'sentryApp'}
      badge={sentryBadge}
      onclick={() => (view = 'sentryApp')}
    >
      {#snippet icon()}
        <svg viewBox="0 0 24 24" fill="currentColor" stroke="none" aria-hidden="true">{@html SVG_SENTRY}</svg>
      {/snippet}
    </RailSourceButton>

    <div class="rail-divider"></div>

    <RailSourceButton
      view="claudeApp"
      label="Claude"
      tooltip="Claude · ⌘4 — drop to attach"
      tone="var(--src-claude)"
      glow="rgba(232,155,125,0.42)"
      active={view === 'claudeApp'}
      busy={claudeBusy}
      dragKind="claude"
      dropOver={dropOverKind === 'claude'}
      onclick={() => (view = 'claudeApp')}
      onDragEnter={railDragEnter}
      onDragOver={railDragOver}
      onDragLeave={railDragLeave}
      onDrop={railDrop}
    >
      {#snippet icon()}
        <img class="rail-brand-img" src="/brand-claude.png" alt="" aria-hidden="true" draggable="false" />
      {/snippet}
    </RailSourceButton>

    <RailSourceButton
      view="cursorApp"
      label="Cursor"
      tooltip="Cursor · ⌘5 — drop to attach"
      tone="var(--src-cursor)"
      glow="rgba(220,220,220,0.32)"
      active={view === 'cursorApp'}
      busy={cursorBusy}
      dragKind="cursor"
      dropOver={dropOverKind === 'cursor'}
      onclick={() => (view = 'cursorApp')}
      onDragEnter={railDragEnter}
      onDragOver={railDragOver}
      onDragLeave={railDragLeave}
      onDrop={railDrop}
    >
      {#snippet icon()}
        <img class="rail-brand-img rail-brand-img--cursor" src="/brand-cursor.png" alt="" aria-hidden="true" draggable="false" />
      {/snippet}
    </RailSourceButton>

    <div class="rail-divider"></div>

    <!-- Tools — multi-instance via RailAppButton: chevron expands an
         inline stack of sub-instances directly in the rail. -->
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
      tooltip="Canvas · ⌘7 — drop to pin as card"
      active={view === 'canvasApp'}
      tone="var(--src-canvas)"
      glow="rgba(125,194,213,0.40)"
      onActivate={() => (view = 'canvasApp')}
      onDropPayload={onCanvasRailDrop}
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
  </div>

  <!-- Foot cluster: system shortcuts + identity avatar. Outside scroll
       so it stays pinned to bottom regardless of how many tool
       instances live above. -->
  <div class="rail-foot">
    <RailSystemButton
      view="connections"
      label="Connections"
      tooltip={anyRetrying ? 'Connections — retrying…' : 'Connections'}
      active={view === 'connections'}
      dot={anyRetrying ? 'retrying' : (!anythingConnected && !statusLoading ? 'disconnected' : null)}
      onclick={() => (view = 'connections')}
    >
      {#snippet icon()}
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M21 11V7a2 2 0 0 0-2-2H5a2 2 0 0 0-2 2v10a2 2 0 0 0 2 2h6"/><circle cx="17" cy="17" r="3"/><path d="M19 17h2"/></svg>
      {/snippet}
    </RailSystemButton>

    <RailSystemButton
      view="rules"
      label="Rules"
      tooltip="Rules"
      active={view === 'rules'}
      onclick={() => (view = 'rules')}
    >
      {#snippet icon()}
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7"/><path d="M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4z"/></svg>
      {/snippet}
    </RailSystemButton>

    <RailSystemButton
      view="library"
      label="Library"
      tooltip="Library — skills & plugins"
      active={view === 'library'}
      onclick={() => (view = 'library')}
    >
      {#snippet icon()}
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M4 4h6v16H4z"/><path d="M14 4h2v16h-2z"/><path d="M18 5l2 .5L22 19l-2 .5z"/></svg>
      {/snippet}
    </RailSystemButton>

    <RailSystemButton
      view="settings"
      label="Settings"
      tooltip="Settings"
      active={view === 'settings'}
      onclick={() => (view = 'settings')}
    >
      {#snippet icon()}
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09a1.65 1.65 0 0 0-1-1.51 1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09a1.65 1.65 0 0 0 1.51-1 1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33h.01a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"/></svg>
      {/snippet}
    </RailSystemButton>

    <RailIdentityAvatar
      {githubStatus}
      {jiraStatus}
      {sentryStatus}
      {claudeStatus}
      {cursorStatus}
    />
  </div>
</aside>

<RailTooltip bind:this={tooltip} />

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
    height: 100%;
    min-height: 0;
    /* No `overflow: hidden` here — the active-button halo is rendered
       as a sibling of `.rail-scroll` (absolute-positioned) and needs
       room to bleed outside the scroll column's strict
       `overflow-y: auto` clip. */
  }

  .rail-scroll {
    flex: 1 1 auto;
    min-height: 0;
    width: 100%;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    overflow-y: auto;
    overflow-x: hidden;
    scrollbar-width: none;
    -ms-overflow-style: none;
    transition: mask-image 180ms var(--ease-out, ease-out),
                -webkit-mask-image 180ms var(--ease-out, ease-out);
  }
  .rail-scroll::-webkit-scrollbar { width: 0; height: 0; display: none; }

  .rail-scroll.fade-top {
    mask-image: linear-gradient(180deg,
      transparent 0, #000 16px, #000 100%);
    -webkit-mask-image: linear-gradient(180deg,
      transparent 0, #000 16px, #000 100%);
  }
  .rail-scroll.fade-bottom {
    mask-image: linear-gradient(180deg,
      #000 0, #000 calc(100% - 16px), transparent 100%);
    -webkit-mask-image: linear-gradient(180deg,
      #000 0, #000 calc(100% - 16px), transparent 100%);
  }
  .rail-scroll.fade-top.fade-bottom {
    mask-image: linear-gradient(180deg,
      transparent 0,
      #000 16px,
      #000 calc(100% - 16px),
      transparent 100%);
    -webkit-mask-image: linear-gradient(180deg,
      transparent 0,
      #000 16px,
      #000 calc(100% - 16px),
      transparent 100%);
  }

  .rail-foot {
    flex: 0 0 auto;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    padding-top: 6px;
  }

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
      inset 0 0 0 1px color-mix(in srgb, var(--accent) 40%, transparent);
  }
  .rail-sigil img {
    width: 30px;
    height: auto;
    max-width: 100%;
    display: block;
    filter: drop-shadow(0 1px 0 rgba(0, 0, 0, 0.35));
  }

  .rail-divider {
    width: 28px; height: 1px;
    background: var(--border);
    margin: 4px 0;
  }
</style>
