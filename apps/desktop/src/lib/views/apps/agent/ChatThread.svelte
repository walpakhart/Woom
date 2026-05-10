<script lang="ts">
  /* ChatThread — message list + action cards for the active session.
     v7: 76px byline column ("@you" / italic-serif "claude") on left,
     message body on right. User messages get a soft bg + 2px clay
     accent stripe. Assistant messages are unbubbled prose with inline
     trace pills, edit cards (editor-stripe), and action cards (per-kind
     stripe — github for PR, etc.). */
  import { sessionsState } from '$lib/state/sessions.svelte';
  import { convertFileSrc } from '@tauri-apps/api/core';
  import Markdown from '$lib/components/ui/Markdown.svelte';
  import ClaudeActionCard from '$lib/components/agent/ClaudeActionCard.svelte';
  import type { ClaudeAction } from '$lib/types';

  type Kind = 'claude' | 'cursor';

  interface Props {
    kind: Kind;
    thinkingStartedAt: number | null;
    thinkingTick: number;
    onUpdateAction: (sessionId: string, actionId: string, patch: Partial<ClaudeAction>) => void;
    onRemoveAction: (sessionId: string, actionId: string) => void;
    onExecuteAction: (sessionId: string, action: ClaudeAction) => void;
    onOpenPrInWoom: (url: string, action: (ClaudeAction & { kind: 'pr' }) | null) => void;
    onStartEditMessage: (sessionId: string, index: number, content: string) => void;
    onResendMessage: (sessionId: string, index: number, content: string) => void;
    /** Click on a file-like reference inside a rendered chat bubble —
     *  the parent owns the resolution against the session's cwd /
     *  worktree / linked editor, so all we have to do here is plumb
     *  the path through. */
    onOpenFile?: (path: string) => void;
  }
  let p: Props = $props();

  const sess = $derived(
    sessionsState.list.find((s) => s.id === sessionsState.activeIds[p.kind]) ?? null
  );

  const elapsed = $derived.by(() => {
    if (!p.thinkingStartedAt || !sess?.sending) return '';
    void p.thinkingTick;
    const ms = Date.now() - p.thinkingStartedAt;
    const s = Math.floor(ms / 1000);
    return s < 60 ? `${s}s` : `${Math.floor(s / 60)}m ${String(s % 60).padStart(2, '0')}s`;
  });

  const repoCwd = $derived(sess?.worktreePath ?? sess?.cwd ?? null);

  let chatEl: HTMLDivElement | null = $state(null);
  $effect(() => {
    if (!chatEl) return;
    const sessId = sess?.id;
    if (sessId) {
      sessionsState.scrollEls[`solo:${sessId}`] = chatEl;
    }
  });
  /* Auto-scroll on new content, but only when the user is already
     anchored near the bottom — otherwise we'd yank them away from
     scrolled-up history every time the agent streams a chunk.
     Streaming triggers this effect on every char delta, so we
     coalesce all writes into ONE rAF per frame; without that, with
     50+ messages mounted, each `scrollTop = scrollHeight` forced a
     full-page layout, pegging the main thread for hundreds of ms
     per second. */
  let scrollPending = false;
  /* Threshold matches "within one bubble of the bottom" — generous
     enough that natural scroll-bounce keeps you stuck-to-bottom,
     tight enough that "I scrolled up to re-read" actually wins. */
  const STICK_PX = 80;
  let stickToBottom = true;

  function onChatScroll() {
    if (!chatEl) return;
    const distanceFromBottom = chatEl.scrollHeight - chatEl.scrollTop - chatEl.clientHeight;
    stickToBottom = distanceFromBottom < STICK_PX;
  }

  $effect(() => {
    if (!chatEl || !sess) return;
    /* Track the cheap signals — message count + last content length.
       Streaming bumps content.length per chunk; we still only do one
       scroll write per frame thanks to the rAF guard below. */
    void sess.messages.length;
    void sess.actions.length;
    void (sess.messages[sess.messages.length - 1]?.content?.length ?? 0);
    if (!stickToBottom) return;
    if (scrollPending) return;
    scrollPending = true;
    requestAnimationFrame(() => {
      scrollPending = false;
      if (!chatEl || !stickToBottom) return;
      chatEl.scrollTop = chatEl.scrollHeight;
    });
  });

  /** Parse a single trace segment. `appendToLastTrace` /
   *  `attachOutputToLastTrace` wrap each tool invocation in unicode
   *  guillemet markers (U+2039/U+203A — `‹toolcall›…‹/toolcall›` and
   *  `‹output›…‹/output›`) so they don't collide with literal HTML
   *  the agent might emit in its prose. We forgive minor layout
   *  variation (inline vs newline, missing close on a partial
   *  stream) and also accept legacy plain-angle `<toolcall>…` for
   *  any pre-migration message that survived in the persisted log.
   *  Plain segments (already markdown-formatted by `formatToolUse`)
   *  fall through to a Markdown render. */
  function parseTraceSegment(
    seg: string
  ): { kind: 'tool'; cmd: string; output: string } | { kind: 'text' } {
    /* Detect either marker style. The unicode guillemets are the
       canonical wrapping today; plain `<…>` survives only on old
       persisted messages. */
    if (!/[‹<](toolcall|output)\b/.test(seg)) return { kind: 'text' };
    function extract(tag: 'toolcall' | 'output'): string {
      /* Try unicode markers first, then plain. Closed pair preferred,
         falling back to "open + rest of segment" for partial streams. */
      const closedU = new RegExp(`‹${tag}›([\\s\\S]*?)‹\\/${tag}›`).exec(seg);
      if (closedU) return closedU[1];
      const closedA = new RegExp(`<${tag}>([\\s\\S]*?)<\\/${tag}>`).exec(seg);
      if (closedA) return closedA[1];
      const openU = new RegExp(`‹${tag}›([\\s\\S]*)`).exec(seg);
      if (openU) return openU[1];
      const openA = new RegExp(`<${tag}>([\\s\\S]*)`).exec(seg);
      return openA ? openA[1] : '';
    }
    /* Strip any inner output chunk first — output is spliced INSIDE
       the toolcall envelope by `attachOutputToLastTrace`, so the
       closing toolcall marker only matches AFTER the output. Without
       this strip, cmd would include the whole captured text. Then
       drop any stray leftover tag markers + the leading `$ ` shell
       prompt (the ▸ glyph already conveys "this is a command"). */
    function clean(s: string, dropOutput: boolean): string {
      let r = s;
      if (dropOutput) {
        r = r.replace(/‹output›[\s\S]*?‹\/output›/g, '');
        r = r.replace(/<output>[\s\S]*?<\/output>/g, '');
      }
      return r
        .replace(/‹\/?toolcall›/g, '')
        .replace(/‹\/?output›/g, '')
        .replace(/<\/?toolcall>/g, '')
        .replace(/<\/?output>/g, '')
        .trim();
    }
    let cmd = clean(extract('toolcall'), true);
    cmd = cmd
      .replace(/^[`'"]?\s*\$\s+/, '')
      .replace(/[`'"]$/, '')
      .trim();
    const output = clean(extract('output'), false);
    return { kind: 'tool', cmd, output };
  }

  function diffStats(oldText: string, newText: string): { add: number; rem: number } {
    const rows = computeDiffRows(oldText ?? '', newText ?? '');
    let add = 0, rem = 0;
    for (const r of rows) {
      if (r.kind === 'add') add++;
      else if (r.kind === 'rem') rem++;
    }
    return { add, rem };
  }

  /** Tiny LCS-based line diff. Good enough for the chat-card preview —
   *  we're not trying to compete with `diff` here, just show the user
   *  what the agent changed without leaving the conversation.
   *  Returns ordered rows tagged add / rem / ctx. */
  type DiffRow = { kind: 'add' | 'rem' | 'ctx'; oldNo?: number; newNo?: number; text: string };
  function computeDiffRows(oldText: string, newText: string): DiffRow[] {
    const a = oldText.split('\n');
    const b = newText.split('\n');
    /* Build LCS dp table. O(m*n) — bounded to ~400 lines per side via
       the slice cap below so a giant write doesn't freeze the UI. */
    const CAP = 400;
    const aTrim = a.length > CAP ? a.slice(0, CAP) : a;
    const bTrim = b.length > CAP ? b.slice(0, CAP) : b;
    const m = aTrim.length, n = bTrim.length;
    const dp: number[][] = Array.from({ length: m + 1 }, () => new Array(n + 1).fill(0));
    for (let i = 1; i <= m; i++) {
      for (let j = 1; j <= n; j++) {
        if (aTrim[i - 1] === bTrim[j - 1]) dp[i][j] = dp[i - 1][j - 1] + 1;
        else dp[i][j] = Math.max(dp[i - 1][j], dp[i][j - 1]);
      }
    }
    const rows: DiffRow[] = [];
    let i = m, j = n;
    while (i > 0 && j > 0) {
      if (aTrim[i - 1] === bTrim[j - 1]) {
        rows.push({ kind: 'ctx', oldNo: i, newNo: j, text: aTrim[i - 1] });
        i--; j--;
      } else if (dp[i - 1][j] >= dp[i][j - 1]) {
        rows.push({ kind: 'rem', oldNo: i, text: aTrim[i - 1] });
        i--;
      } else {
        rows.push({ kind: 'add', newNo: j, text: bTrim[j - 1] });
        j--;
      }
    }
    while (i > 0) { rows.push({ kind: 'rem', oldNo: i, text: aTrim[i - 1] }); i--; }
    while (j > 0) { rows.push({ kind: 'add', newNo: j, text: bTrim[j - 1] }); j--; }
    rows.reverse();
    /* Collapse long stretches of unchanged context. Keep 2 lines of
       padding around each change so the user still gets locality. */
    return collapseContext(rows, 2);
  }
  function collapseContext(rows: DiffRow[], pad: number): DiffRow[] {
    const out: DiffRow[] = [];
    const n = rows.length;
    for (let i = 0; i < n; i++) {
      const r = rows[i];
      if (r.kind !== 'ctx') { out.push(r); continue; }
      /* Find next change. */
      let next = i;
      while (next < n && rows[next].kind === 'ctx') next++;
      const runLen = next - i;
      const isHead = out.length === 0;
      const isTail = next >= n;
      const head = isHead ? 0 : pad;
      const tail = isTail ? 0 : pad;
      if (runLen <= head + tail + 1) {
        for (let k = i; k < next; k++) out.push(rows[k]);
      } else {
        for (let k = i; k < i + head; k++) out.push(rows[k]);
        out.push({ kind: 'ctx', text: `··· ${runLen - head - tail} unchanged lines ···` });
        for (let k = next - tail; k < next; k++) out.push(rows[k]);
      }
      i = next - 1;
    }
    return out;
  }
</script>

{#if !sess}
  <div class="ct-empty">
    <p class="ct-empty-h serif">No active session</p>
    <p class="ct-empty-p">Pick a session from the sidebar or create a new one.</p>
  </div>
{:else}
  <div class="ct" bind:this={chatEl} onscroll={onChatScroll}>
    {#if sess.messages.length === 0 && sess.actions.length === 0}
      <div class="ct-welcome">
        <p class="ct-welcome-h serif">Ask {p.kind === 'claude' ? 'Claude' : 'Cursor'} anything</p>
        <p class="ct-welcome-p">Drop a Jira ticket / PR / file on the composer below to attach context. Use <span class="mono">/</span> for slash commands, <span class="mono">@</span> for files.</p>
      </div>
    {/if}

    {#each sess.messages as msg, i (i)}
      {#if msg.role === 'user'}
        <article class="msg msg--user">
          <div class="msg-byline msg-byline--user">@you</div>
          <div class="msg-body">
            <Markdown source={msg.content} onOpenFile={p.onOpenFile} />
            {#if msg.images && msg.images.length > 0}
              <div class="msg-images">
                {#each msg.images as img (img.path)}
                  <figure class="msg-image-fig">
                    <img class="msg-image" src={convertFileSrc(img.path)} alt={img.name} loading="lazy" />
                    <figcaption class="msg-image-name mono">{img.name}</figcaption>
                  </figure>
                {/each}
              </div>
            {/if}
            <div class="msg-actions">
              <button class="msg-act" onclick={() => p.onStartEditMessage(sess.id, i, msg.content)} title="Edit + resend">
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7"><path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7"/><path d="M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4 9.5-9.5z"/></svg>
              </button>
              <button class="msg-act" onclick={() => p.onResendMessage(sess.id, i, msg.content)} title="Resend">
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7"><path d="M21 12a9 9 0 1 1-3-6.7"/><path d="M21 4v5h-5"/></svg>
              </button>
            </div>
          </div>
        </article>
      {:else if msg.role === 'assistant'}
        <article class="msg msg--assistant">
          <div class="msg-byline msg-byline--assistant">{p.kind}</div>
          <div class="msg-body">
            {#if msg.thinking}
              <details class="thinking-pill">
                <summary>thinking ✓</summary>
                <pre class="thinking-body">{msg.thinking}</pre>
              </details>
            {/if}
            {#if msg.events && msg.events.length > 0}
              {#each msg.events as ev, ei (ei)}
                {#if ev.kind === 'text'}
                  {#if ev.body}<Markdown source={ev.body} onOpenFile={p.onOpenFile} />{/if}
                {:else if ev.kind === 'trace'}
                  <details class="trace" open>
                    <summary class="trace-head">
                      <span class="trace-check" aria-hidden="true">
                        <svg viewBox="0 0 24 24" width="9" height="9" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round"><polyline points="20 6 9 17 4 12"/></svg>
                      </span>
                      <span class="trace-head-label"><strong>{ev.segments.length}</strong> step{ev.segments.length === 1 ? '' : 's'}</span>
                      <span class="trace-head-caret" aria-hidden="true">
                        <svg viewBox="0 0 24 24" width="11" height="11" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"><path d="M6 9l6 6 6-6"/></svg>
                      </span>
                    </summary>
                    <div class="trace-body">
                      {#each ev.segments as seg, si (si)}
                        {@const parsed = parseTraceSegment(seg)}
                        {#if parsed.kind === 'tool' && (parsed.cmd || parsed.output)}
                          <div class="trace-step">
                            {#if parsed.cmd}
                              <div class="trace-cmd-bubble">
                                <span class="trace-cmd-prompt mono">$</span>
                                <code class="trace-cmd-text mono">{parsed.cmd}</code>
                              </div>
                            {/if}
                            {#if parsed.output}
                              {@const lineCount = parsed.output.split('\n').length}
                              <details class="trace-out-bubble">
                                <summary class="trace-out-head">
                                  <span class="trace-out-caret" aria-hidden="true">
                                    <svg viewBox="0 0 24 24" width="10" height="10" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"><path d="M6 9l6 6 6-6"/></svg>
                                  </span>
                                  <span class="trace-out-tag mono">output</span>
                                  <span class="trace-out-meta mono">{lineCount} line{lineCount === 1 ? '' : 's'}</span>
                                </summary>
                                <pre class="trace-out-body mono">{parsed.output}</pre>
                              </details>
                            {/if}
                          </div>
                        {:else if parsed.kind === 'text'}
                          <div class="trace-line"><Markdown source={seg} onOpenFile={p.onOpenFile} /></div>
                        {/if}
                      {/each}
                    </div>
                  </details>
                {:else if ev.kind === 'edit'}
                  {@const stats = diffStats(ev.oldText ?? '', ev.newText ?? '')}
                  <details class="edit-card">
                    <summary class="edit-card-head">
                      {#if ev.isCreate}
                        <span class="edit-tag edit-tag--add">Create</span>
                      {:else if ev.isDelete}
                        <span class="edit-tag edit-tag--rem">Delete</span>
                      {:else if ev.wholeFile}
                        <span class="edit-tag">Write</span>
                      {:else}
                        <span class="edit-tag">Edit</span>
                      {/if}
                      <span class="edit-path mono">{ev.filePath}</span>
                      <span class="edit-stats mono">
                        <span class="add">+{stats.add}</span>
                        <span class="rem">−{stats.rem}</span>
                      </span>
                      <span class="edit-status mono">{ev.status}</span>
                      <span class="edit-expand">
                        <svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"><path d="M6 9l6 6 6-6"/></svg>
                      </span>
                    </summary>
                    <div class="edit-card-body">
                      <div class="diff">
                        {#each computeDiffRows(ev.oldText ?? '', ev.newText ?? '') as row, ri (ri)}
                          <div class="diff-row diff-row--{row.kind}">
                            <span class="diff-no mono">{row.oldNo ?? ''}</span>
                            <span class="diff-no mono">{row.newNo ?? ''}</span>
                            <span class="diff-glyph mono">{row.kind === 'add' ? '+' : row.kind === 'rem' ? '−' : ' '}</span>
                            <span class="diff-text mono">{row.text}</span>
                          </div>
                        {/each}
                      </div>
                    </div>
                  </details>
                {/if}
              {/each}
            {:else if msg.content}
              <Markdown source={msg.content} onOpenFile={p.onOpenFile} />
            {/if}
            {#if sess.sending && i === sess.messages.length - 1 && (!msg.content || msg.content.length < 6)}
              <div class="thinking">
                <span class="dot-row">
                  <span class="dot"></span>
                  <span class="dot"></span>
                  <span class="dot"></span>
                </span>
                <span>thinking{elapsed ? ` · ${elapsed}` : ''}</span>
              </div>
            {/if}
            {#if msg.usage}
              <div class="msg-usage mono">
                {Math.round(msg.usage.contextSize / 1000)}K context · {msg.usage.outputTokens} out
              </div>
            {/if}
          </div>
        </article>
      {:else}
        <article class="msg msg--system">
          <div class="msg-system">{msg.content}</div>
        </article>
      {/if}
    {/each}

    {#each sess.actions as action (action.id)}
      <div class="action-wrap">
        <ClaudeActionCard
          {action}
          onUpdate={(patch) => p.onUpdateAction(sess.id, action.id, patch)}
          onDismiss={() => p.onRemoveAction(sess.id, action.id)}
          onExecute={() => p.onExecuteAction(sess.id, action)}
          onOpenPrInWoom={(url) => p.onOpenPrInWoom(url, action.kind === 'pr' ? action : null)}
          {repoCwd}
        />
      </div>
    {/each}
  </div>
{/if}

<style>
  .ct {
    flex: 1; min-height: 0;
    overflow-y: auto;
    padding: 28px 28px 12px;
    display: flex; flex-direction: column; gap: 24px;
  }

  /* v7 — 76px byline column + 1fr body. */
  .msg {
    display: grid;
    grid-template-columns: 76px 1fr;
    gap: 16px;
    align-items: start;
  }
  .msg--system { grid-template-columns: 1fr; }

  .msg-byline {
    font-size: 11px;
    color: var(--text-mute);
    letter-spacing: 0.04em;
    font-weight: 600;
    padding-top: 4px;
  }
  .msg-byline--user { color: var(--text-2); }
  .msg-byline--assistant {
    color: var(--app-tone, var(--src-claude));
    text-transform: lowercase;
    letter-spacing: -0.01em;
    font-family: 'Geist', 'Inter', -apple-system, system-ui, sans-serif;
    font-size: 13px;
    font-weight: 600;
  }

  .msg-body { min-width: 0; position: relative; }
  .msg-body :global(p) { margin: 0 0 8px; line-height: 1.6; color: var(--text-0); }
  .msg-body :global(p:last-child) { margin-bottom: 0; }
  .msg-body :global(strong) { color: var(--text-0); font-weight: 600; }
  .msg-body :global(code) {
    font-family: 'JetBrains Mono', monospace;
    font-size: 12.5px;
    padding: 1px 6px;
    background: var(--bg-2);
    border: 1px solid var(--border);
    border-radius: 4px;
    color: var(--accent-bright);
  }

  /* User message — soft bg + 2px clay stripe on the left. The
     `position: relative` anchor lets the absolute-positioned hover
     actions float at the top-right without inflating the bubble's
     resting height. */
  .msg--user .msg-body {
    position: relative;
    padding: 12px 16px 12px 18px;
    background: linear-gradient(180deg, var(--bg-2),
      color-mix(in srgb, var(--bg-2) 90%, var(--accent-soft)));
    border: 1px solid var(--border);
    border-left: 2px solid var(--accent);
    border-radius: 10px;
  }

  /* Hover actions sit BELOW the bubble (not over it) — small, naked
     icons, only revealed on hover. No backdrop, no border, no shadow:
     they read as a quiet utility row and don't compete with the prose
     above. The bubble's resting height is content-only because the
     actions are absolute-positioned just below the bottom edge. */
  .msg-actions {
    position: absolute;
    bottom: -22px;
    right: 4px;
    display: flex; gap: 2px;
    opacity: 0;
    transition: opacity 140ms;
    pointer-events: none;
  }
  .msg--user:hover .msg-actions,
  .msg--user:focus-within .msg-actions {
    opacity: 0.85;
    pointer-events: auto;
  }
  .msg-act {
    width: 20px; height: 20px;
    display: grid; place-items: center;
    color: var(--text-mute);
    background: transparent; border: 0; cursor: pointer;
    border-radius: 4px;
    padding: 0;
    transition: color 120ms;
  }
  .msg-act:hover { color: var(--text-0); }
  .msg-act svg { width: 12px; height: 12px; }

  /* External-file attachments (today: pasted/dropped images). Plain
     external file refs typed via @ live in the prompt text inline,
     not here. */
  .msg-images { margin-top: 10px; display: flex; gap: 8px; flex-wrap: wrap; }
  .msg-image-fig {
    margin: 0;
    display: flex; flex-direction: column;
    gap: 4px;
    max-width: 240px;
  }
  .msg-image {
    max-width: 240px; max-height: 200px;
    border-radius: 8px;
    border: 1px solid var(--border);
  }
  .msg-image-name {
    font-size: 10px;
    color: var(--text-mute);
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }

  .msg-system {
    font-size: 11.5px;
    color: var(--text-mute);
    text-align: center;
    
    padding: 4px 12px;
  }

  /* Trace card — outer rounded bubble that holds the head row plus
     a stack of step sub-bubbles when expanded. The bubble has a soft
     bg + brand-tinted left stripe so it stands apart from prose. */
  .trace {
    display: block;
    margin: 6px 0;
    max-width: 720px;
    background: linear-gradient(180deg,
      color-mix(in srgb, var(--app-tone, var(--src-claude)) 4%, var(--bg-2)),
      var(--bg-2));
    border: 1px solid var(--border);
    border-left: 2px solid color-mix(in srgb, var(--app-tone, var(--src-claude)) 70%, var(--border));
    border-radius: 10px;
    overflow: hidden;
  }
  .trace-head {
    display: flex; align-items: center; gap: 8px;
    padding: 8px 12px;
    font-size: 12px;
    color: var(--text-1);
    cursor: pointer;
    user-select: none;
    list-style: none;
  }
  .trace-head::-webkit-details-marker { display: none; }
  .trace-head::marker { content: ''; }
  .trace-head:hover { background: color-mix(in srgb, var(--app-tone, var(--src-claude)) 5%, transparent); }
  .trace-check {
    width: 15px; height: 15px;
    display: grid; place-items: center;
    background: color-mix(in srgb, var(--success) 22%, transparent);
    color: var(--success);
    border-radius: 50%;
    flex-shrink: 0;
  }
  .trace-head-label {
    font-size: 12px;
    color: var(--text-1);
    flex: 1;
  }
  .trace-head :global(strong) {
    color: var(--text-0);
    font-weight: 600;
    margin-right: 2px;
  }
  .trace-head-caret {
    color: var(--text-mute);
    display: grid; place-items: center;
    transition: transform 160ms;
  }
  .trace[open] .trace-head-caret { transform: rotate(180deg); }
  .trace[open] .trace-head { border-bottom: 1px solid var(--border); }
  .trace-body {
    padding: 10px 12px 12px;
    display: flex; flex-direction: column;
    gap: 10px;
  }
  .trace-step {
    display: flex; flex-direction: column;
    gap: 6px;
  }
  /* Inner command sub-bubble — terminal-feel surface with a subtle
     prompt glyph + brand-tinted prompt. Sequence of calls reads like
     a vertical script. */
  .trace-cmd-bubble {
    display: flex; align-items: flex-start; gap: 9px;
    padding: 7px 11px;
    background: var(--bg-1);
    border: 1px solid var(--border);
    border-radius: 7px;
    box-shadow: 0 1px 0 rgba(0, 0, 0, 0.04);
  }
  .trace-cmd-prompt {
    flex-shrink: 0;
    color: var(--app-tone, var(--src-claude));
    font-size: 12px;
    line-height: 1.55;
    font-weight: 600;
    user-select: none;
  }
  .trace-cmd-text {
    flex: 1; min-width: 0;
    font-size: 12px;
    color: var(--text-0);
    word-break: break-all;
    line-height: 1.55;
    background: transparent;
    border: none;
    padding: 0;
  }
  /* Inner output sub-bubble — collapsible. Header reads "output ·
     N lines"; expanded body shows the mono pre. */
  .trace-out-bubble {
    background: var(--bg-1);
    border: 1px solid var(--border);
    border-radius: 7px;
    overflow: hidden;
  }
  .trace-out-head {
    display: flex; align-items: center; gap: 7px;
    padding: 6px 11px;
    cursor: pointer;
    font-size: 10.5px;
    color: var(--text-mute);
    user-select: none;
    list-style: none;
    transition: background 120ms;
  }
  .trace-out-head::-webkit-details-marker { display: none; }
  .trace-out-head::marker { content: ''; }
  .trace-out-head:hover { background: var(--bg-2); }
  .trace-out-caret {
    display: inline-grid; place-items: center;
    color: var(--text-mute);
    transition: transform 140ms;
  }
  .trace-out-bubble[open] .trace-out-caret { transform: rotate(180deg); }
  .trace-out-bubble[open] .trace-out-head { border-bottom: 1px solid var(--border); }
  .trace-out-tag {
    font-size: 10px;
    color: var(--text-2);
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }
  .trace-out-meta {
    font-size: 10px;
    color: var(--text-mute);
    margin-left: auto;
  }
  .trace-out-body {
    margin: 0;
    padding: 9px 11px;
    font-size: 11px;
    line-height: 1.5;
    color: var(--text-1);
    white-space: pre-wrap;
    word-break: break-word;
    max-height: 300px;
    overflow: auto;
    background: var(--bg-1);
  }
  /* Plain text segment fallback — markdown-rendered. */
  .trace-line {
    font-size: 12px;
    color: var(--text-1);
    line-height: 1.55;
  }
  .trace-line :global(p) { margin: 0; }
  .trace-line :global(code) {
    font-family: 'JetBrains Mono', monospace;
    font-size: 11.5px;
    padding: 1px 6px;
    background: var(--bg-3);
    border: 1px solid var(--border);
    border-radius: 4px;
    color: var(--text-0);
  }
  .trace-line :global(em) {
    color: var(--text-2);
    
  }

  /* Edit card — collapsible file pill. Header: tag + path + +/- stats +
     status + caret. Body: real LCS diff with line numbers + glyph. */
  .edit-card {
    margin: 6px 0;
    background: var(--bg-2);
    border: 1px solid var(--border);
    border-left: 2px solid var(--src-editor);
    border-radius: 8px;
    font-size: 12.5px;
    overflow: hidden;
  }
  .edit-card-head {
    display: flex; align-items: center;
    gap: 12px;
    padding: 10px 14px;
    cursor: pointer;
    user-select: none;
    list-style: none;
  }
  .edit-card-head::-webkit-details-marker { display: none; }
  .edit-card-head::marker { content: ''; }
  .edit-card[open] .edit-card-head {
    border-bottom: 1px solid var(--border);
    background: linear-gradient(180deg,
      color-mix(in srgb, var(--src-editor) 5%, transparent),
      transparent);
  }
  .edit-card[open] .edit-expand svg { transform: rotate(180deg); }
  .edit-tag {
    display: inline-flex; align-items: center;
    padding: 2px 7px;
    font-size: 10px; font-weight: 600;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    background: var(--bg-3);
    border-radius: 4px;
    color: var(--text-2);
    flex-shrink: 0;
  }
  .edit-tag--add { color: var(--diff-add-stroke); }
  .edit-tag--rem { color: var(--diff-rem-stroke); }
  .edit-path {
    font-size: 12px;
    color: var(--text-0);
    flex: 1;
    white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
  }
  .edit-stats { display: flex; gap: 8px; font-size: 11px; }
  .edit-stats .add { color: var(--diff-add); }
  .edit-stats .rem { color: var(--diff-rem); }
  .edit-status {
    font-size: 9.5px; color: var(--text-mute);
    text-transform: uppercase; letter-spacing: 0.08em;
    padding: 1px 5px;
    border-radius: 3px;
    background: var(--bg-3);
    border: 1px solid var(--border);
  }
  .edit-expand {
    color: var(--text-mute);
    display: inline-grid; place-items: center;
    transition: transform 160ms;
  }
  .edit-expand svg { transition: transform 160ms; }

  .edit-card-body {
    max-height: 480px;
    overflow: auto;
    background: var(--bg-1);
  }
  .diff {
    display: block;
    font-family: 'JetBrains Mono', monospace;
    font-size: 11.5px;
    line-height: 1.55;
  }
  .diff-row {
    display: grid;
    grid-template-columns: 36px 36px 16px 1fr;
    gap: 0;
    padding: 0 6px;
    white-space: pre;
  }
  .diff-row--add {
    background: rgba(101, 211, 150, 0.08);
  }
  .diff-row--add .diff-glyph { color: var(--diff-add); }
  .diff-row--add .diff-text { color: var(--text-0); }
  .diff-row--rem {
    background: rgba(232, 130, 100, 0.08);
  }
  .diff-row--rem .diff-glyph { color: var(--diff-rem); }
  .diff-row--rem .diff-text { color: var(--text-1); text-decoration: line-through; text-decoration-thickness: 0.5px; text-decoration-color: rgba(232, 130, 100, 0.45); }
  .diff-row--ctx .diff-glyph { color: var(--text-mute); }
  .diff-row--ctx .diff-text { color: var(--text-2); }
  .diff-no {
    color: var(--text-mute);
    text-align: right;
    padding-right: 8px;
    user-select: none;
    font-size: 10.5px;
    line-height: 1.55;
  }
  .diff-glyph {
    text-align: center;
    user-select: none;
    color: var(--text-mute);
  }
  .diff-text {
    overflow: hidden;
    text-overflow: ellipsis;
  }

  /* Thinking pill (reasoning trace, collapsible) */
  .thinking-pill {
    margin-bottom: 10px;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--bg-3);
  }
  .thinking-pill summary {
    padding: 6px 10px;
    font-family: 'JetBrains Mono', monospace; font-size: 10.5px;
    color: var(--text-mute);
    cursor: pointer;
    user-select: none;
  }
  .thinking-body {
    margin: 0; padding: 8px 12px;
    border-top: 1px solid var(--border);
    font-family: 'JetBrains Mono', monospace; font-size: 11px;
    color: var(--text-1); line-height: 1.5;
    white-space: pre-wrap;
    max-height: 320px; overflow: auto;
  }

  /* Live thinking indicator — 3 staggered pulsing dots + elapsed. */
  .thinking {
    display: flex; align-items: center; gap: 10px;
    padding: 6px 0;
    font-size: 12px;
    color: var(--text-mute);
    
  }
  .dot-row { display: inline-flex; gap: 4px; }
  .dot-row .dot {
    width: 5px; height: 5px;
    border-radius: 50%;
    background: var(--app-tone, var(--src-claude));
    opacity: 0.6;
    animation: ct-pulse 1.2s infinite;
  }
  .dot-row .dot:nth-child(2) { animation-delay: 0.18s; }
  .dot-row .dot:nth-child(3) { animation-delay: 0.36s; }
  @keyframes ct-pulse {
    0%, 80%, 100% { opacity: 0.3; transform: scale(0.85); }
    40%           { opacity: 1;   transform: scale(1.15); }
  }

  .msg-usage {
    margin-top: 8px;
    font-size: 10px; color: var(--text-mute);
  }

  .action-wrap { width: 100%; }

  .ct-empty, .ct-welcome {
    margin: auto;
    text-align: center;
    padding: 60px 20px;
    max-width: 480px;
  }
  .ct-empty-h, .ct-welcome-h {
    font-family: 'Geist', 'Inter', -apple-system, system-ui, sans-serif;
    font-size: 26px; font-weight: 600; letter-spacing: -0.015em;
    color: var(--text-0);
    margin: 0 0 10px;
  }
  .ct-empty-p, .ct-welcome-p {
    font-size: 13px; color: var(--text-2);
    line-height: 1.55; margin: 0;
  }
  .ct-welcome-p .mono {
    font-family: 'JetBrains Mono', monospace; font-size: 11.5px;
    padding: 1px 5px; background: var(--bg-2); border: 1px solid var(--border);
    border-radius: 4px;
    color: var(--accent-bright);
  }
</style>
