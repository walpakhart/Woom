# Woom

> A single-player desktop app for developers. Your Jira tickets,
> GitHub PRs, Sentry issues, code editor, terminal, and Claude agent
> in one window — everything a drag away.

Woom is organised as full-screen **solos** — one surface per source
(Home, Jira, GitHub, Sentry, Claude, Editor, Canvas, Terminal),
switched from a side rail. Editors, canvases, and terminals support
multiple named instances; chats link to editors so the agent always
knows which repo and file you're looking at. Release history lives in
[`CHANGELOG.md`](CHANGELOG.md); the forward-looking backlog in
[`docs/FUTURE_FEATURES.md`](docs/FUTURE_FEATURES.md); the release
runbook in [`docs/RELEASES.md`](docs/RELEASES.md).

**Platform:** macOS 13+ only. Universal `.app` bundle (Apple Silicon
+ Intel), distributed as a DMG. Builds are Developer-ID signed and
notarized when a signing identity is configured; otherwise the DMG
ships ad-hoc signed and first launch needs right-click → Open.
Auto-updates are protected by an ed25519 signature independent of
Apple's chain. Windows and Linux are out of scope.

## Quick start

Prerequisites: Node 20+, pnpm 10+, Rust 1.75+, Xcode Command Line Tools.

```bash
pnpm install                # install JS deps
pnpm --filter @woom/desktop tauri icon apps/desktop/src-tauri/icons/source.png
pnpm dev                    # run Tauri dev (opens the app window)
```

Build a signed Universal `.app`:

```bash
pnpm build:universal        # produces Woom.app + Woom_*.dmg
```

## Repo layout

```
woom/
├── apps/
│   └── desktop/           # Tauri 2 + SvelteKit app
│       ├── src/           # Svelte UI
│       └── src-tauri/     # Rust shell + macOS bundle config + sidecars
│           └── sidecars/  # woom-jira, woom-github, woom-sentry,
│                          # woom-memory, woom-app — MCP servers wired
│                          # into Claude via --mcp-config
├── mockup/                # HTML design prototypes — reference only
├── docs/                  # architecture + design specs
└── pnpm-workspace.yaml    # monorepo root
```

## What's inside

| Feature        | What we support                                                       |
|----------------|-----------------------------------------------------------------------|
| Jira           | Tickets, comments, transitions, worklogs, sprints (live API)          |
| GitHub         | PRs, issues, reviews, comments, merge, draft PR creation, Actions runs |
| Sentry         | Issues, events, breadcrumbs, releases, triage from the inbox          |
| Claude Code    | Headless `claude` runs with MCP sidecars, streaming, model picker, worktree-isolated sessions, quota/context meters |
| Editor         | CodeMirror 6 — file tree, multi-root workspaces, git panel, multi-agent diff review, quick-open, symbol outline, find-in-files, markdown/image preview, docked agent chat |
| Canvas         | Boxes, arrows, mermaid, dagre/grid auto-layout, live source cards     |
| Terminal       | Real `/bin/zsh` PTY instances drivable by agents via MCP              |
| Background tasks | Long-running processes in the Preview pane; exits auto-resume the owning chat |
| SDD            | Spec-Driven Development orchestrator — `/sdd <task>` drafts a spec, plans phases, executes each phase as a chained agent turn |
| Workflows      | `/dw` dynamic workflows — agent-built multi-step plans with subagents and verification |
| Memory         | Long-term SQLite FTS5 store; auto-recall at session start; per-chat distill on delete |
| Rules          | Global ruleset injected into every run via `--append-system-prompt`   |
| Connections    | Personal access tokens (GitHub PAT, Jira / Sentry API tokens) in macOS Keychain with Touch ID gate |
| Auto-updates   | ed25519-signed DMGs, manifest-driven, install-now / install-on-quit, snooze + skip controls in Settings |
| Notifications  | macOS Notification Center on agent run completion                     |

## Documentation

Per-module specs in [`docs/`](docs/):

- [`AGENTS.md`](docs/AGENTS.md) — Claude adapter, sessions, slash commands, MCP toolbox
- [`EDITOR.md`](docs/EDITOR.md) — CodeMirror 6 editor, file tree, git panel, diff review
- [`CANVAS.md`](docs/CANVAS.md) — whiteboard primitives, layouts, live cards, agent integration
- [`WORKBENCH.md`](docs/WORKBENCH.md) — solo layout, drag-drop, snap-resize
- [`COMMAND_PALETTE.md`](docs/COMMAND_PALETTE.md) — fuzzy search, MRU, pinned items
- [`CONNECTIONS.md`](docs/CONNECTIONS.md) — PAT-only auth, Keychain, token rotation, diagnostics
- [`JIRA.md`](docs/JIRA.md) · [`GITHUB.md`](docs/GITHUB.md) · [`SENTRY.md`](docs/SENTRY.md) — per-source columns, filters, mutations
- [`MCP.md`](docs/MCP.md) — bundled sidecars, `--mcp-config` shape, user-server merge
- [`RELEASES.md`](docs/RELEASES.md) — signing, notarization, auto-update runbook
- [`FUTURE_FEATURES.md`](docs/FUTURE_FEATURES.md) — backlog

## Out of scope

- **OAuth** — permanent non-goal across every source. Manual PAT entry is the supported flow; tokens live in macOS Keychain behind a Touch ID gate.
- **Slack, Linear, Teams, Notion, GitLab, Asana, Codex, Aider, Copilot** — placeholders only in the Connect modal; see `docs/FUTURE_FEATURES.md`.
- **Team / cloud sync, multi-user workspaces, real-time collaboration.**
- **Windows / Linux builds.** macOS 13+ only.
- **LSP / IntelliSense in the editor, Sentry performance / transactions, Confluence, mobile.**

## License

TBD. Source-available planned.
