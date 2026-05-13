<script lang="ts">
  /* SentryList — левая панель SentryApp. Standalone: читает inbox
     state, рендерит фильтр-row + groups (Surging / Unresolved /
     Resolved) + item cards. Click → inboxState.sentryFocusId. */
  import {
    inboxState,
    sentryItemsFor,
    sentryItemsLoadingFor,
    sentryItemsErrorFor,
    openSentryFocus
  } from '$lib/state/inbox.svelte';
  import { relativeTime, sentryLevelClass, type SentryIssue, type SentryStatus } from '$lib/data';
  import Dropdown from '$lib/components/ui/Dropdown.svelte';
  import ListSearchPicker from '$lib/views/apps/_shared/ListSearchPicker.svelte';
  import { invoke } from '@tauri-apps/api/core';

  interface Props {
    instanceId: string;
    sentryStatus: SentryStatus;
    now: number;
    onOpenBrowser: (url: string) => void;
    onDragStart: (payload: import('$lib/state/drag.svelte').DragPayload, e: DragEvent) => void;
    onDragEnd: () => void;
    onCardMouseDown: (e: MouseEvent) => void;
    isClickNotDrag: (e: MouseEvent) => boolean;
    /** One-click handoff to Claude / Cursor — equivalent to dragging
     *  the row onto the matching rail icon, but for users who prefer
     *  a button over a drag gesture. Shown on each issue card. */
    onSendToClaude: (item: SentryIssue) => void;
    onSendToCursor: (item: SentryIssue) => void;
  }
  let p: Props = $props();

  function clickSendToClaude(it: SentryIssue, e: MouseEvent) {
    e.stopPropagation();
    p.onSendToClaude(it);
  }
  function clickSendToCursor(it: SentryIssue, e: MouseEvent) {
    e.stopPropagation();
    p.onSendToCursor(it);
  }

  const items = $derived(sentryItemsFor(p.instanceId));
  const loading = $derived(sentryItemsLoadingFor(p.instanceId));
  const error = $derived(sentryItemsErrorFor(p.instanceId));

  /** Search + filter state. In-memory; resets on refresh. */
  let query = $state('');
  let levelFilter = $state<'fatal' | 'error' | 'warning' | 'info' | null>(null);
  let statusFilter = $state<'unresolved' | 'resolved' | 'ignored' | null>(null);
  let projectFilter = $state<string | null>(null);

  /** Unique project slugs in the current items, prepended with the
   *  "All projects" sentinel option for the dropdown. */
  const projectOptions = $derived.by(() => {
    const set = new Set<string>();
    for (const it of items) {
      if (it.project_slug) set.add(it.project_slug);
    }
    const list = Array.from(set).sort();
    return [{ value: '__all__' as string, label: 'All projects' }, ...list.map((v) => ({ value: v, label: v }))];
  });

  function toggleLevel(v: 'fatal' | 'error' | 'warning' | 'info') {
    levelFilter = levelFilter === v ? null : v;
  }
  function toggleStatus(v: 'unresolved' | 'resolved' | 'ignored') {
    statusFilter = statusFilter === v ? null : v;
  }

  const filtered = $derived.by(() => {
    const q = query.trim().toLowerCase();
    return items.filter((it) => {
      if (q) {
        const titleMatch = it.title.toLowerCase().includes(q);
        const idMatch = it.short_id.toLowerCase().includes(q);
        const culpritMatch = it.culprit?.toLowerCase().includes(q) ?? false;
        const projMatch = it.project_slug?.toLowerCase().includes(q) ?? false;
        if (!titleMatch && !idMatch && !culpritMatch && !projMatch) return false;
      }
      if (levelFilter && it.level !== levelFilter) return false;
      if (statusFilter && it.status !== statusFilter) return false;
      if (projectFilter && it.project_slug !== projectFilter) return false;
      return true;
    });
  });

  /** Group: Surging (high count + recent) / Unresolved / Resolved. */
  const groups = $derived.by(() => {
    const surging: SentryIssue[] = [];
    const unresolved: SentryIssue[] = [];
    const resolved: SentryIssue[] = [];
    const dayMs = 24 * 60 * 60 * 1000;
    for (const it of filtered) {
      if (it.status === 'resolved') {
        resolved.push(it);
        continue;
      }
      const last = new Date(it.last_seen).getTime();
      const recent = p.now - last < dayMs;
      const high = parseInt(it.count, 10) > 100;
      if (recent && high) surging.push(it);
      else unresolved.push(it);
    }
    return [
      { label: 'Surging', items: surging },
      { label: 'Unresolved', items: unresolved },
      { label: 'Resolved', items: resolved }
    ].filter((g) => g.items.length > 0);
  });

  const anyFilterActive = $derived(
    query.trim().length > 0 ||
    levelFilter !== null ||
    statusFilter !== null ||
    projectFilter !== null
  );

  function clearFilters() {
    query = '';
    levelFilter = null;
    statusFilter = null;
    projectFilter = null;
  }

  function clickItem(it: SentryIssue, e: MouseEvent) {
    if (!p.isClickNotDrag(e)) return;
    openSentryFocus(it.id);
  }

  /* ─── Search picker (server-side) ───────────────────────────────
     Hits `sentry_list_issues` directly with the user's query each
     keystroke (debounced 250ms), ignoring the inline list's level /
     status / project chips. The picker is a quick-jump to any issue,
     not a filtered slice of what's currently in the column. */
  let searchEl = $state<HTMLLabelElement | null>(null);
  let pickerEl = $state<{ handleKey: (e: KeyboardEvent) => boolean } | null>(null);
  let pickerOpen = $state(false);

  let pickerRemoteItems = $state<SentryIssue[] | null>(null);
  let pickerLastQuery = '';
  let pickerSearchTimer: ReturnType<typeof setTimeout> | null = null;

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
      try {
        const res = await invoke<SentryIssue[]>('sentry_list_issues', {
          query: q,
          limit: 25
        });
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
    const ranked = remote.map((it) => {
      const shortL = it.short_id.toLowerCase();
      const titleL = it.title.toLowerCase();
      const projectL = it.project_slug.toLowerCase();
      let rank = 5;
      if (shortL === q) rank = 0;
      else if (shortL.startsWith(q)) rank = 1;
      else if (titleL.includes(q)) rank = 2;
      else if (shortL.includes(q)) rank = 3;
      else if (projectL.includes(q)) rank = 4;
      return {
        id: String(it.id),
        title: it.title,
        sub: `${it.short_id} · ${it.project_slug}`,
        rank
      };
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
    pickerOpen = query.trim().length > 0;
  });

  function pickIssue(id: string) {
    openSentryFocus(id);
    query = '';
    pickerOpen = false;
  }

  function handleSearchKeydown(e: KeyboardEvent) {
    if (pickerEl && pickerEl.handleKey(e)) return;
  }
</script>

<aside class="sl app-pane">
  <header class="sl-head">
    <h1 class="app-pane-head-h">Sentry</h1>
    {#if p.sentryStatus.kind === 'connected'}
      <span class="app-pane-head-meta">{p.sentryStatus.user.organization_slug}</span>
    {/if}
    <span class="app-pane-head-spacer"></span>
    <button class="sl-icon" disabled={loading} title="Refresh disabled — Sentry items pull on poll">
      <svg class:spin={loading} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"><path d="M21 12a9 9 0 1 1-3-6.7"/><path d="M21 4v5h-5"/></svg>
    </button>
  </header>

  <div class="sl-filters">
    <label class="sl-search" bind:this={searchEl}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"><circle cx="11" cy="11" r="8"/><path d="m21 21-4.3-4.3"/></svg>
      <input
        type="text"
        placeholder="Search title, short-id, project…"
        bind:value={query}
        spellcheck="false"
        onkeydown={handleSearchKeydown}
        onfocus={openPicker}
      />
      {#if query}
        <button class="sl-search-clear" onclick={() => (query = '')} aria-label="Clear search">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>
        </button>
      {/if}
    </label>
    <ListSearchPicker
      bind:this={pickerEl}
      anchor={searchEl}
      open={pickerOpen}
      rows={pickerRows}
      source="sentry"
      onPick={pickIssue}
      onClose={closePicker}
    />
    <div class="sl-chips">
      <button class="sl-toggle" class:active={levelFilter === 'fatal'} onclick={() => toggleLevel('fatal')}>
        <span class="sl-toggle-dot"></span>
        Fatal
      </button>
      <button class="sl-toggle" class:active={levelFilter === 'error'} onclick={() => toggleLevel('error')}>
        <span class="sl-toggle-dot"></span>
        Error
      </button>
      <button class="sl-toggle" class:active={levelFilter === 'warning'} onclick={() => toggleLevel('warning')}>
        <span class="sl-toggle-dot"></span>
        Warn
      </button>
      <button class="sl-toggle" class:active={levelFilter === 'info'} onclick={() => toggleLevel('info')}>
        <span class="sl-toggle-dot"></span>
        Info
      </button>
      <span class="sl-divider" aria-hidden="true"></span>
      <button class="sl-toggle" class:active={statusFilter === 'unresolved'} onclick={() => toggleStatus('unresolved')}>
        <span class="sl-toggle-dot"></span>
        Unresolved
      </button>
      <button class="sl-toggle" class:active={statusFilter === 'resolved'} onclick={() => toggleStatus('resolved')}>
        <span class="sl-toggle-dot"></span>
        Resolved
      </button>
      <button class="sl-toggle" class:active={statusFilter === 'ignored'} onclick={() => toggleStatus('ignored')}>
        <span class="sl-toggle-dot"></span>
        Ignored
      </button>
      <span class="sl-divider" aria-hidden="true"></span>
      <span class="sl-dd">
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
      {#if anyFilterActive}
        <button class="sl-chip-clear" onclick={clearFilters} title="Clear all filters">Clear</button>
      {/if}
    </div>
  </div>

  <div class="sl-body">
    {#if error}
      <div class="sl-error">
        <p class="sl-error-h serif">Couldn't load Sentry</p>
        <p class="sl-error-p mono">{error}</p>
      </div>
    {:else if loading && items.length === 0}
      <div class="sl-loading">
        <div class="sl-spinner"></div>
        <span class="mono">Loading…</span>
      </div>
    {:else if items.length === 0}
      <div class="sl-empty">
        <p class="sl-empty-h serif">No issues</p>
        <p class="sl-empty-p">No Sentry issues yet.</p>
      </div>
    {:else if filtered.length === 0}
      <div class="sl-empty">
        <p class="sl-empty-h serif">No matches</p>
        <p class="sl-empty-p">No issues match the current search and filters.</p>
        <button class="sl-error-retry" onclick={clearFilters}>Clear filters</button>
      </div>
    {:else}
      {#each groups as g (g.label)}
        <div class="sl-group">{g.label} <span class="mono">·</span> {g.items.length}</div>
        {#each g.items as it (it.id)}
          {@const isActive = inboxState.sentryFocusId === it.id}
          <button
            class="sl-card"
            class:active={isActive}
            draggable="true"
            onpointerdown={p.onCardMouseDown}
            ondragstart={(e) => p.onDragStart({ source: 'sentry', item: it }, e)}
            ondragend={p.onDragEnd}
            onclick={(e) => clickItem(it, e)}
            ondblclick={() => p.onOpenBrowser(it.permalink)}
          >
            <div class="sl-card-top">
              <span class="sl-level {sentryLevelClass(it.level)}">{it.level}</span>
              <span class="sl-card-id mono">{it.short_id}</span>
              <span class="sl-card-time mono">{relativeTime(it.last_seen, p.now)}</span>
            </div>
            <div class="sl-card-title">{it.title}</div>
            <div class="sl-card-meta">
              <span class="sl-count mono" title={`${it.count} events`}>
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7"><path d="M3 12a9 9 0 0 1 18 0M3 12a9 9 0 0 0 18 0"/></svg>
                {it.count}
              </span>
              <span class="sl-users mono" title={`${it.user_count} users`}>
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7"><path d="M17 21v-2a4 4 0 0 0-4-4H5a4 4 0 0 0-4 4v2"/><circle cx="9" cy="7" r="4"/></svg>
                {it.user_count}
              </span>
              <span class="sl-project mono">{it.project_slug}</span>
              {#if it.status === 'resolved'}<span class="sl-status sl-status--resolved">resolved</span>{/if}
              {#if it.status === 'ignored'}<span class="sl-status sl-status--ignored">ignored</span>{/if}
            </div>
            <span class="sl-card-sends">
              <span
                class="sl-card-send sl-card-send--claude"
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
                class="sl-card-send sl-card-send--cursor"
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
  .sl {
    /* Width comes from the parent `app-shell` grid track. */
    min-width: 0;
  }
  .sl-head {
    flex: 0 0 46px;
    display: flex; align-items: center; gap: 10px;
    padding: 0 12px;
    border-bottom: 1px solid var(--border);
    background: linear-gradient(180deg, var(--bg-2), var(--bg-1));
  }
  .sl-icon {
    margin-left: auto;
    width: 26px; height: 26px;
    display: grid; place-items: center;
    color: var(--text-2);
    background: transparent; border: none; cursor: pointer;
    border-radius: 6px;
  }
  .sl-icon:hover:not(:disabled) { color: var(--text-0); background: var(--bg-elev, var(--bg-2)); }
  .sl-icon svg { width: 13px; height: 13px; }
  .sl-icon .spin { animation: sl-spin 0.9s linear infinite; }
  @keyframes sl-spin { to { transform: rotate(360deg); } }

  /* Filter bar — search + level/status chips + project select. */
  .sl-filters {
    flex: 0 0 auto;
    display: flex; flex-direction: column;
    gap: 6px;
    padding: 8px 12px 10px;
    border-bottom: 1px solid var(--border);
    background: var(--bg-1);
  }
  .sl-search {
    position: relative;
    display: flex; align-items: center; gap: 6px;
    padding: 0 8px;
    height: 28px;
    border-radius: 6px;
    background: var(--bg-2);
    border: 1px solid var(--border);
    transition: border-color 120ms;
  }
  .sl-search:focus-within {
    border-color: var(--border-accent);
    background: var(--bg-1);
  }
  .sl-search > svg { width: 12px; height: 12px; color: var(--text-mute); flex-shrink: 0; }
  .sl-search input {
    flex: 1; min-width: 0;
    background: transparent; border: 0; outline: 0;
    color: var(--text-0); font-size: 12px; font-family: inherit;
  }
  .sl-search input::placeholder { color: var(--text-mute); }
  .sl-search-clear {
    width: 16px; height: 16px;
    display: grid; place-items: center;
    border: 0; background: transparent; color: var(--text-mute);
    cursor: pointer; border-radius: 4px;
  }
  .sl-search-clear:hover { color: var(--text-0); background: var(--bg-3); }
  .sl-search-clear svg { width: 10px; height: 10px; }

  .sl-chips { display: flex; gap: 6px; flex-wrap: wrap; align-items: center; }
  .sl-divider {
    width: 1px; height: 14px;
    background: var(--border);
    margin: 0 2px;
  }

  .sl-toggle {
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
  .sl-toggle-dot {
    width: 8px; height: 8px;
    border-radius: 50%;
    background: transparent;
    box-shadow: inset 0 0 0 1.5px var(--text-mute);
    transition: all 140ms;
    flex-shrink: 0;
  }
  .sl-toggle:hover:not(:disabled):not(.active) {
    color: var(--text-0); background: var(--bg-3); border-color: var(--border-hi);
  }
  .sl-toggle:hover:not(:disabled):not(.active) .sl-toggle-dot {
    box-shadow: inset 0 0 0 1.5px var(--text-1);
  }
  .sl-toggle.active {
    color: var(--accent-bright);
    background: color-mix(in srgb, var(--src-sentry) 14%, transparent);
    border-color: color-mix(in srgb, var(--src-sentry) 40%, transparent);
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--src-sentry) 8%, transparent);
  }
  .sl-toggle.active .sl-toggle-dot {
    background: var(--src-sentry);
    box-shadow:
      inset 0 0 0 1.5px var(--src-sentry),
      0 0 6px color-mix(in srgb, var(--src-sentry) 60%, transparent);
  }

  .sl-dd { display: inline-flex; }
  .sl-dd :global(.dd-trigger) {
    border-radius: 999px;
    border: 1px solid var(--border);
    background: var(--bg-2);
    color: var(--text-1);
    font-size: 11px;
    height: auto;
    padding: 4px 10px;
    transition: all 140ms;
  }
  .sl-dd :global(.dd-trigger:hover) {
    color: var(--text-0); background: var(--bg-3); border-color: var(--border-hi);
  }
  .sl-dd :global(.dd-trigger[aria-expanded="true"]) {
    color: var(--accent-bright);
    background: color-mix(in srgb, var(--src-sentry) 14%, transparent);
    border-color: color-mix(in srgb, var(--src-sentry) 40%, transparent);
  }

  .sl-chip-clear {
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
  .sl-chip-clear:hover { color: var(--text-0); border-color: var(--text-mute); border-style: solid; }

  .sl-error-retry {
    margin-top: 14px;
    padding: 6px 12px;
    border-radius: 7px;
    font-size: 12px;
    background: var(--bg-2);
    border: 1px solid var(--border-hi);
    color: var(--text-1);
    cursor: pointer;
  }
  .sl-error-retry:hover { color: var(--text-0); }

  .sl-body {
    flex: 1; min-height: 0;
    overflow-y: auto;
    padding: 4px 8px 12px;
  }
  .sl-group {
    padding: 14px 8px 6px;
    font-size: 9.5px; font-weight: 700;
    letter-spacing: 0.10em;
    text-transform: uppercase;
    color: var(--text-mute);
    font-family: 'JetBrains Mono', monospace;
    display: flex; align-items: center; gap: 8px;
  }
  .sl-group::after {
    content: ''; flex: 1; height: 1px;
    background: linear-gradient(90deg, var(--border), transparent);
  }
  .sl-group .mono { opacity: 0.5; }

  .sl-card {
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
  .sl-card::before {
    content: ''; position: absolute; left: 0; top: 11px; bottom: 11px;
    width: 2px; border-radius: 2px;
    background: var(--src-sentry);
    opacity: 0; transition: opacity 200ms;
  }
  .sl-card:hover { background: var(--bg-2); border-color: var(--border); }
  .sl-card:hover::before { opacity: 0.5; }
  .sl-card.active {
    background: var(--bg-2);
    border-color: var(--border-hi);
    /* Tint follows the brand var (`--error` is sentry's pulse colour
       on both themes); drop-shadow uses the theme's own `--shadow-2`
       so the lift reads on cream as well as on noir. */
    box-shadow:
      0 0 0 1px color-mix(in srgb, var(--error) 32%, transparent),
      var(--shadow-2);
  }
  .sl-card.active::before {
    opacity: 1;
    box-shadow: 0 0 10px color-mix(in srgb, var(--error) 40%, transparent);
  }

  .sl-card-top { display: flex; align-items: center; gap: 7px; }
  .sl-level {
    padding: 1px 6px;
    border-radius: 4px;
    font-size: 9.5px; font-weight: 700;
    text-transform: uppercase; letter-spacing: 0.04em;
    color: var(--text-1);
    background: var(--bg-3);
    border: 1px solid var(--border);
  }
  /* sentryLevelClass возвращает: tag--fatal | tag--error | tag--warning | tag--info | tag--debug */
  .sl-level.tag--fatal   { color: #F0A38A; background: rgba(232, 130, 100, 0.14); border-color: rgba(232, 130, 100, 0.32); }
  .sl-level.tag--error   { color: var(--error); background: rgba(232, 130, 100, 0.10); border-color: rgba(232, 130, 100, 0.24); }
  .sl-level.tag--warning { color: var(--warning); background: rgba(217, 184, 110, 0.10); border-color: rgba(217, 184, 110, 0.24); }
  .sl-level.tag--info    { color: var(--info); background: rgba(136, 194, 221, 0.08); border-color: rgba(136, 194, 221, 0.20); }
  .sl-level.tag--debug   { color: var(--text-mute); }

  .sl-card-id { font-size: 11px; color: var(--text-2); font-weight: 500; }
  .sl-card-time {
    margin-left: auto;
    font-size: 10px; color: var(--text-mute);
  }

  .sl-card-title {
    font-size: 13px; color: var(--text-0); font-weight: 500;
    line-height: 1.4;
    display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical;
    overflow: hidden;
    overflow-wrap: anywhere;
  }
  .sl-card-meta {
    display: flex; align-items: center; gap: 8px; flex-wrap: wrap;
    font-size: 10.5px; color: var(--text-2);
  }
  .sl-count, .sl-users {
    display: inline-flex; align-items: center; gap: 3px;
    font-size: 10px; color: var(--text-mute);
  }
  .sl-count svg, .sl-users svg { width: 10px; height: 10px; }

  .sl-project {
    padding: 1px 5px;
    border-radius: 3px;
    font-size: 9.5px; color: var(--text-mute);
    background: var(--bg-2);
    border: 1px solid var(--border);
  }
  .sl-status {
    padding: 1px 6px; border-radius: 3px;
    font-size: 9.5px; font-weight: 600;
    text-transform: uppercase; letter-spacing: 0.04em;
  }
  .sl-status--resolved { color: var(--success); background: rgba(168, 217, 184, 0.10); border: 1px solid rgba(168, 217, 184, 0.24); }
  .sl-status--ignored { color: var(--text-mute); background: var(--bg-3); border: 1px solid var(--border); }

  /* Send-to-{Claude,Cursor} chips — appear on hover/active in the
     top-right of the card. Two role="button" spans (a real <button>
     would be invalid HTML inside the row's <button>). */
  .sl-card-sends {
    position: absolute;
    top: 8px; right: 10px;
    display: inline-flex; gap: 4px;
    opacity: 0;
    transition: opacity 140ms;
  }
  .sl-card:hover .sl-card-sends,
  .sl-card-sends:focus-within,
  .sl-card.active .sl-card-sends {
    opacity: 1;
  }
  .sl-card-send {
    display: inline-flex; align-items: center; gap: 4px;
    padding: 3px 8px 3px 7px;
    border-radius: 5px;
    font-size: 10px; font-weight: 600;
    cursor: pointer;
    transition: background 140ms, transform 140ms;
    user-select: none;
  }
  .sl-card-send svg { width: 11px; height: 11px; }
  .sl-card-send--claude {
    color: var(--src-claude);
    background: color-mix(in srgb, var(--src-claude) 12%, transparent);
    border: 1px solid color-mix(in srgb, var(--src-claude) 28%, transparent);
  }
  .sl-card-send--claude:hover {
    background: color-mix(in srgb, var(--src-claude) 22%, transparent);
    color: var(--accent-bright);
    transform: translateY(-1px);
  }
  .sl-card-send--cursor {
    color: var(--src-cursor, var(--text-1));
    background: color-mix(in srgb, var(--src-cursor, var(--text-1)) 14%, transparent);
    border: 1px solid color-mix(in srgb, var(--src-cursor, var(--text-1)) 32%, transparent);
  }
  .sl-card-send--cursor:hover {
    background: color-mix(in srgb, var(--src-cursor, var(--text-1)) 24%, transparent);
    color: var(--text-0);
    transform: translateY(-1px);
  }
  .sl-card-send:active { transform: translateY(0); }

  .sl-empty, .sl-loading, .sl-error {
    text-align: center;
    padding: 50px 20px;
    margin: auto;
  }
  .sl-empty-h, .sl-error-h {
    font-family: 'Geist', 'Inter', -apple-system, system-ui, sans-serif;
    font-size: 22px; font-weight: 600; letter-spacing: -0.015em;
    color: var(--text-0);
    margin: 0 0 10px;
  }
  .sl-empty-p, .sl-error-p {
    font-size: 12.5px; color: var(--text-2);
    line-height: 1.55; margin: 0;
  }
  .sl-error-p { color: var(--error); }
  .sl-loading {
    display: flex; align-items: center; justify-content: center; gap: 10px;
    color: var(--text-2); font-size: 12px;
  }
  .sl-spinner {
    width: 14px; height: 14px;
    border: 1.5px solid var(--border-hi);
    border-top-color: var(--accent);
    border-radius: 50%;
    animation: sl-spin 0.7s linear infinite;
  }
</style>
