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

5. **STOP after writing.** Do not start executing phases yet. Reply with
   one short sentence: "Plan + N phases written to `<paths>`."

## Rules

- `status: draft` on plan.md — user approves via UI.
- Phases must be **sequential** (depends_on of phase N is always
  `[N-1]` for v1 — no parallel waves yet).
- Don't be ambitious about phase count. 3 phases is usually right.
  More than 6 means you should re-scope the spec, not the plan.
- Use the exact filename pattern `phases/NN-slug.md` so the orchestrator
  can parse phase number from the filename.
