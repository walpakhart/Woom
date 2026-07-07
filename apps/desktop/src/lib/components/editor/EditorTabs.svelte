<script lang="ts">
  /* EditorTabs — top tab strip for the Editor's main pane. Pure
     template: state (`tabs`, `activePath`, `dirtyByPath`, `diffTarget`)
     and handlers (`switchTab`, `closeTab`, `closeDiff`,
     `onTabMiddleClick`) all live in EditorView because they share
     ownership of the buffer cache with the actual <Editor> component
     below. We accept them as props so this file stays a thin renderer.
     The tabbar element ref is bound back out to the parent so its
     "scroll active tab into view" effect can keep working.

     Rebuilt to mockup 4i / README §2.7 with fresh `.etab-` markup:
     38px chip-tab strip (active bg-3 + modified warn-dot) and a
     right-anchored cursor/lang readout (`1042:17 · svelte · lf`). */
  interface Props {
    tabs: string[];
    activePath: string;
    dirtyByPath: Record<string, boolean>;
    diffTarget: { path: string; staged: boolean } | null;
    /** How to display the tab label (parent handles dedup against
     *  shared basenames by prepending the immediate parent folder). */
    tabDisplayName: (path: string) => string;
    onSwitch: (path: string) => void;
    onClose: (path: string, ev?: MouseEvent) => void;
    onMiddleClick: (path: string, ev: MouseEvent) => void;
    onCloseDiff: () => void;
    /** Cursor readout for the right end of the strip. Null until the
     *  active buffer emits its first selection event. */
    cursorInfo?: { line: number; col: number; lineEndings: 'lf' | 'crlf'; bytes: number } | null;
    /** Lower-cased language token for the readout (e.g. `svelte`). */
    readoutLang?: string;
    /** Bound back to the parent so it can `scrollIntoView` the active
     *  tab on path change. `null` until the element mounts. */
    tabbarEl?: HTMLDivElement | null;
  }
  let {
    tabs,
    activePath,
    dirtyByPath,
    diffTarget,
    tabDisplayName,
    onSwitch,
    onClose,
    onMiddleClick,
    onCloseDiff,
    cursorInfo = null,
    readoutLang = '',
    tabbarEl = $bindable(null),
  }: Props = $props();
</script>

