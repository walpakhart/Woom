<script lang="ts">
  /* WorktreeBar — узкая полоска под ChatHeader.
     cwd chip + clear button + editor-link chip / picker + worktree
     menu. По сути это новая версия cwd-bar из AgentColumn, но
     самостоятельная. */
  import { sessionsState, focusSession } from '$lib/state/sessions.svelte';
  import { APP_INSTANCE_IDS, layoutState } from '$lib/state/layout.svelte';
  import Dropdown from '$lib/components/ui/Dropdown.svelte';

  type Kind = 'claude' | 'cursor';

  interface Props {
    kind: Kind;
    editorRepoPath: string;
    onPickCwd: () => void;
    onClearCwd: () => void;
    onToggleEditorLink: () => void;
    onLinkToEditorInstance: (id: string) => void;
    onCreateWorktree: () => void;
    onOpenWorktreeDiff: () => void;
    onRemoveWorktree: () => void;
    worktreeBusy: 'creating' | 'removing' | null;
  }
  let p: Props = $props();

  const sess = $derived(
    sessionsState.list.find((s) => s.id === sessionsState.activeIds[p.kind]) ?? null
  );

  /** All editor instances currently open — pulled live from
   *  `layoutState` so the picker reflects every Vermeer / Rothko
   *  spawned via the rail's long-press menu. */
  const editorInstances = $derived(
    layoutState.instances.editor.map((i) => ({ id: i.id, name: i.name }))
  );

  /** The chip shown on the cwd bar when the active session is linked
   *  to one of those editors. Match by id rather than the legacy
   *  primary constant so secondary instances show their curated name. */
  const linkedEditor = $derived.by(() => {
    if (!sess?.linkedToEditor || !sess.linkedToEditorInstanceId) return null;
    const inst = layoutState.instances.editor.find(
      (i) => i.id === sess.linkedToEditorInstanceId
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
      <button class="wb-link" onclick={() => { focusLocal(); p.onToggleEditorLink(); }} title="Linked to Editor — click to unlink">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6"><path d="M3 7v11a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2V9a2 2 0 0 0-2-2h-7L10 5H5a2 2 0 0 0-2 2z"/></svg>
        <span>{linkedEditor.name}</span>
        <svg class="wb-link-x" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M18 6 6 18M6 6l12 12"/></svg>
      </button>
    {:else if editorInstances.length > 0}
      <div class="wb-link-picker">
        <Dropdown
          value=""
          options={editorInstances.map((e) => ({ value: e.id, label: `Link to ${e.name}` }))}
          onChange={(id) => { focusLocal(); p.onLinkToEditorInstance(id); }}
          placeholder="Link editor…"
          ariaLabel="Link to editor"
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
  .wb-link:hover { background: color-mix(in srgb, var(--accent) 12%, transparent); }
  .wb-link svg { width: 11px; height: 11px; }
  .wb-link-x { opacity: 0; transition: opacity 120ms; }
  .wb-link:hover .wb-link-x { opacity: 0.7; }

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
