<script lang="ts">
  import { openUrl } from '@tauri-apps/plugin-opener';
  import Sigil from '$lib/components/ui/Sigil.svelte';
  import Dropdown, { type DropdownOption } from '$lib/components/ui/Dropdown.svelte';
  import {
    jiraStatusClass,
    jiraStatusColor,
    relativeTime,
    type JiraProject,
    type JiraStatus
  } from '$lib/data';
  import {
    inboxState,
    invalidateJiraStatuses,
    loadJiraBoards,
    loadJiraProjects,
    loadJiraSprints,
    loadJiraStatuses,
    openUserPicker,
    refreshJiraInbox,
    updateJiraFilters
  } from '$lib/state/inbox.svelte';

  type View = 'workbench' | 'repositories' | 'rules' | 'connections' | 'settings' | 'tasks';

  interface Props {
    jiraStatus: JiraStatus;
    view: View;
    now: number;
    onOpenCreateIssue: () => void;
  }

  let { jiraStatus, view = $bindable(), now, onOpenCreateIssue }: Props = $props();

  // Resolve the project object for the active filter. Shared with the
  // workbench JiraColumn via `inboxState.jiraFilters.projectKey` — picking a
  // project here persists into the workbench column too, same way a repo
  // selection in RepositoriesView is a separate local state but the inbox
  // column shares the rest of the filter slice.
  const selectedProject = $derived<JiraProject | null>(
    inboxState.jiraFilters.projectKey
      ? inboxState.jiraProjectOptions.find(
          (p) => p.key === inboxState.jiraFilters.projectKey
        ) ?? null
      : null
  );

  // Pulled from inboxState.jiraFilters.projectKey but not-yet-resolved means
  // we still need to load the project list to render either the grid or the
  // hydrated detail header. Either way: trigger `loadJiraProjects` on mount.
  $effect(() => {
    if (jiraStatus.kind !== 'connected') return;
    if (
      !inboxState.jiraProjectOptions.length &&
      !inboxState.jiraProjectOptionsLoading
    ) {
      void loadJiraProjects();
    }
  });

  // First time a project is in scope, make sure we have board/status lookups
  // ready so the filters render populated on open — matches the UX in
  // RepositoriesView where branches load the moment you enter a repo.
  $effect(() => {
    if (!selectedProject) return;
    if (
      !inboxState.jiraBoardOptions.length &&
      !inboxState.jiraBoardOptionsLoading
    ) {
      void loadJiraBoards(selectedProject.key);
    }
  });

  // If we're on a project with no issues cached yet, kick the search. Keeps
  // the grid → detail transition cheap when a project was set from the
  // workbench (items already in inboxState), and does the fetch when it
  // wasn't.
  $effect(() => {
    if (!selectedProject) return;
    if (
      inboxState.jiraItems.length === 0 &&
      !inboxState.jiraItemsLoading &&
      !inboxState.jiraItemsError
    ) {
      void refreshJiraInbox();
    }
  });

  function openProject(p: JiraProject) {
    updateJiraFilters({
      projectKey: p.key,
      boardIds: [],
      sprintId: null,
      statusName: null
    });
    inboxState.jiraBoardOptions = [];
    inboxState.jiraSprintOptions = [];
    invalidateJiraStatuses();
    void loadJiraBoards(p.key);
  }

  function backToProjects() {
    updateJiraFilters({
      projectKey: null,
      boardIds: [],
      sprintId: null,
      statusName: null
    });
    inboxState.jiraBoardOptions = [];
    inboxState.jiraSprintOptions = [];
    invalidateJiraStatuses();
  }

  function onBoardOpen() {
    if (!inboxState.jiraBoardOptions.length) {
      void loadJiraBoards(inboxState.jiraFilters.projectKey);
    }
  }
  function onSprintOpen() {
    const ids = inboxState.jiraFilters.boardIds;
    if (ids.length === 1 && !inboxState.jiraSprintOptions.length) {
      void loadJiraSprints(ids[0]);
    }
  }
  function onStatusOpen() {
    void loadJiraStatuses(inboxState.jiraFilters.projectKey);
  }

  /** Mirror of JiraColumn's multi-select handler — see that file's
   *  comment for semantics. Project drill-down already pinned us to
   *  one project so multi-board here mostly means picking sprints
   *  from multiple boards within that project. */
  function onBoardChange(value: string) {
    const next = inboxState.jiraFilters.boardIds.slice();
    if (!value) {
      updateJiraFilters({ boardIds: [], sprintId: null });
      inboxState.jiraSprintOptions = [];
      return;
    }
    const id = Number(value);
    const idx = next.indexOf(id);
    if (idx >= 0) next.splice(idx, 1);
    else next.push(id);
    updateJiraFilters({ boardIds: next, sprintId: null });
    inboxState.jiraSprintOptions = [];
    if (next.length === 1) void loadJiraSprints(next[0]);
  }
  function removeBoard(id: number) {
    const next = inboxState.jiraFilters.boardIds.filter((b) => b !== id);
    updateJiraFilters({ boardIds: next, sprintId: null });
    inboxState.jiraSprintOptions = [];
    if (next.length === 1) void loadJiraSprints(next[0]);
  }
  function boardLabel(id: number): string {
    return inboxState.jiraBoardOptions.find((b) => b.id === id)?.name ?? `#${id}`;
  }
  function onSprintChange(value: string) {
    let sprintId: number | 'backlog' | null;
    if (value === '') sprintId = null;
    else if (value === 'backlog') sprintId = 'backlog';
    else sprintId = Number(value);
    updateJiraFilters({ sprintId });
  }
  function onStatusChange(value: string) {
    updateJiraFilters({ statusName: value ? value : null });
  }
  function onSearchInput(e: Event) {
    updateJiraFilters({ search: (e.target as HTMLInputElement).value });
  }

  const sprintSelectValue = $derived.by(() => {
    const s = inboxState.jiraFilters.sprintId;
    if (s === 'backlog') return 'backlog';
    if (typeof s === 'number') return String(s);
    return '';
  });

  const boardOptions = $derived<DropdownOption<string>[]>([
    { value: '', label: 'All boards' },
    ...inboxState.jiraBoardOptions.map((b) => ({
      value: String(b.id),
      label: b.name,
      hint: b.type_
    }))
  ]);

  const sprintOptions = $derived<DropdownOption<string>[]>([
    { value: '', label: 'Any sprint' },
    { value: 'backlog', label: 'No sprint (backlog)' },
    ...inboxState.jiraSprintOptions.map((s) => ({
      value: String(s.id),
      label: s.name,
      hint: s.state
    }))
  ]);

  const statusOptions = $derived<DropdownOption<string>[]>([
    { value: '', label: 'Any status' },
    ...inboxState.jiraStatusOptions.map((s) => ({
      value: s.name,
      label: s.name,
      color: jiraStatusColor(s)
    }))
  ]);

  async function openBrowser(url: string) {
    try { await openUrl(url); } catch (e) { console.error(e); }
  }

  function projectBoardUrl(p: JiraProject): string {
    if (jiraStatus.kind !== 'connected') return '';
    return `https://${jiraStatus.user.workspace}/jira/software/projects/${encodeURIComponent(p.key)}/boards`;
  }
