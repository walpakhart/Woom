/* Sentry-side inbox actions: per-column refresh, detail-pane focus,
 * project / environment option caches, the dedicated SentryTab
 * mirror filters. Reads & writes `inboxState` from
 * `inbox-shared.svelte.ts`. */

import { invoke } from '@tauri-apps/api/core';
import type { SentryEnvironment, SentryIssue, SentryProject } from '$lib/data';
import { APP_INSTANCE_IDS } from '$lib/state/layout.svelte';
import {
  DEFAULT_SENTRY_FILTERS,
  inboxState,
  persistSentryFilters,
  persistSentryTabFilters,
  type SentryFiltersPersisted
} from './inbox-shared.svelte';

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

/** Field-level diff helper for the in-memory UI filters
 *  (uiQuery / uiLevelFilter / uiStatusFilter / uiProjectFilter).
 *  Same shape as `persistGithubUiFilters` / `persistJiraUiFilters` —
 *  called from a $effect in SentryList. No server refresh — these
 *  fields filter already-loaded items, not the Sentry API query. */
export function persistSentryUiFilters(
  instanceId: string,
  patch: {
    uiQuery: string;
    uiLevelFilter: 'fatal' | 'error' | 'warning' | 'info' | null;
    uiStatusFilter: 'unresolved' | 'resolved' | 'ignored' | null;
    uiProjectFilter: string | null;
  }
) {
  const current = sentryFiltersFor(instanceId);
  let changed = false;
  const next: SentryFiltersPersisted = { ...current };
  if (current.uiQuery !== patch.uiQuery) { next.uiQuery = patch.uiQuery; changed = true; }
  if (current.uiLevelFilter !== patch.uiLevelFilter) { next.uiLevelFilter = patch.uiLevelFilter; changed = true; }
  if (current.uiStatusFilter !== patch.uiStatusFilter) { next.uiStatusFilter = patch.uiStatusFilter; changed = true; }
  if (current.uiProjectFilter !== patch.uiProjectFilter) { next.uiProjectFilter = patch.uiProjectFilter; changed = true; }
  if (changed) {
    inboxState.sentryFiltersByInstance[instanceId] = next;
    persistSentryFilters();
  }
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
  const ids = [APP_INSTANCE_IDS.sentry];
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

// ---- Detail-pane focus -----------------------------------------------

/** Open the Sentry detail pane on `id`, optionally on a specific event.
 *  Always sets both `sentryFocusId` and `sentryFocusEventId` atomically
 *  so a stale event id from a previous open doesn't leak into the new
 *  one (which would otherwise 404 from `sentry_get_event_detail`). */
export function openSentryFocus(id: string | null, eventId: string | null = null) {
  inboxState.sentryFocusEventId = eventId;
  inboxState.sentryFocusId = id;
}

// ---- Disconnect reset ------------------------------------------------

export function resetSentryInbox() {
  inboxState.sentryItemsByInstance = {};
  inboxState.sentryItemsLoadingByInstance = {};
  inboxState.sentryItemsErrorByInstance = {};
  inboxState.sentryFocusId = null;
}
