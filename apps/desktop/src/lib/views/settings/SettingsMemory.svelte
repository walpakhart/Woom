<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { notify, notifyError } from '$lib/state/toaster.svelte';
  import { formatBytes } from './shared';

  interface MemStats {
    total: number;
    by_kind: Record<string, number>;
    db_bytes: number;
  }
  let memStats = $state<MemStats | null>(null);
  let memStatsBusy = $state(false);
  const memKinds = ['user', 'feedback', 'project', 'reference', 'note'] as const;

  async function refreshMemStats(): Promise<void> {
    if (memStatsBusy) return;
    memStatsBusy = true;
    try {
      memStats = await invoke<MemStats>('memory_stats_local');
    } catch (e) {
      notifyError(e, { title: 'Could not read memory stats' });
    } finally {
      memStatsBusy = false;
    }
  }

  $effect(() => { void refreshMemStats(); });

  // ---- Browser ----------------------------------------------------------

  interface MemRow {
    id: number;
    kind: string;
    content: string;
    tags: string;
    created_at: number;
  }
  let memBrowserOpen = $state(false);
  let memBrowserQuery = $state('');
  let memBrowserKind = $state<string | null>(null);
  let memBrowserRows = $state<MemRow[]>([]);
  let memBrowserBusy = $state(false);
  let memBrowserDebounce: ReturnType<typeof setTimeout> | null = null;

  async function loadMemBrowser(): Promise<void> {
    if (memBrowserBusy) return;
    memBrowserBusy = true;
    try {
      const q = memBrowserQuery.trim();
      if (q) {
        memBrowserRows = await invoke<MemRow[]>('memory_search_local', {
          query: q,
          limit: 50
        });
      } else {
        memBrowserRows = await invoke<MemRow[]>('memory_list_local', {
          kind: memBrowserKind,
          limit: 50,
          offset: 0
        });
      }
    } catch (e) {
      notifyError(e, { title: 'Memory browser failed' });
    } finally {
      memBrowserBusy = false;
    }
  }

  function scheduleMemReload(): void {
    if (memBrowserDebounce) clearTimeout(memBrowserDebounce);
    memBrowserDebounce = setTimeout(() => void loadMemBrowser(), 250);
  }

  $effect(() => {
    if (memBrowserOpen && memBrowserRows.length === 0 && !memBrowserBusy) {
      void loadMemBrowser();
    }
  });

  $effect(() => {
    void memBrowserQuery;
    void memBrowserKind;
    if (memBrowserOpen) scheduleMemReload();
  });

  let editingMemId = $state<number | null>(null);
  let editDraftContent = $state('');
  let editDraftKind = $state('note');
  let editDraftTags = $state('');
  let editBusy = $state(false);

  function startEditMemRow(row: MemRow): void {
    editingMemId = row.id;
    editDraftContent = row.content;
    editDraftKind = row.kind;
    editDraftTags = row.tags;
  }
  function cancelEditMemRow(): void {
    editingMemId = null;
    editDraftContent = '';
    editDraftKind = 'note';
    editDraftTags = '';
  }
  async function saveEditMemRow(): Promise<void> {
    if (editingMemId === null) return;
    const id = editingMemId;
    const content = editDraftContent.trim();
    if (!content) {
      notify({ kind: 'error', title: 'Content cannot be empty', ttlMs: 2200 });
      return;
    }
    editBusy = true;
    try {
      const tagsArr = editDraftTags
        .split(',')
        .map((t) => t.trim())
        .filter((t) => t.length > 0);
      await invoke<number>('memory_update_local', {
        id,
        content,
        kind: editDraftKind,
        tags: tagsArr
      });
      memBrowserRows = memBrowserRows.map((r) =>
        r.id === id
          ? { ...r, content, kind: editDraftKind, tags: tagsArr.join(',') }
          : r
      );
      cancelEditMemRow();
      void refreshMemStats();
      notify({ kind: 'success', title: 'Memory updated', ttlMs: 1800 });
    } catch (e) {
      notifyError(e, { title: 'Update failed' });
    } finally {
      editBusy = false;
    }
  }

  async function deleteMemRow(id: number): Promise<void> {
    if (!window.confirm(`Delete memory #${id}? This can't be undone.`)) return;
    try {
      await invoke<number>('memory_delete_local', { id });
      memBrowserRows = memBrowserRows.filter((r) => r.id !== id);
      void refreshMemStats();
    } catch (e) {
      notifyError(e, { title: 'Delete failed' });
    }
  }

  function memRowDate(epoch: number): string {
    const d = new Date(epoch * 1000);
    const yyyy = d.getFullYear();
    const mm = String(d.getMonth() + 1).padStart(2, '0');
    const dd = String(d.getDate()).padStart(2, '0');
    return `${yyyy}-${mm}-${dd}`;
  }

  function memRowPreview(s: string): string {
    const collapsed = s.replace(/\s+/g, ' ').trim();
    return collapsed.length > 220 ? collapsed.slice(0, 217) + '…' : collapsed;
  }
</script>

