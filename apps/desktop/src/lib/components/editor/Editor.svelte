<script lang="ts">
  import { onDestroy } from 'svelte';
  import { EditorView, basicSetup } from 'codemirror';
  import { EditorState, Compartment } from '@codemirror/state';
  import { keymap } from '@codemirror/view';
  import { oneDark } from '@codemirror/theme-one-dark';
  import { invoke } from '@tauri-apps/api/core';
  import { languageFor } from '$lib/components/editor/codemirrorLang';

  interface Props {
    path: string;
    onDirty?: (dirty: boolean) => void;
    onSaved?: (path: string) => void;
  }
  let { path, onDirty, onSaved }: Props = $props();

  let editorEl: HTMLDivElement;
  let view: EditorView | null = null;
  let lastLoadedPath = $state('');
  let savedContents = $state('');
  let loading = $state(false);
  let error = $state<string | null>(null);
  let dirty = $state(false);

  const languageCompartment = new Compartment();

  async function load(p: string) {
    if (!p || p === lastLoadedPath) return;
    loading = true;
    error = null;
    try {
      const contents = await invoke<string>('fs_read_file', { path: p });
      savedContents = contents;
      lastLoadedPath = p;
      dirty = false;
      onDirty?.(false);

      view?.destroy();
      view = new EditorView({
        parent: editorEl,
        state: EditorState.create({
          doc: contents,
          extensions: [
            basicSetup,
            oneDark,
            languageCompartment.of(languageFor(p)),
            keymap.of([
              { key: 'Mod-s', run: (v) => { void save(v); return true; } }
            ]),
            EditorView.updateListener.of((u) => {
              if (u.docChanged) {
                const cur = u.state.doc.toString();
                const d = cur !== savedContents;
                if (d !== dirty) {
                  dirty = d;
                  onDirty?.(d);
                }
              }
            })
          ]
        })
      });
    } catch (e: unknown) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      loading = false;
    }
  }

  async function save(v: EditorView) {
    if (!lastLoadedPath) return;
    const cur = v.state.doc.toString();
    try {
      await invoke('fs_write_file', { path: lastLoadedPath, contents: cur });
      savedContents = cur;
      dirty = false;
      onDirty?.(false);
      onSaved?.(lastLoadedPath);
    } catch (e: unknown) {
      error = e instanceof Error ? e.message : String(e);
    }
  }

  export async function reload() {
    if (!path) return;
    const prev = lastLoadedPath;
    lastLoadedPath = '';
    await load(prev || path);
  }

  export async function saveNow() {
    if (view) await save(view);
  }

  $effect(() => {
    void load(path);
  });

  onDestroy(() => view?.destroy());
</script>

<div class="ed">
  {#if error}
    <div class="ed-error">{error}</div>
  {/if}
  <div class="ed-surface" bind:this={editorEl}></div>
  {#if loading}<div class="ed-spinner">Loading…</div>{/if}
</div>

<style>
  .ed { position: relative; height: 100%; display: flex; flex-direction: column; overflow: hidden; background: var(--bg-0); }
  .ed-surface { flex: 1; overflow: hidden; min-height: 0; }
  .ed-surface :global(.cm-editor) { height: 100%; font-family: 'JetBrains Mono', ui-monospace, 'SF Mono', monospace; font-size: 13px; }
  .ed-surface :global(.cm-editor.cm-focused) { outline: none; }
  .ed-surface :global(.cm-scroller) { font-family: inherit; }
  .ed-error {
    padding: 8px 14px;
    background: rgba(214, 72, 44, 0.12);
    color: var(--error);
    border-bottom: 1px solid rgba(214, 72, 44, 0.24);
    font-size: 12.5px;
  }
  .ed-spinner {
    position: absolute;
    top: 8px; right: 12px;
    font-size: 11px;
    color: var(--text-2);
  }
</style>
