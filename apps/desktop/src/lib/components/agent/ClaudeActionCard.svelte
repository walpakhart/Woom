<script lang="ts">
  import { openUrl } from '@tauri-apps/plugin-opener';
  import { invoke } from '@tauri-apps/api/core';

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
    /** Called when the user clicks "Open in Woom" on a completed PR card.
        Parent parses the URL and wires up focusItem → solo view. */
    onOpenPrInWoom?: (url: string) => void;
    /** Effective cwd of the calling session — used to resolve the head
        branch for PR cards so the user can see what branch the PR will
        actually be opened FROM (a frequent footgun: the agent proposes
        a PR on the wrong branch and validation fails). null when the
        session has no cwd yet. */
    repoCwd?: string | null;
  }
  let { action, onUpdate, onDismiss, onExecute, onOpenPrInWoom, repoCwd = null }: Props = $props();

  const isBusy = $derived(action.status === 'executing');
  const isDone = $derived(action.status === 'done');
  const isError = $derived(action.status === 'error');
  const isEditable = $derived(action.status === 'pending' || action.status === 'error');

  // Live head-branch readout for PR cards. We re-query when cwd changes or
  // the card moves between executing/error states (a failed PR is often
  // followed by the user manually `git checkout`-ing into a different
  // branch and retrying — the live readout shows the new value).
  let headBranch = $state<string | null>(null);
  let headBranchError = $state<string | null>(null);
  $effect(() => {
    if (action.kind !== 'pr' || !repoCwd) {
      headBranch = null;
      headBranchError = null;
      return;
    }
    // Re-trigger when status flips so a fresh `git checkout` becomes visible.
    void action.status;
    void (async () => {
      try {
        const b = await invoke<string>('git_current_branch', { repo: repoCwd });
        headBranch = b.trim();
        headBranchError = null;
      } catch (e) {
        headBranch = null;
        headBranchError = typeof e === 'string' ? e : String(e);
      }
    })();
  });

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
      <div class="cac-label">
        <span>Head branch <span class="cac-opt">(current branch in {repoCwd ? 'this repo' : 'the working dir'})</span></span>
        <div class="cac-readonly mono" title={headBranchError ?? undefined}>
          {#if headBranchError}
            <span class="cac-readonly--err">⚠ {headBranchError}</span>
          {:else if headBranch}
            {headBranch}
          {:else}
            <span class="cac-readonly--mute">reading…</span>
          {/if}
        </div>
      </div>
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
      {#if action.kind === 'pr' && action.result?.startsWith('http') && onOpenPrInWoom}
        <button
          class="cac-btn cac-btn--primary"
          onclick={() => onOpenPrInWoom?.(action.result!)}
        >
          Open in Woom
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
  /* v6 action-card — premium tactile element, brand-stripe on the
     left, gradient tint per kind, soft glow shadow. The kind's tone
     is exposed via --cac-tone / --cac-glow so the head icon, stripe,
     and outer glow share one source of truth. */
  .cac {
    --cac-tone: var(--accent);
    --cac-glow: var(--accent-glow);
    position: relative;
    background:
      linear-gradient(180deg,
        color-mix(in srgb, var(--cac-tone) 7%, transparent),
        color-mix(in srgb, var(--cac-tone) 2%, transparent) 70%),
      var(--bg-2);
    border: 1px solid color-mix(in srgb, var(--cac-tone) 28%, transparent);
    border-radius: 12px;
    padding: 12px 14px 12px 16px;
    display: flex; flex-direction: column; gap: 10px;
    transition: border-color 200ms, background 200ms;
    box-shadow: 0 1px 3px rgba(0, 0, 0, 0.18);
    overflow: hidden;
  }
  .cac::before {
    content: '';
    position: absolute; left: 0; top: 0; bottom: 0;
    width: 3px;
    background: linear-gradient(180deg,
      var(--cac-tone),
      color-mix(in srgb, var(--cac-tone) 55%, var(--bg-0)));
    box-shadow: 0 0 12px var(--cac-glow);
    z-index: 1;
  }
  .cac--commit { --cac-tone: var(--src-sentry);  --cac-glow: rgba(232, 130, 100, 0.32); }
  .cac--pr     { --cac-tone: var(--accent);      --cac-glow: var(--accent-glow); }
  .cac--switch { --cac-tone: var(--src-jira);    --cac-glow: rgba(79, 142, 255, 0.30); }
  .cac--bash   { --cac-tone: var(--app-tone, var(--accent)); --cac-glow: var(--accent-glow); }
  .cac-input--cmd { background: var(--bg-0); }
  .cac--done   { opacity: 0.72; --cac-glow: rgba(0, 0, 0, 0); }
  .cac--error  { --cac-tone: var(--error); --cac-glow: rgba(232, 130, 100, 0.40); }

  .cac-head { display: flex; align-items: center; gap: 8px; }
  .cac-icon {
    width: 22px; height: 22px; border-radius: 5px;
    display: inline-flex; align-items: center; justify-content: center;
    background: var(--bg-2); color: var(--accent-bright);
  }
  .cac-title { font-size: 12.5px; color: var(--text-0); font-weight: 600; flex: 1; }
  .cac-tag { font-size: 10px; padding: 2px 7px; border-radius: 3px; margin-left: 6px; font-weight: 600; text-transform: uppercase; letter-spacing: 0.05em; }
  .cac-tag--ok { background: rgba(204, 120, 92, 0.18); color: var(--success); }
  .cac-tag--err { background: rgba(232, 130, 100, 0.18); color: var(--error); }
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
  .cac-readonly {
    padding: 7px 10px;
    background: var(--bg-1); border: 1px solid var(--border-neutral);
    border-radius: 6px; color: var(--text-1); font-size: 13px;
    cursor: default;
  }
  .cac-readonly--mute { color: var(--text-mute);  }
  .cac-readonly--err { color: var(--error, #F0A38A); }
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
  .cac-result--err { color: var(--error); background: rgba(232, 130, 100, 0.08); }
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
    background: var(--accent); color: var(--accent-fg);
  }
  .cac-btn--primary:hover:not(:disabled) { background: var(--accent-bright); }
</style>
