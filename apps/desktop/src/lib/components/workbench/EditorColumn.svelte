<script lang="ts">
  import { untrack } from 'svelte';
  import { slide } from 'svelte/transition';
  import { cubicOut } from 'svelte/easing';
  import EditorView from '$lib/components/editor/EditorView.svelte';
  import ColumnControls from '$lib/components/workbench/ColumnControls.svelte';
  import {
    layoutState,
    startResizeById,
    activeInstances,
    findInstanceAnywhere
  } from '$lib/state/layout.svelte';
  import {
    sessionsState,
    updateSession,
    consumeEditorOpenFile
  } from '$lib/state/sessions.svelte';

  interface Props {
    instanceId: string;
    onLinkToAgent: (agentInstanceId: string) => void;
  }

  let { instanceId, onLinkToAgent }: Props = $props();

  // Sessions currently linked to THIS editor instance. Drives the
  // "Linked to <session>" pills in the Editor header so the bidirectional
  // connection is visible from both sides. Resolves the agent's column
  // via `findInstanceAnywhere` so the link survives moving the editor or
  // the agent column to a different workbench — the link is between
  // identities, not workbench-bound.
  //
  // `name` is the SESSION title (e.g. "Sagrada-Familia 2") not the
  // column's art-name — column = the surface, session = the actual
  // chat conversation. Showing the column name was misleading when a
  // user had multiple sessions in the same column: every link looked
  // identical even though they pointed at different threads.
  const linkedAgents = $derived.by(() => {
    const out: {
      sessionId: string;
      agentInstanceId: string;
      kind: 'claude' | 'cursor';
      name: string;       // session title — what the user sees in the chat tab
      columnName: string; // column art-name (kept for tooltip context)
    }[] = [];
    for (const s of sessionsState.list) {
      if (!s.linkedToEditor) continue;
      if (s.linkedToEditorInstanceId !== instanceId) continue;
      if (!s.columnInstanceId) continue;
      const found = findInstanceAnywhere(s.columnInstanceId);
      if (!found) continue;
      const col = found.inst;
      if (col.kind !== 'claude' && col.kind !== 'cursor') continue;
      out.push({
        sessionId: s.id,
        agentInstanceId: col.id,
        kind: col.kind as 'claude' | 'cursor',
        name: s.title || col.name,
        columnName: col.name
      });
    }
    return out;
  });

  function unlinkSession(sessionId: string) {
    updateSession(sessionId, { linkedToEditor: false, linkedToEditorInstanceId: null });
  }

  // Agent columns in the active workbench — the Editor's Link button uses
  // this to either link directly (single target) or surface a picker
  // (multiple agents open).
  const agentInstances = $derived(
    activeInstances()
      .filter((i) => i.kind === 'claude' || i.kind === 'cursor')
      .map((i) => ({ id: i.id, kind: i.kind as 'claude' | 'cursor', name: i.name }))
  );

  // Per-instance editor state. Lazily initialized so the Editor's own
  // `bind:repoPath` has a stable slot to mutate. Two Editor columns can
  // open different folders simultaneously.
  //
  // Sync model: bidirectional with a `lastSyncedFromStore` guard to
  // prevent the original race. Local is mutated by EditorView's
  // `bind:repoPath` (user picks folder); store is also a write target
  // for *external* writers (the agent's `mcp__app__set_editor_repo_path`
  // tool calls `setEditorRepoPath` in +page.svelte, which writes the
  // store directly). The guard tracks the most recent value either side
  // wrote so the two effects don't form a feedback loop.
  let repoPath = $state('');
  let lastSyncedFromStore = $state('');
  let hydrated = false;
  $effect.pre(() => {
    if (hydrated) return;
    hydrated = true;
    const slot = sessionsState.editorInstanceState[instanceId];
    if (!slot) {
      sessionsState.editorInstanceState[instanceId] = { repoPath: '' };
    } else if (slot.repoPath) {
      repoPath = slot.repoPath;
      lastSyncedFromStore = slot.repoPath;
    }
  });

  // Store → local. Adopt external writes (agent-driven set_editor_repo_path
  // / linked-agent path push). `untrack` on the local read keeps this
  // effect's deps to just the store, avoiding the feedback loop the
  // earlier code warned about (where a local change re-triggered this
  // effect with a stale store read and clobbered local).
  $effect(() => {
    const stored = sessionsState.editorInstanceState[instanceId]?.repoPath ?? '';
    if (stored === lastSyncedFromStore) return; // we wrote it via local→store
    lastSyncedFromStore = stored;
    untrack(() => {
      if (stored !== repoPath) repoPath = stored;
    });
  });

  // Local → store. Persists user-side picks + keeps the guard in sync so
  // the store→local effect doesn't bounce.
  $effect(() => {
    const slot = sessionsState.editorInstanceState[instanceId];
    if (slot && slot.repoPath !== repoPath) {
      slot.repoPath = repoPath;
      lastSyncedFromStore = repoPath;
    }
  });

  // EditorView reference — wired via bind:this so we can call its
  // public `openFile(path)` method when an external source (the diff
  // card's clickable file path) requests we focus a file. Without this,
  // setting `pendingOpenFile` in state would have no consumer.
  let editorView = $state<{ openFile: (path: string) => void } | null>(null);

  // pendingOpenFile signal → editorView.openFile. Drained immediately
  // via `consumeEditorOpenFile` so the next request (even for the same
  // path) re-fires; without that, a user clicking the same diff card
  // path twice in a row would only register the first click.
  //
  // Guarded on `editorView` because mount order isn't deterministic
  // relative to the first signal — a fast click on a freshly-spawned
  // editor column could race the bind:this assignment. The next change
  // to pendingOpenFile (or the next mount tick) re-runs this effect,
  // so a missed first signal isn't a permanent bug — just a one-tick
  // delay. (`requestEditorOpenFile` always reassigns the slot, even
  // for identical paths, so the effect re-evaluates.)
  $effect(() => {
    const pending = sessionsState.editorInstanceState[instanceId]?.pendingOpenFile;
    if (!pending || !editorView) return;
    editorView.openFile(pending);
    consumeEditorOpenFile(instanceId);
  });

  const inst = $derived(activeInstances().find((i) => i.id === instanceId));
  const order = $derived(activeInstances().findIndex((i) => i.id === instanceId));
