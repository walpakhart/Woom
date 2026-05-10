<script lang="ts">
  import { fly } from 'svelte/transition';
  import { cubicOut } from 'svelte/easing';
  import { onMount } from 'svelte';
  import { toasterState, dismissToast, type Toast } from '$lib/state/toaster.svelte';

  // 60-fps tick that decrements `ttl` on every visible non-sticky toast and
  // auto-dismisses when it hits zero. One timer for the whole stack.
  onMount(() => {
    let last = performance.now();
    let raf = 0;
    const tick = (now: number) => {
      const dt = now - last;
      last = now;
      // Iterate in REVERSE so `dismissToast` splicing the array
      // mid-loop doesn't make a forward iterator skip the toast that
      // shifted into the just-removed slot. With forward iteration
      // and N expired toasts in one frame, every other one would
      // survive an extra frame.
      for (let i = toasterState.items.length - 1; i >= 0; i--) {
        const t = toasterState.items[i];
        if (!t || t.ttl === null) continue;
        t.ttl -= dt;
        if (t.ttl <= 0) dismissToast(t.id);
      }
      raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  });

  function iconFor(kind: Toast['kind']): string {
    switch (kind) {
      case 'success': return 'M5 12l5 5L20 7';
      case 'warning': return 'M12 9v4M12 17h.01M10.29 3.86 1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z';
      case 'error':   return 'M18 6 6 18M6 6l12 12';
      default:        return 'M12 16v-4M12 8h.01M12 22a10 10 0 1 0 0-20 10 10 0 0 0 0 20z';
    }
  }
</script>

<div class="toaster" aria-live="polite" aria-atomic="false">
  {#each toasterState.items as t (t.id)}
    <div
      class="toast toast--{t.kind}"
      role={t.kind === 'error' ? 'alert' : 'status'}
      transition:fly={{ x: 24, duration: 220, easing: cubicOut }}
    >
      <span class="toast-ico" aria-hidden="true">
        <svg viewBox="0 0 24 24"><path d={iconFor(t.kind)}/></svg>
      </span>
      <div class="toast-body">
        <div class="toast-title">{t.title}</div>
        {#if t.body}<div class="toast-sub">{t.body}</div>{/if}
        {#if t.actions && t.actions.length}
          <div class="toast-actions">
            {#each t.actions as a (a.label)}
              <button type="button" class="toast-act" onclick={() => { a.onClick(); dismissToast(t.id); }}>{a.label}</button>
            {/each}
          </div>
        {/if}
      </div>
      <button type="button" class="toast-x" onclick={() => dismissToast(t.id)} aria-label="Dismiss">
        <svg viewBox="0 0 24 24"><path d="M6 6l12 12M6 18L18 6"/></svg>
      </button>
    </div>
  {/each}
</div>

<style>
  .toaster {
    position: fixed; top: 44px; right: 16px;
    display: flex; flex-direction: column; gap: 8px;
    z-index: 600;
    pointer-events: none;
    width: 360px; max-width: calc(100vw - 32px);
  }
  .toast {
    --toast-tone: var(--text-1);
    --toast-glow: rgba(0, 0, 0, 0);
    pointer-events: auto;
    position: relative;
    display: grid;
    grid-template-columns: 18px 1fr 18px;
    gap: 10px;
    align-items: start;
    padding: 11px 12px 11px 14px;
    border-radius: 10px;
    background: var(--bg-2);
    border: 1px solid var(--border-hi);
    box-shadow:
      var(--shadow-2),
      0 0 0 1px color-mix(in srgb, var(--toast-tone) 12%, transparent),
      0 1px 0 rgba(255, 240, 220, 0.04) inset;
    color: var(--text-0);
    font-size: 12.5px;
    overflow: hidden;
  }
  .toast::before {
    content: '';
    position: absolute; left: 0; top: 0; bottom: 0;
    width: 2.5px;
    background: var(--toast-tone);
    box-shadow: 0 0 10px var(--toast-glow);
  }
  .toast--success { --toast-tone: var(--success); --toast-glow: rgba(168, 217, 184, 0.40); }
  .toast--warning { --toast-tone: var(--warning); --toast-glow: rgba(217, 184, 110, 0.40); }
  .toast--error   { --toast-tone: var(--error);   --toast-glow: rgba(232, 130, 100, 0.42); }
  .toast--info    { --toast-tone: var(--info);    --toast-glow: rgba(136, 194, 221, 0.40); }
  .toast-ico {
    width: 18px; height: 18px;
    display: inline-flex; align-items: center; justify-content: center;
    color: var(--text-1);
    margin-top: 1px;
  }
  .toast--success .toast-ico { color: var(--success); }
  .toast--warning .toast-ico { color: var(--warning); }
  .toast--error   .toast-ico { color: var(--error); }
  .toast-ico svg { width: 14px; height: 14px; stroke: currentColor; stroke-width: 2.2; fill: none; stroke-linecap: round; stroke-linejoin: round; }
  .toast-body { min-width: 0; }
  .toast-title {
    font-weight: 600; color: var(--text-0); line-height: 1.35;
    overflow-wrap: anywhere;
  }
  .toast-sub {
    margin-top: 3px;
    color: var(--text-2); font-size: 11.5px; line-height: 1.45;
    overflow-wrap: anywhere;
    max-height: 6.4em; overflow: hidden;
  }
  .toast-actions { margin-top: 8px; display: flex; gap: 6px; flex-wrap: wrap; }
  .toast-act {
    padding: 4px 9px;
    background: var(--bg-3); border: 1px solid var(--border-neutral-hi);
    border-radius: 5px;
    color: var(--text-1); font-size: 11.5px; font-weight: 500;
    cursor: pointer;
  }
  .toast-act:hover { background: var(--accent-soft); color: var(--accent-bright); border-color: var(--accent); }
  .toast-x {
    width: 18px; height: 18px;
    display: inline-flex; align-items: center; justify-content: center;
    background: transparent; border: none; cursor: pointer;
    color: var(--text-mute); border-radius: 4px;
  }
  .toast-x:hover { background: var(--bg-3); color: var(--text-1); }
  .toast-x svg { width: 11px; height: 11px; stroke: currentColor; stroke-width: 2; fill: none; stroke-linecap: round; }
</style>
