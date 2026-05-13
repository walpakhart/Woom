// Long-lived terminal state. Owns the per-instance PTY id, accumulated
// output chunks (for replay when the user navigates away and comes
// back), and the global output/exit listeners that capture bytes even
// when no <TerminalSurface> is mounted.
//
// The lifecycle the user expects:
//   • First mount of an instance → spawn PTY, register listeners.
//   • Mount → unmount → mount (e.g. user clicks Claude in the rail and
//     comes back to the terminal): the PTY keeps running on the Rust
//     side; on remount we replay the captured chunks into the fresh
//     xterm so the user sees their session continue exactly where they
//     left off — including anything the shell printed while they were
//     away.
//   • App quit: PTYs die with the parent process. Per-instance config
//     (cwd, name) lives on the layout instance record, not here.
//
// Why we don't try to keep the xterm.js Terminal instance itself alive:
// it's bound to a specific DOM host that goes away on unmount, and
// xterm.js doesn't support reparenting cleanly. Replaying chunks into
// a fresh Terminal is fast (xterm processes ANSI without rerendering
// every frame) and lets the surface fit to whatever new host size the
// remount lands in.

import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

export interface TerminalSession {
  /** Rust-side PTY id (from `terminal_spawn`). Stable for the lifetime
   *  of the session — even when the surface unmounts, this id keeps
   *  pointing at the same shell. */
  ptyId: string;
  /** The cwd this PTY was spawned with. Stored for the "Restart shell"
   *  affordance and for surface remounts that want to render breadcrumbs. */
  cwd: string | null;
  /** Last-known PTY dimensions. Tracked here so `resizeTerminal` can
   *  skip the invoke when the surface remounts at the same size — a
   *  no-op SIGWINCH would otherwise prompt zsh themes (Powerlevel10k,
   *  Starship) to redraw their prompt with a `\x1b[2J\x1b[H` sequence.
   *  Those clear-screen bytes land in `chunks`, get captured by the
   *  global listener, and the next remount's replay would end on the
   *  wiped state — exactly the "switched view, came back, terminal
   *  empty" bug that prompted this field. */
  cols: number;
  rows: number;
  /** Captured output chunks (raw base64, exactly as Tauri ships them
   *  on `terminal:output:<id>`). Replayed in order through `term.write`
   *  on every remount so the new xterm reproduces the exact screen
   *  the user had before navigating away. We keep this as base64 so
   *  multi-byte UTF-8 sequences and control codes survive the round-
   *  trip; decoding to a string would risk splitting a code point
   *  mid-byte if a chunk lands on a UTF-8 boundary. */
  chunks: string[];
  /** True after the shell exited (PTY closed). Surfaces show a
   *  "[shell exited]" banner; restart() clears this. */
  exited: boolean;
  /** Last `terminal:error:<id>` payload (if any). Surfaces render this
   *  as a banner inside the host. */
  error: string | null;
}

const _sessions = $state<Record<string, TerminalSession>>({});
const _unlistenByPty = new Map<string, UnlistenFn[]>();

/** Per-instance subscribers for new output. Each subscriber gets the
 *  single new base64 chunk as it arrives (NOT the cumulative
 *  scrollback). Replay-on-mount is handled separately by reading
 *  `chunks` directly. */
type Subscriber = (chunkB64: string) => void;
const _outputSubs = new Map<string, Set<Subscriber>>();
type ExitSub = () => void;
const _exitSubs = new Map<string, Set<ExitSub>>();
type ErrorSub = (msg: string) => void;
const _errorSubs = new Map<string, Set<ErrorSub>>();
/** Per-instance "scrollback was wiped" subscribers — fires when a
 *  user clicks the Clear affordance. The mounted surface listens so
 *  it can call `term.clear()` / `term.reset()` to flush the visible
 *  buffer alongside the cached chunks. */
type ClearSub = () => void;
const _clearSubs = new Map<string, Set<ClearSub>>();

export const terminalsState = {
  /** Live read-only view of all known sessions. Components can use
   *  this in `$derived` blocks to drive UI like "running" badges. */
  get sessions(): Record<string, TerminalSession> {
    return _sessions;
  }
};

