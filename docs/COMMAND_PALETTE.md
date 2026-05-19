# Woom — Command Palette Specification

**Version:** 0.1
**Last updated:** 2026-04-29
**Status:** describes shipping behaviour. The palette is a single
component, `apps/desktop/src/lib/components/ui/CommandPalette.svelte`,
toggled by `⌘K` / `Ctrl+K` from anywhere. It indexes views,
workbenches, columns, repos, boards, projects, tickets, Sentry
issues, GitHub items — but **not** message bodies inside agent chats
and **not** raw MCP tool catalogs.

> The palette is the global "where do I want to go?" prompt.
> It's substring search across known structured objects, not full-text
> search across content. The use cases are concrete: "open the
> GitHub-Woom column", "jump to PROJ-1234", "show me the Sentry
> issues for billing-frontend". Power-user keyboard navigation; modal
> backdrop closes on Escape or click-outside.

---

## 1. Vision & Non-Goals

### 1.1 Vision

A single keyboard shortcut to navigate any column, slide-over, or
top-level view. Search ranks by section first, substring match
second; sections are pre-ordered by usefulness, so a fuzzy match
on "issue" doesn't bury the user's recent focus on a specific
column.

### 1.2 Goals (v1, shipping)

1. ⌘K toggle, Esc / click-outside close.
2. Substring search (case-insensitive) across:
   - Top-level views (`workbench`, GitHub tab, Jira tab, Sentry tab,
     Rules, Connections, Settings).
   - Workbenches (by name).
   - Editor instances (by repo path / linked session / column name).
   - Other column instances (by kind + name + linked context).
   - GitHub repos pulled from the loaded list.
   - Jira boards and projects.
   - Sentry projects.
   - GitHub items aggregated from every column's loaded inbox.
   - Jira issues from every column's inbox + Jira tab cache.
   - Sentry issues from every column + Sentry tab cache.
3. Six results per section ceiling, fixed section order.
4. Arrow keys to navigate, Enter to pick, hover to highlight.

### 1.3 Non-Goals (v1)

- **Fuzzy / typo-tolerant search.** Substring only.
- **Search inside chat history.** Sessions are not indexed.
- **MCP tool catalog** as palette items. Agents call those, users
  don't.
