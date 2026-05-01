/* Token rotation reminders.
 *
 * We can't read upstream expiry — most PATs let the user pick how long
 * the token lives, and even when the provider hard-caps it the field
 * isn't surfaced through their `who am I` endpoint. Forgehold's
 * approximation: stamp the moment the token first hit our keychain
 * (see `markTokenInstalled` callers in `+page.svelte`) and nag on age.
 *
 * Thresholds match `docs/ROADMAP_1.0.md §1.2 / §2.7.7`:
 *   180 d → warn (soft nudge)
 *   300 d → strong warn (clearer copy)
 *   365 d → hard-error (block-style banner; we still let the user
 *           keep using it — most providers don't auto-expire to the
 *           day, and we'd rather annoy than lock anyone out).
 *
 * Stored in localStorage under `forgehold:token-installed-at:v1`. Lives
 * in localStorage rather than the keychain because it's metadata, not
 * a secret, and we want it readable on boot before biometric unlock.
 *
 * We only track sources Forgehold owns the credential for — github,
 * jira, sentry. Agents (claude, cursor) auth to their own services so
 * the rotation conversation belongs to them, not us. */

import { loadVersioned, saveVersioned } from './persist';

const STORAGE_KEY = 'forgehold:token-installed-at:v1';
const STORAGE_VERSION = 1;

export type TokenSource = 'github' | 'jira' | 'sentry';

export type TokenAgeSeverity = 'fresh' | 'warn' | 'strong-warn' | 'expired';

export interface TokenAgeInfo {
  /** ISO timestamp the credential was first stored. */
  installedAt: string;
  /** Whole days since `installedAt`, floored. */
  days: number;
  /** Bucket the age falls into; drives UI tone. */
  severity: TokenAgeSeverity;
}

const DAY_MS = 24 * 60 * 60 * 1000;

const initial = loadInstalledAt();

export const tokenAgeState = $state<{
  /** Map of source → ISO timestamp the credential first hit Keychain.
   *  Null when there's no record (legacy users; or never connected). */
  installedAt: Record<TokenSource, string | null>;
}>({
  installedAt: {
    github: initial.github ?? null,
    jira: initial.jira ?? null,
    sentry: initial.sentry ?? null
  }
});

/** Stamp a fresh install timestamp for `source`. Idempotent on the
 *  same calendar day so repeated connects don't push the date forward
 *  every time the user re-enters the same token; the goal is to nag
 *  on token *age*, not on connect frequency. */
export function markTokenInstalled(source: TokenSource): void {
  const existing = tokenAgeState.installedAt[source];
  if (existing && sameDay(existing, new Date().toISOString())) return;
  tokenAgeState.installedAt[source] = new Date().toISOString();
  persist();
}

/** Forget the install timestamp on disconnect. The next connect will
 *  re-stamp from scratch. */
export function clearTokenInstalled(source: TokenSource): void {
  if (!tokenAgeState.installedAt[source]) return;
  tokenAgeState.installedAt[source] = null;
  persist();
}

/** Age + severity for a source, or `null` if we have no record (which
 *  is the case for legacy users — we deliberately don't backfill
 *  "today" because that would lie about a token that's actually a
 *  year old). */
export function tokenAgeInfo(
  source: TokenSource,
  now: number = Date.now()
): TokenAgeInfo | null {
  const installedAt = tokenAgeState.installedAt[source];
  if (!installedAt) return null;
  const ts = Date.parse(installedAt);
  if (Number.isNaN(ts)) return null;
  const days = Math.max(0, Math.floor((now - ts) / DAY_MS));
  return { installedAt, days, severity: severityForDays(days) };
}

function severityForDays(days: number): TokenAgeSeverity {
  if (days >= 365) return 'expired';
  if (days >= 300) return 'strong-warn';
  if (days >= 180) return 'warn';
  return 'fresh';
}

function sameDay(aIso: string, bIso: string): boolean {
  const a = new Date(aIso);
  const b = new Date(bIso);
  return (
    a.getFullYear() === b.getFullYear() &&
    a.getMonth() === b.getMonth() &&
    a.getDate() === b.getDate()
  );
}

function persist(): void {
  saveVersioned(STORAGE_KEY, STORAGE_VERSION, {
    github: tokenAgeState.installedAt.github,
    jira: tokenAgeState.installedAt.jira,
    sentry: tokenAgeState.installedAt.sentry
  });
}

function loadInstalledAt(): Partial<Record<TokenSource, string>> {
  /* `adapter-static` evaluates +page modules during prerender where
   * `localStorage` doesn't exist; the live app always runs in the
   * Tauri WKWebView where it does. Mirror the SSR guard from
   * `connectionEvents.svelte.ts` so module init doesn't throw at
   * build time. */
  if (typeof localStorage === 'undefined') return {};
  const loaded = loadVersioned<Partial<Record<TokenSource, string>>>(
    STORAGE_KEY,
    { version: STORAGE_VERSION }
  );
  if (!loaded || typeof loaded !== 'object') return {};
  const out: Partial<Record<TokenSource, string>> = {};
  for (const k of ['github', 'jira', 'sentry'] as const) {
    const v = loaded[k];
    if (typeof v === 'string' && !Number.isNaN(Date.parse(v))) {
      out[k] = v;
    }
  }
  return out;
}
