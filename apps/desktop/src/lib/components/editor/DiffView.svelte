<script lang="ts">
  import { onDestroy } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { EditorState } from '@codemirror/state';
  import { EditorView, lineNumbers } from '@codemirror/view';
  import { highlightActiveLineGutter, keymap } from '@codemirror/view';
  import { defaultKeymap, history, historyKeymap } from '@codemirror/commands';
  import { syntaxHighlighting, defaultHighlightStyle, bracketMatching, foldGutter } from '@codemirror/language';
  import { MergeView } from '@codemirror/merge';
  import { languageFor } from '$lib/components/editor/codemirrorLang';
  import { editorThemeExtension } from '$lib/components/editor/editorTheme';
  import { themeState } from '$lib/state/theme.svelte';

  interface Props {
    repo: string;
    path: string;
    /** true = staged-vs-HEAD, false = worktree-vs-index */
    staged: boolean;
  }
  let { repo, path, staged }: Props = $props();

  let containerEl: HTMLDivElement;
  let merge: MergeView | null = null;
  let loading = $state(false);
  let error = $state<string | null>(null);
  let stats = $state<{ add: number; del: number }>({ add: 0, del: 0 });
  /* Full-screen toggle. Pin shows the diff at viewport-cover so the
     user can read both sides without the rail / sidebar fighting for
     pixels. Esc closes; same key dance as overlays elsewhere. */
  let fullscreen = $state(false);
  function toggleFullscreen() { fullscreen = !fullscreen; }
  function onKey(e: KeyboardEvent) {
    if (e.key === 'Escape' && fullscreen) {
      e.preventDefault();
      fullscreen = false;
    }
  }
  $effect(() => {
    if (fullscreen) {
      window.addEventListener('keydown', onKey);
      return () => window.removeEventListener('keydown', onKey);
    }
  });

  function readOnlyExtensions(p: string) {
    return [
      lineNumbers(),
      highlightActiveLineGutter(),
      foldGutter(),
      bracketMatching(),
      syntaxHighlighting(defaultHighlightStyle, { fallback: true }),
      history(),
      keymap.of([...defaultKeymap, ...historyKeymap]),
      editorThemeExtension(themeState.name),
      languageFor(p),
      EditorState.readOnly.of(true),
      EditorView.editable.of(false),
      EditorView.lineWrapping
    ];
  }

  async function load() {
    if (!repo || !path) return;
    loading = true;
    error = null;
    try {
      // staged=true  → HEAD (a) vs index (b)
      // staged=false → index (a) vs worktree (b)
      const [aRev, bRev] = staged ? ['HEAD', ':'] : [':', ''];
      const [aContent, bContent] = await Promise.all([
        invoke<string>('git_show', { repo, revision: aRev, path }),
        invoke<string>('git_show', { repo, revision: bRev, path })
      ]);
      computeStats(aContent, bContent);
      merge?.destroy();
      merge = new MergeView({
        a: {
          doc: aContent,
          extensions: readOnlyExtensions(path)
        },
        b: {
          doc: bContent,
          extensions: readOnlyExtensions(path)
        },
        parent: containerEl,
        orientation: 'a-b',
        revertControls: undefined,
        highlightChanges: true,
        gutter: true,
        collapseUnchanged: { margin: 3, minSize: 4 }
      });
    } catch (e: unknown) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      loading = false;
    }
  }

  function computeStats(a: string, b: string) {
    // Fast approximation: count lines that differ. Not identical to git's
    // +/- but close enough for the header badge.
    const aLines = a.split('\n');
    const bLines = b.split('\n');
    let add = 0, del = 0;
    const max = Math.max(aLines.length, bLines.length);
    for (let i = 0; i < max; i++) {
      const l = aLines[i];
      const r = bLines[i];
      if (l === undefined && r !== undefined) add++;
      else if (l !== undefined && r === undefined) del++;
      else if (l !== r) { add++; del++; }
    }
    stats = { add, del };
  }

  $effect(() => {
    void load();
    /* MergeView doesn't expose its inner EditorViews for compartment-
       reconfigure, so we just rebuild the whole diff when any
       reactive dep flips — including the app theme so the diff
       re-renders with the new editor palette. */
    // eslint-disable-next-line @typescript-eslint/no-unused-expressions
    repo; path; staged; themeState.name;
  });

  onDestroy(() => merge?.destroy());
</script>

