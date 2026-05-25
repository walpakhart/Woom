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
    type ActionLogEntry,
    type SddPhaseDiff,
    type DiffFile,
    type AuditEntry,
    approveSdd,
    approveSddPhase,
    approveSddPhasePlan,
    discardSddPhasePlan,
    setSddPhaseExecutionConfig,
    DEFAULT_PHASE_EXECUTION_CONFIG,
    type PhaseExecutionConfig,
    buildAmendPrompt,
    pauseSdd,
    resumeSdd,
    stopSdd,
    discardSdd,
    saveSddBody,
    retrySddPhase,
    rollbackSddPhase,
    skipSddPhaseWithReason,
    getSddPhaseDiff,
    getSddFileDiff,
    loadAuditLog,
    buildPromptForStage,
    manualContinueSdd,
    acceptSddPhaseFailed,
    targetKey,
    stashUndo,
    popUndo,
    sddState,
    hideSddCard,
    actionLogFor,
  } from '$lib/state/sdd.svelte';
  import { sessionsState } from '$lib/state/sessions.svelte';
  import { diffMarkdown, renderDiffHtml } from '$lib/util/markdownDiff';

  interface Props {
    workspace: SddWorkspace;
    /** Stamp the next prompt into composer + fire send. Parent wires
     *  this to its `setSessionInput` + `sendClaudeMessage()` flow. */
    onAdvance: (prompt: string) => void | Promise<void>;
    /** Read-only fullscreen overlay mode — opened from the header
     *  history popover. Hides the action footer (no Discard, no
     *  Approve/Amend/Stop), forces fullscreen on, and routes the
     *  close button to the parent-supplied `onClose` instead of the
     *  internal `fullscreen=false` toggle. */
    viewOnly?: boolean;
    onClose?: () => void;
  }
  let p: Props = $props();

  /* Collapsed body preview by default — spec/plan can be long. User
   *  clicks the section to expand. Persists per-stage (so unique to
   *  this render, not the workspace globally — re-mount resets). */
  let bodyOpen = $state(false);
  /* Lightbox / fullscreen reading mode. Opens the spec / plan / phase
   *  body as a viewport-cover overlay (a-la Telegram photo viewer),
   *  so long documents are actually readable instead of cramped into
   *  the chat column. Esc closes; same dismissal as DiffView's
   *  full-screen toggle. */
  let fullscreen = $state(false);
  const effectiveFullscreen = $derived(p.viewOnly ? true : fullscreen);
  function openFullscreen() {
    bodyOpen = true;
    fullscreen = true;
  }
  function closeFullscreen() {
    if (p.viewOnly) {
      p.onClose?.();
      return;
    }
    fullscreen = false;
  }
  function onFullscreenKey(e: KeyboardEvent) {
    if (e.key === 'Escape' && effectiveFullscreen) {
      e.preventDefault();
      closeFullscreen();
    }
  }
  $effect(() => {
    if (!effectiveFullscreen) return;
    window.addEventListener('keydown', onFullscreenKey);
    return () => window.removeEventListener('keydown', onFullscreenKey);
  });
  $effect(() => {
    if (p.viewOnly) bodyOpen = true;
  });
  /* Edit mode — swap the rendered Markdown for a textarea. The user
   *  can tweak the agent's spec/plan/phase content before approving.
   *  YAML frontmatter on disk is preserved by the Rust side. */
  let editMode = $state(false);
  let editDraft = $state('');
  /* Agent's last-saved body at the moment edit mode was entered. Used
   *  by the diff-split and diff-unified views to show "what changed
   *  vs the draft". Captured ONCE on `startEdit()` so toggling views
   *  is instant + never re-reads the file. Cleared on cancel / save. */
  let editOriginal = $state('');
  /* Which sub-view of edit mode is active — plain `edit` (textarea),
   *  `diff-split` (read-only original | editable draft), or
   *  `diff-unified` (single-pane jsdiff-rendered HTML). */
  type EditView = 'edit' | 'diff-split' | 'diff-unified';
  let editView = $state<EditView>('edit');
  /* Refs for the split view's two panes — wired so the `onscroll`
   *  handler can copy proportional scroll position between them. */
  let splitOriginalEl: HTMLPreElement | null = $state(null);
  let splitDraftEl: HTMLTextAreaElement | null = $state(null);
  /* Cached unified-diff HTML — recomputed on every keystroke via
   *  `$derived`. For a typical plan (~200 lines) this is cheap;
   *  jsdiff's diffLines is O(n*m) but bounded by line count. */
  const unifiedDiffHtml = $derived.by(() => {
    if (editView !== 'diff-unified') return '';
    return renderDiffHtml(diffMarkdown(editOriginal, editDraft));
  });

  /* Local "advance in flight" gate. Set true the moment the user
   *  clicks the primary CTA so a double-click can't fire two agent
   *  turns. Cleared automatically when the workspace stage changes
   *  (i.e. the agent has touched something — watcher emitted a
   *  rebuild). Prevents the stuck-queue bug where ~20 phase-execute
   *  prompts ended up in `pendingQueue` because the user kept
   *  clicking while the first turn was streaming. */
  let advanceClicked = $state(false);
  // svelte-ignore state_referenced_locally
  let lastStageKind = $state(p.workspace.stage.kind);
  $effect(() => {
    if (p.workspace.stage.kind !== lastStageKind) {
      lastStageKind = p.workspace.stage.kind;
      advanceClicked = false;
      /* Drop any phase peek when the natural stage advances — the
       *  user's intent was "show me phase N right now"; once N is
       *  done and N+1 is the focus, the override is stale. */
      selectedPhaseOverride = null;
      /* Auto-open the body section when the workflow completes so
       *  the user doesn't have to chevron-expand to see the final
       *  summary. Mirror behaviour for Failed so the failure
       *  reason / last phase context is visible immediately. */
      if (
        p.workspace.stage.kind === 'complete' ||
        p.workspace.stage.kind === 'failed'
      ) {
        bodyOpen = true;
      }
    }
  });

  /* 1-second tick — drives the undo affordance's countdown display.
   *  Cheap; only lives while the card is mounted. Same pattern as
   *  PreviewPane's elapsed-time tick. */
  let now = $state(Date.now());
  $effect(() => {
    const t = setInterval(() => { now = Date.now(); }, 1000);
    return () => clearInterval(t);
  });

  /* Undo affordance state — derived from the store slot for the
   *  CURRENT stage's target. Only one undo per file is tracked at a
   *  time; switching stages picks the right slot via `targetKey`.
   *  30-second window; after that the slot stays (until the agent
   *  rewrites or a fresh save replaces it) but the affordance hides. */
  const UNDO_WINDOW_MS = 30_000;
  const undoSlot = $derived.by(() => {
    const t = editTarget();
    if (!t) return null;
    const slots = sddState.undoByWorkspace[p.workspace.id];
    if (!slots) return null;
    return slots[targetKey(t)] ?? null;
  });
  const undoSecondsLeft = $derived.by(() => {
    if (!undoSlot) return 0;
    const left = Math.ceil((undoSlot.savedAt + UNDO_WINDOW_MS - now) / 1000);
    return Math.max(0, left);
  });
  const undoVisible = $derived(undoSlot !== null && undoSecondsLeft > 0);

  const stage = $derived(p.workspace.stage);
  /* Session-busy derived from the linked chat session. While the
   *  agent is mid-turn we DON'T offer the Approve button — clicking
   *  during a streaming reply caused the race-window bugs. UI shows
   *  "agent working…" instead so the user knows why it's quiet. */
  const linkedSession = $derived(
    sessionsState.list.find((s) => s.id === p.workspace.session_id) ?? null
  );
  const sessionSending = $derived(!!linkedSession?.sending);
  const isAwaitingApproval = $derived(
    !sessionSending &&
      (stage.kind === 'spec_ready' ||
        stage.kind === 'plan_ready' ||
        stage.kind === 'phase_done' ||
        stage.kind === 'phase_pending_approval' ||
        stage.kind === 'phase_plan_review')
  );
  const isInFlight = $derived(
    sessionSending ||
      stage.kind === 'drafting' ||
      stage.kind === 'planning' ||
      stage.kind === 'phase_running' ||
      stage.kind === 'phase_planning' ||
      stage.kind === 'phase_implementing' ||
      stage.kind === 'phase_verifying'
  );
  const isTerminal = $derived(
    stage.kind === 'complete' || stage.kind === 'stopped' || stage.kind === 'failed'
  );
  const isPaused = $derived(stage.kind === 'paused');
  /* Manual-continue affordance — when the agent turn ended but the
   *  auto-fire dispatcher didn't pick up (stale prod bundle, silent
   *  drop), the workspace sits in phase_{planning,implementing,
   *  verifying} with !sessionSending. Surface a Continue button so
   *  the user can re-arm the next pass without restarting the app. */
  const canManualContinue = $derived(
    !sessionSending &&
      (stage.kind === 'phase_planning' ||
        stage.kind === 'phase_implementing' ||
        stage.kind === 'phase_verifying')
  );
  const continueLabel = $derived.by(() => {
    switch (stage.kind) {
      case 'phase_planning': return 'Continue plan-pass';
      case 'phase_implementing': return 'Continue implement-pass';
      case 'phase_verifying': return 'Continue verify-pass';
      default: return 'Continue';
    }
  });

  /* Live activity feed — derived from sddState.actionLogByWorkspace.
   *  `showAll` is local UI state (collapse vs. expand). The buffer is
   *  reactive: pushes from `attachActionLogListener` re-render this
   *  block automatically. */
  let liveActivityShowAll = $state(false);
  const liveActivity = $derived.by(() => {
    const visible = stage.kind === 'phase_running';
    if (!visible) {
      return {
        visible: false as const,
        phase: 0,
        entries: [] as ActionLogEntry[],
        headEntries: [] as ActionLogEntry[],
      };
    }
    /* Read through the reactive proxy so $derived tracks updates. */
    const all =
      sddState.actionLogByWorkspace[p.workspace.id]?.[stage.phase] ?? [];
    /* Newest-first inline — easier to scan when the agent is mid-burst. */
    const entries = [...all].reverse();
    const headEntries = entries.slice(0, 5);
    return {
      visible: true as const,
      phase: stage.phase,
      entries,
      headEntries,
    };
  });

  function stageLabel(): string {
    switch (stage.kind) {
      case 'drafting': return 'Drafting spec';
      case 'spec_ready': return 'Spec ready';
      case 'planning': return 'Drafting plan';
      case 'plan_ready': return 'Plan ready';
      case 'phase_pending_approval': return `Phase ${stage.phase} — review`;
      case 'phase_running': return `Phase ${stage.phase} running`;
      case 'phase_planning': return `Phase ${stage.phase} — planning`;
      case 'phase_plan_review': return `Phase ${stage.phase} — plan review`;
      case 'phase_implementing': return `Phase ${stage.phase} — implementing`;
      case 'phase_verifying': return `Phase ${stage.phase} verifying`;
      case 'phase_done': return `Phase ${stage.phase} done`;
      case 'complete': return 'All phases done';
      case 'paused': return 'Paused';
      case 'stopped': return 'Stopped';
      case 'failed': return 'Failed';
    }
  }

  function stageTone(): 'live' | 'ok' | 'warn' | 'dim' {
    if (isInFlight) return 'live';
    if (stage.kind === 'phase_plan_review') return 'warn';
    if (stage.kind === 'failed' || stage.kind === 'stopped') return 'warn';
    if (stage.kind === 'complete') return 'ok';
    return 'dim';
  }

  /* Resolve the prompt for the next agent turn. Called by the action
   *  buttons (Approve / Continue / Start phase). The Rust side advances
   *  the workspace stage first, then we ask for the prompt the new
   *  stage needs. */
  async function advance() {
    /* Guard re-entry — once a click is in flight, ignore further
     *  clicks until the stage changes. */
    if (advanceClicked) return;
    advanceClicked = true;
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
    } else if (stage.kind === 'phase_pending_approval') {
      const w = await approveSddPhase(p.workspace.id, stage.phase);
      if (w) fresh = w;
    } else if (stage.kind === 'phase_plan_review') {
      // Three-call mode plan-review gate. Clear the marker, advance
      // substep-state to Implement, then fire the implement-pass
      // prompt via the standard pipeline.
      const w = await approveSddPhasePlan(p.workspace.id, stage.phase);
      if (w) fresh = w;
    }
    const prompt = await buildPromptForStage(fresh);
    if (prompt) {
      void p.onAdvance(prompt);
    } else {
      /* No prompt to send (e.g. all phases done already) — release
       *  the gate so the button is usable for the next stage. */
      advanceClicked = false;
    }
  }

  /* Three-call mode — surface the verify.json verdict for the
   *  CURRENTLY-displayed phase (active stage's phase, or the override
   *  when the user clicks a phase chip). Null when verify.json is
   *  absent. See `spec-1` FR-10. */
  const verifyForActiveStage = $derived.by(() => {
    const targetPhase =
      selectedPhaseOverride !== null
        ? selectedPhaseOverride
        : 'phase' in stage
          ? stage.phase
          : null;
    if (targetPhase === null) return null;
    const ph = p.workspace.phases.find((x) => x.number === targetPhase);
    if (!ph || !ph.verify) return null;
    return { slug: ph.slug, verdict: ph.verify };
  });

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
    if (stage.kind === 'phase_planning' || stage.kind === 'phase_implementing' || stage.kind === 'phase_verifying') {
      const ph = p.workspace.phases.find((x) => x.number === stage.phase);
      // During verify/implement we already have the plan.md — show it
      // so the user can scan the agent's intended approach while the
      // pass is running. During planning, plan.md may not exist yet.
      if (ph?.plan_body) return { title: `phases/${ph.slug}/plan.md`, markdown: ph.plan_body };
      if (ph) return { title: `phases/${ph.slug}.md`, markdown: ph.body };
    }
    if (stage.kind === 'phase_plan_review') {
      const ph = p.workspace.phases.find((x) => x.number === stage.phase);
      if (ph?.plan_body) return { title: `phases/${ph.slug}/plan.md`, markdown: ph.plan_body };
      if (ph) return { title: `phases/${ph.slug}.md`, markdown: ph.body };
    }
    if (stage.kind === 'phase_done') {
      const ph = p.workspace.phases.find((x) => x.number === stage.phase);
      if (ph?.summary) return { title: `results/${ph.slug}-result.md`, markdown: ph.summary };
      if (ph) return { title: `phases/${ph.slug}.md`, markdown: ph.body };
    }
    if (stage.kind === 'complete') {
      /* Prefer the agent's wrap-up (SUMMARY.md) when present — it's
       *  the curated digest. Fall back to concatenated phase
       *  summaries while the summary is still being written. */
      if (p.workspace.summary_body) {
        return { title: 'SUMMARY.md', markdown: p.workspace.summary_body };
      }
      const all = p.workspace.phases
        .map((ph) => `### Phase ${ph.number}: ${ph.title}\n\n${ph.summary ?? '_no summary written_'}\n`)
        .join('\n');
      return { title: 'all phases', markdown: all || '_no phase summaries — waiting for wrap-up…_' };
    }
    return null;
  }

  /** Phase-pill click override. When set, the body slot renders the
   *  phase's plan section + result (if the phase has run) instead of
   *  whatever the natural stage points at. Click a pill to peek;
   *  click again to deselect; auto-resets when the stage advances
   *  (see the lastStageKind effect above). */
  let selectedPhaseOverride = $state<number | null>(null);

  /** Compose a phase's body — plan section first, result/summary
   *  appended below when the phase has completed. Renders as a real
   *  document, so the lightbox view of a single phase shows the
   *  agent's intent + what shipped side-by-side. */
  function phaseBody(num: number): { title: string; markdown: string } | null {
    const ph = p.workspace.phases.find((x) => x.number === num);
    if (!ph) return null;
    const parts: string[] = [];
    parts.push(`# Phase ${ph.number}: ${ph.title}`);
    parts.push(`_Status: **${ph.status}**_`);
    parts.push('');
    parts.push('## Plan');
    parts.push(ph.body?.trim() || '_no plan body yet_');
    if (ph.summary && ph.summary.trim()) {
      parts.push('');
      parts.push('## Result');
      parts.push(ph.summary.trim());
    }
    return { title: `phases/${ph.slug}.md`, markdown: parts.join('\n') };
  }

  const body = $derived(
    selectedPhaseOverride !== null ? phaseBody(selectedPhaseOverride) : bodyForStage()
  );

  /** Click a phase pill to peek at its plan + result. Re-clicking
   *  the same pill (or the natural-stage pill) clears the override. */
  function togglePhase(num: number) {
    if (selectedPhaseOverride === num) {
      selectedPhaseOverride = null;
    } else {
      selectedPhaseOverride = num;
      bodyOpen = true;
    }
  }

  /* "Next phase" lookup for the [Start phase N] / [Next phase] buttons.
   *  When in plan_ready, points to phase 1; when in phase_done, points
   *  to the next pending phase. */
  const nextPhase = $derived(p.workspace.phases.find((ph) => ph.status === 'pending'));

  function actionLabel(): string {
    if (stage.kind === 'spec_ready') return 'Approve spec · draft plan';
    if (stage.kind === 'plan_ready') return nextPhase ? `Approve plan · start phase ${nextPhase.number}` : 'Approve plan';
    if (stage.kind === 'phase_done') return nextPhase ? `Continue · phase ${nextPhase.number}` : 'Done';
    if (stage.kind === 'phase_pending_approval') return `Approve · start phase ${stage.phase}`;
    if (stage.kind === 'phase_plan_review') return `Approve plan · run phase ${stage.phase}`;
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
    /* Snapshot the agent's last-saved body BEFORE any user edits land
     *  on `editDraft`. Used by diff-split (read-only left pane) and
     *  diff-unified (jsdiff base) so toggling views never re-reads
     *  the file. */
    editOriginal = body.markdown;
    editView = 'edit';
    editMode = true;
    bodyOpen = true;
  }
  function cancelEdit() {
    editMode = false;
    editDraft = '';
    editOriginal = '';
    editView = 'edit';
  }
  async function saveEdit() {
    const t = editTarget();
    if (!t) return;
    /* Snapshot pre-save body into the undo store BEFORE the network
     *  call. Skipped when the draft is identical to the original —
     *  no point flashing an Undo affordance for a no-op save. */
    if (editOriginal !== editDraft) {
      stashUndo(p.workspace.id, targetKey(t), editOriginal);
    }
    await saveSddBody(p.workspace.id, t, editDraft);
    editMode = false;
    editDraft = '';
    editOriginal = '';
    editView = 'edit';
    /* Edit-then-retry chain — when the user clicked "Edit phase &
     *  retry" on the failure card, the save automatically continues
     *  into a retry so they don't have to click twice. We snap the
     *  flag back before firing so re-entry is clean. */
    if (editAndRetryArmed) {
      editAndRetryArmed = false;
      const failed = p.workspace.phases.find(
        (ph) =>
          ph.status === 'failed'
          || (t.kind === 'phase' && ph.number === t.number)
      );
      if (failed && !advanceClicked) {
        advanceClicked = true;
        const fresh = await retrySddPhase(p.workspace.id, failed.number);
        if (fresh) {
          const prompt = await buildPromptForStage(fresh);
          if (prompt) {
            void p.onAdvance(prompt);
            return;
          }
        }
        advanceClicked = false;
      }
    }
  }

  /** Restore the pre-save body for the current stage's target.
   *  Reads + clears the slot, then calls `saveSddBody` to write it
   *  back. The watcher's `sdd:changed` event will refresh the card.
   *  Caller must verify a slot exists (button only renders when it
   *  does) — defence-in-depth handled here by a null-guard. */
  async function onUndo() {
    const t = editTarget();
    if (!t) return;
    const key = targetKey(t);
    const prev = popUndo(p.workspace.id, key);
    if (prev == null) return;
    await saveSddBody(p.workspace.id, t, prev);
  }

  /* Sync-scroll between the split panes — proportional so when one
   *  is taller (e.g. the textarea wraps differently than the <pre>)
   *  both reach end at the same time. Source pane is whichever just
   *  fired `onscroll`; target gets `scrollTop = ratio * targetMax`. */
  function syncScroll(source: HTMLElement, target: HTMLElement | null) {
    if (!target) return;
    const sMax = source.scrollHeight - source.clientHeight;
    const tMax = target.scrollHeight - target.clientHeight;
    if (sMax <= 0 || tMax <= 0) return;
    const ratio = source.scrollTop / sMax;
    /* Skip if the target is already at the matching ratio — avoids a
     *  feedback loop where setting `scrollTop` re-fires the other
     *  pane's `onscroll` handler. */
    const want = Math.round(ratio * tMax);
    if (Math.abs(target.scrollTop - want) <= 1) return;
    target.scrollTop = want;
  }

  /* Amend-mode — inline textarea that lets the user describe a change
   *  to the CURRENT artifact (spec / plan / phase). On submit we
   *  build the amend prompt and fire it through the normal send
   *  pipeline, so the agent edits files in place instead of writing
   *  a new spec from scratch. Different gesture from `editMode`
   *  (which is direct file editing) — amend is "tell the agent what
   *  to change, let it patch". */
  let amendMode = $state(false);
  let amendDraft = $state('');
  function startAmend() {
    amendMode = true;
    amendDraft = '';
  }
  function cancelAmend() {
    amendMode = false;
    amendDraft = '';
  }
  async function sendAmend() {
    const change = amendDraft.trim();
    if (!change || advanceClicked) return;
    advanceClicked = true;
    const prompt = await buildAmendPrompt(p.workspace, change);
    amendMode = false;
    amendDraft = '';
    void p.onAdvance(prompt);
  }

  async function onPause() { await pauseSdd(p.workspace.id); }
  async function onResume() { await resumeSdd(p.workspace.id); }
  async function onStop() { await stopSdd(p.workspace.id); }
  let continueClicked = $state(false);
  async function onContinue() {
    if (continueClicked || !canManualContinue) return;
    continueClicked = true;
    try {
      await manualContinueSdd(p.workspace.id);
    } finally {
      continueClicked = false;
    }
  }
  /** Discard the plan-pass output during plan-review. Calls the
   *  dedicated `sdd_discard_phase_plan` command so the phase flips
   *  to `failed { trigger: plan_discarded }` and the standard
   *  failure card (Retry / Edit & retry / Skip) takes over. */
  async function discardPlanReview() {
    if (advanceClicked) return;
    if (stage.kind !== 'phase_plan_review') return;
    advanceClicked = true;
    try {
      await discardSddPhasePlan(p.workspace.id, stage.phase);
    } finally {
      advanceClicked = false;
    }
  }
  async function onRetry() {
    if (advanceClicked) return;
    advanceClicked = true;
    /* Retry button shows on Failed stage. Pick the phase that's
     *  marked failed (there's at most one in sequential mode) and
     *  reset its status — derive_stage flips us back to PhaseDone
     *  for the prior phase, so the next advance re-issues this one. */
    const failed = p.workspace.phases.find((ph) => ph.status === 'failed');
    if (failed) {
      const fresh = await retrySddPhase(p.workspace.id, failed.number);
      if (fresh) {
        const prompt = await buildPromptForStage(fresh);
        if (prompt) {
          void p.onAdvance(prompt);
          return;
        }
      }
    }
    advanceClicked = false;
  }

  /* Edit-then-retry: open editMode against the failed phase body so
   *  the user can tweak instructions, then chain `saveEdit` →
   *  `retrySddPhase` → fire prompt — saving the user a second click
   *  after Save. We piggy-back on the existing edit machinery (the
   *  textarea, save flow) by setting a one-shot flag the saveEdit
   *  handler picks up. */
  let editAndRetryArmed = $state(false);
  function startEditAndRetry() {
    /* Failed stage's edit target is the failed phase. Switch the peek
     *  to that phase first so `body` resolves to its content; then
     *  open editMode. */
    const failed = p.workspace.phases.find((ph) => ph.status === 'failed');
    if (!failed) return;
    selectedPhaseOverride = failed.number;
    editAndRetryArmed = true;
    /* Defer one tick so the `body` $derived sees the override. */
    queueMicrotask(() => startEdit());
  }

  /* Inline skip-with-reason flow. Opens a textarea in the failure card;
   *  Submit calls `skipSddPhaseWithReason` (5+ char gate enforced
   *  server-side) and clears the failed stage. */
  let skipMode = $state(false);
  let skipDraft = $state('');
  function startSkip() {
    skipMode = true;
    skipDraft = '';
  }
  function cancelSkip() {
    skipMode = false;
    skipDraft = '';
  }
  async function submitSkip() {
    const reason = skipDraft.trim();
    if (reason.length < 5) return;
    const failed = p.workspace.phases.find((ph) => ph.status === 'failed');
    if (!failed) return;
    await skipSddPhaseWithReason(p.workspace.id, failed.number, reason);
    skipMode = false;
    skipDraft = '';
  }

  async function onRollbackFailed() {
    const failed = p.workspace.phases.find((ph) => ph.status === 'failed');
    if (!failed) return;
    await rollbackSddPhase(p.workspace.id, failed.number);
  }

  /* Inline accept-with-reason flow. Mirror of the skip flow but flips
   *  `failed` → `done` instead of `skipped`. Used when the user has
   *  reviewed verifier deviations and decided they are tolerable. */
  let acceptMode = $state(false);
  let acceptDraft = $state('');
  function startAccept() {
    acceptMode = true;
    acceptDraft = '';
  }
  function cancelAccept() {
    acceptMode = false;
    acceptDraft = '';
  }
  async function submitAccept() {
    const reason = acceptDraft.trim();
    if (reason.length < 5) return;
    const failed = p.workspace.phases.find((ph) => ph.status === 'failed');
    if (!failed) return;
    await acceptSddPhaseFailed(p.workspace.id, failed.number, reason);
    acceptMode = false;
    acceptDraft = '';
  }

  /* Phase-diff drawer state. One open file at a time — stores the
   *  patch text keyed by `${phase}::${path}`. Top-level drawer is
   *  collapsed by default; auto-expanded inside the lightbox. */
  let phaseDiffByPhase = $state<Record<number, SddPhaseDiff | 'loading' | null>>({});
  let phaseDiffOpenPhase = $state<number | null>(null);
  let openDiffFiles = $state<Record<string, string | 'loading'>>({});

  /* Which phase number the diff drawer should target. For done /
   *  failed phases, peek-override wins; otherwise follow stage.
   *  Returns null when there's nothing to diff (stage hasn't produced
   *  a post-phase commit yet). */
  const diffTargetPhase = $derived.by<number | null>(() => {
    const candidate = (() => {
      if (selectedPhaseOverride !== null) return selectedPhaseOverride;
      if (stage.kind === 'phase_done') return stage.phase;
      if (stage.kind === 'failed' && stage.failed_phase != null) return stage.failed_phase;
      return null;
    })();
    if (candidate == null) return null;
    /* Only render drawer for phases in a settled state. The peek
     *  override may point at a `pending` phase (user clicking a future
     *  phase pill) — there's no diff to show for those. */
    const ph = p.workspace.phases.find((x) => x.number === candidate);
    if (!ph) return null;
    if (ph.status === 'pending' || ph.status === 'running') return null;
    return candidate;
  });

  /* Auto-trigger the lazy diff load on enter — drawer is collapsed by
   *  default but the head row needs the totals to render its label. */
  $effect(() => {
    if (diffTargetPhase != null) {
      void ensurePhaseDiffLoaded(diffTargetPhase);
    }
  });

  async function ensurePhaseDiffLoaded(phase: number) {
    if (phaseDiffByPhase[phase] !== undefined) return;
    phaseDiffByPhase = { ...phaseDiffByPhase, [phase]: 'loading' };
    const diff = await getSddPhaseDiff(p.workspace.id, phase);
    phaseDiffByPhase = { ...phaseDiffByPhase, [phase]: diff };
  }

  function togglePhaseDiff(phase: number) {
    if (phaseDiffOpenPhase === phase) {
      phaseDiffOpenPhase = null;
      return;
    }
    phaseDiffOpenPhase = phase;
    void ensurePhaseDiffLoaded(phase);
  }

  /* In lightbox, auto-load + auto-open the diff drawer for the
   *  currently-targeted phase. Reading mode benefits from seeing the
   *  diff alongside the phase body. */
  $effect(() => {
    if (!effectiveFullscreen || diffTargetPhase == null) return;
    phaseDiffOpenPhase = diffTargetPhase;
    void ensurePhaseDiffLoaded(diffTargetPhase);
  });

  async function toggleFileDiff(phase: number, path: string) {
    const key = `${phase}::${path}`;
    if (openDiffFiles[key] !== undefined) {
      const next = { ...openDiffFiles };
      delete next[key];
      openDiffFiles = next;
      return;
    }
    openDiffFiles = { ...openDiffFiles, [key]: 'loading' };
    const patch = await getSddFileDiff(p.workspace.id, phase, path);
    openDiffFiles = { ...openDiffFiles, [key]: patch };
  }

  function fileDiffKey(phase: number, path: string): string {
    return `${phase}::${path}`;
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

  // -------- Audit log (phase 6: self-driving MCP) ---------------------
  // Lazy-load on first request; refresh whenever the workspace stage
  // changes (a mutation likely just landed, so cached entries are
  // stale). The overlay is dismissable via Escape.
  let auditEntries = $state<AuditEntry[]>([]);
  let auditOpen = $state(false);
  /** Per-workspace execution-mode config drawer toggle. Opens
   *  inline under the card header — mirrors Settings card controls. */
  let configOpen = $state(false);
  let auditLoaded = $state(false);
  let auditFilter = $state<'all' | 'agent' | 'user' | 'system'>('all');
  let auditExpandedTs = $state<number | null>(null);
  const filteredAudit = $derived(
    auditFilter === 'all'
      ? auditEntries
      : auditEntries.filter((e) => e.source === auditFilter)
  );
  async function refreshAudit() {
    const entries = await loadAuditLog(p.workspace.id);
    auditEntries = entries;
    auditLoaded = true;
  }
  async function toggleAudit() {
    if (!auditOpen) {
      await refreshAudit();
    }
    auditOpen = !auditOpen;
  }
  // Auto-refresh on stage flip — covers user mutations + agent
  // mutations (both of which land on disk before the watcher fires
  // emit_changed and bumps the stage).
  $effect(() => {
    void p.workspace.stage.kind;
    if (auditLoaded) void refreshAudit();
  });
  // Initial load — happens once per mount so the header indicator
  // shows the count without the user opening the overlay.
  $effect(() => {
    void p.workspace.id;
    if (!auditLoaded) void refreshAudit();
  });
  function copyAuditAsJsonl() {
    const text = filteredAudit
      .map((e) => JSON.stringify(e))
      .join('\n');
    void navigator.clipboard.writeText(text);
  }
  function fmtAuditTs(ts: number): string {
    try {
      const d = new Date(ts);
      // HH:MM:SS for compactness (date is shown in the day-grouped
      // header below if we ever group, which v1 doesn't).
      return d.toLocaleTimeString();
    } catch {
      return String(ts);
    }
  }
  function onAuditKey(e: KeyboardEvent) {
    if (e.key === 'Escape' && auditOpen) {
      e.preventDefault();
      auditOpen = false;
    }
  }
  $effect(() => {
    if (!auditOpen) return;
    window.addEventListener('keydown', onAuditKey);
    return () => window.removeEventListener('keydown', onAuditKey);
  });
</script>

<aside
  class="sdd-card"
  class:sdd-card--full={effectiveFullscreen}
  class:sdd-card--view-only={p.viewOnly}
  data-tone={stageTone()}
  in:fly={{ y: 8, duration: 180, easing: cubicOut }}
>
  <header class="sdd-head">
    <span class="sdd-glyph" aria-hidden="true">SDD</span>
    <span class="sdd-stage">{stageLabel()}</span>
    {#if isInFlight}
      <span class="sdd-spin" aria-label="Agent working"></span>
    {/if}
    <span class="sdd-id mono">{p.workspace.id}</span>
    {#if auditEntries.length > 0}
      <button
        type="button"
        class="sdd-audit-chip"
        onclick={toggleAudit}
        title="Audit log — every mutation across agent / user / system"
        aria-pressed={auditOpen}
      >
        · {auditEntries.length} audit · view
      </button>
    {/if}
    {#if !p.viewOnly}
      <!-- Config cog — opens an inline drawer with per-workspace
           three-call mode toggle + plan-gate checkbox. Same controls
           as Settings card but scoped to this workspace. See
           `spec-1` FR-11. -->
      <button
        type="button"
        class="sdd-cog"
        class:sdd-cog--open={configOpen}
        onclick={() => (configOpen = !configOpen)}
        title="Configure execution mode (single-call / three-call) + plan-review gate"
        aria-label="Workspace execution config"
        aria-expanded={configOpen}
      >
        <svg viewBox="0 0 24 24" width="13" height="13" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">
          <circle cx="12" cy="12" r="3"/>
          <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06A1.65 1.65 0 0 0 4.6 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"/>
        </svg>
      </button>
    {/if}
    {#if !effectiveFullscreen && !p.viewOnly}
      <!-- Hide-without-discard. Workspace files stay on disk; the
           card simply leaves the thread until the user re-opens it
           from the header history popover. Distinct from Discard
           (which deletes files) and from the fullscreen × close. -->
      <button
        type="button"
        class="sdd-hide"
        onclick={() => hideSddCard(p.workspace.id)}
        title="Hide this SDD card from the thread (workspace stays on disk; re-open from the SDD chip in the header)"
        aria-label="Hide SDD card"
      >
        <svg viewBox="0 0 24 24" width="13" height="13" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">
          <path d="M5 13h14"/>
        </svg>
      </button>
    {/if}
    {#if effectiveFullscreen}
      <button
        type="button"
        class="sdd-close"
        onclick={closeFullscreen}
        title={p.viewOnly ? 'Close (Esc)' : 'Close fullscreen (Esc)'}
        aria-label={p.viewOnly ? 'Close' : 'Close fullscreen'}
      >
        <svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round">
          <path d="M6 6l12 12M6 18L18 6"/>
        </svg>
      </button>
    {/if}
  </header>

  {#if configOpen}
    <!-- Inline workspace config drawer — mirrors the Settings card
         controls but scoped to this workspace. See `spec-1` FR-11. -->
    <div class="sdd-config-drawer">
      <label class="sdd-config-row">
        <span class="sdd-config-label">Execution mode</span>
        <select
          class="sdd-config-select mono"
          value={p.workspace.phase_execution?.mode ?? 'single_call'}
          onchange={(e) => {
            const mode = (e.currentTarget as HTMLSelectElement).value as 'single_call' | 'three_call';
            void setSddPhaseExecutionConfig(p.workspace.id, {
              ...(p.workspace.phase_execution ?? DEFAULT_PHASE_EXECUTION_CONFIG),
              mode,
            } satisfies PhaseExecutionConfig);
          }}
        >
          <option value="single_call">single-call (legacy)</option>
          <option value="three_call">three-call (plan → implement → verify)</option>
        </select>
      </label>
      <label class="sdd-config-row sdd-config-row--toggle">
        <input
          type="checkbox"
          checked={p.workspace.phase_execution?.plan_gate ?? false}
          disabled={(p.workspace.phase_execution?.mode ?? 'single_call') !== 'three_call'}
          onchange={(e) => {
            const plan_gate = (e.currentTarget as HTMLInputElement).checked;
            void setSddPhaseExecutionConfig(p.workspace.id, {
              ...(p.workspace.phase_execution ?? DEFAULT_PHASE_EXECUTION_CONFIG),
              plan_gate,
            } satisfies PhaseExecutionConfig);
          }}
        />
        <span class="sdd-config-label">Pause between plan and implement (plan-review gate)</span>
      </label>
      <p class="sdd-config-hint">
        Three-call mode runs each phase as three discrete agent passes — adds ~5–15% cost per phase, improves auditability. Config persists in <span class="mono">meta.json#phase_execution</span>.
      </p>
    </div>
  {/if}

  <div class="sdd-prompt-line">
    <span class="sdd-prompt-label">Ask:</span>
    <span class="sdd-prompt-text">{p.workspace.user_prompt}</span>
  </div>

  {#if p.workspace.phases.length > 0}
    <div class="sdd-phases">
      {#each p.workspace.phases as ph (ph.number)}
        <button
          type="button"
          class="sdd-phase-pill"
          class:sdd-phase-pill--selected={selectedPhaseOverride === ph.number}
          data-status={ph.status}
          title="{ph.title} · {ph.status} · click to peek plan + result"
          aria-pressed={selectedPhaseOverride === ph.number}
          onclick={() => togglePhase(ph.number)}
        >
          <span class="sdd-phase-num mono">{ph.number}</span>
          <span class="sdd-phase-title">{ph.title}</span>
          {#if ph.summary && ph.summary.trim()}
            <!-- Tiny dot signals "has a written result you can read" so
                 the user knows pills carry post-run content. -->
            <span class="sdd-phase-dot" aria-hidden="true">●</span>
          {/if}
        </button>
      {/each}
      {#if selectedPhaseOverride !== null}
        <button
          type="button"
          class="sdd-phase-reset"
          onclick={() => (selectedPhaseOverride = null)}
          title="Back to the current stage's body"
        >back to stage</button>
      {/if}
    </div>
  {/if}

  {#if liveActivity.visible}
    <!-- Live activity feed — one row per recent tool_use / tool_result.
         Sourced from the in-memory ring buffer in sdd.svelte.ts; the
         backing JSONL on disk lets us rehydrate after restart so the
         user sees continuity. Cap inline at 5 rows; full log lives in
         the lightbox. -->
    <div class="sdd-activity">
      <div class="sdd-activity-head">
        <span class="sdd-activity-title">
          Phase {liveActivity.phase} · live activity · {liveActivity.entries.length} events
        </span>
        {#if liveActivity.entries.length > liveActivity.headEntries.length}
          <button
            type="button"
            class="sdd-activity-more"
            onclick={() => (liveActivityShowAll = !liveActivityShowAll)}
            title={liveActivityShowAll ? 'Collapse' : 'Show full log'}
          >
            {liveActivityShowAll ? 'collapse' : `+${liveActivity.entries.length - liveActivity.headEntries.length} more`}
          </button>
        {/if}
      </div>
      <ul class="sdd-activity-list">
        {#each (liveActivityShowAll ? liveActivity.entries : liveActivity.headEntries) as e, idx (e.correlation_id ?? `${e.ts}-${e.kind}-${idx}`)}
          <li class="sdd-activity-row" data-status={e.status ?? 'running'} in:fly={{ y: 4, duration: 120, easing: cubicOut }}>
            <span class="sdd-activity-dot" aria-hidden="true"></span>
            <span class="sdd-activity-tool mono">{e.tool ?? e.kind}</span>
            <span class="sdd-activity-summary">{e.summary}</span>
          </li>
        {/each}
      </ul>
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
        {#if editMode}
          <!-- View switch lives inline next to the file title — three
               muted text-buttons separated by ·, no pill chrome. Active
               state is just "color: text-0". Reads as a switch within
               the line, not a panel above it. -->
          <span class="sdd-view-switch" role="tablist" aria-label="Edit view">
            <button
              type="button"
              class="sdd-view-tab"
              class:active={editView === 'edit'}
              role="tab"
              aria-selected={editView === 'edit'}
              onclick={() => (editView = 'edit')}
            >edit</button>
            <span class="sdd-view-sep" aria-hidden="true">·</span>
            <button
              type="button"
              class="sdd-view-tab"
              class:active={editView === 'diff-split'}
              role="tab"
              aria-selected={editView === 'diff-split'}
              onclick={() => (editView = 'diff-split')}
            >split</button>
            <span class="sdd-view-sep" aria-hidden="true">·</span>
            <button
              type="button"
              class="sdd-view-tab"
              class:active={editView === 'diff-unified'}
              role="tab"
              aria-selected={editView === 'diff-unified'}
              onclick={() => (editView = 'diff-unified')}
            >diff</button>
          </span>
        {:else if editTarget() && !p.viewOnly}
          <button type="button" class="sdd-edit-toggle" onclick={startEdit} title="Edit before approving">
            edit ✎
          </button>
        {/if}
        <!-- Expand to fullscreen — spec / plan / phase docs are
             often long; reading them in a 60-char chat column is
             painful. ⛶ pops them into a viewport-cover overlay so
             the user can actually read end-to-end without scroll
             gymnastics. -->
        {#if !p.viewOnly}
        <button
          type="button"
          class="sdd-expand"
          onclick={openFullscreen}
          title="Open fullscreen (Esc to close)"
          aria-label="Open fullscreen"
        >
          <svg viewBox="0 0 24 24" width="13" height="13" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">
            <path d="M4 9V4h5M20 9V4h-5M4 15v5h5M20 15v5h-5"/>
          </svg>
        </button>
        {/if}
      </div>
      {#if bodyOpen}
        {#if editMode}
          <div class="sdd-body-edit">
            {#if editView === 'edit'}
              <textarea
                class="sdd-edit-area mono"
                bind:value={editDraft}
                spellcheck="false"
                rows="14"
              ></textarea>
            {:else if editView === 'diff-split'}
              <div class="sdd-diff-split">
                <pre
                  bind:this={splitOriginalEl}
                  class="sdd-edit-area sdd-edit-area--readonly mono"
                  onscroll={(e) => syncScroll(e.currentTarget, splitDraftEl)}
                >{editOriginal}</pre>
                <textarea
                  bind:this={splitDraftEl}
                  bind:value={editDraft}
                  class="sdd-edit-area mono"
                  spellcheck="false"
                  rows="14"
                  onscroll={(e) => syncScroll(e.currentTarget, splitOriginalEl)}
                ></textarea>
              </div>
            {:else}
              <div class="sdd-diff-unified">
                {@html unifiedDiffHtml}
              </div>
            {/if}
          </div>
        {:else}
          <div class="sdd-body-content">
            <Markdown source={body.markdown} />
          </div>
          {#if verifyForActiveStage}
            <!-- Verify pane — structured render of phases/<slug>/verify.json.
                 Surfaces below the regular body for completed phases so
                 the user can scan task_compliance / deviations / notes
                 without leaving the card. Empty fields hidden. See
                 `spec-1` FR-10. -->
            <div class="sdd-verify-pane" data-deviated={verifyForActiveStage.verdict.deviations.length > 0}>
              <header class="sdd-verify-head mono">verify · phases/{verifyForActiveStage.slug}/verify.json</header>
              {#if verifyForActiveStage.verdict.summary}
                <p class="sdd-verify-summary">{verifyForActiveStage.verdict.summary}</p>
              {/if}
              {#if verifyForActiveStage.verdict.files_changed.length > 0}
                <details class="sdd-verify-section">
                  <summary class="mono">files changed · {verifyForActiveStage.verdict.files_changed.length}</summary>
                  <ul class="sdd-verify-files mono">
                    {#each verifyForActiveStage.verdict.files_changed as f (f)}
                      <li>{f}</li>
                    {/each}
                  </ul>
                </details>
              {/if}
              {#if verifyForActiveStage.verdict.task_compliance.length > 0}
                <details class="sdd-verify-section">
                  <summary class="mono">task compliance · {verifyForActiveStage.verdict.task_compliance.length}</summary>
                  <ul class="sdd-verify-list">
                    {#each verifyForActiveStage.verdict.task_compliance as t (t)}
                      <li><span aria-label="passed">✓</span> {t}</li>
                    {/each}
                  </ul>
                </details>
              {/if}
              {#if verifyForActiveStage.verdict.deviations.length > 0}
                <details class="sdd-verify-section sdd-verify-section--warn" open>
                  <summary class="mono">deviations · {verifyForActiveStage.verdict.deviations.length}</summary>
                  <ul class="sdd-verify-list">
                    {#each verifyForActiveStage.verdict.deviations as d (d)}
                      <li><span aria-label="deviation">⚠️</span> {d}</li>
                    {/each}
                  </ul>
                </details>
              {/if}
              {#if verifyForActiveStage.verdict.notes}
                <details class="sdd-verify-section">
                  <summary class="mono">notes</summary>
                  <p class="sdd-verify-notes">{verifyForActiveStage.verdict.notes}</p>
                </details>
              {/if}
            </div>
          {/if}
        {/if}
      {/if}
    </div>
  {/if}

  {#if diffTargetPhase != null && phaseDiffByPhase[diffTargetPhase] !== undefined}
    {@const diff = phaseDiffByPhase[diffTargetPhase]}
    <!-- Phase diff drawer — collapsible "Files changed" view showing
         per-file stats from `git diff <pre>..<post>`. Click a row to
         load + render its unified-diff patch lazily. Auto-expanded
         in fullscreen / lightbox; collapsed inline by default so the
         card stays compact. -->
    <div class="sdd-diff-drawer">
      <button
        type="button"
        class="sdd-diff-drawer-head"
        onclick={() => togglePhaseDiff(diffTargetPhase)}
        aria-expanded={phaseDiffOpenPhase === diffTargetPhase}
      >
        <svg
          class="sdd-chev"
          class:open={phaseDiffOpenPhase === diffTargetPhase}
          viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"
        >
          <path d="M9 6l6 6-6 6"/>
        </svg>
        {#if diff === 'loading' || diff == null}
          <span class="mono">Files changed · loading…</span>
        {:else if diff.skipped}
          <span class="mono">Files changed · git snapshot was skipped for this phase</span>
        {:else}
          <span class="mono">
            Files changed ({diff.files.length})
            <span class="sdd-diff-ins">+{diff.total_insertions}</span>
            <span class="sdd-diff-del">−{diff.total_deletions}</span>
          </span>
        {/if}
      </button>
      {#if phaseDiffOpenPhase === diffTargetPhase && diff && diff !== 'loading' && !diff.skipped}
        <ul class="sdd-diff-files">
          {#each diff.files as f (f.path)}
            {@const key = fileDiffKey(diffTargetPhase, f.path)}
            {@const open = openDiffFiles[key]}
            <li class="sdd-diff-file" data-status={f.status}>
              <button
                type="button"
                class="sdd-diff-file-row mono"
                onclick={() => !f.is_binary && toggleFileDiff(diffTargetPhase, f.path)}
                disabled={f.is_binary}
                title={f.is_binary ? 'binary file — patch not shown' : 'click to expand inline diff'}
              >
                <span class="sdd-diff-file-status">{f.status}</span>
                <span class="sdd-diff-file-path">{f.path}</span>
                {#if f.is_binary}
                  <span class="sdd-diff-file-bin">binary</span>
                {:else}
                  <span class="sdd-diff-ins">+{f.insertions}</span>
                  <span class="sdd-diff-del">−{f.deletions}</span>
                {/if}
              </button>
              {#if open !== undefined && !f.is_binary}
                <div class="sdd-diff-file-body">
                  {#if open === 'loading'}
                    <span class="sdd-diff-file-loading mono">loading patch…</span>
                  {:else if open}
                    <pre class="sdd-diff-patch mono">{open}</pre>
                  {:else}
                    <span class="sdd-diff-file-loading mono">no patch returned</span>
                  {/if}
                </div>
              {/if}
            </li>
          {/each}
        </ul>
      {/if}
    </div>
  {/if}

  {#if stage.kind === 'failed'}
    <!-- Structured failure card — replaces v1's one-liner reason.
         Header carries trigger label, body lists failed acceptance
         checks (collapsible log_tail per check) + last action-log
         entries before failure for context. Action row lives in the
         main footer so chrome stays consistent with non-failed states. -->
    <div class="sdd-failed">
      <div class="sdd-failed-head">
        <span class="sdd-failed-title">
          {#if stage.failed_phase != null}
            Phase {stage.failed_phase} failed
          {:else}
            Workflow failed
          {/if}
          {#if stage.trigger}
            <span class="sdd-failed-trigger mono">· {stage.trigger.replace('_', ' ')}</span>
          {/if}
        </span>
      </div>
      <div class="sdd-failed-reason">{stage.reason}</div>

      {#if (stage.failed_checks?.length ?? 0) > 0}
        <div class="sdd-failed-checks-line mono">
          Failed checks: {(stage.failed_checks ?? []).map((i) => `#${i + 1}`).join(', ')}
        </div>
      {/if}

      {#if (stage.action_log_tail?.length ?? 0) > 0}
        <details class="sdd-failed-tail">
          <summary class="mono">
            Last actions · {stage.action_log_tail?.length ?? 0}
          </summary>
          <ul class="sdd-failed-tail-list">
            {#each (stage.action_log_tail ?? []).slice(-5) as e, idx (e.correlation_id ?? `${e.ts}-${e.kind}-${idx}`)}
              <li class="sdd-failed-tail-row mono" data-status={e.status ?? 'done'}>
                <span class="sdd-activity-tool">{e.tool ?? e.kind}</span>
                <span class="sdd-activity-summary">{e.summary}</span>
              </li>
            {/each}
          </ul>
        </details>
      {/if}

      {#if skipMode}
        <div class="sdd-skip">
          <textarea
            class="sdd-skip-area mono"
            bind:value={skipDraft}
            placeholder="Why is this phase being skipped? (min 5 chars — recorded for audit)"
            rows="3"
            spellcheck="false"
            {@attach (node: HTMLTextAreaElement) => node.focus()}
            onkeydown={(e) => {
              if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') { e.preventDefault(); void submitSkip(); }
              if (e.key === 'Escape') { e.preventDefault(); cancelSkip(); }
            }}
          ></textarea>
        </div>
      {/if}
      {#if acceptMode}
        <div class="sdd-skip">
          <textarea
            class="sdd-skip-area mono"
            bind:value={acceptDraft}
            placeholder="Why are these deviations acceptable? (min 5 chars — recorded for audit, phase flips to done)"
            rows="3"
            spellcheck="false"
            {@attach (node: HTMLTextAreaElement) => node.focus()}
            onkeydown={(e) => {
              if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') { e.preventDefault(); void submitAccept(); }
              if (e.key === 'Escape') { e.preventDefault(); cancelAccept(); }
            }}
          ></textarea>
        </div>
      {/if}
    </div>
  {/if}

  {#if amendMode}
    <!-- Amend panel — user describes a change to the CURRENT artifact;
         on send the agent gets an "edit existing files" prompt instead
         of the natural next-stage prompt. Cancels back to the normal
         actions row. -->
    <div class="sdd-amend">
      <label class="sdd-amend-label">
        <span class="sdd-amend-hint">Describe the change. Agent will edit `{p.workspace.root.split('/').pop()}` in place — spec / plan / current phase — instead of starting over.</span>
        <textarea
          class="sdd-amend-area mono"
          bind:value={amendDraft}
          placeholder="e.g. drop phase 4, retitle phase 2 to “Combat”, replace Unity with Godot, add an audio router task…"
          rows="4"
          spellcheck="false"
          {@attach (node: HTMLTextAreaElement) => node.focus()}
          onkeydown={(e) => {
            if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') { e.preventDefault(); void sendAmend(); }
            if (e.key === 'Escape') { e.preventDefault(); cancelAmend(); }
          }}
        ></textarea>
      </label>
    </div>
  {/if}

  {#if p.viewOnly && stage.kind === 'failed'}
    <!-- ViewOnly failure footer — popover-opened failed workspaces
         need Retry / Accept / Skip / Rollback affordances even in
         the standalone overlay; otherwise the failure card opens
         read-only and the user has no recovery path. -->
    <footer class="sdd-actions">
      {#if skipMode}
        <button type="button" class="sdd-btn" onclick={cancelSkip}>cancel</button>
        <button
          type="button"
          class="sdd-btn sdd-btn--primary"
          disabled={skipDraft.trim().length < 5}
          onclick={submitSkip}
          title="⌘↵ to submit"
        >Skip with reason</button>
      {:else if acceptMode}
        <button type="button" class="sdd-btn" onclick={cancelAccept}>cancel</button>
        <button
          type="button"
          class="sdd-btn sdd-btn--primary"
          disabled={acceptDraft.trim().length < 5}
          onclick={submitAccept}
          title="⌘↵ to submit — flips status to done"
        >Accept with reason</button>
      {:else}
        <button class="sdd-btn sdd-btn--primary" disabled={advanceClicked} onclick={onRetry}>Retry phase</button>
        <button class="sdd-btn" onclick={startAccept} title="Mark this failed phase as done with an audit reason">
          ✓ Accept anyway
        </button>
        <button class="sdd-btn" onclick={startSkip} title="Force-skip with audit reason">Skip phase</button>
        <button class="sdd-btn" onclick={onRollbackFailed} title="Reset working tree to pre-phase commit">↶ Rollback</button>
      {/if}
    </footer>
  {/if}

  {#if !p.viewOnly}
  <footer class="sdd-actions">
    {#if editMode}
      <!-- Edit-mode footer merges into the card's main actions row —
           cancel + save sit alongside Discard as siblings of the same
           grammar (text-buttons + accent-pill primary), no separate
           edit-actions bar competing for attention. -->
      <button type="button" class="sdd-btn" onclick={cancelEdit}>cancel</button>
      <button type="button" class="sdd-btn sdd-btn--primary" onclick={saveEdit}>save</button>
    {:else if amendMode}
      <button type="button" class="sdd-btn" onclick={cancelAmend}>cancel</button>
      <button
        type="button"
        class="sdd-btn sdd-btn--primary"
        disabled={!amendDraft.trim() || advanceClicked}
        onclick={sendAmend}
        title="⌘↵"
      >{advanceClicked ? 'sending…' : 'Send change'}</button>
    {:else}
      {#if isAwaitingApproval}
        <button class="sdd-btn sdd-btn--primary" disabled={advanceClicked} onclick={advance}>
          {advanceClicked ? 'sending…' : actionLabel()}
        </button>
      {/if}
      {#if stage.kind === 'phase_plan_review'}
        <!-- Plan-review Discard — flips the phase to skipped with a
             plan_discarded audit reason. The full failure-card
             grammar (Retry / Edit & retry) is overkill here; user
             intent is "abandon this plan, move on". -->
        <button
          class="sdd-btn"
          disabled={advanceClicked}
          onclick={discardPlanReview}
          title="Skip this phase — plan rejected"
        >Discard plan</button>
      {/if}
      {#if isInFlight}
        {#if canManualContinue}
          <!-- Manual re-fire — stage is "in-flight" but the agent
               turn ended without the auto-fire dispatcher catching
               up (production bundle predates the fix, or the
               dispatcher silently dropped). User clicks Continue to
               push the next-substep prompt through again. -->
          <button
            class="sdd-btn sdd-btn--primary"
            disabled={continueClicked}
            onclick={onContinue}
            title="Re-fire the prompt for this substep"
          >{continueClicked ? 'sending…' : continueLabel}</button>
        {/if}
        <button class="sdd-btn" onclick={onPause}>Pause</button>
      {/if}
      {#if isPaused}
        <button class="sdd-btn sdd-btn--primary" onclick={onResume}>Resume</button>
      {/if}
      {#if stage.kind === 'failed'}
        {#if skipMode}
          <button type="button" class="sdd-btn" onclick={cancelSkip}>cancel</button>
          <button
            type="button"
            class="sdd-btn sdd-btn--primary"
            disabled={skipDraft.trim().length < 5}
            onclick={submitSkip}
            title="⌘↵ to submit"
          >Skip with reason</button>
        {:else if acceptMode}
          <button type="button" class="sdd-btn" onclick={cancelAccept}>cancel</button>
          <button
            type="button"
            class="sdd-btn sdd-btn--primary"
            disabled={acceptDraft.trim().length < 5}
            onclick={submitAccept}
            title="⌘↵ to submit — flips status to done"
          >Accept with reason</button>
        {:else}
          <button class="sdd-btn sdd-btn--primary" disabled={advanceClicked} onclick={onRetry}>Retry phase</button>
          <button class="sdd-btn" disabled={advanceClicked} onclick={startEditAndRetry} title="Edit phase body, then retry">
            ✎ Edit & retry
          </button>
          <!-- Accept anyway — flip status failed → done with a recorded
               reason. Use when verifier deviations are reviewed and
               judged tolerable trade-offs (the agent's verify summary
               already explains why). Distinct from Skip which leaves
               the phase un-done. -->
          <button class="sdd-btn" onclick={startAccept} title="Mark this failed phase as done with an audit reason — workflow continues from next phase">
            ✓ Accept anyway
          </button>
          <button class="sdd-btn" onclick={startSkip} title="Force-skip with audit reason">Skip phase</button>
          <button class="sdd-btn" onclick={onRollbackFailed} title="Reset working tree to pre-phase commit">↶ Rollback</button>
        {/if}
      {/if}
      {#if !isTerminal && !isInFlight}
        <!-- Amend affordance — only when the workspace is in a settled
             state (drafting/spec_ready/plan_ready/phase_done). During
             in-flight the agent is busy; let it finish before
             corrections. Discard / Stop still available. -->
        <button class="sdd-btn" onclick={startAmend} title="Tell the agent to change the current spec / plan / phase in place">
          ✎ Amend
        </button>
      {/if}
      {#if undoVisible}
        <button class="sdd-btn" onclick={onUndo} title="Restore the body that was there before your last save">
          ↶ Undo last edit ({undoSecondsLeft}s)
        </button>
      {/if}
      {#if !isTerminal}
        <button class="sdd-btn" onclick={onStop}>Stop</button>
      {/if}
    {/if}
    <button class="sdd-btn sdd-btn--mute" onclick={onDiscard}>Discard</button>
  </footer>
  {/if}
  {#if auditOpen}
    <div class="sdd-audit-overlay" role="dialog" aria-label="SDD audit log">
      <header class="sdd-audit-head">
        <span class="sdd-audit-title">Audit log</span>
        <span class="sdd-audit-count mono">{filteredAudit.length} of {auditEntries.length}</span>
        <span class="sdd-audit-spacer"></span>
        <label class="sdd-audit-filter">
          <span class="vh">Filter by source</span>
          <select bind:value={auditFilter} class="mono">
            <option value="all">all</option>
            <option value="agent">agent</option>
            <option value="user">user</option>
            <option value="system">system</option>
          </select>
        </label>
        <button class="sdd-btn sdd-btn--mute" type="button" onclick={copyAuditAsJsonl}>Copy JSONL</button>
        <button class="sdd-btn sdd-btn--mute" type="button" onclick={() => (auditOpen = false)}>Close</button>
      </header>
      <div class="sdd-audit-body">
        {#if filteredAudit.length === 0}
          <p class="sdd-audit-empty">No audit entries yet.</p>
        {:else}
          <ul class="sdd-audit-list">
            {#each filteredAudit as e, idx (e.ts + e.action + (e.phase ?? -1) + '|' + idx)}
              {@const expanded = auditExpandedTs === e.ts}
              <li class="sdd-audit-row" data-source={e.source}>
                <button
                  type="button"
                  class="sdd-audit-row-head"
                  onclick={() => (auditExpandedTs = expanded ? null : e.ts)}
                  aria-expanded={expanded}
                >
                  <span class="sdd-audit-ts mono">{fmtAuditTs(e.ts)}</span>
                  <span class="sdd-audit-source mono" data-source={e.source}>{e.source}</span>
                  <span class="sdd-audit-action mono">{e.action}</span>
                  {#if e.phase != null}
                    <span class="sdd-audit-phase mono">phase {e.phase}</span>
                  {/if}
                  {#if e.reason}
                    <span class="sdd-audit-reason">— {e.reason}</span>
                  {/if}
                </button>
                {#if expanded && (e.before !== undefined || e.after !== undefined)}
                  <div class="sdd-audit-snap mono">
                    {#if e.before !== undefined}
                      <details open>
                        <summary>before</summary>
                        <pre>{JSON.stringify(e.before, null, 2)}</pre>
                      </details>
                    {/if}
                    {#if e.after !== undefined}
                      <details open>
                        <summary>after</summary>
                        <pre>{JSON.stringify(e.after, null, 2)}</pre>
                      </details>
                    {/if}
                  </div>
                {/if}
              </li>
            {/each}
          </ul>
        {/if}
      </div>
    </div>
  {/if}
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
  /* Quiet blockquote — same grammar as `.cac` / `.qc`. No tint, no
   *  rounded right edge; the left accent rail does all the heavy
   *  lifting, typography matches surrounding prose. Per-tone variants
   *  shift the rail colour only, so a live / warn / ok SDD phase
   *  still reads as the SAME element with a different mood. */
  .sdd-card {
    border-left: 2px solid color-mix(in srgb, var(--accent) 75%, transparent);
    background: transparent;
    padding: 4px 0 6px 14px;
    display: flex; flex-direction: column;
    gap: 8px;
    min-width: 0;
    color: var(--text-1);
    font-size: 13.5px;
    line-height: 1.55;
    transition: background 160ms, border-left-color 160ms;
  }
  .sdd-card:hover { background: color-mix(in srgb, var(--accent) 4%, transparent); }
  .sdd-card:focus-within { border-left-color: var(--accent); }
  .sdd-card[data-tone="live"] {
    border-left-color: color-mix(in srgb, #66d39a 75%, transparent);
  }
  .sdd-card[data-tone="live"]:hover {
    background: color-mix(in srgb, #66d39a 4%, transparent);
  }
  .sdd-card[data-tone="warn"] {
    border-left-color: color-mix(in srgb, #e0b16c 75%, transparent);
  }
  .sdd-card[data-tone="warn"]:hover {
    background: color-mix(in srgb, #e0b16c 4%, transparent);
  }
  .sdd-card[data-tone="ok"] {
    border-left-color: color-mix(in srgb, var(--accent) 75%, transparent);
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

  /* Live activity feed — quiet, monospace, status-dot driven. Sits
   *  between phases-pills and the body so it's always within reach
   *  while a phase is running. Transparent bg + 2px accent stripe to
   *  match the established quiet-blockquote idiom of inline cards. */
  .sdd-activity {
    margin-top: 6px;
    padding: 4px 0 4px 10px;
    border-left: 2px solid color-mix(in oklab, var(--accent-source, currentColor) 60%, transparent);
    background: transparent;
    font-size: 12px;
    color: var(--text-1);
  }
  .sdd-activity-head {
    display: flex; align-items: baseline; justify-content: space-between;
    gap: 8px;
    margin-bottom: 3px;
  }
  .sdd-activity-title {
    color: var(--text-2);
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .sdd-activity-more {
    appearance: none;
    background: transparent;
    border: 0;
    padding: 0;
    cursor: pointer;
    color: var(--text-2);
    font-size: 11px;
  }
  .sdd-activity-more:hover { color: var(--text-0); }
  .sdd-activity-list {
    list-style: none;
    margin: 0; padding: 0;
    display: flex; flex-direction: column; gap: 2px;
    max-height: 240px;
    overflow-y: auto;
  }
  .sdd-activity-row {
    display: grid;
    grid-template-columns: 8px auto 1fr;
    align-items: center;
    gap: 8px;
    line-height: 1.45;
    min-width: 0;
  }
  .sdd-activity-dot {
    width: 6px; height: 6px;
    border-radius: 50%;
    background: var(--text-3);
    align-self: center;
  }
  .sdd-activity-row[data-status="running"] .sdd-activity-dot {
    background: color-mix(in oklab, var(--info, #3aa) 80%, transparent);
    animation: sdd-activity-pulse 1.4s ease-in-out infinite;
  }
  .sdd-activity-row[data-status="done"] .sdd-activity-dot {
    background: color-mix(in oklab, var(--success, #4a7) 70%, transparent);
  }
  .sdd-activity-row[data-status="failed"] .sdd-activity-dot {
    background: color-mix(in oklab, var(--danger, #c44) 80%, transparent);
  }
  @keyframes sdd-activity-pulse {
    0%, 100% { opacity: 0.55; }
    50%       { opacity: 1.0; }
  }
  .sdd-activity-tool {
    font-family: var(--font-mono);
    font-size: 11px;
    color: var(--text-2);
    white-space: nowrap;
  }
  .sdd-activity-summary {
    color: var(--text-1);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }
  /* Phase pills look like inline code spans — same font + bg
   *  language as `<code>` from prose. Reinforces "this is part of the
   *  message" feel rather than "this is UI chrome". */
  .sdd-phase-pill {
    display: inline-flex; align-items: center;
    gap: 5px;
    font-family: 'JetBrains Mono', monospace;
    font-size: 10.5px;
    padding: 2px 8px;
    border-radius: 4px;
    border: 1px solid var(--border-neutral-hi);
    background: var(--bg-2);
    color: var(--text-2);
    cursor: pointer;
    transition: background 120ms, border-color 120ms, color 120ms;
  }
  .sdd-phase-pill:hover {
    background: var(--bg-3);
    border-color: var(--border-hi);
    color: var(--text-0);
  }
  /* Selected (currently peeking this phase) — accent ring + brighter
   *  text, so the user can see WHICH pill drove the body content. */
  .sdd-phase-pill--selected {
    border-color: var(--accent) !important;
    background: color-mix(in srgb, var(--accent) 22%, var(--bg-2)) !important;
    color: var(--text-0) !important;
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--accent) 45%, transparent);
  }
  .sdd-phase-dot {
    margin-left: 2px;
    font-size: 8px;
    color: var(--accent-bright);
    opacity: 0.85;
  }
  .sdd-phase-reset {
    margin-left: 4px;
    background: transparent;
    border: 0;
    color: var(--text-mute);
    font-size: 11px;
    cursor: pointer;
    padding: 1px 4px;
    border-radius: 3px;
    transition: color 120ms, background 120ms;
  }
  .sdd-phase-reset:hover {
    color: var(--accent-bright);
    background: color-mix(in srgb, var(--accent) 8%, transparent);
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

  /* Expand-to-fullscreen icon. Sits at the row's right edge as a
   *  square glyph button — same vocabulary as DiffView's ⛶ control
   *  so the gesture transfers between surfaces. */
  .sdd-expand {
    margin-left: auto;
    border: 0;
    background: transparent;
    color: var(--text-mute);
    cursor: pointer;
    display: inline-flex; align-items: center; justify-content: center;
    width: 22px; height: 22px;
    border-radius: 4px;
    transition: background 100ms, color 100ms;
  }
  .sdd-expand:hover {
    background: color-mix(in srgb, var(--accent) 8%, transparent);
    color: var(--accent-bright);
  }

  /* Close-X button — only shown in fullscreen mode. Sits at the
   *  right edge of the header where the sdd-id chip normally lives.
   *  Same square-glyph treatment as .sdd-expand. */
  .sdd-close {
    margin-left: 4px;
    border: 0;
    background: transparent;
    color: var(--text-mute);
    cursor: pointer;
    display: inline-flex; align-items: center; justify-content: center;
    width: 24px; height: 24px;
    border-radius: 4px;
    transition: background 100ms, color 100ms;
  }
  .sdd-hide {
    margin-left: auto;
    border: 0;
    background: transparent;
    color: var(--text-mute);
    cursor: pointer;
    display: inline-flex; align-items: center; justify-content: center;
    width: 22px; height: 22px;
    border-radius: 4px;
    transition: background 100ms, color 100ms;
  }
  .sdd-hide:hover {
    background: color-mix(in srgb, var(--accent) 8%, transparent);
    color: var(--accent-bright);
  }
  .sdd-cog {
    border: 0;
    background: transparent;
    color: var(--text-mute);
    cursor: pointer;
    display: inline-flex; align-items: center; justify-content: center;
    width: 22px; height: 22px;
    border-radius: 4px;
    transition: background 100ms, color 100ms;
  }
  .sdd-cog:hover, .sdd-cog--open {
    background: color-mix(in srgb, var(--accent) 12%, transparent);
    color: var(--accent-bright);
  }
  .sdd-config-drawer {
    margin: 6px 0 8px 0;
    padding: 8px 12px;
    border: 1px solid color-mix(in srgb, var(--accent) 22%, transparent);
    border-radius: 6px;
    background: color-mix(in srgb, var(--accent) 4%, transparent);
    display: flex;
    flex-direction: column;
    gap: 6px;
    font-size: 12px;
  }
  .sdd-config-row {
    display: inline-flex;
    align-items: center;
    gap: 8px;
  }
  .sdd-config-row--toggle { font-size: 11.5px; color: var(--text-1); }
  .sdd-config-row--toggle input[disabled] { opacity: 0.4; }
  .sdd-config-label { color: var(--text-1); }
  .sdd-config-select {
    padding: 3px 6px;
    border-radius: 4px;
    border: 1px solid var(--border);
    background: var(--bg-1);
    color: var(--text-0);
    font-size: 11px;
  }
  .sdd-config-hint {
    margin: 2px 0 0 0;
    font-size: 11px;
    color: var(--text-mute);
    line-height: 1.4;
  }

  .sdd-close:hover {
    background: color-mix(in srgb, var(--error) 14%, transparent);
    color: var(--error);
  }

  /* Lightbox / fullscreen mode — pulls the card out of the chat
   *  scroll, pins it as a viewport-cover panel. Mostly opaque at rest
   *  so spec / plan text is readable without chat bleed-through; hint
   *  of translucency + blur keeps the "overlay" feel. Hover/focus goes
   *  fully solid for active reading. Close via × in header or Esc. */
  .sdd-card--full {
    position: fixed;
    inset: 4vh 4vw;
    z-index: 1000;
    max-width: none;
    /* Near-opaque bg — was 55% which leaked the chat through the body
     * text. 96% keeps a sliver of see-through so the overlay still
     * feels like a lightbox, but reading is no longer painful. Heavier
     * blur kills any residual focus-fighting from chat behind. */
    background: color-mix(in srgb, var(--bg-1) 96%, transparent);
    border-left: 2px solid color-mix(in srgb, var(--accent) 55%, transparent);
    box-shadow: 0 8px 30px rgba(0, 0, 0, 0.35);
    border-radius: 8px;
    padding: 18px 28px 20px;
    overflow: hidden;
    display: flex; flex-direction: column;
    backdrop-filter: blur(12px);
    transition: background 180ms ease, border-color 180ms ease, box-shadow 180ms ease;
  }
  .sdd-card--full:hover,
  .sdd-card--full:focus-within {
    background: var(--bg-1);
    border-left-color: var(--accent);
    box-shadow: 0 14px 44px rgba(0, 0, 0, 0.45);
  }
  .sdd-card--full .sdd-body {
    flex: 1 1 0; min-height: 0;
    display: flex; flex-direction: column;
  }
  /* Pin the action footer to the bottom of the fullscreen card. Without
   * this the flex column lets the actions row hug whatever came
   * before it, leaving empty space below — chat behind shows through
   * and the card reads as half-filled. */
  .sdd-card--full .sdd-actions { margin-top: auto; }
  /* Fullscreen reading column — drop the line-length cap so the body
   *  fills the entire card width. The chat thread behind doesn't read
   *  as the foreground (the card's translucent bg already segregates
   *  them visually), so a wider measure makes better use of the
   *  fullscreen real estate. Pad sides instead of capping width. */
  .sdd-card--full .sdd-body-content {
    flex: 1 1 0; min-height: 0;
    /* Override the 360px cap from the inline-mode rule above —
     * in fullscreen the body fills whatever vertical space the
     * card's flex column gives it. */
    max-height: none;
    /* Drop the inline-quote chrome (left rule + indent + tint
     * border) for fullscreen reading — at this scale it reads as
     * unnecessary chrome between the heading and the text. */
    border-left: 0;
    padding: 0 24px;
    margin-top: 4px;
    overflow-y: auto;
    font-size: 15px;
    line-height: 1.75;
    width: 100%;
    max-width: none;
  }
  .sdd-card--full .sdd-body-content :global(p),
  .sdd-card--full .sdd-body-content :global(li) { font-size: 15px; }
  .sdd-card--full .sdd-body-content :global(h1) { font-size: 24px; margin-top: 24px; }
  .sdd-card--full .sdd-body-content :global(h2) { font-size: 20px; margin-top: 22px; }
  .sdd-card--full .sdd-body-content :global(h3) { font-size: 17px; margin-top: 18px; }
  .sdd-card--full .sdd-body-content :global(pre) { font-size: 13.5px; line-height: 1.55; }
  .sdd-card--full .sdd-body-content :global(table) { width: 100%; }
  .sdd-card--full .sdd-body-edit {
    flex: 1; min-height: 0;
    display: flex; flex-direction: column;
    width: 100%;
    max-width: none;
  }
  .sdd-card--full .sdd-edit-area {
    flex: 1;
    min-height: 0;
    height: auto;
    font-size: 14px;
    line-height: 1.7;
  }

  /* Edit-mode textarea — fills the body slot while editing. mono +
   *  subtle accent border to feel like an "agent's draft you're
   *  amending" rather than a generic form input. */
  .sdd-body-edit {
    display: flex; flex-direction: column;
    gap: 6px;
  }
  /* Edit area — drop the heavy mono frame. Reads as a continuation
   *  of the card's body rather than a form-input transplanted in.
   *  Bg is a touch darker than the card's accent-tint to signal
   *  editability without breaking the surface. Left border-stripe
   *  echoes the card's outer accent stripe (recursive structure). */
  .sdd-edit-area {
    width: 100%;
    min-height: 240px;
    padding: 6px 0 6px 12px;
    border: 0;
    border-left: 2px solid color-mix(in srgb, var(--accent) 30%, transparent);
    background: color-mix(in srgb, var(--bg-0) 70%, transparent);
    color: var(--text-0);
    font-family: 'JetBrains Mono', monospace;
    font-size: 12px;
    line-height: 1.55;
    resize: vertical;
    outline: 0;
  }
  .sdd-edit-area:focus {
    border-left-color: var(--accent);
    background: color-mix(in srgb, var(--bg-0) 85%, transparent);
  }
  /* Read-only `<pre>` for the split view's left pane — same metrics
   *  as the textarea so visible line alignment tracks. */
  .sdd-edit-area--readonly {
    white-space: pre-wrap;
    overflow-y: auto;
    overflow-x: hidden;
    word-break: break-word;
    color: var(--text-2);
    margin: 0;
    cursor: default;
    resize: none;
    user-select: text;
  }
  .sdd-edit-area--readonly:focus { border-left-color: color-mix(in srgb, var(--accent) 30%, transparent); }

  /* Inline view switch — three text-buttons separated by · in the
   *  same body-row as the file title. No pill chrome, no panel —
   *  reads as a typographic switch within the line. Active state is
   *  just color: text-0; inactive stays text-mute. */
  .sdd-view-switch {
    display: inline-flex; align-items: baseline;
    gap: 4px;
    margin-left: auto;
    font-size: 11px;
    color: var(--text-mute);
  }
  .sdd-view-tab {
    padding: 0;
    border: 0;
    background: transparent;
    color: var(--text-mute);
    font-size: 11px;
    cursor: pointer;
    transition: color 120ms;
  }
  .sdd-view-tab:hover { color: var(--accent-bright); }
  .sdd-view-tab.active { color: var(--text-0); }
  .sdd-view-sep {
    color: var(--text-mute);
    opacity: 0.5;
    user-select: none;
  }

  /* Side-by-side split — fixed two-column grid so original (left)
   *  and draft (right) keep equal width. min-width: 0 on grid
   *  children prevents long lines from forcing overflow. */
  .sdd-diff-split {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 12px;
  }
  .sdd-diff-split > * { min-width: 0; }

  /* Unified diff — no separate box, just spacing. The renderer's
   *  inner `<pre class="diff-block">` carries its own padding via
   *  Markdown.svelte's global rules; we just provide the scroll
   *  cap + sit on the same accent-tinted card bg. */
  .sdd-diff-unified {
    min-height: 240px;
    max-height: 480px;
    overflow-y: auto;
    padding-left: 12px;
    border-left: 2px solid color-mix(in srgb, var(--accent) 30%, transparent);
  }
  .sdd-diff-unified :global(pre.diff-block) {
    margin: 0;
    border: 0;
    background: transparent;
    padding: 0;
  }
  /* Failure card — quiet-blockquote grammar with a warn-tone left rail
   *  (not error-red — keeps the card in the same visual register as
   *  the rest of SDD; we don't shout at the user). Header carries
   *  trigger badge, body lists failed checks + last actions, footer
   *  chrome (retry / edit / skip / rollback) lives in the main
   *  actions row so layout stays consistent. */
  .sdd-failed {
    display: flex; flex-direction: column;
    gap: 6px;
    padding: 4px 0 4px 10px;
    border-left: 2px solid color-mix(in srgb, #e0b16c 75%, transparent);
    background: color-mix(in srgb, #e0b16c 6%, transparent);
    color: var(--text-1);
    font-size: 12.5px;
    line-height: 1.5;
  }
  .sdd-failed-head {
    display: flex; align-items: center;
    gap: 8px;
  }
  .sdd-failed-title {
    font-size: 12.5px;
    font-weight: 500;
    color: var(--text-0);
  }
  .sdd-failed-trigger {
    color: var(--text-mute);
    font-size: 11px;
    font-weight: 400;
  }
  .sdd-failed-reason {
    color: var(--text-1);
  }
  .sdd-failed-checks-line {
    color: var(--text-mute);
    font-size: 11.5px;
  }
  .sdd-failed-tail summary,
  .sdd-failed-checks summary {
    cursor: pointer;
    color: var(--text-mute);
    font-size: 11px;
    user-select: none;
  }
  .sdd-failed-tail-list {
    list-style: none;
    padding: 4px 0 0 0;
    margin: 0;
    display: flex; flex-direction: column;
    gap: 2px;
  }
  .sdd-failed-tail-row {
    display: flex; gap: 8px;
    color: var(--text-mute);
    font-size: 11px;
  }
  .sdd-failed-tail-row .sdd-activity-summary {
    color: var(--text-1);
  }
  /* Inline skip-with-reason form — sits inside the failure card. */
  .sdd-skip {
    display: flex;
  }
  .sdd-skip-area {
    width: 100%;
    min-height: 64px;
    padding: 6px 0 6px 10px;
    border: 0;
    border-left: 2px solid color-mix(in srgb, var(--accent) 30%, transparent);
    background: color-mix(in srgb, var(--bg-0) 70%, transparent);
    color: var(--text-0);
    font-family: 'JetBrains Mono', monospace;
    font-size: 12px;
    line-height: 1.5;
    resize: vertical;
    outline: 0;
  }
  .sdd-skip-area:focus {
    border-left-color: var(--accent);
  }

  /* Phase-diff drawer — same chevron + mono-title grammar as
   *  `.sdd-body-toggle` so it reads as another expandable section
   *  rather than a separate element. */
  .sdd-diff-drawer {
    display: flex; flex-direction: column;
    gap: 4px;
  }
  .sdd-diff-drawer-head {
    display: inline-flex; align-items: center;
    gap: 5px;
    padding: 1px 0;
    margin: 0;
    border: 0;
    background: transparent;
    color: var(--text-1);
    font-size: 12.5px;
    line-height: 1.4;
    cursor: pointer;
    user-select: none;
    align-self: flex-start;
  }
  .sdd-diff-drawer-head:hover { color: var(--text-0); }
  .sdd-diff-ins { color: var(--success, #66d39a); }
  .sdd-diff-del { color: #d77e7e; }
  .sdd-diff-files {
    list-style: none;
    padding: 0;
    margin: 0;
    display: flex; flex-direction: column;
  }
  .sdd-diff-file-row {
    display: flex; align-items: center;
    gap: 8px;
    padding: 2px 0 2px 14px;
    margin: 0;
    border: 0;
    background: transparent;
    color: var(--text-1);
    font-size: 11.5px;
    line-height: 1.5;
    text-align: left;
    cursor: pointer;
    width: 100%;
  }
  .sdd-diff-file-row:hover:not(:disabled) {
    background: color-mix(in srgb, var(--accent) 4%, transparent);
  }
  .sdd-diff-file-row:disabled { cursor: default; opacity: 0.7; }
  .sdd-diff-file-status {
    width: 14px; text-align: center;
    color: var(--text-mute);
  }
  .sdd-diff-file-path { flex: 1; }
  .sdd-diff-file-bin {
    color: var(--text-mute);
    font-size: 10.5px;
  }
  .sdd-diff-file-body {
    padding: 4px 0 6px 28px;
  }
  .sdd-diff-patch {
    margin: 0;
    padding: 6px 8px;
    background: color-mix(in srgb, var(--bg-0) 70%, transparent);
    border-left: 2px solid color-mix(in srgb, var(--accent) 30%, transparent);
    font-size: 11.5px;
    line-height: 1.45;
    white-space: pre;
    overflow-x: auto;
    max-height: 360px;
    overflow-y: auto;
  }
  .sdd-diff-file-loading {
    color: var(--text-mute);
    font-size: 11px;
  }
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

  /* Verify pane — structured render of verify.json. Quiet card
   * styling so it sits inline with the body without competing for
   * attention. Deviations get a warn-tone accent. */
  .sdd-verify-pane {
    margin-top: 10px;
    padding: 6px 0 4px 12px;
    border-left: 2px solid color-mix(in srgb, var(--accent) 22%, transparent);
    font-size: 12px;
    color: var(--text-1);
  }
  .sdd-verify-pane[data-deviated="true"] {
    border-left-color: color-mix(in srgb, var(--error) 50%, transparent);
  }
  .sdd-verify-head {
    font-size: 10px;
    color: var(--text-mute);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    margin-bottom: 4px;
  }
  .sdd-verify-summary {
    margin: 4px 0 6px 0;
    line-height: 1.5;
  }
  .sdd-verify-section {
    margin: 3px 0;
  }
  .sdd-verify-section > summary {
    cursor: pointer;
    font-size: 11px;
    color: var(--text-mute);
    padding: 2px 0;
  }
  .sdd-verify-section--warn > summary {
    color: var(--error);
  }
  .sdd-verify-files, .sdd-verify-list {
    margin: 4px 0 6px 0;
    padding-left: 16px;
    list-style: none;
  }
  .sdd-verify-files li {
    padding: 1px 0;
    color: var(--text-1);
  }
  .sdd-verify-list li {
    padding: 2px 0;
    line-height: 1.5;
  }
  .sdd-verify-notes {
    margin: 4px 0;
    color: var(--text-1);
    font-style: italic;
  }

  /* Actions row — buttons stay typographic but now have a visible
   *  hairline + readable text color so the user can find them
   *  without squinting. Earlier version was `color: text-mute` over
   *  a transparent bg, which on the SDD card's accent-tinted surface
   *  read as near-invisible. Primary CTA carries a real fill + border
   *  so it pops at first glance. */
  .sdd-actions {
    display: flex; align-items: center;
    gap: 8px;
    margin-top: 6px;
  }
  .sdd-btn {
    padding: 4px 10px;
    border-radius: 5px;
    border: 1px solid var(--border-neutral-hi);
    background: var(--bg-2);
    color: var(--text-1);
    font-size: 12px;
    font-weight: 500;
    cursor: pointer;
    transition: background 120ms, color 120ms, border-color 120ms;
  }
  .sdd-btn:hover:not(:disabled) {
    color: var(--text-0);
    background: var(--bg-3);
    border-color: var(--border-hi);
  }
  .sdd-btn:disabled { opacity: 0.5; cursor: not-allowed; }
  /* Primary CTA — accent fill with a saturated stroke so it reads as
   *  THE action on the card. Bumped fill % vs the prior soft tint so
   *  it doesn't disappear into the card's own accent-tinted surface. */
  .sdd-btn--primary {
    padding: 4px 14px;
    border-radius: 5px;
    background: var(--accent);
    border: 1px solid var(--accent);
    color: var(--accent-fg);
    font-weight: 600;
    font-size: 12px;
  }
  .sdd-btn--primary:hover:not(:disabled) {
    background: var(--accent-bright);
    border-color: var(--accent-bright);
    color: var(--accent-fg);
  }
  /* Discard — pushed to the far right + tinted with error edge so
   *  it reads as a destructive escape hatch without screaming. */
  .sdd-btn--mute {
    margin-left: auto;
    color: var(--text-2);
    border-color: var(--border-neutral);
    background: transparent;
  }
  .sdd-btn--mute:hover:not(:disabled) {
    color: var(--error);
    border-color: color-mix(in srgb, var(--error) 55%, transparent);
    background: color-mix(in srgb, var(--error) 8%, transparent);
  }

  /* Amend panel — quiet inline form. Same blockquote grammar as the
   * card itself: left accent rail + transparent bg + a single
   * textarea. Reads as "type a change" alongside the card content,
   * not as a modal popup. */
  .sdd-amend {
    margin-top: 4px;
    padding: 6px 0 4px 12px;
    border-left: 2px solid color-mix(in srgb, var(--accent) 35%, transparent);
  }
  .sdd-amend-label { display: flex; flex-direction: column; gap: 4px; }
  .sdd-amend-hint {
    font-size: 11px;
    color: var(--text-mute);
    line-height: 1.45;
  }
  .sdd-amend-area {
    width: 100%;
    padding: 6px 8px;
    border: 1px solid var(--border-neutral-hi);
    border-radius: 4px;
    background: color-mix(in srgb, var(--bg-0) 70%, transparent);
    color: var(--text-0);
    font-family: 'JetBrains Mono', monospace;
    font-size: 12.5px;
    line-height: 1.55;
    resize: vertical;
    outline: 0;
  }
  .sdd-amend-area:focus { border-color: var(--accent); }

  /* -------- Audit log (phase 6) -------- */
  .vh {
    position: absolute; width: 1px; height: 1px;
    overflow: hidden; clip: rect(0 0 0 0);
    white-space: nowrap;
  }
  .sdd-audit-chip {
    appearance: none;
    background: transparent;
    border: 0; padding: 0;
    margin-left: 6px;
    font-family: 'JetBrains Mono', monospace;
    font-size: 10px;
    color: var(--text-mute);
    cursor: pointer;
    opacity: 0.7;
  }
  .sdd-audit-chip:hover { opacity: 1; color: var(--text-0); }
  .sdd-audit-chip[aria-pressed="true"] { color: var(--accent); opacity: 1; }
  .sdd-audit-overlay {
    margin: 8px 0 0 14px;
    padding: 8px 10px;
    border: 1px solid var(--border-neutral-hi);
    background: color-mix(in srgb, var(--bg-1) 92%, transparent);
    border-radius: 4px;
    font-size: 12px;
  }
  .sdd-audit-head {
    display: flex; align-items: center; gap: 8px;
    margin-bottom: 6px;
  }
  .sdd-audit-title { font-weight: 600; }
  .sdd-audit-count { font-size: 10.5px; color: var(--text-mute); }
  .sdd-audit-spacer { flex: 1; }
  .sdd-audit-filter select {
    background: var(--bg-0);
    border: 1px solid var(--border-neutral-hi);
    color: var(--text-0);
    font-size: 11px;
    padding: 1px 4px;
    border-radius: 3px;
  }
  .sdd-audit-empty {
    margin: 4px 0; color: var(--text-mute); font-style: italic;
  }
  .sdd-audit-list {
    list-style: none; margin: 0; padding: 0;
    max-height: 320px; overflow-y: auto;
  }
  .sdd-audit-row + .sdd-audit-row {
    border-top: 1px solid color-mix(in srgb, var(--border-neutral-hi) 50%, transparent);
  }
  .sdd-audit-row-head {
    appearance: none;
    width: 100%;
    display: flex; flex-wrap: wrap; align-items: baseline; gap: 6px;
    background: transparent; border: 0;
    padding: 4px 0;
    text-align: left;
    color: var(--text-0);
    cursor: pointer;
    font: inherit;
  }
  .sdd-audit-row-head:hover {
    background: color-mix(in srgb, var(--accent) 6%, transparent);
  }
  .sdd-audit-ts { font-size: 10.5px; color: var(--text-mute); }
  .sdd-audit-source {
    font-size: 10px; padding: 0 4px; border-radius: 2px;
    background: color-mix(in srgb, var(--text-mute) 18%, transparent);
    color: var(--text-0);
  }
  .sdd-audit-source[data-source="agent"]  { background: color-mix(in srgb, #d6743a 28%, transparent); }
  .sdd-audit-source[data-source="user"]   { background: color-mix(in srgb, #6a8fb5 28%, transparent); }
  .sdd-audit-source[data-source="system"] { background: color-mix(in srgb, #888 22%, transparent); }
  .sdd-audit-action { font-weight: 600; }
  .sdd-audit-phase { font-size: 10.5px; color: var(--text-mute); }
  .sdd-audit-reason { color: var(--text-mute); font-style: italic; }
  .sdd-audit-snap {
    padding: 4px 0 8px 14px;
    font-size: 11px;
  }
  .sdd-audit-snap pre {
    margin: 4px 0 0;
    padding: 6px 8px;
    background: var(--bg-0);
    border: 1px solid var(--border-neutral-hi);
    border-radius: 3px;
    overflow-x: auto;
    font-size: 10.5px;
    line-height: 1.4;
  }
  .sdd-audit-snap summary {
    cursor: pointer;
    color: var(--text-mute);
    font-size: 10.5px;
  }
</style>