<!-- Long-term memory -->
<div class="card">
  <header class="card-head">
    <h2 class="card-title">Long-term memory</h2>
    <p class="card-sub">
      Durable notes that persist across chat sessions. The agent searches them at the start of every turn; the paste-trap and chat archive flows also write here. Stored at <span class="mono">~/Library/Application Support/Woom/memory.db</span>.
    </p>
  </header>
  <div class="grid">
    <div class="stat">
      <div class="stat-label">Total memories</div>
      <div class="stat-value mono">{memStats?.total.toLocaleString() ?? '—'}</div>
    </div>
    <div class="stat">
      <div class="stat-label">DB size</div>
      <div class="stat-value mono">{memStats ? formatBytes(memStats.db_bytes) : '—'}</div>
    </div>
  </div>
  {#if memStats && memStats.total > 0}
    <div class="mem-breakdown">
      {#each memKinds as kind (kind)}
        {@const n = memStats.by_kind[kind] ?? 0}
        {#if n > 0}
          <span class="mem-chip mono" title="{n.toLocaleString()} {kind} memories">
            <span class="mem-chip-kind">{kind}</span>
            <span class="mem-chip-n">{n.toLocaleString()}</span>
          </span>
        {/if}
      {/each}
    </div>
  {/if}
  <div class="update-actions">
    <button class="btn btn--ghost" onclick={refreshMemStats} disabled={memStatsBusy}>
      {memStatsBusy ? 'Refreshing…' : 'Refresh'}
    </button>
    <button class="btn btn--ghost" onclick={() => (memBrowserOpen = !memBrowserOpen)}>
      {memBrowserOpen ? 'Hide browser' : 'Browse'}
    </button>
  </div>
  {#if memBrowserOpen}
    <div class="mem-browser">
      <div class="mem-browser-controls">
        <input
          class="mem-browser-search mono"
          type="text"
          bind:value={memBrowserQuery}
          placeholder="Search memories — words, project names, tags…"
          spellcheck="false"
          autocomplete="off"
        />
        <select
          class="mem-browser-kind"
          bind:value={memBrowserKind}
          disabled={memBrowserQuery.trim().length > 0}
          title={memBrowserQuery.trim() ? 'Kind filter disabled while a search query is active' : 'Filter by kind'}
        >
          <option value={null}>All kinds</option>
          {#each memKinds as kind (kind)}
            <option value={kind}>{kind}</option>
          {/each}
        </select>
        <button class="btn btn--ghost btn--sm" onclick={loadMemBrowser} disabled={memBrowserBusy}>
          {memBrowserBusy ? '…' : 'Reload'}
        </button>
      </div>
      <div class="mem-browser-list">
        {#if memBrowserRows.length === 0 && !memBrowserBusy}
          <div class="mem-browser-empty">
            {memBrowserQuery.trim() ? 'No matches for that query.' : 'No memories yet.'}
          </div>
        {:else}
          {#each memBrowserRows as row (row.id)}
            <div class="mem-row" class:mem-row--editing={editingMemId === row.id}>
              <div class="mem-row-head">
                <span class="mem-row-id mono">#{row.id}</span>
                <span class="mem-row-kind mono">{row.kind}</span>
                <span class="mem-row-date mono">{memRowDate(row.created_at)}</span>
                {#if row.tags}
                  <span class="mem-row-tags mono" title={row.tags}>{row.tags}</span>
                {/if}
                {#if editingMemId !== row.id}
                  <button
                    class="mem-row-edit"
                    onclick={() => startEditMemRow(row)}
                    title="Edit this memory"
                    aria-label="Edit memory #{row.id}"
                  >
                    <svg viewBox="0 0 24 24" width="13" height="13" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                      <path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7"/>
                      <path d="M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4z"/>
                    </svg>
                  </button>
                {/if}
                <button
                  class="mem-row-del"
                  onclick={() => void deleteMemRow(row.id)}
                  title="Delete this memory"
                  aria-label="Delete memory #{row.id}"
                >
                  <svg viewBox="0 0 24 24" width="13" height="13" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                    <polyline points="3 6 5 6 21 6"/>
                    <path d="M19 6l-2 14a2 2 0 0 1-2 2H9a2 2 0 0 1-2-2L5 6"/>
                    <path d="M10 11v6 M14 11v6"/>
                  </svg>
                </button>
              </div>
              {#if editingMemId === row.id}
                <div class="mem-row-edit-form">
                  <textarea
                    class="mem-row-edit-content mono"
                    bind:value={editDraftContent}
                    rows="6"
                    spellcheck="false"
                    placeholder="Memory content…"
                  ></textarea>
                  <div class="mem-row-edit-row">
                    <label class="mem-row-edit-field">
                      <span class="mem-row-edit-label">Kind</span>
                      <select bind:value={editDraftKind} class="mem-row-edit-kind">
                        {#each memKinds as k (k)}
                          <option value={k}>{k}</option>
                        {/each}
                      </select>
                    </label>
                    <label class="mem-row-edit-field mem-row-edit-field--grow">
                      <span class="mem-row-edit-label">Tags (comma)</span>
                      <input
                        type="text"
                        class="mem-row-edit-tags mono"
                        bind:value={editDraftTags}
                        placeholder="comma,separated,tags"
                        spellcheck="false"
                      />
                    </label>
                  </div>
                  <div class="mem-row-edit-actions">
                    <button class="btn btn--ghost btn--sm" onclick={cancelEditMemRow} disabled={editBusy}>Cancel</button>
                    <button class="btn btn--primary btn--sm" onclick={saveEditMemRow} disabled={editBusy}>
                      {editBusy ? 'Saving…' : 'Save'}
                    </button>
                  </div>
                </div>
              {:else}
                <div class="mem-row-body">{memRowPreview(row.content)}</div>
              {/if}
            </div>
          {/each}
        {/if}
      </div>
    </div>
  {/if}
</div>
