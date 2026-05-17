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
    saveSddBody,
    retrySddPhase,
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
  /* Edit mode — swap the rendered Markdown for a textarea. The user
   *  can tweak the agent's spec/plan/phase content before approving.
   *  YAML frontmatter on disk is preserved by the Rust side. */
  let editMode = $state(false);
  let editDraft = $state('');

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

  /* Edit-mode helpers. Save target depends on current stage — spec
   *  when SpecReady, plan when PlanReady, current phase otherwise. */
  function editTarget(): { kind: 'spec' } | { kind: 'plan' } | { kind: 'phase'; number: number } | null {
    if (stage.kind === 'spec_ready') return { kind: 'spec' };
    if (stage.kind === 'plan_ready') return { kind: 'plan' };
    if (stage.kind === 'phase_running' || stage.kind === 'phase_done') {
      return { kind: 'phase', number: stage.phase };
    }
    return null;
  }
  function startEdit() {
    if (!body) return;
    editDraft = body.markdown;
    editMode = true;
    bodyOpen = true;
  }
  function cancelEdit() {
    editMode = false;
    editDraft = '';
  }
  async function saveEdit() {
    const t = editTarget();
    if (!t) return;
    await saveSddBody(p.workspace.id, t, editDraft);
    editMode = false;
    editDraft = '';
  }

  async function onPause() { await pauseSdd(p.workspace.id); }
  async function onResume() { await resumeSdd(p.workspace.id); }
  async function onStop() { await stopSdd(p.workspace.id); }
  async function onRetry() {
    /* Retry button shows on Failed stage. Pick the phase that's
     *  marked failed (there's at most one in sequential mode) and
     *  reset its status — derive_stage flips us back to PhaseDone
     *  for the prior phase, so the next advance re-issues this one. */
    const failed = p.workspace.phases.find((ph) => ph.status === 'failed');
    if (failed) {
      const fresh = await retrySddPhase(p.workspace.id, failed.number);
      if (fresh) {
        const prompt = await buildPromptForStage(fresh);
        if (prompt) void p.onAdvance(prompt);
      }
    }
  }
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
      <div class="sdd-body-row">
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
        {#if !editMode && editTarget()}
          <button type="button" class="sdd-edit-toggle" onclick={startEdit} title="Edit before approving">
            edit ✎
          </button>
        {/if}
      </div>
      {#if bodyOpen}
        {#if editMode}
          <div class="sdd-body-edit">
            <textarea
              class="sdd-edit-area mono"
              bind:value={editDraft}
              spellcheck="false"
              rows="14"
            ></textarea>
            <div class="sdd-edit-actions">
              <button type="button" class="sdd-btn" onclick={cancelEdit}>cancel</button>
              <button type="button" class="sdd-btn sdd-btn--primary" onclick={saveEdit}>save</button>
            </div>
          </div>
        {:else}
          <div class="sdd-body-content">
            <Markdown source={body.markdown} />
          </div>
        {/if}
      {/if}
    </div>
  {/if}

  {#if stage.kind === 'failed'}
    <div class="sdd-failed">
      <strong>Failed:</strong> {stage.reason}
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
    {#if stage.kind === 'failed'}
      <button class="sdd-btn sdd-btn--primary" onclick={onRetry}>Retry phase</button>
    {/if}
    {#if !isTerminal}
      <button class="sdd-btn" onclick={onStop}>Stop</button>
    {/if}
    <button class="sdd-btn sdd-btn--mute" onclick={onDiscard}>Discard</button>
  </footer>
</aside>

<style>
  /* SDD card visual language — modelled on Markdown.svelte's
   *  blockquote treatment (subtle accent-tinted bg + left stripe,
   *  no full border, rounded only on the right). The card should
   *  feel like a rich element WITHIN the message stream, not a
   *  modal pinned on top — same way a markdown table or quote
   *  reads as content rather than an interrupt-y proposal.
   *  Atmosphere: warm accent tint says "agent is offering you a
   *  decision here", typography matches surrounding prose. */
  .sdd-card {
    border-left: 3px solid var(--accent);
    border-radius: 0 6px 6px 0;
    background: var(--accent-soft);
    padding: 10px 14px 11px;
    display: flex; flex-direction: column;
    gap: 8px;
    min-width: 0;
    color: var(--text-1);
    font-size: 13.5px;
    line-height: 1.55;
  }
  .sdd-card[data-tone="live"] {
    border-left-color: #66d39a;
    background: color-mix(in srgb, #66d39a 8%, var(--bg-1));
  }
  .sdd-card[data-tone="warn"] {
    border-left-color: #e0b16c;
    background: color-mix(in srgb, #e0b16c 8%, var(--bg-1));
  }
  .sdd-card[data-tone="ok"] {
    border-left-color: var(--accent);
  }

  /* Byline row reads like prose: "SDD · stage label · spinner · id".
   *  No header chrome, no chip backgrounds — typography carries the
   *  hierarchy. */
  .sdd-head {
    display: flex; align-items: center;
    gap: 8px;
    font-size: 12px;
  }
  .sdd-glyph {
    font-family: 'JetBrains Mono', monospace;
    font-size: 9.5px;
    font-weight: 700;
    letter-spacing: 0.14em;
    color: var(--accent-bright);
    text-transform: uppercase;
  }
  .sdd-glyph::after {
    content: '·';
    color: var(--text-mute);
    margin-left: 8px;
    font-weight: 400;
  }
  .sdd-stage {
    font-size: 12px;
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
    font-family: 'JetBrains Mono', monospace;
    opacity: 0.6;
  }

  .sdd-prompt-line {
    display: flex; gap: 6px;
    font-size: 13px;
    line-height: 1.55;
    color: var(--text-1);
    font-style: italic;
  }
  .sdd-prompt-label {
    color: var(--text-mute);
    font-weight: 500;
    flex-shrink: 0;
    font-style: normal;
  }
  .sdd-prompt-text {
    color: var(--text-1);
    overflow-wrap: anywhere;
  }

  .sdd-phases {
    display: flex; flex-wrap: wrap; gap: 5px;
  }
  /* Phase pills look like inline code spans — same font + bg
   *  language as `<code>` from prose. Reinforces "this is part of the
   *  message" feel rather than "this is UI chrome". */
  .sdd-phase-pill {
    display: inline-flex; align-items: center;
    gap: 5px;
    font-family: 'JetBrains Mono', monospace;
    font-size: 10.5px;
    padding: 1px 8px;
    border-radius: 4px;
    border: 1px solid var(--border-neutral-hi);
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
  .sdd-body-row {
    display: flex; align-items: center;
    gap: 14px;
  }
  /* Edit toggle — text-link aesthetic, sits next to the file-title
   *  chevron. Same hover behaviour as the other text-buttons. */
  .sdd-edit-toggle {
    border: 0;
    background: transparent;
    color: var(--text-mute);
    cursor: pointer;
    font-size: 11px;
    padding: 1px 0;
    transition: color 120ms;
  }
  .sdd-edit-toggle:hover {
    color: var(--accent-bright);
  }

  /* Edit-mode textarea — fills the body slot while editing. mono +
   *  subtle accent border to feel like an "agent's draft you're
   *  amending" rather than a generic form input. */
  .sdd-body-edit {
    display: flex; flex-direction: column;
    gap: 6px;
  }
  .sdd-edit-area {
    width: 100%;
    min-height: 240px;
    padding: 10px 12px;
    border-radius: 5px;
    border: 1px solid color-mix(in srgb, var(--accent) 30%, var(--border-neutral));
    background: var(--bg-0);
    color: var(--text-0);
    font-family: 'JetBrains Mono', monospace;
    font-size: 12px;
    line-height: 1.5;
    resize: vertical;
  }
  .sdd-edit-area:focus {
    outline: 2px solid var(--accent);
    outline-offset: -1px;
    border-color: transparent;
  }
  .sdd-edit-actions {
    display: flex; gap: 14px; align-items: center;
    justify-content: flex-end;
  }

  /* Failed banner — uses warn-tone red accents inside the card, sits
   *  above the actions row so the user sees WHY before they decide
   *  whether to retry. */
  .sdd-failed {
    padding: 6px 10px;
    border-left: 2px solid var(--error);
    background: color-mix(in srgb, var(--error) 10%, transparent);
    border-radius: 3px;
    color: var(--text-1);
    font-size: 12px;
    line-height: 1.5;
  }
  .sdd-failed strong { color: var(--error); }
  /* Body toggle is a text-link, not a button — keeps the "inline
   *  prose" feel. Chevron + filename hint at expandability without
   *  shouting. */
  .sdd-body-toggle {
    display: inline-flex; align-items: center;
    gap: 5px;
    padding: 1px 0;
    border: 0;
    background: transparent;
    color: var(--text-mute);
    cursor: pointer;
    font-size: 11.5px;
    align-self: flex-start;
    transition: color 120ms;
  }
  .sdd-body-toggle:hover {
    color: var(--accent-bright);
  }
  .sdd-chev {
    width: 10px; height: 10px;
    transition: transform 180ms;
  }
  .sdd-chev.open { transform: rotate(90deg); }

  /* Expanded body renders as a quote-within-quote — slightly inset,
   *  bordered on the left, transparent bg so the parent accent tint
   *  stays visible. Reads like a "this is what the agent wrote" excerpt
   *  inside the SDD frame. */
  .sdd-body-content {
    padding: 4px 0 4px 12px;
    border-left: 2px solid color-mix(in srgb, var(--accent) 30%, transparent);
    max-height: 360px;
    overflow-y: auto;
    margin-top: 2px;
  }

  /* Actions read as a row of text-buttons — only the primary CTA has
   *  any chrome (a soft accent fill). Pause/Stop/Discard are bare
   *  prose with hover underline so they don't compete with the
   *  reading flow. */
  .sdd-actions {
    display: flex; align-items: center;
    gap: 14px;
    margin-top: 4px;
  }
  .sdd-btn {
    padding: 2px 0;
    border: 0;
    background: transparent;
    color: var(--text-mute);
    font-size: 12px;
    cursor: pointer;
    transition: color 120ms;
  }
  .sdd-btn:hover {
    color: var(--accent-bright);
  }
  .sdd-btn--primary {
    padding: 4px 12px;
    border-radius: 5px;
    background: color-mix(in srgb, var(--accent) 32%, transparent);
    border: 1px solid color-mix(in srgb, var(--accent) 55%, transparent);
    color: var(--text-0);
    font-weight: 500;
    font-size: 12px;
  }
  .sdd-btn--primary:hover {
    background: color-mix(in srgb, var(--accent) 45%, transparent);
    color: var(--text-0);
  }
  .sdd-btn--mute {
    margin-left: auto;
    color: var(--text-mute);
    font-size: 11.5px;
  }
</style>
