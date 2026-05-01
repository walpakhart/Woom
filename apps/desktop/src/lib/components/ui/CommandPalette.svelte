<script lang="ts">
  /* Spotlight-style universal navigator. Single search box, results
     grouped by section. Filters across every entity the user might
     want to jump to:
       Views — Workbench / Repositories / Tasks / Issues / Rules /
                Connections / Settings (top-level routes)
       Workbenches — switch the active workbench tab
       Editors — editor instances with their cwd + linked-session
                  description (the user explicitly asked for this)
       Columns — non-editor instances (github/jira/sentry/claude/cursor)
       Repos / Boards / Projects — pre-loaded option lists
       Items — currently-loaded GitHub PRs / Jira issues / Sentry errors
     A picked row dispatches the right action (setView, goToInstance,
     updateXFilters, openFocus…) and closes the palette. */

  import { externalId, type InboxItem, type JiraItem, type SentryIssue } from '$lib/data';
  import {
    inboxState,
    selectInboxItem,
    updateJiraTabFilters,
    openSentryFocus,
    scheduleSentryTabFilterRefresh
  } from '$lib/state/inbox.svelte';
  import {
    layoutState,
    setActiveWorkbench,
    goToInstance
  } from '$lib/state/layout.svelte';
  import { sessionsState } from '$lib/state/sessions.svelte';
  import { fuzzyScoreAny } from '$lib/services/fuzzyMatch';
  import { mruRank, recordPalettePick } from '$lib/state/paletteMru.svelte';
  import { isPalettePinned, togglePalettePin, pinnedState } from '$lib/state/pinned.svelte';
  import { focusTrap } from '$lib/actions/focusTrap';
  import type { View } from '$lib/state/view.svelte';

  /** Top-level commands rendered in the palette's "Actions" section.
   *  Pure-data shape so +page.svelte can build the list once with the
   *  right closures (open the connect modal, toggle cheatsheet, …)
   *  rather than the palette knowing every callback. */
  export type PaletteAction = {
    /** Stable id for MRU tracking. */
    id: string;
    /** Verb-led label, e.g. "Connect GitHub". */
    label: string;
    /** Optional one-line hint shown after the label. */
    sub?: string;
    /** Free-form keywords concatenated into the fuzzy match input —
     *  surfaces the action even when the user types something
     *  abbreviated like "ghub" instead of "github". */
    keywords?: string;
    pick: () => void;
  };

  interface Props {
    open: boolean;
    /** `view` is a local $state in +page.svelte, so the palette can't
     *  flip it directly via setView() from view.svelte.ts (that store
     *  is unused there). Parent passes a callback. */
    setView: (v: View) => void;
    /** Top-level "Actions" rows (connect / disconnect / new workbench /
     *  show cheatsheet / report bug …). Built by the parent. */
    actions?: PaletteAction[];
  }

  let { open = $bindable(), setView, actions = [] }: Props = $props();

  let query = $state('');
  let selectedIdx = $state(0);

  type Result = {
    key: string;
    badge: string;
    badgeKind: 'view' | 'workbench' | 'editor' | 'canvas' | 'github' | 'jira' | 'sentry' | 'claude' | 'cursor' | 'action';
    title: string;
    subtitle?: string;
    section: string;
    /** Fuzzy-match score; -1 means "no query, neutral". Higher
     *  scores rank earlier within their section. */
    score: number;
    pick: () => void;
  };

  function close() {
    open = false;
    query = '';
    selectedIdx = 0;
  }

  /* Fuzzy score against any of the supplied fields, plus a small
   * MRU bonus so a row the user just picked floats higher on the
   * next ambiguous query. Returns null when no field matches the
   * query — caller drops the row. */
  function scoreFor(key: string, ...fields: (string | null | undefined)[]): number | null {
    const q = query.trim();
    /* `pinnedState.palette` read so the deriveds re-run when the
     * user pins / unpins a row. */
    const pinned = isPalettePinned(key);
    if (!q) return pinned ? 100 : 0;
    const base = fuzzyScoreAny(q, fields);
    if (base === null) return null;
    /* MRU boost: rank 0..49, give 0..10 points. Smaller than the
     * fuzzy bonuses (boundary +6, consec +4) so MRU never overrides
     * an obvious typed match — only acts as a tie-breaker for short
     * queries. */
    const rank = mruRank(key);
    const mru = rank < 0 ? 0 : Math.max(0, 10 - rank);
    /* Pin boost is large enough to float a pinned item above
     * incidental fuzzy matches but below an exact-prefix typed
     * match. */
    const pinBoost = pinned ? 50 : 0;
    return base + mru + pinBoost;
  }
  /* Re-read via $derived so reactivity tracks Set replacement when
   * pins toggle. */
  $effect(() => {
    void pinnedState.palette;
  });

  const VIEWS: { key: View; title: string; sub: string }[] = [
    { key: 'workbench', title: 'Workbench', sub: 'Active columns' },
    { key: 'githubTab', title: 'GitHub', sub: 'Repos / PRs / issues / actions' },
    { key: 'jiraTab', title: 'Jira', sub: 'Tickets / boards / sprints' },
    { key: 'sentryTab', title: 'Sentry', sub: 'Errors / events' },
    { key: 'rules', title: 'Rules', sub: 'Claude system prompts' },
    { key: 'connections', title: 'Connections', sub: 'GitHub / Jira / Sentry / etc.' },
    { key: 'settings', title: 'Settings', sub: '' }
  ];

  const KIND_BADGE: Record<string, string> = {
    github: 'GH',
    jira: 'J',
    sentry: 'St',
    claude: 'C',
    cursor: 'Cr',
    editor: 'Ed',
    canvas: 'Cv'
  };

  function capitalize(s: string) {
    return s ? s[0].toUpperCase() + s.slice(1) : s;
  }

  function shortFolder(p: string): string {
    const parts = p.split('/').filter(Boolean);
    return parts.length ? parts[parts.length - 1] : p;
  }

  /* Build results derived from query + state. Each section caps at
     LIMIT_PER_SECTION rows so a workspace with hundreds of issues
     doesn't drown the workbench / editors / repos that are usually
     what the user actually wants to reach via Cmd+K. Inside a
     section rows are sorted by descending fuzzy score so the most
     relevant match floats to the top. */
  const LIMIT_PER_SECTION = 6;

  /** Sort results by descending score and trim to the section cap.
   *  Mutates the input array in place — only used on locally-built
   *  arrays. */
  function rankAndCap(rows: Result[]): Result[] {
    rows.sort((a, b) => b.score - a.score);
    return rows.slice(0, LIMIT_PER_SECTION);
  }

  const results = $derived.by((): Result[] => {
    const r: Result[] = [];
    const push = (sectionResults: Result[]) => {
      for (const res of rankAndCap(sectionResults)) r.push(res);
    };

    // 0. Actions (top-level verbs — Connect / Disconnect / Open settings /
    //    Show cheatsheet / etc.). Always rendered before content rows so
    //    the user types "conn" and immediately sees the connect verbs.
    {
      const rows: Result[] = [];
      for (const a of actions) {
        const s = scoreFor(`action:${a.id}`, a.label, a.sub, a.keywords, a.id);
        if (s === null) continue;
        rows.push({
          key: `action:${a.id}`,
          badge: '⚡',
          badgeKind: 'action' as const,
          title: a.label,
          subtitle: a.sub,
          section: 'Actions',
          score: s,
          pick: () => {
            recordPalettePick(`action:${a.id}`);
            a.pick();
            close();
          }
        });
      }
      push(rows);
    }

    // 1. Views
    push(
      VIEWS.flatMap((v) => {
        const s = scoreFor(`view:${v.key}`, v.title, v.sub, v.key);
        if (s === null) return [];
        return [{
          key: `view:${v.key}`,
          badge: '↗',
          badgeKind: 'view' as const,
          title: v.title,
          subtitle: v.sub || undefined,
          section: 'Views',
          score: s,
          pick: () => {
            recordPalettePick(`view:${v.key}`);
            setView(v.key);
            close();
          }
        }];
      })
    );

    // 2. Workbenches
    push(
      layoutState.workbenches.flatMap((wb) => {
        const s = scoreFor(`wb:${wb.id}`, wb.name);
        if (s === null) return [];
        return [{
          key: `wb:${wb.id}`,
          badge: 'WB',
          badgeKind: 'workbench' as const,
          title: wb.name,
          subtitle: `${wb.instances.length} column${wb.instances.length === 1 ? '' : 's'}`,
          section: 'Workbenches',
          score: s,
          pick: () => {
            recordPalettePick(`wb:${wb.id}`);
            setActiveWorkbench(wb.id);
            setView('workbench');
            close();
          }
        }];
      })
    );

    // 3. Editor instances — special: show folder + link status
    const editorRows: Result[] = [];
    for (const wb of layoutState.workbenches) {
      for (const inst of wb.instances) {
        if (inst.kind !== 'editor') continue;
        const ed = sessionsState.editorInstanceState[inst.id];
        const cwd = ed?.repoPath ?? '';
        const folder = cwd ? shortFolder(cwd) : '(empty)';
        const linkedSession = sessionsState.list.find(
          (s) => s.linkedToEditorInstanceId === inst.id
        );
        const linkSub = linkedSession
          ? `linked → ${linkedSession.title || linkedSession.id.slice(0, 6)}`
          : 'unlinked';
        const score = scoreFor(`inst:${inst.id}`, inst.name, cwd, folder, linkSub, wb.name);
        if (score === null) continue;
        editorRows.push({
          key: `inst:${inst.id}`,
          badge: 'Ed',
          badgeKind: 'editor',
          title: `Editor · ${inst.name}`,
          subtitle: `${folder} · ${linkSub} · ${wb.name}`,
          section: 'Editors',
          score,
          pick: () => {
            recordPalettePick(`inst:${inst.id}`);
            void goToInstance(inst.id, wb.id);
            setView('workbench');
            close();
          }
        });
      }
    }
    push(editorRows);

    // 4. Other panel instances (github/jira/sentry/claude/cursor columns)
    const colRows: Result[] = [];
    for (const wb of layoutState.workbenches) {
      for (const inst of wb.instances) {
        if (inst.kind === 'editor') continue;
        const score = scoreFor(`inst:${inst.id}`, inst.name, inst.kind, wb.name);
        if (score === null) continue;
        colRows.push({
          key: `inst:${inst.id}`,
          badge: KIND_BADGE[inst.kind] ?? '?',
          badgeKind: inst.kind as Result['badgeKind'],
          title: `${capitalize(inst.kind)} · ${inst.name}`,
          subtitle: wb.name,
          section: 'Columns',
          score,
          pick: () => {
            recordPalettePick(`inst:${inst.id}`);
            void goToInstance(inst.id, wb.id);
            setView('workbench');
            close();
          }
        });
      }
    }
    push(colRows);

    // 5. GitHub repos
    push(
      inboxState.githubRepoOptions.flatMap((repo) => {
        const score = scoreFor(`repo:${repo.full_name}`, repo.full_name, repo.name, repo.owner);
        if (score === null) return [];
        return [{
          key: `repo:${repo.full_name}`,
          badge: 'GH',
          badgeKind: 'github' as const,
          title: repo.full_name,
          subtitle: 'Repository',
          section: 'GitHub repos',
          score,
          pick: () => {
            recordPalettePick(`repo:${repo.full_name}`);
            inboxState.pendingRepoNav = {
              owner: repo.owner,
              repo: repo.name,
              section: 'pulls'
            };
            setView('githubTab');
            close();
          }
        }];
      })
    );

    // 6. Jira boards
    push(
      inboxState.jiraBoardOptions.flatMap((b) => {
        const score = scoreFor(`jboard:${b.id}`, b.name, b.project_key);
        if (score === null) return [];
        return [{
          key: `jboard:${b.id}`,
          badge: 'J',
          badgeKind: 'jira' as const,
          title: b.name,
          subtitle: `Board · ${b.project_key ?? 'no project'}`,
          section: 'Jira boards',
          score,
          pick: () => {
            recordPalettePick(`jboard:${b.id}`);
            /* Picking a board / project from the palette lands on the
               Jira tab, so we update the tab's filter slice (not the
               column's — those are independent now). */
            updateJiraTabFilters({ boardIds: [b.id] });
            setView('jiraTab');
            close();
          }
        }];
      })
    );

    // 7. Jira projects
    push(
      inboxState.jiraProjectOptions.flatMap((p) => {
        const score = scoreFor(`jproj:${p.key}`, p.name, p.key);
        if (score === null) return [];
        return [{
          key: `jproj:${p.key}`,
          badge: 'J',
          badgeKind: 'jira' as const,
          title: p.name,
          subtitle: `Project · ${p.key}`,
          section: 'Jira projects',
          score,
          pick: () => {
            recordPalettePick(`jproj:${p.key}`);
            updateJiraTabFilters({ projectKey: p.key });
            setView('jiraTab');
            close();
          }
        }];
      })
    );

    // 8. Sentry projects
    push(
      inboxState.sentryProjectOptions.flatMap((p) => {
        const score = scoreFor(`sproj:${p.slug}`, p.name, p.slug);
        if (score === null) return [];
        return [{
          key: `sproj:${p.slug}`,
          badge: 'St',
          badgeKind: 'sentry' as const,
          title: p.name,
          subtitle: `Sentry project · ${p.slug}`,
          section: 'Sentry projects',
          score,
          pick: () => {
            recordPalettePick(`sproj:${p.slug}`);
            inboxState.sentryTabProjects = [p.slug];
            scheduleSentryTabFilterRefresh();
            setView('sentryTab');
            close();
          }
        }];
      })
    );

    // 9. GitHub PRs/issues — merge across every column instance + dedupe
    //     by id since the same PR can appear in two columns.
    {
      const seen = new Set<number>();
      const merged: InboxItem[] = [];
      for (const list of Object.values(inboxState.itemsByInstance)) {
        for (const it of list) {
          if (seen.has(it.id)) continue;
          seen.add(it.id);
          merged.push(it);
        }
      }
      push(
        merged.flatMap((item) => {
          const score = scoreFor(`gh:${item.id}`, item.title, externalId(item));
          if (score === null) return [];
          return [{
            key: `gh:${item.id}`,
            badge: 'GH',
            badgeKind: 'github' as const,
            title: item.title,
            subtitle: externalId(item),
            section: 'GitHub items',
            score,
            pick: () => {
              recordPalettePick(`gh:${item.id}`);
              selectInboxItem(item.id);
              setView('workbench');
              close();
            }
          }];
        })
      );
    }

    // 10. Jira issues — merge across every column instance + the tab
    //     slice, deduped by id.
    {
      const seen = new Set<string>();
      const merged: JiraItem[] = [];
      for (const list of Object.values(inboxState.jiraItemsByInstance)) {
        for (const it of list) {
          if (seen.has(it.id)) continue;
          seen.add(it.id);
          merged.push(it);
        }
      }
      for (const it of inboxState.jiraTabItems) {
        if (seen.has(it.id)) continue;
        seen.add(it.id);
        merged.push(it);
      }
      push(
        merged.flatMap((item) => {
          const score = scoreFor(`j:${item.id}`, item.summary, item.key);
          if (score === null) return [];
          return [{
            key: `j:${item.id}`,
            badge: 'J',
            badgeKind: 'jira' as const,
            title: item.summary,
            subtitle: item.key,
            section: 'Jira issues',
            score,
            pick: () => {
              recordPalettePick(`j:${item.id}`);
              inboxState.jiraFocusKey = item.key;
              setView('workbench');
              close();
            }
          }];
        })
      );
    }

    // 11. Sentry issues — merge across every column instance + the tab
    //     slice, deduped by id.
    {
      const seen = new Set<string>();
      const merged: SentryIssue[] = [];
      for (const list of Object.values(inboxState.sentryItemsByInstance)) {
        for (const it of list) {
          if (seen.has(it.id)) continue;
          seen.add(it.id);
          merged.push(it);
        }
      }
      for (const it of inboxState.sentryTabItems) {
        if (seen.has(it.id)) continue;
        seen.add(it.id);
        merged.push(it);
      }
      push(
        merged.flatMap((item) => {
          const score = scoreFor(`s:${item.id}`, item.title, item.short_id);
          if (score === null) return [];
          return [{
            key: `s:${item.id}`,
            badge: 'St',
            badgeKind: 'sentry' as const,
            title: item.title,
            subtitle: item.short_id,
            section: 'Sentry issues',
            score,
            pick: () => {
              recordPalettePick(`s:${item.id}`);
              openSentryFocus(item.id);
              setView('workbench');
              close();
            }
          }];
        })
      );
    }

    return r;
  });

  /* Group by section preserving insertion order so the section header
     ordering matches how we built `results`. */
  const grouped = $derived.by(() => {
    const map = new Map<string, Result[]>();
    for (const res of results) {
      const arr = map.get(res.section);
      if (arr) arr.push(res);
      else map.set(res.section, [res]);
    }
    return [...map.entries()];
  });

  function onKey(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      close();
      return;
    }
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      selectedIdx = Math.min(selectedIdx + 1, Math.max(results.length - 1, 0));
      return;
    }
    if (e.key === 'ArrowUp') {
      e.preventDefault();
      selectedIdx = Math.max(selectedIdx - 1, 0);
      return;
    }
    if (e.key === 'Enter') {
      e.preventDefault();
      results[selectedIdx]?.pick();
    }
  }

  /* Reset highlight to first result whenever the query changes — keeps
     the keyboard pick aimed at the most relevant match. */
  $effect(() => {
    query;
    selectedIdx = 0;
  });
