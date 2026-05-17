<script lang="ts">
  /* Agent View dashboard — a `⌘⇧A` overlay showing every Claude /
     Cursor session across all instances, grouped by lifecycle state.
     Mirrors Claude Code's `claude agents` table (CLAUDE_PARITY.md §5).
     MVP: no Haiku-summarized one-liners yet — we render the last
     assistant message's first line as a stand-in. Adds Haiku later
     behind a settings toggle + API key.

     Groups (top → bottom): Needs input, Working, Pinned, Recent,
     Older. Single-click selects + closes. Right-click toggles pin. */

  import { fly, fade } from 'svelte/transition';
  import { cubicOut } from 'svelte/easing';
  import { onMount } from 'svelte';
  import {
    sessionsState,
    setActiveSessionInInstance,
    updateSession
  } from '$lib/state/sessions.svelte';
  import { APP_INSTANCE_IDS } from '$lib/state/layout.svelte';
  import BrandIcon from '$lib/components/ui/BrandIcon.svelte';
  import type { ClaudeSession } from '$lib/types';

  interface Props {
    onClose: () => void;
    onActivate: (session: ClaudeSession) => void;
  }
  let p: Props = $props();

  /** Compute group buckets from the live session list. Each session
   *  goes into AT MOST one bucket — the first that matches its state.
   *  Order = display priority (Needs input above Working etc.). */
  const groups = $derived.by(() => {
    const out = {
      pinned: [] as ClaudeSession[],
      needsInput: [] as ClaudeSession[],
      working: [] as ClaudeSession[],
      recent: [] as ClaudeSession[],
      older: [] as ClaudeSession[]
    };
    const now = Date.now();
    const sorted = [...sessionsState.list]
      .sort((a, b) => lastTouchedAt(b) - lastTouchedAt(a));
    for (const s of sorted) {
      if (s.pinned) {
        out.pinned.push(s);
        continue;
      }
      if (s.awaitingApproval) {
        out.needsInput.push(s);
        continue;
      }
      if (s.sending) {
        out.working.push(s);
        continue;
      }
      const age = now - lastTouchedAt(s);
      if (age < 24 * 3600 * 1000) {
        out.recent.push(s);
      } else {
        out.older.push(s);
      }
    }
    return out;
  });

  function lastTouchedAt(s: ClaudeSession): number {
    const last = s.messages[s.messages.length - 1];
    if (last?.at) {
      const t = Date.parse(last.at);
      if (Number.isFinite(t)) return t;
    }
    return 0;
  }

  function summary(s: ClaudeSession): string {
    /* MVP one-liner — first non-empty line of the most-recent
     *  assistant message, trimmed to ~80 chars. Falls back to the
     *  last user message if no assistant has spoken yet. */
    for (let i = s.messages.length - 1; i >= 0; i--) {
      const m = s.messages[i];
      if (m.role !== 'assistant' && m.role !== 'user') continue;
      const line = m.content.split('\n').map((l) => l.trim()).find((l) => l.length > 0);
      if (!line) continue;
      const flat = line.replace(/\s+/g, ' ');
      return flat.length > 80 ? flat.slice(0, 77) + '…' : flat;
    }
    return '(no messages yet)';
  }

  function ageLabel(s: ClaudeSession): string {
    const ms = Date.now() - lastTouchedAt(s);
    const secs = Math.max(0, Math.floor(ms / 1000));
    if (secs < 60) return `${secs}s`;
    const m = Math.floor(secs / 60);
    if (m < 60) return `${m}m`;
    const h = Math.floor(m / 60);
    if (h < 24) return `${h}h`;
    const d = Math.floor(h / 24);
    return `${d}d`;
  }

  function activate(s: ClaudeSession): void {
    const aid = s.agentInstanceId ?? APP_INSTANCE_IDS[s.agentKind];
    if (aid) setActiveSessionInInstance(aid, s.id);
    p.onActivate(s);
    p.onClose();
  }

  function togglePin(s: ClaudeSession, e: MouseEvent): void {
    e.preventDefault();
    e.stopPropagation();
    updateSession(s.id, { pinned: !s.pinned });
  }

  /** Esc closes. Captured at the dashboard root so the underlying
   *  composer doesn't see it. */
  function onKey(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.preventDefault();
      p.onClose();
    }
  }

  /* Live filter — typed text is matched against title + cwd + recent
   *  message content. Case-insensitive substring. */
  let q = $state('');
  let searchEl: HTMLInputElement | null = $state(null);
  onMount(() => {
    /* Focus the search field on mount. Manual focus dodges the svelte
     *  `autofocus` a11y lint while keeping the same UX. */
    queueMicrotask(() => searchEl?.focus());
  });
  function matchesQuery(s: ClaudeSession): boolean {
    const needle = q.trim().toLowerCase();
    if (!needle) return true;
    if (s.title.toLowerCase().includes(needle)) return true;
    if ((s.cwd ?? '').toLowerCase().includes(needle)) return true;
    if (summary(s).toLowerCase().includes(needle)) return true;
    return false;
  }

  const filtered = $derived({
    pinned: groups.pinned.filter(matchesQuery),
    needsInput: groups.needsInput.filter(matchesQuery),
    working: groups.working.filter(matchesQuery),
    recent: groups.recent.filter(matchesQuery),
    older: groups.older.filter(matchesQuery)
  });

  const totalShown = $derived(
    filtered.pinned.length + filtered.needsInput.length + filtered.working.length +
    filtered.recent.length + filtered.older.length
  );

  const sectionDefs = $derived([
    { key: 'pinned',     label: 'Pinned',       rows: filtered.pinned,     tone: 'pinned' },
    { key: 'needsInput', label: 'Needs input',  rows: filtered.needsInput, tone: 'attention' },
    { key: 'working',    label: 'Working',      rows: filtered.working,    tone: 'live' },
    { key: 'recent',     label: 'Recent',       rows: filtered.recent,     tone: 'neutral' },
    { key: 'older',      label: 'Older',        rows: filtered.older,      tone: 'muted' }
  ] as const);
