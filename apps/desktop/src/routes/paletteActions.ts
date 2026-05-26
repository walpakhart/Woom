/* Top-level command-palette actions, factored out of `+page.svelte`
 * (phase 9 split). Built every render of the host `$derived.by` —
 * cheap (~10 entries), no caching needed. Labels flip between
 * Connect / Reconnect / Disconnect based on `connectionsState` so
 * typing `connect github` surfaces the right verb. */

import type { ConnectionMeta } from '$lib/data';

/* Structural snapshot of `connectionsState` — only the source-status
 * fields the palette reads. Keep the actual store import out of this
 * pure helper so the module stays tree-shake-friendly. We use an
 * intentionally loose index signature (allows the agent/non-source
 * fields like `claude`, `cursor`, `statusLoading`, etc.) and read
 * `kind === 'connected'` on the source ones. */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
type ConnectionsSnapshot = { [k: string]: any };

export interface PaletteAction {
  id: string;
  label: string;
  sub?: string;
  keywords?: string;
  pick: () => void;
}

export interface PaletteActionsDeps {
  /** Reactive snapshot — pass `connectionsState` directly; we read live. */
  connectionsState: ConnectionsSnapshot;
  /** Connection metadata. Pure data. */
  sourceConns: ConnectionMeta[];
  agentConns: ConnectionMeta[];
  /** Open the connect modal for a source. */
  openConnectModal: (conn: ConnectionMeta) => void;
  /** Disconnect callbacks — one per source kind. */
  disconnectGithub: () => void | Promise<void>;
  disconnectJiraAll: () => void | Promise<void>;
  disconnectSentryAll: () => void | Promise<void>;
  /** UI toggles owned by the host page. */
  openCheatsheet: () => void;
  setView: (v: 'settings' | 'connections' | 'rules') => void;
}

export function buildPaletteActions(deps: PaletteActionsDeps): PaletteAction[] {
  const {
    connectionsState,
    sourceConns,
    agentConns,
    openConnectModal,
    disconnectGithub,
    disconnectJiraAll,
    disconnectSentryAll,
    openCheatsheet,
    setView,
  } = deps;
  const a: PaletteAction[] = [];
  /* Source connect / disconnect. Use the connectionsMeta source list
   * so this stays in sync if a new source is added. */
  for (const conn of sourceConns) {
    if (!conn.implemented) continue;
    const status = connectionsState[conn.id as 'github' | 'jira' | 'sentry'];
    const isConnected = status?.kind === 'connected';
    a.push({
      id: `connect:${conn.id}`,
      label: isConnected ? `Reconnect ${conn.name}` : `Connect ${conn.name}`,
      sub: isConnected ? 'Re-enter token in the modal' : 'Open the connect modal',
      keywords: `${conn.id} pat token auth`,
      pick: () => openConnectModal(conn),
    });
    if (isConnected) {
      a.push({
        id: `disconnect:${conn.id}`,
        label: `Disconnect ${conn.name}`,
        sub: 'Drop the token from Keychain',
        keywords: `${conn.id} sign out logout`,
        pick: () => {
          if (conn.id === 'github') void disconnectGithub();
          else if (conn.id === 'jira') void disconnectJiraAll();
          else if (conn.id === 'sentry') void disconnectSentryAll();
        },
      });
    }
  }
  /* Agents — open status modals so the user can verify the binary
   * is detected. */
  for (const conn of agentConns) {
    if (!conn.implemented) continue;
    a.push({
      id: `status:${conn.id}`,
      label: `Check ${conn.name} status`,
      sub: 'Detect binary + version',
      keywords: `${conn.id} cli agent`,
      pick: () => openConnectModal(conn),
    });
  }
  a.push({
    id: 'cheatsheet',
    label: 'Show keyboard shortcuts',
    sub: 'Cheatsheet of every binding',
    keywords: 'help ? shortcuts hotkeys',
    pick: openCheatsheet,
  });
  a.push({
    id: 'view:settings',
    label: 'Open settings',
    keywords: 'preferences config theme privacy updates docs',
    pick: () => setView('settings'),
  });
  a.push({
    id: 'view:connections',
    label: 'Open connections',
    keywords: 'sources tokens auth',
    pick: () => setView('connections'),
  });
  a.push({
    id: 'view:rules',
    label: 'Open rules',
    keywords: 'system prompt agent',
    pick: () => setView('rules'),
  });
  return a;
}
