<script lang="ts">
  /* Spotlight-style universal navigator. Single search box, results
     grouped by section. Filters across every entity the user might
     want to jump to:
       Views — top-level solo views + Rules / Connections / Settings
       Editor — current cwd + linked-session description
       Repos / Boards / Projects — pre-loaded option lists
       Items — currently-loaded GitHub PRs / Jira issues / Sentry errors
     A picked row dispatches the right action (setView, openFocus,
     updateXFilters, …) and closes the palette. */

  import { externalId, type InboxItem, type JiraItem, type SentryIssue } from '$lib/data';
  import {
    inboxState,
    selectInboxItem,
    updateJiraTabFilters,
    openSentryFocus,
    scheduleSentryTabFilterRefresh
  } from '$lib/state/inbox.svelte';
  import { kindForInstanceId } from '$lib/state/layout.svelte';
  import { sessionsState } from '$lib/state/sessions.svelte';
  import { fuzzyScoreAny } from '$lib/services/fuzzyMatch';
  import {
    mruRank,
    recordPalettePick,
    forgetPalettePick,
    paletteMruState,
    type MruPicker,
    type MruSnapshot
  } from '$lib/state/paletteMru.svelte';
  import { isPalettePinned, togglePalettePin, pinnedState } from '$lib/state/pinned.svelte';
  import { focusTrap } from '$lib/actions/focusTrap';
  import type { View } from '$lib/state/view.svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { notify } from '$lib/state/toaster.svelte';

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
    /** Top-level "Actions" rows (connect / disconnect / show cheatsheet
     *  / report bug …). Built by the parent. */
    actions?: PaletteAction[];
    /** Optional mode. `recents` opens directly on the Recent section
     *  with a larger cap and other sections collapsed — bound to ⌘E
     *  for a Cmd-Tab-style quick switcher. Falls through to the full
     *  palette as soon as the user types a query that doesn't match
     *  anything recent. */
    mode?: 'normal' | 'recents';
  }

  let { open = $bindable(), setView, actions = [], mode = $bindable('normal') }: Props = $props();

  let query = $state('');
  let selectedIdx = $state(0);

  /* When opened in `recents` mode, the palette starts collapsed to
   * just the Recent section. If the user types a query that has zero
   * matches in recents, we auto-expand to the full palette so the
   * shortcut never feels like a dead-end. */
  const recentsOnly = $derived(mode === 'recents');

  type Result = {
    key: string;
    badge: string;
    badgeKind: 'view' | 'editor' | 'canvas' | 'github' | 'jira' | 'sentry' | 'claude' | 'cursor' | 'action';
    title: string;
    subtitle?: string;
    section: string;
    /** Fuzzy-match score; -1 means "no query, neutral". Higher
     *  scores rank earlier within their section. */
    score: number;
    pick: () => void;
    /** Extras attached to specific row kinds (e.g. Recent rows carry
     *  the original MRU key so the ✕ "forget" button knows what to
     *  remove, and a timestamp for the "2m ago" hint). */
    meta?: { ts?: number; originalKey?: string };
  };

  function close() {
    open = false;
    query = '';
    selectedIdx = 0;
    /* Always settle back to normal mode on close so a re-open via
     * ⌘K starts unbiased. ⌘E re-applies recents mode itself. */
    mode = 'normal';
  }

  /* Re-execute the navigation captured by a Recent row's picker.
   * Mirrors what each section's pick() does today; the Recent
   * section reuses this so we don't have to keep the original
   * closures alive across restarts. Returns false if the picker
   * couldn't be acted on (e.g. a deleted action) so the caller can
   * surface the dead row. */
  function dispatchPicker(picker: MruPicker): boolean {
    switch (picker.kind) {
      case 'view':
        setView(picker.view);
        return true;
      case 'app-editor':
        setView('editorApp');
        return true;
      case 'repo':
        inboxState.pendingRepoNav = {
          owner: picker.owner,
          repo: picker.repo,
          section: 'pulls'
        };
        setView('githubApp');
        return true;
      case 'jira-board':
        updateJiraTabFilters({ boardIds: [picker.boardId] });
        setView('jiraApp');
        return true;
      case 'jira-project':
        updateJiraTabFilters({ projectKey: picker.projectKey });
        setView('jiraApp');
        return true;
      case 'sentry-project':
        inboxState.sentryTabProjects = [picker.slug];
        scheduleSentryTabFilterRefresh();
        setView('sentryApp');
        return true;
      case 'github-item':
        selectInboxItem(picker.itemId);
        setView('githubApp');
        return true;
      case 'jira-issue':
        inboxState.jiraFocusKey = picker.jiraKey;
        setView('jiraApp');
        return true;
      case 'sentry-issue':
        openSentryFocus(picker.issueId);
        setView('sentryApp');
        return true;
      case 'action': {
        const a = actions.find((x) => x.id === picker.actionId);
        if (!a) return false;
        a.pick();
        return true;
      }
    }
  }

  /* Human-friendly "X ago" for the Recent row's right-edge hint.
   * Bucketed at a coarse resolution — the palette closes / reopens
   * faster than the buckets advance, so we don't bother with a
   * 1Hz refresh. */
  function timeAgo(ts: number): string {
    const diff = Date.now() - ts;
    if (diff < 30_000) return 'just now';
    if (diff < 60_000) return `${Math.floor(diff / 1000)}s`;
    if (diff < 3_600_000) return `${Math.floor(diff / 60_000)}m`;
    if (diff < 86_400_000) return `${Math.floor(diff / 3_600_000)}h`;
    if (diff < 7 * 86_400_000) return `${Math.floor(diff / 86_400_000)}d`;
    return new Date(ts).toLocaleDateString();
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
    { key: 'claudeApp', title: 'Claude', sub: 'Agent · sessions · worktrees' },
    { key: 'cursorApp', title: 'Cursor', sub: 'Agent · sessions · worktrees' },
    { key: 'githubApp', title: 'GitHub', sub: 'Repos / PRs / issues / actions' },
    { key: 'jiraApp', title: 'Jira', sub: 'Tickets / boards / sprints' },
    { key: 'sentryApp', title: 'Sentry', sub: 'Errors / events' },
    { key: 'editorApp', title: 'Editor', sub: 'Code · files · diff' },
    { key: 'canvasApp', title: 'Canvas', sub: 'Whiteboard · agent tools' },
    { key: 'terminalApp', title: 'Terminal', sub: 'Shell · quick commands' },
    { key: 'rules', title: 'Rules', sub: 'Claude system prompts' },
    { key: 'connections', title: 'Connections', sub: 'GitHub / Jira / Sentry / etc.' },
    { key: 'settings', title: 'Settings', sub: '' }
  ];


  function shortFolder(p: string): string {
    const parts = p.split('/').filter(Boolean);
    return parts.length ? parts[parts.length - 1] : p;
  }

  /* Build results derived from query + state. Each section caps at
     LIMIT_PER_SECTION rows so a workspace with hundreds of issues
     doesn't drown the solo / editors / repos that are usually
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

    // 0. Recent — top of the stack in both modes. When the palette
    //    opened via ⌘E we show up to 15 entries (Cmd-Tab feel);
    //    otherwise we keep it short (5) so it doesn't shove the other
    //    sections off-screen. Each entry knows how to re-execute its
    //    nav even when the original source data hasn't loaded yet.
    {
      const rows: Result[] = [];
      const recentCap = recentsOnly ? 15 : 5;
      let idx = 0;
      for (const snap of paletteMruState.entries) {
        if (rows.length >= recentCap) break;
        /* Skip dead `action:` rows so the user doesn't click on a verb
         * whose closure is no longer mounted (e.g. "Disconnect GitHub"
         * after the user disconnects). */
        if (snap.picker.kind === 'action') {
          const actionId = snap.picker.actionId;
          const stillMounted = actions.some((a) => a.id === actionId);
          if (!stillMounted) continue;
        }
        /* When there's a query, fuzzy-match within recents so the
         * Recent section also serves as "recent things matching X" —
         * narrows nicely without losing chronological intent. */
        let score: number;
        if (query.trim()) {
          const f = fuzzyScoreAny(query.trim(), [snap.title, snap.subtitle]);
          if (f === null) continue;
          score = f;
        } else {
          /* No query: score by recency so within-section sort is
           * temporal. The Recent section itself is always pushed
           * first below, so global ordering is "Recent first" regardless
           * of these numbers. */
          score = 1000 - idx;
        }
        rows.push({
          key: `recent:${snap.key}`,
          badge: snap.badge,
          badgeKind: snap.badgeKind,
          title: snap.title,
          subtitle: snap.subtitle,
          section: 'Recent',
          score,
          pick: () => {
            /* Re-bump on click so a re-pick stays on top. */
            recordPalettePick({
              key: snap.key,
              title: snap.title,
              subtitle: snap.subtitle,
              badge: snap.badge,
              badgeKind: snap.badgeKind,
              picker: snap.picker
            });
            const ok = dispatchPicker(snap.picker);
            if (ok) close();
            else forgetPalettePick(snap.key);
          },
          meta: { ts: snap.ts, originalKey: snap.key }
        });
        idx++;
      }
      /* Push directly — the section already enforces its own cap
       * (`recentCap`), and the per-section sort by descending score
       * matters even when the cap is 15 (e.g. when a typed query
       * narrows the set, fuzzy scores beat recency). */
      rows.sort((a, b) => b.score - a.score);
      for (const res of rows) r.push(res);
    }

    /* If the palette was opened in `recents` mode AND we have at least
     * one recent row to show, suppress the rest of the sections — that
     * keeps ⌘E's UI tight and Cmd-Tab-like. Empty results auto-falls
     * through to the full palette so the shortcut never feels broken. */
    const recentHits = r.length;
    if (recentsOnly && recentHits > 0) return r;

    // 1. Actions (top-level verbs — Connect / Disconnect / Open settings /
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
            recordPalettePick({
              key: `action:${a.id}`,
              title: a.label,
              subtitle: a.sub,
              badge: '⚡',
              badgeKind: 'action',
              picker: { kind: 'action', actionId: a.id }
            });
            a.pick();
            close();
          }
        });
      }
      push(rows);
    }

    // 2. Views
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
            recordPalettePick({
              key: `view:${v.key}`,
              title: v.title,
              subtitle: v.sub || undefined,
              badge: '↗',
              badgeKind: 'view',
              picker: { kind: 'view', view: v.key }
            });
            setView(v.key);
            close();
          }
        }];
      })
    );

    // 3. Editor — show folder + link status (singleton)
    {
      const editorId = 'editor-solo';
      const ed = sessionsState.editorInstanceState[editorId];
      const cwd = ed?.repoPath ?? '';
      const folder = cwd ? shortFolder(cwd) : '(empty)';
      const linkedSession = sessionsState.list.find(
        (s) => s.linkedToEditorInstanceId === editorId
      );
      const linkSub = linkedSession
        ? `linked → ${linkedSession.title || linkedSession.id.slice(0, 6)}`
        : 'unlinked';
      const score = scoreFor('view:editor', 'editor', cwd, folder, linkSub);
      if (score !== null) {
        push([{
          key: 'view:editor',
          badge: 'Ed',
          badgeKind: 'editor',
          title: 'Editor',
          subtitle: `${folder} · ${linkSub}`,
          section: 'Apps',
          score,
          pick: () => {
            recordPalettePick({
              key: 'view:editor',
              title: 'Editor',
              subtitle: `${folder} · ${linkSub}`,
              badge: 'Ed',
              badgeKind: 'editor',
              picker: { kind: 'app-editor' }
            });
            setView('editorApp');
            close();
          }
        }]);
      }
    }

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
            recordPalettePick({
              key: `repo:${repo.full_name}`,
              title: repo.full_name,
              subtitle: 'Repository',
              badge: 'GH',
              badgeKind: 'github',
              picker: { kind: 'repo', owner: repo.owner, repo: repo.name }
            });
            inboxState.pendingRepoNav = {
              owner: repo.owner,
              repo: repo.name,
              section: 'pulls'
            };
            setView('githubApp');
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
            recordPalettePick({
              key: `jboard:${b.id}`,
              title: b.name,
              subtitle: `Board · ${b.project_key ?? 'no project'}`,
              badge: 'J',
              badgeKind: 'jira',
              picker: { kind: 'jira-board', boardId: b.id }
            });
            /* Picking a board / project from the palette lands on the
               Jira tab, so we update the tab's filter slice (not the
               column's — those are independent now). */
            updateJiraTabFilters({ boardIds: [b.id] });
            setView('jiraApp');
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
            recordPalettePick({
              key: `jproj:${p.key}`,
              title: p.name,
              subtitle: `Project · ${p.key}`,
              badge: 'J',
              badgeKind: 'jira',
              picker: { kind: 'jira-project', projectKey: p.key }
            });
            updateJiraTabFilters({ projectKey: p.key });
            setView('jiraApp');
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
            recordPalettePick({
              key: `sproj:${p.slug}`,
              title: p.name,
              subtitle: `Sentry project · ${p.slug}`,
              badge: 'St',
              badgeKind: 'sentry',
              picker: { kind: 'sentry-project', slug: p.slug }
            });
            inboxState.sentryTabProjects = [p.slug];
            scheduleSentryTabFilterRefresh();
            setView('sentryApp');
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
              recordPalettePick({
                key: `gh:${item.id}`,
                title: item.title,
                subtitle: externalId(item),
                badge: 'GH',
                badgeKind: 'github',
                picker: { kind: 'github-item', itemId: item.id }
              });
              selectInboxItem(item.id);
              setView('githubApp');
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
              recordPalettePick({
                key: `j:${item.id}`,
                title: item.summary,
                subtitle: item.key,
                badge: 'J',
                badgeKind: 'jira',
                picker: { kind: 'jira-issue', jiraKey: item.key }
              });
              inboxState.jiraFocusKey = item.key;
              setView('jiraApp');
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
              recordPalettePick({
                key: `s:${item.id}`,
                title: item.title,
                subtitle: item.short_id,
                badge: 'St',
                badgeKind: 'sentry',
                picker: { kind: 'sentry-issue', issueId: item.id }
              });
              openSentryFocus(item.id);
              setView('sentryApp');
              close();
            }
          }];
        })
      );
    }

    /* 7. Memory hits — long-term notes that match the query. Async-
       sourced via `memory_search_local`; the `memoryHits` cache fills
       in a tick after typing, so the first paint of these rows lags
       slightly behind the other sections. Subtitle is a normalized
       preview of the content (whitespace collapsed). Pick = preview
       the full content in a toast (the user can copy from there or
       jump to Settings → Memory to delete). */
    if (memoryHits.length > 0 && query.trim().length >= 2) {
      const rows: Result[] = [];
      for (const hit of memoryHits) {
        const preview = hit.content.replace(/\s+/g, ' ').trim().slice(0, 120);
        rows.push({
          key: `mem:${hit.id}`,
          badge: 'M',
          /* Reuse 'action' badge kind — closest visual fit (a neutral
             tone, distinct from any source). Adding a dedicated
             'memory' kind would mean extending Result + paletteMru +
             every badge consumer; not worth it for one section. */
          badgeKind: 'action' as const,
          title: preview,
          subtitle: `#${hit.id} · ${hit.kind}${hit.tags ? ` · ${hit.tags}` : ''}`,
          section: 'Memory',
          /* Score 50 — below recents/actions/views (which use 1000-N
             style) but above fuzzy-matched items so a memory hit
             relevant to the query bubbles up early in the result list. */
          score: 50,
          pick: () => {
            notify({
              kind: 'info',
              title: `Memory #${hit.id} (${hit.kind})`,
              body: hit.content,
              ttlMs: 15000
            });
          }
        });
      }
      push(rows);
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

  /* ---------- Long-term memory in the palette ---------- */
  /* Cached hits for the current query — async memory_search_local
     can't fit into the synchronous `results` derivation, so we keep
     an async-mirrored slice here and append rows from it during the
     derivation. Cache is keyed by the query string so a stale call
     completing after the user typed more doesn't overwrite fresher
     results. Min query length 2 — single chars would spam the DB
     and FTS5 with low-value matches. */
  interface MemoryHit {
    id: number;
    kind: string;
    content: string;
    tags: string;
    created_at: number;
  }
  let memoryHits = $state<MemoryHit[]>([]);
  let memoryHitsForQuery = $state<string>('');
  let memoryDebounce: ReturnType<typeof setTimeout> | null = null;

  $effect(() => {
    const q = query.trim();
    if (q === memoryHitsForQuery) return;
    if (q.length < 2) {
      memoryHits = [];
      memoryHitsForQuery = q;
      return;
    }
    if (memoryDebounce) clearTimeout(memoryDebounce);
    memoryDebounce = setTimeout(async () => {
      try {
        const hits = await invoke<MemoryHit[]>('memory_search_local', {
          query: q,
          limit: 5
        });
        /* Stale check: by the time the call returns the user may
           have typed more / cleared the box. Drop the result if it
           doesn't match what's in the input now. */
        if (q === query.trim()) {
          memoryHits = hits;
          memoryHitsForQuery = q;
        }
      } catch {
        if (q === query.trim()) {
          memoryHits = [];
          memoryHitsForQuery = q;
        }
      }
    }, 200);
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
        placeholder={recentsOnly
          ? 'Jump to recent — type to filter, ↵ to open'
          : 'Search anywhere — solos, repos, boards, issues…'}
        autofocus
      />
      {#if results.length === 0}
        <div class="palette-empty">No matches.</div>
      {/if}
      {#if results.length > 0}
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
                    {#if r.meta?.ts}
                      <span class="row-ts mono">{timeAgo(r.meta.ts)}</span>
                    {/if}
                  </button>
                  {#if r.meta?.originalKey}
                    <button
                      class="palette-pin"
                      onclick={(e) => {
                        e.stopPropagation();
                        forgetPalettePick(r.meta!.originalKey!);
                      }}
                      title="Forget this entry"
                      aria-label="Forget this entry"
                      type="button"
                    >✕</button>
                  {:else}
                    <button
                      class="palette-pin"
                      class:pinned={isPalettePinned(r.key)}
                      onclick={(e) => { e.stopPropagation(); togglePalettePin(r.key); }}
                      title={isPalettePinned(r.key) ? 'Unpin from top' : 'Pin to top'}
                      aria-label={isPalettePinned(r.key) ? 'Unpin' : 'Pin'}
                      type="button"
                    >★</button>
                  {/if}
                </div>
              {/each}
            </div>
          {/each}
        </div>
      {/if}

      <div class="palette-foot">
        <span class="grp">
          <span class="kbd">↑</span><span class="kbd">↓</span>
          <span>navigate</span>
        </span>
        <span class="grp">
          <span class="kbd">⏎</span>
          <span>open</span>
        </span>
        <span class="grp">
          <span class="kbd">⌘</span><span class="kbd">D</span>
          <span>pin</span>
        </span>
        <span class="grp" style="margin-left: auto;">
          <span class="kbd">esc</span>
          <span>close</span>
        </span>
      </div>
    </div>
  </div>
{/if}

<style>
  /* v6 command palette — Spotlight-class. Warm-noir surface, blurred
     backdrop, soft entrance, brand-coded source badges. Highlight
     row gets a clay accent stripe + brand soft halo. */
  .palette-backdrop {
    position: fixed; inset: 0;
    background: var(--backdrop);
    backdrop-filter: blur(22px) saturate(1.1);
    -webkit-backdrop-filter: blur(22px) saturate(1.1);
    display: flex; align-items: flex-start; justify-content: center;
    padding-top: 12vh; z-index: 200;
    animation: fadeIn var(--dur-base) var(--ease-out);
  }
  .palette {
    width: 720px; max-width: 92vw;
    max-height: 70vh;
    display: flex; flex-direction: column;
    background: linear-gradient(180deg, var(--bg-2), var(--bg-1));
    border: 1px solid var(--border-hi);
    border-radius: var(--r-modal, 16px);
    overflow: hidden;
    box-shadow:
      var(--shadow-3),
      0 0 0 1px var(--border-accent-2),
      inset 0 1px 0 rgba(255, 240, 220, 0.04);
    animation: slideDown var(--dur-slow) var(--ease-spring);
  }
  .palette-input {
    width: 100%; padding: 18px 22px;
    font-size: 15px; color: var(--text-0);
    border-bottom: 1px solid var(--border);
    background: transparent; border-left: none; border-right: none; border-top: none;
    flex-shrink: 0;
    letter-spacing: -0.005em;
  }
  .palette-input:focus { outline: none; }
  .palette-input::placeholder { color: var(--text-mute); }
  .palette-scroll { overflow-y: auto; flex: 1; padding: 4px 0 10px; }
  .palette-section { padding: 4px 10px; }
  .palette-section-title {
    padding: 12px 14px 6px;
    font-family: 'JetBrains Mono', ui-monospace, monospace;
    font-size: 10px; font-weight: 600;
    color: var(--text-mute); text-transform: uppercase; letter-spacing: 0.10em;
  }
  .palette-empty { padding: 28px 22px; font-size: 13px; color: var(--text-2); text-align: center; }

  .palette-item {
    position: relative;
    display: flex; align-items: stretch;
    border-radius: 8px;
    width: 100%;
    background: none;
    transition: background 100ms ease;
  }
  .palette-item:hover { background: var(--bg-2); }
  .palette-item.highlight {
    background: linear-gradient(90deg,
      color-mix(in srgb, var(--accent) 10%, transparent),
      color-mix(in srgb, var(--accent) 2%, transparent) 60%);
    box-shadow: inset 0 0 0 1px var(--border-accent-2);
  }
  .palette-item.highlight::before {
    content: '';
    position: absolute; left: 0; top: 6px; bottom: 6px;
    width: 2.5px; border-radius: 2px;
    background: var(--accent);
    box-shadow: 0 0 10px var(--accent-glow);
  }
  .palette-item-main {
    flex: 1; min-width: 0;
    display: flex; align-items: center; gap: 12px;
    padding: 9px 14px;
    text-align: left;
    font-size: 13px; color: var(--text-1); cursor: pointer;
    background: none; border: none;
    border-radius: 8px 0 0 8px;
  }
  .palette-item:hover .palette-item-main, .palette-item.highlight .palette-item-main { color: var(--text-0); }
  .palette-pin {
    width: 32px; flex-shrink: 0;
    background: none; border: none; cursor: pointer;
    color: var(--text-mute); font-size: 14px;
    border-radius: 0 8px 8px 0;
    opacity: 0; transition: opacity 120ms, color 120ms;
  }
  .palette-item:hover .palette-pin { opacity: 0.7; }
  .palette-pin:hover { opacity: 1; color: var(--accent-bright); }
  .palette-pin.pinned { opacity: 1; color: var(--accent); }
  .row-id {
    font-family: 'JetBrains Mono', monospace; font-feature-settings: 'tnum';
    color: var(--text-mute); font-size: 11px; min-width: 48px; flex-shrink: 0;
  }
  .row-title {
    flex: 1; min-width: 0;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
    color: var(--text-0);
  }
  /* Trailing "X ago" hint on Recent rows — quiet, monospaced so the
     column edge stays tidy across multiple rows of varying length. */
  .row-ts {
    flex-shrink: 0;
    font-family: 'JetBrains Mono', monospace;
    font-feature-settings: 'tnum';
    font-size: 10.5px;
    color: var(--text-mute);
    opacity: 0.7;
  }
  .palette-item.highlight .row-ts { opacity: 1; }

  /* Source-themed badges — share the rest of the palette's --src-*
     family so the palette and column stripes feel like the same
     design system. */
  .badge {
    display: inline-flex; align-items: center; justify-content: center;
    min-width: 26px; height: 20px; padding: 0 6px;
    border-radius: 5px; font-weight: 700; font-size: 10px;
    letter-spacing: -0.02em; flex-shrink: 0;
    border: 1px solid transparent;
    font-family: 'JetBrains Mono', monospace;
  }
  .badge--view     { background: var(--bg-3); color: var(--text-1); border-color: var(--border); }
  .badge--editor   { background: var(--accent-soft); color: var(--src-editor); border-color: var(--border-accent-2); }
  .badge--github   { background: rgba(181, 132, 255, 0.10); color: var(--src-github);  border-color: rgba(181, 132, 255, 0.26); }
  .badge--jira     { background: rgba(79, 142, 255, 0.10);  color: var(--src-jira);    border-color: rgba(79, 142, 255, 0.26); }
  .badge--sentry   { background: rgba(232, 130, 100, 0.10); color: var(--src-sentry);  border-color: rgba(232, 130, 100, 0.26); }
  .badge--claude   { background: rgba(232, 155, 125, 0.10); color: var(--src-claude);  border-color: rgba(232, 155, 125, 0.26); }
  .badge--cursor   { background: var(--bg-3); color: var(--src-cursor); border-color: var(--border-hi); }
  .badge--action   { background: var(--accent-soft); color: var(--accent-bright); border-color: var(--border-accent); }

  /* v7 — footer kbd hints. */
  .palette-foot {
    padding: 10px 18px;
    border-top: 1px solid var(--border);
    display: flex; align-items: center; gap: 14px;
    font-size: 11px;
    color: var(--text-mute);
    background: var(--bg-2);
    flex-shrink: 0;
  }
  .palette-foot .grp { display: flex; align-items: center; gap: 5px; }
  .palette-foot .kbd {
    display: inline-grid; place-items: center;
    height: 16px; min-width: 16px;
    padding: 0 4px;
    font-family: 'JetBrains Mono', monospace;
    font-size: 9.5px;
    background: var(--bg-3);
    border: 1px solid var(--border);
    border-radius: 4px;
    color: var(--text-1);
  }

  @keyframes fadeIn { from { opacity: 0; } to { opacity: 1; } }
  @keyframes slideDown {
    from { transform: translateY(-10px); opacity: 0; }
    to { transform: translateY(0); opacity: 1; }
  }
</style>
