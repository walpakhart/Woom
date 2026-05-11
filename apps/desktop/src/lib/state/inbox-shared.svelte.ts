/* Inbox-state spine: types, defaults, localStorage persistence, the
 * single `inboxState` reactive store, and the instance-removed cleanup
 * hook. Source-specific behaviour (refreshing a column, building a
 * query, opening a focus pane) lives in `inbox-github.ts` /
 * `inbox-jira.ts` / `inbox-sentry.ts`; those modules import
 * `inboxState` from here and write to their own slice.
 *
 * This file is `.svelte.ts` because of `$state()`. The split modules
 * (which only read/write the store) are plain `.ts`.
 *
 * `inbox.svelte.ts` re-exports everything from the four files so
 * existing `$lib/state/inbox.svelte` import paths keep working
 * unchanged. Adding a new field to the store still happens here. */

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
import { registerInstanceRemovedHook } from '$lib/state/layout.svelte';

// ---- Filter shapes ---------------------------------------------------

/** GitHub filter mode — corresponds to different GitHub search
 *  qualifiers (see the README on GitHub issue search). Default
 *  `involving` replicates the old `search_involves_me` behaviour, so
 *  an empty/default filter → same results. */
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

/** A single sprint scope: a numeric sprint id, OR the literal
 *  `'backlog'` for `sprint is EMPTY`. The filter holds an array of
 *  these — UI lets the user toggle multiple sprints (and/or backlog)
 *  on at once. */
export type SprintScope = number | 'backlog';

export interface JiraFilters {
  projectKey: string | null;
  /** Selected Jira boards. Multi-select: when more than one is picked,
   *  the JQL builder OR-merges their project keys (`project IN (…)`)
   *  so issues from every selected board's project show up in the
   *  same view. Sprint filter is only meaningful with exactly one
   *  board (sprints belong to a board) and is hidden / cleared
   *  otherwise. Empty array = no board filter ("All boards"). */
  boardIds: number[];
  /** Selected sprints (or backlog). Multi-select within a single
   *  board — JQL becomes `sprint IN (id1, id2)`, plus an
   *  `OR sprint is EMPTY` if backlog is in the mix. Empty array =
   *  no sprint filter ("Any sprint"). Cleared whenever the board
   *  set changes since sprint ids are board-scoped. */
  sprintIds: SprintScope[];
  /** Literal workflow status name (`"BLOCKED"`, `"In Review"`, …) or `null`
   *  for "Any". When `null`, JQL does NOT constrain by resolution either —
   *  really show every ticket assigned to the user. */
  statusName: string | null;
  search: string;
  /** Client-side UI filter state persisted so the list view survives
   *  unmount/remount. These fields are NOT used in JQL — they filter
   *  the already-loaded items in JiraList. Assignee is global (lives in
   *  `inboxState.jiraAssignee` / `jiraAssigneeAny`) and drives JQL, so
   *  it's not duplicated here. */
  uiQuery: string;
  uiRoleFilter: 'reporter' | null;
  uiStatusFilter: 'open' | 'inprogress' | 'done' | null;
  uiProjectFilter: string | null;
}

export interface SentryFiltersPersisted {
  search: string;
  status: 'unresolved' | 'resolved' | 'ignored' | 'all';
  level: 'all' | 'fatal' | 'error' | 'warning' | 'info' | 'debug';
  projects: string[];
  environment: string | null;
  sort: 'date' | 'new' | 'priority' | 'freq' | 'user';
}

// ---- Persistence keys ------------------------------------------------

/* Per-column-instance filter persistence — one key per source, payload
   is `Record<instanceId, FilterShape>`. Each column owns one entry,
   `cleanupInstanceState` drops the entry when the instance is closed. */
const GH_COL_FILTERS_KEY = 'woom:github-col-filters-by-instance:v1';
const JIRA_COL_FILTERS_KEY = 'woom:jira-col-filters-by-instance:v1';
const SENTRY_COL_FILTERS_KEY = 'woom:sentry-col-filters-by-instance:v1';
/* Tabs (JiraTab / SentryTab) keep their own filter slice so changing
   a board / project / status in the dedicated tab doesn't yank the
   solo app out from under the user (and vice-versa). Each tab
   persists separately so a reload restores both states independently. */
