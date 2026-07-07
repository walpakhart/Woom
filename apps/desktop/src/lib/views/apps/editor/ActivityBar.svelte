<script lang="ts">
  /* ActivityBar — 44px vertical rail on the left of EditorApp (mockup 4i).
     Nav items: Explorer / Search / Git / Review / Debug / Tests.
     Bottom rail: Claude glyph (toggles AgentDock, ⌘L) + Settings (⌘,).
     git / review counts render as tiny corner digits (--text-1 / --warn). */
  type Tab = 'explorer' | 'search' | 'git' | 'review' | 'debug' | 'tests';

  interface Props {
    activeTab: Tab;
    onPick: (t: Tab) => void;
    /** Bottom-rail Settings shortcut — jumps out to the Settings view.
     *  Optional so the bar still works in preview / standalone. */
    onOpenSettings?: () => void;
    /** Unresolved problems → tiny corner digit on Tests. */
    problemsCount?: number;
    /** Git change count → tiny corner digit on Git. */
    gitCount?: number;
    /** Pending agent edits across linked sessions → corner digit on Review. */
    reviewCount?: number;
    /** Agent-dock open state — drives the Claude glyph's active tint. */
    dockOpen?: boolean;
    /** Toggle the editor↔agent dock (mirrors ⌘L). */
    onToggleDock?: () => void;
  }
  let p: Props = $props();
</script>

<aside class="eab">
  <nav class="eab-cluster">
    <button
      class="eab-item"
      class:eab-item--on={p.activeTab === 'explorer'}
      onclick={() => p.onPick('explorer')}
      title="Explorer · ⇧⌘E"
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">
        <path d="M6.5 3h6l4 4v13.2a.8.8 0 0 1-.8.8H6.5a.8.8 0 0 1-.8-.8V3.8A.8.8 0 0 1 6.5 3z"/>
        <path d="M12.5 3v4h4"/>
        <path d="M8.6 12h5.2M8.6 15h5.2M8.6 18h3.4"/>
      </svg>
    </button>

    <button
      class="eab-item"
      class:eab-item--on={p.activeTab === 'search'}
      onclick={() => p.onPick('search')}
      title="Search · ⇧⌘F"
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">
        <circle cx="10.5" cy="10.5" r="6"/>
        <path d="M15 15l4.5 4.5"/>
      </svg>
    </button>

    <button
      class="eab-item"
      class:eab-item--on={p.activeTab === 'git'}
      onclick={() => p.onPick('git')}
      title="Source Control · ⌃⇧G"
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">
        <path d="M3.5 12h5.3M15.2 12h5.3"/>
        <circle cx="12" cy="12" r="3.3"/>
      </svg>
      {#if p.gitCount && p.gitCount > 0}<span class="eab-count eab-count--git">{p.gitCount}</span>{/if}
    </button>

    <button
      class="eab-item"
      class:eab-item--on={p.activeTab === 'review'}
      onclick={() => p.onPick('review')}
      title="Review agent edits · ⇧⌘R"
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">
        <path d="M2.5 12s3.4-5.6 9.5-5.6S21.5 12 21.5 12 18.1 17.6 12 17.6 2.5 12 2.5 12z"/>
        <circle cx="12" cy="12" r="2.4"/>
      </svg>
      {#if p.reviewCount && p.reviewCount > 0}<span class="eab-count eab-count--review">{p.reviewCount}</span>{/if}
    </button>

    <button
      class="eab-item"
      class:eab-item--on={p.activeTab === 'debug'}
      onclick={() => p.onPick('debug')}
      title="Debug"
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">
        <path d="M12 8.5a4 4.6 0 0 1 4 4.6v.9a4 4.6 0 0 1-8 0v-.9a4 4.6 0 0 1 4-4.6z"/>
        <path d="M9.8 6l1.6 2.2M14.2 6l-1.6 2.2"/>
        <path d="M8 12.5H4.8M16 12.5h3.2M8 16l-2.4 1.4M16 16l2.4 1.4"/>
      </svg>
    </button>

    <button
      class="eab-item"
      class:eab-item--on={p.activeTab === 'tests'}
      onclick={() => p.onPick('tests')}
      title="Tests"
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">
        <path d="M9.5 3h5M10.5 3v6.2l-4.4 7.4a1.4 1.4 0 0 0 1.2 2.1h9.4a1.4 1.4 0 0 0 1.2-2.1L13.5 9.2V3"/>
        <path d="M8.4 14.5h7.2"/>
      </svg>
      {#if p.problemsCount && p.problemsCount > 0}<span class="eab-count eab-count--problems">{p.problemsCount}</span>{/if}
    </button>
  </nav>

  <span class="eab-fill"></span>

  <nav class="eab-cluster">
    {#if p.onToggleDock}
      <button
        class="eab-item eab-item--claude"
        class:eab-item--on={p.dockOpen}
        onclick={() => p.onToggleDock?.()}
        title="Agent dock · ⌘L"
        aria-label="Toggle agent dock"
        aria-pressed={p.dockOpen}
      >
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">
          <path d="M12 3.5v17M3.5 12h17M6 6l12 12M18 6L6 18"/>
        </svg>
      </button>
    {/if}

    <button
      class="eab-item"
      title="Settings · ⌘,"
      onclick={() => p.onOpenSettings?.()}
      disabled={!p.onOpenSettings}
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">
        <path d="M4 8h6M14 8h6"/>
        <circle cx="12" cy="8" r="2.2"/>
        <path d="M4 16h10M18 16h2"/>
        <circle cx="16" cy="16" r="2.2"/>
      </svg>
    </button>
  </nav>
</aside>

<style>
  /* Fill the host pane (.se-activity is a flex COLUMN), so stretch to it
     rather than setting a HEIGHT basis that would squash the rail. */
  .eab {
    width: 100%; flex: 1;
    display: flex; flex-direction: column; align-items: center;
    gap: 4px;
    padding: 8px 0 10px;
    background: var(--bg-0);
  }
  .eab-cluster {
    display: flex; flex-direction: column; align-items: center;
    gap: 4px;
  }
  .eab-fill { flex: 1; }

  .eab-item {
    position: relative;
    width: 32px; height: 32px;
    display: grid; place-items: center;
    border-radius: 8px;
    color: var(--text-2);
    background: transparent; border: none; cursor: pointer;
    transition: color 140ms var(--ease-out), background 140ms var(--ease-out);
  }
  .eab-item:hover { color: var(--text-0); background: var(--bg-2); }
  .eab-item--on {
    color: var(--text-0);
    background: var(--bg-3);
  }
  .eab-item:disabled { opacity: 0.4; cursor: default; }
  .eab-item svg { width: 18px; height: 18px; }

  /* Tiny corner digits — no filled pill, just a colored numeral with a
     thin ring in the rail bg so it stays legible over the icon. */
  .eab-count {
    position: absolute; top: 0; right: 1px;
    font-family: var(--font-mono);
    font-size: 8px; font-weight: 700; line-height: 1;
    letter-spacing: -0.02em;
    text-shadow:
      0 0 2px var(--bg-0), 0 0 2px var(--bg-0);
  }
  .eab-count--git { color: var(--text-1); }
  .eab-count--review { color: var(--warn); }
  .eab-count--problems { color: var(--warn); }
</style>