</script>

<section
  class="wb-column wb-column--editor"
  class:wb-column--maximized={layoutState.maximizedInstanceId === instanceId}
  data-instance-id={instanceId}
  data-kind="editor"
  transition:slide={{ duration: 240, axis: 'x', easing: cubicOut }}
  style="order: {order}; flex: 0 0 {inst?.width ?? 720}px"
>
  <ColumnControls {instanceId} kind="editor" />
  <div class="wb-col-resize" class:snap-flash={layoutState.snapFlashInstanceId === instanceId} role="separator" aria-orientation="vertical" onpointerdown={(e) => startResizeById(instanceId, e)}></div>
  <div class="editor-bench-head">
    <span class="source-mark" aria-hidden="true">
      <svg viewBox="0 0 24 24"><path d="M3 7v11a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2V9a2 2 0 0 0-2-2h-7L10 5H5a2 2 0 0 0-2 2z"/></svg>
    </span>
    <span class="brand-word">Editor</span>
    {#if inst?.name}<span class="bench-name mono" title="Bench id">{inst.name}</span>{/if}
  </div>
  <EditorView bind:this={editorView} bind:repoPath {agentInstances} {linkedAgents} {onLinkToAgent} onUnlinkAgent={unlinkSession} />
</section>

<style>
  /* Editor column uses generic .wb-column rules from +page.svelte. The
     column-specific background + child-height rule is here. */
  .wb-column--editor { background: var(--bg-0); min-width: 420px; display: flex; flex-direction: column; }
  .wb-column--editor :global(.ev) { flex: 1; min-height: 0; }
  /* Matches `.inbox-brand` height from AgentColumn / GithubColumn / JiraColumn:
     padding 16/20/10 + a 22px source-mark badge, so workbench column headers
     line up across kinds. */
  .editor-bench-head {
    display: flex; align-items: center; gap: 10px;
    padding: 16px 20px 10px;
    border-bottom: 1px solid var(--border-neutral);
    background: var(--bg-1);
    flex-shrink: 0;
  }
  .editor-bench-head .source-mark {
    width: 22px; height: 22px; border-radius: 5px;
    display: inline-flex; align-items: center; justify-content: center;
    background: var(--bg-2); color: var(--text-1);
    border: 1px solid var(--border-neutral-hi);
    flex-shrink: 0;
  }
  .editor-bench-head .source-mark svg {
    width: 12px; height: 12px;
    stroke: currentColor; fill: none;
    stroke-width: 2; stroke-linecap: round; stroke-linejoin: round;
  }
  .editor-bench-head .brand-word {
    font-size: 14px; font-weight: 600;
    color: var(--text-0); letter-spacing: -0.01em;
  }
</style>
