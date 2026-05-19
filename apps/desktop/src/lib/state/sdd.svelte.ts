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
  /** Final wrap-up at `<workspace>/SUMMARY.md` — populated after every
   *  phase is done. Null on running / paused / failed workflows. */
  summary_path: string | null;
  summary_body: string | null;
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
  /** One-level undo target per (workspace, target). Target key is
   *  `spec` / `plan` / `phase-NN`. `body` is the pre-save markdown;
   *  `savedAt` is the ms-since-epoch at which the save fired (used
   *  to time out the affordance after 30s). In-memory only — wiped
   *  on app restart by design (the spec is explicit on this). */
  undoByWorkspace: Record<string, Record<string, { body: string; savedAt: number }>>;
  /** Self-save debounce — `lastSelfSaveAt[wsId]` = ms timestamp of the
   *  most recent `saveSddBody` call from THIS app instance. The
   *  agent-rewrite-detection branch in the `sdd:changed` listener
   *  skips its auto-clear when the incoming change landed within
   *  ~500ms of our own save — otherwise our own write round-trip
   *  would false-positive as "agent rewrote it". */
  lastSelfSaveAt: Record<string, number>;
  /** Per-session toggle for the library panel. When true, ChatThread
   *  renders a `SddLibraryCard` listing every workspace on disk so
   *  the user can re-open / discard / inspect past specs. Pure UI
   *  state — not persisted, resets on app restart. */
  libraryOpenBySession: Record<string, boolean>;
  /** Deferred silent SDD prompts, keyed by SESSION id. Populated when
   *  the user clicks Approve/Continue while the chat session is still
   *  mid-turn (e.g. agent's previous reply hasn't fully streamed in).
   *  Drained by the post-turn cleanup in `+page.svelte` — fires the
   *  pending prompt silently as soon as the session frees up. Avoids
   *  the stuck-spinner bug where a clicked Approve would silently
   *  bail because the session was busy at click-time. */
  pendingSilentBySession: Record<string, string>;
}

export const sddState = $state<SddStoreShape>({
  workspaces: [],
  workspaceBySession: {},
  unlistenByWorkspace: {},
  globalUnlisten: null,
  undoByWorkspace: {},
  lastSelfSaveAt: {},
  libraryOpenBySession: {},
  pendingSilentBySession: {},
});

/** Toggle the library panel for a session. */
export function toggleSddLibrary(sessionId: string): void {
  sddState.libraryOpenBySession[sessionId] = !sddState.libraryOpenBySession[sessionId];
}

/** Bind an existing workspace to a session — used when the user
 *  re-opens a workspace from the library. Replaces whatever was
 *  active (if anything) without deleting it; the prior workspace
 *  stays on disk and remains discoverable via the library list. */
export function bindWorkspaceToSession(sessionId: string, workspaceId: string): void {
  sddState.workspaceBySession[sessionId] = workspaceId;
  sddState.libraryOpenBySession[sessionId] = false;
}

/** Park a silent SDD prompt that couldn't fire right now (session
 *  busy). The post-turn drain in `+page.svelte` picks it up + fires
 *  silently. One slot per session — a second click overwrites; the
 *  newest stage-derived prompt is always the right one to send. */
export function setPendingSilent(sessionId: string, prompt: string): void {
  sddState.pendingSilentBySession[sessionId] = prompt;
}
export function popPendingSilent(sessionId: string): string | null {
  const v = sddState.pendingSilentBySession[sessionId];
  if (v === undefined) return null;
  delete sddState.pendingSilentBySession[sessionId];
  return v;
}

/** Stable key for the (target) addressed by a save action. Keeps
 *  spec/plan/phase routes pulling from the same record without
 *  having to thread a discriminated union through every store op. */
export function targetKey(
  target: { kind: 'spec' } | { kind: 'plan' } | { kind: 'phase'; number: number }
): string {
  if (target.kind === 'spec') return 'spec';
  if (target.kind === 'plan') return 'plan';
  return `phase-${target.number}`;
}

/** Stash a pre-save snapshot so the SddCard's "Undo last edit"
 *  affordance can restore it within 30s. Also stamps `lastSelfSaveAt`
 *  so the listener's agent-rewrite-detection branch knows the
 *  upcoming `sdd:changed` event is ours, not the agent's. */
export function stashUndo(workspaceId: string, key: string, prevBody: string): void {
  const slots = sddState.undoByWorkspace[workspaceId] ?? {};
  slots[key] = { body: prevBody, savedAt: Date.now() };
  sddState.undoByWorkspace[workspaceId] = slots;
  sddState.lastSelfSaveAt[workspaceId] = Date.now();
}

/** Read + clear the undo slot. Returns the prior body, or null. */
export function popUndo(workspaceId: string, key: string): string | null {
  const slots = sddState.undoByWorkspace[workspaceId];
  if (!slots) return null;
  const slot = slots[key];
  if (!slot) return null;
  delete slots[key];
  /* Stamp lastSelfSaveAt again — the upcoming write-back from the
   *  caller's `saveSddBody` will also emit `sdd:changed`, which the
   *  listener would otherwise treat as an agent rewrite. */
  sddState.lastSelfSaveAt[workspaceId] = Date.now();
  return slot.body;
}