const JIRA_TAB_FILTERS_KEY = 'woom:jira-tab-filters:v1';
const SENTRY_TAB_FILTERS_KEY = 'woom:sentry-tab-filters:v1';

// ---- Defaults --------------------------------------------------------

export const DEFAULT_GH_FILTERS: GithubFilters = {
  mode: 'involving',
  repo: null,
  search: '',
  customUser: ''
};

export const DEFAULT_JIRA_FILTERS: JiraFilters = {
  projectKey: null,
  boardIds: [],
  sprintIds: [],
  statusName: null,
  search: '',
  uiQuery: '',
  uiRoleFilter: null,
  uiStatusFilter: null,
  uiProjectFilter: null
};

export const DEFAULT_SENTRY_FILTERS: SentryFiltersPersisted = {
  search: '',
  status: 'unresolved',
  level: 'all',
  projects: [],
  environment: null,
  sort: 'date'
};

// ---- Normalisers (defensive parsers) --------------------------------

function normalizeGhFilters(raw: unknown): GithubFilters {
  if (typeof raw !== 'object' || !raw) return { ...DEFAULT_GH_FILTERS };
  const parsed = raw as Record<string, unknown>;
  return {
    mode:
      typeof parsed.mode === 'string'
        ? (parsed.mode as GithubFilters['mode'])
        : 'involving',
    repo: typeof parsed.repo === 'string' ? parsed.repo : null,
    search: typeof parsed.search === 'string' ? parsed.search : '',
    customUser: typeof parsed.customUser === 'string' ? parsed.customUser : ''
  };
}

function normalizeJiraFilters(raw: unknown): JiraFilters {
  if (typeof raw !== 'object' || !raw) return { ...DEFAULT_JIRA_FILTERS };
  const parsed = raw as Record<string, unknown>;
  const sprintIdsRaw = parsed.sprintIds;
  let sprintIds: SprintScope[] = [];
  if (Array.isArray(sprintIdsRaw)) {
    sprintIds = sprintIdsRaw.filter(
      (v): v is SprintScope => typeof v === 'number' || v === 'backlog'
    );
  } else if (typeof parsed.sprintId === 'number') {
    sprintIds = [parsed.sprintId];
  } else if (parsed.sprintId === 'backlog') {
    sprintIds = ['backlog'];
  }
  const statusName =
    typeof parsed.statusName === 'string' && parsed.statusName.trim()
      ? parsed.statusName
      : null;
  const boardIdsRaw = parsed.boardIds;
  let boardIds: number[] = [];
  if (Array.isArray(boardIdsRaw)) {
    boardIds = boardIdsRaw.filter((n): n is number => typeof n === 'number');
  } else if (typeof parsed.boardId === 'number') {
    boardIds = [parsed.boardId];
  }
  const uiRoleRaw = parsed.uiRoleFilter;
  const uiStatusRaw = parsed.uiStatusFilter;
  return {
    projectKey: typeof parsed.projectKey === 'string' ? parsed.projectKey : null,
    boardIds,
    sprintIds,
    statusName,
    search: typeof parsed.search === 'string' ? parsed.search : '',
    uiQuery: typeof parsed.uiQuery === 'string' ? parsed.uiQuery : '',
    // Legacy `'mine'` value (from before the Mine pill was removed) drops
    // to `null` — the global assignee picker replaces it.
    uiRoleFilter: uiRoleRaw === 'reporter' ? 'reporter' : null,
    uiStatusFilter:
      uiStatusRaw === 'open' || uiStatusRaw === 'inprogress' || uiStatusRaw === 'done'
        ? uiStatusRaw
        : null,
    uiProjectFilter: typeof parsed.uiProjectFilter === 'string' ? parsed.uiProjectFilter : null
  };
}

