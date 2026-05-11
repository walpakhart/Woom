<script lang="ts">
  /* GithubApp — full-screen workspace для GitHub.
     Layout: [GithubList 380] [GithubFocusOverlay (flex)].
     Когда focusItem === null — editorial empty state. */
  import GithubList from './github/GithubList.svelte';
  import GithubFocusOverlay from '$lib/components/inbox/GithubFocusOverlay.svelte';
  import Splitter from '$lib/components/ui/Splitter.svelte';
  import { inboxState } from '$lib/state/inbox.svelte';
  import type { ConnectionStatus, InboxItem, CommitEntry } from '$lib/data';
  import type { DetailTab } from '$lib/state/view.svelte';

  interface Props {
    instanceId: string;
    githubStatus: ConnectionStatus;
    now: number;
    tab: DetailTab;
    actionBusy: string | null;
    onSelect: (id: number) => void;
    onRefresh: () => void;
    onOpenCreatePr: () => void;
    onTabChange: (t: DetailTab) => void;
    onToggleFile: (filename: string) => void;
    onRetryLoadDetail: () => void;
    onOpenCommit: (c: CommitEntry) => void;
    onOpenComment: () => void;
    onOpenReview: () => void;
    onOpenMerge: () => void;
    onAskClose: () => void;
    onReopen: () => void;
    onOpenBrowser: (url: string) => void;
    onOpenCheckDetails: (url: string) => void;
    onCloseFocus: () => void;
    mergeDisabled: () => boolean;
    onDragStart: (payload: { source: 'github'; item: InboxItem }, e: DragEvent) => void;
    onDragEnd: () => void;
    onCardMouseDown: (e: MouseEvent) => void;
    isClickNotDrag: (e: MouseEvent) => boolean;
    onSendToClaude: (item: InboxItem) => void;
    onSendToCursor: (item: InboxItem) => void;
  }
  let p: Props = $props();
</script>

<section
  class="app-shell sg-shell"
  style="--app-tone: var(--src-github); --app-glow: rgba(181,132,255,0.40);"
>
  <Splitter
    direction="horizontal"
    fixedSide="start"
    persistKey="github-list:{p.instanceId}"
    initial={380}
    min={280}
    max={640}
  >
    {#snippet start()}
      <GithubList
        instanceId={p.instanceId}
        githubStatus={p.githubStatus}
        now={p.now}
        onRefresh={p.onRefresh}
        onOpenCreatePr={p.onOpenCreatePr}
        onOpenBrowser={p.onOpenBrowser}
        onSelect={p.onSelect}
        onDragStart={p.onDragStart}
        onDragEnd={p.onDragEnd}
        onCardMouseDown={p.onCardMouseDown}
        isClickNotDrag={p.isClickNotDrag}
        onSendToClaude={p.onSendToClaude}
        onSendToCursor={p.onSendToCursor}
      />
    {/snippet}
    {#snippet end()}
      <section class="sg-detail app-pane">
        {#if inboxState.focusItem}
          <GithubFocusOverlay
            now={p.now}
            tab={p.tab}
            actionBusy={p.actionBusy}
            onCloseFocus={p.onCloseFocus}
            onRetryLoadDetail={p.onRetryLoadDetail}
            onTabChange={p.onTabChange}
            onToggleFile={p.onToggleFile}
            onOpenCommit={p.onOpenCommit}
            onOpenComment={p.onOpenComment}
            onOpenReview={p.onOpenReview}
            onOpenMerge={p.onOpenMerge}
            onAskClose={p.onAskClose}
            onReopen={p.onReopen}
            onOpenBrowser={p.onOpenBrowser}
            onOpenCheckDetails={p.onOpenCheckDetails}
            mergeDisabled={p.mergeDisabled}
            onSendToClaude={() => {
              if (inboxState.focusItem) p.onSendToClaude(inboxState.focusItem);
            }}
            onSendToCursor={() => {
              if (inboxState.focusItem) p.onSendToCursor(inboxState.focusItem);
            }}
          />
        {:else}
          <div class="app-empty">
            <div class="app-empty-icon">
              <svg viewBox="0 0 24 24" fill="currentColor"><path d="M12 2a10 10 0 0 0-3.16 19.49c.5.09.68-.22.68-.48l-.01-1.7c-2.78.6-3.37-1.34-3.37-1.34-.46-1.16-1.12-1.47-1.12-1.47-.91-.62.07-.61.07-.61 1.01.07 1.54 1.04 1.54 1.04.9 1.53 2.36 1.09 2.93.83.09-.65.35-1.09.63-1.34-2.22-.25-4.55-1.11-4.55-4.94 0-1.09.39-1.99 1.03-2.69-.1-.25-.45-1.27.1-2.65 0 0 .84-.27 2.75 1.03A9.6 9.6 0 0 1 12 6.84c.85.004 1.7.115 2.5.336 1.91-1.3 2.75-1.03 2.75-1.03.55 1.38.2 2.4.1 2.65.64.7 1.03 1.6 1.03 2.69 0 3.84-2.34 4.69-4.57 4.93.36.31.68.92.68 1.85l-.01 2.74c0 .27.18.58.69.48A10 10 0 0 0 12 2z"/></svg>
            </div>
            <h2 class="app-empty-h">Pick a pull request</h2>
            <p class="app-empty-p">
              Click a PR or issue on the left to see checks, files, and conversation
              inline. Drop it onto a Claude session to start a fix.
            </p>
          </div>
        {/if}
      </section>
    {/snippet}
  </Splitter>
</section>

<style>
  .sg-shell :global(.s-start),
  .sg-shell :global(.s-end) {
    height: 100%;
    display: flex;
    min-width: 0;
  }
  .sg-shell :global(.s-start) > :global(*),
  .sg-shell :global(.s-end) > :global(*) {
    flex: 1 1 auto;
    width: 100%;
    min-width: 0;
  }

  .sg-detail {
    flex: 1;
    min-width: 0;
    display: flex; flex-direction: column;
  }
</style>
