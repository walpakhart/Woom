<script lang="ts">
  /* WorktreeBar — thin strip under ChatHeader.
     cwd chip + clear button + editor-link chip / picker + worktree
     menu. Effectively the standalone successor to the old cwd-bar
     that used to live inside AgentApp. */
  import { sessionsState, focusSession } from '$lib/state/sessions.svelte';
  import { APP_INSTANCE_IDS, layoutState } from '$lib/state/layout.svelte';
  import { canvasState } from '$lib/state/canvas.svelte';
  import Dropdown from '$lib/components/ui/Dropdown.svelte';
  import { cubicOut } from 'svelte/easing';

  /** Custom slide-down + fade transition for the mismatch menu. Pure
   *  CSS keyframes would do the trick visually, but using a Svelte
   *  transition lets us match enter and exit symmetrically (out:
   *  collapses back into the chip on dismiss instead of popping out). */
  function slideFade(_: Element, { duration = 160 }: { duration?: number } = {}) {
    return {
      duration,
      easing: cubicOut,
      css: (t: number) =>
        `opacity: ${t}; transform: translateY(${(1 - t) * -6}px) scaleY(${0.96 + t * 0.04}); transform-origin: top left;`
    };
  }

  type Kind = 'claude' | 'cursor';

  interface Props {
    kind: Kind;
    editorRepoPath: string;
    onPickCwd: () => void;
    onClearCwd: () => void;
    onToggleEditorLink: () => void;
    onLinkToEditorInstance: (id: string) => void;
    /** Move agent cwd onto the linked editor's repoPath — one of the
     *  two choices in the orange "Folder mismatch" menu. */
    onSyncAgentToEditor?: () => void;
    /** Move the linked editor's repoPath onto the agent cwd/worktree
     *  — the other choice in the same menu. */
    onSyncEditorToAgent?: () => void;
    /** Drop the active session's terminal link (sets
     *  `linkedTerminalInstanceId` to null). Same shape as
     *  `onToggleEditorLink` for consistency — the bar handles the chip
     *  click; the parent decides what "unlink" means semantically. */
    onToggleTerminalLink?: () => void;
    /** Bind the active session to a specific terminal instance.
     *  Optional so callers that don't want to surface the link
     *  affordance can omit it; when undefined, the bar simply hides
     *  the picker. */
    onLinkToTerminalInstance?: (id: string) => void;
    onCreateWorktree: () => void;
    onOpenWorktreeDiff: () => void;
    onRemoveWorktree: () => void;
    worktreeBusy: 'creating' | 'removing' | null;
    /** Unlink the active session from its canvas (sets linkedCanvasId = null). */
    onToggleCanvasLink?: () => void;
    /** Link the active session to a specific canvas document by ID. */
    onLinkToCanvas?: (canvasId: string) => void;
  }
  let p: Props = $props();

  const sess = $derived(
    sessionsState.list.find((s) => s.id === sessionsState.activeIds[p.kind]) ?? null
  );

  /** Last path segment from an absolute repo path. Drives the
   *  "Link to Vermeer (woom)" picker label and the linked-chip suffix
   *  so the user can spot at a glance which repo each editor instance
   *  currently has open. */
  function folderName(p: string | null | undefined): string {
    if (!p) return '';
    const trimmed = p.replace(/\/+$/, '');
    const slash = trimmed.lastIndexOf('/');
    return slash >= 0 ? trimmed.slice(slash + 1) : trimmed;
  }

  /** All editor instances currently open — pulled live from
   *  `layoutState` so the picker reflects every Vermeer / Rothko
   *  spawned via the rail's long-press menu. Each entry carries the
   *  open folder name so the picker reads "Link to <editor> (<folder>)"
   *  and the user picks the right one without bouncing through the
   *  editor solo. */
  const editorInstances = $derived(
    layoutState.instances.editor.map((i) => {
      const repoPath = sessionsState.editorInstanceState[i.id]?.repoPath ?? '';
      return { id: i.id, name: i.name, repoPath, folder: folderName(repoPath) };
    })
  );

  /** The chip shown on the cwd bar when the active session is linked
   *  to one of those editors. Match by id rather than the legacy
   *  primary constant so secondary instances show their curated name. */
  const linkedEditor = $derived.by(() => {
    if (!sess?.linkedToEditor || !sess.linkedToEditorInstanceId) return null;
    const inst = layoutState.instances.editor.find(
      (i) => i.id === sess.linkedToEditorInstanceId
    );
    if (!inst) return null;
    const repoPath = sessionsState.editorInstanceState[inst.id]?.repoPath ?? '';
    return { id: inst.id, name: inst.name, repoPath, folder: folderName(repoPath) };
  });

  /** Active agent's "owned" folder — worktree wins over cwd. This is
   *  what the "Folder mismatch" menu will offer to push onto the
   *  editor side. */
  const agentFolder = $derived(sess?.worktreePath || sess?.cwd || '');

  /** Mismatch: link is active, both sides have a non-empty folder, but
   *  they differ. If one side is empty we don't show the pulse —
   *  that's a "not configured yet" state and the link itself will
   *  have already adopted the populated side. */
  const folderMismatch = $derived(
    !!linkedEditor &&
    !!linkedEditor.repoPath &&
    !!agentFolder &&
    linkedEditor.repoPath !== agentFolder
  );

  /** Whether the mismatch resolution menu is open. */
  let mismatchOpen = $state(false);
  let mismatchWrapEl = $state<HTMLDivElement | null>(null);

  $effect(() => {
    if (!mismatchOpen) return;
    function onDown(e: MouseEvent) {
      if (mismatchWrapEl && !mismatchWrapEl.contains(e.target as Node)) {
        mismatchOpen = false;
      }
    }
    window.addEventListener('mousedown', onDown);
    return () => window.removeEventListener('mousedown', onDown);
  });

  // Auto-close the menu when the mismatch resolves (right after one
  // of the two options is picked) so it doesn't linger with stale
  // paths.
  $effect(() => {
    if (!folderMismatch) mismatchOpen = false;
  });

  function pickUseEditor() {
    mismatchOpen = false;
    p.onSyncAgentToEditor?.();
  }
  function pickUseAgent() {
    mismatchOpen = false;
    p.onSyncEditorToAgent?.();
  }

  /** All terminal instances currently open — used for the "Link
   *  terminal…" picker. Mirror of `editorInstances` above. */
  const terminalInstances = $derived(
    layoutState.instances.terminal.map((i) => ({ id: i.id, name: i.name }))
  );

  /** The terminal-link chip data (or null when the active session
   *  isn't bound to a terminal). Match-by-id so spawned terminals
   *  surface their curated name (Hopper / Hokusai / …). */
  const linkedTerminal = $derived.by(() => {
    if (!sess?.linkedTerminalInstanceId) return null;
    const inst = layoutState.instances.terminal.find(
      (i) => i.id === sess.linkedTerminalInstanceId
    );
    return inst ? { id: inst.id, name: inst.name } : null;
  });

  function shortPath(p: string | null | undefined): string {
    if (!p) return '';
    const home = '/Users/';
    if (p.startsWith(home)) {
      const rest = p.slice(home.length);
      const slash = rest.indexOf('/');
      return slash >= 0 ? `~${rest.slice(slash)}` : '~';
    }
    return p;
  }

  /** All active (non-archived) canvases for the picker. */
  const canvases = $derived(
    canvasState.index.filter((c) => !c.archivedAt)
  );

  /** The canvas this session is linked to, if any. */
  const linkedCanvas = $derived.by(() => {
    if (!sess?.linkedCanvasId) return null;
    return canvasState.index.find((c) => c.id === sess!.linkedCanvasId) ?? null;
  });

  function focusLocal() {
    if (sess) focusSession(sess.id);
  }
