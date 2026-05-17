/* CLAUDE.md auto-load — per-cwd cache for the Rust walker in
 * `claudemd.rs`. We pre-fetch on session cwd change so the sync
 * `buildAgentAppContext` builder can read content without awaiting.
 *
 * Refresh is idempotent per (cwd, content-hash) — `loadClaudeMd`
 * compares the new walk result against the cached one and only
 * mutates when it changes. Keeps reactive consumers from churning
 * on every turn. */

import { invoke } from '@tauri-apps/api/core';

export interface ClaudeMdResult {
  content: string;
  sources: string[];
  warnings: string[];
}

const cache = new Map<string, ClaudeMdResult>();
/** Single in-flight promise per cwd so concurrent callers share IO. */
const inflight = new Map<string, Promise<ClaudeMdResult>>();

/** Force a re-read regardless of cache. Used by an explicit
 *  "reload memory" command later — not wired to a UI yet. */
export async function loadClaudeMd(cwd: string | null, force = false): Promise<ClaudeMdResult> {
  const key = cwd ?? '';
  if (!force) {
    const hit = cache.get(key);
    if (hit) return hit;
    const pending = inflight.get(key);
    if (pending) return pending;
  }
  const p = (async () => {
    try {
      const r = await invoke<ClaudeMdResult>('claudemd_load', { cwd: cwd ?? null });
      cache.set(key, r);
      return r;
    } catch (e) {
      console.warn('claudemd_load failed', e);
      const empty: ClaudeMdResult = { content: '', sources: [], warnings: [String(e)] };
      cache.set(key, empty);
      return empty;
    } finally {
      inflight.delete(key);
    }
  })();
  inflight.set(key, p);
  return p;
}

/** Sync accessor — caller must have already awaited `loadClaudeMd`
 *  for this cwd at least once. Returns an empty result on cache miss
 *  rather than throwing — the agent context builder is sync and we
 *  want graceful degradation if the prefetch hasn't run yet. */
export function getCachedClaudeMd(cwd: string | null): ClaudeMdResult {
  return cache.get(cwd ?? '') ?? { content: '', sources: [], warnings: [] };
}

/** Drop the cache entry for a cwd — used when the user explicitly
 *  edits a CLAUDE.md so the next turn re-reads. Not auto-wired
 *  to file-watchers yet (would need an `notify::Watcher` in Rust
 *  scoped to the discovered paths). */
export function invalidateClaudeMd(cwd: string | null): void {
  cache.delete(cwd ?? '');
}
