<script lang="ts">
  import {
    externalId,
    parsePatch,
    relativeTime,
    type ChangedFile,
    type ClaudeStatus,
    type CommitDetail,
    type CommitEntry,
    type CommitEntry as RepoCommitEntry,
    type ConnectionMeta,
    type ConnectionStatus,
    type JiraIssueType,
    type JiraProject,
    type JiraSprint,
    type JiraStatus,
    type JiraUserSummary,
    type RepoBranch
  } from '../data';
  import { firstLine, restLines } from '../format';
  import { inboxState } from '$lib/state/inbox.svelte';
  import Dropdown, { type DropdownOption } from '$lib/Dropdown.svelte';

  type ReviewEvent = 'APPROVE' | 'REQUEST_CHANGES' | 'COMMENT';
  type MergeMethod = 'merge' | 'squash' | 'rebase';

  type CommitModalState = {
    commit: CommitEntry;
    detail: CommitDetail | null;
    loading: boolean;
    error: string | null;
    expanded: Set<string>;
  };
  type UserPickerModalState = {
    query: string;
    results: JiraUserSummary[];
    loading: boolean;
    error: string | null;
  };
  type JiraModalState = {
    workspace: string;
    email: string;
    token: string;
    error: string | null;
    busy: boolean;
  };
  type ClaudeModalState = { status: ClaudeStatus | null; loading: boolean };
  type PatModalState = { conn: ConnectionMeta; token: string; error: string | null; busy: boolean };
  type CommentModalState = { body: string; busy: boolean; error: string | null };
  type ReviewModalState = { event: ReviewEvent; body: string; busy: boolean; error: string | null };
  type MergeModalState = { method: MergeMethod; busy: boolean; error: string | null };
  type ConfirmModalState = {
    title: string;
    body: string;
    confirmText: string;
    danger?: boolean;
    busy: boolean;
    onConfirm: () => Promise<void>;
  };
  type JiraCreateModalState = {
    projectKey: string;
    projects: JiraProject[];
    projectsLoading: boolean;
    issueTypes: JiraIssueType[];
    issueTypeName: string;
    summary: string;
    description: string;
    assigneeAccountId: string;
    sprints: JiraSprint[];
    sprintId: number | null;
    busy: boolean;
    error: string | null;
  };
  type GithubCreatePrModalState = {
    repo: string; // "owner/name"
    repos: { owner: string; name: string; full_name: string; default_branch?: string | null }[];
    reposLoading: boolean;
    branches: RepoBranch[];
    branchesLoading: boolean;
    base: string;
    head: string;
    title: string;
    body: string;
    draft: boolean;
    compare: {
      loading: boolean;
      error: string | null;
      total_commits: number;
      ahead_by: number;
      behind_by: number;
      additions: number;
      deletions: number;
      commits: RepoCommitEntry[];
      files: ChangedFile[];
    } | null;
    filesExpanded: boolean;
    busy: boolean;
    error: string | null;
  };

  interface Props {
    // Bindable modal state
    commitModal: CommitModalState | null;
    userPickerModal: UserPickerModalState | null;
    jiraModal: JiraModalState | null;
    claudeModal: ClaudeModalState | null;
    patModal: PatModalState | null;
    commentModal: CommentModalState | null;
    reviewModal: ReviewModalState | null;
    mergeModal: MergeModalState | null;
    confirmModal: ConfirmModalState | null;
    jiraCreateModal: JiraCreateModalState | null;
    githubCreatePrModal: GithubCreatePrModalState | null;

    // Read-only context
    now: number;
    githubStatus: ConnectionStatus;
    jiraStatus: JiraStatus;

    // Callbacks
    toggleCommitFile: (filename: string) => void;
    openBrowser: (url: string) => void | Promise<void>;
    onUserPickerInput: (q: string) => void;
    selectJiraUser: (u: JiraUserSummary | null) => void | Promise<void>;
    selectAnyJiraUser: () => void | Promise<void>;
    submitJira: () => void | Promise<void>;
    jiraTokenUrl: () => string;
    refreshClaudeModal: () => void | Promise<void>;
    claudeInstallUrl: () => string;
    submitPat: () => void | Promise<void>;
    githubTokenUrl: () => string;
    submitComment: () => void | Promise<void>;
    submitReview: () => void | Promise<void>;
    submitMerge: () => void | Promise<void>;
    runConfirm: () => void | Promise<void>;
    onJiraCreateProjectChange: (key: string) => void | Promise<void>;
    submitJiraCreate: () => void | Promise<void>;
    onGithubPrRepoChange: (full: string) => void | Promise<void>;
    onGithubPrBranchesChange: () => void | Promise<void>;
    submitGithubPr: () => void | Promise<void>;
  }

  let {
    commitModal = $bindable(),
    userPickerModal = $bindable(),
    jiraModal = $bindable(),
    claudeModal = $bindable(),
    patModal = $bindable(),
    commentModal = $bindable(),
    reviewModal = $bindable(),
    mergeModal = $bindable(),
    confirmModal = $bindable(),
    jiraCreateModal = $bindable(),
    githubCreatePrModal = $bindable(),
    now,
    githubStatus,
    jiraStatus,
    toggleCommitFile,
    openBrowser,
    onUserPickerInput,
    selectJiraUser,
    selectAnyJiraUser,
    submitJira,
    jiraTokenUrl,
    refreshClaudeModal,
    claudeInstallUrl,
    submitPat,
    githubTokenUrl,
    submitComment,
    submitReview,
    submitMerge,
    runConfirm,
    onJiraCreateProjectChange,
    submitJiraCreate,
    onGithubPrRepoChange,
    onGithubPrBranchesChange,
    submitGithubPr
  }: Props = $props();
</script>

