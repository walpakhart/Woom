# Forgehold — GitHub Integration Specification

**Version:** 0.1
**Last updated:** 2026-04-29
**Status:** describes shipping behaviour. Two GitHub surfaces are live:
the **GitHub workbench column** (inbox-style, scoped to "items
involving me") and the **GitHub top-level tab** (a repo browser with
`code / pulls / issues / actions / releases` sections). Detail view is
a global slide-over (`GithubFocusOverlay`).

> Forgehold gives GitHub two homes. The column lives next to the agent
> and is for *triage* — the ten things you should look at this morning.
> The tab is for *exploration* — pick a repo, browse code, watch a
> workflow run, see a release note. They share an inbox cache, the
> same auth token, and the same detail slide-over, but their filter
> models are independent.

---

## 1. Vision & Non-Goals

### 1.1 Vision

The GitHub column answers: *what's waiting on me right now?* By
default, "involving me, open" with a 60-second auto-refresh. Filters
narrow to a repo or change the relationship lens (`authored`,
`review_requested`, `assigned`, custom user, all).

The GitHub tab answers: *let me poke around.* Pick a repo from the
dropdown, switch between Code / PRs / Issues / Actions / Releases like
on github.com — but inside Forgehold's chrome, with the same hotkeys
and the same agent integration.

The slide-over `GithubFocusOverlay` is a **shared** PR / issue detail —
clicking an item from the column or the tab opens the same component.

### 1.2 Goals (v1, shipping)

1. PAT-based auth, token in macOS Keychain.
2. Inbox column with `mode / repo / search / customUser` filters,
   per-instance persistence.
3. GitHub tab with section tabs, repo dropdown, branch / tree / file /
   workflow runs / releases.
4. Shared PR / issue detail slide-over with `conversation / commits /
   files / checks / reviews` tabs.
5. Mutations: comment, review (approve / request changes / comment),
   merge (squash / merge / rebase), close / reopen, react.
6. 60-second silent auto-refresh; 300-ms debounced filter refresh.
7. Drag a card onto an agent column → `@`-mention; onto a canvas →
   `github-pr-card` / `github-issue-card`.
8. Agent MCP surface for read + comment + review + merge + propose-*
   tools.
9. Connect-modal flow + idle "connect first" view.

### 1.3 Non-Goals (v1)

- **Real-time push.** No webhook listener; we poll. Fine for a desktop
  app at human cadence.
- **Cross-repo unified search beyond the GitHub search API.** We pass
  user-typed text into the `q=` string verbatim and let GitHub do the
  rest.
- **Authoring tags / releases / labels from the UI.** `propose_*` tools
  exist for branches, commits, PRs but the UI surface is read-only +
  comment / review / merge / close.
- **Inline review thread reply UI.** Comments add at the issue / PR
  level; per-line review replies are deferred to v1.x.
- **Reactions UI.** API supports them but the app doesn't render them
  in v1.
- **OAuth / GitHub Apps installation flow.** PAT only — by design
  (`docs/ROADMAP_1.0.md §6`). Token UX investment goes into rotation
  reminders, multi-org PATs, and diagnostics instead.
- **Server-side rate-limit batching.** We let it 403 and surface "rate
  limited" to the user.

---

## 2. Top-Level Anatomy

```
╭─ workbench column (kind === 'github') ─────────────╮
│ ⠿ ⤢ ✕    forge   ●            data-kind="github"  │
├─────────────────────────────────────────────────────┤
│ inbox-brand: ▢ GitHub  forge  ⟳            New PR  │
├─────────────────────────────────────────────────────┤
│ search-bar (opens command palette)              ⌘K │
│ filter-bar:                                         │
│   [Mode ▾]   [Repo ▾]   ⟳refresh                    │
│ inbox-controls: 32 items                            │
├─────────────────────────────────────────────────────┤
│ Today                                               │
│   ▦ #4123  fix(canvas): dblclick edit  ●OPEN    PR  │
│   ▦ #4099  Crash on Mac…                CLOSED  iss │
│ Yesterday                                           │
│   …                                                 │
│ Earlier                                             │
│   …                                                 │
╰─────────────────────────────────────────────────────╯
```

