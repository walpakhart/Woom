<script lang="ts">
  /* AgentDock — Cursor-style chat dock that lives on the right edge of
     the editor solo. Phase 1: shell only — header with session
     dropdown, empty + not-connected states, and a body placeholder
     where the real ChatThread + Composer land in Phase 2.

     Unlike InlineClaude (a quick-send row LIST shared by Terminal /
     Canvas), AgentDock shows ONE session at a time with a picker —
     the editor↔agent split. EditorApp swaps InlineClaude → AgentDock;
     the other hosts keep InlineClaude untouched. */
  import { sessionsState, focusSession } from '$lib/state/sessions.svelte';
  import { APP_INSTANCE_IDS } from '$lib/state/layout.svelte';
  import BrandIcon from '$lib/components/ui/BrandIcon.svelte';
  import ChatThread from '../agent/ChatThread.svelte';
  import Composer from '../agent/Composer.svelte';
  import { onMount } from 'svelte';
  import type { ClaudeAction } from '$lib/types';

  interface LinkedAgent {
    sessionId: string;
    agentInstanceId: string;
    kind: 'claude';
    name: string;
  }
  interface PickableAgent {
    id: string;
    kind: 'claude';
    name: string;
    sessionId?: string;
  }

  /** Agent callbacks bundled from +page.svelte — the same handlers the
   *  AgentApp solo receives. */
  export interface DockHandlers {
    now: number;
    thinkingStartedAt: Record<string, number | null>;
    thinkingTick: Record<string, number>;
    onSend: () => void;
    onStop: () => void;
    onPasteImages: (
      instanceId: string,
      blobs: { name: string; type: string; blob: Blob }[]
    ) => Promise<number>;
    onDragOver: (instanceId: string, e: DragEvent) => void;
    onDrop: (instanceId: string, e: DragEvent) => void;
    onDragLeave: (instanceId: string) => void;
    onStartEditMessage: (sessionId: string, index: number, content: string) => void;
    onResendMessage: (sessionId: string, index: number, content: string) => void;
    onUpdateAction: (sessionId: string, actionId: string, patch: Partial<ClaudeAction>) => void;
    onRemoveAction: (sessionId: string, actionId: string) => void;
    onExecuteAction: (sessionId: string, action: ClaudeAction) => void;
    onOpenPrInWoom: (url: string, action: (ClaudeAction & { kind: 'pr' }) | null) => void;
    onOpenFile?: (path: string) => void;
    onDwVerify?: (workflowId: string) => void;
    onResumeAfterQuota?: (sessionId: string) => void;
  }

  interface Props {
    instanceId: string;
    /** Sessions linked to THIS editor instance (derived in EditorApp). */
    linkedAgents: LinkedAgent[];
    /** Pickable rows for the empty state — every Claude session
     *  not already linked here (+ a spawn row when there are no chats). */
    agentInstances: PickableAgent[];
    connectedClaude: boolean;
    onClose: () => void;
    /** Activate the session AND jump to its agent solo. Used by the
     *  open-in-solo button and the not-connected CTA. */
    onOpenSession: (sessionId: string, agentInstanceId: string) => void;
    /** Link a session (or spawn-and-link) to this editor. Empty-state
     *  rows call this directly for one-click link-and-dock. */
    onLinkToAgent: (agentInstanceId: string, sessionId?: string) => void;
    /** Agent callback bundle — drives the embedded ChatThread + Composer. */
    dock: DockHandlers;
  }
  let p: Props = $props();

  /** User's explicit pick. Null = follow the most-recent fallback. */
  let selectedSessionId = $state<string | null>(null);

  /* Per-editor-instance persisted dock session. Restored on mount,
     validated against the live link set so a deleted/unlinked id can't
     resurrect. Mirrors the open-state persistence pattern. */
  // svelte-ignore state_referenced_locally
  const dockSessionKey = `editor-dock-session:${p.instanceId}`;
  onMount(() => {
    const stored = localStorage.getItem(dockSessionKey);
    if (stored && p.linkedAgents.some((l) => l.sessionId === stored)) {
      selectedSessionId = stored;
    }
  });

  /** Last-activity timestamp for a session id (most-recent message). */
  function lastActivity(sessionId: string): string {
    const s = sessionsState.list.find((x) => x.id === sessionId);
    return s?.messages[s.messages.length - 1]?.at ?? '';
  }

  /** The session currently shown in the dock. Resolution order:
   *  explicit pick (if still linked) → most-recently-active linked →
   *  null (empty state). */
  const dockSession = $derived.by<LinkedAgent | null>(() => {
    void sessionsState.list;
    const links = p.linkedAgents;
    if (links.length === 0) return null;
    if (selectedSessionId) {
      const hit = links.find((l) => l.sessionId === selectedSessionId);
      if (hit) return hit;
    }
    return [...links].sort(
      (a, b) => lastActivity(b.sessionId).localeCompare(lastActivity(a.sessionId))
    )[0];
  });

  /* Drop a stale explicit pick when its session leaves the link set
     (unlinked / deleted). Nulling it lets `dockSession` fall through to
     most-recent / empty, and the persist effect below wipes the stored
     id so it can't resurrect on next launch. */
  $effect(() => {
    if (selectedSessionId && !p.linkedAgents.some((l) => l.sessionId === selectedSessionId)) {
      selectedSessionId = null;
    }
  });

  /* Persist the EFFECTIVE shown session so relaunch reopens on it even
     when the user never explicitly picked (most-recent default). */
  $effect(() => {
    const id = dockSession?.sessionId;
    if (id) localStorage.setItem(dockSessionKey, id);
    else localStorage.removeItem(dockSessionKey);
  });

  /** Align the global active pointer with the docked session. The
   *  stock ChatThread / Composer + the send pipeline all resolve their
   *  target via `activeIds[kind]`, so docking a session = focusing it.
   *  Intentionally changes which chat the agent solo shows next visit —
   *  same one-active-conversation semantics as the sidebar / Open
   *  affordances. Guarded so we don't thrash when already aligned. */
  $effect(() => {
    const ds = dockSession;
    if (!ds) return;
    if (sessionsState.activeIds.claude !== ds.sessionId) {
      focusSession(ds.sessionId);
    }
  });

  /** Is the Claude CLI connected? Drives the not-connected stub vs.
   *  the chat body. */
  const dockConnected = $derived(dockSession ? p.connectedClaude : false);

  /** Live sending flag for the docked session — header pulse dot. */
  function isSending(sessionId: string): boolean {
    return sessionsState.list.find((s) => s.id === sessionId)?.sending ?? false;
  }

  /** Queued-turn count for a session — dropdown status chip. */
  function queueLen(sessionId: string): number {
    return sessionsState.list.find((s) => s.id === sessionId)?.pendingQueue?.length ?? 0;
  }

  let showPicker = $state(false);
  function pickSession(sessionId: string) {
    selectedSessionId = sessionId;
    showPicker = false;
  }
  function onWindowKey(e: KeyboardEvent) {
    if (e.key === 'Escape' && showPicker) showPicker = false;
  }
