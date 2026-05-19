# Woom — Editor (In-App Code Editor) Specification

**Version:** 0.1
**Last updated:** 2026-04-29
**Status:** describes shipping behaviour as of the current commit. v1
of the Editor column ships in the desktop app today; this document is a
ground-truth spec, not a forward-looking proposal.

> A workbench column for **opening a repo, browsing its file tree, and
> editing files locally**. Sits next to the agent (Claude / Cursor)
> column and acts as the *visible surface* of the agent's work — when
> an agent edits a file, the editor reloads it; when the user picks a
> selection, "Apply to Claude / Cursor" sends a precise line-range
> mention to the linked agent. Repo path is the canonical "where am I
> working" pointer for everything connected to it.

---

## 1. Vision & Non-Goals

### 1.1 Vision

The Editor column is **not** trying to replace VS Code or Cursor IDE.
Woom already runs Cursor and Claude as sidecars, and Cursor IDE itself
is a fully-featured editor we have no interest in re-implementing. The
column has a narrower job:

1. **Anchor the cwd.** Every other column (agent, GitHub PR, Jira
   ticket, canvas, terminal proposals) needs to know "what repo are we
   in right now". The Editor column owns that pointer for the linked
   agents.
2. **See what the agent did.** When Claude / Cursor edit a file, the
   editor reloads the buffer (if not dirty) and a fresh git diff appears
   in the Git panel. The user sees the change land without context
   switching.
3. **Slice context to the agent.** Select N lines, click "Apply to
   Claude" → the linked agent gets a `@<file>:LSTART-LEND` mention in
   its composer with the exact slice as referenced text.
4. **Triage diffs.** Built-in diff view (CodeMirror `MergeView`) for
   git status preview and conflict review.

### 1.2 Goals (v1, shipping)

1. Open any local folder, detect its git root, browse its tree.
2. Edit text files with CodeMirror 6, save with `⌘S`, no autosave.
3. Show git status decorations on tree rows (`M`, `A`, `D`, `??`, …).
4. Live-reload buffers on outside-process file changes (the agent
   modifies `auth.ts` → it reloads in the open tab if not dirty).
5. Persist open repo + open tabs across restarts.
6. Drag a file out of the tree onto an agent column → file mention.
7. "Apply to agent" sends a line-range mention to a chosen agent column.
8. Link / unlink the editor column to one or more agent sessions; cwd
   stays in sync.
9. MCP-driven re-targeting: `mcp__app__set_editor_repo_path` patches
   the column from the agent side, linked agents follow.

### 1.3 Non-Goals (v1)

- **Multi-cursor / VS Code-tier IntelliSense.** We rely on CodeMirror's
  `basicSetup` + language-specific extensions; nothing semantic.
- **Search across files.** The Command Palette (⌘K) indexes columns and
  inbox entries, not file contents.
- **Inline AI suggestions inside CodeMirror.** AI lives one column over,
  by design.
- **Split panes / two buffers side by side.** One editor surface per
  column. Open a second Editor column instance instead.
- **Inline rename / new-file / delete on the tree.** No mkfile, no
  delete, no rename in v1. Use the agent or the OS.
- **Virtualised tree / list.** Trees over a few thousand visible nodes
  start to scroll-lag — out of scope until it actually bites.
- **Syntax-aware refactors.** Pure text editor.

---

## 2. Column Anatomy

```
╭─ editor column ──────────────────────────────────────────────╮
│ ⠿ ⤢ ✕    Editor   Sagrada-Familia                            │  ColumnControls + bench head
├──────────────────────────────────────────────────────────────┤
│ ┌─ sidebar ───┐ ┌─ tabbar (open files) ─────────────────────┐│
│ │ forge ▾   ⇄ │ │ ▦ src/lib/x.ts ●   ▦ docs/CANVAS.md       ││
│ │ ⇋ Claude   │ ├─────────────────────────────────────────────┤│
│ │ + Link …   │ │                                            ││
│ ├─────────────┤ │            CodeMirror 6 surface             ││
│ │  ▣ Work tree│ │                  or                         ││
│ │  ▣ Git      │ │            DiffView (MergeView)             ││
│ ├─────────────┤ │                                            ││
│ │ FileTree    │ │  status: ev-error / ev-loading              ││
│ │  / GitPanel │ │                                            ││
│ └─────────────┘ └─────────────────────────────────────────────┘│
╰──────────────────────────────────────────────────────────────╯
```

