# Changelog

All notable changes to Woom land here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project
uses [Semantic Versioning](https://semver.org/spec/v2.0.0.html). The
release runbook (how this CHANGELOG feeds `latest-mac.json`) lives in
[`docs/RELEASES.md`](docs/RELEASES.md).

## 0.1.0 — 2026-05-19

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
