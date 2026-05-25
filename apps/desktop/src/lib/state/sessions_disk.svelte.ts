// Disk-persistence layer for the session store. Extracted from
// sessions.svelte.ts in wave-1 phase-7 refactor. Owns: the on-disk
// directory pointer, the trailing-debounce write timer, the
// identity-diff "last written" snapshot map, and the
// `persistError.sessions` flag the Settings panel surfaces.
//
// The host (`sessions.svelte.ts`) keeps the reactive store ownership
// and the $effect that re-fires on every state change; we just expose
// `scheduleDiskWrite` / `flushNow` so the host can hand us the
// current `(sessions, activeId)` snapshot at flush time. This split
// removes ~140 LoC of disk plumbing from the god file and makes the
// debounce ↔ ceiling logic unit-testable in isolation.

import { invoke } from '@tauri-apps/api/core';
import type { ClaudeSession } from '$lib/types';
import { notify } from '$lib/state/toaster.svelte';
import { serializeSession } from './sessions_serialize';
import { flushStreamQueueNow } from './sessions_stream';

// ---- Internal state ----
let _diskDir: string | null = null; // e.g. "…/Woom/sessions"
let _diskWriteTimer: ReturnType<typeof setTimeout> | null = null;
/* Wall-clock of the last write that actually hit the disk. The
   debouncer uses this to enforce a hard ceiling on how long the
   trailing flush can be deferred during continuous streaming —
   without it, a rapid succession of `appendAssistantDelta` calls
   would keep resetting the 400ms timer indefinitely, and a force-
   quit mid-stream would lose every message since the last natural
   pause. With the ceiling, we flush at least every 2.5s even if
   changes keep arriving. */
let _diskLastFlushAt = 0;
const DISK_DEBOUNCE_MS = 800;
const DISK_MAX_DEFER_MS = 2_500;

/* Identity-based dirty tracking — `flushToDisk` was unconditionally
 * writing ALL session files on every burst (one Promise.all rebuild
 * of N session files per debounce window). For a 10-session workspace
 * with one active stream that's 9 wasted writes per flush. We instead
 * compare each session's object identity against the snapshot from
 * the last successful write. Every mutator does
 * `sessionsState.list = sessionsState.list.map(...)` which creates a
 * new reference for the changed session only — so the ref-equality
 * check catches exactly the dirty ones. `null` sentinel means "never
 * written" (first flush after init writes everything once). */
const _lastWrittenRef: Map<string, ClaudeSession> = new Map();
let _lastWrittenActiveId: string | null | undefined = undefined; // undefined = never written

let _sessionsToastFired = false;

/** Persistence-error state — read by the Settings panel to render
 *  the "Storage" badge / blocker banner. Sessions / rules use
 *  separate slots because they can fail independently (different
 *  files, different failure modes). `null` = healthy. */
import { persistError as _sharedPersistError } from './sessions_persist_error.svelte';
export const persistError = _sharedPersistError;

function asMessage(e: unknown): string {
  if (e instanceof Error) return e.message || e.name;
  if (typeof e === 'string') return e;
  return String(e);
}

function sessionIndexPath(): string { return `${_diskDir}/index.json`; }
function sessionFilePath(id: string): string { return `${_diskDir}/${id}.json`; }

/** Set the on-disk directory; called once from `initSessionsFromDisk`
 *  after the Rust side has resolved `<app_data>/sessions`. Until this
 *  is called, every flush is a no-op (pre-migration paths fall back
 *  to localStorage in the caller). */
export function setDiskDir(dir: string | null): void {
  _diskDir = dir;
}

export function getDiskDir(): string | null {
  return _diskDir;
}

/** Per-session-file path computed for `id`, exported so caller-side
 *  paths (hydrate, manual nuke, debug logging) don't have to
 *  duplicate the layout. Returns null when disk hasn't been
 *  initialised yet. */
export function sessionFilePathFor(id: string): string | null {
  return _diskDir ? sessionFilePath(id) : null;
}

export function sessionIndexFilePath(): string | null {
  return _diskDir ? sessionIndexPath() : null;
}

/** Reset the "last written" snapshot. Used by `initSessionsFromDisk`
 *  after it manually hydrates from disk so the first post-init write
 *  doesn't redundantly re-write every session. */
export function resetLastWrittenSnapshot(
  sessions: ClaudeSession[],
  activeId: string | null
): void {
  _lastWrittenRef.clear();
  for (const s of sessions) _lastWrittenRef.set(s.id, s);
  _lastWrittenActiveId = activeId;
  _diskLastFlushAt = Date.now();
}