{#if commitModal}
  {@const cm = commitModal}
  <div class="modal-backdrop" onclick={(e) => { if (e.target === e.currentTarget) commitModal = null; }} role="dialog" aria-modal="true" tabindex="-1">
    <div class="modal modal-xl">
      <header class="modal-head">
        {#if cm.commit.author_avatar}
          <img src={cm.commit.author_avatar} alt="" class="meta-avatar" style="width:32px; height:32px;" />
        {/if}
        <div>
          <div class="modal-title">{firstLine(cm.commit.message)}</div>
          <div class="modal-sub">
            <span class="mono">{cm.commit.short_sha}</span>
            · {cm.commit.author_login ? '@' + cm.commit.author_login : cm.commit.author_name}
            · {relativeTime(cm.commit.author_date, now)} ago
          </div>
        </div>
        <button class="modal-close" onclick={() => commitModal = null} aria-label="Close">
          <svg class="i" viewBox="0 0 24 24"><path d="M18 6 6 18M6 6l12 12" /></svg>
        </button>
      </header>
      <div class="modal-body-scroll">
        {#if cm.loading}
          <div class="tab-state">Loading commit…</div>
        {:else if cm.error}
          <div class="tab-state tab-state--error">{cm.error}</div>
        {:else if cm.detail}
          {@const d = cm.detail}
          {#if restLines(d.message)}
            <div class="commit-rest" style="padding: 0 20px 16px;">{restLines(d.message)}</div>
          {/if}
          <div class="files-summary mono" style="padding: 0 20px;">
            {d.files.length} file{d.files.length !== 1 ? 's' : ''} ·
            <span class="chg-add">+{d.additions}</span>
            <span class="chg-del">−{d.deletions}</span>
          </div>
          <div style="padding: 0 20px 20px;">
            {#each d.files as f (f.filename)}
              {@const open = cm.expanded.has(f.filename)}
              <div class="file-block" class:open>
                <button class="file-head" onclick={() => toggleCommitFile(f.filename)}>
                  <svg class="i i-sm chev" viewBox="0 0 24 24" style="transform: rotate({open ? 90 : 0}deg);"><path d="m9 18 6-6-6-6" /></svg>
                  <span class="file-status file-status--{f.status}">{f.status}</span>
                  <span class="file-name mono">{f.filename}</span>
                  <span class="file-changes mono">
                    <span class="chg-add">+{f.additions}</span>
                    <span class="chg-del">−{f.deletions}</span>
                  </span>
                </button>
                {#if open}
                  {#if f.patch}
                    {@const lines = parsePatch(f.patch)}
                    <div class="diff-scroller">
                      <div class="diff-body">
                        {#each lines as line, idx (idx)}
                          {#if line.kind === 'header'}
                            <div class="hunk-header mono">{line.text}</div>
                          {:else}
                            <div class="diff-line {line.kind}">
                              <span class="diff-line-num">{line.kind === 'add' ? '+' : line.kind === 'del' ? '−' : line.newLine ?? ''}</span>
                              <span class="diff-line-content">{line.text}</span>
                            </div>
                          {/if}
                        {/each}
                      </div>
                    </div>
                  {:else}
                    <div class="tab-state">Binary file or no patch available.</div>
                  {/if}
                {/if}
              </div>
            {/each}
          </div>
        {/if}
      </div>
      <footer class="modal-foot">
        <button class="btn btn--ghost" onclick={() => openBrowser(cm.commit.url)}>
          <svg class="i i-sm" viewBox="0 0 24 24"><path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6" /><path d="M15 3h6v6M10 14 21 3" /></svg>
          Open on GitHub
        </button>
        <div style="flex:1"></div>
        <button class="btn btn--primary" onclick={() => commitModal = null}>Close</button>
      </footer>
    </div>
  </div>
{/if}

<!-- Jira user picker -->
{#if userPickerModal}
  {@const m = userPickerModal}
  <div class="modal-backdrop" onclick={(e) => { if (e.target === e.currentTarget) userPickerModal = null; }} role="dialog" aria-modal="true" tabindex="-1">
    <div class="modal modal-wide">
      <header class="modal-head">
        <svg class="i" viewBox="0 0 24 24" style="color: var(--blue-bright)"><path d="M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2"/><circle cx="12" cy="7" r="4"/></svg>
        <div>
          <div class="modal-title">Choose assignee</div>
          <div class="modal-sub">Filter Jira by any user in the workspace</div>
        </div>
        <button class="modal-close" onclick={() => userPickerModal = null} aria-label="Close">
          <svg class="i" viewBox="0 0 24 24"><path d="M18 6 6 18M6 6l12 12" /></svg>
        </button>
      </header>
      <div class="modal-body">
        <!-- svelte-ignore a11y_autofocus -->
        <input
          class="field-input"
          type="search"
          placeholder="Search by name or email…"
          value={m.query}
          oninput={(e) => onUserPickerInput((e.currentTarget as HTMLInputElement).value)}
          autofocus
        />

        <div class="user-list">
          <button class="user-row" class:active={inboxState.jiraAssigneeAny} onclick={() => selectAnyJiraUser()}>
            <span class="user-row-avatar user-row-avatar--any" aria-hidden="true">
              <svg class="i i-sm" viewBox="0 0 24 24"><circle cx="9" cy="7" r="4"/><circle cx="17" cy="7" r="3"/><path d="M3 21v-2a4 4 0 0 1 4-4h4a4 4 0 0 1 4 4v2M15 14h2a4 4 0 0 1 4 4v3"/></svg>
            </span>
            <div class="user-row-body">
              <div class="user-row-name">Any user</div>
              <div class="user-row-email">No assignee filter — everyone's tickets</div>
            </div>
            {#if inboxState.jiraAssigneeAny}
              <svg class="i i-sm" viewBox="0 0 24 24" style="color: var(--accent-bright); margin-left:auto;"><path d="M20 6 9 17l-5-5"/></svg>
            {/if}
          </button>
          <button class="user-row" class:active={inboxState.jiraAssignee === null && !inboxState.jiraAssigneeAny} onclick={() => selectJiraUser(null)}>
            <span class="chip-avatar" style="width:28px; height:28px; border-radius:50%;"></span>
            <div class="user-row-body">
              <div class="user-row-name">Me (authenticated account)</div>
              <div class="user-row-email">{jiraStatus.kind === 'connected' ? jiraStatus.user.display_name : ''}</div>
            </div>
          </button>

          {#if m.loading}
            <div class="tab-state">Searching…</div>
          {:else if m.error}
            <div class="tab-state tab-state--error">{m.error}</div>
          {:else if m.results.length === 0 && m.query.trim()}
            <div class="tab-state">No users found.</div>
          {:else}
            {#each m.results as u (u.account_id)}
              <button
                class="user-row"
                class:active={inboxState.jiraAssignee?.account_id === u.account_id}
                onclick={() => selectJiraUser(u)}
              >
                <img src={u.avatar_url} alt="" class="user-row-avatar" />
                <div class="user-row-body">
                  <div class="user-row-name">{u.display_name}</div>
                  {#if u.email_address}<div class="user-row-email">{u.email_address}</div>{/if}
                </div>
                {#if inboxState.jiraAssignee?.account_id === u.account_id}
                  <svg class="i i-sm" viewBox="0 0 24 24" style="color: var(--accent-bright); margin-left:auto;"><path d="M20 6 9 17l-5-5"/></svg>
                {/if}
              </button>
            {/each}
          {/if}
        </div>
      </div>
      <footer class="modal-foot">
        <button class="btn btn--ghost" onclick={() => userPickerModal = null}>Close</button>
      </footer>
    </div>
  </div>
{/if}

<!-- Jira Modal -->
{#if jiraModal}
  {@const m = jiraModal}
  <div class="modal-backdrop" onclick={(e) => { if (e.target === e.currentTarget && !m.busy) jiraModal = null; }} role="dialog" aria-modal="true" tabindex="-1">
    <div class="modal modal-wide">
      <header class="modal-head">
        <span class="conn-icon conn-icon--jira">J</span>
        <div>
          <div class="modal-title">Connect Jira</div>
          <div class="modal-sub">Atlassian Cloud · email + API token</div>
        </div>
        <button class="modal-close" onclick={() => jiraModal = null} disabled={m.busy} aria-label="Close">
          <svg class="i" viewBox="0 0 24 24"><path d="M18 6 6 18M6 6l12 12" /></svg>
        </button>
      </header>
      <div class="modal-body">
        <p class="modal-copy">Basic auth with an API token. Token is stored in your macOS Keychain, not on disk.</p>
        <button class="link-btn" onclick={() => openBrowser(jiraTokenUrl())}>
          <svg class="i i-sm" viewBox="0 0 24 24"><path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"/><path d="M15 3h6v6M10 14 21 3"/></svg>
          Create an API token on id.atlassian.com
        </button>

        <label class="field">
          <span class="field-label">Workspace</span>
          <!-- svelte-ignore a11y_autofocus -->
          <input class="field-input mono" type="text" bind:value={m.workspace} placeholder="acme.atlassian.net" autofocus disabled={m.busy} />
        </label>
        <label class="field">
          <span class="field-label">Email</span>
          <input class="field-input" type="email" bind:value={m.email} placeholder="you@acme.com" disabled={m.busy} />
        </label>
        <label class="field">
          <span class="field-label">API Token</span>
          <input
            class="field-input mono"
            type="password"
            bind:value={m.token}
            placeholder="ATATT3x…"
            disabled={m.busy}
            onkeydown={(e) => { if (e.key === 'Enter' && !m.busy && m.workspace.trim() && m.email.trim() && m.token.trim()) submitJira(); }}
          />
        </label>

        {#if m.error}<div class="modal-error">{m.error}</div>{/if}
      </div>
      <footer class="modal-foot">
        <button class="btn btn--ghost" onclick={() => jiraModal = null} disabled={m.busy}>Cancel</button>
        <button class="btn btn--primary" onclick={submitJira} disabled={m.busy || !m.workspace.trim() || !m.email.trim() || !m.token.trim()}>
          {m.busy ? 'Verifying…' : 'Connect'}
        </button>
      </footer>
    </div>
  </div>
{/if}

<!-- Claude Modal -->
{#if claudeModal}
  {@const cm = claudeModal}
  {@const s = cm.status}
  <div class="modal-backdrop" onclick={(e) => { if (e.target === e.currentTarget && !cm.loading) claudeModal = null; }} role="dialog" aria-modal="true" tabindex="-1">
    <div class="modal modal-wide">
      <header class="modal-head">
        <span class="conn-icon conn-icon--claude">C</span>
        <div>
          <div class="modal-title">Claude Code</div>
          <div class="modal-sub">Authentication managed by the <code class="mono">claude</code> CLI</div>
        </div>
        <button class="modal-close" onclick={() => claudeModal = null} disabled={cm.loading} aria-label="Close">
          <svg class="i" viewBox="0 0 24 24"><path d="M18 6 6 18M6 6l12 12" /></svg>
        </button>
      </header>
      <div class="modal-body">
        {#if cm.loading || !s}
          <div class="tab-state">Detecting…</div>
        {:else}
          <div class="claude-detect">
            <div class="detect-row" class:ok={s.detected}>
              <span class="detect-dot"></span>
              <div class="detect-main">
                <div class="detect-title">claude CLI</div>
                <div class="detect-sub mono">
                  {#if s.detected}
                    {s.path}{#if s.version} · {s.version}{/if}
                  {:else}
                    not found on PATH
                  {/if}
                </div>
              </div>
            </div>
            <div class="detect-row" class:ok={s.has_config_dir || s.has_api_key_env}>
              <span class="detect-dot"></span>
              <div class="detect-main">
                <div class="detect-title">Authentication</div>
                <div class="detect-sub">
                  {#if s.has_api_key_env}
                    <code class="mono">ANTHROPIC_API_KEY</code> env var set — using API key billing
                  {:else if s.has_config_dir}
                    <code class="mono">~/.claude</code> exists — signed in via subscription (Claude Max / Pro)
                  {:else}
                    not authenticated yet
                  {/if}
                </div>
              </div>
            </div>
          </div>

          {#if !s.detected}
            <div class="claude-hint">
              <div class="claude-hint-title">Install the CLI first</div>
              <div class="claude-hint-body">
                <code class="mono claude-install">curl -fsSL https://claude.ai/install.sh | bash</code>
                <p class="modal-copy" style="margin-top: 10px;">Or see the official docs:</p>
                <button class="link-btn" onclick={() => openBrowser(claudeInstallUrl())}>
                  <svg class="i i-sm" viewBox="0 0 24 24"><path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"/><path d="M15 3h6v6M10 14 21 3"/></svg>
                  Claude Code documentation
                </button>
              </div>
            </div>
          {:else if !s.ready}
            <div class="claude-hint">
              <div class="claude-hint-title">Sign in to your Claude subscription</div>
              <div class="claude-hint-body">
                <p class="modal-copy">Run this once in your terminal — it opens a browser, you sign in with your Max / Pro plan, done.</p>
                <code class="mono claude-install">claude login</code>
                <p class="modal-copy" style="margin-top: 10px; color: var(--text-2);">
                  Prefer to bill via the API instead? Export <code class="mono">ANTHROPIC_API_KEY</code>.
                </p>
              </div>
            </div>
          {:else}
            <div class="claude-ok">
              <svg class="i" viewBox="0 0 24 24" style="color: var(--accent-bright); width: 22px; height: 22px;"><path d="M20 6 9 17l-5-5"/></svg>
              <div>
                <div class="detect-title">Ready to run agents.</div>
                <div class="detect-sub">Forgehold will use this CLI for Claude Code runs. Re-check any time.</div>
              </div>
            </div>
          {/if}
        {/if}
      </div>
      <footer class="modal-foot">
        <button class="btn btn--ghost" onclick={refreshClaudeModal} disabled={cm.loading}>
          {cm.loading ? 'Checking…' : 'Re-check'}
        </button>
        <div style="flex:1"></div>
        <button class="btn btn--primary" onclick={() => claudeModal = null} disabled={cm.loading}>Close</button>
      </footer>
    </div>
  </div>
{/if}

<!-- PAT Modal -->
{#if patModal}
  {@const m = patModal}
  <div class="modal-backdrop" onclick={(e) => { if (e.target === e.currentTarget && !m.busy) patModal = null; }} role="dialog" aria-modal="true" tabindex="-1">
    <div class="modal">
      <header class="modal-head">
        <span class="conn-icon {m.conn.iconClass}" class:conn-icon--svg={!!m.conn.iconSvg}>
          {#if m.conn.iconSvg}
            <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">{@html m.conn.iconSvg}</svg>
          {:else}
            {m.conn.iconLetters}
          {/if}
        </span>
        <div>
          <div class="modal-title">Connect {m.conn.name}</div>
          <div class="modal-sub">Personal access token — stored in macOS Keychain</div>
        </div>
        <button class="modal-close" onclick={() => patModal = null} disabled={m.busy} aria-label="Close">
          <svg class="i" viewBox="0 0 24 24"><path d="M18 6 6 18M6 6l12 12" /></svg>
        </button>
      </header>
      <div class="modal-body">
        {#if m.conn.id === 'github'}
          <p class="modal-copy">Need a token? Create one with scopes <code class="mono">repo</code>, <code class="mono">read:user</code>, <code class="mono">read:org</code>.</p>
          <button class="link-btn" onclick={() => openBrowser(githubTokenUrl())}>
            <svg class="i i-sm" viewBox="0 0 24 24"><path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6" /><path d="M15 3h6v6M10 14 21 3" /></svg>
            Open token page on GitHub
          </button>
        {/if}
        <label class="field">
          <span class="field-label">Token</span>
          <!-- svelte-ignore a11y_autofocus -->
          <input class="field-input mono" type="password" bind:value={m.token} placeholder="ghp_…" autofocus disabled={m.busy} onkeydown={(e) => { if (e.key === 'Enter' && !m.busy && m.token.trim()) submitPat(); }} />
        </label>
        {#if m.error}<div class="modal-error">{m.error}</div>{/if}
      </div>
      <footer class="modal-foot">
        <button class="btn btn--ghost" onclick={() => patModal = null} disabled={m.busy}>Cancel</button>
        <button class="btn btn--primary" onclick={submitPat} disabled={m.busy || !m.token.trim()}>{m.busy ? 'Verifying…' : 'Connect'}</button>
      </footer>
    </div>
  </div>
{/if}

<!-- Comment Modal -->
{#if commentModal}
  {@const m = commentModal}
  <div class="modal-backdrop" onclick={(e) => { if (e.target === e.currentTarget && !m.busy) commentModal = null; }} role="dialog" aria-modal="true" tabindex="-1">
    <div class="modal modal-wide">
      <header class="modal-head">
        <svg class="i" viewBox="0 0 24 24" style="color: var(--text-1)"><path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z" /></svg>
        <div>
          <div class="modal-title">Add comment</div>
          <div class="modal-sub">posts as @{githubStatus.kind === 'connected' ? githubStatus.user.login : ''}</div>
        </div>
        <button class="modal-close" onclick={() => commentModal = null} disabled={m.busy} aria-label="Close">
          <svg class="i" viewBox="0 0 24 24"><path d="M18 6 6 18M6 6l12 12" /></svg>
        </button>
      </header>
      <div class="modal-body">
        <!-- svelte-ignore a11y_autofocus -->
        <textarea class="field-textarea" bind:value={m.body} placeholder="Write your comment (markdown supported)…" autofocus disabled={m.busy}></textarea>
        {#if m.error}<div class="modal-error">{m.error}</div>{/if}
      </div>
      <footer class="modal-foot">
        <button class="btn btn--ghost" onclick={() => commentModal = null} disabled={m.busy}>Cancel</button>
        <button class="btn btn--primary" onclick={submitComment} disabled={m.busy || !m.body.trim()}>{m.busy ? 'Posting…' : 'Comment'}</button>
      </footer>
    </div>
  </div>
{/if}

<!-- Review Modal -->
{#if reviewModal}
  {@const m = reviewModal}
  <div class="modal-backdrop" onclick={(e) => { if (e.target === e.currentTarget && !m.busy) reviewModal = null; }} role="dialog" aria-modal="true" tabindex="-1">
    <div class="modal modal-wide">
      <header class="modal-head">
        <svg class="i" viewBox="0 0 24 24" style="color: var(--text-1)"><path d="M20 6 9 17l-5-5" /></svg>
        <div>
          <div class="modal-title">Submit review</div>
          <div class="modal-sub">on {inboxState.focusItem ? externalId(inboxState.focusItem) : ''}</div>
        </div>
        <button class="modal-close" onclick={() => reviewModal = null} disabled={m.busy} aria-label="Close">
          <svg class="i" viewBox="0 0 24 24"><path d="M18 6 6 18M6 6l12 12" /></svg>
        </button>
      </header>
      <div class="modal-body">
        <div class="radio-group">
          <label class="radio" class:selected={m.event === 'APPROVE'}>
            <input type="radio" bind:group={m.event} value="APPROVE" disabled={m.busy} />
            <div><div class="radio-title">Approve</div><div class="radio-desc">Submit approval</div></div>
          </label>
          <label class="radio" class:selected={m.event === 'REQUEST_CHANGES'}>
            <input type="radio" bind:group={m.event} value="REQUEST_CHANGES" disabled={m.busy} />
            <div><div class="radio-title">Request changes</div><div class="radio-desc">Requires author updates</div></div>
          </label>
          <label class="radio" class:selected={m.event === 'COMMENT'}>
            <input type="radio" bind:group={m.event} value="COMMENT" disabled={m.busy} />
            <div><div class="radio-title">Comment</div><div class="radio-desc">General feedback without approval</div></div>
          </label>
        </div>
        <!-- svelte-ignore a11y_autofocus -->
        <textarea class="field-textarea" bind:value={m.body} placeholder={m.event === 'APPROVE' ? 'Optional: a word on why (markdown supported)' : 'Review body (markdown supported)'} autofocus disabled={m.busy}></textarea>
        {#if m.error}<div class="modal-error">{m.error}</div>{/if}
      </div>
      <footer class="modal-foot">
        <button class="btn btn--ghost" onclick={() => reviewModal = null} disabled={m.busy}>Cancel</button>
        <button class="btn btn--primary" onclick={submitReview} disabled={m.busy || (m.event !== 'APPROVE' && !m.body.trim())}>{m.busy ? 'Submitting…' : 'Submit review'}</button>
      </footer>
    </div>
  </div>
{/if}

<!-- Merge Modal -->
{#if mergeModal}
  {@const m = mergeModal}
  <div class="modal-backdrop" onclick={(e) => { if (e.target === e.currentTarget && !m.busy) mergeModal = null; }} role="dialog" aria-modal="true" tabindex="-1">
    <div class="modal">
      <header class="modal-head">
        <svg class="i" viewBox="0 0 24 24" style="color: var(--accent-bright)"><circle cx="18" cy="18" r="3" /><circle cx="6" cy="6" r="3" /><path d="M6 9v6a6 6 0 0 0 6 6h2" /></svg>
        <div>
          <div class="modal-title">Merge pull request</div>
          <div class="modal-sub">{inboxState.focusItem ? externalId(inboxState.focusItem) : ''}{inboxState.prDetail ? ` · ${inboxState.prDetail.base_ref} ← ${inboxState.prDetail.head_ref}` : ''}</div>
        </div>
        <button class="modal-close" onclick={() => mergeModal = null} disabled={m.busy} aria-label="Close">
          <svg class="i" viewBox="0 0 24 24"><path d="M18 6 6 18M6 6l12 12" /></svg>
        </button>
      </header>
      <div class="modal-body">
        <div class="radio-group">
          <label class="radio" class:selected={m.method === 'squash'}>
            <input type="radio" bind:group={m.method} value="squash" disabled={m.busy} />
            <div><div class="radio-title">Squash and merge</div><div class="radio-desc">All commits combined into one</div></div>
          </label>
          <label class="radio" class:selected={m.method === 'merge'}>
            <input type="radio" bind:group={m.method} value="merge" disabled={m.busy} />
            <div><div class="radio-title">Create a merge commit</div><div class="radio-desc">All commits added with a merge commit</div></div>
          </label>
          <label class="radio" class:selected={m.method === 'rebase'}>
            <input type="radio" bind:group={m.method} value="rebase" disabled={m.busy} />
            <div><div class="radio-title">Rebase and merge</div><div class="radio-desc">Apply commits on top of base</div></div>
          </label>
        </div>
        {#if m.error}<div class="modal-error">{m.error}</div>{/if}
      </div>
      <footer class="modal-foot">
        <button class="btn btn--ghost" onclick={() => mergeModal = null} disabled={m.busy}>Cancel</button>
        <button class="btn btn--primary" onclick={submitMerge} disabled={m.busy}>{m.busy ? 'Merging…' : 'Confirm merge'}</button>
      </footer>
    </div>
  </div>
{/if}

<!-- Jira Create Issue Modal -->
{#if jiraCreateModal}
  {@const m = jiraCreateModal}
  {@const canSubmit = !m.busy && m.projectKey.trim() && m.issueTypeName.trim() && m.summary.trim()}
  {@const projectOpts = [
    { value: '', label: 'Select project…' },
    ...m.projects.map((p) => ({ value: p.key, label: `${p.key} · ${p.name}` }))
  ] as DropdownOption<string>[]}
  {@const issueTypeOpts = (m.issueTypes.length === 0
    ? [
        { value: 'Task', label: 'Task' },
        { value: 'Bug', label: 'Bug' },
        { value: 'Story', label: 'Story' }
      ]
    : m.issueTypes.map((t) => ({ value: t.name, label: t.name }))) as DropdownOption<string>[]}
  {@const sprintOpts = [
    { value: '', label: 'No sprint' },
    ...m.sprints.map((s) => ({ value: String(s.id), label: s.name, hint: s.state }))
  ] as DropdownOption<string>[]}
  <div class="modal-backdrop" onclick={(e) => { if (e.target === e.currentTarget && !m.busy) jiraCreateModal = null; }} role="dialog" aria-modal="true" tabindex="-1">
    <div class="modal modal-wide">
      <header class="modal-head">
        <span class="conn-icon conn-icon--jira">J</span>
        <div>
          <div class="modal-title">New Jira issue</div>
          <div class="modal-sub">{jiraStatus.kind === 'connected' ? jiraStatus.user.workspace : ''}</div>
        </div>
        <button class="modal-close" onclick={() => jiraCreateModal = null} disabled={m.busy} aria-label="Close">
          <svg class="i" viewBox="0 0 24 24"><path d="M18 6 6 18M6 6l12 12" /></svg>
        </button>
      </header>
      <div class="modal-body">
        <div class="grid-2">
          <label class="field">
            <span class="field-label">Project</span>
            <Dropdown
              value={m.projectKey}
              options={projectOpts}
              onChange={(v) => onJiraCreateProjectChange(v)}
              disabled={m.busy}
              ariaLabel="Project"
              placeholder={m.projectsLoading ? 'Loading…' : 'Select project…'}
              width="100%"
            />
          </label>
          <label class="field">
            <span class="field-label">Issue type</span>
            <Dropdown
              value={m.issueTypeName}
              options={issueTypeOpts}
              onChange={(v) => { if (jiraCreateModal) jiraCreateModal = { ...jiraCreateModal, issueTypeName: v }; }}
              disabled={m.busy}
              ariaLabel="Issue type"
              width="100%"
            />
          </label>
        </div>
        <label class="field">
          <span class="field-label">Summary</span>
          <!-- svelte-ignore a11y_autofocus -->
          <input class="field-input" type="text" bind:value={m.summary} placeholder="Short, one-line title" disabled={m.busy} autofocus />
        </label>
        <label class="field">
          <span class="field-label">Description (markdown)</span>
          <textarea class="field-textarea" bind:value={m.description} placeholder="Optional — supports markdown paragraphs" disabled={m.busy}></textarea>
        </label>
        <div class="grid-2">
          <label class="field">
            <span class="field-label">Assignee account id (optional)</span>
            <input class="field-input mono" type="text" bind:value={m.assigneeAccountId} placeholder="leave blank to unassign" disabled={m.busy} />
          </label>
          <label class="field">
            <span class="field-label">Sprint</span>
            <Dropdown
              value={m.sprintId == null ? '' : String(m.sprintId)}
              options={sprintOpts}
              onChange={(v) => {
                if (jiraCreateModal) jiraCreateModal = { ...jiraCreateModal, sprintId: v ? Number(v) : null };
              }}
              disabled={m.busy || m.sprints.length === 0}
              ariaLabel="Sprint"
              width="100%"
            />
          </label>
        </div>
        {#if m.error}<div class="modal-error">{m.error}</div>{/if}
      </div>
      <footer class="modal-foot">
        <button class="btn btn--ghost" onclick={() => jiraCreateModal = null} disabled={m.busy}>Cancel</button>
        <button class="btn btn--primary" onclick={submitJiraCreate} disabled={!canSubmit}>
          {m.busy ? 'Creating…' : 'Create issue'}
        </button>
      </footer>
    </div>
  </div>
{/if}

<!-- GitHub Create PR Modal -->
{#if githubCreatePrModal}
  {@const m = githubCreatePrModal}
  {@const canSubmit = !m.busy && m.repo && m.title.trim() && m.base && m.head && m.base !== m.head}
  {@const prRepoOpts = [
    { value: '', label: 'Select repository…' },
    ...m.repos.map((r) => ({ value: r.full_name, label: r.full_name }))
  ] as DropdownOption<string>[]}
  {@const prBranchOpts = [
    { value: '', label: m.branches.length ? 'Select…' : 'No branches' },
    ...m.branches.map((b) => ({ value: b.name, label: b.name }))
  ] as DropdownOption<string>[]}
  <div class="modal-backdrop" onclick={(e) => { if (e.target === e.currentTarget && !m.busy) githubCreatePrModal = null; }} role="dialog" aria-modal="true" tabindex="-1">
    <div class="modal modal-xl">
      <header class="modal-head">
        <span class="conn-icon" style="background: #0a111e; color: #fff; border: 1px solid var(--border-neutral-hi);">PR</span>
        <div>
          <div class="modal-title">New pull request</div>
          <div class="modal-sub">{m.repo || 'pick a repository'}</div>
        </div>
        <button class="modal-close" onclick={() => githubCreatePrModal = null} disabled={m.busy} aria-label="Close">
          <svg class="i" viewBox="0 0 24 24"><path d="M18 6 6 18M6 6l12 12" /></svg>
        </button>
      </header>
      <div class="modal-body-scroll" style="padding: 0;">
        <div class="modal-body" style="padding-bottom: 6px;">
          <label class="field">
            <span class="field-label">Repository</span>
            <Dropdown
              value={m.repo}
              options={prRepoOpts}
              onChange={(v) => onGithubPrRepoChange(v)}
              disabled={m.busy}
              ariaLabel="Repository"
              placeholder={m.reposLoading ? 'Loading…' : 'Select repository…'}
              width="100%"
            />
          </label>
          <div class="grid-2">
            <label class="field">
              <span class="field-label">Base branch</span>
              <Dropdown
                value={m.base}
                options={prBranchOpts}
                onChange={(v) => {
                  if (githubCreatePrModal) {
                    githubCreatePrModal = { ...githubCreatePrModal, base: v };
                    void onGithubPrBranchesChange();
                  }
                }}
                disabled={m.busy || m.branches.length === 0}
                ariaLabel="Base branch"
                placeholder="Select base…"
                width="100%"
              />
            </label>
            <label class="field">
              <span class="field-label">Head branch</span>
              <Dropdown
                value={m.head}
                options={prBranchOpts}
                onChange={(v) => {
                  if (githubCreatePrModal) {
                    githubCreatePrModal = { ...githubCreatePrModal, head: v };
                    void onGithubPrBranchesChange();
                  }
                }}
                disabled={m.busy || m.branches.length === 0}
                ariaLabel="Head branch"
                placeholder="Select head…"
                width="100%"
              />
            </label>
          </div>
          {#if m.base && m.head && m.base === m.head}
            <div class="modal-error">Head branch must differ from base branch.</div>
          {/if}
          <label class="field">
            <span class="field-label">Title</span>
            <input class="field-input" type="text" bind:value={m.title} placeholder="Pull request title" disabled={m.busy} />
          </label>
          <label class="field">
            <span class="field-label">Description (markdown)</span>
            <textarea class="field-textarea" bind:value={m.body} placeholder="What does this PR do? How did you test it?" disabled={m.busy}></textarea>
          </label>
          <label class="radio" style="align-items: center;">
            <input type="checkbox" bind:checked={m.draft} disabled={m.busy} />
            <div>
              <div class="radio-title">Create as draft</div>
              <div class="radio-desc">Opens the PR in draft state — can't be merged until marked ready.</div>
            </div>
          </label>
        </div>

        {#if m.compare}
          <div class="pr-compare">
            {#if m.compare.loading}
              <div class="tab-state">Comparing branches…</div>
            {:else if m.compare.error}
              <div class="tab-state tab-state--error">{m.compare.error}</div>
            {:else}
              <div class="pr-compare-summary">
                <span><span class="mono">{m.compare.total_commits}</span> commit{m.compare.total_commits === 1 ? '' : 's'}</span>
                <span>·</span>
                <span><span class="mono">{m.compare.files.length}</span> file{m.compare.files.length === 1 ? '' : 's'} changed</span>
                <span>·</span>
                <span class="chg-add">+{m.compare.additions}</span>
                <span class="chg-del">−{m.compare.deletions}</span>
              </div>
              {#if m.compare.commits.length}
                <div class="pr-compare-section">
                  <div class="pr-compare-section-label">Commits</div>
                  <div class="pr-commits">
                    {#each m.compare.commits as c (c.sha)}
                      <div class="pr-commit">
                        {#if c.author_avatar}
                          <img src={c.author_avatar} alt="" class="pr-commit-avatar" />
                        {:else}
                          <span class="pr-commit-avatar placeholder"></span>
                        {/if}
                        <div class="pr-commit-body">
                          <div class="pr-commit-msg">{firstLine(c.message)}</div>
                          <div class="pr-commit-meta">
                            <span class="mono">{c.short_sha}</span>
                            · {c.author_login ? '@' + c.author_login : c.author_name}
                            · {relativeTime(c.author_date, now)} ago
                          </div>
                        </div>
                      </div>
                    {/each}
                  </div>
                </div>
              {/if}
              {#if m.compare.files.length}
                <div class="pr-compare-section">
                  <button
                    type="button"
                    class="pr-files-toggle"
                    onclick={() => { if (githubCreatePrModal) githubCreatePrModal = { ...githubCreatePrModal, filesExpanded: !githubCreatePrModal.filesExpanded }; }}
                  >
                    <svg class="i i-sm chev" viewBox="0 0 24 24" style="transform: rotate({m.filesExpanded ? 90 : 0}deg);"><path d="m9 18 6-6-6-6" /></svg>
                    <span class="pr-compare-section-label">Changed files ({m.compare.files.length})</span>
                  </button>
                  {#if m.filesExpanded}
                    <div class="pr-files">
                      {#each m.compare.files as f (f.filename)}
                        <div class="pr-file">
                          <span class="file-status file-status--{f.status}">{f.status}</span>
                          <span class="file-name mono">{f.filename}</span>
                          <span class="file-changes mono">
                            <span class="chg-add">+{f.additions}</span>
                            <span class="chg-del">−{f.deletions}</span>
                          </span>
                        </div>
                      {/each}
                    </div>
                  {/if}
                </div>
              {/if}
            {/if}
          </div>
        {/if}

        {#if m.error}
          <div class="modal-body" style="padding-top: 0;">
            <div class="modal-error">{m.error}</div>
          </div>
        {/if}
      </div>
      <footer class="modal-foot">
        <button class="btn btn--ghost" onclick={() => githubCreatePrModal = null} disabled={m.busy}>Cancel</button>
        <button class="btn btn--primary" onclick={submitGithubPr} disabled={!canSubmit}>
          {m.busy ? 'Creating…' : (m.draft ? 'Create draft PR' : 'Create PR')}
        </button>
      </footer>
    </div>
  </div>
{/if}

<!-- Confirm Modal -->
{#if confirmModal}
  {@const c = confirmModal}
  <div class="modal-backdrop" onclick={(e) => { if (e.target === e.currentTarget && !c.busy) confirmModal = null; }} role="dialog" aria-modal="true" tabindex="-1">
    <div class="modal">
      <header class="modal-head">
        <svg class="i" viewBox="0 0 24 24" style="color: {c.danger ? '#fca5a5' : 'var(--accent-bright)'}">
          {#if c.danger}
            <circle cx="12" cy="12" r="10" /><line x1="12" y1="8" x2="12" y2="12" /><line x1="12" y1="16" x2="12.01" y2="16" />
          {:else}
            <path d="M20 6 9 17l-5-5" />
          {/if}
        </svg>
        <div>
          <div class="modal-title">{c.title}</div>
          <div class="modal-sub">{c.body}</div>
        </div>
        <button class="modal-close" onclick={() => confirmModal = null} disabled={c.busy} aria-label="Close">
          <svg class="i" viewBox="0 0 24 24"><path d="M18 6 6 18M6 6l12 12" /></svg>
        </button>
      </header>
      <footer class="modal-foot">
        <button class="btn btn--ghost" onclick={() => confirmModal = null} disabled={c.busy}>Cancel</button>
        <button class="btn {c.danger ? 'btn--danger' : 'btn--primary'}" onclick={runConfirm} disabled={c.busy}>
          {c.busy ? 'Working…' : c.confirmText}
        </button>
      </footer>
    </div>
  </div>
{/if}

<style>
  /* Modal shell */
  .modal-backdrop {
    position: fixed; inset: 0;
    background: rgba(10, 17, 30, 0.78);
    backdrop-filter: blur(20px);
    display: flex; align-items: center; justify-content: center;
    z-index: 210;
    animation: fadeIn 180ms ease-out;
  }
  .modal {
    width: 480px; max-width: 90vw;
    background: var(--bg-1); border: 1px solid var(--border-hi2); border-radius: 14px;
    box-shadow: 0 30px 80px rgba(0, 0, 0, 0.6), inset 0 1px 0 rgba(255, 255, 255, 0.04);
    overflow: hidden;
    animation: slideDown 220ms cubic-bezier(0.34, 1.56, 0.64, 1);
    display: flex; flex-direction: column;
    max-height: 90vh;
  }
  .modal-wide { width: 600px; }
  .modal-xl { width: min(1040px, 96vw); max-height: 92vh; }
  .modal-head {
    display: flex; align-items: center; gap: 12px;
    padding: 20px 20px 14px;
    border-bottom: 1px solid var(--border-neutral);
    flex-shrink: 0;
  }
  .modal-title { font-size: 15px; font-weight: 600; color: var(--text-0); }
  .modal-sub { font-size: 12px; color: var(--text-2); margin-top: 2px; }
  .modal-close {
    margin-left: auto; width: 28px; height: 28px;
    border-radius: 7px; color: var(--text-2);
    display: inline-flex; align-items: center; justify-content: center;
    transition: all 120ms;
    background: none; border: none; cursor: pointer;
  }
  .modal-close:hover:not(:disabled) { background: var(--bg-2); color: var(--text-0); }

  .modal-body { padding: 18px 20px; display: flex; flex-direction: column; gap: 14px; }
  .modal-body-scroll { flex: 1; overflow-y: auto; padding: 18px 0; }
  .modal-copy { font-size: 12.5px; color: var(--text-1); margin: 0; line-height: 1.55; }
  .modal-copy code {
    background: var(--bg-2); padding: 1px 6px; border-radius: 4px; font-size: 11.5px;
    border: 1px solid var(--border-neutral-hi); color: var(--accent-bright);
  }

  .modal-error {
    font-size: 12px; color: #fca5a5;
    background: rgba(239, 68, 68, 0.08);
    border: 1px solid rgba(239, 68, 68, 0.22);
    border-radius: 7px;
    padding: 8px 10px;
  }

  .modal-foot {
    display: flex; justify-content: flex-end; gap: 8px;
    padding: 14px 20px;
    border-top: 1px solid var(--border-neutral);
    background: var(--bg-0);
    flex-shrink: 0;
  }

  /* Buttons */
  .btn {
    padding: 8px 14px; border-radius: 8px;
    font-size: 12.5px; font-weight: 500;
    transition: all 120ms;
    display: inline-flex; align-items: center; gap: 6px;
    background: none; border: none; cursor: pointer; color: inherit;
  }
  .btn--ghost { color: var(--text-1); background: transparent; border: 1px solid var(--border-neutral-hi); }
  .btn--ghost:hover:not(:disabled) { background: var(--bg-1); color: var(--text-0); border-color: var(--border-hi2); }
  .btn--primary {
    color: #0a111e;
    background: linear-gradient(135deg, #34d399, #10b981);
    box-shadow: 0 4px 14px rgba(16, 185, 129, 0.25), inset 0 1px 0 rgba(255, 255, 255, 0.2);
    font-weight: 600;
  }
  .btn--primary:hover:not(:disabled) {
    transform: translateY(-1px);
    box-shadow: 0 6px 20px rgba(16, 185, 129, 0.35), inset 0 1px 0 rgba(255, 255, 255, 0.25);
  }
  .btn--danger {
    color: #ffe4e4;
    background: linear-gradient(135deg, #ef4444, #b91c1c);
    border: 1px solid rgba(239, 68, 68, 0.4);
    box-shadow: 0 4px 14px rgba(239, 68, 68, 0.22), inset 0 1px 0 rgba(255, 255, 255, 0.14);
    font-weight: 600;
  }
  .btn--danger:hover:not(:disabled) {
    transform: translateY(-1px);
    box-shadow: 0 6px 20px rgba(239, 68, 68, 0.32), inset 0 1px 0 rgba(255, 255, 255, 0.2);
  }
  .btn:disabled { opacity: 0.5; cursor: not-allowed; }

  .link-btn {
    align-self: flex-start;
    display: inline-flex; align-items: center; gap: 6px;
    padding: 7px 12px; border-radius: 7px;
    background: var(--bg-2); border: 1px solid var(--border-neutral-hi);
    color: var(--accent-bright);
    font-size: 12px; font-weight: 500;
    transition: all 120ms;
    cursor: pointer;
  }
  .link-btn:hover { border-color: var(--border-hi); background: var(--bg-3); }

  /* Fields */
  .field { display: flex; flex-direction: column; gap: 6px; }
  .field-label { font-size: 11px; font-weight: 600; color: var(--text-2); text-transform: uppercase; letter-spacing: 0.08em; }
  .field-input {
    padding: 10px 12px;
    background: var(--bg-0); border: 1px solid var(--border-neutral-hi); border-radius: 8px;
    color: var(--text-0); font-size: 13px;
    transition: border-color 120ms;
  }
  .field-input:focus { border-color: var(--accent); outline: none; }
  .field-input:disabled { opacity: 0.5; }

  .field-textarea {
    padding: 12px 14px;
    background: var(--bg-0); border: 1px solid var(--border-neutral-hi); border-radius: 8px;
    color: var(--text-0); font-size: 13px; font-family: inherit;
    min-height: 140px; resize: vertical;
    transition: border-color 120ms;
  }
  .field-textarea:focus { border-color: var(--accent); outline: none; }
  .field-textarea:disabled { opacity: 0.5; }

  /* Radios */
  .radio-group { display: flex; flex-direction: column; gap: 6px; }
  .radio {
    display: flex; align-items: flex-start; gap: 10px;
    padding: 10px 12px;
    background: var(--bg-0); border: 1px solid var(--border-neutral-hi);
    border-radius: 8px; cursor: pointer;
    transition: all 120ms;
  }
  .radio:hover { border-color: var(--border-hi); background: var(--bg-2); }
  .radio.selected { border-color: var(--accent); background: var(--accent-soft); }
  .radio input { margin-top: 3px; accent-color: var(--accent); }
  .radio-title { font-size: 13px; font-weight: 500; color: var(--text-0); }
  .radio-desc { font-size: 11.5px; color: var(--text-2); margin-top: 2px; }

  /* Connection icon (base; color variants live globally in app.css/routes) */
  .conn-icon {
    width: 36px; height: 36px; border-radius: 9px;
    display: inline-flex; align-items: center; justify-content: center;
    font-size: 13px; font-weight: 700; letter-spacing: -0.02em; flex-shrink: 0;
  }
  .conn-icon--svg svg {
    width: 20px; height: 20px;
    color: currentColor;
    display: block;
  }

  /* User picker */
  .chip-avatar {
    width: 20px; height: 20px;
    border-radius: 50%;
    object-fit: cover;
    flex-shrink: 0;
    background: linear-gradient(135deg, #5aa2ff, #8b96ab);
  }
  .user-list {
    display: flex; flex-direction: column; gap: 4px;
    max-height: 420px;
    overflow-y: auto;
    padding-right: 4px;
  }
  .user-row {
    display: flex; align-items: center; gap: 12px;
    padding: 10px 12px;
    background: var(--bg-0);
    border: 1px solid var(--border-neutral-hi);
    border-radius: 9px;
    transition: all 120ms;
    text-align: left;
    width: 100%;
    cursor: pointer;
  }
  .user-row:hover { background: var(--bg-2); border-color: var(--border-hi); }
  .user-row.active {
    border-color: var(--accent);
    background: var(--accent-soft);
  }
  .user-row-avatar {
    width: 32px; height: 32px; border-radius: 50%;
    flex-shrink: 0;
  }
  .user-row-avatar--any {
    display: inline-flex; align-items: center; justify-content: center;
    background: var(--accent-soft);
    border: 1px solid rgba(232, 163, 58, 0.25);
    color: var(--accent-bright);
  }
  .user-row-avatar--any :global(svg) { width: 16px; height: 16px; }
  .user-row-body { flex: 1; min-width: 0; }
  .user-row-name { font-size: 13px; color: var(--text-0); font-weight: 500; }
  .user-row-email { font-size: 11.5px; color: var(--text-2); margin-top: 2px; }

  /* Claude detect */
  .claude-detect { display: flex; flex-direction: column; gap: 10px; }
  .detect-row {
    display: flex; align-items: flex-start; gap: 12px;
    padding: 12px 14px;
    background: var(--bg-0);
    border: 1px solid var(--border-neutral-hi);
    border-radius: 9px;
  }
  .detect-dot {
    width: 10px; height: 10px; border-radius: 50%;
    background: var(--text-mute);
    margin-top: 4px;
    flex-shrink: 0;
  }
  .detect-row.ok { border-color: rgba(16, 185, 129, 0.24); background: rgba(16, 185, 129, 0.04); }
  .detect-row.ok .detect-dot { background: var(--accent-bright); box-shadow: 0 0 8px var(--accent-glow); }
  .detect-main { flex: 1; min-width: 0; }
  .detect-title { font-size: 13px; color: var(--text-0); font-weight: 500; margin-bottom: 2px; }
  .detect-sub { font-size: 11.5px; color: var(--text-2); word-break: break-all; line-height: 1.5; }
  .detect-sub code {
    background: var(--bg-2); padding: 0 5px; border-radius: 3px;
    color: var(--accent-bright); font-size: 11px;
  }

  .claude-hint {
    margin-top: 6px;
    padding: 14px 16px;
    background: var(--bg-0);
    border: 1px solid var(--border-neutral-hi);
    border-radius: 9px;
  }
  .claude-hint-title {
    font-size: 12.5px; font-weight: 600; color: var(--text-0);
    margin-bottom: 10px;
  }
  .claude-hint-body { display: flex; flex-direction: column; gap: 2px; }
  .claude-install {
    display: block;
    padding: 10px 12px;
    background: var(--bg-2);
    border: 1px solid var(--border-neutral-hi);
    border-radius: 7px;
    font-size: 12px;
    color: var(--accent-bright);
    user-select: all;
  }
  .claude-ok {
    display: flex; align-items: center; gap: 12px;
    padding: 14px 16px;
    background: var(--accent-soft);
    border: 1px solid rgba(16, 185, 129, 0.28);
    border-radius: 9px;
  }

  /* Commit modal body */
  .meta-avatar { width: 18px; height: 18px; border-radius: 50%; }
  .chg-add { color: var(--accent-bright); }
  .chg-del { color: #fca5a5; }

  .tab-state { padding: 40px; text-align: center; color: var(--text-2); font-size: 13px; }
  .tab-state--error { color: #fca5a5; }

  .commit-rest {
    font-size: 12px; color: var(--text-2); margin-top: 3px;
    white-space: pre-wrap;
    font-family: 'JetBrains Mono', monospace; line-height: 1.5;
    max-height: 80px; overflow: hidden;
  }
  .files-summary { font-size: 12px; color: var(--text-2); margin-bottom: 12px; padding: 0 2px; }

  .file-block {
    background: var(--bg-1); border: 1px solid var(--border-neutral);
    border-radius: 8px; margin-bottom: 6px; overflow: hidden;
  }
  .file-head {
    display: flex; align-items: center; gap: 10px;
    width: 100%; padding: 10px 14px;
    text-align: left; transition: background 120ms;
    background: none; border: none; cursor: pointer; color: inherit;
  }
  .file-head:hover { background: var(--bg-2); }
  .chev { color: var(--text-2); transition: transform 160ms; }
  .file-status {
    font-size: 10px; font-weight: 700; text-transform: uppercase;
    letter-spacing: 0.06em; padding: 2px 7px; border-radius: 4px;
  }
  .file-status--added    { color: var(--accent-bright); background: var(--accent-soft); border: 1px solid rgba(16, 185, 129, 0.24); }
  .file-status--modified { color: var(--blue-bright); background: rgba(59, 130, 246, 0.08); border: 1px solid rgba(59, 130, 246, 0.22); }
  .file-status--removed  { color: #fca5a5; background: rgba(239, 68, 68, 0.08); border: 1px solid rgba(239, 68, 68, 0.22); }
  .file-status--renamed  { color: #fcd34d; background: rgba(245, 158, 11, 0.06); border: 1px solid rgba(245, 158, 11, 0.22); }
  .file-name { flex: 1; font-size: 12.5px; color: var(--text-0); overflow-wrap: anywhere; }
  .file-changes { display: inline-flex; gap: 8px; font-size: 11px; }

  /* Diff */
  .diff-scroller {
    border-top: 1px solid var(--border-neutral);
    overflow-x: auto;
    max-height: 640px;
    overflow-y: auto;
    background: var(--bg-0);
  }
  .diff-body {
    font-family: 'JetBrains Mono', ui-monospace, monospace;
    font-size: 12px; line-height: 1.65;
    width: fit-content; min-width: 100%;
  }
  .hunk-header {
    padding: 4px 16px;
    font-size: 11px; color: var(--text-mute);
    background: rgba(15, 24, 40, 0.6);
    border-top: 1px solid var(--border-neutral);
    border-bottom: 1px solid var(--border-neutral);
  }
  .diff-line {
    display: grid; grid-template-columns: 44px 1fr;
  }
  .diff-line-num {
    text-align: right; padding: 0 10px;
    color: var(--text-mute); font-size: 10.5px;
    user-select: none;
    background: var(--bg-0); border-right: 1px solid var(--border-neutral);
    position: sticky; left: 0;
  }
  .diff-line-content { padding: 0 14px; white-space: pre; color: var(--text-1); }
  .diff-line.add .diff-line-content { background: rgba(16, 185, 129, 0.08); color: #6ee7b7; }
  .diff-line.add .diff-line-num { background: rgba(16, 185, 129, 0.12); color: #6ee7b7; }
  .diff-line.del .diff-line-content { background: rgba(239, 68, 68, 0.07); color: #fca5a5; }
  .diff-line.del .diff-line-num { background: rgba(239, 68, 68, 0.1); color: #fca5a5; }

  @keyframes fadeIn { from { opacity: 0; } to { opacity: 1; } }
  @keyframes slideDown {
    from { opacity: 0; transform: translateY(-8px) scale(0.99); }
    to   { opacity: 1; transform: translateY(0) scale(1); }
  }

  /* Two-column layout for paired fields inside the create modals */
  .grid-2 { display: grid; grid-template-columns: 1fr 1fr; gap: 12px; }

  /* PR compare preview */
  .pr-compare {
    padding: 6px 20px 18px;
    display: flex; flex-direction: column; gap: 12px;
  }
  .pr-compare-summary {
    display: flex; align-items: center; gap: 8px;
    padding: 10px 12px;
    background: var(--bg-0);
    border: 1px solid var(--border-neutral-hi);
    border-radius: 8px;
    font-size: 12px; color: var(--text-1);
  }
  .pr-compare-section { display: flex; flex-direction: column; gap: 6px; }
  .pr-compare-section-label {
    font-size: 10.5px; font-weight: 700; color: var(--text-2);
    text-transform: uppercase; letter-spacing: 0.08em;
  }
  .pr-files-toggle {
    display: flex; align-items: center; gap: 6px;
    background: none; border: none; padding: 0;
    color: inherit; cursor: pointer;
  }
  .pr-commits { display: flex; flex-direction: column; gap: 4px; max-height: 260px; overflow-y: auto; }
  .pr-commit {
    display: flex; align-items: flex-start; gap: 10px;
    padding: 8px 10px;
    background: var(--bg-0);
    border: 1px solid var(--border-neutral);
    border-radius: 8px;
  }
  .pr-commit-avatar {
    width: 20px; height: 20px;
    border-radius: 50%;
    object-fit: cover;
    flex-shrink: 0;
  }
  .pr-commit-avatar.placeholder { background: var(--bg-2); }
  .pr-commit-body { flex: 1; min-width: 0; }
  .pr-commit-msg { font-size: 12.5px; color: var(--text-0); line-height: 1.4; word-break: break-word; }
  .pr-commit-meta { font-size: 10.5px; color: var(--text-2); margin-top: 2px; }
  .pr-files { display: flex; flex-direction: column; gap: 4px; max-height: 320px; overflow-y: auto; }
  .pr-file {
    display: flex; align-items: center; gap: 10px;
    padding: 6px 10px;
    background: var(--bg-0);
    border: 1px solid var(--border-neutral);
    border-radius: 6px;
  }
</style>
