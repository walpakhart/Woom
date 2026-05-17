<script lang="ts">
  /* WelcomeOverlay — the "what is this app and how do I use it"
     introduction. Distinct from `Cheatsheet` (which is a dense
     keyboard reference) — this one is the friendly orientation
     surface a new user (or a returning user who forgot how things
     fit together) lands on. Sections explain the solo model, the
     editor↔agent link, what agents can do, the source inboxes, and
     the editor super-powers we shipped most recently.

     Open via `⇧⌘?` (the global help shortcut), via the cheatsheet
     footer link, or via the home page banner. We deliberately keep
     `?` for the keyboard cheatsheet so users coming from "show me
     the keys" don't have to learn a new gesture — Welcome is the
     larger story, Cheatsheet is the lookup table.

     Each section card has either a "Try it" button that navigates
     you straight to the relevant solo, or a kbd hint, or both. The
     point is: don't just describe the feature, give the user a
     one-click way into it. */

  import { focusTrap } from '$lib/actions/focusTrap';
  import BrandIcon from '$lib/components/ui/BrandIcon.svelte';
  import Sigil from '$lib/components/ui/Sigil.svelte';
  import type { View } from '$lib/state/view.svelte';

  interface Props {
    open: boolean;
    onClose: () => void;
    setView: (v: View) => void;
    /** Surfaces the "Show keyboard cheatsheet" button at the bottom —
     *  caller flips its own `?` cheatsheet on. We don't toggle it
     *  ourselves because the cheatsheet may sit at a different layer
     *  in the modal stack and the parent already handles the
     *  exclusion logic. */
    onOpenCheatsheet: () => void;
  }
  let { open, onClose, setView, onOpenCheatsheet }: Props = $props();

  const isMac =
    typeof navigator !== 'undefined' && /Mac/i.test(navigator.platform);
  const mod = isMac ? '⌘' : 'Ctrl';
  const shift = isMac ? '⇧' : 'Shift';

  /* Active section drives the left rail's highlight + the body
     scroll target. We use a hash-style nav (no actual URL change)
     because the modal is a transient surface — bookmarking a
     section doesn't make sense. */
  type SectionId =
    | 'overview'
    | 'solos'
    | 'agents'
    | 'editor'
    | 'sources'
    | 'shortcuts';
  let active = $state<SectionId>('overview');

  const sections: Array<{ id: SectionId; label: string; glyph: string }> = [
    { id: 'overview',  label: 'Welcome',          glyph: '✦' },
    { id: 'solos',     label: 'Solos & instances', glyph: '◫' },
    { id: 'agents',    label: 'Agents',           glyph: '✶' },
    { id: 'editor',    label: 'Editor',           glyph: '⌘' },
    { id: 'sources',   label: 'Sources',          glyph: '⌬' },
    { id: 'shortcuts', label: 'Shortcuts',        glyph: '⌨' }
  ];

  let scrollEl: HTMLDivElement | null = $state(null);

  function scrollTo(id: SectionId) {
    active = id;
    if (!scrollEl) return;
    const target = scrollEl.querySelector(`[data-section="${id}"]`) as HTMLElement | null;
    if (target) target.scrollIntoView({ behavior: 'smooth', block: 'start' });
  }

  function go(view: View) {
    setView(view);
    onClose();
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.preventDefault();
      onClose();
    }
  }
  function onBackdropClick(e: MouseEvent) {
    if (e.target === e.currentTarget) onClose();
  }

  /* Track which section the user has scrolled into so the left rail
     auto-highlights without a click. Cheap IntersectionObserver —
     mounted once when the modal opens, torn down when it closes. */
  let observer: IntersectionObserver | null = null;
  $effect(() => {
    if (!open || !scrollEl) {
      observer?.disconnect();
      observer = null;
      return;
    }
    /* rootMargin keeps the highlight on the section that's actually
       at reading height, not the one that just barely tipped past
       the top edge. */
    observer = new IntersectionObserver(
      (entries) => {
        for (const e of entries) {
          if (e.isIntersecting) {
            const id = (e.target as HTMLElement).dataset.section as SectionId;
            if (id) active = id;
          }
        }
      },
      { root: scrollEl, rootMargin: '-30% 0px -55% 0px', threshold: 0 }
    );
    for (const sec of sections) {
      const el = scrollEl.querySelector(`[data-section="${sec.id}"]`);
      if (el) observer.observe(el);
    }
    return () => observer?.disconnect();
  });
</script>

