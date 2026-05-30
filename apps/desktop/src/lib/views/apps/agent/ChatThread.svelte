<script lang="ts">
  /* ChatThread — message list + action cards for the active session.
     v7: 76px byline column ("@you" / italic-serif "claude") on left,
     message body on right. User messages get a soft bg + 2px clay
     accent stripe. Assistant messages are unbubbled prose with inline
     trace pills, edit cards (editor-stripe), and action cards (per-kind
     stripe — github for PR, etc.). */
  import { sessionsState } from '$lib/state/sessions.svelte';
  import { convertFileSrc, invoke } from '@tauri-apps/api/core';
  import Markdown from '$lib/components/ui/Markdown.svelte';
  import ClaudeActionCard from '$lib/components/agent/ClaudeActionCard.svelte';
  import QuestionCard from '$lib/components/agent/QuestionCard.svelte';
  import SddCard from '$lib/components/agent/SddCard.svelte';
  import DynamicWorkflowCard from '$lib/components/agent/DynamicWorkflowCard.svelte';
  import ResumePill from '$lib/components/agent/ResumePill.svelte';
  import { workspaceForSession, isSddCardHidden } from '$lib/state/sdd.svelte';
  import { activeWorkflowForSession, isWorkflowActive } from '$lib/state/dw.svelte';
  import CardContextMenu, { type MenuItem } from '$lib/views/apps/_shared/CardContextMenu.svelte';
  import { notify } from '$lib/state/toaster.svelte';
  import { setDragPayload } from '$lib/state/drag.svelte';
  import { attachDragChip } from '$lib/dragImage';
  import type { ClaudeAction, ClaudeMessage } from '$lib/types';
  import { parseToolHint, parseTraceSegment, type ToolHint, type ToolKind } from './chatTraceParse';
  import { computeDiffRows, diffStats, type DiffRow } from './chatDiff';

  type Kind = 'claude' | 'cursor';

  interface Props {
    kind: Kind;
    thinkingStartedAt: Record<string, number | null>;
    thinkingTick: Record<string, number>;
    onUpdateAction: (sessionId: string, actionId: string, patch: Partial<ClaudeAction>) => void;
    onRemoveAction: (sessionId: string, actionId: string) => void;
    onExecuteAction: (sessionId: string, action: ClaudeAction) => void;
    onOpenPrInWoom: (url: string, action: (ClaudeAction & { kind: 'pr' }) | null) => void;
    onStartEditMessage: (sessionId: string, index: number, content: string) => void;
    onResendMessage: (sessionId: string, index: number, content: string) => void;
    /** Click on a file-like reference inside a rendered chat bubble —
     *  the parent owns the resolution against the session's cwd /
     *  worktree / linked editor, so all we have to do here is plumb
     *  the path through. */
    onOpenFile?: (path: string) => void;
    /** SDD card "advance" → stamp the next prompt into composer + fire
     *  the same send pipeline a manual user message uses. Wired up from
     *  +page.svelte; null when the parent hasn't plumbed it (e.g. tests). */
    onSddAdvance?: (sessionId: string, prompt: string) => void;
    onDwVerify?: (workflowId: string) => void;
    /** Quota-resume click (SDD Phase 2). Drains the session's
     *  pendingQueue[0] and fires `sendClaudeMessage`. Owned by the
     *  parent so ResumePill stays decoupled from the send-pipeline. */
    onResumeAfterQuota?: (sessionId: string) => void;
  }
  let p: Props = $props();

  const sess = $derived(
    sessionsState.list.find((s) => s.id === sessionsState.activeIds[p.kind]) ?? null
  );

  /* Render window. Long conversations (100+ messages with rich
   *  markdown + tool traces) brought chat-switch animation to a
   *  crawl — Markdown + syntax-highlight compile cost scales linearly
   *  with rendered message count. Show the last `visibleCount` of
   *  the *non-hidden* messages by default; "Show N earlier" button
   *  unfurls more in WINDOW_STEP chunks. Budgeted by non-hidden so
   *  SDD-heavy sessions (every phase prompt + response is `hidden`
   *  orchestration traffic) don't end up with a window of all-hidden
   *  entries and a blank chat. Reset on session switch. */
  const WINDOW_STEP = 30;
  let visibleCount = $state(WINDOW_STEP);
  $effect(() => {
    void sess?.id;
    visibleCount = WINDOW_STEP;
  });
  /* All five window metrics — nonHiddenTotal / windowStart /
     visibleNonHiddenCount / hiddenAboveCount / lastVisibleIndex —
     are derived from the same `sess.messages` array. Five separate
     `$derived.by` loops over the full list (one per metric) was a
     hot spot during streaming: every token delta produced a new
     `sessionsState.list` reference, all five derivations re-ran,
     each walking 100+ messages. Folding them into ONE pass cuts
     re-derive cost by 5×. Single backward walk fills tailIndex
     (lastVisibleIndex) + collects up to `visibleCount` non-hidden
     entries to fix windowStart; a quick forward sweep from
     windowStart finishes visibleNonHiddenCount. */
  const windowMetrics = $derived.by(() => {
    const msgs = sess?.messages ?? [];
    let nonHiddenTotal = 0;
    let lastVisibleIndex = -1;
    let windowStart = 0;
    let seenFromTail = 0;
    let foundWindowStart = false;
    for (let i = msgs.length - 1; i >= 0; i--) {
      if (msgs[i].hidden) continue;
      if (lastVisibleIndex === -1) lastVisibleIndex = i;
      nonHiddenTotal++;
      if (!foundWindowStart) {
        seenFromTail++;
        if (seenFromTail >= visibleCount) {
          windowStart = i;
          foundWindowStart = true;
        }
      }
    }
    if (!foundWindowStart) windowStart = 0;
    let visibleNonHiddenCount = 0;
    for (let i = windowStart; i < msgs.length; i++) {
      if (!msgs[i].hidden) visibleNonHiddenCount++;
    }
    return {
      nonHiddenTotal,
      lastVisibleIndex,
      windowStart,
      visibleNonHiddenCount,
      hiddenAboveCount: nonHiddenTotal - visibleNonHiddenCount,
    };
  });
  const nonHiddenTotal = $derived(windowMetrics.nonHiddenTotal);
  const windowStart = $derived(windowMetrics.windowStart);
  const visibleNonHiddenCount = $derived(windowMetrics.visibleNonHiddenCount);
  const hiddenAboveCount = $derived(windowMetrics.hiddenAboveCount);
  const lastVisibleIndex = $derived(windowMetrics.lastVisibleIndex);
  const visibleMessages = $derived(sess?.messages.slice(windowStart) ?? []);

  const elapsed = $derived.by(() => {
    const startedAt = sess ? p.thinkingStartedAt[sess.id] ?? null : null;
    if (!startedAt || !sess?.sending) return '';
    void (sess ? p.thinkingTick[sess.id] : 0);
    const ms = Date.now() - startedAt;
    const s = Math.floor(ms / 1000);
    return s < 60 ? `${s}s` : `${Math.floor(s / 60)}m ${String(s % 60).padStart(2, '0')}s`;
  });

  const repoCwd = $derived(sess?.worktreePath ?? sess?.cwd ?? null);

  /* Viewport-based lazy mount. Long chats (100+ messages with rich
     Markdown + many trace events per assistant turn) used to render
     every body upfront — Markdown parse + syntax decorate is the
     dominant cost when opening / switching to a long session. We
     observe each <article> with IntersectionObserver (rootMargin
     extended ±1200px so scroll feels smooth) and only mount the
     heavy body content once an article has intersected the viewport
     at least once. Sticky: once an article has been seen it stays
     mounted forever (avoids height-shift jank on scroll-up). The
     last few messages near the tail (lastVisibleIndex - FRESH_TAIL)
     are always eager so the actively streaming reply + recent
     context render immediately on chat open. */
  const FRESH_TAIL = 5;
  let visibleArticleSet = $state(new Set<number>());
  const articleObserver: IntersectionObserver | null = (() => {
    if (typeof IntersectionObserver === 'undefined') return null;
    return new IntersectionObserver(
      (entries) => {
        let added = false;
        const next = new Set(visibleArticleSet);
        for (const entry of entries) {
          if (!entry.isIntersecting) continue;
          const ds = (entry.target as HTMLElement).dataset.msgIdx;
          if (!ds) continue;
          const idx = Number(ds);
          if (!Number.isFinite(idx) || idx < 0) continue;
          if (!next.has(idx)) {
            next.add(idx);
            added = true;
          }
        }
        if (added) visibleArticleSet = next;
      },
      { rootMargin: '1200px 0px 1200px 0px', threshold: 0 }
    );
  })();
  /* Reset the seen-set when switching sessions; the indices belong to
     the previous session's `messages` array and would otherwise leak
     across so e.g. "I've already seen index 47" prevents the new
     session's index-47 message from being treated as off-viewport. */
  $effect(() => {
    void sess?.id;
    visibleArticleSet = new Set<number>();
  });
  function observeArticle(node: HTMLElement, idx: number) {
    node.dataset.msgIdx = String(idx);
    if (articleObserver) articleObserver.observe(node);
    return {
      update(newIdx: number) {
        node.dataset.msgIdx = String(newIdx);
      },
      destroy() {
        if (articleObserver) articleObserver.unobserve(node);
      },
    };
  }
  function shouldRenderBody(absIdx: number): boolean {
    if (!articleObserver) return true; // SSR / no-IO fallback — render all
    if (visibleArticleSet.has(absIdx)) return true;
    if (absIdx >= lastVisibleIndex - FRESH_TAIL) return true;
    return false;
  }

  /* Right-click context menu on chat messages. Captures the message
     + its index so action closures can address it after the menu
     opens. Save-as-memory and Copy work on any message; Edit / Resend
     route through the parent's existing user-message handlers. */
  let ctxCoords = $state<{ x: number; y: number } | null>(null);
  let ctxMsg = $state<{ msg: ClaudeMessage; index: number } | null>(null);

  /* Drag a chat message → Canvas drop target. CanvasSurface already
     accepts `source: 'chat-message'` payloads and turns them into a
     `chat-message-card` shape (see CanvasSurface.svelte:1247). The
     snapshot we attach is the same minimal shape the Canvas card
     uses as a fallback when the live session disappears — title +
     ~200-char excerpt + role + at. */
  function onMsgDragStart(e: DragEvent, msg: ClaudeMessage, index: number): void {
    if (!sess || !e.dataTransfer) return;
    /* Stop the dragstart from bubbling to ancestor drag handlers
       (e.g. composer / chat scroll). The handle itself is the source;
       no parent should intercept. */
    e.stopPropagation();
    const excerpt = msg.content.replace(/\s+/g, ' ').trim().slice(0, 200);
    setDragPayload({
      source: 'chat-message',
      sessionId: sess.id,
      messageIndex: index,
      snapshot: {
        role: msg.role,
        agentKind: sess.agentKind,
        sessionTitle: sess.title || 'Untitled chat',
        excerpt,
        at: msg.at
      }
    });
    /* setData is required for WebKit drag-over to even fire on
       cross-solo drops; the actual payload is in dragState because
       custom mimes get hidden behind whitelist filtering during
       dragover (see drag.svelte for the rationale). */
    e.dataTransfer.setData('text/plain', excerpt);
    e.dataTransfer.effectAllowed = 'copy';
    /* Custom drag chip — same compact pill the inbox sources use, so
       dropping a chat message reads as "this is what I'm moving",
       not "the entire article is following the cursor". Kind picked
       by agent so tint matches the source's brand accent. */
    const role = msg.role === 'user' ? '@you' : sess.agentKind;
    const label = `${role} · ${excerpt.slice(0, 60)}${excerpt.length > 60 ? '…' : ''}`;
    attachDragChip(e, sess.agentKind === 'cursor' ? 'cursor' : 'claude', label);
  }
  function onMsgDragEnd(): void {
    setDragPayload(null);
  }

  function openMsgCtxMenu(e: MouseEvent, msg: ClaudeMessage, index: number) {
    e.preventDefault();
    e.stopPropagation();
    /* Ignore on input/button targets — context menus inside
       interactive children belong to them, not the message wrapper. */
    const t = e.target as HTMLElement | null;
    if (t?.closest('input, textarea, button, [contenteditable="true"]')) return;
    ctxCoords = { x: e.clientX, y: e.clientY };
    ctxMsg = { msg, index };
  }
  function closeMsgCtxMenu() {
    ctxCoords = null;
    ctxMsg = null;
  }

  const ctxItems = $derived.by<MenuItem[]>(() => {
    const entry = ctxMsg;
    if (!entry || !sess) return [];
    const { msg, index } = entry;
    const items: MenuItem[] = [];
    /* Save the message body to long-term memory. Tagged so the user
       can trace which session it came from + filter to "chat-saved"
       entries in the Settings browser. */
    items.push({
      label: 'Save to memory',
      icon: 'M19 21l-7-5-7 5V5a2 2 0 0 1 2-2h10a2 2 0 0 1 2 2z',
      onClick: async () => {
        try {
          await invoke<number>('memory_save_local', {
            content: msg.content,
            kind: 'note',
            tags: [
              'from-chat',
              `session:${sess.id.slice(0, 8)}`,
              `role:${msg.role}`
            ]
          });
          notify({ kind: 'success', title: 'Saved to memory', ttlMs: 2200 });
        } catch (err) {
          notify({ kind: 'error', title: 'Memory save failed', body: String(err) });
        }
      }
    });
    items.push({
      label: 'Copy text',
      icon: 'M16 4h2a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h2 M9 2h6a1 1 0 0 1 1 1v2H8V3a1 1 0 0 1 1-1z',
      onClick: async () => {
        try { await navigator.clipboard.writeText(msg.content); }
        catch (e) { console.warn('clipboard', e); }
      },
      shortcut: '⌘C'
    });
    if (msg.role === 'user') {
      items.push({
        label: 'Edit + resend',
        icon: 'M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7 M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4z',
        onClick: () => p.onStartEditMessage(sess.id, index, msg.content)
      });
      items.push({
        label: 'Resend',
        icon: 'M21 12a9 9 0 1 1-3-6.7 M21 4v5h-5',
        onClick: () => p.onResendMessage(sess.id, index, msg.content)
      });
    }
    return items;
  });

  let chatEl: HTMLDivElement | null = $state(null);
  $effect(() => {
    if (!chatEl) return;
    const sessId = sess?.id;
    if (sessId) {
      sessionsState.scrollEls[`solo:${sessId}`] = chatEl;
    }
  });
  /* Auto-scroll on new content, but only when the user is already
     anchored near the bottom — otherwise we'd yank them away from
     scrolled-up history every time the agent streams a chunk.
     Streaming triggers this effect on every char delta, so we
     coalesce all writes into ONE rAF per frame; without that, with
     50+ messages mounted, each `scrollTop = scrollHeight` forced a
     full-page layout, pegging the main thread for hundreds of ms
     per second. */
  let scrollPending = false;
  /* Threshold matches "within one bubble of the bottom" — generous
     enough that natural scroll-bounce keeps you stuck-to-bottom,
     tight enough that "I scrolled up to re-read" actually wins. */
  const STICK_PX = 80;
  let stickToBottom = true;

  function onChatScroll() {
    if (!chatEl) return;
    const distanceFromBottom = chatEl.scrollHeight - chatEl.scrollTop - chatEl.clientHeight;
    stickToBottom = distanceFromBottom < STICK_PX;
  }

  $effect(() => {
    if (!chatEl || !sess) return;
    /* Track the cheap signals — message count + last content length.
       Streaming bumps content.length per chunk; we still only do one
       scroll write per frame thanks to the rAF guard below. */
    void sess.messages.length;
    void sess.actions.length;
    void (sess.messages[sess.messages.length - 1]?.content?.length ?? 0);
    if (!stickToBottom) return;
    if (scrollPending) return;
    scrollPending = true;
    requestAnimationFrame(() => {
      scrollPending = false;
      if (!chatEl || !stickToBottom) return;
      chatEl.scrollTop = chatEl.scrollHeight;
    });
  });

  /* Trace-segment + tool-hint parsing moved to ./chatTraceParse.ts
   * (wave-1 phase-6 split). Pure string-in / object-out utilities
   * with no Svelte state — kept on the UI side because the
   * over-the-wire format is shared by both agents but the
   * structural decoration (icon + colour + label) belongs in the
   * renderer. */

  /** Set of pending question-action ids that are already anchored to
   *  a `_ask_` trace step in the current session — so the
   *  `inlineActions` snippet at the bottom of the message body doesn't
   *  render them a second time. Walks every message's events once on
   *  change of `sess.messages` / `sess.actions`. Cheap: bounded by
   *  trace-segment count, and most messages have zero asks. */
  const anchoredQuestionIds = $derived.by(() => {
    const ids = new Set<string>();
    if (!sess) return ids;
    // Early-exit: no pending question actions → no anchors possible.
    // Skips O(messages × events × segments) regex traversal that
    // otherwise re-runs on every streaming delta.
    if (!sess.actions.some((a) => a.kind === 'question' && a.status === 'pending')) return ids;
    for (const msg of sess.messages) {
      if (!msg.events) continue;
      for (const ev of msg.events) {
        if (ev.kind !== 'trace') continue;
        for (const seg of ev.segments) {
          const parsed = parseTraceSegment(seg);
          if (parsed.kind !== 'tool' || parsed.output) continue;
          const hint = parseToolHint(parsed.cmd);
          if (hint.kind !== 'ask') continue;
          const q = pendingQuestionForAskHint(hint.target);
          if (q) ids.add(q.id);
        }
      }
    }
    return ids;
  });

  /** Find the pending `question` action whose question text matches
   *  the given `_ask_` trace hint. Used to anchor the QuestionCard at
   *  the trace-step slot instead of trailing at the message bottom.
   *  Match strategy: prefix compare against the hint's truncated text
   *  AND the action's full question, in both directions, so an
   *  ellipsised hint still locks onto its long-form action. */
  function pendingQuestionForAskHint(
    probe: string
  ): Extract<ClaudeAction, { kind: 'question' }> | null {
    if (!sess || !probe) return null;
    const trimmed = probe.replace(/…$/, '').trim();
    if (!trimmed) return null;
    for (const a of sess.actions) {
      if (a.kind !== 'question' || a.status !== 'pending') continue;
      const aq = a.question.trim();
      if (aq === trimmed) return a;
      if (aq.startsWith(trimmed)) return a;
      if (trimmed.startsWith(aq.slice(0, Math.min(80, aq.length)))) return a;
    }
    return null;
  }

  /* LCS line diff helpers moved to ./chatDiff.ts (wave-1 phase-6
   * split). Pure utility — used by the inline edit-card preview to
   * show what the agent changed without leaving the conversation. */