async function flushToDisk(sessions: ClaudeSession[], activeId: string | null): Promise<void> {
  if (!_diskDir) return;
  try {
    /* Identity-diff against the last-written snapshot — write ONLY the
       sessions whose object reference changed since last flush. Every
       mutator allocates a fresh ClaudeSession for the touched id (via
       `list.map((s) => s.id === id ? {...s, ...} : s)`) so a !== check
       reliably tells us which ids carry new content. */
    const ids = sessions.map((s) => s.id);
    const dirty: ClaudeSession[] = [];
    const liveIds = new Set<string>(ids);
    for (const s of sessions) {
      if (_lastWrittenRef.get(s.id) !== s) dirty.push(s);
    }
    /* Index needs a rewrite when the list of ids changed (add / remove /
       reorder) OR when the active pointer moved. */
    let indexDirty = activeId !== _lastWrittenActiveId;
    if (!indexDirty) {
      if (_lastWrittenActiveId === undefined) {
        indexDirty = true;
      } else if (ids.length !== _lastWrittenRef.size) {
        indexDirty = true;
      } else {
        for (const id of _lastWrittenRef.keys()) {
          if (!liveIds.has(id)) { indexDirty = true; break; }
        }
      }
    }
    /* Parallel per-session writes — sequential `await invoke()` in a
       for-loop was the dominant cost during a force-quit's grace
       window. With Promise.all every dirty session goes out
       concurrently and we only await the slowest one before the
       index gets written. The index write stays sequential so we
       never persist an ids[] pointing at a file that hasn't landed
       yet. */
    if (dirty.length > 0) {
      await Promise.all(
        dirty.map((s) =>
          invoke('fs_write_file', {
            path: sessionFilePath(s.id),
            contents: JSON.stringify(serializeSession(s))
          })
        )
      );
      for (const s of dirty) _lastWrittenRef.set(s.id, s);
    }
    /* Prune removed ids from the snapshot map so a re-added id with the
       same value isn't silently skipped on next flush. */
    if (_lastWrittenRef.size !== liveIds.size) {
      for (const id of [..._lastWrittenRef.keys()]) {
        if (!liveIds.has(id)) _lastWrittenRef.delete(id);
      }
    }
    if (indexDirty) {
      await invoke('fs_write_file', {
        path: sessionIndexPath(),
        contents: JSON.stringify({ activeId, ids })
      });
      _lastWrittenActiveId = activeId;
    }
    _diskLastFlushAt = Date.now();
    if (persistError.sessions) persistError.sessions = null;
  } catch (e) {
    const msg = asMessage(e);
    persistError.sessions = msg;
    if (!_sessionsToastFired) {
      _sessionsToastFired = true;
      notify({
        kind: 'error',
        title: "Couldn't save chats",
        body: `${msg}. New messages stay in memory but won't survive a restart. See Settings → Storage.`,
        ttlMs: null
      });
    }
  }
}

/** Schedule a debounced disk flush. Two-phase: a 400ms trailing
 *  timer coalesces bursts (every streaming delta retriggers this);
 *  a 2.5s ceiling guarantees the timer can't be pushed past
 *  `DISK_MAX_DEFER_MS` since the last flush — so a continuous
 *  stream still hits disk regularly enough that a force-quit
 *  mid-stream loses at most ~2.5s of writes. */
export function scheduleDiskWrite(
  sessions: ClaudeSession[],
  activeId: string | null
): void {
  const now = Date.now();
  const sinceLastFlush = now - _diskLastFlushAt;
  if (sinceLastFlush >= DISK_MAX_DEFER_MS) {
    /* We've been deferring too long — flush right away on a microtask
       so we don't block the current call site. */
    if (_diskWriteTimer) {
      clearTimeout(_diskWriteTimer);
      _diskWriteTimer = null;
    }
    queueMicrotask(() => void flushToDisk(sessions, activeId));
    return;
  }
  if (_diskWriteTimer) clearTimeout(_diskWriteTimer);
  const remainingDefer = Math.max(0, DISK_MAX_DEFER_MS - sinceLastFlush);
  const wait = Math.min(DISK_DEBOUNCE_MS, remainingDefer);
  _diskWriteTimer = setTimeout(() => {
    _diskWriteTimer = null;
    void flushToDisk(sessions, activeId);
  }, wait);
}

/** Force an immediate, awaitable flush. Bypasses the debounce timer
 *  so callers (window-close hook, manual "Save now" affordance, …)
 *  can guarantee the on-disk copy reflects the in-memory state
 *  before the next browser tick / quit. Drains the rAF stream queue
 *  first so the trailing few tokens that hadn't landed in state yet
 *  are included. */
export async function flushNow(
  sessions: ClaudeSession[],
  activeId: string | null
): Promise<void> {
  flushStreamQueueNow();
  if (_diskWriteTimer) {
    clearTimeout(_diskWriteTimer);
    _diskWriteTimer = null;
  }
  if (!_diskDir) return;
  await flushToDisk(sessions, activeId);
}

/** Used by `initSessionsFromDisk` to write a fully-hydrated session
 *  bag back out without going through the dirty-diff path (which
 *  would treat every session as new and rewrite everything anyway —
 *  this is just the same operation with intent declared). */
export async function flushAllNow(
  sessions: ClaudeSession[],
  activeId: string | null
): Promise<void> {
  if (!_diskDir) return;
  await flushToDisk(sessions, activeId);
}
