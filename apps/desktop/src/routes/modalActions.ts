// Modal-action helpers extracted from `+page.svelte` in wave-34.
// Covers the GitHub inline-detail actions (comment / review / merge /
// state flip), connection-modal openers + submits (PAT / Jira /
// Sentry / Claude / Cursor), and the create-new-issue / create-PR
// flows that gate the inbox columns.
//
// Each helper takes a `deps` carrying the route's local setters
// (`setView`, `setActionBusy`, `reloadDetailAndLists`,
// `refreshClaudeModal` / `refreshCursorModal` for the secondary
// statuses, plus accessors for the agent-status derived values
// since these can't be passed by reference from a Svelte 5 `$state`).

import { invoke } from '@tauri-apps/api/core';
import { connectionsState, refreshGithubStatus, refreshJiraStatus, refreshSentryStatus, refreshClaudeStatus } from '$lib/state/connections.svelte';
import {
  inboxState,
  refreshAllInboxes,
  refreshAllJiraInboxes,
  refreshAllSentryInboxes,
  resetGithubInbox,
  resetJiraInbox,
  resetSentryInbox,
  openFocusItem,
} from '$lib/state/inbox.svelte';
import { modalsState, openModal, closeModal, patchModal } from '$lib/state/modals.svelte';
import { markTokenInstalled, clearTokenInstalled } from '$lib/state/tokenAge.svelte';
import { notify, notifyError } from '$lib/state/toaster.svelte';
import { openUrl } from '@tauri-apps/plugin-opener';
import {
  externalId,
  connectionsMeta,
  type ConnectionMeta,
  type CompareResult,
  type GithubUser,
  type InboxItem,
  type JiraItem,
  type JiraIssueType,
  type JiraProject,
  type JiraUser,
  type JiraUserSummary,
  type RepoBranch,
  type Repository,
  type SentryUser,
  type ClaudeStatus,
} from '$lib/data';

type View = string;

export interface ModalActionDeps {
  /** Local `view` setter (Svelte 5 `let` state). */
  setView(v: View): void;
  /** Local `actionBusy` setter — null clears the spinner. */
  setActionBusy(s: 'open' | 'closed' | null): void;
  /** Reload focus + lists after a focus-item mutation. */
  reloadDetailAndLists(): Promise<void> | void;
  /** Current agent-status snapshots — passed via callbacks because
   *  these are `$derived` values that can't be referenced cross-
   *  module by handle. */
  getClaudeStatus(): ClaudeStatus | null;
  getCursorStatus(): ClaudeStatus | null;
}

// ---- GitHub focus-item actions ----

export async function submitComment() {
  const m = modalsState.comment;
  if (!m || !inboxState.focusItem?.repo) return;
  const body = m.body;
  if (!body.trim()) return;
  patchModal('comment', { busy: true, error: null });
  try {
    await invoke('github_add_comment', {
      owner: inboxState.focusItem.repo.owner,
      repo: inboxState.focusItem.repo.name,
      number: inboxState.focusItem.number,
      body,
    });
    closeModal('comment');
  } catch (e) {
    patchModal('comment', { busy: false, error: typeof e === 'string' ? e : String(e) });
  }
}

export async function submitReview() {
  const m = modalsState.review;
  if (!m || !inboxState.focusItem?.repo || !inboxState.focusItem.is_pull_request) return;
  const { event, body } = m;
  patchModal('review', { busy: true, error: null });
  try {
    await invoke('github_submit_review', {
      owner: inboxState.focusItem.repo.owner,
      repo: inboxState.focusItem.repo.name,
      number: inboxState.focusItem.number,
      event,
      body,
    });
    closeModal('review');
  } catch (e) {
    patchModal('review', { busy: false, error: typeof e === 'string' ? e : String(e) });
  }
}

export async function submitMerge() {
  const m = modalsState.merge;
  if (!m || !inboxState.focusItem?.repo || !inboxState.focusItem.is_pull_request) return;
  const method = m.method;
  patchModal('merge', { busy: true, error: null });
  try {
    await invoke('github_merge_pr', {
      owner: inboxState.focusItem.repo.owner,
      repo: inboxState.focusItem.repo.name,
      number: inboxState.focusItem.number,
      method,
    });
    closeModal('merge');
  } catch (e) {
    patchModal('merge', { busy: false, error: typeof e === 'string' ? e : String(e) });
  }
}

