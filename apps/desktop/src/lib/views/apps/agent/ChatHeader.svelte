<script lang="ts">
  /* ChatHeader — top row of AgentApp center pane.
     Shows the active session title (Geist, large) + a running pulse +
     stop button while a turn is in flight.

     Title rename: click on the title text or its little pencil hint
     to enter inline-edit mode. Enter or blur commits the new name to
     the session via `updateSession`; Esc cancels and reverts. The
     input shares glyph + size with the static span so the row doesn't
     reflow when entering / leaving edit mode. Falls back to "Untitled
     chat" when the user clears the field on save (otherwise the
     SessionsSidebar would render an empty row). */
  import { sessionsState, updateSession } from '$lib/state/sessions.svelte';
  import { tick } from 'svelte';

  type Kind = 'claude' | 'cursor';

  interface Props {
    kind: Kind;
    instanceId: string;
    thinkingStartedAt: number | null;
    thinkingTick: number;
    onStop: () => void;
  }

  let p: Props = $props();

  const sess = $derived(
    sessionsState.list.find((s) => s.id === sessionsState.activeIds[p.kind]) ?? null
  );

  const elapsed = $derived.by(() => {
    if (!p.thinkingStartedAt || !sess?.sending) return '';
    void p.thinkingTick;
    const ms = Date.now() - p.thinkingStartedAt;
    const s = Math.floor(ms / 1000);
    if (s < 60) return `${s}s`;
    const m = Math.floor(s / 60);
    const r = s % 60;
    return `${m}m ${String(r).padStart(2, '0')}s`;
  });

  /* ---------- Rename ---------- */

  /** Which session the user is currently renaming (null = static
   *  title). Tracked by id so a session swap mid-edit auto-cancels
   *  the rename instead of bleeding draft text into a different chat. */
  let editingSessionId = $state<string | null>(null);
  let draftTitle = $state('');
  let inputEl = $state<HTMLInputElement | null>(null);

  async function startRename() {
    if (!sess) return;
    editingSessionId = sess.id;
    draftTitle = sess.title || '';
    /* Wait for Svelte to mount the input, then select-all so a
       single press of Enter / Backspace replaces the whole title. */
    await tick();
    inputEl?.focus();
    inputEl?.select();
  }

  function commitRename() {
    if (!sess || editingSessionId !== sess.id) {
      editingSessionId = null;
      return;
    }
    const trimmed = draftTitle.trim();
    /* Empty input falls back to "Untitled chat" so list rows in the
       sidebar always render something — same as the original empty-
       state placeholder. */
    const next = trimmed || 'Untitled chat';
    if (next !== sess.title) {
      updateSession(sess.id, { title: next });
    }
    editingSessionId = null;
  }

  function cancelRename() {
    editingSessionId = null;
  }

  function onTitleKey(e: KeyboardEvent) {
    if (e.key === 'Enter') {
      e.preventDefault();
      commitRename();
    } else if (e.key === 'Escape') {
      e.preventDefault();
      cancelRename();
    }
  }

  /* If the active session changes while an edit is in flight, drop
     the draft so it doesn't accidentally apply to the new chat. */
  $effect(() => {
    if (!sess) {
      editingSessionId = null;
      return;
    }
    if (editingSessionId && editingSessionId !== sess.id) {
      editingSessionId = null;
    }
  });
</script>

