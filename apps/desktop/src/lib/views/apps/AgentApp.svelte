<script lang="ts">
  /* AgentApp — full-screen workspace для Claude / Cursor.
     ПОЛНОСТЬЮ standalone, без AgentApp.

     Layout (3 pane):
       [SessionsSidebar 280] [chat (flex)] [WorktreeSide 320]

     Center chat собран из 4 standalone-компонентов:
       ChatHeader  — серифный title + status + model + stop
       WorktreeBar — cwd chip + editor link + worktree controls
       ChatThread  — messages (events / thinking / images) + ClaudeActionCard'ы
       Composer    — textarea + chips + send

     Все они читают sessionsState напрямую. Никакого dependence on
     AgentApp — это полноценное независимое UI. */
  import SessionsSidebar from './agent/SessionsSidebar.svelte';
  import WorktreeSide from './agent/WorktreeSide.svelte';
  import ChatHeader from './agent/ChatHeader.svelte';
  import WorktreeBar from './agent/WorktreeBar.svelte';
  import ChatThread from './agent/ChatThread.svelte';
  import Composer from './agent/Composer.svelte';
  import PreviewPane from './agent/PreviewPane.svelte';
  import Splitter from '$lib/components/ui/Splitter.svelte';
  import { onMount } from 'svelte';
  import { bgTasksState } from '$lib/state/bgTasks.svelte';
  import type { ClaudeAction } from '$lib/types';

  type Kind = 'claude' | 'cursor';

  interface Props {
    kind: Kind;
    instanceId: string;
    /** Тикер времени для elapsed-counter в header / thinking. */
    now: number;
    thinkingStartedAt: Record<string, number | null>;
    thinkingTick: Record<string, number>;
    worktreeBusy: 'creating' | 'removing' | null;

    /** Editor cwd — для cwd-bar editor-link hint. */
    editorRepoPath: string;

    /** Callbacks приходят из +page.svelte (там вся real state-mutation
     *  логика — runAgentRequest, applyWorktree, etc.). */
    onPickCwd: () => void;
    onClearCwd: () => void;
    onToggleEditorLink: () => void;
    onLinkToEditorInstance: (editorInstanceId: string) => void;
    /** Move agent cwd onto the linked editor's repoPath — user pick
     *  surfaced by the orange "Folder mismatch" button in WorktreeBar. */
    onSyncAgentToEditor: () => void;
    /** Inverse direction: push agent's cwd/worktree onto the linked
     *  editor's repoPath. Wired to the second option of the same menu. */
    onSyncEditorToAgent: () => void;
    /** Drop the active session's terminal link. Forwarded into
     *  WorktreeBar so the cwd-bar chip × can untap a session from a
     *  terminal without bouncing to the terminal app. */
    onToggleTerminalLink?: () => void;
    /** Bind the active session to a specific terminal instance. */
    onLinkToTerminalInstance?: (terminalInstanceId: string) => void;
    /** Unlink the active session from its canvas. */
    onToggleCanvasLink?: () => void;
    /** Link the active session to a canvas document by ID. */
    onLinkToCanvas?: (canvasId: string) => void;
    onCreateWorktree: () => void;
    onOpenWorktreeDiff: () => void;
    onOpenWorktreeInEditor: () => void;
    onCopyWorktreeBranch: () => void;
    onRemoveWorktree: () => void;
    onStartEditMessage: (sessionId: string, index: number, content: string) => void;
    onResendMessage: (sessionId: string, index: number, content: string) => void;
    onUpdateAction: (sessionId: string, actionId: string, patch: Partial<ClaudeAction>) => void;
    onRemoveAction: (sessionId: string, actionId: string) => void;
    onExecuteAction: (sessionId: string, action: ClaudeAction) => void;
    onOpenPrInWoom: (url: string, action: (ClaudeAction & { kind: 'pr' }) | null) => void;
    onSend: () => void;
    onStop: () => void;
    onPasteImages: (
      kind: Kind,
      blobs: { name: string; type: string; blob: Blob }[]
    ) => Promise<number>;
    /** Drag-drop forwarded to +page.svelte's `onAgentDrop` /
     *  `onAgentDragOver` / `onAgentDragLeave` so the existing
     *  ticket / file / Sentry attach pipeline keeps working. */
    onDragOver?: (e: DragEvent) => void;
    onDrop?: (e: DragEvent) => void;
    onDragLeave?: (e: DragEvent) => void;
    /** Click on a file/dir reference inside a chat bubble — opens it
     *  in the linked editor (or the active editor instance). */
    onOpenFile?: (path: string) => void;
    onSddAdvance?: (sessionId: string, prompt: string) => void;
    onResumeAfterQuota?: (sessionId: string) => void;
  }
  let p: Props = $props();

  /* App-shell ambient tone. Each agent surface now carries its
     own brand accent — Claude warm peach, Cursor moonlit silver —
     instead of riding the unified mint. The `data-kind` attribute
     hands off to per-shell `--accent-*` overrides in app.css so
     focus rings, link chips, button glows, etc. all retint
     downstream without per-component changes. */
  const tone = $derived('var(--accent)');
  const glow = $derived('var(--accent-glow)');

  /** Worktree pane open state. Persisted globally per agent kind so
   *  Claude and Cursor can have independent preferences (some users
   *  keep Cursor's worktree pane closed because it gets noisy with
   *  many parallel sessions). */
  // svelte-ignore state_referenced_locally
  const wtStorageKey = `agent-worktree-side-open:${p.kind}`;
  let worktreeOpen = $state(true);
  // svelte-ignore state_referenced_locally
  const pvStorageKey = `agent-preview-side-open:${p.kind}`;
  /** Preview pane open state. Defaults closed — opens via the rail icon,
   *  the /preview slash command, or auto-opens when a task spawns from
   *  this session (see effect below). */
  let previewOpen = $state(false);
  onMount(() => {
    const v = localStorage.getItem(wtStorageKey);
    if (v === '0' || v === '1') worktreeOpen = v === '1';
    const pv = localStorage.getItem(pvStorageKey);
    if (pv === '0' || pv === '1') previewOpen = pv === '1';
    /* `/preview` slash command fires this — only respond if the event
     *  matches our kind so Cursor's pane doesn't open when the user
     *  ran the command inside Claude. */
    const onOpen = (e: Event) => {
      const evt = e as CustomEvent<{ kind?: 'claude' | 'cursor' }>;
      if (!evt.detail || evt.detail.kind === p.kind) previewOpen = true;
    };
    window.addEventListener('woom:open-preview', onOpen);
    return () => window.removeEventListener('woom:open-preview', onOpen);
  });
  $effect(() => {
    localStorage.setItem(wtStorageKey, worktreeOpen ? '1' : '0');
  });
  $effect(() => {
    localStorage.setItem(pvStorageKey, previewOpen ? '1' : '0');
  });

  /* Per-kind unread-task badge — count of background tasks the user
   *  hasn't acknowledged. For now: any running task while pane is
   *  closed counts. Future: track per-task "seen" timestamp. */
  const previewBadgeCount = $derived(
    previewOpen ? 0 : bgTasksState.tasks.filter((t) => t.status.kind === 'running').length
  );
