<script lang="ts">
  /* Keyboard shortcut overlay (`?` to toggle).
   *
   * Single source of truth for every shortcut Woom listens to —
   * if you add a new binding in +page.svelte or a child, list it here
   * too. Categorised so the user can scan; rendered as a focus-trapped
   * dialog with an Escape close path for screen readers (matches the
   * 1.0 a11y bar in `docs/ROADMAP_1.0.md §1.6`).
   */

  import { focusTrap } from '$lib/actions/focusTrap';

  interface Props {
    open: boolean;
    onClose: () => void;
    /** Optional handoff to the longer-form Welcome / orientation
     *  surface. When provided, the cheatsheet footer surfaces a
     *  "New here? Take the tour →" link so users who hit `?` looking
     *  for help (not just a key list) can discover it. */
    onOpenWelcome?: () => void;
  }

  let { open, onClose, onOpenWelcome }: Props = $props();

  /* Detect macOS for the ⌘ vs Ctrl rendering. Woom ships macOS-
   * only today (`docs/SPEC.md §13`), but the rest of the codebase
   * still does this check defensively in a few places. */
  const isMac =
    typeof navigator !== 'undefined' && /Mac/i.test(navigator.platform);
  const mod = isMac ? '⌘' : 'Ctrl';
  const altKey = isMac ? '⌥' : 'Alt';
  const shift = isMac ? '⇧' : 'Shift';

  type Shortcut = { keys: string; label: string };
  type Section = { title: string; rows: Shortcut[] };

  const sections: Section[] = $derived([
    {
      title: 'Global',
      rows: [
        { keys: `${mod} K`, label: 'Open command palette — search anywhere' },
        { keys: `${shift}${mod} A`, label: 'Agent View — every Claude / Cursor session grouped by state (Needs input / Working / Pinned / Recent / Older)' },
        { keys: `${mod} E`, label: 'Quick switcher — jump to most-recently-touched' },
        { keys: `${mod} 0`, label: 'Switch to Home' },
        { keys: `${mod} 1..3`, label: 'Switch to Jira / GitHub / Sentry' },
        { keys: `${mod} 4..5`, label: 'Switch to Claude / Cursor' },
        { keys: `${mod} 6..8`, label: 'Switch to Editor / Canvas / Terminal' },
        { keys: `${mod} [`, label: 'Back — previous solo in history' },
        { keys: `${mod} ]`, label: 'Forward — undo a Back step' },
        { keys: `${mod} \\`, label: 'Toggle UI density — Comfortable / Compact' },
        { keys: '?', label: 'Show this cheatsheet' },
        { keys: `${shift}${mod} ?`, label: 'Welcome / tour — what is Woom and how is it organised' },
        { keys: 'Esc', label: 'Close overlay / focus pane' }
      ]
    },
    {
      title: 'Inbox (Jira / GitHub / Sentry)',
      rows: [
        { keys: 'j / k', label: 'Move selection in inbox lists' },
        { keys: 'o', label: 'Open focused row in browser' },
        { keys: 'filter chips', label: 'Search + role + state + repo / project / level filters persist per solo across switches and restarts' },
        { keys: 'right-click row', label: 'Card actions menu — Send to Claude / Cursor, Open in browser, Copy ref' }
      ]
    },
    {
      title: 'Chat rendering',
      rows: [
        { keys: '```diff blocks', label: 'Fenced diff code blocks render with +/− line coloring + left stripe inline' },
        { keys: '@github/PR-N', label: 'Inline mentions get per-source tinted chips in the composer backdrop (purple = GitHub, blue = Jira, plum = Sentry, rust = chat)' },
        { keys: 'interrupted banner', label: 'Amber bar above the chat appears if the previous turn crashed; clears on next send' }
      ]
    },
    {
      title: 'Editor (when focused)',
      rows: [
        { keys: `${mod} S`, label: 'Save active file' },
        { keys: `${mod} P`, label: 'Quick-open file in repo' },
        { keys: `${mod}${shift} O`, label: 'Go to symbol in file (regex outline — TS / Rust / Py / Go / Svelte / MD)' },
        { keys: `${mod}${shift} V`, label: 'Markdown preview — cycle Edit / Split / Preview' },
        { keys: `${mod} F`, label: 'Find in current buffer' },
        { keys: `${mod}${shift} F`, label: 'Find in files — project-wide grep' },
        { keys: `${mod}${shift} R`, label: 'Review pane — accept / reject every agent edit (j / k · a · r · e)' },
        { keys: `${mod}${shift} B`, label: 'New git branch — opens GitPanel branch picker with the create input focused' }
      ]
    },
    {
      title: 'Agent (Claude / Cursor)',
      rows: [
        { keys: 'Enter', label: 'Send message' },
        { keys: `${shift} Enter`, label: 'Newline in composer' },
        { keys: '↑ / ↓', label: 'Cycle through previously-sent prompts in this session' },
        { keys: `${mod} .`, label: 'Stop the active agent run (mid-stream interrupt)' },
        { keys: `${mod} ${altKey} C`, label: 'Compact session (drop history, keep summary)' },
        { keys: `${shift} Tab`, label: 'Plan mode toggle — agent reads only (no edits / mutating bash) until you flip back' },
        { keys: '/', label: 'Open slash-command picker (↑/↓ + Tab to autocomplete, Enter sends)' },
        { keys: '/compact', label: 'Slash command — same as the toolbar compact button' },
        { keys: '/clear', label: 'Slash command — wipe this session\'s messages' },
        { keys: '/usage', label: 'Slash command — token + cost breakdown' },
        { keys: '/help', label: 'Slash command — list every / command' },
        { keys: '@', label: 'Mention picker — pull GitHub PR / Jira / Sentry / file / chat as context' },
        { keys: 'Paste ≥500ch', label: 'Long paste → toast offers "Save as memory" so the content stays after the chat ends' }
      ]
    },
    {
      title: 'Preview (dev servers)',
      rows: [
        { keys: 'right rail icon', label: 'Open Preview pane — long-running tasks (dev servers, watchers, test loops)' },
        { keys: '/preview pnpm dev', label: 'Spawn a background process tracked by id. Output streams in the pane.' },
        { keys: '/preview (no args)', label: 'Open the pane without spawning (focus on existing tasks)' },
        { keys: '/ps', label: 'Inline markdown table of running tasks' },
        { keys: '/kill <id|label>', label: 'Kill a task by id or label substring' },
        { keys: 'webview tab', label: 'Auto-switches to embedded preview when http://localhost:PORT detected; reload glyph forces refresh' },
        { keys: 'horizontal strip', label: 'Task chips scroll horizontally (hidden scrollbar) — click to switch active task' }
      ]
    },
    {
      title: 'Loops',
      rows: [
        { keys: '/loop 5m <prompt>', label: 'Re-send a prompt every 5 minutes (cadence accepts 30s, 2h, 1d, 2h30m). 7-day auto-expiry.' },
        { keys: '/unloop', label: 'Stop the active loop on this session' }
      ]
    },
    {
      title: 'Skills (Claude Code parity)',
      rows: [
        { keys: '~/.claude/skills/<name>/SKILL.md', label: 'User-global skill — frontmatter (name/description/argument-hint/allowed-tools/model) + body with $ARGUMENTS and ``!`<cmd>``` shell injection' },
        { keys: '<repo>/.claude/skills/<name>/SKILL.md', label: 'Project-scoped skill (walked up from session cwd; wins on name collision with user skills)' },
        { keys: '/<skill-name> <args>', label: 'Dispatches via Composer slash picker — pre-resolves !-shell blocks before agent sees the body' }
      ]
    },
    {
      title: 'Project memory (CLAUDE.md)',
      rows: [
        { keys: '~/.claude/CLAUDE.md', label: 'User-global rules auto-loaded into every session\'s system prompt' },
        { keys: 'CLAUDE.md / .claude/CLAUDE.md', label: 'Project rules — walked up from session cwd, more-specific overrides loaded last' },
        { keys: '@path/to/file.md', label: 'Inside CLAUDE.md: include another markdown file (recursive up to 5 hops, cycle-safe)' },
        { keys: 'HTML comments', label: '<!-- ... --> blocks stripped before injection — leave them in for editor readability' }
      ]
    },
    {
      title: 'Agent asks YOU',
      rows: [
        { keys: 'inline question card', label: 'Agent calls `ask_user_question` MCP tool when your preference materially changes the next step — click an option (or pick several when multi-select)' },
        { keys: '"Other" field', label: 'Always present free-form input — type a custom answer the agent gets alongside or instead of clicked options' },
        { keys: 'Dismiss', label: 'Closes the card with "declined to answer"; agent decides whether to ask differently or stop' }
      ]
    },
    {
      title: 'Hooks',
      rows: [
        { keys: '~/Library/Application Support/Woom/hooks.json', label: 'Wire shell scripts into UserPromptSubmit / Stop / SessionStart' },
        { keys: 'exit 2', label: 'Hook blocks the action with stderr fed back as feedback' },
        { keys: 'stdout JSON', label: '{ "updated_prompt": "...", "additional_context": "..." } — append rewritten prompt or context' }
      ]
    },
    {
      title: 'Long-term memory',
      rows: [
        { keys: 'ChatHeader 💾', label: 'Click the bookmark chip to open the project-memory popover (full content + Copy)' },
        { keys: 'SessionsSidebar 💾', label: 'Per-row badge — count of memories tied to that chat (saved + auto-distilled)' },
        { keys: 'Settings → Memory', label: 'Browser with search / kind filter / inline editor / delete (stats panel on top)' },
        { keys: `${mod} K → query`, label: 'Command palette includes a Memory section — Click row to preview' },
        { keys: 'auto-distill', label: 'Deleting a chat writes a snapshot to memory (first user + last assistant)' },
        { keys: 'crash recovery', label: 'Mid-turn force-quit → next send auto-injects "↩ Recovered" recap + rotates CLI uuid' }
      ]
    },
    {
      title: 'Right-click context menus',
      rows: [
        { keys: 'PR / Jira / Sentry row', label: 'Send to Claude · Send to Cursor · Open in browser · Copy URL / key / short-id' },
        { keys: 'Chat message', label: 'Save to memory · Copy text · (user) Edit + resend · (user) Resend' },
        { keys: 'Session row', label: 'Rename · Copy transcript · Save to memory · Delete chat (auto-distills first)' },
        { keys: 'File tree row', label: 'Reveal in Finder · Copy path · Rename · Delete' }
      ]
    },
    {
      title: 'Drag-and-drop',
      rows: [
        { keys: 'Inbox row → rail', label: 'Drop PR / Jira / Sentry on Claude or Cursor rail icon to send as context' },
        { keys: 'Inbox row → Canvas', label: 'Drop on Canvas to pin as a live card (state syncs with the source)' },
        { keys: 'Chat msg grip', label: 'Hover a chat message → ⋮⋮ grip icon → drag to Canvas to pin as a sticky' },
        { keys: 'File → Composer', label: 'Drop a file from Finder or the editor tree to attach as @mention' }
      ]
    },
    {
      title: 'Home',
      rows: [
        { keys: 'hover heatmap', label: '14d × 24h activity grid — hover any cell to see exact count for that day + hour' },
        { keys: 'stat card click', label: 'Jumps to the matching solo (active chats → Claude, open PRs → GitHub, etc.)' }
      ]
    },
    {
      title: 'Editor',
      rows: [
        { keys: 'left-gutter stripe', label: 'Per-line git change bar: green = added, ochre = modified, red hairline = deletion' },
        { keys: 'tree state', label: 'Expanded folders persist per-repo across reloads (localStorage)' },
        { keys: 'tab strip', label: 'Drag a file from the tree to pin it; ⌘W closes; middle-click closes' },
        { keys: 'Diff view ⛶', label: 'Click expand icon in the diff header to open full-screen; Esc closes' },
        { keys: 'Review pane ⛶', label: 'Sidebar review tab also opens full-screen for wide-format diff reading' }
      ]
    },
    {
      title: 'Canvas (when focused)',
      rows: [
        { keys: 'V / S / E / A / T / N / F / D', label: 'Tools: select / shape / ellipse / arrow / text / sticky / frame / draw' },
        { keys: `${mod} Z / ${mod}${shift} Z`, label: 'Undo / redo' },
        { keys: `${mod} G / ${mod}${shift} G`, label: 'Group / ungroup' },
        { keys: 'M', label: 'Toggle minimap' },
        { keys: `${mod} P`, label: 'Open canvas library' }
      ]
    }
  ]);

  function onKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.preventDefault();
      onClose();
    }
  }

  /* Close only when the click landed directly on the backdrop —
   * `currentTarget === target` filters out clicks that bubbled up
   * from inside the panel. Avoids needing a `stopPropagation` on
   * the inner `<section>` (which would trip svelte's a11y lint
   * about non-interactive elements with mouse handlers). */
  function onBackdropClick(e: MouseEvent) {
    if (e.target === e.currentTarget) onClose();
  }
