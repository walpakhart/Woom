/* GitHub-side inbox actions: refreshing the per-column item list,
 * building the GitHub search `q=` string from filter state, the
 * focus / detail-shelf flow on item click. Reads & writes
 * `inboxState` from `inbox-shared.svelte.ts`. No reactive runes here,
 * so this can stay a plain `.ts` file.
 *
 * The detail-shelf code (`loadDetail` + `loadPrChecks`) doesn't fan
 * out to the focused-overlay view-flip — that stays in `+page.svelte`
 * because flipping `view` is page-local. */

import { invoke } from '@tauri-apps/api/core';
import type {
  ChangedFile,
  CheckRun,
  Comment,
  CommitEntry,
  InboxItem,
  PrDetail,
  Review,
  ReviewComment
} from '$lib/data';
import { APP_INSTANCE_IDS } from '$lib/state/layout.svelte';
import {
  DEFAULT_GH_FILTERS,
  inboxState,
  persistGhFilters,
  type GithubFilters
} from './inbox-shared.svelte';

/** Shell-quote a GitHub login/username so spaces/special chars in free-form
 *  search text don't accidentally bleed into a qualifier. */
function ghEscape(s: string): string {
  return s.trim().replace(/\s+/g, '');
}

/** Compose the GitHub search `q=` string from the current filter state.
 *  `me` is the authenticated user's login — used for @me substitution because
 *  GitHub's `@me` qualifier only works for some fields. */
export function buildGithubQuery(filters: GithubFilters, me: string | null): string {
  const parts: string[] = ['is:open'];
  const meLogin = me ? ghEscape(me) : null;
  switch (filters.mode) {
    case 'authored':
      parts.push(meLogin ? `author:${meLogin}` : 'author:@me');
      break;
    case 'review_requested':
      parts.push(meLogin ? `review-requested:${meLogin}` : 'review-requested:@me');
      break;
    case 'assigned':
      parts.push(meLogin ? `assignee:${meLogin}` : 'assignee:@me');
      break;
    case 'user': {
      const u = ghEscape(filters.customUser);
      if (u) parts.push(`involves:${u}`);
      else parts.push('involves:@me');
      break;
    }
    case 'all':
      // No author/assignee/reviewer qualifier — rely on repo + search only.
      break;
    case 'involving':
    default:
      parts.push(meLogin ? `involves:${meLogin}` : 'involves:@me');
  }
  if (filters.repo) parts.push(`repo:${filters.repo}`);
  const q = filters.search.trim();
  if (q) parts.push(q);
  return parts.join(' ');
}

/** Auth'd user's GitHub login — set by +page.svelte whenever `githubStatus`
 *  resolves. Used by `buildGithubQuery` to hydrate `author:<me>` etc. */
let githubMeLogin: string | null = null;
export function setGithubMeLogin(login: string | null) {
  githubMeLogin = login;
}

/* Per-instance filter accessors. Reads return a frozen default if the
   slot doesn't exist yet (e.g. the column just mounted) — first write
   creates the entry. Items / loading / error follow the same pattern. */
export function githubFiltersFor(instanceId: string): GithubFilters {
  return inboxState.githubFiltersByInstance[instanceId] ?? DEFAULT_GH_FILTERS;
}
export function githubItemsFor(instanceId: string): InboxItem[] {
  return inboxState.itemsByInstance[instanceId] ?? [];
}
export function githubLoadingFor(instanceId: string): boolean {
  return inboxState.loadingByInstance[instanceId] ?? false;
}
export function githubErrorFor(instanceId: string): string | null {
  return inboxState.errorByInstance[instanceId] ?? null;
}

export async function refreshInbox(
  instanceId: string,
  { silent = false }: { silent?: boolean } = {}
) {
  if (!silent) inboxState.loadingByInstance[instanceId] = true;
  inboxState.errorByInstance[instanceId] = null;
  try {
    const f = githubFiltersFor(instanceId);
    const usingDefault =
      f.mode === 'involving' && !f.repo && !f.search.trim() && !f.customUser.trim();
    if (usingDefault) {
      inboxState.itemsByInstance[instanceId] = await invoke<InboxItem[]>(
        'github_list_inbox'
      );
    } else {
      const q = buildGithubQuery(f, githubMeLogin);
      inboxState.itemsByInstance[instanceId] = await invoke<InboxItem[]>(
        'github_search_inbox',
        { query: q }
      );
    }
  } catch (e) {
    inboxState.errorByInstance[instanceId] = typeof e === 'string' ? e : String(e);
  } finally {
    inboxState.loadingByInstance[instanceId] = false;
  }
}

