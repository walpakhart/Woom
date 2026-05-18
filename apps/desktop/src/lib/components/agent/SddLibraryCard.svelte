<script lang="ts">
  /* SDD library — inline list of every SDD workspace on disk.
   *
   * Toggled by the composer's [📚] button. Renders right after the
   * latest visible message (same anchor as SddCard) so the user sees
   * their spec history in the chat flow, not in a separate panel.
   * Each row: stage glyph + ask snippet + phase progress + actions
   * ("open" rebinds the workspace to the current session; "discard"
   * wipes the dir). Visual language matches the active SddCard:
   * accent-soft tint, left stripe, prose-typography rows, text-buttons.
   */
  import { fly } from 'svelte/transition';
  import { cubicOut } from 'svelte/easing';
  import {
    sddState,
    bindWorkspaceToSession,
    discardSdd,
    type SddWorkspace,
    type SddPhase,
  } from '$lib/state/sdd.svelte';

  interface Props {
    sessionId: string;
    /** Called after the user picked "open" — gives the parent a chance
     *  to scroll the SddCard into view / focus. Optional. */
    onOpened?: () => void;
  }
  let p: Props = $props();

  /* Newest-first list of all workspaces (the store already sorts on
   *  upsert; just expose it). */
  const workspaces = $derived(sddState.workspaces);

  /* Compact stage label — terse, fits in the one-line row. */
  function stageLabel(w: SddWorkspace): string {
    const s = w.stage;
    switch (s.kind) {
      case 'drafting': return 'drafting spec';
      case 'spec_ready': return 'spec ready';
      case 'planning': return 'drafting plan';
      case 'plan_ready': return 'plan ready';
      case 'phase_running': return `phase ${s.phase} running`;
      case 'phase_done': return `phase ${s.phase} done`;
      case 'complete': return 'complete';
      case 'paused': return 'paused';
      case 'stopped': return 'stopped';
      case 'failed': return 'failed';
    }
  }

  function stageTone(w: SddWorkspace): 'live' | 'ok' | 'warn' | 'dim' {
    const k = w.stage.kind;
    if (k === 'drafting' || k === 'planning' || k === 'phase_running') return 'live';
    if (k === 'failed' || k === 'stopped') return 'warn';
    if (k === 'complete') return 'ok';
    return 'dim';
  }

  function phaseProgress(w: SddWorkspace): string {
    if (w.phases.length === 0) return '';
    const done = w.phases.filter((ph: SddPhase) => ph.status === 'done').length;
    return `${done}/${w.phases.length}`;
  }

  function timeAgo(ms: number): string {
    const s = Math.max(0, Math.floor((Date.now() - ms) / 1000));
    if (s < 60) return `${s}s`;
    const m = Math.floor(s / 60);
    if (m < 60) return `${m}m`;
    const h = Math.floor(m / 60);
    if (h < 24) return `${h}h`;
    const d = Math.floor(h / 24);
    return `${d}d`;
  }

  function open(workspaceId: string) {
    bindWorkspaceToSession(p.sessionId, workspaceId);
    p.onOpened?.();
  }

  async function discard(workspaceId: string) {
    await discardSdd(workspaceId);
  }

  const activeId = $derived(sddState.workspaceBySession[p.sessionId] ?? null);
</script>

