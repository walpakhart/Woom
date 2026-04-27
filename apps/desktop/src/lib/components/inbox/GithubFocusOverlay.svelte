<script lang="ts">
  import Markdown from '$lib/components/ui/Markdown.svelte';
  import {
    externalId,
    kindLabel,
    parsePatch,
    relativeTime,
    repoLabel,
    reviewStateLabel,
    stateTag,
    type CheckRun,
    type CommitEntry,
    type ReviewComment
  } from '$lib/data';
  import { firstLine, labelColorStyle, restLines } from '$lib/format';
  import { inboxState } from '$lib/state/inbox.svelte';

  type DetailTab = 'conversation' | 'commits' | 'files' | 'reviews' | 'checks';

  interface Props {
    now: number;
    tab: DetailTab;
    actionBusy: string | null;
    onCloseFocus: () => void;
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
    now,
    tab,
    actionBusy,
    onCloseFocus,
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

  /** Roll a check run's combined {status, conclusion} down to a single key
      the UI can map to an icon + color. Mirrors GitHub's own summary badge. */
  function checkState(c: CheckRun): 'success' | 'failure' | 'pending' | 'neutral' | 'skipped' | 'cancelled' {
    if (c.status !== 'completed') return 'pending';
    switch (c.conclusion) {
      case 'success':
        return 'success';
      case 'failure':
      case 'timed_out':
      case 'action_required':
        return 'failure';
      case 'skipped':
        return 'skipped';
      case 'cancelled':
        return 'cancelled';
      default:
        return 'neutral';
    }
  }

  // Group review comments by file path — used by the Files tab to pin each
  // comment to its file.
  const reviewCommentsByPath = $derived.by(() => {
    const map = new Map<string, ReviewComment[]>();
    for (const c of inboxState.reviewComments) {
      const arr = map.get(c.path) ?? [];
      arr.push(c);
      map.set(c.path, arr);
    }
    return map;
  });

  // Group review comments by parent review id — lets us show a useful
  // summary on "umbrella" reviews that wrap inline comments.
  const reviewCommentsByReview = $derived.by(() => {
    const map = new Map<number, ReviewComment[]>();
    for (const c of inboxState.reviewComments) {
      if (c.pull_request_review_id == null) continue;
      const arr = map.get(c.pull_request_review_id) ?? [];
      arr.push(c);
      map.set(c.pull_request_review_id, arr);
    }
    return map;
  });

  // Per-review expansion state for the inline comments. The conversation
  // tab renders an "X inline comments on Y files" pill — clicking it
  // expands the comments inline (file path + line + body) right under
  // the review block, instead of jumping to the Files tab. A small
  // "Open in Files →" link still gives the old behaviour for users who
  // want the diff context.
  let expandedReviews = $state(new Set<number>());
  function toggleReviewExpansion(id: number) {
    const next = new Set(expandedReviews);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    expandedReviews = next;
  }

  // Rolled-up counts for the Checks tab badge and summary row.
  const prChecksSummary = $derived.by(() => {
    const counts = {
      total: inboxState.prChecks.length,
      success: 0,
      failure: 0,
      pending: 0,
      neutral: 0,
      skipped: 0,
      cancelled: 0
    };
    for (const c of inboxState.prChecks) counts[checkState(c)] += 1;
    return counts;
  });
</script>

{#if inboxState.focusItem}
  {@const item = inboxState.focusItem}
  {@const stag = stateTag(item)}
  <div class="slide-over" onclick={(e) => { if (e.target === e.currentTarget) onCloseFocus(); }} onkeydown={(e) => { if (e.key === 'Escape') onCloseFocus(); }} role="dialog" aria-modal="true" tabindex="-1">
    <div class="slide-panel">
      <!-- Unified header bar: close-left, metadata center, "Open on
           GitHub" right. Mirrors `.jdp-head` (Jira) and `.sdp-head`
           (Sentry) so all three detail panes share the same skeleton. -->
      <header class="gfo-head">
        <button class="gfo-back" onclick={onCloseFocus} aria-label="Close" title="Close">
          <svg class="i i-sm" viewBox="0 0 24 24"><path d="M18 6 6 18M6 6l12 12" /></svg>
        </button>
        <span class="gfo-key mono">{externalId(item)}</span>
        <span class="chip-state {stag.className}">{stag.text}</span>
        <span class="gfo-kind">{kindLabel(item).toLowerCase()}</span>
        {#if item.repo}
          <span class="gfo-repo mono" title={repoLabel(item)}>{repoLabel(item)}</span>
        {/if}
        {#if item.is_pull_request && inboxState.prDetail}
          <span class="gfo-branches mono">{inboxState.prDetail.base_ref} ← {inboxState.prDetail.head_ref}</span>
        {/if}
        <div style="flex:1"></div>
        <button
          class="gfo-btn gfo-btn--icon"
          onclick={onRetryLoadDetail}
          disabled={inboxState.detailLoading}
          title="Refresh PR detail (PR/files/commits/checks)"
          aria-label="Refresh"
        >
          <svg class="i i-sm" class:gfo-spin={inboxState.detailLoading} viewBox="0 0 24 24">
            <path d="M21 12a9 9 0 0 1-9 9 9 9 0 0 1-8.5-6"/>
            <path d="M3 12a9 9 0 0 1 9-9 9 9 0 0 1 8.5 6"/>
            <polyline points="21 3 21 9 15 9"/>
            <polyline points="3 21 3 15 9 15"/>
          </svg>
        </button>
        <button class="gfo-btn" onclick={() => onOpenBrowser(item.url)} title="Open on GitHub">
          <svg class="i i-sm" viewBox="0 0 24 24"><path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"/><path d="M15 3h6v6M10 14 21 3"/></svg>
          Open on GitHub
        </button>
      </header>
      <div class="focus-scroll">
        <div class="focus-shell">
          <h1 class="focus-title">{item.title}</h1>

          {#if item.labels.length}
            <div class="focus-labels">
              {#each item.labels as label (label.name)}
                <span class="label-chip" style={labelColorStyle(label.color)}>{label.name}</span>
              {/each}
            </div>
          {/if}

          <div class="focus-meta">
            {#if item.author}
              <span class="focus-meta-item">
                <img src={item.author.avatar_url} alt="" class="meta-avatar" />
                <span class="mono">@{item.author.login}</span>
              </span>
              <span class="divider"></span>
            {/if}
            <span class="focus-meta-item">{item.comments} comments</span>
            {#if item.is_pull_request && inboxState.prDetail}
              <span class="divider"></span>
              <span class="focus-meta-item">
                <span class="chg-add">+{inboxState.prDetail.additions}</span>
                <span class="chg-del">−{inboxState.prDetail.deletions}</span>
              </span>
            {/if}
            <span class="divider"></span>
            <span class="focus-meta-item">opened {relativeTime(item.created_at, now)} ago</span>
          </div>

          <div class="detail-tabs">
            <button class="detail-tab" class:active={tab === 'conversation'} onclick={() => onTabChange('conversation')}>
              Conversation
              {#if inboxState.comments.length + inboxState.prReviews.length > 0}<span class="tab-count">{inboxState.comments.length + inboxState.prReviews.length}</span>{/if}
            </button>
            {#if item.is_pull_request}
              <button class="detail-tab" class:active={tab === 'commits'} onclick={() => onTabChange('commits')}>
                Commits{#if inboxState.prCommits.length}<span class="tab-count">{inboxState.prCommits.length}</span>{/if}
              </button>
              <button class="detail-tab" class:active={tab === 'files'} onclick={() => onTabChange('files')}>
                Files{#if inboxState.prFiles.length}<span class="tab-count">{inboxState.prFiles.length}</span>{/if}
              </button>
              <button class="detail-tab" class:active={tab === 'checks'} onclick={() => onTabChange('checks')}>
                Checks
                {#if prChecksSummary.total > 0}
                  <span class="tab-count tab-count--{prChecksSummary.failure > 0 ? 'err' : prChecksSummary.pending > 0 ? 'pending' : 'ok'}">
                    {#if prChecksSummary.failure > 0}
                      ✗ {prChecksSummary.failure}
                    {:else if prChecksSummary.pending > 0}
                      … {prChecksSummary.pending}/{prChecksSummary.total}
                    {:else}
                      ✓ {prChecksSummary.total}
                    {/if}
                  </span>
                {/if}
              </button>
              <button class="detail-tab" class:active={tab === 'reviews'} onclick={() => onTabChange('reviews')}>
                Reviews{#if inboxState.prReviews.length}<span class="tab-count">{inboxState.prReviews.length}</span>{/if}
              </button>
            {/if}
          </div>

          {#if inboxState.detailError}
            <div class="tab-error">Failed to load detail: {inboxState.detailError}
              <button class="link-inline" onclick={onRetryLoadDetail}>Retry</button>
            </div>
          {/if}

          {#if tab === 'conversation'}
            <div class="tab-pane">
              {#if item.body}
                <div class="body-card">
                  <div class="body-head">
                    {#if item.author}
                      <img src={item.author.avatar_url} alt="" class="meta-avatar" />
                      <span class="mono">@{item.author.login}</span>
                    {/if}
                    <span class="meta-time mono">{relativeTime(item.created_at, now)} ago</span>
                  </div>
                  <Markdown source={item.body} />
                </div>
              {:else}
                <div class="body-empty">No description.</div>
              {/if}

              {#if inboxState.detailLoading && !inboxState.comments.length && !inboxState.prReviews.length}
                <div class="tab-state">Loading conversation…</div>
              {:else}
                {@const timeline = [
                  ...inboxState.prReviews.map((r) => ({ type: 'review' as const, at: r.submitted_at ?? '', data: r, key: `review-${r.id}` })),
                  ...inboxState.comments.map((c) => ({ type: 'comment' as const, at: c.created_at, data: c, key: `comment-${c.id}` })),
                  // Commits get interleaved into the same timeline by their
                  // author_date, so reviewers' "approved / changes requested"
                  // bubbles surface next to the SHAs that triggered them —
                  // matches GitHub's own conversation pane layout.
                  ...inboxState.prCommits.map((c) => ({ type: 'commit' as const, at: c.author_date, data: c, key: `commit-${c.sha}` }))
                ].sort((a, b) => a.at.localeCompare(b.at))}
                {#each timeline as entry (entry.key)}
                  {#if entry.type === 'review'}
                    {@const r = entry.data}
                    {@const rl = reviewStateLabel(r.state)}
                    {@const inline = reviewCommentsByReview.get(r.id) ?? []}
                    <div class="timeline-item review-item {rl.className}">
                      <div class="timeline-head">
                        {#if r.user}<img src={r.user.avatar_url} alt="" class="meta-avatar" /><span class="mono">@{r.user.login}</span>{/if}
                        <span class="review-state">{rl.text}</span>
                        {#if r.submitted_at}<span class="meta-time mono">{relativeTime(r.submitted_at, now)} ago</span>{/if}
                      </div>
                      {#if r.body}
                        <Markdown source={r.body} />
                      {:else if inline.length === 0}
                        <div class="review-empty">No written feedback.</div>
                      {/if}
                      {#if inline.length > 0}
                        {@const expanded = expandedReviews.has(r.id)}
                        {@const fileCount = new Set(inline.map((c) => c.path)).size}
                        <button class="review-inline-link" onclick={() => toggleReviewExpansion(r.id)} aria-expanded={expanded}>
                          <svg class="i i-sm review-chevron" class:review-chevron--open={expanded} viewBox="0 0 24 24"><path d="m9 18 6-6-6-6"/></svg>
                          {inline.length} inline comment{inline.length > 1 ? 's' : ''} on {fileCount} file{fileCount > 1 ? 's' : ''}
                        </button>
                        {#if expanded}
                          <div class="inline-comments inline-comments--review">
                            {#each inline as ic (ic.id)}
                              <div class="inline-comment">
                                <div class="inline-comment-head">
                                  {#if ic.user}<img src={ic.user.avatar_url} alt="" class="meta-avatar" /><span class="mono">@{ic.user.login}</span>{/if}
                                  <span class="inline-path mono" title={ic.path}>{ic.path}</span>
                                  {#if ic.line}<span class="inline-line mono">L{ic.line}</span>{/if}
                                  <span class="meta-time mono">{relativeTime(ic.created_at, now)} ago</span>
                                </div>
                                <Markdown source={ic.body} />
                              </div>
                            {/each}
                            <button class="review-inline-link review-inline-link--secondary" onclick={() => onTabChange('files')}>
                              <svg class="i i-sm" viewBox="0 0 24 24"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><path d="M14 2v6h6"/></svg>
                              Open in Files →
                            </button>
                          </div>
                        {/if}
                      {/if}
                    </div>
                  {:else if entry.type === 'commit'}
                    {@const cm = entry.data}
                    <!-- Slim commit row matching GitHub's conversation
                         timeline shape: tiny git icon, subject only (body
                         omitted — same as GH), author chip, short SHA
                         linking to the commit on github.com. -->
                    <div class="timeline-commit">
                      <span class="timeline-commit-icon" aria-hidden="true">
                        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="3.5"/><path d="M3 12h5.5M15.5 12H21"/></svg>
                      </span>
                      <span class="timeline-commit-msg" title={cm.message}>{cm.message.split('\n')[0]}</span>
                      {#if cm.author_avatar}
                        <img class="meta-avatar timeline-commit-avatar" src={cm.author_avatar} alt="" />
                      {/if}
                      <span class="meta-time mono">{relativeTime(cm.author_date, now)} ago</span>
                      <a class="timeline-commit-sha mono" href={cm.url} target="_blank" rel="noopener noreferrer" title="Open commit on GitHub">{cm.short_sha}</a>
                    </div>
                  {:else}
                    {@const c = entry.data}
                    <div class="timeline-item">
                      <div class="timeline-head">
                        {#if c.user}<img src={c.user.avatar_url} alt="" class="meta-avatar" /><span class="mono">@{c.user.login}</span>{/if}
                        <span class="review-state">commented</span>
                        <span class="meta-time mono">{relativeTime(c.created_at, now)} ago</span>
                      </div>
                      <Markdown source={c.body} />
                    </div>
                  {/if}
                {/each}
              {/if}
            </div>

          {:else if tab === 'commits' && item.is_pull_request}
            <div class="tab-pane">
              {#if inboxState.detailLoading && !inboxState.prCommits.length}
                <div class="tab-state">Loading commits…</div>
              {:else if inboxState.prCommits.length === 0}
                <div class="tab-state">No commits.</div>
              {:else}
                {#each inboxState.prCommits as c (c.sha)}
                  <button class="commit-row" onclick={() => onOpenCommit(c)}>
                    <div class="commit-main">
                      {#if c.author_avatar}<img src={c.author_avatar} alt="" class="meta-avatar" />{/if}
                      <div class="commit-body">
                        <div class="commit-title">{firstLine(c.message)}</div>
                        {#if restLines(c.message)}
                          <div class="commit-rest">{restLines(c.message)}</div>
                        {/if}
                        <div class="commit-meta mono">
                          {c.author_login ? '@' + c.author_login : c.author_name}
                          <span>·</span>
                          <span>{relativeTime(c.author_date, now)} ago</span>
                        </div>
                      </div>
                    </div>
                    <span class="commit-sha mono">{c.short_sha}</span>
                  </button>
                {/each}
              {/if}
            </div>

          {:else if tab === 'files' && item.is_pull_request}
            <div class="tab-pane">
              {#if inboxState.detailLoading && !inboxState.prFiles.length}
                <div class="tab-state">Loading files…</div>
              {:else if inboxState.prFiles.length === 0}
                <div class="tab-state">No changed files.</div>
              {:else}
                <div class="files-summary mono">
                  {inboxState.prFiles.length} file{inboxState.prFiles.length !== 1 ? 's' : ''} ·
                  <span class="chg-add">+{inboxState.prFiles.reduce((a, f) => a + f.additions, 0)}</span>
                  <span class="chg-del">−{inboxState.prFiles.reduce((a, f) => a + f.deletions, 0)}</span>
                  <span> · {inboxState.reviewComments.length} inline comment{inboxState.reviewComments.length !== 1 ? 's' : ''}</span>
                </div>
                {#each inboxState.prFiles as f (f.filename)}
                  {@const open = inboxState.expandedFiles.has(f.filename)}
                  {@const fileComments = reviewCommentsByPath.get(f.filename) ?? []}
                  <div class="file-block" class:open>
                    <button class="file-head" onclick={() => onToggleFile(f.filename)}>
                      <svg class="i i-sm chev" viewBox="0 0 24 24" style="transform: rotate({open ? 90 : 0}deg);"><path d="m9 18 6-6-6-6" /></svg>
                      <span class="file-status file-status--{f.status}">{f.status}</span>
                      <span class="file-name mono">{f.filename}</span>
                      {#if fileComments.length}
                        <span class="file-comments-badge">{fileComments.length}</span>
                      {/if}
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
                      {#if fileComments.length}
                        <div class="inline-comments">
                          <div class="inline-comments-head">Inline comments</div>
                          {#each fileComments as ic (ic.id)}
                            <div class="inline-comment">
                              <div class="inline-comment-head">
                                {#if ic.user}<img src={ic.user.avatar_url} alt="" class="meta-avatar" /><span class="mono">@{ic.user.login}</span>{/if}
                                {#if ic.line}<span class="inline-line mono">line {ic.line}</span>{/if}
                                <span class="meta-time mono">{relativeTime(ic.created_at, now)} ago</span>
                              </div>
                              <Markdown source={ic.body} />
                            </div>
                          {/each}
                        </div>
                      {/if}
                    {/if}
                  </div>
                {/each}
              {/if}
            </div>

          {:else if tab === 'checks' && item.is_pull_request}
            <div class="tab-pane">
              {#if inboxState.prChecksLoading && inboxState.prChecks.length === 0}
                <div class="tab-state">Loading checks…</div>
              {:else if inboxState.prChecks.length === 0}
                <div class="tab-state">No checks configured for this PR's head commit.</div>
              {:else}
                <div class="checks-summary">
                  <span class="check-pill check-pill--total">{prChecksSummary.total} total</span>
                  {#if prChecksSummary.success > 0}<span class="check-pill check-pill--ok">✓ {prChecksSummary.success} passing</span>{/if}
                  {#if prChecksSummary.failure > 0}<span class="check-pill check-pill--err">✗ {prChecksSummary.failure} failing</span>{/if}
                  {#if prChecksSummary.pending > 0}<span class="check-pill check-pill--pending">… {prChecksSummary.pending} running</span>{/if}
                  {#if prChecksSummary.skipped > 0}<span class="check-pill check-pill--skip">⊘ {prChecksSummary.skipped} skipped</span>{/if}
                  {#if prChecksSummary.cancelled > 0}<span class="check-pill check-pill--skip">⊗ {prChecksSummary.cancelled} cancelled</span>{/if}
                  {#if prChecksSummary.neutral > 0}<span class="check-pill">• {prChecksSummary.neutral} neutral</span>{/if}
                </div>
                <div class="check-list">
                  {#each inboxState.prChecks as c (c.id)}
                    {@const state = checkState(c)}
                    <div class="check-row check-row--{state}">
                      <span class="check-icon check-icon--{state}" aria-hidden="true">
                        {#if state === 'success'}✓{:else if state === 'failure'}✗{:else if state === 'pending'}…{:else if state === 'skipped'}⊘{:else if state === 'cancelled'}⊗{:else}•{/if}
                      </span>
                      <div class="check-main">
                        <div class="check-name">{c.name}</div>
                        <div class="check-sub mono">
                          {c.app_name ?? 'check'}
                          · {c.status}{c.conclusion ? ` / ${c.conclusion}` : ''}
                          {#if c.completed_at}
                            · {relativeTime(c.completed_at, now)} ago
                          {:else if c.started_at}
                            · started {relativeTime(c.started_at, now)} ago
                          {/if}
                        </div>
                      </div>
                      {#if c.details_url}
                        <button class="check-link mono" onclick={() => onOpenCheckDetails(c.details_url!)} title="Open on GitHub">
                          details →
                        </button>
                      {/if}
                    </div>
                  {/each}
                </div>
              {/if}
            </div>

          {:else if tab === 'reviews' && item.is_pull_request}
            <div class="tab-pane">
              {#if inboxState.detailLoading && !inboxState.prReviews.length}
                <div class="tab-state">Loading reviews…</div>
              {:else if inboxState.prReviews.length === 0}
                <div class="tab-state">No reviews yet.</div>
              {:else}
                {#each inboxState.prReviews as r (r.id)}
                  {@const rl = reviewStateLabel(r.state)}
                  {@const inline = reviewCommentsByReview.get(r.id) ?? []}
                  <div class="timeline-item review-item {rl.className}">
                    <div class="timeline-head">
                      {#if r.user}<img src={r.user.avatar_url} alt="" class="meta-avatar" /><span class="mono">@{r.user.login}</span>{/if}
                      <span class="review-state">{rl.text}</span>
                      {#if r.submitted_at}<span class="meta-time mono">{relativeTime(r.submitted_at, now)} ago</span>{/if}
                    </div>
                    {#if r.body}
                      <Markdown source={r.body} />
                    {:else if inline.length === 0}
                      <div class="review-empty">No written feedback.</div>
                    {/if}
                    {#if inline.length > 0}
                      <button class="review-inline-link" onclick={() => onTabChange('files')}>
                        <svg class="i i-sm" viewBox="0 0 24 24"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><path d="M14 2v6h6"/></svg>
                        {inline.length} inline comment{inline.length > 1 ? 's' : ''} on {new Set(inline.map((c) => c.path)).size} file{new Set(inline.map((c) => c.path)).size > 1 ? 's' : ''} →
                      </button>
                    {/if}
                  </div>
                {/each}
              {/if}
            </div>
          {/if}
        </div>
      </div>

      <footer class="focus-actions">
        <button class="btn btn--ghost" onclick={onOpenComment}>
          <svg class="i i-sm" viewBox="0 0 24 24"><path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z" /></svg>
          Comment
        </button>
        {#if item.is_pull_request}
          <button class="btn btn--ghost" onclick={onOpenReview}>
            <svg class="i i-sm" viewBox="0 0 24 24"><path d="M20 6 9 17l-5-5" /></svg>
            Review
          </button>
        {/if}
        {#if item.state === 'open' && !item.merged}
          <button class="btn btn--ghost" onclick={onAskClose} disabled={actionBusy !== null}>
            <svg class="i i-sm" viewBox="0 0 24 24"><path d="M18 6 6 18M6 6l12 12" /></svg>
            {actionBusy === 'closed' ? 'Closing…' : 'Close'}
          </button>
        {:else if item.merged}
          <button class="btn btn--ghost" disabled>
            <svg class="i i-sm" viewBox="0 0 24 24"><circle cx="6" cy="6" r="3" /><circle cx="18" cy="18" r="3" /><path d="M6 9v6a6 6 0 0 0 6 6h2" /></svg>
            Merged
          </button>
        {:else}
          <button class="btn btn--ghost" onclick={onReopen} disabled={actionBusy !== null}>
            <svg class="i i-sm" viewBox="0 0 24 24"><circle cx="12" cy="12" r="9" /></svg>
            {actionBusy === 'open' ? 'Reopening…' : 'Reopen'}
          </button>
        {/if}
        <div style="flex:1"></div>
        {#if item.is_pull_request}
          <button class="btn btn--primary" onclick={onOpenMerge} disabled={mergeDisabled()}>
            <svg class="i i-sm" viewBox="0 0 24 24"><circle cx="18" cy="18" r="3" /><circle cx="6" cy="6" r="3" /><path d="M6 9v6a6 6 0 0 0 6 6h2" /></svg>
            {inboxState.prDetail?.merged ? 'Merged' : 'Merge'}
          </button>
        {/if}
      </footer>
    </div>
  </div>
{/if}

<style>
  /* Focus-pane styles that intentionally override or extend the base rules
     defined globally in app.css. Kept scoped so the Workbench GitHub column
     and the Repositories view render identically. */

  /* Unified header bar — close on the left, metadata in the middle,
     "Open on GitHub" on the right. Same shape as `.jdp-head` (Jira) and
     `.sdp-head` (Sentry). */
  .gfo-head {
    display: flex; align-items: center; gap: 10px;
    padding: 12px 20px;
    border-bottom: 1px solid var(--border-neutral);
    background: var(--bg-1);
    flex-shrink: 0;
  }
  .gfo-back {
    width: 28px; height: 28px; border-radius: 5px;
    display: inline-flex; align-items: center; justify-content: center;
    background: transparent; color: var(--text-1); border: none; cursor: pointer;
  }
  .gfo-back:hover { background: var(--bg-2); color: var(--text-0); }
  .gfo-key { font-size: 13px; color: var(--accent-bright); font-weight: 600; }
  .gfo-kind { font-size: 11px; color: var(--text-2); }
  .gfo-repo { font-size: 11.5px; color: var(--text-1); padding: 2px 8px; border-radius: 5px; background: var(--bg-2); border: 1px solid var(--border-neutral); max-width: 260px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .gfo-branches { font-size: 11.5px; color: var(--text-2); padding: 2px 8px; border-radius: 5px; background: var(--bg-2); border: 1px solid var(--border-neutral); }
  .gfo-btn {
    display: inline-flex; align-items: center; gap: 6px;
    padding: 6px 12px; border-radius: 6px;
    background: var(--bg-2); color: var(--text-1);
    font-size: 12px; border: 1px solid var(--border-neutral-hi); cursor: pointer;
  }
  .gfo-btn:hover:not(:disabled) { background: var(--bg-3); color: var(--text-0); }
  .gfo-btn:disabled { opacity: 0.45; cursor: not-allowed; }
  .gfo-btn--icon { padding: 6px; }
  .gfo-btn--icon .i-sm { width: 14px; height: 14px; }
  .gfo-spin { animation: gfo-spin 0.8s linear infinite; }
  @keyframes gfo-spin { to { transform: rotate(360deg); } }

  .focus-scroll { flex: 1; overflow-y: auto; }
  .focus-shell { max-width: 920px; margin: 0 auto; padding: 32px 40px 80px; }

  .chip-state { padding: 2px 9px; border-radius: 5px; font-size: 10.5px; font-weight: 500; }

  .focus-title { font-size: 26px; line-height: 1.2; letter-spacing: -0.02em; font-weight: 600; margin-bottom: 16px; max-width: 720px; color: var(--text-0); }
  .focus-labels { display: flex; gap: 6px; margin-bottom: 16px; flex-wrap: wrap; }
  :global(.label-chip) {
    padding: 2px 8px; border-radius: 999px;
    font-size: 10.5px; font-weight: 500;
    border: 1px solid;
  }

  .focus-meta {
    display: flex; align-items: center; gap: 12px;
    font-size: 12px; color: var(--text-2);
    padding-bottom: 20px; margin-bottom: 20px;
    border-bottom: 1px solid var(--border-neutral);
    flex-wrap: wrap;
  }
  .focus-meta .divider { width: 3px; height: 3px; border-radius: 50%; background: var(--text-mute); }
  .focus-meta-item { display: inline-flex; align-items: center; gap: 6px; }
  .meta-avatar { width: 18px; height: 18px; border-radius: 50%; }
  .chg-add { color: var(--accent-bright); }
  .chg-del { color: #fca5a5; }

  .detail-tabs { display: flex; gap: 2px; margin-bottom: 20px; border-bottom: 1px solid var(--border-neutral); }
  .detail-tab {
    padding: 10px 14px; font-size: 12.5px; color: var(--text-2);
    border-bottom: 2px solid transparent; margin-bottom: -1px;
    transition: all 120ms;
  }
  .detail-tab:hover { color: var(--text-0); }
  .detail-tab.active { color: var(--accent-bright); border-bottom-color: var(--accent); }
  .tab-count {
    padding: 0 6px; min-width: 16px; height: 16px;
    border-radius: 8px; background: var(--bg-3);
    font-size: 10.5px; font-weight: 600;
    display: inline-flex; align-items: center; justify-content: center;
    color: var(--text-1); margin-left: 6px;
  }
  .detail-tab.active .tab-count { background: var(--accent-soft); color: var(--accent-bright); }
  .tab-count--ok { background: rgba(16, 185, 129, 0.18); color: #34d399; }
  .tab-count--err { background: rgba(239, 68, 68, 0.18); color: #fca5a5; }
  .tab-count--pending { background: rgba(234, 179, 8, 0.16); color: #fcd34d; }

  .checks-summary { display: flex; flex-wrap: wrap; gap: 6px; margin-bottom: 14px; }
  .check-pill {
    font-size: 11px; padding: 3px 9px; border-radius: 12px;
    border: 1px solid var(--border-neutral);
    background: var(--bg-2); color: var(--text-1);
  }
  .check-pill--total { background: var(--bg-2); color: var(--text-1); }
  .check-pill--ok { background: rgba(16, 185, 129, 0.12); color: #34d399; border-color: rgba(16, 185, 129, 0.3); }
  .check-pill--err { background: rgba(239, 68, 68, 0.12); color: #fca5a5; border-color: rgba(239, 68, 68, 0.3); }
  .check-pill--pending { background: rgba(234, 179, 8, 0.12); color: #fcd34d; border-color: rgba(234, 179, 8, 0.3); }
  .check-pill--skip { background: var(--bg-2); color: var(--text-mute); }

  .check-list {
    border: 1px solid var(--border-neutral); border-radius: 10px;
    overflow: hidden; background: var(--bg-1);
  }
  .check-row {
    display: flex; align-items: center; gap: 12px;
    padding: 12px 14px;
    border-bottom: 1px solid var(--border-neutral);
  }
  .check-row:last-child { border-bottom: none; }
  .check-icon {
    width: 22px; height: 22px; border-radius: 50%;
    display: inline-flex; align-items: center; justify-content: center;
    font-size: 12px; font-weight: 700;
    flex-shrink: 0;
  }
  .check-icon--success { background: rgba(16, 185, 129, 0.15); color: #34d399; }
  .check-icon--failure { background: rgba(239, 68, 68, 0.15); color: #fca5a5; }
  .check-icon--pending { background: rgba(234, 179, 8, 0.15); color: #fcd34d; animation: check-spin 1.6s linear infinite; }
  .check-icon--skipped,
  .check-icon--cancelled,
  .check-icon--neutral { background: var(--bg-2); color: var(--text-mute); }

  .check-main { flex: 1; min-width: 0; }
  .check-name { color: var(--text-0); font-weight: 500; }
  .check-sub { font-size: 10.5px; color: var(--text-mute); margin-top: 2px; }
  .check-link {
    font-size: 11px; color: var(--accent-bright);
    padding: 5px 9px; border-radius: 6px;
    background: transparent;
  }
  .check-link:hover { background: var(--bg-2); }

  .tab-pane { min-height: 100px; }
  .tab-state { padding: 40px; text-align: center; color: var(--text-2); font-size: 13px; }
  .tab-error {
    padding: 12px 14px; font-size: 12.5px; color: #fca5a5;
    background: rgba(239, 68, 68, 0.06); border: 1px solid rgba(239, 68, 68, 0.22);
    border-radius: 8px; margin-bottom: 16px;
  }

  .body-card {
    background: var(--bg-1); border: 1px solid var(--border-neutral);
    border-radius: 10px; padding: 16px 18px; margin-bottom: 16px;
  }
  .body-head {
    display: flex; align-items: center; gap: 8px;
    padding-bottom: 12px; margin-bottom: 12px;
    border-bottom: 1px solid var(--border-neutral);
    font-size: 12.5px; color: var(--text-1);
  }
  .body-empty {
    padding: 14px 16px; background: var(--bg-1);
    border: 1px dashed var(--border-neutral-hi); border-radius: 10px;
    color: var(--text-mute); font-style: italic; font-size: 12.5px; margin-bottom: 16px;
  }
  .timeline-item {
    background: var(--bg-1); border: 1px solid var(--border-neutral);
    border-radius: 10px; padding: 14px 16px; margin-bottom: 10px;
  }
  /* Slim, sparse commit row in the conversation — visually softer than the
     review/comment cards (no background, no border) so the surface still
     reads as "those are the bubbles, this is just history". Matches the
     way GitHub renders commit dots in the same timeline. */
  .timeline-commit {
    display: flex; align-items: center; gap: 8px;
    padding: 6px 4px;
    margin-bottom: 6px;
    font-size: 12px;
    color: var(--text-1);
  }
  .timeline-commit-icon {
    width: 22px; height: 22px;
    display: inline-flex; align-items: center; justify-content: center;
    border-radius: 50%;
    background: var(--bg-2);
    border: 1px solid var(--border-neutral-hi);
    color: var(--text-2);
    flex-shrink: 0;
  }
  .timeline-commit-icon svg { width: 12px; height: 12px; }
  .timeline-commit-msg {
    flex: 1; min-width: 0;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
    color: var(--text-0);
  }
  .timeline-commit-avatar { width: 16px; height: 16px; }
  .timeline-commit-sha {
    padding: 1px 6px;
    border-radius: 4px;
    background: var(--bg-2);
    border: 1px solid var(--border-neutral-hi);
    color: var(--text-1);
    font-size: 10.5px;
    text-decoration: none;
    transition: all 120ms;
  }
  .timeline-commit-sha:hover {
    color: var(--accent-bright);
    border-color: var(--border-hi);
    background: var(--accent-soft);
  }
  .timeline-head {
    display: flex; align-items: center; gap: 8px;
    padding-bottom: 10px; margin-bottom: 10px;
    border-bottom: 1px solid var(--border-neutral);
    font-size: 12.5px; color: var(--text-1);
  }
  .review-state {
    padding: 1px 8px; border-radius: 4px;
    font-size: 10.5px; font-weight: 600; text-transform: uppercase; letter-spacing: 0.04em;
    color: var(--text-2); background: var(--bg-2);
    border: 1px solid var(--border-neutral-hi);
  }
  .meta-time { margin-left: auto; color: var(--text-mute); font-size: 11px; }

  .review-item.rev--approved .review-state { color: var(--accent-bright); background: var(--accent-soft); border-color: rgba(16, 185, 129, 0.3); }
  .review-item.rev--changes .review-state { color: #fca5a5; background: rgba(239, 68, 68, 0.08); border-color: rgba(239, 68, 68, 0.28); }
  .review-item.rev--commented .review-state { color: var(--blue-bright); background: rgba(59, 130, 246, 0.08); border-color: rgba(59, 130, 246, 0.24); }
  .review-item.rev--approved { border-left: 3px solid var(--accent); }
  .review-item.rev--changes { border-left: 3px solid #fca5a5; }

  .review-empty {
    font-size: 12.5px; color: var(--text-mute);
    font-style: italic;
  }
  .review-inline-link {
    margin-top: 10px;
    display: inline-flex; align-items: center; gap: 6px;
    padding: 6px 10px;
    background: rgba(59, 130, 246, 0.08);
    border: 1px solid rgba(59, 130, 246, 0.22);
    border-radius: 7px;
    color: var(--blue-bright);
    font-size: 12px; font-weight: 500;
    transition: all 120ms;
    cursor: pointer;
  }
  .review-inline-link:hover {
    background: rgba(59, 130, 246, 0.14);
    border-color: rgba(59, 130, 246, 0.35);
    transform: translateY(-1px);
  }
  .review-inline-link--secondary {
    margin-top: 8px;
    background: var(--bg-1);
    border-color: var(--border-neutral-hi);
    color: var(--text-1);
  }
  .review-inline-link--secondary:hover {
    background: var(--bg-2);
    color: var(--text-0);
    transform: none;
  }
  .review-chevron {
    transition: transform 140ms ease;
  }
  .review-chevron--open {
    transform: rotate(90deg);
  }
  .inline-comments--review {
    margin-top: 10px;
  }
  .inline-path {
    color: var(--text-1);
    font-size: 11px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 320px;
  }

  /* Commits list */
  .commit-row {
    display: flex; align-items: flex-start; gap: 12px;
    padding: 12px 14px; width: 100%;
    background: var(--bg-1); border: 1px solid var(--border-neutral);
    border-radius: 8px; margin-bottom: 6px;
    text-align: left; transition: all 120ms;
  }
  .commit-row:hover { background: var(--bg-2); border-color: var(--border-neutral-hi); transform: translateY(-1px); }
  .commit-main { display: flex; align-items: flex-start; gap: 10px; flex: 1; min-width: 0; }
  .commit-body { flex: 1; min-width: 0; }
  .commit-title { font-size: 13px; color: var(--text-0); font-weight: 500; line-height: 1.4; overflow-wrap: break-word; }
  .commit-rest {
    font-size: 12px; color: var(--text-2); margin-top: 3px;
    white-space: pre-wrap;
    font-family: 'JetBrains Mono', monospace; line-height: 1.5;
    max-height: 80px; overflow: hidden;
  }
  .commit-meta {
    font-size: 10.5px; color: var(--text-mute);
    margin-top: 6px; display: flex; gap: 6px;
  }
  .commit-sha {
    font-size: 11px; padding: 3px 8px;
    border-radius: 5px;
    background: var(--bg-2); border: 1px solid var(--border-neutral-hi);
    color: var(--accent-bright); flex-shrink: 0;
  }

  /* Files */
  .files-summary { font-size: 12px; color: var(--text-2); margin-bottom: 12px; padding: 0 2px; }
  .file-block {
    background: var(--bg-1); border: 1px solid var(--border-neutral);
    border-radius: 8px; margin-bottom: 6px; overflow: hidden;
  }
  .file-head {
    display: flex; align-items: center; gap: 10px;
    width: 100%; padding: 10px 14px;
    text-align: left; transition: background 120ms;
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
  .file-comments-badge {
    font-size: 10px; font-weight: 600;
    padding: 1px 7px;
    border-radius: 999px;
    background: var(--blue-deep); color: var(--blue-bright);
    border: 1px solid rgba(59, 130, 246, 0.3);
  }

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
    background: var(--bg-1);
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

  /* Inline comments on files */
  .inline-comments {
    border-top: 1px solid var(--border-neutral);
    padding: 12px 14px;
    background: var(--bg-1);
  }
  .inline-comments-head {
    font-size: 10.5px; font-weight: 600; color: var(--text-2);
    text-transform: uppercase; letter-spacing: 0.08em; margin-bottom: 10px;
  }
  .inline-comment {
    padding: 10px 12px;
    background: var(--bg-1);
    border: 1px solid var(--border-neutral);
    border-radius: 8px; margin-bottom: 6px;
    border-left: 3px solid var(--blue);
  }
  .inline-comment-head {
    display: flex; align-items: center; gap: 8px;
    font-size: 12px; color: var(--text-1);
    padding-bottom: 8px; margin-bottom: 8px;
    border-bottom: 1px solid var(--border-neutral);
  }
  .inline-line { color: var(--blue-bright); font-size: 11px; }

  /* Focus action bar */
  .focus-actions {
    border-top: 1px solid var(--border-neutral);
    padding: 12px 24px;
    display: flex; align-items: center; gap: 8px;
    background: var(--backdrop);
    backdrop-filter: blur(12px);
  }

  /* Slide-over */
  .slide-over {
    position: fixed; inset: 0;
    background: var(--backdrop);
    backdrop-filter: blur(8px);
    z-index: 180;
    display: flex; justify-content: flex-end;
    animation: fadeIn 180ms ease-out;
  }
  .slide-panel {
    width: min(1080px, 92vw);
    height: 100%;
    background: var(--bg-0);
    border-left: 1px solid var(--border-neutral-hi);
    box-shadow: -20px 0 60px rgba(0, 0, 0, 0.5);
    display: flex; flex-direction: column;
    overflow: hidden;
    animation: slideInRight 240ms cubic-bezier(0.34, 1.56, 0.64, 1);
    position: relative;
  }
  @keyframes slideInRight {
    from { transform: translateX(40px); opacity: 0; }
    to   { transform: translateX(0); opacity: 1; }
  }
  @keyframes fadeIn { from { opacity: 0; } to { opacity: 1; } }
  @keyframes check-spin { from { transform: rotate(0deg); } to { transform: rotate(360deg); } }

  .slide-close {
    position: absolute;
    top: 12px; right: 16px;
    z-index: 5;
    width: 32px; height: 32px;
    border-radius: 8px;
    background: var(--bg-2);
    border: 1px solid var(--border-neutral-hi);
    color: var(--text-1);
    display: inline-flex; align-items: center; justify-content: center;
    transition: all 120ms;
  }
  .slide-close:hover { background: var(--bg-3); color: var(--text-0); border-color: var(--border-hi2); }
</style>
