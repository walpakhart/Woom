<script lang="ts">
  /* SentryList — left panel of SentryApp. Standalone: reads inbox
     state, renders filter row + groups (Surging / Unresolved /
     Resolved) + item cards. Click → inboxState.sentryFocusId. */
  import {
    inboxState,
    sentryItemsFor,
    sentryItemsLoadingFor,
    sentryItemsErrorFor,
    openSentryFocus,
    sentryFiltersFor,
    persistSentryUiFilters
  } from '$lib/state/inbox.svelte';
  import { relativeTime, sentryLevelClass, type SentryIssue, type SentryStatus } from '$lib/data';
  import Dropdown from '$lib/components/ui/Dropdown.svelte';
  import ListSearchPicker from '$lib/views/apps/_shared/ListSearchPicker.svelte';
  import CardContextMenu, { type MenuItem } from '$lib/views/apps/_shared/CardContextMenu.svelte';
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
    /** One-click handoff to Claude — equivalent to dragging
     *  the row onto the rail icon, but for users who prefer
     *  a button over a drag gesture. Shown on each issue card. */
    onSendToClaude: (item: SentryIssue) => void;
    /** Fired after a row navigates (Quiet: closes the switcher popover). */
    onNavigate?: () => void;
  }
  let p: Props = $props();

  /** 24 hourly points → 10 sparkline buckets (mockup: 10 bars, 3-16px). */
  function sparkBars(stats: number[] | undefined): number[] {
    const pts = stats ?? [];
    if (pts.length === 0) return [];
    const buckets = 10;
    const per = Math.max(1, Math.ceil(pts.length / buckets));
    const out: number[] = [];
    for (let i = 0; i < pts.length; i += per) {
      out.push(pts.slice(i, i + per).reduce((a, b) => a + b, 0));
    }
    const max = Math.max(1, ...out);
    return out.map((v) => 3 + Math.round((v / max) * 13));
  }

  function clickSendToClaude(it: SentryIssue, e: MouseEvent) {
    e.stopPropagation();
    p.onSendToClaude(it);
  }

  const items = $derived(sentryItemsFor(p.instanceId));
  const loading = $derived(sentryItemsLoadingFor(p.instanceId));
  const error = $derived(sentryItemsErrorFor(p.instanceId));

  /** Search + filter state. Persisted per-instance via
   *  `persistSentryUiFilters` — survives solo switches + app
   *  restart. Init reads from the store; mutations bind to $state
   *  locally then a $effect mirrors the diff back. */
  const _init = sentryFiltersFor(p.instanceId);
  let query = $state(_init.uiQuery ?? '');
  let levelFilter = $state<'fatal' | 'error' | 'warning' | 'info' | null>(_init.uiLevelFilter ?? null);
  let statusFilter = $state<'unresolved' | 'resolved' | 'ignored' | null>(_init.uiStatusFilter ?? null);
  let projectFilter = $state<string | null>(_init.uiProjectFilter ?? null);

  $effect(() => {
    persistSentryUiFilters(p.instanceId, {
      uiQuery: query,
      uiLevelFilter: levelFilter,
      uiStatusFilter: statusFilter,
      uiProjectFilter: projectFilter
    });
  });

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
    p.onNavigate?.();
  }

  /* Right-click context menu — Send to Claude + Open + Copy.
     Status flip (resolve / ignore / unresolve) deferred — would need
     a `sentry_update_issue` Tauri command path through the agent
     `propose_*` channel; out of scope for the menu wiring round. */
  let ctxCoords = $state<{ x: number; y: number } | null>(null);
  let ctxItem = $state<SentryIssue | null>(null);
  function openCtxMenu(e: MouseEvent, it: SentryIssue) {
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
        onClick: () => p.onOpenBrowser(it.permalink)
      },
      {
        label: 'Copy short-id',
        icon: 'M16 4h2a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h2 M9 2h6a1 1 0 0 1 1 1v2H8V3a1 1 0 0 1 1-1z',
        onClick: async () => {
          try { await navigator.clipboard.writeText(it.short_id); }
          catch (e) { console.warn('clipboard', e); }
        }
      },
      {
        label: 'Copy URL',
        icon: 'M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.72 M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71',
        onClick: async () => {
          try { await navigator.clipboard.writeText(it.permalink); }
          catch (e) { console.warn('clipboard', e); }
        }
      }
    ];
  });

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
    p.onNavigate?.();
  }

  function handleSearchKeydown(e: KeyboardEvent) {
    if (pickerEl && pickerEl.handleKey(e)) return;
  }
