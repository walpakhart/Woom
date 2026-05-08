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
    scheduleSentryFilterRefresh,
    sentryFiltersFor,
    sentryItemsErrorFor,
    sentryItemsFor,
    sentryItemsLoadingFor,
    setSentryFilters
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

  /* Per-instance state. Two SentryColumn instances each get their
     own filter / item slot keyed by instanceId. */
  const filters = $derived(sentryFiltersFor(instanceId));
  const items = $derived(sentryItemsFor(instanceId));
  const itemsLoading = $derived(sentryItemsLoadingFor(instanceId));
  const itemsError = $derived(sentryItemsErrorFor(instanceId));

  /* Auto-load on first mount when connected and this column has no slot
     yet (empty results aren't undefined → won't loop forever). */
  $effect(() => {
    if (sentryStatus.kind !== 'connected') return;
    if (
      inboxState.sentryItemsByInstance[instanceId] === undefined &&
      !itemsLoading
    ) {
      void refreshSentryInbox(instanceId, { silent: false });
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

  /* Filter pick helpers — patch this column's filter slice via
     `setSentryFilters` and trigger a debounced + persisted refresh. */
  function pickProject(slug: string) {
    setSentryFilters(instanceId, { projects: slug === '__all__' ? [] : [slug] });
    void loadSentryEnvironments(slug === '__all__' ? undefined : slug);
    scheduleSentryFilterRefresh(instanceId);
  }
  function pickEnvironment(name: string) {
    setSentryFilters(instanceId, { environment: name || null });
    scheduleSentryFilterRefresh(instanceId);
  }
  function pickStatus(s: string) {
    setSentryFilters(instanceId, { status: s as SentryFiltersStatus });
    scheduleSentryFilterRefresh(instanceId);
  }
  function pickLevel(l: string) {
    setSentryFilters(instanceId, { level: l as SentryFiltersLevel });
    scheduleSentryFilterRefresh(instanceId);
  }
  function pickSort(s: string) {
    setSentryFilters(instanceId, { sort: s as SentryFiltersSort });
    scheduleSentryFilterRefresh(instanceId);
  }
  function onSearchInput(e: Event) {
    setSentryFilters(instanceId, { search: (e.target as HTMLInputElement).value });
    scheduleSentryFilterRefresh(instanceId);
  }

  type SentryFiltersStatus = 'unresolved' | 'resolved' | 'ignored' | 'all';
  type SentryFiltersLevel = 'all' | 'fatal' | 'error' | 'warning' | 'info' | 'debug';
  type SentryFiltersSort = 'date' | 'new' | 'priority' | 'freq' | 'user';

  const projectValue = $derived(filters.projects[0] ?? '__all__');

  // Truncate long error titles for the card. Two-line clamp via CSS,
  // but we still hard-truncate the meta line.
  function shortText(s: string | null | undefined, max = 90): string {
    if (!s) return '';
    return s.length <= max ? s : `${s.slice(0, max - 1)}…`;
  }
</script>

<section
  class="wb-column sentry-col"
  class:wb-column--maximized={layoutState.maximizedInstanceId === instanceId}
  data-instance-id={instanceId}
  data-kind="sentry"
  style="order: {order}; flex: 0 0 {inst?.width ?? 440}px"
  transition:slide={{ duration: 240, axis: 'x', easing: cubicOut }}
>
  <ColumnControls {instanceId} kind="sentry" />
  <div class="wb-col-resize" class:snap-flash={layoutState.snapFlashInstanceId === instanceId} role="separator" aria-orientation="vertical" onpointerdown={(e) => startResizeById(instanceId, e)}></div>

  <div class="inbox-brand">
    <span class="brand-icon conn-icon--sentry conn-icon--svg" aria-hidden="true">
      <svg viewBox="0 0 24 24" fill="currentColor">{@html sentryMeta.iconSvg}</svg>
    </span>
    <span class="brand-word">Sentry</span>
    {#if inst?.name}<span class="bench-name mono" title="Bench id">{inst.name}</span>{/if}
    {#if sentryStatus.kind === 'connected'}
      <span class="brand-sub mono" title={sentryStatus.user.organization_name}>
        {sentryStatus.user.organization_slug}
      </span>
    {/if}
    <button
      class="refresh-btn"
      onclick={() => void refreshSentryInbox(instanceId, { silent: true })}
      disabled={itemsLoading}
      title="Refresh"
      aria-label="Refresh"
    >
      <svg class="i i-sm" viewBox="0 0 24 24" class:spin={itemsLoading}>
        <path d="M3 12a9 9 0 1 0 3-6.7M3 4v5h5"/>
      </svg>
    </button>
  </div>

  <div class="inbox-header">
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
        value={filters.environment ?? ''}
        options={envOpts}
        onChange={pickEnvironment}
        ariaLabel="Environment"
        placeholder="Environment"
        width="100%"
      />
    </div>
    <div class="filter-row three-col">
      <Dropdown
        value={filters.status}
        options={statusOpts}
        onChange={pickStatus}
        ariaLabel="Status"
        width="100%"
      />
      <Dropdown
        value={filters.level}
        options={levelOpts}
        onChange={pickLevel}
        ariaLabel="Level"
        width="100%"
      />
      <Dropdown
        value={filters.sort}
        options={sortOpts}
        onChange={pickSort}
        ariaLabel="Sort"
        width="100%"
      />
    </div>
    <input
      class="filter-search mono"
      type="search"
      value={filters.search}
      oninput={onSearchInput}
      onkeydown={(e) => { if (e.key === 'Enter') { void refreshSentryInbox(instanceId, { silent: false }); } }}
      placeholder="search… (free text or `tag:value`)"
      aria-label="Sentry search query"
    />
    </div>
  </div>

  <div class="inbox-list">
    {#if sentryStatus.kind !== 'connected'}
      <div class="inbox-state">Sentry isn't connected.</div>
    {:else if itemsLoading && items.length === 0}
      <div class="inbox-state">Loading…</div>
    {:else if itemsError}
      <div class="inbox-state inbox-state--error">
        {itemsError}
        <button class="link-inline" onclick={() => void refreshSentryInbox(instanceId, { silent: false })}>Retry</button>
      </div>
    {:else if items.length === 0}
      <div class="inbox-state">No issues match the current query.</div>
    {:else}
      {#each items as issue (issue.id)}
        {@const levelClass = sentryLevelClass(issue.level)}
        {@const dupeExcerpt = issue.metadata_value && issue.title.includes(issue.metadata_value)}
        <div
          class="inbox-item sentry-item {levelClass}"
          draggable="true"
          role="button"
          tabindex="0"
          ondragstart={(e) => onDragStart({ source: 'sentry', item: issue }, e)}
          ondragend={onDragEnd}
          onmousedown={onCardMouseDown}
          onclick={(e) => { if (isClickNotDrag(e)) openSentryFocus(issue.id); }}
          onkeydown={(e) => { if (e.key === 'Enter') openSentryFocus(issue.id); }}
        >
          <div class="inbox-item-row1">
            <span class="state-dot"></span>
            <span class="inbox-item-id mono">{issue.short_id || issue.id}</span>
            <span class="state-pill {levelClass}">{issue.level}</span>
            {#if issue.status !== 'unresolved'}
              <span class="kind-tag">{issue.status}</span>
            {/if}
            <span class="events-chip mono" title="event count · users affected">
              {issue.count}{#if issue.user_count > 0}·{issue.user_count}u{/if}
            </span>
            <span class="inbox-item-time mono">{relativeTime(issue.last_seen, now)}</span>
          </div>
          <div class="inbox-item-title">{shortText(issue.title, 140)}</div>
          {#if issue.metadata_value && !dupeExcerpt}
            <div class="inbox-item-sub mono">{shortText(issue.metadata_value, 120)}</div>
          {/if}
          <div class="inbox-item-row3 mono">{issue.project_slug}</div>
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
    background: var(--bg-1);
  }
  /* Lock the brand row's box height as `height` (not min-height) so it
     can't grow from intrinsic child sizes (e.g. new-issue-btn 27.25px
     vs refresh-btn 27px) and the bottom border lines up with Jira /
     GitHub at the exact same Y. */
  .inbox-brand {
    padding: 16px 20px 10px; display: flex; align-items: center; gap: 10px;
    height: 54px;
  }
  .brand-word { font-size: 14px; font-weight: 600; color: var(--text-0); letter-spacing: -0.01em; }
  /* Inbox-item source-mark (small "St" badge inside each issue card) —
     brand-bar icon now uses the shared .brand-icon class so it matches
     Jira / GitHub one-for-one. */
  .source-mark {
    width: 22px; height: 22px; border-radius: 5px;
    display: inline-flex; align-items: center; justify-content: center;
    font-size: 10.5px; font-weight: 700;
    background: var(--bg-2); color: var(--text-1);
    border: 1px solid var(--border-neutral-hi);
  }
  .sentry-mark { color: #f88f74; background: rgba(248, 143, 116, 0.12); border-color: rgba(248, 143, 116, 0.3); }
  /* Mirror Jira's `new-issue-btn` / GitHub's `new-pr-btn` box
     (padding 4px 8px, 1px border, font-size 11.5px / line-height 1.5
     → ~27px tall). Since this button is icon-only, an explicit
     `min-height` ensures it doesn't collapse to the 14px icon and
     undercut the row height — without it, sentry's brand-bar would
     resolve to 22px (the source-mark) instead of matching jira's
     ~27px row dictated by its text-bearing button. */
  .refresh-btn {
    display: inline-flex; align-items: center; justify-content: center;
    padding: 0 8px; min-height: 27px;
    border-radius: 6px;
    background: transparent;
    border: 1px solid var(--border-neutral-hi);
    color: var(--text-2);
    cursor: pointer;
  }
  .refresh-btn:hover { background: var(--bg-2); color: var(--text-0); }
  .refresh-btn .spin { animation: spin 0.8s linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }

  /* Padding is provided by the parent .inbox-header (global rule in
     app.css: 14px 20px 8px) so the filter strip lines up vertically
     with Jira / GitHub. */
  .filter-bar {
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

  /* `.inbox-item` base layout + state-pill colors live in app.css
     (shared with GitHub / Jira). This component only adds Sentry-
     specific bits: the optional metadata-excerpt sub-line and the
     events-count chip on the right of row 1. */
  .inbox-item-sub {
    font-size: 11px; color: var(--text-2);
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  /* Events / users count — pushed to the right next to the
     timestamp. Tabular-numerics + a tighter background so the
     "10420" digit count doesn't shove the time off the row. The
     chip itself doesn't have its own bg in dark mode (would be
     visual noise alongside the state-pill); just dim text. */
  .events-chip {
    margin-left: auto;
    font-size: 10.5px;
    color: var(--text-mute);
    font-variant-numeric: tabular-nums;
    padding: 1px 6px;
    border-radius: 4px;
    background: rgba(139, 150, 171, 0.10);
  }
  /* Push the time AFTER the events-chip — both want margin-left: auto
     but the chip claims it first; this keeps the time pinned to the
     end of the row regardless of chip width. */
  .inbox-item-row1 .events-chip + .inbox-item-time { margin-left: 6px; }
  .mini-kind { font-size: 10.5px; color: var(--text-1); }
  .mini-repo { color: var(--text-mute); }
</style>
