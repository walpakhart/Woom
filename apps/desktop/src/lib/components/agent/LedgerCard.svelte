<script lang="ts">
  /* Ledger card — chat-inline checklist for the sequential machine-
   * checked workflow. Pre-run the checklist is fully EDITABLE (edit /
   * remove / reorder / add — the approval gate is only as strong as
   * its ability to fix the plan); while running, items stream a live
   * action feed; the review gate exposes the branch diff + Apply. */
  import { invoke } from '@tauri-apps/api/core';
  import { slide, fade } from 'svelte/transition';
  import { ledgerState, setLedgerSquash, injectLedgerNote } from '$lib/state/ledger.svelte';
  import { formatCostUsd } from '$lib/usage';

  interface Props {
    workflowId: string;
  }
  const { workflowId }: Props = $props();

  const wf = $derived(ledgerState.workflows.find((w) => w.id === workflowId) ?? null);

  let expandedId = $state<string | null>(null);
  let showFullDiff = $state(false);
  let busy = $state(false);
  /* Last command failure, surfaced on the card — apply/resume/etc used
     to fail silently (console.warn only), so a broken apply looked like
     a dead button. */
  let actionErr = $state<string | null>(null);
  /* Raise-cap input shown on a budget pause (empty → auto-bump). */
  let capInput = $state('');
  /* Inline editor state — one item at a time. */
  let editId = $state<string | null>(null);
  let eTitle = $state('');
  let eDetail = $state('');
  let eCheck = $state('');
  let eParallel = $state(false);
  let adding = $state(false);

  const passed = $derived(wf ? wf.items.filter((i) => i.status === 'passed').length : 0);
  const settled = $derived(
    wf ? wf.items.filter((i) => i.status === 'passed' || i.status === 'skipped').length : 0
  );
  const editableWf = $derived(
    wf ? ['building', 'awaiting_launch', 'failed'].includes(wf.status) : false
  );
  const live = $derived(
    wf ? ['building', 'running', 'paused_quota'].includes(wf.status) : false
  );
  /** +/-/file counts pulled straight out of the unified diff text. */
  const diffStats = $derived.by(() => {
    const d = wf?.fullDiff;
    if (!d) return null;
    let files = 0, add = 0, del = 0;
    for (const line of d.split('\n')) {
      if (line.startsWith('diff --git')) files++;
      else if (line.startsWith('+') && !line.startsWith('+++')) add++;
      else if (line.startsWith('-') && !line.startsWith('---')) del++;
    }
    return { files, add, del };
  });

  function itemEditable(status: string): boolean {
    return editableWf && ['queued', 'failed', 'skipped'].includes(status);
  }

  function glyph(status: string): string {
    switch (status) {
      case 'passed': return '●';
      case 'failed': return '✕';
      case 'skipped': return '−';
      case 'working': return '◐';
      case 'checking': return '◒';
      default: return '○';
    }
  }

  /** Per-line class for the colorized diff view. Meta lines (+++/---,
   *  headers) are checked before +/- so they don't read as add/del. */
  function diffCls(ln: string): string {
    if (
      ln.startsWith('diff --git') ||
      ln.startsWith('index ') ||
      ln.startsWith('--- ') ||
      ln.startsWith('+++ ') ||
      ln.startsWith('new file') ||
      ln.startsWith('deleted file') ||
      ln.startsWith('rename ') ||
      ln.startsWith('similarity ')
    )
      return 'lg-dl--meta';
    if (ln.startsWith('@@')) return 'lg-dl--hunk';
    if (ln.startsWith('+')) return 'lg-dl--add';
    if (ln.startsWith('-')) return 'lg-dl--del';
    return '';
  }

  interface DiffFile {
    path: string;
    add: number;
    del: number;
    lines: string[];
  }
  /** Split a unified diff into per-file sections so the card can show a
   *  collapsible list instead of one flat sheet. Path is taken from the
   *  `diff --git a/… b/…` header (b-side). */
  function splitDiffFiles(text: string): DiffFile[] {
    const files: DiffFile[] = [];
    let cur: DiffFile | null = null;
    for (const ln of text.split('\n')) {
      if (ln.startsWith('diff --git')) {
        const m = ln.match(/ b\/(.+)$/);
        cur = { path: m ? m[1] : ln.replace('diff --git ', ''), add: 0, del: 0, lines: [] };
        files.push(cur);
        continue;
      }
      if (!cur) {
        cur = { path: '(diff)', add: 0, del: 0, lines: [] };
        files.push(cur);
      }
      cur.lines.push(ln);
      if (ln.startsWith('+') && !ln.startsWith('+++')) cur.add++;
      else if (ln.startsWith('-') && !ln.startsWith('---')) cur.del++;
    }
    return files;
  }

  /** Resume a paused run. `raiseCap` folds the (optional) cap input, or
   *  auto-bumps past current spend when left empty. */
  async function resumeLedger(raiseCap: boolean): Promise<void> {
    const args: Record<string, unknown> = {};
    if (raiseCap && wf) {
      const typed = parseFloat(capInput);
      const cap = Number.isFinite(typed) && typed > 0 ? typed : Math.ceil(wf.totalCostUsd + 20);
      args.budgetCapUsd = cap;
    }
    capInput = '';
    await call('ledger_resume', args);
  }

  async function call(cmd: string, args: Record<string, unknown> = {}): Promise<void> {
    if (busy) return;
    busy = true;
    actionErr = null;
    try {
      await invoke(cmd, { workflowId, ...args });
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      actionErr = `${cmd.replace('ledger_', '')} failed: ${msg}`;
      console.warn(`${cmd} failed`, e);
    } finally {
      busy = false;
    }
  }

  function openEdit(item: { id: string; title: string; detail?: string | null; checkCmd?: string | null; parallel: boolean }): void {
    editId = item.id;
    adding = false;
    eTitle = item.title;
    eDetail = item.detail ?? '';
    eCheck = item.checkCmd ?? '';
    eParallel = item.parallel;
  }

  function openAdd(): void {
    adding = true;
    editId = null;
    eTitle = '';
    eDetail = '';
    eCheck = '';
    eParallel = false;
  }

  function closeEditor(): void {
    editId = null;
    adding = false;
  }

  /* Mid-run steering. */
  let injectText = $state('');
  let injecting = $state(false);
  async function sendInject(): Promise<void> {
    const note = injectText.trim();
    if (!note || injecting) return;
    injecting = true;
    try {
      await injectLedgerNote(workflowId, note);
      injectText = '';
    } catch (e) {
      console.warn('ledger_inject failed', e);
    } finally {
      injecting = false;
    }
  }
  function onInjectKey(e: KeyboardEvent): void {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      void sendInject();
    }
  }

  async function saveEditor(): Promise<void> {
    if (!eTitle.trim()) return;
    if (adding) {
      await call('ledger_add_item', {
        title: eTitle,
        detail: eDetail || null,
        checkCmd: eCheck || null,
        maxAttempts: null,
        parallel: eParallel,
      });
    } else if (editId) {
      await call('ledger_update_item', {
        itemId: editId,
        title: eTitle,
        detail: eDetail,
        checkCmd: eCheck,
        maxAttempts: null,
        parallel: eParallel,
      });
    }
    closeEditor();
  }
