# Forge — Technical Specification

**Version:** 0.1
**Last updated:** 2026-04-25
**Status:** alpha — implementation matches this spec where noted; cuts and
revisions are summarized in [MVP.md](MVP.md). Read MVP.md for the
current scope; this doc describes the intended architecture.

> **Important:** this is the **historical** intended-architecture
> document from the early planning phase. The current source of truth
> is the per-module spec set (`AGENTS.md`, `CANVAS.md`,
> `CONNECTIONS.md`, `EDITOR.md`, `GITHUB.md`, `JIRA.md`, `MCP.md`,
> `SENTRY.md`, `WORKBENCH.md`, `COMMAND_PALETTE.md`) plus
> `ROADMAP_1.0.md` for what's planned. Several decisions in this
> document have since been **reversed or made permanent**:
>
> - **OAuth (§6.1, §6.2, §6.5).** Cancelled — Woom is PAT-only
>   forever (`docs/ROADMAP_1.0.md §6`). Ignore the OAuth flow tables
>   and `forge://` URL scheme; we use macOS Keychain with manual PAT
>   entry, no redirect handler.
> - **SQLite primary store.** Replaced with disk-backed JSON for
>   sessions / canvases (M1 of `ROADMAP_1.0.md`) and localStorage for
>   small UI prefs.
> - **Slack source.** Listed in `FUTURE_FEATURES.md §A.1`, not 1.0.
> - **Per-scope rules parser.** Replaced with a single global rules
>   string (`RulesView.svelte`).
> - **Saved workflows / `.forge.yaml`.** Deferred to
>   `FUTURE_FEATURES.md §E.1–§E.2`.
>
> Anything in this doc that doesn't contradict the per-module specs
> or `ROADMAP_1.0.md` still represents the plan. When in doubt, the
> per-module spec wins.

---

## 1. Vision & Non-Goals

### Vision

Provide an interface where an engineer drags **work objects** (tickets, PRs,
messages, comments) onto **actions** (AI agent, send, remind, transform),
and Forge orchestrates everything through a common protocol — without the
user having to think about APIs, tokens, or formats.

### Goals (v0.1)

1. A unified inbox of objects from Jira and GitHub.
2. Drag-and-drop workflow: ticket / file / PR → Claude or Cursor.
3. Local credentials encrypted in the macOS Keychain with Touch ID gating.
4. Architecture ready for a team use case (post-v1; v0.1 is single-player).
5. Claude / Cursor invoked as headless CLIs, sidecars exposed via MCP.

### Non-Goals (v0.1)

- Mobile app.
- Cloud sync between devices. (Coming in v0.3.)
- ~~A built-in code editor~~ — **Reversed 2026-04-23.** Editor is in scope
  (CodeMirror 6, file tree, git panel, diff). Zed / VS Code remain
  reachable via MCP.
- Our own LLM. We use Claude / Cursor.
- A long list of integrations — kept narrow. Slack moved to post-v1.
- A visual no-code builder for complex workflows. A workflow is a linear chain.
- **Windows and Linux builds.** v0.1 ships macOS only; cross-platform is
  post-v1.0. Every design decision below assumes macOS semantics (see §13).

---

## 2. Core Concepts

The whole system rests on seven entities (five core + two for local work):

### 2.1 Source

An instance of an integration. For example, "my Jira account at
acme.atlassian.net". A Source owns credentials and knows how to fetch
objects and execute actions.

### 2.2 Object

A unit of work pulled from a Source. Universal interface:

```
Object {
  id: String               // Forge-global, not external
  source_id: String
  external_id: String      // "PROJ-1234", "owner/repo#5", "CXXX/ts=..."
  kind: ObjectKind         // Ticket | PullRequest | Issue | Message | Thread | Comment | Commit
  title: String
  body: Option<String>
  url: String              // deep-link back to the original service
  author: Option<Actor>
  metadata: Json           // source-specific fields
  relations: Vec<Relation> // "linked_to", "replied_to", ...
  fetched_at: Timestamp
  updated_at: Timestamp
}
```

An Object is **not** copied into Forge as authoritative — it's a projection
of external state with a TTL. The source of truth is Jira/GitHub/Slack.

