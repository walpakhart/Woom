# Forgehold — Workbench Shell Specification

**Version:** 0.1
**Last updated:** 2026-04-29
**Status:** describes shipping behaviour. The workbench is the
top-level surface of every Forgehold view that isn't a settings page.

> A workbench is **a named layout of columns**. The user has many
> workbenches (tabs at the top), each with its own set of column
> instances. A column instance is a live mount of a `PanelKind`
> (github / jira / sentry / claude / cursor / editor / canvas) with a
> width, a name, and per-instance state living inside the relevant
> store. The shell owns drag-drop between columns, resize with snap,
> archive/restore, maximize-overlay, and persistence; everything below
> the shell — the contents of a column, the agent's chat, the canvas
> — is owned by the column component itself.

---

## 1. Vision & Non-Goals

### 1.1 Vision

The workbench replaces the file-based "open project / open tab"
metaphor with a **per-task layout** model. The user's brain organises
work by *what I'm doing right now* — "I'm reviewing a PR while
chatting with Claude in repo X" — and that's the natural unit. A
workbench is named, switchable, and savable; closing the app and
re-opening it lands you exactly where you were.

### 1.2 Goals (v1, shipping)

1. Multiple workbenches (tabs across the top).
2. Per-workbench list of `PanelInstance`s with persisted ordering and
   widths.
3. Pill bar with per-`PanelKind` group (active count + archived count).
4. Drag-and-drop:
   - column → other workbench tab (move column),
   - inbox item → agent column (mention),
   - inbox item → agent pill (spring-loaded → instance picker),
   - inbox item → canvas surface (live card).
5. Resize handle with snap-to-equal-neighbour, snap-to-viewport-fraction,
   snap flash visual.
6. **Maximize** any column to fill the bench overlay-style; restore
   with `Esc` or the same button.
7. **Archive** preserves the column's state, accessible via the
   pill's chevron menu; un-archive places it back at its original
   index.
8. Persistence in `localStorage` with v1 → v3 migration.
9. Empty state nudging the user to connect a source.

### 1.3 Non-Goals (v1)

- **Saving / loading workbenches as files.** They live in localStorage.
- **Sharing a workbench across users.** Single-player.
- **Animated column reorder.** Static drop only; we do an instant
  reorder, no FLIP animation in v1.
- **Floating / modal columns.** All columns dock left-to-right inline.
- **Per-column z-stacking.** Columns are always visually equal except
  for maximize.
- **Auto-arrange / "fit to window"** beyond the resize snaps.

---

## 2. Concept Map

| Term                | Defined in                                 | Meaning                                                      |
|---------------------|--------------------------------------------|--------------------------------------------------------------|
| **Workbench**       | `apps/desktop/src/lib/types.ts:24-28`      | `{ id, name, instances: PanelInstance[] }`                   |
| **PanelInstance**   | `apps/desktop/src/lib/types.ts:17-22`      | A live column: `{ id, kind, width, name }`                   |
| **PanelKind**       | `apps/desktop/src/lib/types.ts:7`          | `'github' \| 'jira' \| 'sentry' \| 'claude' \| 'cursor' \| 'editor' \| 'canvas'` |
| **Pill**            | `+page.svelte:3840-3972`                   | Group button in `.wb-bar` per kind                            |
| **Snap-flash**      | `layoutState.snapFlashInstanceId`          | Brief outline pulse on a resize handle when a snap fires     |
| **Maximize**        | `layoutState.maximizedInstanceId`          | One column overlays the bench area                            |
| **Archive**         | `layoutState.archivedInstances[]`          | Detached but state-preserving column slot                    |
| **`name`**          | per `PanelInstance`                        | Globally-unique art name (e.g. "Sagrada-Familia")             |

---

## 3. PanelKind: Catalogue, Defaults, Multi-instance

```ts
// apps/desktop/src/lib/state/layout.svelte.ts:10-18
export const DEFAULT_PANEL_ORDER: PanelKind[] =
  ['github', 'jira', 'sentry', 'claude', 'cursor', 'editor', 'canvas'];
export const DEFAULT_PANEL_WIDTHS: Record<PanelKind, number> = {
  github: 420,
  jira:   420,
  sentry: 440,
  claude: 520,
  cursor: 520,
  editor: 720,
  canvas: 720
};
```

