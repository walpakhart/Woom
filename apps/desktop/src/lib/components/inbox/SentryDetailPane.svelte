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
  import { openFileInEditor } from '$lib/services/editorNavigation';
  import { recordCursor } from '$lib/state/editorCursors.svelte';

  interface Props {
    issueId: string;
    now: number;
    onClose: () => void;
    onOpenBrowser: (url: string) => void;
    /** Hand the focused issue off to Claude. Optional so
     *  any existing call site that doesn't wire it up still
     *  compiles — the header button is hidden when undefined. */
    onSendToClaude?: () => void;
  }
  let { issueId, now, onClose, onOpenBrowser, onSendToClaude }: Props = $props();

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
      /* Patch every Sentry column's items list — issue ids are global,
         the same record can appear in multiple columns when their
         filters overlap. Each list re-renders with the new status
         tag without waiting for a server refresh. */
      for (const id of Object.keys(inboxState.sentryItemsByInstance)) {
        const list = inboxState.sentryItemsByInstance[id];
        const idx = list.findIndex((i: SentryIssue) => i.id === issueId);
        if (idx >= 0) {
          inboxState.sentryItemsByInstance[id] = [
            ...list.slice(0, idx),
            updated,
            ...list.slice(idx + 1)
          ];
        }
      }
      notify({ kind: 'success', title: `Marked ${label}d`, ttlMs: 1800 });
    } catch (e) {
      notifyError(e, { title: `Couldn't ${label}` });
    } finally {
      actionBusy = null;
    }
  }

  /** Stack-frame → built-in editor (M4 §2.5.5). Sentry's `abs_path`
   *  is the source-on-disk location at deploy time; on the user's
   *  machine the same source typically lives at the same absolute
   *  path (monorepo + checkouts). When `lineno` is present we
   *  compute the document offset by reading the file and counting
   *  newlines, then stash it in `editorCursors` so EditorView lands
   *  on that line on first paint. Errors are swallowed — the worst
   *  case is the file opens at the top, which is still useful. */
  async function openFrameInEditor(
    f: NonNullable<SentryEventDetail['exceptions'][number]['frames']>[number]
  ) {
    const path = f.abs_path ?? f.filename;
    if (!path) return;
    if (typeof f.lineno === 'number' && f.lineno > 0) {
      try {
        const contents = await invoke<string>('fs_read_file', { path });
        const lines = contents.split('\n');
        let offset = 0;
        for (let i = 0; i < f.lineno - 1 && i < lines.length; i++) {
          offset += lines[i].length + 1; /* +1 for the newline */
        }
        recordCursor(path, { from: offset, to: offset, scrollTop: 0 });
      } catch {
        /* file might not exist locally (different deploy machine) —
         * just open at the top below. */
      }
    }
    try {
      await openFileInEditor(path);
    } catch (e) {
      notifyError(e, { title: 'Could not open in editor' });
    }
  }

  /** `file:line` for a frame — the muted location half of the summary. */
  function frameLoc(f: NonNullable<SentryEventDetail['exceptions'][number]['frames']>[number]): string {
    const file = f.filename ?? f.abs_path ?? '?';
    const line = f.lineno != null ? `:${f.lineno}` : '';
    return `${file}${line}`;
  }

  /** Severity → tone token for the header dot (err/warn/info). */
  function levelTone(level: string): 'err' | 'warn' | 'info' {
    if (level === 'fatal' || level === 'error') return 'err';
    if (level === 'warning') return 'warn';
    return 'info';
  }
</script>

