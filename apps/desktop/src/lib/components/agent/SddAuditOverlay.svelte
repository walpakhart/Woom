<script lang="ts">
  /* SddAuditOverlay — the per-workspace "Audit log" panel that
     slides out under the SddCard header. Extracted from
     SddCard.svelte in wave-1 phase-8 refactor.

     Owns its own state (entries, filter, expanded row), loading
     lifecycle (lazy + auto-refresh on stage flip + Esc-to-close),
     and clipboard export. The parent only tracks the `open`
     boolean + reads `entriesCount` for the header chip — the
     overlay is otherwise self-contained, so SddCard sheds
     ~150 LoC of audit plumbing. */
  import type { AuditEntry } from '$lib/state/sdd.svelte';
  import { loadAuditLog } from '$lib/state/sdd.svelte';

  interface Props {
    /** Workspace whose audit log we're viewing. Re-fetch fires when
     *  this changes (mounting a different SDD workspace). */
    workspaceId: string;
    /** Stage discriminant — bumped by the parent whenever the
     *  workspace stage flips so we know to refresh entries (a
     *  state-changing command just landed on disk). */
    stageKind: string;
    /** Open / close binding so the parent header chip can toggle
     *  visibility. Esc inside the overlay flips this back to false. */
    open: boolean;
    /** Exposed so the parent's "· N audit · view" chip can render
     *  the count without subscribing to our internal state. Updated
     *  via $bindable. */
    entriesCount?: number;
  }
  let {
    workspaceId,
    stageKind,
    open = $bindable(false),
    entriesCount = $bindable(0),
  }: Props = $props();

  let entries = $state<AuditEntry[]>([]);
  let loaded = $state(false);
  let filter = $state<'all' | 'agent' | 'user' | 'system'>('all');
  let expandedTs = $state<number | null>(null);

  const filtered = $derived(
    filter === 'all' ? entries : entries.filter((e) => e.source === filter)
  );

  async function refresh(): Promise<void> {
    const next = await loadAuditLog(workspaceId);
    entries = next;
    entriesCount = next.length;
    loaded = true;
  }

  /* Auto-refresh on stage flip — covers user mutations + agent
     mutations (both land on disk before the watcher fires
     `sdd:changed`). Also covers the initial mount via the
     `void workspaceId` read. */
  $effect(() => {
    void stageKind;
    void workspaceId;
    if (loaded) void refresh();
  });
  /* Initial load — happens once per mount so the parent header
     chip's count shows without the user opening the overlay. */
  $effect(() => {
    if (!loaded) void refresh();
  });

  function copyAsJsonl(): void {
    const text = filtered.map((e) => JSON.stringify(e)).join('\n');
    void navigator.clipboard.writeText(text);
  }

  function fmtTs(ts: number): string {
    try {
      const d = new Date(ts);
      // HH:MM:SS for compactness (date is shown in the day-grouped
      // header below if we ever group, which v1 doesn't).
      return d.toLocaleTimeString();
    } catch {
      return String(ts);
    }
  }

  function onKey(e: KeyboardEvent): void {
    if (e.key === 'Escape' && open) {
      e.preventDefault();
      open = false;
    }
  }
  $effect(() => {
    if (!open) return;
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  });
</script>

