/* Background-task reactive store. Mirrors `BgRegistry` in `bg_tasks.rs`:
 * one row per long-running process the agent or user spawned. The Preview
 * pane (right side of Claude/Cursor solo) renders this list; each row's
 * detail subscribes to `bg:line:<id>` for live log tail and `bg:status:<id>`
 * for exit transitions.
 *
 * Source of truth lives in Rust. We refetch on `bg:tasks-changed` (broad
 * notification — spawn/kill/exit) and apply per-task patches on
 * `bg:status:<id>` (status flips). Per-line events stream into a separate
 * per-task buffer; we DON'T re-render the whole list on every line. */

import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

export type BgStream = 'stdout' | 'stderr';

export interface BgLine {
  id: string;
  at: number;
  stream: BgStream;
  line: string;
}

export type BgStatus =
  | { kind: 'running' }
  | { kind: 'exited'; code: number }
  | { kind: 'killed'; reason: string };

export interface BgTask {
  id: string;
  label: string;
  cmd: string;
  cwd: string;
  session_id: string | null;
  pid: number | null;
  started_at: number;
  status: BgStatus;
  log_path: string;
  detected_urls: string[];
  detected_ports: number[];
  recent_lines: BgLine[];
}

export interface BgTasksState {
  /** All known tasks, newest first (matches the Rust list() order). */
  tasks: BgTask[];
  /** Per-task in-memory log tail. Capped — we re-fetch the file
   *  contents via `bg_logs` when the user opens the detail view, so
   *  this is just the "since-mount" stream. */
  lines: Record<string, BgLine[]>;
  /** Active row in the Preview pane (user-selected). Null = show the
   *  most recent task by default. */
  activeId: string | null;
  /** Bound to the per-task `bg:line:<id>` listeners we've installed so
   *  we don't double-subscribe. */
  lineUnlisten: Record<string, UnlistenFn>;
  /** Single global listener handles. */
  globalUnlisten: UnlistenFn[];
  /** How many lines per task we keep in memory. 500 is plenty for the
   *  log-tail UI; older lines live in the rolling file on disk. */
  lineCapPerTask: number;
}

export const bgTasksState = $state<BgTasksState>({
  tasks: [],
  lines: {},
  activeId: null,
  lineUnlisten: {},
  globalUnlisten: [],
  lineCapPerTask: 500,
});

/** Refetch the full task list. Cheap (Rust just walks an in-memory
 *  HashMap), so we call this on any `bg:tasks-changed` notification. */
export async function refreshBgTasks(): Promise<void> {
  try {
    const list = await invoke<BgTask[]>('bg_list');
    bgTasksState.tasks = list;
    // Subscribe to per-task `bg:line:<id>` for any task we haven't seen
    // yet. Tear down listeners for tasks that have been removed (we
    // don't remove on exit — only on the eventual `bg_remove` we'll
    // add later. For now, listeners for exited tasks stay armed; the
    // channel is dropped server-side so the listener is silently a
    // no-op).
    const seen = new Set(list.map((t) => t.id));
    for (const id of Object.keys(bgTasksState.lineUnlisten)) {
      if (!seen.has(id)) {
        try { bgTasksState.lineUnlisten[id](); } catch { /* noop */ }
        delete bgTasksState.lineUnlisten[id];
        delete bgTasksState.lines[id];
      }
    }
    for (const t of list) {
      if (!bgTasksState.lineUnlisten[t.id]) {
        await subscribeTaskLines(t.id);
      }
    }
    if (!bgTasksState.activeId && list.length > 0) {
      bgTasksState.activeId = list[0].id;
    }
    if (bgTasksState.activeId && !seen.has(bgTasksState.activeId)) {
      bgTasksState.activeId = list[0]?.id ?? null;
    }
  } catch (e) {
    console.warn('bg_list failed', e);
  }
}

async function subscribeTaskLines(id: string): Promise<void> {
  try {
    const un = await listen<BgLine>(`bg:line:${id}`, (evt) => {
      const cur = bgTasksState.lines[id] ?? [];
      cur.push(evt.payload);
      if (cur.length > bgTasksState.lineCapPerTask) {
        const drop = cur.length - bgTasksState.lineCapPerTask;
        cur.splice(0, drop);
      }
      bgTasksState.lines[id] = cur;
    });
    bgTasksState.lineUnlisten[id] = un;
  } catch (e) {
    console.warn(`subscribe bg:line:${id}`, e);
  }
}

/** Boot the listeners. Called once from `+page.svelte` onMount. Status
 *  flips piggyback on `bg:tasks-changed` (Rust fires that AFTER every
 *  status mutation), so the single broad listener handles both spawn
 *  and exit transitions. */
export async function initBgTasks(): Promise<void> {
  if (bgTasksState.globalUnlisten.length > 0) return;
  const unChanged = await listen<string>('bg:tasks-changed', () => {
    void refreshBgTasks();
  });
  bgTasksState.globalUnlisten.push(unChanged);
  await refreshBgTasks();
}

export async function spawnBgTask(args: {
  cmd: string;
  cwd: string;
  label?: string;
  session_id?: string;
  env?: Record<string, string>;
}): Promise<BgTask | null> {
  try {
    const task = await invoke<BgTask>('bg_spawn', { args });
    bgTasksState.activeId = task.id;
    // refreshBgTasks fires from the bg:tasks-changed event, but
    // pre-populating now keeps the UI snappy.
    bgTasksState.tasks = [task, ...bgTasksState.tasks.filter((t) => t.id !== task.id)];
    await subscribeTaskLines(task.id);
    return task;
  } catch (e) {
    console.warn('bg_spawn failed', e);
    return null;
  }
}

export async function killBgTask(id: string): Promise<void> {
  try {
    await invoke<void>('bg_kill', { id });
  } catch (e) {
    console.warn('bg_kill failed', e);
  }
}

export async function sendBgStdin(id: string, data: string): Promise<void> {
  try {
    await invoke<void>('bg_send_stdin', { id, data });
  } catch (e) {
    console.warn('bg_send_stdin failed', e);
  }
}

export async function fetchBgLogs(id: string, tail?: number): Promise<string> {
  try {
    return await invoke<string>('bg_logs', { id, tail });
  } catch (e) {
    console.warn('bg_logs failed', e);
    return '';
  }
}

/** Quick helper to label-or-id-resolve a task. Used by `/kill <token>`. */
export function findBgTaskByToken(token: string): BgTask | null {
  const t = token.toLowerCase();
  return (
    bgTasksState.tasks.find((x) => x.id === token) ??
    bgTasksState.tasks.find((x) => x.label.toLowerCase().includes(t)) ??
    bgTasksState.tasks.find((x) => x.cmd.toLowerCase().includes(t)) ??
    null
  );
}
