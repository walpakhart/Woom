# SDD Phase {{phase_number}} — Plan pass

Plan at `{{workspace_root}}/plan.md` is `status: approved`. You are
running in **three-call execution mode**, **plan pass**: produce a
detailed implementation plan for **phase {{phase_number}}** as your
final assistant message. **You do not edit any files in this pass.**

## Context bundle

- **Spec:** `{{workspace_root}}/spec.md`
- **Plan:** `{{workspace_root}}/plan.md`
- **This phase:** `{{workspace_root}}/phases/{{phase_file}}`
- **Prior phase results:** `{{workspace_root}}/results/*.md`

## What to do

1. **Read** the phase file via
   `mcp__app__sdd_get_phase(id="{{workspace_id}}", phase={{phase_number}})`.
   The tool returns Goal, Tasks, Acceptance criteria + Verification
   commands in one shot.

2. **Read** the spec + plan + prior phase result files (use Read /
   Glob / Grep). Build a complete mental model of what's already
   landed in earlier phases so your plan inherits, not duplicates,
   their decisions.

3. **Survey the target codebase.** Use read-only tools to map every
   file the phase says it will touch, every related symbol, every
   adjacent test. Quote function names + line numbers in your plan
   so the implement-pass agent can jump directly to them.

4. **Write your plan as the FINAL assistant message.** Keep it to
   ≤80 lines of markdown. Structure:

   ```markdown
   ## Plan for phase {{phase_number}}

   ### Approach
   2-3 paragraphs — strategy + ordering rationale + key trade-offs.

   ### Step-by-step
   1. Task 1 — touches `path/to/file.rs:lines`. Use pattern X.
   2. Task 2 — …

   ### Files to touch
   - `apps/.../file.rs` — new fn `foo()`, modify `bar()` at line N
   - `apps/.../file.ts` — extend `Type Z`, add wrapper fn

   ### Tests to add / update
   - `path/to/test.rs::module::test_name`

   ### Risks
   - Risk → mitigation.
   ```

5. **Close the plan pass** with:

   ```
   mcp__app__sdd_save_phase_plan(
     id = "{{workspace_id}}",
     phase = {{phase_number}},
     body = "<your plan markdown verbatim>"
   )
   ```

   This persists your plan to `phases/{{phase_slug}}/plan.md` and
   advances `substep-state.json` to `Implement` (or `PlanReview`
   when `plan_gate=true`). Without this call the orchestrator
   leaves the workspace in `phase_planning` forever — the plan you
   wrote in the chat is lost on next refresh.

6. Reply with ONE sentence: "Phase {{phase_number}} plan recorded."
   Do NOT call any other mutating MCP tool — implement pass fires
   on its own once the substep transitions.

## Rules

- **READ-ONLY pass for everything EXCEPT close-out.** Allowed
  tools: Read, Glob, Grep, `mcp__app__sdd_get_phase`,
  `mcp__app__sdd_get`, web fetches, AND the single
  `mcp__app__sdd_save_phase_plan` close-out in step 5. FORBIDDEN
  tools: Edit, Write, NotebookEdit, Bash-that-mutates,
  `mcp__app__propose_*`, `mcp__app__sdd_log_phase_done`,
  `mcp__app__sdd_save_phase_verify`,
  `mcp__app__sdd_approve_phase_plan`,
  `mcp__app__sdd_discard_phase_plan`,
  `mcp__app__sdd_*_phase` (retry/skip/advance).
- Do not propose IaC actions, commit anything, or open PRs.
- If you discover the phase is ambiguous or impossible to plan
  without a binary-choice answer from the user, use
  `ask_user_question`. Otherwise make tasteful defaults and put the
  open questions at the bottom of your plan in a **Questions for
  implement pass** section.
- Read-only sentinel: the orchestrator captures `git status` before
  and after this pass; any disk mutation flips the phase to
  `failed { trigger: "plan_mutated_disk" }`.
- Length matches scope. A 1-line bug-fix phase gets ~10 lines of
  plan; a refactor across 10 files gets the full 80.
