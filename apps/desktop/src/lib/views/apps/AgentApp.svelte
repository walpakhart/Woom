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
  import Splitter from '$lib/components/ui/Splitter.svelte';
  import type { ClaudeAction } from '$lib/types';

  type Kind = 'claude' | 'cursor';

  interface Props {
    kind: Kind;
    instanceId: string;
    /** Тикер времени для elapsed-counter в header / thinking. */
    now: number;
    thinkingStartedAt: number | null;
    thinkingTick: number;
    worktreeBusy: 'creating' | 'removing' | null;

    /** Editor cwd — для cwd-bar editor-link hint. */
    editorRepoPath: string;

    /** Callbacks приходят из +page.svelte (там вся real state-mutation
     *  логика — runAgentRequest, applyWorktree, etc.). */
    onPickCwd: () => void;
    onClearCwd: () => void;
    onToggleEditorLink: () => void;
    onLinkToEditorInstance: (editorInstanceId: string) => void;
    /** Drop the active session's terminal link. Forwarded into
     *  WorktreeBar so the cwd-bar chip × can untap a session from a
     *  terminal without bouncing to the terminal app. */
    onToggleTerminalLink?: () => void;
    /** Bind the active session to a specific terminal instance. */
    onLinkToTerminalInstance?: (terminalInstanceId: string) => void;
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
  }
  let p: Props = $props();

  /* App-shell ambient tone. Both agent apps now ride the main brand
     accent (mint/sage) so the entire workspace feels cohesive under
     the new W-mark palette. Per-source identification (Claude warm
     peach, Cursor grey) is preserved in the inbox chips + rail icons
     where the user needs to triage which agent is which — the chat
     SHELL itself is brand-uniform. */
  const tone = $derived('var(--accent)');
  const glow = $derived('var(--accent-glow)');
</script>

<section
  class="app-shell sa"
  style="--app-tone: {tone}; --app-glow: {glow};"
>
  <!-- Outer split: sessions sidebar (280, fixed) | chat + worktree (flex). -->
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
              onToggleTerminalLink={p.onToggleTerminalLink}
              onLinkToTerminalInstance={p.onLinkToTerminalInstance}
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
          />
        {/snippet}
      </Splitter>
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
  }

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
