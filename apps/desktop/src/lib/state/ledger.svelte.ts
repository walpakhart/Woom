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
  model: string;
  totalCostUsd: number;
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

/** Hydrate from disk on app boot. `ledger_list` returns full workflows
 *  (checklists are small — no shell/lazy split needed like DW). */
export async function loadPersistedLedgers(): Promise<void> {
  try {
    ledgerState.workflows = await invoke<LedgerWorkflow[]>('ledger_list');
  } catch (e) {
    console.warn('ledger_list failed', e);
  }
}
