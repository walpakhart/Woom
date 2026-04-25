<script lang="ts">
  import { openUrl } from '@tauri-apps/plugin-opener';

  type Action =
    | {
        id: string;
        kind: 'commit';
        message: string;
        body: string;
        push: boolean;
        note: string;
        status: 'pending' | 'executing' | 'done' | 'error';
        result?: string;
      }
    | {
        id: string;
        kind: 'pr';
        title: string;
        body: string;
        base: string;
        draft: boolean;
        note: string;
        status: 'pending' | 'executing' | 'done' | 'error';
        result?: string;
      }
    | {
        id: string;
        kind: 'switch_cwd';
        path: string;
        reason: string;
        status: 'pending' | 'executing' | 'done' | 'error';
        result?: string;
      }
    | {
        id: string;
        kind: 'bash';
        command: string;
        reason: string;
        status: 'pending' | 'executing' | 'done' | 'error';
        result?: string;
        exitCode?: number;
      };

  interface Props {
    action: Action;
    onUpdate: (patch: Partial<Action>) => void;
    onDismiss: () => void;
    onExecute: () => void;
    /** Called when the user clicks "Open in Forgehold" on a completed PR card.
        Parent parses the URL and wires up focusItem → workbench view. */
    onOpenPrInForgehold?: (url: string) => void;
  }
  let { action, onUpdate, onDismiss, onExecute, onOpenPrInForgehold }: Props = $props();

  const isBusy = $derived(action.status === 'executing');
  const isDone = $derived(action.status === 'done');
  const isError = $derived(action.status === 'error');
  const isEditable = $derived(action.status === 'pending' || action.status === 'error');

  function openResultUrl() {
    if (action.kind === 'pr' && action.result?.startsWith('http')) {
      void openUrl(action.result);
    }
  }
</script>

