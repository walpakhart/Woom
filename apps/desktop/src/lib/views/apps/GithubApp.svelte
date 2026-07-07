<script lang="ts">
  /* GithubApp — full-screen workspace for GitHub.
     Layout: [GithubList 380] [GithubFocusOverlay (flex)].
     When focusItem === null — editorial empty state. */
  import GithubList from './github/GithubList.svelte';
  import GithubFocusOverlay from '$lib/components/inbox/GithubFocusOverlay.svelte';
  import Splitter from '$lib/components/ui/Splitter.svelte';
  import QuietSoloHeader from './_shared/QuietSoloHeader.svelte';
  import { inboxState, githubItemsFor, openFocusItem } from '$lib/state/inbox.svelte';
  import { layoutModeState } from '$lib/state/layoutMode.svelte';
  import { externalId, kindLabel, repoLabel, type ConnectionStatus, type InboxItem, type CommitEntry } from '$lib/data';
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
    onFixWithDw: (item: InboxItem) => void;
  }
  let p: Props = $props();

  const quiet = $derived(layoutModeState.mode === 'quiet');

  /* Quiet §3.4 — list → «N ▾» switcher popover; focused PR = document. */
  const items = $derived(githubItemsFor(p.instanceId));
  const focusItem = $derived(inboxState.focusItem);
  const switchItems = $derived(
    items.map((it) => ({
      id: String(it.id),
      label: it.title,
      sub: externalId(it),
      active: it.id === inboxState.focusItem?.id
    }))
  );
  function pickItem(id: string) {
    const it = items.find((x) => String(x.id) === id);
    if (it) openFocusItem(it);
  }
  function sendFocusedToClaude() {
    if (inboxState.focusItem) p.onSendToClaude(inboxState.focusItem);
  }
</script>

<section
  class="app-shell sg-shell"
  class:sg-shell--quiet={quiet}
  style="--app-tone: var(--src-github); --app-glow: rgba(181,132,255,0.40);"
>
  {#if quiet}
    <div class="qsolo-doc">
      <QuietSoloHeader
        count={items.length}
        noun="PR"
        items={switchItems}
        onPick={pickItem}
        ariaLabel="Pull requests"
      >
        {#snippet lead()}
          {#if focusItem}
            <span class="qsolo-tag qsolo-tag--{focusItem.state}">{focusItem.state}</span>
            <span class="qsolo-key mono">{externalId(focusItem)}</span>
            <span class="qsolo-sub mono">{repoLabel(focusItem)}</span>
            <span class="qsolo-sub">{kindLabel(focusItem).toLowerCase()}</span>
          {/if}
        {/snippet}
        {#snippet actions()}
          {#if focusItem}
            <button class="qsolo-act" onclick={() => focusItem && p.onOpenBrowser(focusItem.url)}>on GitHub ↗</button>
            <button class="qsolo-act qsolo-act--claude" onclick={sendFocusedToClaude}>→ claude</button>
          {/if}
        {/snippet}
      </QuietSoloHeader>
      {#if inboxState.focusItem}
        <div class="qsolo-pane">
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
            onSendToClaude={sendFocusedToClaude}
          />
        </div>
      {:else}
        <div class="qsolo-empty">
          <h2 class="qsolo-empty-h">Pick a pull request</h2>
          <p class="qsolo-empty-p">Use the «{items.length} ▾» switcher above to open one.</p>
        </div>
      {/if}
    </div>
  {:else}
  <Splitter
    direction="horizontal"
    fixedSide="start"
    persistKey="github-list:{p.instanceId}"
    initial={300}
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
        onFixWithDw={p.onFixWithDw}
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
  {/if}
</section>

<style>
  /* Quiet §3.4 — single centred document; list → header switcher. */
  .sg-shell--quiet { display: block; overflow-y: auto; padding: 18px 40px 40px; }
  .qsolo-doc { width: 100%; max-width: 800px; margin: 0 auto; display: flex; flex-direction: column; }
  .qsolo-pane { display: flex; min-height: 0; }
  .qsolo-pane > :global(.gfo) { flex: 1; min-width: 0; background: transparent; }
  .qsolo-pane :global(.gfo-head) { display: none; }

  .qsolo-key { font-size: 12px; font-weight: 600; color: var(--src-github); }
  .qsolo-sub { font-size: 11px; color: var(--text-mute); }
  .qsolo-tag {
    font-size: 10.5px; color: var(--text-mute);
    padding: 1px 7px; border-radius: var(--r-chip);
    border: 1px solid var(--border-hi); text-transform: lowercase;
  }
  .qsolo-tag--open { color: var(--ok); border-color: var(--ok-border); }
  .qsolo-tag--merged { color: var(--src-github); border-color: var(--src-github-border, var(--border-hi)); }
  .qsolo-tag--closed { color: var(--err); border-color: var(--err-border); }

  .qsolo-act {
    background: transparent; border: 0; cursor: pointer;
    font-size: 12.5px; color: var(--text-1);
    padding: 0 0 1px;
    border-bottom: 1px dotted color-mix(in srgb, var(--text-1) 40%, transparent);
  }
  .qsolo-act:hover { color: var(--text-0); border-bottom-color: var(--text-0); }
  .qsolo-act--claude { color: var(--src-claude); border-bottom-color: color-mix(in srgb, var(--src-claude) 45%, transparent); }
  .qsolo-act--claude:hover { color: var(--accent-bright); border-bottom-color: var(--accent-bright); }

  .qsolo-empty { padding: 64px 20px; text-align: center; }
  .qsolo-empty-h { font-size: 20px; font-weight: 600; color: var(--text-0); margin: 0 0 8px; letter-spacing: -0.015em; }
  .qsolo-empty-p { font-size: 12.5px; color: var(--text-2); margin: 0; }

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
