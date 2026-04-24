<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';

  interface Entry { name: string; path: string; is_dir: boolean; size: number; }
  interface Item { name: string; path: string; is_dir: boolean; depth: number; expanded: boolean; }

  interface Props {
    rootPath: string;
    selectedPath: string;
    onSelect: (path: string) => void;
    /** Map of absolute path → 1-char git status code (M/A/D/?/R/U). */
    gitStatusByPath?: Record<string, string>;
  }
  let { rootPath, selectedPath, onSelect, gitStatusByPath = {} }: Props = $props();

  let items = $state<Item[]>([]);
  let loading = $state(false);
  let error = $state<string | null>(null);

  async function loadRoot() {
    if (!rootPath) {
      items = [];
      return;
    }
    loading = true;
    error = null;
    try {
      const kids = await invoke<Entry[]>('fs_list_dir', { path: rootPath });
      items = kids.map((e) => ({
        name: e.name, path: e.path, is_dir: e.is_dir, depth: 0, expanded: false
      }));
    } catch (e: unknown) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      loading = false;
    }
  }

  async function toggle(idx: number) {
    const it = items[idx];
    if (!it) return;
    if (!it.is_dir) {
      onSelect(it.path);
      return;
    }
    if (it.expanded) {
      // collapse: drop all following items whose depth > it.depth, until we hit a sibling
      const drop: number[] = [];
      for (let j = idx + 1; j < items.length; j++) {
        if (items[j].depth <= it.depth) break;
        drop.push(j);
      }
      items = [...items.slice(0, idx), { ...it, expanded: false }, ...items.slice(idx + drop.length + 1)];
      return;
    }
    // expand: fetch children, insert after
    let kids: Entry[] = [];
    try {
      kids = await invoke<Entry[]>('fs_list_dir', { path: it.path });
    } catch (e: unknown) {
      error = e instanceof Error ? e.message : String(e);
      return;
    }
    const inserted: Item[] = kids.map((e) => ({
      name: e.name, path: e.path, is_dir: e.is_dir, depth: it.depth + 1, expanded: false
    }));
    items = [
      ...items.slice(0, idx),
      { ...it, expanded: true },
      ...inserted,
      ...items.slice(idx + 1)
    ];
  }

  $effect(() => { void loadRoot(); });

  function gitClass(code: string): string {
    switch (code) {
      case 'M': return 'mod';
      case 'A': return 'add';
      case 'D': return 'del';
      case 'R': return 'ren';
      case '?': return 'new';
      case 'U': return 'conflict';
      default: return 'mod';
    }
  }
  function gitTitle(code: string): string {
    switch (code) {
      case 'M': return 'Modified';
      case 'A': return 'Added';
      case 'D': return 'Deleted';
      case 'R': return 'Renamed';
      case '?': return 'Untracked';
      case 'U': return 'Conflict';
      default: return code;
    }
  }
</script>

<div class="tree">
  {#if loading}<div class="tree-state">Loading…</div>{/if}
  {#if error}<div class="tree-state tree-error">{error}</div>{/if}
  {#each items as it, i (it.path)}
    <button
      class="tree-row"
      class:selected={selectedPath === it.path && !it.is_dir}
      class:dir={it.is_dir}
      style="padding-left: {8 + it.depth * 12}px"
      onclick={() => toggle(i)}
      title={it.path}
      draggable="true"
      ondragstart={(e) => {
        if (!e.dataTransfer) return;
        const payload = { path: it.path, isDir: it.is_dir, name: it.name };
        e.dataTransfer.setData('application/x-forgehold-file', JSON.stringify(payload));
        e.dataTransfer.setData('text/plain', it.path);
        e.dataTransfer.effectAllowed = 'copy';
      }}
    >
      <span class="tree-chevron">
        {#if it.is_dir}
          <svg class="i i-sm" viewBox="0 0 24 24" style="transform: rotate({it.expanded ? 90 : 0}deg)"><path d="M9 6l6 6-6 6" /></svg>
        {:else}
          <span class="tree-dot"></span>
        {/if}
      </span>
      <span class="tree-name mono">{it.name}</span>
      {#if !it.is_dir && gitStatusByPath[it.path]}
        {@const code = gitStatusByPath[it.path]}
        <span class="tree-git mono tree-git--{gitClass(code)}" title={gitTitle(code)}>{code}</span>
      {/if}
    </button>
  {/each}
</div>

<style>
  .tree { height: 100%; overflow: auto; padding: 4px 0; }
  .tree-state { padding: 8px 14px; font-size: 11.5px; color: var(--text-2); }
  .tree-error { color: var(--error); }
  .tree-row {
    display: flex; align-items: center; gap: 6px;
    width: 100%; padding: 3px 8px 3px 8px;
    font-size: 12.5px; color: var(--text-1);
    text-align: left; border-radius: 0;
    background: transparent;
    transition: background 80ms ease;
  }
  .tree-row:hover { background: var(--bg-2); color: var(--text-0); }
  .tree-row.selected { background: var(--accent-soft); color: var(--accent-bright); }
  .tree-row.dir { color: var(--text-0); font-weight: 500; }
  .tree-chevron {
    display: inline-flex; width: 14px; height: 14px;
    align-items: center; justify-content: center; flex-shrink: 0;
    color: var(--text-2);
  }
  .tree-chevron :global(svg) { width: 11px; height: 11px; transition: transform 120ms ease; }
  .tree-dot { width: 3px; height: 3px; border-radius: 50%; background: var(--text-mute); }
  .tree-name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; flex: 1; }
  .tree-git {
    font-size: 10px; font-weight: 600;
    padding: 0 5px;
    border-radius: 3px;
    margin-left: 6px;
    flex-shrink: 0;
    min-width: 14px; text-align: center;
  }
  .tree-git--mod { color: var(--warning); background: rgba(229, 162, 42, 0.14); }
  .tree-git--add { color: var(--success); background: rgba(217, 145, 60, 0.16); }
  .tree-git--del { color: var(--error); background: rgba(214, 72, 44, 0.18); }
  .tree-git--new { color: var(--accent-bright); background: var(--accent-soft); }
  .tree-git--ren { color: var(--accent); background: var(--accent-soft); }
  .tree-git--conflict { color: var(--error); background: rgba(214, 72, 44, 0.25); }
</style>
