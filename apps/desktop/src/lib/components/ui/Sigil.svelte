<script lang="ts">
  /* Woom sigil — inline SVG, scales sharply at any size.
     Glyph: a stylized lowercase ω / w — two interlocking arches under
     a softly rounded container, in the warm terracotta brand tone.
     Reads as a "W" peaks at small sizes; gains personality at large
     sizes (the inner serif drop in the center valley evokes Instrument
     Serif typography).

     Light mode + dark mode share the same mark (the container holds
     the brand gradient regardless of theme). */
  interface Props {
    size?: number;
    glow?: boolean;
  }
  let { size = 36, glow = true }: Props = $props();
</script>

<svg
  class="sigil"
  class:glow
  width={size}
  height={size}
  viewBox="0 0 64 64"
  fill="none"
  aria-label="Woom"
  role="img"
>
  <defs>
    <linearGradient id="woomBg" x1="0" y1="0" x2="64" y2="64" gradientUnits="userSpaceOnUse">
      <stop offset="0" stop-color="#E89B7D"/>
      <stop offset="0.55" stop-color="#CC785C"/>
      <stop offset="1" stop-color="#9C5A41"/>
    </linearGradient>
    <radialGradient id="woomShine" cx="20" cy="14" r="40" gradientUnits="userSpaceOnUse">
      <stop offset="0" stop-color="#FFE3D2" stop-opacity="0.55"/>
      <stop offset="0.55" stop-color="#FFE3D2" stop-opacity="0"/>
    </radialGradient>
  </defs>

  <!-- Rounded container with brand gradient + top-left highlight -->
  <rect x="2" y="2" width="60" height="60" rx="16" fill="url(#woomBg)"/>
  <rect x="2" y="2" width="60" height="60" rx="16" fill="url(#woomShine)"/>
  <rect x="2.5" y="2.5" width="59" height="59" rx="15.5"
        fill="none" stroke="#FAEEE0" stroke-opacity="0.18"/>

  <!-- Single-path W → two soft U valleys joined at the center peak.
       Drawn in cream on the terracotta gradient. Round caps + joins
       keep the silhouette warm at small sizes (rail = 30px). The
       stroke is generous (6.5) so the mark reads as a single
       confident glyph rather than four parallel strokes when
       downscaled. -->
  <path
    d="M13 21 C 13 38, 22 49, 25 49 C 28 49, 31 38, 32 28 C 33 38, 36 49, 39 49 C 42 49, 51 38, 51 21"
    stroke="#FAEEE0" stroke-width="6.5"
    stroke-linecap="round" stroke-linejoin="round"
    fill="none"
  />

  <!-- Editorial serif tittle above center — the "om" dot. -->
  <circle cx="32" cy="13" r="2.6" fill="#FAEEE0"/>
</svg>

<style>
  .sigil {
    display: block;
    user-select: none;
    -webkit-user-drag: none;
  }
  .sigil.glow {
    filter: drop-shadow(0 0 12px rgba(204, 120, 92, 0.42));
  }
</style>
