<script lang="ts">
  /* AgentApp — full-screen Claude workspace (Cabin, redesign v2 §2.5).
     Three flat panes:
       [SessionsSidebar 264] [chat (flex)] [ContextDock 300 · collapsible]
     The old Splitter cascade + WorktreeBar + WorktreeSide + the two 44px
     rails are gone; repo/links/run/budget/memory/tasks all live in the
     ContextDock. PreviewPane is now an overlay sheet (opened from the
     dock's Tasks section or the /preview command), not a permanent pane. */
  import SessionsSidebar from './agent/SessionsSidebar.svelte';
  import ChatHeader from './agent/ChatHeader.svelte';
  import QuietChatHeader from './agent/QuietChatHeader.svelte';
  import ChatThread from './agent/ChatThread.svelte';
  import Composer from './agent/Composer.svelte';
  import ContextDock from './agent/ContextDock.svelte';
  import PreviewPane from './agent/PreviewPane.svelte';
  import { onMount } from 'svelte';
  import { sessionsState } from '$lib/state/sessions.svelte';
  import { layoutModeState } from '$lib/state/layoutMode.svelte';
  import type { ClaudeAction } from '$lib/types';

  type Kind = 'claude';

  interface Props {
    kind: Kind;
    instanceId: string;
    now: number;
    thinkingStartedAt: Record<string, number | null>;
    thinkingTick: Record<string, number>;
    worktreeBusy: 'creating' | 'removing' | null;
    editorRepoPath: string;
    onPickCwd: () => void;
    onClearCwd: () => void;
    onToggleEditorLink: () => void;
    onLinkToEditorInstance: (editorInstanceId: string) => void;
    onSyncAgentToEditor: () => void;
    onSyncEditorToAgent: () => void;
    onToggleTerminalLink?: () => void;
    onLinkToTerminalInstance?: (terminalInstanceId: string) => void;
    onToggleCanvasLink?: () => void;
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
      blobs: { name: string; type: string; blob: Blob }[]
    ) => Promise<number>;
    onDragOver?: (e: DragEvent) => void;
    onDrop?: (e: DragEvent) => void;
    onDragLeave?: (e: DragEvent) => void;
    onOpenFile?: (path: string) => void;
    onDwVerify?: (workflowId: string) => void;
    onResumeAfterQuota?: (sessionId: string) => void;
  }
  let p: Props = $props();

  const tone = $derived('var(--accent)');
  const glow = $derived('var(--accent-glow)');
  const quiet = $derived(layoutModeState.mode === 'quiet');

  /* Context dock open state — persisted per agent kind (Cabin only).
     In Quiet the context is a popover from the header chip, defaulting
     closed (`ctxPopOpen`), so the empty stage stays empty. */
  // svelte-ignore state_referenced_locally
  const dockKey = `woom:agent-context-dock:v1:${p.kind}`;
  let dockOpen = $state(true);
  let ctxPopOpen = $state(false);

  /* Preview overlay — ephemeral. Opened from the dock's Tasks section
     ("preview") or the /preview slash command's window event. */
  let previewOpen = $state(false);

  onMount(() => {
    const v = localStorage.getItem(dockKey);
    if (v === '0' || v === '1') dockOpen = v === '1';
    const onOpen = () => { previewOpen = true; };
    window.addEventListener('woom:open-preview', onOpen);
    return () => window.removeEventListener('woom:open-preview', onOpen);
  });
  $effect(() => { localStorage.setItem(dockKey, dockOpen ? '1' : '0'); });
</script>

<section
  class="app-shell sa"
  data-kind={p.kind}
  style="--app-tone: {tone}; --app-glow: {glow};"
