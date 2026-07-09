# Ledger Robust Execution Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make a Ledger run impossible to freeze — every worker attempt is time- and activity-bounded, every failure surfaces with a reason, the repo's declared Node toolchain is applied to every subprocess, and the agent can stop a run itself.

**Architecture:** All execution logic lives in `apps/desktop/src-tauri/src/ledger.rs`. Ledger spawns both subprocess kinds it must bound — the Claude worker (`Command::new(bin)`, `:531`, awaited at `:1204`) and the shell check (`run_shell_check`, `:837`). Decision logic is extracted into pure functions covered by the existing `#[cfg(test)] mod tests` (currently 20 tests) so it is verified headless with `cargo test`; the async wiring + env injection are integration concerns the app owner verifies at runtime. The `ledger_stop` MCP tool follows the established thin-sidecar-stub + `+page` dispatch pattern used by `start_ledger`.

**Tech Stack:** Rust (tokio async), Tauri 2, the woom-app rmcp sidecar, SvelteKit (`+page.svelte` MCP dispatch).

**Spec:** `docs/superpowers/specs/2026-07-09-ledger-robust-execution-design.md`

**Verification commands** (run from `apps/desktop/src-tauri`):
- Unit tests: `cargo test --lib ledger::tests`
- Sidecar compiles: `cargo check -p woom-app`
- Frontend (for +page/UI): from `apps/desktop`, `pnpm check`

---

## Phase 1 — Bounded execution (kills the hang)

### Task 1: Watchdog decision function (pure)

**Files:**
- Modify: `apps/desktop/src-tauri/src/ledger.rs` (add near the other pure
  helpers, above `#[cfg(test)] mod tests`)
- Test: same file, inside `mod tests`

- [ ] **Step 1: Write the failing test** (append inside `mod tests`)

```rust
#[test]
fn watchdog_flags_stall_and_timeout() {
    // limits: item wall-clock 600s, stall 120s
    let lim = WorkerLimits { item_secs: 600, stall_secs: 120 };
    // healthy: recent activity, well under wall-clock
    assert_eq!(worker_watchdog(10, 5, lim), WorkerTick::Continue);
    // stalled: no activity for >= stall window
    assert_eq!(worker_watchdog(200, 130, lim), WorkerTick::Stall);
    // timed out: total elapsed >= item window (even if just active)
    assert_eq!(worker_watchdog(601, 1, lim), WorkerTick::Timeout);
    // timeout takes precedence over stall when both trip
    assert_eq!(worker_watchdog(700, 300, lim), WorkerTick::Timeout);
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --lib ledger::tests::watchdog_flags_stall_and_timeout`
Expected: FAIL — `WorkerLimits` / `worker_watchdog` / `WorkerTick` not found.

- [ ] **Step 3: Implement**

```rust
#[derive(Clone, Copy, Debug)]
pub(crate) struct WorkerLimits {
    /// Hard wall-clock cap for one worker attempt, seconds.
    pub item_secs: u64,
    /// Max seconds with no stream activity before the attempt is stalled.
    pub stall_secs: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WorkerTick {
    Continue,
    Stall,
    Timeout,
}

/// Pure watchdog decision. `elapsed_secs` = seconds since the attempt
/// started; `idle_secs` = seconds since the last stream event. Timeout
/// wins over stall so the surfaced reason names the harder bound.
pub(crate) fn worker_watchdog(elapsed_secs: u64, idle_secs: u64, lim: WorkerLimits) -> WorkerTick {
    if elapsed_secs >= lim.item_secs {
        WorkerTick::Timeout
    } else if idle_secs >= lim.stall_secs {
        WorkerTick::Stall
    } else {
        WorkerTick::Continue
    }
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test --lib ledger::tests::watchdog_flags_stall_and_timeout`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add apps/desktop/src-tauri/src/ledger.rs
git commit -m "ledger: pure worker-watchdog decision fn + test"
```

### Task 2: Add worker limits constants + optional per-workflow override

**Files:**
- Modify: `apps/desktop/src-tauri/src/ledger.rs` (constants near `CHECK_TIMEOUT` at `:44`; `LedgerWorkflow` struct field)

- [ ] **Step 1: Add constants** (next to `CHECK_TIMEOUT`)

```rust
/// Default hard wall-clock cap for one worker attempt.
const ITEM_TIMEOUT_SECS: u64 = 600;
/// Default no-stream-activity window before a worker attempt is stalled.
const STALL_TIMEOUT_SECS: u64 = 120;
```

- [ ] **Step 2: Add serde-defaulted override field to `LedgerWorkflow`**

Add to the struct (with the other optional fields), so old persisted
workflows still deserialize:

```rust
    /// Optional per-workflow worker wall-clock cap (seconds). None → ITEM_TIMEOUT_SECS.
    #[serde(default)]
    pub item_timeout_secs: Option<u64>,
