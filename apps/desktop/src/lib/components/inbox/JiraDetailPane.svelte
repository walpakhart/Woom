<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import { jiraStatusClass, relativeTime, type JiraComment, type JiraDetail, type JiraUserSummary, type JiraWorklog } from '$lib/data';
  import { formatDuration, jiraStartedString, parseDuration } from '$lib/format';
  import Markdown from '$lib/components/ui/Markdown.svelte';

  interface Props {
    issueKey: string;
    now: number;
    onClose: () => void;
    onStatusChange?: () => void;
    /** Hand the focused ticket off to Claude / Cursor. Optional so
     *  any existing call site that doesn't wire them up still
     *  compiles — each header button is hidden when undefined. */
    onSendToClaude?: () => void;
    onSendToCursor?: () => void;
  }
  let { issueKey, now, onClose, onStatusChange, onSendToClaude, onSendToCursor }: Props = $props();

  let detail = $state<JiraDetail | null>(null);
  let loading = $state(false);
  let error = $state<string | null>(null);

  let editingSummary = $state(false);
  let summaryDraft = $state('');
  let editingDesc = $state(false);
  let descDraft = $state('');

  let saving = $state(false);
  let statusBusy = $state(false);

  let newComment = $state('');
  let addingComment = $state(false);

  let showTransitions = $state(false);

  // Assignee edit popover
  let showAssigneePicker = $state(false);
  let assigneeQuery = $state('');
  let assigneeResults = $state<JiraUserSummary[]>([]);
  let assigneeSearching = $state(false);
  let assigneeDebounce: ReturnType<typeof setTimeout> | null = null;

  // Priority edit popover
  let showPriorityPicker = $state(false);
  const PRIORITIES = ['Highest', 'High', 'Medium', 'Low', 'Lowest'] as const;

  // Labels edit
  let editingLabels = $state(false);
  let labelsDraft = $state('');

  // Worklogs — loaded lazily the first time the Time section renders, so
  // opening a ticket stays fast when the user doesn't care about hours.
  let worklogs = $state<JiraWorklog[]>([]);
  let worklogsLoading = $state(false);
  let worklogsLoaded = $state(false);
  let worklogsError = $state<string | null>(null);
  let newWorklogDuration = $state('');
  let newWorklogComment = $state('');
  let addingWorklog = $state(false);
  let deletingWorklogId = $state<string | null>(null);
  // Parsed preview of the duration input — lets the button/label echo "1h 30m"
  // (Jira's own shape) as the user types, so they can see how their input
  // will land before submitting.
  const parsedWorklogSeconds = $derived<number | null>(
    newWorklogDuration.trim() ? parseDuration(newWorklogDuration) : null
  );
  const totalWorklogSeconds = $derived(
    worklogs.reduce((sum, w) => sum + w.time_spent_seconds, 0)
  );

  async function load() {
    loading = true;
    error = null;
    try {
      detail = await invoke<JiraDetail>('jira_get_issue_detail', { key: issueKey });
    } catch (e) {
      error = typeof e === 'string' ? e : String(e);
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    void load();
    // eslint-disable-next-line @typescript-eslint/no-unused-expressions
    issueKey;
  });

  async function saveSummary() {
    if (!detail || !summaryDraft.trim() || summaryDraft === detail.summary) {
      editingSummary = false; return;
    }
    saving = true;
    try {
      await invoke('jira_update_issue', { key: issueKey, summary: summaryDraft.trim(), description: null });
      detail.summary = summaryDraft.trim();
      editingSummary = false;
      onStatusChange?.();
    } catch (e) {
      error = typeof e === 'string' ? e : String(e);
    } finally {
      saving = false;
    }
  }

  async function saveDesc() {
    if (!detail) return;
    saving = true;
    try {
      await invoke('jira_update_issue', { key: issueKey, summary: null, description: descDraft });
      detail.description = descDraft;
      editingDesc = false;
    } catch (e) {
      error = typeof e === 'string' ? e : String(e);
    } finally {
      saving = false;
    }
  }

  async function transitionTo(id: string, toStatus: string) {
    if (!detail) return;
    showTransitions = false;
    statusBusy = true;
    try {
      await invoke('jira_transition_issue', { key: issueKey, transitionId: id });
      // Reload to get fresh status + transitions list (they change per status).
      await load();
      onStatusChange?.();
      // As a fast optimistic update if load raced:
      if (detail) detail.status = toStatus;
    } catch (e) {
      error = typeof e === 'string' ? e : String(e);
    } finally {
      statusBusy = false;
    }
  }

  async function postComment() {
    const body = newComment.trim();
    if (!body || !detail) return;
    addingComment = true;
    try {
      const added = await invoke<JiraComment>('jira_add_comment', { key: issueKey, body });
      detail.comments = [...detail.comments, added];
      newComment = '';
    } catch (e) {
      error = typeof e === 'string' ? e : String(e);
    } finally {
      addingComment = false;
    }
  }

  function startEditSummary() {
    if (!detail) return;
    summaryDraft = detail.summary;
    editingSummary = true;
  }
  function startEditDesc() {
    if (!detail) return;
    descDraft = detail.description;
    editingDesc = true;
  }

  function scheduleAssigneeSearch() {
    if (assigneeDebounce) clearTimeout(assigneeDebounce);
    assigneeDebounce = setTimeout(async () => {
      assigneeSearching = true;
      try {
        assigneeResults = await invoke<JiraUserSummary[]>('jira_search_users', { query: assigneeQuery });
      } catch (e) {
        error = typeof e === 'string' ? e : String(e);
      } finally {
        assigneeSearching = false;
      }
    }, 220);
  }

  async function setAssignee(accountId: string | null) {
    if (!detail) return;
    showAssigneePicker = false;
    saving = true;
    try {
      await invoke('jira_set_assignee', { key: issueKey, accountId });
      await load();
      onStatusChange?.();
    } catch (e) {
      error = typeof e === 'string' ? e : String(e);
    } finally {
      saving = false;
    }
  }

  async function setPriority(priority: string) {
    if (!detail) return;
    showPriorityPicker = false;
    saving = true;
    try {
      await invoke('jira_set_priority', { key: issueKey, priority });
      detail.priority = priority;
    } catch (e) {
      error = typeof e === 'string' ? e : String(e);
    } finally {
      saving = false;
    }
  }

  function startEditLabels() {
    if (!detail) return;
    labelsDraft = detail.labels.join(', ');
    editingLabels = true;
  }

  async function saveLabels() {
    if (!detail) return;
    const labels = labelsDraft.split(',').map((s) => s.trim()).filter(Boolean);
    saving = true;
    try {
      await invoke('jira_set_labels', { key: issueKey, labels });
      detail.labels = labels;
      editingLabels = false;
    } catch (e) {
      error = typeof e === 'string' ? e : String(e);
    } finally {
      saving = false;
    }
  }

  // Fire a worklog reload whenever the pane opens a different ticket. Using a
  // plain effect keyed on `issueKey` keeps the async boundary out of `load()`
  // — that one already races comments/transitions and we don't want worklog
  // failures to block the main issue view.
  $effect(() => {
    // Reset per-issue worklog state whenever we switch tickets.
    issueKey; // dependency
    worklogs = [];
    worklogsLoaded = false;
    worklogsError = null;
    newWorklogDuration = '';
    newWorklogComment = '';
    void loadWorklogs();
  });

  async function loadWorklogs() {
    worklogsLoading = true;
    worklogsError = null;
    try {
      worklogs = await invoke<JiraWorklog[]>('jira_list_worklogs', { key: issueKey });
      worklogsLoaded = true;
    } catch (e) {
      worklogsError = typeof e === 'string' ? e : String(e);
    } finally {
      worklogsLoading = false;
    }
  }

  async function addWorklog() {
    const seconds = parsedWorklogSeconds;
    // Jira rejects worklogs under 60 seconds outright; surface that as an
    // inline error instead of a silent API 400.
    if (!seconds || seconds < 60) {
      worklogsError = 'Duration must be at least 1 minute. Try e.g. "1h 30m" or "45m".';
      return;
    }
    addingWorklog = true;
    worklogsError = null;
    try {
      const added = await invoke<JiraWorklog>('jira_add_worklog', {
        key: issueKey,
        timeSpentSeconds: seconds,
        started: jiraStartedString(new Date()),
        comment: newWorklogComment.trim() || null
      });
      worklogs = [...worklogs, added];
      newWorklogDuration = '';
      newWorklogComment = '';
    } catch (e) {
      worklogsError = typeof e === 'string' ? e : String(e);
    } finally {
      addingWorklog = false;
    }
  }

  async function deleteWorklog(id: string) {
    deletingWorklogId = id;
    worklogsError = null;
    try {
      await invoke('jira_delete_worklog', { key: issueKey, worklogId: id });
      worklogs = worklogs.filter((w) => w.id !== id);
    } catch (e) {
      worklogsError = typeof e === 'string' ? e : String(e);
    } finally {
      deletingWorklogId = null;
    }
  }
