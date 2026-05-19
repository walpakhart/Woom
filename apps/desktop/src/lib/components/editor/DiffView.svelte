<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { diffLines, diffWordsWithSpace, type Change } from 'diff';
  import { languageFor } from '$lib/components/editor/codemirrorLang';
  import { tokenizeByLine, buildSegments, type LineTokens, type Segment } from '$lib/components/editor/diffHighlight';

  interface Props {
    repo: string;
    path: string;
    /** true = staged-vs-HEAD, false = worktree-vs-index */
    staged: boolean;
    /** Jump from the diff to the real file. `line` is a 1-based line
     *  number on the b-side (worktree) where the first hunk starts. */
    onOpenFile?: (line: number) => void;
  }
  let { repo, path, staged, onOpenFile }: Props = $props();

  /* ────────────────────────────────────────────────────────────────
   * Why a hand-rolled grid renderer instead of CodeMirror Merge:
   *
   * `@codemirror/merge` doesn't pad the shorter side of a chunk —
   * line N on the left can sit next to line N+9 on the right after
   * a 9-line addition. Block-widget spacers patched on top of its
   * `collapseUnchanged` placeholders collide and produce huge phantom
   * gaps. Two independent .cm-scroller's also need a JS scroll-mirror,
   * which jitters under fast wheel input.
   *
   * Here every diff row is ONE HTML grid row containing both sides
   * (num | code | num | code). Alignment is structural — there's
   * literally no way for the two sides to drift. The whole renderer
   * lives in a single `.dv-scroll`, so vertical and horizontal scroll
   * are unified for free. No observers, no dispatchers, no async DOM
   * dance — just `rows` derived from `diffLines`. ─────────────────── */

  type Part = { text: string; hl?: 'add' | 'rem' };
  type Row =
    | { kind: 'equal'; ln: number; rn: number; l: string; r: string }
    | { kind: 'change'; ln: number; rn: number; l: string; r: string; lParts: Part[]; rParts: Part[] }
    | { kind: 'del'; ln: number; l: string }
    | { kind: 'add'; rn: number; r: string }
    | { kind: 'collapsed'; lFrom: number; lTo: number; rFrom: number; rTo: number; lines: string[]; rLines: string[] };

  let loading = $state(false);
  let error = $state<string | null>(null);
  let rows = $state<Row[]>([]);
  let stats = $state<{ add: number; del: number }>({ add: 0, del: 0 });
  /* Per-line syntax tokens for each side. Computed once after fetch
   * via `tokenizeByLine` — Lezer parse, classHighlighter pass. Empty
   * maps mean "no language detected" → plain text fallback. */
  let aTokens = $state<LineTokens>(new Map());
  let bTokens = $state<LineTokens>(new Map());
  /** 1-based first-changed b-side line. Defaults to 1 when there are
   *  no hunks (empty diff / brand-new file). */
  let firstChangedLine = $state(1);

  /* Full-screen overlay — viewport-cover so split panels can breathe. */
  let fullscreen = $state(false);
  function toggleFullscreen() { fullscreen = !fullscreen; }

  /* Split vs Unified view. Unified is the default — split-diff in a
   * narrow side panel halves the code column twice over and forces
   * horizontal scroll on virtually every real line. Unified gives the
   * full panel width to the code, with a single line marker (` `/`-`/`+`)
   * doing what split's empty placeholder column did. User choice is
   * persisted in localStorage so the toggle sticks across reloads. */
  type ViewMode = 'split' | 'unified';
  const VIEW_MODE_KEY = 'woom.diffview.mode';
  function readViewMode(): ViewMode {
    if (typeof localStorage === 'undefined') return 'unified';
    const v = localStorage.getItem(VIEW_MODE_KEY);
    return v === 'split' ? 'split' : 'unified';
  }
  let viewMode = $state<ViewMode>(readViewMode());
  function toggleViewMode() {
    viewMode = viewMode === 'split' ? 'unified' : 'split';
    try { localStorage.setItem(VIEW_MODE_KEY, viewMode); } catch { /* private mode */ }
  }

  /* ── Unified row stream ──────────────────────────────────────────────
   * Flatten the side-by-side `rows[]` into a single-column sequence:
   *   equal   → 1 row " " + (ln, rn, line)
   *   del     → 1 row "-" + (ln, —, line)
   *   add     → 1 row "+" + (—, rn, line)
   *   change  → 2 rows: "-" lParts on left ln, "+" rParts on right rn
   *   collapsed → 1 row spanning the whole grid
   * Indexed by the source `rows[]` position via `srcIdx` so the
   * collapsed-expand click still works against the original array. */
  type URow =
    | { kind: 'eq-u'; ln: number; rn: number; line: string }
    | { kind: 'del-u'; ln: number; line: string; parts: Part[] | null }
    | { kind: 'add-u'; rn: number; line: string; parts: Part[] | null }
    | { kind: 'col-u'; srcIdx: number; lFrom: number; lTo: number; rFrom: number; rTo: number; count: number };

  let unifiedRows = $derived.by<URow[]>(() => {
    const out: URow[] = [];
    for (let i = 0; i < rows.length; i++) {
      const r = rows[i];
      if (r.kind === 'equal') {
        out.push({ kind: 'eq-u', ln: r.ln, rn: r.rn, line: r.l });
      } else if (r.kind === 'del') {
        out.push({ kind: 'del-u', ln: r.ln, line: r.l, parts: null });
      } else if (r.kind === 'add') {
        out.push({ kind: 'add-u', rn: r.rn, line: r.r, parts: null });
      } else if (r.kind === 'change') {
        out.push({ kind: 'del-u', ln: r.ln, line: r.l, parts: r.lParts });
        out.push({ kind: 'add-u', rn: r.rn, line: r.r, parts: r.rParts });
      } else if (r.kind === 'collapsed') {
        out.push({
          kind: 'col-u', srcIdx: i,
          lFrom: r.lFrom, lTo: r.lTo,
          rFrom: r.rFrom, rTo: r.rTo,
          count: r.lines.length
        });
      }
    }
    return out;
  });
  function onKey(e: KeyboardEvent) {
    if (e.key === 'Escape' && fullscreen) {
      e.preventDefault();
      fullscreen = false;
    }
  }
  $effect(() => {
    if (fullscreen) {
      window.addEventListener('keydown', onKey);
      return () => window.removeEventListener('keydown', onKey);
    }
  });

  /** `diffLines` parts come with `\n` baked into `.value`. Strip the
   *  trailing newline before splitting so a `"a\nb\n"` part yields
   *  `['a','b']`, not `['a','b','']` — the empty tail would render as
   *  a phantom blank row. Empty parts collapse to `[]` instead of `[
   *  ''  ]`. */
  function splitLines(s: string): string[] {
    if (s === '') return [];
    const tail = s.endsWith('\n') ? s.slice(0, -1) : s;
    return tail.split('\n');
  }

  /** Walk a paired (removed, added) part with word-level diff and
   *  split it into per-line left/right segments. Tokens within a
   *  changed line keep their granular highlighting (the GitHub
   *  "brighter background on changed chars" effect). */
  function pairChangedLines(oldLines: string[], newLines: string[]): Row[] {
    const max = Math.max(oldLines.length, newLines.length);
    const out: Row[] = [];
    for (let k = 0; k < max; k++) {
      const ll = oldLines[k];
      const rr = newLines[k];
      out.push({
        kind: (ll != null && rr != null) ? 'change' : (ll != null ? 'del' : 'add'),
        // patched in by caller
      } as unknown as Row);
      const row = out[out.length - 1];
      if (row.kind === 'change') {
        const wd = diffWordsWithSpace(ll!, rr!);
        const lParts: Part[] = [];
        const rParts: Part[] = [];
        for (const w of wd) {
          if (w.added) rParts.push({ text: w.value, hl: 'add' });
          else if (w.removed) lParts.push({ text: w.value, hl: 'rem' });
          else { lParts.push({ text: w.value }); rParts.push({ text: w.value }); }
        }
        const cr = row as Extract<Row, { kind: 'change' }>;
        cr.l = ll!;
        cr.r = rr!;
        cr.lParts = lParts;
        cr.rParts = rParts;
      } else if (row.kind === 'del') {
        (row as Extract<Row, { kind: 'del' }>).l = ll!;
      } else if (row.kind === 'add') {
        (row as Extract<Row, { kind: 'add' }>).r = rr!;
      }
    }
    return out;
  }

  /** Build the row stream from raw a/b file contents. Per-line index
   *  is assigned by walking the diff parts in order. Collapses long
   *  unchanged stretches: rows with > CONTEXT*2 unchanged lines
   *  become a single `collapsed` row with the middle hidden, click-
   *  to-expand. */
  function buildRows(a: string, b: string): { rows: Row[]; add: number; del: number; firstChanged: number } {
    const parts: Change[] = diffLines(a, b);
    const rows: Row[] = [];
    let la = 1, lb = 1;
    let addCount = 0, delCount = 0;
    let firstChanged = -1;
    const CONTEXT = 3;

    for (let i = 0; i < parts.length; i++) {
      const p = parts[i];
      if (!p.added && !p.removed) {
        const lines = splitLines(p.value);
        if (lines.length === 0) continue;
        const isFirst = rows.length === 0;
        const isLast = i === parts.length - 1;
        /* Collapse middle if the run is long enough that hiding still
         * leaves CONTEXT lines on the near side(s). At edges we only
         * need one side of context, so the threshold is asymmetric. */
        const headKeep = isFirst ? 0 : CONTEXT;
        const tailKeep = isLast ? 0 : CONTEXT;
        if (lines.length > headKeep + tailKeep + 2) {
          /* keep head context */
          for (let k = 0; k < headKeep; k++) {
            const line = lines[k];
            rows.push({ kind: 'equal', ln: la, rn: lb, l: line, r: line });
            la++; lb++;
          }
          const hiddenStart = headKeep;
          const hiddenEnd = lines.length - tailKeep;
          const hiddenCount = hiddenEnd - hiddenStart;
          const hiddenLines = lines.slice(hiddenStart, hiddenEnd);
          rows.push({
            kind: 'collapsed',
            lFrom: la, lTo: la + hiddenCount - 1,
            rFrom: lb, rTo: lb + hiddenCount - 1,
            lines: hiddenLines, rLines: hiddenLines
          });
          la += hiddenCount; lb += hiddenCount;
          for (let k = 0; k < tailKeep; k++) {
            const line = lines[lines.length - tailKeep + k];
            rows.push({ kind: 'equal', ln: la, rn: lb, l: line, r: line });
            la++; lb++;
          }
        } else {
          for (const line of lines) {
            rows.push({ kind: 'equal', ln: la, rn: lb, l: line, r: line });
            la++; lb++;
          }
        }
      } else if (p.removed) {
        const next = parts[i + 1];
        if (next && next.added) {
          if (firstChanged < 0) firstChanged = lb;
          const oldLines = splitLines(p.value);
          const newLines = splitLines(next.value);
          delCount += oldLines.length;
          addCount += newLines.length;
          const paired = pairChangedLines(oldLines, newLines);
          for (const row of paired) {
            if (row.kind === 'change') { row.ln = la++; row.rn = lb++; }
            else if (row.kind === 'del') { row.ln = la++; }
            else if (row.kind === 'add') { row.rn = lb++; }
            rows.push(row);
          }
          i++; /* skip the paired addition */
        } else {
          if (firstChanged < 0) firstChanged = lb;
          const oldLines = splitLines(p.value);
          delCount += oldLines.length;
          for (const line of oldLines) {
            rows.push({ kind: 'del', ln: la++, l: line });
          }
        }
      } else if (p.added) {
        if (firstChanged < 0) firstChanged = lb;
        const newLines = splitLines(p.value);
        addCount += newLines.length;
        for (const line of newLines) {
          rows.push({ kind: 'add', rn: lb++, r: line });
        }
      }
    }
    return { rows, add: addCount, del: delCount, firstChanged: Math.max(1, firstChanged < 0 ? 1 : firstChanged) };
  }

  async function load() {
    if (!repo || !path) return;
    loading = true;
    error = null;
    try {
      const [aRev, bRev] = staged ? ['HEAD', ':'] : [':', ''];
      const [a, b] = await Promise.all([
        invoke<string>('git_show', { repo, revision: aRev, path }),
        invoke<string>('git_show', { repo, revision: bRev, path })
      ]);
      const built = buildRows(a, b);
      rows = built.rows;
      stats = { add: built.add, del: built.del };
      firstChangedLine = built.firstChanged;
      /* Tokenize both sides AFTER rows are built so the heavier parse
       * pass doesn't block the first paint of the diff. The render
       * gracefully degrades to plain text until tokens land. */
      const lang = languageFor(path);
      aTokens = tokenizeByLine(a, lang);
      bTokens = tokenizeByLine(b, lang);
    } catch (e: unknown) {
      error = e instanceof Error ? e.message : String(e);
      rows = [];
      stats = { add: 0, del: 0 };
      aTokens = new Map();
      bTokens = new Map();
    } finally {
      loading = false;
    }
  }

  $effect(() => { void load(); repo; path; staged; });

  /** Click-to-expand a collapsed run — inline the hidden equal lines
   *  in place. We replace the placeholder row with N real equal rows
   *  so the user can read the context. No animation: the size jump
   *  is intentional, mirrors GitHub's "show N more" behavior. */
  function expandCollapsed(idx: number) {
    const r = rows[idx];
    if (r.kind !== 'collapsed') return;
    const replacement: Row[] = [];
    let ln = r.lFrom;
    let rn = r.rFrom;
    for (let i = 0; i < r.lines.length; i++) {
      replacement.push({ kind: 'equal', ln: ln++, rn: rn++, l: r.lines[i], r: r.rLines[i] });
    }
    rows = [...rows.slice(0, idx), ...replacement, ...rows.slice(idx + 1)];
  }