### 2.3 Action

Something you can do with an object. An action has:

- `input_kinds` — which Object kinds it accepts (drop zones filter on this)
- `output_kind` — what it produces (Object, Artifact, or void)
- `params_schema` — JSON Schema for configuration (e.g. target channel)
- `executor` — how it runs (MCP tool call, native, Claude agent)

Example actions:
- `claude.implement` — input: Ticket/Issue, output: Artifact (diff)
- `slack.post_message` — input: Object | Artifact, output: Message
- `jira.add_comment` — input: Ticket + Object | Artifact, output: Comment
- `github.draft_pr` — input: Artifact (diff) + Ticket, output: PullRequest
- `reminder.schedule` — input: Object | Artifact, output: void

### 2.4 Workflow

A named, saved chain: `[Object] → Action → Action → ... → terminal`.

```
Workflow {
  id: Uuid
  name: String
  trigger: Trigger        // Manual | Scheduled | EventBased
  steps: Vec<Step>
  workspace_id: Uuid
  owner_id: Uuid
  shared: bool
}

Step {
  action_id: String
  params: Json
  on_error: ErrorPolicy   // Stop | Continue | Prompt
}
```

### 2.5 Run

A single execution of a workflow. Holds logs, intermediate results,
artifacts.

```
Run {
  id: Uuid
  workflow_id: Option<Uuid>   // None for ad-hoc runs
  triggered_by: Uuid
  status: RunStatus           // Queued | Running | Paused | Done | Failed | Cancelled
  input_object_id: String
  current_step: usize
  artifacts: Vec<ArtifactId>
  started_at: Timestamp
  finished_at: Option<Timestamp>
}
```

### 2.6 Artifact

The output of a step that isn't an Object — a code diff, a message draft,
a screenshot, generated text.

```
Artifact {
  id: Uuid
  run_id: Uuid
  kind: ArtifactKind   // Diff | TextDraft | Binary | StructuredData
  mime: String
  content: Bytes | Ref  // inline for small, file-ref for large
  preview: Option<String>
  pinned: bool          // protection from auto-cleanup
}
```

Artifacts themselves become inputs to subsequent actions — they're
draggable like any other object.

### 2.7 Repository

A local clone of a git repo that Forge tracks. Separate from the GitHub
Source (which is about the API), a first-class entity. Provides the target
for Claude runs via worktrees. Details in [REPOS.md](REPOS.md).

### 2.8 Rules

Declarative rules for Claude runs with four scopes: `global`, `repo`,
`folder:<path>`, `run`. Composed into a prompt prefix. Details in
[RULES.md](RULES.md).

---

## 3. Technical Stack

| Layer               | Choice                         | Rationale                                                  |
|---------------------|--------------------------------|------------------------------------------------------------|
| Target platform     | **macOS 13+**, Universal `.app`| See §13. MVP is macOS-only.                                |
| Desktop shell       | **Tauri 2**                    | Rust core, native WebKit, clean `.app` bundle + code sign  |
| Backend language    | **Rust**                       | Type safety, performance, good async                        |
| Frontend framework  | **SvelteKit (SPA mode)**       | Compact bundle, good animation out of the box              |
| Styling             | **Tailwind + CSS vars**        | Fast + design tokens via vars                              |
| Motion              | **Motion One** (or Framer)     | Spring physics, lightweight                                |
| Local DB            | **SQLite** (sqlx)              | Zero-config, has everything we need                        |
| Key-value cache     | **SQLite** (separate table)    | Not pulling in Redis for local use                         |
| Secret storage      | **macOS Keychain Services** (keyring crate) | Never plaintext tokens on disk               |
| MCP                 | `rmcp` crate (Rust MCP SDK)    | Proven, fork only if streaming needs it                    |
| Agent SDK           | Claude Agent SDK (Node.js)     | Stable SDK, headless mode; bundled via sidecar if embedded |
| IPC Tauri ↔ UI      | Tauri commands + events        | Standard                                                    |
| MCP sidecar runtime | **Rust native** (preferred)    | No system Node.js dependency; bundled in `.app`            |
| Team backend (v0.3) | Rust + axum + Postgres         | Stack symmetry, self-hostable                              |
| Sync protocol (v0.3)| CRDT (Automerge) or event log  | Offline-first is mandatory                                 |

