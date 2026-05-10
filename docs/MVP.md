# Forge — MVP Plan

**Target (revised 2026-04-25):** a working single-player macOS desktop
app with Jira + GitHub + Claude Code + Cursor, including local-repo
management (GitHub Desktop-style) and a global rules ruleset.
Delivered as a signed & notarized Universal `.app` bundle (see SPEC.md §13).

**Reality at 2026-04-25:** core surface is built (workbench, agents,
git, worktrees, editor, sidecars, MCP bridge, biometric unlock,
in-app + native notifications). Remaining work to v0.1: signed/notarized
release pipeline, first-run wizard, polish.

## Cuts from the original plan

- **Slack** — moved to post-v1. The cost (OAuth + sidecar + send/poll
  flow + UI per-source) is not justified for the current single-player
  scope. The drag-drop architecture stays Slack-friendly for later.
- **OAuth PKCE for GitHub & Jira** — replaced with manual PAT entry.
  Each user pastes their own token; no app-wide OAuth client to
  register, rotate, or audit. Tokens live in macOS Keychain with a
  `kSecAccessControlUserPresence` gate (Touch ID on read).
  **Made permanent (2026-04):** OAuth is now an explicit non-goal for
  every source (`docs/ROADMAP_1.0.md §6`). The investment that would
  have gone into OAuth flows goes into PAT UX instead — rotation
  reminders, multi-account, scope guidance, diagnostics + event log.
- **SQLite-backed object cache, event log, repository table, rule_set
  table, workspace/member tables** — none are needed for v0.1. State
  lives in localStorage on the frontend; the only persistent backend
  store is the `woom-memory` MCP sidecar's note DB. Re-add when
  the team layer arrives in v0.3.
- **Per-scope `.forge/rules.md` parser** — replaced with a single
  global ruleset injected via `--append-system-prompt`. Per-folder /
  per-run composition is post-v1.
- **Output drop zones (Slack/Jira-comment/GH-PR-draft) and Workflow
  save** (originally M5) — post-v1. Drag-to-Claude is the v0.1 flow.
- **Monorepo `crates/`/`packages/`/`mcp-servers/` split** (originally
  M0) — collapsed into `apps/desktop/{src,src-tauri,src-tauri/sidecars}`.
  The split costs more than it saves with one app.

## Additions vs the original plan

- **Cursor** as a first-class second AI agent (`cursor-agent -p`,
  resume-by-id, model picker). Same session shape as Claude.
- **Native macOS notifications** on agent run completion via the
  Tauri notification plugin (`UNUserNotificationCenter`).
- **In-app toaster** for transient feedback (success/warning/error).
- **Touch ID gate** at app start (`LAContext` via objc2) plus
  per-keychain-read OS prompts.
- **Built-in editor** (CodeMirror 6) with file tree + git panel +
  diff viewer + AI commit messages. Originally in §Non-Goals; reversed
  2026-04-23.

---

## Milestones

### M0 — Skeleton & macOS bundle scaffolding (week 1) — **DONE**

- [x] Tauri 2 project + SvelteKit SPA mode + Tailwind.
- [x] ~~Monorepo structure (apps/, crates/, mcp-servers/, packages/).~~ Cut: single `apps/desktop` + `apps/desktop/src-tauri/sidecars`.
- [ ] ~~`forge-core` crate~~ — Cut. Types live in `apps/desktop/src/lib/types.ts` and `apps/desktop/src-tauri/src/*.rs`.
- [ ] ~~`forge-db`: SQLite + migrations~~ — Cut. localStorage on frontend, no backend DB in v0.1.
- [ ] Dev loop: `pnpm tauri dev` launches the app, hot reload works.
- [ ] **macOS bundle config** (see SPEC.md §13.2):
  - [ ] Bundle ID `com.forge.desktop`
  - [ ] `Info.plist` with `LSMinimumSystemVersion=13.0`, URL scheme `forge://`
  - [ ] Placeholder `icon.icns` (full size set)
  - [ ] Tauri config for Universal target (`aarch64` + `x86_64`)
  - [ ] Hardened runtime entitlements file
- [ ] **Signing setup:** obtain Developer ID Application certificate,
  verify local signing via `codesign --verify`.
- [ ] CI (GitHub Actions on `macos-latest`): cargo check + svelte-check
  + prettier + a non-signed `tauri build` smoke test.

**Deliverable:** `pnpm tauri build --target universal-apple-darwin`
produces `Forge.app` that opens on macOS 13+, shows a window saying
"Forge", and initializes the DB on first run. Unsigned is acceptable
for M0; signing verified but not yet in CI.

### M1 — Auth & Sources (week 2–3) — **DONE (revised)**

