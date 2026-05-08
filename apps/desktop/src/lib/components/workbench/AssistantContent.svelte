<!--
  Renders an assistant message's content with tool-call markers
  parsed into unified INPUT/OUTPUT cards. The marker shape (emitted
  by the stream parser into trace segments + persisted in
  sess.messages) is:

    ‹toolcall›
    <command markdown — typically `$ cmd` inline-code>
    ‹output›             ← optional, present once tool_result arrives
    <output text>
    ‹/output›
    ‹/toolcall›

  Each `‹toolcall›` block renders as ONE card with the command on
  top (always visible) and a collapsible OUTPUT section below
  (default collapsed, click to expand). When there's no output yet
  (mid-flight or no-output tool), only the command part shows.

  Free-floating `‹output›` markers (no enclosing toolcall) are
  treated as legacy and rendered as standalone collapsibles for
  backwards compat with messages persisted by older builds.
-->
<script lang="ts">
  import Markdown from '$lib/components/ui/Markdown.svelte';

  interface Props {
    /** Full assistant message content. May contain `‹toolcall›` blocks
     *  with optional `‹output›` inside, plus plain text and free
     *  `‹output›` markers (legacy). */
    source: string;
    /** Forwarded to the Markdown component for path-mention links. */
    onOpenFile?: (path: string) => void;
  }
  let { source, onOpenFile }: Props = $props();

  type Segment =
    | { kind: 'text'; body: string }
    | { kind: 'output'; body: string }
    | { kind: 'toolcall'; input: string; output: string | null };

  /** Walk the source string, peeling off the next segment at each
   *  step. Order of priority at each cursor:
   *    1. `‹toolcall›…‹/toolcall›` (with optional inner `‹output›`)
   *    2. `‹output›…‹/output›` (legacy free-floating)
   *    3. plain text up to the next marker
   *  Tolerant of unclosed markers — the trailing chunk just becomes
   *  whichever kind we were inside, ungated.
   */
  function splitContent(s: string): Segment[] {
    const segs: Segment[] = [];
    const callOpen = '‹toolcall›';
    const callClose = '‹/toolcall›';
    const outOpen = '‹output›';
    const outClose = '‹/output›';
    let cursor = 0;
    while (cursor < s.length) {
      const callIdx = s.indexOf(callOpen, cursor);
      const outIdx = s.indexOf(outOpen, cursor);
      // Pick the earlier of the two openings as the next event.
      // -1 means "not found" — convert to Infinity for math.
      const nextCall = callIdx < 0 ? Infinity : callIdx;
      const nextOut = outIdx < 0 ? Infinity : outIdx;
      const nextMarker = Math.min(nextCall, nextOut);
      if (nextMarker === Infinity) {
        segs.push({ kind: 'text', body: s.slice(cursor) });
        break;
      }
      if (nextMarker > cursor) {
        segs.push({ kind: 'text', body: s.slice(cursor, nextMarker) });
      }
      if (nextCall <= nextOut) {
        // `‹toolcall›…‹/toolcall›` — extract input + optional output.
        const bodyStart = nextCall + callOpen.length;
        const closeAt = s.indexOf(callClose, bodyStart);
        if (closeAt < 0) {
          // Unclosed — treat the rest as one toolcall body without
          // a parsed output.
          const body = s.slice(bodyStart).trim();
          segs.push({ kind: 'toolcall', input: body, output: null });
          break;
        }
        const inner = s.slice(bodyStart, closeAt);
        const outOpenInner = inner.indexOf(outOpen);
        const outCloseInner = outOpenInner >= 0 ? inner.indexOf(outClose, outOpenInner + outOpen.length) : -1;
        if (outOpenInner >= 0 && outCloseInner >= 0) {
          const input = inner.slice(0, outOpenInner).trim();
          const output = inner.slice(outOpenInner + outOpen.length, outCloseInner).trim();
          segs.push({ kind: 'toolcall', input, output });
        } else {
          segs.push({ kind: 'toolcall', input: inner.trim(), output: null });
        }
        cursor = closeAt + callClose.length;
        continue;
      }
      // `‹output›…‹/output›` (legacy free-floating).
      const bodyStart = nextOut + outOpen.length;
      const closeAt = s.indexOf(outClose, bodyStart);
      if (closeAt < 0) {
        segs.push({ kind: 'output', body: s.slice(bodyStart).trim() });
        break;
      }
      segs.push({ kind: 'output', body: s.slice(bodyStart, closeAt).trim() });
      cursor = closeAt + outClose.length;
    }
    return segs;
  }

  const segments = $derived(splitContent(source));

  /** Per-card expansion state. Keyed by index in `segments`. */
  let expanded = $state<Set<number>>(new Set());
  function toggle(i: number) {
    if (expanded.has(i)) expanded.delete(i);
    else expanded.add(i);
    expanded = new Set(expanded);
  }

  function lineCount(body: string): number {
    return body.split('\n').length;
  }
  function peek(body: string): string {
    const firstLine = body.split('\n').find((l) => l.trim().length > 0) ?? '';
    return firstLine.length > 80 ? `${firstLine.slice(0, 79)}…` : firstLine;
  }
</script>

