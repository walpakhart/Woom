# Forgehold — Future Features (post-1.0 backlog)

**Version:** 0.1
**Last updated:** 2026-04-29
**Status:** ideas + sketches *outside* the current 10 module specs.

> 1.0 fixes what we shipped (`docs/ROADMAP_1.0.md`). This file fixes
> what we **didn't** ship at all — new pillars, sources, surfaces, and
> agent capabilities that don't live inside Editor / Agents / GitHub /
> Jira / Sentry / Workbench / Connections / Command Palette / MCP /
> Canvas as those modules exist today.
>
> Treat this as a **menu**, not a roadmap. Things move from here into
> a real roadmap once we pick them.

Each entry has:

- **Pitch** — one sentence the user would understand.
- **Anatomy** — concrete sketch, where it slots in.
- **Effort** — `S` (≤1 wk), `M` (1–3 wk), `L` (3–6 wk), `XL` (6+ wk).
- **Value** — `essential` / `important` / `nice-to-have` (subjective).
- **Depends on** — 1.0 capabilities this assumes are shipped.

---

## A. New data sources (more "Connections")

These all go through the same flow as 1.0 GitHub / Jira / Sentry —
Keychain creds, MCP sidecar, dedicated column, slide-over. The pattern
is well-trodden; the cost is mostly per-source domain modelling.

### A.1 Slack

- **Pitch:** A column for the channels and threads the user actually
  follows, with the agent able to post / search / DM and link any
  message into a canvas.
- **Anatomy:** `forgehold-slack` sidecar (Bolt + RTM); `slack` column
  with Workspace → Channel filters; slide-over for a thread; agent
  tools `slack.search`, `slack.post_message`, `slack.add_reaction`.
- **Effort:** L. Slack's surface is huge; we ship channels + threads
  + DMs only.
- **Value:** essential — it's where every conversation finishes.
- **Depends on:** §1.2 OAuth pattern (Slack OAuth flow), §2.7.2 multi-
  account (Slack workspaces are usually multiple).

### A.2 Linear

- **Pitch:** Mirror of the Jira pattern. Column with views / cycles /
  triage; slide-over with sub-issues / parent / project; agent can
  triage / move / comment.
- **Anatomy:** `forgehold-linear` sidecar over Linear's GraphQL.
  `LinearColumn`, `LinearTab` (org browser), filters as chips.