- [x] ~~`forge-auth` crate: OAuth PKCE~~ — Replaced with manual PAT (GitHub PAT, Jira API token).
- [x] Keychain storage via `security-framework` (real `SecItem*` calls, `UserPresence` ACL).
- [x] ~~Custom URI scheme `forge://oauth/callback`~~ — Not needed for PAT mode.
- [x] Settings UI: add GitHub / Jira as a source. (Slack cut to post-v1.)
- [x] Full flow: paste token → validate via `/user` endpoint → store in keychain.
- [x] Logout: clears the keychain entry and refreshes inbox state.

**Deliverable:** the user can sign into Jira and GitHub; tokens are safe behind Touch ID.

### M2 — MCP servers for the sources (week 3–5) — **DONE**

Built as Rust sidecars in `apps/desktop/src-tauri/sidecars/`. Wired into
Claude via `--mcp-config` (see `apps/desktop/src-tauri/src/claude.rs`
`build_mcp_config`).

**woom-jira** — read-only context for Claude runs:
- `mcp__jira__get_issue`, `mcp__jira__search`

**woom-github** — read-only context:
- `mcp__github__get_pr`, `mcp__github__get_pr_diff`, `mcp__github__get_pr_files`,
  `mcp__github__get_pr_comments`, `mcp__github__list_tree`, `mcp__github__get_file`,
  `mcp__github__list_commits`

**woom-memory** — long-term scratch memory (SQLite + FTS5):
- `mcp__memory__save`, `mcp__memory__search`, `mcp__memory__list`, `mcp__memory__delete`

**~~forge-slack~~** — Cut to post-v1.

**Deliverable:** Claude can ask follow-up questions about the dragged ticket / PR without leaving the run, and persist notes between sessions.

### M3 — Inbox & Inspector (week 5–6) — **DONE (revised)**

- [x] On-demand fetch: inbox refresh every 60s when GitHub/Jira are connected.
- [x] Rail (left sidebar) with workbench / repos / tasks / rules / connections views.
- [x] Inbox view (`TasksView`) — unified list of GitHub PRs/issues + Jira tickets.
- [x] Inspector panel (`GithubFocusOverlay`, `JiraDetailPane`).
- [x] Global palette ⌘K with fuzzy search over the inbox.
- [x] ~~`object_cache` table~~ — Cut. Live API only in v0.1.

**Deliverable:** a single UI showing the list and details of every object. ✓

### M3.5 — Repositories (week 6–7) — **DONE**

GitHub Desktop-style local repo management. Details in [REPOS.md](REPOS.md).

- [x] ~~`repository` table~~ — Cut, replaced with localStorage.
- [x] Git ops (shell-out via `git`): clone, fetch, pull, status, branches, log, diff, stage, commit, push, checkout, create-branch.
- [x] Worktrees (`worktree create/remove/list/diff/apply`) under `~/Library/Application Support/Woom/worktrees/<session>`.
- [x] Repositories view (`RepositoriesView`) — card list, repo detail, branches, releases, files.
- [ ] Clone-from-GitHub-picker dialog (manual clone is wired; picker UI is pending).
- [ ] "Open in Zed/VS Code" spawn action (open-in-Editor works; external IDE handoff pending).

**Deliverable:** clone a repo, see its status, edit files, run agents in worktrees. ✓ (clone-via-picker pending)

### M3.75 — Rules system — **DOWNGRADED to single global ruleset**

- [x] Single global ruleset (UI: `RulesView` → textarea), persisted in localStorage.
- [x] Injected into every Claude / Cursor run via `--append-system-prompt`.
- [ ] ~~`.forge/rules.md` parser, scope directives, `rule_set` table~~ — Cut to post-v1.
- [ ] ~~Branch templates / post-run checks~~ — Cut to post-v1.

**Deliverable:** the user can write project-wide rules once and every run picks them up. ✓

### M4 — Drag-and-drop + Claude action — **DONE**

- [x] Drag from Jira/GitHub inbox or Editor file tree onto a Claude/Cursor column.
- [x] Cross-workbench routing (drag onto a header pill → menu shows every instance across all workbenches).
- [x] Claude bridge: `claude -p` headless + `--mcp-config` pointing to bundled sidecars + `--allowedTools` allow-list. Streaming via `stream-json`. Session resume by uuid.
- [x] Cursor bridge: `cursor-agent -p` with chat-id management + model picker.
- [x] Worktree-isolated runs (per-session branch `woom/<sessionId>`); main tree never touched.
- [x] Apply / discard worktree actions exposed in the column header.
- [x] Action cards inline in chat for proposed commits / PRs / bash / cwd switches; user approves before they run.
- [ ] ~~Output → Artifact (diff + summary as a draggable artifact)~~ — Cut to post-v1; the diff modal covers the inspect path.

**Deliverable:** drag a ticket → run starts in worktree → user reviews + approves commit/PR. ✓

### M5 — Output routing — **POSTPONED to post-v1**

The drag-to-output flow (Slack channel / Jira comment / GitHub PR draft) is
deferred. The current path is "drag in → Claude does the work → user clicks
the proposed action". Saveable workflows are also post-v1.

