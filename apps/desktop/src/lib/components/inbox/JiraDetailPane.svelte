<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import { relativeTime, type JiraComment, type JiraDetail, type JiraUserSummary, type JiraWorklog } from '$lib/data';
  import { formatDuration, jiraStartedString, parseDuration } from '$lib/format';
  import Markdown from '$lib/components/ui/Markdown.svelte';

  interface Props {
    issueKey: string;
    now: number;
    onClose: () => void;
    onStatusChange?: () => void;
    /** Hand the focused ticket off to Claude. Optional so
     *  any existing call site that doesn't wire it up still
     *  compiles — the header button is hidden when undefined. */
    onSendToClaude?: () => void;
  }
  let { issueKey, now, onClose, onStatusChange, onSendToClaude }: Props = $props();

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

<div class="jrd">
  <header class="jrd-head">
    <button class="jrd-back" onclick={onClose} aria-label="Close" title="Close">
      <svg class="i i-sm" viewBox="0 0 24 24"><path d="M18 6 6 18M6 6l12 12" /></svg>
    </button>
    <span class="jrd-ref mono">{issueKey}</span>
    {#if detail}
      <span class="jrd-chip mono">{detail.status.toLowerCase()}</span>
      <span class="jrd-chip mono">{detail.issue_type.toLowerCase()}</span>
      {#if detail.priority}<span class="jrd-prio">· {detail.priority.toLowerCase()}</span>{/if}
    {/if}
    <div class="jrd-spring"></div>
    <button
      class="jrd-iconbtn"
      onclick={() => void load()}
      disabled={loading}
      title="Refresh issue"
      aria-label="Refresh"
    >
      <svg class="i i-sm" class:jrd-spin={loading} viewBox="0 0 24 24">
        <path d="M21 12a9 9 0 0 1-9 9 9 9 0 0 1-8.5-6"/>
        <path d="M3 12a9 9 0 0 1 9-9 9 9 0 0 1 8.5 6"/>
        <polyline points="21 3 21 9 15 9"/>
        <polyline points="3 21 3 15 9 15"/>
      </svg>
    </button>
    <button class="jrd-ghostbtn" onclick={() => detail && openUrl(detail.url)} disabled={!detail} title="Open on Jira">
      Open in Jira
      <svg class="i i-sm" viewBox="0 0 24 24"><path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6" /><path d="M15 3h6v6M10 14 21 3" /></svg>
    </button>
    {#if onSendToClaude}
      <button class="jrd-claudebtn" onclick={onSendToClaude} disabled={!detail} title="Send this ticket to Claude">→ claude</button>
    {/if}
  </header>

  {#if loading && !detail}
    <div class="jrd-state">Loading issue…</div>
  {:else if error}
    <div class="jrd-state jrd-err">{error} <button class="jrd-link" onclick={load}>Retry</button></div>
  {:else if detail}
    <div class="jrd-scroll">
      <div class="jrd-doc">
        <!-- Summary (editable) -->
        <section class="jrd-summary-sec">
          {#if editingSummary}
            <input
              class="jrd-summary-input"
              bind:value={summaryDraft}
              onkeydown={(e) => { if (e.key === 'Enter') void saveSummary(); if (e.key === 'Escape') editingSummary = false; }}
              disabled={saving}
            />
            <div class="jrd-save-row">
              <button class="jrd-btn jrd-btn--primary" onclick={saveSummary} disabled={saving || !summaryDraft.trim()}>Save</button>
              <button class="jrd-link" onclick={() => (editingSummary = false)}>Cancel</button>
            </div>
          {:else}
            <button class="jrd-summary" onclick={startEditSummary} title="Click to edit">
              <h1 class="jrd-title">{detail.summary}</h1>
              <svg class="i i-sm jrd-edit-icon" viewBox="0 0 24 24"><path d="M12 20h9M16.5 3.5a2.12 2.12 0 0 1 3 3L7 19l-4 1 1-4L16.5 3.5z" /></svg>
            </button>
          {/if}
        </section>

        <!-- Meta grid — caps-label + value, 16px avatars. -->
        <section class="jrd-meta-grid">
          <div class="jrd-meta jrd-meta--editable">
            <div class="jrd-meta-label">Assignee</div>
            <button class="jrd-meta-val jrd-edit-target" onclick={() => { showAssigneePicker = !showAssigneePicker; showPriorityPicker = false; if (showAssigneePicker && !assigneeResults.length) scheduleAssigneeSearch(); }} disabled={saving}>
              {#if detail.assignee}
                {#if detail.assignee.avatar_url}
                  <img class="jrd-avatar" src={detail.assignee.avatar_url} alt={detail.assignee.display_name} />
                {/if}
                <span>{detail.assignee.display_name}</span>
              {:else}
                <span class="jrd-none">unassigned</span>
              {/if}
              <svg class="i i-sm jrd-edit-caret" viewBox="0 0 24 24"><path d="M6 9l6 6 6-6" /></svg>
            </button>
            {#if showAssigneePicker}
              <div class="jrd-popover">
                <input
                  class="jrd-popover-input mono"
                  placeholder="Search users…"
                  bind:value={assigneeQuery}
                  oninput={scheduleAssigneeSearch}
                  {@attach (node: HTMLInputElement) => node.focus()}
                />
                <button class="jrd-popover-item" onclick={() => setAssignee(null)}>
                  <span class="jrd-none">Unassigned</span>
                </button>
                {#if assigneeSearching}
                  <div class="jrd-popover-state">Searching…</div>
                {:else}
                  {#each assigneeResults as u (u.account_id)}
                    <button class="jrd-popover-item" onclick={() => setAssignee(u.account_id)}>
                      <img class="jrd-avatar" src={u.avatar_url} alt={u.display_name} />
                      <span>{u.display_name}</span>
                      {#if u.email_address}<span class="jrd-popover-sub mono">{u.email_address}</span>{/if}
                    </button>
                  {/each}
                {/if}
              </div>
            {/if}
          </div>
          <div class="jrd-meta jrd-meta--editable">
            <div class="jrd-meta-label">Priority</div>
            <button class="jrd-meta-val jrd-edit-target" onclick={() => { showPriorityPicker = !showPriorityPicker; showAssigneePicker = false; }} disabled={saving}>
              <span>{detail.priority ?? 'None'}</span>
              <svg class="i i-sm jrd-edit-caret" viewBox="0 0 24 24"><path d="M6 9l6 6 6-6" /></svg>
            </button>
            {#if showPriorityPicker}
              <div class="jrd-popover jrd-popover--narrow">
                {#each PRIORITIES as p (p)}
                  <button class="jrd-popover-item" onclick={() => setPriority(p)} class:active={detail.priority === p}>
                    <span>{p}</span>
                  </button>
                {/each}
              </div>
            {/if}
          </div>
          <div class="jrd-meta">
            <div class="jrd-meta-label">Reporter</div>
            <div class="jrd-meta-val">
              {#if detail.reporter}
                {#if detail.reporter.avatar_url}
                  <img class="jrd-avatar" src={detail.reporter.avatar_url} alt={detail.reporter.display_name} />
                {/if}
                <span>{detail.reporter.display_name}</span>
              {:else}
                <span class="jrd-none">—</span>
              {/if}
            </div>
          </div>
          <div class="jrd-meta">
            <div class="jrd-meta-label">Updated</div>
            <div class="jrd-meta-val">{relativeTime(detail.updated, now)} ago</div>
          </div>
          <div class="jrd-meta jrd-meta--full">
            <div class="jrd-sec-head">
              <span class="jrd-meta-label">Labels</span>
              <div class="jrd-spring"></div>
              {#if !editingLabels}
                <button class="jrd-link" onclick={startEditLabels}>Edit</button>
              {/if}
            </div>
            {#if editingLabels}
              <input
                class="jrd-input"
                placeholder="comma-separated labels"
                bind:value={labelsDraft}
                disabled={saving}
                onkeydown={(e) => { if (e.key === 'Enter') void saveLabels(); if (e.key === 'Escape') editingLabels = false; }}
              />
              <div class="jrd-save-row">
                <button class="jrd-btn jrd-btn--primary" onclick={saveLabels} disabled={saving}>Save</button>
                <button class="jrd-link" onclick={() => (editingLabels = false)}>Cancel</button>
              </div>
            {:else if detail.labels.length}
              <div class="jrd-meta-val jrd-labels">
                {#each detail.labels as l, _i (l + '|' + _i)}<span class="jrd-label mono">{l}</span>{/each}
              </div>
            {:else}
              <span class="jrd-none">no labels</span>
            {/if}
          </div>
        </section>

        <!-- Status transitions — current = inverse chip + 2px shadow,
             available transitions follow as ghost chips (arrow between). -->
        <section class="jrd-section">
          <div class="jrd-transition-row">
            <span class="jrd-status-current">{detail.status}</span>
            {#if detail.transitions.length}
              <span class="jrd-trans-arrow" aria-hidden="true">→</span>
              {#each detail.transitions as t (t.id)}
                <button
                  class="jrd-transition"
                  onclick={() => transitionTo(t.id, t.to_status)}
                  disabled={statusBusy}
                  title={t.to_status ? `→ ${t.to_status}` : t.name}
                >
                  {t.name}
                </button>
              {/each}
            {:else}
              <span class="jrd-meta-muted">no transitions available</span>
            {/if}
          </div>
        </section>

        <!-- Description -->
        <section class="jrd-section">
          <div class="jrd-sec-head">
            <span class="jrd-sec-label">Description</span>
            <span class="hatch" aria-hidden="true"></span>
            <div class="jrd-spring"></div>
            {#if !editingDesc}
              <button class="jrd-link" onclick={startEditDesc}>Edit</button>
            {/if}
          </div>
          {#if editingDesc}
            <textarea class="jrd-input" bind:value={descDraft} rows="12" disabled={saving}></textarea>
            <div class="jrd-save-row">
              <button class="jrd-btn jrd-btn--primary" onclick={saveDesc} disabled={saving}>Save</button>
              <button class="jrd-link" onclick={() => (editingDesc = false)}>Cancel</button>
            </div>
          {:else if detail.description}
            <div class="jrd-desc"><Markdown source={detail.description} /></div>
          {:else}
            <div class="jrd-none">No description. <button class="jrd-link" onclick={startEditDesc}>Add one</button></div>
          {/if}
        </section>

        <!-- Time — native Jira worklog. Tempo syncs these in/out by default,
             so logging here is the same thing you'd see in the Tempo timesheet
             at /plugins/servlet/ac/io.tempo.jira/tempo-app. -->
        <section class="jrd-section">
          <div class="jrd-sec-head">
            <span class="jrd-sec-label">
              Time {#if worklogs.length}· {formatDuration(totalWorklogSeconds)} logged{/if}
            </span>
            <span class="hatch" aria-hidden="true"></span>
            <div class="jrd-spring"></div>
            {#if worklogsLoaded}
              <button class="jrd-link" onclick={loadWorklogs} disabled={worklogsLoading} title="Refresh worklogs">
                Refresh
              </button>
            {/if}
          </div>

          {#if worklogsLoading && !worklogsLoaded}
            <div class="jrd-none">Loading worklogs…</div>
          {:else if worklogs.length === 0 && worklogsLoaded}
            <div class="jrd-none">No time logged yet.</div>
          {:else}
            <div class="jrd-worklogs">
              {#each worklogs as w (w.id)}
                <div class="jrd-worklog">
                  <div class="jrd-worklog-head">
                    {#if w.author?.avatar_url}
                      <img class="jrd-avatar" src={w.author.avatar_url} alt={w.author.display_name} />
                    {/if}
                    <span class="jrd-worklog-author">{w.author?.display_name ?? 'Unknown'}</span>
                    <span class="jrd-worklog-dur mono">{w.time_spent}</span>
                    <span class="jrd-worklog-time mono">{relativeTime(w.started, now)} ago</span>
                    <button
                      class="jrd-worklog-del"
                      onclick={() => deleteWorklog(w.id)}
                      disabled={deletingWorklogId === w.id}
                      title="Delete worklog (only your own)"
                      aria-label="Delete worklog"
                    >
                      <svg class="i i-sm" viewBox="0 0 24 24"><path d="M3 6h18M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2M19 6l-1 14a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2L5 6"/></svg>
                    </button>
                  </div>
                  {#if w.comment.trim()}
                    <div class="jrd-worklog-body"><Markdown source={w.comment} /></div>
                  {/if}
                </div>
              {/each}
            </div>
          {/if}

          <div class="jrd-log-time">
            <div class="jrd-log-row">
              <input
                class="jrd-log-dur mono"
                type="text"
                placeholder="1h 30m"
                bind:value={newWorklogDuration}
                disabled={addingWorklog}
                onkeydown={(e) => { if (e.key === 'Enter') void addWorklog(); }}
                aria-label="Duration"
              />
              <input
                class="jrd-log-note"
                type="text"
                placeholder="What did you work on? (optional)"
                bind:value={newWorklogComment}
                disabled={addingWorklog}
                onkeydown={(e) => { if (e.key === 'Enter') void addWorklog(); }}
                aria-label="Worklog comment"
              />
              <button
                class="jrd-btn jrd-btn--primary"
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
              <div class="jrd-log-err">{worklogsError}</div>
            {/if}
            <div class="jrd-log-hint">
              Format: <span class="mono">1h 30m</span>, <span class="mono">45m</span>, <span class="mono">2h</span>, <span class="mono">1.5h</span>, <span class="mono">1d 2h</span>. Jira: 1d = 8h, 1w = 5d.
            </div>
          </div>
        </section>

        <!-- Comments — border-left quotes (GitHub-detail grammar). -->
        <section class="jrd-section">
          <div class="jrd-sec-head">
            <span class="jrd-sec-label">Comments · {detail.comments.length}</span>
            <span class="hatch" aria-hidden="true"></span>
          </div>
          <div class="jrd-comments">
            {#each detail.comments as c (c.id)}
              <div class="jrd-quote">
                <div class="jrd-quote-head">
                  {#if c.author?.avatar_url}
                    <img class="jrd-avatar" src={c.author.avatar_url} alt={c.author.display_name} />
                  {/if}
                  <span class="jrd-quote-author">{c.author?.display_name ?? 'Unknown'}</span>
                  <span class="jrd-quote-time mono">{relativeTime(c.created, now)} ago</span>
                </div>
                <div class="jrd-quote-body"><Markdown source={c.body} /></div>
              </div>
            {/each}
            {#if detail.comments.length === 0}
              <div class="jrd-none">No comments yet.</div>
            {/if}
          </div>
          <div class="jrd-add-comment">
            <textarea
              class="jrd-input"
              placeholder="Add a comment (⌘↵ to send)"
              bind:value={newComment}
              rows="3"
              disabled={addingComment}
              onkeydown={(e) => { if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') void postComment(); }}
            ></textarea>
            <div class="jrd-save-row">
              <button class="jrd-btn jrd-btn--primary" onclick={postComment} disabled={addingComment || !newComment.trim()}>
                {addingComment ? 'Posting…' : 'Comment'}
              </button>
            </div>
          </div>
        </section>
      </div>
    </div>
  {/if}
</div>

<style>
  /* §2.6 Jira detail (mockup 4c) — single scrolling document, no side
     panels. Fresh `.jrd-` grammar: 52px header, centred document (max 800),
     summary + meta-grid + transitions + Description/Time/Comments sections. */

  .jrd { height: 100%; display: flex; flex-direction: column; min-height: 0; background: var(--bg-0); }

  /* Header ------------------------------------------------------------- */
  .jrd-head {
    flex: none;
    display: flex; align-items: center; gap: 10px;
    height: 52px; padding: 0 24px;
    border-bottom: 1px solid var(--border-lo);
    background: var(--bg-0);
  }
  .jrd-back {
    width: 28px; height: 28px; border-radius: 5px;
    display: inline-flex; align-items: center; justify-content: center;
    color: var(--text-1);
  }
  .jrd-back:hover { background: var(--bg-2); color: var(--text-0); }
  .jrd-ref { font-size: 12px; color: var(--text-1); font-weight: 600; }
  .jrd-chip {
    font-size: 11px; padding: 2px 8px; border-radius: 5px;
    background: var(--accent-soft); color: var(--text-1);
  }
  .jrd-prio { font-size: 12px; color: var(--text-faint); }
  .jrd-spring { flex: 1; }
  .jrd-iconbtn {
    width: 30px; height: 28px; border-radius: 6px;
    display: inline-flex; align-items: center; justify-content: center;
    background: transparent; color: var(--text-2);
    border: 1px solid var(--border-neutral);
  }
  .jrd-iconbtn:hover:not(:disabled) { background: var(--bg-2); color: var(--text-0); }
  .jrd-iconbtn:disabled { opacity: 0.45; cursor: not-allowed; }
  .jrd-iconbtn .i-sm { width: 14px; height: 14px; }
  .jrd-spin { animation: jrd-spin 0.8s linear infinite; }
  @keyframes jrd-spin { to { transform: rotate(360deg); } }
  .jrd-ghostbtn {
    display: inline-flex; align-items: center; gap: 6px;
    padding: 6px 12px; border-radius: 6px;
    background: transparent; color: var(--text-1);
    font-size: 12px; border: 1px solid var(--border-neutral-hi);
  }
  .jrd-ghostbtn:hover:not(:disabled) { background: var(--bg-2); color: var(--text-0); }
  .jrd-ghostbtn:disabled { opacity: 0.5; cursor: default; }
  /* → claude — primary inverse pill (spec §2.6: инверсный + shadow-pill). */
  .jrd-claudebtn {
    display: inline-flex; align-items: center;
    padding: 6px 14px; border-radius: 999px;
    background: var(--accent); color: var(--accent-fg);
    font-size: 12px; font-weight: 600; border: none;
    box-shadow: var(--shadow-pill);
  }
  .jrd-claudebtn:hover:not(:disabled) { background: var(--accent-bright); color: var(--accent-fg); }
  .jrd-claudebtn:disabled { opacity: 0.5; cursor: default; }

  .jrd-state { padding: 40px; text-align: center; color: var(--text-2); }
  .jrd-err { color: var(--error); }
  .jrd-link { color: var(--accent-bright); font-size: 12px; text-decoration: none; }
  .jrd-link:hover { text-decoration: underline; }

  /* Document ----------------------------------------------------------- */
  .jrd-scroll { flex: 1; min-height: 0; overflow-y: auto; }
  .jrd-doc { max-width: 800px; margin: 0 auto; padding: 30px 40px 60px; }

  /* Summary ------------------------------------------------------------ */
  .jrd-summary-sec { margin-bottom: 4px; }
  .jrd-summary {
    display: flex; align-items: flex-start; gap: 10px;
    padding: 4px; border-radius: 6px;
    text-align: left; width: 100%; color: inherit;
    transition: background 100ms;
  }
  .jrd-summary:hover { background: var(--bg-1); }
  .jrd-title {
    font-size: 22px; font-weight: 600; color: var(--text-0);
    letter-spacing: -0.015em; line-height: 1.25; margin: 0;
  }
  .jrd-edit-icon { opacity: 0; color: var(--text-2); margin-top: 6px; flex-shrink: 0; transition: opacity 120ms; }
  .jrd-summary:hover .jrd-edit-icon { opacity: 0.8; }
  .jrd-summary-input {
    width: 100%; font-size: 22px; font-weight: 600; color: var(--text-0);
    padding: 6px 10px; background: var(--bg-1); border: 1px solid var(--border-hi);
    border-radius: 6px; font-family: inherit;
  }
  .jrd-summary-input:focus { outline: none; border-color: var(--accent); }
  .jrd-save-row { display: flex; gap: 8px; margin-top: 10px; align-items: center; }

  /* Meta grid — caps-label + value, 16px avatars. --------------------- */
  .jrd-meta-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(150px, 1fr));
    gap: 14px 24px;
    padding: 16px 0 18px; margin-top: 12px;
    border-top: 1px solid var(--border);
    border-bottom: 1px solid var(--border);
  }
  .jrd-meta { display: flex; flex-direction: column; gap: 5px; }
  .jrd-meta--full { grid-column: 1 / -1; }
  .jrd-meta--editable { position: relative; }
  .jrd-meta-label {
    font-size: 10.5px; font-weight: 600;
    color: var(--text-faint);
    text-transform: uppercase; letter-spacing: 0.09em;
  }
  .jrd-meta-val { display: inline-flex; align-items: center; gap: 6px; font-size: 13px; color: var(--text-0); }
  .jrd-avatar { width: 16px; height: 16px; border-radius: 50%; flex-shrink: 0; }
  .jrd-none { color: var(--text-mute); font-size: 12.5px; }
  .jrd-labels { flex-wrap: wrap; }
  .jrd-label {
    font-size: 11px; padding: 2px 7px; border-radius: 4px;
    background: var(--accent-soft); color: var(--text-1);
  }
  .jrd-edit-target {
    display: inline-flex; align-items: center; gap: 6px;
    font-size: 13px; color: var(--text-0);
    padding: 2px 4px; border-radius: 4px; text-align: left;
    transition: background 100ms;
  }
  .jrd-edit-target:hover:not(:disabled) { background: var(--bg-1); }
  .jrd-edit-target:disabled { opacity: 0.5; cursor: default; }
  .jrd-edit-caret { color: var(--text-2); opacity: 0.6; margin-left: 4px; }

  /* Popovers ----------------------------------------------------------- */
  .jrd-popover {
    position: absolute; top: calc(100% + 4px); left: 0;
    min-width: 300px; max-width: 360px; max-height: 300px; overflow-y: auto;
    background: var(--bg-2); border: 1px solid var(--border-hi);
    border-radius: 8px; z-index: 10; box-shadow: var(--shadow-2);
    padding: 4px; display: flex; flex-direction: column; gap: 2px;
  }
  .jrd-popover--narrow { min-width: 180px; }
  .jrd-popover-input {
    width: 100%; padding: 6px 10px;
    background: var(--bg-0); border: 1px solid var(--border-neutral-hi);
    border-radius: 5px; color: var(--text-0); font-size: 12px; margin-bottom: 2px;
  }
  .jrd-popover-input:focus { outline: none; border-color: var(--border-hi2); }
  .jrd-popover-item {
    display: flex; align-items: center; gap: 8px;
    padding: 6px 10px; border-radius: 5px;
    font-size: 12.5px; color: var(--text-1); text-align: left;
  }
  .jrd-popover-item:hover { background: var(--bg-3); color: var(--text-0); }
  .jrd-popover-item.active { background: var(--accent-soft); color: var(--accent-bright); }
  .jrd-popover-sub { margin-left: auto; font-size: 10.5px; color: var(--text-mute); }
  .jrd-popover-state { padding: 8px 10px; font-size: 11.5px; color: var(--text-2); }

  /* Sections — caps label + hatch ornament. --------------------------- */
  .jrd-section { margin-top: 28px; }
  .jrd-sec-head { display: flex; align-items: center; gap: 10px; margin-bottom: 12px; }
  .jrd-sec-label {
    font-size: 10.5px; font-weight: 600;
    text-transform: uppercase; letter-spacing: 0.09em;
    color: var(--text-faint);
  }

  /* Transitions — current = inverse chip + 2px shadow, rest ghost. ----- */
  .jrd-transition-row { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; }
  .jrd-status-current {
    padding: 5px 12px; font-size: 11px; border-radius: var(--r-item);
    background: var(--accent); color: var(--accent-fg);
    font-weight: 600; box-shadow: var(--shadow-1);
  }
  .jrd-trans-arrow { color: var(--text-mute); font-size: 12px; }
  .jrd-meta-muted { color: var(--text-mute); font-size: 12px; }
  .jrd-transition {
    display: inline-flex; align-items: center; padding: 5px 12px;
    border-radius: var(--r-item); border: 1px solid var(--border-hi);
    font-size: 11px; color: var(--text-1); background: transparent;
    transition: background 120ms, color 120ms;
  }
  .jrd-transition:hover:not(:disabled) { background: var(--bg-hover); color: var(--text-0); }
  .jrd-transition:disabled { opacity: 0.5; cursor: default; }

  /* Inputs (description / labels / comment) — r9. --------------------- */
  .jrd-input {
    width: 100%; padding: 10px 14px;
    font-family: inherit; font-size: 13px;
    background: var(--bg-1); color: var(--text-0);
    border: 1px solid var(--border-neutral-hi); border-radius: 9px;
    line-height: 1.55; resize: vertical;
  }
  .jrd-input:focus { outline: none; border-color: var(--border-hi2); }
  .jrd-desc { color: var(--text-1); font-size: 13.5px; line-height: 1.6; }

  /* Comments — border-left quotes (GitHub-detail grammar). ------------ */
  .jrd-comments { display: flex; flex-direction: column; margin-bottom: 18px; }
  .jrd-quote {
    border-left: 2px solid var(--border-hi); padding-left: 14px; margin-bottom: 18px;
  }
  .jrd-quote-head {
    display: flex; align-items: center; gap: 8px;
    font-size: 11.5px; color: var(--text-faint); margin-bottom: 6px;
  }
  .jrd-quote-author { color: var(--text-1); font-weight: 500; }
  .jrd-quote-time { margin-left: auto; color: var(--text-mute); font-size: 11px; }
  .jrd-quote-body { font-size: 13.5px; line-height: 1.6; color: var(--text-1); }
  .jrd-add-comment { display: flex; flex-direction: column; }

  /* Worklog — list of native Jira time entries + inline "Log time" form. */
  .jrd-worklogs { display: flex; flex-direction: column; margin-bottom: 14px; }
  .jrd-worklog { padding: 8px 0; border-bottom: 1px solid var(--border-lo); }
  .jrd-worklog:last-child { border-bottom: none; }
  .jrd-worklog-head { display: flex; align-items: center; gap: 8px; }
  .jrd-worklog-author { font-size: 12.5px; color: var(--text-0); font-weight: 500; }
  .jrd-worklog-dur {
    font-size: 11px; font-weight: 600; padding: 2px 7px; border-radius: 4px;
    color: var(--accent-bright); background: var(--accent-soft);
  }
  .jrd-worklog-time { margin-left: auto; font-size: 11px; color: var(--text-mute); }
  .jrd-worklog-del {
    width: 22px; height: 22px; border-radius: 4px;
    display: inline-flex; align-items: center; justify-content: center;
    color: var(--text-mute); background: transparent; opacity: 0; transition: all 120ms;
  }
  .jrd-worklog:hover .jrd-worklog-del { opacity: 1; }
  .jrd-worklog-del:hover:not(:disabled) { color: var(--error); background: var(--bg-3); }
  .jrd-worklog-del:disabled { opacity: 0.3; cursor: default; }
  .jrd-worklog-body {
    margin-top: 6px; font-size: 12.5px; color: var(--text-1); line-height: 1.5; padding-left: 2px;
  }

  .jrd-log-time {
    border-top: 1px solid var(--border-neutral); padding-top: 12px;
    display: flex; flex-direction: column; gap: 8px;
  }
  .jrd-log-row { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; }
  .jrd-log-dur {
    width: 90px; padding: 7px 10px;
    background: var(--bg-0); border: 1px solid var(--border-neutral-hi);
    border-radius: 6px; color: var(--text-0); font-size: 12.5px;
    font-feature-settings: 'zero'; text-align: center;
  }
  .jrd-log-dur:focus { outline: none; border-color: var(--accent); }
  .jrd-log-note {
    flex: 1; min-width: 180px; padding: 7px 10px;
    background: var(--bg-0); border: 1px solid var(--border-neutral-hi);
    border-radius: 6px; color: var(--text-0); font-size: 12.5px; font-family: inherit;
  }
  .jrd-log-note:focus { outline: none; border-color: var(--accent); }
  .jrd-log-err { font-size: 11.5px; color: var(--error); }
  .jrd-log-hint { font-size: 10.5px; color: var(--text-mute); }

  /* Buttons ------------------------------------------------------------ */
  .jrd-btn {
    display: inline-flex; align-items: center; gap: 6px;
    padding: 6px 12px; border-radius: 6px;
    background: var(--bg-2); color: var(--text-1);
    font-size: 12px; border: 1px solid var(--border-neutral-hi);
  }
  .jrd-btn:hover:not(:disabled) { background: var(--bg-3); color: var(--text-0); }
  .jrd-btn:disabled { opacity: 0.5; cursor: default; }
  .jrd-btn--primary {
    background: var(--accent); color: var(--accent-fg);
    border-color: transparent; font-weight: 600;
  }
  .jrd-btn--primary:hover:not(:disabled) { background: var(--accent-bright); color: var(--accent-fg); }
</style>