- **Effort:** M (Linear's API is clean).
- **Value:** essential for Linear-native teams.
- **Depends on:** Jira pattern from 1.0.

### A.3 GitLab

- **Pitch:** GitHub-shaped column for self-hosted teams.
- **Anatomy:** `forgehold-gitlab` sidecar; same `code/MRs/issues/CI/
  releases` sections; same propose-* shape.
- **Effort:** M (mostly remap the GitHub adapter).
- **Value:** important.
- **Depends on:** GitHub OAuth + sidecar pattern.

### A.4 Notion

- **Pitch:** A read-mostly column for design docs, plus drag-into-
  agent / canvas for any page.
- **Anatomy:** `forgehold-notion` sidecar; "page tree" sidebar plus
  page renderer (Notion-flavour MD); slide-over with page content.
- **Effort:** M.
- **Value:** important for doc-heavy teams.
- **Depends on:** OAuth.

### A.5 Discord

- **Pitch:** Same as Slack but for community / OSS workflows.
- **Anatomy:** `forgehold-discord` sidecar; Server → Channel; same
  slide-over thread. No DMs unless explicitly opted in.
- **Effort:** L.
- **Value:** nice-to-have (niche audience).

### A.6 Datadog (or compatible APM)

- **Pitch:** Logs, metrics, traces, dashboards in a column. Drag a
  trace into an agent; pin a flame-graph onto a canvas.
- **Anatomy:** `forgehold-datadog` sidecar; column flavours `Logs`,
  `Metrics`, `APM`, `Dashboards`. Logs section streams via Datadog's
  events API. Metrics widget renders sparklines.
- **Effort:** XL — APM data is its own beast.
- **Value:** essential for SRE workflows.
- **Depends on:** OAuth + a small charting primitive (sparklines).

### A.7 PagerDuty / OpsGenie (on-call)

- **Pitch:** Active incident column + on-call schedule overlay so you
  see who's primary right now.
- **Anatomy:** `forgehold-pagerduty` sidecar; `Incidents` column;
  slide-over per incident with timeline; "ack / resolve / escalate"
  mutations.
- **Effort:** M.
- **Value:** important for ops teams.

### A.8 Cloud consoles (AWS / GCP / Azure)

- **Pitch:** A column listing your Lambdas / EC2 / Cloud Run / Cloud
  Functions filtered by env, with a slide-over showing logs + recent
  invocations.
- **Anatomy:** Per-cloud sidecar (AWS SDK); column filters by region
  + service; slide-over groups recent logs from CloudWatch / Cloud
  Logging. `propose_*` for restart / scale / deploy.
- **Effort:** XL (one cloud at a time).
- **Value:** important for cloud-heavy backends.
- **Depends on:** Vault / secret pattern (§E.5) — you don't want raw
  AWS creds in a Keychain blob.

### A.9 Kubernetes

- **Pitch:** `kubectl get pods --watch` as a column, with a slide-
  over per pod (logs + events + describe).
- **Anatomy:** `forgehold-k8s` sidecar talking to a kubeconfig; column
  filters context + namespace + label-selector; slide-over with
  `Logs / Events / Describe / Exec` tabs (exec opens an embedded
  terminal).
- **Effort:** L.
- **Value:** important for backend engineers.
- **Depends on:** Terminal column (§B.1).

### A.10 Calendar (Google / Outlook)

- **Pitch:** A timeline column with today's meetings + linked
  tickets / PRs / docs per event.
- **Anatomy:** `forgehold-calendar` sidecar; column shows agenda +
  busy-slots; per-event tabs link to a workbench preset.
- **Effort:** M.
- **Value:** nice-to-have for solo devs, important in PM-heavy roles.

### A.11 Stripe / Shopify / payment / commerce

- **Pitch:** Customer support workflow — find an order, refund, see
  the related Sentry trace.
- **Anatomy:** `forgehold-stripe` sidecar; `Customers` column with
  search; per-customer slide-over with charges / refunds / linked
  Sentry user.
- **Effort:** L.
- **Value:** niche but high-value where applicable.

### A.12 LaunchDarkly / Statsig / GrowthBook (feature flags)

- **Pitch:** Column listing flags, kill-switch from the workbench,
  slide-over with rollout history.
- **Anatomy:** Sidecar per provider; `propose_flag_change` action card
  before any flip.
- **Effort:** M.
- **Value:** important for product teams.

---

## B. New columns (Forgehold-internal surfaces)

Things that don't reach out to a third party — they just give the user
new tools inside the workbench.

### B.1 Terminal column

- **Pitch:** A real terminal column (like iTerm2 inside Forgehold) so
  the agent's `effectiveCwd` and your shell live side-by-side.
- **Anatomy:** xterm.js + a Tauri PTY backend; tabs per shell session;
  inherits `effectiveCwd` from the linked agent / editor; `propose_
  bash` action cards can run **inside** this terminal so the user sees
  the output.
- **Effort:** L.
- **Value:** essential — every dev I know wants this.
- **Depends on:** PTY plumbing in Rust (`portable-pty` crate works).

### B.2 Database column

- **Pitch:** Connect a Postgres / MySQL / SQLite / Mongo / Redis;
  browse schemas, run queries, save snippets, inspect row counts.
- **Anatomy:** `forgehold-db` sidecar (per backend); column with
  `Schema sidebar / Query editor (CodeMirror SQL) / Result table`;
  slide-over with row detail + edit; agent gets `db.run_query` tool.
- **Effort:** XL.
- **Value:** important — it replaces "open TablePlus" for most quick
  triage.
- **Depends on:** §1.2 OAuth doesn't apply here; we use connection-
  string auth stored in Keychain.

### B.3 HTTP / API client column

- **Pitch:** Postman-as-a-column. Build a request, save collections,
  send, inspect response. Linkable from any URL in chat / canvas.
- **Anatomy:** Reuses CodeMirror (JSON / form / multipart editor);
  request store on disk; agent tool `http.send` for use in workflows.
- **Effort:** M.
- **Value:** important for API-heavy work.

### B.4 Notes column

- **Pitch:** Plain markdown notes per workspace, wiki-linked
  (`[[other-note]]`), drag-to-canvas to import the body as a sticky.
- **Anatomy:** A flat folder of `.md` files
  (`~/Library/Application Support/Forgehold/notes/`); column shows
  list + active note; CodeMirror MD editor with preview pane.
- **Effort:** M.
- **Value:** essential — every dev keeps scratch notes somewhere.

### B.5 Tasks column (private TODO)

- **Pitch:** Local TODO list, separate from Jira. Useful for "today's
  scratch" things that shouldn't litter the team's tracker.
- **Anatomy:** Flat JSON file; checkboxes; drag-from-Jira to "shadow
  copy" a ticket as a private task.
- **Effort:** S.
- **Value:** important.

### B.6 Daily standup column

- **Pitch:** "What I did yesterday" auto-collected from commits, PRs,
  Jira transitions, and chat user messages, ready to copy into Slack.
- **Anatomy:** Aggregator that pulls from connected sources for the
  last 24 h, groups by source, renders a single MD block.
- **Effort:** M.
- **Value:** nice-to-have but loved when it lands. Pitches the cross-
  source query pipeline (§D.1).

### B.7 Build / test runner column

- **Pitch:** Watches `pnpm test --watch`, `cargo test`, `vitest`,
  `pytest`, etc. Streams pass/fail tree, jumps to failing test in
  Editor.
- **Anatomy:** Rust child-process; output parser per framework
  (start with vitest + jest + cargo); column shows tree + failure
  detail.
- **Effort:** L.
- **Value:** important — replaces the second terminal everyone has
  open.
- **Depends on:** Terminal column (§B.1) for fallback raw output.

### B.8 Memory / RAG column

- **Pitch:** Visualise what `forgehold-memory` knows. Pin facts,
  archive, search.
- **Anatomy:** Renders the memory graph as a list + inline edit; right-
  click "Pin to context" to keep it fresh.
- **Effort:** M.
- **Value:** important for power users running long-running agents.
- **Depends on:** §2.9.5 (memory descriptors) shipping in 1.0.

### B.9 Diff inspector column

- **Pitch:** Open any two refs / PRs / files / branches and view the
  diff in a dedicated column (today the only diff view is inside
  GitHub PR detail).
- **Anatomy:** Reuses CodeMirror MergeView; left/right pickers in the
  header.
- **Effort:** M.
- **Value:** important for code review-heavy workflows.

### B.10 Profiler / flame-graph viewer

- **Pitch:** Drop a profile file (chrome trace, perf, samply) and get
  a flame chart inline.
- **Anatomy:** `speedscope`-style renderer; lazy-loaded; zoom + search.
- **Effort:** L.
- **Value:** niche, but irreplaceable when you need it.

---

## C. Agent capabilities (beyond chat)

The 1.0 agent column does live chat plus action cards. These add new
modes of agent operation.

### C.1 Watch mode

- **Pitch:** "Watch this file / canvas / Jira filter and ping me when
  X happens." The agent runs as a tiny background loop.
- **Anatomy:** A `WatchSession` distinct from `ClaudeSession`: no chat
  UI, just a small dock at the rail with a status pill and last-fired
  reason. Defined by `trigger` (file change, schedule, JQL match,
  Sentry alert) + `prompt` + `output channel` (chat / Slack / canvas
  sticky).
- **Effort:** L.
- **Value:** essential — unlocks "agent helps me triage Sentry every
  hour" patterns.
- **Depends on:** Trigger framework (§E.1).

### C.2 Auto-pilot

- **Pitch:** Hand a Jira ticket to an agent and have it work on a
  branch unsupervised until it's PR-ready. Final user touch is
  reviewing the PR.
- **Anatomy:** A wrapper around the existing agent column that runs
  with `approvalPolicy: 'aggressive'` and a hard budget cap. Posts a
  status update every N tool calls.
- **Effort:** L.
- **Value:** important — table-stakes for the "AI engineer" claim.
- **Depends on:** §2.2.5 approval policies, §2.2.21 cost ceiling.

### C.3 Code review agent

- **Pitch:** Open a PR slide-over, click "Have agent review" — agent
  posts inline comments + a top-level summary.
- **Anatomy:** A `ReviewAgentSession` with read-only GitHub tools +
  one `propose_review` action card holding the draft. The user can
  edit before submit.
- **Effort:** M.
- **Value:** important.

### C.4 Sentry auto-triage agent

- **Pitch:** Daily, sweeps unresolved Sentry issues, assigns
  ownership based on the stack frame's `git blame`, summarises top 5
  on a canvas slide.
- **Anatomy:** A `WatchSession` with a 24-h schedule; output goes to
  a dedicated canvas + a Slack DM.
- **Effort:** M.
- **Value:** important for ops-heavy teams.
- **Depends on:** §C.1 watch mode + Editor blame (`§2.1.15`).

### C.5 Smart compose for Jira / GitHub

- **Pitch:** A button next to the Jira / GitHub composer that drafts
  the title + body from the current chat / linked PR / canvas
  context.
- **Anatomy:** Plumbed through Claude with a small prompt template; UI
  is the existing composer with a "Draft with AI" pill.
- **Effort:** S.
- **Value:** important.

### C.6 Conversation branching

- **Pitch:** Fork a chat at any message; explore two paths in
  parallel (different agents / models).
- **Anatomy:** `ClaudeSession.parentId` + `forkedAt: messageIndex`;
  UI renders forks side-by-side under the same tab.
- **Effort:** L.
- **Value:** nice-to-have.

### C.7 Multi-agent orchestration

- **Pitch:** "Claude, ask Cursor to compile this and report errors."
  Two agents collaborate via a shared MCP channel.
- **Anatomy:** A new `mcp__app__ask_agent(instance_name, prompt)` tool
  that posts a message to another agent's chat and streams the result
  back.
- **Effort:** M (mostly just plumbing — the existing agents already
  drop into the same workbench).
- **Value:** experimental; novel positioning.

### C.8 Voice input

- **Pitch:** Hold ⌥ to dictate into the composer. Local Whisper.
- **Anatomy:** `whisper.cpp` bundled or a Tauri sidecar; mic capture
  via web APIs.
- **Effort:** L.
- **Value:** nice-to-have.

### C.9 Image generation tool

- **Pitch:** Agent calls `image.gen({prompt})` and the output lands
  on the linked canvas as a `image-card`.
- **Anatomy:** Sidecar wrapping any image API (OpenAI / Anthropic /
  Replicate); `propose_image` action card with cost preview.
- **Effort:** M.
- **Value:** nice-to-have, fun.

### C.10 RAG over the active repo

- **Pitch:** Agent answers "where do we set the auth header?" without
  reading the entire tree by hitting an embedded vector index.
- **Anatomy:** `forgehold-rag` sidecar building / refreshing a local
  vector index per repo; `rag.query` and `rag.refresh` tools.
- **Effort:** XL — embeddings + chunking + invalidation is its own
  product.
- **Value:** important — tackles "the agent rereads the same files
  every turn" pain.

### C.11 Custom agent personas

- **Pitch:** Saved persona = system prompt + MCP profile + model +
  default cwd. Pick from a dropdown when starting a session.
- **Anatomy:** `personas.json`, surfaced in the agent column header
  picker. "Code reviewer", "Debugger", "Tech writer".
- **Effort:** S.
- **Value:** important — codifies "the agent that's good at X".

### C.12 Per-token streaming display

- **Pitch:** During response, render token usage updating live in the
  composer footer (instead of after the turn).
- **Anatomy:** Read partial usage from `MessageEvent.usage` deltas.
- **Effort:** S.
- **Value:** nice-to-have.

---

## D. Cross-source intelligence

Pillars where the value is connecting *between* the modules we already
have.

### D.1 Unified search

- **Pitch:** ⌘K Pro: "find anything across GitHub + Jira + Sentry +
  chat history + canvases + notes". Returns ranked, grouped results.
- **Anatomy:** A small index in SQLite of titles + bodies + ids per
  source, refreshed on poll. Palette section "All sources".
- **Effort:** L.
- **Value:** essential — ties the whole product together.
- **Depends on:** Each source landing on disk (§1.1) so the index has
  something to crawl.

### D.2 Smart inbox

- **Pitch:** A single column merging GitHub mentions, Jira at-me-s,
  Sentry assignments, Slack DMs into a chronological feed with one-
  click "open the thing".
- **Anatomy:** Aggregator over connected sources; column flavour
  `inbox-unified`.
- **Effort:** L.
- **Value:** important — solves "five tabs, all unread".

### D.3 Daily digest

- **Pitch:** Morning email-style summary inside Forgehold: "yesterday
  you closed 3 tickets, got 2 reviews, X new Sentry errors, here's
  today's calendar".
- **Anatomy:** Same aggregator as §D.2 + a templated MD output;
  rendered in a popover or as a daily "first column" in a special
  workbench.
- **Effort:** M.
- **Value:** nice-to-have but buzzy.

### D.4 Context graph

- **Pitch:** A canvas-shaped graph of "things mentioning each other":
  PR ↔ ticket ↔ Sentry issue ↔ commit ↔ chat message. Useful when
  triaging a complex bug.
- **Anatomy:** Background extractor finds cross-references (Jira keys
  in PR titles, GH PR URLs in tickets, etc.). Rendered as a Canvas
  with auto-layout.
- **Effort:** L.
- **Value:** important for tech leads.
- **Depends on:** Canvas v1 (1.0).

### D.5 Cross-source filter chips

- **Pitch:** A column header that says "show me everything from this
  feature area"; chips filter every visible column at once.
- **Anatomy:** Workbench-level `globalFilters` mirrored to each
  column's local filter state.
- **Effort:** M.
- **Value:** nice-to-have.

---

## E. Workflow & automation

Programmable Forgehold — let users automate the boring routes.

### E.1 Triggers + actions framework

- **Pitch:** "When X happens, do Y." X = source event (new Sentry
  issue, PR opened, Jira status change). Y = run agent prompt,
  open a workbench preset, post to Slack, run a shell command.
- **Anatomy:** A `triggers.json` per workspace; UI in Settings to
  list / edit / disable; the watch-mode infrastructure (§C.1) is
  this same engine.
- **Effort:** L.
- **Value:** essential for power users — Forge becomes scriptable.

### E.2 Workflow templates

- **Pitch:** Named multi-step recipes: "Onboard a new microservice"
  = open editor on path X, add Sentry column filtered by project Y,
  start Claude with persona Z. One-click via palette.
- **Anatomy:** JSON describing column kinds + filters + agent
  presets; instantiated into a fresh workbench. Triggerable also via
  triggers (§E.1).
- **Effort:** M.
- **Value:** important — "starter kits" for new team members.

### E.3 Macros / keyboard chords

- **Pitch:** User-defined keystroke recordings ("⌘⌥1" → "open Jira
  triage workbench, start Claude, ask 'morning roundup'").
- **Anatomy:** `macros.json`; recorder UI in Settings.
- **Effort:** M.
- **Value:** nice-to-have.

### E.4 Quick actions on rows

- **Pitch:** Right-click a row in any column to get a context menu of
  kind-aware actions: "Move to backlog", "Mark as duplicate", "Open
  in browser", "Forward to agent". Today the slide-over is the only
  path.
- **Anatomy:** A registry of `kind → actions[]`; renders the same in
  context menu and in `propose_*` action cards.
- **Effort:** M.
- **Value:** important.

### E.5 Vault / secret manager

- **Pitch:** Beyond Keychain — a per-workspace encrypted vault for
  cloud creds / DB strings / SSH keys. Agents request access via an
  approval flow.
- **Anatomy:** `forgehold-vault` sidecar wrapping age / sops /
  keyring; agent tool `vault.get(secretName)` that surfaces a
  "Vault: Claude wants `aws-prod-readonly`. Allow?" modal.
- **Effort:** L.
- **Value:** essential before we ship cloud-console / DB columns
  (§A.8, §B.2).

### E.6 Scheduled runs

- **Pitch:** Cron-style schedule for any workflow (§E.2) or agent
  prompt (§C.1).
- **Anatomy:** `scheduledTasks.json`; runner in Tauri main thread.
- **Effort:** S (once §C.1 lands).
- **Value:** nice-to-have.

---

## F. Collaboration / teams

We currently target solo. These flip Forgehold into a team product.

### F.1 Shared workbenches

- **Pitch:** A workbench saved as a doc in the team org; teammates
  open the same canvas, see the same Sentry filter, follow each
  other's cursor.
- **Anatomy:** A backend (Forgehold Cloud or self-host) holding
  layouts + canvases + chat transcripts. CRDT sync (Y.js).
- **Effort:** XL.
- **Value:** essential for any team-positioning move.

### F.2 Live presence

- **Pitch:** See teammates' avatars on the workbench rail, on the
  canvas, in the GitHub slide-over they're viewing.
- **Anatomy:** WebSocket presence service; cursor tracking on canvas;
  attention indicators in lists.
- **Effort:** L.
- **Value:** important once shared workbenches exist.
- **Depends on:** §F.1.

### F.3 Comments & mentions on canvas / chat

- **Pitch:** Drop a comment pin on a canvas shape; @-mention a
  teammate; they see it in their inbox.
- **Anatomy:** Comment thread per shape; notification fan-out via
  the shared backend.
- **Effort:** M.
- **Depends on:** §F.1.

### F.4 SSO + permissions

- **Pitch:** Org-wide SAML; per-workspace role (admin / member /
  viewer); per-source visibility.
- **Anatomy:** Auth-service in the cloud product; identity provider
  abstraction.
- **Effort:** XL.
- **Value:** required for enterprise.
- **Depends on:** §F.1.

### F.5 Audit log + cost reporting

- **Pitch:** Org admin sees "who used how much LLM compute, where
  agents touched main, who connected what".
- **Anatomy:** Append-only event log per workspace; admin dashboard.
- **Effort:** L.
- **Depends on:** §F.1, §F.4.

### F.6 Billing / quotas

- **Pitch:** Self-serve subscription, per-seat or per-LLM-token; org-
  wide budget ceilings.
- **Anatomy:** Stripe; gating server middleware.
- **Effort:** L.
- **Value:** required for monetisation.

---

## G. Companion surfaces

### G.1 Web read-only

- **Pitch:** Share a canvas via a public URL; recipient sees a read-
  only render in any browser.
- **Anatomy:** Static SSR of `Canvas.toJSON()`; hosted by Forgehold
  Cloud.
- **Effort:** M.
- **Value:** nice-to-have for design review / external sharing.

### G.2 Mobile companion

- **Pitch:** Push notifications for assignments / mentions / agent
  approvals; quick "Approve" / "Resolve" / "Reply" without the
  laptop.
- **Anatomy:** Native shell (React Native or Capacitor) with a slim
  UI hitting the cloud sync from §F.1.
- **Effort:** XL.
- **Value:** nice-to-have.
- **Depends on:** §F.1.

### G.3 Apple Watch quick-triage

- **Pitch:** A Sentry alert lands → wrist haptic → ack from watch.
- **Anatomy:** Tiny watch companion to §G.2.
- **Effort:** M (after the iOS app exists).
- **Value:** novelty.

### G.4 Browser extension

- **Pitch:** Right-click any GitHub PR / Jira issue / Sentry event in
  the browser → "Open in Forgehold". Skips the search step.
- **Anatomy:** Chrome / Firefox extension calling a local
  Forgehold-installed deep link (`forgehold://`).
- **Effort:** S.
- **Value:** important — frictionless entry.

---

## H. Marketplace & extensibility

### H.1 Public MCP API

- **Pitch:** Document the MCP-on-loopback contract so third parties
  can ship sidecars Forgehold loads.
- **Anatomy:** Stable IPC contract + a `~/.config/forgehold/mcps/`
  directory; settings UI to enable / disable user-installed servers.
- **Effort:** M.
- **Value:** important for ecosystem moves.

### H.2 Theme / icon marketplace

- **Pitch:** Pull a community theme from a registry; preview, apply,
  contribute back.
- **Anatomy:** Theme = JSON of CSS variables + icon overrides; index
  hosted on GitHub initially.
- **Effort:** S.
- **Value:** nice-to-have.

### H.3 Workflow marketplace

- **Pitch:** Browse community-shared workflow templates (§E.2).
- **Anatomy:** Same registry pattern as themes; semantic search.
- **Effort:** M.
- **Value:** nice-to-have.

### H.4 Plugin API for canvas blocks

- **Pitch:** Anyone can register a new live block kind via JS.
- **Anatomy:** Sandboxed iframe per plugin; `postMessage` contract;
  `Plugin manifest` describing fields + render function.
- **Effort:** L.
- **Value:** experimental.
- **Depends on:** Canvas v1.

---

## I. Quality of life

Small but loved.

### I.1 Focus mode / Pomodoro

- **Pitch:** ⌘⌥F dims the rest of the workbench, hides notifications,
  starts a 25-min timer.
- **Effort:** S.
- **Value:** nice-to-have.

### I.2 Time tracking

- **Pitch:** Auto-track time-on-ticket: when the user opens Jira
  slide-over for `PROJ-123`, a timer starts; closes when focus leaves
  for >5 min.
- **Anatomy:** Integrates with Jira worklogs (already a 1.0 capability)
  to log automatically (with confirm).
- **Effort:** M.
- **Value:** important for billable consultancies.

### I.3 Snippet library

- **Pitch:** Reusable text snippets (commit-message templates, PR-
  description templates). Pickable in any composer via `;`-prefix.
- **Effort:** S.
- **Value:** nice-to-have.

### I.4 Onboarding tour 2.0

- **Pitch:** After 1.0 nails the first-launch flow, ship contextual
  spotlights — "tip of the day" style.
- **Effort:** S.
- **Value:** nice-to-have.

### I.5 Quick switcher (`⌘E`)

- **Pitch:** Cycle "recent items" — last 10 things you opened across
  any source, fastest possible context switch.
- **Effort:** S.
- **Value:** important.

### I.6 Snapshots / time travel

- **Pitch:** "Snapshot this workbench" → returns later as a frozen
  archive (read-only). Useful for "before-the-deploy" diagnostics.
- **Anatomy:** Same disk layout as canvases, but for whole workbench
  state.
- **Effort:** L.
- **Value:** nice-to-have.

### I.7 Layout templates (visual)

- **Pitch:** Gallery of pre-made workbench layouts (Frontend, Backend
  on-call, Triage). Click to instantiate.
- **Effort:** S.
- **Value:** nice-to-have.

### I.8 Compact / mini mode

- **Pitch:** Collapse Forgehold to a 320×240 widget pinned in the
  corner showing notifications + agent approvals.
- **Effort:** M.
- **Value:** nice-to-have.

---

## J. Insights / observability for the user

Forgehold has rich data on what the user does. Surface it back.

### J.1 Personal usage dashboard

- **Pitch:** "This week you reviewed 7 PRs, closed 4 tickets, agents
  cost $11.40, you spent 6h in Editor, 3h in Canvas."
- **Anatomy:** Aggregates the telemetry pipeline (§1.8 of 1.0).
- **Effort:** M.
- **Value:** nice-to-have.

### J.2 Cost dashboard

- **Pitch:** Per-day / per-session / per-workbench LLM spend.
- **Effort:** S (once §2.2.3 + §2.2.22 land).
- **Value:** important.

### J.3 Time-on-task report

- **Pitch:** "What you worked on yesterday" — feeds standup (§B.6) or
  end-of-week reviews.
- **Effort:** M.
- **Value:** nice-to-have.

---

## K. Distribution / dev story

### K.1 Windows + Linux

- **Pitch:** Currently macOS only. Forge needs to be cross-platform
  for an ecosystem move.
- **Anatomy:** Tauri already cross-builds; effort is in shell-out
  paths (Keychain replacements per OS), notarisation per OS, file-
  watcher edge cases, terminal sidecar.
- **Effort:** XL.
- **Value:** important — doubles addressable audience.

### K.2 Auto-installer / package managers

- **Pitch:** `brew install forgehold` and `winget install`.
- **Effort:** S.
- **Value:** nice-to-have.

### K.3 Headless mode (CLI)

- **Pitch:** `forgehold run workflow my-triage` — execute a workflow
  from the terminal without UI; useful for CI and for cron.
- **Effort:** L.
- **Value:** experimental.

### K.4 Public docs site

- **Pitch:** A real `forgehold.dev` with the module specs, screenshots,
  workflow recipes, integrations directory.
- **Effort:** M.
- **Value:** important once we go public.

---

## Priority Matrix (quick read)

A reading aid. Subjective. Use as a starting argument, not a verdict.

```
                 ESSENTIAL          IMPORTANT          NICE-TO-HAVE
        ┌─────────────────────┬─────────────────────┬─────────────────────┐
   S   │ I.5 Quick switcher  │ C.5 Smart compose   │ C.12 Live tokens    │
        │                     │ B.5 Tasks column    │ I.1 Focus mode      │
        │                     │ G.4 Browser ext     │ I.3 Snippets        │
        ├─────────────────────┼─────────────────────┼─────────────────────┤
   M   │ A.2 Linear          │ A.4 Notion          │ A.10 Calendar       │
        │ B.4 Notes column   │ A.7 PagerDuty       │ B.6 Standup         │
        │ B.3 HTTP client    │ A.12 Feature flags  │ C.6 Branching       │
        │ D.5 Cross-filters  │ E.4 Quick actions   │ I.2 Time tracking   │
        │                     │ J.2 Cost dashboard  │                     │
        ├─────────────────────┼─────────────────────┼─────────────────────┤
   L   │ A.1 Slack          │ A.9 K8s             │ A.5 Discord         │
        │ B.1 Terminal       │ B.7 Test runner     │ A.11 Stripe         │
        │ C.1 Watch mode     │ B.9 Diff inspector  │ C.10 RAG            │
        │ D.1 Unified search │ C.2 Auto-pilot      │ I.6 Snapshots       │
        │ E.1 Triggers       │ C.3 Code review     │                     │
        │ E.5 Vault          │ D.4 Context graph   │                     │
        │                     │ F.2 Presence        │                     │
        ├─────────────────────┼─────────────────────┼─────────────────────┤
  XL   │ B.2 Database column │ A.6 Datadog         │ G.2 Mobile          │
        │ F.1 Shared bench    │ A.8 Cloud consoles  │ K.3 Headless        │
        │                     │ K.1 Win/Linux       │                     │
        └─────────────────────┴─────────────────────┴─────────────────────┘
```

---

## Suggested 2.0 Anchors

If we have to pick what defines 2.0 (a year-ish after 1.0), I'd pick
**three** anchors and order around them:

### Anchor 1 — "The terminal is in the workbench"

- B.1 Terminal column (the moment users stop alt-tabbing to iTerm)
- B.7 Test runner (fits next to terminal)
- B.9 Diff inspector
- E.4 Quick actions

Devs who try this lock in.

### Anchor 2 — "Forgehold is the inbox"

- A.1 Slack
- A.2 Linear (if not in 1.x)
- D.1 Unified search
- D.2 Smart inbox
- G.4 Browser extension

Solves the "five tabs, all unread" complaint definitively.

### Anchor 3 — "Forgehold runs itself"

- C.1 Watch mode
- C.2 Auto-pilot
- C.3 Code review agent
- E.1 Triggers
- E.5 Vault

Moves the AI story from "chat assistant" to "co-worker".

These three anchors are independent enough that they can ship in
parallel tracks; they're cohesive enough that any one of them alone
justifies a 2.0 marketing beat.

---

## Anti-goals (kept here, not just deferred)

Things we explicitly *do not* want to be:

- **An IDE.** No LSP / IntelliSense / refactor tooling. Editor exists
  to keep you in the workbench while you Apply-to-agent — that's it.
- **A CRM.** Customer columns (§A.11) are read/triage, not pipeline.
- **A doc-editor.** Notes (§B.4) are scratch, not Notion-replacement.
- **A standalone monitoring product.** APM (§A.6) is a viewer, not
  Datadog itself.
- **A general scripting platform.** Workflows (§E.1, §E.2) are
  recipes that compose existing tools; they are not Zapier.
- **A presentation tool.** Canvas (1.0) is a thinking surface. We
  don't ship animation timelines or speaker notes.
- **A general LLM playground.** Agents are tied to a workspace; no
  "untethered chat" mode.

Keeping this list short and honest is what stops Forgehold from
sprawling into a Microsoft Office of dev tools.

---

## How this file evolves

- Anyone can add an entry under the right bucket. Format:
  `### X.N Title` + Pitch / Anatomy / Effort / Value / Depends on.
- When something gets accepted into a real roadmap, mark it with
  `→ moved to <roadmap-section>` instead of removing it (audit trail).
- Reorder buckets only when a major release ships.
