# Woom — Agents (Claude / Cursor) Specification

**Version:** 0.1
**Last updated:** 2026-04-29
**Status:** describes shipping behaviour. Two agent kinds are in
production today (`claude`, `cursor`). Codex / Aider / Copilot exist
in the connections list as `implemented: false` placeholders only.

> Agent columns are how Woom talks to coding LLMs. Each column is
> a viewing window on a single conversation with a CLI-driven agent —
> Claude Code or Cursor Agent — whose stdout is parsed live into a
> structured stream of text, tool traces, edit cards, and proposed
> actions. The column owns no model state; it's a render of the global
> session list scoped to one `agentKind`. The cwd binding to an Editor
> column is what makes "Claude on this repo" a stable concept across
> restarts.

---

## 1. Vision & Non-Goals

### 1.1 Vision

LLM coding tools are CLIs at heart. `claude` and `cursor-agent` both
print streamed JSONL on stdout, and that's what Woom renders.
Three things matter:

1. **Structure the stream.** Raw stdout is unreadable. We parse every
   line, classify it (text delta / tool call / edit / proposal /
   error), and surface the parts that need user attention.
2. **Make actions reviewable.** When the agent wants to commit, open
   a PR, run `bash`, or switch repos, that's an `Action Card` the user
   approves — never a silent execution.
3. **Make code mutations reversible.** Every `Edit` / `Write` /
   `MultiEdit` / `Delete` becomes an `Edit Card` with a unified diff
   and Keep / Revert buttons.

### 1.2 Goals (v1, shipping)

1. Run `claude` and `cursor-agent` as Tauri sidecars; stream stdout
   to the UI; allow stop / resume / compact.
2. Persist all sessions to `localStorage` with `cwd`, `linkedToEditor`,
   `worktreePath`, `linkedCanvasId`, message history, mentions.
3. Stream-time parse of tool calls into `MessageEvent`s; lazy diff
   computation in `EditDiffCard`.
4. `propose_*` MCP tools (commit / PR / bash / switch-cwd) → action
   cards with approval flow.
5. Cwd / worktree / canvas linking with bidirectional sync to other
   columns.
6. Drop files / inbox items / chat messages onto an agent column to
   pin them as `@`-mentions.
7. MCP tool catalog scoped via `ToolProfile` filters per session.
8. Image attachments via paste + drop; text-range mentions via
   "Apply to agent" from the Editor column.
9. Per-column "active session" pointer so two Claude columns can show
   different chats from the same global list.

### 1.3 Non-Goals (v1)

- **First-party model serving.** Forge calls CLI tools that do their
  own auth and routing — we never see API keys.
- **Multi-model orchestration.** One agent kind per column. Cross-agent
  hand-offs are user-driven (drag a chat message, send to the other
  one).
- **Long-form session search.** No full-text search of message history;
  recall is by tab, by linked editor, or by command palette over
  column metadata only.
- **Inline AI in the Editor column.** See [`EDITOR.md §1.3`](EDITOR.md).
- **Streaming voice / dictation.** Composer is text + paste only.
- **Branching / forking conversations.** One linear thread per session.
  Compact (= summarize) is the only "rewrite" operation.

---

## 2. Supported Agent Kinds

The `agentKind` enum in `apps/desktop/src/lib/types.ts:226` is
**`'claude' | 'cursor'`**. The corresponding `PanelKind`s are
`'claude'` and `'cursor'`. `Codex`, `Aider`, `Copilot` show in
`connectionsMeta` (`apps/desktop/src/lib/data.ts:45-56`) with
`implemented: false` so they appear in the Connect modal as roadmap
chips, but no column / sidecar exists for them.

The Rust side mirrors this:

```rust
// apps/desktop/src-tauri/src/agent.rs
pub enum AgentKind { Claude, Cursor }
```

The MCP descriptor folder names use the `woom-` prefix and have
no agent-specific server (the agent itself is the consumer of MCPs,
not a producer).

---

## 3. Session Lifecycle

```
            ┌──────────────────────────┐
            │ newClaudeSession(...)    │
            └────────────┬─────────────┘
                         ▼
                ┌────────────────┐
   sendClaudeMessage ───▶ │ sending = true │
                ▼                └─┬──────┬──────┘
       runAgentRequest              │      │
       (Tauri claude_ask)           │      │
                ▼                ▼      ▼
       listen('claude:stream:…')   stop      done
              ▼                          │      │
         handleStreamEvent ◀─────────────┘      │
              │                                 │
              ▼                                 ▼
        ClaudeMessage[]              persisted to localStorage
              │
              ▼
        deleteClaudeSession (UI button)
```

