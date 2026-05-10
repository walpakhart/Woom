/* Per-file cursor / selection / scroll persistence
 * (`docs/ROADMAP_1.0.md §2.1.1`).
 *
 * Re-opening a tab should land on the same line + column + scroll
 * the user left, with any active selection restored. Stored in
 * localStorage under `woom:editor:cursors:v1` keyed by the
 * absolute file path; capped at the most-recent N files so a long
 * day of file hopping doesn't pad the origin quota.
 *
 * The values are CodeMirror document offsets (`from`, `to`) plus a
 * pixel `scrollTop`. We don't try to reconstruct the cursor across
 * external file edits — if the file has shifted significantly the
 * caret may land somewhere unexpected. The user-visible damage is
 * minor; clamping at the doc length is enough to avoid a runtime
 * error when a file shrank between sessions.
 */

const STORAGE_KEY = 'woom:editor:cursors:v1';
const MAX_ENTRIES = 200;

export interface EditorCursorRecord {
  /** Document offset of the selection anchor / left edge. */
  from: number;
  /** Document offset of the selection head / right edge. Equal to
   *  `from` for a caret-only state. */
  to: number;
  /** Pixel scrollTop of the editor. Restored after the selection so
   *  the caret stays visible even if the user had scrolled below it. */
  scrollTop: number;
  /** ISO timestamp the record was last written, used for capacity
   *  trimming. */
  at: string;
}

const initial = loadAll();

export const editorCursorsState = $state<{
  byPath: Record<string, EditorCursorRecord>;
}>({
  byPath: initial
});

/** Stash a position for `path`. Idempotent overwrite; the timestamp
 *  doubles as the LRU age for capacity trimming. */
export function recordCursor(path: string, rec: Omit<EditorCursorRecord, 'at'>): void {
  if (!path) return;
  editorCursorsState.byPath[path] = {
    from: Math.max(0, Math.floor(rec.from)),
    to: Math.max(0, Math.floor(rec.to)),
    scrollTop: Math.max(0, Math.floor(rec.scrollTop)),
    at: new Date().toISOString()
  };
  scheduleFlush();
}

/** Read a previously-stored position, or null. */
export function readCursor(path: string): EditorCursorRecord | null {
  return editorCursorsState.byPath[path] ?? null;
}

/** Drop a single record (called on file delete / rename). */
export function clearCursor(path: string): void {
  if (!editorCursorsState.byPath[path]) return;
  delete editorCursorsState.byPath[path];
  scheduleFlush();
}

let flushTimer: ReturnType<typeof setTimeout> | null = null;

/** Debounce the localStorage write so rapid selection-change events
 *  during a drag don't hammer the origin storage. 300 ms is the same
 *  cadence the editor's dirty-flag uses. */
function scheduleFlush(): void {
  if (typeof localStorage === 'undefined') return;
  if (flushTimer) clearTimeout(flushTimer);
  flushTimer = setTimeout(flushNow, 300);
}

function flushNow(): void {
  flushTimer = null;
  if (typeof localStorage === 'undefined') return;
  /* Trim to MAX_ENTRIES, oldest-first by `at`. The Object → Array →
   * sorted slice round-trip is O(N log N) but N is bounded by the
   * cap; even a worst-case 200 entries is sub-millisecond. */
  const entries = Object.entries(editorCursorsState.byPath);
  if (entries.length > MAX_ENTRIES) {
    entries.sort(([, a], [, b]) => Date.parse(b.at) - Date.parse(a.at));
    const trimmed = Object.fromEntries(entries.slice(0, MAX_ENTRIES));
    editorCursorsState.byPath = trimmed;
  }
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(editorCursorsState.byPath));
  } catch {
    /* SSR / quota — non-critical, loss only of cross-launch state */
  }
}

function loadAll(): Record<string, EditorCursorRecord> {
  if (typeof localStorage === 'undefined') return {};
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return {};
    const parsed = JSON.parse(raw);
    if (!parsed || typeof parsed !== 'object') return {};
    const out: Record<string, EditorCursorRecord> = {};
    for (const [k, v] of Object.entries(parsed)) {
      if (!v || typeof v !== 'object') continue;
      const r = v as Partial<EditorCursorRecord>;
      if (typeof r.from !== 'number' || typeof r.to !== 'number') continue;
      out[k] = {
        from: r.from,
        to: r.to,
        scrollTop: typeof r.scrollTop === 'number' ? r.scrollTop : 0,
        at: typeof r.at === 'string' ? r.at : new Date(0).toISOString()
      };
    }
    return out;
  } catch {
    return {};
  }
}
