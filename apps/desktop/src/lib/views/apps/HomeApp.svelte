<script lang="ts">
  /* HomeApp — workspace dashboard. Click the W in the rail to land
     here; the page stitches together a snapshot of every other app
     so the user can see "what's going on" at a glance and jump into
     any thread of work without first picking the right rail icon.

     Visual language (v2):
       • Aurora hero. The page header sits on a soft mesh gradient
         tinted by the live theme accent (mint/sage). It anchors the
         eye and sets a warmer, less-spreadsheet mood than the v1
         dashboard had.
       • Productivity ribbon. Single sentence under the greeting —
         "3 chats · 12 messages today · 1 worktree active" — gives
         the user a real-time pulse without making them parse cards.
       • Brand-tinted cards. Stats and source-digests use accent-color
         gradients on their tiles instead of the prior flat
         var(--bg-1). The colours match the rail icons, so scanning
         the home page primes the user for the rail layout.
       • 24-hour activity bar chart. Plots messages-sent per hour
         across the last day; gives a sense of when the user has
         actually worked, not just "what's pending".

     Everything reads off live state (`sessionsState`, `inboxState`,
     `layoutState`). Empty states are first-class — when no data is
     present the cards still render with onboarding copy so a fresh
     install lands somewhere useful instead of a blank page. */

  import { sessionsState } from '$lib/state/sessions.svelte';
  import { inboxState } from '$lib/state/inbox.svelte';
  import { APP_INSTANCE_IDS, layoutState, kindForInstanceId } from '$lib/state/layout.svelte';
  import { terminalsState } from '$lib/state/terminals.svelte';
  import type { View } from '$lib/state/view.svelte';
  import Sigil from '$lib/components/ui/Sigil.svelte';
  import BrandIcon from '$lib/components/ui/BrandIcon.svelte';

  interface Props {
    now: number;
    onNavigate: (v: View) => void;
    onOpenSession: (sessionId: string, agentInstanceId: string) => void;
    onNewChat: (kind: 'claude' | 'cursor') => void;
    /** Opens the long-form Welcome / Help overlay. Surfaced as a
     *  small "Take the tour" pill in the hero so first-time users
     *  have a discoverable entry point that doesn't require knowing
     *  the ⇧⌘? shortcut up front. Optional — when omitted, the
     *  pill is simply not rendered. */
    onOpenWelcome?: () => void;
  }
  let p: Props = $props();

  /* ---------- Greeting + date ---------- */

  const greeting = $derived.by(() => {
    const h = new Date(p.now).getHours();
    if (h < 5) return 'Working late';
    if (h < 12) return 'Good morning';
    if (h < 18) return 'Good afternoon';
    return 'Good evening';
  });

  const todayLabel = $derived.by(() => {
    const d = new Date(p.now);
    /* Locked to en-US so the dashboard reads the same regardless of
       the user's OS locale. The rest of the app is English-only, so
       a Russian / German weekday name here would feel inconsistent. */
    return d.toLocaleDateString('en-US', {
      weekday: 'long',
      month: 'long',
      day: 'numeric'
    });
  });

  /* ---------- Sessions roll-up ---------- */

  type ChatRow = {
    id: string;
    title: string;
    kind: 'claude' | 'cursor';
    agentInstanceId: string;
    sending: boolean;
    queueLen: number;
    lastAt: string | null;
    lastSnippet: string | null;
    msgCount: number;
    msgsToday: number;
  };

  const recentChats = $derived.by<ChatRow[]>(() => {
    const out: ChatRow[] = [];
    const dayStart = p.now - 24 * 60 * 60 * 1000;
    for (const s of sessionsState.list) {
      if (s.agentKind !== 'claude' && s.agentKind !== 'cursor') continue;
      const agentInstanceId = s.agentInstanceId ?? APP_INSTANCE_IDS[s.agentKind];
      if (!kindForInstanceId(agentInstanceId)) continue;
      const last = s.messages[s.messages.length - 1];
      const snippet = (() => {
        if (!last) return null;
        const c = (last as { content?: unknown }).content;
        if (typeof c !== 'string') return null;
        const trimmed = c.trim();
        if (!trimmed) return null;
        return last.role === 'user'
          ? `You: ${trimmed.slice(0, 110)}`
          : trimmed.slice(0, 120);
      })();
      let msgsToday = 0;
      for (const m of s.messages) {
        const t = new Date(m.at ?? '').getTime();
        if (Number.isFinite(t) && t >= dayStart) msgsToday += 1;
      }
      out.push({
        id: s.id,
        title: s.title || 'Untitled chat',
        kind: s.agentKind,
        agentInstanceId,
        sending: s.sending,
        queueLen: s.pendingQueue?.length ?? 0,
        lastAt: last?.at ?? null,
        lastSnippet: snippet,
        msgCount: s.messages.length,
        msgsToday
      });
    }
    out.sort((a, b) => (b.lastAt ?? '').localeCompare(a.lastAt ?? ''));
    return out.slice(0, 6);
  });

  /* ---------- Aggregated stats ---------- */

  const stats = $derived.by(() => {
    let runningChats = 0;
    let totalChats = 0;
    let messagesToday = 0;
    const dayStart = p.now - 24 * 60 * 60 * 1000;
    /* 24 hourly buckets ending at `now`; index 0 = oldest hour. */
    const hourlyBuckets = new Array<number>(24).fill(0);
    for (const s of sessionsState.list) {
      if (s.agentKind !== 'claude' && s.agentKind !== 'cursor') continue;
      totalChats += 1;
      if (s.sending) runningChats += 1;
      for (const m of s.messages) {
        const t = new Date(m.at ?? '').getTime();
        if (!Number.isFinite(t)) continue;
        if (t < dayStart || t > p.now) continue;
        messagesToday += 1;
        const bucket = Math.min(
          23,
          Math.max(0, 23 - Math.floor((p.now - t) / (60 * 60 * 1000)))
        );
        hourlyBuckets[bucket] += 1;
      }
    }
    let openPRs = 0;
    for (const items of Object.values(inboxState.itemsByInstance)) {
      for (const it of items) {
        if (it.is_pull_request && it.state === 'open') openPRs += 1;
      }
    }
    let openTickets = 0;
    for (const items of Object.values(inboxState.jiraItemsByInstance)) {
      for (const it of items) {
        if (it.status_category !== 'done') openTickets += 1;
      }
    }
    let unresolvedSentry = 0;
    for (const items of Object.values(inboxState.sentryItemsByInstance)) {
      for (const it of items) {
        if (it.status === 'unresolved') unresolvedSentry += 1;
      }
    }
    return {
      runningChats,
      totalChats,
      messagesToday,
      hourlyBuckets,
      openPRs,
      openTickets,
      unresolvedSentry
    };
  });

  /** One-line "what's happening right now" string. Listed in priority
   *  order so the most-actionable signal lands first; falls back to a
   *  warm onboarding line when nothing is going on yet. */
  const pulse = $derived.by<string>(() => {
    const parts: string[] = [];
    if (stats.runningChats > 0) {
      parts.push(`${stats.runningChats} chat${stats.runningChats === 1 ? '' : 's'} running`);
    }
    if (stats.messagesToday > 0) {
      parts.push(`${stats.messagesToday} message${stats.messagesToday === 1 ? '' : 's'} today`);
    }
    const liveTerminals = Object.values(terminalsState.sessions).filter((t) => !t.exited).length;
    if (liveTerminals > 0) {
      parts.push(`${liveTerminals} terminal${liveTerminals === 1 ? '' : 's'} live`);
    }
    if (stats.openPRs > 0) {
      parts.push(`${stats.openPRs} PR${stats.openPRs === 1 ? '' : 's'} open`);
    }
    if (stats.openTickets > 0) {
      parts.push(`${stats.openTickets} ticket${stats.openTickets === 1 ? '' : 's'} in flight`);
    }
    if (parts.length === 0) {
      return stats.totalChats === 0
        ? 'Pick a chat below to get rolling.'
        : 'All quiet. Pick up where you left off below.';
    }
    return parts.join(' · ');
  });

  /** Peak hour in the 24h hourly bucket — used as a chip under the
   *  bar chart so the visualisation has a single data takeaway, not
   *  just a shape. */
  const peakHour = $derived.by(() => {
    let max = 0;
    let idx = -1;
    for (let i = 0; i < stats.hourlyBuckets.length; i++) {
      if (stats.hourlyBuckets[i] > max) {
        max = stats.hourlyBuckets[i];
        idx = i;
      }
    }
    if (idx === -1 || max === 0) return null;
    /* Convert bucket index back to clock hour. Bucket 23 = current
       hour (now), bucket 0 = 23 hours ago. */
    const hourOffset = 23 - idx;
    const peak = new Date(p.now - hourOffset * 60 * 60 * 1000);
    const hh = peak.getHours();
    const ampm = hh < 12 ? 'am' : 'pm';
    const display = ((hh + 11) % 12) + 1;
    return { label: `${display}${ampm}`, count: max };
  });

  /* ---------- Inbox digest ---------- */

  const topPRs = $derived.by(() => {
    const flat = Object.values(inboxState.itemsByInstance).flat();
    const open = flat.filter((it) => it.state === 'open' && it.is_pull_request);
    open.sort((a, b) => b.updated_at.localeCompare(a.updated_at));
    return open.slice(0, 4);
  });

  const topTickets = $derived.by(() => {
    const flat = Object.values(inboxState.jiraItemsByInstance).flat();
    const open = flat.filter((it) => it.status_category !== 'done');
    open.sort((a, b) => b.updated.localeCompare(a.updated));
    return open.slice(0, 4);
  });

  const topErrors = $derived.by(() => {
    const flat = Object.values(inboxState.sentryItemsByInstance).flat();
    const open = flat.filter((it) => it.status === 'unresolved');
    open.sort((a, b) => b.last_seen.localeCompare(a.last_seen));
    return open.slice(0, 4);
  });

  /* ---------- Helpers ---------- */

  function relativeTime(iso: string | null): string {
    if (!iso) return '';
    const t = new Date(iso).getTime();
    if (!Number.isFinite(t)) return '';
    const dMs = p.now - t;
    if (dMs < 60_000) return 'just now';
    if (dMs < 3_600_000) return `${Math.floor(dMs / 60_000)}m`;
    if (dMs < 86_400_000) return `${Math.floor(dMs / 3_600_000)}h`;
    if (dMs < 7 * 86_400_000) return `${Math.floor(dMs / 86_400_000)}d`;
    return `${Math.floor(dMs / (7 * 86_400_000))}w`;
  }

  function openChat(c: ChatRow) {
    p.onOpenSession(c.id, c.agentInstanceId);
  }

  const terminalCount = $derived(layoutState.instances.terminal.length);
  const editorCount = $derived(layoutState.instances.editor.length);

  /** Bar height for the activity chart. Caller hands a count, the
   *  fn maps it to a 4-32px range so empty hours still get a 2-3px
   *  baseline (otherwise zero-runs read as missing data). */
  function barHeight(count: number, max: number): number {
    if (max === 0) return 4;
    const ratio = count / max;
    return Math.max(4, Math.round(4 + ratio * 28));
  }

  const hourlyMax = $derived(Math.max(...stats.hourlyBuckets, 0));
