<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import Sigil from '$lib/components/ui/Sigil.svelte';
  import Markdown from '$lib/components/ui/Markdown.svelte';
  import GithubFocusOverlay from '$lib/components/inbox/GithubFocusOverlay.svelte';
  import Dropdown, { type DropdownOption } from '$lib/components/ui/Dropdown.svelte';
  import {
    kindLabel,
    relativeTime,
    stateTag,
    type CommitEntry,
    type FileBlob,
    type InboxItem,
    type RepoBranch,
    type Release,
    type Repository,
    type RepoReadme,
    type TreeEntry,
    type WorkflowRun
  } from '$lib/data';
  import { formatBytes } from '$lib/format';
  import { inboxState } from '$lib/state/inbox.svelte';

  type View = 'workbench' | 'repositories' | 'tasks' | 'rules' | 'connections' | 'settings';
  type DetailTab = 'conversation' | 'commits' | 'files' | 'reviews' | 'checks';
  type RepoSection = 'code' | 'pulls' | 'issues' | 'actions' | 'releases';
  type RepoTab = 'open' | 'closed' | 'all';

  interface Props {
    connectedGithub: boolean;
    now: number;
    view: View;
    tab: DetailTab;
    actionBusy: string | null;
    onOpenFocusItem: (item: InboxItem) => void;
    // Focus-overlay callbacks — same handlers the parent passes to GithubColumn,
    // wired through so opening a PR in Repositories shows the same detail pane.
    // Close is handled internally (we own when the overlay disappears).
    onRetryLoadDetail: () => void;
    onTabChange: (tab: DetailTab) => void;
    onToggleFile: (filename: string) => void;
    onOpenCommit: (c: CommitEntry) => void;
    onOpenComment: () => void;
    onOpenReview: () => void;
    onOpenMerge: () => void;
    onAskClose: () => void;
    onReopen: () => void;
    onOpenBrowser: (url: string) => void;
    onOpenCheckDetails: (url: string) => void;
    mergeDisabled: () => boolean;
  }

  let {
    connectedGithub,
    now,
    view = $bindable(),
    tab,
    actionBusy,
    onOpenFocusItem,
    onRetryLoadDetail,
    onTabChange,
    onToggleFile,
    onOpenCommit,
    onOpenComment,
    onOpenReview,
    onOpenMerge,
    onAskClose,
    onReopen,
    onOpenBrowser,
    onOpenCheckDetails,
    mergeDisabled
  }: Props = $props();

  // Repositories state (owned by this component)
  let repos = $state<Repository[]>([]);
  let reposLoading = $state(false);
  let reposError = $state<string | null>(null);
  let selectedRepo = $state<Repository | null>(null);
  let repoItems = $state<InboxItem[]>([]);
  let repoItemsLoading = $state(false);
  let repoItemsError = $state<string | null>(null);
  let repoStateFilter = $state<RepoTab>('open');
  let repoSection = $state<RepoSection>('pulls');
  let workflowRuns = $state<WorkflowRun[]>([]);
  let workflowRunsLoading = $state(false);
  let workflowRunsError = $state<string | null>(null);
  let repoReadme = $state<RepoReadme | null>(null);
  let repoReadmeLoading = $state(false);

  // Code tab — GitHub-style file browser state. `repoCodeBranch` falls back
  // to the repo's default branch when we first enter a repo. `repoCodeTree`
  // holds the recursive tree (GitHub returns the whole tree flat); the
  // breadcrumb `repoCodePath` decides which directory level is rendered.
  let repoCodeBranches = $state<RepoBranch[]>([]);
  let repoCodeBranchesLoading = $state(false);
  let repoCodeBranch = $state<string>('');
  let repoCodeTree = $state<TreeEntry[]>([]);
  let repoCodeTreeLoading = $state(false);
  let repoCodeTreeError = $state<string | null>(null);
  let repoCodePath = $state<string>(''); // '' = repo root
  let repoCodeFile = $state<FileBlob | null>(null);
  let repoCodeFileLoading = $state(false);
  let repoCodeFileError = $state<string | null>(null);

  let repoReleases = $state<Release[]>([]);
  let repoReleasesLoading = $state(false);
  let repoReleasesError = $state<string | null>(null);

  // In-flight workflow rerun / cancel guards (keyed by run id).
  let runBusy = $state<Set<number>>(new Set());

  const repoPulls = $derived(repoItems.filter((i) => i.is_pull_request));
  const repoIssues = $derived(repoItems.filter((i) => !i.is_pull_request));

  // Lazy-load repos when Repositories view opens.
  $effect(() => {
    if (connectedGithub && !repos.length && !reposLoading) {
      void loadRepos();
    }
  });

  // When GitHub disconnects, wipe all repo state — parent used to clear it on
  // disconnect, but the state now lives here.
  $effect(() => {
    if (!connectedGithub) {
      repos = [];
      selectedRepo = null;
      repoItems = [];
    }
  });

  async function loadRepos() {
    reposLoading = true;
    reposError = null;
    try {
      repos = await invoke<Repository[]>('github_list_repos');
    } catch (e) {
      reposError = typeof e === 'string' ? e : String(e);
    } finally {
      reposLoading = false;
    }
  }

  async function openRepo(repo: Repository) {
    selectedRepo = repo;
    repoItems = [];
    repoItemsError = null;
    // Clear per-repo caches so we don't flash the previous repo's tree/file
    // through before fresh data lands.
    repoReadme = null;
    repoCodeBranches = [];
    repoCodeBranch = repo.default_branch;
    repoCodeTree = [];
    repoCodeTreeError = null;
    repoCodePath = '';
    repoCodeFile = null;
    repoCodeFileError = null;
    workflowRuns = [];
    repoReleases = [];
    await loadRepoItems();
  }

  async function loadRepoItems() {
    if (!selectedRepo) return;
    repoItemsLoading = true;
    repoItemsError = null;
    try {
      repoItems = await invoke<InboxItem[]>('github_list_repo_items', {
        owner: selectedRepo.owner,
        repo: selectedRepo.name,
        state: repoStateFilter
      });
    } catch (e) {
      repoItemsError = typeof e === 'string' ? e : String(e);
    } finally {
      repoItemsLoading = false;
    }
  }

  async function loadWorkflowRuns() {
    if (!selectedRepo) return;
    workflowRunsLoading = true;
    workflowRunsError = null;
    try {
      workflowRuns = await invoke<WorkflowRun[]>('github_list_workflow_runs', {
        owner: selectedRepo.owner,
        repo: selectedRepo.name
      });
    } catch (e) {
      workflowRunsError = typeof e === 'string' ? e : String(e);
    } finally {
      workflowRunsLoading = false;
    }
  }

  async function loadRepoReadme() {
    if (!selectedRepo) return;
    repoReadmeLoading = true;
    try {
      repoReadme = await invoke<RepoReadme | null>('github_get_readme', {
        owner: selectedRepo.owner,
        repo: selectedRepo.name
      });
    } catch {
      repoReadme = null;
    } finally {
      repoReadmeLoading = false;
    }
  }

  function selectRepoSection(s: RepoSection) {
    repoSection = s;
    if (s === 'actions' && !workflowRuns.length && !workflowRunsLoading) void loadWorkflowRuns();
    if (s === 'code') {
      if (!repoReadme && !repoReadmeLoading) void loadRepoReadme();
      if (!repoCodeBranches.length && !repoCodeBranchesLoading) void loadRepoBranches();
      if (!repoCodeTree.length && !repoCodeTreeLoading) void loadRepoTree();
    }
    if (s === 'releases' && !repoReleases.length && !repoReleasesLoading) void loadReleases();
  }

  async function loadRepoBranches() {
    if (!selectedRepo) return;
    repoCodeBranchesLoading = true;
    try {
      repoCodeBranches = await invoke<RepoBranch[]>('github_list_repo_branches', {
        owner: selectedRepo.owner,
        repo: selectedRepo.name
      });
      if (!repoCodeBranch) repoCodeBranch = selectedRepo.default_branch;
    } catch (e) {
      console.error('github_list_repo_branches', e);
    } finally {
      repoCodeBranchesLoading = false;
    }
  }

  async function loadRepoTree() {
    if (!selectedRepo) return;
    const ref = repoCodeBranch || selectedRepo.default_branch;
    repoCodeTreeLoading = true;
    repoCodeTreeError = null;
    try {
      repoCodeTree = await invoke<TreeEntry[]>('github_list_tree', {
        owner: selectedRepo.owner,
        repo: selectedRepo.name,
        reference: ref
      });
    } catch (e) {
      repoCodeTreeError = typeof e === 'string' ? e : String(e);
      repoCodeTree = [];
    } finally {
      repoCodeTreeLoading = false;
    }
  }

  /** Filter the flat recursive tree down to the immediate children of the
      current path — mimics GitHub's single-level view while avoiding per-
      directory API calls (we already have the whole tree in memory). */
  function repoCodeEntriesAtPath(path: string): TreeEntry[] {
    const prefix = path ? path + '/' : '';
    const seen = new Set<string>();
    const out: TreeEntry[] = [];
    for (const e of repoCodeTree) {
      if (!e.path.startsWith(prefix)) continue;
      const tail = e.path.slice(prefix.length);
      if (!tail) continue;
      const slash = tail.indexOf('/');
      const name = slash === -1 ? tail : tail.slice(0, slash);
      if (seen.has(name)) continue;
      seen.add(name);
      // Direct child → use the real entry. Nested child → synthesize a dir
      // entry (GitHub's recursive tree doesn't always emit intermediate
      // tree nodes in a predictable order).
      if (slash === -1) {
        out.push(e);
      } else {
        out.push({ path: prefix + name, sha: '', kind: 'tree', size: null });
      }
    }
    out.sort((a, b) => {
      if (a.kind !== b.kind) return a.kind === 'tree' ? -1 : 1;
      return a.path.localeCompare(b.path);
    });
    return out;
  }

  async function openRepoFile(entry: TreeEntry) {
    if (!selectedRepo || entry.kind !== 'blob') return;
    const ref = repoCodeBranch || selectedRepo.default_branch;
    repoCodeFileLoading = true;
    repoCodeFileError = null;
    try {
      repoCodeFile = await invoke<FileBlob>('github_get_file_content', {
        owner: selectedRepo.owner,
        repo: selectedRepo.name,
        path: entry.path,
        reference: ref
      });
    } catch (e) {
      repoCodeFileError = typeof e === 'string' ? e : String(e);
      repoCodeFile = null;
    } finally {
      repoCodeFileLoading = false;
    }
  }

  function repoCodeCloseFile() {
    repoCodeFile = null;
    repoCodeFileError = null;
  }

  function repoCodeNavigate(path: string) {
    repoCodePath = path;
    repoCodeCloseFile();
  }

  function switchRepoCodeBranch(branch: string) {
    if (branch === repoCodeBranch) return;
    repoCodeBranch = branch;
    repoCodePath = '';
    repoCodeCloseFile();
    void loadRepoTree();
  }

  /** Split a path like `foo/bar/baz.ts` into progressively-deeper pairs
      `[{name, path}, …]` for breadcrumb rendering. */
  function repoCodeCrumbs(path: string): Array<{ name: string; path: string }> {
    if (!path) return [];
    const segs = path.split('/');
    const out: Array<{ name: string; path: string }> = [];
    let acc = '';
    for (const s of segs) {
      acc = acc ? acc + '/' + s : s;
      out.push({ name: s, path: acc });
    }
    return out;
  }

  async function loadReleases() {
    if (!selectedRepo) return;
    repoReleasesLoading = true;
    repoReleasesError = null;
    try {
      repoReleases = await invoke<Release[]>('github_list_releases', {
        owner: selectedRepo.owner,
        repo: selectedRepo.name
      });
    } catch (e) {
      repoReleasesError = typeof e === 'string' ? e : String(e);
    } finally {
      repoReleasesLoading = false;
    }
  }

  async function rerunWorkflow(runId: number) {
    if (!selectedRepo) return;
    runBusy.add(runId); runBusy = new Set(runBusy);
    try {
      await invoke('github_rerun_workflow', {
        owner: selectedRepo.owner,
        repo: selectedRepo.name,
        runId
      });
      // Give GitHub a moment then refresh.
      setTimeout(() => void loadWorkflowRuns(), 1200);
    } catch (e) {
      workflowRunsError = typeof e === 'string' ? e : String(e);
    } finally {
      runBusy.delete(runId); runBusy = new Set(runBusy);
    }
  }

  async function cancelWorkflow(runId: number) {
    if (!selectedRepo) return;
    runBusy.add(runId); runBusy = new Set(runBusy);
    try {
      await invoke('github_cancel_workflow', {
        owner: selectedRepo.owner,
        repo: selectedRepo.name,
        runId
      });
      setTimeout(() => void loadWorkflowRuns(), 1200);
    } catch (e) {
      workflowRunsError = typeof e === 'string' ? e : String(e);
    } finally {
      runBusy.delete(runId); runBusy = new Set(runBusy);
    }
  }

  async function openBrowser(url: string) {
    try { await openUrl(url); } catch (e) { console.error(e); }
  }

  // Exposed so the parent can refresh the repo items list after cross-cutting
  // actions (e.g. merging a PR via the modal flow) without knowing about the
  // underlying state.
  export function refreshItems() {
    if (selectedRepo) void loadRepoItems();
  }