</script>

{#if open}
  <div
    class="palette-backdrop"
    onclick={(e) => {
      if (e.target === e.currentTarget) close();
    }}
    onkeydown={onKey}
    role="dialog"
    aria-modal="true"
    aria-label="Command palette"
    tabindex="-1"
    use:focusTrap
  >
    <div class="palette">
      <!-- svelte-ignore a11y_autofocus -->
      <input
        class="palette-input"
        bind:value={query}
        placeholder="Search anywhere — workbenches, columns, repos, boards, issues…"
        autofocus
      />
      {#if results.length === 0}
        <div class="palette-empty">No matches.</div>
      {:else}
        <div class="palette-scroll">
          {#each grouped as [section, items] (section)}
            <div class="palette-section">
              <div class="palette-section-title">{section}</div>
              {#each items as r (r.key)}
                <div
                  class="palette-item"
                  class:highlight={results.indexOf(r) === selectedIdx}
                  onmouseenter={() => (selectedIdx = results.indexOf(r))}
                  role="presentation"
                >
                  <button
                    class="palette-item-main"
                    onclick={r.pick}
                    type="button"
                  >
                    <span class="badge badge--{r.badgeKind}">{r.badge}</span>
                    {#if r.subtitle}
                      <span class="row-id mono">{r.subtitle}</span>
                    {/if}
                    <span class="row-title">{r.title}</span>
                  </button>
                  <button
                    class="palette-pin"
                    class:pinned={isPalettePinned(r.key)}
                    onclick={(e) => { e.stopPropagation(); togglePalettePin(r.key); }}
                    title={isPalettePinned(r.key) ? 'Unpin from top' : 'Pin to top'}
                    aria-label={isPalettePinned(r.key) ? 'Unpin' : 'Pin'}
                    type="button"
                  >★</button>
                </div>
              {/each}
            </div>
          {/each}
        </div>
      {/if}
    </div>
  </div>
{/if}

<style>
  .palette-backdrop {
    position: fixed; inset: 0;
    background: var(--backdrop);
    backdrop-filter: blur(20px);
    display: flex; align-items: flex-start; justify-content: center;
    padding-top: 12vh; z-index: 200;
    animation: fadeIn 180ms ease-out;
  }
  .palette {
    width: 720px; max-width: 92vw;
    max-height: 70vh;
    display: flex; flex-direction: column;
    background: var(--bg-1);
    backdrop-filter: blur(24px);
    border: 1px solid var(--border-hi2); border-radius: 14px;
    overflow: hidden;
    box-shadow: var(--shadow-3), inset 0 1px 0 rgba(255, 255, 255, 0.04);
    animation: slideDown 220ms cubic-bezier(0.34, 1.56, 0.64, 1);
  }
  .palette-input {
    width: 100%; padding: 18px 22px;
    font-size: 15px; color: var(--text-0);
    border-bottom: 1px solid var(--border-neutral);
    background: transparent; border-left: none; border-right: none; border-top: none;
    flex-shrink: 0;
  }
  .palette-input:focus { outline: none; }
  .palette-input::placeholder { color: var(--text-2); }
  .palette-scroll { overflow-y: auto; flex: 1; padding: 4px 0 8px; }
  .palette-section { padding: 4px 10px; }
  .palette-section-title {
    padding: 8px 12px 4px; font-size: 10.5px; font-weight: 600;
    color: var(--text-mute); text-transform: uppercase; letter-spacing: 0.08em;
  }
  .palette-empty { padding: 24px 22px; font-size: 13px; color: var(--text-2); text-align: center; }
  .palette-item {
    display: flex; align-items: stretch;
    border-radius: 7px;
    width: 100%;
    background: none;
    transition: background 100ms ease;
  }
  .palette-item:hover, .palette-item.highlight { background: var(--bg-2); }
  .palette-item.highlight { box-shadow: inset 0 0 0 1px var(--border-hi); }
  .palette-item-main {
    flex: 1; min-width: 0;
    display: flex; align-items: center; gap: 10px;
    padding: 8px 12px;
    text-align: left;
    font-size: 13px; color: var(--text-1); cursor: pointer;
    background: none; border: none;
    border-radius: 7px 0 0 7px;
  }
  .palette-item:hover .palette-item-main, .palette-item.highlight .palette-item-main { color: var(--text-0); }
  .palette-pin {
    width: 32px; flex-shrink: 0;
    background: none; border: none; cursor: pointer;
    color: var(--text-mute); font-size: 14px;
    border-radius: 0 7px 7px 0;
    opacity: 0.35; transition: opacity 120ms;
  }
  .palette-item:hover .palette-pin { opacity: 0.7; }
  .palette-pin:hover { opacity: 1; color: var(--accent-bright); }
  .palette-pin.pinned { opacity: 1; color: var(--accent); }
  .row-id {
    color: var(--text-2); font-size: 11.5px; min-width: 48px; flex-shrink: 0;
  }
  .row-title {
    flex: 1; min-width: 0;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }

  /* Source-themed badges. Each kind gets its own tinted pill so the
     user can scan the result list visually without reading every word. */
  .badge {
    display: inline-flex; align-items: center; justify-content: center;
    min-width: 26px; height: 20px; padding: 0 5px;
    border-radius: 5px; font-weight: 700; font-size: 10px;
    letter-spacing: -0.02em; flex-shrink: 0;
    border: 1px solid transparent;
  }
  .badge--view     { background: rgba(160, 160, 180, 0.10); color: #c8d0e2; border-color: rgba(160, 160, 180, 0.18); }
  .badge--workbench{ background: rgba(232, 163, 58, 0.10); color: #f3c068; border-color: rgba(232, 163, 58, 0.22); }
  .badge--editor   { background: rgba(255, 255, 255, 0.05); color: #e5ebf4; border-color: rgba(255, 255, 255, 0.1); }
  .badge--github   { background: rgba(139, 92, 246, 0.1);  color: #b199f6; border-color: rgba(139, 92, 246, 0.22); }
  .badge--jira     { background: rgba(59, 130, 246, 0.12); color: #60a5fa; border-color: rgba(59, 130, 246, 0.24); }
  .badge--sentry   { background: rgba(248, 143, 116, 0.08); color: #f8a994; border-color: rgba(248, 143, 116, 0.2); }
  .badge--claude   { background: rgba(16, 185, 129, 0.12); color: #34d399; border-color: rgba(16, 185, 129, 0.24); }
  .badge--cursor   { background: rgba(255, 255, 255, 0.05); color: #e5ebf4; border-color: rgba(255, 255, 255, 0.1); }
  .badge--action   { background: rgba(232, 163, 58, 0.15); color: #f3c068; border-color: rgba(232, 163, 58, 0.32); }

  @keyframes fadeIn { from { opacity: 0; } to { opacity: 1; } }
  @keyframes slideDown {
    from { transform: translateY(-10px); opacity: 0; }
    to { transform: translateY(0); opacity: 1; }
  }
</style>
