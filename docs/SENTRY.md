# Forgehold — Sentry Integration Specification

**Version:** 0.1
**Last updated:** 2026-04-29
**Status:** describes shipping behaviour. Sentry self-hosted and Sentry
SaaS both supported (`host` is configurable). Auth is a single
internal integration token.

> The Sentry column shows live grouped errors for the connected org.
> Filters narrow by project / environment / status / level / sort, plus
> free-text query in Sentry's own search syntax. The shared
> `SentryDetailPane` opens issue + (optional) event in one slide — it's
> a single component that handles both, switching on
> `inboxState.sentryFocusEventId`. There's no separate "issue pane" vs
> "event pane".

---

## 1. Vision & Non-Goals

### 1.1 Vision

Triage. The user keeps a Sentry column open while shipping; new errors
roll in, the user clicks one, sees the latest stack frame + breadcrumbs
+ tags, marks resolved. The agent has parallel access through the
`user-forgehold-sentry` MCP server: `search_issues`, `get_event`,
`get_issue_tags`, plus mutations for status / comment.

### 1.2 Goals (v1, shipping)

1. Token auth: host + organization slug + token; in macOS Keychain.
2. Per-instance filter set: search, status, level, projects[],
   environment, sort.
3. Slide-over `SentryDetailPane` showing: issue summary, latest event
   (or arbitrary event id), exceptions + stack frames + source context,
   breadcrumbs summary, tags (≤ 30 pairs), other-events list (≤ 30).
4. Status mutations: resolve / unresolve / ignore via
   `sentry_set_status` IPC.
5. 250 ms filter debounce, on-demand refresh; no built-in polling.
6. Drag to canvas → `sentry-event-card`; drag to agent → mention.
7. MCP tools for issue / event read, tags, search, status update,
   comment.

### 1.3 Non-Goals (v1)

- **Sentry session replay.** Out of scope.
- **Performance monitoring views** (transactions / spans). Errors only.
- **Discover / metrics queries.**
- **Alerts / rules editing.**
- **Custom dashboards.**
- **Live tail / streaming events** with WebSocket. Pull-only.
- **Background polling** in v1 — Sentry is **not** in the 60-second
  GitHub/Jira tick.
- **Cross-org views.** One Sentry connection at a time.
- **OAuth.** Internal integration tokens only — by design
  (`docs/ROADMAP_1.0.md §6`).

---

## 2. Column Anatomy

```
╭─ Sentry column (kind === 'sentry') ────────────────╮
│ ⠿ ⤢ ✕  Sentry — sentry.io/forgehold     data-kind  │
├─────────────────────────────────────────────────────┤
│ filter row 1: [Project ▾]   [Environment ▾]         │
│ filter row 2: [Status ▾] [Level ▾] [Sort ▾]   ⟳     │
│ search: ▼ "TypeError near useReducer"               │
├─────────────────────────────────────────────────────┤
│ FORGE-1A2  Cannot read 'foo'…   FATAL  ×3.4k        │
│ FORGE-1B7  ENOENT canvas-state…  ERROR ×210         │
│ …                                                   │
╰─────────────────────────────────────────────────────╯
```

Width default `440` (`apps/desktop/src/lib/state/layout.svelte.ts:13`).
Wider than the GitHub/Jira columns because Sentry titles tend to be
the long stack-trace first line.

Slide-over mounted globally via `inboxState.sentryFocusId` →
`SentryDetailPane`.

---

## 3. Filter Model (`SentryFiltersPersisted`)

```ts
// apps/desktop/src/lib/state/inbox.svelte.ts:105-112
interface SentryFiltersPersisted {
  search: string;
  status: 'unresolved' | 'resolved' | 'ignored' | 'all';
  level: 'all' | 'fatal' | 'error' | 'warning' | 'info' | 'debug';
  projects: string[];                  // project slugs
  environment: string | null;          // e.g. 'production', 'staging'
  sort: 'date' | 'new' | 'priority' | 'freq' | 'user';
}
```

### 3.1 Query construction

`buildSentryQuery(instanceId)` (`inbox.svelte.ts:1280-1290`):

```text
parts = [];
if (status !== 'all') parts.push(`is:${status}`);
if (level !== 'all')  parts.push(`level:${level}`);
if (search)           parts.push(search);
return parts.join(' ');
```

Sentry's own search syntax — qualifiers like `browser.name:Chrome`,
`release:1.2.3`, `os:iOS` — pass through `search` verbatim.

### 3.2 Other parameters

`refreshSentryInbox` calls `sentry_list_issues` with:

- `query` — the constructed string above.
- `projectSlugs` — array (multi-select).
- `environment` — single value or `null`.
- `sort` — directly mapped.
- `limit: 50` — same cap pattern as GitHub.

### 3.3 Environment is global

`sentryEnvironmentOptions` is a single global cache (one Sentry org →
one env list), so all Sentry columns share the dropdown. There's a
TODO note in `inbox.svelte.ts:1392-1401` to revisit if multiple orgs
become a thing.

---

## 4. List Rendering

