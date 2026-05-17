<script lang="ts">
  /* RailAppButton — a rail icon for a multi-instance app (Editor /
     Canvas / Terminal). The kind has a single primary instance + an
     optional list of secondary instances. Single-click on the primary
     icon switches the view and activates that instance. When more
     than one instance exists, a chevron appears at the bottom of the
     primary icon; clicking it expands the rail inline so each extra
     instance gets its own rail-btn directly under the primary. From
     the expanded stack the user can add (+ button) or remove (× on
     hover) instances without leaving the rail. The previous popover
     modal is gone — everything lives in the rail itself. */

  import {
    layoutState,
    addInstance,
    removeInstance,
    setActiveInstance,
    type AppKind
  } from '$lib/state/layout.svelte';
  import { sessionsState } from '$lib/state/sessions.svelte';
  import type { Snippet } from 'svelte';

  /** Just the base folder name — best inline label for "what repo is
   *  this editor showing". Falls back to empty when the instance has
   *  no path bound yet. */
  function folderName(path: string | null | undefined): string {
    if (!path) return '';
    const parts = path.replace(/\/$/, '').split('/');
    return parts[parts.length - 1] || '';
  }

  interface Props {
    kind: AppKind;
    /** Pretty label used in tooltips. */
    label: string;
    /** Tooltip + cmd-N hint, e.g. "Editor · ⌘6". */
    tooltip: string;
    /** True when this kind is the currently active top-level view. */
    active: boolean;
    /** rail-btn brand tone CSS — same vars the existing buttons use. */
    tone: string;
    glow: string;
    /** Inline `<svg>` icon snippet (so each kind keeps its own glyph). */
    icon: Snippet;
    /** Switches the top-level view to this kind. The parent +page.svelte
     *  reads `layoutState.activeInstance[kind]` to pick the actual instance,
     *  so we set the active-instance pointer before calling. */
    onActivate: () => void;
    /** Optional drop handler. When provided, the primary icon accepts
     *  drag payloads and renders the `rail-dropping` highlight while
     *  one is hovering. Used by Canvas so users can drag a card from
     *  any inbox / chat-message directly onto the rail icon without
     *  first switching view. */
    onDropPayload?: (e: DragEvent) => void;
  }
  let p: Props = $props();

  const instances = $derived(layoutState.instances[p.kind] ?? []);
  const activeId = $derived(layoutState.activeInstance[p.kind]);
  const primaryInst = $derived(instances.find((i) => i.primary) ?? instances[0]);
  const nonPrimary = $derived(instances.filter((i) => !i.primary));
  const hasExtras = $derived(instances.length > 1);

  /** Compose tooltip text for one instance — editor instances enrich
   *  the curated name with the folder they're currently bound to so
   *  hover reveals the repo at a glance ("Editor · Klimt · woom").
   *  Non-editor kinds keep the base "<Label> · <name>" pattern. */
  function tooltipFor(inst: { id: string; name: string } | undefined): string {
    if (!inst) return p.tooltip;
    if (p.kind === 'editor') {
      const repoPath = sessionsState.editorInstanceState[inst.id]?.repoPath ?? '';
      const folder = folderName(repoPath);
      if (folder) {
        // `data-tooltip` is rendered single-line by `.rail-btn[data-tooltip]:hover::before`;
        // keep it compact — folder name only ("Editor · Klimt · woom").
        return `${p.label} · ${inst.name} · ${folder}`;
      }
    }
    return `${p.label} · ${inst.name}`;
  }

  /** Folder-name suffix for the primary icon's tooltip. Empty string
   *  when the editor isn't bound to a folder so the template renders
   *  the base tooltip unchanged. */
  function primaryFolderSuffix(): string {
    if (p.kind !== 'editor' || !primaryInst) return '';
    const f = folderName(sessionsState.editorInstanceState[primaryInst.id]?.repoPath ?? '');
    return f ? ` · ${f}` : '';
  }

  /** Whether the secondary instance icons are visible inline. Collapses
   *  automatically when extras drop back to 0 (e.g. user × the last
   *  non-primary) so the rail doesn't carry a dead chevron state. */
  let expanded = $state(false);
  $effect(() => {
    if (!hasExtras) expanded = false;
  });

  let dragHoverTimer: ReturnType<typeof setTimeout> | null = null;
  /** True while a drop-acceptable payload hovers the primary icon. Drives
   *  the `rail-dropping` highlight so the user sees Canvas (etc.) is a
   *  valid drop target — same vocabulary the Claude / Cursor rail uses. */
  let dropOver = $state(false);

  /** WebKit hides `application/x-woom-*` mimes during dragover; mirror
   *  the predicate from Rail.svelte so the primary rail-btn accepts
   *  the same set of payloads the column drop targets handle. */
  function hasDropPayload(e: DragEvent): boolean {
    if (!p.onDropPayload) return false;
    const types = e.dataTransfer?.types;
    if (!types) return false;
    return (
      types.indexOf('Files') !== -1 ||
      types.indexOf('text/uri-list') !== -1 ||
      types.indexOf('text/plain') !== -1 ||
      types.indexOf('application/x-woom-file') !== -1 ||
      types.indexOf('application/x-woom-jira') !== -1 ||
      types.indexOf('application/x-woom-github') !== -1 ||
      types.indexOf('application/x-woom-sentry') !== -1
    );
  }

  /** Map kind → `data-view` value the parent Rail's CSS uses to scope
   *  drag-pulse + active-halo selectors. Mirrors the literals on the
   *  singleton rail buttons (Claude / Cursor / Editor / etc.). */
  function viewForKind(kind: AppKind): string {
    return `${kind}App`;
  }

  function activate(id: string) {
    setActiveInstance(p.kind, id);
    p.onActivate();
  }
  function onClickPrimary() {
    if (primaryInst) activate(primaryInst.id);
  }
  /** Right-click on any icon = add a new instance fast. Skips the
   *  expand step entirely; the new instance becomes active. */
  function onContextMenu(e: MouseEvent) {
    e.preventDefault();
    spawnInstance();
  }
  function toggleExpand(e: MouseEvent) {
    e.stopPropagation();
    expanded = !expanded;
  }
  function spawnInstance() {
    const inst = addInstance(p.kind);
    if (inst) {
      expanded = true;
      p.onActivate();
    }
  }
  function discardInstance(id: string, name: string, e: MouseEvent) {
    e.stopPropagation();
    if (!confirm(`Close ${p.label} · ${name}? Any unsaved per-instance state is dropped.`)) return;
    removeInstance(p.kind, id);
  }

  /** Drag-hover expand. A 450 ms dwell over the primary icon opens the
   *  inline stack so a payload can be dropped onto a specific instance.
   *  Cancelled on dragleave. */
  function onDragEnterPrimary(e: DragEvent) {
    if (!e.dataTransfer) return;
    if (hasDropPayload(e)) {
      e.preventDefault();
      dropOver = true;
    }
    if (!hasExtras) return;
    if (dragHoverTimer) clearTimeout(dragHoverTimer);
    dragHoverTimer = setTimeout(() => {
      expanded = true;
      dragHoverTimer = null;
    }, 450);
  }
  function onDragOverPrimary(e: DragEvent) {
    /* preventDefault on dragover is what *enables* the drop — without
     *  it WebKit reports "no-drop" and the drop event never fires on
     *  release. */
    if (!hasDropPayload(e)) return;
    e.preventDefault();
    if (e.dataTransfer) e.dataTransfer.dropEffect = 'copy';
    if (!dropOver) dropOver = true;
  }
  function onDragLeavePrimary() {
    if (dragHoverTimer) {
      clearTimeout(dragHoverTimer);
      dragHoverTimer = null;
    }
    dropOver = false;
  }
  function onDropPrimary(e: DragEvent) {
    dropOver = false;
    if (!p.onDropPayload) return;
    e.preventDefault();
    /* Switch view to this kind so the surface mounts and can drain the
     *  queued payload from module state. */
    p.onActivate();
    p.onDropPayload(e);
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === 'Escape' && expanded) expanded = false;
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="rab" class:expanded>
  <div class="rab-slot">
    <button
      class="rail-btn rab-btn"
      class:active={p.active && activeId === primaryInst?.id}
      class:kind-active={p.active && activeId !== primaryInst?.id}
      class:has-extras={hasExtras}
      class:rail-dropping={dropOver}
      style="--rail-tone: {p.tone}; --rail-glow: {p.glow};"
      data-tooltip={hasExtras
        ? `${p.tooltip} · ${primaryInst?.name}${primaryFolderSuffix()}`
        : `${p.tooltip}${primaryFolderSuffix()}`}
      aria-label={p.label}
      data-view={viewForKind(p.kind)}
      onclick={onClickPrimary}
      oncontextmenu={onContextMenu}
      ondragenter={onDragEnterPrimary}
      ondragover={onDragOverPrimary}
      ondragleave={onDragLeavePrimary}
      ondrop={onDropPrimary}
    >
      {@render p.icon()}
    </button>
    {#if hasExtras}
      <button
        class="rab-chevron"
        class:open={expanded}
        class:kind-active={p.active}
        style="--rail-tone: {p.tone}; --rail-glow: {p.glow};"
        onclick={toggleExpand}
        aria-label={expanded ? `Collapse ${p.label} stack` : `Expand ${p.label} stack`}
        title={expanded ? 'Collapse' : `${instances.length} ${p.label.toLowerCase()}s open`}
      >
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <polyline points="6 9 12 15 18 9" />
        </svg>
      </button>
    {/if}
  </div>

  {#if expanded}
    <div class="rab-stack">
      {#each nonPrimary as inst (inst.id)}
        {@const isActive = p.active && activeId === inst.id}
        <div class="rab-slot rab-sub">
          <button
            class="rail-btn rab-btn rab-sub-btn"
            class:active={isActive}
            style="--rail-tone: {p.tone}; --rail-glow: {p.glow};"
            data-tooltip={tooltipFor(inst)}
            aria-label="{p.label} {inst.name}"
            onclick={() => activate(inst.id)}
            oncontextmenu={onContextMenu}
          >
            {@render p.icon()}
          </button>
          <button
            class="rab-x"
            onclick={(e) => discardInstance(inst.id, inst.name, e)}
            aria-label="Close {p.label} {inst.name}"
            title="Close {inst.name}"
          >
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" aria-hidden="true">
              <path d="M6 6l12 12M6 18 18 6" />
            </svg>
          </button>
        </div>
      {/each}
      <button
        class="rab-add"
        style="--rail-tone: {p.tone}; --rail-glow: {p.glow};"
        onclick={spawnInstance}
        aria-label="New {p.label.toLowerCase()}"
        title="New {p.label.toLowerCase()}"
      >
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" aria-hidden="true">
          <path d="M12 5v14M5 12h14" />
        </svg>
      </button>
    </div>
  {/if}
</div>

<style>
  /* Wraps the primary slot + the optional expanded sub-stack. Lays them
     out as a column matching the parent rail's own flex gap so spacing
     between RailAppButton entries reads as one continuous rail. */
  .rab {
    position: relative;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
  }

  /* One row of the stack — primary or sub. The primary slot stacks
     its button + chevron vertically (in flow, so the slot's height
     reserves the chevron's space and the next rail entry below
     doesn't get crowded). Sub slots are just the button; the × badge
     overlays absolutely. */
  .rab-slot {
    position: relative;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 1px;
  }

  .rab-btn { position: relative; }

  /* Hide the .rail-btn.active::after accent dot when this kind has
     extras — the chevron replaces it. Keeps the bottom-of-icon area
     uncluttered. */
  :global(.rab-btn.has-extras.active::after) { display: none; }

  /* The kind is the current view but the active instance is a sub —
     give the primary icon a subtle border tint so the rail still
     telegraphs "you're somewhere in Editor". The chevron carries the
     louder accent. */
  :global(.rab-btn.kind-active:not(.active)) {
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--rail-tone) 22%, transparent);
  }

  /* Chevron sits below the primary icon as a quiet hint — no border, no
     filled pill, just a small glyph at low opacity. The whole 22×10 box
     is the hit area, but only the 8px glyph reads, so it doesn't fight
     the icon above for attention. Brightens on hover and inherits the
     kind tone when the kind is the current view. */
  .rab-chevron {
    width: 24px;
    height: 10px;
    display: grid;
    place-items: center;
    padding: 0;
    border: 0;
    background: transparent;
    color: var(--text-mute);
    opacity: 0.55;
    cursor: pointer;
    transition: opacity 140ms, color 140ms;
  }
  .rab-chevron svg {
    width: 9px;
    height: 9px;
    transition: transform var(--dur-slow) var(--ease-spring);
  }
  .rab-chevron:hover {
    opacity: 1;
    color: var(--rail-tone);
  }
  .rab-chevron.kind-active {
    opacity: 1;
    color: var(--rail-tone);
  }
  .rab-chevron.open {
    opacity: 1;
  }
  .rab-chevron.open svg {
    transform: rotate(180deg);
  }

  /* Sub-instance stack — pads a touch on top so the chevron's anchor
     doesn't overlap the first sub icon. */
  .rab-stack {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    padding-top: 4px;
    animation: rab-slide var(--dur-base) var(--ease-spring);
  }
  @keyframes rab-slide {
    from { opacity: 0; transform: translateY(-6px); }
    to   { opacity: 1; transform: translateY(0); }
  }
  @media (prefers-reduced-motion: reduce) {
    .rab-stack { animation: none; }
    .rab-chevron, .rab-chevron svg { transition: none; }
  }

  /* Sub icons render the same kind glyph at a slightly smaller chassis
     so the primary stays visually dominant. Same rail-btn class, so
     hover / active / glow vocabulary is preserved. */
  :global(.rab-sub-btn) {
    width: 38px !important;
    height: 38px !important;
    border-radius: 9px !important;
  }
  :global(.rab-sub-btn svg) { width: 17px; height: 17px; }

  /* × badge on the sub row — only visible on hover. Sits at the top-
     right of the sub icon's slot so it doesn't fight the click area. */
  .rab-x {
    position: absolute;
    top: -4px;
    right: -6px;
    width: 18px;
    height: 18px;
    display: grid;
    place-items: center;
    padding: 0;
    border: 1px solid var(--border-neutral-hi);
    border-radius: 50%;
    background: var(--bg-3);
    color: var(--text-mute);
    cursor: pointer;
    opacity: 0;
    transition: opacity 120ms, color 120ms, background 120ms, transform 120ms;
    z-index: 3;
  }
  .rab-x svg { width: 9px; height: 9px; }
  .rab-sub:hover .rab-x,
  .rab-x:focus-visible {
    opacity: 1;
  }
  .rab-x:hover {
    color: var(--error);
    background: color-mix(in srgb, var(--error) 14%, var(--bg-3));
    border-color: color-mix(in srgb, var(--error) 50%, var(--border-neutral-hi));
    transform: scale(1.06);
  }

  /* "+ new instance" button at the bottom of the expanded stack. Dashed
     ring + plus glyph so it reads as an "add slot" affordance rather
     than another live icon. */
  .rab-add {
    width: 38px;
    height: 38px;
    display: grid;
    place-items: center;
    padding: 0;
    border: 1px dashed color-mix(in srgb, var(--border-neutral-hi) 90%, transparent);
    border-radius: 9px;
    background: transparent;
    color: var(--text-mute);
    cursor: pointer;
    transition: all 140ms;
  }
  .rab-add svg { width: 14px; height: 14px; }
  .rab-add:hover {
    color: var(--rail-tone);
    border-color: color-mix(in srgb, var(--rail-tone) 60%, transparent);
    background: color-mix(in srgb, var(--rail-tone) 10%, transparent);
    box-shadow: 0 0 12px color-mix(in srgb, var(--rail-glow) 60%, transparent);
  }
</style>