<div class="snd">
  <header class="snd-head">
    <button class="snd-back" onclick={onClose} aria-label="Close" title="Close">
      <svg class="i i-sm" viewBox="0 0 24 24"><path d="M18 6 6 18M6 6l12 12" /></svg>
    </button>
    {#if issue}<span class="snd-dot snd-dot--{levelTone(issue.level)}"></span>{/if}
    <span class="snd-ref mono">{issue?.short_id ?? issueId}</span>
    {#if issue}
      <span class="snd-meta">
        {issue.level}{#if issue.platform} · {issue.platform}{/if} · {issue.project_slug}{#if issue.status !== 'unresolved'} · {issue.status}{/if}
      </span>
    {/if}
    <div class="snd-spring"></div>
    {#if issue}
      {#if issue.status === 'resolved'}
        <button class="snd-btn" disabled={actionBusy !== null} onclick={() => void setStatus('unresolved', 'unresolve')}>
          {actionBusy === 'unresolve' ? 'Re-opening…' : 'Re-open'}
        </button>
      {:else}
        <button class="snd-btn snd-btn--primary" disabled={actionBusy !== null} onclick={() => void setStatus('resolved', 'resolve')}>
          {actionBusy === 'resolve' ? 'Resolving…' : 'Resolve'}
        </button>
        <button class="snd-btn" disabled={actionBusy !== null || issue.status === 'ignored'} onclick={() => void setStatus('ignored', 'ignore')}>
          {actionBusy === 'ignore' ? 'Ignoring…' : 'Ignore'}
        </button>
      {/if}
    {/if}
    <button
      class="snd-iconbtn"
      onclick={() => { void loadIssue(); void loadEvent(); }}
      disabled={issueLoading || eventLoading}
      title="Refresh issue + latest event"
      aria-label="Refresh"
    >
      <svg class="i i-sm" class:snd-spin={issueLoading || eventLoading} viewBox="0 0 24 24">
        <path d="M21 12a9 9 0 0 1-9 9 9 9 0 0 1-8.5-6"/>
        <path d="M3 12a9 9 0 0 1 9-9 9 9 0 0 1 8.5 6"/>
        <polyline points="21 3 21 9 15 9"/>
        <polyline points="3 21 3 15 9 15"/>
      </svg>
    </button>
    <button class="snd-ghostbtn" onclick={() => issue?.permalink && onOpenBrowser(issue.permalink)} disabled={!issue?.permalink} title="Open on Sentry">
      Sentry
      <svg class="i i-sm" viewBox="0 0 24 24"><path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"/><path d="M15 3h6v6M10 14 21 3"/></svg>
    </button>
    {#if onSendToClaude}
      <button class="snd-claude-link" onclick={onSendToClaude} disabled={!issue} title="Send this issue to Claude">→ claude</button>
    {/if}
  </header>

  {#if issueLoading && !issue}
    <div class="snd-state">Loading issue…</div>
  {:else if issueError}
    <div class="snd-state snd-err">
      {issueError}
      <button class="snd-link" onclick={() => void loadIssue()}>Retry</button>
    </div>
  {:else if issue}
    <div class="snd-scroll">
      <div class="snd-doc">
        <!-- Exception title — mono 20/600 (spec §2.6: это exception-текст). -->
        <h1 class="snd-title mono">{issue.title}</h1>
        {#if issue.culprit}<div class="snd-culprit mono">{issue.culprit}</div>{/if}

        <!-- Stats row (numbers mono 17 + caps labels 10.5) + tag chips. -->
        <div class="snd-overview">
          <div class="snd-stats">
            <div class="snd-stat">
              <div class="snd-stat-v mono">{issue.count}</div>
              <div class="snd-stat-k">events</div>
            </div>
            <div class="snd-stat">
              <div class="snd-stat-v mono">{issue.user_count}</div>
              <div class="snd-stat-k">users</div>
            </div>
            <div class="snd-stat">
              <div class="snd-stat-v mono">{relativeTime(issue.first_seen, now)}</div>
              <div class="snd-stat-k">first seen</div>
            </div>
            <div class="snd-stat">
              <div class="snd-stat-v mono">{relativeTime(issue.last_seen, now)}</div>
              <div class="snd-stat-k">last seen</div>
            </div>
          </div>
          {#if event}
            <div class="snd-chips">
              {#if event.release}<span class="snd-chip mono">release {event.release}</span>{/if}
              {#if event.environment}<span class="snd-chip mono">env {event.environment}</span>{/if}
              {#if event.user_email || event.user_id}<span class="snd-chip mono">{event.user_email ?? event.user_id}</span>{/if}
              {#each event.tags.slice(0, 8) as [k, v] (k + v)}<span class="snd-chip mono">{k} {v}</span>{/each}
            </div>
          {/if}
        </div>

        <!-- Other events picker. Collapsed by default to keep the pane
             compact; expand to scan / pick a different occurrence. The
             agent's `mcp__app__open_sentry_event` calls land on the
             same `sentryFocusEventId` slot, so click-from-UI and
             click-from-chat funnel through one path. -->
        {#if events.length > 1 || eventsLoading || eventsError}
          <section class="snd-sec">
            <div class="snd-sec-head">
              <span class="snd-sec-label">Other events</span>
              {#if events.length > 0}
                <span class="snd-sec-sub mono">{events.length}{events.length === 30 ? '+' : ''}</span>
              {/if}
              <span class="hatch" aria-hidden="true"></span>
              <div class="snd-spring"></div>
              <button
                class="snd-iconbtn snd-iconbtn--sm"
                onclick={() => void loadEvents()}
                disabled={eventsLoading}
                title="Reload events"
                aria-label="Reload events"
              >
                <svg class="i i-sm" class:snd-spin={eventsLoading} viewBox="0 0 24 24">
                  <path d="M21 12a9 9 0 0 1-9 9 9 9 0 0 1-8.5-6"/>
                  <path d="M3 12a9 9 0 0 1 9-9 9 9 0 0 1 8.5 6"/>
                  <polyline points="21 3 21 9 15 9"/>
                  <polyline points="3 21 3 15 9 15"/>
                </svg>
              </button>
              <button
                class="snd-iconbtn snd-iconbtn--sm"
                onclick={() => (eventsExpanded = !eventsExpanded)}
                aria-expanded={eventsExpanded}
                title={eventsExpanded ? 'Hide list' : 'Show list'}
                aria-label={eventsExpanded ? 'Hide list' : 'Show list'}
              >
                <svg class="i i-sm snd-chevron" class:snd-chevron--open={eventsExpanded} viewBox="0 0 24 24">
                  <path d="m9 18 6-6-6-6"/>
                </svg>
              </button>
            </div>
            {#if eventsError}
              <div class="snd-state snd-err">{eventsError}</div>
            {:else if eventsExpanded}
              <div class="snd-events">
                {#each events as ev (ev.event_id)}
                  {@const active = (inboxState.sentryFocusEventId ?? '') === ev.event_id
                    || (!inboxState.sentryFocusEventId && event?.event_id === ev.event_id)}
                  <button
                    class="snd-event-row"
                    class:snd-event-row--active={active}
                    onclick={() => openSentryFocus(issueId, ev.event_id)}
                    title={ev.event_id}
                  >
                    <span class="snd-event-id mono">{ev.event_id.slice(0, 8)}</span>
                    <span class="snd-event-when mono">{relativeTime(ev.date_created, now)}</span>
                    <span class="snd-event-msg">{ev.exception_summary ?? ev.message ?? ''}</span>
                    {#if ev.platform}<span class="snd-event-tag mono">{ev.platform}</span>{/if}
                  </button>
                {/each}
                {#if inboxState.sentryFocusEventId}
                  <button class="snd-link snd-link--center" onclick={() => openSentryFocus(issueId, null)}>
                    ← Back to latest
                  </button>
                {/if}
              </div>
            {/if}
          </section>
        {/if}

        <!-- Stack trace — charcoal inset (spec §2.6). -->
        <section class="snd-sec">
          <div class="snd-sec-head">
            <span class="snd-sec-label">Stack trace</span>
            <span class="snd-sec-sub mono">
              {inboxState.sentryFocusEventId ? 'selected event' : 'latest event'}{#if event?.event_id} · {event.event_id.slice(0, 8)}{/if}
            </span>
            <span class="hatch" aria-hidden="true"></span>
            <div class="snd-spring"></div>
            <button
              class="snd-iconbtn snd-iconbtn--sm"
              onclick={() => void loadEvent()}
              disabled={eventLoading}
              title="Reload event"
              aria-label="Reload event"
            >
              <svg class="i i-sm" class:snd-spin={eventLoading} viewBox="0 0 24 24">
                <path d="M21 12a9 9 0 0 1-9 9 9 9 0 0 1-8.5-6"/>
                <path d="M3 12a9 9 0 0 1 9-9 9 9 0 0 1 8.5 6"/>
                <polyline points="21 3 21 9 15 9"/>
                <polyline points="3 21 3 15 9 15"/>
              </svg>
            </button>
          </div>

          {#if eventLoading && !event}
            <div class="snd-state">Loading event…</div>
          {:else if eventError}
            <div class="snd-state snd-err">{eventError}</div>
          {:else if event}
            {#each event.exceptions as exc, idx (idx)}
              <div class="snd-trace">
                {#if exc.type || exc.value}
                  <div class="snd-trace-err mono">{#if exc.type}<span class="snd-trace-err-type">{exc.type}</span>{/if}{#if exc.value}: {exc.value}{/if}</div>
                {/if}
                {#if exc.frames.length > 0}
                  {#each exc.frames.slice().reverse() as f, fi (fi)}
                    <details class="snd-frame" class:in-app={f.in_app} open={f.in_app && fi < 3}>
                      <summary class="snd-frame-sum mono" title={frameLoc(f)}>
                        <span class="snd-frame-at">at</span>
                        <span class="snd-frame-fn">{f.function ?? '?'}</span>
                        <span class="snd-frame-loc">({frameLoc(f)})</span>
                        {#if f.in_app}<span class="snd-frame-inapp">in-app</span>{/if}
                        <span class="snd-frame-spring"></span>
                        {#if f.abs_path || f.filename}
                          <button
                            class="snd-frame-open"
                            onclick={(e) => { e.preventDefault(); e.stopPropagation(); void openFrameInEditor(f); }}
                            title="Open in Woom's editor at this line"
                            aria-label="Open in editor"
                          >→ open</button>
                        {/if}
                      </summary>
                      {#if f.context.length > 0}
                        <pre class="snd-src mono">{#each f.context as l (l.line)}<span class="snd-src-line" class:active={l.line === f.lineno}><span class="snd-src-num">{l.line}</span>{l.source}
</span>{/each}</pre>
                      {/if}
                    </details>
                  {/each}
                {/if}
              </div>
            {/each}

            {#if event.breadcrumbs_summary}
              <div class="snd-crumbs-head">
                <span class="snd-sec-label">Breadcrumbs</span>
                <span class="snd-sec-sub mono">recent</span>
                <span class="hatch" aria-hidden="true"></span>
              </div>
              <pre class="snd-crumbs mono">{event.breadcrumbs_summary}</pre>
            {/if}
          {/if}
        </section>

        <!-- Bottom hint card — surfaces the Claude handoff (spec §2.6). -->
        {#if onSendToClaude}
          <div class="snd-handoff">
            <p class="snd-handoff-text">Hand this issue to Claude with the full stack trace and breadcrumbs.</p>
            <button class="snd-handoff-claude" onclick={onSendToClaude} disabled={!issue} title="Send this issue to Claude">→ claude</button>
          </div>
        {/if}
      </div>
    </div>
  {/if}
</div>

<style>
  /* §2.6 Sentry detail (mockup 4g) — single scrolling document, no side
     panels. Fresh `.snd-` grammar: 52px header (Resolve / Ignore triage),
     centred document (max 800): exception title + stats + tag chips +
     charcoal stack-trace inset + breadcrumbs + Claude handoff card. */

  .snd { height: 100%; display: flex; flex-direction: column; min-height: 0; background: var(--bg-0); }

  /* Header ------------------------------------------------------------- */
  .snd-head {
    flex: none;
    display: flex; align-items: center; gap: 10px;
    height: 52px; padding: 0 24px;
    border-bottom: 1px solid var(--border-lo);
    background: var(--bg-0);
  }
  .snd-back {
    width: 28px; height: 28px; border-radius: 5px;
    display: inline-flex; align-items: center; justify-content: center;
    background: transparent; color: var(--text-1); border: none; cursor: pointer;
  }
  .snd-back:hover { background: var(--bg-2); color: var(--text-0); }
  .snd-dot { width: 7px; height: 7px; border-radius: 50%; background: var(--text-mute); flex: none; }
  .snd-dot--err { background: var(--err); }
  .snd-dot--warn { background: var(--warn); }
  .snd-dot--info { background: var(--text-mute); }
  .snd-ref { font-size: 12px; color: var(--text-1); font-weight: 600; }
  .snd-meta { font-size: 12px; color: var(--text-faint); }
  .snd-spring { flex: 1; }
  .snd-btn {
    display: inline-flex; align-items: center; gap: 6px;
    padding: 6px 12px; border-radius: 6px;
    background: transparent; color: var(--text-1);
    font-size: 12px; border: 1px solid var(--border-neutral-hi); cursor: pointer;
  }
  .snd-btn:hover:not(:disabled) { background: var(--bg-2); color: var(--text-0); }
  .snd-btn:disabled { opacity: 0.5; cursor: default; }
  /* Resolve — triage primary (spec §2.6: Resolve primary, Ignore ghost). */
  .snd-btn--primary {
    background: var(--accent); color: var(--accent-fg);
    border-color: transparent; font-weight: 600;
  }
  .snd-btn--primary:hover:not(:disabled) { background: var(--accent-bright); color: var(--accent-fg); }
  .snd-iconbtn {
    width: 30px; height: 28px; border-radius: 6px;
    display: inline-flex; align-items: center; justify-content: center;
    background: transparent; color: var(--text-2);
    border: 1px solid var(--border-neutral); cursor: pointer;
  }
  .snd-iconbtn:hover:not(:disabled) { background: var(--bg-2); color: var(--text-0); }
  .snd-iconbtn:disabled { opacity: 0.45; cursor: default; }
  .snd-iconbtn .i-sm { width: 14px; height: 14px; }
  .snd-iconbtn--sm { width: 24px; height: 24px; border-color: transparent; }
  .snd-iconbtn--sm .i-sm { width: 13px; height: 13px; }
  .snd-spin { animation: snd-spin 0.8s linear infinite; }
  @keyframes snd-spin { to { transform: rotate(360deg); } }
  .snd-chevron { transition: transform var(--dur-base) var(--ease-spring); }
  .snd-chevron--open { transform: rotate(90deg); }
  .snd-ghostbtn {
    display: inline-flex; align-items: center; gap: 6px;
    padding: 6px 12px; border-radius: 6px;
    background: transparent; color: var(--text-1);
    font-size: 12px; border: 1px solid var(--border-neutral-hi); cursor: pointer;
  }
  .snd-ghostbtn:hover:not(:disabled) { background: var(--bg-2); color: var(--text-0); }
  .snd-ghostbtn:disabled { opacity: 0.5; cursor: default; }
  .snd-ghostbtn .i-sm { width: 13px; height: 13px; }
  /* → claude — dotted hotspot link in the header (the primary handoff
     lives in the bottom card, per mockup 4g). */
  .snd-claude-link {
    background: transparent; border: none; cursor: pointer;
    font-size: 12px; color: var(--text-0); padding: 2px 0;
    border-bottom: 1px dotted color-mix(in srgb, var(--text-0) 30%, transparent);
  }
  .snd-claude-link:hover:not(:disabled) { color: var(--accent-bright); border-bottom-color: var(--accent-bright); }
  .snd-claude-link:disabled { opacity: 0.5; cursor: default; }

  .snd-state { padding: 40px; text-align: center; color: var(--text-2); }
  .snd-err { color: var(--error); }
  .snd-link {
    color: var(--accent-bright); margin-left: 6px; cursor: pointer;
    background: none; border: none; padding: 0; text-decoration: underline; font-size: 12px;
  }
  .snd-link:disabled { opacity: 0.5; cursor: default; }
  .snd-link--center { display: block; margin: 6px auto 0; text-align: center; font-size: 11px; }

  /* Document ----------------------------------------------------------- */
  .snd-scroll { flex: 1; min-height: 0; overflow-y: auto; }
  .snd-doc { max-width: 800px; margin: 0 auto; padding: 30px 40px 60px; }

  .snd-title {
    font-size: 20px; line-height: 1.3; font-weight: 600;
    color: var(--text-0); letter-spacing: -0.015em;
    margin: 0; overflow-wrap: anywhere;
  }
  .snd-culprit { font-size: 12.5px; color: var(--text-2); margin-top: 8px; overflow-wrap: anywhere; }

  /* Stats row + tag chips --------------------------------------------- */
  .snd-overview {
    display: flex; align-items: flex-start; justify-content: space-between;
    flex-wrap: wrap; gap: 16px 32px;
    margin-top: 22px;
    padding-bottom: 22px;
    border-bottom: 1px solid var(--border);
  }
  .snd-stats { display: flex; flex-wrap: wrap; gap: 28px; }
  .snd-stat { display: flex; flex-direction: column; gap: 3px; }
  .snd-stat-v { font-size: 17px; font-weight: 600; color: var(--text-0); line-height: 1; }
  .snd-stat-k {
    font-size: 10.5px; font-weight: 600; color: var(--text-mute);
    text-transform: uppercase; letter-spacing: 0.08em;
  }
  .snd-chips { display: flex; flex-wrap: wrap; gap: 6px; align-items: center; }
  .snd-chip {
    font-size: 11px; padding: 2px 8px; border-radius: 5px;
    background: var(--accent-soft); color: var(--text-1);
    overflow-wrap: anywhere;
  }

  /* Sections — caps label + hatch ornament. --------------------------- */
  .snd-sec { margin-top: 28px; }
  .snd-sec-head { display: flex; align-items: center; gap: 8px; margin-bottom: 12px; }
  .snd-sec-label {
    font-size: 10.5px; font-weight: 600;
    text-transform: uppercase; letter-spacing: 0.09em;
    color: var(--text-faint);
  }
  .snd-sec-sub { font-size: 11px; color: var(--text-mute); }

  /* Stack trace — charcoal inset (spec §2.6). ------------------------- */
  .snd-trace {
    background: var(--dark-0);
    border-radius: 10px;
    padding: 14px 16px;
    box-shadow: var(--shadow-1);
    font-size: 12px; line-height: 1.7;
    overflow: hidden;
  }
  .snd-trace + .snd-trace { margin-top: 10px; }
  .snd-trace-err { color: var(--term-err); overflow-wrap: anywhere; margin-bottom: 4px; }
  .snd-trace-err-type { font-weight: 600; }
  .snd-frame { border: 0; background: transparent; }
  .snd-frame-sum {
    list-style: none; cursor: pointer;
    display: flex; align-items: center; gap: 8px;
    padding: 2px 0; color: var(--dark-text);
    overflow-wrap: anywhere;
  }
  .snd-frame-sum::-webkit-details-marker { display: none; }
  .snd-frame:not(.in-app) .snd-frame-sum { color: var(--dark-text-2); }
  .snd-frame-at { color: var(--dark-mute); }
  .snd-frame-fn { color: var(--dark-text); }
  .snd-frame:not(.in-app) .snd-frame-fn { color: var(--dark-text-2); }
  .snd-frame-loc { color: var(--dark-text-2); }
  .snd-frame-inapp {
    font-size: 10.5px; color: var(--dark-mute);
    text-transform: lowercase; letter-spacing: 0.02em;
  }
  .snd-frame-spring { flex: 1; }
  .snd-frame-open {
    font-size: 10.5px; color: var(--dark-mute);
    padding: 1px 6px; border-radius: 3px; cursor: pointer;
    background: transparent; border: 1px solid color-mix(in srgb, var(--dark-text-2) 30%, transparent);
    opacity: 0; transition: opacity 100ms, color 100ms, border-color 100ms;
  }
  .snd-frame-sum:hover .snd-frame-open { opacity: 1; }
  .snd-frame-open:hover { color: var(--dark-text); border-color: color-mix(in srgb, var(--dark-text-2) 55%, transparent); }
  .snd-src {
    margin: 6px 0 8px; padding: 0;
    font-size: 12px; line-height: 1.7;
    color: var(--dark-text-2); white-space: pre; overflow-x: auto;
  }
  /* Highlight line — full-bleed to the inset padding via negative margins. */
  .snd-src-line { display: block; margin: 0 -16px; padding: 0 16px; }
  .snd-src-line.active { background: color-mix(in srgb, var(--term-err) 10%, transparent); }
  .snd-src-num {
    display: inline-block; min-width: 34px; margin-right: 12px;
    color: var(--dark-mute); text-align: right;
  }

  /* Breadcrumbs — mono block (data arrives pre-formatted as a summary
     string, so it renders as text rather than the structured grid). */
  .snd-crumbs-head { display: flex; align-items: center; gap: 8px; margin: 20px 0 10px; }
  .snd-crumbs {
    background: var(--bg-1); border: 1px solid var(--border-neutral);
    border-radius: 10px; padding: 12px 14px; margin: 0;
    font-size: 11.5px; line-height: 1.6; color: var(--text-1);
    white-space: pre-wrap; overflow-wrap: anywhere;
  }

  /* Other-events picker rows. ----------------------------------------- */
  .snd-events { display: flex; flex-direction: column; gap: 2px; }
  .snd-event-row {
    display: flex; align-items: center; gap: 10px;
    padding: 6px 10px; width: 100%; text-align: left; cursor: pointer;
    background: var(--bg-1); border: 1px solid var(--border-neutral);
    border-radius: 6px; color: var(--text-1); font-size: 12px;
    transition: background 100ms;
  }
  .snd-event-row:hover { background: var(--bg-2); color: var(--text-0); }
  .snd-event-row--active { background: var(--accent-soft); border-color: var(--border-hi); color: var(--text-0); }
  .snd-event-id { color: var(--text-2); font-size: 11px; min-width: 70px; }
  .snd-event-when { color: var(--text-mute); font-size: 11px; min-width: 70px; }
  .snd-event-msg {
    flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 11.5px;
  }
  .snd-event-tag {
    font-size: 10px; padding: 1px 6px; border-radius: 3px;
    background: var(--bg-2); color: var(--text-2); border: 1px solid var(--border-neutral);
  }

  /* Bottom hint card — bg-2, border, r10, shadow-1 (spec §2.6). ------- */
  .snd-handoff {
    display: flex; align-items: center; gap: 16px;
    margin-top: 28px; padding: 14px 16px;
    background: var(--bg-2); border: 1px solid var(--border-neutral);
    border-radius: 10px; box-shadow: var(--shadow-1);
  }
  .snd-handoff-text { flex: 1; margin: 0; font-size: 12.5px; line-height: 1.5; color: var(--text-1); }
  .snd-handoff-claude {
    flex: none; display: inline-flex; align-items: center;
    padding: 6px 14px; border-radius: 999px; cursor: pointer;
    background: var(--accent); color: var(--accent-fg);
    font-size: 12px; font-weight: 600; border: none;
    box-shadow: var(--shadow-pill);
  }
  .snd-handoff-claude:hover:not(:disabled) { background: var(--accent-bright); color: var(--accent-fg); }
  .snd-handoff-claude:disabled { opacity: 0.5; cursor: default; }
</style>
