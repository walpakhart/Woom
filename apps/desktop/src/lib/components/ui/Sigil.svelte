<script lang="ts">
  /* Woom sigil — engraved W wordmark. Rendered as an alpha mask
     (generated from the etched logo PNG) filled with `--text-0`,
     so it re-inks itself per theme: warm ink on paper, cream on
     charcoal. The hatching detail lives in the mask's alpha.

     Knobs kept compatible with previous versions:
       • size — pixel side length of the square box; the wide mark
                centers inside it (aspect ≈ 512:233).
       • glow — soft shadow under the glyph; on by default because
                empty-state callsites relied on it. Now a quiet ink
                offset instead of the old mint halo. */
  interface Props {
    size?: number;
    glow?: boolean;
  }
  let { size = 36, glow = true }: Props = $props();
</script>

<span
  class="sigil"
  class:glow
  role="img"
  aria-label="Woom"
  style="width:{size}px;height:{size}px"
></span>

<style>
  .sigil {
    display: block;
    user-select: none;
    background: var(--text-0);
    -webkit-mask: url('/woom-mark-ink.png') center / contain no-repeat;
    mask: url('/woom-mark-ink.png') center / contain no-repeat;
  }
  .sigil.glow {
    filter: drop-shadow(2px 2px 0 color-mix(in srgb, var(--text-0) 14%, transparent));
  }
</style>
