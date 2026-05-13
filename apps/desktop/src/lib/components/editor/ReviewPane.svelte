<script lang="ts">
  /* ReviewPane — sidebar tab that turns the agents' streamed edits into
     a reviewable workspace. Same idea as VS Code's "Source Control"
     pane, but the unit isn't a git hunk — it's an agent edit event
     (`MessageEvent { kind: 'edit' }`) that's been written to disk and
     is awaiting the user's verdict.

     What ends up here:
       - Every `applied` edit event from every Claude / Cursor session
         linked to THIS editor instance. We read sessions via
         `linkedAgents` (passed in from EditorView) so the pane doesn't
         have to know about the link store.
       - Grouped first by file path, then ordered chat-time within a
         file. Multiple agents can stack edits on the same file — each
         row carries the source agent badge so the user always knows
         who wrote which change.

     What the user can do here:
       - Open the file at the change (sets `pendingOpenFile` via the
         existing editor-open signal).
       - Keep / Revert per edit — same Tauri dispatch as the (planned)
         in-chat card; reuses `revertEditEvent` from `diffActions.ts`
         so the per-card and bulk-bar paths can never drift apart.
       - "Refine this hunk" — drops a primed `@<file>:start-end` mention
         + draft text into the source agent's composer and pings the
         InlineClaude expand signal so the user lands typing in the
         right session.
       - Top bar: Keep all / Revert all (delegates to the existing
         bulk helpers in `diffActions.ts`).

     Keyboard:
       - j / ArrowDown, k / ArrowUp — move selection between rows.
       - Enter / o — open the selected file at the change.
       - a — Keep the selected edit.
       - r — Revert / Restore the selected edit.
       - e — Refine the selected edit (focus its source session,
              prime the composer).

     Why a sidebar tab and not a separate solo: every other "review"
     surface in the app already lives next to the file tree (Search,
     Git). Putting Review there too means the user reviews + reads the
     file in the same screen, no rail jump needed. */
  import { sessionsState, requestEditorOpenFile, setSessionInput } from '$lib/state/sessions.svelte';
  import { revertEditEvent } from '$lib/services/diffActions';
  import { keepAllPendingEdits, revertAllPendingEdits } from '$lib/services/diffActions';
  import { updateEditEvent, getPendingEditEvents } from '$lib/state/sessions.svelte';
  import { notify, notifyError } from '$lib/state/toaster.svelte';
  import type { MessageEvent } from '$lib/types';

  type EditEvent = Extract<MessageEvent, { kind: 'edit' }>;

  interface LinkedAgent {
    sessionId: string;
    agentInstanceId: string;
    kind: 'claude' | 'cursor';
    name: string;
  }

  interface Props {
    /** Sessions linked to the editor this pane belongs to. Pulled from
     *  EditorView so the pane shares one source of truth with the
     *  rest of the editor's link UI — agents shown here, agents
     *  receiving "Apply to" mentions, and agents whose chat the
     *  Refine button reaches into are all the same set. */
    linkedAgents: LinkedAgent[];
    /** Editor instance id — passed straight through to
     *  `requestEditorOpenFile` so file opens land in this editor and
     *  not the primary singleton when the user has more than one
     *  Editor instance open. */
    instanceId: string;
    /** Repo root for shortening paths in the row label. Optional —
     *  when missing the row falls back to the absolute path. */
    repoPath: string;
  }
  let p: Props = $props();

  type Row = {
    /** `${sessionId}::${toolId}` — stable across re-renders so j/k
     *  keeps its place even when a Keep/Revert removes a row above. */
    key: string;
    sessionId: string;
    sessionTitle: string;
    sessionKind: 'claude' | 'cursor';
    event: EditEvent;
    /** Pre-computed for the head row (so the template doesn't recompute
     *  diffStats four times per render — once for the header, once for
     *  the diff body when expanded). */
    stats: { add: number; rem: number };
  };

  type FileGroup = {
    filePath: string;
    relPath: string;
    rows: Row[];
    addTotal: number;
    remTotal: number;
  };

  /* ── Diff utilities (vendored from ChatThread.svelte to keep this
     pane self-contained — both implementations are copies of the same
     LCS body, the chat one will be folded into a shared util in a
     follow-up). Kept inside the component so the chat copy keeps its
     own collapse heuristics tuned for an inline bubble. */
  type DiffRow = { kind: 'add' | 'rem' | 'ctx'; oldNo?: number; newNo?: number; text: string };

  const DIFF_LINE_CAP = 400;

  function computeDiffRows(oldText: string, newText: string): DiffRow[] {
    const a = oldText.split('\n');
    const b = newText.split('\n');
    const aTrim = a.length > DIFF_LINE_CAP ? a.slice(0, DIFF_LINE_CAP) : a;
    const bTrim = b.length > DIFF_LINE_CAP ? b.slice(0, DIFF_LINE_CAP) : b;
    const m = aTrim.length, n = bTrim.length;
    const dp: number[][] = Array.from({ length: m + 1 }, () => new Array(n + 1).fill(0));
    for (let i = 1; i <= m; i++) {
      for (let j = 1; j <= n; j++) {
        if (aTrim[i - 1] === bTrim[j - 1]) dp[i][j] = dp[i - 1][j - 1] + 1;
        else dp[i][j] = Math.max(dp[i - 1][j], dp[i][j - 1]);
      }
    }
    const rows: DiffRow[] = [];
    let i = m, j = n;
    while (i > 0 && j > 0) {
      if (aTrim[i - 1] === bTrim[j - 1]) {
        rows.push({ kind: 'ctx', oldNo: i, newNo: j, text: aTrim[i - 1] });
        i--; j--;
      } else if (dp[i - 1][j] >= dp[i][j - 1]) {
        rows.push({ kind: 'rem', oldNo: i, text: aTrim[i - 1] });
        i--;
      } else {
        rows.push({ kind: 'add', newNo: j, text: bTrim[j - 1] });
        j--;
      }
    }
    while (i > 0) { rows.push({ kind: 'rem', oldNo: i, text: aTrim[i - 1] }); i--; }
    while (j > 0) { rows.push({ kind: 'add', newNo: j, text: bTrim[j - 1] }); j--; }
    rows.reverse();
    return collapseContext(rows, 2);
  }

  function collapseContext(rows: DiffRow[], pad: number): DiffRow[] {
    const out: DiffRow[] = [];
    const n = rows.length;
    for (let i = 0; i < n; i++) {
      const r = rows[i];
      if (r.kind !== 'ctx') { out.push(r); continue; }
      let next = i;
      while (next < n && rows[next].kind === 'ctx') next++;
      const runLen = next - i;
      const isHead = out.length === 0;
      const isTail = next >= n;
      const head = isHead ? 0 : pad;
      const tail = isTail ? 0 : pad;
      if (runLen <= head + tail + 1) {
        for (let k = i; k < next; k++) out.push(rows[k]);
      } else {
        for (let k = i; k < i + head; k++) out.push(rows[k]);
        out.push({ kind: 'ctx', text: `··· ${runLen - head - tail} unchanged lines ···` });
        for (let k = next - tail; k < next; k++) out.push(rows[k]);
      }
      i = next - 1;
    }
    return out;
  }

  function diffStats(oldText: string, newText: string): { add: number; rem: number } {
    const rows = computeDiffRows(oldText ?? '', newText ?? '');
    let add = 0, rem = 0;
    for (const r of rows) {
      if (r.kind === 'add') add++;
      else if (r.kind === 'rem') rem++;
    }
    return { add, rem };
  }

  function relTo(repo: string, path: string): string {
    if (!repo) return path;
    const root = repo.replace(/\/$/, '');
    if (path === root) return '/';
    if (path.startsWith(root + '/')) return path.slice(root.length + 1);
    return path;
  }

  /* ── Reactive list of pending edits across every linked agent. We
     touch sessionsState.list inside the derived so $derived re-runs on
     any session mutation (new edit appended, status flipped, etc.). */
  const allRows = $derived.by<Row[]>(() => {
    void sessionsState.list;
    const out: Row[] = [];
    for (const la of p.linkedAgents) {
      const events = getPendingEditEvents(la.sessionId);
      for (const ev of events) {
        out.push({
          key: `${la.sessionId}::${ev.toolId}`,
          sessionId: la.sessionId,
          sessionTitle: la.name || (la.kind === 'claude' ? 'Claude' : 'Cursor'),
          sessionKind: la.kind,
          event: ev,
          stats: diffStats(ev.oldText ?? '', ev.newText ?? '')
        });
      }
    }
    return out;
  });

  /** Row count surfaced via `getReviewCount` so EditorView's badge
   *  reactively follows. Needed because Svelte 5 modules can't export
   *  $derived values, hence the function below. */
  function rowCount(): number {
    return allRows.length;
  }

  /* Group by file path. Order of files = order of first appearance in
     `allRows` (chat-time). Inside a file: chat-time order too. */
  const groups = $derived.by<FileGroup[]>(() => {
    const map = new Map<string, FileGroup>();
    for (const r of allRows) {
      const key = r.event.filePath;
      let g = map.get(key);
      if (!g) {
        g = {
          filePath: key,
          relPath: relTo(p.repoPath, key),
          rows: [],
          addTotal: 0,
          remTotal: 0
        };
        map.set(key, g);
      }
      g.rows.push(r);
      g.addTotal += r.stats.add;
      g.remTotal += r.stats.rem;
    }
    return Array.from(map.values());
  });

  const totals = $derived.by(() => {
    let add = 0, rem = 0;
    for (const r of allRows) { add += r.stats.add; rem += r.stats.rem; }
    return { add, rem, count: allRows.length };
  });

  /* ── Selection (j/k/Enter). The selection is a row key so add/remove
     events from above don't shift it; if the selected key disappears
     (Keep / Revert), we reset to the first row. Expanded set lives
     parallel — closing a row should NOT collapse if the same row is
     re-selected by keyboard.  */
  let selectedKey = $state<string | null>(null);
  let expandedKeys = $state<Set<string>>(new Set());

  /* When the row list changes, repair selection: keep selectedKey if
     still present, else collapse to the first row, else null. Expanded
     keys are pruned to what still exists so a Set replacement triggers
     reactivity downstream. */
  $effect(() => {
    const keys = new Set(allRows.map((r) => r.key));
    if (selectedKey && !keys.has(selectedKey)) {
      selectedKey = allRows[0]?.key ?? null;
    }
    if (selectedKey === null && allRows.length > 0) {
      selectedKey = allRows[0].key;
    }
    let needsRebuild = false;
    for (const k of expandedKeys) if (!keys.has(k)) { needsRebuild = true; break; }
    if (needsRebuild) {
      const next = new Set<string>();
      for (const k of expandedKeys) if (keys.has(k)) next.add(k);
      expandedKeys = next;
    }
  });

  function selectIndex(delta: number) {
    if (allRows.length === 0) return;
    const idx = Math.max(0, allRows.findIndex((r) => r.key === selectedKey));
    const next = (idx + delta + allRows.length) % allRows.length;
    selectedKey = allRows[next].key;
    /* Scroll the new selection into view. The row's DOM node carries
       data-row-key so we can address it without a per-row binding. */
    queueMicrotask(() => {
      const el = paneEl?.querySelector<HTMLElement>(`[data-row-key="${cssEscape(allRows[next].key)}"]`);
      el?.scrollIntoView({ block: 'nearest', behavior: 'instant' });
    });
  }

  function cssEscape(s: string): string {
    /* Defensive — toolIds and sessionIds are uuids/short ids so this
       is mostly a no-op, but a stray `:` would break the selector. */
    if (typeof CSS !== 'undefined' && typeof CSS.escape === 'function') return CSS.escape(s);
    return s.replace(/([^\w-])/g, '\\$1');
  }

  function toggleExpanded(key: string) {
    const next = new Set(expandedKeys);
    if (next.has(key)) next.delete(key);
    else next.add(key);
    expandedKeys = next;
  }

  let busyKeys = $state<Set<string>>(new Set());
  let bulkBusy = $state(false);

  function setBusy(key: string, on: boolean) {
    const next = new Set(busyKeys);
    if (on) next.add(key);
    else next.delete(key);
    busyKeys = next;
  }

  async function openAt(row: Row) {
    requestEditorOpenFile(p.instanceId, row.event.filePath);
  }

  function keepRow(row: Row) {
    /* Disk untouched — flip status only. Mirror of `keepAllPendingEdits`
       per single row so the per-row affordance stays in sync with the
       bulk one. */
    updateEditEvent(row.sessionId, row.event.toolId, { status: 'kept', note: undefined });
  }

  async function revertRow(row: Row) {
    if (busyKeys.has(row.key)) return;
    setBusy(row.key, true);
    const r = await revertEditEvent(row.sessionId, row.event);
    setBusy(row.key, false);
    if (!r.ok) {
      /* `revertEditEvent` already stamps `status: 'error'` and the
         `note` on the event itself; surface the toast so the user
         doesn't have to expand the row to see why. */
      notifyError(r.error, { title: `Couldn't revert ${row.event.filePath}` });
    }
  }

  function refineRow(row: Row) {
    /* Drop a primed prompt into the source session's composer + ping
       the InlineClaude expand signal so the user lands typing in the
       right place. We don't auto-send — refining is the user's
       opportunity to say "do this differently", so the agent should
       see the user's words, not a templated form. */
    const rel = relTo(p.repoPath, row.event.filePath);
    const verb = row.event.isCreate
      ? 'just created'
      : row.event.isDelete
      ? 'just deleted'
      : 'just changed';
    const draft = `Refine the edit you ${verb} in @${rel}: `;
    setSessionInput(row.sessionId, draft);
    sessionsState.requestInlineExpandFor = row.sessionId;
  }

  async function onKeepAll() {
    if (bulkBusy || allRows.length === 0) return;
    bulkBusy = true;
    let kept = 0;
    /* Bulk Keep is per-session because `keepAllPendingEdits` reads
       state for one session at a time. With one editor linked to two
       agents we run it once per agent and sum. */
    const seen = new Set<string>();
    for (const r of allRows) {
      if (seen.has(r.sessionId)) continue;
      seen.add(r.sessionId);
      kept += keepAllPendingEdits(r.sessionId);
    }
    bulkBusy = false;
    notify({ kind: 'success', title: kept === 1 ? 'Kept 1 edit' : `Kept ${kept} edits` });
  }

  async function onRevertAll() {
    if (bulkBusy || allRows.length === 0) return;
    bulkBusy = true;
    let reverted = 0, failed = 0, total = 0;
    const seen = new Set<string>();
    for (const r of allRows) {
      if (seen.has(r.sessionId)) continue;
      seen.add(r.sessionId);
      const summary = await revertAllPendingEdits(r.sessionId);
      reverted += summary.reverted;
      failed += summary.failed;
      total += summary.total;
    }
    bulkBusy = false;
    if (failed > 0) {
      notify({
        kind: 'warning',
        title: `Reverted ${reverted}/${total} edits`,
        body: `${failed} couldn't be undone — open the file and resolve manually.`
      });
    } else {
      notify({ kind: 'success', title: total === 1 ? 'Reverted 1 edit' : `Reverted ${total} edits` });
    }
  }

  /* ── Keyboard. The pane only listens when the user has actually
     focused something inside it — otherwise typing j in the editor
     would jump rows here. The wrapping `<section>` is `tabindex={0}`
     so it can be focused by clicking the empty area or arrowing in
     from the row list. */
  let paneEl: HTMLElement | null = $state(null);
  function onKey(e: KeyboardEvent) {
    if (allRows.length === 0) return;
    /* Don't intercept keystrokes inside an input/textarea/contenteditable. */
    const t = e.target as HTMLElement | null;
    if (t && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.isContentEditable)) return;
    if (e.key === 'j' || e.key === 'ArrowDown') { e.preventDefault(); selectIndex(1); return; }
    if (e.key === 'k' || e.key === 'ArrowUp')   { e.preventDefault(); selectIndex(-1); return; }
    const row = allRows.find((r) => r.key === selectedKey);
    if (!row) return;
    if (e.key === 'Enter' || e.key === 'o') { e.preventDefault(); void openAt(row); return; }
    if (e.key === 'a') { e.preventDefault(); keepRow(row); return; }
    if (e.key === 'r') { e.preventDefault(); void revertRow(row); return; }
    if (e.key === 'e') { e.preventDefault(); refineRow(row); return; }
    if (e.key === ' ') { e.preventDefault(); toggleExpanded(row.key); return; }
  }

  /* Re-export for the parent badge — EditorView reads this via a
     `bind:reviewCount` shape. Keeping it as an export-able function
     instead of an output prop avoids prop ping-pong on every selection
     change (which wouldn't affect the count anyway). */
  export { rowCount };
