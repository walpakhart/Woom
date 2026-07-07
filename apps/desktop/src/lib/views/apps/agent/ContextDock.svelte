<script lang="ts">
  /* ContextDock — 300px right pane of the Cabin Claude solo (redesign
     v2 §2.5). Absorbs WorktreeBar + WorktreeSide + the mem/linked bits
     of ChatHeader into one quiet column of labelled sections:
       1 Repo    — cwd/worktree + actions (diff / editor / branch / remove)
       2 Links   — editor / terminal / canvas chips or link pickers
       3 Run     — model + effort (ModelEngine) + rtk/fast toggles
       4 Budget  — turns · $ · tokens + ctx/5h/week bars
       5 Memory  — project memory count + last preview
       6 Tasks   — background tasks (preview opens the PreviewPane overlay)
     All logic reuses the callbacks +page already threads down and the
     existing stores — no IPC/state rewrite. */
  import { sessionsState, updateSession, focusSession, editorRoots } from '$lib/state/sessions.svelte';
  import { layoutState } from '$lib/state/layout.svelte';
  import { canvasState } from '$lib/state/canvas.svelte';
  import { bgTasksState } from '$lib/state/bgTasks.svelte';
  import { quotaState } from '$lib/state/quota.svelte';
  import { sessionUsageTotals, contextWindowFor, formatCostUsd, formatTokens } from '$lib/usage';
  import { sessionDwTotals } from '$lib/state/dw.svelte';
  import { claudeModels, claudeEffort } from './composerHelpers';
  import ModelEngine from './ModelEngine.svelte';
  import BudgetPopover from '$lib/components/agent/BudgetPopover.svelte';
  import Dropdown from '$lib/components/ui/Dropdown.svelte';
  import { invoke } from '@tauri-apps/api/core';

  type Kind = 'claude';
  interface Props {
    kind: Kind;
    instanceId: string;
    editorRepoPath: string;
    worktreeBusy: 'creating' | 'removing' | null;
    onPickCwd: () => void;
    onClearCwd: () => void;
    onToggleEditorLink: () => void;
    onLinkToEditorInstance: (id: string) => void;
    onSyncAgentToEditor?: () => void;
    onSyncEditorToAgent?: () => void;
    onToggleTerminalLink?: () => void;
    onLinkToTerminalInstance?: (id: string) => void;
    onToggleCanvasLink?: () => void;
    onLinkToCanvas?: (canvasId: string) => void;
    onCreateWorktree: () => void;
    onOpenWorktreeDiff: () => void;
    onOpenWorktreeInEditor: () => void;
    onCopyWorktreeBranch: () => void;
    onRemoveWorktree: () => void;
    onCollapse?: () => void;
  }
  let p: Props = $props();

  const sess = $derived(
    sessionsState.list.find((s) => s.id === sessionsState.activeIds[p.kind]) ?? null
  );

  /* ---------- helpers (mirrors WorktreeBar) ---------- */
  function folderName(x: string | null | undefined): string {
    if (!x) return '';
    const t = x.replace(/\/+$/, '');
    const i = t.lastIndexOf('/');
    return i >= 0 ? t.slice(i + 1) : t;
  }
  function shortPath(x: string | null | undefined): string {
    if (!x) return '';
    if (x.startsWith('/Users/')) {
      const rest = x.slice('/Users/'.length);
      const slash = rest.indexOf('/');
      return slash >= 0 ? `~${rest.slice(slash)}` : '~';
    }
    return x;
  }
  function rootsLabel(id: string): string {
    const rs = editorRoots(id);
    if (rs.length === 0) return '';
    if (rs.length === 1) return folderName(rs[0]);
    return `${folderName(rs[0])} +${rs.length - 1}`;
  }
  function focusLocal() { if (sess) focusSession(sess.id); }

  const repoName = $derived(folderName(sess?.worktreePath || sess?.cwd || p.editorRepoPath));
  const repoPath = $derived(sess?.worktreePath || sess?.cwd || p.editorRepoPath || '');

  const editorInstances = $derived(
    layoutState.instances.editor.map((i) => ({
      id: i.id,
      name: i.name,
      repoPath: sessionsState.editorInstanceState[i.id]?.repoPath ?? '',
      folder: rootsLabel(i.id)
    }))
  );
  const linkedEditor = $derived.by(() => {
    if (!sess?.linkedToEditor || !sess.linkedToEditorInstanceId) return null;
    const inst = layoutState.instances.editor.find((i) => i.id === sess!.linkedToEditorInstanceId);
    if (!inst) return null;
    return { id: inst.id, name: inst.name, repoPath: sessionsState.editorInstanceState[inst.id]?.repoPath ?? '', folder: rootsLabel(inst.id) };
  });
  const agentFolder = $derived(sess?.worktreePath || sess?.cwd || '');
  const folderMismatch = $derived(
    !!linkedEditor && !!linkedEditor.repoPath && !!agentFolder && linkedEditor.repoPath !== agentFolder
  );
  let mismatchOpen = $state(false);

  const terminalInstances = $derived(layoutState.instances.terminal.map((i) => ({ id: i.id, name: i.name })));
  const linkedTerminal = $derived.by(() => {
    if (!sess?.linkedTerminalInstanceId) return null;
    const inst = layoutState.instances.terminal.find((i) => i.id === sess!.linkedTerminalInstanceId);
    return inst ? { id: inst.id, name: inst.name } : null;
  });
  const canvases = $derived(canvasState.index.filter((c) => !c.archivedAt));
  const linkedCanvas = $derived.by(() =>
    sess?.linkedCanvasId ? canvasState.index.find((c) => c.id === sess!.linkedCanvasId) ?? null : null
  );

  /* ---------- Run ---------- */
  function setModel(v: string) { if (sess) updateSession(sess.id, { claudeModel: v }); }
  function setEffort(v: string) {
    if (sess) updateSession(sess.id, { thinkingEffort: v as 'auto' | 'low' | 'medium' | 'high' | 'max' | 'ultracode' });
  }
  const rtkEnabled = $derived(sess?.rtkEnabled !== false);
  const fastCapable = $derived((sess?.claudeModel ?? '').startsWith('claude-opus-4-8'));

  /* ---------- Budget ---------- */
  const totals = $derived(sess ? sessionUsageTotals(sess) : null);
  const dw = $derived(sess ? sessionDwTotals(sess.id) : { costUsd: 0, runs: 0 });
  const grandCost = $derived((totals?.costUsd ?? 0) + dw.costUsd);
  const ctxPct = $derived.by(() => {
    if (!sess) return 0;
    let size = 0;
    for (let i = sess.messages.length - 1; i >= 0; i--) {
      const u = sess.messages[i].usage;
      if (u && u.contextSize) { size = u.contextSize; break; }
    }
    const win = contextWindowFor(sess.claudeModel ?? null);
    return win > 0 ? Math.min(100, Math.round((size / win) * 100)) : 0;
  });
  const fivePct = $derived(Math.round(quotaState.usage?.five_hour?.utilization ?? 0));
  const weekPct = $derived(Math.round(quotaState.usage?.seven_day?.utilization ?? 0));
  let budgetOpen = $state(false);

  /* ---------- Memory ---------- */
  const cwdBase = $derived(folderName(sess?.worktreePath || sess?.cwd || p.editorRepoPath));
  interface MemHit { id: number; content: string; }
  let memHits = $state<MemHit[]>([]);
  let memFetchedFor = $state<string | null>(null);
  $effect(() => {
    const base = cwdBase;
    if (!base) { memHits = []; memFetchedFor = null; return; }
    if (memFetchedFor === base) return;
    memFetchedFor = base;
    invoke<MemHit[]>('memory_search_local', { query: base, limit: 5 })
      .then((h) => { if (memFetchedFor === base) memHits = h; })
      .catch(() => { if (memFetchedFor === base) memHits = []; });
  });

  /* ---------- Tasks ---------- */
  const tasks = $derived(bgTasksState.tasks);
  function elapsed(startedAt: number): string {
    const s = Math.floor((Date.now() - startedAt) / 1000);
    if (s < 60) return `${s}s`;
    const m = Math.floor(s / 60);
    return m < 60 ? `${m}m` : `${Math.floor(m / 60)}h`;
  }
  function openPreview() { window.dispatchEvent(new CustomEvent('woom:open-preview')); }
