<script lang="ts">
  /* GithubList — left panel of GithubApp. Standalone: reads inbox
     state, renders PRs / Issues + filter row + groups + item cards.
     Click on item → selectInboxItem (sets inboxState.focusItem).
     Drag handlers for drop-into-Claude. */
  import {
    inboxState,
    githubItemsFor,
    githubLoadingFor,
    githubErrorFor,
    openFocusItem,
    githubFiltersFor,
    persistGithubUiFilters
  } from '$lib/state/inbox.svelte';
  import { relativeTime, type InboxItem, type ConnectionStatus, type Repository } from '$lib/data';
  import Dropdown from '$lib/components/ui/Dropdown.svelte';
  import ListSearchPicker from '$lib/views/apps/_shared/ListSearchPicker.svelte';
  import CardContextMenu, { type MenuItem } from '$lib/views/apps/_shared/CardContextMenu.svelte';
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
    /** One-click handoff to Claude — equivalent to dragging
     *  the row onto the rail icon, but for users who prefer
     *  a button over a drag gesture. Shown on each PR/issue card. */
    onSendToClaude: (item: InboxItem) => void;
    /** Seed a Dynamic Workflow from this item — templates a task and
     *  routes through the live-build DW pipeline. */
    onFixWithDw: (item: InboxItem) => void;
  }
  let p: Props = $props();

  function clickSendToClaude(it: InboxItem, e: MouseEvent) {
    /* Don't let the row's onclick fire — sending should not also
       focus the item; the user has expressed a different intent. */
    e.stopPropagation();
    p.onSendToClaude(it);
  }

  const items = $derived(githubItemsFor(p.instanceId));
  const loading = $derived(githubLoadingFor(p.instanceId));
  const error = $derived(githubErrorFor(p.instanceId));

  /** Search + filter state. Persisted per-instance via
   *  `persistGithubUiFilters` — survives solo switches + app restart.
   *  Each chip is a toggle (click again to deselect → "all");
   *  dropdowns use a null sentinel for "any". Init reads from the
   *  store; mutations bind to $state locally then a $effect mirrors
   *  the diff back. */
  const _init = githubFiltersFor(p.instanceId);
  let query = $state(_init.uiQuery ?? '');
  let roleFilter = $state<'reviewer' | null>(_init.uiRoleFilter ?? null);
  let stateFilter = $state<'open' | 'draft' | null>(_init.uiStateFilter ?? null);
  /** `null` = inbox scope (involves:@me, the default). `'__all_open__'`
   *  = drop the involves filter, search every accessible repo.
   *  Otherwise the literal `owner/name` of a specific repo. */
  let repoFilter = $state<string | null>(_init.uiRepoFilter ?? null);
  let authorFilter = $state<string | null>(_init.uiAuthorFilter ?? null);

  /* Mirror UI filter state into the persistent slot. Effect re-runs
     whenever any field changes; `persistGithubUiFilters` is a no-op
     when nothing differs so re-renders for unrelated reasons (items
     refresh, focus change) don't punish disk writes. */
  $effect(() => {
    persistGithubUiFilters(p.instanceId, {
      uiQuery: query,
      uiRoleFilter: roleFilter,
      uiStateFilter: stateFilter,
      uiRepoFilter: repoFilter,
      uiAuthorFilter: authorFilter
    });
  });

  /* Right-click context menu state. `ctxCoords` holds the cursor
     position; `ctxItem` carries the InboxItem the menu was opened
     against (the action closures capture it so we don't lose the row
     reference after the menu opens). */
  let ctxCoords = $state<{ x: number; y: number } | null>(null);
  let ctxItem = $state<InboxItem | null>(null);

  function openCtxMenu(e: MouseEvent, it: InboxItem) {
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
        label: 'Fix with DW',
        icon: 'M12 2v4 M12 18v4 M4.93 4.93l2.83 2.83 M16.24 16.24l2.83 2.83 M2 12h4 M18 12h4 M4.93 19.07l2.83-2.83 M16.24 7.76l2.83-2.83',
        onClick: () => p.onFixWithDw(it)
      },
      {
        label: 'Open in browser',
        icon: 'M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6 M15 3h6v6 M10 14L21 3',
        onClick: () => p.onOpenBrowser(it.url)
      },
      {
        label: 'Copy URL',
        icon: 'M16 4h2a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h2 M9 2h6a1 1 0 0 1 1 1v2H8V3a1 1 0 0 1 1-1z',
        onClick: async () => {
          try {
            await navigator.clipboard.writeText(it.url);
          } catch (e) {
            console.warn('clipboard', e);
          }
        },
        shortcut: '⌘C'
      }
    ];
  });

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
      /* Searching or "All open PRs": scope to the exact repos loaded
         via github_list_repos (the dropdown list) — no unrelated public
         repos. Falls back to involves:@me until the list loads. */
      if (availableRepos.length > 0) {
        availableRepos.forEach((r) => parts.push(`repo:${r.owner}/${r.name}`));
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
  function toggleRole(v: 'reviewer') {
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
      if (roleFilter === 'reviewer') {
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

  /** Group by type + review status:
   *  - "Awaiting review" — PRs with a pending review request for us
   *  - "Pull requests" — the rest of the PRs
   *  - "Issues" — plain issues */
  const groups = $derived.by(() => {
    const awaiting: InboxItem[] = [];
    const prs: InboxItem[] = [];
    const issues: InboxItem[] = [];
    for (const it of filtered) {
      if (!it.is_pull_request) {
        issues.push(it);
        continue;
      }
      // Heuristic: "awaiting" if author isn't me AND item is still open
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

  /* ─── Search picker (server-side) ───────────────────────────────
     Independent of the main inline search: fires its OWN GitHub
     search on every keystroke (debounced 250ms) with a minimal
     query — ignores chip filters (Mine / Reviewer / Open / Draft /
     Author) so the picker can quick-jump to any PR the user has
     access to, even if the chip filters would currently hide it. */
  let searchEl = $state<HTMLLabelElement | null>(null);
  let pickerEl = $state<{ handleKey: (e: KeyboardEvent) => boolean } | null>(null);
  let pickerOpen = $state(false);

  let pickerRemoteItems = $state<InboxItem[] | null>(null);
  let pickerLastQuery = '';
  let pickerSearchTimer: ReturnType<typeof setTimeout> | null = null;

  /** Picker-only query: text + repo scope + is:pr is:open. No
   *  involves:@me, no role/state/author chips — picker is a global
   *  quick-jump, not a filtered view. */
  function buildPickerQuery(text: string): string {
    const parts: string[] = [text, 'is:pr', 'is:open', 'sort:updated-desc'];
    if (availableRepos.length > 0) {
      availableRepos.forEach((r) => parts.push(`repo:${r.owner}/${r.name}`));
    } else {
      parts.push('involves:@me');
    }
    return parts.join(' ');
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
      try {
        const composed = buildPickerQuery(q);
        const res = await invoke<InboxItem[]>('github_search_inbox', { query: composed });
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
    const numMatch = q.replace(/^#/, '');
    const remote = pickerRemoteItems ?? [];
    const ranked = remote.map((it) => {
      const num = String(it.number);
      const titleL = it.title.toLowerCase();
      const repoL = it.repo ? `${it.repo.owner}/${it.repo.name}`.toLowerCase() : '';
      let rank = 6;
      if (num === numMatch) rank = 0;
      else if (titleL.includes(q)) rank = 2;
      else if (num.includes(numMatch) && /^\d+$/.test(numMatch)) rank = 3;
      else if (repoL.includes(q)) rank = 4;
      else if (it.author?.login?.toLowerCase().includes(q)) rank = 5;
      return {
        id: String(it.id),
        title: it.title,
        sub: `#${it.number}${it.repo ? ` · ${it.repo.name}` : ''}`,
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

  function pickPr(id: string) {
    const numId = parseInt(id);
    /* Prefer the remote-search hit so picking works even when the PR
       isn't in the local inbox (different repo / not involving the
       user). Fall back to the parent's selectInboxItem path for
       items already in the local list. */
    const remote = pickerRemoteItems?.find((it) => String(it.id) === id);
    if (remote) openFocusItem(remote);
    else p.onSelect(numId);
    query = '';
    pickerOpen = false;
  }

  /** Open the typed `#NNN` directly when the picker is empty (PR
   *  isn't in any visible repo). Builds a scoped GitHub search URL
   *  limited to the user's known repos so the browser jump lands
   *  somewhere relevant. */
  function openByNumberInBrowser(num: number) {
    const knownRepos = availableRepos.length > 0
      ? availableRepos.map((r) => `${r.owner}/${r.name}`)
      : [...new Map(
          items.filter((it) => it.repo).map((it) => [`${it.repo!.owner}/${it.repo!.name}`, `${it.repo!.owner}/${it.repo!.name}`])
        ).values()];
    const repoScope = knownRepos.slice(0, 12).map((r) => `repo:${r}`).join(' ');
    const q = repoScope
      ? `is:pr is:open #${num} ${repoScope}`
      : `is:pr is:open #${num} involves:@me`;
    p.onOpenBrowser(`https://github.com/search?q=${encodeURIComponent(q)}&type=pullrequests`);
  }

  function handleSearchKeydown(e: KeyboardEvent) {
    if (pickerEl && pickerEl.handleKey(e)) return;
    if (e.key !== 'Enter') return;
    const q = query.trim();
    if (/^#?\d+$/.test(q)) {
      const num = parseInt(q.replace('#', ''));
      const hit = items.find((it) => it.number === num)
        ?? searchResults?.find((it) => it.number === num)
        ?? null;
      if (hit) p.onSelect(hit.id);
      else openByNumberInBrowser(num);
      query = '';
      e.preventDefault();
    }
  }

  function stateLabel(it: InboxItem): string {
    if (it.merged) return 'merged';
    if (it.draft) return 'draft';
    if (it.state === 'closed') return 'closed';
    return 'open';
  }

  function stateClass(it: InboxItem): string {
    if (it.merged) return 'tag--merged';
    if (it.draft) return 'tag--draft';
    if (it.state === 'closed') return 'tag--closed';
    return 'tag--open';
  }
</script>

<aside class="lp ghl">
  <header class="lp-head">
    <span class="lp-title">Pull requests</span>
    {#if filtered.length > 0}<span class="lp-count">{filtered.length}</span>{/if}
    <span class="lp-head-spring"></span>
    <button class="lp-add" onclick={p.onOpenCreatePr} title="New PR" aria-label="New PR" disabled={p.githubStatus.kind !== 'connected'}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round"><path d="M12 5v14M5 12h14"/></svg>
    </button>
    <button class="lp-ghostbtn" onclick={p.onRefresh} title="Refresh" aria-label="Refresh" disabled={loading}>
      <svg class:spin={loading} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"><path d="M21 12a9 9 0 1 1-3-6.7"/><path d="M21 4v5h-5"/></svg>
    </button>
  </header>

  <label class="ghl-search" class:ghl-search--remote={wantsRemoteSearch} bind:this={searchEl}>
    {#if searching}
      <span class="ghl-search-spin" aria-hidden="true"></span>
    {:else}
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"><circle cx="11" cy="11" r="8"/><path d="m21 21-4.3-4.3"/></svg>
    {/if}
    <input
      type="text"
      placeholder={wantsRemoteSearch ? 'Searching all of GitHub…' : 'Search title, #number, @author, repo…'}
      bind:value={query}
      spellcheck="false"
      onkeydown={handleSearchKeydown}
      onfocus={openPicker}
    />
    {#if query}
      <button class="ghl-search-clear" onclick={() => (query = '')} aria-label="Clear search">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>
      </button>
    {/if}
  </label>
  <ListSearchPicker
    bind:this={pickerEl}
    anchor={searchEl}
    open={pickerOpen}
    rows={pickerRows}
    source="github"
    onPick={pickPr}
    onClose={closePicker}
  />

  <div class="lp-chips">
    <button
      class="lp-chip"
      class:active={roleFilter === 'reviewer'}
      disabled={!me}
      onclick={() => toggleRole('reviewer')}
      title="PRs awaiting your review"
    >Reviewer</button>
    <button
      class="lp-chip"
      class:active={stateFilter === 'open'}
      onclick={() => toggleState('open')}
      title="Open (non-draft)"
    >Open</button>
    <button
      class="lp-chip"
      class:active={stateFilter === 'draft'}
      onclick={() => toggleState('draft')}
      title="Drafts only"
    >Draft</button>

    <span class="ghl-dd">
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
    <span class="ghl-dd">
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
      <button class="ghl-chip-clear" onclick={clearFilters} title="Clear all filters">Clear</button>
    {/if}
  </div>

  <div class="lp-list">
    {#if searchError}
      <div class="ghl-error">
        <p class="ghl-error-h serif">Search failed</p>
        <p class="ghl-error-p mono">{searchError}</p>
        <button class="ghl-error-retry" onclick={() => { searchError = null; lastSearchKey = ''; }}>Retry</button>
      </div>
    {:else if error}
      <div class="ghl-error">
        <p class="ghl-error-h serif">Couldn't load GitHub</p>
        <p class="ghl-error-p mono">{error}</p>
        <button class="ghl-error-retry" onclick={p.onRefresh}>Retry</button>
      </div>
    {:else if loading && items.length === 0 && !wantsRemoteSearch}
      <div class="ghl-loading">
        <div class="ghl-spinner"></div>
        <span class="mono">Loading…</span>
      </div>
    {:else if searching && (searchResults?.length ?? 0) === 0}
      <div class="ghl-loading">
        <div class="ghl-spinner"></div>
        <span class="mono">Searching GitHub…</span>
      </div>
    {:else if !wantsRemoteSearch && items.length === 0}
      <div class="ghl-empty">
        <p class="ghl-empty-h serif">Nothing here</p>
        <p class="ghl-empty-p">No PRs or issues yet. Create one or refresh.</p>
      </div>
    {:else if filtered.length === 0}
      <div class="ghl-empty">
        <p class="ghl-empty-h serif">No matches</p>
        <p class="ghl-empty-p">{wantsRemoteSearch ? 'No GitHub items match this query and filters.' : 'No items match the current filters.'}</p>
        <button class="ghl-error-retry" onclick={clearFilters}>Clear filters</button>
      </div>
    {:else}
      {#each groups as g (g.label)}
        <div class="lp-group-label">{g.label} · {g.items.length}</div>
        {#each g.items as it (it.id)}
          {@const isActive = inboxState.focusItem?.id === it.id}
          <button
            class="lp-row ghl-row"
            class:active={isActive}
            draggable="true"
            onpointerdown={p.onCardMouseDown}
            ondragstart={(e) => p.onDragStart({ source: 'github', item: it }, e)}
            ondragend={p.onDragEnd}
            onclick={(e) => clickItem(it, e)}
            ondblclick={() => p.onOpenBrowser(it.url)}
            oncontextmenu={(e) => openCtxMenu(e, it)}
          >
            <div class="ghl-top">
              <span class="state-pill {stateClass(it)}">{stateLabel(it)}</span>
              <span class="ghl-num">
                {#if it.is_pull_request}
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6"><circle cx="6" cy="6" r="2.5"/><circle cx="6" cy="18" r="2.5"/><circle cx="18" cy="18" r="2.5"/><path d="M6 8.5v7M8.5 6h7a3 3 0 0 1 3 3v6.5"/></svg>
                {:else}
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6"><circle cx="12" cy="12" r="9"/><circle cx="12" cy="12" r="2.5"/></svg>
                {/if}
                #{it.number}
              </span>
              {#if it.repo}
                <span class="ghl-repo" title={`${it.repo.owner}/${it.repo.name}`}>{it.repo.name}</span>
              {/if}
              <span class="ghl-time">{relativeTime(it.updated_at, p.now)}</span>
            </div>
            <div class="ghl-title">{it.title}</div>
            <div class="ghl-meta">
              {#if it.labels.length > 0}
                {#each it.labels.slice(0, 3) as l (l.name)}
                  <span class="ghl-label" style="background: #{l.color}22; border-color: #{l.color}55; color: #{l.color};">
                    {l.name}
                  </span>
                {/each}
                {#if it.labels.length > 3}<span class="ghl-label-more">+{it.labels.length - 3}</span>{/if}
              {/if}
              {#if it.author}
                <span class="ghl-ava" title={it.author.login}>
                  {#if it.author.avatar_url}
                    <img src={it.author.avatar_url} alt={it.author.login} loading="lazy" />
                  {:else}
                    {(it.author.login || '?').slice(0, 1).toUpperCase()}
                  {/if}
                </span>
              {/if}
              {#if it.comments > 0}
                <span class="ghl-comments" title={`${it.comments} comments`}>
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7"><path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/></svg>
                  {it.comments}
                </span>
              {/if}
            </div>
            <span class="ghl-sends">
              <span
                class="ghl-send"
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
  .ghl { min-width: 0; }
  .ghl .spin { animation: ghl-spin 0.9s linear infinite; }
  @keyframes ghl-spin { to { transform: rotate(360deg); } }
  .lp-add:disabled { opacity: 0.5; cursor: not-allowed; }
  .lp-ghostbtn:disabled { opacity: 0.5; cursor: not-allowed; }
  .lp-chip:disabled { opacity: 0.45; cursor: not-allowed; }

  /* Search field — §2.4: h30 r8, bg-0 (light bg-2), border --border, text 12 faint. */
  .ghl-search {
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
  :root[data-theme='light'] .ghl-search { background: var(--bg-2); }
  .ghl-search:focus-within { border-color: var(--border-hi2, var(--border-hi)); }
  .ghl-search > svg {
    width: 12px; height: 12px;
    color: var(--text-mute);
    flex-shrink: 0;
  }
  /* Remote-search active — brand tint on the border so the user reads
     "this is hitting the GitHub Search API now". */
  .ghl-search.ghl-search--remote {
    border-color: color-mix(in srgb, var(--src-github) 36%, var(--border));
    box-shadow: 0 0 0 1px color-mix(in srgb, var(--src-github) 18%, transparent);
  }
  /* Spinner replacing the magnifier during a remote search — same 12px
     footprint so the input layout doesn't shift. */
  .ghl-search-spin {
    width: 12px; height: 12px;
    border: 1.5px solid color-mix(in srgb, var(--src-github) 24%, var(--border));
    border-top-color: var(--src-github);
    border-radius: 50%;
    animation: ghl-spin 0.7s linear infinite;
    flex-shrink: 0;
  }
  .ghl-search input {
    flex: 1; min-width: 0;
    background: transparent; border: 0; outline: 0;
    color: var(--text-0);
    font-size: 12px;
    font-family: inherit;
  }
  .ghl-search input::placeholder { color: var(--text-faint); }
  .ghl-search-clear {
    width: 16px; height: 16px;
    display: grid; place-items: center;
    border: 0; background: transparent;
    color: var(--text-mute);
    cursor: pointer;
    border-radius: 4px;
  }
  .ghl-search-clear:hover { color: var(--text-0); background: var(--bg-3); }
  .ghl-search-clear svg { width: 10px; height: 10px; }

  /* Filter dropdowns — align the shared Dropdown trigger to the §2.4
     chip scale (pad 3/9, r6, 11px, --text-mute; active = border-hi2 +
     accent-soft + text-0). */
  .ghl-dd { display: inline-flex; }
  .ghl-dd :global(.dd-trigger) {
    border-radius: 6px;
    border: 1px solid var(--border);
    background: transparent;
    color: var(--text-mute);
    font-size: 11px;
    height: auto;
    padding: 3px 9px;
    transition: color 120ms, border-color 120ms, background 120ms;
  }
  .ghl-dd :global(.dd-trigger:hover) { color: var(--text-1); }
  .ghl-dd :global(.dd-trigger[aria-expanded="true"]) {
    border-color: var(--border-hi2, var(--border-hi));
    background: var(--accent-soft);
    color: var(--text-0);
  }

  .ghl-chip-clear {
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
  .ghl-chip-clear:hover { color: var(--text-0); border-color: var(--text-mute); border-style: solid; }

  /* Row = shared .lp-row shell; .ghl-row adds the vertical stack. */
  .ghl-row {
    position: relative;
    display: flex; flex-direction: column; gap: 5px;
    user-select: none;
  }

  .ghl-top { display: flex; align-items: center; gap: 7px; flex-wrap: wrap; }
  .ghl-num {
    display: inline-flex; align-items: center; gap: 4px;
    font-size: 11px; color: var(--text-faint);
    font-family: var(--font-mono);
  }
  .ghl-num svg { width: 11px; height: 11px; color: var(--src-github); }
  .ghl-repo {
    font-size: 11px; color: var(--text-faint);
    font-family: var(--font-mono);
    max-width: 110px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .ghl-time {
    margin-left: auto;
    font-size: 11px; color: var(--text-faint);
    font-family: var(--font-mono);
  }
  /* Title 13 --text-1; active row → 600 --text-0 (§2.4). */
  .ghl-title {
    font-size: 13px; color: var(--text-1);
    line-height: 1.4;
    display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical;
    overflow: hidden;
  }
  .lp-row.active .ghl-title { color: var(--text-0); font-weight: 600; }
  .ghl-meta {
    display: flex; align-items: center; gap: 5px; flex-wrap: wrap;
    font-size: 11px; color: var(--text-2);
  }

  /* Label chip — background/border/text driven by GitHub's per-label
     color (data, injected inline), not a theme token. */
  .ghl-label {
    padding: 1px 6px;
    border-radius: 3px;
    font-size: 9.5px;
    border: 1px solid transparent;
    max-width: 100px;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .ghl-label-more { font-size: 10px; color: var(--text-faint); font-family: var(--font-mono); }

  .ghl-ava {
    width: 16px; height: 16px;
    border-radius: 50%;
    background: linear-gradient(135deg, var(--accent-bright), var(--accent-deep));
    color: var(--accent-fg);
    font-size: 9px; font-weight: 700;
    display: grid; place-items: center;
    overflow: hidden;
    flex-shrink: 0;
  }
  .ghl-ava img { width: 100%; height: 100%; object-fit: cover; }

  .ghl-comments {
    display: inline-flex; align-items: center; gap: 3px;
    font-size: 11px; color: var(--text-faint);
    font-family: var(--font-mono);
  }
  .ghl-comments svg { width: 10px; height: 10px; }

  /* Hover action — §2.4: dotted-underline 11px on the right, revealed
     on row hover/focus/active. role="button" span (a nested <button>
     would be invalid inside the row's own <button>). */
  .ghl-sends {
    position: absolute;
    top: 10px; right: 12px;
    display: inline-flex; gap: 10px;
    opacity: 0;
    transition: opacity 140ms;
  }
  .ghl-row:hover .ghl-sends,
  .ghl-sends:focus-within,
  .lp-row.active .ghl-sends {
    opacity: 1;
  }
  .ghl-send {
    font-size: 11px;
    color: var(--text-mute);
    text-decoration: underline dotted;
    text-underline-offset: 2px;
    cursor: pointer;
    user-select: none;
    background: transparent; border: 0; padding: 0;
  }
  .ghl-send:hover { color: var(--text-0); }

  .ghl-empty, .ghl-loading, .ghl-error {
    text-align: center;
    padding: 50px 20px;
    margin: auto;
  }
  .ghl-empty-h, .ghl-error-h {
    font-family: var(--font-mono);
    font-size: 22px; font-weight: 600; letter-spacing: -0.015em;
    color: var(--text-0);
    margin: 0 0 10px;
  }
  .ghl-empty-p, .ghl-error-p {
    font-size: 12.5px; color: var(--text-2);
    line-height: 1.55; margin: 0;
  }
  .ghl-error-p { color: var(--error); }
  .ghl-error-retry {
    margin-top: 14px;
    padding: 6px 12px;
    border-radius: 7px;
    font-size: 12px;
    background: var(--bg-2);
    border: 1px solid var(--border-hi);
    color: var(--text-1);
    cursor: pointer;
  }
  .ghl-error-retry:hover { color: var(--text-0); }
  .ghl-loading {
    display: flex; align-items: center; justify-content: center; gap: 10px;
    color: var(--text-2); font-size: 12px;
  }
  .ghl-spinner {
    width: 14px; height: 14px;
    border: 1.5px solid var(--border-hi);
    border-top-color: var(--accent);
    border-radius: 50%;
    animation: ghl-spin 0.7s linear infinite;
  }
</style>