---

## 4. Architecture

### 4.1 High-level diagram

```
┌─────────────────────────────────────────────────────────────────┐
│                       FORGE DESKTOP APP                         │
│                                                                 │
│  ┌──────────────────────┐   ┌──────────────────────────────┐    │
│  │                      │   │                              │    │
│  │  SvelteKit UI        │◄──┤  Tauri core (Rust)           │    │
│  │  (webview)           │   │                              │    │
│  │                      │   │  ┌────────────────────────┐  │    │
│  │  - Workbench         │   │  │ forge-core (domain)    │  │    │
│  │  - Inspector         │   │  │ forge-db  (SQLite)     │  │    │
│  │  - Command palette   │   │  │ forge-auth (keychain)  │  │    │
│  │  - Timeline          │   │  │ forge-runtime (exec)   │  │    │
│  │                      │   │  └────────┬───────────────┘  │    │
│  └──────────────────────┘   │           │                  │    │
│                             │           ▼                  │    │
│                             │  ┌────────────────────────┐  │    │
│                             │  │ forge-mcp (host+client)│  │    │
│                             │  └────────┬───────────────┘  │    │
│                             └───────────┼──────────────────┘    │
└─────────────────────────────────────────┼───────────────────────┘
                                          │ (stdio/socket)
              ┌───────────┬───────────────┼───────────┬──────────────┐
              ▼           ▼               ▼           ▼              ▼
       ┌────────────┐ ┌──────────┐ ┌────────────┐ ┌────────────┐ ┌──────────┐
       │ forge-jira │ │forge-gh  │ │forge-slack │ │forge-claude│ │ user's   │
       │ MCP server │ │MCP server│ │MCP server  │ │MCP wrapper │ │ custom   │
       └─────┬──────┘ └────┬─────┘ └─────┬──────┘ └─────┬──────┘ │ MCP      │
             │             │             │              │        └──────────┘
             ▼             ▼             ▼              ▼
         Jira API      GitHub API    Slack API   Claude Agent SDK
```

### 4.2 Process model

Every MCP server is a separate child process, launched lazily on first
use, kept alive with a 5-minute idle timeout. The Tauri core owns the
lifecycle (spawn, health check, restart).

### 4.3 MCP layer

**Forge as an MCP host**: connects to its own MCP servers (jira/github/
slack) and uses their tools for native actions (without an LLM).

**Forge as an MCP server**: Forge exposes an MCP API outward. This lets:
- Claude Code in Zed invoke Forge tools and fetch object context.
- Other agents and scripts do the same.
- Third-party MCP hosts (Cursor, Windsurf) access workflows.

**forge-claude MCP wrapper**: when a workflow step is a Claude action,
Forge starts the Claude Agent SDK in headless mode and gives the agent
access to its MCP servers plus the user's repo. The agent writes code,
commits to a worktree, returns a diff as an Artifact.

### 4.4 Claude Code integration (deep)

Two paths:

**Path A — embedded agent:** Forge starts the Claude Agent SDK itself.
Pros: full control, unified UX. Cons: duplicates functionality already in
the `claude` CLI.

**Path B — bridge to the existing Claude Code:** Forge invokes the `claude`
CLI in headless mode (`-p` / `--print`) with the required MCP servers.
Pros: the user already configured `~/.claude/settings.json`, no need to
duplicate. Cons: the CLI is not always convenient for UI streaming.

**Decision for MVP:** Path B, with fallback to Path A if the `claude` CLI
isn't found. Streaming output is read via Tauri sidecar and relayed as
stream events to the UI.

### 4.5 Execution runtime

`forge-runtime` is a state machine for a Run:

```
Queued → Running → (StepStart → StepRun → StepEnd)* → Done
                       │             │
                       │             ▼
                       │          Failed → (retry?) ↺
                       ▼
                    Paused (user prompt: "approve next step?")
```

