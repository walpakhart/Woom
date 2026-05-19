# Woom — Jira Integration Specification

**Version:** 0.1
**Last updated:** 2026-04-29
**Status:** describes shipping behaviour. Atlassian Cloud only; Server
/ Data Center is not supported. Authentication is workspace + email +
API token. PAT-only by design — Woom does not ship OAuth for any
source.

> The Jira column is the inbox of issues you should be looking at, with
> per-instance filters that translate into JQL the way you'd write it
> by hand. The detail slide-over (`JiraDetailPane`) does the heavier
> lifting: comments, transitions, assignee picker, worklogs, labels,
> markdown-to-ADF for free. Everything goes through Tauri IPC backed
> by the `apps/desktop/src-tauri/src/jira.rs` module — no Atlassian
> SDK, just hand-rolled REST against `/rest/api/3/`.

---

## 1. Vision & Non-Goals

### 1.1 Vision

Jira is a busy place. The column shows the slice that matters today:
"my unresolved issues, on this sprint, in this project". Filters are
structured (project / boards / sprints / status) so the user doesn't
have to remember JQL grammar; complex queries belong in the search
field where the substring matches title + description.

The slide-over is a near-drop-in replacement for the Jira issue page —
description (editable), comments (markdown), transitions (workflow
buttons), assignee, priority, labels, worklogs.

### 1.2 Goals (v1, shipping)

1. Token auth with workspace URL + email; storage in macOS Keychain.
2. Per-column filter set: `projectKey`, `boardIds[]`, `sprintIds[]`,
   `statusName`, `search`. Persisted per `instanceId`.
3. Workspace-global assignee chip (`jiraAssignee` / `jiraAssigneeAny`).
4. JQL builder: `buildJiraJql(filters, assignee, assigneeAny)`.
5. List with optional grouping by project key (when ≥ 2 boards selected).
6. Slide-over `JiraDetailPane` with edit / transition / comment /
   assignee / priority / labels / worklogs.
7. Markdown ⇄ ADF conversion for description and comment bodies.
8. Drag to canvas → `jira-card` shape with `ticketKey` + snapshot.
9. Drag to agent column → `@TICKET-KEY` mention.
10. MCP read + write surface (read, create, update, transition,
    comment, list projects / users / sprints).

### 1.3 Non-Goals (v1)

- **Atlassian Server / DC.** Cloud only.
- **OAuth** with Atlassian. Permanent non-goal — every source uses
  PATs. Token UX is invested in instead:
  rotation reminders, multi-account, diagnostics.
- **Custom fields** beyond the standard set surfaced in `JiraDetail`.
- **Native JQL editor** in the column. Use the global Search if you
  need raw JQL — it's substring-only in v1; raw JQL goes through MCP
  `search`.
- **Subtasks tree** in the column / detail. Surfaced as plain links
  if present.
- **Attachments upload** — viewable, not uploadable.
- **Confluence integration.**
- **Per-project workflows beyond status names.** We don't model the
  status graph; transitions come from the API per issue.

---

## 2. Column Anatomy

```
╭─ Jira column (kind === 'jira') ────────────────────╮
│ ⠿ ⤢ ✕   Woom                  data-kind="jira"│
├─────────────────────────────────────────────────────┤
│ inbox-brand:  Ⓙ Jira  forge-cloud  ⟳   New issue   │
├─────────────────────────────────────────────────────┤
│ search-bar (palette)                            ⌘K  │
│ filter-bar:                                         │
│   [Project ▾] [Boards ▾] [Sprints ▾] [Status ▾] ⟳   │
│ inbox-controls: 14 items                            │
├─────────────────────────────────────────────────────┤
│ DEVOPS                                              │
│   ▦ DEVOPS-414  Re-mount canvas...   IN PROGRESS    │
│ FORGE                                               │
│   ▦ FORGE-92    Edit blocks dblclick   TO DO        │
╰─────────────────────────────────────────────────────╯
```

Width default `420` (`apps/desktop/src/lib/state/layout.svelte.ts:11`).
Multiple Jira columns are valid; each has its own `JiraFilters`. The
**assignee** chip is shared across the whole app via `inboxState`.

The slide-over `JiraDetailPane` is mounted globally:

```svelte
{#if inboxState.jiraFocusKey}
  <div class="slide-over"><div class="slide-panel">
    <JiraDetailPane issueKey={...} {now}
      onClose={() => (inboxState.jiraFocusKey = null)}
      onStatusChange={() => void refreshAllJiraInboxes({ silent: true })} />
  </div></div>
{/if}
```