- **Create:** `newClaudeSession({ agentKind, cwd, … })` mints a uuid,
  pushes onto `sessionsState.list`, sets `activeId`, and persists.
- **Resume:** Claude CLI keeps a per-project conversation by uuid; we
  store `claudeUuid` and pass `--resume` on the next ask. If `cwd`
  changes we rotate the uuid (`applySessionCwd`) because Claude scopes
  conversations by directory.
- **Compact:** `agent_compact_session` IPC summarises and resets the
  thread to one system "previously" message.
- **Delete:** `deleteClaudeSession(id)` removes from the list and
  clears `activeByInstance` slots pointing to it. Files (`worktreePath`)
  are not auto-cleaned — see Worktree section in `docs/SETTINGS.md`
  (TBD) for the cleanup chore.
- **Archive of *sessions* does not exist** — only columns are archivable
  (`docs/WORKBENCH.md §A.7`). A "stale" session sits in the global list
  forever unless deleted.

Persistence:

```ts
// apps/desktop/src/lib/state/sessions.svelte.ts
export const SESSIONS_STORAGE_KEY = 'woom:claude-sessions:v1';
```

Payload: `{ sessions: ClaudeSession[], activeId: string | null }`.
Quota errors fall back to `persistError` and a toast (see `:192-261`).

---

## 4. Message Model

```ts
// apps/desktop/src/lib/types.ts (excerpt)
export type MessageEvent =
  | { kind: 'text'; body: string }
  | { kind: 'trace'; segments: string[] }
  | {
      kind: 'edit';
      toolId: string;
      filePath: string;
      // … see types.ts:42-90
      status: 'loading' | 'applied' | 'kept' | 'reverted' | 'error';
    };

export type ClaudeMessage = {
  role: 'system' | 'user' | 'assistant';
  content: string;          // canonical body — full assistant text or user prompt
  trace?: string;           // legacy free-text trace
  events?: MessageEvent[];  // structured events (preferred over `trace`)
  thinking?: string;        // model reasoning if present
  usage?: { inputTokens; outputTokens; … };
  images?: { path: string; name: string }[];   // user-attached images
};
```

`events` is the canonical structured form; `trace` is kept for
backwards compatibility with sessions saved before the structured
parser landed. Renderers prefer `events` and fall back to `trace`
if absent. See `apps/desktop/src/lib/components/workbench/AgentColumn.svelte`
message-rendering branches.

The first `'text'` event of an assistant message is the visible body;
any `'trace'` events are folded into a "N steps" pill that expands to
show the structured tool calls.

---

## 5. Streaming

### 5.1 Channel

The Rust side emits one event per JSONL line:

```rust
let event_name = format!("claude:stream:{}", session_id);
let _ = app.emit(&event_name, &line);
```

Both Claude (`apps/desktop/src-tauri/src/claude.rs:407-411`) and Cursor
(`apps/desktop/src-tauri/src/cursor.rs:224-226`) use this same channel
name — once the Rust adapter has normalised the line, the frontend
doesn't care which CLI produced it. There is one event per line.

### 5.2 Frontend pipeline

`apps/desktop/src/lib/exec/claude.ts`:

```ts
unlisten = await listen<string>(`claude:stream:${req.sessionId}`, (event) => {
  const parsed = JSON.parse(event.payload);
  handleStreamEvent(req.sessionId, parsed, {
    appendAssistantDelta,
    onActionResolved,
    ...
  });
});
```

`handleStreamEvent` (in `apps/desktop/src/lib/exec/agentStream.ts`):

- Routes to `appendToLastAssistant(chunk)` for text deltas.
- Intercepts tool calls — `Edit`, `MultiEdit`, `Write`, `Delete` →
  pushes a `MessageEvent` `kind: 'edit'`.
- Intercepts `mcp__github__propose_*` → registers a `ClaudeAction` on
  the session (see §7).
- Intercepts `mcp__app__*` navigation tools → routes to UI handlers
  (`open_github_pr`, `set_editor_repo_path`, etc.) without surfacing
  in chat.

