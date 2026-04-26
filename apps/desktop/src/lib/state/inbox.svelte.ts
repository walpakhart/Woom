// Inbox / focus state. Owns the GitHub involves-me inbox, the Jira issue
// inbox, the shared "currently-focused object" pointer used by both the
// GitHub focus pane and the Jira slide-over, and the detail shelf populated
// on focus-change (PR detail + files + commits + reviews + review comments
// + issue comments + check runs + expanded-file set + loading/error flags).
//
// The live clock (`now`) stays in +page.svelte because it's a cross-cutting
// timer that drives every relative-time label in the app, not just inbox.
//
// Handlers that fan out into view changes (e.g. openFocusItem sets focus
// AND flips the workbench view) are kept at the call site — this store only
// owns state that's cleanly "inbox / focus / detail" and the handlers that
// read/write only that slice.

import { invoke } from '@tauri-apps/api/core';
import type {
  ChangedFile,
  CheckRun,
  Comment,
  CommitEntry,
  InboxItem,
  JiraBoard,
  JiraItem,
  JiraProject,
  JiraSprint,
  JiraUserSummary,
  JiraWorkflowStatus,
  PrDetail,
  Review,
  ReviewComment,
  SentryEnvironment,
  SentryIssue,
  SentryProject
} from '$lib/data';
import { closeModal, modalsState, openModal, patchModal } from '$lib/state/modals.svelte';

// ---- GitHub filter state ----
//
// Filter mode corresponds to different GitHub search qualifiers (see the
// README on GitHub issue search). Default `involving` replicates the old
// `search_involves_me` behavior, so an empty/default filter → same results.
export type GithubFilterMode =
  | 'involving'
  | 'authored'
  | 'review_requested'
  | 'assigned'
  | 'user'
  /** No involvement qualifier — show every issue/PR in the selected
      repo/query scope. Useful when the user picks a specific repo and
      wants to browse the whole backlog, not just their own slice. */
  | 'all';

export interface GithubFilters {
  mode: GithubFilterMode;
  /** `owner/name` or null for all repos. */
  repo: string | null;
  /** Free-text search — appended verbatim to the GitHub `q=` string. */
  search: string;
  /** GitHub login when `mode === 'user'`. */
  customUser: string;
}

/** Sprint filter: numeric sprint id, `'backlog'` to restrict to `sprint is EMPTY`,
 *  or `null` for no sprint constraint at all. */
export type JiraSprintFilter = number | 'backlog' | null;

export interface JiraFilters {
  projectKey: string | null;
  /** Selected Jira boards. Multi-select: when more than one is picked,
   *  the JQL builder OR-merges their project keys (`project IN (…)`)
   *  so issues from every selected board's project show up in the
   *  same view. Sprint filter is only meaningful with exactly one
   *  board (sprints belong to a board) and is hidden / cleared
   *  otherwise. Empty array = no board filter ("All boards"). */
  boardIds: number[];
  sprintId: JiraSprintFilter;
  /** Literal workflow status name (`"BLOCKED"`, `"In Review"`, …) or `null`
   *  for "Any". When `null`, JQL does NOT constrain by resolution either —
   *  really show every ticket assigned to the user. */
  statusName: string | null;
  search: string;
}

const GITHUB_FILTERS_KEY = 'forgehold:github-filters:v1';
const JIRA_FILTERS_KEY = 'forgehold:jira-filters:v1';
const SENTRY_FILTERS_KEY = 'forgehold:sentry-filters:v1';

interface SentryFiltersPersisted {
  search: string;
  status: 'unresolved' | 'resolved' | 'ignored' | 'all';
  level: 'all' | 'fatal' | 'error' | 'warning' | 'info' | 'debug';
  projects: string[];
  environment: string | null;
  sort: 'date' | 'new' | 'priority' | 'freq' | 'user';
}

const DEFAULT_SENTRY_FILTERS: SentryFiltersPersisted = {
  search: '',
  status: 'unresolved',
  level: 'all',
  projects: [],
  environment: null,
  sort: 'date'
};