```
╭─ top-level GitHub tab (view === 'githubTab') ──────╮
│ Repo ▾  forge ────────────────────────────────────  │
│ [ Code ] [ Pull requests ] [ Issues ] [ Actions ]  │
│ [ Releases ]                                        │
├─────────────────────────────────────────────────────┤
│   <section content per active tab>                  │
╰─────────────────────────────────────────────────────╯
```

The slide-over is mounted globally:

```svelte
{#if inboxState.focusItem}
  <div class="slide-over"><div class="slide-panel">
    <GithubFocusOverlay item={inboxState.focusItem} ... />
  </div></div>
{/if}
```

---

## 3. Column Anatomy

`apps/desktop/src/lib/components/workbench/GithubColumn.svelte:188-285`.

| Region              | Selector              | Notes                               |
|---------------------|-----------------------|-------------------------------------|
| `ColumnControls`    | top-left              | move / archive / maximize           |
| Resize handle       | `.wb-col-resize`      | snap-flash on threshold             |
| Brand strip         | `.inbox-brand`        | logo + repo / user + New PR         |
| Search proxy        | `.search-bar`         | clicking opens Command Palette      |
| Filter bar          | `.filter-bar`         | mode dropdown + repo dropdown + ⟳   |
| Counter             | `.inbox-count`        | `{n} items`                         |
| List                | `.inbox-list`         | `Today / Yesterday / Earlier` groups |

Width default `420` px (`apps/desktop/src/lib/state/layout.svelte.ts:11`).
Multiple GitHub columns are allowed — each with its own filter set
(see §4).

There is **no** in-column section tabbar (`code / pulls / issues / …`)
— that lives only on the top-level GitHub tab. The column is strictly
inbox-shaped.

---

## 4. Filter Model (`GithubFilters`)

```ts
// apps/desktop/src/lib/state/inbox.svelte.ts
export interface GithubFilters {
  mode: GithubFilterMode;
  repo: string | null;       // 'owner/name' or null = all repos
  search: string;            // appended verbatim to GitHub `q=`
  customUser: string;        // login when mode === 'user'
}

export type GithubFilterMode =
  | 'involving' | 'authored' | 'review_requested'
  | 'assigned'  | 'user'     | 'all';
```

### 4.1 Persistence

`localStorage` key `forgehold:github-col-filters-by-instance:v1`
maps `instanceId → GithubFilters`. Filters survive across restarts and
are scoped per column, so two GitHub columns can be set to different
modes.

### 4.2 Query construction

`buildGithubQuery(filters, me)` (`inbox.svelte.ts:633-666`):

```text
['is:open']
  + ('involves:<me>' | 'author:<me>' | 'review-requested:<me>' | 'assignee:<me>' | 'involves:<customUser>' | nothing)
  + ('repo:owner/name' if filters.repo)
  + (filters.search verbatim)
  → joined with spaces
```

We do **not** structure status / assignee / author / label as separate
controls. The user types them into `search` and GitHub interprets the
qualifier. This trades discoverability for power.

### 4.3 Hot-path vs full search

- **`mode === 'involving'` + no repo + empty search** → uses the
  pre-aggregated `github_list_inbox` IPC (cached on the Rust side per
  account).
- **Anything else** → `github_search_inbox(q)` with `per_page=50`.

The cap of 50 is enforced in `apps/desktop/src-tauri/src/github.rs:377-400`.

---

## 5. List Rendering

- **No virtualization.** Plain `{#each}` over the items array; the cap
  of 50 keeps it cheap.
- **Time grouping:** `groupByTime(items)` partitions by `updated_at`
  into `today / yesterday / earlier`. Section headers render only if
  the bucket is non-empty.
- **Card content** per row: GH-marker `▦`, `#number`, relative time,
  truncated title, mini-tag for state (open / closed / merged / draft),
  kind glyph (PR vs issue), `· owner/repo` if filter is "all repos".
- **Loading / error / empty:** `inbox-state` rows above the list. Retry
  button on errors.

`InboxItem` shape:

```ts
// apps/desktop/src/lib/data.ts:287-304
export interface InboxItem {
  id: number;
  number: number;
  title: string;
  body: string | null;
  state: string;            // 'open' | 'closed'
  is_pull_request: boolean;
  draft: boolean;
  merged: boolean;
  url: string;
  author: Actor | null;
  labels: Label[];
  assignees: Actor[];
  repo: RepoRef | null;
  comments: number;
  created_at: string;
  updated_at: string;
}
```