### 5.3 Backpressure / re-entry

Cursor's stream is deduped against re-emit storms by
`cursor.rs:228-233` (a small ring of recently-seen line hashes).

---

## 6. Edit Cards (`EditDiffCard.svelte`)

When a tool call mutates a file, `agentStream.ts:293-448` synthesizes a
`MessageEvent` of `kind: 'edit'` and the assistant message gains a
collapsed "diff pill" that expands inline.

The card runs an LCS-based line-diff (own implementation in
`EditDiffCard.svelte:64-164`) and groups changes into hunks with
`CONTEXT_LINES = 3`.

Statuses:

| `status`    | Meaning                                              |
|-------------|------------------------------------------------------|
| `loading`   | tool reported start, file not yet read for diff      |
| `applied`   | first render after file read                         |
| `kept`      | user clicked "Keep" — irreversible from UI            |
| `reverted`  | user clicked "Revert" — file restored, marker stays |
| `error`     | tool result was an error or file became inaccessible |

Buttons:

- **Open** — opens the file in the linked editor column at the change.
- **Keep** — accepts the edit. No undo; the agent's next mutation may
  still revert it.
- **Revert** — invokes the matching reverter:
  - `Edit` → `revert_edit { sessionId, toolId }`
  - `Write` → `revert_write { sessionId, toolId }` (restores prior
    contents from snapshot taken pre-write).
  - `Delete` → `redelete_file { sessionId, toolId }` (typo retained
    historically; semantically "restore the deleted file").
  - Untracked write of a new file → `fs_write_file(path, '')` (clear).

### 6.1 What we do **not** do

- No three-way merge with the user's own edits in the editor (CodeMirror
  diff is read-only). If the user edits the file between agent write
  and Revert, Revert wins.
- No deferred apply — Claude / Cursor CLIs already write to disk before
  emitting the tool result. We intercept *after the fact*.

---

## 7. Action Cards (`ClaudeActionCard.svelte`)

The four `mcp__github__propose_*` tools never execute; they queue an
`Action Card` on the session:

```ts
// apps/desktop/src/lib/types.ts (excerpt)
export type ClaudeActionKind = 'commit' | 'pr' | 'bash' | 'switch-cwd';

export interface ClaudeAction {
  id: string;
  kind: ClaudeActionKind;
  args: Record<string, unknown>;   // tool args verbatim
  status: 'pending' | 'executing' | 'done' | 'error';
  resultSummary?: string;
  createdAt: number;
}

export interface ClaudeSession {
  ...
  actions: ClaudeAction[];
  awaitingApproval: boolean;
}
```

UI: `AgentColumn.svelte:1636-1661` renders one `<ClaudeActionCard>`
per pending action, with `Run` / `Skip` buttons.

Execution flow (`+page.svelte:2856-2907`):

```ts
function executeAction(sessionId: string, action: ClaudeAction) {
  dispatchAction(sessionId, action, appendAssistantDelta, onActionResolved);
}

// onActionResolved → continueAgentTurn with synthetic prompt:
const continuation = recentActionSummaries
  ? `[Woom: action card resolved]\n${recentActionSummaries}\n\nLast result: ${result.ok ? '✓' : '✗'} ${result.summary}\n\nContinue with what you were doing.`
  : '';
```

`dispatchAction` (`apps/desktop/src/lib/exec/actions.ts`) routes by
`kind`:

| `kind`        | Implementation                                      |
|---------------|-----------------------------------------------------|
| `commit`      | `git add` + `git commit` IPC; pushes if `push: true` |
| `pr`          | `git_create_pr` IPC; opens GitHub create PR modal    |
| `bash`        | `proposed_bash_run` IPC, output streamed back        |
| `switch-cwd`  | `applySessionCwd(s.id, args.path, { breakLink: true })` |

`effectiveCwd(s)` resolves `worktreePath → cwd → first editor's repoPath
→ null` for any action that needs a working directory.

---

## 8. Prompt Input (Composer)

The composer is one `<textarea>` plus an optional `Mention` strip.

### 8.1 Mentions

A `Mention` is anything pinned to the next message:

```ts
type Mention =
  | { source: 'file'; path: string; isDir: boolean; … }
  | { source: 'github'; item: InboxItem; externalId: string }
  | { source: 'jira'; item: JiraItem; externalId: string }
  | { source: 'sentry'; item: SentryIssue; externalId: string }
  | { source: 'chat-message'; sessionId: string; messageIndex: number };
```

