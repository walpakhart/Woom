<script lang="ts">
  import { marked } from 'marked';

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

  const html = $derived.by(() => {
    if (!source) return '';
    try {
      const raw = marked.parse(source, { async: false }) as string;
      // Highlight @mentions:
      //   @DEVOPS-437 / @EFF-21190               — Jira keys (single segment)
      //   @CATALOG-API-76 / @BMS-API-J6          — Sentry short-ids (multi-segment)
      //   @#482                                  — GitHub issue/PR numbers
      //   @path/to/file.ext or @dir/             — file/folder paths
      //   @file.ext (with a dot)                 — bare filename mentions
      // Must operate on rendered HTML; avoid matching inside existing tags.
      return raw.replace(
        /(^|[\s>(\[])@((?:#\d+)|(?:[A-Z][A-Z0-9_]*(?:-[A-Z0-9_]+)+)|(?:[a-zA-Z0-9_.\-]+\/[a-zA-Z0-9_./\-]*)|(?:[a-zA-Z0-9_\-]+\.[a-zA-Z0-9]+))/g,
        (_m, lead: string, token: string) => {
          const cls = mentionClass(token);
          // Escape double-quotes in the path just in case — only relevant
          // for file tokens because tickets/issues never contain them.
          const safe = token.replace(/"/g, '&quot;');
          return `${lead}<span class="${cls}" data-path="${safe}">@${token}</span>`;
        }
      );
    } catch {
      return source;
    }
  });

  /** Delegate clicks to any clickable mention inside the rendered tree. */
  function onClickProse(ev: MouseEvent) {
    const t = ev.target as HTMLElement | null;
    const el = t?.closest?.('.ment-clickable') as HTMLElement | null;
    if (!el) return;
    const path = el.dataset.path;
    if (!path) return;
    ev.preventDefault();
    onOpenFile?.(path);
  }
</script>

<div class="prose" onclick={onClickProse} role="presentation">{@html html}</div>

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
    font-family: 'JetBrains Mono', monospace;
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
    font-family: 'JetBrains Mono', monospace;
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
    font-family: 'JetBrains Mono', monospace;
    font-size: 0.92em;
    border: 1px solid rgba(16, 185, 129, 0.22);
    font-weight: 500;
  }
  /* File / folder mentions are clickable — show the pointer and a mild
     hover so it's obvious you can open them. Ticket + issue mentions
     stay pure highlight (no interaction yet). */
  .prose :global(.ment-clickable) { cursor: pointer; transition: all 120ms; }
  .prose :global(.ment-clickable:hover) {
    color: #0a111e;
    background: var(--accent-bright);
    border-color: var(--accent);
  }
</style>