/** Compare a stashed undo body against the workspace's current body
 *  for the same target key. Returns true when they DIFFER, meaning
 *  the agent has rewritten the file since we stashed. Used by the
 *  listener to drop stale undo slots. */
function bodyForKey(ws: SddWorkspace, key: string): string | null {
  if (key === 'spec') return ws.spec_body;
  if (key === 'plan') return ws.plan_body;
  if (key.startsWith('phase-')) {
    const n = Number(key.slice('phase-'.length));
    const ph = ws.phases.find((p) => p.number === n);
    return ph?.body ?? null;
  }
  return null;
}

/** Initialise the global `sdd:changed` listener AND hydrate from disk.
 *  Idempotent — safe to call from `+page.svelte` onMount more than
 *  once. Hydration rebuilds any workspaces that existed before app
 *  restart, so the user's previous SDD session resumes seamlessly. */
export async function initSdd(): Promise<void> {
  if (sddState.globalUnlisten) return;
  sddState.globalUnlisten = await listen<string>('sdd:changed', async (evt) => {
    /* Broadcast carries the workspace id. Re-pull just that one — much
     *  cheaper than `sdd_list` on every event. */
    const id = evt.payload;
    try {
      const ws = await invoke<SddWorkspace>('sdd_get', { id });
      upsertWorkspace(ws);
      /* Agent-rewrite detection. For every stashed undo slot whose
       *  underlying body now DIFFERS from the snapshot we stashed at
       *  save time AND the change DIDN'T land within ~500ms of our
       *  own save (which would be the round-trip from our own
       *  saveSddBody emit), drop the slot — the user undo'ing back
       *  to an old body the agent has since rewritten would be a
       *  footgun. */
      const slots = sddState.undoByWorkspace[id];
      if (slots) {
        const lastSelf = sddState.lastSelfSaveAt[id] ?? 0;
        const now = Date.now();
        if (now - lastSelf > 500) {
          for (const key of Object.keys(slots)) {
            const current = bodyForKey(ws, key);
            if (current !== null && current !== slots[key].body) {
              delete slots[key];
            }
          }
        }
      }
    } catch {
      /* Workspace was discarded between emit + receive — fine, drop. */
      removeWorkspace(id);
    }
  });
  /* Hydrate any workspaces left on disk from a prior run. The Rust
   *  side scans `<app_data>/sdd-workspaces/*` and rebuilds in-memory
   *  state from the files. Failures here are non-fatal — worst case
   *  the user has to /sdd again. */
  try {
    const ws = await invoke<SddWorkspace[]>('sdd_hydrate');
    for (const w of ws) upsertWorkspace(w);
  } catch (e) {
    console.warn('sdd_hydrate failed', e);
  }
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

/** Save user-edited body for the spec, plan, or a specific phase. The
 *  YAML frontmatter is preserved verbatim on the Rust side. */
export async function saveSddBody(
  id: string,
  target: { kind: 'spec' } | { kind: 'plan' } | { kind: 'phase'; number: number },
  body: string
): Promise<SddWorkspace | null> {
  const args = target.kind === 'phase'
    ? { kind: 'phase', number: target.number }
    : { kind: target.kind };
  try {
    const ws = await invoke<SddWorkspace>('sdd_save_body', { id, target: args, body });
    upsertWorkspace(ws);
    return ws;
  } catch (e) {
    console.warn('sdd_save_body failed', e);
    return null;
  }
}

/** Reset a failed (or done) phase back to `pending`. The next
 *  advance fires the phase prompt again with a fresh status. */
export async function retrySddPhase(
  id: string,
  phase: number
): Promise<SddWorkspace | null> {
  try {
    const ws = await invoke<SddWorkspace>('sdd_retry_phase', { id, phase });
    upsertWorkspace(ws);
    return ws;
  } catch (e) {
    console.warn('sdd_retry_phase failed', e);
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

type PromptKind = 'spec' | 'plan' | 'phase' | 'summary' | 'amend';

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
    case 'complete': {
      /* Workflow finished — agent writes the final wrap-up.
       *  Returned ONLY if no SUMMARY.md exists yet, so the prompt
       *  fires once per workspace. Caller (orchestrator post-turn
       *  hook) detects this and silently sends it. */
      if (ws.summary_body) return null;
      const tpl = await fetchPrompt('summary');
      return interpolate(tpl, { workspace_root: root, user_prompt: ws.user_prompt });
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

/** Build the in-place amend prompt — used when the user wants to
 *  correct the current spec / plan / phase instead of approving it.
 *  The agent edits files under `ws.root` rather than scaffolding a
 *  fresh workspace. Caller drops the result into the composer and
 *  fires the normal send pipeline. */
export async function buildAmendPrompt(
  ws: SddWorkspace,
  userChange: string
): Promise<string> {
  const tpl = await fetchPrompt('amend');
  return interpolate(tpl, {
    workspace_root: ws.root,
    user_prompt: ws.user_prompt,
    stage_kind: ws.stage.kind,
    user_change: userChange.trim(),
  });
}
