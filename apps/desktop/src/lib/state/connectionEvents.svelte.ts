/* Connection event log.
 *
 * Append-only ring buffer recording every outcome of a `*_status()`
 * round-trip per source. Powers two surfaces:
 *
 *  - Test-connection button in `ConnectionsView` — clicking it kicks off
 *    a refresh, the result lands here, and the per-card UI reads
 *    `lastEventForSource(id)` to render success/failure + latency.
 *  - Connection diagnostics card in `SettingsView` — full chronological
 *    log, useful for "why did my Jira go dark at 3 AM" debugging.
 *
 * Stored in localStorage under `forgehold:connection-events:v1` capped
 * at 200 events; oldest are dropped on overflow. Persistence failures
 * are swallowed (privacy-mode tabs still get the in-session log).
 */

import { loadVersioned, saveVersioned } from './persist';

const STORAGE_KEY = 'forgehold:connection-events:v1';
const STORAGE_VERSION = 1;
const MAX_EVENTS = 200;

export type ConnectionEventSource =
  | 'github'
  | 'jira'
  | 'sentry'
  | 'claude'
  | 'cursor';

export type ConnectionEventKind =
  | 'connected' /* refresh succeeded — source is live */
  | 'disconnected' /* refresh deliberately removed creds (e.g. invalid token) */
  | 'error' /* generic failure: network blip, 5xx, etc. */
  | 'rate_limited'; /* 429 from upstream */

export interface ConnectionEvent {
  id: string;
  source: ConnectionEventSource;
  kind: ConnectionEventKind;
  /** ISO 8601, UTC. */
  at: string;
  /** Round-trip duration, ms. `null` when the call short-circuited
   *  before issuing the network request (e.g. no token in Keychain). */
  latencyMs: number | null;
  /** Short user-facing detail. Multi-line strings are flattened. */
  message?: string;
}

const initial = loadEvents();

export const connectionEventsState = $state<{
  events: ConnectionEvent[];
}>({
  events: initial
});

/** Record an event. Trims to `MAX_EVENTS`, persists best-effort. */
export function recordConnectionEvent(
  source: ConnectionEventSource,
  kind: ConnectionEventKind,
  detail: { latencyMs?: number | null; message?: string } = {}
): ConnectionEvent {
  const ev: ConnectionEvent = {
    id: cryptoRandomId(),
    source,
    kind,
    at: new Date().toISOString(),
    latencyMs: detail.latencyMs ?? null,
    message: detail.message?.replace(/\s+/g, ' ').trim() || undefined
  };
  connectionEventsState.events = [ev, ...connectionEventsState.events].slice(
    0,
    MAX_EVENTS
  );
  saveVersioned(STORAGE_KEY, STORAGE_VERSION, connectionEventsState.events);
  return ev;
}

/** Last event recorded for a source, or `null`. Lookups are O(N) but
 *  N ≤ 200; keeps the data structure trivial. */
export function lastEventForSource(
  source: ConnectionEventSource
): ConnectionEvent | null {
  for (const ev of connectionEventsState.events) {
    if (ev.source === source) return ev;
  }
  return null;
}

export function clearConnectionEvents(): void {
  connectionEventsState.events = [];
  saveVersioned(STORAGE_KEY, STORAGE_VERSION, []);
}

function loadEvents(): ConnectionEvent[] {
  if (typeof localStorage === 'undefined') return [];
  const loaded = loadVersioned<ConnectionEvent[]>(STORAGE_KEY, {
    version: STORAGE_VERSION
  });
  if (!Array.isArray(loaded)) return [];
  return loaded.filter(isValidEvent).slice(0, MAX_EVENTS);
}

function isValidEvent(x: unknown): x is ConnectionEvent {
  if (!x || typeof x !== 'object') return false;
  const e = x as Partial<ConnectionEvent>;
  return (
    typeof e.id === 'string' &&
    typeof e.at === 'string' &&
    typeof e.source === 'string' &&
    typeof e.kind === 'string'
  );
}

/** `crypto.randomUUID` was added to Safari in 15.4; we run inside Tauri
 *  on macOS which is Big Sur+, so the API is always available. The
 *  manual fallback exists only to satisfy SSR / tests. */
function cryptoRandomId(): string {
  if (typeof crypto !== 'undefined' && 'randomUUID' in crypto) {
    return crypto.randomUUID();
  }
  return `evt-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`;
}
