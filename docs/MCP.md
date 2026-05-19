# Woom — MCP Surface Specification

**Version:** 0.1
**Last updated:** 2026-04-29
**Status:** describes shipping behaviour. Woom ships five MCP
servers as Tauri sidecars (`woom-app`, `woom-github`,
`woom-jira`, `woom-sentry`, `woom-memory`) and exposes
them to Claude Code (via temporary `--mcp-config`) and Cursor Agent
(via merged `~/.cursor/mcp.json`). The descriptors the user's IDE
uses for documentation/linting live separately under
`/Users/nikolay-khartanovich/.cursor/projects/Users-nikolay-khartanovich-Repos-pers-forge/mcps/`.

> MCP is the contract between Woom and the LLM. Each sidecar
> binary speaks JSON-RPC over stdio; each exposes a small bag of
> tools; each is fed env vars by the parent app at spawn time. The
> agent is then given an allow-list filtered by `ToolProfile` so a
> Sentry-triage session doesn't see Jira tooling and vice versa. This
> doc is the index — per-server detail is in the source-side docs
> (`GITHUB.md`, `JIRA.md`, `SENTRY.md`).

---

## 1. Vision & Non-Goals

### 1.1 Vision

The agent should never have to ask the user "what's the Jira API for
…" — it should call a tool. Tools are deliberately small and
single-purpose; we trade depth for predictability. `propose_*` is the
shape for any state-changing shell / git operation, so the user
always sees what's about to happen.

### 1.2 Goals (v1, shipping)

1. Five sidecars wired up: app navigation, GitHub, Jira, Sentry,
   memory.
2. Per-session `ToolProfile` filter so allow-lists scope nicely.
3. Temp-file `--mcp-config` for Claude (one per session, deleted on
   stop), `~/.cursor/mcp.json` merge for Cursor.
4. Approval-gated `propose_*` tools (commit / PR / bash / switch-cwd)
   queue in the session as Action Cards.
5. Read-only MCP `app.*` tools for UI navigation that don't surface in
   the chat.
6. Token / credential injection via env vars at spawn time so sidecars
   don't read keychain themselves.
7. MCP server discovery for the user's Cursor IDE through descriptor
   JSON files in the user's MCPs folder.
8. **Third-party MCP pass-through.** Any server the user installs via
   `claude mcp add ...` (recorded in `~/.claude.json`) is merged into
   the Claude session's MCP config alongside Woom's bundled sidecars.
   Allowed-tools list grants server-wide access (`mcp__<name>`) so we
   don't need to hard-code each third-party server's tool surface. Name
   collisions resolve to Woom's built-in (we never spin up two of the
   same server). See `claude_mcp.rs::user_mcp_servers`.

### 1.3 Non-Goals (v1)

- **Multi-tenant credentials.** One token per source per app instance.
- **Per-tool curation for third-party MCPs.** We don't enumerate each
  third-party server's tool list at config time, and we don't promise
  the bot-vs-human approval gating (`propose_*` flow) for them. Users
  who add their own servers via `claude mcp add` get a server-wide
  grant or nothing.
- **Streaming / progress callbacks** beyond stdio. MCP doesn't have a
  rich progress model and we don't simulate one.
- **Tool result attachments** beyond text + image. No binary blobs in
  responses.
- **Cross-session shared state** other than the memory store and
  inbox caches.

---

## 2. Server Catalogue

| Logical name | Sidecar binary path                                                               | Auth env                                                  |
|--------------|-----------------------------------------------------------------------------------|-----------------------------------------------------------|
| `app`        | `apps/desktop/src-tauri/sidecars/woom-app/`                                  | (none — talks to the parent UI via local socket)          |
| `github`     | `apps/desktop/src-tauri/sidecars/woom-github/`                               | `GITHUB_TOKEN`                                             |
| `jira`       | `apps/desktop/src-tauri/sidecars/woom-jira/`                                 | `JIRA_HOST`, `JIRA_EMAIL`, `JIRA_TOKEN`                    |
| `sentry`     | `apps/desktop/src-tauri/sidecars/woom-sentry/`                               | `SENTRY_HOST`, `SENTRY_ORG`, `SENTRY_TOKEN`                 |
| `memory`     | `apps/desktop/src-tauri/sidecars/woom-memory/`                               | `WOOM_MEMORY_DB` (path to sqlite)                     |

