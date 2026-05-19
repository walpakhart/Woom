/* Bug-report bundle builder.
 *
 * Produces a Markdown report the user can copy or attach to a GitHub
 * issue. Sources are deliberately local-only: no IPC out, no upload,
 * no PII beyond what the user already typed in their description.
 *
 * What we include:
 *   - App version + build target.
 *   - Per-source connection state (kind only; never the token).
 *   - Last N entries from `connectionEventsState` — the closest thing
 *     we have to "last 200 log lines" without wiring a live log
 *     stream.
 *   - Layout snapshot: active solo, column kinds + counts.
 *   - User description (markdown-escaped).
 *
 * Out of scope (deferred to 1.x): live terminal output bundling, full
 * session transcript export. Both surface elsewhere in Settings.
 */

import { connectionEventsState, type ConnectionEvent } from '$lib/state/connectionEvents.svelte';
import { connectionsState } from '$lib/state/connections.svelte';
import { layoutState } from '$lib/state/layout.svelte';

const MAX_EVENTS_IN_REPORT = 50;

export interface BugReportInput {
  description: string;
  /** Caller-supplied app version string. Frontend doesn't have a
   *  reliable way to read `tauri.conf.json > version` at runtime
   *  without an extra IPC, so we accept it from the caller (which
   *  already shows it as `Woom 0.1.0` in Settings). */
  appVersion: string;
}

/** Render the full report as Markdown. */
export function buildBugReport(input: BugReportInput): string {
  const now = new Date().toISOString();
  const sections: string[] = [];

  sections.push('## Description', input.description.trim() || '_(no description)_');

  sections.push('## Environment');
  sections.push(`- Date: ${now}`);
  sections.push(`- App: ${input.appVersion}`);
  sections.push(`- Platform: macOS (${navigatorSummary()})`);

  sections.push('## Connection state', formatConnectionsState());

  const events = connectionEventsState.events.slice(0, MAX_EVENTS_IN_REPORT);
  if (events.length > 0) {
    sections.push(`## Connection events (last ${events.length})`);
    sections.push(formatEvents(events));
  }

  sections.push('## Layout snapshot', formatLayout());

  return sections.join('\n\n') + '\n';
}

/** Build a `mailto:` URL with the report pre-filled. Apps without a
 *  configured target email get a `null` so the UI can fall back to
 *  copy-to-clipboard. */
export function bugReportMailto(report: string, to: string | null): string | null {
  if (!to) return null;
  const subject = encodeURIComponent('Woom bug report');
  const body = encodeURIComponent(report);
  return `mailto:${to}?subject=${subject}&body=${body}`;
}

/** Build a GitHub `new issue` URL with the report pre-filled in the
 *  body. `owner/repo` configurable; `null` when not set. */
export function bugReportGithubIssueUrl(
  report: string,
  ownerRepo: string | null
): string | null {
  if (!ownerRepo || !/^[\w.-]+\/[\w.-]+$/.test(ownerRepo)) return null;
  const title = encodeURIComponent('Bug report');
  const body = encodeURIComponent(report);
  return `https://github.com/${ownerRepo}/issues/new?title=${title}&body=${body}`;
}

function navigatorSummary(): string {
  if (typeof navigator === 'undefined') return 'unknown';
  /* User-Agent strings are noisy but contain the WKWebView build the
   * Tauri webview runs. Truncate hard so the report stays readable. */
  const ua = navigator.userAgent ?? 'unknown';
  return ua.length > 200 ? ua.slice(0, 200) + '…' : ua;
}

function formatConnectionsState(): string {
  const c = connectionsState;
  const lines: string[] = [];
  lines.push(`- GitHub: ${c.github.kind === 'connected' ? `connected as @${c.github.user.login}` : c.github.kind}`);
  lines.push(`- Jira: ${c.jira.kind === 'connected' ? `connected as ${c.jira.user.display_name} (${c.jira.user.workspace})` : c.jira.kind}`);
  lines.push(`- Sentry: ${c.sentry.kind === 'connected' ? `connected (${c.sentry.user.organization_slug} on ${c.sentry.user.host})` : c.sentry.kind}`);
  lines.push(`- Claude: ${c.claude?.ready ? `ready (v${c.claude.version ?? '?'})` : 'not detected'}`);
  lines.push(`- Cursor: ${c.cursor?.ready ? `ready (v${c.cursor.version ?? '?'})` : 'not detected'}`);
  return lines.join('\n');
}

function formatEvents(events: ConnectionEvent[]): string {
  /* Code-fence the table — keeps formatting reliable when pasted into
   * GitHub issue bodies, where indentation rules can otherwise eat
   * pipes inside non-fenced lists. */
  const rows = events.map((e) => {
    const lat = e.latencyMs == null ? '–' : `${e.latencyMs}ms`;
    const msg = e.message ? ` ${e.message.slice(0, 120)}` : '';
    return `${e.at}  ${pad(e.source, 7)}  ${pad(e.kind, 14)}  ${pad(lat, 7)}${msg}`;
  });
  return '```\n' + rows.join('\n') + '\n```';
}

function pad(s: string, width: number): string {
  return s.length >= width ? s : s + ' '.repeat(width - s.length);
}

function formatLayout(): string {
  const lines: string[] = [];
  if (layoutState.active.editor.repoPath) {
    lines.push(`- editor.repo_path = \`${layoutState.active.editor.repoPath}\``);
  }
  if (layoutState.active.canvas.canvasId) {
    lines.push(`- canvas.active = \`${layoutState.active.canvas.canvasId}\``);
  }
  if (layoutState.active.terminal.cwd) {
    lines.push(`- terminal.cwd = \`${layoutState.active.terminal.cwd}\``);
  }
  const linkSummary: string[] = [];
  for (const [repo, sessions] of Object.entries(layoutState.links.editorToAgent)) {
    if (sessions.length) linkSummary.push(`editor(${repo})↔${sessions.length} agent(s)`);
  }
  for (const [canvasId, sessions] of Object.entries(layoutState.links.canvasToAgent)) {
    if (sessions.length) linkSummary.push(`canvas(${canvasId.slice(0, 6)})↔${sessions.length} agent(s)`);
  }
  if (linkSummary.length) lines.push(`- links: ${linkSummary.join(', ')}`);
  return lines.length === 0 ? '_(no active solo state)_' : lines.join('\n');
}