function normalizeSentryFilters(raw: unknown): SentryFiltersPersisted {
  if (typeof raw !== 'object' || !raw) return { ...DEFAULT_SENTRY_FILTERS };
  const parsed = raw as Partial<SentryFiltersPersisted>;
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
}

// ---- localStorage I/O -----------------------------------------------

function readGhFiltersByInstance(): Record<string, GithubFilters> {
  try {
    const raw = localStorage.getItem(GH_COL_FILTERS_KEY);
    if (!raw) return {};
    const parsed = JSON.parse(raw);
    if (typeof parsed !== 'object' || !parsed) return {};
    const out: Record<string, GithubFilters> = {};
    for (const [id, f] of Object.entries(parsed)) {
      out[id] = normalizeGhFilters(f);
    }
    return out;
  } catch {
    return {};
  }
}

function readJiraFiltersByInstance(): Record<string, JiraFilters> {
  try {
    const raw = localStorage.getItem(JIRA_COL_FILTERS_KEY);
    if (!raw) return {};
    const parsed = JSON.parse(raw);
    if (typeof parsed !== 'object' || !parsed) return {};
    const out: Record<string, JiraFilters> = {};
    for (const [id, f] of Object.entries(parsed)) {
      out[id] = normalizeJiraFilters(f);
    }
    return out;
  } catch {
    return {};
  }
}

function readSentryFiltersByInstance(): Record<string, SentryFiltersPersisted> {
  try {
    const raw = localStorage.getItem(SENTRY_COL_FILTERS_KEY);
    if (!raw) return {};
    const parsed = JSON.parse(raw);
    if (typeof parsed !== 'object' || !parsed) return {};
    const out: Record<string, SentryFiltersPersisted> = {};
    for (const [id, f] of Object.entries(parsed)) {
      out[id] = normalizeSentryFilters(f);
    }
    return out;
  } catch {
    return {};
  }
}

export function persistGhFilters() {
  try {
    localStorage.setItem(
      GH_COL_FILTERS_KEY,
      JSON.stringify(inboxState.githubFiltersByInstance)
    );
  } catch {/* ignore */}
}

export function persistJiraFilters() {
  try {
    localStorage.setItem(
      JIRA_COL_FILTERS_KEY,
      JSON.stringify(inboxState.jiraFiltersByInstance)
    );
  } catch {/* ignore */}
}

export function persistSentryFilters() {
  try {
    localStorage.setItem(
      SENTRY_COL_FILTERS_KEY,
      JSON.stringify(inboxState.sentryFiltersByInstance)
    );
  } catch {/* ignore */}
}

/* Tabs persist under their own keys so the column / tab stories stay
   independent across reloads. The body migrates the same way as the
   shared key — `readJiraFilters` already handles legacy field shapes. */
function readJiraTabFilters(): JiraFilters {
  try {
    const raw = localStorage.getItem(JIRA_TAB_FILTERS_KEY);
    if (!raw) return { ...DEFAULT_JIRA_FILTERS };
    const parsed = JSON.parse(raw);
    const sprintIdsRaw = parsed.sprintIds;
    let sprintIds: SprintScope[] = [];
    if (Array.isArray(sprintIdsRaw)) {
      sprintIds = sprintIdsRaw.filter(
        (v): v is SprintScope => typeof v === 'number' || v === 'backlog'
      );
    }
    const boardIdsRaw = parsed.boardIds;
    let boardIds: number[] = [];
    if (Array.isArray(boardIdsRaw)) {
      boardIds = boardIdsRaw.filter((n): n is number => typeof n === 'number');
    }
    return {
      projectKey: typeof parsed.projectKey === 'string' ? parsed.projectKey : null,
      boardIds,
      sprintIds,
      statusName:
        typeof parsed.statusName === 'string' && parsed.statusName.trim()
          ? parsed.statusName
          : null,
      search: typeof parsed.search === 'string' ? parsed.search : '',
      uiQuery: '',
      uiRoleFilter: null,
      uiStatusFilter: null,
      uiProjectFilter: null
    };
  } catch {
    return { ...DEFAULT_JIRA_FILTERS };
  }
}

