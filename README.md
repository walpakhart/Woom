# Forgehold

> A single-player desktop workbench for developers. Your Jira tickets,
> GitHub PRs, code editor, and Claude/Cursor agents in one window —
> everything a drag away.

**Status:** alpha. Workbench, agents (Claude + Cursor), and editor work
end-to-end; team / cloud sync features are explicitly post-v1.

**Platform:** macOS 13+ only. Ships as a signed & notarized `.app`
bundle (Universal binary: Apple Silicon + Intel). Windows and Linux
are post-v1.0.

## Quick start

Prerequisites: Node 20+, pnpm 10+, Rust 1.75+, Xcode Command Line Tools.

```bash
pnpm install                # install JS deps
pnpm --filter @forgehold/desktop tauri icon apps/desktop/src-tauri/icons/source.png
pnpm dev                    # run Tauri dev (opens the app window)
```

Build a signed Universal `.app`:

```bash
pnpm build:universal        # produces Forgehold.app + Forgehold_*.dmg
```

## Repo layout

```
forgehold/
├── apps/
│   └── desktop/           # Tauri 2 + SvelteKit app
│       ├── src/           # Svelte UI
│       └── src-tauri/     # Rust shell + macOS bundle config + sidecars
│           └── sidecars/  # forgehold-jira, forgehold-github, forgehold-memory
│                          # (MCP servers wired into Claude via --mcp-config)
├── mockup/                # HTML design prototypes (v1–v4) — reference only
├── docs/                  # architecture + design specs
└── pnpm-workspace.yaml    # monorepo root
```

## Documents

- [docs/SPEC.md](docs/SPEC.md) — architecture, stack, auth model, macOS packaging
- [docs/UI.md](docs/UI.md) — visual language, wireframes, interactions
- [docs/REPOS.md](docs/REPOS.md) — local repo management (GitHub Desktop-style)
- [docs/RULES.md](docs/RULES.md) — rules for Claude runs (current scope: single global ruleset)
- [docs/MVP.md](docs/MVP.md) — milestones, scope cuts, post-MVP roadmap

## v0.1 scope (single-player)

| Feature       | What we support                                                       |
|---------------|-----------------------------------------------------------------------|
| Jira          | Tickets, comments, transitions, worklogs (live API; no offline cache) |
| GitHub        | PRs, issues, reviews, comments, merge, draft PR creation              |
| Claude Code   | Headless `claude -p` with MCP sidecars + streaming + worktree-isolated runs |
| Cursor        | Headless `cursor-agent` with session resume + model picker            |
| Repositories  | Clone, fetch, branches, worktrees, commit, push, PR creation          |
| Editor        | Built-in CodeMirror 6 editor with file tree, git panel, diff viewer   |
| Rules         | Single global ruleset injected into every run via `--append-system-prompt` |
| Connections   | Personal access tokens (GitHub PAT, Jira API token), stored in macOS Keychain with Touch ID gate |
| Notifications | macOS Notification Center on Claude/Cursor run completion             |

## Cut from v0.1 (originally in spec)

- **Slack source** — not implemented; revisit post-v1 once core flow is solid.
- **OAuth PKCE** — manual PAT is a deliberate choice; each user enters their own token, no app-wide OAuth client to register or rotate.
- **Per-scope `.forge/rules.md` parser** — current model is one global ruleset; per-scope composition is post-v1.
- **SQLite-backed object cache & event log** — everything is live API + localStorage in v0.1.
- **Workspace / member / shared sources** — team layer is post-v0.3.
- **Output drop zones** (Slack/Jira-comment/GitHub-PR-draft) and saveable workflows — post-v1.
- **Linear, Teams, Notion** — never in v0.1.

## License

TBD. Source-available planned.
