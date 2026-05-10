<script lang="ts">
  /* ListSearchPicker — dropdown popover that hangs under the search
     input on Jira / GitHub / Sentry inbox lists. Mirrors the @-mention
     picker UX: as the user types, the popover lists the top matches
     (title + key/id), arrow-key nav highlights a row, Enter / click
     opens it, Esc dismisses.

     The list itself stays scrollable underneath (the popover is just a
     quick-jump affordance, not a replacement). It opens the moment the
     user types something AND the search input has focus; closes when
     focus leaves the input *unless* the cursor moved into the popover
     itself.

     Parent owns:
       • the search-input ref (for anchor positioning)
       • the filtered `rows` (already matched + ranked however the
         parent wants — we don't dedupe or rank here)
       • `onPick(id)` to actually open the item
     We own:
       • selected index + arrow-key handling
       • outside-click / blur dismissal
       • absolute positioning via fixed coords from the anchor rect */

  interface Row {
    /** Stable handle the parent uses in `onPick` to focus the item.
     *  For Jira this is the issue key, GitHub the PR number or id,
     *  Sentry the issue id. */
    id: string;
    /** Bold first line — issue summary / PR title. */
    title: string;
    /** Mono right-side hint (KEY-123 / #42 / SHORT-ABC). */
    sub: string;
  }

  interface Props {
    /** Search input element — used to compute the popover's position
     *  and to know when focus left for outside-click dismissal. Pass
     *  `null` while it's still binding; the popover bails out. */
    anchor: HTMLElement | null;
    /** Whether the popover should be visible. Parent flips this to
     *  false on Esc / blur / pick. */
    open: boolean;
    /** Top-N matches the parent already filtered/ranked. We render
     *  whatever's here; pass an empty array to render the empty state. */
    rows: Row[];
    /** Source label rendered above the rows ("JIRA" / "GITHUB" /
     *  "SENTRY"). Drives the brand-tone via `data-source`. */
    source: 'jira' | 'github' | 'sentry';
    /** Click / Enter on a row. */
    onPick: (id: string) => void;
    /** Esc / outside-click. Parent should set `open = false` and
     *  optionally refocus the search input. */
    onClose: () => void;
  }
  let p: Props = $props();

  let selectedIdx = $state(0);
  let popoverEl = $state<HTMLDivElement | null>(null);

  /* Reset selection whenever the row set changes — keeps the
     highlight on the first hit instead of pointing at a row that just
     scrolled off the top. */
  $effect(() => {
    void p.rows;
    selectedIdx = 0;
  });

  /** Compute fixed-position coords from the anchor rect. Re-derived on
   *  scroll / resize via the window listener below so the popover
   *  stays glued to the input. */
  let coords = $state<{ left: number; top: number; width: number } | null>(null);
  function recompute() {
    if (!p.anchor) {
      coords = null;
      return;
    }
    const r = p.anchor.getBoundingClientRect();
    coords = { left: r.left, top: r.bottom + 4, width: r.width };
  }
  $effect(() => {
    void p.open;
    void p.anchor;
    if (!p.open) {
      coords = null;
      return;
    }
    recompute();
  });
  $effect(() => {
    if (!p.open) return;
    const onScrollResize = () => recompute();
    window.addEventListener('scroll', onScrollResize, true);
    window.addEventListener('resize', onScrollResize);
    return () => {
      window.removeEventListener('scroll', onScrollResize, true);
      window.removeEventListener('resize', onScrollResize);
    };
  });

  /** Arrow-key + Enter handling. The parent's keydown handler should
   *  forward the event to us only when the popover is open. We do
   *  NOT preventDefault on Enter when there's nothing to pick — lets
   *  the parent's own Enter behaviour (e.g. hot-open) still fire. */
  export function handleKey(e: KeyboardEvent): boolean {
    if (!p.open || p.rows.length === 0) return false;
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      selectedIdx = Math.min(selectedIdx + 1, p.rows.length - 1);
      return true;
    }
    if (e.key === 'ArrowUp') {
      e.preventDefault();
      selectedIdx = Math.max(selectedIdx - 1, 0);
      return true;
    }
    if (e.key === 'Enter') {
      e.preventDefault();
      const row = p.rows[selectedIdx];
      if (row) p.onPick(row.id);
      return true;
    }
    if (e.key === 'Escape') {
      e.preventDefault();
      p.onClose();
      return true;
    }
    return false;
  }

  /* Outside-click: anything that's neither the anchor nor the popover
     dismisses. Ignores clicks on the anchor itself so the user can
     re-focus the input without a flicker. */
  $effect(() => {
    if (!p.open) return;
    function onDown(e: MouseEvent) {
      const t = e.target as Node;
      if (popoverEl && popoverEl.contains(t)) return;
      if (p.anchor && p.anchor.contains(t)) return;
      p.onClose();
    }
    window.addEventListener('mousedown', onDown);
    return () => window.removeEventListener('mousedown', onDown);
  });

  function pick(id: string) {
    p.onPick(id);
  }