Run state persists in SQLite and survives application restarts.

---

## 5. Data Model

### 5.1 Schema (SQLite DDL, simplified)

```sql
-- Multi-tenancy from day one
CREATE TABLE workspace (
  id        TEXT PRIMARY KEY,
  name      TEXT NOT NULL,
  owner_id  TEXT NOT NULL,
  created_at INTEGER NOT NULL
);

CREATE TABLE member (
  id          TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL REFERENCES workspace(id),
  user_id     TEXT NOT NULL,
  role        TEXT NOT NULL,  -- owner | admin | member | viewer
  joined_at   INTEGER NOT NULL
);

CREATE TABLE source (
  id           TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL REFERENCES workspace(id),
  kind         TEXT NOT NULL,   -- jira | github | slack | claude
  config       JSON NOT NULL,   -- non-secret: host, org, default channel
  shared       INTEGER NOT NULL, -- 0 = personal, 1 = team
  created_by   TEXT NOT NULL,
  created_at   INTEGER NOT NULL
);

-- Tokens stored separately, keychain-ref in DB, never plaintext
CREATE TABLE credential (
  source_id  TEXT PRIMARY KEY REFERENCES source(id),
  keychain_ref TEXT NOT NULL,  -- "forge:source:<uuid>"
  scope      TEXT NOT NULL,
  expires_at INTEGER,
  refresh_keychain_ref TEXT
);

CREATE TABLE object_cache (
  id           TEXT PRIMARY KEY,       -- Forge global id
  source_id    TEXT NOT NULL,
  external_id  TEXT NOT NULL,
  kind         TEXT NOT NULL,
  payload      JSON NOT NULL,
  fetched_at   INTEGER NOT NULL,
  ttl_seconds  INTEGER NOT NULL,
  UNIQUE (source_id, external_id)
);

CREATE TABLE workflow (
  id           TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL,
  owner_id     TEXT NOT NULL,
  name         TEXT NOT NULL,
  definition   JSON NOT NULL,
  shared       INTEGER NOT NULL,
  created_at   INTEGER NOT NULL,
  updated_at   INTEGER NOT NULL
);

CREATE TABLE run (
  id            TEXT PRIMARY KEY,
  workspace_id  TEXT NOT NULL,
  workflow_id   TEXT,
  triggered_by  TEXT NOT NULL,
  status        TEXT NOT NULL,
  input_object  TEXT,
  state         JSON NOT NULL,        -- full state machine: current_step, step results
  started_at    INTEGER NOT NULL,
  finished_at   INTEGER
);

CREATE TABLE artifact (
  id       TEXT PRIMARY KEY,
  run_id   TEXT NOT NULL REFERENCES run(id),
  step_idx INTEGER NOT NULL,
  kind     TEXT NOT NULL,
  mime     TEXT NOT NULL,
  content  BLOB,              -- inline or NULL if file_ref
  file_ref TEXT,              -- path inside app data dir
  preview  TEXT,
  created_at INTEGER NOT NULL
);

CREATE TABLE notification (
  id           TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL,
  user_id      TEXT NOT NULL,
  title        TEXT NOT NULL,
  body         TEXT,
  trigger_at   INTEGER,        -- unix ts; NULL = immediate
  linked_object TEXT,
  status       TEXT NOT NULL,  -- pending | fired | dismissed
  created_at   INTEGER NOT NULL
);
```

```sql
-- Local git repos (see REPOS.md for full detail)
CREATE TABLE repository (
  id                TEXT PRIMARY KEY,
  workspace_id      TEXT NOT NULL REFERENCES workspace(id),
  name              TEXT NOT NULL,
  remote_url        TEXT,
  local_path        TEXT NOT NULL UNIQUE,
  default_branch    TEXT NOT NULL,
  linked_source_id  TEXT REFERENCES source(id),
  rules_path        TEXT,
  tags              JSON NOT NULL DEFAULT '[]',
  last_fetched_at   INTEGER,
  created_at        INTEGER NOT NULL
);

-- Global (workspace-scope) rules. Repo/folder rules live in repo files.
CREATE TABLE rule_set (
  id           TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL REFERENCES workspace(id),
  scope        TEXT NOT NULL,      -- global | (repo-level stored in a file)
  title        TEXT NOT NULL,
  body         TEXT NOT NULL,      -- markdown
  version      INTEGER NOT NULL,
  created_at   INTEGER NOT NULL,
  updated_at   INTEGER NOT NULL
);
```

