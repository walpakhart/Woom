<script lang="ts">
  /* JiraApp — full-screen workspace для Jira.
     Layout: [JiraList 380] [JiraDetailPane (flex)]
     - List: standalone, читает inbox state, click → setает focusKey
     - Detail: используем существующий JiraDetailPane (он уже
       standalone-компонент с богатой logic'ой comments/transitions/
       worklogs — переписывать заново нет смысла). CSS-override в
       app.css делает .slide-over (его обёртка) inline-friendly. */
  import JiraList from './jira/JiraList.svelte';
  import JiraDetailPane from '$lib/components/inbox/JiraDetailPane.svelte';
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
  class="app-shell"
  style="--app-tone: var(--src-jira); --app-glow: rgba(79,142,255,0.40); grid-template-columns: 380px 1fr;"
>
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

  <section class="sj-detail app-pane">
    {#if inboxState.jiraFocusKey}
      {@const focusKey = inboxState.jiraFocusKey}
      <JiraDetailPane
        issueKey={focusKey}
        now={p.now}
        onClose={() => (inboxState.jiraFocusKey = null)}
        onStatusChange={() => void p.refreshAllJiraInboxes({ silent: true })}
        onSendToClaude={() => {
          /* Resolve the focused ticket out of the Jira-per-instance
             inbox slice — `inboxState.focusItem` only tracks GitHub
             focus, and Jira tickets live in their own map keyed by
             instance id. Fallback to a global flatten so cross-
             instance focus still works. */
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
          <svg viewBox="0 0 24 24" fill="currentColor"><path d="M11.53 2L11.53 11.5Q11.53 13 13 13H22V11.5Q22 2 11.53 2ZM5 7.5V17Q5 18.5 6.5 18.5H15.5V17Q15.5 7.5 5 7.5Z"/></svg>
        </div>
        <h2 class="app-empty-h">Pick a ticket</h2>
        <p class="app-empty-p">
          Click an item on the left to read it inline. Drop it onto a Claude
          session to hand it to the agent — the Jira workspace stays in sync.
        </p>
      </div>
    {/if}
  </section>
</section>

<style>
  .sj-detail {
    flex: 1;
    min-width: 0;
    display: flex; flex-direction: column;
  }
  /* JiraDetailPane не использует .slide-over wrap (он только внутри
     модального overlay в +page.svelte). Здесь рендерится сразу как
     корневой .jdp элемент — растягиваем его на весь pane. */
  .sj-detail :global(.jdp) {
    flex: 1; min-height: 0;
    display: flex; flex-direction: column;
    overflow-y: auto;
  }
</style>
