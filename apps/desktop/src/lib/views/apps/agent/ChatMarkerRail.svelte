<script lang="ts">
  /* ChatMarkerRail — a custom scroll rail for the chat thread that
     doubles as a semantic minimap. Markers are derived from the live
     DOM (not a separate data model) so lazy-mounted bodies self-heal:
       • every user message  → a "request" tick (clay)
       • every assistant turn → an "answer" tick (muted) + one "topic"
         tick per markdown heading inside it (blue) — the smart, content
         placed navigation points.
     Hold ⌘ while hovering the chat → tick labels unfurl beside the rail;
     ⌘+↑/↓ jumps between markers. Idle scroll magnet-snaps the nearest
     marker to the top. The rail owns the visible scrollbar thumb too —
     WKWebView hides the native overlay bar anyway. */

  type MarkerKind =
    | 'request'
    | 'answer'
    | 'topic'
    | 'edit'
    | 'write'
    | 'create'
    | 'delete'
    | 'commit'
    | 'pr'
    | 'switch_cwd'
    | 'bash'
    | 'webfetch'
    | 'websearch'
    | 'grep'
    | 'glob'
    | 'mcp'
    | 'todos'
    | 'ask'
    | 'read';

  type MarkerGroup = 'request' | 'answer' | 'change' | 'run' | 'read' | 'topic';

  interface Marker {
    key: string;
    kind: MarkerKind;
    group: MarkerGroup;
    label: string;
    count: number; // how many raw markers collapsed into this cluster
    frac: number; // 0..1 position along the content
    top: number; // px offset within the scroller content
  }

  /* Higher wins when a cluster of markers collapses into one tick.
     Change-ops (commit/PR/edit) outrank a bare "read"; a user request
     outranks the reads under it; a plain answer loses to any action
     it produced so the tick reflects what the turn DID. */
  const PRI: Record<string, number> = {
    pr: 8,
    commit: 8,
    create: 7,
    delete: 7,
    write: 7,
    edit: 7,
    request: 6,
    switch_cwd: 5,
    ask: 5,
    bash: 4,
    topic: 4,
    webfetch: 3,
    websearch: 3,
    grep: 2,
    glob: 2,
    mcp: 2,
    todos: 2,
    read: 1,
    answer: 3,
  };
  const GROUP: Record<string, MarkerGroup> = {
    request: 'request',
    answer: 'answer',
    topic: 'topic',
    edit: 'change',
    write: 'change',
    create: 'change',
    delete: 'change',
    commit: 'change',
    pr: 'change',
    switch_cwd: 'run',
    bash: 'run',
    webfetch: 'read',
    websearch: 'read',
    grep: 'read',
    glob: 'read',
    mcp: 'read',
    todos: 'read',
    ask: 'read',
    read: 'read',
  };
  const GLYPH: Record<string, string> = {
    request: '›',
    answer: '·',
    topic: '#',
    edit: '±',
    write: '±',
    create: '±',
    delete: '✕',
    commit: '⎇',
    pr: '⎇',
    switch_cwd: '⇢',
    bash: '$',
    webfetch: '↗',
    websearch: '↗',
    grep: '⌕',
    glob: '⌕',
    mcp: '◇',
    todos: '☰',
    ask: '?',
    read: '◎',
  };
  const CLUSTER_PX = 26; // raw markers closer than this collapse into one
  /* Only these tool ops earn a rail tick — they're the meaningful "what
     changed" moments. read/grep/glob/bash/mcp/webfetch are dropped:
     their labels (search patterns, paths, raw commands) are noise. */
  const CHANGE_KINDS = new Set(['edit', 'write', 'create', 'delete', 'commit', 'pr']);

  interface Props {
    /** The chat scroll container (`.ct`). */
    scroller: HTMLElement | null;
    /** Active session id — rebuild + reset when it changes. */
    sessionId: string;
    /** Bump to force a marker rebuild (message count / streaming length /
        lazy-mount reveal all fold into this). */
    revision: number;
    /** Set false to disable magnetic snap. */
    snap?: boolean;
  }
  let p: Props = $props();

  let markers = $state<Marker[]>([]);
  let thumbTop = $state(0); // fraction 0..1
  let thumbH = $state(0.12); // fraction 0..1
  let cmdHeld = $state(false);
  let pointerInside = $state(false);
  let hoverKey = $state<string | null>(null);
  let dragging = $state(false);
  let railEl: HTMLElement | null = $state(null);

  const open = $derived(cmdHeld && pointerInside);
  const TOP_PAD = 8; // align a marker this far below the viewport top

  function labelFrom(el: HTMLElement): string {
    const src = el.querySelector('.prose, .msg-body, .msg-stub') ?? el;
    const t = (src.textContent ?? '').replace(/\s+/g, ' ').trim();
    return t.slice(0, 96) || '—';
  }

  interface RawMark {
    key: string;
    kind: MarkerKind;
    label: string;
    top: number;
  }

  function measure() {
    const sc = p.scroller;
    if (!sc) {
      markers = [];
      return;
    }
    const sh = sc.scrollHeight;
    const ch = sc.clientHeight;
    if (sh <= 0 || ch <= 0) return;
    thumbH = Math.min(1, ch / sh);
    thumbTop = sc.scrollTop / sh;

    const scTop = sc.getBoundingClientRect().top;
    const topOf = (el: HTMLElement) => el.getBoundingClientRect().top - scTop + sc.scrollTop;

    const raw: RawMark[] = [];
    const arts = sc.querySelectorAll<HTMLElement>('[data-msg-idx][data-msg-role]');
    for (const art of arts) {
      const role = art.dataset.msgRole;
      const idx = art.dataset.msgIdx ?? '?';
      const top = topOf(art);
      if (role === 'user') {
        raw.push({ key: `u${idx}`, kind: 'request', label: labelFrom(art), top });
        continue;
      }
      if (role !== 'assistant') continue;
      // Base answer tick — loses to any action it produced, so it only
      // survives clustering when the turn was pure prose.
      raw.push({ key: `a${idx}`, kind: 'answer', label: labelFrom(art), top });
      // Tool actions carry data-mark-* anchors, but most are NOISE as
      // nav labels — grep regexes, glob patterns, read paths, raw bash.
      // Keep ONLY the meaningful "what changed" ops (edit/commit/PR);
      // drop the search/read/run chatter that made the rail unreadable.
      const marks = art.querySelectorAll<HTMLElement>('[data-mark-kind]');
      let mi = 0;
      for (const el of marks) {
        const kind = (el.dataset.markKind ?? 'read') as MarkerKind;
        if (!CHANGE_KINDS.has(kind)) continue;
        let label = (el.dataset.markLabel ?? '').replace(/\s+/g, ' ').trim();
        if (label.includes('/')) label = label.slice(label.lastIndexOf('/') + 1); // basename
        mi++;
        raw.push({ key: `a${idx}-e${mi}`, kind, label: label || kind, top: topOf(el) });
      }
      // Markdown headings → topic ticks (prose answers).
      const heads = art.querySelectorAll<HTMLElement>('.prose h1, .prose h2, .prose h3');
      let hi = 0;
      for (const h of heads) {
        hi++;
        raw.push({
          key: `a${idx}-h${hi}`,
          kind: 'topic',
          label: (h.textContent ?? '').replace(/\s+/g, ' ').trim().slice(0, 96),
          top: topOf(h),
        });
      }
    }
    raw.sort((a, b) => a.top - b.top);

    // Collapse near-coincident marks into one tick; the highest-priority
    // member becomes the representative (kind/label/glyph), lesser ones
    // bump the count badge.
    const out: Marker[] = [];
    for (const r of raw) {
      const last = out[out.length - 1];
      if (last && r.top - last.top < CLUSTER_PX) {
        last.count++;
        if ((PRI[r.kind] ?? 0) > (PRI[last.kind] ?? 0)) {
          last.kind = r.kind;
          last.group = GROUP[r.kind] ?? 'read';
          last.label = r.label;
        }
        continue;
      }
      out.push({
        key: r.key,
        kind: r.kind,
        group: GROUP[r.kind] ?? 'read',
        label: r.label,
        count: 1,
        frac: r.top / sh,
        top: r.top,
      });
    }

    // Second pass in RAIL pixel space: even after content clustering,
    // ticks can land < a few px apart on a long thread and pile into an
    // unreadable smear. Merge anything closer than MIN_GAP_PX on the
    // rail, keeping the highest-priority representative.
    const MIN_GAP_PX = 9;
    const packed: Marker[] = [];
    for (const m of out) {
      const last = packed[packed.length - 1];
      if (last && (m.frac - last.frac) * ch < MIN_GAP_PX) {
        last.count += m.count;
        if ((PRI[m.kind] ?? 0) > (PRI[last.kind] ?? 0)) {
          last.kind = m.kind;
          last.group = m.group;
          last.label = m.label;
        }
        continue;
      }
      packed.push(m);
    }
    markers = packed;
  }

  /* Heavy rebuild (querySelectorAll + cluster) — only on content change
     / resize, NEVER per scroll frame. Marker `frac` is content-relative
     so scrolling doesn't move ticks; only the thumb moves. */
  let raf = 0;
  function schedule() {
    if (raf) return;
    raf = requestAnimationFrame(() => {
      raf = 0;
      measure();
    });
  }

  /* Cheap per-scroll update — just the thumb geometry. No DOM query. */
  function applyThumb() {
    const sc = p.scroller;
    if (!sc) return;
    const sh = sc.scrollHeight;
    if (sh <= 0) return;
    thumbH = Math.min(1, sc.clientHeight / sh);
    thumbTop = sc.scrollTop / sh;
  }
  let thumbRaf = 0;
  function scheduleThumb() {
    if (thumbRaf) return;
    thumbRaf = requestAnimationFrame(() => {
      thumbRaf = 0;
      applyThumb();
    });
  }

  /* Flash the message row we jumped to for ~1.8s so the eye lands. */
  let flashEl: HTMLElement | null = null;
  let flashTimer = 0;
  function flashAt(top: number) {
    const sc = p.scroller;
    if (!sc) return;
    const scTop = sc.getBoundingClientRect().top;
    let best: HTMLElement | null = null;
    let bestD = Infinity;
    for (const a of sc.querySelectorAll<HTMLElement>('[data-msg-idx]')) {
      const t = a.getBoundingClientRect().top - scTop + sc.scrollTop;
      const d = Math.abs(t - top);
      if (d < bestD) {
        bestD = d;
        best = a;
      }
    }
    if (!best) return;
    flashEl?.classList.remove('cmr-flash');
    best.classList.add('cmr-flash');
    flashEl = best;
    if (flashTimer) clearTimeout(flashTimer);
    const el = best;
    flashTimer = window.setTimeout(() => el.classList.remove('cmr-flash'), 1800);
  }

  function scrollToTop(top: number) {
    p.scroller?.scrollTo({ top: Math.max(0, top - TOP_PAD), behavior: 'smooth' });
    flashAt(top);
  }

  function goto(m: Marker) {
    scrollToTop(m.top);
  }

  function navMarker(dir: 1 | -1) {
    const sc = p.scroller;
    if (!sc || markers.length === 0) return;
    const cur = sc.scrollTop + TOP_PAD;
    let target: Marker | null = null;
    if (dir > 0) {
      for (const m of markers) {
        if (m.top > cur + 6) {
          target = m;
          break;
        }
      }
    } else {
      for (let i = markers.length - 1; i >= 0; i--) {
        if (markers[i].top < cur - 6) {
          target = markers[i];
          break;
        }
      }
    }
    if (target) scrollToTop(target.top);
  }

  /* ---- thumb / track drag ---- */
  function scrollFromClientY(clientY: number) {
    const sc = p.scroller;
    if (!sc || !railEl) return;
    const rect = railEl.getBoundingClientRect();
    const ratio = (clientY - rect.top) / rect.height;
    sc.scrollTop = Math.max(0, Math.min(1, ratio)) * (sc.scrollHeight - sc.clientHeight);
  }
  function onThumbDown(e: PointerEvent) {
    e.preventDefault();
    e.stopPropagation();
    dragging = true;
    const sc = p.scroller;
    if (!sc || !railEl) return;
    const rect = railEl.getBoundingClientRect();
    const startY = e.clientY;
    const startScroll = sc.scrollTop;
    const move = (ev: PointerEvent) => {
      const denom = rect.height * (1 - thumbH);
      if (denom <= 0) return;
      sc.scrollTop =
        startScroll + ((ev.clientY - startY) / denom) * (sc.scrollHeight - sc.clientHeight);
    };
    const up = () => {
      dragging = false;
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', up);
    };
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', up);
  }
  function onTrackDown(e: PointerEvent) {
    if (e.target !== railEl) return; // ignore clicks that land on ticks/thumb
    scrollFromClientY(e.clientY);
  }

  /* ---- keyboard: ⌘ hold + ⌘↑/↓ nav ----
     Registered in the CAPTURE phase so we intercept BEFORE the composer
     textarea's own onKey (which would otherwise consume the arrow) and
     before the WebView's native ⌘↑/↓ scroll-to-edge. The chat that owns
     the jump is simply the one currently laid out (clientHeight > 0);
     only one ChatThread is ever on-screen at a time. Editable focus is
     NOT a bail-out — ⌘↑/↓ is the chat-nav chord even while typing. */
  function onKeyDown(e: KeyboardEvent) {
    if (e.key === 'Meta' || e.metaKey) cmdHeld = true;
    if (!(e.metaKey && (e.key === 'ArrowUp' || e.key === 'ArrowDown'))) return;
    const sc = p.scroller;
    if (!sc || sc.clientHeight <= 0) return; // this chat isn't visible
    e.preventDefault();
    e.stopPropagation();
    navMarker(e.key === 'ArrowDown' ? 1 : -1);
  }
  function onKeyUp(e: KeyboardEvent) {
    if (e.key === 'Meta' || !e.metaKey) cmdHeld = false;
  }
  function onBlur() {
    cmdHeld = false;
  }

  /* Attach lifecycle to the scroller. Re-runs if the bound element
     swaps (session remount). */
  $effect(() => {
    const sc = p.scroller;
    if (!sc) return;
    const shell = sc.parentElement;
    const onScroll = () => scheduleThumb();
    const onEnter = () => (pointerInside = true);
    const onLeave = () => (pointerInside = false);
    sc.addEventListener('scroll', onScroll, { passive: true });
    shell?.addEventListener('pointerenter', onEnter);
    shell?.addEventListener('pointerleave', onLeave);
    const ro = new ResizeObserver(schedule);
    ro.observe(sc);
    window.addEventListener('keydown', onKeyDown, true);
    window.addEventListener('keyup', onKeyUp, true);
    window.addEventListener('blur', onBlur);
    measure();
    return () => {
      sc.removeEventListener('scroll', onScroll);
      shell?.removeEventListener('pointerenter', onEnter);
      shell?.removeEventListener('pointerleave', onLeave);
      ro.disconnect();
      window.removeEventListener('keydown', onKeyDown, true);
      window.removeEventListener('keyup', onKeyUp, true);
      window.removeEventListener('blur', onBlur);
      if (flashTimer) clearTimeout(flashTimer);
    };
  });

  /* Rebuild markers when the thread content changes (message count,
     streaming length, lazy-mount reveal — all folded into `revision`)
     and on session switch. */
  $effect(() => {
    void p.revision;
    void p.sessionId;
    schedule();
  });