Concretely, the Svelte composition:

- `EditorColumn.svelte` — workbench-side shell: `ColumnControls`,
  resize handle, bench head with the column's art-name, then
  `<EditorView>` taking the rest.
- `EditorView.svelte` — the actual content: left sidebar (`ev-left-head`
  + `ev-sidebar-tabs` toggling between **Work tree** and **Git**),
  right pane with tabbar + `Editor` / `DiffView`, splitter via
  `Splitter` (persisted under key `editor-main`, default 300px,
  bounds [180, 520]).
- `FileTree.svelte` — recursive lazy-loaded tree over `fs_list_dir`.
- `Editor.svelte` — single CodeMirror instance per active path.
- `DiffView.svelte` — read-only `MergeView` for git diffs / Apply
  preview.
- `GitPanel.svelte` (sidebar tab) — branch, file list, stage / unstage,
  commit message, push / pull.

There is no breadcrumb bar; the file's repo-relative path is shown on
its tab via `relToRepo(path)` (see `apps/desktop/src/lib/components/editor/EditorView.svelte`,
`relToRepo` ~396).

### 2.1 Bench head and column controls

```svelte
<section
  class="wb-column wb-column--editor"
  class:wb-column--maximized={layoutState.maximizedInstanceId === instanceId}
  data-instance-id={instanceId}
  data-kind="editor"
  ...
>
  <ColumnControls {instanceId} kind="editor" />
  <div class="wb-col-resize" .../>
  <div class="editor-bench-head">
    <span class="brand-word">Editor</span>
    {#if inst?.name}<span class="bench-name mono">{inst.name}</span>{/if}
  </div>
  <EditorView ... />
</section>
```

`ColumnControls` adds the standard maximize / archive / move-to-workbench
chip — no Editor-specific buttons. The Linked-agents pills live inside
`ev-left-head` instead.

---

## 3. File Tree

### 3.1 Data flow

The tree is **lazy** — only the root and the explicitly expanded
folders are loaded. Each load is a Tauri `invoke('fs_list_dir', { path })`
returning `Vec<DirEntry>`; `fs.rs` skips `.git` and `.DS_Store` so the
UI never has to filter them.

```ts
// apps/desktop/src/lib/components/editor/FileTree.svelte (excerpt)
const kids = await invoke<Entry[]>('fs_list_dir', { path: it.path });
const ignored = await checkIgnored(kids.map((e) => e.path));
items = kids.map((e) => ({
  name: e.name, path: e.path, is_dir: e.is_dir,
  depth: 0, expanded: false, ignored: ignored.has(e.path)
}));
```

Ignored files come from a single batch call to
`invoke('git_check_ignore', { repo: rootPath, paths })` — much cheaper
than per-row checks. Returned set is stored on each entry as `ignored`.

### 3.2 Decorations

Git status flows from `EditorView.onGitStatusChange` (subscribed to the
result of a `git_status` invoke after mount and on every `fs:changed`).
We collapse the `XY` two-char code to a single rendered char per file
using the priority "index status > worktree status":

| Code | Class           | Meaning            |
|------|-----------------|--------------------|
| `M`  | `tree-git--mod` | Modified           |
| `A`  | `tree-git--add` | Added              |
| `D`  | `tree-git--del` | Deleted            |
| `R`  | `tree-git--ren` | Renamed            |
| `C`  | `tree-git--ren` | Copied             |
| `U`  | `tree-git--conflict` | Unmerged      |
| `??` | `tree-git--new` | Untracked          |

`gitignore`d files render with reduced opacity but still appear (so the
user can see their build output without it disappearing).

