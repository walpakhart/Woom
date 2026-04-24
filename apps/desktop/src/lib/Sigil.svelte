<script lang="ts">
  interface Props {
    size?: number;
    glow?: boolean;
  }
  let { size = 36, glow = true }: Props = $props();
</script>

<img
  src="/forgehold-mark.png"
  alt="Forgehold"
  width={size}
  height={size}
  class="sigil"
  class:glow
  draggable="false"
/>

<style>
  .sigil {
    display: block;
    user-select: none;
    -webkit-user-drag: none;
  }
  .sigil.glow {
    filter: drop-shadow(0 0 10px rgba(232, 163, 58, 0.28));
  }
</style>