function readSentryFilters(): SentryFiltersPersisted {
  try {
    const raw = localStorage.getItem(SENTRY_FILTERS_KEY);
    if (!raw) return { ...DEFAULT_SENTRY_FILTERS };
    const parsed = JSON.parse(raw) as Partial<SentryFiltersPersisted>;
    return {
      search: typeof parsed.search === 'string' ? parsed.search : '',
      status:
        parsed.status === 'unresolved' ||
        parsed.status === 'resolved' ||
        parsed.status === 'ignored' ||
        parsed.status === 'all'
          ? parsed.status
          : 'unresolved',
      level:
        parsed.level === 'all' ||
        parsed.level === 'fatal' ||
        parsed.level === 'error' ||
        parsed.level === 'warning' ||
        parsed.level === 'info' ||
        parsed.level === 'debug'
          ? parsed.level
          : 'all',
      projects: Array.isArray(parsed.projects)
        ? parsed.projects.filter((p): p is string => typeof p === 'string')
        : [],
      environment: typeof parsed.environment === 'string' ? parsed.environment : null,
      sort:
        parsed.sort === 'date' ||
        parsed.sort === 'new' ||
        parsed.sort === 'priority' ||
        parsed.sort === 'freq' ||
        parsed.sort === 'user'
          ? parsed.sort
          : 'date'
    };
  } catch {
    return { ...DEFAULT_SENTRY_FILTERS };
  }
}

function persistSentryFilters() {
  try {
    const payload: SentryFiltersPersisted = {
      search: inboxState.sentrySearch,
      status: inboxState.sentryStatus,
      level: inboxState.sentryLevel,
      projects: inboxState.sentryProjects,
      environment: inboxState.sentryEnvironment,
      sort: inboxState.sentrySort
    };
    localStorage.setItem(SENTRY_FILTERS_KEY, JSON.stringify(payload));
  } catch {
    /* quota / SSR: ignore */
  }
}

const DEFAULT_GH_FILTERS: GithubFilters = {
  mode: 'involving',
  repo: null,
  search: '',
  customUser: ''
};

const DEFAULT_JIRA_FILTERS: JiraFilters = {
  projectKey: null,
  boardIds: [],
  sprintId: null,
  statusName: null,
  search: ''
};

function readGhFilters(): GithubFilters {
  try {
    const raw = localStorage.getItem(GITHUB_FILTERS_KEY);
    if (!raw) return { ...DEFAULT_GH_FILTERS };
    const parsed = JSON.parse(raw);
    return {
      mode: typeof parsed.mode === 'string' ? parsed.mode : 'involving',
      repo: typeof parsed.repo === 'string' ? parsed.repo : null,
      search: typeof parsed.search === 'string' ? parsed.search : '',
      customUser: typeof parsed.customUser === 'string' ? parsed.customUser : ''
    };
  } catch {
    return { ...DEFAULT_GH_FILTERS };
  }
}

function readJiraFilters(): JiraFilters {
  try {
    const raw = localStorage.getItem(JIRA_FILTERS_KEY);
    if (!raw) return { ...DEFAULT_JIRA_FILTERS };
    const parsed = JSON.parse(raw);
    const sprintRaw = parsed.sprintId;
    let sprintId: JiraSprintFilter = null;
    if (typeof sprintRaw === 'number') sprintId = sprintRaw;
    else if (sprintRaw === 'backlog') sprintId = 'backlog';
    // New payload shape persists a literal status name (or null for "Any").
    // Old payloads used a 4-value category enum (`any|todo|in_progress|done`)
    // under `status` — we can't reliably map those to a specific workflow
    // status per project, so just drop the legacy field and reset to "Any".
    const statusName =
      typeof parsed.statusName === 'string' && parsed.statusName.trim()
        ? parsed.statusName
        : null;
    // Migrate legacy single-board persisted shape (`boardId: number | null`)
    // into the new array shape (`boardIds: number[]`). Array form is
    // preferred — old single-board users land with [boardId], everyone
    // else with [].
    const boardIdsRaw = parsed.boardIds;
    let boardIds: number[] = [];
    if (Array.isArray(boardIdsRaw)) {
      boardIds = boardIdsRaw.filter((n): n is number => typeof n === 'number');
    } else if (typeof parsed.boardId === 'number') {
      boardIds = [parsed.boardId];
    }
    return {
      projectKey: typeof parsed.projectKey === 'string' ? parsed.projectKey : null,
      boardIds,
      sprintId,
      statusName,
      search: typeof parsed.search === 'string' ? parsed.search : ''
    };
  } catch {
    return { ...DEFAULT_JIRA_FILTERS };
  }
}

