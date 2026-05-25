<script lang="ts">
  /* EditorFileSearch — file-name fuzzy search panel for the Editor
     sidebar's "search" tab. Extracted from EditorView.svelte (wave-1
     phase-4 split) because it owns a self-contained slice: its own
     query / include / exclude state, a debounced effect that calls
     `fs_list_dir` to walk the repo, and a result-row list. Nothing
     about the host editor (active path, tabs, git status, etc.)
     leaks in here — only `rootPath` (which directory to walk) and
     `onSelect` (what to do when the user picks a row).

     v7-style: same chrome the inline version had — small search
     input + filter-toggle, optional include/exclude panel, empty /
     searching / no-results / list states. */
  import { invoke } from '@tauri-apps/api/core';

  interface Props {
    /** Repo root the search walks recursively. Empty string disables
     *  the search effect (the placeholder copy still renders). */
    rootPath: string;
    /** Callback when the user clicks a result row — receives the
     *  absolute file path. EditorView wires this to `openFile`. */
    onSelect: (absolutePath: string) => void;
  }
  let { rootPath, onSelect }: Props = $props();

  /* Last path segment of the repo root — used in the "Find files"
   *  empty-state hint. Inline-derived so the component stays
   *  dependency-free (no util import). */
  const rootLabel = $derived.by(() => {
    if (!rootPath) return 'this repo';
    const slash = rootPath.lastIndexOf('/');
    return slash >= 0 ? rootPath.slice(slash + 1) : rootPath;
  });

  let searchQuery = $state('');
  let searchInclude = $state('');
  let searchExclude = $state('');
  let showSearchFilters = $state(false);
  let searchResults = $state<{ path: string; name: string; rel: string }[]>([]);
  let searching = $state(false);
  let searchTimer: ReturnType<typeof setTimeout> | null = null;

  /** Walk the tree breadth-first via `fs_list_dir`, collecting up to
   *  80 file hits whose basename contains the query substring (case
   *  insensitive). Include / exclude filters are comma-separated
   *  substrings matched against the path *relative to root* (include)
   *  and against directory names while descending (exclude). 80 is a
   *  generous cap that fits comfortably in the sidebar without
   *  scrolling becoming useless. */
  async function searchFiles(
    root: string,
    query: string,
    include: string,
    exclude: string
  ): Promise<{ path: string; name: string; rel: string }[]> {
    const q = query.toLowerCase();
    const incl = include.split(',').map((s) => s.trim().toLowerCase()).filter(Boolean);
    const excl = exclude.split(',').map((s) => s.trim().toLowerCase()).filter(Boolean);
    const results: { path: string; name: string; rel: string }[] = [];
    const queue: string[] = [root];
    while (queue.length > 0 && results.length < 80) {
      const dir = queue.shift()!;
      let entries: { name: string; path: string; is_dir: boolean }[] = [];
      try {
        entries = await invoke('fs_list_dir', { path: dir });
      } catch { continue; }
      for (const e of entries) {
        if (e.is_dir) {
          const skip = excl.some((p) => e.name.toLowerCase().includes(p));
          if (!skip) queue.push(e.path);
        } else {
          const rel = e.path.startsWith(root + '/') ? e.path.slice(root.length + 1) : e.path;
          const relLow = rel.toLowerCase();
          if (excl.some((p) => relLow.includes(p))) continue;
          if (incl.length > 0 && !incl.some((p) => relLow.includes(p))) continue;
          if (e.name.toLowerCase().includes(q)) {
            results.push({ path: e.path, name: e.name, rel });
            if (results.length >= 80) break;
          }
        }
      }
    }
    return results;
  }

  $effect(() => {
    const q = searchQuery.trim();
    const inc = searchInclude;
    const exc = searchExclude;
    if (searchTimer) { clearTimeout(searchTimer); searchTimer = null; }
    if (q.length < 2) { searchResults = []; searching = false; return; }
    searching = true;
    searchTimer = setTimeout(async () => {
      if (!rootPath) { searching = false; return; }
      searchResults = await searchFiles(rootPath, q, inc, exc);
      searching = false;
    }, 280);
  });
</script>