</script>

{#if sess}
  <div class="wb" class:wb--linked={sess.linkedToEditor}>
    <button
      class="wb-chip"
      class:wb-chip--has={!!sess.cwd}
      class:wb-chip--linked={sess.linkedToEditor || (!sess.cwd && p.editorRepoPath)}
      class:wb-chip--muted={!!sess.worktreePath}
      onclick={() => { focusLocal(); p.onPickCwd(); }}
      title={sess.worktreePath
        ? `Overridden by worktree below`
        : (sess.cwd ?? (p.editorRepoPath ? `Editor folder: ${p.editorRepoPath}` : 'Pick working directory'))}
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7"><path d="M3 7v11a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2V9a2 2 0 0 0-2-2h-7L10 5H5a2 2 0 0 0-2 2z"/></svg>
      <span class="wb-chip-label mono">
        {#if sess.cwd}{shortPath(sess.cwd)}
        {:else if p.editorRepoPath}↳ {shortPath(p.editorRepoPath)}
        {:else}No folder
        {/if}
      </span>
    </button>

    {#if sess.cwd}
      <button class="wb-x" onclick={() => { focusLocal(); p.onClearCwd(); }} title="Clear folder override" aria-label="Clear">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M18 6 6 18M6 6l12 12"/></svg>
      </button>
    {/if}

    {#if sess.linkedToEditor && linkedEditor}
      <button
        class="wb-link"
        onclick={() => { focusLocal(); p.onToggleEditorLink(); }}
        title={linkedEditor.repoPath
          ? `Linked to ${linkedEditor.name} — ${linkedEditor.repoPath}\nClick to unlink`
          : `Linked to ${linkedEditor.name} — click to unlink`}
      >
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M16 18l6-6-6-6M8 6l-6 6 6 6"/></svg>
        <span>{linkedEditor.name}</span>
        {#if linkedEditor.folder}
          <span class="wb-link-folder mono">({linkedEditor.folder})</span>
        {/if}
        <svg class="wb-link-x" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M18 6 6 18M6 6l12 12"/></svg>
      </button>

      {#if folderMismatch}
        <div class="wb-mismatch-wrap" bind:this={mismatchWrapEl}>
          <button
            class="wb-mismatch"
            class:wb-mismatch--open={mismatchOpen}
            onclick={() => (mismatchOpen = !mismatchOpen)}
            title={`Folder mismatch:\n  agent: ${agentFolder}\n  editor: ${linkedEditor.repoPath}`}
          >
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 9v4"/><circle cx="12" cy="17" r="0.6" fill="currentColor"/><path d="M10.29 3.86 1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z"/></svg>
            <span>Folder mismatch</span>
          </button>
          {#if mismatchOpen}
            <div class="wb-mismatch-menu" role="menu" transition:slideFade={{ duration: 160 }}>
              <div class="wb-mismatch-head">
                Pick which side to keep:
              </div>
              <button class="wb-mismatch-opt" role="menuitem" onclick={pickUseEditor}>
                <div class="wb-mismatch-opt-side">Use editor's folder</div>
                <div class="wb-mismatch-opt-path mono">{linkedEditor.folder || linkedEditor.repoPath}</div>
                <div class="wb-mismatch-opt-hint">agent moves here</div>
              </button>
              <button class="wb-mismatch-opt" role="menuitem" onclick={pickUseAgent}>
                <div class="wb-mismatch-opt-side">Use agent's folder</div>
                <div class="wb-mismatch-opt-path mono">{folderName(agentFolder) || agentFolder}</div>
                <div class="wb-mismatch-opt-hint">editor opens it</div>
              </button>
            </div>
          {/if}
        </div>
      {/if}
    {:else if editorInstances.length > 0}
      <div class="wb-link-picker">
        <Dropdown
          value=""
          options={editorInstances.map((e) => ({
            value: e.id,
            label: e.folder ? `Link to ${e.name} (${e.folder})` : `Link to ${e.name}`,
            hint: e.repoPath || undefined
          }))}
          onChange={(id) => { focusLocal(); p.onLinkToEditorInstance(id); }}
          placeholder="Link editor…"
          ariaLabel="Link to editor"
        />
      </div>
    {/if}

    <!-- Terminal link — same chip / picker pair as the editor link
         but bound to the session's `linkedTerminalInstanceId`. Wired
         to `linkSessionToTerminal` / `unlinkSessionFromTerminal` in
         +page.svelte. Hidden entirely when the parent doesn't pass
         the `onLinkToTerminalInstance` prop, so views that don't want
         the affordance (e.g. embed contexts) get the old layout. -->
    {#if sess.linkedTerminalInstanceId && linkedTerminal && p.onToggleTerminalLink}
      <button class="wb-link wb-link--term" onclick={() => { focusLocal(); p.onToggleTerminalLink?.(); }} title="Linked to Terminal — click to unlink">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><polyline points="4 17 10 11 4 5"/><line x1="12" y1="19" x2="20" y2="19"/></svg>
        <span>{linkedTerminal.name}</span>
        <svg class="wb-link-x" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M18 6 6 18M6 6l12 12"/></svg>
      </button>
    {:else if p.onLinkToTerminalInstance && terminalInstances.length > 0}
      <div class="wb-link-picker">
        <Dropdown
          value=""
          options={terminalInstances.map((t) => ({ value: t.id, label: `Link to ${t.name}` }))}
          onChange={(id) => { focusLocal(); p.onLinkToTerminalInstance?.(id); }}
          placeholder="Link terminal…"
          ariaLabel="Link to terminal"
        />
      </div>
    {/if}

    <!-- Canvas link — chip when linked, picker dropdown when not.
         Linking is per-canvas-document (linkedCanvasId), not per-instance. -->
    {#if sess.linkedCanvasId && linkedCanvas && p.onToggleCanvasLink}
      <button class="wb-link wb-link--canvas" onclick={() => { focusLocal(); p.onToggleCanvasLink?.(); }} title="Linked to Canvas — click to unlink">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7"><rect x="3" y="3" width="18" height="18" rx="3"/><circle cx="8" cy="8" r="1.5" fill="currentColor"/><path d="M3 13h18M13 3v18"/></svg>
        <span>{linkedCanvas.name}</span>
        <svg class="wb-link-x" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M18 6 6 18M6 6l12 12"/></svg>
      </button>
    {:else if p.onLinkToCanvas && canvases.length > 0}
      <div class="wb-link-picker">
        <Dropdown
          value=""
          options={canvases.map((c) => ({ value: c.id, label: `Link to ${c.name}` }))}
          onChange={(id) => { focusLocal(); p.onLinkToCanvas?.(id); }}
          placeholder="Link canvas…"
          ariaLabel="Link to canvas"
        />
      </div>
    {/if}

    <div class="wb-spacer"></div>

    <!-- Show only the active-branch chip when a worktree exists. The
         "+ Create worktree" CTA lives in the right-hand WorktreeSide
         pane; keeping it here too was redundant. -->
    {#if sess.worktreePath}
      <button class="wb-worktree" onclick={p.onOpenWorktreeDiff} title="Open worktree diff">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7"><circle cx="6" cy="6" r="2.5"/><circle cx="18" cy="18" r="2.5"/><path d="M6 8.5V14a4 4 0 0 0 4 4h6"/></svg>
        <span>{sess.worktreeBranch ?? 'worktree'}</span>
      </button>
    {/if}
  </div>
{/if}

<style>
  .wb {
    flex: 0 0 38px;
    display: flex; align-items: center; gap: 8px;
    padding: 0 22px;
    border-bottom: 1px solid var(--border);
    background: var(--bg-1);
    min-height: 0;
    font-size: 11.5px;
    color: var(--text-1);
  }
  .wb--linked {
    background: linear-gradient(180deg, color-mix(in srgb, var(--accent) 4%, transparent), transparent);
  }

  .wb-chip {
    display: inline-flex; align-items: center; gap: 6px;
    padding: 4px 10px;
    border-radius: 7px;
    background: var(--bg-2);
    border: 1px solid var(--border);
    color: var(--text-1);
    font-size: 11.5px;
    cursor: pointer;
    transition: border-color 140ms, background 140ms, color 140ms;
  }
  .wb-chip:hover { color: var(--text-0); border-color: var(--border-hi); }
  .wb-chip--has { color: var(--text-0); border-color: var(--border-hi); }
  .wb-chip--linked {
    color: var(--accent-bright);
    background: var(--accent-soft);
    border-color: var(--border-accent-2);
  }
  .wb-chip--muted { opacity: 0.6; }
  .wb-chip svg { width: 12px; height: 12px; flex-shrink: 0; }
  .wb-chip-label { font-size: 11px; }

  .wb-x {
    width: 20px; height: 20px;
    display: grid; place-items: center;
    color: var(--text-mute);
    background: transparent; border: none; cursor: pointer;
    border-radius: 4px;
  }
  .wb-x:hover { color: var(--error); background: var(--bg-2); }
  .wb-x svg { width: 11px; height: 11px; }

  .wb-link {
    display: inline-flex; align-items: center; gap: 5px;
    padding: 4px 9px;
    border-radius: 7px;
    background: var(--accent-soft);
    border: 1px solid var(--border-accent-2);
    color: var(--accent-bright);
    font-size: 11px;
    cursor: pointer;
  }
  .wb-link {
    transition: background 160ms, border-color 160ms, transform 120ms;
  }
  .wb-link:hover {
    background: color-mix(in srgb, var(--accent) 12%, transparent);
    transform: translateY(-0.5px);
  }
  .wb-link:active { transform: translateY(0); }
  .wb-link svg { width: 11px; height: 11px; }
  .wb-link-x { opacity: 0; transition: opacity 160ms; }
  .wb-link:hover .wb-link-x { opacity: 0.75; }
  .wb-link-folder {
    /* Folder name inside the chip — slightly muted vs the editor
       name so the eye anchors on "Vermeer" first and reads "(woom)"
       as the secondary hint. */
    color: color-mix(in srgb, var(--accent-bright) 65%, var(--text-mute));
    font-size: 10.5px;
    opacity: 0.85;
  }

  /* "Folder mismatch" button — orange, pulsing so it stands out among
     the other chips in the bar. The pulse animates background +
     border-color + box-shadow only, so the chip's box doesn't shift
     and surrounding layout stays stable. Hover and open states cut the
     pulse — once the user is engaging with the menu the chip stops
     drawing attention to itself. */
  .wb-mismatch-wrap { position: relative; }
  .wb-mismatch {
    display: inline-flex; align-items: center; gap: 5px;
    padding: 4px 9px;
    border-radius: 7px;
    background: color-mix(in srgb, #f0a050 18%, transparent);
    border: 1px solid color-mix(in srgb, #f0a050 50%, transparent);
    color: #f0a050;
    font-size: 11px; font-weight: 500;
    cursor: pointer;
    box-shadow: 0 0 0 0 rgba(240, 160, 80, 0.0);
    animation: wb-mismatch-pulse 1.6s ease-in-out infinite;
    transition: background 140ms, border-color 140ms, transform 120ms;
  }
  .wb-mismatch:hover,
  .wb-mismatch--open {
    background: color-mix(in srgb, #f0a050 28%, transparent);
    border-color: color-mix(in srgb, #f0a050 75%, transparent);
    /* While the menu is engaged or hovered, kill the pulse. */
    animation: none;
    box-shadow: 0 0 0 3px rgba(240, 160, 80, 0.18);
  }
  .wb-mismatch svg { width: 12px; height: 12px; flex-shrink: 0; }

  @keyframes wb-mismatch-pulse {
    0%, 100% {
      background: color-mix(in srgb, #f0a050 14%, transparent);
      border-color: color-mix(in srgb, #f0a050 42%, transparent);
      box-shadow: 0 0 0 0 rgba(240, 160, 80, 0.0);
    }
    50% {
      background: color-mix(in srgb, #f0a050 28%, transparent);
      border-color: color-mix(in srgb, #f0a050 78%, transparent);
      box-shadow: 0 0 0 4px rgba(240, 160, 80, 0.22);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    /* Honour the OS-level "reduce motion" preference — drop the
       pulse but keep the orange accent so the alert is still visible. */
    .wb-mismatch { animation: none; }
  }

  .wb-mismatch-menu {
    position: absolute;
    top: calc(100% + 6px);
    left: 0;
    width: 280px;
    background: var(--bg-1);
    border: 1px solid var(--border-hi);
    border-radius: 9px;
    box-shadow: 0 8px 28px rgba(0, 0, 0, 0.40), 0 0 0 1px rgba(0,0,0,0.18);
    overflow: hidden;
    z-index: 220;
  }
  .wb-mismatch-head {
    padding: 9px 12px 6px;
    font-size: 11px;
    color: var(--text-2);
    border-bottom: 1px solid var(--border);
  }
  .wb-mismatch-opt {
    position: relative;
    display: block;
    width: 100%;
    text-align: left;
    padding: 9px 12px 9px 14px;
    border: none;
    background: transparent;
    color: var(--text-0);
    cursor: pointer;
    transition: background 160ms ease, padding-left 160ms ease;
    overflow: hidden;
  }
  .wb-mismatch-opt + .wb-mismatch-opt {
    border-top: 1px solid var(--border);
  }
  /* Accent stripe slides in on hover — same motif we use elsewhere
     (option rows, list items) so the menu feels in-system. */
  .wb-mismatch-opt::before {
    content: '';
    position: absolute;
    left: 0; top: 6px; bottom: 6px;
    width: 2px;
    border-radius: 0 2px 2px 0;
    background: #f0a050;
    transform: scaleY(0);
    transform-origin: center;
    transition: transform 200ms ease;
  }
  .wb-mismatch-opt:hover {
    background: var(--bg-2);
    padding-left: 18px;
  }
  .wb-mismatch-opt:hover::before { transform: scaleY(1); }
  .wb-mismatch-opt:active { background: var(--bg-3); }
  .wb-mismatch-opt-side {
    font-size: 12px; font-weight: 600;
    color: var(--text-0);
  }
  .wb-mismatch-opt-path {
    font-size: 11px;
    color: #f0a050;
    margin-top: 2px;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .wb-mismatch-opt-hint {
    font-size: 10.5px;
    color: var(--text-mute);
    margin-top: 2px;
  }

  /* Terminal-link variant — uses the terminal source token so the
     chip reads as a different beat from the (mint) editor-link
     chip when both are present. */
  .wb-link--term {
    background: color-mix(in srgb, var(--src-term) 10%, transparent);
    border-color: color-mix(in srgb, var(--src-term) 28%, transparent);
    color: var(--src-term);
  }
  .wb-link--term:hover {
    background: color-mix(in srgb, var(--src-term) 18%, transparent);
  }

  .wb-link--canvas {
    background: color-mix(in srgb, var(--src-canvas) 10%, transparent);
    border-color: color-mix(in srgb, var(--src-canvas) 28%, transparent);
    color: var(--src-canvas);
  }
  .wb-link--canvas:hover {
    background: color-mix(in srgb, var(--src-canvas) 18%, transparent);
  }

  .wb-link-picker { font-size: 11px; }

  .wb-spacer { flex: 1; }

  /* Active-branch chip — only renders when a worktree exists. */
  .wb-worktree {
    display: inline-flex; align-items: center; gap: 5px;
    padding: 4px 9px;
    border-radius: 7px;
    font-size: 11.5px; font-weight: 500;
    cursor: pointer;
    background: var(--accent-soft);
    color: var(--accent-bright);
    border: 1px solid var(--border-accent-2);
    transition: background 140ms;
  }
  .wb-worktree:hover { background: color-mix(in srgb, var(--accent) 12%, transparent); }
  .wb-worktree svg { width: 11px; height: 11px; }
</style>