export async function setIssueState(state: 'closed' | 'open', deps: ModalActionDeps) {
  if (!inboxState.focusItem?.repo) return;
  deps.setActionBusy(state);
  try {
    await invoke('github_set_state', {
      owner: inboxState.focusItem.repo.owner,
      repo: inboxState.focusItem.repo.name,
      number: inboxState.focusItem.number,
      state,
    });
    // Optimistically update focusItem so the UI flips Close→Reopen right away,
    // even though the inbox (filtered is:open) may drop the item on refresh.
    if (inboxState.focusItem) {
      inboxState.focusItem = { ...inboxState.focusItem, state };
    }
    await deps.reloadDetailAndLists();
  } catch (e) {
    inboxState.detailError = typeof e === 'string' ? e : String(e);
  } finally {
    deps.setActionBusy(null);
  }
}

export function askClose(deps: ModalActionDeps) {
  if (!inboxState.focusItem) return;
  const kind = inboxState.focusItem.is_pull_request ? 'pull request' : 'issue';
  openModal('confirm', {
    title: `Close this ${kind}?`,
    body: `${externalId(inboxState.focusItem)} — ${inboxState.focusItem.title}`,
    confirmText: 'Close',
    danger: true,
    busy: false,
    onConfirm: async () => {
      await setIssueState('closed', deps);
    },
  });
}

// ---- Connect modal openers ----

export function openConnectModal(conn: ConnectionMeta, deps: ModalActionDeps) {
  if (!conn.implemented) return;
  if (conn.id === 'github') {
    openModal('pat', { conn, token: '', error: null, busy: false });
  } else if (conn.id === 'jira') {
    openModal('jiraConnect', { workspace: '', email: '', token: '', error: null, busy: false });
  } else if (conn.id === 'sentry') {
    openModal('sentryConnect', {
      host: 'https://sentry.io',
      organization_slug: '',
      token: '',
      error: null,
      busy: false,
    });
  } else if (conn.id === 'claude') {
    openModal('claudeStatus', { status: deps.getClaudeStatus(), loading: false });
    void refreshClaudeModal(deps);
  } else if (conn.id === 'cursor') {
    openModal('cursorStatus', { status: deps.getCursorStatus(), loading: false });
    void refreshCursorModal(deps);
  }
}

export async function refreshClaudeModal(deps: ModalActionDeps) {
  if (!modalsState.claudeStatus) return;
  patchModal('claudeStatus', { loading: true });
  await refreshClaudeStatus();
  if (modalsState.claudeStatus) {
    patchModal('claudeStatus', { status: deps.getClaudeStatus(), loading: false });
  }
}

export async function refreshCursorModal(deps: ModalActionDeps) {
  if (!modalsState.cursorStatus) return;
  patchModal('cursorStatus', { loading: true });
  // refreshClaudeStatus() actually refreshes BOTH agents (cursor + claude) —
  // see `agent_status` Tauri command. Reuse so we don't double-poll.
  await refreshClaudeStatus();
  if (modalsState.cursorStatus) {
    patchModal('cursorStatus', { status: deps.getCursorStatus(), loading: false });
  }
}

export const cursorInstallUrl = () => 'https://cursor.com/docs/cli/installation';
export const claudeInstallUrl = () => 'https://docs.claude.com/en/docs/claude-code/overview';
export const jiraTokenUrl = () => 'https://id.atlassian.com/manage-profile/security/api-tokens';
export function githubTokenUrl(): string {
  const scopes = ['repo', 'read:user', 'read:org'].join(',');
  return `https://github.com/settings/tokens/new?scopes=${scopes}&description=Woom%20Desktop`;
}

export function sentryTokenUrl(): string {
  const host = modalsState.sentryConnect?.host?.trim() || 'https://sentry.io';
  return `${host.replace(/\/+$/, '')}/settings/account/api/auth-tokens/`;
}

// ---- Connect submits / disconnects ----

