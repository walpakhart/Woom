<script lang="ts">
  /* SddVerifyPane — structured render of `phases/<slug>/verify.json`.
     Surfaces below the regular body for completed phases so the user
     can scan task_compliance / deviations / notes without leaving the
     card. Empty fields are hidden. Extracted from SddCard in wave-14
     split. See `spec-1` FR-10. */
  import type { VerifyOutput } from '$lib/state/sdd.svelte';

  interface Props {
    slug: string;
    verdict: VerifyOutput;
  }
  let { slug, verdict }: Props = $props();
</script>

<div class="sdd-verify-pane" data-deviated={verdict.deviations.length > 0}>
  <header class="sdd-verify-head mono">verify · phases/{slug}/verify.json</header>
  {#if verdict.summary}
    <p class="sdd-verify-summary">{verdict.summary}</p>
  {/if}
  {#if verdict.files_changed.length > 0}
    <details class="sdd-verify-section">
      <summary class="mono">files changed · {verdict.files_changed.length}</summary>
      <ul class="sdd-verify-files mono">
        {#each verdict.files_changed as f, i (f + '|' + i)}
          <li>{f}</li>
        {/each}
      </ul>
    </details>
  {/if}
  {#if verdict.task_compliance.length > 0}
    <details class="sdd-verify-section">
      <summary class="mono">task compliance · {verdict.task_compliance.length}</summary>
      <ul class="sdd-verify-list">
        {#each verdict.task_compliance as t, i (t + '|' + i)}
          <li><span aria-label="passed">✓</span> {t}</li>
        {/each}
      </ul>
    </details>
  {/if}
  {#if verdict.deviations.length > 0}
    <details class="sdd-verify-section sdd-verify-section--warn" open>
      <summary class="mono">deviations · {verdict.deviations.length}</summary>
      <ul class="sdd-verify-list">
        {#each verdict.deviations as d, i (d + '|' + i)}
          <li><span aria-label="deviation">⚠️</span> {d}</li>
        {/each}
      </ul>
    </details>
  {/if}
  {#if verdict.notes}
    <details class="sdd-verify-section">
      <summary class="mono">notes</summary>
      <p class="sdd-verify-notes">{verdict.notes}</p>
    </details>
  {/if}
</div>

<style>
  .sdd-verify-pane {
    margin-top: 10px;
    padding: 6px 0 4px 12px;
    border-left: 2px solid color-mix(in srgb, var(--accent) 22%, transparent);
    font-size: 12px;
    color: var(--text-1);
  }
  .sdd-verify-pane[data-deviated="true"] {
    border-left-color: color-mix(in srgb, var(--error) 50%, transparent);
  }
  .sdd-verify-head {
    font-size: 10px;
    color: var(--text-mute);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    margin-bottom: 4px;
  }
  .sdd-verify-summary {
    margin: 4px 0 6px 0;
    line-height: 1.5;
  }
  .sdd-verify-section {
    margin: 3px 0;
  }
  .sdd-verify-section > summary {
    cursor: pointer;
    font-size: 11px;
    color: var(--text-mute);
    padding: 2px 0;
  }
  .sdd-verify-section--warn > summary {
    color: var(--error);
  }
  .sdd-verify-files, .sdd-verify-list {
    margin: 4px 0 6px 0;
    padding-left: 16px;
    list-style: none;
  }
  .sdd-verify-files li {
    padding: 1px 0;
    color: var(--text-1);
  }
  .sdd-verify-list li {
    padding: 2px 0;
    line-height: 1.5;
  }
  .sdd-verify-notes {
    margin: 4px 0;
    color: var(--text-1);
    font-style: italic;
  }
</style>
