/* MCP-payload type guards + small parsers, factored out of
 * `+page.svelte` (phase-9 split). All pure — no Svelte state, no
 * side-effects. Used by `handleAppNavigation` to narrow string
 * inputs coming over the agent IPC into the typed unions the rest
 * of the codebase expects. Defined here instead of inside the inbox
 * state module because they're only needed for the agent-driven
 * path; the UI dropdowns build their unions by construction. */

import type { GithubFilterMode, SprintScope } from '$lib/state/inbox-shared.svelte';

/** Narrow a string to the `GithubFilterMode` union — anything else
 *  (typo from the agent, future mode the frontend doesn't know)
 *  silently no-ops, matching the rest of `handleAppNavigation`'s
 *  "bad input = skip" contract. */
export function isGithubFilterMode(s: string): s is GithubFilterMode {
  return (
    s === 'involving' ||
    s === 'authored' ||
    s === 'review_requested' ||
    s === 'assigned' ||
    s === 'user' ||
    s === 'all'
  );
}

export type SentryStatus = 'unresolved' | 'resolved' | 'ignored' | 'all';
export type SentryLevel = 'all' | 'fatal' | 'error' | 'warning' | 'info' | 'debug';

export function isSentryStatus(s: string): s is SentryStatus {
  return s === 'unresolved' || s === 'resolved' || s === 'ignored' || s === 'all';
}

export function isSentryLevel(s: string): s is SentryLevel {
  return (
    s === 'all' ||
    s === 'fatal' ||
    s === 'error' ||
    s === 'warning' ||
    s === 'info' ||
    s === 'debug'
  );
}

/** Subset of the column-store SentryFilters shape that the agent is
 *  allowed to patch. Excludes `sort` (which isn't exposed over MCP)
 *  so the typed `unknown` payload narrowing doesn't have to satisfy
 *  the full persisted-filter shape. */
export type SentryFilterPatch = {
  projects?: string[];
  search?: string;
  status?: SentryStatus;
  level?: SentryLevel;
  environment?: string | null;
};

/** Internal `View` union — kept in sync with the one declared inside
 *  `+page.svelte`. We re-declare here so this module can stand alone
 *  without a circular Svelte→helper→Svelte import. If you add a new
 *  view there, mirror it here AND extend `mapAgentViewToInternal`. */
export type ViewName =
  | 'home'
  | 'jiraApp'
  | 'githubApp'
  | 'sentryApp'
  | 'claudeApp'
  | 'cursorApp'
  | 'editorApp'
  | 'canvasApp'
  | 'terminalApp'
  | 'rules'
  | 'library'
  | 'connections'
  | 'settings';

/** MCP `switch_view` ships platform-named views (`github` / `jira` /
 *  `sentry` / `claude` / `cursor` / `editor` / `canvas` / `terminal`)
 *  so a future GitLab/Bitbucket tab can claim its own slot. Translate
 *  to the internal `…App` view name. Returns `null` for unknown values
 *  so the handler can no-op cleanly. */
export function mapAgentViewToInternal(v: string): ViewName | null {
  switch (v) {
    case 'github':       return 'githubApp';
    case 'jira':         return 'jiraApp';
    case 'sentry':       return 'sentryApp';
    case 'claude':       return 'claudeApp';
    case 'cursor':       return 'cursorApp';
    case 'editor':       return 'editorApp';
    case 'canvas':       return 'canvasApp';
    case 'terminal':     return 'terminalApp';
    case 'rules':
    case 'connections':
    case 'settings':
      return v;
    default:
      return null;
  }
}

/** Coerce raw `sprint_ids` payload entries into the persisted
 *  `SprintScope[]` shape (numeric id or the literal `'backlog'`).
 *  The MCP tool's JSON schema accepts `string | number`; we accept
 *  either here too because cursor-agent and Claude have shipped
 *  both shapes in the wild. */
export function parseSprintScopes(raw: unknown[]): SprintScope[] {
  const out: SprintScope[] = [];
  for (const x of raw) {
    if (typeof x === 'number' && Number.isFinite(x) && x > 0) {
      out.push(x);
    } else if (typeof x === 'string') {
      if (x === 'backlog') {
        out.push('backlog');
      } else {
        const n = Number(x);
        if (Number.isFinite(n) && n > 0) out.push(n);
      }
    }
  }
  return out;
}
