<script lang="ts">
  /* SessionsSidebar — left pane of AgentApp.
     v7: serif "Claude" / "Cursor" head + "+" iconbtn, group labels
     (Today / Yesterday / This week / Older), session rows with sparkle
     icon + 13px title + meta line (time · N msgs · status), bottom
     dashed "+ New chat" button. */

  import {
    sessionsState,
    focusSession,
    setActiveSessionInInstance,
    newClaudeSession,
    deleteClaudeSession,
    updateSession
  } from '$lib/state/sessions.svelte';
  import { relativeTime } from '$lib/data';
  import BrandIcon from '$lib/components/ui/BrandIcon.svelte';
  import CardContextMenu, { type MenuItem } from '$lib/views/apps/_shared/CardContextMenu.svelte';
  import { notify } from '$lib/state/toaster.svelte';
  import { invoke } from '@tauri-apps/api/core';

  type Kind = 'claude' | 'cursor';

  interface Props {
    kind: Kind;
    /** App instance the active session is bound to (worktree ownership,
     *  MCP routing). App view receives this from +page.svelte. */
    instanceId: string;
    now: number;
  }

  let { kind, instanceId, now }: Props = $props();

  type Session = (typeof sessionsState.list)[number];

  const groups = $derived.by(() => {
    const items = sessionsState.list.filter((s) => s.agentKind === kind);
    const dayMs = 24 * 60 * 60 * 1000;
    const sessTime = (s: Session) => {
      const last = s.messages[s.messages.length - 1]?.at;
      return last ? new Date(last).getTime() : 0;
    };
    const sorted = [...items].sort((a, b) => sessTime(b) - sessTime(a));
    const today: Session[] = [];
    const yesterday: Session[] = [];
    const week: Session[] = [];
    const older: Session[] = [];
    for (const s of sorted) {
      const t = sessTime(s);
      if (t === 0) {
        older.push(s);
        continue;
      }
      const ageDays = Math.floor((now - t) / dayMs);
      if (ageDays < 1) today.push(s);
      else if (ageDays < 2) yesterday.push(s);
      else if (ageDays < 7) week.push(s);
      else older.push(s);
    }
    return [
      { label: 'Today', items: today },
      { label: 'Yesterday', items: yesterday },
      { label: 'Earlier this week', items: week },
      { label: 'Older', items: older }
    ].filter((g) => g.items.length > 0);
  });

  const totalCount = $derived(
    sessionsState.list.filter((s) => s.agentKind === kind).length
  );

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

  const label = $derived(kind === 'claude' ? 'Claude' : 'Cursor');

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
      label: 'Delete chat',
      icon: 'M3 6h18 M19 6l-2 14a2 2 0 0 1-2 2H9a2 2 0 0 1-2-2L5 6 M10 11v6 M14 11v6',
      danger: true,
      onClick: () => {
        if (window.confirm(`Delete "${s.title || 'Untitled'}"? Auto-distill saves a memory snapshot first.`)) {
          deleteClaudeSession(s.id);
        }
      }
    });
    return items;
  });

  function pickSession(sessId: string) {
    setActiveSessionInInstance(instanceId, sessId);
    focusSession(sessId);
  }

  function createNew() {
    newClaudeSession({ agentKind: kind, agentInstanceId: instanceId });
  }

  function deleteSession(sessId: string, sessTitle: string, e: MouseEvent) {
    /* Stop the delete-icon click from also bubbling to the row's
       click-to-activate handler. */
    e.stopPropagation();
    e.preventDefault();
    if (!confirm(`Delete chat "${sessTitle || 'Untitled chat'}"? This can't be undone.`)) {
      return;
    }
    deleteClaudeSession(sessId);
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

<aside class="ssb app-pane">
  <div class="ssb-head">
    <span class="ssb-logo" data-agent={kind} aria-hidden="true">
      <BrandIcon kind={kind} size={16} />
    </span>
    <h2 class="ssb-h">{label}</h2>
    <button class="ssb-headbtn" onclick={createNew} title="New chat (⌘N)">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"><path d="M12 5v14M5 12h14"/></svg>
    </button>
  </div>

  <div class="ssb-list">
    {#if groups.length === 0}
      <div class="ssb-empty">
        <p class="ssb-empty-h serif">No {label} sessions yet</p>
        <p class="ssb-empty-p">
          Click <strong>+ New chat</strong> to begin. Drop a Jira ticket,
          PR, or file onto the chat to attach context.
        </p>
      </div>
    {:else}
      {#each groups as g (g.label)}
        <div class="ssb-group-label">{g.label}</div>
        {#each g.items as sess (sess.id)}
          {@const isActive = sess.id === sessionsState.activeIds[kind]}
          {@const lastMsg = sess.messages[sess.messages.length - 1]}
          {@const lastAt = lastMsg?.at ?? null}
          {@const msgCount = sess.messages.length}
          <div
            class="ssb-row"
            class:active={isActive}
            role="button"
            tabindex="0"
            onclick={() => pickSession(sess.id)}
            onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') pickSession(sess.id); }}
            oncontextmenu={(e) => openSessCtx(e, sess)}
          >
            <div class="ssb-icon">
              <svg viewBox="0 0 24 24" fill="currentColor"><path d="M12 2 L14.5 9.5 L22 12 L14.5 14.5 L12 22 L9.5 14.5 L2 12 L9.5 9.5 Z"/></svg>
            </div>
            <div class="ssb-body">
              <div class="ssb-title">{sess.title || 'Untitled chat'}</div>
              <div class="ssb-meta">
                <span class="mono">{shortTime(lastAt ?? undefined) || relativeTime(lastAt ?? new Date().toISOString(), now)}</span>
                <span class="ssb-dot">·</span>
                <span>{msgCount} msgs</span>
                {#if memCountFor(sess.id) > 0}
                  <span class="ssb-dot">·</span>
                  <span class="ssb-mem mono" title="{memCountFor(sess.id)} long-term memories saved from this chat">
                    <svg viewBox="0 0 24 24" width="10" height="10" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                      <path d="M19 21l-7-5-7 5V5a2 2 0 0 1 2-2h10a2 2 0 0 1 2 2z"/>
                    </svg>
                    <span>{memCountFor(sess.id)}</span>
                  </span>
                {/if}
                {#if sess.sending}
                  <span class="ssb-dot">·</span>
                  <span class="ssb-running">◷ thinking</span>
                {:else if sess.worktreeBranch}
                  <span class="ssb-dot">·</span>
                  <span class="ssb-link">☘ Editor</span>
                {/if}
              </div>
            </div>
            <button
              class="ssb-del"
              title="Delete chat"
              aria-label="Delete chat"
              onclick={(e) => deleteSession(sess.id, sess.title, e)}
            >
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"><path d="M18 6 6 18M6 6l12 12"/></svg>
            </button>
          </div>
        {/each}
      {/each}
    {/if}

    <button class="ssb-new" onclick={createNew}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"><path d="M12 5v14M5 12h14"/></svg>
      New chat
    </button>
  </div>

  <div class="ssb-foot mono" title="Total {label} sessions">
    <span class="ssb-foot-pip"></span>
    <span>{totalCount} sessions</span>
  </div>
</aside>

<CardContextMenu coords={ctxCoords} items={ctxItems} onClose={closeSessCtx} />

<style>
  .ssb {
    display: flex; flex-direction: column;
    min-height: 0; min-width: 0;
  }

  .ssb-head {
    display: flex; align-items: center;
    padding: 16px 18px 12px;
    border-bottom: 1px solid var(--border);
    flex-shrink: 0;
    gap: 8px;
  }
  /* Agent logo chip — Claude burst or Cursor hex in the agent's
     ACTUAL brand color (coral for Claude, neutral grey for Cursor),
     not the app shell's accent. Brand identity stays per-source even
     when the surrounding app paints in mint/sage. */
  .ssb-logo {
    width: 26px; height: 26px;
    display: inline-flex; align-items: center; justify-content: center;
    border-radius: 7px;
    flex-shrink: 0;
    line-height: 0;
  }
  .ssb-logo[data-agent="claude"] {
    color: var(--src-claude);
    background: color-mix(in srgb, var(--src-claude) 12%, var(--bg-2));
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--src-claude) 28%, transparent);
  }
  .ssb-logo[data-agent="cursor"] {
    color: var(--src-cursor);
    background: color-mix(in srgb, var(--src-cursor) 12%, var(--bg-2));
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--src-cursor) 28%, transparent);
  }
  /* BrandIcon renders the SVG / IMG with its own width/height
     attributes, so we just keep the centering rhythm and let the
     glyph honour its intrinsic size. */
  .ssb-h {
    font-family: 'Geist', 'Inter', -apple-system, system-ui, sans-serif;
    font-size: 18px; font-weight: 600;
    flex: 1;
    letter-spacing: -0.01em;
    color: var(--text-0);
    margin: 0;
  }
  .ssb-headbtn {
    width: 28px; height: 28px;
    display: grid; place-items: center;
    border-radius: 7px;
    background: transparent;
    border: 0;
    color: var(--text-2);
    cursor: pointer;
    transition: background 120ms;
  }
  .ssb-headbtn:hover { background: var(--bg-3); color: var(--text-0); }
  .ssb-headbtn svg { width: 14px; height: 14px; }

  .ssb-list {
    flex: 1; overflow-y: auto;
    padding: 8px 8px 12px;
  }

  .ssb-group-label {
    padding: 14px 10px 8px;
    font-size: 9.5px; font-weight: 700;
    letter-spacing: 0.10em;
    text-transform: uppercase;
    color: var(--text-mute);
  }

  .ssb-row {
    display: flex; align-items: flex-start; gap: 10px;
    padding: 10px 11px;
    border-radius: 9px;
    margin-bottom: 2px;
    position: relative;
    transition: background 120ms, border-color 120ms;
    border: 1px solid transparent;
    width: 100%;
    text-align: left;
    background: transparent;
    cursor: pointer;
  }
  .ssb-row::before {
    content: '';
    position: absolute;
    left: 4px; top: 12px; bottom: 12px;
    width: 2px;
    border-radius: 2px;
    background: color-mix(in srgb, var(--app-tone, var(--src-claude)) 40%, transparent);
    opacity: 0.5;
    transition: opacity 200ms;
  }
  .ssb-row:hover { background: var(--bg-2); }
  .ssb-row.active {
    background: var(--bg-2);
    border-color: var(--border-hi);
  }
  .ssb-row.active::before {
    background: var(--app-tone, var(--src-claude));
    opacity: 1;
    box-shadow: 0 0 8px var(--app-tone, var(--src-claude));
  }

  .ssb-icon {
    width: 22px; height: 22px;
    display: grid; place-items: center;
    border-radius: 6px;
    background: var(--bg-3);
    color: var(--app-tone, var(--src-claude));
    flex-shrink: 0;
    margin-left: 4px;
    margin-top: 2px;
  }
  .ssb-icon svg { width: 12px; height: 12px; fill: currentColor; }

  .ssb-body { flex: 1; min-width: 0; }
  .ssb-title {
    font-size: 13px; font-weight: 500;
    color: var(--text-0);
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .ssb-meta {
    display: flex; align-items: center; gap: 6px;
    margin-top: 3px;
    font-size: 10.5px;
    color: var(--text-mute);
  }
  .ssb-meta .mono { font-size: 10px; }
  .ssb-dot { opacity: 0.6; }
  .ssb-running { color: var(--app-tone, var(--accent-bright)); }
  .ssb-link { color: var(--src-editor); }
  /* Memory-presence badge — small inline pill with the bookmark
     glyph + count. Mute tone so it doesn't compete with the running
     /linked indicators next to it. */
  .ssb-mem {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    color: var(--text-mute);
  }
  .ssb-mem svg { flex-shrink: 0; opacity: 0.85; }

  /* Delete-X — sits on the right of the row, fades in on hover.
     Hover state turns it red so the user feels the destructive
     intent before clicking. */
  .ssb-del {
    flex-shrink: 0;
    width: 22px; height: 22px;
    display: grid; place-items: center;
    border-radius: 5px;
    background: transparent;
    border: 0;
    color: var(--text-mute);
    cursor: pointer;
    opacity: 0;
    margin-left: 2px;
    transition: opacity 100ms, background 100ms, color 100ms;
  }
  .ssb-del svg { width: 12px; height: 12px; }
  .ssb-row:hover .ssb-del,
  .ssb-row:focus-within .ssb-del { opacity: 0.85; }
  .ssb-del:hover {
    opacity: 1;
    color: var(--error);
    background: rgba(232, 130, 100, 0.10);
  }

  .ssb-new {
    margin: 8px 4px;
    padding: 11px;
    display: flex; align-items: center; justify-content: center;
    gap: 8px;
    border: 1px dashed var(--border-neutral-hi);
    border-radius: 9px;
    color: var(--text-2);
    font-size: 12.5px; font-weight: 500;
    background: transparent;
    cursor: pointer;
    width: calc(100% - 8px);
    transition: all 140ms;
  }
  .ssb-new svg { width: 13px; height: 13px; }
  .ssb-new:hover {
    color: var(--accent-bright);
    border-color: var(--border-accent);
    background: var(--accent-soft);
  }

  .ssb-foot {
    flex: 0 0 auto;
    display: flex; align-items: center; gap: 8px;
    padding: 8px 16px;
    border-top: 1px solid var(--border);
    font-size: 10px; color: var(--text-mute);
  }
  .ssb-foot-pip {
    width: 5px; height: 5px; border-radius: 50%;
    background: var(--success);
    box-shadow: 0 0 5px var(--success);
  }

  .ssb-empty {
    text-align: center;
    padding: 30px 18px;
  }
  .ssb-empty-h {
    font-family: 'Geist', 'Inter', -apple-system, system-ui, sans-serif;
    font-size: 20px; font-weight: 600; letter-spacing: -0.01em;
    color: var(--text-0);
    margin: 0 0 8px;
  }
  .ssb-empty-p {
    font-size: 12px; color: var(--text-2);
    line-height: 1.5; margin: 0;
  }
</style>
