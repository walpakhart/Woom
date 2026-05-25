# Manual smoke checklist — refactor wave 1

Source: `spec.md` § Success criteria. Run end-to-end in dev build after EVERY phase that touches its area. Use as the human floor; `npm run check` is the automated floor.

Baseline observation date: 2026-05-24. Each item lists what to expect at baseline so a regression is obvious.

---

## 1. Cross-session message integrity

- [ ] Open 3 chat sessions (Claude column). Send a message in session A, switch to B, send a message in B, switch back to A.
- **Expect:** Session A's transcript stays as it was — no bleed from B's message.
- **Baseline:** works green. The previous perf-patch broke this; current code uses list-replace via `.map()` in `sessions.svelte.ts` (see `red-lines.md` §1).

## 2. SDD card re-renders per tool-use

- [ ] Run a 3-phase SDD workflow. Watch the inline SDD card during a phase.
- **Expect:** Per tool_use event the card updates (action log row appears within ≤1s, status pill flips, progress text updates).
- **Baseline:** works green. `sdd:changed` listener in `sdd.svelte.ts:531` triggers array-replace on `sddState.workspaces`.

## 3. Streaming throughput in long chat

- [ ] In a session with 100+ prior messages, send a prompt that returns a 500-line response.
- **Expect:** Delta render is smooth; no visible jank, no scroll stutter.
- **Baseline:** works green. Hot path is `appendToLastAssistant` in `sessions.svelte.ts:1231`, called from `agentStream.ts:243` + `+page.svelte:3375`.

## 4. `propose_commit` round-trip

- [ ] Trigger an agent turn that produces `mcp__github__propose_commit`. Approve in the card.
- **Expect:** Commit hash returns in the SAME agent turn — agent continues immediately, no extra "continue" needed.
- **Baseline:** works green. Synchronous-tool semantics in `agentStream.ts:516`.

## 5. `propose_pr` round-trip

- [ ] After §4, trigger `mcp__github__propose_pr`. Approve.
- **Expect:** PR URL returned in same turn. Agent can chain follow-up tool calls (e.g. add labels).
- **Baseline:** works green. Same synchronous boundary as §4.

## 6. Slash-picker → send

- [ ] In Composer, type `/`. Pick a slash command from the picker. Send.
- **Expect:** Skill/slash expands silently (no visible expansion text in chat), agent receives the expanded prompt.
- **Baseline:** works green per recent commit `27203a1` (slash picker + silent skill expansion).

## 7. Paste-image attachment

- [ ] Paste an image into Composer (Cmd-V from clipboard). Send.
- **Expect:** Image appears as attachment chip; on send, image is part of the user message and visible in the chat bubble.
- **Baseline:** works green.

## 8. Canvas — shape + edge + save

- [ ] Open a canvas instance. Add a shape. Add an edge. Save.
- **Expect:** Shape + edge render correctly, persist after reload of the canvas solo.
- **Baseline:** works green. Mutations route through `canvasState.shapes` / `.edges` list-replace.

## 9. Editor — repo switch + file open

- [ ] Open editor instance. Click the popover to switch repo. Open a file from the file tree.
- **Expect:** Editor view updates; linked agent sessions follow the cwd switch automatically (per Woom layout-link rule).
- **Baseline:** works green.

## 10. Settings — section switching

- [ ] Open Settings solo. Switch each section (Accounts, MCP, Hooks, Appearance, etc.).
- **Expect:** Each section renders; form state persists when navigating away + returning within the same session.
- **Baseline:** works green.

## 11. Rail — second editor instance

- [ ] Click the editor rail icon's popover. Spawn a new editor instance. Switch between the two via popover.
- **Expect:** Both instances appear in the popover; switching swaps the open folder + tab strip; each has its own state.
- **Baseline:** works green.

## 12. Silent-prompt queue during stream

- [ ] While an agent turn is mid-stream in session X, trigger an SDD-pending state on the same session (approve a phase in the card).
- **Expect:** Approve click does NOT inject a visible user message; on stream completion the silent prompt fires automatically as the next agent turn.
- **Baseline:** works green. Queue helpers in `sdd.svelte.ts:464-470` (`setPendingSilent` / `popPendingSilent`); drain logic in `+page.svelte:3304`.
