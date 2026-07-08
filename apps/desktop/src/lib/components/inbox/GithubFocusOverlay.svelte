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
  import { labelColorStyle } from '$lib/format';
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
    /** Hand the focused PR off to a Claude session. Optional
     *  so older call sites that haven't wired them up yet still
     *  compile; the overlay hides the button when undefined. */
    onSendToClaude?: () => void;
  }

  // `tab` / `onTabChange` stay in Props (the parent still passes them) but the
  // §2.6 detail is a single scrolling document — Checks / Files / Conversation
  // are stacked sections, not tabs — so they are intentionally not consumed.
  let {
    now,
    actionBusy,
    onCloseFocus,
    onRetryLoadDetail,
    onToggleFile,
    onOpenCommit,
    onOpenComment,
    onOpenReview,
    onOpenMerge,
    onAskClose,
    onReopen,
    onOpenBrowser,
    onOpenCheckDetails,
    mergeDisabled,
    onSendToClaude
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

  // Group review comments by file path — pins each comment to its file block.
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

  // Per-review expansion state for the inline comments in the conversation.
  let expandedReviews = $state(new Set<number>());
  function toggleReviewExpansion(id: number) {
    const next = new Set(expandedReviews);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    expandedReviews = next;
  }

  // Rolled-up counts for the Checks section header + summary.
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

  // Two-segment 56px diffstat bar: ok (additions) + err (deletions).
  function diffstat(additions: number, deletions: number): { ok: number; err: number } {
    const total = additions + deletions;
    if (total === 0) return { ok: 0, err: 0 };
    const ok = Math.max(2, Math.min(54, Math.round((56 * additions) / total)));
    return { ok, err: 56 - ok };
  }
</script>

{#if inboxState.focusItem}
  {@const item = inboxState.focusItem}
  {@const stag = stateTag(item)}
  {@const pr = inboxState.prDetail}
  <div
    class="ghd"
    onkeydown={(e) => { if (e.key === 'Escape') onCloseFocus(); }}
    role="region"
    aria-label="Pull request detail"
    tabindex="-1"
  >
    <div class="ghd-panel">
      <!-- §2.6 document header, 52px, flush on --bg-0. -->
      <header class="ghd-head">
        <button class="ghd-back" onclick={onCloseFocus} aria-label="Close" title="Close">
          <svg class="i i-sm" viewBox="0 0 24 24"><path d="M18 6 6 18M6 6l12 12" /></svg>
        </button>
        <span class="ghd-ref mono">{externalId(item)}</span>
        <span class="state-pill {stag.className}">{stag.text}</span>
        <span class="ghd-kind">{kindLabel(item).toLowerCase()}</span>
        {#if item.repo}
          <span class="ghd-repo mono" title={repoLabel(item)}>{repoLabel(item)}</span>
        {/if}
        <div class="ghd-spring"></div>
        <button
          class="ghd-iconbtn"
          onclick={onRetryLoadDetail}
          disabled={inboxState.detailLoading}
          title="Refresh PR detail (PR/files/commits/checks)"
          aria-label="Refresh"
        >
          <svg class="i i-sm" class:ghd-spin={inboxState.detailLoading} viewBox="0 0 24 24">
            <path d="M21 12a9 9 0 0 1-9 9 9 9 0 0 1-8.5-6"/>
            <path d="M3 12a9 9 0 0 1 9-9 9 9 0 0 1 8.5 6"/>
            <polyline points="21 3 21 9 15 9"/>
            <polyline points="3 21 3 15 9 15"/>
          </svg>
        </button>
        <button class="ghd-ghostbtn" onclick={() => onOpenBrowser(item.url)} title="Open on GitHub">
          Open on GitHub
          <svg class="i i-sm" viewBox="0 0 24 24"><path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"/><path d="M15 3h6v6M10 14 21 3"/></svg>
        </button>
        {#if onSendToClaude}
          <button class="ghd-claudebtn" onclick={onSendToClaude} title="Send this PR to a Claude session">→ claude</button>
        {/if}
      </header>

      <div class="ghd-scroll">
        <div class="ghd-doc">
          <h1 class="ghd-title">{item.title}</h1>

          {#if item.labels.length}
            <div class="ghd-labels">
              {#each item.labels as label (label.name)}
                <span class="ghd-label" style={labelColorStyle(label.color)}>{label.name}</span>
              {/each}
            </div>
          {/if}

          <!-- Branch row: head → base chips + diffstat + files count + author. -->
          <div class="ghd-branchrow">
            {#if item.is_pull_request && pr}
              <span class="ghd-branch mono">{pr.head_ref}</span>
              <span class="ghd-branch-arrow" aria-hidden="true">→</span>
              <span class="ghd-branch mono">{pr.base_ref}</span>
              <span class="ghd-dotsep" aria-hidden="true"></span>
              <span class="ghd-diffnum mono">
                <span class="ghd-add">+{pr.additions.toLocaleString()}</span>
                <span class="ghd-del">−{pr.deletions.toLocaleString()}</span>
                · {pr.changed_files} file{pr.changed_files === 1 ? '' : 's'}
              </span>
            {/if}
            {#if item.author}
              <span class="ghd-dotsep" aria-hidden="true"></span>
              <span class="ghd-author">
                <img src={item.author.avatar_url} alt="" class="ghd-avatar" />
                <span class="mono">@{item.author.login}</span>
              </span>
            {/if}
            <span class="ghd-dotsep" aria-hidden="true"></span>
            <span class="ghd-when mono">opened {relativeTime(item.created_at, now)} ago</span>
          </div>

          {#if inboxState.detailError}
            <div class="ghd-error">
              Failed to load detail: {inboxState.detailError}
              <button class="ghd-link" onclick={onRetryLoadDetail}>Retry</button>
            </div>
          {/if}

          <!-- CHECKS — grid [16][1fr][auto], mono 12. -->
          {#if item.is_pull_request}
            <section class="ghd-section">
              <div class="ghd-sec-head">
                <span class="ghd-sec-label">Checks</span>
                {#if prChecksSummary.total > 0}
                  <span
                    class="ghd-sec-count mono"
                    class:is-err={prChecksSummary.failure > 0}
                    class:is-run={prChecksSummary.failure === 0 && prChecksSummary.pending > 0}
                  >{prChecksSummary.success}/{prChecksSummary.total}</span>
                {/if}
              </div>
              {#if inboxState.prChecksLoading && inboxState.prChecks.length === 0}
                <div class="ghd-empty">Loading checks…</div>
              {:else if inboxState.prChecks.length === 0}
                <div class="ghd-empty">No checks configured for this PR's head commit.</div>
              {:else}
                <div class="ghd-checks">
                  {#each inboxState.prChecks as c (c.id)}
                    {@const st = checkState(c)}
                    <div class="ghd-check ghd-check--{st}">
                      <span class="ghd-check-icon" aria-hidden="true">
                        {#if st === 'success'}✓
                        {:else if st === 'failure'}✗
                        {:else if st === 'pending'}<span class="ghd-run-dot"></span>
                        {:else if st === 'skipped'}⊘
                        {:else if st === 'cancelled'}⊗
                        {:else}•{/if}
                      </span>
                      <span class="ghd-check-name mono">
                        {c.name}{#if c.app_name}<span class="ghd-check-app"> · {c.app_name}</span>{/if}
                      </span>
                      {#if c.details_url}
                        <button class="ghd-check-time mono" onclick={() => onOpenCheckDetails(c.details_url!)} title="Open on GitHub">
                          {#if c.completed_at}{relativeTime(c.completed_at, now)}{:else if c.started_at}{relativeTime(c.started_at, now)}{:else}—{/if}
                        </button>
                      {:else}
                        <span class="ghd-check-time mono">
                          {#if c.completed_at}{relativeTime(c.completed_at, now)}{:else if c.started_at}{relativeTime(c.started_at, now)}{:else}—{/if}
                        </span>
                      {/if}
                    </div>
                  {/each}
                </div>
              {/if}
            </section>
          {/if}

          <!-- FILES — name mono + ±n + 56×4 diffstat bar. -->
          {#if item.is_pull_request}
            <section class="ghd-section">
              <div class="ghd-sec-head">
                <span class="ghd-sec-label">Files</span>
                {#if inboxState.prFiles.length}<span class="ghd-sec-count mono">{inboxState.prFiles.length}</span>{/if}
              </div>
              {#if inboxState.detailLoading && !inboxState.prFiles.length}
                <div class="ghd-empty">Loading files…</div>
              {:else if inboxState.prFiles.length === 0}
                <div class="ghd-empty">No changed files.</div>
              {:else}
                <div class="ghd-files">
                  {#each inboxState.prFiles as f (f.filename)}
                    {@const open = inboxState.expandedFiles.has(f.filename)}
                    {@const fileComments = reviewCommentsByPath.get(f.filename) ?? []}
                    {@const bar = diffstat(f.additions, f.deletions)}
                    <div class="ghd-file" class:open>
                      <button class="ghd-file-head" onclick={() => onToggleFile(f.filename)}>
                        <svg class="i i-sm ghd-chev" viewBox="0 0 24 24" style="transform: rotate({open ? 90 : 0}deg);"><path d="m9 18 6-6-6-6" /></svg>
                        <span class="ghd-file-name mono">{f.filename}</span>
                        {#if fileComments.length}<span class="ghd-file-badge">{fileComments.length}</span>{/if}
                        <span class="ghd-file-chg mono">
                          <span class="ghd-add">+{f.additions}</span>
                          <span class="ghd-del">−{f.deletions}</span>
                        </span>
                        <span class="ghd-diffstat" aria-hidden="true">
                          <span class="ghd-diffstat-ok" style="width:{bar.ok}px"></span>
                          <span class="ghd-diffstat-err" style="width:{bar.err}px"></span>
                        </span>
                      </button>
                      {#if open}
                        {#if f.patch}
                          {@const lines = parsePatch(f.patch)}
                          <div class="ghd-diff-scroller">
                            <div class="ghd-diff-body">
                              {#each lines as line, idx (idx)}
                                {#if line.kind === 'header'}
                                  <div class="ghd-hunk mono">{line.text}</div>
                                {:else}
                                  <div class="ghd-diff-line {line.kind}">
                                    <span class="ghd-diff-num">{line.kind === 'add' ? '+' : line.kind === 'del' ? '−' : line.newLine ?? ''}</span>
                                    <span class="ghd-diff-content">{line.text}</span>
                                  </div>
                                {/if}
                              {/each}
                            </div>
                          </div>
                        {:else}
                          <div class="ghd-empty">Binary file or no patch available.</div>
                        {/if}
                        {#if fileComments.length}
                          <div class="ghd-file-comments">
                            {#each fileComments as ic (ic.id)}
                              <div class="ghd-quote">
                                <div class="ghd-quote-head">
                                  {#if ic.user}<img src={ic.user.avatar_url} alt="" class="ghd-avatar" /><span class="mono">@{ic.user.login}</span>{/if}
                                  {#if ic.line}<span class="ghd-quote-line mono">line {ic.line}</span>{/if}
                                  <span class="ghd-quote-time mono">{relativeTime(ic.created_at, now)} ago</span>
                                </div>
                                <div class="ghd-quote-body"><Markdown source={ic.body} /></div>
                              </div>
                            {/each}
                          </div>
                        {/if}
                      {/if}
                    </div>
                  {/each}
                </div>
              {/if}
            </section>
          {/if}

          <!-- CONVERSATION — description + timeline as border-left quotes. -->
          <section class="ghd-section">
            <div class="ghd-sec-head">
              <span class="ghd-sec-label">Conversation</span>
              {#if inboxState.comments.length + inboxState.prReviews.length > 0}
                <span class="ghd-sec-count mono">{inboxState.comments.length + inboxState.prReviews.length}</span>
              {/if}
            </div>

            {#if item.body}
              <div class="ghd-quote">
                <div class="ghd-quote-head">
                  {#if item.author}
                    <img src={item.author.avatar_url} alt="" class="ghd-avatar" />
                    <span class="mono">@{item.author.login}</span>
                  {/if}
                  <span class="ghd-quote-time mono">{relativeTime(item.created_at, now)} ago</span>
                </div>
                <div class="ghd-quote-body"><Markdown source={item.body} /></div>
              </div>
            {:else}
              <div class="ghd-empty">No description.</div>
            {/if}

            {#if inboxState.detailLoading && !inboxState.comments.length && !inboxState.prReviews.length}
              <div class="ghd-empty">Loading conversation…</div>
            {:else}
              {@const timeline = [
                ...inboxState.prReviews.map((r) => ({ type: 'review' as const, at: r.submitted_at ?? '', data: r, key: `review-${r.id}` })),
                ...inboxState.comments.map((c) => ({ type: 'comment' as const, at: c.created_at, data: c, key: `comment-${c.id}` })),
                // Commits interleave by author_date so reviewers' bubbles surface
                // next to the SHAs that triggered them — matches GitHub's own pane.
                ...inboxState.prCommits.map((c) => ({ type: 'commit' as const, at: c.author_date, data: c, key: `commit-${c.sha}` }))
              ].sort((a, b) => a.at.localeCompare(b.at))}
              {#each timeline as entry (entry.key)}
                {#if entry.type === 'review'}
                  {@const r = entry.data}
                  {@const rl = reviewStateLabel(r.state)}
                  {@const inline = reviewCommentsByReview.get(r.id) ?? []}
                  <div class="ghd-quote ghd-quote--review {rl.className}">
                    <div class="ghd-quote-head">
                      {#if r.user}<img src={r.user.avatar_url} alt="" class="ghd-avatar" /><span class="mono">@{r.user.login}</span>{/if}
                      <span class="ghd-review-state">{rl.text}</span>
                      {#if r.submitted_at}<span class="ghd-quote-time mono">{relativeTime(r.submitted_at, now)} ago</span>{/if}
                    </div>
                    {#if r.body}
                      <div class="ghd-quote-body"><Markdown source={r.body} /></div>
                    {:else if inline.length === 0}
                      <div class="ghd-quote-empty">No written feedback.</div>
                    {/if}
                    {#if inline.length > 0}
                      {@const expanded = expandedReviews.has(r.id)}
                      {@const fileCount = new Set(inline.map((c) => c.path)).size}
                      <button class="ghd-inline-toggle" onclick={() => toggleReviewExpansion(r.id)} aria-expanded={expanded}>
                        <svg class="i i-sm ghd-chev" class:ghd-chev--open={expanded} viewBox="0 0 24 24"><path d="m9 18 6-6-6-6"/></svg>
                        {inline.length} inline comment{inline.length > 1 ? 's' : ''} on {fileCount} file{fileCount > 1 ? 's' : ''}
                      </button>
                      {#if expanded}
                        <div class="ghd-inline-list">
                          {#each inline as ic (ic.id)}
                            <div class="ghd-quote ghd-quote--nested">
                              <div class="ghd-quote-head">
                                {#if ic.user}<img src={ic.user.avatar_url} alt="" class="ghd-avatar" /><span class="mono">@{ic.user.login}</span>{/if}
                                <span class="ghd-quote-line mono" title={ic.path}>{ic.path}</span>
                                {#if ic.line}<span class="ghd-quote-line mono">L{ic.line}</span>{/if}
                                <span class="ghd-quote-time mono">{relativeTime(ic.created_at, now)} ago</span>
                              </div>
                              <div class="ghd-quote-body"><Markdown source={ic.body} /></div>
                            </div>
                          {/each}
                        </div>
                      {/if}
                    {/if}
                  </div>
                {:else if entry.type === 'commit'}
                  {@const cm = entry.data}
                  <button class="ghd-commit" onclick={() => onOpenCommit(cm)} title={cm.message}>
                    <span class="ghd-commit-icon" aria-hidden="true">
                      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="3.5"/><path d="M3 12h5.5M15.5 12H21"/></svg>
                    </span>
                    <span class="ghd-commit-msg">{cm.message.split('\n')[0]}</span>
                    {#if cm.author_avatar}<img class="ghd-avatar" src={cm.author_avatar} alt="" />{/if}
                    <span class="ghd-quote-time mono">{relativeTime(cm.author_date, now)} ago</span>
                    <span class="ghd-commit-sha mono">{cm.short_sha}</span>
                  </button>
                {:else}
                  {@const c = entry.data}
                  <div class="ghd-quote">
                    <div class="ghd-quote-head">
                      {#if c.user}<img src={c.user.avatar_url} alt="" class="ghd-avatar" /><span class="mono">@{c.user.login}</span>{/if}
                      <span class="ghd-quote-time mono">{relativeTime(c.created_at, now)} ago</span>
                    </div>
                    <div class="ghd-quote-body"><Markdown source={c.body} /></div>
                  </div>
                {/if}
              {/each}
            {/if}
          </section>
        </div>
      </div>

      <!-- §2.6 actions: Approve ▾ primary, Merge · squash ▾ ghost, note faint. -->
      <footer class="ghd-actions">
        {#if item.is_pull_request}
          <button class="ghd-act ghd-act--primary" onclick={onOpenReview}>
            Approve
            <svg class="i i-sm ghd-caret" viewBox="0 0 24 24"><path d="m6 9 6 6 6-6"/></svg>
          </button>
          <button class="ghd-act ghd-act--ghost" onclick={onOpenMerge} disabled={mergeDisabled()}>
            {inboxState.prDetail?.merged ? 'Merged' : 'Merge · squash'}
            <svg class="i i-sm ghd-caret" viewBox="0 0 24 24"><path d="m6 9 6 6 6-6"/></svg>
          </button>
        {/if}
        <button class="ghd-act ghd-act--ghost" onclick={onOpenComment}>Comment</button>
        {#if item.state === 'open' && !item.merged}
          <button class="ghd-act ghd-act--ghost" onclick={onAskClose} disabled={actionBusy !== null}>
            {actionBusy === 'closed' ? 'Closing…' : 'Close'}
          </button>
        {:else if item.merged}
          <button class="ghd-act ghd-act--ghost" disabled>Merged</button>
        {:else}
          <button class="ghd-act ghd-act--ghost" onclick={onReopen} disabled={actionBusy !== null}>
            {actionBusy === 'open' ? 'Reopening…' : 'Reopen'}
          </button>
        {/if}
        <div class="ghd-spring"></div>
        {#if item.is_pull_request && mergeDisabled()}
          <span class="ghd-merge-note">merge blocked until notarize</span>
        {/if}
      </footer>
    </div>
  </div>
{/if}

<style>
  /* §2.6 GitHub detail (mockup 4e) — single scrolling document, no tabs.
     Fresh `.ghd-` grammar: 52px header, centred document (max 800), stacked
     Checks / Files / Conversation sections, sticky action bar. */

  .ghd {
    flex: 1; min-height: 0;
    display: flex;
    width: 100%; height: 100%;
    background: transparent;
  }
  .ghd-panel {
    flex: 1; min-width: 0; min-height: 0;
    display: flex; flex-direction: column;
    background: var(--bg-0);
    overflow: hidden;
    position: relative;
  }
  .ghd-spring { flex: 1; }

  /* Header ------------------------------------------------------------- */
  .ghd-head {
    display: flex; align-items: center; gap: 10px;
    height: 52px;
    padding: 0 24px;
    border-bottom: 1px solid var(--border-lo);
    background: var(--bg-0);
    flex-shrink: 0;
  }
  .ghd-back {
    width: 28px; height: 28px; border-radius: 5px;
    display: inline-flex; align-items: center; justify-content: center;
    background: transparent; color: var(--text-1); border: none; cursor: pointer;
  }
  .ghd-back:hover { background: var(--bg-2); color: var(--text-0); }
  .ghd-ref { font-size: 12px; color: var(--text-1); font-weight: 600; }
  .ghd-kind { font-size: 12px; color: var(--text-faint); }
  .ghd-repo {
    font-size: 11.5px; color: var(--text-1);
    padding: 2px 8px; border-radius: 5px;
    background: var(--bg-2); border: 1px solid var(--border-neutral);
    max-width: 240px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .ghd-iconbtn {
    width: 30px; height: 28px; border-radius: 6px;
    display: inline-flex; align-items: center; justify-content: center;
    background: transparent; color: var(--text-2);
    border: 1px solid var(--border-neutral); cursor: pointer;
  }
  .ghd-iconbtn:hover:not(:disabled) { background: var(--bg-2); color: var(--text-0); }
  .ghd-iconbtn:disabled { opacity: 0.45; cursor: not-allowed; }
  .ghd-iconbtn .i-sm { width: 14px; height: 14px; }
  .ghd-ghostbtn {
    display: inline-flex; align-items: center; gap: 6px;
    padding: 6px 12px; border-radius: 6px;
    background: transparent; color: var(--text-1);
    font-size: 12px; border: 1px solid var(--border-neutral-hi); cursor: pointer;
  }
  .ghd-ghostbtn:hover { background: var(--bg-2); color: var(--text-0); }
  /* Send-to-Claude — primary inverse pill (spec: inverse + shadow-pill). */
  .ghd-claudebtn {
    display: inline-flex; align-items: center;
    padding: 6px 14px; border-radius: 999px;
    background: var(--text-0); color: var(--bg-0);
    font-size: 12px; font-weight: 600;
    border: none; cursor: pointer;
    box-shadow: var(--shadow-1);
  }
  .ghd-claudebtn:hover { background: var(--text-1); }
  .ghd-spin { animation: ghd-spin 0.8s linear infinite; }
  @keyframes ghd-spin { to { transform: rotate(360deg); } }

  /* Document ----------------------------------------------------------- */
  .ghd-scroll { flex: 1; overflow-y: auto; }
  .ghd-doc { max-width: 800px; margin: 0 auto; padding: 30px 40px 80px; }

  .ghd-title {
    font-size: 22px; line-height: 1.25;
    letter-spacing: -0.015em; font-weight: 600;
    color: var(--text-0);
    margin-bottom: 14px; max-width: 720px;
  }
  .ghd-labels { display: flex; gap: 6px; margin-bottom: 14px; flex-wrap: wrap; }
  .ghd-label {
    padding: 2px 8px; border-radius: 999px;
    font-size: 10.5px; font-weight: 500;
    color: var(--label-color);
    border: 1px solid color-mix(in srgb, var(--label-color) 45%, transparent);
    background: color-mix(in srgb, var(--label-color) 12%, transparent);
  }

  /* Branch row --------------------------------------------------------- */
  .ghd-branchrow {
    display: flex; align-items: center; gap: 8px; flex-wrap: wrap;
    font-size: 12px; color: var(--text-2);
    padding-bottom: 22px; margin-bottom: 4px;
    border-bottom: 1px solid var(--border-neutral);
  }
  .ghd-branch {
    font-size: 11.5px; color: var(--text-1);
    padding: 2px 8px; border-radius: 5px;
    background: var(--accent-soft);
  }
  .ghd-branch-arrow { color: var(--text-mute); font-size: 12px; }
  .ghd-diffnum { font-size: 11.5px; color: var(--text-2); }
  .ghd-add { color: var(--accent-bright); }
  .ghd-del { color: var(--err); }
  .ghd-dotsep { width: 3px; height: 3px; border-radius: 50%; background: var(--text-mute); }
  .ghd-author { display: inline-flex; align-items: center; gap: 6px; font-size: 11.5px; color: var(--text-1); }
  .ghd-when { font-size: 11.5px; color: var(--text-mute); }
  .ghd-avatar { width: 16px; height: 16px; border-radius: 50%; }

  .ghd-error {
    margin: 18px 0 0;
    padding: 12px 14px; font-size: 12.5px; color: var(--err);
    background: color-mix(in srgb, var(--err) 7%, transparent);
    border: 1px solid color-mix(in srgb, var(--err) 24%, transparent);
    border-radius: 8px;
  }
  .ghd-link {
    background: none; border: none; cursor: pointer;
    color: var(--accent-bright); text-decoration: underline; font-size: inherit;
  }

  /* Sections ----------------------------------------------------------- */
  .ghd-section { margin-top: 28px; }
  .ghd-sec-head {
    display: flex; align-items: baseline; gap: 10px;
    margin-bottom: 12px;
  }
  .ghd-sec-label {
    font-size: 10.5px; font-weight: 600;
    text-transform: uppercase; letter-spacing: 0.09em;
    color: var(--text-2);
  }
  .ghd-sec-count { font-size: 11px; color: var(--ok); }
  .ghd-sec-count.is-err { color: var(--err); }
  .ghd-sec-count.is-run { color: var(--warn); }
  .ghd-empty {
    padding: 18px 2px; font-size: 12.5px; color: var(--text-mute);
  }

  /* Checks — grid [16][1fr][auto], mono 12. --------------------------- */
  .ghd-checks { display: flex; flex-direction: column; }
  .ghd-check {
    display: grid; grid-template-columns: 16px 1fr auto;
    align-items: center; gap: 12px;
    padding: 6px 0;
    font-family: var(--font-mono); font-size: 12px;
  }
  .ghd-check-icon {
    display: inline-flex; align-items: center; justify-content: center;
    width: 16px; font-size: 12px; font-weight: 700;
  }
  .ghd-check--success .ghd-check-icon { color: var(--ok); }
  .ghd-check--failure .ghd-check-icon { color: var(--err); }
  .ghd-check--pending .ghd-check-icon { color: var(--warn); }
  .ghd-check--skipped .ghd-check-icon,
  .ghd-check--cancelled .ghd-check-icon,
  .ghd-check--neutral .ghd-check-icon { color: var(--text-faint); }
  .ghd-run-dot {
    width: 7px; height: 7px; border-radius: 50%;
    background: var(--warn);
    animation: ghd-pulse 1.4s ease-in-out infinite;
  }
  @keyframes ghd-pulse { 0%,100% { opacity: 1; } 50% { opacity: 0.35; } }
  .ghd-check-name { color: var(--text-1); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .ghd-check-app { color: var(--text-mute); }
  .ghd-check-time {
    font-size: 11.5px; color: var(--text-faint);
    background: none; border: none; padding: 0; cursor: default;
  }
  button.ghd-check-time { cursor: pointer; }
  button.ghd-check-time:hover { color: var(--accent-bright); }

  /* Files — name mono + ±n + 56×4 diffstat bar. ---------------------- */
  .ghd-files { display: flex; flex-direction: column; }
  .ghd-file { border-bottom: 1px solid var(--border-lo); }
  .ghd-file:last-child { border-bottom: none; }
  .ghd-file-head {
    display: flex; align-items: center; gap: 10px;
    width: 100%; padding: 7px 0;
    background: none; border: none; cursor: pointer; text-align: left;
  }
  .ghd-file-head:hover .ghd-file-name { color: var(--accent-bright); }
  .ghd-chev { color: var(--text-mute); transition: transform 160ms; flex-shrink: 0; }
  .ghd-chev--open { transform: rotate(90deg); }
  .ghd-file-name {
    flex: 1; min-width: 0;
    font-size: 12px; color: var(--text-1);
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .ghd-file-badge {
    font-size: 10px; font-weight: 600;
    padding: 1px 7px; border-radius: 999px;
    background: var(--accent-soft); color: var(--accent-bright);
  }
  .ghd-file-chg { display: inline-flex; gap: 7px; font-size: 11.5px; flex-shrink: 0; }
  .ghd-diffstat {
    display: inline-flex; gap: 1px;
    width: 56px; height: 4px; flex-shrink: 0;
    border-radius: 2px; overflow: hidden;
    background: var(--bg-3);
  }
  .ghd-diffstat-ok { height: 4px; background: var(--ok); }
  .ghd-diffstat-err { height: 4px; background: var(--err); }

  .ghd-diff-scroller {
    margin: 2px 0 10px;
    border: 1px solid var(--border-neutral); border-radius: 8px;
    overflow: auto; max-height: 620px;
    background: var(--bg-0);
  }
  .ghd-diff-body {
    font-family: var(--font-mono); font-size: 12px; line-height: 1.65;
    width: fit-content; min-width: 100%;
  }
  .ghd-hunk {
    padding: 4px 16px; font-size: 11px; color: var(--text-mute);
    background: var(--bg-1);
    border-bottom: 1px solid var(--border-neutral);
  }
  .ghd-diff-line { display: grid; grid-template-columns: 44px 1fr; }
  .ghd-diff-num {
    text-align: right; padding: 0 10px;
    color: var(--text-mute); font-size: 10.5px; user-select: none;
    background: var(--bg-0); border-right: 1px solid var(--border-neutral);
    position: sticky; left: 0;
  }
  .ghd-diff-content { padding: 0 14px; white-space: pre; color: var(--text-1); }
  .ghd-diff-line.add .ghd-diff-content { background: color-mix(in srgb, var(--ok) 8%, transparent); color: color-mix(in srgb, var(--ok) 78%, var(--text-0)); }
  .ghd-diff-line.add .ghd-diff-num { background: color-mix(in srgb, var(--ok) 12%, transparent); }
  .ghd-diff-line.del .ghd-diff-content { background: color-mix(in srgb, var(--err) 8%, transparent); color: color-mix(in srgb, var(--err) 82%, var(--text-0)); }
  .ghd-diff-line.del .ghd-diff-num { background: color-mix(in srgb, var(--err) 12%, transparent); }
  .ghd-file-comments { padding: 0 0 12px 26px; }

  /* Conversation — border-left quotes. -------------------------------- */
  .ghd-quote {
    border-left: 2px solid var(--border-hi);
    padding-left: 14px;
    margin-bottom: 18px;
  }
  .ghd-quote-head {
    display: flex; align-items: center; gap: 8px;
    font-size: 11.5px; color: var(--text-faint);
    margin-bottom: 6px;
  }
  .ghd-quote-body {
    font-size: 13.5px; line-height: 1.6; color: var(--text-1);
  }
  .ghd-quote-empty { font-size: 12.5px; color: var(--text-mute); }
  .ghd-quote-time { margin-left: auto; color: var(--text-mute); font-size: 11px; }
  .ghd-quote-line {
    color: var(--text-2); font-size: 11px;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap; max-width: 300px;
  }
  .ghd-review-state {
    padding: 1px 8px; border-radius: 4px;
    font-size: 10px; font-weight: 600;
    text-transform: uppercase; letter-spacing: 0.04em;
    color: var(--text-2); background: var(--bg-2);
    border: 1px solid var(--border-neutral-hi);
  }
  .ghd-quote--review.rev--approved { border-left-color: var(--ok); }
  .ghd-quote--review.rev--approved .ghd-review-state { color: var(--accent-bright); background: var(--accent-soft); border-color: color-mix(in srgb, var(--ok) 30%, transparent); }
  .ghd-quote--review.rev--changes { border-left-color: var(--err); }
  .ghd-quote--review.rev--changes .ghd-review-state { color: var(--err); background: color-mix(in srgb, var(--err) 8%, transparent); border-color: color-mix(in srgb, var(--err) 28%, transparent); }
  .ghd-quote--review.rev--commented .ghd-review-state { color: var(--blue-bright); background: color-mix(in srgb, var(--blue) 10%, transparent); border-color: color-mix(in srgb, var(--blue) 24%, transparent); }
  .ghd-quote--nested { margin-bottom: 12px; }

  .ghd-inline-toggle {
    margin-top: 8px;
    display: inline-flex; align-items: center; gap: 6px;
    background: none; border: none; padding: 0; cursor: pointer;
    color: var(--accent-bright); font-size: 12px; font-weight: 500;
  }
  .ghd-inline-toggle:hover { text-decoration: underline; }
  .ghd-inline-list { margin-top: 10px; }

  /* Slim commit rows in the timeline. */
  .ghd-commit {
    display: flex; align-items: center; gap: 8px;
    width: 100%; padding: 5px 0; margin-bottom: 12px;
    background: none; border: none; cursor: pointer; text-align: left;
    font-size: 12px; color: var(--text-1);
  }
  .ghd-commit-icon {
    width: 22px; height: 22px; flex-shrink: 0;
    display: inline-flex; align-items: center; justify-content: center;
    border-radius: 50%;
    background: var(--bg-2); border: 1px solid var(--border-neutral-hi);
    color: var(--text-2);
  }
  .ghd-commit-icon svg { width: 12px; height: 12px; }
  .ghd-commit-msg {
    flex: 1; min-width: 0;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
    color: var(--text-0);
  }
  .ghd-commit:hover .ghd-commit-msg { color: var(--accent-bright); }
  .ghd-commit-sha {
    padding: 1px 6px; border-radius: 4px;
    background: var(--bg-2); border: 1px solid var(--border-neutral-hi);
    color: var(--text-1); font-size: 10.5px;
  }

  /* Action bar --------------------------------------------------------- */
  /* Flush on the sheet (mockup 4e) — the footer is pinned, nothing
     scrolls under it, so the old --backdrop blur just read as an odd
     lighter band. Match the document bg + a hairline separator. */
  .ghd-actions {
    display: flex; align-items: center; gap: 8px;
    padding: 12px 24px;
    border-top: 1px solid var(--border-lo);
    background: var(--bg-0);
  }
  .ghd-act {
    display: inline-flex; align-items: center; gap: 5px;
    padding: 7px 14px; border-radius: 7px;
    font-size: 12.5px; font-weight: 500; cursor: pointer;
    transition: all 120ms;
  }
  .ghd-act:disabled { opacity: 0.5; cursor: not-allowed; }
  .ghd-caret { width: 13px; height: 13px; opacity: 0.7; }
  .ghd-act--primary {
    background: var(--accent); color: var(--accent-contrast, var(--bg-0));
    border: 1px solid var(--accent);
  }
  .ghd-act--primary:hover:not(:disabled) { background: var(--accent-bright); border-color: var(--accent-bright); }
  .ghd-act--ghost {
    background: transparent; color: var(--text-1);
    border: 1px solid var(--border-neutral-hi);
  }
  .ghd-act--ghost:hover:not(:disabled) { background: var(--bg-2); color: var(--text-0); }
  .ghd-merge-note { font-size: 11.5px; color: var(--text-faint); }
</style>