</script>

<section
  class="rp"
  bind:this={paneEl}
  tabindex="0"
  onkeydown={onKey}
  aria-label="Agent edits review"
>
  {#if allRows.length === 0}
    <div class="rp-empty">
      <div class="rp-empty-icon" aria-hidden="true">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">
          <path d="M4 12l5 5L20 6"/>
        </svg>
      </div>
      <p class="rp-empty-h serif">Nothing to review</p>
      <p class="rp-empty-p">
        {#if p.linkedAgents.length === 0}
          Link a Claude or Cursor session to this editor and the agent's
          edits will show up here as soon as it touches a file.
        {:else}
          Edits from
          {#each p.linkedAgents as la, i (la.sessionId)}
            <span class="rp-empty-agent">{la.name || (la.kind === 'claude' ? 'Claude' : 'Cursor')}</span>{i < p.linkedAgents.length - 1 ? ', ' : ''}
          {/each}
          land here grouped by file. Keep, revert, or refine without
          leaving the editor.
        {/if}
      </p>
    </div>
  {:else}
    <header class="rp-bar">
      <span class="rp-bar-count mono">
        {totals.count} edit{totals.count === 1 ? '' : 's'}
      </span>
      <span class="rp-bar-stats mono">
        <span class="rp-add">+{totals.add}</span>
        <span class="rp-rem">−{totals.rem}</span>
      </span>
      <span class="rp-bar-spacer"></span>
      <button
        class="rp-bar-btn"
        disabled={bulkBusy}
        onclick={() => void onRevertAll()}
        title="Revert every applied edit (newest first). Each revert checks the file is still in the agent-written state — out-of-sync edits surface as warnings."
      >Revert all</button>
      <button
        class="rp-bar-btn rp-bar-btn--primary"
        disabled={bulkBusy}
        onclick={() => void onKeepAll()}
        title="Mark every applied edit as kept. Disk is untouched; this just records your verdict so the per-row affordance flips."
      >Keep all</button>
    </header>

    <div class="rp-list" role="listbox" aria-label="Pending agent edits">
      {#each groups as g (g.filePath)}
        <div class="rp-group">
          <button
            class="rp-group-head mono"
            onclick={() => void requestEditorOpenFile(p.instanceId, g.filePath)}
            title="Open {g.relPath}"
          >
            <span class="rp-group-name">{g.relPath}</span>
            <span class="rp-group-stats">
              <span class="rp-add">+{g.addTotal}</span>
              <span class="rp-rem">−{g.remTotal}</span>
            </span>
          </button>

          {#each g.rows as row (row.key)}
            {@const expanded = expandedKeys.has(row.key)}
            {@const selected = selectedKey === row.key}
            {@const busy = busyKeys.has(row.key)}
            <div
              class="rp-row"
              class:rp-row--selected={selected}
              class:rp-row--expanded={expanded}
              data-row-key={row.key}
              role="option"
              aria-selected={selected}
              onclick={() => { selectedKey = row.key; }}
            >
              <button
                class="rp-row-head"
                onclick={(e) => { e.stopPropagation(); selectedKey = row.key; toggleExpanded(row.key); }}
                title="Toggle diff"
              >
                <span class="rp-row-caret" aria-hidden="true" class:rp-row-caret--open={expanded}>
                  <svg viewBox="0 0 24 24" width="10" height="10" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"><path d="M6 9l6 6 6-6"/></svg>
                </span>
                <span class="rp-row-tag rp-row-tag--{row.event.isCreate ? 'add' : row.event.isDelete ? 'rem' : 'edit'}">
                  {#if row.event.isCreate}Create
                  {:else if row.event.isDelete}Delete
                  {:else if row.event.wholeFile}Write
                  {:else}Edit{/if}
                </span>
                <span class="rp-row-agent rp-row-agent--{row.sessionKind}" title="From {row.sessionTitle}">
                  {row.sessionKind === 'claude' ? 'C' : 'X'}
                </span>
                <span class="rp-row-stats mono">
                  <span class="rp-add">+{row.stats.add}</span>
                  <span class="rp-rem">−{row.stats.rem}</span>
                </span>
                <span class="rp-row-spacer"></span>
                {#if row.event.status === 'loading'}
                  <span class="rp-row-status rp-row-status--loading mono">streaming…</span>
                {/if}
              </button>

              <div class="rp-row-actions">
                <button
                  class="rp-act"
                  onclick={(e) => { e.stopPropagation(); void openAt(row); }}
                  title="Open file (Enter / o)"
                >Open</button>
                <button
                  class="rp-act"
                  onclick={(e) => { e.stopPropagation(); refineRow(row); }}
                  title="Type a follow-up to {row.sessionTitle} (e)"
                >Refine</button>
                <button
                  class="rp-act"
                  disabled={busy}
                  onclick={(e) => { e.stopPropagation(); void revertRow(row); }}
                  title={row.event.isDelete ? 'Restore (r)' : 'Revert (r)'}
                >{row.event.isDelete ? 'Restore' : 'Revert'}</button>
                <button
                  class="rp-act rp-act--primary"
                  disabled={busy}
                  onclick={(e) => { e.stopPropagation(); keepRow(row); }}
                  title="Keep (a)"
                >Keep</button>
              </div>

              {#if expanded}
                <div class="rp-row-body">
                  {#if row.event.status === 'loading'}
                    <div class="rp-row-loading mono">Waiting for the agent to finish writing…</div>
                  {:else}
                    <div class="rp-diff" role="presentation">
                      {#each computeDiffRows(row.event.oldText ?? '', row.event.newText ?? '') as drow, di (di)}
                        <div class="rp-diff-row rp-diff-row--{drow.kind}">
                          <span class="rp-diff-no mono">{drow.oldNo ?? ''}</span>
                          <span class="rp-diff-no mono">{drow.newNo ?? ''}</span>
                          <span class="rp-diff-glyph mono">{drow.kind === 'add' ? '+' : drow.kind === 'rem' ? '−' : ' '}</span>
                          <span class="rp-diff-text mono">{drow.text}</span>
                        </div>
                      {/each}
                    </div>
                  {/if}
                </div>
              {/if}
            </div>
          {/each}
        </div>
      {/each}
    </div>

    <footer class="rp-foot mono">
      <kbd>j</kbd>/<kbd>k</kbd> move
      <kbd>a</kbd> keep
      <kbd>r</kbd> revert
      <kbd>e</kbd> refine
      <kbd>↵</kbd> open
      <kbd>space</kbd> diff
    </footer>
  {/if}
</section>

<style>
  .rp {
    display: flex; flex-direction: column;
    height: 100%;
    min-height: 0;
    outline: none;
    background: var(--bg-1);
  }
  .rp:focus-visible { box-shadow: inset 0 0 0 1px var(--border-accent-2); }

  /* Empty state — same vocabulary as the Debug / Tests placeholders
     in EditorView so the sidebar feels uniform across tabs. */
  .rp-empty {
    flex: 1; min-height: 0;
    display: flex; flex-direction: column; align-items: center; justify-content: center;
    text-align: center;
    padding: 24px 22px;
    color: var(--text-2);
  }
  .rp-empty-icon {
    width: 38px; height: 38px;
    border-radius: 12px;
    display: grid; place-items: center;
    background: var(--bg-2);
    color: var(--text-mute);
    margin-bottom: 12px;
  }
  .rp-empty-icon svg { width: 20px; height: 20px; }
  .rp-empty-h { color: var(--text-0); margin: 0 0 6px; font-size: 14px; }
  .rp-empty-p { font-size: 12.5px; line-height: 1.5; max-width: 280px; margin: 0; }
  .rp-empty-agent {
    color: var(--text-1);
    font-family: 'JetBrains Mono', monospace;
    font-size: 12px;
  }

  /* Top bar — sticky so Keep/Revert all stay visible while the user
     scrolls a long list. Mirrors the chat composer's "pending" bar
     styling so the affordance reads the same regardless of where the
     user reviews from. */
  .rp-bar {
    position: sticky; top: 0; z-index: 2;
    display: flex; align-items: center; gap: 10px;
    padding: 8px 10px;
    background: var(--bg-1);
    border-bottom: 1px solid var(--border);
  }
  .rp-bar-count { font-size: 11.5px; color: var(--text-1); font-weight: 600; }
  .rp-bar-stats { display: flex; gap: 6px; font-size: 11px; }
  .rp-bar-spacer { flex: 1; }
  .rp-bar-btn {
    padding: 4px 10px;
    background: var(--bg-2);
    border: 1px solid var(--border);
    color: var(--text-1);
    border-radius: 5px;
    font-size: 11.5px;
    cursor: pointer;
    transition: color 120ms, border-color 120ms, background 120ms;
  }
  .rp-bar-btn:hover { color: var(--text-0); border-color: var(--border-strong, var(--border)); }
  .rp-bar-btn:disabled { opacity: 0.5; cursor: not-allowed; }
  .rp-bar-btn--primary {
    background: var(--accent-soft);
    border-color: var(--border-accent-2);
    color: var(--accent-bright);
  }
  .rp-bar-btn--primary:hover { background: var(--accent-soft-strong, var(--accent-soft)); color: var(--accent-bright); }

  .rp-list {
    flex: 1; min-height: 0;
    overflow-y: auto;
    padding: 4px 0 8px;
  }

  .rp-group { padding: 8px 0 4px; }
  .rp-group-head {
    width: 100%;
    display: flex; align-items: center; gap: 8px;
    padding: 4px 12px;
    background: transparent; border: 0; cursor: pointer;
    color: var(--text-1);
    font-size: 11.5px;
    text-align: left;
  }
  .rp-group-head:hover { color: var(--text-0); }
  .rp-group-name {
    flex: 1; min-width: 0;
    white-space: nowrap; text-overflow: ellipsis; overflow: hidden;
  }
  .rp-group-stats { display: flex; gap: 6px; font-size: 10.5px; flex: 0 0 auto; }

  .rp-row {
    margin: 2px 6px;
    border: 1px solid transparent;
    border-radius: 6px;
    background: var(--bg-2);
    transition: border-color 120ms, background 120ms;
    cursor: default;
  }
  .rp-row:hover { border-color: var(--border); }
  .rp-row--selected {
    border-color: var(--border-accent-2);
    background: linear-gradient(180deg, var(--accent-soft), var(--bg-2));
  }
  .rp-row--expanded { background: var(--bg-1); border-color: var(--border); }

  .rp-row-head {
    width: 100%;
    display: flex; align-items: center; gap: 8px;
    padding: 6px 8px;
    background: transparent; border: 0; cursor: pointer;
    color: var(--text-1);
    font-size: 11.5px;
    text-align: left;
  }
  .rp-row-head:hover { color: var(--text-0); }
  .rp-row-caret {
    display: inline-flex; align-items: center; justify-content: center;
    width: 12px; height: 12px;
    color: var(--text-mute);
    transition: transform 140ms;
  }
  .rp-row-caret--open { transform: rotate(180deg); }
  .rp-row-tag {
    font-family: 'JetBrains Mono', monospace;
    font-size: 10px; font-weight: 700;
    padding: 1px 6px;
    border-radius: 4px;
    background: var(--bg-1);
    color: var(--text-2);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .rp-row-tag--add { color: var(--diff-add-stroke); }
  .rp-row-tag--rem { color: var(--diff-rem-stroke); }
  .rp-row-agent {
    width: 16px; height: 16px;
    display: grid; place-items: center;
    border-radius: 4px;
    font-family: 'JetBrains Mono', monospace;
    font-size: 9.5px; font-weight: 700;
    color: var(--accent-fg);
    flex: 0 0 auto;
  }
  .rp-row-agent--claude { background: var(--src-claude); }
  .rp-row-agent--cursor { background: var(--src-cursor); }
  .rp-row-stats { display: flex; gap: 6px; font-size: 10.5px; }
  .rp-row-spacer { flex: 1; }
  .rp-row-status { font-size: 10px; color: var(--text-mute); }
  .rp-row-status--loading { color: var(--accent-bright); }

  .rp-add { color: var(--diff-add); }
  .rp-rem { color: var(--diff-rem); }

  .rp-row-actions {
    display: flex; gap: 4px;
    padding: 0 8px 6px 30px;
    opacity: 0;
    transition: opacity 140ms;
    pointer-events: none;
  }
  .rp-row:hover .rp-row-actions,
  .rp-row--selected .rp-row-actions,
  .rp-row--expanded .rp-row-actions {
    opacity: 1;
    pointer-events: auto;
  }
  .rp-act {
    padding: 2px 8px;
    background: transparent;
    border: 1px solid var(--border);
    color: var(--text-2);
    border-radius: 4px;
    font-size: 11px;
    cursor: pointer;
    transition: color 120ms, border-color 120ms, background 120ms;
  }
  .rp-act:hover { color: var(--text-0); border-color: var(--border-strong, var(--border)); background: var(--bg-elev, var(--bg-2)); }
  .rp-act:disabled { opacity: 0.5; cursor: not-allowed; }
  .rp-act--primary {
    color: var(--accent-bright);
    border-color: var(--border-accent-2);
    background: var(--accent-soft);
  }
  .rp-act--primary:hover { color: var(--accent-bright); }

  .rp-row-body { padding: 0 8px 8px 30px; }
  .rp-row-loading {
    padding: 6px 8px;
    color: var(--text-mute);
    font-size: 11px;
    background: var(--bg-2);
    border-radius: 4px;
  }
  .rp-diff {
    border-radius: 4px;
    border: 1px solid var(--border);
    background: var(--bg-0);
    overflow-x: auto;
  }
  .rp-diff-row {
    display: grid;
    grid-template-columns: 28px 28px 12px 1fr;
    gap: 0;
    align-items: baseline;
    padding: 0 6px;
    font-size: 11.5px;
    line-height: 1.45;
    white-space: pre;
  }
  .rp-diff-row--add {
    background: color-mix(in srgb, var(--diff-add) 18%, transparent);
    color: var(--diff-add-stroke);
  }
  .rp-diff-row--rem {
    background: color-mix(in srgb, var(--diff-rem) 18%, transparent);
    color: var(--diff-rem-stroke);
  }
  .rp-diff-row--ctx { color: var(--text-2); }
  .rp-diff-no {
    color: var(--text-mute);
    text-align: right; padding-right: 4px;
    font-size: 10.5px;
  }
  .rp-diff-glyph { color: inherit; opacity: 0.7; }
  .rp-diff-text {
    overflow: hidden; text-overflow: ellipsis;
    color: inherit;
  }

  .rp-foot {
    padding: 6px 12px;
    border-top: 1px solid var(--border);
    color: var(--text-mute);
    font-size: 10.5px;
    display: flex; gap: 10px; flex-wrap: wrap;
    background: var(--bg-1);
  }
  .rp-foot kbd {
    background: var(--bg-2);
    border: 1px solid var(--border);
    border-radius: 3px;
    padding: 0 5px;
    color: var(--text-1);
    font-family: inherit;
    font-size: 10px;
  }
</style>