<div class="efs">
  <div class="efs-bar">
    <input
      class="efs-input mono"
      placeholder="Search files by name…"
      type="search"
      bind:value={searchQuery}
    />
    <button
      class="efs-filter-toggle"
      class:efs-filter-toggle--active={showSearchFilters}
      title={showSearchFilters ? 'Hide filters' : 'Show include / exclude filters'}
      onclick={() => (showSearchFilters = !showSearchFilters)}
      aria-label="Toggle filters"
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"><path d="M22 3H2l8 9.46V19l4 2v-8.54L22 3z"/></svg>
    </button>
  </div>
  {#if showSearchFilters}
    <div class="efs-filters">
      <label class="efs-filter-row">
        <span class="efs-filter-label mono">include</span>
        <input
          class="efs-filter-input mono"
          placeholder="src/lib, *.ts, …"
          bind:value={searchInclude}
          title="Comma-separated substrings — only match files whose path contains one of these"
        />
      </label>
      <label class="efs-filter-row">
        <span class="efs-filter-label mono">exclude</span>
        <input
          class="efs-filter-input mono"
          placeholder="node_modules, dist, …"
          bind:value={searchExclude}
          title="Comma-separated substrings — skip dirs/files whose path contains one of these"
        />
      </label>
    </div>
  {/if}
  {#if searchQuery.trim().length < 2}
    <div class="efs-empty">
      <p class="efs-empty-h serif">Find files</p>
      <p class="efs-empty-p">Type 2+ characters to search filenames in <span class="mono">{rootLabel}</span>.</p>
    </div>
  {:else if searching}
    <div class="efs-empty">
      <p class="efs-empty-p">Searching…</p>
    </div>
  {:else if searchResults.length === 0}
    <div class="efs-empty">
      <p class="efs-empty-p">No files found for <span class="mono">"{searchQuery}"</span></p>
    </div>
  {:else}
    <div class="efs-results">
      {#each searchResults as r (r.path)}
        <button
          class="efs-result"
          onclick={() => onSelect(r.path)}
          title={r.path}
        >
          <span class="efs-result-name mono">{r.name}</span>
          <span class="efs-result-dir mono">{r.rel.slice(0, r.rel.length - r.name.length - 1) || '/'}</span>
        </button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .efs {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
    padding: 8px 10px 0;
    gap: 8px;
  }
  .efs-bar {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .efs-input {
    flex: 1;
    height: 28px;
    padding: 0 10px;
    background: var(--bg-2);
    color: var(--text-0);
    border: 1px solid var(--border);
    border-radius: 6px;
    font-size: 12px;
    outline: none;
  }
  .efs-input:focus {
    border-color: var(--accent);
  }
  .efs-filter-toggle {
    width: 28px;
    height: 28px;
    display: grid;
    place-items: center;
    background: transparent;
    border: 1px solid var(--border);
    border-radius: 6px;
    color: var(--text-2);
    cursor: pointer;
    transition:
      background var(--dur-quick) var(--ease-out),
      border-color var(--dur-quick) var(--ease-out),
      color var(--dur-quick) var(--ease-out);
  }
  .efs-filter-toggle:hover {
    color: var(--text-0);
    background: var(--bg-2);
  }
  .efs-filter-toggle--active {
    color: var(--accent);
    border-color: var(--accent);
    background: color-mix(in srgb, var(--accent) 10%, transparent);
  }
  .efs-filter-toggle svg {
    width: 13px;
    height: 13px;
  }
  .efs-filters {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 8px;
    background: var(--bg-2);
    border: 1px solid var(--border);
    border-radius: 6px;
  }
  .efs-filter-row {
    display: grid;
    grid-template-columns: 56px 1fr;
    align-items: center;
    gap: 8px;
  }
  .efs-filter-label {
    font-size: 10px;
    color: var(--text-2);
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }
  .efs-filter-input {
    height: 24px;
    padding: 0 8px;
    background: var(--bg-0);
    color: var(--text-0);
    border: 1px solid var(--border);
    border-radius: 4px;
    font-size: 11px;
    outline: none;
  }
  .efs-filter-input:focus {
    border-color: var(--accent);
  }
  .efs-empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: 28px 18px;
    gap: 8px;
    text-align: center;
    color: var(--text-2);
  }
  .efs-empty-h {
    margin: 0;
    font-size: 14px;
    color: var(--text-0);
  }
  .efs-empty-p {
    margin: 0;
    font-size: 12px;
    line-height: 1.4;
  }
  .efs-results {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding-bottom: 8px;
  }
  .efs-result {
    display: grid;
    grid-template-columns: minmax(0, auto) minmax(0, 1fr);
    align-items: baseline;
    gap: 8px;
    padding: 4px 8px;
    background: transparent;
    border: none;
    border-radius: 4px;
    color: var(--text-0);
    text-align: left;
    cursor: pointer;
    font-size: 12px;
    transition: background var(--dur-quick) var(--ease-out);
  }
  .efs-result:hover {
    background: var(--bg-2);
  }
  .efs-result-name {
    color: var(--text-0);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .efs-result-dir {
    color: var(--text-mute);
    font-size: 10px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