/** Refresh every GitHub column on every solo. Used by page-level
 *  handlers (bootstrap on connect, after a PR is created, etc.) that
 *  don't have a specific instanceId in scope. */
export async function refreshAllInboxes(opts: { silent?: boolean } = {}) {
  const ids = [APP_INSTANCE_IDS.github];
  await Promise.all(ids.map((id) => refreshInbox(id, opts)));
}

const githubFilterDebounces: Map<string, ReturnType<typeof setTimeout>> = new Map();

/** Patch this column's filter state, persist it, and re-run the search
 *  with a 300 ms debounce so typing in the search box doesn't spam. */
export function updateGithubFilters(instanceId: string, patch: Partial<GithubFilters>) {
  inboxState.githubFiltersByInstance[instanceId] = {
    ...githubFiltersFor(instanceId),
    ...patch
  };
  persistGhFilters();
  const t = githubFilterDebounces.get(instanceId);
  if (t) clearTimeout(t);
  githubFilterDebounces.set(
    instanceId,
    setTimeout(() => void refreshInbox(instanceId, { silent: true }), 300)
  );
}

/** Patch only the UI-side filter fields (uiQuery / uiRoleFilter /
 *  uiStateFilter / uiRepoFilter / uiAuthorFilter) and persist. Skips
 *  the API refresh that `updateGithubFilters` schedules — these
 *  fields filter already-loaded items and don't affect the GitHub
 *  search query, so a network round-trip would be wasted. Pair with
 *  the existing GithubList local-state usage to make filters survive
 *  solo switches. */
export function updateGithubUiFilters(instanceId: string, patch: Partial<GithubFilters>) {
  inboxState.githubFiltersByInstance[instanceId] = {
    ...githubFiltersFor(instanceId),
    ...patch
  };
  persistGhFilters();
}

/** Field-level diff-and-write helper matching `persistJiraUiFilters`.
 *  Called from a $effect in GithubList — reads each local state
 *  field and writes ONLY changed ones back into the persistent slot.
 *  Avoids the "every keystroke rebinds the proxy" symptom where deep
 *  effects watching the filter object would otherwise re-fire on
 *  every typing event. */
export function persistGithubUiFilters(
  instanceId: string,
  patch: {
    uiQuery: string;
    uiRoleFilter: 'reviewer' | null;
    uiStateFilter: 'open' | 'draft' | null;
    uiRepoFilter: string | null;
    uiAuthorFilter: string | null;
  }
) {
  const current = githubFiltersFor(instanceId);
  let changed = false;
  const next: GithubFilters = { ...current };
  if (current.uiQuery !== patch.uiQuery) { next.uiQuery = patch.uiQuery; changed = true; }
  if (current.uiRoleFilter !== patch.uiRoleFilter) { next.uiRoleFilter = patch.uiRoleFilter; changed = true; }
  if (current.uiStateFilter !== patch.uiStateFilter) { next.uiStateFilter = patch.uiStateFilter; changed = true; }
  if (current.uiRepoFilter !== patch.uiRepoFilter) { next.uiRepoFilter = patch.uiRepoFilter; changed = true; }
  if (current.uiAuthorFilter !== patch.uiAuthorFilter) { next.uiAuthorFilter = patch.uiAuthorFilter; changed = true; }
  if (changed) {
    inboxState.githubFiltersByInstance[instanceId] = next;
    persistGhFilters();
  }
}

export async function loadGithubRepoOptions() {
  if (inboxState.githubRepoOptions.length || inboxState.githubRepoOptionsLoading) return;
  inboxState.githubRepoOptionsLoading = true;
  try {
    type RawRepo = { full_name: string; owner: string; name: string };
    const repos = await invoke<RawRepo[]>('github_list_repos');
    inboxState.githubRepoOptions = repos.map((r) => ({
      owner: r.owner,
      name: r.name,
      full_name: r.full_name
    }));
  } catch {
    inboxState.githubRepoOptions = [];
  } finally {
    inboxState.githubRepoOptionsLoading = false;
  }
}

/* Resolve `id` against every column's items list — item ids are
   process-global (GitHub returns the same numeric id everywhere), so
   the FIRST instance to have the item wins. Used by the palette and
   agent-driven nav, neither of which knows which column the user
   wants to focus. */
export function selectInboxItem(id: number) {
  for (const list of Object.values(inboxState.itemsByInstance)) {
    const item = list.find((i) => i.id === id);
    if (item) {
      inboxState.focusItem = item;
      return;
    }
  }
}

