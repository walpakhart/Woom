<script lang="ts">
  /* ChatHeader — top row of AgentApp center pane, "session vitals" layout.
     Three zones: HEAD (editable title + turns·$·tok sub-line), RIBBON
     (one bar per completed turn — height = cost, dot = tool calls), NOW
     (project-memory chip; swaps to a live pip + elapsed + stop while a
     turn is in flight).

     DW + SDD history used to live here as popover chips; they moved to
     Settings → Workflows (SettingsWorkflows.svelte). The header now
     carries only what's about the *active conversation*: its story
     (ribbon), its spend (sub-line + BudgetPopover), and its project
     memory.

     Title rename: click the title to edit inline; Enter / blur commits,
     Esc cancels. Empty falls back to "Untitled chat". */
  import { sessionsState, updateSession, dismissInterrupted } from '$lib/state/sessions.svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { sessionUsageTotals, formatTokens, formatCostUsd } from '$lib/usage';
  import { sessionDwTotals } from '$lib/state/dw.svelte';
  import BudgetPopover from '$lib/components/agent/BudgetPopover.svelte';
  import { notify } from '$lib/state/toaster.svelte';
  import { tick, untrack } from 'svelte';

  type Kind = 'claude';

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
   *  snapshot in the current session. */
  const budget = $derived(sessionUsageTotals(sess));
  const dwTotals = $derived(sess ? sessionDwTotals(sess.id) : { costUsd: 0, runs: 0 });
  const chipCostUsd = $derived(budget.costUsd + dwTotals.costUsd);
  const totalTokens = $derived(budget.input + budget.output);

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
  let editingSessionId = $state<string | null>(null);
  let draftTitle = $state('');
  let inputEl = $state<HTMLInputElement | null>(null);

  async function startRename() {
    if (!sess) return;
    editingSessionId = sess.id;
    draftTitle = sess.title || '';
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
    const next = trimmed || 'Untitled chat';
    if (next !== sess.title) updateSession(sess.id, { title: next });
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
  $effect(() => {
    if (!sess) {
      editingSessionId = null;
      return;
    }
    if (editingSessionId && editingSessionId !== sess.id) editingSessionId = null;
  });

  /* ---------- Workspace memory indicator ---------- */
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
    invoke<MemoryHit[]>('memory_search_local', { query: base, limit: 5 })
      .then((hits) => {
        if (memFetchedFor === base) memHits = hits;
      })
      .catch(() => {
        if (memFetchedFor === base) memHits = [];
      });
  });

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
  function memDate(epoch: number): string {
    const d = new Date(epoch * 1000);
    const yyyy = d.getFullYear();
    const mm = String(d.getMonth() + 1).padStart(2, '0');
    const dd = String(d.getDate()).padStart(2, '0');
    return `${yyyy}-${mm}-${dd}`;
  }

  /* ---------- Budget popover ---------- */
  let budgetPopoverOpen = $state(false);
  let budgetPopoverEl = $state<HTMLDivElement | null>(null);
  function toggleBudgetPopover() {
    budgetPopoverOpen = !budgetPopoverOpen;
  }
  function closeBudgetPopover() {
    budgetPopoverOpen = false;
  }

  /* Outside-click / Esc dismissal — bound only while a popover is open. */
  $effect(() => {
    if (!memPopoverOpen) return;
    const onDown = (e: MouseEvent) => {
      if (!memPopoverEl) return;
      if (memPopoverEl.contains(e.target as Node)) return;
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

  $effect(() => {
    if (!budgetPopoverOpen) return;
    const onDown = (e: MouseEvent) => {
      if (!budgetPopoverEl) return;
      if (budgetPopoverEl.contains(e.target as Node)) return;
      const t = e.target as HTMLElement | null;
      if (t?.closest?.('.ch-spend')) return;
      closeBudgetPopover();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault();
        closeBudgetPopover();
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
  {#if sess}
    {#if editingSessionId === sess.id}
      <input
        bind:this={inputEl}
        class="ch-name ch-name-input"
        bind:value={draftTitle}
        onkeydown={onTitleKey}
        onblur={commitRename}
        maxlength="120"
        aria-label="Chat title"
        spellcheck="false"
      />
    {:else}
      <button class="ch-name-btn" onclick={startRename} title="Rename chat" aria-label="Rename chat">
        <span class="ch-name" class:ch-name--empty={!sess.title}>{sess.title || 'Untitled chat'}</span>
      </button>
    {/if}
  {:else}
    <span class="ch-name ch-name--empty">No session</span>
  {/if}

  {#if sess?.sending}
    <span class="ch-state ch-state--live">streaming{elapsed ? ` · ${elapsed}` : ''}</span>
    <button class="ch-stop" onclick={p.onStop} title="Stop generation" aria-label="Stop generation">
      <svg viewBox="0 0 24 24" fill="currentColor"><rect x="6" y="6" width="12" height="12" rx="1.5"/></svg>
    </button>
  {/if}

  <div class="ch-spring"></div>

  {#if sess && (budget.turns > 0 || dwTotals.runs > 0)}
    <div class="ch-spend-wrap">
      <button
        class="ch-spend"
        class:ch-spend--open={budgetPopoverOpen}
        onclick={toggleBudgetPopover}
        title="Session token budget — click for the breakdown"
        aria-label="Session token budget"
        aria-expanded={budgetPopoverOpen}
      >
        {budget.turns} turns{#if chipCostUsd > 0}&nbsp;· {formatCostUsd(chipCostUsd)}{/if} · {formatTokens(totalTokens)} tok
      </button>
      {#if budgetPopoverOpen}
        <div class="ch-budget-pop-anchor" bind:this={budgetPopoverEl}>
          <BudgetPopover session={sess} onClose={closeBudgetPopover} />
        </div>
      {/if}
    </div>
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
        <svg viewBox="0 0 24 24" width="11" height="11" fill="none" stroke="currentColor" stroke-width="1.7" aria-hidden="true">
          <path d="M19 21l-7-5-7 5V5a2 2 0 0 1 2-2h10a2 2 0 0 1 2 2z" stroke-linejoin="round"/>
        </svg>
        <span>{memHits.length}</span>
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
                  ><path d="M6 9l6 6 6-6"/></svg>
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

  {#if cwdBasename}
    <span class="ch-linked" title={effCwd}>
      linked: <span class="ch-linked-src">{sess?.linkedToEditor ? 'editor · ' : ''}{cwdBasename}</span>{#if sess?.worktreeBranch}&nbsp;· worktree {sess.worktreeBranch}{/if}
    </span>
  {/if}
</header>

{#if sess?.interrupted}
  <!--
    Crash-recovery banner — surfaces when this session hydrated from a
    disk record whose `pendingTurn` was non-null (Woom died mid-stream).
    The next send auto-stamps an `app_crash` recap + rotates the CLI
    uuid; the banner just makes that visible. Dismiss only hides it.
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
  /* Single-line header per the paper mockup: title + streaming chip
     left, spend / memory / linked right. */
  .ch {
    flex: none;
    display: flex; align-items: center; gap: 10px;
    padding: 12px 22px;
    border-bottom: 1px solid var(--border);
    background: var(--bg-1);
    min-height: 0;
  }
  .ch-spring { flex: 1; }

  .ch-name-btn {
    display: inline-flex; align-items: center;
    max-width: 340px; min-width: 0;
    background: transparent; border: 0; padding: 2px 6px;
    margin-left: -6px;
    border-radius: var(--r-item);
    cursor: text; color: inherit; font: inherit;
    transition: background 140ms;
  }
  .ch-name-btn:hover { background: var(--bg-hover); }
  .ch-name {
    font-size: 13px; font-weight: 600;
    color: var(--text-0);
    white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
    min-width: 0;
  }
  .ch-name--empty { color: var(--text-mute); }
  .ch-name-input {
    min-width: 180px; max-width: 300px; width: auto;
    background: var(--bg-2);
    border: 1px solid var(--border-solid);
    border-radius: var(--r-item);
    padding: 2px 8px;
    color: var(--text-0);
    font-size: 13px; font-weight: 600;
    outline: none;
  }

  /* Streaming chip — outline, claude tone, per the mockup. */
  .ch-state {
    font-size: 10px; font-weight: 500;
    padding: 2px 8px;
    border-radius: var(--r-chip);
    white-space: nowrap;
  }
  .ch-state--live {
    border: 1px solid var(--src-claude-border);
    color: var(--src-claude);
  }

  .ch-stop {
    width: 22px; height: 22px;
    display: grid; place-items: center;
    border-radius: var(--r-btn);
    background: transparent;
    border: 1px solid var(--err-border);
    color: var(--err);
    cursor: pointer;
    transition: background 140ms;
  }
  .ch-stop:hover { background: color-mix(in srgb, var(--err) 10%, transparent); }
  .ch-stop svg { width: 10px; height: 10px; }

  .ch-spend-wrap { position: relative; }
  .ch-spend {
    background: transparent; border: 0; padding: 1px 4px;
    border-radius: var(--r-chip);
    cursor: pointer; font: inherit;
    font-size: 10.5px; color: var(--text-mute);
    white-space: nowrap;
    font-variant-numeric: tabular-nums;
    transition: background 140ms, color 140ms;
  }
  .ch-spend:hover, .ch-spend--open { background: var(--bg-hover); color: var(--text-1); }
  .ch-budget-pop-anchor { position: absolute; top: calc(100% + 8px); right: 0; z-index: 200; }

  .ch-linked {
    font-size: 10.5px; color: var(--text-mute);
    white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
    max-width: 340px;
  }
  .ch-linked-src { color: var(--src-editor); }

  /* Memory chip + popover */

  .ch-mem-wrap { position: relative; }
  .ch-mem {
    display: inline-flex; align-items: center; gap: 5px;
    height: 20px; padding: 0 7px;
    background: transparent;
    border: 1px solid var(--border-hi);
    border-radius: var(--r-chip);
    color: var(--text-mute);
    font-size: 10.5px;
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
  .ch-mem-count { font-weight: 600; color: var(--text-1); font-variant-numeric: tabular-nums; }
  .ch-mem--open .ch-mem-count { color: var(--accent-bright); }

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
    .ch-turn--live { animation: none; }
  }
  .ch-mem-pop-head {
    display: flex; align-items: center; gap: 8px;
    padding: 10px 12px;
    border-bottom: 1px solid var(--border);
    background: linear-gradient(180deg, color-mix(in srgb, var(--accent) 4%, transparent), transparent);
  }
  .ch-mem-pop-title {
    flex: 1; min-width: 0;
    font-size: 12px; color: var(--text-1);
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .ch-mem-pop-cwd { color: var(--text-0); font-weight: 600; }
  .ch-mem-pop-close {
    width: 20px; height: 20px;
    display: grid; place-items: center;
    background: transparent; border: 0;
    border-radius: 4px; color: var(--text-mute); cursor: pointer;
    transition: color 120ms, background 120ms;
  }
  .ch-mem-pop-close:hover { color: var(--text-0); background: var(--bg-2); }
  .ch-mem-pop-list {
    flex: 1; min-height: 0;
    overflow-y: auto; padding: 6px;
    display: flex; flex-direction: column; gap: 4px;
  }
  .ch-mem-row {
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg-2);
    overflow: hidden;
    transition: border-color 120ms;
  }
  .ch-mem-row--open { border-color: color-mix(in srgb, var(--accent) 50%, var(--border)); }
  .ch-mem-row-head {
    display: flex; align-items: center; gap: 8px;
    width: 100%; padding: 6px 10px;
    background: transparent; border: 0;
    color: var(--text-1); font-size: 11px;
    cursor: pointer; text-align: left;
    transition: background 120ms;
  }
  .ch-mem-row-head:hover { background: var(--bg-3, rgba(255,255,255,0.04)); }
  .ch-mem-row-id { color: var(--text-mute); }
  .ch-mem-row-kind {
    font-size: 10px; text-transform: uppercase; letter-spacing: 0.05em;
    color: var(--accent-bright);
    background: color-mix(in srgb, var(--accent) 14%, transparent);
    padding: 1px 5px; border-radius: 3px;
  }
  .ch-mem-row-date { color: var(--text-mute); flex: 1; }
  .ch-mem-row-caret { color: var(--text-mute); transition: transform 140ms; }
  .ch-mem-row-caret--open { transform: rotate(180deg); }
  .ch-mem-row-preview {
    padding: 0 10px 8px;
    color: var(--text-mute); font-size: 11.5px; line-height: 1.5;
    overflow: hidden; display: -webkit-box;
    -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical;
    text-overflow: ellipsis;
  }
  .ch-mem-row-body {
    padding: 4px 10px 10px;
    border-top: 1px dashed var(--border);
    background: color-mix(in srgb, var(--accent) 4%, transparent);
  }
  .ch-mem-row-body p {
    margin: 6px 0; color: var(--text-0); font-size: 12px; line-height: 1.6;
    white-space: pre-wrap; word-wrap: break-word;
  }
  .ch-mem-row-actions { display: flex; align-items: center; gap: 8px; margin-top: 8px; }
  .ch-mem-row-tags {
    flex: 1; min-width: 0; font-size: 10.5px; color: var(--text-mute);
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .ch-mem-row-copy {
    display: inline-flex; align-items: center; gap: 4px;
    padding: 3px 8px;
    background: var(--bg-3); border: 1px solid var(--border);
    border-radius: 4px; color: var(--text-1); font-size: 10.5px;
    cursor: pointer; transition: background 120ms, color 120ms;
  }
  .ch-mem-row-copy:hover { background: var(--bg-2); color: var(--text-0); }

  /* Interrupted-session banner */
  .ch-interrupt {
    display: flex; align-items: center; gap: 10px;
    padding: 8px 22px;
    background: rgba(232, 169, 100, 0.10);
    border-bottom: 1px solid rgba(232, 169, 100, 0.30);
    font-size: 12px; color: var(--text-1);
  }
  .ch-interrupt-dot {
    flex: 0 0 8px; width: 8px; height: 8px; border-radius: 50%;
    background: rgba(232, 169, 100, 0.85);
    animation: ch-pulse 1.6s infinite;
  }
  .ch-interrupt-text { flex: 1; min-width: 0; }
  .ch-interrupt-dismiss {
    flex: 0 0 22px; width: 22px; height: 22px;
    display: grid; place-items: center;
    background: transparent; border: 0; border-radius: 6px;
    font-size: 16px; line-height: 1; color: var(--text-mute);
    cursor: pointer; transition: color 140ms, background 140ms;
  }
  .ch-interrupt-dismiss:hover { color: var(--text-0); background: rgba(232, 169, 100, 0.12); }
</style>
