// Shared drag-and-drop state. Single source of truth for "is something
// being dragged right now, and what is it" — read by drop targets to decide
// whether to accept the drop, by the workbench-bar pills to auto-open their
// hover menus mid-drag, and by drop handlers to recover the payload when
// WKWebView hides custom-mime data on `dataTransfer.types` during dragover.
//
// WebKit's drag-over event exposes only a curated whitelist of mime types
// (text/plain, text/html, text/uri-list, Files) — custom application/x-*
// mimes set via `setData()` aren't visible until the drop event. Tracking
// the payload in module state side-steps that and keeps highlight + drop
// logic working from `dragstart` onward.

import type { InboxItem, JiraItem, SentryIssue } from '$lib/data';

export type DragPayload =
  | { source: 'github'; item: InboxItem }
  | { source: 'jira'; item: JiraItem }
  | { source: 'sentry'; item: SentryIssue }
  | { source: 'file'; path: string; isDir: boolean; name: string }
  /** A specific message within a Claude / Cursor session, referenced
   *  by `(sessionId, messageIndex)`. The drop target captures a small
   *  snapshot (role, first ~200 chars of content, agent kind) so the
   *  card stays meaningful even if the source session is later
   *  deleted. The renderer prefers live data when the session still
   *  exists. Used by Canvas's `chat-message-card` shape kind. */
  | {
      source: 'chat-message';
      sessionId: string;
      messageIndex: number;
      /** Snapshot — used by the Canvas card if the live message is
       *  ever unreachable. Kept tiny on purpose. */
      snapshot: {
        role: 'user' | 'assistant' | 'system';
        agentKind: 'claude' | 'cursor';
        sessionTitle: string;
        excerpt: string;
        at: string;
      };
    };

export const dragState = $state<{ payload: DragPayload | null }>({ payload: null });

export function setDragPayload(p: DragPayload | null) {
  dragState.payload = p;
}

/** Install a window-level safety net that clears the drag payload on
 *  any dragend or drop, regardless of whether the originating element
 *  remembered to wire its own `ondragend`. Without this, a stray drag
 *  cancellation (Esc, drop on an inert region, drag of a future
 *  component that forgets the local handler) leaves `dragState.payload`
 *  populated and drop targets continue to report `dragOver` for the
 *  next drag. Idempotent — calling more than once is a no-op via the
 *  `installed` guard. Called once from `+page.svelte`'s onMount.
 *
 *  Critical: listeners attach in BUBBLE phase (default), not capture.
 *  A capture-phase `drop` listener at the window fires BEFORE the
 *  drop-target's own handler, which means by the time the target
 *  reads `dragState.payload` to process the drop it's already null
 *  — drag of a Jira/GitHub/Sentry item into a Claude/Cursor column
 *  silently dropped because the target couldn't see what was being
 *  dropped. Bubble-phase fires AFTER all per-element handlers have
 *  had their turn, which is the safe time to clear. */
let installed = false;
export function installGlobalDragSafetyNet() {
  if (installed || typeof window === 'undefined') return;
  installed = true;
  // `dragend` fires on the source element after a drop OR after a
  // cancellation (Esc / drop on an invalid target). `drop` fires on
  // the destination if accepted. We DO NOT clear on `drop` here —
  // a subset of targets stop propagation on accept, so a
  // bubble-phase drop listener would never fire for those. Instead
  // we rely on `dragend`, which the browser fires on the source for
  // EVERY drag termination (success or cancel, including Esc). It
  // also bubbles to window even when the drop target stopped
  // propagation on its own drop event.
  window.addEventListener('dragend', () => setDragPayload(null));
}
