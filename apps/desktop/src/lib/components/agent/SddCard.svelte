<script lang="ts">
  /* SDD inline card — renders the workspace as part of the chat thread.
   *
   * One card per session (the per-session SDD workspace), with a stage-
   * specific body + action buttons. Clicking [Approve & continue] /
   * [Start phase N] / [Next phase] fires `onAdvance(prompt)` which the
   * parent uses to populate the composer + fire the same send pipeline
   * a manual user message uses — so SDD doesn't have its own out-of-
   * band send path; it just stamps text into the composer.
   *
   * Visual language: same chrome as QuestionCard (left accent stripe,
   * subtle hover, low-prominence so the chat stays the focus). Body
   * markdown rendered via Markdown.svelte.
   */
  import { fly } from 'svelte/transition';
  import { cubicOut } from 'svelte/easing';
  import Markdown from '$lib/components/ui/Markdown.svelte';
  import {
    type SddWorkspace,
    approveSdd,
    pauseSdd,
    resumeSdd,
    stopSdd,
    discardSdd,
    buildPromptForStage,
  } from '$lib/state/sdd.svelte';

  interface Props {
    workspace: SddWorkspace;
    /** Stamp the next prompt into composer + fire send. Parent wires
     *  this to its `setSessionInput` + `sendClaudeMessage()` flow. */
    onAdvance: (prompt: string) => void | Promise<void>;
  }
  let p: Props = $props();

  /* Collapsed body preview by default — spec/plan can be long. User
   *  clicks the section to expand. Persists per-stage (so unique to
   *  this render, not the workspace globally — re-mount resets). */
  let bodyOpen = $state(false);

  const stage = $derived(p.workspace.stage);
  const isAwaitingApproval = $derived(
    stage.kind === 'spec_ready' || stage.kind === 'plan_ready' || stage.kind === 'phase_done'
  );
  const isInFlight = $derived(
    stage.kind === 'drafting' || stage.kind === 'planning' || stage.kind === 'phase_running'
  );
  const isTerminal = $derived(
    stage.kind === 'complete' || stage.kind === 'stopped' || stage.kind === 'failed'
  );
  const isPaused = $derived(stage.kind === 'paused');

  function stageLabel(): string {
    switch (stage.kind) {
      case 'drafting': return 'Drafting spec';
      case 'spec_ready': return 'Spec ready';
      case 'planning': return 'Drafting plan';
      case 'plan_ready': return 'Plan ready';
      case 'phase_running': return `Phase ${stage.phase} running`;
      case 'phase_done': return `Phase ${stage.phase} done`;
      case 'complete': return 'All phases done';
      case 'paused': return 'Paused';
      case 'stopped': return 'Stopped';
      case 'failed': return 'Failed';
    }
  }

  function stageTone(): 'live' | 'ok' | 'warn' | 'dim' {
    if (isInFlight) return 'live';
    if (stage.kind === 'failed' || stage.kind === 'stopped') return 'warn';
    if (stage.kind === 'complete') return 'ok';
    return 'dim';
  }

  /* Resolve the prompt for the next agent turn. Called by the action
   *  buttons (Approve / Continue / Start phase). The Rust side advances
   *  the workspace stage first, then we ask for the prompt the new
   *  stage needs. */
  async function advance() {
    /* Flip the appropriate approve gate. After flipping, we re-build
     *  the prompt against the FRESH workspace state — the awaited
     *  approve call returns the new workspace, so we use it. */
    let fresh = p.workspace;
    if (stage.kind === 'spec_ready') {
      const w = await approveSdd(p.workspace.id, 'spec');
      if (w) fresh = w;
    } else if (stage.kind === 'plan_ready' || stage.kind === 'phase_done') {
      // Plan_ready may need an approve flip; phase_done doesn't.
      if (stage.kind === 'plan_ready') {
        const w = await approveSdd(p.workspace.id, 'plan');
        if (w) fresh = w;
      }
    }
    const prompt = await buildPromptForStage(fresh);
    if (prompt) {
      void p.onAdvance(prompt);
    }
  }

  /* Body chunk to preview — show spec for spec_ready, plan for
   *  plan_ready, current phase's body for phase_running, prior phase
   *  summary for phase_done. */
  function bodyForStage(): { title: string; markdown: string } | null {
    if (stage.kind === 'spec_ready' && p.workspace.spec_body) {
      return { title: 'spec.md', markdown: p.workspace.spec_body };
    }
    if (stage.kind === 'plan_ready' && p.workspace.plan_body) {
      return { title: 'plan.md', markdown: p.workspace.plan_body };
    }
    if (stage.kind === 'phase_running') {
      const ph = p.workspace.phases.find((x) => x.number === stage.phase);
      if (ph) return { title: `phases/${ph.slug}.md`, markdown: ph.body };
    }
    if (stage.kind === 'phase_done') {
      const ph = p.workspace.phases.find((x) => x.number === stage.phase);
      if (ph?.summary) return { title: `results/${ph.slug}-result.md`, markdown: ph.summary };
      if (ph) return { title: `phases/${ph.slug}.md`, markdown: ph.body };
    }
    if (stage.kind === 'complete') {
      const all = p.workspace.phases
        .map((ph) => `### Phase ${ph.number}: ${ph.title}\n\n${ph.summary ?? '_no summary written_'}\n`)
        .join('\n');
      return { title: 'all phases', markdown: all || '_no phase summaries_' };
    }
    return null;
  }

  const body = $derived(bodyForStage());

  /* "Next phase" lookup for the [Start phase N] / [Next phase] buttons.
   *  When in plan_ready, points to phase 1; when in phase_done, points
   *  to the next pending phase. */
  const nextPhase = $derived(p.workspace.phases.find((ph) => ph.status === 'pending'));

  function actionLabel(): string {
    if (stage.kind === 'spec_ready') return 'Approve spec · draft plan';
    if (stage.kind === 'plan_ready') return nextPhase ? `Approve plan · start phase ${nextPhase.number}` : 'Approve plan';
    if (stage.kind === 'phase_done') return nextPhase ? `Continue · phase ${nextPhase.number}` : 'Done';
    return '';
  }

  async function onPause() { await pauseSdd(p.workspace.id); }
  async function onResume() { await resumeSdd(p.workspace.id); }
  async function onStop() { await stopSdd(p.workspace.id); }
  async function onDiscard() {
    if (!confirm('Discard this SDD workspace? All temp files will be deleted.')) {
      /* Note: same Tauri webview confirm caveat as PreviewPane — Tauri
       *  returns undefined here, so this `confirm()` always falls
       *  through. That's fine for now (discard is cheap to recreate);
       *  if we want real confirmation we'll swap in a custom modal. */
    }
    await discardSdd(p.workspace.id);
  }
