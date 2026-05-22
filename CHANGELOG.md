# Changelog

All notable changes to Woom land here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project
uses [Semantic Versioning](https://semver.org/spec/v2.0.0.html). The
release runbook (how this CHANGELOG feeds `latest-mac.json`) lives in
[`docs/RELEASES.md`](docs/RELEASES.md).

## 0.1.3 — 2026-05-22

UX polish + reliability fixes across updater, SDD orchestrator, and the
embedded terminal.

### Fixed

- **Updater "0.1.x skipped" zombie state** — `check_and_emit` now
  auto-clears `skipped_version` whenever it equals the running
  `CARGO_PKG_VERSION` (you can't be "skipping" the version you're
  already on). Pairs with a Settings affordance so the "clear skip &
  re-check" button surfaces whenever the in-memory phase is
  `Skipped`, regardless of what's on disk — escape hatch for any
  ghost state left over from a prior session.
- **SDD `phase_pending_approval` stuck without an Approve button** —
  v2 workspaces gated each phase behind a per-phase approval marker,
  but the SddCard's `isAwaitingApproval` derivation only matched
  `spec_ready` / `plan_ready` / `phase_done`, and `advance()` had no
  branch for the new stage. Cards landed on review with only
  Amend/Stop/Discard and no way to proceed. Card now offers
  `Approve · start phase N` which calls `approveSddPhase(id, phase)`
  and chains into the existing phase-prompt pipeline.
- **TodoWrite trace pill hid the actual plan** — the row showed
  `4 items · 3 done · 1 in progress` and nothing else, so users
  couldn't tell what the agent was about to do. `formatTodos` now
  emits the bullet list into the toolcall envelope's `‹output›`
  slot; clicking the row expands `<details>` and shows every todo
  with a status glyph (`✓ ▸ ○ ✕`).

### Changed

- **Terminal renderer flipped to WebGL** — heavy output (npm install,
  agent tool_use bursts, long build logs) was stalling the chat UI
  because xterm's DOM renderer mutates one DOM node per visible cell
  per frame. Added `@xterm/addon-webgl@0.19.0`; renderer now does a
  single texture upload per frame. Falls back silently to DOM on
  `onContextLoss` (e.g. after sleep/resume) so behaviour stays
  identical when the GPU path is unavailable.

## 0.1.2 — 2026-05-21

Hotfix: 0.1.1 shipped with a WebView that mounted to a black screen on
first launch. Two distinct regressions piled up on the same release —
this version unwinds both and tightens the release pipeline so neither
can reappear.

### Fixed

- **Black-screen-after-keychain on launch** — `pnpm.overrides` in the
  0.1.1 lockfile-hardening commit used open-ended `>=` ranges that
  pulled Vite up to 8.x and Svelte to 5.55.x. Vite 8 + the pinned
  `@sveltejs/vite-plugin-svelte@4` no longer add the `browser`
  resolve-condition by default, so `import { onDestroy } from 'svelte'`
  resolved to `svelte/src/index-server.js` in the client bundle. That
  module throws at mount (`Cannot read properties of undefined (reading
  'r')`) — surfaced as a silent `unhandledrejection`, leaving the
  WebView a blank `#0C1117` canvas. `vite.config.ts` now pins
  `resolve.conditions` to `['browser', 'module', 'import', 'default']`.
- **CSP blocked SvelteKit's bootstrap script** — vite's generated
  `index.html` carries one inline `<script>` that hydrates
  `__sveltekit_*`; our `script-src 'self' 'wasm-unsafe-eval'` CSP
  killed it before Svelte could mount. Added `'unsafe-inline'` to
  `script-src` (acceptable for a desktop app loading only local
  embedded assets) and whitelisted `https://fonts.gstatic.com` in
  `font-src` so Geist / Inter actually load.
- **Settings showed `Woom 0.1.0`** — three hard-coded literals
  (Updates → Current version, App → Build, bug-report payload)
  replaced with a single live `appVersionLabel` derived from
  `@tauri-apps/api/app#getVersion()` so the panel can't drift from
  the actual `Info.plist` value again.

### CI

- **`release.yml`** — added an explicit `pnpm --filter @woom/desktop
  build` step before `tauri build`, plus a guard that fails the job
  early when `apps/desktop/build/index.html` is missing or empty.
  Touch on `apps/desktop/src-tauri/src/lib.rs` invalidates the
  Swatinem cargo cache so `tauri-codegen` re-embeds the freshly built
  frontend instead of reusing a stale `target/` from a prior run.

## 0.1.1 — 2026-05-21

SDD orchestrator overhaul: spec-driven workflow is now a real
data-as-code engine with verifier, git lifecycle, live action log,
structured failure surface, and a self-driving MCP API.

### Added

- **SDD plan-as-data** — workspaces now carry an `is_v2` flag and a
  `phase_pending_approval` gate; plan/phase frontmatter is the
  source of truth, so the orchestrator advances on disk-observed
  `status: done` transitions instead of message-passing.
- **Acceptance verifier** (`sdd_verify` module) — runs typecheck /
  test / lint commands declared in `plan.md` after each phase,
  records `acceptance.json`, and only flips a phase to `done` when
  every check passes (or the user marks it manually). 14 dedicated
  unit tests.
- **Git integration** — auto-init of a per-workspace branch on
  `sdd_start`, post-phase commits with structured messages,
  rollback / recover commands, orphan-phase detection on disk
  rebuild. 13 git-helper tests.
- **Live action log** — `agentStream.ts` publishes tool-use /
  tool-result events; the orchestrator persists them under
  `phases/NN/action-log.jsonl` and replays them in the SddCard so
  you see what the agent is actually doing in real time.
- **Failure surface + diff drawer** — when a phase fails, the card
  shows the structured verifier output (which check, exit code,
  trimmed stderr), an editable retry form with reason, a skip
  form, and a per-file diff drawer powered by `git::phase_diff`.
  `retry_count` and `skip_reason` are persisted in phase
  frontmatter for audit.
- **Self-driving MCP** — 12 new `mcp__app__sdd_*` tools (5
  read-only + 7 mutating) exposed by the `woom-app` sidecar:
  `sdd_get`, `_list_phases`, `_get_phase`, `_get_action_log`,
  `_get_results`, `_advance_phase`, `_retry_phase`, `_skip_phase`,
  `_pause`, `_resume`, `_log_phase_done`, `_log_action`. Every
  mutation requires a `reason ≥ 5 chars`. `approve_spec` /
  `approve_plan` are intentionally absent — user gates stay user
  gates.
- **Audit log** — append-only `<workspace>/audit-log.jsonl` records
  every mutation (agent / user / system) with timestamp, action,
  optional phase, reason, and before/after snapshots. SddCard
  header shows `· N audit · view` chip; overlay supports source
  filter, expanded before/after diffs, and copy-as-JSONL export.
- **Agent context inject** — `agentContext.ts` advertises
  `linked_to_sdd_phase=<wsid>:<phase>` on the linked-session row
  and embeds an SDD-orchestrator discipline block teaching the
  agent how (and when not) to call the new tools.

### Changed

- **SDD prompts** (`phase.md`, `plan.md`) rewritten to use the new
  MCP API instead of recommending manual frontmatter edits. Legacy
  frontmatter-edit path still works as a fallback for old
  workspaces.
- **`SddWorkspace` JSON shape** extends with `is_v2`,
  `recovery_state`, `audit-log.jsonl` path, structured failure
  fields. Frontend types in `sdd.svelte.ts` mirror the new shape.



First public release.

### Added

- **Solo-mode workspace** — full-screen surfaces for Home, Jira,
  GitHub, Sentry, Claude, Cursor, Editor, Canvas, Terminal. Rail
  switcher with `⌘0…⌘8`.
- **Agents** — Claude Code and Cursor Agent as Tauri sidecars with
  streaming stdout, MCP toolbox (jira / github / sentry / memory /
  app / canvas), per-session tool profiles, `--resume` continuity,
  worktree-isolated runs.
- **Approval cards** for `propose_commit` / `propose_pr` /
  `propose_bash` / `propose_switch_cwd`. Action card has an editable
  preview, runs the action only on Approve.
- **Editor** — CodeMirror 6 with file tree, git panel, multi-agent
  diff review (`⇧⌘R`, j/k navigation, a/r/e actions), quick-open
  (`⌘P`), symbol outline (`⇧⌘O`), find-in-files (`⇧⌘F`), markdown
  preview (`⇧⌘V`), image preview, pending-edits banner.
- **Canvas** with rects / ellipses / arrows / mermaid / live source
  cards, dagre / grid / row / column auto-layout, MCP control.
- **Terminal** — real `/bin/zsh` PTY instances drivable by agents via
  the `mcp__app__terminal_*` toolbox.
- **SDD (Spec-Driven Development) orchestrator** — `/sdd <task>`
  drafts a spec, plans phases, executes each phase as a chained
  agent turn. Workspaces persist under
  `~/Library/Application Support/com.woom.desktop/sdd-workspaces/`
  so runs survive across sessions.
- **Long-term memory** — SQLite FTS5 store with kind taxonomy
  (`user` / `feedback` / `project` / `reference` / `note`),
  auto-recall at session start, per-chat distill on delete,
  `Settings → Memory` browser.
- **macOS auto-updates** — ed25519-signed updater payload, manifest
  at `releases/latest/download/latest-mac.json`, Settings card with
  Check / Install now / Install on quit / Snooze / Skip controls.
  Defense-in-depth sha256 in the manifest.
- **Crash recovery** — mid-turn force-quit auto-injects a recap on
  the next send and rotates the CLI uuid. Amber banner surfaces
  the recovery in the chat.
- **Hooks** — `~/Library/Application Support/Woom/hooks.json`
  binds shell scripts to UserPromptSubmit / Stop / SessionStart.
- **Skills + slash commands** under `~/.claude/skills/` and
  `<repo>/.claude/skills/`, with `$ARGUMENTS` and inline
  `` !`<cmd>` `` shell injection.
- **CLAUDE.md auto-load** walked from repo root + user-global,
  with `@path` includes and HTML comment stripping.
- **Welcome / Cheatsheet** overlays — `⇧⌘?` for the tour, `?` for
  the keyboard reference (this CHANGELOG-style release surfaces in
  Settings → Updates).
- **Preview pane** for dev servers / watchers / test loops via
  `/preview <cmd>`, with `bg_wait_line` MCP for line-reactive flows
  and an embedded webview for detected `http://localhost:PORT`
  URLs.

### Platform

- macOS 13+ only. Universal `.app` bundle (Apple Silicon + Intel),
  ad-hoc signed. First launch may show a Gatekeeper warning until
  the user right-clicks → Open or removes the quarantine flag
  (`xattr -dr com.apple.quarantine /Applications/Woom.app`). Apple
  Developer ID signing + notarization can be enabled later by
  populating the Apple secrets in CI — workflow is already wired.

### Notes

- The previous internal `1.0.0` tag in development manifests has
  been retired — this is the actual first public release and the
  trust root for the auto-updater begins here.