>
  <div class="sa-sessions">
    <SessionsSidebar kind={p.kind} instanceId={p.instanceId} now={p.now} />
  </div>

  <section class="sa-chat">
    {#if quiet}
      <QuietChatHeader
        kind={p.kind}
        instanceId={p.instanceId}
        thinkingStartedAt={p.thinkingStartedAt}
        thinkingTick={p.thinkingTick}
        onStop={p.onStop}
        onPickCwd={p.onPickCwd}
        contextOpen={ctxPopOpen}
        onToggleContext={() => (ctxPopOpen = !ctxPopOpen)}
      />
    {:else}
      <ChatHeader
        kind={p.kind}
        instanceId={p.instanceId}
        thinkingStartedAt={p.thinkingStartedAt}
        thinkingTick={p.thinkingTick}
        onStop={p.onStop}
        contextOpen={dockOpen}
        onToggleContext={() => (dockOpen = !dockOpen)}
      />
    {/if}
    <!-- {#key}: thread MUST remount per session — the windowed lazy-mount
         (IntersectionObserver) leaves reused nodes stuck as stubs. -->
    {#key sessionsState.activeIds[p.kind]}
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
        onDwVerify={p.onDwVerify}
        onResumeAfterQuota={p.onResumeAfterQuota}
      />
    {/key}
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

  {#snippet contextDock(onCollapse: () => void)}
    <ContextDock
      kind={p.kind}
      instanceId={p.instanceId}
      editorRepoPath={p.editorRepoPath}
      worktreeBusy={p.worktreeBusy}
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
      onOpenWorktreeInEditor={p.onOpenWorktreeInEditor}
      onCopyWorktreeBranch={p.onCopyWorktreeBranch}
      onRemoveWorktree={p.onRemoveWorktree}
      {onCollapse}
    />
  {/snippet}

  {#if quiet}
    {#if ctxPopOpen}
      <div class="sa-ctx-pop">{@render contextDock(() => (ctxPopOpen = false))}</div>
      <button class="sa-ctx-scrim" aria-label="Close context" onclick={() => (ctxPopOpen = false)}></button>
    {/if}
  {:else if dockOpen}
    {@render contextDock(() => (dockOpen = false))}
  {:else}
    <aside class="sa-dock-rail">
      <button class="sa-dock-expand" aria-label="Open context" title="Open context" onclick={() => (dockOpen = true)}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M14 6l-6 6 6 6"/></svg>
      </button>
    </aside>
  {/if}
</section>

{#if previewOpen}
  <div class="sa-preview-overlay" role="button" tabindex="-1"
    onclick={(e) => { if (e.target === e.currentTarget) previewOpen = false; }}
    onkeydown={(e) => { if (e.key === 'Escape') previewOpen = false; }}>
    <div class="sa-preview-sheet">
      <PreviewPane kind={p.kind} instanceId={p.instanceId} onCollapse={() => (previewOpen = false)} />
    </div>
  </div>
{/if}

<style>
  /* Three flat panes butted together — Splitter cascade removed. */
  .sa { display: flex; padding: 0; }
  .sa-sessions { flex: none; width: 264px; display: flex; min-height: 0; }
  .sa-sessions > :global(*) { flex: 1; width: 100%; }
  .sa-chat {
    flex: 1; min-width: 0;
    display: flex; flex-direction: column;
    overflow: hidden; height: 100%;
    background: var(--bg-0);
    position: relative;
  }
  /* Quiet context popover (§3.2) — floats the ContextDock under the
     header "context ▾" chip instead of a persistent column. */
  .sa-ctx-pop {
    position: fixed;
    top: 92px;
    /* Anchor under the header "context ▾" chip. NO `transform` here —
       a transformed ancestor becomes the containing block for
       `position:fixed` descendants, which threw the ModelEngine model
       picker + BudgetPopover (both fixed, positioned in viewport coords)
       off-screen. Use a plain `left` calc for the same offset. */
    left: calc(50% - 210px);
    width: 384px; max-height: 72vh;
    z-index: 130;
    border-radius: 12px; border: 1px solid var(--border-hi);
    box-shadow: var(--shadow-3);
    overflow: hidden auto;
    display: flex;
  }
  .sa-ctx-pop > :global(.cd) { width: 100%; border: 0; }
  .sa-ctx-scrim {
    position: fixed; inset: 0; z-index: 120;
    background: transparent; border: 0; cursor: default;
  }

  /* Collapsed-dock rail — thin strip with an expand chevron. */
  .sa-dock-rail {
    flex: none; width: 44px;
    display: flex; flex-direction: column; align-items: center;
    padding: 10px 0;
    background: var(--bg-1);
    border-left: 1px solid var(--border-lo);
  }
  .sa-dock-expand {
    width: 32px; height: 32px; display: grid; place-items: center;
    border: 1px solid transparent; border-radius: 8px;
    background: transparent; color: var(--text-2); cursor: pointer;
  }
  .sa-dock-expand:hover { color: var(--text-0); background: var(--bg-2); border-color: var(--border-hi); }
  .sa-dock-expand svg { width: 14px; height: 14px; }

  .sa-preview-overlay {
    position: fixed; inset: 0; z-index: 300;
    background: var(--backdrop, rgba(0,0,0,0.3));
    display: flex; justify-content: flex-end;
  }
  .sa-preview-sheet {
    width: 460px; max-width: 80vw; height: 100%;
    background: var(--bg-1);
    border-left: 1px solid var(--border);
    box-shadow: var(--shadow-3);
    display: flex; flex-direction: column;
  }
  .sa-preview-sheet > :global(*) { flex: 1; min-height: 0; }
</style>
