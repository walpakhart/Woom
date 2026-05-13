<script lang="ts">
  /* MentionPicker — inline autocomplete that pops above the composer
     when the user types an @ followed by anything (or clicks the @
     icon). Sources: recent Claude/Cursor sessions, Jira tickets,
     GitHub PRs/issues, Sentry events. Arrow-key nav + Enter to pick;
     Esc / outside-click to dismiss.

     The composer holds the source of truth for the textarea value;
     we only emit `onPick(mention, replaceFrom)` and let the parent
     splice the mention into the input and append it to `mentions[]`. */
  import { sessionsState } from '$lib/state/sessions.svelte';
  import { inboxState } from '$lib/state/inbox.svelte';
  import { jiraItemsFor } from '$lib/state/inbox-jira';
  import { githubItemsFor } from '$lib/state/inbox-github';
  import { sentryItemsFor } from '$lib/state/inbox-sentry';
  import { APP_INSTANCE_IDS, layoutState } from '$lib/state/layout.svelte';
  import { externalId } from '$lib/data';
  import type { Mention } from '$lib/types';
  import { invoke } from '@tauri-apps/api/core';

  type Suggestion = {
    /** What gets pasted into the input replacing `@<query>`. */
    display: string;
    /** Mention payload appended to `session.mentions`. */
    mention: Mention;
    /** Bucket label shown as a section header. */
    section: 'Files' | 'Jira' | 'GitHub' | 'Sentry' | 'Sessions';
    /** Brand-tinted dot color for the row icon. */
    tone: string;
    /** Short title shown on the row. */
    title: string;
    /** Mono ID/key shown next to the title. */
    sub?: string;
  };

  interface Props {
    /** Anchor rect for positioning the popover. The picker positions
     *  itself above this rect so it never overlaps the textarea. */
    anchor: { left: number; top: number; width: number } | null;
    /** Current query text — the user's input AFTER the @ trigger. */
    query: string;
    /** When the picker has results AND is open, swallow the next
     *  Enter / arrow keys at the textarea level. The composer reads
     *  `hasFocus` to decide whether to forward keystrokes to us. */
    onPick: (s: Suggestion) => void;
    onClose: () => void;
  }
  let p: Props = $props();

  let listEl: HTMLDivElement | null = $state(null);
  let selectedIdx = $state(0);

  /* Live filesystem search results — async, debounced. The cached
     entries flow into `filesBucket` below, so the user gets every
     file in the open repo (not just the ones already touched). */
  type FsHit = { name: string; path: string };
  let fsHits = $state<FsHit[]>([]);
  let fsRepoPath = $state('');
  let fsSearchTimer: ReturnType<typeof setTimeout> | null = null;
  let fsLoading = $state(false);

  /** Debounced fs walk — invokes the Rust command after 120ms of
   *  keyboard quiet. Cancels in-flight calls when the query keeps
   *  changing. The Rust side is bounded (max 2,000 files, depth 8)
   *  so a single round-trip is cheap; we still debounce so we don't
   *  fire on every keystroke. */
  function scheduleFsSearch() {
    if (fsSearchTimer) clearTimeout(fsSearchTimer);
    fsSearchTimer = setTimeout(() => void runFsSearch(), 120);
  }
  async function runFsSearch() {
    const editorActive = layoutState.activeInstance.editor;
    const repoPath =
      sessionsState.editorInstanceState[editorActive]?.repoPath ?? '';
    if (!repoPath) {
      fsHits = [];
      return;
    }
    fsRepoPath = repoPath;
    const q = (p.query || '').trim();
    /* Empty query → top-level files only (cap 60); typed → whole
       repo filtered by leaf name. Both go through the same Rust
       command for consistency. */
    fsLoading = true;
    try {
      const hits = await invoke<{ name: string; path: string; is_dir: boolean }[]>(
        'fs_walk_files',
        {
          root: repoPath,
          query: q.length > 0 ? q : null,
          maxFiles: q.length > 0 ? 200 : 60,
          maxDepth: q.length > 0 ? 8 : 3
        }
      );
      fsHits = hits.filter((h) => !h.is_dir).map((h) => ({ name: h.name, path: h.path }));
    } catch {
      /* Tauri unavailable (browser preview) — leave the cached tabs
         + previously-mentioned bucket as the only file source. */
      fsHits = [];
    } finally {
      fsLoading = false;
    }
  }
  /* Re-run the fs search whenever the query changes. */
  $effect(() => {
    void p.query;
    scheduleFsSearch();
  });

  /* Build the suggestion pool from live state. */

  /** Files the user has touched recently — pulled from the editor's
   *  open-tabs cache, the open repo root (folder mention), and any
   *  files already mentioned in any past session. Cheap, no FS read.
   *  In the live app the user can also drag a file from the Editor
   *  tree onto the composer to add it; this bucket is the keyboard
   *  fallback for "I want to mention a file I already touched". */
  const filesBucket = $derived.by((): Suggestion[] => {
    const out: Suggestion[] = [];
    const seen = new Set<string>();

    /* The open repo root, surfaced as a folder mention so users can say
       "look at the whole project". */
    const editorActive = layoutState.activeInstance.editor;
    const repoPath =
      sessionsState.editorInstanceState[editorActive]?.repoPath ?? '';

    /* Editor's open tabs — the user is most likely to mention these.
       v8: read the per-instance cache (`woom:editor:tabs:<id>`) instead
       of the legacy single-key (`woom:editor:tabs`) which never gets
       written today. Fallback to the legacy key for sessions saved
       before the per-instance migration so an upgrade window still
       surfaces tabs. The currently-active file is read from a sibling
       per-instance key and pinned to the top with a "current" sub-label
       so "@" + Enter just mentions whatever the editor is showing. */
    let tabs: string[] = [];
    let activeTab = '';
    try {
      const rawScoped = localStorage.getItem(`woom:editor:tabs:${editorActive}`);
      if (rawScoped) {
        const parsed = JSON.parse(rawScoped);
        if (Array.isArray(parsed)) tabs = parsed.filter((p) => typeof p === 'string');
      } else {
        const rawLegacy = localStorage.getItem('woom:editor:tabs');
        if (rawLegacy) {
          const parsed = JSON.parse(rawLegacy);
          if (Array.isArray(parsed)) tabs = parsed.filter((p) => typeof p === 'string');
        }
      }
      activeTab = localStorage.getItem(`woom:editor:active:${editorActive}`) || '';
    } catch { /* ignore corrupted cache */ }

    function shortRel(abs: string): string {
      if (repoPath && abs.startsWith(repoPath + '/')) return abs.slice(repoPath.length + 1);
      const slash = abs.lastIndexOf('/');
      return slash >= 0 ? abs.slice(slash + 1) : abs;
    }

    /* Pin the active editor file FIRST so a quick "@" + Enter on an
       empty query mentions whatever the user is reading right now. */
    if (activeTab && !seen.has(activeTab)) {
      const rel = shortRel(activeTab);
      out.push({
        display: '@' + rel + ' ',
        mention: {
          source: 'file',
          externalId: rel,
          title: rel.split('/').pop() || rel,
          body: activeTab,
          isDir: false
        },
        section: 'Files',
        tone: 'var(--src-editor)',
        title: rel.split('/').pop() || rel,
        sub: 'current'
      });
      seen.add(activeTab);
    }

    if (repoPath && !seen.has(repoPath)) {
      const name = repoPath.slice(repoPath.lastIndexOf('/') + 1) || repoPath;
      out.push({
        display: '@' + name + '/ ',
        mention: {
          source: 'file',
          externalId: name + '/',
          title: name,
          body: repoPath,
          isDir: true
        },
        section: 'Files',
        tone: 'var(--src-editor)',
        title: name + '/',
        sub: 'project root'
      });
      seen.add(repoPath);
    }

    for (const path of tabs) {
      if (seen.has(path)) continue;
      seen.add(path);
      const rel = shortRel(path);
      out.push({
        display: '@' + rel + ' ',
        mention: {
          source: 'file',
          externalId: rel,
          title: rel.split('/').pop() || rel,
          body: path,
          isDir: false
        },
        section: 'Files',
        tone: 'var(--src-editor)',
        title: rel.split('/').pop() || rel,
        sub: rel
      });
    }

    /* Any file already attached to any session — surfaces "the same
       README.md the user pasted yesterday" without re-typing. */
    for (const sess of sessionsState.list) {
      for (const m of sess.mentions ?? []) {
        if (m.source !== 'file') continue;
        const key = m.body || m.externalId;
        if (seen.has(key)) continue;
        seen.add(key);
        const rel = shortRel(m.body || m.externalId);
        out.push({
          display: '@' + rel + (m.isDir ? '/' : '') + ' ',
          mention: {
            source: 'file',
            externalId: rel + (m.isDir ? '/' : ''),
            title: m.title,
            body: m.body,
            isDir: m.isDir
          },
          section: 'Files',
          tone: 'var(--src-editor)',
          title: m.title || rel,
          sub: rel
        });
      }
    }

    /* Live fs walk results — every file in the repo matching the
       current query. These dominate when the user has actually typed
       a query (the static buckets above are the "always there" base). */
    for (const hit of fsHits) {
      if (seen.has(hit.path)) continue;
      seen.add(hit.path);
      const rel = shortRel(hit.path);
      out.push({
        display: '@' + rel + ' ',
        mention: {
          source: 'file',
          externalId: rel,
          title: hit.name,
          body: hit.path,
          isDir: false
        },
        section: 'Files',
        tone: 'var(--src-editor)',
        title: hit.name,
        sub: rel
      });
    }

    return out.slice(0, 60);
  });

  const sessionsBucket = $derived.by((): Suggestion[] => {
    return sessionsState.list
      .filter((s) => (s.title || '').trim() || s.messages.length > 0)
      .map((s) => ({
        display: '@' + (s.title || 'chat').replace(/\s+/g, '-') + ' ',
        mention: {
          source: 'chat' as const,
          externalId: s.id,
          title: s.title || 'Untitled chat',
          body: s.messages
            .slice(-2)
            .map((m) => `${m.role}: ${(m.content || '').slice(0, 200)}`)
            .join('\n')
        },
        section: 'Sessions' as const,
        tone: s.agentKind === 'cursor' ? 'var(--src-cursor)' : 'var(--src-claude)',
        title: s.title || 'Untitled chat',
        sub: s.agentKind === 'cursor' ? 'cursor' : 'claude'
      }));
  });

  const jiraBucket = $derived.by((): Suggestion[] => {
    const items = jiraItemsFor(APP_INSTANCE_IDS.jira) ?? [];
    return items.slice(0, 30).map((it) => ({
      display: '@' + it.key + ' ',
      mention: {
        source: 'jira',
        externalId: it.key,
        title: it.summary,
        body: it.description ?? null
      } as Mention,
      section: 'Jira' as const,
      tone: 'var(--src-jira)',
      title: it.summary,
      sub: it.key
    }));
  });

  const githubBucket = $derived.by((): Suggestion[] => {
    const items = githubItemsFor(APP_INSTANCE_IDS.github) ?? [];
    return items.slice(0, 30).map((it) => ({
      display: '@' + externalId(it) + ' ',
      mention: {
        source: 'github',
        externalId: externalId(it),
        title: it.title,
        body: it.body ?? null
      } as Mention,
      section: 'GitHub' as const,
      tone: 'var(--src-github)',
      title: it.title,
      sub: externalId(it)
    }));
  });

  const sentryBucket = $derived.by((): Suggestion[] => {
    const items = sentryItemsFor(APP_INSTANCE_IDS.sentry) ?? [];
    return items.slice(0, 20).map((it) => ({
      display: '@' + (it.short_id || it.id) + ' ',
      mention: {
        source: 'sentry',
        externalId: it.short_id || it.id,
        title: it.title,
        body: it.culprit ?? null
      } as Mention,
      section: 'Sentry' as const,
      tone: 'var(--src-sentry)',
      title: it.title,
      sub: it.short_id || it.id
    }));
  });

  /** All suggestions, filtered + ranked by query. Empty query shows
   *  the first few from each bucket. Ranking: exact-prefix on title
   *  or sub > substring match. We keep the section ordering stable
   *  so the user always knows where each kind sits. */
  const filtered = $derived.by((): Suggestion[] => {
    const q = (p.query || '').trim().toLowerCase();
    /* Order matches what the user is most likely to mention next:
       files first (they're working in the editor), then trackers,
       then sessions last (cross-chat references are rare). */
    const all = [
      ...filesBucket,
      ...jiraBucket,
      ...githubBucket,
      ...sentryBucket,
      ...sessionsBucket
    ];
    if (!q) return all.slice(0, 24);
    return all
      .map((s) => {
        const t = s.title.toLowerCase();
        const sub = (s.sub ?? '').toLowerCase();
        let score = 0;
        if (sub.startsWith(q)) score = 100;
        else if (t.startsWith(q)) score = 80;
        else if (sub.includes(q)) score = 50;
        else if (t.includes(q)) score = 30;
        return { s, score };
      })
      .filter((x) => x.score > 0)
      .sort((a, b) => b.score - a.score)
      .map((x) => x.s)
      .slice(0, 30);
  });

  /* Group filtered list by section for the rendered headers. */
  const grouped = $derived.by(() => {
    const map = new Map<Suggestion['section'], Suggestion[]>();
    for (const s of filtered) {
      if (!map.has(s.section)) map.set(s.section, []);
      map.get(s.section)!.push(s);
    }
    return [...map.entries()];
  });

  /* Keyboard handling — listen at window level while open. */
  function onKey(e: KeyboardEvent) {
    if (filtered.length === 0) {
      if (e.key === 'Escape') {
        e.preventDefault();
        p.onClose();
      }
      return;
    }
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      selectedIdx = Math.min(selectedIdx + 1, filtered.length - 1);
      scrollIntoView();
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      selectedIdx = Math.max(selectedIdx - 1, 0);
      scrollIntoView();
    } else if (e.key === 'Enter') {
      e.preventDefault();
      const s = filtered[selectedIdx];
      if (s) p.onPick(s);
    } else if (e.key === 'Escape') {
      e.preventDefault();
      p.onClose();
    }
  }
  function scrollIntoView() {
    if (!listEl) return;
    const row = listEl.querySelectorAll('.mp-row')[selectedIdx];
    if (row) (row as HTMLElement).scrollIntoView({ block: 'nearest' });
  }

  $effect(() => {
    /* Reset highlight when the query or list changes. */
    void p.query;
    void filtered.length;
    selectedIdx = 0;
  });
