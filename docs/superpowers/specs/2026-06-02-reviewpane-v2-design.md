# ReviewPane v2 — compact navigator, editor is the diff machine

Date: 2026-06-02
Component: `apps/desktop/src/lib/components/editor/ReviewPane.svelte`

## Problem

The current ReviewPane renders a full inline diff inside every edit card
(expandable), with four always-visible text buttons (Open / Refine /
Revert / Keep) per card. Result: visually noisy, vertically long, and it
ships a *second* hand-rolled LCS diff (`computeDiffRows` + collapse
heuristics) vendored from `ChatThread.svelte` — duplicate of the diff
engine that `inlineHunks.ts` already drives in the editor buffer. The pane
is 919 lines and the diff dominates the screen.

## Goal

Turn the pane into a compact **master** list and let the **editor buffer
be the single detail/diff surface** (the inline overlay built in 0.3.0:
green add-wash, struck-through ghost removes, Tab/Esc per hunk). One diff
machine for the whole editor.

## Design

### List (master)

- One **row per edit**, single line:
  `[agent badge] relPath · +N −M · ●status`.
- **File grouping** retained: a collapsible file header with per-file
  `+N −M` totals; rows nest under their file.
- Top bar unchanged: `N edits` summary + `Keep all` / `Revert all`.

### Detail = the editor

- Selecting a row (j/k or click) calls `requestEditorOpenFile(instanceId,
  filePath)` and scrolls to the edit's first hunk. The inline overlay then
  renders the diff in the buffer. **No diff is rendered inside the pane.**
- Resolution stays two-way and already-synced: editor Tab/Esc per hunk OR
  the row's keep/revert both flip the `EditEvent` status via
  `updateEditEvent`; the list is `$derived` off `getPendingEditEvents`, so
  it updates reactively from either path.

### Row actions

- Row is clean by default. On **hover / selected**, reveal compact icon
  buttons: `✓` Keep, `↩` Revert (→ Restore for delete/create), `…` Refine.
- Keyboard always live (unchanged handlers): `a` keep, `r` revert,
  `e` refine, `o` / `Enter` open. Footer hint strip stays.

### Status indicator

- Colored dot: `applied` = ochre, `kept` = green, `reverted` = muted/grey,
  `error` = red (with the existing `note` as tooltip). `loading` = subtle
  pulse.

## Removals

- Inline diff render block in the template.
- `computeDiffRows`, the collapse helper, `DIFF_LINE_CAP`, `expandedKeys`,
  `toggleExpanded`, `DiffRow` type — the whole vendored LCS path.
- `+N −M` stats now come from `computeHunks(oldText, newText)` in
  `inlineHunks.ts` (sum `added.length` / `removed.length` across hunks),
  so the pane no longer carries its own diff algorithm.

## Kept as-is

`openAt`, `keepRow`, `revertRow`, `refineRow`, `onKeepAll`, `onRevertAll`,
selection repair `$effect`, `selectIndex` + `data-row-key` scroll,
`busyKeys` / `bulkBusy`, `rowCount` badge export, `relTo`.

## Non-goals

- Cross-hunk navigation inside the editor (separate 0.3.x item).
- Retiring the Cursor solo (separate).
- Multi-edit-per-file overlay coordination (P4, already deferred).

## Risk / sizing

~919 → ~450 lines. Behavior-preserving for actions; only the diff surface
moves from pane to buffer. Scroll-to-hunk needs the editor to expose a
"reveal line" path — `goToLine` already exists in `Editor.svelte`; wire the
edit's first-hunk line through `requestEditorOpenFile` (extend signal with
an optional line, or fire `woom:editor:goto` after open).
