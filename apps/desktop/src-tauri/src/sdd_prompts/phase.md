# SDD Phase 3+ — Execute phase {{phase_number}}

Plan at `{{workspace_root}}/plan.md` is `status: approved`. Now execute
**phase {{phase_number}}** end-to-end.

## Context bundle

- **Spec:** `{{workspace_root}}/spec.md`
- **Plan:** `{{workspace_root}}/plan.md`
- **This phase:** `{{workspace_root}}/phases/{{phase_file}}`
- **Prior phase results** (read these first to inherit context):
  `{{workspace_root}}/results/*.md`

## What to do

1. **Re-read** the phase file end-to-end. Internalize Goal, Tasks,
   Acceptance criteria, and the Verification commands.

2. **Set phase status to `running`** by updating the phase file's
   frontmatter `status: running`. Leave the body untouched.

3. **Implement each task in order.** Touch only files the phase says you
   may touch. After each task, update `tasks_completed` in the
   frontmatter.

4. **Run the verification commands** listed in the phase file. If any
   command fails:
   - Read the failure output, fix the root cause, re-run.
   - You have **{{retries_max}} retries** of the full verification loop.
     If still failing, set `status: failed` in the phase frontmatter and
     STOP. Reply with a short failure summary referencing the verify
     command output. The user will decide whether to skip / retry / abort.

5. **Write the result summary** to:

   `{{workspace_root}}/results/{{phase_slug}}-result.md`

   Template:

   ```markdown
   ---
   phase: {{phase_number}}
   title: <copied from phase>
   status: completed
   started_at: <ISO>
   completed_at: <ISO>
   files_changed:
     - path/touched/A.ts
     - path/touched/B.ts
   ---

   ## Summary
   2-3 sentences — what changed, observable.

   ## Deviations from plan
   - <empty | bullet list of anything done differently and why>

   ## Notes for next phases
   - <hints / pitfalls discovered>
   ```

6. **Flip phase status to `done`** in the frontmatter of the phase file.

7. Reply with ONE sentence: "Phase {{phase_number}} done." Do NOT start
   the next phase yourself — the orchestrator advances on its own when
   the previous phase's `status: done` is observed.

## Rules

- NEVER touch `spec.md` or `plan.md` during phase execution. Their
  approved state is sacred — if you discovered a spec/plan-level issue,
  call it out in the result file's "Deviations" section and STOP.
- NEVER touch files outside the workspace AND outside what the phase
  file lists. The plan explicitly enumerated the file surface.
- Use `ask_user_question` ONLY if the phase truly can't proceed without
  a binary choice from the human. Avoid trivial questions — make
  tasteful defaults.
- For UI/frontend changes: actually run the build / typecheck command
  from `verification:` (if listed in the plan). For backend: actually
  run the test command. Don't just claim "looks good".