</script>

{#if open}
  <!-- Backdrop captures clicks landing outside the panel. -->
  <div
    class="cheatsheet-backdrop"
    role="dialog"
    aria-modal="true"
    aria-labelledby="cheatsheet-title"
    onclick={onBackdropClick}
    onkeydown={onKeydown}
    tabindex="-1"
    use:focusTrap
  >
    <section class="cheatsheet-panel" aria-label="Keyboard shortcuts">
      <header class="cheatsheet-head">
        <h2 id="cheatsheet-title" class="cheatsheet-title">Keyboard shortcuts</h2>
        <button class="cheatsheet-close" onclick={onClose} aria-label="Close cheatsheet">×</button>
      </header>
      <div class="cheatsheet-body">
        {#each sections as sec (sec.title)}
          <section class="cheatsheet-section">
            <h3 class="cheatsheet-section-title">{sec.title}</h3>
            <dl class="cheatsheet-list">
              {#each sec.rows as row (row.keys + row.label)}
                <div class="cheatsheet-row">
                  <dt class="cheatsheet-keys mono">{row.keys}</dt>
                  <dd class="cheatsheet-label">{row.label}</dd>
                </div>
              {/each}
            </dl>
          </section>
        {/each}
      </div>
      <footer class="cheatsheet-foot">
        <span class="cheatsheet-hint">Read the spec: <span class="mono">docs/ROADMAP_1.0.md</span></span>
        {#if onOpenWelcome}
          <button class="cheatsheet-tour-btn" onclick={onOpenWelcome} type="button">
            New here? Take the tour <span class="mono">{shift}{mod} ?</span> →
          </button>
        {/if}
      </footer>
    </section>
  </div>
{/if}

<style>
  .cheatsheet-backdrop {
    position: fixed; inset: 0;
    background: rgba(0, 0, 0, 0.55);
    backdrop-filter: blur(4px);
    display: flex; align-items: center; justify-content: center;
    z-index: 1000;
    padding: 32px;
  }
  .cheatsheet-panel {
    width: 100%; max-width: 720px;
    max-height: calc(100vh - 64px);
    display: flex; flex-direction: column;
    background: var(--bg-1);
    border: 1px solid var(--border-neutral-hi);
    border-radius: 14px;
    box-shadow: 0 32px 64px rgba(0, 0, 0, 0.4);
    overflow: hidden;
  }
  .cheatsheet-head {
    display: flex; align-items: center; justify-content: space-between;
    padding: 16px 22px;
    border-bottom: 1px solid var(--border-neutral);
  }
  .cheatsheet-title { margin: 0; font-size: 16px; font-weight: 600; color: var(--text-0); }
  .cheatsheet-close {
    width: 28px; height: 28px; border-radius: 6px;
    background: none; border: none; color: var(--text-2);
    font-size: 22px; line-height: 1; cursor: pointer;
  }
  .cheatsheet-close:hover { background: var(--bg-2); color: var(--text-0); }

  .cheatsheet-body {
    overflow-y: auto;
    padding: 18px 22px 12px;
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(280px, 1fr));
    gap: 18px 28px;
  }
  .cheatsheet-section { min-width: 0; }
  .cheatsheet-section-title {
    margin: 0 0 8px;
    font-size: 11px; font-weight: 600; text-transform: uppercase;
    letter-spacing: 0.08em; color: var(--text-mute);
  }
  .cheatsheet-list { margin: 0; display: flex; flex-direction: column; gap: 6px; }
  .cheatsheet-row {
    display: grid;
    grid-template-columns: minmax(0, auto) 1fr;
    gap: 12px; align-items: baseline;
  }
  .cheatsheet-keys {
    font-size: 11.5px; color: var(--text-0);
    background: var(--bg-2);
    border: 1px solid var(--border-neutral);
    border-radius: 5px; padding: 1px 6px;
    white-space: nowrap;
  }
  .cheatsheet-label { margin: 0; font-size: 12.5px; color: var(--text-1); line-height: 1.4; }
  .cheatsheet-foot {
    display: flex; align-items: center; justify-content: space-between; gap: 12px;
    padding: 10px 22px; border-top: 1px solid var(--border-neutral);
    font-size: 11px; color: var(--text-mute);
  }
  .cheatsheet-tour-btn {
    background: transparent; border: 0;
    color: var(--accent-bright);
    font-size: 11px; font-weight: 600;
    cursor: pointer;
    padding: 0;
    display: inline-flex; align-items: center; gap: 6px;
    letter-spacing: 0.01em;
  }
  .cheatsheet-tour-btn:hover { color: var(--accent); }
  .cheatsheet-tour-btn .mono {
    background: var(--bg-2);
    border: 1px solid var(--border-neutral);
    border-radius: 4px;
    padding: 1px 5px;
    font-size: 10px;
    color: var(--text-1);
  }

  @media (prefers-reduced-motion: reduce) {
    .cheatsheet-backdrop { backdrop-filter: none; }
  }
</style>