There is no separate `GithubItem` type — `InboxItem` is the single
GitHub list shape both in column and in tab.

---

## 6. Detail Slide-over (`GithubFocusOverlay`)

Component: `apps/desktop/src/lib/components/inbox/GithubFocusOverlay.svelte`.

Tabs (rendered conditionally on `is_pull_request`):

| Tab            | Source IPC                                                | Notes                                |
|----------------|-----------------------------------------------------------|--------------------------------------|
| `conversation` | `github_get_pr` + `github_list_comments` + `github_list_pr_reviews` + `github_list_review_comments` | Sorted timeline, body first |
| `commits`      | `github_list_pr_commits`                                  | PR only                              |
| `files`        | `github_list_pr_files` + `parsePatch(patch)`              | PR only; per-file accordion + unified diff |
| `checks`       | `github_list_check_runs(sha=head_sha)`                    | PR only                              |
| `reviews`      | grouped review comments                                   | PR only                              |

Issues skip `commits` / `files` / `checks` / `reviews` and only have
`conversation`.

The conversation timeline merges issue comments and PR reviews and
commits into a single chronological list (sorted by `at`). Inline
review comments are nested under their parent review when expanded.
Reactions are not rendered.

### 6.1 Pre-fetch on focus

When `inboxState.focusItem` changes, `loadDetail(item)` (in
`inbox.svelte.ts`) parallel-fetches the data sets above and writes
them to a `detailShelf` shared between the slide-over and the column
list (so badges in the column can react to comment count changes
post-mutation).

### 6.2 MCP `open_github_pr` tab argument

The MCP `open_github_pr` tool (`apps/desktop/src-tauri/sidecars/forgehold-app/src/main.rs:785-799`)
takes an optional `tab` parameter:

```ts
tab?: 'conversation' | 'commits' | 'files' | 'reviews' | 'checks';
```

Forgehold opens the slide-over with the specified tab pre-selected.
Useful for "Claude, take me to the files of #4123".

---

## 7. Mutations

| User intent      | UI                                  | IPC                       |
|------------------|-------------------------------------|---------------------------|
| Add comment      | Conversation tab "Comment" textarea | `github_add_comment`      |
| Submit review    | "Review" button → modal             | `github_submit_review` (`event: APPROVE \| REQUEST_CHANGES \| COMMENT`) |
| Merge PR         | "Merge" button → modal              | `github_merge_pr` (`method: merge \| squash \| rebase`) |
| Close            | Header chevron menu                 | `github_set_state(state: 'closed')` |
| Reopen           | Same place                          | `github_set_state(state: 'open')` |
| Create PR        | "New PR" in column header           | `github_create_pr`        |

After each mutation we `reloadDetailAndLists()` — refresh detail and
every GitHub column's inbox.

The agent has its own write surface via the GitHub MCP server. The
`propose_*` tools (`propose_commit`, `propose_pr`, `propose_bash`,
`propose_switch_cwd`) **don't** call these IPC paths directly — they
queue an Action Card on the chat session, which on approval routes
through `dispatchAction` (see `docs/AGENTS.md §7`). The plain
`add_comment / submit_review / merge_pr` MCP tools execute immediately.

---

## 8. Live Updates / Polling

```ts
// +page.svelte:540-546
if (connectedGithub) {
  refreshInterval = setInterval(() => {
    void refreshAllInboxes({ silent: true });
    if (connectedJira) void refreshAllJiraInboxes({ silent: true });
  }, 60_000);
}
```

`silent: true` means no spinner — the badge counts and list update
in-place. Filters debounce at 300 ms (`updateGithubFilters`).

Sentry is **not** in this 60-second tick (see `docs/SENTRY.md §7`).
Relative-time labels in the UI tick at 30 s (`now = Date.now()` ticker
in `+page.svelte:488`).

There is no WebSocket / SSE; no `listen('github:*')` events on the
frontend. Pure pull.

---

## 9. Background Sync, Retry, Rate Limits

- On boot: `refreshAllInboxes()` after the auth check completes.
- On reconnect: same.
- On column added: just-in-time refresh for that column only.

`apps/desktop/src-tauri/src/github.rs:246-261` detects rate-limit
responses:

