<script lang="ts">
  import { slide } from 'svelte/transition';
  import { cubicOut } from 'svelte/easing';
  import Dropdown, { type DropdownOption } from '$lib/components/ui/Dropdown.svelte';
  import {
    connectionsMeta,
    jiraStatusClass,
    jiraStatusColor,
    relativeTime,
    type JiraItem,
    type JiraStatus
  } from '$lib/data';
  const jiraMeta = connectionsMeta.find((c) => c.id === 'jira')!;
  import {
    layoutState,
    startResizeById,
    activeInstances
  } from '$lib/state/layout.svelte';
  import ColumnControls from '$lib/components/workbench/ColumnControls.svelte';
  import {
    inboxState,
    invalidateJiraStatuses,
    loadJiraBoards,
    loadJiraProjects,
    loadJiraSprints,
    loadJiraStatuses,
    updateJiraFilters
  } from '$lib/state/inbox.svelte';

  interface Props {
    instanceId: string;
    jiraStatus: JiraStatus;
    now: number;
    onOpenUserPicker: () => void;
    onRefreshJiraInbox: () => void;
    onDragStart: (payload: { source: 'jira'; item: JiraItem }, e: DragEvent) => void;
    onDragEnd: () => void;
    onCardMouseDown: (e: MouseEvent) => void;
    isClickNotDrag: (e: MouseEvent) => boolean;
    onOpenBrowser: (url: string) => void;
    onOpenCreateIssue: () => void;
  }

  let {
    instanceId,
    jiraStatus,
    now,
    onOpenUserPicker,
    onRefreshJiraInbox,
    onDragStart,
    onDragEnd,
    onCardMouseDown,
    isClickNotDrag,
    onOpenBrowser,
    onOpenCreateIssue
  }: Props = $props();

  // Lazy loaders — fire when a Dropdown opens, so disconnected users /
  // Free-tier workspaces don't pay the round-trip cost before the filter
  // panel is actually opened.
  function onProjectOpen() {
    if (!inboxState.jiraProjectOptions.length) void loadJiraProjects();
  }

  function onBoardOpen() {
    if (!inboxState.jiraBoardOptions.length) {
      void loadJiraBoards(inboxState.jiraFilters.projectKey);
    }
  }

  function onSprintOpen() {
    const bid = inboxState.jiraFilters.boardId;
    if (bid != null && !inboxState.jiraSprintOptions.length) {
      void loadJiraSprints(bid);
    }
  }

  function onStatusOpen() {
    void loadJiraStatuses(inboxState.jiraFilters.projectKey);
  }

  function onProjectChange(value: string) {
    const projectKey = value ? value : null;
    // Picking a project invalidates the board/sprint/status choices — status
    // names are project-specific, so the cached list is stale too.
    updateJiraFilters({ projectKey, boardId: null, sprintId: null, statusName: null });
    inboxState.jiraBoardOptions = [];
    inboxState.jiraSprintOptions = [];
    invalidateJiraStatuses();
    void loadJiraBoards(projectKey);
  }

  function onBoardChange(value: string) {
    const boardId = value ? Number(value) : null;
    updateJiraFilters({ boardId, sprintId: null });
    inboxState.jiraSprintOptions = [];
    void loadJiraSprints(boardId);
  }

  // Sprint dropdown value is a string for wire stability (numeric ids and
  // the `'backlog'` literal both coexist). Empty string = "any".
  function onSprintChange(value: string) {
    let sprintId: number | 'backlog' | null;
    if (value === '') sprintId = null;
    else if (value === 'backlog') sprintId = 'backlog';
    else sprintId = Number(value);
    updateJiraFilters({ sprintId });
  }

  function onStatusChange(value: string) {
    updateJiraFilters({ statusName: value ? value : null });
  }

  function onSearchInput(e: Event) {
    const value = (e.target as HTMLInputElement).value;
    updateJiraFilters({ search: value });
  }

  const sprintSelectValue = $derived.by(() => {
    const s = inboxState.jiraFilters.sprintId;
    if (s === 'backlog') return 'backlog';
    if (typeof s === 'number') return String(s);
    return '';
  });

  const projectOptions = $derived<DropdownOption<string>[]>([
    { value: '', label: 'All projects' },
    ...inboxState.jiraProjectOptions.map((p) => ({
      value: p.key,
      label: `${p.key} · ${p.name}`
    }))
  ]);

  const boardOptions = $derived<DropdownOption<string>[]>([
    { value: '', label: 'All boards' },
    ...inboxState.jiraBoardOptions.map((b) => ({
      value: String(b.id),
      label: b.name,
      hint: b.type_
    }))
  ]);

  const sprintOptions = $derived<DropdownOption<string>[]>([
    { value: '', label: 'Any sprint' },
    { value: 'backlog', label: 'No sprint (backlog)' },
    ...inboxState.jiraSprintOptions.map((s) => ({
      value: String(s.id),
      label: s.name,
      hint: s.state
    }))
  ]);

  const statusOptions = $derived<DropdownOption<string>[]>([
    { value: '', label: 'Any status' },
    ...inboxState.jiraStatusOptions.map((s) => ({
      value: s.name,
      label: s.name,
      color: jiraStatusColor(s)
    }))
  ]);

  const inst = $derived(activeInstances().find((i) => i.id === instanceId));
  const order = $derived(activeInstances().findIndex((i) => i.id === instanceId));
