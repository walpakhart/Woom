# SDD Phase {{phase_number}} — Implement pass

Plan-pass completed; your plan is now persisted at
`{{workspace_root}}/phases/{{phase_slug}}/plan.md`. You are running in
**three-call execution mode**, **implement pass**: execute the plan
end-to-end, write code, run verification commands.

## Context bundle

- **Spec:** `{{workspace_root}}/spec.md`
- **Plan:** `{{workspace_root}}/plan.md`
- **This phase:** `{{workspace_root}}/phases/{{phase_file}}`
- **YOUR plan (from plan pass):** `{{workspace_root}}/phases/{{phase_slug}}/plan.md`
- **Prior phase results:** `{{workspace_root}}/results/*.md`

## What to do

1. **Re-read your plan** at
   `{{workspace_root}}/phases/{{phase_slug}}/plan.md`. The plan
   represents the contract you negotiated with the orchestrator —
   stick to it. Deviations are allowed when the plan was wrong, but
   you must call them out at close time.

2. **Re-read the phase file** via
   `mcp__app__sdd_get_phase(id="{{workspace_id}}", phase={{phase_number}})`.

3. **Execute each task in order.** Touch only files the phase says
   you may touch.

4. **Run the build / typecheck / lint commands** from the phase's
   `## Verification` section yourself BEFORE closing the pass. The
   verify-pass agent re-reads them but you should land green; a
   broken build means the implement pass didn't finish. For
   destructive operations propose them via `propose_bash` cards,
   not inline.

5. **Close the implement pass** with:

   ```
   mcp__app__sdd_complete_phase_implement(
     id = "{{workspace_id}}",
     phase = {{phase_number}},
     summary = "<2-3 sentence summary of observable changes>",
     files_changed = ["path/touched/A.ts", "path/touched/B.ts"]
   )
   ```

   This advances the substep checkpoint from `Implement` →
   `Verify`. The orchestrator fires the verify-pass prompt on the
   next agent turn — the verify pass writes `verify.json` + flips
   phase `done` / `failed` based on deviations. Do NOT manually
   edit the phase frontmatter or write `result.md` yourself in
   three-call mode.

6. Reply with ONE sentence: "Phase {{phase_number}} implement done."
   Do NOT start the verify pass yourself — the orchestrator queues
   it once the substep transition lands on disk.

## Rules

- NEVER touch `spec.md` or `plan.md` (workspace root) during phase
  execution. Their approved state is sacred — if you discovered a
  spec/plan-level issue, call it out in the result file's
  "Deviations" section and STOP.
- NEVER touch files outside the workspace AND outside what your
  plan listed. The plan explicitly enumerated the file surface; the
  verify pass will compare actual changes against that list.
- Use `ask_user_question` ONLY if the phase truly can't proceed
  without a binary choice from the human. Avoid trivial questions —
  make tasteful defaults.
- For UI/frontend changes: actually run the build / typecheck
  command from `verification:` (if listed in the plan). For
  backend: actually run the test command. Don't just claim "looks
  good".
- For destructive shell operations (`rm -rf`, `git push --force`,
  migrations, deploys): propose via `mcp__app__propose_bash` or
  `mcp__github__propose_*` cards — never run them inline.