```rust
if status == TOO_MANY_REQUESTS || status == FORBIDDEN {
  if x-ratelimit-remaining == "0" {
    return GithubError::RateLimited;
  }
}
```

Frontend surfaces `'rate limited'` as the inbox error string. We do
not auto-retry; the user retries via the inline button or waits for
the next tick.

---

## 10. Code Browser (GitHub Tab)

The Code section lives in `apps/desktop/src/lib/views/GithubTab.svelte`.

| Action            | IPC                            | Notes                                     |
|-------------------|--------------------------------|-------------------------------------------|
| List branches     | `github_list_repo_branches`    | branch dropdown                            |
| Tree (one level)  | `github_list_tree`             | recursive=true on the API, depth-1 in UI  |
| File contents     | `github_get_file_content`      | base64 → string                            |
| README            | `github_get_readme`            | rendered as markdown via `Markdown.svelte` |

Truncation: GitHub returns `truncated: true` on huge repos; we display
"Tree was truncated by GitHub (very large repo). Showing a partial
view." (`GithubTab.svelte:611-616`). No client-side pagination; the
user clicks into a subdirectory to fetch its slice.

This is the same fetch path used by `RepoCodeView.svelte` when an
Editor column or chat mention needs remote read access.

---

## 11. Authentication

Only personal access tokens are supported. OAuth / GitHub App installs
are a permanent non-goal (`docs/ROADMAP_1.0.md §6`); fine-grained PATs
already give per-repo scope without the OAuth registration cost.

```rust
const GITHUB_KEY: &str = "github";

async fn github_connect_pat(token: String) -> Result<GithubUser, String> {
  let trimmed = token.trim();
  // validate by fetching /user
  keychain::set(GITHUB_KEY, &trimmed)?;
}

async fn github_status() -> Result<ConnectionStatus, String> {
  match keychain::get(GITHUB_KEY)? {
    None => Disconnected,
    Some(t) => match fetch_user(&t).await {
      Ok(user) => Connected { user },
      Err(InvalidToken) => { keychain::delete(GITHUB_KEY)?; Disconnected }
    }
  }
}
```

`apps/desktop/src-tauri/src/lib.rs:243-267`.

The Connect modal sends the user to the right "create token" URL with
pre-filled scopes:

```ts
function githubTokenUrl() {
  const scopes = ['repo', 'read:user', 'read:org'].join(',');
  return `https://github.com/settings/tokens/new?scopes=${scopes}&description=Forgehold%20Desktop`;
}
```

Sidecar `forgehold-github` reads the token from `GITHUB_TOKEN` env var
that the parent process sets (see `docs/MCP.md`).

---

## 12. Drag-to-Canvas / Drag-to-Pill / Drag-to-Agent

### 12.1 Source — column item

```svelte
<div class="inbox-item"
     draggable="true"
     ondragstart={(e) => onDragStart({ source: 'github', item }, e)}
     ondblclick={() => onFocusItem(item)}>
```

`onDragStart` (in `+page.svelte:606-631`) does:

- `e.dataTransfer.setData('text/plain', `${item.repo?.full_name ?? '#'}#${item.number} · ${item.title}`)`
- `attachDragChip(e, 'github', label)` to render the floating chip.
- `setDragPayload({ source: 'github', item })` — global module state for
  the drop side (see `apps/desktop/src/lib/state/drag.svelte.ts`).

### 12.2 Drop targets

| Target                          | Result                                                          |
|---------------------------------|-----------------------------------------------------------------|
| Agent column body               | `@<repo>#<number>` mention in composer                          |
| `claude` / `cursor` pill        | Spring-loaded — opens menu of instances; drop on one to add mention |
| Other pills (`github`, `jira`, …)| Rejected (`pillCanAccept` returns `false` for non-agent kinds)  |
| Canvas surface                  | `github-pr-card` / `github-issue-card` shape                    |

