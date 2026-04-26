<script lang="ts">
  import { openUrl } from '@tauri-apps/plugin-opener';
  import Sigil from '$lib/components/ui/Sigil.svelte';
  import Dropdown, { type DropdownOption } from '$lib/components/ui/Dropdown.svelte';
  import {
    relativeTime,
    sentryLevelClass,
    type SentryProject,
    type SentryStatus
  } from '$lib/data';
  import {
    inboxState,
    loadSentryEnvironments,
    loadSentryProjects,
    openSentryFocus,
    refreshSentryTabInbox,
    scheduleSentryTabFilterRefresh
  } from '$lib/state/inbox.svelte';

  type View = 'workbench' | 'githubTab' | 'jiraTab' | 'sentryTab' | 'rules' | 'connections' | 'settings';

  interface Props {
    sentryStatus: SentryStatus;
    view: View;
    now: number;
  }

  let { sentryStatus, view = $bindable(), now }: Props = $props();

  // Drill-down model: zero projects selected = projects grid, one project
  // selected = filtered issue list. Mirrors JiraTab's projectKey pattern.
  // Issues tab keeps its own filter slice (`sentryTabProjects`,
  // `sentryTabStatus`, etc.) so a project / status / level pick here
  // does not yank the workbench SentryColumn out from under the user, and
  // vice-versa. Project + environment OPTION caches stay shared since
  // they're per-account static data, not user-picked filter state.
  const selectedProject = $derived<SentryProject | null>(
    inboxState.sentryTabProjects.length === 1
      ? inboxState.sentryProjectOptions.find(
          (p) => p.slug === inboxState.sentryTabProjects[0]
        ) ?? null
      : null
  );

  $effect(() => {
    if (sentryStatus.kind !== 'connected') return;
    if (
      inboxState.sentryProjectOptions.length === 0 &&
      !inboxState.sentryProjectOptionsLoading
    ) {
      void loadSentryProjects().then(() => loadSentryEnvironments());
    }
  });

  // Same `didAutoLoad`-style guard as SentryColumn: an empty success
  // response would otherwise re-trigger this effect forever
  // (items=[], loading=false, error=null is identical to the
  // pre-load state). Refresh on project switch is handled by
  // openProject directly.
  let lastFetchedProjectKey = $state<string | null>(null);
  $effect(() => {
    if (!selectedProject) return;
    if (lastFetchedProjectKey === selectedProject.slug) return;
    if (inboxState.sentryTabItemsLoading) return;
    lastFetchedProjectKey = selectedProject.slug;
    void refreshSentryTabInbox({ silent: false });
  });

  function openProject(p: SentryProject) {
    inboxState.sentryTabProjects = [p.slug];
    inboxState.sentryTabEnvironment = null;
    inboxState.sentryEnvironmentOptions = [];
    scheduleSentryTabFilterRefresh();
    void loadSentryEnvironments();
  }

  function backToProjects() {
    inboxState.sentryTabProjects = [];
    inboxState.sentryTabEnvironment = null;
    inboxState.sentryEnvironmentOptions = [];
    scheduleSentryTabFilterRefresh();
  }

  const envOpts = $derived<DropdownOption<string>[]>([
    { value: '', label: 'All environments' },
    ...inboxState.sentryEnvironmentOptions.map((e) => ({
      value: e.name,
      label: e.name
    }))
  ]);
  const statusOpts: DropdownOption<string>[] = [
    { value: 'unresolved', label: 'Unresolved' },
    { value: 'resolved', label: 'Resolved' },
    { value: 'ignored', label: 'Ignored' },
    { value: 'all', label: 'Any status' }
  ];
  const levelOpts: DropdownOption<string>[] = [
    { value: 'all', label: 'Any level' },
    { value: 'fatal', label: 'Fatal' },
    { value: 'error', label: 'Error' },
    { value: 'warning', label: 'Warning' },
    { value: 'info', label: 'Info' },
    { value: 'debug', label: 'Debug' }
  ];
  const sortOpts: DropdownOption<string>[] = [
    { value: 'date', label: 'Last seen' },
    { value: 'new', label: 'First seen' },
    { value: 'priority', label: 'Priority' },
    { value: 'freq', label: 'Events count' },
    { value: 'user', label: 'Users affected' }
  ];

  /* Each filter pick goes through the schedule helper so the new
     filter shape is both persisted (`forgehold:sentry-tab-filters:v1`)
     and refreshed — the column has its own slice now and won't push
     persisted state for us. */
  function pickEnvironment(name: string) {
    inboxState.sentryTabEnvironment = name || null;
    scheduleSentryTabFilterRefresh();
  }
  function pickStatus(s: string) {
    inboxState.sentryTabStatus = s as typeof inboxState.sentryTabStatus;
    scheduleSentryTabFilterRefresh();
  }
  function pickLevel(l: string) {
    inboxState.sentryTabLevel = l as typeof inboxState.sentryTabLevel;
    scheduleSentryTabFilterRefresh();
  }
  function pickSort(s: string) {
    inboxState.sentryTabSort = s as typeof inboxState.sentryTabSort;
    scheduleSentryTabFilterRefresh();
  }
  function onSearchInput(e: Event) {
    inboxState.sentryTabSearch = (e.target as HTMLInputElement).value;
    scheduleSentryTabFilterRefresh();
  }

  async function openBrowser(url: string) {
    try { await openUrl(url); } catch (e) { console.error(e); }
  }

  function shortText(s: string | null | undefined, max = 120): string {
    if (!s) return '';
    return s.length <= max ? s : `${s.slice(0, max - 1)}…`;
  }
