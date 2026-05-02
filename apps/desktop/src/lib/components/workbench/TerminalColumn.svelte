<script lang="ts">
  /*
   * TerminalColumn — workbench column hosting an xterm.js terminal
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
  import { slide } from 'svelte/transition';
  import { cubicOut } from 'svelte/easing';
  import { invoke } from '@tauri-apps/api/core';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import { Terminal } from '@xterm/xterm';
  import { FitAddon } from '@xterm/addon-fit';
  import { WebLinksAddon } from '@xterm/addon-web-links';
  import '@xterm/xterm/css/xterm.css';
  import {
    layoutState,
    startResizeById,
    activeInstances
  } from '$lib/state/layout.svelte';
  import ColumnControls from '$lib/components/workbench/ColumnControls.svelte';

  interface Props {
    instanceId: string;
    /** Optional initial cwd. Falls back to $HOME inside the shell. */
    cwd?: string | null;
  }
  let { instanceId, cwd = null }: Props = $props();

  const inst = $derived(activeInstances().find((i) => i.id === instanceId));
  const order = $derived(activeInstances().findIndex((i) => i.id === instanceId));

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

    term = new Terminal({
      fontFamily: '"JetBrains Mono", "SF Mono", ui-monospace, monospace',
      fontSize: 12.5,
      lineHeight: 1.25,
      cursorBlink: true,
      scrollback: 5000,
      allowProposedApi: true,
      convertEol: false,
      // Match Forgehold's v5 palette so the terminal feels native to
      // the column it sits in (not a chunky "external app embed").
      theme: {
        background: '#0A0C10',
        foreground: '#E9EAEE',
        cursor: '#D9772E',
        cursorAccent: '#0A0C10',
        selectionBackground: 'rgba(217, 119, 46, 0.32)',
        black: '#1A1E24',
        red: '#E5715C',
        green: '#65D396',
        yellow: '#E0B33A',
        blue: '#6FA9F2',
        magenta: '#B289F2',
        cyan: '#7FD9D9',
        white: '#C8D0DC',
        brightBlack: '#494D55',
        brightRed: '#FF8B72',
        brightGreen: '#86E5AA',
        brightYellow: '#FFCB5C',
        brightBlue: '#92BFFF',
        brightMagenta: '#CBA9FF',
        brightCyan: '#A1ECEC',
        brightWhite: '#E9EAEE'
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
          opts: { cwd, cols, rows }
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
  class="wb-column terminal-col"
  class:wb-column--maximized={layoutState.maximizedInstanceId === instanceId}
  data-instance-id={instanceId}
  data-kind="terminal"
  style="order: {order}; flex: 0 0 {inst?.width ?? 560}px"
  transition:slide={{ duration: 240, axis: 'x', easing: cubicOut }}
>
  <ColumnControls {instanceId} kind="terminal" />
  <div class="wb-col-resize" class:snap-flash={layoutState.snapFlashInstanceId === instanceId} role="separator" aria-orientation="vertical" onpointerdown={(e) => startResizeById(instanceId, e)}></div>

  <header class="term-brand">
    <span class="brand-icon term-mark" aria-hidden="true">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">
        <polyline points="4 17 10 11 4 5"/>
        <line x1="12" y1="19" x2="20" y2="19"/>
      </svg>
    </span>
    <span class="brand-word">Terminal</span>
    {#if inst?.name}<span class="bench-name mono" title="Bench id">{inst.name}</span>{/if}
    {#if exited}
      <span class="state-tag state-tag--exited">exited</span>
    {:else if busy}
      <span class="state-tag state-tag--busy">opening…</span>
    {:else if error}
      <span class="state-tag state-tag--error" title={error}>error</span>
    {:else}
      <span class="state-tag state-tag--live">live</span>
    {/if}
    <button
      class="term-action"
      onclick={() => void restart()}
      title="Kill and restart shell"
      aria-label="Restart"
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
        <path d="M3 12a9 9 0 1 0 3-6.7M3 4v5h5"/>
      </svg>
    </button>
  </header>

  <div class="term-host" bind:this={host}></div>

  {#if error}
    <div class="term-error" role="alert">{error}</div>
  {/if}
</section>

<style>
  .terminal-col {
    background: var(--bg-0);
    display: flex; flex-direction: column;
    min-height: 0;
    position: relative;
  }

  .term-brand {
    padding: 12px 14px 8px;
    display: flex; align-items: center; gap: 10px;
    border-bottom: 1px solid var(--border-neutral);
    flex-shrink: 0;
    height: 48px;
    box-sizing: border-box;
  }
  .brand-icon {
    width: 22px; height: 22px;
    display: inline-flex; align-items: center; justify-content: center;
    color: var(--accent-bright);
    background: var(--accent-soft);
    border-radius: 5px;
    flex-shrink: 0;
  }
  .brand-icon svg { width: 13px; height: 13px; }
  .brand-word {
    font-size: 14px; font-weight: 600;
    color: var(--text-0);
    letter-spacing: -0.01em;
  }
  .bench-name {
    font-size: 11px;
    color: var(--text-2);
    padding: 1px 6px;
    border: 1px solid var(--border-neutral);
    border-radius: 4px;
  }
  .state-tag {
    margin-left: auto;
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.04em;
    padding: 2px 7px;
    border-radius: 4px;
    text-transform: uppercase;
  }
  .state-tag--live  { color: var(--success); background: rgba(101, 211, 150, 0.10); }
  .state-tag--busy  { color: var(--text-2);  background: var(--bg-2); }
  .state-tag--error { color: var(--error);   background: rgba(229, 113, 92, 0.12); }
  .state-tag--exited { color: var(--text-mute); background: var(--bg-2); }
  .state-tag:not(:last-child) { margin-left: auto; }
  .term-action {
    width: 26px; height: 26px;
    border-radius: 5px;
    display: inline-flex; align-items: center; justify-content: center;
    color: var(--text-2);
    background: transparent;
    border: 0;
    cursor: pointer;
    transition: all 120ms;
  }
  .term-action:hover { color: var(--accent-bright); background: var(--accent-soft); }
  .term-action svg { width: 13px; height: 13px; }

  .term-host {
    flex: 1;
    min-height: 0;
    padding: 8px 4px 4px 8px;
    background: #0A0C10;
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
