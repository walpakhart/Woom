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
        { keys: `${mod} E`, label: 'Quick switcher — jump to most-recently-touched' },
        { keys: `${mod} 0`, label: 'Switch to Home' },
        { keys: `${mod} 1..3`, label: 'Switch to Jira / GitHub / Sentry' },
        { keys: `${mod} 4..5`, label: 'Switch to Claude / Cursor' },
        { keys: `${mod} 6..8`, label: 'Switch to Editor / Canvas / Terminal' },
        { keys: '?', label: 'Show this cheatsheet' },
        { keys: `${shift}${mod} ?`, label: 'Welcome / tour — what is Woom and how is it organised' },
        { keys: 'Esc', label: 'Close overlay / focus pane' }
      ]
    },
    {
      title: 'Inbox (Jira / GitHub / Sentry)',
      rows: [
        { keys: 'j / k', label: 'Move selection in inbox lists' },
        { keys: 'o', label: 'Open focused row in browser' }
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
        { keys: `${mod}${shift} R`, label: 'Review pane — accept / reject every agent edit (j / k · a · r · e)' }
      ]
    },
    {
      title: 'Agent (Claude / Cursor)',
      rows: [
        { keys: 'Enter', label: 'Send message' },
        { keys: `${shift} Enter`, label: 'Newline in composer' },
        { keys: '↑ / ↓', label: 'Cycle through previously-sent prompts in this session' },
        { keys: `${mod} ${altKey} C`, label: 'Compact session (drop history, keep summary)' },
        { keys: '/compact', label: 'Slash command — same as the toolbar compact button' },
        { keys: '/clear', label: 'Slash command — wipe this session\'s messages' },
        { keys: '/usage', label: 'Slash command — token + cost breakdown' },
        { keys: '/help', label: 'Slash command — list every / command' }
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
