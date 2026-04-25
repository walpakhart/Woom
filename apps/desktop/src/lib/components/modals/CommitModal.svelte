<script lang="ts">
  import { parsePatch, relativeTime } from '$lib/data';
  import { firstLine, restLines } from '$lib/format';
  import { modalsState, closeModal, patchModal } from '$lib/state/modals.svelte';

  interface Props {
    /** Live timestamp for relative-time labels. */
    now: number;
    openBrowser: (url: string) => void | Promise<void>;
  }
  let { now, openBrowser }: Props = $props();

  const m = $derived(modalsState.commit);

  /** Toggle the per-file expand state in the modal. Mutates the same Set
   *  object so the registry doesn't think the modal closed. */
  function toggleFile(filename: string) {
    if (!m) return;
    const next = new Set(m.expanded);
    if (next.has(filename)) next.delete(filename);
    else next.add(filename);
    patchModal('commit', { expanded: next });
  }
</script>

{#if m}
  {@const cm = m}
  <div
    class="modal-backdrop"
    onclick={(e) => { if (e.target === e.currentTarget) closeModal('commit'); }}
    onkeydown={(e) => { if (e.key === 'Escape') closeModal('commit'); }}
    role="dialog"
    aria-modal="true"
    tabindex="-1"
  >
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
        <button class="modal-close" onclick={() => closeModal('commit')} aria-label="Close">
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
                <button class="file-head" onclick={() => toggleFile(f.filename)}>
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
                              <span class="diff-line-num">
                                {line.kind === 'add' ? '+' : line.kind === 'del' ? '−' : line.newLine ?? ''}
                              </span>
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
          <svg class="i i-sm" viewBox="0 0 24 24"><path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"/><path d="M15 3h6v6M10 14 21 3"/></svg>
          Open on GitHub
        </button>
        <div style="flex:1"></div>
        <button class="btn btn--primary" onclick={() => closeModal('commit')}>Close</button>
      </footer>
    </div>
  </div>
{/if}
