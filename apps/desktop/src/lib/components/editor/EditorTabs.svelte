<script lang="ts">
  /* EditorTabs — top tab strip for the Editor's main pane. Pure
     template: state (`tabs`, `activePath`, `dirtyByPath`, `diffTarget`)
     and handlers (`switchTab`, `closeTab`, `closeDiff`,
     `onTabMiddleClick`) all live in EditorView because they share
     ownership of the buffer cache with the actual <Editor> component
     below. We accept them as props so this file stays a thin renderer.
     The tabbar element ref is bound back out to the parent so its
     "scroll active tab into view" effect can keep working. */
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
    tabbarEl = $bindable(null),
  }: Props = $props();
</script>

<div class="ev-tabbar" bind:this={tabbarEl}>
  {#if diffTarget}
    <div class="ev-tab-wrap active" title={diffTarget.path}>
      <button class="ev-tab-btn" onclick={onCloseDiff}>
        <span class="ev-tab-diff-icon" title="Diff">Δ</span>
        <span class="ev-tab-name mono">{diffTarget.path}</span>
        <span class="ev-tab-side">{diffTarget.staged ? 'staged' : 'working'}</span>
      </button>
      <button class="ev-tab-x" onclick={onCloseDiff} title="Close diff">
        <svg class="i i-sm" viewBox="0 0 24 24"><path d="M6 6l12 12M6 18L18 6" /></svg>
      </button>
    </div>
  {:else if tabs.length === 0}
    <div class="ev-tab-empty">Pick a file in the tree to open it here.</div>
  {:else}
    {#each tabs as path (path)}
      <div
        class="ev-tab-wrap"
        class:active={path === activePath}
        class:dirty={dirtyByPath[path]}
        title={path}
      >
        <button
          class="ev-tab-btn"
          onclick={() => onSwitch(path)}
          onauxclick={(e) => onMiddleClick(path, e)}
        >
          <span class="ev-tab-name mono">{tabDisplayName(path)}</span>
        </button>
        <button class="ev-tab-x" onclick={(e) => onClose(path, e)} title={dirtyByPath[path] ? 'Close (unsaved)' : 'Close'}>
          {#if dirtyByPath[path]}
            <span class="ev-tab-dot"></span>
          {:else}
            <svg class="i i-sm" viewBox="0 0 24 24"><path d="M6 6l12 12M6 18L18 6" /></svg>
          {/if}
        </button>
      </div>
    {/each}
  {/if}
</div>

<style>
  /* Top tab strip styles — moved from EditorView during the wave-1
     phase-4 split. Class names kept the `.ev-tab-*` prefix so the
     editor's existing visual rhythm stays untouched (Svelte scopes
     each per-component selector with its own hash, so these are
     isolated to this file even though the names look shared). */
  .ev-tabbar {
    display: flex; align-items: center; gap: 2px;
    padding: 0 12px;
    min-height: 36px;
    background: var(--bg-1);
    overflow-x: auto;
    flex-shrink: 0;
    border-bottom: 1px solid var(--border);
  }
  .ev-tabbar::-webkit-scrollbar { height: 0; }
  .ev-tab-empty {
    padding: 6px 10px;
    font-size: 12px; color: var(--text-mute);
    white-space: nowrap;
  }
  /* Mockup tabs: flat text tabs, active = 600 weight + 2px editor
     underline; modification dot in claude tone. No pill chrome. */
  .ev-tab-wrap {
    display: inline-flex; align-items: center; gap: 0;
    height: 34px;
    padding: 0 6px 0 13px;
    background: transparent;
    border: 0;
    border-bottom: 2px solid transparent;
    border-radius: 0;
    flex-shrink: 0;
    max-width: 260px;
    transition: color 120ms, border-color 120ms;
    cursor: pointer;
  }
  .ev-tab-wrap:hover { background: var(--bg-hover); }
  .ev-tab-wrap.active {
    background: transparent;
    border-bottom-color: var(--src-editor);
  }
  .ev-tab-wrap.active .ev-tab-name { font-weight: 600; }
  /* Modification dot — claude tone when dirty, hidden otherwise. */
  .ev-tab-wrap::before {
    content: '';
    flex-shrink: 0;
    width: 6px; height: 6px;
    border-radius: 50%;
    margin-right: 7px;
    background: transparent;
    transition: background 140ms;
  }
  .ev-tab-wrap.active::before { background: var(--src-editor); }
  .ev-tab-wrap.dirty::before { background: var(--src-claude); }
  .ev-tab-btn {
    display: inline-flex; align-items: center; gap: 6px;
    padding: 0;
    font-size: 12.5px; color: var(--text-1);
    background: transparent; border: 0;
    min-width: 0;
    cursor: pointer;
  }
  .ev-tab-wrap.active .ev-tab-btn { color: var(--text-0); }
  .ev-tab-name {
    font-family: var(--font-mono);
    font-size: 11.5px;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .ev-tab-x {
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
  .ev-tab-x:hover { background: rgba(232, 130, 100, 0.10); color: var(--error); }
  .ev-tab-x :global(svg) { width: 10px; height: 10px; }
  /* Inline dirty dot inside the close-button slot — only used when
     the buffer is unsaved and the user hasn't hovered the row yet. */
  .ev-tab-dot { width: 6px; height: 6px; border-radius: 50%; background: var(--warning); box-shadow: var(--shadow-1); }
  .ev-tab-diff-icon {
    color: var(--accent-bright); font-weight: 700;
    width: 14px; text-align: center;
    flex-shrink: 0;
  }
  .ev-tab-side {
    font-size: 10px; padding: 1px 5px;
    border-radius: 3px; background: var(--bg-3);
    color: var(--text-2);
    flex-shrink: 0;
  }
</style>
