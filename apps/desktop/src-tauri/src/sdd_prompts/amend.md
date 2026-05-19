# SDD — Amend / correct in-flight workspace

The user is asking you to AMEND the existing SDD workspace at
`{{workspace_root}}` — NOT to write a fresh spec, NOT to discard
work-in-progress. Treat their change as a delta on the current
artifacts.

## Workspace state

- **Current stage:** `{{stage_kind}}`
- **Workspace root:** `{{workspace_root}}`
- **Original user prompt:** {{user_prompt}}

## User's correction

> {{user_change}}

## How to apply it

1. **Read the relevant artifact first.** Depending on the current
   stage, the user's change is usually a delta on:
   - `spec_ready` / `drafting`  → `{{workspace_root}}/spec.md`
   - `plan_ready` / `planning`  → `{{workspace_root}}/plan.md` and
     all `phases/NN-*.md` files
   - `phase_running` / `phase_done` → the active phase's
     `phases/NN-*.md` AND potentially the plan if the change
     affects downstream phases
   - `complete` → `SUMMARY.md` + any phase result the user is
     correcting

2. **Use `Edit` / `MultiEdit` to patch the file.** Do NOT use
   `Write` to overwrite — that loses unrelated content and the
   YAML frontmatter. Preserve `status:` field and other metadata.

3. **Cascade changes** when the correction implies them:
   - Spec change that drops a feature → update plan's Phase
     overview + the affected phase files. Set those phases'
     `status:` back to `pending` if they were `done`.
   - Plan-level architecture change → patch every affected phase
     file; explain in the plan's Approach section.
   - Phase-task addition → bump `tasks_total` in that phase's
     frontmatter.

4. **Do not create new workspace files** under a different
   workspace id. If you genuinely need new phase files (because
   the user's correction adds a phase), create them under the
   SAME `{{workspace_root}}/phases/` with the next available
   number.

5. **Do not flip `status: approved` yourself.** The user controls
   approval via the UI. If your edit invalidates a previously-
   approved doc, flip it back to `status: draft` so the user sees
   the re-review affordance.

6. **Stop after writing.** Reply with one short line per file
   touched: "Amended `<path>`: <one-sentence reason>". Do NOT
   start executing phases or write a SUMMARY.

## Rules

- This is an in-place EDIT pass. The user has already invested in
  the current spec / plan / phases; respect that investment by
  preserving structure + frontmatter, patching only what the
  correction requires.
- If the correction is ambiguous about WHICH artifact to touch,
  use `ask_user_question` (kind=single) to disambiguate before
  editing. Don't guess + thrash multiple files.
- If the correction conflicts with `status: approved` content the
  user already signed off on, ask for explicit confirmation via
  `ask_user_question` (kind=confirm) before invalidating it.