</script>

<svelte:window onkeydown={onKey} />

<div class="ad-backdrop" in:fade={{ duration: 140 }} out:fade={{ duration: 100 }}>
  <div
    class="ad-panel"
    role="dialog"
    aria-modal="true"
    aria-label="Agent sessions dashboard"
    in:fly={{ y: -12, duration: 220, easing: cubicOut }}
  >
    <header class="ad-head">
      <span class="ad-h">Agents</span>
      <input
        class="ad-search mono"
        type="text"
        bind:value={q}
        placeholder="filter by title, cwd, or last message…"
        bind:this={searchEl}
      />
      <button class="ad-close" onclick={p.onClose} aria-label="Close">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"><path d="M6 6l12 12M6 18 18 6"/></svg>
      </button>
    </header>

    {#if totalShown === 0}
      <div class="ad-empty">
        <p>No sessions match. Open a Claude or Cursor solo to start one.</p>
      </div>
    {:else}
      <div class="ad-scroll">
        {#each sectionDefs as sec (sec.key)}
          {#if sec.rows.length > 0}
            <div class="ad-section">
              <div class="ad-section-h" data-tone={sec.tone}>
                <span>{sec.label}</span>
                <span class="ad-section-count mono">{sec.rows.length}</span>
              </div>
              {#each sec.rows as s (s.id)}
                <button
                  class="ad-row"
                  data-kind={s.agentKind}
                  onclick={() => activate(s)}
                  oncontextmenu={(e) => togglePin(s, e)}
                  title="Click to activate · right-click to {s.pinned ? 'unpin' : 'pin'}"
                >
                  <div class="ad-row-icon">
                    <BrandIcon kind={s.agentKind} size={16} />
                  </div>
                  <div class="ad-row-body">
                    <div class="ad-row-line1">
                      <span class="ad-row-title">{s.title || 'Untitled chat'}</span>
                      {#if s.permissionMode === 'plan'}
                        <span class="ad-tag ad-tag--plan mono">plan</span>
                      {/if}
                      {#if s.worktreeBranch}
                        <span class="ad-tag ad-tag--branch mono">{s.worktreeBranch}</span>
                      {/if}
                      {#if s.pinned}
                        <span class="ad-pin mono" aria-label="Pinned">📌</span>
                      {/if}
                      <span class="ad-row-age mono">{ageLabel(s)}</span>
                    </div>
                    <div class="ad-row-summary">{summary(s)}</div>
                  </div>
                </button>
              {/each}
            </div>
          {/if}
        {/each}
      </div>
    {/if}

    <footer class="ad-foot mono">
      <span>↑↓ navigate · ↵ open · right-click pin · Esc close</span>
    </footer>
  </div>
</div>

<style>
  /* Backdrop sits over the whole app, semi-opaque blur. Clicking
     outside the panel closes (handled via Escape — adding explicit
     backdrop-click is a future polish). */
  .ad-backdrop {
    position: fixed; inset: 0;
    background: rgba(8, 10, 12, 0.55);
    backdrop-filter: blur(4px);
    -webkit-backdrop-filter: blur(4px);
    z-index: 800;
    display: flex; align-items: flex-start; justify-content: center;
    padding-top: 72px;
  }
  .ad-panel {
    width: min(720px, calc(100vw - 48px));
    max-height: calc(100vh - 144px);
    display: flex; flex-direction: column;
    background: var(--bg-1);
    border: 1px solid var(--border-hi);
    border-radius: 14px;
    box-shadow: 0 30px 80px rgba(0, 0, 0, 0.55), 0 0 0 1px rgba(255, 255, 255, 0.02);
    overflow: hidden;
  }

  .ad-head {
    display: flex; align-items: center;
    gap: 10px;
    padding: 12px 16px;
    border-bottom: 1px solid var(--border);
  }
  .ad-h {
    font-family: var(--font-serif, Georgia, serif);
    font-size: 16px;
    color: var(--text-0);
  }
  .ad-search {
    flex: 1;
    padding: 6px 10px;
    border-radius: 6px;
    border: 1px solid var(--border);
    background: var(--bg-2);
    color: var(--text-0);
    font-size: 12.5px;
  }
  .ad-search:focus {
    outline: 2px solid var(--accent);
    outline-offset: -1px;
    border-color: transparent;
  }
  .ad-close {
    width: 28px; height: 28px;
    display: grid; place-items: center;
    background: transparent; border: 0;
    color: var(--text-mute);
    border-radius: 6px;
    cursor: pointer;
  }
  .ad-close:hover { color: var(--text-0); background: var(--bg-2); }
  .ad-close svg { width: 14px; height: 14px; }

  .ad-scroll {
    flex: 1 1 auto;
    overflow-y: auto;
    padding: 8px 8px 12px;
  }

  .ad-section { margin-bottom: 12px; }
  .ad-section-h {
    display: flex; align-items: center;
    gap: 8px;
    padding: 6px 10px;
    font-size: 10.5px; font-weight: 700;
    letter-spacing: 0.10em;
    text-transform: uppercase;
    color: var(--text-mute);
  }
  .ad-section-h[data-tone="attention"] { color: #e0b16c; }
  .ad-section-h[data-tone="live"]      { color: #6ec3a4; }
  .ad-section-h[data-tone="pinned"]    { color: var(--accent-bright); }
  .ad-section-count {
    padding: 0 6px;
    font-size: 9.5px;
    background: var(--bg-2);
    border-radius: 4px;
    color: var(--text-mute);
  }

  /* Row chassis */
  .ad-row {
    display: flex; align-items: flex-start;
    gap: 10px;
    width: 100%;
    padding: 8px 10px;
    background: transparent;
    border: 1px solid transparent;
    border-radius: 7px;
    cursor: pointer;
    text-align: left;
    transition: background 100ms, border-color 100ms;
  }
  .ad-row:hover {
    background: var(--bg-2);
    border-color: var(--border);
  }
  .ad-row-icon {
    width: 22px; height: 22px;
    display: grid; place-items: center;
    flex-shrink: 0;
  }
  .ad-row[data-kind="claude"] { border-left: 2px solid transparent; }
  .ad-row[data-kind="claude"]:hover { border-left-color: var(--src-claude); }
  .ad-row[data-kind="cursor"]:hover { border-left-color: var(--src-cursor); }

  .ad-row-body {
    flex: 1; min-width: 0;
  }
  .ad-row-line1 {
    display: flex; align-items: center;
    gap: 6px;
    margin-bottom: 2px;
  }
  .ad-row-title {
    font-size: 12.5px;
    color: var(--text-0);
    font-weight: 500;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
    flex: 1; min-width: 0;
  }
  .ad-row-age {
    font-size: 10.5px;
    color: var(--text-mute);
    margin-left: auto;
    flex-shrink: 0;
  }
  .ad-tag {
    font-size: 9.5px; font-weight: 700;
    padding: 1px 5px;
    border-radius: 3px;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    border: 1px solid var(--border);
    background: var(--bg-2);
    color: var(--text-mute);
    flex-shrink: 0;
  }
  .ad-tag--plan {
    color: #e0b16c;
    border-color: color-mix(in srgb, #e0b16c 40%, var(--border));
  }
  .ad-tag--branch {
    text-transform: none;
    letter-spacing: 0;
    font-weight: 500;
    max-width: 140px;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .ad-pin { font-size: 11px; flex-shrink: 0; }

  .ad-row-summary {
    font-size: 11.5px;
    color: var(--text-mute);
    line-height: 1.4;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }

  .ad-empty {
    padding: 40px 16px;
    text-align: center;
    color: var(--text-mute);
    font-size: 12.5px;
  }
  .ad-empty p { margin: 0; }

  .ad-foot {
    padding: 8px 16px;
    border-top: 1px solid var(--border);
    font-size: 10.5px;
    color: var(--text-mute);
    text-align: center;
  }
</style>