```

- [ ] **Step 3: Verify old-workflow load still compiles + tests pass**

Run: `cargo test --lib ledger::tests`
Expected: PASS (the existing `wf()` helper must still build — if the test
helper constructs the struct literally, add `item_timeout_secs: None`).

- [ ] **Step 4: Commit**

```bash
git add apps/desktop/src-tauri/src/ledger.rs
git commit -m "ledger: worker timeout constants + per-workflow override field"
```

### Task 3: Wire wall-clock timeout + no-output watchdog into the worker turn

**Files:**
- Modify: `apps/desktop/src-tauri/src/ledger.rs` — `run_worker_streaming`
  (`:511`) stream loop (`:562`) and its call site (`:1204`).

- [ ] **Step 1: Track last activity in the stream loop**

In `run_worker_streaming`, replace the bare
`while let Ok(Some(line)) = lines.next_line().await { … }` with a
`tokio::select!` that races the next line against a stall/timeout timer,
updating a `last_activity: Instant` on every line:

```rust
    let started = std::time::Instant::now();
    let mut last_activity = std::time::Instant::now();
    let lim = WorkerLimits {
        item_secs: item_timeout_secs, // threaded in as a new param (see Step 2)
        stall_secs: STALL_TIMEOUT_SECS,
    };
    loop {
        let tick = tokio::time::sleep(std::time::Duration::from_secs(5));
        tokio::select! {
            line = lines.next_line() => {
                match line {
                    Ok(Some(l)) => { last_activity = std::time::Instant::now(); /* existing parse of `l` */ }
                    Ok(None) | Err(_) => break,
                }
            }
            _ = tick => {
                let elapsed = started.elapsed().as_secs();
                let idle = last_activity.elapsed().as_secs();
                match worker_watchdog(elapsed, idle, lim) {
                    WorkerTick::Continue => {}
                    WorkerTick::Stall => {
                        let _ = child.start_kill();
                        return Err(format!("worker stalled: no output for {idle}s"));
                    }
                    WorkerTick::Timeout => {
                        let _ = child.start_kill();
                        return Err(format!("worker timed out after {elapsed}s"));
                    }
                }
            }
        }
    }
```

(Keep the existing per-line parse body — move it into the `Ok(Some(l))` arm
verbatim, renaming the binding to match.)

- [ ] **Step 2: Thread the per-attempt timeout in**

Add `item_timeout_secs: u64` as a parameter to `run_worker_streaming`. At the
call site (`:1204`) pass `wf_now.item_timeout_secs.unwrap_or(ITEM_TIMEOUT_SECS)`.

- [ ] **Step 3: Verify it compiles + existing tests pass**

Run: `cargo test --lib ledger::tests` then `cargo check` (from `src-tauri`)
Expected: PASS / clean.

- [ ] **Step 4: Commit**

```bash
git add apps/desktop/src-tauri/src/ledger.rs
git commit -m "ledger: bound worker turn (wall-clock + no-output watchdog), kill on trip"
```

### Task 4: Confirm the failure routes to attempt-fail + surfaces

**Files:**
- Modify: `apps/desktop/src-tauri/src/ledger.rs` — the attempt loop around
  the `run_worker_streaming(...).await?` at `:1204`-`:1214`.

- [ ] **Step 1:** The `?` on `run_worker_streaming(...).await?` currently
  bubbles the error out of the whole run loop. Change it to CAPTURE the error
  as this attempt's failure feedback instead of aborting the run:

```rust
    let (text, usage) = match run_worker_streaming(
        app, reg, wf_id, &item.id, &prompt, Path::new(&wt_path), &model,
        wf_now.item_timeout_secs.unwrap_or(ITEM_TIMEOUT_SECS),
    ).await {
        Ok(v) => v,
        Err(reason) => {
            // Bounded failure (timeout / stall / spawn) → feed it back as this
            // attempt's check output and let the attempt loop retry / settle
            // to `failed` after maxAttempts, exactly like a failed check.
            feedback = Some(reason.clone());
            reg.mutate_persist(app, wf_id, |w| {
                for it in w.items.iter_mut() {
                    if it.id == item.id {
                        it.status = "failed".to_string();
                        it.check_output = Some(reason.clone());
                    }
                }
            });
            emit_updated(app, reg, wf_id);
            continue; // next attempt (or the attempt loop settles to failed)
        }
    };