/** Spawn-or-attach. Idempotent: a second call with the same instanceId
 *  returns the existing session without re-spawning. The cwd / cols /
 *  rows / name args are only honoured on first spawn — once the PTY
 *  exists, it keeps its original geometry until `terminal_resize` is
 *  invoked by the surface. `name` is the human-readable column label
 *  (e.g. "Vermeer") so MCP `terminal_list` returns names agents can
 *  call directly instead of opaque uuids. */
export async function ensureTerminalSession(
  instanceId: string,
  cwd: string | null,
  cols = 120,
  rows = 32,
  name: string | null = null
): Promise<TerminalSession> {
  const existing = _sessions[instanceId];
  if (existing) return existing;

  const result = await invoke<{ id: string }>('terminal_spawn', {
    /* `instance_id` is the Svelte-side handle for the column (e.g.
       `terminal-solo`). Carrying it into the Rust registry lets the
       MCP bridge resolve agent calls that pass the layout id directly,
       which is what the agent sees first in its preamble. */
    opts: { cwd, cols, rows, name, instance_id: instanceId }
  });

  _sessions[instanceId] = {
    ptyId: result.id,
    cwd,
    cols,
    rows,
    chunks: [],
    exited: false,
    error: null
  };

  /* Attach the long-lived listeners ONCE per PTY id. They survive
     surface unmounts so output keeps accumulating into `chunks` even
     when nothing's rendering — the next remount replays it. We also
     fan out to per-instance subscribers so the active surface (if
     any) can stream bytes into xterm in real time.

     CRITICAL: every mutation goes through `_sessions[instanceId]`
     (the Svelte proxy), NOT through a captured plain-object closure.
     Svelte 5's $state proxy doesn't share the underlying with a
     local reference once you've assigned the object — pushes to a
     plain `sess` would silently drop on the floor while the proxy
     stays empty, which is exactly the "switched to canvas, came
     back, terminal empty" symptom we hit in the wild. */
  const u1 = await listen<string>(`terminal:output:${result.id}`, (e) => {
    const s = _sessions[instanceId];
    if (!s) return;
    s.chunks.push(e.payload);
    const subs = _outputSubs.get(instanceId);
    if (subs) for (const fn of subs) fn(e.payload);
  });
  const u2 = await listen<null>(`terminal:exit:${result.id}`, () => {
    const s = _sessions[instanceId];
    if (s) s.exited = true;
    const subs = _exitSubs.get(instanceId);
    if (subs) for (const fn of subs) fn();
  });
  const u3 = await listen<string>(`terminal:error:${result.id}`, (e) => {
    const s = _sessions[instanceId];
    if (s) s.error = e.payload;
    const subs = _errorSubs.get(instanceId);
    if (subs) for (const fn of subs) fn(e.payload);
  });
  _unlistenByPty.set(result.id, [u1, u2, u3]);

  return _sessions[instanceId];
}

/** Subscribe to NEW output chunks for a given terminal instance. The
 *  callback fires once per chunk as Tauri ships it; replay of past
 *  chunks is the caller's responsibility (read `session.chunks`).
 *  Returns an unsubscribe function — surfaces should call it on
 *  unmount so we don't write into a stale xterm. */
export function subscribeTerminalOutput(
  instanceId: string,
  fn: Subscriber
): () => void {
  let set = _outputSubs.get(instanceId);
  if (!set) {
    set = new Set();
    _outputSubs.set(instanceId, set);
  }
  set.add(fn);
  return () => {
    set?.delete(fn);
  };
}

export function subscribeTerminalExit(instanceId: string, fn: ExitSub): () => void {
  let set = _exitSubs.get(instanceId);
  if (!set) {
    set = new Set();
    _exitSubs.set(instanceId, set);
  }
  set.add(fn);
  return () => {
    set?.delete(fn);
  };
}

export function subscribeTerminalError(instanceId: string, fn: ErrorSub): () => void {
  let set = _errorSubs.get(instanceId);
  if (!set) {
    set = new Set();
    _errorSubs.set(instanceId, set);
  }
  set.add(fn);
  return () => {
    set?.delete(fn);
  };
}