</script>

<section
  class="app-shell sa"
  data-kind={p.kind}
  style="--app-tone: {tone}; --app-glow: {glow};"
>
  <!-- Outer split: sessions sidebar (280, fixed) | chat + worktree + preview (flex). -->
  <Splitter
    direction="horizontal"
    fixedSide="start"
    persistKey="agent-{p.kind}-sessions"
    initial={280}
    min={220}
    max={480}
  >
    {#snippet start()}
      <SessionsSidebar kind={p.kind} instanceId={p.instanceId} now={p.now} />
    {/snippet}
    {#snippet end()}
      {#snippet midStack()}
        {#if worktreeOpen}
        <!-- Inner split: chat (flex) | worktree side (320, fixed). -->
        <Splitter
          direction="horizontal"
          fixedSide="end"
          persistKey="agent-{p.kind}-worktree"
          initial={320}
          min={260}
          max={520}
        >
          {#snippet start()}
            <section class="sa-chat app-pane">
              <ChatHeader
                kind={p.kind}
                instanceId={p.instanceId}
                thinkingStartedAt={p.thinkingStartedAt}
                thinkingTick={p.thinkingTick}
                onStop={p.onStop}
              />
              <WorktreeBar
                kind={p.kind}
                editorRepoPath={p.editorRepoPath}
                onPickCwd={p.onPickCwd}
                onClearCwd={p.onClearCwd}
                onToggleEditorLink={p.onToggleEditorLink}
                onLinkToEditorInstance={p.onLinkToEditorInstance}
                onSyncAgentToEditor={p.onSyncAgentToEditor}
                onSyncEditorToAgent={p.onSyncEditorToAgent}
                onToggleTerminalLink={p.onToggleTerminalLink}
                onLinkToTerminalInstance={p.onLinkToTerminalInstance}
                onToggleCanvasLink={p.onToggleCanvasLink}
                onLinkToCanvas={p.onLinkToCanvas}
                onCreateWorktree={p.onCreateWorktree}
                onOpenWorktreeDiff={p.onOpenWorktreeDiff}
                onRemoveWorktree={p.onRemoveWorktree}
                worktreeBusy={p.worktreeBusy}
              />
              <ChatThread
                kind={p.kind}
                thinkingStartedAt={p.thinkingStartedAt}
                thinkingTick={p.thinkingTick}
                onUpdateAction={p.onUpdateAction}
                onRemoveAction={p.onRemoveAction}
                onExecuteAction={p.onExecuteAction}
                onOpenPrInWoom={p.onOpenPrInWoom}
                onStartEditMessage={p.onStartEditMessage}
                onResendMessage={p.onResendMessage}
                onOpenFile={p.onOpenFile}
                onSddAdvance={p.onSddAdvance}
              onResumeAfterQuota={p.onResumeAfterQuota}
  />
              <Composer
                kind={p.kind}
                onSend={p.onSend}
                onStop={p.onStop}
                onPasteImages={p.onPasteImages}
                onDragOver={p.onDragOver}
                onDrop={p.onDrop}
                onDragLeave={p.onDragLeave}
              />
            </section>
          {/snippet}
          {#snippet end()}
            <WorktreeSide
              kind={p.kind}
              onOpenWorktreeDiff={p.onOpenWorktreeDiff}
              onCopyWorktreeBranch={p.onCopyWorktreeBranch}
              onOpenWorktreeInEditor={p.onOpenWorktreeInEditor}
              onCreateWorktree={p.onCreateWorktree}
              onRemoveWorktree={p.onRemoveWorktree}
              worktreeBusy={p.worktreeBusy}
              onCollapse={() => (worktreeOpen = false)}
            />
          {/snippet}
        </Splitter>
      {:else}
        <!-- Worktree collapsed: chat pane on the left (1fr) + 44px
             rail on the right. Two siblings inside the Splitter's
             `end` snippet — wrapper grid orchestrates the layout
             so each keeps its own .app-pane chrome and they don't
             share borders. -->
        <div class="sa-end-grid">
          <section class="sa-chat sa-chat--full app-pane">
            <ChatHeader
              kind={p.kind}
              instanceId={p.instanceId}
              thinkingStartedAt={p.thinkingStartedAt}
              thinkingTick={p.thinkingTick}
              onStop={p.onStop}
            />
            <WorktreeBar
              kind={p.kind}
              editorRepoPath={p.editorRepoPath}
              onPickCwd={p.onPickCwd}
              onClearCwd={p.onClearCwd}
              onToggleEditorLink={p.onToggleEditorLink}
              onLinkToEditorInstance={p.onLinkToEditorInstance}
              onSyncAgentToEditor={p.onSyncAgentToEditor}
              onSyncEditorToAgent={p.onSyncEditorToAgent}
              onToggleTerminalLink={p.onToggleTerminalLink}
              onLinkToTerminalInstance={p.onLinkToTerminalInstance}
              onToggleCanvasLink={p.onToggleCanvasLink}
              onLinkToCanvas={p.onLinkToCanvas}
              onCreateWorktree={p.onCreateWorktree}
              onOpenWorktreeDiff={p.onOpenWorktreeDiff}
              onRemoveWorktree={p.onRemoveWorktree}
              worktreeBusy={p.worktreeBusy}
            />
            <ChatThread
              kind={p.kind}
              thinkingStartedAt={p.thinkingStartedAt}
              thinkingTick={p.thinkingTick}
              onUpdateAction={p.onUpdateAction}
              onRemoveAction={p.onRemoveAction}
              onExecuteAction={p.onExecuteAction}
              onOpenPrInWoom={p.onOpenPrInWoom}
              onStartEditMessage={p.onStartEditMessage}
              onResendMessage={p.onResendMessage}
              onOpenFile={p.onOpenFile}
              onSddAdvance={p.onSddAdvance}
              onResumeAfterQuota={p.onResumeAfterQuota}
/>
            <Composer
              kind={p.kind}
              onSend={p.onSend}
              onStop={p.onStop}
              onPasteImages={p.onPasteImages}
              onDragOver={p.onDragOver}
              onDrop={p.onDrop}
              onDragLeave={p.onDragLeave}
            />
          </section>
          <aside class="sa-rail app-pane">
            <button
              class="sa-rail-btn"
              aria-label="Expand worktree pane"
              onclick={() => (worktreeOpen = true)}
            >
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M14 6l-6 6 6 6"/></svg>
            </button>
            <div class="sa-rail-divider" aria-hidden="true"></div>
            <div class="sa-rail-glyph" aria-label="Worktree">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                <path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>
              </svg>
            </div>
          </aside>
        </div>
      {/if}
      {/snippet}
      <!-- Preview pane: another collapsible right side. When open,
           splits midStack ↔ preview; when closed, midStack + a 44px
           preview-rail. Mirrors the worktree pattern so both sides
           collapse independently. -->
      {#if previewOpen}
        <Splitter
          direction="horizontal"
          fixedSide="end"
          persistKey="agent-{p.kind}-preview"
          initial={360}
          min={280}
          max={600}
        >
          {#snippet start()}
            {@render midStack()}
          {/snippet}
          {#snippet end()}
            <PreviewPane
              kind={p.kind}
              instanceId={p.instanceId}
              onCollapse={() => (previewOpen = false)}
            />
          {/snippet}
        </Splitter>
      {:else}
        <div class="sa-outer-grid">
          {@render midStack()}
          <aside class="sa-rail sa-rail--preview app-pane">
            <button
              class="sa-rail-btn"
              aria-label="Expand preview pane"
              onclick={() => (previewOpen = true)}
            >
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M14 6l-6 6 6 6"/></svg>
              {#if previewBadgeCount > 0}
                <span class="sa-rail-badge mono">{previewBadgeCount > 9 ? '9+' : previewBadgeCount}</span>
              {/if}
            </button>
            <div class="sa-rail-divider" aria-hidden="true"></div>
            <div class="sa-rail-glyph" aria-label="Preview">
              <!-- "play-in-window" glyph — distinguishes the preview
                   rail from the worktree rail's folder icon. -->
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                <rect x="3" y="5" width="18" height="14" rx="2"/>
                <polygon points="10,9 16,12 10,15" fill="currentColor" stroke="none"/>
              </svg>
            </div>
          </aside>
        </div>
      {/if}
    {/snippet}
  </Splitter>
</section>

<style>
  /* The shell becomes a single block (no grid) — Splitter handles layout. */
  .sa { display: block; padding: var(--app-pad, 14px); }

  /* Center chat pane — header(56) + cwd(38) + thread(flex) + composer(auto).
     Pane chrome (border + radius + shadow) comes from .app-pane. */
  .sa-chat {
    flex: 1;
    min-width: 0;
    display: flex; flex-direction: column;
    overflow: hidden;
    height: 100%;
    position: relative;
  }
  .sa-chat--full {
    width: 100%;
  }
  /* Worktree-collapsed end-snippet wrapper: chat (1fr) + rail
     (44px). The rail is a sibling of the chat pane (not nested
     inside it), so each keeps its own .app-pane chrome and the
     rail looks/behaves like a mini ActivityBar floating with
     rounded corners on the right. The 14px gap matches the
     surrounding `.app-shell` rhythm. */
  .sa-end-grid {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 44px;
    /* `minmax(0, 1fr)` on the row track prevents the grid item (chat
       pane with its long ChatThread) from forcing the row taller than
       the viewport — without it, `grid-template-rows` defaults to
       `auto`, the row grows to fit content, and Composer slides off
       the bottom of the screen on long sessions. */
    grid-template-rows: minmax(0, 1fr);
    gap: var(--app-gap, 14px);
    width: 100%; height: 100%;
    min-height: 0;
    transition: grid-template-columns var(--dur-base) var(--ease-out);
  }
  /* Outer grid wraps midStack + the 44px preview rail when preview
     pane is collapsed. Same vocabulary as .sa-end-grid so the rail
     reads as a sibling of the inner stack, not nested. */
  .sa-outer-grid {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 44px;
    /* See .sa-end-grid above — row track must be capped at viewport
       height or the inner midStack (chat pane) overflows and pushes
       the composer off-screen when Preview pane collapses. */
    grid-template-rows: minmax(0, 1fr);
    gap: var(--app-gap, 14px);
    width: 100%; height: 100%;
    min-height: 0;
    transition: grid-template-columns var(--dur-base) var(--ease-out);
  }
  /* Grid children must also opt-in to `min-height: 0` so their
     internal flex containers (`.sa-chat` etc.) honour the row cap.
     CSS grid sets `min-height: auto` by default on items, which
     defeats the row track's `minmax(0, 1fr)` constraint. */
  .sa-outer-grid > :global(*),
  .sa-end-grid > :global(*) {
    min-height: 0;
    min-width: 0;
  }
  /* Discriminator class for future per-rail styling (e.g. tone shift).
     Empty for now — keeps the selector in place so JS code that toggles
     the class doesn't need a no-op fallback. */
  .sa-rail--preview {
    /* placeholder — extend with tone shift later */
    background: transparent;
  }
  .sa-rail-badge {
    position: absolute;
    top: -3px; right: -3px;
    min-width: 14px; height: 14px;
    padding: 0 3px;
    border-radius: 7px;
    background: var(--accent-bright);
    color: var(--accent-fg, #1a1f1c);
    font-size: 9px; font-weight: 700;
    line-height: 14px;
    display: grid; place-items: center;
    box-shadow: 0 0 0 2px var(--bg-1);
    pointer-events: none;
  }
  .sa-rail-btn { position: relative; }
  .sa-rail {
    display: flex; flex-direction: column;
    align-items: center;
    gap: 4px;
    padding: 8px 0;
    height: 100%;
  }
  .sa-rail-btn {
    width: 32px; height: 32px;
    display: grid; place-items: center;
    border-radius: 8px;
    color: var(--text-2);
    background: transparent; border: 1px solid transparent;
    cursor: pointer;
    transition:
      color var(--dur-quick) var(--ease-out),
      background var(--dur-quick) var(--ease-out),
      border-color var(--dur-quick) var(--ease-out);
  }
  .sa-rail-btn:hover {
    color: var(--text-0);
    background: var(--bg-2);
    border-color: var(--border-hi);
  }
  .sa-rail-btn svg { width: 14px; height: 14px; }
  .sa-rail-divider {
    width: 22px; height: 1px;
    background: var(--border);
    margin: 2px 0;
  }
  .sa-rail-glyph {
    width: 32px; height: 32px;
    display: grid; place-items: center;
    color: var(--text-mute);
  }
  .sa-rail-glyph svg { width: 16px; height: 16px; }

  /* Splitter children fill their pane fully. */
  .sa :global(.s-start),
  .sa :global(.s-end) {
    height: 100%;
    display: flex;
    min-width: 0;
  }
  .sa :global(.s-start) > :global(*),
  .sa :global(.s-end) > :global(*) {
    flex: 1 1 auto;
    min-width: 0;
    width: 100%;
  }
</style>
