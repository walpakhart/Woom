<script lang="ts">
  import { tick } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { setDragPayload } from '$lib/state/drag.svelte';
  import { attachDragChip } from '$lib/dragImage';
  import { iconFor } from '$lib/components/editor/fileIcons';

  interface Entry { name: string; path: string; is_dir: boolean; size: number; }
  interface Item { name: string; path: string; is_dir: boolean; depth: number; expanded: boolean; ignored: boolean; }

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
  let treeContainer = $state<HTMLDivElement | null>(null);
  // Path we last revealed — guards against re-running the expand-and-scroll
  // dance on every reactive flicker (e.g. an unrelated state update). Reset
  // when rootPath changes (new repo → forget what we revealed in the old).
  let lastRevealed = $state('');

  /** Batch-check `paths` against the repo's gitignore rules. Returns a Set
      of ignored absolute paths. Silent on failure (non-git dir, transient
      git error) — the tree keeps rendering without dimming. */
  async function checkIgnored(paths: string[]): Promise<Set<string>> {
    if (!rootPath || paths.length === 0) return new Set();
    try {
      const out = await invoke<string[]>('git_check_ignore', { repo: rootPath, paths });
      return new Set(out);
    } catch {
      return new Set();
    }
  }

  async function loadRoot() {
    if (!rootPath) {
      items = [];
      return;
    }
    loading = true;
    error = null;
    try {
      const kids = await invoke<Entry[]>('fs_list_dir', { path: rootPath });
      const ignored = await checkIgnored(kids.map((e) => e.path));
      items = kids.map((e) => ({
        name: e.name, path: e.path, is_dir: e.is_dir, depth: 0, expanded: false,
        ignored: ignored.has(e.path)
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
    const ignoredHere = await checkIgnored(kids.map((e) => e.path));
    const inserted: Item[] = kids.map((e) => ({
      name: e.name, path: e.path, is_dir: e.is_dir, depth: it.depth + 1, expanded: false,
      // A child inside an already-ignored dir is also ignored by definition —
      // saves a follow-up `git check-ignore` roundtrip for deep ignored trees.
      ignored: it.ignored || ignoredHere.has(e.path)
    }));
    items = [
      ...items.slice(0, idx),
      { ...it, expanded: true },
      ...inserted,
      ...items.slice(idx + 1)
    ];
  }

  $effect(() => { void loadRoot(); lastRevealed = ''; });

  /** Walk from `rootPath` down to `target`, expanding every parent
   *  folder that's collapsed along the way. Top-down so each toggle's
   *  freshly-fetched children become findable for the next iteration.
   *  Top-level files (no nesting) and paths outside `rootPath` are
   *  no-ops. After the tree settles we scroll the selected row into
   *  view — same UX as VSCode's Reveal in Explorer. */
  async function revealPath(target: string) {
    if (!target || !rootPath) return;
    if (!target.startsWith(rootPath + '/') && target !== rootPath) return;
    const rel = target.slice(rootPath.length + 1);
    const segments = rel.split('/').filter(Boolean);
    let cur = rootPath;
    // Expand every PARENT (skip the last segment — that's the target itself).
    for (let i = 0; i < segments.length - 1; i++) {
      cur = `${cur}/${segments[i]}`;
      const idx = items.findIndex((it) => it.path === cur && it.is_dir);
      // If a parent isn't in the tree yet, the previous toggle didn't
      // produce it (might be hidden by a virtualised list later, or
      // a race with rootPath reload). Bail rather than loop forever.
      if (idx < 0) return;
      if (!items[idx].expanded) {
        await toggle(idx);
      }
    }
    await tick();
    const rowIdx = items.findIndex((it) => it.path === target);
    if (rowIdx < 0 || !treeContainer) return;
    const row = treeContainer.querySelectorAll('.tree-row')[rowIdx] as HTMLElement | undefined;
    row?.scrollIntoView({ block: 'nearest', behavior: 'smooth' });
  }

  /** Drive `revealPath` from the `selectedPath` prop. Skips no-op
   *  re-runs (`lastRevealed === selectedPath`) so a brief unrelated
   *  state churn doesn't re-walk the tree. */
  $effect(() => {
    if (!selectedPath || selectedPath === lastRevealed) return;
    if (items.length === 0) return; // root not loaded yet — wait
    lastRevealed = selectedPath;
    void revealPath(selectedPath);
  });

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

<div class="tree" bind:this={treeContainer}>
  {#if loading}<div class="tree-state">Loading…</div>{/if}
  {#if error}<div class="tree-state tree-error">{error}</div>{/if}
  {#each items as it, i (it.path)}
    <button
      class="tree-row"
      class:selected={selectedPath === it.path && !it.is_dir}
      class:dir={it.is_dir}
      class:ignored={it.ignored}
      style="padding-left: {8 + it.depth * 12}px"
      onclick={() => toggle(i)}
      title={it.ignored ? `${it.path}\n(gitignored)` : it.path}
      draggable="true"
      ondragstart={(e) => {
        if (!e.dataTransfer) return;
        const payload = { path: it.path, isDir: it.is_dir, name: it.name };
        // Module state is the authoritative payload (WKWebView hides the
        // custom application/x-* mime on `dataTransfer.types` during
        // dragover, so drop targets can't rely on the mime to detect us).
        // We still set the mime for non-WKWebView platforms / other apps.
        setDragPayload({ source: 'file', ...payload });
        e.dataTransfer.setData('application/x-forgehold-file', JSON.stringify(payload));
        e.dataTransfer.setData('text/plain', it.path);
        e.dataTransfer.effectAllowed = 'copy';
        attachDragChip(e, it.is_dir ? 'dir' : 'file', it.name);
      }}
      ondragend={() => setDragPayload(null)}
    >
      <span class="tree-chevron">
        {#if it.is_dir}
          <svg class="i i-sm" viewBox="0 0 24 24" style="transform: rotate({it.expanded ? 90 : 0}deg)"><path d="M9 6l6 6-6 6" /></svg>
        {:else}
          <span class="tree-chevron-pad"></span>
        {/if}
      </span>
      {#snippet typeIcon()}
        {@const icon = iconFor(it.name, it.is_dir, it.expanded)}
        <svg class="tree-icon" viewBox="0 0 24 24" aria-hidden="true">
          <path d={icon.d}/>
          {#if icon.d2}<path d={icon.d2}/>{/if}
        </svg>
      {/snippet}
      {@render typeIcon()}
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
  /* Gitignored files/dirs — dimmed + italic so they read as "outside git"
     at a glance (mirrors VS Code / IntelliJ). `.selected` still wins, so
     opening an ignored file still shows the accent highlight. */
  .tree-row.ignored { color: var(--text-mute); font-style: italic; opacity: 0.65; }
  .tree-row.ignored:hover { color: var(--text-2); opacity: 0.85; }
  .tree-row.ignored.dir { color: var(--text-mute); font-weight: 400; }
  .tree-chevron {
    display: inline-flex; width: 14px; height: 14px;
    align-items: center; justify-content: center; flex-shrink: 0;
    color: var(--text-2);
  }
  .tree-chevron :global(svg) { width: 11px; height: 11px; transition: transform 120ms ease; }
  .tree-chevron-pad { width: 11px; height: 11px; }
  /* Type icon — drawn from `fileIcons.ts` SVG paths. Stroke-only,
     monochrome, takes its colour from the row text so dimmed /
     ignored rows fade out together with their label. */
  .tree-icon {
    width: 14px; height: 14px;
    flex-shrink: 0;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.5;
    stroke-linecap: round;
    stroke-linejoin: round;
    color: var(--text-2);
    opacity: 0.85;
  }
  .tree-row.dir .tree-icon { color: var(--text-1); opacity: 1; }
  .tree-row.selected .tree-icon { color: var(--accent-bright); }
  .tree-row.ignored .tree-icon { color: var(--text-mute); opacity: 0.55; }
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