The user's Cursor IDE also has descriptor folders for each server
under `/Users/nikolay-khartanovich/.cursor/projects/Users-nikolay-khartanovich-Repos-pers-forge/mcps/user-woom-*/`,
each containing a `tools/<tool-name>.json` JSON-Schema descriptor and
an `INSTRUCTIONS.md` that the IDE shows to the model.

`woom-memory` has no `tools/` JSON folder in the user's MCPs dir
in the current state — only `SERVER_METADATA.json` and `STATUS.md`.
The Rust binary still ships with four real tools (`memory_save`,
`memory_search`, `memory_list`, `memory_delete`) which are visible to
the agent at runtime.

---

## 3. Tool Inventory

### 3.1 `woom-app` (UI navigation)

| Tool                       | Purpose                                                                  |
|----------------------------|--------------------------------------------------------------------------|
| `add_editor_instance`      | (deprecated alias) Add an editor column. Use `add_workbench_instance` with `kind=editor`. |
| `add_workbench_instance`   | Add a new column of given kind to the active workbench.                  |
| `focus_workbench_instance` | Scroll-to + briefly highlight a column.                                  |
| `list_instances`           | Re-read the workbench layout for an up-to-date map.                      |
| `new_workbench`            | Create a fresh workbench tab.                                            |
| `switch_workbench`         | Switch active workbench by name or index.                                |
| `switch_view`              | Change top-level view (`workbench` / `githubTab` / …).                    |
| `open_connect_modal`       | Surface the connect modal for a source.                                  |
| `open_github_pr`           | Open the PR slide-over (with optional `tab`).                             |
| `open_github_issue`        | Open the issue slide-over.                                               |
| `open_github_repo`         | GitHub tab + repo on a section + optional file path.                      |
| `open_jira_issue`          | Open the Jira slide-over.                                                |
| `open_jira_tab`            | Switch to the Jira tab with filters.                                     |
| `open_sentry_issue`        | Open the Sentry slide-over.                                              |
| `open_sentry_event`        | Open the Sentry slide-over with a specific event id.                      |
| `open_sentry_tab`          | Switch to the Sentry tab with filters.                                    |
| `set_agent_cwd`            | Change an agent session's cwd.                                            |
| `set_editor_repo_path`     | Change an editor's open folder; linked agents follow.                     |
| `set_github_column`        | Patch filters on an existing GitHub column.                              |
| `set_jira_column`          | Patch filters on an existing Jira column.                                |
| `set_sentry_column`        | Patch filters on an existing Sentry column.                              |

