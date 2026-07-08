<script lang="ts">
  /* TerminalApp — full-screen workspace for the terminal.
     Layout: [TerminalSurface (flex)] [InlineClaude side pane (300)]

     The right pane is the SAME `<InlineClaude>` component the editor
     uses, parameterised with `linkKind="terminal"` so it filters by
     `linkedTerminalInstanceId` and surfaces a "+ Link…" picker so the
     user can attach a Claude chat without leaving the
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
  import { layoutState, kindForInstanceId, APP_INSTANCE_IDS, setActiveInstance, addInstance, removeInstance } from '$lib/state/layout.svelte';
  import { layoutModeState } from '$lib/state/layoutMode.svelte';
  import { sessionsState } from '$lib/state/sessions.svelte';
  import { applyTerminalSelectionToAgent } from '$lib/services/applyToAgent';
  import { clearTerminalScrollback } from '$lib/state/terminals.svelte';
  import { fly } from 'svelte/transition';
  import { cubicOut } from 'svelte/easing';

  interface Props {
    instanceId: string;
    cwd?: string | null;
    onOpenClaude: () => void;
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

  /* Redesign v2 §2.7 — instance list column. Switching sets the active
     terminal instance; +page's `{#key activeInstance.terminal}` remounts
     this app on the new PTY. */
  const termInstances = $derived(layoutState.instances.terminal);
  const activeTermId = $derived(layoutState.activeInstance.terminal);
  function removeTerminal(id: string, e: MouseEvent) {
    e.stopPropagation();
    removeInstance('terminal', id);
  }

  const quiet = $derived(layoutModeState.mode === 'quiet');
  /* Quiet §3.4 — PTY inset almost full-window; the instance list
     collapses into a «Name ▾» switcher floating at the traffic-lights. */
  const cwdShort = $derived(p.cwd ? p.cwd.replace(/^\/Users\/[^/]+/, '~') : '');
  let termSwitchOpen = $state(false);
  function pickTerminal(id: string) {
    setActiveInstance('terminal', id);
    termSwitchOpen = false;
  }
  $effect(() => {
    if (!termSwitchOpen) return;
    const onDown = (e: MouseEvent) => {
      const t = e.target as HTMLElement | null;
      if (t?.closest?.('.qsolo-float')) return;
      termSwitchOpen = false;
    };
    window.addEventListener('mousedown', onDown, true);
    return () => window.removeEventListener('mousedown', onDown, true);
  });

  /** Sessions linked TO this terminal — used here ONLY to feed the
   *  floating Apply popover's button list. The InlineClaude pane
   *  derives its own copy from the same fields, so we don't pass this
   *  in. Kept local to keep the Apply pipeline self-contained. */
  const linkedAgents = $derived.by(() => {
    const out: { sessionId: string; agentInstanceId: string; title: string }[] = [];
    for (const s of sessionsState.list) {
      if (s.archived) continue;
      if (s.linkedTerminalInstanceId !== p.instanceId) continue;
      const agentInstanceId = s.agentInstanceId ?? APP_INSTANCE_IDS.claude;
      if (kindForInstanceId(agentInstanceId) !== 'claude') continue;
      out.push({ sessionId: s.id, agentInstanceId, title: s.title });
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
     when there's exactly one linked session, prefix per-session names
     when two+ are linked. Keeps the popover scannable. */
  type ApplyBtn = {
    sessionId: string;
    agentInstanceId: string;
    label: string;
  };
  const applyButtons = $derived.by<ApplyBtn[]>(() => {
    if (linkedAgents.length === 0) return [];
    if (linkedAgents.length === 1) {
      const a = linkedAgents[0];
      return [{ sessionId: a.sessionId, agentInstanceId: a.agentInstanceId, label: 'Claude' }];
    }
    return linkedAgents.map((a) => ({
      sessionId: a.sessionId,
      agentInstanceId: a.agentInstanceId,
      label: `Claude · ${a.title}`
    }));
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

{#snippet termHead()}
  <header class="st-head">
    <span class="st-head-title">{instanceLabel}</span>
    <span class="st-head-meta">zsh{p.cwd ? ` · ${p.cwd.replace(/^\/Users\/[^/]+/, '~')}` : ''}</span>
    <span class="st-head-spring"></span>
    <span class="st-head-name">drivable by agents via MCP</span>
  </header>
{/snippet}

<section
  class="app-shell st-shell"
  class:st-shell--rail={!sideOpen}
  class:st-shell--quiet={quiet}
  style="--app-tone: var(--src-term); --app-glow: rgba(245,240,234,0.30);"
>
  {#if quiet}
    <div class="qsolo-term">
      <div class="qsolo-float">
        <button
          class="qsolo-float-title"
          class:open={termSwitchOpen}
          onclick={() => (termSwitchOpen = !termSwitchOpen)}
          aria-expanded={termSwitchOpen}
          title="Switch terminal"
        >
          {instanceLabel} <span class="qsolo-caret" aria-hidden="true">▾</span>
        </button>
        <span class="qsolo-term-meta mono">zsh{cwdShort ? ` · ${cwdShort}` : ''} · drivable by agents via MCP</span>
        {#if termSwitchOpen}
          <div class="qsolo-float-pop" role="listbox" aria-label="Terminals">
            {#each termInstances as inst (inst.id)}
              <button
                class="qsolo-float-item"
                class:active={inst.id === activeTermId}
                onclick={() => pickTerminal(inst.id)}
                role="option"
                aria-selected={inst.id === activeTermId}
              >
                <span class="qsolo-float-name">{inst.name}</span>
                <span class="qsolo-float-sub mono">zsh</span>
                {#if !inst.primary}
                  <span
                    class="qsolo-float-x"
                    role="button"
                    tabindex="-1"
                    title="Close {inst.name}"
                    aria-label="Close {inst.name}"
                    onclick={(e) => removeTerminal(inst.id, e)}
                    onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); removeTerminal(inst.id, e as unknown as MouseEvent); } }}
                  >
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"><line x1="6" y1="6" x2="18" y2="18"/><line x1="6" y1="18" x2="18" y2="6"/></svg>
                  </span>
                {/if}
              </button>
            {/each}
            <button class="qsolo-float-add" onclick={() => { addInstance('terminal'); termSwitchOpen = false; }}>
              + New terminal
            </button>
          </div>
        {/if}
      </div>
      <section class="app-pane st-main st-main--quiet">
        <TerminalSurface
          instanceId={p.instanceId}
          cwd={p.cwd ?? null}
          onSelectionChange={(s) => (xtermSelection = s)}
          clearSelectionRef={clearSelRef}
        />
        <button class="st-clear" onclick={clearScreen} title="Clear terminal (keeps the shell session running)" aria-label="Clear terminal">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <path d="M3 6h18"/>
            <path d="M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/>
            <path d="M19 6 18 20a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2L5 6"/>
          </svg>
        </button>
        {#if xtermSelection && applyButtons.length > 0}
          <div class="st-apply-pop" style:left="{xtermSelection.anchor.x}px" style:top="{xtermSelection.anchor.y}px" role="toolbar" aria-label="Apply terminal selection to agent">
            {#each applyButtons as btn (btn.sessionId)}
              <button class="st-apply-pop-btn claude" onmousedown={(e) => e.preventDefault()} onclick={() => handleApplyTo(btn)} title={`Pin selection to ${btn.label}'s composer`}>
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M5 12h12M13 6l6 6-6 6"/></svg>
                <span>Apply to {btn.label}</span>
              </button>
            {/each}
          </div>
        {/if}
      </section>
    </div>
  {:else}
  <aside class="lp st-list">
    <header class="lp-head">
      <span class="lp-title">Terminals</span>
      <span class="lp-count">{termInstances.length}</span>
      <span class="lp-head-spring"></span>
      <button class="lp-add" onclick={() => addInstance('terminal')} title="New terminal" aria-label="New terminal">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round"><path d="M12 5v14M5 12h14"/></svg>
      </button>
    </header>
    <div class="lp-list">
      {#each termInstances as inst (inst.id)}
        <button
          class="lp-row st-list-row"
          class:active={inst.id === activeTermId}
          onclick={() => setActiveInstance('terminal', inst.id)}
        >
          <span class="st-list-dot" aria-hidden="true"></span>
          <span class="lp-row-title">{inst.name}</span>
          <span class="lp-row-meta">zsh</span>
          {#if !inst.primary}
            <span
              class="st-list-x"
              role="button"
              tabindex="-1"
              title="Close {inst.name}"
              aria-label="Close {inst.name}"
              onclick={(e) => removeTerminal(inst.id, e)}
              onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); removeTerminal(inst.id, e as unknown as MouseEvent); } }}
            >
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"><line x1="6" y1="6" x2="18" y2="18"/><line x1="6" y1="18" x2="18" y2="6"/></svg>
            </span>
          {/if}
        </button>
      {/each}
    </div>
  </aside>
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
          {@render termHead()}
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
                  class="st-apply-pop-btn claude"
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
      {@render termHead()}
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
              class="st-apply-pop-btn claude"
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
    <SidePaneRail
      linkedAgents={linkedAgents.map((la) => ({
        sessionId: la.sessionId,
        agentInstanceId: la.agentInstanceId,
        kind: 'claude' as const,
        title: la.title
      }))}
      onExpand={() => (sideOpen = true)}
    />
  {/if}
  {/if}
</section>

<style>
  /* Single-cell GRID, not block — every other solo shell is a grid
     and renders reliably; the block variant was the odd one out and
     the only solo whose whole subtree (head + surface + side pane,
     all inside the Splitter) intermittently came up invisible. A grid
     cell gives the Splitter a definite stretch context in both axes. */
  /* Redesign v2 §2.7 — [list 264][PTY(+side)]. Shell padding dropped so
     the list is flush-left; the charcoal PTY gets its own margin-inset. */
  .st-shell {
    display: grid;
    grid-template-columns: 264px minmax(0, 1fr);
    grid-template-rows: minmax(0, 1fr);
    padding: 0;
  }
  /* Rail-collapsed: list + terminal pane + 44px rail. */
  .st-shell.st-shell--rail {
    grid-template-columns: 264px minmax(0, 1fr) 44px;
    transition: grid-template-columns var(--dur-base) var(--ease-out);
  }
  /* Quiet §3.4 — PTY inset almost full-window, no list column. */
  .st-shell.st-shell--quiet { display: block; grid-template-columns: none; }
  .qsolo-term { position: relative; display: flex; flex-direction: column; height: 100%; }
  /* Specificity bump (two classes) so this wins over the base `.st-main`
     margin, which is declared LATER in source — otherwise the quiet PTY
     inset started at 14px and collided with the floating header. */
  .st-main.st-main--quiet { margin: 52px 40px 18px; }
  .qsolo-float { position: absolute; top: 13px; left: 96px; z-index: 40; display: flex; align-items: baseline; gap: 10px; }
  .qsolo-float-title {
    background: transparent; border: 0; cursor: pointer;
    display: inline-flex; align-items: center; gap: 5px;
    font-size: 15px; font-weight: 600; color: var(--text-0);
    letter-spacing: -0.01em;
  }
  .qsolo-float-title .qsolo-caret { font-size: 10px; color: var(--text-faint); }
  .qsolo-float-title:hover, .qsolo-float-title.open { color: var(--accent-bright); }
  .qsolo-term-meta { font-size: 11px; color: var(--text-mute); }
  .qsolo-float-pop {
    position: absolute; top: calc(100% + 6px); left: 0; z-index: 50;
    min-width: 220px; max-height: 340px; overflow-y: auto; padding: 4px;
    background: var(--bg-1); border: 1px solid var(--border-hi);
    border-radius: 12px; box-shadow: var(--shadow-3);
    display: flex; flex-direction: column; gap: 1px;
  }
  .qsolo-float-item {
    display: flex; align-items: center; gap: 8px;
    padding: 7px 10px; border-radius: 8px;
    background: transparent; border: 0; cursor: pointer; text-align: left;
    color: var(--text-1); font-size: 13px;
  }
  .qsolo-float-item:hover { background: var(--bg-hover); color: var(--text-0); }
  .qsolo-float-item.active { background: var(--bg-3); color: var(--text-0); box-shadow: var(--shadow-1); }
  .qsolo-float-name { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .qsolo-float-sub { flex: none; font-size: 10.5px; color: var(--text-mute); }
  .qsolo-float-x {
    flex: none; width: 18px; height: 18px;
    display: grid; place-items: center;
    border-radius: 4px; color: var(--text-mute); cursor: pointer;
    opacity: 0; transition: opacity 120ms, color 120ms, background 120ms;
  }
  .qsolo-float-x svg { width: 11px; height: 11px; }
  .qsolo-float-item:hover .qsolo-float-x { opacity: 0.8; }
  .qsolo-float-x:hover { opacity: 1; color: var(--err); background: var(--bg-3); }
  .qsolo-float-add {
    margin-top: 2px; padding: 7px 10px; border-radius: 8px;
    background: transparent; border: 0; cursor: pointer; text-align: left;
    color: var(--text-2); font-size: 12px;
  }
  .qsolo-float-add:hover { background: var(--bg-hover); color: var(--text-0); }
  /* Instance list column. */
  .st-list { min-height: 0; }
  .st-list-row {
    display: flex; align-items: center; gap: 8px;
  }
  .st-list-dot {
    width: 6px; height: 6px; border-radius: 50%;
    background: var(--text-linenum, var(--text-mute));
    flex: none;
  }
  .st-list-row.active .st-list-dot { background: var(--src-term, var(--ok)); }
  .st-list-row .lp-row-title { flex: 1; min-width: 0; }
  .st-list-row .lp-row-meta { flex: none; }
  .st-list-x {
    flex: none; width: 18px; height: 18px;
    display: grid; place-items: center;
    border-radius: 4px; color: var(--text-mute); cursor: pointer;
    opacity: 0; transition: opacity 120ms, color 120ms, background 120ms;
  }
  .st-list-x svg { width: 11px; height: 11px; }
  .st-list-row:hover .st-list-x { opacity: 0.8; }
  .st-list-x:hover { opacity: 1; color: var(--err); background: var(--bg-3); }
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

  /* Mockup terminal header — charcoal strip over the inset. */
  .st-head {
    flex: none;
    display: flex; align-items: center; gap: 10px;
    height: 44px;
    padding: 0 20px;
    background: var(--dark-0);
    border-bottom: 1px solid rgba(0, 0, 0, 0.3);
    font-size: 11px;
  }
  .st-head-title { font-size: 13.5px; font-weight: 600; color: var(--dark-text); }
  .st-head-meta { color: var(--dark-mute); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .st-head-spring { flex: 1; }
  .st-head-name { color: var(--dark-text-2); font-size: 10.5px; }

  .st-main {
    display: flex; flex-direction: column;
    overflow: hidden;
    /* Charcoal inset — margin gap + rounded corners (§2.7). Height comes
       from the splitter pane's flex, not 100% (margin would overflow). */
    background: var(--dark-0);
    position: relative;
    flex: 1; min-height: 0;
    margin: 14px 20px 18px;
    border-radius: 10px;
    box-shadow: var(--shadow-1);
  }
  .st-main :global(.terminal-surface) {
    background: var(--dark-0) !important;
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
    /* Sits over the terminal's charcoal inset — always dark. */
    background: color-mix(in srgb, var(--dark-1) 78%, transparent);
    border: 1px solid rgba(216, 210, 190, 0.14);
    color: var(--dark-text-2);
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
    color: var(--dark-text);
    border-color: rgba(216, 210, 190, 0.3);
    background: var(--dark-1);
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
    box-shadow: 0 1px 0 0 rgba(0, 0, 0, 0.1), var(--shadow-1);
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
</style>