### 3.3 No virtual scrolling

The tree is a simple vertical container with `{#each items as it (it.path)}`.
If a folder has 5000 immediate children the user scrolls — we do not
slice the list. This is documented as a deliberate non-goal (§1.3).

The "reveal in tree" function uses `scrollIntoView` on the row's DOM
element rather than a virtual index lookup.

---

## 4. Editor Surface

CodeMirror 6 with the `basicSetup` package, an `EditorState`, and two
`Compartment`s (theme + language) so we can hot-swap when the active
file or theme changes:

```ts
view = new EditorView({
  parent: editorEl,
  state: EditorState.create({
    doc: contents,
    extensions: [
      basicSetup,
      themeCompartment.of(editorThemeExtension(themeState.name)),
      languageCompartment.of(languageFor(p)),
      keymap.of([
        { key: 'Mod-s', run: (v) => { void save(v); return true; } }
      ])
    ]
  })
});
```

Reference: `apps/desktop/src/lib/components/editor/Editor.svelte` ~66.

### 4.1 Theme

`editorThemeExtension(name)` returns `oneDark` for the dark theme and
`[]` (i.e. `basicSetup`'s defaults) for the light theme. Switching
the global app theme triggers `themeCompartment.reconfigure(...)` —
no remount. See `editorTheme.ts:21-24`.

### 4.2 Languages

`languageFor(path)` selects the language extension by extension — the
table lives in `codemirrorLang.ts` and covers TypeScript, JavaScript,
JSON, CSS, HTML, Svelte, Rust, Markdown, Mermaid, DOT, Python, Go, and
a few others. Unknown extensions get the no-op `[]`.

### 4.3 Save model

- **`⌘S` only.** No autosave, no save-on-blur.
- "Dirty" is computed by string-equality with `savedContents`. On
  `Mod-s` we write to disk via `fs_write_file` and reset the marker.
- Switching tab away from a dirty file shows a `confirm()` modal:
  "Save before switching? OK = save, Cancel = discard".
- Closing a dirty tab uses the same prompt.

### 4.4 Diff view

`DiffView.svelte` mounts a CodeMirror `MergeView` in **read-only** mode
(no edit), with `EditorView.lineWrapping` so long diff lines don't
require horizontal scrolling. Used for:

- "Show working-tree diff for `path`" (clicking a modified file in the
  Git panel).
- Future: previewing an "Apply" range from the agent (the route is
  there, the consumer side is in `applyRangeToAgent`).

There is no inline `<EditorView.lineWrapping>` in the main `Editor.svelte`
— code wraps only inside `DiffView`.

---

## 5. Repo-Path Linking

The repo path is the **canonical** identifier for an editor column.
It lives in `sessionsState.editorInstanceState[instanceId].repoPath`
(see `apps/desktop/src/lib/state/sessions.svelte.ts:166-169`).

```ts
editorInstanceState: Record<
  string,
  { repoPath: string; pendingOpenFile?: string | null }
>;
```

`EditorColumn.svelte` keeps a local `repoPath` `$state` mirror and
runs two `$effect`s for *store ↔ local* sync (see :76-113). A
sentinel `lastSyncedFromStore` prevents the two effects from
ping-ponging.

### 5.1 `setEditorRepoPath(value, instanceId?)`

Patches `sessionsState.editorInstanceState[id].repoPath` (creating the
record if absent). Defined in `+page.svelte` ~239 and called from:

- The folder-picker (`pickFolder()` in `EditorView.svelte`).
- `mcp__app__set_editor_repo_path` (see §7).
- `linkActiveSessionToEditor` (when the chat session "owns" a worktree
  whose path differs from the editor's current path).

### 5.2 Persistence

A separate `localStorage` key `woom:editor-state:v1`
(`EDITOR_STATE_STORAGE_KEY`) stores `{ instanceId: { repoPath } }`.
`EditorView.svelte` *also* persists the **last-opened root** under
`woom:editor:root` and the **open tabs** under
`woom:editor:tabs`, used as a fallback when the column has no
prior repo path on first mount.

Cursor / selection positions inside files are **not** persisted — open
the same tab twice and you get the top of the file.

---

## 6. Agent Linking

A single editor column can be linked to **N** agent sessions; one
session can be linked to one editor instance. Bookkeeping:

- `ClaudeSession.linkedToEditor: boolean`
- `ClaudeSession.linkedToEditorInstanceId: string | null`
- `ClaudeSession.columnInstanceId: string | null` — which agent column
  hosts the active session

Establishment paths:

| Trigger                              | Function                     | Location                                |
|--------------------------------------|------------------------------|-----------------------------------------|
| Editor "Link Claude / Cursor"        | `linkEditorToAgent`          | `+page.svelte:1077-1111`                |
| Agent "Link editor"                  | `linkActiveSessionToEditor`  | `+page.svelte:1031-1057`                |
| MCP `set_editor_repo_path` follow-up | `applySessionCwd`            | `+page.svelte:2084-2104`, `sessionCwd.ts` |

### 6.1 cwd ownership rules

When the *agent* is in an active worktree (`worktreePath` set) we treat
the worktree path as authoritative — the editor *follows* the worktree
when linking. Otherwise the editor's repo path becomes the new cwd:

```ts
function linkActiveSessionToEditor(editorInstanceId: string) {
  const editorPath = sessionsState.editorInstanceState[editorInstanceId]?.repoPath ?? '';
  const aiWorktree = activeSession?.worktreePath;
  const ownedCwd = activeSession?.cwd;
  const ownedPath = aiWorktree || ownedCwd;
  if (ownedPath && ownedPath !== editorPath) {
    setEditorRepoPath(ownedPath, editorInstanceId);
  }
  updateSession(activeSession.id, {
    linkedToEditor: true,
    linkedToEditorInstanceId: editorInstanceId,
    cwd: ownedPath || editorPath || null
  });
}
```

For the inverse direction (editor changes path), `applySessionCwd`
rotates the linked Claude session's `claudeUuid` (Claude CLI scopes
conversations by project; resuming an old uuid in a new project fails),
clears `claudeResumable`, and writes a `cwdSwitchRecap` line so the
agent's next prompt makes the change visible (see `services/sessionCwd.ts`).

### 6.2 Live reload of edits

The editor watches the open repo via Tauri:

```ts
watchUnlisten = await listen<{ path: string; kind: string }>('fs:changed', (e) => {
  const p = e.payload.path;
  if (p === activePath && !dirtyByPath[activePath] && editor) {
    void editor.reload();
  }
  scheduleGitStatus(250);
});
```

`fs_watch_start(path)` is invoked on `setRoot`. A debounced
(`scheduleGitStatus(250)`) `git_status` re-runs after every change so
the tree decorations stay fresh.

---

## 7. MCP Tool Surface

The Editor is driven from the agent side via two `mcp__app__*` tools:

### 7.1 `mcp__app__set_editor_repo_path`

```jsonc
{
  "repo_path": "/abs/path/to/repo",
  // pick the editor instance:
  "instance_name": "Sagrada-Familia",   // art-name
  "instance_id": "ed-…"                 // alternative
}
```

Effect, in `+page.svelte:2084-2104`:

1. Find the matching editor instance.
2. Switch top-level view to `workbench` if not already.
3. `setEditorRepoPath(repoPath, editor.id)`.
4. For every linked session, `applySessionCwd(s.id, repoPath, { breakLink: false })`.
5. `scrollInstanceIntoView(editor.id)` so the user sees the change.

### 7.2 `mcp__app__set_agent_cwd`

Mirrors `set_editor_repo_path` but targets the agent column directly,
without changing the editor — `applySessionCwd(s.id, repoPath, { breakLink: true })`.
"Break link" because the agent moved away from the editor's path on its
own; staying linked would re-snap them on the next editor change.

### 7.3 What the editor does **not** expose

There is no `editor.read_file` or `editor.write_file` MCP tool — those
are routed through the agent's own filesystem tools (Claude Code's
`Read` / `Write` / `Edit`, Cursor's equivalent). The editor is a *view*
on disk state, not a programmable surface.

---

## 8. Drag-and-Drop

### 8.1 Source — file tree

Each tree row is `draggable="true"`:

```ts
ondragstart={(e) => {
  const payload = { path: it.path, isDir: it.is_dir, name: it.name };
  setDragPayload({ source: 'file', ...payload });
  e.dataTransfer.setData('application/x-woom-file', JSON.stringify(payload));
  e.dataTransfer.setData('text/plain', it.path);
}}
```

The drop targets are:

- **Agent columns** — `onAgentDrop` ingests it as a `@<rel-path>` mention
  pinned to the chat composer.
- **Canvas columns** — produces a `file-card` shape with `repoRoot`,
  `relPath`, `isDir`. See `docs/CANVAS.md §5.4 (file-card)`.

### 8.2 Target — the editor column itself

The editor column is **not** a drop target in v1. Dropping a file path
onto it does nothing. The agent column ingests files; the canvas
ingests cards; the editor opens by tree click or by `pendingOpenFile`
slot on `editorInstanceState[id]`.

---

## 9. "Apply to <agent>"

Selecting N lines in `Editor.svelte` makes a floating button cluster
appear listing the linked agents. Clicking one calls:

```ts
applyRangeToAgent({
  sessionId: btn.sessionId,
  agentInstanceId: btn.agentInstanceId,
  filePath: activePath,
  startLine: selection.startLine,
  endLine: selection.endLine
});
```

Internally (`apps/desktop/src/lib/services/applyToAgent.ts`):

1. `attachLineRangeMention(sessionId, filePath, startLine, endLine)`
   builds a `@path:Lstart-Lend` token, attaches a `Mention` of `kind: 'file-range'`
   to the session, returns the token to display.
2. `setActiveSessionInColumn(agentInstanceId, sessionId)` switches the
   target column to the right session.
3. `scrollInstanceIntoView(agentInstanceId)` brings it into view.

The agent then sees the file slice in its system prompt as part of the
"Referenced items" block (see `docs/AGENTS.md §8 — Prompt input`).

---

## 10. Live Indicators

| Indicator              | Source                                           | Visual                        |
|------------------------|--------------------------------------------------|-------------------------------|
| Tab dirty dot          | `dirtyByPath[path]`                              | `.ev-tab-dot` (orange)        |
| Tree git status letter | `git_status` → `gitStatusByPath[path]`           | `.tree-git--{mod,add,…}`      |
| Linked agent pill      | `linkedAgents` derivation in `EditorColumn`      | `.ev-link-pill`                |
| Editor error           | `error` `$state` set on `fs_read_file` / parse fail | `.ed-error` red banner       |
| Tree error             | `error` from `fs_list_dir`                       | `.tree-state.tree-error`      |
| Loading                | `loading` from initial root expand               | `.tree-state` with "Loading…" |

A "watched-by-agent" indicator for files actively edited by Claude does
**not** exist; we'd see it in the diff dot anyway.

---

## 11. Empty / Error States

- **No repo path yet:** `EditorView` renders the
  `ev-empty` card — "Open a repository", click → folder picker.
- **Repo path stale (folder moved/deleted):** `fs_path_exists(rootToLoad)`
  on mount; if false we silently fall back to the empty state and clear
  the persisted root.
- **Read failure:** `Editor.svelte` shows a single red `.ed-error` line
  with the IPC error text.
- **List failure:** `FileTree.svelte` shows `.tree-error` with retry
  via re-expanding the parent.

---

## 12. Keyboard

| Key       | Scope                | Action                        |
|-----------|----------------------|-------------------------------|
| `⌘S` / `Ctrl+S` | CodeMirror focus | Save active buffer            |
| `⌘K` / `Ctrl+K` | global           | Toggle Command Palette        |
| `Esc`     | global               | Close palette / restore maximize |

The `j` / `k` inbox-navigation keys at the workbench level are blocked
when focus is in a typing target (`isTypingTarget()` in `+page.svelte:3398`),
so editing inside CodeMirror never triggers them.

There is no built-in Find dialog override beyond `basicSetup`'s default
(`⌘F` opens CodeMirror's panel).

---

## 13. Persistence Summary

| Key                                | Owner                | Shape                                          |
|------------------------------------|----------------------|------------------------------------------------|
| `woom:editor:root`            | `EditorView.svelte`  | `string` (last open repo)                      |
| `woom:editor:tabs`            | `EditorView.svelte`  | `string[]` (open paths)                        |
| `woom:editor:sidebar-tab`     | `EditorView.svelte`  | `'tree' \| 'git'`                              |
| `woom:editor-state:v1`        | `sessions.svelte.ts` | `Record<instanceId, { repoPath }>`             |
| `woom:editor-main` (splitter) | `Splitter`           | `number` (px)                                  |

Worktrees, sessions, and chat history live elsewhere — see
`docs/AGENTS.md §13`.

---

## 14. Tauri IPC Surface

Names the editor module calls into the Rust backend:

```text
fs_read_file(path)              -> string
fs_write_file(path, contents)   -> ()
fs_list_dir(path)               -> Vec<DirEntry>
fs_path_exists(path)            -> bool
fs_watch_start(path)            -> ()
fs_watch_stop(path)             -> ()
git_repo_root(path)             -> string | null
git_status(repo)                -> Vec<FileStatus>
git_check_ignore(repo, paths)   -> Vec<string>
git_show(repo, revision, path)  -> string
```

`GitPanel.svelte` adds another ~15 git commands (branches, commit, pull,
push, log, create-PR…). They're enumerated in `apps/desktop/src-tauri/src/lib.rs`
under the `tauri::generate_handler!` block.

Events: `fs:changed` is emitted by `apps/desktop/src-tauri/src/watch.rs`
with payload `{ path, kind }`. See §6.2.

---

## 15. Open Questions / TODOs

1. **Cursor position persistence** is missing. Re-opening a tab snaps
   to line 1. Trivially backed by `localStorage` once we decide on a
   key shape (`woom:editor:cursors:v1` keyed by repoPath + relPath).
2. **No file-level rename / delete UI.** Easy `fs.rs` additions, but
   the agent-driven workflow rarely needs it. Re-evaluate if real
   usage shows otherwise.
3. **No multi-buffer split.** Solving this is a UI question, not a
   data one — we already store tabs[] independent of activePath.
4. **Big-tree perf.** Folders with >5000 immediate entries scroll-lag.
   Plug a tiny virtual scroller (`@tanstack/virtual`?) when triggered.
5. **No "Apply to agent" preview.** The line-range becomes a mention
   silently; users have asked for a confirmation toast quoting the
   first / last 3 lines.
6. **No Find-in-files / project-wide search.** Currently delegated to
   the agent ("grep this for me"). Could ship `rg` integration if it
   becomes a habit.
7. **`editor*.ts` services:** there's no `apps/desktop/src/lib/state/editor.svelte.ts`
   — editor state is fragmented between `EditorView` local state and
   `sessionsState.editorInstanceState`. Consolidating into one
   `editor.svelte.ts` would shake out the sentinel-effect awkwardness.

---

## 16. Glossary

- **Repo path** — the canonical absolute path of the open folder for
  this editor column. Lives on `editorInstanceState[id].repoPath`.
- **Linked agent** — a `ClaudeSession` whose
  `linkedToEditorInstanceId === thisInstanceId`. Its cwd follows.
- **Worktree** — an agent-owned `git worktree` (separate folder); when
  set, takes priority over the editor's repoPath for cwd resolution.
- **Apply to agent** — sending a precise line-range mention from the
  editor selection to a chosen agent column.
- **`fs:changed`** — Tauri event emitted by the Rust watcher; drives
  buffer reload and tree decoration refresh.
- **Sidebar tab** — `'tree' | 'git'`, persisted under
  `woom:editor:sidebar-tab`.