</script>

<aside class="lp snl">
  <header class="lp-head">
    <span class="lp-title">Issues</span>
    {#if filtered.length > 0}<span class="lp-count">{filtered.length}</span>{/if}
    <span class="lp-head-spring"></span>
    <button class="lp-ghostbtn" disabled={loading} title="Refresh disabled — Sentry items pull on poll" aria-label="Refresh">
      <svg class:spin={loading} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"><path d="M21 12a9 9 0 1 1-3-6.7"/><path d="M21 4v5h-5"/></svg>
    </button>
  </header>

  <label class="snl-search" bind:this={searchEl}>
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
      <button class="snl-search-clear" onclick={() => (query = '')} aria-label="Clear search">
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
  <div class="lp-chips">
    <button class="lp-chip" class:active={levelFilter === 'fatal'} onclick={() => toggleLevel('fatal')}>Fatal</button>
    <button class="lp-chip" class:active={levelFilter === 'error'} onclick={() => toggleLevel('error')}>Error</button>
    <button class="lp-chip" class:active={levelFilter === 'warning'} onclick={() => toggleLevel('warning')}>Warn</button>
    <button class="lp-chip" class:active={levelFilter === 'info'} onclick={() => toggleLevel('info')}>Info</button>
    <button class="lp-chip" class:active={statusFilter === 'unresolved'} onclick={() => toggleStatus('unresolved')}>Unresolved</button>
    <button class="lp-chip" class:active={statusFilter === 'resolved'} onclick={() => toggleStatus('resolved')}>Resolved</button>
    <button class="lp-chip" class:active={statusFilter === 'ignored'} onclick={() => toggleStatus('ignored')}>Ignored</button>
    <span class="snl-dd">
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
      <button class="snl-chip-clear" onclick={clearFilters} title="Clear all filters">Clear</button>
    {/if}
  </div>

  <div class="lp-list">
    {#if error}
      <div class="snl-error">
        <p class="snl-error-h serif">Couldn't load Sentry</p>
        <p class="snl-error-p mono">{error}</p>
      </div>
    {:else if loading && items.length === 0}
      <div class="snl-loading">
        <div class="snl-spinner"></div>
        <span class="mono">Loading…</span>
      </div>
    {:else if items.length === 0}
      <div class="snl-empty">
        <p class="snl-empty-h serif">No issues</p>
        <p class="snl-empty-p">No Sentry issues yet.</p>
      </div>
    {:else if filtered.length === 0}
      <div class="snl-empty">
        <p class="snl-empty-h serif">No matches</p>
        <p class="snl-empty-p">No issues match the current search and filters.</p>
        <button class="snl-error-retry" onclick={clearFilters}>Clear filters</button>
      </div>
    {:else}
      {#each groups as g (g.label)}
        <div class="lp-group-label">{g.label} · {g.items.length}</div>
        {#each g.items as it (it.id)}
          {@const isActive = inboxState.sentryFocusId === it.id}
          <button
            class="lp-row snl-row"
            class:active={isActive}
            draggable="true"
            onpointerdown={p.onCardMouseDown}
            ondragstart={(e) => p.onDragStart({ source: 'sentry', item: it }, e)}
            ondragend={p.onDragEnd}
            onclick={(e) => clickItem(it, e)}
            ondblclick={() => p.onOpenBrowser(it.permalink)}
            oncontextmenu={(e) => openCtxMenu(e, it)}
          >
            <div class="snl-top">
              <span class="state-pill {sentryLevelClass(it.level)}">{it.level}</span>
              <span class="snl-id">{it.short_id}</span>
              <span class="snl-time">{relativeTime(it.last_seen, p.now)}</span>
            </div>
            <div class="snl-title">{it.title}</div>
            {#if sparkBars(it.stats_24h).length}
              <div class="snl-spark" class:snl-spark--hot={it.level === 'error' || it.level === 'fatal'} aria-hidden="true">
                {#each sparkBars(it.stats_24h) as h, i (i)}
                  <span class="snl-spark-bar" style:height="{h}px"></span>
                {/each}
              </div>
            {/if}
            <div class="snl-meta">
              <span class="snl-count" title={`${it.count} events`}>
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7"><path d="M3 12a9 9 0 0 1 18 0M3 12a9 9 0 0 0 18 0"/></svg>
                {it.count}
              </span>
              <span class="snl-users" title={`${it.user_count} users`}>
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7"><path d="M17 21v-2a4 4 0 0 0-4-4H5a4 4 0 0 0-4 4v2"/><circle cx="9" cy="7" r="4"/></svg>
                {it.user_count}
              </span>
              <span class="snl-project">{it.project_slug}</span>
              {#if it.status === 'resolved'}<span class="snl-status snl-status--resolved">resolved</span>{/if}
              {#if it.status === 'ignored'}<span class="snl-status snl-status--ignored">ignored</span>{/if}
            </div>
            <span class="snl-sends">
              <span
                class="snl-send"
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
  .snl { min-width: 0; }
  .snl .spin { animation: snl-spin 0.9s linear infinite; }
  @keyframes snl-spin { to { transform: rotate(360deg); } }
  .lp-ghostbtn:disabled { opacity: 0.5; cursor: not-allowed; }
  .lp-chip:disabled { opacity: 0.45; cursor: not-allowed; }

  /* Search field — §2.4: h30 r8, bg-0 (light bg-2), border --border,
     text 12 faint. Kept identical to Github/Jira lists for consistency. */
  .snl-search {
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
  :root[data-theme='light'] .snl-search { background: var(--bg-2); }
  .snl-search:focus-within { border-color: var(--border-hi2, var(--border-hi)); }
  .snl-search > svg { width: 12px; height: 12px; color: var(--text-mute); flex-shrink: 0; }
  .snl-search input {
    flex: 1; min-width: 0;
    background: transparent; border: 0; outline: 0;
    color: var(--text-0); font-size: 12px; font-family: inherit;
  }
  .snl-search input::placeholder { color: var(--text-faint); }
  .snl-search-clear {
    width: 16px; height: 16px;
    display: grid; place-items: center;
    border: 0; background: transparent; color: var(--text-mute);
    cursor: pointer; border-radius: 4px;
  }
  .snl-search-clear:hover { color: var(--text-0); background: var(--bg-3); }
  .snl-search-clear svg { width: 10px; height: 10px; }

  /* Filter dropdown — align the shared Dropdown trigger to the §2.4
     chip scale (pad 3/9, r6, 11px, --text-mute; active = border-hi2 +
     accent-soft + text-0). Kept identical to Github/Jira lists. */
  .snl-dd { display: inline-flex; }
  .snl-dd :global(.dd-trigger) {
    border-radius: 6px;
    border: 1px solid var(--border);
    background: transparent;
    color: var(--text-mute);
    font-size: 11px;
    height: auto;
    padding: 3px 9px;
    transition: color 120ms, border-color 120ms, background 120ms;
  }
  .snl-dd :global(.dd-trigger:hover) { color: var(--text-1); }
  .snl-dd :global(.dd-trigger[aria-expanded="true"]) {
    border-color: var(--border-hi2, var(--border-hi));
    background: var(--accent-soft);
    color: var(--text-0);
  }

  .snl-chip-clear {
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
  .snl-chip-clear:hover { color: var(--text-0); border-color: var(--text-mute); border-style: solid; }

  /* Row = shared .lp-row shell; .snl-row adds the vertical stack. */
  .snl-row {
    position: relative;
    display: flex; flex-direction: column; gap: 5px;
    user-select: none;
  }

  .snl-top { display: flex; align-items: center; gap: 7px; flex-wrap: wrap; }
  /* Level tag = shared .state-pill .tag--{fatal|error|warning|info};
     sentryLevelClass drives the modifier (tokens live in src/app.css). */
  .snl-id {
    font-size: 11px; color: var(--text-faint);
    font-family: var(--font-mono);
  }
  .snl-time {
    margin-left: auto;
    font-size: 11px; color: var(--text-faint);
    font-family: var(--font-mono);
  }

  /* Title 13 --text-1; active row → 600 --text-0 (§2.4). */
  .snl-title {
    font-size: 13px; color: var(--text-1);
    line-height: 1.4;
    display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical;
    overflow: hidden;
    overflow-wrap: anywhere;
  }
  .lp-row.active .snl-title { color: var(--text-0); font-weight: 600; }

  /* Sparkline — 10 bars, 5px wide, ink alpha; hot issues in err. */
  .snl-spark {
    display: flex; align-items: flex-end; gap: 2px;
    height: 16px;
  }
  .snl-spark-bar {
    width: 5px;
    border-radius: 1.5px;
    background: color-mix(in srgb, var(--text-0) 22%, transparent);
  }
  .snl-spark--hot .snl-spark-bar {
    background: color-mix(in srgb, var(--err) 45%, transparent);
  }

  .snl-meta {
    display: flex; align-items: center; gap: 8px; flex-wrap: wrap;
    font-size: 11px; color: var(--text-2);
  }
  .snl-count, .snl-users {
    display: inline-flex; align-items: center; gap: 3px;
    font-size: 11px; color: var(--text-faint);
    font-family: var(--font-mono);
  }
  .snl-count svg, .snl-users svg { width: 10px; height: 10px; }
  .snl-project {
    font-size: 11px; color: var(--text-faint);
    font-family: var(--font-mono);
    max-width: 120px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .snl-status {
    padding: 1px 6px; border-radius: 4px;
    font-size: 9.5px; font-weight: 600;
    text-transform: uppercase; letter-spacing: 0.04em;
  }
  .snl-status--resolved { color: var(--success); background: color-mix(in srgb, var(--ok) 10%, transparent); border: 1px solid color-mix(in srgb, var(--ok) 24%, transparent); }
  .snl-status--ignored { color: var(--text-mute); background: var(--bg-3); border: 1px solid var(--border); }

  /* Hover action — §2.4: dotted-underline 11px on the right, revealed
     on row hover/focus/active. role="button" span (a nested <button>
     would be invalid inside the row's own <button>). */
  .snl-sends {
    position: absolute;
    top: 10px; right: 12px;
    display: inline-flex; gap: 10px;
    opacity: 0;
    transition: opacity 140ms;
  }
  .snl-row:hover .snl-sends,
  .snl-sends:focus-within,
  .lp-row.active .snl-sends {
    opacity: 1;
  }
  .snl-send {
    font-size: 11px;
    color: var(--text-mute);
    text-decoration: underline dotted;
    text-underline-offset: 2px;
    cursor: pointer;
    user-select: none;
    background: transparent; border: 0; padding: 0;
  }
  .snl-send:hover { color: var(--text-0); }

  .snl-empty, .snl-loading, .snl-error {
    text-align: center;
    padding: 50px 20px;
    margin: auto;
  }
  .snl-empty-h, .snl-error-h {
    font-family: var(--font-mono);
    font-size: 22px; font-weight: 600; letter-spacing: -0.015em;
    color: var(--text-0);
    margin: 0 0 10px;
  }
  .snl-empty-p, .snl-error-p {
    font-size: 12.5px; color: var(--text-2);
    line-height: 1.55; margin: 0;
  }
  .snl-error-p { color: var(--error); }
  .snl-error-retry {
    margin-top: 14px;
    padding: 6px 12px;
    border-radius: 7px;
    font-size: 12px;
    background: var(--bg-2);
    border: 1px solid var(--border-hi);
    color: var(--text-1);
    cursor: pointer;
  }
  .snl-error-retry:hover { color: var(--text-0); }
  .snl-loading {
    display: flex; align-items: center; justify-content: center; gap: 10px;
    color: var(--text-2); font-size: 12px;
  }
  .snl-spinner {
    width: 14px; height: 14px;
    border: 1.5px solid var(--border-hi);
    border-top-color: var(--accent);
    border-radius: 50%;
    animation: snl-spin 0.7s linear infinite;
  }
</style>
