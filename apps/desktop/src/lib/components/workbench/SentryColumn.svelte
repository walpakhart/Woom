<script lang="ts">
  import { slide } from 'svelte/transition';
  import { cubicOut } from 'svelte/easing';
  import {
    connectionsMeta,
    relativeTime,
    sentryLevelClass,
    type SentryIssue,
    type SentryStatus
  } from '$lib/data';
  const sentryMeta = connectionsMeta.find((c) => c.id === 'sentry')!;
  import { layoutState, startResizeById, activeInstances } from '$lib/state/layout.svelte';
  import {
    inboxState,
    loadSentryEnvironments,
    loadSentryProjects,
    openSentryFocus,
    refreshSentryInbox,
    scheduleSentryFilterRefresh
  } from '$lib/state/inbox.svelte';
  import ColumnControls from '$lib/components/workbench/ColumnControls.svelte';
  import Dropdown, { type DropdownOption } from '$lib/components/ui/Dropdown.svelte';

  type DragSource = { source: 'sentry'; item: SentryIssue };

  interface Props {
    instanceId: string;
    sentryStatus: SentryStatus;
    now: number;
    onDragStart: (payload: DragSource, e: DragEvent) => void;
    onDragEnd: () => void;
    onCardMouseDown: (e: MouseEvent) => void;
    isClickNotDrag: (e: MouseEvent) => boolean;
    /** Open issue's permalink in the user's default browser. Sentry's
     *  detail UI is rich and live; better than re-implementing it. */
    onOpenBrowser: (url: string) => void;
  }
  let {
    instanceId,
    sentryStatus,
    now,
    onDragStart,
    onDragEnd,
    onCardMouseDown,
    isClickNotDrag,
    onOpenBrowser
  }: Props = $props();

  const inst = $derived(activeInstances().find((i) => i.id === instanceId));
  const order = $derived(activeInstances().findIndex((i) => i.id === instanceId));

  // Auto-load on mount when connected. Subsequent refreshes are user-driven
  // via the refresh button in the column header; no polling for now.
  $effect(() => {
    if (sentryStatus.kind !== 'connected') return;
    if (
      inboxState.sentryItems.length === 0 &&
      !inboxState.sentryItemsLoading &&
      !inboxState.sentryItemsError
    ) {
      void refreshSentryInbox({ silent: false });
    }
  });

  // Lazy-load project + env dropdowns the first time they're shown.
  $effect(() => {
    if (sentryStatus.kind !== 'connected') return;
    if (
      inboxState.sentryProjectOptions.length === 0 &&
      !inboxState.sentryProjectOptionsLoading
    ) {
      void loadSentryProjects().then(() => loadSentryEnvironments());
    }
  });

  // Filter dropdown shapes — pre-built so the template stays readable.
  const projectOpts = $derived<DropdownOption<string>[]>([
    { value: '__all__', label: 'All projects' },
    ...inboxState.sentryProjectOptions.map((p) => ({
      value: p.slug,
      label: p.name || p.slug,
      hint: p.platform ?? undefined
    }))
  ]);
  const envOpts = $derived<DropdownOption<string>[]>([
    { value: '', label: 'All environments' },
    ...inboxState.sentryEnvironmentOptions.map((e) => ({
      value: e.name,
      label: e.name
    }))
  ]);
  const statusOpts: DropdownOption<string>[] = [
    { value: 'unresolved', label: 'Unresolved' },
    { value: 'resolved', label: 'Resolved' },
    { value: 'ignored', label: 'Ignored' },
    { value: 'all', label: 'Any status' }
  ];
  const levelOpts: DropdownOption<string>[] = [
    { value: 'all', label: 'Any level' },
    { value: 'fatal', label: 'Fatal' },
    { value: 'error', label: 'Error' },
    { value: 'warning', label: 'Warning' },
    { value: 'info', label: 'Info' },
    { value: 'debug', label: 'Debug' }
  ];
  const sortOpts: DropdownOption<string>[] = [
    { value: 'date', label: 'Last seen' },
    { value: 'new', label: 'First seen' },
    { value: 'priority', label: 'Priority' },
    { value: 'freq', label: 'Events count' },
    { value: 'user', label: 'Users affected' }
  ];

  // Selection helpers — wrap mutations + debounced refresh so filter
  // changes feel instant but we don't spam the API on every keystroke.
  function pickProject(slug: string) {
    inboxState.sentryProjects = slug === '__all__' ? [] : [slug];
    void loadSentryEnvironments();
    void refreshSentryInbox({ silent: true });
  }
  function pickEnvironment(name: string) {
    inboxState.sentryEnvironment = name || null;
    void refreshSentryInbox({ silent: true });
  }
  function pickStatus(s: string) {
    inboxState.sentryStatus = s as typeof inboxState.sentryStatus;
    void refreshSentryInbox({ silent: true });
  }
  function pickLevel(l: string) {
    inboxState.sentryLevel = l as typeof inboxState.sentryLevel;
    void refreshSentryInbox({ silent: true });
  }
  function pickSort(s: string) {
    inboxState.sentrySort = s as typeof inboxState.sentrySort;
    void refreshSentryInbox({ silent: true });
  }
  function onSearchInput(e: Event) {
    inboxState.sentrySearch = (e.target as HTMLInputElement).value;
    scheduleSentryFilterRefresh();
  }

  const projectValue = $derived(inboxState.sentryProjects[0] ?? '__all__');

  // Truncate long error titles for the card. Two-line clamp via CSS,
  // but we still hard-truncate the meta line.
  function shortText(s: string | null | undefined, max = 90): string {
    if (!s) return '';
    return s.length <= max ? s : `${s.slice(0, max - 1)}…`;
  }