The canvas card carries `{ owner, repo, number, snapshot }` so the
shape renders even when the GitHub column for that repo isn't open
(the snapshot is taken at drop time and refreshes when a column with
that repo loads). See [`CANVAS.md §5.4`](CANVAS.md#54-forge-live-cards).

---

## 13. MCP Tools

### 13.1 `user-forgehold-github` (read + write)

| Tool              | Required args                                | Notes                                |
|-------------------|----------------------------------------------|--------------------------------------|
| `get_pr`          | `owner`, `repo`, `number`                    |                                      |
| `get_pr_diff`     | same                                         | Unified diff                         |
| `get_pr_files`    | same                                         | Per-file structured                  |
| `get_pr_comments` | same                                         |                                      |
| `list_tree`       | `owner`, `repo` + opt. `reference`            |                                      |
| `get_file`        | `owner`, `repo`, `path` + opt. `reference`   | Base64 in transit                    |
| `list_commits`    | `owner`, `repo` + opt. `reference`, `limit`  |                                      |
| `list_releases`   | `owner`, `repo` + opt. `limit`               |                                      |
| `list_workflow_runs` | `owner`, `repo` + opt. `limit`            |                                      |
| `list_repos`      | opt. `name`, `limit` (≤200)                  | For repo dropdown                    |
| `get_readme`      | `owner`, `repo`                              |                                      |
| `search_prs`      | `query` + opt. `limit`                       | Same `q` syntax as `buildGithubQuery` |
| `search_issues`   | `query` + opt. `limit`                       |                                      |
| `add_comment`     | `owner`, `repo`, `number`, `body`            | Direct write                         |
| `submit_review`   | `owner`, `repo`, `number`, `event` + opt. `body` | `APPROVE / REQUEST_CHANGES / COMMENT` |
| `merge_pr`        | `owner`, `repo`, `number` + opt. `method`, `commit_title` | Direct write           |
| `propose_commit`  | `message` + opt. `body`, `note`, `push`      | Approval-gated via Action Card       |
| `propose_pr`      | `title` + opt. `body`, `base`, `draft`, `note` | Approval-gated                     |
| `propose_switch_cwd` | `path` + opt. `reason`                    | Approval-gated                       |
| `propose_bash`    | `command` + opt. `reason`                    | Approval-gated; mandatory for any state-changing shell command per server use-instructions |

Auth: `GITHUB_TOKEN` env var passed to the sidecar by the parent app
(see `claude_mcp.rs:188-327`).

### 13.2 `user-forgehold-app` (UI navigation; not GitHub-specific but commonly invoked alongside)

Subset relevant to GitHub:

| Tool                    | Effect                                            |
|-------------------------|---------------------------------------------------|
| `open_github_pr`        | Slide-over for PR (with optional `tab`)           |
| `open_github_issue`     | Slide-over for issue                              |
| `open_github_repo`      | Top-level GitHub tab on a section + repo + path   |
| `set_github_column`     | Patch filters on an existing GitHub column        |
| `open_connect_modal`    | Surface the GitHub PAT modal                      |

These tools execute on the frontend by dispatching from the agent
stream into `+page.svelte:1888-1912` and the `mcp__app__*` switch.
Read-only handlers don't touch the user's data — pure UI navigation.

---

## 14. PanelKind Defaults & Multi-Instance

```ts
DEFAULT_PANEL_WIDTHS.github = 420;
```

Multiple GitHub columns are valid per workbench. Each carries its own
`GithubFilters` (per `instanceId`) so a "Reviews" column and an
"Authored by me" column can sit side by side.

The historical comment in `forgehold-app`'s `add_workbench_instance`
description ("github / jira / sentry are singletons") is **stale**;
`addPanelInstance` always mints a new instance.

In the v1→v3 layout migration default toggle, GitHub is **on** for
existing users (see `docs/WORKBENCH.md §A.15`).

---

## 15. Empty / Loading / Error

### 15.1 Column

- "Loading inbox…"
- "Failed to load. Retry"
- "No open items involving you." (or similar based on `mode`)

### 15.2 GitHub tab

- Not connected: "Connect GitHub first" CTA → opens connect modal.
- No repo selected: blank section with "Select a repository".
- Per-section: "Loading…" / "No items" / error card.

### 15.3 Slide-over

- `detailError` with retry inline.
- Per-tab: "Loading…" / "No items".

---

## 16. Keyboard Shortcuts

| Key       | Scope            | Action                                                   |
|-----------|------------------|----------------------------------------------------------|
| `⌘K`      | global           | Toggle Command Palette                                    |
| `Esc`     | global           | Close palette → close slide-over → close modals → restore maximize |
| `j`       | workbench, no modal | `moveSelection(+1)` over inbox items in focused column |
| `k`       | workbench, no modal, no `⌘` | `moveSelection(-1)`                          |
| `Enter`   | inbox row focus  | Open in slide-over                                        |
| `o`       | (TBD)            | Currently no shortcut for "open in browser"               |

`anyModalOpen()` returns `true` while `inboxState.focusItem` is set, so
**`j` / `k` are blocked while the slide-over is open**. This is by
design — the slide-over has its own scroll context.

---

## 17. Connection Modal

`open_connect_modal` finds the connection meta by source id and opens
the matching modal. For GitHub:

```ts
if (conn.id === 'github') {
  openModal('pat', { conn, token: '', error: null, busy: false });
}
```

`apps/desktop/src/routes/+page.svelte:3209-3213`.

The modal posts to `github_connect_pat`; on success refreshes `connectionsState`
and triggers `refreshAllInboxes()`.

---

## 18. Tauri IPC Surface

```text
# Auth
github_connect_pat(token)   -> GithubUser
github_status()             -> ConnectionStatus
github_disconnect()         -> ()

# Lists
github_list_inbox()                 -> Vec<InboxItem>
github_search_inbox(query)          -> Vec<InboxItem>
github_list_repos(name?, limit?)    -> Vec<Repository>
github_list_repo_items(...)         -> Vec<InboxItem>
github_list_workflow_runs(...)      -> Vec<WorkflowRun>
github_list_tree(owner, repo, ref?) -> Tree (with truncated flag)
github_list_releases(...)           -> Vec<Release>
github_list_repo_commits(...)       -> Vec<Commit>
github_list_repo_branches(...)      -> Vec<Branch>
github_get_readme(owner, repo)      -> RepoReadme
github_get_file_content(...)        -> FileBlob

# Detail
github_get_pr(owner, repo, number)         -> PrDetail
github_list_pr_files(...)                  -> Vec<ChangedFile>
github_list_pr_commits(...)                -> Vec<Commit>
github_list_check_runs(owner, repo, sha)   -> Vec<CheckRun>
github_list_pr_reviews(...)                -> Vec<Review>
github_list_review_comments(...)           -> Vec<ReviewComment>
github_list_comments(...)                  -> Vec<Comment>
github_get_commit(owner, repo, sha)        -> Commit
github_get_inbox_item(owner, repo, number) -> InboxItem (single)

# Mutations
github_add_comment(...)
github_submit_review(...)
github_set_state(...)
github_merge_pr(...)
github_compare(...)
github_create_pr(...)
github_rerun_workflow(...)
github_cancel_workflow(...)
```

Defined in `apps/desktop/src-tauri/src/lib.rs:113-141`.

There are **no Tauri events** for incremental GitHub updates. Refresh
is always pull-based.

---

## 19. Open TODOs

1. Stale header comment in `apps/desktop/src-tauri/sidecars/forgehold-github/src/main.rs:1-6`
   still says "read-only phase 2" while the file ships writes.
2. Stale comment in `forgehold-app` `add_workbench_instance` description
   says GitHub / Jira / Sentry are singletons; multi-instance is the
   actual behaviour.
3. No dedicated review-thread reply UI (per-line comment threads).
4. No reactions UI (we have the data, just no rendering).
5. No "open in browser" shortcut for the focused row.
6. `q` is wide-open; we don't validate it. A malformed query falls
   through to the GitHub error path, which is okay but unfriendly.
7. We don't paginate beyond `per_page=50`. For "all repos / all open"
   workflows users hit the cap fast.
8. Rate-limit error doesn't display the reset window (we have the
   `x-ratelimit-reset` header but don't surface it).

---

## 20. Glossary

- **Inbox** — the column flavour of GitHub: items relevant to me right now.
- **GitHub tab** — the top-level repo browser (`view === 'githubTab'`).
- **Focus item** — `inboxState.focusItem`, the global pointer the
  slide-over reads.
- **Detail shelf** — `inboxState.detailShelf`, the cache of fetched
  detail data per focused item, shared by column rows and slide-over.
- **`GithubFocusOverlay`** — the shared PR / issue slide-over component.
- **`GithubFilters`** — the per-instance filter object for a GitHub
  column.
- **Hot-path** — `github_list_inbox` (no query) vs `github_search_inbox`.
- **`propose_*`** — the four agent tools that queue an Action Card
  instead of executing.
