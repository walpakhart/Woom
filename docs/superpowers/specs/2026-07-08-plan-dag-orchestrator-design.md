# Plan — a durable, agent-first DAG orchestrator (evolving Ledger)

Date: 2026-07-08
Status: design / awaiting review
Author: brainstorm session (Chat 68)

## Why

Woom has two overlapping agentic task engines:

- **Ledger** (`src-tauri/src/ledger.rs`, `state/ledger.svelte.ts`, `components/agent/LedgerCard.svelte`): a
  sequential, machine-verified checklist in ONE worktree. Per-item commit
  `ledger: <title>`, verification = shell exit-code or a fresh-context LLM grader,
  `deps[]` accepted, "waves" run adjacent `parallel:true` disjoint-file items
  concurrently, review-gate → apply/discard, budget/quota pauses, mid-run steering.
- **Dynamic Workflow (DW)** (`state/dw.svelte.ts`): fan-out of parallel subagents in
  isolated worktrees, each graded by an LLM verifier.

Two engines, no unifying principle → both the agent and the user have to guess which
to use. Execution in Ledger is effectively linear (waves are a disjoint-file hack, not
a topological scheduler). There is no durable plan/contract artifact, and no final
integration verification before apply.

### Market convergence (2026)

- Plan-mode-first + **spec-driven** development (Product Requirement Prompt / PRP).
- **PDAR** loop: Plan → Do → Assess → Review.
- **DAG orchestration** (LLMCompiler-style): planner emits a dependency graph, executor
  topo-sorts, dispatches a node as soon as its deps complete. Reported 1.8–3.7×
  faster, up to 6× cheaper vs. sequential.
- Orchestrator + parallel specialised agents + a final "Janitor" verifier → git merge.
- One git worktree per task (what Woom already does).

### Agent-effectiveness lens (what makes the driving agent work better)

Ranked by impact on the agent that DRIVES this:

1. **A durable plan/contract the agent re-reads.** Woom resets the agent CLI often
   (context compaction / app restart). A task-scoped plan artifact the agent authors and
   maintains is its working memory ACROSS resets. Highest leverage. Missing today.
2. **Ready-set, not order-guessing** — deps + "what can run now" computed for the agent.
3. **Objective per-node verification** — a hard done/not-done signal (Ledger has this; keep).
4. **A final integration gate** — stops premature "done" (a common agent failure mode).
5. **One-glance status** — cheap re-orientation after a reset.

The spine of the design is therefore the **plan artifact + verification gates**. The
Canvas graph view is a human-facing moat, secondary to agent effectiveness.

### Moat

Woom is a local desktop IDE with Canvas, multiple git worktrees, live git, and a visual
chat rail. A live task DAG rendered on a canvas, parallel worktree lanes, drag-to-reorder
of a graph, and a durable local plan artifact tied to the worktree are things a
CLI-only agent structurally cannot do.

## Goal

Evolve Ledger into **Plan**: one durable, agent-first, DAG-scheduled orchestrator that
subsumes both today's Ledger (a chain of deps) and DW (nodes with no deps = fan-out).

## Non-goals

- Not a general workflow/CI product. Scoped to driving Claude agents on one repo.
- No cloud execution. Local worktrees only.

## Design

### Data model (additions to `LedgerWorkflow` / `LedgerItem`)

- `plan: string` (markdown) — the durable PRP/contract for the whole workflow. Authored
  by the agent at build time, editable by the user, rendered at the top of the card and
  persisted with the workflow. This is the spine (agent-effectiveness #1).
- `final_check: string?` — a shell command (build + test) run once after every node
  passes, as the integration "Janitor" gate before apply is allowed.
- Node `kind: 'task' | 'check' | 'explore' | 'group'` — DW dissolves into `task` nodes
  (no deps = fan-out) and `explore` nodes (read-only, produce a note/artifact).
- Node `blocked` status — a skipped node with dependents marks the downstream subtree
  `blocked` (not run blindly). Fixes "dead branch runs after a skip".

### Execution: real topological scheduler (Phase 2)

Replace the linear loop + wave hack with a ready-set scheduler:

1. `ready = nodes where every dep ∈ {passed, skipped}`.
2. Dispatch up to `N` ready nodes concurrently. Writing nodes each get their own
   worktree; read-only (`explore`/`check`) nodes are cheap.
3. On a node finishing → recompute `ready`. Merge writing-node diffs in dependency order.
4. Skipping a node with dependents → mark the subtree `blocked`.

This one engine expresses today's Ledger (a dep chain) AND DW (independent nodes).

### Final verification (Janitor)

After all nodes are `passed/skipped`, run `final_check` against the whole branch diff.
Fail → block apply, surface output, allow retry/steer. Pass → apply is enabled.

### Canvas graph view (Phase 3, the human moat)

Render the DAG on Canvas via `canvas_add_shape` / `canvas_add_edge`: nodes as shapes,
dependency edges, live status colour, parallel lanes. Drag to reorder / add deps
pre-launch; live progress at runtime. The chat card stays the compact list; Canvas is
the full "mission control" graph.

### Housekeeping

- Lift `MAX_ITEMS = 30` (ledger.rs:40).
- Cost-aware retries (don't sink full cost on a node ultimately skipped).
- Dependency visualisation in the card (which items block which), not just a `⤺ N` count.

## Wiring note (must trace before Phase 1 code)

The MCP tools in `sidecars/woom-app/src/main.rs` (`ledger_set_task`, `ledger_add_item`,
`ledger_launch`, `ledger_run`, …) are **stubs**: they validate params and return guidance
text; they do NOT mutate state. The real `ledgerState` mutation happens via a bridge in
the main app (the app applies the agent's tool_use stream). Phase 1 MUST trace this bridge
(how `ledger_add_item` reaches `ledgerState`) before adding `ledger_set_plan` /
`ledger_set_final_check`, so the new tools are wired end-to-end, not left as dead stubs.

## Phasing (each phase independently shippable + verifiable)

- **Phase 1 — Plan artifact (highest agent value, contained).** Add `plan` +
  `final_check` to the model; tools `ledger_set_plan` / `ledger_set_final_check`
  (wired through the traced bridge); render the plan section + final-check gate in
  LedgerCard. Verifiable via `cargo check` + `pnpm check`.
- **Phase 2 — Topological scheduler.** Replace linear loop with the ready-set engine;
  fold DW's fan-out in; `blocked` subtree on skip. Highest risk (async Rust, needs
  runtime testing by the app owner). Reviewed carefully.
- **Phase 3 — Canvas graph view + node kinds + housekeeping.** The visual moat.

## Risks

- Phase 2 rewrites an async Rust scheduler that cannot be runtime-verified by the agent
  (the app owner runs the app). Land behind review + manual testing.
- Folding DW in must not regress existing DW users mid-flight.
