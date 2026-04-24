# Forgehold — MVP Plan

**Target:** a working single-player macOS desktop app with Jira +
GitHub + Slack + Claude Code, including local-repo management (GitHub
Desktop-style) and a Rules system. Delivered as a signed & notarized
Universal `.app` bundle (see SPEC.md §13).
**Timeline estimate:** ~10–12 weeks full-time for one engineer.

---

## Milestones

### M0 — Skeleton & macOS bundle scaffolding (week 1)

- [ ] Tauri 2 project + SvelteKit SPA mode + Tailwind.
- [ ] Monorepo structure (apps/, crates/, mcp-servers/, packages/).
- [ ] `forge-core` crate: define types (Object, Action, Workflow, Run, Artifact).
- [ ] `forge-db`: SQLite + migrations (schema from SPEC.md §5.1).
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
produces `Forgehold.app` that opens on macOS 13+, shows a window saying
"Forgehold", and initializes the DB on first run. Unsigned is acceptable
for M0; signing verified but not yet in CI.

### M1 — Auth & Sources (week 2–3)

- [ ] `forge-auth` crate: OAuth PKCE + keychain storage (keyring crate).
- [ ] Custom URI scheme `forge://oauth/callback`.
- [ ] Settings UI: add Jira / GitHub / Slack as a source.
- [ ] Full flow: "Connect" button → browser → callback → token in keychain.
- [ ] Logout: clears the keychain + DB record.

**Deliverable:** the user can sign into all three services; tokens are safe.

### M2 — MCP servers for the three sources (week 3–5)

Minimum tool set for each:

**forge-jira** (Node.js or Rust):
- `jira.list_issues(jql, limit)`
- `jira.get_issue(key)`
- `jira.add_comment(key, body)`
- `jira.transition(key, to_state)`

**forge-github:**
- `github.list_prs(repo, state)`
- `github.list_issues(repo, state)`
- `github.get_pr(repo, number)`
- `github.create_pr(repo, title, body, branch)`
- `github.review_comment(repo, pr, line, body)`

**forge-slack:**
- `slack.list_channels()`
- `slack.recent_messages(channel, limit)`
- `slack.send_message(channel, text, blocks?)`
- `slack.schedule_reminder(user, text, at)`

**Deliverable:** through a dev tool (e.g. the MCP inspector) you can call
every tool and get data.

### M3 — Inbox & Inspector (week 5–6)

- [ ] Periodic fetch: every 60s we pull fresh objects into `object_cache`.
- [ ] Left sidebar with sources and counters.
- [ ] Inbox view: unified list of cards, sorted by `updated_at`.
- [ ] Object card — initial `<ObjectCard>` implementation.
- [ ] Inspector panel: pick a card, see its metadata.
- [ ] Global palette ⌘K with fuzzy search over the cache (fzf-style).

**Deliverable:** a single UI showing the list and details of every object.

### M3.5 — Repositories (week 6–7)

GitHub Desktop-style local repo management. Details in [REPOS.md](REPOS.md).

- [ ] `repository` table + CRUD in `forge-db`.
- [ ] `forge-git` crate (or a forge-runtime module) on top of `git2`:
  - [ ] clone, fetch, pull, status, list branches
  - [ ] create/remove worktree (for Claude runs)
  - [ ] branch create/switch (with an uncommitted-changes warning)
- [ ] Repositories view in the UI: card list, tag filter, clone dialog.
- [ ] GitHub repos picker in the clone dialog (via linked GitHub Source).
- [ ] Repo detail view: branches, worktrees, recent runs.
- [ ] Default clone dir + default editor in settings.
- [ ] "Open in Zed" action (spawns `zed <path>`).

**Deliverable:** you clone a repo through the GitHub picker, see its
status, and open it in Zed with one click.

### M3.75 — Rules system (week 7–8)

Per-scope rules injected into Claude runs. Details in [RULES.md](RULES.md).

- [ ] Parser for `rules.md` (YAML frontmatter + markdown + `@scope`
  directives).
- [ ] `rule_set` table + a chainable resolver (global → repo → folder → run).
- [ ] Rules editor UI: markdown editor + scope selector + live preview.
- [ ] "Effective rules" preview (test-case picker).
- [ ] Policies runtime: branch-template resolver, post-run check
  hookpoints.
- [ ] Inline "add rule for this run" field on the Claude drop zone.
- [ ] Don't touch `CLAUDE.md` — Claude reads it natively.

**Deliverable:** you set up `.forgehold/rules.md` with a branch template
and a post-run check; launching a run creates a branch from the
template and runs typecheck automatically.

### M4 — Drag-and-drop + Claude action (week 8–10)

The heart of the MVP. The biggest risk — Claude execution.

- [ ] `<DropZone>` component with a state machine
  (idle/active/invalid/running).
