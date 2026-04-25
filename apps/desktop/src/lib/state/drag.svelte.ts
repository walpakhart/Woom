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
  | { source: 'file'; path: string; isDir: boolean; name: string };

export const dragState = $state<{ payload: DragPayload | null }>({ payload: null });

export function setDragPayload(p: DragPayload | null) {
  dragState.payload = p;
}
