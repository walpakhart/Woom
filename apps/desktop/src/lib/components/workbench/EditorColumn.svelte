<script lang="ts">
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
  import { sessionsState, updateSession } from '$lib/state/sessions.svelte';

  interface Props {
    instanceId: string;
    onLinkToAgent: (agentInstanceId: string) => void;
  }

  let { instanceId, onLinkToAgent }: Props = $props();

  // Sessions currently linked to THIS editor instance. Drives the
  // "Linked to <agent>" pills in the Editor header so the bidirectional
  // connection is visible from both sides. Resolves the agent's column
  // via `findInstanceAnywhere` so the link survives moving the editor or
  // the agent column to a different workbench — the link is between
  // identities, not workbench-bound.
  const linkedAgents = $derived.by(() => {
    const out: { sessionId: string; agentInstanceId: string; kind: 'claude' | 'cursor'; name: string }[] = [];
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
        name: col.name
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
  // Sync model: local `repoPath` is the write side. The store
  // (`sessionsState.editorInstanceState[instanceId].repoPath`) is a
  // persistence mirror — a one-way effect copies local → store on every
  // change. We previously also synced store → local, but that created a
  // race: a new path coming in via `bind:repoPath` from the EditorView
  // child would land on local first; the store-sync effect read the stale
  // store value and overwrote local back to '' before the local→store
  // effect could persist it. End result: clicking "Open folder" appeared
  // to do nothing. Initial hydration is now done once at mount via the
  // `$state(...)` initializer below.
  let repoPath = $state('');
  // Hydrate once on mount from whatever we last persisted for this column
  // instance. `instanceId` is stable per-mount (different ids produce a
  // fresh Svelte component via the `{#each}` keyed block), so a one-shot
  // pre-effect is the right shape — using a `$state(...)` initializer
  // would trip Svelte's `state_referenced_locally` warning since the
  // initializer captures the prop at construction time.
  let hydrated = false;
  $effect.pre(() => {
    if (hydrated) return;
    hydrated = true;
    if (!sessionsState.editorInstanceState[instanceId]) {
      sessionsState.editorInstanceState[instanceId] = { repoPath: '' };
    } else if (sessionsState.editorInstanceState[instanceId].repoPath) {
      repoPath = sessionsState.editorInstanceState[instanceId].repoPath;
    }
  });

  // One-way mirror: local → store. The local var is the authoritative
  // write side (mutated by `bind:repoPath` from EditorView when the user
  // picks a folder); the store is just a persistence layer so the panel
  // remembers the path across reloads / column re-mounts.
  $effect(() => {
    const slot = sessionsState.editorInstanceState[instanceId];
    if (slot && slot.repoPath !== repoPath) {
      slot.repoPath = repoPath;
    }
  });

  const inst = $derived(activeInstances().find((i) => i.id === instanceId));
  const order = $derived(activeInstances().findIndex((i) => i.id === instanceId));
</script>

<section
  class="wb-column wb-column--editor"
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
  <EditorView bind:repoPath {agentInstances} {linkedAgents} {onLinkToAgent} onUnlinkAgent={unlinkSession} />
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
    background: rgba(15, 24, 40, 0.4);
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