(Exact shape in `apps/desktop/src/lib/types.ts`.)

Mentions are added by:

- **Drag drop** onto the column body — `onAgentDrop` in `+page.svelte:834-875`.
- **Drag drop** onto a pill of kind `claude` / `cursor` — `pillCanAccept`,
  `onPillDrop`. Other pill kinds reject the drop.
- **`@`-typing** in the composer (file completion via `fs_list_dir`).
- **"Apply to agent"** from the Editor column — see [`EDITOR.md §9`](EDITOR.md#9-apply-to-agent).

When sending, the prompt is wrapped in a "Referenced items" preamble:

```ts
let prompt = text;
if (mentionsSnapshot.length) {
  const ctx = mentionsSnapshot.map(m => /* per-source body */).join('\n\n');
  prompt = `Referenced items:\n\n${ctx}\n\n----\n\nUser message:\n${text}`;
}
```

`+page.svelte:1462-1540`.

### 8.2 Image attachments

`onPasteImages` and `attachBlobsToSession` (paths in
`apps/desktop/src/lib/state/sessions.svelte.ts` and
`AgentColumn.svelte`) write image data to a temp folder and attach
`{ path, name }[]` to the user message. Both Claude and Cursor CLIs
ingest images via their image-input flag.

### 8.3 Send hotkeys

```svelte
{#if e.key === 'Enter' && !e.shiftKey && !sess.sending}
  e.preventDefault();
  ...
  onSendClaudeMessage();
{/if}
```

`AgentColumn.svelte:905-908`.

`Shift+Enter` inserts a newline. `Enter` while a `@`-mention popover is
open commits the popover instead — there's a flag the Enter handler
checks.

### 8.4 Slash commands

There is **no** structured slash-command parser in v1. Anything starting
with `/` goes through to the agent verbatim. Claude Code and Cursor
both define their own slash commands which stay invisible to Woom.

---

## 9. Cwd / Worktree Binding

| Field on `ClaudeSession` | Meaning                                                          |
|--------------------------|------------------------------------------------------------------|
| `cwd`                    | Last known working directory. Mirror of editor when linked.       |
| `linkedToEditor`         | Boolean; `true` when this session follows an editor column.       |
| `linkedToEditorInstanceId` | Which editor instance.                                          |
| `columnInstanceId`       | Which agent column hosts this session as its active session.      |
| `worktreePath`           | If the agent created a `git worktree`, its path. Beats `cwd`.     |
| `claudeUuid`             | Persistent Claude CLI conversation uuid (per cwd).                |
| `cwdUuids`               | `Record<cwd, claudeUuid>` so switching between known cwds resumes |
| `cwdSwitchRecap`         | Pending one-shot system message to inject on next send.           |

Resolution at send-time (`+page.svelte:1542-1548`):

```ts
const cwd = sess?.worktreePath || sess?.cwd || editorRepoPath || null;
```

Sync direction:

- **Editor → agents** (when editor's repoPath changes): a `$effect` in
  `+page.svelte:1319-1355` writes `cwd` on every linked session.
- **Agent → editor** (when MCP `set_editor_repo_path` runs): see
  [`EDITOR.md §7.1`](EDITOR.md#71-mcp__app__set_editor_repo_path).
- **Agent → self** (`set_agent_cwd`): `applySessionCwd(... { breakLink: true })`.

`applySessionCwd` lives in `apps/desktop/src/lib/services/sessionCwd.ts`
and tested by `sessionCwd.test.ts`.

---

## 10. Editor Linking

Already covered in [`EDITOR.md §6`](EDITOR.md#6-agent-linking) from the
editor side. From the agent side:

- **Visual:** `link-editor-btn` in `AgentColumn.svelte:1025-1035`. When
  unlinked, the button reads "Link editor"; clicked → opens an editor
  picker, calls `linkActiveSessionToEditor`.
- **Status pill:** under the column header, "→ Sagrada-Familia" with
  the linked editor's `inst.name`.
- **Drag**: the editor's pill can be dragged onto the agent column to
  link, mirroring the canvas-link UX. See `docs/WORKBENCH.md`.

---

## 11. MCP Tooling Available to Agents

Sidecar MCP servers wired by `apps/desktop/src-tauri/src/claude_mcp.rs`:

| Server logical name | Description                                                       |
|---------------------|-------------------------------------------------------------------|
| `app`               | Woom app navigation. See [`MCP.md`](MCP.md).                 |
| `github`            | GitHub read + propose-commit/PR/bash/switch-cwd writes.           |
| `jira`              | Atlassian read + write (markdown→ADF).                            |
| `sentry`            | Sentry read + write (status / comments).                          |
| `memory`            | Local KV store for cross-session notes (`memory_save`, `_search`, `_list`, `_delete`). |

`ToolProfile` filters this catalog per session (`claude_mcp.rs:50-172`):

```rust
pub enum ToolProfile { All, Coding, Github, Jira, Sentry, Triage }
```

Each profile defines an allow-list of `mcp__*__*` tool name patterns;
the rest are dropped from the `--allowedTools` list passed to Claude
or filtered server-side for Cursor.

The agent context preamble (`apps/desktop/src/lib/services/agentContext.ts`)
documents the `mcp__app__set_editor_repo_path` / `mcp__app__set_agent_cwd`
contract in plain English so the model knows to call them. See full
text starting at `agentContext.ts:40-55`.

---

## 12. Sidecar Process

### 12.1 Claude

`apps/desktop/src-tauri/src/claude.rs`:

```text
claude_ask(session_id, prompt, ... )
  → spawns `claude` subprocess with:
      --output-format stream-json
      --allowedTools <list>
      --mcp-config <temp file>
      --resume <claudeUuid>      # if set
      cwd: resolved cwd
  → drain stderr (toast on failure)
  → for each stdout line:
      app.emit("claude:stream:{id}", line)
  → register pid in Runners

claude_stop(session_id)
  → SIGTERM the recorded pid
```

Exact spawn at `claude.rs:694-705`.

### 12.2 Cursor

`apps/desktop/src-tauri/src/cursor.rs:152-177`:

```rust
cmd.arg("--print")
   .arg("--output-format").arg("stream-json")
   .arg("--stream-partial-output")
   .arg("--resume").arg(&chat_id)
   .arg("--force")
   .arg("--approve-mcps")
   .arg("--trust");
if let Some(model) = ... { cmd.arg("--model").arg(model); }
if let Some(workspace) = ... { cmd.arg("--workspace").arg(workspace); }
cmd.current_dir(cwd);
```

Same `Runners` registry, same `claude:stream:{id}` channel post-normalize.

### 12.3 Bin discovery

`agent.rs::resolve_bin(kind)` walks `$PATH` and falls back to a few
canonical install locations (e.g. Anthropic's installer drop). If
unfound → returns a typed error → frontend shows a status modal with a
"Install Claude" link.

---

## 13. Persistence

| Field                              | Where                              |
|------------------------------------|------------------------------------|
| Sessions list, activeId            | `localStorage: woom:claude-sessions:v1` |
| Per-instance active session        | `sessionsState.activeByInstance` (in same blob) |
| Editor instance state              | `localStorage: woom:editor-state:v1` |
| Rules templates (system prompts)   | `localStorage: woom:claude-rules:v1` |
| Worktree on-disk dirs              | filesystem (`worktreePath`)        |
| Memory MCP db                      | `WOOM_MEMORY_DB` env (sqlite) |

There is **no** per-session JSON on disk. The whole conversation lives
in `localStorage`. Quota is a real risk (~5 MB) for long chats with
many edit hunks; see `persistError` toast in `sessions.svelte.ts`.

---

## 14. History View

There is **no** dedicated history page in v1. Recall:

- **Per-column tab strip** — `chat-tabs` inside `AgentColumn.svelte:1364-1396`,
  one tab per session of that `agentKind`.
- **Command palette** indexes column metadata only; you can find the
  column "Sagrada-Familia (Claude)" but not a phrase from a 200-message
  chat. See `docs/COMMAND_PALETTE.md`.
- **Manual scroll** in the column body.

---

## 15. Permissions / Approval

The session's `awaitingApproval: boolean` blocks the composer with a
hint when there are `pending` actions. Once all `pending`/`executing`
slots resolve, `awaitingApproval` clears and a synthetic continuation
prompt is auto-sent (see §7).

There is no fine-grained "tool X needs approval" — the approval surface
is just the four `propose_*` tool kinds. Other tools (read,
search, create_issue, etc.) execute as MCP calls without approval.

---

## 16. Status Indicators

| State             | Source                                  | Visual                                     |
|-------------------|-----------------------------------------|--------------------------------------------|
| Sending           | `sess.sending`                          | `chat-typing` pulsing dot + thinking timer |
| Tool running      | last event is `'trace'` / unresolved    | "N steps" pill on the assistant message    |
| Awaiting approval | `sess.awaitingApproval`                 | `.awaiting-approval` row above composer    |
| Errored           | last assistant content begins with `**Claude failed:**` | Red banner + retry button       |
| Idle              | none of the above                       | Composer focused, history scrollable       |

Thinking time is from `Date.now() - sess.lastSendAt` ticked by `thinkingTick`
(`AgentColumn.svelte:1624-1632`).

---

## 17. Special Surfaces

### 17.1 Canvas linking

A session can link to one canvas (`linkedCanvasId`, `linkedCanvasInstanceId`).
When linked, the agent system preamble grows a canvas summary (see
`agentContext.ts` + `+page.svelte:1562-1585`) and `canvas.*` MCP tools
are exposed (per [`CANVAS.md §10.4`](CANVAS.md#104-writing--the-mcp-tool-catalog)).

A live PNG snapshot is produced on demand by `saveCanvasScreenshot` and
fed to the agent via the image-input channel.

### 17.2 Worktree modal

`WorktreeDiffModal` (mounted from `+page.svelte:4145-4152`) shows the
diff between the agent's worktree and the original repo HEAD when an
agent finishes work in an isolated worktree. Approve → merges into the
linked editor's repo path; reject → leaves the worktree untouched.

### 17.3 "Open in Trail"

References to a "Trail bench" in older transcripts and code comments
were a scrapped feature; no UI ships under that name today. If you see
the string in old comments, it's stale.

---

## 18. Keyboard

| Key                 | Scope          | Action                                    |
|---------------------|----------------|-------------------------------------------|
| `Enter`             | composer       | Send (unless mention popover open)         |
| `Shift+Enter`       | composer       | Newline                                    |
| `Esc`               | composer       | Cancel mention popover; otherwise no-op   |
| `⌘↵` / `Ctrl+↵`     | message edit   | Commit edit                                |
| `Esc`               | message edit   | Cancel edit                                |
| `Enter`             | tab close `✕`  | Close that chat tab                        |
| `⌘K` / `Ctrl+K`     | global         | Toggle Command Palette                     |

---

## 19. Open TODOs / Known Issues

1. **Stale comment in `+page.svelte:3028-3030`** — `// ---- Claude
   stub flow ----` and "Real agent execution is the next milestone"
   doesn't match the now-shipped real path. Worth a sweep.
2. **Quota** — `localStorage` is finite; long sessions with many big
   edits eventually hit the quota. We surface a toast but don't
   auto-evict; a "compact older messages" command would help.
3. **No per-session JSON on disk.** Single bad write to localStorage
   can lose all sessions; persistence layer needs hardening.
4. **No structured slash commands** in v1; could expose
   `/compact`, `/clear`, `/checkout <branch>` purely as Forge UX.
5. **Session search** is missing — chat tabs grow indefinitely.
6. **Codex / Aider / Copilot** are placeholders only; building them
   means a new sidecar adapter, plus disambiguating `agentKind`.
7. **Action approval UX** is two buttons; a "review the diff first"
   step for `propose_pr` would be valuable for non-trivial PRs.

---

## 20. Glossary

- **Agent column** — a workbench column of `kind === 'claude' | 'cursor'`.
- **Session** — `ClaudeSession`, the conversation object — name kept for
  historical reasons, applies to both kinds.
- **Edit card** — UI for an `Edit` / `Write` / `MultiEdit` / `Delete`
  tool result. Lives inside the assistant message.
- **Action card** — UI for a `propose_*` tool call awaiting user OK.
- **Worktree** — `git worktree` directory the agent created to keep its
  changes separate from the linked editor's repo.
- **Cwd switch recap** — synthetic system message injected on next send
  whenever the agent's cwd changed since the last turn, so the model
  notices the move.
- **`claude:stream:{id}`** — the single Tauri event channel for
  per-session stdout (used by both Claude and Cursor sidecars).
- **`ToolProfile`** — Rust enum that filters which MCP tools the agent
  is allowed to invoke this session.
