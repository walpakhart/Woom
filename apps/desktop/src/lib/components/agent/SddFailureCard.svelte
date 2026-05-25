<script lang="ts">
  /* SddFailureCard — structured failure body for SddCard. Extracted in
     wave-9 split. Renders the header (phase/trigger), reason line,
     failed-check chip line, collapsible action-log tail, and the two
     inline reason-textareas (skip + accept).

     Parent (SddCard) owns the surrounding workspace state and the
     submit/cancel handlers; this component is a pure renderer that
     binds skip/accept text + invokes callbacks. Keeps SddCard
     focused on top-level orchestration. */
  import type { ActionLogEntry, SddWorkspace } from '$lib/state/sdd.svelte';

  /** SddStage.Failed payload shape — extracted from the discriminated
   *  union so the parent can pass it directly without re-narrowing. */
  type FailedStage = Extract<SddWorkspace['stage'], { kind: 'failed' }>;

  interface Props {
    stage: FailedStage;
    /** Whether the inline "Skip phase" textarea is open. Toggled by
     *  the parent's `startSkip` / `cancelSkip` actions. */
    skipMode: boolean;
    skipDraft: string;
    onSubmitSkip: () => void | Promise<void>;
    onCancelSkip: () => void;
    /** Whether the inline "Accept anyway" textarea is open. Toggled
     *  by the parent's `startAccept` / `cancelAccept` actions. */
    acceptMode: boolean;
    acceptDraft: string;
    onSubmitAccept: () => void | Promise<void>;
    onCancelAccept: () => void;
  }
  let {
    stage,
    skipMode,
    skipDraft = $bindable(),
    onSubmitSkip,
    onCancelSkip,
    acceptMode,
    acceptDraft = $bindable(),
    onSubmitAccept,
    onCancelAccept,
  }: Props = $props();
</script>

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
      Failed checks: {(stage.failed_checks ?? []).map((i: number) => `#${i + 1}`).join(', ')}
    </div>
  {/if}

  {#if (stage.action_log_tail?.length ?? 0) > 0}
    <details class="sdd-failed-tail">
      <summary class="mono">
        Last actions · {stage.action_log_tail?.length ?? 0}
      </summary>
      <ul class="sdd-failed-tail-list">
        {#each (stage.action_log_tail ?? []).slice(-5) as e, idx (`${e.correlation_id ?? ''}|${e.kind}|${idx}`)}
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
          if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') { e.preventDefault(); void onSubmitSkip(); }
          if (e.key === 'Escape') { e.preventDefault(); onCancelSkip(); }
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
          if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') { e.preventDefault(); void onSubmitAccept(); }
          if (e.key === 'Escape') { e.preventDefault(); onCancelAccept(); }
        }}
      ></textarea>
    </div>
  {/if}
</div>

<style>
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
  .sdd-failed-tail summary {
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
</style>
