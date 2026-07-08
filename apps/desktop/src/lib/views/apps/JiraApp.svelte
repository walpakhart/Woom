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
  import QuietSoloHeader from './_shared/QuietSoloHeader.svelte';
  import { inboxState, jiraItemsFor } from '$lib/state/inbox.svelte';
  import { layoutModeState } from '$lib/state/layoutMode.svelte';
  import { jiraStatusClass, type JiraStatus, type JiraItem } from '$lib/data';

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
    onFixWithDw: (item: JiraItem) => void;
  }
  let p: Props = $props();

  const quiet = $derived(layoutModeState.mode === 'quiet');

  /* Quiet §3.4 — the list panel collapses into a «N ▾» switcher popover;
     the focused ticket becomes a single centred document. */
  const items = $derived(jiraItemsFor(p.instanceId));
  const focusItem = $derived(items.find((it) => it.key === inboxState.jiraFocusKey) ?? null);
  const switchItems = $derived(
    items.map((it) => ({ id: it.key, label: it.summary || it.key, sub: it.key, active: it.key === inboxState.jiraFocusKey }))
  );
  function pickTicket(key: string) {
    inboxState.jiraFocusKey = key;
  }
  function sendFocusedToClaude() {
    if (focusItem) p.onSendToClaude(focusItem);
  }
  function dwFocused() {
    if (focusItem) p.onFixWithDw(focusItem);
  }
</script>

<section
  class="app-shell sj-shell"
  class:sj-shell--quiet={quiet}
  style="--app-tone: var(--src-jira); --app-glow: rgba(79,142,255,0.40);"
>
  {#if quiet}
    <div class="qsolo-doc">
      <QuietSoloHeader
        count={items.length}
        noun="tickets"
        items={switchItems}
        onPick={pickTicket}
        ariaLabel="Tickets"
      >
        {#snippet panel(close)}
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
            onFixWithDw={p.onFixWithDw}
            onNavigate={close}
          />
        {/snippet}
        {#snippet lead()}
          {#if focusItem}
            <span class="qsolo-key mono">{focusItem.key}</span>
            <span class="qsolo-tag">{focusItem.issue_type.toLowerCase()}</span>
            <span class="qsolo-tag {jiraStatusClass(focusItem.status_category)}">{focusItem.status.toLowerCase()}</span>
          {/if}
        {/snippet}
        {#snippet actions()}
          {#if focusItem}
            <button class="qsolo-act" onclick={() => focusItem && p.onOpenBrowser(focusItem.url)}>in Jira ↗</button>
            <button class="qsolo-act qsolo-act--claude" onclick={sendFocusedToClaude}>→ claude</button>
            <button class="qsolo-act" onclick={dwFocused}>/dw</button>
          {/if}
        {/snippet}
      </QuietSoloHeader>
      {#if inboxState.jiraFocusKey}
        {@const focusKey = inboxState.jiraFocusKey}
        <div class="qsolo-pane">
          <JiraDetailPane
            issueKey={focusKey}
            now={p.now}
            onClose={() => (inboxState.jiraFocusKey = null)}
            onStatusChange={() => void p.refreshAllJiraInboxes({ silent: true })}
            onSendToClaude={sendFocusedToClaude}
          />
        </div>
      {:else}
        <div class="qsolo-empty">
          <h2 class="qsolo-empty-h">Pick a ticket</h2>
          <p class="qsolo-empty-p">Use the «{items.length} ▾» switcher above to open one.</p>
        </div>
      {/if}
    </div>
  {:else}
  <Splitter
    direction="horizontal"
    fixedSide="start"
    persistKey="jira-list:{p.instanceId}"
    initial={300}
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
        onFixWithDw={p.onFixWithDw}
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
  {/if}
</section>

<style>
  /* Quiet §3.4 — single centred document; the list panel is gone,
     replaced by the header's «N ▾» switcher popover. */
  .sj-shell--quiet { display: block; overflow-y: auto; padding: 18px 40px 40px; }
  .qsolo-doc { width: 100%; max-width: 800px; margin: 0 auto; display: flex; flex-direction: column; }
  .qsolo-pane { display: flex; min-height: 0; }
  .qsolo-pane > :global(.jrd) { flex: 1; min-width: 0; background: transparent; }
  /* The Cabin 52px toolbar head is replaced by the qsolo eyebrow above. */
  .qsolo-pane :global(.jrd-head) { display: none; }
  .qsolo-pane :global(.jrd-doc) { padding: 4px 0 40px; max-width: none; }

  .qsolo-key { font-size: 12px; font-weight: 600; color: var(--src-jira); }
  .qsolo-tag {
    font-size: 10.5px; color: var(--text-mute);
    padding: 1px 7px; border-radius: var(--r-chip);
    border: 1px solid var(--border-hi);
  }
  /* jiraStatusClass → tag--open (to-do) / tag--draft (in progress) / tag--closed (done). */
  .qsolo-tag.tag--open { color: var(--src-jira); border-color: var(--src-jira-border); }
  .qsolo-tag.tag--draft { color: var(--warn); border-color: color-mix(in srgb, var(--warn) 45%, transparent); }
  .qsolo-tag.tag--closed { color: var(--ok); border-color: var(--ok-border); }

  /* Solo actions — dotted-underline links (§3.4). */
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
  /* JiraDetailPane root = `.jrd` — stretch it to fill the pane. */
  .sj-detail :global(.jrd) {
    flex: 1; min-height: 0;
    display: flex; flex-direction: column;
    overflow-y: auto;
  }
</style>
