# Ledger robust execution — design

Date: 2026-07-09
Status: design / approved
Author: brainstorm session (Chat "woom")

## Why

A Ledger run (DEVOPS-507) froze at `0/4` for ~35 minutes with no surfaced
failure. Root cause was NOT a code deadlock: a worker did a fresh `pnpm
install` in its worktree under the machine's Node v25, while the repo's
`.nvmrc` pins v22.21.1. The native module `v8-profiler-next@1.10.0` had no
prebuilt for the node-v141 ABI, so `node-gyp` failed (`ELIFECYCLE` exit 1);
the per-item checks `npx nx lint` / `npx nx test` then had no `node_modules`
and could make no progress. The run sat "working" indefinitely.

Two structural gaps let a single bad worker stall the whole run:

1. **The worker agent turn has no wall-clock bound.** `run_worker_streaming`
   (`ledger.rs:511`) spawns the Claude worker (`Command::new(bin)`, `:531`)
   and streams with no `tokio::time::timeout`. Only the *check* command is
   bounded (`CHECK_TIMEOUT = 300s`, `:44` / `:841`). A worker blocked on a
   hung child (or looping on a broken install) runs forever.
2. **The environment is never validated.** Workers are dispatched into a
   worktree whose toolchain may not match what the repo declares, so every
   worker burns turns failing the same install.

Neither failure was surfaced: the card showed `0/4` with no reason.

### What robust orchestrators do (research)

