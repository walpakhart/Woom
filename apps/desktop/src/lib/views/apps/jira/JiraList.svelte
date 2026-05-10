<script lang="ts">
  /* JiraList — левая панель JiraApp. Standalone: читает inbox state
     напрямую, рендерит filter row + groups + item cards. Click on item
     → sets inboxState.jiraFocusKey (детейл откроется в правой
     панели JiraApp inline).
     Drag handlers пробрасываются сверху (для drop-в-Claude). */
  import {
    inboxState,
    jiraItemsFor,
    jiraItemsLoadingFor,
    jiraItemsErrorFor
  } from '$lib/state/inbox.svelte';
  import { relativeTime, jiraStatusClass, type JiraItem, type JiraStatus } from '$lib/data';
  import Dropdown from '$lib/components/ui/Dropdown.svelte';

  interface Props {
    instanceId: string;
    jiraStatus: JiraStatus;
    now: number;
    onRefresh: () => void;
    onOpenCreateIssue: () => void;
    onOpenBrowser: (url: string) => void;
    onDragStart: (payload: { source: 'jira'; item: JiraItem }, e: DragEvent) => void;
    onDragEnd: () => void;
    onCardMouseDown: (e: MouseEvent) => void;
    isClickNotDrag: (e: MouseEvent) => boolean;
    /** One-click handoff to Claude / Cursor — equivalent to dragging
     *  the row onto the matching rail icon, but for users who prefer
     *  a button over a drag gesture. Shown on each ticket card. */
    onSendToClaude: (item: JiraItem) => void;
    onSendToCursor: (item: JiraItem) => void;
  }
  let p: Props = $props();

  function clickSendToClaude(it: JiraItem, e: MouseEvent) {
    e.stopPropagation();
    p.onSendToClaude(it);
  }
  function clickSendToCursor(it: JiraItem, e: MouseEvent) {
    e.stopPropagation();
    p.onSendToCursor(it);
  }

  const items = $derived(jiraItemsFor(p.instanceId));
  const loading = $derived(jiraItemsLoadingFor(p.instanceId));
  const error = $derived(jiraItemsErrorFor(p.instanceId));

  /** Search + filter state. In-memory; resets on refresh. Each chip
   *  is a toggle (click again to deselect → "all"); dropdowns use a
   *  `__all__` sentinel for "any". */
  let query = $state('');
  let roleFilter = $state<'mine' | 'reporter' | null>(null);
  let statusFilter = $state<'open' | 'inprogress' | 'done' | null>(null);
  let projectFilter = $state<string | null>(null);
  let assigneeFilter = $state<string | null>(null);

  const me = $derived(p.jiraStatus.kind === 'connected' ? p.jiraStatus.user.account_id : null);

  /** Unique projects (key prefix before the dash) seen in the items. */
  const projectOptions = $derived.by(() => {
    const set = new Set<string>();
    for (const it of items) {
      const dash = it.key.indexOf('-');
      if (dash > 0) set.add(it.key.slice(0, dash));
    }
    const list = Array.from(set).sort();
    return [{ value: '__all__' as string, label: 'All projects' }, ...list.map((v) => ({ value: v, label: v }))];
  });

  /** Unique assignees seen in the items. Current user pinned first. */
  const assigneeOptions = $derived.by(() => {
    const map = new Map<string, string>(); // account_id → display_name
    for (const it of items) {
      const a = it.assignee;
      if (a?.account_id) map.set(a.account_id, a.display_name);
    }
    const out: { value: string; label: string; hint?: string }[] = [
      { value: '__all__', label: 'Anyone' }
    ];
    if (me && map.has(me)) {
      out.push({ value: me, label: map.get(me)!, hint: 'You' });
    }
    for (const [id, name] of Array.from(map.entries()).sort((a, b) => a[1].localeCompare(b[1]))) {
      if (id === me) continue;
      out.push({ value: id, label: name });
    }
    return out;
  });

  function toggleRole(v: 'mine' | 'reporter') {
    roleFilter = roleFilter === v ? null : v;
  }
  function toggleStatus(v: 'open' | 'inprogress' | 'done') {
    statusFilter = statusFilter === v ? null : v;
  }

  const filtered = $derived.by(() => {
    const q = query.trim().toLowerCase();
    return items.filter((it) => {
      if (q) {
        const titleMatch = it.summary.toLowerCase().includes(q);
        const keyMatch = it.key.toLowerCase().includes(q);
        const assigneeMatch = it.assignee?.display_name?.toLowerCase().includes(q) ?? false;
        const reporterMatch = it.reporter?.display_name?.toLowerCase().includes(q) ?? false;
        if (!titleMatch && !keyMatch && !assigneeMatch && !reporterMatch) return false;
      }
      if (roleFilter === 'mine') {
        if (!me || it.assignee?.account_id !== me) return false;
      } else if (roleFilter === 'reporter') {
        if (!me || it.reporter?.account_id !== me) return false;
      }
      if (statusFilter === 'open' && it.status_category !== 'new') return false;
      if (statusFilter === 'inprogress' && it.status_category !== 'indeterminate') return false;
      if (statusFilter === 'done' && it.status_category !== 'done') return false;
      if (projectFilter) {
        const dash = it.key.indexOf('-');
        const proj = dash > 0 ? it.key.slice(0, dash) : '';
        if (proj !== projectFilter) return false;
      }
      if (assigneeFilter) {
        if (it.assignee?.account_id !== assigneeFilter) return false;
      }
      return true;
    });
  });

  /** Group items by status_category: in-progress / new (triage) / done.
   *  Inside each group items already arrive sorted by `updated` desc. */
  const groups = $derived.by(() => {
    const inprogress: JiraItem[] = [];
    const triage: JiraItem[] = [];
    const done: JiraItem[] = [];
    for (const it of filtered) {
      if (it.status_category === 'indeterminate') inprogress.push(it);
      else if (it.status_category === 'done') done.push(it);
      else triage.push(it);
    }
    return [
      { label: 'In progress', items: inprogress },
      { label: 'Triage', items: triage },
      { label: 'Done', items: done }
    ].filter((g) => g.items.length > 0);
  });

  const anyFilterActive = $derived(
    query.trim().length > 0 ||
    roleFilter !== null ||
    statusFilter !== null ||
    projectFilter !== null ||
    assigneeFilter !== null
  );

  function clearFilters() {
    query = '';
    roleFilter = null;
    statusFilter = null;
    projectFilter = null;
    assigneeFilter = null;
  }

  function clickItem(it: JiraItem, e: MouseEvent) {
    if (!p.isClickNotDrag(e)) return;
    inboxState.jiraFocusKey = it.key;
  }

  function priorityClass(pri: string | null): string {
    if (!pri) return '';
    const p = pri.toLowerCase();
    if (p === 'highest' || p === 'high') return 'pri--high';
    if (p === 'medium') return 'pri--med';
    return 'pri--low';
  }

  function typeClass(t: string): string {
    const tt = t.toLowerCase();
    if (tt === 'bug') return 'type--bug';
    if (tt === 'story') return 'type--story';
    if (tt === 'epic') return 'type--epic';
    if (tt === 'task') return 'type--task';
    return '';
  }
