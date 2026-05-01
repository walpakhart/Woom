// Source + agent connection statuses. Owns the raw status objects returned
// by the Rust IPC (`github_status`, `jira_status`, `agent_status`) plus the
// derived convenience flags consumers reach for (`connectedGithub`, the
// `connectedIds` set, source/agent filters, …).
//
// Every status refresh is logged to `connectionEventsState` via
// `recordConnectionEvent` so the diagnostics card and the per-card "Test
// connection" button have a uniform ground truth.

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
import {
  lastEventForSource,
  recordConnectionEvent,
  type ConnectionEventSource
} from './connectionEvents.svelte';

export const connectionsState = $state<{
  github: ConnectionStatus;
  jira: JiraStatus;
  sentry: SentryStatus;
  claude: ClaudeStatus | null;
  cursor: CursorStatus | null;
  statusLoading: boolean;
  /** `true` while a per-source test is in flight. Drives the spinner
   *  on the `Test` button in `ConnectionsView`. */
  testing: Record<ConnectionEventSource, boolean>;
  /** `true` while the boot retry/backoff loop is mid-attempt for a
   *  source — i.e. the first refresh threw a transient error and we're
   *  about to retry. Drives a "Retrying…" indicator in the rail and
   *  the per-card status pill so a flaky network on launch doesn't
   *  read as a permanent disconnect. Cleared once the source settles
   *  (connected / disconnected / rate_limited) or the retry budget is
   *  exhausted. */
  retrying: Record<ConnectionEventSource, boolean>;
}>({
  github: { kind: 'disconnected' },
  jira: { kind: 'disconnected' },
  sentry: { kind: 'disconnected' },
  claude: null,
  cursor: null,
  statusLoading: true,
  testing: {
    github: false,
    jira: false,
    sentry: false,
    claude: false,
    cursor: false
  },
  retrying: {
    github: false,
    jira: false,
    sentry: false,
    claude: false,
    cursor: false
  }
});

export const sourceConns = connectionsMeta.filter((c) => c.category === 'sources');
export const agentConns = connectionsMeta.filter((c) => c.category === 'agents');

export async function refreshGithubStatus() {
  connectionsState.statusLoading = true;
  const start = performance.now();
  try {
    connectionsState.github = await invoke<ConnectionStatus>('github_status');
    const latencyMs = Math.round(performance.now() - start);
    if (connectionsState.github.kind === 'connected') {
      const rl = connectionsState.github.rate_limit;
      const quotaSuffix = rl
        ? ` · ${rl.remaining}/${rl.limit} left`
        : '';
      recordConnectionEvent('github', 'connected', {
        latencyMs,
        message: `as @${connectionsState.github.user.login}${quotaSuffix}`
      });
    } else {
      recordConnectionEvent('github', 'disconnected', {
        latencyMs,
        message: 'no token in Keychain'
      });
    }
  } catch (e) {
    console.error('github_status', e);
    connectionsState.github = { kind: 'disconnected' };
    /* `GithubError::RateLimited` flattens to "rate limited — try again
     *  later" in `e.to_string()`. Pattern-match on that so the event
     *  log distinguishes "your token's quota is exhausted" from a
     *  generic network error — different user remediations. */
    const msg = stringifyError(e);
    const isRateLimit = /rate.?limit|too many requests|429/i.test(msg);
    recordConnectionEvent('github', isRateLimit ? 'rate_limited' : 'error', {
      latencyMs: Math.round(performance.now() - start),
      message: msg
    });
  } finally {
    connectionsState.statusLoading = false;
  }
}

export async function refreshJiraStatus() {
  const start = performance.now();
  try {
    connectionsState.jira = await invoke<JiraStatus>('jira_status');
    const latencyMs = Math.round(performance.now() - start);
    if (connectionsState.jira.kind === 'connected') {
      recordConnectionEvent('jira', 'connected', {
        latencyMs,
        message: `${connectionsState.jira.user.display_name} on ${connectionsState.jira.user.workspace}`
      });
    } else {
      recordConnectionEvent('jira', 'disconnected', {
        latencyMs,
        message: 'no token in Keychain'
      });
    }
  } catch (e) {
    console.error('jira_status', e);
    connectionsState.jira = { kind: 'disconnected' };
    recordConnectionEvent('jira', 'error', {
      latencyMs: Math.round(performance.now() - start),
      message: stringifyError(e)
    });
  }
}

export async function refreshSentryStatus() {
  const start = performance.now();
  try {
    connectionsState.sentry = await invoke<SentryStatus>('sentry_status');
    const latencyMs = Math.round(performance.now() - start);
    if (connectionsState.sentry.kind === 'connected') {
      recordConnectionEvent('sentry', 'connected', {
        latencyMs,
        message: `${connectionsState.sentry.user.organization_slug} on ${connectionsState.sentry.user.host.replace(/^https?:\/\//, '')}`
      });
    } else {
      recordConnectionEvent('sentry', 'disconnected', {
        latencyMs,
        message: 'no token in Keychain'
      });
    }
  } catch (e) {
    console.error('sentry_status', e);
    connectionsState.sentry = { kind: 'disconnected' };
    recordConnectionEvent('sentry', 'error', {
      latencyMs: Math.round(performance.now() - start),
      message: stringifyError(e)
    });
  }
}