{#each segments as seg, i (i)}
  {#if seg.kind === 'text'}
    {#if seg.body.trim().length > 0}
      <Markdown source={seg.body} {onOpenFile} />
    {/if}
  {:else if seg.kind === 'toolcall'}
    <div class="tc-card" class:tc-card--open={expanded.has(i)}>
      <div class="tc-input">
        <span class="tc-label">input</span>
        <div class="tc-input-body">
          <Markdown source={seg.input} {onOpenFile} />
        </div>
      </div>
      {#if seg.output !== null}
        <button
          type="button"
          class="tc-output-toggle"
          onclick={() => toggle(i)}
          aria-expanded={expanded.has(i)}
          title={expanded.has(i) ? 'Hide output' : 'Show output'}
        >
          <svg class="i i-sm tc-chevron" class:tc-chevron--open={expanded.has(i)} viewBox="0 0 24 24"><path d="m9 18 6-6-6-6"/></svg>
          <span class="tc-label">output</span>
          <span class="tc-meta mono">{lineCount(seg.output)}&nbsp;line{lineCount(seg.output) === 1 ? '' : 's'}</span>
          {#if !expanded.has(i)}
            <span class="tc-peek">{peek(seg.output)}</span>
          {/if}
        </button>
        {#if expanded.has(i)}
          <pre class="tc-output-body mono">{seg.output}</pre>
        {/if}
      {/if}
    </div>
  {:else}
    <!-- Legacy free-floating ‹output› — render as a standalone
         collapsible (same shape as the toolcall's output section
         minus the input). -->
    <div class="tc-card tc-card--legacy" class:tc-card--open={expanded.has(i)}>
      <button
        type="button"
        class="tc-output-toggle"
        onclick={() => toggle(i)}
        aria-expanded={expanded.has(i)}
        title={expanded.has(i) ? 'Hide output' : 'Show output'}
      >
        <svg class="i i-sm tc-chevron" class:tc-chevron--open={expanded.has(i)} viewBox="0 0 24 24"><path d="m9 18 6-6-6-6"/></svg>
        <span class="tc-label">output</span>
        <span class="tc-meta mono">{lineCount(seg.body)}&nbsp;line{lineCount(seg.body) === 1 ? '' : 's'}</span>
        {#if !expanded.has(i)}
          <span class="tc-peek">{peek(seg.body)}</span>
        {/if}
      </button>
      {#if expanded.has(i)}
        <pre class="tc-output-body mono">{seg.body}</pre>
      {/if}
    </div>
  {/if}
{/each}

<style>
  .tc-card {
    margin: 8px 0;
    border: 1px solid var(--border-subtle, rgba(255, 255, 255, 0.08));
    border-radius: 6px;
    background: var(--bg-2, rgba(255, 255, 255, 0.02));
    overflow: hidden;
  }
  .tc-card--open {
    background: var(--bg-1, rgba(255, 255, 255, 0.04));
  }

  /* INPUT row — always visible at the top of the card. Slim
     header strip with a label, command body inline. */
  .tc-input {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    padding: 6px 10px 6px 10px;
  }
  .tc-input-body {
    flex: 1;
    min-width: 0;
    /* Inline Markdown content — collapse default block margins so
       the command sits flush with the label without weird gaps. */
    line-height: 1.45;
  }
  .tc-input-body :global(p) {
    margin: 0;
  }
  .tc-input-body :global(code) {
    font-size: 11.5px;
  }
  .tc-input-body :global(pre) {
    margin: 0;
    background: transparent;
    padding: 0;
  }

  .tc-label {
    flex-shrink: 0;
    font-weight: 500;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    font-size: 10px;
    color: var(--accent-2, var(--accent, #f59e0b));
    padding-top: 2px;
    width: 50px;
  }

  /* OUTPUT toggle row — sits below INPUT, separated by a thin
     border. Click to expand/collapse the output body. */
  .tc-output-toggle {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 6px 10px;
    background: transparent;
    border: 0;
    border-top: 1px solid var(--border-subtle, rgba(255, 255, 255, 0.06));
    color: var(--text-2, rgba(255, 255, 255, 0.6));
    font-size: 12px;
    font-family: inherit;
    cursor: pointer;
    text-align: left;
  }
  .tc-output-toggle:hover {
    background: var(--bg-1, rgba(255, 255, 255, 0.04));
    color: var(--text-1, rgba(255, 255, 255, 0.85));
  }
  .tc-card--legacy .tc-output-toggle {
    border-top: 0;
  }

  .tc-chevron {
    flex-shrink: 0;
    width: 12px;
    height: 12px;
    transition: transform 120ms ease;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.8;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  .tc-chevron--open {
    transform: rotate(90deg);
  }

  .tc-meta {
    flex-shrink: 0;
    color: var(--text-3, rgba(255, 255, 255, 0.4));
    font-size: 10.5px;
  }

  .tc-peek {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--text-3, rgba(255, 255, 255, 0.4));
    font-family: var(--font-mono, ui-monospace, SFMono-Regular, Menlo, monospace);
    font-size: 11px;
  }

  .tc-output-body {
    margin: 0;
    padding: 8px 12px;
    background: var(--bg-0, rgba(0, 0, 0, 0.25));
    border-top: 1px solid var(--border-subtle, rgba(255, 255, 255, 0.06));
    font-size: 11.5px;
    line-height: 1.45;
    color: var(--text-1, rgba(255, 255, 255, 0.85));
    white-space: pre-wrap;
    word-break: break-word;
    max-height: 400px;
    overflow-y: auto;
  }
</style>