function persistGhFilters() {
  try {
    localStorage.setItem(GITHUB_FILTERS_KEY, JSON.stringify(inboxState.githubFilters));
  } catch {/* ignore */}
}

function persistJiraFilters() {
  try {
    localStorage.setItem(JIRA_FILTERS_KEY, JSON.stringify(inboxState.jiraFilters));
  } catch {/* ignore */}
}

export const inboxState = $state<{
  // GitHub inbox (involves-me list)
  items: InboxItem[];
  loading: boolean;
  error: string | null;

  // GitHub filter state (mode / repo / search). Persisted to localStorage
  // under `forgehold:github-filters:v1`.
  githubFilters: GithubFilters;
  /** Cached repo list for the repo-scope dropdown. Lazily populated. */
  githubRepoOptions: { owner: string; name: string; full_name: string }[];
  githubRepoOptionsLoading: boolean;

  // Shared focus across GitHub + Jira flows. Holds whatever PR/issue the
  // user last clicked from the GitHub column, or opened via the command
  // palette / repo view.
  focusItem: InboxItem | null;

  // Detail shelf for the focused item — PR metadata, files, commits,
  // reviews, inline review comments, issue comments, and check runs. All
  // populated in parallel by `loadDetail` on focus-change.
  prDetail: PrDetail | null;
  prFiles: ChangedFile[];
  prCommits: CommitEntry[];
  prReviews: Review[];
  prChecks: CheckRun[];
  prChecksLoading: boolean;
  reviewComments: ReviewComment[];
  comments: Comment[];
  detailLoading: boolean;
  detailError: string | null;
  expandedFiles: Set<string>;

  // Jira inbox
  jiraItems: JiraItem[];
  jiraItemsLoading: boolean;
  jiraItemsError: string | null;
  // Selected assignee to filter by. `null` + `jiraAssigneeAny=false` ⇒
  // authenticated account ("Me"). When `jiraAssigneeAny=true`, no assignee
  // constraint at all — show tickets for everyone in the workspace/scope.
  jiraAssignee: JiraUserSummary | null;
  jiraAssigneeAny: boolean;

  // Jira project/board/sprint filters. Persisted to localStorage under
  // `forgehold:jira-filters:v1`. Dropdown options are loaded lazily on
  // first-open so the column stays cheap when you don't touch filters.
  jiraFilters: JiraFilters;
  jiraProjectOptions: JiraProject[];
  jiraProjectOptionsLoading: boolean;
  jiraBoardOptions: JiraBoard[];
  jiraBoardOptionsLoading: boolean;
  jiraSprintOptions: JiraSprint[];
  jiraSprintOptionsLoading: boolean;
  jiraStatusOptions: JiraWorkflowStatus[];
  jiraStatusOptionsLoading: boolean;
  /** Project key the cached `jiraStatusOptions` were loaded for, so we
   *  can skip refetching when it already matches. `null` means "global". */
  jiraStatusOptionsProjectKey: string | null | undefined;

  // Jira detail slide-over — independent from `focusItem` because the Jira
  // column uses its own key-based slide-over (JiraDetailPane fetches by
  // issue key, not by InboxItem shape).
  jiraFocusKey: string | null;

  // Sentry inbox — issue list + filter dimensions. The filter state mirrors
  // Sentry's own UI: status (is:unresolved | is:resolved | is:ignored),
  // level (error/warning/info/debug/fatal), per-project scope (multi),
  // environment, sort key, free-text search. Filters compose into a single
  // `query` plus `project` + `environment` URL params on each refresh.
  sentryItems: SentryIssue[];
  sentryItemsLoading: boolean;
  sentryItemsError: string | null;
  /** Free-text search bar value (joined with structured filters at refresh). */
  sentrySearch: string;
  sentryStatus: 'unresolved' | 'resolved' | 'ignored' | 'all';
  sentryLevel: 'all' | 'fatal' | 'error' | 'warning' | 'info' | 'debug';
  sentryProjects: string[];
  sentryEnvironment: string | null;
  sentrySort: 'date' | 'new' | 'priority' | 'freq' | 'user';
  /** Project / env dropdown source data. */
  sentryProjectOptions: SentryProject[];
  sentryProjectOptionsLoading: boolean;
  sentryEnvironmentOptions: SentryEnvironment[];
  sentryEnvironmentOptionsLoading: boolean;
  /** Slide-over pane key — issue id of the currently focused issue. */
  sentryFocusId: string | null;
  /** When the agent (or a deep link) wants to land on a specific event
      of the focused issue rather than the latest. SentryDetailPane reads
      this on mount and passes it into `sentry_get_event_detail`.
      Null = use "latest" (default). Cleared whenever sentryFocusId
      changes so a stale event id doesn't follow you to another issue. */
  sentryFocusEventId: string | null;

  // ---- App-navigation channel (driven by `mcp__app__*` tools) ----
  // RepositoriesView watches `pendingRepoNav` and, when set, opens the
  // requested repo on the requested section, then nulls it back out. We
  // can't call into the view component directly because state is owned
  // there; this reactive channel lets the agent-driven navigation
  // tools land cleanly without a circular dep. Section is a hint —
  // RepositoriesView validates against its own RepoSection union.
  pendingRepoNav: { owner: string; repo: string; section: string } | null;
}>({
  items: [],
  loading: false,
  error: null,
  githubFilters: readGhFilters(),
  githubRepoOptions: [],
  githubRepoOptionsLoading: false,
  focusItem: null,
  prDetail: null,
  prFiles: [],
  prCommits: [],
  prReviews: [],
  prChecks: [],
  prChecksLoading: false,
  reviewComments: [],
  comments: [],
  detailLoading: false,
  detailError: null,
  expandedFiles: new Set(),
  jiraItems: [],
  jiraItemsLoading: false,
  jiraItemsError: null,
  jiraAssignee: null,
  jiraAssigneeAny: false,
  jiraFilters: readJiraFilters(),
  jiraProjectOptions: [],
  jiraProjectOptionsLoading: false,
  jiraBoardOptions: [],
  jiraBoardOptionsLoading: false,
  jiraSprintOptions: [],
  jiraSprintOptionsLoading: false,
  jiraStatusOptions: [],
  jiraStatusOptionsLoading: false,
  jiraStatusOptionsProjectKey: undefined,
  jiraFocusKey: null,
  sentryItems: [],
  sentryItemsLoading: false,
  sentryItemsError: null,
  ...(() => {
    const f = readSentryFilters();
    return {
      sentrySearch: f.search,
      sentryStatus: f.status,
      sentryLevel: f.level,
      sentryProjects: f.projects,
      sentryEnvironment: f.environment,
      sentrySort: f.sort
    };
  })(),
  sentryProjectOptions: [],
  sentryProjectOptionsLoading: false,
  sentryEnvironmentOptions: [],
  sentryEnvironmentOptionsLoading: false,
  sentryFocusId: null,
  sentryFocusEventId: null,
  pendingRepoNav: null
});