### 5.2 Indexes

```sql
CREATE INDEX idx_object_cache_source ON object_cache(source_id, fetched_at);
CREATE INDEX idx_run_status ON run(status, started_at);
CREATE INDEX idx_workflow_workspace ON workflow(workspace_id);
CREATE INDEX idx_repo_workspace ON repository(workspace_id);
CREATE INDEX idx_repo_linked_source ON repository(linked_source_id);
CREATE INDEX idx_rule_set_workspace ON rule_set(workspace_id, scope);
```

---

## 6. Auth & Security

> **Reversed:** §6.1 and §6.2 are obsolete. Woom is PAT-only
> permanently — no OAuth, no redirect handler (`docs/ROADMAP_1.0.md §6`).
> The current auth shape is `docs/CONNECTIONS.md`. Sections below kept
> for historical context only.

### 6.1 ~~OAuth flows~~ (cancelled)

~~All providers use OAuth 2.0 with PKCE.~~ Woom uses Personal
Access Tokens / API tokens for every source — see
`docs/CONNECTIONS.md §3` for the actual scopes per provider.

### 6.2 ~~Redirect handling~~ (cancelled)

~~Tauri registers the custom URI scheme `forge://oauth/callback`.~~
The `forge://` URL scheme is not registered; PAT entry happens in the
in-app connect modal.

### 6.3 Token storage

- **Primary:** OS keychain (the `keyring` crate for Rust → Keychain
  Services on macOS, Credential Manager on Windows, libsecret on Linux).
- **Fallback:** encrypted SQLite with a master key in the keychain. Used
  only if the keychain is unavailable.

### 6.4 Per-source secret refs

Each source has a deterministic keychain key: `forge:source:<source_id>`.
The DB stores only the ref, never the token itself.

### 6.5 Team auth (v0.3 preview)

> **Reversed:** team OAuth is also cancelled. If teams ever land
> (`docs/FUTURE_FEATURES.md §F`), workspace credentials will use the
> same PAT model — every member supplies their own tokens, optionally
> mediated by a workspace-managed Vault (`§E.5`). No OAuth flow.

In the MVP everything is personal. The `source.shared` flag stays as
a forward-compat hint but its contract is "one PAT shared across
workspace members" not "OAuth via workspace app".

### 6.6 Sandbox

MCP servers are started with a restricted environment and no access to
`$HOME` beyond a specific data directory. For `forge-claude`, the
worktree sits in a tmp directory, not the main repo.

---

## 7. Team Foundation (present in MVP, activated later)

### 7.1 Workspace model

Even in single-player mode the user works inside an implicit "personal"
workspace. Every Source, Workflow, and Run is tied to a `workspace_id`.
This makes migrating to a team mode mechanical: add members, enable sync.

### 7.2 Shared resources

The `shared: bool` flag on Source and Workflow decides whether other
workspace members see them. Personal by default stays local.

### 7.3 Sync strategy (post-MVP)

Two layers:

1. **Metadata sync:** workflows, sources (without credentials), notifications.
   Through an append-only event log with sequential versions.
2. **Credentials:** workspace-scoped credentials live on the backend; the
   desktop fetches short-lived tokens.

Artifacts and runs stay local, but `Run` metadata (status, who started it)
can be shared optionally for a team inbox.

### 7.4 Permissions

Roles: `owner > admin > member > viewer`. The permission matrix is a
simple table. Not implemented in the MVP, but the DB schema is ready.

### 7.5 Team roadmap

- **v0.1 (MVP):** local-only, workspace=personal, all fields in place.
- **v0.2:** export/import workflow as a file (shared via git/slack).
- **v0.3:** self-hostable backend, invite flow, shared workflows.
- **v0.4:** shared credentials, SSO (SAML/OIDC).

---

