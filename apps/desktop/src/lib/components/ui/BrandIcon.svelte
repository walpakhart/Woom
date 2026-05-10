<script lang="ts">
  /* BrandIcon — single source of truth for source / agent brand
     glyphs across the app. ConnectionsView (the page that introduces
     these brands to the user) is the visual canon: Octocat for
     GitHub, Atlassian wave for Jira, Sentry crown for Sentry, and
     curated PNGs for Claude / Cursor (their official marks rely on
     gradients + facets that don't distill into a mono `<path>`).

     Every other surface — HomeApp tiles, InlineClaude rows,
     SessionsSidebar header, WorktreeSide empty state, source-app
     empty states — funnels through this component instead of redrawing
     the glyph inline. Means a brand refresh only needs to land here. */
  import { SVG_GITHUB, SVG_JIRA, SVG_SENTRY } from '$lib/data';

  type Kind = 'claude' | 'cursor' | 'github' | 'jira' | 'sentry';

  interface Props {
    kind: Kind;
    /** Pixel side length. The component renders a square viewBox so
     *  the W's wide native ratio doesn't sneak in here — Claude /
     *  Cursor PNGs use object-fit: contain to honour their inner
     *  margin, SVGs scale via the 24×24 viewBox like in Connections. */
    size?: number;
    /** Optional aria-label override. Defaults to the kind name so
     *  screen readers announce something useful without the parent
     *  having to plumb a label every time. */
    label?: string;
  }
  let { kind, size = 16, label }: Props = $props();

  const altLabel = $derived(label ?? brandLabel(kind));

  function brandLabel(k: Kind): string {
    switch (k) {
      case 'claude': return 'Claude';
      case 'cursor': return 'Cursor';
      case 'github': return 'GitHub';
      case 'jira': return 'Jira';
      case 'sentry': return 'Sentry';
    }
  }

  /** Path to the curated PNG for raster brands. Same files Connections
   *  loads on its agent cards. */
  const imgSrc = $derived(
    kind === 'claude' ? '/brand-claude.png'
    : kind === 'cursor' ? '/brand-cursor.png'
    : null
  );

  const inlineSvg = $derived(
    kind === 'github' ? SVG_GITHUB
    : kind === 'jira' ? SVG_JIRA
    : kind === 'sentry' ? SVG_SENTRY
    : null
  );
</script>

{#if imgSrc}
  <img
    class="brand-img"
    src={imgSrc}
    alt={altLabel}
    width={size}
    height={size}
    draggable="false"
  />
{:else if inlineSvg}
  <svg
    class="brand-svg"
    viewBox="0 0 24 24"
    fill="currentColor"
    stroke="none"
    aria-label={altLabel}
    role="img"
    width={size}
    height={size}
  >
    {@html inlineSvg}
  </svg>
{/if}

<style>
  .brand-img {
    object-fit: contain;
    display: block;
    -webkit-user-drag: none;
    user-select: none;
  }
  .brand-svg {
    display: block;
  }
</style>
