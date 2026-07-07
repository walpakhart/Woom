<script lang="ts">
  /* Quiet-direction chat document header (redesign v2 §3.2, screen 3c).
     Replaces the Cabin ChatHeader + Chats sidebar: a centred document
     head with the session title (inline-rename), a «N ▾» switcher
     popover that lists sessions (the Chats list moves here in Quiet),
     the turns/spend chip → BudgetPopover, a dotted meta line, and a
     "контекст ▾" chip that toggles the context popover. */
  import { sessionsState, updateSession, setActiveSessionInInstance } from '$lib/state/sessions.svelte';
  import { sessionUsageTotals, formatTokens, formatCostUsd } from '$lib/usage';
  import { sessionDwTotals } from '$lib/state/dw.svelte';
  import BudgetPopover from '$lib/components/agent/BudgetPopover.svelte';
  import { tick } from 'svelte';

  type Kind = 'claude';
  interface Props {
    kind: Kind;
    instanceId: string;
    thinkingStartedAt: Record<string, number | null>;
    thinkingTick: Record<string, number>;
    onStop: () => void;
    contextOpen?: boolean;
    onToggleContext?: () => void;
  }
  let p: Props = $props();

  const sess = $derived(
    sessionsState.list.find((s) => s.id === sessionsState.activeIds[p.kind]) ?? null
  );

  const budget = $derived(sessionUsageTotals(sess));
  const dwTotals = $derived(sess ? sessionDwTotals(sess.id) : { costUsd: 0, runs: 0 });
  const chipCostUsd = $derived(budget.costUsd + dwTotals.costUsd);
  const totalTokens = $derived(budget.input + budget.output);

  const elapsed = $derived.by(() => {
    const startedAt = sess ? p.thinkingStartedAt[sess.id] ?? null : null;
    if (!startedAt || !sess?.sending) return '';
    void (sess ? p.thinkingTick[sess.id] : 0);
    const s = Math.floor((Date.now() - startedAt) / 1000);
    return s < 60 ? `${s}s` : `${Math.floor(s / 60)}m ${String(s % 60).padStart(2, '0')}s`;
  });

  function shortModel(m: string | null | undefined): string {
    if (!m) return '';
    let s = m.replace(/^claude-/, '');
    s = s.replace(/-(\d+)-(\d+)/, '-$1.$2');
    s = s.replace('[1m]', ' · 1M');
    s = s.replace(/-\d{8}$/, '');
    return s;
  }
  const modelLabel = $derived(shortModel(sess?.claudeModel));
  const repoLabel = $derived.by(() => {
    const cwd = sess?.worktreePath ?? sess?.cwd ?? null;
    if (!cwd) return '';
    return cwd.split('/').filter(Boolean).pop() ?? '';
  });

  /* ---- session switcher ---- */
  const sessions = $derived.by(() => {
    const items = sessionsState.list.filter((s) => !s.archived);
    const t = (s: (typeof items)[number]) => {
      const last = s.messages[s.messages.length - 1]?.at;
      return last ? new Date(last).getTime() : 0;
    };
    return [...items].sort((a, b) => t(b) - t(a));
  });
  let switcherOpen = $state(false);
  function pickSession(id: string) {
    setActiveSessionInInstance(p.instanceId, id);
    switcherOpen = false;
  }

  /* ---- rename ---- */
  let editing = $state(false);
  let draft = $state('');
  let inputEl = $state<HTMLInputElement | null>(null);
  async function startRename() {
    if (!sess) return;
    editing = true;
    draft = sess.title || '';
    await tick();
    inputEl?.focus();
    inputEl?.select();
  }
  function commitRename() {
    if (!sess || !editing) { editing = false; return; }
    const next = draft.trim() || 'Untitled chat';
    if (next !== sess.title) updateSession(sess.id, { title: next });
    editing = false;
  }
  function onTitleKey(e: KeyboardEvent) {
    if (e.key === 'Enter') { e.preventDefault(); commitRename(); }
    else if (e.key === 'Escape') { e.preventDefault(); editing = false; }
  }

  /* ---- budget popover ---- */
  let budgetOpen = $state(false);
  $effect(() => {
    if (!budgetOpen) return;
    const onDown = (e: MouseEvent) => {
      const t = e.target as HTMLElement | null;
      if (t?.closest?.('.qh-budget-wrap')) return;
      budgetOpen = false;
    };
    window.addEventListener('mousedown', onDown, true);
    return () => window.removeEventListener('mousedown', onDown, true);
  });

  /* Close switcher on outside click. */
  $effect(() => {
    if (!switcherOpen) return;
    const onDown = (e: MouseEvent) => {
      const t = e.target as HTMLElement | null;
      if (t?.closest?.('.qh-switch-wrap')) return;
      switcherOpen = false;
    };
    window.addEventListener('mousedown', onDown, true);
    return () => window.removeEventListener('mousedown', onDown, true);
  });
