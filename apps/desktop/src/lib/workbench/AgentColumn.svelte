<script lang="ts">
  import { slide } from 'svelte/transition';
  import { cubicOut } from 'svelte/easing';
  import Markdown from '$lib/Markdown.svelte';
  import ClaudeActionCard from '$lib/ClaudeActionCard.svelte';
  import Dropdown, { type DropdownOption } from '$lib/Dropdown.svelte';
  import { convertFileSrc } from '@tauri-apps/api/core';
  import { open as openDialog } from '@tauri-apps/plugin-dialog';
  import {
    relativeTime,
    type ClaudeStatus,
    type ConnectionStatus,
    type CursorStatus
  } from '$lib/data';
  import { isImagePath, shortPath, shortenFsPath, shortRemote } from '$lib/format';
  import {
    sessionsState,
    updateSession,
    attachPathsToSession,
    sessionsForInstance,
    activeSessionInInstance,
    setActiveSessionInColumn
  } from '$lib/state/sessions.svelte';
  import {
    layoutState,
    movePanelById,
    closePanelById,
    startResizeById,
    activeInstances
  } from '$lib/state/layout.svelte';
  import type { ClaudeAction, RepoInfo } from '$lib/types';

  type Kind = 'claude' | 'cursor';

  interface Props {
    kind: Kind;
    instanceId: string;
    claudeStatus: ClaudeStatus | null;
    cursorStatus: CursorStatus | null;
    githubStatus: ConnectionStatus;
    editorRepoPath: string;
    activeRepoInfo: RepoInfo | null;
    dragOverInstanceId: string | null;
    worktreeBusy: 'creating' | 'removing' | null;
    worktreeMenuOpen: boolean;
    editingMsg: { sessionId: string; index: number; draft: string } | null;
    thinkingStartedAt: number | null;
    thinkingTick: number;
    now: number;
    // Callbacks
    onAgentDragOver: (instanceId: string, kind: Kind, e: DragEvent) => void;
    onAgentDragLeave: (instanceId: string) => void;
    onAgentDrop: (instanceId: string, kind: Kind, e: DragEvent) => void;
    onPickCwd: () => void;
    onClearCwd: () => void;
    onOpenSessionFolderInEditor: () => void;
    onToggleEditorLink: () => void;
    onLinkToEditorInstance: (editorInstanceId: string) => void;
    onCreateWorktree: () => void;
    onToggleWorktreeMenu: () => void;
    onOpenWorktreeDiff: () => void;
    onOpenWorktreeInEditor: () => void;
    onCopyWorktreeBranch: () => void;
    onApplyWorktree: () => void;
    onRemoveWorktree: () => void;
    onUpdateSessionCursorModel: (sessionId: string, model: string | null) => void;
    onDeleteClaudeSession: (id: string) => void;
    onNewClaudeSession: (opts: { agentKind: Kind; columnInstanceId: string }) => void;
    onStartEditMessage: (sessionId: string, index: number, content: string) => void;
    onResendMessage: (sessionId: string, index: number, content: string) => void;
    onCancelEditMessage: () => void;
    onCommitEditMessage: () => void;
    onSetEditingMsgDraft: (draft: string) => void;
    onUpdateAction: (sessionId: string, actionId: string, patch: Partial<ClaudeAction>) => void;
    onRemoveAction: (sessionId: string, actionId: string) => void;
    onExecuteAction: (sessionId: string, action: ClaudeAction) => void;
    onOpenPrInForgehold: (url: string, action: (ClaudeAction & { kind: 'pr' }) | null) => void;
    onSetSessionInput: (sessionId: string, input: string) => void;
    onSendClaudeMessage: () => void;
    onStopClaude: () => void;
  }

  let {
    kind,
    instanceId,
    claudeStatus,
    cursorStatus,
    githubStatus,
    editorRepoPath,
    activeRepoInfo,
    dragOverInstanceId,
    worktreeBusy,
    worktreeMenuOpen,
    editingMsg,
    thinkingStartedAt,
    thinkingTick,
    now,
    onAgentDragOver,
    onAgentDragLeave,
    onAgentDrop,
    onPickCwd,
    onClearCwd,
    onOpenSessionFolderInEditor,
    onToggleEditorLink,
    onLinkToEditorInstance,
    onCreateWorktree,
    onToggleWorktreeMenu,
    onOpenWorktreeDiff,
    onOpenWorktreeInEditor,
    onCopyWorktreeBranch,
    onApplyWorktree,
    onRemoveWorktree,
    onUpdateSessionCursorModel,
    onDeleteClaudeSession,
    onNewClaudeSession,
    onStartEditMessage,
    onResendMessage,
    onCancelEditMessage,
    onCommitEditMessage,
    onSetEditingMsgDraft,
    onUpdateAction,
    onRemoveAction,
    onExecuteAction,
    onOpenPrInForgehold,
    onSetSessionInput,
    onSendClaudeMessage,
    onStopClaude
  }: Props = $props();

  const brandLabel = $derived(kind === 'claude' ? 'Claude Code' : 'Cursor');
  const brandInitial = $derived(kind === 'claude' ? 'C' : 'Cr');
  const brandVersion = $derived(kind === 'claude' ? claudeStatus?.version : cursorStatus?.version);
  // First instance of its kind in the workbench — adopts orphaned/floating
  // sessions so pre-v2 persisted sessions surface somewhere.
  const isFirstOfKind = $derived(
    activeInstances().find((i) => i.kind === kind)?.id === instanceId
  );
  const kindSessions = $derived(sessionsForInstance(instanceId, kind, isFirstOfKind));
  const activeSess = $derived(activeSessionInInstance(instanceId, kind, isFirstOfKind));
  const dragOver = $derived(dragOverInstanceId === instanceId);
  const inst = $derived(activeInstances().find((i) => i.id === instanceId));
  const order = $derived(activeInstances().findIndex((i) => i.id === instanceId));

  /** Editor instance this session is linked to (for the linked pill label).
      Null when the link target was closed or never set. */
  const linkedEditor = $derived.by(() => {
    const boundId = activeSess?.linkedToEditorInstanceId;
    if (!boundId) return null;
    return activeInstances().find((i) => i.id === boundId && i.kind === 'editor') ?? null;
  });
  /** All Editor instances in the current workbench — used by the link dropdown
      when user wants to pick a specific one. */
  const editorInstances = $derived(activeInstances().filter((i) => i.kind === 'editor'));

  function focusLocalSession(id: string) {
    setActiveSessionInColumn(instanceId, id);
  }

  // Cursor model options — empty string means "auto" (forward `--model` unset).
  const cursorModelOptions: DropdownOption<string>[] = [
    { value: '', label: 'auto' },
    { value: 'composer-2', label: 'Composer 2' },
    { value: 'composer-2-fast', label: 'Composer 2 Fast' },
    { value: 'sonnet-4-thinking', label: 'Sonnet 4 Thinking' },
    { value: 'claude-opus-4-7-thinking-high', label: 'Opus 4.7 Thinking High' },
    { value: 'gpt-5.3-codex-high', label: 'Codex 5.3 High' },
    { value: 'gpt-5.4-high', label: 'GPT-5.4 High' }
  ];

  async function pickFiles() {
    if (!activeSess) onNewClaudeSession({ agentKind: kind, columnInstanceId: instanceId });
    const picked = await openDialog({
      multiple: true,
      title: 'Attach files or images',
      filters: [
        { name: 'Images', extensions: ['png', 'jpg', 'jpeg', 'webp', 'gif', 'bmp', 'svg', 'heic', 'heif', 'avif'] },
        { name: 'All files', extensions: ['*'] }
      ]
    });
    if (!picked) return;
    const paths = Array.isArray(picked) ? picked : [picked];
    if (!activeSess) return;
    const n = attachPathsToSession(activeSess.id, paths as string[]);
    if (n > 0) focusLocalSession(activeSess.id);
  }

  function removeMention(externalId: string) {
    if (!activeSess) return;
    const mentions = activeSess.mentions.filter((m) => m.externalId !== externalId);
    // Best-effort: also strip the `@<externalId>` token from input so the two
    // stay in sync. Users can put it back manually if they deleted by accident.
    const token = new RegExp(
      `(^|\\s)@${externalId.replace(/[.*+?^${}()|[\\]\\\\]/g, '\\\\$&')}(?=\\s|$)`
    );
    const input = activeSess.input.replace(token, '$1').replace(/\s{2,}/g, ' ');
    updateSession(activeSess.id, { mentions, input });
    focusLocalSession(activeSess.id);
  }