/** Subscribe to "scrollback wiped" events for an instance. Surfaces
 *  use this to trigger `term.clear()` / `term.reset()` so the visible
 *  buffer empties alongside the cached chunks. Returns an unsubscribe. */
export function subscribeTerminalClear(instanceId: string, fn: ClearSub): () => void {
  let set = _clearSubs.get(instanceId);
  if (!set) {
    set = new Set();
    _clearSubs.set(instanceId, set);
  }
  set.add(fn);
  return () => {
    set?.delete(fn);
  };
}

/** Wipe the captured scrollback for an instance and notify the
 *  mounted surface (if any) to clear its xterm. The shell process
 *  itself keeps running — this only flushes the visible / replayable
 *  history. Useful as a "Clear screen" affordance that doesn't kill
 *  the underlying PTY. */
export function clearTerminalScrollback(instanceId: string): void {
  const sess = _sessions[instanceId];
  if (sess) {
    /* Reassign rather than truncate so Svelte's $state proxy
       observes the change and downstream `$derived` blocks
       (e.g. message-count badges) re-evaluate. */
    sess.chunks = [];
    sess.error = null;
  }
  const subs = _clearSubs.get(instanceId);
  if (subs) for (const fn of subs) fn();
}

/** Restart the shell for an instance: kills the existing PTY, drops
 *  the captured scrollback, and spawns fresh. Listeners stay attached
 *  to the new PTY id under the same instance key, so subscribers
 *  don't need to re-register. Used by the surface's "Restart" button. */
export async function restartTerminalSession(
  instanceId: string,
  cwd: string | null,
  cols = 120,
  rows = 32,
  name: string | null = null
): Promise<TerminalSession> {
  const existing = _sessions[instanceId];
  if (existing) {
    const unlistens = _unlistenByPty.get(existing.ptyId);
    if (unlistens) {
      for (const u of unlistens) u();
      _unlistenByPty.delete(existing.ptyId);
    }
    try { await invoke('terminal_kill', { id: existing.ptyId }); } catch {}
    delete _sessions[instanceId];
  }
  return ensureTerminalSession(instanceId, cwd, cols, rows, name);
}

/** Permanent kill — used when the user explicitly closes a terminal
 *  instance via the rail's "Remove" affordance. Drops the PTY, the
 *  scrollback, and all subscribers (they're per-instance, so nothing
 *  external to clean up). */
export async function killTerminalSession(instanceId: string): Promise<void> {
  const sess = _sessions[instanceId];
  if (!sess) return;
  const unlistens = _unlistenByPty.get(sess.ptyId);
  if (unlistens) {
    for (const u of unlistens) u();
    _unlistenByPty.delete(sess.ptyId);
  }
  try { await invoke('terminal_kill', { id: sess.ptyId }); } catch {}
  _outputSubs.delete(instanceId);
  _exitSubs.delete(instanceId);
  _errorSubs.delete(instanceId);
  _clearSubs.delete(instanceId);
  delete _sessions[instanceId];
}

/** Forward keystrokes to the PTY. Thin wrapper so surfaces don't have
 *  to import the Tauri invoke directly. */
export async function writeToTerminal(instanceId: string, b64: string): Promise<void> {
  const sess = _sessions[instanceId];
  if (!sess) return;
  try {
    await invoke('terminal_write', { id: sess.ptyId, data: b64 });
  } catch {/* shell may have exited */}
}

/** Forward resize to the PTY (TIOCSWINSZ). No-op when the requested
 *  geometry already matches what the PTY was last sized for — sending
 *  SIGWINCH for a same-size resize wakes zsh prompt frameworks
 *  (Powerlevel10k, Starship, etc.) which redraw with a clear-screen
 *  prefix; those bytes get captured into `chunks` and clobber the
 *  visible buffer on the next replay. Tracking last-applied dims
 *  here keeps remounts visually stable. */
export async function resizeTerminal(
  instanceId: string,
  cols: number,
  rows: number
): Promise<void> {
  const sess = _sessions[instanceId];
  if (!sess) return;
  if (sess.cols === cols && sess.rows === rows) return;
  sess.cols = cols;
  sess.rows = rows;
  try {
    await invoke('terminal_resize', { id: sess.ptyId, cols, rows });
  } catch {/* shell may have exited */}
}