</script>

<header class="qh">
  <div class="qh-titlerow">
    {#if editing}
      <input
        bind:this={inputEl}
        class="qh-title-input"
        bind:value={draft}
        onkeydown={onTitleKey}
        onblur={commitRename}
        maxlength="120"
        spellcheck="false"
        aria-label="Chat title"
      />
    {:else}
      <button class="qh-title" onclick={startRename} title="Rename chat">{sess?.title || 'Untitled chat'}</button>
    {/if}

    <div class="qh-switch-wrap">
      <button class="qh-switch" class:open={switcherOpen} onclick={() => (switcherOpen = !switcherOpen)} title="Switch chat" aria-expanded={switcherOpen}>
        {sessions.length} <span class="qh-caret" aria-hidden="true">▾</span>
      </button>
      {#if switcherOpen}
        <div class="qh-switch-pop" role="listbox" aria-label="Chats">
          {#each sessions as s (s.id)}
            <button
              class="qh-switch-item"
              class:active={s.id === sessionsState.activeIds[p.kind]}
              onclick={() => pickSession(s.id)}
              role="option"
              aria-selected={s.id === sessionsState.activeIds[p.kind]}
            >
              <span class="qh-switch-dot" class:running={s.sending} aria-hidden="true"></span>
              <span class="qh-switch-name">{s.title || 'Untitled chat'}</span>
            </button>
          {/each}
        </div>
      {/if}
    </div>

    <span class="qh-spring"></span>

    {#if sess?.sending}
      <span class="qh-state">streaming{elapsed ? ` · ${elapsed}` : ''}</span>
      <button class="qh-stop" onclick={p.onStop} title="Stop generation" aria-label="Stop">
        <svg viewBox="0 0 24 24" fill="currentColor"><rect x="6" y="6" width="12" height="12" rx="1.5"/></svg>
      </button>
    {/if}

    {#if sess && (budget.turns > 0 || dwTotals.runs > 0)}
      <div class="qh-budget-wrap">
        <button class="qh-spend mono" class:open={budgetOpen} onclick={() => (budgetOpen = !budgetOpen)} title="Session budget">
          {budget.turns} turns{#if chipCostUsd > 0}&nbsp;· {formatCostUsd(chipCostUsd)}{/if} · {formatTokens(totalTokens)}
        </button>
        {#if budgetOpen && sess}
          <div class="qh-budget-pop">
            <BudgetPopover session={sess} onClose={() => (budgetOpen = false)} />
          </div>
        {/if}
      </div>
    {/if}
  </div>

  <div class="qh-meta">
    {#if repoLabel}<span class="qh-hot">{repoLabel}{#if sess?.worktreeBranch}<span class="qh-mono"> · wt {sess.worktreeBranch}</span>{/if}</span>{/if}
    {#if modelLabel}<span class="qh-hot qh-mono">{modelLabel}</span>{/if}
    {#if p.onToggleContext}
      <button class="qh-ctx-chip" class:open={p.contextOpen} onclick={p.onToggleContext} aria-expanded={p.contextOpen}>контекст <span class="qh-caret" aria-hidden="true">▾</span></button>
    {/if}
  </div>
</header>

<style>
  .qh {
    flex: none;
    display: flex; flex-direction: column; gap: 6px;
    width: 100%; max-width: 720px; margin: 0 auto;
    padding: 6px 0 14px;
  }
  .qh-titlerow { display: flex; align-items: baseline; gap: 10px; }
  .qh-title {
    font-size: 21px; font-weight: 600; letter-spacing: -0.015em;
    color: var(--text-0);
    background: transparent; border: 0; padding: 0; cursor: text;
    max-width: 60%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .qh-title:hover { color: var(--accent-bright); }
  .qh-title-input {
    font-size: 21px; font-weight: 600; letter-spacing: -0.015em;
    color: var(--text-0); background: var(--bg-2);
    border: 1px solid var(--border-hi); border-radius: 7px;
    padding: 2px 8px; outline: none; min-width: 240px;
  }

  .qh-switch-wrap { position: relative; }
  .qh-switch {
    background: transparent; border: 0; cursor: pointer;
    font-size: 12px; color: var(--text-faint);
    display: inline-flex; align-items: center; gap: 3px;
  }
  .qh-switch:hover, .qh-switch.open { color: var(--text-1); }
  .qh-caret { font-size: 9px; opacity: 0.8; }
  .qh-switch-pop {
    position: absolute; top: calc(100% + 8px); left: 0; z-index: 200;
    min-width: 260px; max-height: 340px; overflow-y: auto;
    padding: 4px;
    background: var(--bg-1); border: 1px solid var(--border-hi);
    border-radius: 12px; box-shadow: var(--shadow-3);
    display: flex; flex-direction: column; gap: 1px;
  }
  .qh-switch-item {
    display: flex; align-items: center; gap: 8px;
    padding: 8px 10px; border-radius: 8px;
    background: transparent; border: 0; cursor: pointer; text-align: left;
    color: var(--text-1); font-size: 13px;
  }
  .qh-switch-item:hover { background: var(--bg-hover); color: var(--text-0); }
  .qh-switch-item.active { background: var(--bg-3); color: var(--text-0); box-shadow: var(--shadow-1); }
  .qh-switch-dot {
    width: 6px; height: 6px; border-radius: 50%; flex: none;
    background: var(--text-linenum, var(--text-mute));
  }
  .qh-switch-dot.running { background: var(--ok); }
  .qh-switch-name { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }

  .qh-spring { flex: 1; }
  .qh-state {
    font-size: 11px; color: var(--src-claude);
    border: 1px solid var(--src-claude-border); border-radius: var(--r-chip);
    padding: 2px 8px; white-space: nowrap;
  }
  .qh-stop {
    width: 22px; height: 22px; display: grid; place-items: center;
    border-radius: var(--r-btn); background: transparent;
    border: 1px solid var(--err-border); color: var(--err); cursor: pointer;
  }
  .qh-stop svg { width: 10px; height: 10px; }

  .qh-budget-wrap { position: relative; }
  .qh-spend {
    background: transparent; border: 0; padding: 2px 4px; cursor: pointer;
    font-size: 11px; color: var(--text-faint);
    font-variant-numeric: tabular-nums; white-space: nowrap;
  }
  .qh-spend:hover, .qh-spend.open { color: var(--text-1); }
  .qh-budget-pop { position: absolute; top: calc(100% + 8px); right: 0; z-index: 200; }

  .qh-meta { display: flex; align-items: center; gap: 14px; flex-wrap: wrap; }
  .qh-hot {
    font-size: 12.5px; color: var(--text-2);
    border-bottom: 1px dotted color-mix(in srgb, var(--text-2) 40%, transparent);
    padding-bottom: 1px;
  }
  .qh-mono { font-family: var(--font-mono); font-size: 11.5px; }
  .qh-ctx-chip {
    display: inline-flex; align-items: center; gap: 4px;
    padding: 2px 8px; border-radius: 6px;
    background: var(--bg-3); border: 1px solid transparent;
    color: var(--text-1); font-size: 12px; cursor: pointer;
    transition: border-color 120ms, color 120ms;
  }
  .qh-ctx-chip:hover { color: var(--text-0); border-color: var(--border); }
  .qh-ctx-chip.open { border-color: var(--border-hi); color: var(--text-0); }
</style>
