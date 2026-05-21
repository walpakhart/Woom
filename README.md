# Woom

> A single-player desktop app for developers. Your Jira tickets,
> GitHub PRs, code editor, and Claude/Cursor agents in one window —
> everything a drag away.

**Status:** 0.1.2 — first public release. Solos (Home, Jira, GitHub,
Sentry, Claude, Cursor, Editor, Canvas, Terminal), agents (Claude +
Cursor with MCP toolbox), built-in editor with multi-agent diff
review, canvas, terminal, SDD (Spec-Driven Development) orchestrator,
signed macOS auto-updates, and persistent memory all ship. Team /
cloud sync are post-0.1. Forward-looking backlog lives in
[`docs/FUTURE_FEATURES.md`](docs/FUTURE_FEATURES.md); release runbook
in [`docs/RELEASES.md`](docs/RELEASES.md); per-version notes in
[`CHANGELOG.md`](CHANGELOG.md).

**Platform:** macOS 13+ only. Universal `.app` bundle (Apple Silicon
+ Intel). Distributed as an ad-hoc-signed DMG — on first launch
right-click the app and choose Open (Gatekeeper warning is expected;
the build is not Apple-notarized). Auto-updates are protected by an
ed25519 signature independent of Apple's chain. Windows and Linux are
out of scope.

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
│           └── sidecars/  # woom-jira, woom-github, woom-memory
│                          # (MCP servers wired into Claude via --mcp-config)
├── mockup/                # HTML design prototypes (v1–v4) — reference only
├── docs/                  # architecture + design specs
└── pnpm-workspace.yaml    # monorepo root
```

## Documentation

Per-module specs in [`docs/`](docs/):

- [`AGENTS.md`](docs/AGENTS.md) — Claude / Cursor adapter, sessions, slash commands, MCP toolbox
- [`EDITOR.md`](docs/EDITOR.md) — CodeMirror 6 editor, file tree, git panel, diff review
- [`CANVAS.md`](docs/CANVAS.md) — whiteboard primitives, layouts, live cards, agent integration
- [`WORKBENCH.md`](docs/WORKBENCH.md) — solo layout, drag-drop, snap-resize
- [`COMMAND_PALETTE.md`](docs/COMMAND_PALETTE.md) — fuzzy search, MRU, pinned items
- [`CONNECTIONS.md`](docs/CONNECTIONS.md) — PAT-only auth, Keychain, token rotation, diagnostics
- [`JIRA.md`](docs/JIRA.md) · [`GITHUB.md`](docs/GITHUB.md) · [`SENTRY.md`](docs/SENTRY.md) — per-source columns, filters, mutations
- [`MCP.md`](docs/MCP.md) — bundled sidecars, `--mcp-config` shape, user-server merge
- [`RELEASES.md`](docs/RELEASES.md) — signing, notarization, auto-update runbook
- [`FUTURE_FEATURES.md`](docs/FUTURE_FEATURES.md) — post-0.1 backlog

## 0.1.0 scope

| Feature        | What we support                                                       |
|----------------|-----------------------------------------------------------------------|
| Jira           | Tickets, comments, transitions, worklogs, sprints (live API)          |
| GitHub        | PRs, issues, reviews, comments, merge, draft PR creation              |
| Sentry         | Issues, events, breadcrumbs, releases, triage from the inbox          |
| Claude Code    | Headless `claude -p` with MCP sidecars + streaming + worktree-isolated runs |
| Cursor         | Headless `cursor-agent` with session resume + model picker            |
| Editor         | CodeMirror 6 — file tree, git panel, multi-agent diff review, quick-open, symbol outline, find-in-files, markdown/image preview |
| Canvas         | Boxes, arrows, mermaid, dagre/grid auto-layout, live source cards     |
| Terminal       | Real `/bin/zsh` PTY instances drivable by agents via MCP              |
| SDD            | Spec-Driven Development orchestrator — `/sdd <task>` drafts a spec, plans phases, executes each phase as a chained agent turn |
| Memory         | Long-term SQLite FTS5 store; auto-recall at session start; per-chat distill on delete |
| Rules          | Global ruleset injected into every run via `--append-system-prompt`   |
| Connections    | Personal access tokens (GitHub PAT, Jira / Sentry API tokens) in macOS Keychain with Touch ID gate |
| Auto-updates   | ed25519-signed DMGs, manifest-driven, install-now / install-on-quit, snooze + skip controls in Settings |
| Notifications  | macOS Notification Center on Claude / Cursor run completion           |

## Out of scope for 0.1.0

- **OAuth** — permanent non-goal across every source. Manual PAT entry is the supported flow; tokens live in macOS Keychain behind a Touch ID gate.
- **Slack, Linear, Teams, Notion, GitLab, Asana, Codex, Aider, Copilot** — placeholders only in the Connect modal; see `docs/FUTURE_FEATURES.md`.
- **Team / cloud sync, multi-user workspaces, real-time collaboration.**
- **Windows / Linux builds.** macOS 13+ only.
- **LSP / IntelliSense in the editor, Sentry performance / transactions, Confluence, mobile.**

## License

TBD. Source-available planned.
