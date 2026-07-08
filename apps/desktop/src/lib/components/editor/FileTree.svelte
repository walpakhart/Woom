<script lang="ts">
  import { tick, onMount, onDestroy } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import { setDragPayload } from '$lib/state/drag.svelte';
  import { attachDragChip } from '$lib/dragImage';
  import { foldStatus } from '$lib/components/editor/gitDecorations';

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
    if (!treeContainer) return;
    // Compact folders mean row index ≠ item index — locate by path instead.
    const row = treeContainer.querySelector(
      `.etree-row[data-path="${CSS.escape(target)}"]`
    ) as HTMLElement | null;
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

  /* Folder rollup: aggregate each directory row's git status from its
     descendants. Computed in ONE pass over `gitStatusByPath` (no per-node
     scan, no git calls) — for each changed file we attribute its code to
     every ancestor-dir path that's currently present in the tree, then
     fold each bucket to its highest-severity code. Recomputes only when
     `gitStatusByPath` or `items` change. */
  const folderStatusByPath = $derived.by<Record<string, string>>(() => {
    const dirSet = new Set<string>();
    for (const it of items) if (it.is_dir) dirSet.add(it.path);
    if (dirSet.size === 0) return {};
    const buckets: Record<string, string[]> = {};
    for (const [path, code] of Object.entries(gitStatusByPath)) {
      // Walk up the ancestors of this file, attributing its code to any
      // ancestor dir we're actually rendering.
      let slash = path.lastIndexOf('/');
      while (slash > 0) {
        const dir = path.slice(0, slash);
        if (dirSet.has(dir)) (buckets[dir] ??= []).push(code);
        slash = path.lastIndexOf('/', slash - 1);
      }
    }
    const out: Record<string, string> = {};
    for (const [dir, codes] of Object.entries(buckets)) {
      const folded = foldStatus(codes);
      if (folded) out[dir] = folded;
    }
    return out;
  });

  /** Git code for a row (file → direct status, dir → rolled-up), or ''. */
  function rowCode(it: Item): string {
    return it.is_dir ? (folderStatusByPath[it.path] ?? '') : (gitStatusByPath[it.path] ?? '');
  }

  /* Compact folders (mockup 4i / 4j) — a directory whose parent dir has
     exactly ONE child (and that child is also a dir) merges up into a
     single row: `apps/desktop/src`, `views/apps/agent`. Purely a display
     transform over the flat `items` list — `toggle` / expand-cache /
     `revealPath` keep operating on the real per-dir items, so persistence
     and reveal are untouched. Each output row carries the DEEPEST item in
     its merged chain (drives chevron / selection / toggle) and a compacted
     `depth` (merged chains count as one indent level). */
  interface Row { item: Item; idx: number; display: string; depth: number; }
  const rows = $derived.by<Row[]>(() => {
    const n = items.length;
    if (n === 0) return [];
    // Parent index = nearest preceding item one depth shallower.
    const parent = new Array<number>(n).fill(-1);
    const stack: number[] = [];
    for (let i = 0; i < n; i++) {
      const d = items[i].depth;
      parent[i] = d > 0 ? (stack[d - 1] ?? -1) : -1;
      stack[d] = i;
      stack.length = d + 1;
    }
    const childCount = new Map<number, number>();
    for (let i = 0; i < n; i++) {
      const p = parent[i];
      if (p >= 0) childCount.set(p, (childCount.get(p) ?? 0) + 1);
    }
    const out: Row[] = [];
    const rowOf = new Array<number>(n).fill(-1);
    for (let i = 0; i < n; i++) {
      const it = items[i];
      const p = parent[i];
      const mergeUp =
        p >= 0 && rowOf[p] >= 0 &&
        items[p].is_dir && it.is_dir &&
        childCount.get(p) === 1;
      if (mergeUp) {
        const r = out[rowOf[p]];
        r.display += '/' + it.name;
        r.item = it;
        r.idx = i;
        rowOf[i] = rowOf[p];
      } else {
        const depth = p >= 0 && rowOf[p] >= 0 ? out[rowOf[p]].depth + 1 : 0;
        out.push({ item: it, idx: i, display: it.name, depth });
        rowOf[i] = out.length - 1;
      }
    }
    return out;
  });

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

  /* New file / new folder. The inline input is pinned at the top of the
   * tree (the flat list is rebuilt by the fs watcher, so injecting a row
   * at the exact parent slot mid-list is fragile). On commit we create
   * via the Rust fs command; the watcher's smartRefresh() folds in the
   * real row, and `revealPath` expands the parent + scrolls to it. */
  let creating = $state<{ parentDir: string; isDir: boolean; draft: string } | null>(null);

  function parentDirOf(it: Item): string {
    if (it.is_dir) return it.path;
    const lastSlash = it.path.lastIndexOf('/');
    return lastSlash > 0 ? it.path.slice(0, lastSlash) : (rootPath ?? '');
  }
  async function startCreate(it: Item, isDir: boolean) {
    closeContextMenu();
    const parentDir = parentDirOf(it);
    // Expand the target folder so the freshly-created child shows once
    // the watcher refresh lands.
    if (it.is_dir && !it.expanded) {
      const idx = items.findIndex((x) => x.path === it.path);
      if (idx >= 0) await toggle(idx);
    }
    creating = { parentDir, isDir, draft: '' };
  }
  async function commitCreate() {
    if (!creating) return;
    const name = creating.draft.trim();
    // Reject empty + path separators — keep this single-level (a name
    // with `/` would silently create nested dirs).
    if (!name || name.includes('/')) {
      creating = null;
      return;
    }
    const isDir = creating.isDir;
    const newPath = `${creating.parentDir}/${name}`;
    creating = null;
    try {
      await invoke(isDir ? 'fs_create_dir' : 'fs_create_file', { path: newPath });
      await revealPath(newPath);
    } catch (e) {
      console.warn(isDir ? 'fs_create_dir' : 'fs_create_file', e);
      window.alert(`Couldn't create ${isDir ? 'folder' : 'file'}: ${e instanceof Error ? e.message : String(e)}`);
    }
  }
  function cancelCreate() {
    creating = null;
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

<div class="etree" bind:this={treeContainer}>
  {#if loading}<div class="etree-state">Loading…</div>{/if}
  {#if error}<div class="etree-state etree-error">{error}</div>{/if}
  {#snippet createRow(depth: number)}
    <div class="etree-row etree-creating" style="padding-left: {8 + depth * 12}px">
      <span class="etree-chevron"><span class="etree-chevron-pad"></span></span>
      <!-- svelte-ignore a11y_autofocus -->
      <input
        class="etree-rename mono"
        value={creating?.draft ?? ''}
        oninput={(e) => { if (creating) creating.draft = e.currentTarget.value; }}
        autofocus
        placeholder={creating?.isDir ? 'new folder' : 'new file'}
        onclick={(e) => e.stopPropagation()}
        onkeydown={(e) => {
          e.stopPropagation();
          if (e.key === 'Enter') void commitCreate();
          else if (e.key === 'Escape') cancelCreate();
        }}
        onblur={commitCreate}
      />
    </div>
  {/snippet}
  <!-- Root-level create (target is the repo root, e.g. acted on a
       top-level file) renders at the top; folder-scoped create renders
       nested under its parent folder row inside the loop below. -->
  {#if creating && creating.parentDir === (rootPath ?? '')}
    {@render createRow(0)}
  {/if}
  {#each rows as r (r.item.path)}
    {@const it = r.item}
    <button
      class="etree-row"
      class:selected={selectedPath === it.path && !it.is_dir}
      class:dir={it.is_dir}
      class:ignored={it.ignored}
      class:create-target={creating?.parentDir === it.path}
      style="padding-left: {8 + r.depth * 12}px"
      data-path={it.path}
      onclick={() => toggle(r.idx)}
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
      <span class="etree-chevron">
        {#if it.is_dir}
          <svg class="i i-sm" viewBox="0 0 24 24" style="transform: rotate({it.expanded ? 90 : 0}deg)"><path d="M9 6l6 6-6 6" /></svg>
        {:else}
          <span class="etree-chevron-pad"></span>
        {/if}
      </span>
      {#if renaming && renaming.path === it.path}
        <!-- Inline rename input — replaces the name label until the
             user commits (Enter) or cancels (Esc / blur). -->
        <!-- svelte-ignore a11y_autofocus -->
        <input
          class="etree-rename mono"
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
        <span class="etree-name mono">{r.display}</span>
      {/if}
      {#if rowCode(it)}
        {@const code = rowCode(it)}
        <span
          class="etree-git mono etree-git--{gitClass(code)}"
          title={it.is_dir ? `Contains ${gitTitle(code).toLowerCase()} changes` : gitTitle(code)}
        >{code}</span>
      {/if}
    </button>
    {#if creating && creating.parentDir === it.path}
      {@render createRow(r.depth + 1)}
    {/if}
  {/each}
</div>

{#if contextMenu}
  <div
    class="etree-ctx-backdrop"
    onclick={closeContextMenu}
    onkeydown={(e) => { if (e.key === 'Escape') closeContextMenu(); }}
    role="presentation"
  ></div>
  <div class="etree-ctx" style="left: {contextMenu.x}px; top: {contextMenu.y}px" role="menu">
    <button class="etree-ctx-item" onclick={() => void ctxRevealInFinder(contextMenu!.item)} role="menuitem">
      Reveal in Finder
    </button>
    <button class="etree-ctx-item" onclick={() => void ctxCopyPath(contextMenu!.item)} role="menuitem">
      Copy path
    </button>
    <button class="etree-ctx-item" onclick={() => void startCreate(contextMenu!.item, false)} role="menuitem">
      New File…
    </button>
    <button class="etree-ctx-item" onclick={() => void startCreate(contextMenu!.item, true)} role="menuitem">
      New Folder…
    </button>
    <button class="etree-ctx-item" onclick={() => ctxRename(contextMenu!.item)} role="menuitem">
      Rename…
    </button>
    <button class="etree-ctx-item etree-ctx-item--danger" onclick={() => void ctxDelete(contextMenu!.item)} role="menuitem">
      Delete
    </button>
  </div>
{/if}

<style>
  /* Mockup 4i / README §2.7 editor tree. Rows 12.5px, line-height 2.0,
     radius 6, active file = bg-nav fill. Fresh `.etree-` markup. */
  .etree { height: 100%; overflow: auto; padding: 4px 6px; }
  .etree-state { padding: 8px 14px; font-size: 11.5px; color: var(--text-2); }
  .etree-error { color: var(--err); }
  .etree-row {
    display: flex; align-items: center; gap: 7px;
    width: 100%; padding: 0 8px;
    font-size: 12.5px; line-height: 2.0;
    color: var(--text-1);
    text-align: left; border-radius: 6px;
    background: transparent;
    transition: background 80ms ease;
  }
  .etree-row:hover { background: var(--bg-hover); color: var(--text-0); }
  .etree-row.selected { background: var(--bg-nav); color: var(--text-0); }
  .etree-row.dir { color: var(--text-0); }
  /* Folder the inline create input is nested under — highlight so it's
     unmistakable WHERE the new file/folder lands (VSCode-style). */
  .etree-row.create-target { background: color-mix(in srgb, var(--accent) 12%, transparent); }
  .etree-creating { background: color-mix(in srgb, var(--accent) 8%, transparent); }
  /* Gitignored files/dirs — dimmed so they read as "outside git" at a
     glance. `.selected` still wins so opening an ignored file highlights. */
  .etree-row.ignored { color: var(--text-mute); opacity: 0.65; }
  .etree-row.ignored:hover { color: var(--text-2); opacity: 0.85; }
  .etree-row.ignored.dir { color: var(--text-mute); }
  .etree-chevron {
    display: inline-flex; width: 14px; height: 14px;
    align-items: center; justify-content: center; flex-shrink: 0;
    color: var(--text-2);
  }
  .etree-chevron :global(svg) {
    width: 11px; height: 11px;
    transition: transform var(--dur-base) var(--ease-spring);
  }
  .etree-chevron-pad { width: 11px; height: 11px; }
  /* Mockup 4i / 4j tree is icon-less: chevron for dirs, text + a
     trailing git badge. No per-file type glyphs. */
  .etree-name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; flex: 1; }
  /* Git status — bare mono letter, right-aligned, warn/ok tinted. */
  .etree-git {
    font-size: 10px; font-weight: 600;
    margin-left: 6px;
    flex-shrink: 0;
    min-width: 12px; text-align: right;
  }
  .etree-git--mod { color: var(--warn); }
  .etree-git--add { color: var(--ok); }
  .etree-git--del { color: var(--err); }
  .etree-git--new { color: var(--ok); }
  .etree-git--ren { color: var(--text-mute); }
  .etree-git--conflict { color: var(--err); }

  /* Inline rename input — sized to fit the row, same font so the swap
     doesn't shift the row height. */
  .etree-rename {
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
  .etree-ctx-backdrop {
    position: fixed; inset: 0; z-index: 600;
    background: transparent;
  }
  .etree-ctx {
    position: fixed; z-index: 601;
    min-width: 180px;
    padding: 4px;
    background: var(--bg-3);
    border: 1px solid var(--border-neutral-hi);
    border-radius: 8px;
    box-shadow: var(--shadow-3);
  }
  .etree-ctx-item {
    display: block; width: 100%;
    padding: 6px 10px; border-radius: 5px;
    background: none; border: none; text-align: left;
    color: var(--text-1); font-size: 12px; cursor: pointer;
  }
  .etree-ctx-item:hover { background: var(--bg-2); color: var(--text-0); }
  .etree-ctx-item--danger { color: var(--err); }
  .etree-ctx-item--danger:hover { background: color-mix(in srgb, var(--err) 12%, transparent); }
</style>
