/* Jira-side inbox actions: per-column refresh, JQL composition,
 * project / board / sprint / status option caches, the assignee
 * picker. Reads & writes `inboxState` from `inbox-shared.svelte.ts`.
 *
 * The user-picker modal is driven through `modalsState` from
 * `modals.svelte.ts`; we only own the search query that populates
 * its results. */

import { invoke } from '@tauri-apps/api/core';
import type {
  JiraBoard,
  JiraItem,
  JiraProject,
  JiraSprint,
  JiraUserSummary,
  JiraWorkflowStatus
} from '$lib/data';
import { closeModal, modalsState, openModal, patchModal } from '$lib/state/modals.svelte';
import { APP_INSTANCE_IDS } from '$lib/state/layout.svelte';
import {
  DEFAULT_JIRA_FILTERS,
  inboxState,
  persistJiraFilters,
  persistJiraTabFilters,
  type JiraFilters
} from './inbox-shared.svelte';

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
  // in that case too — see Jira app.svelte). Multi-sprint within
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
  const ids = [APP_INSTANCE_IDS.jira];
  await Promise.all(ids.map((id) => refreshJiraInbox(id, opts)));
}

const jiraFilterDebounces: Map<string, ReturnType<typeof setTimeout>> = new Map();

/** Lazily ensure a slot exists for `instanceId`, returning the *live*
 *  state object (not a copy). Both filter-mutation paths below need
 *  the live reference so they can patch fields IN PLACE — spreading
 *  into a fresh object would replace the slot reference and force
 *  every $derived consuming `jiraFiltersFor(...)` to re-emit on every
 *  keystroke (we hit that bug in the wild: per-keystroke
 *  `loadJiraBoards` API calls froze the app for hundreds of ms). */
function ensureFiltersSlot(instanceId: string): JiraFilters {
  let slot = inboxState.jiraFiltersByInstance[instanceId];
  if (!slot) {
    /* DEFAULT_JIRA_FILTERS is shared by every fresh instance — we
       MUST clone it so a per-instance mutation doesn't leak globally. */
    slot = { ...DEFAULT_JIRA_FILTERS };
    inboxState.jiraFiltersByInstance[instanceId] = slot;
  }
  return slot;
}

/** Apply a patch to the slot in place, only writing fields whose
 *  value actually changed. Svelte 5's deep proxy emits reactivity at
 *  the field level, so callers that depend on a specific field
 *  (`f.projectKey`) only re-run when THAT field's value differs —
 *  consumers reading other fields stay quiet. Returns true if any
 *  field was actually written, so callers can skip side effects on
 *  no-op patches. */
function applyFiltersPatch(slot: JiraFilters, patch: Partial<JiraFilters>): boolean {
  let changed = false;
  /* Cast through unknown rather than directly to a Record — the
     compiler is right that JiraFilters isn't string-indexable, but
     the keys here are guaranteed to come from `keyof JiraFilters`,
     so the runtime write is sound. The unknown bounce silences
     `Conversion ... may be a mistake` without resorting to `any`. */
  const bag = slot as unknown as Record<string, unknown>;
  for (const k of Object.keys(patch) as (keyof JiraFilters)[]) {
    const next = patch[k];
    if (next === undefined) continue;
    /* Reference-equality is enough for our scalar fields (strings,
       numbers, null). The arrays in JiraFilters (boardIds /
       sprintIds) get reassigned on filter updates rather than
       mutated in place, so a fresh array passed in WILL register as
       changed even if its contents match — that's fine for our
       triggers (refreshJiraInbox respects the debounce anyway). */
    if (bag[k as string] !== next) {
      bag[k as string] = next as unknown;
      changed = true;
    }
  }
  return changed;
}

/** Persist UI-only filter state (query, role, status, project, assignee)
 *  without triggering a server-side refresh — these fields are not in
 *  the JQL and should not cause a round-trip on every keystroke. */
export function persistJiraUiFilters(instanceId: string, patch: Partial<JiraFilters>) {
  const slot = ensureFiltersSlot(instanceId);
  if (applyFiltersPatch(slot, patch)) {
    persistJiraFilters();
  }
}

/** Patch one column's Jira filter state, persist all instances, and
 *  re-run that column's search (debounced 300 ms). */
export function updateJiraFilters(instanceId: string, patch: Partial<JiraFilters>) {
  const slot = ensureFiltersSlot(instanceId);
  if (!applyFiltersPatch(slot, patch)) return;
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

// ---- Option caches (project / board / sprint / status) --------------

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

// ---- Assignee picker -------------------------------------------------

let userPickerDebounce: ReturnType<typeof setTimeout> | null = null;

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

// ---- Disconnect reset ------------------------------------------------

export function resetJiraInbox() {
  inboxState.jiraItemsByInstance = {};
  inboxState.jiraItemsLoadingByInstance = {};
  inboxState.jiraItemsErrorByInstance = {};
  inboxState.jiraAssignee = null;
  inboxState.jiraAssigneeAny = false;
  inboxState.jiraStatusOptions = [];
  inboxState.jiraStatusOptionsProjectKey = undefined;
}