```

(Match the exact variable names + control flow of the existing attempt loop —
read `:1158`-`:1260` first; the intent is: a worker error is a normal attempt
failure, never a run abort.)

- [ ] **Step 2:** Verify no run-abort path remains for a worker error.

Run: `cargo test --lib ledger::tests` + `cargo check`
Expected: PASS / clean.

- [ ] **Step 3: Commit**

```bash
git add apps/desktop/src-tauri/src/ledger.rs
git commit -m "ledger: worker error is a bounded attempt failure, not a run abort"
```

---

## Phase 2 — Toolchain resolution + injection + preflight block

### Task 5: `parse_required_node` (pure)

**Files:** Modify `ledger.rs` (pure helper + test in `mod tests`).

- [ ] **Step 1: Failing test**

```rust
#[test]
fn parse_required_node_reads_sources_in_order() {
    // .nvmrc wins
    assert_eq!(parse_required_node(Some("v22.21.1\n"), None, None).as_deref(), Some("22.21.1"));
    // engines.node next
    assert_eq!(parse_required_node(None, Some(">=22 <23"), None).as_deref(), Some("22"));
    // .tool-versions nodejs line last
    assert_eq!(parse_required_node(None, None, Some("nodejs 22.21.1\npnpm 10.17.1")).as_deref(), Some("22.21.1"));
    // nothing declared
    assert_eq!(parse_required_node(None, None, None), None);
}
```

- [ ] **Step 2: Run — expect FAIL** (`parse_required_node` undefined).
- [ ] **Step 3: Implement**

```rust
/// Extract the repo's declared Node major/full version from, in priority:
/// `.nvmrc`, `package.json` engines.node, `.tool-versions`. Returns the
/// version string stripped of a leading `v`/range operators; None if none
/// declared. Best-effort: takes the first numeric token.
pub(crate) fn parse_required_node(
    nvmrc: Option<&str>,
    engines_node: Option<&str>,
    tool_versions: Option<&str>,
) -> Option<String> {
    fn first_version(s: &str) -> Option<String> {
        let t: String = s.trim().trim_start_matches(['v', 'V', '>', '=', '<', '^', '~', ' ']).to_string();
        let v: String = t.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
        if v.is_empty() { None } else { Some(v) }
    }
    if let Some(s) = nvmrc { if let Some(v) = first_version(s) { return Some(v); } }
    if let Some(s) = engines_node { if let Some(v) = first_version(s) { return Some(v); } }
    if let Some(s) = tool_versions {
        for line in s.lines() {
            if let Some(rest) = line.trim().strip_prefix("nodejs") {
                if let Some(v) = first_version(rest) { return Some(v); }
            }
        }
    }
    None
}
```

- [ ] **Step 4: Run — expect PASS.**
- [ ] **Step 5: Commit** `git commit -m "ledger: parse_required_node + test"`

### Task 6: `detect_version_manager` + `preflight_decision` (pure)

**Files:** Modify `ledger.rs` (pure helpers + tests).

- [ ] **Step 1: Failing test**

```rust
#[test]
fn preflight_decision_covers_inject_block_noop() {
    // no declared version → Noop regardless of active
    assert_eq!(preflight_decision(None, Some("25.2.1"), Some("/nvm/22/bin")), Preflight::Noop);
    // required matches active major → Noop
    assert_eq!(preflight_decision(Some("22"), Some("22.21.1"), None), Preflight::Noop);
    // mismatch + a resolvable bin → Inject
    assert_eq!(
        preflight_decision(Some("22"), Some("25.2.1"), Some("/nvm/22/bin")),
        Preflight::Inject("/nvm/22/bin".to_string())
    );
    // mismatch + no bin → Block with a reason mentioning both versions
    match preflight_decision(Some("22"), Some("25.2.1"), None) {
        Preflight::Block(r) => { assert!(r.contains("22") && r.contains("25")); }
        other => panic!("expected Block, got {other:?}"),
    }
}
```

- [ ] **Step 2: Run — expect FAIL.**
- [ ] **Step 3: Implement** (`detect_version_manager` scans for fnm/nvm/volta/asdf
  and, combined with the required version, yields a resolved bin path passed
  into `preflight_decision`; keep the filesystem probing in a thin non-pure
  wrapper so the decision stays pure):

```rust
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Preflight {
    Noop,
    Inject(String),   // prepend this dir to PATH
    Block(String),    // refuse to run; reason for the card
}