</script>

<aside class="cd">
  <header class="cd-head">
    <span class="cd-head-title">Context</span>
    <span class="cd-head-spring"></span>
    {#if p.onCollapse}
      <button class="cd-collapse" onclick={p.onCollapse} title="Collapse context" aria-label="Collapse context">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M10 6l6 6-6 6"/></svg>
      </button>
    {/if}
  </header>

  {#if sess}
    <div class="cd-body">
      <!-- 1 · Repo -->
      <section class="cd-sec">
        <div class="cd-label-row"><span class="cd-label">Repo</span><span class="hatch"></span></div>
        <div class="cd-repo-row">
          <span class="cd-repo-name">{repoName || 'No folder'}</span>
          <button class="cd-dotted" onclick={() => { focusLocal(); p.onPickCwd(); }}>change</button>
        </div>
        {#if repoPath}<div class="cd-path mono" title={repoPath}>{shortPath(repoPath)}</div>{/if}
        {#if sess.worktreePath}
          <div class="cd-wt-row">
            <span class="cd-wt-branch mono">{sess.worktreeBranch ?? 'worktree'}</span>
          </div>
          <div class="cd-chips">
            <button class="cd-chip" onclick={p.onOpenWorktreeDiff}>diff</button>
            <button class="cd-chip" onclick={p.onOpenWorktreeInEditor}>in editor</button>
            <button class="cd-chip" onclick={p.onCopyWorktreeBranch}>branch ⧉</button>
            <button class="cd-chip cd-chip--danger" disabled={p.worktreeBusy === 'removing'} onclick={p.onRemoveWorktree}>remove</button>
          </div>
        {:else}
          <button class="cd-dashed" disabled={p.worktreeBusy === 'creating'} onclick={p.onCreateWorktree}>
            + create worktree
          </button>
        {/if}
        {#if folderMismatch && linkedEditor}
          <div class="cd-mismatch-wrap">
            <button class="cd-mismatch" onclick={() => (mismatchOpen = !mismatchOpen)}>
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M12 9v4"/><circle cx="12" cy="17" r="0.6" fill="currentColor"/><path d="M10.29 3.86 1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z"/></svg>
              folder mismatch
            </button>
            {#if mismatchOpen}
              <div class="cd-mismatch-menu">
                <button class="cd-mismatch-opt" onclick={() => { mismatchOpen = false; p.onSyncAgentToEditor?.(); }}>
                  Use editor's folder <span class="mono">{linkedEditor.folder}</span>
                </button>
                <button class="cd-mismatch-opt" onclick={() => { mismatchOpen = false; p.onSyncEditorToAgent?.(); }}>
                  Use agent's folder <span class="mono">{folderName(agentFolder)}</span>
                </button>
              </div>
            {/if}
          </div>
        {/if}
      </section>

      <!-- 2 · Links -->
      <section class="cd-sec">
        <div class="cd-label-row"><span class="cd-label">Links</span><span class="hatch"></span></div>
        <!-- editor -->
        <div class="cd-link-row">
          <span class="cd-link-key">editor</span>
          {#if linkedEditor}
            <button class="cd-link-chip" onclick={() => { focusLocal(); p.onToggleEditorLink(); }} title="Unlink">
              {linkedEditor.name}{#if linkedEditor.folder} · {linkedEditor.folder}{/if} <span class="cd-link-x">×</span>
            </button>
          {:else if editorInstances.length > 0}
            <Dropdown value="" options={editorInstances.map((e) => ({ value: e.id, label: e.folder ? `${e.name} (${e.folder})` : e.name, hint: e.repoPath || undefined }))} onChange={(id) => { focusLocal(); p.onLinkToEditorInstance(id); }} placeholder="link ▾" ariaLabel="Link editor" />
          {:else}
            <span class="cd-link-none">—</span>
          {/if}
        </div>
        <!-- terminal -->
        {#if p.onLinkToTerminalInstance}
          <div class="cd-link-row">
            <span class="cd-link-key">terminal</span>
            {#if linkedTerminal}
              <button class="cd-link-chip" onclick={() => { focusLocal(); p.onToggleTerminalLink?.(); }} title="Unlink">{linkedTerminal.name} <span class="cd-link-x">×</span></button>
            {:else if terminalInstances.length > 0}
              <Dropdown value="" options={terminalInstances.map((t) => ({ value: t.id, label: t.name }))} onChange={(id) => { focusLocal(); p.onLinkToTerminalInstance?.(id); }} placeholder="link ▾" ariaLabel="Link terminal" />
            {:else}
              <span class="cd-link-none">—</span>
            {/if}
          </div>
        {/if}
        <!-- canvas -->
        {#if p.onLinkToCanvas}
          <div class="cd-link-row">
            <span class="cd-link-key">canvas</span>
            {#if linkedCanvas}
              <button class="cd-link-chip" onclick={() => { focusLocal(); p.onToggleCanvasLink?.(); }} title="Unlink">{linkedCanvas.name} <span class="cd-link-x">×</span></button>
            {:else if canvases.length > 0}
              <Dropdown value="" options={canvases.map((c) => ({ value: c.id, label: c.name }))} onChange={(id) => { focusLocal(); p.onLinkToCanvas?.(id); }} placeholder="link ▾" ariaLabel="Link canvas" />
            {:else}
              <span class="cd-link-none">—</span>
            {/if}
          </div>
        {/if}
      </section>

      <!-- 3 · Run -->
      <section class="cd-sec">
        <div class="cd-label-row"><span class="cd-label">Run</span><span class="hatch"></span></div>
        <ModelEngine
          model={sess.claudeModel ?? 'claude-sonnet-4-6'}
          modelOptions={claudeModels}
          effort={sess.thinkingEffort ?? 'auto'}
          effortOptions={claudeEffort}
          onModelChange={setModel}
          onEffortChange={setEffort}
        />
        <div class="cd-toggles">
          <button class="cd-toggle" class:on={rtkEnabled} onclick={() => updateSession(sess.id, { rtkEnabled: !rtkEnabled })} aria-pressed={rtkEnabled}>
            <span class="cd-toggle-track"><span class="cd-toggle-knob"></span></span> RTK
          </button>
          {#if fastCapable}
            <button class="cd-toggle" class:on={sess.fastMode === true} onclick={() => updateSession(sess.id, { fastMode: !(sess.fastMode === true) })} aria-pressed={sess.fastMode === true}>
              <span class="cd-toggle-track"><span class="cd-toggle-knob"></span></span> FAST
            </button>
          {/if}
        </div>
        <div class="cd-hint">launch: <span class="mono">/dw</span> · <span class="mono">/ledger</span> — type in the field</div>
      </section>

      <!-- 4 · Budget -->
      {#if totals && (totals.turns > 0 || dw.runs > 0)}
        <section class="cd-sec">
          <div class="cd-label-row"><span class="cd-label">Budget</span><span class="hatch"></span></div>
          <div class="cd-budget-tot mono">{totals.turns} turns · {formatCostUsd(grandCost)} · {formatTokens(totals.input + totals.output)} tok</div>
          {#each [{ k: 'ctx', v: ctxPct }, { k: '5h', v: fivePct }, { k: 'week', v: weekPct }] as bar (bar.k)}
            <div class="cd-bar-row">
              <span class="cd-bar-label mono">{bar.k}</span>
              <span class="cd-bar-track"><span class="cd-bar-fill" class:warn={bar.v >= 100} style="width:{Math.min(100, bar.v)}%"></span></span>
            </div>
          {/each}
          <button class="cd-dotted" onclick={() => (budgetOpen = true)}>expand</button>
        </section>
      {/if}

      <!-- 5 · Memory -->
      {#if memHits.length > 0}
        <section class="cd-sec">
          <div class="cd-label-row"><span class="cd-label">Memory</span><span class="hatch"></span></div>
          <div class="cd-mem-count mono">{memHits.length} record{memHits.length === 1 ? '' : 's'} · {cwdBase}</div>
          <div class="cd-mem-preview">{memHits[0].content.replace(/\s+/g, ' ').slice(0, 120)}</div>
        </section>
      {/if}

      <!-- 6 · Tasks -->
      {#if tasks.length > 0}
        <section class="cd-sec">
          <div class="cd-label-row"><span class="cd-label">Tasks</span><span class="hatch"></span></div>
          {#each tasks.slice(0, 6) as t (t.id)}
            <div class="cd-task-row">
              <span class="cd-task-dot" class:running={t.status.kind === 'running'}></span>
              <span class="cd-task-name">{t.label ?? t.id}</span>
              {#if t.status.kind === 'running'}<span class="cd-task-el mono">{elapsed(t.started_at)}</span>{/if}
              <button class="cd-dotted" onclick={openPreview}>preview</button>
            </div>
          {/each}
        </section>
      {/if}
    </div>
  {:else}
    <div class="cd-empty">No active session.</div>
  {/if}
</aside>

{#if budgetOpen && sess}
  <div class="cd-overlay" role="button" tabindex="-1" onclick={() => (budgetOpen = false)} onkeydown={(e) => { if (e.key === 'Escape') budgetOpen = false; }}>
    <div class="cd-overlay-inner" role="dialog" aria-label="Budget breakdown">
      <BudgetPopover session={sess} onClose={() => (budgetOpen = false)} />
    </div>
  </div>
{/if}

<style>
  .cd {
    width: 300px; flex: none;
    display: flex; flex-direction: column;
    background: var(--bg-1);
    border-left: 1px solid var(--border-lo);
    min-height: 0;
  }
  .cd-head {
    flex: none;
    display: flex; align-items: center;
    padding: 14px 14px 10px;
  }
  .cd-head-title { font-size: 13px; font-weight: 600; color: var(--text-0); }
  .cd-head-spring { flex: 1; }
  .cd-collapse {
    width: 24px; height: 24px; display: grid; place-items: center;
    border: 0; border-radius: 6px; background: transparent;
    color: var(--text-faint); cursor: pointer;
  }
  .cd-collapse:hover { color: var(--text-0); background: var(--bg-hover); }
  .cd-collapse svg { width: 13px; height: 13px; }

  .cd-body { flex: 1; min-height: 0; overflow-y: auto; padding: 0 14px 16px; }
  .cd-empty { padding: 20px 14px; color: var(--text-faint); font-size: 12px; }

  .cd-sec { padding: 14px 0; border-top: 1px solid var(--border-lo); }
  .cd-sec:first-child { border-top: 0; }
  .cd-label-row { display: flex; align-items: center; gap: 8px; margin-bottom: 9px; }
  .cd-label {
    font-size: 10.5px; font-weight: 600; letter-spacing: 0.09em;
    text-transform: uppercase; color: var(--text-faint);
  }
  .cd-label-row .hatch { flex: 1; }

  .cd-repo-row { display: flex; align-items: baseline; gap: 8px; }
  .cd-repo-name { font-size: 13px; font-weight: 500; color: var(--text-0); }
  .cd-path { font-size: 11px; color: var(--text-faint); margin-top: 3px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .cd-wt-row { margin-top: 8px; }
  .cd-wt-branch { font-size: 11.5px; color: var(--text-1); }

  .cd-dotted {
    background: transparent; border: 0; padding: 0; cursor: pointer;
    font-size: 11px; color: var(--text-mute);
    border-bottom: 1px dotted var(--border-hi);
    margin-left: auto;
  }
  .cd-dotted:hover { color: var(--text-0); }

  .cd-chips { display: flex; flex-wrap: wrap; gap: 5px; margin-top: 8px; }
  .cd-chip {
    padding: 3px 8px; border: 1px solid var(--border-hi); border-radius: 6px;
    background: transparent; color: var(--text-mute); font-size: 11px; cursor: pointer;
  }
  .cd-chip:hover { color: var(--text-0); border-color: var(--border-hi2); }
  .cd-chip--danger:hover { color: var(--err); border-color: var(--err); }
  .cd-chip:disabled { opacity: 0.5; cursor: default; }
  .cd-dashed {
    width: 100%; margin-top: 4px; padding: 7px;
    border: 1px dashed var(--border-hi); border-radius: 8px;
    background: transparent; color: var(--text-mute); font-size: 11.5px; cursor: pointer;
  }
  .cd-dashed:hover { color: var(--text-0); border-color: var(--border-hi2); }

  .cd-mismatch-wrap { margin-top: 8px; position: relative; }
  .cd-mismatch {
    display: inline-flex; align-items: center; gap: 5px;
    padding: 4px 9px; border-radius: 7px;
    background: color-mix(in srgb, #f0a050 18%, transparent);
    border: 1px solid color-mix(in srgb, #f0a050 50%, transparent);
    color: #f0a050; font-size: 11px; cursor: pointer;
  }
  .cd-mismatch svg { width: 12px; height: 12px; }
  .cd-mismatch-menu {
    margin-top: 6px; display: flex; flex-direction: column; gap: 4px;
    border: 1px solid var(--border-hi); border-radius: 8px; padding: 6px;
    background: var(--bg-2); box-shadow: var(--shadow-1);
  }
  .cd-mismatch-opt {
    text-align: left; background: transparent; border: 0; cursor: pointer;
    padding: 5px 6px; border-radius: 5px; font-size: 11.5px; color: var(--text-1);
  }
  .cd-mismatch-opt:hover { background: var(--bg-hover); }

  .cd-link-row { display: flex; align-items: center; gap: 8px; margin-bottom: 6px; font-size: 11.5px; }
  .cd-link-key { color: var(--text-mute); min-width: 56px; }
  .cd-link-none { color: var(--text-faint); margin-left: auto; }
  .cd-link-chip {
    margin-left: auto; display: inline-flex; align-items: center; gap: 5px;
    padding: 3px 8px; border-radius: 6px;
    background: var(--bg-3); border: 0; color: var(--text-1);
    font-size: 11px; cursor: pointer;
  }
  .cd-link-chip:hover { color: var(--text-0); }
  .cd-link-x { color: var(--text-faint); }

  .cd-toggles { display: flex; gap: 8px; margin-top: 10px; }
  .cd-toggle {
    display: inline-flex; align-items: center; gap: 6px;
    background: transparent; border: 0; cursor: pointer;
    font-size: 10px; font-weight: 600; letter-spacing: 0.08em;
    color: var(--text-faint);
  }
  .cd-toggle.on { color: var(--text-1); }
  .cd-toggle-track {
    width: 26px; height: 15px; border-radius: 8px;
    background: var(--bg-4); position: relative;
    transition: background 140ms;
  }
  .cd-toggle.on .cd-toggle-track { background: var(--accent); }
  .cd-toggle-knob {
    position: absolute; top: 2px; left: 2px;
    width: 11px; height: 11px; border-radius: 50%;
    background: var(--text-faint); transition: transform 140ms, background 140ms;
  }
  .cd-toggle.on .cd-toggle-knob { transform: translateX(11px); background: var(--accent-fg); }
  .cd-hint { margin-top: 10px; font-size: 11px; color: var(--text-faint); }

  .cd-budget-tot { font-size: 11.5px; color: var(--text-1); margin-bottom: 8px; }
  .cd-bar-row { display: flex; align-items: center; gap: 8px; margin-bottom: 5px; }
  .cd-bar-label { font-size: 11px; color: var(--text-mute); min-width: 34px; }
  .cd-bar-track { flex: 1; height: 4px; border-radius: 2px; background: var(--bg-4); overflow: hidden; }
  .cd-bar-fill { display: block; height: 100%; background: var(--text-1); border-radius: 2px; }
  .cd-bar-fill.warn { background: var(--warn); }

  .cd-mem-count { font-size: 11.5px; color: var(--text-1); margin-bottom: 5px; }
  .cd-mem-preview {
    font-size: 11.5px; color: var(--text-faint); line-height: 1.5;
    display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2;
    -webkit-box-orient: vertical; overflow: hidden;
  }

  .cd-task-row { display: flex; align-items: center; gap: 7px; margin-bottom: 6px; font-size: 11.5px; }
  .cd-task-dot { width: 6px; height: 6px; border-radius: 50%; background: var(--text-linenum); flex: none; }
  .cd-task-dot.running { background: var(--ok); animation: cd-pulse 1.6s infinite; }
  .cd-task-name { flex: 1; min-width: 0; color: var(--text-1); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .cd-task-el { font-size: 11px; color: var(--text-faint); }
  @keyframes cd-pulse { 0%, 100% { opacity: 0.35; } 50% { opacity: 1; } }

  .cd-overlay {
    position: fixed; inset: 0; z-index: 300;
    background: var(--backdrop, rgba(0,0,0,0.3));
    display: grid; place-items: center;
  }
  .cd-overlay-inner { position: relative; }
</style>
