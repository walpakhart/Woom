// Ledger workflow state — the sequential machine-checked sibling of DW.
// One reactive singleton; backend `ledger:*` events feed in via
// listeners wired in `+page.svelte`, cards read straight from here.

import { invoke } from '@tauri-apps/api/core';

export interface LedgerItem {
  id: string;
  title: string;
  detail?: string | null;
  checkCmd?: string | null;
  status: 'queued' | 'working' | 'checking' | 'passed' | 'failed' | 'skipped';
  attempts: number;
  maxAttempts: number;
  diff?: string | null;
  commitSha?: string | null;
  checkOutput?: string | null;
  error?: string | null;
  tokensIn: number;
  tokensOut: number;
  costUsd: number;
  notes?: string | null;
  parallel: boolean;
  /** Ids of earlier items that must pass before this one is eligible.
   *  Empty = gated only by list order (classic linear ledger). */
  deps: string[];
  /** Parent item id for sub-items. An item that is some other item's
   *  parent is a CONTAINER (grouping header) — not executed; its status
   *  rolls up from its children. Null/absent = top-level item. */
  parentId?: string | null;
  feed: string[];
}

export interface LedgerWorkflow {
  id: string;
  sessionId: string;
  task: string;
  status:
    | 'building'
    | 'awaiting_launch'
    | 'running'
    | 'paused_quota'
    | 'paused'
    | 'paused_budget'
    | 'awaiting_review'
    | 'done'
    | 'failed'
    | 'cancelled';
  items: LedgerItem[];
  currentItem?: string | null;
  worktreePath?: string | null;
  branch?: string | null;
  baseSha?: string | null;
  fullDiff?: string | null;
  applied: boolean;
  /** Apply as one squashed commit (default) vs preserving per-item
   *  commits + a merge commit. */
  squash: boolean;
  /** Steering notes queued mid-run, drained into the next worker turn. */
  injections: string[];
  model: string;
  totalCostUsd: number;
  /** Spend ceiling; run pauses (`paused_budget`) when crossed, raised on resume. */
  budgetCapUsd: number;
  createdAt: number;
  startedAt?: number | null;
  completedAt?: number | null;
  parentCwd?: string | null;
}

export const ledgerState = $state<{ workflows: LedgerWorkflow[] }>({
  workflows: []
});

export function addLedger(w: LedgerWorkflow): void {
  ledgerState.workflows = [w, ...ledgerState.workflows];
}

export function getLedger(id: string): LedgerWorkflow | null {
  return ledgerState.workflows.find((w) => w.id === id) ?? null;
}

export function upsertLedger(w: LedgerWorkflow): void {
  const list = ledgerState.workflows;
  for (let i = 0; i < list.length; i++) {
    if (list[i].id === w.id) {
      list[i] = w;
      return;
    }
  }
  ledgerState.workflows = [w, ...list];
}

const ACTIVE_LEDGER_STATUSES = new Set([
  'building',
  'awaiting_launch',
  'running',
  'paused_quota',
  'paused',
  'paused_budget',
  'awaiting_review'
]);

/** The session's currently-active ledger, if any (newest first). */
export function activeLedgerForSession(sessionId: string): LedgerWorkflow | null {
  return (
    ledgerState.workflows.find(
      (w) => w.sessionId === sessionId && ACTIVE_LEDGER_STATUSES.has(w.status)
    ) ?? null
  );
}

export function isLedgerActive(id: string): boolean {
  const w = ledgerState.workflows.find((x) => x.id === id);
  return w ? ACTIVE_LEDGER_STATUSES.has(w.status) : false;
}

/** Toggle squash-on-apply. Optimistically flips local state; the
 *  backend echoes a `ledger:updated` that reconciles it. */
export async function setLedgerSquash(id: string, squash: boolean): Promise<void> {
  const w = getLedger(id);
  if (w) w.squash = squash;
  try {
    await invoke('ledger_set_squash', { workflowId: id, squash });
  } catch (e) {
    console.warn('ledger_set_squash failed', e);
    if (w) w.squash = !squash; // revert on failure
  }
}

/** Queue a mid-run steering note. Backend rejects unless running. */
export async function injectLedgerNote(id: string, note: string): Promise<void> {
  await invoke('ledger_inject', { workflowId: id, note });
}

/** Hydrate from disk on app boot. `ledger_list` returns full workflows
 *  (checklists are small — no shell/lazy split needed like DW). */
export async function loadPersistedLedgers(): Promise<void> {
  try {
    ledgerState.workflows = await invoke<LedgerWorkflow[]>('ledger_list');
  } catch (e) {
    console.warn('ledger_list failed', e);
  }
}
