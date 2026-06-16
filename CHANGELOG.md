# Changelog

All notable changes to Woom land here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project
uses [Semantic Versioning](https://semver.org/spec/v2.0.0.html). The
release runbook (how this CHANGELOG feeds `latest-mac.json`) lives in
[`docs/RELEASES.md`](docs/RELEASES.md).

## Unreleased

## 0.4.7 — 2026-06-16

Bumps version 0.4.6 → 0.4.7 across `apps/desktop/package.json`,
`apps/desktop/src-tauri/Cargo.toml`, `apps/desktop/src-tauri/tauri.conf.json`,
`apps/desktop/src-tauri/Cargo.lock`.

### Fixed

- **DW fan-out no longer fails with $0 / empty results on fresh or
  non-repo folders** — subagents run in `git worktree add … HEAD`, which
  dies on an unborn HEAD (repo with `.git` but zero commits) or a plain
  folder (no `.git`). A new `ensure_worktree_base` `git init`s the folder
  and lays down an empty initial commit before the worktree add.
- **`/dw` and `/sdd` slash commands fire on multi-line prompts** — the
  parser regex lacked the `s` flag, so any newline in the argument made
  the command fall through to the raw CLI ("Unknown command").
- **DW build-brief no longer leaks into the chat** — it now goes through
  the `opts.prompt` programmatic channel instead of `updateSession({ input })`,
  which also stops it clobbering the user's composer draft.

## 0.4.6 — 2026-06-12

Bumps version 0.4.5 → 0.4.6 across `apps/desktop/package.json`,
`apps/desktop/src-tauri/Cargo.toml`, `apps/desktop/src-tauri/tauri.conf.json`,
`apps/desktop/src-tauri/Cargo.lock`.

### Fixed

- **Inline diff hunks anchor at their real position** — Edit/MultiEdit
  snippets used to paint at the top of the file regardless of where the
  change landed; they now locate the snippet in the live buffer first.
  Unresolvable (drifted) snippets render nothing instead of garbage.
- **Unlinking a chat from the editor survives restarts** — the unlink
  bypassed the persist scheduler and silently reverted.
- **Rail active-button halo no longer sticks mid-animation** — the
  offset "shadow square" after expanding instance stacks; geometry now
  re-settles after transitions finish.
- **Editor scrollbar thumb visible on the dark theme**; Review sidebar
  rows get icon actions that fit the narrow pane and higher-contrast
  +/− stats.

### Docs

- README rewritten version-free (history lives in CHANGELOG); Cursor
  integration purged from all module specs.

## 0.4.5 — 2026-06-12

Bumps version 0.4.4 → 0.4.5 across `apps/desktop/package.json`,
`apps/desktop/src-tauri/Cargo.toml`, `apps/desktop/src-tauri/tauri.conf.json`,
`apps/desktop/src-tauri/Cargo.lock`.

### Added

- **Background tasks wake their agent** — `bg_spawn` tasks tagged with
  the owning session now auto-resume the chat with the log tail when
  they exit, same as Claude CLI background tasks. No more silent
  finishes in the Preview pane.

### Fixed

- **Editor restores the last-focused tab** after leaving and returning
  to the editor solo — the active-tab memory was being erased on mount
  before the restore could read it.
- **Chat file links actually open the editor** — clicking a file path
  on an edit card or in message markdown opens it in the session's
  linked editor; relative paths (`apps/desktop/src/...`) resolve
  against the session repo, `+page.svelte`-style names are clickable,
  and dead/truncated paths show a toast instead of an editor error tab.
- **Edit-card paths render repo-relative** with the full path in the
  tooltip; expanded diffs get always-visible scrollbars.

## 0.4.4 — 2026-06-11

Bumps version 0.4.3 → 0.4.4 across `apps/desktop/package.json`,
`apps/desktop/src-tauri/Cargo.toml`, `apps/desktop/src-tauri/tauri.conf.json`,
`apps/desktop/src-tauri/Cargo.lock`.

### Added

- **Claude Fable 5 · 1M model variant** in the Composer picker, with a
  correct 200K/1M context ring and a quota "n/a" fallback when the CLI
  doesn't report limits.
- **Edit cards: file path opens in editor** — clicking the file name on
  a chat edit card jumps to that file; expanded diffs scroll
  horizontally instead of clipping long lines.

### Fixed

- **Memory search recall** — queries now match word prefixes (inflected
  Russian works: `нудж` finds `нуджа`), one stray word no longer zeroes
  out results (AND pass with OR fallback), and ranking adds recency +
  kind penalties so pasted-log noise stops outranking curated entries.
  Applies to both the MCP sidecar and the first-turn auto-recall.
- **Auto-sent prompts no longer clobber the composer draft.**

### Removed

- **Cursor agent integration** — the Cursor solo and its session
  plumbing are gone; Claude is the single agent surface.

## 0.3.5 — 2026-06-09

Bumps version 0.3.4 → 0.3.5 across `apps/desktop/package.json`,
`apps/desktop/src-tauri/Cargo.toml`, `apps/desktop/src-tauri/tauri.conf.json`,
`apps/desktop/src-tauri/Cargo.lock`.

### Added

- **Claude Fable 5 + Mythos 5 models** — new top tier above Opus 4.8,
  surfaced in the Composer model picker. Both bill at $10/$50 per 1M
  (cache write/read at the standard 1.25× / 0.1× of base) and carry a
  1M-token context window. Mythos 5 is the safeguard-lifted variant
  (Project Glasswing, gated) — listed since the CLI resolves the id for
  authorized accounts. (`usage.ts`, `composerHelpers.ts`.)

## 0.3.4 — 2026-06-04

Bumps version 0.3.3 → 0.3.4 across `apps/desktop/package.json`,
`apps/desktop/src-tauri/Cargo.toml`, `apps/desktop/src-tauri/tauri.conf.json`,
`apps/desktop/src-tauri/Cargo.lock`.

### Fixed

- **Composer attach button works again — files _and_ folders** — the
  paperclip had no click handler (dead button). It now opens a small
  Files… / Folder… menu backed by the native picker
  (`@tauri-apps/plugin-dialog`); picked paths route through the same
  `attachPathsToSession` pipeline as drag-drop. `attachPathsToSession`
  gained an `asDir` flag so attached folders carry `isDir` + a
  trailing-slash `@`-mention token (previously every path was forced
  to `isDir: false`). (`Composer.svelte`, `sessions.svelte.ts`.)

### Known limitations

- **Dragging files/folders from Finder still doesn't attach** (images
  do, because they're read as bytes). External OS drops need the
  native Tauri drag-drop event — `dragDropEnabled` is currently off so
  the WebView never sees real paths. Deferred to a focused pass that
  can verify the global drag-drop flip live.

## 0.3.3 — 2026-06-04

Bumps version 0.3.2 → 0.3.3 across `apps/desktop/package.json`,
`apps/desktop/src-tauri/Cargo.toml`, `apps/desktop/src-tauri/tauri.conf.json`,
`apps/desktop/src-tauri/Cargo.lock`.

### Fixed

- **Multi-root editors surface every open folder** — 0.3.2 gave the
  agent one row per editor *instance*, but still showed only a single
  `repo_path` per instance, so an editor with multiple roots open (a
  multi-root workspace) hid every root but the first. The layout
  snapshot now lists all of an instance's roots as `repo_roots=[a, b]`
  (and keeps `repo_path=a` for the common single-root case), via
  `editorRoots()`. (`agentContext.ts`.)

## 0.3.2 — 2026-06-04

Bumps version 0.3.1 → 0.3.2 across `apps/desktop/package.json`,
`apps/desktop/src-tauri/Cargo.toml`, `apps/desktop/src-tauri/tauri.conf.json`,
`apps/desktop/src-tauri/Cargo.lock`.

Multi-editor context fixes, a force-send escape hatch for the quota
guard, and chat archiving.

### Added

- **Send anyway on the quota pause** — the 5H/7D ≥95% pause modal now
  offers a third action, «Отправить всё равно», alongside wait/cancel.
  Pre-send (`sendClaudeMessage`) falls straight through to the send;
  the mid-turn auto-continuation guard (`agentTurn`) clears the
  interrupted/awaiting marker and re-enters with the quota check
  bypassed. The turn may get cut off if the bucket crosses 100%, but
  that's now the user's explicit call. (`QuotaPauseModal.svelte`,
  `modals.svelte.ts`, `sendClaudeMessage.ts`, `agentTurn.ts`.)
- **Chat archive** — deleting a chat now moves it to an Archive
  instead of hard-deleting. The sidebar "Delete chat" / X soft-deletes
  (reversible, no confirm; still auto-distills a memory snapshot); a
  collapsible **Archived** section lists archived chats with Restore
  and Delete-forever. New `archived` / `archivedAt` on `ClaudeSession`
  (persisted), plus `restoreClaudeSession` / `purgeClaudeSession`.
  (`types.ts`, `sessions.svelte.ts`, `sessions_serialize.ts`,
  `SessionsSidebar.svelte`.)

### Fixed

- **Agent now sees every editor, not just the primary** — the per-turn
  layout snapshot emitted one row per kind via the singleton id, so
  with multiple editor (or canvas / terminal) instances open the agent
  only saw the primary column and its repo — it read the wrong folder
  when its linked session lived in a secondary editor. The snapshot now
  emits one row per instance with each instance's own `repo_path` /
  `open_file` / `linked_agents`, and a session's `linked_to_editor`
  names the specific instance (name + id) instead of the bare kind.
  (`agentContext.ts`.)
- **ReviewPane file groups no longer overlap** — the review sidebar's
  column-flex list shrank each file card below its content height
  instead of scrolling, piling rows on top of each other. File cards
  are pinned to `flex: 0 0 auto` so the list overflows and scrolls.
  (`ReviewPane.svelte`.)

## 0.3.1 — 2026-06-02

Bumps version 0.3.0 → 0.3.1 across `apps/desktop/package.json`,
`apps/desktop/src-tauri/Cargo.toml`, `apps/desktop/src-tauri/tauri.conf.json`,
`apps/desktop/src-tauri/Cargo.lock`.

**ReviewPane v2** — the AGENT EDITS panel is now a compact navigator and the
editor is the single diff surface.

### Changed

- **Compact ReviewPane** — dropped the panel's vendored inline-diff (LCS) and
  the fullscreen overlay. Each edit is one quiet line (status dot · tag ·
  source chip · `+N −M`) under collapsible per-file cards; `+N −M` now derives
  from the shared `computeHunks` engine via a new `reviewStats.ts` (+12 unit
  tests). (`ReviewPane.svelte` 919→~620 lines.)
- **Review sidebar legibility** — file headers show a bold filename + muted
  directory (filename keeps priority, directory truncates first), an edit-count
  badge, and summed `+N −M`; rows nest under their file card with hairline
  separators.

### Added

- **Select-to-focus an edit** — selecting a row (click or `j`/`k`) opens the
  edit's file and scrolls to + highlights *that edit's* hunks in the editor
  overlay (`cm-inline-hunk--focus` accent rail), via a new `selectedEditKey`
  wired ReviewPane → EditorView → Editor.

### Fixed

- **Multi-hunk reject no longer drifts** — rejecting one hunk of a
  multi-hunk agent edit kept the *other* hunks' line numbers frozen at
  their original positions, so a second reject (or the inline overlay)
  landed on the wrong lines once an earlier reject changed the buffer's
  line count. The overlay now recomputes from the **live buffer** after
  every accept/reject (single-edit case), and hunk ids are anchored on the
  old side so they stay stable across recomputes — sequential rejects in
  any order now round-trip exactly. The per-edit roster that drives the
  `kept`/`reverted` status flip is still taken from the frozen edit, so
  the all-rejected tally stays correct as hunks drop out of the live diff.
  (`inlineHunks.ts`, `Editor.svelte`; +4 unit tests.) This was the 0.3.0
  multi-hunk known limitation.
- **Stacked-edit overlay collisions** — hunk ids are now namespaced per edit
  (`sessionId:toolId#oldId`); previously old-anchored ids collided when several
  edits stacked on one file, cross-assigning owners and piling overlapping
  overlays. The overlay also renders only the selected edit's hunks.
  (`Editor.svelte`)
- **Scroll-to-hunk clobbered on open** — `load()`'s saved-scroll restore (rAF)
  overrode the jump-to-hunk in the same frame, so the editor never moved to the
  selected edit; the jump now defers past the restore. (`Editor.svelte`)

## 0.3.0 — 2026-06-02

Bumps version 0.2.27 → 0.3.0 across `apps/desktop/package.json`,
`apps/desktop/src-tauri/Cargo.toml`, `apps/desktop/src-tauri/tauri.conf.json`,
`apps/desktop/src-tauri/Cargo.lock`. CHANGELOG entry added for 0.3.0.

**Inline agentic editing** — the 0.3.0 anchor. The built-in editor
becomes the place where agent edits land *and* get reviewed, so the
separate Cursor solo is no longer needed for tight inline accept/reject.

### Added

- **Live buffer sync** — when the linked agent edits the file you have
  open, the buffer now reloads from disk live (cursor + scroll preserved
  via the existing `recordCursor` store) instead of going stale until a
  manual reopen. Unsaved manual edits are never clobbered: a dirty buffer
  sets an `agentEditPendingReload` flag and skips the auto-reload, and
  the editor's own autosave echo is deduped against `savedContents`
  (`Editor.svelte`).
- **Inline diff overlay** — pending agent edits for the open file render
  directly in the buffer: added/modified lines get a green line wash,
  removed lines a struck-through ghost block widget anchored where they
  used to be. Built on a new line-level LCS diff (`computeHunks`) and a
  CodeMirror decoration field, sibling to the git change-bar and hand-
  rolled like `DiffView` (no `@codemirror/merge`). New
  `inlineHunks.ts` + 15 unit tests; wired through `EditorView` from the
  same source as the file-level review banner.
- **Hunk-level accept / reject** — resolve each hunk from the keyboard:
  **Tab** accepts (content already on disk, overlay clears), **Esc**
  rejects (that hunk's lines splice back to the pre-edit text and save to
  disk). A `Prec.highest` keymap scopes the keys to the hunk under the
  caret and falls through to normal indent / close-search otherwise.
  Resolutions persist across recomputes and agent turns (non-blocking);
  once an edit's hunks are all resolved its `EditEvent` flips to `kept`
  or `reverted`, keeping the chat-side review in sync.

### Known limitations

- Rejecting one hunk of a *multi-hunk* edit can shift the remaining
  hunks' line numbers (single-hunk and accept-all are exact) — deferred
  to a 0.3.x pass alongside cross-hunk navigation and retiring the Cursor
  solo. The `agentEditPendingReload` dirty-buffer guard is wired but not
  yet surfaced in the UI.

## 0.2.27 — 2026-06-02

Bumps version 0.2.26 → 0.2.27 across `apps/desktop/package.json`,
`apps/desktop/src-tauri/Cargo.toml`, `apps/desktop/src-tauri/tauri.conf.json`,
`apps/desktop/src-tauri/Cargo.lock`. CHANGELOG entry added for 0.2.27.

Claude cost & prompt-cache optimization pass. Four fixes that cut
per-turn token spend with zero quality change: stop re-writing the
prompt cache every turn, gate dead SDD instruction text, and route
background/utility calls off the Max Opus default.

### Changed

- **Volatile context moved out of `--append-system-prompt`** — the
  per-turn layout snapshot, canvas summary, and `cwdSwitchRecap` used
  to live at the tail of the system prompt, which changed the cached
  prefix every turn and forced a full cache *write* (re-billing the
  entire conversation history each turn). They now ride in the
  user-turn message instead, leaving the system prefix byte-stable so
  the CLI serves cache *reads*. Largest recurring win — grows with
  session length (`agentContext.ts`, `+page.svelte`, `agentTurn.ts`,
  `sendClaudeMessage.ts`).
- **SDD instruction blocks gated behind active phase** — the SDD
  live-log + orchestrator blocks (~429 tok/turn) were emitted on every
  turn but are only relevant inside an SDD-managed phase. Now wrapped
  in `if (callingSdd)`, so the ~90% of sessions with no SDD workflow
  stop paying for them (`agentContext.ts`).
- **Commit-message generation → Haiku** — was issued with no `--model`,
  riding the Max Opus default for a single-line subject. Pinned to
  `claude-haiku-4-5-20251001` (`claude.rs`). ~84% cheaper per commit
  and frees the 5h Opus quota.
- **Memory distill forced off fast/thinking** — `/remember`'s background
  distill pass inherited `session.fastMode` + thinking effort, so a user
  who enabled FAST for interactive turns silently paid 2× on an
  unwatched background call. Now hardcoded `fastMode: false`,
  `thinkingEffort: null` (`distillMemory.ts`).

## 0.1.4 — 2026-05-25

Bumps version 0.1.3 → 0.1.4 across `package.json`, `apps/desktop/package.json`,
`apps/desktop/src-tauri/Cargo.toml`, `apps/desktop/src-tauri/tauri.conf.json`,
`apps/desktop/src-tauri/Cargo.lock`. CHANGELOG entry added for 0.1.4.

Bundles the SDD three-call execution mode (post-0.1.3 unreleased work)
with a fresh batch of stuck-state escapes (manual Continue, Accept
anyway), a Claude-CLI background-task watcher, two big component
splits (Settings, Rail), and a streaming/disk perf pass.

### Added

- **SDD three-call execution mode** — every phase optionally runs
  through three discrete agent passes (`plan` / `implement` / `verify`)
  with a structured-JSON verify verdict
  (`summary` / `files_changed` / `task_compliance` / `deviations` /
  `notes`) persisted to `phases/<slug>/verify.json`. Phase frontmatter
  `summary:` auto-fills from the verify pass so the SDD history pane
  stops showing "(no summary)". Opt-in plan-review gate adds an
  Approve/Discard step between plan and implement. New Settings card
  exposes per-workspace mode toggle + plan-gate checkbox. Legacy
  workspaces auto-migrate to `single_call` mode on first hydrate so
  in-flight workflows don't shift behaviour mid-execution. See SDD
  workspace `sdd-926d553a7b` for the full design.
- **SDD sub-step badges + verify pane in SddCard** — running phases
  surface `Phase N — planning / implementing / verifying` labels in
  warm-live tone; completed phases gain an inline Verify pane
  rendering the JSON verdict with ✓ task compliance and ⚠️
  deviations. Live-feed action log gains `— plan —` / `— implement —`
  / `— verify —` divider rows so the user can scan which pass an
  event belongs to. Crash-recovery banner now reads
  "Phase N (during &lt;sub-step&gt;) interrupted" when a checkpoint
  is recovered from `control/phase-N-substep-state.json`.
- **Per-workspace config cog in SddCard** — `⚙` button in the card
  header opens an inline drawer with mode select (single-call /
  three-call) + plan-review gate checkbox, scoped to the workspace.
- **Dedicated `plan_discarded` failure trigger** — Discard button
  during plan-review now calls `sdd_discard_phase_plan` (instead of
  the legacy `skipSddPhaseWithReason` workaround), flips the phase
  to `failed { trigger: plan_discarded }` so the standard failure
  card (Retry / Edit & retry / Skip) surfaces.
- **SDD manual Continue + Accept-anyway buttons** — when an agent turn
  ends without the auto-fire dispatcher picking up (silent-drop or
  stale bundle), the SddCard footer now surfaces a Continue button for
  `phase_planning` / `phase_implementing` / `phase_verifying` stages
  that re-fires the substep prompt through the chat send pipeline
  (`manualContinueSdd` bypasses the `lastAutoFireKey` dedupe). New
  Tauri command `sdd_accept_phase_failed(id, phase, reason)` flips a
  `failed` phase to `done` with `accepted_reason` persisted to phase
  frontmatter + `phase-meta.json` for the audit trail; surfaced as the
  "✓ Accept anyway" button alongside Retry / Edit & retry / Skip /
  Rollback. Failure footer now also renders inside `viewOnly` standalone
  view so popover-opened failed workspaces have a recovery path.
- **Claude CLI background-task watcher** (`claude_bg.rs`) — polls the
  output file's mtime for Bash tasks fired with `run_in_background:true`
  and emits `claude:bg_done` with a tail snapshot when the file goes
  idle (or `timed_out` when the watch cap is reached). `+page.svelte`
  folds the tail into a silent continuation prompt so the agent picks
  up the bg task's output without the user having to nudge.
- **SDD auto-fire dispatcher** — `+page.svelte` registers an auto-fire
  dispatcher before `initSdd` so hydrate-time catch-up (workspace left
  mid-substep across app restart) reaches the chat send pipeline
  immediately. Pending silent prompts park in `pendingSilentBySession`
  when a turn is in flight and drain at end-of-turn.

### Changed

- **`SddStage` enum extended** with `PhasePlanning` /
  `PhasePlanReview` / `PhaseImplementing` variants; existing
  `PhaseVerifying` placeholder now wired. Single-call mode keeps
  emitting `PhaseRunning` byte-for-byte. New `FailureTrigger`
  variants — `PlanMutatedDisk` / `VerifyFailed` / `VerifyParseFail` /
  `PlanDiscarded` — drive richer failure-card copy.
- **Failed SDD workspaces always render inline** — `isSddCardHidden`
  now ignores the hidden flag for `failed` stage. A hidden failure
  card was a footgun (workspace silently stuck with no entry point).

### Fixed

- **SDD three-call mode close-out tools** — `sdd_save_phase_plan`,
  `sdd_complete_phase_implement`, `sdd_save_phase_verify`,
  `sdd_approve_phase_plan`, and `sdd_discard_phase_plan` are now
  exposed via the `woom-app` MCP sidecar, allowlisted in
  `claude_mcp.rs`, and routed through `+page.svelte`'s
  `handleAppNavigation` so the Tauri commands actually fire when the
  agent calls them. Without this, three-call execution mode was stuck
  after the plan pass — the agent couldn't advance the substep
  checkpoint.
- **Cold-start cursor/claude detection retry** — `refreshAgentsWithBootRetry`
  now retries when the binary is detected but `--version` returned
  None within the 2 s timeout (first child spawn on macOS routinely
  needs 1–3 s). Users no longer have to reload the webview after a
  fresh launch.

### Refactor

- **SettingsView split** — the 2166-line monolith carved into a thin
  shell plus eight per-section components (`Storage`, `Appearance`,
  `Memory`, `Updates`, `SDD`, `Privacy`, `Agents`, `About`) under
  `lib/views/settings/`. Shared chrome lives in `chrome.css`. All 14
  cards preserved, no behaviour change.
- **Rail split** — `Rail.svelte` carved into five focused subcomponents
  (`RailActiveHalo`, `RailIdentityAvatar`, `RailSourceButton`,
  `RailSystemButton`, `RailTooltip`) under `components/ui/rail/`.

### Performance

- **Coalesced streaming deltas** — `appendToLastAssistant` /
  `appendToLastThinking` no longer rebuild the full session list on
  every token. Per-token text/thinking deltas batch into an rAF-aligned
  `_streamQueue` that flushes once per frame per session with merged
  consecutive same-kind ops. Causal order preserved via
  `flushStreamQueueNow()` calls in non-streaming mutators.
- **Dirty-tracked session writes** — `flushToDisk` ref-compares each
  session against the last-written snapshot, so a 10-session workspace
  with one active stream rewrites one file per debounce window instead
  of ten. Disk debounce raised 400 ms → 800 ms.
- **Lazy-mount chat bodies** — `ChatThread` mounts message bodies via
  IntersectionObserver with `FRESH_TAIL = 5` always-eager near the tail.
  Early-exit in `anchoredQuestionIds` skips the O(messages × events ×
  segments) regex traversal when no pending question actions exist.

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