### 3.1 Multi-instance support

Every kind allows multiple instances per workbench. The per-instance
filter / state stores are keyed by `instanceId`, so two GitHub columns
side-by-side carry independent filters; two Editor columns carry
independent open repos and tabs.

`addPanelInstance(kind)` always creates a fresh instance — there is no
"singleton" pathway despite stale comments in `forgehold-app`'s tool
descriptions.

### 3.2 Visibility

A column may exist in `workbench.instances` but not render — e.g. a
`github` column when the user is not connected to GitHub. The check:
`isInstanceVisible(inst)` (`layout.svelte.ts:556-564`). Snap math and
"navigate to first column of kind" both respect visibility.

---

## 4. `layoutState`

Single Svelte 5 `$state` blob (`apps/desktop/src/lib/state/layout.svelte.ts:108-132`):

```ts
export const layoutState = $state<{
  workbenches: Workbench[];
  activeWorkbenchId: string;
  snapFlashInstanceId: string | null;
  archivedInstances: ArchivedInstance[];
  maximizedInstanceId: string | null;  // ephemeral; not persisted
}>({ ... });
```

`ArchivedInstance` (101-106):

```ts
{
  inst: PanelInstance;
  originalWorkbenchId: string;
  originalIndex: number;       // for "put it back where it was"
  archivedAt: number;
}
```

### 4.1 Exported functions

In rough order they appear:

```text
activeWorkbench(), activeInstances(), findInstanceAnywhere(id)
persistPanelState(), restorePanelState()
registerInstanceRemovedHook(cb)
closePanelById(id), archiveInstance(id), unarchiveInstance(id, workbenchId)
listArchivedOfKind(kind)
movePanelById(id, fromIdx, toIdx)
moveInstanceToWorkbench(id, targetWorkbenchId)
addPanelInstance(kind) -> id
firstInstanceOfKind(kind), listInstancesOfKind(kind)
isInstanceVisible(inst)
snapWidth(rawNext, instanceId, vpw, neighbours)
flashSnap(id), startResizeById(id, e)
goToInstance(id, workbenchId), scrollInstanceIntoView(id), scrollKindIntoView(kind)
addWorkbench(name?), removeWorkbench(id), renameWorkbench(id, name)
setActiveWorkbench(id)
toggleMaximize(id), restoreMaximized()
```

Plus constants `DEFAULT_PANEL_ORDER`, `DEFAULT_PANEL_WIDTHS`,
`SNAP_THRESHOLD = 18`.

---

## 5. Drag and Drop

### 5.1 Payload model

```ts
// apps/desktop/src/lib/state/drag.svelte.ts:15-18
export type DragPayload =
  | { source: 'github'; item: InboxItem }
  | { source: 'jira'; item: JiraItem }
  | { source: 'sentry'; item: SentryIssue }
  | { source: 'file'; path: string; isDir: boolean; name: string }
  | { source: 'chat-message'; sessionId: string; messageIndex: number };
```

We also use raw `application/x-forgehold-column` (column move) and
`application/x-forgehold-file` (file path) MIME types for redundancy
across WKWebView quirks.

### 5.2 Chip preview

`apps/desktop/src/lib/dragImage.ts`:

```ts
export type DragChipKind =
  | 'file' | 'dir'
  | 'jira' | 'github' | 'sentry'
  | 'cursor' | 'claude'
  | 'editor' | 'canvas';

export function attachDragChip(e: DragEvent, kind: DragChipKind, label: string): void;
```

Renders an off-screen chip element, calls `e.dataTransfer.setDragImage(el, ...)`,
removes the element on next tick.

### 5.3 Drop targets

| Target                 | Accepts                                             | Result                                                       |
|------------------------|-----------------------------------------------------|--------------------------------------------------------------|
| Agent column body      | `github / jira / sentry / file / chat-message`      | Adds a `Mention` to the active session's composer            |
| Pill (claude / cursor) | Same as above                                        | Spring-loaded menu of instances; drop on row → mention       |
| Pill (other kinds)     | Nothing — `pillCanAccept` returns false             | Visually rejects the hover                                   |
| Workbench tab          | Column move (`application/x-forgehold-column`)       | `moveInstanceToWorkbench(...)` to that workbench             |
| Canvas surface         | inbox payloads, file payloads, OS files, OS images   | Live card / file card / image shape (see `CANVAS.md §9`)     |