<div class="etab-bar" bind:this={tabbarEl}>
  <div class="etab-tabs">
    {#if diffTarget}
      <div class="etab-tab active" title={diffTarget.path}>
        <button class="etab-btn" onclick={onCloseDiff}>
          <span class="etab-diff-icon" title="Diff">Δ</span>
          <span class="etab-name mono">{diffTarget.path}</span>
          <span class="etab-side">{diffTarget.staged ? 'staged' : 'working'}</span>
        </button>
        <button class="etab-x" onclick={onCloseDiff} title="Close diff">
          <svg class="i i-sm" viewBox="0 0 24 24"><path d="M6 6l12 12M6 18L18 6" /></svg>
        </button>
      </div>
    {:else if tabs.length === 0}
      <div class="etab-empty">Pick a file in the tree to open it here.</div>
    {:else}
      {#each tabs as path (path)}
        <div
          class="etab-tab"
          class:active={path === activePath}
          class:dirty={dirtyByPath[path]}
          title={path}
        >
          <button
            class="etab-btn"
            onclick={() => onSwitch(path)}
            onauxclick={(e) => onMiddleClick(path, e)}
          >
            <span class="etab-name mono">{tabDisplayName(path)}</span>
          </button>
          <button class="etab-x" onclick={(e) => onClose(path, e)} title={dirtyByPath[path] ? 'Close (unsaved)' : 'Close'}>
            {#if dirtyByPath[path]}
              <span class="etab-dot"></span>
            {:else}
              <svg class="i i-sm" viewBox="0 0 24 24"><path d="M6 6l12 12M6 18L18 6" /></svg>
            {/if}
          </button>
        </div>
      {/each}
    {/if}
  </div>

  {#if activePath || diffTarget}
    <div class="etab-readout mono">
      {#if cursorInfo}<span>{cursorInfo.line}:{cursorInfo.col}</span>{/if}
      {#if cursorInfo && readoutLang}<span class="etab-readout-sep">·</span>{/if}
      {#if readoutLang}<span>{readoutLang}</span>{/if}
      {#if cursorInfo}<span class="etab-readout-sep">·</span><span>{cursorInfo.lineEndings}</span>{/if}
    </div>
  {/if}
</div>

<style>
  /* Mockup 4i / README §2.7 — 38px strip, chip tabs (active bg-3),
     modified dot in the trailing slot (warn tone), and a faint mono
     cursor/lang readout pinned to the right end. Svelte scopes each
     selector to this file, so the fresh `.etab-` names are isolated. */
  .etab-bar {
    display: flex; align-items: center;
    min-height: 38px;
    padding: 0 12px;
    background: var(--bg-1);
    flex-shrink: 0;
    border-bottom: 1px solid var(--border);
  }
  .etab-tabs {
    display: flex; align-items: center; gap: 3px;
    flex: 1 1 auto; min-width: 0;
    overflow-x: auto;
  }
  .etab-tabs::-webkit-scrollbar { height: 0; }
  .etab-empty {
    padding: 6px 10px;
    font-size: 12px; color: var(--text-mute);
    white-space: nowrap;
  }
  .etab-tab {
    display: inline-flex; align-items: center; gap: 0;
    height: 30px;
    padding: 0 6px 0 12px;
    background: transparent;
    border: 0;
    border-radius: 7px;
    flex-shrink: 0;
    max-width: 260px;
    transition: color 120ms, background 120ms;
    cursor: pointer;
  }
  .etab-tab:hover { background: var(--bg-hover); }
  .etab-tab.active { background: var(--bg-3); }
  .etab-tab.active .etab-name { font-weight: 600; }
  .etab-btn {
    display: inline-flex; align-items: center; gap: 6px;
    padding: 0;
    font-size: 12.5px; color: var(--text-1);
    background: transparent; border: 0;
    min-width: 0;
    cursor: pointer;
  }
  .etab-tab.active .etab-btn { color: var(--text-0); }
  .etab-name {
    font-family: var(--font-mono);
    font-size: 11.5px;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .etab-x {
    display: inline-flex; align-items: center; justify-content: center;
    width: 18px; height: 18px; border-radius: 4px;
    margin-left: 6px;
    color: var(--text-mute);
    background: transparent; border: 0;
    align-self: center;
    flex-shrink: 0;
    cursor: pointer;
    transition: background 100ms, color 100ms;
  }
  .etab-x:hover { background: color-mix(in srgb, var(--err) 10%, transparent); color: var(--err); }
  .etab-x :global(svg) { width: 10px; height: 10px; }
  /* Inline dirty dot — only shown when the buffer is unsaved. */
  .etab-dot { width: 6px; height: 6px; border-radius: 50%; background: var(--warn); box-shadow: var(--shadow-1); }
  .etab-diff-icon {
    color: var(--accent-bright); font-weight: 700;
    width: 14px; text-align: center;
    flex-shrink: 0;
  }
  .etab-side {
    font-size: 10px; padding: 1px 5px;
    border-radius: 3px; background: var(--bg-3);
    color: var(--text-2);
    flex-shrink: 0;
  }
  /* Cursor / language readout — `1042:17 · svelte · lf`, mono 11 faint. */
  .etab-readout {
    display: inline-flex; align-items: center; gap: 5px;
    flex-shrink: 0;
    margin-left: 12px;
    font-size: 11px; color: var(--text-faint);
    white-space: nowrap;
  }
  .etab-readout-sep { color: var(--text-mute); opacity: 0.6; }
</style>
