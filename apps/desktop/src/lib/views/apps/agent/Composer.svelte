<script lang="ts">
  /* Composer — bottom row: model picker + textarea + send.
     v8: model picker moved INTO the composer (was in ChatHeader),
     context-usage ring next to the token counter, Claude five-hour
     and weekly quota chips on the right edge for plan-aware users,
     no inner scroll on the textarea (auto-grows up to 70% of the
     viewport), inline @ autocomplete (sessions / Jira / GH / Sentry)
     anchored to the textarea, and OS / Editor drag drops accepted
     into the input as @-mentions. */
  import { sessionsState, setSessionInput, updateSession, attachPathsToSession } from '$lib/state/sessions.svelte';
  import { refreshPlanUsage } from '$lib/state/quota.svelte';
  import { isImagePath } from '$lib/format';
  import { convertFileSrc, invoke } from '@tauri-apps/api/core';
  import { open as openDialog } from '@tauri-apps/plugin-dialog';
  import { notify } from '$lib/state/toaster.svelte';
  import ModelEngine from './ModelEngine.svelte';
  import MentionPicker from './MentionPicker.svelte';
  import { onMount, untrack } from 'svelte';
  import type { Mention } from '$lib/types';
  import {
    KNOWN_SLASH_COMMANDS,
    SLASH_COMMAND_DESCRIPTIONS,
    type SlashCommand
  } from '$lib/services/slashCommands';
  import { skillsState, refreshSkills, type Skill } from '$lib/state/skills.svelte';
  import { statuslineState } from '$lib/state/statusline.svelte';
  import { layoutModeState } from '$lib/state/layoutMode.svelte';
  import { getRtkStatus, type RtkStatus } from '$lib/services/rtk';
  import {
    claudeEffort,
    claudeModels,
    detectTriggerPosition,
    modelContextLimit,
    spliceTriggerInsertion,
  } from './composerHelpers';

  type Kind = 'claude';

  interface Props {
    kind: Kind;
    /** Narrow-column layout (agent dock). Sheds the noisy suffix chips
     *  (quota pills + numeric context label) so the bottom row fits in
     *  ~340px; the context ring stays. Solo leaves it undefined. */
    compact?: boolean;
    onSend: () => void;
    onStop: () => void;
    onPasteImages: (
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
    matchBackdropWrap();
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

  /* Attach button → native file / folder picker. macOS' NSOpenPanel
     can't offer files AND directories in one panel, so the paperclip
     opens a tiny two-item menu. Picked paths route through the same
     `attachPathsToSession` pipeline as drag-drop (folders get
     `asDir=true` so they're @-mentioned with a trailing slash). */
  let attachMenu = $state(false);

  /* Quiet composer pill (§3.3): run controls (model / rtk / fast /
     launchers) collapse behind a ⋯ toggle so the input is a single
     line. Only in the Quiet direction on the full solo (not the
     compact AgentDock composer). */
  const quietPill = $derived(layoutModeState.mode === 'quiet' && !p.compact);
  let runOpen = $state(false);

  async function pickAttachments(asDir: boolean) {
    attachMenu = false;
    if (!sess) return;
    try {
      const picked = await openDialog({ multiple: true, directory: asDir });
      if (!picked) return;
      const paths = Array.isArray(picked) ? picked : [picked];
      const n = attachPathsToSession(sess.id, paths, asDir);
      if (n > 0) queueMicrotask(autoGrow);
    } catch (err) {
      notify({ kind: 'error', title: 'Could not attach', body: String(err) });
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
      await p.onPasteImages(blobs);
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

  /* Which VISUAL row the caret sits on. The old check counted `\n`
     only, so a soft-WRAPPED paragraph (no newline, several visual
     rows) read as a single first line — pressing ↑ from row 2 jumped
     into history instead of moving the caret up to row 1. We measure
     the caret's pixel row in a hidden mirror that copies the
     backdrop's exact box geometry (width + padding + font + wrap), so
     soft wraps count as real rows. */
  function caretVisualRow(): { onFirst: boolean; onLast: boolean } {
    if (!ta || !backdropEl) return { onFirst: true, onLast: true };
    const v = ta.value;
    const caret = ta.selectionStart ?? v.length;
    const cs = getComputedStyle(backdropEl);
    const mirror = document.createElement('div');
    const copy = [
      'fontFamily', 'fontSize', 'fontWeight', 'fontStyle', 'lineHeight',
      'letterSpacing', 'wordSpacing', 'whiteSpace', 'wordBreak',
      'overflowWrap', 'fontVariantLigatures', 'fontFeatureSettings',
      'fontKerning', 'textRendering', 'tabSize', 'boxSizing',
      'paddingTop', 'paddingRight', 'paddingBottom', 'paddingLeft',
    ] as const;
    for (const p of copy) mirror.style[p] = cs[p];
    mirror.style.position = 'absolute';
    mirror.style.visibility = 'hidden';
    mirror.style.top = '0';
    mirror.style.left = '-9999px';
    mirror.style.width = backdropEl.offsetWidth + 'px';
    const mk = document.createElement('span');
    mk.textContent = '​';
    mirror.append(
      document.createTextNode(v.slice(0, caret)),
      mk,
      document.createTextNode(v.slice(caret) || '​'),
    );
    document.body.appendChild(mirror);
    const padTop = parseFloat(cs.paddingTop) || 0;
    const padBottom = parseFloat(cs.paddingBottom) || 0;
    const lh = parseFloat(cs.lineHeight) || parseFloat(cs.fontSize) * 1.55;
    const caretTop = mk.offsetTop - padTop;
    const contentH = mirror.scrollHeight - padTop - padBottom;
    document.body.removeChild(mirror);
    return {
      onFirst: caretTop < lh * 0.5,
      onLast: caretTop > contentH - lh * 1.5,
    };
  }

  function shouldNavigateHistory(direction: 'ArrowUp' | 'ArrowDown'): boolean {
    if (userHistory.length === 0) return false;
    /* Already in history mode → always intercept. */
    if (historyPos >= 0) return true;
    /* Empty composer → both arrows are free for history. */
    const v = sess?.input ?? '';
    if (v.length === 0) return true;
    /* Otherwise hijack ↑ only on the first VISUAL row and ↓ only on
     * the last — so multi-row drafts (hard \n OR soft wrap) move the
     * caret within the text first, then reach history at the edges. */
    if (!ta) return false;
    const { onFirst, onLast } = caretVisualRow();
    return direction === 'ArrowUp' ? onFirst : onLast;
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
    /* untrack: refreshSkills sync-reads its own store (guard) before the
       first await — without untrack those reads register as deps of THIS
       effect and every store write re-fires it. */
    untrack(() => void refreshSkills(cwd));
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
  async function pickMention(s: { display: string; mention: Mention }) {
    if (!sess || !ta || mentionFrom < 0) return;
    const sessId = sess.id;
    const value = ta.value ?? '';
    const caret = ta.selectionStart ?? value.length;
    const before = value.slice(0, mentionFrom);
    const after = value.slice(caret);
    const next = before + s.display + after;
    setSessionInput(sess.id, next);
    /* `@git:*` mentions carry no body from the picker — fetch the git
       output now (read-only) so the literal diff/show rides into the
       prompt as the mention body (terminal-source render path). */
    let mention = s.mention;
    if (mention.source === 'terminal' && mention.externalId.startsWith('git:')) {
      const rest = mention.externalId.slice('git:'.length);
      const kind = rest === 'diff' ? 'diff' : rest === 'HEAD' ? 'head' : 'sha';
      const cwd = sess.worktreePath ?? sess.cwd ?? null;
      let body: string;
      if (!cwd) {
        body = '(git unavailable: session has no working directory)';
      } else {
        try {
          body = await invoke<string>('git_context', {
            cwd,
            kind,
            sha: kind === 'sha' ? rest : null
          });
        } catch (e) {
          body = `(git error: ${e})`;
        }
      }
      mention = { ...mention, body };
    } else if (mention.source === 'terminal' && mention.externalId.startsWith('web:')) {
      const url = mention.externalId.slice('web:'.length);
      if (!url) {
        // Hint row picked with no URL — nothing to fetch yet.
        mention = { ...mention, body: '(type a URL: @web:https://…)' };
      } else {
        let body: string;
        try {
          body = await invoke<string>('fetch_url_md', { url });
        } catch (e) {
          body = `(web error: ${e})`;
        }
        mention = { ...mention, body };
      }
    }
    /* De-dupe by externalId so picking the same mention twice doesn't
       double up the context payload. */
    const live = sessionsState.list.find((x) => x.id === sessId);
    const dedupedMentions = (live?.mentions ?? sess.mentions).filter(
      (m) => !(m.source === mention.source && m.externalId === mention.externalId)
    );
    updateSession(sessId, { mentions: [...dedupedMentions, mention] });
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
    // RTK pill state — fetch once per mount. Probe shells out for
    // `--version` + `hook claude --help` with a 2s deadline, so the
    // result is stable for the lifetime of the composer.
    if (p.kind === 'claude') {
      void getRtkStatus().then((s) => { rtkStatus = s; });
    }
    // Re-sync backdrop wrap when the input's available width changes
    // (suffix chips appear/disappear, window resize, pane resize).
    // The required pad is width-independent in theory but rounding at
    // a new width can flip a boundary word, so recompute on resize.
    let ro: ResizeObserver | null = null;
    if (ta && typeof ResizeObserver !== 'undefined') {
      // Width-only guard: autoGrow() mutates the textarea HEIGHT, which
      // would re-fire the observer and loop. Only react when the WIDTH
      // actually changed (the thing that flips wrap points).
      let lastW = ta.clientWidth;
      ro = new ResizeObserver((entries) => {
        const w = entries[0]?.contentRect.width ?? lastW;
        if (Math.abs(w - lastW) < 0.5) return;
        lastW = w;
        autoGrow();
      });
      ro.observe(ta);
    }
    // Initial match once layout settles (input may be non-empty after a
    // session switch / restored draft).
    requestAnimationFrame(() => autoGrow());
    return () => ro?.disconnect();
  });

  /** Snapshot of bundled / system RTK availability. Null until the
   *  initial `getRtkStatus()` resolves; the derive below treats null
   *  as "still probing" and renders the pill conservatively (on). */
  let rtkStatus = $state<RtkStatus | null>(null);
  /** `true` when the user hasn't explicitly disabled RTK for this
   *  session. Default-on lives here (rather than on the field) so a
   *  legacy persisted session without the field still reads as on. */
  const rtkEnabled = $derived(sess?.rtkEnabled !== false);
  /** Tri-state for the pill render path. */
  const rtkUiState = $derived.by<'on' | 'off' | 'unavailable' | 'error'>(() => {
    if (p.kind !== 'claude') return 'unavailable';
    if (!rtkStatus) {
      // Still probing — assume the bundled sidecar is fine so the
      // pill reflects the user's toggle. The probe rarely fails.
      return rtkEnabled ? 'on' : 'off';
    }
    if (!rtkStatus.platformSupported) return 'unavailable';
    if (!rtkStatus.bundledAvailable && !rtkStatus.systemVersion) return 'error';
    return rtkEnabled ? 'on' : 'off';
  });

  /* Per-model context window. Anthropic ships different ceilings per
     model — surfacing the wrong number means the ring shows 100% on
     models that actually have 5× the headroom. Numbers tracked
     against Anthropic's published limits as of late-2025; if a new
     model lands, fall through to the `200_000` Sonnet/Haiku default. */
  const tokenLimit = $derived(modelContextLimit(sess?.claudeModel ?? null));
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

  /* Model catalogues + claudeEffort moved to ./composerHelpers.ts
   * (wave-1 phase-6 split). Edit the lists there when adding new
   * SKUs or changing labels — Composer just renders them. */

  function setModel(v: string | null) {
    if (!sess) return;
    updateSession(sess.id, { claudeModel: v });
  }
  function setEffort(v: string | null) {
    if (!sess) return;
    // Persist effort on the session; threaded to MAX_THINKING_TOKENS
    // on the next spawn (see exec/claude.ts → claude_ask).
    updateSession(sess.id, {
      thinkingEffort: (v as 'auto' | 'low' | 'medium' | 'high' | 'max' | 'ultracode' | null) ?? null
    });
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

  /* Caret-drift killer. The textarea (owns the caret) and the backdrop
     div (owns the visible glyphs) share font/width/padding, yet a
     WKWebView <textarea> reserves end-of-line caret room a bare <div>
     doesn't — so the div fits one extra word per line and the caret
     lands on a different physical row than the glyph it sits next to.
     We grow the backdrop's right padding (a CSS var) until the two
     boxes report the SAME content height i.e. identical line count.
     The needed pad is just the reserve (≈ one space-width, constant
     per font/zoom), so we start from the last value and nudge ±1 —
     steady state is 2 scrollHeight reads, no per-keystroke thrash. */
  let backdropPadR = 0;
  function matchBackdropWrap() {
    if (!ta || !backdropEl) return;
    const setPad = (px: number) => {
      backdropPadR = px;
      backdropEl!.style.setProperty('--backdrop-pad-r', px + 'px');
      // read forces synchronous layout with the new pad applied
      return backdropEl!.scrollHeight;
    };
    let back = setPad(backdropPadR);
    let area = ta.scrollHeight;
    let guard = 0;
    // backdrop shorter than textarea => it fit more text => widen pad
    while (back < area && backdropPadR < 24 && guard++ < 32) {
      back = setPad(backdropPadR + 1);
    }
    // backdrop taller => it wrapped too early => shrink pad to the
    // smallest value that still matches (avoids over-padding drift the
    // other way, where a short line's caret sits past the last glyph)
    while (back > area && backdropPadR > 0 && guard++ < 64) {
      back = setPad(backdropPadR - 1);
      if (back < area) { setPad(backdropPadR + 1); break; }
    }
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
  <!-- Shared textarea + backdrop. Single-sourced so the Cabin footer
       and the Quiet single-line pill (§3.3) reuse the exact same input
       wiring — caret/glyph sync, mention backdrop, all handlers. -->
  {#snippet composerArea()}
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
          ? 'Type to queue — fires after the current turn finishes.'
          : 'Reply…  ( / commands · @ mention · drag items here )'}
        rows="1"
        spellcheck="false"
        autocomplete="off"
        {...{ autocorrect: 'off', autocapitalize: 'off' }}
      ></textarea>
    </div>
  {/snippet}

  <!-- Queued-messages popover — shared by the Cabin queue chip and the
       Quiet meta-row queue readout. -->
  {#snippet queuePanel()}
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
  {/snippet}

  <div
    class="cmp"
    class:cmp--compact={p.compact}
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
      class:cmp-shell--quiet={quietPill}
      class:cmp-shell--run-open={runOpen}
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

        {#if quietPill}
          <!-- §3.3 Quiet composer pill — single line: [text][⋯][send-circle].
               Attach/@ hidden (drag-drop + typed @// only); model/rtk/fast/
               launchers hide behind the ⋯ run toggle. -->
          <div class="qpill-row">
            {@render composerArea()}
            <button
              class="qpill-run"
              class:qpill-run--on={runOpen}
              onclick={() => (runOpen = !runOpen)}
              title="Run settings — model · rtk · fast · launchers"
              aria-label="Run settings"
              aria-expanded={runOpen}
            >
              <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><circle cx="5" cy="12" r="1.7"/><circle cx="12" cy="12" r="1.7"/><circle cx="19" cy="12" r="1.7"/></svg>
            </button>
            {#if sess.sending}
              <button class="qpill-circle qpill-circle--stop" onclick={p.onStop} title="Stop the running turn" aria-label="Stop the running turn">
                <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><rect x="6" y="6" width="12" height="12" rx="2"/></svg>
              </button>
              <button class="qpill-circle" onclick={doSend} disabled={!sess.input?.trim()} title="Queue this message — fires automatically when the current turn finishes" aria-label="Queue message">
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><line x1="12" y1="19" x2="12" y2="5"/><polyline points="6 11 12 5 18 11"/></svg>
              </button>
            {:else}
              <button class="qpill-circle" onclick={doSend} disabled={!sess.input?.trim()} title="Send" aria-label="Send message">
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><line x1="12" y1="19" x2="12" y2="5"/><polyline points="6 11 12 5 18 11"/></svg>
              </button>
            {/if}
          </div>

          {#if runOpen}
            <!-- Run controls revealed by ⋯ (§3.3) — model chip, /dw /ledger
                 launchers, RTK/FAST toggles. Fresh qpill- markup; same wiring
                 as the Cabin footer. -->
            <div class="qpill-run-panel">
              <ModelEngine
                model={sess.claudeModel ?? 'claude-sonnet-4-6'}
                modelOptions={claudeModels}
                effort={sess.thinkingEffort ?? 'auto'}
                effortOptions={claudeEffort}
                onModelChange={setModel}
                onEffortChange={setEffort}
              />
              <span class="qpill-run-launchers">
                <button
                  class="qpill-launch"
                  onclick={() => {
                    if (!sess) return;
                    updateSession(sess.id, { input: '/dw ' });
                    queueMicrotask(() => { if (ta) { ta.selectionStart = ta.value.length; ta.selectionEnd = ta.value.length; ta.focus(); } });
                  }}
                  aria-label="Start a Dynamic Workflow"
                  title="DW — planner fans out parallel subagents, then a verifier synthesises one answer."
                >
                  <span class="qpill-launch-glyph">/dw</span>
                </button>
                <button
                  class="qpill-launch"
                  onclick={() => {
                    if (!sess) return;
                    updateSession(sess.id, { input: '/ledger ' });
                    queueMicrotask(() => { if (ta) { ta.selectionStart = ta.value.length; ta.selectionEnd = ta.value.length; ta.focus(); } });
                  }}
                  aria-label="Start a Ledger workflow"
                  title="Ledger — decomposes the task into a machine-checked checklist; fresh-context workers land items one by one."
                >
                  <span class="qpill-launch-glyph">/ledger</span>
                </button>
              </span>
              <span class="qpill-run-toggles">
                {#if rtkUiState !== 'unavailable'}
                  <button
                    class="qpill-tog"
                    class:qpill-tog--on={rtkUiState === 'on'}
                    class:qpill-tog--error={rtkUiState === 'error'}
                    disabled={rtkUiState === 'error'}
                    onclick={() => updateSession(sess.id, { rtkEnabled: !(rtkEnabled) })}
                    aria-pressed={rtkUiState === 'on'}
                    aria-label={rtkUiState === 'on' ? 'RTK output compression active — click to disable for this session' : rtkUiState === 'off' ? 'RTK output compression disabled — click to re-enable for this session' : 'RTK binary missing — reinstall Woom'}
                    title={rtkUiState === 'error' ? 'RTK binary missing — reinstall Woom' : rtkUiState === 'on' ? 'RTK rewrites bash commands to compact output. Click to disable for this session.' : 'RTK disabled for this session. Click to re-enable. (Applies to next spawn.)'}
                  >
                    <span class="qpill-tog-dot" aria-hidden="true"></span>
                    <span class="qpill-tog-glyph" aria-hidden="true">RTK</span>
                  </button>
                {/if}
                {#if (sess.claudeModel ?? '').startsWith('claude-opus-4-8')}
                  <button
                    class="qpill-tog"
                    class:qpill-tog--on={sess.fastMode === true}
                    onclick={() => updateSession(sess.id, { fastMode: !(sess.fastMode === true) })}
                    aria-pressed={sess.fastMode === true}
                    aria-label={sess.fastMode === true ? 'Fast mode active — click to disable for this session' : 'Fast mode disabled — click to enable for this session'}
                    title={sess.fastMode === true ? 'Fast mode ON — Opus 4.8 streams 2.5× faster at 2× cost. Click to disable.' : 'Fast mode OFF — click to enable. Opus 4.8 streams 2.5× faster at 2× cost.'}
                  >
                    <span class="qpill-tog-dot" aria-hidden="true"></span>
                    <span class="qpill-tog-glyph" aria-hidden="true">FAST</span>
                  </button>
                {/if}
              </span>
            </div>
          {/if}
        {:else}
        {@render composerArea()}

        <div class="cbar">
          <div class="cbar-lead">
            <div class="cmp-attach-wrap">
              <button
                class="cbar-ghost"
                class:active={attachMenu}
                title="Attach files or folders"
                aria-haspopup="menu"
                aria-expanded={attachMenu}
                onclick={() => (attachMenu = !attachMenu)}
              >
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7"><path d="M21.44 11.05 12.25 20.24a6 6 0 1 1-8.49-8.49l9.19-9.19a4 4 0 1 1 5.66 5.66l-9.2 9.19a2 2 0 0 1-2.83-2.83l8.49-8.49"/></svg>
              </button>
              {#if attachMenu}
                <button
                  class="cmp-attach-backdrop"
                  aria-label="Close attach menu"
                  onclick={() => (attachMenu = false)}
                ></button>
                <div class="cmp-attach-menu" role="menu">
                  <button class="cmp-attach-item" role="menuitem" onclick={() => void pickAttachments(false)}>
                    <svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><path d="M14 2v6h6"/></svg>
                    Files…
                  </button>
                  <button class="cmp-attach-item" role="menuitem" onclick={() => void pickAttachments(true)}>
                    <svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M3 7a2 2 0 0 1 2-2h4l2 3h8a2 2 0 0 1 2 2v7a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/></svg>
                    Folder…
                  </button>
                </div>
              {/if}
            </div>
            <button class="cbar-ghost" title="@ mention" onclick={clickMention}>
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7"><circle cx="12" cy="12" r="4"/><path d="M16 8v5a3 3 0 0 0 6 0v-1a10 10 0 1 0-3.92 7.94"/></svg>
            </button>
          </div>

          <!-- 1×16 hairline separating the prefix icons from the model chip. -->
          <span class="cbar-divider" aria-hidden="true"></span>

          <!-- model config — model picker + effort slider (opens ModelEngine panel). -->
          <span class="cbar-model">
            <ModelEngine
              model={sess.claudeModel ?? 'claude-sonnet-4-6'}
              modelOptions={claudeModels}
              effort={sess.thinkingEffort ?? 'auto'}
              effortOptions={claudeEffort}
              onModelChange={setModel}
              onEffortChange={setEffort}
            />
          </span>

          <!-- launchers + toggles. Two semantic sub-clusters: launchers
               (/dw /ledger) are one-shot slash-command shortcuts, styled
               as mono ghost chips with a leading slash. Toggles (RTK/FAST)
               are persistent session modes, styled as on/off pills with a
               5px status dot (filled = on, hollow = off). -->
          <span class="cbar-run">
          <span class="cbar-launchers">
          <!-- DW button — prefills `/dw ` into the composer. Same code
               path as typing the slash command (`runDwFromSlash` in
               routes/handleSlashCommand.ts). -->
          <button
            class="cbar-launch"
            onclick={() => {
              if (!sess) return;
              updateSession(sess.id, { input: '/dw ' });
              queueMicrotask(() => {
                if (ta) {
                  ta.selectionStart = ta.value.length;
                  ta.selectionEnd = ta.value.length;
                  ta.focus();
                }
              });
            }}
            aria-label="Start a Dynamic Workflow"
            title="DW — planner fans out parallel subagents (each in its own git worktree), then a verifier synthesises one answer. Shows a cost estimate before it runs."
          >
            <span class="cbar-launch-glyph">/dw</span>
          </button>

          <!-- Ledger button — prefills `/ledger `. Same code path as the
               slash command (`runLedgerFromSlash`). -->
          <button
            class="cbar-launch"
            onclick={() => {
              if (!sess) return;
              updateSession(sess.id, { input: '/ledger ' });
              queueMicrotask(() => {
                if (ta) {
                  ta.selectionStart = ta.value.length;
                  ta.selectionEnd = ta.value.length;
                  ta.focus();
                }
              });
            }}
            aria-label="Start a Ledger workflow"
            title="Ledger — the agent decomposes the task into a machine-checked checklist you can edit, then fresh-context workers land items one by one (waves run in parallel), each verified by its check command with auto-retry. Review the diff, apply."
          >
            <span class="cbar-launch-glyph">/ledger</span>
          </button>

          </span><!-- /cbar-launchers -->
          <span class="cbar-toggles">

          <!-- RTK output-compression pill. On by default for every new
               Claude session (`newClaudeSession` sets `rtkEnabled: true`).
               One click toggles; the change applies to the NEXT spawn
               of `claude` (mid-stream turn isn't affected). Hidden when
               the bundled rtk binary isn't usable on this platform
               (native Windows — see Phase 1's `platformSupported`). -->
          {#if rtkUiState !== 'unavailable'}
            <button
              class="cbar-pill"
              class:cbar-pill--on={rtkUiState === 'on'}
              class:cbar-pill--error={rtkUiState === 'error'}
              disabled={rtkUiState === 'error'}
              onclick={() => updateSession(sess.id, { rtkEnabled: !(rtkEnabled) })}
              aria-pressed={rtkUiState === 'on'}
              aria-label={rtkUiState === 'on'
                ? 'RTK output compression active — click to disable for this session'
                : rtkUiState === 'off'
                  ? 'RTK output compression disabled — click to re-enable for this session'
                  : 'RTK binary missing — reinstall Woom'}
              title={rtkUiState === 'error'
                ? 'RTK binary missing — reinstall Woom'
                : rtkUiState === 'on'
                  ? 'RTK rewrites bash commands to compact output. ~80% token savings on git/test/ls. Click to disable for this session.'
                  : 'RTK disabled for this session — bash output passes through raw. Click to re-enable. (Applies to next spawn.)'}
            >
              <span class="cbar-dot" aria-hidden="true"></span>
              <span class="cbar-pill-glyph" aria-hidden="true">RTK</span>
            </button>
          {/if}

          <!-- Fast-mode toggle. Visible only when the active Claude
               model belongs to the Opus 4.8 family — Anthropic's only
               Fast-capable SKU at launch. Default off. Toggling on
               costs 2× per token but the endpoint streams 2.5× faster
               (same model, dedicated infra). Effect applies to the
               NEXT spawn — current turn stays on whatever endpoint it
               started on. -->
          {#if (sess.claudeModel ?? '').startsWith('claude-opus-4-8')}
            <button
              class="cbar-pill"
              class:cbar-pill--on={sess.fastMode === true}
              onclick={() => updateSession(sess.id, { fastMode: !(sess.fastMode === true) })}
              aria-pressed={sess.fastMode === true}
              aria-label={sess.fastMode === true
                ? 'Fast mode active — click to disable for this session'
                : 'Fast mode disabled — click to enable for this session'}
              title={sess.fastMode === true
                ? 'Fast mode ON — Opus 4.8 streams 2.5× faster at 2× cost. Click to disable. (Applies to next spawn.)'
                : 'Fast mode OFF — click to enable. Opus 4.8 will stream 2.5× faster at 2× cost. Same model, dedicated endpoint.'}
            >
              <span class="cbar-dot" aria-hidden="true"></span>
              <span class="cbar-pill-glyph" aria-hidden="true">FAST</span>
            </button>
          {/if}
          </span><!-- /cbar-toggles -->
          </span>


          <span class="cbar-spring" aria-hidden="true"></span>

          {#if (sess.pendingQueue?.length ?? 0) > 0}
            <div class="cbar-queue-wrap" bind:this={queueWrapEl}>
              <button
                class="cbar-queue"
                class:cbar-queue--open={queueOpen}
                onclick={toggleQueue}
                title="Show queued messages"
              >
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"><path d="M3 6h18M3 12h18M3 18h12"/></svg>
                {sess.pendingQueue?.length}
              </button>
              {#if queueOpen}
                {@render queuePanel()}
              {/if}
            </div>
          {/if}

          <span
            class="cbar-ctx"
            class:cbar-ctx--warn={ctxPct >= 70 && ctxPct < 90}
            class:cbar-ctx--err={ctxPct >= 90}
            title="Context window: {contextTokens.toLocaleString()} / {tokenLimit.toLocaleString()} tokens ({ctxLabel})"
          >ctx {ctxPct}%</span>

          {#if sess.sending}
            <button class="cbar-stop" onclick={p.onStop} title="Stop the running turn">
              <svg viewBox="0 0 24 24" fill="currentColor"><rect x="6" y="6" width="12" height="12" rx="2"/></svg>
              <span class="cbar-send-label">Stop</span>
            </button>
            <button
              class="cbar-send cbar-send--queue"
              onclick={doSend}
              disabled={!sess.input?.trim()}
              title="Queue this message — fires automatically when the current turn finishes"
            >
              <span class="cbar-send-label">Queue</span>
              <span class="cbar-send-kbd">⏎</span>
            </button>
          {:else}
            <button class="cbar-send" onclick={doSend} disabled={!sess.input?.trim()}>
              <span class="cbar-send-label">Send</span>
              <span class="cbar-send-kbd">⏎</span>
            </button>
          {/if}
      </div>
        {/if}
    </div>

    {#if quietPill}
      <!-- §3.3 — one ghost mono row under the pill: queued · ctx · statusline. -->
      <div class="qpill-meta">
        {#if (sess.pendingQueue?.length ?? 0) > 0}
          <div class="qpill-meta-queue" bind:this={queueWrapEl}>
            <button
              class="qpill-meta-btn"
              class:qpill-meta-btn--open={queueOpen}
              onclick={toggleQueue}
              title="Show queued messages"
            >⧗ {sess.pendingQueue?.length} queued</button>
            {#if queueOpen}
              {@render queuePanel()}
            {/if}
          </div>
          <span class="qpill-meta-sep" aria-hidden="true">·</span>
        {/if}
        <span
          class="qpill-meta-ctx"
          class:qpill-meta-ctx--warn={ctxPct >= 70 && ctxPct < 90}
          class:qpill-meta-ctx--err={ctxPct >= 90}
          title="Context window: {contextTokens.toLocaleString()} / {tokenLimit.toLocaleString()} tokens ({ctxLabel})"
        >ctx {ctxPct}%</span>
        {#if statuslineState.lastResult && statuslineState.lastResult.stdout.trim().length > 0}
          <span class="qpill-meta-sep" aria-hidden="true">·</span>
          <span
            class="qpill-meta-status"
            class:qpill-meta-status--err={!statuslineState.lastResult.ok}
            title={statuslineState.lastResult.ok ? `last ran ${Math.round((Date.now() - statuslineState.lastRanAt) / 1000)}s ago` : (statuslineState.lastResult.stderr || 'statusline error')}
          >▸ {statuslineState.lastResult.stdout.trim()}</span>
        {/if}
      </div>
    {/if}

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
  {#if !quietPill && statuslineState.lastResult && statuslineState.lastResult.stdout.trim().length > 0}
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
    padding: 12px 22px 16px;
    background: transparent;
    border-top: 0;
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
  /* Redesign v2 §2.5 — boxless statusline under the pill: no bg/border,
     mono 10.5 linenum tone, `▸` prefix. */
  .cmp-statusline {
    margin-top: 6px;
    padding: 0 2px;
    font: 10.5px / 1.5 var(--font-mono), ui-monospace, monospace;
    color: var(--text-linenum, var(--text-mute));
    background: transparent;
    border: 0;
    border-radius: 0;
    white-space: pre-wrap;
    word-break: break-word;
    max-height: 72px;
    overflow-y: auto;
    flex-shrink: 0;
  }
  .cmp-statusline::before { content: '▸ '; opacity: 0.7; }
  .cmp-statusline--err { color: #e0b16c; }

  /* Pill matches the thread column width (kept in sync with ChatThread
     `.ct > *`): bg-1 (light bg-2), hairline border, r12, shadow-1. */
  .cmp-shell {
    width: 100%;
    max-width: min(1280px, 94%);
    margin: 0 auto;
    background: var(--bg-1);
    border: 1px solid var(--border);
    border-radius: 12px;
    padding: 12px 14px;
    box-shadow: var(--shadow-1);
    transition: border-color 200ms, box-shadow 200ms;
  }
  :global(:root[data-theme='light']) .cmp-shell { background: var(--bg-2); }
  .cmp-shell:focus-within {
    border-color: var(--border-hi2);
    box-shadow: var(--shadow-1);
  }
  /* Drop target hint — terracotta dashed outline + soft glow while
     the user is dragging a file / ticket / PR / error onto us. */
  .cmp-shell--drop {
    border-color: var(--accent-bright);
    border-style: dashed;
    box-shadow: 0 0 0 4px var(--accent-soft), var(--shadow-3);
  }

  /* ── Quiet composer pill (§3.3) — single-line pill [text][⋯][send-circle]
     with a ghost mono meta row beneath. Fresh qpill- markup; base pill bg /
     border / shadow come from .cmp-shell above. ── */
  .cmp-shell--quiet {
    border-radius: 14px;
    padding: 12px 12px 12px 16px;
  }
  /* One line: textarea flexes, ⋯ + send-circle pin to the end, all centered. */
  .qpill-row {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .qpill-row .cmp-area-wrap { flex: 1; min-width: 0; }
  /* Spec text metrics 14.5 / 1.6. Backdrop MUST track the textarea byte-for-byte
     or the @-mention caret drifts, so both sides get identical metrics. */
  .cmp-shell--quiet .cmp-area,
  .cmp-shell--quiet .cmp-area-backdrop {
    font-size: 14.5px;
    line-height: 1.6;
  }
  /* ⋯ run toggle — 28×28 ghost-border square. */
  .qpill-run {
    flex: none;
    width: 28px; height: 28px;
    display: grid; place-items: center;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: transparent;
    color: var(--text-2);
    cursor: pointer;
    transition: color 120ms, border-color 120ms, background 120ms;
  }
  .qpill-run:hover { color: var(--text-0); border-color: var(--border-hi); }
  .qpill-run--on { color: var(--text-0); background: var(--bg-3); border-color: var(--border-hi); }
  .qpill-run svg { width: 15px; height: 15px; }
  /* Send-circle — 32px inverse disc, shadow-pill. Stop reuses the shape but
     tinted with the error token. */
  .qpill-circle {
    flex: none;
    width: 32px; height: 32px;
    display: grid; place-items: center;
    border: none; border-radius: 50%;
    background: var(--accent);
    color: var(--accent-fg);
    cursor: pointer;
    box-shadow: var(--shadow-pill);
    transition: transform 120ms, box-shadow 200ms;
  }
  .qpill-circle:hover:not(:disabled) {
    transform: translate(-1px, -1px);
    box-shadow: var(--shadow-2);
  }
  .qpill-circle:disabled { opacity: 0.45; cursor: not-allowed; box-shadow: none; }
  .qpill-circle svg { width: 16px; height: 16px; }
  .qpill-circle--stop {
    background: color-mix(in srgb, var(--error) 10%, transparent);
    border: 1px solid color-mix(in srgb, var(--error) 30%, transparent);
    color: var(--error);
    box-shadow: none;
  }
  .qpill-circle--stop:hover:not(:disabled) {
    background: color-mix(in srgb, var(--error) 18%, transparent);
    transform: none;
    box-shadow: none;
  }
  .qpill-circle--stop svg { width: 12px; height: 12px; }
  @media (prefers-reduced-motion: reduce) {
    .qpill-run, .qpill-circle { transition: none; }
    .qpill-circle:hover:not(:disabled) { transform: none; }
  }

  /* Run controls revealed by ⋯ — model chip + launchers + toggles below the
     input line, separated by a hairline. */
  .qpill-run-panel {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    margin-top: 10px;
    padding-top: 10px;
    border-top: 1px solid var(--border-lo);
  }
  .qpill-run-launchers,
  .qpill-run-toggles {
    display: inline-flex;
    align-items: center;
    gap: 4px;
  }
  .qpill-run-toggles:empty { display: none; }
  /* /dw /ledger launchers — mono ghost chips. */
  .qpill-launch {
    display: inline-flex; align-items: center;
    padding: 2px 7px;
    border-radius: 5px;
    border: 1px solid transparent;
    background: transparent;
    color: var(--text-mute);
    cursor: pointer;
    transition: background 120ms, color 120ms;
  }
  .qpill-launch:hover { background: var(--bg-3); color: var(--text-1); }
  .qpill-launch-glyph {
    font-family: var(--font-mono);
    font-size: 11px; font-weight: 600;
  }
  /* RTK / FAST toggles — on/off pills with a 5px status dot. */
  .qpill-tog {
    display: inline-flex; align-items: center; gap: 5px;
    padding: 3px 8px;
    border-radius: 6px;
    border: 1px solid var(--border);
    background: transparent;
    color: var(--text-mute);
    cursor: pointer;
    transition: background 120ms, border-color 120ms, color 120ms;
  }
  .qpill-tog:hover:not(:disabled) { border-color: var(--border-hi); color: var(--text-1); }
  .qpill-tog--on {
    background: var(--bg-3);
    border-color: var(--border-hi);
    color: var(--text-0);
  }
  .qpill-tog--error { opacity: 0.5; cursor: not-allowed; }
  .qpill-tog-glyph {
    font-family: var(--font-mono);
    font-size: 10px; font-weight: 700;
    letter-spacing: 0.04em;
  }
  .qpill-tog-dot {
    width: 5px; height: 5px; border-radius: 50%;
    border: 1px solid currentColor;
    background: transparent;
  }
  .qpill-tog--on .qpill-tog-dot { background: currentColor; }
  @media (prefers-reduced-motion: reduce) {
    .qpill-launch, .qpill-tog { transition: none; }
  }

  /* ── Ghost mono meta row under the pill (§3.3): ⧗ N queued · ctx X% ·
     ▸ statusline. One line, mono 10.5, linenum tone. ── */
  .qpill-meta {
    display: flex;
    align-items: baseline;
    gap: 6px;
    width: 100%;
    max-width: min(1280px, 94%);
    margin: 6px auto 0;
    padding: 0 2px;
    font: 10.5px / 1.5 var(--font-mono), ui-monospace, monospace;
    color: var(--text-linenum, var(--text-mute));
    font-variant-numeric: tabular-nums;
  }
  .qpill-meta-sep { opacity: 0.55; flex: none; }
  .qpill-meta-queue { position: relative; flex: none; }
  .qpill-meta-btn {
    font: inherit;
    color: inherit;
    background: transparent;
    border: 0;
    padding: 0;
    cursor: pointer;
    transition: color 120ms;
  }
  .qpill-meta-btn:hover,
  .qpill-meta-btn--open { color: var(--text-1); }
  .qpill-meta-ctx { flex: none; }
  .qpill-meta-ctx--warn { color: var(--warn); }
  .qpill-meta-ctx--err { color: var(--err); }
  .qpill-meta-status {
    flex: 1; min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .qpill-meta-status--err { color: var(--warn); }

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

  /* ── Footer control bar (§2.5, mockup 3a). Single centered row:
     [+] [@] · divider · model chip · /dw /ledger · RTK/FAST · spring ·
     queue · ctx · Send. `align-items: center` keeps every control on
     the textarea's baseline; when the draft grows multi-line the row
     stays vertically centered against the taller textarea above it. ── */
  .cbar {
    display: flex; align-items: center; gap: 8px;
    border-top: 1px solid var(--border-lo);
    padding-top: 8px;
    margin-top: 8px;
    min-height: 28px;
  }
  .cbar-lead {
    display: flex; align-items: center; gap: 2px;
  }
  /* [+] Files/Folder + [@] mention — 26×26 ghost squares, faint until
     hovered. */
  .cbar-ghost {
    width: 26px; height: 26px;
    display: grid; place-items: center;
    border-radius: 7px;
    color: var(--text-mute);
    background: transparent;
    border: 0;
    cursor: pointer;
    transition: background 120ms, color 120ms;
  }
  .cbar-ghost:hover { background: var(--bg-3); color: var(--text-1); }
  .cbar-ghost.active { background: var(--bg-3); color: var(--text-0); }
  .cbar-ghost svg { width: 15px; height: 15px; }
  /* 1×16 hairline between prefix icons and the model chip. */
  .cbar-divider {
    width: 1px; height: 16px;
    background: var(--border);
    flex-shrink: 0;
  }

  /* Attach menu — small popover anchored above the paperclip. */
  .cmp-attach-wrap { position: relative; display: inline-flex; }
  .cmp-attach-backdrop {
    position: fixed; inset: 0; z-index: 40;
    background: transparent; border: 0; padding: 0; cursor: default;
  }
  .cmp-attach-menu {
    position: absolute; bottom: calc(100% + 6px); left: 0; z-index: 41;
    min-width: 150px;
    display: flex; flex-direction: column;
    padding: 4px;
    background: var(--bg-1);
    border: 1px solid var(--border);
    border-radius: 8px;
    box-shadow: var(--shadow-3);
  }
  .cmp-attach-item {
    display: flex; align-items: center; gap: 8px;
    padding: 7px 9px;
    border: 0; border-radius: 6px;
    background: transparent;
    color: var(--text-1);
    font-size: 12.5px;
    text-align: left;
    cursor: pointer;
    transition: background 120ms, color 120ms;
  }
  .cmp-attach-item:hover { background: var(--bg-2); color: var(--text-0); }
  .cmp-attach-item svg { color: var(--text-mute); flex-shrink: 0; }

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
    /* right padding is driven at runtime via --backdrop-pad-r so the
       backdrop wraps at the SAME column as the textarea — WKWebView's
       <textarea> reserves ~one space-width of end-of-line caret room,
       so a bare div (no reserve) fits one extra word per line and the
       caret drifts onto the wrong physical row. matchBackdropWrap()
       grows this pad until both boxes report the same line count. */
    padding: 5px var(--backdrop-pad-r, 0px) 5px 0;
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
    /* WKWebView ignores font-feature-settings/font-kerning on a bare
       <textarea>, so it keeps kerning ON while this div had it off —
       per-glyph advances diverged and the caret (textarea) drifted a
       few px to the RIGHT of the visible glyph (backdrop). optimizeSpeed
       forces no-kerning/no-ligatures at the rasteriser level on BOTH,
       which the textarea DOES honour, so advances match byte-for-byte. */
    text-rendering: optimizeSpeed;
    letter-spacing: 0;
    word-spacing: 0;
    font-weight: 400;
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
    /* flex child of .cmp-area-wrap — without min-width:0 a long
       unbreakable token blows the intrinsic min-content width past
       the 1fr track, making the textarea wider than the backdrop and
       wrapping the two differently. */
    min-width: 0;
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
    /* See backdrop: optimizeSpeed is the one WKWebView honours on a
       <textarea>, so caret advance matches the backdrop glyph. */
    text-rendering: optimizeSpeed;
    letter-spacing: 0;
    word-spacing: 0;
    font-weight: 400;
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

  /* Compact (agent dock, ~340px): the single row can't hold textarea +
     full control run. Go two-tier — textarea gets its own full-width
     line, controls drop to a second line below. Shed the group divider.
     Keep /dw + RTK/FAST + model + send — the second tier has room for
     them. Solo never gets `.cmp--compact`. */
  .cmp--compact .cbar-divider { display: none; }
  .cmp--compact .cbar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px 6px;
  }
  .cmp--compact .cmp-area-wrap {
    order: 1;
    flex: 1 1 100%;   /* tier 1 — textarea full width */
  }
  .cmp--compact .cbar-lead { order: 2; }
  /* Send pinned to the right edge of tier 2; everything else flows left. */
  .cmp--compact .cbar-send,
  .cmp--compact .cbar-stop { margin-left: auto; }
  .cmp--compact .cbar-send ~ .cbar-send,
  .cmp--compact .cbar-stop ~ .cbar-send { margin-left: 0; }
  /* launchers + toggles cluster (one semantic group between the model
     chip and the spring). Tight intra-group gap. */
  .cbar-run {
    display: inline-flex; align-items: center; gap: 6px;
    min-width: 0;
  }
  /* Launchers (/dw /ledger) and toggles (RTK/FAST) sit a hair apart so
     the "command shortcut" cluster reads distinctly from the "session
     mode" pair without a hard divider between them. */
  .cbar-launchers, .cbar-toggles {
    display: inline-flex; align-items: center; gap: 4px;
  }
  .cbar-toggles:empty { display: none; }

  /* Model-chip wrapper — hosts ModelEngine's `Opus 4.8 · 1M · ultracode ▾`
     chip. */
  .cbar-model {
    flex-shrink: 0;
    display: inline-flex; align-items: center; gap: 4px;
  }

  /* Spring pushes the queue / ctx / Send cluster to the right edge. */
  .cbar-spring { flex: 1; }

  /* Context readout — mono, quiet; colour shifts as the window fills
     (≥70% warn, ≥90% err). */
  .cbar-ctx {
    font-family: var(--font-mono);
    font-size: 11px;
    color: var(--text-mute);
    flex-shrink: 0;
    white-space: nowrap;
    font-variant-numeric: tabular-nums;
  }
  .cbar-ctx--warn { color: var(--warn); }
  .cbar-ctx--err  { color: var(--err); }

  /* Launchers (/dw /ledger) — mono ghost chips; click prefills the
     slash command into the input. */
  .cbar-launch {
    display: inline-flex; align-items: center;
    padding: 2px 7px;
    border-radius: 5px;
    border: 1px solid transparent;
    background: transparent;
    color: var(--text-mute);
    cursor: pointer;
    flex-shrink: 0;
    transition: background 120ms, color 120ms;
  }
  .cbar-launch:hover { background: var(--bg-3); color: var(--text-1); }
  .cbar-launch-glyph {
    font-family: var(--font-mono);
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.01em;
  }

  /* Session-mode toggles (RTK / FAST) — mono uppercase pills with a
     5px status dot (filled = on, hollow = off). OFF is the quiet base
     (hairline border + faint text); ON lifts to accent-soft fill +
     rgba(fg,.30) border + full-strength text. Toggles apply to the
     NEXT claude spawn (spawn-level env-var semantics in
     `claude.rs::spawn_claude_armed`). */
  .cbar-pill {
    display: inline-flex; align-items: center;
    gap: 5px;
    padding: 3px 7px;
    border-radius: 5px;
    border: 1px solid var(--border);
    background: transparent;
    color: var(--text-mute);
    font-family: var(--font-mono);
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    cursor: pointer;
    flex-shrink: 0;
    transition: background 120ms, border-color 120ms, color 120ms;
  }
  .cbar-pill:hover:not(:disabled) {
    background: var(--bg-3);
    border-color: var(--border-hi);
    color: var(--text-1);
  }
  /* ON — active session mode: accent-soft fill, fg-tinted border. */
  .cbar-pill--on {
    background: var(--accent-soft);
    border-color: color-mix(in srgb, var(--text-0) 30%, transparent);
    color: var(--text-0);
  }
  .cbar-pill--on:hover:not(:disabled) {
    background: color-mix(in srgb, var(--accent) 22%, var(--accent-soft));
    border-color: color-mix(in srgb, var(--text-0) 42%, transparent);
    color: var(--text-0);
  }
  /* Error — bundled rtk binary missing / corrupt. Disabled so the user
     can't toggle into a broken state; tooltip surfaces the recovery. */
  .cbar-pill--error {
    background: color-mix(in srgb, var(--error) 10%, transparent);
    border-color: color-mix(in srgb, var(--error) 35%, var(--border));
    color: var(--error);
    cursor: not-allowed;
  }
  .cbar-pill-glyph {
    font-family: var(--font-mono);
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.08em;
  }
  /* 5px status dot — hollow ring by default (off), filled when on.
     Inherits the pill's current text color. */
  .cbar-dot {
    width: 5px; height: 5px; border-radius: 50%;
    background: transparent;
    box-shadow: inset 0 0 0 1.4px currentColor;
    flex-shrink: 0;
  }
  .cbar-pill--on .cbar-dot {
    background: currentColor;
    box-shadow: none;
  }
  @media (prefers-reduced-motion: reduce) {
    .cbar-launch, .cbar-pill { transition: none; }
  }

  /* §2.5 — accent primary Send, engraved shadow-pill; ⏎ kbd at op.55.
     Sits last in DOM order so the spring right-aligns it. */
  .cbar-send {
    display: inline-flex; align-items: center; gap: 6px;
    padding: 6px 14px;
    border-radius: 8px;
    font-size: 12.5px; font-weight: 600;
    background: var(--accent);
    color: var(--accent-fg);
    border: none; cursor: pointer;
    box-shadow: var(--shadow-pill);
    transition: transform 120ms, box-shadow 200ms;
  }
  .cbar-send:hover:not(:disabled) {
    transform: translate(-1px, -1px);
    box-shadow: var(--shadow-2);
  }
  .cbar-send:disabled { opacity: 0.45; cursor: not-allowed; box-shadow: none; }
  .cbar-send-kbd {
    font-family: var(--font-mono);
    font-size: 10px;
    color: var(--accent-fg);
    opacity: 0.55;
  }

  .cbar-stop {
    display: inline-flex; align-items: center; gap: 5px;
    padding: 6px 11px;
    border-radius: 7px;
    font-size: 12px; font-weight: 500;
    background: color-mix(in srgb, var(--error) 10%, transparent);
    border: 1px solid color-mix(in srgb, var(--error) 30%, transparent);
    color: var(--error);
    cursor: pointer;
    transition: background 140ms;
  }
  .cbar-stop:hover { background: color-mix(in srgb, var(--error) 18%, transparent); }
  .cbar-stop svg { width: 11px; height: 11px; }

  /* "Queue" variant of Send — appears alongside Stop while a turn is in
     flight. Same shape as Send but tinted neutral so it reads as "park
     for later" instead of "fire now". */
  .cbar-send--queue {
    background: linear-gradient(180deg,
      color-mix(in srgb, var(--accent) 28%, var(--bg-3)),
      color-mix(in srgb, var(--accent) 14%, var(--bg-3)));
    color: var(--text-0);
    box-shadow:
      0 1px 0 rgba(0, 0, 0, 0.10),
      inset 0 1px 0 rgba(255, 255, 255, 0.08);
  }
  .cbar-send--queue:hover:not(:disabled) {
    background: linear-gradient(180deg,
      color-mix(in srgb, var(--accent) 38%, var(--bg-3)),
      color-mix(in srgb, var(--accent) 22%, var(--bg-3)));
  }

  /* Queue chip (icon + count) — quiet mono ghost; opens the floating
     queued-messages panel below. */
  .cbar-queue-wrap { position: relative; }
  .cbar-queue {
    display: inline-flex; align-items: center; gap: 4px;
    padding: 3px 8px;
    border-radius: 6px;
    background: transparent;
    border: 1px solid var(--border);
    color: var(--text-mute);
    font-family: var(--font-mono);
    font-size: 11px; font-weight: 600;
    cursor: pointer;
    transition: background 120ms, border-color 120ms, color 120ms;
    user-select: none;
    font-variant-numeric: tabular-nums;
  }
  .cbar-queue:hover,
  .cbar-queue--open {
    background: var(--bg-3);
    border-color: var(--border-hi);
    color: var(--text-1);
  }
  .cbar-queue svg { width: 11px; height: 11px; }

  .cmp-queue-panel {
    position: absolute;
    bottom: calc(100% + 8px);
    right: 0;
    width: 320px;
    background: var(--bg-1);
    border: 1px solid var(--border-hi);
    border-radius: 10px;
    box-shadow: 0 0 0 1px rgba(0,0,0,0.12), var(--shadow-3);
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