export function persistJiraTabFilters() {
  try {
    localStorage.setItem(
      JIRA_TAB_FILTERS_KEY,
      JSON.stringify(inboxState.jiraTabFilters)
    );
  } catch {/* ignore */}
}

function readSentryTabFilters(): SentryFiltersPersisted {
  try {
    const raw = localStorage.getItem(SENTRY_TAB_FILTERS_KEY);
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

export function persistSentryTabFilters() {
  try {
    const payload: SentryFiltersPersisted = {
      search: inboxState.sentryTabSearch,
      status: inboxState.sentryTabStatus,
      level: inboxState.sentryTabLevel,
      projects: inboxState.sentryTabProjects,
      environment: inboxState.sentryTabEnvironment,
      sort: inboxState.sentryTabSort
    };
    localStorage.setItem(SENTRY_TAB_FILTERS_KEY, JSON.stringify(payload));
  } catch {/* ignore */}
}

// ---- Reactive store -------------------------------------------------

export const inboxState = $state<{
  // ---- GitHub inbox — per-column-instance ----
  // Each GitHub app keeps its own filter / item / loading / error slot.
  // Persisted as one Record under
  // `woom:github-col-filters-by-instance:v1`.
  itemsByInstance: Record<string, InboxItem[]>;
  loadingByInstance: Record<string, boolean>;
  errorByInstance: Record<string, string | null>;
  githubFiltersByInstance: Record<string, GithubFilters>;
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

  // ---- Jira inbox — per-column-instance ----
  // Two Jira app instances on the same solo (or across solos)
  // each get their own filter / item / loading / error slot, keyed by
  // PanelInstance.id, so changing a board on column A doesn't reload
  // column B. Filters persist as one Record under
  // `woom:jira-col-filters-by-instance:v1`. When an instance is
  // removed (closed / moved), `cleanupInstanceState` (registered with
  // `registerInstanceRemovedHook`) drops its slots so abandoned ids
  // don't pile up in localStorage.
  jiraItemsByInstance: Record<string, JiraItem[]>;
  jiraItemsLoadingByInstance: Record<string, boolean>;
  jiraItemsErrorByInstance: Record<string, string | null>;
  jiraFiltersByInstance: Record<string, JiraFilters>;

  // Selected assignee to filter by. `null` + `jiraAssigneeAny=false` ⇒
  // authenticated account ("Me"). When `jiraAssigneeAny=true`, no assignee
  // constraint at all — show tickets for everyone in the workspace/scope.
  // Stays global — applies to every Jira column equally and is a "who am I
  // looking at" knob, not a per-board view filter.
  jiraAssignee: JiraUserSummary | null;
  jiraAssigneeAny: boolean;
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

  // Jira detail focus — independent from `focusItem` because the Jira
  // column uses its own key-based pane (JiraDetailPane fetches by
  // issue key, not by InboxItem shape).
  jiraFocusKey: string | null;

  // ---- Sentry inbox — per-column-instance ----
  // Same per-instance shape as Jira: each Sentry app keeps its own
  // filter and item slots so two Sentry columns on the same solo can
  // browse different projects / levels / environments simultaneously.
  sentryItemsByInstance: Record<string, SentryIssue[]>;
  sentryItemsLoadingByInstance: Record<string, boolean>;
  sentryItemsErrorByInstance: Record<string, string | null>;
  sentryFiltersByInstance: Record<string, SentryFiltersPersisted>;
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

  // ---- JiraTab (Jira tab) — independent slice ----
  // Mirrors the column-side jira* shape but lives behind the Tasks
  // top-level view so changing a project / board / status in the tab
  // doesn't snap the column out from under the user. Each slice
  // persists & refreshes separately. Dropdown option caches
  // (`jiraProjectOptions`, `jiraBoardOptions`, etc.) ARE shared since
  // they're per-account static data, not user-picked filter state.
  jiraTabFilters: JiraFilters;
  jiraTabItems: JiraItem[];
  jiraTabItemsLoading: boolean;
  jiraTabItemsError: string | null;

  // ---- SentryTab (Sentry tab) — independent slice ----
  sentryTabSearch: string;
  sentryTabStatus: 'unresolved' | 'resolved' | 'ignored' | 'all';
  sentryTabLevel: 'all' | 'fatal' | 'error' | 'warning' | 'info' | 'debug';
  sentryTabProjects: string[];
  sentryTabEnvironment: string | null;
  sentryTabSort: 'date' | 'new' | 'priority' | 'freq' | 'user';
  sentryTabItems: SentryIssue[];
  sentryTabItemsLoading: boolean;
  sentryTabItemsError: string | null;

  // ---- App-navigation channel (driven by `mcp__app__*` tools) ----
  // GithubTab watches `pendingRepoNav` and, when set, opens the
  // requested repo on the requested section, then nulls it back out. We
  // can't call into the view component directly because state is owned
  // there; this reactive channel lets the agent-driven navigation
  // tools land cleanly without a circular dep. Section is a hint —
  // GithubTab validates against its own RepoSection union.
  // Optional `path` only matters for `section === 'code'`: when set the
  // tab drills into the file viewer at that repo-relative path. Empty
  // string / `null` = open at the section root.
  pendingRepoNav: {
    owner: string;
    repo: string;
    section: string;
    path?: string | null;
  } | null;
}>({
  // GitHub per-instance — hydrated from localStorage at boot
  itemsByInstance: {},
  loadingByInstance: {},
  errorByInstance: {},
  githubFiltersByInstance: readGhFiltersByInstance(),
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
  // Jira per-instance
  jiraItemsByInstance: {},
  jiraItemsLoadingByInstance: {},
  jiraItemsErrorByInstance: {},
  jiraFiltersByInstance: readJiraFiltersByInstance(),
  jiraAssignee: null,
  jiraAssigneeAny: false,
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
  // Sentry per-instance
  sentryItemsByInstance: {},
  sentryItemsLoadingByInstance: {},
  sentryItemsErrorByInstance: {},
  sentryFiltersByInstance: readSentryFiltersByInstance(),
  sentryProjectOptions: [],
  sentryProjectOptionsLoading: false,
  sentryEnvironmentOptions: [],
  sentryEnvironmentOptionsLoading: false,
  sentryFocusId: null,
  sentryFocusEventId: null,
  // Jira tab filter slice (top-level Jira view)
  jiraTabFilters: readJiraTabFilters(),
  jiraTabItems: [],
  jiraTabItemsLoading: false,
  jiraTabItemsError: null,
  // Sentry tab filter slice (top-level Sentry view)
  ...(() => {
    const f = readSentryTabFilters();
    return {
      sentryTabSearch: f.search,
      sentryTabStatus: f.status,
      sentryTabLevel: f.level,
      sentryTabProjects: f.projects,
      sentryTabEnvironment: f.environment,
      sentryTabSort: f.sort
    };
  })(),
  sentryTabItems: [],
  sentryTabItemsLoading: false,
  sentryTabItemsError: null,
  pendingRepoNav: null
});

// ---- Cleanup on column close ----------------------------------------

/* Drop a closed column's state slots so abandoned instance ids don't
   pile up across reloads. layoutState fires this hook before it removes
   the instance from the solo, so we still have the id we need. */
registerInstanceRemovedHook((id) => {
  delete inboxState.itemsByInstance[id];
  delete inboxState.loadingByInstance[id];
  delete inboxState.errorByInstance[id];
  delete inboxState.githubFiltersByInstance[id];
  delete inboxState.jiraItemsByInstance[id];
  delete inboxState.jiraItemsLoadingByInstance[id];
  delete inboxState.jiraItemsErrorByInstance[id];
  delete inboxState.jiraFiltersByInstance[id];
  delete inboxState.sentryItemsByInstance[id];
  delete inboxState.sentryItemsLoadingByInstance[id];
  delete inboxState.sentryItemsErrorByInstance[id];
  delete inboxState.sentryFiltersByInstance[id];
  persistGhFilters();
  persistJiraFilters();
  persistSentryFilters();
});
