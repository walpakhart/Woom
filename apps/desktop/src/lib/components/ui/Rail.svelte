<script lang="ts">
  import RailAppButton from '$lib/components/ui/RailAppButton.svelte';
  import { fade } from 'svelte/transition';
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
    | 'library'
    | 'connections'
    | 'settings';

  interface Props {
    view: View;
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

  /* Floating tooltip — JS-positioned, rendered as a fixed-position
     sibling of `.rail-scroll`. The CSS pseudo-element approach used
     until now lived inside the scrollable middle column, and CSS
     doesn't allow `overflow-y: scroll` together with
     `overflow-x: visible` (both axes are forced to a non-visible
     value as soon as one is). That clipped every tooltip on a
     rail-btn inside `.rail-scroll`. A `position: fixed` element
     escapes ALL ancestor clip contexts, so it's the cleanest way
     to keep the hover affordance while preserving the scroll. */
  let tooltipText = $state('');
  let tooltipX = $state(0);
  let tooltipY = $state(0);
  let tooltipVisible = $state(false);

  function showTooltip(target: HTMLElement) {
    const text = target.getAttribute('data-tooltip');
    if (!text) return;
    const rect = target.getBoundingClientRect();
    tooltipText = text;
    /* Position: 8px to the right of the button, vertically centered.
       The `.rail-tooltip` itself uses `transform: translateY(-50%)`
       so we anchor on the button's vertical midpoint. */
    tooltipX = rect.right + 8;
    tooltipY = rect.top + rect.height / 2;
    tooltipVisible = true;
  }
  function hideTooltip() {
    tooltipVisible = false;
  }
  /* Event delegation: any descendant button carrying `data-tooltip`
     opts in. Mouseover/mouseout bubble up to the rail root unlike
     mouseenter/mouseleave — exactly what we want for delegation. */
  function onRailMouseOver(e: MouseEvent) {
    const t = (e.target as HTMLElement | null)?.closest?.('[data-tooltip]') as HTMLElement | null;
    if (t) showTooltip(t);
  }
  function onRailMouseOut(e: MouseEvent) {
    const t = (e.target as HTMLElement | null)?.closest?.('[data-tooltip]') as HTMLElement | null;
    const related = e.relatedTarget as HTMLElement | null;
    if (!t) return;
    if (related && t.contains(related)) return;
    hideTooltip();
  }

  /* Dynamic fade mask for `.rail-scroll`. The static gradient (always
     fading the top 14px) made the first icon (Jira) look perpetually
     dimmed even when the column wasn't actually overflowing. With
     scroll-driven flags we paint:
       - top fade only when the user has scrolled away from the top
       - bottom fade only when there's more content below the viewport
     When the column fits entirely, both flags stay false and no mask
     is applied → every icon reads at full contrast. */
  let scrollEl = $state<HTMLDivElement | null>(null);
  let scrolledFromTop = $state(false);
  let moreBelow = $state(false);

  function recomputeFades() {
    const el = scrollEl;
    if (!el) return;
    /* `>= 1` instead of `> 0` because some engines emit fractional
       scrollTop values (e.g. 0.6 on hidpi macbook trackpads). A
       sub-pixel offset shouldn't trip the fade. */
    scrolledFromTop = el.scrollTop >= 1;
    moreBelow = el.scrollTop + el.clientHeight < el.scrollHeight - 1;
  }

  $effect(() => {
    const el = scrollEl;
    if (!el) return;
    recomputeFades();
    el.addEventListener('scroll', recomputeFades, { passive: true });
    /* Recompute when the column's children change height — new
       instances added, popovers expanded — or when the rail itself
       resizes (window vertical zoom). */
    const ro = new ResizeObserver(() => recomputeFades());
    ro.observe(el);
    for (const child of Array.from(el.children)) ro.observe(child as Element);
    return () => {
      el.removeEventListener('scroll', recomputeFades);
      ro.disconnect();
    };
  });

  /* Active-button halo overlay.
   *
   * The button-attached `box-shadow: 0 0 22px ...` halo gets clipped
   * at the right edge of `.rail-scroll` because that container's
   * `overflow-y: auto` forces `overflow-x` to a clipped value too
   * (CSS spec). The button is centered in a narrow rail, so the
   * 22px outer glow can't fit. Painting it via a sibling overlay
   * outside `.rail-scroll` escapes the clip — the overlay sits on
   * top of any sibling content with its own `position: absolute`.
   *
   * We find the active button via `data-view="<view>"` attribute on
   * every rail-btn, pull its `getBoundingClientRect`, and translate
   * its center into a `top` offset relative to the rail. */
  let railEl = $state<HTMLElement | null>(null);
  let activeHaloY = $state(0);
  let activeHaloW = $state(44);
  let activeHaloH = $state(44);
  let activeHaloR = $state(11);
  let activeHaloGlow = $state('var(--accent-glow)');
  let activeHaloVisible = $state(false);
  /* Anchor box for the halo's CLIP container — matches the
     `.rail-scroll`'s vertical bounds in rail coordinates, padded
     by `HALO_BLUR_PAD` on top + bottom so the box-shadow's outer
     blur (≈22px) doesn't get sliced when the active button sits
     exactly at the scroll's top or bottom edge. The wrapper
     mirrors the scroll's `.fade-top` / `.fade-bottom` flags and
     applies a matching `mask-image` so the halo fades together
     with the scroll content when the user scrolls past the
     active item — outline and glow disappear in lockstep instead
     of the glow leaking through the fade region.
     Reading these from `scrollEl` keeps the clip in lockstep
     with layout: foot cluster height changes, scroll repositions,
     halo wrapper follows. */
  const HALO_BLUR_PAD = 30;
  let haloClipTop = $state(0);
  let haloClipHeight = $state(0);
  /* Whether the currently-haloed target sits inside `.rail-scroll`.
     The Home sigil (`.rail-sigil`) lives ABOVE the scroll column, so
     when it's active the halo wrapper must anchor to the sigil itself
     and skip the scroll-driven fade masks — those masks fade in
     wrapper-local coordinates calibrated for the scroll's top edge,
     which doesn't line up with the sigil. */
  let haloInScroll = $state(true);
  /* Halo X anchor — center of the rail's button column (not the
     clip wrapper, which extends past the rail's right edge). The
     wrapper is wider than the rail so the box-shadow has room to
     spread into the chat panel; the halo itself stays centered
     above the active button. */
  let haloAnchorX = $state(28);
  /* Stable identity of the currently-haloed button. Drives the
     `{#key}` block so the halo unmounts + remounts ONLY when the
     active target changes (not when the user simply scrolls and
     position shifts). Built from `aria-label` + `data-tooltip`
     because both rail-btns AND RailAppButton sub-instances expose
     them with stable per-instance content ("Editor Klimt" stays
     identifiable across scrolls / reflows). */
  let activeHaloKey = $state('');

  function recomputeHalo() {
    const rail = railEl;
    const scroll = scrollEl;
    if (!rail) {
      activeHaloVisible = false;
      return;
    }
    /* Center of the rail's button column — used as the halo's
       horizontal anchor inside the (wider) clip wrapper. */
    haloAnchorX = rail.clientWidth / 2;
    /* Find any descendant that's currently `active`. We use a class
       selector instead of `[data-view]` because RailAppButton (used
       by Editor / Canvas / Terminal) renders the button internally
       and doesn't carry a single `data-view` — its primary becomes
       `.active` when both the kind and the primary instance are
       selected, and a sub-instance becomes `.active` otherwise.
       Either way the class identifies the right node. The rail-sigil
       (Home) also flips `.active` so it's covered too. */
    const target = rail.querySelector<HTMLElement>(
      '.rail-btn.active, .rail-sigil.active'
    );
    if (!target) {
      activeHaloVisible = false;
      activeHaloKey = '';
      return;
    }
    const railRect = rail.getBoundingClientRect();
    const tRect = target.getBoundingClientRect();
    /* Anchor the clip wrapper differently depending on whether the
       active target lives inside `.rail-scroll` or above it (the
       Home sigil). Inside the scroll, we snap to the scroll's bounds
       padded by HALO_BLUR_PAD so the halo fades with scroll content
       via mask-image. Outside (sigil), we snap to the target itself
       padded by HALO_BLUR_PAD — anchoring to the scroll's bounds
       clipped the sigil's halo at the top (the user-reported bug),
       since the sigil sits above the scroll's top with less than
       HALO_BLUR_PAD breathing room. */
    haloInScroll = !!scroll && scroll.contains(target);
    if (scroll && haloInScroll) {
      const scrollRect = scroll.getBoundingClientRect();
      haloClipTop = scrollRect.top - railRect.top - HALO_BLUR_PAD;
      haloClipHeight = scrollRect.height + HALO_BLUR_PAD * 2;
    } else {
      haloClipTop = tRect.top - railRect.top - HALO_BLUR_PAD;
      haloClipHeight = tRect.height + HALO_BLUR_PAD * 2;
    }
    activeHaloY = tRect.top + tRect.height / 2 - railRect.top;
    /* Mirror the active button's exact size + corner radius so the
       halo overlay traces the same chassis (44×44/11 for primary
       rail-btns, 38×38/9 for RailAppButton's sub-instances). Reading
       this from the DOM means we don't have to teach the overlay
       about each kind's dimensions. */
    activeHaloW = tRect.width;
    activeHaloH = tRect.height;
    const cs = getComputedStyle(target);
    const radius = parseFloat(cs.borderTopLeftRadius || '11');
    activeHaloR = Number.isFinite(radius) ? radius : 11;
    /* Pull the glow tone from the active button's computed style so
       each source keeps its branded hue (Jira blue / Sentry purple /
       Editor terracotta / …) without the overlay component having
       to know about every variable. The W sigil doesn't set
       `--rail-glow` — fall back to the global `--accent-glow` so
       Home gets the right accent tone too. */
    const rawGlow = cs.getPropertyValue('--rail-glow').trim();
    if (rawGlow) {
      activeHaloGlow = rawGlow;
    } else {
      const accentGlow = cs.getPropertyValue('--accent-glow').trim();
      activeHaloGlow = accentGlow || 'rgba(120, 200, 255, 0.45)';
    }
    /* Identity key — combines aria-label + data-tooltip + kind so
       sub-instances of the same kind ("Editor Klimt" vs "Editor
       Hilma") get distinct keys even though their CSS class set
       is identical. Position is intentionally NOT in the key. */
    const aria = target.getAttribute('aria-label') ?? '';
    const tip = target.getAttribute('data-tooltip') ?? '';
    activeHaloKey = `${aria}|${tip}`;
    activeHaloVisible = true;
  }

  /* Batch recomputes via rAF so that bursts of scroll / mutation
     events collapse into one DOM read per frame. Without this the
     scroll handler + mutation observer + resize observer can fire
     multiple times in a single frame and each call to
     `getBoundingClientRect` triggers a layout flush — which is
     exactly the "halo jitters with many instances" symptom users
     reported when stacks were big enough to scroll. */
  let _haloRaf: number | null = null;
  function scheduleHaloRecompute() {
    if (_haloRaf != null) return;
    _haloRaf = requestAnimationFrame(() => {
      _haloRaf = null;
      recomputeHalo();
    });
  }

  $effect(() => {
    /* React to view changes — re-find the active button and reposition. */
    void view;
    /* Wait a frame so DOM reflects the new `class:active` after
       Svelte's update. Without rAF the overlay would briefly point
       at the previously-active button on every nav. */
    scheduleHaloRecompute();
  });

  $effect(() => {
    /* Track scroll, resize, AND DOM mutations so the halo follows
       the active button when the user scrolls past it, the rail
       reflows, or the user adds / removes / expands instance
       stacks (which shifts every button below the change point).
       The MutationObserver also covers `class:active` swaps —
       e.g. activating a sub-instance flips `.active` on a sibling
       without going through the `view` reactive path. */
    const rail = railEl;
    const scroll = scrollEl;
    if (!rail) return;
    scheduleHaloRecompute();
    const onScroll = () => scheduleHaloRecompute();
    scroll?.addEventListener('scroll', onScroll, { passive: true });
    const ro = new ResizeObserver(scheduleHaloRecompute);
    ro.observe(rail);
    if (scroll) ro.observe(scroll);
    const mo = new MutationObserver(scheduleHaloRecompute);
    mo.observe(rail, {
      subtree: true,
      childList: true,
      attributes: true,
      attributeFilter: ['class']
    });
    return () => {
      scroll?.removeEventListener('scroll', onScroll);
      ro.disconnect();
      mo.disconnect();
      if (_haloRaf != null) {
        cancelAnimationFrame(_haloRaf);
        _haloRaf = null;
      }
    };
  });

</script>

<aside
  class="rail"
  onmouseover={onRailMouseOver}
  onmouseout={onRailMouseOut}
  role="navigation"
  bind:this={railEl}
>
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
    data-view="home"
  >
    <img src="/woom-mark-transparent.png" alt="Woom" />
  </button>

  <!-- Active-button halo + its clip wrapper. Vertically the wrapper
       matches `.rail-scroll`'s bounds (so the halo's box-shadow
       gets clipped at the same top/bottom edges as the active
       button's inset outline — they hide together when the user
       scrolls past the active item). Horizontally the wrapper
       extends 30px past the rail's right edge so the box-shadow's
       blur can fully spread into the chat panel without being
       clipped at the rail's right border. The wrapper sits BEFORE
       `.rail-scroll` in DOM so the rail-btns naturally stack above
       it (no z-index gymnastics).
       `{#key activeHaloKey}` causes a clean unmount + remount each
       time the active button changes — Svelte's `fade` transition
       runs both old (out) and new (in) at the same time, giving the
       "outline appears / disappears" feel the user asked for instead
       of a slide between positions. Position changes from scroll /
       reflow keep the same key, so they don't trigger the animation. -->
  <div
    class="rail-halo-clip"
    class:fade-top={scrolledFromTop && haloInScroll}
    class:fade-bottom={moreBelow && haloInScroll}
    style="top: {haloClipTop}px; height: {haloClipHeight}px;"
    aria-hidden="true"
  >
    {#key activeHaloKey}
      {#if activeHaloVisible}
        <div
          class="rail-halo"
          style="top: {activeHaloY - haloClipTop}px; left: {haloAnchorX}px; width: {activeHaloW}px; height: {activeHaloH}px; border-radius: {activeHaloR}px; --rail-glow: {activeHaloGlow};"
          in:fade={{ duration: 160 }}
          out:fade={{ duration: 120 }}
        ></div>
      {/if}
    {/key}
  </div>

  <!-- Scrollable middle column. Sources / agents / tools live here so
       the system cluster + avatar at the bottom stay anchored even
       when the user has accumulated 10+ canvas / editor / terminal
       instances. Scrollbar is hidden via `scrollbar-width: none` +
       `::-webkit-scrollbar { display: none }` — the gradient masks
       below telegraph "there's more above / below" without painting
       a chrome track. -->
  <div
    class="rail-scroll"
    class:fade-top={scrolledFromTop}
    class:fade-bottom={moreBelow}
    bind:this={scrollEl}
  >
  <!-- Source solos -->
  <button
    class="rail-btn"
    class:active={view === 'jiraApp'}
    style="--rail-tone: var(--src-jira); --rail-glow: rgba(79,142,255,0.40);"
    data-tooltip="Jira · ⌘1"
    data-view="jiraApp"
    onclick={() => (view = 'jiraApp')}
    aria-label="Jira"
  >
    <svg viewBox="0 0 24 24" fill="currentColor" stroke="none" aria-hidden="true">{@html SVG_JIRA}</svg>
  </button>

  <button
    class="rail-btn"
    class:active={view === 'githubApp'}
    style="--rail-tone: var(--src-github); --rail-glow: rgba(181,132,255,0.40);"
    data-tooltip="GitHub · ⌘2"
    data-view="githubApp"
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
    data-view="sentryApp"
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
    data-view="claudeApp"
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
    data-view="cursorApp"
    onclick={() => (view = 'cursorApp')}
    ondragenter={(e) => railDragEnter('cursor', e)}
    ondragover={(e) => railDragOver('cursor', e)}
    ondragleave={railDragLeave}
    ondrop={(e) => railDrop('cursor', e)}
    aria-label="Cursor"
  >
    <!-- Same rationale as Claude above — Anysphere's mark is a faceted
         3D hex with subtle gradients, baked into the brand PNG. -->
    <img class="rail-brand-img rail-brand-img--cursor" src="/brand-cursor.png" alt="" aria-hidden="true" draggable="false" />
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

  </div>
  <!-- /rail-scroll -->

  <!-- Foot cluster: system shortcuts + identity avatar. Sits outside
       the scroll area so it stays pinned to the bottom regardless of
       how many tool instances live above. -->
  <div class="rail-foot">
  <!-- System cluster -->
  <button
    class="rail-btn"
    class:active={view === 'connections'}
    data-tooltip={anyRetrying ? 'Connections — retrying…' : 'Connections'}
    data-view="connections"
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
    data-view="rules"
    onclick={() => (view = 'rules')}
    aria-label="Rules"
  >
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7"/><path d="M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4z"/></svg>
  </button>

  <button
    class="rail-btn"
    class:active={view === 'library'}
    data-tooltip="Library — skills & plugins"
    data-view="library"
    onclick={() => (view = 'library')}
    aria-label="Library"
  >
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M4 4h6v16H4z"/><path d="M14 4h2v16h-2z"/><path d="M18 5l2 .5L22 19l-2 .5z"/></svg>
  </button>

  <button
    class="rail-btn"
    class:active={view === 'settings'}
    data-tooltip="Settings"
    data-view="settings"
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
  </div>
  <!-- /rail-foot -->
</aside>

{#if tooltipVisible}
  <!-- Floating tooltip — MUST be rendered OUTSIDE `<aside class="rail">`
       so it isn't trapped by the rail's containing block. The rail has
       `backdrop-filter: blur(12px)` which (per CSS spec) creates a new
       containing block for fixed-positioned descendants. That, combined
       with the rail's `overflow: hidden`, was clipping the tooltip even
       though it was `position: fixed`. Placed at the component root
       (sibling of `<aside>`), the tooltip's containing block is the
       viewport again — math (`rect.right + 8`, `rect.top + h/2`) lines
       up cleanly. -->
  <div
    class="rail-tooltip"
    role="tooltip"
    style="left: {tooltipX}px; top: {tooltipY}px;"
  >
    {tooltipText}
  </div>
{/if}

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
       `overflow-y: auto` clip. `.rail-scroll` handles its own scroll
       clipping; the rail wrapper just hosts the layout + the halo. */
  }

  /* Scrollable middle column for source / agent / tool buttons. Hides
     the scrollbar chrome on every engine — `scrollbar-width: none`
     covers Firefox, `::-webkit-scrollbar { display:none }` covers
     WebKit / Chromium. The container itself remains a flex column so
     dividers + RailAppButton stacks inherit the same 6px gap the rail
     used to set on its top-level flex. Soft fade masks at the top /
     bottom edges hint at the overflow without a visible track. */
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
    /* No static mask: a permanent top/bottom fade dimmed the first
       icon (Jira) even when the column wasn't overflowing. Fades
       are now conditional — see `.fade-top` / `.fade-bottom` — and
       only paint when there's actually hidden content in that
       direction. The mask gradient transitions softly so adding /
       removing one edge doesn't pop. */
    transition: mask-image 180ms var(--ease-out, ease-out),
                -webkit-mask-image 180ms var(--ease-out, ease-out);
  }
  .rail-scroll::-webkit-scrollbar { width: 0; height: 0; display: none; }

  /* Edge fades — applied only when the matching direction has hidden
     content. Combining both gives the original symmetric fade; one
     alone gives an asymmetric hint. */
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

  /* Foot cluster — pinned to the bottom of the rail; mirrors the flex
     gap of the scroll column so the visual rhythm stays continuous. */
  .rail-foot {
    flex: 0 0 auto;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    padding-top: 6px;
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
    /* Outer glow lives on `.rail-halo` overlay (same as `.rail-btn.active`)
       so the W gets a halo of identical weight to every other rail icon.
       Only the inset accent ring stays on the sigil itself. */
    box-shadow:
      inset 0 0 0 1px color-mix(in srgb, var(--accent) 40%, transparent);
  }
  .rail-sigil img {
    /* The W is wider than tall, so we anchor to a fixed width and
       let height auto-derive. 30px reads at a comparable visual
       weight to the 19px square nav icons below despite the glyph's
       low aspect ratio. */
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
  /* Cursor's white-on-transparent icon is too bright at the default
     opacity — dial it down further so it reads as clearly inactive. */
  :global(.rail-brand-img--cursor) {
    filter: grayscale(1) opacity(0.35);
  }
  :global(.rail-btn:hover .rail-brand-img--cursor),
  :global(.rail-btn.active .rail-brand-img--cursor) {
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
    /* Only the inset 1px ring stays on the button — the 22px outer
       halo lives on `.rail-halo` (rendered outside `.rail-scroll`
       so it isn't clipped at the scroll container's right edge).
       The overlay is positioned + sized to exactly match this
       button, so visually it's the same as having the box-shadow
       here. */
    box-shadow:
      inset 0 0 0 1px color-mix(in srgb, var(--rail-tone) 35%, transparent);
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
  @media (prefers-reduced-motion: reduce) {
    /* Even with motion off, fade should still feel intentional —
       80ms linear reads as a flicker. Keep the same easing as the
       default just shrink the duration. */
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

  /* Active-button halo overlay.
   *
   * Mirrors the EXACT geometry of a 44×44 rail-btn (same width,
   * height, border-radius) and re-applies the original
   * `box-shadow: 0 0 22px var(--rail-glow)` that used to live on
   * `.rail-btn.active`. Because this element is a sibling of
   * `.rail-scroll` (not a descendant), the 22px outer blur can
   * extend rightward without being clipped at the scroll
   * container's right edge — that was the original "old was
   * ideal, just clipping was broken" behavior the user wanted
   * back.
   *
   * The element itself is transparent (no fill, no inset border —
   * those still live on the button so the chassis renders inside
   * the scroll area). It only paints the outer halo. */
  /* Halo clip wrapper. `overflow: hidden` clips the halo's outer
     blur at the wrapper's bounds:
       - Vertically: starts 30px ABOVE the scroll's top and ends
         30px BELOW the scroll's bottom (HALO_BLUR_PAD in JS).
         That extra padding gives the 22px box-shadow blur room
         to spread when the active button sits exactly at the
         scroll's top / bottom edge — without the pad, the halo
         was sliced flat at the scroll's top (the "the glow is
         cut off at the top of the scrollable sidebar" complaint).
       - Inside that padded vertical range, a `mask-image` mirrors
         the `.rail-scroll`'s `.fade-top` / `.fade-bottom` masks
         in WRAPPER-LOCAL coordinates: the gradient transitions
         start at `30px` (= scroll's top in wrapper coords) and
         finish at `46px` (= scroll y=16 = end of scroll's fade
         transition). Above the wrapper-local 30px, when fade-top
         is active, the mask is fully transparent — the halo
         extending into "above-scroll" space is hidden, matching
         the scroll's behavior of fading out content there. With
         no fade, no mask is applied → halo extends freely and
         the user sees the original "ideal" outer-glow look.
       - Horizontally: `left: 0` and `right: -30px` extends the
         wrapper 30px past the rail's right edge so the 22px outer
         blur has room to fully render before being cut.
     The wrapper itself paints nothing — just defines the clip box. */
  .rail-halo-clip {
    position: absolute;
    left: 0;
    right: -30px;            /* extend past rail's right edge */
    pointer-events: none;
    z-index: 0;
    overflow: hidden;
    transition: mask-image 180ms var(--ease-out, ease-out),
                -webkit-mask-image 180ms var(--ease-out, ease-out);
  }
  .rail-halo-clip.fade-top {
    mask-image: linear-gradient(180deg,
      transparent 0,
      transparent 30px,
      #000 46px,
      #000 100%);
    -webkit-mask-image: linear-gradient(180deg,
      transparent 0,
      transparent 30px,
      #000 46px,
      #000 100%);
  }
  .rail-halo-clip.fade-bottom {
    mask-image: linear-gradient(180deg,
      #000 0,
      #000 calc(100% - 46px),
      transparent calc(100% - 30px),
      transparent 100%);
    -webkit-mask-image: linear-gradient(180deg,
      #000 0,
      #000 calc(100% - 46px),
      transparent calc(100% - 30px),
      transparent 100%);
  }
  .rail-halo-clip.fade-top.fade-bottom {
    mask-image: linear-gradient(180deg,
      transparent 0,
      transparent 30px,
      #000 46px,
      #000 calc(100% - 46px),
      transparent calc(100% - 30px),
      transparent 100%);
    -webkit-mask-image: linear-gradient(180deg,
      transparent 0,
      transparent 30px,
      #000 46px,
      #000 calc(100% - 46px),
      transparent calc(100% - 30px),
      transparent 100%);
  }
  .rail-halo {
    position: absolute;
    /* `left` is set inline in pixels (= rail's button-column center),
       NOT 50% — the wrapper extends past the rail's right edge so
       a percentage-based center would be off by half the extension.
       `top` is also set inline relative to the wrapper. */
    pointer-events: none;
    transform: translate(-50%, -50%);
    box-shadow: 0 0 22px color-mix(in srgb, var(--rail-glow) 60%, transparent);
    /* No position transition — Svelte's `{#key}` + `fade` transition
       in the template drives "fade out at old, fade in at new" so
       the halo behaves like the inset border ring (appears /
       disappears in place rather than skidding between buttons).
       The only thing CSS still animates is the glow tone for
       reduced-motion users where the JS transition is short-
       circuited. */
  }

  /* Floating tooltip — JS-positioned via `tooltipX/tooltipY` so it
     escapes every ancestor clip context (including `.rail-scroll`'s
     `overflow-y: auto`, which until now silently clipped every
     rail-btn pseudo-tooltip). Anchored to the right of the button
     at vertical midpoint via `translateY(-50%)`. `pointer-events:
     none` so the tooltip itself never intercepts hover. */
  .rail-tooltip {
    position: fixed;
    padding: 4px 10px;
    background: var(--bg-3);
    border: 1px solid var(--border-neutral-hi);
    border-radius: 6px;
    font-size: 11.5px;
    color: var(--text-0);
    white-space: nowrap;
    pointer-events: none;
    z-index: 9999;
    box-shadow: var(--shadow-1);
    transform: translateY(-50%);
    /* Subtle entrance — gives the user a visual cue the tooltip
       isn't a stuck-from-the-last-hover relic. */
    animation: rail-tooltip-in 120ms var(--ease-out, ease-out);
  }
  @keyframes rail-tooltip-in {
    from { opacity: 0; transform: translate(-2px, -50%); }
    to   { opacity: 1; transform: translate(0, -50%); }
  }
  @media (prefers-reduced-motion: reduce) {
    .rail-tooltip { animation: none; }
  }
</style>
