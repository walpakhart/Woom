<script lang="ts">
  /* WorktreeSide — right pane of AgentApp.
     v7: branch-card with stats (added/removed/files), action-stack
     vertical buttons (Open diff / Open in Editor / Copy branch /
     Apply to main / Discard), label "Linked apps" + linked-list rows. */

  import { sessionsState } from '$lib/state/sessions.svelte';

  type Kind = 'claude' | 'cursor';

  interface Props {
    kind: Kind;
    onOpenWorktreeDiff: () => void;
    onCopyWorktreeBranch: () => void;
    onOpenWorktreeInEditor: () => void;
    onCreateWorktree: () => void;
    onRemoveWorktree: () => void;
    worktreeBusy: 'creating' | 'removing' | null;
  }

  let p: Props = $props();

  const activeSess = $derived(
    sessionsState.list.find((s) => s.id === sessionsState.activeIds[p.kind]) ?? null
  );

  const label = $derived(p.kind === 'claude' ? 'Claude' : 'Cursor');

  /** Diff stat counters from session edit events (each assistant message
   *  carries an `events` array; `edit` events have oldText/newText). */
  const diffStats = $derived.by(() => {
    if (!activeSess) return { added: 0, removed: 0, files: 0 };
    const files = new Set<string>();
    let added = 0;
    let removed = 0;
    for (const m of activeSess.messages ?? []) {
      for (const ev of m.events ?? []) {
        if (ev.kind !== 'edit') continue;
        files.add(ev.filePath);
        const oldLines = ev.oldText ? ev.oldText.split('\n').length : 0;
        const newLines = ev.newText ? ev.newText.split('\n').length : 0;
        const diff = newLines - oldLines;
        if (diff > 0) added += diff;
        else if (diff < 0) removed += -diff;
        else if (ev.oldText !== ev.newText) {
          added += 1;
          removed += 1;
        }
      }
    }
    return { added, removed, files: files.size };
  });

  function shortPath(path: string | null | undefined): string {
    if (!path) return '';
    const home = '/Users/';
    if (path.startsWith(home)) {
      const rest = path.slice(home.length);
      const slash = rest.indexOf('/');
      return slash >= 0 ? `~${rest.slice(slash)}` : '~';
    }
    return path;
  }
</script>