</script>

{#if jiraStatus.kind !== 'connected'}
  <section class="full-center">
    <div class="empty">
      <Sigil size={56} />
      <h2 class="empty-title">Connect Jira first</h2>
      <p class="empty-sub">
        Your Jira projects and issues land here once Jira is connected.
      </p>
      <button class="btn btn--primary" onclick={() => (view = 'connections')}>Set up connections</button>
    </div>
  </section>
{:else if !selectedProject}
  <section class="projects-view">
    <div class="projects-header">
      <h1 class="view-title">Tasks</h1>
      <p class="view-sub">
        Jira projects on <span class="mono">{jiraStatus.user.workspace}</span>.
        Pick a project to browse its issues — filters, search and status
        transitions work natively through the API.
      </p>
    </div>
    <div class="projects-body">
      {#if inboxState.jiraProjectOptionsLoading && inboxState.jiraProjectOptions.length === 0}
        <div class="tab-state">Loading projects…</div>
      {:else if inboxState.jiraProjectOptions.length === 0}
        <div class="tab-state">
          No projects visible to your account on this workspace.
          <button class="link-inline" onclick={() => loadJiraProjects()}>Retry</button>
        </div>
      {:else}
        <div class="projects-grid">
          {#each inboxState.jiraProjectOptions as p (p.id)}
            <button class="project-card" onclick={() => openProject(p)}>
              <div class="project-card-head">
                {#if p.avatar_url}
                  <img class="project-avatar" src={p.avatar_url} alt="" />
                {:else}
                  <span class="project-avatar project-avatar--letter mono">{p.key.slice(0, 2)}</span>
                {/if}
                <div class="project-card-title">
                  <div class="project-key mono">{p.key}</div>
                  <div class="project-name">{p.name}</div>
                </div>
              </div>
              <div class="project-card-meta">
                <span class="project-card-hint">Browse issues →</span>
              </div>
            </button>
          {/each}
        </div>
      {/if}
    </div>
  </section>
{:else}
  <section class="project-detail">
    <header class="project-detail-head">
      <button class="back-btn" onclick={backToProjects}>
        <svg class="i i-sm" viewBox="0 0 24 24"><path d="M19 12H5M12 19l-7-7 7-7" /></svg>
        All projects
      </button>
      <div class="project-detail-title mono">{selectedProject.key}</div>
      <div class="project-detail-desc">{selectedProject.name}</div>
      <div style="flex:1"></div>
      <button class="btn btn--ghost" onclick={onOpenCreateIssue}>
        <svg class="i i-sm" viewBox="0 0 24 24"><path d="M12 5v14M5 12h14" /></svg>
        New issue
      </button>
      <button class="btn btn--ghost" onclick={() => openBrowser(projectBoardUrl(selectedProject))}>
        <svg class="i i-sm" viewBox="0 0 24 24"><path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6" /><path d="M15 3h6v6M10 14 21 3" /></svg>
        Open on Jira
      </button>
    </header>

    <div class="project-filters">
      <div class="filter-cell">
        <!-- Multi-select board picker (see JiraColumn.svelte for the
             same shape). Selected boards live as chips below the
             filter row; sprint dropdown only shows when exactly one
             board is selected. -->
        <Dropdown
          value=""
          options={boardOptions}
          onChange={onBoardChange}
          onOpen={onBoardOpen}
          ariaLabel="Board"
          placeholder={inboxState.jiraFilters.boardIds.length === 0
            ? (inboxState.jiraBoardOptionsLoading ? 'Loading…' : 'All boards')
            : '+ Add another board'}
          width="200px"
        />
      </div>
      {#if inboxState.jiraFilters.boardIds.length === 1}
        <div class="filter-cell">
          <Dropdown
            value={sprintSelectValue}
            options={sprintOptions}
            onChange={onSprintChange}
            onOpen={onSprintOpen}
            ariaLabel="Sprint"
            placeholder={inboxState.jiraSprintOptionsLoading ? 'Loading…' : 'Any sprint'}
            width="200px"
          />
        </div>
      {/if}
      <div class="filter-cell">
        <Dropdown
          value={inboxState.jiraFilters.statusName ?? ''}
          options={statusOptions}
          onChange={onStatusChange}
          onOpen={onStatusOpen}
          ariaLabel="Status"
          placeholder={inboxState.jiraStatusOptionsLoading ? 'Loading…' : 'Any status'}
          width="180px"
        />
      </div>
      <input
        class="filter-input"
        type="text"
        placeholder="Search summary/description…"
        value={inboxState.jiraFilters.search}
        oninput={onSearchInput}
        aria-label="Search Jira issues"
      />
      <button class="assignee-chip" onclick={openUserPicker} title="Change assignee filter">
        {#if inboxState.jiraAssigneeAny}
          <svg class="i i-sm" viewBox="0 0 24 24"><circle cx="9" cy="7" r="4"/><circle cx="17" cy="7" r="3"/><path d="M3 21v-2a4 4 0 0 1 4-4h4a4 4 0 0 1 4 4v2M15 14h2a4 4 0 0 1 4 4v3"/></svg>
          <span>Any user</span>
        {:else if inboxState.jiraAssignee}
          <img src={inboxState.jiraAssignee.avatar_url} alt="" class="chip-avatar" />
          <span>{inboxState.jiraAssignee.display_name}</span>
        {:else}
          <svg class="i i-sm" viewBox="0 0 24 24"><path d="M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2"/><circle cx="12" cy="7" r="4"/></svg>
          <span>Me</span>
        {/if}
        <svg class="i i-sm" viewBox="0 0 24 24" style="color: var(--text-2)"><path d="m6 9 6 6 6-6"/></svg>
      </button>
      <div style="flex:1"></div>
      <span class="issues-count mono">{inboxState.jiraItems.length} issues</span>
      <button class="icon-btn" onclick={() => refreshJiraInbox()} title="Refresh" aria-label="Refresh Jira" disabled={inboxState.jiraItemsLoading}>
        <svg class="i i-sm" viewBox="0 0 24 24" style="transform: rotate({inboxState.jiraItemsLoading ? 360 : 0}deg); transition: transform 0.6s;">
          <path d="M21 12a9 9 0 0 1-9 9 9 9 0 0 1-8.5-6" />
          <path d="M3 12a9 9 0 0 1 9-9 9 9 0 0 1 8.5 6" />
          <polyline points="21 3 21 9 15 9" />
          <polyline points="3 21 3 15 9 15" />
        </svg>
      </button>
    </div>

    {#if inboxState.jiraFilters.boardIds.length > 0}
      <div class="board-chips">
        <span class="board-chips-label">Boards:</span>
        {#each inboxState.jiraFilters.boardIds as bid (bid)}
          <button class="board-chip" onclick={() => removeBoard(bid)} title="Remove board">
            <span class="board-chip-name">{boardLabel(bid)}</span>
            <svg class="i i-sm" viewBox="0 0 24 24"><path d="M18 6 6 18M6 6l12 12"/></svg>
          </button>
        {/each}
      </div>
    {/if}

    <div class="issues-list">
      {#if inboxState.jiraItemsLoading && inboxState.jiraItems.length === 0}
        <div class="tab-state">Loading issues…</div>
      {:else if inboxState.jiraItemsError}
        <div class="tab-state tab-state--error">
          {inboxState.jiraItemsError}
          <button class="link-inline" onclick={() => refreshJiraInbox()}>Retry</button>
        </div>
      {:else if inboxState.jiraItems.length === 0}
        <div class="tab-state">No issues match the current filters.</div>
      {:else}
        {#each inboxState.jiraItems as j (j.id)}
          <button class="issue-row" onclick={() => (inboxState.jiraFocusKey = j.key)}>
            <span class="mini-tag {jiraStatusClass(j.status_category)}">{j.status.toLowerCase()}</span>
            <span class="issue-id mono">{j.key}</span>
            <span class="issue-title">{j.summary}</span>
            <span class="issue-kind mono">{j.issue_type.toLowerCase()}</span>
            {#if j.priority}<span class="issue-priority">{j.priority.toLowerCase()}</span>{/if}
            <span class="issue-time mono">{relativeTime(j.updated, now)}</span>
          </button>
        {/each}
      {/if}
    </div>
  </section>
{/if}

<!-- JiraDetailPane slide-over is mounted globally at the page root. -->

<style>
  /* Shared layout helpers — mirrored from RepositoriesView so Tasks feels at
     home next to Repositories (same titles, tab-state colors, mono font). */
  .full-center { flex: 1; display: flex; align-items: center; justify-content: center; padding: 40px; }
  .empty { display: flex; flex-direction: column; align-items: center; gap: 16px; text-align: center; max-width: 420px; }
  .empty-title { font-size: 22px; font-weight: 600; margin: 12px 0 0; color: var(--text-0); letter-spacing: -0.015em; }
  .empty-sub { font-size: 13.5px; color: var(--text-1); margin: 0; line-height: 1.55; max-width: 380px; }

  .view-title { font-size: 28px; font-weight: 600; letter-spacing: -0.025em; color: var(--text-0); margin-bottom: 10px; }
  .view-sub { font-size: 14px; color: var(--text-2); max-width: 620px; margin: 0 auto; line-height: 1.5; }

  .btn {
    display: inline-flex; align-items: center; justify-content: center; gap: 8px;
    padding: 8px 16px; border-radius: 7px; font-size: 12.5px; font-weight: 500;
    border: none; cursor: pointer; transition: all 140ms; white-space: nowrap;
  }
  .btn--ghost { color: var(--text-1); background: transparent; border: 1px solid var(--border-neutral-hi); }
  .btn--ghost:hover:not(:disabled) { background: var(--bg-1); color: var(--text-0); border-color: var(--border-hi2); }
  .btn--primary {
    color: #0a111e;
    background: linear-gradient(135deg, #34d399, #10b981);
    box-shadow: 0 2px 8px rgba(16, 185, 129, 0.2), inset 0 1px 0 rgba(255, 255, 255, 0.2);
    font-weight: 600;
  }
  .btn--primary:hover:not(:disabled) {
    box-shadow: 0 4px 14px rgba(16, 185, 129, 0.3), inset 0 1px 0 rgba(255, 255, 255, 0.25);
    transform: translateY(-1px);
  }
  .btn:disabled { opacity: 0.5; cursor: not-allowed; }

  .back-btn {
    display: inline-flex; align-items: center; gap: 6px;
    padding: 6px 10px; border-radius: 6px; font-size: 12px; color: var(--text-1);
    background: transparent; transition: all 120ms; border: none; cursor: pointer;
  }
  .back-btn:hover { background: var(--bg-2); color: var(--text-0); }

  .icon-btn {
    display: inline-flex; align-items: center; justify-content: center;
    width: 26px; height: 26px; border-radius: 5px;
    color: var(--text-2); background: transparent;
    transition: all 120ms; border: none; cursor: pointer;
  }
  .icon-btn:hover:not(:disabled) { background: var(--bg-1); color: var(--text-0); }
  .icon-btn:disabled { opacity: 0.4; cursor: not-allowed; }

  .link-inline {
    display: inline-block; margin-left: 8px;
    color: var(--accent-bright); font-weight: 500; text-decoration: underline;
    background: transparent; border: none; cursor: pointer; font-size: inherit; padding: 0;
  }

  .tab-state { padding: 40px; text-align: center; color: var(--text-2); font-size: 13px; }
  .tab-state--error { color: #fca5a5; }

  .mini-tag { padding: 1px 6px; border-radius: 4px; font-size: 10px; font-weight: 600; text-transform: lowercase; }

  .i { width: 16px; height: 16px; stroke-width: 2; stroke: currentColor; fill: none; flex-shrink: 0; }
  .i-sm { width: 14px; height: 14px; }

  .mono { font-family: 'JetBrains Mono', ui-monospace, 'SF Mono', monospace; }

  /* Projects grid — parallel to RepositoriesView.repos-* for cross-view
     consistency. */
  .projects-view { overflow-y: auto; flex: 1 1 0; min-height: 0; }
  .projects-header { padding: 48px 56px 20px; text-align: center; }
  .projects-body { padding: 20px 56px 100px; max-width: 1100px; margin: 0 auto; width: 100%; }
  .projects-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(260px, 1fr));
    gap: 10px;
  }
  .project-card {
    padding: 16px 18px;
    background: var(--bg-1); border: 1px solid var(--border-neutral);
    border-radius: 10px; text-align: left;
    transition: all 180ms;
    display: flex; flex-direction: column; gap: 12px;
    cursor: pointer;
  }
  .project-card:hover {
    background: var(--bg-2); border-color: var(--border-neutral-hi);
    transform: translateY(-2px);
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.3);
  }
  .project-card-head { display: flex; align-items: center; gap: 12px; }
  .project-avatar {
    width: 36px; height: 36px; border-radius: 8px;
    background: var(--bg-2);
    object-fit: cover;
    flex-shrink: 0;
  }
  .project-avatar--letter {
    display: inline-flex; align-items: center; justify-content: center;
    font-size: 12.5px; font-weight: 700; letter-spacing: 0.02em;
    color: var(--accent-bright);
    background: var(--accent-soft);
    border: 1px solid rgba(232, 163, 58, 0.25);
  }
  .project-card-title { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
  .project-key { font-size: 11px; color: var(--text-2); font-weight: 600; letter-spacing: 0.04em; text-transform: uppercase; }
  .project-name {
    font-size: 14px; color: var(--text-0); font-weight: 600;
    word-break: break-word; line-height: 1.35;
  }
  .project-card-meta { display: flex; align-items: center; gap: 8px; margin-top: auto; }
  .project-card-hint { font-size: 11px; color: var(--text-mute); }
  .project-card:hover .project-card-hint { color: var(--accent-bright); }

  /* Project detail — mirrors RepositoriesView.repo-detail. */
  .project-detail {
    display: flex; flex-direction: column; overflow: hidden;
    flex: 1 1 0; min-height: 0; height: 100%;
  }
  .project-detail-head {
    padding: 16px 28px;
    border-bottom: 1px solid var(--border-neutral);
    display: flex; align-items: center; gap: 14px;
    flex-shrink: 0;
  }
  .project-detail-title { font-size: 14px; color: var(--text-0); font-weight: 600; }
  .project-detail-desc { font-size: 12.5px; color: var(--text-2); }

  .project-filters {
    display: flex; flex-wrap: wrap; align-items: center; gap: 8px;
    padding: 10px 28px;
    border-bottom: 1px solid var(--border-neutral);
    background: var(--bg-1);
    flex-shrink: 0;
  }
  .filter-cell { flex: 0 0 auto; min-width: 0; }
  .filter-input {
    flex: 1 1 220px; min-width: 180px;
    padding: 7px 10px;
    background: var(--bg-0);
    border: 1px solid var(--border-neutral);
    border-radius: 6px;
    color: var(--text-1);
    font-size: 12px;
    font-family: inherit;
    line-height: 1.2;
  }
  .filter-input:hover { border-color: var(--border-neutral-hi); }
  .filter-input:focus { outline: 1px solid var(--accent); outline-offset: 0; border-color: var(--accent); }

  .assignee-chip {
    display: inline-flex; align-items: center; gap: 8px;
    padding: 6px 10px;
    background: var(--bg-0);
    border: 1px solid var(--border-neutral);
    border-radius: 7px;
    font-size: 12px;
    color: var(--text-1);
    transition: all 120ms;
    cursor: pointer;
  }
  .assignee-chip:hover { border-color: var(--border-neutral-hi); color: var(--text-0); background: var(--bg-1); }
  .chip-avatar {
    width: 18px; height: 18px;
    border-radius: 50%;
    object-fit: cover;
    flex-shrink: 0;
    background: linear-gradient(135deg, #5aa2ff, #8b96ab);
  }
  .issues-count { font-size: 11px; color: var(--text-mute); }

  /* Selected-board chip strip — sits between filters and the issue
     list when 1+ boards are selected. Mirrors the JiraColumn version. */
  .board-chips {
    display: flex; flex-wrap: wrap; align-items: center; gap: 6px;
    padding: 6px 28px 0;
  }
  .board-chips-label { font-size: 11px; color: var(--text-mute); text-transform: uppercase; letter-spacing: 0.04em; }
  .board-chip {
    display: inline-flex; align-items: center; gap: 5px;
    padding: 3px 6px 3px 9px;
    background: var(--accent-soft);
    border: 1px solid rgba(232, 163, 58, 0.25);
    border-radius: 5px;
    font-size: 11px;
    color: var(--text-1);
    cursor: pointer;
    transition: background 100ms;
  }
  .board-chip:hover { background: rgba(232, 163, 58, 0.18); color: var(--text-0); }
  .board-chip-name { white-space: nowrap; max-width: 200px; overflow: hidden; text-overflow: ellipsis; }
  .board-chip .i-sm { width: 11px; height: 11px; opacity: 0.6; }
  .board-chip:hover .i-sm { opacity: 1; }

  .issues-list { flex: 1; overflow-y: auto; padding: 8px 28px 60px; min-height: 0; }
  .issue-row {
    display: flex; align-items: center; gap: 12px;
    width: 100%; padding: 10px 14px;
    text-align: left; transition: background 120ms;
    border-radius: 8px;
    border: 1px solid transparent;
    background: transparent;
    cursor: pointer;
  }
  .issue-row:hover { background: var(--bg-1); border-color: var(--border-neutral); }
  .issue-id { font-size: 11.5px; color: var(--text-2); min-width: 76px; font-weight: 500; }
  .issue-title { flex: 1; font-size: 13px; color: var(--text-0); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .issue-kind { font-size: 10.5px; color: var(--text-mute); text-transform: lowercase; min-width: 48px; }
  .issue-priority {
    font-size: 10.5px; color: var(--text-2);
    padding: 1px 6px; border-radius: 3px;
    background: var(--bg-2);
    border: 1px solid var(--border-neutral);
  }
  .issue-time { font-size: 10.5px; color: var(--text-mute); min-width: 52px; text-align: right; }
</style>
