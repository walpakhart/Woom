# Red-line grep map — refactor wave 1

Baseline: 2026-05-24. Each invariant lists a runnable grep + the file:line snapshot at baseline. Future phases re-run the grep and verify each hit either still resolves OR was deliberately migrated (call it out in the phase's deviations section).

Run all greps from repo root (`/Users/nikolay-khartanovich/Repos/pers/woom`).

---

## 1. List-replace mutations on `sessionsState.list`

**Invariant.** Any mutation feeding a `.find()`-based `$derived` MUST use array-replace (`list = [...]`) or list-replace via `.map()`. Field-level proxy mutation is banned for these code paths.

```bash
grep -nE 'sessionsState\.list = ' apps/desktop/src -r
```

**Baseline:** 21 hits, all in `apps/desktop/src/lib/state/sessions.svelte.ts`. Sample:

```
src/lib/state/sessions.svelte.ts:429:      sessionsState.list = sessions;
src/lib/state/sessions.svelte.ts:586:  sessionsState.list = [
src/lib/state/sessions.svelte.ts:637:  sessionsState.list = rest;
src/lib/state/sessions.svelte.ts:752:  sessionsState.list = sessionsState.list.map(...)
src/lib/state/sessions.svelte.ts:773:  sessionsState.list = sessionsState.list.map(...)
src/lib/state/sessions.svelte.ts:785:  sessionsState.list = sessionsState.list.map(...)
src/lib/state/sessions.svelte.ts:796:  sessionsState.list = sessionsState.list.map(...)
src/lib/state/sessions.svelte.ts:1080: sessionsState.list = sessionsState.list.map(...)
... (21 total — all in sessions.svelte.ts)
```

**Watch:** phase 7 (`sessions-store-split`) must route every mutation through the `replaceSession(id, next)` primitive described in the plan. Any `sessionsState.list[i].messages.push(...)` or similar field-level write that bypasses the list-replace = regression.

---

## 2. `.find()`-based derived chains

**Invariant.** Components that read state via `list.find((x) => x.id === id)` rely on list-replace to re-fire. Document where they live so phase 7-8 know not to break them.

```bash
grep -nE '\.find\(\(?[a-z]+\)? ?=> ?[a-z]+\.id' apps/desktop/src -r
```

**Baseline:** ~30 hits across `sessions.svelte.ts`, `canvas.svelte.ts`, `bgTasks.svelte.ts`, `agentStream.ts`, `+page.svelte`, `inbox-github.ts`. Sample:

```
src/lib/stream/agentStream.ts:509: const sess = sessionsState.list.find((s) => s.id === sessionId);
src/lib/stream/agentStream.ts:973: const s = sessionsState.list.find((x) => x.id === sessionId);
src/lib/state/sessions.svelte.ts:431: const active = sessions.find((s) => s.id === sessionsState.activeClaudeId);
src/lib/state/sessions.svelte.ts:625: const doomed = sessionsState.list.find((s) => s.id === id);
src/lib/state/canvas.svelte.ts:762: const shape = c.shapes.find((s) => s.id === shapeId);
src/lib/state/canvas.svelte.ts:867: const edge = c.edges.find((e) => e.id === edgeId);
src/lib/state/canvas.svelte.ts:898: const src = c.shapes.find((s) => s.id === id);
src/lib/state/bgTasks.svelte.ts:197: bgTasksState.tasks.find((x) => x.id === token)
src/lib/state/inbox-github.ts:218: const item = list.find((i) => i.id === id);
```

---

## 3. Streaming hot-path symbols

**Invariant.** Streaming throughput stays identical. The exported sessions-store hot path is `appendToLastAssistant`; the route-shell wrapper is `appendAssistantDelta`. Plan's spec mentions `appendToLastAssistant` — that's correct for the store export. The wrapper name appeared in the plan-pass output; both are real, both are tracked here.

```bash
grep -nE 'appendToLastAssistant\(' apps/desktop/src -r
grep -nE 'appendAssistantDelta\(' apps/desktop/src -r
```

**Baseline (export):** 3 hits.

```
src/lib/state/sessions.svelte.ts:1231: export function appendToLastAssistant(sessionId: string, delta: string) {
src/lib/stream/agentStream.ts:243:    appendToLastAssistant(sessionId, delta);
src/routes/+page.svelte:3375:    appendToLastAssistant(sessionId, delta);
```

**Baseline (wrapper):** 1 hit.

```
src/routes/+page.svelte:3374:  function appendAssistantDelta(sessionId: string, delta: string) {
```

**Watch:** phase 6 (`agent-ui-split`) must keep `appendToLastAssistant` body byte-identical (or only refactor allocations into provably-equivalent form). Phase 9 (`page-shell-split`) may move the `appendAssistantDelta` wrapper — fine — but the call chain `agentStream → wrapper → appendToLastAssistant` must remain unbroken.

---

## 4. SDD event channel

**Invariant.** `sdd:changed` Tauri events fire per tool-use during a phase; the listener triggers workspace refresh + card re-render via array-replace.

```bash
grep -nE 'sdd:changed|sdd_changed' apps/desktop/src -r
```

**Baseline:** 8 hits — listener in `sdd.svelte.ts:531`, dispatchers/comments scattered.

```
src/lib/state/sdd.svelte.ts:531: sddState.globalUnlisten = await listen<string>('sdd:changed', async (evt) => {
src/lib/state/sdd.svelte.ts:11:  *   - listener for `sdd:changed:*` Tauri events
src/lib/state/sdd.svelte.ts:325: * agent-rewrite-detection branch in the `sdd:changed` listener
src/lib/state/sdd.svelte.ts:488: * upcoming `sdd:changed` event is ours, not the agent's.
src/lib/state/sdd.svelte.ts:504: * caller's `saveSddBody` will also emit `sdd:changed`
src/lib/components/agent/SddCard.svelte:513: * back. The watcher's `sdd:changed` event will refresh the card.
src/routes/+page.svelte:3206: * Fire-and-forget; the resulting `sdd:changed:<id>` event will
```

---

## 5. `propose_*` action-card dispatch

**Invariant.** Action cards are SYNCHRONOUS — the agent's turn stays under tool-response control, no UI race that resolves the card before the agent's next turn arrives.

```bash
grep -nE 'propose_(commit|pr|bash)' apps/desktop/src -r
```

**Baseline:** ~11 hits across 5 files. Dispatch sites:

```
src/lib/stream/agentStream.ts:516: case 'mcp__github__propose_commit':
src/lib/stream/agentStream.ts:533: case 'mcp__github__propose_pr':
src/lib/stream/agentStream.ts:567: case 'mcp__github__propose_bash':
src/lib/stream/agentStream.ts:568: case 'mcp__app__propose_bash':
src/lib/format.ts:113: if (name === 'mcp__app__propose_bash' || name === 'mcp__github__propose_bash')
src/lib/format.ts:121: if (name === 'mcp__github__propose_commit')
src/lib/format.ts:125: if (name === 'mcp__github__propose_pr')
src/routes/+page.svelte:4462:  // (e.g. propose_pr after the commit lands) ...
```

**Watch:** phase 9 (`page-shell-split`) extracts the MCP-dispatch hook from `+page.svelte` into `lib/services/mcpDispatch.ts`. The `await` boundary on tool-response MUST be preserved — manual smoke item §4 (propose_commit round-trip) tests this end-to-end.

---

## 6. Silent-prompt queue

**Invariant.** SDD-pending + Claude-bg-done both enqueue to the same `pendingSilentBySession` slot; ordering preserved.

```bash
grep -nE 'pendingSilent|setPendingSilent|popPendingSilent' apps/desktop/src -r
```

**Baseline:** 8 hits across 2 files.

```
src/lib/state/sdd.svelte.ts:342: pendingSilentBySession: Record<string, string>;
src/lib/state/sdd.svelte.ts:408: pendingSilentBySession: {},
src/lib/state/sdd.svelte.ts:464: export function setPendingSilent(sessionId: string, prompt: string): void {
src/lib/state/sdd.svelte.ts:465: sddState.pendingSilentBySession[sessionId] = prompt;
src/lib/state/sdd.svelte.ts:467: export function popPendingSilent(sessionId: string): string | null {
src/lib/state/sdd.svelte.ts:468: const v = sddState.pendingSilentBySession[sessionId];
src/lib/state/sdd.svelte.ts:470: delete sddState.pendingSilentBySession[sessionId];
src/routes/+page.svelte:2765: const { setPendingSilent } = await import('$lib/state/sdd.svelte');
src/routes/+page.svelte:2766: setPendingSilent(s.id, promptText);
src/routes/+page.svelte:3304: const { popPendingSilent } = await import('$lib/state/sdd.svelte');
src/routes/+page.svelte:3305: const deferred = popPendingSilent(id);
```

**Watch:** phase 8 (`sdd-pair-split`) keeps queue helpers in `sdd/store.ts`. Consumers in `+page.svelte` keep reading via the same export names.