<aside class="wts app-pane">
  <header class="app-pane-head">
    <span class="app-pane-head-h">Worktree</span>
  </header>

  <div class="wts-body">
    {#if activeSess?.worktreePath && activeSess.worktreeRepo}
      <div class="branch-card">
        <div class="branch-row">
          <span class="branch-label">branch</span>
          <span class="branch-name mono">{activeSess.worktreeBranch}</span>
          <button class="wts-icon" title="Copy branch" onclick={p.onCopyWorktreeBranch}>
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7"><rect x="9" y="9" width="13" height="13" rx="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></svg>
          </button>
        </div>
        <div class="branch-blurb">Isolated from main. Commits stay here until applied.</div>
        <div class="branch-stats">
          <div class="stat-block">
            <div class="stat-num add mono">+{diffStats.added}</div>
            <div class="stat-lbl">Added</div>
          </div>
          <div class="stat-block">
            <div class="stat-num rem mono">−{diffStats.removed}</div>
            <div class="stat-lbl">Removed</div>
          </div>
          <div class="stat-block">
            <div class="stat-num files mono">{diffStats.files}</div>
            <div class="stat-lbl">Files</div>
          </div>
        </div>
      </div>

      <div class="action-stack">
        <button class="wts-btn" onclick={p.onOpenWorktreeDiff}>
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><polyline points="14 2 14 8 20 8"/><line x1="9" y1="13" x2="15" y2="13"/><line x1="9" y1="17" x2="15" y2="17"/></svg>
          Open diff
        </button>
        <button class="wts-btn" onclick={p.onOpenWorktreeInEditor}>
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"><rect x="3" y="3" width="18" height="18" rx="2"/><line x1="3" y1="9" x2="21" y2="9"/></svg>
          Open in Editor
        </button>
        <button class="wts-btn" onclick={p.onCopyWorktreeBranch}>
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"><rect x="9" y="9" width="13" height="13" rx="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></svg>
          Copy branch
        </button>
        <button
          class="wts-btn wts-btn--discard"
          onclick={p.onRemoveWorktree}
          disabled={p.worktreeBusy === 'removing'}
        >
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"><polyline points="3 6 5 6 21 6"/><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/></svg>
          {p.worktreeBusy === 'removing' ? 'Removing…' : 'Discard worktree'}
        </button>
      </div>

      <div class="app-label wts-section">Linked apps</div>
      <div class="linked-list">
        <div class="linked-row" title="Editor cwd shared">
          <span class="linked-dot linked-dot--editor"></span>
          <span class="linked-name">Editor</span>
          <span class="linked-meta mono">cwd shared</span>
        </div>
        <div class="linked-row dim" title="Terminal — not linked">
          <span class="linked-dot linked-dot--off"></span>
          <span class="linked-name">Terminal</span>
          <span class="linked-meta mono">+ Link</span>
        </div>
      </div>

      <div class="wts-meta mono">{shortPath(activeSess.worktreePath)}</div>
    {:else if activeSess}
      <div class="wts-empty">
        <div class="wts-empty-icon">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6">
            <circle cx="6" cy="6" r="2.5" />
            <circle cx="18" cy="18" r="2.5" />
            <path d="M6 8.5V14a4 4 0 0 0 4 4h6" />
          </svg>
        </div>
        <p class="wts-empty-h serif">No worktree</p>
        <p class="wts-empty-p">
          {label} writes to a feature-branch worktree so the main checkout
          stays untouched. Use the cwd bar in the chat above to start one.
        </p>
        <button
          class="wts-btn wts-btn--primary"
          onclick={p.onCreateWorktree}
          disabled={p.worktreeBusy === 'creating' || !activeSess.cwd}
        >
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M12 5v14M5 12h14"/></svg>
          {p.worktreeBusy === 'creating' ? 'Creating…' : 'Create worktree'}
        </button>
      </div>
    {:else}
      <div class="wts-empty">
        <div class="wts-empty-icon">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6">
            <path d="M12 2 L14.5 9.5 L22 12 L14.5 14.5 L12 22 L9.5 14.5 L2 12 L9.5 9.5 Z" />
          </svg>
        </div>
        <p class="wts-empty-h serif">Pick a session</p>
        <p class="wts-empty-p">Worktree, diff, and quick actions appear here once a session is active.</p>
      </div>
    {/if}
  </div>
</aside>

<style>
  .wts { display: flex; flex-direction: column; }
  .wts-body {
    flex: 1; overflow-y: auto;
    padding: 18px;
    display: flex; flex-direction: column; gap: 0;
    min-height: 0;
  }

  .branch-card {
    padding: 14px;
    background: var(--bg-2);
    border: 1px solid var(--border);
    border-radius: 11px;
    margin-bottom: 16px;
  }
  .branch-row {
    display: flex; align-items: center; gap: 8px;
    margin-bottom: 4px;
  }
  .branch-label {
    font-size: 9.5px; font-weight: 700;
    letter-spacing: 0.10em; text-transform: uppercase;
    color: var(--text-mute);
  }
  .branch-name {
    font-size: 13px;
    color: var(--accent-bright);
    flex: 1;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .branch-blurb {
    font-size: 11.5px; color: var(--text-mute);
    margin-bottom: 12px;
  }
  .branch-stats {
    display: flex; gap: 18px;
    padding: 12px 0 4px;
    border-top: 1px solid var(--border);
  }
  .stat-block {
    flex: 1;
    display: flex; flex-direction: column; align-items: center;
  }
  .stat-num {
    font-size: 22px; font-weight: 600;
    line-height: 1;
  }
  .stat-num.add { color: var(--diff-add); }
  .stat-num.rem { color: var(--diff-rem); }
  .stat-num.files { color: var(--text-0); }
  .stat-lbl {
    font-size: 9.5px;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    color: var(--text-mute);
    margin-top: 4px;
  }

  .wts-icon {
    width: 24px; height: 24px;
    display: grid; place-items: center;
    border-radius: 6px;
    background: transparent;
    border: 0;
    color: var(--text-2);
    cursor: pointer;
  }
  .wts-icon:hover { background: var(--bg-3); color: var(--text-0); }
  .wts-icon svg { width: 13px; height: 13px; }

  .action-stack { display: flex; flex-direction: column; gap: 6px; }
  .wts-btn {
    display: inline-flex; align-items: center; gap: 9px;
    justify-content: flex-start;
    padding: 10px 12px;
    font-size: 12.5px; font-weight: 500;
    background: var(--bg-2);
    border: 1px solid var(--border-neutral-hi);
    border-radius: 8px;
    color: var(--text-0);
    cursor: pointer;
    transition: border-color 140ms, background 140ms;
    width: 100%;
  }
  .wts-btn:hover { border-color: var(--border-hi); background: var(--bg-3); }
  .wts-btn svg { width: 14px; height: 14px; color: var(--text-2); }
  .wts-btn:hover svg { color: var(--accent-bright); }
  .wts-btn:disabled { opacity: 0.5; cursor: not-allowed; }

  .wts-btn--discard {
    background: transparent;
    border: 1px dashed var(--border-neutral-hi);
    color: var(--text-mute);
    margin-top: 4px;
  }
  .wts-btn--discard:hover { color: var(--error); border-color: var(--error); }
  .wts-btn--discard:hover svg { color: var(--error); }

  .wts-btn--primary {
    background: linear-gradient(180deg, var(--accent-bright), var(--accent));
    color: var(--accent-fg);
    border: none;
    justify-content: center;
    margin-top: 16px;
    box-shadow: 0 2px 8px var(--accent-glow), inset 0 1px 0 rgba(255,255,255,0.18);
  }
  .wts-btn--primary svg { color: var(--accent-fg); }
  .wts-btn--primary:hover svg { color: var(--accent-fg); }

  .wts-section { margin: 24px 0 8px; padding: 0 4px; }

  .linked-list {
    display: flex; flex-direction: column; gap: 6px;
  }
  .linked-row {
    display: flex; align-items: center; gap: 10px;
    padding: 9px 10px;
    background: var(--bg-2);
    border: 1px solid var(--border);
    border-radius: 8px;
    cursor: pointer;
  }
  .linked-row:hover { border-color: var(--border-hi); }
  .linked-row.dim { opacity: 0.6; }
  .linked-dot {
    width: 8px; height: 8px; border-radius: 50%;
    background: var(--src-editor);
    box-shadow: 0 0 8px var(--src-editor);
  }
  .linked-dot--editor { background: var(--src-editor); box-shadow: 0 0 8px var(--src-editor); }
  .linked-dot--off { background: var(--text-mute); box-shadow: none; }
  .linked-name { flex: 1; font-size: 12.5px; }
  .linked-meta { font-size: 10.5px; color: var(--text-mute); }

  .wts-meta {
    margin-top: 14px;
    font-size: 10px; color: var(--text-mute);
    word-break: break-all;
    padding: 0 4px;
  }

  .wts-empty {
    text-align: center;
    padding: 40px 20px;
    margin: auto;
  }
  .wts-empty-icon {
    width: 56px; height: 56px;
    margin: 0 auto 20px;
    display: grid; place-items: center;
    border-radius: 14px;
    background: color-mix(in srgb, var(--app-tone, var(--accent)) 10%, var(--bg-2));
    color: var(--app-tone, var(--accent));
    box-shadow:
      inset 0 0 0 1px color-mix(in srgb, var(--app-tone, var(--accent)) 24%, transparent),
      0 0 24px var(--app-glow);
  }
  .wts-empty-icon svg { width: 26px; height: 26px; }
  .wts-empty-h {
    font-family: 'Instrument Serif', 'New York', Georgia, serif;
    font-size: 22px; font-weight: 400; letter-spacing: -0.015em;
    color: var(--text-0);
    margin: 0 0 10px;
  }
  .wts-empty-p {
    font-size: 12.5px; color: var(--text-2);
    line-height: 1.55; margin: 0;
  }
</style>
