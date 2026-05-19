<script lang="ts">
  /* Preview pane — right-side panel inside Claude/Cursor solo for
     long-running processes the agent (or user) spawns. Mirrors the
     `terminal` solo's UX but for background tasks that DON'T need a
     full PTY: dev servers, build watchers, test loops.

     Layout:
       ┌──────────────────────────────────────────────┐
       │ task strip — horizontal scroll, hidden bar   │
       │  [▶ pnpm dev (5173)] [▶ pnpm test] [✕ ...]  │ ← chips
       ├──────────────────────────────────────────────┤
       │ active task detail:                          │
       │  • header (label, cmd, pid, status, URL pill)│
       │  • log tail (most recent N lines)            │
       │  • toolbar: [kill] [restart] [open url] [+]  │
       └──────────────────────────────────────────────┘

     Task strip is horizontally scrollable; native scrollbar hidden via
     ::-webkit-scrollbar / scrollbar-width: none. Wheel + drag scroll
     to keep the surface tactile. */

  import { fly } from 'svelte/transition';
  import { cubicOut } from 'svelte/easing';
  import {
    bgTasksState,
    spawnBgTask,
    killBgTask,
    fetchBgLogs,
    type BgTask,
    type BgStatus
  } from '$lib/state/bgTasks.svelte';
  import { sessionsState } from '$lib/state/sessions.svelte';
  import { notify } from '$lib/state/toaster.svelte';

  interface Props {
    kind: 'claude' | 'cursor';
    instanceId: string;
    /** Collapse callback — parent (AgentApp) owns the open/closed
     *  state so it can also render the 44px rail when closed. */
    onCollapse: () => void;
  }
  let p: Props = $props();

  const tasks = $derived(bgTasksState.tasks);
  const activeTask = $derived(
    tasks.find((t) => t.id === bgTasksState.activeId) ?? tasks[0] ?? null
  );

  /* Quick-spawn form state. The "+ new" chip opens an inline composer
     instead of a modal — keeps focus inside the pane. cwd defaults to
     the active session's cwd (which is the worktree path for sessions
     that have one) so a /preview without args runs in the right tree. */
  let composerOpen = $state(false);
  let composerCmd = $state('');

  const activeSession = $derived(
    sessionsState.list.find((s) => s.id === sessionsState.activeIds[p.kind])
  );
  const defaultCwd = $derived(
    activeSession?.worktreePath ?? activeSession?.cwd ?? ''
  );

  async function handleSpawn() {
    const cmd = composerCmd.trim();
    if (!cmd) return;
    if (!defaultCwd) {
      notify({ kind: 'error', title: 'No cwd', body: 'Pick a folder for the active session first.' });
      return;
    }
    const task = await spawnBgTask({
      cmd,
      cwd: defaultCwd,
      session_id: activeSession?.id
    });
    if (task) {
      composerCmd = '';
      composerOpen = false;
    }
  }

  async function handleKill(id: string) {
    /* No confirm dialog — Tauri webview blocks native `window.confirm()`
     *  (returns undefined synchronously). Killing a tracked bg task is
     *  cheap to undo via `restart` anyway, so a single click is fine. */
    await killBgTask(id);
  }

  async function handleRestart(t: BgTask) {
    await killBgTask(t.id);
    /* Re-spawn with same cmd/cwd. The new task gets a fresh id; old one
     *  sticks around in the list with status=Killed until we ship a
     *  remove action. */
    await spawnBgTask({
      cmd: t.cmd,
      cwd: t.cwd,
      label: t.label,
      session_id: t.session_id ?? undefined
    });
  }

  function statusLabel(s: BgStatus): string {
    switch (s.kind) {
      case 'running': return 'running';
      case 'exited': return s.code === 0 ? 'done' : `exit ${s.code}`;
      case 'killed': return 'killed';
    }
  }

  function statusTone(s: BgStatus): 'live' | 'ok' | 'warn' | 'dim' {
    switch (s.kind) {
      case 'running': return 'live';
      case 'exited': return s.code === 0 ? 'ok' : 'warn';
      case 'killed': return 'dim';
    }
  }

  function primaryUrl(t: BgTask): string | null {
    return t.detected_urls[0] ?? null;
  }

  async function openUrl(url: string) {
    try {
      const { openUrl: openInOs } = await import('@tauri-apps/plugin-opener');
      await openInOs(url);
    } catch {
      window.open(url, '_blank');
    }
  }

  /* Open in standalone Tauri WebviewWindow — full-fidelity preview
   *  (real cursor / scroll / DevTools) without the iframe sandbox
   *  limitations. Window label = `preview-<task_id>`, so reopening
   *  focuses the existing window instead of spawning duplicates. */
  async function openInWindow(task: BgTask) {
    const url = primaryUrl(task);
    if (!url) return;
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      await invoke('preview_open_window', {
        taskId: task.id,
        url,
        title: `${task.label} · ${url}`
      });
    } catch (e) {
      notify({ kind: 'error', title: 'Preview window', body: String(e) });
    }
  }

  /* Horizontal wheel scroll on the task strip — touch trackpads on Mac
     already get this for free, but a vertical mouse wheel needs to
     translate to horizontal here. Native scrollbar hidden via CSS. */
  let stripEl: HTMLElement | null = $state(null);
  function onStripWheel(e: WheelEvent) {
    if (!stripEl) return;
    if (Math.abs(e.deltaY) > Math.abs(e.deltaX)) {
      stripEl.scrollLeft += e.deltaY;
      e.preventDefault();
    }
  }

  /* Log-tail rendering — derive from the per-task lines buffer, with a
     hard cap so DOM stays sane. Wrap in `$derived` so changes to
     `lines[active.id]` trigger re-render without a manual subscribe. */
  const tail = $derived.by(() => {
    if (!activeTask) return [];
    const lines = bgTasksState.lines[activeTask.id] ?? activeTask.recent_lines ?? [];
    return lines.slice(-200);
  });

  /* On task switch, optionally pull recent file contents for the new
     active task — only if we have no in-memory lines yet. Avoids
     blank logs after app restart. */
  let lastHydratedId = $state<string | null>(null);
  $effect(() => {
    const id = activeTask?.id ?? null;
    if (!id) return;
    if (lastHydratedId === id) return;
    if ((bgTasksState.lines[id]?.length ?? 0) > 0) {
      lastHydratedId = id;
      return;
    }
    lastHydratedId = id;
    void fetchBgLogs(id, 200).then((raw) => {
      if (!raw) return;
      const lines = raw.split('\n').filter((l) => l.length > 0).map((l, i) => ({
        id,
        at: Date.now() - (raw.length - i),
        stream: 'stdout' as const,
        line: l
      }));
      // Only seed if still empty — guard against races with live stream.
      if ((bgTasksState.lines[id]?.length ?? 0) === 0) {
        bgTasksState.lines[id] = lines;
      }
    });
  });

  function ageLabel(startedAt: number, now = Date.now()): string {
    const s = Math.max(0, Math.floor((now - startedAt) / 1000));
    if (s < 60) return `${s}s`;
    const m = Math.floor(s / 60);
    if (m < 60) return `${m}m`;
    const h = Math.floor(m / 60);
    return `${h}h${m % 60}m`;
  }

  /* 1s tick for the elapsed counters — matches existing ChatHeader pattern.
     Cheap; lives only while pane is mounted. */
  let now = $state(Date.now());
  $effect(() => {
    const t = setInterval(() => { now = Date.now(); }, 1000);
    return () => clearInterval(t);
  });

  /* Tab toggle for the detail body — `logs` (text tail) vs `preview`
     (embedded webview of the detected URL). Per-task so switching
     tasks remembers each one's preferred tab. Preview tab auto-
     activates the first time a URL gets detected for a task the user
     is currently viewing — feels like "the preview just lit up". */
  type DetailTab = 'logs' | 'preview';
  let detailTabByTask = $state<Record<string, DetailTab>>({});

  function detailTab(id: string): DetailTab {
    return detailTabByTask[id] ?? 'logs';
  }
  function setDetailTab(id: string, tab: DetailTab) {
    detailTabByTask[id] = tab;
  }

  /* NOTE: previously we auto-flipped to the `preview` tab on first URL
   *  detection. Removed — auto-loading an iframe at `http://localhost:PORT`
   *  inside Tauri's WKWebView competes with main UI for the renderer
   *  thread on macOS (single WebContent process for same-app webviews).
   *  Vite/Next dev servers run HEAVY JS on first paint (HMR client,
   *  module graph fetch); pairing that with a high-rate IPC event
   *  stream from `bg:line:<id>` froze the whole app. User now chooses
   *  explicitly: click `preview` tab for inline iframe, or `pop out ↗`
   *  for a separate Tauri window (different renderer process). */

  /** `bust` query param so a manual "reload" button forces the iframe
   *  to re-fetch without changing the src URL meaningfully. */
  let iframeBustByTask = $state<Record<string, number>>({});
  function reloadIframe(id: string) {
    iframeBustByTask[id] = Date.now();
  }
  function iframeSrc(task: BgTask): string | null {
    const url = primaryUrl(task);
    if (!url) return null;
    const bust = iframeBustByTask[task.id];
    if (!bust) return url;
    const sep = url.includes('?') ? '&' : '?';
    return `${url}${sep}_woom_r=${bust}`;
  }
