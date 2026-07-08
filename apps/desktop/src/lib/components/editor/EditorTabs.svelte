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
    /** Quiet direction (mockup 4j): replace the tab strip with a path
     *  breadcrumb — `{file} ▾` (▾ opens an open-buffer switcher) + the
     *  repo-relative dir + a right readout (`● изменён · git · N ·
     *  review · N · {instance} · {repo}`). Cabin (false) keeps the tabs. */
    quiet?: boolean;
    crumbName?: string;
    crumbDir?: string;
    repoLabel?: string;
    instanceLabel?: string;
    gitCount?: number;
    reviewCount?: number;
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
    quiet = false,
    crumbName = '',
    crumbDir = '',
    repoLabel = '',
    instanceLabel = '',
    gitCount = 0,
    reviewCount = 0,
  }: Props = $props();

  /* Open-buffer switcher for the Quiet breadcrumb caret. */
  let crumbOpen = $state(false);
  function pickCrumb(path: string) {
    onSwitch(path);
    crumbOpen = false;
  }
  $effect(() => {
    if (!crumbOpen) return;
    const onDown = (e: MouseEvent) => {
      const t = e.target as HTMLElement | null;
      if (t?.closest?.('.etab-crumb-wrap')) return;
      crumbOpen = false;
    };
    window.addEventListener('mousedown', onDown, true);
    return () => window.removeEventListener('mousedown', onDown, true);
  });
</script>

<div class="etab-bar" class:etab-bar--quiet={quiet} bind:this={tabbarEl}>
  {#if quiet}
    {#if crumbName}
      <div class="etab-crumb-wrap">
        <button class="etab-crumb" onclick={() => (crumbOpen = !crumbOpen)} title="Switch open file" aria-expanded={crumbOpen}>
          <span class="etab-crumb-name mono">{crumbName}</span>
          {#if dirtyByPath[activePath] && !diffTarget}<span class="etab-crumb-dot" title="Unsaved"></span>{/if}
          <span class="etab-crumb-caret" aria-hidden="true">▾</span>
        </button>
        {#if crumbOpen}
          <div class="etab-crumb-pop" role="listbox" aria-label="Open files">
            {#each tabs as path (path)}
              <button
                class="etab-crumb-item"
                class:active={path === activePath}
                onclick={() => pickCrumb(path)}
                role="option"
                aria-selected={path === activePath}
                title={path}
              >
                <span class="etab-crumb-item-name mono">{tabDisplayName(path)}</span>
                {#if dirtyByPath[path]}<span class="etab-crumb-dot"></span>{/if}
              </button>
            {/each}
            {#if tabs.length === 0}<div class="etab-crumb-empty">No open files.</div>{/if}
          </div>
        {/if}
      </div>
      {#if crumbDir}<span class="etab-crumb-dir mono">{crumbDir}</span>{/if}
      <span class="etab-crumb-spring"></span>
      <div class="etab-crumb-status mono">
        {#if dirtyByPath[activePath] && !diffTarget}<span class="etab-crumb-mod">● изменён</span>{/if}
        {#if gitCount > 0}<span>git · {gitCount}</span>{/if}
        {#if reviewCount > 0}<span>review · {reviewCount}</span>{/if}
        <span class="etab-crumb-loc">{#if instanceLabel}{instanceLabel} · {/if}{repoLabel}</span>
      </div>
    {:else}
      <div class="etab-empty">Pick a file in the tree to open it here.</div>
    {/if}
  {:else}
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

  /* ── Quiet breadcrumb (mockup 4j) ──────────────────────────────────
     `{file} ▾  {dir}` on the left, a `● изменён · git · N · review · N ·
     {instance} · {repo}` readout on the right. Transparent bar so it
     reads as document chrome, not a toolbar. */
  .etab-bar--quiet {
    min-height: 44px;
    padding: 0 20px;
    background: transparent;
    border-bottom: 1px solid var(--border-lo, var(--border));
    gap: 10px;
  }
  .etab-crumb-wrap { position: relative; flex: none; }
  .etab-crumb {
    display: inline-flex; align-items: center; gap: 6px;
    padding: 2px 6px 2px 0;
    background: transparent; border: 0; cursor: pointer;
    color: var(--text-0);
  }
  .etab-crumb-name { font-size: 14px; font-weight: 600; }
  .etab-crumb-caret { font-size: 9px; color: var(--text-faint); }
  .etab-crumb:hover .etab-crumb-caret { color: var(--text-1); }
  .etab-crumb-dot {
    width: 6px; height: 6px; border-radius: 50%;
    background: var(--warn); flex: none;
  }
  .etab-crumb-dir {
    font-size: 12px; color: var(--text-faint);
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
    min-width: 0;
  }
  .etab-crumb-spring { flex: 1 1 auto; }
  .etab-crumb-status {
    display: inline-flex; align-items: center; flex: none;
    font-size: 11px; color: var(--text-faint); white-space: nowrap;
  }
  .etab-crumb-status > * + * { margin-left: 6px; }
  .etab-crumb-status > * + *::before {
    content: '·'; margin-right: 6px; color: var(--text-mute); opacity: 0.6;
  }
  .etab-crumb-mod { color: var(--warn); }
  .etab-crumb-loc { color: var(--text-2); }
  .etab-crumb-pop {
    position: absolute; top: calc(100% + 6px); left: 0; z-index: 200;
    min-width: 240px; max-width: 420px; max-height: 340px; overflow-y: auto;
    padding: 4px;
    background: var(--bg-1); border: 1px solid var(--border-hi);
    border-radius: 10px; box-shadow: var(--shadow-3);
    display: flex; flex-direction: column; gap: 1px;
  }
  .etab-crumb-item {
    display: flex; align-items: center; gap: 8px;
    padding: 7px 10px; border-radius: 7px;
    background: transparent; border: 0; cursor: pointer; text-align: left;
    color: var(--text-1);
  }
  .etab-crumb-item:hover { background: var(--bg-hover); color: var(--text-0); }
  .etab-crumb-item.active { background: var(--bg-3); color: var(--text-0); }
  .etab-crumb-item-name {
    flex: 1; min-width: 0; font-size: 12px;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .etab-crumb-empty { padding: 8px 10px; font-size: 12px; color: var(--text-mute); }
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
