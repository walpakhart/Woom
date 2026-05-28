<script lang="ts">
  /* ChatHeader — top row of AgentApp center pane.
     Shows the active session title (Geist, large) + a running pulse +
     stop button while a turn is in flight.

     Title rename: click on the title text or its little pencil hint
     to enter inline-edit mode. Enter or blur commits the new name to
     the session via `updateSession`; Esc cancels and reverts. The
     input shares glyph + size with the static span so the row doesn't
     reflow when entering / leaving edit mode. Falls back to "Untitled
     chat" when the user clears the field on save (otherwise the
     SessionsSidebar would render an empty row). */
  import { sessionsState, updateSession, dismissInterrupted } from '$lib/state/sessions.svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { sessionUsageTotals, formatTokens, formatCostUsd } from '$lib/usage';
  import { notify } from '$lib/state/toaster.svelte';
  import { tick, untrack } from 'svelte';
  import {
    sddState,
    openStandaloneView,
    discardSdd,
    showSddCard,
    attachSddToSession,
    type SddWorkspace,
    type SddPhase,
  } from '$lib/state/sdd.svelte';

  type Kind = 'claude' | 'cursor';

  interface Props {
    kind: Kind;
    instanceId: string;
    thinkingStartedAt: Record<string, number | null>;
    thinkingTick: Record<string, number>;
    onStop: () => void;
  }

  let p: Props = $props();

  const sess = $derived(
    sessionsState.list.find((s) => s.id === sessionsState.activeIds[p.kind]) ?? null
  );

  /** Live token + USD totals across every assistant-message usage
   *  snapshot in the current session. Updates once per turn (usage is
   *  stamped at end-of-turn, not on every delta), so the chip doesn't
   *  stutter mid-stream — matches the existing streaming-batch pattern. */
  const budget = $derived(sessionUsageTotals(sess));

  const elapsed = $derived.by(() => {
    const startedAt = sess ? p.thinkingStartedAt[sess.id] ?? null : null;
    if (!startedAt || !sess?.sending) return '';
    void (sess ? p.thinkingTick[sess.id] : 0);
    const ms = Date.now() - startedAt;
    const s = Math.floor(ms / 1000);
    if (s < 60) return `${s}s`;
    const m = Math.floor(s / 60);
    const r = s % 60;
    return `${m}m ${String(r).padStart(2, '0')}s`;
  });

  /* ---------- Rename ---------- */

  /** Which session the user is currently renaming (null = static
   *  title). Tracked by id so a session swap mid-edit auto-cancels
   *  the rename instead of bleeding draft text into a different chat. */
  let editingSessionId = $state<string | null>(null);
  let draftTitle = $state('');
  let inputEl = $state<HTMLInputElement | null>(null);

  async function startRename() {
    if (!sess) return;
    editingSessionId = sess.id;
    draftTitle = sess.title || '';
    /* Wait for Svelte to mount the input, then select-all so a
       single press of Enter / Backspace replaces the whole title. */
    await tick();
    inputEl?.focus();
    inputEl?.select();
  }

  function commitRename() {
    if (!sess || editingSessionId !== sess.id) {
      editingSessionId = null;
      return;
    }
    const trimmed = draftTitle.trim();
    /* Empty input falls back to "Untitled chat" so list rows in the
       sidebar always render something — same as the original empty-
       state placeholder. */
    const next = trimmed || 'Untitled chat';
    if (next !== sess.title) {
      updateSession(sess.id, { title: next });
    }
    editingSessionId = null;
  }

  function cancelRename() {
    editingSessionId = null;
  }

  function onTitleKey(e: KeyboardEvent) {
    if (e.key === 'Enter') {
      e.preventDefault();
      commitRename();
    } else if (e.key === 'Escape') {
      e.preventDefault();
      cancelRename();
    }
  }

  /* If the active session changes while an edit is in flight, drop
     the draft so it doesn't accidentally apply to the new chat. */
  $effect(() => {
    if (!sess) {
      editingSessionId = null;
      return;
    }
    if (editingSessionId && editingSessionId !== sess.id) {
      editingSessionId = null;
    }
  });

  /* ---------- Workspace memory indicator ----------
   * Surface how many long-term-memory rows the current cwd's
   * basename matches. The chip is a soft hint — "there are N
   * memories the agent could pull on this project" — clickable to
   * preview the top 5. Helps the user trust the agent isn't going
   * in blind on a familiar project. Refetched whenever the active
   * session's cwd (or worktree) changes; we cache the last query
   * value to skip redundant Tauri round-trips when re-renders fire
   * for unrelated reasons (sending toggle, title rename, etc.). */
  interface MemoryHit {
    id: number;
    kind: string;
    content: string;
    tags: string;
    created_at: number;
  }
  let memHits = $state<MemoryHit[]>([]);
  let memFetchedFor = $state<string | null>(null);

  const effCwd = $derived(
    sess?.worktreePath ?? sess?.cwd
      ?? (sess?.linkedToEditor && sess?.linkedToEditorInstanceId
        ? sessionsState.editorInstanceState[sess.linkedToEditorInstanceId]?.repoPath
        : null)
      ?? null
  );
  const cwdBasename = $derived.by(() => {
    if (!effCwd) return null;
    const parts = effCwd.split('/').filter((s: string) => s.length > 0);
    return parts[parts.length - 1] ?? null;
  });

  $effect(() => {
    const base = cwdBasename;
    if (!base) {
      untrack(() => {
        memHits = [];
        memFetchedFor = null;
      });
      return;
    }
    if (memFetchedFor === base) return;
    memFetchedFor = base;
    /* Fire-and-forget. Settles into the reactive state when the
       Tauri call returns; the chip pops in once the count is known.
       Errors silently fall back to zero — the chip just hides. */
    invoke<MemoryHit[]>('memory_search_local', { query: base, limit: 5 })
      .then((hits) => {
        if (memFetchedFor === base) memHits = hits;
      })
      .catch(() => {
        if (memFetchedFor === base) memHits = [];
      });
  });

  /* Popover state for the workspace-memory chip. Click toggles
     visibility; outside-click / Escape closes. Replaces the prior
     toast-based "showMemoryHits" — toast crammed every hit into one
     truncated body, which read as a wall of text and lost the per-
     row structure. The popover renders each hit as its own card
     with metadata header + preview, scrollable when there are many,
     and a row-click expands the full content inline so the user can
     read without leaving the header. */
  let memPopoverOpen = $state(false);
  let memPopoverEl = $state<HTMLDivElement | null>(null);
  let memExpandedId = $state<number | null>(null);

  function toggleMemPopover() {
    if (memHits.length === 0) {
      memPopoverOpen = false;
      notify({
        kind: 'info',
        title: 'No memories scoped to this project yet',
        body: 'They get saved automatically as the agent learns project facts.',
        ttlMs: 4000
      });
      return;
    }
    memPopoverOpen = !memPopoverOpen;
    memExpandedId = null;
  }

  function closeMemPopover() {
    memPopoverOpen = false;
    memExpandedId = null;
  }

  function toggleMemExpanded(id: number) {
    memExpandedId = memExpandedId === id ? null : id;
  }

  async function copyMemContent(content: string) {
    try {
      await navigator.clipboard.writeText(content);
      notify({ kind: 'success', title: 'Memory copied', ttlMs: 1500 });
    } catch (e) {
      console.warn('clipboard', e);
    }
  }

  /* SDD history popover — same chassis as the memory chip but driven
   *  by `sddState.workspaces`. Click toggles, outside-click / Escape
   *  close. Row click rebinds the workspace to the current session
   *  (SddCard re-renders below). Replaces the inline `SddLibraryCard`
   *  + composer `[HISTORY]` button approach — user feedback: history
   *  belongs in the header alongside memory, NOT as a floating inline
   *  card in the message stream. */
  let sddPopoverOpen = $state(false);
  let sddPopoverEl = $state<HTMLDivElement | null>(null);

  function toggleSddPopover() {
    sddPopoverOpen = !sddPopoverOpen;
  }
  function closeSddPopover() {
    sddPopoverOpen = false;
  }
  function sddStageLabel(w: SddWorkspace): string {
    const s = w.stage;
    switch (s.kind) {
      case 'drafting': return 'drafting spec';
      case 'spec_ready': return 'spec ready';
      case 'planning': return 'drafting plan';
      case 'plan_ready': return 'plan ready';
      case 'phase_pending_approval': return `phase ${s.phase} pending`;
      case 'phase_running': return `phase ${s.phase} running`;
      case 'phase_planning': return `phase ${s.phase} planning`;
      case 'phase_plan_review': return `phase ${s.phase} plan review`;
      case 'phase_implementing': return `phase ${s.phase} implementing`;
      case 'phase_verifying': return `phase ${s.phase} verifying`;
      case 'phase_done': return `phase ${s.phase} done`;
      case 'complete': return 'complete';
      case 'paused': return 'paused';
      case 'stopped': return 'stopped';
      case 'failed': return 'failed';
    }
  }
  function sddStageTone(w: SddWorkspace): 'live' | 'ok' | 'warn' | 'dim' {
    const k = w.stage.kind;
    if (
      k === 'drafting' ||
      k === 'planning' ||
      k === 'phase_running' ||
      k === 'phase_planning' ||
      k === 'phase_implementing' ||
      k === 'phase_verifying'
    ) return 'live';
    if (k === 'phase_plan_review') return 'warn';
    if (k === 'failed' || k === 'stopped') return 'warn';
    if (k === 'complete') return 'ok';
    return 'dim';
  }
  function sddPhaseProgress(w: SddWorkspace): string {
    if (w.phases.length === 0) return '';
    const done = w.phases.filter((ph: SddPhase) => ph.status === 'done').length;
    return `${done}/${w.phases.length}`;
  }
  function sddOpenWorkspace(workspaceId: string) {
    // Restore inline visibility if previously hidden — clicking the
    // history row is the "bring it back" gesture pair for the
    // SddCard's "—" hide button.
    showSddCard(workspaceId);
    openStandaloneView(workspaceId);
    closeSddPopover();
  }
  async function sddDiscardWorkspace(workspaceId: string) {
    await discardSdd(workspaceId);
  }
  /** Pull a workspace onto the current chat — rebinds its
   *  `session_id` so SddCard's Approve / Amend / advance buttons
   *  route their MCP calls through the agent CLI bound to `sess.id`. */
  async function sddAttachWorkspace(workspaceId: string) {
    if (!sess) return;
    await attachSddToSession(workspaceId, sess.id);
    showSddCard(workspaceId);
    closeSddPopover();
  }

  function memDate(epoch: number): string {
    const d = new Date(epoch * 1000);
    const yyyy = d.getFullYear();
    const mm = String(d.getMonth() + 1).padStart(2, '0');
    const dd = String(d.getDate()).padStart(2, '0');
    return `${yyyy}-${mm}-${dd}`;
  }

  /* Outside-click / Esc dismissal. Bound only while popover is open
     so we don't pay listener overhead on every header render. */
  $effect(() => {
    if (!memPopoverOpen) return;
    const onDown = (e: MouseEvent) => {
      if (!memPopoverEl) return;
      if (memPopoverEl.contains(e.target as Node)) return;
      /* Clicks on the chip itself toggle via its onclick — skip
         that to avoid an immediate re-open after this close. */
      const t = e.target as HTMLElement | null;
      if (t?.closest?.('.ch-mem')) return;
      closeMemPopover();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault();
        closeMemPopover();
      }
    };
    window.addEventListener('mousedown', onDown, true);
    window.addEventListener('keydown', onKey);
    return () => {
      window.removeEventListener('mousedown', onDown, true);
      window.removeEventListener('keydown', onKey);
    };
  });