</script>

<section class="ho">
  <!-- Aurora background — single absolutely-positioned div so the
       gradient lives behind the content without affecting flow.
       Subtle radial wash anchored top-left where the hero sits, plus
       a counter-wash bottom-right to keep the page from reading flat. -->
  <div class="ho-aurora" aria-hidden="true"></div>

  <div class="ho-scroll">
    <!-- HERO ----------------------------------------------------- -->
    <header class="ho-hero">
      <div class="ho-hero-left">
        <div class="ho-hero-mark" aria-hidden="true">
          <Sigil size={56} />
        </div>
        <div class="ho-hero-text">
          <h1 class="ho-hero-h">{greeting}.</h1>
          <p class="ho-hero-pulse">{pulse}</p>
          <div class="ho-hero-meta">
            <span class="ho-hero-date mono">{todayLabel}</span>
            {#if p.onOpenWelcome}
              <button class="ho-hero-tour" onclick={p.onOpenWelcome} type="button">
                <span aria-hidden="true">✦</span>
                Take the tour
                <span class="ho-hero-tour-kbd mono">⇧⌘?</span>
              </button>
            {/if}
          </div>
        </div>
      </div>

      <!-- Right-side panel: 24h activity histogram. Bars are per-hour
           message counts; the peak chip below summarises the chart in
           one line so users get something even if they don't read the
           bars. Empty state gets a friendly idle copy. -->
      <div class="ho-spark">
        <div class="ho-spark-head">
          <span class="ho-spark-label mono">Last 24h</span>
          {#if peakHour}
            <span class="ho-spark-meta mono">peak {peakHour.label} · {peakHour.count} msg</span>
          {:else}
            <span class="ho-spark-meta mono">no activity</span>
          {/if}
        </div>
        <div class="ho-spark-bars">
          {#each stats.hourlyBuckets as count, i}
            <span
              class="ho-spark-bar"
              class:ho-spark-bar--zero={count === 0}
              class:ho-spark-bar--peak={count > 0 && count === hourlyMax && hourlyMax > 0}
              style:height="{barHeight(count, hourlyMax)}px"
              title={count === 0 ? '—' : `${count} message${count === 1 ? '' : 's'}`}
            ></span>
          {/each}
        </div>
        <div class="ho-spark-axis mono">
          <span>24h ago</span>
          <span>now</span>
        </div>
      </div>
    </header>

    <!-- STAT STRIP --------------------------------------------- -->
    <div class="ho-stats">
      <button class="ho-stat" data-tone="agent" onclick={() => p.onNavigate('claudeApp')}>
        <span class="ho-stat-glow" aria-hidden="true"></span>
        <span class="ho-stat-icon" data-tone="agent">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"><path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/></svg>
        </span>
        <span class="ho-stat-label mono">Active chats</span>
        <span class="ho-stat-row">
          <span class="ho-stat-value">{stats.runningChats}</span>
          <span class="ho-stat-of">/ {stats.totalChats}</span>
        </span>
        <span class="ho-stat-sub">
          {stats.runningChats > 0
            ? `${stats.runningChats} running now`
            : stats.totalChats > 0
            ? 'all idle · pick one'
            : 'no chats yet'}
        </span>
      </button>

      <button class="ho-stat" data-tone="github" onclick={() => p.onNavigate('githubApp')}>
        <span class="ho-stat-glow" aria-hidden="true"></span>
        <span class="ho-stat-icon" data-tone="github">
          <BrandIcon kind="github" size={14} />
        </span>
        <span class="ho-stat-label mono">Open PRs</span>
        <span class="ho-stat-row">
          <span class="ho-stat-value">{stats.openPRs}</span>
        </span>
        <span class="ho-stat-sub">{stats.openPRs === 0 ? 'inbox clean' : 'GitHub'}</span>
      </button>

      <button class="ho-stat" data-tone="jira" onclick={() => p.onNavigate('jiraApp')}>
        <span class="ho-stat-glow" aria-hidden="true"></span>
        <span class="ho-stat-icon" data-tone="jira">
          <BrandIcon kind="jira" size={14} />
        </span>
        <span class="ho-stat-label mono">In flight</span>
        <span class="ho-stat-row">
          <span class="ho-stat-value">{stats.openTickets}</span>
        </span>
        <span class="ho-stat-sub">{stats.openTickets === 0 ? 'no tickets' : 'Jira'}</span>
      </button>

      <button class="ho-stat" data-tone="sentry" onclick={() => p.onNavigate('sentryApp')}>
        <span class="ho-stat-glow" aria-hidden="true"></span>
        <span class="ho-stat-icon" data-tone="sentry">
          <BrandIcon kind="sentry" size={14} />
        </span>
        <span class="ho-stat-label mono">Unresolved</span>
        <span class="ho-stat-row">
          <span class="ho-stat-value">{stats.unresolvedSentry}</span>
        </span>
        <span class="ho-stat-sub">{stats.unresolvedSentry === 0 ? 'no errors' : 'Sentry'}</span>
      </button>
    </div>

    <!-- TWO-UP GRID ----------------------------------------------- -->
    <div class="ho-grid">
      <!-- Active conversations -->
      <section class="ho-card ho-card--chats">
        <header class="ho-card-head">
          <h2 class="ho-card-h">Active conversations</h2>
          <button class="ho-card-link mono" onclick={() => p.onNavigate('claudeApp')}>
            Open agent app →
          </button>
        </header>

        {#if recentChats.length === 0}
          <div class="ho-empty">
            <p class="ho-empty-h">No chats yet</p>
            <p class="ho-empty-p">
              Start a Claude or Cursor session to bring agents into your workflow.
            </p>
            <div class="ho-empty-row">
              <button class="ho-cta ho-cta--claude" onclick={() => p.onNewChat('claude')}>
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M12 5v14M5 12h14"/></svg>
                New Claude chat
              </button>
              <button class="ho-cta ho-cta--cursor" onclick={() => p.onNewChat('cursor')}>
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M12 5v14M5 12h14"/></svg>
                New Cursor chat
              </button>
            </div>
          </div>
        {:else}
          <ul class="ho-chat-list">
            {#each recentChats as c (c.id)}
              <li>
                <button class="ho-chat-row" data-agent={c.kind} onclick={() => openChat(c)}>
                  <span class="ho-chat-icon" data-agent={c.kind}>
                    <BrandIcon kind={c.kind} size={18} />
                  </span>
                  <span class="ho-chat-body">
                    <span class="ho-chat-title">{c.title}</span>
                    {#if c.lastSnippet}
                      <span class="ho-chat-snippet">{c.lastSnippet}</span>
                    {:else}
                      <span class="ho-chat-snippet ho-chat-snippet--empty">No messages yet</span>
                    {/if}
                  </span>
                  <span class="ho-chat-meta">
                    {#if c.sending}
                      <span class="ho-chip ho-chip--running" data-agent={c.kind}>
                        <span class="ho-chip-dot"></span>running
                      </span>
                    {:else if c.queueLen > 0}
                      <span class="ho-chip ho-chip--queued">queued · {c.queueLen}</span>
                    {:else if c.msgsToday > 0}
                      <span class="ho-chip-time mono">{c.msgsToday} today</span>
                    {:else if c.lastAt}
                      <span class="ho-chip-time mono">{relativeTime(c.lastAt)}</span>
                    {:else}
                      <span class="ho-chip-time mono">idle</span>
                    {/if}
                  </span>
                </button>
              </li>
            {/each}
          </ul>
        {/if}
      </section>

      <!-- Inbox digest with brand-tinted block headers. -->
      <section class="ho-card ho-card--inbox">
        <header class="ho-card-head">
          <h2 class="ho-card-h">Inbox</h2>
          <span class="ho-card-sub mono">snapshot</span>
        </header>

        <div class="ho-source" data-source="github">
          <div class="ho-source-head">
            <span class="ho-source-tag mono" data-source="github">GitHub</span>
            <span class="ho-source-count mono">
              {#if stats.openPRs > 0}{stats.openPRs} open{:else}clean{/if}
            </span>
            <button class="ho-source-link mono" onclick={() => p.onNavigate('githubApp')}>Open →</button>
          </div>
          {#if topPRs.length === 0}
            <p class="ho-source-empty">No open PRs.</p>
          {:else}
            <ul class="ho-source-list">
              {#each topPRs as pr (pr.id)}
                <li>
                  <button class="ho-source-row" onclick={() => p.onNavigate('githubApp')}>
                    <span class="ho-source-num mono" data-source="github">#{pr.number}</span>
                    <span class="ho-source-title">{pr.title}</span>
                    <span class="ho-source-when mono">{relativeTime(pr.updated_at)}</span>
                  </button>
                </li>
              {/each}
            </ul>
          {/if}
        </div>

        <div class="ho-source" data-source="jira">
          <div class="ho-source-head">
            <span class="ho-source-tag mono" data-source="jira">Jira</span>
            <span class="ho-source-count mono">
              {#if stats.openTickets > 0}{stats.openTickets} in flight{:else}empty{/if}
            </span>
            <button class="ho-source-link mono" onclick={() => p.onNavigate('jiraApp')}>Open →</button>
          </div>
          {#if topTickets.length === 0}
            <p class="ho-source-empty">No tickets in flight.</p>
          {:else}
            <ul class="ho-source-list">
              {#each topTickets as t (t.id)}
                <li>
                  <button class="ho-source-row" onclick={() => p.onNavigate('jiraApp')}>
                    <span class="ho-source-num mono" data-source="jira">{t.key}</span>
                    <span class="ho-source-title">{t.summary}</span>
                    <span class="ho-source-when mono">{relativeTime(t.updated)}</span>
                  </button>
                </li>
              {/each}
            </ul>
          {/if}
        </div>

        <div class="ho-source" data-source="sentry">
          <div class="ho-source-head">
            <span class="ho-source-tag mono" data-source="sentry">Sentry</span>
            <span class="ho-source-count mono">
              {#if stats.unresolvedSentry > 0}{stats.unresolvedSentry} unresolved{:else}quiet{/if}
            </span>
            <button class="ho-source-link mono" onclick={() => p.onNavigate('sentryApp')}>Open →</button>
          </div>
          {#if topErrors.length === 0}
            <p class="ho-source-empty">No unresolved errors.</p>
          {:else}
            <ul class="ho-source-list">
              {#each topErrors as e (e.id)}
                <li>
                  <button class="ho-source-row" onclick={() => p.onNavigate('sentryApp')}>
                    <span class="ho-source-num mono" data-source="sentry">{e.short_id}</span>
                    <span class="ho-source-title">{e.title}</span>
                    <span class="ho-source-when mono">{relativeTime(e.last_seen)}</span>
                  </button>
                </li>
              {/each}
            </ul>
          {/if}
        </div>
      </section>
    </div>

    <!-- QUICK ACTIONS -------------------------------------------- -->
    <section class="ho-quick">
      <h2 class="ho-quick-h">Jump in</h2>
      <div class="ho-quick-grid">
        <button class="ho-quick-card" data-agent="claude" onclick={() => p.onNewChat('claude')}>
          <span class="ho-quick-icon" data-agent="claude">
            <BrandIcon kind="claude" size={20} />
          </span>
          <span class="ho-quick-body">
            <span class="ho-quick-title">New Claude chat</span>
            <span class="ho-quick-sub">spawn a fresh agent session</span>
          </span>
        </button>

        <button class="ho-quick-card" data-agent="cursor" onclick={() => p.onNewChat('cursor')}>
          <span class="ho-quick-icon" data-agent="cursor">
            <BrandIcon kind="cursor" size={20} />
          </span>
          <span class="ho-quick-body">
            <span class="ho-quick-title">New Cursor chat</span>
            <span class="ho-quick-sub">cursor-agent CLI session</span>
          </span>
        </button>

        <!-- Editor / Terminal / Canvas glyphs lifted verbatim from the
             rail (Rail.svelte) so the home tile and the rail icon
             match — the user shouldn't have to re-learn which icon
             means which app every time they switch context. -->
        <button class="ho-quick-card" onclick={() => p.onNavigate('editorApp')}>
          <span class="ho-quick-icon ho-quick-icon--mint">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="18" height="18" rx="2"/><line x1="3" y1="9" x2="21" y2="9"/><line x1="9" y1="9" x2="9" y2="21"/></svg>
          </span>
          <span class="ho-quick-body">
            <span class="ho-quick-title">Open Editor</span>
            <span class="ho-quick-sub">{editorCount === 0 ? 'pick a folder' : `${editorCount} open`}</span>
          </span>
        </button>

        <button class="ho-quick-card" onclick={() => p.onNavigate('terminalApp')}>
          <span class="ho-quick-icon ho-quick-icon--mint">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><polyline points="4 17 10 11 4 5"/><line x1="12" y1="19" x2="20" y2="19"/></svg>
          </span>
          <span class="ho-quick-body">
            <span class="ho-quick-title">Open Terminal</span>
            <span class="ho-quick-sub">{terminalCount === 0 ? 'no shell yet' : `${terminalCount} running`}</span>
          </span>
        </button>

        <button class="ho-quick-card" onclick={() => p.onNavigate('canvasApp')}>
          <span class="ho-quick-icon ho-quick-icon--mint">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="18" height="14" rx="2"/><rect x="6" y="6" width="9" height="6" rx="1"/><rect x="13" y="13" width="5" height="3" rx="0.5"/></svg>
          </span>
          <span class="ho-quick-body">
            <span class="ho-quick-title">Open Canvas</span>
            <span class="ho-quick-sub">notes, diagrams, scratchpad</span>
          </span>
        </button>

        <button class="ho-quick-card" onclick={() => p.onNavigate('connections')}>
          <span class="ho-quick-icon ho-quick-icon--mint">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"><path d="M9 17H7A5 5 0 1 1 7 7h2M15 7h2a5 5 0 1 1 0 10h-2M8 12h8"/></svg>
          </span>
          <span class="ho-quick-body">
            <span class="ho-quick-title">Connections</span>
            <span class="ho-quick-sub">link Jira / GitHub / Sentry</span>
          </span>
        </button>
      </div>
    </section>
  </div>
</section>

<style>
  .ho {
    position: relative;
    height: 100%;
    min-height: 0;
    background: var(--bg-0);
    display: flex; flex-direction: column;
    overflow: hidden;
  }
  /* Aurora wash. Two large radial gradients fade off in soft mint
     /sage so the page reads as ambient instead of flat dark. The
     positioned div sits behind the scroll content (z-index: 0) and
     itself isn't interactive. */
  .ho-aurora {
    position: absolute; inset: 0;
    pointer-events: none;
    background:
      radial-gradient(70% 60% at 12% -10%, color-mix(in srgb, var(--accent) 22%, transparent), transparent 65%),
      radial-gradient(60% 50% at 110% 110%, color-mix(in srgb, var(--src-claude) 14%, transparent), transparent 70%),
      radial-gradient(40% 40% at 50% 35%, color-mix(in srgb, var(--accent) 8%, transparent), transparent 70%);
    z-index: 0;
  }
  .ho-scroll {
    position: relative; z-index: 1;
    flex: 1;
    overflow-y: auto;
    scrollbar-width: none;
    padding: 56px 64px 96px;
    display: flex; flex-direction: column; gap: 28px;
    max-width: 1200px;
    margin: 0 auto;
    width: 100%;
  }
  @media (max-width: 720px) {
    .ho-scroll { padding: 28px 22px 60px; gap: 20px; }
  }

  /* HERO ---------------------------------------------------------- */
  .ho-hero {
    display: grid;
    grid-template-columns: minmax(0, 1.4fr) minmax(280px, 1fr);
    gap: 28px;
    align-items: stretch;
    padding: 24px 28px;
    border-radius: 22px;
    background:
      linear-gradient(180deg, color-mix(in srgb, var(--accent) 5%, transparent), transparent),
      var(--bg-1);
    border: 1px solid color-mix(in srgb, var(--accent) 18%, var(--border));
    box-shadow:
      inset 0 1px 0 color-mix(in srgb, var(--accent) 14%, transparent),
      0 12px 30px -16px var(--accent-glow);
  }
  @media (max-width: 880px) {
    .ho-hero { grid-template-columns: 1fr; }
  }
  .ho-hero-left { display: flex; align-items: center; gap: 22px; min-width: 0; }
  /* Brand mark sits naked on the hero — no tile, no border, no glow.
     The aurora wash behind the page already gives the W enough
     atmosphere; framing it again read as nested cards. */
  .ho-hero-mark {
    width: 76px; height: 76px;
    display: grid; place-items: center;
    flex-shrink: 0;
  }
  .ho-hero-text { display: flex; flex-direction: column; gap: 6px; min-width: 0; }
  .ho-hero-h {
    margin: 0;
    font-family: 'Geist', 'Inter', system-ui, sans-serif;
    font-size: 34px;
    font-weight: 600;
    letter-spacing: -0.02em;
    color: var(--text-0);
    line-height: 1;
  }
  /* Pulse line — mid-weight, one-line, primary signal of the page. */
  .ho-hero-pulse {
    margin: 0;
    font-size: 14.5px;
    font-weight: 500;
    color: var(--text-1);
    line-height: 1.4;
  }
  .ho-hero-date {
    font-size: 10.5px;
    text-transform: uppercase;
    letter-spacing: 0.13em;
    color: var(--text-mute);
  }
  /* Meta row under the pulse line — date stays calm, tour pill draws
     a soft accent stroke so first-time users notice it without it
     screaming on every load. */
  .ho-hero-meta {
    display: flex; align-items: center; gap: 14px;
    margin-top: 4px;
    flex-wrap: wrap;
  }
  .ho-hero-tour {
    display: inline-flex; align-items: center; gap: 6px;
    padding: 4px 10px 4px 8px;
    border-radius: 999px;
    background: color-mix(in srgb, var(--accent) 8%, transparent);
    border: 1px solid color-mix(in srgb, var(--accent) 28%, var(--border));
    color: var(--accent-bright);
    font-size: 11.5px;
    font-weight: 500;
    cursor: pointer;
    transition: background 140ms, border-color 140ms, transform 140ms;
  }
  .ho-hero-tour:hover {
    background: color-mix(in srgb, var(--accent) 16%, transparent);
    border-color: color-mix(in srgb, var(--accent) 42%, var(--border));
    transform: translateY(-1px);
  }
  .ho-hero-tour-kbd {
    background: var(--bg-2);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 0 5px;
    font-size: 9.5px;
    color: var(--text-1);
  }

  /* 24h activity histogram — ambient, not central. */
  .ho-spark {
    display: flex; flex-direction: column; gap: 10px;
    padding: 16px 18px;
    border-radius: 14px;
    background:
      linear-gradient(180deg, color-mix(in srgb, var(--accent) 8%, transparent), transparent),
      color-mix(in srgb, var(--bg-2) 82%, transparent);
    border: 1px solid var(--border);
    min-width: 0;
  }
  .ho-spark-head { display: flex; align-items: baseline; gap: 12px; }
  .ho-spark-label {
    font-size: 9.5px; font-weight: 700;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    color: var(--text-mute);
  }
  .ho-spark-meta {
    font-size: 10.5px;
    color: var(--accent-bright);
    margin-left: auto;
  }
  .ho-spark-bars {
    display: grid;
    grid-template-columns: repeat(24, 1fr);
    gap: 3px;
    align-items: end;
    height: 38px;
  }
  .ho-spark-bar {
    background: var(--accent);
    border-radius: 2px;
    width: 100%;
    transition: background 140ms;
  }
  .ho-spark-bar--zero {
    background: color-mix(in srgb, var(--accent) 16%, transparent);
  }
  .ho-spark-bar--peak {
    background: var(--accent-bright);
    box-shadow: 0 0 8px var(--accent-glow);
  }
  .ho-spark-axis {
    display: flex; justify-content: space-between;
    font-size: 9.5px;
    color: var(--text-mute);
    letter-spacing: 0.04em;
  }

  /* STAT STRIP -------------------------------------------------- */
  .ho-stats {
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    gap: 12px;
  }
  @media (max-width: 720px) {
    .ho-stats { grid-template-columns: repeat(2, minmax(0, 1fr)); }
  }
  .ho-stat {
    position: relative;
    overflow: hidden;
    text-align: left;
    background: var(--bg-1);
    border: 1px solid var(--border);
    border-radius: 14px;
    padding: 16px 18px;
    display: flex; flex-direction: column; gap: 8px;
    cursor: pointer;
    transition: border-color 160ms, transform 160ms, box-shadow 160ms;
    isolation: isolate;
  }
  .ho-stat:hover { transform: translateY(-2px); }

  /* Per-tone gradient halo behind the stat — sits above the bg but
     below the content (z-index: 0; content gets z-index: 1 via the
     `position: relative` on the .ho-stat-* children). */
  .ho-stat-glow {
    position: absolute;
    inset: 0;
    z-index: 0;
    opacity: 0.35;
    background: radial-gradient(120% 120% at 110% -20%, var(--ho-stat-tone, transparent), transparent 65%);
    pointer-events: none;
    transition: opacity 160ms;
  }
  .ho-stat:hover .ho-stat-glow { opacity: 0.6; }
  .ho-stat-icon, .ho-stat-label, .ho-stat-row, .ho-stat-sub { position: relative; z-index: 1; }

  .ho-stat[data-tone="agent"] {
    --ho-stat-tone: var(--accent);
    border-color: color-mix(in srgb, var(--accent) 28%, var(--border));
  }
  .ho-stat[data-tone="github"] {
    --ho-stat-tone: var(--src-github);
    border-color: color-mix(in srgb, var(--src-github) 22%, var(--border));
  }
  .ho-stat[data-tone="jira"] {
    --ho-stat-tone: var(--src-jira);
    border-color: color-mix(in srgb, var(--src-jira) 22%, var(--border));
  }
  .ho-stat[data-tone="sentry"] {
    --ho-stat-tone: var(--src-sentry);
    border-color: color-mix(in srgb, var(--src-sentry) 22%, var(--border));
  }
  .ho-stat:hover {
    box-shadow: 0 8px 24px -12px var(--ho-stat-tone);
    border-color: color-mix(in srgb, var(--ho-stat-tone) 50%, var(--border));
  }

  .ho-stat-icon {
    width: 28px; height: 28px;
    display: grid; place-items: center;
    border-radius: 8px;
    background: color-mix(in srgb, var(--ho-stat-tone) 14%, var(--bg-2));
    color: var(--ho-stat-tone);
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--ho-stat-tone) 28%, transparent);
  }
  .ho-stat-icon svg { width: 14px; height: 14px; }
  .ho-stat-label {
    font-size: 9.5px;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    color: var(--text-mute);
    font-weight: 700;
  }
  .ho-stat-row { display: inline-flex; align-items: baseline; gap: 6px; }
  .ho-stat-value {
    font-family: 'Geist', system-ui, sans-serif;
    font-size: 32px;
    font-weight: 600;
    letter-spacing: -0.025em;
    line-height: 1;
    color: var(--ho-stat-tone);
  }
  .ho-stat-of {
    font-size: 14px;
    font-weight: 500;
    color: var(--text-mute);
    letter-spacing: 0;
  }
  .ho-stat-sub { font-size: 11px; color: var(--text-2); }

  /* TWO-UP GRID ------------------------------------------------- */
  .ho-grid {
    display: grid;
    grid-template-columns: minmax(0, 1.4fr) minmax(0, 1fr);
    gap: 14px;
  }
  @media (max-width: 960px) { .ho-grid { grid-template-columns: minmax(0, 1fr); } }

  .ho-card {
    background: linear-gradient(180deg, color-mix(in srgb, var(--accent) 4%, transparent), transparent), var(--bg-1);
    border: 1px solid var(--border);
    border-radius: 16px;
    padding: 18px 20px 20px;
    display: flex; flex-direction: column; gap: 14px;
    min-width: 0;
  }
  .ho-card-head { display: flex; align-items: baseline; gap: 12px; }
  .ho-card-h {
    margin: 0;
    font-family: 'Geist', system-ui, sans-serif;
    font-size: 17px;
    font-weight: 600;
    color: var(--text-0);
    letter-spacing: -0.01em;
    flex: 1;
  }
  .ho-card-link {
    background: transparent; border: 0;
    color: var(--accent-bright);
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    cursor: pointer;
  }
  .ho-card-link:hover { color: var(--accent); }
  .ho-card-sub {
    font-size: 9.5px;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    color: var(--text-mute);
    font-weight: 700;
  }

  /* Empty state inside chats card — same vibe across cards. */
  .ho-empty {
    display: flex; flex-direction: column; gap: 10px;
    padding: 24px 4px 6px;
    text-align: center;
    color: var(--text-1);
  }
  .ho-empty-h { margin: 0; font-size: 13px; font-weight: 600; color: var(--text-0); }
  .ho-empty-p { margin: 0; font-size: 12px; color: var(--text-2); line-height: 1.5; }
  .ho-empty-row { display: flex; gap: 8px; justify-content: center; flex-wrap: wrap; margin-top: 6px; }
  .ho-cta {
    display: inline-flex; align-items: center; gap: 6px;
    padding: 7px 14px;
    border-radius: 8px;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
    border: 1px solid var(--border-hi);
    background: var(--bg-2);
    color: var(--text-0);
  }
  .ho-cta:hover { background: var(--bg-3); }
  .ho-cta--claude { color: var(--src-claude); border-color: color-mix(in srgb, var(--src-claude) 30%, transparent); }
  .ho-cta--cursor { color: var(--src-cursor); border-color: color-mix(in srgb, var(--src-cursor) 30%, transparent); }
  .ho-cta svg { width: 11px; height: 11px; }

  /* Active conversations rows. Brand-colour left border on hover so
     the row reads as a "claude row" or a "cursor row" without
     putting a chip on every line. */
  .ho-chat-list { list-style: none; padding: 0; margin: 0; display: flex; flex-direction: column; gap: 6px; }
  .ho-chat-row {
    position: relative;
    display: grid; grid-template-columns: 32px 1fr auto; align-items: center; gap: 12px;
    width: 100%;
    padding: 11px 12px 11px 14px;
    border-radius: 10px;
    background: var(--bg-2);
    border: 1px solid transparent;
    text-align: left;
    cursor: pointer;
    transition: border-color 160ms, background 160ms, transform 160ms;
  }
  .ho-chat-row::before {
    content: '';
    position: absolute;
    left: 0; top: 8px; bottom: 8px;
    width: 3px;
    border-radius: 2px;
    background: transparent;
    transition: background 160ms;
  }
  .ho-chat-row[data-agent="claude"]::before { background: color-mix(in srgb, var(--src-claude) 30%, transparent); }
  .ho-chat-row[data-agent="cursor"]::before { background: color-mix(in srgb, var(--src-cursor) 30%, transparent); }
  .ho-chat-row:hover {
    background: var(--bg-3);
    transform: translateX(2px);
  }
  .ho-chat-row[data-agent="claude"]:hover {
    border-color: color-mix(in srgb, var(--src-claude) 38%, transparent);
  }
  .ho-chat-row[data-agent="cursor"]:hover {
    border-color: color-mix(in srgb, var(--src-cursor) 38%, transparent);
  }
  .ho-chat-row[data-agent="claude"]:hover::before { background: var(--src-claude); }
  .ho-chat-row[data-agent="cursor"]:hover::before { background: var(--src-cursor); }
  .ho-chat-icon {
    width: 32px; height: 32px;
    display: grid; place-items: center;
    border-radius: 8px;
    box-shadow: inset 0 0 0 1px var(--border);
    flex-shrink: 0;
  }
  .ho-chat-icon[data-agent="claude"] {
    background: color-mix(in srgb, var(--src-claude) 14%, var(--bg-3));
    color: var(--src-claude);
  }
  .ho-chat-icon[data-agent="cursor"] {
    background: color-mix(in srgb, var(--src-cursor) 14%, var(--bg-3));
    color: var(--src-cursor);
  }
  /* BrandIcon sets its own width/height — no per-svg sizing needed. */
  .ho-chat-body { display: flex; flex-direction: column; gap: 3px; min-width: 0; }
  .ho-chat-title {
    font-size: 13px; font-weight: 500; color: var(--text-0);
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .ho-chat-snippet {
    font-size: 11.5px; color: var(--text-2);
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .ho-chat-snippet--empty { color: var(--text-mute); }
  .ho-chat-meta { flex-shrink: 0; }

  .ho-chip {
    display: inline-flex; align-items: center; gap: 5px;
    font-size: 9.5px; font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    padding: 2px 8px;
    border-radius: 999px;
    border: 1px solid transparent;
  }
  .ho-chip-dot {
    width: 6px; height: 6px;
    border-radius: 50%;
    animation: ho-pulse 1.2s ease-in-out infinite;
  }
  @keyframes ho-pulse {
    0%, 100% { opacity: 0.45; transform: scale(0.85); }
    50%      { opacity: 1; transform: scale(1.15); }
  }
  .ho-chip--running[data-agent="claude"] {
    color: var(--src-claude);
    background: color-mix(in srgb, var(--src-claude) 14%, transparent);
    border-color: color-mix(in srgb, var(--src-claude) 30%, transparent);
  }
  .ho-chip--running[data-agent="claude"] .ho-chip-dot {
    background: var(--src-claude);
    box-shadow: 0 0 6px color-mix(in srgb, var(--src-claude) 70%, transparent);
  }
  .ho-chip--running[data-agent="cursor"] {
    color: var(--src-cursor);
    background: color-mix(in srgb, var(--src-cursor) 14%, transparent);
    border-color: color-mix(in srgb, var(--src-cursor) 30%, transparent);
  }
  .ho-chip--running[data-agent="cursor"] .ho-chip-dot {
    background: var(--src-cursor);
    box-shadow: 0 0 6px color-mix(in srgb, var(--src-cursor) 70%, transparent);
  }
  .ho-chip--queued {
    color: var(--accent-bright);
    background: color-mix(in srgb, var(--accent) 14%, transparent);
    border-color: color-mix(in srgb, var(--accent) 30%, transparent);
  }
  .ho-chip-time { font-size: 11px; color: var(--text-mute); }

  /* Inbox digest blocks — tinted backgrounds so they're never plain. */
  .ho-source {
    border-radius: 12px;
    padding: 12px 14px;
    display: flex; flex-direction: column; gap: 8px;
    border: 1px solid var(--border);
  }
  .ho-source[data-source="github"] {
    background: linear-gradient(180deg, color-mix(in srgb, var(--src-github) 8%, transparent), transparent);
    border-color: color-mix(in srgb, var(--src-github) 18%, var(--border));
  }
  .ho-source[data-source="jira"] {
    background: linear-gradient(180deg, color-mix(in srgb, var(--src-jira) 8%, transparent), transparent);
    border-color: color-mix(in srgb, var(--src-jira) 18%, var(--border));
  }
  .ho-source[data-source="sentry"] {
    background: linear-gradient(180deg, color-mix(in srgb, var(--src-sentry) 8%, transparent), transparent);
    border-color: color-mix(in srgb, var(--src-sentry) 18%, var(--border));
  }
  .ho-source-head { display: flex; align-items: center; gap: 10px; }
  .ho-source-tag {
    display: inline-flex;
    padding: 2px 8px;
    border-radius: 4px;
    font-size: 9.5px;
    font-weight: 700;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    flex-shrink: 0;
  }
  .ho-source-tag[data-source="github"] {
    background: color-mix(in srgb, var(--src-github) 18%, var(--bg-2));
    color: var(--src-github);
    border: 1px solid color-mix(in srgb, var(--src-github) 30%, transparent);
  }
  .ho-source-tag[data-source="jira"] {
    background: color-mix(in srgb, var(--src-jira) 18%, var(--bg-2));
    color: var(--src-jira);
    border: 1px solid color-mix(in srgb, var(--src-jira) 30%, transparent);
  }
  .ho-source-tag[data-source="sentry"] {
    background: color-mix(in srgb, var(--src-sentry) 18%, var(--bg-2));
    color: var(--src-sentry);
    border: 1px solid color-mix(in srgb, var(--src-sentry) 30%, transparent);
  }
  .ho-source-count {
    font-size: 10px; color: var(--text-2);
    letter-spacing: 0.04em;
  }
  .ho-source-link {
    margin-left: auto;
    background: transparent; border: 0;
    color: var(--text-mute);
    font-size: 9.5px;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    cursor: pointer;
  }
  .ho-source-link:hover { color: var(--accent-bright); }
  .ho-source-empty { margin: 0; font-size: 11.5px; color: var(--text-mute); }
  .ho-source-list { list-style: none; padding: 0; margin: 0; display: flex; flex-direction: column; gap: 4px; }
  .ho-source-row {
    display: grid; grid-template-columns: auto 1fr auto; align-items: center; gap: 10px;
    width: 100%;
    padding: 6px 8px;
    border-radius: 6px;
    background: transparent; border: 0;
    text-align: left;
    cursor: pointer;
    transition: background 120ms;
  }
  .ho-source-row:hover { background: color-mix(in srgb, var(--bg-3) 70%, transparent); }
  .ho-source-num {
    font-size: 10.5px;
    flex-shrink: 0;
    padding: 1px 6px;
    border-radius: 4px;
    background: color-mix(in srgb, var(--bg-2) 80%, transparent);
  }
  .ho-source-num[data-source="github"] { color: var(--src-github); }
  .ho-source-num[data-source="jira"] { color: var(--src-jira); }
  .ho-source-num[data-source="sentry"] { color: var(--src-sentry); }
  .ho-source-title {
    font-size: 12px; color: var(--text-0);
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .ho-source-when { font-size: 10px; color: var(--text-mute); flex-shrink: 0; }

  /* QUICK ACTIONS ------------------------------------------------ */
  .ho-quick { display: flex; flex-direction: column; gap: 12px; }
  .ho-quick-h {
    margin: 0;
    font-family: 'Geist', system-ui, sans-serif;
    font-size: 17px;
    font-weight: 600;
    color: var(--text-0);
    letter-spacing: -0.01em;
  }
  .ho-quick-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
    gap: 10px;
  }
  .ho-quick-card {
    display: flex; align-items: center; gap: 12px;
    padding: 14px 16px;
    border-radius: 12px;
    background: var(--bg-1);
    border: 1px solid var(--border);
    cursor: pointer;
    text-align: left;
    transition: border-color 160ms, background 160ms, transform 160ms, box-shadow 160ms;
  }
  .ho-quick-card:hover {
    transform: translateY(-2px);
    background: var(--bg-2);
  }
  .ho-quick-card[data-agent="claude"]:hover {
    border-color: color-mix(in srgb, var(--src-claude) 38%, transparent);
    box-shadow: 0 8px 22px -14px var(--src-claude);
  }
  .ho-quick-card[data-agent="cursor"]:hover {
    border-color: color-mix(in srgb, var(--src-cursor) 38%, transparent);
    box-shadow: 0 8px 22px -14px var(--src-cursor);
  }
  .ho-quick-card:not([data-agent]):hover {
    border-color: color-mix(in srgb, var(--accent) 38%, transparent);
    box-shadow: 0 8px 22px -14px var(--accent-glow);
  }
  .ho-quick-icon {
    width: 36px; height: 36px;
    display: grid; place-items: center;
    border-radius: 9px;
    box-shadow: inset 0 0 0 1px var(--border);
    flex-shrink: 0;
    background: var(--bg-3); color: var(--accent-bright);
  }
  .ho-quick-icon[data-agent="claude"] {
    background:
      linear-gradient(160deg, color-mix(in srgb, var(--src-claude) 22%, transparent), color-mix(in srgb, var(--src-claude) 6%, transparent)),
      var(--bg-3);
    color: var(--src-claude);
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--src-claude) 32%, transparent);
  }
  .ho-quick-icon[data-agent="cursor"] {
    background:
      linear-gradient(160deg, color-mix(in srgb, var(--src-cursor) 22%, transparent), color-mix(in srgb, var(--src-cursor) 6%, transparent)),
      var(--bg-3);
    color: var(--src-cursor);
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--src-cursor) 32%, transparent);
  }
  .ho-quick-icon--mint {
    background:
      linear-gradient(160deg, color-mix(in srgb, var(--accent) 22%, transparent), color-mix(in srgb, var(--accent) 6%, transparent)),
      var(--bg-3);
    color: var(--accent-bright);
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--accent) 32%, transparent);
  }
  .ho-quick-icon svg { width: 16px; height: 16px; }
  .ho-quick-body { display: flex; flex-direction: column; gap: 3px; min-width: 0; }
  .ho-quick-title { font-size: 13px; font-weight: 500; color: var(--text-0); }
  .ho-quick-sub { font-size: 11px; color: var(--text-mute); }
</style>
