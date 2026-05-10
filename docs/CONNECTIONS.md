# Woom — Connections & Settings Specification

**Version:** 0.1
**Last updated:** 2026-04-29
**Status:** describes shipping behaviour. Connections cover the
external services Woom can pull from (GitHub, Jira, Sentry) and
the local agent CLIs it can drive (Claude, Cursor). Slack, Linear,
Notion, GitLab, Teams, Asana, Codex, Aider, Copilot are catalogued in
the Connect view as "coming soon" placeholders only.

> Woom's connection model is deliberately small. Tokens go in the
> macOS Keychain. Status is a Tauri `invoke` per source — connected /
> disconnected / connecting. There is no central OAuth roundtrip
> infrastructure, and there never will be (`docs/ROADMAP_1.0.md §6`):
> GitHub uses a PAT, Jira uses email + API token, Sentry uses an
> internal-integration token. PAT-only is the permanent shape of
> Woom auth; we invest in token UX (rotation reminders, multi-
> account, diagnostics) rather than OAuth bureaucracy. Agents
> (Claude / Cursor) aren't tokens at all — they're shelled subprocesses;
> we just check the binary is on PATH and authenticated to its own
> service.

---

## 1. Vision & Non-Goals

### 1.1 Vision

A user should be able to wire up Woom to their entire toolchain
in five minutes, all from one screen. The Connect view shows every
known integration with a uniform status pill, a single CTA, and a
revocation path. No magic; no surprise re-auth roundtrips at runtime.

### 1.2 Goals (v1, shipping)

1. Five live integrations: GitHub, Jira, Sentry (data sources),
   Claude, Cursor (agent CLIs).
2. macOS Keychain storage for every token (`security` keychain access).
3. Token validation on connect; eviction on `InvalidToken` at status
   check.
4. Per-source connect modal opened from anywhere
   (`open_connect_modal` MCP, the Connections view, the rail dot).
5. `connectionsState` Svelte store with one entry per source, mirrored
   into derived booleans (`connectedGithub`, `connectedJira`, …).
6. SettingsView for theme, font scale, localStorage stats, worktree
   disk usage, cleanup, hard-reset operations.

### 1.3 Non-Goals (v1)

- **OAuth.** Permanent non-goal — every source is and stays token-
  paste (`docs/ROADMAP_1.0.md §6`). Token UX investment instead:
  scope guidance, rotation reminders, multi-account, diagnostics.
- **Multiple accounts of the same source.** One GitHub, one Jira, one
  Sentry per Woom instance (multi-account is a 1.0 backlog
  item, see ROADMAP §2.7.1).
- **Slack / Linear / Notion / GitLab / Teams / Asana / Codex / Aider /
  Copilot.** Roadmap chips only.
- **Per-workspace credentials.** Tokens are user-global.
- **Login with Atlassian / Google / GitHub SSO** for Woom itself.
  Woom has no account model.

---

## 2. Sources Catalogue

```ts
// apps/desktop/src/lib/data.ts:45-56
export const connectionsMeta: ConnectionMeta[] = [
  { id: 'github', name: 'GitHub', kind: 'source',  implemented: true },
  { id: 'jira',   name: 'Jira',   kind: 'source',  implemented: true },
  { id: 'sentry', name: 'Sentry', kind: 'source',  implemented: true },
  { id: 'claude', name: 'Claude', kind: 'agent',   implemented: true },
  { id: 'cursor', name: 'Cursor', kind: 'agent',   implemented: true },
  { id: 'slack',  name: 'Slack',  kind: 'source',  implemented: false },
  { id: 'linear', name: 'Linear', kind: 'source',  implemented: false },
  // notion, gitlab, teams, asana, codex, aider, copilot — implemented: false
];
```

`implemented: false` means: shows in the Connect view as a "Coming
soon" chip; clicking it does nothing (or, with `open_connect_modal`,
the agent surfaces a no-op).

There is no Slack MCP server in the local descriptor folder either —
the entry is purely catalogue-level.

---

## 3. Auth Model

### 3.1 Storage

All tokens go through `apps/desktop/src-tauri/src/keychain.rs`:

```rust
pub fn set(account: &str, value: &str) -> Result<()>;
pub fn get(account: &str) -> Result<Option<String>>;
pub fn delete(account: &str) -> Result<()>;
```

Backed by macOS `security` framework via the `keyring` crate. The
keychain item shows up as `Service: Woom, Account: <key>`.

| Key (`account`) | Source                  |
|-----------------|-------------------------|
| `"github"`      | PAT                     |
| `"jira"`        | JSON `{ workspace, email, token }` |
| `"sentry"`      | JSON `{ host, organization_slug, token }` |

Claude and Cursor have no Woom-managed token; they auth to their
own services.

### 3.2 Connect roundtrip

For each source the ⌘-shape is:

```text
1. user fills modal form
2. `*_connect(...)` Tauri command:
   2.1 trim / normalize inputs
   2.2 call provider's "who am I" endpoint with the credential
   2.3 if 200 → `keychain::set(KEY, ...)` and return user payload
   2.4 if 401/403 → typed `InvalidToken` error → modal shows it
3. `connectionsState[source] = { kind: 'connected', user }`
4. trigger initial refresh of dependent state
   (refreshAllInboxes for github/jira; addPanelInstance('sentry') for
   sentry first-connect; status modal close for claude/cursor)
```

For agents, "connect" is a poll:

```rust
// apps/desktop/src-tauri/src/agent.rs
async fn agent_status(kind: AgentKind) -> AgentStatus {
  resolve_bin(kind)
    .map(|p| Connected { path: p, version: ... })
    .unwrap_or(Disconnected { reason: "binary not found" })
}
```

### 3.3 Disconnect / revocation

`*_disconnect()` deletes the Keychain entry. The `connectionsState`
flips to `disconnected`; columns of that kind hide (visibility check)
or render their disconnected empty state. Per-instance filter records
remain in localStorage so reconnecting later restores the same state.

### 3.4 Status validation on boot

`onMount` in `+page.svelte` calls `*_status()` for every source in
parallel. A `InvalidToken` response auto-evicts the keychain item
(see GitHub: `lib.rs:243-267`).

`statusLoading` is true until every source returns; the empty state
("Connect a source") doesn't flash for users who already have
connections.

---

## 4. Connect Modal

`openConnectModal(conn)` (`+page.svelte:3209-3229`) routes by
`conn.id`:

```ts
function openConnectModal(conn: ConnectionMeta) {
  if (!conn.implemented) return;
  if (conn.id === 'github') {
    openModal('pat', { conn, token: '', error: null, busy: false });
    return;
  }
  if (conn.id === 'jira') openModal('jiraConnect', { ... });
  if (conn.id === 'sentry') openModal('sentryConnect', { ... });
  if (conn.id === 'claude') openModal('claudeStatus', { ... });
  if (conn.id === 'cursor') openModal('cursorStatus', { ... });
}
```

`ModalsRoot.svelte` mounts the right body for each:

| `kind`         | Component                                                   |
|----------------|-------------------------------------------------------------|
| `pat`          | `apps/desktop/src/lib/components/connect/PatConnectModal.svelte` |
| `jiraConnect`  | `apps/desktop/src/lib/components/connect/JiraConnectModal.svelte` |
| `sentryConnect`| `apps/desktop/src/lib/components/connect/SentryConnectModal.svelte` |
| `claudeStatus` | a status pane with "Install Claude" link + version readout  |
| `cursorStatus` | same shape as claudeStatus, for `cursor-agent`              |

PAT modal helpers expose the right "create token" URL with prefilled
scopes (see `docs/GITHUB.md §11`).

### 4.1 MCP triggering

The `mcp__app__open_connect_modal` tool (sidecar `woom-app`)
takes `{ source: 'github' | 'jira' | ... }` and calls
`openConnectModal(connectionsMeta.find(c => c.id === source))`. Useful
for "Claude, hook me up to Slack" — the agent looks for `slack` in the
connected set, finds it absent, calls `open_connect_modal({ source:
'slack' })` so the user gets a prompt without leaving the chat. Slack
isn't `implemented` so the modal short-circuits, but the affordance
exists.

---

## 5. `connectionsState`

```ts
// apps/desktop/src/lib/state/connections.svelte.ts (paraphrased)
export const connectionsState = $state<{
  github: { kind: 'connected', user: GithubUser } | { kind: 'disconnected' } | { kind: 'connecting' };
  jira: ...;
  sentry: ...;
  claude: ...;
  cursor: ...;
}>({ ... });
```

Derived helpers:

```ts
const connectedGithub = $derived(connectionsState.github.kind === 'connected');
const connectedJira   = $derived(...);
// etc.

const connectedIds = $derived.by(() => {
  const s = new Set<string>();
  if (connectedGithub) s.add('github');
  ...
  return s;
});

const anythingConnected = $derived(connectedIds.size > 0);
const statusLoading     = $derived(/* still resolving any *_status() */);
```

The empty workbench, the rail dot, and the pill bar all read these.

---

## 6. Visual Indicators

| Surface            | Source                              | Visual                                            |
|--------------------|-------------------------------------|---------------------------------------------------|
| Rail (left)        | `!anythingConnected && !statusLoading` | Dot on the Connections icon                    |
| Pill bar           | per-source `connected*`             | Kind group renders only when its source is on    |
| Connections view   | each source row                     | "Connected as @user" / "Connect" / "Reconnect"   |
| Per-source columns | `isInstanceVisible`                 | Disconnected source column doesn't render        |
| Empty workbench    | `!anythingConnected && !statusLoading` | "Connect a source" hero card                  |