/// `required` = major/full from parse_required_node; `active` = `node -v`
/// output stripped; `resolved_bin` = Some(dir) when a version manager can
/// supply `required`. Compares on the major component.
pub(crate) fn preflight_decision(
    required: Option<&str>,
    active: Option<&str>,
    resolved_bin: Option<&str>,
) -> Preflight {
    let req = match required { Some(r) => r, None => return Preflight::Noop };
    let major = |v: &str| v.trim_start_matches('v').split('.').next().unwrap_or("").to_string();
    if let Some(a) = active { if major(a) == major(req) { return Preflight::Noop; } }
    match resolved_bin {
        Some(bin) => Preflight::Inject(bin.to_string()),
        None => Preflight::Block(format!(
            "Node {} ≠ required {} (.nvmrc/engines); workers would fail install. \
             Install fnm/nvm or switch Node, then re-run.",
            active.unwrap_or("?"), req
        )),
    }
}
```

- [ ] **Step 4: Run — expect PASS.**
- [ ] **Step 5: Commit** `git commit -m "ledger: version-manager detect + preflight decision + tests"`

### Task 7: Run preflight at run start; inject or block

**Files:** Modify `ledger.rs` — `run_ledger` (`:1061`), before the run loop;
`run_shell_check` (`:837`) + `run_worker_streaming` (`:531`) env.

- [ ] **Step 1:** In `run_ledger`, after the worktree is ready and before the
  loop, gather the probe (read `.nvmrc`/`package.json`/`.tool-versions` from
  the worktree; `node -v`; scan for a version manager), call
  `parse_required_node` + `detect_version_manager` + `preflight_decision`,
  then:
  - `Preflight::Block(reason)` → set workflow `status = "failed"`, stamp the
    reason, `emit`, and `return Ok(())` (no worker dispatch).
  - `Preflight::Inject(bin)` → stash the bin dir on a run-local variable
    threaded into both spawners.
  - `Preflight::Noop` → nothing.

- [ ] **Step 2:** Thread the optional inject-bin into `run_worker_streaming`
  and `run_shell_check`; when present, prepend to `PATH`:

```rust
    if let Some(bin) = inject_bin {
        let path = std::env::var("PATH").unwrap_or_default();
        cmd.env("PATH", format!("{bin}:{path}"));
    }
```

- [ ] **Step 3:** Verify: `cargo test --lib ledger::tests` + `cargo check`.
- [ ] **Step 4: Commit** `git commit -m "ledger: preflight env check — inject repo Node or block run with reason"`

---

## Phase 3 — `ledger_stop` MCP tool (agent-side cancel)

### Task 8: Sidecar tool + params

**Files:** Modify `apps/desktop/src-tauri/sidecars/woom-app/src/main.rs`
(mirror `start_ledger` / `ledger_run`).

- [ ] **Step 1:** Add the `#[tool]` fn + a `LedgerStopParams { workflow_id: String }`
  struct (mirror `LedgerLaunchParams`):

```rust
    #[tool(
        description = "Stop / cancel a RUNNING Ledger workflow. Pass the `workflowId`. Use when a run is stuck, misbehaving, or you decide to abort. Halts in-flight worker turns; already-committed items are kept and the workflow can be resumed later with ledger_run."
    )]
    async fn ledger_stop(
        &self,
        Parameters(LedgerStopParams { workflow_id }): Parameters<LedgerStopParams>,
    ) -> Result<CallToolResult, ErrorData> {
        if workflow_id.trim().is_empty() {
            return Err(ErrorData::invalid_params("workflowId required", None));
        }
        Ok(CallToolResult::success(vec![Content::text(
            "Stopping the ledger run. In-flight workers are cancelled; committed items are kept — resume later with ledger_run.",
        )]))
    }
```

- [ ] **Step 2:** Verify `cargo check -p woom-app`. **Commit.**

### Task 9: `+page` dispatch + Tauri command

**Files:** Modify `apps/desktop/src/routes/+page.svelte` (dispatch near the
other `mcp__app__ledger_*` blocks, ~`:2236`); confirm a `ledger_cancel`/stop
Tauri command exists in `ledger.rs` (the card's Stop button already cancels —
reuse that command).

- [ ] **Step 1:** Add dispatch:

```js
    if (name === 'mcp__app__ledger_stop') {
      void invoke('ledger_cancel', { workflowId: str('workflow_id') });
      return;
    }
```

(Use whatever command name the card's Stop button invokes — grep
`ledger_cancel` / `ledger_stop` in `ledger.svelte.ts` + `LedgerCard.svelte`
and reuse it; do NOT add a second cancel path.)

- [ ] **Step 2:** Verify `pnpm check` (from `apps/desktop`). **Commit.**

---

## Self-review notes

- Spec coverage: L1 → Tasks 1-4; L2 → Tasks 5-7; L3 (surfacing) → Task 4 +
  Task 7 Block; L4 (stop tool) → Tasks 8-9; L5 (resume) → reused, asserted in
  Task 7 Block ("committed items kept").
- Pure fns fully coded + tested (Tasks 1,5,6). Async wiring (Tasks 3,4,7) gives
  concrete code against confirmed signatures; exact variable names to be matched
  to the live attempt loop (`:1158`-`:1260`) when implementing.
- Runtime-only verification (child-kill, env injection, real hang) is the app
  owner's manual test — noted in the spec's Risks.