let userPickerDebounce: ReturnType<typeof setTimeout> | null = null;
let githubFilterDebounce: ReturnType<typeof setTimeout> | null = null;
let jiraFilterDebounce: ReturnType<typeof setTimeout> | null = null;

// ---- GitHub inbox ----

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

export async function refreshInbox({ silent = false }: { silent?: boolean } = {}) {
  if (!silent) inboxState.loading = true;
  inboxState.error = null;
  try {
    // If the filters are the default (everything unset, mode=involving), use
    // the dedicated `github_list_inbox` endpoint to preserve its exact
    // behavior (matches the `search_involves_me` call signature). Otherwise
    // fall through to the more general `github_search_inbox`.
    const f = inboxState.githubFilters;
    const usingDefault =
      f.mode === 'involving' && !f.repo && !f.search.trim() && !f.customUser.trim();
    if (usingDefault) {
      inboxState.items = await invoke<InboxItem[]>('github_list_inbox');
    } else {
      const q = buildGithubQuery(f, githubMeLogin);
      inboxState.items = await invoke<InboxItem[]>('github_search_inbox', { query: q });
    }
    // Intentionally do NOT auto-select an item — selection must be
    // explicit (click), otherwise polling re-opens the slide-over.
  } catch (e) {
    inboxState.error = typeof e === 'string' ? e : String(e);
  } finally {
    inboxState.loading = false;
  }
}