export async function submitJira() {
  const m = modalsState.jiraConnect;
  if (!m) return;
  const { workspace, email, token } = m;
  patchModal('jiraConnect', { busy: true, error: null });
  try {
    await invoke<JiraUser>('jira_connect', { workspace, email, token });
    markTokenInstalled('jira');
    closeModal('jiraConnect');
    await refreshJiraStatus();
  } catch (e) {
    patchModal('jiraConnect', { busy: false, error: typeof e === 'string' ? e : String(e) });
  }
}

export async function disconnectJira() {
  await invoke('jira_disconnect');
  clearTokenInstalled('jira');
  await refreshJiraStatus();
}

export async function submitSentry() {
  if (!modalsState.sentryConnect) return;
  const { host, organization_slug, token } = modalsState.sentryConnect;
  patchModal('sentryConnect', { busy: true, error: null });
  try {
    await invoke<SentryUser>('sentry_connect', {
      host,
      organizationSlug: organization_slug,
      token,
    });
    markTokenInstalled('sentry');
    closeModal('sentryConnect');
    await refreshSentryStatus();
    void refreshAllSentryInboxes();
  } catch (e) {
    patchModal('sentryConnect', { busy: false, error: typeof e === 'string' ? e : String(e) });
  }
}

export async function disconnectSentryAll() {
  try {
    await invoke('sentry_disconnect');
    clearTokenInstalled('sentry');
    await refreshSentryStatus();
    resetSentryInbox();
    notify({ kind: 'success', title: 'Disconnected from Sentry' });
  } catch (e) {
    notifyError(e, { title: 'Sentry disconnect failed' });
  }
}

export async function submitPat(deps: ModalActionDeps) {
  const m = modalsState.pat;
  if (!m) return;
  const token = m.token;
  patchModal('pat', { busy: true, error: null });
  try {
    const user = await invoke<GithubUser>('github_connect_pat', { token });
    connectionsState.github = { kind: 'connected', user };
    markTokenInstalled('github');
    closeModal('pat');
    await refreshAllInboxes();
    deps.setView('githubApp');
  } catch (e) {
    patchModal('pat', { busy: false, error: typeof e === 'string' ? e : String(e) });
  }
}

export async function disconnectGithub() {
  try {
    await invoke('github_disconnect');
    clearTokenInstalled('github');
    await refreshGithubStatus();
    resetGithubInbox();
    notify({ kind: 'success', title: 'Disconnected from GitHub' });
  } catch (e) {
    notifyError(e, { title: 'GitHub disconnect failed' });
  }
  // Repo state is owned by GithubTab — it wipes itself via its
  // `$effect` on `connectedGithub` becoming false.
}

export async function disconnectJiraAll() {
  try {
    await invoke('jira_disconnect');
    clearTokenInstalled('jira');
    await refreshJiraStatus();
    resetJiraInbox();
    notify({ kind: 'success', title: 'Disconnected from Jira' });
  } catch (e) {
    notifyError(e, { title: 'Jira disconnect failed' });
  }
}

export async function openBrowser(url: string) {
  try { await openUrl(url); } catch (e) { notifyError(e, { title: 'Could not open browser' }); }
}

// ---- Jira Create Issue flow ----

export async function openJiraCreateIssue() {
  /* Pull defaults from the FIRST jira column's filter — no perfect
     answer with multiple columns, but most setups have one and the
     user expects the new-issue dialog to pre-fill from "the" column. */
  const firstId = Object.keys(inboxState.jiraFiltersByInstance)[0];
  const active = firstId
    ? inboxState.jiraFiltersByInstance[firstId]
    : { projectKey: null, sprintIds: [] as (number | 'backlog')[] };
  openModal('jiraCreate', {
    projectKey: active.projectKey ?? '',
    projects: inboxState.jiraProjectOptions,
    projectsLoading: false,
    issueTypes: [],
    issueTypeName: 'Task',
    summary: '',
    description: '',
    assigneeAccountId: '',
    assignees: [],
    assigneesLoading: false,
    sprints: inboxState.jiraSprintOptions,
    sprintId: active.sprintIds.find((s): s is number => typeof s === 'number') ?? null,
    busy: false,
    error: null,
  });
  if (!inboxState.jiraProjectOptions.length) {
    patchModal('jiraCreate', { projectsLoading: true });
    try {
      const projects = await invoke<JiraProject[]>('jira_list_projects');
      inboxState.jiraProjectOptions = projects;
      patchModal('jiraCreate', { projects, projectsLoading: false });
    } catch {
      patchModal('jiraCreate', { projectsLoading: false });
    }
  }
  if (modalsState.jiraCreate?.projectKey) {
    void onJiraCreateProjectChange(modalsState.jiraCreate.projectKey);
  }
}