<aside class="lib" in:fly={{ y: 8, duration: 180, easing: cubicOut }}>
  <header class="lib-head">
    <span class="lib-glyph" aria-hidden="true">SDD</span>
    <span class="lib-title">history</span>
    <span class="lib-count mono">{workspaces.length}</span>
  </header>

  {#if workspaces.length === 0}
    <div class="lib-empty">No specs yet. Click [SDD] in the composer to start one.</div>
  {:else}
    <ul class="lib-rows" role="list">
      {#each workspaces as w (w.id)}
        {@const isActive = w.id === activeId}
        <li class="lib-row" data-tone={stageTone(w)} class:active={isActive}>
          <button
            type="button"
            class="lib-row-main"
            onclick={() => open(w.id)}
            title="Open this workspace in the current chat"
          >
            <span class="lib-row-stage">{stageLabel(w)}</span>
            <span class="lib-row-ask">{w.user_prompt || '(no ask recorded)'}</span>
            {#if phaseProgress(w)}
              <span class="lib-row-progress mono">{phaseProgress(w)}</span>
            {/if}
            <span class="lib-row-time mono" title="Created {new Date(w.created_at).toLocaleString()}">
              {timeAgo(w.created_at)}
            </span>
          </button>
          {#if w.phases.length > 0}
            <div class="lib-row-phases">
              {#each w.phases as ph (ph.number)}
                <span class="lib-phase-dot" data-status={ph.status} title="{ph.number}. {ph.title} · {ph.status}">
                  {ph.number}
                </span>
              {/each}
            </div>
          {/if}
          <button
            type="button"
            class="lib-row-discard"
            onclick={() => discard(w.id)}
            title="Delete this workspace from disk"
          >discard</button>
        </li>
      {/each}
    </ul>
  {/if}
</aside>

<style>
  /* Library card — same blockquote chrome as SddCard so the two read
   *  as siblings: accent-soft tint, 3 px left accent stripe, rounded
   *  only on the right. Quiet enough to scroll past, dense enough to
   *  scan many workspaces at a glance. */
  .lib {
    border-left: 3px solid var(--accent);
    border-radius: 0 6px 6px 0;
    background: var(--accent-soft);
    padding: 10px 14px 11px;
    display: flex; flex-direction: column;
    gap: 8px;
    min-width: 0;
    color: var(--text-1);
    font-size: 13px;
    line-height: 1.5;
  }

  .lib-head {
    display: flex; align-items: baseline;
    gap: 8px;
    font-size: 12px;
  }
  .lib-glyph {
    font-family: 'JetBrains Mono', monospace;
    font-size: 9.5px;
    font-weight: 700;
    letter-spacing: 0.14em;
    color: var(--accent-bright);
    text-transform: uppercase;
  }
  .lib-glyph::after {
    content: '·';
    color: var(--text-mute);
    margin-left: 8px;
  }
  .lib-title { font-weight: 500; color: var(--text-0); }
  .lib-count {
    margin-left: auto;
    font-size: 10px;
    color: var(--text-mute);
  }

  .lib-empty {
    color: var(--text-mute);
    font-style: italic;
    padding: 4px 0;
  }

  .lib-rows {
    list-style: none;
    margin: 0; padding: 0;
    display: flex; flex-direction: column;
    gap: 2px;
  }
  .lib-row {
    display: flex; flex-wrap: wrap; align-items: center;
    gap: 6px 12px;
    padding: 4px 0;
  }
  .lib-row.active .lib-row-stage {
    color: var(--accent-bright);
    font-weight: 600;
  }

  /* Main clickable row — text-button aesthetic, no panel. Spans the
   *  available width; phase dots + discard sit beside it. */
  .lib-row-main {
    flex: 1;
    display: inline-flex; align-items: baseline;
    gap: 10px;
    padding: 2px 0;
    background: transparent;
    border: 0;
    color: var(--text-1);
    cursor: pointer;
    text-align: left;
    font: inherit;
    min-width: 0;
  }
  .lib-row-main:hover .lib-row-ask { color: var(--text-0); }
  .lib-row-stage {
    font-family: 'JetBrains Mono', monospace;
    font-size: 10.5px;
    color: var(--text-mute);
    flex-shrink: 0;
    min-width: 110px;
  }
  .lib-row[data-tone="live"] .lib-row-stage { color: #66d39a; }
  .lib-row[data-tone="warn"] .lib-row-stage { color: #e0b16c; }
  .lib-row[data-tone="ok"] .lib-row-stage { color: var(--accent-bright); }
  .lib-row-ask {
    flex: 1;
    min-width: 0;
    color: var(--text-1);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    transition: color 120ms;
  }
  .lib-row-progress {
    flex-shrink: 0;
    font-size: 10.5px;
    color: var(--text-mute);
  }
  .lib-row-time {
    flex-shrink: 0;
    font-size: 10px;
    color: var(--text-mute);
  }

  /* Phase pills row — tiny numbered dots, color picks up status. */
  .lib-row-phases {
    display: inline-flex;
    gap: 3px;
    flex-shrink: 0;
  }
  .lib-phase-dot {
    display: inline-grid; place-items: center;
    width: 16px; height: 16px;
    border-radius: 50%;
    background: var(--bg-2);
    color: var(--text-mute);
    font-family: 'JetBrains Mono', monospace;
    font-size: 9.5px;
    border: 1px solid var(--border-neutral);
  }
  .lib-phase-dot[data-status="running"] {
    color: #66d39a;
    border-color: color-mix(in srgb, #66d39a 50%, var(--border-neutral));
    background: color-mix(in srgb, #66d39a 12%, var(--bg-2));
  }
  .lib-phase-dot[data-status="done"] {
    color: var(--text-0);
    border-color: color-mix(in srgb, var(--accent) 50%, var(--border-neutral));
    background: color-mix(in srgb, var(--accent) 14%, var(--bg-2));
  }
  .lib-phase-dot[data-status="failed"] {
    color: #e0b16c;
    border-color: color-mix(in srgb, #e0b16c 50%, var(--border-neutral));
    background: color-mix(in srgb, #e0b16c 10%, var(--bg-2));
  }

  .lib-row-discard {
    border: 0;
    background: transparent;
    color: var(--text-mute);
    cursor: pointer;
    font-size: 11px;
    padding: 2px 0;
    flex-shrink: 0;
    transition: color 120ms;
  }
  .lib-row-discard:hover { color: var(--error); }
</style>
