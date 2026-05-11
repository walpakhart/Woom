/* Back-compat barrel for the inbox state. The real implementation
 * lives in four sibling files, split by source so each one fits in
 * one screen of context:
 *
 *   - inbox-shared.svelte.ts  → types, defaults, persistence,
 *                               `inboxState` reactive store, the
 *                               instance-removed cleanup hook
 *   - inbox-github.ts         → GitHub inbox + detail-shelf actions
 *   - inbox-jira.ts           → Jira inbox + JQL + assignee picker
 *   - inbox-sentry.ts         → Sentry inbox + tab mirror + detail pane
 *
 * Existing call-sites import from `$lib/state/inbox.svelte` — keep
 * that path working by re-exporting the public surface here. New
 * code can import from the per-source modules directly when it
 * only needs one slice. */

export {
  inboxState,
  persistGhFilters,
  persistJiraFilters,
  persistSentryFilters,
  persistJiraTabFilters,
  persistSentryTabFilters,
  DEFAULT_GH_FILTERS,
  DEFAULT_JIRA_FILTERS,
  DEFAULT_SENTRY_FILTERS,
  type GithubFilterMode,
  type GithubFilters,
  type JiraFilters,
  type SentryFiltersPersisted,
  type SprintScope
} from './inbox-shared.svelte';

export {
  buildGithubQuery,
  closeFocusItem,
  githubErrorFor,
  githubFiltersFor,
  githubItemsFor,
  githubLoadingFor,
  loadDetail,
  loadGithubRepoOptions,
  moveSelection,
  openFocusItem,
  refreshAllInboxes,
  refreshInbox,
  reloadDetailAndLists,
  resetGithubInbox,
  selectInboxItem,
  setGithubMeLogin,
  toggleFile,
  updateGithubFilters
} from './inbox-github';

export {
  buildJiraJql,
  invalidateJiraStatuses,
  jiraFiltersFor,
  jiraItemsErrorFor,
  jiraItemsFor,
  jiraItemsLoadingFor,
  loadJiraBoards,
  loadJiraProjects,
  loadJiraSprints,
  loadJiraStatuses,
  onUserPickerInput,
  openUserPicker,
  refreshAllJiraInboxes,
  refreshJiraInbox,
  refreshJiraTabInbox,
  resetJiraInbox,
  selectAnyAssignee,
  selectAssignee,
  persistJiraUiFilters,
  updateJiraFilters,
  updateJiraTabFilters
} from './inbox-jira';

export {
  buildSentryQuery,
  buildSentryTabQuery,
  loadSentryEnvironments,
  loadSentryProjects,
  openSentryFocus,
  refreshAllSentryInboxes,
  refreshSentryInbox,
  refreshSentryTabInbox,
  resetSentryInbox,
  scheduleSentryFilterRefresh,
  scheduleSentryTabFilterRefresh,
  sentryFiltersFor,
  sentryItemsErrorFor,
  sentryItemsFor,
  sentryItemsLoadingFor,
  setSentryFilters
} from './inbox-sentry';