## 8. MVP Scope (acceptance criteria)

> **Note:** the original MVP criteria below included OAuth and Slack;
> both were dropped (`MVP.md`, `ROADMAP_1.0.md §6`). Today's actual
> 1.0 acceptance criteria live in `ROADMAP_1.0.md §5`.

MVP is done when:

1. ~~✅ The user logs into Jira / GitHub / Slack through OAuth.~~
   Reversed → user pastes a PAT for Jira / GitHub / Sentry (Slack is
   `FUTURE_FEATURES.md §A.1`).
2. ✅ The Inbox shows objects from all three sources, sorted by `updated_at`.
3. ✅ A Ticket can be dragged onto the "Claude Code" zone and a real-time run starts.
4. ✅ The result (Artifact: diff + summary) can be dragged onto
   "Slack → #channel" or "Jira comment" and the message is sent.
5. ✅ A global palette (⌘K) offers fuzzy search across all objects.
6. ✅ A "ticket → claude → slack" workflow is saved and re-runnable with one click.
7. ✅ There's a Timeline of active and completed runs with access to logs.
8. ✅ Tokens live in the keychain, not in the DB. Logout wipes them.

---

## 9. Post-MVP Roadmap

- **v0.2 — Polish & extensibility (~1 month after MVP)**
  - Reminders & notifications system (native OS notifications)
  - Custom MCP servers (the user can add their own)
  - Rich artifact preview (diff viewer, markdown renderer)
  - Workflow templates gallery

- **v0.3 — Team (~2–3 months)**
  - Self-hostable backend (Rust/axum + Postgres)
  - Invite flow, workspace management UI
  - Shared workflows & sources
  - Activity feed

- **v0.4 — Enterprise**
  - SSO (SAML/OIDC)
  - Audit log
  - RBAC with custom roles
  - Deployment: Docker Compose + Helm chart

- **v0.5+**
  - Linear, Teams, Notion, GitLab integrations
  - Scheduled workflows (cron)
  - Mobile companion (read-only inbox + approvals)

---

## 10. Decisions (formerly open questions)

Resolution of all v0.1 open questions:

- **MCP client:** use `rmcp` (Rust MCP SDK) as a dependency. Fork or
  custom client only if we hit streaming limits during M4. Saves ~1 week
  of bootstrap.
- **Artifact storage:** `~/Library/Application Support/Forge/artifacts/`
  (macOS; equivalents elsewhere). Auto-cleanup after 14 days LRU,
  hard cap 5 GB per workspace. Pinned artifacts (flag `pinned: bool`)
  are exempt from cleanup.
- **Rate limits:** a centralized **per-source token bucket** in
  `forge-runtime`, with **per-endpoint sub-buckets** when a provider
  documents different limits. Every MCP server reaches the network via
  `forge-runtime::http_client`, which enforces the bucket. Retry with
  jitter and backoff is built in.
- **Zed integration:** **MCP-only** in MVP. Forge exposes an MCP server,
  Zed connects natively. A dedicated Zed extension is v0.2+.
- **Telemetry:** no network telemetry in MVP. Local crash logs in the
  app data dir; the user exports them manually (share sheet) to help
  with bug reports. Opt-in network telemetry is v0.3 at the earliest.
- **Multi-select inbox:** allowed only for **safe bulk actions** (pin,
  reminder, mark read, archive). Multi-drop onto Claude Code is
  **forbidden** in MVP — one object = one run (keeps UX simple, avoids
  worktree collisions).

---

## 11. Repositories (summary, detail in REPOS.md)

Forge treats local git repos as first-class entities, separate from the
GitHub Source (which is API-only). Capabilities:

- Clone by URL or from the GitHub picker (UI similar to GitHub Desktop).
- Fetch / pull / branches / worktree — all through the action interface.
- **Every Claude run lives in its own worktree**, never touching the main tree.
- Never-destructive: commit/push only with explicit user consent.
- Can be linked to a GitHub Source (to pull issues/PRs) or remain
  standalone.

See [REPOS.md](REPOS.md) for the entity schema, lifecycle, UI, and security.

---

## 12. Rules (summary, detail in RULES.md)

