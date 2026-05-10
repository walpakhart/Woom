<script lang="ts">
  /* SentryApp — full-screen workspace для Sentry.
     Layout: [SentryList 380] [SentryDetailPane (flex)]
     Detail = существующий SentryDetailPane (events, stack frames,
     breadcrumbs, status). Рендерится inline. */
  import SentryList from './sentry/SentryList.svelte';
  import SentryDetailPane from '$lib/components/inbox/SentryDetailPane.svelte';
  import Splitter from '$lib/components/ui/Splitter.svelte';
  import BrandIcon from '$lib/components/ui/BrandIcon.svelte';
  import { inboxState } from '$lib/state/inbox.svelte';
  import type { SentryIssue, SentryStatus } from '$lib/data';
  import type { DragPayload } from '$lib/state/drag.svelte';

  interface Props {
    instanceId: string;
    sentryStatus: SentryStatus;
    now: number;
    onOpenBrowser: (url: string) => void;
    onDragStart: (payload: DragPayload, e: DragEvent) => void;
    onDragEnd: () => void;
    onCardMouseDown: (e: MouseEvent) => void;
    isClickNotDrag: (e: MouseEvent) => boolean;
    onSendToClaude: (item: SentryIssue) => void;
    onSendToCursor: (item: SentryIssue) => void;
  }
  let p: Props = $props();
</script>

<section
  class="app-shell ssn-shell"
  style="--app-tone: var(--src-sentry); --app-glow: rgba(110,80,155,0.40);"
>
  <Splitter
    direction="horizontal"
    fixedSide="start"
    persistKey="sentry-list:{p.instanceId}"
    initial={380}
    min={280}
    max={640}
  >
    {#snippet start()}
      <SentryList
        instanceId={p.instanceId}
        sentryStatus={p.sentryStatus}
        now={p.now}
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
      <section class="ssn-detail app-pane">
        {#if inboxState.sentryFocusId}
          {@const focusId = inboxState.sentryFocusId}
          <SentryDetailPane
            issueId={focusId}
            now={p.now}
            onClose={() => (inboxState.sentryFocusId = null)}
            onOpenBrowser={p.onOpenBrowser}
            onSendToClaude={() => {
              const items = inboxState.sentryItemsByInstance[p.instanceId] ?? [];
              const it = items.find((x) => x.id === focusId)
                ?? Object.values(inboxState.sentryItemsByInstance).flat().find((x) => x.id === focusId);
              if (it) p.onSendToClaude(it);
            }}
            onSendToCursor={() => {
              const items = inboxState.sentryItemsByInstance[p.instanceId] ?? [];
              const it = items.find((x) => x.id === focusId)
                ?? Object.values(inboxState.sentryItemsByInstance).flat().find((x) => x.id === focusId);
              if (it) p.onSendToCursor(it);
            }}
          />
        {:else}
          <div class="app-empty">
            <div class="app-empty-icon">
              <BrandIcon kind="sentry" size={28} />
            </div>
            <h2 class="app-empty-h">Pick an issue</h2>
            <p class="app-empty-p">
              Click an error on the left to read its stack trace and breadcrumbs
              inline. Drop it onto a Claude session to start a fix.
            </p>
          </div>
        {/if}
      </section>
    {/snippet}
  </Splitter>
</section>

<style>
  .ssn-shell :global(.s-start),
  .ssn-shell :global(.s-end) {
    height: 100%;
    display: flex;
    min-width: 0;
  }
  .ssn-shell :global(.s-start) > :global(*),
  .ssn-shell :global(.s-end) > :global(*) {
    flex: 1 1 auto;
    width: 100%;
    min-width: 0;
  }

  .ssn-detail {
    flex: 1;
    min-width: 0;
    display: flex; flex-direction: column;
  }
  /* SentryDetailPane root = `.sdp` — stretch it to fill the pane. */
  .ssn-detail :global(.sdp) {
    flex: 1; min-height: 0;
    display: flex; flex-direction: column;
    overflow-y: auto;
  }
</style>
