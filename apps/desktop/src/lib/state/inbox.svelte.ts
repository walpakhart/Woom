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
import { listInstancesOfKind, registerInstanceRemovedHook } from '$lib/state/layout.svelte';

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
}

/* Per-column-instance filter persistence — one key per source, payload
   is `Record<instanceId, FilterShape>`. Each column owns one entry,
   `cleanupInstanceState` drops the entry when the instance is closed. */
const GH_COL_FILTERS_KEY = 'forgehold:github-col-filters-by-instance:v1';
const JIRA_COL_FILTERS_KEY = 'forgehold:jira-col-filters-by-instance:v1';
const SENTRY_COL_FILTERS_KEY = 'forgehold:sentry-col-filters-by-instance:v1';
/* Tabs (JiraTab / SentryTab) keep their own filter slice so changing
   a board / project / status in the dedicated tab doesn't yank the
   workbench column out from under the user (and vice-versa). Each tab
   persists separately so a reload restores both states independently. */
const JIRA_TAB_FILTERS_KEY = 'forgehold:jira-tab-filters:v1';
const SENTRY_TAB_FILTERS_KEY = 'forgehold:sentry-tab-filters:v1';

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

const DEFAULT_GH_FILTERS: GithubFilters = {
  mode: 'involving',
  repo: null,
  search: '',
  customUser: ''
};

const DEFAULT_JIRA_FILTERS: JiraFilters = {
  projectKey: null,
  boardIds: [],
  sprintIds: [],
  statusName: null,
  search: ''
};

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
  return {
    projectKey: typeof parsed.projectKey === 'string' ? parsed.projectKey : null,
    boardIds,
    sprintIds,
    statusName,
    search: typeof parsed.search === 'string' ? parsed.search : ''
  };
}

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

function persistGhFilters() {
  try {
    localStorage.setItem(
      GH_COL_FILTERS_KEY,
      JSON.stringify(inboxState.githubFiltersByInstance)
    );
  } catch {/* ignore */}
}

function persistJiraFilters() {
  try {
    localStorage.setItem(
      JIRA_COL_FILTERS_KEY,
      JSON.stringify(inboxState.jiraFiltersByInstance)
    );
  } catch {/* ignore */}
}

function persistSentryFilters() {
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
      search: typeof parsed.search === 'string' ? parsed.search : ''
    };
  } catch {
    return { ...DEFAULT_JIRA_FILTERS };
  }
}