</script>

<aside class="jl app-pane">
  <header class="jl-head">
    <h1 class="app-pane-head-h">Jira</h1>
    {#if p.jiraStatus.kind === 'connected'}
      <span class="app-pane-head-meta">{p.jiraStatus.user.workspace}</span>
    {/if}
    <span class="app-pane-head-spacer"></span>
    <button class="jl-act" onclick={p.onOpenCreateIssue} title="New issue" disabled={p.jiraStatus.kind !== 'connected'}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"><path d="M12 5v14M5 12h14"/></svg>
      <span>New</span>
    </button>
    <button class="jl-icon" onclick={p.onRefresh} title="Refresh" disabled={loading}>
      <svg class:spin={loading} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"><path d="M21 12a9 9 0 1 1-3-6.7"/><path d="M21 4v5h-5"/></svg>
    </button>
  </header>

  <div class="jl-filters">
    <label class="jl-search">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"><circle cx="11" cy="11" r="8"/><path d="m21 21-4.3-4.3"/></svg>
      <input type="text" placeholder="Search summary, KEY-123, assignee…" bind:value={query} spellcheck="false" />
      {#if query}
        <button class="jl-search-clear" onclick={() => (query = '')} aria-label="Clear search">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>
        </button>
      {/if}
    </label>
    <div class="jl-chips">
      <button class="jl-toggle" class:active={roleFilter === 'mine'} disabled={!me} onclick={() => toggleRole('mine')} title="Assigned to me">
        <span class="jl-toggle-dot"></span>
        Mine
      </button>
      <button class="jl-toggle" class:active={roleFilter === 'reporter'} disabled={!me} onclick={() => toggleRole('reporter')} title="Reported by me">
        <span class="jl-toggle-dot"></span>
        Reporter
      </button>
      <span class="jl-divider" aria-hidden="true"></span>
      <button class="jl-toggle" class:active={statusFilter === 'open'} onclick={() => toggleStatus('open')} title="Open / triage">
        <span class="jl-toggle-dot"></span>
        Open
      </button>
      <button class="jl-toggle" class:active={statusFilter === 'inprogress'} onclick={() => toggleStatus('inprogress')} title="In progress">
        <span class="jl-toggle-dot"></span>
        In progress
      </button>
      <button class="jl-toggle" class:active={statusFilter === 'done'} onclick={() => toggleStatus('done')} title="Done">
        <span class="jl-toggle-dot"></span>
        Done
      </button>
      <span class="jl-divider" aria-hidden="true"></span>

      <span class="jl-dd">
        <Dropdown
          value={projectFilter ?? '__all__'}
          options={projectOptions}
          onChange={(v) => (projectFilter = v === '__all__' ? null : v)}
          placeholder="Project"
          ariaLabel="Filter by project"
          variant="chip"
          compact
        />
      </span>
      <span class="jl-dd">
        <Dropdown
          value={assigneeFilter ?? '__all__'}
          options={assigneeOptions}
          onChange={(v) => (assigneeFilter = v === '__all__' ? null : v)}
          placeholder="Assignee"
          ariaLabel="Filter by assignee"
          variant="chip"
          compact
        />
      </span>

      {#if anyFilterActive}
        <button class="jl-chip-clear" onclick={clearFilters} title="Clear all filters">Clear</button>
      {/if}
    </div>
  </div>

  <div class="jl-body">
    {#if error}
      <div class="jl-error">
        <p class="jl-error-h serif">Couldn't load Jira</p>
        <p class="jl-error-p mono">{error}</p>
        <button class="jl-error-retry" onclick={p.onRefresh}>Retry</button>
      </div>
    {:else if loading && items.length === 0}
      <div class="jl-loading">
        <div class="jl-spinner"></div>
        <span class="mono">Loading…</span>
      </div>
    {:else if items.length === 0}
      <div class="jl-empty">
        <p class="jl-empty-h serif">Inbox is empty</p>
        <p class="jl-empty-p">No tickets yet. Create one or refresh.</p>
      </div>
    {:else if filtered.length === 0}
      <div class="jl-empty">
        <p class="jl-empty-h serif">No matches</p>
        <p class="jl-empty-p">No tickets match the current search and filters.</p>
        <button class="jl-error-retry" onclick={clearFilters}>Clear filters</button>
      </div>
    {:else}
      {#each groups as g (g.label)}
        <div class="jl-group">{g.label} <span class="mono">·</span> {g.items.length}</div>
        {#each g.items as it (it.key)}
          {@const isActive = inboxState.jiraFocusKey === it.key}
          <button
            class="jl-card"
            class:active={isActive}
            draggable="true"
            onpointerdown={p.onCardMouseDown}
            ondragstart={(e) => p.onDragStart({ source: 'jira', item: it }, e)}
            ondragend={p.onDragEnd}
            onclick={(e) => clickItem(it, e)}
            ondblclick={() => p.onOpenBrowser(it.url)}
            title={`${it.key} · ${it.summary}`}
          >
            <div class="jl-card-top">
              <span class="jl-card-status {jiraStatusClass(it.status_category)}" title={it.status}></span>
              <span class="jl-card-key mono">{it.key}</span>
              <span class="jl-card-time mono">{relativeTime(it.updated, p.now)}</span>
            </div>
            <div class="jl-card-title">{it.summary}</div>
            <div class="jl-card-meta">
              {#if it.priority}
                <span class="pri {priorityClass(it.priority)}">{it.priority}</span>
              {/if}
              <span class="type {typeClass(it.issue_type)}">{it.issue_type}</span>
              {#if it.assignee}
                <span class="ava" title={it.assignee.display_name}>
                  {(it.assignee.display_name || '?').slice(0, 1).toUpperCase()}
                </span>
              {/if}
              {#if it.labels.length > 0}
                {#each it.labels.slice(0, 2) as l (l)}
                  <span class="label mono">{l}</span>
                {/each}
                {#if it.labels.length > 2}
                  <span class="label-more mono">+{it.labels.length - 2}</span>
                {/if}
              {/if}
            </div>
            <span class="jl-card-sends">
              <span
                class="jl-card-send jl-card-send--claude"
                role="button"
                tabindex="0"
                onclick={(e) => clickSendToClaude(it, e)}
                onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); clickSendToClaude(it, e as unknown as MouseEvent); } }}
                onpointerdown={(e) => e.stopPropagation()}
                title="Send to Claude"
                aria-label="Send to Claude"
              >
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M22 2 11 13"/><path d="m22 2-7 20-4-9-9-4 20-7z"/></svg>
                <span>Claude</span>
              </span>
              <span
                class="jl-card-send jl-card-send--cursor"
                role="button"
                tabindex="0"
                onclick={(e) => clickSendToCursor(it, e)}
                onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); clickSendToCursor(it, e as unknown as MouseEvent); } }}
                onpointerdown={(e) => e.stopPropagation()}
                title="Send to Cursor"
                aria-label="Send to Cursor"
              >
                <svg viewBox="0 0 24 24" fill="currentColor"><path d="M3 3l8 18 2-8 8-2z"/></svg>
                <span>Cursor</span>
              </span>
            </span>
          </button>
        {/each}
      {/each}
    {/if}
  </div>
</aside>

<style>
  .jl {
    /* Width comes from the parent `app-shell` grid track. */
    min-width: 0;
  }

  .jl-head {
    flex: 0 0 46px;
    display: flex; align-items: center; gap: 10px;
    padding: 0 12px;
    border-bottom: 1px solid var(--border);
    background: linear-gradient(180deg, var(--bg-2), var(--bg-1));
  }
  .jl-act {
    display: inline-flex; align-items: center; gap: 5px;
    padding: 4px 9px;
    border-radius: 7px;
    font-size: 11.5px; color: var(--text-1);
    background: var(--bg-2);
    border: 1px solid var(--border);
    cursor: pointer;
    transition: color 140ms, background 140ms, border-color 140ms;
  }
  .jl-act:hover:not(:disabled) {
    color: var(--accent-bright);
    background: var(--accent-soft);
    border-color: var(--border-accent-2);
  }
  .jl-act:disabled { opacity: 0.5; cursor: not-allowed; }
  .jl-act svg { width: 11px; height: 11px; }
  .jl-icon {
    width: 26px; height: 26px;
    display: grid; place-items: center;
    color: var(--text-2);
    background: transparent; border: none; cursor: pointer;
    border-radius: 6px;
  }
  .jl-icon:hover:not(:disabled) { color: var(--text-0); background: var(--bg-elev, var(--bg-2)); }
  .jl-icon svg { width: 13px; height: 13px; }
  .jl-icon .spin { animation: jl-spin 0.9s linear infinite; }
  @keyframes jl-spin { to { transform: rotate(360deg); } }

  /* Filter bar — search + role/status chips + project select. */
  .jl-filters {
    flex: 0 0 auto;
    display: flex; flex-direction: column;
    gap: 6px;
    padding: 8px 12px 10px;
    border-bottom: 1px solid var(--border);
    background: var(--bg-1);
  }
  .jl-search {
    position: relative;
    display: flex; align-items: center; gap: 6px;
    padding: 0 8px;
    height: 28px;
    border-radius: 6px;
    background: var(--bg-2);
    border: 1px solid var(--border);
    transition: border-color 120ms;
  }
  .jl-search:focus-within {
    border-color: var(--border-accent);
    background: var(--bg-1);
  }
  .jl-search > svg { width: 12px; height: 12px; color: var(--text-mute); flex-shrink: 0; }
  .jl-search input {
    flex: 1; min-width: 0;
    background: transparent; border: 0; outline: 0;
    color: var(--text-0); font-size: 12px; font-family: inherit;
  }
  .jl-search input::placeholder { color: var(--text-mute); }
  .jl-search-clear {
    width: 16px; height: 16px;
    display: grid; place-items: center;
    border: 0; background: transparent; color: var(--text-mute);
    cursor: pointer; border-radius: 4px;
  }
  .jl-search-clear:hover { color: var(--text-0); background: var(--bg-3); }
  .jl-search-clear svg { width: 10px; height: 10px; }

  .jl-chips { display: flex; gap: 6px; flex-wrap: wrap; align-items: center; }
  .jl-divider {
    width: 1px; height: 14px;
    background: var(--border);
    margin: 0 2px;
  }

  .jl-toggle {
    display: inline-flex; align-items: center; gap: 6px;
    padding: 4px 10px 4px 8px;
    border-radius: 999px;
    background: var(--bg-2);
    border: 1px solid var(--border);
    color: var(--text-1);
    font-size: 11px; font-weight: 500;
    cursor: pointer;
    transition: all 140ms;
    user-select: none;
    white-space: nowrap;
  }
  .jl-toggle-dot {
    width: 8px; height: 8px;
    border-radius: 50%;
    background: transparent;
    box-shadow: inset 0 0 0 1.5px var(--text-mute);
    transition: all 140ms;
    flex-shrink: 0;
  }
  .jl-toggle:hover:not(:disabled):not(.active) {
    color: var(--text-0); background: var(--bg-3); border-color: var(--border-hi);
  }
  .jl-toggle:hover:not(:disabled):not(.active) .jl-toggle-dot {
    box-shadow: inset 0 0 0 1.5px var(--text-1);
  }
  .jl-toggle.active {
    color: var(--accent-bright);
    background: color-mix(in srgb, var(--src-jira) 14%, transparent);
    border-color: color-mix(in srgb, var(--src-jira) 40%, transparent);
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--src-jira) 8%, transparent);
  }
  .jl-toggle.active .jl-toggle-dot {
    background: var(--src-jira);
    box-shadow:
      inset 0 0 0 1.5px var(--src-jira),
      0 0 6px color-mix(in srgb, var(--src-jira) 60%, transparent);
  }
  .jl-toggle:disabled { opacity: 0.45; cursor: not-allowed; }

  .jl-dd { display: inline-flex; }
  .jl-dd :global(.dd-trigger) {
    border-radius: 999px;
    border: 1px solid var(--border);
    background: var(--bg-2);
    color: var(--text-1);
    font-size: 11px;
    height: auto;
    padding: 4px 10px;
    transition: all 140ms;
  }
  .jl-dd :global(.dd-trigger:hover) {
    color: var(--text-0); background: var(--bg-3); border-color: var(--border-hi);
  }
  .jl-dd :global(.dd-trigger[aria-expanded="true"]) {
    color: var(--accent-bright);
    background: color-mix(in srgb, var(--src-jira) 14%, transparent);
    border-color: color-mix(in srgb, var(--src-jira) 40%, transparent);
  }

  .jl-chip-clear {
    padding: 4px 10px;
    background: transparent;
    border: 1px dashed var(--border-hi);
    border-radius: 999px;
    font-size: 10.5px;
    color: var(--text-mute);
    cursor: pointer;
    margin-left: auto;
    transition: all 120ms;
  }
  .jl-chip-clear:hover { color: var(--text-0); border-color: var(--text-mute); border-style: solid; }

  .jl-body {
    flex: 1; min-height: 0;
    overflow-y: auto;
    padding: 4px 8px 12px;
  }

  .jl-group {
    padding: 14px 8px 6px;
    font-size: 9.5px; font-weight: 700;
    letter-spacing: 0.10em;
    text-transform: uppercase;
    color: var(--text-mute);
    font-family: 'JetBrains Mono', monospace;
    display: flex; align-items: center; gap: 8px;
  }
  .jl-group::after {
    content: ''; flex: 1; height: 1px;
    background: linear-gradient(90deg, var(--border), transparent);
  }
  .jl-group .mono { opacity: 0.5; }

  .jl-card {
    position: relative;
    display: flex; flex-direction: column; gap: 5px;
    padding: 10px 12px 11px 14px;
    margin-bottom: 3px;
    width: 100%;
    border-radius: 9px;
    border: 1px solid transparent;
    text-align: left;
    background: transparent;
    cursor: pointer;
    transition: background 120ms, border-color 120ms, box-shadow 200ms;
    user-select: none;
  }
  .jl-card::before {
    content: ''; position: absolute; left: 0; top: 11px; bottom: 11px;
    width: 2px; border-radius: 2px;
    background: var(--src-jira);
    opacity: 0; transition: opacity 200ms;
  }
  .jl-card:hover { background: var(--bg-2); border-color: var(--border); }
  .jl-card:hover::before { opacity: 0.5; }
  .jl-card.active {
    background: var(--bg-2);
    border-color: var(--border-hi);
    box-shadow:
      0 0 0 1px rgba(79, 142, 255, 0.32),
      0 12px 24px rgba(0, 0, 0, 0.28);
  }
  .jl-card.active::before {
    opacity: 1;
    box-shadow: 0 0 10px rgba(79, 142, 255, 0.40);
  }

  .jl-card-top { display: flex; align-items: center; gap: 7px; }
  .jl-card-status {
    width: 7px; height: 7px; border-radius: 50%;
    background: var(--text-mute);
    flex-shrink: 0;
  }
  .jl-card-status.tag--inprogress { background: var(--accent); }
  .jl-card-status.tag--done { background: var(--success); }
  .jl-card-status.tag--open { background: var(--info); }
  .jl-card-key {
    font-size: 11px; color: var(--text-2); font-weight: 500;
  }
  .jl-card-time {
    margin-left: auto;
    font-size: 10px; color: var(--text-mute);
  }

  .jl-card-title {
    font-size: 13px; color: var(--text-0); font-weight: 500;
    line-height: 1.4;
    display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical;
    overflow: hidden;
  }
  .jl-card-meta {
    display: flex; align-items: center; gap: 5px; flex-wrap: wrap;
    font-size: 10.5px; color: var(--text-2);
  }

  .pri {
    display: inline-flex; align-items: center; gap: 3px;
    padding: 1px 6px;
    border-radius: 4px;
    font-size: 10px; font-weight: 500;
    text-transform: capitalize;
  }
  .pri--high { color: #F0A38A; background: rgba(232, 130, 100, 0.10); border: 1px solid rgba(232, 130, 100, 0.24); }
  .pri--med  { color: #D9B86E; background: rgba(217, 184, 110, 0.08); border: 1px solid rgba(217, 184, 110, 0.22); }
  .pri--low  { color: #88C2DD; background: rgba(136, 194, 221, 0.08); border: 1px solid rgba(136, 194, 221, 0.20); }

  .type {
    display: inline-flex; align-items: center;
    padding: 1px 6px;
    border-radius: 4px;
    font-size: 10px; font-weight: 500;
    color: var(--text-1);
    background: var(--bg-3);
    border: 1px solid var(--border);
    text-transform: capitalize;
  }
  .type.type--bug { color: #D9B86E; border-color: rgba(217, 184, 110, 0.22); background: rgba(217, 184, 110, 0.06); }
  .type.type--story { color: #A8DEC8; border-color: rgba(168, 222, 200, 0.22); background: rgba(168, 222, 200, 0.06); }
  .type.type--epic { color: #C9A0E0; border-color: rgba(181, 132, 255, 0.22); background: rgba(181, 132, 255, 0.06); }
  .type.type--task { color: var(--src-jira-2); border-color: rgba(117, 168, 255, 0.22); background: rgba(117, 168, 255, 0.06); }

  .ava {
    width: 16px; height: 16px;
    border-radius: 50%;
    background: linear-gradient(135deg, var(--accent-bright), var(--accent-deep));
    color: #1F1410;
    font-size: 9px; font-weight: 700;
    display: grid; place-items: center;
    flex-shrink: 0;
  }

  .label {
    padding: 1px 5px;
    border-radius: 3px;
    font-size: 9.5px; color: var(--text-mute);
    background: var(--bg-2);
    border: 1px solid var(--border);
  }
  .label-more { font-size: 10px; color: var(--text-mute); }

  /* Send-to-{Claude,Cursor} chips — appear on hover/active in the
     top-right of the card. Two role="button" spans (a real <button>
     would be invalid HTML inside the row's <button>). */
  .jl-card-sends {
    position: absolute;
    top: 8px; right: 10px;
    display: inline-flex; gap: 4px;
    opacity: 0;
    transition: opacity 140ms;
  }
  .jl-card:hover .jl-card-sends,
  .jl-card-sends:focus-within,
  .jl-card.active .jl-card-sends {
    opacity: 1;
  }
  .jl-card-send {
    display: inline-flex; align-items: center; gap: 4px;
    padding: 3px 8px 3px 7px;
    border-radius: 5px;
    font-size: 10px; font-weight: 600;
    cursor: pointer;
    transition: background 140ms, transform 140ms;
    user-select: none;
  }
  .jl-card-send svg { width: 11px; height: 11px; }
  .jl-card-send--claude {
    color: var(--src-claude);
    background: color-mix(in srgb, var(--src-claude) 12%, transparent);
    border: 1px solid color-mix(in srgb, var(--src-claude) 28%, transparent);
  }
  .jl-card-send--claude:hover {
    background: color-mix(in srgb, var(--src-claude) 22%, transparent);
    color: var(--accent-bright);
    transform: translateY(-1px);
  }
  .jl-card-send--cursor {
    color: var(--src-cursor, var(--text-1));
    background: color-mix(in srgb, var(--src-cursor, var(--text-1)) 14%, transparent);
    border: 1px solid color-mix(in srgb, var(--src-cursor, var(--text-1)) 32%, transparent);
  }
  .jl-card-send--cursor:hover {
    background: color-mix(in srgb, var(--src-cursor, var(--text-1)) 24%, transparent);
    color: var(--text-0);
    transform: translateY(-1px);
  }
  .jl-card-send:active { transform: translateY(0); }

  /* Empty / loading / error */
  .jl-empty, .jl-loading, .jl-error {
    text-align: center;
    padding: 50px 20px;
    margin: auto;
  }
  .jl-empty-h, .jl-error-h {
    font-family: 'Geist', 'Inter', -apple-system, system-ui, sans-serif;
    font-size: 22px; font-weight: 600; letter-spacing: -0.015em;
    color: var(--text-0);
    margin: 0 0 10px;
  }
  .jl-empty-p, .jl-error-p {
    font-size: 12.5px; color: var(--text-2);
    line-height: 1.55; margin: 0;
  }
  .jl-error-p { color: var(--error); }
  .jl-error-retry {
    margin-top: 14px;
    padding: 6px 12px;
    border-radius: 7px;
    font-size: 12px;
    background: var(--bg-2);
    border: 1px solid var(--border-hi);
    color: var(--text-1);
    cursor: pointer;
  }
  .jl-error-retry:hover { color: var(--text-0); border-color: var(--border-hi2); }

  .jl-loading {
    display: flex; align-items: center; justify-content: center; gap: 10px;
    color: var(--text-2); font-size: 12px;
  }
  .jl-spinner {
    width: 14px; height: 14px;
    border: 1.5px solid var(--border-hi);
    border-top-color: var(--accent);
    border-radius: 50%;
    animation: jl-spin 0.7s linear infinite;
  }
</style>
