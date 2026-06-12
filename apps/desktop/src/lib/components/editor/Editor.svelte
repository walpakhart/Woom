<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import { EditorView, basicSetup } from 'codemirror';
  import { EditorState, Compartment, Prec } from '@codemirror/state';
  import { keymap } from '@codemirror/view';
  import { invoke } from '@tauri-apps/api/core';
  import { languageFor } from '$lib/components/editor/codemirrorLang';
  import { editorThemeExtension } from '$lib/components/editor/editorTheme';
  import { themeState } from '$lib/state/theme.svelte';
  import { editorPrefs } from '$lib/state/editorPrefs.svelte';
  import { recordCursor, readCursor } from '$lib/state/editorCursors.svelte';
  import {
    changeBarExtension,
    setChangeBar,
    parseUnifiedDiff,
    type LineChanges,
    type LineChangeKind,
    type ChangeHunk
  } from '$lib/components/editor/changeBar';
  import { diffWordsWithSpace } from 'diff';
  import { overlayScrollbars } from '$lib/actions/overlayScrollbars';
  import {
    inlineHunksExtension,
    setHunks,
    setFocusedHunks,
    computeHunks,
    hunkAtLine,
    hunkNewRange,
    buildHunkRevert,
    type Hunk
  } from '$lib/components/editor/inlineHunks';
  import { updateEditEvent } from '$lib/state/sessions.svelte';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';

  interface Props {
    path: string;
    /** Editor solo instance id this buffer belongs to. Used to filter
     *  cross-component navigation events (`woom:editor:goto`) so the
     *  symbol picker can target the right buffer when the user has
     *  multiple editor instances open at once. Defaults to
     *  `'default'` to match EditorView's prop default. */
    instanceId?: string;
    onDirty?: (dirty: boolean) => void;
    onSaved?: (path: string) => void;
    /** Fires whenever the user's selection or the editor geometry
     *  changes (so scrolling moves the popover with the selection
     *  rectangle). Reports:
     *    - `startLine`, `endLine` — 1-based inclusive line range.
     *    - `anchor` — viewport-relative coordinates of the END of the
     *       last selected line, used by EditorView to position the
     *       floating "Apply to <agent>" popover. `null` means the
     *       end of the selection is currently scrolled out of view —
     *       the popover hides until it's visible again, but the
     *       selection itself isn't lost so re-scrolling brings it
     *       back without the user re-selecting.
     *  Whole result is `null` only when the selection collapses to a
     *  caret. */
    onSelectionChange?: (
      sel:
        | {
            startLine: number;
            endLine: number;
            anchor: { x: number; y: number } | null;
          }
        | null
    ) => void;
    /** Fires on every cursor move (including collapsed carets) — drives
     *  the status bar's "Ln, Col" readout + line-endings indicator.
     *  Cheap to compute (CodeMirror exposes `lineAt(pos)`); no debounce
     *  needed because the bar only re-renders when these numbers
     *  actually change. */
    onCursorChange?: (
      info: {
        line: number;
        col: number;
        lineEndings: 'lf' | 'crlf';
        bytes: number;
      } | null
    ) => void;
    /** Toggle CodeMirror's `EditorView.lineWrapping`. Defaults to off
     *  (matches every IDE I know — wrapping interferes with reading
     *  long log lines / tables). The status bar exposes a one-click
     *  toggle so the user can flip per-buffer when a Markdown / poem
     *  benefits from wrapping. */
    wordWrap?: boolean;
    /** Fires when the user changes the editor's text — exposes the
     *  in-memory contents so the parent can mirror it (e.g. a
     *  Markdown live preview). Cheap to add: a single `u.state.doc`
     *  call. Skipped when not provided so most call sites pay
     *  nothing. */
    onTextChange?: (text: string) => void;
    /** Absolute path of the repo this file belongs to. Drives the
     *  left-gutter change bar (HEAD vs worktree). Empty / non-git
     *  → no gutter. */
    repoPath?: string;
    /** Pending agent edits for THIS file (already filtered to the active
     *  path by EditorView). Each carries the full-file `oldText`/`newText`;
     *  we diff them into hunks and render an inline overlay (adds
     *  highlighted, deletes ghosted). Empty / omitted → no overlay. */
    pendingEdits?: {
      sessionId: string;
      toolId: string;
      oldText: string;
      newText: string;
      /** True for full-file edits (Write) where oldText/newText are the
       *  whole document. False for Edit/MultiEdit snippets, which must be
       *  anchored at their real position in the buffer before rendering. */
      wholeFile?: boolean;
    }[];
    /** Fires once `load()` has read the file and built the view, with the
     *  loaded path. Lets the parent drive a post-load `goToLine` (e.g. the
     *  Review pane scrolling to an edit's first hunk) without racing the
     *  async load. */
    onLoaded?: (path: string) => void;
    /** The agent edit currently selected in the ReviewPane (`sessionId:toolId`).
     *  When set, the overlay scrolls to + emphasises that edit's hunks so the
     *  reviewer sees exactly which chunk the row points at. */
    selectedEditKey?: string | null;
  }
  let {
    path,
    instanceId = 'default',
    onDirty,
    onSaved,
    onSelectionChange,
    onCursorChange,
    wordWrap = false,
    onTextChange,
    repoPath = '',
    pendingEdits = [],
    onLoaded,
    selectedEditKey = null
  }: Props = $props();

  let editorEl: HTMLDivElement;
  let edEl: HTMLDivElement;
  let view: EditorView | null = null;
  let lastLoadedPath = $state('');
  let savedContents = $state('');
  let loading = $state(false);
  let error = $state<string | null>(null);
  let dirty = $state(false);
  /* Set when the linked agent edited the open file while the buffer had
     unsaved manual edits. We deliberately do NOT auto-reload in that case
     (the user's in-progress text wins — see spec open-q (c)); this flag
     lets a later phase surface a "reload / keep mine" affordance. */
  let agentEditPendingReload = $state(false);
  /* Bumped every time a fresh EditorView is created (initial load + each
     reload). The inline-hunk recompute $effect reads it so the overlay is
     re-dispatched after the view is torn down + rebuilt (e.g. P1 reload),
     not just when `pendingEdits` changes. */
  let viewVersion = $state(0);

  /* P3 hunk resolution. resolvedHunkIds persists accept/reject decisions
     across recomputes (hunk ids are stable for a given edit), so a resolved
     hunk stays gone even though computeHunks would re-derive it. The version
     counter drives the recompute $effect since the Set isn't reactive. The
     owner maps (rebuilt each $effect run) let a hunk reach its EditEvent. */
  const resolvedHunkIds = new Set<string>();
  let resolvedVersion = $state(0);
  let currentHunks = $state<Hunk[]>([]);
  let hunkOwners = new Map<string, { sessionId: string; toolId: string }>();
  let editHunkIds = new Map<string, string[]>();
  const perEditRejects = new Map<string, number>();
  /* The file `resolvedHunkIds` belongs to — cleared only on a genuine file
     switch, NOT on a same-file reload() (which blanks lastLoadedPath). */
  let resolvedForPath = '';

  /* Autosave: write dirty buffers to disk after a short idle window.
     600 ms feels right — long enough to avoid mid-token saves under
     normal typing, short enough that Cmd-Tab to the file tree sees
     up-to-date contents. We still expose Cmd-S so a deliberate
     "save NOW" stays instant. */
  const AUTOSAVE_MS = 600;
  let autosaveTimer: ReturnType<typeof setTimeout> | null = null;

  const languageCompartment = new Compartment();
  /* Theme lives in its own compartment so we can swap CodeMirror's
     editor theme without rebuilding the EditorState. Reactive
     $effect below dispatches a `reconfigure` whenever the user flips
     the app palette in Settings. */
  const themeCompartment = new Compartment();
  /* Word-wrap toggle compartment — `EditorView.lineWrapping` is a
     facet (a fixed extension), so we stash it behind a Compartment
     to flip it at runtime via `dispatch({effects: reconfigure(…)})`. */
  const wrapCompartment = new Compartment();

  async function load(p: string) {
    if (!p || p === lastLoadedPath) return;
    loading = true;
    error = null;
    gutterPopup = null;
    try {
      const contents = await invoke<string>('fs_read_file', { path: p });
      savedContents = contents;
      lastLoadedPath = p;
      dirty = false;
      onDirty?.(false);

      /* Genuine file switch (not a same-file reload) → drop prior hunk
         resolutions; they belong to the file we're leaving. */
      if (p !== resolvedForPath) {
        resolvedHunkIds.clear();
        perEditRejects.clear();
        resolvedForPath = p;
        resolvedVersion++;
      }

      /* Persist the previous file's cursor before swapping to the
       * new file's. Without this, the user's last position in
       * `oldPath` is lost when they switch tabs. */
      if (view && lastLoadedPath && lastLoadedPath !== p) {
        const sel = view.state.selection.main;
        recordCursor(lastLoadedPath, {
          from: sel.from,
          to: sel.to,
          scrollTop: view.scrollDOM.scrollTop
        });
        /* Flush a pending autosave to disk before we destroy the view.
           We bypass save() here because save() touches `view` after
           the await (refreshChangeBar dispatches), and the next line
           tears it down. Direct invoke + onSaved notification is
           enough to keep the GitPanel and dirty indicator in sync. */
        if (autosaveTimer) { clearTimeout(autosaveTimer); autosaveTimer = null; }
        if (dirty) {
          const pendingPath = lastLoadedPath;
          const pendingContents = view.state.doc.toString();
          try {
            await invoke('fs_write_file', { path: pendingPath, contents: pendingContents });
            onSaved?.(pendingPath);
          } catch {
            /* Swallow — surfacing a stale-buffer error in the new file
               would confuse the user. They'll see the dirty dot when
               they navigate back to pendingPath. */
          }
        }
      }
      view?.destroy();
      /* Restore the new file's saved selection (clamped to the
       * current doc length, which may have changed since last visit
       * if the file was edited externally). Returns null when there
       * is no saved record, in which case CodeMirror defaults to
       * caret at offset 0. */
      const stored = readCursor(p);
      const docLen = contents.length;
      const initialSel = stored
        ? {
            anchor: Math.min(Math.max(0, stored.from), docLen),
            head: Math.min(Math.max(0, stored.to), docLen)
          }
        : undefined;
      view = new EditorView({
        parent: editorEl,
        state: EditorState.create({
          doc: contents,
          selection: initialSel,
          extensions: [
            basicSetup,
            themeCompartment.of(editorThemeExtension(themeState.name)),
            languageCompartment.of(languageFor(p)),
            wrapCompartment.of(wordWrap ? EditorView.lineWrapping : []),
            changeBarExtension({ onMarkClick: onChangeMarkClick }),
            inlineHunksExtension(),
            // Scoped hunk resolution: Tab=accept / Esc=reject the hunk under
            // the caret. Prec.highest inspects them first, but each handler
            // returns false (falls through to indent / close-search) when
            // the caret isn't in a pending hunk.
            Prec.highest(
              keymap.of([
                { key: 'Tab', run: acceptFocusedHunk },
                { key: 'Escape', run: rejectFocusedHunk }
              ])
            ),
            keymap.of([
              { key: 'Mod-s', run: (v) => { void save(v); return true; } }
            ]),
            EditorView.updateListener.of((u) => {
              if (u.docChanged) {
                const cur = u.state.doc.toString();
                const d = cur !== savedContents;
                if (d !== dirty) {
                  dirty = d;
                  onDirty?.(d);
                }
                scheduleChangeBar();
                gutterPopup = null;
                if (d) scheduleAutosave();
                /* Stream the buffer text up so the Markdown live-
                   preview can re-render. Only fired when the parent
                   wired a callback — no cost otherwise. */
                onTextChange?.(cur);
              }
              // Selection-change OR geometry-change → recompute the
              // popover anchor so it tracks the end of the selection
              // rectangle even as the user scrolls inside CodeMirror.
              // We collapse caret-only selections to `null` so the
              // parent doesn't have to special-case "is this a real
              // range", and report `anchor: null` (rather than a fake
              // off-screen pos) when the end of the selection is
              // outside the visible viewport — the parent hides the
              // popover but keeps the selection state, so scrolling
              // back into view re-anchors without re-selecting.
              if (
                u.selectionSet ||
                u.docChanged ||
                u.geometryChanged ||
                u.viewportChanged
              ) {
                /* Persist the cursor on every selection change.
                 * `recordCursor` debounces the localStorage write
                 * itself, so we can fire on every dispatch without
                 * worrying about IO storms. */
                if (lastLoadedPath && u.view.scrollDOM) {
                  const sel = u.state.selection.main;
                  recordCursor(lastLoadedPath, {
                    from: sel.from,
                    to: sel.to,
                    scrollTop: u.view.scrollDOM.scrollTop
                  });
                }
                /* Fire the cursor-info callback on every dispatch.
                   The status bar uses this to render "Ln 11, Col 38";
                   Svelte's reactivity will skip re-render if the
                   numbers haven't changed, so the cost is just one
                   shallow object creation per dispatch. */
                if (onCursorChange) {
                  const sel = u.state.selection.main;
                  const lineInfo = u.state.doc.lineAt(sel.head);
                  const col = sel.head - lineInfo.from + 1;
                  /* Probe the document for the first \r\n vs \n run.
                     Cheap: scan up to the first 4KB for a newline. */
                  const head = u.state.doc.sliceString(0, Math.min(4096, u.state.doc.length));
                  const lineEndings = head.includes('\r\n') ? 'crlf' : 'lf';
                  onCursorChange({
                    line: lineInfo.number,
                    col,
                    lineEndings,
                    bytes: u.state.doc.length
                  });
                }
                if (onSelectionChange) {
                  const sel = u.state.selection.main;
                  if (sel.from === sel.to) {
                    onSelectionChange(null);
                  } else {
                    const startLine = u.state.doc.lineAt(sel.from).number;
                    // CodeMirror selections are exclusive at `to` — a
                    // line-end selection lands on the next line's
                    // first column, which would over-report by one.
                    // Snap back to the previous line in that case.
                    const rawEndLine = u.state.doc.lineAt(sel.to).number;
                    const endLine =
                      rawEndLine > startLine && sel.to === u.state.doc.line(rawEndLine).from
                        ? rawEndLine - 1
                        : rawEndLine;
                    // Anchor on the END of the last selected line so
                    // the popover sits flush with the right edge of
                    // the highlight rectangle on the bottom-most line,
                    // matching how Cursor / GitHub Copilot anchor
                    // their inline action bars.
                    const anchorPos = u.state.doc.line(endLine).to;
                    const coords = u.view.coordsAtPos(anchorPos);
                    onSelectionChange({
                      startLine,
                      endLine,
                      anchor: coords ? { x: coords.right, y: coords.bottom } : null
                    });
                  }
                }
              }
            })
          ]
        })
      });
      // Signal the inline-hunk recompute $effect that a fresh view exists.
      viewVersion++;
      /* Restore scroll position after CodeMirror has measured. The
       * raf-then-microtask dance avoids a flicker where the editor
       * mounts at scrollTop=0 then jumps; we delay the restore until
       * after the first paint when geometry is real. */
      if (stored && stored.scrollTop > 0) {
        const v = view;
        requestAnimationFrame(() => {
          if (v && v.scrollDOM) v.scrollDOM.scrollTop = stored.scrollTop;
        });
      }
      void refreshChangeBar();
      /* View + lastLoadedPath are live here — safe for the parent to fire
         a goToLine in response (e.g. Review-pane scroll-to-hunk). */
      onLoaded?.(p);
    } catch (e: unknown) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      loading = false;
    }
  }

  async function save(v: EditorView) {
    if (!lastLoadedPath) return;
    if (autosaveTimer) { clearTimeout(autosaveTimer); autosaveTimer = null; }
    const cur = v.state.doc.toString();
    try {
      await invoke('fs_write_file', { path: lastLoadedPath, contents: cur });
      savedContents = cur;
      dirty = false;
      onDirty?.(false);
      onSaved?.(lastLoadedPath);
      void refreshChangeBar();
    } catch (e: unknown) {
      error = e instanceof Error ? e.message : String(e);
    }
  }

  /* ── Custom vertical scrollbar ──────────────────────────────────────
     WKWebView ignores `::-webkit-scrollbar` styling whenever macOS runs
     overlay scrollbars ("Show scroll bars: When scrolling" — the
     trackpad default), so the native bar stays invisible until a scroll
     gesture no matter what alpha we give the thumb. Same road VS Code
     took: draw our own track + thumb and drive scrollTop directly. */
  let vbarTop = $state(0);
  let vbarH = $state(0);
  let vbarVisible = $state(false);
  let vbarDragging = $state(false);

  function refreshVbar() {
    const sd = view?.scrollDOM;
    if (!sd) {
      vbarVisible = false;
      return;
    }
    const { scrollTop, scrollHeight, clientHeight } = sd;
    if (scrollHeight <= clientHeight + 1) {
      vbarVisible = false;
      return;
    }
    const h = Math.max(24, (clientHeight / scrollHeight) * clientHeight);
    vbarH = h;
    vbarTop = (scrollTop / (scrollHeight - clientHeight)) * (clientHeight - h);
    vbarVisible = true;
  }

  function vbarThumbDown(e: PointerEvent) {
    const sd = view?.scrollDOM;
    if (!sd) return;
    e.preventDefault();
    e.stopPropagation();
    vbarDragging = true;
    const startY = e.clientY;
    const startTop = sd.scrollTop;
    const onMove = (ev: PointerEvent) => {
      const { scrollHeight, clientHeight } = sd;
      const denom = clientHeight - vbarH;
      if (denom <= 0) return;
      sd.scrollTop = startTop + (ev.clientY - startY) * ((scrollHeight - clientHeight) / denom);
    };
    const onUp = () => {
      vbarDragging = false;
      window.removeEventListener('pointermove', onMove);
      window.removeEventListener('pointerup', onUp);
    };
    window.addEventListener('pointermove', onMove);
    window.addEventListener('pointerup', onUp);
  }

  function vbarTrackDown(e: PointerEvent) {
    const sd = view?.scrollDOM;
    if (!sd) return;
    if ((e.target as HTMLElement).classList.contains('ed-vbar-thumb')) return;
    const track = e.currentTarget as HTMLElement;
    const rect = track.getBoundingClientRect();
    const { scrollHeight, clientHeight } = sd;
    const denom = clientHeight - vbarH;
    if (denom <= 0) return;
    const ratio = (e.clientY - rect.top - vbarH / 2) / denom;
    sd.scrollTop = Math.max(0, Math.min(1, ratio)) * (scrollHeight - clientHeight);
  }

  /* Wire scroll + geometry observers to the live view. Re-runs per
     viewVersion (fresh EditorView per file load). The content element
     is observed too — doc edits / folds change scrollHeight without
     firing a scroll event. */
  $effect(() => {
    void viewVersion;
    const sd = view?.scrollDOM;
    if (!sd) return;
    refreshVbar();
    const onScroll = () => {
      refreshVbar();
      // The peek popup is anchored to a viewport position — scrolling
      // moves the hunk away from it, so dismiss rather than drift.
      if (gutterPopup) gutterPopup = null;
    };
    sd.addEventListener('scroll', onScroll, { passive: true });
    const ro = new ResizeObserver(refreshVbar);
    ro.observe(sd);
    const content = sd.querySelector('.cm-content');
    if (content) ro.observe(content);
    return () => {
      sd.removeEventListener('scroll', onScroll);
      ro.disconnect();
    };
  });

  /* Overview ruler (à la VS Code) — colored marks on the scrollbar strip
     showing WHERE in the document the git changes live. Derived from the
     same LineChanges map as the gutter change bar; contiguous same-kind
     lines collapse into one block so a 50-line hunk is one DOM node. */
  let rulerMarks = $state<{ top: number; h: number; kind: LineChangeKind }[]>([]);
  function buildRulerMarks(
    map: LineChanges,
    totalLines: number
  ): { top: number; h: number; kind: LineChangeKind }[] {
    if (map.size === 0 || totalLines < 1) return [];
    const lines = [...map.keys()].sort((a, b) => a - b);
    const blocks: { top: number; h: number; kind: LineChangeKind }[] = [];
    let start = -1;
    let prev = -2;
    let kind: LineChangeKind = 'add';
    const flush = (end: number) => {
      if (start < 0) return;
      blocks.push({
        top: ((start - 1) / totalLines) * 100,
        h: ((end - start + 1) / totalLines) * 100,
        kind
      });
    };
    for (const ln of lines) {
      const k = map.get(ln)!;
      if (ln === prev + 1 && k === kind) {
        prev = ln;
        continue;
      }
      flush(prev);
      start = ln;
      prev = ln;
      kind = k;
    }
    flush(prev);
    return blocks;
  }

  /* Click-to-peek diff popup on the gutter change bar. Clicking a mark
     opens a floating GitHub-style inline diff of that hunk: paired
     old/new lines collapse into ONE row with word-level del/ins
     segments (diffWordsWithSpace — same engine as DiffView), unpaired
     lines render as full del/add rows. Rows are snapshotted at click
     time so a background change-bar refresh can't shift them under the
     reader. */
  type PopupPart = { text: string; hl?: 'add' | 'del' };
  type PopupRow = {
    kind: 'change' | 'add' | 'del';
    oldNo: number | null;
    newNo: number | null;
    parts: PopupPart[];
  };
  let gutterHunks: ChangeHunk[] = [];
  let gutterLineHunk = new Map<number, number>();
  let gutterPopup = $state<{
    key: number;
    top: number;
    left: number;
    dels: number;
    adds: number;
    rows: PopupRow[];
  } | null>(null);
  let gutterPopupEl = $state<HTMLDivElement | null>(null);

  function buildPopupRows(h: ChangeHunk): PopupRow[] {
    const rows: PopupRow[] = [];
    const n = Math.max(h.oldLines.length, h.newLines.length);
    for (let i = 0; i < n; i++) {
      const o = h.oldLines[i];
      const nw = h.newLines[i];
      const oldNo = o != null ? h.oldStart + i : null;
      const newNo = nw != null ? h.newStart + i : null;
      if (o != null && nw != null) {
        const parts: PopupPart[] = diffWordsWithSpace(o, nw).map((w) =>
          w.added ? { text: w.value, hl: 'add' as const }
          : w.removed ? { text: w.value, hl: 'del' as const }
          : { text: w.value }
        );
        rows.push({ kind: 'change', oldNo, newNo, parts });
      } else if (o != null) {
        rows.push({ kind: 'del', oldNo, newNo: null, parts: [{ text: o }] });
      } else {
        rows.push({ kind: 'add', oldNo: null, newNo, parts: [{ text: nw! }] });
      }
    }
    return rows;
  }

  function onChangeMarkClick(lineNo: number, ev: MouseEvent): boolean {
    const idx = gutterLineHunk.get(lineNo);
    if (idx == null) {
      gutterPopup = null;
      return false;
    }
    if (gutterPopup?.key === idx) {
      gutterPopup = null;
      return true;
    }
    const h = gutterHunks[idx];
    const rows = buildPopupRows(h);
    const rect = edEl.getBoundingClientRect();
    const popW = Math.min(560, rect.width - 24);
    let left = ev.clientX - rect.left + 12;
    if (left + popW > rect.width - 12) left = Math.max(12, rect.width - 12 - popW);
    const estH = Math.min(300, 40 + rows.length * 20);
    let top = ev.clientY - rect.top + 8;
    if (top + estH > rect.height - 10) top = Math.max(10, ev.clientY - rect.top - estH - 8);
    gutterPopup = {
      key: idx,
      top,
      left,
      dels: h.oldLines.length,
      adds: h.newLines.length,
      rows
    };
    return true;
  }

  /** Fetch `git diff` for the active file and push parsed per-line
   *  markers into the editor's changeBar state field. Silent on
   *  non-git roots / untracked paths — change bar just stays empty. */
  let cbDebounce: ReturnType<typeof setTimeout> | null = null;
  async function refreshChangeBar() {
    if (!view || !lastLoadedPath || !repoPath) return;
    if (!lastLoadedPath.startsWith(repoPath)) return;
    const rel = lastLoadedPath.slice(repoPath.length + 1);
    if (!rel) return;
    try {
      const diff = await invoke<string>('git_diff', {
        repo: repoPath,
        path: rel,
        staged: false
      });
      const parsed = parseUnifiedDiff(diff);
      view.dispatch({ effects: setChangeBar.of(parsed.map) });
      rulerMarks = buildRulerMarks(parsed.map, view.state.doc.lines);
      gutterHunks = parsed.hunks;
      gutterLineHunk = parsed.lineHunk;
    } catch {
      view.dispatch({ effects: setChangeBar.of(new Map()) });
      rulerMarks = [];
      gutterHunks = [];
      gutterLineHunk = new Map();
      gutterPopup = null;
    }
  }
  function scheduleChangeBar() {
    if (cbDebounce) clearTimeout(cbDebounce);
    cbDebounce = setTimeout(() => { void refreshChangeBar(); }, 300);
  }

  /** Restart the autosave countdown. Fires from the updateListener on
   *  every doc change when the buffer is dirty; the most recent timer
   *  wins, so steady typing never triggers a save mid-keystroke.
   *  Skipped entirely when the user has flipped `editorPrefs.autosave`
   *  off — Mod-S keymap (and the `saveNow` exported action) still
   *  work, so manual saves remain available. */
  function scheduleAutosave() {
    if (!editorPrefs.autosave) return;
    if (autosaveTimer) clearTimeout(autosaveTimer);
    autosaveTimer = setTimeout(() => {
      autosaveTimer = null;
      if (view && dirty) void save(view);
    }, AUTOSAVE_MS);
  }

  /** Loose path equality for matching `fs:changed` payloads against the
   *  open buffer. The watcher and the `path` prop are both absolute on
   *  macOS; we normalise trailing slashes + collapse the rare duplicate
   *  separator so a cosmetic difference doesn't miss a real match. */
  function pathsEqual(a: string, b: string): boolean {
    if (!a || !b) return false;
    const norm = (p: string) => p.trim().replace(/\/+/g, '/').replace(/\/$/, '');
    return norm(a) === norm(b);
  }

  let watchUnlisten: UnlistenFn | null = null;
  onMount(async () => {
    watchUnlisten = await listen<{ path: string }>('fs:changed', (e) => {
      // Always refresh the git change-bar (cheap, covers sibling writes).
      scheduleChangeBar();

      // Live buffer sync: only when the change is THIS open file.
      const changed = e.payload?.path;
      if (!changed || loading || !pathsEqual(changed, lastLoadedPath)) return;

      // Unsaved manual edits win — never clobber them on an agent write.
      if (dirty) {
        agentEditPendingReload = true;
        return;
      }

      // Echo-dedupe: our own autosave fires fs:changed too. Re-read and
      // only reload when disk actually diverged from what we last saved,
      // so a self-write is a no-op (no view-recreate flicker).
      void (async () => {
        try {
          const onDisk = await invoke<string>('fs_read_file', { path: changed });
          if (onDisk !== savedContents) await reloadFromDisk();
        } catch {
          /* Read failed (file vanished / perms) — leave the buffer as-is;
             the change-bar refresh above already reflects git state. */
        }
      })();
    });
  });

  export async function reload() {
    if (!path) return;
    // `load` recreates the view, so capture focus and restore it after —
    // an external-change reload shouldn't yank the caret out of the editor.
    const wasFocused = view?.hasFocus ?? false;
    const prev = lastLoadedPath;
    lastLoadedPath = '';
    await load(prev || path);
    if (wasFocused) view?.focus();
  }

  /** Reload the buffer from disk and clear the pending-reload flag.
   *  Thin alias over `reload()` with a stable name later phases call
   *  after a hunk-reject re-writes the file. Cursor + scroll round-trip
   *  through the `recordCursor` store, so position is preserved. */
  export async function reloadFromDisk() {
    await reload();
    agentEditPendingReload = false;
  }

  /* Recompute the inline-hunk overlay whenever the pending agent edits for
     this file change OR a fresh view is created (viewVersion) OR a hunk was
     just resolved (resolvedVersion). For a SINGLE pending edit (the common
     case) we diff its `oldText` against the LIVE buffer rather than the
     edit's frozen `newText`: a reject splices lines back to disk and shrinks/
     grows the doc, so re-diffing against the live text keeps every remaining
     hunk's line numbers exact (sequential multi-hunk reject no longer drifts).
     Accepted hunks still differ from `oldText`, so they're suppressed by
     `resolvedHunkIds` (ids are old-anchored ⇒ stable across the recompute).
     With STACKED edits there's no single coherent new-side, so we fall back
     to each edit's own frozen newText — multi-edit refinement is deferred
     (P4) and already documented as approximate. */
  /** 1-based line number containing string index `idx`. */
  function lineOfIndex(text: string, idx: number): number {
    let line = 1;
    for (let i = 0; i < idx; i++) if (text.charCodeAt(i) === 10) line++;
    return line;
  }

  /** Anchor a snippet edit inside the live buffer: the new side is what's
   *  on disk after the edit applied (normal case); the old side matches
   *  when the edit was reverted or not yet applied. Returns the 1-based
   *  line where the snippet begins, or null when neither side is present
   *  (buffer drifted) — in which case we render nothing rather than
   *  painting the hunk at line 1. */
  function anchorSnippet(live: string, newText: string, oldText: string): number | null {
    if (newText) {
      const idx = live.indexOf(newText);
      if (idx >= 0) return lineOfIndex(live, idx);
    }
    if (oldText) {
      const idx = live.indexOf(oldText);
      if (idx >= 0) return lineOfIndex(live, idx);
    }
    return null;
  }

  $effect(() => {
    // Track dependencies explicitly.
    const edits = pendingEdits;
    const selKey = selectedEditKey; // re-filter the overlay when selection changes
    void viewVersion;
    void resolvedVersion;
    if (!view) return;
    const singleEdit = edits.length === 1;
    const liveText = view.state.doc.toString();
    const owners = new Map<string, { sessionId: string; toolId: string }>();
    const byEdit = new Map<string, string[]>();
    const merged: Hunk[] = [];
    for (const e of edits) {
      if (e.oldText == null || e.newText == null) continue;
      const key = `${e.sessionId}:${e.toolId}`;
      /* Roster + owners come from the FROZEN edit (oldText→newText) so the
         per-edit id set stays complete as hunks resolve — the all-rejected→
         'reverted' vs any-accepted→'kept' tally in resolveHunk depends on it.
         Ids are NAMESPACED by edit key (`key#oldId`): old-anchored ids alone
         collide when several edits stack on one file (two edits both anchor a
         hunk at old line 5), which cross-assigned owners and piled overlapping
         decorations. Namespacing keeps each edit's hunks distinct + stable. */
      const frozen = computeHunks(e.oldText, e.newText);
      const ids: string[] = [];
      for (const h of frozen) {
        const nid = `${key}#${h.id}`;
        ids.push(nid);
        owners.set(nid, { sessionId: e.sessionId, toolId: e.toolId });
      }
      byEdit.set(key, ids);
      if (e.wholeFile) {
        /* Whole-file edit (Write): oldText/newText ARE the document, so
           hunk coordinates are already file-absolute. For a single edit,
           diff against the live buffer so a prior reject's line-count
           change is reflected exactly; resolved hunks are filtered out. */
        const newSide = singleEdit ? liveText : e.newText;
        for (const h of computeHunks(e.oldText, newSide)) {
          const nid = `${key}#${h.id}`;
          if (!resolvedHunkIds.has(nid)) merged.push({ ...h, id: nid });
        }
      } else {
        /* Snippet edit (Edit/MultiEdit): oldText/newText are fragments —
           diffing them yields snippet-relative line numbers (1..n), which
           painted hunks at the TOP of the file regardless of where the
           edit landed. Locate the snippet in the live buffer and offset
           every hunk to its real position; if the snippet can't be found
           (buffer drifted past both sides), skip rendering entirely. */
        const anchor = anchorSnippet(liveText, e.newText, e.oldText);
        if (anchor == null) continue;
        for (const h of frozen) {
          const nid = `${key}#${h.id}`;
          if (resolvedHunkIds.has(nid)) continue;
          merged.push({
            ...h,
            id: nid,
            oldStart: h.oldStart + anchor - 1,
            newStart: h.newStart + anchor - 1
          });
        }
      }
    }
    hunkOwners = owners;
    editHunkIds = byEdit;
    /* When the reviewer has picked an edit that lives in THIS file, show only
       that edit's hunks — stacked edits on one file otherwise pile overlapping
       overlays ("наслоения"). A stale selection for another file (`!byEdit.has`)
       falls through to showing all, so opening a file normally still works. */
    const displayed = selKey && byEdit.has(selKey)
      ? merged.filter((h) => h.id.startsWith(selKey + '#'))
      : merged;
    currentHunks = displayed;
    view.dispatch({ effects: setHunks.of(displayed) });
  });

  /* Emphasise + scroll to the hunks of the edit selected in the ReviewPane.
     Reactive on `selectedEditKey` AND `currentHunks` so it fires once the
     overlay is (re)computed — e.g. right after the file opens and the
     recompute effect populates `currentHunks`. We scroll only when the
     SELECTION changes (not on every recompute), so reviewing in the editor
     doesn't yank the viewport. */
  let lastFocusScrollKey: string | null = null;
  $effect(() => {
    const key = selectedEditKey;
    const hunks = currentHunks; // reactive dependency
    if (!view) return;
    if (!key) {
      view.dispatch({ effects: setFocusedHunks.of([]) });
      lastFocusScrollKey = null;
      return;
    }
    const ids = new Set(editHunkIds.get(key) ?? []);
    const present = hunks.filter((h) => ids.has(h.id));
    view.dispatch({ effects: setFocusedHunks.of(present.map((h) => h.id)) });
    if (present.length > 0 && key !== lastFocusScrollKey) {
      lastFocusScrollKey = key;
      const firstLine = Math.min(...present.map((h) => hunkNewRange(h).fromLine));
      /* Defer past load()'s scroll-restore rAF (which sets scrollDOM.scrollTop
         back to the saved position) — otherwise our jump-to-hunk is clobbered
         the same frame and the editor never scrolls to the selected edit. */
      requestAnimationFrame(() => goToLine(firstLine));
    }
  });

  /* Finalise a hunk's resolution: drop it from the live set, and once
     every hunk of its owning edit is resolved, flip the EditEvent's status
     so the chat-side review (ReviewPane / EditDiffCard) agrees. All-reject
     → 'reverted'; any accept in the mix → 'kept' (reviewed). */
  function resolveHunk(h: Hunk, kind: 'accept' | 'reject') {
    resolvedHunkIds.add(h.id);
    const owner = hunkOwners.get(h.id);
    if (owner) {
      const key = `${owner.sessionId}:${owner.toolId}`;
      if (kind === 'reject') perEditRejects.set(key, (perEditRejects.get(key) ?? 0) + 1);
      const ids = editHunkIds.get(key) ?? [];
      if (ids.length > 0 && ids.every((id) => resolvedHunkIds.has(id))) {
        const rejects = perEditRejects.get(key) ?? 0;
        updateEditEvent(owner.sessionId, owner.toolId, {
          status: rejects === ids.length ? 'reverted' : 'kept'
        });
      }
    }
    resolvedVersion++; // re-runs the recompute $effect → redraws overlay
  }

  /** Tab: accept the hunk under the caret (content already on disk — just
   *  clear it). Returns false when the caret isn't in a hunk so Tab keeps
   *  its default (indent / fall-through). */
  function acceptFocusedHunk(v: EditorView): boolean {
    const line = v.state.doc.lineAt(v.state.selection.main.head).number;
    const h = hunkAtLine(currentHunks, line);
    if (!h) return false;
    resolveHunk(h, 'accept');
    return true;
  }

  /** Esc: reject the hunk under the caret — splice its lines back to the
   *  pre-edit text in the buffer, persist to disk, clear it. Returns false
   *  when the caret isn't in a hunk so Esc keeps its default (close
   *  search / autocomplete). */
  function rejectFocusedHunk(v: EditorView): boolean {
    const line = v.state.doc.lineAt(v.state.selection.main.head).number;
    const h = hunkAtLine(currentHunks, line);
    if (!h) return false;
    const change = buildHunkRevert(v.state.doc, h);
    v.dispatch({ changes: change });
    void save(v);
    resolveHunk(h, 'reject');
    return true;
  }

  export async function saveNow() {
    if (view) await save(view);
  }

  /** Move the caret to the start of `line` (1-based) and scroll it
   *  into the centre of the viewport. Used by the symbol picker —
   *  also exported so other call sites (jump-to-error, follow-link)
   *  can land on the same surface without re-implementing the
   *  CodeMirror dispatch dance. Clamped so an out-of-range line
   *  number from a stale picker entry doesn't throw. */
  export function goToLine(line1: number) {
    if (!view) return;
    const doc = view.state.doc;
    const safe = Math.max(1, Math.min(doc.lines, line1 | 0));
    const lineInfo = doc.line(safe);
    view.dispatch({
      selection: { anchor: lineInfo.from, head: lineInfo.from },
      effects: EditorView.scrollIntoView(lineInfo.from, { y: 'center' })
    });
    /* Steal focus so the next keystroke lands in the editor, not in
       whatever overlay-input the user just dismissed. */
    view.focus();
  }

  /* Cross-component goto bus — the symbol picker (and any future
     jump-here surface) fires `woom:editor:goto` with the editor
     instance id + file + 1-based line. We filter by both instance
     and path so every Editor component can listen safely without
     two buffers fighting for the jump. */
  function onGoto(ev: Event) {
    const e = ev as CustomEvent<{ editorId?: string; filePath?: string; line?: number }>;
    if (!e.detail) return;
    if (e.detail.editorId && e.detail.editorId !== instanceId) return;
    if (e.detail.filePath && e.detail.filePath !== lastLoadedPath) return;
    const line = e.detail.line;
    if (typeof line !== 'number' || line < 1) return;
    goToLine(line);
  }
  onMount(() => {
    window.addEventListener('woom:editor:goto', onGoto as EventListener);
    return () => window.removeEventListener('woom:editor:goto', onGoto as EventListener);
  });

  $effect(() => {
    /* Refresh change bar whenever repoPath changes (open new repo). */
    if (repoPath) scheduleChangeBar();
  });

  $effect(() => {
    void load(path);
  });

  /* Re-configure the theme compartment when the user flips palette.
     `view?.dispatch` is a no-op when the editor isn't mounted yet,
     so this is safe at any time. */
  $effect(() => {
    const name = themeState.name;
    if (!view) return;
    view.dispatch({
      effects: themeCompartment.reconfigure(editorThemeExtension(name))
    });
  });

  /* Same compartment dance for word-wrap: dispatch a reconfigure
     when the prop flips so the user can toggle without losing their
     scroll / selection. */
  $effect(() => {
    const wrap = wordWrap;
    if (!view) return;
    view.dispatch({
      effects: wrapCompartment.reconfigure(wrap ? EditorView.lineWrapping : [])
    });
  });

  /** Snapshot the current buffer text. Useful for parents that want
   *  to seed a preview without subscribing to every keystroke via
   *  onTextChange — call once when opening the preview, then rely
   *  on the callback for incremental updates. */
  export function getText(): string {
    return view?.state.doc.toString() ?? '';
  }

  onDestroy(() => {
    watchUnlisten?.();
    if (cbDebounce) clearTimeout(cbDebounce);
    if (autosaveTimer) clearTimeout(autosaveTimer);
    /* Last-chance flush of the current cursor so a quit (or column
     * close) doesn't lose the user's position. The updateListener
     * already records most positions on the fly; this catches the
     * tail-end case where the user typed and immediately quit
     * before the debounce flushed. */
    if (view && lastLoadedPath) {
      const sel = view.state.selection.main;
      recordCursor(lastLoadedPath, {
        from: sel.from,
        to: sel.to,
        scrollTop: view.scrollDOM?.scrollTop ?? 0
      });
      /* Same idea for the buffer: if the user typed and immediately
         closed the column, the autosave timer never fired. Fire a
         best-effort write here so unsaved keystrokes don't vanish.
         Bypass save() to avoid touching the about-to-be-destroyed
         view via refreshChangeBar. */
      if (dirty) {
        const pendingPath = lastLoadedPath;
        const pendingContents = view.state.doc.toString();
        void invoke('fs_write_file', { path: pendingPath, contents: pendingContents })
          .then(() => onSaved?.(pendingPath))
          .catch(() => {});
      }
    }
    view?.destroy();
  });