<header class="ch">
  <div class="ch-title">
    {#if sess}
      {#if editingSessionId === sess.id}
        <input
          bind:this={inputEl}
          class="ch-sess ch-sess-input"
          bind:value={draftTitle}
          onkeydown={onTitleKey}
          onblur={commitRename}
          maxlength="120"
          aria-label="Chat title"
          spellcheck="false"
        />
      {:else}
        <button
          class="ch-sess-btn"
          onclick={startRename}
          title="Rename chat (click to edit)"
          aria-label="Rename chat"
        >
          <span class="ch-sess" class:ch-sess--empty={!sess.title}>
            {sess.title || 'Untitled chat'}
          </span>
          <svg class="ch-rename-hint" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7"/>
            <path d="M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4z"/>
          </svg>
        </button>
      {/if}
    {:else}
      <span class="ch-sess ch-sess--empty">No session</span>
    {/if}
  </div>

  {#if sess?.sending}
    <span class="ch-running">
      <span class="ch-pip"></span>
      <span class="mono">{elapsed || 'thinking'}</span>
    </span>

    <button class="ch-stop" onclick={p.onStop} title="Stop generation">
      <svg viewBox="0 0 24 24" fill="currentColor"><rect x="6" y="6" width="12" height="12" rx="1.5"/></svg>
    </button>
  {/if}
</header>

<style>
  .ch {
    flex: 0 0 56px;
    display: flex; align-items: center; gap: 12px;
    padding: 0 22px;
    border-bottom: 1px solid var(--border);
    background: linear-gradient(180deg, var(--bg-2), var(--bg-1));
    min-height: 0;
  }
  .ch-title {
    flex: 1; min-width: 0;
    /* Center alignment plays nicer with the inline input — baseline
       was fine for plain text but pushed the input box above the
       row's vertical centre, leaving a visual gap below the focus
       ring. */
    display: flex; align-items: center; gap: 8px;
    font-family: 'Geist', 'Inter', -apple-system, system-ui, sans-serif;
    font-size: 22px; font-weight: 600;
    letter-spacing: -0.02em;
    white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
  }
  /* Static title chip — looks like text, behaves like a click target.
     The pencil hint stays faded until hover so the title doesn't read
     as a button at rest, but the affordance is discoverable on hover.

     No negative margins: those used to pull the chip 8px left so the
     baseline text aligned with the original `.ch-sess` span position,
     but combined with `.ch-title`'s `overflow: hidden` they made the
     hover bg clip lopsidedly on the left and bled the rename input's
     border off the visible row. Symmetric padding + flush-left
     alignment trades a small right-shift in the title for a clean
     pill on hover and a fully-visible input border during rename. */
  .ch-sess-btn {
    display: inline-flex; align-items: center; gap: 8px;
    background: transparent; border: 0;
    padding: 3px 10px;
    border-radius: 8px;
    cursor: text;
    color: inherit;
    font: inherit;
    letter-spacing: inherit;
    transition: background 140ms;
    max-width: 100%;
    min-width: 0;
  }
  .ch-sess-btn:hover { background: var(--bg-2); }
  .ch-sess-btn:hover .ch-rename-hint { opacity: 0.5; }
  .ch-rename-hint {
    width: 13px; height: 13px;
    color: var(--text-mute);
    opacity: 0;
    transition: opacity 140ms;
    flex-shrink: 0;
  }
  .ch-sess {
    color: var(--text-0);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }
  .ch-sess--empty { color: var(--text-mute); }

  /* Inline rename input — fits the natural title length instead of
     spanning the full title flex column, and stays inside the 56px
     header bounds. Aligned with the static `.ch-sess-btn` (same
     padding, no negative margin) so the row doesn't shift sideways
     when entering / leaving edit mode and the left border doesn't
     get clipped by `.ch-title`'s overflow: hidden. */
  .ch-sess-input {
    flex: 0 1 auto;
    min-width: 180px;
    max-width: 480px;
    width: auto;
    background: var(--bg-2);
    border: 1px solid var(--accent);
    border-radius: 8px;
    padding: 2px 10px;
    color: var(--text-0);
    font-family: inherit;
    font-size: inherit;
    font-weight: inherit;
    letter-spacing: inherit;
    outline: none;
  }
  .ch-sess-input:focus {
    /* Mint hairline ring on focus — thin enough to stay inside the
       row, bright enough to read as "you're editing". */
    box-shadow: 0 0 0 1px color-mix(in srgb, var(--accent) 60%, transparent);
  }

  .ch-running {
    display: inline-flex; align-items: center; gap: 6px;
    font-size: 11px; color: var(--text-mute);
  }
  .ch-pip {
    width: 6px; height: 6px; border-radius: 50%;
    background: var(--app-tone, var(--accent));
    box-shadow: 0 0 6px var(--app-glow, var(--accent-glow));
    animation: ch-pulse 1.4s infinite;
  }
  @keyframes ch-pulse {
    0%, 100% { opacity: 0.45; transform: scale(0.9); }
    50%      { opacity: 1;    transform: scale(1.1); }
  }

  .ch-stop {
    width: 28px; height: 28px;
    display: grid; place-items: center;
    border-radius: 7px;
    background: rgba(232, 130, 100, 0.10);
    border: 1px solid rgba(232, 130, 100, 0.32);
    color: var(--error);
    cursor: pointer;
    transition: background 140ms;
  }
  .ch-stop:hover { background: rgba(232, 130, 100, 0.18); }
  .ch-stop svg { width: 12px; height: 12px; }
</style>