{#if open}
  <div class="sdd-audit-overlay" role="dialog" aria-label="SDD audit log">
    <header class="sdd-audit-head">
      <span class="sdd-audit-title">Audit log</span>
      <span class="sdd-audit-count mono">{filtered.length} of {entries.length}</span>
      <span class="sdd-audit-spacer"></span>
      <label class="sdd-audit-filter">
        <span class="vh">Filter by source</span>
        <select bind:value={filter} class="mono">
          <option value="all">all</option>
          <option value="agent">agent</option>
          <option value="user">user</option>
          <option value="system">system</option>
        </select>
      </label>
      <button class="sdd-btn sdd-btn--mute" type="button" onclick={copyAsJsonl}>Copy JSONL</button>
      <button class="sdd-btn sdd-btn--mute" type="button" onclick={() => (open = false)}>Close</button>
    </header>
    <div class="sdd-audit-body">
      {#if filtered.length === 0}
        <p class="sdd-audit-empty">No audit entries yet.</p>
      {:else}
        <ul class="sdd-audit-list">
          {#each filtered as e, idx (e.ts + e.action + (e.phase ?? -1) + '|' + idx)}
            {@const expanded = expandedTs === e.ts}
            <li class="sdd-audit-row" data-source={e.source}>
              <button
                type="button"
                class="sdd-audit-row-head"
                onclick={() => (expandedTs = expanded ? null : e.ts)}
                aria-expanded={expanded}
              >
                <span class="sdd-audit-ts mono">{fmtTs(e.ts)}</span>
                <span class="sdd-audit-source mono" data-source={e.source}>{e.source}</span>
                <span class="sdd-audit-action mono">{e.action}</span>
                {#if e.phase != null}
                  <span class="sdd-audit-phase mono">phase {e.phase}</span>
                {/if}
                {#if e.reason}
                  <span class="sdd-audit-reason">— {e.reason}</span>
                {/if}
              </button>
              {#if expanded && (e.before !== undefined || e.after !== undefined)}
                <div class="sdd-audit-snap mono">
                  {#if e.before !== undefined}
                    <details open>
                      <summary>before</summary>
                      <pre>{JSON.stringify(e.before, null, 2)}</pre>
                    </details>
                  {/if}
                  {#if e.after !== undefined}
                    <details open>
                      <summary>after</summary>
                      <pre>{JSON.stringify(e.after, null, 2)}</pre>
                    </details>
                  {/if}
                </div>
              {/if}
            </li>
          {/each}
        </ul>
      {/if}
    </div>
  </div>
{/if}

<style>
  .sdd-audit-overlay {
    margin: 8px 0 0 14px;
    padding: 8px 10px;
    border: 1px solid var(--border-neutral-hi);
    background: color-mix(in srgb, var(--bg-1) 92%, transparent);
    border-radius: 4px;
    font-size: 12px;
  }
  .sdd-audit-head {
    display: flex; align-items: center; gap: 8px;
    margin-bottom: 6px;
  }
  .sdd-audit-title { font-weight: 600; }
  .sdd-audit-count { font-size: 10.5px; color: var(--text-mute); }
  .sdd-audit-spacer { flex: 1; }
  .sdd-audit-filter select {
    background: var(--bg-0);
    border: 1px solid var(--border-neutral-hi);
    color: var(--text-0);
    font-size: 11px;
    padding: 1px 4px;
    border-radius: 3px;
  }
  .sdd-audit-empty {
    margin: 4px 0; color: var(--text-mute); font-style: italic;
  }
  .sdd-audit-list {
    list-style: none; margin: 0; padding: 0;
    max-height: 320px; overflow-y: auto;
  }
  .sdd-audit-row + .sdd-audit-row {
    border-top: 1px solid color-mix(in srgb, var(--border-neutral-hi) 50%, transparent);
  }
  .sdd-audit-row-head {
    appearance: none;
    width: 100%;
    display: flex; flex-wrap: wrap; align-items: baseline; gap: 6px;
    background: transparent; border: 0;
    padding: 4px 0;
    text-align: left;
    color: var(--text-0);
    cursor: pointer;
    font: inherit;
  }
  .sdd-audit-row-head:hover {
    background: color-mix(in srgb, var(--accent) 6%, transparent);
  }
  .sdd-audit-ts { font-size: 10.5px; color: var(--text-mute); }
  .sdd-audit-source {
    font-size: 10px; padding: 0 4px; border-radius: 2px;
    background: color-mix(in srgb, var(--text-mute) 18%, transparent);
    color: var(--text-0);
  }
  .sdd-audit-source[data-source="agent"]  { background: color-mix(in srgb, #d6743a 28%, transparent); }
  .sdd-audit-source[data-source="user"]   { background: color-mix(in srgb, #6a8fb5 28%, transparent); }
  .sdd-audit-source[data-source="system"] { background: color-mix(in srgb, #888 22%, transparent); }
  .sdd-audit-action { font-weight: 600; }
  .sdd-audit-phase { font-size: 10.5px; color: var(--text-mute); }
  .sdd-audit-reason { color: var(--text-mute); font-style: italic; }
  .sdd-audit-snap {
    padding: 4px 0 8px 14px;
    font-size: 11px;
  }
  .sdd-audit-snap pre {
    margin: 4px 0 0;
    padding: 6px 8px;
    background: var(--bg-0);
    border: 1px solid var(--border-neutral-hi);
    border-radius: 3px;
    overflow-x: auto;
    font-size: 10.5px;
    line-height: 1.4;
  }
  .sdd-audit-snap summary {
    cursor: pointer;
    color: var(--text-mute);
    font-size: 10.5px;
  }
  /* `sdd-btn` lives in the parent (SddCard) — we use the same class
     names so the buttons inherit the card's visual rhythm. Svelte
     scopes per-component, so we declare the minimal styles here too
     for the buttons rendered inside this overlay. */
  .sdd-btn {
    appearance: none;
    background: var(--bg-2); border: 1px solid var(--border-neutral-hi);
    color: var(--text-0);
    padding: 2px 8px;
    border-radius: 3px;
    font: inherit; font-size: 11px;
    cursor: pointer;
  }
  .sdd-btn--mute { background: transparent; color: var(--text-2); }
  .sdd-btn:hover { background: var(--bg-3); color: var(--text-0); }
  .vh {
    position: absolute; width: 1px; height: 1px; padding: 0; margin: -1px;
    overflow: hidden; clip: rect(0, 0, 0, 0); white-space: nowrap; border: 0;
  }
</style>