/** Patch the GitHub filter state, persist it, and re-run the search with a
 *  300 ms debounce so typing in the search box doesn't spam the API. */
export function updateGithubFilters(patch: Partial<GithubFilters>) {
  inboxState.githubFilters = { ...inboxState.githubFilters, ...patch };
  persistGhFilters();
  if (githubFilterDebounce) clearTimeout(githubFilterDebounce);
  githubFilterDebounce = setTimeout(() => void refreshInbox({ silent: true }), 300);
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

export function selectInboxItem(id: number) {
  const item = inboxState.items.find((i) => i.id === id);
  if (item) inboxState.focusItem = item;
}

/** Open an InboxItem in the focus pane. Does NOT switch views — callers are
 *  expected to toggle `view = 'workbench'` themselves if they need to. */
export function openFocusItem(item: InboxItem) {
  inboxState.focusItem = item;
}

export function closeFocusItem() {
  inboxState.focusItem = null;
}

/** j/k keyboard nav through the current inbox list. No-ops if there's no
    focus item or the current focus isn't in the list (e.g. it came from
    RepositoriesView). */
export function moveSelection(delta: number) {
  if (!inboxState.items.length || !inboxState.focusItem) return;
  const idx = inboxState.items.findIndex((i) => i.id === inboxState.focusItem!.id);
  if (idx < 0) return;
  const next = Math.max(0, Math.min(inboxState.items.length - 1, idx + delta));
  inboxState.focusItem = inboxState.items[next];
}

// ---- Detail shelf (PR / issue metadata on focus-change) ----

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
  await refreshInbox({ silent: true });
}

export function toggleFile(filename: string) {
  const next = new Set(inboxState.expandedFiles);
  if (next.has(filename)) next.delete(filename);
  else next.add(filename);
  inboxState.expandedFiles = next;
}

// ---- Jira inbox ----

/** Escape a double-quoted JQL string literal. Jira accepts backslash-escaped
 *  quotes inside `"..."` values. */
