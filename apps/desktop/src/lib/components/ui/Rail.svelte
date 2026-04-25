<script lang="ts">
  import Sigil from '$lib/components/ui/Sigil.svelte';
  import type { ConnectionStatus } from '$lib/data';

  type View = 'workbench' | 'repositories' | 'tasks' | 'issues' | 'rules' | 'connections' | 'settings';

  interface Props {
    view: View;
    inboxCount: number;
    anythingConnected: boolean;
    statusLoading: boolean;
    githubStatus: ConnectionStatus;
  }

  let {
    view = $bindable(),
    inboxCount,
    anythingConnected,
    statusLoading,
    githubStatus
  }: Props = $props();
</script>

<aside class="rail">
  <div class="rail-top">
    <div class="rail-sigil"><Sigil size={36} /></div>
    <button class="rail-btn" class:active={view === 'workbench'} data-tooltip="Workbench" onclick={() => (view = 'workbench')} aria-label="Workbench">
      <svg class="i" viewBox="0 0 24 24"><path d="M4 6h16M4 12h10M4 18h16" /></svg>
      {#if inboxCount > 0}<span class="rail-badge">{inboxCount}</span>{/if}
    </button>
    <button class="rail-btn" class:active={view === 'repositories'} data-tooltip="Repositories" onclick={() => (view = 'repositories')} aria-label="Repositories">
      <svg class="i" viewBox="0 0 24 24"><path d="M3 7v11a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2V9a2 2 0 0 0-2-2h-7L10 5H5a2 2 0 0 0-2 2z" /></svg>
    </button>
    <button class="rail-btn" class:active={view === 'tasks'} data-tooltip="Tasks" onclick={() => (view = 'tasks')} aria-label="Tasks">
      <svg class="i" viewBox="0 0 24 24"><rect x="4" y="3" width="16" height="18" rx="2" /><path d="M9 3v2h6V3" /><path d="M8 11h8M8 15h6" /></svg>
    </button>
    <button class="rail-btn" class:active={view === 'issues'} data-tooltip="Issues (Sentry)" onclick={() => (view = 'issues')} aria-label="Issues">
      <svg class="i" viewBox="0 0 24 24"><circle cx="12" cy="12" r="9"/><path d="M12 8v4M12 16h.01"/></svg>
    </button>
    <button class="rail-btn" class:active={view === 'rules'} data-tooltip="Rules" onclick={() => (view = 'rules')} aria-label="Rules">
      <svg class="i" viewBox="0 0 24 24"><path d="M14 3v4a1 1 0 0 0 1 1h4M17 21H7a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h7l5 5v11a2 2 0 0 1-2 2z" /><path d="M9 12h6M9 16h6" /></svg>
    </button>
    <div class="rail-sep"></div>
    <button class="rail-btn" class:active={view === 'connections'} data-tooltip="Connections" onclick={() => (view = 'connections')} aria-label="Connections">
      <svg class="i" viewBox="0 0 24 24"><path d="M10 13a5 5 0 0 0 7 0l3-3a5 5 0 0 0-7-7l-1 1" /><path d="M14 11a5 5 0 0 0-7 0l-3 3a5 5 0 0 0 7 7l1-1" /></svg>
      {#if !anythingConnected && !statusLoading}<span class="rail-dot"></span>{/if}
    </button>
  </div>
  <div class="rail-bottom">
    <button class="rail-btn" class:active={view === 'settings'} data-tooltip="Settings" onclick={() => (view = 'settings')} aria-label="Settings">
      <svg class="i" viewBox="0 0 24 24"><circle cx="12" cy="12" r="3" /><path d="M12 1v4M12 19v4M4.22 4.22l2.83 2.83M16.95 16.95l2.83 2.83M1 12h4M19 12h4M4.22 19.78l2.83-2.83M16.95 7.05l2.83-2.83" /></svg>
    </button>
    <div class="rail-avatar">
      {#if githubStatus.kind === 'connected'}
        <img src={githubStatus.user.avatar_url} alt={githubStatus.user.login} />
      {:else}—{/if}
    </div>
  </div>
</aside>

<style>
  .rail {
    display: flex; flex-direction: column; align-items: center;
    padding: 14px 0; gap: 6px;
    border-right: 1px solid var(--border-neutral);
    background: rgba(10, 17, 30, 0.6);
    backdrop-filter: blur(8px);
    /* Guarantee the rail renders above workbench tabs / pill hover menus
       so its badges and active-view indicator stay readable even when a
       neighbouring panel has its own stacking context. */
    position: relative;
    z-index: 5;
  }
  .rail-top, .rail-bottom { display: flex; flex-direction: column; align-items: center; gap: 6px; }
  .rail-bottom { margin-top: auto; }
  .rail-sigil { margin-bottom: 14px; }
  .rail-btn {
    width: 38px; height: 38px; border-radius: 9px;
    display: inline-flex; align-items: center; justify-content: center;
    color: var(--text-2); position: relative; transition: all 140ms ease;
    background: none; border: none; cursor: pointer; padding: 0;
  }
  .rail-btn:hover { color: var(--text-0); background: var(--bg-1); }
  .rail-btn.active { color: var(--accent-bright); background: var(--bg-2); box-shadow: inset 0 0 0 1px var(--border-hi); }
  .rail-btn.active::before {
    content: ''; position: absolute; left: -14px; top: 10px; bottom: 10px; width: 2px;
    background: var(--accent); border-radius: 2px; box-shadow: 0 0 8px var(--accent-glow);
  }
  .rail-badge {
    position: absolute; top: 2px; right: 2px;
    min-width: 14px; height: 14px; padding: 0 3px; border-radius: 7px;
    background: var(--accent); color: #0a111e;
    font-size: 9.5px; font-weight: 700;
    display: inline-flex; align-items: center; justify-content: center;
    box-shadow: 0 0 0 2px var(--bg-0), 0 0 8px var(--accent-glow);
  }
  .rail-dot {
    position: absolute; top: 6px; right: 6px; width: 7px; height: 7px;
    border-radius: 50%; background: var(--warning);
    box-shadow: 0 0 0 2px var(--bg-0), 0 0 8px rgba(245, 158, 11, 0.5);
  }
  .rail-avatar {
    width: 30px; height: 30px; border-radius: 50%;
    background: linear-gradient(135deg, #3b82f6, #10b981);
    display: inline-flex; align-items: center; justify-content: center;
    color: #fff; font-weight: 600; font-size: 11px;
    box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.15); overflow: hidden;
  }
  .rail-avatar img { width: 100%; height: 100%; object-fit: cover; }
  .rail-sep { width: 20px; height: 1px; background: var(--border-neutral); margin: 4px 0; }
  .rail-btn[data-tooltip]:hover::after {
    content: attr(data-tooltip);
    position: absolute; left: 46px; top: 50%; transform: translateY(-50%);
    padding: 4px 10px; background: var(--bg-3); border: 1px solid var(--border-neutral-hi);
    border-radius: 6px; font-size: 11.5px; color: var(--text-0);
    white-space: nowrap; pointer-events: none; z-index: 10;
  }
</style>
