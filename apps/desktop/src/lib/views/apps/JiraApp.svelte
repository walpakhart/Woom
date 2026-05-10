<script lang="ts">
  /* JiraApp — full-screen workspace for Jira.
     Layout: [JiraList (resizable)] [JiraDetailPane (flex)]
     - List: standalone, reads inbox state, click → sets focusKey.
     - Detail: existing JiraDetailPane (already a standalone component
       with the comments/transitions/worklogs logic — reused as-is).
     - Splitter: width persists per-instance under
       `woom:splitter:jira-list:<instanceId>` so the user's preferred
       reading width sticks across reloads. */
  import JiraList from './jira/JiraList.svelte';
  import JiraDetailPane from '$lib/components/inbox/JiraDetailPane.svelte';
  import Splitter from '$lib/components/ui/Splitter.svelte';
  import BrandIcon from '$lib/components/ui/BrandIcon.svelte';
  import { inboxState } from '$lib/state/inbox.svelte';
  import type { JiraStatus, JiraItem } from '$lib/data';

  interface Props {
    instanceId: string;
    jiraStatus: JiraStatus;
    now: number;
    onRefresh: () => void;
    onOpenCreateIssue: () => void;
    onOpenBrowser: (url: string) => void;
    onDragStart: (payload: { source: 'jira'; item: JiraItem }, e: DragEvent) => void;
    onDragEnd: () => void;
    onCardMouseDown: (e: MouseEvent) => void;
    isClickNotDrag: (e: MouseEvent) => boolean;
    refreshAllJiraInboxes: (opts?: { silent?: boolean }) => Promise<void>;
    onSendToClaude: (item: JiraItem) => void;
    onSendToCursor: (item: JiraItem) => void;
  }
  let p: Props = $props();
</script>

<section
  class="app-shell sj-shell"
  style="--app-tone: var(--src-jira); --app-glow: rgba(79,142,255,0.40);"
>
  <Splitter
    direction="horizontal"
    fixedSide="start"
    persistKey="jira-list:{p.instanceId}"
    initial={380}
    min={280}
    max={640}
  >
    {#snippet start()}
      <JiraList
        instanceId={p.instanceId}
        jiraStatus={p.jiraStatus}
        now={p.now}
        onRefresh={p.onRefresh}
        onOpenCreateIssue={p.onOpenCreateIssue}
        onOpenBrowser={p.onOpenBrowser}
        onDragStart={p.onDragStart}
        onDragEnd={p.onDragEnd}
        onCardMouseDown={p.onCardMouseDown}
        isClickNotDrag={p.isClickNotDrag}
        onSendToClaude={p.onSendToClaude}
        onSendToCursor={p.onSendToCursor}
      />
    {/snippet}
    {#snippet end()}
      <section class="sj-detail app-pane">
        {#if inboxState.jiraFocusKey}
          {@const focusKey = inboxState.jiraFocusKey}
          <JiraDetailPane
            issueKey={focusKey}
            now={p.now}
            onClose={() => (inboxState.jiraFocusKey = null)}
            onStatusChange={() => void p.refreshAllJiraInboxes({ silent: true })}
            onSendToClaude={() => {
              const items = inboxState.jiraItemsByInstance[p.instanceId] ?? [];
              const it = items.find((x) => x.key === focusKey)
                ?? (Object.values(inboxState.jiraItemsByInstance)
                    .flat()
                    .find((x) => x.key === focusKey) as JiraItem | undefined);
              if (it) p.onSendToClaude(it);
            }}
            onSendToCursor={() => {
              const items = inboxState.jiraItemsByInstance[p.instanceId] ?? [];
              const it = items.find((x) => x.key === focusKey)
                ?? (Object.values(inboxState.jiraItemsByInstance)
                    .flat()
                    .find((x) => x.key === focusKey) as JiraItem | undefined);
              if (it) p.onSendToCursor(it);
            }}
          />
        {:else}
          <div class="app-empty">
            <div class="app-empty-icon">
              <BrandIcon kind="jira" size={28} />
            </div>
            <h2 class="app-empty-h">Pick a ticket</h2>
            <p class="app-empty-p">
              Click an item on the left to read it inline. Drop it onto a Claude
              session to hand it to the agent — the Jira workspace stays in sync.
            </p>
          </div>
        {/if}
      </section>
    {/snippet}
  </Splitter>
</section>

<style>
  /* Splitter snippets render bare into the splitter panes — give them
     space to fill via `:global` so we don't need to wrap each in a
     stretch container. The shell itself sits on the standard
     `.app-shell` chrome (set by app.css), so all we add here is the
     pane fillers + the unchanged JiraDetailPane override. */
  .sj-shell :global(.s-start),
  .sj-shell :global(.s-end) {
    height: 100%;
    display: flex;
    min-width: 0;
  }
  .sj-shell :global(.s-start) > :global(*),
  .sj-shell :global(.s-end) > :global(*) {
    flex: 1 1 auto;
    width: 100%;
    min-width: 0;
  }

  .sj-detail {
    flex: 1;
    min-width: 0;
    display: flex; flex-direction: column;
  }
  /* JiraDetailPane is normally rendered inside the `.slide-over` modal
     overlay in +page.svelte. Here it renders as the bare `.jdp` root —
     stretch it to fill the pane. */
  .sj-detail :global(.jdp) {
    flex: 1; min-height: 0;
    display: flex; flex-direction: column;
    overflow-y: auto;
  }
</style>
