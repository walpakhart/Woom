<script lang="ts">
  /* ActivityBar — узкая вертикальная полоса слева в EditorApp.
     Кнопки: Explorer / Search / Git / Debug / Tests / Inline Claude.
     На MVP большинство — переключатели side-panel'a (visual только).
     Bottom: Settings. */
  type Tab = 'explorer' | 'search' | 'git' | 'debug' | 'tests' | 'claude';

  interface Props {
    activeTab: Tab;
    onPick: (t: Tab) => void;
    /** Bottom-rail Settings shortcut — jumps the user out of the editor
     *  to the Settings view. Optional so the bar still works in
     *  preview / standalone embeddings. */
    onOpenSettings?: () => void;
    /** Number of unresolved problems — displayed as a badge on Tests. */
    problemsCount?: number;
    /** Linked agent count → claude badge. */
    claudeCount?: number;
    /** Git change count → git badge. */
    gitCount?: number;
  }
  let p: Props = $props();
</script>

<aside class="ab">
  <button class="ab-btn" class:active={p.activeTab === 'explorer'} onclick={() => p.onPick('explorer')} title="Explorer · ⇧⌘E">
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">
      <path d="M3 7v11a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2V9a2 2 0 0 0-2-2h-7L10 5H5a2 2 0 0 0-2 2z"/>
    </svg>
  </button>
  <button class="ab-btn" class:active={p.activeTab === 'search'} onclick={() => p.onPick('search')} title="Search · ⇧⌘F">
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"><circle cx="11" cy="11" r="7"/><path d="m20 20-3-3"/></svg>
  </button>
  <button class="ab-btn" class:active={p.activeTab === 'git'} onclick={() => p.onPick('git')} title="Source Control · ⌃⇧G">
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7"><circle cx="6" cy="6" r="2.5"/><circle cx="18" cy="18" r="2.5"/><path d="M6 8.5V14a4 4 0 0 0 4 4h6"/></svg>
    {#if p.gitCount && p.gitCount > 0}<span class="ab-badge">{p.gitCount}</span>{/if}
  </button>
  <button class="ab-btn" class:active={p.activeTab === 'debug'} onclick={() => p.onPick('debug')} title="Debug">
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7"><circle cx="12" cy="13" r="6"/><path d="M12 7v-3M9 4h6M5 11l-2 1M19 11l2 1M5 17l-2 1M19 17l2 1"/></svg>
  </button>
  <button class="ab-btn" class:active={p.activeTab === 'tests'} onclick={() => p.onPick('tests')} title="Tests">
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7"><polyline points="9 11 12 14 22 4"/><path d="M21 12v7a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11"/></svg>
    {#if p.problemsCount && p.problemsCount > 0}<span class="ab-badge ab-badge--err">{p.problemsCount}</span>{/if}
  </button>
  <button class="ab-btn" class:active={p.activeTab === 'claude'} onclick={() => p.onPick('claude')} title="Inline Claude · ⇧⌘L">
    <svg viewBox="0 0 24 24" fill="currentColor" stroke="none"><path d="M5 4h14a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2H8l-5 4V6a2 2 0 0 1 2-2z"/></svg>
    {#if p.claudeCount && p.claudeCount > 0}<span class="ab-badge ab-badge--claude">{p.claudeCount}</span>{/if}
  </button>
  <span class="ab-spacer"></span>
  <button
    class="ab-btn"
    title="Settings · ⌘,"
    onclick={() => p.onOpenSettings?.()}
    disabled={!p.onOpenSettings}
  >
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09A1.65 1.65 0 0 0 4.6 9 1.65 1.65 0 0 0 4.27 7.18l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06A1.65 1.65 0 0 0 9 4.6 1.65 1.65 0 0 0 10 3.09V3a2 2 0 1 1 4 0v.09c0 .68.4 1.29 1 1.51a1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9c.22.6.83 1 1.51 1H21a2 2 0 1 1 0 4h-.09c-.68 0-1.29.4-1.51 1z"/></svg>
  </button>
</aside>

<style>
  .ab {
    width: 44px; flex: 0 0 44px;
    display: flex; flex-direction: column; align-items: center;
    gap: 4px;
    padding: 8px 0 10px;
    background: var(--bg-glass, rgba(34, 28, 23, 0.66));
    border-right: 1px solid var(--border);
    backdrop-filter: blur(14px);
    -webkit-backdrop-filter: blur(14px);
  }
  .ab-btn {
    position: relative;
    width: 32px; height: 32px;
    display: grid; place-items: center;
    border-radius: 7px;
    color: var(--text-2);
    background: transparent; border: none; cursor: pointer;
    transition: color 140ms, background 140ms;
  }
  .ab-btn:hover { color: var(--text-0); background: var(--bg-elev, var(--bg-2)); }
  .ab-btn.active {
    color: var(--accent-bright);
    background: var(--accent-soft);
    box-shadow: inset 0 0 0 1px var(--border-accent-2);
  }
  .ab-btn.active::before {
    content: '';
    position: absolute; left: -8px; top: 7px; bottom: 7px;
    width: 2.5px; border-radius: 2px;
    background: linear-gradient(180deg, var(--accent-bright), var(--accent-deep));
    box-shadow: 0 0 10px var(--accent-glow);
  }
  .ab-btn svg { width: 17px; height: 17px; stroke-linecap: round; stroke-linejoin: round; }
  .ab-badge {
    position: absolute; top: 1px; right: 1px;
    min-width: 14px; height: 14px; padding: 0 3px;
    border-radius: 7px;
    font-family: 'JetBrains Mono', monospace;
    font-size: 9px; font-weight: 700;
    background: var(--accent); color: var(--accent-fg);
    display: grid; place-items: center;
    box-shadow: 0 0 0 2px var(--bg-1);
  }
  .ab-badge--err { background: var(--error); color: var(--bg-0); }
  .ab-badge--claude { background: var(--src-claude); color: var(--accent-fg); }
  .ab-spacer { flex: 1; }
</style>
