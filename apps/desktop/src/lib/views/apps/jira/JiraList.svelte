<script lang="ts">
  /* JiraList — left panel of JiraApp. Standalone: reads inbox state
     directly, renders filter row + groups + item cards. Click on item
     → sets inboxState.jiraFocusKey (detail opens inline in the right
     panel of JiraApp).
     Drag handlers are passed down from above (for drop-into-Claude). */
  import {
    inboxState,
    jiraItemsFor,
    jiraItemsLoadingFor,
    jiraItemsErrorFor,
    updateJiraFilters,
    persistJiraUiFilters,
    jiraFiltersFor,
    loadJiraSprints,
    loadJiraBoards,
    openUserPicker,
    selectAssignee
  } from '$lib/state/inbox.svelte';
  import { relativeTime, type JiraItem, type JiraStatus } from '$lib/data';
  import Dropdown from '$lib/components/ui/Dropdown.svelte';
  import ListSearchPicker from '$lib/views/apps/_shared/ListSearchPicker.svelte';
  import CardContextMenu, { type MenuItem } from '$lib/views/apps/_shared/CardContextMenu.svelte';
  import { invoke } from '@tauri-apps/api/core';

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
    /** One-click handoff to Claude — equivalent to dragging
     *  the row onto the rail icon, but for users who prefer
     *  a button over a drag gesture. Shown on each ticket card. */
    onSendToClaude: (item: JiraItem) => void;
    /** Fired after a row navigates (Quiet: closes the switcher popover). */
    onNavigate?: () => void;
  }
  let p: Props = $props();

  function clickSendToClaude(it: JiraItem, e: MouseEvent) {
    e.stopPropagation();
    p.onSendToClaude(it);
  }

  const items = $derived(jiraItemsFor(p.instanceId));
  const loading = $derived(jiraItemsLoadingFor(p.instanceId));
  const error = $derived(jiraItemsErrorFor(p.instanceId));

  const currentFilters = $derived(jiraFiltersFor(p.instanceId));

  // Read persisted UI state once at init — call the function directly
  // so Svelte doesn't warn about referencing a derived in $state init.
  // instanceId is stable for the lifetime of this component instance.
  const { instanceId: _instanceId } = p;
  const _init = jiraFiltersFor(_instanceId);

  /** Search + filter state — initialised from persisted filters so the
   *  view survives unmount/remount. Changes are synced back via $effect.
   *  Assignee filter lives in global `inboxState.jiraAssignee` /
   *  `jiraAssigneeAny` (drives the server-side JQL), not in this component. */
  let query = $state(_init.uiQuery ?? '');
  let roleFilter = $state<'reporter' | null>(
    _init.uiRoleFilter === 'reporter' ? 'reporter' : null
  );
  let statusFilter = $state<'open' | 'inprogress' | 'done' | null>(_init.uiStatusFilter ?? null);
  let projectFilter = $state<string | null>(_init.uiProjectFilter ?? null);

  const me = $derived(p.jiraStatus.kind === 'connected' ? p.jiraStatus.user.account_id : null);
  const selectedSprintId = $derived(
    currentFilters?.sprintIds?.length ? String(currentFilters.sprintIds[0]) : '__all__'
  );

  const sprintOptions = $derived.by(() => {
    const opts = inboxState.jiraSprintOptions ?? [];
    const out: { value: string; label: string; hint?: string }[] = [
      { value: '__all__', label: 'All sprints' }
    ];
    for (const s of opts) {
      const hint = s.state === 'active' ? 'Active' : s.state === 'future' ? 'Future' : undefined;
      out.push({ value: String(s.id), label: s.name, hint });
    }
    return out;
  });

  function selectSprint(v: string) {
    if (v === '__all__') {
      updateJiraFilters(p.instanceId, { sprintIds: [] });
    } else {
      updateJiraFilters(p.instanceId, { sprintIds: [parseInt(v)] });
    }
  }

  // Persist UI filter state so it survives remounts — no server refresh needed
  $effect(() => {
    persistJiraUiFilters(p.instanceId, {
      uiQuery: query,
      uiRoleFilter: roleFilter,
      uiStatusFilter: statusFilter,
      uiProjectFilter: projectFilter
    });
  });

  /* Reload boards+sprints whenever the project filter changes. The
     persist effect above mutates filter fields IN PLACE (via
     `persistJiraUiFilters`'s field-level diff), so `currentFilters`
     keeps the same proxy reference across keystrokes and Svelte 5's
     deep tracking only re-runs this effect when `projectKey` itself
     differs. No manual dedupe guard needed — that was the symptom of
     the prior reassign-on-every-keystroke bug. */
  $effect(() => {
    if (p.jiraStatus.kind !== 'connected') return;
    const key = projectFilter ?? currentFilters.projectKey ?? null;
    loadJiraBoards(key).then(() => {
      const board = inboxState.jiraBoardOptions?.[0];
      if (board) loadJiraSprints(board.id);
    }).catch(() => {});
  });

  const JIRA_KEY_RE = /^[A-Z][A-Z0-9]+-\d+$/;

  /* ─── Search picker (server-side) ───────────────────────────────
     The picker fires a JQL search on every keystroke (debounced 250ms)
     and shows top-8 results. It DELIBERATELY ignores the inline list's
     filter chips (Mine / Open / project / sprint) — the picker is a
     quick-jump to *any* ticket in the workspace, not a narrowed view of
     what's currently visible. The previous local-only filter caused
     "ticket exists but picker doesn't find it" when the ticket was
     outside the active filter scope. */
  let searchEl = $state<HTMLLabelElement | null>(null);
  let pickerEl = $state<{ handleKey: (e: KeyboardEvent) => boolean } | null>(null);
  let pickerOpen = $state(false);

  let pickerRemoteItems = $state<JiraItem[] | null>(null);
  let pickerLastQuery = '';
  let pickerSearchTimer: ReturnType<typeof setTimeout> | null = null;

  function jqlEscapeLiteral(s: string): string {
    return s.replace(/\\/g, '\\\\').replace(/"/g, '\\"');
  }

  $effect(() => {
    const q = query.trim();
    if (pickerSearchTimer) {
      clearTimeout(pickerSearchTimer);
      pickerSearchTimer = null;
    }
    if (!q) {
      pickerRemoteItems = null;
      pickerLastQuery = '';
      return;
    }
    if (q === pickerLastQuery && pickerRemoteItems !== null) return;
    pickerSearchTimer = setTimeout(async () => {
      pickerLastQuery = q;
      const upper = q.toUpperCase();
      const isKey = JIRA_KEY_RE.test(upper);
      const esc = jqlEscapeLiteral(q);
      const jql = isKey
        ? `key = "${upper}"`
        : `summary ~ "${esc}" ORDER BY updated DESC`;
      try {
        const res = await invoke<JiraItem[]>('jira_search', { jql });
        if (pickerLastQuery !== q) return;
        pickerRemoteItems = res;
      } catch {
        if (pickerLastQuery !== q) return;
        pickerRemoteItems = [];
      }
    }, 250);
  });

  const pickerRows = $derived.by(() => {
    const q = query.trim().toLowerCase();
    if (!q) return [] as { id: string; title: string; sub: string }[];
    const remote = pickerRemoteItems ?? [];
    const exactKey = q.toUpperCase();
    const ranked = remote.map((it) => {
      const keyL = it.key.toLowerCase();
      let rank = 5;
      if (it.key === exactKey) rank = 0;
      else if (keyL.startsWith(q)) rank = 1;
      else if (it.summary.toLowerCase().includes(q)) rank = 2;
      else if (keyL.includes(q)) rank = 3;
      return { id: it.key, title: it.summary || it.key, sub: it.key, rank };
    });
    ranked.sort((a, b) => a.rank - b.rank);
    return ranked.slice(0, 8).map(({ id, title, sub }) => ({ id, title, sub }));
  });

  function openPicker() {
    if (query.trim().length > 0) pickerOpen = true;
  }
  function closePicker() {
    pickerOpen = false;
  }
  $effect(() => {
    /* Auto-open while there's a query; hide when emptied. */
    pickerOpen = query.trim().length > 0;
  });

  function pickJira(key: string) {
    inboxState.jiraFocusKey = key;
    query = '';
    pickerOpen = false;
    p.onNavigate?.();
  }

  function handleSearchKeydown(e: KeyboardEvent) {
    /* Picker grabs ↑↓ Enter Esc when it has rows. */
    if (pickerEl && pickerEl.handleKey(e)) return;
    /* Fallback: Enter on a typed `KEY-123` jumps directly even when
       the local list doesn't contain it (the user may have typed an
       issue from outside the current filter / project). */
    if (e.key === 'Enter') {
      const q = query.trim().toUpperCase();
      if (JIRA_KEY_RE.test(q)) {
        inboxState.jiraFocusKey = q;
        query = '';
        e.preventDefault();
      }
    }
  }

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

  /** Assignee chip label — reads the GLOBAL Jira assignee filter, which
   *  drives the JQL and a server refetch on change. `jiraAssigneeAny`
   *  trumps `jiraAssignee` ("Anyone" wins); the fall-through "Me" matches
   *  the modal's "Me (authenticated account)" row, which is the default
   *  (`assignee = currentUser()`). */
  const assigneeLabel = $derived.by(() => {
    if (inboxState.jiraAssigneeAny) return 'Anyone';
    if (inboxState.jiraAssignee) return inboxState.jiraAssignee.display_name;
    return 'Me';
  });

  const assigneeActive = $derived(
    inboxState.jiraAssigneeAny || inboxState.jiraAssignee !== null
  );

  function toggleRole(v: 'reporter') {
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
      if (roleFilter === 'reporter') {
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
    assigneeActive ||
    selectedSprintId !== '__all__'
  );

  function clearFilters() {
    query = '';
    roleFilter = null;
    statusFilter = null;
    projectFilter = null;
    updateJiraFilters(p.instanceId, { sprintIds: [] });
    // Reset GLOBAL assignee to default ("Me") — same as picking "Me" in
    // the user picker modal.
    if (assigneeActive) void selectAssignee(null);
  }

  function clickItem(it: JiraItem, e: MouseEvent) {
    if (!p.isClickNotDrag(e)) return;
    inboxState.jiraFocusKey = it.key;
    p.onNavigate?.();
  }

  /* Right-click context menu — same pattern as GithubList. Holds the
     coordinates + the row's JiraItem so action closures can capture
     the item independent of subsequent renders. */
  let ctxCoords = $state<{ x: number; y: number } | null>(null);
  let ctxItem = $state<JiraItem | null>(null);
  function openCtxMenu(e: MouseEvent, it: JiraItem) {
    e.preventDefault();
    e.stopPropagation();
    ctxCoords = { x: e.clientX, y: e.clientY };
    ctxItem = it;
  }
  function closeCtxMenu() {
    ctxCoords = null;
    ctxItem = null;
  }
  const ctxItems = $derived.by<MenuItem[]>(() => {
    const it = ctxItem;
    if (!it) return [];
    return [
      {
        label: 'Send to Claude',
        icon: 'M22 2 11 13 M22 2l-7 20-4-9-9-4 20-7z',
        onClick: () => p.onSendToClaude(it)
      },
      {
        label: 'Open in browser',
        icon: 'M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6 M15 3h6v6 M10 14L21 3',
        onClick: () => p.onOpenBrowser(it.url)
      },
      {
        label: 'Copy key',
        icon: 'M16 4h2a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h2 M9 2h6a1 1 0 0 1 1 1v2H8V3a1 1 0 0 1 1-1z',
        onClick: async () => {
          try { await navigator.clipboard.writeText(it.key); }
          catch (e) { console.warn('clipboard', e); }
        }
      },
      {
        label: 'Copy URL',
        icon: 'M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.72 M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71',
        onClick: async () => {
          try { await navigator.clipboard.writeText(it.url); }
          catch (e) { console.warn('clipboard', e); }
        }
      }
    ];
  });

  /** Map Jira status → shared `.state-pill .tag--*` token (src/app.css).
   *  Done → done (green); indeterminate → inreview when the status text
   *  mentions review, else inprogress (gold); everything else → todo. */
  function statusPillClass(it: JiraItem): string {
    if (it.status_category === 'done') return 'tag--done';
    if (it.status_category === 'indeterminate') {
      return (it.status || '').toLowerCase().includes('review') ? 'tag--inreview' : 'tag--inprogress';
    }
    return 'tag--todo';
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

<aside class="lp jrl">
  <header class="lp-head">
    <span class="lp-title">Tickets</span>
    {#if filtered.length > 0}<span class="lp-count">{filtered.length}</span>{/if}
    <span class="lp-head-spring"></span>
    <button class="lp-add" onclick={p.onOpenCreateIssue} title="New issue" aria-label="New issue" disabled={p.jiraStatus.kind !== 'connected'}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round"><path d="M12 5v14M5 12h14"/></svg>
    </button>
    <button class="lp-ghostbtn" onclick={p.onRefresh} title="Refresh" aria-label="Refresh" disabled={loading}>
      <svg class:spin={loading} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"><path d="M21 12a9 9 0 1 1-3-6.7"/><path d="M21 4v5h-5"/></svg>
    </button>
  </header>

    <label class="jrl-search" bind:this={searchEl}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"><circle cx="11" cy="11" r="8"/><path d="m21 21-4.3-4.3"/></svg>
      <input
        type="text"
        placeholder="Search summary, KEY-123, assignee…"
        bind:value={query}
        spellcheck="false"
        onkeydown={handleSearchKeydown}
        onfocus={openPicker}
      />
      {#if query}
        <button class="jrl-search-clear" onclick={() => (query = '')} aria-label="Clear search">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>
        </button>
      {/if}
    </label>
    <ListSearchPicker
      bind:this={pickerEl}
      anchor={searchEl}
      open={pickerOpen}
      rows={pickerRows}
      source="jira"
      onPick={pickJira}
      onClose={closePicker}
    />
    <div class="lp-chips">
      <button class="lp-chip" class:active={roleFilter === 'reporter'} disabled={!me} onclick={() => toggleRole('reporter')} title="Reported by me">Reporter</button>
      <button class="lp-chip" class:active={statusFilter === 'open'} onclick={() => toggleStatus('open')} title="Open / triage">Open</button>
      <button class="lp-chip" class:active={statusFilter === 'inprogress'} onclick={() => toggleStatus('inprogress')} title="In progress">In progress</button>
      <button class="lp-chip" class:active={statusFilter === 'done'} onclick={() => toggleStatus('done')} title="Done">Done</button>

      <span class="jrl-dd">
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
      <button
        class="lp-chip"
        class:active={assigneeActive}
        disabled={p.jiraStatus.kind !== 'connected'}
        onclick={openUserPicker}
        title="Filter by assignee"
      >{assigneeLabel}</button>
      <span class="jrl-dd">
        <Dropdown
          value={selectedSprintId}
          options={sprintOptions}
          onChange={selectSprint}
          placeholder="Sprint"
          ariaLabel="Filter by sprint"
          variant="chip"
          compact
        />
      </span>

      {#if anyFilterActive}
        <button class="jrl-chip-clear" onclick={clearFilters} title="Clear all filters">Clear</button>
      {/if}
    </div>

  <div class="lp-list">
    {#if error}
      <div class="jrl-error">
        <p class="jrl-error-h serif">Couldn't load Jira</p>
        <p class="jrl-error-p mono">{error}</p>
        <button class="jrl-error-retry" onclick={p.onRefresh}>Retry</button>
      </div>
    {:else if loading && items.length === 0}
      <div class="jrl-loading">
        <div class="jrl-spinner"></div>
        <span class="mono">Loading…</span>
      </div>
    {:else if items.length === 0}
      <div class="jrl-empty">
        <p class="jrl-empty-h serif">Inbox is empty</p>
        <p class="jrl-empty-p">No tickets yet. Create one or refresh.</p>
      </div>
    {:else if filtered.length === 0}
      <div class="jrl-empty">
        <p class="jrl-empty-h serif">No matches</p>
        <p class="jrl-empty-p">No tickets match the current search and filters.</p>
        <button class="jrl-error-retry" onclick={clearFilters}>Clear filters</button>
      </div>
    {:else}
      {#each groups as g (g.label)}
        <div class="lp-group-label">{g.label} · {g.items.length}</div>
        {#each g.items as it (it.key)}
          {@const isActive = inboxState.jiraFocusKey === it.key}
          <button
            class="lp-row jrl-row"
            class:active={isActive}
            draggable="true"
            onpointerdown={p.onCardMouseDown}
            ondragstart={(e) => p.onDragStart({ source: 'jira', item: it }, e)}
            ondragend={p.onDragEnd}
            onclick={(e) => clickItem(it, e)}
            ondblclick={() => p.onOpenBrowser(it.url)}
            oncontextmenu={(e) => openCtxMenu(e, it)}
          >
            <div class="jrl-top">
              <span class="state-pill {statusPillClass(it)}">{it.status}</span>
              <span class="jrl-key">{it.key}</span>
              <span class="jrl-type {typeClass(it.issue_type)}">{it.issue_type}</span>
              <span class="jrl-time">{relativeTime(it.updated, p.now)}</span>
            </div>
            <div class="jrl-title">{it.summary}</div>
            <div class="jrl-meta">
              {#if it.priority}
                <span class="jrl-pri {priorityClass(it.priority)}">{it.priority}</span>
              {/if}
              {#if it.labels.length > 0}
                {#each it.labels.slice(0, 2) as l (l)}
                  <span class="jrl-label">{l}</span>
                {/each}
                {#if it.labels.length > 2}
                  <span class="jrl-label-more">+{it.labels.length - 2}</span>
                {/if}
              {/if}
              {#if it.assignee}
                <span class="jrl-ava" title={it.assignee.display_name}>
                  {#if it.assignee.avatar_url}
                    <img src={it.assignee.avatar_url} alt={it.assignee.display_name} loading="lazy" />
                  {:else}
                    {(it.assignee.display_name || '?').slice(0, 1).toUpperCase()}
                  {/if}
                </span>
              {/if}
            </div>
            <span class="jrl-sends">
              <span
                class="jrl-send"
                role="button"
                tabindex="0"
                onclick={(e) => clickSendToClaude(it, e)}
                onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); clickSendToClaude(it, e as unknown as MouseEvent); } }}
                onpointerdown={(e) => e.stopPropagation()}
                title="Send to Claude"
                aria-label="Send to Claude"
              >→ claude</span>
            </span>
          </button>
        {/each}
      {/each}
    {/if}
  </div>
</aside>

<CardContextMenu coords={ctxCoords} items={ctxItems} onClose={closeCtxMenu} />

<style>
  /* Width comes from the parent `app-shell` grid track. */
  .jrl { min-width: 0; }
  .jrl .spin { animation: jrl-spin 0.9s linear infinite; }
  @keyframes jrl-spin { to { transform: rotate(360deg); } }
  .lp-add:disabled { opacity: 0.5; cursor: not-allowed; }
  .lp-ghostbtn:disabled { opacity: 0.5; cursor: not-allowed; }
  .lp-chip:disabled { opacity: 0.45; cursor: not-allowed; }

  /* Search field — §2.4: h30 r8, bg-0 (light bg-2), border --border,
     text 12 faint. Wrapper keeps the magnifier + clear + picker anchor. */
  .jrl-search {
    position: relative;
    display: flex; align-items: center; gap: 6px;
    margin: 0 14px 8px;
    padding: 0 10px;
    height: 30px;
    border-radius: 8px;
    background: var(--bg-0);
    border: 1px solid var(--border);
    transition: border-color 120ms;
  }
  :root[data-theme='light'] .jrl-search { background: var(--bg-2); }
  .jrl-search:focus-within { border-color: var(--border-hi2, var(--border-hi)); }
  .jrl-search > svg { width: 12px; height: 12px; color: var(--text-mute); flex-shrink: 0; }
  .jrl-search input {
    flex: 1; min-width: 0;
    background: transparent; border: 0; outline: 0;
    color: var(--text-0); font-size: 12px; font-family: inherit;
  }
  .jrl-search input::placeholder { color: var(--text-faint); }
  .jrl-search-clear {
    width: 16px; height: 16px;
    display: grid; place-items: center;
    border: 0; background: transparent; color: var(--text-mute);
    cursor: pointer; border-radius: 4px;
  }
  .jrl-search-clear:hover { color: var(--text-0); background: var(--bg-3); }
  .jrl-search-clear svg { width: 10px; height: 10px; }

  /* Filter dropdowns — align the shared Dropdown trigger to the §2.4
     chip scale (pad 3/9, r6, 11px, --text-mute; active = border-hi2 +
     accent-soft + text-0). Kept identical to GithubList for consistency. */
  .jrl-dd { display: inline-flex; }
  .jrl-dd :global(.dd-trigger) {
    border-radius: 6px;
    border: 1px solid var(--border);
    background: transparent;
    color: var(--text-mute);
    font-size: 11px;
    height: auto;
    padding: 3px 9px;
    transition: color 120ms, border-color 120ms, background 120ms;
  }
  .jrl-dd :global(.dd-trigger:hover) { color: var(--text-1); }
  .jrl-dd :global(.dd-trigger[aria-expanded="true"]) {
    border-color: var(--border-hi2, var(--border-hi));
    background: var(--accent-soft);
    color: var(--text-0);
  }

  .jrl-chip-clear {
    padding: 3px 9px;
    background: transparent;
    border: 1px dashed var(--border-hi);
    border-radius: 6px;
    font-size: 11px;
    color: var(--text-mute);
    cursor: pointer;
    margin-left: auto;
    transition: color 120ms, border-color 120ms;
  }
  .jrl-chip-clear:hover { color: var(--text-0); border-color: var(--text-mute); border-style: solid; }

  /* Row = shared .lp-row shell; .jrl-row adds the vertical stack. */
  .jrl-row {
    position: relative;
    display: flex; flex-direction: column; gap: 5px;
    user-select: none;
  }

  .jrl-top { display: flex; align-items: center; gap: 7px; flex-wrap: wrap; }
  .jrl-key {
    font-size: 11px; color: var(--src-jira); font-weight: 600;
    font-family: var(--font-mono);
  }
  .jrl-type {
    display: inline-flex; align-items: center;
    padding: 1px 6px;
    border-radius: 4px;
    font-size: 10px; font-weight: 500;
    color: var(--text-1);
    background: var(--bg-3);
    border: 1px solid var(--border);
    text-transform: capitalize;
  }
  .jrl-type.type--bug { color: var(--err); border-color: var(--err-border); background: transparent; }
  .jrl-type.type--story { color: var(--ok); border-color: var(--ok-border); background: transparent; }
  .jrl-type.type--epic { color: var(--src-sentry); border-color: var(--src-sentry-border); background: transparent; }
  .jrl-type.type--task {
    color: var(--src-jira-2);
    border-color: color-mix(in srgb, var(--src-jira-2) 22%, transparent);
    background: color-mix(in srgb, var(--src-jira-2) 6%, transparent);
  }
  .jrl-time {
    margin-left: auto;
    font-size: 11px; color: var(--text-faint);
    font-family: var(--font-mono);
  }

  /* Title 13 --text-1; active row → 600 --text-0 (§2.4). */
  .jrl-title {
    font-size: 13px; color: var(--text-1);
    line-height: 1.4;
    display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical;
    overflow: hidden;
  }
  .lp-row.active .jrl-title { color: var(--text-0); font-weight: 600; }

  .jrl-meta {
    display: flex; align-items: center; gap: 5px; flex-wrap: wrap;
    font-size: 11px; color: var(--text-2);
  }

  .jrl-pri {
    display: inline-flex; align-items: center; gap: 3px;
    padding: 1px 6px;
    border-radius: 4px;
    font-size: 10px; font-weight: 500;
    text-transform: capitalize;
  }
  .jrl-pri.pri--high { color: var(--err); background: transparent; border: 1px solid var(--err-border); }
  .jrl-pri.pri--med  { color: var(--warn); background: transparent; border: 1px solid var(--warn-border); }
  .jrl-pri.pri--low  { color: var(--info); background: transparent; border: 1px solid var(--border-hi); }

  .jrl-label {
    padding: 1px 5px;
    border-radius: 3px;
    font-size: 9.5px; color: var(--text-mute);
    background: var(--bg-2);
    border: 1px solid var(--border);
    font-family: var(--font-mono);
    max-width: 100px;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .jrl-label-more { font-size: 10px; color: var(--text-faint); font-family: var(--font-mono); }

  .jrl-ava {
    width: 16px; height: 16px;
    border-radius: 50%;
    background: linear-gradient(135deg, var(--accent-bright), var(--accent-deep));
    color: var(--accent-fg);
    font-size: 9px; font-weight: 700;
    display: grid; place-items: center;
    overflow: hidden;
    flex-shrink: 0;
  }
  .jrl-ava img { width: 100%; height: 100%; object-fit: cover; }

  /* Hover action — §2.4: dotted-underline 11px on the right, revealed
     on row hover/focus/active. role="button" span (a nested <button>
     would be invalid inside the row's own <button>). */
  .jrl-sends {
    position: absolute;
    top: 10px; right: 12px;
    display: inline-flex; gap: 10px;
    opacity: 0;
    transition: opacity 140ms;
  }
  .jrl-row:hover .jrl-sends,
  .jrl-sends:focus-within,
  .lp-row.active .jrl-sends {
    opacity: 1;
  }
  .jrl-send {
    font-size: 11px;
    color: var(--text-mute);
    text-decoration: underline dotted;
    text-underline-offset: 2px;
    cursor: pointer;
    user-select: none;
    background: transparent; border: 0; padding: 0;
  }
  .jrl-send:hover { color: var(--text-0); }

  .jrl-empty, .jrl-loading, .jrl-error {
    text-align: center;
    padding: 50px 20px;
    margin: auto;
  }
  .jrl-empty-h, .jrl-error-h {
    font-family: var(--font-mono);
    font-size: 22px; font-weight: 600; letter-spacing: -0.015em;
    color: var(--text-0);
    margin: 0 0 10px;
  }
  .jrl-empty-p, .jrl-error-p {
    font-size: 12.5px; color: var(--text-2);
    line-height: 1.55; margin: 0;
  }
  .jrl-error-p { color: var(--error); }
  .jrl-error-retry {
    margin-top: 14px;
    padding: 6px 12px;
    border-radius: 7px;
    font-size: 12px;
    background: var(--bg-2);
    border: 1px solid var(--border-hi);
    color: var(--text-1);
    cursor: pointer;
  }
  .jrl-error-retry:hover { color: var(--text-0); }
  .jrl-loading {
    display: flex; align-items: center; justify-content: center; gap: 10px;
    color: var(--text-2); font-size: 12px;
  }
  .jrl-spinner {
    width: 14px; height: 14px;
    border: 1.5px solid var(--border-hi);
    border-top-color: var(--accent);
    border-radius: 50%;
    animation: jrl-spin 0.7s linear infinite;
  }
</style>
