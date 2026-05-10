<script lang="ts">
  /* InlineClaude — правая панель EditorApp.
     Per-session row exposes:
       - per-row "Open" button: focuses the session AND switches the
         top-level view to its agent app (Claude / Cursor)
       - click on the row body: toggles an inline mini-composer below
         where the user can dash off a message without leaving the
         editor. The message is delivered via the parent's
         `onQuickSend(sessionId, text)` — which queues if the session
         is currently mid-turn, or fires immediately if idle.
     The panel-level "Open Claude" CTA in the header still exists for
     the no-sessions empty state and as a quick-jump for users who
     just want the full Claude app. */
  import { sessionsState, setSessionInput } from '$lib/state/sessions.svelte';
  import { kindForInstanceId } from '$lib/state/layout.svelte';

  interface Props {
    instanceId: string;
    onLinkToAgent: (agentInstanceId: string) => void;
    onClose: () => void;
    onOpenClaude: () => void;
    /** Quick-send to a specific session. Parent decides whether to
     *  fire now or queue based on the session's `sending` state. */
    onQuickSend: (sessionId: string, text: string) => void;
    /** Activate the session AND switch the top-level view to its
     *  agent app — same as the panel-level "Open Claude" CTA but
     *  scoped to a specific session. */
    onOpenSession: (sessionId: string, agentInstanceId: string) => void;
  }
  let p: Props = $props();

  const linkedAgents = $derived.by(() => {
    const out: { sessionId: string; agentInstanceId: string; kind: 'claude' | 'cursor'; title: string; sending: boolean; queueLen: number }[] = [];
    for (const s of sessionsState.list) {
      if (!s.linkedToEditor) continue;
      if (s.linkedToEditorInstanceId !== p.instanceId) continue;
      if (!s.columnInstanceId) continue;
      const kind = kindForInstanceId(s.columnInstanceId);
      if (kind !== 'claude' && kind !== 'cursor') continue;
      out.push({
        sessionId: s.id,
        agentInstanceId: s.columnInstanceId,
        kind,
        title: s.title,
        sending: s.sending,
        queueLen: s.pendingQueue?.length ?? 0
      });
    }
    return out;
  });

  /** Which row is currently expanded (mini-composer visible). One at
   *  a time — opening another collapses the previous so we don't tile
   *  multiple textareas in a 280px panel. */
  let expandedSessionId = $state<string | null>(null);

  /** Auto-expand the matching row when `applyRangeToAgent` (Editor's
   *  selection bar) signals it. Cleared back to null after consumption
   *  so the next click of "Apply to <other-session>" works the same
   *  way. The check filters by linkedAgents so a signal targeting a
   *  session NOT linked to this editor doesn't accidentally trigger
   *  here. */
  $effect(() => {
    const sid = sessionsState.requestInlineExpandFor;
    if (!sid) return;
    const isLinkedHere = linkedAgents.some((la) => la.sessionId === sid);
    if (!isLinkedHere) {
      sessionsState.requestInlineExpandFor = null;
      return;
    }
    expandedSessionId = sid;
    /* queueMicrotask: clear AFTER Svelte settles the expand state so
       the effect doesn't immediately re-trigger on the next tick. */
    queueMicrotask(() => {
      sessionsState.requestInlineExpandFor = null;
    });
  });

  function toggleExpand(sessionId: string) {
    expandedSessionId = expandedSessionId === sessionId ? null : sessionId;
  }

  /** Read/write the SAME `sess.input` field the agent app's main
   *  composer uses — so what you type here mirrors there and vice
   *  versa, and `applyRangeToAgent`'s mention appended to `sess.input`
   *  is visible in both places without extra plumbing. */
  function getDraft(sessionId: string): string {
    return sessionsState.list.find((s) => s.id === sessionId)?.input ?? '';
  }
  function setDraft(sessionId: string, value: string) {
    setSessionInput(sessionId, value);
  }

  function sendDraft(sessionId: string) {
    const text = getDraft(sessionId).trim();
    if (!text) return;
    p.onQuickSend(sessionId, text);
    /* Clear `sess.input` so both composers reset for the next prompt;
       stay expanded — user might want to dash off another follow-up. */
    setSessionInput(sessionId, '');
  }

  function onDraftKey(e: KeyboardEvent, sessionId: string) {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      sendDraft(sessionId);
    }
  }