{#if open}
  <div
    class="wo-backdrop"
    role="dialog"
    aria-modal="true"
    aria-labelledby="wo-title"
    onclick={onBackdropClick}
    onkeydown={onKeydown}
    tabindex="-1"
    use:focusTrap
  >
    <section class="wo-panel" aria-label="Welcome to Woom">
      <!-- Aurora wash mirrors HomeApp so the Welcome modal feels
           like an extension of the dashboard, not a fresh surface
           with its own design language. -->
      <div class="wo-aurora" aria-hidden="true"></div>

      <header class="wo-head">
        <div class="wo-brand">
          <Sigil size={36} />
          <div class="wo-brand-text">
            <h2 id="wo-title" class="wo-title">Welcome to Woom</h2>
            <p class="wo-tagline">An IDE that brings your agents, sources, and code into one keyboard-driven surface.</p>
          </div>
        </div>
        <button class="wo-close" onclick={onClose} aria-label="Close welcome">×</button>
      </header>

      <div class="wo-body">
        <!-- LEFT RAIL: section nav + glyphs -->
        <nav class="wo-nav" aria-label="Sections">
          {#each sections as sec (sec.id)}
            <button
              class="wo-nav-row"
              class:wo-nav-row--active={active === sec.id}
              onclick={() => scrollTo(sec.id)}
              type="button"
            >
              <span class="wo-nav-glyph" aria-hidden="true">{sec.glyph}</span>
              <span class="wo-nav-label">{sec.label}</span>
            </button>
          {/each}

          <div class="wo-nav-spacer"></div>
          <button class="wo-nav-cheatsheet" onclick={onOpenCheatsheet} type="button">
            <span class="kbd">?</span>
            <span>All shortcuts</span>
          </button>
        </nav>

        <!-- SCROLLING CONTENT -->
        <div class="wo-scroll" bind:this={scrollEl}>
          <!-- ╭─ OVERVIEW ─╮ -->
          <section class="wo-sec" data-section="overview">
            <h3 class="wo-sec-h">What is Woom</h3>
            <p class="wo-sec-lead">
              One desktop app where you talk to AI agents (Claude, Cursor),
              read your code, run terminals, draw on a canvas, and triage
              Jira / GitHub / Sentry — without flipping between five tools.
              Everything is keyboard-first; nothing is hidden behind menus.
            </p>

            <div class="wo-3col">
              <div class="wo-tile" data-tone="agent">
                <div class="wo-tile-glyph">⏵</div>
                <h4 class="wo-tile-h">Agents that act</h4>
                <p class="wo-tile-p">
                  Claude and Cursor edit files, run terminals, search Jira
                  and GitHub, and stream their reasoning live. Every action
                  is visible; risky ones queue an Approval card.
                </p>
              </div>
              <div class="wo-tile" data-tone="editor">
                <div class="wo-tile-glyph">&#123; &#125;</div>
                <h4 class="wo-tile-h">A real editor</h4>
                <p class="wo-tile-p">
                  CodeMirror under the hood, with multi-agent diff review,
                  inline "compose here" prompts, fuzzy quick-open, and a
                  symbol outline — designed for paste-from-Cursor users.
                </p>
              </div>
              <div class="wo-tile" data-tone="source">
                <div class="wo-tile-glyph">⌬</div>
                <h4 class="wo-tile-h">Sources, unified</h4>
                <p class="wo-tile-p">
                  Live inboxes for Jira tickets, GitHub PRs &amp; issues,
                  Sentry errors. Agents can read and act on them through
                  the same MCP toolbox you use to navigate.
                </p>
              </div>
            </div>
          </section>

          <!-- ╭─ SOLOS ─╮ -->
          <section class="wo-sec" data-section="solos">
            <h3 class="wo-sec-h">Solos &amp; instances</h3>
            <p class="wo-sec-lead">
              Woom is organised as <em>solos</em> — full-screen surfaces,
              one per kind. The narrow rail on the left is your switcher;
              the keys <span class="kbd">{mod} 0</span>…<span class="kbd">{mod} 8</span>
              jump you between them. Most solos are singletons, but
              <strong>Editor</strong>, <strong>Canvas</strong>, and
              <strong>Terminal</strong> support many named instances side by side.
            </p>

            <div class="wo-link-grid">
              <button class="wo-link" data-tone="agent" onclick={() => go('claudeApp')}>
                <BrandIcon kind="claude" size={18} />
                <span class="wo-link-label">Claude</span>
                <span class="wo-link-key mono">{mod} 4</span>
              </button>
              <button class="wo-link" data-tone="agent" onclick={() => go('cursorApp')}>
                <BrandIcon kind="cursor" size={18} />
                <span class="wo-link-label">Cursor</span>
                <span class="wo-link-key mono">{mod} 5</span>
              </button>
              <button class="wo-link" data-tone="editor" onclick={() => go('editorApp')}>
                <span class="wo-link-svg">
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="18" height="18" rx="2"/><line x1="3" y1="9" x2="21" y2="9"/><line x1="9" y1="9" x2="9" y2="21"/></svg>
                </span>
                <span class="wo-link-label">Editor</span>
                <span class="wo-link-key mono">{mod} 6</span>
              </button>
              <button class="wo-link" data-tone="canvas" onclick={() => go('canvasApp')}>
                <span class="wo-link-svg">
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="18" height="14" rx="2"/><rect x="6" y="6" width="9" height="6" rx="1"/></svg>
                </span>
                <span class="wo-link-label">Canvas</span>
                <span class="wo-link-key mono">{mod} 7</span>
              </button>
              <button class="wo-link" data-tone="terminal" onclick={() => go('terminalApp')}>
                <span class="wo-link-svg">
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><polyline points="4 17 10 11 4 5"/><line x1="12" y1="19" x2="20" y2="19"/></svg>
                </span>
                <span class="wo-link-label">Terminal</span>
                <span class="wo-link-key mono">{mod} 8</span>
              </button>
              <button class="wo-link" data-tone="home" onclick={() => go('home')}>
                <span class="wo-link-svg">
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M3 12 12 3l9 9"/><path d="M5 10v10h14V10"/></svg>
                </span>
                <span class="wo-link-label">Home</span>
                <span class="wo-link-key mono">{mod} 0</span>
              </button>
            </div>

            <div class="wo-callout">
              <div class="wo-callout-h">The editor↔agent link</div>
              <p class="wo-callout-p">
                Drop a folder on the Editor and the linked agent's
                working directory follows. The agent's edit cards appear
                in the chat <em>and</em> in the editor's Review pane —
                same data, two views. Switching the editor's repo also
                switches the agent's cwd, atomically.
              </p>
            </div>
          </section>

          <!-- ╭─ AGENTS ─╮ -->
          <section class="wo-sec" data-section="agents">
            <h3 class="wo-sec-h">What agents can do</h3>
            <p class="wo-sec-lead">
              Agents speak to a shared <strong>MCP toolbox</strong> —
              the same one you can call from the palette. They can read,
              write, navigate, and search; nothing is opaque.
            </p>

            <div class="wo-cap-grid">
              <div class="wo-cap" data-tone="editor">
                <div class="wo-cap-h">
                  <span class="wo-cap-icon" aria-hidden="true">
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><polyline points="14 2 14 8 20 8"/><line x1="9" y1="13" x2="15" y2="13"/><line x1="9" y1="17" x2="13" y2="17"/></svg>
                  </span>
                  <span>Files</span>
                </div>
                <p class="wo-cap-p">
                  Read, write, create, delete. Every change shows up
                  as an <em>edit card</em> with Keep / Revert. One-click
                  rollback per hunk or per file.
                </p>
              </div>
              <div class="wo-cap" data-tone="terminal">
                <div class="wo-cap-h">
                  <span class="wo-cap-icon" aria-hidden="true">
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="18" height="16" rx="2"/><polyline points="7 10 10 13 7 16"/><line x1="13" y1="16" x2="17" y2="16"/></svg>
                  </span>
                  <span>Terminal</span>
                </div>
                <p class="wo-cap-p">
                  Spawn shells, run commands, drive interactive prompts.
                  Output streams into a terminal instance you can see —
                  no hidden subprocesses.
                </p>
              </div>
              <div class="wo-cap" data-tone="source">
                <div class="wo-cap-h">
                  <span class="wo-cap-icon" aria-hidden="true">
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><circle cx="11" cy="11" r="7"/><line x1="20" y1="20" x2="16.65" y2="16.65"/></svg>
                  </span>
                  <span>Sources</span>
                </div>
                <p class="wo-cap-p">
                  Search and act on Jira, GitHub, Sentry, plus a
                  long-term memory store. Agents can open a PR, log a
                  comment, transition a ticket, claim a Sentry issue.
                </p>
              </div>
              <div class="wo-cap" data-tone="canvas">
                <div class="wo-cap-h">
                  <span class="wo-cap-icon" aria-hidden="true">
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="18" height="14" rx="2"/><rect x="6" y="6" width="9" height="6" rx="1"/><line x1="8" y1="20" x2="16" y2="20"/><line x1="12" y1="17" x2="12" y2="20"/></svg>
                  </span>
                  <span>Canvas</span>
                </div>
                <p class="wo-cap-p">
                  Draw boxes, arrows, mermaid diagrams. Auto-layout via
                  dagre/grid. Useful when an agent needs to <em>show</em>
                  a flow instead of describe it.
                </p>
              </div>
              <div class="wo-cap" data-tone="approval">
                <div class="wo-cap-h">
                  <span class="wo-cap-icon" aria-hidden="true">
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M12 2 4 5v6c0 5 3.5 9 8 11 4.5-2 8-6 8-11V5z"/><polyline points="9 12 11 14 15 10"/></svg>
                  </span>
                  <span>Approval cards</span>
                </div>
                <p class="wo-cap-p">
                  Anything destructive (commit, push, rm, migrations)
                  pauses on a card you approve or edit. Read-only stuff
                  runs straight through.
                </p>
              </div>
              <div class="wo-cap" data-tone="memory">
                <div class="wo-cap-h">
                  <span class="wo-cap-icon" aria-hidden="true">
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M19 21l-7-5-7 5V5a2 2 0 0 1 2-2h10a2 2 0 0 1 2 2z"/></svg>
                  </span>
                  <span>Memory</span>
                </div>
                <p class="wo-cap-p">
                  Persists past every chat. Agent recalls automatically
                  at session start; deleting a chat auto-distills a
                  snapshot. The 💾 chip in the chat header and the per-
                  row badge in the sidebar show what's saved. Browse +
                  edit + delete in Settings → Memory.
                </p>
              </div>
              <div class="wo-cap" data-tone="recovery">
                <div class="wo-cap-h">
                  <span class="wo-cap-icon" aria-hidden="true">
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M21 12a9 9 0 1 1-3-6.7"/><path d="M21 4v5h-5"/></svg>
                  </span>
                  <span>Crash recovery</span>
                </div>
                <p class="wo-cap-p">
                  Force-quit mid-turn or lose the CLI process — the
                  next send auto-injects a recap of the prior transcript
                  and rotates the CLI uuid so the agent picks up
                  exactly where it left off. An amber banner above the
                  chat warns you it happened.
                </p>
              </div>
              <div class="wo-cap" data-tone="canvas">
                <div class="wo-cap-h">
                  <span class="wo-cap-icon" aria-hidden="true">
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="5" width="18" height="14" rx="2"/><polygon points="10,9 16,12 10,15" fill="currentColor" stroke="none"/></svg>
                  </span>
                  <span>Preview pane</span>
                </div>
                <p class="wo-cap-p">
                  Spawn dev servers, watchers, test loops without leaving
                  the chat. Right-side collapsible rail on the Claude /
                  Cursor solo. <span class="mono">/preview pnpm dev</span>
                  starts a tracked task; agents can react to its output
                  via the <span class="mono">bg_wait_line</span> tool.
                  Detected <span class="mono">http://localhost:PORT</span>
                  auto-opens an embedded webview.
                </p>
              </div>
              <div class="wo-cap" data-tone="approval">
                <div class="wo-cap-h">
                  <span class="wo-cap-icon" aria-hidden="true">
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M9 11l3 3L22 4"/><path d="M21 12v7a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11"/></svg>
                  </span>
                  <span>Plan mode</span>
                </div>
                <p class="wo-cap-p">
                  <span class="mono">⇧⇥</span> in the composer flips the
                  agent into read-only mode — no edits or mutating bash
                  until you flip back. Useful before a big refactor: ask
                  the agent to plan, review the plan, then approve to
                  execute.
                </p>
              </div>
              <div class="wo-cap" data-tone="memory">
                <div class="wo-cap-h">
                  <span class="wo-cap-icon" aria-hidden="true">
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><polyline points="14 2 14 8 20 8"/></svg>
                  </span>
                  <span>CLAUDE.md auto-load</span>
                </div>
                <p class="wo-cap-p">
                  Drop a <span class="mono">CLAUDE.md</span> in any repo
                  (or <span class="mono">~/.claude/CLAUDE.md</span> for
                  user-global). Walked up to repo root; concatenated;
                  HTML comments stripped; <span class="mono">@path</span>
                  imports resolved. Auto-prepended to every agent turn's
                  system prompt.
                </p>
              </div>
              <div class="wo-cap" data-tone="source">
                <div class="wo-cap-h">
                  <span class="wo-cap-icon" aria-hidden="true">
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><circle cx="9" cy="9" r="6"/><path d="M21 21l-7-7"/><path d="M3 17l3-3"/></svg>
                  </span>
                  <span>Skills + slash commands</span>
                </div>
                <p class="wo-cap-p">
                  Custom skills under <span class="mono">~/.claude/skills/</span>
                  and <span class="mono">&lt;repo&gt;/.claude/skills/</span>.
                  Body supports <span class="mono">$ARGUMENTS</span> and
                  inline <span class="mono">{`!`}</span><span class="mono">{`<cmd>`}</span> shell injection — the agent reads
                  pre-resolved data, not commands. <span class="mono">/loop 5m
                  &lt;prompt&gt;</span> schedules a recurring send.
                  <span class="mono">⌘⇧A</span> opens the Agent View
                  dashboard.
                </p>
              </div>
            </div>

            <div class="wo-row">
              <button class="wo-go" onclick={() => go('claudeApp')}>
                <BrandIcon kind="claude" size={14} />
                Open Claude
              </button>
              <button class="wo-go" onclick={() => go('cursorApp')}>
                <BrandIcon kind="cursor" size={14} />
                Open Cursor
              </button>
              <button class="wo-go" onclick={() => go('rules')}>
                <span class="wo-go-svg">
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"><path d="M4 6h16M4 12h16M4 18h10"/></svg>
                </span>
                Edit agent rules
              </button>
            </div>
          </section>

          <!-- ╭─ EDITOR ─╮ -->
          <section class="wo-sec" data-section="editor">
            <h3 class="wo-sec-h">Editor super-powers</h3>
            <p class="wo-sec-lead">
              Familiar shortcuts (⌘P, ⌘F, ⌘S) plus Woom-only moves
              for working with agents. Designed so the muscle memory
              you brought from Cursor / VSCode keeps working — and
              picks up new gears as you go.
            </p>

            <ol class="wo-feats">
              <li class="wo-feat">
                <div class="wo-feat-num">01</div>
                <div class="wo-feat-body">
                  <div class="wo-feat-h">
                    Multi-agent Diff Review
                    <span class="kbd">{shift}{mod} R</span>
                  </div>
                  <p class="wo-feat-p">
                    When agents touch many files, open a code-review
                    surface in the editor: hunks list, j/k navigation,
                    <span class="kbd">a</span>&nbsp;accept,
                    <span class="kbd">r</span>&nbsp;revert,
                    <span class="kbd">e</span>&nbsp;refine (sends a
                    targeted prompt back to the agent), Apply all → one
                    commit. Beats Cursor's in-chat accept boxes.
                  </p>
                </div>
              </li>
              <li class="wo-feat">
                <div class="wo-feat-num">02</div>
                <div class="wo-feat-body">
                  <div class="wo-feat-h">"Composer here" inline bubble</div>
                  <p class="wo-feat-p">
                    Select lines → click <em>Edit…</em> in the popover.
                    A mini composer opens, anchored to the selection,
                    with the agent and a <span class="mono">@path:start-end</span>
                    mention pre-attached. Send goes straight to the
                    linked chat.
                  </p>
                </div>
              </li>
              <li class="wo-feat">
                <div class="wo-feat-num">03</div>
                <div class="wo-feat-body">
                  <div class="wo-feat-h">
                    Quick open
                    <span class="kbd">{mod} P</span>
                  </div>
                  <p class="wo-feat-p">
                    Fuzzy-find any file in the active editor's repo.
                    Cycles between editor instances if you have several
                    open.
                  </p>
                </div>
              </li>
              <li class="wo-feat">
                <div class="wo-feat-num">04</div>
                <div class="wo-feat-body">
                  <div class="wo-feat-h">
                    Go to symbol
                    <span class="kbd">{shift}{mod} O</span>
                  </div>
                  <p class="wo-feat-p">
                    Outline of functions, classes, types in the open
                    file. Regex-driven for TS/JS/Svelte/Rust/Python/Go/MD —
                    no language server required.
                  </p>
                </div>
              </li>
              <li class="wo-feat">
                <div class="wo-feat-num">05</div>
                <div class="wo-feat-body">
                  <div class="wo-feat-h">
                    Find in files
                    <span class="kbd">{shift}{mod} F</span>
                  </div>
                  <p class="wo-feat-p">
                    Project-wide grep with live results. Same speed as
                    JetBrains' Find in Path; jumps to the file on Enter.
                  </p>
                </div>
              </li>
              <li class="wo-feat">
                <div class="wo-feat-num">06</div>
                <div class="wo-feat-body">
                  <div class="wo-feat-h">Pending-edits banner</div>
                  <p class="wo-feat-p">
                    When an agent writes to a file you're staring at,
                    a thin banner appears above the buffer: hunk count,
                    diff stats, Keep / Revert / Open Review. No need
                    to hunt for the chat card.
                  </p>
                </div>
              </li>
              <li class="wo-feat">
                <div class="wo-feat-num">07</div>
                <div class="wo-feat-body">
                  <div class="wo-feat-h">
                    Markdown preview
                    <span class="kbd">{shift}{mod} V</span>
                  </div>
                  <p class="wo-feat-p">
                    Open any <span class="mono">.md</span> in
                    <em>Edit</em> / <em>Split</em> / <em>Preview</em> —
                    live-typed, themed code blocks, callouts, tables,
                    GFM task lists. The shortcut cycles the three
                    modes; clicking the toolbar tabs works too.
                  </p>
                </div>
              </li>
              <li class="wo-feat">
                <div class="wo-feat-num">08</div>
                <div class="wo-feat-body">
                  <div class="wo-feat-h">Image preview</div>
                  <p class="wo-feat-p">
                    Open <span class="mono">.png</span>,
                    <span class="mono">.jpg</span>, <span class="mono">.svg</span>,
                    <span class="mono">.webp</span> — gets a real image
                    surface (checkerboard for transparency, fit/actual
                    toggle, wheel-zoom, drag-pan) instead of binary
                    garbage in CodeMirror.
                  </p>
                </div>
              </li>
            </ol>

            <div class="wo-row">
              <button class="wo-go" onclick={() => go('editorApp')}>
                <span class="wo-go-svg">
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="18" height="18" rx="2"/><line x1="3" y1="9" x2="21" y2="9"/></svg>
                </span>
                Open Editor
              </button>
            </div>
          </section>

          <!-- ╭─ SOURCES ─╮ -->
          <section class="wo-sec" data-section="sources">
            <h3 class="wo-sec-h">Sources you connect</h3>
            <p class="wo-sec-lead">
              Each source is its own solo with a live inbox; agents see
              them through MCP tools so they can act in the same world
              you read.
            </p>

            <div class="wo-src-grid">
              <button class="wo-src" data-source="github" onclick={() => go('githubApp')}>
                <BrandIcon kind="github" size={22} />
                <div class="wo-src-body">
                  <div class="wo-src-h">GitHub</div>
                  <div class="wo-src-p">PRs, issues, files, checks. Comment, review, merge from the keyboard.</div>
                </div>
                <span class="wo-src-key mono">{mod} 2</span>
              </button>
              <button class="wo-src" data-source="jira" onclick={() => go('jiraApp')}>
                <BrandIcon kind="jira" size={22} />
                <div class="wo-src-body">
                  <div class="wo-src-h">Jira</div>
                  <div class="wo-src-p">Tickets, sprints, transitions, worklogs. Filter by project / status.</div>
                </div>
                <span class="wo-src-key mono">{mod} 1</span>
              </button>
              <button class="wo-src" data-source="sentry" onclick={() => go('sentryApp')}>
                <BrandIcon kind="sentry" size={22} />
                <div class="wo-src-body">
                  <div class="wo-src-h">Sentry</div>
                  <div class="wo-src-p">Issues, events, breadcrumbs, releases. Triage and resolve inline.</div>
                </div>
                <span class="wo-src-key mono">{mod} 3</span>
              </button>
              <button class="wo-src" data-source="connect" onclick={() => go('connections')}>
                <span class="wo-src-svg">
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"><path d="M9 17H7A5 5 0 1 1 7 7h2M15 7h2a5 5 0 1 1 0 10h-2M8 12h8"/></svg>
                </span>
                <div class="wo-src-body">
                  <div class="wo-src-h">Connections</div>
                  <div class="wo-src-p">Hook up new sources, paste tokens, manage agent installs.</div>
                </div>
                <span class="wo-src-key mono">→</span>
              </button>
            </div>
          </section>

          <!-- ╭─ SHORTCUTS ─╮ -->
          <section class="wo-sec" data-section="shortcuts">
            <h3 class="wo-sec-h">Keyboard, the short list</h3>
            <p class="wo-sec-lead">
              The handful you'll actually use every day. Hit
              <span class="kbd">?</span> for the full reference.
            </p>

            <div class="wo-keys">
              <div class="wo-keys-col">
                <div class="wo-keys-h">Navigate</div>
                <div class="wo-key-row">
                  <span class="wo-key-keys"><span class="kbd">{mod} K</span></span>
                  <span class="wo-key-desc">Command palette — search anywhere</span>
                </div>
                <div class="wo-key-row">
                  <span class="wo-key-keys"><span class="kbd">{mod} E</span></span>
                  <span class="wo-key-desc">Recent things — chats, files, tickets</span>
                </div>
                <div class="wo-key-row">
                  <span class="wo-key-keys"><span class="kbd">{mod} 0</span>…<span class="kbd">{mod} 8</span></span>
                  <span class="wo-key-desc">Jump between solos by number</span>
                </div>
                <div class="wo-key-row">
                  <span class="wo-key-keys"><span class="kbd">?</span></span>
                  <span class="wo-key-desc">All keyboard shortcuts</span>
                </div>
              </div>

              <div class="wo-keys-col">
                <div class="wo-keys-h">In the editor</div>
                <div class="wo-key-row">
                  <span class="wo-key-keys"><span class="kbd">{mod} P</span></span>
                  <span class="wo-key-desc">Quick-open any file</span>
                </div>
                <div class="wo-key-row">
                  <span class="wo-key-keys"><span class="kbd">{shift}{mod} O</span></span>
                  <span class="wo-key-desc">Go to symbol in file</span>
                </div>
                <div class="wo-key-row">
                  <span class="wo-key-keys"><span class="kbd">{shift}{mod} F</span></span>
                  <span class="wo-key-desc">Find in files</span>
                </div>
                <div class="wo-key-row">
                  <span class="wo-key-keys"><span class="kbd">{shift}{mod} R</span></span>
                  <span class="wo-key-desc">Review pane (j/k · a · r · e)</span>
                </div>
                <div class="wo-key-row">
                  <span class="wo-key-keys"><span class="kbd">{mod} S</span></span>
                  <span class="wo-key-desc">Save active file</span>
                </div>
              </div>

              <div class="wo-keys-col">
                <div class="wo-keys-h">In a chat</div>
                <div class="wo-key-row">
                  <span class="wo-key-keys"><span class="kbd">Enter</span></span>
                  <span class="wo-key-desc">Send message</span>
                </div>
                <div class="wo-key-row">
                  <span class="wo-key-keys"><span class="kbd">{shift} Enter</span></span>
                  <span class="wo-key-desc">Newline</span>
                </div>
                <div class="wo-key-row">
                  <span class="wo-key-keys"><span class="kbd">↑</span> / <span class="kbd">↓</span></span>
                  <span class="wo-key-desc">Cycle previously-sent prompts</span>
                </div>
                <div class="wo-key-row">
                  <span class="wo-key-keys"><span class="mono">/help</span></span>
                  <span class="wo-key-desc">List slash commands</span>
                </div>
              </div>
            </div>

            <div class="wo-row">
              <button class="wo-go" onclick={onOpenCheatsheet}>
                <span class="kbd">?</span>
                Show full cheatsheet
              </button>
            </div>
          </section>

          <footer class="wo-foot">
            <span class="mono">⇧⌘?</span> reopens this guide any time. Press
            <span class="kbd">Esc</span> to dismiss.
          </footer>
        </div>
      </div>
    </section>
  </div>
{/if}

<style>
  .wo-backdrop {
    position: fixed; inset: 0;
    background: rgba(0, 0, 0, 0.55);
    backdrop-filter: blur(10px) saturate(1.05);
    -webkit-backdrop-filter: blur(10px) saturate(1.05);
    display: flex; align-items: center; justify-content: center;
    z-index: 1000;
    padding: 32px;
    animation: woFade var(--dur-base) var(--ease-out);
  }
  .wo-panel {
    position: relative;
    width: 100%; max-width: 1080px;
    height: min(820px, calc(100vh - 64px));
    display: flex; flex-direction: column;
    background: var(--bg-1);
    border: 1px solid color-mix(in srgb, var(--accent) 16%, var(--border-neutral-hi));
    border-radius: 18px;
    box-shadow:
      0 32px 80px rgba(0, 0, 0, 0.5),
      inset 0 1px 0 color-mix(in srgb, var(--accent) 12%, transparent);
    overflow: hidden;
    animation: woSlide var(--dur-slow) var(--ease-spring);
  }
  .wo-aurora {
    position: absolute; inset: 0;
    pointer-events: none;
    background:
      radial-gradient(60% 50% at 0% 0%, color-mix(in srgb, var(--accent) 18%, transparent), transparent 65%),
      radial-gradient(50% 50% at 110% 110%, color-mix(in srgb, var(--src-claude) 10%, transparent), transparent 70%);
    z-index: 0;
  }

  /* ── HEAD ─────────────────────────────────────────── */
  .wo-head {
    position: relative; z-index: 1;
    display: flex; align-items: flex-start;
    padding: 22px 26px 18px;
    border-bottom: 1px solid color-mix(in srgb, var(--accent) 8%, var(--border));
    gap: 16px;
  }
  .wo-brand { display: flex; gap: 16px; align-items: center; flex: 1; min-width: 0; }
  .wo-brand-text { display: flex; flex-direction: column; gap: 4px; min-width: 0; }
  .wo-title {
    margin: 0;
    font-family: 'Geist', system-ui, sans-serif;
    font-size: 22px;
    font-weight: 600;
    letter-spacing: -0.02em;
    color: var(--text-0);
  }
  .wo-tagline {
    margin: 0;
    font-size: 13px;
    color: var(--text-2);
    line-height: 1.45;
    max-width: 540px;
  }
  .wo-close {
    width: 30px; height: 30px;
    border-radius: 8px;
    background: transparent;
    border: 1px solid var(--border);
    color: var(--text-2);
    font-size: 20px; line-height: 1;
    cursor: pointer;
    flex-shrink: 0;
  }
  .wo-close:hover { background: var(--bg-2); color: var(--text-0); }

  /* ── BODY (rail + scroll) ─────────────────────────── */
  .wo-body {
    position: relative; z-index: 1;
    display: grid;
    grid-template-columns: 200px minmax(0, 1fr);
    flex: 1; min-height: 0;
  }
  @media (max-width: 720px) {
    .wo-body { grid-template-columns: 1fr; }
    .wo-nav { display: none !important; }
  }

  /* Left rail */
  .wo-nav {
    display: flex; flex-direction: column;
    padding: 16px 12px;
    border-right: 1px solid var(--border);
    background: color-mix(in srgb, var(--bg-2) 60%, transparent);
    gap: 2px;
    overflow-y: auto;
  }
  .wo-nav-row {
    display: flex; align-items: center; gap: 10px;
    padding: 8px 12px;
    border-radius: 8px;
    background: transparent;
    border: 0;
    color: var(--text-2);
    font-size: 13px;
    cursor: pointer;
    text-align: left;
    transition: background 120ms, color 120ms;
  }
  .wo-nav-row:hover { background: var(--bg-2); color: var(--text-0); }
  .wo-nav-row--active {
    background: color-mix(in srgb, var(--accent) 14%, var(--bg-2));
    color: var(--accent-bright);
  }
  .wo-nav-glyph {
    width: 18px; text-align: center;
    color: var(--text-mute);
    font-size: 13px;
  }
  .wo-nav-row--active .wo-nav-glyph { color: var(--accent-bright); }
  .wo-nav-spacer { flex: 1; min-height: 12px; }
  .wo-nav-cheatsheet {
    display: flex; align-items: center; gap: 8px;
    padding: 8px 12px;
    border-radius: 8px;
    background: var(--bg-2);
    border: 1px solid var(--border);
    color: var(--text-1);
    font-size: 12px;
    cursor: pointer;
  }
  .wo-nav-cheatsheet:hover { background: var(--bg-3); color: var(--text-0); }

  /* Scroll content */
  .wo-scroll {
    overflow-y: auto;
    padding: 22px 32px 32px;
    scroll-behavior: smooth;
    scrollbar-width: thin;
  }
  .wo-sec {
    padding: 20px 0 26px;
    border-bottom: 1px dashed color-mix(in srgb, var(--border) 70%, transparent);
  }
  .wo-sec:first-child { padding-top: 4px; }
  .wo-sec:last-of-type { border-bottom: 0; }
  .wo-sec-h {
    margin: 0 0 8px;
    font-family: 'Geist', system-ui, sans-serif;
    font-size: 19px;
    font-weight: 600;
    letter-spacing: -0.015em;
    color: var(--text-0);
  }
  .wo-sec-lead {
    margin: 0 0 18px;
    font-size: 13.5px;
    color: var(--text-1);
    line-height: 1.55;
    max-width: 720px;
  }

  /* Overview tile row ── */
  .wo-3col {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 12px;
  }
  @media (max-width: 720px) { .wo-3col { grid-template-columns: 1fr; } }
  .wo-tile {
    padding: 14px 16px;
    border-radius: 12px;
    background: var(--bg-2);
    border: 1px solid var(--border);
    display: flex; flex-direction: column; gap: 6px;
    min-width: 0;
  }
  .wo-tile[data-tone="agent"]  { border-color: color-mix(in srgb, var(--accent) 24%, var(--border)); background: linear-gradient(180deg, color-mix(in srgb, var(--accent) 8%, transparent), transparent), var(--bg-2); }
  .wo-tile[data-tone="editor"] { border-color: color-mix(in srgb, var(--src-claude) 22%, var(--border)); background: linear-gradient(180deg, color-mix(in srgb, var(--src-claude) 7%, transparent), transparent), var(--bg-2); }
  .wo-tile[data-tone="source"] { border-color: color-mix(in srgb, var(--src-jira) 22%, var(--border)); background: linear-gradient(180deg, color-mix(in srgb, var(--src-jira) 7%, transparent), transparent), var(--bg-2); }
  .wo-tile-glyph {
    font-size: 17px;
    color: var(--accent-bright);
    line-height: 1;
  }
  .wo-tile-h { margin: 0; font-size: 13.5px; font-weight: 600; color: var(--text-0); }
  .wo-tile-p { margin: 0; font-size: 12px; color: var(--text-2); line-height: 1.5; }

  /* Solo links grid ── */
  .wo-link-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(170px, 1fr));
    gap: 8px;
    margin-bottom: 16px;
  }
  .wo-link {
    display: flex; align-items: center; gap: 10px;
    padding: 10px 12px;
    border-radius: 10px;
    background: var(--bg-2);
    border: 1px solid var(--border);
    cursor: pointer;
    color: var(--text-1);
    text-align: left;
    transition: background 120ms, transform 120ms, border-color 120ms;
  }
  .wo-link:hover { transform: translateY(-1px); background: var(--bg-3); border-color: var(--border-hi); }
  .wo-link-svg { width: 18px; height: 18px; display: grid; place-items: center; color: var(--accent-bright); }
  .wo-link-svg svg, .wo-link svg { width: 18px; height: 18px; }
  .wo-link-label { font-size: 12.5px; color: var(--text-0); flex: 1; }
  .wo-link-key {
    font-size: 10px;
    color: var(--text-mute);
    background: var(--bg-3);
    padding: 1px 6px;
    border-radius: 4px;
    border: 1px solid var(--border);
  }

  /* Callout box (editor↔agent link explanation) */
  .wo-callout {
    margin-top: 6px;
    padding: 14px 16px;
    border-radius: 12px;
    background: color-mix(in srgb, var(--accent) 6%, var(--bg-2));
    border: 1px solid color-mix(in srgb, var(--accent) 22%, var(--border));
    border-left-width: 3px;
  }
  .wo-callout-h {
    font-size: 12.5px; font-weight: 600;
    color: var(--accent-bright);
    margin-bottom: 4px;
    letter-spacing: -0.01em;
  }
  .wo-callout-p { margin: 0; font-size: 12.5px; color: var(--text-1); line-height: 1.5; }

  /* Capability grid (agents) */
  .wo-cap-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
    gap: 10px;
    margin-bottom: 14px;
  }
  .wo-cap {
    --cap-tone: var(--accent-bright);
    padding: 12px 14px;
    border-radius: 10px;
    background: var(--bg-2);
    border: 1px solid var(--border);
    border-left: 2px solid color-mix(in srgb, var(--cap-tone) 55%, var(--border));
  }
  /* Per-capability tone — same palette as the source/agent solos so the
     glyphs read as coherent with the rest of the app, not random. */
  .wo-cap[data-tone="editor"]   { --cap-tone: var(--src-editor, var(--accent-bright)); }
  .wo-cap[data-tone="terminal"] { --cap-tone: var(--src-term, var(--text-2)); }
  .wo-cap[data-tone="source"]   { --cap-tone: var(--src-jira); }
  .wo-cap[data-tone="canvas"]   { --cap-tone: var(--src-canvas); }
  .wo-cap[data-tone="approval"] { --cap-tone: var(--info, var(--accent-bright)); }
  .wo-cap[data-tone="memory"]   { --cap-tone: var(--src-github); }
  .wo-cap-h {
    display: flex; align-items: center; gap: 8px;
    font-size: 12.5px; font-weight: 600;
    color: var(--text-0);
    margin-bottom: 6px;
  }
  .wo-cap-icon {
    flex-shrink: 0;
    width: 22px; height: 22px;
    display: grid; place-items: center;
    border-radius: 6px;
    color: var(--cap-tone);
    background: color-mix(in srgb, var(--cap-tone) 14%, transparent);
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--cap-tone) 22%, transparent);
  }
  .wo-cap-icon svg { width: 13px; height: 13px; }
  .wo-cap-p { margin: 0; font-size: 11.5px; color: var(--text-2); line-height: 1.5; }

  .wo-row { display: flex; gap: 8px; flex-wrap: wrap; }
  .wo-go {
    display: inline-flex; align-items: center; gap: 7px;
    padding: 7px 13px;
    border-radius: 8px;
    background: var(--bg-2);
    border: 1px solid var(--border-hi);
    color: var(--text-0);
    font-size: 12px;
    font-weight: 500;
    cursor: pointer;
    transition: background 120ms, border-color 120ms;
  }
  .wo-go:hover { background: var(--bg-3); border-color: color-mix(in srgb, var(--accent) 30%, var(--border)); }
  .wo-go-svg { width: 14px; height: 14px; display: grid; place-items: center; color: var(--accent-bright); }
  .wo-go svg { width: 14px; height: 14px; }

  /* Editor features list */
  .wo-feats {
    list-style: none;
    margin: 0; padding: 0;
    display: flex; flex-direction: column; gap: 10px;
  }
  .wo-feat {
    display: grid;
    grid-template-columns: 36px 1fr;
    gap: 12px;
    padding: 12px 14px;
    border-radius: 10px;
    background: var(--bg-2);
    border: 1px solid var(--border);
  }
  .wo-feat-num {
    font-family: 'JetBrains Mono', monospace;
    font-size: 11px;
    color: var(--accent-bright);
    background: color-mix(in srgb, var(--accent) 14%, var(--bg-3));
    border-radius: 6px;
    display: grid; place-items: center;
    align-self: start;
    height: 22px;
  }
  .wo-feat-body { display: flex; flex-direction: column; gap: 4px; min-width: 0; }
  .wo-feat-h {
    display: flex; align-items: center; gap: 8px; flex-wrap: wrap;
    font-size: 13px; font-weight: 600; color: var(--text-0);
  }
  .wo-feat-p { margin: 0; font-size: 12px; color: var(--text-2); line-height: 1.5; }

  /* Source cards */
  .wo-src-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(260px, 1fr));
    gap: 10px;
  }
  .wo-src {
    display: grid;
    grid-template-columns: 28px 1fr auto;
    gap: 12px; align-items: center;
    padding: 12px 14px;
    border-radius: 12px;
    background: var(--bg-2);
    border: 1px solid var(--border);
    cursor: pointer;
    text-align: left;
    transition: background 120ms, transform 120ms, border-color 120ms;
  }
  .wo-src:hover { background: var(--bg-3); transform: translateY(-1px); }
  .wo-src[data-source="github"]:hover { border-color: color-mix(in srgb, var(--src-github) 32%, var(--border)); }
  .wo-src[data-source="jira"]:hover   { border-color: color-mix(in srgb, var(--src-jira) 32%, var(--border)); }
  .wo-src[data-source="sentry"]:hover { border-color: color-mix(in srgb, var(--src-sentry) 32%, var(--border)); }
  .wo-src[data-source="connect"]:hover { border-color: color-mix(in srgb, var(--accent) 32%, var(--border)); }
  .wo-src-svg { display: grid; place-items: center; color: var(--accent-bright); }
  .wo-src-svg svg { width: 22px; height: 22px; }
  .wo-src-body { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
  .wo-src-h { font-size: 13px; font-weight: 600; color: var(--text-0); }
  .wo-src-p { font-size: 11.5px; color: var(--text-2); line-height: 1.4; }
  .wo-src-key {
    font-size: 10px;
    color: var(--text-mute);
    background: var(--bg-3);
    padding: 1px 6px;
    border-radius: 4px;
    border: 1px solid var(--border);
  }

  /* Shortcuts grid */
  .wo-keys {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
    gap: 16px;
    margin-bottom: 16px;
  }
  .wo-keys-col { display: flex; flex-direction: column; gap: 6px; }
  .wo-keys-h {
    font-size: 10px; font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.1em;
    color: var(--text-mute);
    margin-bottom: 4px;
  }
  .wo-key-row {
    display: grid;
    grid-template-columns: minmax(0, auto) 1fr;
    gap: 10px;
    align-items: baseline;
  }
  .wo-key-keys { display: inline-flex; gap: 4px; align-items: center; flex-wrap: wrap; }
  .wo-key-desc { font-size: 12px; color: var(--text-1); line-height: 1.4; }

  /* Inline kbd */
  .kbd {
    display: inline-flex; align-items: center;
    padding: 1px 6px;
    border-radius: 4px;
    background: var(--bg-3);
    border: 1px solid var(--border);
    color: var(--text-0);
    font-family: 'JetBrains Mono', monospace;
    font-size: 10.5px;
    line-height: 1.4;
    white-space: nowrap;
  }

  .wo-foot {
    margin-top: 18px;
    padding-top: 14px;
    border-top: 1px solid var(--border);
    font-size: 11px;
    color: var(--text-mute);
    text-align: center;
  }

  @keyframes woFade { from { opacity: 0; } to { opacity: 1; } }
  @keyframes woSlide {
    from { opacity: 0; transform: translateY(8px) scale(0.985); }
    to   { opacity: 1; transform: translateY(0)   scale(1); }
  }
  @media (prefers-reduced-motion: reduce) {
    .wo-backdrop, .wo-panel { animation: none !important; }
    .wo-scroll { scroll-behavior: auto; }
  }
</style>
