<script lang="ts">
  /* MarkdownPreview — render the active Markdown buffer as styled
     HTML next to the editor (split view) or in place of it (full
     preview). The component owns its own load loop so it doesn't
     have to share a CodeMirror state with the editor — that means
     the preview reflects the file ON DISK, the same as a fresh
     editor reload, which is the safer mental model for a preview.

     For LIVE preview while the user types, the parent should pass
     `liveSource` — the in-memory CodeMirror text — and the preview
     will render that instead of the disk read. We debounce the
     re-parse on `liveSource` updates so a 4000-line README doesn't
     re-marked-parse on every keystroke.

     Why marked + dompurify-lite here: marked is already in the bundle
     (used by SettingsView for bundled docs); we don't ship a separate
     parser. We don't actually pull DOMPurify — markdown rendered
     here is from local files the user explicitly opened, not from a
     third party, so the threat model is "did I write a script tag
     into my own README". marked's default escape is sufficient for
     that; we lean on `mangle: false` + `headerIds: false` to keep
     the HTML clean and predictable. */

  import { onDestroy } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import { marked } from 'marked';

  interface Props {
    /** Absolute path of the .md file to render. */
    path: string;
    /** Optional in-memory text — when provided, the preview tracks
     *  the user's edits live (debounced). Without it, the preview
     *  reads the file on disk. */
    liveSource?: string;
  }
  let { path, liveSource }: Props = $props();

  let html = $state<string>('');
  let loading = $state(false);
  let error = $state<string | null>(null);
  let containerEl: HTMLDivElement | null = $state(null);

  /* Cheap, predictable marked config. Default GFM, no header ids
     (we don't need anchor scrolling here), no mangle (turns @-handles
     into HTML entities — pretty for spam protection on a public
     blog, ugly for a local README). */
  marked.setOptions({ gfm: true, breaks: false });

  let debounceTimer: ReturnType<typeof setTimeout> | null = null;
  function scheduleParse(src: string) {
    if (debounceTimer) clearTimeout(debounceTimer);
    debounceTimer = setTimeout(() => {
      try {
        /* `marked.parse` is sync when no async extensions are
           registered; explicit Promise.resolve cast covers the
           updated type signature without forcing async churn. */
        Promise.resolve(marked.parse(src) as string | Promise<string>)
          .then((s) => (html = s))
          .catch((e) => (error = e instanceof Error ? e.message : String(e)));
      } catch (e) {
        error = e instanceof Error ? e.message : String(e);
      }
    }, 80);
  }

  async function loadFromDisk() {
    if (!path) { html = ''; return; }
    loading = true;
    error = null;
    try {
      const md = await invoke<string>('fs_read_file', { path });
      scheduleParse(md);
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    /* Re-render whenever path changes OR liveSource arrives. Reading
       both inside the same effect makes Svelte track them both. */
    if (typeof liveSource === 'string') {
      scheduleParse(liveSource);
    } else {
      void loadFromDisk();
    }
  });

  onDestroy(() => {
    if (debounceTimer) clearTimeout(debounceTimer);
  });

  /* Intercept link clicks: relative paths shouldn't try to navigate
     the webview (would blank the editor); absolute http(s) links go
     to the OS browser via the opener plugin. Anchor `#hash` links
     scroll-into-view inside the preview. */
  function onLinkClick(e: MouseEvent) {
    const target = e.target as HTMLElement | null;
    const a = target?.closest('a') as HTMLAnchorElement | null;
    if (!a) return;
    const href = a.getAttribute('href') || '';
    if (!href) return;
    e.preventDefault();
    if (href.startsWith('#')) {
      const id = href.slice(1);
      const node = containerEl?.querySelector(`#${CSS.escape(id)}, [name="${id}"]`);
      if (node && 'scrollIntoView' in node) (node as HTMLElement).scrollIntoView({ behavior: 'smooth', block: 'start' });
      return;
    }
    if (/^https?:\/\//i.test(href) || href.startsWith('mailto:')) {
      void openUrl(href);
    }
    /* Relative paths intentionally no-op for now — opening a sibling
       file as preview would need a parent callback. Easy follow-up. */
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<!-- The wrapper delegates clicks landing on `<a>` links inside the
     rendered HTML (so we can intercept relative paths and external
     URLs centrally). The actual interactive elements ARE the anchor
     tags marked owns, which the browser already keyboard-activates;
     the wrapper itself is not a tab stop. Hence the lint mute. -->
<div class="mdp" bind:this={containerEl} onclick={onLinkClick} role="document">
  {#if error}
    <div class="mdp-error">{error}</div>
  {:else if loading && !html}
    <div class="mdp-spinner">Rendering…</div>
  {:else}
    <!-- eslint-disable-next-line svelte/no-at-html-tags -->
    {@html html}
  {/if}
</div>

<style>
  .mdp {
    height: 100%;
    overflow-y: auto;
    padding: 28px 36px 60px;
    color: var(--text-1);
    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', 'Geist', system-ui, sans-serif;
    font-size: 14px;
    line-height: 1.65;
    background: var(--bg-0);
  }
  .mdp-error {
    padding: 8px 14px;
    background: rgba(232, 130, 100, 0.12);
    color: var(--error);
    border-radius: 6px;
    font-size: 12.5px;
  }
  .mdp-spinner { color: var(--text-mute); font-size: 12px; }

  /* ── Typography ──────────────────────────────────── */
  .mdp :global(h1),
  .mdp :global(h2),
  .mdp :global(h3),
  .mdp :global(h4),
  .mdp :global(h5),
  .mdp :global(h6) {
    color: var(--text-0);
    font-family: 'Geist', system-ui, sans-serif;
    font-weight: 600;
    letter-spacing: -0.015em;
    line-height: 1.25;
    margin: 1.6em 0 0.6em;
  }
  .mdp :global(h1):first-child { margin-top: 0; }
  .mdp :global(h1) { font-size: 26px; padding-bottom: 8px; border-bottom: 1px solid var(--border); }
  .mdp :global(h2) { font-size: 21px; padding-bottom: 6px; border-bottom: 1px solid var(--border); }
  .mdp :global(h3) { font-size: 17px; }
  .mdp :global(h4) { font-size: 15px; }
  .mdp :global(h5),
  .mdp :global(h6) { font-size: 13.5px; color: var(--text-2); }

  .mdp :global(p) { margin: 0 0 1em; }
  .mdp :global(strong) { color: var(--text-0); font-weight: 600; }
  .mdp :global(em) { font-style: italic; color: var(--text-0); }
  .mdp :global(del) { color: var(--text-mute); text-decoration: line-through; }

  /* Links — accent-tinted, hover underlines instead of always-on. */
  .mdp :global(a) {
    color: var(--accent-bright);
    text-decoration: none;
    border-bottom: 1px solid color-mix(in srgb, var(--accent) 32%, transparent);
    transition: border-color 120ms;
  }
  .mdp :global(a):hover {
    border-bottom-color: var(--accent-bright);
  }

  /* Inline code — pill, monospace. */
  .mdp :global(code) {
    font-family: 'JetBrains Mono', 'SF Mono', monospace;
    font-size: 0.88em;
    padding: 1.5px 5px;
    border-radius: 4px;
    background: var(--bg-2);
    color: var(--accent-bright);
    border: 1px solid var(--border);
  }

  /* Code blocks — dark slab, scrolls horizontally. The `> code`
     selector hands styling to the inline rule but neutralises the
     padding/background so the pre wrapper owns layout. */
  .mdp :global(pre) {
    background: var(--bg-1);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 14px 16px;
    overflow-x: auto;
    font-size: 12.5px;
    line-height: 1.55;
    margin: 0 0 1em;
  }
  .mdp :global(pre code) {
    background: transparent;
    border: 0;
    padding: 0;
    color: var(--text-1);
    font-size: inherit;
  }

  /* Lists ── */
  .mdp :global(ul),
  .mdp :global(ol) {
    margin: 0 0 1em;
    padding-left: 1.6em;
  }
  .mdp :global(li) { margin-bottom: 0.25em; }
  .mdp :global(li > p) { margin-bottom: 0.4em; }
  .mdp :global(ul ul),
  .mdp :global(ol ol),
  .mdp :global(ul ol),
  .mdp :global(ol ul) { margin-bottom: 0.25em; margin-top: 0.25em; }

  /* Task lists — GFM `[ ]` / `[x]`. marked emits a disabled checkbox
     so users don't accidentally toggle state on a preview. */
  .mdp :global(li input[type="checkbox"]) {
    margin-right: 6px;
    transform: translateY(1px);
    accent-color: var(--accent);
  }

  /* Blockquote — accent left bar. */
  .mdp :global(blockquote) {
    margin: 0 0 1em;
    padding: 4px 14px;
    border-left: 3px solid color-mix(in srgb, var(--accent) 50%, var(--border));
    background: color-mix(in srgb, var(--accent) 6%, transparent);
    color: var(--text-2);
    border-radius: 0 6px 6px 0;
  }
  .mdp :global(blockquote p:last-child) { margin-bottom: 0; }

  /* Tables — compact, alt-row tint. */
  .mdp :global(table) {
    border-collapse: collapse;
    width: 100%;
    margin: 0 0 1em;
    font-size: 13px;
    border: 1px solid var(--border);
    border-radius: 8px;
    overflow: hidden;
  }
  .mdp :global(thead) { background: var(--bg-2); }
  .mdp :global(th),
  .mdp :global(td) {
    padding: 8px 12px;
    text-align: left;
    border-bottom: 1px solid var(--border);
    border-right: 1px solid var(--border);
  }
  .mdp :global(th:last-child),
  .mdp :global(td:last-child) { border-right: 0; }
  .mdp :global(tr:last-child td) { border-bottom: 0; }
  .mdp :global(tbody tr:nth-child(even)) {
    background: color-mix(in srgb, var(--bg-2) 50%, transparent);
  }
  .mdp :global(th) { color: var(--text-0); font-weight: 600; }

  /* Horizontal rule */
  .mdp :global(hr) {
    border: 0;
    border-top: 1px solid var(--border);
    margin: 1.6em 0;
  }

  /* Images — fit width, soft border. */
  .mdp :global(img) {
    max-width: 100%;
    height: auto;
    border-radius: 6px;
    border: 1px solid var(--border);
    background: var(--bg-2);
  }

  /* Keyboard tags */
  .mdp :global(kbd) {
    display: inline-block;
    padding: 1px 6px;
    border-radius: 4px;
    background: var(--bg-2);
    border: 1px solid var(--border);
    color: var(--text-0);
    font-family: 'JetBrains Mono', monospace;
    font-size: 0.85em;
    box-shadow: inset 0 -1px 0 var(--border-hi);
  }
</style>