</script>

{#if sentryStatus.kind !== 'connected'}
  <section class="full-center">
    <div class="empty">
      <Sigil size={56} />
      <h2 class="empty-title">Connect Sentry first</h2>
      <p class="empty-sub">
        Sentry projects and recent issues land here once Sentry is connected.
      </p>
      <button class="btn btn--primary" onclick={() => (view = 'connections')}>Set up connections</button>
    </div>
  </section>
{:else if !selectedProject}
  <section class="projects-view">
    <div class="projects-header">
      <h1 class="view-title">Sentry</h1>
      <p class="view-sub">
        Sentry projects you're a member of. Pick one to browse its issues with
        live status, level and environment filters.
      </p>
    </div>
    <div class="projects-body">
      {#if inboxState.sentryProjectOptionsLoading && inboxState.sentryProjectOptions.length === 0}
        <div class="tab-state">Loading projects…</div>
      {:else if inboxState.sentryProjectOptions.length === 0}
        <div class="tab-state">
          No Sentry projects visible to your token.
          <button class="link-inline" onclick={() => loadSentryProjects()}>Retry</button>
        </div>
      {:else}
        <div class="projects-grid">
          {#each inboxState.sentryProjectOptions as p (p.id)}
            <button class="project-card" onclick={() => openProject(p)}>
              <div class="project-card-head">
                <span class="project-avatar project-avatar--letter mono">
                  {(p.name || p.slug).slice(0, 2).toUpperCase()}
                </span>
                <div class="project-card-title">
                  <div class="project-key mono">{p.slug}</div>
                  <div class="project-name">{p.name || p.slug}</div>
                </div>
              </div>
              <div class="project-card-meta">
                {#if p.platform}<span class="project-platform mono">{p.platform}</span>{/if}
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
      <div class="project-detail-title mono">{selectedProject.slug}</div>
      <div class="project-detail-desc">{selectedProject.name || selectedProject.slug}</div>
      <div style="flex:1"></div>
    </header>

    <div class="project-filters">
      <div class="filter-cell">
        <Dropdown
          value={inboxState.sentryTabStatus}
          options={statusOpts}
          onChange={pickStatus}
          ariaLabel="Status"
          width="180px"
        />
      </div>
      <div class="filter-cell">
        <Dropdown
          value={inboxState.sentryTabLevel}
          options={levelOpts}
          onChange={pickLevel}
          ariaLabel="Level"
          width="160px"
        />
      </div>
      <div class="filter-cell">
        <Dropdown
          value={inboxState.sentryTabEnvironment ?? ''}
          options={envOpts}
          onChange={pickEnvironment}
          ariaLabel="Environment"
          placeholder={inboxState.sentryEnvironmentOptionsLoading ? 'Loading…' : 'All environments'}
          width="200px"
        />
      </div>
      <div class="filter-cell">
        <Dropdown
          value={inboxState.sentryTabSort}
          options={sortOpts}
          onChange={pickSort}
          ariaLabel="Sort"
          width="170px"
        />
      </div>
      <input
        class="filter-input"
        type="text"
        placeholder="Search title / tag:value…"
        value={inboxState.sentryTabSearch}
        oninput={onSearchInput}
        onkeydown={(e) => { if (e.key === 'Enter') void refreshSentryTabInbox({ silent: false }); }}
        aria-label="Search Sentry issues"
      />
      <div style="flex:1"></div>
      <span class="issues-count mono">{inboxState.sentryTabItems.length} issues</span>
      <button class="icon-btn" onclick={() => refreshSentryTabInbox({ silent: true })} title="Refresh" aria-label="Refresh Sentry" disabled={inboxState.sentryTabItemsLoading}>
        <svg class="i i-sm" viewBox="0 0 24 24" style="transform: rotate({inboxState.sentryTabItemsLoading ? 360 : 0}deg); transition: transform 0.6s;">
          <path d="M21 12a9 9 0 0 1-9 9 9 9 0 0 1-8.5-6" />
          <path d="M3 12a9 9 0 0 1 9-9 9 9 0 0 1 8.5 6" />
          <polyline points="21 3 21 9 15 9" />
          <polyline points="3 21 3 15 9 15" />
        </svg>
      </button>
    </div>

    <div class="issues-list">
      {#if inboxState.sentryTabItemsLoading && inboxState.sentryTabItems.length === 0}
        <div class="tab-state">Loading issues…</div>
      {:else if inboxState.sentryTabItemsError}
        <div class="tab-state tab-state--error">
          {inboxState.sentryTabItemsError}
          <button class="link-inline" onclick={() => refreshSentryTabInbox({ silent: false })}>Retry</button>
        </div>
      {:else if inboxState.sentryTabItems.length === 0}
        <div class="tab-state">No issues match the current filters.</div>
      {:else}
        {#each inboxState.sentryTabItems as issue (issue.id)}
          <button class="issue-row" onclick={() => openSentryFocus(issue.id)}>
            <span class="mini-tag {sentryLevelClass(issue.level)}">{issue.level}</span>
            <span class="issue-id mono">{issue.short_id || issue.id}</span>
            <span class="issue-title">{shortText(issue.title, 110)}</span>
            <span class="issue-kind mono">{issue.count} ev</span>
            {#if issue.user_count > 0}
              <span class="issue-priority">{issue.user_count} user{issue.user_count === 1 ? '' : 's'}</span>
            {/if}
            <span class="issue-time mono">{relativeTime(issue.last_seen, now)}</span>
          </button>
        {/each}
      {/if}
    </div>
  </section>
{/if}

<!-- SentryDetailPane slide-over is mounted globally at the page root. -->

<style>
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
  .btn--primary {
    color: var(--accent-fg);
    background: linear-gradient(135deg, #34d399, #10b981);
    box-shadow: 0 2px 8px rgba(16, 185, 129, 0.2), inset 0 1px 0 rgba(255, 255, 255, 0.2);
    font-weight: 600;
  }
  .btn--primary:hover:not(:disabled) {
    box-shadow: 0 4px 14px rgba(16, 185, 129, 0.3), inset 0 1px 0 rgba(255, 255, 255, 0.25);
    transform: translateY(-1px);
  }

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

  .mini-tag {
    padding: 1px 6px; border-radius: 3px; font-weight: 600;
    text-transform: uppercase; font-size: 9.5px; letter-spacing: 0.04em;
  }

  .i { width: 16px; height: 16px; stroke-width: 2; stroke: currentColor; fill: none; flex-shrink: 0; }
  .i-sm { width: 14px; height: 14px; }

  .mono { font-family: 'JetBrains Mono', ui-monospace, 'SF Mono', monospace; }

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
    flex-shrink: 0;
  }
  .project-avatar--letter {
    display: inline-flex; align-items: center; justify-content: center;
    font-size: 12.5px; font-weight: 700; letter-spacing: 0.02em;
    color: #f88f74;
    background: rgba(248, 143, 116, 0.12);
    border: 1px solid rgba(248, 143, 116, 0.3);
  }
  .project-card-title { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
  .project-key { font-size: 11px; color: var(--text-2); font-weight: 600; letter-spacing: 0.04em; text-transform: uppercase; }
  .project-name {
    font-size: 14px; color: var(--text-0); font-weight: 600;
    word-break: break-word; line-height: 1.35;
  }
  .project-card-meta { display: flex; align-items: center; gap: 8px; margin-top: auto; }
  .project-platform {
    padding: 1px 6px; border-radius: 3px; font-size: 10px;
    background: var(--bg-2); color: var(--text-2);
    border: 1px solid var(--border-neutral);
  }
  .project-card-hint { font-size: 11px; color: var(--text-mute); margin-left: auto; }
  .project-card:hover .project-card-hint { color: var(--accent-bright); }

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
  .issues-count { font-size: 11px; color: var(--text-mute); }

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
  .issue-id { font-size: 11.5px; color: var(--text-2); min-width: 96px; font-weight: 500; }
  .issue-title { flex: 1; font-size: 13px; color: var(--text-0); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .issue-kind { font-size: 10.5px; color: var(--text-mute); text-transform: lowercase; min-width: 56px; text-align: right; }
  .issue-priority {
    font-size: 10.5px; color: var(--text-2);
    padding: 1px 6px; border-radius: 3px;
    background: var(--bg-2);
    border: 1px solid var(--border-neutral);
  }
  .issue-time { font-size: 10.5px; color: var(--text-mute); min-width: 52px; text-align: right; }
</style>
