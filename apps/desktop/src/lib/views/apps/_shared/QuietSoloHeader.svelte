<script lang="ts">
  /* Quiet-direction document eyebrow shared by the non-chat solos
     (Jira / GitHub / Sentry, redesign v2 §3.4). Mirrors the chat solo's
     QuietChatHeader grammar: a compact identifier row + a «N items ▾»
     switcher popover (the list panel moves in here in Quiet) + solo
     actions rendered as dotted links. The big title + meta stay in the
     document body below — this only owns the switcher + action chrome
     that the Cabin toolbar head used to carry. */
  import type { Snippet } from 'svelte';

  export interface QsoloSwitchItem {
    id: string;
    label: string;
    sub?: string;
    active?: boolean;
    running?: boolean;
  }

  interface Props {
    count: number;
    /** Noun after the count, e.g. "tickets" / "PR" / "errors". */
    noun: string;
    items: QsoloSwitchItem[];
    onPick: (id: string) => void;
    ariaLabel?: string;
    /** Identifier chips shown before the switcher (key / tags). */
    lead?: Snippet;
    /** Solo actions rendered on the right as dotted links. */
    actions?: Snippet;
  }
  let p: Props = $props();

  let switcherOpen = $state(false);
  function pick(id: string) {
    p.onPick(id);
    switcherOpen = false;
  }

  $effect(() => {
    if (!switcherOpen) return;
    const onDown = (e: MouseEvent) => {
      const t = e.target as HTMLElement | null;
      if (t?.closest?.('.qsolo-switch-wrap')) return;
      switcherOpen = false;
    };
    window.addEventListener('mousedown', onDown, true);
    return () => window.removeEventListener('mousedown', onDown, true);
  });
</script>

<header class="qsolo-head">
  <div class="qsolo-eyebrow">
    {#if p.lead}<span class="qsolo-lead">{@render p.lead()}</span>{/if}

    <div class="qsolo-switch-wrap">
      <button
        class="qsolo-switch"
        class:open={switcherOpen}
        onclick={() => (switcherOpen = !switcherOpen)}
        title="Switch"
        aria-expanded={switcherOpen}
      >
        {p.count} {p.noun} <span class="qsolo-caret" aria-hidden="true">▾</span>
      </button>
      {#if switcherOpen}
        <div class="qsolo-switch-pop" role="listbox" aria-label={p.ariaLabel ?? 'Switch'}>
          {#each p.items as it (it.id)}
            <button
              class="qsolo-switch-item"
              class:active={it.active}
              onclick={() => pick(it.id)}
              role="option"
              aria-selected={it.active}
            >
              <span class="qsolo-switch-dot" class:running={it.running} aria-hidden="true"></span>
              <span class="qsolo-switch-name">{it.label}</span>
              {#if it.sub}<span class="qsolo-switch-sub mono">{it.sub}</span>{/if}
            </button>
          {/each}
          {#if p.items.length === 0}
            <div class="qsolo-switch-empty">Nothing here yet.</div>
          {/if}
        </div>
      {/if}
    </div>

    <span class="qsolo-spring"></span>

    {#if p.actions}<span class="qsolo-actions">{@render p.actions()}</span>{/if}
  </div>
</header>

<style>
  .qsolo-head {
    flex: none;
    width: 100%; max-width: 800px; margin: 0 auto;
    padding: 6px 0 4px;
  }
  .qsolo-eyebrow {
    display: flex; align-items: center; gap: 10px;
    min-height: 22px;
  }
  .qsolo-lead {
    display: inline-flex; align-items: center; gap: 8px;
    min-width: 0;
  }

  .qsolo-switch-wrap { position: relative; flex: none; }
  .qsolo-switch {
    background: transparent; border: 0; cursor: pointer;
    font-size: 12px; color: var(--text-faint);
    display: inline-flex; align-items: center; gap: 4px;
    padding: 0;
  }
  .qsolo-switch:hover, .qsolo-switch.open { color: var(--text-1); }
  .qsolo-caret { font-size: 9px; opacity: 0.8; }
  .qsolo-switch-pop {
    position: absolute; top: calc(100% + 8px); left: 0; z-index: 200;
    min-width: 300px; max-width: 420px; max-height: 360px; overflow-y: auto;
    padding: 4px;
    background: var(--bg-1); border: 1px solid var(--border-hi);
    border-radius: 12px; box-shadow: var(--shadow-3);
    display: flex; flex-direction: column; gap: 1px;
  }
  .qsolo-switch-item {
    display: flex; align-items: center; gap: 8px;
    padding: 8px 10px; border-radius: 8px;
    background: transparent; border: 0; cursor: pointer; text-align: left;
    color: var(--text-1); font-size: 13px;
  }
  .qsolo-switch-item:hover { background: var(--bg-hover); color: var(--text-0); }
  .qsolo-switch-item.active { background: var(--bg-3); color: var(--text-0); box-shadow: var(--shadow-1); }
  .qsolo-switch-dot {
    width: 6px; height: 6px; border-radius: 50%; flex: none;
    background: var(--text-linenum, var(--text-mute));
  }
  .qsolo-switch-dot.running { background: var(--ok); }
  .qsolo-switch-name { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .qsolo-switch-sub { flex: none; font-size: 10.5px; color: var(--text-mute); }
  .qsolo-switch-empty { padding: 8px 10px; font-size: 12px; color: var(--text-mute); }

  .qsolo-spring { flex: 1; }
  .qsolo-actions { display: inline-flex; align-items: center; gap: 14px; flex: none; }
</style>