</script>

<div class="jdp">
  <header class="jdp-head">
    <button class="jdp-back" onclick={onClose} aria-label="Close" title="Close">
      <svg class="i i-sm" viewBox="0 0 24 24"><path d="M18 6 6 18M6 6l12 12" /></svg>
    </button>
    <span class="jdp-key mono">{issueKey}</span>
    {#if detail}
      <span class="mini-tag {jiraStatusClass(detail.status_category)}">{detail.status.toLowerCase()}</span>
      <span class="jdp-kind">{detail.issue_type.toLowerCase()}</span>
      {#if detail.priority}<span class="jdp-prio">· {detail.priority.toLowerCase()}</span>{/if}
    {/if}
    <div style="flex:1"></div>
    <button
      class="jdp-btn jdp-btn--icon"
      onclick={() => void load()}
      disabled={loading}
      title="Refresh issue"
      aria-label="Refresh"
    >
      <svg class="i i-sm" class:jdp-spin={loading} viewBox="0 0 24 24">
        <path d="M21 12a9 9 0 0 1-9 9 9 9 0 0 1-8.5-6"/>
        <path d="M3 12a9 9 0 0 1 9-9 9 9 0 0 1 8.5 6"/>
        <polyline points="21 3 21 9 15 9"/>
        <polyline points="3 21 3 15 9 15"/>
      </svg>
    </button>
    {#if onSendToClaude}
      <button class="jdp-btn jdp-btn--claude" onclick={onSendToClaude} disabled={!detail} title="Send this ticket to Claude">
        <svg class="i i-sm" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M22 2 11 13"/><path d="m22 2-7 20-4-9-9-4 20-7z"/></svg>
        Send to Claude
      </button>
    {/if}
    {#if onSendToCursor}
      <button class="jdp-btn jdp-btn--cursor" onclick={onSendToCursor} disabled={!detail} title="Send this ticket to Cursor">
        <svg class="i i-sm" viewBox="0 0 24 24" fill="currentColor"><path d="M3 3l8 18 2-8 8-2z"/></svg>
        Send to Cursor
      </button>
    {/if}
    <button class="jdp-btn" onclick={() => detail && openUrl(detail.url)} disabled={!detail} title="Open on Jira">
      <svg class="i i-sm" viewBox="0 0 24 24"><path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6" /><path d="M15 3h6v6M10 14 21 3" /></svg>
      Open on Jira
    </button>
  </header>

  {#if loading && !detail}
    <div class="jdp-state">Loading issue…</div>
  {:else if error}
    <div class="jdp-state jdp-err">{error} <button class="jdp-link" onclick={load}>Retry</button></div>
  {:else if detail}
    <div class="jdp-body">
      <!-- Summary (editable) -->
      <section class="jdp-section">
        {#if editingSummary}
          <input
            class="jdp-summary-input"
            bind:value={summaryDraft}
            onkeydown={(e) => { if (e.key === 'Enter') void saveSummary(); if (e.key === 'Escape') editingSummary = false; }}
            disabled={saving}
          />
          <div class="jdp-save-row">
            <button class="jdp-btn jdp-btn--primary" onclick={saveSummary} disabled={saving || !summaryDraft.trim()}>Save</button>
            <button class="jdp-link" onclick={() => (editingSummary = false)}>Cancel</button>
          </div>
        {:else}
          <button class="jdp-summary" onclick={startEditSummary} title="Click to edit">
            <h1 class="jdp-summary-text">{detail.summary}</h1>
            <svg class="i i-sm jdp-edit-icon" viewBox="0 0 24 24"><path d="M12 20h9M16.5 3.5a2.12 2.12 0 0 1 3 3L7 19l-4 1 1-4L16.5 3.5z" /></svg>
          </button>
        {/if}
      </section>

      <!-- Meta row -->
      <section class="jdp-meta-grid">
        <div class="jdp-meta jdp-meta--editable">
          <div class="jdp-meta-label">Assignee</div>
          <button class="jdp-meta-val jdp-edit-target" onclick={() => { showAssigneePicker = !showAssigneePicker; showPriorityPicker = false; if (showAssigneePicker && !assigneeResults.length) scheduleAssigneeSearch(); }} disabled={saving}>
            {#if detail.assignee}
              {#if detail.assignee.avatar_url}
                <img class="jdp-avatar" src={detail.assignee.avatar_url} alt={detail.assignee.display_name} />
              {/if}
              <span>{detail.assignee.display_name}</span>
            {:else}
              <span class="jdp-none">unassigned</span>
            {/if}
            <svg class="i i-sm jdp-edit-caret" viewBox="0 0 24 24"><path d="M6 9l6 6 6-6" /></svg>
          </button>
          {#if showAssigneePicker}
            <div class="jdp-popover">
              <input
                class="jdp-popover-input mono"
                placeholder="Search users…"
                bind:value={assigneeQuery}
                oninput={scheduleAssigneeSearch}
                {@attach (node: HTMLInputElement) => node.focus()}
              />
              <button class="jdp-popover-item" onclick={() => setAssignee(null)}>
                <span class="jdp-none">Unassigned</span>
              </button>
              {#if assigneeSearching}
                <div class="jdp-popover-state">Searching…</div>
              {:else}
                {#each assigneeResults as u (u.account_id)}
                  <button class="jdp-popover-item" onclick={() => setAssignee(u.account_id)}>
                    <img class="jdp-avatar" src={u.avatar_url} alt={u.display_name} />
                    <span>{u.display_name}</span>
                    {#if u.email_address}<span class="jdp-popover-sub mono">{u.email_address}</span>{/if}
                  </button>
                {/each}
              {/if}
            </div>
          {/if}
        </div>
        <div class="jdp-meta jdp-meta--editable">
          <div class="jdp-meta-label">Priority</div>
          <button class="jdp-meta-val jdp-edit-target" onclick={() => { showPriorityPicker = !showPriorityPicker; showAssigneePicker = false; }} disabled={saving}>
            <span>{detail.priority ?? 'None'}</span>
            <svg class="i i-sm jdp-edit-caret" viewBox="0 0 24 24"><path d="M6 9l6 6 6-6" /></svg>
          </button>
          {#if showPriorityPicker}
            <div class="jdp-popover jdp-popover--narrow">
              {#each PRIORITIES as p (p)}
                <button class="jdp-popover-item" onclick={() => setPriority(p)} class:active={detail.priority === p}>
                  <span>{p}</span>
                </button>
              {/each}
            </div>
          {/if}
        </div>
        <div class="jdp-meta">
          <div class="jdp-meta-label">Reporter</div>
          <div class="jdp-meta-val">
            {#if detail.reporter}
              {#if detail.reporter.avatar_url}
                <img class="jdp-avatar" src={detail.reporter.avatar_url} alt={detail.reporter.display_name} />
              {/if}
              <span>{detail.reporter.display_name}</span>
            {:else}
              <span class="jdp-none">—</span>
            {/if}
          </div>
        </div>
        <div class="jdp-meta">
          <div class="jdp-meta-label">Updated</div>
          <div class="jdp-meta-val">{relativeTime(detail.updated, now)} ago</div>
        </div>
        <div class="jdp-meta jdp-meta--full">
          <div class="jdp-section-head">
            <span class="jdp-meta-label">Labels</span>
            {#if !editingLabels}
              <button class="jdp-link" onclick={startEditLabels}>Edit</button>
            {/if}
          </div>
          {#if editingLabels}
            <input
              class="jdp-desc-input"
              placeholder="comma-separated labels"
              bind:value={labelsDraft}
              disabled={saving}
              onkeydown={(e) => { if (e.key === 'Enter') void saveLabels(); if (e.key === 'Escape') editingLabels = false; }}
            />
            <div class="jdp-save-row">
              <button class="jdp-btn jdp-btn--primary" onclick={saveLabels} disabled={saving}>Save</button>
              <button class="jdp-link" onclick={() => (editingLabels = false)}>Cancel</button>
            </div>
          {:else if detail.labels.length}
            <div class="jdp-meta-val jdp-labels">
              {#each detail.labels as l (l)}<span class="jdp-label mono">{l}</span>{/each}
            </div>
          {:else}
            <span class="jdp-none">no labels</span>
          {/if}
        </div>
      </section>

      <!-- Status transitions -->
      <section class="jdp-section">
        <div class="jdp-meta-label">Status</div>
        <div class="jdp-transition-row">
          <!-- Current status is always shown — the Move-to button only
               transitions to NEW states, so without this tag the user has
               to scroll back up to the header to see where the ticket is. -->
          <span class="jdp-status-current mini-tag {jiraStatusClass(detail.status_category)}">
            {detail.status.toLowerCase()}
          </span>
          {#if detail.transitions.length}
            <button class="jdp-btn" onclick={() => (showTransitions = !showTransitions)} disabled={statusBusy}>
              Move to…
              <svg class="i i-sm" viewBox="0 0 24 24"><path d="M6 9l6 6 6-6" /></svg>
            </button>
            {#if showTransitions}
              <div class="jdp-transitions">
                {#each detail.transitions as t (t.id)}
                  <button class="jdp-transition" onclick={() => transitionTo(t.id, t.to_status)} disabled={statusBusy}>
                    <span class="jdp-transition-name">{t.name}</span>
                    {#if t.to_status}<span class="mini-tag {jiraStatusClass(t.to_status_category)}">{t.to_status.toLowerCase()}</span>{/if}
                  </button>
                {/each}
              </div>
            {/if}
          {:else}
            <span class="jdp-meta-muted">no transitions available</span>
          {/if}
        </div>
      </section>

      <!-- Description -->
      <section class="jdp-section">
        <div class="jdp-section-head">
          <span class="jdp-meta-label">Description</span>
          {#if !editingDesc}
            <button class="jdp-link" onclick={startEditDesc}>Edit</button>
          {/if}
        </div>
        {#if editingDesc}
          <textarea class="jdp-desc-input" bind:value={descDraft} rows="12" disabled={saving}></textarea>
          <div class="jdp-save-row">
            <button class="jdp-btn jdp-btn--primary" onclick={saveDesc} disabled={saving}>Save</button>
            <button class="jdp-link" onclick={() => (editingDesc = false)}>Cancel</button>
          </div>
        {:else if detail.description}
          <div class="jdp-desc"><Markdown source={detail.description} /></div>
        {:else}
          <div class="jdp-none">No description. <button class="jdp-link" onclick={startEditDesc}>Add one</button></div>
        {/if}
      </section>

      <!-- Time — native Jira worklog. Tempo syncs these in/out by default,
           so logging here is the same thing you'd see in the Tempo timesheet
           at /plugins/servlet/ac/io.tempo.jira/tempo-app. -->
      <section class="jdp-section">
        <div class="jdp-section-head">
          <span class="jdp-meta-label">
            Time {#if worklogs.length}({formatDuration(totalWorklogSeconds)} logged){/if}
          </span>
          {#if worklogsLoaded}
            <button class="jdp-link" onclick={loadWorklogs} disabled={worklogsLoading} title="Refresh worklogs">
              Refresh
            </button>
          {/if}
        </div>

        {#if worklogsLoading && !worklogsLoaded}
          <div class="jdp-none">Loading worklogs…</div>
        {:else if worklogs.length === 0 && worklogsLoaded}
          <div class="jdp-none">No time logged yet.</div>
        {:else}
          <div class="jdp-worklogs">
            {#each worklogs as w (w.id)}
              <div class="jdp-worklog">
                <div class="jdp-worklog-head">
                  {#if w.author?.avatar_url}
                    <img class="jdp-avatar" src={w.author.avatar_url} alt={w.author.display_name} />
                  {/if}
                  <span class="jdp-worklog-author">{w.author?.display_name ?? 'Unknown'}</span>
                  <span class="jdp-worklog-dur mono">{w.time_spent}</span>
                  <span class="jdp-worklog-time mono">{relativeTime(w.started, now)} ago</span>
                  <button
                    class="jdp-worklog-del"
                    onclick={() => deleteWorklog(w.id)}
                    disabled={deletingWorklogId === w.id}
                    title="Delete worklog (only your own)"
                    aria-label="Delete worklog"
                  >
                    <svg class="i i-sm" viewBox="0 0 24 24"><path d="M3 6h18M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2M19 6l-1 14a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2L5 6"/></svg>
                  </button>
                </div>
                {#if w.comment.trim()}
                  <div class="jdp-worklog-body"><Markdown source={w.comment} /></div>
                {/if}
              </div>
            {/each}
          </div>
        {/if}

        <div class="jdp-log-time">
          <div class="jdp-log-row">
            <input
              class="jdp-log-dur mono"
              type="text"
              placeholder="1h 30m"
              bind:value={newWorklogDuration}
              disabled={addingWorklog}
              onkeydown={(e) => { if (e.key === 'Enter') void addWorklog(); }}
              aria-label="Duration"
            />
            <input
              class="jdp-log-note"
              type="text"
              placeholder="What did you work on? (optional)"
              bind:value={newWorklogComment}
              disabled={addingWorklog}
              onkeydown={(e) => { if (e.key === 'Enter') void addWorklog(); }}
              aria-label="Worklog comment"
            />
            <button
              class="jdp-btn jdp-btn--primary"
              onclick={addWorklog}
              disabled={addingWorklog || !parsedWorklogSeconds || parsedWorklogSeconds < 60}
            >
              {#if addingWorklog}
                Logging…
              {:else if parsedWorklogSeconds && parsedWorklogSeconds >= 60}
                Log {formatDuration(parsedWorklogSeconds)}
              {:else}
                Log time
              {/if}
            </button>
          </div>
          {#if worklogsError}
            <div class="jdp-log-err">{worklogsError}</div>
          {/if}
          <div class="jdp-log-hint">
            Format: <span class="mono">1h 30m</span>, <span class="mono">45m</span>, <span class="mono">2h</span>, <span class="mono">1.5h</span>, <span class="mono">1d 2h</span>. Jira: 1d = 8h, 1w = 5d.
          </div>
        </div>
      </section>

      <!-- Comments -->
      <section class="jdp-section">
        <div class="jdp-section-head">
          <span class="jdp-meta-label">Comments ({detail.comments.length})</span>
        </div>
        <div class="jdp-comments">
          {#each detail.comments as c (c.id)}
            <div class="jdp-comment">
              <div class="jdp-comment-head">
                {#if c.author?.avatar_url}
                  <img class="jdp-avatar" src={c.author.avatar_url} alt={c.author.display_name} />
                {/if}
                <span class="jdp-comment-author">{c.author?.display_name ?? 'Unknown'}</span>
                <span class="jdp-comment-time mono">{relativeTime(c.created, now)} ago</span>
              </div>
              <div class="jdp-comment-body"><Markdown source={c.body} /></div>
            </div>
          {/each}
          {#if detail.comments.length === 0}
            <div class="jdp-none">No comments yet.</div>
          {/if}
        </div>
        <div class="jdp-add-comment">
          <textarea
            class="jdp-desc-input"
            placeholder="Add a comment (⌘↵ to send)"
            bind:value={newComment}
            rows="3"
            disabled={addingComment}
            onkeydown={(e) => { if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') void postComment(); }}
          ></textarea>
          <div class="jdp-save-row">
            <button class="jdp-btn jdp-btn--primary" onclick={postComment} disabled={addingComment || !newComment.trim()}>
              {addingComment ? 'Posting…' : 'Comment'}
            </button>
          </div>
        </div>
      </section>
    </div>
  {/if}
</div>

<style>
  .jdp { height: 100%; display: flex; flex-direction: column; min-height: 0; background: var(--bg-0); }
  .jdp-head {
    display: flex; align-items: center; gap: 10px;
    padding: 12px 20px;
    border-bottom: 1px solid var(--border-neutral);
    background: var(--bg-1);
  }
  .jdp-back {
    width: 28px; height: 28px; border-radius: 5px;
    display: inline-flex; align-items: center; justify-content: center;
    color: var(--text-1);
  }
  .jdp-back:hover { background: var(--bg-2); color: var(--text-0); }
  .jdp-key { font-size: 13px; color: var(--accent-bright); font-weight: 600; }
  .jdp-kind { font-size: 11px; color: var(--text-2); }
  .jdp-prio { font-size: 11px; color: var(--text-2); }
  .jdp-btn {
    display: inline-flex; align-items: center; gap: 6px;
    padding: 6px 12px; border-radius: 6px;
    background: var(--bg-2); color: var(--text-1);
    font-size: 12px; border: 1px solid var(--border-neutral-hi);
  }
  .jdp-btn:hover:not(:disabled) { background: var(--bg-3); color: var(--text-0); }
  .jdp-btn:disabled { opacity: 0.5; cursor: default; }
  .jdp-btn--icon { padding: 6px; }
  .jdp-btn--icon .i-sm { width: 14px; height: 14px; }
  .jdp-spin { animation: jdp-spin 0.8s linear infinite; }
  @keyframes jdp-spin { to { transform: rotate(360deg); } }
  .jdp-btn--primary {
    background: var(--accent); color: var(--accent-fg);
    border-color: transparent; font-weight: 600;
  }
  .jdp-btn--primary:hover:not(:disabled) { background: var(--accent-bright); color: var(--accent-fg); }
  /* Send-to-Claude — brand-tinted ghost so the handoff stands apart
     from the Jira-native actions (Refresh / Open on Jira). */
  .jdp-btn--claude {
    color: var(--src-claude);
    background: color-mix(in srgb, var(--src-claude) 8%, transparent);
    border-color: color-mix(in srgb, var(--src-claude) 30%, transparent);
  }
  .jdp-btn--claude:hover:not(:disabled) {
    background: color-mix(in srgb, var(--src-claude) 18%, transparent);
    color: var(--accent-bright);
    border-color: color-mix(in srgb, var(--src-claude) 50%, transparent);
  }
  .jdp-btn--cursor {
    color: var(--src-cursor, var(--text-1));
    background: color-mix(in srgb, var(--src-cursor, var(--text-1)) 10%, transparent);
    border-color: color-mix(in srgb, var(--src-cursor, var(--text-1)) 32%, transparent);
  }
  .jdp-btn--cursor:hover:not(:disabled) {
    background: color-mix(in srgb, var(--src-cursor, var(--text-1)) 22%, transparent);
    color: var(--text-0);
    border-color: color-mix(in srgb, var(--src-cursor, var(--text-1)) 50%, transparent);
  }

  .jdp-state { padding: 40px; text-align: center; color: var(--text-2); }
  .jdp-err { color: var(--error); }

  .jdp-body { flex: 1; overflow-y: auto; padding: 20px 28px 60px; }
  .jdp-section { margin-bottom: 24px; }
  .jdp-section-head { display: flex; align-items: center; justify-content: space-between; margin-bottom: 6px; }

  .jdp-summary {
    display: flex; align-items: flex-start; gap: 10px;
    padding: 4px; border-radius: 6px;
    text-align: left; width: 100%;
    cursor: pointer;
    transition: background 100ms;
    color: inherit;
  }
  .jdp-summary:hover { background: var(--bg-1); }
  .jdp-summary-text {
    font-family: 'Geist', 'Inter', -apple-system, system-ui, sans-serif;
    font-size: 30px; font-weight: 600; color: var(--text-0);
    letter-spacing: -0.02em;
    line-height: 1.18; margin: 0;
  }
  .jdp-edit-icon { opacity: 0; color: var(--text-2); margin-top: 6px; flex-shrink: 0; transition: opacity 120ms; }
  .jdp-summary:hover .jdp-edit-icon { opacity: 0.8; }
  .jdp-summary-input {
    width: 100%;
    font-size: 22px; font-weight: 600; color: var(--text-0);
    padding: 6px 10px;
    background: var(--bg-1); border: 1px solid var(--border-hi);
    border-radius: 6px; font-family: inherit;
  }
  .jdp-summary-input:focus { outline: none; border-color: var(--accent); }
  .jdp-save-row { display: flex; gap: 8px; margin-top: 10px; align-items: center; }

  .jdp-meta-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(160px, 1fr));
    gap: 14px 24px;
    padding: 16px 0 18px;
    border-top: 1px solid var(--border);
    border-bottom: 1px solid var(--border);
  }
  .jdp-meta { display: flex; flex-direction: column; gap: 4px; }
  .jdp-meta--full { grid-column: 1 / -1; }
  .jdp-meta-label {
    font-size: 9.5px; font-weight: 700;
    color: var(--text-mute);
    text-transform: uppercase; letter-spacing: 0.10em;
  }
  .jdp-meta-val { display: inline-flex; align-items: center; gap: 6px; font-size: 13px; color: var(--text-0); }
  .jdp-avatar { width: 20px; height: 20px; border-radius: 50%; }
  .jdp-none { color: var(--text-mute);  font-size: 12.5px; }
  .jdp-labels { flex-wrap: wrap; }
  .jdp-label {
    font-size: 11px; padding: 2px 7px;
    border-radius: 4px;
    background: var(--bg-2); color: var(--text-1);
    border: 1px solid var(--border-neutral);
  }

  .jdp-meta--editable { position: relative; }
  .jdp-edit-target {
    display: inline-flex; align-items: center; gap: 6px;
    font-size: 13px; color: var(--text-0);
    padding: 2px 4px;
    border-radius: 4px;
    text-align: left;
    transition: background 100ms;
  }
  .jdp-edit-target:hover:not(:disabled) { background: var(--bg-1); }
  .jdp-edit-target:disabled { opacity: 0.5; cursor: default; }
  .jdp-edit-caret { color: var(--text-2); opacity: 0.6; margin-left: 4px; }
  .jdp-popover {
    position: absolute; top: calc(100% + 4px); left: 0;
    min-width: 300px; max-width: 360px;
    max-height: 300px; overflow-y: auto;
    background: var(--bg-2);
    border: 1px solid var(--border-hi);
    border-radius: 8px;
    z-index: 10;
    box-shadow: var(--shadow-2);
    padding: 4px;
    display: flex; flex-direction: column; gap: 2px;
  }
  .jdp-popover--narrow { min-width: 180px; }
  .jdp-popover-input {
    width: 100%; padding: 6px 10px;
    background: var(--bg-0); border: 1px solid var(--border-neutral-hi);
    border-radius: 5px; color: var(--text-0); font-size: 12px;
    margin-bottom: 2px;
  }
  .jdp-popover-input:focus { outline: none; border-color: var(--border-hi2); }
  .jdp-popover-item {
    display: flex; align-items: center; gap: 8px;
    padding: 6px 10px; border-radius: 5px;
    font-size: 12.5px; color: var(--text-1);
    text-align: left;
  }
  .jdp-popover-item:hover { background: var(--bg-3); color: var(--text-0); }
  .jdp-popover-item.active { background: var(--accent-soft); color: var(--accent-bright); }
  .jdp-popover-sub { margin-left: auto; font-size: 10.5px; color: var(--text-mute); }
  .jdp-popover-state { padding: 8px 10px; font-size: 11.5px; color: var(--text-2);  }

  .jdp-transition-row {
    position: relative;
    display: flex; align-items: center; gap: 10px; flex-wrap: wrap;
  }
  .jdp-status-current {
    /* Slightly chunkier than the inline transition pills so the current
       status reads as "the anchor" rather than one option among many. */
    padding: 4px 10px;
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    font-weight: 600;
  }
  .jdp-meta-muted { color: var(--text-mute); font-size: 12px; }
  .jdp-transitions {
    position: absolute; top: 100%; left: 0;
    margin-top: 4px;
    min-width: 280px;
    background: var(--bg-2);
    border: 1px solid var(--border-hi);
    border-radius: 8px;
    z-index: 10;
    box-shadow: var(--shadow-2);
    padding: 4px;
  }
  .jdp-transition {
    display: flex; align-items: center; justify-content: space-between;
    gap: 8px;
    width: 100%; padding: 6px 10px;
    border-radius: 5px;
    font-size: 12px; color: var(--text-1);
    text-align: left;
  }
  .jdp-transition:hover:not(:disabled) { background: var(--bg-3); color: var(--text-0); }
  .jdp-transition-name { font-weight: 500; }

  .jdp-desc-input {
    width: 100%; padding: 10px 14px;
    font-family: inherit; font-size: 13px;
    background: var(--bg-1); color: var(--text-0);
    border: 1px solid var(--border-neutral-hi); border-radius: 8px;
    line-height: 1.55; resize: vertical;
  }
  .jdp-desc-input:focus { outline: none; border-color: var(--border-hi2); }
  .jdp-desc { padding: 2px; color: var(--text-0); font-size: 13.5px; line-height: 1.6; }

  .jdp-link { color: var(--accent-bright); font-size: 12px; text-decoration: none; }
  .jdp-link:hover { text-decoration: underline; }

  .jdp-comments { display: flex; flex-direction: column; gap: 14px; margin-bottom: 20px; }
  .jdp-comment {
    background: var(--bg-2);
    border: 1px solid var(--border);
    border-radius: 11px;
    padding: 12px 14px;
  }
  .jdp-comment-head .jdp-avatar {
    width: 22px; height: 22px;
    border-radius: 50%;
    background: var(--bg-3);
    display: grid; place-items: center;
    font-size: 10.5px; font-weight: 600;
    color: var(--text-1);
  }
  .jdp-comment-head { display: flex; align-items: center; gap: 8px; margin-bottom: 6px; }
  .jdp-comment-author { font-size: 12.5px; color: var(--text-0); font-weight: 500; }
  .jdp-comment-time { font-size: 11px; color: var(--text-mute); }
  .jdp-comment-body { font-size: 13px; color: var(--text-1); padding-left: 2px; }
  .jdp-add-comment { border-top: 1px solid var(--border-neutral); padding-top: 14px; }

  /* Worklog — list of native Jira time entries + inline "Log time" form.
     Same card aesthetic as .jdp-comment so the two sections feel like
     siblings, but with a dedicated mono duration chip on the right. */
  .jdp-worklogs { display: flex; flex-direction: column; gap: 8px; margin-bottom: 14px; }
  .jdp-worklog {
    background: var(--bg-1);
    border: 1px solid var(--border-neutral);
    border-radius: 8px;
    padding: 10px 12px;
  }
  .jdp-worklog-head { display: flex; align-items: center; gap: 8px; }
  .jdp-worklog-author { font-size: 12.5px; color: var(--text-0); font-weight: 500; }
  .jdp-worklog-dur {
    font-size: 11px; font-weight: 600;
    padding: 2px 7px; border-radius: 4px;
    color: var(--accent-bright);
    background: var(--accent-soft);
    border: 1px solid rgba(204, 120, 92, 0.22);
  }
  .jdp-worklog-time { margin-left: auto; font-size: 11px; color: var(--text-mute); }
  .jdp-worklog-del {
    width: 22px; height: 22px; border-radius: 4px;
    display: inline-flex; align-items: center; justify-content: center;
    color: var(--text-mute); background: transparent;
    opacity: 0; transition: all 120ms;
  }
  .jdp-worklog:hover .jdp-worklog-del { opacity: 1; }
  .jdp-worklog-del:hover:not(:disabled) { color: var(--error); background: var(--bg-3); }
  .jdp-worklog-del:disabled { opacity: 0.3; cursor: default; }
  .jdp-worklog-body {
    margin-top: 6px;
    font-size: 12.5px; color: var(--text-1); line-height: 1.5;
    padding-left: 2px;
  }

  .jdp-log-time {
    border-top: 1px solid var(--border-neutral);
    padding-top: 12px;
    display: flex; flex-direction: column; gap: 8px;
  }
  .jdp-log-row { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; }
  .jdp-log-dur {
    width: 110px; padding: 7px 10px;
    background: var(--bg-0); border: 1px solid var(--border-neutral-hi);
    border-radius: 6px; color: var(--text-0);
    font-size: 12.5px;
    font-feature-settings: 'zero';
    text-align: center;
  }
  .jdp-log-dur:focus { outline: none; border-color: var(--accent); }
  .jdp-log-note {
    flex: 1; min-width: 180px;
    padding: 7px 10px;
    background: var(--bg-0); border: 1px solid var(--border-neutral-hi);
    border-radius: 6px; color: var(--text-0);
    font-size: 12.5px; font-family: inherit;
  }
  .jdp-log-note:focus { outline: none; border-color: var(--accent); }
  .jdp-log-err { font-size: 11.5px; color: var(--error); }
  .jdp-log-hint { font-size: 10.5px; color: var(--text-mute); }
</style>
