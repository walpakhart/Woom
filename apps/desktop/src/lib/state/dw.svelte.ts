// Dynamic Workflow state (SDD `sdd-98a42f3bdb` Phase 4). One reactive
// singleton holding every live + recently-finished workflow. Card +
// preflight modal read straight from this; backend events feed in via
// `tauri::emit` listeners wired in `+page.svelte`.

import { invoke } from '@tauri-apps/api/core';
import type { DynamicWorkflow, DwSubagent } from '$lib/types';

/** Light-weight summary returned by `dw_list` — mirrors the Rust
 *  `DwSummary` struct. Phase 5 hydration uses this to populate the
 *  in-memory state on app boot; full workflow JSON is loaded lazily
 *  via `loadWorkflow` when the card needs the subagent detail. */
export interface DwSummary {
  id: string;
  status: DynamicWorkflow['status'];
  sessionId: string;
  completedAt?: number;
  totalCostUsd: number;
  subagentCount: number;
  userPrompt: string;
  planRationale?: string;
  createdAt: number;
}

export const dwState = $state<{ workflows: DynamicWorkflow[] }>({
  workflows: []
});

export function addWorkflow(w: DynamicWorkflow): void {
  dwState.workflows = [w, ...dwState.workflows];
}

export function getWorkflow(id: string): DynamicWorkflow | null {
  return dwState.workflows.find((w) => w.id === id) ?? null;
}

/** Statuses where the workflow is still in-flight (not a terminal
 *  record). The pinned, bottom-following card slot renders the active
 *  workflow here so it stays visible like the SDD card, instead of
 *  scrolling away at its origin message. */
const ACTIVE_DW_STATUSES = new Set([
  'building',
  'awaiting_approval',
  'running',
  'awaiting_verify',
  'verifying',
  'paused_quota',
]);

/** The session's currently-active workflow, if any. Newest first —
 *  `addWorkflow` prepends, so the first match is the latest. */
export function activeWorkflowForSession(sessionId: string): DynamicWorkflow | null {
  return (
    dwState.workflows.find(
      (w) => w.sessionId === sessionId && ACTIVE_DW_STATUSES.has(w.status)
    ) ?? null
  );
}

/** Is this workflow still in-flight? The pinned slot renders active
 *  workflows (bottom-following); the per-message slot renders only
 *  terminal ones (done/failed/cancelled) so a finished run stays as a
 *  record at its origin without double-rendering the live card. */
export function isWorkflowActive(id: string): boolean {
  const w = dwState.workflows.find((x) => x.id === id);
  return w ? ACTIVE_DW_STATUSES.has(w.status) : false;
}

/** Aggregate DW spend + run count for a session. DW subagent cost lives
 *  on the workflow (not on chat-message `usage`), so the session budget
 *  chip / popover have to fold it in explicitly or the $ total ignores
 *  everything the fan-outs spent. */
export function sessionDwTotals(sessionId: string): {
  costUsd: number;
  runs: number;
  quota5h: number;
  quota7d: number;
} {
  let costUsd = 0;
  let runs = 0;
  let quota5h = 0;
  let quota7d = 0;
  for (const w of dwState.workflows) {
    if (w.sessionId !== sessionId) continue;
    costUsd += w.totalCostUsd || 0;
    quota5h += w.quotaDelta5h || 0;
    quota7d += w.quotaDelta7d || 0;
    runs += 1;
  }
  return { costUsd, runs, quota5h, quota7d };
}

export function updateWorkflow(id: string, patch: Partial<DynamicWorkflow>): void {
  const list = dwState.workflows;
  for (let i = 0; i < list.length; i++) {
    if (list[i].id === id) {
      list[i] = { ...list[i], ...patch };
      return;
    }
  }
}

export function updateSubagent(
  workflowId: string,
  subagentId: string,
  patch: Partial<DwSubagent>
): void {
  const list = dwState.workflows;
  for (let i = 0; i < list.length; i++) {
    if (list[i].id !== workflowId) continue;
    const subs = list[i].subagents.map((s) =>
      s.id === subagentId ? { ...s, ...patch } : s
    );
    list[i] = { ...list[i], subagents: subs };
    return;
  }
}

/** Sum of all subagent costs — fed back into `workflow.totalCostUsd`
 *  via `updateWorkflow` after each subagent event. */
export function aggregateCost(w: DynamicWorkflow): number {
  return w.subagents.reduce((acc, s) => acc + (s.costUsd || 0), 0);
}

/** Hydrate dwState.workflows from disk on app boot (Phase 5). Each
 *  summary becomes a shell `DynamicWorkflow` — full subagent detail
 *  loads on demand via `loadWorkflow` when the user expands a card. */
export async function loadPersistedWorkflows(): Promise<void> {
  try {
    const list = await invoke<DwSummary[]>('dw_list');
    const shells: DynamicWorkflow[] = list.map((s) => ({
      id: s.id,
      sessionId: s.sessionId,
      userPrompt: s.userPrompt,
      status: s.status,
      planRationale: s.planRationale,
      subagents: [],
      budgetCapUsd: 5,
      totalCostUsd: s.totalCostUsd,
      createdAt: s.createdAt,
      completedAt: s.completedAt
    }));
    dwState.workflows = shells;
  } catch (e) {
    console.warn('dw_list failed', e);
  }
}

/** Lazy-load the full workflow JSON for an id. Card calls this in an
 *  `$effect` when it sees a shell entry (empty subagents). Idempotent —
 *  the backend caches the on-disk parse into its registry. */
export async function loadWorkflow(id: string): Promise<void> {
  try {
    const wf = await invoke<DynamicWorkflow | null>('dw_get', { workflowId: id });
    if (wf) updateWorkflow(id, wf);
  } catch (e) {
    console.warn('dw_get failed', e);
  }
}