- **No virtualization.** Cap of 50 items per fetch.
- **No grouping** — flat list, ordered by `sort`.
- **Card content** per row: short id, truncated title, level chip
  (`sentryLevelClass(level)` mapping `fatal → red`, `error → orange`,
  `warning → yellow`, `info → blue`, `debug → gray`), event count
  with `×` prefix, project slug.

---

## 5. Slide-over (`SentryDetailPane`)

`apps/desktop/src/lib/components/inbox/SentryDetailPane.svelte`.

Loads in parallel:

- `sentry_get_issue(issueId)` → header data (title, culprit, status,
  count, first/last seen, project).
- `sentry_get_event_detail(issueId, eventId | null = latest)` → the
  current event's full payload.
- `sentry_list_events(issueId, limit: 30)` → "other events" picker.

Sections (top to bottom):

1. **Issue header** — short id, title, level, status, project, count,
   first/last seen, action buttons (resolve / ignore / open in browser).
2. **Event meta** — event id, timestamp, environment, release, OS,
   browser, user, tags (top several).
3. **Exceptions** — for each exception in `event.exceptions`:
   - `type: message`
   - frames (deepest first; reversed in render so most recent call is
     on top): module / function / file:line, with optional source
     context lines under `<details>`.
4. **Breadcrumbs summary** — pre-formatted multi-line string from
   the API. Could be rendered as a list in v1.x.
5. **Tags** — full `[key, value][]` (up to 30 pairs).
6. **Other events** — link list to switch the focused event id without
   leaving the slide-over.

Switching events updates `inboxState.sentryFocusEventId` and re-fetches.

---

## 6. Mutations

| Action      | UI                          | IPC                                  |
|-------------|-----------------------------|--------------------------------------|
| Resolve     | Header "Resolve"            | `sentry_set_status(issueId, 'resolved')` |
| Unresolve   | Header (after resolve)      | `sentry_set_status(issueId, 'unresolved')` |
| Ignore      | Header "Ignore"             | `sentry_set_status(issueId, 'ignored')` |
| Comment     | **Not in UI** — MCP only    | `add_comment` MCP tool (sidecar)     |

After mutation, the slide-over patches local state and triggers
`refreshAllSentryInboxes({ silent: true })` so column lists reflect.

There is **no** `sentry_add_comment` Tauri command in `lib.rs`. The
comment surface is exclusively MCP-driven (sidecar `forgehold-sentry`
binary), reflecting that comments are an agent / triage tool more
than a daily UI gesture.

---

## 7. Live Updates

- **Filter debounce:** `scheduleSentryFilterRefresh(instanceId)` waits
  **250 ms** then calls `refreshSentryInbox(silent: true)`.
- **Polling:** **none.** Sentry is intentionally out of the 60 s tick
  to keep API budget for important orgs. Refresh is on:
  - column mount,
  - filter change (debounced),
  - manual ⟳,
  - mutation in slide-over.
- **No push events.** No `listen('sentry:*')` on the frontend.

---

## 8. Authentication

`apps/desktop/src-tauri/src/lib.rs:634-685`:

```rust
async fn sentry_connect(host, organization_slug, token) -> Result<SentryUser> {
  let creds = SentryCredentials {
    host: sentry::normalize_host(&host),
    organization_slug: organization_slug.trim(),
    token: token.trim(),
  };
  // Validate by GET /api/0/organizations/<slug>/
  keychain::set(SENTRY_KEY, &payload)?;
}
```

`SENTRY_KEY = "sentry"`. Disconnect: `sentry_disconnect`.

`SentryUser` shape — `data.ts:141-152`:

```ts
export interface SentryUser {
  host: string;
  organization_slug: string;
  username: string;
  email: string | null;
  organization_name: string | null;
}
```

