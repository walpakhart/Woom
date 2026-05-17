<script lang="ts">
  /* QuestionCard — inline render of an `ask_user_question` MCP card.
     Rendered by ChatThread alongside the existing ClaudeActionCards.
     The card blocks the same way `propose_bash` etc. do: the sidecar
     holds the tool result open over IPC until the user clicks an
     option (or types into the always-present "Other" field). On
     submit we call `resolve_action_wait(waitId, true, summary)` and
     the agent gets the choice as the tool_result IN THE SAME TURN. */

  import { invoke } from '@tauri-apps/api/core';

  type QuestionOption = { label: string; description?: string };
  type Action = {
    id: string;
    kind: 'question';
    question: string;
    header?: string;
    options: QuestionOption[];
    multiSelect?: boolean;
    status: 'pending' | 'executing' | 'done' | 'error';
    chosen?: string[];
    other?: string;
    result?: string;
    waitId?: string;
  };

  interface Props {
    action: Action;
    onUpdate: (patch: Partial<Action>) => void;
    onDismiss: () => void;
  }
  let p: Props = $props();

  const isPending = $derived(p.action.status === 'pending');
  const isDone = $derived(p.action.status === 'done');
  const isExecuting = $derived(p.action.status === 'executing');

  /* Per-card local state — selected option set (single or multi) plus
     the "Other" free-form text. We don't write into action.chosen
     until submit so the agent only sees the final value. */
  let picked = $state<Set<string>>(new Set());
  let otherText = $state('');

  function isSelected(label: string): boolean {
    return picked.has(label);
  }

  function toggle(label: string): void {
    if (!isPending) return;
    if (p.action.multiSelect) {
      const next = new Set(picked);
      if (next.has(label)) next.delete(label);
      else next.add(label);
      picked = next;
    } else {
      /* Radio behaviour — single click also submits when single-select
       *  and no other text typed. Faster path for the common case. */
      picked = new Set([label]);
      void submit();
    }
  }

  async function submit(): Promise<void> {
    if (!isPending) return;
    if (!p.action.waitId) {
      /* No IPC handle — card was synthesized from the stream parser
       *  before the sidecar's IPC request landed. Mark error so the
       *  agent's turn doesn't deadlock. */
      p.onUpdate({ status: 'error', result: 'no waitId — IPC handshake missing' });
      return;
    }
    const labels = [...picked];
    const other = otherText.trim();
    if (labels.length === 0 && other.length === 0) return;
    p.onUpdate({
      status: 'executing',
      chosen: labels,
      other: other || undefined
    });
    /* Summary text fed back to the agent. Includes both clicked
     *  options AND the "Other" text when present, so the agent sees
     *  the user's full intent — not just the chosen labels. */
    const parts: string[] = [];
    if (labels.length > 0) parts.push(`Chose: ${labels.join(', ')}`);
    if (other) parts.push(`Other: ${other}`);
    const summary = parts.join(' · ');
    try {
      await invoke<boolean>('resolve_action_wait', {
        waitId: p.action.waitId,
        ok: true,
        summary
      });
      p.onUpdate({ status: 'done', result: summary });
    } catch (e) {
      p.onUpdate({ status: 'error', result: String(e) });
    }
  }

  async function dismiss(): Promise<void> {
    /* Dismiss = decline to answer. Resolve the IPC wait with ok=false
     *  so the agent sees "user dismissed without choosing" and can
     *  decide whether to ask differently or stop. */
    if (p.action.waitId && isPending) {
      try {
        await invoke<boolean>('resolve_action_wait', {
          waitId: p.action.waitId,
          ok: false,
          summary: 'User dismissed the question without choosing.'
        });
      } catch { /* best-effort */ }
    }
    p.onDismiss();
  }
</script>

