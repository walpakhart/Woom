<script lang="ts">
  /* UpdateNotesPane — full-screen release-notes reader for an
   * incoming auto-update. Triggered by the "View" action on the
   * sticky update toast (see `+page.svelte`). Markdown rendered via
   * the same component the chat thread uses, so GFM tables / fenced
   * code / task lists work end-to-end.
   *
   * Visual model: same lightbox pattern as `SddCard.svelte`'s
   * fullscreen mode — fixed inset, translucent backdrop with blur,
   * Esc + backdrop click + × close. Three CTAs in the footer:
   * - Later (close pane, leave the toast active)
   * - Install on quit (download + stage, no restart)
   * - Install now (download + apply + restart, the louder choice)
   *
   * Phase reference: SDD workspace `sdd-2508eeb82e`, phase 4 task 3. */

  import Markdown from '$lib/components/ui/Markdown.svelte';

  interface Props {
    version: string;
    notes: string;
    pubDate?: string | null;
    /** Fire `installNow` / `installOnQuit` from `$lib/state/updates`.
     *  Parent owns the action wiring so the pane stays pure-display. */
    onInstallNow: () => void;
    onInstallOnQuit: () => void;
    onClose: () => void;
  }
  let { version, notes, pubDate = null, onInstallNow, onInstallOnQuit, onClose }: Props = $props();

  /* Esc dismisses. Mount the listener via $effect so cleanup runs
   * automatically on unmount — matches the SddCard lightbox pattern. */
  function onKey(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.preventDefault();
      onClose();
    }
  }
  $effect(() => {
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  });

  /* Format `pub_date` if present. Tauri's updater emits an RFC3339
   * string via the `time` crate's Display impl — Date() handles it. */
  const dateLabel = $derived.by(() => {
    if (!pubDate) return null;
    try {
      const d = new Date(pubDate);
      if (Number.isNaN(d.getTime())) return null;
      return d.toLocaleDateString(undefined, { year: 'numeric', month: 'short', day: 'numeric' });
    } catch {
      return null;
    }
  });
</script>

<div class="unp-backdrop" onclick={onClose} role="presentation"></div>

<article
  class="unp"
  role="dialog"
  aria-modal="true"
  aria-labelledby="unp-title"
>
  <header class="unp-head">
    <h2 id="unp-title" class="unp-title">
      <span class="unp-product">Woom</span>
      <span class="unp-version mono">{version}</span>
    </h2>
    {#if dateLabel}
      <span class="unp-date">{dateLabel}</span>
    {/if}
    <button
      type="button"
      class="unp-close"
      onclick={onClose}
      title="Close (Esc)"
      aria-label="Close release notes"
    >
      <svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round">
        <path d="M6 6l12 12M6 18L18 6"/>
      </svg>
    </button>
  </header>

  <div class="unp-body">
    {#if notes.trim()}
      <Markdown source={notes} />
    {:else}
      <p class="unp-empty">No release notes published. See the GitHub releases page for the commit-level changelog.</p>
    {/if}
  </div>

  <footer class="unp-foot">
    <button type="button" class="unp-btn" onclick={onClose}>Later</button>
    <button type="button" class="unp-btn unp-btn--ghost" onclick={onInstallOnQuit}>Install on quit</button>
    <button type="button" class="unp-btn unp-btn--primary" onclick={onInstallNow}>Install now</button>
  </footer>
</article>

<style>
  /* Backdrop sits below the pane, dims chat, intercepts clicks
     outside the article so the user can dismiss with a click in
     the margin. */
  .unp-backdrop {
    position: fixed;
    inset: 0;
    z-index: 1000;
    background: var(--backdrop);
    backdrop-filter: blur(3px);
  }

  /* Pane — viewport-cover panel with generous padding + the same
     accent-rail visual language as SddCard's lightbox. */
  .unp {
    position: fixed;
    inset: 5vh 6vw;
    z-index: 1001;
    background: var(--bg-1);
    border-left: 3px solid var(--accent);
    border-radius: 8px;
    box-shadow: var(--shadow-3);
    padding: 18px 28px 18px;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    color: var(--text-0);
  }

  .unp-head {
    display: flex;
    align-items: baseline;
    gap: 16px;
    margin-bottom: 12px;
  }
  .unp-title {
    flex: 1;
    margin: 0;
    font-size: 22px;
    font-weight: 600;
    letter-spacing: -0.015em;
    display: flex;
    align-items: baseline;
    gap: 10px;
  }
  .unp-product { color: var(--text-0); }
  .unp-version {
    font-size: 18px;
    color: var(--accent-bright);
    font-weight: 500;
  }
  .unp-date {
    font-size: 12px;
    color: var(--text-mute);
  }
  .unp-close {
    border: 0;
    background: transparent;
    color: var(--text-mute);
    cursor: pointer;
    width: 26px; height: 26px;
    border-radius: 5px;
    display: inline-flex; align-items: center; justify-content: center;
    transition: background 100ms, color 100ms;
  }
  .unp-close:hover {
    background: color-mix(in srgb, var(--error) 14%, transparent);
    color: var(--error);
  }

  .unp-body {
    flex: 1 1 0;
    min-height: 0;
    overflow-y: auto;
    padding-right: 8px;
    font-size: 14.5px;
    line-height: 1.7;
  }
  .unp-empty {
    color: var(--text-mute);
    font-style: italic;
  }

  .unp-foot {
    display: flex;
    justify-content: flex-end;
    align-items: center;
    gap: 8px;
    margin-top: 12px;
  }
  .unp-btn {
    padding: 6px 14px;
    border-radius: 5px;
    border: 1px solid var(--border-neutral-hi);
    background: var(--bg-2);
    color: var(--text-1);
    font-size: 12.5px;
    font-weight: 500;
    cursor: pointer;
    transition: background 120ms, color 120ms, border-color 120ms;
  }
  .unp-btn:hover {
    background: var(--bg-3);
    color: var(--text-0);
    border-color: var(--border-hi);
  }
  .unp-btn--ghost {
    background: transparent;
  }
  .unp-btn--primary {
    background: var(--accent);
    border-color: var(--accent);
    color: var(--accent-fg);
    font-weight: 600;
  }
  .unp-btn--primary:hover {
    background: var(--accent-bright);
    border-color: var(--accent-bright);
    color: var(--accent-fg);
  }
</style>