</script>

<aside class="ic">
  <header class="ic-head">
    <span class="ic-brand">
      <svg viewBox="0 0 24 24" fill="currentColor" stroke="none"><path d="M5 4h14a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2H8l-5 4V6a2 2 0 0 1 2-2z"/></svg>
    </span>
    <span class="ic-title-block">
      <span class="ic-title serif">Inline Claude</span>
      <span class="ic-sub mono">{linkedAgents.length} linked · this editor</span>
    </span>
    <button class="ic-x" title="Open Claude (⇧⌘L)" onclick={p.onOpenClaude}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7"><path d="M15 3h6v6M9 21H3v-6M21 3l-7 7M3 21l7-7"/></svg>
    </button>
    <button class="ic-x" title="Hide" onclick={p.onClose}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7"><path d="M18 6 6 18M6 6l12 12"/></svg>
    </button>
  </header>

  <div class="ic-body">
    {#if linkedAgents.length === 0}
      <div class="ic-empty">
        <div class="ic-empty-icon">
          <svg viewBox="0 0 24 24" fill="currentColor" stroke="none"><path d="M12 2 L14.5 9.5 L22 12 L14.5 14.5 L12 22 L9.5 14.5 L2 12 L9.5 9.5 Z"/></svg>
        </div>
        <p class="ic-empty-h serif">No agents linked</p>
        <p class="ic-empty-p">
          From any Claude / Cursor session use <strong>Link to Editor</strong> in
          the cwd bar. Linked sessions appear here for quick switching.
        </p>
        <button class="ic-cta" onclick={p.onOpenClaude}>
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M12 5v14M5 12h14"/></svg>
          Open Claude
        </button>
      </div>
    {:else}
      {#each linkedAgents as la (la.sessionId)}
        {@const isExpanded = expandedSessionId === la.sessionId}
        <div class="ic-link-card" class:ic-link-card--expanded={isExpanded}>
          <div class="ic-link-row">
            <button
              class="ic-link-main"
              onclick={() => toggleExpand(la.sessionId)}
              title={isExpanded ? 'Collapse quick-send' : 'Quick-send to this session'}
            >
              <span class="ic-link-icon">
                {#if la.kind === 'claude'}
                  <svg viewBox="0 0 24 24" fill="currentColor"><path d="M5 4h14a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2H8l-5 4V6a2 2 0 0 1 2-2z"/></svg>
                {:else}
                  <svg viewBox="0 0 24 24" fill="currentColor"><path d="M3 3l8 18 2-8 8-2z"/></svg>
                {/if}
              </span>
              <span class="ic-link-body">
                <span class="ic-link-title">{la.title}</span>
                <span class="ic-link-sub">
                  {la.kind === 'claude' ? 'Claude' : 'Cursor'}
                  {#if la.sending}
                    <span class="ic-status ic-status--running">
                      <span class="ic-pulse"></span>
                      running
                    </span>
                  {/if}
                  {#if la.queueLen > 0}
                    <span class="ic-status ic-status--queued">queued: {la.queueLen}</span>
                  {/if}
                  {#if !la.sending && la.queueLen === 0}
                    <span class="ic-status ic-status--idle">idle · click to ask</span>
                  {/if}
                </span>
              </span>
              <span class="ic-link-caret" class:ic-link-caret--open={isExpanded} aria-hidden="true">
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M6 9l6 6 6-6"/></svg>
              </span>
            </button>
            <button
              class="ic-link-open"
              onclick={() => p.onOpenSession(la.sessionId, la.agentInstanceId)}
              title="Open this chat in the {la.kind === 'claude' ? 'Claude' : 'Cursor'} app"
              aria-label="Open in {la.kind === 'claude' ? 'Claude' : 'Cursor'}"
            >
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"><path d="M15 3h6v6"/><path d="M21 3l-7 7"/><path d="M19 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V7a2 2 0 0 1 2-2h6"/></svg>
            </button>
          </div>

          {#if isExpanded}
            <div class="ic-quick">
              <textarea
                class="ic-quick-area"
                placeholder={la.sending
                  ? 'Type a message — it\'ll queue and fire when the current turn finishes (Enter to queue)'
                  : 'Quick message to ' + (la.kind === 'claude' ? 'Claude' : 'Cursor') + ' — Enter to send, Shift+Enter for newline'}
                value={getDraft(la.sessionId)}
                oninput={(e) => setDraft(la.sessionId, (e.currentTarget as HTMLTextAreaElement).value)}
                onkeydown={(e) => onDraftKey(e, la.sessionId)}
                rows="3"
              ></textarea>
              <div class="ic-quick-row">
                <span class="ic-quick-hint mono">
                  {#if la.sending}
                    will queue · response in chat
                  {:else}
                    sends immediately · response in chat
                  {/if}
                </span>
                <button
                  class="ic-quick-send"
                  class:ic-quick-send--queue={la.sending}
                  disabled={!getDraft(la.sessionId).trim()}
                  onclick={() => sendDraft(la.sessionId)}
                >
                  {la.sending ? 'Queue' : 'Send'}
                </button>
              </div>
            </div>
          {/if}
        </div>
      {/each}
    {/if}
  </div>
</aside>

<style>
  .ic {
    display: grid; grid-template-rows: 46px 1fr;
    background: var(--bg-1);
    border-left: 1px solid var(--border);
    min-height: 0;
    width: 280px; flex: 0 0 280px;
  }
  .ic-head {
    display: flex; align-items: center; gap: 10px;
    padding: 0 12px;
    border-bottom: 1px solid var(--border);
    background:
      linear-gradient(180deg, color-mix(in srgb, var(--src-claude) 8%, transparent), transparent),
      var(--bg-1);
  }
  .ic-brand {
    width: 24px; height: 24px;
    display: grid; place-items: center;
    border-radius: 6px;
    background: color-mix(in srgb, var(--src-claude) 10%, var(--bg-2));
    color: var(--src-claude);
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--src-claude) 26%, transparent);
  }
  .ic-brand svg { width: 13px; height: 13px; }
  .ic-title-block { flex: 1; display: flex; flex-direction: column; gap: 1px; min-width: 0; }
  .ic-title {
    font-family: 'Instrument Serif', 'New York', Georgia, serif;
    font-size: 16px; font-weight: 400; letter-spacing: -0.01em;
    color: var(--text-0); line-height: 1.1;
  }
  .ic-sub { font-size: 10px; color: var(--text-mute); }
  .ic-x {
    width: 24px; height: 24px;
    display: grid; place-items: center;
    color: var(--text-2);
    background: transparent; border: none; cursor: pointer;
    border-radius: 5px;
    transition: color 140ms, background 140ms;
  }
  .ic-x:hover { color: var(--text-0); background: var(--bg-elev, var(--bg-2)); }
  .ic-x svg { width: 13px; height: 13px; }

  .ic-body {
    overflow-y: auto;
    padding: 14px;
    display: flex; flex-direction: column; gap: 8px;
  }
  .ic-empty {
    text-align: center;
    margin: auto 0;
    padding: 30px 14px;
  }
  .ic-empty-icon {
    width: 56px; height: 56px;
    margin: 0 auto 18px;
    display: grid; place-items: center;
    border-radius: 14px;
    background: color-mix(in srgb, var(--src-claude) 10%, var(--bg-2));
    color: var(--src-claude);
    box-shadow:
      inset 0 0 0 1px color-mix(in srgb, var(--src-claude) 24%, transparent),
      0 0 28px rgba(232, 155, 125, 0.32);
  }
  .ic-empty-icon svg { width: 26px; height: 26px; }
  .ic-empty-h {
    font-family: 'Instrument Serif', 'New York', Georgia, serif;
    font-size: 22px; font-weight: 400; letter-spacing: -0.015em;
    color: var(--text-0);
    margin: 0 0 10px;
  }
  .ic-empty-p {
    font-size: 12.5px; color: var(--text-2);
    line-height: 1.55; margin: 0 0 18px;
  }
  .ic-cta {
    display: inline-flex; align-items: center; gap: 6px;
    padding: 7px 12px;
    border-radius: 8px;
    font-size: 12px; font-weight: 600;
    background: linear-gradient(180deg, var(--accent-bright), var(--accent));
    color: var(--accent-fg);
    border: none; cursor: pointer;
    box-shadow:
      0 6px 18px rgba(204, 120, 92, 0.30),
      inset 0 1px 0 rgba(255, 230, 200, 0.32);
    transition: transform 140ms;
  }
  .ic-cta:hover { transform: translateY(-1px); }
  .ic-cta svg { width: 12px; height: 12px; }

  /* Per-session card. Container shape stays consistent whether
     collapsed (just the row) or expanded (row + composer below).
     Border tint hot-swaps to clay when the row is open so the user
     reads "this is the active editing target". */
  .ic-link-card {
    border-radius: 9px;
    border: 1px solid var(--border);
    background: var(--bg-2);
    overflow: hidden;
    transition: border-color 140ms;
  }
  .ic-link-card:hover { border-color: var(--border-hi); }
  .ic-link-card--expanded {
    border-color: color-mix(in srgb, var(--src-claude) 38%, var(--border));
    background: color-mix(in srgb, var(--src-claude) 4%, var(--bg-2));
  }

  .ic-link-row {
    display: flex; align-items: stretch;
    gap: 0;
  }
  .ic-link-main {
    flex: 1; min-width: 0;
    display: flex; align-items: center; gap: 10px;
    padding: 10px 8px 10px 11px;
    background: transparent;
    border: 0;
    text-align: left;
    cursor: pointer;
  }
  .ic-link-main:hover { background: color-mix(in srgb, var(--src-claude) 4%, transparent); }
  .ic-link-icon {
    width: 28px; height: 28px;
    display: grid; place-items: center;
    border-radius: 7px;
    background: color-mix(in srgb, var(--src-claude) 10%, var(--bg-3));
    color: var(--src-claude);
    box-shadow: inset 0 0 0 1px var(--border);
    flex-shrink: 0;
  }
  .ic-link-icon svg { width: 14px; height: 14px; }
  .ic-link-body { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 2px; }
  .ic-link-title {
    font-size: 12.5px; color: var(--text-0); font-weight: 500;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .ic-link-sub {
    font-size: 10.5px; color: var(--text-mute);
    display: inline-flex; align-items: center; gap: 6px;
    flex-wrap: wrap;
  }

  /* Status pills inside the row sub-text. running = pulsing dot,
     queued = neutral count chip, idle = greyed hint. */
  .ic-status {
    display: inline-flex; align-items: center; gap: 4px;
    font-size: 9.5px;
    padding: 1px 6px;
    border-radius: 999px;
    border: 1px solid transparent;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    font-weight: 600;
  }
  .ic-status--running {
    color: var(--src-claude);
    background: color-mix(in srgb, var(--src-claude) 14%, transparent);
    border-color: color-mix(in srgb, var(--src-claude) 30%, transparent);
  }
  .ic-status--queued {
    color: var(--accent-bright);
    background: color-mix(in srgb, var(--accent) 14%, transparent);
    border-color: color-mix(in srgb, var(--accent) 30%, transparent);
  }
  .ic-status--idle {
    color: var(--text-mute);
    background: transparent;
    border-color: transparent;
    text-transform: none;
    letter-spacing: 0;
    font-weight: 500;
    font-size: 10px;
    padding: 0;
  }
  .ic-pulse {
    width: 5px; height: 5px;
    border-radius: 50%;
    background: var(--src-claude);
    box-shadow: 0 0 6px color-mix(in srgb, var(--src-claude) 70%, transparent);
    animation: ic-pulse 1.2s ease-in-out infinite;
  }
  @keyframes ic-pulse {
    0%, 100% { opacity: 0.45; transform: scale(0.85); }
    50%      { opacity: 1; transform: scale(1.15); }
  }

  .ic-link-caret {
    color: var(--text-mute);
    display: grid; place-items: center;
    flex-shrink: 0;
    transition: transform 160ms;
  }
  .ic-link-caret svg { width: 12px; height: 12px; }
  .ic-link-caret--open { transform: rotate(180deg); color: var(--src-claude); }

  /* Per-row "Open in <Agent>" button. Anchored to the right edge of
     the row, separate from the click-to-expand main area so it's
     unambiguous: tap row body → toggle composer; tap arrow icon →
     jump to the agent app. */
  .ic-link-open {
    width: 36px;
    display: grid; place-items: center;
    border: 0;
    border-left: 1px solid var(--border);
    background: transparent;
    color: var(--text-mute);
    cursor: pointer;
    transition: background 140ms, color 140ms;
  }
  .ic-link-open:hover {
    background: color-mix(in srgb, var(--src-claude) 14%, transparent);
    color: var(--src-claude);
  }
  .ic-link-open svg { width: 13px; height: 13px; }

  /* Inline mini-composer revealed on row expand. */
  .ic-quick {
    border-top: 1px solid var(--border);
    padding: 10px 11px 11px;
    display: flex; flex-direction: column; gap: 8px;
    background: var(--bg-1);
  }
  .ic-quick-area {
    width: 100%;
    resize: vertical;
    min-height: 60px;
    max-height: 200px;
    padding: 8px 10px;
    border-radius: 7px;
    border: 1px solid var(--border);
    background: var(--bg-2);
    color: var(--text-0);
    font-family: inherit;
    font-size: 12px; line-height: 1.5;
    outline: none;
    transition: border-color 120ms;
  }
  .ic-quick-area:focus { border-color: var(--border-accent); background: var(--bg-1); }
  .ic-quick-area::placeholder { color: var(--text-mute); }
  .ic-quick-row {
    display: flex; align-items: center; justify-content: space-between;
    gap: 8px;
  }
  .ic-quick-hint {
    font-size: 9.5px; color: var(--text-mute);
    text-transform: uppercase; letter-spacing: 0.05em;
  }
  .ic-quick-send {
    padding: 5px 14px;
    border-radius: 6px;
    font-size: 11.5px; font-weight: 600;
    background: linear-gradient(180deg, var(--accent-bright), var(--accent));
    color: var(--accent-fg);
    border: 0; cursor: pointer;
    box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.18);
    transition: transform 100ms, box-shadow 200ms;
  }
  .ic-quick-send:hover:not(:disabled) {
    transform: translateY(-1px);
    box-shadow: 0 3px 10px var(--accent-glow), inset 0 1px 0 rgba(255, 255, 255, 0.18);
  }
  .ic-quick-send:disabled { opacity: 0.45; cursor: not-allowed; box-shadow: none; }
  /* Queue variant — same shape, neutral fill so the user reads
     "park for later" instead of "fire now". */
  .ic-quick-send--queue {
    background: linear-gradient(180deg,
      color-mix(in srgb, var(--accent) 30%, var(--bg-3)),
      color-mix(in srgb, var(--accent) 14%, var(--bg-3)));
    color: var(--text-0);
  }
  .ic-quick-send--queue:hover:not(:disabled) {
    background: linear-gradient(180deg,
      color-mix(in srgb, var(--accent) 40%, var(--bg-3)),
      color-mix(in srgb, var(--accent) 22%, var(--bg-3)));
  }
</style>
