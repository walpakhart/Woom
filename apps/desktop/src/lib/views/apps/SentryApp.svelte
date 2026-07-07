<script lang="ts">
  /* SentryApp — full-screen workspace for Sentry.
     Layout: [SentryList 380] [SentryDetailPane (flex)]
     Detail = the existing SentryDetailPane (events, stack frames,
     breadcrumbs, status). Rendered inline. */
  import SentryList from './sentry/SentryList.svelte';
  import SentryDetailPane from '$lib/components/inbox/SentryDetailPane.svelte';
  import Splitter from '$lib/components/ui/Splitter.svelte';
  import BrandIcon from '$lib/components/ui/BrandIcon.svelte';
  import QuietSoloHeader from './_shared/QuietSoloHeader.svelte';
  import { inboxState, sentryItemsFor } from '$lib/state/inbox.svelte';
  import { layoutModeState } from '$lib/state/layoutMode.svelte';
  import { sentryLevelClass, type SentryIssue, type SentryStatus } from '$lib/data';
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
    onFixWithDw: (item: SentryIssue) => void;
  }
  let p: Props = $props();

  const quiet = $derived(layoutModeState.mode === 'quiet');

  /* Quiet §3.4 — list → «N ▾» switcher popover. The Sentry document
     keeps its own header row (Resolve / ignore live there and only the
     detail pane owns that mutation), so the qsolo eyebrow above it is
     purely the cross-issue switcher. */
  const items = $derived(sentryItemsFor(p.instanceId));
  const focusItem = $derived(items.find((it) => it.id === inboxState.sentryFocusId) ?? null);
  const switchItems = $derived(
    items.map((it) => ({
      id: it.id,
      label: it.title,
      sub: it.short_id,
      active: it.id === inboxState.sentryFocusId
    }))
  );
  function pickIssue(id: string) {
    inboxState.sentryFocusId = id;
  }
  function sendFocusedToClaude() {
    if (focusItem) p.onSendToClaude(focusItem);
  }
  function dwFocused() {
    if (focusItem) p.onFixWithDw(focusItem);
  }
</script>

<section
  class="app-shell ssn-shell"
  class:ssn-shell--quiet={quiet}
  style="--app-tone: var(--src-sentry); --app-glow: rgba(110,80,155,0.40);"
>
  {#if quiet}
    <div class="qsolo-doc">
      <QuietSoloHeader
        count={items.length}
        noun="errors"
        items={switchItems}
        onPick={pickIssue}
        ariaLabel="Issues"
      >
        {#snippet lead()}
          {#if focusItem}
            <span class="qsolo-key mono">{focusItem.short_id}</span>
            <span class="qsolo-tag {sentryLevelClass(focusItem.level)}">{focusItem.level.toLowerCase()}</span>
            <span class="qsolo-tag">{focusItem.status.toLowerCase()}</span>
          {/if}
        {/snippet}
        {#snippet actions()}
          {#if focusItem}
            <button class="qsolo-act" onclick={() => focusItem && p.onOpenBrowser(focusItem.permalink)}>in Sentry ↗</button>
            <button class="qsolo-act qsolo-act--claude" onclick={sendFocusedToClaude}>→ claude</button>
            <button class="qsolo-act" onclick={dwFocused}>/dw</button>
          {/if}
        {/snippet}
      </QuietSoloHeader>
      {#if inboxState.sentryFocusId}
        {@const focusId = inboxState.sentryFocusId}
        <div class="qsolo-pane">
          <SentryDetailPane
            issueId={focusId}
            now={p.now}
            onClose={() => (inboxState.sentryFocusId = null)}
            onOpenBrowser={p.onOpenBrowser}
            onSendToClaude={() => {
              const it = items.find((x) => x.id === focusId)
                ?? Object.values(inboxState.sentryItemsByInstance).flat().find((x) => x.id === focusId);
              if (it) p.onSendToClaude(it);
            }}
          />
        </div>
      {:else}
        <div class="qsolo-empty">
          <h2 class="qsolo-empty-h">Pick an issue</h2>
          <p class="qsolo-empty-p">Use the «{items.length} ▾» switcher above to open one.</p>
        </div>
      {/if}
    </div>
  {:else}
  <Splitter
    direction="horizontal"
    fixedSide="start"
    persistKey="sentry-list:{p.instanceId}"
    initial={300}
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
        onFixWithDw={p.onFixWithDw}
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
  {/if}
</section>

<style>
  /* Quiet §3.4 — single centred document; list → header switcher. The
     SentryDetailPane keeps its own head (Resolve / ignore), softened to
     flow under the eyebrow. */
  .ssn-shell--quiet { display: block; overflow-y: auto; padding: 18px 40px 40px; }
  .qsolo-doc { width: 100%; max-width: 800px; margin: 0 auto; display: flex; flex-direction: column; }
  .qsolo-pane { display: flex; min-height: 0; }
  .qsolo-pane > :global(.snd) { flex: 1; min-width: 0; background: transparent; }
  .qsolo-pane :global(.snd-head) { height: auto; padding: 8px 0; border-bottom: 0; background: transparent; }
  .qsolo-pane :global(.snd-back) { display: none; }
  .qsolo-pane :global(.snd-doc) { padding: 4px 0 40px; max-width: none; }

  .qsolo-empty { padding: 64px 20px; text-align: center; }
  .qsolo-empty-h { font-size: 20px; font-weight: 600; color: var(--text-0); margin: 0 0 8px; letter-spacing: -0.015em; }
  .qsolo-empty-p { font-size: 12.5px; color: var(--text-2); margin: 0; }

  .qsolo-key { font-size: 12px; font-weight: 600; color: var(--src-sentry); }
  .qsolo-tag {
    font-size: 10.5px; color: var(--text-mute);
    padding: 1px 7px; border-radius: var(--r-chip);
    border: 1px solid var(--border-hi);
  }
  /* sentryLevelClass → tag--fatal / tag--error / tag--warning / tag--info. */
  .qsolo-tag.tag--fatal { color: #f87171; border-color: color-mix(in srgb, #f87171 45%, transparent); }
  .qsolo-tag.tag--error { color: var(--error); border-color: color-mix(in srgb, var(--error) 45%, transparent); }
  .qsolo-tag.tag--warning { color: #D9B86E; border-color: color-mix(in srgb, #D9B86E 45%, transparent); }
  .qsolo-tag.tag--info { color: #60a5fa; border-color: color-mix(in srgb, #60a5fa 45%, transparent); }

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
  /* SentryDetailPane root = `.snd` — stretch it to fill the pane. */
  .ssn-detail :global(.snd) {
    flex: 1; min-height: 0;
    display: flex; flex-direction: column;
    overflow-y: auto;
  }
</style>