- [ ] DnD libs: native HTML5 DnD (simple case) or svelte-dnd-action.
- [ ] `forge-runtime` crate: Run state machine, DB persistence.
- [ ] **Claude bridge:** run `claude -p` headless + MCP config pointing
  to our servers.
- [ ] Streaming output from the claude CLI → events in the UI.
- [ ] Repo/worktree selection: automatic or via a chip in the drop zone.
- [ ] Rules injection: prompt prefix from global + repo + folder + run
  rules.
- [ ] Policy application: branch name from template, post-run checks.
- [ ] Output → Artifact (diff + summary + check results in markdown).

**Deliverable:** you drag a Jira ticket onto the Claude zone → a run
starts in a worktree with a name from the template, Claude sees every
rule, post-run checks complete — you see the diff + summary as an
artifact.

### M5 — Output routing (week 10–11)

- [ ] Drop zones for: `Slack → channel`, `Jira → comment`,
  `GitHub → PR draft`, `Reminder`.
- [ ] Each accepts an Object OR an Artifact, renders a preview, and
  sends it.
- [ ] Workflow save: a chain of runs saved as a Workflow definition.
- [ ] Timeline / running strip at the bottom of the screen.

**Deliverable:** the full cycle — Jira ticket → Claude → artifact →
Slack channel — saved as a template.

### M6 — Polish & shippable .app (week 11–12)

- [ ] Animations (dragging, dropping, running).
- [ ] Error states, empty states, loading states.
- [ ] Native macOS notifications (`UNUserNotificationCenter`) for
  completed runs.
- [ ] Onboarding flow (first-run wizard: connect sources).
- [ ] **Shippable macOS `.app` pipeline** (see SPEC.md §13):
  - [ ] All sidecars (MCP servers, `forge-claude`) signed as part of
    the main bundle.
  - [ ] `PrivacyInfo.xcprivacy` with required-reason API declarations.
  - [ ] Hardened runtime entitlements finalized.
  - [ ] `tauri build --target universal-apple-darwin` produces a
    signed Universal binary.
  - [ ] Automated notarization via `notarytool` during CI build.
  - [ ] `xcrun stapler staple` on the `.dmg`.
  - [ ] Custom `.dmg` with background, Applications symlink, proper
    layout.
  - [ ] Gatekeeper check: downloading and opening the `.dmg` on a
    clean Mac works without a warning dialog.
- [ ] GitHub Actions release workflow: tagged push → builds, signs,
  notarizes, publishes `.dmg` + stapled `.app` as release assets.

**Deliverable:** a signed, notarized `Forgehold.dmg` (Universal) that
opens without Gatekeeper warnings on any macOS 13+ Mac. Drag-install
to Applications, launch, full MVP flow works end-to-end.

---

## Team foundation check (must be in place for MVP)

We don't implement sync, but the schema and layering must support:

- [x] `workspace_id` on every entity (even with an implicit personal workspace).
- [x] `shared: bool` flag on Source and Workflow.
- [x] Credentials separate from the DB (keychain), easy to swap for a
  backend token.
- [x] Event log for future sync (can be added in M5 as a no-op append).
- [x] `member` table exists, even if it always has one row.

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
| Claude CLI streaming is finicky                              | Fall back to the embedded Agent SDK (Path A in SPEC.md)       |
| Atlassian OAuth is complex (3LO, rotating refresh)          | Write it once in `forge-auth`, mock-based tests               |
| DnD UX doesn't feel alive                                   | Prototype a week before locking the stack                     |
| SQLite becomes a bottleneck under frequent polling          | WAL mode + batched writes, measure early                      |
| macOS signing / notarization quirks                          | Set up Developer ID + entitlements in M0; don't defer         |
| Sidecar signing (every nested binary must be signed)        | Automate via Tauri bundler; verify with `codesign --verify --deep` |
| Notarization rejections (hardened runtime conflicts)        | Minimal entitlements; review notary log on each CI build       |
| Universal binary size bloat                                 | Lean sidecars (Rust native, not Node.js); measure with `dsymutil` |

---

## How we know MVP is done

The user can complete this sequence in under 2 minutes without docs:

1. First launch: sign into Jira / GitHub / Slack.
2. Click "+ Clone" → pick `acme/forgehold` in the GitHub picker → clone.
3. Open `.forgehold/rules.md`, confirm a branch template and post-run check
   already exist (or create them).
4. See Jira ticket PROJ-1234 in the Inbox.
5. Drag it onto the Claude Code zone (repo is auto-populated).
6. Watch streaming progress; the branch is named from the template; the
   worktree is isolated.
7. Receive diff + summary + ✓ typecheck passed as an artifact.
8. Drag the artifact onto the Slack zone → "#eng-chat".
9. A message is sent with a brief description and links to the ticket
   and worktree.
10. Click "Save as workflow" → name it → next time, one shortcut does
    all of this.

If that path works reliably, the MVP is shipped.
