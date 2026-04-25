<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import {
    relativeTime,
    sentryLevelClass,
    type SentryEvent,
    type SentryEventDetail,
    type SentryIssue
  } from '$lib/data';
  import { inboxState, openSentryFocus } from '$lib/state/inbox.svelte';
  import { notify, notifyError } from '$lib/state/toaster.svelte';

  interface Props {
    issueId: string;
    now: number;
    onClose: () => void;
    onOpenBrowser: (url: string) => void;
  }
  let { issueId, now, onClose, onOpenBrowser }: Props = $props();

  let issue = $state<SentryIssue | null>(null);
  let issueLoading = $state(false);
  let issueError = $state<string | null>(null);

  let event = $state<SentryEventDetail | null>(null);
  let eventLoading = $state(false);
  let eventError = $state<string | null>(null);

  // Per-issue events list — populates the "Other events" picker so the
  // user can hop between occurrences without going through the agent
  // or leaving the app. Loaded once per issueId change (separate from
  // the active event detail above so we don't re-fetch the list every
  // time the user clicks a different event).
  let events = $state<SentryEvent[]>([]);
  let eventsLoading = $state(false);
  let eventsError = $state<string | null>(null);
  let eventsExpanded = $state(false);

  // Refresh on every issueId change. Also re-runs when the agent (via
  // mcp__app__open_sentry_event) sets `inboxState.sentryFocusEventId`
  // to a specific event id — without that dependency the pane would
  // stay on the latest event even after the agent navigated.
  $effect(() => {
    if (!issueId) return;
    // touch the focus-event slot so the effect re-runs when it changes
    void inboxState.sentryFocusEventId;
    void loadIssue();
    void loadEvent();
  });

  // Events list is keyed only on issueId (no eventId dep) so picking a
  // different event from the list doesn't re-fetch the list itself.
  $effect(() => {
    if (!issueId) return;
    void loadEvents();
  });

  async function loadEvents() {
    eventsLoading = true;
    eventsError = null;
    try {
      events = await invoke<SentryEvent[]>('sentry_list_events', {
        issueId,
        limit: 30
      });
    } catch (e) {
      events = [];
      eventsError = typeof e === 'string' ? e : String(e);
    } finally {
      eventsLoading = false;
    }
  }

  async function loadIssue() {
    issueLoading = true;
    issueError = null;
    try {
      issue = await invoke<SentryIssue>('sentry_get_issue', { issueId });
    } catch (e) {
      issueError = typeof e === 'string' ? e : String(e);
    } finally {
      issueLoading = false;
    }
  }

  async function loadEvent() {
    eventLoading = true;
    eventError = null;
    try {
      event = await invoke<SentryEventDetail>('sentry_get_event_detail', {
        issueId,
        eventId: inboxState.sentryFocusEventId ?? 'latest'
      });
    } catch (e) {
      eventError = typeof e === 'string' ? e : String(e);
    } finally {
      eventLoading = false;
    }
  }

  let actionBusy = $state<'resolve' | 'unresolve' | 'ignore' | null>(null);

  async function setStatus(
    status: 'resolved' | 'unresolved' | 'ignored',
    label: 'resolve' | 'unresolve' | 'ignore'
  ) {
    if (!issue) return;
    actionBusy = label;
    try {
      const updated = await invoke<SentryIssue>('sentry_set_status', { issueId, status });
      issue = updated;
      const idx = inboxState.sentryItems.findIndex((i) => i.id === issueId);
      if (idx >= 0) {
        inboxState.sentryItems = [
          ...inboxState.sentryItems.slice(0, idx),
          updated,
          ...inboxState.sentryItems.slice(idx + 1)
        ];
      }
      notify({ kind: 'success', title: `Marked ${label}d`, ttlMs: 1800 });
    } catch (e) {
      notifyError(e, { title: `Couldn't ${label}` });
    } finally {
      actionBusy = null;
    }
  }

  function frameLabel(f: NonNullable<SentryEventDetail['exceptions'][number]['frames']>[number]): string {
    const fn = f.function ?? '?';
    const file = f.filename ?? f.abs_path ?? '?';
    const line = f.lineno != null ? `:${f.lineno}` : '';
    return `${fn} (${file}${line})`;
  }
</script>

