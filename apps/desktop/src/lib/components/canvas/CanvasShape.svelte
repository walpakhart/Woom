<script lang="ts">
  // CanvasShape — render-only dispatcher for the M-canvas-2 shape catalog.
  //
  // Each shape is a single absolutely-positioned `<div class="cv-shape">`
  // sized to its bbox in canvas pixels. The stage transform (translate +
  // scale) on the parent .stage gives us the camera; this component just
  // draws shapes in their own native pixels and lets the parent do the
  // CSS-transform scaling.
  //
  // Pointer interaction (select / translate / resize) lives on the parent
  // CanvasSurface — this file is purely visual. It exposes:
  //   - `data-shape-id` so hit-testing can climb back to the shape model.
  //   - A `selected` prop so the surrounding selection ring renders.
  //   - A `dim` prop the marquee tool sets while dragging the marquee
  //     across un-selected shapes (for visual feedback).
  //
  // Every shape's `props` is `Record<string, unknown>` at the type level
  // (see canvas.svelte.ts). We narrow per-kind here with small accessor
  // helpers — adding fields = update the helper, no schema migration
  // needed because shapes are plain JSON. Defaults come from the
  // `defaultPropsFor` table (canvas.svelte.ts) at creation, so a missing
  // field is a corrupt save, not a normal case.

  import type { Shape } from '$lib/state/canvas.svelte';
  import { patchShape } from '$lib/state/canvas.svelte';
  import { marked } from 'marked';
  import { getStroke } from 'perfect-freehand';
  import { findJiraItem, findGithubItem, findSentryIssue, findChatMessage } from '$lib/services/liveCardData';

  interface Props {
    shape: Shape;
    selected: boolean;
    /** Marquee in progress and this shape is not (yet) inside the rect.
     *  Renders at reduced opacity so the user sees what's about to be
     *  picked vs ignored. */
    dim: boolean;
    /** Camera zoom — used so stroke widths and selection-ring outlines
     *  stay one CSS pixel regardless of zoom. Without this, zooming out
     *  makes 2 px strokes invisible and zooming in turns them into
     *  fat blobs. */
    zoom: number;
    /** Canvas id this shape belongs to. Required for inline-edit
     *  (text / sticky / mermaid / code) which writes back through
     *  `patchShape(canvasId, ...)`. Optional preview shapes
     *  (drawingPreview, freehandPreview) pass null because they're
     *  not yet committed and shouldn't be editable. */
    canvasId: string | null;
    /** Optional double-click handler for live cards. When the user
     *  double-clicks a `*-card` shape, the parent column dispatches to
     *  the right source-specific opener (Jira pane, GH focus pane,
     *  editor open, agent column scroll). Only live-card kinds bind
     *  this — primitives ignore it. */
    onCardOpen?: (shape: Shape) => void;
  }

  let { shape, selected, dim, zoom, canvasId, onCardOpen }: Props = $props();

  /* Live-card kinds: a click here means "open the source object the
     card represents". Stored as a Set for O(1) check during dblclick. */
  const LIVE_CARD_KINDS: ReadonlySet<string> = new Set([
    'jira-card', 'github-pr-card', 'github-issue-card',
    'sentry-event-card', 'file-card', 'chat-message-card'
  ]);
  const isLiveCard = $derived(LIVE_CARD_KINDS.has(shape.kind));

  /* Editable kinds — double-click swaps the rendered body for an
     inline `<textarea>`. The text content of these shapes lives in
     a single `props` field which we serialize here:
       text/sticky      → props.text / props.markdown
       mermaid/dot      → props.source
       code             → props.source
       frame            → props.title (single-line)
     A different shape kind would just get a no-op dblclick. */
  const EDITABLE_KINDS: ReadonlySet<string> = new Set([
    'text', 'sticky', 'mermaid', 'code', 'frame'
  ]);
  const isEditable = $derived(EDITABLE_KINDS.has(shape.kind) && canvasId !== null);

  /** Inline-edit state. `editing` flips on dblclick of an editable
   *  shape; `editDraft` mirrors what the user has typed so we can
   *  commit on blur / ⌘Enter or discard on Esc without touching
   *  global state mid-edit. */
  let editing = $state(false);
  let editDraft = $state('');

  function readShapeText(s: Shape): string {
    const p = s.props as Record<string, unknown>;
    switch (s.kind) {
      case 'text':    return typeof p.text === 'string' ? p.text : '';
      case 'sticky':  return typeof p.markdown === 'string' ? p.markdown : '';
      case 'mermaid':
      case 'dot':
      case 'code':    return typeof p.source === 'string' ? p.source : '';
      case 'frame':   return typeof p.title === 'string' ? p.title : '';
      default:        return '';
    }
  }

  function shapeTextPropKey(s: Shape): 'text' | 'markdown' | 'source' | 'title' | null {
    switch (s.kind) {
      case 'text':    return 'text';
      case 'sticky':  return 'markdown';
      case 'mermaid':
      case 'dot':
      case 'code':    return 'source';
      case 'frame':   return 'title';
      default:        return null;
    }
  }

  function enterEdit() {
    if (!isEditable) return;
    editDraft = readShapeText(shape);
    editing = true;
  }

  function commitEdit() {
    if (!editing || !canvasId) { editing = false; return; }
    const key = shapeTextPropKey(shape);
    if (key) {
      const cur = shape.props as Record<string, unknown>;
      patchShape(canvasId, shape.id, {
        props: { ...cur, [key]: editDraft }
      });
    }
    editing = false;
  }

  function cancelEdit() {
    editing = false;
    editDraft = '';
  }

  function onDoubleClick(e: MouseEvent) {
    /* Pan / draw / drag gestures don't double-fire dblclick (the
       browser only emits it after two clicks within ~500ms on the
       same target). So we don't need to gate on gesture state — if
       this fired, it was an honest double-click.
       Live-card and editable are mutually exclusive by kind, so the
       order here is just convention. */
    if (isLiveCard) {
      if (!onCardOpen) return;
      e.stopPropagation();
      onCardOpen(shape);
      return;
    }
    if (isEditable) {
      e.stopPropagation();
      enterEdit();
      return;
    }
  }

  /* Counter-zoom factor — divides any visual that should stay screen-sized
     (selection ring, handle markers) by the camera scale, so 1 / zoom CSS
     px in canvas space = 1 CSS px on screen. */
  const cz = $derived(1 / Math.max(zoom, 0.0001));

  /* Per-kind narrowed views of `props`. Keeping these as $derived keeps
     the templates legible — `pRect.fill` instead of `(shape.props as
     Record<string, unknown>).fill`. If a field is the wrong type at
     runtime (corrupt save), we fall back to a sane default rather than
     throwing — the renderer should never crash on bad data. */
  const pRect = $derived.by(() => {
    const p = shape.props as Record<string, unknown>;
    return {
      fill: typeof p.fill === 'string' ? p.fill : null,
      stroke: typeof p.stroke === 'string' ? p.stroke : 'border-hi',
      strokeWidth: typeof p.strokeWidth === 'number' ? p.strokeWidth : 2,
      radius: typeof p.radius === 'number' ? p.radius : 6
    };
  });

  const pEllipse = $derived.by(() => {
    const p = shape.props as Record<string, unknown>;
    return {
      fill: typeof p.fill === 'string' ? p.fill : null,
      stroke: typeof p.stroke === 'string' ? p.stroke : 'border-hi',
      strokeWidth: typeof p.strokeWidth === 'number' ? p.strokeWidth : 2
    };
  });

  /* Lines / arrows store endpoints in **bbox-local** canvas px, so the
     shape's bbox stays the AABB of the line and resize-by-corner naturally
     scales the endpoints. `from` / `to` default to the corner-to-corner
     diagonal so a freshly-created line with default props is still
     visible. */
  const pLine = $derived.by(() => {
    const p = shape.props as Record<string, unknown>;
    const from = (p.from as { x: number; y: number } | undefined) ?? { x: 0, y: 0 };
    const to = (p.to as { x: number; y: number } | undefined) ?? { x: shape.w, y: shape.h };
    return {
      from,
      to,
      thickness: typeof p.thickness === 'number' ? p.thickness : 2,
      dash: typeof p.dash === 'string' ? p.dash : 'solid'
    };
  });

  const pArrow = $derived.by(() => {
    const p = shape.props as Record<string, unknown>;
    const from = (p.from as { x: number; y: number } | undefined) ?? { x: 0, y: 0 };
    const to = (p.to as { x: number; y: number } | undefined) ?? { x: shape.w, y: shape.h };
    return {
      from,
      to,
      thickness: typeof p.thickness === 'number' ? p.thickness : 2,
      head: typeof p.head === 'string' ? p.head : 'filled'
    };
  });

  const pText = $derived.by(() => {
    const p = shape.props as Record<string, unknown>;
    return {
      text: typeof p.text === 'string' ? p.text : '',
      fontSize: typeof p.fontSize === 'number' ? p.fontSize : 16,
      fontWeight: typeof p.fontWeight === 'number' ? p.fontWeight : 500,
      align: typeof p.align === 'string' ? p.align : 'left',
      color: typeof p.color === 'string' ? p.color : null
    };
  });

  const pSticky = $derived.by(() => {
    const p = shape.props as Record<string, unknown>;
    return {
      body: typeof p.markdown === 'string' ? p.markdown : '',
      tint: typeof p.tint === 'string' ? p.tint : 'forge',
      fontSize: typeof p.fontSize === 'number' ? p.fontSize : 13
    };
  });

  /* Sticky markdown is rendered by the same `marked` instance the chat
     column uses (already a project dep). We trust the markdown source —
     it's either typed by the user or written by an MCP-linked agent
     locally; nothing crosses a network boundary into the canvas, so we
     skip a sanitizer and accept the standard CommonMark output. */
  const stickyHtml = $derived(
    marked.parse(pSticky.body, { gfm: true, breaks: true, async: false }) as string
  );

  const pMermaid = $derived.by(() => {
    const p = shape.props as Record<string, unknown>;
    return {
      source: typeof p.source === 'string' ? p.source : '',
      theme: typeof p.theme === 'string' ? p.theme : 'forge-dark'
    };
  });

  const pCode = $derived.by(() => {
    const p = shape.props as Record<string, unknown>;
    return {
      source: typeof p.source === 'string' ? p.source : '',
      language: typeof p.language === 'string' ? p.language : 'text',
      lineNumbers: p.lineNumbers !== false /* default true */
    };
  });

  const pImage = $derived.by(() => {
    const p = shape.props as Record<string, unknown>;
    return {
      dataUrl: typeof p.dataUrl === 'string' ? p.dataUrl : '',
      alt: typeof p.alt === 'string' ? p.alt : null
    };
  });

  const pFreehand = $derived.by(() => {
    const p = shape.props as Record<string, unknown>;
    const points = Array.isArray(p.points)
      ? (p.points as unknown[]).filter(
          (pt): pt is [number, number, number] =>
            Array.isArray(pt) && pt.length >= 2 && typeof pt[0] === 'number' && typeof pt[1] === 'number'
        )
      : [];
    return {
      points,
      color: typeof p.color === 'string' ? p.color : 'text-0',
      thickness: typeof p.thickness === 'number' ? p.thickness : 2,
      smoothing: typeof p.smoothing === 'number' ? p.smoothing : 0.5
    };
  });

  /* Convert the freehand sample list into a smoothed SVG path. The
     `getStroke` output is a polygon (outline) — we close it and
     fill-only render so the brush has a tapering, natural feel.
     Re-runs on every props change (Svelte's reactivity) so live
     drawing updates without us wiring a separate effect. */
  const freehandPath = $derived.by(() => {
    if (pFreehand.points.length === 0) return '';
    const stroke = getStroke(pFreehand.points, {
      size: pFreehand.thickness * 2,
      thinning: 0.5,
      smoothing: pFreehand.smoothing,
      streamline: 0.5,
      simulatePressure: pFreehand.points[0].length < 3
    });
    if (stroke.length === 0) return '';
    const d = stroke
      .map(([x, y], i) => `${i === 0 ? 'M' : 'L'} ${x.toFixed(2)} ${y.toFixed(2)}`)
      .join(' ');
    return `${d} Z`;
  });

  /* Live cards — narrowed per-kind reads of `props`. Live-data lookups
     happen at render time so a freshly fetched ticket / PR / event is
     reflected without us wiring a per-shape subscription. The
     `inboxState` deps inside the helpers make these $derived values
     re-evaluate on every relevant change. */
  const pJiraCard = $derived.by(() => {
    const p = shape.props as Record<string, unknown>;
    const ticketKey = typeof p.ticketKey === 'string' ? p.ticketKey : '';
    const snapshot = (p.snapshot ?? null) as {
      key?: string; summary?: string; status?: string; priority?: string | null;
      issueType?: string; assignee?: string | null; updated?: string;
    } | null;
    const live = findJiraItem(ticketKey);
    return {
      ticketKey,
      summary: live?.summary ?? snapshot?.summary ?? '',
      status: live?.status ?? snapshot?.status ?? '',
      priority: live?.priority ?? snapshot?.priority ?? null,
      issueType: live?.issue_type ?? snapshot?.issueType ?? '',
      assignee: live?.assignee?.display_name ?? snapshot?.assignee ?? null,
      stale: !live  /* true when we're rendering snapshot only */
    };
  });

  const pGhCard = $derived.by(() => {
    const p = shape.props as Record<string, unknown>;
    const owner = typeof p.owner === 'string' ? p.owner : '';
    const repo = typeof p.repo === 'string' ? p.repo : '';
    const number = typeof p.number === 'number' ? p.number : 0;
    const snapshot = (p.snapshot ?? null) as {
      title?: string; state?: string; merged?: boolean; draft?: boolean;
      author?: string | null; comments?: number; updated?: string;
    } | null;
    const live = findGithubItem(owner, repo, number);
    return {
      owner, repo, number,
      title: live?.title ?? snapshot?.title ?? '',
      state: live?.state ?? snapshot?.state ?? '',
      merged: live?.merged ?? snapshot?.merged ?? false,
      draft: live?.draft ?? snapshot?.draft ?? false,
      author: live?.author?.login ?? snapshot?.author ?? null,
      comments: live?.comments ?? snapshot?.comments ?? 0,
      stale: !live
    };
  });

  const pSentryCard = $derived.by(() => {
    const p = shape.props as Record<string, unknown>;
    const issueId = typeof p.issueId === 'string' ? p.issueId : '';
    const shortId = typeof p.shortId === 'string' ? p.shortId : null;
    const snapshot = (p.snapshot ?? null) as {
      title?: string; level?: string; status?: string; count?: string;
      culprit?: string | null; project?: string;
    } | null;
    const live = findSentryIssue(issueId);
    return {
      issueId,
      shortId: live?.short_id ?? shortId ?? '',
      title: live?.title ?? snapshot?.title ?? '',
      level: live?.level ?? snapshot?.level ?? 'error',
      status: live?.status ?? snapshot?.status ?? '',
      count: live?.count ?? snapshot?.count ?? '',
      culprit: live?.culprit ?? snapshot?.culprit ?? null,
      project: live?.project_slug ?? snapshot?.project ?? '',
      stale: !live
    };
  });

  const pChatCard = $derived.by(() => {
    const p = shape.props as Record<string, unknown>;
    const sessionId = typeof p.sessionId === 'string' ? p.sessionId : '';
    const messageIndex = typeof p.messageIndex === 'number' ? p.messageIndex : 0;
    const snapshot = (p.snapshot ?? null) as {
      role?: 'user' | 'assistant' | 'system';
      agentKind?: 'claude' | 'cursor';
      sessionTitle?: string;
      excerpt?: string;
      at?: string;
    } | null;
    const live = findChatMessage(sessionId, messageIndex);
    /* Live message body trumps the snapshot when available. We only
       show an excerpt — full markdown rendering on a small canvas
       card would either truncate awkwardly or push the card height
       sky-high. The user can resize to read more. */
    const liveExcerpt = live?.message?.content
      ? live.message.content.replace(/\s+/g, ' ').trim().slice(0, 220)
      : null;
    return {
      sessionId, messageIndex,
      role: live?.message.role ?? snapshot?.role ?? 'assistant',
      agentKind: live?.session.agentKind ?? snapshot?.agentKind ?? 'claude',
      sessionTitle: live?.session.title ?? snapshot?.sessionTitle ?? '(session gone)',
      excerpt: liveExcerpt ?? snapshot?.excerpt ?? '',
      at: live?.message.at ?? snapshot?.at ?? '',
      stale: !live
    };
  });

  const pFileCard = $derived.by(() => {
    const p = shape.props as Record<string, unknown>;
    const repoRoot = typeof p.repoRoot === 'string' ? p.repoRoot : null;
    const relPath = typeof p.relPath === 'string' ? p.relPath : '';
    const isDir = !!p.isDir;
    /* The file lives on disk; it's "live" by definition. We don't watch
       for delete here — the card just shows the path. A later iteration
       could add an existence check via the fs IPC, but for v1 the user
       sees a stale path until they try to navigate to it. */
    const slash = relPath.lastIndexOf('/');
    const basename = slash >= 0 ? relPath.slice(slash + 1) : relPath;
    const parent = slash >= 0 ? relPath.slice(0, slash) : '';
    return { repoRoot, relPath, isDir, basename, parent };
  });

  /* Mermaid is dynamic-imported on first render of any mermaid shape so
     the ~100 KB gz library doesn't bloat first-paint of the solo
     column. Each shape calls into the same module-level instance. */
  let mermaidEl = $state<HTMLDivElement | null>(null);
  let mermaidErr = $state<string | null>(null);
  /* Reactive trigger key — concatenates source + theme + shape id so the
     effect re-runs only when render-relevant fields change. Using a key
     keeps the body small (no need to manually compare prior values). */
  const mermaidKey = $derived(
    shape.kind === 'mermaid' ? `${shape.id}|${pMermaid.theme}|${pMermaid.source}` : null
  );
  $effect(() => {
    if (shape.kind !== 'mermaid' || !mermaidEl || !mermaidKey) return;
    const target = mermaidEl;
    const source = pMermaid.source;
    let cancelled = false;
    void (async () => {
      try {
        const m = await import('mermaid');
        if (cancelled) return;
        m.default.initialize({
          startOnLoad: false,
          theme: 'dark',
          themeVariables: {
            background: 'transparent',
            primaryColor: '#1a1614',
            primaryTextColor: '#f4f0eb',
            primaryBorderColor: '#ee6b1f',
            lineColor: '#a8acb4',
            secondaryColor: '#2a221d',
            tertiaryColor: '#3b3027'
          },
          fontFamily: '-apple-system, system-ui, sans-serif'
        });
        const id = `mermaid-${shape.id.replace(/[^a-zA-Z0-9]/g, '')}`;
        const { svg } = await m.default.render(id, source);
        if (cancelled) return;
        target.innerHTML = svg;
        mermaidErr = null;
      } catch (err) {
        if (cancelled) return;
        target.innerHTML = '';
        mermaidErr = err instanceof Error ? err.message : String(err);
      }
    })();
    return () => { cancelled = true; };
  });

  /* Theme colors come from CSS vars; props store the *name* (`'border-hi'`,
     `'accent'`, `'text-mute'`) and we map to `var(--name)` here. Falls
     back to the literal value if it doesn't match a known token, so an
     agent passing a hex (`'#ff8800'`) still works. */
  function tokenize(name: string | null): string {
    if (!name) return 'transparent';
    if (name.startsWith('#') || name.startsWith('rgb')) return name;
    return `var(--${name})`;
  }

  /* Sticky tints come from a small fixed palette of CSS var names so
     `tint: 'forge'` resolves to the warm accent the rest of the app uses,
     `'yellow'` to a softer post-it tone, etc. Anything unrecognised falls
     back to the forge accent. */
  const STICKY_BG: Record<string, string> = {
    forge: 'rgba(232, 130, 100, 0.18)',
    yellow: 'rgba(244, 196, 48, 0.20)',
    pink: 'rgba(232, 78, 158, 0.18)',
    blue: 'rgba(79, 142, 255, 0.18)',
    green: 'rgba(168, 217, 184, 0.18)',
    gray: 'rgba(168, 172, 180, 0.16)'
  };
  const STICKY_BORDER: Record<string, string> = {
    forge: 'rgba(232, 130, 100, 0.55)',
    yellow: 'rgba(244, 196, 48, 0.55)',
    pink: 'rgba(232, 78, 158, 0.55)',
    blue: 'rgba(79, 142, 255, 0.55)',
    green: 'rgba(168, 217, 184, 0.55)',
    gray: 'rgba(168, 172, 180, 0.45)'
  };
  const stickyBg = $derived(STICKY_BG[pSticky.tint] ?? STICKY_BG.forge);
  const stickyBorder = $derived(STICKY_BORDER[pSticky.tint] ?? STICKY_BORDER.forge);
