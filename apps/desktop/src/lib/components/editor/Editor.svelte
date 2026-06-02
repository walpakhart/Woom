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
    parseUnifiedDiffToLineChanges,
    type LineChanges
  } from '$lib/components/editor/changeBar';
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
    pendingEdits?: { sessionId: string; toolId: string; oldText: string; newText: string }[];
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
            changeBarExtension(),
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
      const map: LineChanges = parseUnifiedDiffToLineChanges(diff);
      view.dispatch({ effects: setChangeBar.of(map) });
    } catch {
      view.dispatch({ effects: setChangeBar.of(new Map()) });
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
      const ids: string[] = [];
      for (const h of computeHunks(e.oldText, e.newText)) {
        const nid = `${key}#${h.id}`;
        ids.push(nid);
        owners.set(nid, { sessionId: e.sessionId, toolId: e.toolId });
      }
      byEdit.set(key, ids);
      /* Geometry to render + revert: for a single edit, diff against the live
         buffer so a prior reject's line-count change is reflected exactly;
         resolved hunks are filtered out (rejected ones also drop from the
         live diff naturally, accepted ones are suppressed here). */
      const newSide = singleEdit ? liveText : e.newText;
      for (const h of computeHunks(e.oldText, newSide)) {
        const nid = `${key}#${h.id}`;
        if (!resolvedHunkIds.has(nid)) merged.push({ ...h, id: nid });
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

<div class="ed">
  {#if error}
    <div class="ed-error">{error}</div>
  {/if}
  <div class="ed-surface" bind:this={editorEl}></div>
  {#if loading}<div class="ed-spinner">Loading…</div>{/if}
</div>

<style>
  .ed { position: relative; height: 100%; display: flex; flex-direction: column; overflow: hidden; background: var(--bg-0); }
  .ed-surface { flex: 1; overflow: hidden; min-height: 0; }
  .ed-surface :global(.cm-editor) { height: 100%; font-family: 'JetBrains Mono', ui-monospace, 'SF Mono', monospace; font-size: 13px; }
  .ed-surface :global(.cm-editor.cm-focused) { outline: none; }
  .ed-surface :global(.cm-scroller) { font-family: inherit; }

  /* Git change bar — a dedicated thin gutter column (à la VS Code / Cursor):
     crisp full-line-height stripes that never shift the code. add = green,
     mod = ochre, del = a small red triangle on the line above removed code. */
  .ed-surface :global(.cm-changebar) {
    width: 3px;
    padding: 0;
    background: transparent;
    border: none;
  }
  .ed-surface :global(.cm-changebar .cm-gutterElement) {
    padding: 0;
    display: flex;
    align-items: stretch;
  }
  .ed-surface :global(.cm-changebar-mark) {
    width: 3px;
    align-self: stretch;
  }
  .ed-surface :global(.cm-changebar-mark--add) { background: #6faE88; }
  .ed-surface :global(.cm-changebar-mark--mod) { background: #d9b86e; }
  /* Deleted-above indicator: a downward red triangle pinned to the cell
     bottom, so it reads as "lines removed here" rather than a full stripe. */
  .ed-surface :global(.cm-changebar-mark--del) {
    width: 0; height: 0;
    background: transparent;
    align-self: flex-end;
    border-left: 4px solid transparent;
    border-right: 4px solid transparent;
    border-top: 5px solid #e88264;
    margin-left: -2.5px;
  }
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