</script>

<div
  class="cmr"
  class:cmr--open={open}
  bind:this={railEl}
  onpointerdown={onTrackDown}
  role="scrollbar"
  aria-controls="chat-scroll"
  aria-valuenow={Math.round(thumbTop * 100)}
  aria-orientation="vertical"
  tabindex="-1"
>
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="cmr-thumb"
    style="top:{thumbTop * 100}%; height:{Math.max(3, thumbH * 100)}%"
    onpointerdown={onThumbDown}
  ></div>
  {#each markers as m (m.key)}
    <button
      class="cmr-tick cmr-tick--{m.group}"
      class:cmr-tick--hot={hoverKey === m.key}
      style="top:{m.frac * 100}%"
      title={m.label}
      onclick={() => goto(m)}
      onmouseenter={() => (hoverKey = m.key)}
      onmouseleave={() => hoverKey === m.key && (hoverKey = null)}
    >
      <span class="cmr-label">
        <span class="cmr-label-glyph mono">{GLYPH[m.kind] ?? '·'}</span>
        <span class="cmr-label-text">{m.label}</span>
        {#if m.count > 1}<span class="cmr-label-count mono">{m.count}</span>{/if}
      </span>
      <span class="cmr-dot"></span>
    </button>
  {/each}
</div>

<style>
  .cmr {
    position: absolute;
    top: 0;
    right: 0;
    bottom: 0;
    width: 12px;
    z-index: 7;
    cursor: pointer;
  }
  .cmr-thumb {
    position: absolute;
    right: 2px;
    width: 5px;
    min-height: 18px;
    border-radius: 3px;
    background: color-mix(in srgb, var(--text-mute) 40%, transparent);
    transition: background 140ms, width 140ms;
  }
  .cmr-thumb:hover,
  .cmr--open .cmr-thumb {
    background: color-mix(in srgb, var(--text-mute) 58%, transparent);
  }

  .cmr-tick {
    position: absolute;
    right: 0;
    transform: translateY(-50%);
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 6px;
    height: 14px;
    padding: 0;
    border: 0;
    background: transparent;
    cursor: pointer;
    /* labels overlay leftward without widening the rail */
    width: max-content;
    max-width: 440px;
    right: 0;
  }
  .cmr-dot {
    flex: none;
    width: 6px;
    height: 6px;
    margin-right: 3px;
    border-radius: 50%;
    background: var(--tone);
    box-shadow: 0 0 0 2px var(--bg-1, var(--bg-0));
    transition: width 140ms, height 140ms, transform 140ms;
  }
  .cmr-tick--request {
    --tone: var(--accent);
  }
  .cmr-tick--answer {
    --tone: color-mix(in srgb, var(--text-mute) 70%, transparent);
  }
  .cmr-tick--change {
    --tone: var(--diff-add, #65d396);
  }
  .cmr-tick--run {
    --tone: #d19a5b;
  }
  .cmr-tick--read {
    --tone: #6ea3c9;
  }
  .cmr-tick--topic {
    --tone: #8b93d0;
  }
  .cmr-tick:hover .cmr-dot,
  .cmr-tick--hot .cmr-dot {
    width: 8px;
    height: 8px;
  }

  .cmr-label {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    max-width: 0;
    overflow: hidden;
    white-space: nowrap;
    opacity: 0;
    font-size: 11px;
    line-height: 1.2;
    color: var(--text-1);
    background: color-mix(in srgb, var(--bg-2) 92%, transparent);
    border: 1px solid var(--border);
    border-left: 2px solid var(--tone);
    border-radius: 5px;
    padding: 0;
    transition: max-width 160ms ease, opacity 120ms, padding 160ms;
    pointer-events: none;
  }
  .cmr-label-glyph {
    flex: none;
    color: var(--tone);
    font-size: 10px;
    width: 10px;
    text-align: center;
  }
  .cmr-label-text {
    overflow: hidden;
    text-overflow: ellipsis;
    min-width: 0;
  }
  .cmr-label-count {
    flex: none;
    font-size: 9.5px;
    color: var(--text-2);
    background: color-mix(in srgb, var(--tone) 22%, transparent);
    border-radius: 999px;
    padding: 0 5px;
    line-height: 1.5;
  }
  /* Labels unfurl when ⌘ held over the chat, or on individual hover. */
  .cmr--open .cmr-label,
  .cmr-tick:hover .cmr-label {
    max-width: 420px;
    opacity: 1;
    padding: 2px 8px;
  }
  /* Keep the hovered/focused label above its neighbours. */
  .cmr-tick:hover,
  .cmr-tick--hot {
    z-index: 2;
  }

  /* Jump-target flash — soft accent wash that fades over ~1.8s so the
     eye lands on the row we scrolled to. Global: the class is toggled
     on the chat <article>, which lives outside this component. */
  :global(.cmr-flash) {
    animation: cmrFlash 1.8s var(--ease-out, ease-out);
    border-radius: 8px;
  }
  @keyframes -global-cmrFlash {
    0% {
      background: color-mix(in srgb, var(--accent) 26%, transparent);
      box-shadow: 0 0 0 6px color-mix(in srgb, var(--accent) 12%, transparent);
    }
    100% {
      background: transparent;
      box-shadow: 0 0 0 6px transparent;
    }
  }
</style>
