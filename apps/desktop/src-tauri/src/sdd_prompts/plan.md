# SDD Phase 2 — Write the plan

The spec at `{{workspace_root}}/spec.md` is now `status: approved`. Your
job is to design the TECHNICAL approach and break it into 2-6 phases
that can be executed sequentially.

## What to do

1. **Re-read the spec.** Internalize Goal + Success criteria — every
   phase should pull weight toward them.

2. **Explore the codebase** as needed (Read, Glob, Grep) to ground your
   plan in reality — file paths, existing patterns, naming conventions.

3. **Write the plan** to:

   `{{workspace_root}}/plan.md`

   Template:

   ```markdown
   ---
   id: plan-1
   spec: spec-1
   status: draft
   total_phases: <N>
   updated: <today>
   verification:
     typecheck: <command or empty>
     test: <command or empty>
     lint: <command or empty>
   ---

   ## Approach
   2-4 paragraphs — the WHY behind the architecture. Trade-offs, what we
   considered and ruled out, dependencies between layers.

   ## Phase overview
   | # | Phase | Depends on | Goal |
   |---|-------|-----------|------|
   | 1 | foundation | — | … |
   | 2 | api | 1 | … |
   | 3 | ui | 2 | … |

   ## Risks
   - Risk → mitigation.
   ```

4. **Write one phase file per phase** to:

   `{{workspace_root}}/phases/NN-slug.md`   (NN = zero-padded number)

   Each phase file template:

   ```markdown
   ---
   phase: <N>
   title: <2-4 word title>
   depends_on: [<list of phase numbers>]
   status: pending
   tasks_total: <N>
   tasks_completed: 0
   ---

   ## Goal
   One paragraph — what this phase delivers.

   ## Context
   - Files to touch: …
   - Patterns to follow: …
   - Prior phase outputs you'll build on: …

   ## Tasks
   1. <task title>
      - **Files:** path/to/file.ts
      - **Acceptance:** observable thing that proves it works
   2. …

   ## Verification
   Commands to run after this phase to confirm it works:
   ```bash
   …
   ```

   ## Done when
   - [ ] All tasks complete
   - [ ] Verification passes
   ```

5. **Write `plan.json`** alongside `plan.md` — a machine-readable
   mirror of the phase list. The orchestrator uses this as the source
   of truth for structural changes (insert / reorder / delete) and for
   per-phase acceptance criteria in later phases.

   `{{workspace_root}}/plan.json`:

   ```json
   {
     "version": 1,
     "phases": [
       {
         "number": 1,
         "slug": "01-foundation",
         "title": "Foundation",
         "depends_on": [],
         "complexity": "medium",
         "acceptance": []
       }
     ]
   }
   ```

   Rules for `plan.json`:
   - `slug` MUST match the phase markdown filename without `.md`
     (`phases/01-foundation.md` → `"01-foundation"`).
   - `complexity` is one of `"low" | "medium" | "high"` (best-guess —
     used by the UI to size phase cards).
   - **`acceptance` is REQUIRED**. Each phase needs at least 2 checks
     so the verifier can confirm the phase delivered. Three shapes:

     ```jsonc
     // Run a shell command. Pass when exit matches `expect_exit` AND
     // (if set) `stdout_match` substring is in stdout/stderr.
     { "type": "shell",
       "cmd": "cargo test ... my_module::",
       "expect_exit": 0,
       "stdout_match": "test result: ok",     // optional
       "timeout_ms": 120000 }                 // optional, default 120s

     // All listed paths must exist (workspace-relative).
     { "type": "file_exists",
       "paths": ["src/foo.ts", "tests/foo.test.ts"] }

     // User-eyeballed; verifier records it as `manual_unmarked` and
     // the SddCard surfaces a Mark-passed/Mark-failed pair.
     { "type": "manual",
       "description": "Verify the new icon renders on a HiDPI display." }
     ```

     Pick at minimum ONE shell-or-file_exists check + (where
     applicable) one manual. Frontend-heavy phases: `pnpm --filter
     desktop check` + a manual smoke. Backend-heavy: a focused
     `cargo test ... <module>::` + `file_exists` for new files.
   - 2-space indent; trailing newline.

6. **STOP after writing.** Do not start executing phases yet. Reply with
   one short sentence: "Plan + N phases written to `<paths>`."

## Rules

- `status: draft` on plan.md — user approves via UI.
- Phases must be **sequential** (depends_on of phase N is always
  `[N-1]` for v1 — no parallel waves yet).
- **Phase count matches scope.** For a tiny feature, 2-3 phases is
  right. For an ambitious project (game engine, multi-service
  refactor, end-to-end product feature), 6-10 phases with detailed
  per-phase tasks is normal — don't artificially compress. The Woom
  UI renders plans + phases in a fullscreen lightbox, so depth is
  readable.
- **Per-phase depth.** Each phase file should be RICH: spell out
  every file you'll touch, the exact functions/classes/components,
  data shapes, API contracts, migration steps, fallbacks, rollback
  story. A phase that just says "build the API" is too vague — it
  should have ~5-15 numbered tasks, each with `Files`, `Acceptance`,
  and where useful a code-fenced snippet showing the shape.
- Plan-level **Approach** section should be a real design doc, not
  a one-paragraph hand-wave: cover the architecture decisions, the
  alternatives you ruled out, data flow, threading / async model,
  failure modes, security posture if relevant. Use sub-headings
  (`### Storage`, `### Rendering`, `### Networking`) when the
  surface area warrants it.
- Use the exact filename pattern `phases/NN-slug.md` so the orchestrator
  can parse phase number from the filename.
