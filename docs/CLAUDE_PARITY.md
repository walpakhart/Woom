# Claude Code parity — research, spec, phased plan

> Goal: cherry-pick the highest-leverage ideas from Anthropic's `claude` CLI and bake them into Woom's existing claude/cursor adapter without forking the project off-course. User-stated motivator: **a preview/dev-server surface** where the agent can run, monitor, and expose running processes. Around that, we ladder in hooks, skills, plan mode, agent view, /loop, /fork, statusline, tool search, CLAUDE.md auto-load — in that priority order.

---

## 0. Where Woom stands today (baseline)

What already works (don't redo):
- **Process layer** (`apps/desktop/src-tauri/src/claude.rs`, `lib/exec/claude.ts`) — shells out to `claude`/`cursor-agent` with `--output-format stream-json`, `--mcp-config`, `--resume`, warm-pool prewarming (~150s idle), orphan-session detection with cwd-recap injection.
- **Stream parser** (`lib/stream/agentStream.ts`) — line-by-line JSON events → `onAssistantDelta` / `onThinkingDelta` / `onTraceDelta` / `onToolUse` / `onUsage` callbacks. tool_use is intercepted **post-hoc** to synthesize diff cards.
- **Session storage** (`lib/state/sessions.svelte.ts`, `lib/types.ts`) — `ClaudeSession` with linked editor/canvas/terminal instance ids, `worktreePath/worktreeBranch`, `cwdUuids`, `cwdSwitchRecap`, `lastContextSize`, debounced JSON persistence.
- **MCP plumbing** (`claude_mcp.rs`, `cursor_mcp.rs`) — 5 bundled sidecars (woom-jira/github/sentry/memory/app), per-session temp config, `--strict-mcp-config`, user-server merge from `~/.claude.json` (commit `57e33f3`).
- **Worktree isolation** (`worktree.rs`) — per-session branches under `~/Library/Application Support/Woom/worktrees/<sid>/`.
- **Action approval IPC** — Unix sockets `/tmp/woom-action-<pid>.sock` for blocking `propose_commit` / `propose_pr` / `propose_bash` / `propose_switch_cwd`.
- **Slash commands** (`lib/services/slashCommands.ts`) — only `/compact`, `/clear`, `/usage`, `/help`. Closed set, dispatched client-side, not forwarded to CLI.
- **Memory** (`memory_local.rs`) — SQLite + FTS5 store. `memory_save_local` / `memory_search_local` exposed as Tauri commands and as MCP tools via `woom-memory` sidecar. **Not** auto-loaded into first-turn preamble.
- **Terminal bridge** (`terminal_bridge.rs`) — HTTP `127.0.0.1:<port>` with `POST /:id/run` (subprocess-per-call, hardened env: `PAGER=cat NO_COLOR=1 CI=1`, stdin=null), `POST /:id/write`, `GET /:id/buffer`. **No** long-running daemon mode.

Gaps relative to Claude Code:
- No pre-tool hooks (post-hoc intercept only).
- No plan mode (no `--permission-mode plan`, no ExitPlanMode modal).
- No CLAUDE.md auto-load.
- No skill registry / no `!`-shell injection in user-defined prompts.
- No long-running process registry (every `terminal_run` is one-shot).
- No "preview" surface (clickable localhost URLs, port-scanner, browser pane).
- No agent-view dashboard (multi-session table with Haiku one-liners, PR dots).
- No statusline-as-script (model/cwd/cost piped via stdin JSON).
- No `/loop` / `/fork` / `/btw`.
- No tool-search for MCP (every server's full tool schema goes into context).
- Memory exists but no auto-attach.

---

## 1. North-star UX

After all phases shipped, the user should be able to:

1. Type `/preview pnpm dev` in any Claude/Cursor session → Woom spawns a tracked background process, opens a **Preview** pane (in Canvas-style or new solo) showing live logs **and** an embedded webview pointed at the detected `http://localhost:5173`. Agent gets per-line events through a `Monitor` MCP tool.
2. Type `/skill review-pr` → loads `~/.claude/skills/review-pr/SKILL.md` whose body contains `` !`gh pr diff` `` placeholders, pre-resolved before Claude sees them.
3. Hit `Shift+Tab` → cycles permission mode `default → acceptEdits → plan → auto`. In `plan`, edits are gated; on `ExitPlanMode`, a 4-button modal asks "review each / accept all / auto / keep planning".
4. Drop a payload on Canvas rail (already shipped 🎉).
5. Open `⌘K` → "agents" view: a unified table of all sessions across Claude+Cursor, grouped by **Needs input / Working / Ready for review / Completed**, with a Haiku-generated one-liner per row, refreshed every 15s.
6. Type `/loop 5m check the deploy` → CronCreate wakes the agent every 5 min until task completes.
7. Statusline at the bottom of every solo: `claude-sonnet-4-6 · woom · $1.42 · 67% context · 5h: 23%`.
8. New session → `CLAUDE.md` walked from cwd up to repo root, concatenated, prepended as first system message.
9. Memory: at session start, top 200 lines of `~/.../woom-memory/MEMORY.md` injected automatically.

---

## 2. Phased plan

Each phase = standalone PR(s), shippable independently. Estimated effort is fingers-in-the-air (1 = afternoon, 5 = ~2 weeks solo).

| # | Phase | Effort | Depends on | Ships |
|---|------|--------|-----------|-------|
| 1 | Dev-server / Preview | 4 | — | `BackgroundTasks` registry, Preview solo, Monitor MCP tool, port autodetect |
| 2 | Hooks (Pre/Post tool, SessionStart) | 3 | — | `settings.hooks.*` schema, hook runner, `CLAUDE_ENV_FILE` |
| 3 | Skills + `!`-shell injection | 3 | — | `.claude/skills/` discovery, frontmatter parser, slash command extension |
| 4 | Plan mode + ExitPlanMode modal | 2 | hooks (3) | Shift+Tab cycle, gated tool list, modal UI |
| 5 | Agent View | 3 | — | `⌘K agents` overlay, status groups, Haiku summaries via cheap model |
| 6 | /loop + /fork + /btw | 2 | hooks (3) | CronCreate tool family, fork tool, btw-isolated-call |
| 7 | Statusline + CLAUDE.md + auto-memory | 2 | — | per-solo statusbar JSON pipe, file walker, MEMORY.md first-turn inject |
| 8 | Tool Search for MCP | 2 | — | tool catalog cache, ToolSearch synthesized tool, deferred schemas |

Total nominal effort: ~21 person-days. Real-world ×1.5 for polish/regressions → ~5 calendar weeks if one engineer, full focus.

---

## Phase 1 — Dev-server / Preview (USER ASK)

### 1.1 Problem

Today every `terminal_run` is fire-and-forget (subprocess-per-call). Agent can't `pnpm dev` and then react to compilation errors. User can't see running servers, find their URLs, or kill them from one place.

### 1.2 Spec

#### 1.2.1 BackgroundTasks registry (Tauri)

New module `src-tauri/src/bg_tasks.rs`:

```rust
pub struct BgTask {
  pub id: String,           // `bg-{short_random}`
  pub label: String,        // user-given or auto-derived from cmd
  pub cmd: String,
  pub cwd: String,
  pub session_id: Option<String>, // origin session
  pub pid: u32,
  pub started_at: u64,
  pub status: TaskStatus,   // Running | Exited(i32) | Killed
  pub stdout_path: String,  // rolling file on disk
  pub stderr_path: String,
  pub detected_urls: Vec<String>, // populated by URL scanner
  pub detected_ports: Vec<u16>,
}

pub enum TaskStatus { Running, Exited(i32), Killed }
```

Storage: in-memory `HashMap<String, BgTask>` + on-disk metadata at `~/Library/Application Support/Woom/bg-tasks/<id>.json` so tasks survive app restarts (process itself doesn't — restart marks status=Killed with reason="app-restart").

Tauri commands:
- `bg_spawn(cmd, cwd, label?, session_id?) -> BgTask` — `tokio::process::Command`, pipes stdout+stderr to rolling files (10MB cap, rotate to `.1`), emits `bg:line:<id>` Tauri event per stdout line.
- `bg_list() -> Vec<BgTask>` — sorted by `started_at desc`.
- `bg_kill(id) -> ()` — `kill -TERM`, then `-KILL` after 3s.
- `bg_logs(id, tail?: usize) -> String` — read rolling file.
- `bg_send_stdin(id, data) -> ()` — for interactive servers.

URL/port autodetect: regex `\bhttps?://(?:localhost|127\.0\.0\.1)(?::(\d+))?\S*` on every line. First match per task auto-fills `detected_urls`; subsequent unique matches append.

#### 1.2.2 Monitor MCP tool

Add to `woom-app` sidecar (`apps/desktop/src-tauri/sidecars/woom-app/src/`):

```jsonc
{
  "name": "bg_spawn",
  "description": "Start a long-running command in the background. Returns a task id immediately. Each new stdout line surfaces as a notification you can react to mid-turn.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "cmd": { "type": "string" },
      "cwd": { "type": "string" },
      "label": { "type": "string" }
    },
    "required": ["cmd"]
  }
}
```

Plus `bg_list`, `bg_logs`, `bg_kill`, `bg_send_stdin`, **and `bg_wait_line`** — the killer feature: a long-poll tool that blocks up to N seconds waiting for the next stdout line matching an optional regex. Implementation: Tauri command broadcasts new lines to an in-process pub/sub; the sidecar tool subscribes for the requested timeout.

This matches Anthropic's `Monitor` tool semantics ("each output line = one event") without forcing us to invent a streaming-tool extension to MCP.

#### 1.2.3 Preview solo

New `kind=preview` in `layoutState.instances`, with art-name pool (e.g. "Hokusai", "Bosch"). Surface = split pane:

```
┌──────────────────────────────────┬─────────────────────────┐
│  task list                       │  active task detail     │
│  ├─ pnpm dev (5173) ● running    │  ┌──────────┐ ┌──────┐  │
│  ├─ pnpm test:watch  ● running   │  │ webview  │ │ logs │  │
│  └─ cargo build      ● exited(0) │  │  iframe  │ │ tail │  │
│                                  │  └──────────┘ └──────┘  │
│  [+ New task]                    │  cmd · cwd · pid · age  │
│                                  │  [kill] [restart] [...] │
└──────────────────────────────────┴─────────────────────────┘
```

Webview uses Tauri's WebviewWindow or, simpler, a `<webview>` tag (Tauri allows after config). Falls back to "Open in browser" link if iframe blocked by CSP.

Rail icon: new entry in `Rail.svelte` between Canvas and Terminal. Tone: `var(--src-preview)` — pick aqua-leaning teal distinct from canvas. Drop-target wiring (later phase): can drop a file path → spawn `npx serve <path>`.

#### 1.2.4 Slash command sugar

Inside Claude/Cursor session composer:
- `/preview pnpm dev` → calls `bg_spawn` via the MCP tool, switches view to preview solo, focuses the new task row.
- `/kill <id|label-substr>` → `bg_kill`.
- `/ps` → renders task list inline in chat (MCP tool call wrapped).

These are NOT forwarded to CLI — they're parsed in `slashCommands.ts` and dispatched to `bg_*` Tauri commands directly.

### 1.3 Risk / open questions

- **Webview CSP**: many dev servers (Vite) ship strict CSP that breaks iframe embedding. Fallback: a separate Tauri WebviewWindow per task (heavier but always works).
- **Process leak on app crash**: parent-death detection via `prctl(PR_SET_PDEATHSIG)` on Linux, `posix_spawn` + signal-on-parent-exit on mac. Track child pids in a sidecar file so a cold-start can reap orphans.
- **Disk pressure**: 10MB×N tasks. Per-task LRU eviction of old `.1` rotation file.
- **Monitor tool starvation**: a chatty server (1000 lines/s) can flood the agent. Per-line throttle: deliver max 30 lines/s to a single `bg_wait_line` listener, summarize the rest as "N lines skipped" line.

### 1.4 Deliverables

- [ ] `src-tauri/src/bg_tasks.rs` — process, rolling logs, URL scanner.
- [ ] `bg_*` Tauri commands registered in `lib.rs`.
- [ ] `woom-app` sidecar: 5 new tools (`bg_spawn`, `bg_list`, `bg_logs`, `bg_kill`, `bg_send_stdin`, `bg_wait_line`).
- [ ] `lib/state/preview.svelte.ts` — reactive store mirroring `bg_list()`.
- [ ] `lib/views/apps/PreviewApp.svelte` + `preview/` subfolder (TaskList, TaskDetail, WebviewPane, LogTail).
- [ ] `Rail.svelte` — `RailAppButton` for `kind=preview`, tone+glow vars.
- [ ] `slashCommands.ts` — `/preview`, `/kill`, `/ps`.
- [ ] Cheatsheet entry under new "Preview" section.

---

## Phase 2 — Hooks

### 2.1 Spec

#### 2.1.1 Settings schema

`apps/desktop/.claude-style-settings.json` (or extend existing `~/Library/Application Support/Woom/settings.json`):

```jsonc
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Bash",
        "handler": { "type": "command", "command": "/path/to/script" },
        "timeout": 5000
      }
    ],
    "PostToolUse": [...],
    "SessionStart": [...],
    "UserPromptSubmit": [...]
  }
}
```

Subset of Claude Code's 31 events — start with the 5 highest-leverage:
1. **PreToolUse** — gate/rewrite tool calls (especially `Bash`).
2. **PostToolUse** — react to results (auto-format after edit, etc.).
3. **SessionStart** — env injection, context preload.
4. **UserPromptSubmit** — last-mile prompt rewriting (e.g. auto-prepend cwd context).
5. **Stop** — turn-finished notification.

#### 2.1.2 Handler contract

Stdin JSON (shape stolen from Claude Code docs):
```jsonc
{
  "session_id": "ses_...",
  "hook_event_name": "PreToolUse",
  "cwd": "/Users/.../woom",
  "tool_name": "Bash",
  "tool_input": { "command": "rm -rf /tmp/foo" },
  "permission_mode": "default"
}
```

Stdout JSON:
```jsonc
{
  "permissionDecision": "ask" | "allow" | "deny",
  "reason": "...",
  "updatedInput": { "command": "rm -rf /tmp/foo  # confirmed" }, // rewrites
  "additionalContext": "..."
}
```

Exit `0` = pass-through; exit `2` = block with stderr as feedback; other = warn-only.

#### 2.1.3 Runner

New module `src-tauri/src/hooks.rs`:

```rust
pub async fn run_hook(event: HookEvent, payload: serde_json::Value)
  -> Result<HookOutcome, HookError>
```

Called from `agentStream.ts` (frontend → Tauri command) at the right lifecycle points. **Critical**: `PreToolUse` needs to fire *before* the CLI runs the tool. Today we can only intercept after. Two options:

- **A. Stop forwarding tool_use to CLI** — when agent emits `tool_use`, we hold the message, run hook, optionally rewrite input via a `tool_use_modified` event we synthesize. Heavy: we now own tool dispatch.
- **B. Wrap tools in a sidecar that calls hooks** — only works for tools we proxy (Bash, Edit, Write). Limited but tractable.

Recommend B for phase 2. Phase later: build A for full parity.

#### 2.1.4 SessionStart env file

When SessionStart hook writes to `$CLAUDE_ENV_FILE` (path we pass in env), subsequent `Bash` calls in the session inherit those exports. Implementation: hook returns an env-patch JSON; we persist to session state; `terminal_run` merges into env before spawning.

### 2.2 Deliverables

- [ ] `hooks.rs` — runner, event types, sandboxed exec (15s timeout default).
- [ ] Settings UI: `SettingsView.svelte` adds "Hooks" section.
- [ ] `agentStream.ts` — hook invocation points (PreToolUse for Bash/Edit/Write; PostToolUse for all; SessionStart on session create/resume; UserPromptSubmit before send).
- [ ] Default hook examples shipped: "auto-format after Edit", "block rm -rf without confirmation".

---

## Phase 3 — Skills + `!`-shell injection

### 3.1 Spec

#### 3.1.1 Discovery

Walk these dirs at session start (with file watcher for live reload):
1. `~/.claude/skills/<name>/SKILL.md` — user
2. `.claude/skills/<name>/SKILL.md` — project (walk up from cwd to repo root)
3. Bundled skills under `apps/desktop/skills/` (ship a few defaults)

#### 3.1.2 Frontmatter parser (subset)

```yaml
---
name: review-pr
description: Review the active GitHub PR
when_to_use: User says "review pr" or pastes a PR link
argument-hint: "<pr-number>"
allowed-tools: ["mcp__github__get_pr_diff", "mcp__github__add_comment"]
model: claude-sonnet-4-6
---
```

Implement only: `name`, `description`, `when_to_use`, `argument-hint`, `allowed-tools`, `model`. Defer `paths:`, `disable-model-invocation`, `context: fork` to a later phase.

#### 3.1.3 Body template

```markdown
The PR is:

!`gh pr view $ARGUMENTS --json title,body,files`

Diff:

!`gh pr diff $ARGUMENTS`

Now review using these guidelines: ...
```

Shell injection: `` !`...` `` runs the command (with `tokio::process::Command`, 30s timeout, 100KB stdout cap), replaces inline. Multi-line via fenced ```! blocks.

Run BEFORE Claude sees the skill content (resolves into the first user message that triggers the skill).

#### 3.1.4 Dispatch

Slash menu autocompletion: when user types `/`, show registered skills + built-ins. Selecting one inserts `/skill-name ` with cursor at args. On enter, frontend:
1. Loads skill body, substitutes `$ARGUMENTS`, runs `!` blocks.
2. Sends as user message with prefix `[skill:review-pr]`.
3. Optionally narrows tool allowlist for that turn (if `allowed-tools` present).

### 3.2 Deliverables

- [ ] `lib/services/skills.ts` — discovery, frontmatter parser, body renderer with `!` resolver.
- [ ] `lib/state/skills.svelte.ts` — reactive registry.
- [ ] `Composer.svelte` — autocomplete menu integrates skills next to existing built-ins.
- [ ] Bundle 3 default skills under `apps/desktop/skills/`: `review-pr`, `debug-test`, `summarize-changes`.

---

## Phase 4 — Plan mode

### 4.1 Spec

#### 4.1.1 Permission mode state

New session field: `permissionMode: 'default' | 'acceptEdits' | 'plan' | 'auto'`. Persisted; defaults to `default`. Cycled via `Shift+Tab` in composer (already used by something? check first).

#### 4.1.2 In `plan` mode

Hooks fire and DENY any tool in `{Edit, Write, Bash}` (unless command matches read-only allowlist: `git log`, `git diff`, `git status`, `ls`, `cat`, `rg`, `grep`, `find`, `pwd`, `which`, `node -v`, etc.).

Status banner in chat thread when mode != default.

#### 4.1.3 ExitPlanMode tool

Synthetic tool exposed via `woom-app` sidecar:
```jsonc
{
  "name": "exit_plan_mode",
  "description": "Present the planned approach to the user and wait for approval.",
  "inputSchema": { "type": "object", "properties": { "plan": { "type": "string" } }, "required": ["plan"] }
}
```

When agent calls it, frontend opens modal:

```
┌─ Plan ────────────────────────────────────┐
│  <rendered markdown of `plan` input>      │
│                                           │
│  [Review each edit]                       │
│  [Accept all edits]                       │
│  [Auto mode]                              │
│  [Keep planning]      [⌘E edit in editor] │
└───────────────────────────────────────────┘
```

On approval → `permissionMode` flips to chosen mode, agent gets `tool_result` with chosen mode. On "Keep planning" → tool_result is `{ status: "keep_planning", feedback?: "..." }`.

`⌘E` opens the plan markdown in the editor solo (linked editor instance if any, else primary). On save, re-injects edited text as new tool_result.

### 4.2 Deliverables

- [ ] `lib/state/sessions.svelte.ts` — add `permissionMode` field.
- [ ] Composer keybinding: Shift+Tab cycles mode.
- [ ] `lib/views/apps/agent/PlanModeModal.svelte`.
- [ ] `claude_mcp.rs` / `cursor_mcp.rs` — sidecar `exit_plan_mode` tool.
- [ ] Tool allowlist enforcement in PreToolUse hook (depends on Phase 2 partial).

---

## Phase 5 — Agent View

### 5.1 Spec

A `⌘K` overlay (or new home subview) showing all sessions across Claude+Cursor in groups:

- **Needs input** — sessions where last message is `tool_use awaiting approval` OR an unresolved user mention.
- **Working** — `sending: true` in `sessionsState.activeIds`.
- **Ready for review** — last assistant message ended without a follow-up question, has PR-related actions.
- **Completed** — no activity in N hours, no pending state.
- **Pinned** — user-flagged.

Per row:
- Animated icon (busy spinner or `∙` idle).
- Source-tinted dot (claude=rust, cursor=silver).
- Linked GitHub PR badge (state dot: open=green, draft=yellow, merged=purple, closed=grey) — derive from `worktreeBranch` → search GitHub inbox for matching PR.
- One-line Haiku summary (see §5.2).
- Last activity timestamp.

Hover/Space = peek panel with last 3 messages + smart-reply hints. Enter = activate session (switch view + select).

### 5.2 Haiku summarizer

Background job: every 15s, for sessions changed since last summary, call Anthropic API directly (no CLI) with `claude-haiku-4-5` and a 100-token cap:

```
You are summarizing one ongoing coding session in one line ≤80 chars.
Last 3 messages:
<user>: ...
<assistant>: ...
<user>: ...
Output: the line, no preamble.
```

Cost ~$0.0001/summary × 50 sessions × 240 refreshes/hour ≈ $1.2/hour worst case. Throttle: only summarize sessions with `changedAt > lastSummaryAt`. Cache hits = free.

Need a Tauri command `anthropic_haiku_summary(messages)` using the user's stored API key (if set). If no API key, skip summaries; show last message excerpt instead.

### 5.3 Deliverables

- [ ] `lib/views/apps/AgentView.svelte` — group rendering, filter chips.
- [ ] `lib/state/agentView.svelte.ts` — derived store grouping sessions.
- [ ] `lib/services/sessionSummary.ts` — Haiku worker.
- [ ] `⌘K` command palette entry "Agents view".
- [ ] PR-state resolver: link session worktree branch → GitHub PR.

---

## Phase 6 — /loop, /fork, /btw

### 6.1 /loop

Reuses Claude Code's slash semantics:
- `/loop 5m <prompt>` — fixed cadence.
- `/loop <prompt>` — agent picks next interval (60–3600s).
- `/loop` alone — load default loop prompt from `~/.claude/loop.md`.

Backed by Tauri tasks scheduled via tokio + a `bg_tasks`-style registry (Phase 1 reuse). On fire, post `prompt` to session as new user message (queue if turn in flight, similar to existing `pendingQueue`).

7-day expiry hard cap. `Esc` cancels pending wakeups. CronCreate/CronList/CronDelete exposed as MCP tools so agent can manage its own loops.

### 6.2 /fork

Spawn a sibling session inheriting the parent's full message history. New session id, `parentSessionId` field, `forkedAt` timestamp. Runs in background by default — surfaces in the Agent View "Working" group.

Implementation: copy `messages[]` into new session, call `claude --resume <new-uuid>` with a fresh transcript. Prompt cache is best-effort (CLI handles).

UI: panel below composer with running forks, ↑/↓/Enter/x controls (per Claude Code's design).

### 6.3 /btw

"Side question." Sends the entire conversation to the agent but the response is discarded (not added to `messages[]`) — just shown in a transient toast/modal.

Implementation: short-circuit `claude --resume` path. Use one-shot `claude -p --append-system-prompt 'You are answering a side question; do not call tools' --output-format json` with full message history serialized.

### 6.4 Deliverables

- [ ] `lib/state/loops.svelte.ts` — schedule, fire, cancel.
- [ ] `lib/services/fork.ts` — copy session, spawn child.
- [ ] `lib/services/btw.ts` — headless one-shot.
- [ ] `slashCommands.ts` — register `/loop`, `/fork`, `/btw`.
- [ ] `woom-app` sidecar: `cron_create`, `cron_list`, `cron_delete` tools.

---

## Phase 7 — Statusline + CLAUDE.md + Auto-memory

### 7.1 Statusline

Settings entry:
```jsonc
{
  "statusLine": {
    "type": "command",
    "command": "/Users/me/.config/woom/statusline.sh",
    "refreshInterval": 30
  }
}
```

Pipe JSON to stdin matching Claude Code's contract:
```jsonc
{
  "model": { "id": "claude-sonnet-4-6", "display_name": "Sonnet 4.6" },
  "cwd": "/Users/.../woom",
  "workspace": { "current_dir": "...", "project_dir": "...", "git_worktree": "..." },
  "cost": { "total_cost_usd": 1.42, "total_lines_added": 234, "total_lines_removed": 12 },
  "context_window": { "used_percentage": 67, "context_window_size": 200000 },
  "session_id": "...",
  "session_name": "..."
}
```

Render multi-line output in a 22px bar at bottom of agent solo (replace today's static info row). ANSI colors via `ansi_up` or similar.

Bonus: `/statusline <natural language>` skill that generates the script for the user.

### 7.2 CLAUDE.md auto-load

On session start (or cwd change), walk:
1. `~/.claude/CLAUDE.md` (user)
2. `.claude/CLAUDE.md` and `CLAUDE.md` from cwd up to repo root, concatenated.
3. `CLAUDE.local.md` if present (gitignored).

Strip `<!-- -->` comments. Support `@path/to/file.md` imports (recursive ≤ 5).

Inject as **first user message** in session (not system prompt) to match Claude Code's behavior — survives `/clear`-style compaction differently than system prompt.

### 7.3 Auto-memory boost

`memory_local.rs` already has FTS5. New behavior:
- At session start, fetch top 200 lines of "user kind" memories + a search on cwd-derived keywords → inject as first user message under heading `<memory>` ... `</memory>`.
- After every assistant turn, run a cheap LLM call (`claude-haiku-4-5`) on the last user+assistant exchange asking "is anything here worth remembering long-term?". If yes → `memory_save_local` with auto-tags.

(Already half-built; just wire the inject + heuristic.)

### 7.4 Deliverables

- [ ] `lib/services/statusline.ts` — JSON builder, command spawner, output renderer.
- [ ] `lib/services/claudemd.ts` — file walker, comment stripper, import resolver.
- [ ] `lib/services/autoMemory.ts` — first-turn inject + post-turn classifier.
- [ ] Settings UI entries for all three.

---

## Phase 8 — Tool Search for MCP

### 8.1 Problem

Today we serialize EVERY tool from EVERY MCP server into the system prompt at session start. Five sidecars × ~10 tools each = 50 tool schemas at minimum. Plus user-registered MCP servers. Eats ~5-15k tokens before the user types anything.

### 8.2 Spec

At session start:
- Build a flat catalog: `{ tool_name, server, brief: <30 words of description> }[]`.
- System prompt gets only catalog (name + brief), not schemas.
- Synthesize a `tool_search` tool whose schema is:
  ```jsonc
  {
    "name": "tool_search",
    "description": "Find and load tool schemas matching a query. Tools must be loaded before they can be called. ...",
    "inputSchema": {
      "type": "object",
      "properties": {
        "query": { "type": "string", "description": "select:<name>[,<name>...] or keyword search" },
        "max_results": { "type": "number" }
      },
      "required": ["query"]
    }
  }
  ```
- When agent calls `tool_search` with `select:foo,bar` or `keyword`, return matching schemas as a tool_result that the agent can immediately use.

This is exactly the contract Anthropic uses (visible in this very prompt — see ToolSearch tool description).

### 8.3 Toggle

Setting `enableToolSearch: 'auto' | 'on' | 'off'` (auto: enable when catalog > 30 tools). `alwaysLoad: true` per server in settings to opt out.

### 8.4 Deliverables

- [ ] `lib/services/toolCatalog.ts` — builds catalog from MCP server lists.
- [ ] `claude_mcp.rs` — inject tool_search as a synthetic tool when feature on.
- [ ] Settings UI: toggle + per-server alwaysLoad checkbox.

---

## 3. Cross-cutting concerns

### 3.1 Type contracts

Centralize in `lib/types/claude-code-shapes.ts`:
- `HookEvent`, `HookPayload`, `HookOutcome`.
- `StatuslineState` (the JSON shape).
- `SkillFrontmatter`.
- `BgTask`, `BgTaskStatus`.
- `PermissionMode`.

Mirror in Rust under `src-tauri/src/claude_code_shapes.rs` with `serde` derives.

### 3.2 Settings file

Today: ad-hoc keys under `~/Library/Application Support/Woom/settings.json`. Migrate to namespaced sections:

```jsonc
{
  "claude_code": {
    "hooks": { "PreToolUse": [...] },
    "statusLine": { "type": "command", "command": "...", "refreshInterval": 30 },
    "skills": { "userDir": "~/.claude/skills" },
    "memory": { "autoInjectEnabled": true, "topLines": 200 },
    "toolSearch": { "mode": "auto", "alwaysLoadServers": ["woom-memory"] },
    "permissionMode": "default"
  }
}
```

Settings migration: on app start, move existing keys into namespace, keep backwards-compatible reads for one minor version.

### 3.3 Telemetry / cost

Phases 5 and 7 introduce LLM calls outside the CLI (Haiku summaries, auto-memory classifier). Track separately:
- New `costState.haikuUsd`, `costState.classifierUsd`.
- Soft per-day cap in settings (default $5).
- Disable features gracefully when cap hit; show banner.

### 3.4 Testing

Existing test surface (`apps/desktop/sessionCwd.test.ts` etc.) — add:
- `bg_tasks_test.rs` — process lifecycle, URL detection, log rotation.
- `hooks_test.rs` — runner, timeout, exit code semantics.
- `skills_test.ts` — frontmatter parser, `!` shell-block resolver (mock subprocess), import resolver.
- `plan_mode_test.ts` — Shift+Tab cycle, denied-tool list.
- `tool_catalog_test.ts` — catalog build, search query parser.

### 3.5 Documentation

Each phase ships docs in `docs/CLAUDE_PARITY_<phase>.md` (P1 = preview, P2 = hooks, etc.), plus updates to:
- `docs/AGENTS.md` — session model, permission modes, hooks lifecycle.
- `docs/MCP.md` — tool search, monitor tool family.
- `docs/UI.md` — new solo, statusbar, plan-mode modal.

---

## 4. Open decisions for the user

These need answers before Phase 1 starts:

1. **Preview solo OR Canvas-side card?** Spec assumes new solo. Alt: render preview tasks as live cards in Canvas (reusing existing card infra). Solo is cleaner but more code.
2. **Webview engine.** Tauri allows `tauri::WebviewWindow` (separate window) or `<webview>` HTML tag (inline). Inline is nicer UX but loses some sites due to CSP. Pick one as default.
3. **Hook config location.** Settings.json key or separate `~/.config/woom/hooks.json`? Claude Code uses settings.json — recommend matching.
4. **Skill discovery roots.** Mirror Claude Code's `~/.claude/skills` (share with user's existing Claude Code install) OR isolate under `~/.config/woom/skills`? Recommend mirroring — zero-config for existing Claude Code users.
5. **Plan-mode default for new sessions.** Per-project override allowed? (Claude Code allows `"permissions": { "defaultMode": "plan" }`.)
6. **Statusline placement.** Bottom of agent solo only, OR a global app-bottom strip visible across all solos? Global has cohesion; per-solo gives more space.
7. **Haiku API key.** Use user-stored Anthropic key (settings)? Skip feature if unset? Or hit Bedrock if configured?

---

## 5. What I'd build first

If forced to ship a single PR demonstrating the direction: **Phase 1 (Preview)** with minimal scope — `bg_spawn` + `bg_list` + `bg_kill` + `bg_wait_line` MCP tools + a tiny PreviewApp that's just a task list (no webview yet). Two days of work, immediately useful, validates the whole "Claude Code primitive in Woom skin" pattern. Webview and `/loop` follow once the primitive's solid.

The rest of the phases are independent enough to be reordered if priorities change.
