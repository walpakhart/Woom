/* Spec-Driven Development orchestrator — frontend state machine.
 *
 * Wraps the Rust `sdd_*` Tauri commands in a reactive store. Each chat
 * session can have AT MOST one active SDD workspace; `workspaceBySession`
 * is the per-session index. We refresh the workspace from disk after
 * every agent turn that touches the workspace, so the UI cards stay in
 * sync with what the agent has written.
 *
 * This file owns:
 *   - the reactive store + per-session index
 *   - listener for `sdd:changed:*` Tauri events
 *   - prompt interpolation (spec / plan / phase templates)
 *   - high-level button-handler helpers the SDD card calls
 *
 * It does NOT own the chat send pipeline — to push a prompt to the
 * agent, callers `setSessionInput(...)` + `sendClaudeMessage()` from
 * the existing chat code path. Keeps SDD a thin shell over the same
 * machinery a manual user-typed prompt uses (no second send code path
 * to maintain).
 */

import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

// --- Mirrored types from `sdd.rs` ------------------------------------

export type SddStage =
  | { kind: 'drafting' }
  | { kind: 'spec_ready' }
  | { kind: 'planning' }
  | { kind: 'plan_ready' }
  | { kind: 'phase_running'; phase: number }
  | { kind: 'phase_done'; phase: number }
  | { kind: 'complete' }
  | { kind: 'paused' }
  | { kind: 'stopped' }
  | { kind: 'failed'; reason: string };

export interface SddPhase {
  number: number;
  slug: string;
  title: string;
  depends_on: number[];
  status: string; // pending | running | done | failed | skipped
  tasks_total: number;
  tasks_done: number;
  body: string;
  path: string;
  summary: string | null;
}

export interface SddWorkspace {
  id: string;
  session_id: string | null;
  root: string;
  stage: SddStage;
  user_prompt: string;
  spec_path: string | null;
  spec_body: string | null;
  plan_path: string | null;
  plan_body: string | null;
  phases: SddPhase[];
  created_at: number;
  updated_at: number;
}

// --- Store -----------------------------------------------------------

interface SddStoreShape {
  /** All known workspaces — newest-first. */
  workspaces: SddWorkspace[];
  /** Session id → workspace id mapping. One SDD workspace per session
   *  at a time; if the user starts a second SDD in the same session
   *  the previous one is dropped (`/sdd` overwrites). */
  workspaceBySession: Record<string, string>;
  /** Per-workspace targeted listener handles — we tear these down on
   *  discard so dead workspaces don't keep waking the renderer. */
  unlistenByWorkspace: Record<string, UnlistenFn>;
  /** Global broadcast listener — one per app lifecycle. */
  globalUnlisten: UnlistenFn | null;
}

export const sddState = $state<SddStoreShape>({
  workspaces: [],
  workspaceBySession: {},
  unlistenByWorkspace: {},
  globalUnlisten: null,
});

/** Initialise the global `sdd:changed` listener. Idempotent — safe to
 *  call from `+page.svelte` onMount more than once. */
export async function initSdd(): Promise<void> {
  if (sddState.globalUnlisten) return;
  sddState.globalUnlisten = await listen<string>('sdd:changed', async (evt) => {
    /* Broadcast carries the workspace id. Re-pull just that one — much
     *  cheaper than `sdd_list` on every event. */
    const id = evt.payload;
    try {
      const ws = await invoke<SddWorkspace>('sdd_get', { id });
      upsertWorkspace(ws);
    } catch {
      /* Workspace was discarded between emit + receive — fine, drop. */
      removeWorkspace(id);
    }
  });
}

function upsertWorkspace(ws: SddWorkspace): void {
  const idx = sddState.workspaces.findIndex((w) => w.id === ws.id);
  if (idx === -1) {
    sddState.workspaces = [ws, ...sddState.workspaces];
  } else {
    const copy = [...sddState.workspaces];
    copy[idx] = ws;
    sddState.workspaces = copy;
  }
  if (ws.session_id) {
    sddState.workspaceBySession[ws.session_id] = ws.id;
  }
}

function removeWorkspace(id: string): void {
  sddState.workspaces = sddState.workspaces.filter((w) => w.id !== id);
  for (const [sid, wid] of Object.entries(sddState.workspaceBySession)) {
    if (wid === id) delete sddState.workspaceBySession[sid];
  }
  const un = sddState.unlistenByWorkspace[id];
  if (un) {
    try { un(); } catch { /* noop */ }
    delete sddState.unlistenByWorkspace[id];
  }
}

/** Active workspace for a session, or null. */
export function workspaceForSession(sessionId: string): SddWorkspace | null {
  const wid = sddState.workspaceBySession[sessionId];
  if (!wid) return null;
  return sddState.workspaces.find((w) => w.id === wid) ?? null;
}

// --- Commands --------------------------------------------------------