</script>

{#if !connectedGithub}
  <section class="full-center">
    <div class="empty">
      <Sigil size={56} />
      <h2 class="empty-title">Connect GitHub first</h2>
      <p class="empty-sub">Your repos will appear here once GitHub is connected.</p>
      <button class="btn btn--primary" onclick={() => (view = 'connections')}>Set up connections</button>
    </div>
  </section>
{:else if !selectedRepo}
  <section class="repos-view">
    <div class="repos-header">
      <h1 class="view-title">Repositories</h1>
      <p class="view-sub">Your accessible repos on GitHub. Click one to browse its pull requests and issues.</p>
    </div>
    <div class="repos-body">
      {#if reposLoading}
        <div class="tab-state">Loading repos…</div>
      {:else if reposError}
        <div class="tab-state tab-state--error">{reposError} <button class="link-inline" onclick={() => loadRepos()}>Retry</button></div>
      {:else if repos.length === 0}
        <div class="tab-state">No repos found.</div>
      {:else}
        <div class="repos-grid">
          {#each repos as r (r.id)}
            <button class="repo-card" onclick={() => openRepo(r)}>
              <div class="repo-card-head">
                <span class="repo-name mono">{r.full_name}</span>
                {#if r.private}<span class="repo-flag">private</span>{/if}
                {#if r.fork}<span class="repo-flag">fork</span>{/if}
                {#if r.archived}<span class="repo-flag repo-flag--archived">archived</span>{/if}
              </div>
              {#if r.description}<div class="repo-desc">{r.description}</div>{/if}
              <div class="repo-meta">
                {#if r.language}<span class="repo-lang">{r.language}</span>{/if}
                <span class="repo-stars mono">★ {r.stargazers_count}</span>
                <span class="repo-open mono">{r.open_issues_count} open</span>
                <span class="repo-updated mono" style="margin-left:auto">{relativeTime(r.updated_at, now)} ago</span>
              </div>
            </button>
          {/each}
        </div>
      {/if}
    </div>
  </section>
{:else}
  <section class="repo-detail">
    <header class="repo-detail-head">
      <button class="back-btn" onclick={() => { selectedRepo = null; repoItems = []; }}>
        <svg class="i i-sm" viewBox="0 0 24 24"><path d="M19 12H5M12 19l-7-7 7-7" /></svg>
        All repos
      </button>
      <div class="repo-detail-title mono">{selectedRepo.full_name}</div>
      {#if selectedRepo.description}
        <div class="repo-detail-desc">{selectedRepo.description}</div>
      {/if}
      <div style="flex:1"></div>
      <button class="btn btn--ghost" onclick={() => openBrowser(selectedRepo!.html_url)}>
        <svg class="i i-sm" viewBox="0 0 24 24"><path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6" /><path d="M15 3h6v6M10 14 21 3" /></svg>
        Open on GitHub
      </button>
    </header>

    <div class="repo-sections">
      <button class="repo-section-tab" class:active={repoSection === 'code'} onclick={() => selectRepoSection('code')}>
        <svg class="i i-sm" viewBox="0 0 24 24"><path d="M16 18l6-6-6-6M8 6l-6 6 6 6" /></svg>
        Code
      </button>
      <button class="repo-section-tab" class:active={repoSection === 'pulls'} onclick={() => selectRepoSection('pulls')}>
        <svg class="i i-sm" viewBox="0 0 24 24"><circle cx="6" cy="6" r="2.5"/><circle cx="6" cy="18" r="2.5"/><circle cx="18" cy="18" r="2.5"/><path d="M6 8.5v7M8.5 6h7a3 3 0 0 1 3 3v6.5"/></svg>
        Pull requests
        {#if repoPulls.length}<span class="repo-section-count mono">{repoPulls.length}</span>{/if}
      </button>
      <button class="repo-section-tab" class:active={repoSection === 'issues'} onclick={() => selectRepoSection('issues')}>
        <svg class="i i-sm" viewBox="0 0 24 24"><circle cx="12" cy="12" r="9"/><circle cx="12" cy="12" r="3"/></svg>
        Issues
        {#if repoIssues.length}<span class="repo-section-count mono">{repoIssues.length}</span>{/if}
      </button>
      <button class="repo-section-tab" class:active={repoSection === 'actions'} onclick={() => selectRepoSection('actions')}>
        <svg class="i i-sm" viewBox="0 0 24 24"><polygon points="5 3 19 12 5 21 5 3" /></svg>
        Actions
        {#if workflowRuns.length}<span class="repo-section-count mono">{workflowRuns.length}</span>{/if}
      </button>
      <button class="repo-section-tab" class:active={repoSection === 'releases'} onclick={() => selectRepoSection('releases')}>
        <svg class="i i-sm" viewBox="0 0 24 24"><path d="M20.59 13.41 13.42 20.58a2 2 0 0 1-2.83 0L2 12V2h10l8.59 8.59a2 2 0 0 1 0 2.82z" /><line x1="7" y1="7" x2="7.01" y2="7" /></svg>
        Releases
        {#if repoReleases.length}<span class="repo-section-count mono">{repoReleases.length}</span>{/if}
      </button>
    </div>

    {#if repoSection === 'code'}
      <!-- Pre-compute branch selector bits outside the dropdown. `{@const}`
           has to sit directly under a block tag, not inside arbitrary HTML
           — doing it here keeps the JSX clean. The branch dropdown surfaces
           the current branch explicitly as an option even when the API
           listing hasn't returned yet or doesn't contain it, so the trigger
           label always matches the actual selection. -->
      {@const knownBranchNames = new Set(repoCodeBranches.map((b) => b.name))}
      {@const currentBranch = repoCodeBranch || selectedRepo.default_branch}
      {@const branchOptions: DropdownOption<string>[] = [
        ...(currentBranch && !knownBranchNames.has(currentBranch)
          ? [{ value: currentBranch, label: currentBranch }]
          : []),
        ...repoCodeBranches.map((b) => ({ value: b.name, label: b.name }))
      ]}
      <div class="repo-code">
        <div class="repo-code-meta">
          <Dropdown
            value={currentBranch}
            options={branchOptions}
            onChange={(v) => switchRepoCodeBranch(v)}
            disabled={repoCodeBranchesLoading && !repoCodeBranches.length}
            ariaLabel="Switch branch"
            placeholder="Pick branch"
          />
          {#if selectedRepo.language}<span class="repo-code-chip">{selectedRepo.language}</span>{/if}
          <span class="repo-code-chip">★ {selectedRepo.stargazers_count}</span>
          <span class="repo-code-chip">issues {selectedRepo.open_issues_count}</span>
          {#if selectedRepo.private}<span class="repo-code-chip repo-code-chip--warn">private</span>{/if}
          {#if selectedRepo.fork}<span class="repo-code-chip">fork</span>{/if}
          {#if selectedRepo.archived}<span class="repo-code-chip repo-code-chip--warn">archived</span>{/if}
        </div>

        <!-- Breadcrumb. Root gets the repo name so users never lose
             orientation after drilling into deep paths. -->
        <div class="repo-path-bar mono">
          <button class="repo-crumb" onclick={() => repoCodeNavigate('')} title="Back to root">
            {selectedRepo.name}
          </button>
          {#each repoCodeCrumbs(repoCodePath) as crumb (crumb.path)}
            <span class="repo-crumb-sep">/</span>
            <button class="repo-crumb" onclick={() => repoCodeNavigate(crumb.path)}>{crumb.name}</button>
          {/each}
          {#if repoCodeFile}
            <span class="repo-crumb-sep">·</span>
            <button class="repo-crumb repo-crumb--back" onclick={repoCodeCloseFile}>← back to tree</button>
          {/if}
        </div>

        {#if repoCodeFile}
          <div class="repo-file-viewer">
            <div class="repo-file-head mono">
              <span>{repoCodeFile.path}</span>
              <span class="repo-file-size">{formatBytes(repoCodeFile.size)}</span>
            </div>
            {#if repoCodeFileLoading}
              <div class="tab-state">Loading file…</div>
            {:else if !repoCodeFile.is_text}
              <div class="tab-state">Binary file — preview not available.</div>
            {:else if /\.md$/i.test(repoCodeFile.path)}
              <div class="repo-readme-body">
                <Markdown source={repoCodeFile.content} />
              </div>
            {:else}
              <pre class="repo-file-source mono">{repoCodeFile.content}</pre>
            {/if}
          </div>
        {:else if repoCodeTreeLoading && !repoCodeTree.length}
          <div class="tab-state">Loading tree…</div>
        {:else if repoCodeTreeError}
          <div class="tab-state tab-state--error">{repoCodeTreeError} <button class="link-inline" onclick={() => loadRepoTree()}>Retry</button></div>
        {:else}
          <div class="repo-tree-list">
            {#each repoCodeEntriesAtPath(repoCodePath) as entry (entry.path)}
              {@const name = entry.path.split('/').pop() ?? entry.path}
              {#if entry.kind === 'notice'}
                <div class="tab-state">
                  Tree was truncated by GitHub (very large repo). Showing a partial view.
                </div>
              {:else if entry.kind === 'tree'}
                <button class="repo-tree-row" onclick={() => repoCodeNavigate(entry.path)}>
                  <svg class="i i-sm repo-tree-icon" viewBox="0 0 24 24"><path d="M3 7v11a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2V9a2 2 0 0 0-2-2h-7L10 5H5a2 2 0 0 0-2 2z"/></svg>
                  <span class="repo-tree-name mono">{name}</span>
                </button>
              {:else}
                <button class="repo-tree-row" onclick={() => openRepoFile(entry)} disabled={repoCodeFileLoading}>
                  <svg class="i i-sm repo-tree-icon repo-tree-icon--file" viewBox="0 0 24 24"><path d="M14 3v4a1 1 0 0 0 1 1h4M17 21H7a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h7l5 5v11a2 2 0 0 1-2 2z"/></svg>
                  <span class="repo-tree-name mono">{name}</span>
                  {#if entry.size}<span class="repo-tree-size mono">{formatBytes(entry.size)}</span>{/if}
                </button>
              {/if}
            {:else}
              <div class="tab-state">Empty directory.</div>
            {/each}
          </div>

          {#if repoCodePath === '' && repoReadme}
            <div class="repo-readme">
              <div class="repo-readme-head mono">{repoReadme.name}</div>
              <div class="repo-readme-body">
                <Markdown source={repoReadme.content} />
              </div>
            </div>
          {/if}
        {/if}
      </div>
    {:else if repoSection === 'actions'}
      <div class="repo-actions-head">
        <span class="view-sub">Recent workflow runs (last 30)</span>
        <div style="flex:1"></div>
        <button class="icon-btn" onclick={() => loadWorkflowRuns()} title="Refresh" aria-label="Refresh" disabled={workflowRunsLoading}>
          <svg class="i i-sm" viewBox="0 0 24 24"><path d="M21 12a9 9 0 0 1-9 9 9 9 0 0 1-8.5-6" /><path d="M3 12a9 9 0 0 1 9-9 9 9 0 0 1 8.5 6" /><polyline points="21 3 21 9 15 9" /><polyline points="3 21 3 15 9 15" /></svg>
        </button>
      </div>
      <div class="repo-items-list">
        {#if workflowRunsLoading && !workflowRuns.length}
          <div class="tab-state">Loading workflow runs…</div>
        {:else if workflowRunsError}
          <div class="tab-state tab-state--error">{workflowRunsError} <button class="link-inline" onclick={() => loadWorkflowRuns()}>Retry</button></div>
        {:else if workflowRuns.length === 0}
          <div class="tab-state">No workflow runs yet.</div>
        {:else}
          {#each workflowRuns as run (run.id)}
            {@const running = run.status === 'in_progress' || run.status === 'queued' || run.status === 'waiting' || run.status === 'pending'}
            {@const isBusy = runBusy.has(run.id)}
            <div class="repo-run-row-wrap">
              <button class="repo-item-row repo-run-row" onclick={() => openBrowser(run.html_url)}>
                <span class="mini-tag run-status run-status--{run.conclusion ?? run.status}">{run.conclusion ?? run.status}</span>
                <span class="repo-run-name">{run.name}</span>
                <span class="repo-item-title">{run.display_title || '—'}</span>
                <span class="repo-run-branch mono">{run.head_branch}</span>
                <span class="repo-item-id mono">#{run.run_number}</span>
                {#if run.actor_login}
                  <span class="repo-item-author mono">@{run.actor_login}</span>
                {/if}
                <span class="repo-item-time mono">{relativeTime(run.updated_at, now)}</span>
              </button>
              <div class="repo-run-actions">
                {#if running}
                  <button class="repo-run-btn" onclick={() => cancelWorkflow(run.id)} disabled={isBusy} title="Cancel">
                    <svg class="i i-sm" viewBox="0 0 24 24"><rect x="6" y="6" width="12" height="12" rx="2" /></svg>
                  </button>
                {:else}
                  <button class="repo-run-btn" onclick={() => rerunWorkflow(run.id)} disabled={isBusy} title="Re-run">
                    <svg class="i i-sm" viewBox="0 0 24 24"><path d="M3 12a9 9 0 0 1 15-6.7L21 8M21 3v5h-5" /><path d="M21 12a9 9 0 0 1-15 6.7L3 16M3 21v-5h5" /></svg>
                  </button>
                {/if}
              </div>
            </div>
          {/each}
        {/if}
      </div>
    {:else}
      <div class="repo-tabs">
        <button class="repo-tab" class:active={repoStateFilter === 'open'} onclick={() => { repoStateFilter = 'open'; loadRepoItems(); }}>Open</button>
        <button class="repo-tab" class:active={repoStateFilter === 'closed'} onclick={() => { repoStateFilter = 'closed'; loadRepoItems(); }}>Closed</button>
        <button class="repo-tab" class:active={repoStateFilter === 'all'} onclick={() => { repoStateFilter = 'all'; loadRepoItems(); }}>All</button>
        <div style="flex:1"></div>
        <button class="icon-btn" onclick={() => loadRepoItems()} title="Refresh" aria-label="Refresh" disabled={repoItemsLoading}>
          <svg class="i i-sm" viewBox="0 0 24 24" style="transform: rotate({repoItemsLoading ? 360 : 0}deg); transition: transform 0.6s;">
            <path d="M21 12a9 9 0 0 1-9 9 9 9 0 0 1-8.5-6" /><path d="M3 12a9 9 0 0 1 9-9 9 9 0 0 1 8.5 6" />
            <polyline points="21 3 21 9 15 9" /><polyline points="3 21 3 15 9 15" />
          </svg>
        </button>
      </div>

      {@const list = repoSection === 'pulls' ? repoPulls : repoIssues}
      <div class="repo-items-list">
        {#if repoItemsLoading && !repoItems.length}
          <div class="tab-state">Loading…</div>
        {:else if repoItemsError}
          <div class="tab-state tab-state--error">{repoItemsError} <button class="link-inline" onclick={() => loadRepoItems()}>Retry</button></div>
        {:else if list.length === 0}
          <div class="tab-state">No {repoStateFilter} {repoSection === 'pulls' ? 'pull requests' : 'issues'}.</div>
        {:else}
          {#each list as item (item.id)}
            {@const stag = stateTag(item)}
            <button class="repo-item-row" onclick={() => onOpenFocusItem(item)}>
              <span class="mini-tag {stag.className}">{stag.text}</span>
              <span class="repo-item-kind mono">{kindLabel(item)}</span>
              <span class="repo-item-id mono">#{item.number}</span>
              <span class="repo-item-title">{item.title}</span>
              {#if item.author}
                <span class="repo-item-author mono">@{item.author.login}</span>
              {/if}
              <span class="repo-item-time mono">{relativeTime(item.updated_at, now)}</span>
            </button>
          {/each}
        {/if}
      </div>
    {/if}
  </section>
{/if}

<GithubFocusOverlay
  {now}
  {tab}
  {actionBusy}
  onCloseFocus={() => (inboxState.focusItem = null)}
  {onRetryLoadDetail}
  {onTabChange}
  {onToggleFile}
  {onOpenCommit}
  {onOpenComment}
  {onOpenReview}
  {onOpenMerge}
  {onAskClose}
  {onReopen}
  {onOpenBrowser}
  {onOpenCheckDetails}
  {mergeDisabled}
/>

<style>
  /* Shared layout helpers — duplicated from +page.svelte so the component is
     self-contained. They're also kept in the parent because other views use
     them. */
  .full-center { flex: 1; display: flex; align-items: center; justify-content: center; padding: 40px; }
  .empty { display: flex; flex-direction: column; align-items: center; gap: 16px; text-align: center; max-width: 420px; }
  .empty-title { font-size: 22px; font-weight: 600; margin: 12px 0 0; color: var(--text-0); letter-spacing: -0.015em; }
  .empty-sub { font-size: 13.5px; color: var(--text-1); margin: 0; line-height: 1.55; max-width: 380px; }

  .view-title { font-size: 28px; font-weight: 600; letter-spacing: -0.025em; color: var(--text-0); margin-bottom: 10px; }
  .view-sub { font-size: 14px; color: var(--text-2); max-width: 520px; margin: 0 auto; line-height: 1.5; }

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
    color: var(--text-2); background: transparent; transition: all 120ms; border: none; cursor: pointer;
  }
  .icon-btn:hover:not(:disabled) { background: var(--bg-1); color: var(--text-0); }
  .icon-btn:disabled { opacity: 0.4; cursor: not-allowed; }

  .link-inline { display: inline-block; margin-left: 8px; color: var(--accent-bright); font-weight: 500; text-decoration: underline; background: transparent; border: none; cursor: pointer; font-size: inherit; padding: 0; }

  .tab-state { padding: 40px; text-align: center; color: var(--text-2); font-size: 13px; }
  .tab-state--error { color: #fca5a5; }

  .mini-tag { padding: 1px 6px; border-radius: 4px; font-size: 10px; font-weight: 600; text-transform: lowercase; }
  .mini-tag--warn { background: rgba(229, 162, 42, 0.15); color: var(--warning); }

  .i { width: 16px; height: 16px; stroke-width: 2; stroke: currentColor; fill: none; flex-shrink: 0; }
  .i-sm { width: 14px; height: 14px; }

  .mono { font-family: 'JetBrains Mono', ui-monospace, 'SF Mono', monospace; }

  /* Repo-specific styles (moved wholesale from +page.svelte). */
  .repos-view { overflow-y: auto; flex: 1 1 0; min-height: 0; }
  .repos-header { padding: 48px 56px 20px; text-align: center; }
  .repos-body { padding: 20px 56px 100px; max-width: 1100px; margin: 0 auto; width: 100%; }

  .repos-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(320px, 1fr)); gap: 10px; }
  .repo-card {
    padding: 16px 18px;
    background: var(--bg-1); border: 1px solid var(--border-neutral);
    border-radius: 10px; text-align: left;
    transition: all 180ms;
    display: flex; flex-direction: column; gap: 10px;
    cursor: pointer;
  }
  .repo-card:hover {
    background: var(--bg-2); border-color: var(--border-neutral-hi);
    transform: translateY(-2px); box-shadow: 0 8px 24px rgba(0, 0, 0, 0.3);
  }
  .repo-card-head { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .repo-name { font-size: 13px; color: var(--text-0); font-weight: 600; word-break: break-all; }
  .repo-flag {
    font-size: 9.5px; font-weight: 600; text-transform: uppercase; letter-spacing: 0.06em;
    padding: 1px 6px; border-radius: 4px;
    color: var(--text-2); background: var(--bg-3); border: 1px solid var(--border-neutral-hi);
  }
  .repo-flag--archived { color: #fcd34d; border-color: rgba(245, 158, 11, 0.2); background: rgba(245, 158, 11, 0.05); }
  .repo-desc {
    font-size: 12.5px; color: var(--text-1); line-height: 1.5;
    display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; line-clamp: 2;
  }
  .repo-meta {
    display: flex; align-items: center; gap: 12px; font-size: 11px; color: var(--text-2);
    margin-top: auto;
  }
  .repo-lang {
    color: var(--blue-bright); font-weight: 500;
  }
  .repo-stars, .repo-open, .repo-updated { color: var(--text-mute); }

  .repo-detail {
    display: flex; flex-direction: column; overflow: hidden;
    /* `flex: 1 1 0` (basis 0) + `min-height: 0` is the reliable combo for
       "this flex item should fill the remaining parent height and let its
       own child scroll." With `basis: auto`, flex sizes off content, which
       is what caused the whole repo view to overflow the viewport with no
       inner scroll on the file list / README. */
    flex: 1 1 0;
    min-height: 0;
    height: 100%;
  }
  .repo-detail-head {
    padding: 16px 28px;
    border-bottom: 1px solid var(--border-neutral);
    display: flex; align-items: center; gap: 14px;
    flex-shrink: 0;
  }
  .repo-detail-title { font-size: 14px; color: var(--text-0); font-weight: 600; }
  .repo-detail-desc { font-size: 12.5px; color: var(--text-2); }

  .repo-tabs {
    padding: 0 28px;
    border-bottom: 1px solid var(--border-neutral);
    display: flex; align-items: center; gap: 2px;
  }
  .repo-tab {
    padding: 10px 14px; font-size: 12.5px; color: var(--text-2);
    border-bottom: 2px solid transparent; margin-bottom: -1px;
    transition: all 120ms;
    background: transparent; border-left: none; border-right: none; border-top: none;
    cursor: pointer;
  }
  .repo-tab:hover { color: var(--text-0); }
  .repo-tab.active { color: var(--accent-bright); border-bottom-color: var(--accent); }

  .repo-sections {
    padding: 0 28px;
    border-bottom: 1px solid var(--border-neutral);
    display: flex; align-items: center; gap: 4px;
    background: var(--bg-1);
    flex-shrink: 0;
  }
  .repo-section-tab {
    display: inline-flex; align-items: center; gap: 7px;
    padding: 10px 14px; font-size: 12.5px; color: var(--text-1);
    border-bottom: 2px solid transparent; margin-bottom: -1px;
    transition: color 120ms, border-color 120ms;
    background: transparent; border-left: none; border-right: none; border-top: none;
    cursor: pointer;
  }
  .repo-section-tab:hover { color: var(--text-0); }
  .repo-section-tab.active { color: var(--accent-bright); border-bottom-color: var(--accent); }
  .repo-section-count {
    padding: 0 7px; min-width: 18px; height: 18px;
    border-radius: 9px;
    background: var(--bg-3);
    font-size: 10.5px; font-weight: 600;
    display: inline-flex; align-items: center; justify-content: center;
    color: var(--text-1);
  }
  .repo-section-tab.active .repo-section-count { background: var(--accent-soft); color: var(--accent-bright); }

  .repo-code {
    flex: 1 1 0;
    min-height: 0;
    overflow-y: auto;
    padding: 18px 28px 60px;
    display: flex; flex-direction: column; gap: 16px;
  }
  /* Pin every direct child's natural height — the default flex-item behavior
     is `flex: 0 1 auto`, which shrinks items to fit the parent. In a
     flex-column with `overflow-y: auto`, that means "no scroll, just
     squash the tree list". Setting `flex-shrink: 0` lets the total content
     exceed the parent's height so `overflow-y: auto` finally scrolls. */
  .repo-code > * { flex-shrink: 0; }
  .repo-code-meta { display: flex; flex-wrap: wrap; gap: 6px; }
  .repo-code-chip {
    font-size: 11px; padding: 3px 9px; border-radius: 12px;
    background: var(--bg-2); color: var(--text-1);
    border: 1px solid var(--border-neutral);
  }
  .repo-code-chip--warn { background: rgba(229, 162, 42, 0.12); color: var(--warning); border-color: rgba(229, 162, 42, 0.3); }

  /* Breadcrumb + tree listing for the GitHub-style file browser. */
  .repo-path-bar {
    display: flex; align-items: center; flex-wrap: wrap; gap: 4px;
    padding: 8px 12px;
    background: var(--bg-1);
    border: 1px solid var(--border-neutral);
    border-radius: 8px;
    font-size: 12px;
  }
  .repo-crumb {
    padding: 3px 7px; border-radius: 5px;
    color: var(--accent-bright);
    background: transparent;
    border: none; cursor: pointer;
  }
  .repo-crumb:hover { background: var(--bg-2); }
  .repo-crumb--back { color: var(--text-2); }
  .repo-crumb-sep { color: var(--text-mute); padding: 0 2px; }

  .repo-tree-list {
    display: flex; flex-direction: column;
    border: 1px solid var(--border-neutral); border-radius: 10px;
    background: var(--bg-1); overflow: hidden;
  }
  .repo-tree-row {
    display: flex; align-items: center; gap: 10px;
    padding: 8px 14px;
    text-align: left;
    border-bottom: 1px solid var(--border-neutral);
    font-size: 12.5px;
    color: var(--text-0);
    background: transparent;
    transition: background 80ms;
    border-left: none; border-right: none; border-top: none;
    cursor: pointer;
    width: 100%;
  }
  .repo-tree-row:last-child { border-bottom: none; }
  .repo-tree-row:hover:not(:disabled) { background: var(--bg-2); }
  .repo-tree-row:disabled { opacity: 0.5; cursor: default; }
  .repo-tree-icon { color: var(--accent-bright); flex-shrink: 0; }
  .repo-tree-icon--file { color: var(--text-2); }
  .repo-tree-name { flex: 1; }
  .repo-tree-size { color: var(--text-mute); font-size: 10.5px; }

  /* File viewer — simple preformatted source for text blobs, markdown
     rendering for .md. Large files scroll vertically; long lines overflow-x. */
  .repo-file-viewer {
    border: 1px solid var(--border-neutral);
    border-radius: 10px;
    background: var(--bg-1);
    overflow: hidden;
  }
  .repo-file-head {
    display: flex; align-items: center; justify-content: space-between; gap: 12px;
    padding: 8px 14px;
    background: var(--bg-2);
    border-bottom: 1px solid var(--border-neutral);
    font-size: 11.5px;
    color: var(--text-1);
  }
  .repo-file-size { color: var(--text-mute); }
  .repo-file-source {
    margin: 0;
    padding: 14px 18px;
    font-size: 12.5px; line-height: 1.55;
    color: var(--text-0);
    max-height: 70vh; overflow: auto;
    white-space: pre;
    tab-size: 2;
  }
  .repo-readme {
    border: 1px solid var(--border-neutral);
    border-radius: 10px;
    overflow: hidden;
    background: var(--bg-1);
  }
  .repo-readme-head {
    padding: 8px 14px;
    border-bottom: 1px solid var(--border-neutral);
    font-size: 11.5px; color: var(--text-2);
    background: var(--bg-2);
  }
  .repo-readme-body { padding: 16px 22px; }

  .repo-actions-head {
    padding: 10px 28px;
    border-bottom: 1px solid var(--border-neutral);
    display: flex; align-items: center; gap: 10px;
  }
  .repo-run-row {
    grid-template-columns: auto auto 1fr auto auto auto auto !important;
  }
  .repo-run-name { font-size: 12px; color: var(--text-0); font-weight: 500; }
  .repo-run-branch { font-size: 11px; color: var(--text-2); }
  .run-status { text-transform: lowercase; }
  .run-status--success { background: rgba(217, 145, 60, 0.15); color: var(--success); }
  .run-status--failure,
  .run-status--timed_out { background: rgba(214, 72, 44, 0.18); color: var(--error); }
  .run-status--cancelled,
  .run-status--skipped { background: var(--bg-3); color: var(--text-2); }
  .run-status--in_progress,
  .run-status--queued,
  .run-status--waiting,
  .run-status--pending { background: rgba(229, 162, 42, 0.15); color: var(--warning); }
  .run-status--neutral { background: var(--bg-3); color: var(--text-1); }

  .repo-run-row-wrap { position: relative; }
  .repo-run-actions {
    position: absolute; right: 14px; top: 50%; transform: translateY(-50%);
    display: flex; gap: 4px;
    opacity: 0; transition: opacity 120ms;
    pointer-events: none;
  }
  .repo-run-row-wrap:hover .repo-run-actions { opacity: 1; pointer-events: auto; }
  .repo-run-btn {
    display: inline-flex; align-items: center; justify-content: center;
    width: 28px; height: 28px; border-radius: 6px;
    background: var(--bg-2); color: var(--text-1);
    border: 1px solid var(--border-neutral-hi);
    cursor: pointer;
  }
  .repo-run-btn:hover:not(:disabled) { background: var(--bg-3); color: var(--accent-bright); }
  .repo-run-btn:disabled { opacity: 0.4; cursor: default; }

  .repo-release {
    border: 1px solid var(--border-neutral);
    border-radius: 10px;
    padding: 14px 18px;
    margin-bottom: 12px;
    background: var(--bg-1);
  }
  .repo-release-head {
    display: flex; align-items: center; gap: 10px;
    margin-bottom: 10px;
  }
  .repo-release-title { display: inline-flex; align-items: baseline; gap: 10px; text-align: left; }
  .repo-release-tag {
    font-size: 11px; color: var(--accent-bright);
    padding: 3px 8px; border-radius: 4px;
    background: var(--accent-soft);
    border: 1px solid rgba(238, 107, 31, 0.25);
  }
  .repo-release-name { font-size: 13px; color: var(--text-0); font-weight: 600; }
  .repo-release-author { font-size: 11px; color: var(--text-2); }
  .repo-release-avatar { width: 18px; height: 18px; border-radius: 50%; }
  .repo-release-time { font-size: 11px; color: var(--text-mute); }
  .repo-release-body { font-size: 13px; color: var(--text-1); padding-left: 2px; }

  .repo-items-list { flex: 1; overflow-y: auto; padding: 8px 28px 60px; }
  .repo-item-row {
    display: flex; align-items: center; gap: 12px;
    width: 100%; padding: 10px 14px;
    text-align: left; transition: background 120ms;
    border-radius: 8px;
    border: 1px solid transparent;
    background: transparent;
    cursor: pointer;
  }
  .repo-item-row:hover { background: var(--bg-1); border-color: var(--border-neutral); }
  .repo-item-kind { font-size: 10.5px; color: var(--text-mute); text-transform: lowercase; min-width: 34px; }
  .repo-item-id { font-size: 11.5px; color: var(--text-2); min-width: 54px; }
  .repo-item-title { flex: 1; font-size: 13px; color: var(--text-0); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .repo-item-author { font-size: 11px; color: var(--text-2); }
  .repo-item-time { font-size: 10.5px; color: var(--text-mute); min-width: 36px; text-align: right; }
</style>