/** Open an InboxItem in the focus pane. Does NOT switch views — callers are
 *  expected to flip the rail view themselves if they need to. */
export function openFocusItem(item: InboxItem) {
  inboxState.focusItem = item;
}

export function closeFocusItem() {
  inboxState.focusItem = null;
}

/** j/k keyboard nav through the inbox list of whichever GitHub column
    holds the focused item. No-ops if there's no focus item or the
    focus isn't in any column's list (e.g. it came from GithubTab). */
export function moveSelection(delta: number) {
  if (!inboxState.focusItem) return;
  const focusId = inboxState.focusItem.id;
  for (const list of Object.values(inboxState.itemsByInstance)) {
    const idx = list.findIndex((i) => i.id === focusId);
    if (idx < 0) continue;
    const next = Math.max(0, Math.min(list.length - 1, idx + delta));
    inboxState.focusItem = list[next];
    return;
  }
}

// ---- Detail shelf (PR / issue metadata on focus-change) -------------

export async function loadDetail() {
  const item = inboxState.focusItem;
  if (!item || !item.repo) {
    inboxState.prDetail = null;
    inboxState.prFiles = [];
    inboxState.prCommits = [];
    inboxState.prReviews = [];
    inboxState.reviewComments = [];
    inboxState.comments = [];
    inboxState.prChecks = [];
    return;
  }
  inboxState.detailLoading = true;
  inboxState.detailError = null;
  inboxState.expandedFiles = new Set();
  inboxState.prDetail = null;
  inboxState.prFiles = [];
  inboxState.prCommits = [];
  inboxState.prReviews = [];
  inboxState.reviewComments = [];
  inboxState.comments = [];
  inboxState.prChecks = [];

  const { owner, name } = item.repo;
  const number = item.number;
  const isPr = item.is_pull_request;

  const args = { owner, repo: name, number };
  const errors: string[] = [];

  async function load<T>(cmd: string, setter: (v: T) => void) {
    try {
      setter(await invoke<T>(cmd, args));
    } catch (e) {
      const msg = typeof e === 'string' ? e : String(e);
      console.error(`${cmd} failed:`, msg);
      errors.push(`${cmd}: ${msg}`);
    }
  }

  const tasks: Promise<void>[] = [
    load<Comment[]>('github_list_comments', (v) => (inboxState.comments = v))
  ];
  if (isPr) {
    tasks.push(
      load<PrDetail>('github_get_pr', (v) => (inboxState.prDetail = v)),
      load<ChangedFile[]>('github_list_pr_files', (v) => (inboxState.prFiles = v)),
      load<CommitEntry[]>('github_list_pr_commits', (v) => (inboxState.prCommits = v)),
      load<Review[]>('github_list_pr_reviews', (v) => (inboxState.prReviews = v)),
      load<ReviewComment[]>('github_list_review_comments', (v) => (inboxState.reviewComments = v))
    );
  }
  await Promise.all(tasks);
  if (errors.length) inboxState.detailError = errors.join(' · ');
  inboxState.detailLoading = false;

  // Fire a secondary fetch for check runs once we know the PR's head SHA.
  // The last element in `prCommits` is the PR head — GitHub's check-runs
  // API wants a SHA (refs from forks may 404), so we use that.
  if (isPr && inboxState.prCommits.length) {
    const head = inboxState.prCommits[inboxState.prCommits.length - 1].sha;
    void loadPrChecks(owner, name, head);
  }
}

async function loadPrChecks(owner: string, repo: string, sha: string) {
  inboxState.prChecksLoading = true;
  try {
    inboxState.prChecks = await invoke<CheckRun[]>('github_list_check_runs', {
      owner,
      repo,
      reference: sha
    });
  } catch (e) {
    console.error('github_list_check_runs failed:', e);
    inboxState.prChecks = [];
  } finally {
    inboxState.prChecksLoading = false;
  }
}

/** After a mutating action on the focused item (comment/review/merge/close)
    re-pull detail so the UI reflects server state, then re-poll the inbox
    list so the item's state/updated_at freshens everywhere it's shown. */
export async function reloadDetailAndLists() {
  await loadDetail();
  await refreshAllInboxes({ silent: true });
}

export function toggleFile(filename: string) {
  const next = new Set(inboxState.expandedFiles);
  if (next.has(filename)) next.delete(filename);
  else next.add(filename);
  inboxState.expandedFiles = next;
}

// ---- Disconnect reset ------------------------------------------------

export function resetGithubInbox() {
  inboxState.itemsByInstance = {};
  inboxState.loadingByInstance = {};
  inboxState.errorByInstance = {};
  inboxState.focusItem = null;
}