Naming note: the codebase has **`JiraDetailPane`**, not
`JiraIssuePane` or `JiraSlideover` — those names don't exist.

---

## 3. Filter Model (`JiraFilters`)

```ts
// apps/desktop/src/lib/state/inbox.svelte.ts:70-90
export interface JiraFilters {
  projectKey: string | null;
  boardIds: number[];
  sprintIds: SprintScope[];   // SprintScope = number | 'backlog'
  statusName: string | null;
  search: string;
}
```

### 3.1 What each filter does

- **`projectKey`** — single key or `null` ("All projects").
- **`boardIds`** — multiple boards. With ≥ 2 boards selected, the
  query becomes `project IN (key1, key2, ...)`.
- **`sprintIds`** — only meaningful with **exactly one** board selected.
  Multi-sprint allowed; `'backlog'` is a special token meaning
  `sprint is EMPTY`.
- **`statusName`** — verbatim status name (e.g. `"In Progress"`).
- **`search`** — substring matched against `summary` and `description`.

### 3.2 Global assignee

The assignee chip is **not** per-column — it's `inboxState.jiraAssignee`
(`JiraUserSummary | null`) plus `inboxState.jiraAssigneeAny: boolean`.

- `jiraAssigneeAny === true` → no `assignee` clause.
- `jiraAssigneeAny === false` and `jiraAssignee === null` → defaults to
  `assignee = currentUser()`.
- Otherwise `assignee = "<account_id>"`.

### 3.3 JQL construction

`buildJiraJql(filters, assignee, assigneeAny)` (`inbox.svelte.ts:918-987`):

```text
let parts = ['resolution = Unresolved'];
if (statusName) parts.push(`status = "..."`);
if (assigneeAny) {} else if (assignee) parts.push(`assignee = "<id>"`);
else parts.push('assignee = currentUser()');
// project / sprint / board / search appended per filters
return `${parts.join(' AND ')} ORDER BY updated DESC`;
```

Sprint logic:

- One board, no sprints → `sprint in openSprints("BOARD")`.
- One board, sprints picked → `sprint IN (1234, 5678)` plus
  `sprint is EMPTY` if backlog included.
- ≥ 2 boards → ignore sprint, fall through to project/board scope.

We **don't** expose raw JQL input. Power users use the MCP `search`
tool from the agent.

---

## 4. List Rendering

- **No virtualization.**
- **Grouping** by project key only when ≥ 2 boards selected (the
  column visually splits into `DEVOPS / FORGE / …` headers, sourced
  from the prefix of `issue.key`).

```ts
// apps/desktop/src/lib/components/workbench/JiraColumn.svelte:189-205
const groupedJiraItems = $derived.by(() => {
  if (filters.boardIds.length <= 1) return null;
  const groups = new Map<string, typeof items>();
  for (const item of items) {
    const proj = (item.key.split('-')[0] ?? 'OTHER').toUpperCase();
    ...
  }
  return Array.from(groups.entries()).map(([project, items]) => ({ project, items }));
});
```

- **Card content** per row: `Ⓙ` marker, `KEY`, summary, relative time
  (using `relativeTime(updated)`), status class derived from
  `status_category`, issue type, priority chip when present.
- Status colour mapping uses `status_category` (`new` / `indeterminate` /
  `done`) — mirrors Atlassian's own three-bucket model.

---

## 5. Slide-over (`JiraDetailPane`)

`apps/desktop/src/lib/components/inbox/JiraDetailPane.svelte`.

Loads on mount:

```text
invoke('jira_get_issue_detail', { key: issueKey })
  -> JiraDetail {
       id, key, summary, description, status, priority, …,
       comments, transitions, ...
     }
```

Sections (top to bottom):

1. **Header** — key, summary (editable inline), browser link.
2. **Metadata grid** — status (with transition dropdown), assignee
   (picker), reporter, priority, labels (chip editor).
3. **Description** — editable markdown; on save converted to ADF.
4. **Comments** — list, each with author / time / body (markdown→ADF).
5. **Worklogs** — lazy-loaded section (`jira_list_worklogs`); add /
   delete via `jira_add_worklog`, `jira_delete_worklog`.