<div class="cac"
  class:cac--commit={action.kind === 'commit'}
  class:cac--pr={action.kind === 'pr'}
  class:cac--switch={action.kind === 'switch_cwd'}
  class:cac--bash={action.kind === 'bash'}
  class:cac--done={isDone}
  class:cac--error={isError}>
  <div class="cac-head">
    <div class="cac-icon">
      {#if action.kind === 'commit'}
        <svg class="i i-sm" viewBox="0 0 24 24"><circle cx="12" cy="12" r="4"/><path d="M12 2v6M12 16v6"/></svg>
      {:else if action.kind === 'pr'}
        <svg class="i i-sm" viewBox="0 0 24 24"><circle cx="6" cy="6" r="2.5"/><circle cx="6" cy="18" r="2.5"/><circle cx="18" cy="18" r="2.5"/><path d="M6 8.5v7M8.5 6h7a3 3 0 0 1 3 3v6.5"/></svg>
      {:else if action.kind === 'switch_cwd'}
        <svg class="i i-sm" viewBox="0 0 24 24"><path d="M3 7v11a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2V9a2 2 0 0 0-2-2h-7L10 5H5a2 2 0 0 0-2 2z"/><path d="M9 13l2 2 4-4"/></svg>
      {:else}
        <svg class="i i-sm" viewBox="0 0 24 24"><polyline points="4 17 10 11 4 5"/><line x1="12" y1="19" x2="20" y2="19"/></svg>
      {/if}
    </div>
    <div class="cac-title">
      {#if action.kind === 'commit'}
        Claude proposed a commit
      {:else if action.kind === 'pr'}
        Claude proposed a pull request
      {:else if action.kind === 'switch_cwd'}
        Claude wants to switch working directory
      {:else}
        Claude wants to run a command
      {/if}
      {#if isDone}<span class="cac-tag cac-tag--ok">done</span>{/if}
      {#if isError}<span class="cac-tag cac-tag--err">failed</span>{/if}
    </div>
    <button class="cac-x" onclick={onDismiss} aria-label="Dismiss" title="Dismiss">
      <svg class="i i-sm" viewBox="0 0 24 24"><path d="M6 6l12 12M6 18L18 6"/></svg>
    </button>
  </div>

  {#if (action.kind === 'commit' || action.kind === 'pr') && action.note}
    <div class="cac-note">{action.note}</div>
  {/if}
  {#if (action.kind === 'switch_cwd' || action.kind === 'bash') && action.reason}
    <div class="cac-note">{action.reason}</div>
  {/if}

  {#if action.kind === 'commit'}
    <div class="cac-body">
      <label class="cac-label">
        <span>Subject</span>
        <input
          class="cac-input cac-input--subject"
          value={action.message}
          oninput={(e) => onUpdate({ message: e.currentTarget.value })}
          disabled={!isEditable}
          placeholder="commit subject (imperative, ≤72 chars)"
        />
      </label>
      <label class="cac-label">
        <span>Body <span class="cac-opt">(optional)</span></span>
        <textarea
          class="cac-input cac-textarea"
          rows="3"
          value={action.body}
          oninput={(e) => onUpdate({ body: e.currentTarget.value })}
          disabled={!isEditable}
          placeholder="extended description, if any"
        ></textarea>
      </label>
      <label class="cac-check">
        <input
          type="checkbox"
          checked={action.push}
          onchange={(e) => onUpdate({ push: e.currentTarget.checked })}
          disabled={!isEditable}
        />
        Push to origin after commit
      </label>
    </div>
  {:else if action.kind === 'switch_cwd'}
    <div class="cac-body">
      <label class="cac-label">
        <span>Path</span>
        <input
          class="cac-input mono"
          value={action.path}
          oninput={(e) => onUpdate({ path: e.currentTarget.value })}
          disabled={!isEditable}
        />
      </label>
    </div>
  {:else if action.kind === 'bash'}
    <div class="cac-body">
      <label class="cac-label">
        <span>Command <span class="cac-opt">(runs via `sh -c` in current cwd)</span></span>
        <textarea
          class="cac-input cac-textarea cac-input--cmd mono"
          rows="2"
          value={action.command}
          oninput={(e) => onUpdate({ command: e.currentTarget.value })}
          disabled={!isEditable}
        ></textarea>
      </label>
    </div>
  {:else}
    <div class="cac-body">
      <label class="cac-label">
        <span>Title</span>
        <input
          class="cac-input"
          value={action.title}
          oninput={(e) => onUpdate({ title: e.currentTarget.value })}
          disabled={!isEditable}
        />
      </label>
      <label class="cac-label">
        <span>Base branch <span class="cac-opt">(empty = repo default)</span></span>
        <input
          class="cac-input mono"
          value={action.base}
          oninput={(e) => onUpdate({ base: e.currentTarget.value })}
          disabled={!isEditable}
          placeholder="main"
        />
      </label>
      <label class="cac-label">
        <span>Body</span>
        <textarea
          class="cac-input cac-textarea"
          rows="4"
          value={action.body}
          oninput={(e) => onUpdate({ body: e.currentTarget.value })}
          disabled={!isEditable}
        ></textarea>
      </label>
      <label class="cac-check">
        <input
          type="checkbox"
          checked={action.draft}
          onchange={(e) => onUpdate({ draft: e.currentTarget.checked })}
          disabled={!isEditable}
        />
        Open as draft
      </label>
    </div>
  {/if}

  {#if action.result}
    <div class="cac-result" class:cac-result--err={isError}>
      {#if action.kind === 'pr' && action.result.startsWith('http')}
        <button class="cac-link" onclick={openResultUrl}>{action.result}</button>
      {:else}
        {action.result}
      {/if}
    </div>
  {/if}

  <div class="cac-actions">
    {#if isDone}
      {#if action.kind === 'pr' && action.result?.startsWith('http') && onOpenPrInForgehold}
        <button
          class="cac-btn cac-btn--primary"
          onclick={() => onOpenPrInForgehold?.(action.result!)}
        >
          Open in Forgehold
        </button>
      {/if}
      <button class="cac-btn cac-btn--ghost" onclick={onDismiss}>Close</button>
    {:else}
      <button class="cac-btn cac-btn--ghost" onclick={onDismiss} disabled={isBusy}>Dismiss</button>
      <button class="cac-btn cac-btn--primary" onclick={onExecute} disabled={isBusy || !isEditable}>
        {#if isBusy}
          Working…
        {:else if action.kind === 'commit'}
          {action.push ? 'Commit & Push' : 'Commit'}
        {:else if action.kind === 'switch_cwd'}
          Switch to this path
        {:else if action.kind === 'bash'}
          Run
        {:else}
          Create PR
        {/if}
      </button>
    {/if}
  </div>
</div>

<style>
  .cac {
    background: var(--bg-1);
    border: 1px solid var(--border-hi);
    border-radius: 10px;
    padding: 12px 14px;
    display: flex; flex-direction: column; gap: 10px;
    transition: border-color 200ms, background 200ms;
  }
  .cac--commit { border-color: rgba(238, 107, 31, 0.35); background: linear-gradient(180deg, rgba(238, 107, 31, 0.04), transparent 70%); }
  .cac--pr { border-color: rgba(217, 145, 60, 0.35); background: linear-gradient(180deg, rgba(217, 145, 60, 0.05), transparent 70%); }
  .cac--switch { border-color: rgba(96, 165, 250, 0.3); background: linear-gradient(180deg, rgba(96, 165, 250, 0.04), transparent 70%); }
  .cac--bash { border-color: rgba(176, 153, 246, 0.35); background: linear-gradient(180deg, rgba(176, 153, 246, 0.05), transparent 70%); }
  .cac-input--cmd { background: var(--bg-0); }
  .cac--done { border-color: rgba(217, 145, 60, 0.5); opacity: 0.7; }
  .cac--error { border-color: rgba(214, 72, 44, 0.5); }

  .cac-head { display: flex; align-items: center; gap: 8px; }
  .cac-icon {
    width: 22px; height: 22px; border-radius: 5px;
    display: inline-flex; align-items: center; justify-content: center;
    background: var(--bg-2); color: var(--accent-bright);
  }
  .cac-title { font-size: 12.5px; color: var(--text-0); font-weight: 600; flex: 1; }
  .cac-tag { font-size: 10px; padding: 2px 7px; border-radius: 3px; margin-left: 6px; font-weight: 600; text-transform: uppercase; letter-spacing: 0.05em; }
  .cac-tag--ok { background: rgba(217, 145, 60, 0.18); color: var(--success); }
  .cac-tag--err { background: rgba(214, 72, 44, 0.18); color: var(--error); }
  .cac-x {
    width: 22px; height: 22px; border-radius: 4px;
    display: inline-flex; align-items: center; justify-content: center;
    color: var(--text-2);
  }
  .cac-x:hover { background: var(--bg-3); color: var(--text-0); }

  .cac-note {
    font-size: 12px; color: var(--text-1);
    padding: 6px 10px;
    background: var(--bg-2);
    border-left: 2px solid var(--accent);
    border-radius: 3px;
    line-height: 1.5;
  }

  .cac-body { display: flex; flex-direction: column; gap: 10px; }
  .cac-label { display: flex; flex-direction: column; gap: 4px; font-size: 11px; color: var(--text-2); text-transform: uppercase; letter-spacing: 0.04em; }
  .cac-label > span { font-weight: 500; }
  .cac-opt { color: var(--text-mute); text-transform: none; letter-spacing: 0; font-weight: 400; }
  .cac-input {
    padding: 7px 10px;
    background: var(--bg-0); border: 1px solid var(--border-neutral-hi);
    border-radius: 6px; color: var(--text-0);
    font-size: 13px; font-family: inherit;
  }
  .cac-input:focus { outline: none; border-color: var(--border-hi2); }
  .cac-input--subject { font-family: 'JetBrains Mono', ui-monospace, 'SF Mono', monospace; font-size: 12.5px; }
  .cac-textarea { resize: vertical; line-height: 1.5; min-height: 54px; }
  .cac-check { display: flex; align-items: center; gap: 7px; font-size: 12px; color: var(--text-1); cursor: pointer; }
  .cac-check input { cursor: pointer; }
  .cac-check input:disabled { cursor: default; }

  .cac-result {
    padding: 8px 10px;
    background: var(--bg-2);
    border-radius: 5px;
    font-size: 11.5px; color: var(--text-1);
    font-family: 'JetBrains Mono', ui-monospace, 'SF Mono', monospace;
    line-height: 1.5;
    white-space: pre-wrap;
    word-break: break-word;
  }
  .cac-result--err { color: var(--error); background: rgba(214, 72, 44, 0.08); }
  .cac-link {
    color: var(--accent-bright); font: inherit; text-align: left;
    padding: 0; background: transparent; word-break: break-all;
  }
  .cac-link:hover { text-decoration: underline; }

  .cac-actions { display: flex; gap: 8px; justify-content: flex-end; margin-top: 2px; }
  .cac-btn {
    padding: 6px 14px; border-radius: 6px;
    font-size: 12px; font-weight: 600;
    transition: background 100ms;
  }
  .cac-btn:disabled { opacity: 0.4; cursor: default; }
  .cac-btn--ghost {
    background: var(--bg-2); color: var(--text-1);
    border: 1px solid var(--border-neutral-hi);
  }
  .cac-btn--ghost:hover:not(:disabled) { background: var(--bg-3); color: var(--text-0); }
  .cac-btn--primary {
    background: var(--accent); color: #1a0a04;
  }
  .cac-btn--primary:hover:not(:disabled) { background: var(--accent-bright); }
</style>