</script>

<svelte:window onkeydown={onKey} />

{#if p.anchor}
  <div
    class="mp"
    role="listbox"
    aria-label="Mention suggestions"
    style:left={p.anchor.left + 'px'}
    style:top={p.anchor.top + 'px'}
    style:min-width={p.anchor.width + 'px'}
  >
    {#if filtered.length === 0}
      <div class="mp-empty">No matches.</div>
    {:else}
      <div class="mp-list" bind:this={listEl}>
        {#each grouped as [section, items] (section)}
          <div class="mp-section">{section}</div>
          {#each items as s (s.section + ':' + s.mention.externalId)}
            {@const idx = filtered.indexOf(s)}
            <button
              type="button"
              class="mp-row"
              class:selected={idx === selectedIdx}
              onmouseenter={() => (selectedIdx = idx)}
              onmousedown={(e) => e.preventDefault()}
              onclick={() => p.onPick(s)}
              role="option"
              aria-selected={idx === selectedIdx}
            >
              <span class="mp-dot" style:background={s.tone}></span>
              <span class="mp-title">{s.title}</span>
              {#if s.sub}<span class="mp-sub mono">{s.sub}</span>{/if}
            </button>
          {/each}
        {/each}
      </div>
    {/if}
    <div class="mp-foot mono">
      <span><span class="mp-kbd">↑↓</span> navigate</span>
      <span><span class="mp-kbd">⏎</span> insert</span>
      <span><span class="mp-kbd">esc</span> close</span>
    </div>
  </div>
{/if}

<style>
  .mp {
    position: fixed;
    transform: translateY(-100%);
    margin-top: -8px;
    max-width: 480px;
    max-height: 360px;
    background: var(--bg-1);
    border: 1px solid var(--border-hi);
    border-radius: 11px;
    box-shadow: var(--shadow-3);
    z-index: 90;
    display: flex; flex-direction: column;
    overflow: hidden;
    animation: mp-pop 140ms ease-out;
  }
  @keyframes mp-pop {
    from { opacity: 0; transform: translateY(-100%) translateY(-4px); }
    to   { opacity: 1; transform: translateY(-100%); }
  }
  .mp-empty { padding: 22px; text-align: center; color: var(--text-2); font-size: 12.5px; }

  .mp-list {
    overflow-y: auto;
    padding: 6px 6px 4px;
  }
  .mp-section {
    padding: 8px 10px 6px;
    font-size: 9.5px; font-weight: 700;
    letter-spacing: 0.10em;
    text-transform: uppercase;
    color: var(--text-mute);
  }
  .mp-row {
    display: grid;
    grid-template-columns: auto 1fr auto;
    align-items: center;
    gap: 9px;
    width: 100%;
    padding: 7px 10px;
    border-radius: 7px;
    background: transparent;
    border: 0;
    text-align: left;
    cursor: pointer;
    color: var(--text-1);
    transition: background 100ms;
  }
  .mp-row:hover, .mp-row.selected {
    background: var(--bg-2);
    color: var(--text-0);
  }
  .mp-row.selected {
    box-shadow: inset 0 0 0 1px var(--border-accent-2);
  }
  .mp-dot {
    width: 7px; height: 7px;
    border-radius: 50%;
    flex-shrink: 0;
    box-shadow: 0 0 6px currentColor;
  }
  .mp-title {
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
    font-size: 12.5px;
  }
  .mp-sub {
    font-size: 10.5px; color: var(--text-mute);
  }

  .mp-foot {
    display: flex; gap: 12px;
    padding: 8px 12px;
    border-top: 1px solid var(--border);
    background: var(--bg-2);
    font-size: 10px;
    color: var(--text-mute);
  }
  .mp-kbd {
    display: inline-grid; place-items: center;
    height: 14px; min-width: 14px;
    padding: 0 4px;
    background: var(--bg-3);
    border: 1px solid var(--border);
    border-radius: 3px;
    font-size: 9px;
    color: var(--text-1);
    margin-right: 4px;
  }
</style>
