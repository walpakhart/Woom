<script lang="ts">
  import { marked } from 'marked';
  import { onDestroy } from 'svelte';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import { sanitizeMarkdownHtml } from '$lib/markdownSafe';

  interface Props {
    source: string;
    /** Clicking an @file/@dir mention in rendered output calls this with
        the bare path (without the @). When omitted the mention renders
        as a non-clickable highlight (same as before). */
    onOpenFile?: (path: string) => void;
  }
  let { source, onOpenFile }: Props = $props();

  marked.setOptions({ gfm: true, breaks: false });

  /** Which token flavor a match is — governs which class/dataset the span
      carries, so file tokens can be wired up clickable and ticket tokens
      render as plain highlights.

      Ticket pattern accepts BOTH single-segment Jira keys (`DEVOPS-437`)
      AND multi-segment Sentry short-ids (`CATALOG-API-76`, `BMS-API-J6`).
      Trailing segment must be alphanumeric (allows Sentry's base-32-style
      suffixes like `J6` / `JX5` in addition to plain numbers). */
  function mentionClass(token: string): string {
    if (token.startsWith('#')) return 'ment ment-issue';
    if (/^[A-Z][A-Z0-9_]*(?:-[A-Z0-9_]+)+$/.test(token)) return 'ment ment-ticket';
    return 'ment ment-file ment-clickable';
  }

  /** Heuristic: does this string LOOK like a path / filename the user
   *  would want to open? We're permissive but not reckless — paths
   *  with slashes, or single-segment names with a known-extension
   *  suffix, qualify. Whitespace and `=` (eg. JS variable assignments
   *  in code blocks) disqualify, so e.g. `const x = foo` doesn't
   *  light up. Empty / multi-line content (full code blocks) is
   *  caught by the caller; we only see single-line `<code>` content
   *  here because the regex is `[^<\n]+`. */
  function looksLikeFile(s: string): boolean {
    const trimmed = s.trim();
    if (trimmed.length === 0 || trimmed.length > 200) return false;
    if (/[\s=;{}]/.test(trimmed)) return false;
    /* Ellipsis = truncated-for-display path ("/Users/me/repo/apps/…").
       Opening it can only produce a "No such file" error tab. */
    if (trimmed.includes('…') || trimmed.includes('...')) return false;
    /* Path with at least one slash + non-empty trailing segment.
       `+` allowed for SvelteKit route files (+page.svelte, +layout.ts). */
    if (/^[a-zA-Z0-9_./\-@+]+\/[a-zA-Z0-9_./\-+]+$/.test(trimmed)) return true;
    /* Single-segment filename with extension. The suffix list keeps
       false positives down (`a.b.c` bare strings aren't always files;
       ticking only when the suffix looks like an actual file kind). */
    if (/^[a-zA-Z0-9_\-+]+(?:\.[a-zA-Z0-9]+){1,3}$/.test(trimmed)) {
      const ext = trimmed.split('.').pop() ?? '';
      const known = new Set([
        'ts', 'tsx', 'js', 'jsx', 'mjs', 'cjs', 'json', 'jsonc',
        'svelte', 'vue', 'astro',
        'rs', 'go', 'py', 'rb', 'java', 'kt', 'swift', 'c', 'cc', 'cpp', 'h', 'hpp',
        'md', 'mdx', 'txt', 'log',
        'css', 'scss', 'sass', 'less', 'html', 'htm', 'xml',
        'yml', 'yaml', 'toml', 'ini', 'env',
        'sh', 'bash', 'zsh', 'fish', 'ps1',
        'sql', 'graphql', 'gql', 'proto',
        'lock', 'gitignore', 'editorconfig'
      ]);
      return known.has(ext.toLowerCase());
    }
    return false;
  }

  /** Post-process a `<pre>...</pre>` block emitted by marked. When the
   *  inner `<code>` has class `language-diff`, split content on
   *  newlines and wrap each line in a span tagged by its first
   *  character — `+`/`-`/`@`/space — so CSS can paint inline +/-
   *  coloring instead of falling through to a flat monospace block.
   *
   *  Returns the input untouched when:
   *    - the block isn't a diff fence,
   *    - parsing fails for any reason.
   *  The +/+ prefix and `@@` hunk headers are KEPT in the rendered
   *  output so copying still produces a valid unified-diff snippet. */
  function decorateDiffBlock(html: string): string {
    /* Cheap structural match — marked always emits the language as a
       class on the inner <code>. If we don't see a diff fence, exit
       fast and let the default <pre> render handle it. */
    if (!/<code\s+class="[^"]*\blanguage-diff\b/.test(html)) return html;
    const m = html.match(/^<pre>(<code\b[^>]*>)([\s\S]*?)<\/code><\/pre>$/);
    if (!m) return html;
    const codeOpenTag = m[1];
    const body = m[2];
    /* marked already escaped < / > / & inside the body, so the
       content we walk here is HTML-safe text + we only insert our
       own span tags around already-escaped text. The split keeps
       trailing empty strings so leading / trailing blank lines stay
       visible in the rendered diff. */
    const decorated = body
      .split('\n')
      .map((line) => {
        if (line === '') return '<span class="diff-line diff-ctx"> </span>';
        const first = line.charAt(0);
        let cls = 'diff-ctx';
        if (first === '+') cls = 'diff-add';
        else if (first === '-') cls = 'diff-rem';
        else if (first === '@') cls = 'diff-hunk';
        return `<span class="diff-line ${cls}">${line}</span>`;
      })
      .join('\n');
    /* Add a marker class on <pre> so chat CSS can opt into the
       framed/striped diff look without polluting non-diff <pre>
       blocks (plain JSON/TS code listings stay un-decorated). */
    return `<pre class="diff-block">${codeOpenTag}${decorated}</code></pre>`;
  }

  /* Throttle markdown re-parsing during rapid streaming. `marked.parse`
   * plus the regex decoration passes below run over the FULL source on
   * every change; for the actively-streaming assistant message that's
   * O(n²) on growing text and was a main-thread hog behind chat scroll
   * jank. We parse a throttled mirror of `source`; a trailing timer
   * guarantees the final content always lands. The first change is never
   * throttled, so one-shot (non-streaming) callers parse immediately. */
  let parseSource = $state('');
  let lastParseAt = 0;
  let trailingTimer: ReturnType<typeof setTimeout> | null = null;
  const PARSE_THROTTLE_MS = 80;

  $effect(() => {
    const s = source;
    const now = performance.now();
    const elapsed = now - lastParseAt;
    if (elapsed >= PARSE_THROTTLE_MS) {
      if (trailingTimer) { clearTimeout(trailingTimer); trailingTimer = null; }
      lastParseAt = now;
      parseSource = s;
    } else {
      if (trailingTimer) clearTimeout(trailingTimer);
      trailingTimer = setTimeout(() => {
        lastParseAt = performance.now();
        parseSource = source;
        trailingTimer = null;
      }, PARSE_THROTTLE_MS - elapsed);
    }
  });

  onDestroy(() => {
    if (trailingTimer) clearTimeout(trailingTimer);
  });

  const html = $derived.by(() => {
    if (!parseSource) return '';
    try {
      const raw = marked.parse(parseSource, { async: false }) as string;

      /* Stash `<pre>...</pre>` blocks (full code listings) so the
         single-token file detector below doesn't accidentally tag
         lines inside them. We restore them after the inline-code
         pass. */
      const preBlocks: string[] = [];
      const stashed = raw.replace(/<pre>[\s\S]*?<\/pre>/g, (m) => {
        const idx = preBlocks.push(decorateDiffBlock(m)) - 1;
        return `PRE_${idx}`;
      });

      /* Inline `<code>` mentions that look like file paths get
         promoted to clickable file links — same `data-path` shape
         as @-mentions so the parent `onClickProse` handler routes
         them through the same `onOpenFile` callback. Only when the
         caller wired a handler; otherwise leave them as plain code. */
      const codeMarked = onOpenFile
        ? stashed.replace(/<code>([^<\n]+)<\/code>/g, (m, content: string) => {
            if (!looksLikeFile(content)) return m;
            const safe = content.trim().replace(/"/g, '&quot;');
            return `<code class="ment-code-file ment-clickable" data-path="${safe}">${content}</code>`;
          })
        : stashed;

      /* Highlight @mentions:
           @DEVOPS-437 / @EFF-21190               — Jira keys (single segment)
           @CATALOG-API-76 / @BMS-API-J6          — Sentry short-ids (multi-segment)
           @#482                                  — GitHub issue/PR numbers
           @path/to/file.ext or @dir/             — file/folder paths
           @file.ext (with a dot)                 — bare filename mentions
         Must operate on rendered HTML; avoid matching inside existing tags. */
      const withMentions = codeMarked.replace(
        /(^|[\s>(\[])@((?:#\d+)|(?:[A-Z][A-Z0-9_]*(?:-[A-Z0-9_]+)+)|(?:[a-zA-Z0-9_.\-]+\/[a-zA-Z0-9_./\-]*)|(?:[a-zA-Z0-9_\-]+\.[a-zA-Z0-9]+))/g,
        (_m, lead: string, token: string) => {
          const cls = mentionClass(token);
          const safe = token.replace(/"/g, '&quot;');
          return `${lead}<span class="${cls}" data-path="${safe}">@${token}</span>`;
        }
      );

      /* Restore `<pre>` blocks, then sanitize — untrusted markdown
         (agent / GitHub / Jira / cloned repo) can embed `<img onerror>`
         etc that marked passes through verbatim. */
      const restored = withMentions.replace(/PRE_(\d+)/g, (_, idx: string) => preBlocks[Number(idx)] ?? '');
      return sanitizeMarkdownHtml(restored);
    } catch {
      return parseSource;
    }
  });

  /** Delegate clicks to any clickable mention inside the rendered tree.
      Also intercepts any plain anchor tag click — markdown links land in
      Tauri's embedded WebView by default, which strips logged-in cookies,
      ad-blockers, and password manager. Route them through the system
      browser via `tauri-plugin-opener` so external URLs always open in
      the user's actual browser (Chrome / Safari / Firefox). Mirrors the
      "Open on GitHub" buttons in the focus pane.
      Internal anchors (`#section`) and JS-only links (`javascript:`) are
      left to the default behaviour. */
  function onClickProse(ev: MouseEvent) {
    const t = ev.target as HTMLElement | null;
    if (!t) return;
    const mention = t.closest?.('.ment-clickable') as HTMLElement | null;
    if (mention) {
      const path = mention.dataset.path;
      if (path) {
        ev.preventDefault();
        onOpenFile?.(path);
      }
      return;
    }
    const a = t.closest?.('a[href]') as HTMLAnchorElement | null;
    if (!a) return;
    const href = a.getAttribute('href') ?? '';
    // In-page anchors and explicit non-http schemes we don't want to
    // hijack (mailto / tel work via the system handler, javascript: is
    // a no-op anyway). Route only http(s) externally.
    if (!/^https?:\/\//i.test(href)) return;
    ev.preventDefault();
    void openUrl(href);
  }
</script>

<!-- `auxclick` is the middle-button equivalent of click; without it cmd+click
     and middle-click would fall through to the WebView's default (which is
     basically a no-op in Tauri but feels broken to the user). -->
<div class="prose" onclick={onClickProse} onauxclick={onClickProse} role="presentation">{@html html}</div>

<style>
  .prose {
    color: var(--text-0);
    font-size: 13.5px;
    line-height: 1.65;
    word-wrap: break-word;
  }
  .prose :global(h1),
  .prose :global(h2),
  .prose :global(h3),
  .prose :global(h4),
  .prose :global(h5),
  .prose :global(h6) {
    color: var(--text-0);
    font-weight: 600;
    letter-spacing: -0.015em;
    margin: 1.4em 0 0.4em;
    line-height: 1.3;
  }
  .prose :global(h1) { font-size: 20px; }
  .prose :global(h2) { font-size: 17px; border-bottom: 1px solid var(--border-neutral); padding-bottom: 6px; }
  .prose :global(h3) { font-size: 15px; }
  .prose :global(h4),
  .prose :global(h5),
  .prose :global(h6) { font-size: 13.5px; }

  .prose :global(p) { margin: 0 0 0.9em; color: var(--text-1); }
  .prose :global(a) { color: var(--accent-bright); text-decoration: none; border-bottom: 1px solid rgba(52, 211, 153, 0.3); }
  .prose :global(a:hover) { border-bottom-color: var(--accent-bright); }

  .prose :global(strong) { color: var(--text-0); font-weight: 600; }
  .prose :global(em) { color: var(--text-0); }

  .prose :global(ul),
  .prose :global(ol) {
    margin: 0 0 0.9em;
    padding-left: 22px;
    color: var(--text-1);
  }
  .prose :global(li) { margin-bottom: 0.3em; }
  .prose :global(li > p) { margin: 0; }
  .prose :global(ul ul),
  .prose :global(ul ol),
  .prose :global(ol ul),
  .prose :global(ol ol) { margin-top: 0.3em; margin-bottom: 0.3em; }

  .prose :global(code) {
    font-family: var(--font-mono);
    font-size: 12px;
    padding: 1px 6px;
    border-radius: 4px;
    background: var(--bg-2);
    border: 1px solid var(--border-neutral-hi);
    color: var(--accent-bright);
  }

  .prose :global(pre) {
    background: var(--bg-0);
    border: 1px solid var(--border-neutral);
    border-radius: 8px;
    padding: 12px 14px;
    overflow-x: auto;
    margin: 0 0 1em;
  }
  .prose :global(pre code) {
    background: transparent;
    border: 0;
    padding: 0;
    color: var(--text-1);
    font-size: 12px;
    line-height: 1.55;
  }

  /* Inline unified diff rendering. ```diff fenced blocks land here
     post `decorateDiffBlock` — each line carries a `diff-line` class
     plus a kind (`diff-add`, `diff-rem`, `diff-ctx`, `diff-hunk`).
     The styling preserves the `+` / `-` / `@@` glyphs verbatim so a
     user-side copy still produces a valid diff snippet; we only add
     line-level background tint + a left stripe per kind.

     The colors are intentionally muted (12-18% blend with bg-0) so a
     long diff doesn't read as a screaming traffic light against the
     surrounding prose, but still pops enough to tell + from − in
     low-contrast monitors. */
  .prose :global(pre.diff-block) {
    padding: 8px 0;
    background: var(--bg-0);
  }
  .prose :global(pre.diff-block code) {
    display: block;
    font-family: var(--font-mono);
  }
  .prose :global(.diff-line) {
    display: block;
    padding: 0 14px;
    border-left: 2px solid transparent;
    white-space: pre;
  }
  .prose :global(.diff-add) {
    background: color-mix(in srgb, #4ade80 14%, transparent);
    border-left-color: color-mix(in srgb, #4ade80 70%, transparent);
    color: color-mix(in srgb, #4ade80 80%, var(--text-0));
  }
  .prose :global(.diff-rem) {
    background: color-mix(in srgb, #f87171 14%, transparent);
    border-left-color: color-mix(in srgb, #f87171 70%, transparent);
    color: color-mix(in srgb, #f87171 80%, var(--text-0));
  }
  .prose :global(.diff-hunk) {
    background: color-mix(in srgb, var(--accent) 8%, transparent);
    color: var(--text-mute);
    font-weight: 600;
  }
  .prose :global(.diff-ctx) {
    color: var(--text-2);
  }

  .prose :global(blockquote) {
    border-left: 3px solid var(--accent);
    padding: 2px 0 2px 12px;
    margin: 0 0 1em;
    color: var(--text-1);
    background: var(--accent-soft);
    border-radius: 0 6px 6px 0;
  }
  .prose :global(blockquote p:last-child) { margin-bottom: 0; }

  .prose :global(hr) {
    border: 0;
    height: 1px;
    background: var(--border-neutral);
    margin: 1.4em 0;
  }

  .prose :global(img) { max-width: 100%; border-radius: 6px; margin: 0.6em 0; }

  .prose :global(table) {
    border-collapse: collapse;
    margin: 0 0 1em;
    font-size: 12.5px;
    width: 100%;
  }
  .prose :global(th),
  .prose :global(td) {
    padding: 7px 10px;
    border: 1px solid var(--border-neutral);
    text-align: left;
  }
  .prose :global(th) {
    background: var(--bg-2);
    font-weight: 600;
    color: var(--text-0);
  }

  .prose :global(input[type="checkbox"]) { margin-right: 6px; }
  .prose :global(del) { color: var(--text-2); }
  .prose :global(kbd) {
    font-family: var(--font-mono);
    font-size: 11px;
    padding: 1px 6px;
    background: var(--bg-2);
    border: 1px solid var(--border-neutral-hi);
    border-radius: 4px;
    color: var(--text-1);
  }

  .prose :global(.ment) {
    color: var(--accent-bright);
    background: var(--accent-soft);
    padding: 1px 6px;
    border-radius: 4px;
    font-family: var(--font-mono);
    font-size: 0.92em;
    border: 1px solid color-mix(in srgb, var(--ok) 22%, transparent);
    font-weight: 500;
  }
  /* File / folder mentions are clickable — show the pointer and a mild
     hover so it's obvious you can open them. Ticket + issue mentions
     stay pure highlight (no interaction yet). */
  .prose :global(.ment-clickable) { cursor: pointer; transition: all 120ms; }
  .prose :global(.ment-clickable:hover) {
    color: var(--accent-fg);
    background: var(--accent-bright);
    border-color: var(--accent);
  }
  /* File-shaped inline `<code>` (e.g. `gitleaks.toml`) that the
     model emits without an `@` prefix — same click affordance as
     `.ment` files but keeps the existing code-span chrome (border +
     bg) so it doesn't visually shout. Underline-on-hover hints at
     the link behaviour without restyling the span entirely. */
  .prose :global(code.ment-code-file) {
    color: var(--accent-bright);
    text-decoration: underline dotted color-mix(in srgb, var(--accent-bright) 50%, transparent);
    text-underline-offset: 3px;
  }
  .prose :global(code.ment-code-file:hover) {
    color: var(--accent-fg);
    background: var(--accent-bright);
    border-color: var(--accent);
    text-decoration-color: transparent;
  }
</style>