<div class="sdp">
  <header class="sdp-head">
    <button class="sdp-back" onclick={onClose} aria-label="Close" title="Close">
      <svg class="i i-sm" viewBox="0 0 24 24"><path d="M18 6 6 18M6 6l12 12" /></svg>
    </button>
    <span class="sdp-key mono">{issue?.short_id ?? issueId}</span>
    {#if issue}
      <span class="mini-tag {sentryLevelClass(issue.level)}">{issue.level}</span>
      <span class="mini-tag {issue.status === 'resolved' ? 'tag--closed' : issue.status === 'ignored' ? 'tag--draft' : 'tag--open'}">{issue.status}</span>
      {#if issue.platform}<span class="sdp-kind">· {issue.platform}</span>{/if}
      <span class="sdp-kind">· {issue.project_slug}</span>
    {/if}
    <div style="flex:1"></div>
    {#if issue}
      {#if issue.status === 'resolved'}
        <button class="sdp-btn" disabled={actionBusy !== null} onclick={() => void setStatus('unresolved', 'unresolve')}>
          {actionBusy === 'unresolve' ? 'Re-opening…' : 'Re-open'}
        </button>
      {:else}
        <button class="sdp-btn sdp-btn--primary" disabled={actionBusy !== null} onclick={() => void setStatus('resolved', 'resolve')}>
          {actionBusy === 'resolve' ? 'Resolving…' : 'Resolve'}
        </button>
        <button class="sdp-btn" disabled={actionBusy !== null || issue.status === 'ignored'} onclick={() => void setStatus('ignored', 'ignore')}>
          {actionBusy === 'ignore' ? 'Ignoring…' : 'Ignore'}
        </button>
      {/if}
    {/if}
    <button
      class="sdp-btn sdp-btn--icon"
      onclick={() => { void loadIssue(); void loadEvent(); }}
      disabled={issueLoading || eventLoading}
      title="Refresh issue + latest event"
      aria-label="Refresh"
    >
      <svg class="i i-sm" class:sdp-spin={issueLoading || eventLoading} viewBox="0 0 24 24">
        <path d="M21 12a9 9 0 0 1-9 9 9 9 0 0 1-8.5-6"/>
        <path d="M3 12a9 9 0 0 1 9-9 9 9 0 0 1 8.5 6"/>
        <polyline points="21 3 21 9 15 9"/>
        <polyline points="3 21 3 15 9 15"/>
      </svg>
    </button>
    <button class="sdp-btn" onclick={() => issue?.permalink && onOpenBrowser(issue.permalink)} disabled={!issue?.permalink} title="Open on Sentry">
      <svg class="i i-sm" viewBox="0 0 24 24"><path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"/><path d="M15 3h6v6M10 14 21 3"/></svg>
      Open on Sentry
    </button>
  </header>

  {#if issueLoading && !issue}
    <div class="sdp-state">Loading issue…</div>
  {:else if issueError}
    <div class="sdp-state sdp-err">
      {issueError}
      <button class="sdp-link" onclick={() => void loadIssue()}>Retry</button>
    </div>
  {:else if issue}
    <div class="sdp-body">
      <!-- Title -->
      <h1 class="sdp-title">{issue.title}</h1>

      {#if issue.metadata_value}
        <div class="sdp-exception mono">
          {#if issue.metadata_type}<span class="sdp-exc-type">{issue.metadata_type}:</span>{/if}
          {issue.metadata_value}
        </div>
      {/if}

      {#if issue.culprit}
        <div class="sdp-culprit mono">{issue.culprit}</div>
      {/if}

      <!-- Stat grid -->
      <div class="sdp-stats">
        <div class="sdp-stat">
          <div class="sdp-stat-k">EVENTS</div>
          <div class="sdp-stat-v mono">{issue.count}</div>
        </div>
        <div class="sdp-stat">
          <div class="sdp-stat-k">USERS AFFECTED</div>
          <div class="sdp-stat-v mono">{issue.user_count}</div>
        </div>
        <div class="sdp-stat">
          <div class="sdp-stat-k">FIRST SEEN</div>
          <div class="sdp-stat-v">{relativeTime(issue.first_seen, now)}</div>
        </div>
        <div class="sdp-stat">
          <div class="sdp-stat-k">LAST SEEN</div>
          <div class="sdp-stat-v">{relativeTime(issue.last_seen, now)}</div>
        </div>
      </div>

      <!-- Other events picker. Collapsed by default to keep the pane
           compact; expand to scan / pick a different occurrence. The
           agent's `mcp__app__open_sentry_event` calls land on the
           same `sentryFocusEventId` slot, so click-from-UI and
           click-from-chat funnel through one path. -->
      {#if events.length > 1 || eventsLoading || eventsError}
        <section class="sdp-section">
          <header class="sdp-section-head">
            <h3 class="sdp-section-title">Other events</h3>
            {#if events.length > 0}
              <span class="sdp-section-sub mono">{events.length}{events.length === 30 ? '+' : ''}</span>
            {/if}
            <div style="flex:1"></div>
            <button class="sdp-link" onclick={() => void loadEvents()} disabled={eventsLoading}>
              {eventsLoading ? 'Loading…' : 'Refresh'}
            </button>
            <button class="sdp-link" onclick={() => (eventsExpanded = !eventsExpanded)}>
              {eventsExpanded ? 'Hide' : 'Show'}
            </button>
          </header>
          {#if eventsError}
            <div class="sdp-state sdp-err">{eventsError}</div>
          {:else if eventsExpanded}
            <div class="sdp-events">
              {#each events as ev (ev.event_id)}
                {@const active = (inboxState.sentryFocusEventId ?? '') === ev.event_id
                  || (!inboxState.sentryFocusEventId && event?.event_id === ev.event_id)}
                <button
                  class="sdp-event-row"
                  class:sdp-event-row--active={active}
                  onclick={() => openSentryFocus(issueId, ev.event_id)}
                  title={ev.event_id}
                >
                  <span class="sdp-event-id mono">{ev.event_id.slice(0, 8)}</span>
                  <span class="sdp-event-when mono">{relativeTime(ev.date_created, now)}</span>
                  <span class="sdp-event-msg">{ev.exception_summary ?? ev.message ?? ''}</span>
                  {#if ev.platform}<span class="sdp-event-tag mono">{ev.platform}</span>{/if}
                </button>
              {/each}
              {#if inboxState.sentryFocusEventId}
                <button class="sdp-link sdp-link--center" onclick={() => openSentryFocus(issueId, null)}>
                  ← Back to latest
                </button>
              {/if}
            </div>
          {/if}
        </section>
      {/if}

      <!-- Latest event -->
      <section class="sdp-section">
        <header class="sdp-section-head">
          <h3 class="sdp-section-title">{inboxState.sentryFocusEventId ? 'Selected event' : 'Latest event'}</h3>
          {#if event?.event_id}<span class="sdp-section-sub mono">{event.event_id.slice(0, 8)}</span>{/if}
          <div style="flex:1"></div>
          <button class="sdp-link" onclick={() => void loadEvent()} disabled={eventLoading}>
            {eventLoading ? 'Loading…' : 'Refresh'}
          </button>
        </header>
        {#if eventLoading && !event}
          <div class="sdp-state">Loading event…</div>
        {:else if eventError}
          <div class="sdp-state sdp-err">{eventError}</div>
        {:else if event}
          <div class="sdp-event-meta">
            {#if event.release}<span class="sdp-chip mono">release: {event.release}</span>{/if}
            {#if event.user_email || event.user_id}
              <span class="sdp-chip">user: {event.user_email ?? event.user_id}</span>
            {/if}
            <span class="sdp-chip mono">{event.date_created}</span>
          </div>

          {#each event.exceptions as exc, idx (idx)}
            <div class="sdp-exception-block">
              <div class="sdp-exception-head mono">
                {#if exc.type}<span class="sdp-exc-type">{exc.type}</span>{/if}
                {#if exc.value}<span class="sdp-exc-value"> : {exc.value}</span>{/if}
              </div>
              {#if exc.frames.length > 0}
                <div class="sdp-frames">
                  {#each exc.frames.slice().reverse() as f, fi (fi)}
                    <details class="sdp-frame" class:in-app={f.in_app} open={f.in_app && fi < 3}>
                      <summary class="sdp-frame-summary mono">
                        <span class="sdp-frame-fn">{frameLabel(f)}</span>
                        {#if f.in_app}<span class="sdp-frame-tag">app</span>{/if}
                      </summary>
                      {#if f.context.length > 0}
                        <pre class="sdp-frame-source mono">{#each f.context as l (l.line)}<span class="sdp-src-line" class:active={l.line === f.lineno}><span class="sdp-src-num">{l.line}</span>{l.source}
</span>{/each}</pre>
                      {/if}
                    </details>
                  {/each}
                </div>
              {/if}
            </div>
          {/each}

          {#if event.breadcrumbs_summary}
            <section class="sdp-section">
              <header class="sdp-section-head"><h3 class="sdp-section-title">Breadcrumbs (recent)</h3></header>
              <pre class="sdp-breadcrumbs mono">{event.breadcrumbs_summary}</pre>
            </section>
          {/if}

          {#if event.tags.length > 0}
            <section class="sdp-section">
              <header class="sdp-section-head"><h3 class="sdp-section-title">Tags</h3></header>
              <div class="sdp-tags-grid">
                {#each event.tags.slice(0, 30) as [k, v] (k + v)}
                  <div class="sdp-tag-row">
                    <span class="sdp-tag-k mono">{k}</span>
                    <span class="sdp-tag-v mono">{v}</span>
                  </div>
                {/each}
              </div>
            </section>
          {/if}
        {/if}
      </section>
    </div>
  {/if}
</div>

<style>
  /* Same shape as `.jdp` (JiraDetailPane) — fills the parent `.slide-panel`,
     header bar with close on the left + action buttons on the right, body
     scrolls with consistent padding. Keeps both panes feeling the same. */
  .sdp { height: 100%; display: flex; flex-direction: column; min-height: 0; background: var(--bg-0); }
  .sdp-head {
    display: flex; align-items: center; gap: 10px;
    padding: 12px 20px;
    border-bottom: 1px solid var(--border-neutral);
    background: var(--bg-1);
    flex-shrink: 0;
  }
  .sdp-back {
    width: 28px; height: 28px; border-radius: 5px;
    display: inline-flex; align-items: center; justify-content: center;
    background: transparent; color: var(--text-1); border: none; cursor: pointer;
  }
  .sdp-back:hover { background: var(--bg-2); color: var(--text-0); }
  .sdp-key { font-size: 13px; color: var(--accent-bright); font-weight: 600; }
  .sdp-kind { font-size: 11px; color: var(--text-2); }
  .sdp-btn {
    display: inline-flex; align-items: center; gap: 6px;
    padding: 6px 12px; border-radius: 6px;
    background: var(--bg-2); color: var(--text-1);
    font-size: 12px; border: 1px solid var(--border-neutral-hi); cursor: pointer;
  }
  .sdp-btn:hover:not(:disabled) { background: var(--bg-3); color: var(--text-0); }
  .sdp-btn:disabled { opacity: 0.5; cursor: default; }
  .sdp-btn--icon { padding: 6px; }
  .sdp-btn--icon .i-sm { width: 14px; height: 14px; }
  .sdp-spin { animation: sdp-spin 0.8s linear infinite; }
  @keyframes sdp-spin { to { transform: rotate(360deg); } }
  .sdp-btn--primary {
    background: var(--accent); color: #1a0a04;
    border-color: transparent; font-weight: 600;
  }
  .sdp-btn--primary:hover:not(:disabled) { background: var(--accent-bright); }

  .sdp-state { padding: 40px; text-align: center; color: var(--text-2); }
  .sdp-err { color: var(--error); }
  .sdp-link {
    color: var(--accent-bright); margin-left: 6px; cursor: pointer;
    background: none; border: none; padding: 0; text-decoration: underline; font-size: 12px;
  }
  .sdp-link:disabled { opacity: 0.5; cursor: default; }

  .sdp-body {
    flex: 1; overflow-y: auto;
    padding: 24px 28px 60px;
    display: flex; flex-direction: column; gap: 18px;
  }
  .sdp-title {
    font-size: 22px; line-height: 1.3; font-weight: 600;
    color: var(--text-0); letter-spacing: -0.01em;
    margin: 0;
    overflow-wrap: anywhere;
  }
  .sdp-exception {
    background: var(--bg-1); border: 1px solid var(--border-neutral);
    border-radius: 8px; padding: 12px 14px; font-size: 13px;
    color: var(--text-0); line-height: 1.5;
    overflow-wrap: anywhere;
  }
  .sdp-exc-type { color: var(--accent-bright); font-weight: 600; }
  .sdp-exc-value { color: var(--text-1); }
  .sdp-culprit { font-size: 12.5px; color: var(--text-2); }

  .sdp-stats {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(140px, 1fr));
    gap: 10px;
  }
  .sdp-stat {
    background: var(--bg-1); border: 1px solid var(--border-neutral);
    border-radius: 8px; padding: 10px 14px;
  }
  .sdp-stat-k {
    font-size: 10.5px; color: var(--text-mute);
    text-transform: uppercase; letter-spacing: 0.05em; font-weight: 600;
  }
  .sdp-stat-v { font-size: 14px; color: var(--text-0); margin-top: 4px; }

  .sdp-section { display: flex; flex-direction: column; gap: 10px; }
  .sdp-section-head { display: flex; align-items: center; gap: 8px; }
  .sdp-section-title {
    font-size: 11px; font-weight: 700; color: var(--text-mute);
    text-transform: uppercase; letter-spacing: 0.05em; margin: 0;
  }
  .sdp-section-sub { font-size: 11px; color: var(--text-mute); }

  /* Other-events picker rows. Compact, click-to-load, highlight the
     currently-loaded one so the user can tell which event the body
     below corresponds to. */
  .sdp-events { display: flex; flex-direction: column; gap: 2px; }
  .sdp-event-row {
    display: flex; align-items: center; gap: 10px;
    padding: 6px 10px;
    background: var(--bg-1); border: 1px solid var(--border-neutral);
    border-radius: 6px; text-align: left; cursor: pointer;
    color: var(--text-1); font-size: 12px;
    transition: background 100ms;
    width: 100%;
  }
  .sdp-event-row:hover { background: var(--bg-2); color: var(--text-0); }
  .sdp-event-row--active {
    background: var(--accent-soft); border-color: rgba(232, 163, 58, 0.3);
    color: var(--text-0);
  }
  .sdp-event-id { color: var(--text-2); font-size: 11px; min-width: 70px; }
  .sdp-event-when { color: var(--text-mute); font-size: 11px; min-width: 70px; }
  .sdp-event-msg {
    flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
    font-size: 11.5px;
  }
  .sdp-event-tag {
    font-size: 10px; padding: 1px 6px; border-radius: 3px;
    background: var(--bg-2); color: var(--text-2);
    border: 1px solid var(--border-neutral);
  }
  .sdp-link--center {
    display: block; margin: 4px auto 0;
    text-align: center; font-size: 11px;
  }

  .sdp-event-meta { display: flex; flex-wrap: wrap; gap: 6px; font-size: 11px; }
  .sdp-chip {
    padding: 2px 8px; border-radius: 6px; font-size: 11px;
    background: var(--bg-2); color: var(--text-1);
    border: 1px solid var(--border-neutral);
  }

  .sdp-exception-block {
    display: flex; flex-direction: column; gap: 6px;
    background: var(--bg-1); border: 1px solid var(--border-neutral);
    border-radius: 8px; padding: 12px 14px;
  }
  .sdp-exception-head { font-size: 13px; color: var(--text-0); overflow-wrap: anywhere; }

  .sdp-frames { display: flex; flex-direction: column; gap: 4px; }
  .sdp-frame {
    background: var(--bg-0); border: 1px solid var(--border-neutral);
    border-radius: 6px; overflow: hidden;
  }
  .sdp-frame.in-app { border-color: var(--accent-soft); }
  .sdp-frame-summary {
    list-style: none; cursor: pointer; padding: 7px 12px;
    font-size: 11.5px; color: var(--text-1);
    display: flex; align-items: center; gap: 8px;
    overflow-wrap: anywhere;
  }
  .sdp-frame-summary::-webkit-details-marker { display: none; }
  .sdp-frame[open] .sdp-frame-summary {
    background: var(--bg-2); color: var(--text-0);
    border-bottom: 1px solid var(--border-neutral);
  }
  .sdp-frame-fn { flex: 1; }
  .sdp-frame-tag {
    font-size: 9px; font-weight: 700;
    padding: 1px 6px; border-radius: 3px;
    background: var(--accent-soft); color: var(--accent-bright);
    text-transform: uppercase; letter-spacing: 0.05em;
  }
  .sdp-frame-source {
    margin: 0; padding: 8px 0;
    background: var(--bg-0);
    font-size: 11.5px; line-height: 1.5;
    overflow-x: auto; color: var(--text-1);
    white-space: pre;
  }
  .sdp-src-line { display: inline-block; min-width: 100%; padding: 0 14px; }
  .sdp-src-line.active { background: rgba(232, 163, 58, 0.08); }
  .sdp-src-num {
    display: inline-block; min-width: 36px;
    color: var(--text-mute); margin-right: 12px;
    text-align: right;
  }

  .sdp-breadcrumbs {
    background: var(--bg-1); border: 1px solid var(--border-neutral);
    border-radius: 8px; padding: 12px 14px;
    font-size: 11.5px; color: var(--text-1); margin: 0;
    white-space: pre-wrap; overflow-wrap: anywhere;
  }

  .sdp-tags-grid {
    display: grid; grid-template-columns: max-content 1fr;
    gap: 6px 18px; font-size: 12px;
  }
  .sdp-tag-k { color: var(--text-mute); }
  .sdp-tag-v { color: var(--text-1); overflow-wrap: anywhere; }

  .mini-tag {
    padding: 2px 7px; border-radius: 4px; font-weight: 600;
    text-transform: uppercase; font-size: 10px; letter-spacing: 0.05em;
  }
</style>