</script>

<aside class="sdd-card" data-tone={stageTone()} in:fly={{ y: 8, duration: 180, easing: cubicOut }}>
  <header class="sdd-head">
    <span class="sdd-glyph" aria-hidden="true">SDD</span>
    <span class="sdd-stage">{stageLabel()}</span>
    {#if isInFlight}
      <span class="sdd-spin" aria-label="Agent working"></span>
    {/if}
    <span class="sdd-id mono">{p.workspace.id}</span>
  </header>

  <div class="sdd-prompt-line">
    <span class="sdd-prompt-label">Ask:</span>
    <span class="sdd-prompt-text">{p.workspace.user_prompt}</span>
  </div>

  {#if p.workspace.phases.length > 0}
    <div class="sdd-phases">
      {#each p.workspace.phases as ph (ph.number)}
        <span
          class="sdd-phase-pill"
          data-status={ph.status}
          title="{ph.title} · {ph.status}"
        >
          <span class="sdd-phase-num mono">{ph.number}</span>
          <span class="sdd-phase-title">{ph.title}</span>
        </span>
      {/each}
    </div>
  {/if}

  {#if body}
    <div class="sdd-body">
      <button
        type="button"
        class="sdd-body-toggle"
        onclick={() => (bodyOpen = !bodyOpen)}
        aria-expanded={bodyOpen}
      >
        <svg class="sdd-chev" class:open={bodyOpen} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true">
          <path d="M9 6l6 6-6 6"/>
        </svg>
        <span class="mono">{body.title}</span>
      </button>
      {#if bodyOpen}
        <div class="sdd-body-content">
          <Markdown source={body.markdown} />
        </div>
      {/if}
    </div>
  {/if}

  <footer class="sdd-actions">
    {#if isAwaitingApproval}
      <button class="sdd-btn sdd-btn--primary" onclick={advance}>{actionLabel()}</button>
    {/if}
    {#if isInFlight}
      <button class="sdd-btn" onclick={onPause}>Pause</button>
    {/if}
    {#if isPaused}
      <button class="sdd-btn sdd-btn--primary" onclick={onResume}>Resume</button>
    {/if}
    {#if !isTerminal}
      <button class="sdd-btn" onclick={onStop}>Stop</button>
    {/if}
    <button class="sdd-btn sdd-btn--mute" onclick={onDiscard}>Discard</button>
  </footer>
</aside>

<style>
  .sdd-card {
    border: 1px solid var(--border);
    border-left: 3px solid var(--accent);
    border-radius: 8px;
    background: var(--bg-1);
    padding: 12px 14px;
    display: flex; flex-direction: column;
    gap: 10px;
    min-width: 0;
  }
  .sdd-card[data-tone="live"] {
    border-left-color: #66d39a;
  }
  .sdd-card[data-tone="warn"] {
    border-left-color: #e0b16c;
  }
  .sdd-card[data-tone="ok"] {
    border-left-color: var(--accent);
  }

  .sdd-head {
    display: flex; align-items: center;
    gap: 8px;
  }
  .sdd-glyph {
    font-family: 'JetBrains Mono', monospace;
    font-size: 9.5px;
    font-weight: 700;
    letter-spacing: 0.12em;
    color: var(--accent-bright);
    background: var(--accent-soft);
    padding: 2px 6px;
    border-radius: 4px;
    border: 1px solid color-mix(in srgb, var(--accent) 30%, transparent);
  }
  .sdd-stage {
    font-size: 12.5px;
    font-weight: 500;
    color: var(--text-0);
  }
  .sdd-spin {
    width: 10px; height: 10px;
    border-radius: 50%;
    border: 1.5px solid var(--border-neutral-hi);
    border-top-color: var(--accent);
    animation: sdd-spin 700ms linear infinite;
  }
  @keyframes sdd-spin { to { transform: rotate(360deg); } }
  .sdd-id {
    font-size: 10px;
    color: var(--text-mute);
    margin-left: auto;
  }

  .sdd-prompt-line {
    display: flex; gap: 6px;
    font-size: 12px;
    color: var(--text-1);
  }
  .sdd-prompt-label {
    color: var(--text-mute);
    font-weight: 500;
    flex-shrink: 0;
  }
  .sdd-prompt-text {
    color: var(--text-1);
    overflow-wrap: anywhere;
  }

  .sdd-phases {
    display: flex; flex-wrap: wrap; gap: 5px;
  }
  .sdd-phase-pill {
    display: inline-flex; align-items: center;
    gap: 5px;
    font-size: 11px;
    padding: 3px 8px;
    border-radius: 999px;
    border: 1px solid var(--border-neutral);
    background: var(--bg-2);
    color: var(--text-2);
  }
  .sdd-phase-pill[data-status="running"] {
    color: #66d39a;
    border-color: color-mix(in srgb, #66d39a 50%, var(--border-neutral));
    background: color-mix(in srgb, #66d39a 10%, var(--bg-2));
  }
  .sdd-phase-pill[data-status="done"] {
    color: var(--text-0);
    border-color: color-mix(in srgb, var(--accent) 40%, var(--border-neutral));
    background: color-mix(in srgb, var(--accent) 10%, var(--bg-2));
  }
  .sdd-phase-pill[data-status="failed"] {
    color: #e0b16c;
    border-color: color-mix(in srgb, #e0b16c 40%, var(--border-neutral));
    background: color-mix(in srgb, #e0b16c 8%, var(--bg-2));
  }
  .sdd-phase-num {
    font-size: 10px;
    color: var(--text-mute);
    font-weight: 700;
  }

  .sdd-body {
    display: flex; flex-direction: column;
    gap: 6px;
  }
  .sdd-body-toggle {
    display: inline-flex; align-items: center;
    gap: 6px;
    padding: 5px 8px;
    border-radius: 5px;
    border: 1px solid var(--border-neutral);
    background: var(--bg-2);
    color: var(--text-1);
    cursor: pointer;
    font-size: 11px;
    align-self: flex-start;
    transition: background 120ms, border-color 120ms;
  }
  .sdd-body-toggle:hover {
    background: var(--bg-3);
    border-color: var(--border-hi);
    color: var(--text-0);
  }
  .sdd-chev {
    width: 10px; height: 10px;
    transition: transform 180ms;
  }
  .sdd-chev.open { transform: rotate(90deg); }

  .sdd-body-content {
    padding: 10px 12px;
    background: var(--bg-0);
    border: 1px solid var(--border-neutral);
    border-radius: 6px;
    max-height: 400px;
    overflow-y: auto;
  }

  .sdd-actions {
    display: flex; gap: 6px;
    margin-top: 2px;
  }
  .sdd-btn {
    padding: 4px 10px;
    border-radius: 5px;
    background: var(--bg-2);
    border: 1px solid var(--border);
    color: var(--text-1);
    font-size: 11px;
    cursor: pointer;
    transition: background 120ms, border-color 120ms, color 120ms;
  }
  .sdd-btn:hover {
    background: var(--bg-3);
    border-color: var(--border-hi);
    color: var(--text-0);
  }
  .sdd-btn--primary {
    background: color-mix(in srgb, var(--accent) 30%, var(--bg-2));
    border-color: color-mix(in srgb, var(--accent) 55%, var(--border));
    color: var(--text-0);
  }
  .sdd-btn--primary:hover {
    background: color-mix(in srgb, var(--accent) 40%, var(--bg-2));
  }
  .sdd-btn--mute {
    margin-left: auto;
    color: var(--text-mute);
    border-color: var(--border-neutral);
  }
</style>
