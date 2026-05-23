# SDD Phase {{phase_number}} — Verify pass

Implement pass completed; your changes are committed to the working
tree (or staged for review). You are running in **three-call
execution mode**, **verify pass**: review what you implemented against
the phase spec and produce a structured verdict.

## Context bundle

- **Spec:** `{{workspace_root}}/spec.md`
- **Plan:** `{{workspace_root}}/plan.md`
- **This phase:** `{{workspace_root}}/phases/{{phase_file}}`
- **Your implementation plan:** `{{workspace_root}}/phases/{{phase_slug}}/plan.md`
- **Your implement-pass result:** `{{workspace_root}}/results/{{phase_slug}}-result.md`

## What to do

1. **Re-read the spec + phase file + your plan.** You're checking
   your own work against the contract; load both sides of the
   comparison.

2. **Inspect the actual changes.** Use read-only tools (Read, Glob,
   Grep, `git diff` via Bash). Compare what's on disk now against
   what the phase tasks said you'd do and what your plan said you'd
   touch.

3. **Run the verification commands** listed in the phase's
   `## Verification` block if they're cheap (typecheck, focused
   tests). Skip ones that take >60s or have side effects (deploys,
   migrations). The orchestrator's acceptance verifier runs the
   `acceptance: []` checks separately as a hard gate; your job is
   the soft self-review.

4. **Respond with ONLY a JSON object matching this schema. No
   markdown fences, no prose around it.**

   ```json
   {
     "summary": "string",
     "files_changed": ["string"],
     "task_compliance": ["string"],
     "deviations": ["string"],
     "notes": "string"
   }
   ```

   Field semantics:

   - **`summary`** — 1-3 sentences. What the phase delivered,
     observable. Goes verbatim into phase frontmatter `summary` if
     non-empty.
   - **`files_changed`** — repo-relative paths you actually touched
     (use `git diff --name-only HEAD` as the source of truth, not
     your memory of the implement pass).
   - **`task_compliance`** — one entry per phase task, restating
     the task title + verdict ("Task 1: schema extended ✓").
   - **`deviations`** — bullet points describing anything you did
     differently from the phase tasks or your plan. **EMPTY ARRAY
     means the phase is clean.** A non-empty array flips the phase
     to `failed { trigger: "verify_failed" }`, so be honest — if
     you skipped a sub-task or used a different file path than
     planned, say so. The user reviews this list and decides
     whether to retry / edit / skip the phase.
   - **`notes`** — anything the next phase should know (hints,
     pitfalls, dependencies you noticed). Empty string is fine.

5. **Close the verify pass** with:

   ```
   mcp__app__sdd_save_phase_verify(
     id = "{{workspace_id}}",
     phase = {{phase_number}},
     raw_json = "<your JSON verdict verbatim>"
   )
   ```

   This persists your JSON to `phases/{{phase_slug}}/verify.json`,
   auto-fills phase frontmatter `summary`, flips phase to
   `done` (no deviations) or `failed { trigger: verify_failed }`
   (deviations present), and clears the substep checkpoint.

6. Reply with ONE sentence: "Phase {{phase_number}} verify recorded."

## Rules

- **READ-ONLY pass for everything EXCEPT close-out.** Allowed
  tools: Read, Glob, Grep, `mcp__app__sdd_get_phase`,
  `mcp__app__sdd_get`, `git diff` / `git status` / `git log` via
  Bash, AND the single `mcp__app__sdd_save_phase_verify`
  close-out in step 5. FORBIDDEN tools: Edit, Write, NotebookEdit,
  Bash-that-mutates, `mcp__app__propose_*`,
  `mcp__app__sdd_log_phase_done`,
  `mcp__app__sdd_save_phase_plan`,
  `mcp__app__sdd_approve_phase_plan`,
  `mcp__app__sdd_discard_phase_plan`,
  `mcp__app__sdd_*_phase` (retry/skip/advance).
- **Strict JSON only.** No code fences, no leading prose, no
  trailing explanation. If you can't determine a field, use an
  empty string / empty array — the orchestrator's parser has a
  fallback for malformed JSON but emits a `verify_parse_fail`
  trigger which the user has to manually clear.
- Time budget is short — keep the pass to ≤10 tool calls and ≤60
  seconds wall-clock. The orchestrator times out the call past
  that.
- Do NOT re-implement anything you find wrong. Surface it in
  `deviations` and let the user decide. The Retry / Edit & retry
  cards handle the recovery path.
