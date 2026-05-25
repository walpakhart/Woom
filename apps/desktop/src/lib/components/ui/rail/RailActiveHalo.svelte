<script lang="ts">
  /* Active-button halo overlay.
   *
   * The button-attached `box-shadow: 0 0 22px ...` halo gets clipped at
   * the right edge of `.rail-scroll` because that container's
   * `overflow-y: auto` forces `overflow-x` to a clipped value too (CSS
   * spec). The button is centered in a narrow rail, so the 22px outer
   * glow can't fit. Painting it via a sibling overlay outside
   * `.rail-scroll` escapes the clip — the overlay sits on top of any
   * sibling content with its own `position: absolute`.
   *
   * We find the active button via `.rail-btn.active` / `.rail-sigil.active`
   * inside the parent rail element, pull its `getBoundingClientRect`,
   * and translate centre into a `top` offset relative to the rail.
   */
  import { fade } from 'svelte/transition';

  interface Props {
    /** Parent `<aside class="rail">` element. Halo queries descendants
     *  to find `.rail-btn.active` / `.rail-sigil.active`. */
    railEl: HTMLElement | null;
    /** `.rail-scroll` wrapper. Halo's clip-rect anchors to this so it
     *  fades together with the scroll content via `mask-image`. */
    scrollEl: HTMLDivElement | null;
    /** Current top-level view — drives `$effect` recomputation when
     *  user navigates between solos. */
    view: string;
    /** Whether the scroll content is offset from the top — drives
     *  `.fade-top` on the clip wrapper. Owned by parent because the
     *  `.rail-scroll`'s own fades read the same flag. */
    scrolledFromTop: boolean;
    /** Whether more scroll content lives below the viewport — drives
     *  `.fade-bottom`. */
    moreBelow: boolean;
  }
  let p: Props = $props();

  /* Anchor box for the halo's CLIP container — matches `.rail-scroll`'s
     vertical bounds in rail coordinates, padded by `HALO_BLUR_PAD` on
     top + bottom so the box-shadow's outer blur (≈22px) doesn't get
     sliced when the active button sits exactly at the scroll's top or
     bottom edge. The wrapper mirrors the scroll's `.fade-top` /
     `.fade-bottom` flags and applies a matching `mask-image` so the
     halo fades together with the scroll content when the user scrolls
     past the active item — outline and glow disappear in lockstep
     instead of glow leaking through the fade region. */
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
  /* Halo X anchor — centre of rail's button column (not clip wrapper,
     which extends past the rail's right edge). Wrapper is wider than
     the rail so the box-shadow has room to spread into the chat panel;
     halo itself stays centred above the active button. */
  let haloAnchorX = $state(28);
  let activeHaloY = $state(0);
  let activeHaloW = $state(44);
  let activeHaloH = $state(44);
  let activeHaloR = $state(11);
  let activeHaloGlow = $state('var(--accent-glow)');
  let activeHaloVisible = $state(false);
  /* Stable identity of currently-haloed button. Drives the `{#key}`
     block so halo unmounts + remounts ONLY when active target changes
     (not when user simply scrolls and position shifts). Built from
     `aria-label` + `data-tooltip` because both rail-btns AND
     RailAppButton sub-instances expose them with stable per-instance
     content ("Editor Klimt" stays identifiable across scrolls /
     reflows). */
  let activeHaloKey = $state('');

  function recomputeHalo() {
    const rail = p.railEl;
    const scroll = p.scrollEl;
    if (!rail) {
      activeHaloVisible = false;
      return;
    }
    haloAnchorX = rail.clientWidth / 2;
    /* Find any descendant currently `.active`. Class selector (not
       `[data-view]`) because RailAppButton renders its button
       internally and doesn't carry a single `data-view` — its primary
       becomes `.active` when both kind and primary instance are
       selected; sub-instance becomes `.active` otherwise. Sigil also
       flips `.active`. */
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
    /* Anchor clip wrapper differently depending on whether the active
       target lives inside `.rail-scroll` or above it (the Home sigil).
       Inside scroll, snap to scroll's bounds padded by HALO_BLUR_PAD.
       Outside (sigil), snap to target itself padded by HALO_BLUR_PAD —
       anchoring to scroll's bounds clipped sigil's halo at the top
       (user-reported bug), since sigil sits above scroll with less
       than HALO_BLUR_PAD breathing room. */
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
    /* Mirror active button's exact size + corner radius so halo
       overlay traces the same chassis (44×44/11 for primary
       rail-btns, 38×38/9 for RailAppButton's sub-instances). Reading
       from DOM means overlay doesn't need to know each kind's
       dimensions. */
    activeHaloW = tRect.width;
    activeHaloH = tRect.height;
    const cs = getComputedStyle(target);
    const radius = parseFloat(cs.borderTopLeftRadius || '11');
    activeHaloR = Number.isFinite(radius) ? radius : 11;
    /* Pull glow tone from active button's computed style so each
       source keeps its branded hue (Jira blue / Sentry purple / Editor
       terracotta / …) without overlay having to know about every
       variable. W sigil doesn't set `--rail-glow` — fall back to
       global `--accent-glow` so Home gets right accent tone. */
    const rawGlow = cs.getPropertyValue('--rail-glow').trim();
    if (rawGlow) {
      activeHaloGlow = rawGlow;
    } else {
      const accentGlow = cs.getPropertyValue('--accent-glow').trim();
      activeHaloGlow = accentGlow || 'rgba(120, 200, 255, 0.45)';
    }
    /* Identity key — combines aria-label + data-tooltip so
       sub-instances of the same kind ("Editor Klimt" vs "Editor
       Hilma") get distinct keys though their CSS class set is
       identical. Position intentionally NOT in key. */
    const aria = target.getAttribute('aria-label') ?? '';
    const tip = target.getAttribute('data-tooltip') ?? '';
    activeHaloKey = `${aria}|${tip}`;
    activeHaloVisible = true;
  }

  /* Batch recomputes via rAF so bursts of scroll / mutation events
     collapse into one DOM read per frame. Without this, scroll handler
     + mutation observer + resize observer can fire multiple times per
     frame, each `getBoundingClientRect` triggering layout flush — the
     "halo jitters with many instances" symptom when stacks were big
     enough to scroll. */
  let _haloRaf: number | null = null;
  function scheduleHaloRecompute() {
    if (_haloRaf != null) return;
    _haloRaf = requestAnimationFrame(() => {
      _haloRaf = null;
      recomputeHalo();
    });
  }

  $effect(() => {
    /* React to view changes — re-find active button + reposition.
       Wait a frame so DOM reflects new `class:active` after Svelte's
       update. Without rAF, overlay would briefly point at previously-
       active button on every nav. */
    void p.view;
    scheduleHaloRecompute();
  });

  $effect(() => {
    /* Track scroll, resize, AND DOM mutations so halo follows active
       button when user scrolls past it, rail reflows, or user adds /
       removes / expands instance stacks (which shifts every button
       below the change point). MutationObserver also covers
       `class:active` swaps — e.g. activating a sub-instance flips
       `.active` on a sibling without going through `view` reactive
       path. */
    const rail = p.railEl;
    const scroll = p.scrollEl;
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

<!-- Active-button halo + its clip wrapper. Vertically the wrapper
     matches `.rail-scroll`'s bounds (so halo's box-shadow gets
     clipped at same top/bottom edges as active button's inset
     outline — they hide together when user scrolls past the active
     item). Horizontally the wrapper extends 30px past the rail's
     right edge so the box-shadow's blur can fully spread into the
     chat panel without being clipped at the rail's right border.
     Wrapper sits BEFORE `.rail-scroll` in DOM so the rail-btns
     naturally stack above it (no z-index gymnastics).
     `{#key activeHaloKey}` causes a clean unmount + remount each
     time the active button changes — Svelte's `fade` transition
     runs both old (out) and new (in) at same time, giving the
     "outline appears / disappears" feel instead of a slide between
     positions. Position changes from scroll / reflow keep the same
     key, so they don't trigger the animation. -->
<div
  class="rail-halo-clip"
  class:fade-top={p.scrolledFromTop && haloInScroll}
  class:fade-bottom={p.moreBelow && haloInScroll}
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

<style>
  .rail-halo-clip {
    position: absolute;
    left: 0;
    right: -30px;
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
    pointer-events: none;
    transform: translate(-50%, -50%);
    box-shadow: 0 0 22px color-mix(in srgb, var(--rail-glow) 60%, transparent);
  }
</style>