</script>

<section
  class="wb-column inbox"
  data-instance-id={instanceId}
  data-kind="jira"
  transition:slide={{ duration: 240, axis: 'x', easing: cubicOut }}
  style="order: {order}; flex: 0 0 {inst?.width ?? 420}px"
>
  <ColumnControls {instanceId} kind="jira" />
  <div class="wb-col-resize" class:snap-flash={layoutState.snapFlashInstanceId === instanceId} role="separator" aria-orientation="vertical" onpointerdown={(e) => startResizeById(instanceId, e)}></div>
  <div class="inbox-brand">
    <span class="brand-icon {jiraMeta.iconClass}" class:conn-icon--svg={!!jiraMeta.iconSvg}>
      {#if jiraMeta.iconSvg}
        <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">{@html jiraMeta.iconSvg}</svg>
      {:else}
        {jiraMeta.iconLetters}
      {/if}
    </span>
    <span class="brand-word">Jira</span>
    {#if inst?.name}<span class="bench-name mono" title="Bench id">{inst.name}</span>{/if}
    {#if jiraStatus.kind === 'connected'}
      <span class="brand-sub mono">{jiraStatus.user.workspace}</span>
    {/if}
    <button
      class="new-issue-btn"
      onclick={onOpenCreateIssue}
      title="Create a new Jira issue"
      aria-label="New issue"
      disabled={jiraStatus.kind !== 'connected'}
    >
      <svg class="i i-sm" viewBox="0 0 24 24"><path d="M12 5v14M5 12h14" /></svg>
      <span>New issue</span>
    </button>
  </div>
  <div class="inbox-header">
    <div class="filter-bar">
      <div class="filter-row">
        <div class="filter-cell">
          <Dropdown
            value={inboxState.jiraFilters.projectKey ?? ''}
            options={projectOptions}
            onChange={onProjectChange}
            onOpen={onProjectOpen}
            ariaLabel="Project"
            placeholder={inboxState.jiraProjectOptionsLoading ? 'Loading…' : 'All projects'}
            width="100%"
          />
        </div>
        <button class="icon-btn" onclick={onRefreshJiraInbox} title="Refresh" aria-label="Refresh Jira" disabled={inboxState.jiraItemsLoading}>
          <svg class="i i-sm" viewBox="0 0 24 24" style="transform: rotate({inboxState.jiraItemsLoading ? 360 : 0}deg); transition: transform 0.6s;">
            <path d="M21 12a9 9 0 0 1-9 9 9 9 0 0 1-8.5-6" />
            <path d="M3 12a9 9 0 0 1 9-9 9 9 0 0 1 8.5 6" />
            <polyline points="21 3 21 9 15 9" />
            <polyline points="3 21 3 15 9 15" />
          </svg>
        </button>
      </div>
      <div class="filter-row">
        <div class="filter-cell">
          <Dropdown
            value={inboxState.jiraFilters.boardId == null ? '' : String(inboxState.jiraFilters.boardId)}
            options={boardOptions}
            onChange={onBoardChange}
            onOpen={onBoardOpen}
            ariaLabel="Board"
            placeholder={inboxState.jiraBoardOptionsLoading ? 'Loading…' : 'All boards'}
            width="100%"
          />
        </div>
        <div class="filter-cell">
          <Dropdown
            value={sprintSelectValue}
            options={sprintOptions}
            onChange={onSprintChange}
            onOpen={onSprintOpen}
            ariaLabel="Sprint"
            placeholder={inboxState.jiraSprintOptionsLoading ? 'Loading…' : 'Any sprint'}
            width="100%"
          />
        </div>
      </div>
      <div class="filter-row">
        <div class="filter-cell">
          <Dropdown
            value={inboxState.jiraFilters.statusName ?? ''}
            options={statusOptions}
            onChange={onStatusChange}
            onOpen={onStatusOpen}
            ariaLabel="Status"
            placeholder={inboxState.jiraStatusOptionsLoading ? 'Loading…' : 'Any status'}
            width="100%"
          />
        </div>
      </div>
      <div class="filter-row">
        <input
          class="filter-input"
          type="text"
          placeholder="Search summary/description…"
          value={inboxState.jiraFilters.search}
          oninput={onSearchInput}
          aria-label="Search Jira issues"
        />
      </div>
    </div>
    <button class="assignee-chip" onclick={onOpenUserPicker} title="Change assignee filter">
      {#if inboxState.jiraAssigneeAny}
        <svg class="i i-sm" viewBox="0 0 24 24"><circle cx="9" cy="7" r="4"/><circle cx="17" cy="7" r="3"/><path d="M3 21v-2a4 4 0 0 1 4-4h4a4 4 0 0 1 4 4v2M15 14h2a4 4 0 0 1 4 4v3"/></svg>
        <span>Any user</span>
      {:else if inboxState.jiraAssignee}
        <img src={inboxState.jiraAssignee.avatar_url} alt="" class="chip-avatar" />
        <span>{inboxState.jiraAssignee.display_name}</span>
      {:else}
        <svg class="i i-sm" viewBox="0 0 24 24"><path d="M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2"/><circle cx="12" cy="7" r="4"/></svg>
        <span>Me (authenticated)</span>
      {/if}
      <svg class="i i-sm" viewBox="0 0 24 24" style="color: var(--text-2)"><path d="m6 9 6 6 6-6"/></svg>
    </button>
    <div class="inbox-controls">
      <span class="inbox-count mono">{inboxState.jiraItems.length} issues</span>
    </div>
  </div>
  <div class="inbox-list">
    {#if inboxState.jiraItemsLoading && inboxState.jiraItems.length === 0}
      <div class="inbox-state">Loading…</div>
    {:else if inboxState.jiraItemsError}
      <div class="inbox-state inbox-state--error">
        {inboxState.jiraItemsError}
        <button class="link-inline" onclick={onRefreshJiraInbox}>Retry</button>
      </div>
    {:else if inboxState.jiraItems.length === 0}
      <div class="inbox-state">
        {#if inboxState.jiraAssigneeAny}
          No open issues match the current filters.
        {:else}
          No open issues assigned to {inboxState.jiraAssignee ? inboxState.jiraAssignee.display_name : 'the authenticated account'}.
        {/if}
      </div>
    {:else}
      {#each inboxState.jiraItems as j (j.id)}
        <div
          class="inbox-item"
          draggable="true"
          role="button"
          tabindex="0"
          ondragstart={(e) => onDragStart({ source: 'jira', item: j }, e)}
          ondragend={onDragEnd}
          onmousedown={onCardMouseDown}
          onclick={(e) => { if (isClickNotDrag(e)) inboxState.jiraFocusKey = j.key; }}
          onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); inboxState.jiraFocusKey = j.key; } }}
        >
          <div class="inbox-item-top">
            <span class="source-mark">J</span>
            <span class="inbox-item-id mono">{j.key}</span>
            <button
              class="inbox-item-ext"
              onclick={(e) => { e.stopPropagation(); onOpenBrowser(j.url); }}
              aria-label="Open on Jira"
              title="Open on Jira"
            >
              <svg class="i i-sm" viewBox="0 0 24 24"><path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6" /><path d="M15 3h6v6M10 14 21 3" /></svg>
            </button>
            <span class="inbox-item-time mono">{relativeTime(j.updated, now)}</span>
          </div>
          <div class="inbox-item-title">{j.summary}</div>
          <div class="inbox-item-meta">
            <span class="mini-tag {jiraStatusClass(j.status_category)}">{j.status.toLowerCase()}</span>
            <span class="mini-kind">{j.issue_type.toLowerCase()}</span>
            {#if j.priority}<span class="mini-repo">· {j.priority.toLowerCase()}</span>{/if}
          </div>
        </div>
      {/each}
    {/if}
  </div>
</section>

<style>
  /* Jira inbox column. Uses generic .wb-column / .wb-col-controls
     / .wb-col-resize rules defined in +page.svelte (shared). */

  .inbox { display: flex; flex-direction: column; min-height: 0; }

  .inbox-brand {
    padding: 16px 20px 10px; display: flex; align-items: center; gap: 10px;
  }
  .brand-word { font-size: 14px; font-weight: 600; color: var(--text-0); letter-spacing: -0.01em; }
  .brand-sub { font-size: 11.5px; color: var(--text-2); margin-left: auto; }
  .new-issue-btn {
    display: inline-flex; align-items: center; gap: 4px;
    padding: 4px 8px; border-radius: 6px;
    background: var(--bg-1);
    border: 1px solid var(--border-neutral-hi);
    color: var(--text-1);
    font-size: 11.5px; font-weight: 500;
    transition: all 120ms;
    cursor: pointer;
  }
  .new-issue-btn:hover:not(:disabled) { background: var(--bg-2); color: var(--text-0); border-color: var(--border-hi); }
  .new-issue-btn:disabled { opacity: 0.5; cursor: not-allowed; }
  .inbox-header { padding: 14px 20px 8px; }
  .inbox-controls { margin-top: 10px; display: flex; align-items: center; justify-content: space-between; padding: 0 4px; }
  .inbox-count { font-size: 11px; color: var(--text-mute); }

  .filter-bar { display: flex; flex-direction: column; gap: 6px; margin-bottom: 10px; }
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

  .inbox-item {
    padding: 10px 12px;
    border-radius: 8px;
    background: var(--bg-1); border: 1px solid var(--border-neutral);
    cursor: pointer;
    transition: all 120ms;
    display: flex; flex-direction: column; gap: 5px;
  }
  .inbox-item:hover { background: var(--bg-2); border-color: var(--border-neutral-hi); }
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
  .inbox-item-ext {
    display: inline-flex; align-items: center; justify-content: center;
    width: 18px; height: 18px; border-radius: 4px;
    color: var(--text-mute); opacity: 0;
    transition: all 120ms;
  }
  .inbox-item-ext :global(svg) { width: 10px; height: 10px; }
  .inbox-item:hover .inbox-item-ext { opacity: 1; }
  .inbox-item-ext:hover { color: var(--accent-bright); background: var(--bg-2); }
  .inbox-item-time { margin-left: auto; font-size: 10.5px; color: var(--text-mute); font-variant-numeric: tabular-nums; }
  .inbox-item-title {
    font-size: 13px; color: var(--text-0); font-weight: 500;
    line-height: 1.4; margin-bottom: 6px; word-break: break-word;
  }
  .inbox-item-meta { display: flex; align-items: center; gap: 6px; font-size: 11px; color: var(--text-2); flex-wrap: wrap; }

  .mini-tag { padding: 1px 6px; border-radius: 4px; font-size: 10px; font-weight: 600; text-transform: lowercase; }
  .mini-kind { color: var(--text-2); text-transform: lowercase; }
  .mini-repo { color: var(--text-mute); font-size: 10.5px; }

  .assignee-chip {
    display: inline-flex; align-items: center; gap: 8px;
    padding: 8px 12px;
    background: var(--bg-1);
    border: 1px solid var(--border-neutral);
    border-radius: 8px;
    font-size: 12px;
    color: var(--text-1);
    transition: all 120ms;
    width: 100%;
    margin-bottom: 10px;
  }
  .assignee-chip:hover { border-color: var(--border-neutral-hi); color: var(--text-0); background: var(--bg-2); }
  .chip-avatar {
    width: 20px; height: 20px;
    border-radius: 50%;
    object-fit: cover;
    flex-shrink: 0;
    background: linear-gradient(135deg, #5aa2ff, #8b96ab);
  }
</style>
