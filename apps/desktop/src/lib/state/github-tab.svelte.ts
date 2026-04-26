/* GitHub tab (top-level view, formerly RepositoriesView) state.
 *
 * Lifted out of the component because the parent re-mounts views on
 * every `view` switch — without a module-level store, every time the
 * user opens the GitHub tab, all repo / section / branch state
 * resets to the initial empty values. The Jira and Sentry tabs don't
 * have this problem because their state lives in `inboxState`
 * (selectedProject is derived from `jiraTabFilters.projectKey` /
 * `sentryTabProjects[0]`, which survives unmount).
 *
 * Kept as a separate module instead of stuffing into `inboxState`
 * because this is purely UI-route state — none of it is re-used by
 * the workbench column, MCP tools, or other views — and the inbox
 * state is already large.
 */

import type {
  FileBlob,
  InboxItem,
  RepoBranch,
  RepoReadme,
  Release,
  Repository,
  TreeEntry,
  WorkflowRun
} from '$lib/data';

export type RepoSection = 'code' | 'pulls' | 'issues' | 'actions' | 'releases';
export type RepoTab = 'open' | 'closed' | 'all';

export const githubTabState = $state<{
  repos: Repository[];
  reposLoading: boolean;
  reposError: string | null;

  /* Currently-opened repo. `null` = grid view. Survives unmount so
     re-entering the tab restores the same drilldown. */
  selectedRepo: Repository | null;
  repoItems: InboxItem[];
  repoItemsLoading: boolean;
  repoItemsError: string | null;
  repoStateFilter: RepoTab;
  repoSection: RepoSection;

  workflowRuns: WorkflowRun[];
  workflowRunsLoading: boolean;
  workflowRunsError: string | null;

  repoReadme: RepoReadme | null;
  repoReadmeLoading: boolean;

  /* Code tab — file browser state. */
  repoCodeBranches: RepoBranch[];
  repoCodeBranchesLoading: boolean;
  repoCodeBranch: string;
  repoCodeTree: TreeEntry[];
  repoCodeTreeLoading: boolean;
  repoCodeTreeError: string | null;
  repoCodePath: string;
  repoCodeFile: FileBlob | null;
  repoCodeFileLoading: boolean;
  repoCodeFileError: string | null;

  repoReleases: Release[];
  repoReleasesLoading: boolean;
  repoReleasesError: string | null;

  /* In-flight workflow rerun / cancel guards (keyed by run id). */
  runBusy: Set<number>;
}>({
  repos: [],
  reposLoading: false,
  reposError: null,
  selectedRepo: null,
  repoItems: [],
  repoItemsLoading: false,
  repoItemsError: null,
  repoStateFilter: 'open',
  repoSection: 'pulls',
  workflowRuns: [],
  workflowRunsLoading: false,
  workflowRunsError: null,
  repoReadme: null,
  repoReadmeLoading: false,
  repoCodeBranches: [],
  repoCodeBranchesLoading: false,
  repoCodeBranch: '',
  repoCodeTree: [],
  repoCodeTreeLoading: false,
  repoCodeTreeError: null,
  repoCodePath: '',
  repoCodeFile: null,
  repoCodeFileLoading: false,
  repoCodeFileError: null,
  repoReleases: [],
  repoReleasesLoading: false,
  repoReleasesError: null,
  runBusy: new Set()
});