</script>

<section
  class="wb-column sentry-col"
  data-instance-id={instanceId}
  data-kind="sentry"
  style="order: {order}; flex: 0 0 {inst?.width ?? 440}px"
  transition:slide={{ duration: 240, axis: 'x', easing: cubicOut }}
>
  <ColumnControls {instanceId} kind="sentry" />
  <div class="wb-col-resize" class:snap-flash={layoutState.snapFlashInstanceId === instanceId} role="separator" aria-orientation="vertical" onpointerdown={(e) => startResizeById(instanceId, e)}></div>

  <div class="inbox-brand">
    <span class="source-mark conn-icon--sentry" aria-hidden="true">
      <svg viewBox="0 0 24 24" fill="currentColor">{@html sentryMeta.iconSvg}</svg>
    </span>
    <span class="brand-word">Sentry</span>
    {#if inst?.name}<span class="bench-name mono" title="Bench id">{inst.name}</span>{/if}
    <div style="flex: 1"></div>
    <button
      class="refresh-btn"
      onclick={() => void refreshSentryInbox({ silent: true })}
      disabled={inboxState.sentryItemsLoading}
      title="Refresh"
      aria-label="Refresh"
    >
      <svg class="i i-sm" viewBox="0 0 24 24" class:spin={inboxState.sentryItemsLoading}>
        <path d="M3 12a9 9 0 1 0 3-6.7M3 4v5h5"/>
      </svg>
    </button>
  </div>

  <div class="filter-bar">
    <div class="filter-row">
      <Dropdown
        value={projectValue}
        options={projectOpts}
        onChange={pickProject}
        ariaLabel="Project"
        placeholder={inboxState.sentryProjectOptionsLoading ? 'Loading…' : 'Project'}
        width="100%"
      />
      <Dropdown
        value={inboxState.sentryEnvironment ?? ''}
        options={envOpts}
        onChange={pickEnvironment}
        ariaLabel="Environment"
        placeholder="Environment"
        width="100%"
      />
    </div>
    <div class="filter-row three-col">
      <Dropdown
        value={inboxState.sentryStatus}
        options={statusOpts}
        onChange={pickStatus}
        ariaLabel="Status"
        width="100%"
      />
      <Dropdown
        value={inboxState.sentryLevel}
        options={levelOpts}
        onChange={pickLevel}
        ariaLabel="Level"
        width="100%"
      />
      <Dropdown
        value={inboxState.sentrySort}
        options={sortOpts}
        onChange={pickSort}
        ariaLabel="Sort"
        width="100%"
      />
    </div>
    <input
      class="filter-search mono"
      type="search"
      value={inboxState.sentrySearch}
      oninput={onSearchInput}
      onkeydown={(e) => { if (e.key === 'Enter') { void refreshSentryInbox({ silent: false }); } }}
      placeholder="search… (free text or `tag:value`)"
      aria-label="Sentry search query"
    />
  </div>

  <div class="inbox-list">
    {#if sentryStatus.kind !== 'connected'}
      <div class="inbox-state">Sentry isn't connected.</div>
    {:else if inboxState.sentryItemsLoading && inboxState.sentryItems.length === 0}
      <div class="inbox-state">Loading…</div>
    {:else if inboxState.sentryItemsError}
      <div class="inbox-state inbox-state--error">
        {inboxState.sentryItemsError}
        <button class="link-inline" onclick={() => void refreshSentryInbox({ silent: false })}>Retry</button>
      </div>
    {:else if inboxState.sentryItems.length === 0}
      <div class="inbox-state">No issues match the current query.</div>
    {:else}
      {#each inboxState.sentryItems as issue (issue.id)}
        <div
          class="inbox-item sentry-item"
          draggable="true"
          role="button"
          tabindex="0"
          ondragstart={(e) => onDragStart({ source: 'sentry', item: issue }, e)}
          ondragend={onDragEnd}
          onmousedown={onCardMouseDown}
          onclick={(e) => { if (isClickNotDrag(e)) openSentryFocus(issue.id); }}
          onkeydown={(e) => { if (e.key === 'Enter') openSentryFocus(issue.id); }}
        >
          <div class="inbox-item-top">
            <span class="source-mark sentry-mark" aria-hidden="true">St</span>
            <span class="inbox-item-id mono">{issue.short_id || issue.id}</span>
            <span class="mini-tag {sentryLevelClass(issue.level)}">{issue.level}</span>
            <span class="inbox-item-time mono">{relativeTime(issue.last_seen, now)}</span>
          </div>
          <div class="inbox-item-title">{shortText(issue.title, 120)}</div>
          {#if issue.metadata_value}
            <div class="inbox-item-sub mono">{shortText(issue.metadata_value, 110)}</div>
          {/if}
          <div class="inbox-item-meta">
            <span class="mini-kind">{issue.project_slug}</span>
            <span class="mini-repo">· {issue.count} event{issue.count === '1' ? '' : 's'}</span>
            {#if issue.user_count > 0}
              <span class="mini-repo">· {issue.user_count} user{issue.user_count === 1 ? '' : 's'}</span>
            {/if}
            {#if issue.status !== 'unresolved'}
              <span class="mini-repo">· {issue.status}</span>
            {/if}
          </div>
        </div>
      {/each}
    {/if}
  </div>
</section>

<style>
  .sentry-col {
    flex: 1 1 420px;
    min-width: 360px;
    display: flex; flex-direction: column;
    background: rgba(16, 24, 40, 0.3);
  }
  .inbox-brand {
    padding: 16px 20px 10px; display: flex; align-items: center; gap: 10px;
  }
  .brand-word { font-size: 14px; font-weight: 600; color: var(--text-0); letter-spacing: -0.01em; }
  .bench-name { font-size: 11px; color: var(--text-2); }
  .source-mark {
    width: 22px; height: 22px; border-radius: 5px;
    display: inline-flex; align-items: center; justify-content: center;
    font-size: 10.5px; font-weight: 700;
    background: var(--bg-2); color: var(--text-1);
    border: 1px solid var(--border-neutral-hi);
  }
  .source-mark svg { width: 12px; height: 12px; color: currentColor; }
  .sentry-mark { color: #f88f74; background: rgba(248, 143, 116, 0.12); border-color: rgba(248, 143, 116, 0.3); }
  .refresh-btn {
    width: 26px; height: 26px; border-radius: 6px;
    display: inline-flex; align-items: center; justify-content: center;
    color: var(--text-2); background: transparent; cursor: pointer;
  }
  .refresh-btn:hover { background: var(--bg-2); color: var(--text-0); }
  .refresh-btn .spin { animation: spin 0.8s linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }

  .filter-bar {
    padding: 10px 16px; border-bottom: 1px solid var(--border-neutral);
    display: flex; flex-direction: column; gap: 6px;
  }
  .filter-row { display: grid; grid-template-columns: 1fr 1fr; gap: 6px; }
  .filter-row.three-col { grid-template-columns: 1fr 1fr 1fr; }
  .filter-search {
    width: 100%; padding: 7px 10px;
    background: var(--bg-1); border: 1px solid var(--border-neutral);
    border-radius: 6px; color: var(--text-0); font-size: 12px;
  }
  .filter-search:focus { outline: none; border-color: var(--accent); }

  .inbox-list { flex: 1; overflow-y: auto; padding: 8px 12px 18px; display: flex; flex-direction: column; gap: 8px; }
  .inbox-state { padding: 14px 16px; font-size: 12px; color: var(--text-2); text-align: center; }
  .inbox-state--error { color: var(--error); }
  .link-inline { color: var(--accent-bright); margin-left: 6px; cursor: pointer; background: none; border: none; padding: 0; text-decoration: underline; }

  .inbox-item {
    background: var(--bg-1); border: 1px solid var(--border-neutral);
    border-radius: 8px; padding: 10px 12px;
    display: flex; flex-direction: column; gap: 5px;
    cursor: grab; transition: all 120ms;
  }
  .inbox-item:hover { border-color: var(--border-neutral-hi); background: var(--bg-2); }
  .inbox-item:active { cursor: grabbing; }
  .inbox-item-top {
    display: flex; align-items: center; gap: 8px;
    font-size: 11px;
  }
  .inbox-item-id { color: var(--text-1); font-weight: 600; }
  .inbox-item-time { margin-left: auto; color: var(--text-mute); font-size: 10.5px; }
  .inbox-item-title {
    font-size: 13px; color: var(--text-0); line-height: 1.4;
    display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical;
    overflow: hidden;
  }
  .inbox-item-sub {
    font-size: 11px; color: var(--text-2);
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .inbox-item-meta {
    display: flex; align-items: center; gap: 6px;
    font-size: 10.5px; color: var(--text-mute);
  }
  .mini-tag {
    padding: 1px 6px; border-radius: 3px; font-weight: 600;
    text-transform: uppercase; font-size: 9.5px; letter-spacing: 0.04em;
  }
  .mini-kind { font-size: 10.5px; color: var(--text-1); }
  .mini-repo { color: var(--text-mute); }
</style>
