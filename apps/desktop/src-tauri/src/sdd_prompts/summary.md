# SDD final — Write the workflow summary

All phases for this SDD workspace have completed. Your job RIGHT NOW
is to write the user-facing wrap-up: what shipped, what to test, what
to follow up on.

## Context bundle

- **Spec:** `{{workspace_root}}/spec.md`
- **Plan:** `{{workspace_root}}/plan.md`
- **Phase results** (read all of them, this is your source of truth):
  `{{workspace_root}}/results/*.md`

## What to do

1. **Write a digest file** to:

   `{{workspace_root}}/SUMMARY.md`

   Use this exact template:

   ```markdown
   ---
   id: summary-1
   spec: spec-1
   created: <today YYYY-MM-DD>
   ---

   ## What shipped
   2-4 sentences — the observable change the user can see/use NOW.

   ## Files changed
   - path/to/A.ts
   - path/to/B.svelte

   ## Trade-offs accepted
   - <short bullet | "none">

   ## Follow-up
   - <things deferred / known limitations / next-iteration candidates | "none">

   ## How to test
   - Concrete clicks/commands the user should run to verify.
   ```

2. **Reply visibly with a 3-5 sentence digest.** This is the user's
   first signal that the workflow is done — make it count. Lead with
   what shipped (concrete observable change), then mention files,
   then any caveat. No "Phase N done" prefix; this is the END.

   Example shape:
   > Shipped flat tool-call rows in the chat — borders/bg gone, single
   > text line per call, expand-on-click for output. Touched
   > `ChatThread.svelte` only (~140 CSS lines reshaped). Error-state
   > styling deferred (no signal source today). Smoke: scroll a chat
   > with multi-tool turns; reload Woom if old assistant turns still
   > render boxed.

## Rules

- NEVER touch `spec.md`, `plan.md`, or any phase file. Their state is
  the source you READ; don't write back.
- Read EVERY `results/*.md` file before drafting. The digest must
  reflect actual shipped work, not the plan's intent.
- Keep SUMMARY.md tight — half a page is the target. The visible reply
  is even tighter (3-5 sentences).
- Don't repeat the spec/plan verbatim. The user already saw those.
  Write what they DIDN'T see: outcomes, file paths, anything that
  surprised you.