<div class="dv" class:dv--full={fullscreen}>
  <div class="dv-head">
    <span class="dv-path mono">{path}</span>
    <span class="dv-side">{staged ? 'HEAD → Staged' : 'Index → Working tree'}</span>
    <span class="dv-stats mono">
      <span class="dv-add">+{stats.add}</span>
      <span class="dv-del">−{stats.del}</span>
    </span>
    <button
      class="dv-fullscreen"
      onclick={toggleFullscreen}
      title={fullscreen ? 'Exit full screen (Esc)' : 'Open in full screen'}
      aria-label={fullscreen ? 'Exit full screen' : 'Open in full screen'}
    >
      {#if fullscreen}
        <svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">
          <path d="M9 4v5H4M15 4v5h5M9 20v-5H4M15 20v-5h5"/>
        </svg>
      {:else}
        <svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">
          <path d="M4 9V4h5M20 9V4h-5M4 15v5h5M20 15v5h-5"/>
        </svg>
      {/if}
    </button>
  </div>
  {#if loading}
    <div class="dv-state">Loading diff…</div>
  {:else if error}
    <div class="dv-state dv-err">{error}</div>
  {/if}
  <div class="dv-body" bind:this={containerEl}></div>
</div>

<style>
  .dv { display: flex; flex-direction: column; height: 100%; min-height: 0; background: var(--bg-0); }
  /* Full-screen overlay — fixed cover, above rail/sidebar. Click X
     in the header (or Esc) to dismiss. */
  .dv.dv--full {
    position: fixed;
    inset: 0;
    z-index: 1000;
    background: var(--bg-0);
    box-shadow: 0 0 0 1px var(--border-neutral), 0 20px 60px rgba(0, 0, 0, 0.5);
  }
  .dv-fullscreen {
    background: transparent;
    border: 1px solid var(--border-neutral);
    border-radius: 5px;
    padding: 3px 6px;
    color: var(--text-1);
    cursor: pointer;
    display: inline-flex; align-items: center; justify-content: center;
    transition: background 100ms ease, color 100ms ease, border-color 100ms ease;
  }
  .dv-fullscreen:hover { background: var(--bg-2); color: var(--text-0); border-color: var(--border-neutral-hi); }
  .dv-head {
    display: flex; align-items: center; gap: 12px;
    padding: 6px 14px;
    border-bottom: 1px solid var(--border-neutral);
    background: var(--bg-1);
    font-size: 12px;
    flex-shrink: 0;
  }
  .dv-path { color: var(--text-0); font-weight: 500; flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .dv-side { color: var(--text-2); font-size: 10.5px; padding: 2px 7px; border-radius: 3px; background: var(--bg-3); }
  .dv-stats { color: var(--text-1); font-size: 11px; display: inline-flex; gap: 8px; }
  .dv-add { color: var(--success); }
  .dv-del { color: var(--error); }

  .dv-state { padding: 12px 14px; color: var(--text-2); font-size: 12.5px; border-bottom: 1px solid var(--border-neutral); }
  .dv-err { color: var(--error); }

  /* The full chain needs min-height: 0 so the inner `.cm-scroller` can
     overflow and scroll instead of pushing the flex container taller. */
  .dv-body { flex: 1; min-height: 0; overflow: hidden; display: flex; }
  .dv-body :global(.cm-mergeView) {
    height: 100%; width: 100%; min-height: 0;
    display: flex; flex-direction: column;
  }
  .dv-body :global(.cm-mergeViewEditors) {
    display: flex; flex: 1;
    min-height: 0; min-width: 0;
  }
  .dv-body :global(.cm-mergeViewEditor) {
    flex: 1; min-width: 0; min-height: 0;
    display: flex; flex-direction: column;
  }
  .dv-body :global(.cm-editor) {
    flex: 1; min-height: 0;
    font-family: 'JetBrains Mono', ui-monospace, 'SF Mono', monospace;
    font-size: 12.5px;
  }
  .dv-body :global(.cm-editor.cm-focused) { outline: none; }
  .dv-body :global(.cm-scroller) { overflow: auto; }

  /* Recolor CodeMirror merge diff backgrounds. Saturated enough to
     read at low ambient brightness without losing the underlying
     token-color (CodeMirror token spans set their own color, we only
     touch the line background + inline-change tint). */
  .dv-body :global(.cm-editor) { color: var(--text-0); }
  .dv-body :global(.cm-content) { color: var(--text-0); }
  .dv-body :global(.cm-changedLine) {
    background: rgba(217, 184, 110, 0.16);
    color: var(--text-0);
  }
  .dv-body :global(.cm-deletedChunk) {
    background: rgba(232, 130, 100, 0.22);
    color: var(--text-0);
  }
  .dv-body :global(.cm-changedText) {
    background: rgba(217, 184, 110, 0.40);
    color: var(--text-0);
    font-weight: 500;
  }
  .dv-body :global(.cm-deletedText) {
    background: rgba(232, 130, 100, 0.45);
    color: var(--text-0);
    text-decoration: none;
    font-weight: 500;
  }
  .dv-body :global(.cm-insertedLine) {
    background: rgba(111, 174, 136, 0.20);
    color: var(--text-0);
  }
  /* Deleted-chunk inner text wrapper — CodeMirror nests a span here
     and the default color is dimmer than the surrounding bg. Force
     full-strength text so `- removed line` reads cleanly. */
  .dv-body :global(.cm-deletedChunk .cm-deletedLine),
  .dv-body :global(.cm-deletedChunk *) { color: var(--text-0); }
  .dv-body :global(.cm-collapsedLines) {
    background: var(--bg-2);
    color: var(--text-2);
    padding: 4px 10px;
    font-size: 11px;
  }
  .dv-body :global(.cm-collapsedLines:hover) { color: var(--accent-bright); cursor: pointer; }
</style>