### 5.4 Snap flash

When `snapWidth(...)` decides the dragged width should snap to an
anchor (equal neighbour, viewport fraction, grid step), it calls
`flashSnap(instanceId)`. This sets `layoutState.snapFlashInstanceId`,
the column's `.wb-col-resize` adds the `snap-flash` class, CSS plays
the pulse animation, then a `setTimeout` clears it.

---

## 6. Resize

```svelte
<div
  class="wb-col-resize"
  class:snap-flash={layoutState.snapFlashInstanceId === instanceId}
  role="separator"
  aria-orientation="vertical"
  onpointerdown={(e) => startResizeById(instanceId, e)}
></div>
```

Behaviour (`layout.svelte.ts:616-674`):

- Left mouse button only.
- `setPointerCapture` for the duration.
- Each `pointermove`: compute `rawNext` (clamped 280..1600), pass to
  `snapWidth`. If the snapped value differs from `rawNext`, `flashSnap`.
- Auto-scroll the bench horizontally when the pointer is near the
  edge of the viewport.
- On `pointerup`: `persistPanelState()`.

There is **no** double-click-to-reset on the resize handle.

CSS handles (`apps/desktop/src/app.css:293-314`): hitbox 6 px,
`::before` line on hover + active, accent flash for `.snap-flash`.

---

## 7. Maximize

State machine: `maximizedInstanceId: string | null`.

```ts
function toggleMaximize(id) {
  layoutState.maximizedInstanceId =
    layoutState.maximizedInstanceId === id ? null : id;
}
```

CSS in `+page.svelte:4730-4758`:

```css
:global(.wb-columns:has(.wb-column--maximized) > .wb-column:not(.wb-column--maximized)) {
  visibility: hidden;
  pointer-events: none;
}
:global(.wb-column.wb-column--maximized) {
  position: absolute !important;
  inset: 0 !important;
  z-index: 50 !important;
  flex: 1 1 100% !important;
  width: auto !important;
}
```

The non-maximized columns stay mounted (so their state — chat
streaming, in-flight git diffs — keeps running), they're just
visually hidden.

### 7.1 Auto-clear cases

Maximize state is dropped when:

- The maximized column is `closePanelById(id)`d (392-394).
- It's `archiveInstance`d (416-418).
- Active workbench changes via `setActiveWorkbench(id)` (773-778).
- `goToInstance(target)` runs and `target !== maximizedInstanceId`
  (705-714) — otherwise the user clicks a pill expecting to navigate
  but the existing overlay hides what they wanted.

### 7.2 Keyboard

`Esc` is wired in `+page.svelte:3360-3388`:

```ts
} else if (e.key === 'Escape') {
  if (paletteOpen) paletteOpen = false;
  else if (anyModalOpen()) closeAllModals();
  else if (inboxState.focusItem) closeFocusItem();
  else if (inboxState.jiraFocusKey) inboxState.jiraFocusKey = null;
  else if (inboxState.sentryFocusId) inboxState.sentryFocusId = null;
  else if (layoutState.maximizedInstanceId) restoreMaximized();
}
```

I.e. modals and slide-overs win over maximize — predictable layering.

---

## 8. Archive

`archiveInstance(id)`:

1. Find the workbench and index.
2. Splice the instance out.
3. Push `{ inst, originalWorkbenchId, originalIndex, archivedAt }` to
   `layoutState.archivedInstances`.
4. **Does not** call `onInstanceRemoved` callbacks — the column is
   merely detached, its per-instance state in other stores stays.

`unarchiveInstance(id, workbenchId)`:

1. Find the archive entry.
2. Insert at `originalIndex` if still valid, else push.
3. Remove from `archivedInstances`.

UI: pill chevron menu shows live + archived rows. See
`+page.svelte:3924-3943`.