- **Custom commands / palette extensions.**
- **History of recent picks** to bias ranking.
- **Multi-step "command + argument" chains** (à la VS Code's "create
  file: name?").
- **Inline preview pane** on hover.

---

## 2. Trigger and Scope

```ts
// apps/desktop/src/routes/+page.svelte:3356-3360
function onKey(e: KeyboardEvent) {
  if ((e.metaKey || e.ctrlKey) && e.key === 'k') {
    e.preventDefault();
    paletteOpen = !paletteOpen;
  }
  ...
}
```

The shortcut works regardless of `view`. It also opens by clicking the
🔎 icon in the pill bar (`+page.svelte:3969-3971`).

The component receives `setView` as a callback because `view` is local
state in `+page.svelte`:

```ts
// apps/desktop/src/lib/components/ui/CommandPalette.svelte:34-37
interface Props {
  open: boolean;
  setView: (v: View) => void;
  onClose: () => void;
}
```

---

## 3. Item Model

```ts
// CommandPalette.svelte:45-53 (paraphrased)
type Result = {
  id: string;
  section: string;          // "Views" | "Workbenches" | "Editors" | ...
  title: string;            // primary label
  subtitle?: string;        // secondary line under title
  badge?: string;           // small pill on the right (kind chip)
  pick: () => void;         // executed on Enter / click
};
```

Each row renders title + optional subtitle + optional badge.

There is no `kind: 'action'` vs `kind: 'navigation'` distinction —
every result is a function call. Mutating actions like "Create new
workbench" are not represented in the palette today; they live on
explicit buttons.

---

## 4. Result Sources

`$derived.by` block in `CommandPalette.svelte:102-385` builds the
list. Sections are pushed in this order:

| Section            | Source                                                           |
|--------------------|------------------------------------------------------------------|
| Views              | hard-coded `VIEWS = ['workbench', 'githubTab', 'jiraTab', 'sentryTab', 'rules', 'connections', 'settings']` |
| Workbenches        | `layoutState.workbenches`                                        |
| Editors            | every editor instance, with `repoPath`, linked session info       |
| Columns            | every non-editor instance, with kind + name + linked context     |
| GitHub repos       | `inboxState.githubRepos` (from `github_list_repos` cache)        |
| Jira boards        | `jiraTabState.boards`                                            |
| Jira projects      | `jiraTabState.projects`                                          |
| Sentry projects    | `inboxState.sentryProjects`                                      |
| GitHub items       | merged from every column's `itemsByInstance`                     |
| Jira issues        | column inboxes + Jira tab cache                                   |
| Sentry issues      | column inboxes + Sentry tab cache                                 |

Cross-section dedup: items are keyed by their source `id`, so the same
PR being in two columns shows once.

### 4.1 What's not in the palette

- Agent sessions / chat tabs — accessed via the column's tab strip.
- Canvases — opened from the canvas column header.
- Files — Editor's tree is the source of truth.
- MCP tools — agent-only.

---

## 5. Search and Ranking

```ts
// CommandPalette.svelte:61-65
function matches(query: string, ...fields: string[]): boolean {
  if (!query) return true;
  const q = query.toLowerCase();
  return fields.some((f) => f.toLowerCase().includes(q));
}
```

Per-section limit:

```ts
// CommandPalette.svelte:100-105
const LIMIT_PER_SECTION = 6;
```

Section order is fixed by the order of `push` in the derivation. There
is no relevance-based ranking; the user trusts the section ordering
("Views first, content last").

This means a query of `"forge"` in a workspace with a "Woom"
workbench, the "forge" GitHub repo, multiple "forge" PRs, and several
Jira tickets in projects starting with "FORGE-" will return rows
ordered:

1. Workbench "Woom" (if any).
2. Editor instances on `forge`.
3. Non-editor columns linked to `forge`.
4. GitHub repo `forge`.
5. Jira boards / projects / issues with `forge` in summary.
6. Sentry projects / issues.

Each capped at six.

---

## 6. Keyboard Navigation

```ts
// CommandPalette.svelte:400-418
function onKey(e: KeyboardEvent) {
  if (e.key === 'Escape')      { close(); return; }
  if (e.key === 'ArrowDown')   { selectedIdx = Math.min(selectedIdx + 1, results.length - 1); }
  if (e.key === 'ArrowUp')     { selectedIdx = Math.max(selectedIdx - 1, 0); }
  if (e.key === 'Enter')       { results[selectedIdx]?.pick(); }
}
```

`selectedIdx` resets to 0 on every `query` change. Hover updates it
too:

```svelte
<button
  class="cp-row"
  class:selected={i === selectedIdx}
  onmouseenter={() => selectedIdx = i}
  onclick={r.pick}
>
```

The list is one flat array — section labels are non-interactive
headers, navigation skips them.

---

## 7. Pick Actions

Each `pick()` is a closure that does the right thing for its section:

| Section            | Effect                                                            |
|--------------------|-------------------------------------------------------------------|
| Views              | `setView(viewId)`                                                 |
| Workbenches        | `setActiveWorkbench(id)`                                          |
| Editors / Columns  | `goToInstance(instanceId, workbenchId)` — switches workbench if needed, scrolls into view |
| GitHub repos       | `setView('githubTab')` + `setGithubTabRepo(owner, repo)`          |
| Jira boards        | `setView('jiraTab')` + `setJiraTabBoard(boardId)`                 |
| Jira projects      | `setView('jiraTab')` + `setJiraTabProject(projectKey)`            |
| Sentry projects    | `setView('sentryTab')` + filter by project                        |
| GitHub items       | `inboxState.focusItem = item` (opens slide-over)                  |
| Jira issues        | `inboxState.jiraFocusKey = item.key` (opens slide-over)           |
| Sentry issues      | `inboxState.sentryFocusId = item.id` (opens slide-over)           |

`pick()` always calls `close()` at the end so the palette dismisses.

---

## 8. UI

```
╔════════════════════════════════════════════════════════════════╗
║  ⌕ search columns, repos, tickets…                          [esc]║
╠════════════════════════════════════════════════════════════════╣
║                                                                  ║
║  Views                                                           ║
║  ▶ Workbench                                                     ║
║    GitHub                                                        ║
║    Jira                                                          ║
║                                                                  ║
║  Workbenches                                                     ║
║    Main                                                          ║
║    Auth                                                          ║
║                                                                  ║
║  Editors                                                         ║
║    Sagrada-Familia · ~/Repos/forge                               ║
║                                                                  ║
║  GitHub items                                                    ║
║    forge#4123  fix(canvas): dblclick edit                  PR    ║
║    …                                                             ║
╚════════════════════════════════════════════════════════════════╝
```

Backdrop is a translucent overlay that absorbs clicks (closing the
palette).

---

## 9. Loading / Empty / Error States

The palette is **non-blocking** — every section reads from caches
already populated by other parts of the app. If `inboxState.githubRepos`
isn't loaded yet, the GitHub repos section is empty. We don't trigger
loads from the palette to avoid surprising side effects.

If `query.trim() === ''`, every section shows up to six items by recency
(workbenches in store order, etc.). If a section has zero results
matching the query, its header is hidden.

If everything's empty: a single "No results" row.

---

## 10. Open TODOs

1. No fuzzy / typo-tolerant search. A misspelled query = empty result.
2. No relevance ranking; section order is the only signal.
3. No MRU bias.
4. `forge`-style queries that match a lot will be capped at 6 per
   section without telling the user there are more — a "+N more"
   footer per section would help.
5. No "create" actions: "create new workbench", "open new GitHub
   column", etc. would feel natural here.
6. No agent commands (`/compact`, `/clear`) yet.
7. No file search inside the palette — Editor handles its own tree
   for now.
8. No keyboard shortcut to jump between sections (`⌘↓` / `⌘↑` would be
   nice).

---

## 11. Glossary

- **Result** — single rendered row in the palette.
- **Section** — header grouping (`Views`, `Workbenches`, …).
- **Pick** — the closure executed on Enter / click.
- **`setView`** — top-level view setter passed in as a prop.
- **`goToInstance`** — central helper that switches workbench tab if
  needed and scrolls the column into view (see `docs/WORKBENCH.md
  §A.6`).
