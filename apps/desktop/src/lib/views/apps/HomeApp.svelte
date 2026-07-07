<script lang="ts">
  /* HomeApp — paper-mockup dashboard. Header: quiet date/path line +
     one hero sentence ("Morning. 4 tickets, 2 PRs waiting, 1
     regression."). Body: two columns — INBOX (rows pulled from every
     connected source) | AGENT ACTIVITY cards + BACKGROUND TASKS
     charcoal card. Everything reads live stores; rows navigate. */

  import { sessionsState } from '$lib/state/sessions.svelte';
  import { inboxState } from '$lib/state/inbox.svelte';
  import { bgTasksState } from '$lib/state/bgTasks.svelte';
  import { APP_INSTANCE_IDS, layoutState } from '$lib/state/layout.svelte';
  import { relativeTime } from '$lib/data';
  import type { View } from '$lib/state/view.svelte';

  interface Props {
    now: number;
    onNavigate: (v: View) => void;
    onOpenSession: (sessionId: string, agentInstanceId: string) => void;
    onNewChat: () => void;
    onOpenWelcome?: () => void;
  }
  let p: Props = $props();

  /* ---------- header ---------- */
  const greeting = $derived.by(() => {
    const h = new Date(p.now).getHours();
    if (h < 5) return 'Working late';
    if (h < 12) return 'Morning';
    if (h < 18) return 'Afternoon';
    return 'Evening';
  });

  const todayLabel = $derived.by(() => {
    const d = new Date(p.now);
    return d.toLocaleDateString('en-US', { weekday: 'long', month: 'long', day: 'numeric' });
  });

  const cwdLabel = $derived.by(() => {
    const path = layoutState.active.editor.repoPath;
    if (!path) return null;
    const parts = path.split('/').filter(Boolean);
    return `~/${parts.slice(-2).join('/')}`;
  });

  const jiraItems = $derived(Object.values(inboxState.jiraItemsByInstance).flat());
  const ghItems = $derived(
    Object.values(inboxState.itemsByInstance).flat().filter((i) => i.is_pull_request && i.state === 'open')
  );
  const sentryItems = $derived(Object.values(inboxState.sentryItemsByInstance).flat());

  const heroLine = $derived.by(() => {
    const parts: string[] = [];
    if (jiraItems.length) parts.push(`${jiraItems.length} ticket${jiraItems.length === 1 ? '' : 's'}`);
    if (ghItems.length) parts.push(`${ghItems.length} PR${ghItems.length === 1 ? '' : 's'} waiting`);
    if (sentryItems.length) parts.push(`${sentryItems.length} unresolved`);
    return parts.length ? parts.join(', ') + '.' : 'All clear.';
  });

  /* ---------- inbox rows ---------- */
  interface Row {
    id: string;
    src: 'jira' | 'github' | 'sentry';
    label: string;
    ref: string;
    title: string;
    at: number;
    go: () => void;
  }
  const inboxRows = $derived.by<Row[]>(() => {
    const rows: Row[] = [];
    const seen = new Set<string>();
    for (const it of jiraItems) {
      if (seen.has(`j${it.key}`)) continue;
      seen.add(`j${it.key}`);
      rows.push({
        id: `j${it.key}`, src: 'jira', label: 'JIRA', ref: it.key,
        title: it.summary, at: new Date(it.updated).getTime(),
        go: () => { inboxState.jiraFocusKey = it.key; p.onNavigate('jiraApp'); }
      });
    }
    for (const it of ghItems) {
      if (seen.has(`g${it.id}`)) continue;
      seen.add(`g${it.id}`);
      rows.push({
        id: `g${it.id}`, src: 'github', label: 'GITHUB', ref: `#${it.number}`,
        title: it.title, at: new Date(it.updated_at).getTime(),
        go: () => p.onNavigate('githubApp')
      });
    }
    for (const it of sentryItems) {
      if (seen.has(`s${it.id}`)) continue;
      seen.add(`s${it.id}`);
      rows.push({
        id: `s${it.id}`, src: 'sentry', label: 'SENTRY', ref: it.short_id,
        title: it.title, at: new Date(it.last_seen).getTime(),
        go: () => { inboxState.sentryFocusId = it.id; p.onNavigate('sentryApp'); }
      });
    }
    return rows.sort((a, b) => b.at - a.at).slice(0, 14);
  });

  /* ---------- agent activity ---------- */
  const agentCards = $derived.by(() => {
    const list = sessionsState.list.filter((s) => !s.archived);
    const t = (s: (typeof list)[number]) => {
      const last = s.messages[s.messages.length - 1]?.at;
      return last ? new Date(last).getTime() : 0;
    };
    return [...list].sort((a, b) => Number(b.sending) - Number(a.sending) || t(b) - t(a)).slice(0, 3);
  });
  function sessMeta(s: (typeof sessionsState.list)[number]): string {
    const parts: string[] = [];
    const model = s.claudeModel?.replace(/^claude-/, '');
    if (model) parts.push(model);
    parts.push(s.sending ? 'streaming' : 'idle');
    if (s.worktreeBranch) parts.push(`worktree ${s.worktreeBranch}`);
    return parts.join(' · ');
  }

  const runningTasks = $derived(
    bgTasksState.tasks.filter((t) => t.status.kind === 'running').slice(0, 2)
  );
