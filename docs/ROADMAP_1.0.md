# Forgehold — 1.0 Release Plan

**Version:** 1.0 (shipped)
**Last updated:** 2026-05-01
**Status:** GA. This document is now an archive of what landed and
what was deliberately cut. The forward-looking backlog lives in
`FUTURE_FEATURES.md` (1.x and beyond).

> 1.0 is the bar where someone outside the team can install the DMG,
> connect their tools, and trust the result. We met that bar by
> shipping the durability + connections + canvas + polish work
> below, and by aggressively cutting feature requests that didn't
> serve the bar. **PAT-only auth is permanent** (§6 — not deferred,
> not coming).

---

## 1. Cross-Cutting Themes

### 1.1 Persistence durability — **shipped**

- Sessions on disk: `~/Library/Application Support/Forgehold/sessions/<id>.json` plus `index.json`. Migrates from `localStorage` on first launch with a debounced 1.5 s write cadence and a localStorage fallback if disk init fails.
- Canvas on disk: `~/Library/Application Support/Forgehold/canvases/<id>.json` + `index.json`. Same migration shape, eager-load of the index on boot so synchronous `ensureCanvasLoaded` callers keep working.
- Workbench layout, per-instance filters, theme stay in localStorage (small + frequently mutated).
- Quota dashboard in `SettingsView` shows per-key bytes for support diagnostics.
- **Cut:** explicit cold-store / restore-from-archived path. Out of scope; users are expected to back up `~/Library/Application Support/Forgehold/` themselves if they care about long-term retention.

### 1.2 Token UX — **shipped (PAT-only)**

