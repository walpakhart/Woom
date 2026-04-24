# Forgehold

> A team-scale operating layer for developers. One interface where your
> tickets, PRs, messages, and AI agent live — everything a drag away.

**Status:** pre-alpha, M0 (skeleton in progress).

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
│       └── src-tauri/     # Rust shell + macOS bundle config
├── mockup/                # HTML design prototypes (v1–v4)
├── docs/                  # architecture + design specs
└── pnpm-workspace.yaml    # monorepo root
```

## Documents

- [docs/SPEC.md](docs/SPEC.md) — architecture, stack, data model, auth, team layer, macOS packaging
- [docs/UI.md](docs/UI.md) — visual language, wireframes, interactions
- [docs/REPOS.md](docs/REPOS.md) — local repo management (GitHub Desktop-style)
- [docs/RULES.md](docs/RULES.md) — per-scope rules for Claude runs
- [docs/MVP.md](docs/MVP.md) — exact MVP scope and post-MVP roadmap

## MVP scope

| Feature       | What we support                                          |
|---------------|----------------------------------------------------------|
| Jira          | Tickets, read/post comments, transitions                 |
| GitHub        | PRs, issues, review comments, draft PR from diff         |
| Slack         | Read channels, send messages, threads, reminders         |
| Claude Code   | Headless execution via the Agent SDK + MCP host          |
| Repositories  | Clone, fetch, branches, worktrees (GitHub Desktop-style) |
| Rules         | Per-scope rules (global/repo/folder/run) for Claude runs |
| Connections   | One-click OAuth from within the app, tokens in Keychain  |

The team layer (workspace, members, shared connections) is baked into
the foundation from day one, but cloud sync is post-MVP.

## License

TBD. Likely source-available with commercial licensing for team features.
