<script lang="ts">
  /*
   * TerminalSurface — xterm.js view onto a long-lived PTY-backed shell.
   *
   * The PTY lifecycle is owned by `$lib/state/terminals.svelte.ts` so
   * the shell survives view switches: when the user clicks Claude in
   * the rail and comes back, the same shell is still running and we
   * just replay captured output into a fresh xterm instance. We do
   * NOT call `terminal_kill` on unmount — only on explicit user
   * action ("close terminal") or via `restartTerminalSession`.
   *
   * Output flow:
   *   • `ensureTerminalSession` returns the existing session (or
   *     spawns one) and registers a long-lived listener on
   *     `terminal:output:<ptyId>` that pushes every chunk into
   *     `session.chunks`.
   *   • On mount we replay `session.chunks` into the new xterm so the
   *     screen state matches what the user saw before.
   *   • While mounted, we subscribe to the terminals-state pub/sub so
   *     new chunks land in xterm in real time alongside being
   *     persisted.
   *
   * Resize: a ResizeObserver on the host element calls `fit.fit()`
   * whenever the surface resizes, then forwards new (cols, rows) via
   * `resizeTerminal` so the kernel's TIOCSWINSZ matches.
   *
   * Phase-2 MCP write: when a Claude / Cursor agent calls
   * `terminal.write(id, data)` via MCP, the bytes go through the same
   * master fd this component reads from — the user sees keystrokes
   * appear live without any extra plumbing.
   */
  import { onMount, onDestroy } from 'svelte';
  import { Terminal } from '@xterm/xterm';
  import { FitAddon } from '@xterm/addon-fit';
  import { WebLinksAddon } from '@xterm/addon-web-links';
  import '@xterm/xterm/css/xterm.css';
  import { sessionsState } from '$lib/state/sessions.svelte';
  import {
    ensureTerminalSession,
    restartTerminalSession,
    subscribeTerminalOutput,
    subscribeTerminalExit,
    subscribeTerminalError,
    subscribeTerminalClear,
    writeToTerminal,
    resizeTerminal
  } from '$lib/state/terminals.svelte';

  interface Props {
    instanceId: string;
    /** Optional initial cwd. Falls back to $HOME inside the shell. */
    cwd?: string | null;
    /** Mirror xterm's selection state up to the parent (TerminalApp)
     *  so it can render the floating "Apply to <agent>" popover. The
     *  payload is `null` when nothing is selected; when set, `text`
     *  carries the captured chunk and `anchor` carries viewport-fixed
     *  pixel coords for the popover (right edge of the last selected
     *  cell, mirrored from xterm's selection geometry). Parent reads
     *  it as transient state — drop it on `onClearSelection` to
     *  dismiss the popover after Apply. */
    onSelectionChange?: (
      payload: { text: string; anchor: { x: number; y: number } } | null
    ) => void;
    /** Imperatively clear the xterm selection from the parent. Used
     *  after the user picks an "Apply to" target so the floating chip
     *  hides itself. Optional — TerminalSurface tracks selection
     *  internally and parent doesn't need it for read-only flows. */
    clearSelectionRef?: { fn: (() => void) | null };
  }
  let { instanceId, cwd = null, onSelectionChange, clearSelectionRef }: Props = $props();

  /**
   * Sessions that link THIS terminal (`linkedTerminalInstanceId === instanceId`).
   * Drives the "Linked: <session>" pill in the header so the user knows
   * which agent will land here when it calls `terminal_run`.
   *
   * Auto-link convention surfaced to the user: if any of those sessions
   * also link an editor, the editor's repoPath wins as the spawn cwd
   * (over the explicit `cwd` prop). Lets the user "make a chat-bound
   * terminal that follows the chat's project" with one click in the
   * AgentApp.
   */
  const linkedSessions = $derived.by(() => {
    const out: { sessionId: string; title: string; kind: 'claude' | 'cursor' }[] = [];
    for (const s of sessionsState.list) {
      if (s.linkedTerminalInstanceId !== instanceId) continue;
      out.push({
        sessionId: s.id,
        title: s.title,
        kind: s.agentKind
      });
    }
    return out;
  });

  /** Most recent linked-session whose chat ALSO links an editor — we use
   *  that editor's repoPath as the auto-cwd. Picks the first match deterministically. */
  const autoLinkedCwd = $derived.by(() => {
    for (const s of sessionsState.list) {
      if (s.linkedTerminalInstanceId !== instanceId) continue;
      if (!s.linkedToEditor || !s.linkedToEditorInstanceId) continue;
      const slot = sessionsState.editorInstanceState[s.linkedToEditorInstanceId];
      if (slot?.repoPath) return slot.repoPath;
    }
    return null;
  });

  let host = $state<HTMLDivElement | null>(null);
  let term: Terminal | null = null;
  let fit: FitAddon | null = null;
  let unsubOutput: (() => void) | null = null;
  let unsubExit: (() => void) | null = null;
  let unsubError: (() => void) | null = null;
  let unsubClear: (() => void) | null = null;
  let resizeObserver: ResizeObserver | null = null;
  let busy = $state(true);
  let error = $state<string | null>(null);
  let exited = $state(false);


  /**
   * Convert a base64-encoded chunk from the PTY into bytes and
   * write to xterm. Doing the decode inline keeps the IPC payload
   * binary-clean (Tauri serialises strings as UTF-16, which would
   * mangle control bytes if we shipped raw text).
   */
  function writeChunk(b64: string) {
    if (!term) return;
    const bin = atob(b64);
    const bytes = new Uint8Array(bin.length);
    for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
    term.write(bytes);
  }

  /** Pack a UTF-8 string back into base64 for the Rust side. */
  function toB64(s: string): string {
    const bytes = new TextEncoder().encode(s);
    let bin = '';
    for (const b of bytes) bin += String.fromCharCode(b);
    return btoa(bin);
  }

  /**
   * Recompute terminal dimensions to fit the host element, then push
   * the new (cols, rows) to the PTY so applications using TIOCGWINSZ
   * (vim, htop, less, …) redraw correctly. Failures are silent: the
   * shell may have already exited.
   */
  function fitAndPush() {
    if (!fit || !term) return;
    try {
      fit.fit();
    } catch {
      return;
    }
    void resizeTerminal(instanceId, term.cols, term.rows);
  }

  onMount(() => {
    if (!host) return;

    /* Pull surface + text + accent from the live theme so the
     * terminal blends with the rest of the app — Header, host padding,
     * and the xterm canvas all settle on `--bg-1` so the terminal
     * reads as part of the chrome family instead of the deeper
     * `--bg-0` used by Editor / Agent / Sentry. */
    const css = getComputedStyle(document.documentElement);
    const v = (name: string, fallback: string) =>
      (css.getPropertyValue(name) || fallback).trim() || fallback;
    const text0 = v('--text-0', '#EDE5D1');
    const accentBright = v('--accent-bright', '#E8A33A');
    const bg1 = v('--bg-1', '#131A23');

    term = new Terminal({
      fontFamily: '"JetBrains Mono", "SF Mono", ui-monospace, monospace',
      fontSize: 12.5,
      lineHeight: 1.25,
      cursorBlink: true,
      scrollback: 5000,
      allowProposedApi: true,
      convertEol: false,
      // Use the live theme's surface + foreground so the terminal
      // doesn't look like an embed of a different app. Per-source
      // palette below stays a fixed warm/blue mix that reads well
      // on every theme — those are content colours from `ls`,
      // `git status`, etc., not chrome.
      theme: {
        background: bg1,
        foreground: text0,
        cursor: accentBright,
        cursorAccent: bg1,
        /* Selection halo — sage tint matching the new app accent. */
        selectionBackground: 'rgba(176, 220, 200, 0.32)',
        black: '#0E1112',
        red: '#D4664A',
        green: '#6FAE88',
        yellow: '#D99540',
        blue: '#6FA9F2',
        magenta: '#B289F2',
        cyan: '#7FD9D9',
        white: '#C8CDC9',
        brightBlack: '#5C6663',
        brightRed: '#E48C70',
        brightGreen: '#8FCAA0',
        brightYellow: '#E5B574',
        brightBlue: '#92BFFF',
        brightMagenta: '#CBA9FF',
        brightCyan: '#A1ECEC',
        brightWhite: '#EBEFEC'
      }
    });
    fit = new FitAddon();
    term.loadAddon(fit);
    term.loadAddon(new WebLinksAddon());
    term.open(host);

    // Forward keystrokes to the PTY. Multi-byte input (paste, dead
    // keys, IME) all goes through `onData` as a single string, which
    // we re-encode to base64 before the invoke.
    term.onData((data) => {
      void writeToTerminal(instanceId, toB64(data));
    });

    /* Selection → "Apply to <agent>" hook. Xterm fires
       `onSelectionChange` whenever the user drags-or shift-clicks; we
       reflect the captured text (and a fixed-position anchor at the
       right end of the last selected cell) up to the parent so it can
       render a floating chip without TerminalSurface needing to know
       about sessions. The popover's lifetime mirrors the selection's
       — clearing the selection (Esc, click, scroll-out-then-clear)
       hides the chip via the same callback firing with `null`. */
    term.onSelectionChange(() => {
      if (!term || !host || !onSelectionChange) return;
      const text = term.getSelection();
      if (!text) {
        onSelectionChange(null);
        return;
      }
      /* Anchor coords: viewport-relative pixel position at the right
         edge of the last selected cell, computed from the host's
         bounding box + xterm's current cell metrics. We don't reach
         into xterm internals — `term.cols` / `term.rows` plus the
         host's clientWidth/Height give us a good-enough estimate
         (xterm centres the canvas inside the host with small padding,
         which we ignore; the popover has a `transform: translate`
         offset to nudge it clear of the cell anyway). The popover is
         `position: fixed` so we add the host's viewport rect. */
      const rect = host.getBoundingClientRect();
      let anchor: { x: number; y: number };
      const sel = term.getSelectionPosition?.();
      if (sel && term.cols > 0 && term.rows > 0) {
        const cellW = rect.width / term.cols;
        const cellH = rect.height / term.rows;
        anchor = {
          x: rect.left + (sel.end.x + 1) * cellW,
          y: rect.top + (sel.end.y + 1) * cellH
        };
      } else {
        /* Fallback when xterm doesn't have a selection rect (rare —
           usually means the selection is entirely off-screen). Park
           the chip at bottom-right of the viewport so it stays
           reachable. */
        anchor = { x: rect.right - 12, y: rect.bottom - 12 };
      }
      onSelectionChange({ text, anchor });
    });

    /* Imperative escape hatch for the parent: e.g. after the user
       picks "Apply to Claude · Mona-Lisa", clear xterm's native
       selection so the popover's dismissal isn't shadowed by xterm
       still showing the highlight rectangle. We expose it as a ref
       object instead of a callback prop because Svelte 5 doesn't
       have a clean two-way handle pattern; the ref's `fn` field is
       set on mount and consumed on demand by the parent. */
    if (clearSelectionRef) {
      clearSelectionRef.fn = () => {
        term?.clearSelection();
        onSelectionChange?.(null);
      };
    }

    // Attach to a long-lived PTY-backed session: returns the existing
    // one if this instance was rendered before, spawns a new one
    // otherwise. Either way, the session's accumulated `chunks` are
    // immediately replayed into the fresh xterm so the screen state
    // matches what the user saw before navigating away.
    (async () => {
      try {
        try { fit.fit(); } catch {}
        const cols = term.cols;
        const rows = term.rows;
        const sess = await ensureTerminalSession(instanceId, autoLinkedCwd ?? cwd, cols, rows);

        /* Replay every captured chunk in order. Xterm processes ANSI
           and writes to its scrollback buffer synchronously, so this
           is fast even for long histories — the user sees the prior
           session re-render in one frame. */
        for (const chunkB64 of sess.chunks) writeChunk(chunkB64);
        if (sess.exited) {
          exited = true;
          term?.write('\r\n\x1b[2m[shell exited]\x1b[0m\r\n');
        }
        if (sess.error) error = sess.error;

        /* Subscribe to NEW output for live streaming. Replay above
           caught up history; this handles everything that arrives
           from the PTY going forward. The subscription auto-detaches
           on unmount via the unsub fns in onDestroy. */
        unsubOutput = subscribeTerminalOutput(instanceId, (b64) => writeChunk(b64));
        unsubExit = subscribeTerminalExit(instanceId, () => {
          exited = true;
          term?.write('\r\n\x1b[2m[shell exited]\x1b[0m\r\n');
        });
        unsubError = subscribeTerminalError(instanceId, (msg) => {
          error = msg;
        });
        /* Wipe-on-clear: when something else fires
           `clearTerminalScrollback(instanceId)` (e.g. the Clear button
           in TerminalApp's header), reset our xterm so the visible
           buffer empties alongside the cached chunks. */
        unsubClear = subscribeTerminalClear(instanceId, () => {
          term?.reset();
          exited = false;
          error = null;
        });

        /* The ResizeObserver attached below will fire its initial
           callback as soon as the host's first layout settles and
           push the current dims via `fitAndPush` → `resizeTerminal`.
           That call no-ops when the geometry already matches the
           PTY's last-known dims (tracked inside terminalsState), so
           we don't accidentally SIGWINCH the shell on a clean
           remount and trigger prompt-redraw clear sequences. */
        busy = false;
      } catch (e) {
        error = typeof e === 'string' ? e : String(e);
        busy = false;
      }
    })();

    // Resize observer pushes new dimensions to the PTY — debounced
    // by browser frame timing already (ResizeObserver fires once
    // per layout). Using one observer over rAF avoids a jittery
    // resize during splitter drag.
    resizeObserver = new ResizeObserver(() => fitAndPush());
    resizeObserver.observe(host);
  });

  onDestroy(() => {
    resizeObserver?.disconnect();
    /* Drop our subscriptions so we don't write into a torn-down xterm,
       BUT do NOT kill the PTY — the global terminals-state keeps it
       alive so the next mount of this instance can replay output. The
       only paths that kill a PTY are explicit user actions: rail
       "Remove instance" or the surface's restart() below. */
    unsubOutput?.();
    unsubExit?.();
    unsubError?.();
    unsubClear?.();
    unsubOutput = unsubExit = unsubError = unsubClear = null;
    if (clearSelectionRef) clearSelectionRef.fn = null;
    term?.dispose();
    term = null;
    fit = null;
  });

  /**
   * Soft-restart: kill the current PTY, spawn a fresh one. Useful
   * when the user wants a clean shell after a crashed process or
   * after editing $PATH and wanting it picked up. Also drops the
   * captured scrollback so the new shell starts on an empty screen.
   */
  async function restart() {
    error = null;
    exited = false;
    busy = true;
    term?.clear();
    try {
      const cols = term?.cols ?? 120;
      const rows = term?.rows ?? 32;
      await restartTerminalSession(instanceId, cwd, cols, rows);
      /* Subscribers stay attached because they're keyed by instanceId,
         not PTY id — `restartTerminalSession` rewires the listeners
         under the same instance key. Same xterm, fresh PTY. */
      busy = false;
    } catch (e) {
      error = typeof e === 'string' ? e : String(e);
      busy = false;
    }
  }
  /* Suppress lint noise for `restart` — it's intentionally retained
     as a future-affordance hook that the rail / cwd-bar will wire in
     a follow-up; not exposing it via UI yet keeps the surface focused. */
  void restart;
