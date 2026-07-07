<script lang="ts">
  /* SessionsSidebar — left pane of AgentApp.
     v7: serif "Claude" head + "+" iconbtn, group labels
     (Today / Yesterday / This week / Older), session rows with sparkle
     icon + 13px title + meta line (time · N msgs · status), bottom
     dashed "+ New chat" button. */

  import {
    sessionsState,
    focusSession,
    setActiveSessionInInstance,
    newClaudeSession,
    deleteClaudeSession,
    restoreClaudeSession,
    purgeClaudeSession,
    updateSession
  } from '$lib/state/sessions.svelte';
  import { relativeTime } from '$lib/data';
  import BrandIcon from '$lib/components/ui/BrandIcon.svelte';
  import CardContextMenu, { type MenuItem } from '$lib/views/apps/_shared/CardContextMenu.svelte';
  import { notify } from '$lib/state/toaster.svelte';
  import { invoke } from '@tauri-apps/api/core';

  type Kind = 'claude';

  interface Props {
    kind: Kind;
    /** App instance the active session is bound to (worktree ownership,
     *  MCP routing). App view receives this from +page.svelte. */
    instanceId: string;
    now: number;
  }

  let { kind, instanceId, now }: Props = $props();

  type Session = (typeof sessionsState.list)[number];

  /* Search filter over chat titles (redesign v2 §2.5). */
  let query = $state('');

  /* Flat, newest-first — the paper mockup has no date groups; the
     status dot + meta line carry the recency signal instead. */
  const sorted = $derived.by(() => {
    const q = query.trim().toLowerCase();
    const items = sessionsState.list.filter(
      (s) => !s.archived && (!q || (s.title || 'Untitled chat').toLowerCase().includes(q))
    );
    const sessTime = (s: Session) => {
      const last = s.messages[s.messages.length - 1]?.at;
      return last ? new Date(last).getTime() : 0;
    };
    return [...items].sort((a, b) => sessTime(b) - sessTime(a));
  });

  /** Shorten a model id for the meta line: `claude-opus-4-8[1m]` →
   *  `opus-4.8·1m`, `claude-sonnet-4-6` → `sonnet-4.6`,
   *  `claude-haiku-4-5-20251001` → `haiku-4.5`. */
  function shortModel(m: string | null | undefined): string {
    if (!m) return '';
    let s = m.replace(/^claude-/, '');
    s = s.replace(/-(\d+)-(\d+)/, '-$1.$2'); // 4-8 → 4.8
    s = s.replace('[1m]', '·1m');
    s = s.replace(/-\d{8}$/, '');            // drop haiku date suffix
    return s;
  }

  /** Meta line per the mockup: `opus-4.8·1m · idle · 19:26` /
   *  `… · wt/branch`. */
  function sessMeta(sess: Session): string {
    const parts: string[] = [];
    const model = shortModel(sess.claudeModel);
    if (model) parts.push(model);
    parts.push(sess.sending ? 'streaming' : 'idle');
    if (sess.worktreeBranch) parts.push(`wt/${sess.worktreeBranch}`);
    const lastAt = sess.messages[sess.messages.length - 1]?.at;
    const t = shortTime(lastAt ?? undefined);
    if (t) parts.push(t);
    return parts.join(' · ');
  }

  const totalCount = $derived(
    sessionsState.list.filter((s) => !s.archived).length
  );

  /* Archived chats of this kind — newest-archived first. Rendered in a
     collapsible section at the bottom of the list; restore brings one
     back, "delete forever" purges it for good. */
  const archivedItems = $derived.by(() =>
    sessionsState.list
      .filter((s) => s.archived)
      .sort((a, b) => (b.archivedAt ?? 0) - (a.archivedAt ?? 0))
  );
  let showArchived = $state(false);

  /* Per-session memory presence. Keyed by the 8-char id prefix the
     auto-distill / paste-trap / right-click-save flows write into
     `from-session:<prefix>` tags. Fetched once on mount + on session
     list growth (every new session might add memories elsewhere) —
     no need to refetch on every render since memory writes happen
     from a few well-defined entry points. */
  let memCounts = $state<Record<string, number>>({});
  async function refreshMemCounts(): Promise<void> {
    try {
      const map = await invoke<Record<string, number>>('memory_session_counts_local');
      memCounts = map;
    } catch {
      /* Silent — sidebar still renders without the badge. */
      memCounts = {};
    }
  }
  $effect(() => { void refreshMemCounts(); });
  /* Re-fetch whenever the visible session count changes — a newly-
     deleted session triggers auto-distill which adds rows, and a
     newly-created session won't have rows yet. The cost is one cheap
     SQL scan; running it on count changes only avoids the per-keystroke
     waste of running on every reactive tick. */
  $effect(() => {
    void totalCount;
    void refreshMemCounts();
  });
  function memCountFor(sessId: string): number {
    return memCounts[sessId.slice(0, 8)] ?? 0;
  }

  const label = 'Claude';

  /* Right-click context menu on session rows. Reuses the shared
     CardContextMenu used in inbox lists + chat-thread messages.
     Session type is already declared above for the groups derivation
     — reuse the same alias here without re-declaring. */
  let ctxCoords = $state<{ x: number; y: number } | null>(null);
  let ctxSess = $state<Session | null>(null);

  function openSessCtx(e: MouseEvent, sess: Session) {
    e.preventDefault();
    e.stopPropagation();
    ctxCoords = { x: e.clientX, y: e.clientY };
    ctxSess = sess;
  }
  function closeSessCtx() {
    ctxCoords = null;
    ctxSess = null;
  }

  const ctxItems = $derived.by<MenuItem[]>(() => {
    const s = ctxSess;
    if (!s) return [];
    const items: MenuItem[] = [];
    items.push({
      label: 'Rename',
      icon: 'M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7 M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4z',
      onClick: () => {
        const next = window.prompt('New chat title:', s.title || '');
        if (next === null) return;
        const trimmed = next.trim();
        if (trimmed && trimmed !== s.title) {
          updateSession(s.id, { title: trimmed });
        }
      }
    });
    items.push({
      label: 'Copy transcript',
      icon: 'M16 4h2a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h2 M9 2h6a1 1 0 0 1 1 1v2H8V3a1 1 0 0 1 1-1z',
      onClick: async () => {
        const text = s.messages
          .filter((m) => m.content.trim().length > 0)
          .map((m) => `## ${m.role}\n\n${m.content}`)
          .join('\n\n---\n\n');
        try {
          await navigator.clipboard.writeText(text);
          notify({ kind: 'success', title: 'Transcript copied', ttlMs: 1800 });
        } catch (e) {
          notify({ kind: 'error', title: 'Copy failed', body: String(e) });
        }
      }
    });
    items.push({
      label: 'Save to memory',
      icon: 'M19 21l-7-5-7 5V5a2 2 0 0 1 2-2h10a2 2 0 0 1 2 2z',
      onClick: async () => {
        const users = s.messages.filter((m) => m.role === 'user' && m.content.trim());
        const asst = s.messages.filter((m) => m.role === 'assistant' && m.content.trim());
        if (users.length === 0) {
          notify({ kind: 'info', title: 'Nothing to distill yet', ttlMs: 2200 });
          return;
        }
        const cwdBase = (s.worktreePath || s.cwd || '')
          .split('/').filter(Boolean).pop() ?? '';
        const trunc = (str: string, n: number): string =>
          str.length > n ? str.slice(0, n - 1) + '…' : str;
        const body: string[] = [
          `Chat "${s.title || 'Untitled'}"${cwdBase ? ` (${cwdBase})` : ''}.`,
          `First user prompt: ${trunc(users[0].content.trim(), 1200)}`
        ];
        if (asst.length) {
          body.push(`Last agent reply: ${trunc(asst[asst.length - 1].content.trim(), 1200)}`);
        }
        const tags = ['manual-distill', `from-session:${s.id.slice(0, 8)}`];
        if (cwdBase) tags.push(`project:${cwdBase}`);
        try {
          await invoke<number>('memory_save_local', {
            content: body.join('\n\n'),
            kind: 'note',
            tags
          });
          notify({ kind: 'success', title: 'Saved chat to memory', ttlMs: 2200 });
          void refreshMemCounts();
        } catch (e) {
          notify({ kind: 'error', title: 'Memory save failed', body: String(e) });
        }
      }
    });
    items.push({
      label: 'Archive chat',
      icon: 'M21 8v13H3V8 M1 3h22v5H1z M10 12h4',
      danger: true,
      onClick: () => {
        /* Soft-delete → Archive. Recoverable from the Archived section;
           a memory snapshot is auto-distilled first. No confirm needed —
           it's reversible. */
        deleteClaudeSession(s.id);
        notify({ kind: 'success', title: 'Chat archived', ttlMs: 2000 });
      }
    });
    return items;
  });

  function restoreSession(sessId: string, e: MouseEvent) {
    e.stopPropagation();
    e.preventDefault();
    restoreClaudeSession(sessId);
  }

  function purgeSession(sessId: string, sessTitle: string, e: MouseEvent) {
    e.stopPropagation();
    e.preventDefault();
    if (!confirm(`Permanently delete "${sessTitle || 'Untitled chat'}"? This can't be undone.`)) {
      return;
    }
    purgeClaudeSession(sessId);
  }

  function pickSession(sessId: string) {
    setActiveSessionInInstance(instanceId, sessId);
    focusSession(sessId);
  }

  function createNew() {
    newClaudeSession({ agentInstanceId: instanceId });
  }

  function deleteSession(sessId: string, _sessTitle: string, e: MouseEvent) {
    /* Stop the archive-icon click from also bubbling to the row's
       click-to-activate handler. Soft-delete → Archive (reversible),
       so no confirm dialog — the user can restore from the Archived
       section below. */
    e.stopPropagation();
    e.preventDefault();
    deleteClaudeSession(sessId);
    notify({ kind: 'success', title: 'Chat archived', ttlMs: 2000 });
  }

  function shortTime(at: string | undefined): string {
    if (!at) return '';
    const d = new Date(at);
    const today = new Date();
    if (
      d.getFullYear() === today.getFullYear() &&
      d.getMonth() === today.getMonth() &&
      d.getDate() === today.getDate()
    ) {
      return `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`;
    }
    const days = Math.floor((today.getTime() - d.getTime()) / (24 * 60 * 60 * 1000));
    if (days === 1) return 'YDA';
    if (days < 7) return `${days}d`;
    if (days < 30) return `${Math.floor(days / 7)}w`;
    return `${Math.floor(days / 30)}mo`;
  }