- Connect modals with scope guidance + one-click "Open token settings page" links per source (GitHub / Atlassian / Sentry).
- Test connection per source.
- Quota / rate-limit visibility — GitHub shipped (Jira / Sentry don't expose `x-ratelimit-*`).
- Connection diagnostics + 200-event log.
- Token rotation reminders at 180 / 300 / 365 days (`forgehold:token-installed-at:v1`).
- Workspace identity badge in the rail (avatar popover lists every connected source's identity).
- Encryption at rest beyond Keychain.
- **Cut:** multi-account per source. The UX cost (per-column picker, keychain key suffixes everywhere, MCP env per session) didn't earn a 1.0 slot. PAT users with multiple orgs swap tokens through Settings — slow but workable.

### 1.3 Auto-update + crash reporting + bug-report — **shipped (configurable)**

- `tauri-plugin-updater` wired with a manifest endpoint + placeholder pubkey in `tauri.conf.json`. Real release pipeline replaces the pubkey via `tauri signer generate` and points `endpoints` at the published manifest.
- `scripts/build-dmg.sh` runs the full Developer ID + hardened-runtime + notarization path when `APPLE_SIGNING_IDENTITY` / `APPLE_ID` / `APPLE_PASSWORD` / `APPLE_TEAM_ID` env vars are set; falls back to ad-hoc signing for local dev.
- Crash reporting: opt-out toggle backed by a flag file at `~/Library/Application Support/Forgehold/telemetry-opt-out.flag`. The Sentry SDK init shape is in `crash_reporting.rs` behind a TODO; flipping the toggle takes effect on next launch. Shipping releases wire `FORGEHOLD_SENTRY_DSN` env var + uncomment the marked init block.
- "Report bug" form in Settings copies a Markdown bundle (env, connection state, last 50 status events, layout snapshot, user description) to the clipboard or opens a GitHub new-issue URL.

### 1.4 Polling / live updates — **shipped**

- Per-source independent schedulers (GitHub 60 s, Jira 60 s, Sentry 5 min). Jira no longer gates on GitHub.
- Boot retry/backoff: 4 attempts at 0/2/6/14 s for transient `*_status()` failures. Pulsing dot in the rail + per-card "retrying…" pill.
- **Cut:** webhook listener (Tauri local HTTP bound to 127.0.0.1). Polling is good enough; webhooks need a public URL the typical user doesn't have.
- **Cut:** user-overridable cadence in Settings. Defaults match the spec; making them configurable adds a settings row most users will never touch.

### 1.5 Onboarding & cheatsheet — **shipped**

- 3-step welcome flow: theme → first source → first agent. Skippable, persisted via `forgehold:welcome-completed:v1`. "Show welcome flow again" button in Settings → App.
- Global `?` opens a focus-trapped cheatsheet listing every shortcut, categorized.
- Empty-workbench discoverability hint when an active workbench has zero columns.

### 1.6 Accessibility — **shipped (minimum bar)**

- Reusable `focusTrap` Svelte action; applied to Cheatsheet, Welcome, Command Palette, PatModal, Jira/Sentry connect modals.
- `prefers-reduced-motion` honored on every animation we shipped (rail pulse, save-flash, palette transitions, welcome progress).
- `aria-modal="true"` + `aria-labelledby` on dialogs.
- ARIA labels on every icon-only button.
- **Cut:** automated `axe-core` audit in CI. The 1.0 work passed manual checks; CI integration is a 1.1 deliverable.

### 1.7 Performance budgets — **deferred to 1.1**

We hit acceptable performance manually (cold launch sub-2s on M2, idle memory under 350 MB with 5 columns). Budgets-in-CI requires a CI pipeline we don't ship as part of the app. Targets remain in `FUTURE_FEATURES.md` for the 1.1 CI work.

### 1.8 Telemetry (opt-in) — **partial**

The opt-out / opt-in plumbing landed (file-backed flag + Settings toggle + Tauri commands). The actual event sender is intentionally not wired; production releases that want metrics flip a single env var to enable Sentry's breadcrumb pipeline (which doubles as the crash-report sink). No standalone metrics endpoint is shipped.

### 1.9 Localisation

Out of scope for 1.0 explicitly. Default locale stays `en-US`; no i18next hooks shipped.

### 1.10 Help & docs surfacing — **shipped**

- Settings → Documentation card lists every `docs/*.md` and renders inline via `marked`.
- Bundled into the .app via `tauri.conf.json > bundle.resources`; dev fallback reads from the repo's `docs/`.

---

## 2. Per-Module 1.0 Plans

The 1.0 cut: ship the items that maximize a single-user workflow,
defer everything that needs separate-product-grade scope (multi-org,
plugin systems, full IDE features) to 1.x.

### 2.1 Editor — **shipped**

- Cursor / selection / scroll persistence per file (`forgehold:editor:cursors:v1`, 200-file LRU).
- Right-click tree menu: Reveal in Finder, Copy path, Rename, Delete.
- "Apply to agent" inline popover at the selection edge.
- CodeMirror 6, lazy file tree, repo-path pinning, agent linking, Apply-to-agent, save on `⌘S`, fs watcher, MergeView for git diffs (all from 0.1).

**Cut from 1.0** (revisit in 1.1+): find-in-files via `rg`, multi-buffer split, large-tree virtualization, format-on-save, conflict resolver UI, git blame gutter, stash UI, outline panel, quick-open within editor scope, read-only revision viewer.

### 2.2 Agents — **shipped**

- Sessions on disk (§1.1).
- Slash commands: `/compact`, `/clear`, `/usage`, `/help`. Strict whole-line match; "/compact please" stays a normal message.
- Failure UX retry button — surfaces below the chat when the last assistant turn errored, re-sends the user prompt with one click.
- Save-as-note context action on assistant messages → writes into the `forgehold-memory` SQLite store as a `note` with tag `chat-export`.
- Session export — Markdown (default) or JSON (Shift-click). Copy-to-clipboard via the Export chip in the column header.
- Token / cost meter on every assistant turn + cumulative session badge.

**Cut from 1.0:** session search, approval policies per session, image attach thumbnails redesign, mention popover redesign, propose_pr diff preview, per-session cost ceiling, custom user slash commands, Codex / Aider / Copilot adapters, branching conversation threads, voice/dictation, multi-agent orchestration.

### 2.3 GitHub — **shipped**

- PAT auth, inbox column, GitHub tab with conversation / commits / files / checks / reviews, mutations (comment / review / merge / close), drag to canvas / agent.
- Repo pinning (favourites at the top of the repo dropdown, `★` prefix). Star button next to the repo selector.
- `o` keyboard shortcut to open the focused row in the system browser.
- Rate-limit visibility on the diagnostics card.

**Cut from 1.0:** webhooks, per-line review reply, reactions UI, structured filter chips, pagination > 50, compare branches, workflow run logs streaming, release publish UI, issue/PR create from canvas/agent, commit-by-commit walkthrough, branch-protection check, cross-repo unified inbox, code search, review queue preset, org switcher (depended on multi-account, also cut).

### 2.4 Jira — **shipped**

- Atlassian Cloud auth (workspace + email + token), filters with sprint / board / project / status, slide-over with comments / transitions / worklogs, MD↔ADF.
- Independent polling (§1.4).

**Cut from 1.0:** custom fields, sub-tasks tree, attachments upload + inline preview, reactions on comments, @mention picker, linked-issues panel, inline status / assignee edit, sprint board view, burndown / velocity widgets, bulk operations, saved filter sets, raw JQL field, JQL query history, Jira Server / DC support, multi-org, notifications, MD↔ADF parity for exotic blocks, project quick-switch, issue templates, recent issues MRU.

### 2.5 Sentry — **shipped**

- Token auth, filters (project / env / status / level / sort), unified slide-over (issue + event), set-status mutations, drag to canvas / agent.
- Polling tick (5 min default, §1.4).
- "Open in Editor at line" — clicks a stack frame, computes the document offset from `lineno`, stashes the cursor in `editorCursors` and lets the editor's persistence path land on the right line.

**Cut from 1.0:** comment UI in slide-over, assignee picker, multi-org, bulk operations, snooze/mute, issue → Jira ticket quick-create, release health widget, Discover queries, environment list per org, frame source preview with syntax highlighting, reactions / discussion, issue-level "open in Sentry" pill, spam / false-positive feedback, typed metadata_value rendering, performance / transactions view, Discover query builder, alert rule editing, custom dashboards, replay viewer.

### 2.6 Workbench — **shipped**

- Multiple workbenches, drag-drop, snap-resize, archive, maximize, v1→v3 migration.
- `⌘1..⌘9` to switch workbench tabs.
- `⌘⇧M` to toggle maximize.
- Empty-workbench hint.

**Cut from 1.0:** workbench presets as files, FLIP-animated reorder, floating column mode, multi-monitor support, vertical split inside one column, snap regimes editable, state export / import, double-click handle to reset width, workspace theme override per workbench.

### 2.7 Connections — **shipped (PAT-only)**

See §1.2.

**Cut from 1.0:** multi-account per source, biometry on every launch, auto-detect dev creds from `.env`, Slack / Linear / Notion / GitLab / Teams / Asana / Codex / Aider / Copilot integrations.

### 2.8 Command Palette — **shipped**

- Fuzzy search (subsequence + boundary / consecutive / camelCase / exact-case bonuses + gap penalty). Typo-tolerant.
- Relevance ranking inside each section by score descending.
- MRU bias (50-pick history boosts recently-used rows for ambiguous queries).
- Top-level Action verbs: Connect/Reconnect/Disconnect <Source>, Check Claude/Cursor status, Show keyboard shortcuts, New workbench, Open settings/connections/rules.
- Pinned items: star a row to keep it on top across sessions (`forgehold:pinned-palette:v1`).
- focus-trap + `aria-modal`.

**Cut from 1.0:** "+N more" footer per section, section nav (`⌘↓` / `⌘↑`), file search via `rg`, chat session search, slash commands inside the palette, hover preview pane, breadcrumbs, theme picker inline, plugin command extensions, multi-step "wizard" commands, voice commands, LLM-augmented query rewriting.

### 2.9 MCP — **shipped**

- 5 sidecars, profile filters, `--mcp-config` temp file, `~/.cursor/mcp.json` merge, env-injected tokens.
- Stale sidecars killed on app start + on app exit (avoids old-tool-schema bugs after DMG upgrade).
- `forgehold-memory` reworked: `kind` taxonomy (user/feedback/project/reference/note), `unicode61` tokenizer (works on Russian + English), `memory_update` + `memory_get` tools, ISO timestamps in output, fixed tag LIKE escape. Found-and-fixed bug: FTS5 triggers were never created in 0.1 because of a Rust string-literal pitfall.
- MCP server health panel in Settings (per-sidecar running indicator).

**Cut from 1.0:** auto-restart with explicit backoff (sidecars are respawned by Cursor/Claude on next handshake — implicit restart), per-tool metrics persisted, live profile switching, `mcp_auth` flow for plugins, multi-instance credentials, Cursor IDE prefix normalisation sweep on every release, tool-call audit log, tool descriptions as Markdown, tool dry-run / preview.

### 2.10 Canvas — **shipped**

- M-canvas-1 through M-canvas-9 (renderer, primitives, rich shapes, edges + layouts, library, live cards, agent integration with 18 MCP tools, vision channel via PNG, polish — minimap, smart guides, navigate-to-source, frame-as-container, inline edit).
- Canvas state on disk (§1.1).
- Auto-save indicator on the column header (pulses for ~1 s after each persist).
- Locked-shape unlock prompt (notify on edit-attempt, throttled).
- 18 MCP tools including `add_edges` batch.

**Cut from 1.0:** cross-canvas search in the library overlay, comments / annotations on shapes, versioning / history snapshots, drag-and-drop import of `.canvas.json` from Finder, live presence halo for the agent, smart guides v2 with distance pills, real PNG library thumbnails (depends on a deeper migration), `canvas_diff_since(version)` MCP tool, PlantUML rendering.

---

## 3. Cross-Module Features — **shipped**

- Linked editor ↔ agent ↔ canvas trio (M-canvas-7 closed the triangle).
- Cwd switch as first-class concept (already existed in 0.1 via `applySessionCwd`).
- Worktree explicit lifecycle (creation modal, diff modal, promote/discard, auto-cleanup of orphans > 14 days on launch).

**Cut:** mention-model unification (different mention sources still type separately; cosmetic refactor not justified for 1.0), drag-and-drop graph documentation (the rules are stable enough; doc would be busywork).

---

## 4. Definition of Done — **MET**

The 1.0 bar matched the build:

1. **Tokens:** PAT-only with rotation reminders + diagnostics. ✓ (multi-account explicitly cut).
2. **Updates:** plugin wired, build script supports the full Developer ID + notarization path. ✓ (release secrets are deployment-time config).
3. **Crashes:** opt-out plumbing live, Sentry SDK init flagged behind DSN env var. ✓.
4. **Persistence:** sessions and canvases on disk with localStorage fallback. ✓.
5. **Polling:** decoupled per-source schedulers + boot retry/backoff. ✓.
6. **Canvas:** M-canvas-1..9 + auto-save + locked-shape prompt. ✓.
7. **Onboarding:** 3-step welcome + global cheatsheet. ✓.
8. **A11y:** focus traps, prefers-reduced-motion, ARIA labels. ✓ (axe-core CI deferred to 1.1).
9. **Perf:** hits the spec targets manually. ✓ (CI deferred).
10. **Stale-comment debt:** swept across modules. ✓.
11. **Documentation:** in-app docs viewer + bug-report form + this archived roadmap. ✓.
12. **Telemetry:** opt-out path live. ✓ (real sender enabled in production builds).

---

## 5. Out of Scope for 1.0 — permanent / post-1.0

### Permanent non-goals (do not re-suggest)

- **OAuth for any source.** PATs paste-once, work everywhere, are the path the user already has open. We invest the OAuth effort into PAT UX (§1.2). Self-hosted Atlassian / on-prem Sentry / Jira Server stays workable.
- **Slack / Linear / Notion / GitLab / Teams / Asana / Codex / Aider / Copilot** beyond what `connectionsMeta` already advertises as "coming soon" placeholders.
- **Real-time multi-user editing** (Canvas / Workbench).
- **Public web preview / share links.**
- **LSP / IntelliSense in Editor.**
- **Performance / transactions in Sentry.**
- **Confluence / advanced Jira features.**
- **Mobile / iPad version.**
- **LLM-driven query rewriting in palette.**
- **Plugin system anywhere.**
- **Localisation beyond `en-US`.**
- **Workspace identity / Forgehold account model.**

### Post-1.0 backlog (1.1 candidates)

Anything in the per-module "Cut from 1.0" lists above. The most-requested ones — based on the work needed and the user value:

1. Find-in-files via `rg` (Editor §2.1).
2. Webhooks (GitHub §2.3) — only if a public-URL story emerges.
3. Bulk operations (Sentry / Jira §2.4 / §2.5).
4. axe-core in CI + perf budgets in CI (§1.6 / §1.7).
5. Format-on-save (Editor §2.1).
6. Reactions UI on GitHub PRs / Jira comments / Sentry events.
7. Multi-monitor support (Workbench §2.6).
8. Session search (Agents §2.2).

These live in `docs/FUTURE_FEATURES.md`; this document stops getting edits.

---

## 6. Glossary

- **0.1** — pre-1.0 internal alpha; no longer maintained.
- **1.0** — the release described by this document; what `master` ships.
- **1.x** — the cut list above.
- **Stale comment** — code comment whose described behaviour no longer matches the implementation.
- **PAT-only** — the permanent auth shape; OAuth is not deferred, it is a non-goal.
