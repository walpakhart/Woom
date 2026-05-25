<script lang="ts">
  import { themeState, applyTheme, type ThemeName } from '$lib/state/theme.svelte';
  import { scaleState, applyScale, SCALE_OPTIONS } from '$lib/state/scale.svelte';
  import { densityState, applyDensity, type Density } from '$lib/state/density.svelte';

  const THEMES: { name: ThemeName; label: string; sub: string; bg: string; fg: string; accent: string }[] = [
    { name: 'iconic', label: 'Iconic', sub: 'Sage + mint on cool noir', bg: '#0E1112', fg: '#EBEFEC', accent: '#B0DCC8' },
    { name: 'light',  label: 'Light',  sub: 'Sage + mint on cream',     bg: '#F1F5F2', fg: '#0E1B16', accent: '#2E5A4A' }
  ];
</script>

<!-- Theme picker -->
<div class="card">
  <header class="card-head">
    <h2 class="card-title">Theme</h2>
    <p class="card-sub">
      Pick a colour palette. Layout, fonts and spacing stay the same — only colours flip.
    </p>
  </header>
  <div class="theme-grid">
    {#each THEMES as t (t.name)}
      <button
        class="theme-card"
        class:active={themeState.name === t.name}
        onclick={() => applyTheme(t.name)}
        title={t.sub}
        aria-pressed={themeState.name === t.name}
      >
        <span class="theme-swatch" style="background: {t.bg}; color: {t.fg};">
          <span class="theme-swatch-dot" style="background: {t.accent};"></span>
          <span class="theme-swatch-text">Aa</span>
        </span>
        <span class="theme-label">{t.label}</span>
        <span class="theme-sub">{t.sub}</span>
      </button>
    {/each}
  </div>
</div>

<!-- UI scale -->
<div class="card">
  <header class="card-head">
    <h2 class="card-title">UI scale</h2>
    <p class="card-sub">
      Zoom every glyph, border and spacing in the window. Useful on external monitors where
      the OS scaling feels too tight or too loose for chat reading.
    </p>
  </header>
  <div class="scale-grid">
    {#each SCALE_OPTIONS as opt (opt.value)}
      <button
        class="scale-card"
        class:active={scaleState.value === opt.value}
        onclick={() => applyScale(opt.value)}
        aria-pressed={scaleState.value === opt.value}
      >
        <span class="scale-label">{opt.label}</span>
      </button>
    {/each}
  </div>
</div>

<!-- UI density -->
<div class="card">
  <header class="card-head">
    <h2 class="card-title">UI density</h2>
    <p class="card-sub">
      Trim padding around inbox cards and chat messages to fit more on screen. Distinct from UI scale — fonts stay the same size. Keyboard shortcut: <span class="mono">⌘ \</span>.
    </p>
  </header>
  <div class="scale-grid">
    {#each [{ value: 'comfortable' as Density, label: 'Comfortable' }, { value: 'compact' as Density, label: 'Compact' }] as opt (opt.value)}
      <button
        class="scale-card"
        class:active={densityState.value === opt.value}
        onclick={() => applyDensity(opt.value)}
        aria-pressed={densityState.value === opt.value}
      >
        <span class="scale-label">{opt.label}</span>
      </button>
    {/each}
  </div>
</div>