- **Per-attempt wall-clock timeout** returning a *structured* "timed out"
  result to the orchestrator, not a hang (Claude Code Task() no-timeout bug
  #49150; GitHub Actions `timeout-minutes`).
- **No-output / inactivity watchdog** — kill a step that produces no output
  for N seconds (CircleCI `no_output_timeout`, "Too long with no output").
- **Heartbeat / last-activity timestamp** surfaced so "thinking" is
  distinguishable from "hung".
- **Checkpoint + resume** from the last good state rather than restart.

## Goal

Make Ledger execution robust: **every attempt is time-bounded, every failure
surfaces with an actionable reason, the toolchain the repo declares is applied
to every subprocess, and no worker can ever freeze the run.**

## Non-goals

- Not the DEVOPS-507 ticket itself (that's the trigger; unblocked separately).
- No new scheduler (the ready-set picker / `next_ready_item` stays).
- No support for remote/cloud execution — local worktrees only.

## Design

All execution lives in `apps/desktop/src-tauri/src/ledger.rs`. Ledger spawns
BOTH subprocess kinds itself — the worker (`Command::new(bin)`, `:531`) and the
check (`/bin/sh -lc`, `run_shell_check` `:837`) — so it fully controls their
timeouts and environment.

### Layer 1 — Bounded execution (the "never hangs" core)

- **Worker-turn wall-clock timeout.** Wrap the `run_worker_streaming` await in
  `tokio::time::timeout(item_timeout)`. Default `ITEM_TIMEOUT ≈ 600s`,
  overridable per-workflow (new optional field, serde-defaulted so old
  persisted workflows load). On elapse: kill the child, return a structured
  failure `worker timed out after {N}s (last output {M}s ago): {tail}`.
- **No-output watchdog.** `run_worker_streaming` already consumes a stream of
  events; record `last_activity: Instant` on each event. A companion
  `tokio::select!` timer fires if `now - last_activity > STALL_TIMEOUT`
  (default ≈ 120s) → abort the attempt with `no output for {N}s (last:
  {tail})`. Catches a blocked child (hung `pnpm install`) that emits no
  tokens, which the wall-clock alone would only catch much later.
- **Unified failure path.** Worker-timeout, stall, and the existing
  `CHECK_TIMEOUT` all route into the SAME attempt-failure handling that
  already exists: the reason becomes the feedback for the next attempt, the
  attempt counter increments, and after `maxAttempts` the item settles to
  `failed`. The run then reaches the review gate (or stops) instead of
  sitting in `working` forever.

### Layer 2 — Toolchain resolution + injection

Run ONCE when a run starts, per worktree, before any worker dispatch:

- **Resolve the required toolchain (pure, unit-tested).** Read, in order:
  `.nvmrc` → `package.json` `engines.node` → `.tool-versions` (the `nodejs`
  line). Produce a required Node version (or `None` if the repo declares
  nothing — then this layer is a no-op).
- **Detect an installed version manager (pure over a probe result).** Look for
  `fnm`, `nvm`, `volta`, `asdf` (presence of their dirs / binaries).
- **Inject.** If a manager can provide the required version, resolve its Node
  `bin` directory and prepend it to `PATH` in the env of BOTH spawned
  Commands — the worker (`:531`) and the check (`:837`) — via `.env("PATH", …)`.
  Workers then run `node`/`pnpm` at the repo's version automatically; the user
  never runs `nvm use` by hand.
- **Block on unrecoverable mismatch.** If the repo requires version X, the
  active Node differs, and NO manager can supply X → do NOT dispatch workers.
  Fail the run immediately with an actionable reason on the card: `Node
  {active} ≠ required {X} (.nvmrc); workers would fail install. Install fnm/nvm
  or switch Node, then re-run.` Never run workers in a known-broken env.

### Layer 3 — Failure surfacing (never silent)

Every bounded failure (worker timeout, stall, check timeout, preflight block,
max-attempts) sets a human-readable reason + last-output tail on the item/run
and renders on `LedgerCard`. The run leaves `working` deterministically. Builds
on the existing `failed` status + `check_output` rendering.

### Layer 4 — Agent + user control

- **`mcp__app__ledger_stop`** — a new sidecar MCP tool + `+page` dispatch that
  cancels a running workflow, mirroring `ledger_run`. Lets the agent stop a
  runaway run itself (today only the card's Stop button can). Thin-stub +
  desktop-dispatch pattern, same as `start_ledger`.

### Layer 5 — Resume (reuse, don't rebuild)

Per-item commits + resume already exist. A preflight block or an item failure
leaves committed items intact; fixing the env and pressing run resumes from the
first unsettled item. No change beyond making the new failure paths
resume-clean.

## Testability (no shortcuts)

Extract decision logic as pure functions and cover them headless with
`cargo test` (extending the existing 20-test module):

- `parse_required_node(nvmrc, engines, tool_versions) -> Option<Version>`
- `detect_version_manager(probe) -> Option<Manager>`
- `resolve_toolchain_bin(manager, version) -> Option<PathBuf>` (over a faked
  filesystem probe)
- preflight decision: `(required, active, manager) -> Preflight::{Inject(path),
  Block(reason), Noop}`
- watchdog/timeout decision: given `last_activity` + `elapsed` + limits →
  `Continue | Stall | Timeout` (pure; the async wiring calls it)

Async wall-clock/stall wiring and env injection are integration concerns
verified by the app owner at runtime, as before.

## Phasing (each independently shippable + verifiable)

- **Phase 1 — Bounded execution.** Worker wall-clock timeout + no-output
  watchdog + unified failure path + surfacing. Kills the hang outright. Highest
  value, lowest risk.
- **Phase 2 — Toolchain resolution + injection + preflight block.** The
  auto-fix for the Node-mismatch class. Pure resolvers + env injection on both
  Commands.
- **Phase 3 — `ledger_stop` MCP tool.** Agent-side cancel.

## Risks

- **Timeout too tight** kills legitimately long installs/builds → defaults
  generous (600s item / 120s stall) + per-workflow override.
- **Version-manager detection is environment-specific** → on any doubt, degrade
  to the preflight BLOCK (clear message), never a silent wrong-Node run.
- **Async wall-clock/stall + child-kill cannot be agent-runtime-verified** →
  land behind the app owner's manual test; keep the pure decision core
  headless-tested.
