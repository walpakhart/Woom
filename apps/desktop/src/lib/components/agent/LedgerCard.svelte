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

  async function call(cmd: string, args: Record<string, unknown> = {}): Promise<void> {
    if (busy) return;
    busy = true;
    try {
      await invoke(cmd, { workflowId, ...args });
    } catch (e) {
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
  <div class="lg-editor" transition:slide={{ duration: 160 }}>
    <input class="lg-input mono" bind:value={eTitle} placeholder="requirement — what must become true" />
    <textarea class="lg-input lg-input--area mono" bind:value={eDetail} placeholder="detail (optional worker instructions)" rows="2"></textarea>
    <input class="lg-input mono" bind:value={eCheck} placeholder="check command (exit 0 = pass; empty → llm grade)" />
    <label class="lg-flag mono">
      <input type="checkbox" bind:checked={eParallel} />
      parallel-safe (may run in a wave)
    </label>
    <div class="lg-editor-actions">
      <button class="lg-btn lg-btn--ink" disabled={busy || !eTitle.trim()} onclick={saveEditor}>save</button>
      <button class="lg-btn" onclick={closeEditor}>cancel</button>
    </div>
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
      {:else if wf.status === 'running' || wf.status === 'paused_quota' || wf.status === 'building'}
        <button class="lg-btn" disabled={busy} onclick={() => call('ledger_cancel')}>cancel</button>
      {:else if wf.status === 'awaiting_review'}
        <button class="lg-btn lg-btn--ink" disabled={busy} onclick={() => call('ledger_apply')}>apply</button>
        <button class="lg-btn" disabled={busy} onclick={() => call('ledger_discard')}>discard</button>
      {/if}
    </header>

    {#if wf.items.length > 0}
      <div class="lg-progress" role="progressbar" aria-valuemin="0" aria-valuemax={wf.items.length} aria-valuenow={settled}>
        <div class="lg-progress-fill" style="width: {(settled / wf.items.length) * 100}%"></div>
      </div>
    {/if}

    {#if wf.status === 'building'}
      <p class="lg-hint" transition:fade={{ duration: 150 }}>agent is building the checklist…</p>
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
              {#if item.checkOutput}
                <pre class="lg-pre mono">{item.checkOutput}</pre>
              {/if}
              {#if item.diff}
                <pre class="lg-pre lg-pre--diff mono">{item.diff}</pre>
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
        <pre class="lg-pre lg-pre--diff mono" transition:slide={{ duration: 180 }}>{wf.fullDiff}</pre>
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
        <p class="lg-hint">diff applied to {wf.parentCwd}</p>
      </div>
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
  .lg-editor {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 6px 8px 8px 0;
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
  .mono {
    font-family: var(--font-mono, ui-monospace, monospace);
  }
</style>
