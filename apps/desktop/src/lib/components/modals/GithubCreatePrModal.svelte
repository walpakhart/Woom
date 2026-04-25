<script lang="ts">
  import { relativeTime } from '$lib/data';
  import { firstLine } from '$lib/format';
  import { modalsState, closeModal, patchModal } from '$lib/state/modals.svelte';
  import Dropdown, { type DropdownOption } from '$lib/components/ui/Dropdown.svelte';

  interface Props {
    now: number;
    onRepoChange: (full: string) => Promise<void> | void;
    onBranchesChange: () => Promise<void> | void;
    onSubmit: () => Promise<void> | void;
  }
  let { now, onRepoChange, onBranchesChange, onSubmit }: Props = $props();

  const m = $derived(modalsState.githubCreatePr);

  const repoOpts = $derived<DropdownOption<string>[]>(
    m
      ? [
          { value: '', label: 'Select repository…' },
          ...m.repos.map((r) => ({ value: r.full_name, label: r.full_name }))
        ]
      : []
  );
  const branchOpts = $derived<DropdownOption<string>[]>(
    m
      ? [
          { value: '', label: m.branches.length ? 'Select…' : 'No branches' },
          ...m.branches.map((b) => ({ value: b.name, label: b.name }))
        ]
      : []
  );
  const canSubmit = $derived(
    !!m && !m.busy && !!m.repo && m.title.trim().length > 0 && !!m.base && !!m.head && m.base !== m.head
  );
</script>

{#if m}
  <div
    class="modal-backdrop"
    onclick={(e) => { if (e.target === e.currentTarget && !m.busy) closeModal('githubCreatePr'); }}
    onkeydown={(e) => { if (e.key === 'Escape' && !m.busy) closeModal('githubCreatePr'); }}
    role="dialog"
    aria-modal="true"
    tabindex="-1"
  >
    <div class="modal modal-xl">
      <header class="modal-head">
        <span class="conn-icon" style="background: #0a111e; color: #fff; border: 1px solid var(--border-neutral-hi);">PR</span>
        <div>
          <div class="modal-title">New pull request</div>
          <div class="modal-sub">{m.repo || 'pick a repository'}</div>
        </div>
        <button class="modal-close" onclick={() => closeModal('githubCreatePr')} disabled={m.busy} aria-label="Close">
          <svg class="i" viewBox="0 0 24 24"><path d="M18 6 6 18M6 6l12 12" /></svg>
        </button>
      </header>
      <div class="modal-body-scroll" style="padding: 0;">
        <div class="modal-body" style="padding-bottom: 6px;">
          <label class="field">
            <span class="field-label">Repository</span>
            <Dropdown
              value={m.repo}
              options={repoOpts}
              onChange={(v) => onRepoChange(v)}
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
                options={branchOpts}
                onChange={(v) => { patchModal('githubCreatePr', { base: v }); void onBranchesChange(); }}
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
                options={branchOpts}
                onChange={(v) => { patchModal('githubCreatePr', { head: v }); void onBranchesChange(); }}
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
            <input
              class="field-input"
              type="text"
              value={m.title}
              oninput={(e) => patchModal('githubCreatePr', { title: (e.currentTarget as HTMLInputElement).value })}
              placeholder="Pull request title"
              disabled={m.busy}
            />
          </label>
          <label class="field">
            <span class="field-label">Description (markdown)</span>
            <textarea
              class="field-textarea"
              value={m.body}
              oninput={(e) => patchModal('githubCreatePr', { body: (e.currentTarget as HTMLTextAreaElement).value })}
              placeholder="What does this PR do? How did you test it?"
              disabled={m.busy}
            ></textarea>
          </label>
          <label class="radio" style="align-items: center;">
            <input
              type="checkbox"
              checked={m.draft}
              onchange={(e) => patchModal('githubCreatePr', { draft: (e.currentTarget as HTMLInputElement).checked })}
              disabled={m.busy}
            />
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
                    onclick={() => patchModal('githubCreatePr', { filesExpanded: !m.filesExpanded })}
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
        <button class="btn btn--ghost" onclick={() => closeModal('githubCreatePr')} disabled={m.busy}>Cancel</button>
        <button class="btn btn--primary" onclick={() => void onSubmit()} disabled={!canSubmit}>
          {m.busy ? 'Creating…' : (m.draft ? 'Create draft PR' : 'Create PR')}
        </button>
      </footer>
    </div>
  </div>
{/if}