function persistJiraTabFilters() {
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

function persistSentryTabFilters() {
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

export const inboxState = $state<{
  // ---- GitHub inbox — per-column-instance ----
  // Each GithubColumn keeps its own filter / item / loading / error slot.
  // Persisted as one Record under
  // `forgehold:github-col-filters-by-instance:v1`.
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
  // Two JiraColumn instances on the same workbench (or across workbenches)
  // each get their own filter / item / loading / error slot, keyed by
  // PanelInstance.id, so changing a board on column A doesn't reload
  // column B. Filters persist as one Record under
  // `forgehold:jira-col-filters-by-instance:v1`. When an instance is
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

  // Jira detail slide-over — independent from `focusItem` because the Jira
  // column uses its own key-based slide-over (JiraDetailPane fetches by
  // issue key, not by InboxItem shape).
  jiraFocusKey: string | null;

  // ---- Sentry inbox — per-column-instance ----
  // Same per-instance shape as Jira: each SentryColumn keeps its own
  // filter and item slots so two Sentry columns on the same workbench can
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

let userPickerDebounce: ReturnType<typeof setTimeout> | null = null;
/* Filter debounces are now per-instance (see githubFilterDebounces /
   jiraFilterDebounces / sentryFilterDebounces Maps below) so column A
   typing in its search box doesn't cancel column B's pending refresh. */

/* Drop a closed column's state slots so abandoned instance ids don't
   pile up across reloads. layoutState fires this hook before it removes
   the instance from the workbench, so we still have the id we need. */
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

/** Refresh every GitHub column on every workbench. Used by page-level
 *  handlers (bootstrap on connect, after a PR is created, etc.) that
 *  don't have a specific instanceId in scope. */
export async function refreshAllInboxes(opts: { silent?: boolean } = {}) {
  const ids = listInstancesOfKind('github').map((i) => i.id);
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
 *  expected to toggle `view = 'workbench'` themselves if they need to. */
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
  await refreshAllInboxes({ silent: true });
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
  // in that case too — see JiraColumn.svelte). Multi-sprint within
  // one board fans out: numeric ids → `sprint IN (...)`, plus an
  // OR for backlog if it's in the mix.
  if (filters.boardIds.length <= 1 && filters.sprintIds.length > 0) {
    const numeric = filters.sprintIds.filter((s): s is number => typeof s === 'number');
    const includeBacklog = filters.sprintIds.includes('backlog');
    const subParts: string[] = [];
    if (numeric.length === 1) {
      subParts.push(`sprint = ${numeric[0]}`);
    } else if (numeric.length > 1) {
      subParts.push(`sprint IN (${numeric.join(', ')})`);
    }
    if (includeBacklog) subParts.push('sprint is EMPTY');
    if (subParts.length === 1) {
      parts.push(subParts[0]);
    } else if (subParts.length > 1) {
      parts.push(`(${subParts.join(' OR ')})`);
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

/* Per-instance Jira accessors. */
export function jiraFiltersFor(instanceId: string): JiraFilters {
  return inboxState.jiraFiltersByInstance[instanceId] ?? DEFAULT_JIRA_FILTERS;
}
export function jiraItemsFor(instanceId: string): JiraItem[] {
  return inboxState.jiraItemsByInstance[instanceId] ?? [];
}
export function jiraItemsLoadingFor(instanceId: string): boolean {
  return inboxState.jiraItemsLoadingByInstance[instanceId] ?? false;
}
export function jiraItemsErrorFor(instanceId: string): string | null {
  return inboxState.jiraItemsErrorByInstance[instanceId] ?? null;
}

export async function refreshJiraInbox(
  instanceId: string,
  { silent = false }: { silent?: boolean } = {}
) {
  if (!silent) inboxState.jiraItemsLoadingByInstance[instanceId] = true;
  inboxState.jiraItemsErrorByInstance[instanceId] = null;
  try {
    const f = jiraFiltersFor(instanceId);
    const usingDefault =
      !f.projectKey &&
      f.boardIds.length === 0 &&
      f.sprintIds.length === 0 &&
      f.statusName == null &&
      !f.search.trim() &&
      !inboxState.jiraAssigneeAny;
    if (usingDefault) {
      inboxState.jiraItemsByInstance[instanceId] = await invoke<JiraItem[]>(
        'jira_list_inbox_for',
        { assigneeAccountId: inboxState.jiraAssignee?.account_id ?? null }
      );
    } else {
      const jql = buildJiraJql(f, inboxState.jiraAssignee, inboxState.jiraAssigneeAny);
      inboxState.jiraItemsByInstance[instanceId] = await invoke<JiraItem[]>(
        'jira_search',
        { jql }
      );
    }
  } catch (e) {
    inboxState.jiraItemsErrorByInstance[instanceId] =
      typeof e === 'string' ? e : String(e);
  } finally {
    inboxState.jiraItemsLoadingByInstance[instanceId] = false;
  }
}

export async function refreshAllJiraInboxes(opts: { silent?: boolean } = {}) {
  const ids = listInstancesOfKind('jira').map((i) => i.id);
  await Promise.all(ids.map((id) => refreshJiraInbox(id, opts)));
}

const jiraFilterDebounces: Map<string, ReturnType<typeof setTimeout>> = new Map();

/** Patch one column's Jira filter state, persist all instances, and
 *  re-run that column's search (debounced 300 ms). */
export function updateJiraFilters(instanceId: string, patch: Partial<JiraFilters>) {
  inboxState.jiraFiltersByInstance[instanceId] = {
    ...jiraFiltersFor(instanceId),
    ...patch
  };
  persistJiraFilters();
  const t = jiraFilterDebounces.get(instanceId);
  if (t) clearTimeout(t);
  jiraFilterDebounces.set(
    instanceId,
    setTimeout(() => void refreshJiraInbox(instanceId, { silent: true }), 300)
  );
}

let jiraTabFilterDebounce: ReturnType<typeof setTimeout> | null = null;

/* JiraTab mirror of refreshJiraInbox — same JQL builder, same backend
   call, but reads `jiraTabFilters` and writes to `jiraTabItems` so
   the Jira tab and the Jira column don't trample each other's lists. */
export async function refreshJiraTabInbox(
  { silent = false }: { silent?: boolean } = {}
) {
  if (!silent) inboxState.jiraTabItemsLoading = true;
  inboxState.jiraTabItemsError = null;
  try {
    const f = inboxState.jiraTabFilters;
    const usingDefault =
      !f.projectKey &&
      f.boardIds.length === 0 &&
      f.sprintIds.length === 0 &&
      f.statusName == null &&
      !f.search.trim() &&
      !inboxState.jiraAssigneeAny;
    if (usingDefault) {
      inboxState.jiraTabItems = await invoke<JiraItem[]>('jira_list_inbox_for', {
        assigneeAccountId: inboxState.jiraAssignee?.account_id ?? null
      });
    } else {
      const jql = buildJiraJql(f, inboxState.jiraAssignee, inboxState.jiraAssigneeAny);
      inboxState.jiraTabItems = await invoke<JiraItem[]>('jira_search', { jql });
    }
  } catch (e) {
    inboxState.jiraTabItemsError = typeof e === 'string' ? e : String(e);
  } finally {
    inboxState.jiraTabItemsLoading = false;
  }
}

export function updateJiraTabFilters(patch: Partial<JiraFilters>) {
  inboxState.jiraTabFilters = { ...inboxState.jiraTabFilters, ...patch };
  persistJiraTabFilters();
  if (jiraTabFilterDebounce) clearTimeout(jiraTabFilterDebounce);
  jiraTabFilterDebounce = setTimeout(
    () => void refreshJiraTabInbox({ silent: true }),
    300
  );
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
  await refreshAllJiraInboxes();
}

/** Drop the assignee constraint entirely — show tickets for everyone in the
 *  active project/board/sprint scope. */
export async function selectAnyAssignee() {
  inboxState.jiraAssignee = null;
  inboxState.jiraAssigneeAny = true;
  closeModal('userPicker');
  await refreshAllJiraInboxes();
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
  inboxState.itemsByInstance = {};
  inboxState.loadingByInstance = {};
  inboxState.errorByInstance = {};
  inboxState.focusItem = null;
}

export function resetJiraInbox() {
  inboxState.jiraItemsByInstance = {};
  inboxState.jiraItemsLoadingByInstance = {};
  inboxState.jiraItemsErrorByInstance = {};
  inboxState.jiraAssignee = null;
  inboxState.jiraAssigneeAny = false;
  inboxState.jiraStatusOptions = [];
  inboxState.jiraStatusOptionsProjectKey = undefined;
}

// ---- Sentry inbox ----

/* Per-instance Sentry accessors. Filter state lives as a single
   SentryFiltersPersisted blob per instance (search/status/level/
   projects/environment/sort all together) since they're refreshed in
   one network call. */
export function sentryFiltersFor(instanceId: string): SentryFiltersPersisted {
  return inboxState.sentryFiltersByInstance[instanceId] ?? DEFAULT_SENTRY_FILTERS;
}
export function sentryItemsFor(instanceId: string): SentryIssue[] {
  return inboxState.sentryItemsByInstance[instanceId] ?? [];
}
export function sentryItemsLoadingFor(instanceId: string): boolean {
  return inboxState.sentryItemsLoadingByInstance[instanceId] ?? false;
}
export function sentryItemsErrorFor(instanceId: string): string | null {
  return inboxState.sentryItemsErrorByInstance[instanceId] ?? null;
}

export function setSentryFilters(
  instanceId: string,
  patch: Partial<SentryFiltersPersisted>
) {
  inboxState.sentryFiltersByInstance[instanceId] = {
    ...sentryFiltersFor(instanceId),
    ...patch
  };
}

/** Compose the `query=` string from this column's structured filter
 *  state. Empty status/level slots translate to "no qualifier" (Sentry
 *  default matches everything when no `is:` is present). */
export function buildSentryQuery(instanceId: string): string {
  const parts: string[] = [];
  const f = sentryFiltersFor(instanceId);
  if (f.status !== 'all') parts.push(`is:${f.status}`);
  if (f.level !== 'all') parts.push(`level:${f.level}`);
  const search = f.search.trim();
  if (search) parts.push(search);
  return parts.join(' ');
}

const sentryFilterDebounces: Map<string, ReturnType<typeof setTimeout>> = new Map();

export async function refreshSentryInbox(
  instanceId: string,
  { silent = false }: { silent?: boolean } = {}
) {
  if (!silent) inboxState.sentryItemsLoadingByInstance[instanceId] = true;
  inboxState.sentryItemsErrorByInstance[instanceId] = null;
  try {
    const f = sentryFiltersFor(instanceId);
    const items = await invoke<SentryIssue[]>('sentry_list_issues', {
      query: buildSentryQuery(instanceId) || null,
      projectSlugs: f.projects,
      environment: f.environment,
      sort: f.sort,
      limit: 50
    });
    inboxState.sentryItemsByInstance[instanceId] = items;
  } catch (e) {
    inboxState.sentryItemsErrorByInstance[instanceId] =
      typeof e === 'string' ? e : String(e);
  } finally {
    inboxState.sentryItemsLoadingByInstance[instanceId] = false;
  }
}

export async function refreshAllSentryInboxes(opts: { silent?: boolean } = {}) {
  const ids = listInstancesOfKind('sentry').map((i) => i.id);
  await Promise.all(ids.map((id) => refreshSentryInbox(id, opts)));
}

/** Schedule a debounced refresh after a filter change (250ms). Persists
 *  every instance's filters to one localStorage entry. */
export function scheduleSentryFilterRefresh(instanceId: string) {
  persistSentryFilters();
  const t = sentryFilterDebounces.get(instanceId);
  if (t) clearTimeout(t);
  sentryFilterDebounces.set(
    instanceId,
    setTimeout(() => void refreshSentryInbox(instanceId, { silent: true }), 250)
  );
}

/* SentryTab (Sentry tab) mirror — same query builder, separate
   filter state + items list so the tab and column don't share. */
export function buildSentryTabQuery(): string {
  const parts: string[] = [];
  const { sentryTabStatus, sentryTabLevel, sentryTabSearch } = inboxState;
  if (sentryTabStatus !== 'all') parts.push(`is:${sentryTabStatus}`);
  if (sentryTabLevel !== 'all') parts.push(`level:${sentryTabLevel}`);
  const search = sentryTabSearch.trim();
  if (search) parts.push(search);
  return parts.join(' ');
}

let sentryTabFilterDebounce: ReturnType<typeof setTimeout> | null = null;

export async function refreshSentryTabInbox(
  { silent = false }: { silent?: boolean } = {}
) {
  if (!silent) inboxState.sentryTabItemsLoading = true;
  inboxState.sentryTabItemsError = null;
  try {
    const items = await invoke<SentryIssue[]>('sentry_list_issues', {
      query: buildSentryTabQuery() || null,
      projectSlugs: inboxState.sentryTabProjects,
      environment: inboxState.sentryTabEnvironment,
      sort: inboxState.sentryTabSort,
      limit: 50
    });
    inboxState.sentryTabItems = items;
  } catch (e) {
    inboxState.sentryTabItemsError = typeof e === 'string' ? e : String(e);
  } finally {
    inboxState.sentryTabItemsLoading = false;
  }
}

export function scheduleSentryTabFilterRefresh() {
  persistSentryTabFilters();
  if (sentryTabFilterDebounce) clearTimeout(sentryTabFilterDebounce);
  sentryTabFilterDebounce = setTimeout(
    () => void refreshSentryTabInbox({ silent: true }),
    250
  );
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

/** Pull environments for the given project slug. Caller passes the
 *  slug explicitly so each column can load its own picked project's
 *  envs without depending on a single global "selected project". With
 *  no slug provided we fall back to the first member-project so the
 *  dropdown isn't empty.
 *
 *  Note: `sentryEnvironmentOptions` is still a single shared list — two
 *  Sentry columns picking different projects will see whichever project
 *  loaded last. Per-project env caches were too much surface for a
 *  lightly-used dropdown; revisit if it actually bites. */
export async function loadSentryEnvironments(projectSlug?: string) {
  const slug =
    projectSlug ??
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
  inboxState.sentryItemsByInstance = {};
  inboxState.sentryItemsLoadingByInstance = {};
  inboxState.sentryItemsErrorByInstance = {};
  inboxState.sentryFocusId = null;
}