Rules like "write code this way, name the branch this way, follow these
conventions" — composable rule sets scoped by visibility.

- **Scopes:** `global` (workspace) / `repo` (in `.forge/rules.md`) /
  `folder:<path>` / `run` (ephemeral).
- **Composition:** a narrower scope overrides a broader one.
- **Integration:** Forge injects rules as a system-prompt prefix on
  every Claude run; the native `CLAUDE.md` in the repo is still read
  (by Claude directly — we don't break it).
- **Policies in rules** (branch template, commit template, post-run
  checks, forbidden paths) are applied automatically by forge-runtime.

See [RULES.md](RULES.md) for file schema, resolution, and worked examples.

---

## 13. Target Platform & Packaging

### 13.1 macOS-first

MVP targets macOS exclusively. Windows and Linux builds are deferred
post-v1.0. Every decision below assumes macOS semantics; cross-platform
concessions are explicitly flagged.

**Minimum version:** macOS 13.0 (Ventura).
- Covers the overwhelming majority of active Macs in 2026.
- Lets us use modern AppKit APIs without backports.
- Tauri 2 supports older, but we pick a higher floor for sanity.

**Architecture:** Universal binary (`aarch64-apple-darwin` +
`x86_64-apple-darwin`). Shipped as a single `.app`; the user doesn't
choose a download.

### 13.2 Bundle layout

```
Forge.app/
└── Contents/
    ├── Info.plist                 # bundle id, URI scheme, min version
    ├── PkgInfo
    ├── MacOS/
    │   ├── forge                  # main Tauri executable (universal)
    │   ├── forge-jira             # sidecar MCP server
    │   ├── forge-github           # sidecar MCP server
    │   ├── forge-slack            # sidecar MCP server
    │   └── forge-claude           # sidecar (Claude Agent SDK bridge)
    ├── Resources/
    │   ├── icon.icns              # 16–1024pt full set
    │   ├── Forge.sdef             # (optional) AppleScript support
    │   └── en.lproj/
    │       └── InfoPlist.strings
    ├── Frameworks/                # WebKit and anything we vendor
    ├── PrivacyInfo.xcprivacy      # required-reason API declarations
    └── _CodeSignature/
```

**Info.plist essentials:**

| Key                              | Value                                         |
|----------------------------------|-----------------------------------------------|
| `CFBundleIdentifier`             | `com.forge.desktop`                           |
| `CFBundleName`                   | `Forge`                                       |
| `LSMinimumSystemVersion`         | `13.0`                                        |
| `LSApplicationCategoryType`      | `public.app-category.developer-tools`         |
| `NSHumanReadableCopyright`       | `© 2026 Forge`                                |
| ~~`CFBundleURLTypes`~~           | ~~`forge://` scheme for OAuth callbacks~~ — cancelled, no OAuth |
| `NSAppleEventsUsageDescription`  | Reason for scripting access (if used)         |
| `LSUIElement`                    | `false` (regular window app, not background)  |
| `NSHighResolutionCapable`        | `true`                                        |
| `NSRequiresAquaSystemAppearance` | `false` (we support light/dark mode)          |

### 13.3 Sidecar binaries

MCP servers and any native helpers ship as **Tauri sidecars** — bundled
into `Contents/MacOS/` and signed as part of the main bundle.

Rules:
- **Prefer Rust** for MCP servers. Native, small, trivially signed.
- Acceptable: Node.js sources compiled with `bun build --compile` into
  a standalone single-file binary.
- **Forbidden:** depending on a system-installed Node.js, Python, or
  any runtime the user has to install separately.

Exception: the user's own `claude` CLI. We bridge to it when present,
and fall back to the embedded `forge-claude` sidecar (which wraps the
Claude Agent SDK) when it isn't — see §4.4.

### 13.4 Code signing

- **Apple Developer Program** enrollment required ($99/year).
- Signing identity: `Developer ID Application: <Team Name> (<TEAM_ID>)`.
- Uses the **hardened runtime** with entitlements:
  - `com.apple.security.cs.allow-jit` — for the WebKit JS engine
  - `com.apple.security.network.client` — HTTPS to providers
  - `com.apple.security.files.user-selected.read-write` — user-picked
    clone target dir
  - `com.apple.security.cs.disable-library-validation` — only if
    unavoidable for a specific sidecar
- Every nested binary (every sidecar) is signed with the same identity
  before the parent is sealed.

### 13.5 Notarization

After signing, the `.dmg` is submitted to Apple's notary service via
`notarytool`:

```bash
xcrun notarytool submit Forge.dmg \
  --apple-id "$APPLE_ID" \
  --team-id "$TEAM_ID" \
  --password "$APP_SPECIFIC_PASSWORD" \
  --wait
```

Then the ticket is stapled:

```bash
xcrun stapler staple Forge.dmg
```

Tauri's bundler handles both steps automatically if `APPLE_ID`,
`APPLE_PASSWORD`, and `APPLE_TEAM_ID` environment variables are set
during `tauri build`.

### 13.6 Distribution

**Primary (MVP):** signed & notarized `.dmg` hosted on our own server
or GitHub Releases. Not the Mac App Store — that requires a full
sandbox, which would complicate sidecar spawning and `claude` CLI
bridging.

**DMG layout:**
- Custom background image with "drag Forge to Applications" hint.
- `/Applications` symlink.
- `Forge.app` positioned at a pleasing offset.
- Built via `create-dmg` or Tauri's built-in DMG builder.

**Auto-updates:** deferred to v0.2. Decision between Tauri's built-in
updater (signed with a separate Ed25519 key) and Sparkle will be made
then. MVP ships with a manual "download new version" link via the
About panel.

### 13.7 Native macOS integrations we use

| Feature                       | API / Mechanism                                     |
|-------------------------------|-----------------------------------------------------|
| Credential storage            | Keychain Services via `keyring` crate               |
| Run-completed notifications   | `UNUserNotificationCenter` via Tauri notify plugin  |
| "Open in Zed / VS Code"       | `NSWorkspace` / direct `spawn`                      |
| ~~OAuth callbacks~~           | Cancelled — Woom is PAT-only (`ROADMAP_1.0.md §6`) |
| Dark Mode                     | Follows system (`NSRequiresAquaSystemAppearance = NO`) |
| Dock badge (unread count)     | `NSApplication.setDockBadge` (v0.2)                 |
| Global shortcut (summon)      | Tauri `global-shortcut` plugin (v0.2)               |
| Drag-and-drop from Finder     | `NSDraggingDestination` via Tauri                   |
| "Services" menu entries       | v0.2                                                |

### 13.8 Build pipeline

**Local dev:**

```bash
pnpm tauri dev             # hot reload, unsigned debug build
```

**Production build (macOS host):**

```bash
pnpm tauri build --target universal-apple-darwin
```

Produces `Forge.app` and `Forge_<version>_universal.dmg`, signed and
notarized (if env vars are present), in `target/universal-apple-darwin/release/bundle/`.

**CI (GitHub Actions on `macos-latest`):**

1. Check out, install Rust toolchain + both Apple targets.
2. `pnpm install`.
3. Import the Developer ID certificate (from encrypted secret) into a
   temporary keychain.
4. `pnpm tauri build --target universal-apple-darwin`.
5. Tauri signs, builds the DMG, and submits it for notarization.
6. Upload the stapled DMG to the release artifacts.

Encrypted secrets in CI:

| Secret                     | Purpose                                   |
|----------------------------|-------------------------------------------|
| `APPLE_CERTIFICATE`        | Developer ID .p12 (base64)                |
| `APPLE_CERTIFICATE_PASSWORD` | Password for .p12                       |
| `APPLE_ID`                 | Developer account email                    |
| `APPLE_PASSWORD`           | App-specific password                      |
| `APPLE_TEAM_ID`            | 10-char team identifier                    |

### 13.9 Forbidden in MVP

- Menu-bar-only mode (`LSUIElement=true`). v0.2 as an optional flag.
- Widget Extensions.
- Quick Look plugin for artifacts.
- Mac App Store build — defer until sandboxing is feasible.
- Shipping with any runtime the user must pre-install.
