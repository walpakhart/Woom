/** Shared revert / keep handlers for diff cards.
 *
 *  Both the per-card buttons in `EditDiffCard.svelte` and the
 *  bulk-action bar above the composer need to dispatch the same Tauri
 *  commands — `revert_edit`, `revert_write`, or `restore_deleted_file`,
 *  picked by the event's `isDelete` / `wholeFile` flags. Keeping the
 *  branching here means the card stays a thin view layer and the bar
 *  can't drift out of sync (e.g. miss a delete-restore branch).
 */

import { invoke } from '@tauri-apps/api/core';
import type { MessageEvent } from '$lib/types';
import {
  updateEditEvent,
  getPendingEditEvents
} from '$lib/state/sessions.svelte';

type EditEvent = Extract<MessageEvent, { kind: 'edit' }>;

/** Revert one edit event. Wraps the same three-way Tauri dispatch the
 *  card already uses, plus the state update so the card flips visually.
 *  Returns a result object instead of throwing so the bulk caller can
 *  tally success/failure across many events without aborting on the
 *  first one. */
export async function revertEditEvent(
  sessionId: string,
  ev: EditEvent
): Promise<{ ok: true } | { ok: false; error: string }> {
  try {
    if (ev.isDelete) {
      // The agent deleted the file → "Revert" recreates it from the
      // captured prevContent (cursor's `prevContent` or git HEAD
      // fallback). Rust refuses if a file already exists at the path
      // so we don't clobber manual recreation.
      await invoke('restore_deleted_file', {
        filePath: ev.filePath,
        prevContent: ev.oldText
      });
    } else if (ev.wholeFile) {
      // Write: rewrite the full file, or delete it if the agent created
      // it from scratch. Rust validates current contents match newText
      // so a stale Revert click can't trample post-Write edits, and
      // refuses to delete a file that's tracked in git even if FE says
      // isCreate=true (defensive, see lib.rs::revert_write).
      await invoke('revert_write', {
        filePath: ev.filePath,
        oldText: ev.oldText,
        newText: ev.newText,
        isCreate: ev.isCreate
      });
    } else {
      // Edit / MultiEdit: search-and-replace newText back to oldText.
      // Rust requires a unique match so we can't blindly insert into
      // positions the agent never touched.
      await invoke('revert_edit', {
        filePath: ev.filePath,
        oldText: ev.oldText,
        newText: ev.newText
      });
    }
    updateEditEvent(sessionId, ev.toolId, {
      status: 'reverted',
      note: undefined
    });
    return { ok: true };
  } catch (e: unknown) {
    const msg = e instanceof Error ? e.message : String(e);
    updateEditEvent(sessionId, ev.toolId, { status: 'error', note: msg });
    return { ok: false, error: msg };
  }
}

/** Revert every pending edit in the session, newest-first.
 *
 *  Why newest-first: revert_edit / revert_write both validate that the
 *  on-disk content currently equals `newText` (the agent's write). If
 *  Edit B sits on top of Edit A on the same file, the disk holds B's
 *  contents — only B's revert can match. Once B is undone the disk
 *  holds A's `newText`, and A's revert can run. Reversing chat order
 *  before iterating is the cheapest way to guarantee that ordering
 *  without a per-file dependency graph.
 *
 *  Failures don't abort: each event reports independently via
 *  `updateEditEvent` (status → 'error'), and the totals returned here
 *  feed a single toast so the user gets one summary instead of N
 *  separate errors. */
export async function revertAllPendingEdits(
  sessionId: string
): Promise<{ reverted: number; failed: number; total: number }> {
  const pending = getPendingEditEvents(sessionId);
  const ordered = pending.slice().reverse();
  let reverted = 0;
  let failed = 0;
  for (const ev of ordered) {
    const r = await revertEditEvent(sessionId, ev);
    if (r.ok) reverted++;
    else failed++;
  }
  return { reverted, failed, total: pending.length };
}

/** Mark every currently-pending edit as `kept`. Disk is untouched —
 *  the file already has the agent's `newText`; we just record the
 *  user's "I'm OK with this" decision so the bulk bar's count drops
 *  and the per-card buttons swap to a single "Unkeep" affordance.
 *
 *  Mirror of `revertAllPendingEdits` (which transitions cards to
 *  `reverted` and gives them "Reapply"). Together the two states give
 *  the user a clear, undoable verdict on every change. */
export function keepAllPendingEdits(sessionId: string): number {
  const pending = getPendingEditEvents(sessionId);
  for (const ev of pending) {
    updateEditEvent(sessionId, ev.toolId, { status: 'kept', note: undefined });
  }
  return pending.length;
}
