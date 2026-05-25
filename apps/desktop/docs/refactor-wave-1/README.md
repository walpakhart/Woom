# Refactor wave 1 — god-file split

This folder is the operating manual for refactor wave 1 (spec `sdd-9b3215f00c`). Every phase of the wave reads from here at start and writes back here at close.

## Phase index

| # | Phase | Goal |
|---|-------|------|
| 1 | foundation-guardrails | Baseline snapshot, smoke checklist, red-line grep map (this folder) |
| 2 | settings-split | `SettingsView.svelte` (2166) → sectioned components |
| 3 | rail-split | `Rail.svelte` (1392) → shared button + popover |
| 4 | editor-split | `EditorView.svelte` (2292) → file-tree / tabs / host / repo lifecycle |
| 5 | canvas-split | `CanvasSurface.svelte` (2303) → viewport / hit-test / render / tools |
| 6 | agent-ui-split | `Composer.svelte` (2029) + `ChatThread.svelte` (1543) pair split |
| 7 | sessions-store-split | `sessions.svelte.ts` (1641) → disk / mutations / messages / hydrate / distill |
| 8 | sdd-pair-split | `SddCard.svelte` (2475) + `sdd.svelte.ts` (1538) pair split |
| 9 | page-shell-split | `+page.svelte` (6354) → route shell + extracted listeners/dispatch |

Order is **safest → riskiest**. Each phase = one logical commit (or PR), so a regression bisects to one file. See `../../../sdd-9b3215f00c/plan.md` for full rationale.

## Sibling docs

- [`smoke-checklist.md`](./smoke-checklist.md) — 12-item manual smoke test. Run after EVERY phase.
- [`red-lines.md`](./red-lines.md) — grep-based invariants + file:line snapshots. Phase verifiers re-run these greps to catch regressions.
- [`baseline.md`](./baseline.md) — line-count baseline + `npm run check` / `npm test` exit codes at 2026-05-24.

## Verification baseline summary

- **`npm run check` (typecheck):** exit 0, 16 warnings, 12 files-with-problems. Baseline green.
- **`npm test` (vitest):** exit 1 — **pre-existing harness breakage**, NOT caused by this wave. 5 test files fail to load with `__vite_ssr_exportName__ is not defined` (vitest 2.1 / Vite 7 mismatch). Phase verifiers do NOT gate on `npm test` until a follow-up wave repairs the harness.

Full breakdown in `baseline.md`.

## Deviation from plan (executable command)

Plan and phase files reference `bun run check` / `bun run test`. `bun` is not installed on the dev machine where phase 1 ran (`command not found: bun`). Substituted with `npm run check` / `npm test` — same package.json script body executes (`svelte-kit sync && svelte-check --tsconfig ./tsconfig.json` for check; `vitest run` for test). Documented in `baseline.md` § Verification baseline.

## In-flight changes (decision point)

At phase 1 entry, the working tree had uncommitted edits:

```
 M apps/desktop/src-tauri/src/lib.rs
 M apps/desktop/src/lib/stream/agentStream.ts
 M apps/desktop/src/lib/views/apps/agent/ChatThread.svelte
 M apps/desktop/src/routes/+page.svelte
?? apps/desktop/src-tauri/src/claude_bg.rs
```

These are NOT refactor work. They are feature work on the Claude-bg watcher pipeline (`claude_bg.rs` + `agentStream.ts` + `ChatThread.svelte` + a small bit of `+page.svelte`).

**Additional in-flight change made DURING phase 1 implement-pass:** the SDD three-call orchestrator auto-fire fix in `apps/desktop/src/routes/+page.svelte` (extending the end-of-turn handler to auto-fire `phase_implementing` / `phase_verifying` substep transitions, guarded by `ph.plan_body` / `ph.summary` presence). Without this fix the implement-pass of THIS phase would not have fired.

### Decision: commit before phase 2

**Recommendation:** commit the in-flight changes as **two separate commits BEFORE phase 2 starts**:

1. **`fix(sdd): auto-fire implement/verify substep transitions`** — JUST the `+page.svelte` change made during phase 1 implement-pass. This is the orchestrator bug fix; isolating it makes bisect trivial.
2. **`feat(claude-bg): in-flight watcher work`** — the four pre-existing edits (`lib.rs`, `agentStream.ts`, `ChatThread.svelte`, `claude_bg.rs`) as one commit. Feature work, scoped to the bg-watcher pipeline.

**Rationale:** the refactor wave's per-phase commits ship under the assumption of a clean working tree before each phase. Stashing risks losing context; carrying the changes forward risks them colliding with phase 6 (`agent-ui-split`) which touches `ChatThread.svelte`. Committing first keeps the refactor commits clean AND preserves the bg-watcher work as a reviewable atom.

**Execution:** user clicks `propose_commit` for each — the implement-pass agent does NOT commit inline (Woom rule: agent must propose, user approves).

If user prefers stash, that's acceptable too — note the decision in this README before phase 2 starts.

## How phases use this folder

1. At phase start, agent re-reads `red-lines.md` to know what NOT to break.
2. During phase, agent only touches the files the phase plan enumerated; the verify-pass diffs actual changes against that list.
3. At phase close, the implement-pass agent calls `sdd_complete_phase_implement` with a summary + `files_changed`. The verify-pass re-runs `npm run check` + the phase's acceptance criteria + the smoke items relevant to the touched area.

Per spec, the smoke checklist is the human floor; `npm run check` (green) + smoke (green) = phase passes.