<div class="qc" class:qc--done={isDone} class:qc--executing={isExecuting}>
  <header class="qc-head">
    <div class="qc-icon" aria-hidden="true">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">
        <circle cx="12" cy="12" r="10"/>
        <path d="M9.09 9a3 3 0 0 1 5.83 1c0 2-3 3-3 3"/>
        <path d="M12 17h.01"/>
      </svg>
    </div>
    <div class="qc-title-wrap">
      <div class="qc-title">{p.action.question}</div>
      {#if p.action.header}
        <div class="qc-sub">{p.action.header}</div>
      {/if}
    </div>
    {#if isDone}<span class="qc-tag qc-tag--ok mono">answered</span>{/if}
    {#if isExecuting}<span class="qc-tag mono">sending…</span>{/if}
  </header>

  {#if isPending}
    <div class="qc-opts" role={p.action.multiSelect ? 'group' : 'radiogroup'}>
      {#each p.action.options as opt, i (i)}
        {@const selected = isSelected(opt.label)}
        <button
          type="button"
          class="qc-opt"
          class:qc-opt--selected={selected}
          role={p.action.multiSelect ? 'checkbox' : 'radio'}
          aria-checked={selected}
          onclick={() => toggle(opt.label)}
          disabled={!isPending}
        >
          <span class="qc-opt-pip" aria-hidden="true">
            {#if p.action.multiSelect}
              {#if selected}<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round"><path d="M5 12l5 5 9-11"/></svg>{/if}
            {:else}
              {#if selected}<span class="qc-opt-dot"></span>{/if}
            {/if}
          </span>
          <div class="qc-opt-body">
            <div class="qc-opt-label">{opt.label}</div>
            {#if opt.description}
              <div class="qc-opt-desc">{opt.description}</div>
            {/if}
          </div>
        </button>
      {/each}
    </div>
    <div class="qc-other">
      <input
        class="qc-other-input"
        type="text"
        bind:value={otherText}
        placeholder="Other (free-form answer)…"
        onkeydown={(e) => { if (e.key === 'Enter') { void submit(); } }}
      />
    </div>
    <footer class="qc-foot">
      <button class="qc-btn qc-btn--ghost" onclick={() => void dismiss()}>Dismiss</button>
      <button
        class="qc-btn qc-btn--primary"
        onclick={() => void submit()}
        disabled={picked.size === 0 && otherText.trim().length === 0}
      >
        {p.action.multiSelect ? 'Submit' : 'Send'}
      </button>
    </footer>
  {:else if isDone}
    <div class="qc-resolved">
      {#if p.action.chosen && p.action.chosen.length > 0}
        <div class="qc-resolved-row">
          <span class="qc-resolved-tag mono">Chose</span>
          <span>{p.action.chosen.join(', ')}</span>
        </div>
      {/if}
      {#if p.action.other}
        <div class="qc-resolved-row">
          <span class="qc-resolved-tag mono">Other</span>
          <span>{p.action.other}</span>
        </div>
      {/if}
    </div>
  {/if}
</div>

<style>
  .qc {
    margin: 8px 0;
    border: 1px solid var(--border-hi);
    border-radius: 10px;
    background: var(--bg-1);
    overflow: hidden;
  }
  .qc--done {
    background: var(--bg-2);
    border-color: var(--border);
  }
  .qc-head {
    display: flex; align-items: flex-start;
    gap: 10px;
    padding: 10px 14px;
    border-bottom: 1px solid var(--border);
  }
  .qc--done .qc-head { border-bottom: 0; }
  .qc-icon {
    width: 22px; height: 22px;
    display: grid; place-items: center;
    color: var(--accent);
    flex-shrink: 0;
  }
  .qc-icon svg { width: 16px; height: 16px; }
  .qc-title-wrap { flex: 1; min-width: 0; }
  .qc-title {
    font-size: 13px;
    font-weight: 500;
    color: var(--text-0);
    line-height: 1.4;
  }
  .qc-sub {
    margin-top: 3px;
    font-size: 11.5px;
    color: var(--text-mute);
    line-height: 1.45;
  }
  .qc-tag {
    flex-shrink: 0;
    padding: 1px 7px;
    border-radius: 4px;
    background: var(--bg-2);
    color: var(--text-mute);
    font-size: 9.5px;
    font-weight: 700;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    align-self: center;
  }
  .qc-tag--ok {
    color: #6ec3a4;
    background: color-mix(in srgb, #6ec3a4 10%, transparent);
    border: 1px solid color-mix(in srgb, #6ec3a4 35%, transparent);
  }

  .qc-opts {
    display: flex; flex-direction: column;
    gap: 4px;
    padding: 10px 12px 0;
  }
  .qc-opt {
    display: flex; align-items: flex-start;
    gap: 10px;
    padding: 8px 10px;
    background: var(--bg-2);
    border: 1px solid var(--border);
    border-radius: 7px;
    color: var(--text-0);
    cursor: pointer;
    text-align: left;
    transition: background 100ms, border-color 100ms;
  }
  .qc-opt:hover:not(:disabled) {
    background: var(--bg-3);
    border-color: var(--border-hi);
  }
  .qc-opt:disabled { cursor: not-allowed; opacity: 0.5; }
  .qc-opt--selected {
    background: color-mix(in srgb, var(--accent) 12%, var(--bg-2));
    border-color: var(--accent);
  }
  .qc-opt-pip {
    width: 16px; height: 16px;
    border-radius: 50%;
    display: grid; place-items: center;
    border: 1px solid var(--border-hi);
    background: var(--bg-1);
    flex-shrink: 0;
    margin-top: 1px;
  }
  .qc-opt--selected .qc-opt-pip {
    border-color: var(--accent);
    background: color-mix(in srgb, var(--accent) 18%, var(--bg-1));
  }
  .qc-opt-pip svg { width: 9px; height: 9px; color: var(--accent); }
  .qc-opt-dot {
    width: 7px; height: 7px;
    border-radius: 50%;
    background: var(--accent);
  }
  .qc-opt-body { flex: 1; min-width: 0; }
  .qc-opt-label {
    font-size: 12.5px;
    font-weight: 500;
    color: var(--text-0);
  }
  .qc-opt-desc {
    margin-top: 2px;
    font-size: 11px;
    color: var(--text-mute);
    line-height: 1.45;
  }

  .qc-other {
    padding: 8px 12px 0;
  }
  .qc-other-input {
    width: 100%;
    padding: 6px 10px;
    border-radius: 6px;
    border: 1px solid var(--border);
    background: var(--bg-2);
    color: var(--text-0);
    font-size: 12px;
  }
  .qc-other-input:focus {
    outline: 2px solid var(--accent);
    outline-offset: -1px;
    border-color: transparent;
  }

  .qc-foot {
    display: flex; justify-content: flex-end;
    gap: 6px;
    padding: 10px 12px;
  }
  .qc-btn {
    padding: 4px 12px;
    border-radius: 5px;
    border: 1px solid var(--border);
    background: var(--bg-2);
    color: var(--text-1);
    font-size: 11.5px;
    font-weight: 500;
    cursor: pointer;
  }
  .qc-btn:hover { background: var(--bg-3); }
  .qc-btn:disabled { opacity: 0.5; cursor: not-allowed; }
  .qc-btn--primary {
    background: color-mix(in srgb, var(--accent) 30%, var(--bg-2));
    border-color: color-mix(in srgb, var(--accent) 60%, var(--border));
    color: var(--text-0);
  }
  .qc-btn--primary:hover:not(:disabled) {
    background: color-mix(in srgb, var(--accent) 40%, var(--bg-2));
  }

  .qc-resolved {
    padding: 10px 14px;
    display: flex; flex-direction: column;
    gap: 4px;
  }
  .qc-resolved-row {
    display: flex; align-items: baseline;
    gap: 8px;
    font-size: 12px;
    color: var(--text-1);
  }
  .qc-resolved-tag {
    font-size: 9.5px; font-weight: 700;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--text-mute);
    padding: 1px 6px;
    border-radius: 3px;
    background: var(--bg-1);
    border: 1px solid var(--border);
  }
</style>
