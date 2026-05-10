<script lang="ts">
  /* Woom sigil — renders the brand mark (transparent PNG generated
     from the macOS installer icon). The earlier traced SVG looked
     jagged at small sizes; the rasterised mark stays sharp because
     it ships at 512×512 with the same gradient the installer uses,
     just minus the dark card backdrop.

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
  src="/woom-mark-transparent.png"
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
