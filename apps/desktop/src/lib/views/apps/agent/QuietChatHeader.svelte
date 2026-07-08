<script lang="ts">
  /* Quiet-direction chat document header (redesign v2 §3.2, screen 3c).
     Replaces the Cabin ChatHeader + Chats sidebar: a centred document
     head with the session title (inline-rename), a «N ▾» switcher
     popover that lists sessions (the Chats list moves here in Quiet),
     the turns/spend chip → BudgetPopover, a dotted meta line, and a
     "context ▾" chip that toggles the context popover. */
  import { sessionsState, updateSession, setActiveSessionInInstance, newClaudeSession, deleteClaudeSession, restoreClaudeSession } from '$lib/state/sessions.svelte';
  import { resolveSessionCwd } from '$lib/services/sessionCwd';
  import { notify } from '$lib/state/toaster.svelte';
  import { sessionUsageTotals, formatTokens, formatCostUsd } from '$lib/usage';
  import BudgetPopover from '$lib/components/agent/BudgetPopover.svelte';
  import ModelEngine from './ModelEngine.svelte';
  import { claudeModels, claudeEffort } from './composerHelpers';
  import { tick } from 'svelte';

  type Kind = 'claude';
  interface Props {
    kind: Kind;
    instanceId: string;
    thinkingStartedAt: Record<string, number | null>;
    thinkingTick: Record<string, number>;
    onStop: () => void;
    /** Open the folder picker to change the session's working dir. */
    onPickCwd?: () => void;
    contextOpen?: boolean;
    onToggleContext?: () => void;
  }
  let p: Props = $props();

  function newChat() {
    const id = newClaudeSession({ agentInstanceId: p.instanceId });
    setActiveSessionInInstance(p.instanceId, id);
    switcherOpen = false;
  }
  function setModel(m: string) {
    if (sess) updateSession(sess.id, { claudeModel: m });
  }
  function setEffort(e: string) {
    if (sess) updateSession(sess.id, { thinkingEffort: e as 'auto' | 'low' | 'medium' | 'high' | 'max' | 'ultracode' });
  }

  const sess = $derived(
    sessionsState.list.find((s) => s.id === sessionsState.activeIds[p.kind]) ?? null
  );

  const budget = $derived(sessionUsageTotals(sess));
  const chipCostUsd = $derived(budget.costUsd);
  const totalTokens = $derived(budget.input + budget.output);

  const elapsed = $derived.by(() => {
    const startedAt = sess ? p.thinkingStartedAt[sess.id] ?? null : null;
    if (!startedAt || !sess?.sending) return '';
    void (sess ? p.thinkingTick[sess.id] : 0);
    const s = Math.floor((Date.now() - startedAt) / 1000);
    return s < 60 ? `${s}s` : `${Math.floor(s / 60)}m ${String(s % 60).padStart(2, '0')}s`;
  });

  const repoLabel = $derived.by(() => {
    const cwd = resolveSessionCwd(sess);
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

  /* Soft-delete → Archive (recoverable from the Archived section below).
     No confirm — it's reversible. Mirrors SessionsSidebar. */
  function archiveChat(id: string) {
    deleteClaudeSession(id);
    notify({ kind: 'success', title: 'Chat archived', ttlMs: 2000 });
  }
  function restoreChat(id: string) {
    restoreClaudeSession(id);
  }

  /* Archived chats — newest-archived first. Rendered in a collapsible
     section at the popover bottom; restore brings one back. */
  const archivedItems = $derived.by(() =>
    sessionsState.list
      .filter((s) => s.archived)
      .sort((a, b) => (b.archivedAt ?? 0) - (a.archivedAt ?? 0))
  );
  let showArchived = $state(false);

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
          <button class="qh-switch-item qh-switch-new" onclick={newChat}>
            <span class="qh-switch-plus" aria-hidden="true">+</span>
            <span class="qh-switch-name">New chat</span>
          </button>
          {#each sessions as s (s.id)}
            <div
              class="qh-switch-item"
              class:active={s.id === sessionsState.activeIds[p.kind]}
              role="option"
              aria-selected={s.id === sessionsState.activeIds[p.kind]}
            >
              <button class="qh-switch-pick" onclick={() => pickSession(s.id)} title={s.title || 'Untitled chat'}>
                <span class="qh-switch-dot" class:running={s.sending} aria-hidden="true"></span>
                <span class="qh-switch-name">{s.title || 'Untitled chat'}</span>
              </button>
              <button
                class="qh-switch-arch"
                title="Archive chat"
                aria-label="Archive chat"
                onclick={() => archiveChat(s.id)}
              >
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M21 8v13H3V8M1 3h22v5H1zM10 12h4"/></svg>
              </button>
            </div>
          {/each}

          {#if archivedItems.length > 0}
            <button
              class="qh-arch-toggle"
              onclick={() => (showArchived = !showArchived)}
              aria-expanded={showArchived}
            >
              <svg
                class="qh-arch-caret"
                class:open={showArchived}
                viewBox="0 0 24 24" width="11" height="11"
                fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"
              ><path d="M9 6l6 6-6 6"/></svg>
              <span>Archived</span>
              <span class="qh-arch-count mono">{archivedItems.length}</span>
            </button>
            {#if showArchived}
              {#each archivedItems as s (s.id)}
                <div class="qh-switch-item qh-arch-row">
                  <span class="qh-arch-icon" aria-hidden="true">
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M21 8v13H3V8M1 3h22v5H1zM10 12h4"/></svg>
                  </span>
                  <span class="qh-switch-name">{s.title || 'Untitled chat'}</span>
                  <button
                    class="qh-switch-arch qh-arch-restore"
                    title="Restore chat"
                    aria-label="Restore chat"
                    onclick={() => restoreChat(s.id)}
                  >
                    <svg viewBox="0 0 24 24" width="13" height="13" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M3 7v6h6M3 13a9 9 0 1 0 3-7.7L3 8"/></svg>
                  </button>
                </div>
              {/each}
            {/if}
          {/if}
        </div>
      {/if}
    </div>

    <button class="qh-new" onclick={newChat} title="New chat" aria-label="New chat">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M12 5v14M5 12h14"/></svg>
      new
    </button>

    <span class="qh-spring"></span>

    {#if sess?.sending}
      <span class="qh-state">streaming{elapsed ? ` · ${elapsed}` : ''}</span>
      <button class="qh-stop" onclick={p.onStop} title="Stop generation" aria-label="Stop">
        <svg viewBox="0 0 24 24" fill="currentColor"><rect x="6" y="6" width="12" height="12" rx="1.5"/></svg>
      </button>
    {/if}

    {#if sess && budget.turns > 0}
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
    {#if repoLabel}
      {#if p.onPickCwd}
        <button class="qh-hot qh-hotbtn" onclick={() => p.onPickCwd?.()} title="Change working folder">{repoLabel}{#if sess?.worktreeBranch}<span class="qh-mono"> · wt {sess.worktreeBranch}</span>{/if}</button>
      {:else}
        <span class="qh-hot">{repoLabel}{#if sess?.worktreeBranch}<span class="qh-mono"> · wt {sess.worktreeBranch}</span>{/if}</span>
      {/if}
    {/if}
    {#if sess}
      <ModelEngine
        model={sess.claudeModel ?? 'claude-opus-4-8'}
        modelOptions={claudeModels}
        effort={sess.thinkingEffort ?? 'auto'}
        effortOptions={claudeEffort}
        onModelChange={setModel}
        onEffortChange={setEffort}
      />
    {/if}
    {#if p.onToggleContext}
      <button class="qh-ctx-chip" class:open={p.contextOpen} onclick={p.onToggleContext} aria-expanded={p.contextOpen}>context <span class="qh-caret" aria-hidden="true">▾</span></button>
    {/if}
  </div>
</header>

<style>
  .qh {
    flex: none;
    display: flex; flex-direction: column; gap: 6px;
    width: 100%; max-width: var(--quiet-measure, min(1100px, 90%)); margin: 0 auto;
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
    scrollbar-width: none; -ms-overflow-style: none;
  }
  .qh-switch-pop::-webkit-scrollbar { display: none; }
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

  /* Row is now a flex container (div); the pick area is an inner button
     so the per-row archive/restore action can sit beside it (button-in-
     button is invalid). Pick inherits the row's text colour. */
  .qh-switch-pick {
    flex: 1; min-width: 0;
    display: flex; align-items: center; gap: 8px;
    background: transparent; border: 0; padding: 0; cursor: pointer;
    text-align: left; color: inherit; font: inherit;
  }
  .qh-switch-arch {
    display: none;
    width: 20px; height: 20px; flex: none;
    place-items: center;
    border: 0; border-radius: 5px;
    background: transparent; color: var(--text-faint); cursor: pointer;
  }
  .qh-switch-arch svg { width: 12px; height: 12px; }
  .qh-switch-item:hover .qh-switch-arch { display: grid; }
  .qh-switch-arch:hover { color: var(--err); background: var(--bg-3); }

  /* Archived collapsible section — mirrors SessionsSidebar. */
  .qh-arch-toggle {
    display: flex; align-items: center; gap: 6px;
    width: 100%;
    margin-top: 6px;
    padding: 6px 10px;
    border: 0; border-radius: 8px;
    background: transparent;
    font-size: 10px; font-weight: 600;
    letter-spacing: 0.10em; text-transform: uppercase;
    color: var(--text-faint); cursor: pointer;
  }
  .qh-arch-toggle:hover { color: var(--text-1); background: var(--bg-hover); }
  .qh-arch-caret { transition: transform 140ms; }
  .qh-arch-caret.open { transform: rotate(90deg); }
  .qh-arch-count { margin-left: auto; }
  .qh-arch-row { opacity: 0.75; }
  .qh-arch-row:hover { opacity: 1; }
  .qh-arch-icon {
    flex: none; display: grid; place-items: center;
    width: 16px; height: 16px; color: var(--text-faint);
  }
  .qh-arch-icon svg { width: 14px; height: 14px; }
  .qh-arch-restore { display: grid; }
  .qh-arch-restore svg { width: 13px; height: 13px; }
  .qh-arch-restore:hover { color: var(--text-0); background: var(--bg-3); }

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
  .qh-hotbtn {
    font: inherit; background: transparent; border: 0; padding: 0 0 1px;
    cursor: pointer;
  }
  .qh-hotbtn:hover { color: var(--text-0); }

  /* New-chat — visible button beside the «N ▾» switcher (the Chats
     list + its "+" live only in the popover in Quiet, so surface a
     first-class new-chat affordance here). */
  .qh-new {
    display: inline-flex; align-items: center; gap: 4px;
    background: transparent; border: 1px solid var(--border);
    border-radius: 6px; padding: 2px 8px 2px 6px;
    color: var(--text-1); font-size: 11.5px; cursor: pointer;
    transition: color 120ms, border-color 120ms, background 120ms;
  }
  .qh-new svg { width: 12px; height: 12px; }
  .qh-new:hover { color: var(--text-0); border-color: var(--border-hi); background: var(--bg-2); }
  .qh-switch-new { color: var(--accent-bright, var(--accent)); }
  .qh-switch-new:hover { background: var(--bg-hover); }
  .qh-switch-plus {
    width: 6px; flex: none; text-align: center;
    font-size: 14px; line-height: 1; font-weight: 700;
    color: var(--accent-bright, var(--accent));
  }
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