</script>

{#if !sess}
  <div class="ct-empty">
    <p class="ct-empty-h serif">No active session</p>
    <p class="ct-empty-p">Pick a session from the sidebar or create a new one.</p>
  </div>
{:else}
  <div class="ct" bind:this={chatEl} onscroll={onChatScroll}>
    {#if sess.messages.length === 0 && sess.actions.length === 0}
      <div class="ct-welcome">
        <p class="ct-welcome-h serif">Ask {p.kind === 'claude' ? 'Claude' : 'Cursor'} anything</p>
        <p class="ct-welcome-p">Drop a Jira ticket / PR / file on the composer below to attach context. Use <span class="mono">/</span> for slash commands, <span class="mono">@</span> for files.</p>
      </div>
    {/if}

    <!-- Inline cards (SDD + question / propose_*) live INSIDE the
         current message's body so they inherit the byline-column
         indent and scroll naturally with the prose, instead of
         floating full-width below the conversation. Rendered once,
         under whichever message is `lastVisibleIndex`. -->
    {#snippet inlineActions()}
      {#if activeWorkflowForSession(sess.id)}
        {@const activeDw = activeWorkflowForSession(sess.id)}
        {#if activeDw}
          <!-- Active DW pinned to follow the conversation bottom (same
               slot grammar as the SDD card). Terminal workflows render
               at their origin message instead. -->
          <div class="action-wrap">
            <DynamicWorkflowCard workflowId={activeDw.id} onVerify={() => p.onDwVerify?.(activeDw.id)} />
          </div>
        {/if}
      {/if}
      {#if workspaceForSession(sess.id)}
        {@const sddWs = workspaceForSession(sess.id)}
        {#if sddWs && !isSddCardHidden(sddWs.id)}
          <div class="action-wrap">
            <SddCard
              workspace={sddWs}
              onAdvance={(prompt) => p.onSddAdvance?.(sess.id, prompt)}
            />
          </div>
        {/if}
        <!-- When hidden via the card's "—" button the SDD card is
             removed from the thread entirely. Re-open it from the
             SDD chip in the ChatHeader (which calls `showSddCard`
             alongside `openStandaloneView`). Workspace files on
             disk are untouched. -->
      {/if}

      {#each sess.actions as action (action.id)}
        {#if action.kind === 'question' && anchoredQuestionIds.has(action.id)}
          <!-- Already rendered inline at its `_ask_` trace step.
               Skip the bottom-of-body fallback to avoid duplicate
               cards. -->
        {:else}
        <div class="action-wrap">
          {#if action.kind === 'question'}
            <QuestionCard
              {action}
              onUpdate={(patch) => p.onUpdateAction(sess.id, action.id, patch)}
              onDismiss={() => p.onRemoveAction(sess.id, action.id)}
            />
          {:else}
            <ClaudeActionCard
              {action}
              onUpdate={(patch) => p.onUpdateAction(sess.id, action.id, patch)}
              onDismiss={() => p.onRemoveAction(sess.id, action.id)}
              onExecute={() => p.onExecuteAction(sess.id, action)}
              onOpenPrInWoom={(url) => p.onOpenPrInWoom(url, action.kind === 'pr' ? action : null)}
              {repoCwd}
            />
          {/if}
        </div>
        {/if}
      {/each}
    {/snippet}

    {#if hiddenAboveCount > 0}
      <button
        class="ct-load-more mono"
        onclick={() => (visibleCount = Math.min(nonHiddenTotal, visibleCount + WINDOW_STEP))}
        title="Render the previous {Math.min(WINDOW_STEP, hiddenAboveCount)} messages"
      >
        + Show {Math.min(WINDOW_STEP, hiddenAboveCount)} earlier {hiddenAboveCount === 1 ? 'message' : 'messages'}
        <span class="ct-load-more-sub">({visibleNonHiddenCount} of {nonHiddenTotal} shown)</span>
      </button>
    {/if}
    {#each visibleMessages as msg, sliceI (windowStart + sliceI)}
      {@const i = windowStart + sliceI}
      {#if msg.hidden}
        <!-- Hidden orchestration traffic (SDD phase prompts) — agent
             CLI sees it via --resume, the user doesn't. Skipped entirely. -->
      {:else if msg.role === 'user'}
        <article
          class="msg msg--user"
          use:observeArticle={i}
          oncontextmenu={(e) => openMsgCtxMenu(e, msg, i)}
        >
          <div class="msg-byline msg-byline--user">@you</div>
          <div class="msg-body">
            {#if shouldRenderBody(i)}
              <Markdown source={msg.content} onOpenFile={p.onOpenFile} />
            {:else}
              <div class="msg-stub" aria-hidden="true">{msg.content.slice(0, 80)}…</div>
            {/if}
            {#if msg.images && msg.images.length > 0}
              <div class="msg-images">
                {#each msg.images as img (img.path)}
                  <figure class="msg-image-fig">
                    <img class="msg-image" src={convertFileSrc(img.path)} alt={img.name} loading="lazy" />
                    <figcaption class="msg-image-name mono">{img.name}</figcaption>
                  </figure>
                {/each}
              </div>
            {/if}
            <div class="msg-actions">
              <span
                class="msg-act msg-drag"
                draggable="true"
                ondragstart={(e) => onMsgDragStart(e, msg, i)}
                ondragend={onMsgDragEnd}
                role="button"
                tabindex="-1"
                aria-label="Drag this message to Canvas or Editor"
                title="Drag → Canvas pins as a card. Drag → another chat attaches as @mention."
              >
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" aria-hidden="true">
                  <circle cx="9" cy="6" r="1.2" fill="currentColor"/>
                  <circle cx="15" cy="6" r="1.2" fill="currentColor"/>
                  <circle cx="9" cy="12" r="1.2" fill="currentColor"/>
                  <circle cx="15" cy="12" r="1.2" fill="currentColor"/>
                  <circle cx="9" cy="18" r="1.2" fill="currentColor"/>
                  <circle cx="15" cy="18" r="1.2" fill="currentColor"/>
                </svg>
              </span>
              <button class="msg-act" onclick={() => p.onStartEditMessage(sess.id, i, msg.content)} title="Edit + resend">
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7"><path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7"/><path d="M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4 9.5-9.5z"/></svg>
              </button>
              <button class="msg-act" onclick={() => p.onResendMessage(sess.id, i, msg.content)} title="Resend">
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7"><path d="M21 12a9 9 0 1 1-3-6.7"/><path d="M21 4v5h-5"/></svg>
              </button>
            </div>
            {#if i === lastVisibleIndex}{@render inlineActions()}{/if}
          </div>
        </article>
      {:else if msg.role === 'assistant'}
        <article
          class="msg msg--assistant"
          use:observeArticle={i}
          oncontextmenu={(e) => openMsgCtxMenu(e, msg, i)}
        >
          <!-- Floating drag handle. Pinned to the article's top-right
               via .msg-drag--float; visible on hover so it doesn't
               clutter the chat at rest. Pure dnd source — clicks on
               the article body don't trigger drag, only the handle. -->
          <span
            class="msg-act msg-drag msg-drag--float"
            draggable="true"
            ondragstart={(e) => onMsgDragStart(e, msg, i)}
            ondragend={onMsgDragEnd}
            role="button"
            tabindex="-1"
            aria-label="Drag this message to Canvas or Editor"
            title="Drag → Canvas pins as a card. Drag → another chat attaches as @mention."
          >
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" aria-hidden="true">
              <circle cx="9" cy="6" r="1.2" fill="currentColor"/>
              <circle cx="15" cy="6" r="1.2" fill="currentColor"/>
              <circle cx="9" cy="12" r="1.2" fill="currentColor"/>
              <circle cx="15" cy="12" r="1.2" fill="currentColor"/>
              <circle cx="9" cy="18" r="1.2" fill="currentColor"/>
              <circle cx="15" cy="18" r="1.2" fill="currentColor"/>
            </svg>
          </span>
          <div class="msg-byline msg-byline--assistant">{p.kind}</div>
          <div class="msg-body">
            {#if msg.thinking}
              <details class="thinking-pill">
                <summary>thinking ✓</summary>
                <pre class="thinking-body">{msg.thinking}</pre>
              </details>
            {/if}
            {#if msg.dwWorkflowId && !isWorkflowActive(msg.dwWorkflowId)}
              <!-- Terminal workflows stay as a record at their origin
                   message. The ACTIVE one renders in the pinned
                   bottom-following slot (inlineActions) so it stays
                   visible like the SDD card instead of scrolling away. -->
              <DynamicWorkflowCard workflowId={msg.dwWorkflowId} onVerify={() => p.onDwVerify?.(msg.dwWorkflowId!)} />
            {/if}
            {#if !shouldRenderBody(i)}
              <!-- Off-viewport placeholder. Approximate height keeps the
                   scrollbar honest until the article scrolls within the
                   IntersectionObserver buffer and the real body mounts. -->
              <div class="msg-stub msg-stub--assistant" aria-hidden="true">
                {msg.content.slice(0, 160) || '…'}
              </div>
            {:else if msg.events && msg.events.length > 0}
              {#each msg.events as ev, ei (ei)}
                {#if ev.kind === 'text'}
                  {#if ev.body}<Markdown source={ev.body} onOpenFile={p.onOpenFile} />{/if}
                {:else if ev.kind === 'trace'}
                  <details class="trace" class:trace--single={ev.segments.length === 1} open>
                    <!-- Summary stays in the DOM for a11y (focus + Enter
                         toggle), but is hidden via CSS when there's
                         only one step — single calls render straight
                         under the prose, no "1 step" header noise.
                         The carets / glyphs collapse to text-glyph
                         dimensions; bg + bottom border were dropped
                         in phase 1 already. -->
                    <summary class="trace-head">
                      <span class="trace-head-caret" aria-hidden="true">
                        <svg viewBox="0 0 24 24" width="9" height="9" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"><path d="M9 6l6 6-6 6"/></svg>
                      </span>
                      <span class="trace-head-label"><strong>{ev.segments.length}</strong> step{ev.segments.length === 1 ? '' : 's'}</span>
                    </summary>
                    <div class="trace-body">
                      {#snippet toolIcon(kind: string)}
                        {#if kind === 'read'}
                          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M2 12s3-7 10-7 10 7 10 7-3 7-10 7S2 12 2 12z"/><circle cx="12" cy="12" r="3"/></svg>
                        {:else if kind === 'edit'}
                          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M12 20h9"/><path d="M16.5 3.5a2.121 2.121 0 0 1 3 3L7 19l-4 1 1-4 12.5-12.5z"/></svg>
                        {:else if kind === 'write' || kind === 'create'}
                          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><polyline points="14 2 14 8 20 8"/><line x1="12" y1="18" x2="12" y2="12"/><line x1="9" y1="15" x2="15" y2="15"/></svg>
                        {:else if kind === 'delete'}
                          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><polyline points="3 6 5 6 21 6"/><path d="M19 6l-2 14a2 2 0 0 1-2 2H9a2 2 0 0 1-2-2L5 6"/><path d="M10 11v6M14 11v6"/></svg>
                        {:else if kind === 'bash'}
                          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><polyline points="4 17 10 11 4 5"/><line x1="12" y1="19" x2="20" y2="19"/></svg>
                        {:else if kind === 'grep'}
                          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><circle cx="11" cy="11" r="7"/><line x1="20" y1="20" x2="17" y2="17"/></svg>
                        {:else if kind === 'glob'}
                          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M3 7v11a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2V9a2 2 0 0 0-2-2h-7L10 5H5a2 2 0 0 0-2 2z"/><path d="M9 13l2 2 4-4"/></svg>
                        {:else if kind === 'webfetch' || kind === 'websearch'}
                          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="9"/><path d="M3 12h18M12 3a14 14 0 0 1 0 18M12 3a14 14 0 0 0 0 18"/></svg>
                        {:else if kind === 'commit' || kind === 'pr'}
                          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><circle cx="6" cy="6" r="2.5"/><circle cx="18" cy="18" r="2.5"/><path d="M6 8.5V14a4 4 0 0 0 4 4h6"/></svg>
                        {:else if kind === 'switch_cwd'}
                          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M3 7v11a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2V9a2 2 0 0 0-2-2h-7L10 5H5a2 2 0 0 0-2 2z"/><path d="M16 14l3-3-3-3M9 11h10"/></svg>
                        {:else if kind === 'todos'}
                          <!-- Three-line checklist with a tick on the first
                               row — telegraphs "agent's plan" at a glance. -->
                          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><polyline points="9 6 11 8 15 4"/><line x1="3" y1="6" x2="6" y2="6"/><line x1="3" y1="12" x2="21" y2="12"/><line x1="3" y1="18" x2="21" y2="18"/></svg>
                        {:else if kind === 'ask'}
                          <!-- Speech bubble + question mark — the
                               resolved trace step shows what the
                               agent asked and what the user picked. -->
                          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M21 11.5a8.38 8.38 0 0 1-.9 3.8 8.5 8.5 0 0 1-7.6 4.7 8.38 8.38 0 0 1-3.8-.9L3 21l1.9-5.7a8.38 8.38 0 0 1-.9-3.8 8.5 8.5 0 0 1 4.7-7.6 8.38 8.38 0 0 1 3.8-.9h.5a8.48 8.48 0 0 1 8 8v.5z"/></svg>
                        {:else if kind === 'mcp'}
                          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><polygon points="12 2 22 8.5 22 15.5 12 22 2 15.5 2 8.5 12 2"/><line x1="12" y1="22" x2="12" y2="12"/><line x1="22" y1="8.5" x2="12" y2="12"/><line x1="2" y1="8.5" x2="12" y2="12"/></svg>
                        {:else}
                          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09A1.65 1.65 0 0 0 4.6 9 1.65 1.65 0 0 0 4.27 7.18l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06A1.65 1.65 0 0 0 9 4.6 1.65 1.65 0 0 0 10 3.09V3a2 2 0 1 1 4 0v.09c0 .68.4 1.29 1 1.51"/></svg>
                        {/if}
                      {/snippet}
                      {#each ev.segments as seg, si (si)}
                        {@const parsed = parseTraceSegment(seg)}
                        {#if parsed.kind === 'tool' && (parsed.cmd || parsed.output)}
                          {@const hint = parseToolHint(parsed.cmd)}
                          {@const fallbackBash =
                            !hint.target && !hint.scope && parsed.cmd && hint.kind === 'bash'
                              ? parsed.cmd : ''}
                          {@const lineCount = parsed.output ? parsed.output.split('\n').length : 0}
                          {#if parsed.output}
                            <!-- Combined card: command is the SUMMARY, output is
                                 the BODY. Click anywhere on the header to
                                 expand the inline output — same widget,
                                 instead of two stacked pills. -->
                            <details class="trace-step trace-step--{hint.kind} trace-step--has-output">
                              <summary class="trace-cmd-row trace-cmd-row--toggle">
                                <span class="trace-cmd-icon" aria-hidden="true">
                                  {@render toolIcon(hint.kind)}
                                </span>
                                <span class="trace-cmd-label mono">{hint.label}</span>
                                {#if hint.target}
                                  <code class="trace-cmd-target mono" title={hint.target}>{hint.target}</code>
                                {/if}
                                {#if hint.scope}
                                  <span class="trace-cmd-scope mono">{hint.scope}</span>
                                {/if}
                                {#if fallbackBash}
                                  <code class="trace-cmd-target mono">{fallbackBash}</code>
                                {/if}
                                <span class="trace-cmd-meta mono" aria-label="output line count">{lineCount} line{lineCount === 1 ? '' : 's'}</span>
                                <span class="trace-cmd-caret" aria-hidden="true">
                                  <svg viewBox="0 0 24 24" width="11" height="11" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"><path d="M6 9l6 6 6-6"/></svg>
                                </span>
                              </summary>
                              <pre class="trace-out-body mono">{parsed.output}</pre>
                            </details>
                          {:else if hint.kind === 'ask'}
                            <!-- `ask_user_question` running: swap the trace
                                 row for the interactive QuestionCard
                                 inline at the EXACT step position, so
                                 the prompt reads as part of the agent's
                                 turn (same vertical slot as the tool
                                 call), not as a separate panel at the
                                 bottom of the message. Match by question
                                 prefix — wait_id ↔ tool_use_id linkage
                                 doesn't exist in our IPC, so we lean on
                                 the question text. -->
                            {@const pendingQ = pendingQuestionForAskHint(hint.target)}
                            {#if pendingQ}
                              <QuestionCard
                                action={pendingQ}
                                onUpdate={(patch) => p.onUpdateAction(sess.id, pendingQ.id, patch)}
                                onDismiss={() => p.onRemoveAction(sess.id, pendingQ.id)}
                              />
                            {:else}
                              <div class="trace-step trace-step--ask" data-status="running">
                                <div class="trace-cmd-row">
                                  <span class="trace-cmd-icon" aria-hidden="true">
                                    {@render toolIcon('ask')}
                                  </span>
                                  <span class="trace-cmd-label mono">{hint.label}</span>
                                  {#if hint.target}
                                    <code class="trace-cmd-target mono" title={hint.target}>{hint.target}</code>
                                  {/if}
                                </div>
                              </div>
                            {/if}
                          {:else}
                            <!-- No output (yet): streaming row.
                                 `data-status="running"` triggers the
                                 leading-glyph pulse animation so the
                                 user sees this step is in-flight. -->
                            <div class="trace-step trace-step--{hint.kind}" data-status="running">
                              <div class="trace-cmd-row">
                                <span class="trace-cmd-icon" aria-hidden="true">
                                  {@render toolIcon(hint.kind)}
                                </span>
                                <span class="trace-cmd-label mono">{hint.label}</span>
                                {#if hint.target}
                                  <code class="trace-cmd-target mono" title={hint.target}>{hint.target}</code>
                                {/if}
                                {#if hint.scope}
                                  <span class="trace-cmd-scope mono">{hint.scope}</span>
                                {/if}
                                {#if fallbackBash}
                                  <code class="trace-cmd-target mono" title={fallbackBash}>{fallbackBash}</code>
                                {/if}
                              </div>
                            </div>
                          {/if}
                        {:else if parsed.kind === 'text'}
                          <div class="trace-line"><Markdown source={seg} onOpenFile={p.onOpenFile} /></div>
                        {/if}
                      {/each}
                    </div>
                  </details>
                {:else if ev.kind === 'edit'}
                  {@const stats = diffStats(ev.oldText ?? '', ev.newText ?? '')}
                  <details class="edit-card">
                    <summary class="edit-card-head">
                      {#if ev.isCreate}
                        <span class="edit-tag edit-tag--add">Create</span>
                      {:else if ev.isDelete}
                        <span class="edit-tag edit-tag--rem">Delete</span>
                      {:else if ev.wholeFile}
                        <span class="edit-tag">Write</span>
                      {:else}
                        <span class="edit-tag">Edit</span>
                      {/if}
                      <span class="edit-path mono">{ev.filePath}</span>
                      <span class="edit-stats mono">
                        <span class="add">+{stats.add}</span>
                        <span class="rem">−{stats.rem}</span>
                      </span>
                      <span class="edit-status mono">{ev.status}</span>
                      <span class="edit-expand">
                        <svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"><path d="M6 9l6 6 6-6"/></svg>
                      </span>
                    </summary>
                    <div class="edit-card-body">
                      <div class="diff">
                        {#each computeDiffRows(ev.oldText ?? '', ev.newText ?? '') as row, ri (ri)}
                          <div class="diff-row diff-row--{row.kind}">
                            <span class="diff-no mono">{row.oldNo ?? ''}</span>
                            <span class="diff-no mono">{row.newNo ?? ''}</span>
                            <span class="diff-glyph mono">{row.kind === 'add' ? '+' : row.kind === 'rem' ? '−' : ' '}</span>
                            <span class="diff-text mono">{row.text}</span>
                          </div>
                        {/each}
                      </div>
                    </div>
                  </details>
                {/if}
              {/each}
            {:else if msg.content}
              <Markdown source={msg.content} onOpenFile={p.onOpenFile} />
            {/if}
            {#if sess.sending && i === sess.messages.length - 1 && (!msg.content || msg.content.length < 6)}
              <div class="thinking">
                <span class="dot-row">
                  <span class="dot"></span>
                  <span class="dot"></span>
                  <span class="dot"></span>
                </span>
                <span>thinking{elapsed ? ` · ${elapsed}` : ''}</span>
              </div>
            {/if}
            {#if msg.usage}
              <div class="msg-usage mono">
                {Math.round(msg.usage.contextSize / 1000)}K context · {msg.usage.outputTokens} out
              </div>
            {/if}
            {#if i === lastVisibleIndex}{@render inlineActions()}{/if}
          </div>
        </article>
        {#if msg.interrupted === 'quota' && i === sess.messages.length - 1 && sess.awaitingResume && p.onResumeAfterQuota}
          <ResumePill session={sess} onResume={p.onResumeAfterQuota} />
        {/if}
      {:else}
        <article class="msg msg--system" use:observeArticle={i}>
          <div class="msg-system">{msg.content}</div>
          {#if i === lastVisibleIndex}{@render inlineActions()}{/if}
        </article>
      {/if}
    {/each}
  </div>
{/if}

<CardContextMenu coords={ctxCoords} items={ctxItems} onClose={closeMsgCtxMenu} />

<style>
  .ct {
    flex: 1; min-height: 0;
    overflow-y: auto;
    padding: 28px 28px 12px;
    display: flex; flex-direction: column; gap: 24px;
  }
  .ct-load-more {
    align-self: center;
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 7px 14px;
    border-radius: 8px;
    background: color-mix(in srgb, var(--accent) 6%, transparent);
    border: 1px dashed color-mix(in srgb, var(--border) 80%, transparent);
    color: var(--text-2);
    font-size: 11px;
    cursor: pointer;
    transition: background 140ms, border-color 140ms;
  }
  .ct-load-more:hover {
    background: color-mix(in srgb, var(--accent) 12%, transparent);
    border-color: color-mix(in srgb, var(--accent) 40%, transparent);
    color: var(--text);
  }
  .ct-load-more-sub {
    opacity: 0.6;
    font-size: 10px;
  }

  /* v7 — 76px byline column + 1fr body. */
  .msg {
    display: grid;
    grid-template-columns: 76px 1fr;
    gap: 16px;
    align-items: start;
  }
  .msg--system { grid-template-columns: 1fr; }

  .msg-byline {
    font-size: 11px;
    color: var(--text-mute);
    letter-spacing: 0.04em;
    font-weight: 600;
    padding-top: 4px;
  }
  .msg-byline--user { color: var(--text-2); }
  .msg-byline--assistant {
    color: var(--app-tone, var(--src-claude));
    text-transform: lowercase;
    letter-spacing: -0.01em;
    font-family: 'Geist', 'Inter', -apple-system, system-ui, sans-serif;
    font-size: 13px;
    font-weight: 600;
  }

  .msg-body { min-width: 0; position: relative; }

  /* Off-viewport placeholder for lazy-mounted message bodies.
     Shows a single-line excerpt so scroll position stays meaningful
     and a11y tools have something to read; the real <Markdown /> +
     trace tree mounts as soon as the IntersectionObserver fires
     (rootMargin ±1200px so this swap-in happens well before the
     stub actually scrolls into view). */
  .msg-stub {
    color: var(--text-2);
    font-size: 13.5px;
    line-height: 1.6;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    min-height: 22px;
    opacity: 0.6;
    font-style: italic;
  }
  .msg-stub--assistant {
    min-height: 80px;
    white-space: normal;
    display: -webkit-box;
    -webkit-line-clamp: 3;
    line-clamp: 3;
    -webkit-box-orient: vertical;
  }
  .msg-body :global(p) { margin: 0 0 8px; line-height: 1.6; color: var(--text-0); }
  .msg-body :global(p:last-child) { margin-bottom: 0; }
  .msg-body :global(strong) { color: var(--text-0); font-weight: 600; }
  .msg-body :global(code) {
    font-family: 'JetBrains Mono', monospace;
    font-size: 12.5px;
    padding: 1px 6px;
    background: var(--bg-2);
    border: 1px solid var(--border);
    border-radius: 4px;
    color: var(--accent-bright);
  }

  /* User message — flat blockquote-style on the prose surface.
     Same chrome grammar as SddCard / Markdown.svelte blockquote:
     3px accent stripe + accent-soft tint + rounded only on the
     right. No full border, no gradient bg. User's text reads as
     part of the typographic surface, not as a chat bubble widget. */
  .msg--user .msg-body {
    position: relative;
    padding: 8px 14px 9px;
    background: var(--accent-soft);
    border: 0;
    border-left: 3px solid var(--accent);
    border-radius: 0 6px 6px 0;
  }

  /* Hover actions sit BELOW the bubble (not over it) — small, naked
     icons, only revealed on hover. No backdrop, no border, no shadow:
     they read as a quiet utility row and don't compete with the prose
     above. The bubble's resting height is content-only because the
     actions are absolute-positioned just below the bottom edge. */
  .msg-actions {
    position: absolute;
    bottom: -22px;
    right: 4px;
    display: flex; gap: 2px;
    opacity: 0;
    transition: opacity 140ms;
    pointer-events: none;
  }
  .msg--user:hover .msg-actions,
  .msg--user:focus-within .msg-actions {
    opacity: 0.85;
    pointer-events: auto;
  }
  .msg-act {
    width: 20px; height: 20px;
    display: grid; place-items: center;
    color: var(--text-mute);
    background: transparent; border: 0; cursor: pointer;
    border-radius: 4px;
    padding: 0;
    transition: color 120ms;
  }
  .msg-act:hover { color: var(--text-0); }
  .msg-act svg { width: 12px; height: 12px; }

  /* Drag handle — `<span>` instead of `<button>` because Safari/
     WebKit drag is more reliable on plain elements. cursor: grab tells
     the user it's draggable; switching to grabbing on :active matches
     macOS conventions. */
  .msg-drag {
    cursor: grab;
    color: var(--text-mute);
    /* Inline-flex with align-items: center so the SVG inside is
       centered without the host needing exact width/height. */
    display: inline-flex; align-items: center; justify-content: center;
  }
  .msg-drag:active { cursor: grabbing; }
  .msg-drag:hover { color: var(--accent-bright); }
  /* SVG inside the handle shouldn't capture pointer events — otherwise
     the dragstart fires on the path element, not the span, and some
     WebKit builds refuse to start a drag from inside an inline SVG. */
  .msg-drag svg { pointer-events: none; }

  /* Floating handle for assistant messages — pinned to the article's
     top-right. Anchored on .msg (which is `display: grid; position:
     static`), so we need to make the article a positioning context.
     Hidden at rest, revealed on hover. */
  .msg--assistant { position: relative; }
  .msg-drag--float {
    position: absolute;
    top: 0;
    right: 4px;
    width: 22px;
    height: 22px;
    display: grid;
    place-items: center;
    background: var(--bg-2);
    border: 1px solid var(--border);
    border-radius: 5px;
    opacity: 0;
    transition: opacity 140ms, color 120ms;
    z-index: 2;
  }
  .msg-drag--float svg { width: 12px; height: 12px; }
  .msg--assistant:hover .msg-drag--float,
  .msg--assistant:focus-within .msg-drag--float {
    opacity: 0.85;
  }
  .msg-drag--float:hover { opacity: 1; }

  /* External-file attachments (today: pasted/dropped images). Plain
     external file refs typed via @ live in the prompt text inline,
     not here. */
  .msg-images { margin-top: 10px; display: flex; gap: 8px; flex-wrap: wrap; }
  .msg-image-fig {
    margin: 0;
    display: flex; flex-direction: column;
    gap: 4px;
    max-width: 240px;
  }
  .msg-image {
    max-width: 240px; max-height: 200px;
    border-radius: 8px;
    border: 1px solid var(--border);
  }
  .msg-image-name {
    font-size: 10px;
    color: var(--text-mute);
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }

  .msg-system {
    font-size: 11.5px;
    color: var(--text-mute);
    text-align: center;
    
    padding: 4px 12px;
  }

  /* Trace cluster — outer marker for a run of tool-call steps from
     one assistant turn. Single soft left stripe groups the run as a
     cluster; the inner per-step rows render as flat text lines on
     the prose surface, NOT as bordered widgets. Spec ref:
     "annotated text lines, not boxed widgets". */
  .trace {
    display: block;
    margin: 6px 0;
    /* Span the full message-body column so trace rows align with the
     * Edit / Write diff cards rendered alongside — earlier 720px cap
     * left bash / read / grep rows visually short of the right edge
     * while edit-cards ran to the chat-column boundary, producing a
     * jagged right-margin in the trace. Uniform width reads cleaner. */
     width: 100%;
    border: 0;
    background: transparent;
    border-radius: 0;
    border-left: 2px solid color-mix(in srgb, var(--accent) 25%, transparent);
    padding-left: 12px;
  }
  /* Outer "N steps" head — quiet text prefix above the run.
     Caret + label only, no bg, no border, no separator. Hidden via
     `.trace--single` when there's exactly one step (the modifier is
     set on `<details>` from the markup based on segments.length). */
  .trace-head {
    display: flex; align-items: baseline; gap: 5px;
    padding: 0;
    font-size: 11.5px;
    color: var(--text-mute);
    cursor: pointer;
    user-select: none;
    list-style: none;
  }
  .trace-head::-webkit-details-marker { display: none; }
  .trace-head::marker { content: ''; }
  .trace-head:hover { color: var(--text-1); }
  .trace--single > .trace-head { display: none; }
  .trace-head-label {
    font-size: 11.5px;
    color: var(--text-mute);
  }
  .trace-head :global(strong) {
    color: var(--text-1);
    font-weight: 500;
    margin-right: 1px;
  }
  /* Leading ▸ caret on the head — rotates to ▾ when the run is open.
     Matches the per-step caret rotation; same visual grammar. */
  .trace-head-caret {
    color: var(--text-mute);
    display: inline-grid; place-items: center;
    transition: transform 160ms;
    transform: translateY(1px);
    opacity: 0.7;
  }
  .trace[open] .trace-head-caret { transform: translateY(1px) rotate(90deg); }
  /* No bottom border when open — flat continuation into step lines. */
  .trace-body {
    padding: 2px 0 4px;
    display: flex; flex-direction: column;
    gap: 1px;
  }
  /* Step row — flat text line. No border, no bg, no hover panel; just
     glyph + label + target + scope + meta + caret on one baseline.
     `--step-tone` lives on for the glyph color only. */
  .trace-step {
    --step-tone: var(--accent-bright);
    display: flex; flex-direction: column;
    background: transparent;
    border: 0;
    border-radius: 0;
    overflow: hidden;
    transition: none;
  }
  .trace-step--has-output { cursor: pointer; }
  .trace-step--has-output:hover .trace-cmd-label { color: var(--text-0); }

  /* In-flight pulse — slow 1.2s opacity oscillation on the leading
     glyph only; rest of the row stays steady so the eye doesn't
     get tugged by a whole flashing line. Stamped via `data-status="running"`
     from the streaming-branch markup; completed rows drop the
     attribute, so the animation auto-stops when output lands. */
  @keyframes trace-pulse {
    0%, 100% { opacity: 1; }
    50%      { opacity: 0.35; }
  }
  .trace-step[data-status="running"] .trace-cmd-icon {
    animation: trace-pulse 1.2s ease-in-out infinite;
  }
  @media (prefers-reduced-motion: reduce) {
    .trace-step[data-status="running"] .trace-cmd-icon { animation: none; opacity: 0.7; }
  }

  /* Error state — DEFERRED. The plan called for swapping glyph +
     label to `var(--error)` via `data-status="error"`, but
     `parseTraceSegment` doesn't currently expose an error flag (no
     exit-code / stderr-marker plumbing), so no markup branch can
     stamp the attribute. Wiring lands separately; the CSS rule was
     dropped here to keep svelte-check clean instead of leaving a
     dead selector. When detection lands, re-add:
       `.trace-step[data-status="error"] { --step-tone: var(--error); }`
       `.trace-step[data-status="error"] .trace-cmd-label,`
       `.trace-step[data-status="error"] .trace-cmd-target { color: var(--error); }` */
  /* Command row — single flex line at prose line-height. Icon column is
     a fixed-width text-glyph slot; no chip background. Items align
     on the baseline so the glyph + label + target read as one strip
     of text. */
  .trace-cmd-row {
    display: flex; align-items: baseline; gap: 6px;
    padding: 0;
    min-width: 0;
    line-height: 1.55;
  }
  .trace-cmd-row--toggle {
    list-style: none;
    user-select: none;
  }
  .trace-cmd-row--toggle::-webkit-details-marker { display: none; }
  .trace-cmd-row--toggle::marker { content: ''; }
  .trace-cmd-icon {
    flex-shrink: 0;
    width: 14px; height: 14px;
    display: inline-grid; place-items: center;
    color: var(--step-tone);
    background: transparent;
    box-shadow: none;
    /* Lift slightly so SVG aligns with text baseline. */
    transform: translateY(2px);
  }
  .trace-cmd-icon svg { width: 12px; height: 12px; }
  .trace-cmd-label {
    flex-shrink: 0;
    font-family: 'JetBrains Mono', monospace;
    font-size: 12px;
    font-weight: 500;
    color: var(--text-2);
    text-transform: lowercase;
    letter-spacing: 0;
    line-height: 1.55;
  }
  .trace-cmd-target {
    flex: 1; min-width: 0;
    /* Soft cap kept generous (~140ch) so long bash / grep targets
     *  stay readable end-to-end before truncation kicks in. Caller's
     *  `title=` attr preserves the full string for hover. */
    max-width: 140ch;
    font-size: 12px;
    /* Drop the inline-code chip chrome inherited from prose-level
     *  `code` styling (bg + border). Trace targets read as bare
     *  mono text — color carries the per-step tone via `--step-tone`
     *  instead of a gray pill. */
    color: var(--step-tone, var(--text-1));
    background: transparent !important;
    border: 0 !important;
    padding: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    /* Default LTR ellipsis — right-truncates with `…` at the end.
       Path-shaped kinds (read/edit/write/create/delete) flip to RTL
       below so basenames stay visible on long absolute paths.
       (`apps/desktop/.../codemirrorLang.ts` →
       `…/codemirrorLang.ts`.) Bash / grep / shell-command kinds
       keep LTR so the verb at the start of the command stays
       visible — RTL on bash produced `…d /Users/... && git` which
       hid `cd` and clipped the tail under the meta. */
    direction: ltr;
    text-align: left;
  }
  .trace-step--read .trace-cmd-target,
  .trace-step--edit .trace-cmd-target,
  .trace-step--write .trace-cmd-target,
  .trace-step--create .trace-cmd-target,
  .trace-step--delete .trace-cmd-target,
  .trace-step--glob .trace-cmd-target {
    direction: rtl;
    unicode-bidi: plaintext;
  }
  /* When the row is open, drop the truncation so the user sees the
     entire command above the output. */
  .trace-step[open] .trace-cmd-target {
    direction: ltr;
    white-space: pre-wrap;
    overflow: visible;
    text-overflow: clip;
    word-break: break-all;
  }
  .trace-cmd-scope {
    flex-shrink: 0;
    font-size: 11px;
    color: var(--text-mute);
    line-height: 1.55;
  }
  .trace-cmd-meta {
    flex-shrink: 0;
    font-size: 10.5px;
    color: var(--text-mute);
    margin-left: auto;
    padding-left: 6px;
  }
  .trace-cmd-caret {
    flex-shrink: 0;
    display: inline-grid; place-items: center;
    color: var(--text-mute);
    transition: transform 140ms;
    opacity: 0.6;
  }
  .trace-step[open] .trace-cmd-caret { transform: rotate(180deg); opacity: 0.9; }
  /* Per-kind hue overrides. Read = info blue, mutations = warm
     editor-orange, search = mint, web = teal, mcp = lavender,
     cwd/commit/pr = jira blue. Keeps the sequence scannable
     ("oh that block was all reads, that one wrote files"). */
  .trace-step--read       { --step-tone: var(--info, #88C2DD); }
  .trace-step--grep,
  .trace-step--glob       { --step-tone: var(--accent-bright); }
  .trace-step--bash       { --step-tone: var(--src-term, var(--text-2)); }
  .trace-step--edit,
  .trace-step--write,
  .trace-step--create     { --step-tone: var(--src-editor); }
  .trace-step--delete     { --step-tone: var(--error); }
  .trace-step--webfetch,
  .trace-step--websearch  { --step-tone: var(--src-canvas); }
  .trace-step--commit,
  .trace-step--pr         { --step-tone: var(--src-jira); }
  .trace-step--switch_cwd { --step-tone: var(--src-jira); }
  .trace-step--mcp        { --step-tone: var(--src-github); }
  /* Expanded output — indented mono continuation under the row. No
     box, no separator divider, no bg fill. A dashed left stripe sits
     under the step row's icon column, so the eye associates the
     continuation with its parent. Matches Markdown.svelte's
     blockquote pattern but inset further to nest inside the trace
     cluster's outer stripe. */
  .trace-out-body {
    margin: 2px 0 4px 20px;
    padding-left: 12px;
    border-left: 1px dashed color-mix(in srgb, var(--step-tone) 35%, transparent);
    font-family: 'JetBrains Mono', monospace;
    font-size: 11.5px;
    line-height: 1.55;
    color: var(--text-2);
    white-space: pre-wrap;
    word-break: break-word;
    max-height: 320px;
    overflow-y: auto;
    background: transparent;
  }
  /* Plain text segment fallback — markdown-rendered. */
  .trace-line {
    font-size: 12px;
    color: var(--text-1);
    line-height: 1.55;
  }
  .trace-line :global(p) { margin: 0; }
  .trace-line :global(code) {
    font-family: 'JetBrains Mono', monospace;
    font-size: 11.5px;
    padding: 1px 6px;
    background: var(--bg-3);
    border: 1px solid var(--border);
    border-radius: 4px;
    color: var(--text-0);
  }
  .trace-line :global(em) {
    color: var(--text-2);
    
  }

  /* Edit card — flat file-edit line on the prose surface, same
     grammar as trace step rows. No box, no full border, no bg fill;
     just a 2px editor-tone left stripe + 12px indent so the file
     edit reads as an annotation in the conversation. Expanded body
     (diff) keeps its content but loses the wrapper bg. */
  .edit-card {
    margin: 6px 0;
    border-left: 2px solid color-mix(in srgb, var(--src-editor) 70%, transparent);
    padding-left: 12px;
    background: transparent;
    border-radius: 0;
    font-size: 12.5px;
  }
  .edit-card-head {
    display: flex; align-items: baseline;
    gap: 8px;
    padding: 0;
    cursor: pointer;
    user-select: none;
    list-style: none;
    line-height: 1.55;
  }
  .edit-card-head::-webkit-details-marker { display: none; }
  .edit-card-head::marker { content: ''; }
  /* No head bg on open — flat continuation into the diff body. */
  .edit-card[open] .edit-expand svg { transform: rotate(180deg); }
  /* Tag reads as a lowercase mono prefix, not a chip. */
  .edit-tag {
    display: inline-flex; align-items: baseline;
    padding: 0;
    font-family: 'JetBrains Mono', monospace;
    font-size: 11px;
    font-weight: 500;
    letter-spacing: 0;
    text-transform: lowercase;
    background: transparent;
    border-radius: 0;
    color: var(--src-editor);
    flex-shrink: 0;
  }
  .edit-tag--add { color: var(--diff-add-stroke); }
  .edit-tag--rem { color: var(--diff-rem-stroke); }
  .edit-path {
    font-size: 12px;
    color: var(--text-1);
    flex: 1;
    white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
    direction: rtl;
    text-align: left;
    unicode-bidi: plaintext;
  }
  .edit-card[open] .edit-path {
    direction: ltr;
    white-space: pre-wrap;
    word-break: break-all;
  }
  .edit-stats { display: flex; gap: 8px; font-size: 10.5px; }
  .edit-stats .add { color: var(--diff-add); }
  .edit-stats .rem { color: var(--diff-rem); }
  /* Status reads as muted text — "applied", "pending" — not a pill. */
  .edit-status {
    font-size: 10px;
    color: var(--text-mute);
    text-transform: lowercase;
    letter-spacing: 0;
    padding: 0;
    border-radius: 0;
    background: transparent;
    border: 0;
  }
  .edit-expand {
    color: var(--text-mute);
    display: inline-grid; place-items: center;
    transition: transform 160ms;
    opacity: 0.6;
  }
  .edit-expand svg { transition: transform 160ms; }

  /* Expanded diff body — same indent as the head, no wrapper bg. */
  .edit-card-body {
    max-height: 480px;
    overflow: auto;
    background: transparent;
    margin-top: 4px;
  }
  .diff {
    display: block;
    font-family: 'JetBrains Mono', monospace;
    font-size: 11.5px;
    line-height: 1.55;
  }
  .diff-row {
    display: grid;
    grid-template-columns: 36px 36px 16px 1fr;
    gap: 0;
    padding: 0 6px;
    white-space: pre;
  }
  .diff-row--add {
    background: rgba(101, 211, 150, 0.08);
  }
  .diff-row--add .diff-glyph { color: var(--diff-add); }
  .diff-row--add .diff-text { color: var(--text-0); }
  .diff-row--rem {
    background: rgba(232, 130, 100, 0.08);
  }
  .diff-row--rem .diff-glyph { color: var(--diff-rem); }
  .diff-row--rem .diff-text { color: var(--text-1); text-decoration: line-through; text-decoration-thickness: 0.5px; text-decoration-color: rgba(232, 130, 100, 0.45); }
  .diff-row--ctx .diff-glyph { color: var(--text-mute); }
  .diff-row--ctx .diff-text { color: var(--text-2); }
  .diff-no {
    color: var(--text-mute);
    text-align: right;
    padding-right: 8px;
    user-select: none;
    font-size: 10.5px;
    line-height: 1.55;
  }
  .diff-glyph {
    text-align: center;
    user-select: none;
    color: var(--text-mute);
  }
  .diff-text {
    overflow: hidden;
    text-overflow: ellipsis;
  }

  /* Thinking pill (reasoning trace, collapsible) */
  .thinking-pill {
    margin-bottom: 10px;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--bg-3);
  }
  .thinking-pill summary {
    padding: 6px 10px;
    font-family: 'JetBrains Mono', monospace; font-size: 10.5px;
    color: var(--text-mute);
    cursor: pointer;
    user-select: none;
  }
  .thinking-body {
    margin: 0; padding: 8px 12px;
    border-top: 1px solid var(--border);
    font-family: 'JetBrains Mono', monospace; font-size: 11px;
    color: var(--text-1); line-height: 1.5;
    white-space: pre-wrap;
    max-height: 320px; overflow: auto;
  }

  /* Live thinking indicator — 3 staggered pulsing dots + elapsed. */
  .thinking {
    display: flex; align-items: center; gap: 10px;
    padding: 6px 0;
    font-size: 12px;
    color: var(--text-mute);
    
  }
  .dot-row { display: inline-flex; gap: 4px; }
  .dot-row .dot {
    width: 5px; height: 5px;
    border-radius: 50%;
    background: var(--app-tone, var(--src-claude));
    opacity: 0.6;
    animation: ct-pulse 1.2s infinite;
  }
  .dot-row .dot:nth-child(2) { animation-delay: 0.18s; }
  .dot-row .dot:nth-child(3) { animation-delay: 0.36s; }
  @keyframes ct-pulse {
    0%, 80%, 100% { opacity: 0.3; transform: scale(0.85); }
    40%           { opacity: 1;   transform: scale(1.15); }
  }

  .msg-usage {
    margin-top: 8px;
    font-size: 10px; color: var(--text-mute);
  }

  .action-wrap { width: 100%; }


  .ct-empty, .ct-welcome {
    margin: auto;
    text-align: center;
    padding: 60px 20px;
    max-width: 480px;
  }
  .ct-empty-h, .ct-welcome-h {
    font-family: 'Geist', 'Inter', -apple-system, system-ui, sans-serif;
    font-size: 26px; font-weight: 600; letter-spacing: -0.015em;
    color: var(--text-0);
    margin: 0 0 10px;
  }
  .ct-empty-p, .ct-welcome-p {
    font-size: 13px; color: var(--text-2);
    line-height: 1.55; margin: 0;
  }
  .ct-welcome-p .mono {
    font-family: 'JetBrains Mono', monospace; font-size: 11.5px;
    padding: 1px 5px; background: var(--bg-2); border: 1px solid var(--border);
    border-radius: 4px;
    color: var(--accent-bright);
  }
</style>
