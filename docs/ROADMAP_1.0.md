# Forgehold — 1.0 Release Plan

**Version:** 0.1 (the plan, not the app)
**Last updated:** 2026-04-29
**Status:** master roadmap pulling together every module spec
(`AGENTS.md`, `CANVAS.md`, `COMMAND_PALETTE.md`, `CONNECTIONS.md`,
`EDITOR.md`, `GITHUB.md`, `JIRA.md`, `MCP.md`, `SENTRY.md`,
`WORKBENCH.md`) into a single delta from "0.1 ships" to "1.0 ships".

> 0.1 is feature-rich but rough. The integrations work, the agents
> stream, the workbench persists — but tokens are pasted (no OAuth),
> sessions live in `localStorage` (quota risk), polling is gated on
> GitHub being connected (Jira-only users get no auto-refresh), there
> is no auto-update / no crash reporting / no first-run onboarding,
> and dozens of comments in the codebase are stale relative to the
> behaviour they describe. 1.0 is the bar where someone outside the
> team can install the DMG, connect their tools, and trust the result.

This doc is a living plan. It is **not** a marketing roadmap — it
intentionally over-scopes so we can cut, not under-scope and discover
holes mid-beta.

---

## 1. Cross-Cutting Themes

These apply across every module. Implementing them once unblocks
every per-module 1.0 deliverable. Order is rough priority.

### 1.1 Persistence durability

`localStorage` is the wrong primary store for chat sessions, canvas
JSON, and per-instance filters. Browsers cap the origin at ~5 MB; one
multi-week Claude session with edit cards approaches that on its own.

**1.0:**

- Move `forgehold:claude-sessions:v1` (`docs/AGENTS.md §13`) to disk:
  `~/Library/Application Support/Forgehold/sessions/<id>.json`, plus a
  small index file for boot.
- Same migration for `forgehold:canvas:index:v1` and the per-canvas
  `<id>.json` files (already on disk per `docs/CANVAS.md §11.1` —
  finish wiring on read paths).
- Workbench layout, per-instance filters, theme stay in localStorage
  (small + frequently mutated).
- Eviction policy: oldest archived session > 90 days → cold-store
  (`sessions/cold/`); user can restore from settings.
- Quota dashboard in `SettingsView` (already there per
  `docs/CONNECTIONS.md §8.5`) — extend to show per-source breakdown
  with a "Migrate to disk" CTA for users on the legacy schema.

### 1.2 OAuth across the board (where supported)

Token-paste UX is a distribution killer. PATs are also a privilege-
escalation footgun (every PAT we issue has `repo` scope; the user's
Sentry token is org-wide).

**1.0:**

- GitHub OAuth via the `device flow` (no client secret required, fits
  Tauri).
- Atlassian OAuth 2.0 (3LO) for Jira — needs a registered Forgehold
  client; deferred OAuth registration → ship side-by-side with PAT
  fallback.
- Sentry OAuth via "User Auth Tokens" (their OAuth-equivalent for
  desktop apps).
