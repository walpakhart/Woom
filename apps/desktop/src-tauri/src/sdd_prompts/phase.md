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

1. **Re-read** the phase file via
   `mcp__app__sdd_get_phase(id="{{workspace_id}}", phase={{phase_number}})`.
   The tool returns Goal, Tasks, Acceptance criteria, and the Verification
   commands in one shot — prefer it over manually reading the markdown
   file. (Falls back to reading the file directly if the MCP tool isn't
   available — legacy workspaces without phase 6 still work.)

2. **Implement each task in order.** Touch only files the phase says you
   may touch.

3. **Verification runs automatically.** The orchestrator's verifier
   executes the phase's `acceptance` checks as soon as the phase is
   closed in step 4. The `## Verification` section in the phase markdown
   is human-readable context, NOT the source of truth. You do NOT need
   to run those commands yourself.

   On verifier failure the workspace card surfaces 3 actions to the user
   (Retry / Edit & retry / Skip with reason). **The user decides** —
   DO NOT call `mcp__app__sdd_retry_phase` / `_skip_phase` /
   `_advance_phase` yourself unless `sdd_autonomy=semi-auto` is in your
   context. In default mode the user gates phase transitions.

4. **Close the phase** with:

   ```
   mcp__app__sdd_log_phase_done(
     id = "{{workspace_id}}",
     phase = {{phase_number}},
     summary = "<2-3 sentence summary of observable changes>",
     files_changed = ["path/touched/A.ts", "path/touched/B.ts"]
   )
   ```

   The orchestrator handles the rest — writes the result.md, runs the
   verifier, commits to git. Do NOT manually edit the phase frontmatter
   when `sdd_log_phase_done` is available.

   **Legacy fallback** (workspaces without phase 6 tools, or if the MCP
   call returns method-not-found): write the result file at
   `{{workspace_root}}/results/{{phase_slug}}-result.md` with the
   template below, then flip the phase frontmatter to `status: done`
   manually. The verifier picks up the status flip the same way.

   Result template:

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

5. Reply with ONE sentence: "Phase {{phase_number}} done." Do NOT start
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