</script>

{#snippet editorFields()}
  <div class="lg-form" transition:fade={{ duration: 120 }}>
    <div class="lg-form-head mono">{adding ? 'new step' : 'edit step'}</div>
    <label class="lg-field">
      <span class="lg-field-label mono">requirement</span>
      <input class="lg-input mono" bind:value={eTitle} placeholder="what must become true" />
    </label>
    <label class="lg-field">
      <span class="lg-field-label mono">detail <span class="lg-field-opt">optional</span></span>
      <textarea class="lg-input lg-input--area mono" bind:value={eDetail} placeholder="worker instructions + relevant file paths" rows="3"></textarea>
    </label>
    <label class="lg-field">
      <span class="lg-field-label mono">check <span class="lg-field-opt">exit 0 = pass · empty → llm grade</span></span>
      <input class="lg-input mono" bind:value={eCheck} placeholder="cd apps/desktop && pnpm svelte-check" />
    </label>
    <label class="lg-flag mono">
      <input type="checkbox" bind:checked={eParallel} />
      parallel-safe (may run in a wave)
    </label>
    <div class="lg-editor-actions">
      <button class="lg-btn lg-btn--ink" disabled={busy || !eTitle.trim()} onclick={saveEditor}>{adding ? 'add step' : 'save'}</button>
      <button class="lg-btn" onclick={closeEditor}>cancel</button>
    </div>
  </div>
{/snippet}

{#snippet diffBlock(text: string, openByDefault: boolean)}
  <div class="lg-diffwrap" transition:slide={{ duration: 160 }}>
    {#each splitDiffFiles(text) as f, fi (f.path + fi)}
      <details class="lg-file" open={openByDefault}>
        <summary class="lg-file-head mono">
          <span class="lg-file-path">{f.path}</span>
          <span class="lg-file-stat">
            <span class="lg-add">+{f.add}</span>
            <span class="lg-del">−{f.del}</span>
          </span>
        </summary>
        <div class="lg-diff mono">
          {#each f.lines as ln, i (i)}
            <div class="lg-dl {diffCls(ln)}">{ln || ' '}</div>
          {/each}
        </div>
      </details>
    {/each}
  </div>
{/snippet}

{#if wf}
  <div class="lg-card" data-status={wf.status}>
    <header class="lg-head">
      <span class="lg-badge mono">
        {#if live}<span class="lg-livedot" aria-hidden="true"></span>{/if}
        ledger · {wf.status.replace(/_/g, ' ')}
      </span>
      <span class="lg-task">{wf.task}</span>
      <span class="lg-totals mono">
        {passed}/{wf.items.length}
        {#if wf.totalCostUsd > 0}· {formatCostUsd(wf.totalCostUsd)}{/if}
      </span>
      {#if wf.status === 'awaiting_launch'}
        <button class="lg-btn lg-btn--ink" disabled={busy} onclick={() => call('ledger_run')}>run</button>
        <button class="lg-btn" disabled={busy} onclick={() => call('ledger_cancel')}>cancel</button>
      {:else if wf.status === 'running' || wf.status === 'paused_quota'}
        <button class="lg-btn" disabled={busy} onclick={() => call('ledger_pause')}>pause</button>
        <button class="lg-btn" disabled={busy} onclick={() => call('ledger_cancel')}>cancel</button>
      {:else if wf.status === 'building'}
        <button class="lg-btn" disabled={busy} onclick={() => call('ledger_cancel')}>cancel</button>
      {:else if wf.status === 'paused'}
        <button class="lg-btn lg-btn--ink" disabled={busy} onclick={() => resumeLedger(false)}>resume</button>
        <button class="lg-btn" disabled={busy} onclick={() => call('ledger_cancel')}>cancel</button>
      {:else if wf.status === 'paused_budget'}
        <input
          class="lg-input lg-cap mono"
          bind:value={capInput}
          placeholder={`cap $${Math.ceil(wf.totalCostUsd + 20)}`}
          inputmode="decimal"
          title="new budget cap in USD (empty → auto-bump past current spend)"
        />
        <button class="lg-btn lg-btn--ink" disabled={busy} onclick={() => resumeLedger(true)}>resume</button>
        <button class="lg-btn" disabled={busy} onclick={() => call('ledger_cancel')}>cancel</button>
      {:else if wf.status === 'awaiting_review'}
        <button class="lg-btn lg-btn--ink" disabled={busy} onclick={() => call('ledger_apply')}>apply</button>
        <button class="lg-btn" disabled={busy} onclick={() => call('ledger_discard')}>discard</button>
      {/if}
    </header>

    {#if actionErr}
      <p class="lg-err" transition:slide={{ duration: 140 }}>{actionErr}</p>
    {/if}

    {#if wf.items.length > 0}
      <div class="lg-progress" role="progressbar" aria-valuemin="0" aria-valuemax={wf.items.length} aria-valuenow={settled}>
        <div class="lg-progress-fill" style="width: {(settled / wf.items.length) * 100}%"></div>
      </div>
    {/if}

    {#if wf.status === 'building'}
      <p class="lg-hint" transition:fade={{ duration: 150 }}>agent is building the checklist…</p>
    {:else if wf.status === 'paused_budget'}
      <p class="lg-hint lg-hint--warn" transition:fade={{ duration: 150 }}>
        budget cap ${wf.budgetCapUsd.toFixed(0)} reached at {formatCostUsd(wf.totalCostUsd)} — raise the cap and resume; committed items are kept.
      </p>
    {:else if wf.status === 'paused'}
      <p class="lg-hint" transition:fade={{ duration: 150 }}>paused — resume to continue where it left off.</p>
    {/if}

    {#if wf.status === 'building' && wf.items.length === 0}
      <div class="lg-skeleton" aria-hidden="true">
        {#each [0, 1, 2] as i (i)}
          <div class="lg-skel-row" style="animation-delay: {i * 140}ms">
            <span class="lg-skel-dot"></span>
            <span class="lg-skel-bar" style="width: {72 - i * 14}%"></span>
          </div>
        {/each}
      </div>
    {/if}

    <ul class="lg-items">
      {#each wf.items as item, idx (item.id)}
        <li
          class="lg-item"
          data-status={item.status}
          class:lg-item--current={wf.currentItem === item.id}
          class:lg-item--wave={item.parallel && (wf.items[idx - 1]?.parallel || wf.items[idx + 1]?.parallel)}
          in:slide={{ duration: 180 }}
        >
          {#if editId === item.id}
            {@render editorFields()}
          {:else}
            <div class="lg-rowline">
              <button
                class="lg-row"
                onclick={() => (expandedId = expandedId === item.id ? null : item.id)}
                aria-expanded={expandedId === item.id}
              >
                <span class="lg-num mono">{String(idx + 1).padStart(2, '0')}</span>
                <span class="lg-glyph mono" class:lg-glyph--live={item.status === 'working' || item.status === 'checking'}>{glyph(item.status)}</span>
                <span class="lg-title">{item.title}</span>
                {#if item.parallel}<span class="lg-par mono" title="parallel-safe — may run in a wave">∥</span>{/if}
                {#if item.checkCmd}
                  <span class="lg-check mono" title={item.checkCmd}>{item.checkCmd}</span>
                {:else}
                  <span class="lg-check lg-check--grader mono">llm grade</span>
                {/if}
                {#if item.attempts > 1 || item.status === 'failed'}
                  <span class="lg-attempts mono">try {item.attempts}/{item.maxAttempts}</span>
                {/if}
              </button>
              {#if itemEditable(item.status)}
                <span class="lg-tools">
                  <button class="lg-tool" title="Edit item" onclick={() => openEdit(item)}>✎</button>
                  <button class="lg-tool" title="Move up" disabled={busy} onclick={() => call('ledger_move_item', { itemId: item.id, direction: 'up' })}>↑</button>
                  <button class="lg-tool" title="Move down" disabled={busy} onclick={() => call('ledger_move_item', { itemId: item.id, direction: 'down' })}>↓</button>
                  <button class="lg-tool" title="Remove item" disabled={busy} onclick={() => call('ledger_remove_item', { itemId: item.id })}>×</button>
                </span>
              {/if}
            </div>
          {/if}
          {#if (item.status === 'working' || item.status === 'checking') && item.feed.length > 0}
            <div class="lg-feed mono" transition:slide={{ duration: 160 }}>
              {#each item.feed.slice(-6) as line, li (item.feed.length - 6 + li)}
                <div class="lg-feed-line" in:fade={{ duration: 200 }}>{line}</div>
              {/each}
            </div>
          {/if}
          {#if wf.status === 'failed' && item.status === 'failed'}
            {#if item.error}<p class="lg-err mono">{item.error}</p>{/if}
            <div class="lg-item-actions">
              <button class="lg-btn" disabled={busy} onclick={() => call('ledger_retry_item', { itemId: item.id })}>retry</button>
              <button class="lg-btn" disabled={busy} onclick={() => call('ledger_skip_item', { itemId: item.id })}>skip</button>
            </div>
          {/if}
          {#if expandedId === item.id && editId !== item.id}
            <div class="lg-detail" transition:slide={{ duration: 160 }}>
              {#if item.detail}<p class="lg-detail-text">{item.detail}</p>{/if}
              {#if item.notes}
                <p class="lg-notes mono">notes → {item.notes}</p>
              {/if}
              {#if item.error}
                <p class="lg-err mono">{item.error}</p>
              {/if}
              {#if item.checkOutput}
                <pre class="lg-pre mono">{item.checkOutput}</pre>
              {/if}
              {#if item.diff}
                {@render diffBlock(item.diff, true)}
              {/if}
            </div>
          {/if}
        </li>
      {/each}
      {#if editableWf}
        <li class="lg-item">
          {#if adding}
            {@render editorFields()}
          {:else}
            <button class="lg-add mono" onclick={openAdd}>+ add item</button>
          {/if}
        </li>
      {/if}
    </ul>

    {#if wf.status === 'awaiting_review' && wf.fullDiff}
      <div class="lg-review" transition:slide={{ duration: 160 }}>
        <button class="lg-difftoggle mono" onclick={() => (showFullDiff = !showFullDiff)}>
          {showFullDiff ? 'hide' : 'review'} full diff
        </button>
        {#if diffStats}
          <span class="lg-diffstats mono">
            {diffStats.files} {diffStats.files === 1 ? 'file' : 'files'}
            <span class="lg-add">+{diffStats.add}</span>
            <span class="lg-del">−{diffStats.del}</span>
          </span>
        {/if}
        <label
          class="lg-squash mono"
          title="On: apply lands as one clean commit. Off: keep every per-item commit + a merge commit."
        >
          <input type="checkbox" checked={wf.squash} disabled={busy} onchange={(e) => setLedgerSquash(workflowId, e.currentTarget.checked)} />
          one commit
        </label>
      </div>
      {#if showFullDiff}
        {@render diffBlock(wf.fullDiff ?? '', false)}
      {/if}
    {/if}

    {#if wf.status === 'running' || wf.status === 'paused_quota'}
      <div class="lg-inject" transition:slide={{ duration: 160 }}>
        <input
          class="lg-input lg-inject-input mono"
          bind:value={injectText}
          onkeydown={onInjectKey}
          placeholder="steer the running ledger — folded into the next worker turn"
          disabled={injecting}
        />
        <button class="lg-btn" disabled={injecting || !injectText.trim()} onclick={sendInject}>nudge</button>
      </div>
      {#if wf.injections.length > 0}
        <p class="lg-hint mono">{wf.injections.length} note{wf.injections.length === 1 ? '' : 's'} queued for the next turn</p>
      {/if}
    {/if}
    {#if wf.status === 'done' && wf.applied}
      <div class="lg-done" in:fade={{ duration: 250 }}>
        <span class="lg-stamp mono" aria-hidden="true">applied</span>
        <span class="lg-donesum mono">
          {passed}/{wf.items.length} items · {formatCostUsd(wf.totalCostUsd)}{#if diffStats} · <span class="lg-add">+{diffStats.add}</span> <span class="lg-del">−{diffStats.del}</span> in {diffStats.files} {diffStats.files === 1 ? 'file' : 'files'}{/if}
        </span>
        {#if wf.fullDiff}
          <button class="lg-difftoggle mono" onclick={() => (showFullDiff = !showFullDiff)}>{showFullDiff ? 'hide' : 'review'} diff</button>
        {/if}
      </div>
      <p class="lg-hint">applied to {wf.parentCwd}</p>
      {#if showFullDiff && wf.fullDiff}
        {@render diffBlock(wf.fullDiff, false)}
      {/if}
    {/if}
  </div>
{/if}

<style>
  .lg-card {
    margin: 8px 0;
    padding: 10px 14px 12px;
    border: 1px solid var(--border);
    border-left: 2px solid color-mix(in srgb, var(--accent) 75%, transparent);
    border-radius: 4px;
    background: var(--bg-1, transparent);
    box-shadow: var(--shadow-1, none);
    min-width: 0;
    max-width: 100%;
    overflow: hidden;
  }
  .lg-card[data-status='done'] {
    border-left-color: color-mix(in srgb, var(--ok, #6cb87a) 60%, var(--border));
  }
  .lg-card[data-status='failed'],
  .lg-card[data-status='cancelled'] {
    border-left-color: color-mix(in srgb, var(--error, #e88264) 50%, var(--border));
  }
  .lg-head {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
    margin-bottom: 4px;
  }
  .lg-badge {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 1px 7px;
    border-radius: 3px;
    font-size: 9.5px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    background: var(--bg-3);
    color: var(--text-1);
  }
  .lg-livedot {
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: var(--text-0);
    animation: lg-pulse 1.2s ease-in-out infinite;
  }
  .lg-progress {
    height: 3px;
    margin: 3px 0 7px;
    background: color-mix(in srgb, var(--border) 70%, transparent);
    border-radius: 2px;
    overflow: hidden;
  }
  .lg-progress-fill {
    height: 100%;
    background: var(--text-0);
    transition: width 400ms ease;
  }
  .lg-task {
    flex: 1 1 120px;
    /* min-width:0 beats the flex default of min-width:auto — without it
       the nowrap text refuses to shrink and pushes the action buttons
       clean out of the card. */
    min-width: 0;
    font-size: 12px;
    color: var(--text-0);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .lg-totals {
    font-size: 10.5px;
    color: var(--text-2);
  }
  .lg-btn {
    font: inherit;
    font-size: 10.5px;
    padding: 2px 10px;
    border: 1px solid var(--border);
    border-radius: 3px;
    background: transparent;
    color: var(--text-1);
    cursor: pointer;
  }
  .lg-btn:hover { background: var(--bg-3); }
  .lg-btn--ink {
    background: var(--text-0);
    color: var(--bg-0);
    border-color: var(--text-0);
    box-shadow: var(--shadow-pill, none);
  }
  .lg-btn--ink:hover { background: var(--text-1); }
  .lg-review {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-top: 6px;
  }
  .lg-diffstats {
    font-size: 10.5px;
    color: var(--text-2);
  }
  .lg-add { color: var(--ok, #6cb87a); }
  .lg-del { color: var(--error, #e88264); }
  .lg-squash {
    margin-left: auto;
    display: inline-flex;
    align-items: center;
    gap: 5px;
    font-size: 10.5px;
    color: var(--text-2);
    cursor: pointer;
    white-space: nowrap;
  }
  .lg-squash input { cursor: pointer; }
  .lg-inject {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 8px;
  }
  .lg-inject-input { flex: 1; min-width: 0; margin: 0; }
  .lg-done {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-top: 6px;
  }
  /* Letterpress stamp — the ledger's closing mark. */
  .lg-stamp {
    flex: none;
    padding: 2px 10px;
    border: 1.5px solid var(--text-1);
    border-radius: 3px;
    font-size: 10px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.18em;
    color: var(--text-1);
    transform: rotate(-4deg);
    opacity: 0.85;
  }
  .lg-btn:disabled { opacity: 0.5; cursor: default; }
  .lg-hint {
    margin: 4px 0;
    font-size: 11px;
    color: var(--text-2);
    font-style: italic;
  }
  .lg-skeleton { padding: 4px 0 2px; }
  .lg-skel-row {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 5px 0;
    animation: lg-skel 1.4s ease-in-out infinite;
  }
  .lg-skel-dot {
    width: 9px;
    height: 9px;
    border-radius: 50%;
    background: var(--border);
    flex: none;
  }
  .lg-skel-bar {
    height: 8px;
    border-radius: 2px;
    background: var(--border);
  }
  @keyframes lg-skel {
    0%, 100% { opacity: 0.45; }
    50% { opacity: 1; }
  }
  .lg-items {
    list-style: none;
    margin: 4px 0 0;
    padding: 0;
  }
  .lg-item { margin: 0; position: relative; }
  /* Current item — ink caret in the left gutter. */
  .lg-item--current::before {
    content: '▸';
    position: absolute;
    left: -12px;
    top: 4px;
    font-size: 10px;
    color: var(--text-0);
    animation: lg-pulse 1.2s ease-in-out infinite;
  }
  /* Wave members — a thin double rail on the left ties the parallel
     burst together visually. */
  .lg-item--wave {
    border-left: 3px double color-mix(in srgb, var(--text-2) 55%, transparent);
    padding-left: 7px;
    margin-left: -10px;
  }
  .lg-rowline {
    display: flex;
    align-items: center;
    min-width: 0;
  }
  .lg-row {
    display: flex;
    align-items: center;
    gap: 8px;
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    padding: 3px 6px 3px 0;
    border: 0;
    background: transparent;
    font: inherit;
    text-align: left;
    cursor: pointer;
    color: var(--text-0);
  }
  .lg-row:hover { background: color-mix(in srgb, var(--accent) 4%, transparent); }
  .lg-num {
    flex: none;
    font-size: 9.5px;
    color: var(--text-2);
    opacity: 0.7;
    width: 16px;
  }
  .lg-glyph {
    width: 14px;
    flex: none;
    font-size: 11px;
    color: var(--text-2);
  }
  .lg-item[data-status='passed'] .lg-glyph { color: var(--ok, #6cb87a); }
  .lg-item[data-status='failed'] .lg-glyph { color: var(--error, #e88264); }
  .lg-glyph--live { animation: lg-pulse 1.2s ease-in-out infinite; }
  @keyframes lg-pulse {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.35; }
  }
  .lg-title {
    flex: 1 1 auto;
    font-size: 12px;
    min-width: 0;
  }
  .lg-item[data-status='skipped'] .lg-title {
    text-decoration: line-through;
    color: var(--text-2);
  }
  .lg-par {
    flex: none;
    font-size: 11px;
    color: var(--text-2);
  }
  .lg-check {
    flex: 0 1 auto;
    min-width: 0;
    max-width: 220px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 10px;
    color: var(--text-2);
    border: 1px solid var(--border);
    border-radius: 3px;
    padding: 0 6px;
  }
  .lg-check--grader { border-style: dashed; }
  .lg-attempts {
    flex: none;
    font-size: 10px;
    color: var(--text-2);
  }
  .lg-tools {
    display: flex;
    gap: 2px;
    flex: none;
    opacity: 0;
    transition: opacity 120ms;
  }
  .lg-rowline:hover .lg-tools { opacity: 1; }
  .lg-tool {
    font: inherit;
    font-size: 11px;
    width: 20px;
    height: 20px;
    border: 0;
    border-radius: 3px;
    background: transparent;
    color: var(--text-2);
    cursor: pointer;
  }
  .lg-tool:hover { background: var(--bg-3); color: var(--text-0); }
  .lg-tool:disabled { opacity: 0.4; cursor: default; }
  .lg-add {
    font: inherit;
    font-size: 10.5px;
    margin-top: 2px;
    padding: 2px 8px;
    border: 1px dashed var(--border);
    border-radius: 3px;
    background: transparent;
    color: var(--text-2);
    cursor: pointer;
  }
  .lg-add:hover { color: var(--text-0); border-color: var(--text-2); }
  /* Add/edit-step form — a settled bordered panel (no slide-out), the
     fields labelled so it reads as a form, not a raw input stack. */
  .lg-form {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin: 4px 0 6px;
    padding: 10px 12px;
    background: var(--bg-1);
    border: 1px solid var(--border);
    border-radius: 8px;
    box-shadow: var(--shadow-1, none);
  }
  .lg-form-head {
    font-size: 9.5px;
    font-weight: 600;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--text-2);
  }
  .lg-field {
    display: flex;
    flex-direction: column;
    gap: 3px;
  }
  .lg-field-label {
    font-size: 10px;
    color: var(--text-1);
  }
  .lg-field-opt {
    color: var(--text-linenum, var(--text-mute));
    font-weight: 400;
  }
  .lg-input {
    font: inherit;
    font-size: 11px;
    padding: 4px 8px;
    border: 1px solid var(--border);
    border-radius: 3px;
    background: var(--bg-2);
    color: var(--text-0);
  }
  .lg-input--area { resize: vertical; }
  .lg-flag {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 10.5px;
    color: var(--text-1);
  }
  .lg-editor-actions {
    display: flex;
    gap: 6px;
  }
  .lg-feed {
    padding: 2px 0 4px 22px;
    font-size: 10px;
    line-height: 1.6;
    color: var(--text-2);
  }
  .lg-feed-line {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .lg-item-actions {
    display: flex;
    gap: 6px;
    padding: 2px 0 4px 22px;
  }
  .lg-detail { padding: 2px 0 6px 22px; }
  .lg-detail-text {
    margin: 2px 0 6px;
    font-size: 11.5px;
    color: var(--text-1);
    white-space: pre-wrap;
  }
  .lg-notes {
    margin: 2px 0 6px;
    font-size: 10.5px;
    color: var(--text-2);
    white-space: pre-wrap;
  }
  .lg-pre {
    margin: 4px 0 0;
    padding: 8px 10px;
    max-height: 260px;
    overflow: auto;
    font-size: 10.5px;
    line-height: 1.5;
    background: var(--bg-2);
    border: 1px solid var(--border);
    border-radius: 4px;
    white-space: pre-wrap;
    word-break: break-word;
    color: var(--text-1);
  }
  .lg-difftoggle {
    font: inherit;
    font-size: 10.5px;
    border: 0;
    background: transparent;
    color: var(--text-1);
    text-decoration: underline;
    cursor: pointer;
    padding: 0;
  }
  /* Per-file collapsible diff — file sections you expand one at a time
     instead of one flat sheet; colorized line-per-row inside. */
  .lg-diffwrap {
    margin: 4px 0 0;
    max-height: 420px;
    overflow: auto;
    border: 1px solid var(--border);
    border-radius: 4px;
    background: var(--bg-1);
  }
  .lg-file + .lg-file { border-top: 1px solid var(--border); }
  .lg-file-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    padding: 5px 10px;
    font-size: 11px;
    color: var(--text-1);
    cursor: pointer;
    list-style: none;
    user-select: none;
  }
  .lg-file-head::-webkit-details-marker { display: none; }
  .lg-file-head::before {
    content: '▸';
    margin-right: 6px;
    color: var(--text-2);
    font-size: 9px;
  }
  .lg-file[open] > .lg-file-head::before { content: '▾'; }
  .lg-file-head:hover { background: var(--bg-2); }
  .lg-file-path {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    flex: 1;
    min-width: 0;
  }
  .lg-file-stat { flex-shrink: 0; font-size: 10px; }
  .lg-diff {
    font-size: 10.5px;
    line-height: 1.55;
    background: var(--bg-2);
    border-top: 1px solid var(--border);
    padding: 6px 0;
  }
  .lg-dl {
    padding: 0 10px;
    white-space: pre-wrap;
    word-break: break-word;
    color: var(--text-1);
  }
  .lg-dl--add {
    color: var(--ok, #6cb87a);
    background: color-mix(in srgb, var(--ok, #6cb87a) 12%, transparent);
  }
  .lg-dl--del {
    color: var(--error, #e88264);
    background: color-mix(in srgb, var(--error, #e88264) 12%, transparent);
  }
  .lg-dl--hunk {
    color: var(--accent, #7a9cc6);
    background: color-mix(in srgb, var(--accent, #7a9cc6) 8%, transparent);
  }
  .lg-dl--meta {
    color: var(--text-2);
    font-weight: 600;
  }
  .lg-err {
    margin: 2px 0 6px;
    padding: 4px 8px 4px 22px;
    font-size: 10.5px;
    color: var(--error, #e88264);
    white-space: pre-wrap;
    word-break: break-word;
  }
  .lg-hint--warn {
    color: var(--warn, #d99a4e);
    font-style: normal;
  }
  .lg-cap {
    width: 92px;
    margin: 0;
    padding: 2px 8px;
    font-size: 10.5px;
  }
  .lg-donesum {
    font-size: 10.5px;
    color: var(--text-2);
  }
  .mono {
    font-family: var(--font-mono, ui-monospace, monospace);
  }
</style>