---

## 7. Connections View

`apps/desktop/src/lib/views/ConnectionsView.svelte`. Reachable via:

- The rail icon.
- Empty-workbench CTA.
- Settings → "Manage connections" link.

Header copy: "Tokens live in your macOS Keychain." (`ConnectionsView.svelte:45`)

Layout: a vertical list of every entry in `connectionsMeta`. For each
row: name, status pill, primary action button.

- **Implemented + connected:** "Connected as @user · Reconnect /
  Disconnect".
- **Implemented + disconnected:** "Connect" → opens the right modal.
- **Not implemented:** "Coming soon" greyed chip.

There's no per-source advanced settings page; everything's in the
modal.

---

## 8. SettingsView

`apps/desktop/src/lib/views/SettingsView.svelte`.

Sections (in display order):

1. **Theme** — light / dark switch (writes to `themeState.name`,
   persisted under `woom:theme:v1`).
2. **Font scale** — % multiplier, applied via CSS `font-size` on
   `:root`.
3. **localStorage stats** — total bytes used, per-key breakdown.
   Useful when sessions grow toward the ~5 MB browser quota.
4. **Worktree disk** — list of agent worktrees with size, last
   accessed, "Delete" buttons.
5. **Cleanup** — buttons to:
   - clear archived workbench instances older than N days,
   - hard-delete a single session (confirms first),
   - reset Woom (clears localStorage; keychain stays).
6. **Storage keys** — read-only list of every `localStorage` key
   Woom uses, mainly for support diagnostics.

Persistence keys touched:

```text
woom:theme:v1
woom:claude-sessions:v1   (-> docs/AGENTS.md §13)
woom:claude-rules:v1
woom:editor-state:v1      (-> docs/EDITOR.md §13)
woom:editor:root          (-> docs/EDITOR.md §13)
woom:editor:tabs
woom:editor:sidebar-tab
woom:editor-main          (Splitter)
woom:workbenches:v1       (-> docs/WORKBENCH.md §15)
woom:github-col-filters-by-instance:v1
woom:jira-col-filters-by-instance:v1
woom:sentry-col-filters-by-instance:v1
woom:canvas:index:v1      (-> docs/CANVAS.md §3.1)
woom:canvas:viewport:v1
```

Worktree dirs are on-disk under `~/Library/Application Support/Woom/worktrees/`.

---

## 9. Biometry / Unlock

A first-launch biometric prompt (`security` `kSecAccessControl`) is
configured for the keychain item — successful unlock keeps the items
accessible for the session. Failed unlock keeps Woom open but
every `*_status()` returns disconnected. Reconnecting stores under a
fresh access-control object.

---

## 10. Agent-Side Sources Discovery

The agent context preamble (`agentContext.ts`) lists all connected
sources verbatim so the agent can reason about what's available
without trying tools blind:

```text
sources:
  github: connected as @nikolay
  jira: connected (woom.atlassian.net, nikolay@…)
  sentry: connected (sentry.io, woom)
  claude: connected (3.x)
agents:
  claude (you): linked editor=Sagrada-Familia
  cursor: linked editor=Notre-Dame
```

If the user mentions Slack and Slack isn't in the connected list, the
agent is instructed to call `mcp__app__open_connect_modal({source: 'slack'})`
(which short-circuits to a polite "Slack isn't yet supported" toast,
since `implemented: false`).

---

## 11. Open TODOs

1. No retry / backoff on flaky `*_status()` calls. Network blip on
   boot means a manual reconnect.
2. No multi-account support. A user with two GitHub orgs needs two
   PATs; today only one fits.
3. Slack / Linear / Notion / GitLab / Teams / Asana / Codex / Aider /
   Copilot rows are dead ends. Each is a separate sidecar + UI surface;
   roadmap, not blocking.
4. `claudeStatus` / `cursorStatus` modals show binary path + version,
   not "logged in as" — the CLIs don't expose a uniform identity API.
5. Migration of v1 boolean columns persistence assumes a Main
   workbench; unusual for users who reset state.
6. No "test connection" button per source — it's "Connect" or
   nothing.

---

## 12. Glossary

- **Source** — a service Woom reads / writes data from (GitHub,
  Jira, Sentry, Slack-future).
- **Agent** — a CLI Woom drives as a subprocess (Claude, Cursor).
- **`connectionsMeta`** — static catalogue of all known sources +
  agents and whether they're implemented yet.
- **`connectionsState`** — runtime status per source.
- **`connectedIds`** — set of currently-connected source ids.
- **`anythingConnected`** — boolean, used for empty-workbench gating.
- **`open_connect_modal`** — MCP tool the agent calls when it wants to
  ask the user to wire up a source.
- **Status modal** — read-only modal for agents (no token to enter,
  just a binary to install).