</script>

<header class="ch">
  <div class="ch-title">
    {#if sess}
      {#if editingSessionId === sess.id}
        <input
          bind:this={inputEl}
          class="ch-sess ch-sess-input"
          bind:value={draftTitle}
          onkeydown={onTitleKey}
          onblur={commitRename}
          maxlength="120"
          aria-label="Chat title"
          spellcheck="false"
        />
      {:else}
        <button
          class="ch-sess-btn"
          onclick={startRename}
          title="Rename chat (click to edit)"
          aria-label="Rename chat"
        >
          <span class="ch-sess" class:ch-sess--empty={!sess.title}>
            {sess.title || 'Untitled chat'}
          </span>
          <svg class="ch-rename-hint" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7"/>
            <path d="M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4z"/>
          </svg>
        </button>
      {/if}
    {:else}
      <span class="ch-sess ch-sess--empty">No session</span>
    {/if}
  </div>

  {#if sess && budget.turns > 0}
    <button
      class="ch-budget"
      class:ch-budget--high={budget.costUsd >= 1}
      title={`Session token budget — ${budget.turns} turn${budget.turns === 1 ? '' : 's'}\n`
        + `Input: ${formatTokens(budget.input)}  ·  Output: ${formatTokens(budget.output)}\n`
        + `Cache read: ${formatTokens(budget.cacheRead)}  ·  Cache write: ${formatTokens(budget.cacheCreation)}\n`
        + `Estimated cost: ${formatCostUsd(budget.costUsd)}`}
      aria-label="Session token budget"
    >
      <span class="ch-budget-tokens mono">{formatTokens(budget.input + budget.output)}</span>
      {#if budget.costUsd > 0}
        <span class="ch-budget-cost mono">{formatCostUsd(budget.costUsd)}</span>
      {/if}
    </button>
  {/if}

  {#if memHits.length > 0}
    <div class="ch-mem-wrap">
      <button
        class="ch-mem"
        class:ch-mem--open={memPopoverOpen}
        onclick={toggleMemPopover}
        title="Memories matched to this project — click to preview"
        aria-label="Show project memories"
        aria-expanded={memPopoverOpen}
      >
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" aria-hidden="true">
          <path d="M19 21l-7-5-7 5V5a2 2 0 0 1 2-2h10a2 2 0 0 1 2 2z" stroke-linejoin="round"/>
        </svg>
        <span class="ch-mem-count">{memHits.length}</span>
      </button>
      {#if memPopoverOpen}
        <div bind:this={memPopoverEl} class="ch-mem-pop" role="dialog" aria-label="Project memories">
          <div class="ch-mem-pop-head">
            <span class="ch-mem-pop-title">
              {memHits.length} {memHits.length === 1 ? 'memory' : 'memories'} for
              <span class="ch-mem-pop-cwd mono">{cwdBasename}</span>
            </span>
            <button class="ch-mem-pop-close" onclick={closeMemPopover} aria-label="Close">
              <svg viewBox="0 0 24 24" width="13" height="13" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" aria-hidden="true">
                <line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/>
              </svg>
            </button>
          </div>
          <div class="ch-mem-pop-list">
            {#each memHits as hit (hit.id)}
              {@const isOpen = memExpandedId === hit.id}
              <div class="ch-mem-row" class:ch-mem-row--open={isOpen}>
                <button class="ch-mem-row-head" onclick={() => toggleMemExpanded(hit.id)} type="button">
                  <span class="ch-mem-row-id mono">#{hit.id}</span>
                  <span class="ch-mem-row-kind mono">{hit.kind}</span>
                  <span class="ch-mem-row-date mono">{memDate(hit.created_at)}</span>
                  <svg
                    class="ch-mem-row-caret"
                    class:ch-mem-row-caret--open={isOpen}
                    viewBox="0 0 24 24" width="10" height="10"
                    fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"
                  >
                    <path d="M6 9l6 6 6-6"/>
                  </svg>
                </button>
                {#if isOpen}
                  <div class="ch-mem-row-body">
                    <p>{hit.content}</p>
                    <div class="ch-mem-row-actions">
                      {#if hit.tags}
                        <span class="ch-mem-row-tags mono" title={hit.tags}>{hit.tags}</span>
                      {/if}
                      <button class="ch-mem-row-copy" onclick={() => void copyMemContent(hit.content)} type="button">
                        <svg viewBox="0 0 24 24" width="11" height="11" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                          <path d="M16 4h2a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h2"/>
                          <path d="M9 2h6a1 1 0 0 1 1 1v2H8V3a1 1 0 0 1 1-1z"/>
                        </svg>
                        Copy
                      </button>
                    </div>
                  </div>
                {:else}
                  <div class="ch-mem-row-preview">{hit.content.replace(/\s+/g, ' ').slice(0, 140)}</div>
                {/if}
              </div>
            {/each}
          </div>
        </div>
      {/if}
    </div>
  {/if}

  <!-- SDD history chip — same chassis as the memory chip. Click
       toggles a popover listing every SDD workspace on disk with
       its stage, phase progress, and per-row open / discard. Replaces
       the inline library card; history now lives next to memory,
       NOT in the message stream. -->
  <div class="ch-mem-wrap">
    <button
      class="ch-mem"
      class:ch-mem--open={sddPopoverOpen}
      onclick={toggleSddPopover}
      title="SDD workspace history"
      aria-label="Show SDD workspace history"
      aria-expanded={sddPopoverOpen}
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <rect x="3" y="3" width="18" height="18" rx="2"/>
        <path d="M3 9h18M9 21V9"/>
      </svg>
      <span class="ch-mem-count">{sddState.workspaces.length}</span>
    </button>
    {#if sddPopoverOpen}
      <div bind:this={sddPopoverEl} class="ch-mem-pop" role="dialog" aria-label="SDD workspace history">
        <div class="ch-mem-pop-head">
          <span class="ch-mem-pop-title">
            {sddState.workspaces.length} SDD workspace{sddState.workspaces.length === 1 ? '' : 's'}
          </span>
          <button class="ch-mem-pop-close" onclick={closeSddPopover} aria-label="Close">
            <svg viewBox="0 0 24 24" width="13" height="13" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" aria-hidden="true">
              <line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/>
            </svg>
          </button>
        </div>
        <div class="ch-mem-pop-list">
          {#if sddState.workspaces.length === 0}
            <div class="ch-sdd-empty">No specs yet. Type <span class="mono">/sdd &lt;ask&gt;</span> to start one.</div>
          {:else}
            {#each sddState.workspaces as w (w.id)}
              {@const activeWid = sess ? sddState.workspaceBySession[sess.id] : null}
              {@const isActive = w.id === activeWid}
              <div class="ch-sdd-row" data-tone={sddStageTone(w)} class:active={isActive}>
                <button class="ch-sdd-row-main" type="button" onclick={() => sddOpenWorkspace(w.id)} title="Open this workspace in the current chat (read-only overlay)">
                  <span class="ch-sdd-stage mono">{sddStageLabel(w)}</span>
                  <span class="ch-sdd-ask">{w.user_prompt || '(no ask)'}</span>
                  {#if sddPhaseProgress(w)}
                    <span class="ch-sdd-prog mono">{sddPhaseProgress(w)}</span>
                  {/if}
                </button>
                {#if sess && w.session_id !== sess.id}
                  <button
                    class="ch-sdd-attach"
                    type="button"
                    onclick={() => void sddAttachWorkspace(w.id)}
                    title="Attach this workspace to the current chat — SddCard buttons will drive the agent here"
                  >
                    <svg viewBox="0 0 24 24" width="12" height="12" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                      <path d="M21.44 11.05l-9.19 9.19a6 6 0 0 1-8.49-8.49l9.19-9.19a4 4 0 0 1 5.66 5.66l-9.2 9.19a2 2 0 0 1-2.83-2.83l8.49-8.48"/>
                    </svg>
                  </button>
                {/if}
                <button class="ch-sdd-discard" type="button" onclick={() => void sddDiscardWorkspace(w.id)} title="Delete this workspace">
                  <svg viewBox="0 0 24 24" width="11" height="11" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                    <polyline points="3 6 5 6 21 6"/><path d="M19 6l-2 14a2 2 0 0 1-2 2H9a2 2 0 0 1-2-2L5 6"/>
                  </svg>
                </button>
              </div>
            {/each}
          {/if}
        </div>
      </div>
    {/if}
  </div>

  {#if sess?.sending}
    <span class="ch-running">
      <span class="ch-pip"></span>
      <span class="mono">{elapsed || 'thinking'}</span>
    </span>

    <button class="ch-stop" onclick={p.onStop} title="Stop generation">
      <svg viewBox="0 0 24 24" fill="currentColor"><rect x="6" y="6" width="12" height="12" rx="1.5"/></svg>
    </button>
  {/if}
</header>

{#if sess?.interrupted}
  <!--
    Crash-recovery banner. Surfaces when this session was hydrated
    from a disk record whose `pendingTurn` was non-null — Woom died
    mid-stream the last time this chat ran. The user's next send
    will auto-stamp an `app_crash` recap onto cwdSwitchRecap and
    rotate the CLI uuid (see +page.svelte send-flow); the banner
    just lets them know that's what's happening so a sudden rotation
    + recap injection doesn't feel like silent magic. Dismissing
    only hides the banner — it doesn't suppress the auto-recap on
    send, since that's the right behaviour either way.
  -->
  <div class="ch-interrupt" role="status">
    <span class="ch-interrupt-dot" aria-hidden="true"></span>
    <span class="ch-interrupt-text">
      Previous turn was interrupted. Sending will continue from where it left off.
    </span>
    <button
      class="ch-interrupt-dismiss"
      onclick={() => sess && dismissInterrupted(sess.id)}
      title="Dismiss"
      aria-label="Dismiss interrupted-session banner"
    >
      ×
    </button>
  </div>
{/if}

<style>
  .ch {
    flex: 0 0 56px;
    display: flex; align-items: center; gap: 12px;
    padding: 0 22px;
    border-bottom: 1px solid var(--border);
    background: linear-gradient(180deg, var(--bg-2), var(--bg-1));
    min-height: 0;
  }
  .ch-title {
    flex: 1; min-width: 0;
    /* Center alignment plays nicer with the inline input — baseline
       was fine for plain text but pushed the input box above the
       row's vertical centre, leaving a visual gap below the focus
       ring. */
    display: flex; align-items: center; gap: 8px;
    font-family: 'Geist', 'Inter', -apple-system, system-ui, sans-serif;
    font-size: 22px; font-weight: 600;
    letter-spacing: -0.02em;
    white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
  }
  /* Static title chip — looks like text, behaves like a click target.
     The pencil hint stays faded until hover so the title doesn't read
     as a button at rest, but the affordance is discoverable on hover.

     No negative margins: those used to pull the chip 8px left so the
     baseline text aligned with the original `.ch-sess` span position,
     but combined with `.ch-title`'s `overflow: hidden` they made the
     hover bg clip lopsidedly on the left and bled the rename input's
     border off the visible row. Symmetric padding + flush-left
     alignment trades a small right-shift in the title for a clean
     pill on hover and a fully-visible input border during rename. */
  .ch-sess-btn {
    display: inline-flex; align-items: center; gap: 8px;
    background: transparent; border: 0;
    padding: 3px 10px;
    border-radius: 8px;
    cursor: text;
    color: inherit;
    font: inherit;
    letter-spacing: inherit;
    transition: background 140ms;
    max-width: 100%;
    min-width: 0;
  }
  .ch-sess-btn:hover { background: var(--bg-2); }
  .ch-sess-btn:hover .ch-rename-hint { opacity: 0.5; }
  .ch-rename-hint {
    width: 13px; height: 13px;
    color: var(--text-mute);
    opacity: 0;
    transition: opacity 140ms;
    flex-shrink: 0;
  }
  .ch-sess {
    color: var(--text-0);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }
  .ch-sess--empty { color: var(--text-mute); }

  /* Inline rename input — fits the natural title length instead of
     spanning the full title flex column, and stays inside the 56px
     header bounds. Aligned with the static `.ch-sess-btn` (same
     padding, no negative margin) so the row doesn't shift sideways
     when entering / leaving edit mode and the left border doesn't
     get clipped by `.ch-title`'s overflow: hidden. */
  .ch-sess-input {
    flex: 0 1 auto;
    min-width: 180px;
    max-width: 480px;
    width: auto;
    background: var(--bg-2);
    border: 1px solid var(--accent);
    border-radius: 8px;
    padding: 2px 10px;
    color: var(--text-0);
    font-family: inherit;
    font-size: inherit;
    font-weight: inherit;
    letter-spacing: inherit;
    outline: none;
  }
  .ch-sess-input:focus {
    /* Mint hairline ring on focus — thin enough to stay inside the
       row, bright enough to read as "you're editing". */
    box-shadow: 0 0 0 1px color-mix(in srgb, var(--accent) 60%, transparent);
  }

  .ch-running {
    display: inline-flex; align-items: center; gap: 6px;
    font-size: 11px; color: var(--text-mute);
  }
  .ch-pip {
    width: 6px; height: 6px; border-radius: 50%;
    background: var(--app-tone, var(--accent));
    box-shadow: 0 0 6px var(--app-glow, var(--accent-glow));
    animation: ch-pulse 1.4s infinite;
  }
  @keyframes ch-pulse {
    0%, 100% { opacity: 0.45; transform: scale(0.9); }
    50%      { opacity: 1;    transform: scale(1.1); }
  }

  .ch-stop {
    width: 28px; height: 28px;
    display: grid; place-items: center;
    border-radius: 7px;
    background: rgba(232, 130, 100, 0.10);
    border: 1px solid rgba(232, 130, 100, 0.32);
    color: var(--error);
    cursor: pointer;
    transition: background 140ms;
  }
  .ch-stop:hover { background: rgba(232, 130, 100, 0.18); }
  .ch-stop svg { width: 12px; height: 12px; }

  /* Token / USD budget chip. Sits left of the memory chip, same
     height + radius so they read as siblings. Renders only after
     the first assistant turn (gated on `budget.turns > 0`). Tooltip
     carries the per-bucket breakdown so the user can grok cache vs
     output without us building a popover yet. */
  .ch-budget {
    display: inline-flex; align-items: center; gap: 6px;
    height: 24px; padding: 0 10px;
    background: var(--bg-2);
    border: 1px solid var(--border);
    border-radius: 999px;
    color: var(--text-mute);
    font-size: 11px;
    line-height: 1;
    cursor: default;
    transition: color 140ms, background 140ms, border-color 140ms;
  }
  .ch-budget:hover {
    color: var(--text-1);
    border-color: var(--border-hi);
  }
  .ch-budget-tokens {
    color: var(--text-1);
    font-weight: 600;
    letter-spacing: 0.02em;
  }
  .ch-budget-cost {
    color: var(--text-mute);
    font-weight: 500;
  }
  /* High-cost session crosses $1 — gentle warning tint so the user
     notices without panic colours. */
  .ch-budget--high {
    background: color-mix(in srgb, var(--accent) 8%, var(--bg-2));
    border-color: color-mix(in srgb, var(--accent) 35%, var(--border));
  }
  .ch-budget--high .ch-budget-cost {
    color: var(--accent-bright, var(--accent));
  }

  /* Workspace memory chip — small subtle pill on the right edge of
     the header. Surfaces "the agent has prior context on this
     project" without being a primary affordance. Mute by default,
     darkens on hover, accent-tinted when the popover is open. */
  .ch-mem-wrap { position: relative; }
  .ch-mem {
    display: inline-flex; align-items: center; gap: 5px;
    height: 24px; padding: 0 8px;
    background: var(--bg-2);
    border: 1px solid var(--border);
    border-radius: 999px;
    color: var(--text-mute);
    font-size: 11px;
    cursor: pointer;
    transition: color 140ms, background 140ms, border-color 140ms;
  }
  .ch-mem:hover {
    color: var(--text-0);
    background: var(--bg-3, var(--bg-2));
    border-color: var(--border-strong, var(--border));
  }
  .ch-mem--open {
    color: var(--accent-bright);
    border-color: color-mix(in srgb, var(--accent) 50%, var(--border));
    background: color-mix(in srgb, var(--accent) 14%, var(--bg-2));
  }
  .ch-mem svg { width: 12px; height: 12px; }
  .ch-mem-count {
    font-weight: 600;
    color: var(--text-1);
    font-variant-numeric: tabular-nums;
  }
  .ch-mem--open .ch-mem-count { color: var(--accent-bright); }

  /* Popover — anchored to the chip wrapper. Right-aligned because the
     chip sits near the right edge of the header. Scrollable when the
     list exceeds the cap. Each row is a button that expands inline
     to show the full content + Copy + tags. */
  .ch-mem-pop {
    position: absolute;
    top: calc(100% + 6px);
    right: 0;
    width: 380px;
    max-height: 480px;
    display: flex;
    flex-direction: column;
    background: var(--bg-3);
    border: 1px solid var(--border-neutral-hi, var(--border));
    border-radius: 10px;
    box-shadow: var(--shadow-2, 0 12px 32px rgba(0, 0, 0, 0.32));
    z-index: 200;
    overflow: hidden;
    animation: ch-mem-pop-in 140ms var(--ease-out, ease-out);
  }
  @keyframes ch-mem-pop-in {
    from { opacity: 0; transform: translateY(-4px); }
    to   { opacity: 1; transform: translateY(0); }
  }
  @media (prefers-reduced-motion: reduce) {
    .ch-mem-pop { animation: none; }
  }
  .ch-mem-pop-head {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 12px;
    border-bottom: 1px solid var(--border);
    background: linear-gradient(180deg, color-mix(in srgb, var(--accent) 4%, transparent), transparent);
  }
  .ch-mem-pop-title {
    flex: 1; min-width: 0;
    font-size: 12px;
    color: var(--text-1);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .ch-mem-pop-cwd {
    color: var(--text-0);
    font-weight: 600;
  }
  .ch-mem-pop-close {
    width: 20px; height: 20px;
    display: grid; place-items: center;
    background: transparent; border: 0;
    border-radius: 4px;
    color: var(--text-mute);
    cursor: pointer;
    transition: color 120ms, background 120ms;
  }
  .ch-mem-pop-close:hover {
    color: var(--text-0);
    background: var(--bg-2);
  }
  .ch-mem-pop-list {
    flex: 1; min-height: 0;
    overflow-y: auto;
    padding: 6px;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  /* SDD history rows — same popover chassis as memory but denser
   *  (one line per workspace, click = re-bind to current session,
   *  trash glyph = discard). Active workspace highlighted via
   *  accent-bright label color. */
  .ch-sdd-empty {
    padding: 16px 12px;
    color: var(--text-mute);
    font-size: 12px;
    font-style: italic;
    text-align: center;
  }
  .ch-sdd-row {
    display: flex; align-items: center;
    gap: 6px;
    padding: 4px 6px;
    border-radius: 5px;
    transition: background 120ms;
  }
  .ch-sdd-row:hover { background: var(--bg-2); }
  .ch-sdd-row.active .ch-sdd-stage { color: var(--accent-bright); font-weight: 600; }
  .ch-sdd-row-main {
    flex: 1; min-width: 0;
    display: inline-flex; align-items: baseline; gap: 10px;
    padding: 2px 0;
    background: transparent; border: 0;
    color: var(--text-1);
    cursor: pointer;
    text-align: left;
    font: inherit;
  }
  .ch-sdd-stage {
    flex-shrink: 0;
    font-size: 10px;
    color: var(--text-mute);
    min-width: 100px;
  }
  .ch-sdd-row[data-tone="live"] .ch-sdd-stage { color: #66d39a; }
  .ch-sdd-row[data-tone="warn"] .ch-sdd-stage { color: #e0b16c; }
  .ch-sdd-row[data-tone="ok"] .ch-sdd-stage { color: var(--accent-bright); }
  .ch-sdd-ask {
    flex: 1; min-width: 0;
    font-size: 12px;
    color: var(--text-1);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .ch-sdd-prog {
    flex-shrink: 0;
    font-size: 10px;
    color: var(--text-mute);
  }
  .ch-sdd-discard {
    width: 22px; height: 22px;
    display: grid; place-items: center;
    background: transparent; border: 0;
    color: var(--text-mute);
    border-radius: 4px;
    cursor: pointer;
    flex-shrink: 0;
    transition: color 120ms, background 120ms;
  }
  .ch-sdd-discard:hover { color: var(--error); background: var(--bg-3); }
  .ch-sdd-attach {
    width: 22px; height: 22px;
    display: grid; place-items: center;
    background: transparent; border: 0;
    color: var(--text-mute);
    border-radius: 4px;
    cursor: pointer;
    flex-shrink: 0;
    transition: color 120ms, background 120ms;
  }
  .ch-sdd-attach:hover { color: var(--accent); background: var(--bg-3); }

  .ch-mem-row {
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg-2);
    overflow: hidden;
    transition: border-color 120ms;
  }
  .ch-mem-row--open {
    border-color: color-mix(in srgb, var(--accent) 50%, var(--border));
  }
  .ch-mem-row-head {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 6px 10px;
    background: transparent;
    border: 0;
    color: var(--text-1);
    font-size: 11px;
    cursor: pointer;
    text-align: left;
    transition: background 120ms;
  }
  .ch-mem-row-head:hover { background: var(--bg-3, rgba(255,255,255,0.04)); }
  .ch-mem-row-id { color: var(--text-mute); }
  .ch-mem-row-kind {
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--accent-bright);
    background: color-mix(in srgb, var(--accent) 14%, transparent);
    padding: 1px 5px;
    border-radius: 3px;
  }
  .ch-mem-row-date { color: var(--text-mute); flex: 1; }
  .ch-mem-row-caret {
    color: var(--text-mute);
    transition: transform 140ms;
  }
  .ch-mem-row-caret--open { transform: rotate(180deg); }
  .ch-mem-row-preview {
    padding: 0 10px 8px;
    color: var(--text-mute);
    font-size: 11.5px;
    line-height: 1.5;
    overflow: hidden;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    -webkit-box-orient: vertical;
    text-overflow: ellipsis;
  }
  .ch-mem-row-body {
    padding: 4px 10px 10px;
    border-top: 1px dashed var(--border);
    background: color-mix(in srgb, var(--accent) 4%, transparent);
  }
  .ch-mem-row-body p {
    margin: 6px 0;
    color: var(--text-0);
    font-size: 12px;
    line-height: 1.6;
    white-space: pre-wrap;
    word-wrap: break-word;
  }
  .ch-mem-row-actions {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 8px;
  }
  .ch-mem-row-tags {
    flex: 1; min-width: 0;
    font-size: 10.5px;
    color: var(--text-mute);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .ch-mem-row-copy {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 3px 8px;
    background: var(--bg-3);
    border: 1px solid var(--border);
    border-radius: 4px;
    color: var(--text-1);
    font-size: 10.5px;
    cursor: pointer;
    transition: background 120ms, color 120ms;
  }
  .ch-mem-row-copy:hover {
    background: var(--bg-2);
    color: var(--text-0);
  }

  /* Interrupted-session banner — slim warning row that sits directly
     below the chat header. Warm amber to read as "attention" without
     hitting error-red intensity (the session is recoverable; this is
     a recap heads-up, not a failure). Dismiss × shrinks to a faint
     mute glyph until hovered. */
  .ch-interrupt {
    display: flex; align-items: center; gap: 10px;
    padding: 8px 22px;
    background: rgba(232, 169, 100, 0.10);
    border-bottom: 1px solid rgba(232, 169, 100, 0.30);
    font-size: 12px;
    color: var(--text-1);
  }
  .ch-interrupt-dot {
    flex: 0 0 8px;
    width: 8px; height: 8px;
    border-radius: 50%;
    background: rgba(232, 169, 100, 0.85);
    animation: ch-pulse 1.6s infinite;
  }
  .ch-interrupt-text { flex: 1; min-width: 0; }
  .ch-interrupt-dismiss {
    flex: 0 0 22px;
    width: 22px; height: 22px;
    display: grid; place-items: center;
    background: transparent;
    border: 0;
    border-radius: 6px;
    font-size: 16px;
    line-height: 1;
    color: var(--text-mute);
    cursor: pointer;
    transition: color 140ms, background 140ms;
  }
  .ch-interrupt-dismiss:hover {
    color: var(--text-0);
    background: rgba(232, 169, 100, 0.12);
  }
</style>
