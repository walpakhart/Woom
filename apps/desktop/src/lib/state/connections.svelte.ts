// Source + agent connection statuses. Owns the raw status objects returned
// by the Rust IPC (`github_status`, `jira_status`, `agent_status`) plus the
// derived convenience flags consumers reach for (`connectedGithub`, the
// `connectedIds` set, source/agent filters, …).

import { invoke } from '@tauri-apps/api/core';
import {
  connectionsMeta,
  type AgentStatus,
  type ClaudeStatus,
  type ConnectionStatus,
  type CursorStatus,
  type JiraStatus,
  type SentryStatus
} from '$lib/data';

export const connectionsState = $state<{
  github: ConnectionStatus;
  jira: JiraStatus;
  sentry: SentryStatus;
  claude: ClaudeStatus | null;
  cursor: CursorStatus | null;
  statusLoading: boolean;
}>({
  github: { kind: 'disconnected' },
  jira: { kind: 'disconnected' },
  sentry: { kind: 'disconnected' },
  claude: null,
  cursor: null,
  statusLoading: true
});

export const sourceConns = connectionsMeta.filter((c) => c.category === 'sources');
export const agentConns = connectionsMeta.filter((c) => c.category === 'agents');

export async function refreshGithubStatus() {
  connectionsState.statusLoading = true;
  try {
    connectionsState.github = await invoke<ConnectionStatus>('github_status');
  } catch (e) {
    console.error('github_status', e);
    connectionsState.github = { kind: 'disconnected' };
  } finally {
    connectionsState.statusLoading = false;
  }
}

export async function refreshJiraStatus() {
  try {
    connectionsState.jira = await invoke<JiraStatus>('jira_status');
  } catch (e) {
    console.error('jira_status', e);
    connectionsState.jira = { kind: 'disconnected' };
  }
}

export async function refreshSentryStatus() {
  try {
    connectionsState.sentry = await invoke<SentryStatus>('sentry_status');
  } catch (e) {
    console.error('sentry_status', e);
    connectionsState.sentry = { kind: 'disconnected' };
  }
}

export async function refreshClaudeStatus() {
  // `agent_status` returns both CLIs in one round trip — cheaper than two
  // separate Tauri calls and keeps the two status flags in lockstep.
  try {
    const agentStatus = await invoke<AgentStatus>('agent_status');
    connectionsState.claude = agentStatus.claude;
    connectionsState.cursor = agentStatus.cursor;
  } catch (e) {
    console.error('agent_status', e);
    connectionsState.claude = null;
    connectionsState.cursor = null;
  }
}

export async function refreshAllStatus() {
  await Promise.all([
    refreshGithubStatus(),
    refreshJiraStatus(),
    refreshSentryStatus(),
    refreshClaudeStatus()
  ]);
}
