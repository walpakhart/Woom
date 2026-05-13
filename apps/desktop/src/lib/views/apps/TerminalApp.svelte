<script lang="ts">
  /* TerminalApp — full-screen workspace for the terminal.
     Layout: [TerminalSurface (flex)] [InlineClaude side pane (300)]

     The right pane is the SAME `<InlineClaude>` component the editor
     uses, parameterised with `linkKind="terminal"` so it filters by
     `linkedTerminalInstanceId` and surfaces a "+ Link…" picker so the
     user can attach a Claude / Cursor chat without leaving the
     terminal app. Once linked, that chat's row in the pane behaves
     identically to the editor's: click → expand mini-composer; the
     Apply popover (below) pipes selected terminal text straight in.

     Selection-bridge: TerminalSurface streams xterm selection state up
     here via `onSelectionChange`. When something is highlighted AND at
     least one agent is linked, a floating "Apply to <agent>" popover
     anchors to the end of the selection. Clicking it pins the captured
     text as a `@terminal/<label>:<hash>` mention into the target
     session's composer (via `applyTerminalSelectionToAgent`) and
     auto-expands the inline-agents row so the user can tack on a
     question. Same UX as the editor's "Apply to" bar — just sourced
     from a shell selection instead of a CodeMirror range. */

  import TerminalSurface from './terminal/TerminalSurface.svelte';
  import InlineClaude from './editor/InlineClaude.svelte';
  import Splitter from '$lib/components/ui/Splitter.svelte';
  import SidePaneRail from '$lib/components/ui/SidePaneRail.svelte';
  import { layoutState, kindForInstanceId, APP_INSTANCE_IDS } from '$lib/state/layout.svelte';
  import { sessionsState } from '$lib/state/sessions.svelte';
  import { applyTerminalSelectionToAgent } from '$lib/services/applyToAgent';
  import { clearTerminalScrollback } from '$lib/state/terminals.svelte';
  import { fly } from 'svelte/transition';
  import { cubicOut } from 'svelte/easing';

  interface Props {
    instanceId: string;
    cwd?: string | null;
    onOpenClaude: () => void;
    onOpenCursor: () => void;
    /** Quick-send to a specific session — same shape as EditorApp's
     *  prop, threaded through +page.svelte's `quickSendToSession`.
     *  Fires immediately if idle, queues if a turn is in flight. */
    onQuickSend?: (sessionId: string, text: string) => void;
    /** Activate a session AND switch the top-level view to its agent
     *  app — per-row "Open" affordance on each inline-agents card. */
    onOpenSession?: (sessionId: string, agentInstanceId: string) => void;
    /** Bind a chat session to this terminal (sets
     *  `linkedTerminalInstanceId`). Surfaced as a picker in the
     *  InlineClaude header so the user doesn't have to bounce out to
     *  the agent app's cwd bar to set up the link. */
    onLinkSession?: (sessionId: string) => void;
    /** Drop the link from a specific session. Wired to the × button
     *  on each inline-agents card. */
    onUnlinkSession?: (sessionId: string) => void;
  }
  let p: Props = $props();

  let sideOpen = $state(true);

  /** Curated mark of the active Terminal instance — surfaces in the
   *  @-mention's title when the user applies a selection, so the agent
   *  reads which terminal the output came from. */
  const instanceLabel = $derived(
    layoutState.instances.terminal.find((i) => i.id === p.instanceId)?.name ?? 'Terminal'
  );

  /** Sessions linked TO this terminal — used here ONLY to feed the
   *  floating Apply popover's button list. The InlineClaude pane
   *  derives its own copy from the same fields, so we don't pass this
   *  in. Kept local to keep the Apply pipeline self-contained. */
  const linkedAgents = $derived.by(() => {
    const out: { sessionId: string; agentInstanceId: string; kind: 'claude' | 'cursor'; title: string }[] = [];
    for (const s of sessionsState.list) {
      if (s.linkedTerminalInstanceId !== p.instanceId) continue;
      const agentInstanceId =
        s.agentInstanceId
        ?? (s.agentKind === 'claude' || s.agentKind === 'cursor'
          ? APP_INSTANCE_IDS[s.agentKind]
          : null);
      if (!agentInstanceId) continue;
      const kind = kindForInstanceId(agentInstanceId);
      if (kind !== 'claude' && kind !== 'cursor') continue;
      out.push({ sessionId: s.id, agentInstanceId, kind, title: s.title });
    }
    return out;
  });

  /** Live xterm selection — `null` when nothing is highlighted. The
   *  popover renders iff this is non-null AND `linkedAgents` is
   *  non-empty. Cleared by:
   *    • the user picking "Apply to <agent>" (`clearSelRef.fn`)
   *    • the user collapsing the selection in xterm (callback fires
   *      with null)
   *    • re-mounting the surface (the ref's `fn` resets to null in
   *      onDestroy, so a stale ref can't fire on a new instance). */
  let xtermSelection = $state<{
    text: string;
    anchor: { x: number; y: number };
  } | null>(null);

  /** Imperative handle into TerminalSurface — set on mount, used
   *  after a successful Apply to clear xterm's native highlight so
   *  the popover doesn't linger over a phantom selection. */
  let clearSelRef = $state<{ fn: (() => void) | null }>({ fn: null });

  /* Same shape as EditorView's `applyButtons` — collapse to "Claude"
     when there's exactly one of a kind, prefix per-session names when
     two+ Claudes/Cursors are linked. Keeps the popover scannable. */
  type ApplyBtn = {
    sessionId: string;
    agentInstanceId: string;
    label: string;
    kind: 'claude' | 'cursor';
  };
  const applyButtons = $derived.by<ApplyBtn[]>(() => {
    if (linkedAgents.length === 0) return [];
    const byKind: Record<'claude' | 'cursor', typeof linkedAgents> = { claude: [], cursor: [] };
    for (const a of linkedAgents) byKind[a.kind].push(a);
    const out: ApplyBtn[] = [];
    for (const k of ['claude', 'cursor'] as const) {
      const group = byKind[k];
      if (group.length === 0) continue;
      const kindLabel = k === 'claude' ? 'Claude' : 'Cursor';
      if (group.length === 1) {
        out.push({
          sessionId: group[0].sessionId,
          agentInstanceId: group[0].agentInstanceId,
          kind: k,
          label: kindLabel
        });
      } else {
        for (const a of group) {
          out.push({
            sessionId: a.sessionId,
            agentInstanceId: a.agentInstanceId,
            kind: k,
            label: `${kindLabel} · ${a.title}`
          });
        }
      }
    }
    return out;
  });

  function handleApplyTo(btn: ApplyBtn) {
    if (!xtermSelection) return;
    applyTerminalSelectionToAgent({
      sessionId: btn.sessionId,
      agentInstanceId: btn.agentInstanceId,
      terminalLabel: instanceLabel,
      content: xtermSelection.text
    });
    xtermSelection = null;
    clearSelRef.fn?.();
  }

  /** Wipe the captured scrollback + reset the live xterm. The shell
   *  process keeps running — same session, fresh screen. State-level
   *  call also clears any cached error banner so the surface comes
   *  back to a pristine "you can type now" state. */
  function clearScreen() {
    clearTerminalScrollback(p.instanceId);
  }