</script>

<section class="ho">
  <header class="ho-head">
    <div class="ho-date">
      {todayLabel}{#if cwdLabel}&nbsp;· <span class="ho-date-path">{cwdLabel}</span>{/if}
      {#if p.onOpenWelcome}
        <button class="ho-tour" onclick={p.onOpenWelcome} type="button" title="Take the tour · ⇧⌘?">tour</button>
      {/if}
    </div>
    <h1 class="ho-hero">{greeting}. {heroLine}</h1>
  </header>

  <div class="ho-body">
    <div class="ho-inbox">
      <div class="app-label ho-inbox-label">Inbox — pulled from your sources</div>
      {#if inboxRows.length === 0}
        <div class="ho-quiet">Nothing pending. Connect sources in Settings, or enjoy the silence.</div>
      {:else}
        {#each inboxRows as r (r.id)}
          <button class="ho-row" onclick={r.go}>
            <span class="ho-row-src" data-src={r.src}>{r.label}</span>
            <span class="ho-row-ref">{r.ref}</span>
            <span class="ho-row-title">{r.title}</span>
            <span class="ho-row-when">{relativeTime(new Date(r.at).toISOString(), p.now)}</span>
          </button>
        {/each}
        <div class="ho-hint">Drag any item onto an agent chat to mention it.</div>
      {/if}
    </div>

    <div class="ho-side">
      <div class="app-label ho-side-label">Agent activity</div>
      {#if agentCards.length === 0}
        <button class="ho-card" onclick={p.onNewChat}>
          <div class="ho-card-h">No sessions yet</div>
          <div class="ho-card-meta">Start a chat — drop a ticket, PR or file on it.</div>
        </button>
      {:else}
        {#each agentCards as s (s.id)}
          <button
            class="ho-card"
            class:ho-card--live={s.sending}
            onclick={() => p.onOpenSession(s.id, s.agentInstanceId ?? APP_INSTANCE_IDS.claude)}
          >
            <div class="ho-card-h">
              {#if s.sending}<span class="ho-pulse"></span>{/if}
              {s.title || 'Untitled chat'}
            </div>
            <div class="ho-card-meta">{sessMeta(s)}</div>
          </button>
        {/each}
      {/if}

      <div class="app-label ho-side-label ho-side-label--tasks">Background tasks</div>
      {#if runningTasks.length === 0}
        <div class="ho-quiet ho-quiet--side">None running.</div>
      {:else}
        {#each runningTasks as t (t.id)}
          <button class="ho-task" onclick={() => p.onNavigate('claudeApp')}>
            <div class="ho-task-h">
              <span class="ho-spin" aria-hidden="true"></span>
              $ {t.cmd}
            </div>
            <div class="ho-task-meta">{t.label} · {t.cwd.split('/').filter(Boolean).pop()}</div>
          </button>
        {/each}
      {/if}
    </div>
  </div>
</section>

<style>
  /* Redesign v2 §2.7 — no list column; one centred column, max 1160,
     padding 46/64, everything scrolls together. */
  .ho {
    flex: 1; min-height: 0;
    overflow-y: auto;
    background: var(--bg-0);
    padding: 46px 64px 60px;
  }

  .ho-head {
    max-width: 1160px;
    margin: 0 auto 30px;
    padding: 0;
    border: 0;
  }
  .ho-date {
    font-size: 12.5px; color: var(--text-faint);
    margin-bottom: 8px;
    display: flex; align-items: center; gap: 6px;
  }
  .ho-date-path { color: var(--text-1); font-family: var(--font-mono); font-size: 11.5px; }
  .ho-tour {
    margin-left: 8px;
    font-size: 10px; color: var(--text-faint);
    background: transparent; border: 1px solid var(--border-hi);
    border-radius: var(--r-chip); padding: 1px 7px;
    cursor: pointer;
  }
  .ho-tour:hover { color: var(--text-0); border-color: var(--border-hi2); }
  .ho-hero {
    font-size: 27px; font-weight: 600;
    color: var(--text-0);
    letter-spacing: -0.015em;
    margin: 0;
  }

  .ho-body {
    max-width: 1160px;
    margin: 0 auto;
    display: grid;
    grid-template-columns: 1fr 340px;
    gap: 56px;
    align-items: start;
  }

  .ho-inbox { min-width: 0; }
  .ho-inbox-label { padding: 0 0 8px; display: block; }
  .ho-row {
    display: grid;
    grid-template-columns: 64px 92px 1fr auto;
    align-items: baseline; gap: 12px;
    width: 100%;
    padding: 10px 10px;
    border: 0;
    border-radius: 8px;
    background: transparent;
    cursor: pointer;
    text-align: left;
    transition: background 120ms;
  }
  .ho-row:hover { background: var(--bg-1); }
  .ho-row-src {
    font-size: 10px; font-weight: 600;
    letter-spacing: 0.06em; text-transform: uppercase;
  }
  .ho-row-src[data-src='jira'] { color: var(--src-jira); }
  .ho-row-src[data-src='github'] { color: var(--src-github); }
  .ho-row-src[data-src='sentry'] { color: var(--src-sentry); }
  .ho-row-ref { font-size: 11.5px; color: var(--text-1); font-family: var(--font-mono); }
  .ho-row-title {
    font-size: 13px; color: var(--text-0);
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
    min-width: 0;
  }
  .ho-row-when { font-size: 11px; color: var(--text-faint); font-family: var(--font-mono); text-align: right; }
  .ho-hint { padding: 10px 10px; font-size: 11px; color: var(--text-faint); }
  .ho-quiet { padding: 12px 10px; font-size: 11.5px; color: var(--text-mute); }
  .ho-quiet--side { padding: 4px 0; }

  .ho-side { min-width: 0; }
  .ho-side-label { display: block; padding: 0 0 8px; }
  .ho-side-label--tasks { padding-top: 20px; }

  .ho-card {
    display: block; width: 100%;
    border: 1px solid var(--border);
    border-radius: var(--r-card);
    background: transparent;
    padding: 12px 13px;
    margin-bottom: 8px;
    cursor: pointer;
    text-align: left;
    transition: border-color 140ms;
  }
  .ho-card:hover { border-color: var(--border-hi2); }
  .ho-card--live { background: var(--bg-2); box-shadow: var(--shadow-1); }
  .ho-card-h {
    display: flex; align-items: center; gap: 7px;
    font-size: 12px; font-weight: 600; color: var(--text-0);
  }
  .ho-card-meta { font-size: 11px; color: var(--text-mute); margin-top: 5px; }
  .ho-pulse {
    width: 7px; height: 7px; border-radius: 50%;
    background: var(--src-claude);
    animation: ho-pulsedot 1.6s infinite;
    flex: none;
  }

  /* Background task — charcoal inset card (same in both themes). */
  .ho-task {
    display: block; width: 100%;
    border: 1px solid rgba(0, 0, 0, 0.25);
    border-radius: var(--r-card);
    background: var(--dark-1);
    padding: 11px 13px;
    margin-bottom: 8px;
    cursor: pointer;
    text-align: left;
    box-shadow: var(--shadow-1);
  }
  .ho-task-h {
    display: flex; align-items: center; gap: 8px;
    font-size: 11.5px; color: var(--dark-text);
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .ho-task-meta { font-size: 10.5px; color: var(--dark-mute); margin-top: 5px; }
  .ho-spin {
    display: inline-block; flex: none;
    width: 10px; height: 10px;
    border: 1.5px solid var(--dark-mute);
    border-top-color: var(--dark-text);
    border-radius: 50%;
    animation: ho-spin 0.9s linear infinite;
  }

  @keyframes ho-spin { to { transform: rotate(360deg); } }
  @keyframes ho-pulsedot {
    0%, 100% { opacity: 0.35; }
    50% { opacity: 1; }
  }
</style>
