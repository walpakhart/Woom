// Inbox/view/instance MCP-tool dispatcher cases extracted from
// `handleAppNavigation` in `+page.svelte` (wave-32 split). Covers all
// 21 non-canvas/SDD cases (open_jira_issue, switch_view, open_repo,
// set_*_instance, set_editor_repo_path, set_agent_cwd, etc).
//
// Stateful caller deps come in via the `InboxMcpDeps` interface so
// this module never touches `+page.svelte` directly. Returns `true`
// when a case matched.

import {
  num as _mcpNum,
  pickDeep,
  pickFrom,
  str as _mcpStr,
  REPO_PATH_KEYS_DEEP,
  INSTANCE_NAME_KEYS_DEEP,
  INSTANCE_ID_KEYS_DEEP,
} from './mcpInputParse';
import { inboxState } from '$lib/state/inbox.svelte';
import { sessionsState } from '$lib/state/sessions.svelte';
import { applySessionCwd } from '$lib/services/sessionCwd';
import type { GithubFilters, GithubFilterMode, JiraFilters, SprintScope } from '$lib/state/inbox.svelte';
import {
  updateGithubFilters,
  updateJiraFilters,
  updateJiraTabFilters,
  setSentryFilters,
  scheduleSentryTabFilterRefresh,
  openSentryFocus,
} from '$lib/state/inbox.svelte';
import { connectionsMeta, type ConnectionMeta } from '$lib/data';
import {
  APP_INSTANCE_IDS,
  MULTI_INSTANCE_KINDS,
  addInstance as addLayoutInstance,
} from '$lib/state/layout.svelte';
import type { PanelKind } from '$lib/types';

// View enum mirrors `+page.svelte`'s local View. Kept loose (`string`)
// here so we don't force the helper to import from the route file.
type View = string;
type DetailTab = string;
type SentryStatus = string;
type SentryLevel = string;
type SentryFilterPatch = {
  projects?: string[];
  search?: string;
  status?: SentryStatus;
  level?: SentryLevel;
  environment?: string | null;
};

export interface InboxMcpDeps {
  /** Local `view` setter. Required because Svelte 5 `let`-state
   *  can't be passed by ref. */
  setView(v: View): void;
  /** Local `tab` setter for the GitHub focus pane. */
  setTab(t: DetailTab): void;
  /** Maps an agent-supplied view name (`github`/`jira`/`sentry`/…)
   *  to the matching `…App` view enum. Returns null for unknown. */
  mapAgentViewToInternal(v: string): View | null;
  /** Resolves a (kind, name?, id?) instance triple to the singleton
   *  layout record. Returns `null` only for invalid kinds. */
  findInstanceByNameOrId(
    kind: PanelKind,
    name: string,
    id: string,
  ): { id: string; kind: PanelKind; name: string; width: number } | null;
  /** Updates the editor app's repo path for the given instance id. */
  setEditorRepoPath(repoPath: string, instanceId?: string): void;
  /** Async — fetches a GitHub item by `(owner, repo, number)` and
   *  surfaces it in the focus pane. Tab hint is optional. */
  resolveGithubFocus(
    owner: string,
    repo: string,
    number: number,
    tabHint?: DetailTab | null,
  ): void | Promise<void>;
  /** Opens the connections modal for the supplied source meta. */
  openConnectModal(conn: ConnectionMeta): void;
  /** Type predicates — kept on the route file because they live with
   *  the canonical filter enum types. */
  isGithubFilterMode(s: string): s is GithubFilterMode;
  isSentryStatus(s: string): s is SentryStatus;
  isSentryLevel(s: string): s is SentryLevel;
  /** Parses an agent's sprint-id array into the canonical
   *  SprintScope tuples (mixes numeric ids + the 'backlog' literal). */
  parseSprintScopes(raw: unknown[]): SprintScope[];
}