</script>

{#snippet code(segments: Segment[])}
  {#each segments as seg, i (i)}
    {#if seg.cls && seg.hl}
      <span class="{seg.cls} dv-tok-{seg.hl === 'add' ? 'add' : 'rem'}">{seg.text}</span>
    {:else if seg.cls}
      <span class={seg.cls}>{seg.text}</span>
    {:else if seg.hl}
      <span class="dv-tok-{seg.hl === 'add' ? 'add' : 'rem'}">{seg.text}</span>
    {:else}
      {seg.text}
    {/if}
  {/each}
{/snippet}

<div class="dv" class:dv--full={fullscreen}>
  <div class="dv-head">
    <span class="dv-path mono">{path}</span>
    <span class="dv-side">{staged ? 'HEAD → Staged' : 'Index → Working tree'}</span>
    <span class="dv-stats mono">
      <span class="dv-add">+{stats.add}</span>
      <span class="dv-del">−{stats.del}</span>
    </span>
    {#if onOpenFile}
      <button
        class="dv-openfile"
        onclick={() => onOpenFile?.(firstChangedLine)}
        title="Open file at first change"
        aria-label="Open file at first change"
      >
        <svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">
          <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/>
          <polyline points="14 2 14 8 20 8"/>
          <path d="M11 14h6M14 11l3 3-3 3"/>
        </svg>
        <span>open file</span>
      </button>
    {/if}
    <button
      class="dv-mode"
      onclick={toggleViewMode}
      title={viewMode === 'split' ? 'Switch to unified view' : 'Switch to split view'}
      aria-label={viewMode === 'split' ? 'Switch to unified view' : 'Switch to split view'}
    >
      {#if viewMode === 'split'}
        <svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">
          <rect x="3" y="4" width="8" height="16" rx="1"/>
          <rect x="13" y="4" width="8" height="16" rx="1"/>
        </svg>
        <span>split</span>
      {:else}
        <svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">
          <rect x="3" y="4" width="18" height="16" rx="1"/>
          <line x1="3" y1="10" x2="21" y2="10"/>
          <line x1="3" y1="15" x2="21" y2="15"/>
        </svg>
        <span>unified</span>
      {/if}
    </button>
    <button
      class="dv-fullscreen"
      onclick={toggleFullscreen}
      title={fullscreen ? 'Exit full screen (Esc)' : 'Open in full screen'}
      aria-label={fullscreen ? 'Exit full screen' : 'Open in full screen'}
    >
      {#if fullscreen}
        <svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">
          <path d="M9 4v5H4M15 4v5h5M9 20v-5H4M15 20v-5h5"/>
        </svg>
      {:else}
        <svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">
          <path d="M4 9V4h5M20 9V4h-5M4 15v5h5M20 15v5h-5"/>
        </svg>
      {/if}
    </button>
  </div>
  {#if loading}
    <div class="dv-state">Loading diff…</div>
  {:else if error}
    <div class="dv-state dv-err">{error}</div>
  {:else}
    <div class="dv-scroll">
      {#if viewMode === 'unified'}
        <div class="dv-grid dv-grid-u">
          {#each unifiedRows as u, uidx (uidx)}
            {#if u.kind === 'col-u'}
              <button
                type="button"
                class="dv-collapsed"
                onclick={() => expandCollapsed(u.srcIdx)}
                title="Expand {u.count} hidden line{u.count === 1 ? '' : 's'}"
              >
                <svg viewBox="0 0 24 24" width="11" height="11" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
                  <polyline points="6 9 12 15 18 9"/>
                </svg>
                <span>{u.count} unchanged line{u.count === 1 ? '' : 's'}</span>
                <span class="dv-collapsed-range">{u.lFrom}…{u.lTo} ↔ {u.rFrom}…{u.rTo}</span>
              </button>
            {:else if u.kind === 'eq-u'}
              <div class="dv-num">{u.ln}</div>
              <div class="dv-num">{u.rn}</div>
              <div class="dv-mark"> </div>
              <div class="dv-code">
                {#if u.line}{@render code(buildSegments(u.line, aTokens.get(u.ln), null))}{:else}{'\u200b'}{/if}
              </div>
            {:else if u.kind === 'del-u'}
              <div class="dv-num dv-num-del">{u.ln}</div>
              <div class="dv-num dv-num-empty"></div>
              <div class="dv-mark dv-mark-del">−</div>
              <div class="dv-code dv-code-del">
                {#if u.line}{@render code(buildSegments(u.line, aTokens.get(u.ln), u.parts))}{:else}{'\u200b'}{/if}
              </div>
            {:else if u.kind === 'add-u'}
              <div class="dv-num dv-num-empty"></div>
              <div class="dv-num dv-num-add">{u.rn}</div>
              <div class="dv-mark dv-mark-add">+</div>
              <div class="dv-code dv-code-add">
                {#if u.line}{@render code(buildSegments(u.line, bTokens.get(u.rn), u.parts))}{:else}{'\u200b'}{/if}
              </div>
            {/if}
          {/each}
        </div>
      {:else}
      <div class="dv-grid">
        {#each rows as r, idx (idx)}
          {#if r.kind === 'collapsed'}
            <button
              type="button"
              class="dv-collapsed"
              onclick={() => expandCollapsed(idx)}
              title="Expand {r.lines.length} hidden line{r.lines.length === 1 ? '' : 's'}"
            >
              <svg viewBox="0 0 24 24" width="11" height="11" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
                <polyline points="6 9 12 15 18 9"/>
              </svg>
              <span>{r.lines.length} unchanged line{r.lines.length === 1 ? '' : 's'}</span>
              <span class="dv-collapsed-range">{r.lFrom}…{r.lTo} ↔ {r.rFrom}…{r.rTo}</span>
            </button>
          {:else if r.kind === 'equal'}
            <div class="dv-num">{r.ln}</div>
            <div class="dv-code">
              {#if r.l}{@render code(buildSegments(r.l, aTokens.get(r.ln), null))}{:else}{'\u200b'}{/if}
            </div>
            <div class="dv-num">{r.rn}</div>
            <div class="dv-code">
              {#if r.r}{@render code(buildSegments(r.r, bTokens.get(r.rn), null))}{:else}{'\u200b'}{/if}
            </div>
          {:else if r.kind === 'change'}
            <div class="dv-num dv-num-del">{r.ln}</div>
            <div class="dv-code dv-code-del">
              {@render code(buildSegments(r.l, aTokens.get(r.ln), r.lParts))}
            </div>
            <div class="dv-num dv-num-add">{r.rn}</div>
            <div class="dv-code dv-code-add">
              {@render code(buildSegments(r.r, bTokens.get(r.rn), r.rParts))}
            </div>
          {:else if r.kind === 'del'}
            <div class="dv-num dv-num-del">{r.ln}</div>
            <div class="dv-code dv-code-del">
              {#if r.l}{@render code(buildSegments(r.l, aTokens.get(r.ln), null))}{:else}{'\u200b'}{/if}
            </div>
            <div class="dv-num dv-num-empty"></div>
            <div class="dv-code dv-code-empty"></div>
          {:else if r.kind === 'add'}
            <div class="dv-num dv-num-empty"></div>
            <div class="dv-code dv-code-empty"></div>
            <div class="dv-num dv-num-add">{r.rn}</div>
            <div class="dv-code dv-code-add">
              {#if r.r}{@render code(buildSegments(r.r, bTokens.get(r.rn), null))}{:else}{'\u200b'}{/if}
            </div>
          {/if}
        {/each}
      </div>
      {/if}
    </div>
  {/if}
</div>

<style>
  .dv { display: flex; flex-direction: column; height: 100%; min-height: 0; background: var(--bg-0); }
  .dv.dv--full {
    position: fixed;
    inset: 0;
    z-index: 1000;
    background: var(--bg-0);
    box-shadow: 0 0 0 1px var(--border-neutral), 0 20px 60px rgba(0, 0, 0, 0.5);
  }
  .dv-head {
    display: flex; align-items: center; gap: 12px;
    padding: 6px 14px;
    border-bottom: 1px solid var(--border-neutral);
    background: var(--bg-1);
    font-size: 12px;
    flex-shrink: 0;
  }
  .dv-path { color: var(--text-0); font-weight: 500; flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .dv-side { color: var(--text-2); font-size: 10.5px; padding: 2px 7px; border-radius: 3px; background: var(--bg-3); }
  .dv-stats { color: var(--text-1); font-size: 11px; display: inline-flex; gap: 8px; }
  .dv-add { color: var(--success); }
  .dv-del { color: var(--error); }
  .dv-openfile {
    background: transparent; border: 1px solid var(--border-neutral); border-radius: 5px;
    padding: 3px 8px 3px 6px; color: var(--text-1); cursor: pointer;
    display: inline-flex; align-items: center; gap: 5px; font-size: 11px; font-family: inherit;
    transition: background 100ms ease, color 100ms ease, border-color 100ms ease;
  }
  .dv-openfile:hover { background: var(--bg-2); color: var(--accent-bright); border-color: var(--accent); }
  .dv-openfile svg { flex-shrink: 0; }
  .dv-fullscreen {
    background: transparent; border: 1px solid var(--border-neutral); border-radius: 5px;
    padding: 3px 6px; color: var(--text-1); cursor: pointer;
    display: inline-flex; align-items: center; justify-content: center;
    transition: background 100ms ease, color 100ms ease, border-color 100ms ease;
  }
  .dv-fullscreen:hover { background: var(--bg-2); color: var(--text-0); border-color: var(--border-neutral-hi); }
  .dv-mode {
    background: transparent; border: 1px solid var(--border-neutral); border-radius: 5px;
    padding: 3px 8px 3px 6px; color: var(--text-1); cursor: pointer;
    display: inline-flex; align-items: center; gap: 5px; font-size: 11px; font-family: inherit;
    transition: background 100ms ease, color 100ms ease, border-color 100ms ease;
  }
  .dv-mode:hover { background: var(--bg-2); color: var(--text-0); border-color: var(--border-neutral-hi); }
  .dv-mode svg { flex-shrink: 0; }

  .dv-state { padding: 12px 14px; color: var(--text-2); font-size: 12.5px; border-bottom: 1px solid var(--border-neutral); }
  .dv-err { color: var(--error); }

  /* Single scroller for both columns. Vertical wheel — both sides
   * move together (one DOM). Horizontal scrollbar shared, lining up
   * long lines on either side without per-side state. */
  .dv-scroll {
    flex: 1; min-height: 0;
    overflow: auto;
    font-family: 'JetBrains Mono', ui-monospace, 'SF Mono', monospace;
    font-size: 12.5px;
    line-height: 1.55;
    color: var(--text-0);
    /* tabular numbers keep gutter widths stable across rows */
    font-variant-numeric: tabular-nums;
  }
  .dv-grid {
    /* `auto` columns hug the widest line-number; `minmax(0, 1fr)` lets
     * code cells shrink below their natural width so `min-width:
     * max-content` on the grid is what actually drives horizontal
     * overflow into the parent scroller. */
    display: grid;
    grid-template-columns: auto minmax(0, 1fr) auto minmax(0, 1fr);
    min-width: max-content;
  }
  /* Unified: [ln-a][ln-b][marker][code]. Single code column gets the
   * whole panel width — no half-and-half waste, no horizontal scroll
   * on normal-length lines. */
  .dv-grid-u {
    grid-template-columns: auto auto auto minmax(0, 1fr);
  }
  .dv-mark {
    padding: 0 6px;
    text-align: center;
    user-select: none;
    color: var(--text-2);
    font-size: 11px;
    background: var(--bg-1);
    border-right: 1px solid var(--border-neutral);
    min-width: 1.5ch;
    white-space: pre;
  }
  .dv-mark-del { background: var(--diff-rem-line); color: var(--error); }
  .dv-mark-add { background: var(--diff-add-line); color: var(--success); }
  .dv-num {
    padding: 0 10px;
    text-align: right;
    user-select: none;
    color: var(--text-2);
    font-size: 11px;
    background: var(--bg-1);
    border-right: 1px solid var(--border-neutral);
    min-width: 3.5ch;
    white-space: pre;
  }
  .dv-code {
    padding: 0 12px;
    white-space: pre;
    tab-size: 4;
  }
  /* GitHub-style washes: line-wide tint at 15% alpha, token highlight
   * at 40% on top of that line tint — single hue, only opacity changes. */
  .dv-code-del { background: var(--diff-rem-line); }
  .dv-code-add { background: var(--diff-add-line); }
  .dv-num-del { background: color-mix(in srgb, var(--diff-rem-token) 60%, var(--bg-1)); color: var(--text-1); }
  .dv-num-add { background: color-mix(in srgb, var(--diff-add-token) 60%, var(--bg-1)); color: var(--text-1); }
  .dv-tok-rem, .dv-code :global(.dv-tok-rem) { background: var(--diff-rem-token); }
  .dv-tok-add, .dv-code :global(.dv-tok-add) { background: var(--diff-add-token); }

  /* Stable `tok-*` class names emitted by `@lezer/highlight`'s
   * `classHighlighter`. Dynamic — set at render time from string
   * values — so Svelte's scoper can't see them at compile time;
   * `:global()` is required. Palette is a muted GitHub-ish set that
   * reads cleanly on top of the diff line washes (which double as
   * background for change rows). Color only, no font-weight tricks,
   * so character widths stay identical to plain text and grid
   * alignment doesn't shift. */
  .dv-code :global(.tok-keyword),
  .dv-code :global(.tok-controlKeyword),
  .dv-code :global(.tok-modifier),
  .dv-code :global(.tok-operatorKeyword) { color: var(--diff-tok-keyword); }
  .dv-code :global(.tok-string),
  .dv-code :global(.tok-string2),
  .dv-code :global(.tok-regexp),
  .dv-code :global(.tok-escape),
  .dv-code :global(.tok-character) { color: var(--diff-tok-string); }
  .dv-code :global(.tok-comment),
  .dv-code :global(.tok-meta),
  .dv-code :global(.tok-lineComment),
  .dv-code :global(.tok-blockComment) { color: var(--diff-tok-comment); font-style: italic; }
  .dv-code :global(.tok-number),
  .dv-code :global(.tok-bool),
  .dv-code :global(.tok-atom) { color: var(--diff-tok-number); }
  .dv-code :global(.tok-typeName),
  .dv-code :global(.tok-className),
  .dv-code :global(.tok-namespace) { color: var(--diff-tok-type); }
  .dv-code :global(.tok-function),
  .dv-code :global(.tok-macroName),
  .dv-code :global(.tok-labelName) { color: var(--diff-tok-function); }
  .dv-code :global(.tok-propertyName),
  .dv-code :global(.tok-attributeName) { color: var(--diff-tok-property); }
  .dv-code :global(.tok-variableName),
  .dv-code :global(.tok-name) { color: var(--diff-tok-name); }
  .dv-code :global(.tok-tagName) { color: var(--diff-tok-tag); }
  .dv-code :global(.tok-attributeValue) { color: var(--diff-tok-string); }
  .dv-code :global(.tok-operator),
  .dv-code :global(.tok-derefOperator),
  .dv-code :global(.tok-punctuation),
  .dv-code :global(.tok-bracket),
  .dv-code :global(.tok-paren),
  .dv-code :global(.tok-brace),
  .dv-code :global(.tok-squareBracket),
  .dv-code :global(.tok-separator) { color: var(--diff-tok-punct); }
  .dv-code :global(.tok-invalid) { color: var(--error); }
  .dv-code :global(.tok-heading),
  .dv-code :global(.tok-strong) { color: var(--diff-tok-keyword); font-weight: 600; }
  .dv-code :global(.tok-emphasis) { font-style: italic; }
  .dv-code :global(.tok-link),
  .dv-code :global(.tok-url) { color: var(--diff-tok-string); text-decoration: underline; }

  /* Empty half on a single-sided del/add row. Slight tonal wash so
   * the eye reads it as "absent on this side" rather than "blank
   * code". No diagonal stripes — that was the wrong call last
   * iteration; a flat muted tone matches GitHub. */
  .dv-num-empty, .dv-code-empty { background: var(--bg-2); }
  .dv-num-empty { border-right: 1px solid var(--border-neutral); }

  /* Collapsed unchanged region. Spans all 4 grid columns; click to
   * expand the hidden lines inline. */
  .dv-collapsed {
    grid-column: 1 / -1;
    display: flex; align-items: center; gap: 8px;
    padding: 6px 12px;
    background: var(--bg-2);
    border-top: 1px solid var(--border-neutral);
    border-bottom: 1px solid var(--border-neutral);
    color: var(--text-2);
    font-size: 11px;
    font-family: inherit;
    cursor: pointer;
    transition: background 100ms ease, color 100ms ease;
    text-align: left;
  }
  .dv-collapsed:hover { background: var(--bg-3); color: var(--accent-bright); }
  .dv-collapsed-range { margin-left: auto; opacity: 0.7; font-variant-numeric: tabular-nums; }

  .mono { font-family: 'JetBrains Mono', ui-monospace, 'SF Mono', monospace; }
</style>
