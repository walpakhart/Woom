<script lang="ts">
  /* Composer — bottom row: model picker + textarea + send.
     v8: model picker moved INTO the composer (was in ChatHeader),
     context-usage ring next to the token counter, Claude five-hour
     and weekly quota chips on the right edge for plan-aware users,
     no inner scroll on the textarea (auto-grows up to 70% of the
     viewport), inline @ autocomplete (sessions / Jira / GH / Sentry)
     anchored to the textarea, and OS / Editor drag drops accepted
     into the input as @-mentions. */
  import { sessionsState, setSessionInput, updateSession } from '$lib/state/sessions.svelte';
  import { quotaState, refreshPlanUsage } from '$lib/state/quota.svelte';
  import { isImagePath } from '$lib/format';
  import { convertFileSrc } from '@tauri-apps/api/core';
  import Dropdown from '$lib/components/ui/Dropdown.svelte';
  import MentionPicker from './MentionPicker.svelte';
  import { onMount } from 'svelte';
  import type { Mention } from '$lib/types';

  type Kind = 'claude' | 'cursor';

  interface Props {
    kind: Kind;
    onSend: () => void;
    onStop: () => void;
    onPasteImages: (
      kind: Kind,
      blobs: { name: string; type: string; blob: Blob }[]
    ) => Promise<number>;
    /** OS / inbox drag-drop. The composer surfaces the dragover hint
     *  itself; the parent owns the drop handler so it can plug into
     *  the existing `attachPathsToSession` / inbox-mention pipeline. */
    onDragOver?: (e: DragEvent) => void;
    onDrop?: (e: DragEvent) => void;
    onDragLeave?: (e: DragEvent) => void;
  }
  let p: Props = $props();

  const sess = $derived(
    sessionsState.list.find((s) => s.id === sessionsState.activeIds[p.kind]) ?? null
  );

  let ta: HTMLTextAreaElement | null = $state(null);
  let shellEl: HTMLDivElement | null = $state(null);

  /* Auto-grow with no inner scrollbar. When the textarea is empty,
     `scrollHeight` in WebKit includes the placeholder text — and a
     long placeholder in a narrow grid cell wraps to 3-4 lines, which
     would inflate the composer to ~100px tall on every fresh chat.
     Skip the measurement on empty input and let the CSS `min-height`
     handle the resting size instead. */
  function autoGrow() {
    if (!ta) return;
    ta.style.height = 'auto';
    if (!ta.value) {
      ta.style.height = '';
      ta.style.overflowY = 'hidden';
      return;
    }
    const cap = Math.floor(window.innerHeight * 0.7);
    const next = Math.min(ta.scrollHeight, cap);
    ta.style.height = next + 'px';
    ta.style.overflowY = ta.scrollHeight > cap ? 'auto' : 'hidden';
  }

  function onInput(e: Event) {
    if (!sess) return;
    const v = (e.currentTarget as HTMLTextAreaElement).value;
    setSessionInput(sess.id, v);
    autoGrow();
    detectMentionTrigger();
  }

  function onKey(e: KeyboardEvent) {
    if (!sess) return;
    /* Forward arrow / enter / escape to the mention picker first when
       it's open, so navigation doesn't fight with text input. */
    if (mentionOpen) {
      if (e.key === 'Escape') {
        e.preventDefault();
        closeMention();
        return;
      }
      if (e.key === 'ArrowDown' || e.key === 'ArrowUp' || e.key === 'Enter') {
        /* The picker has its own window-level keydown listener. We
           just need to NOT submit the form on Enter while it's open. */
        if (e.key === 'Enter') e.preventDefault();
        return;
      }
    }
    if (e.key === 'Enter' && !e.shiftKey && !e.metaKey && !e.ctrlKey) {
      e.preventDefault();
      doSend();
    } else if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) {
      e.preventDefault();
      doSend();
    }
  }

  function doSend() {
    if (!sess || !sess.input.trim()) return;
    /* Queue while a turn is in flight: parent's onSend handler sees
       `sending=true` and pushes input → pendingQueue, clears the
       composer. The drain in the send pipeline auto-fires the next
       queued message when the current turn finishes. So we don't
       short-circuit here on `sending` anymore — we let the parent
       decide whether to dispatch or queue. */
    p.onSend();
    queueMicrotask(autoGrow);
  }

  async function onPaste(e: ClipboardEvent) {
    if (!sess || !e.clipboardData) return;
    const blobs: { name: string; type: string; blob: Blob }[] = [];
    for (const it of Array.from(e.clipboardData.items)) {
      if (it.kind === 'file' && it.type.startsWith('image/')) {
        const f = it.getAsFile();
        if (f) blobs.push({ name: f.name || 'pasted.png', type: f.type, blob: f });
      }
    }
    if (blobs.length === 0) return;
    e.preventDefault();
    await p.onPasteImages(p.kind, blobs);
  }

  /* ─── Mention picker state + helpers ──────────────────────────── */

  /** Position rect for the picker — null when closed. */
  let mentionAnchor = $state<{ left: number; top: number; width: number } | null>(null);
  /** Substring after the most recent @ that still has caret focus
   *  inside it — feeds the picker's filter. */
  let mentionQuery = $state('');
  /** Index in the input where the @ trigger started. Used to splice
   *  the chosen mention back in cleanly. */
  let mentionFrom = $state(-1);
  const mentionOpen = $derived(mentionAnchor !== null);

  /** Re-evaluate whether the caret is currently inside an @-trigger
   *  span. Called on every input event. We treat the most recent
   *  unescaped @ before the caret as the trigger; mention closes when
   *  whitespace appears between the @ and the caret. */
  function detectMentionTrigger() {
    if (!ta || !sess) return;
    const value = ta.value ?? '';
    const caret = ta.selectionStart ?? value.length;
    /* Find the last @ in [0, caret] that's either at index 0 or
       preceded by whitespace. */
    let at = -1;
    for (let i = caret - 1; i >= 0; i--) {
      const c = value[i];
      if (c === '@') {
        if (i === 0 || /\s/.test(value[i - 1])) at = i;
        break;
      }
      if (/\s/.test(c)) break;
    }
    if (at < 0) {
      closeMention();
      return;
    }
    const q = value.slice(at + 1, caret);
    if (q.includes('\n')) {
      closeMention();
      return;
    }
    mentionQuery = q;
    mentionFrom = at;
    /* Anchor the popover to the textarea's left edge, slightly above. */
    const rect = ta.getBoundingClientRect();
    mentionAnchor = {
      left: rect.left,
      top: rect.top,
      width: Math.min(rect.width, 480)
    };
  }
  function closeMention() {
    mentionAnchor = null;
    mentionQuery = '';
    mentionFrom = -1;
  }

  /** Selected from the picker — splice the display text in place of
   *  `@<query>` and append the mention payload to the session. */
  function pickMention(s: { display: string; mention: Mention }) {
    if (!sess || !ta || mentionFrom < 0) return;
    const value = ta.value ?? '';
    const caret = ta.selectionStart ?? value.length;
    const before = value.slice(0, mentionFrom);
    const after = value.slice(caret);
    const next = before + s.display + after;
    setSessionInput(sess.id, next);
    /* De-dupe by externalId so picking the same mention twice doesn't
       double up the context payload. */
    const dedupedMentions = sess.mentions.filter(
      (m) => !(m.source === s.mention.source && m.externalId === s.mention.externalId)
    );
    updateSession(sess.id, { mentions: [...dedupedMentions, s.mention] });
    closeMention();
    queueMicrotask(() => {
      if (!ta) return;
      ta.focus();
      const pos = (before + s.display).length;
      ta.setSelectionRange(pos, pos);
      autoGrow();
    });
  }

  /** Click on the @ icon — insert @ at caret + force the picker open
   *  with an empty query. The user gets the same UX as if they just
   *  typed @ at that spot. */
  function clickMention() {
    if (!sess) return;
    const value = sess.input ?? '';
    const start = ta?.selectionStart ?? value.length;
    const end = ta?.selectionEnd ?? value.length;
    const before = value.slice(0, start);
    const after = value.slice(end);
    const sep = before.length > 0 && !/\s$/.test(before) ? ' ' : '';
    const next = before + sep + '@' + after;
    setSessionInput(sess.id, next);
    queueMicrotask(() => {
      if (!ta) return;
      ta.focus();
      const pos = (before + sep + '@').length;
      ta.setSelectionRange(pos, pos);
      autoGrow();
      detectMentionTrigger();
    });
  }

  /* Drag-drop visual hint — dim the shell while a payload is over us. */
  let dragOver = $state(false);
  function onShellDragEnter(e: DragEvent) {
    if (!hasDropPayload(e)) return;
    e.preventDefault();
    dragOver = true;
  }
  function onShellDragOver(e: DragEvent) {
    if (!hasDropPayload(e)) return;
    e.preventDefault();
    dragOver = true;
    p.onDragOver?.(e);
  }
  function onShellDragLeave(e: DragEvent) {
    /* Only clear when the drag truly leaves the outer drop target.
       `dragleave` fires for child enters too — we use the bounding
       client rect for a coarse "outside the shell?" check since
       relatedTarget can be null in some browsers. */
    const x = e.clientX, y = e.clientY;
    const r = shellEl?.parentElement?.getBoundingClientRect();
    if (r && (x < r.left || x > r.right || y < r.top || y > r.bottom)) {
      dragOver = false;
      p.onDragLeave?.(e);
    }
  }
  function onShellDrop(e: DragEvent) {
    dragOver = false;
    p.onDrop?.(e);
  }
  function hasDropPayload(e: DragEvent): boolean {
    const types = e.dataTransfer?.types;
    if (!types) return false;
    return (
      types.indexOf('Files') !== -1 ||
      types.indexOf('text/uri-list') !== -1 ||
      types.indexOf('application/x-woom-file') !== -1 ||
      types.indexOf('application/x-woom-jira') !== -1 ||
      types.indexOf('application/x-woom-github') !== -1 ||
      types.indexOf('application/x-woom-sentry') !== -1
    );
  }

  $effect(() => {
    void sess?.id;
    autoGrow();
  });

  onMount(() => {
    if (p.kind === 'claude') void refreshPlanUsage();
  });

  /* Per-model context window. Anthropic ships different ceilings per
     model — surfacing the wrong number means the ring shows 100% on
     models that actually have 5× the headroom. Numbers tracked
     against Anthropic's published limits as of late-2025; if a new
     model lands, fall through to the `200_000` Sonnet/Haiku default. */
  function modelContextLimit(model: string | null | undefined): number {
    if (!model) return 200_000;
    /* Opus 4.x ships with a 1M-token window by default (the same
       extended-context tier Sonnet 4.5 has). */
    if (model.startsWith('claude-opus-4')) return 1_000_000;
    if (model.startsWith('claude-sonnet-4-6')) return 200_000;
    if (model.startsWith('claude-sonnet')) return 1_000_000;
    if (model.startsWith('claude-haiku')) return 200_000;
    /* Cursor models inherit the Anthropic limits when proxied. */
    if (model.includes('opus-4')) return 1_000_000;
    if (model.includes('sonnet-4-6')) return 200_000;
    return 200_000;
  }
  const tokenLimit = $derived(
    p.kind === 'claude'
      ? modelContextLimit(sess?.claudeModel ?? null)
      : modelContextLimit(sess?.cursorModel ?? null)
  );
  const inputTokens = $derived(
    sess?.input ? Math.ceil(sess.input.length / 4) : 0
  );
  const contextTokens = $derived.by(() => {
    /* Walk in reverse to find the LATEST stamped usage — the live
       context size, not a cumulative max. /compact and similar ops
       can shrink it, and the user wants to see that shrink reflected
       in the ring instead of seeing a stale ceiling. */
    const msgs = sess?.messages ?? [];
    for (let i = msgs.length - 1; i >= 0; i--) {
      const u = msgs[i]?.usage;
      if (u?.contextSize) return u.contextSize + inputTokens;
    }
    let n = inputTokens;
    if (sess?.mentions) {
      for (const m of sess.mentions) n += Math.ceil((m.title?.length ?? 0) / 4) + 8;
    }
    return n;
  });
  const ctxPct = $derived(
    Math.max(0, Math.min(100, Math.round((contextTokens / tokenLimit) * 100)))
  );
  const ctxLabel = $derived(
    contextTokens >= 1000
      ? `${(contextTokens / 1000).toFixed(1)}k`
      : `${contextTokens}`
  );
  const RING_C = 50.27;
  const ctxRingOffset = $derived(RING_C * (1 - ctxPct / 100));

  const fiveHour = $derived(
    p.kind === 'claude' ? quotaState.usage?.five_hour ?? null : null
  );
  const sevenDay = $derived(
    p.kind === 'claude' ? quotaState.usage?.seven_day ?? null : null
  );

  /* Real model ids only — Claude CLI rejects the run with "model
     does not exist" if we pass anything it can't resolve. The 1M /
     legacy variants we sketched earlier weren't actual ids on
     anthropic's side, so they're gone. */
  const claudeModels = [
    { value: 'claude-opus-4-7', label: 'Opus 4.7' },
    { value: 'claude-sonnet-4-6', label: 'Sonnet 4.6' },
    { value: 'claude-haiku-4-5-20251001', label: 'Haiku 4.5' }
  ];
  const cursorModels = [
    { value: 'sonnet-4-6', label: 'Sonnet 4.6' },
    { value: 'opus-4-7', label: 'Opus 4.7' },
    { value: 'gpt-5.1', label: 'GPT 5.1' }
  ];
  const claudeEffort = [
    { value: 'auto', label: 'Effort · auto' },
    { value: 'low', label: 'Effort · low' },
    { value: 'medium', label: 'Effort · medium' },
    { value: 'high', label: 'Effort · high' },
    { value: 'max', label: 'Effort · max' }
  ];

  function setModel(v: string | null) {
    if (!sess) return;
    if (p.kind === 'claude') updateSession(sess.id, { claudeModel: v });
    else updateSession(sess.id, { cursorModel: v });
  }
  function setEffort(v: string | null) {
    if (!sess) return;
    /* Stash effort on the session so future runs include it. We
       don't have a typed slot yet, so persist on the existing
       session shape via `updateSession` — schema already takes
       arbitrary patches. */
    updateSession(sess.id, { thinkingEffort: v ?? null } as never);
  }

  function fmtPct(b: { utilization: number | null } | null): string {
    if (!b || b.utilization == null) return '—';
    return `${Math.round(b.utilization)}%`;
  }
  function pctClass(b: { utilization: number | null } | null): string {
    if (!b || b.utilization == null) return '';
    if (b.utilization >= 90) return 'cmp-q--err';
    if (b.utilization >= 70) return 'cmp-q--warn';
    return '';
  }

  /* Attachments — only files / images dragged or pasted from OUTSIDE the
     app. In-app @-mentions (picker, editor-tree drag, line ranges) are
     inline `@token` references in the prompt text, so they don't appear
     here. */
  const attachments = $derived.by(() => {
    if (!sess) return [] as { mention: Mention; isImage: boolean; fileSrc?: string }[];
    return sess.mentions
      .filter((m) => m.attached || (m.source === 'file' && !m.isDir && !!m.body && isImagePath(m.body)))
      .map((m) => {
        const isImage = m.source === 'file' && !!m.body && isImagePath(m.body);
        return {
          mention: m,
          isImage,
          fileSrc: isImage && m.body ? convertFileSrc(m.body) : undefined
        };
      });
  });

  /* Render the input text into a styled HTML mirror that lives BEHIND
     the textarea. `@token` runs become tinted chip spans so the user
     sees their mentions highlighted in-place while typing. The
     textarea itself stays interactive but transparent — the backdrop
     provides the visible glyphs. Newline runs need a trailing space
     so the final blank line still reserves a row in the backdrop's
     box (HTML eats the trailing \n otherwise). */
  function escHtml(s: string): string {
    return s.replace(/[&<>"']/g, (c) =>
      c === '&' ? '&amp;' : c === '<' ? '&lt;' : c === '>' ? '&gt;' : c === '"' ? '&quot;' : '&#39;'
    );
  }
  function backdropHtml(text: string): string {
    /* Match @-tokens at start / after whitespace, same shape as
       `pruneMentionsByInput`'s regex so the highlight aligns with
       what the prompt builder treats as a mention. */
    const re = /(^|\s)@([^\s@]+)/g;
    let out = '';
    let i = 0;
    let m: RegExpExecArray | null;
    while ((m = re.exec(text)) !== null) {
      const idx = m.index + m[1].length;
      out += escHtml(text.slice(i, idx));
      out += `<span class="cmp-area-mention">@${escHtml(m[2])}</span>`;
      i = idx + 1 + m[2].length;
    }
    out += escHtml(text.slice(i));
    if (text.endsWith('\n')) out += ' ';
    return out;
  }
  let backdropEl: HTMLDivElement | null = $state(null);
  function syncBackdropScroll() {
    if (!ta || !backdropEl) return;
    backdropEl.scrollTop = ta.scrollTop;
    backdropEl.scrollLeft = ta.scrollLeft;
  }

  function removeAttachment(m: Mention) {
    if (!sess) return;
    const next = sess.mentions.filter(
      (x) => !(x.source === m.source && x.externalId === m.externalId)
    );
    /* Non-image attachments also have a `@token` in the input. Strip
       it so the visible textarea matches the chip strip. Image
       attachments don't write a token (their path can have spaces),
       so the input is left untouched. */
    let nextInput = sess.input;
    const isImage = m.source === 'file' && !!m.body && isImagePath(m.body);
    if (!isImage) {
      const token = m.externalId;
      const escaped = token.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
      const re = new RegExp(`(^|\\s)@${escaped}(\\s|$)`, 'g');
      nextInput = nextInput.replace(re, (_, pre, post) => pre + (post === '' ? '' : ' ')).replace(/\s+$/, '');
    }
    updateSession(sess.id, { mentions: next, input: nextInput });
    queueMicrotask(autoGrow);
  }
</script>

{#if sess}
  <div
    class="cmp"
    ondragenter={onShellDragEnter}
    ondragover={onShellDragOver}
    ondragleave={onShellDragLeave}
    ondrop={onShellDrop}
    role="region"
    aria-label="Composer drop target"
  >
    <div
      bind:this={shellEl}
      class="cmp-shell"
      class:cmp-shell--filled={(sess.input?.length ?? 0) > 0}
      class:cmp-shell--drop={dragOver}
    >
      {#if attachments.length > 0}
        <div class="cmp-attach">
          {#each attachments as a (a.mention.source + ':' + a.mention.externalId)}
            {#if a.isImage && a.fileSrc}
              <span class="cmp-attach-img" title={a.mention.title}>
                <img src={a.fileSrc} alt={a.mention.title} loading="lazy" />
                <button class="cmp-attach-x" type="button" onclick={() => removeAttachment(a.mention)} aria-label="Remove attachment" title="Remove">
                  <svg viewBox="0 0 24 24" width="11" height="11" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>
                </button>
              </span>
            {:else}
              <span class="cmp-attach-file mono" title={a.mention.title}>
                <svg viewBox="0 0 24 24" width="12" height="12" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><polyline points="14 2 14 8 20 8"/></svg>
                <span class="cmp-attach-name">{a.mention.title}</span>
                <button class="cmp-attach-x cmp-attach-x--inline" type="button" onclick={() => removeAttachment(a.mention)} aria-label="Remove attachment" title="Remove">
                  <svg viewBox="0 0 24 24" width="10" height="10" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>
                </button>
              </span>
            {/if}
          {/each}
        </div>
      {/if}

      <div class="cmp-row">
        <div class="cmp-prefix">
          <button class="cmp-iconbtn" title="Attach file">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7"><path d="M21.44 11.05 12.25 20.24a6 6 0 1 1-8.49-8.49l9.19-9.19a4 4 0 1 1 5.66 5.66l-9.2 9.19a2 2 0 0 1-2.83-2.83l8.49-8.49"/></svg>
          </button>
          <button class="cmp-iconbtn" title="@ mention" onclick={clickMention}>
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7"><circle cx="12" cy="12" r="4"/><path d="M16 8v5a3 3 0 0 0 6 0v-1a10 10 0 1 0-3.92 7.94"/></svg>
          </button>
        </div>

        <div class="cmp-area-wrap">
          <div class="cmp-area-backdrop" bind:this={backdropEl} aria-hidden="true">{@html backdropHtml(sess.input ?? '')}</div>
          <textarea
            bind:this={ta}
            class="cmp-area"
            value={sess.input}
            oninput={onInput}
            onkeydown={onKey}
            onpaste={onPaste}
            onclick={detectMentionTrigger}
            onkeyup={detectMentionTrigger}
            onscroll={syncBackdropScroll}
            placeholder={sess.sending
              ? (p.kind === 'claude'
                  ? 'Type to queue — fires after the current Claude turn finishes.'
                  : 'Type to queue — fires after the current Cursor turn finishes.')
              : (p.kind === 'claude'
                  ? 'Ask Claude anything…  Drop a Jira card / PR / file to attach context.'
                  : 'Ask Cursor…  Drop a Jira card / PR / file to attach context.')}
            rows="1"
          ></textarea>
        </div>

        <div class="cmp-suffix">
          <span class="cmp-ctx" title="Context window: {contextTokens.toLocaleString()} / {tokenLimit.toLocaleString()} tokens">
            <svg class="cmp-ring" viewBox="0 0 20 20" aria-hidden="true">
              <circle class="cmp-ring-bg" cx="10" cy="10" r="8"/>
              <circle
                class="cmp-ring-fg"
                class:cmp-ring--warn={ctxPct >= 70 && ctxPct < 90}
                class:cmp-ring--err={ctxPct >= 90}
                cx="10" cy="10" r="8"
                stroke-dasharray={RING_C}
                stroke-dashoffset={ctxRingOffset}
              />
            </svg>
            <span class="cmp-ctx-label mono">{ctxLabel}</span>
          </span>

          {#if p.kind === 'claude' && (fiveHour || sevenDay)}
            <span class="cmp-quotas">
              {#if fiveHour}
                <span class="cmp-q {pctClass(fiveHour)}" title="5-hour rolling usage">
                  <span class="cmp-q-tag mono">5h</span>
                  <span class="cmp-q-val mono">{fmtPct(fiveHour)}</span>
                </span>
              {/if}
              {#if sevenDay}
                <span class="cmp-q {pctClass(sevenDay)}" title="7-day weekly usage">
                  <span class="cmp-q-tag mono">7d</span>
                  <span class="cmp-q-val mono">{fmtPct(sevenDay)}</span>
                </span>
              {/if}
            </span>
          {/if}

          <span class="cmp-model">
            {#if p.kind === 'claude'}
              <Dropdown
                value={sess.claudeModel ?? 'claude-sonnet-4-6'}
                options={claudeModels}
                onChange={setModel}
                placeholder="model"
                ariaLabel="Claude model"
                forceUp={true}
              />
              <Dropdown
                value={(sess as unknown as { thinkingEffort?: string | null }).thinkingEffort ?? 'auto'}
                options={claudeEffort}
                onChange={setEffort}
                placeholder="effort"
                ariaLabel="Thinking effort"
                forceUp={true}
              />
            {:else}
              <Dropdown
                value={sess.cursorModel ?? 'sonnet-4-6'}
                options={cursorModels}
                onChange={setModel}
                placeholder="model"
                ariaLabel="Cursor model"
                forceUp={true}
              />
            {/if}
          </span>

          {#if sess.sending}
            <button class="cmp-stop" onclick={p.onStop} title="Stop the running turn">
              <svg viewBox="0 0 24 24" fill="currentColor"><rect x="6" y="6" width="12" height="12" rx="2"/></svg>
              Stop
            </button>
            <button
              class="cmp-send cmp-send--queue"
              onclick={doSend}
              disabled={!sess.input?.trim()}
              title="Queue this message — fires automatically when the current turn finishes"
            >
              {(sess.pendingQueue?.length ?? 0) > 0 ? `Queue · ${(sess.pendingQueue?.length ?? 0) + 1}` : 'Queue'}
              <span class="cmp-send-kbd">⏎</span>
            </button>
          {:else}
            {#if (sess.pendingQueue?.length ?? 0) > 0}
              <span class="cmp-queue-indicator" title="Messages waiting to be sent after the current turn finishes">
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"><path d="M3 6h18M3 12h18M3 18h12"/></svg>
                {sess.pendingQueue?.length}
              </span>
            {/if}
            <button class="cmp-send" onclick={doSend} disabled={!sess.input?.trim()}>
              Send
              <span class="cmp-send-kbd">⏎</span>
            </button>
          {/if}
        </div>
      </div>
    </div>
  </div>

  {#if mentionOpen}
    <MentionPicker
      anchor={mentionAnchor}
      query={mentionQuery}
      onPick={pickMention}
      onClose={closeMention}
    />
  {/if}
{/if}

<style>
  .cmp {
    flex: 0 0 auto;
    padding: 12px 22px 18px;
    background: linear-gradient(0deg, var(--bg-2) 30%, var(--bg-1));
    border-top: 1px solid var(--border);
  }
  .cmp-shell {
    background: var(--bg-1);
    border: 1px solid var(--border-hi);
    border-radius: 12px;
    padding: 8px 12px 8px 14px;
    box-shadow: 0 0 0 0 var(--accent-glow);
    transition: box-shadow 200ms, border-color 200ms;
  }
  .cmp-shell:focus-within {
    border-color: var(--border-accent);
    box-shadow: 0 0 0 3px var(--accent-soft), 0 0 22px var(--accent-glow);
  }
  /* Drop target hint — terracotta dashed outline + soft glow while
     the user is dragging a file / ticket / PR / error onto us. */
  .cmp-shell--drop {
    border-color: var(--accent-bright);
    border-style: dashed;
    box-shadow: 0 0 0 4px var(--accent-soft), 0 0 28px var(--accent-glow);
  }

  /* Attachments strip — externals only (OS drag, paste). Each chip is
     removable via an × overlay. Image attachments preview as small
     thumbnails with the × pinned to the top-right; non-image files
     fall back to a label chip with a trailing ×. */
  .cmp-attach {
    display: flex; gap: 6px; flex-wrap: wrap;
    padding: 4px 0 8px;
    border-bottom: 1px dashed var(--border);
    margin-bottom: 6px;
  }
  .cmp-attach-img {
    position: relative;
    display: inline-block;
    border-radius: 6px;
    overflow: hidden;
    border: 1px solid var(--border);
    background: var(--bg-3);
  }
  .cmp-attach-img img {
    display: block;
    height: 44px;
    max-width: 84px;
    object-fit: cover;
  }
  .cmp-attach-file {
    display: inline-flex; align-items: center; gap: 6px;
    padding: 4px 4px 4px 8px;
    border-radius: 6px;
    background: var(--bg-3);
    border: 1px solid var(--border);
    color: var(--text-1);
    font-size: 11px;
    max-width: 200px;
  }
  .cmp-attach-file svg { color: var(--text-mute); flex-shrink: 0; }
  .cmp-attach-name {
    color: var(--text-0);
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .cmp-attach-x {
    position: absolute;
    top: 2px; right: 2px;
    width: 16px; height: 16px;
    display: grid; place-items: center;
    border: none;
    border-radius: 50%;
    background: rgba(20, 14, 10, 0.65);
    color: rgba(255, 255, 255, 0.92);
    cursor: pointer;
    padding: 0;
    backdrop-filter: blur(2px);
    transition: background 120ms, transform 120ms;
  }
  .cmp-attach-x:hover { background: rgba(232, 130, 100, 0.92); transform: scale(1.06); }
  .cmp-attach-x--inline {
    position: static;
    width: 16px; height: 16px;
    background: transparent;
    color: var(--text-mute);
    border-radius: 4px;
    backdrop-filter: none;
  }
  .cmp-attach-x--inline:hover {
    background: color-mix(in srgb, var(--error) 22%, transparent);
    color: var(--error);
    transform: none;
  }

  /* Row layout. `align-items: center` keeps the icons, textarea text,
     dropdowns and Send button on a single visual line for the common
     1-line case. When the textarea grows (multi-line draft) the
     prefix/suffix remain vertically centered against the taller
     content — feels natural and avoids the "icons glued to the
     floor" look that `align-items: end` produced. */
  .cmp-row {
    display: grid;
    grid-template-columns: auto 1fr auto;
    align-items: center;
    gap: 10px;
    min-height: 32px;
  }
  .cmp-prefix {
    display: flex; align-items: center; gap: 4px;
  }
  .cmp-iconbtn {
    width: 24px; height: 24px;
    display: grid; place-items: center;
    border-radius: 6px;
    color: var(--text-2);
    background: transparent;
    border: 0;
    cursor: pointer;
    transition: all 120ms;
  }
  .cmp-iconbtn:hover { background: var(--bg-3); color: var(--text-0); }
  .cmp-iconbtn svg { width: 14px; height: 14px; }

  /* Textarea + highlighted backdrop overlay. The backdrop renders the
     same text as the textarea but with `@token` runs wrapped in tinted
     chip spans, so mentions stand out while typing. Textarea sits on
     top with transparent text + visible caret — it owns interaction;
     the backdrop owns appearance. Both share IDENTICAL font / padding /
     line-height / wrapping so the chip outlines line up under the
     caret pixel-perfect. */
  .cmp-area-wrap {
    position: relative;
    width: 100%;
    min-width: 0;
    /* Don't stretch to the row's track height — match the textarea's
       intrinsic size so the placeholder/text glyph baseline lines up
       with the prefix icons and suffix chips/buttons. */
    align-self: center;
    display: flex;
  }
  .cmp-area-backdrop {
    position: absolute; inset: 0;
    padding: 5px 0;
    font-family: inherit;
    font-size: 14px; line-height: 1.55;
    color: var(--text-0);
    white-space: pre-wrap;
    word-break: break-word;
    overflow: hidden;
    pointer-events: none;
    user-select: none;
  }
  /* Inline @-mention chip — soft accent tint, no border (would shift
     glyph metrics out of sync with the textarea). */
  .cmp-area-backdrop :global(.cmp-area-mention) {
    background: color-mix(in srgb, var(--accent) 18%, transparent);
    color: var(--accent-bright);
    border-radius: 4px;
    padding: 0 2px;
    margin: 0 -1px;
    font-weight: 500;
  }
  .cmp-area {
    position: relative;
    width: 100%;
    resize: none; outline: none; border: none;
    background: transparent;
    color: transparent;
    -webkit-text-fill-color: transparent;
    caret-color: var(--text-0);
    font-family: inherit;
    font-size: 14px; line-height: 1.55;
    padding: 5px 0;
    min-height: 24px;
    overflow: hidden;
    scrollbar-width: none;
  }
  .cmp-area::-webkit-scrollbar { display: none; }
  .cmp-area::placeholder {
    color: var(--text-mute);
    -webkit-text-fill-color: var(--text-mute);
  }
  .cmp-area::selection {
    background: var(--accent-soft);
    color: var(--text-0);
    -webkit-text-fill-color: var(--text-0);
  }
  .cmp-area:disabled { opacity: 0.5; cursor: not-allowed; }

  .cmp-suffix {
    display: flex; align-items: center; gap: 8px;
  }

  .cmp-ctx {
    display: inline-flex; align-items: center; gap: 5px;
    color: var(--text-mute);
    flex-shrink: 0;
  }
  .cmp-ring { width: 18px; height: 18px; transform: rotate(-90deg); }
  .cmp-ring-bg { fill: none; stroke: var(--border); stroke-width: 2; }
  .cmp-ring-fg {
    fill: none;
    stroke: var(--accent-bright);
    stroke-width: 2;
    stroke-linecap: round;
    transition: stroke-dashoffset 240ms ease, stroke 200ms;
  }
  .cmp-ring-fg.cmp-ring--warn { stroke: var(--warning); }
  .cmp-ring-fg.cmp-ring--err  { stroke: var(--error); }
  .cmp-ctx-label { font-size: 10.5px; }

  .cmp-quotas {
    display: inline-flex; align-items: center; gap: 4px;
    flex-shrink: 0;
  }
  .cmp-q {
    display: inline-flex; align-items: center; gap: 4px;
    padding: 2px 7px;
    border-radius: 5px;
    background: var(--bg-3);
    border: 1px solid var(--border);
    font-size: 10px;
    color: var(--text-1);
  }
  .cmp-q-tag {
    color: var(--text-mute);
    font-size: 9px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }
  .cmp-q-val { color: var(--text-0); }
  .cmp-q.cmp-q--warn {
    background: rgba(217, 184, 110, 0.10);
    border-color: rgba(217, 184, 110, 0.32);
    color: var(--warning);
  }
  .cmp-q.cmp-q--warn .cmp-q-val { color: var(--warning); }
  .cmp-q.cmp-q--err {
    background: rgba(232, 130, 100, 0.10);
    border-color: rgba(232, 130, 100, 0.34);
    color: var(--error);
  }
  .cmp-q.cmp-q--err .cmp-q-val { color: var(--error); }

  .cmp-model {
    flex-shrink: 0;
    display: inline-flex; align-items: center; gap: 4px;
  }

  .cmp-send {
    display: inline-flex; align-items: center; gap: 6px;
    padding: 6px 12px;
    border-radius: 7px;
    font-size: 12px; font-weight: 600;
    background: linear-gradient(180deg, var(--accent-bright), var(--accent));
    color: var(--accent-fg);
    border: none; cursor: pointer;
    box-shadow:
      0 2px 8px var(--accent-glow),
      inset 0 1px 0 rgba(255, 255, 255, 0.18);
    transition: transform 120ms, box-shadow 200ms;
  }
  .cmp-send:hover:not(:disabled) {
    transform: translateY(-1px);
    box-shadow: 0 4px 14px var(--accent-glow), inset 0 1px 0 rgba(255, 255, 255, 0.18);
  }
  .cmp-send:disabled { opacity: 0.45; cursor: not-allowed; box-shadow: none; }
  .cmp-send-kbd {
    font-family: 'JetBrains Mono', monospace;
    font-size: 10px;
    padding: 1px 5px;
    border-radius: 4px;
    background: rgba(26, 20, 16, 0.30);
    border: 1px solid rgba(26, 20, 16, 0.40);
    color: var(--accent-fg);
  }

  .cmp-stop {
    display: inline-flex; align-items: center; gap: 5px;
    padding: 6px 11px;
    border-radius: 7px;
    font-size: 12px; font-weight: 500;
    background: rgba(232, 130, 100, 0.10);
    border: 1px solid rgba(232, 130, 100, 0.30);
    color: var(--error);
    cursor: pointer;
    transition: background 140ms;
  }
  .cmp-stop:hover { background: rgba(232, 130, 100, 0.18); }
  .cmp-stop svg { width: 11px; height: 11px; }

  /* "Queue" variant of the send button — appears alongside Stop while
     a turn is in flight. Same shape as Send but tinted neutral so the
     user reads it as "park this for later" instead of "fire now". */
  .cmp-send--queue {
    background: linear-gradient(180deg,
      color-mix(in srgb, var(--accent) 28%, var(--bg-3)),
      color-mix(in srgb, var(--accent) 14%, var(--bg-3)));
    color: var(--text-0);
    box-shadow:
      0 1px 0 rgba(0, 0, 0, 0.10),
      inset 0 1px 0 rgba(255, 255, 255, 0.08);
  }
  .cmp-send--queue:hover:not(:disabled) {
    background: linear-gradient(180deg,
      color-mix(in srgb, var(--accent) 38%, var(--bg-3)),
      color-mix(in srgb, var(--accent) 22%, var(--bg-3)));
  }

  /* Queue indicator — small chip with a stack glyph + count, shown
     between the model picker and Send when there are messages parked
     for after the current turn. Helps the user remember they have
     things lined up. */
  .cmp-queue-indicator {
    display: inline-flex; align-items: center; gap: 4px;
    padding: 4px 8px;
    border-radius: 999px;
    background: color-mix(in srgb, var(--accent) 14%, transparent);
    border: 1px solid color-mix(in srgb, var(--accent) 32%, transparent);
    color: var(--accent-bright);
    font-size: 11px; font-weight: 600;
    user-select: none;
  }
  .cmp-queue-indicator svg { width: 11px; height: 11px; }
</style>