Self-hosted Sentry works as long as it's API-compatible with the
managed product (Sentry's own self-host is). Different `host` value;
same paths.

---

## 9. MCP Tools (`user-forgehold-sentry`)

| Tool             | Required          | Optional                                 |
|------------------|-------------------|------------------------------------------|
| `get_issue`      | `issue_id`        |                                          |
| `search_issues`  | `query`           | `limit`                                  |
| `get_event`      | `issue_id`        | `event_id` (null = latest)               |
| `list_events`    | `issue_id`        | `limit`                                  |
| `get_issue_tags` | `issue_id`        |                                          |
| `update_issue`   | `issue_id`        | `status`, `assigned_to`                  |
| `add_comment`    | `issue_id`, `body`|                                          |
| `list_projects`  | (none)            |                                          |
| `list_releases`  | (none)            | `limit`, `project`                       |

Sentry triage flow per server use-instructions:

```
get_issue(short_id) → quick summary
get_event(short_id) → latest stack
get_issue_tags(short_id) → browser/OS/release/env distribution
list_events(short_id) → many occurrences
search_issues(query)
update_issue(short_id, status='resolved')
add_comment(short_id, body)
```

Auth: sidecar reads `SENTRY_HOST`, `SENTRY_TOKEN`, `SENTRY_ORG`
env vars set by the parent (see `docs/MCP.md`).

---

## 10. Drag-and-Drop

Source: column rows draggable; payload `{ source: 'sentry', item }`.

```ts
const ref = payload.item.short_id || payload.item.id;
e.dataTransfer.setData('text/plain', ref);
attachDragChip(e, 'sentry', `${ref} · ${payload.item.title}`);
```

Drop targets:

| Target          | Effect                                                                    |
|-----------------|---------------------------------------------------------------------------|
| Agent column    | `@<short_id>` mention; body includes summary / culprit / level / status / project |
| Agent pill      | Spring-loaded → instance picker → mention                                 |
| Canvas surface  | `sentry-event-card` shape                                                 |

Canvas card props:

```ts
{
  issueId: 'abc...',
  shortId: 'FORGE-1A2',
  snapshot: { title, level, status, count, culprit, project }
}
```

Live lookup: `findSentryIssue(issueId)` — `liveCardData.ts`.

---

## 11. PanelKind Defaults & Persistence

```ts
DEFAULT_PANEL_WIDTHS.sentry = 440;
DEFAULT_PANEL_ORDER includes 'sentry';
```

In the v1→v3 migration default toggle, **Sentry is off** for existing
users (see `docs/WORKBENCH.md §A.15`). New users with Sentry connected
get a Sentry column auto-spawned at first connect.

Per-instance filters: `localStorage` under
`forgehold:sentry-col-filters-by-instance:v1`.

---

## 12. Empty / Loading / Error States

Same shape as Jira:

- `inbox-state` row above the list: "Loading…", error with Retry,
  empty state.
- Slide-over: separate `issueLoading` / `eventError` / "no events"
  paths; retry buttons inline.

---

## 13. Keyboard Shortcuts

- `Enter` in the search field forces a non-silent `refreshSentryInbox`.

```svelte
onkeydown={(e) => {
  if (e.key === 'Enter') {
    void refreshSentryInbox(instanceId, { silent: false });
  }
}}
```

`apps/desktop/src/lib/components/workbench/SentryColumn.svelte:246-251`.

- `Esc` on slide-over closes; backdrop click does the same.
- `⌘K` opens command palette (Sentry issues are indexed).

No `j`/`k` row navigation specifically wired for Sentry yet.

---

## 14. TS Types

```ts
// apps/desktop/src/lib/data.ts:160-243
export interface SentryIssue {
  id: string;
  short_id: string;
  title: string;
  culprit: string | null;
  level: string;
  status: string;
  platform: string | null;
  project_slug: string;
  project_name: string;
  count: string;
  user_count: number;
  first_seen: string;
  last_seen: string;
  permalink: string;
  metadata_type: string | null;
  metadata_value: string | null;
}
export interface SentryEvent { ... }
export interface SentryEventDetail {
  event_id: string;
  ...
  tags: [string, string][];
  exceptions: SentryException[];
  breadcrumbs_summary: string | null;
  ...
}
```

`SentryFrame`, `SentryException`, `SentryProject` in the same file.

---

## 15. Tauri IPC Surface

```text
sentry_connect(host, slug, token) -> SentryUser
sentry_status()                   -> ConnectionStatus
sentry_disconnect()               -> ()
sentry_list_issues(query, projectSlugs?, environment?, sort, limit)
sentry_get_issue(issueId)         -> SentryIssue
sentry_list_events(issueId, limit)
sentry_get_event_detail(issueId, eventId|null)
sentry_list_projects()
sentry_list_environments()
sentry_set_status(issueId, status)
```

`apps/desktop/src-tauri/src/lib.rs:694-751`. No frontend events.

---

## 16. Open TODOs

1. **Polling cadence.** Currently zero. A 5-min low-priority tick
   would catch new errors without blowing budget. Not yet wired.
2. **Comment UI.** No in-app surface for adding a comment — only via
   MCP. Pretty common ask.
3. **Single global env list** for the org — multi-org users will see
   the wrong list. TODO comment in `inbox.svelte.ts`.
4. **No assignee picker** in slide-over (API supports it; UI
   doesn't).
5. **No "release" or "browser" surface** at issue level beyond the
   tags blob.
6. **No metrics for false-positives** (mark as spam, etc).
7. **No issue rule editing.**
8. **`metadata_value`** rendering depends on `metadata_type`; we just
   render the value string — for some types this is a JSON blob.

---

## 17. Glossary

- **`SentryDetailPane`** — the unified slide-over for issue + event.
- **`sentryFocusId` / `sentryFocusEventId`** — `inboxState` pointers
  to the open issue and (optionally) a specific event id within it.
- **`SentryFiltersPersisted`** — per-instance filter object.
- **Sort modes** — `date` (last seen), `new` (first seen), `priority`,
  `freq`, `user` (user count). Mapped 1:1 to Sentry API sort strings.
- **`SENTRY_KEY`** — Keychain key (`"sentry"`).
- **Triage flow** — the canonical sequence in the MCP server's
  use-instructions: get_issue → get_event → tags → list_events →
  search → update_issue/add_comment.