</script>

<svelte:window
  onkeydown={(e) => {
    if (e.key === 'Escape' && gutterPopup) gutterPopup = null;
  }}
  onpointerdown={(e) => {
    if (!gutterPopup) return;
    const t = e.target as HTMLElement;
    if (gutterPopupEl?.contains(t)) return;
    /* Clicks on the change bar are handled by the gutter's own
       mousedown (open / toggle) — closing here too would make the
       same-mark toggle reopen instantly. */
    if (t.closest?.('.cm-changebar')) return;
    gutterPopup = null;
  }}
/>

<div class="ed" bind:this={edEl}>
  {#if error}
    <div class="ed-error">{error}</div>
  {/if}
  <div class="ed-surface" bind:this={editorEl}></div>
  {#if gutterPopup}
    <div
      class="ed-gd"
      style="top:{gutterPopup.top}px;left:{gutterPopup.left}px"
      bind:this={gutterPopupEl}
    >
      <div class="ed-gd-head">
        {#if gutterPopup.dels > 0}<span class="ed-gd-stat ed-gd-stat--del">−{gutterPopup.dels}</span>{/if}
        {#if gutterPopup.adds > 0}<span class="ed-gd-stat ed-gd-stat--add">+{gutterPopup.adds}</span>{/if}
        <span class="ed-gd-title">uncommitted change</span>
        <button class="ed-gd-x" onclick={() => (gutterPopup = null)} aria-label="Close">✕</button>
      </div>
      <div class="ed-gd-body" use:overlayScrollbars>
        {#each gutterPopup.rows as r, i (i)}
          <div class="ed-gd-row ed-gd-row--{r.kind}">
            <span class="ed-gd-ln">{r.oldNo ?? ''}</span>
            <span class="ed-gd-ln">{r.newNo ?? ''}</span>
            <span class="ed-gd-sign">{r.kind === 'add' ? '+' : r.kind === 'del' ? '−' : '±'}</span>
            <span class="ed-gd-code">{#each r.parts as p, j (j)}<span class={p.hl ? `ed-gd-seg ed-gd-seg--${p.hl}` : ''}>{p.text}</span>{/each}</span>
          </div>
        {/each}
      </div>
    </div>
  {/if}
  {#if vbarVisible}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="ed-vbar" class:ed-vbar--drag={vbarDragging} onpointerdown={vbarTrackDown} aria-hidden="true">
      <div
        class="ed-vbar-thumb"
        style="top:{vbarTop}px;height:{vbarH}px"
        onpointerdown={vbarThumbDown}
      ></div>
    </div>
  {/if}
  {#if rulerMarks.length > 0}
    <!-- Overview ruler — sits over the scrollbar track (pointer-events
         none so the thumb stays draggable through it). -->
    <div class="ed-ruler" aria-hidden="true">
      {#each rulerMarks as m, i (i)}
        <div
          class="ed-ruler-mark ed-ruler-mark--{m.kind}"
          style="top:{m.top}%;height:{m.h}%"
        ></div>
      {/each}
    </div>
  {/if}
  {#if loading}<div class="ed-spinner">Loading…</div>{/if}
</div>

<style>
  .ed { position: relative; height: 100%; display: flex; flex-direction: column; overflow: hidden; background: var(--bg-0); }
  .ed-surface { flex: 1; overflow: hidden; min-height: 0; }
  .ed-surface :global(.cm-editor) { height: 100%; font-family: 'JetBrains Mono', ui-monospace, 'SF Mono', monospace; font-size: 13px; }
  .ed-surface :global(.cm-editor.cm-focused) { outline: none; }
  .ed-surface :global(.cm-scroller) { font-family: inherit; }

  /* Native vertical scrollbar suppressed — WKWebView ignores
     ::-webkit-scrollbar styling under macOS overlay scrollbars (the
     trackpad default), so the styled-native approach (v0.4.3-0.4.6)
     never showed for most users. `.ed-vbar` below is the real bar.
     Horizontal stays native: rarely needed, overlay is fine there. */
  .ed-surface :global(.cm-scroller::-webkit-scrollbar:vertical) {
    width: 0;
  }
  .ed-surface :global(.cm-scroller::-webkit-scrollbar:horizontal) {
    height: 10px;
  }
  .ed-surface :global(.cm-scroller::-webkit-scrollbar-track),
  .ed-surface :global(.cm-scroller::-webkit-scrollbar-corner) {
    background: transparent;
  }
  .ed-surface :global(.cm-scroller::-webkit-scrollbar-thumb) {
    background: color-mix(in srgb, var(--text-mute) 48%, transparent);
    background-clip: padding-box;
    border: 2px solid transparent;
    border-radius: 7px;
  }

  /* Custom always-visible vertical scrollbar (VS Code-style). Sits over
     the editor's right edge; overview-ruler dots paint on top of the
     track (pointer-events: none keeps the thumb draggable through). */
  .ed-vbar {
    position: absolute;
    top: 0;
    bottom: 0;
    right: 0;
    width: 14px;
    z-index: 4;
    background: color-mix(in srgb, var(--text-mute) 6%, transparent);
  }
  .ed-vbar-thumb {
    position: absolute;
    left: 2px;
    right: 2px;
    border-radius: 5px;
    background: color-mix(in srgb, var(--text-mute) 45%, transparent);
    transition: background 120ms;
  }
  .ed-vbar-thumb:hover {
    background: color-mix(in srgb, var(--text-mute) 60%, transparent);
  }
  .ed-vbar--drag .ed-vbar-thumb {
    background: color-mix(in srgb, var(--text-mute) 75%, transparent);
  }

  /* Overview ruler — left lane of the scrollbar track. Marks paint over
     the track but under the pointer (pointer-events: none), so dragging
     the thumb through a mark still works. Palette echoes the gutter
     change bar. */
  .ed-ruler {
    position: absolute;
    top: 0;
    bottom: 10px; /* leave the horizontal-bar corner clear */
    right: 9px;
    width: 4px;
    pointer-events: none;
    z-index: 5;
  }
  .ed-ruler-mark {
    position: absolute;
    left: 0;
    width: 100%;
    min-height: 3px;
    border-radius: 1px;
  }
  .ed-ruler-mark--add { background: rgba(111, 174, 136, 0.85); }
  .ed-ruler-mark--mod { background: rgba(217, 184, 110, 0.85); }
  .ed-ruler-mark--del { background: rgba(232, 130, 100, 0.9); }

  /* Git change bar — a dedicated thin gutter column (à la VS Code / Cursor):
     crisp full-line-height stripes that never shift the code. add = green,
     mod = ochre, del = a small red triangle on the line above removed code. */
  /* 8px column = humane click target for the peek-diff popup; the
     visible stripe stays 3px (transparent right border + padding-box
     clip on the mark below). */
  .ed-surface :global(.cm-changebar) {
    width: 8px;
    padding: 0;
    background: transparent;
    border: none;
  }
  .ed-surface :global(.cm-changebar .cm-gutterElement) {
    padding: 0;
    display: flex;
    align-items: stretch;
    cursor: pointer;
  }
  /* Marks are clickable (peek-diff popup): visible stripe is 3px but
     the element fills the 8px column (transparent right border,
     background clipped to padding) so the hit target is humane; hover
     shrinks the border → stripe widens to 6px, VS Code-style. */
  .ed-surface :global(.cm-changebar-mark) {
    width: 8px;
    align-self: stretch;
    border-right: 5px solid transparent;
    background-clip: padding-box;
    transition: border-right-width 90ms ease, filter 90ms ease;
  }
  .ed-surface :global(.cm-changebar-mark--add) { background-color: #6faE88; }
  .ed-surface :global(.cm-changebar-mark--mod) { background-color: #d9b86e; }
  .ed-surface :global(.cm-gutterElement:hover .cm-changebar-mark--add),
  .ed-surface :global(.cm-gutterElement:hover .cm-changebar-mark--mod) {
    border-right-width: 2px;
    filter: brightness(1.25);
  }
  /* Deleted-above indicator: a downward red triangle pinned to the cell
     bottom, so it reads as "lines removed here" rather than a full stripe. */
  .ed-surface :global(.cm-changebar-mark--del) {
    width: 0; height: 0;
    background: transparent;
    align-self: flex-end;
    border-left: 4px solid transparent;
    border-right: 4px solid transparent;
    border-top: 5px solid #e88264;
    margin-left: -1px;
    transition: transform 90ms ease;
    transform-origin: 50% 100%;
  }
  .ed-surface :global(.cm-gutterElement:hover .cm-changebar-mark--del) {
    transform: scale(1.4);
  }

  /* Peek-diff popup (click on a change-bar mark). GitHub-style inline
     rows: word-level del segments red, ins segments green, both in the
     SAME line for modified pairs. */
  .ed-gd {
    position: absolute;
    z-index: 30;
    width: min(560px, calc(100% - 24px));
    background: var(--bg-2);
    border: 1px solid var(--border-hi);
    border-radius: 8px;
    box-shadow: 0 8px 28px rgba(0, 0, 0, 0.45);
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }
  .ed-gd-head {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 5px 8px 5px 10px;
    border-bottom: 1px solid var(--border);
    font-size: 11px;
  }
  .ed-gd-stat { font-weight: 600; font-variant-numeric: tabular-nums; }
  .ed-gd-stat--del { color: #e88264; }
  .ed-gd-stat--add { color: #6faE88; }
  .ed-gd-title { color: var(--text-mute); }
  .ed-gd-x {
    margin-left: auto;
    background: none;
    border: none;
    color: var(--text-mute);
    cursor: pointer;
    font-size: 11px;
    padding: 2px 4px;
    border-radius: 4px;
  }
  .ed-gd-x:hover { color: var(--text-1); background: color-mix(in srgb, var(--text-mute) 15%, transparent); }
  .ed-gd-body {
    overflow: auto;
    max-height: 260px;
    padding: 4px 0;
    font-family: 'JetBrains Mono', ui-monospace, 'SF Mono', monospace;
    font-size: 12px;
    line-height: 1.55;
  }
  .ed-gd-row {
    display: flex;
    align-items: baseline;
    white-space: pre;
    min-width: max-content;
    padding-right: 12px;
  }
  .ed-gd-row--del { background: rgba(232, 130, 100, 0.10); }
  .ed-gd-row--add { background: rgba(111, 174, 136, 0.10); }
  .ed-gd-ln {
    flex: none;
    width: 34px;
    text-align: right;
    padding-right: 6px;
    color: var(--text-mute);
    font-variant-numeric: tabular-nums;
    user-select: none;
  }
  .ed-gd-sign {
    flex: none;
    width: 16px;
    text-align: center;
    color: var(--text-mute);
    user-select: none;
  }
  .ed-gd-row--del .ed-gd-sign { color: #e88264; }
  .ed-gd-row--add .ed-gd-sign { color: #6faE88; }
  .ed-gd-code { flex: none; }
  .ed-gd-seg { border-radius: 2px; }
  .ed-gd-seg--del {
    background: rgba(232, 130, 100, 0.28);
    text-decoration: line-through;
    text-decoration-color: rgba(232, 130, 100, 0.7);
  }
  .ed-gd-seg--add { background: rgba(111, 174, 136, 0.28); }
  /* Inline agentic-edit overlay (sibling to the change bar above).
     Added/modified lines get a soft green wash across the full line;
     removed lines render as a ghost block (struck-through, red-tinted)
     anchored where they used to be. Tones echo the change-bar palette. */
  .ed-surface :global(.cm-inline-hunk--add) {
    background: rgba(111, 174, 136, 0.16);
  }
  .ed-surface :global(.cm-inline-hunk--del) {
    background: rgba(232, 130, 100, 0.12);
    border-left: 2px solid rgba(232, 130, 100, 0.55);
    padding: 0 0 0 6px;
    margin: 0;
    font-family: inherit;
    font-size: 13px;
    white-space: pre-wrap;
  }
  .ed-surface :global(.cm-inline-hunk--del-line) {
    color: var(--text-2);
    text-decoration: line-through;
    text-decoration-color: rgba(232, 130, 100, 0.7);
    opacity: 0.85;
  }
  /* Focused edit — the hunks of the row selected in the ReviewPane. Brighter
     wash + a left accent rail so the reviewer's eye lands on exactly that
     edit's chunk amongst stacked overlays. */
  .ed-surface :global(.cm-inline-hunk--add.cm-inline-hunk--focus) {
    background: rgba(111, 174, 136, 0.32);
    box-shadow: inset 2px 0 0 var(--accent, #c9784f);
  }
  .ed-surface :global(.cm-inline-hunk--del.cm-inline-hunk--focus) {
    background: rgba(232, 130, 100, 0.22);
    border-left-color: var(--accent, #c9784f);
  }
  .ed-error {
    padding: 8px 14px;
    background: rgba(232, 130, 100, 0.12);
    color: var(--error);
    border-bottom: 1px solid rgba(232, 130, 100, 0.24);
    font-size: 12.5px;
  }
  .ed-spinner {
    position: absolute;
    top: 8px; right: 12px;
    font-size: 11px;
    color: var(--text-2);
  }
</style>
