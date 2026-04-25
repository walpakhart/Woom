// Versioned localStorage helper. Wraps payloads with `{ v, data }` so future
// schema changes have a clean path: define a migration function for each
// gap version → version, and `loadVersioned` walks them in order.
//
// Existing stores (layout, sessions, inbox) wrote raw JSON before this
// helper existed. They keep working — `loadVersioned` falls back to
// `legacyParse` on payloads without the wrapper.

interface VersionedEnvelope<T> {
  v: number;
  data: T;
}

export type Migrator<T> = (data: unknown) => T;

interface LoadOpts<T> {
  /** Current schema version. */
  version: number;
  /** version → migrator. Each migrator takes the previous version's payload
   *  and returns the next version's. The chain runs in order from the stored
   *  version up to `version`. */
  migrations?: Record<number, Migrator<unknown>>;
  /** Called when the stored payload has no `{v,data}` wrapper. Should validate
   *  the legacy shape and return a v1-equivalent object. Return `null` to
   *  signal "unrecognized; treat as missing". */
  legacyParse?: (raw: unknown) => T | null;
}

/** Load + migrate a value. Returns `null` when nothing was stored or all
 *  recovery paths failed (caller falls back to its built-in default). */
export function loadVersioned<T>(key: string, opts: LoadOpts<T>): T | null {
  const raw = localStorage.getItem(key);
  if (raw === null) return null;
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return null;
  }
  // New format: `{ v, data }` wrapper.
  if (parsed && typeof parsed === 'object' && 'v' in parsed && 'data' in parsed) {
    const env = parsed as VersionedEnvelope<unknown>;
    let cur = env.data;
    let curV = typeof env.v === 'number' ? env.v : 1;
    if (curV === opts.version) return cur as T;
    if (opts.migrations) {
      while (curV < opts.version) {
        const next = curV + 1;
        const migrate = opts.migrations[next];
        if (!migrate) return null;
        try {
          cur = migrate(cur);
          curV = next;
        } catch {
          return null;
        }
      }
      return cur as T;
    }
    return null;
  }
  // Legacy: bare payload, no envelope. Hand to caller to unwrap.
  if (opts.legacyParse) {
    try {
      return opts.legacyParse(parsed);
    } catch {
      return null;
    }
  }
  return null;
}

/** Persist a value with the version envelope. Throws are swallowed (quota
 *  exceeded / storage disabled in incognito etc.) — callers don't need to
 *  worry about persistence breaking the live UI. */
export function saveVersioned<T>(key: string, version: number, data: T): void {
  try {
    const env: VersionedEnvelope<T> = { v: version, data };
    localStorage.setItem(key, JSON.stringify(env));
  } catch {
    /* quota / disabled storage: ignore */
  }
}
