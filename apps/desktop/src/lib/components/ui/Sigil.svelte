<script lang="ts">
  /* Woom sigil — renders the curated `woom-logo.svg` (the brand mark
     the designer hands over) so every place that used to draw the
     hand-coded approximation now shows the real W. The asset already
     ships transparent, so callers get a free standalone glyph that
     drops onto whatever backdrop hosts it.

     Two knobs, kept compatible with the prior inline-SVG version so
     consumers (Welcome, +page empty-state, …) didn't need updating:
       • size — pixel side length, mark stays square inside.
       • glow — soft mint drop-shadow under the glyph; on by default
                because the empty-state callsites relied on it. */
  interface Props {
    size?: number;
    glow?: boolean;
  }
  let { size = 36, glow = true }: Props = $props();
</script>

<img
  class="sigil"
  class:glow
  src="/woom-logo.svg"
  alt="Woom"
  width={size}
  height={size}
  draggable="false"
/>

<style>
  .sigil {
    display: block;
    user-select: none;
    -webkit-user-drag: none;
    object-fit: contain;
  }
  .sigil.glow {
    filter: drop-shadow(0 0 12px rgba(166, 212, 194, 0.40));
  }
</style>