/** Kick off a new SDD workspace bound to the chat session. If a
 *  workspace is already attached to this session, it's discarded first
 *  (one SDD per session, simplest model). */
export async function startSdd(
  sessionId: string,
  userPrompt: string
): Promise<SddWorkspace | null> {
  // Discard any existing workspace tied to this session.
  const existing = sddState.workspaceBySession[sessionId];
  if (existing) {
    try { await invoke('sdd_discard', { id: existing }); } catch { /* noop */ }
    removeWorkspace(existing);
  }
  try {
    const ws = await invoke<SddWorkspace>('sdd_start', {
      args: { session_id: sessionId, user_prompt: userPrompt },
    });
    upsertWorkspace(ws);
    return ws;
  } catch (e) {
    console.warn('sdd_start failed', e);
    return null;
  }
}

/** Re-read workspace state from disk. Call after the agent finishes a
 *  turn that should have written files (spec.md, plan.md, phase
 *  files). The Rust side derives the stage from what's on disk. */
export async function refreshSdd(id: string): Promise<SddWorkspace | null> {
  try {
    const ws = await invoke<SddWorkspace>('sdd_refresh', { id });
    upsertWorkspace(ws);
    return ws;
  } catch (e) {
    console.warn('sdd_refresh failed', e);
    return null;
  }
}

export async function approveSdd(
  id: string,
  target: 'spec' | 'plan'
): Promise<SddWorkspace | null> {
  try {
    const ws = await invoke<SddWorkspace>('sdd_approve', { id, target });
    upsertWorkspace(ws);
    return ws;
  } catch (e) {
    console.warn('sdd_approve failed', e);
    return null;
  }
}

export async function pauseSdd(id: string): Promise<void> {
  try { await invoke('sdd_pause', { id }); } catch (e) { console.warn(e); }
}
export async function resumeSdd(id: string): Promise<void> {
  try { await invoke('sdd_resume', { id }); } catch (e) { console.warn(e); }
}
export async function stopSdd(id: string): Promise<void> {
  try { await invoke('sdd_stop', { id }); } catch (e) { console.warn(e); }
}
export async function discardSdd(id: string): Promise<void> {
  try { await invoke('sdd_discard', { id }); } catch (e) { console.warn(e); }
  removeWorkspace(id);
}

// --- Prompt assembly -------------------------------------------------

type PromptKind = 'spec' | 'plan' | 'phase';

/** Fetch a prompt template from the Rust side. Templates are embedded
 *  via `include_str!` at build time so they ship with the binary. */
async function fetchPrompt(kind: PromptKind): Promise<string> {
  return await invoke<string>('sdd_prompt', { kind });
}

/** Substitute `{{key}}` placeholders. Keeps placeholders that weren't
 *  provided so an unfilled template is visible in the chat (easier
 *  debugging than a silently-blank prompt). */
function interpolate(tpl: string, vars: Record<string, string>): string {
  return tpl.replace(/\{\{(\w+)\}\}/g, (_m, k) => (vars[k] !== undefined ? vars[k] : `{{${k}}}`));
}

/** Build the prompt the agent should receive for the workspace's
 *  CURRENT stage. Returns null when the stage doesn't have an agent-
 *  facing prompt (Complete / Stopped / Paused / Failed).
 *
 *  The caller (UI / slash handler) drops this into `session.input`
 *  and fires `sendClaudeMessage()` so the existing send pipeline
 *  handles hooks, history, etc. */
export async function buildPromptForStage(ws: SddWorkspace): Promise<string | null> {
  const root = ws.root;
  switch (ws.stage.kind) {
    case 'drafting': {
      const tpl = await fetchPrompt('spec');
      return interpolate(tpl, { workspace_root: root, user_prompt: ws.user_prompt });
    }
    case 'planning': {
      const tpl = await fetchPrompt('plan');
      return interpolate(tpl, { workspace_root: root, user_prompt: ws.user_prompt });
    }
    case 'plan_ready':
    case 'phase_done': {
      /* Plan approved (or previous phase done) — find next phase to
       *  execute. Sequential: lowest-numbered phase still in `pending`
       *  status. */
      const next = ws.phases.find((p) => p.status === 'pending');
      if (!next) return null;
      const tpl = await fetchPrompt('phase');
      return interpolate(tpl, {
        workspace_root: root,
        user_prompt: ws.user_prompt,
        phase_number: String(next.number),
        phase_slug: next.slug,
        phase_file: `${next.slug}.md`,
        retries_max: '1',
      });
    }
    case 'phase_running':
      return null; // already in flight
    case 'spec_ready':
      return null; // waiting on user approve
    default:
      return null;
  }
}

/** Convenience for the slash command. Returns interpolated spec prompt
 *  ready to send. */
export async function buildKickoffPrompt(
  ws: SddWorkspace
): Promise<string> {
  const tpl = await fetchPrompt('spec');
  return interpolate(tpl, {
    workspace_root: ws.root,
    user_prompt: ws.user_prompt,
  });
}
