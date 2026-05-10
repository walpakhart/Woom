<script lang="ts">
  /* GithubList — левая панель GithubApp. Standalone: читает inbox
     state, рендерит PRs / Issues + filter row + groups + item cards.
     Click on item → selectInboxItem (sets inboxState.focusItem).
     Drag handlers для drop-в-Claude. */
  import {
    inboxState,
    githubItemsFor,
    githubLoadingFor,
    githubErrorFor
  } from '$lib/state/inbox.svelte';
  import { relativeTime, type InboxItem, type ConnectionStatus, type Repository } from '$lib/data';
  import Dropdown from '$lib/components/ui/Dropdown.svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';

  interface Props {
    instanceId: string;
    githubStatus: ConnectionStatus;
    now: number;
    onRefresh: () => void;
    onOpenCreatePr: () => void;
    onOpenBrowser: (url: string) => void;
    onSelect: (id: number) => void;
    onDragStart: (payload: { source: 'github'; item: InboxItem }, e: DragEvent) => void;
    onDragEnd: () => void;
    onCardMouseDown: (e: MouseEvent) => void;
    isClickNotDrag: (e: MouseEvent) => boolean;
    /** One-click handoff to Claude / Cursor — equivalent to dragging
     *  the row onto the matching rail icon, but for users who prefer
     *  a button over a drag gesture. Shown on each PR/issue card. */
    onSendToClaude: (item: InboxItem) => void;
    onSendToCursor: (item: InboxItem) => void;
  }
  let p: Props = $props();

  function clickSendToClaude(it: InboxItem, e: MouseEvent) {
    /* Don't let the row's onclick fire — sending should not also
       focus the item; the user has expressed a different intent. */
    e.stopPropagation();
    p.onSendToClaude(it);
  }
  function clickSendToCursor(it: InboxItem, e: MouseEvent) {
    e.stopPropagation();
    p.onSendToCursor(it);
  }

  const items = $derived(githubItemsFor(p.instanceId));
  const loading = $derived(githubLoadingFor(p.instanceId));
  const error = $derived(githubErrorFor(p.instanceId));

  /** Search + filter state. Kept local to the list (in-memory) so
   *  filtering doesn't survive a refresh — that's intentional;
   *  filters are an exploration tool, not a saved view. Each chip is
   *  a toggle (click again to deselect → "all"); dropdowns use a
   *  null sentinel for "any". */
  let query = $state('');
  let roleFilter = $state<'mine' | 'reviewer' | null>(null);
  let stateFilter = $state<'open' | 'draft' | null>(null);
  /** `null` = inbox scope (involves:@me, the default). `'__all_open__'`
   *  = drop the involves filter, search every accessible repo.
   *  Otherwise the literal `owner/name` of a specific repo. */
  let repoFilter = $state<string | null>(null);
  let authorFilter = $state<string | null>(null);

  const me = $derived(p.githubStatus.kind === 'connected' ? p.githubStatus.user.login : null);

  /** All repos the connected user has access to. Loaded once on
   *  mount via `github_list_repos`; refreshed when the connected
   *  account changes. Falls back to items-derived repos when this
   *  hasn't loaded yet (or fails). */
  let availableRepos = $state<Repository[]>([]);
  let availableReposLoading = $state(false);

  async function loadAvailableRepos() {
    if (p.githubStatus.kind !== 'connected') return;
    availableReposLoading = true;
    try {
      availableRepos = await invoke<Repository[]>('github_list_repos');
    } catch (e) {
      console.warn('github_list_repos failed:', e);
    } finally {
      availableReposLoading = false;
    }
  }
  onMount(() => { void loadAvailableRepos(); });
  /* Re-fetch when the connected user changes (e.g. user re-auths
     under a different account). Read login as a dependency. */
  $effect(() => {
    void me;
    if (availableRepos.length === 0 && p.githubStatus.kind === 'connected') {
      void loadAvailableRepos();
    }
  });

  /** Repo dropdown options. Two pinned scope options at the top:
   *
   *    - `__inbox__` (default) — the GitHub inbox: `involves:@me is:open`.
   *      What you see at rest. Honest label: "Inbox · involves you".
   *    - `__all_open__` — all open PRs across every accessible repo.
   *      Drops the `involves:@me` qualifier; capped at 1000 results
   *      by GitHub Search API.
   *
   *  Below those, the full list of accessible repos (loaded via
   *  `github_list_repos`). Items-derived fallback is used while the
   *  API call is in flight. */
  const repoOptions = $derived.by(() => {
    const map = new Map<string, string>();
    if (availableRepos.length > 0) {
      for (const r of availableRepos) {
        map.set(`${r.owner}/${r.name}`, r.name);
      }
    } else {
      for (const it of items) {
        if (!it.repo) continue;
        map.set(`${it.repo.owner}/${it.repo.name}`, it.repo.name);
      }
    }
    const list = Array.from(map.entries())
      .map(([value, label]) => ({ value, label }))
      .sort((a, b) => a.label.localeCompare(b.label));
    return [
      { value: '__inbox__' as string, label: 'Inbox', hint: 'involves you' },
      { value: '__all_open__' as string, label: 'All open PRs', hint: `across ${list.length || '∞'} repos` },
      ...list
    ];
  });

  /** Author dropdown — union of authors seen in the current items,
   *  remote search results, and the connected user (so "filter to my
   *  PRs" is always a one-click pick even if I haven't authored
   *  anything visible yet). */
  const authorOptions = $derived.by(() => {
    const set = new Set<string>();
    for (const it of items) {
      if (it.author?.login) set.add(it.author.login);
    }
    for (const it of searchResults ?? []) {
      if (it.author?.login) set.add(it.author.login);
    }
    if (me) set.add(me);
    const sorted = Array.from(set).sort();
    const out: { value: string; label: string; hint?: string }[] = [
      { value: '__all__', label: 'Anyone' }
    ];
    if (me && set.has(me)) {
      out.push({ value: me, label: `@${me}`, hint: 'You' });
    }
    for (const login of sorted) {
      if (login === me) continue;
      out.push({ value: login, label: `@${login}` });
    }
    return out;
  });

  /** Remote search results. Null while the user is using the local
   *  inbox (no query, no remote-only filters). Populated by
   *  `github_search_inbox` when the user's query is non-empty or a
   *  remote-only filter (specific repo / non-self author) is set. */
  let searchResults = $state<InboxItem[] | null>(null);
  let searching = $state(false);
  let searchError = $state<string | null>(null);
  let searchTimer: ReturnType<typeof setTimeout> | null = null;
  let lastSearchKey = $state('');

  /** Compose a GitHub search query string from the active filters.
   *  See https://docs.github.com/en/search-github/searching-on-github/searching-issues-and-pull-requests
   *  for the full grammar.
   *
   *  Scope rules:
   *    - `repoFilter === '__all_open__'` — explicit "all PRs everywhere":
   *      drop `involves:@me`, scope to `is:pr` only.
   *    - `repoFilter === 'owner/name'` — scope to that repo, drop
   *      involves so you see ALL its open PRs (not just yours).
   *    - Otherwise — default inbox scope: `involves:@me`.
   *
   *  The Open/Draft chips refine state; everything is `is:open` by
   *  default since the inbox UX is open-only. */
  function buildSearchQuery(): string {
    const parts: string[] = [];
    const text = query.trim();
    if (text) parts.push(text);

    const isAllOpen = repoFilter === '__all_open__';
    const isSpecificRepo = repoFilter !== null && repoFilter !== '__all_open__';

    if (isSpecificRepo) {
      /* Scoped to a single repo — no involvement filter needed. */
      parts.push(`repo:${repoFilter}`);
    } else if (text || isAllOpen) {
      /* Searching or "All open PRs": scope to repos the user has access
         to via user: qualifiers (one per unique org/owner in the list).
         Falls back to involves:@me if repos haven't loaded yet so the
         first keystroke still works before loadAvailableRepos finishes. */
      const owners = [...new Set(availableRepos.map((r) => r.owner))];
      if (owners.length > 0) {
        owners.forEach((o) => parts.push(`user:${o}`));
      } else {
        parts.push('involves:@me');
      }
    } else {
      /* Pure inbox (no query, no special scope): involves:@me so the
         default view shows only PRs/issues that need the user's attention,
         not the full commit history of every repo. */
      const hasNarrower =
        (authorFilter !== null && authorFilter !== me) || roleFilter === 'reviewer';
      if (!hasNarrower) parts.push('involves:@me');
    }

    if (roleFilter === 'mine' && me) parts.push(`author:${me}`);
    if (roleFilter === 'reviewer' && me) parts.push('review-requested:@me');

    if (stateFilter === 'open') parts.push('is:open', 'draft:false');
    else if (stateFilter === 'draft') parts.push('is:open', 'draft:true');
    else parts.push('is:open');

    /* Always pin to PRs when searching so issues don't pollute results. */
    if (text || isAllOpen) parts.push('is:pr');
    if (authorFilter) parts.push(`author:${authorFilter}`);

    parts.push('sort:updated-desc');
    return parts.join(' ');
  }

  /** True when the active filter set diverges from the local inbox
   *  scope (involves:@me is:open). Triggers a remote search even when
   *  the search input is empty — otherwise picking "All open PRs" or
   *  filtering by a specific repo wouldn't actually broaden anything. */
  const wantsRemoteSearch = $derived(
    query.trim().length > 0 ||
    repoFilter !== null ||
    (authorFilter !== null && authorFilter !== me) ||
    roleFilter === 'reviewer'
  );

  $effect(() => {
    /* Whenever the user changes the search box or any remote-affecting
       filter, debounce 300ms and fire a single search request. The
       `lastSearchKey` guard prevents duplicate fetches when reactive
       reads re-trigger the effect with the same composed query. */
    void query;
    void roleFilter;
    void stateFilter;
    void repoFilter;
    void authorFilter;
    void me;

    if (searchTimer) {
      clearTimeout(searchTimer);
      searchTimer = null;
    }

    if (!wantsRemoteSearch) {
      searchResults = null;
      searching = false;
      searchError = null;
      lastSearchKey = '';
      return;
    }

    const q = buildSearchQuery();
    if (q === lastSearchKey && searchResults !== null) return;
    searching = true;
    searchError = null;
    searchTimer = setTimeout(async () => {
      lastSearchKey = q;
      try {
        const res = await invoke<InboxItem[]>('github_search_inbox', { query: q });
        /* Race-guard: if another keystroke landed while we were
           awaiting, ignore this stale response. */
        if (lastSearchKey !== q) return;
        searchResults = res;
      } catch (e) {
        if (lastSearchKey !== q) return;
        searchError = typeof e === 'string' ? e : (e as Error).message ?? 'search failed';
        searchResults = [];
      } finally {
        if (lastSearchKey === q) searching = false;
      }
    }, 300);
  });

  /** The list the rest of the component renders from. Remote search
   *  takes precedence when active; otherwise we fall back to the
   *  local inbox (still post-filtered for state/role chips that can
   *  apply locally). */
  const sourceItems = $derived(searchResults ?? items);

  /** Toggle helpers — clicking an already-active chip deselects it,
   *  reverting the filter to "all". Saves a separate "All" button. */
  function toggleRole(v: 'mine' | 'reviewer') {
    roleFilter = roleFilter === v ? null : v;
  }
  function toggleState(v: 'open' | 'draft') {
    stateFilter = stateFilter === v ? null : v;
  }

  /** Apply search + role/state/repo/author filters to produce the
   *  visible list. When `searchResults` is active the source already
   *  reflects the remote query — we just post-filter for the local-
   *  only chips (state/role) so toggling Draft after a search runs
   *  doesn't require a roundtrip. Grouping (below) operates on this
   *  filtered subset so group counts reflect what the user sees. */
  const filtered = $derived.by(() => {
    const q = query.trim().toLowerCase();
    const usingRemote = searchResults !== null;
    return sourceItems.filter((it) => {
      /* When using remote search, the query is already applied
         server-side; skip the local title/author match. When using
         the local inbox (no remote), apply the substring filter. */
      if (q && !usingRemote) {
        const numStr = `#${it.number}`;
        const titleMatch = it.title.toLowerCase().includes(q);
        const numMatch = numStr.includes(q) || String(it.number) === q.replace(/^#/, '');
        const authorMatch = it.author?.login?.toLowerCase().includes(q) ?? false;
        const repoMatch = it.repo
          ? `${it.repo.owner}/${it.repo.name}`.toLowerCase().includes(q)
          : false;
        if (!titleMatch && !numMatch && !authorMatch && !repoMatch) return false;
      }
      if (roleFilter === 'mine') {
        if (!me || it.author?.login !== me) return false;
      } else if (roleFilter === 'reviewer') {
        if (!it.is_pull_request) return false;
        if (!me || it.author?.login === me) return false;
        if (it.state !== 'open' || it.merged) return false;
      }
      if (stateFilter === 'open' && (it.state !== 'open' || it.merged || it.draft)) return false;
      if (stateFilter === 'draft' && !it.draft) return false;
      /* Specific-repo guard. The `__all_open__` sentinel widens scope
         server-side; locally it's a no-op (we want every result). */
      if (repoFilter && repoFilter !== '__all_open__') {
        const key = it.repo ? `${it.repo.owner}/${it.repo.name}` : '';
        if (key !== repoFilter) return false;
      }
      if (authorFilter) {
        if (it.author?.login !== authorFilter) return false;
      }
      return true;
    });
  });

  /** Group по типу + ревью-статусу:
   *  - "Awaiting review" — PR где у нас есть pending review request
   *  - "Pull requests" — остальные PR
   *  - "Issues" — обычные issues */
  const groups = $derived.by(() => {
    const awaiting: InboxItem[] = [];
    const prs: InboxItem[] = [];
    const issues: InboxItem[] = [];
    for (const it of filtered) {
      if (!it.is_pull_request) {
        issues.push(it);
        continue;
      }
      // Heuristic: "awaiting" если author не я И item ещё open
      if (me && it.author?.login !== me && it.state === 'open' && !it.merged) {
        awaiting.push(it);
      } else {
        prs.push(it);
      }
    }
    return [
      { label: 'Awaiting review', items: awaiting },
      { label: 'Pull requests', items: prs },
      { label: 'Issues', items: issues }
    ].filter((g) => g.items.length > 0);
  });

  const anyFilterActive = $derived(
    query.trim().length > 0 ||
    roleFilter !== null ||
    stateFilter !== null ||
    repoFilter !== null ||
    authorFilter !== null
  );

  function clearFilters() {
    query = '';
    roleFilter = null;
    stateFilter = null;
    repoFilter = null;
    authorFilter = null;
  }

  function clickItem(it: InboxItem, e: MouseEvent) {
    if (!p.isClickNotDrag(e)) return;
    p.onSelect(it.id);
  }

  function handleSearchKeydown(e: KeyboardEvent) {
    if (e.key !== 'Enter') return;
    const q = query.trim();
    if (/^#?\d+$/.test(q)) {
      doHotOpen();
      e.preventDefault();
    }
  }

  const hotOpenNum = $derived.by(() => {
    const q = query.trim();
    if (!/^#?\d+$/.test(q)) return null;
    return parseInt(q.replace('#', ''));
  });

  // Check both local inbox items and remote search results
  const hotOpenItem = $derived(
    hotOpenNum !== null
      ? (items.find((it) => it.number === hotOpenNum) ??
         searchResults?.find((it) => it.number === hotOpenNum) ??
         null)
      : null
  );

  function doHotOpen() {
    if (hotOpenNum === null) return;
    if (hotOpenItem) {
      p.onSelect(hotOpenItem.id);
    } else {
      // Build a scoped URL limited to repos in the dropdown list
      const knownRepos = availableRepos.length > 0
        ? availableRepos.map((r) => `${r.owner}/${r.name}`)
        : [...new Map(
            items.filter((it) => it.repo).map((it) => [`${it.repo!.owner}/${it.repo!.name}`, `${it.repo!.owner}/${it.repo!.name}`])
          ).values()];
      const repoScope = knownRepos.slice(0, 12).map((r) => `repo:${r}`).join(' ');
      const q = repoScope
        ? `is:pr is:open #${hotOpenNum} ${repoScope}`
        : `is:pr is:open #${hotOpenNum} involves:@me`;
      p.onOpenBrowser(`https://github.com/search?q=${encodeURIComponent(q)}&type=pullrequests`);
    }
    query = '';
  }

  function stateLabel(it: InboxItem): string {
    if (it.merged) return 'merged';
    if (it.draft) return 'draft';
    if (it.state === 'closed') return 'closed';
    return 'open';
  }

  function stateClass(it: InboxItem): string {
    if (it.merged) return 'st--merged';
    if (it.draft) return 'st--draft';
    if (it.state === 'closed') return 'st--closed';
    return 'st--open';
  }
</script>

<aside class="gl app-pane">
  <header class="gl-head">
    <h1 class="app-pane-head-h">GitHub</h1>
    {#if p.githubStatus.kind === 'connected'}
      <span class="app-pane-head-meta">@{p.githubStatus.user.login}</span>
    {/if}
    <span class="app-pane-head-spacer"></span>
    <button class="gl-act" onclick={p.onOpenCreatePr} title="New PR" disabled={p.githubStatus.kind !== 'connected'}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"><path d="M12 5v14M5 12h14"/></svg>
      <span>New PR</span>
    </button>
    <button class="gl-icon" onclick={p.onRefresh} title="Refresh" disabled={loading}>
      <svg class:spin={loading} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"><path d="M21 12a9 9 0 1 1-3-6.7"/><path d="M21 4v5h-5"/></svg>
    </button>
  </header>

  <!-- Filter bar: search input + role/state toggles + repo/author
       dropdowns. Lives between the header and the grouped list so the
       user can scope the inbox quickly. Each toggle chip is independent
       (click again to unset → "all"); the dropdowns share a `__all__`
       sentinel for the "no filter" option. -->
  <div class="gl-filters">
    <label class="gl-search" class:gl-search--remote={wantsRemoteSearch}>
      {#if searching}
        <span class="gl-search-spin" aria-hidden="true"></span>
      {:else}
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"><circle cx="11" cy="11" r="8"/><path d="m21 21-4.3-4.3"/></svg>
      {/if}
      <input
        type="text"
        placeholder={wantsRemoteSearch ? 'Searching all of GitHub…' : 'Search title, #number, @author, repo…'}
        bind:value={query}
        spellcheck="false"
        onkeydown={handleSearchKeydown}
      />
      {#if query}
        <button class="gl-search-clear" onclick={() => (query = '')} aria-label="Clear search">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>
        </button>
      {/if}
    </label>
    {#if hotOpenNum !== null}
      <button class="gl-hot-hint" onclick={doHotOpen} title="Open #{hotOpenNum} (Enter)">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"><path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"/><polyline points="15 3 21 3 21 9"/><line x1="10" y1="14" x2="21" y2="3"/></svg>
        {#if hotOpenItem}
          #{hotOpenItem.number} · {hotOpenItem.title}
        {:else}
          Open #{hotOpenNum} on GitHub
        {/if}
      </button>
    {/if}

    <div class="gl-chips">
      <button
        class="gl-toggle"
        class:active={roleFilter === 'mine'}
        disabled={!me}
        onclick={() => toggleRole('mine')}
        title={me ? `Author = @${me}` : 'Connect GitHub to filter by author'}
      >
        <span class="gl-toggle-dot"></span>
        Mine
      </button>
      <button
        class="gl-toggle"
        class:active={roleFilter === 'reviewer'}
        disabled={!me}
        onclick={() => toggleRole('reviewer')}
        title="PRs awaiting your review"
      >
        <span class="gl-toggle-dot"></span>
        Reviewer
      </button>
      <span class="gl-divider" aria-hidden="true"></span>
      <button
        class="gl-toggle"
        class:active={stateFilter === 'open'}
        onclick={() => toggleState('open')}
        title="Open (non-draft)"
      >
        <span class="gl-toggle-dot"></span>
        Open
      </button>
      <button
        class="gl-toggle"
        class:active={stateFilter === 'draft'}
        onclick={() => toggleState('draft')}
        title="Drafts only"
      >
        <span class="gl-toggle-dot"></span>
        Draft
      </button>
      <span class="gl-divider" aria-hidden="true"></span>

      <span class="gl-dd">
        <Dropdown
          value={repoFilter ?? '__inbox__'}
          options={repoOptions}
          onChange={(v) => (repoFilter = v === '__inbox__' ? null : v)}
          placeholder="Repo"
          ariaLabel="Repository scope"
          variant="chip"
          compact
        />
      </span>
      <span class="gl-dd">
        <Dropdown
          value={authorFilter ?? '__all__'}
          options={authorOptions}
          onChange={(v) => (authorFilter = v === '__all__' ? null : v)}
          placeholder="Author"
          ariaLabel="Filter by author"
          variant="chip"
          compact
        />
      </span>

      {#if anyFilterActive}
        <button class="gl-chip-clear" onclick={clearFilters} title="Clear all filters">Clear</button>
      {/if}
    </div>
  </div>

  <div class="gl-body">
    {#if searchError}
      <div class="gl-error">
        <p class="gl-error-h serif">Search failed</p>
        <p class="gl-error-p mono">{searchError}</p>
        <button class="gl-error-retry" onclick={() => { searchError = null; lastSearchKey = ''; }}>Retry</button>
      </div>
    {:else if error}
      <div class="gl-error">
        <p class="gl-error-h serif">Couldn't load GitHub</p>
        <p class="gl-error-p mono">{error}</p>
        <button class="gl-error-retry" onclick={p.onRefresh}>Retry</button>
      </div>
    {:else if loading && items.length === 0 && !wantsRemoteSearch}
      <div class="gl-loading">
        <div class="gl-spinner"></div>
        <span class="mono">Loading…</span>
      </div>
    {:else if searching && (searchResults?.length ?? 0) === 0}
      <div class="gl-loading">
        <div class="gl-spinner"></div>
        <span class="mono">Searching GitHub…</span>
      </div>
    {:else if !wantsRemoteSearch && items.length === 0}
      <div class="gl-empty">
        <p class="gl-empty-h serif">Nothing here</p>
        <p class="gl-empty-p">No PRs or issues yet. Create one or refresh.</p>
      </div>
    {:else if filtered.length === 0}
      <div class="gl-empty">
        <p class="gl-empty-h serif">No matches</p>
        <p class="gl-empty-p">{wantsRemoteSearch ? 'No GitHub items match this query and filters.' : 'No items match the current filters.'}</p>
        <button class="gl-error-retry" onclick={clearFilters}>Clear filters</button>
      </div>
    {:else}
      {#each groups as g (g.label)}
        <div class="gl-group">{g.label} <span class="mono">·</span> {g.items.length}</div>
        {#each g.items as it (it.id)}
          {@const isActive = inboxState.focusItem?.id === it.id}
          <button
            class="gl-card"
            class:active={isActive}
            draggable="true"
            onpointerdown={p.onCardMouseDown}
            ondragstart={(e) => p.onDragStart({ source: 'github', item: it }, e)}
            ondragend={p.onDragEnd}
            onclick={(e) => clickItem(it, e)}
            ondblclick={() => p.onOpenBrowser(it.url)}
            title={it.title}
          >
            <div class="gl-card-top">
              <span class="gl-card-st {stateClass(it)}">{stateLabel(it)}</span>
              <span class="gl-card-num mono">
                {#if it.is_pull_request}
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6"><circle cx="6" cy="6" r="2.5"/><circle cx="6" cy="18" r="2.5"/><circle cx="18" cy="18" r="2.5"/><path d="M6 8.5v7M8.5 6h7a3 3 0 0 1 3 3v6.5"/></svg>
                {:else}
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6"><circle cx="12" cy="12" r="9"/><circle cx="12" cy="12" r="2.5"/></svg>
                {/if}
                #{it.number}
              </span>
              {#if it.repo}
                <span class="gl-card-repo mono" title={`${it.repo.owner}/${it.repo.name}`}>{it.repo.name}</span>
              {/if}
              <span class="gl-card-time mono">{relativeTime(it.updated_at, p.now)}</span>
            </div>
            <div class="gl-card-title">{it.title}</div>
            <div class="gl-card-meta">
              {#if it.labels.length > 0}
                {#each it.labels.slice(0, 3) as l (l.name)}
                  <span class="label" style="background: #{l.color}22; border-color: #{l.color}55; color: #{l.color};">
                    {l.name}
                  </span>
                {/each}
                {#if it.labels.length > 3}<span class="label-more mono">+{it.labels.length - 3}</span>{/if}
              {/if}
              {#if it.author}
                <span class="ava" title={it.author.login}>
                  {#if it.author.avatar_url}
                    <img src={it.author.avatar_url} alt={it.author.login} loading="lazy" />
                  {:else}
                    {(it.author.login || '?').slice(0, 1).toUpperCase()}
                  {/if}
                </span>
              {/if}
              {#if it.comments > 0}
                <span class="gl-card-comments mono" title={`${it.comments} comments`}>
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7"><path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/></svg>
                  {it.comments}
                </span>
              {/if}
            </div>
            <span class="gl-card-sends">
              <span
                class="gl-card-send gl-card-send--claude"
                role="button"
                tabindex="0"
                onclick={(e) => clickSendToClaude(it, e)}
                onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); clickSendToClaude(it, e as unknown as MouseEvent); } }}
                onpointerdown={(e) => e.stopPropagation()}
                title="Send to Claude"
                aria-label="Send to Claude"
              >
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">
                  <path d="M22 2 11 13"/>
                  <path d="m22 2-7 20-4-9-9-4 20-7z"/>
                </svg>
                <span>Claude</span>
              </span>
              <span
                class="gl-card-send gl-card-send--cursor"
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
  .gl {
    /* Width comes from the parent `app-shell` grid track. */
    min-width: 0;
  }
  .gl-head {
    flex: 0 0 46px;
    display: flex; align-items: center; gap: 10px;
    padding: 0 12px;
    border-bottom: 1px solid var(--border);
    background: linear-gradient(180deg, var(--bg-2), var(--bg-1));
  }
  .gl-act {
    display: inline-flex; align-items: center; gap: 5px;
    padding: 4px 9px;
    border-radius: 7px;
    font-size: 11.5px; color: var(--text-1);
    background: var(--bg-2);
    border: 1px solid var(--border);
    cursor: pointer;
  }
  .gl-act:hover:not(:disabled) {
    color: var(--accent-bright);
    background: var(--accent-soft);
    border-color: var(--border-accent-2);
  }
  .gl-act:disabled { opacity: 0.5; cursor: not-allowed; }
  .gl-act svg { width: 11px; height: 11px; }
  .gl-icon {
    width: 26px; height: 26px;
    display: grid; place-items: center;
    color: var(--text-2);
    background: transparent; border: none; cursor: pointer;
    border-radius: 6px;
  }
  .gl-icon:hover:not(:disabled) { color: var(--text-0); background: var(--bg-elev, var(--bg-2)); }
  .gl-icon svg { width: 13px; height: 13px; }
  .gl-icon .spin { animation: gl-spin 0.9s linear infinite; }
  @keyframes gl-spin { to { transform: rotate(360deg); } }

  /* Filter bar — search + chips + repo select. Sits between header
     and list so it always reads as "scope this view". */
  .gl-filters {
    flex: 0 0 auto;
    display: flex; flex-direction: column;
    gap: 6px;
    padding: 8px 12px 10px;
    border-bottom: 1px solid var(--border);
    background: var(--bg-1);
  }
  .gl-search {
    position: relative;
    display: flex; align-items: center; gap: 6px;
    padding: 0 8px;
    height: 28px;
    border-radius: 6px;
    background: var(--bg-2);
    border: 1px solid var(--border);
    transition: border-color 120ms;
  }
  .gl-search:focus-within {
    border-color: var(--border-accent);
    background: var(--bg-1);
  }
  .gl-search > svg {
    width: 12px; height: 12px;
    color: var(--text-mute);
    flex-shrink: 0;
  }
  /* Remote-search active — clay tint on the box border so the user
     reads "this is hitting the GitHub Search API now". */
  .gl-search.gl-search--remote {
    border-color: color-mix(in srgb, var(--src-github) 36%, var(--border));
    box-shadow: 0 0 0 1px color-mix(in srgb, var(--src-github) 18%, transparent);
  }
  /* Spinner that replaces the magnifier glyph during a remote
     search — same 12px footprint so the input layout doesn't shift. */
  .gl-search-spin {
    width: 12px; height: 12px;
    border: 1.5px solid color-mix(in srgb, var(--src-github) 24%, var(--border));
    border-top-color: var(--src-github);
    border-radius: 50%;
    animation: gl-spin 0.7s linear infinite;
    flex-shrink: 0;
  }
  .gl-search input {
    flex: 1; min-width: 0;
    background: transparent; border: 0; outline: 0;
    color: var(--text-0);
    font-size: 12px;
    font-family: inherit;
  }
  .gl-search input::placeholder { color: var(--text-mute); }
  .gl-search-clear {
    width: 16px; height: 16px;
    display: grid; place-items: center;
    border: 0; background: transparent;
    color: var(--text-mute);
    cursor: pointer;
    border-radius: 4px;
  }
  .gl-search-clear:hover { color: var(--text-0); background: var(--bg-3); }
  .gl-search-clear svg { width: 10px; height: 10px; }

  .gl-hot-hint {
    display: inline-flex; align-items: center; gap: 6px;
    padding: 3px 10px;
    border-radius: 6px;
    background: color-mix(in srgb, var(--accent) 10%, var(--bg-2));
    border: 1px solid color-mix(in srgb, var(--accent) 30%, transparent);
    color: var(--accent-bright);
    font-size: 11.5px; font-weight: 500;
    cursor: pointer;
    max-width: 100%;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
    transition: background 120ms, border-color 120ms;
  }
  .gl-hot-hint:hover {
    background: color-mix(in srgb, var(--accent) 18%, var(--bg-2));
    border-color: color-mix(in srgb, var(--accent) 50%, transparent);
  }
  .gl-hot-hint svg { width: 11px; height: 11px; flex-shrink: 0; }

  .gl-chips {
    display: flex; gap: 6px; flex-wrap: wrap; align-items: center;
  }
  .gl-divider {
    width: 1px; height: 14px;
    background: var(--border);
    margin: 0 2px;
  }

  /* Toggle pill — independent on/off chip. Active state shows a
     filled brand-tinted dot to the left of the label, like Linear's
     filter chips. Inactive shows a hollow dot. */
  .gl-toggle {
    display: inline-flex; align-items: center; gap: 6px;
    padding: 4px 10px 4px 8px;
    border-radius: 999px;
    background: var(--bg-2);
    border: 1px solid var(--border);
    color: var(--text-1);
    font-size: 11px;
    font-weight: 500;
    cursor: pointer;
    transition: all 140ms;
    user-select: none;
  }
  .gl-toggle-dot {
    width: 8px; height: 8px;
    border-radius: 50%;
    background: transparent;
    box-shadow: inset 0 0 0 1.5px var(--text-mute);
    transition: all 140ms;
    flex-shrink: 0;
  }
  .gl-toggle:hover:not(:disabled):not(.active) {
    color: var(--text-0);
    background: var(--bg-3);
    border-color: var(--border-hi);
  }
  .gl-toggle:hover:not(:disabled):not(.active) .gl-toggle-dot {
    box-shadow: inset 0 0 0 1.5px var(--text-1);
  }
  .gl-toggle.active {
    color: var(--accent-bright);
    background: color-mix(in srgb, var(--src-github) 14%, transparent);
    border-color: color-mix(in srgb, var(--src-github) 40%, transparent);
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--src-github) 8%, transparent);
  }
  .gl-toggle.active .gl-toggle-dot {
    background: var(--src-github);
    box-shadow:
      inset 0 0 0 1.5px var(--src-github),
      0 0 6px color-mix(in srgb, var(--src-github) 60%, transparent);
  }
  .gl-toggle:disabled { opacity: 0.45; cursor: not-allowed; }

  /* Dropdown wrapper — make the existing Dropdown trigger blend with
     our pill scale. The component itself uses `.dd-trigger` internally
     via global selectors here. */
  .gl-dd { display: inline-flex; }
  .gl-dd :global(.dd-trigger) {
    border-radius: 999px;
    border: 1px solid var(--border);
    background: var(--bg-2);
    color: var(--text-1);
    font-size: 11px;
    height: auto;
    padding: 4px 10px;
    transition: all 140ms;
  }
  .gl-dd :global(.dd-trigger:hover) {
    color: var(--text-0);
    background: var(--bg-3);
    border-color: var(--border-hi);
  }
  .gl-dd :global(.dd-trigger[aria-expanded="true"]) {
    color: var(--accent-bright);
    background: color-mix(in srgb, var(--src-github) 14%, transparent);
    border-color: color-mix(in srgb, var(--src-github) 40%, transparent);
  }

  .gl-chip-clear {
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
  .gl-chip-clear:hover { color: var(--text-0); border-color: var(--text-mute); border-style: solid; }

  .gl-body {
    flex: 1; min-height: 0;
    overflow-y: auto;
    padding: 4px 8px 12px;
  }

  .gl-group {
    padding: 14px 8px 6px;
    font-size: 9.5px; font-weight: 700;
    letter-spacing: 0.10em;
    text-transform: uppercase;
    color: var(--text-mute);
    font-family: 'JetBrains Mono', monospace;
    display: flex; align-items: center; gap: 8px;
  }
  .gl-group::after {
    content: ''; flex: 1; height: 1px;
    background: linear-gradient(90deg, var(--border), transparent);
  }
  .gl-group .mono { opacity: 0.5; }

  .gl-card {
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
  .gl-card::before {
    content: ''; position: absolute; left: 0; top: 11px; bottom: 11px;
    width: 2px; border-radius: 2px;
    background: var(--src-github);
    opacity: 0; transition: opacity 200ms;
  }
  .gl-card:hover { background: var(--bg-2); border-color: var(--border); }
  .gl-card:hover::before { opacity: 0.5; }
  .gl-card.active {
    background: var(--bg-2);
    border-color: var(--border-hi);
    box-shadow:
      0 0 0 1px rgba(181, 132, 255, 0.32),
      0 12px 24px rgba(0, 0, 0, 0.28);
  }
  .gl-card.active::before {
    opacity: 1;
    box-shadow: 0 0 10px rgba(181, 132, 255, 0.40);
  }

  .gl-card-top { display: flex; align-items: center; gap: 7px; flex-wrap: wrap; }
  .gl-card-st {
    padding: 1px 6px;
    border-radius: 4px;
    font-size: 9.5px; font-weight: 600;
    text-transform: uppercase; letter-spacing: 0.04em;
  }
  .st--open    { color: var(--success); background: rgba(168, 217, 184, 0.10); border: 1px solid rgba(168, 217, 184, 0.24); }
  .st--draft   { color: var(--text-2); background: var(--bg-3); border: 1px solid var(--border-hi); }
  .st--merged  { color: #C9A0E0; background: rgba(181, 132, 255, 0.10); border: 1px solid rgba(181, 132, 255, 0.24); }
  .st--closed  { color: var(--text-mute); background: var(--bg-3); border: 1px solid var(--border); }

  .gl-card-num {
    display: inline-flex; align-items: center; gap: 4px;
    font-size: 11px; color: var(--text-2);
  }
  .gl-card-num svg { width: 11px; height: 11px; color: var(--src-github); }
  .gl-card-repo {
    font-size: 10px; color: var(--text-mute);
    padding: 1px 5px;
    background: var(--bg-3);
    border: 1px solid var(--border);
    border-radius: 3px;
    max-width: 110px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .gl-card-time {
    margin-left: auto;
    font-size: 10px; color: var(--text-mute);
  }
  .gl-card-title {
    font-size: 13px; color: var(--text-0); font-weight: 500;
    line-height: 1.4;
    display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical;
    overflow: hidden;
  }
  .gl-card-meta {
    display: flex; align-items: center; gap: 5px; flex-wrap: wrap;
    font-size: 10.5px; color: var(--text-2);
  }

  .label {
    padding: 1px 6px;
    border-radius: 3px;
    font-size: 9.5px;
    border: 1px solid transparent;
    max-width: 100px;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .label-more { font-size: 10px; color: var(--text-mute); }

  .ava {
    width: 16px; height: 16px;
    border-radius: 50%;
    background: linear-gradient(135deg, var(--accent-bright), var(--accent-deep));
    color: #1F1410;
    font-size: 9px; font-weight: 700;
    display: grid; place-items: center;
    overflow: hidden;
    flex-shrink: 0;
  }
  .ava img { width: 100%; height: 100%; object-fit: cover; }

  .gl-card-comments {
    display: inline-flex; align-items: center; gap: 3px;
    font-size: 10px; color: var(--text-mute);
  }
  .gl-card-comments svg { width: 10px; height: 10px; }

  /* Send-to-{agent} pair — appears on hover/active in the top-right of
     the card. Two role="button" spans (a real <button> would be invalid
     HTML inside the row's <button>). Each is brand-tinted by agent
     kind so the user reads at-a-glance which way they're handing off. */
  .gl-card-sends {
    position: absolute;
    top: 8px; right: 10px;
    display: inline-flex; gap: 4px;
    opacity: 0;
    transition: opacity 140ms;
  }
  .gl-card:hover .gl-card-sends,
  .gl-card-sends:focus-within,
  .gl-card.active .gl-card-sends {
    opacity: 1;
  }
  .gl-card-send {
    display: inline-flex; align-items: center; gap: 4px;
    padding: 3px 8px 3px 7px;
    border-radius: 5px;
    font-size: 10px; font-weight: 600;
    letter-spacing: 0.02em;
    cursor: pointer;
    transition: background 140ms, transform 140ms;
    user-select: none;
  }
  .gl-card-send svg { width: 11px; height: 11px; }
  .gl-card-send--claude {
    color: var(--src-claude);
    background: color-mix(in srgb, var(--src-claude) 12%, transparent);
    border: 1px solid color-mix(in srgb, var(--src-claude) 28%, transparent);
  }
  .gl-card-send--claude:hover {
    background: color-mix(in srgb, var(--src-claude) 22%, transparent);
    color: var(--accent-bright);
    transform: translateY(-1px);
  }
  .gl-card-send--cursor {
    color: var(--src-cursor, var(--text-1));
    background: color-mix(in srgb, var(--src-cursor, var(--text-1)) 14%, transparent);
    border: 1px solid color-mix(in srgb, var(--src-cursor, var(--text-1)) 32%, transparent);
  }
  .gl-card-send--cursor:hover {
    background: color-mix(in srgb, var(--src-cursor, var(--text-1)) 24%, transparent);
    color: var(--text-0);
    transform: translateY(-1px);
  }
  .gl-card-send:active { transform: translateY(0); }

  .gl-empty, .gl-loading, .gl-error {
    text-align: center;
    padding: 50px 20px;
    margin: auto;
  }
  .gl-empty-h, .gl-error-h {
    font-family: 'Geist', 'Inter', -apple-system, system-ui, sans-serif;
    font-size: 22px; font-weight: 600; letter-spacing: -0.015em;
    color: var(--text-0);
    margin: 0 0 10px;
  }
  .gl-empty-p, .gl-error-p {
    font-size: 12.5px; color: var(--text-2);
    line-height: 1.55; margin: 0;
  }
  .gl-error-p { color: var(--error); }
  .gl-error-retry {
    margin-top: 14px;
    padding: 6px 12px;
    border-radius: 7px;
    font-size: 12px;
    background: var(--bg-2);
    border: 1px solid var(--border-hi);
    color: var(--text-1);
    cursor: pointer;
  }
  .gl-error-retry:hover { color: var(--text-0); }
  .gl-loading {
    display: flex; align-items: center; justify-content: center; gap: 10px;
    color: var(--text-2); font-size: 12px;
  }
  .gl-spinner {
    width: 14px; height: 14px;
    border: 1.5px solid var(--border-hi);
    border-top-color: var(--accent);
    border-radius: 50%;
    animation: gl-spin 0.7s linear infinite;
  }
</style>