### M6 — Polish & shippable .app (current focus) — **IN PROGRESS**

Done:
- [x] Toast / in-app notification system for transient feedback.
- [x] Native macOS notifications (`tauri-plugin-notification` →
  `UNUserNotificationCenter`) on agent run completion when the app is
  not focused.
- [x] CSP + narrowed `assetProtocol.scope` (was `**`).
- [x] Touch ID gate at app start; per-keychain-read OS prompts via
  `kSecAccessControlUserPresence`.

Remaining:
- [ ] First-run wizard (connect Jira → connect GitHub → pick repo → done).
- [ ] All sidecars + nested binaries signed as part of the main bundle.
- [ ] `PrivacyInfo.xcprivacy` with required-reason API declarations.
- [ ] Hardened runtime entitlements finalized.
- [ ] `tauri build --target universal-apple-darwin` Universal binary.
- [ ] Automated notarization via `notarytool` in CI.
- [ ] `xcrun stapler staple` on the `.dmg`.
- [ ] Custom `.dmg` background + Applications symlink.
- [ ] GitHub Actions release workflow: tag → build → notarize → publish.

**Deliverable:** a signed, notarized `Woom.dmg` (Universal) that
opens on macOS 13+ without Gatekeeper warnings. v0.1 ships when this
checklist is closed.

---

## Team foundation check (must be in place for MVP)

We don't implement sync, but the schema and layering must support:

- [ ] ~~`workspace_id` on every entity~~ — Cut to v0.3 (no DB in v0.1).
- [ ] ~~`shared: bool` flag on Source and Workflow~~ — Cut to v0.3.
- [x] Credentials separate from data (Keychain), easy to swap to a remote token store later.
- [ ] ~~Event log for future sync~~ — Cut to v0.3.
- [ ] ~~`member` table~~ — Cut to v0.3.

When team mode lands, the schema lives in a real backend (Postgres or
remote SQLite) — frontend keeps its localStorage-only fast path for
single-player.

---

## Out of MVP (important not to pull in)

- Custom diff editor (use a simple markdown render + Monaco only if
  there's time left).
- User-supplied custom MCP servers.
- Themes / light mode.
- Linear, Teams, Notion.
- Scheduled / triggered workflows (manual only).
- Advanced workflow builder with conditions / branching.
- Mobile.
- Self-hosted backend.
- Git submodules, LFS, shallow clones (first version handles flat
  vanilla repos).
- Multi-repo Claude runs (one run = one repo).
- Conditional rules (`if touching X then Y`).
- Auto-commit and auto-push without explicit consent (safety > convenience
  in MVP).

All of these are explicitly in the post-MVP roadmap (see SPEC.md §9).

---

## Risks & mitigations

| Risk                                                        | Mitigation                                                    |
|-------------------------------------------------------------|---------------------------------------------------------------|
| Claude CLI streaming is finicky                              | Done — `claude.rs` parses `--output-format stream-json`, drains stderr to a capped buffer, propagates cancellation via `claude_stop` |
| ~~Atlassian OAuth is complex~~                              | N/A — replaced with PAT entry. User holds their own token.    |
| DnD UX doesn't feel alive                                   | Done — pill cross-workbench drop targets, drag autoscroll, in-app toast + native notification on completion |
| ~~SQLite becomes a bottleneck~~                             | N/A — no SQLite in v0.1.                                       |
| macOS signing / notarization quirks                          | Pending — entitlements + notarytool wiring is the M6 critical path |
| Sidecar signing (every nested binary must be signed)        | Pending — to be automated via Tauri bundler; verify with `codesign --verify --deep` |
| Notarization rejections (hardened runtime conflicts)        | Minimal entitlements; review notary log on each CI build       |
| Universal binary size bloat                                 | All sidecars are Rust native, not Node.js; LTO + `panic=abort` already enabled in `Cargo.toml` |
| Frontend god component (`+page.svelte`, ~3000 LOC)          | Drag store, action executor, toaster, and notifications already extracted; remaining extracts (worktree manager, modal registry) are post-v0.1 |

---

## How we know v0.1 is done

The user can complete this sequence in under 90 seconds without docs:

1. First launch: connect Jira (paste API token) and GitHub (paste PAT). Touch ID confirms storage.
2. Open a folder in the Editor column (or click the existing repo card in `Repositories`).
3. See Jira ticket `PROJ-1234` in the Tasks / Inbox view.
4. Drag it onto the Claude column → a worktree is created on a fresh branch.
5. Watch streaming progress; Claude reads `mcp__jira__get_issue` for the ticket context.
6. Approve the proposed commit + PR action cards inline in chat.
7. Tab away — when the run finishes, a macOS Notification Center alert appears.
8. Click `Apply` on the worktree to fast-forward main and reclaim the disk.

If that path works reliably end-to-end on a clean Mac (signed, notarized, no Gatekeeper prompts), v0.1 is shipped.