`closePanelById(id)` is the **destructive** alternative — calls all
`onInstanceRemoved` hooks, used when the user clicks the `✕` on the
column. Per-instance state in `sessionsState.editorInstanceState`,
`canvasState`, etc., gets purged.

---

## 9. Multi-Workbench Tabs

Top strip `.wb-tabs` (`+page.svelte:3788-3838`):

- Active tab styled, click switches via `setActiveWorkbench(id)`.
- Double-click renames inline (commit on Enter, cancel on Esc).
- Drop a column onto a tab → `moveInstanceToWorkbench(...)`.
- "+" button → `addWorkbench()` with art-name auto-pick.
- Right-click on inactive tab → confirm modal to remove it
  (`askRemoveWorkbench`).

`removeWorkbench(id)` calls `onInstanceRemoved` for each instance —
state purge — and reverts to the first remaining workbench.

---

## 10. Pills

`apps/desktop/src/routes/+page.svelte:3840-3972`.

Order in the bar:

1. github (when `connectedGithub`).
2. jira (when `connectedJira`).
3. sentry (when `connectedSentry`).
4. claude (when `connectedClaude`).
5. cursor (when `connectedCursor`).
6. editor (always).
7. canvas (always).

A pill renders:

- The kind glyph.
- A name or count.
- Two badges: **`pill-count`** for live instances on the active
  workbench, **`pill-count--archived`** for archived ones.

Click behaviour (`navToKind`):

1. Prefer the first instance of `kind` on the active workbench.
2. Else first instance globally.
3. Else `addPanelInstance(kind)`.

The chevron opens a dropdown listing all instances of that kind across
all workbenches plus archived ones. Drag a row to focus / move /
restore.

Spring-loaded drag: hovering with a drag payload over a `claude` /
`cursor` pill for `PILL_OPEN_DELAY` ms opens the dropdown; leaving for
`PILL_CLOSE_DELAY` ms closes it.

---

## 11. Drag Chips (column move)

When the user starts a drag on `ColumnControls`'s "move to workbench"
handle:

```ts
attachDragChip(e, kind === 'editor' ? 'file' : kind, `${inst.name}`);
e.dataTransfer.setData('application/x-forgehold-column', JSON.stringify({ instanceId, kind }));
```

`ColumnControls.svelte:112-114, 116`. The chip uses the same chip
gallery as inbox drags so visual language is consistent.

---

## 12. Empty State

If `!anythingConnected && !statusLoading`:

```svelte
<div class="wb-empty">
  <h2>Connect a source</h2>
  <p>Drag a chip to add a column. Hook up GitHub, Jira, or Sentry below.</p>
  <button onclick={() => view = 'connections'}>Open Connections</button>
</div>
```

(`+page.svelte:3772-3779`, paraphrased.)

There is no per-workbench "empty workbench" hint when sources are
connected but instances are zero — the bench just renders blank columns
area; the pill bar remains the obvious add affordance.

---

## 13. Hooks: `registerInstanceRemovedHook`

The hook collects callbacks invoked when `closePanelById(id)` or
`removeWorkbench(...)` removes an instance. Implemented as an array
(was a single-callback setter pre-refactor; the array fix prevented
silent overwrites).

Registered hooks today (in `+page.svelte:424-431`):

```ts
registerInstanceRemovedHook((id) => orphanSessionsForInstance(id));
registerInstanceRemovedHook((id) => dropCanvasInstance(id));
```

Editor instance state in `sessionsState.editorInstanceState[id]` is
purged via `orphanSessionsForInstance`.

---

## 14. Workbench-Level Keyboard

`<svelte:window onkeydown={onKey} />` in `+page.svelte:3333`:

| Key       | Condition                                            | Action                              |
|-----------|------------------------------------------------------|-------------------------------------|
| `⌘K` / `Ctrl+K` | always                                       | Toggle Command Palette              |
| `Esc`     | layered (palette → modals → slide-overs → maximize)  | Close current top-most overlay      |
| `j`       | `view === 'workbench'` and `!anyModalOpen()`         | `moveSelection(+1)` (GitHub inbox)  |
| `k`       | same, no `⌘`                                          | `moveSelection(-1)`                  |

