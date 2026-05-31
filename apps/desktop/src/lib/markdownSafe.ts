import DOMPurify from 'dompurify';

/** Sanitize marked-rendered HTML before it hits `{@html}`. Woom renders
 *  markdown from untrusted sources — agent output, GitHub/Jira bodies,
 *  cloned-repo READMEs — and `marked` passes embedded HTML through
 *  verbatim (its `sanitize` option was removed in 0.7). Combined with the
 *  webview's `script-src 'unsafe-inline'`, an `<img onerror=…>` in that
 *  markdown would execute with `window.__TAURI__` IPC reach. DOMPurify
 *  strips scripts / inline event handlers / `javascript:` URIs while
 *  keeping the tags + classes + `data-*` attrs our renderers rely on
 *  (e.g. `data-path` on clickable file mentions).
 */
export function sanitizeMarkdownHtml(html: string): string {
  return DOMPurify.sanitize(html, { ADD_ATTR: ['data-path', 'target'] });
}