</script>

{#if p.open && coords && p.rows.length > 0}
  <div
    bind:this={popoverEl}
    class="lsp"
    data-source={p.source}
    style:left="{coords.left}px"
    style:top="{coords.top}px"
    style:width="{coords.width}px"
    role="listbox"
  >
    <div class="lsp-head mono" data-source={p.source}>{p.source}</div>
    <div class="lsp-list">
      {#each p.rows as row, i (row.id)}
        <button
          class="lsp-row"
          class:active={i === selectedIdx}
          role="option"
          aria-selected={i === selectedIdx}
          onmouseenter={() => (selectedIdx = i)}
          onmousedown={(e) => {
            /* mousedown (not click) so the input doesn't blur first
               and dismiss us before we fire the pick. */
            e.preventDefault();
            pick(row.id);
          }}
        >
          <span class="lsp-dot" data-source={p.source} aria-hidden="true"></span>
          <span class="lsp-title">{row.title}</span>
          <span class="lsp-sub mono">{row.sub}</span>
        </button>
      {/each}
    </div>
    <div class="lsp-foot mono">
      <span><kbd>↑↓</kbd> navigate</span>
      <span><kbd>↵</kbd> open</span>
      <span><kbd>esc</kbd> close</span>
    </div>
  </div>
{/if}

<style>
  .lsp {
    position: fixed;
    z-index: 50;
    background: var(--bg-2);
    border: 1px solid var(--border-hi);
    border-radius: 10px;
    box-shadow: 0 14px 32px -8px rgba(0, 0, 0, 0.55);
    overflow: hidden;
    display: flex; flex-direction: column;
    max-height: 380px;
  }
  .lsp-head {
    padding: 8px 12px 6px;
    font-size: 9.5px;
    font-weight: 700;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    color: var(--text-mute);
    border-bottom: 1px solid var(--border);
  }
  .lsp-head[data-source="jira"]   { color: var(--src-jira); }
  .lsp-head[data-source="github"] { color: var(--src-github); }
  .lsp-head[data-source="sentry"] { color: var(--src-sentry); }

  .lsp-list {
    flex: 1;
    overflow-y: auto;
    padding: 4px;
  }
  .lsp-row {
    display: grid;
    grid-template-columns: 12px 1fr auto;
    align-items: center;
    gap: 10px;
    width: 100%;
    padding: 7px 10px;
    border: 0;
    background: transparent;
    border-radius: 7px;
    text-align: left;
    cursor: pointer;
    transition: background 100ms;
  }
  .lsp-row.active { background: var(--bg-3); }
  .lsp-dot {
    width: 6px; height: 6px;
    border-radius: 50%;
    flex-shrink: 0;
  }
  .lsp-dot[data-source="jira"]   { background: var(--src-jira); }
  .lsp-dot[data-source="github"] { background: var(--src-github); }
  .lsp-dot[data-source="sentry"] { background: var(--src-sentry); }
  .lsp-title {
    font-size: 12.5px;
    color: var(--text-0);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .lsp-sub {
    font-size: 10.5px;
    color: var(--text-mute);
    flex-shrink: 0;
  }

  .lsp-foot {
    display: flex; gap: 14px;
    padding: 6px 12px;
    border-top: 1px solid var(--border);
    background: var(--bg-1);
    font-size: 9.5px;
    color: var(--text-mute);
  }
  .lsp-foot kbd {
    display: inline-block;
    padding: 0 4px;
    border-radius: 3px;
    background: var(--bg-3);
    border: 1px solid var(--border);
    color: var(--text-1);
    margin-right: 3px;
    font-family: inherit;
    font-size: 9px;
  }
</style>