export async function refreshClaudeStatus() {
  /* `agent_status` returns both CLIs in one round trip — cheaper than two
   *  separate Tauri calls and keeps the two status flags in lockstep. */
  const start = performance.now();
  try {
    const agentStatus = await invoke<AgentStatus>('agent_status');
    connectionsState.claude = agentStatus.claude;
    connectionsState.cursor = agentStatus.cursor;
    const latencyMs = Math.round(performance.now() - start);
    recordAgentEvent('claude', agentStatus.claude, latencyMs);
    recordAgentEvent('cursor', agentStatus.cursor, latencyMs);
  } catch (e) {
    console.error('agent_status', e);
    connectionsState.claude = null;
    connectionsState.cursor = null;
    const latencyMs = Math.round(performance.now() - start);
    recordConnectionEvent('claude', 'error', { latencyMs, message: stringifyError(e) });
    recordConnectionEvent('cursor', 'error', { latencyMs, message: stringifyError(e) });
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

/** Boot variant of `refreshAllStatus`. Wraps each per-source refresh
 *  in an exponential-backoff retry so a single network blip during
 *  app launch doesn't leave a source disconnected until the user hits
 *  reconnect manually. Up to 4 attempts at 0s / 2s / 6s / 14s — total
 *  budget ~22 s, well under the 30 s spec target.
 *
 *  Only `error` outcomes (transient: network blip, 5xx, DNS) are
 *  retried. `disconnected` (no token in keychain — intentional) and
 *  `rate_limited` (different remediation: wait for `Retry-After`) and
 *  `connected` (already settled) all short-circuit. `connectionsState
 *  .retrying[source]` flips true between attempts so the UI can render
 *  a "Retrying…" cue instead of a permanent disconnect dot. */
export async function refreshAllStatusOnBoot() {
  await Promise.all([
    refreshWithBootRetry('github', refreshGithubStatus),
    refreshWithBootRetry('jira', refreshJiraStatus),
    refreshWithBootRetry('sentry', refreshSentryStatus),
    refreshAgentsWithBootRetry()
  ]);
}

const BOOT_RETRY_DELAYS_MS = [0, 2_000, 6_000, 14_000];

async function refreshWithBootRetry(
  source: ConnectionEventSource,
  refresh: () => Promise<void>
): Promise<void> {
  for (let attempt = 0; attempt < BOOT_RETRY_DELAYS_MS.length; attempt++) {
    if (attempt > 0) {
      connectionsState.retrying[source] = true;
      await delay(BOOT_RETRY_DELAYS_MS[attempt]);
    }
    await refresh();
    /* `refresh` swallows the error and records an event; inspect the
     * latest event to decide whether to retry. Anything other than
     * `error` is a settled outcome (connected / disconnected /
     * rate_limited) and we stop. */
    const last = lastEventForSource(source);
    if (!last || last.kind !== 'error') break;
  }
  connectionsState.retrying[source] = false;
}

/** Claude + Cursor share a single Tauri call (`agent_status`), so the
 *  retry loop has to look at *both* sources' last events to decide
 *  whether to keep going. We retry while either is still erroring. */
async function refreshAgentsWithBootRetry(): Promise<void> {
  for (let attempt = 0; attempt < BOOT_RETRY_DELAYS_MS.length; attempt++) {
    if (attempt > 0) {
      connectionsState.retrying.claude = true;
      connectionsState.retrying.cursor = true;
      await delay(BOOT_RETRY_DELAYS_MS[attempt]);
    }
    await refreshClaudeStatus();
    const claudeLast = lastEventForSource('claude');
    const cursorLast = lastEventForSource('cursor');
    const stillErroring =
      (claudeLast?.kind === 'error') || (cursorLast?.kind === 'error');
    if (!stillErroring) break;
  }
  connectionsState.retrying.claude = false;
  connectionsState.retrying.cursor = false;
}

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

/** Manual "Test connection" trigger. Functionally identical to a refresh
 *  but flips `connectionsState.testing[source]` so the per-card button
 *  can render a spinner — and groups Claude+Cursor under one call so
 *  hitting "Test" on either card animates both (they share the IPC). */
export async function testConnection(source: ConnectionEventSource): Promise<void> {
  if (connectionsState.testing[source]) return;
  connectionsState.testing[source] = true;
  if (source === 'claude' || source === 'cursor') {
    /* Claude and Cursor share `agent_status`; mark both busy so the
     *  spinner is consistent regardless of which card the user clicked. */
    connectionsState.testing.claude = true;
    connectionsState.testing.cursor = true;
  }
  try {
    switch (source) {
      case 'github':
        await refreshGithubStatus();
        break;
      case 'jira':
        await refreshJiraStatus();
        break;
      case 'sentry':
        await refreshSentryStatus();
        break;
      case 'claude':
      case 'cursor':
        await refreshClaudeStatus();
        break;
    }
  } finally {
    connectionsState.testing[source] = false;
    if (source === 'claude' || source === 'cursor') {
      connectionsState.testing.claude = false;
      connectionsState.testing.cursor = false;
    }
  }
}

function recordAgentEvent(
  source: 'claude' | 'cursor',
  status: ClaudeStatus | CursorStatus | null,
  latencyMs: number
): void {
  if (status?.ready) {
    recordConnectionEvent(source, 'connected', {
      latencyMs,
      message: status.version ? `version ${status.version}` : undefined
    });
  } else {
    recordConnectionEvent(source, 'disconnected', {
      latencyMs,
      message: status?.path ? `binary at ${status.path} not ready` : 'binary not detected'
    });
  }
}

function stringifyError(e: unknown): string {
  if (typeof e === 'string') return e;
  if (e && typeof e === 'object' && 'message' in e) {
    return String((e as { message: unknown }).message);
  }
  try {
    return JSON.stringify(e);
  } catch {
    return String(e);
  }
}
