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

  import { externalId } from '$lib/data';
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
  import type { View } from '$lib/state/view.svelte';

  interface Props {
    open: boolean;
    /** `view` is a local $state in +page.svelte, so the palette can't
     *  flip it directly via setView() from view.svelte.ts (that store
     *  is unused there). Parent passes a callback. */
    setView: (v: View) => void;
  }

  let { open = $bindable(), setView }: Props = $props();

  let query = $state('');
  let selectedIdx = $state(0);

  type Result = {
    key: string;
    badge: string;
    badgeKind: 'view' | 'workbench' | 'editor' | 'github' | 'jira' | 'sentry' | 'claude' | 'cursor';
    title: string;
    subtitle?: string;
    section: string;
    pick: () => void;
  };

  function close() {
    open = false;
    query = '';
    selectedIdx = 0;
  }

  function matches(...fields: (string | null | undefined)[]): boolean {
    const q = query.trim().toLowerCase();
    if (!q) return true;
    return fields.some((f) => f && f.toLowerCase().includes(q));
  }

  const VIEWS: { key: View; title: string; sub: string }[] = [
    { key: 'workbench', title: 'Workbench', sub: 'Active columns' },
    { key: 'githubTab', title: 'GitHub', sub: 'Repositories' },
    { key: 'jiraTab', title: 'Jira', sub: 'Issues / boards / projects' },
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
    editor: 'Ed'
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
     what the user actually wants to reach via Cmd+K. */
  const LIMIT_PER_SECTION = 6;

  const results = $derived.by((): Result[] => {
    const r: Result[] = [];
    const push = (sectionResults: Result[]) => {
      for (const res of sectionResults.slice(0, LIMIT_PER_SECTION)) r.push(res);
    };

    // 1. Views
    push(
      VIEWS.filter((v) => matches(v.title, v.sub, v.key)).map((v) => ({
        key: `view:${v.key}`,
        badge: '↗',
        badgeKind: 'view' as const,
        title: v.title,
        subtitle: v.sub || undefined,
        section: 'Views',
        pick: () => {
          setView(v.key);
          close();
        }
      }))
    );

    // 2. Workbenches
    push(
      layoutState.workbenches
        .filter((wb) => matches(wb.name))
        .map((wb) => ({
          key: `wb:${wb.id}`,
          badge: 'WB',
          badgeKind: 'workbench' as const,
          title: wb.name,
          subtitle: `${wb.instances.length} column${wb.instances.length === 1 ? '' : 's'}`,
          section: 'Workbenches',
          pick: () => {
            setActiveWorkbench(wb.id);
            setView('workbench');
            close();
          }
        }))
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
        if (!matches(inst.name, cwd, folder, linkSub, wb.name)) continue;
        editorRows.push({
          key: `inst:${inst.id}`,
          badge: 'Ed',
          badgeKind: 'editor',
          title: `Editor · ${inst.name}`,
          subtitle: `${folder} · ${linkSub} · ${wb.name}`,
          section: 'Editors',
          pick: () => {
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
        if (!matches(inst.name, inst.kind, wb.name)) continue;
        colRows.push({
          key: `inst:${inst.id}`,
          badge: KIND_BADGE[inst.kind] ?? '?',
          badgeKind: inst.kind as Result['badgeKind'],
          title: `${capitalize(inst.kind)} · ${inst.name}`,
          subtitle: wb.name,
          section: 'Columns',
          pick: () => {
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
      inboxState.githubRepoOptions
        .filter((repo) => matches(repo.full_name, repo.name, repo.owner))
        .map((repo) => ({
          key: `repo:${repo.full_name}`,
          badge: 'GH',
          badgeKind: 'github' as const,
          title: repo.full_name,
          subtitle: 'Repository',
          section: 'GitHub repos',
          pick: () => {
            inboxState.pendingRepoNav = {
              owner: repo.owner,
              repo: repo.name,
              section: 'pulls'
            };
            setView('githubTab');
            close();
          }
        }))
    );

    // 6. Jira boards
    push(
      inboxState.jiraBoardOptions
        .filter((b) => matches(b.name, b.project_key))
        .map((b) => ({
          key: `jboard:${b.id}`,
          badge: 'J',
          badgeKind: 'jira' as const,
          title: b.name,
          subtitle: `Board · ${b.project_key ?? 'no project'}`,
          section: 'Jira boards',
          pick: () => {
            /* Picking a board / project from the palette lands on the
               Tasks tab, so we update the tab's filter slice (not the
               column's — those are independent now). */
            updateJiraTabFilters({ boardIds: [b.id] });
            setView('jiraTab');
            close();
          }
        }))
    );

    // 7. Jira projects
    push(
      inboxState.jiraProjectOptions
        .filter((p) => matches(p.name, p.key))
        .map((p) => ({
          key: `jproj:${p.key}`,
          badge: 'J',
          badgeKind: 'jira' as const,
          title: p.name,
          subtitle: `Project · ${p.key}`,
          section: 'Jira projects',
          pick: () => {
            updateJiraTabFilters({ projectKey: p.key });
            setView('jiraTab');
            close();
          }
        }))
    );

    // 8. Sentry projects
    push(
      inboxState.sentryProjectOptions
        .filter((p) => matches(p.name, p.slug))
        .map((p) => ({
          key: `sproj:${p.slug}`,
          badge: 'St',
          badgeKind: 'sentry' as const,
          title: p.name,
          subtitle: `Sentry project · ${p.slug}`,
          section: 'Sentry projects',
          pick: () => {
            inboxState.sentryTabProjects = [p.slug];
            scheduleSentryTabFilterRefresh();
            setView('sentryTab');
            close();
          }
        }))
    );

    // 9. GitHub PRs/issues (loaded inbox)
    push(
      inboxState.items
        .filter((item) => matches(item.title, externalId(item)))
        .map((item) => ({
          key: `gh:${item.id}`,
          badge: 'GH',
          badgeKind: 'github' as const,
          title: item.title,
          subtitle: externalId(item),
          section: 'GitHub items',
          pick: () => {
            selectInboxItem(item.id);
            setView('workbench');
            close();
          }
        }))
    );

    // 10. Jira issues — searches column items + tasks-tab items
    //     (independent slices since the recent decoupling), deduped
    //     by id so an issue loaded by both doesn't appear twice.
    {
      const seen = new Set<string>();
      const merged: typeof inboxState.jiraItems = [];
      for (const it of [...inboxState.jiraItems, ...inboxState.jiraTabItems]) {
        if (seen.has(it.id)) continue;
        seen.add(it.id);
        merged.push(it);
      }
      push(
        merged
          .filter((item) => matches(item.summary, item.key))
          .map((item) => ({
            key: `j:${item.id}`,
            badge: 'J',
            badgeKind: 'jira' as const,
            title: item.summary,
            subtitle: item.key,
            section: 'Jira issues',
            pick: () => {
              inboxState.jiraFocusKey = item.key;
              setView('workbench');
              close();
            }
          }))
      );
    }

    // 11. Sentry issues — same column + issues-tab merge.
    {
      const seen = new Set<string>();
      const merged: typeof inboxState.sentryItems = [];
      for (const it of [...inboxState.sentryItems, ...inboxState.sentryTabItems]) {
        if (seen.has(it.id)) continue;
        seen.add(it.id);
        merged.push(it);
      }
      push(
        merged
          .filter((item) => matches(item.title, item.short_id))
          .map((item) => ({
            key: `s:${item.id}`,
            badge: 'St',
            badgeKind: 'sentry' as const,
            title: item.title,
            subtitle: item.short_id,
            section: 'Sentry issues',
            pick: () => {
              openSentryFocus(item.id);
              setView('workbench');
              close();
            }
          }))
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
    tabindex="-1"
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
                <button
                  class="palette-item"
                  class:highlight={results.indexOf(r) === selectedIdx}
                  onclick={r.pick}
                  onmouseenter={() => (selectedIdx = results.indexOf(r))}
                >
                  <span class="badge badge--{r.badgeKind}">{r.badge}</span>
                  {#if r.subtitle}
                    <span class="row-id mono">{r.subtitle}</span>
                  {/if}
                  <span class="row-title">{r.title}</span>
                </button>
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
    background: rgba(10, 17, 30, 0.78);
    backdrop-filter: blur(20px);
    display: flex; align-items: flex-start; justify-content: center;
    padding-top: 12vh; z-index: 200;
    animation: fadeIn 180ms ease-out;
  }
  .palette {
    width: 720px; max-width: 92vw;
    max-height: 70vh;
    display: flex; flex-direction: column;
    background: rgba(15, 24, 40, 0.94);
    backdrop-filter: blur(24px);
    border: 1px solid var(--border-hi2); border-radius: 14px;
    overflow: hidden;
    box-shadow: 0 30px 80px rgba(0, 0, 0, 0.6), inset 0 1px 0 rgba(255, 255, 255, 0.04);
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
    display: flex; align-items: center; gap: 10px;
    padding: 8px 12px; border-radius: 7px;
    width: 100%; text-align: left;
    font-size: 13px; color: var(--text-1); cursor: pointer;
    background: none; border: none;
  }
  .palette-item:hover, .palette-item.highlight { background: var(--bg-2); color: var(--text-0); }
  .palette-item.highlight { box-shadow: inset 0 0 0 1px var(--border-hi); }
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

  @keyframes fadeIn { from { opacity: 0; } to { opacity: 1; } }
  @keyframes slideDown {
    from { transform: translateY(-10px); opacity: 0; }
    to { transform: translateY(0); opacity: 1; }
  }
</style>
