/* Top-level command-palette actions (`docs/ROADMAP_1.0.md §2.8.6`).
 *
 * Pure builder — given a set of callbacks for the things this depends
 * on (open a connect modal, disconnect a source, flip the cheatsheet
 * toggle, switch view, create a workbench), it returns the list of
 * actions the palette renders in its top "Actions" section.
 *
 * Lives outside `+page.svelte` so the action list is testable, the
 * palette doesn't need a 100-line derived inline, and the labels +
 * keyword maps are in one obvious place when extending. The actual
 * state read (`connectionsState`) is wrapped via the `isConnected`
 * callback so this module stays free of reactivity imports. */

import type { ConnectionMeta } from '$lib/data';
import type { PaletteAction } from '$lib/components/ui/CommandPalette.svelte';

export interface PaletteActionsContext {
  sourceConns: ConnectionMeta[];
  agentConns: ConnectionMeta[];
  /** True if the source's `connectionsState[id]` reads as connected.
   *  Pure passthrough — keeps this module unaware of the reactive
   *  store shape. */
  isConnected: (sourceId: 'github' | 'jira' | 'sentry') => boolean;
  /** Pop the source-specific connect modal. */
  openConnectModal: (conn: ConnectionMeta) => void;
  /** Disconnect helpers. The palette dispatches by `conn.id`; this
   *  bag avoids a per-source switch in the builder. */
  disconnectGithub: () => void | Promise<void>;
  disconnectJira: () => void | Promise<void>;
  disconnectSentry: () => void | Promise<void>;
  /** Flip the cheatsheet overlay open. */
  openCheatsheet: () => void;
  /** Spawn a new workbench, activate it, and ensure the user lands
   *  on the workbench view. */
  createWorkbench: () => void;
  /** Switch the top-level view. */
  setView: (v: 'workbench' | 'githubTab' | 'jiraTab' | 'sentryTab' | 'rules' | 'connections' | 'settings') => void;
}

export function buildPaletteActions(ctx: PaletteActionsContext): PaletteAction[] {
  const a: PaletteAction[] = [];

  /* Source connect / reconnect / disconnect. The label flips on the
   * live connection state so typing "connect github" surfaces the
   * action that matches the user's intent. */
  for (const conn of ctx.sourceConns) {
    if (!conn.implemented) continue;
    const isConn = ctx.isConnected(conn.id as 'github' | 'jira' | 'sentry');
    a.push({
      id: `connect:${conn.id}`,
      label: isConn ? `Reconnect ${conn.name}` : `Connect ${conn.name}`,
      sub: isConn ? 'Re-enter token in the modal' : 'Open the connect modal',
      keywords: `${conn.id} pat token auth`,
      pick: () => ctx.openConnectModal(conn)
    });
    if (isConn) {
      a.push({
        id: `disconnect:${conn.id}`,
        label: `Disconnect ${conn.name}`,
        sub: 'Drop the token from Keychain',
        keywords: `${conn.id} sign out logout`,
        pick: () => {
          if (conn.id === 'github') void ctx.disconnectGithub();
          else if (conn.id === 'jira') void ctx.disconnectJira();
          else if (conn.id === 'sentry') void ctx.disconnectSentry();
        }
      });
    }
  }

  /* Agents — status check (the user can't "connect" an agent; the
   * status modal just confirms the binary is detected). */
  for (const conn of ctx.agentConns) {
    if (!conn.implemented) continue;
    a.push({
      id: `status:${conn.id}`,
      label: `Check ${conn.name} status`,
      sub: 'Detect binary + version',
      keywords: `${conn.id} cli agent`,
      pick: () => ctx.openConnectModal(conn)
    });
  }

  a.push({
    id: 'cheatsheet',
    label: 'Show keyboard shortcuts',
    sub: 'Cheatsheet of every binding',
    keywords: 'help ? shortcuts hotkeys',
    pick: () => ctx.openCheatsheet()
  });
  a.push({
    id: 'workbench:new',
    label: 'New workbench',
    sub: 'Create a fresh column tab',
    keywords: 'create add tab',
    pick: () => ctx.createWorkbench()
  });
  a.push({
    id: 'view:settings',
    label: 'Open settings',
    keywords: 'preferences config theme privacy updates docs',
    pick: () => ctx.setView('settings')
  });
  a.push({
    id: 'view:connections',
    label: 'Open connections',
    keywords: 'sources tokens auth',
    pick: () => ctx.setView('connections')
  });
  a.push({
    id: 'view:rules',
    label: 'Open rules',
    keywords: 'system prompt agent',
    pick: () => ctx.setView('rules')
  });
  return a;
}