export async function onJiraCreateProjectChange(key: string) {
  if (!modalsState.jiraCreate) return;
  // Project change wipes assignee — accountId is project-scoped (a user
  // assignable in PROJECTA may not exist as an option in PROJECTB), so
  // resetting avoids carrying a stale id forward.
  patchModal('jiraCreate', {
    projectKey: key,
    issueTypes: [],
    assignees: [],
    assigneeAccountId: '',
  });
  if (!key) return;
  void (async () => {
    try {
      const types = await invoke<JiraIssueType[]>('jira_list_issue_types', { projectKey: key });
      const m = modalsState.jiraCreate;
      if (!m) return;
      const preserved = types.find((t) => t.name === m.issueTypeName);
      const nextName = preserved ? preserved.name : types[0]?.name ?? 'Task';
      patchModal('jiraCreate', { issueTypes: types, issueTypeName: nextName });
    } catch {/* fallback to hardcoded list */}
  })();
  void (async () => {
    patchModal('jiraCreate', { assigneesLoading: true });
    try {
      const users = await invoke<JiraUserSummary[]>('jira_list_assignable_users', { projectKey: key });
      users.sort((a, b) => a.display_name.localeCompare(b.display_name));
      patchModal('jiraCreate', { assignees: users, assigneesLoading: false });
    } catch {
      patchModal('jiraCreate', { assigneesLoading: false });
    }
  })();
}

export async function submitJiraCreate() {
  const m = modalsState.jiraCreate;
  if (!m) return;
  const { projectKey, summary, description, issueTypeName, assigneeAccountId, sprintId } = m;
  if (!projectKey.trim() || !summary.trim() || !issueTypeName.trim()) return;
  patchModal('jiraCreate', { busy: true, error: null });
  try {
    const created = await invoke<JiraItem>('jira_create_issue', {
      projectKey: projectKey.trim(),
      issueType: issueTypeName,
      summary: summary.trim(),
      description,
      assigneeAccountId: assigneeAccountId.trim() || null,
      sprintId,
    });
    for (const id of Object.keys(inboxState.jiraItemsByInstance)) {
      inboxState.jiraItemsByInstance[id] = [
        created,
        ...inboxState.jiraItemsByInstance[id],
      ];
    }
    closeModal('jiraCreate');
    void refreshAllJiraInboxes({ silent: true });
  } catch (e) {
    patchModal('jiraCreate', { busy: false, error: typeof e === 'string' ? e : String(e) });
  }
}

// ---- GitHub Create PR flow ----

export async function openGithubCreatePr() {
  /* Pull repo default from the first inbox's filter, same
     trade-off as openJiraCreateIssue. */
  const firstId = Object.keys(inboxState.githubFiltersByInstance)[0];
  const activeRepo = firstId
    ? inboxState.githubFiltersByInstance[firstId].repo
    : null;
  openModal('githubCreatePr', {
    repo: activeRepo ?? '',
    repos: inboxState.githubRepoOptions.map((r) => ({
      owner: r.owner,
      name: r.name,
      full_name: r.full_name,
      default_branch: null,
    })),
    reposLoading: false,
    branches: [],
    branchesLoading: false,
    base: '',
    head: '',
    title: '',
    body: '',
    draft: false,
    compare: null,
    filesExpanded: false,
    busy: false,
    error: null,
  });
  if (!inboxState.githubRepoOptions.length) {
    patchModal('githubCreatePr', { reposLoading: true });
    try {
      const repos = await invoke<Repository[]>('github_list_repos');
      inboxState.githubRepoOptions = repos.map((r) => ({
        owner: r.owner,
        name: r.name,
        full_name: r.full_name,
      }));
      patchModal('githubCreatePr', {
        repos: repos.map((r) => ({
          owner: r.owner,
          name: r.name,
          full_name: r.full_name,
          default_branch: r.default_branch,
        })),
        reposLoading: false,
      });
    } catch {
      patchModal('githubCreatePr', { reposLoading: false });
    }
  }
  if (modalsState.githubCreatePr?.repo) {
    void onGithubPrRepoChange(modalsState.githubCreatePr.repo);
  }
}

