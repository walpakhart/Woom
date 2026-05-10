<script lang="ts">
  /*
   * TerminalColumn — solo app hosting an xterm.js terminal
   * bound to a real PTY-backed shell on the Rust side.
   *
   * Lifecycle:
   *   onMount   → invoke `terminal_spawn` → got id → subscribe to
   *               `terminal:output:<id>` → mount xterm into the DOM
   *   onDestroy → invoke `terminal_kill` (drops the PTY + child shell)
   *
   * Resize: a ResizeObserver on the host element calls `fit.fit()`
   * whenever the column resizes, then forwards new (cols, rows) to
   * `terminal_resize` so the kernel's TIOCSWINSZ matches.
   *
   * Phase-2 MCP write: when a Claude / Cursor agent calls
   * `terminal.write(id, data)` via MCP, the bytes go through the same
   * master fd this component reads from — the user sees keystrokes
   * appear live without any extra plumbing.
   */
  import { onMount, onDestroy } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import { Terminal } from '@xterm/xterm';
  import { FitAddon } from '@xterm/addon-fit';
  import { WebLinksAddon } from '@xterm/addon-web-links';
  import '@xterm/xterm/css/xterm.css';
  import { sessionsState } from '$lib/state/sessions.svelte';

  interface Props {
    instanceId: string;
    /** Optional initial cwd. Falls back to $HOME inside the shell. */
    cwd?: string | null;
  }
  let { instanceId, cwd = null }: Props = $props();

  /**
   * Sessions that link THIS terminal (`linkedTerminalInstanceId === instanceId`).
   * Drives the "Linked: <session>" pill in the header so the user knows
   * which agent will land here when it calls `terminal_run`.
   *
   * Auto-link convention surfaced to the user: if any of those sessions
   * also link an editor, the editor's repoPath wins as the spawn cwd
   * (over the explicit `cwd` prop). Lets the user "make a chat-bound
   * terminal that follows the chat's project" with one click in the
   * AgentColumn.
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
  let sessionId = $state<string | null>(null);
  let unlistenOutput: UnlistenFn | null = null;
  let unlistenExit: UnlistenFn | null = null;
  let unlistenError: UnlistenFn | null = null;
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
    if (!fit || !term || !sessionId) return;
    try {
      fit.fit();
    } catch {
      return;
    }
    const cols = term.cols;
    const rows = term.rows;
    void invoke('terminal_resize', { id: sessionId, cols, rows }).catch(() => {});
  }

  onMount(() => {
    if (!host) return;

    /* Pull surface + text + accent from the live theme so the column
     * matches the inbox/canvas columns visually — Github/Jira sit on
     * `.wb-column.inbox` (= --bg-1) and Canvas's head + surface are
     * also --bg-1, so that's the "default chrome" of the solo.
     * Header, host padding, and the xterm canvas all settle on
     * `--bg-1` so the terminal reads as part of that family instead
     * of the deeper `--bg-0` used by Editor/Agent/Sentry. */
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
      // Use the live theme's surface + foreground so the column
      // doesn't look like an embed of a different app. Per-source
      // palette below stays a fixed warm/blue mix that reads well
      // on every theme — those are content colours from `ls`,
      // `git status`, etc., not chrome.
      theme: {
        background: bg1,
        foreground: text0,
        cursor: accentBright,
        cursorAccent: bg1,
        selectionBackground: 'rgba(204, 120, 92, 0.32)',
        black: '#1A1410',
        red: '#D4664A',
        green: '#6FAE88',
        yellow: '#D99540',
        blue: '#6FA9F2',
        magenta: '#B289F2',
        cyan: '#7FD9D9',
        white: '#C8C0AE',
        brightBlack: '#5E5648',
        brightRed: '#E48C70',
        brightGreen: '#8FCAA0',
        brightYellow: '#E5B574',
        brightBlue: '#92BFFF',
        brightMagenta: '#CBA9FF',
        brightCyan: '#A1ECEC',
        brightWhite: '#FAEEE0'
      }
    });
    fit = new FitAddon();
    term.loadAddon(fit);
    term.loadAddon(new WebLinksAddon());
    term.open(host);

    // Forward keystrokes to the PTY. Multi-byte input (paste, dead
    // keys, IME) all goes through `onData` as a single string, which
    // we re-encode to base64 before invoke.
    term.onData((data) => {
      if (!sessionId) return;
      void invoke('terminal_write', { id: sessionId, data: toB64(data) }).catch(() => {});
    });

    // Spawn the PTY + wire the output stream.
    (async () => {
      try {
        // A first fit BEFORE spawn lets us pass the actual terminal
        // size to portable-pty so the shell starts with the right
        // $COLUMNS / $LINES — otherwise we get a 80x24 default that
        // wraps awkwardly until the first resize event lands.
        try { fit.fit(); } catch {}
        const cols = term.cols;
        const rows = term.rows;
        const result = await invoke<{ id: string }>('terminal_spawn', {
          opts: { cwd: autoLinkedCwd ?? cwd, cols, rows, name: null }
        });
        sessionId = result.id;

        unlistenOutput = await listen<string>(`terminal:output:${sessionId}`, (e) => {
          writeChunk(e.payload);
        });
        unlistenExit = await listen<null>(`terminal:exit:${sessionId}`, () => {
          exited = true;
          term?.write('\r\n\x1b[2m[shell exited]\x1b[0m\r\n');
        });
        unlistenError = await listen<string>(`terminal:error:${sessionId}`, (e) => {
          error = e.payload;
        });
        busy = false;
      } catch (e) {
        error = typeof e === 'string' ? e : String(e);
        busy = false;
      }
    })();

    // Resize observer pushes new dimensions to the PTY — debounced
    // by browser frame timing already (ResizeObserver fires once
    // per layout). Using one observer over rAF avoids a jittery
    // resize during column drag.
    resizeObserver = new ResizeObserver(() => fitAndPush());
    resizeObserver.observe(host);
  });

  onDestroy(() => {
    resizeObserver?.disconnect();
    unlistenOutput?.();
    unlistenExit?.();
    unlistenError?.();
    if (sessionId) {
      void invoke('terminal_kill', { id: sessionId }).catch(() => {});
    }
    term?.dispose();
    term = null;
    fit = null;
  });

  /**
   * Soft-restart: kill the current PTY, spawn a fresh one. Useful
   * when the user wants a clean shell after a crashed process or
   * after editing $PATH and wanting it picked up.
   */
  async function restart() {
    if (sessionId) {
      try { await invoke('terminal_kill', { id: sessionId }); } catch {}
    }
    sessionId = null;
    error = null;
    exited = false;
    busy = true;
    term?.clear();
    try {
      const cols = term?.cols ?? 120;
      const rows = term?.rows ?? 32;
      const result = await invoke<{ id: string }>('terminal_spawn', {
        opts: { cwd, cols, rows }
      });
      sessionId = result.id;
      unlistenOutput?.(); unlistenExit?.(); unlistenError?.();
      unlistenOutput = await listen<string>(`terminal:output:${sessionId}`, (e) => writeChunk(e.payload));
      unlistenExit = await listen<null>(`terminal:exit:${sessionId}`, () => {
        exited = true;
        term?.write('\r\n\x1b[2m[shell exited]\x1b[0m\r\n');
      });
      unlistenError = await listen<string>(`terminal:error:${sessionId}`, (e) => { error = e.payload; });
      busy = false;
    } catch (e) {
      error = typeof e === 'string' ? e : String(e);
      busy = false;
    }
  }
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
       the same surface as every other column kind in the solo. */
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
