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
