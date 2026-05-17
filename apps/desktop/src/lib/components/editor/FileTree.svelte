<script lang="ts">
  import { tick, onMount, onDestroy } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
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

  /* Per-repo expanded-paths cache. localStorage key includes the
     repo root so two open folders don't trample each other's tree
     state. Saved on every toggle + smartRefresh; restored on
     mount / repo switch via the same walk-and-expand logic that
     smartRefresh uses on watcher events. */
  function treeExpandKey(root: string): string {
    return `woom:editor:tree-expanded:v1:${root}`;
  }
  function readExpandedFromCache(root: string): Set<string> {
    if (!root) return new Set();
    try {
      const raw = localStorage.getItem(treeExpandKey(root));
      if (!raw) return new Set();
      const parsed = JSON.parse(raw);
      if (!Array.isArray(parsed)) return new Set();
      return new Set(parsed.filter((p): p is string => typeof p === 'string'));
    } catch {
      return new Set();
    }
  }
  function writeExpandedToCache(): void {
    if (!rootPath) return;
    try {
      const paths = items.filter((it) => it.is_dir && it.expanded).map((it) => it.path);
      localStorage.setItem(treeExpandKey(rootPath), JSON.stringify(paths));
    } catch {
      /* localStorage full / unavailable — non-essential, skip. */
    }
  }

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
      /* Restore previously-expanded dirs from localStorage and walk
         into them so the tree comes back the way the user left it.
         No cached state → standard one-level root listing. */
      const cached = readExpandedFromCache(rootPath);
      if (cached.size === 0) {
        const kids = await invoke<Entry[]>('fs_list_dir', { path: rootPath });
        const ignored = await checkIgnored(kids.map((e) => e.path));
        items = kids.map((e) => ({
          name: e.name, path: e.path, is_dir: e.is_dir, depth: 0, expanded: false,
          ignored: ignored.has(e.path)
        }));
      } else {
        /* Use the same walk-and-rebuild path smartRefresh uses, but
           seeded from the cached `expanded` set instead of the
           current in-memory items (there ARE none on first mount). */
        const flat: Item[] = [];
        async function walk(parent: string, depth: number, parentIgnored: boolean): Promise<void> {
          const kids = await invoke<Entry[]>('fs_list_dir', { path: parent });
          const ignoredHere = await checkIgnored(kids.map((e) => e.path));
          for (const e of kids) {
            const ignored = parentIgnored || ignoredHere.has(e.path);
            const wasExpanded = e.is_dir && cached.has(e.path);
            flat.push({ name: e.name, path: e.path, is_dir: e.is_dir, depth, expanded: wasExpanded, ignored });
            if (wasExpanded) {
              try { await walk(e.path, depth + 1, ignored); }
              catch { /* skip kids whose dir disappeared since last save */ }
            }
          }
        }
        await walk(rootPath, 0, false);
        items = flat;
      }
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
      writeExpandedToCache();
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
    writeExpandedToCache();
  }

  $effect(() => { void loadRoot(); lastRevealed = ''; });

  /** Smart refresh after a filesystem change emitted by the Rust watcher.
      Snapshot which dirs are currently expanded, reload root + each
      expanded subtree, then rebuild the flat `items` array preserving
      the previous expansion state. New / deleted files surface; the
      tree doesn't collapse to root every time Claude writes a file. */
  let refreshDebounce: ReturnType<typeof setTimeout> | null = null;
  async function smartRefresh() {
    if (!rootPath) return;
    const expanded = new Set<string>(items.filter((it) => it.is_dir && it.expanded).map((it) => it.path));
    try {
      const flat: Item[] = [];
      async function walk(parentPath: string, depth: number, parentIgnored: boolean): Promise<void> {
        const kids = await invoke<Entry[]>('fs_list_dir', { path: parentPath });
        const ignoredHere = await checkIgnored(kids.map((e) => e.path));
        for (const e of kids) {
          const ignored = parentIgnored || ignoredHere.has(e.path);
          const wasExpanded = e.is_dir && expanded.has(e.path);
          flat.push({ name: e.name, path: e.path, is_dir: e.is_dir, depth, expanded: wasExpanded, ignored });
          if (wasExpanded) {
            try { await walk(e.path, depth + 1, ignored); }
            catch { /* dir went away mid-refresh — skip its kids */ }
          }
        }
      }
      await walk(rootPath, 0, false);
      items = flat;
      writeExpandedToCache();
    } catch (e: unknown) {
      // Don't blow up the UI on a refresh error — leave stale state in place,
      // the next event will retry. (Common cause: rootPath disappeared, the
      // watcher will eventually emit nothing and we just stop refreshing.)
      console.warn('FileTree.smartRefresh failed:', e);
    }
  }

  let watchUnlisten: UnlistenFn | null = null;
  onMount(async () => {
    // Coalesce bursts (Claude writing 5 files = 1 refresh, not 5).
    watchUnlisten = await listen<{ path: string; kind: string }>('fs:changed', () => {
      if (refreshDebounce) clearTimeout(refreshDebounce);
      refreshDebounce = setTimeout(() => { void smartRefresh(); }, 300);
    });
  });
  onDestroy(() => {
    watchUnlisten?.();
    if (refreshDebounce) clearTimeout(refreshDebounce);
  });

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
      // Parent not in tree — either a race with rootPath reload, or the
      // file is reachable via a symlink that fs_list_dir already resolved
      // (pnpm: tree shows node_modules/pkg/ but item paths are .pnpm/...).
      // Stop expanding but still try to scroll to the target if it's
      // already visible in the current items list.
      if (idx < 0) break;
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

  /* Right-click context menu (M4 §2.1.2). Standard macOS Finder
   * complement: Reveal, Copy path, Rename, Delete. Anchored at the
   * cursor; closes on outside click / Esc. Rename is inline (an
   * input swaps in for the row label); delete confirms via
   * `window.confirm` since we don't have a non-modal confirmation
   * surface available from the tree component yet. */
  type ContextMenu = {
    x: number;
    y: number;
    item: Item;
  };
  let contextMenu = $state<ContextMenu | null>(null);
  let renaming = $state<{ path: string; original: string; draft: string } | null>(null);

  function openContextMenu(e: MouseEvent, it: Item) {
    e.preventDefault();
    contextMenu = { x: e.clientX, y: e.clientY, item: it };
  }
  function closeContextMenu() {
    contextMenu = null;
  }
  async function ctxRevealInFinder(it: Item) {
    closeContextMenu();
    try {
      await invoke('fs_reveal_in_finder', { path: it.path });
    } catch (e) {
      console.warn('fs_reveal_in_finder', e);
    }
  }
  async function ctxCopyPath(it: Item) {
    closeContextMenu();
    try {
      await navigator.clipboard.writeText(it.path);
    } catch (e) {
      console.warn('clipboard', e);
    }
  }
  function ctxRename(it: Item) {
    closeContextMenu();
    renaming = { path: it.path, original: it.name, draft: it.name };
  }
  async function commitRename() {
    if (!renaming) return;
    const next = renaming.draft.trim();
    if (!next || next === renaming.original) {
      renaming = null;
      return;
    }
    /* Build the destination path by replacing the basename. We
     * deliberately don't allow `/` in the new name — that would
     * effectively be a "move to subdir" operation and we want this
     * menu to stay focused on rename-in-place. */
    if (next.includes('/')) {
      renaming = null;
      return;
    }
    const lastSlash = renaming.path.lastIndexOf('/');
    const parent = lastSlash > 0 ? renaming.path.slice(0, lastSlash) : '';
    const dst = parent ? `${parent}/${next}` : next;
    try {
      await invoke('fs_rename', { from: renaming.path, to: dst });
    } catch (e) {
      console.warn('fs_rename', e);
    } finally {
      renaming = null;
    }
  }
  function cancelRename() {
    renaming = null;
  }
  async function ctxDelete(it: Item) {
    closeContextMenu();
    if (it.is_dir) {
      /* Directory delete is recursive — call it out explicitly in
       * the confirm so the user can't muscle-memory through "yes"
       * and lose a whole subtree. The Rust side has a depth guard
       * that blocks anything shallower than `/Users/<name>/x` so
       * a misclick can't wipe a system folder. */
      if (
        !window.confirm(
          `Delete the folder "${it.name}" and ALL its contents? This cannot be undone.`
        )
      )
        return;
      try {
        await invoke('fs_remove_dir', { path: it.path });
        // Notify open editors so they can close any tabs that lived
        // inside the deleted subtree. The fs watcher will refresh
        // the tree itself a moment later via smartRefresh().
        window.dispatchEvent(
          new CustomEvent('woom:fs:path-deleted', { detail: { path: it.path, isDir: true } })
        );
      } catch (e) {
        console.warn('fs_remove_dir', e);
        window.alert(`Couldn't delete folder: ${e instanceof Error ? e.message : String(e)}`);
      }
      return;
    }
    if (!window.confirm(`Delete ${it.name}? This cannot be undone.`)) return;
    try {
      await invoke('fs_remove_file', { path: it.path });
      window.dispatchEvent(
        new CustomEvent('woom:fs:path-deleted', { detail: { path: it.path, isDir: false } })
      );
    } catch (e) {
      console.warn('fs_remove_file', e);
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
      oncontextmenu={(e) => openContextMenu(e, it)}
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
        e.dataTransfer.setData('application/x-woom-file', JSON.stringify(payload));
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
      {#if renaming && renaming.path === it.path}
        <!-- Inline rename input — replaces the name label until the
             user commits (Enter) or cancels (Esc / blur). -->
        <!-- svelte-ignore a11y_autofocus -->
        <input
          class="tree-rename mono"
          bind:value={renaming.draft}
          autofocus
          onclick={(e) => e.stopPropagation()}
          onkeydown={(e) => {
            e.stopPropagation();
            if (e.key === 'Enter') void commitRename();
            else if (e.key === 'Escape') cancelRename();
          }}
          onblur={commitRename}
        />
      {:else}
        <span class="tree-name mono">{it.name}</span>
      {/if}
      {#if !it.is_dir && gitStatusByPath[it.path]}
        {@const code = gitStatusByPath[it.path]}
        <span class="tree-git mono tree-git--{gitClass(code)}" title={gitTitle(code)}>{code}</span>
      {/if}
    </button>
  {/each}
</div>

{#if contextMenu}
  <div
    class="tree-ctx-backdrop"
    onclick={closeContextMenu}
    onkeydown={(e) => { if (e.key === 'Escape') closeContextMenu(); }}
    role="presentation"
  ></div>
  <div class="tree-ctx" style="left: {contextMenu.x}px; top: {contextMenu.y}px" role="menu">
    <button class="tree-ctx-item" onclick={() => void ctxRevealInFinder(contextMenu!.item)} role="menuitem">
      Reveal in Finder
    </button>
    <button class="tree-ctx-item" onclick={() => void ctxCopyPath(contextMenu!.item)} role="menuitem">
      Copy path
    </button>
    <button class="tree-ctx-item" onclick={() => ctxRename(contextMenu!.item)} role="menuitem">
      Rename…
    </button>
    <button class="tree-ctx-item tree-ctx-item--danger" onclick={() => void ctxDelete(contextMenu!.item)} role="menuitem">
      Delete
    </button>
  </div>
{/if}

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
  .tree-row.ignored { color: var(--text-mute);  opacity: 0.65; }
  .tree-row.ignored:hover { color: var(--text-2); opacity: 0.85; }
  .tree-row.ignored.dir { color: var(--text-mute); font-weight: 400; }
  .tree-chevron {
    display: inline-flex; width: 14px; height: 14px;
    align-items: center; justify-content: center; flex-shrink: 0;
    color: var(--text-2);
  }
  .tree-chevron :global(svg) {
    width: 11px; height: 11px;
    /* Spring-out easing gives the chevron rotate a tiny snap on expand
       — much more satisfying than a flat ease for repeated clicks
       while exploring the tree. */
    transition: transform var(--dur-base) var(--ease-spring);
  }
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
  .tree-git--mod { color: var(--warning); background: rgba(217, 184, 110, 0.14); }
  .tree-git--add { color: var(--success); background: rgba(204, 120, 92, 0.16); }
  .tree-git--del { color: var(--error); background: rgba(232, 130, 100, 0.18); }
  .tree-git--new { color: var(--accent-bright); background: var(--accent-soft); }
  .tree-git--ren { color: var(--accent); background: var(--accent-soft); }
  .tree-git--conflict { color: var(--error); background: rgba(232, 130, 100, 0.25); }

  /* Inline rename input — sized to fit the row, takes the same font
     so the swap doesn't shift the row height. */
  .tree-rename {
    flex: 1; min-width: 0;
    padding: 1px 4px;
    background: var(--bg-0);
    border: 1px solid var(--accent);
    border-radius: 3px;
    color: var(--text-0);
    font-size: 12.5px;
    outline: none;
  }

  /* Right-click context menu. Positioned absolute at the cursor;
     backdrop captures outside clicks so the menu dismisses. */
  .tree-ctx-backdrop {
    position: fixed; inset: 0; z-index: 600;
    background: transparent;
  }
  .tree-ctx {
    position: fixed; z-index: 601;
    min-width: 180px;
    padding: 4px;
    background: var(--bg-3);
    border: 1px solid var(--border-neutral-hi);
    border-radius: 8px;
    box-shadow: 0 12px 32px rgba(0, 0, 0, 0.36);
  }
  .tree-ctx-item {
    display: block; width: 100%;
    padding: 6px 10px; border-radius: 5px;
    background: none; border: none; text-align: left;
    color: var(--text-1); font-size: 12px; cursor: pointer;
  }
  .tree-ctx-item:hover { background: var(--bg-2); color: var(--text-0); }
  .tree-ctx-item--danger { color: #F0A38A; }
  .tree-ctx-item--danger:hover { background: rgba(232, 130, 100, 0.12); }
</style>