</script>

<aside class="ssb">
  <div class="ssb-head">
    <span class="ssb-title">Chats</span>
    <span class="ssb-count mono">{totalCount}</span>
    <span class="ssb-head-spring"></span>
    <button class="ssb-add" onclick={createNew} title="New chat (⌘N)" aria-label="New chat">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" aria-hidden="true"><line x1="12" y1="5" x2="12" y2="19"/><line x1="5" y1="12" x2="19" y2="12"/></svg>
    </button>
  </div>

  <input
    class="ssb-search"
    bind:value={query}
    placeholder="Search chats"
    spellcheck="false"
    aria-label="Search chats"
  />

  <div class="ssb-list">
    {#if sorted.length === 0}
      <div class="ssb-empty">
        <p class="ssb-empty-h serif">No {label} sessions yet</p>
        <p class="ssb-empty-p">
          Click <strong>+</strong> to begin. Drop a Jira ticket,
          PR, or file onto the chat to attach context.
        </p>
      </div>
    {:else}
      {#each sorted as sess (sess.id)}
        {@const isActive = sess.id === sessionsState.activeIds.claude}
        <div
          class="ssb-row"
          class:active={isActive}
          role="button"
          tabindex="0"
          onclick={() => pickSession(sess.id)}
          onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') pickSession(sess.id); }}
          oncontextmenu={(e) => openSessCtx(e, sess)}
        >
          <div class="ssb-row-top">
            <span
              class="ssb-status"
              class:pulse={sess.sending}
              class:idle={!sess.sending}
            ></span>
            <span class="ssb-title" class:bold={isActive}>{sess.title || 'Untitled chat'}</span>
            {#if memCountFor(sess.id) > 0}
              <span class="ssb-mem" title="{memCountFor(sess.id)} long-term memories saved from this chat">
                <svg viewBox="0 0 24 24" width="10" height="10" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                  <path d="M19 21l-7-5-7 5V5a2 2 0 0 1 2-2h10a2 2 0 0 1 2 2z"/>
                </svg>
              </span>
            {/if}
            <button
              class="ssb-del"
              title="Archive chat"
              aria-label="Archive chat"
              onclick={(e) => deleteSession(sess.id, sess.title, e)}
            >
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M21 8v13H3V8M1 3h22v5H1zM10 12h4"/></svg>
            </button>
          </div>
          <div class="ssb-meta">{sessMeta(sess)}</div>
        </div>
      {/each}
    {/if}

    {#if archivedItems.length > 0}
      <button
        class="ssb-arch-toggle"
        onclick={() => (showArchived = !showArchived)}
        aria-expanded={showArchived}
      >
        <svg
          class="ssb-arch-caret"
          class:open={showArchived}
          viewBox="0 0 24 24" width="11" height="11"
          fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"
        ><path d="M9 6l6 6-6 6"/></svg>
        <span>Archived</span>
        <span class="ssb-arch-count mono">{archivedItems.length}</span>
      </button>
      {#if showArchived}
        {#each archivedItems as sess (sess.id)}
          <div class="ssb-row ssb-row--archived">
            <div class="ssb-icon">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M21 8v13H3V8M1 3h22v5H1zM10 12h4"/></svg>
            </div>
            <div class="ssb-body">
              <div class="ssb-title">{sess.title || 'Untitled chat'}</div>
              <div class="ssb-meta">
                <span>{sess.messages.length} msgs</span>
                {#if sess.archivedAt}
                  <span class="ssb-dot">·</span>
                  <span class="mono">archived {relativeTime(new Date(sess.archivedAt).toISOString(), now)}</span>
                {/if}
              </div>
            </div>
            <button
              class="ssb-arch-act"
              title="Restore chat"
              aria-label="Restore chat"
              onclick={(e) => restoreSession(sess.id, e)}
            >
              <svg viewBox="0 0 24 24" width="13" height="13" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M3 7v6h6M3 13a9 9 0 1 0 3-7.7L3 8"/></svg>
            </button>
            <button
              class="ssb-arch-act ssb-arch-act--danger"
              title="Delete forever"
              aria-label="Delete forever"
              onclick={(e) => purgeSession(sess.id, sess.title, e)}
            >
              <svg viewBox="0 0 24 24" width="13" height="13" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"><path d="M3 6h18M19 6l-2 14a2 2 0 0 1-2 2H9a2 2 0 0 1-2-2L5 6M10 11v6M14 11v6"/></svg>
            </button>
          </div>
        {/each}
      {/if}
    {/if}

  </div>
</aside>

<CardContextMenu coords={ctxCoords} items={ctxItems} onClose={closeSessCtx} />

<style>
  .ssb {
    display: flex; flex-direction: column;
    min-height: 0; min-width: 0;
    background: var(--bg-1);
    border-right: 1px solid var(--border-lo);
  }

  .ssb-head {
    display: flex; align-items: center; gap: 8px;
    padding: 14px 14px 10px;
    flex-shrink: 0;
  }
  .ssb-title { font-size: 13px; font-weight: 600; color: var(--text-0); }
  .ssb-count { font-size: 12px; color: var(--text-faint); }
  .ssb-head-spring { flex: 1; }
  .ssb-add {
    width: 26px; height: 26px; flex: none;
    display: grid; place-items: center;
    border: 0; border-radius: 8px;
    background: var(--accent); color: var(--accent-fg);
    cursor: pointer;
    box-shadow: 1.5px 1.5px 0 rgba(var(--ink-shadow), 0.22), 3px 3px 0 rgba(var(--ink-shadow), 0.12);
  }
  .ssb-add:hover { filter: brightness(1.05); }
  .ssb-add svg { width: 14px; height: 14px; }

  .ssb-search {
    margin: 0 14px 8px;
    height: 30px; padding: 0 10px;
    border: 1px solid var(--border); border-radius: 8px;
    background: var(--bg-0);
    color: var(--text-1); font-size: 12px;
    font-family: var(--font-ui);
    outline: none;
  }
  :global(:root[data-theme='light']) .ssb-search { background: var(--bg-2); }
  .ssb-search::placeholder { color: var(--text-faint); }

  .ssb-list {
    flex: 1; overflow-y: auto;
    padding: 0 10px 12px;
    scrollbar-width: none;
  }
  .ssb-list::-webkit-scrollbar { display: none; }

  .ssb-row {
    display: block;
    width: 100%;
    padding: 9px 10px;
    border-radius: 8px;
    margin-bottom: 2px;
    background: transparent;
    cursor: pointer;
    text-align: left;
    transition: background 120ms;
  }
  .ssb-row:hover { background: var(--bg-hover); }
  .ssb-row.active { background: var(--bg-nav); box-shadow: var(--shadow-1); }

  .ssb-row-top { display: flex; align-items: center; gap: 7px; }
  .ssb-status {
    width: 6px; height: 6px; border-radius: 50%;
    flex: none;
    background: var(--text-linenum);
  }
  .ssb-status.pulse {
    background: var(--ok);
    animation: ssb-pulsedot 1.6s infinite;
  }
  /* Row title uses Geist (var(--font-ui) via body); metrics stay mono. */
  .ssb-row .ssb-title {
    font-size: 13px; font-weight: 400; color: var(--text-1);
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
    min-width: 0; flex: 1;
  }
  .ssb-row .ssb-title.bold { font-weight: 600; color: var(--text-0); }
  .ssb-meta {
    font-size: 11px; color: var(--text-faint);
    font-family: var(--font-mono);
    margin-top: 3px;
    padding-left: 13px;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .ssb-mem { color: var(--text-faint); display: inline-flex; flex: none; }

  .ssb-del {
    display: none;
    width: 18px; height: 18px;
    place-items: center;
    border: 0; border-radius: 5px;
    background: transparent;
    color: var(--text-faint);
    cursor: pointer;
    flex: none;
  }
  .ssb-del svg { width: 12px; height: 12px; }
  .ssb-row:hover .ssb-del { display: grid; }
  .ssb-del:hover { color: var(--err); background: var(--bg-3); }

  .ssb-empty { padding: 22px 14px; }
  .ssb-empty-h { font-size: 14px; color: var(--text-0); margin: 0 0 8px; }
  .ssb-empty-p { font-size: 11.5px; color: var(--text-mute); line-height: 1.55; margin: 0; }

  .ssb-arch-toggle {
    display: flex; align-items: center; gap: 6px;
    width: 100%;
    margin-top: 12px;
    padding: 6px 10px;
    border: 0; border-radius: var(--r-item);
    background: transparent;
    font-size: 10px; font-weight: 600;
    letter-spacing: 0.10em; text-transform: uppercase;
    color: var(--text-faint);
    cursor: pointer;
  }
  .ssb-arch-toggle:hover { color: var(--text-1); background: var(--bg-hover); }
  .ssb-arch-caret { transition: transform 140ms; }
  .ssb-arch-caret.open { transform: rotate(90deg); }
  .ssb-arch-count { margin-left: auto; }

  /* Archived rows keep the icon + body + actions layout — the base
     .ssb-row went display:block in the paper redesign, which let the
     unsized archive SVG blow up to full row width. */
  .ssb-row--archived {
    opacity: 0.75;
    display: flex; align-items: center; gap: 8px;
  }
  .ssb-row--archived:hover { opacity: 1; }
  .ssb-icon {
    flex: none; display: grid; place-items: center;
    width: 16px; height: 16px;
    color: var(--text-faint);
  }
  .ssb-icon svg { width: 14px; height: 14px; }
  .ssb-body { flex: 1; min-width: 0; }
  .ssb-row--archived .ssb-meta { padding-left: 0; }
  .ssb-arch-act {
    display: grid; place-items: center;
    width: 20px; height: 20px;
    border: 0; border-radius: 5px;
    background: transparent;
    color: var(--text-faint);
    cursor: pointer;
    flex: none;
  }
  .ssb-arch-act:hover { color: var(--text-0); background: var(--bg-3); }
  .ssb-arch-act--danger:hover { color: var(--err); }

  @keyframes ssb-pulsedot {
    0%, 100% { opacity: 0.35; }
    50% { opacity: 1; }
  }
</style>