The MCP request hits the sidecar; the sidecar emits a custom JSON event
back to the parent app over a local-loopback socket; the parent app
runs the equivalent UI action. The agent's chat does **not** surface
these calls (they're routed in `+page.svelte`'s
`handleAppNavigation` / `handleStreamEvent`'s `mcp__app__*` switch).

### 3.2 `woom-github`

See [`docs/GITHUB.md §13.1`](GITHUB.md#131-user-woom-github-read--write).
Twenty tools across read / search / write / propose-`*`.

### 3.3 `woom-jira`

See [`docs/JIRA.md §8`](JIRA.md#8-mcp-tools-user-woom-jira).
Nine tools across read / write / metadata.

### 3.4 `woom-sentry`

See [`docs/SENTRY.md §9`](SENTRY.md#9-mcp-tools-user-woom-sentry).
Nine tools across read / write / metadata.

### 3.5 `woom-memory`

A local SQLite-backed key-value-with-search store. Same idea as
Anthropic's "memory" example MCP, but local-only — useful for the
agent to drop notes between sessions ("user prefers short replies",
"pino logger style is current convention").

```text
memory_save(key, value, tags?)
memory_search(query, tags?, limit?)
memory_list(tags?, limit?)
memory_delete(key)
```

DB location: `WOOM_MEMORY_DB` env var (set by parent to a path
under app data dir).

---

## 4. How Agents See It

### 4.1 Claude Code

`apps/desktop/src-tauri/src/claude_mcp.rs:188-327`:

```rust
fn build_mcp_config(session_id: &str, profile: ToolProfile)
  -> Option<(PathBuf, Vec<String>)> {
  let mcp = serde_json::json!({
    "mcpServers": {
      "woom-app":    { "command": "<bin>", "env": {} },
      "woom-github": { "command": "<bin>", "env": { "GITHUB_TOKEN": "<token>" } },
      "woom-jira":   { "command": "<bin>", "env": { ... } },
      "woom-sentry": { "command": "<bin>", "env": { ... } },
      "woom-memory": { "command": "<bin>", "env": { "WOOM_MEMORY_DB": "<path>" } }
    }
  });
  let path = std::env::temp_dir().join(format!("woom-mcp-{}.json", session_id));
  std::fs::write(&path, serde_json::to_string(&mcp)?)?;
  let allowed = compute_allow_list(profile);   // see §5
  Some((path, allowed))
}
```

The path is passed to the spawned `claude` as
`--mcp-config <path>` plus `--allowedTools <space-separated>` filtered
by `ToolProfile`. The temp file is cleaned up by `TempFile`'s `Drop`
when the session ends.

### 4.2 Cursor Agent

Cursor reads `~/.cursor/mcp.json` directly. We **merge** Woom's
servers into that file at app startup (server names prefixed
`woom-`) — see `apps/desktop/src-tauri/src/cursor_mcp.rs`. We
don't pass `--mcp-config` to `cursor-agent`; instead it picks them up
from the user's profile file. The `--approve-mcps` and `--trust`
flags are added so the user isn't prompted on first invocation.

### 4.3 Tool name normalization

Claude prefixes server names like `mcp__woom-app__open_github_pr`
in its tool-use stream. Cursor sometimes normalises differently
depending on version. `apps/desktop/src-tauri/src/cursor.rs::normalize_mcp_tool_name`
unifies them so frontend matchers (e.g. `agentStream.ts:229-273`) can
treat `mcp__app__*` as the canonical form.

---

## 5. `ToolProfile` and Allow-Lists

`apps/desktop/src-tauri/src/claude_mcp.rs:50-172`:

```rust
pub enum ToolProfile { All, Coding, Github, Jira, Sentry, Triage }
```

| Profile  | Allow-list rough shape                                                                              |
|----------|------------------------------------------------------------------------------------------------------|
| `All`    | Every tool from every server, minus `mcp__memory__*`, minus a few canvas-only `app` tools.            |
| `Coding` | `mcp__app__*` (navigation), `mcp__github__*` (read + propose-*), local tools (`Edit`, `Write`, etc.). |
| `Github` | Tightened to GitHub read + write + UI nav.                                                            |
| `Jira`   | Tightened to Jira tools + UI nav.                                                                      |
| `Sentry` | Tightened to Sentry tools + UI nav.                                                                   |
| `Triage` | Read-only across the four sources + UI nav. No write tools.                                           |

Patterns are applied per-tool by exact name. Anything not in the
list is dropped from the `--allowedTools` Claude sees, so the model
literally can't call it.

The profile is decided per session at spawn time. Switching profile
mid-session requires a stop / restart (the temp `--mcp-config` is
written once).

---

## 6. Authentication / Credential Plumbing

Tokens never live inside MCP sidecars' source — they're passed in via
env vars. The flow:

1. User connects in the UI; token saved to keychain (see
   [`docs/CONNECTIONS.md §3`](CONNECTIONS.md#3-auth-model)).
2. On Claude session start, `claude_ask` reads the keychain and
   constructs the `mcp.json`'s `env` map per server.
3. Sidecar boots, sees env, configures its HTTP client.
4. On session end, `--mcp-config` temp file is dropped.

Cursor's case is similar but the env propagation is via the merged
`~/.cursor/mcp.json` file's `env` field, written at app startup.
Re-merging happens on every Woom launch so revoked tokens get
purged.

There is **no** `mcp_auth` tool in the `user-woom-*` directories.
The `mcp_auth` concept (a tool an MCP server exposes for its own
credential round-trip) is unused here — Woom owns the auth,
sidecars read what they're told. Woom itself is PAT-only across
the board; a future `mcp_auth` is for
third-party / community servers we don't ship, not for our own.

---

## 7. Use-Instructions (per server, summarised)

Each `INSTRUCTIONS.md` ships with a server descriptor and is shown to
the model as part of the system prompt:

- **`user-woom-app`** — UI navigation. Detail panes, top-level
  tabs, workbench operations (new/switch/add column, set cwd, focus,
  list_instances). Use `open_connect_modal` when the user mentions an
  integration that isn't connected yet.
- **`user-woom-github`** — READ / WRITE / PROPOSE. Always call
  `propose_bash` for anything mutating (git, npm, deploy). Read-only
  shell commands (`git status`, `ls`, `cat`, `rg`) can use `Bash`
  directly.
- **`user-woom-jira`** — READ + WRITE. Markdown auto-converts to
  ADF. Resolve names to ids via `list_assignable_users` /
  `list_sprints` before calling `create_issue` / `update_issue`.
- **`user-woom-sentry`** — Triage flow: `get_issue` → `get_event`
  → `get_issue_tags` → `list_events` → `search_issues` → `update_issue` /
  `add_comment`.
- **`user-woom-memory`** — `STATUS.md` only in the user's MCPs
  folder right now (likely an environment artifact). The actual
  Woom memory binary is wired for the agent at runtime.

---

## 8. Sidecar Process Model

Each sidecar is its own Rust binary, built into the `Resources/_up_/`
of the macOS app bundle. Tauri spawns them on demand and pipes stdio:

- Claude path: spawn at session start, kill at session stop.
  `--mcp-config` carries the temp file path.
- Cursor path: spawned lazily by `cursor-agent` itself when the model
  invokes a tool. Woom's only role is to put the entries in
  `~/.cursor/mcp.json`.

Logs (stderr) are forwarded to the parent app's log buffer; if a
sidecar dies mid-session, the agent gets a tool-error response on its
next call ("server woom-jira disconnected"). No auto-restart in
v1.

---

## 9. Extending: How to Add a Tool

The five-step shape:

1. Add the tool handler in the relevant sidecar (`apps/desktop/src-tauri/sidecars/woom-X/src/main.rs`).
2. Update the `tools/` JSON descriptor in the user's Cursor MCPs
   folder *(this is for the IDE / user; runtime doesn't need it)*.
3. Add the tool name to the right `ToolProfile` allow-list in
   `claude_mcp.rs`.
4. If the tool is `mcp__app__*` (UI nav), add a handler in
   `+page.svelte`'s `handleStreamEvent` switch.
5. If the tool needs a per-session API call response that doesn't yet
   exist, add a Tauri IPC for it on the Rust side and wire the sidecar
   to call back via the same loopback socket.

---

## 10. Open TODOs

1. **`woom-memory` descriptors missing.** The runtime tools work,
   but the user's IDE can't surface schemas — needs the four JSONs.
2. **No restart-on-crash** for sidecars.
3. **No metrics** on tool calls (rate, latency, error rate).
4. **`mcp_auth`** tool not implemented — would be useful if we ever
   add an MCP server whose creds should live inside the server itself
   rather than handed through env.
5. **Cursor IDE prefix** drift: occasional `mcp__woom-app__` vs
   `mcp__app__` mismatches. The normalizer handles current versions;
   needs a refresh per Cursor update.
6. **Tool descriptions in JSON** sometimes lag behind actual server
   behaviour (e.g. `woom-github`'s "read-only phase 2" header
   comment is stale).
7. **No multi-instance credentials** — one source = one token = one
   MCP env-set. Multi-org hits a wall.
8. **Profile switching mid-session** requires stop/start; could be a
   live `--config-reload` if the CLI supported it.

---

## 11. Glossary

- **MCP** — Model Context Protocol. Stdio JSON-RPC contract between
  the agent CLI and tool servers.
- **Sidecar** — a small Rust binary shipped inside the Tauri app
  bundle, spawned per session.
- **`ToolProfile`** — Rust enum that filters which MCP tools the agent
  is allowed to call this session.
- **`--mcp-config`** — Claude CLI flag pointing to a JSON file
  describing servers. Woom writes a temp file per session.
- **`~/.cursor/mcp.json`** — Cursor's standard MCP server registry,
  merged by Woom at app startup.
- **`open_connect_modal`** — UI navigation tool the agent calls to ask
  the user to wire up a missing source.
- **`propose_*`** — write-prefix indicating the tool queues an Action
  Card instead of executing. See `docs/AGENTS.md §7`.
- **Tool name normalization** — `cursor.rs::normalize_mcp_tool_name`
  unifies prefixes across CLI versions.