`anyModalOpen()` (`+page.svelte:3397-3413`) returns true while *any*
of the slide-overs / modals / palette are visible; the j/k keys don't
fight against text input or open detail panes.

---

## 15. Persistence

`forgehold:workbenches:v1` (current canonical key) holds:

```jsonc
{
  "workbenches": [
    { "id": "wb-1", "name": "Main",
      "instances": [
        { "id": "gh-…", "kind": "github", "width": 420, "name": "Aurora-Borealis" },
        ...
      ]
    }
  ],
  "activeId": "wb-1",
  "archived": [
    { "inst": { "id": "gh-…", "kind": "github", "width": 420, "name": "..." },
      "originalWorkbenchId": "wb-1", "originalIndex": 0, "archivedAt": 1714... }
  ]
}
```

Older keys are still read for migration:

```text
forgehold:workbench:columns   v1: per-kind boolean
forgehold:workbench:order     v1: kind ordering
forgehold:workbench:widths    v1: kind widths
forgehold:layout:v2           v2: flat instance list
forgehold:workbenches:v1      v3: current
```

`restorePanelState()` (`layout.svelte.ts:209-363`) chains these:

- v1 → `{ Main: instances built from columns + order + widths }`.
- v2 → wrap in a single Main workbench.
- v3 → use as-is, validating `kind` against `DEFAULT_PANEL_ORDER`.

For users who never toggled column visibility, the migration default
for v1→v3 is:

```ts
const defaults: Record<PanelKind, boolean> = {
  github: true, jira: true,  sentry: false,
  claude: true, cursor: false,
  editor: false, canvas: false
};
```

`maximizedInstanceId` and `snapFlashInstanceId` are **not** persisted.

---

## 16. View Switch (workbench / tabs / settings)

The same `+page.svelte` switch decides what to render below the top
bar:

| `view`        | Renders                                            |
|---------------|----------------------------------------------------|
| `workbench`   | The pill bar + columns (the workbench itself)      |
| `githubTab`   | `<GithubTab>` repo browser                         |
| `jiraTab`     | `<JiraTab>` (project browser, board picker)        |
| `sentryTab`   | `<SentryTab>` (org-level error overview)           |
| `rules`       | `<RulesView>` (system prompt templates)            |
| `connections` | `<ConnectionsView>`                                |
| `settings`    | `<SettingsView>`                                   |
| (other)       | "Unknown view" placeholder                         |

Switching view via `setView` is one-call from anywhere. The MCP
`switch_view` tool dispatches into this same state.

---

## 17. Open TODOs

1. Stale comment in `+page.svelte:1299-1301` claims `spawnColumnInstance`
   is a "singleton no-op" — it's not, every call adds a new instance.
2. Stale comment in `forgehold-app`'s `add_workbench_instance`
   description says GitHub / Jira / Sentry are singletons.
3. No double-click-to-reset on resize handle.
4. No FLIP / animated reorder when columns move.
5. The "snap to neighbour width" rule sometimes feels too eager when
   the user's intent is "slightly bigger than X". A pause-then-snap
   tweak would help.
6. Maximize doesn't have a shortcut to toggle (`F` is a candidate; not
   bound).
7. Empty-workbench state (zero instances after archive) doesn't render
   a "drag a chip to start" hint.
8. Migration code paths need a sweep — v1 was last seen in production
   before the workbench refactor, so the v1 branch can be deprecated
   with one more major bump.

---

## 18. Glossary

- **Workbench / bench** — a named layout of columns (a tab).
- **Column / instance** — a single rendered `PanelInstance`.
- **Kind** — `PanelKind` discriminator.
- **Pill** — kind-grouped button in the top bar.
- **Snap** — clamping a resize delta to a "nice" anchor (neighbour
  width, viewport fraction, grid step).
- **Flash** — temporary outline highlight applied by `flashSnap(...)`.
- **Maximize / restore** — overlay state for a single column,
  ephemeral.
- **Archive** — detached column slot, state preserved.
- **Hook** — `registerInstanceRemovedHook(cb)` callback fired on
  destructive removal.
- **Art name** — the `name` field on a `PanelInstance`, e.g.
  "Sagrada-Familia". Globally unique. Used in MCP `instance_name`
  arguments.
