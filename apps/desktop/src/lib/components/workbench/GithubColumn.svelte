<script lang="ts">
  import { slide } from 'svelte/transition';
  import { cubicOut } from 'svelte/easing';
  import Dropdown, { type DropdownOption } from '$lib/components/ui/Dropdown.svelte';
  import {
    connectionsMeta,
    externalId,
    groupByTime,
    kindLabel,
    relativeTime,
    repoLabel,
    stateTag,
    type CommitEntry,
    type ConnectionStatus,
    type InboxItem
  } from '$lib/data';
  const ghMeta = connectionsMeta.find((c) => c.id === 'github')!;
  import {
    layoutState,
    startResizeById,
    activeInstances
  } from '$lib/state/layout.svelte';
  import ColumnControls from '$lib/components/workbench/ColumnControls.svelte';
  import {
    inboxState,
    loadGithubRepoOptions,
    updateGithubFilters,
    type GithubFilterMode
  } from '$lib/state/inbox.svelte';

  type DetailTab = 'conversation' | 'commits' | 'files' | 'reviews' | 'checks';

  interface Props {
    instanceId: string;
    githubStatus: ConnectionStatus;
    now: number;
    tab: DetailTab;
    actionBusy: string | null;
    // Callbacks from the inbox list
    onSelectInboxItem: (id: number) => void;
    onRefreshInbox: () => void;
    onOpenPalette: () => void;
    onDragStart: (payload: { source: 'github'; item: InboxItem }, e: DragEvent) => void;
    onDragEnd: () => void;
    onCardMouseDown: (e: MouseEvent) => void;
    isClickNotDrag: (e: MouseEvent) => boolean;
    // Callbacks from the focus / slide-over
    onTabChange: (tab: DetailTab) => void;
    onToggleFile: (filename: string) => void;
    onRetryLoadDetail: () => void;
    onOpenCommit: (c: CommitEntry) => void;
    onOpenComment: () => void;
    onOpenReview: () => void;
    onOpenMerge: () => void;
    onAskClose: () => void;
    onReopen: () => void;
    onOpenBrowser: (url: string) => void;
    onOpenCheckDetails: (url: string) => void;
    onCloseFocus: () => void;
    mergeDisabled: () => boolean;
    onOpenCreatePr: () => void;
  }

  let {
    instanceId,
    githubStatus,
    now,
    tab,
    actionBusy,
    onSelectInboxItem,
    onRefreshInbox,
    onOpenPalette,
    onDragStart,
    onDragEnd,
    onCardMouseDown,
    isClickNotDrag,
    onTabChange,
    onToggleFile,
    onRetryLoadDetail,
    onOpenCommit,
    onOpenComment,
    onOpenReview,
    onOpenMerge,
    onAskClose,
    onReopen,
    onOpenBrowser,
    onOpenCheckDetails,
    onCloseFocus,
    mergeDisabled,
    onOpenCreatePr
  }: Props = $props();

  const grouped = $derived(groupByTime(inboxState.items, now));

  // Filter mode options — label + the corresponding GitHub search qualifier
  // key (mirrored in `buildGithubQuery`).
  const FILTER_MODES: { value: GithubFilterMode; label: string }[] = [
    { value: 'involving', label: 'Involving me' },
    { value: 'authored', label: 'Authored by me' },
    { value: 'review_requested', label: 'Review requested' },
    { value: 'assigned', label: 'Assigned to me' },
    { value: 'user', label: 'Custom user…' },
    { value: 'all', label: 'All in repo' }
  ];

  // Lazily populate the repo dropdown the first time the panel opens — avoids
  // a blocking `github_list_repos` call on every column mount.
  function onRepoOpen() {
    if (!inboxState.githubRepoOptions.length) void loadGithubRepoOptions();
  }

  function onModeChange(value: GithubFilterMode) {
    updateGithubFilters({ mode: value });
  }

  function onRepoChange(value: string) {
    updateGithubFilters({ repo: value ? value : null });
  }

  function onSearchInput(e: Event) {
    const value = (e.target as HTMLInputElement).value;
    updateGithubFilters({ search: value });
  }

  function onCustomUserInput(e: Event) {
    const value = (e.target as HTMLInputElement).value;
    updateGithubFilters({ customUser: value });
  }

  const modeOptions = $derived<DropdownOption<GithubFilterMode>[]>(
    FILTER_MODES.map((m) => ({ value: m.value, label: m.label }))
  );
  const repoOptions = $derived<DropdownOption<string>[]>([
    { value: '', label: 'All repos' },
    ...inboxState.githubRepoOptions.map((r) => ({
      value: r.full_name,
      label: r.full_name
    }))
  ]);

  const inst = $derived(activeInstances().find((i) => i.id === instanceId));
  const order = $derived(activeInstances().findIndex((i) => i.id === instanceId));