function jqlEscape(s: string): string {
  return s.replace(/\\/g, '\\\\').replace(/"/g, '\\"');
}

/** Build the JQL query from the current filter + assignee state.
 *
 *  When no status is selected ("Any"), we intentionally do NOT add
 *  `resolution = Unresolved` — the user asked for every ticket assigned to
 *  them, Done included. The old behavior hid resolved tickets and violated
 *  the principle of least surprise.
 *
 *  `assigneeAny=true` means no assignee clause at all — browse everyone's
 *  tickets. `assignee=null, assigneeAny=false` means `currentUser()` ("Me"). */
export function buildJiraJql(
  filters: JiraFilters,
  assignee: JiraUserSummary | null,
  assigneeAny: boolean = false
): string {
  const parts: string[] = [];
  if (filters.projectKey) {
    parts.push(`project = "${jqlEscape(filters.projectKey)}"`);
  } else if (filters.boardIds.length > 0) {
    // Project filter takes precedence — when explicit `projectKey` is
    // set, the user has already drilled into one project; respect that.
    // Otherwise translate selected boards to their backing project keys
    // and OR-merge so issues from every selected board's project show.
    // Boards without a known project_key are skipped (rare — happens
    // before `loadJiraBoards` populates options, or when a saved board
    // id was deleted upstream and we haven't re-fetched).
    const projectKeys = new Set<string>();
    for (const bid of filters.boardIds) {
      const board = inboxState.jiraBoardOptions.find((b) => b.id === bid);
      if (board?.project_key) projectKeys.add(board.project_key);
    }
    if (projectKeys.size === 1) {
      parts.push(`project = "${jqlEscape([...projectKeys][0])}"`);
    } else if (projectKeys.size > 1) {
      const list = [...projectKeys].map((k) => `"${jqlEscape(k)}"`).join(', ');
      parts.push(`project IN (${list})`);
    }
  }
  // Sprint clause is per-board; only meaningful with exactly one board
  // selected. Multi-board scope drops it (UI hides the sprint dropdown
  // in that case too — see JiraColumn.svelte).
  if (filters.boardIds.length <= 1) {
    if (filters.sprintId === 'backlog') {
      parts.push('sprint is EMPTY');
    } else if (typeof filters.sprintId === 'number') {
      parts.push(`sprint = ${filters.sprintId}`);
    }
  }
  if (filters.statusName) {
    parts.push(`status = "${jqlEscape(filters.statusName)}"`);
  }
  if (assigneeAny) {
    // No assignee clause.
  } else if (assignee) {
    parts.push(`assignee = "${jqlEscape(assignee.account_id)}"`);
  } else {
    parts.push('assignee = currentUser()');
  }
  const q = filters.search.trim();
  if (q) {
    const esc = jqlEscape(q);
    parts.push(`(summary ~ "${esc}" OR description ~ "${esc}")`);
  }
  // Empty JQL (assignee=Any + no filters) would be rejected by Jira; fall
  // back to a harmless always-true predicate so the search still runs.
  if (parts.length === 0) {
    return 'ORDER BY updated DESC';
  }
  return `${parts.join(' AND ')} ORDER BY updated DESC`;
}

export async function refreshJiraInbox({ silent = false }: { silent?: boolean } = {}) {
  if (!silent) inboxState.jiraItemsLoading = true;
  inboxState.jiraItemsError = null;
  try {
    const f = inboxState.jiraFilters;
    // "Any" assignee can't use the specific-account fast path — it needs the
    // no-assignee-clause JQL route. Same for non-default filters.
    const usingDefault =
      !f.projectKey &&
      f.boardIds.length === 0 &&
      f.sprintId == null &&
      f.statusName == null &&
      !f.search.trim() &&
      !inboxState.jiraAssigneeAny;
    // Fast path: preserve the old single-shot `jira_list_inbox_for` call
    // when the user hasn't touched the new filters. Backend is equivalent
    // but this keeps the wire format identical to what shipped before.
    if (usingDefault) {
      inboxState.jiraItems = await invoke<JiraItem[]>('jira_list_inbox_for', {
        assigneeAccountId: inboxState.jiraAssignee?.account_id ?? null
      });
    } else {
      const jql = buildJiraJql(f, inboxState.jiraAssignee, inboxState.jiraAssigneeAny);
      inboxState.jiraItems = await invoke<JiraItem[]>('jira_search', { jql });
    }
  } catch (e) {
    inboxState.jiraItemsError = typeof e === 'string' ? e : String(e);
  } finally {
    inboxState.jiraItemsLoading = false;
  }
}

/** Patch Jira filter state, persist, and re-run the search (debounced 300 ms). */
export function updateJiraFilters(patch: Partial<JiraFilters>) {
  inboxState.jiraFilters = { ...inboxState.jiraFilters, ...patch };
  persistJiraFilters();
  if (jiraFilterDebounce) clearTimeout(jiraFilterDebounce);
  jiraFilterDebounce = setTimeout(() => void refreshJiraInbox({ silent: true }), 300);
}

export async function loadJiraProjects() {
  if (inboxState.jiraProjectOptions.length || inboxState.jiraProjectOptionsLoading) return;
  inboxState.jiraProjectOptionsLoading = true;
  try {
    inboxState.jiraProjectOptions = await invoke<JiraProject[]>('jira_list_projects');
  } catch {
    inboxState.jiraProjectOptions = [];
  } finally {
    inboxState.jiraProjectOptionsLoading = false;
  }
}

export async function loadJiraBoards(projectKey: string | null) {
  inboxState.jiraBoardOptionsLoading = true;
  try {
    inboxState.jiraBoardOptions = await invoke<JiraBoard[]>('jira_list_boards', {
      projectKey
    });
  } catch {
    inboxState.jiraBoardOptions = [];
  } finally {
    inboxState.jiraBoardOptionsLoading = false;
  }
}

export async function loadJiraSprints(boardId: number | null) {
  if (boardId == null) {
    inboxState.jiraSprintOptions = [];
    return;
  }
  inboxState.jiraSprintOptionsLoading = true;
  try {
    inboxState.jiraSprintOptions = await invoke<JiraSprint[]>('jira_list_sprints', {
      boardId
    });
  } catch {
    inboxState.jiraSprintOptions = [];
  } finally {
    inboxState.jiraSprintOptionsLoading = false;
  }
}

/** Fetch the workflow statuses for a project (or global if `projectKey` is
 *  null). Cached per-project — repeated calls with the same key are no-ops.
 *  Callers should pass `null` to request the global list. */
export async function loadJiraStatuses(projectKey: string | null) {
  if (
    inboxState.jiraStatusOptionsProjectKey === projectKey &&
    inboxState.jiraStatusOptions.length
  ) {
    return;
  }
  if (inboxState.jiraStatusOptionsLoading) return;
  inboxState.jiraStatusOptionsLoading = true;
  try {
    inboxState.jiraStatusOptions = await invoke<JiraWorkflowStatus[]>('jira_list_statuses', {
      projectKey
    });
    inboxState.jiraStatusOptionsProjectKey = projectKey;
  } catch {
    inboxState.jiraStatusOptions = [];
    inboxState.jiraStatusOptionsProjectKey = projectKey;
  } finally {
    inboxState.jiraStatusOptionsLoading = false;
  }
}

/** Drop the cached status list — called when the selected project changes
 *  so the next open re-fetches from the right project. */
export function invalidateJiraStatuses() {
  inboxState.jiraStatusOptions = [];
  inboxState.jiraStatusOptionsProjectKey = undefined;
}

export function openUserPicker() {
  openModal('userPicker', { query: '', results: [], loading: true, error: null });
  void searchJiraUsers('');
}

export function onUserPickerInput(q: string) {
  if (!modalsState.userPicker) return;
  patchModal('userPicker', { query: q });
  if (userPickerDebounce) clearTimeout(userPickerDebounce);
  userPickerDebounce = setTimeout(() => void searchJiraUsers(q), 250);
}

async function searchJiraUsers(q: string) {
  if (!modalsState.userPicker) return;
  patchModal('userPicker', { loading: true, error: null });
  try {
    const results = await invoke<JiraUserSummary[]>('jira_search_users', { query: q });
    patchModal('userPicker', { results, loading: false });
  } catch (e) {
    patchModal('userPicker', { loading: false, error: typeof e === 'string' ? e : String(e) });
  }
}

export async function selectAssignee(u: JiraUserSummary | null) {
  inboxState.jiraAssignee = u;
  inboxState.jiraAssigneeAny = false;
  closeModal('userPicker');
  await refreshJiraInbox();
}

/** Drop the assignee constraint entirely — show tickets for everyone in the
 *  active project/board/sprint scope. */
export async function selectAnyAssignee() {
  inboxState.jiraAssignee = null;
  inboxState.jiraAssigneeAny = true;
  closeModal('userPicker');
  await refreshJiraInbox();
}

// ---- Disconnect resets ----
// Called by +page.svelte when the user disconnects a source, or from the
// connect-state effect when the auth check reveals we've lost credentials.
// State tied to the disconnected source is wiped so stale data doesn't
// leak into the UI.

/** Open the Sentry slide-over on `id`, optionally on a specific event.
 *  Always sets both `sentryFocusId` and `sentryFocusEventId` atomically
 *  so a stale event id from a previous open doesn't leak into the new
 *  one (which would otherwise 404 from `sentry_get_event_detail`). */
export function openSentryFocus(id: string | null, eventId: string | null = null) {
  inboxState.sentryFocusEventId = eventId;
  inboxState.sentryFocusId = id;
}

export function resetGithubInbox() {
  inboxState.items = [];
  inboxState.focusItem = null;
}

export function resetJiraInbox() {
  inboxState.jiraItems = [];
  inboxState.jiraAssignee = null;
  inboxState.jiraAssigneeAny = false;
  inboxState.jiraStatusOptions = [];
  inboxState.jiraStatusOptionsProjectKey = undefined;
}

// ---- Sentry inbox ----

/** Compose the `query=` string from the current structured filter state.
 *  Empty status/level slots translate to "no qualifier" (Sentry default
 *  matches everything when no `is:` is present). Free-text search is
 *  appended last so terms like `User-Agent` aren't parsed as a column. */
export function buildSentryQuery(): string {
  const parts: string[] = [];
  const { sentryStatus, sentryLevel, sentrySearch } = inboxState;
  if (sentryStatus !== 'all') parts.push(`is:${sentryStatus}`);
  if (sentryLevel !== 'all') parts.push(`level:${sentryLevel}`);
  const search = sentrySearch.trim();
  if (search) parts.push(search);
  return parts.join(' ');
}

let sentryFilterDebounce: ReturnType<typeof setTimeout> | null = null;

export async function refreshSentryInbox({ silent = false }: { silent?: boolean } = {}) {
  if (!silent) inboxState.sentryItemsLoading = true;
  inboxState.sentryItemsError = null;
  try {
    const items = await invoke<SentryIssue[]>('sentry_list_issues', {
      query: buildSentryQuery() || null,
      projectSlugs: inboxState.sentryProjects,
      environment: inboxState.sentryEnvironment,
      sort: inboxState.sentrySort,
      limit: 50
    });
    inboxState.sentryItems = items;
  } catch (e) {
    inboxState.sentryItemsError = typeof e === 'string' ? e : String(e);
  } finally {
    inboxState.sentryItemsLoading = false;
  }
}

/** Schedule a debounced refresh after a filter change (250ms). Avoids
 *  hammering the API while the user types in the search box. Also
 *  persists the new filter shape to localStorage so the user comes back
 *  to the same view after restart (mirrors Jira / GitHub behavior). */
export function scheduleSentryFilterRefresh() {
  persistSentryFilters();
  if (sentryFilterDebounce) clearTimeout(sentryFilterDebounce);
  sentryFilterDebounce = setTimeout(() => void refreshSentryInbox({ silent: true }), 250);
}

export async function loadSentryProjects() {
  if (inboxState.sentryProjectOptionsLoading) return;
  inboxState.sentryProjectOptionsLoading = true;
  try {
    inboxState.sentryProjectOptions = await invoke<SentryProject[]>('sentry_list_projects');
  } catch {
    // Silent — the project picker just shows "no projects" until refreshed.
  } finally {
    inboxState.sentryProjectOptionsLoading = false;
  }
}

/** Pull environments for whatever projects are currently selected. With
 *  no project picked we fall back to the first member-project so the
 *  dropdown isn't empty. */
export async function loadSentryEnvironments() {
  const slug =
    inboxState.sentryProjects[0] ??
    inboxState.sentryProjectOptions.find((p) => p.is_member)?.slug ??
    inboxState.sentryProjectOptions[0]?.slug ??
    null;
  if (!slug) {
    inboxState.sentryEnvironmentOptions = [];
    return;
  }
  inboxState.sentryEnvironmentOptionsLoading = true;
  try {
    inboxState.sentryEnvironmentOptions = await invoke<SentryEnvironment[]>(
      'sentry_list_environments',
      { projectSlug: slug }
    );
  } catch {
    inboxState.sentryEnvironmentOptions = [];
  } finally {
    inboxState.sentryEnvironmentOptionsLoading = false;
  }
}

export function resetSentryInbox() {
  inboxState.sentryItems = [];
  inboxState.sentryItemsError = null;
  inboxState.sentryFocusId = null;
}
