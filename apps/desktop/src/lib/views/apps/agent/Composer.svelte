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
  import { convertFileSrc, invoke } from '@tauri-apps/api/core';
  import { notify } from '$lib/state/toaster.svelte';
  import Dropdown from '$lib/components/ui/Dropdown.svelte';
  import MentionPicker from './MentionPicker.svelte';
  import { onMount } from 'svelte';
  import type { Mention } from '$lib/types';
  import {
    KNOWN_SLASH_COMMANDS,
    SLASH_COMMAND_DESCRIPTIONS,
    type SlashCommand
  } from '$lib/services/slashCommands';
  import { skillsState, refreshSkills, type Skill } from '$lib/state/skills.svelte';
  import { statuslineState } from '$lib/state/statusline.svelte';
  import {
    claudeEffort,
    claudeModels,
    cursorModels,
    detectTriggerPosition,
    fmtPct,
    modelContextLimit,
    pctClass,
    spliceTriggerInsertion,
  } from './composerHelpers';

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

  function onInput(_e: Event) {
    /* `bind:value={sess.input}` already pushes the new value into the
       session state — calling `setSessionInput` here on top would do
       a redundant assignment AND, more importantly, force Svelte to
       re-apply the textarea's `value` on the next reactive flush.
       That re-apply is exactly what made the caret jitter for cursor
       sessions on fast typing: bind reads → state writes → reactive
       update → DOM-write back → caret gets nudged to the end of the
       value. With bind alone, Svelte tracks that the latest write
       came FROM the input element and skips the echo, so the caret
       stays exactly where the user left it. */
    if (!sess) return;
    autoGrow();
    detectMentionTrigger();
    detectSlashTrigger();
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
    /* Shift+Tab cycles permission mode (default ↔ plan). Matches the
       Claude Code keybinding so muscle memory transfers. Only fires
       when the picker isn't open (Tab is reserved for picker confirm)
       and no IME composition is active. */
    if (
      e.key === 'Tab' &&
      e.shiftKey &&
      !e.metaKey && !e.ctrlKey && !e.altKey &&
      !slashOpen && !mentionOpen && sess
    ) {
      e.preventDefault();
      const next = (sess.permissionMode ?? 'default') === 'plan' ? 'default' : 'plan';
      updateSession(sess.id, { permissionMode: next });
      return;
    }
    /* Slash picker — caret-aware, mirrors @-mention key handling.
       ↑/↓ navigate, Enter/Tab confirm the selection (splice into the
       input at the trigger position), Escape closes the picker
       without touching the input. Typing whitespace closes naturally
       via detectSlashTrigger. */
    if (slashOpen) {
      const totalRows = slashMatches.length + skillMatches.length;
      if (e.key === 'ArrowDown') {
        e.preventDefault();
        slashSelectedIdx = Math.min(slashSelectedIdx + 1, totalRows - 1);
        return;
      }
      if (e.key === 'ArrowUp') {
        e.preventDefault();
        slashSelectedIdx = Math.max(slashSelectedIdx - 1, 0);
        return;
      }
      if (e.key === 'Tab' || e.key === 'Enter') {
        e.preventDefault();
        confirmPickerSelection();
        return;
      }
      if (e.key === 'Escape') {
        e.preventDefault();
        closeSlash();
        return;
      }
    }
    /* Bash-style prompt history. ↑/↓ on the textarea cycle through
       previously-sent prompts when (a) we're already in history mode,
       or (b) the composer is empty / the caret sits on the first
       physical line. Mid-message ↑/↓ stays as normal cursor movement
       so multi-line editing isn't hijacked. */
    if (
      (e.key === 'ArrowUp' || e.key === 'ArrowDown') &&
      !e.shiftKey && !e.metaKey && !e.ctrlKey && !e.altKey &&
      shouldNavigateHistory(e.key)
    ) {
      e.preventDefault();
      navigateHistory(e.key === 'ArrowUp' ? 1 : -1);
      return;
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
    resetHistoryCursor();
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
    if (blobs.length > 0) {
      e.preventDefault();
      await p.onPasteImages(p.kind, blobs);
      return;
    }
    /* Long-text paste trap. When the user pastes a substantial block
     * of text (session summary, error log, design doc, JSON dump),
     * the content currently lives only in the composer — once they
     * send the turn it's buried in this session's transcript and a
     * NEW chat in this solo has no way to reach it. Surface an inline
     * "save as memory" action so the user can capture the paste as a
     * durable note without leaving the composer. Default text-paste
     * behavior (insert into the textarea) is preserved — we just
     * peek at clipboardData and fire a non-blocking toast.
     *
     * 500-char threshold picked to skip command-line one-liners,
     * short snippets, and URLs while catching genuinely-context-laden
     * blocks. The toast auto-dismisses; the user actively clicks the
     * Save chip if they want it captured. */
    const text = e.clipboardData.getData('text/plain');
    if (text && text.length >= 500) {
      const preview = text.slice(0, 60).replace(/\s+/g, ' ').trim();
      const len = text.length;
      notify({
        kind: 'info',
        title: `Long paste — save as memory?`,
        body: `${len.toLocaleString()} chars · "${preview}…"`,
        ttlMs: 8000,
        actions: [
          {
            label: 'Save',
            onClick: async () => {
              try {
                /* Tag with kind=note + a session-id breadcrumb so the
                 * future user can grep "from session: foo" if they
                 * need to trace the origin. The memory_local Tauri
                 * command writes through the same SQLite store the
                 * MCP sidecar serves, so subsequent memory_search
                 * calls from any agent will find this row. */
                const sessId = sess?.id ?? 'unknown';
                await invoke<number>('memory_save_local', {
                  content: text,
                  kind: 'note',
                  tags: ['pasted', `from-session:${sessId.slice(0, 8)}`]
                });
                notify({
                  kind: 'success',
                  title: 'Saved to memory',
                  ttlMs: 2500
                });
              } catch (err) {
                notify({
                  kind: 'error',
                  title: 'Memory save failed',
                  body: String(err)
                });
              }
            }
          }
        ]
      });
    }
  }

  /* ─── Prompt history (↑/↓ on textarea) ────────────────────────────
   * Bash-style recall of previously-sent user messages in THIS session.
   * Persists across restarts for free because we read straight off
   * `sess.messages` — which is already on disk.
   *
   * `historyPos`:
   *   -1 → not navigating (composer holds whatever the user typed)
   *    0 → showing newest past prompt (history[0])
   *    N → showing the (N+1)-th from the end
   *
   * `historyDraft`: text the user had typed BEFORE entering history mode.
   * Restored when ↓ exits at the bottom (historyPos → -1) so a hijacked
   * ↑ never destroys an in-flight draft. */
  let historyPos = $state(-1);
  let historyDraft = $state('');

  /* Derived view of past user prompts — newest first, empty content
   * dropped (e.g. attachment-only turns) so ↑ doesn't show blank slots. */
  const userHistory = $derived.by((): string[] => {
    if (!sess) return [];
    const out: string[] = [];
    for (let i = sess.messages.length - 1; i >= 0; i--) {
      const m = sess.messages[i];
      if (m.role !== 'user') continue;
      const t = m.content?.trim();
      if (!t) continue;
      out.push(m.content);
    }
    return out;
  });

  function shouldNavigateHistory(direction: 'ArrowUp' | 'ArrowDown'): boolean {
    if (userHistory.length === 0) return false;
    /* Already in history mode → always intercept. */
    if (historyPos >= 0) return true;
    /* Empty composer → both arrows are free for history. */
    const v = sess?.input ?? '';
    if (v.length === 0) return true;
    /* Otherwise, only hijack ↑ on the first physical line (caret has
     * no newline before it) and ↓ on the last line — matches what
     * users expect from a multiline shell prompt. */
    if (!ta) return false;
    const caret = ta.selectionStart ?? v.length;
    if (direction === 'ArrowUp') {
      return v.slice(0, caret).indexOf('\n') === -1;
    }
    return v.slice(caret).indexOf('\n') === -1;
  }

  function navigateHistory(step: 1 | -1) {
    if (!sess) return;
    const len = userHistory.length;
    if (len === 0) return;
    /* First entry into history mode: stash the live draft so we can
     * restore it on the way back out. */
    if (historyPos === -1 && step === 1) {
      historyDraft = sess.input ?? '';
    }
    const next = historyPos + step;
    if (next < -1) return;
    if (next >= len) return; /* Already at the oldest — clamp. */
    historyPos = next;
    const text = next === -1 ? historyDraft : userHistory[next];
    setSessionInput(sess.id, text);
    /* Move caret to the end + autoGrow on next tick so the textarea
     * resizes to fit the recalled prompt. */
    queueMicrotask(() => {
      if (!ta) return;
      ta.value = text;
      ta.setSelectionRange(text.length, text.length);
      autoGrow();
    });
  }

  /* Send / submit resets the cursor so the next ↑ starts fresh from
   * the latest prompt (which is the one we just sent). */
  function resetHistoryCursor() {
    historyPos = -1;
    historyDraft = '';
  }

  /* Switching sessions resets the cursor — otherwise position 2 in
   * session A would carry over to session B and load whatever lives
   * there at index 2 (probably nothing remotely related). */
  $effect(() => {
    sess?.id;
    resetHistoryCursor();
  });

  /* ─── Queue panel ──────────────────────────────────────────────── */

  let queueOpen = $state(false);

  function toggleQueue() {
    queueOpen = !queueOpen;
  }

  function removeFromQueue(index: number) {
    if (!sess) return;
    const next = (sess.pendingQueue ?? []).filter((_, i) => i !== index);
    updateSession(sess.id, { pendingQueue: next });
    if (next.length === 0) queueOpen = false;
  }

  function clearQueue() {
    if (!sess) return;
    updateSession(sess.id, { pendingQueue: [] });
    queueOpen = false;
  }

  let queueWrapEl = $state<HTMLDivElement | null>(null);

  $effect(() => {
    if (!queueOpen) return;
    function onDown(e: MouseEvent) {
      if (queueWrapEl && !queueWrapEl.contains(e.target as Node)) {
        queueOpen = false;
      }
    }
    window.addEventListener('mousedown', onDown);
    return () => window.removeEventListener('mousedown', onDown);
  });

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

  /* ---------- Slash picker ───────────────────────────────────────
     Mirrors the @-mention picker: a `/` trigger anywhere in the input
     opens the picker against the substring between `/` and the caret.
     Pick splices `/<name>` at the trigger position (instead of
     replacing the whole input), so the user can compose prose around
     a skill invocation — same UX as @-mentions. Send-path scans for
     a `/<skillname>` token and renders the skill body with the prose
     around it passed as $ARGUMENTS. */

  /** Selected index inside the combined (slash + skill) match list
   *  when the picker is open. Resets to 0 on every match-set change. */
  let slashSelectedIdx = $state(0);
  /** Position rect for the picker — null when closed. */
  let slashAnchor = $state<{ left: number; top: number; width: number } | null>(null);
  /** Substring after the most recent `/` that still has the caret
   *  inside it — feeds the picker's filter. */
  let slashQuery = $state('');
  /** Index in the input where the `/` trigger started. Used to splice
   *  the chosen entry back in cleanly. */
  let slashFrom = $state(-1);

  /** Slash commands that prefix-match the current trigger query. */
  const slashMatches = $derived.by<SlashCommand[]>(() => {
    if (!sess || slashFrom < 0) return [];
    const lower = slashQuery.toLowerCase();
    return KNOWN_SLASH_COMMANDS.filter((c) => c.startsWith(lower));
  });
  /** Live `permissionMode === 'plan'` flag for the toggle pill. */
  const planActive = $derived((sess?.permissionMode ?? 'default') === 'plan');

  /** Skill names that prefix-match the trigger query. Project-scoped
   *  skills sort first (they're already at the head of
   *  `skillsState.list` because discovery walks cwd before user home). */
  const skillMatches = $derived.by<Skill[]>(() => {
    if (!sess || slashFrom < 0) return [];
    const lower = slashQuery.toLowerCase();
    return skillsState.list.filter((sk) => sk.name.toLowerCase().startsWith(lower));
  });
  const slashOpen = $derived(
    slashAnchor !== null && (slashMatches.length > 0 || skillMatches.length > 0)
  );

  /* Discover skills when the session's cwd changes. Cheap (Rust scans
   *  a handful of dirs); `refreshSkills` no-ops if cwd hasn't moved. */
  $effect(() => {
    const cwd = sess?.worktreePath ?? sess?.cwd ?? null;
    void refreshSkills(cwd);
  });

  $effect(() => {
    /* Re-pin the highlight at the top of the list when the filter
       narrows or widens — avoids the highlight pointing at a row
       that's no longer in the matches array. */
    void slashMatches.length;
    slashSelectedIdx = 0;
  });

  function pickSlashCommand(cmd: SlashCommand): void {
    if (!sess || !ta || slashFrom < 0) return;
    const value = ta.value ?? '';
    const caret = ta.selectionStart ?? value.length;
    const { next, caretAfter } = spliceTriggerInsertion(value, caret, slashFrom, `/${cmd} `);
    setSessionInput(sess.id, next);
    closeSlash();
    queueMicrotask(() => {
      if (!ta) return;
      ta.focus();
      ta.selectionStart = caretAfter;
      ta.selectionEnd = caretAfter;
    });
  }

  function pickSkill(sk: Skill): void {
    if (!sess || !ta || slashFrom < 0) return;
    const value = ta.value ?? '';
    const caret = ta.selectionStart ?? value.length;
    const trailing = sk.argument_hint ? ' ' : '';
    const { next, caretAfter } = spliceTriggerInsertion(
      value, caret, slashFrom, `/${sk.name}${trailing}`
    );
    setSessionInput(sess.id, next);
    closeSlash();
    queueMicrotask(() => {
      if (!ta) return;
      ta.focus();
      ta.selectionStart = caretAfter;
      ta.selectionEnd = caretAfter;
    });
  }

  /** Re-evaluate whether the caret is currently inside a `/`-trigger
   *  span. Delegates to `detectTriggerPosition` for the pure
   *  string-scan + applies the result to picker state. */
  function detectSlashTrigger() {
    if (!ta || !sess) return;
    const value = ta.value ?? '';
    const caret = ta.selectionStart ?? value.length;
    const hit = detectTriggerPosition(value, caret, '/');
    if (!hit) { closeSlash(); return; }
    slashQuery = hit.query;
    slashFrom = hit.at;
    const rect = ta.getBoundingClientRect();
    slashAnchor = {
      left: rect.left,
      top: rect.top,
      width: Math.min(rect.width, 480)
    };
  }
  function closeSlash(): void {
    slashAnchor = null;
    slashQuery = '';
    slashFrom = -1;
  }

  /** Confirm the currently highlighted row — slash commands go first
   *  in the visual order, then skills. Index N hits slash[N] if N <
   *  slashMatches.length, else skill[N - slashMatches.length]. */
  function confirmPickerSelection(): void {
    const i = slashSelectedIdx;
    if (i < slashMatches.length) {
      pickSlashCommand(slashMatches[i]);
      return;
    }
    const sk = skillMatches[i - slashMatches.length];
    if (sk) pickSkill(sk);
  }

  /** Re-evaluate whether the caret is currently inside an @-trigger
   *  span. Called on every input event. We treat the most recent
   *  unescaped @ before the caret as the trigger; mention closes when
   *  whitespace appears between the @ and the caret. */
  function detectMentionTrigger() {
    if (!ta || !sess) return;
    const value = ta.value ?? '';
    const caret = ta.selectionStart ?? value.length;
    const hit = detectTriggerPosition(value, caret, '@');
    if (!hit) { closeMention(); return; }
    mentionQuery = hit.query;
    mentionFrom = hit.at;
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

  /* Model catalogues + claudeEffort moved to ./composerHelpers.ts
   * (wave-1 phase-6 split). Edit the lists there when adding new
   * SKUs or changing labels — Composer just renders them. */

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

  /* fmtPct + pctClass moved to ./composerHelpers.ts. */

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
    /* Build a lookup so we can decorate inline tokens with the
       matched mention's source + title. Each @token's first segment
       (before `/`) is conventionally the source ("github", "jira",
       "sentry"…) — fall back on that when the externalId match misses
       (e.g. user typed the token but the picker resolution hasn't
       landed yet). */
    const byExternalId = new Map<string, Mention>();
    for (const mn of sess?.mentions ?? []) {
      byExternalId.set(mn.externalId, mn);
    }
    while ((m = re.exec(text)) !== null) {
      const idx = m.index + m[1].length;
      out += escHtml(text.slice(i, idx));
      const token = m[2];
      const resolved = byExternalId.get(token);
      const sourceFromToken = token.includes('/') ? token.split('/')[0] : '';
      const source = resolved?.source ?? sourceFromToken;
      /* Per-source tinted class. Falls back to the generic mention
         class when the source isn't one we have brand color for —
         keeps the highlight visible even for plain file mentions /
         freshly-typed tokens whose source hasn't been classified. */
      const sourceClass =
        source === 'github' ? 'cmp-area-mention--github'
        : source === 'jira' ? 'cmp-area-mention--jira'
        : source === 'sentry' ? 'cmp-area-mention--sentry'
        : source === 'chat' ? 'cmp-area-mention--chat'
        : '';
      const titleAttr = resolved?.title
        ? ` title="${escHtml(resolved.title)}"`
        : '';
      out += `<span class="cmp-area-mention ${sourceClass}"${titleAttr}>@${escHtml(token)}</span>`;
      i = idx + 1 + token.length;
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
          {#each attachments as a, i (a.mention.source + ':' + a.mention.externalId + '|' + i)}
            {#if a.isImage && a.fileSrc}
              <span class="cmp-attach-img" title={a.mention.title}>
                <img src={a.fileSrc} alt={a.mention.title} loading="lazy" />
                <button class="cmp-attach-x" type="button" onclick={() => removeAttachment(a.mention)} aria-label="Remove attachment" title="Remove">
                  <svg viewBox="0 0 24 24" width="11" height="11" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>
                </button>
              </span>
            {:else}
              <span
                class="cmp-attach-file mono cmp-attach-file--{a.mention.source}"
                title={a.mention.title}
              >
                {#if a.mention.source === 'github'}
                  <!-- Octocat-style outline -->
                  <svg viewBox="0 0 24 24" width="12" height="12" fill="currentColor" aria-hidden="true"><path d="M12 .5C5.65.5.5 5.65.5 12c0 5.09 3.29 9.4 7.86 10.93.58.11.79-.25.79-.56v-2.18c-3.2.7-3.88-1.36-3.88-1.36-.53-1.34-1.29-1.7-1.29-1.7-1.06-.72.08-.71.08-.71 1.17.08 1.79 1.2 1.79 1.2 1.04 1.78 2.73 1.26 3.4.96.1-.75.41-1.26.74-1.55-2.55-.29-5.24-1.28-5.24-5.68 0-1.26.45-2.28 1.19-3.08-.12-.29-.52-1.46.11-3.04 0 0 .97-.31 3.17 1.18a11 11 0 0 1 2.89-.39c.98 0 1.96.13 2.89.39 2.2-1.49 3.17-1.18 3.17-1.18.63 1.58.24 2.75.12 3.04.74.8 1.18 1.82 1.18 3.08 0 4.41-2.69 5.38-5.25 5.67.42.36.79 1.07.79 2.16v3.21c0 .31.21.68.8.56C20.21 21.4 23.5 17.09 23.5 12 23.5 5.65 18.35.5 12 .5z"/></svg>
                {:else if a.mention.source === 'jira'}
                  <!-- Stylised "J" pill -->
                  <svg viewBox="0 0 24 24" width="12" height="12" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="3" y="3" width="18" height="18" rx="3"/><path d="M9 8h7M13 8v6a2.5 2.5 0 0 1-5 0"/></svg>
                {:else if a.mention.source === 'sentry'}
                  <!-- Triangle alert -->
                  <svg viewBox="0 0 24 24" width="12" height="12" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M10.29 3.86 1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z"/><line x1="12" y1="9" x2="12" y2="13"/><line x1="12" y1="17" x2="12.01" y2="17"/></svg>
                {:else if a.mention.source === 'chat'}
                  <!-- Speech bubble -->
                  <svg viewBox="0 0 24 24" width="12" height="12" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/></svg>
                {:else if a.mention.source === 'terminal'}
                  <!-- Terminal prompt -->
                  <svg viewBox="0 0 24 24" width="12" height="12" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><polyline points="4 17 10 11 4 5"/><line x1="12" y1="19" x2="20" y2="19"/></svg>
                {:else}
                  <!-- Generic file outline (default) -->
                  <svg viewBox="0 0 24 24" width="12" height="12" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><polyline points="14 2 14 8 20 8"/></svg>
                {/if}
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
            bind:value={sess.input}
            oninput={onInput}
            onkeydown={onKey}
            onpaste={onPaste}
            onclick={() => { detectMentionTrigger(); detectSlashTrigger(); }}
            onkeyup={() => { detectMentionTrigger(); detectSlashTrigger(); }}
            onscroll={syncBackdropScroll}
            placeholder={sess.sending
              ? (p.kind === 'claude'
                  ? 'Type to queue — fires after the current Claude turn finishes.'
                  : 'Type to queue — fires after the current Cursor turn finishes.')
              : (p.kind === 'claude'
                  ? 'Ask Claude anything…  Drop a Jira card / PR / file to attach context.'
                  : 'Ask Cursor…  Drop a Jira card / PR / file to attach context.')}
            rows="1"
            spellcheck="false"
            autocomplete="off"
            {...{ autocorrect: 'off', autocapitalize: 'off' }}
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

          <!-- SDD button — prefills `/sdd ` into the composer so the
               user can type their ask + hit Enter. Same code path as
               typing the slash command manually (`startSddFromSlash`
               in services/slashCommands.ts); this is purely an
               affordance hint that SDD mode exists. Tooltip explains
               what happens. -->
          <button
            class="cmp-sdd-btn"
            onclick={() => {
              if (!sess) return;
              updateSession(sess.id, { input: '/sdd ' });
              queueMicrotask(() => {
                if (ta) {
                  ta.selectionStart = ta.value.length;
                  ta.selectionEnd = ta.value.length;
                  ta.focus();
                }
              });
            }}
            aria-label="Start a Spec-Driven Development workflow"
            title="SDD — agent writes spec/plan/phases to a temp folder and executes them step-by-step. Won't touch your repo until you approve."
          >
            <span class="cmp-sdd-glyph">SDD</span>
          </button>

          <!-- SDD history moved to ChatHeader chip (next to memory).
               Removed the [HISTORY] composer button on user feedback:
               "I thought history would be where memory is in the
               header so it opens a menu". -->


          <!-- Permission mode toggle. Single button, two states. When
               `default`, renders a quiet dot — barely visible at rest
               so the composer footer doesn't shout. When `plan`, the
               dot fills + amber-glows + the word "plan" appears next
               to it. Click toggles; ⇧⇥ in the textarea also cycles
               (handled in onKey). Tooltip carries the kbd hint so the
               affordance stays discoverable without crowding the UI. -->
          <button
            class="cmp-mode-dot"
            class:cmp-mode-dot--plan={planActive}
            onclick={() => updateSession(sess.id, { permissionMode: planActive ? 'default' : 'plan' })}
            aria-pressed={planActive}
            aria-label={planActive ? 'Plan mode active — click to switch back to default' : 'Default mode — click to enter plan mode'}
            title={planActive
              ? 'Plan mode — agent reads only (no edits, no mutating bash). ⇧⇥ to toggle.'
              : '⇧⇥ for plan mode — flips the agent into read-only investigation.'}
          >
            <span class="cmp-mode-dot-pip" aria-hidden="true"></span>
            {#if planActive}
              <span class="cmp-mode-dot-label">plan</span>
            {/if}
          </button>

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

          {#if (sess.pendingQueue?.length ?? 0) > 0}
            <div class="cmp-queue-wrap" bind:this={queueWrapEl}>
              <button
                class="cmp-queue-indicator"
                class:cmp-queue-indicator--open={queueOpen}
                onclick={toggleQueue}
                title="Show queued messages"
              >
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"><path d="M3 6h18M3 12h18M3 18h12"/></svg>
                {sess.pendingQueue?.length}
              </button>
              {#if queueOpen}
                <div class="cmp-queue-panel">
                  <div class="cmp-queue-panel-head">
                    <span>Queued messages</span>
                    <button class="cmp-queue-clear" onclick={clearQueue}>Clear all</button>
                  </div>
                  {#each sess.pendingQueue ?? [] as msg, i (i)}
                    <div class="cmp-queue-item">
                      <span class="cmp-queue-num">{i + 1}</span>
                      <div class="cmp-queue-text-wrap">
                        {#if msg.mentions.some(m => m.attached)}
                          <span class="cmp-queue-attachments">
                            {#each msg.mentions.filter(m => m.attached) as att}
                              <span class="cmp-queue-att-chip" title={att.body ?? att.title}>
                                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M21.44 11.05l-9.19 9.19a6 6 0 01-8.49-8.49l9.19-9.19a4 4 0 015.66 5.66l-9.2 9.19a2 2 0 01-2.83-2.83l8.49-8.48"/></svg>
                                {att.title}
                              </span>
                            {/each}
                          </span>
                        {/if}
                        {#if msg.text}
                          <span class="cmp-queue-text">{msg.text}</span>
                        {/if}
                      </div>
                      <button
                        class="cmp-queue-del"
                        onclick={() => removeFromQueue(i)}
                        aria-label="Remove"
                      >
                        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M6 6l12 12M6 18L18 6"/></svg>
                      </button>
                    </div>
                  {/each}
                </div>
              {/if}
            </div>
          {/if}

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
              Queue
              <span class="cmp-send-kbd">⏎</span>
            </button>
          {:else}
            <button class="cmp-send" onclick={doSend} disabled={!sess.input?.trim()}>
              Send
              <span class="cmp-send-kbd">⏎</span>
            </button>
          {/if}
        </div>
      </div>
    </div>

    {#if slashOpen}
      <!-- Slash-command picker. Anchored to `.cmp` (position: relative)
           via `position: absolute; bottom: 100%; left: 36px`. Lives
           INSIDE the .cmp wrapper so the absolute positioning resolves
           against the composer footer, not the viewport. Mouse-click
           selects via onmousedown (preventDefault keeps the textarea
           focused); ↑/↓ + Enter/Tab handled by the textarea's onKey
           above. Picker stays open while the input is a single bare
           slash token; a space or no-match dismisses. -->
      <div class="slash-picker" role="listbox" aria-label="Slash command picker">
        {#each slashMatches as cmd, idx (cmd + '|' + idx)}
          <button
            type="button"
            class="slash-item"
            class:slash-item--active={idx === slashSelectedIdx}
            onmousedown={(e) => { e.preventDefault(); pickSlashCommand(cmd); }}
            role="option"
            aria-selected={idx === slashSelectedIdx}
          >
            <span class="slash-item-cmd mono">/{cmd}</span>
            <span class="slash-item-desc">{SLASH_COMMAND_DESCRIPTIONS[cmd]}</span>
          </button>
        {/each}
        {#if skillMatches.length > 0}
          {#if slashMatches.length > 0}
            <div class="slash-picker-sep" aria-hidden="true">skills</div>
          {/if}
          {#each skillMatches as sk, i (sk.id)}
            {@const idx = slashMatches.length + i}
            <button
              type="button"
              class="slash-item slash-item--skill"
              class:slash-item--active={idx === slashSelectedIdx}
              onmousedown={(e) => { e.preventDefault(); pickSkill(sk); }}
              role="option"
              aria-selected={idx === slashSelectedIdx}
              title={sk.path}
            >
              <span class="slash-item-cmd mono">/{sk.name}{sk.argument_hint ? ' ' + sk.argument_hint : ''}</span>
              <span class="slash-item-desc">
                <span class="slash-item-scope mono">{sk.scope}</span>
                {sk.description ?? '(no description)'}
              </span>
            </button>
          {/each}
        {/if}
      </div>
    {/if}
  </div>

  <!-- Statusline strip — renders the user's `statusline.json` script
       output. Hidden when no script configured or output empty.
       Multi-line stdout becomes multi-line (max 4 visible rows; the
       rest scrolls within the strip's max-height). -->
  {#if statuslineState.lastResult && statuslineState.lastResult.stdout.trim().length > 0}
    <div
      class="cmp-statusline"
      class:cmp-statusline--err={!statuslineState.lastResult.ok}
      title={statuslineState.lastResult.ok ? `last ran ${Math.round((Date.now() - statuslineState.lastRanAt) / 1000)}s ago` : (statuslineState.lastResult.stderr || 'statusline error')}
    >{statuslineState.lastResult.stdout.trim()}</div>
  {/if}

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
    padding: 14px 22px;
    background: linear-gradient(0deg, var(--bg-2) 30%, var(--bg-1));
    border-top: 1px solid var(--border);
    /* Stack the composer + statusline strip vertically so the strip
       sits as a sibling below the pill instead of overlapping it.
       Centering is reapplied on the pill itself via margin: auto. */
    display: flex; flex-direction: column; align-items: stretch;
    /* Centre the pill horizontally + vertically inside the footer
       container so the composer sits as a balanced bar instead of
       slumping toward the left edge under wide layouts. */
    display: flex; align-items: center; justify-content: center;
    /* Anchor for absolute-positioned children — slash picker floats
       above the composer pill, not over the chat thread. */
    position: relative;
  }

  /* Slash-command picker. Sits in the composer container, anchored
     to its left edge + bottom + composer pill top. Caret-tracking
     would be nicer but the picker only fires when the input starts
     with `/`, so it always sits under "/", which is always the first
     glyph of the first line — fixed anchor reads as natural. */
  .slash-picker {
    position: absolute;
    left: 36px;
    bottom: calc(100% + 6px);
    min-width: 240px;
    max-width: 380px;
    padding: 4px;
    background: var(--bg-3);
    border: 1px solid var(--border-neutral-hi, var(--border));
    border-radius: 8px;
    box-shadow: var(--shadow-2, 0 12px 32px rgba(0, 0, 0, 0.32));
    z-index: 50;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .slash-item {
    display: flex;
    align-items: baseline;
    gap: 10px;
    padding: 6px 10px;
    background: transparent;
    border: 0;
    border-radius: 5px;
    color: var(--text-0);
    font-size: 12.5px;
    text-align: left;
    cursor: pointer;
    transition: background 120ms;
  }
  .slash-item:hover,
  .slash-item--active {
    background: var(--bg-2);
  }
  .slash-item-cmd {
    flex: 0 0 auto;
    color: var(--accent-bright);
    font-weight: 600;
  }
  .slash-item-desc {
    flex: 1; min-width: 0;
    color: var(--text-mute);
    font-size: 11.5px;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  /* Section separator between built-in commands and skills — small
     all-caps label rendered between the two groups. */
  .slash-picker-sep {
    padding: 6px 10px 2px;
    font-size: 9.5px; font-weight: 700;
    letter-spacing: 0.10em;
    text-transform: uppercase;
    color: var(--text-mute);
    border-top: 1px solid var(--border);
    margin-top: 4px;
  }
  /* Skill items render the same chassis as built-in slash items but
     get a small `user|project` scope chip next to the description. */
  .slash-item-scope {
    display: inline-block;
    font-size: 9px; font-weight: 700;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    padding: 1px 5px;
    margin-right: 6px;
    border-radius: 3px;
    background: var(--bg-2);
    color: var(--text-mute);
    border: 1px solid var(--border);
  }
  .slash-item--skill .slash-item-cmd { color: var(--accent); }
  /* Statusline strip — user's `statusline.json` script output. Lives
     directly below the composer pill. Monospace, single-color tone,
     hidden when no output. Vertical scroll if multi-line. */
  .cmp-statusline {
    margin-top: 8px;
    padding: 4px 12px;
    font: 10.5px / 1.5 'JetBrains Mono', ui-monospace, monospace;
    color: var(--text-mute);
    background: var(--bg-2);
    border: 1px solid var(--border);
    border-radius: 6px;
    white-space: pre-wrap;
    word-break: break-word;
    max-height: 72px;
    overflow-y: auto;
    flex-shrink: 0;
  }
  .cmp-statusline--err {
    color: #e0b16c;
    border-color: color-mix(in srgb, #e0b16c 35%, var(--border));
  }

  .cmp-shell {
    width: 100%;
    background: var(--bg-1);
    border: 1px solid var(--border-hi);
    border-radius: 12px;
    /* Symmetric horizontal padding so the @-icons on the left and
       the Send button on the right have the same breathing room. */
    padding: 8px 12px;
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
  /* Per-source tinted chip variants. Background + 1px border are the
     same width as the default, so swapping styles never reflows the
     attachments row. Icon picks up the same color via `currentColor`. */
  .cmp-attach-file--github {
    background: color-mix(in srgb, var(--src-github, #8b5cf6) 14%, var(--bg-3));
    border-color: color-mix(in srgb, var(--src-github, #8b5cf6) 35%, var(--border));
    color: color-mix(in srgb, var(--src-github, #8b5cf6) 80%, var(--text-1));
  }
  .cmp-attach-file--github svg { color: var(--src-github, #8b5cf6); }
  .cmp-attach-file--jira {
    background: color-mix(in srgb, var(--src-jira, #4f8eff) 14%, var(--bg-3));
    border-color: color-mix(in srgb, var(--src-jira, #4f8eff) 35%, var(--border));
    color: color-mix(in srgb, var(--src-jira, #4f8eff) 80%, var(--text-1));
  }
  .cmp-attach-file--jira svg { color: var(--src-jira, #4f8eff); }
  .cmp-attach-file--sentry {
    background: color-mix(in srgb, var(--src-sentry, #b56af0) 14%, var(--bg-3));
    border-color: color-mix(in srgb, var(--src-sentry, #b56af0) 35%, var(--border));
    color: color-mix(in srgb, var(--src-sentry, #b56af0) 80%, var(--text-1));
  }
  .cmp-attach-file--sentry svg { color: var(--src-sentry, #b56af0); }
  .cmp-attach-file--chat {
    background: color-mix(in srgb, var(--src-claude, #d97757) 14%, var(--bg-3));
    border-color: color-mix(in srgb, var(--src-claude, #d97757) 35%, var(--border));
    color: color-mix(in srgb, var(--src-claude, #d97757) 80%, var(--text-1));
  }
  .cmp-attach-file--chat svg { color: var(--src-claude, #d97757); }
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
    /* CRITICAL: textarea and backdrop must wrap IDENTICALLY.
       WebKit textarea doesn't honour `overflow-wrap: anywhere` —
       it falls back to word-boundary wrapping (`break-word`
       semantics). So both sides use the same mild combo: `pre-wrap`
       + `break-word` + default word-break. Long tokens break only
       when they must, the same way on both sides. */
    white-space: pre-wrap;
    word-break: normal;
    overflow-wrap: break-word;
    overflow: hidden;
    pointer-events: none;
    user-select: none;
    box-sizing: border-box;
    border: 0;
    /* Lock glyph geometry to match the textarea byte-for-byte —
       ligatures off, no kerning, fixed tab width. Any of these
       diverging caused the caret (rendered by the textarea) to
       drift away from the visible glyph (rendered by the backdrop)
       on tokens like `=>` / `->` / `==`. */
    font-variant-ligatures: none;
    font-feature-settings: "liga" 0, "clig" 0, "calt" 0;
    font-kerning: none;
    tab-size: 4;
    -moz-tab-size: 4;
  }
  /* Inline @-mention chip — soft accent tint. CRITICAL: padding and
     margin MUST be zero. The previous `padding: 0 2px; margin: 0 -1px`
     added +2px of horizontal width to every @-token. The WebKit
     textarea renders the same token WITHOUT that padding (it's just
     plain text in there), so the wrapping diverged: a long line with
     an @-token could break to the next row in the backdrop but stay
     on the same row in the textarea — and the caret (rendered by the
     textarea) ended up on one line while the visible glyph sat on
     another. That's exactly the "caret jumps far from where it
     should" symptom users reported. If a tighter chip look is
     desired, use `background` + `border-radius` only (no padding) so
     the fill lands exactly on the glyph box and layout shift stays
     at zero. */
  .cmp-area-backdrop :global(.cmp-area-mention) {
    background: color-mix(in srgb, var(--accent) 18%, transparent);
    color: var(--accent-bright);
    border-radius: 3px;
    padding: 0;
    margin: 0;
    /* CRITICAL: weight must MATCH the textarea so the glyph widths
       are identical. The textarea renders the @-token at the
       inherited weight (400). If the backdrop bumps the same
       glyphs to 500, every variable / hinted font widens them
       slightly — and that width delta accumulates on each chip,
       so by the end of the line the caret (rendered by the
       textarea at the narrow weight) ends up several pixels to
       the LEFT of where the bolder backdrop glyph appears.
       The user reads this as "caret is too far / dancing".
       Tinting the chip via background + color is enough to make
       it stand out — no weight change needed. */
    font-weight: inherit;
  }
  /* Per-source tinting overrides — same width-preserving rules
     (background + color only). Picks the canonical source accent
     from --src-* tokens so a @github mention reads purple, @jira
     reads blue, @sentry reads plum, @chat reads rust.

     Falls through to the default .cmp-area-mention style when the
     source isn't classified. */
  .cmp-area-backdrop :global(.cmp-area-mention--github) {
    background: color-mix(in srgb, var(--src-github, #8b5cf6) 22%, transparent);
    color: color-mix(in srgb, var(--src-github, #8b5cf6) 90%, white 10%);
  }
  .cmp-area-backdrop :global(.cmp-area-mention--jira) {
    background: color-mix(in srgb, var(--src-jira, #4f8eff) 20%, transparent);
    color: color-mix(in srgb, var(--src-jira, #4f8eff) 90%, white 10%);
  }
  .cmp-area-backdrop :global(.cmp-area-mention--sentry) {
    background: color-mix(in srgb, var(--src-sentry, #b56af0) 22%, transparent);
    color: color-mix(in srgb, var(--src-sentry, #b56af0) 90%, white 10%);
  }
  .cmp-area-backdrop :global(.cmp-area-mention--chat) {
    background: color-mix(in srgb, var(--src-claude, #d97757) 22%, transparent);
    color: color-mix(in srgb, var(--src-claude, #d97757) 90%, white 10%);
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
    /* Same wrapping rules as the backdrop — pre-wrap + break-word.
       WebKit textarea doesn't honour `overflow-wrap: anywhere` like
       a div does: inside a textarea it falls back to word-boundary
       wrap (effectively `break-word`). The backdrop used to be
       `anywhere` while the textarea was effectively `break-word`,
       which is exactly why caret vs glyph positions drifted apart
       on long unbreakable words. Both sides now spell out
       `break-word` so the wrap is identical. */
    white-space: pre-wrap;
    word-break: normal;
    overflow-wrap: break-word;
    box-sizing: border-box;
    /* Mirror backdrop glyph geometry — see backdrop CSS for rationale.
       Without these the caret drifts off ligated tokens. */
    font-variant-ligatures: none;
    font-feature-settings: "liga" 0, "clig" 0, "calt" 0;
    font-kerning: none;
    tab-size: 4;
    -moz-tab-size: 4;
  }
  .cmp-area::-webkit-scrollbar { display: none; }
  .cmp-area::placeholder {
    color: var(--text-mute);
    -webkit-text-fill-color: var(--text-mute);
  }
  /* Selection: highlight rectangle only. We MUST keep
   * `-webkit-text-fill-color` transparent here — flipping it back to
   * `var(--text-0)` paints the textarea's glyphs visible inside the
   * selection range while the backdrop ALSO paints those same
   * glyphs, producing the double-render the user sees as "selection
   * bleeds + cursor far from character". The native selection rect
   * still highlights the area; backdrop text remains visible
   * underneath; nothing double-paints. */
  .cmp-area::selection {
    background: var(--accent-soft);
    color: transparent;
    -webkit-text-fill-color: transparent;
  }
  .cmp-area::-moz-selection {
    background: var(--accent-soft);
    color: transparent;
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

  /* Permission-mode toggle — sits left of the model picker. Quiet
     dot at rest, amber "plan" pill when active. Designed to fade
     into the composer footer so the eye lands on the model/Send
     row first; the dot only commands attention when the session
     is actually in plan mode. */
  /* SDD button — same visual register as `.cmp-mode-dot`'s quiet
   *  rest state, but always shows the "SDD" glyph since it's a one-
   *  shot launcher (no toggle states). Hover lifts to the accent
   *  tint so users discover what it does without the button itself
   *  shouting. */
  .cmp-sdd-btn {
    display: inline-flex; align-items: center;
    padding: 2px 7px;
    border-radius: 5px;
    border: 1px solid transparent;
    background: transparent;
    color: var(--text-mute);
    cursor: pointer;
    flex-shrink: 0;
    transition: background 120ms, border-color 120ms, color 120ms;
  }
  .cmp-sdd-btn:hover {
    background: color-mix(in srgb, var(--accent) 14%, transparent);
    border-color: color-mix(in srgb, var(--accent) 35%, transparent);
    color: var(--accent-bright);
  }
  .cmp-sdd-btn--active {
    background: color-mix(in srgb, var(--accent) 22%, transparent);
    border-color: color-mix(in srgb, var(--accent) 50%, transparent);
    color: var(--accent-bright);
  }
  .cmp-sdd-glyph {
    font-family: 'JetBrains Mono', monospace;
    font-size: 9.5px;
    font-weight: 700;
    letter-spacing: 0.14em;
  }

  .cmp-mode-dot {
    display: inline-flex; align-items: center;
    gap: 5px;
    padding: 3px 5px;
    border-radius: 5px;
    border: 1px solid transparent;
    background: transparent;
    color: var(--text-mute);
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.03em;
    cursor: pointer;
    flex-shrink: 0;
    transition: background 120ms, border-color 120ms, color 120ms;
  }
  .cmp-mode-dot:hover {
    background: var(--bg-3);
    border-color: var(--border);
    color: var(--text-1);
  }
  .cmp-mode-dot-pip {
    /* Default state — small hollow ring. Reads as "off but available". */
    width: 8px; height: 8px;
    border-radius: 50%;
    border: 1.5px solid var(--text-mute);
    background: transparent;
    transition: border-color 150ms, background 150ms, box-shadow 150ms;
    flex-shrink: 0;
  }
  .cmp-mode-dot:hover .cmp-mode-dot-pip {
    border-color: var(--text-1);
  }
  /* Plan state — pill fills with amber tone + soft glow so the user
     can't miss they're in read-only mode. The dot becomes solid. */
  .cmp-mode-dot--plan {
    background: color-mix(in srgb, #e0b16c 14%, transparent);
    border-color: color-mix(in srgb, #e0b16c 40%, var(--border));
    color: #e0b16c;
    padding: 3px 8px 3px 6px;
  }
  .cmp-mode-dot--plan:hover {
    background: color-mix(in srgb, #e0b16c 22%, transparent);
    border-color: color-mix(in srgb, #e0b16c 55%, var(--border-hi));
    color: #f0c084;
  }
  .cmp-mode-dot--plan .cmp-mode-dot-pip {
    border-color: #e0b16c;
    background: #e0b16c;
    box-shadow: 0 0 0 3px color-mix(in srgb, #e0b16c 22%, transparent);
  }
  .cmp-mode-dot-label {
    font-size: 10px;
    font-weight: 700;
    letter-spacing: 0.06em;
    text-transform: lowercase;
  }
  @media (prefers-reduced-motion: reduce) {
    .cmp-mode-dot,
    .cmp-mode-dot-pip { transition: none; }
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
    background: rgba(14, 17, 18, 0.30);
    border: 1px solid rgba(14, 17, 18, 0.40);
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

  /* Queue indicator button + floating panel */
  .cmp-queue-wrap {
    position: relative;
  }
  .cmp-queue-indicator {
    display: inline-flex; align-items: center; gap: 4px;
    padding: 4px 8px;
    border-radius: 999px;
    background: color-mix(in srgb, var(--accent) 14%, transparent);
    border: 1px solid color-mix(in srgb, var(--accent) 32%, transparent);
    color: var(--accent-bright);
    font-size: 11px; font-weight: 600;
    cursor: pointer;
    transition: background 120ms, border-color 120ms;
    user-select: none;
  }
  .cmp-queue-indicator:hover,
  .cmp-queue-indicator--open {
    background: color-mix(in srgb, var(--accent) 22%, transparent);
    border-color: color-mix(in srgb, var(--accent) 50%, transparent);
  }
  .cmp-queue-indicator svg { width: 11px; height: 11px; }

  .cmp-queue-panel {
    position: absolute;
    bottom: calc(100% + 8px);
    right: 0;
    width: 320px;
    background: var(--bg-1);
    border: 1px solid var(--border-hi);
    border-radius: 10px;
    box-shadow: 0 8px 28px rgba(0, 0, 0, 0.36), 0 0 0 1px rgba(0,0,0,0.12);
    overflow: hidden;
    z-index: 200;
  }
  .cmp-queue-panel-head {
    display: flex; align-items: center; justify-content: space-between;
    padding: 9px 12px 7px;
    border-bottom: 1px solid var(--border);
    font-size: 11px; font-weight: 600; color: var(--text-2);
    text-transform: uppercase; letter-spacing: 0.05em;
  }
  .cmp-queue-clear {
    font-size: 11px; color: var(--text-2); font-weight: 500;
    padding: 2px 6px; border-radius: 4px;
  }
  .cmp-queue-clear:hover { background: var(--bg-3); color: var(--error); }

  .cmp-queue-item {
    display: flex; align-items: flex-start; gap: 8px;
    padding: 8px 10px 8px 12px;
    border-bottom: 1px solid var(--border);
  }
  .cmp-queue-item:last-child { border-bottom: none; }
  .cmp-queue-item:hover { background: var(--bg-2); }
  .cmp-queue-num {
    flex: 0 0 auto;
    width: 16px; height: 16px; margin-top: 1px;
    border-radius: 50%;
    background: color-mix(in srgb, var(--accent) 18%, transparent);
    border: 1px solid color-mix(in srgb, var(--accent) 35%, transparent);
    color: var(--accent-bright);
    font-size: 10px; font-weight: 700;
    display: flex; align-items: center; justify-content: center;
    line-height: 1;
  }
  .cmp-queue-text-wrap {
    flex: 1; min-width: 0;
    display: flex; flex-direction: column; gap: 4px;
  }
  .cmp-queue-attachments {
    display: flex; flex-wrap: wrap; gap: 4px;
  }
  .cmp-queue-att-chip {
    display: inline-flex; align-items: center; gap: 3px;
    padding: 2px 6px; border-radius: 4px;
    background: color-mix(in srgb, var(--accent) 12%, transparent);
    border: 1px solid color-mix(in srgb, var(--accent) 28%, transparent);
    color: var(--accent-bright);
    font-size: 10.5px; font-weight: 500;
    max-width: 160px;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .cmp-queue-att-chip svg { width: 10px; height: 10px; flex-shrink: 0; }
  .cmp-queue-text {
    font-size: 12.5px; color: var(--text-1); line-height: 1.45;
    white-space: pre-wrap; word-break: break-word;
    display: -webkit-box; -webkit-line-clamp: 3; -webkit-box-orient: vertical;
    overflow: hidden;
  }
  .cmp-queue-del {
    flex: 0 0 auto; margin-top: 1px;
    width: 18px; height: 18px; border-radius: 4px;
    display: flex; align-items: center; justify-content: center;
    color: var(--text-mute);
    transition: background 100ms, color 100ms;
  }
  .cmp-queue-del:hover { background: rgba(232, 130, 100, 0.14); color: var(--error); }
  .cmp-queue-del svg { width: 11px; height: 11px; }
</style>