</script>

<section
  class="wb-column inbox"
  data-instance-id={instanceId}
  data-kind="github"
  transition:slide={{ duration: 240, axis: 'x', easing: cubicOut }}
  style="order: {order}; flex: 0 0 {inst?.width ?? 420}px"
>
  <ColumnControls {instanceId} kind="github" />
  <div class="wb-col-resize" class:snap-flash={layoutState.snapFlashInstanceId === instanceId} role="separator" aria-orientation="vertical" onpointerdown={(e) => startResizeById(instanceId, e)}></div>
  <div class="inbox-brand">
    <span class="brand-icon {ghMeta.iconClass}" class:conn-icon--svg={!!ghMeta.iconSvg}>
      {#if ghMeta.iconSvg}
        <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">{@html ghMeta.iconSvg}</svg>
      {:else}
        {ghMeta.iconLetters}
      {/if}
    </span>
    <span class="brand-word">GitHub</span>
    {#if inst?.name}<span class="bench-name mono" title="Bench id">{inst.name}</span>{/if}
    {#if githubStatus.kind === 'connected'}
      <span class="brand-sub mono">@{githubStatus.user.login}</span>
    {/if}
    <button
      class="new-pr-btn"
      onclick={onOpenCreatePr}
      title="Create a new pull request"
      aria-label="New PR"
      disabled={githubStatus.kind !== 'connected'}
    >
      <svg class="i i-sm" viewBox="0 0 24 24"><path d="M12 5v14M5 12h14" /></svg>
      <span>New PR</span>
    </button>
  </div>
  <div class="inbox-header">
    <button class="search-bar" onclick={onOpenPalette}>
      <svg class="i i-sm" viewBox="0 0 24 24"><circle cx="11" cy="11" r="7" /><path d="m20 20-3-3" /></svg>
      <span>Search or jump to...</span>
      <span class="search-bar-hint"><kbd>⌘</kbd><kbd>K</kbd></span>
    </button>
    <div class="filter-bar">
      <div class="filter-row">
        <div class="filter-cell">
          <Dropdown
            value={inboxState.githubFilters.mode}
            options={modeOptions}
            onChange={onModeChange}
            ariaLabel="Filter mode"
            width="100%"
          />
        </div>
        <div class="filter-cell">
          <Dropdown
            value={inboxState.githubFilters.repo ?? ''}
            options={repoOptions}
            onChange={onRepoChange}
            onOpen={onRepoOpen}
            ariaLabel="Repo scope"
            placeholder={inboxState.githubRepoOptionsLoading ? 'Loading…' : 'All repos'}
            width="100%"
          />
        </div>
        <button class="icon-btn" onclick={onRefreshInbox} title="Refresh" aria-label="Refresh" disabled={inboxState.loading}>
          <svg class="i i-sm" viewBox="0 0 24 24" style="transform: rotate({inboxState.loading ? 360 : 0}deg); transition: transform 0.6s;">
            <path d="M21 12a9 9 0 0 1-9 9 9 9 0 0 1-8.5-6" />
            <path d="M3 12a9 9 0 0 1 9-9 9 9 0 0 1 8.5 6" />
            <polyline points="21 3 21 9 15 9" />
            <polyline points="3 21 3 15 9 15" />
          </svg>
        </button>
      </div>
      <div class="filter-row">
        <input
          class="filter-input"
          type="text"
          placeholder="Search…"
          value={inboxState.githubFilters.search}
          oninput={onSearchInput}
          aria-label="Search GitHub issues"
        />
      </div>
      {#if inboxState.githubFilters.mode === 'user'}
        <div class="filter-row">
          <input
            class="filter-input"
            type="text"
            placeholder="GitHub username (e.g. octocat)"
            value={inboxState.githubFilters.customUser}
            oninput={onCustomUserInput}
            aria-label="Custom user login"
          />
        </div>
      {/if}
    </div>
    <div class="inbox-controls">
      <span class="inbox-count mono">{inboxState.items.length} items</span>
    </div>
  </div>
  <div class="inbox-list">
    {#if inboxState.loading && inboxState.items.length === 0}
      <div class="inbox-state">Loading inbox…</div>
    {:else if inboxState.error}
      <div class="inbox-state inbox-state--error">
        {inboxState.error}
        <button class="link-inline" onclick={onRefreshInbox}>Retry</button>
      </div>
    {:else if inboxState.items.length === 0}
      <div class="inbox-state">No open items involving you.</div>
    {:else}
      {#each [['today', grouped.today, 'Today'], ['yesterday', grouped.yesterday, 'Yesterday'], ['earlier', grouped.earlier, 'Earlier']] as [key, list, label] (key)}
        {@const items = list as InboxItem[]}
        {#if items.length}
          <div class="inbox-group-label">{label}</div>
          {#each items as item (item.id)}
            {@const stag = stateTag(item)}
            <div
              class="inbox-item"
              class:active={item.id === inboxState.focusItem?.id}
              draggable="true"
              role="button"
              tabindex="0"
              ondragstart={(e) => onDragStart({ source: 'github', item }, e)}
              ondragend={onDragEnd}
              onmousedown={onCardMouseDown}
              onclick={(e) => { if (isClickNotDrag(e)) onSelectInboxItem(item.id); }}
              onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); onSelectInboxItem(item.id); } }}
            >
              <div class="inbox-item-top">
                <span class="source-mark">GH</span>
                <span class="inbox-item-id mono">{externalId(item)}</span>
                <span class="inbox-item-time mono">{relativeTime(item.updated_at, now)}</span>
              </div>
              <div class="inbox-item-title">{item.title}</div>
              <div class="inbox-item-meta">
                <span class="mini-tag {stag.className}">{stag.text}</span>
                <span class="mini-kind">{kindLabel(item)}</span>
                {#if item.repo}<span class="mini-repo mono">· {repoLabel(item)}</span>{/if}
              </div>
            </div>
          {/each}
        {/if}
      {/each}
    {/if}
  </div>
</section>

<!-- GithubFocusOverlay is mounted globally at the page root (in
     `+page.svelte`) so it can render regardless of which view / column
     is on screen. -->

<style>
  /* GitHub inbox column. Uses generic .wb-column / .wb-col-controls /
     .wb-col-resize rules defined in +page.svelte (shared across all
     columns). Focus-pane / slide-over styles live in GithubFocusOverlay. */

  .inbox { display: flex; flex-direction: column; min-height: 0; }

  .inbox-brand {
    padding: 16px 20px 10px; display: flex; align-items: center; gap: 10px;
    height: 54px;
  }
  .brand-word { font-size: 14px; font-weight: 600; color: var(--text-0); letter-spacing: -0.01em; }
  .brand-sub { font-size: 11.5px; color: var(--text-2); margin-left: auto; }
  .new-pr-btn {
    display: inline-flex; align-items: center; gap: 4px;
    padding: 4px 8px; border-radius: 6px;
    background: var(--bg-1);
    border: 1px solid var(--border-neutral-hi);
    color: var(--text-1);
    font-size: 11.5px; font-weight: 500;
    transition: all 120ms;
    cursor: pointer;
  }
  .new-pr-btn:hover:not(:disabled) { background: var(--bg-2); color: var(--text-0); border-color: var(--border-hi); }
  .new-pr-btn:disabled { opacity: 0.5; cursor: not-allowed; }
  .inbox-header { padding: 14px 20px 8px; }
  .inbox-controls { margin-top: 10px; display: flex; align-items: center; justify-content: space-between; padding: 0 4px; }
  .inbox-count { font-size: 11px; color: var(--text-mute); }

  .filter-bar { margin-top: 10px; display: flex; flex-direction: column; gap: 6px; }
  .filter-row { display: flex; align-items: center; gap: 6px; }
  .filter-cell { flex: 1 1 auto; min-width: 0; }
  .filter-input {
    flex: 1 1 auto; min-width: 0;
    padding: 6px 8px;
    background: var(--bg-1);
    border: 1px solid var(--border-neutral);
    border-radius: 6px;
    color: var(--text-1);
    font-size: 12px;
    font-family: inherit;
    line-height: 1.2;
  }
  .filter-input:hover { border-color: var(--border-neutral-hi); }
  .filter-input:focus { outline: 1px solid var(--accent); outline-offset: 0; border-color: var(--accent); }

  .inbox-list {
    flex: 1; overflow-y: auto; padding: 8px 12px 20px;
    display: flex; flex-direction: column; gap: 8px;
  }
  .inbox-state { padding: 40px 16px; text-align: center; font-size: 12.5px; color: var(--text-2); }
  .inbox-state--error { color: #fca5a5; }

  .inbox-group-label {
    display: flex; align-items: center; gap: 10px;
    font-size: 10.5px; font-weight: 600; color: var(--text-mute);
    text-transform: uppercase; letter-spacing: 0.08em;
    margin: 8px 4px 0;
  }
  .inbox-group-label::after { content: ''; flex: 1; height: 1px; background: var(--border-neutral); }

  .inbox-item {
    padding: 10px 12px;
    border-radius: 8px;
    background: var(--bg-1); border: 1px solid var(--border-neutral);
    cursor: pointer;
    transition: all 120ms;
    display: flex; flex-direction: column; gap: 5px;
  }
  .inbox-item:hover { background: var(--bg-2); border-color: var(--border-neutral-hi); }
  .inbox-item.active { background: var(--bg-2); border-color: var(--accent); box-shadow: 0 0 0 1px var(--accent-soft); }
  .inbox-item:active { cursor: grabbing; transform: scale(0.99); }
  .inbox-item:focus-visible { outline: 2px solid var(--accent); outline-offset: -2px; }
  .inbox-item-top { display: flex; align-items: center; gap: 8px; margin-bottom: 4px; }

  .source-mark {
    width: 22px; height: 22px; border-radius: 5px;
    display: inline-flex; align-items: center; justify-content: center;
    font-size: 10.5px; font-weight: 700; letter-spacing: -0.02em;
    background: var(--bg-2); color: var(--text-1);
    border: 1px solid var(--border-neutral-hi);
  }
  .inbox-item-id { font-size: 11px; color: var(--text-2); font-weight: 500; }
  .inbox-item-time { margin-left: auto; font-size: 10.5px; color: var(--text-mute); font-variant-numeric: tabular-nums; }
  .inbox-item-title {
    font-size: 13px; color: var(--text-0); font-weight: 500;
    line-height: 1.4; margin-bottom: 6px; word-break: break-word;
  }
  .inbox-item-meta { display: flex; align-items: center; gap: 6px; font-size: 11px; color: var(--text-2); flex-wrap: wrap; }

  .mini-tag { padding: 1px 6px; border-radius: 4px; font-size: 10px; font-weight: 600; text-transform: lowercase; }
  .mini-kind { color: var(--text-2); text-transform: lowercase; }
  .mini-repo { color: var(--text-mute); font-size: 10.5px; }
  :global(.tag--open)   { color: var(--accent-bright); background: var(--accent-soft); border: 1px solid rgba(16, 185, 129, 0.22); }
  :global(.tag--closed) { color: #8b96ab; background: rgba(139, 150, 171, 0.08); border: 1px solid rgba(139, 150, 171, 0.2); }
  :global(.tag--merged) { color: #b199f6; background: rgba(139, 92, 246, 0.08); border: 1px solid rgba(139, 92, 246, 0.24); }
  :global(.tag--draft)  { color: #fcd34d; background: rgba(245, 158, 11, 0.06); border: 1px solid rgba(245, 158, 11, 0.2); }
</style>
