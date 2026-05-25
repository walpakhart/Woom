# Baseline — refactor wave 1

Captured: 2026-05-24. Use as the reference green-state. Every phase verifier diffs against this.

## Top-20 source-file line counts

Source: `find apps/desktop/src -type f \( -name '*.svelte' -o -name '*.ts' \) -exec wc -l {} + | sort -rn | head -20`.

| Rank | Lines | File | Target? |
|------|------:|------|---------|
| 1 | 6354 | `apps/desktop/src/routes/+page.svelte` | yes (phase 9) |
| 2 | 2475 | `apps/desktop/src/lib/components/agent/SddCard.svelte` | yes (phase 8) |
| 3 | 2303 | `apps/desktop/src/lib/views/apps/canvas/CanvasSurface.svelte` | yes (phase 5) |
| 4 | 2292 | `apps/desktop/src/lib/components/editor/EditorView.svelte` | yes (phase 4) |
| 5 | 2166 | `apps/desktop/src/lib/views/SettingsView.svelte` | yes (phase 2) |
| 6 | 2029 | `apps/desktop/src/lib/views/apps/agent/Composer.svelte` | yes (phase 6) |
| 7 | 1641 | `apps/desktop/src/lib/state/sessions.svelte.ts` | yes (phase 7) |
| 8 | 1543 | `apps/desktop/src/lib/views/apps/agent/ChatThread.svelte` | yes (phase 6) |
| 9 | 1538 | `apps/desktop/src/lib/state/sdd.svelte.ts` | yes (phase 8) |
| 10 | 1392 | `apps/desktop/src/lib/components/ui/Rail.svelte` | yes (phase 3) |
| 11 | 1335 | `apps/desktop/src/lib/views/apps/HomeApp.svelte` | no |
| 12 | 1302 | `apps/desktop/src/lib/state/canvas.svelte.ts` | no |
| 13 | 1264 | `apps/desktop/src/lib/views/apps/github/GithubList.svelte` | no |
| 14 | 1194 | `apps/desktop/src/lib/components/ui/WelcomeOverlay.svelte` | no |
| 15 | 1186 | `apps/desktop/src/lib/components/canvas/CanvasShape.svelte` | no |
| 16 | 1111 | `apps/desktop/src/lib/stream/agentStream.ts` | no (spec: streaming-stable, out of scope) |
| 17 | 1083 | `apps/desktop/src/lib/components/ui/CommandPalette.svelte` | no |
| 18 | 1053 | `apps/desktop/src/lib/components/inbox/GithubFocusOverlay.svelte` | no |
| 19 | 1004 | `apps/desktop/src/lib/views/apps/jira/JiraList.svelte` | no |
| 20 |  948 | `apps/desktop/src/lib/components/inbox/JiraDetailPane.svelte` | no |

**10-target total:** 23 733 lines. **Top-20 total:** ~33 419 lines.

## Phase target size goal

Spec § Success criteria: post-refactor, no source file in the touched feature folders exceeds **~1000 lines** (smell-threshold, not hard limit). Exceeding it requires a one-line justification comment in the file header.

## Verification baseline

### `npm run check` (svelte-check + tsconfig)

**Command (used):** `cd apps/desktop && npm run check`
**Command in plan (`bun run check`):** unavailable — `bun` not installed on developer machine. Substitution documented; same package.json script body runs (`svelte-kit sync && svelte-check --tsconfig ./tsconfig.json`).

**Exit code:** `0` (green).

**Summary:** `940 FILES 0 ERRORS 16 WARNINGS 12 FILES_WITH_PROBLEMS`.

**Pre-existing warnings (16):** allowed; phase verifiers diff against this count. New warnings = regression. Categories at baseline:

- 1 × `css_unused_selector` (`SddCard.svelte`)
- 7 × a11y noninteractive / role / focus warnings (`ReviewPane.svelte`, `GithubFocusOverlay.svelte`, `UpdateNotesPane.svelte`, `CanvasSurface.svelte`, `HomeApp.svelte`)
- 2 × `a11y_mouse_events_have_key_events` (`Rail.svelte`)
- 2 × `line-clamp` standard-property compat (`ChatHeader.svelte`, `Composer.svelte`)
- 3 × `state_referenced_locally` (`GithubList.svelte`, `JiraList.svelte`, `SentryList.svelte`)
- 1 × `a11y_no_static_element_interactions` (`HomeApp.svelte`)

### `npm test` (vitest run)

**Command (used):** `cd apps/desktop && npm test` (= `vitest run`)
**Command in plan (`bun run test`):** unavailable; substitution as above.

**Exit code:** `1` (red) — **pre-existing breakage**.

**Summary:** `5 failed (5)` test files; `no tests` collected.

**Failure mode:** every test file fails to load with `ReferenceError: __vite_ssr_exportName__ is not defined`. Vitest 2.1 vs Vite 7 / rolldown-vite incompatibility. Affected files:

- `src/lib/usage.test.ts`
- `src/lib/services/fuzzyMatch.test.ts`
- `src/lib/services/sessionCwd.test.ts`
- `src/lib/services/slashCommands.test.ts`
- `src/lib/state/tokenAge.test.ts`

**Impact on the refactor wave:** the test harness was already broken before phase 1. Phase verifiers will NOT use `npm test` as a gate until a follow-up wave repairs the harness. Smoke checklist (manual) is the floor.

**Deviation logged.** Spec § Success criteria implicitly assumed tests run; they don't. The verifier acceptance for each phase should use only `npm run check` (typecheck) + manual smoke until the harness is fixed.