</script>

<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<div
  class="cv-shape"
  class:cv-shape--selected={selected}
  class:cv-shape--dim={dim}
  class:cv-shape--openable={isLiveCard}
  data-shape-id={shape.id}
  data-shape-kind={shape.kind}
  style="
    left: {shape.x}px;
    top: {shape.y}px;
    width: {shape.w}px;
    height: {shape.h}px;
    transform: rotate({shape.rot}rad);
    z-index: {Math.round(shape.z) % 1000000};
  "
  aria-label={shape.label ?? `${shape.kind} shape`}
  ondblclick={onDoubleClick}
  role={isLiveCard ? 'button' : undefined}
  tabindex={isLiveCard ? 0 : undefined}
  title={isLiveCard ? 'Double-click to open the source' : undefined}
>
  {#if shape.kind === 'rect'}
    <svg width="100%" height="100%" overflow="visible">
      <rect
        x={pRect.strokeWidth / 2}
        y={pRect.strokeWidth / 2}
        width={Math.max(0, shape.w - pRect.strokeWidth)}
        height={Math.max(0, shape.h - pRect.strokeWidth)}
        rx={pRect.radius}
        ry={pRect.radius}
        fill={tokenize(pRect.fill)}
        stroke={tokenize(pRect.stroke)}
        stroke-width={pRect.strokeWidth}
        vector-effect="non-scaling-stroke"
      />
    </svg>
  {:else if shape.kind === 'ellipse'}
    <svg width="100%" height="100%" overflow="visible">
      <ellipse
        cx={shape.w / 2}
        cy={shape.h / 2}
        rx={Math.max(0, shape.w / 2 - pEllipse.strokeWidth / 2)}
        ry={Math.max(0, shape.h / 2 - pEllipse.strokeWidth / 2)}
        fill={tokenize(pEllipse.fill)}
        stroke={tokenize(pEllipse.stroke)}
        stroke-width={pEllipse.strokeWidth}
        vector-effect="non-scaling-stroke"
      />
    </svg>
  {:else if shape.kind === 'line'}
    <svg width="100%" height="100%" overflow="visible">
      <line
        x1={pLine.from.x}
        y1={pLine.from.y}
        x2={pLine.to.x}
        y2={pLine.to.y}
        stroke="var(--text-1)"
        stroke-width={pLine.thickness}
        stroke-linecap="round"
        stroke-dasharray={pLine.dash === 'dashed' ? '6 4' : pLine.dash === 'dotted' ? '2 4' : undefined}
        vector-effect="non-scaling-stroke"
      />
    </svg>
  {:else if shape.kind === 'arrow-shape'}
    <svg width="100%" height="100%" overflow="visible">
      <defs>
        <marker
          id="arrow-head-{shape.id}"
          viewBox="0 0 10 10"
          refX="8"
          refY="5"
          markerWidth="6"
          markerHeight="6"
          orient="auto-start-reverse"
        >
          <path d="M 0 0 L 10 5 L 0 10 z" fill="var(--text-0)" />
        </marker>
      </defs>
      <line
        x1={pArrow.from.x}
        y1={pArrow.from.y}
        x2={pArrow.to.x}
        y2={pArrow.to.y}
        stroke="var(--text-0)"
        stroke-width={pArrow.thickness}
        stroke-linecap="round"
        marker-end={pArrow.head !== 'none' ? `url(#arrow-head-${shape.id})` : undefined}
        vector-effect="non-scaling-stroke"
      />
    </svg>
  {:else if shape.kind === 'text'}
    <div
      class="cv-text"
      style="
        font-size: {pText.fontSize}px;
        font-weight: {pText.fontWeight};
        text-align: {pText.align};
        color: {pText.color ? tokenize(pText.color) : 'var(--text-0)'};
      "
    >{pText.text}</div>
  {:else if shape.kind === 'sticky'}
    <div
      class="cv-sticky"
      style="
        --sticky-bg: {stickyBg};
        --sticky-border: {stickyBorder};
        font-size: {pSticky.fontSize}px;
      "
    >
      <!-- Markdown rendered through `marked` — local content only, no
           sanitizer needed (see `stickyHtml` derivation). -->
      {@html stickyHtml}
    </div>
  {:else if shape.kind === 'mermaid'}
    <div class="cv-mermaid">
      <div class="cv-mermaid-render" bind:this={mermaidEl}></div>
      {#if mermaidErr}
        <div class="cv-shape-err mono" title={mermaidErr}>mermaid: {mermaidErr.slice(0, 60)}…</div>
      {/if}
    </div>
  {:else if shape.kind === 'code'}
    <pre class="cv-code mono"
      data-lang={pCode.language}
      class:cv-code--lines={pCode.lineNumbers}
    ><code>{pCode.source}</code></pre>
  {:else if shape.kind === 'image'}
    {#if pImage.dataUrl}
      <img class="cv-image" src={pImage.dataUrl} alt={pImage.alt ?? 'pasted image'} draggable="false" />
    {:else}
      <div class="cv-image-empty mono">no image</div>
    {/if}
  {:else if shape.kind === 'freehand'}
    <svg width="100%" height="100%" overflow="visible">
      {#if freehandPath}
        <path d={freehandPath} fill={tokenize(pFreehand.color)} stroke="none" />
      {/if}
    </svg>
  {:else if shape.kind === 'jira-card'}
    <div class="cv-card cv-card--jira" class:cv-card--stale={pJiraCard.stale}>
      <div class="cv-card-stripe cv-card-stripe--jira"></div>
      <div class="cv-card-body">
        <div class="cv-card-meta">
          <span class="cv-card-key mono">{pJiraCard.ticketKey || '?'}</span>
          {#if pJiraCard.issueType}<span class="cv-card-chip">{pJiraCard.issueType}</span>{/if}
          {#if pJiraCard.priority}<span class="cv-card-chip">{pJiraCard.priority}</span>{/if}
          {#if pJiraCard.stale}<span class="cv-card-stale-tag" title="Cached snapshot — open the Jira column to refresh">cached</span>{/if}
        </div>
        <div class="cv-card-title">{pJiraCard.summary}</div>
        <div class="cv-card-meta">
          {#if pJiraCard.status}<span class="cv-card-status">{pJiraCard.status}</span>{/if}
          {#if pJiraCard.assignee}<span class="cv-card-meta-text">@{pJiraCard.assignee}</span>{/if}
        </div>
      </div>
    </div>
  {:else if shape.kind === 'github-pr-card'}
    <div class="cv-card cv-card--github" class:cv-card--stale={pGhCard.stale}>
      <div class="cv-card-stripe cv-card-stripe--github"></div>
      <div class="cv-card-body">
        <div class="cv-card-meta">
          <span class="cv-card-key mono">#{pGhCard.number}</span>
          <span class="cv-card-chip">{pGhCard.owner}/{pGhCard.repo}</span>
          {#if pGhCard.merged}
            <span class="cv-card-chip cv-card-chip--merged">merged</span>
          {:else if pGhCard.draft}
            <span class="cv-card-chip cv-card-chip--draft">draft</span>
          {:else if pGhCard.state}
            <span class="cv-card-chip cv-card-chip--{pGhCard.state}">{pGhCard.state}</span>
          {/if}
          {#if pGhCard.stale}<span class="cv-card-stale-tag" title="Cached snapshot — open the GitHub column to refresh">cached</span>{/if}
        </div>
        <div class="cv-card-title">{pGhCard.title}</div>
        <div class="cv-card-meta">
          {#if pGhCard.author}<span class="cv-card-meta-text">@{pGhCard.author}</span>{/if}
          {#if pGhCard.comments > 0}<span class="cv-card-meta-text">💬 {pGhCard.comments}</span>{/if}
        </div>
      </div>
    </div>
  {:else if shape.kind === 'github-issue-card'}
    <div class="cv-card cv-card--github" class:cv-card--stale={pGhCard.stale}>
      <div class="cv-card-stripe cv-card-stripe--github"></div>
      <div class="cv-card-body">
        <div class="cv-card-meta">
          <span class="cv-card-key mono">#{pGhCard.number}</span>
          <span class="cv-card-chip">{pGhCard.owner}/{pGhCard.repo}</span>
          {#if pGhCard.state}
            <span class="cv-card-chip cv-card-chip--{pGhCard.state}">{pGhCard.state}</span>
          {/if}
          {#if pGhCard.stale}<span class="cv-card-stale-tag">cached</span>{/if}
        </div>
        <div class="cv-card-title">{pGhCard.title}</div>
        <div class="cv-card-meta">
          {#if pGhCard.author}<span class="cv-card-meta-text">@{pGhCard.author}</span>{/if}
          {#if pGhCard.comments > 0}<span class="cv-card-meta-text">💬 {pGhCard.comments}</span>{/if}
        </div>
      </div>
    </div>
  {:else if shape.kind === 'sentry-event-card'}
    <div class="cv-card cv-card--sentry" class:cv-card--stale={pSentryCard.stale}>
      <div class="cv-card-stripe cv-card-stripe--sentry"></div>
      <div class="cv-card-body">
        <div class="cv-card-meta">
          <span class="cv-card-key mono">{pSentryCard.shortId || '?'}</span>
          <span class="cv-card-chip cv-card-chip--{pSentryCard.level}">{pSentryCard.level}</span>
          {#if pSentryCard.status}<span class="cv-card-chip">{pSentryCard.status}</span>{/if}
          {#if pSentryCard.stale}<span class="cv-card-stale-tag">cached</span>{/if}
        </div>
        <div class="cv-card-title">{pSentryCard.title}</div>
        <div class="cv-card-meta">
          {#if pSentryCard.culprit}<span class="cv-card-meta-text mono">{pSentryCard.culprit}</span>{/if}
          {#if pSentryCard.count}<span class="cv-card-meta-text">×{pSentryCard.count}</span>{/if}
        </div>
      </div>
    </div>
  {:else if shape.kind === 'file-card'}
    <div class="cv-card cv-card--file">
      <div class="cv-card-stripe cv-card-stripe--file"></div>
      <div class="cv-card-body">
        <div class="cv-card-meta">
          <span class="cv-card-chip">{pFileCard.isDir ? '📁 dir' : '📄 file'}</span>
        </div>
        <div class="cv-card-title mono">{pFileCard.basename || '(empty path)'}</div>
        {#if pFileCard.parent}
          <div class="cv-card-meta">
            <span class="cv-card-meta-text mono">{pFileCard.parent}</span>
          </div>
        {/if}
      </div>
    </div>
  {:else if shape.kind === 'chat-message-card'}
    <div class="cv-card cv-card--chat" class:cv-card--stale={pChatCard.stale}>
      <div
        class="cv-card-stripe"
        class:cv-card-stripe--claude={pChatCard.agentKind === 'claude'}
        class:cv-card-stripe--cursor={pChatCard.agentKind === 'cursor'}
      ></div>
      <div class="cv-card-body">
        <div class="cv-card-meta">
          <span class="cv-card-chip">{pChatCard.role === 'user' ? 'You' : pChatCard.agentKind}</span>
          <span class="cv-card-chip">{pChatCard.sessionTitle.slice(0, 24)}</span>
          {#if pChatCard.stale}<span class="cv-card-stale-tag">snapshot</span>{/if}
        </div>
        <!-- Excerpt rendered as plain text wrapped paragraph; full
             markdown render is reserved for the source AgentApp so
             the card stays compact. -->
        <div class="cv-card-chat-body">{pChatCard.excerpt}</div>
      </div>
    </div>
  {:else if shape.kind === 'frame'}
    <!-- Frame: labeled container with a thin border + title bar. The
         title bar is just a positioned span at the top edge — it's
         clipped INTO the bbox so the frame stays self-contained. -->
    <div class="cv-frame">
      <span class="cv-frame-title mono">{(shape.props as Record<string, unknown>).title ?? 'Frame'}</span>
    </div>
  {:else if shape.kind === 'group'}
    <!-- Pure logical group — no border, just a hit target. Pointer-
         events still resolve so the user can click-select the group
         from anywhere inside its bbox (clicks on children take
         priority via z-stack). -->
    <div class="cv-group"></div>
  {/if}

  {#if editing}
    <!-- Inline editor — replaces the shape body in-place. Sized to fit
         the bbox so what the user sees IS what gets committed. The
         pointerdown stopPropagation prevents the canvas surface from
         starting a translate gesture under the textarea. -->
    <textarea
      class="cv-shape-edit cv-shape-edit--{shape.kind}"
      bind:value={editDraft}
      onblur={commitEdit}
      onpointerdown={(ev) => ev.stopPropagation()}
      onkeydown={(ev) => {
        if (ev.key === 'Escape') { ev.preventDefault(); cancelEdit(); return; }
        /* ⌘/⌃ Enter commits and exits — like Slack / GitHub
           comments. Plain Enter just inserts a newline. */
        if (ev.key === 'Enter' && (ev.metaKey || ev.ctrlKey)) {
          ev.preventDefault();
          commitEdit();
          return;
        }
      }}
      {@attach (n: HTMLTextAreaElement) => { n.focus(); n.select(); }}
      spellcheck={false}
      style="font-size: {shape.kind === 'text' ? pText.fontSize : shape.kind === 'sticky' ? pSticky.fontSize : 12}px;"
    ></textarea>
  {/if}

  {#if selected}
    <!-- Selection ring. Counter-zoomed so the outline stays 1.5 CSS px
         regardless of zoom level. Drawn on top of the shape, not behind,
         so it shows over filled rects too. -->
    <span
      class="cv-shape-ring"
      style="outline-width: {1.5 * cz}px; outline-offset: {2 * cz}px;"
    ></span>
  {/if}
</div>

<style>
  .cv-shape {
    position: absolute;
    transform-origin: center center;
    pointer-events: auto;
  }
  .cv-shape.cv-shape--dim { opacity: 0.55; }
  /* Live-card hover affordance — no cursor change (would break the
     drag-translate signal) but a faint accent shadow to telegraph
     "double-click does something". */
  .cv-shape--openable:hover :global(.cv-card) {
    box-shadow: 0 0 0 1px var(--accent), 0 4px 12px rgba(232, 130, 100, 0.15);
  }

  .cv-shape svg {
    display: block;
    pointer-events: none;
  }

  .cv-text {
    width: 100%;
    height: 100%;
    overflow: hidden;
    line-height: 1.35;
    word-break: break-word;
    white-space: pre-wrap;
    user-select: none;
    /* Readability backdrop — soft dark halo so light text stays legible
       even when an agent placed the text shape over a pastel rect.
       Stacked shadows create a "stroke" effect without `-webkit-text-
       stroke` (which thins glyphs). Cheap on render and works
       independent of the text colour. */
    text-shadow:
      0 0 1px rgba(0, 0, 0, 0.85),
      0 0 3px rgba(0, 0, 0, 0.55),
      0 1px 2px rgba(0, 0, 0, 0.6);
  }

  .cv-sticky {
    width: 100%;
    height: 100%;
    box-sizing: border-box;
    padding: 10px 12px;
    border-radius: 8px;
    background: var(--sticky-bg);
    border: 1px solid var(--sticky-border);
    color: var(--text-0);
    line-height: 1.5;
    overflow: hidden;
    word-break: break-word;
    white-space: pre-wrap;
    user-select: none;
    backdrop-filter: blur(2px);
  }

  /* Selection ring — uses outline so it sits outside the shape's bbox
     and never affects layout. The shape itself sets it via inline style
     so the counter-zoom math is local to this component. */
  .cv-shape-ring {
    position: absolute;
    inset: 0;
    pointer-events: none;
    outline-style: solid;
    outline-color: var(--accent);
    border-radius: inherit;
  }

  /* Frame: labeled container. The border lives on the bbox edge; a
     title chip sits just inside the top-left so it doesn't push past
     the frame's bounds. Children render on top via the z-stack
     (groupShapes positions container z below the lowest child). */
  .cv-frame {
    width: 100%;
    height: 100%;
    box-sizing: border-box;
    border: 1px dashed color-mix(in srgb, var(--text-1) 35%, transparent);
    border-radius: 12px;
    pointer-events: none;
    position: relative;
  }
  .cv-frame-title {
    position: absolute;
    top: -8px;
    left: 14px;
    padding: 2px 8px;
    font-size: 10px;
    color: var(--text-1);
    background: var(--bg-0);
    border: 1px solid color-mix(in srgb, var(--text-1) 35%, transparent);
    border-radius: 4px;
    white-space: nowrap;
    max-width: calc(100% - 28px);
    overflow: hidden;
    text-overflow: ellipsis;
  }
  /* Group: no visual chrome — just a hit target sized to the bbox. */
  .cv-group {
    width: 100%;
    height: 100%;
    pointer-events: none;
  }

  /* Inline-edit textarea — sized to fill the shape's bbox. Drawn on
     top of (and visually replacing) the rendered body during edit.
     Per-kind variants tweak background to match what the rendered
     shape would have looked like, so the swap doesn't flash. */
  .cv-shape-edit {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    box-sizing: border-box;
    padding: 6px 8px;
    border: 1px solid var(--accent);
    border-radius: 6px;
    background: var(--bg-1);
    color: var(--text-0);
    font-family: inherit;
    line-height: 1.45;
    resize: none;
    outline: none;
    z-index: 2;
  }
  .cv-shape-edit--text   { background: rgba(0, 0, 0, 0.45); }
  .cv-shape-edit--sticky { background: var(--sticky-bg, rgba(232, 130, 100, 0.18)); }
  .cv-shape-edit--mermaid,
  .cv-shape-edit--code {
    font-family: 'JetBrains Mono', 'Geist Mono', ui-monospace, monospace;
    background: var(--bg-1);
    white-space: pre;
  }

  /* ---- Sticky markdown ---- */

  .cv-sticky :global(p) { margin: 0 0 6px; }
  .cv-sticky :global(p:last-child) { margin-bottom: 0; }
  .cv-sticky :global(strong) { color: var(--text-0); }
  .cv-sticky :global(em) { color: var(--text-0);  }
  .cv-sticky :global(a) { color: var(--accent-bright, var(--accent)); }
  .cv-sticky :global(code) {
    font-family: -ui-monospace, 'JetBrains Mono', 'Geist Mono', monospace;
    background: rgba(0, 0, 0, 0.25);
    padding: 1px 4px;
    border-radius: 3px;
    font-size: 0.9em;
  }
  .cv-sticky :global(pre) {
    background: rgba(0, 0, 0, 0.3);
    padding: 8px;
    border-radius: 6px;
    overflow: hidden;
    margin: 4px 0;
    font-size: 0.85em;
  }
  .cv-sticky :global(ul), .cv-sticky :global(ol) {
    padding-left: 18px;
    margin: 4px 0;
  }
  .cv-sticky :global(h1), .cv-sticky :global(h2), .cv-sticky :global(h3) {
    margin: 4px 0;
    font-size: 1.05em;
    font-weight: 600;
  }

  /* ---- Mermaid ---- */

  .cv-mermaid {
    width: 100%;
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    overflow: hidden;
    background: rgba(255, 255, 255, 0.02);
    border: 1px solid var(--border-neutral);
    border-radius: 6px;
    padding: 8px;
    box-sizing: border-box;
    user-select: none;
  }
  .cv-mermaid-render {
    width: 100%;
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .cv-mermaid-render :global(svg) {
    max-width: 100%;
    max-height: 100%;
    width: 100%;
    height: 100%;
  }
  .cv-shape-err {
    position: absolute;
    bottom: 4px;
    left: 4px;
    right: 4px;
    font-size: 10px;
    color: var(--error, #EF4444);
    background: rgba(0, 0, 0, 0.55);
    padding: 2px 6px;
    border-radius: 3px;
    pointer-events: none;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* ---- Code block ---- */

  .cv-code {
    width: 100%;
    height: 100%;
    margin: 0;
    padding: 10px 12px;
    box-sizing: border-box;
    border-radius: 6px;
    border: 1px solid var(--border-neutral);
    background: var(--bg-1);
    color: var(--text-0);
    font-family: 'JetBrains Mono', 'Geist Mono', ui-monospace, monospace;
    font-size: 12px;
    line-height: 1.5;
    overflow: hidden;
    white-space: pre;
    user-select: none;
  }
  .cv-code code {
    font-family: inherit;
    color: inherit;
    background: none;
  }

  /* ---- Image ---- */

  .cv-image {
    width: 100%;
    height: 100%;
    object-fit: cover;
    border-radius: 4px;
    pointer-events: none;
    user-select: none;
  }
  .cv-image-empty {
    width: 100%;
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--bg-2);
    border: 1px dashed var(--border-neutral-hi);
    border-radius: 6px;
    color: var(--text-mute);
    font-size: 11px;
    user-select: none;
  }

  /* ---- Forge live cards ----
     Shared shell across kinds — a left accent stripe (per source) plus
     a body with key, title, and metadata. Mirrors the inbox-card design
     in [`UI.md §4.2`](../../../../docs/UI.md) so a card on the canvas
     reads like the same object you saw in the column.
  */
  .cv-card {
    width: 100%;
    height: 100%;
    box-sizing: border-box;
    border-radius: 8px;
    background: var(--bg-1);
    border: 1px solid var(--border-neutral);
    box-shadow: inset 0 1px 0 rgba(245, 240, 234, 0.04);
    display: flex;
    overflow: hidden;
    user-select: none;
    transition: border-color 120ms;
  }
  .cv-card:hover { border-color: var(--border-hi, var(--border-neutral-hi)); }

  /* Snapshot-only mode — visually communicate "this isn't live right now". */
  .cv-card--stale .cv-card-body { opacity: 0.85; }

  .cv-card-stripe {
    width: 3px;
    flex: 0 0 3px;
    background: var(--text-mute);
  }
  .cv-card-stripe--jira    { background: #2684FF; }
  .cv-card-stripe--github  { background: #8B5CF6; }
  .cv-card-stripe--sentry  { background: #F88F74; }
  .cv-card-stripe--file    { background: #E8A33A; }
  .cv-card-stripe--claude  { background: #D97757; }
  .cv-card-stripe--cursor  { background: #B099F6; }

  .cv-card-body {
    flex: 1 1 auto;
    min-width: 0;
    padding: 10px 12px;
    display: flex;
    flex-direction: column;
    gap: 6px;
    overflow: hidden;
  }
  .cv-card-meta {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    align-items: center;
    font-size: 10px;
    color: var(--text-2);
    min-height: 0;
  }
  .cv-card-key {
    font-weight: 600;
    color: var(--text-1);
    letter-spacing: 0;
  }
  .cv-card-chip {
    padding: 1px 6px;
    border-radius: 4px;
    background: var(--bg-2);
    border: 1px solid var(--border-neutral);
    color: var(--text-1);
    font-size: 10px;
    line-height: 1.4;
    white-space: nowrap;
  }
  .cv-card-chip--merged { color: #c084fc; border-color: rgba(192, 132, 252, 0.4); }
  .cv-card-chip--draft  { color: var(--text-2); }
  .cv-card-chip--open   { color: #A8D9B8; border-color: rgba(52, 211, 153, 0.35); }
  .cv-card-chip--closed { color: var(--text-2); }
  .cv-card-chip--error    { color: #E88264; border-color: rgba(232, 130, 100, 0.4); }
  .cv-card-chip--warning  { color: #D9B86E; border-color: rgba(217, 184, 110, 0.4); }
  .cv-card-chip--info     { color: #4F8EFF; border-color: rgba(79, 142, 255, 0.4); }
  .cv-card-chip--fatal    { color: #E88264; border-color: rgba(232, 130, 100, 0.6); background: rgba(232, 130, 100, 0.08); }
  .cv-card-chip--debug    { color: var(--text-2); }
  .cv-card-stale-tag {
    margin-left: auto;
    font-size: 9px;
    color: var(--text-mute);
    text-transform: uppercase;
    letter-spacing: 0.05em;
    padding: 1px 4px;
    border: 1px dashed var(--border-neutral);
    border-radius: 3px;
  }
  .cv-card-title {
    color: var(--text-0);
    font-size: 13px;
    font-weight: 500;
    line-height: 1.35;
    overflow: hidden;
    display: -webkit-box;
    -webkit-line-clamp: 3;
    line-clamp: 3;
    -webkit-box-orient: vertical;
    word-break: break-word;
  }
  .cv-card-status {
    font-size: 10px;
    color: var(--text-1);
    padding: 1px 6px;
    border-radius: 4px;
    background: var(--bg-2);
    border: 1px solid var(--border-neutral);
  }
  .cv-card-meta-text {
    font-size: 10px;
    color: var(--text-2);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }

  /* Chat-message card body — taller text block, can wrap, but
     line-clamps to keep card height predictable. Resize handle gives
     full width; clamp adjusts visually. */
  .cv-card-chat-body {
    color: var(--text-0);
    font-size: 12px;
    line-height: 1.45;
    overflow: hidden;
    word-break: break-word;
    white-space: pre-wrap;
    flex: 1 1 auto;
    min-height: 0;
  }
</style>
