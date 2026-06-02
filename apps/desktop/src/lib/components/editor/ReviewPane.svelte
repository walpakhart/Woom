<script lang="ts">
  /* ReviewPane — sidebar tab that turns the agents' streamed edits into
     a reviewable workspace. Same idea as VS Code's "Source Control"
     pane, but the unit isn't a git hunk — it's an agent edit event
     (`MessageEvent { kind: 'edit' }`) that's been written to disk and
     is awaiting the user's verdict.

     This is a COMPACT NAVIGATOR: one quiet line per edit, grouped under
     collapsible file headers. No diff renders here — selecting a row
     drives the editor to open the file scrolled to the edit's first
     hunk, where the inline overlay (the app's single diff surface)
     shows adds/deletes and Tab/Esc resolve per hunk. The row also
     offers Keep / Revert / Refine; either path flips the EditEvent
     status and the list updates reactively.

     Keyboard:
       - j / ArrowDown, k / ArrowUp — move selection (auto-opens in editor).
       - Enter / o — open the selected file at the change.
       - a — Keep the selected edit.
       - r — Revert / Restore the selected edit.
       - e — Refine the selected edit (focus its source session).
       - space — fold / unfold the selected row's file group. */
  import { sessionsState, requestEditorOpenFile, setSessionInput } from '$lib/state/sessions.svelte';
  import { revertEditEvent } from '$lib/services/diffActions';
  import { keepAllPendingEdits, revertAllPendingEdits } from '$lib/services/diffActions';
  import { updateEditEvent, getPendingEditEvents } from '$lib/state/sessions.svelte';
  import { notify, notifyError } from '$lib/state/toaster.svelte';
  import { editStats } from '$lib/components/editor/reviewStats';
  import type { MessageEvent } from '$lib/types';

  type EditEvent = Extract<MessageEvent, { kind: 'edit' }>;

  interface LinkedAgent {
    sessionId: string;
    agentInstanceId: string;
    kind: 'claude' | 'cursor';
    name: string;
  }

  interface Props {
    /** Sessions linked to the editor this pane belongs to. Shared source
     *  of truth with the rest of the editor's link UI. */
    linkedAgents: LinkedAgent[];
    /** Editor instance id — so file opens land in THIS editor. */
    instanceId: string;
    /** Repo root for shortening paths in the file header. */
    repoPath: string;
    /** Select an agent edit: opens its file in the editor and highlights +
     *  scrolls to exactly that edit's hunks (the editor is the only diff
     *  surface). Wired by EditorView. */
    onSelectEdit?: (filePath: string, sessionId: string, toolId: string) => void;
  }
  let p: Props = $props();

  type Row = {
    /** `${sessionId}::${toolId}` — stable across re-renders so j/k keeps
     *  its place even when a Keep/Revert removes a row above. */
    key: string;
    sessionId: string;
    sessionTitle: string;
    sessionKind: 'claude' | 'cursor';
    event: EditEvent;
    stats: { add: number; rem: number };
  };

  type FileGroup = {
    filePath: string;
    relPath: string;
    rows: Row[];
    addTotal: number;
    remTotal: number;
  };

  function relTo(repo: string, path: string): string {
    if (!repo) return path;
    const root = repo.replace(/\/$/, '');
    if (path === root) return '/';
    if (path.startsWith(root + '/')) return path.slice(root.length + 1);
    return path;
  }

  /** Split a path into its directory and filename so the file header can show
   *  a bold name with a muted, left-truncated directory beside it. */
  function splitPath(rel: string): { dir: string; name: string } {
    const i = rel.lastIndexOf('/');
    return i < 0 ? { dir: '', name: rel } : { dir: rel.slice(0, i), name: rel.slice(i + 1) };
  }

  /* ── Reactive list of pending edits across every linked agent. Touch
     sessionsState.list inside the derive so it re-runs on any session
     mutation (new edit appended, status flipped, etc.). Counts come from
     `editStats` — the SAME engine the editor overlay uses (no second
     diff impl lives here anymore). */
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
          stats: editStats(ev.oldText ?? '', ev.newText ?? '')
        });
      }
    }
    return out;
  });

  /** Row count surfaced via `getReviewCount` so EditorView's badge
   *  reactively follows (Svelte 5 modules can't export $derived). */
  function rowCount(): number {
    return allRows.length;
  }

  /* Group by file path. File order = first appearance (chat-time). */
  const groups = $derived.by<FileGroup[]>(() => {
    const map = new Map<string, FileGroup>();
    for (const r of allRows) {
      const key = r.event.filePath;
      let g = map.get(key);
      if (!g) {
        g = { filePath: key, relPath: relTo(p.repoPath, key), rows: [], addTotal: 0, remTotal: 0 };
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

  /* ── Selection (j/k/Enter). Keyed by row key so add/remove above doesn't
     shift it; if the selected key disappears (Keep/Revert), reset to the
     first row. */
  let selectedKey = $state<string | null>(null);

  $effect(() => {
    const keys = new Set(allRows.map((r) => r.key));
    if (selectedKey && !keys.has(selectedKey)) {
      selectedKey = allRows[0]?.key ?? null;
    }
    if (selectedKey === null && allRows.length > 0) {
      selectedKey = allRows[0].key;
    }
  });

  /* ── Collapsible file groups. A collapsed file hides its rows but keeps
     the summed +N −M visible on the header. */
  let collapsedFiles = $state<Set<string>>(new Set());
  function toggleFile(filePath: string) {
    const next = new Set(collapsedFiles);
    if (next.has(filePath)) next.delete(filePath);
    else next.add(filePath);
    collapsedFiles = next;
  }

  function selectIndex(delta: number) {
    if (allRows.length === 0) return;
    const idx = Math.max(0, allRows.findIndex((r) => r.key === selectedKey));
    const next = (idx + delta + allRows.length) % allRows.length;
    const row = allRows[next];
    selectedKey = row.key;
    openSelected(row);
    queueMicrotask(() => {
      const el = paneEl?.querySelector<HTMLElement>(`[data-row-key="${cssEscape(row.key)}"]`);
      el?.scrollIntoView({ block: 'nearest', behavior: 'instant' });
    });
  }

  function cssEscape(s: string): string {
    if (typeof CSS !== 'undefined' && typeof CSS.escape === 'function') return CSS.escape(s);
    return s.replace(/([^\w-])/g, '\\$1');
  }

  let busyKeys = $state<Set<string>>(new Set());
  let bulkBusy = $state(false);

  function setBusy(key: string, on: boolean) {
    const next = new Set(busyKeys);
    if (on) next.add(key);
    else next.delete(key);
    busyKeys = next;
  }

  /** Open the row's file in the editor and highlight + scroll to exactly this
   *  edit's hunks — the editor's inline overlay is the only diff surface. */
  function openSelected(row: Row) {
    if (p.onSelectEdit) p.onSelectEdit(row.event.filePath, row.sessionId, row.event.toolId);
    else requestEditorOpenFile(p.instanceId, row.event.filePath);
  }

  function clickRow(row: Row) {
    selectedKey = row.key;
    openSelected(row);
  }

  function keepRow(row: Row) {
    updateEditEvent(row.sessionId, row.event.toolId, { status: 'kept', note: undefined });
  }

  async function revertRow(row: Row) {
    if (busyKeys.has(row.key)) return;
    setBusy(row.key, true);
    const r = await revertEditEvent(row.sessionId, row.event);
    setBusy(row.key, false);
    if (!r.ok) {
      notifyError(r.error, { title: `Couldn't revert ${row.event.filePath}` });
    }
  }

  function refineRow(row: Row) {
    const rel = relTo(p.repoPath, row.event.filePath);
    const verb = row.event.isCreate ? 'just created' : row.event.isDelete ? 'just deleted' : 'just changed';
    const draft = `Refine the edit you ${verb} in @${rel}: `;
    setSessionInput(row.sessionId, draft);
    sessionsState.requestInlineExpandFor = row.sessionId;
  }

  async function onKeepAll() {
    if (bulkBusy || allRows.length === 0) return;
    bulkBusy = true;
    let kept = 0;
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

  /* ── Keyboard. Only listens when focus is inside the pane; ignores
     keystrokes inside inputs. */
  let paneEl: HTMLElement | null = $state(null);
  function onKey(e: KeyboardEvent) {
    if (allRows.length === 0) return;
    const t = e.target as HTMLElement | null;
    if (t && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.isContentEditable)) return;
    if (e.key === 'j' || e.key === 'ArrowDown') { e.preventDefault(); selectIndex(1); return; }
    if (e.key === 'k' || e.key === 'ArrowUp')   { e.preventDefault(); selectIndex(-1); return; }
    const row = allRows.find((r) => r.key === selectedKey);
    if (!row) return;
    if (e.key === 'Enter' || e.key === 'o') { e.preventDefault(); openSelected(row); return; }
    if (e.key === 'a') { e.preventDefault(); keepRow(row); return; }
    if (e.key === 'r') { e.preventDefault(); void revertRow(row); return; }
    if (e.key === 'e') { e.preventDefault(); refineRow(row); return; }
    if (e.key === ' ') { e.preventDefault(); toggleFile(row.event.filePath); return; }
  }

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
          land here grouped by file. Select one to review it in the editor.
        {/if}
      </p>
    </div>
  {:else}
    <header class="rp-bar">
      <span class="rp-bar-count mono">{totals.count} edit{totals.count === 1 ? '' : 's'}</span>
      <span class="rp-bar-stats mono">
        <span class="rp-add">+{totals.add}</span>
        <span class="rp-rem">−{totals.rem}</span>
      </span>
      <span class="rp-bar-spacer"></span>
      <button
        class="rp-bar-btn"
        disabled={bulkBusy}
        onclick={() => void onRevertAll()}
        title="Revert every applied edit (newest first)."
      >Revert all</button>
      <button
        class="rp-bar-btn rp-bar-btn--primary"
        disabled={bulkBusy}
        onclick={() => void onKeepAll()}
        title="Mark every applied edit as kept. Disk untouched."
      >Keep all</button>
    </header>

    <div class="rp-list" role="listbox" aria-label="Pending agent edits">
      {#each groups as g (g.filePath)}
        {@const collapsed = collapsedFiles.has(g.filePath)}
        {@const parts = splitPath(g.relPath)}
        <div class="rp-group" class:rp-group--collapsed={collapsed}>
          <button
            class="rp-group-head mono"
            class:rp-group-head--collapsed={collapsed}
            onclick={() => toggleFile(g.filePath)}
            title="{collapsed ? 'Expand' : 'Collapse'} {g.relPath}"
          >
            <span class="rp-group-caret" aria-hidden="true" class:rp-group-caret--open={!collapsed}>
              <svg viewBox="0 0 24 24" width="11" height="11" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M6 9l6 6 6-6"/></svg>
            </span>
            <span class="rp-group-file">
              <span class="rp-group-name">{parts.name}</span>
              {#if parts.dir}<span class="rp-group-dir">{parts.dir}</span>{/if}
            </span>
            <span class="rp-group-meta">
              <span class="rp-group-n" title="{g.rows.length} edit{g.rows.length === 1 ? '' : 's'}">{g.rows.length}</span>
              <span class="rp-group-stats">
                <span class="rp-add">+{g.addTotal}</span>
                <span class="rp-rem">−{g.remTotal}</span>
              </span>
            </span>
          </button>

          {#if !collapsed}
            {#each g.rows as row (row.key)}
              {@const selected = selectedKey === row.key}
              {@const busy = busyKeys.has(row.key)}
              <div
                class="rp-row"
                class:rp-row--selected={selected}
                data-row-key={row.key}
                role="option"
                aria-selected={selected}
                tabindex="-1"
                onclick={() => clickRow(row)}
                onkeydown={(e) => { if (e.key === 'Enter') { e.preventDefault(); clickRow(row); } }}
              >
                <span
                  class="rp-dot rp-dot--{row.event.status}"
                  title="{row.event.status}"
                  aria-hidden="true"
                ></span>
                <span class="rp-row-tag rp-row-tag--{row.event.isCreate ? 'add' : row.event.isDelete ? 'rem' : 'edit'}">
                  {#if row.event.isCreate}Create
                  {:else if row.event.isDelete}Delete
                  {:else if row.event.wholeFile}Write
                  {:else}Edit{/if}
                </span>
                <span class="rp-row-agent rp-row-agent--{row.sessionKind}" title="From {row.sessionTitle}">
                  {row.sessionKind === 'claude' ? 'C' : 'X'}
                </span>
                {#if row.event.status === 'loading'}
                  <span class="rp-row-streaming mono">streaming…</span>
                {/if}
                <span class="rp-row-spacer"></span>
                <span class="rp-row-stats mono">
                  <span class="rp-add">+{row.stats.add}</span>
                  <span class="rp-rem">−{row.stats.rem}</span>
                </span>
                <span class="rp-row-actions">
                  <button class="rp-act" onclick={(e) => { e.stopPropagation(); openSelected(row); }} title="Open (Enter / o)">Open</button>
                  <button class="rp-act" onclick={(e) => { e.stopPropagation(); refineRow(row); }} title="Refine (e)">Refine</button>
                  <button class="rp-act" disabled={busy} onclick={(e) => { e.stopPropagation(); void revertRow(row); }} title={row.event.isDelete ? 'Restore (r)' : 'Revert (r)'}>{row.event.isDelete ? 'Restore' : 'Revert'}</button>
                  <button class="rp-act rp-act--primary" disabled={busy} onclick={(e) => { e.stopPropagation(); keepRow(row); }} title="Keep (a)">Keep</button>
                </span>
              </div>
            {/each}
          {/if}
        </div>
      {/each}
    </div>

    <footer class="rp-foot mono">
      <kbd>j</kbd>/<kbd>k</kbd> move
      <kbd>a</kbd> keep
      <kbd>r</kbd> revert
      <kbd>e</kbd> refine
      <kbd>↵</kbd> open
      <kbd>space</kbd> fold
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

  /* Empty state — same vocabulary as the Debug / Tests placeholders. */
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

  /* Top bar — sticky count + bulk actions. */
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
    padding: 6px;
    display: flex; flex-direction: column; gap: 8px;
  }

  /* File group — a distinct card so files read as separate buckets. */
  .rp-group {
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--bg-2);
    overflow: hidden;
  }
  .rp-group--collapsed { background: var(--bg-1); }

  /* File header — banded, sticky-ish, bold name + muted dir + meta. */
  .rp-group-head {
    width: 100%;
    display: flex; align-items: center; gap: 8px;
    padding: 7px 10px;
    background: var(--bg-3, var(--bg-2));
    border: 0;
    border-bottom: 1px solid var(--border);
    cursor: pointer;
    color: var(--text-1);
    font-size: 11.5px;
    text-align: left;
    min-width: 0;
    overflow: hidden;
  }
  .rp-group-head--collapsed { border-bottom-color: transparent; }
  .rp-group-head:hover { background: var(--bg-3, var(--bg-2)); color: var(--text-0); }
  .rp-group-caret {
    display: inline-flex; align-items: center; justify-content: center;
    width: 13px; height: 13px; flex: 0 0 auto;
    color: var(--text-2);
    transform: rotate(-90deg);
    transition: transform 140ms;
  }
  .rp-group-caret--open { transform: rotate(0deg); }
  .rp-group-file {
    flex: 1 1 auto; min-width: 0;
    display: flex; align-items: baseline; gap: 6px;
    overflow: hidden;
  }
  /* Filename keeps priority — it shrinks slowly (shrink 1) and only after the
     directory (shrink 1000) has fully collapsed to an ellipsis. */
  .rp-group-name {
    flex: 0 1 auto;
    min-width: 2ch;
    color: var(--text-0);
    font-weight: 700;
    white-space: nowrap; text-overflow: ellipsis; overflow: hidden;
  }
  .rp-group-dir {
    flex: 1 1000 auto;
    min-width: 0;
    color: var(--text-mute);
    font-size: 10.5px;
    white-space: nowrap; overflow: hidden;
    text-overflow: ellipsis;
  }
  .rp-group-meta { display: flex; align-items: center; gap: 8px; flex: 0 0 auto; }
  .rp-group-n {
    min-width: 16px; height: 16px;
    padding: 0 5px;
    display: inline-flex; align-items: center; justify-content: center;
    border-radius: 8px;
    background: var(--bg-1);
    border: 1px solid var(--border);
    color: var(--text-1);
    font-size: 10px; font-weight: 700;
  }
  .rp-group-stats { display: flex; gap: 6px; font-size: 10.5px; flex: 0 0 auto; }

  /* Row — one compact line, nested under its file header. Separated by
     hairlines; actions hidden until hover / selection. */
  .rp-row {
    display: flex; align-items: center; gap: 8px;
    padding: 6px 10px 6px 12px;
    border-left: 2px solid transparent;
    border-top: 1px solid var(--border);
    cursor: pointer;
    transition: background 120ms, border-color 120ms;
    outline: none;
  }
  .rp-row:first-of-type { border-top: 0; }
  .rp-row:hover { background: var(--bg-3, var(--bg-1)); }
  .rp-row--selected {
    border-left-color: var(--accent);
    background: linear-gradient(90deg, var(--accent-soft), transparent 70%);
  }

  .rp-dot {
    width: 7px; height: 7px; border-radius: 50%;
    flex: 0 0 auto;
    background: var(--text-mute);
  }
  .rp-dot--applied { background: var(--accent); }
  .rp-dot--kept { background: var(--diff-add); }
  .rp-dot--reverted { background: var(--text-mute); }
  .rp-dot--error { background: var(--diff-rem); }
  .rp-dot--loading { background: var(--accent-bright); }

  .rp-row-tag {
    font-family: 'JetBrains Mono', monospace;
    font-size: 10px; font-weight: 700;
    padding: 1px 6px;
    border-radius: 4px;
    background: color-mix(in srgb, var(--accent) 18%, transparent);
    color: var(--accent-bright);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    flex: 0 0 auto;
  }
  .rp-row-tag--add { background: color-mix(in srgb, var(--diff-add) 28%, transparent); color: var(--text-0); }
  .rp-row-tag--rem { background: color-mix(in srgb, var(--diff-rem) 28%, transparent); color: var(--text-0); }

  /* Per-source brand accent — KEEP distinct (Claude rust, Cursor tone). */
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

  .rp-row-streaming { font-size: 10px; color: var(--accent-bright); flex: 0 0 auto; }
  .rp-row-spacer { flex: 1; }
  .rp-row-stats { display: flex; gap: 6px; font-size: 10.5px; flex: 0 0 auto; }
  .rp-add { color: var(--diff-add); }
  .rp-rem { color: var(--diff-rem); }

  .rp-row-actions {
    display: flex; gap: 5px;
    flex: 0 0 auto;
    opacity: 0;
    pointer-events: none;
    transition: opacity 120ms;
  }
  .rp-row:hover .rp-row-actions,
  .rp-row--selected .rp-row-actions {
    opacity: 1;
    pointer-events: auto;
  }
  .rp-act {
    padding: 2px 8px;
    background: var(--bg-1);
    border: 1px solid var(--border-neutral-hi, var(--border));
    color: var(--text-0);
    border-radius: 4px;
    font-size: 10.5px;
    cursor: pointer;
    transition: color 120ms, border-color 120ms, background 120ms;
  }
  .rp-act:hover { border-color: var(--accent); background: var(--bg-3, var(--bg-2)); }
  .rp-act:disabled { opacity: 0.45; cursor: not-allowed; }
  .rp-act--primary {
    color: var(--accent-bright);
    border-color: var(--accent);
    background: var(--accent-soft);
  }
  .rp-act--primary:hover { color: var(--text-0); background: var(--accent); border-color: var(--accent); }

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
