<script lang="ts">
  /* Resume pill — rendered below an interrupted assistant message
   * (SDD `sdd-98a42f3bdb` Phase 2). Two states:
   *   - countdown — `now < session.resumeAt` → disabled button with
   *     live "Resume in 12m 34s" text.
   *   - active — `now >= session.resumeAt` → `▶ Resume` button. Click
   *     calls the parent's `onResume` callback (which drains the
   *     session's `pendingQueue[0]` and fires `sendClaudeMessage`).
   *
   * The pill is intentionally narrow scope — it doesn't know about
   * `sendClaudeMessage` directly; the parent owns that wiring so the
   * pill stays in `$lib/components/agent/`. */
  import { onDestroy } from 'svelte';
  import type { ClaudeSession } from '$lib/types';
  import { formatResumeIn } from '$lib/state/quota.svelte';

  interface Props {
    session: ClaudeSession;
    onResume: (sessionId: string) => void;
  }
  const { session, onResume }: Props = $props();

  let now = $state(Date.now());
  const tick = setInterval(() => { now = Date.now(); }, 1000);
  onDestroy(() => clearInterval(tick));

  const remaining = $derived(
    session.resumeAt ? Math.max(0, session.resumeAt - now) : 0
  );
  const ready = $derived(remaining <= 0);
</script>

<div class="resume-pill-wrap">
  <button
    class="resume-pill"
    class:resume-pill--ready={ready}
    disabled={!ready}
    onclick={() => ready && onResume(session.id)}
    aria-live="polite"
    title={ready
      ? 'Quota reset — click to resume the interrupted turn'
      : `Auto-resume in ${formatResumeIn(remaining)} (or click after countdown ends).`}
  >
    {#if ready}
      <span class="resume-pill-glyph" aria-hidden="true">▶</span>
      <span>Resume</span>
    {:else}
      <span class="resume-pill-glyph resume-pill-glyph--wait" aria-hidden="true">⏸</span>
      <span>Resume in <span class="mono">{formatResumeIn(remaining)}</span></span>
    {/if}
  </button>
</div>

<style>
  .resume-pill-wrap {
    display: flex;
    justify-content: flex-start;
    padding: 6px 0 2px;
  }
  .resume-pill {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 5px 12px;
    border-radius: 999px;
    font-size: 12px;
    font-weight: 600;
    background: color-mix(in srgb, var(--accent) 8%, transparent);
    border: 1px solid color-mix(in srgb, var(--accent) 28%, var(--border));
    color: var(--text-mute);
    cursor: not-allowed;
    transition: background 140ms, border-color 140ms, color 140ms;
  }
  .resume-pill--ready {
    background: color-mix(in srgb, var(--accent) 18%, transparent);
    border-color: color-mix(in srgb, var(--accent) 55%, var(--border-hi));
    color: var(--accent-bright, var(--accent));
    cursor: pointer;
  }
  .resume-pill--ready:hover {
    background: color-mix(in srgb, var(--accent) 28%, transparent);
  }
  .resume-pill-glyph {
    font-size: 11px;
  }
  .resume-pill-glyph--wait {
    opacity: 0.7;
  }
  .mono {
    font-family: 'JetBrains Mono', monospace;
    font-size: 11.5px;
  }
</style>