export function handleInboxOrViewMcp(
  sessionId: string,
  name: string,
  input: Record<string, unknown>,
  deps: InboxMcpDeps,
): boolean {
  const str = (k: string): string => _mcpStr(input, k);
  const num = (k: string): number => _mcpNum(input, k);

  switch (name) {
    case 'mcp__app__open_jira_issue': {
      const key = str('key');
      if (key) {
        inboxState.jiraFocusKey = key;
        deps.setView('jiraApp');
      }
      return true;
    }
    case 'mcp__app__open_sentry_issue': {
      const id = str('id');
      if (id) {
        openSentryFocus(id);
        deps.setView('sentryApp');
      }
      return true;
    }
    case 'mcp__app__open_sentry_event': {
      const id = str('issue_id');
      const eventId = str('event_id') || null;
      if (id) {
        openSentryFocus(id, eventId);
        deps.setView('sentryApp');
      }
      return true;
    }
    case 'mcp__app__open_github_pr':
    case 'mcp__app__open_github_issue': {
      const owner = str('owner');
      const repo = str('repo');
      const n = num('number');
      if (!owner || !repo || !Number.isFinite(n)) return true;
      const tabHint = str('tab') as DetailTab | '';
      void deps.resolveGithubFocus(owner, repo, n, tabHint || null);
      return true;
    }
    case 'mcp__app__switch_view': {
      const v = str('view');
      const mapped = deps.mapAgentViewToInternal(v);
      if (mapped) deps.setView(mapped);
      return true;
    }
    case 'mcp__app__open_repo': {
      const repoPath = str('repo_path');
      deps.setView('editorApp');
      if (repoPath) deps.setEditorRepoPath(repoPath, APP_INSTANCE_IDS.editor);
      return true;
    }
    case 'mcp__app__open_connect_modal': {
      const sourceId = str('source');
      const conn = connectionsMeta.find((c) => c.id === sourceId);
      if (conn) deps.openConnectModal(conn);
      return true;
    }
    case 'mcp__app__open_github_repo': {
      const owner = str('owner');
      const repo = str('repo');
      const section = str('section') || 'pulls';
      const path = str('path');
      if (!owner || !repo) return true;
      deps.setView('githubApp');
      inboxState.pendingRepoNav = {
        owner,
        repo,
        section,
        path: section === 'code' && path ? path : null,
      };
      return true;
    }
    case 'mcp__app__open_jira_tab': {
      const patch: Partial<JiraFilters> = {};
      if ('project_key' in input) patch.projectKey = str('project_key') || null;
      if ('search' in input) patch.search = str('search');
      if ('status_name' in input) patch.statusName = str('status_name') || null;
      if (Array.isArray(input.board_ids)) {
        patch.boardIds = input.board_ids
          .map((x) => Number(x))
          .filter((x): x is number => Number.isFinite(x) && x > 0);
      }
      if (Array.isArray(input.sprint_ids)) {
        patch.sprintIds = deps.parseSprintScopes(input.sprint_ids);
      }
      deps.setView('jiraApp');
      updateJiraTabFilters(patch);
      return true;
    }
    case 'mcp__app__open_sentry_tab': {
      deps.setView('sentryApp');
      if (Array.isArray(input.projects)) {
        inboxState.sentryTabProjects = input.projects
          .map((x) => String(x))
          .filter((s) => s.length > 0);
      }
      if ('search' in input) inboxState.sentryTabSearch = str('search');
      if ('status' in input) {
        const s = str('status');
        if (s) inboxState.sentryTabStatus = s as typeof inboxState.sentryTabStatus;
      }
      if ('level' in input) {
        const l = str('level');
        if (l) inboxState.sentryTabLevel = l as typeof inboxState.sentryTabLevel;
      }
      if ('environment' in input) {
        const e = str('environment');
        inboxState.sentryTabEnvironment = e ? e : null;
      }
      scheduleSentryTabFilterRefresh();
      return true;
    }
    case 'mcp__app__set_github_instance': {
      const inst = deps.findInstanceByNameOrId('github', str('instance_name'), str('instance_id'));
      if (!inst) return true;
      const patch: Partial<GithubFilters> = {};
      if ('repo' in input) {
        const r = str('repo');
        patch.repo = r ? r : null;
      }
      if ('mode' in input) {
        const m = str('mode');
        if (deps.isGithubFilterMode(m)) patch.mode = m;
      }
      if ('search' in input) patch.search = str('search');
      if ('custom_user' in input) patch.customUser = str('custom_user');
      deps.setView('githubApp');
      updateGithubFilters(inst.id, patch);
      return true;
    }
    case 'mcp__app__set_jira_instance': {
      const inst = deps.findInstanceByNameOrId('jira', str('instance_name'), str('instance_id'));
      if (!inst) return true;
      const patch: Partial<JiraFilters> = {};
      if ('project_key' in input) {
        const p = str('project_key');
        patch.projectKey = p ? p : null;
      }
      if ('status_name' in input) {
        const s = str('status_name');
        patch.statusName = s ? s : null;
      }
      if ('search' in input) patch.search = str('search');
      if (Array.isArray(input.board_ids)) {
        patch.boardIds = input.board_ids
          .map((x) => Number(x))
          .filter((x): x is number => Number.isFinite(x) && x > 0);
      }
      if (Array.isArray(input.sprint_ids)) {
        patch.sprintIds = deps.parseSprintScopes(input.sprint_ids);
      }
      deps.setView('jiraApp');
      updateJiraFilters(inst.id, patch);
      return true;
    }
    case 'mcp__app__set_sentry_instance': {
      const inst = deps.findInstanceByNameOrId('sentry', str('instance_name'), str('instance_id'));
      if (!inst) return true;
      const patch: SentryFilterPatch = {};
      if (Array.isArray(input.projects)) {
        patch.projects = input.projects
          .map((x) => String(x))
          .filter((s) => s.length > 0);
      }
      if ('search' in input) patch.search = str('search');
      if ('status' in input) {
        const s = str('status');
        if (deps.isSentryStatus(s)) patch.status = s;
      }
      if ('level' in input) {
        const l = str('level');
        if (deps.isSentryLevel(l)) patch.level = l;
      }
      if ('environment' in input) {
        const e = str('environment');
        patch.environment = e ? e : null;
      }
      deps.setView('sentryApp');
      setSentryFilters(inst.id, patch);
      return true;
    }
    case 'mcp__app__set_editor_repo_path': {
      const repoPath = pickDeep(input as Record<string, unknown>, REPO_PATH_KEYS_DEEP);
      const instName = pickDeep(input as Record<string, unknown>, INSTANCE_NAME_KEYS_DEEP);
      const instId = pickDeep(input as Record<string, unknown>, INSTANCE_ID_KEYS_DEEP);
      if (!repoPath) return true;
      const editor = deps.findInstanceByNameOrId('editor', instName, instId);
      if (!editor) return true;
      deps.setView('editorApp');
      deps.setEditorRepoPath(repoPath, editor.id);
      return true;
    }
    case 'mcp__app__set_agent_cwd': {
      const repoPath = pickDeep(input as Record<string, unknown>, REPO_PATH_KEYS_DEEP);
      if (!repoPath) return true;
      const target = str('target').toLowerCase();
      let sessId: string | null = null;
      if (target === 'self') {
        sessId = sessionId;
      } else {
        const instName = pickDeep(input as Record<string, unknown>, INSTANCE_NAME_KEYS_DEEP);
        const instId = pickDeep(input as Record<string, unknown>, INSTANCE_ID_KEYS_DEEP);
        const inst = deps.findInstanceByNameOrId('claude', instName, instId)
          ?? deps.findInstanceByNameOrId('cursor', instName, instId);
        if (inst) {
          deps.setView(inst.kind === 'cursor' ? 'cursorApp' : 'claudeApp');
          sessId = sessionsState.activeByInstance[inst.id] ?? null;
        }
      }
      if (!sessId) return true;
      applySessionCwd(sessId, repoPath, { breakLink: false });
      return true;
    }
    case 'mcp__app__list_instances': {
      // No-op: data lives in the system-prompt preamble.
      return true;
    }
    case 'mcp__app__add_app_instance': {
      const kindRaw = str('kind').toLowerCase();
      const VALID_KINDS: PanelKind[] = [
        'github', 'jira', 'sentry', 'claude', 'cursor',
        'editor', 'canvas', 'terminal',
      ];
      if (!(VALID_KINDS as readonly string[]).includes(kindRaw)) return true;
      const kind = kindRaw as PanelKind;
      const VIEW_BY_KIND: Record<PanelKind, View> = {
        github: 'githubApp',
        jira: 'jiraApp',
        sentry: 'sentryApp',
        claude: 'claudeApp',
        cursor: 'cursorApp',
        editor: 'editorApp',
        canvas: 'canvasApp',
        terminal: 'terminalApp',
      };
      if (!MULTI_INSTANCE_KINDS.has(kind)) {
        deps.setView(VIEW_BY_KIND[kind]);
        return true;
      }
      const inst = addLayoutInstance(kind);
      if (!inst) return true;
      if (kind === 'editor') {
        const repoPath = pickDeep(input as Record<string, unknown>, REPO_PATH_KEYS_DEEP);
        if (repoPath) deps.setEditorRepoPath(repoPath, inst.id);
      }
      deps.setView(VIEW_BY_KIND[kind]);
      return true;
    }
  }
  return false;
}

// Re-export for the wave-30 mcpInputParse imports to keep working.
export { pickFrom };