</script>

<section
  class="wb-column claude-col"
  class:wb-column--cursor={kind === 'cursor'}
  class:drag-over={dragOver}
  ondragover={(e) => onAgentDragOver(instanceId, kind, e)}
  ondragleave={() => onAgentDragLeave(instanceId)}
  ondrop={(e) => onAgentDrop(instanceId, kind, e)}
  role="region"
  aria-label={brandLabel}
  data-instance-id={instanceId}
  data-kind={kind}
  style="order: {order}; flex: 0 0 {inst?.width ?? 520}px"
  transition:slide={{ duration: 240, axis: 'x', easing: cubicOut }}
>
  <div class="wb-col-controls">
    <button class="wb-col-ctl" onclick={() => movePanelById(instanceId, -1)} aria-label="Move left" title="Move left"><svg class="i i-sm" viewBox="0 0 24 24"><path d="M15 6l-6 6 6 6" /></svg></button>
    <button class="wb-col-ctl" onclick={() => movePanelById(instanceId, 1)} aria-label="Move right" title="Move right"><svg class="i i-sm" viewBox="0 0 24 24"><path d="M9 6l6 6-6 6" /></svg></button>
    <button class="wb-col-ctl wb-col-ctl--close" onclick={() => closePanelById(instanceId)} aria-label="Hide column" title="Hide"><svg class="i i-sm" viewBox="0 0 24 24"><path d="M6 6l12 12M6 18L18 6" /></svg></button>
  </div>
  <div class="wb-col-resize" class:snap-flash={layoutState.snapFlashInstanceId === instanceId} role="separator" aria-orientation="vertical" onpointerdown={(e) => startResizeById(instanceId, e)}></div>
  <div class="inbox-brand">
    {#if kind === 'claude'}
      <span class="source-mark" style="color: var(--accent-bright); background: rgba(16, 185, 129, 0.12); border-color: rgba(16, 185, 129, 0.3);">{brandInitial}</span>
    {:else}
      <span class="source-mark" style="color: #c7a8ff; background: rgba(176, 153, 246, 0.14); border-color: rgba(176, 153, 246, 0.35);">{brandInitial}</span>
    {/if}
    <span class="brand-word">{brandLabel}</span>
    {#if inst?.name}<span class="bench-name mono" title="Bench id — use this to link from elsewhere">{inst.name}</span>{/if}
    {#if brandVersion}<span class="brand-sub mono">{brandVersion.split(' ')[0]}</span>{/if}
  </div>

  {#if activeSess}
    <div class="cwd-bar" class:cwd-bar--linked={activeSess.linkedToEditor}>
      {#if activeSess.linkedToEditor}
        <!-- Tight linked strip. Folder + branch are visible in the Editor
             column right next to this one — no need to repeat them here.
             Isolate stays available so you can still spin up a worktree
             even when linked. Click the 🔗 pill to jump to the Editor. -->
        <button
          class="linked-pill"
          onclick={() => { focusLocalSession(activeSess.id); onOpenSessionFolderInEditor(); }}
          title={editorRepoPath ? `Reveal in Editor: ${editorRepoPath}` : 'Editor has no folder open'}
        >
          <span class="linked-pill-dot"></span>
          <svg class="i i-sm" viewBox="0 0 24 24"><path d="M9 17H7A5 5 0 1 1 7 7h2M15 7h2a5 5 0 1 1 0 10h-2M8 12h8"/></svg>
          <span class="linked-pill-label">Linked to Editor</span>
          {#if linkedEditor}
            <span class="linked-pill-bench mono">{linkedEditor.name}</span>
          {/if}
        </button>
        <div style="flex:1"></div>
        {#if !activeSess.worktreePath}
          <button
            class="wt-chip wt-chip--create"
            onclick={() => { focusLocalSession(activeSess.id); onCreateWorktree(); }}
            disabled={worktreeBusy === 'creating' || !editorRepoPath}
            title={editorRepoPath ? 'Run in an isolated git worktree. Safer for parallel agents — your main working tree stays untouched.' : 'Editor has no folder open'}
          >
            <svg class="i i-sm" viewBox="0 0 24 24"><path d="M6 3v18M6 9a3 3 0 0 0 3 3h4a3 3 0 0 1 3 3v6M18 3a3 3 0 1 1 0 6 3 3 0 0 1 0-6z"/></svg>
            <span>{worktreeBusy === 'creating' ? 'Isolating…' : 'Isolate'}</span>
          </button>
        {/if}
        <button
          class="unlink-btn"
          onclick={() => { focusLocalSession(activeSess.id); onToggleEditorLink(); }}
          title="Unlink — the chat keeps its current folder as an explicit cwd"
        >
          <svg class="i i-sm" viewBox="0 0 24 24"><path d="M9 17H7A5 5 0 1 1 7 7h2M15 7h2a5 5 0 0 1 4 8M3 3l18 18"/></svg>
          <span>Unlink</span>
        </button>
      {:else}
        <button
          class="cwd-chip"
          class:has-cwd={activeSess.cwd}
          class:editor-linked={!activeSess.cwd && editorRepoPath}
          class:muted={!!activeSess.worktreePath}
          onclick={() => { focusLocalSession(activeSess.id); onPickCwd(); }}
          title={activeSess.worktreePath ? `Overridden by worktree below` : (activeSess.cwd ?? (editorRepoPath ? `Editor folder: ${editorRepoPath}` : 'Pick working directory'))}
        >
          <svg class="i i-sm" viewBox="0 0 24 24"><path d="M3 7v11a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2V9a2 2 0 0 0-2-2h-7L10 5H5a2 2 0 0 0-2 2z" /></svg>
          <span class="cwd-label mono">
            {#if activeSess.cwd}
              {shortPath(activeSess.cwd)}
            {:else if editorRepoPath}
              ↳ {shortenFsPath(editorRepoPath)}
            {:else}
              No folder
            {/if}
          </span>
        </button>
        {#if activeSess.cwd}
          <button class="icon-btn" onclick={() => { focusLocalSession(activeSess.id); onClearCwd(); }} title="Clear folder override" aria-label="Clear folder">
            <svg class="i i-sm" viewBox="0 0 24 24"><path d="M18 6 6 18M6 6l12 12"/></svg>
          </button>
        {/if}
        {#if editorInstances.length > 1}
          <!-- Multiple Editor benches open — let the user pick which one to
               attach this chat to by bench name. -->
          <div class="link-editor-picker">
            <Dropdown
              value=""
              options={editorInstances.map((e) => ({
                value: e.id,
                label: `Link to ${e.name}`
              }))}
              onChange={(id) => { focusLocalSession(activeSess.id); onLinkToEditorInstance(id); }}
              placeholder="Link editor…"
              ariaLabel="Link to editor bench"
            />
          </div>
        {:else}
          <button
            class="link-editor-btn"
            onclick={() => { focusLocalSession(activeSess.id); onToggleEditorLink(); }}
            disabled={editorInstances.length === 0}
            title={editorInstances.length === 0
              ? 'Open an Editor column first to link this chat to its folder.'
              : 'Link this chat to the Editor folder so the cwd tracks the Editor live.'}
          >
            <svg class="i i-sm" viewBox="0 0 24 24"><path d="M9 17H7A5 5 0 1 1 7 7h2M15 7h2a5 5 0 1 1 0 10h-2M8 12h8"/></svg>
            <span>Link editor</span>
          </button>
        {/if}
        {#if !activeSess.worktreePath}
          <button
            class="wt-chip wt-chip--create"
            onclick={() => { focusLocalSession(activeSess.id); onCreateWorktree(); }}
            disabled={worktreeBusy === 'creating' || (!activeSess.cwd && !editorRepoPath)}
            title={activeSess.cwd || editorRepoPath ? 'Run in an isolated git worktree. Safer for parallel agents — your main working tree stays untouched.' : 'Pick a folder first'}
          >
            <svg class="i i-sm" viewBox="0 0 24 24"><path d="M6 3v18M6 9a3 3 0 0 0 3 3h4a3 3 0 0 1 3 3v6M18 3a3 3 0 1 1 0 6 3 3 0 0 1 0-6z"/></svg>
            <span>{worktreeBusy === 'creating' ? 'Isolating…' : 'Isolate'}</span>
          </button>
        {/if}
      {/if}
      {#if activeSess.agentKind === 'cursor'}
        <div class="model-chip" title="Cursor model — forwarded to cursor-agent --model">
          <svg class="i i-sm" viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="3"/><path d="M12 2v3M12 19v3M2 12h3M19 12h3M4.9 4.9l2.1 2.1M17 17l2.1 2.1M4.9 19.1 7 17M17 7l2.1-2.1"/></svg>
          {#key activeSess.id}
            <Dropdown
              value={activeSess.cursorModel ?? ''}
              options={cursorModelOptions}
              onChange={(v) => onUpdateSessionCursorModel(activeSess.id, v || null)}
              ariaLabel="Cursor model"
              variant="ghost"
              compact
            />
          {/key}
        </div>
      {/if}
    </div>
    {#if sessionsState.activeByInstance[instanceId] === activeSess.id && activeRepoInfo && !activeRepoInfo.missing}
      <div class="repo-info-bar" class:is-git={activeRepoInfo.is_git} class:not-git={!activeRepoInfo.is_git}>
        {#if activeRepoInfo.is_git}
          <span class="repo-info-chip" title={activeRepoInfo.root ?? ''}>
            <svg class="i i-sm" viewBox="0 0 24 24"><path d="M6 3v18M6 9a3 3 0 0 0 3 3h4a3 3 0 0 1 3 3v6M18 3a3 3 0 1 1 0 6 3 3 0 0 1 0-6z"/></svg>
            <span class="mono">{activeRepoInfo.current_branch ?? 'detached'}</span>
          </span>
          {#if activeRepoInfo.dirty_count > 0}
            <span class="repo-info-chip repo-info-dirty" title="{activeRepoInfo.dirty_count} modified file(s), {activeRepoInfo.untracked_count} untracked">
              <span class="repo-info-dot"></span>
              {activeRepoInfo.dirty_count} dirty
            </span>
          {:else}
            <span class="repo-info-chip repo-info-clean" title="Working tree clean">
              <svg class="i i-sm" viewBox="0 0 24 24"><path d="M20 6 9 17l-5-5"/></svg>
              clean
            </span>
          {/if}
          {#if activeRepoInfo.ahead > 0 || activeRepoInfo.behind > 0}
            <span class="repo-info-chip" title="ahead/behind upstream">
              {#if activeRepoInfo.ahead > 0}↑{activeRepoInfo.ahead}{/if}
              {#if activeRepoInfo.behind > 0}↓{activeRepoInfo.behind}{/if}
            </span>
          {/if}
          {#if activeRepoInfo.remote_url}
            <span class="repo-info-remote mono" title={activeRepoInfo.remote_url}>
              {shortRemote(activeRepoInfo.remote_url)}
            </span>
          {:else}
            <span class="repo-info-chip repo-info-noremote" title="No remote configured — PR creation will fail">
              no remote
            </span>
          {/if}
        {:else}
          <span class="repo-info-chip repo-info-notgit">
            <svg class="i i-sm" viewBox="0 0 24 24"><path d="M12 9v4M12 17h.01M10.29 3.86 1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z"/></svg>
            not a git repo
          </span>
        {/if}
      </div>
    {/if}
    {#if activeSess.worktreePath}
      <div class="wt-bar">
        <button
          class="wt-chip wt-chip--active"
          onclick={() => { focusLocalSession(activeSess.id); onToggleWorktreeMenu(); }}
          title={activeSess.worktreePath}
          disabled={worktreeBusy === 'removing'}
        >
          <span class="wt-dot"></span>
          <svg class="i i-sm" viewBox="0 0 24 24"><path d="M6 3v18M6 9a3 3 0 0 0 3 3h4a3 3 0 0 1 3 3v6M18 3a3 3 0 1 1 0 6 3 3 0 0 1 0-6z"/></svg>
          <span class="mono">{activeSess.worktreeBranch}</span>
          <span class="wt-sub">isolated</span>
          <svg class="i i-sm" viewBox="0 0 24 24"><path d="M6 9l6 6 6-6"/></svg>
        </button>
        {#if worktreeMenuOpen && sessionsState.activeByInstance[instanceId] === activeSess.id}
          <div class="wt-menu">
            <div class="wt-menu-header mono" title={activeSess.worktreePath}>{shortenFsPath(activeSess.worktreePath)}</div>
            <button class="wt-menu-item" onclick={() => { focusLocalSession(activeSess.id); onOpenWorktreeDiff(); }}>
              <svg class="i i-sm" viewBox="0 0 24 24"><path d="M12 3v18M6 8l-3 4 3 4M18 8l3 4-3 4"/></svg>
              View changes
            </button>
            <button class="wt-menu-item" onclick={() => { focusLocalSession(activeSess.id); onOpenWorktreeInEditor(); }}>
              <svg class="i i-sm" viewBox="0 0 24 24"><path d="M3 7v11a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2V9a2 2 0 0 0-2-2h-7L10 5H5a2 2 0 0 0-2 2z" /></svg>
              Open worktree in Editor
            </button>
            <button class="wt-menu-item" onclick={() => { focusLocalSession(activeSess.id); onCopyWorktreeBranch(); }}>
              <svg class="i i-sm" viewBox="0 0 24 24"><rect x="9" y="9" width="13" height="13" rx="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></svg>
              Copy branch name
            </button>
            <div class="wt-menu-sep"></div>
            <button class="wt-menu-item wt-menu-item--apply" onclick={() => { focusLocalSession(activeSess.id); onApplyWorktree(); }} disabled={worktreeBusy !== null}>
              <svg class="i i-sm" viewBox="0 0 24 24"><path d="M20 6 9 17l-5-5"/></svg>
              Apply to current branch (merge)
            </button>
            <button class="wt-menu-item wt-menu-item--danger" onclick={() => { focusLocalSession(activeSess.id); onRemoveWorktree(); }} disabled={worktreeBusy === 'removing'}>
              <svg class="i i-sm" viewBox="0 0 24 24"><path d="M3 6h18M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2M19 6l-1 14a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2L5 6"/></svg>
              {worktreeBusy === 'removing' ? 'Removing…' : 'Discard worktree & branch'}
            </button>
          </div>
        {/if}
      </div>
    {/if}
  {/if}

  <!-- Session tabs -->
  <div class="chat-tabs">
    <div class="chat-tabs-scroll">
      {#each kindSessions as s (s.id)}
        <button
          class="chat-tab"
          class:active={s.id === (activeSess?.id ?? null)}
          onclick={() => focusLocalSession(s.id)}
          title={s.title}
        >
          {#if s.mentions.length > 0}
            <span class="chat-tab-mark mono">{s.mentions.length}</span>
          {/if}
          <span class="chat-tab-title">{s.title}</span>
          {#if kindSessions.length > 1}
            <span
              class="chat-tab-close"
              role="button"
              tabindex="0"
              aria-label="Close chat"
              onclick={(e) => { e.stopPropagation(); onDeleteClaudeSession(s.id); }}
              onkeydown={(e) => { if (e.key === 'Enter') { e.stopPropagation(); onDeleteClaudeSession(s.id); } }}
            >
              <svg class="i i-sm" viewBox="0 0 24 24"><path d="M18 6 6 18M6 6l12 12"/></svg>
            </span>
          {/if}
        </button>
      {/each}
    </div>
    <button class="chat-new" onclick={() => onNewClaudeSession({ agentKind: kind, columnInstanceId: instanceId })} title="New chat" aria-label="New chat">
      <svg class="i i-sm" viewBox="0 0 24 24"><path d="M12 5v14M5 12h14"/></svg>
    </button>
  </div>

  {#if !activeSess}
    <div class="claude-drop" class:drag-over={dragOver}>
      <svg class="i" viewBox="0 0 24 24" style="width:44px; height:44px; color: var(--accent-bright); opacity: 0.6;"><path d="M12 2l2.09 6.26L20 10.27l-5 4.87L16.18 22 12 18.56 7.82 22 9 15.14l-5-4.87 5.91-2.01L12 2z"/></svg>
      <div class="claude-drop-title">Start a chat</div>
      <div class="claude-drop-sub">Click + above to create a chat. Or drop a ticket here to open a context-first session.</div>
    </div>
  {:else}
    {@const sess = activeSess}
    <div class="claude-chat">

      <div class="chat-messages" bind:this={sessionsState.scrollEls[instanceId]}>
        {#if sess.messages.length === 0}
          <div class="chat-empty">
            <svg class="i" viewBox="0 0 24 24" style="width:28px; height:28px; color: var(--text-mute);"><path d="M12 2l2.09 6.26L20 10.27l-5 4.87L16.18 22 12 18.56 7.82 22 9 15.14l-5-4.87 5.91-2.01L12 2z"/></svg>
            <div class="chat-empty-title">Ask {brandLabel} anything</div>
            <div class="chat-empty-sub">
              Type a question below. Drop a ticket on this column to start a session with context.
            </div>
          </div>
        {:else}
          {#each sess.messages as msg, idx (idx)}
            <div class="chat-msg chat-msg--{msg.role}" class:chat-msg--editing={editingMsg && editingMsg.sessionId === sess.id && editingMsg.index === idx}>
              <div class="chat-msg-head">
                {#if msg.role === 'assistant'}
                  <span class="chat-avatar chat-avatar--claude">{brandInitial}</span>
                  <span class="chat-who">{brandLabel}</span>
                {:else if msg.role === 'user'}
                  {#if githubStatus.kind === 'connected'}
                    <img src={githubStatus.user.avatar_url} alt="" class="chat-avatar" />
                  {:else}
                    <span class="chat-avatar">NK</span>
                  {/if}
                  <span class="chat-who">You</span>
                {:else}
                  <span class="chat-avatar chat-avatar--system">•</span>
                  <span class="chat-who">System</span>
                {/if}
                <span class="chat-time mono">{relativeTime(msg.at, now)}</span>
                {#if msg.role === 'user' && !sess.sending}
                  <div class="chat-msg-actions">
                    <button
                      class="chat-msg-act"
                      onclick={() => { focusLocalSession(sess.id); onStartEditMessage(sess.id, idx, msg.content); }}
                      title="Edit & resend — everything after will be erased"
                      aria-label="Edit message"
                    >
                      <svg class="i i-sm" viewBox="0 0 24 24"><path d="M12 20h9M16.5 3.5a2.12 2.12 0 0 1 3 3L7 19l-4 1 1-4L16.5 3.5z"/></svg>
                    </button>
                    <button
                      class="chat-msg-act"
                      onclick={() => { focusLocalSession(sess.id); onResendMessage(sess.id, idx, msg.content); }}
                      title="Resend — everything after will be erased"
                      aria-label="Resend message"
                    >
                      <svg class="i i-sm" viewBox="0 0 24 24"><path d="M1 4v6h6M23 20v-6h-6"/><path d="M20.49 9A9 9 0 0 0 5.64 5.64L1 10m22 4-4.64 4.36A9 9 0 0 1 3.51 15"/></svg>
                    </button>
                  </div>
                {/if}
              </div>
              <div class="chat-msg-body">
                {#if editingMsg && editingMsg.sessionId === sess.id && editingMsg.index === idx}
                  <textarea
                    class="chat-msg-edit"
                    value={editingMsg.draft}
                    oninput={(e) => onSetEditingMsgDraft((e.currentTarget as HTMLTextAreaElement).value)}
                    rows="3"
                    {@attach (node: HTMLTextAreaElement) => { node.focus(); node.setSelectionRange(node.value.length, node.value.length); }}
                    onkeydown={(e) => {
                      if (e.key === 'Escape') { e.preventDefault(); onCancelEditMessage(); }
                      if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) { e.preventDefault(); onCommitEditMessage(); }
                    }}
                  ></textarea>
                  <div class="chat-msg-edit-actions">
                    <button class="btn-tiny" onclick={onCancelEditMessage}>Cancel</button>
                    <button class="btn-tiny btn-tiny--primary" onclick={onCommitEditMessage} disabled={!editingMsg.draft.trim()}>Send ⌘↵</button>
                  </div>
                {:else if msg.role === 'system'}
                  <div class="chat-plain">{msg.content}</div>
                {:else}
                  <Markdown source={msg.content} />
                {/if}
              </div>
            </div>
          {/each}
        {/if}
        {#if sess.sending}
          <div class="chat-typing">
            <span class="dot-pulse"></span><span class="dot-pulse"></span><span class="dot-pulse"></span>
            {#if thinkingStartedAt && sessionsState.activeByInstance[instanceId] === sess.id}
              <span class="thinking-time mono">
                {thinkingTick}s
              </span>
            {/if}
          </div>
        {/if}
      </div>

      {#if sess.actions.length > 0}
        <div class="action-cards">
          {#each sess.actions as act (act.id)}
            <ClaudeActionCard
              action={act}
              onUpdate={(patch) => onUpdateAction(sess.id, act.id, patch)}
              onDismiss={() => onRemoveAction(sess.id, act.id)}
              onExecute={() => {
                focusLocalSession(sess.id);
                onExecuteAction(sess.id, act);
              }}
              onOpenPrInForgehold={(url) => onOpenPrInForgehold(url, act.kind === 'pr' ? act : null)}
            />
          {/each}
        </div>
      {/if}

      {#if sess.mentions.some((m) => m.source === 'file')}
        <div class="attach-row">
          {#each sess.mentions.filter((m) => m.source === 'file') as m (m.externalId)}
            {@const abs = m.body ?? ''}
            {@const isImg = !m.isDir && isImagePath(abs)}
            <div class="attach-chip" class:attach-chip--image={isImg} title={abs || m.title}>
              {#if isImg}
                <img class="attach-thumb" src={convertFileSrc(abs)} alt={m.title} draggable="false" />
              {:else if m.isDir}
                <svg class="i i-sm attach-icon" viewBox="0 0 24 24"><path d="M3 7v11a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2V9a2 2 0 0 0-2-2h-7L10 5H5a2 2 0 0 0-2 2z" /></svg>
              {:else}
                <svg class="i i-sm attach-icon" viewBox="0 0 24 24"><path d="M14 3v4a1 1 0 0 0 1 1h4M17 21H7a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h7l5 5v11a2 2 0 0 1-2 2z" /></svg>
              {/if}
              <span class="attach-name mono">{m.title}</span>
              <button
                type="button"
                class="attach-remove"
                onclick={() => removeMention(m.externalId)}
                aria-label="Remove attachment"
                title="Remove"
              >
                <svg class="i i-sm" viewBox="0 0 24 24"><path d="M18 6 6 18M6 6l12 12"/></svg>
              </button>
            </div>
          {/each}
        </div>
      {/if}

      <form class="chat-input" onsubmit={(e) => { e.preventDefault(); focusLocalSession(sess.id); onSendClaudeMessage(); }}>
        <button
          type="button"
          class="chat-attach"
          onclick={() => { focusLocalSession(sess.id); void pickFiles(); }}
          disabled={sess.sending}
          aria-label="Attach files or images"
          title="Attach files or images (⌘-click for multi-select)"
        >
          <svg class="i" viewBox="0 0 24 24"><path d="M12 5v14M5 12h14"/></svg>
        </button>
        <textarea
          class="chat-textarea"
          value={sess.input}
          oninput={(e) => onSetSessionInput(sess.id, (e.currentTarget as HTMLTextAreaElement).value)}
          placeholder={sess.mentions.length ? 'Ask about the attached items (use @IDs in your text)…' : `Ask ${brandLabel} anything…`}
          disabled={sess.sending}
          onfocus={() => focusLocalSession(sess.id)}
          onkeydown={(e) => {
            if (e.key === 'Enter' && !e.shiftKey && !sess.sending) {
              e.preventDefault();
              focusLocalSession(sess.id);
              onSendClaudeMessage();
            }
          }}
        ></textarea>
        {#if sess.sending}
          <button
            type="button"
            class="chat-send chat-stop"
            onclick={() => { focusLocalSession(sess.id); onStopClaude(); }}
            aria-label="Stop"
            title="Stop generation"
          >
            <svg class="i" viewBox="0 0 24 24" fill="currentColor" stroke="none"><rect x="6" y="6" width="12" height="12" rx="2"/></svg>
          </button>
        {:else}
          <button
            type="submit"
            class="chat-send"
            disabled={!sess.input.trim()}
            aria-label="Send"
          >
            <svg class="i" viewBox="0 0 24 24"><path d="M22 2 11 13"/><polygon points="22 2 15 22 11 13 2 9 22 2"/></svg>
          </button>
        {/if}
      </form>
    </div>
  {/if}
</section>

<style>
  /* Agent (Claude / Cursor) chat column. Uses generic .wb-column rules
     from +page.svelte; all chat-, claude-, cwd-, wt-, model-, agent-
     scoped rules live here. */

  .claude-col {
    flex: 1.3 1 420px;
    min-width: 400px;
    display: flex; flex-direction: column;
    background: rgba(16, 24, 40, 0.3);
    transition: background 180ms, box-shadow 180ms;
  }
  .claude-col.drag-over {
    background: rgba(16, 185, 129, 0.05);
    box-shadow: inset 0 0 0 2px rgba(16, 185, 129, 0.4);
  }
  .claude-col .inbox-brand { border-bottom: 1px solid var(--border-neutral); }

  .inbox-brand {
    padding: 16px 20px 10px; display: flex; align-items: center; gap: 10px;
  }
  .brand-word { font-size: 14px; font-weight: 600; color: var(--text-0); letter-spacing: -0.01em; }
  .brand-sub { font-size: 11.5px; color: var(--text-2); margin-left: auto; }
  .source-mark {
    width: 22px; height: 22px; border-radius: 5px;
    display: inline-flex; align-items: center; justify-content: center;
    font-size: 10.5px; font-weight: 700; letter-spacing: -0.02em;
    background: var(--bg-2); color: var(--text-1);
    border: 1px solid var(--border-neutral-hi);
  }

  .claude-drop {
    flex: 1; margin: 16px;
    padding: 36px 20px;
    border: 1.5px dashed var(--border-neutral-hi);
    border-radius: 14px;
    display: flex; flex-direction: column;
    align-items: center; justify-content: center;
    text-align: center; gap: 10px;
    transition: all 220ms;
  }
  .claude-drop.drag-over {
    border-color: rgba(16, 185, 129, 0.55);
    background: radial-gradient(ellipse at center, rgba(16, 185, 129, 0.08), transparent 70%);
    transform: scale(1.01);
  }
  .claude-drop-title {
    font-size: 14px; font-weight: 600; color: var(--text-0);
    margin-top: 6px;
  }
  .claude-drop-sub { font-size: 12.5px; color: var(--text-2); max-width: 300px; line-height: 1.55; }

  .claude-chat {
    flex: 1; display: flex; flex-direction: column; min-height: 0;
  }

  .chat-messages {
    flex: 1; overflow-y: auto; padding: 16px 16px 8px;
    display: flex; flex-direction: column; gap: 14px;
  }
  .chat-msg-actions {
    display: inline-flex; gap: 2px; margin-left: auto;
    opacity: 0; transition: opacity 120ms;
  }
  .chat-msg:hover .chat-msg-actions { opacity: 1; }
  .chat-msg-act {
    width: 22px; height: 22px; border-radius: 4px;
    color: var(--text-2); background: transparent;
    display: inline-flex; align-items: center; justify-content: center;
    transition: all 120ms;
  }
  .chat-msg-act:hover { background: var(--bg-3); color: var(--accent-bright); }
  .chat-msg-act :global(svg) { width: 12px; height: 12px; }

  .chat-msg--editing {
    outline: 2px solid rgba(16, 185, 129, 0.35);
    outline-offset: -2px; border-radius: 8px;
  }
  .chat-msg-edit {
    width: 100%;
    padding: 10px 12px;
    background: var(--bg-0);
    border: 1px solid var(--border-hi2);
    border-radius: 8px;
    font: inherit;
    color: var(--text-0);
    font-size: 13px;
    resize: vertical;
    min-height: 72px;
  }
  .chat-msg-edit:focus { outline: none; border-color: var(--accent); }
  .chat-msg-edit-actions {
    margin-top: 6px;
    display: flex; gap: 6px; justify-content: flex-end;
  }
  :global(.btn-tiny) {
    padding: 5px 10px; border-radius: 6px; font-size: 11.5px; font-weight: 500;
    background: var(--bg-2); color: var(--text-1); border: 1px solid var(--border-neutral-hi);
    transition: all 120ms;
  }
  :global(.btn-tiny:hover:not(:disabled)) { background: var(--bg-3); color: var(--text-0); }
  :global(.btn-tiny--primary) {
    color: #0a111e;
    background: linear-gradient(135deg, #34d399, #10b981);
    border-color: rgba(16, 185, 129, 0.5);
    font-weight: 600;
  }
  :global(.btn-tiny--primary:hover:not(:disabled)) {
    filter: brightness(1.06);
  }
  :global(.btn-tiny:disabled) { opacity: 0.5; cursor: not-allowed; }

  .chat-msg {
    display: flex; flex-direction: column; gap: 6px;
  }
  .chat-msg-head {
    display: flex; align-items: center; gap: 8px;
    font-size: 12px;
  }
  .chat-avatar {
    width: 22px; height: 22px; border-radius: 50%;
    display: inline-flex; align-items: center; justify-content: center;
    font-size: 10px; font-weight: 700;
    background: var(--bg-2); color: var(--text-1);
    border: 1px solid var(--border-neutral-hi);
    flex-shrink: 0;
    object-fit: cover;
  }
  .chat-avatar--claude {
    background: rgba(16, 185, 129, 0.14);
    color: var(--accent-bright);
    border-color: rgba(16, 185, 129, 0.3);
  }
  .chat-avatar--system {
    background: rgba(59, 130, 246, 0.12);
    color: var(--blue-bright);
    border-color: rgba(59, 130, 246, 0.24);
    font-size: 14px;
  }
  .chat-who { font-weight: 600; color: var(--text-1); }
  .chat-time { margin-left: auto; color: var(--text-mute); font-size: 10.5px; }

  .chat-msg-body {
    padding-left: 30px;
    font-size: 13px; line-height: 1.55; color: var(--text-0);
  }
  .chat-plain {
    color: var(--text-1);
    font-size: 12.5px;
  }
  .chat-msg--user .chat-msg-body {
    color: var(--text-0);
  }
  .chat-msg--system .chat-msg-body {
    color: var(--text-2); font-style: italic;
  }

  .chat-typing {
    display: inline-flex; gap: 5px; align-items: center;
    padding-left: 30px;
  }
  .thinking-time {
    margin-left: 8px;
    color: var(--text-mute); font-size: 10.5px;
    font-variant-numeric: tabular-nums;
  }
  .dot-pulse {
    width: 6px; height: 6px; border-radius: 50%;
    background: var(--accent-bright);
    animation: pulse 1.2s ease-in-out infinite;
  }
  .dot-pulse:nth-child(2) { animation-delay: 0.2s; }
  .dot-pulse:nth-child(3) { animation-delay: 0.4s; }
  @keyframes pulse {
    0%, 100% { opacity: 0.3; transform: scale(0.8); }
    50% { opacity: 1; transform: scale(1.1); }
  }

  .action-cards {
    display: flex; flex-direction: column; gap: 10px;
    padding: 0 16px 10px;
  }

  /* Attachment chips — shown above the composer whenever the active session
     has `mentions`. Image mentions get a live thumbnail via `convertFileSrc`;
     everything else falls back to a folder/file/source-tag icon. */
  .attach-row {
    display: flex; flex-wrap: wrap; gap: 6px;
    padding: 10px 14px 0;
    background: var(--bg-1);
    border-top: 1px solid var(--border-neutral);
  }
  .attach-chip {
    display: inline-flex; align-items: center; gap: 6px;
    padding: 4px 4px 4px 8px;
    background: var(--bg-2);
    border: 1px solid var(--border-neutral-hi);
    border-radius: 7px;
    max-width: 240px;
    transition: border-color 120ms;
  }
  .attach-chip:hover { border-color: var(--border-hi); }
  .attach-chip--image { padding-left: 4px; }
  .attach-thumb {
    width: 26px; height: 26px;
    border-radius: 4px;
    object-fit: cover;
    flex-shrink: 0;
    background: var(--bg-3);
  }
  .attach-icon {
    width: 20px; height: 20px;
    color: var(--text-2);
    flex-shrink: 0;
  }
  .attach-icon--ticket {
    display: inline-flex; align-items: center; justify-content: center;
    width: 20px; height: 20px;
    font-size: 9px; font-weight: 700;
    border-radius: 4px;
    background: var(--bg-3);
    border: 1px solid var(--border-neutral-hi);
    color: var(--text-1);
  }
  .attach-name {
    font-size: 11.5px; color: var(--text-1);
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
    max-width: 160px;
  }
  .attach-remove {
    width: 20px; height: 20px;
    display: inline-flex; align-items: center; justify-content: center;
    border-radius: 4px;
    color: var(--text-mute);
    flex-shrink: 0;
    background: none; border: none; cursor: pointer;
  }
  .attach-remove:hover { color: var(--error); background: var(--bg-3); }
  .attach-remove svg { width: 12px; height: 12px; }

  .chat-input {
    display: flex; align-items: flex-end; gap: 8px;
    padding: 12px 14px;
    border-top: 1px solid var(--border-neutral);
    background: var(--bg-1);
  }
  .chat-attach {
    width: 38px; height: 38px; border-radius: 8px;
    display: inline-flex; align-items: center; justify-content: center;
    color: var(--text-2);
    background: var(--bg-0);
    border: 1px dashed var(--border-neutral-hi);
    transition: all 120ms;
    flex-shrink: 0;
    cursor: pointer;
  }
  .chat-attach:hover:not(:disabled) {
    color: var(--accent-bright);
    border-color: var(--border-hi);
    background: var(--accent-soft);
  }
  .chat-attach:disabled { opacity: 0.4; cursor: not-allowed; }
  .chat-attach svg { width: 18px; height: 18px; }
  .chat-textarea {
    flex: 1;
    min-height: 40px; max-height: 140px;
    padding: 10px 12px;
    background: var(--bg-0);
    border: 1px solid var(--border-neutral-hi);
    border-radius: 8px;
    color: var(--text-0);
    font: inherit;
    font-size: 13px;
    resize: vertical;
    transition: border-color 120ms;
  }
  .chat-textarea:focus { border-color: var(--accent); outline: none; }
  .chat-textarea:disabled { opacity: 0.5; }

  .chat-send {
    width: 38px; height: 38px; border-radius: 8px;
    display: inline-flex; align-items: center; justify-content: center;
    color: var(--accent-bright);
    background: var(--accent-soft);
    border: 1px solid rgba(16, 185, 129, 0.3);
    transition: all 120ms;
    flex-shrink: 0;
  }
  .chat-send:hover:not(:disabled) {
    background: rgba(16, 185, 129, 0.18);
    border-color: rgba(16, 185, 129, 0.5);
  }
  .chat-send:disabled { opacity: 0.4; cursor: not-allowed; }

  .chat-stop {
    color: var(--error);
    background: rgba(214, 72, 44, 0.12);
    border-color: rgba(214, 72, 44, 0.3);
  }
  .chat-stop:hover {
    background: rgba(214, 72, 44, 0.22);
    border-color: rgba(214, 72, 44, 0.5);
  }

  /* Cwd bar */
  .cwd-bar {
    display: flex; align-items: center; gap: 6px;
    padding: 8px 14px;
    border-bottom: 1px solid var(--border-neutral);
    background: var(--bg-1);
  }
  .agent-select {
    padding: 5px 8px; padding-right: 22px;
    background: var(--bg-0);
    border: 1px solid var(--border-neutral-hi);
    border-radius: 6px;
    color: var(--text-1);
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.03em;
    text-transform: uppercase;
    cursor: pointer;
    appearance: none;
    -webkit-appearance: none;
    background-image: url("data:image/svg+xml;utf8,<svg xmlns='http://www.w3.org/2000/svg' width='8' height='8' viewBox='0 0 24 24' fill='none' stroke='%23888' stroke-width='2.5' stroke-linecap='round' stroke-linejoin='round'><polyline points='6 9 12 15 18 9'/></svg>");
    background-repeat: no-repeat;
    background-position: right 6px center;
  }
  .agent-select:hover { border-color: var(--border-hi); color: var(--text-0); }
  .agent-select:focus { outline: none; border-color: var(--border-hi2); }
  .cwd-chip {
    display: inline-flex; align-items: center; gap: 8px;
    padding: 6px 10px;
    background: var(--bg-0);
    border: 1px dashed var(--border-neutral-hi);
    border-radius: 7px;
    color: var(--text-2);
    font-size: 11.5px;
    transition: all 120ms;
    flex: 1; min-width: 0;
  }
  .cwd-chip:hover { border-color: var(--border-hi); color: var(--text-0); }
  .cwd-chip.has-cwd {
    border-style: solid;
    border-color: rgba(16, 185, 129, 0.25);
    background: var(--accent-soft);
    color: var(--accent-bright);
  }
  .cwd-chip.editor-linked {
    border-color: var(--border-hi);
    color: var(--text-1);
  }
  .cwd-chip.muted { opacity: 0.5; }

  .model-chip {
    display: inline-flex; align-items: center; gap: 6px;
    padding: 4px 4px 4px 8px;
    border: 1px solid var(--border-neutral-hi);
    border-radius: 7px;
    background: var(--bg-0);
    color: var(--text-2);
    flex-shrink: 0;
  }
  .model-chip:hover { border-color: var(--border-hi); color: var(--text-1); }
  .model-chip:focus-within { border-color: var(--border-hi2); }
  .model-select { border: none; padding: 4px 20px 4px 0; background-color: transparent; }
  .model-select:hover { border: none; }
  .model-select:focus { border: none; }

  .repo-info-bar {
    display: flex; flex-wrap: wrap; align-items: center; gap: 5px;
    padding: 4px 14px 8px;
    background: var(--bg-1);
    font-size: 11px;
  }
  .repo-info-chip {
    display: inline-flex; align-items: center; gap: 4px;
    padding: 2px 7px;
    border-radius: 3px;
    background: var(--bg-2);
    color: var(--text-1);
    border: 1px solid var(--border-neutral);
  }
  .repo-info-clean { color: var(--success); border-color: rgba(217, 145, 60, 0.2); }
  .repo-info-dirty { color: var(--warning); border-color: rgba(229, 162, 42, 0.28); }
  .repo-info-dirty .repo-info-dot {
    width: 6px; height: 6px; border-radius: 50%;
    background: var(--warning);
  }
  .repo-info-notgit { color: var(--text-2); border-style: dashed; }
  .repo-info-noremote { color: var(--text-2); font-style: italic; }
  .repo-info-remote { color: var(--text-2); font-size: 10.5px; margin-left: 2px; opacity: 0.8; }

  .wt-chip {
    display: inline-flex; align-items: center; gap: 6px;
    padding: 5px 10px;
    border-radius: 7px;
    font-size: 11.5px;
    border: 1px dashed var(--border-hi);
    background: transparent;
    color: var(--text-1);
    transition: all 120ms;
    flex-shrink: 0;
  }
  .wt-chip:hover:not(:disabled) { color: var(--text-0); border-color: var(--accent); background: var(--accent-soft); }
  .wt-chip:disabled { opacity: 0.5; cursor: default; }
  .wt-chip--active {
    background: linear-gradient(135deg, rgba(238, 107, 31, 0.15), rgba(217, 145, 60, 0.08));
    border: 1px solid rgba(238, 107, 31, 0.4);
    color: var(--accent-bright);
    font-weight: 500;
  }
  .wt-chip--active:hover:not(:disabled) {
    border-color: var(--accent);
    background: linear-gradient(135deg, rgba(238, 107, 31, 0.22), rgba(217, 145, 60, 0.12));
  }
  .wt-dot {
    width: 7px; height: 7px; border-radius: 50%;
    background: var(--accent-bright);
    box-shadow: 0 0 8px var(--accent-glow);
    animation: wt-pulse 1.6s ease-in-out infinite;
  }
  @keyframes wt-pulse {
    0%, 100% { opacity: 0.55; transform: scale(0.9); }
    50% { opacity: 1; transform: scale(1.1); }
  }
  .wt-sub {
    font-size: 10px;
    color: var(--text-2);
    padding: 1px 6px;
    border-radius: 3px;
    background: var(--bg-2);
    margin-left: 2px;
    font-weight: 500;
    letter-spacing: 0.02em;
  }

  .wt-bar {
    position: relative;
    padding: 0 14px 10px;
    background: var(--bg-1);
  }
  .wt-menu {
    position: absolute; top: calc(100% - 6px); left: 14px; right: 14px;
    background: var(--bg-2);
    border: 1px solid var(--border-hi2);
    border-radius: 8px;
    box-shadow: 0 10px 30px rgba(0, 0, 0, 0.5);
    z-index: 20;
    padding: 4px;
    display: flex; flex-direction: column; gap: 1px;
  }
  .wt-menu-header {
    font-size: 10.5px; color: var(--text-2);
    padding: 8px 10px 6px;
    border-bottom: 1px solid var(--border-neutral);
    margin-bottom: 4px;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .wt-menu-item {
    display: flex; align-items: center; gap: 8px;
    padding: 7px 10px;
    border-radius: 5px;
    font-size: 12.5px; color: var(--text-1);
    text-align: left;
    transition: all 100ms;
  }
  .wt-menu-item:hover:not(:disabled) { background: var(--bg-3); color: var(--text-0); }
  .wt-menu-item:disabled { opacity: 0.5; cursor: default; }
  .wt-menu-item--danger { color: var(--error); }
  .wt-menu-item--danger:hover:not(:disabled) { background: rgba(214, 72, 44, 0.12); color: var(--error); }
  .wt-menu-item--apply { color: var(--accent-bright); font-weight: 500; }
  .wt-menu-item--apply:hover:not(:disabled) { background: var(--accent-soft); color: var(--accent); }
  .wt-menu-sep { height: 1px; background: var(--border-neutral); margin: 4px 0; }
  .cwd-label {
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
    font-size: 11px;
  }

  /* Chat tabs */
  .chat-tabs {
    display: flex;
    align-items: center; gap: 4px;
    border-bottom: 1px solid var(--border-neutral);
    padding: 8px 8px 8px 14px;
    background: var(--bg-1);
  }
  .chat-tabs-scroll {
    flex: 1;
    display: flex;
    align-items: center; gap: 2px;
    overflow-x: auto;
    scrollbar-width: none;
  }
  .chat-tabs-scroll::-webkit-scrollbar { display: none; }
  .chat-tab {
    display: inline-flex; align-items: center; gap: 6px;
    padding: 5px 9px;
    border-radius: 6px;
    font-size: 11.5px; color: var(--text-2);
    background: transparent;
    border: 1px solid transparent;
    transition: all 120ms;
    flex-shrink: 0;
    max-width: 160px;
  }
  .chat-tab:hover { color: var(--text-0); background: var(--bg-2); }
  .chat-tab.active {
    color: var(--text-0);
    background: var(--bg-2);
    border-color: var(--border-neutral-hi);
  }
  .chat-tab-mark {
    font-size: 9.5px; font-weight: 700;
    padding: 1px 5px;
    border-radius: 3px;
    background: rgba(16, 185, 129, 0.12);
    color: var(--accent-bright);
  }
  .chat-tab-title {
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .chat-tab-close {
    display: inline-flex; align-items: center; justify-content: center;
    width: 16px; height: 16px;
    border-radius: 3px;
    color: var(--text-mute);
    opacity: 0; transition: all 120ms;
  }
  .chat-tab:hover .chat-tab-close,
  .chat-tab.active .chat-tab-close { opacity: 1; }
  .chat-tab-close:hover { background: var(--bg-3); color: #fca5a5; }

  .chat-new {
    width: 26px; height: 26px;
    border-radius: 6px;
    color: var(--text-2);
    display: inline-flex; align-items: center; justify-content: center;
    transition: all 120ms;
    flex-shrink: 0;
  }
  .chat-new:hover { background: var(--bg-2); color: var(--accent-bright); }

  .chat-empty {
    flex: 1;
    display: flex; flex-direction: column; align-items: center; justify-content: center;
    gap: 10px; padding: 40px 20px;
    text-align: center;
  }
  .chat-empty-title {
    font-size: 13px; font-weight: 500; color: var(--text-1);
  }
  .chat-empty-sub {
    font-size: 12px; color: var(--text-2);
    max-width: 280px; line-height: 1.55;
  }
</style>
