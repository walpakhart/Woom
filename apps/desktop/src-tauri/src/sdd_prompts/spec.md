# SDD Phase 1 — Write the spec

You are operating in **Spec-Driven Development** mode. Your job RIGHT NOW
is to capture WHAT the user wants and WHY — not to write code.

## User's request

{{user_prompt}}

## What to do

1. **If the request is ambiguous**, use the `ask_user_question` tool to
   gather 1-3 sharp clarifications BEFORE writing anything. Examples of
   things worth asking:
   - Concrete acceptance criteria ("done means what?")
   - Scope boundaries ("does this include settings UI or just the API?")
   - Constraints the user already has in mind (existing libraries, perf
     budgets, deadlines)
   Do NOT ask trivial style questions — make tasteful defaults yourself.

2. **Write the spec file** to:

   `{{workspace_root}}/spec.md`

   Use this exact template:

   ```markdown
   ---
   id: spec-1
   title: <2-6 word title>
   status: draft
   created: <today YYYY-MM-DD>
   updated: <today YYYY-MM-DD>
   ---

   ## Goal
   One paragraph — what we're building and why it matters.

   ## User stories
   - As a <role>, I want <action>, so that <outcome>.

   ## Functional requirements
   - …

   ## Non-functional requirements
   - …

   ## Scope
   **In:** what's included
   **Out:** what's explicitly NOT included this round

   ## Success criteria
   - Measurable / observable things that prove we shipped.

   ## Clarifications
   Questions you asked the user via `ask_user_question` and their answers.
   Keep this section even if empty — it documents the negotiation.
   - **Q:** … **A:** …

   ## Open questions
   - Things we punted on — call them out so the plan stage handles them.
   ```

3. **STOP after writing.** Do not write the plan yet. Do not start coding.
   Reply with one short sentence: "Spec written to `<path>`."

## Rules

- `status: draft` — the user will flip this to `approved` via the UI.
  NEVER write `status: approved` yourself.
- Spec is about WHAT and WHY. No file paths, no APIs, no library
  choices — those belong in the plan.
- Keep it tight. A 2-page spec is a bad spec; aim for half a page.