- Keep PAT modals as fallback / power-user path (some self-hosted
  Sentry / on-prem Jira won't have OAuth).
- Refresh-token rotation handled in keychain; expired-token UX is "you
  need to reconnect" not silent failure.

### 1.3 Auto-update + crash reporting + bug-report

A 1.0 desktop app that doesn't update itself is a museum exhibit.

**1.0:**

- Tauri updater (`@tauri-apps/plugin-updater`) wired to a signed
  artifact channel.
- Per-update: release notes shown in-app, optional rollback to the
  prior version (Tauri supports binary-diff updates; if too heavy,
  ship full bundles).
- Code-signing via Developer ID + notarization (currently
  `Warn skipping app notarization, no APPLE_ID & APPLE_PASSWORD …`
  shows in our build log — that has to be off in 1.0).
- Crash reporting: a dogfood Sentry project for the Forge app itself.
  Reports include sanitized session metadata; user can opt out.
- In-app **"Report bug"** form that bundles the last 200 log lines +
  active layout snapshot + (with consent) recent terminal output. Goes
  to a GitHub issue or a dedicated email.

### 1.4 Polling / live updates

Currently:

- GitHub + Jira poll on a 60-s tick **only when GitHub is connected**
  (`docs/JIRA.md §6`).
- Sentry doesn't poll at all (`docs/SENTRY.md §7`).
- Editor file watcher works.
- No webhooks for any source.

**1.0:**

- Decouple Jira's tick from GitHub. Each source has its own scheduler
  with per-source cadence (GitHub 60s, Jira 60s, Sentry 5min default,
  user-overridable in `SettingsView`).
- Webhook listener (Tauri local HTTP server bound to `127.0.0.1`) for
  GitHub at minimum; falls back to polling when no public URL is
  available (most users).
- Backoff on 429 / rate-limit errors so we don't hammer their token.
- Visual "last refreshed Xm ago" footer per column.

### 1.5 Onboarding & cheatsheet

The first-launch experience today is an empty workbench with a
"Connect a source" hero card. That's a fine empty state; it's not an
onboarding.

**1.0:**

- 4-step welcome flow: theme + identity + first source + first agent.
  Skippable.
- `?` global shortcut → keyboard cheatsheet overlay listing every
  shortcut from every module's "Keyboard" section.
- "Show me around" → 30-second guided tour highlighting workbench
  tabs, the pill bar, the agent column, the Editor link button, the
  command palette.
- Empty-workbench state nudges: "Drag a chip" hint when no instances
  exist.

### 1.6 Accessibility

We use semantic HTML and `data-instance-id` for testing, but we don't
currently audit:

- ARIA labels on icon-only buttons (lots of "" label).
- Keyboard order across the whole app (try tab from the empty
  workbench; you fall off the rail).
- Screen-reader text for live regions (chat streaming).
- Focus trapping in modals (slide-overs and the command palette).
- Reduced-motion preference for animated reorder / snap-flash.

**1.0:**

- Pass `axe-core` audit on every primary surface.
- Implement focus traps in `ModalsRoot.svelte` and slide-overs.
- `prefers-reduced-motion` honoured for snap-flash, FLIP animations,
  spring eases.
- Keyboard cheatsheet (§1.5) is also the screen-reader-friendly path.

### 1.7 Performance budgets

Targets that we publicly commit to:

| Surface                      | Target                                  |
|------------------------------|-----------------------------------------|
| Cold launch (DMG → workbench)| < 2.0 s on M2 Air                        |
| Warm launch                  | < 800 ms                                 |
| Idle memory (5 columns)      | < 350 MB RSS                             |
| Chat scroll FPS              | 60                                       |
| Canvas pan/zoom (2k shapes)  | 60 (already in `docs/CANVAS.md §14`)     |
| Tree expand 1k entries       | < 50 ms                                  |

A `make perf` script with regression detection (compare to last GA
build's metrics) on each PR.

### 1.8 Telemetry (opt-in)

We don't measure anything today. 1.0 ships **opt-in** anonymous
telemetry with three classes of event:

- App launch + version + theme + connected sources (presence only).
- Per-source connect / disconnect.
- Tool-call success / failure rate by `mcp__*` server (not args, not
  bodies).

Anything richer needs an explicit opt-in beyond the default.

### 1.9 Localisation

Out of scope for 1.0 explicitly. We bake `i18next`-style hooks now so
1.1 can ship strings; default locale stays `en-US`.

### 1.10 Help & docs surfacing

Every module spec in `docs/` should be reachable in-app:

- `?` overlay's "Read the spec" link per surface.
- Settings → "Documentation" panel with rendered Markdown viewer (we
  already render Markdown for stickies / sentry / chat — same component).
- Each spec gets a stable URL on the GitHub repo for external linking.

---

## 2. Per-Module 1.0 Plans

Each subsection is the dedicated 1.0 backlog for that module, on top
of the cross-cutting work above.

### 2.1 Editor → 1.0

#### Already in 0.1

CodeMirror 6, lazy file tree, repo-path pinning, agent linking, Apply-
to-agent, save on `⌘S`, fs watcher, MergeView for git diffs.

#### Gaps to close

1. **Cursor / selection persistence per file.** Re-opening a tab
   should restore line + column, scroll, and any active selection.
   Key: `forgehold:editor:cursors:v1`, scoped to `repoRoot + relPath`.
2. **Right-click menu on tree:** `New file`, `New folder`, `Rename`,
   `Delete`, `Reveal in Finder`, `Copy path`, `Show in Git`. All
   trivially Tauri commands; tree is the only file UI we have.
3. **Find-in-files** via `rg`. Sidebar tab `Search`. Shows matches
   with context, click → open file at line. No semantic search.
4. **Multi-buffer split.** A column can be split horizontally into
   two or three editor panes; each pane has its own `tabs[]` and
   `activePath`. Driven by a thin `EditorPane.svelte` wrapper.
5. **Large-tree virtual scrolling.** Plug `@tanstack/svelte-virtual`
   when a folder has > 1000 immediate children.
6. **"Apply to agent" preview toast.** Show first / last 3 lines and
   line count before sending. User confirm, then mention attaches.
7. **Live agent indicator.** When a linked Claude / Cursor session
   has a tool call in flight that touches the active file, render a
   blinking dot on the tab. State source: `MessageEvent.kind === 'edit'`
   with `status: 'loading'`.
8. **Outline panel** (per-file symbol tree) — defer to 1.1; CodeMirror
   doesn't have a tree-sitter outline today and we don't want LSP.
9. **Theme polish:** ship one usable light theme; current state is
   `editorThemeExtension(name === 'light') → []` (defaults). Build a
   matched-to-the-app-palette light theme.
10. **Format-on-save.** Detect `.prettierrc` / `biome.json` /
    `rustfmt.toml` and run the formatter via Tauri shell. Default off,
    explicit per-repo opt-in.
11. **Read-only revision viewer.** Click a commit in the GitPanel
    history → opens that file revision in a read-only buffer (already
    have `git_show`).
12. **Quick-open** within editor scope: `⌘P` → fuzzy file search in
    the open repo. Distinct from the global Command Palette (which
    indexes columns / tickets).
13. **Empty `repoPath` recovery flow.** Today moving the repo dir on
    disk silently falls back to the welcome card. 1.0: surface "Repo
    moved? `Browse…` to relink".
14. **Consolidate editor state into `editor.svelte.ts`.** Today it's
    fragmented between `EditorView` local state and
    `sessionsState.editorInstanceState`. Single source of truth shakes
    out the sentinel-effect awkwardness called out in
    `docs/EDITOR.md §15.7`.
15. **Git blame gutter.** Per-line `git blame` overlay (lazy-loaded,
    folds older revisions into a cap). Click a line → mini popover
    with author / commit / message + "Open in PR" if the SHA matches
    a known GitHub PR head.
16. **Conflict resolver UI.** Today merge conflicts surface as raw
    `<<<<<<<` markers. 1.0 ships a CodeMirror MergeView-based per-
    file resolver with `theirs / ours / both` quick-keep buttons.
17. **Stash UI.** `git_stash_list / push / pop / drop` Tauri commands
    + a small stash drawer in `GitPanel.svelte`. Useful when the
    agent's worktree gets messy and the user wants a clean slate.

#### Out of scope for 1.0

- LSP integration / IntelliSense.
- Multi-cursor editing beyond CodeMirror's defaults.
- Inline AI completions (the agent column owns that interaction).
- Full project-wide refactor tools.

### 2.2 Agents → 1.0

#### Already in 0.1

Stream parser, edit cards, action cards, mention attach, drag drop,
`linkedToEditor`, worktrees, `linkedCanvasId`, MCP profile filters.

#### Gaps to close

1. **Sessions on disk.** §1.1.
2. **Session search.** Full-text over messages of the active `agentKind`
   in the current column's tab strip. Surfaced in the chat header
   strip as a magnifying glass.
3. **Token / cost meter** per session — sums `usage.inputTokens *
   inputCost + usage.outputTokens * outputCost`. Displayed in the
   chat-header overlay; rates per model in `data.ts`.
4. **Slash commands** as Forge UX (don't pass to the agent verbatim):
   - `/compact` — synonym for the existing compact button.
   - `/clear` — start a fresh thread, archive the old session.
   - `/checkout <branch>` — git checkout in `effectiveCwd`, surface a
     confirm card.
   - `/worktree <branch>` — spawn a worktree for a branch.
   - `/usage` — paste a usage breakdown into the chat (no agent call).
5. **Approval policies per session.** A small JSON in
   `ClaudeSession.approvalPolicy`:
   ```ts
   {
     autoApproveCommitsOn: string[];      // branches; e.g. ["feature/*"]
     autoApprovePushIfBranch: string[];
     autoApproveBashWhitelist: RegExp[];  // serialised pattern strings
   }
   ```
   With sane defaults that match current behaviour (everything
   `pending`).
6. **Failure UX.** Today errored sessions show a red banner saying
   "**Claude failed:** …". 1.0: a structured "Retry" button that
   re-runs the last user prompt, plus expandable stderr.
7. **Compact preview.** Show the proposed summary before applying so
   the user can edit / cancel.
8. **Session export.** Markdown + JSON (full transcript) via "Export
   chat…" in the chat-header overflow. Useful for bug reports and for
   feeding into other tools.
9. **Image attach UX.** Currently paste / drop works, but we don't
   render thumbnails inline in the composer. 1.0: thumbnails strip,
   click to remove, preview on hover.
10. **Mention popover redesign.** Single popover with tabs:
    `Files / GitHub / Jira / Sentry / Chat messages`. Keyboard navigable.
11. **"Save as note"** — convert any chat message to a memory entry
    via `mcp__memory__memory_save` from a context menu.
12. **Stop button progress** — "Cancelling — finishing tool call".
13. **Resume detection.** If the CLI dies mid-tool-call, the next
    `claude_ask` should detect orphan state via `claude_status` and
    do a clean resume instead of a phantom send.
14. **Codex / Aider / Copilot adapters.** Currently `implemented: false`
    in `connectionsMeta`. Move the most-requested one to real (likely
    Codex since OpenAI tooling is common). Each new agent kind needs
    a Rust spawner + a stream normaliser into our `MessageEvent`.
15. **`Edit selection only`** on edit cards — partial keep.
16. **`propose_pr` diff preview.** Show actual diff inline before the
    user clicks "Run". Today the user just sees PR title + body.
17. **Worktree visualization.** When a session has a `worktreePath`,
    a small panel above the chat shows the working diff and "Promote"
    / "Discard worktree" actions.
18. **Per-session model picker** — currently inferred / locked. Let
    the user pick within the kind's available list; persisted.
19. **Stale comment cleanup** (`+page.svelte:3028-3030` "stub flow").
20. **Pin message in context.** A push-pin on any chat message keeps
    it inside the agent's context window even after `/compact` or
    rolling truncation. Distinct from the existing "drag to canvas"
    pin (which is presentation-only).
21. **Pre-send token estimate.** Before clicking Send, the composer
    shows the estimated context size + cost for this turn. Uses the
    model's tokeniser (cached in renderer).
22. **Per-session cost ceiling.** Optional `maxCostUsd` per session;
    when reached, sends are blocked with a clear message until the
    user lifts the cap.
23. **Custom user slash commands.** A small JSON
    (`~/Library/Application Support/Forgehold/slash-commands.json`)
    of `name → expansion template`, with `${selection}` /
    `${cwd}` / `${file}` substitutions. Power-user lever.

#### Out of scope for 1.0

- Voice / dictation.
- Real-time multi-user sessions.
- Branching conversation threads.
- Multi-agent orchestration ("Claude, ask Cursor").

### 2.3 GitHub → 1.0

#### Already in 0.1

PAT auth, inbox column, GitHub tab, slide-over with conversation /
commits / files / checks / reviews, mutations (comment / review /
merge / close), drag to canvas / agent.

#### Gaps to close

1. **OAuth + GitHub App.** §1.2 + extra GitHub-side benefit:
   fine-grained per-repo permissions.
2. **Webhooks** for live updates (§1.4). Optional; falls back to poll.
3. **Per-line review reply UI.** Inline threaded comments in the
   `files` tab.
4. **Reactions UI** on issues / PRs / comments.
5. **Structured filter chips:** assignee, label, milestone, status as
   first-class controls (not buried in `search`).
6. **`o` keyboard shortcut** to open focused row in browser.
7. **Rate-limit display.** When 429 hits, show
   "rate-limited; resets in 12 m". Reads `x-ratelimit-reset` (already
   have).
8. **Pagination > 50.** Load-more in column lists; cursor-based.
9. **Compare branches.** New section in GitHub tab: `compare`. Wraps
   `github_compare`.
10. **Workflow run logs streaming.** Currently we list runs; clicking
    "View logs" goes to browser. 1.0: stream logs in-app.
11. **Release publish** — currently read-only. Add a "Draft release"
    flow.
12. **Repo pinning** — favourites at the top of the repo dropdown,
    persisted per workspace.
13. **Issue / PR create from canvas / agent.** Already have
    `github_create_pr`; surface in canvas right-click menu and
    agent slash command.
14. **Commit-by-commit walkthrough** in PR detail.
15. **Branch protection check** before merge action — surface
    required-reviews / status-checks state.
16. **Cross-repo unified inbox.** A column flavour that pulls from
    multiple repos with one query.
17. **Stale `phase 2` header comment** in `forgehold-github/src/main.rs`
    swept.
18. **Query validation.** A malformed `q` falls through to a generic
    GitHub error (`docs/GITHUB.md §19.6`). 1.0: validate before send,
    surface a parse-error pill that doesn't blow the whole column.
19. **Code search.** GitHub `search/code` API exposed in the GitHub
    tab as a third section beside `code / pulls / issues`. Useful
    when the user remembers a function name but not the file.
20. **Review queue.** A first-class column flavour preset for
    `requested-review:@me`. Today this is a hand-typed query in the
    inbox; 1.0 promotes it to a one-click pill ("My reviews").
21. **Org switcher.** Multi-org users (incl. forks) get a chevron in
    the GitHub tab header listing every org their token sees, with
    repo subtree per org. Today the repo dropdown silently mixes
    orgs.

#### Out of scope for 1.0

- Full code review workflow with suggested-changes.
- GitHub Actions trigger UI.
- Project boards (V2) integration.
- Discussions.
- Sponsors.

### 2.4 Jira → 1.0

#### Already in 0.1

Atlassian Cloud auth (workspace + email + token), filters with sprint /
board / project / status, slide-over with comments / transitions /
worklogs, MD↔ADF.

#### Gaps to close

1. **OAuth (Atlassian 3LO)** §1.2.
2. **Independent polling** §1.4.
3. **Custom fields.** Surface the most common (Story Points, Epic Link,
   Components, Fix Versions) in the slide-over. Schema fetched per
   project; fields hidden when not present.
4. **Sub-tasks tree** in the slide-over.
5. **Attachments upload + inline preview.**
6. **Reactions on comments.**
7. **@mention picker** in comment composer (uses
   `list_assignable_users`).
8. **Linked-issues panel** (blocks / blocked-by / relates-to / clones).
9. **Inline status / assignee edit** in the column rows (current state
   requires opening the slide-over).
10. **Sprint board view** (kanban per sprint) as a Jira tab section.
11. **Burndown / velocity widgets** in the same tab.
12. **Bulk operations:** multi-select rows, batch assign / transition.
13. **Saved filter sets** — name a `JiraFilters` snapshot, restore.
14. **Raw JQL field** for power users (in the column header behind a
    chevron).
15. **JQL query history.**
16. **Jira Server / DC support.** Different REST surface (`/rest/api/2/`);
    Forge probes which one works and adapts.
17. **Multi-org support.** `JiraConnections[]` instead of single
    `JIRA_KEY`.
18. **Notifications** (Atlassian-side notifications mirrored in
    Forgehold's badge).
19. **MD↔ADF parity.** Round-trip is lossy for Jira-specific blocks
    (panels, info / warning callouts, expand cards) per
    `docs/JIRA.md §15.6`. 1.0: extend the converter to preserve those
    blocks instead of flattening them; keep "user pasted exotic ADF"
    as a known-loss with a toast warning.
20. **Project quick-switch.** `⌘G P` (or palette action) pops a tiny
    project picker that retargets the active Jira column without
    opening the slide-over. Mirrors the GitHub repo dropdown.
21. **Issue templates.** A small `templates.json` of common shapes
    (Bug, Spike, Tech-debt) prefilled when the user clicks
    "+ New issue". Per-project; team can ship presets via export.
22. **Recent issues MRU.** A "Recently viewed" section at the top of
    the Jira column when no filter is active. Surfaces last 10
    issues touched.

#### Out of scope for 1.0

- Confluence integration.
- Roadmaps / advanced planning.
- JSM (Jira Service Management) features.

### 2.5 Sentry → 1.0

#### Already in 0.1

Token auth, filters (project / env / status / level / sort), unified
slide-over (issue + event), set-status mutations, drag to canvas /
agent.

#### Gaps to close

1. **Polling tick** §1.4 — Sentry currently has zero auto-refresh.
2. **Comment UI in slide-over.** Today `add_comment` is MCP-only.
3. **Assignee picker.**
4. **Multi-org** §2.4.17 equivalent.
5. **"Open in Editor at line"** — clicking a stack frame whose path
   matches the linked editor's repo opens the file at the line. Needs
   a path-mapping config (e.g. `src/foo.ts` in stack → `<repo>/src/foo.ts`).
6. **Bulk operations** — mark resolved on N selected issues.
7. **Snooze / mute** — currently only `resolved | unresolved | ignored`;
   add temporary mute with auto-unmute.
8. **Issue → Jira ticket** quick-create, with prefilled summary +
   stack frame.
9. **Release health** widget in `SentryTab` for the connected projects.
10. **Discover queries** (basic): saved queries that aren't
    issue-scoped.
11. **Environment list per org** (currently global cache, see TODO in
    `inbox.svelte.ts:1392-1401`).
12. **Frame source preview.** `event.exceptions[].frames[].source_lines`
    rendered as a tiny code block (like the current `<details>` but
    syntax-highlighted via CodeMirror).
13. **Reactions / discussion** — Sentry has comment reactions; surface.
14. **Issue-level "open in browser" / release surface.** Today only
    raw tags are shown (`docs/SENTRY.md §16.5`). 1.0: per-issue
    release pill linking to the Sentry release page + an "open in
    Sentry" button.
15. **Spam / false-positive feedback** (`docs/SENTRY.md §16.6`). A
    "mark as not a bug" affordance that calls Sentry's ignore /
    archive APIs with a reason.
16. **Typed `metadata_value` rendering.** Today we render the raw
    string for every `metadata_type` (`docs/SENTRY.md §16.8`). 1.0:
    detect `event_type === 'transaction'` / `csp` / `expectct` and
    render structured panes per type rather than dumping JSON.

#### Out of scope for 1.0

- Performance / transaction view.
- Full Discover query builder.
- Alert rule editing.
- Custom dashboards.
- Replay viewer.

### 2.6 Workbench → 1.0

#### Already in 0.1

Multiple workbenches, drag-drop, snap-resize, archive, maximize, v1→v3
migration.

#### Gaps to close

1. **Workbench presets as files.** Save / load from disk
   (`~/Library/Application Support/Forgehold/workbenches/<id>.json`),
   shareable. Useful for "Auth-debugging" preset etc.
2. **FLIP-animated reorder** (drop-to-tab and within-bench moves).
3. **Floating column** mode — pin a single column above the bench
   while you switch workbenches. Useful for "always-visible Sentry
   triage".
4. **Multi-monitor support.** Drag a column to a second display →
   detached window with the same column rendering.
5. **Resize: double-click handle to reset width** to default.
6. **Maximize keyboard shortcut.** Today only the toolbar button +
   Esc to restore. Bind `F` (Figma) or `⌘⇧M`.
7. **Workbench-level hotkeys** `⌘1..⌘9` to switch tabs.
8. **Empty-workbench hint** ("drag a chip to add a column") when an
   active workbench has zero instances.
9. **Workspace theme override** per workbench (e.g. "Auth" workbench
   in red accent so it's visually distinct).
10. **Snap regimes editable.** Currently hardcoded thresholds; expose
    "snap to neighbours" / "snap to fractions" / "snap to grid"
    toggles. Also re-tune the default: per `docs/WORKBENCH.md §17.5`
    "snap to neighbour width" feels too eager when the user wants
    "slightly bigger than X". Add a small dwell delay before snap
    engages.
11. **State export / import** — for support.
12. **Migration cleanup** — drop the v1 path after one major bump
    (we've been carrying it).
13. **Stale comments swept** (the singleton-not-singleton comments in
    `+page.svelte` and the `forgehold-app` MCP descriptions).
14. **Vertical split inside one column.** Rare but useful: split an
    Editor column horizontally so two files sit one above the other,
    or stack a Sentry slide-over above the Sentry list. Implemented
    as `PanelInstance.split: { axis: 'h', children: [...] }`. Saves
    horizontal real estate on multi-column laptops.

#### Out of scope for 1.0

- Multi-user shared workbenches.
- Public web preview.
- Workbench versioning / undo.

### 2.7 Connections → 1.0

#### Already in 0.1

PAT/token auth, Keychain storage, per-source connect modal, status
booleans, biometry first-launch.

#### Gaps to close

1. **OAuth** §1.2 across GitHub / Atlassian / Sentry.
2. **Multi-account per source.** Add "Add another GitHub" inside the
   modal. Stored as `keychain[github:1]`, `keychain[github:2]`, etc.
3. **Slack** real integration (currently `implemented: false`).
4. **Linear** real integration.
5. **GitLab** real integration (mirror of GitHub flow; same
   read/write/propose-* shape).
6. **Per-source "Test connection"** button that reruns `*_status()`
   and surfaces the precise failure mode.
7. **Quota / rate-limit visibility** per source: GitHub remaining /
   reset, Jira `Retry-After`, Sentry response codes.
8. **Token rotation reminder** — if the credential we have has an
   inferable expiry (GitHub tokens optionally), warn N days ahead.
9. **Biometry on every launch** as a setting.
10. **Encryption at rest** beyond Keychain — defence in depth for
    laptops with Keychain unlocked but no FileVault.
11. **Connection diagnostics page** — full status with verbose
    error / response codes per source. Linked from "Report bug".
12. **Auto-detect dev creds** from `.env` files in the open repo as a
    suggestion (never auto-import).
13. **Workspace identity badge** — current account email visible in
    the rail. Wraps `docs/CONNECTIONS.md §11.4` ("`claudeStatus` /
    `cursorStatus` show binary path + version, not 'logged in as'").
    For agents we surface whatever identity the CLI exposes (Claude:
    auth account; Cursor: active workspace) and label "unknown" when
    the CLI doesn't report it.
14. **Retry / backoff on `*_status()` boot calls.** Today a single
    network blip on launch leaves a source disconnected until the
    user reconnects manually (`docs/CONNECTIONS.md §11.1`). 1.0:
    exponential backoff up to 4 attempts in the first 30 s with a
    "Retrying…" state in the rail.
15. **Notion / Codex / Aider / Copilot / Asana / Teams** — beyond 1.0
    unless one is requested loud enough.
16. **Connection event log.** Persisted history per source: connected
    at, disconnected at, last token-refresh, last 429, last error.
    Surfaced in the diagnostics page (§2.7.11). Critical for "why did
    my Jira column go dark at 3 AM" debugging.

#### Out of scope for 1.0

- SSO / Forgehold-account model.
- Cross-device session sync.
- Encrypted shared workspace creds for teams.

### 2.8 Command Palette → 1.0

#### Already in 0.1

⌘K toggle, substring search, fixed sections, keyboard nav.

#### Gaps to close

1. **Fuzzy search** (typo-tolerant). Either `fzf-style` scoring or
   `fuzzysort` (small dep). Improves discoverability for everyone
   typing "githb".
2. **Relevance ranking.** Today section order is the only signal
   (`docs/COMMAND_PALETTE.md §10.2`). 1.0: per-row score = `fuzzyScore
   * recencyBonus * frequencyBonus`, then sort within section.
3. **MRU bias.** Track last 50 picks per workspace, apply a small
   score boost for recent items.
4. **"+N more"** footer per section when the cap of 6 hides items.
5. **Section nav** — `⌘↓` / `⌘↑` to jump headers.
6. **Action verbs:**
   - `Create new workbench`, `Open new GitHub column`,
   - `Connect GitHub`, `Disconnect Sentry`,
   - `Open settings`, `Open theme picker`,
   - `Show keyboard cheatsheet`,
   - `Report bug`.
7. **File search via `rg`.** New section "Files in active editor".
8. **Chat session search** — full-text. Requires §2.2.2 but works
   from this side too.
9. **Slash commands inside the palette** so the `/compact` etc.
   shortcuts work even when the agent column doesn't have focus.
10. **Hover preview.** On the right-hand side, show a tiny pane with
    the focused row's context (PR title + body, ticket summary, file
    first 10 lines).
11. **Pinned items.** Star a row to keep it on top.
12. **Recent items section** at the very top when query is empty.
13. **Theme picker** inline (no need to dive to Settings).
14. **Breadcrumbs** for nested objects (Jira board → sprint → issue).

#### Out of scope for 1.0

- Plugin command extensions.
- Multi-step "wizard" commands.
- Voice commands.
- LLM-augmented query rewriting.

### 2.9 MCP → 1.0

#### Already in 0.1

5 sidecars, profile filters, `--mcp-config` temp file, `~/.cursor/mcp.json`
merge, env-injected tokens.

#### Gaps to close

1. **Auto-restart on crash** with exponential backoff and a status
   indicator in `SettingsView`.
2. **Per-tool metrics** — invocation count, latency p50/p95, error
   rate. Persisted; shown in `SettingsView` and surfaced when an
   agent reports tool failures.
3. **Live profile switching.** Today requires session stop / start.
   1.0: write a new `--mcp-config` and either signal Claude to reload
   or transparently restart the CLI sidecar with a session resume.
4. **`mcp_auth` flow** for OAuth-needing servers (Slack, Linear,
   anything with a refresh-token round-trip). Even though we don't
   ship those servers in 1.0, shape the flow now.
5. **`forgehold-memory` descriptors** — add `tools/*.json` so the
   user's IDE shows schemas (currently only `STATUS.md`).
6. **Tool descriptor sweep** — fix stale `read-only phase 2` and
   `singleton` comments.
7. **Multi-instance credentials** — when §2.7.2 lands, MCP env per
   session needs to pick the right token.
8. **Server health panel** in `SettingsView`.
9. **Cursor IDE prefix normalisation sweep** with each Cursor release.
10. **Tool-call audit log** per session, exportable. Helps when a tool
    misbehaves silently.
11. **Tool descriptions** as Markdown (currently JSON-Schema only) so
    the agent has richer context.
12. **Tool dry-run / preview.** A "would call X with args Y" mode the
    agent can use to surface intent without touching the wire. Helps
    debugging, helps `propose_*` cards become richer (the action card
    can show the dry-run output).

#### Out of scope for 1.0

- User-defined custom tools.
- WASI / sandbox isolation for sidecars beyond Tauri's process model.
- Tool versioning / migration.

### 2.10 Canvas → 1.0

The Canvas spec already plans through `M-canvas-9` (`docs/CANVAS.md §16`).
For Forgehold 1.0 we ship M-canvas-1 through M-canvas-9 in full plus
1.0 polish:

1. **Auto-save indicator** — small pulse on the column header when a
   write is in flight, last-saved timestamp on hover.
2. **Cross-canvas search** in the library overlay.
3. **Comment / annotation on shapes** (tiny pin → speech bubble).
4. **Versioning / history snapshots** — snapshot button creates an
   immutable copy you can diff against later.
5. **Open / save / import / export polish** — drag a `.canvas.json`
   onto Forgehold from Finder → opens it.
6. **Live presence indicator** for the agent — when an agent is
   editing the canvas, render a soft halo on its current operations.
7. **Smart guides v2** — show distance pills (`12 ↔ 12`) per
   `docs/CANVAS.md §4.4`.
8. **Locked-shape unlock prompt** when the user tries to edit one
   (currently silently no-ops).
9. **Performance pass** — hit the §14 targets across realistic scenes.
10. **Reduced-motion** for spring animations (§1.6).

#### Out of scope for 1.0

Already enumerated in `docs/CANVAS.md §17` — keep them deferred.

---

## 3. Cross-Module Features

Features that touch multiple modules at once; promoted here so we
don't double-count them in the per-module backlogs above.

### 3.1 Linked editor — agent — canvas trio

Already partly there: editor ↔ agent (§6 of `docs/EDITOR.md`), agent
↔ canvas (`docs/AGENTS.md §17.1`). 1.0 closes the triangle:

- Editor → canvas — "Pin this file" to active linked canvas → adds a
  `file-card`.
- Canvas → editor — `file-card` double-click already opens; 1.0 adds
  "Open in linked editor" in the right-click menu.
- Agent canvas summary preamble feeds the active editor file's name +
  cursor line so "look at the function I'm in" works.

### 3.2 Cwd switch as first-class concept

`applySessionCwd` in `docs/AGENTS.md §9` rotates `claudeUuid` and
writes `cwdSwitchRecap`. 1.0 makes this visible:

- Banner inside the chat: "cwd changed from X to Y because you
  switched the editor's repo". Click → "Undo".
- A small graph icon next to a session showing every cwd it's been
  through. Useful when worktrees pile up.

### 3.3 Mention model unification

`Mention` is split per source today (`Mention.source = 'file' |
'github' | 'jira' | 'sentry' | 'chat-message'`). A single "anything
goes" mention model would let canvas shapes, sentry frames, and
arbitrary URLs become first-class. 1.0:

- Add `Mention.source = 'canvas-shape'` and `Mention.source = 'url'`.
- Mention popover (§2.2.10) drives off this unified type.
- Canvas drag → agent column produces a canvas-shape mention with the
  shape's compact JSON.

### 3.4 Worktrees explicit lifecycle

Today they're created on demand, lurk on disk, and are listed in
`SettingsView`. 1.0:

- Worktree creation modal: name, base branch, agent.
- `WorktreeDiffModal` (`docs/AGENTS.md §17.2`) becomes a tab on the
  agent column with "Promote to main" / "Discard" / "Open Editor on
  this worktree" actions.
- Cleanup chore in `SettingsView` already exists; auto-suggest old
  worktrees > 30d.

### 3.5 Drag-and-drop graph

A single document — `docs/DRAG.md` (new) — that enumerates every
`source × target` pair and what happens. Today the rules are
implicit, scattered across `+page.svelte` `onDragStart`/`onAgentDrop`
and `CanvasColumn.svelte` `insertLiveCard`. Documenting forces us to
fix the asymmetries (e.g. drop-on-pill works for some kinds, not
others).

---

## 4. Phasing & Milestones

Sized for a small team (1–2 engineers full-time on Forgehold). Each
phase is "shippable" — the app is stable at the end of every phase,
just with fewer 1.0 boxes ticked.

### M1 — Durability foundation (≈ 3 weeks)

- §1.1 Persistence to disk for sessions + canvases (already half-on for
  canvases).
- §1.3 Auto-update + crash reporting + Apple notarisation.
- §1.4 Per-source polling decoupled.
- All "stale comment" sweeps across modules.
- M-canvas-1 if not already shipped.

**Shippable as 0.2.**

### M2 — OAuth + Connections polish (≈ 2 weeks)

- §1.2 GitHub OAuth, Atlassian OAuth, Sentry OAuth (best-effort each).
- §2.7.6 Test-connection button.
- §2.7.7 Quota visibility.
- §2.7.11 Connection diagnostics.

**Shippable as 0.3.**

### M3 — Canvas v1 (≈ 6 weeks, parallel-friendly)

- M-canvas-2 through M-canvas-9 per `docs/CANVAS.md`.
- §2.10 Canvas 1.0 polish at the end.

**Shippable as 0.4.**

### M4 — Module 1.0 backlogs (≈ 4 weeks)

- Top items from §2.1, §2.2, §2.3, §2.4, §2.5, §2.6, §2.8, §2.9 ranked
  by user demand. Cut anything that's not blocking 1.0 to 1.x.

**Shippable as 0.5.**

### M5 — Polish & 1.0 GA prep (≈ 2 weeks)

- §1.5 Onboarding + cheatsheet.
- §1.6 A11y audit.
- §1.7 Performance budget enforcement.
- §1.8 Telemetry opt-in pipeline.
- §1.10 In-app docs viewer.
- Final bug bash.

**Shippable as 1.0.**

Total: **17 weeks** end-to-end if linear; **12 weeks** with M3 running
parallel to M2/M4 (canvas is enough surface area to be its own track).

---

## 5. Definition of Done — 1.0 Criteria

Forgehold is 1.0 when **all** of the following are true:

1. **Tokens:** OAuth available for ≥2 of {GitHub, Atlassian, Sentry};
   PAT fallback for the rest.
2. **Updates:** Auto-update channel live, signed + notarised binaries.
3. **Crashes:** Captured by an internal Sentry project; mean
   time-to-fix-after-report < 1 week.
4. **Persistence:** Sessions on disk, surviving localStorage quota
   blow-outs, with a documented cold-store / restore path.
5. **Polling:** Each connected source self-refreshes on its own
   schedule; user can override.
6. **Canvas:** M-canvas-1 through M-canvas-9 shipped; meets
   `docs/CANVAS.md §14` perf targets; 1.0 polish §2.10 done.
7. **Onboarding:** First-launch tour exists; ≥80% of new users complete
   at least the Theme + Source steps in internal beta.
8. **A11y:** axe-core passes on every primary surface; reduced-motion
   honoured; keyboard cheatsheet exists.
9. **Perf:** Hits §1.7 budgets in CI on a reference machine.
10. **Stale-comment debt:** All known stale comments enumerated in this
    doc swept.
11. **Documentation:** `docs/*.md` matches behaviour; in-app docs
    viewer reachable; "Report bug" form bundles logs.
12. **Telemetry:** Opt-in path live, dashboards exist for the team.
13. **Per-module Open TODOs:** Each module's "Open TODOs" section in
    its spec contains nothing the team considers a 1.0 blocker.

---

## 6. Out of Scope for 1.0 (deferred to 1.x)

Maintained here so we can say "no" without re-deriving why.

- Real-time multi-user editing (Canvas / Workbench).
- Public web preview / share links.
- Slack / Linear / Notion / GitLab / Teams / Asana / Codex / Aider /
  Copilot beyond whichever **two** integrations §2.7 ships.
- LSP / IntelliSense in Editor.
- Performance / transactions in Sentry.
- Confluence / advanced Jira features.
- Mobile / iPad version.
- LLM-driven query rewriting in palette.
- Plugin system anywhere.
- Localisation beyond `en-US`.
- Workspace identity / Forgehold account model.

---

## 7. Risks & Open Questions

1. **OAuth client registration delay.** Atlassian's OAuth approval is
   weeks, not days — start the application **before** M2 starts.
2. **Notarisation cost.** Apple Developer Program is annual; budget
   it. Without it M1 can't fully ship.
3. **Memory MCP descriptors gap.** Whether to ship without
   documenting the schema risks the agent calling unknown tools. Fix
   in M1 sweep (§2.9.5).
4. **Quota fallout** during M1 disk migration. Some users will already
   be at quota; the migration tool must run before localStorage purge,
   not after, and must surface "couldn't migrate this session"
   gracefully.
5. **Canvas perf at high zoom.** Mermaid + freehand at 0.1 zoom
   stresses GPU compositing. Spike a worst-case scene before
   committing to the §1.7 budget for canvas.
6. **Cursor CLI flag stability.** `--print --output-format stream-json`
   etc. are unofficial; pin to a known working version and surface a
   warning if the user has a newer Cursor.
7. **Telemetry consent UX.** Get this wrong and we get bad press;
   default-off, explicit opt-in, no surprises.
8. **Codex / OpenAI parity.** If we promote a third agent kind, the
   stream-normaliser becomes a non-trivial backlog (their CLI is
   different shape). Consider keeping it 1.x.

---

## 8. Tracking

A shadow GitHub Project (or whatever) with one card per bullet here.
Each card links back to the sub-section in this doc. Issues are not
the source of truth — this doc is. Issues track **execution**.

Per-section status badges added inline as we ship:

- `[ ]` → pending
- `[~]` → in flight
- `[x]` → done
- `[~1.x]` → deferred to 1.x

---

## 9. Glossary

- **0.1** — current shipped state as of this commit.
- **1.0** — release target defined by §5.
- **1.x** — anything intentionally deferred from 1.0 (§6 + per-module
  out-of-scope).
- **Shippable** — the app is stable at this milestone end; not all
  1.0 criteria met yet but no regressions vs 0.1.
- **Stale comment** — code comment whose described behaviour no
  longer matches the implementation. We have a pile of these; M1
  sweeps them.
- **Cross-cutting** — a theme that touches every module; lives in §1
  rather than per-module sections.