</script>

<section
  class="app-shell st-shell"
  class:st-shell--rail={!sideOpen}
  style="--app-tone: var(--src-term); --app-glow: rgba(245,240,234,0.30);"
>
  {#if sideOpen}
    <Splitter
      direction="horizontal"
      fixedSide="end"
      persistKey="terminal-side"
      initial={300}
      min={240}
      max={520}
    >
      {#snippet start()}
        <section class="app-pane st-main">
          <TerminalSurface
            instanceId={p.instanceId}
            cwd={p.cwd ?? null}
            onSelectionChange={(s) => (xtermSelection = s)}
            clearSelectionRef={clearSelRef}
          />
          <!-- Clear-screen affordance — sits in the top-right corner
               of the terminal surface, only fully opaque on hover so
               it doesn't compete with the shell prompt at rest.
               Wipes the captured scrollback + xterm view; the PTY
               keeps running so the user comes back to the same
               session with a fresh screen. -->
          <button
            class="st-clear"
            onclick={clearScreen}
            title="Clear terminal (keeps the shell session running)"
            aria-label="Clear terminal"
          >
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
              <path d="M3 6h18"/>
              <path d="M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/>
              <path d="M19 6 18 20a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2L5 6"/>
            </svg>
          </button>
          {#if xtermSelection && applyButtons.length > 0}
            <div
              class="st-apply-pop"
              style:left="{xtermSelection.anchor.x}px"
              style:top="{xtermSelection.anchor.y}px"
              role="toolbar"
              aria-label="Apply terminal selection to agent"
            >
              {#each applyButtons as btn (btn.sessionId)}
                <button
                  class="st-apply-pop-btn"
                  class:claude={btn.kind === 'claude'}
                  class:cursor={btn.kind === 'cursor'}
                  onmousedown={(e) => e.preventDefault()}
                  onclick={() => handleApplyTo(btn)}
                  title={`Pin selection to ${btn.label}'s composer`}
                >
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                    <path d="M5 12h12M13 6l6 6-6 6"/>
                  </svg>
                  <span>Apply to {btn.label}</span>
                </button>
              {/each}
            </div>
          {/if}
        </section>
      {/snippet}
      {#snippet end()}
        <aside class="app-pane st-side" in:fly={{ x: 24, duration: 220, easing: cubicOut }}>
          <InlineClaude
            instanceId={p.instanceId}
            linkKind="terminal"
            onClose={() => (sideOpen = false)}
            onOpenClaude={p.onOpenClaude}
            onQuickSend={p.onQuickSend ?? (() => {})}
            onOpenSession={p.onOpenSession ?? (() => {})}
            onLinkSession={p.onLinkSession}
            onUnlinkSession={p.onUnlinkSession}
          />
        </aside>
      {/snippet}
    </Splitter>
  {:else}
    <section class="app-pane st-main">
      <TerminalSurface
        instanceId={p.instanceId}
        cwd={p.cwd ?? null}
        onSelectionChange={(s) => (xtermSelection = s)}
        clearSelectionRef={clearSelRef}
      />
      <button
        class="st-clear"
        onclick={clearScreen}
        title="Clear terminal (keeps the shell session running)"
        aria-label="Clear terminal"
      >
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <path d="M3 6h18"/>
          <path d="M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/>
          <path d="M19 6 18 20a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2L5 6"/>
        </svg>
      </button>
      {#if xtermSelection && applyButtons.length > 0}
        <div
          class="st-apply-pop"
          style:left="{xtermSelection.anchor.x}px"
          style:top="{xtermSelection.anchor.y}px"
          role="toolbar"
          aria-label="Apply terminal selection to agent"
        >
          {#each applyButtons as btn (btn.sessionId)}
            <button
              class="st-apply-pop-btn"
              class:claude={btn.kind === 'claude'}
              class:cursor={btn.kind === 'cursor'}
              onmousedown={(e) => e.preventDefault()}
              onclick={() => handleApplyTo(btn)}
              title={`Pin selection to ${btn.label}'s composer`}
            >
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                <path d="M5 12h12M13 6l6 6-6 6"/>
              </svg>
              <span>Apply to {btn.label}</span>
            </button>
          {/each}
        </div>
      {/if}
    </section>
    <div class="st-rail-slot" in:fly={{ x: 24, duration: 220, easing: cubicOut }}>
      <SidePaneRail
        linkedAgents={linkedAgents.map((la) => ({
          sessionId: la.sessionId,
          agentInstanceId: la.agentInstanceId,
          kind: la.kind,
          title: la.title
        }))}
        onExpand={() => (sideOpen = true)}
      />
    </div>
  {/if}
</section>

<style>
  .st-shell { display: block; padding: var(--app-pad, 14px); }
  .st-shell.st-shell--rail {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 52px;
    gap: 0;
    transition: grid-template-columns var(--dur-base) var(--ease-out);
  }
  .st-rail-slot { height: 100%; min-width: 0; }
  .st-rail-slot :global(.spr) { width: 100%; }
  .st-shell :global(.s-start),
  .st-shell :global(.s-end) {
    height: 100%;
    display: flex;
    min-width: 0;
  }
  .st-shell :global(.s-start) > :global(*),
  .st-shell :global(.s-end) > :global(*) {
    flex: 1 1 auto;
    width: 100%;
    min-width: 0;
  }
  /* The shared InlineClaude pane uses 280px by default; let it stretch
     to whatever the splitter assigns instead of locking to its own. */
  .st-shell :global(.ic) { width: 100%; flex: 1; }

  .st-main {
    display: flex;
    overflow: hidden;
    background: var(--bg-0);
    position: relative;
    height: 100%;
  }
  .st-main :global(.terminal-surface) {
    background: var(--bg-0) !important;
    flex: 1 1 auto;
  }
  /* Clear-screen pill — sits one slot below the show-side toggle so
     the two affordances stack neatly when both are visible. Faded by
     default, fades up on parent hover so it doesn't compete with the
     shell prompt at rest. */
  .st-clear {
    position: absolute;
    top: 14px; right: 14px;
    width: 26px; height: 26px;
    display: grid; place-items: center;
    border-radius: 6px;
    background: rgba(20, 24, 26, 0.7);
    border: 1px solid var(--border);
    color: var(--text-2);
    cursor: pointer;
    backdrop-filter: blur(8px);
    opacity: 0;
    transition: opacity 160ms, color 140ms, border-color 140ms, background 140ms;
    z-index: 5;
  }
  .st-main:hover .st-clear,
  .st-clear:focus-visible {
    opacity: 0.85;
  }
  .st-clear:hover {
    opacity: 1;
    color: var(--accent-bright);
    border-color: color-mix(in srgb, var(--accent) 38%, var(--border));
    background: rgba(20, 24, 26, 0.92);
  }
  .st-clear svg { width: 13px; height: 13px; }

  /* Floating "Apply to <agent>" popover — same look + layering as
     EditorView's `.ev-apply-pop`. Anchored to the end of the
     selection via fixed-position viewport coordinates from
     TerminalSurface's xterm-cell metrics; brand-color edge per agent
     kind so the user reads the routing without parsing the label. */
  .st-apply-pop {
    position: fixed;
    z-index: 1000;
    transform: translate(8px, 6px);
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 3px;
    background: var(--bg-2);
    border: 1px solid var(--border-hi);
    border-radius: 7px;
    box-shadow: 0 6px 20px -6px rgba(0, 0, 0, 0.55), 0 1px 0 0 rgba(0, 0, 0, 0.1);
    white-space: nowrap;
  }
  .st-apply-pop-btn {
    display: inline-flex; align-items: center; gap: 6px;
    padding: 4px 10px;
    border-radius: 5px;
    background: transparent;
    border: 1px solid transparent;
    color: var(--text-0);
    font-size: 12px; font-weight: 500;
    cursor: pointer;
    transition: background 100ms, border-color 100ms, color 100ms;
  }
  .st-apply-pop-btn:hover {
    background: var(--accent-soft);
    border-color: var(--accent);
  }
  .st-apply-pop-btn svg { width: 12px; height: 12px; opacity: 0.85; }
  .st-apply-pop-btn.claude { border-left: 2px solid var(--src-claude); padding-left: 8px; }
  .st-apply-pop-btn.cursor { border-left: 2px solid var(--src-cursor); padding-left: 8px; }
</style>