</script>

<svelte:window onkeydown={onWindowKey} />
<aside class="edock" data-agent={dockSession?.kind ?? 'claude'} aria-label="Agent dock">
  <header class="edock-head">
    {#if dockSession}
      <span
        class="edock-dot"
        class:running={isSending(dockSession.sessionId)}
        data-agent={dockSession.kind}
        aria-label={isSending(dockSession.sessionId) ? 'running' : 'connected'}
      ></span>
      <button
        class="edock-title-btn"
        class:pick={p.linkedAgents.length >= 2}
        disabled={p.linkedAgents.length < 2}
        onclick={() => { if (p.linkedAgents.length >= 2) showPicker = !showPicker; }}
        title="Active conversation — also shown in the agent solo"
        aria-expanded={showPicker}
      >
        <span class="edock-title">{dockSession.name || 'Untitled chat'}</span>
        {#if p.linkedAgents.length >= 2}
          <span class="edock-caret" class:open={showPicker} aria-hidden="true">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M6 9l6 6 6-6"/></svg>
          </span>
        {/if}
      </button>
      <button
        class="edock-open"
        onclick={() => p.onOpenSession(dockSession!.sessionId, dockSession!.agentInstanceId)}
        title="Open this chat in the Claude app"
        aria-label="Open in agent solo"
      >
        <span class="edock-open-arrow" aria-hidden="true">→</span>
        <span class="edock-open-label">claude</span>
        <svg class="edock-open-ext" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M7 17 17 7"/><path d="M9 7h8v8"/></svg>
      </button>
    {:else}
      <span class="edock-dot edock-dot--idle" aria-hidden="true"></span>
      <span class="edock-title edock-title--static">Agent dock</span>
    {/if}
    <button class="edock-collapse" title="Collapse dock · ⌘L" aria-label="Collapse dock" onclick={p.onClose}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M10 6l6 6-6 6"/></svg>
    </button>
  </header>

  <div class="edock-body">
    {#if showPicker && p.linkedAgents.length >= 2}
      <div class="edock-picker">
        <div class="edock-picker-head">
          <span>Switch conversation</span>
          <button class="edock-picker-close" onclick={() => (showPicker = false)} aria-label="Close picker">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7"><path d="M18 6 6 18M6 6l12 12"/></svg>
          </button>
        </div>
        {#each p.linkedAgents as la (la.sessionId)}
          <button class="edock-picker-item" class:active={la.sessionId === dockSession?.sessionId} onclick={() => pickSession(la.sessionId)}>
            <span class="edock-picker-kind" data-agent="claude">Claude</span>
            <span class="edock-picker-name">{la.name || 'Untitled chat'}</span>
            {#if isSending(la.sessionId)}
              <span class="edock-rowdot" data-agent={la.kind} aria-label="running"></span>
            {:else if queueLen(la.sessionId) > 0}
              <span class="edock-q mono" aria-label="queued">{queueLen(la.sessionId)}</span>
            {/if}
          </button>
        {/each}
      </div>
    {/if}

    {#if !dockSession}
      <!-- Empty state: no sessions linked to this editor. -->
      <div class="edock-empty">
        <div class="edock-empty-icon">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round" stroke-linecap="round">
            <path d="M12 2 L21 7 L21 17 L12 22 L3 17 L3 7 Z"/>
            <path d="M12 12 L12 22"/><path d="M12 12 L3 7"/><path d="M12 12 L21 7"/>
          </svg>
        </div>
        <p class="edock-empty-h serif">No agent linked</p>
        <p class="edock-empty-p">Link a Claude chat to this editor — it docks here side by side with your code.</p>
        {#if p.agentInstances.length > 0}
          <div class="edock-link-list">
            {#each p.agentInstances as a (a.sessionId ?? a.id)}
              <button class="edock-link-item" onclick={() => p.onLinkToAgent(a.id, a.sessionId)}>
                <span class="edock-picker-kind" data-agent="claude">Claude</span>
                <span class="edock-picker-name">{a.name}</span>
              </button>
            {/each}
          </div>
        {/if}
      </div>
    {:else if !dockConnected}
      <!-- Selected session's agent CLI not connected. -->
      <div class="edock-empty">
        <div class="edock-empty-icon" data-agent={dockSession.kind}>
          <BrandIcon kind={dockSession.kind} size={26} />
        </div>
        <p class="edock-empty-h serif">Connect Claude first</p>
        <p class="edock-empty-p">The Claude CLI isn't connected. Open the agent solo to finish setup.</p>
        <button class="edock-cta" onclick={() => p.onOpenSession(dockSession!.sessionId, dockSession!.agentInstanceId)}>
          Open Claude
        </button>
      </div>
    {:else}
      <!-- Real chat. {#key} forces a clean remount on session switch so
           the windowed lazy-mount (IntersectionObserver) can't leak DOM
           across sessions — stricter than the solo, which relies on
           ChatThread's internal visibleCount reset. -->
      {#key dockSession.sessionId}
        <div class="edock-chat">
          <ChatThread
            kind={dockSession.kind}
            compact
            thinkingStartedAt={p.dock.thinkingStartedAt}
            thinkingTick={p.dock.thinkingTick}
            onUpdateAction={p.dock.onUpdateAction}
            onRemoveAction={p.dock.onRemoveAction}
            onExecuteAction={p.dock.onExecuteAction}
            onOpenPrInWoom={p.dock.onOpenPrInWoom}
            onStartEditMessage={p.dock.onStartEditMessage}
            onResendMessage={p.dock.onResendMessage}
            onOpenFile={p.dock.onOpenFile}
            onDwVerify={p.dock.onDwVerify}
            onResumeAfterQuota={p.dock.onResumeAfterQuota}
          />
          <Composer
            kind={dockSession.kind}
            compact
            onSend={() => p.dock.onSend()}
            onStop={() => p.dock.onStop()}
            onPasteImages={(blobs) => p.dock.onPasteImages(APP_INSTANCE_IDS.claude, blobs)}
            onDragOver={(e) => p.dock.onDragOver(APP_INSTANCE_IDS.claude, e)}
            onDrop={(e) => p.dock.onDrop(APP_INSTANCE_IDS.claude, e)}
            onDragLeave={() => p.dock.onDragLeave(APP_INSTANCE_IDS.claude)}
          />
        </div>
      {/key}
    {/if}
  </div>
</aside>

<style>
  /* AgentDock — mockup 4i chrome. Fresh .edock- markup: a status
     dot + title + «→ claude ↗» open-in-solo header, a scroll body that
     hosts the compact ChatThread (mini transcript) + Composer (mini
     send-circle), plus the picker / empty / not-connected states. */
  .edock {
    display: grid; grid-template-rows: 40px 1fr;
    background: var(--bg-1);
    border-left: 1px solid var(--border);
    min-height: 0;
    width: 100%; height: 100%;
  }
  .edock-head {
    display: flex; align-items: center; gap: 7px;
    padding: 0 6px 0 13px;
    border-bottom: 1px solid var(--border);
    background: var(--bg-1);
  }

  /* Running dot — solid ok when idle-connected, pulses in the agent
     hue while sending. Replaces the old brand-icon box in the header. */
  .edock-dot {
    width: 7px; height: 7px; border-radius: 50%; flex-shrink: 0;
    background: var(--ok);
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--ok) 16%, transparent);
  }
  .edock-dot--idle { background: var(--text-mute); box-shadow: none; }
  .edock-dot.running[data-agent='claude'] {
    background: var(--src-claude);
    box-shadow: var(--shadow-1);
    animation: edock-pulse 1.2s ease-in-out infinite;
  }
  @keyframes edock-pulse {
    0%, 100% { opacity: 0.5; transform: scale(0.82); }
    50%      { opacity: 1; transform: scale(1.15); }
  }

  .edock-title-btn {
    flex: 1; min-width: 0;
    display: flex; align-items: center; gap: 5px;
    padding: 3px 5px;
    background: transparent; border: 0; border-radius: 6px;
    cursor: default; text-align: left;
    color: var(--text-0);
    transition: background 120ms var(--ease-out);
  }
  .edock-title-btn.pick { cursor: pointer; }
  .edock-title-btn.pick:hover { background: var(--bg-2); }
  .edock-title {
    font-size: 13px; font-weight: 600; letter-spacing: -0.01em;
    color: var(--text-0);
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .edock-title--static { flex: 1; min-width: 0; padding: 0 3px; }
  .edock-caret { color: var(--text-mute); display: grid; place-items: center; flex-shrink: 0; transition: transform 160ms var(--ease-out); }
  .edock-caret svg { width: 12px; height: 12px; }
  .edock-caret.open { transform: rotate(180deg); }
  .edock[data-agent='claude'] .edock-caret.open { color: var(--src-claude); }

  /* «→ claude ↗» — open-in-solo text link (faint mono, agent hue on
     hover). Mirrors the mockup's right-aligned header affordance. */
  .edock-open {
    display: inline-flex; align-items: center; gap: 4px;
    padding: 3px 6px; flex-shrink: 0;
    background: transparent; border: 0; border-radius: 6px;
    cursor: pointer;
    font-family: var(--font-mono); font-size: 11px; letter-spacing: -0.01em;
    color: var(--text-mute);
    transition: color 140ms var(--ease-out), background 140ms var(--ease-out);
  }
  .edock-open:hover { color: var(--text-1); background: var(--bg-2); }
  .edock-open-arrow { color: var(--text-2); }
  .edock[data-agent='claude'] .edock-open:hover .edock-open-label { color: var(--src-claude); }
  .edock-open-ext { width: 11px; height: 11px; }

  .edock-collapse {
    width: 24px; height: 24px;
    display: grid; place-items: center;
    color: var(--text-2);
    background: transparent; border: none; cursor: pointer;
    border-radius: 5px; flex-shrink: 0;
    transition: color 140ms var(--ease-out), background 140ms var(--ease-out);
  }
  .edock-collapse:hover { color: var(--text-0); background: var(--bg-2); }
  .edock-collapse svg { width: 13px; height: 13px; }

  .edock-body {
    overflow-y: auto;
    display: flex; flex-direction: column;
    min-height: 0;
  }

  /* Inline session picker — rendered at body top (never clipped). */
  .edock-picker {
    margin: 10px 10px 0;
    border-radius: 9px;
    border: 1px solid var(--border-hi);
    background: var(--bg-2);
    overflow: hidden;
    flex-shrink: 0;
  }
  .edock-picker-head {
    display: flex; align-items: center; justify-content: space-between;
    padding: 6px 8px 6px 10px;
    border-bottom: 1px solid var(--border);
    font-size: 9.5px; font-weight: 700;
    text-transform: uppercase; letter-spacing: 0.08em;
    color: var(--text-mute);
  }
  .edock-picker-close {
    width: 20px; height: 20px;
    display: grid; place-items: center;
    background: transparent; border: none; color: var(--text-mute);
    cursor: pointer; border-radius: 4px; flex-shrink: 0;
    transition: color 120ms var(--ease-out), background 120ms var(--ease-out);
  }
  .edock-picker-close:hover { color: var(--text-0); background: var(--bg-3); }
  .edock-picker-close svg { width: 11px; height: 11px; }
  .edock-picker-item, .edock-link-item {
    display: flex; align-items: center; gap: 8px;
    width: 100%; padding: 7px 8px;
    background: transparent; border: 0; text-align: left;
    color: var(--text-0); font-size: 12px; cursor: pointer;
  }
  .edock-picker-item:hover, .edock-link-item:hover { background: var(--bg-3); }
  .edock-picker-item.active { background: color-mix(in srgb, var(--accent) 8%, var(--bg-3)); }
  .edock-picker-kind {
    display: inline-flex; padding: 1px 6px; border-radius: 4px;
    font-size: 9.5px; font-weight: 700; letter-spacing: 0.04em;
    text-transform: uppercase; flex-shrink: 0;
  }
  .edock-picker-kind[data-agent='claude'] {
    background: color-mix(in srgb, var(--src-claude) 12%, var(--bg-3));
    color: var(--src-claude);
    border: 1px solid color-mix(in srgb, var(--src-claude) 28%, transparent);
  }
  .edock-picker-name { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  /* Running dot in dropdown rows. */
  .edock-rowdot {
    width: 6px; height: 6px; border-radius: 50%; flex-shrink: 0;
    animation: edock-pulse 1.2s ease-in-out infinite;
  }
  .edock-rowdot[data-agent='claude'] { background: var(--src-claude); box-shadow: var(--shadow-1); }
  /* Queued-count chip in dropdown rows — neutral accent fill. */
  .edock-q {
    flex-shrink: 0;
    min-width: 16px; padding: 0 5px; height: 15px;
    display: grid; place-items: center;
    border-radius: 999px;
    font-size: 9.5px; font-weight: 700;
    color: var(--accent-bright);
    background: color-mix(in srgb, var(--accent) 14%, transparent);
    border: 1px solid color-mix(in srgb, var(--accent) 30%, transparent);
  }

  /* Empty / not-connected states. */
  .edock-empty { text-align: center; margin: auto 0; padding: 30px 18px; }
  .edock-empty-icon {
    width: 56px; height: 56px; margin: 0 auto 18px;
    display: grid; place-items: center; border-radius: 14px;
    background: color-mix(in srgb, var(--accent) 10%, var(--bg-2));
    color: var(--accent-bright);
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--accent) 24%, transparent), var(--shadow-3);
  }
  .edock-empty-icon[data-agent='claude'] {
    background: color-mix(in srgb, var(--src-claude) 12%, var(--bg-2));
    color: var(--src-claude);
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--src-claude) 26%, transparent);
  }
  .edock-empty-icon svg { width: 26px; height: 26px; }
  .edock-empty-h {
    font-family: var(--font-mono);
    font-size: 20px; font-weight: 600; letter-spacing: -0.015em;
    color: var(--text-0); margin: 0 0 10px;
  }
  .edock-empty-p { font-size: 12.5px; color: var(--text-2); line-height: 1.55; margin: 0 0 16px; }
  .edock-link-list {
    display: flex; flex-direction: column; gap: 2px;
    border: 1px solid var(--border); border-radius: 9px;
    overflow: hidden; text-align: left;
  }
  .edock-cta {
    display: inline-flex; align-items: center; gap: 6px;
    padding: 7px 14px; border-radius: 8px;
    font-size: 12px; font-weight: 600;
    background: linear-gradient(180deg, var(--accent-bright), var(--accent));
    color: var(--accent-fg); border: none; cursor: pointer;
    box-shadow: inset 0 1px 0 rgba(255,255,255,0.20), var(--shadow-2);
    transition: transform 140ms var(--ease-out);
  }
  .edock-cta:hover { transform: translateY(-1px); }

  /* Chat body — compact ChatThread (flex:1 scroll, the mini transcript:
     prose 12.5 + trace rows) + compact Composer (auto, the send-circle
     footer). Fills the dock column so the composer pins to the bottom. */
  .edock-chat {
    flex: 1; min-height: 0; min-width: 0;
    display: flex; flex-direction: column;
    overflow: hidden;
    position: relative;
  }
  .edock-chat > :global(*) { min-width: 0; }
  /* Narrow-pane guard: at 300px a wide code block would otherwise blow
     the dock width. Keep code scrolling INSIDE the bubble. */
  .edock-chat :global(pre) { overflow-x: auto; max-width: 100%; }
</style>
