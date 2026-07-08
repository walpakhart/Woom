<script lang="ts">
  /* ChatHeader — single 52px line for the Cabin Claude solo
     (redesign v2 §2.5). Left: editable title + streaming state (+ stop).
     Right: spend chip (→ BudgetPopover) + Context toggle. The project-
     memory chip and the `linked:` label moved to the ContextDock. */
  import { sessionsState, updateSession } from '$lib/state/sessions.svelte';
  import { resolveSessionCwd } from '$lib/services/sessionCwd';
  import { sessionUsageTotals, formatTokens, formatCostUsd } from '$lib/usage';
  import BudgetPopover from '$lib/components/agent/BudgetPopover.svelte';
  import { tick } from 'svelte';

  type Kind = 'claude';

  interface Props {
    kind: Kind;
    instanceId: string;
    thinkingStartedAt: Record<string, number | null>;
    thinkingTick: Record<string, number>;
    onStop: () => void;
    /** Context dock open state + toggle (rendered as the "Context" button). */
    contextOpen?: boolean;
    onToggleContext?: () => void;
  }
  let p: Props = $props();

  const sess = $derived(
    sessionsState.list.find((s) => s.id === sessionsState.activeIds[p.kind]) ?? null
  );

  const budget = $derived(sessionUsageTotals(sess));
  const chipCostUsd = $derived(budget.costUsd);
  const totalTokens = $derived(budget.input + budget.output);

  /* Working folder — surfaced in the header so the session's cwd is
     visible at a glance (was Context-dock-only). Mirrors QuietChatHeader. */
  const repoLabel = $derived.by(() => {
    const cwd = resolveSessionCwd(sess);
    if (!cwd) return '';
    return cwd.split('/').filter(Boolean).pop() ?? '';
  });

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

  /** Idle clock — last message time, shown when not streaming. */
  const idleClock = $derived.by(() => {
    if (!sess || sess.sending) return '';
    const at = sess.messages[sess.messages.length - 1]?.at;
    if (!at) return '';
    const d = new Date(at);
    return `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`;
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
    if (!sess || editingSessionId !== sess.id) { editingSessionId = null; return; }
    const trimmed = draftTitle.trim();
    const next = trimmed || 'Untitled chat';
    if (next !== sess.title) updateSession(sess.id, { title: next });
    editingSessionId = null;
  }
  function cancelRename() { editingSessionId = null; }
  function onTitleKey(e: KeyboardEvent) {
    if (e.key === 'Enter') { e.preventDefault(); commitRename(); }
    else if (e.key === 'Escape') { e.preventDefault(); cancelRename(); }
  }
  $effect(() => {
    if (!sess) { editingSessionId = null; return; }
    if (editingSessionId && editingSessionId !== sess.id) editingSessionId = null;
  });

  /* ---------- Budget popover ---------- */
  let budgetPopoverOpen = $state(false);
  let budgetPopoverEl = $state<HTMLDivElement | null>(null);
  function closeBudgetPopover() { budgetPopoverOpen = false; }
  $effect(() => {
    if (!budgetPopoverOpen) return;
    const onDown = (e: MouseEvent) => {
      if (!budgetPopoverEl) return;
      if (budgetPopoverEl.contains(e.target as Node)) return;
      const t = e.target as HTMLElement | null;
      if (t?.closest?.('.ch-spend')) return;
      closeBudgetPopover();
    };
    const onKey = (e: KeyboardEvent) => { if (e.key === 'Escape') { e.preventDefault(); closeBudgetPopover(); } };
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

  {#if repoLabel}
    <button
      class="ch-folder"
      onclick={p.onToggleContext}
      title="Working folder — open Context"
      aria-label="Working folder {repoLabel}"
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>
      </svg>
      <span class="ch-folder-name">{repoLabel}</span>
      {#if sess?.worktreeBranch}<span class="ch-folder-wt mono">wt {sess.worktreeBranch}</span>{/if}
    </button>
  {/if}

  {#if sess?.sending}
    <span class="ch-state ch-state--live">streaming{elapsed ? ` · ${elapsed}` : ''}</span>
    <button class="ch-stop" onclick={p.onStop} title="Stop generation" aria-label="Stop generation">
      <svg viewBox="0 0 24 24" fill="currentColor"><rect x="6" y="6" width="12" height="12" rx="1.5"/></svg>
    </button>
  {:else if idleClock}
    <span class="ch-state mono">idle · {idleClock}</span>
  {/if}

  <div class="ch-spring"></div>

  {#if sess && budget.turns > 0}
    <div class="ch-spend-wrap">
      <button
        class="ch-spend mono"
        class:ch-spend--open={budgetPopoverOpen}
        onclick={() => (budgetPopoverOpen = !budgetPopoverOpen)}
        title="Session token budget — click for the breakdown"
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

  {#if p.onToggleContext}
    <button
      class="ch-ctx"
      class:ch-ctx--open={p.contextOpen}
      onclick={p.onToggleContext}
      title="Toggle context panel"
      aria-pressed={p.contextOpen}
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <rect x="3" y="4" width="18" height="16" rx="2"/><line x1="15" y1="4" x2="15" y2="20"/>
      </svg>
      <span>Context</span>
    </button>
  {/if}
</header>

<style>
  .ch {
    flex: none;
    display: flex; align-items: center; gap: 10px;
    height: 52px;
    padding: 0 24px;
    border-bottom: 1px solid var(--border-lo);
    background: var(--bg-0);
  }
  .ch-name-btn {
    display: inline-flex; align-items: center;
    max-width: 420px; min-width: 0;
    background: transparent; border: 0; padding: 2px 6px;
    margin-left: -6px;
    border-radius: var(--r-item);
    cursor: text; color: inherit; font: inherit;
    transition: background 140ms;
  }
  .ch-name-btn:hover { background: var(--bg-hover); }
  .ch-name {
    font-size: 15px; font-weight: 600;
    color: var(--text-0);
    white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
    min-width: 0;
  }
  .ch-name--empty { color: var(--text-mute); }
  .ch-name-input {
    min-width: 200px; max-width: 360px; width: auto;
    background: var(--bg-2);
    border: 1px solid var(--border-solid, var(--border-hi));
    border-radius: var(--r-item);
    padding: 3px 8px;
    color: var(--text-0);
    font-size: 15px; font-weight: 600;
    outline: none;
  }

  .ch-folder {
    display: inline-flex; align-items: center; gap: 5px;
    flex: none; min-width: 0; max-width: 220px;
    padding: 3px 8px;
    border-radius: var(--r-chip);
    border: 1px solid transparent;
    background: transparent;
    color: var(--text-2);
    font-size: 12px; cursor: pointer;
    transition: color 120ms, background 120ms, border-color 120ms;
  }
  .ch-folder:hover { color: var(--text-0); background: var(--bg-hover); border-color: var(--border); }
  .ch-folder svg { width: 12px; height: 12px; flex: none; color: var(--text-faint); }
  .ch-folder-name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; }
  .ch-folder-wt { font-size: 11px; color: var(--text-faint); flex: none; }

  .ch-state {
    font-size: 12px; color: var(--text-faint);
    white-space: nowrap;
  }
  .ch-state--live {
    color: var(--src-claude);
    border: 1px solid var(--src-claude-border);
    border-radius: var(--r-chip);
    padding: 2px 8px; font-size: 11px; font-weight: 500;
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

  .ch-spring { flex: 1; }

  .ch-spend-wrap { position: relative; }
  .ch-spend {
    background: transparent; border: 0; padding: 2px 6px;
    border-radius: var(--r-chip);
    cursor: pointer;
    font-size: 11px; color: var(--text-faint);
    white-space: nowrap;
    font-variant-numeric: tabular-nums;
    transition: background 140ms, color 140ms;
  }
  .ch-spend:hover, .ch-spend--open { background: var(--bg-hover); color: var(--text-1); }
  .ch-budget-pop-anchor { position: absolute; top: calc(100% + 8px); right: 0; z-index: 200; }

  .ch-ctx {
    display: inline-flex; align-items: center; gap: 6px;
    padding: 4px 10px;
    border-radius: 7px;
    border: 1px solid transparent;
    background: transparent;
    color: var(--text-mute);
    font-size: 12px; cursor: pointer;
    transition: color 120ms, background 120ms, border-color 120ms;
  }
  .ch-ctx:hover { color: var(--text-1); border-color: var(--border); }
  .ch-ctx--open { background: var(--bg-3); border-color: var(--border); color: var(--text-0); }
  .ch-ctx svg { width: 12px; height: 12px; }
</style>
