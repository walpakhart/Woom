<script lang="ts">
  /* EmptyState — themed empty card для solo modes когда:
     - источник не подключён → подсказывает Connect
     - подключён, но пусто → подсказывает что появится
     - идёт реализация → "coming next iteration"

     Использует .app-empty primitives из solo.css. Brand-tone halo
     задаётся через --app-tone и --app-glow на корне. */

  interface Action {
    label: string;
    onClick: () => void;
    primary?: boolean;
  }

  interface Props {
    /** Title shown big, in Instrument Serif. */
    title: string;
    /** Body text under title. */
    body?: string;
    /** SVG path-d (single-path icons only — keeps the file tiny). */
    iconPath: string;
    /** Stroke or fill — most icons are stroked, brand glyphs (claude /
     *  jira / github / sentry) are filled. */
    iconFilled?: boolean;
    /** Brand tone — `var(--src-jira)` etc. Drives icon glow + background halo. */
    tone: string;
    /** Brand glow rgba — softer halo behind the icon. */
    glow: string;
    /** Optional action buttons (primary + secondary). */
    actions?: Action[];
  }

  let {
    title,
    body = '',
    iconPath,
    iconFilled = false,
    tone,
    glow,
    actions = []
  }: Props = $props();
</script>

<section class="app-shell" style="--app-tone: {tone}; --app-glow: {glow};">
  <div class="app-empty">
    <div class="app-empty-icon">
      <svg
        viewBox="0 0 24 24"
        fill={iconFilled ? 'currentColor' : 'none'}
        stroke={iconFilled ? 'none' : 'currentColor'}
        stroke-width="1.6"
        stroke-linecap="round"
        stroke-linejoin="round"
      >
        <path d={iconPath} />
      </svg>
    </div>
    <h2 class="app-empty-h">{title}</h2>
    {#if body}
      <p class="app-empty-p">{body}</p>
    {/if}
    {#if actions.length > 0}
      <div class="app-empty-actions">
        {#each actions as a (a.label)}
          <button
            class="btn"
            class:btn--primary={a.primary}
            class:btn--ghost={!a.primary}
            onclick={a.onClick}
          >
            {a.label}
          </button>
        {/each}
      </div>
    {/if}
  </div>
</section>