export async function onGithubPrRepoChange(full: string) {
  if (!modalsState.githubCreatePr) return;
  patchModal('githubCreatePr', {
    repo: full,
    branches: [],
    base: '',
    head: '',
    compare: null,
    branchesLoading: !!full,
  });
  if (!full) return;
  const [owner, name] = full.split('/');
  if (!owner || !name) return;
  try {
    const branches = await invoke<RepoBranch[]>('github_list_repo_branches', { owner, repo: name });
    let defaultBranch =
      modalsState.githubCreatePr?.repos.find((r) => r.full_name === full)?.default_branch ?? null;
    if (!defaultBranch) {
      try {
        const repos = await invoke<Repository[]>('github_list_repos');
        defaultBranch = repos.find((r) => r.full_name === full)?.default_branch ?? null;
      } catch { /* ignore */ }
    }
    patchModal('githubCreatePr', {
      branches,
      branchesLoading: false,
      base: defaultBranch ?? branches[0]?.name ?? '',
    });
  } catch (e) {
    patchModal('githubCreatePr', {
      branchesLoading: false,
      error: typeof e === 'string' ? e : String(e),
    });
  }
}

export async function onGithubPrBranchesChange() {
  const m = modalsState.githubCreatePr;
  if (!m) return;
  if (m.head && !m.title.trim()) {
    const pretty = m.head
      .replace(/^[a-zA-Z]+\//, '')
      .replace(/[-_/]+/g, ' ')
      .trim()
      .split(' ')
      .filter(Boolean)
      .map((w) => w.charAt(0).toUpperCase() + w.slice(1))
      .join(' ');
    if (pretty) patchModal('githubCreatePr', { title: pretty });
  }
  if (!m.repo || !m.base || !m.head || m.base === m.head) {
    if (m.compare) patchModal('githubCreatePr', { compare: null });
    return;
  }
  const [owner, name] = m.repo.split('/');
  if (!owner || !name) return;
  patchModal('githubCreatePr', {
    compare: {
      loading: true,
      error: null,
      total_commits: 0,
      ahead_by: 0,
      behind_by: 0,
      additions: 0,
      deletions: 0,
      commits: [],
      files: [],
    },
  });
  try {
    const result = await invoke<CompareResult>('github_compare', {
      owner,
      repo: name,
      base: m.base,
      head: m.head,
    });
    patchModal('githubCreatePr', { compare: { loading: false, error: null, ...result } });
  } catch (e) {
    patchModal('githubCreatePr', {
      compare: {
        loading: false,
        error: typeof e === 'string' ? e : String(e),
        total_commits: 0,
        ahead_by: 0,
        behind_by: 0,
        additions: 0,
        deletions: 0,
        commits: [],
        files: [],
      },
    });
  }
}

export async function submitGithubPr(deps: ModalActionDeps) {
  const m = modalsState.githubCreatePr;
  if (!m) return;
  const { repo, base, head, title, body, draft } = m;
  if (!repo || !base || !head || base === head || !title.trim()) return;
  const [owner, name] = repo.split('/');
  if (!owner || !name) return;
  patchModal('githubCreatePr', { busy: true, error: null });
  try {
    const created = await invoke<InboxItem>('github_create_pr', {
      owner,
      repo: name,
      title: title.trim(),
      body,
      base,
      head,
      draft,
    });
    closeModal('githubCreatePr');
    for (const id of Object.keys(inboxState.itemsByInstance)) {
      inboxState.itemsByInstance[id] = [created, ...inboxState.itemsByInstance[id]];
    }
    openFocusItem(created);
    deps.setView('githubApp');
    void refreshAllInboxes({ silent: true });
  } catch (e) {
    patchModal('githubCreatePr', { busy: false, error: typeof e === 'string' ? e : String(e) });
  }
}