</script>

<aside class="pv app-pane" in:fly={{ x: 24, duration: 220, easing: cubicOut }}>
  <header class="app-pane-head pv-head">
    <button
      class="pv-collapse-btn"
      onclick={p.onCollapse}
      title="Collapse preview pane"
      aria-label="Collapse preview pane"
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <path d="M10 6l6 6-6 6"/>
      </svg>
    </button>
    <span class="app-pane-head-h">Preview</span>
    <span class="app-pane-head-meta mono">{tasks.length}</span>
  </header>

  <div
    class="pv-strip"
    bind:this={stripEl}
    onwheel={onStripWheel}
    role="tablist"
    aria-label="Background tasks"
  >
    {#each tasks as t (t.id)}
      {@const isActive = activeTask?.id === t.id}
      <button
        class="pv-chip"
        class:active={isActive}
        data-tone={statusTone(t.status)}
        role="tab"
        aria-selected={isActive}
        onclick={() => (bgTasksState.activeId = t.id)}
        title="{t.cmd} · cwd={t.cwd}"
      >
        <span class="pv-chip-dot" aria-hidden="true"></span>
        <span class="pv-chip-label">{t.label}</span>
        {#if primaryUrl(t)}
          {@const port = t.detected_ports[0]}
          <span class="pv-chip-port mono">:{port ?? '—'}</span>
        {/if}
      </button>
    {/each}
    <button
      class="pv-chip pv-chip--new"
      onclick={() => (composerOpen = !composerOpen)}
      aria-label="New background task"
      title="Spawn a new background task"
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M12 5v14M5 12h14"/></svg>
    </button>
  </div>

  {#if composerOpen}
    <div class="pv-composer">
      <input
        class="pv-composer-input mono"
        bind:value={composerCmd}
        placeholder="pnpm dev"
        onkeydown={(e) => { if (e.key === 'Enter') { void handleSpawn(); } if (e.key === 'Escape') { composerOpen = false; composerCmd = ''; } }}
      />
      <div class="pv-composer-cwd mono" title={defaultCwd}>cwd: {defaultCwd || '—'}</div>
      <div class="pv-composer-row">
        <button class="pv-btn" onclick={() => (composerOpen = false)}>cancel</button>
        <button class="pv-btn pv-btn--primary" disabled={!composerCmd.trim() || !defaultCwd} onclick={handleSpawn}>spawn</button>
      </div>
    </div>
  {/if}

  {#if activeTask}
    <section class="pv-detail">
      <div class="pv-detail-head">
        <div class="pv-detail-title">
          <span class="pv-detail-status pv-detail-status--{statusTone(activeTask.status)}">
            {statusLabel(activeTask.status)}
          </span>
          <span class="pv-detail-label">{activeTask.label}</span>
          {#if activeTask.pid}
            <span class="pv-detail-pid mono">pid {activeTask.pid}</span>
          {/if}
          <span class="pv-detail-age mono" title="Started {new Date(activeTask.started_at).toLocaleString()}">
            {ageLabel(activeTask.started_at, now)}
          </span>
        </div>
        <div class="pv-detail-cmd mono" title={activeTask.cmd}>{activeTask.cmd}</div>
        {#if primaryUrl(activeTask)}
          <div class="pv-detail-urls">
            {#each activeTask.detected_urls as u (u)}
              <button class="pv-url-chip mono" onclick={() => openUrl(u)} title="Open in browser">{u}</button>
            {/each}
          </div>
        {/if}
        <div class="pv-detail-tools">
          {#if activeTask.status.kind === 'running'}
            <button class="pv-btn" onclick={() => handleKill(activeTask.id)} title="SIGKILL the process">kill</button>
          {:else}
            <button class="pv-btn pv-btn--primary" onclick={() => handleRestart(activeTask)} title="Re-spawn with same cmd/cwd">restart</button>
          {/if}
          <div class="pv-tabs" role="tablist" aria-label="Detail view">
            <button
              class="pv-tab"
              class:active={detailTab(activeTask.id) === 'logs'}
              role="tab"
              aria-selected={detailTab(activeTask.id) === 'logs'}
              onclick={() => setDetailTab(activeTask.id, 'logs')}
            >logs</button>
            <button
              class="pv-tab"
              class:active={detailTab(activeTask.id) === 'preview'}
              role="tab"
              aria-selected={detailTab(activeTask.id) === 'preview'}
              disabled={!primaryUrl(activeTask)}
              title={primaryUrl(activeTask) ? 'Embed the detected URL' : 'No URL detected yet'}
              onclick={() => setDetailTab(activeTask.id, 'preview')}
            >preview</button>
            {#if detailTab(activeTask.id) === 'preview' && primaryUrl(activeTask)}
              <button
                class="pv-tab pv-tab--icon"
                onclick={() => reloadIframe(activeTask.id)}
                title="Reload embedded preview"
                aria-label="Reload preview"
              >
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                  <path d="M21 12a9 9 0 1 1-3-6.7M21 4v5h-5"/>
                </svg>
              </button>
            {/if}
          </div>
        </div>
      </div>
      {#if detailTab(activeTask.id) === 'preview' && primaryUrl(activeTask)}
        {@const src = iframeSrc(activeTask)}
        <div class="pv-webview">
          <!-- Sandboxed iframe — most localhost dev servers (Vite,
               webpack, Next, Vercel dev) don't set X-Frame-Options on
               localhost, so embedding works. Sandbox grants
               same-origin + scripts + forms (enough for typical SPA
               flows) without letting the page navigate top frame or
               run popups. If the dev server's CSP blocks the embed,
               the user falls back to the URL chip "open in browser". -->
          {#if src}
            <iframe
              title="Embedded preview for {activeTask.label}"
              src={src}
              sandbox="allow-same-origin allow-scripts allow-forms allow-popups allow-modals"
              referrerpolicy="no-referrer"
            ></iframe>
          {/if}
          <div class="pv-webview-foot mono">
            <span>{primaryUrl(activeTask)}</span>
            <button class="pv-link-btn" onclick={() => openInWindow(activeTask)} title="Open in a separate Woom window (real cursor / scroll / DevTools)">pop out ↗</button>
            <button class="pv-link-btn" onclick={() => openUrl(primaryUrl(activeTask) ?? '')} title="Open in your default browser">browser ↗</button>
          </div>
        </div>
      {:else}
        <div class="pv-log" role="log" aria-live="polite">
          {#each tail as l, i (l.at + '-' + i)}
            <div class="pv-log-line" data-stream={l.stream}>{l.line}</div>
          {:else}
            <div class="pv-log-empty mono">No output yet…</div>
          {/each}
        </div>
      {/if}
    </section>
  {:else}
    <section class="pv-empty">
      <div class="pv-empty-glyph" aria-hidden="true">
        <svg viewBox="0 0 64 64"><rect x="8" y="14" width="48" height="36" rx="6"/><path d="M14 24h36M14 32h22M14 40h28"/></svg>
      </div>
      <p class="pv-empty-h">No tasks yet</p>
      <p class="pv-empty-sub">Spawn a dev server, build watcher, or test loop — output streams here. Logs survive app restarts; the agent can react to lines via the bg_wait_line MCP tool.</p>
      <button class="pv-btn pv-btn--primary" onclick={() => (composerOpen = true)}>+ New task</button>
    </section>
  {/if}
</aside>

<style>
  .pv {
    display: flex; flex-direction: column;
    overflow: hidden;
    height: 100%;
    min-width: 0;
  }

  /* Head bar: uses canonical .app-pane-head + .app-pane-head-h +
     .app-pane-head-meta from `_shared/app.css` so font + size match
     every other solo's pane header (Geist 22px / mono badge). Local
     `.pv-head` only tweaks the leading chevron's spacing. */
  .pv-head {
    flex-shrink: 0;
    gap: 10px;
  }
  .pv-collapse-btn {
    width: 24px; height: 24px;
    display: grid; place-items: center;
    background: transparent; border: 0; padding: 0;
    color: var(--text-mute);
    border-radius: 6px;
    cursor: pointer;
    transition: color 120ms, background 120ms;
  }
  .pv-collapse-btn:hover { color: var(--text-0); background: var(--bg-2); }
  .pv-collapse-btn svg { width: 14px; height: 14px; }

  /* Horizontal task strip — hidden scrollbar (per user spec). */
  .pv-strip {
    display: flex; align-items: center;
    gap: 6px;
    padding: 8px 10px;
    overflow-x: auto;
    overflow-y: hidden;
    flex-shrink: 0;
    border-bottom: 1px solid var(--border);
    scrollbar-width: none;
    -ms-overflow-style: none;
  }
  .pv-strip::-webkit-scrollbar { display: none; width: 0; height: 0; }

  .pv-chip {
    display: inline-flex; align-items: center;
    gap: 6px;
    padding: 5px 9px;
    border-radius: 999px;
    border: 1px solid var(--border-neutral);
    background: var(--bg-1);
    color: var(--text-1);
    cursor: pointer;
    font-size: 11.5px;
    white-space: nowrap;
    flex-shrink: 0;
    transition: background 120ms, border-color 120ms, color 120ms, transform 120ms;
  }
  .pv-chip:hover {
    background: var(--bg-2);
    border-color: var(--border-hi);
  }
  .pv-chip.active {
    border-color: color-mix(in srgb, var(--accent) 60%, var(--border-hi));
    background: color-mix(in srgb, var(--accent) 14%, var(--bg-1));
    color: var(--text-0);
  }
  .pv-chip-dot {
    width: 7px; height: 7px;
    border-radius: 50%;
    background: var(--text-mute);
    transition: background 120ms, box-shadow 120ms;
  }
  .pv-chip[data-tone="live"] .pv-chip-dot {
    background: #66d39a;
    box-shadow: 0 0 0 2px rgba(102, 211, 154, 0.18);
    animation: pv-dot-pulse 1.4s ease-in-out infinite;
  }
  .pv-chip[data-tone="ok"] .pv-chip-dot { background: #6ec3a4; }
  .pv-chip[data-tone="warn"] .pv-chip-dot { background: #e0b16c; }
  .pv-chip[data-tone="dim"] .pv-chip-dot { background: #5e6566; }
  @keyframes pv-dot-pulse {
    0%, 100% { box-shadow: 0 0 0 2px rgba(102, 211, 154, 0.18); }
    50%      { box-shadow: 0 0 0 4px rgba(102, 211, 154, 0.32); }
  }
  @media (prefers-reduced-motion: reduce) {
    .pv-chip[data-tone="live"] .pv-chip-dot { animation: none; }
  }
  .pv-chip-label { font-weight: 500; }
  .pv-chip-port { color: var(--text-mute); font-size: 10.5px; }
  .pv-chip--new {
    width: 26px; height: 26px;
    padding: 0;
    display: grid; place-items: center;
    border-style: dashed;
    color: var(--text-mute);
  }
  .pv-chip--new svg { width: 12px; height: 12px; }

  /* Inline composer — opens below the strip. */
  .pv-composer {
    padding: 10px;
    border-bottom: 1px solid var(--border);
    background: var(--bg-1);
  }
  .pv-composer-input {
    width: 100%;
    padding: 8px 10px;
    border-radius: 6px;
    border: 1px solid var(--border);
    background: var(--bg-2);
    color: var(--text-0);
    font-size: 12.5px;
    margin-bottom: 6px;
  }
  .pv-composer-input:focus {
    outline: 2px solid var(--accent);
    outline-offset: -1px;
  }
  .pv-composer-cwd {
    font-size: 10.5px; color: var(--text-mute);
    margin-bottom: 8px;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .pv-composer-row {
    display: flex; justify-content: flex-end; gap: 6px;
  }

  /* Detail pane — header + log tail. */
  .pv-detail {
    flex: 1 1 auto;
    display: flex; flex-direction: column;
    min-height: 0;
  }
  .pv-detail-head {
    padding: 10px 12px;
    border-bottom: 1px solid var(--border);
    flex-shrink: 0;
  }
  .pv-detail-title {
    display: flex; align-items: center;
    gap: 8px;
    margin-bottom: 6px;
  }
  .pv-detail-status {
    font-size: 9.5px; font-weight: 700;
    letter-spacing: 0.10em;
    text-transform: uppercase;
    padding: 2px 7px;
    border-radius: 4px;
    border: 1px solid transparent;
  }
  .pv-detail-status--live {
    color: #6ec3a4;
    border-color: color-mix(in srgb, #6ec3a4 35%, transparent);
    background: color-mix(in srgb, #6ec3a4 10%, transparent);
  }
  .pv-detail-status--ok {
    color: var(--text-1);
    border-color: var(--border-neutral);
    background: var(--bg-2);
  }
  .pv-detail-status--warn {
    color: #e0b16c;
    border-color: color-mix(in srgb, #e0b16c 35%, transparent);
    background: color-mix(in srgb, #e0b16c 10%, transparent);
  }
  .pv-detail-status--dim {
    color: var(--text-mute);
    border-color: var(--border-neutral);
    background: var(--bg-2);
  }
  .pv-detail-label { font-weight: 500; color: var(--text-0); }
  .pv-detail-pid { font-size: 10.5px; color: var(--text-mute); }
  .pv-detail-age { font-size: 10.5px; color: var(--text-mute); margin-left: auto; }
  .pv-detail-cmd {
    font-size: 11px;
    color: var(--text-2);
    padding: 4px 6px;
    border-radius: 4px;
    background: var(--bg-2);
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .pv-detail-urls {
    display: flex; flex-wrap: wrap; gap: 4px;
    margin-top: 6px;
  }
  .pv-url-chip {
    font-size: 10.5px;
    padding: 3px 8px;
    border-radius: 4px;
    border: 1px solid color-mix(in srgb, var(--accent) 40%, var(--border));
    background: color-mix(in srgb, var(--accent) 10%, var(--bg-2));
    color: var(--accent-bright);
    cursor: pointer;
    text-decoration: none;
  }
  .pv-url-chip:hover {
    background: color-mix(in srgb, var(--accent) 20%, var(--bg-2));
  }
  .pv-detail-tools {
    display: flex; gap: 6px;
    margin-top: 8px;
  }

  /* Tab toggle inside the detail head — logs ↔ preview. Lives on the
     right side of the tools row. Reload glyph appears only when the
     preview tab is active and a URL is detected. */
  .pv-tabs {
    display: inline-flex;
    margin-left: auto;
    background: var(--bg-2);
    border-radius: 6px;
    padding: 2px;
    gap: 2px;
  }
  .pv-tab {
    padding: 3px 9px;
    border: 0;
    background: transparent;
    color: var(--text-mute);
    font-size: 10.5px;
    font-weight: 500;
    letter-spacing: 0.02em;
    border-radius: 4px;
    cursor: pointer;
    transition: background 120ms, color 120ms;
  }
  .pv-tab:hover:not(:disabled) { color: var(--text-1); }
  .pv-tab.active {
    background: var(--bg-0);
    color: var(--text-0);
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.18);
  }
  .pv-tab:disabled { opacity: 0.4; cursor: not-allowed; }
  .pv-tab--icon { padding: 3px 6px; }
  .pv-tab--icon svg { width: 11px; height: 11px; vertical-align: -1px; }

  /* Embedded webview — iframe pointed at the detected localhost URL.
     Foot strip shows the current src + an OS-browser fallback button. */
  .pv-webview {
    flex: 1 1 auto;
    display: flex; flex-direction: column;
    background: var(--bg-2);
    min-height: 0;
  }
  .pv-webview iframe {
    flex: 1 1 auto;
    width: 100%;
    border: 0;
    background: #fff;
    min-height: 0;
  }
  .pv-webview-foot {
    display: flex; align-items: center;
    gap: 10px;
    padding: 5px 10px;
    border-top: 1px solid var(--border);
    background: var(--bg-1);
    font-size: 10.5px;
    color: var(--text-mute);
  }
  .pv-webview-foot > span {
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
    flex: 1 1 auto;
  }
  .pv-link-btn {
    background: transparent;
    border: 0;
    color: var(--accent-bright);
    cursor: pointer;
    font-size: 10.5px;
    padding: 2px 4px;
  }
  .pv-link-btn:hover { text-decoration: underline; }

  /* Log tail — monospaced, vertical scroll, lines wrap softly. */
  .pv-log {
    flex: 1 1 auto;
    overflow-y: auto;
    padding: 8px 12px;
    background: var(--bg-0);
    font: 11px / 1.55 'JetBrains Mono', ui-monospace, monospace;
    color: var(--text-1);
    min-height: 0;
  }
  .pv-log-line {
    white-space: pre-wrap;
    word-break: break-word;
  }
  .pv-log-line[data-stream="stderr"] { color: #e0b16c; }
  .pv-log-empty { color: var(--text-mute); padding: 4px 0; }

  /* Empty state — fills the pane when no tasks exist. */
  .pv-empty {
    display: flex; flex-direction: column;
    align-items: center; justify-content: center;
    gap: 10px;
    padding: 24px;
    text-align: center;
    flex: 1 1 auto;
    min-height: 0;
  }
  .pv-empty-glyph {
    color: var(--text-mute);
    opacity: 0.5;
  }
  .pv-empty-glyph svg {
    width: 48px; height: 48px;
    fill: none; stroke: currentColor; stroke-width: 2;
    stroke-linecap: round; stroke-linejoin: round;
  }
  .pv-empty-h { color: var(--text-1); font-weight: 500; margin: 0; }
  .pv-empty-sub {
    color: var(--text-mute); font-size: 11.5px; line-height: 1.5;
    max-width: 280px; margin: 0;
  }

  .pv-btn {
    padding: 4px 10px;
    border-radius: 5px;
    background: var(--bg-2);
    border: 1px solid var(--border);
    color: var(--text-1);
    font-size: 11px;
    cursor: pointer;
    transition: background 120ms, border-color 120ms, color 120ms;
  }
  .pv-btn:hover { background: var(--bg-3); border-color: var(--border-hi); color: var(--text-0); }
  .pv-btn:disabled { opacity: 0.5; cursor: not-allowed; }
  .pv-btn--primary {
    background: color-mix(in srgb, var(--accent) 30%, var(--bg-2));
    border-color: color-mix(in srgb, var(--accent) 55%, var(--border));
    color: var(--text-0);
  }
  .pv-btn--primary:hover {
    background: color-mix(in srgb, var(--accent) 40%, var(--bg-2));
  }
</style>