</script>

<section
  class="terminal-surface"
  data-instance-id={instanceId}
  data-kind="terminal"
>
  <div class="term-host" bind:this={host}></div>

  {#if error}
    <div class="term-error" role="alert">{error}</div>
  {/if}
</section>

<style>
  .terminal-surface {
    background: var(--bg-1);
    width: 100%; height: 100%;
    flex: 1 1 auto; min-width: 0; min-height: 0;
    display: flex; flex-direction: column;
    position: relative;
    overflow: hidden;
  }

  .term-host {
    flex: 1;
    min-height: 0;
    padding: 8px 4px 4px 8px;
    /* Inherit `--bg-0` from `.terminal-col` so header + body share
       the same surface as every other app kind in the solo. */
    overflow: hidden;
  }
  /* xterm.js draws into a sized child div — make it fill the host. */
  .term-host :global(.xterm) {
    height: 100%;
    width: 100%;
  }
  .term-host :global(.xterm-viewport) {
    background: transparent !important;
  }

  .term-error {
    position: absolute;
    bottom: 0; left: 0; right: 0;
    padding: 6px 12px;
    background: rgba(229, 113, 92, 0.12);
    border-top: 1px solid rgba(229, 113, 92, 0.32);
    color: var(--error);
    font-size: 11px;
    font-family: 'JetBrains Mono', ui-monospace, monospace;
    pointer-events: none;
  }
</style>