There are no comment-thread reactions / @mentions UI in v1 (the API
supports them; UI doesn't render).

### 5.1 Mutation IPC list

| User intent       | IPC                       |
|-------------------|---------------------------|
| Edit summary/desc | `jira_update_issue`       |
| Move workflow     | `jira_transition_issue`   |
| Add comment       | `jira_add_comment`        |
| Set assignee      | `jira_set_assignee`       |
| Set priority      | `jira_set_priority`       |
| Set labels        | `jira_set_labels`         |
| Add worklog       | `jira_add_worklog`        |
| Delete worklog    | `jira_delete_worklog`     |

`jira_create_issue` is also there but used from the **Create Issue**
modal opened via the column's **New issue** button, not the slide-over.

---

## 6. Live Updates

- **Filter debounce:** `updateJiraFilters` triggers a `setTimeout(...300ms)`
  per instance.
- **Polling:** `refreshAllJiraInboxes({ silent: true })` runs inside the
  same 60-second interval as GitHub, but **only when GitHub is
  connected** — the interval is gated on `connectedGithub` in
  `+page.svelte:540-546`. If only Jira is connected, there's no
  background refresh; the user must hit ⟳ or change a filter.
- **No push events:** no `listen(.*jira)` on the frontend.

---

## 7. Authentication

`apps/desktop/src-tauri/src/lib.rs:569-623`:

```rust
async fn jira_connect(workspace, email, token) -> Result<JiraUser> {
  let creds = JiraCredentials {
    workspace: jira::normalize_workspace(&workspace),  // strips trailing slash, scheme normalization
    email: email.trim(),
    token: token.trim(),
  };
  // Validate by calling /rest/api/3/myself
  keychain::set(JIRA_KEY, &payload)?;
}
```

Logical key: `JIRA_KEY = "jira"`. Stored as JSON `{ workspace, email, token }`.
`jira_status` re-validates by calling `fetch_myself` and clears the key
on `InvalidToken`.

The Connect modal collects all three fields (`JiraConnectModal.svelte`).
There is no OAuth path — that's by design.
Disconnect: `jira_disconnect`.

---

## 8. MCP Tools (`user-woom-jira`)

| Tool                    | Required                          | Optional                                                  |
|-------------------------|-----------------------------------|-----------------------------------------------------------|
| `get_issue`             | `key`                             |                                                           |
| `search`                | `jql`                             | `max_results`                                             |
| `add_comment`           | `key`, `body`                     |                                                           |
| `transition_issue`      | `key`, `to`                       | `comment` (added with the transition)                     |
| `create_issue`          | `project_key`, `summary`          | `issue_type` (default `Task`), `description`, `assignee_account_id`, `sprint_id` |
| `update_issue`          | `key`                             | `summary`, `description`, `assignee_account_id`, `sprint_id`, `labels` |
| `list_projects`         |                                   | `query`, `limit`                                          |
| `list_assignable_users` | `project_key`                     | `query`                                                   |
| `list_sprints`          | `project_key`                     | `state` (defaults to active+future; `'all'` for closed)   |

Use-instructions hint: when assigning by a human name, the agent calls
`list_assignable_users(project_key, query: '<name>')` first to resolve
the `accountId`. When picking a sprint, `list_sprints` resolves the
sprint id.

All write tools accept **markdown** in rich-text fields (description,
comment body) and translate to ADF: headings, bullet/ordered lists,
fenced code blocks, links, bold/italic/strike preserved. Conversion
lives in `apps/desktop/src-tauri/src/jira.rs` (search for `markdown_to_adf`).

The `propose_*` flow GitHub has does **not** apply here — Jira writes
execute immediately when the agent calls them. They're cheap and
reversible (a comment can be edited, a transition can be transitioned
back).

---

## 9. Drag-and-Drop

Source: column rows draggable; payload `{ source: 'jira', item }`
(`apps/desktop/src/lib/state/drag.svelte.ts:15-18`).

```ts
e.dataTransfer.setData('text/plain', payload.item.key);
attachDragChip(e, 'jira', `${payload.item.key} · ${payload.item.summary}`);
```

Drop targets:

| Target          | Effect                                                     |
|-----------------|------------------------------------------------------------|
| Agent column    | `@DEVOPS-414` mention with snapshot of summary / status    |
| Agent pill      | Spring-loaded → instance picker → mention                  |
| Canvas surface  | `jira-card` shape (see [`CANVAS.md §5.4`](CANVAS.md#54-forge-live-cards)) |

Canvas card props:

```ts
{
  ticketKey: 'DEVOPS-414',
  snapshot: {
    key, summary, status, priority, issueType, assignee, updated
  }
}
```

Live lookup: `findJiraItem(ticketKey)` scans every column's
`jiraItemsByInstance` (`apps/desktop/src/lib/services/liveCardData.ts`).
A card on a canvas auto-refreshes when **any** Jira column has the
issue loaded.

---

## 10. PanelKind Defaults & Persistence

```ts
DEFAULT_PANEL_WIDTHS.jira = 420;
DEFAULT_PANEL_ORDER includes 'jira';
```

In the v1→v3 layout migration default toggle, **Jira is on** for
existing users (see `docs/WORKBENCH.md §A.15`).

Per-instance filter persistence: `localStorage` key
`woom:jira-col-filters-by-instance:v1` (mirroring the GitHub one).

Workspace-global assignee chip persistence: `inboxState.jiraAssignee`
plus `jiraAssigneeAny`, persisted via the same effect that persists
inbox state.

---

## 11. Empty / Loading / Error States

```svelte
{#if itemsLoading && items.length === 0}
  <div class="inbox-state">Loading…</div>
{:else if itemsError}
  <div class="inbox-state inbox-state--error">
    {itemsError}
    <button class="link-inline" onclick={onRefreshJiraInbox}>Retry</button>
{:else if items.length === 0}
  <div class="inbox-state">…</div>
```

`apps/desktop/src/lib/components/workbench/JiraColumn.svelte:423-437`.

The slide-over has its own loading / error states and a retry button
on detail-load failure.

---

## 12. Keyboard Shortcuts

- **`Enter` / `Space`** on an inbox row: open slide-over.
- **`Esc`** on slide-over: close (also via clicking backdrop).
- **`⌘K`** global: command palette — Jira issues are indexed there
  with section "Jira issues" and "Jira boards" / "Jira projects"
  separately. See `docs/COMMAND_PALETTE.md §C.2`.

There's no `j` / `k` row navigation specifically wired for Jira (those
keys move selection in the GitHub column only at present).

---

## 13. TS Types

```ts
// apps/desktop/src/lib/data.ts:122-137
export interface JiraItem {
  id: string;
  key: string;
  summary: string;
  description: string | null;
  status: string;
  status_category: string;
  priority: string | null;
  issue_type: string;
  assignee: JiraActor | null;
  reporter: JiraActor | null;
  labels: string[];
  updated: string;
  created: string;
  url: string;
}
```

Related: `JiraDetail` (`data.ts:598-615`) carries `comments[]` and
`transitions[]`. `JiraComment`, `JiraTransition`, `JiraWorkflowStatus`
in the same file. `JiraUserSummary` for the assignee picker.

---

## 14. Tauri IPC Surface

```text
# Auth
jira_connect(workspace, email, token) -> JiraUser
jira_status()                         -> ConnectionStatus
jira_disconnect()                     -> ()

# Lists
jira_list_inbox()
jira_list_inbox_for(instanceId)       # variant for column
jira_search(jql, max_results?)
jira_list_projects(query?, limit?)
jira_list_boards(project_key?)
jira_list_sprints(project_key, state?)
jira_list_statuses(project_key?)
jira_list_assignable_users(project_key, query?)

# Detail / mutations
jira_get_issue_detail(key)
jira_update_issue(key, patch)
jira_transition_issue(key, to, comment?)
jira_add_comment(key, body)
jira_set_assignee(key, account_id)
jira_set_priority(key, priority)
jira_set_labels(key, labels)
jira_create_issue(project_key, summary, ...)
jira_list_worklogs(key)
jira_add_worklog(key, ...)
jira_delete_worklog(key, worklog_id)
```

(See `apps/desktop/src-tauri/src/lib.rs:144-178`.)

No frontend `listen()` channels for Jira.

---

## 15. Open TODOs

1. No raw JQL field in column UI — power users go through MCP `search`.
2. Polling is gated on GitHub connectedness (see §6) — silently no-ops
   when only Jira is connected.
3. No subtasks tree in the slide-over.
4. No attachment upload.
5. No reactions.
6. Markdown→ADF round-trip is lossy for some Jira-specific blocks
   (panels, cards) that aren't in standard markdown — agent-generated
   content stays clean; user-pasted exotic content might lose styling.
7. Custom fields beyond the standard schema aren't surfaced.
8. Server / Data Center support requires a different REST surface
   (`/rest/api/2/`) — not in v1.

---

## 16. Glossary

- **`JiraDetailPane`** — the slide-over component for a focused issue.
- **`jiraFocusKey`** — `inboxState.jiraFocusKey: string | null`, the
  global pointer to the open issue.
- **`SprintScope`** — `number | 'backlog'`, used in `JiraFilters.sprintIds`.
- **ADF** — Atlassian Document Format, JSON tree representation of rich
  text. Description and comment bodies round-trip through ADF.
- **`JIRA_KEY`** — Keychain key (`"jira"`) holding `{ workspace, email, token }`.
- **Hot path** — there is no GitHub-style "fast list" alternative; every
  Jira list request goes through `jira_search` with a JQL.
