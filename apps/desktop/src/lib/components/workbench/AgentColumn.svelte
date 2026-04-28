<script lang="ts">
  import { tick } from 'svelte';
  import { slide } from 'svelte/transition';
  import { cubicOut } from 'svelte/easing';
  import Markdown from '$lib/components/ui/Markdown.svelte';
  import ClaudeActionCard from '$lib/components/workbench/ClaudeActionCard.svelte';
  import EditDiffCard from '$lib/components/workbench/EditDiffCard.svelte';
  import Dropdown, { type DropdownOption } from '$lib/components/ui/Dropdown.svelte';
  import {
    cacheHitRate,
    contextPct,
    contextWindowFor,
    costForUsage,
    formatCostUsd,
    formatTokens
  } from '$lib/usage';
  import { quotaState, formatResetsIn, refreshPlanUsage } from '$lib/state/quota.svelte';
  import { invoke, convertFileSrc } from '@tauri-apps/api/core';
  import { open as openDialog } from '@tauri-apps/plugin-dialog';
  import { inboxState } from '$lib/state/inbox.svelte';
  import type { Mention } from '$lib/types';
  import {
    connectionsMeta,
    relativeTime,
    type ClaudeStatus,
    type ConnectionStatus,
    type CursorStatus
  } from '$lib/data';
  const claudeMeta = connectionsMeta.find((c) => c.id === 'claude')!;
  const cursorMeta = connectionsMeta.find((c) => c.id === 'cursor')!;
  import { shortPath, shortenFsPath, shortRemote, isImagePath } from '$lib/format';
  import {
    sessionsState,
    updateSession,
    attachPathsToSession,
    sessionsForInstance,
    activeSessionInInstance,
    setActiveSessionInColumn,
    getPendingEditEvents,
    pruneMentionsByInput
  } from '$lib/state/sessions.svelte';
  import {
    revertAllPendingEdits,
    keepAllPendingEdits
  } from '$lib/services/diffActions';
  import { notify, notifyError } from '$lib/state/toaster.svelte';
  import {
    layoutState,
    startResizeById,
    activeInstances,
    findInstanceAnywhere
  } from '$lib/state/layout.svelte';
  import ColumnControls from '$lib/components/workbench/ColumnControls.svelte';
  import type { ClaudeAction, RepoInfo } from '$lib/types';

  type Kind = 'claude' | 'cursor';

  interface Props {
    kind: Kind;
    instanceId: string;
    claudeStatus: ClaudeStatus | null;
    cursorStatus: CursorStatus | null;
    githubStatus: ConnectionStatus;
    editorRepoPath: string;
    activeRepoInfo: RepoInfo | null;
    dragOverInstanceId: string | null;
    worktreeBusy: 'creating' | 'removing' | null;
    worktreeMenuOpen: boolean;
    editingMsg: { sessionId: string; index: number; draft: string } | null;
    thinkingStartedAt: number | null;
    thinkingTick: number;
    now: number;
    // Callbacks
    onAgentDragEnter: (instanceId: string, kind: Kind, e: DragEvent) => void;
    onAgentDragOver: (instanceId: string, kind: Kind, e: DragEvent) => void;
    onAgentDragLeave: (instanceId: string) => void;
    onAgentDrop: (instanceId: string, kind: Kind, e: DragEvent) => void;
    onPickCwd: () => void;
    onClearCwd: () => void;
    onOpenSessionFolderInEditor: () => void;
    onToggleEditorLink: () => void;
    onLinkToEditorInstance: (editorInstanceId: string) => void;
    onCreateWorktree: () => void;
    onToggleWorktreeMenu: () => void;
    onOpenWorktreeDiff: () => void;
    onOpenWorktreeInEditor: () => void;
    onCopyWorktreeBranch: () => void;
    onApplyWorktree: () => void;
    onRemoveWorktree: () => void;
    onUpdateSessionCursorModel: (sessionId: string, model: string | null) => void;
    onUpdateSessionClaudeModel: (sessionId: string, model: string | null) => void;
    onUpdateSessionClaudeToolProfile: (
      sessionId: string,
      profile: 'all' | 'coding' | 'github' | 'jira' | 'sentry' | 'triage' | null
    ) => void;
    /** Fork-compact: ask the live CLI session to summarise itself, mint
     *  a fresh session UUID, seed it with the summary, swap the
     *  session's UUID over. Resolves when the new session is ready
     *  for the user's next normal turn. Errors bubble up so the
     *  caller can toast. */
    onCompactSession: (sessionId: string) => Promise<void>;
    onDeleteClaudeSession: (id: string) => void;
    onNewClaudeSession: (opts: { agentKind: Kind; columnInstanceId: string }) => void;
    onStartEditMessage: (sessionId: string, index: number, content: string) => void;
    onResendMessage: (sessionId: string, index: number, content: string) => void;
    onCancelEditMessage: () => void;
    onCommitEditMessage: () => void;
    onSetEditingMsgDraft: (draft: string) => void;
    onUpdateAction: (sessionId: string, actionId: string, patch: Partial<ClaudeAction>) => void;
    onRemoveAction: (sessionId: string, actionId: string) => void;
    onExecuteAction: (sessionId: string, action: ClaudeAction) => void;
    onOpenPrInForgehold: (url: string, action: (ClaudeAction & { kind: 'pr' }) | null) => void;
    onSetSessionInput: (sessionId: string, input: string) => void;
    onSendClaudeMessage: () => void;
    onStopClaude: () => void;
    /** Click on a @file/@dir mention in the rendered markdown — parent
        resolves the path against the session's cwd/worktree/editor and
        opens it in the Editor column. */
    onOpenMentionPath: (path: string) => void;
    /** Cmd+V of an image in the composer — parent saves bytes to disk and
        attaches them to the active session. Returns the count attached so
        the column can decide whether to flash a hint. */
    onPasteImages: (instanceId: string, kind: Kind, blobs: { name: string; type: string; blob: Blob }[]) => Promise<number>;
  }

  let {
    kind,
    instanceId,
    claudeStatus,
    cursorStatus,
    githubStatus,
    editorRepoPath,
    activeRepoInfo,
    dragOverInstanceId,
    worktreeBusy,
    worktreeMenuOpen,
    editingMsg,
    thinkingStartedAt,
    thinkingTick,
    now,
    onAgentDragEnter,
    onAgentDragOver,
    onAgentDragLeave,
    onAgentDrop,
    onPickCwd,
    onClearCwd,
    onOpenSessionFolderInEditor,
    onToggleEditorLink,
    onLinkToEditorInstance,
    onCreateWorktree,
    onToggleWorktreeMenu,
    onOpenWorktreeDiff,
    onOpenWorktreeInEditor,
    onCopyWorktreeBranch,
    onApplyWorktree,
    onRemoveWorktree,
    onUpdateSessionCursorModel,
    onUpdateSessionClaudeModel,
    onUpdateSessionClaudeToolProfile,
    onCompactSession,
    onDeleteClaudeSession,
    onNewClaudeSession,
    onStartEditMessage,
    onResendMessage,
    onCancelEditMessage,
    onCommitEditMessage,
    onSetEditingMsgDraft,
    onUpdateAction,
    onRemoveAction,
    onExecuteAction,
    onOpenPrInForgehold,
    onSetSessionInput,
    onSendClaudeMessage,
    onStopClaude,
    onOpenMentionPath,
    onPasteImages
  }: Props = $props();

  const brandLabel = $derived(kind === 'claude' ? 'Claude Code' : 'Cursor');
  const brandInitial = $derived(kind === 'claude' ? 'C' : 'Cr');
  const brandVersion = $derived(kind === 'claude' ? claudeStatus?.version : cursorStatus?.version);
  // First instance of its kind in the workbench — adopts orphaned/floating
  // sessions so pre-v2 persisted sessions surface somewhere.
  const isFirstOfKind = $derived(
    activeInstances().find((i) => i.kind === kind)?.id === instanceId
  );
  const kindSessions = $derived(sessionsForInstance(instanceId, kind, isFirstOfKind));
  const activeSess = $derived(activeSessionInInstance(instanceId, kind, isFirstOfKind));
  const dragOver = $derived(dragOverInstanceId === instanceId);

  // Per-session compacting flag (one fork-compact in flight at a time).
  // Local because the operation finishes in <30s and component scope
  // is enough; persisting across reloads or scoping to the global
  // session store would only complicate the cancel/abort story.
  let compactingId: string | null = $state(null);

  async function runCompact() {
    const s = activeSess;
    if (!s || compactingId === s.id) return;
    if (s.messages.length < 4) {
      // Nothing meaningful to compact yet — skip silently.
      return;
    }
    compactingId = s.id;
    try {
      await onCompactSession(s.id);
    } catch (e) {
      // Parent already toasted (it has access to the toast store).
      // Still log so the dev console keeps a record.
      console.warn('[compact] failed:', e);
    } finally {
      if (compactingId === s.id) compactingId = null;
    }
  }

  // Context-ring + cumulative-cost summaries for the chip in the column
  // header. Both update reactively whenever a turn lands a new usage
  // snapshot. ctxRatio uses the LAST turn's context size (because that
  // reflects how full the window actually is right now); cumCostUsd is
  // a running sum across every turn in this session.
  //
  // Effective model = claudeModel for claude sessions, cursorModel for
  // cursor sessions. Used to size the context window (200k Sonnet, 1M
  // Opus, 200k default for cursor's composer/gpt models which we don't
  // have a precise mapping for).
  const effectiveModel = $derived(
    activeSess
      ? activeSess.agentKind === 'claude' ? activeSess.claudeModel : activeSess.cursorModel
      : null
  );
  const ctxRatio = $derived(
    activeSess && activeSess.lastContextSize > 0
      ? Math.min(
          1,
          activeSess.lastContextSize /
            contextWindowFor(effectiveModel, activeSess.agentKind)
        )
      : 0
  );
  const cumCostUsd = $derived(
    activeSess
      ? activeSess.messages.reduce(
          (sum, m) => (m.usage ? sum + costForUsage(m.usage) : sum),
          0
        )
      : 0
  );

  // Diff cards in `applied` status — the agent's change is on disk
  // but the user hasn't decided yet (Keep would flip them to `kept`,
  // Revert to `reverted`). Drives the bulk-action bar above the
  // composer. Recomputes on every sessionsState change because
  // `getPendingEditEvents` walks `sessionsState.list` and Svelte 5's
  // reactivity tracks that read.
  const pendingEdits = $derived(
    activeSess ? getPendingEditEvents(activeSess.id) : []
  );
  let bulkActionBusy = $state(false);

  /** Revert every pending edit for the active session, newest-first.
   *  Disabled while in flight so the user can't re-trigger before the
   *  Tauri round-trips finish; failures bubble up as a single toast
   *  instead of N (per-event errors are already on the cards). */
  async function handleRevertAllPending() {
    if (!activeSess || bulkActionBusy) return;
    const sid = activeSess.id;
    bulkActionBusy = true;
    try {
      const r = await revertAllPendingEdits(sid);
      if (r.failed > 0) {
        notify({
          kind: 'warning',
          title: `Reverted ${r.reverted} of ${r.total}`,
          body: `${r.failed} change${r.failed === 1 ? '' : 's'} couldn't be reverted — see the cards above for details.`
        });
      } else if (r.reverted > 0) {
        notify({
          kind: 'success',
          title: `Reverted ${r.reverted} change${r.reverted === 1 ? '' : 's'}`
        });
      }
    } catch (e) {
      notifyError(e, { title: 'Revert all failed' });
    } finally {
      bulkActionBusy = false;
    }
  }

  /** Flip every pending edit to `kept` in one go. Doesn't touch disk
   *  (the agent's writes are already there); only the cards' UI
   *  changes — pill turns to "kept", buttons collapse to a single
   *  "Unkeep" affordance, and the bar's count drops to zero. */
  function handleKeepAllPending() {
    if (!activeSess || bulkActionBusy) return;
    keepAllPendingEdits(activeSess.id);
  }

  // Plan-usage chip values. The 5-hour rolling bucket is the
  // "headline" number — that's the limit that resets fastest and
  // burns first when the user spams turns, so it's most actionable
  // for a glance. Other buckets surface in the popover + (when over
  // the warn threshold) we color the chip.
  const planFiveHour = $derived(quotaState.usage?.five_hour?.utilization ?? null);
  const planSevenDay = $derived(quotaState.usage?.seven_day?.utilization ?? null);
  // Color by the worst of the two — long-tail weekly hit feels
  // identical to the user as a near-term 5h hit, both cap turns.
  const planRingClass = $derived.by(() => {
    const worst = Math.max(planFiveHour ?? 0, planSevenDay ?? 0);
    if (worst >= 85) return 'plan-chip plan-chip--alert';
    if (worst >= 60) return 'plan-chip plan-chip--warn';
    return 'plan-chip';
  });
  // Plan-usage popover open/close. Local to the active column —
  // each AgentColumn instance has its own toggle since the chip
  // lives in each column's header.
  let planPopoverOpen = $state(false);
  /** Pick the bar color by utilisation. Mirrors how Claude Code's
   *  /usage panel paints: blue under 60%, yellow 60-85, red over
   *  85. Same thresholds as the chip border so the bar inside the
   *  popover matches the outer chip's vibe. */
  function planBarColor(pct: number | null): string {
    if (pct == null || pct < 60) return 'var(--accent-bright)';
    if (pct < 85) return '#d18a4a';
    return '#c75a4a';
  }

  // Per-message expansion of the "Thinking" pill. Keyed by `${sessId}:${idx}`
  // so two sessions in the same column don't share state. Default = collapsed
  // (only show the badge); user clicks to read the chain-of-thought.
  let expandedThinking = $state(new Set<string>());
  function toggleThinkingExpansion(key: string) {
    const next = new Set(expandedThinking);
    if (next.has(key)) next.delete(key);
    else next.add(key);
    expandedThinking = next;
  }
  // Same shape, separate Set for the "✓ N steps" trace pill (tool-use
  // hints). Independent so a user can expand thinking without auto-
  // expanding trace and vice versa.
  let expandedTrace = $state(new Set<string>());
  function toggleTraceExpansion(key: string) {
    const next = new Set(expandedTrace);
    if (next.has(key)) next.delete(key);
    else next.add(key);
    expandedTrace = next;
  }

  // Snap the chat scroll to the bottom whenever the active session changes
  // (workbench switch, app reopen, user picks a different chat from the
  // dropdown, agent column re-mounts). Without this the column lands at
  // the top of the message list, which on long conversations means the
  // user has to scroll past N old messages every time. Watching just
  // `activeSess.id` keeps mid-stream auto-scroll (which is handled by
  // appendAssistantDelta on the page side) untouched.
  let lastScrolledSessionId: string | null = null;
  $effect(() => {
    const sid = activeSess?.id ?? null;
    if (!sid || sid === lastScrolledSessionId) return;
    lastScrolledSessionId = sid;
    void (async () => {
      await tick();
      const el = sessionsState.scrollEls[instanceId];
      if (el) el.scrollTop = el.scrollHeight;
    })();
  });
  const inst = $derived(activeInstances().find((i) => i.id === instanceId));
  const order = $derived(activeInstances().findIndex((i) => i.id === instanceId));

  /** Editor instance this session is linked to (for the linked pill label).
      Null only when the link target was actually closed; surviving across
      workbench moves via `findInstanceAnywhere`. */
  const linkedEditor = $derived.by(() => {
    const boundId = activeSess?.linkedToEditorInstanceId;
    if (!boundId) return null;
    const found = findInstanceAnywhere(boundId);
    if (!found || found.inst.kind !== 'editor') return null;
    return found.inst;
  });
  /** All Editor instances in the current workbench — used by the link dropdown
      when user wants to pick a specific one. */
  const editorInstances = $derived(activeInstances().filter((i) => i.kind === 'editor'));

  function focusLocalSession(id: string) {
    setActiveSessionInColumn(instanceId, id);
  }

  // Cursor model options — empty string means "auto" (forward `--model` unset).
  const cursorModelOptions: DropdownOption<string>[] = [
    { value: '', label: 'auto' },
    { value: 'composer-2', label: 'Composer 2' },
    { value: 'composer-2-fast', label: 'Composer 2 Fast' },
    { value: 'sonnet-4-thinking', label: 'Sonnet 4 Thinking' },
    { value: 'claude-opus-4-7-thinking-high', label: 'Opus 4.7 Thinking High' },
    { value: 'gpt-5.3-codex-high', label: 'Codex 5.3 High' },
    { value: 'gpt-5.4-high', label: 'GPT-5.4 High' }
  ];

  // Claude model options. Empty = no `--model` flag → CLI picks (Opus 4.7
  // on Max plans). Sonnet 4.6 is the per-session default for new chats
  // (see createSession in sessions.svelte.ts).
  const claudeModelOptions: DropdownOption<string>[] = [
    { value: 'claude-sonnet-4-6', label: 'Sonnet 4.6' },
    { value: 'claude-opus-4-7', label: 'Opus 4.7' },
    { value: 'claude-haiku-4-5-20251001', label: 'Haiku 4.5' },
    { value: '', label: 'auto (CLI default)' }
  ];

  // Tool-profile options. 'coding' = the new-session default — App nav +
  // Memory only, no Jira/GitHub/Sentry. Single-source profiles (github /
  // jira / sentry) re-enable that source full-access. 'triage' is the
  // read-only cross-tool variant. 'all' is the legacy "every tool wired"
  // behavior. See ToolProfile in claude.rs for the exact tool list per
  // profile.
  const claudeToolProfileOptions: DropdownOption<string>[] = [
    { value: 'coding', label: 'Coding' },
    { value: 'github', label: 'GitHub' },
    { value: 'jira', label: 'Jira' },
    { value: 'sentry', label: 'Sentry' },
    { value: 'triage', label: 'Triage (read-only)' },
    { value: 'all', label: 'All tools' }
  ];

  async function pickFiles() {
    if (!activeSess) onNewClaudeSession({ agentKind: kind, columnInstanceId: instanceId });
    const picked = await openDialog({
      multiple: true,
      title: 'Attach files or images',
      filters: [
        { name: 'Images', extensions: ['png', 'jpg', 'jpeg', 'webp', 'gif', 'bmp', 'svg', 'heic', 'heif', 'avif'] },
        { name: 'All files', extensions: ['*'] }
      ]
    });
    if (!picked) return;
    const paths = Array.isArray(picked) ? picked : [picked];
    if (!activeSess) return;
    const n = attachPathsToSession(activeSess.id, paths as string[]);
    if (n > 0) focusLocalSession(activeSess.id);
  }

  function removeMention(externalId: string) {
    if (!activeSess) return;
    const mentions = activeSess.mentions.filter((m) => m.externalId !== externalId);
    // Best-effort: also strip the `@<externalId>` token from input so the two
    // stay in sync. Users can put it back manually if they deleted by accident.
    const token = new RegExp(
      `(^|\\s)@${externalId.replace(/[.*+?^${}()|[\\]\\\\]/g, '\\\\$&')}(?=\\s|$)`
    );
    const input = activeSess.input.replace(token, '$1').replace(/\s{2,}/g, ' ');
    updateSession(activeSess.id, { mentions, input });
    focusLocalSession(activeSess.id);
  }

  // ---- @-autocomplete ----
  //
  // Detects when the user is actively typing a mention (@-token with no
  // whitespace from `@` to caret) and pops a filtered list of Jira issues,
  // GitHub inbox items, Sentry issues, and repo files. Picking an item
  // replaces the query with `@<externalId>` and attaches a Mention to the
  // session so the prompt builder can bake context in on send.

  type MentionCandidate = {
    source: 'jira' | 'github' | 'sentry' | 'file';
    externalId: string;
    title: string;
    hint: string;
    isDir?: boolean;
    absPath?: string;
    /** Sentry-only — compact context (`type: value · culprit · level`)
        baked into the resulting Mention's `body` so prompt builder
        forwards it to Claude before MCP follow-up calls. */
    sentryBody?: string;
  };

  let textareaEl = $state<HTMLTextAreaElement | null>(null);
  let backdropEl = $state<HTMLDivElement | null>(null);
  let mentionQuery = $state<string | null>(null);
  let mentionAt = $state(0);
  let mentionSelectedIdx = $state(0);
  let fileIndex = $state<{ repo: string; paths: string[] } | null>(null);

  /** HTML-escape a string so we can wrap @tokens in spans without letting
      `<` / `&` from the user's text turn into real markup. Keeps the
      backdrop a faithful mirror of the textarea content. */
  function escapeHtml(s: string): string {
    return s.replace(/[&<>"']/g, (c) => {
      switch (c) {
        case '&': return '&amp;';
        case '<': return '&lt;';
        case '>': return '&gt;';
        case '"': return '&quot;';
        case "'": return '&#39;';
      }
      return c;
    });
  }

  /** A `@<token>` earns a highlight only when it resolves to something
      real. Keeps random strings like `@bla-bla-bla` as plain text so the
      chip style stays meaningful. The rules, in priority order:
        1. Jira-style key: `DEVOPS-437` / `EFF-21190`
        2. GitHub-style shorthand: `#482`
        3. Already attached to the session via the popover or drop
        4. Any path containing `/` (assume the user is referring to one)
        5. Exact match in the current repo's `git ls-files` index
      Rule (4) is intentionally permissive — partial paths like `@src/f`
      get a chip as soon as the slash appears, so typing doesn't feel
      laggy; (5) catches single-segment filenames at repo root like
      `@README.md` where no slash is present. */
  function isKnownMention(
    token: string,
    mentions: { externalId: string }[],
    fileSet: Set<string>
  ): boolean {
    // Single-segment Jira keys (DEVOPS-437) + multi-segment Sentry short
    // ids (CATALOG-API-76, BMS-API-J6). Trailing segment alphanumeric so
    // base-32-suffix Sentry ids match too.
    if (/^[A-Z][A-Z0-9_]*(?:-[A-Z0-9_]+)+$/.test(token)) return true;
    if (/^#\d+$/.test(token)) return true;
    if (mentions.some((m) => m.externalId === token)) return true;
    if (token.includes('/')) return true;
    if (fileSet.has(token)) return true;
    return false;
  }

  /** Build the highlighted HTML for the textarea backdrop. Wraps known
      `@<token>`s in a span; unknown ones pass through as plain escaped
      text. The span intentionally has NO padding / border / margin —
      otherwise it would widen the glyphs and the backdrop's line-wrapping
      would drift out of sync with the actual textarea wrapping. */
  function highlightMentions(
    text: string,
    mentions: { externalId: string }[],
    fileSet: Set<string>
  ): string {
    const re = /(^|\s)(@[^\s]+)/g;
    let out = '';
    let last = 0;
    let m: RegExpExecArray | null;
    while ((m = re.exec(text)) !== null) {
      const boundary = m[1];
      const tokenFull = m[2]; // includes leading '@'
      const token = tokenFull.slice(1);
      const tokenStart = m.index + boundary.length;
      const tokenEnd = tokenStart + tokenFull.length;
      out += escapeHtml(text.slice(last, m.index));
      out += escapeHtml(boundary);
      if (isKnownMention(token, mentions, fileSet)) {
        out += `<span class="mention-hl">${escapeHtml(tokenFull)}</span>`;
      } else {
        out += escapeHtml(tokenFull);
      }
      last = tokenEnd;
    }
    out += escapeHtml(text.slice(last));
    // Trailing newline: browsers collapse a pure trailing `\n` in white-space:
    // pre-wrap DIVs, so add a zero-width placeholder to keep the backdrop
    // one line taller (matching the textarea's trailing-newline behavior).
    if (out.endsWith('\n')) out += '\u200b';
    return out;
  }

  // `git ls-files` output as a set for O(1) exact-match lookups. Pre-seeded
  // lazily on repo change so the backdrop can validate `@README.md`-style
  // filename mentions before the autocomplete popover fires.
  const fileIndexSet = $derived(new Set(fileIndex?.paths ?? []));
  // Pull the file index the moment a session with a repo becomes active —
  // the chip won't light up on `@README.md` otherwise until the user
  // types an `@` somewhere.
  $effect(() => {
    if (activeRepo) void ensureFileIndex();
  });
  const highlightedInput = $derived(
    highlightMentions(
      activeSess?.input ?? '',
      activeSess?.mentions ?? [],
      fileIndexSet
    )
  );

  /** Mirror the textarea's scroll offset onto the backdrop so highlighted
      text stays aligned with glyphs when the textarea overflows vertically.
      `line-height` + padding must match between the two elements for this
      to read as a single combined element. */
  function syncBackdropScroll() {
    if (!textareaEl || !backdropEl) return;
    backdropEl.scrollTop = textareaEl.scrollTop;
    backdropEl.scrollLeft = textareaEl.scrollLeft;
  }

  const activeRepo = $derived(activeSess?.worktreePath || activeSess?.cwd || editorRepoPath || '');

  // Lazy file index — loaded once per repo via `git ls-files`. Kept in the
  // component because each session might pick a different repo. ~30ms cold,
  // instant warm.
  async function ensureFileIndex() {
    if (!activeRepo) return;
    if (fileIndex && fileIndex.repo === activeRepo) return;
    try {
      const paths = await invoke<string[]>('git_ls_files', { repo: activeRepo });
      fileIndex = { repo: activeRepo, paths };
    } catch {
      fileIndex = { repo: activeRepo, paths: [] };
    }
  }

  /** Walk back from the caret: if we're inside a `@token` (started by
      whitespace or line-start), set `mentionQuery` to everything between
      the `@` and the caret. Any whitespace breaks the token and closes
      the popover.

      Resets `mentionSelectedIdx` to 0 only when the query actually
      changes — otherwise ArrowUp/Down + `keyup` would reset the
      selection back to the first item between every keystroke. */
  function syncMentionFromTextarea(el: HTMLTextAreaElement) {
    const value = el.value;
    const pos = el.selectionStart ?? value.length;
    let i = pos - 1;
    while (i >= 0) {
      const c = value[i];
      if (c === '@') {
        // Require whitespace or start-of-string before the '@' so e.g.
        // an email address isn't mistaken for a mention.
        if (i === 0 || /\s/.test(value[i - 1])) {
          const nextQuery = value.slice(i + 1, pos);
          mentionAt = i;
          if (nextQuery !== mentionQuery) {
            mentionQuery = nextQuery;
            mentionSelectedIdx = 0;
          }
          void ensureFileIndex();
          return;
        }
        break;
      }
      if (/\s/.test(c)) break;
      i--;
    }
    mentionQuery = null;
  }

  function closeMentionPopover() {
    mentionQuery = null;
    mentionSelectedIdx = 0;
  }

  /** Rank-one fuzzy score — case-insensitive substring match, with a
      big bonus for prefix and a small bonus for contiguous matches near
      the start of the string. Good-enough for a composer popover. */
  function score(haystack: string, needle: string): number {
    if (!needle) return 1;
    const h = haystack.toLowerCase();
    const n = needle.toLowerCase();
    if (h.startsWith(n)) return 1000 - h.length;
    const idx = h.indexOf(n);
    if (idx < 0) return -1;
    return 500 - idx - h.length;
  }

  const mentionCandidates = $derived<MentionCandidate[]>(
    (() => {
      if (mentionQuery === null) return [];
      const q = mentionQuery;
      const out: { cand: MentionCandidate; s: number }[] = [];

      /* Walk every Jira / GitHub / Sentry column's loaded items —
         post per-instance refactor each column has its own list, but
         @-mention candidates draw from anything the user has open. */
      const allJiraItems = Object.values(inboxState.jiraItemsByInstance).flat();
      const allGhItems = Object.values(inboxState.itemsByInstance).flat();
      const allSentryItems = Object.values(inboxState.sentryItemsByInstance).flat();

      // Jira issues — externalId is the key (e.g. DEVOPS-437).
      for (const j of allJiraItems) {
        const s = Math.max(score(j.key, q), score(j.summary, q));
        if (s < 0) continue;
        out.push({
          cand: {
            source: 'jira',
            externalId: j.key,
            title: j.summary,
            hint: `Jira · ${j.status.toLowerCase()}`
          },
          s: s + 10 // small boost: tickets feel most "reference-y"
        });
      }

      // GitHub issues/PRs — externalId is `#<number>` for @mention parity
      // with how the Markdown renderer styles them.
      for (const it of allGhItems) {
        const id = `#${it.number}`;
        const s = Math.max(score(id, q), score(it.title, q));
        if (s < 0) continue;
        out.push({
          cand: {
            source: 'github',
            externalId: id,
            title: it.title,
            hint: it.is_pull_request ? 'PR' : 'Issue'
          },
          s
        });
      }

      // Sentry issues — externalId is the short id (e.g. `BMS-API-J6`).
      // `sentryBody` is the compact context block stitched into the Mention
      // so Claude can answer "what's @CATALOG-API-76?" without an MCP
      // round-trip for the basics.
      for (const it of allSentryItems) {
        const s = Math.max(score(it.short_id, q), score(it.title, q));
        if (s < 0) continue;
        const bodyParts: string[] = [];
        if (it.metadata_type || it.metadata_value) {
          const t = it.metadata_type ?? '';
          const v = it.metadata_value ?? '';
          bodyParts.push(`${t}${t && v ? ': ' : ''}${v}`.trim());
        }
        if (it.culprit) bodyParts.push(`at ${it.culprit}`);
        bodyParts.push(`level=${it.level}`);
        if (it.project_slug) bodyParts.push(`project=${it.project_slug}`);
        if (it.permalink) bodyParts.push(it.permalink);
        out.push({
          cand: {
            source: 'sentry',
            externalId: it.short_id,
            title: it.title,
            hint: `Sentry · ${it.level}`,
            sentryBody: bodyParts.join(' · ')
          },
          s: s + 8 // small boost — Sentry short-ids are reference-y too
        });
      }

      // Files + folders — filter only when the user has typed at least
      // one char; otherwise the popover would dump the whole repo.
      if (q.length > 0 && fileIndex) {
        for (const p of fileIndex.paths) {
          const s = score(p, q);
          if (s < 0) continue;
          const slash = p.lastIndexOf('/');
          const name = slash >= 0 ? p.slice(slash + 1) : p;
          const dir = slash >= 0 ? p.slice(0, slash) : '';
          out.push({
            cand: {
              source: 'file',
              externalId: p,
              title: name,
              hint: dir || 'file',
              isDir: false,
              absPath: `${activeRepo.replace(/\/$/, '')}/${p}`
            },
            s: s - 2 // slight deprioritization vs tickets
          });
        }
      }

      out.sort((a, b) => b.s - a.s);
      return out.slice(0, 12).map((x) => x.cand);
    })()
  );

  /** Replace the live `@query` span in the textarea with `@<externalId>`,
      add a Mention to the session so the prompt builder can inject
      context on send, and close the popover. */
  function applyMentionCandidate(c: MentionCandidate) {
    if (!activeSess || !textareaEl || mentionQuery === null) return;
    const el = textareaEl;
    const value = el.value;
    const caret = el.selectionStart ?? value.length;
    const before = value.slice(0, mentionAt);
    const after = value.slice(caret);
    const token = `@${c.externalId}`;
    const insertion = token + (after.startsWith(' ') ? '' : ' ');
    const newInput = before + insertion + after;

    const alreadyMentioned = activeSess.mentions.some((m) => m.externalId === c.externalId);
    let mentionBody: string | null = null;
    if (c.source === 'file') mentionBody = c.absPath ?? null;
    else if (c.source === 'sentry') mentionBody = c.sentryBody ?? null;
    const mention: Mention = alreadyMentioned
      ? activeSess.mentions.find((m) => m.externalId === c.externalId)!
      : {
          source: c.source,
          externalId: c.externalId,
          title: c.title,
          body: mentionBody,
          isDir: c.isDir ?? false
        };
    const mentions = alreadyMentioned
      ? activeSess.mentions
      : [...activeSess.mentions, mention];

    onSetSessionInput(activeSess.id, newInput);
    updateSession(activeSess.id, { mentions });
    closeMentionPopover();
    // Restore caret position just after the inserted token.
    queueMicrotask(() => {
      if (textareaEl) {
        const pos = (before + insertion).length;
        textareaEl.focus();
        textareaEl.setSelectionRange(pos, pos);
      }
    });
  }

  /** Keydown handler on the textarea. When the mention popover is open
      we swallow ↑/↓/Enter/Tab/Esc so they drive the popover instead of
      the composer; everything else falls through to the existing
      `submit on Enter` logic. */
  function onTextareaKeydown(e: KeyboardEvent, sess: (typeof sessionsState.list)[number]) {
    if (mentionQuery !== null && mentionCandidates.length > 0) {
      if (e.key === 'ArrowDown') {
        e.preventDefault();
        mentionSelectedIdx = (mentionSelectedIdx + 1) % mentionCandidates.length;
        return;
      }
      if (e.key === 'ArrowUp') {
        e.preventDefault();
        mentionSelectedIdx = (mentionSelectedIdx - 1 + mentionCandidates.length) % mentionCandidates.length;
        return;
      }
      if (e.key === 'Enter' || e.key === 'Tab') {
        e.preventDefault();
        applyMentionCandidate(mentionCandidates[mentionSelectedIdx]);
        return;
      }
      if (e.key === 'Escape') {
        e.preventDefault();
        closeMentionPopover();
        return;
      }
    }
    if (e.key === 'Enter' && !e.shiftKey && !sess.sending) {
      e.preventDefault();
      focusLocalSession(sess.id);
      onSendClaudeMessage();
    }
  }
</script>

<section
  class="wb-column claude-col"
  class:wb-column--cursor={kind === 'cursor'}
  class:drag-over={dragOver}
  ondragenter={(e) => onAgentDragEnter(instanceId, kind, e)}
  ondragover={(e) => onAgentDragOver(instanceId, kind, e)}
  ondragleave={() => onAgentDragLeave(instanceId)}
  ondrop={(e) => onAgentDrop(instanceId, kind, e)}
  aria-label={brandLabel}
  data-instance-id={instanceId}
  data-kind={kind}
  style="order: {order}; flex: 0 0 {inst?.width ?? 520}px"
  transition:slide={{ duration: 240, axis: 'x', easing: cubicOut }}
>
  <ColumnControls {instanceId} {kind} />
  <div class="wb-col-resize" class:snap-flash={layoutState.snapFlashInstanceId === instanceId} role="separator" aria-orientation="vertical" onpointerdown={(e) => startResizeById(instanceId, e)}></div>
  <div class="inbox-brand">
    {#if kind === 'claude'}
      <span class="brand-icon conn-icon--claude conn-icon--img" aria-hidden="true">
        <img src={claudeMeta.iconImg} alt="" class="conn-icon-img" />
      </span>
    {:else}
      <span class="brand-icon conn-icon--cursor conn-icon--img" aria-hidden="true">
        <img src={cursorMeta.iconImg} alt="" class="conn-icon-img" />
      </span>
    {/if}
    <span class="brand-word">{brandLabel}</span>
    {#if inst?.name}<span class="bench-name mono" title="Bench id — use this to link from elsewhere">{inst.name}</span>{/if}
    {#if brandVersion}<span class="brand-sub mono">{brandVersion.split(' ')[0]}</span>{/if}
  </div>

  {#if activeSess}
    <div class="cwd-bar" class:cwd-bar--linked={activeSess.linkedToEditor}>
      {#if activeSess.linkedToEditor}
        <!-- Tight linked strip. Folder + branch are visible in the Editor
             column right next to this one — no need to repeat them here.
             Isolate stays available so you can still spin up a worktree
             even when linked. Click the 🔗 pill to jump to the Editor. -->
        <button
          class="linked-pill"
          onclick={() => { focusLocalSession(activeSess.id); onOpenSessionFolderInEditor(); }}
          title={editorRepoPath ? `Reveal in Editor: ${editorRepoPath}` : 'Editor has no folder open'}
        >
          <span class="linked-pill-dot"></span>
          <svg class="i i-sm" viewBox="0 0 24 24"><path d="M9 17H7A5 5 0 1 1 7 7h2M15 7h2a5 5 0 1 1 0 10h-2M8 12h8"/></svg>
          <span class="linked-pill-label">Linked to Editor</span>
          {#if linkedEditor}
            <span class="linked-pill-bench mono">{linkedEditor.name}</span>
          {/if}
        </button>
        <div style="flex:1"></div>
        {#if !activeSess.worktreePath}
          <button
            class="wt-chip wt-chip--create"
            onclick={() => { focusLocalSession(activeSess.id); onCreateWorktree(); }}
            disabled={worktreeBusy === 'creating' || !editorRepoPath}
            title={editorRepoPath ? 'Run in an isolated git worktree. Safer for parallel agents — your main working tree stays untouched.' : 'Editor has no folder open'}
          >
            <svg class="i i-sm" viewBox="0 0 24 24"><path d="M6 3v18M6 9a3 3 0 0 0 3 3h4a3 3 0 0 1 3 3v6M18 3a3 3 0 1 1 0 6 3 3 0 0 1 0-6z"/></svg>
            <span>{worktreeBusy === 'creating' ? 'Isolating…' : 'Isolate'}</span>
          </button>
        {/if}
        <button
          class="unlink-btn"
          onclick={() => { focusLocalSession(activeSess.id); onToggleEditorLink(); }}
          title="Unlink — the chat keeps its current folder as an explicit cwd"
        >
          <svg class="i i-sm" viewBox="0 0 24 24"><path d="M9 17H7A5 5 0 1 1 7 7h2M15 7h2a5 5 0 0 1 4 8M3 3l18 18"/></svg>
          <span>Unlink</span>
        </button>
      {:else}
        <button
          class="cwd-chip"
          class:has-cwd={activeSess.cwd}
          class:editor-linked={!activeSess.cwd && editorRepoPath}
          class:muted={!!activeSess.worktreePath}
          onclick={() => { focusLocalSession(activeSess.id); onPickCwd(); }}
          title={activeSess.worktreePath ? `Overridden by worktree below` : (activeSess.cwd ?? (editorRepoPath ? `Editor folder: ${editorRepoPath}` : 'Pick working directory'))}
        >
          <svg class="i i-sm" viewBox="0 0 24 24"><path d="M3 7v11a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2V9a2 2 0 0 0-2-2h-7L10 5H5a2 2 0 0 0-2 2z" /></svg>
          <span class="cwd-label mono">
            {#if activeSess.cwd}
              {shortPath(activeSess.cwd)}
            {:else if editorRepoPath}
              ↳ {shortenFsPath(editorRepoPath)}
            {:else}
              No folder
            {/if}
          </span>
        </button>
        {#if activeSess.cwd}
          <button class="icon-btn" onclick={() => { focusLocalSession(activeSess.id); onClearCwd(); }} title="Clear folder override" aria-label="Clear folder">
            <svg class="i i-sm" viewBox="0 0 24 24"><path d="M18 6 6 18M6 6l12 12"/></svg>
          </button>
        {/if}
        {#if editorInstances.length > 1}
          <!-- Multiple Editor benches open — let the user pick which one to
               attach this chat to by bench name. -->
          <div class="link-editor-picker">
            <Dropdown
              value=""
              options={editorInstances.map((e) => ({
                value: e.id,
                label: `Link to ${e.name}`
              }))}
              onChange={(id) => { focusLocalSession(activeSess.id); onLinkToEditorInstance(id); }}
              placeholder="Link editor…"
              ariaLabel="Link to editor bench"
            />
          </div>
        {:else}
          <button
            class="link-editor-btn"
            onclick={() => { focusLocalSession(activeSess.id); onToggleEditorLink(); }}
            disabled={editorInstances.length === 0}
            title={editorInstances.length === 0
              ? 'Open an Editor column first to link this chat to its folder.'
              : 'Link this chat to the Editor folder so the cwd tracks the Editor live.'}
          >
            <svg class="i i-sm" viewBox="0 0 24 24"><path d="M9 17H7A5 5 0 1 1 7 7h2M15 7h2a5 5 0 1 1 0 10h-2M8 12h8"/></svg>
            <span>Link editor</span>
          </button>
        {/if}
        {#if !activeSess.worktreePath}
          <button
            class="wt-chip wt-chip--create"
            onclick={() => { focusLocalSession(activeSess.id); onCreateWorktree(); }}
            disabled={worktreeBusy === 'creating' || (!activeSess.cwd && !editorRepoPath)}
            title={activeSess.cwd || editorRepoPath ? 'Run in an isolated git worktree. Safer for parallel agents — your main working tree stays untouched.' : 'Pick a folder first'}
          >
            <svg class="i i-sm" viewBox="0 0 24 24"><path d="M6 3v18M6 9a3 3 0 0 0 3 3h4a3 3 0 0 1 3 3v6M18 3a3 3 0 1 1 0 6 3 3 0 0 1 0-6z"/></svg>
            <span>{worktreeBusy === 'creating' ? 'Isolating…' : 'Isolate'}</span>
          </button>
        {/if}
      {/if}
      {#if activeSess.agentKind === 'cursor'}
        <div class="model-chip" title="Cursor model — forwarded to cursor-agent --model">
          <svg class="i i-sm" viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="3"/><path d="M12 2v3M12 19v3M2 12h3M19 12h3M4.9 4.9l2.1 2.1M17 17l2.1 2.1M4.9 19.1 7 17M17 7l2.1-2.1"/></svg>
          {#key activeSess.id}
            <Dropdown
              value={activeSess.cursorModel ?? ''}
              options={cursorModelOptions}
              onChange={(v) => onUpdateSessionCursorModel(activeSess.id, v || null)}
              ariaLabel="Cursor model"
              variant="ghost"
              compact
            />
          {/key}
        </div>
      {/if}
      {#if activeSess.agentKind === 'claude'}
        <div class="model-chip" title="Claude model — forwarded as `claude --model`. Default Sonnet 4.6 (~5x cheaper output than Opus on Max plans).">
          <svg class="i i-sm" viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="3"/><path d="M12 2v3M12 19v3M2 12h3M19 12h3M4.9 4.9l2.1 2.1M17 17l2.1 2.1M4.9 19.1 7 17M17 7l2.1-2.1"/></svg>
          {#key activeSess.id}
            <Dropdown
              value={activeSess.claudeModel ?? ''}
              options={claudeModelOptions}
              onChange={(v) => onUpdateSessionClaudeModel(activeSess.id, v || null)}
              ariaLabel="Claude model"
              variant="ghost"
              compact
            />
          {/key}
        </div>
        <div class="model-chip" title="Tool profile — selects which MCP tool group is wired. Coding (default) skips GitHub/Jira/Sentry to save ~10-15k tokens of MCP schemas per turn.">
          <svg class="i i-sm" viewBox="0 0 24 24" aria-hidden="true"><path d="M14.7 6.3a4 4 0 0 0-5.4 5.4l-6.6 6.6a1 1 0 0 0 1.4 1.4l6.6-6.6a4 4 0 0 0 5.4-5.4l-2.7 2.7-2-2 2.7-2.7z"/></svg>
          {#key activeSess.id}
            <Dropdown
              value={activeSess.claudeToolProfile ?? 'all'}
              options={claudeToolProfileOptions}
              onChange={(v) => onUpdateSessionClaudeToolProfile(
                activeSess.id,
                v as 'all' | 'coding' | 'github' | 'jira' | 'sentry' | 'triage'
              )}
              ariaLabel="Claude tool profile"
              variant="ghost"
              compact
            />
          {/key}
        </div>
      {/if}
      <!-- Compact button: works for both claude and cursor. Backend
           dispatches by `agentKind` to claude::compact_session or
           cursor::compact_session — same fork-session pattern, same
           CompactResult shape. Hidden until the chat has enough
           history to be worth summarising. -->
      <button
        class="wt-chip"
        onclick={runCompact}
        disabled={compactingId === activeSess.id || activeSess.sending || activeSess.messages.length < 4}
        title={compactingId === activeSess.id
          ? 'Compacting…'
          : activeSess.messages.length < 4
            ? 'Not enough conversation to compact yet — chat a bit first.'
            : `Compact: ask ${activeSess.agentKind === 'claude' ? 'Claude' : 'cursor-agent'} to summarise the conversation, then start a fresh session seeded with that summary. Cuts context size and quota cost on long chats.`}
      >
        <svg class="i i-sm" viewBox="0 0 24 24"><path d="M4 14h6v6M4 10h6V4M14 10h6V4M14 14h6v6"/></svg>
        <span>{compactingId === activeSess.id ? 'Compacting…' : 'Compact'}</span>
      </button>
      {#if activeSess.lastContextSize > 0}
          {@const cw = contextWindowFor(effectiveModel, activeSess.agentKind)}
          {@const pct = Math.round(ctxRatio * 100)}
          {@const ringClass = ctxRatio >= 0.85 ? 'ctx-ring-fill ctx-ring-fill--alert'
            : ctxRatio >= 0.6 ? 'ctx-ring-fill ctx-ring-fill--warn'
            : 'ctx-ring-fill'}
          <!-- Context-window ring + session cost. Ring shows the LAST
               turn's context size as a fraction of the model's window
               (200k Sonnet/Haiku, 1M Opus). Number to its right is
               the cumulative API-rate-card cost across every turn in
               this session — directional on a Pro/Max subscription
               (you're not actually billed per token), literal if the
               user has set ANTHROPIC_API_KEY. -->
          <div
            class="ctx-ring"
            title={`Context: ${formatTokens(activeSess.lastContextSize)} / ${formatTokens(cw)} (${pct}%) · session cost ≈ ${formatCostUsd(cumCostUsd)} (API-rate equivalent; subscription users aren't billed per token)`}
          >
            <svg width="18" height="18" viewBox="0 0 20 20" aria-hidden="true">
              <circle class="ctx-ring-track" cx="10" cy="10" r="8" fill="none" stroke-width="2"/>
              <circle
                class={ringClass}
                cx="10" cy="10" r="8" fill="none" stroke-width="2"
                stroke-dasharray={`${(ctxRatio * 50.265).toFixed(2)} 50.265`}
                stroke-dashoffset="0"
                transform="rotate(-90 10 10)"
                stroke-linecap="round"
              />
            </svg>
            <span class="mono">{pct}% · {formatCostUsd(cumCostUsd)}</span>
          </div>
      {/if}
      {#if activeSess.agentKind === 'claude'}
        {#if quotaState.usage && (planFiveHour !== null || planSevenDay !== null)}
          <!-- Subscription quota chip → click opens a popover with
               progress bars, mirroring `claude /usage` in the CLI.
               Same OAuth endpoint, same numbers. Headline on the
               chip is 5h + weekly because those are the two walls
               users hit; full breakdown (Opus / Sonnet / Claude
               Design) lives in the popover. -->
          <div class="plan-chip-wrap">
            <button
              type="button"
              class={planRingClass}
              onclick={() => { planPopoverOpen = !planPopoverOpen; if (planPopoverOpen) void refreshPlanUsage({ force: true }); }}
              aria-expanded={planPopoverOpen}
              aria-haspopup="dialog"
              title="Plan usage — click for breakdown"
            >
              <svg class="i i-sm" viewBox="0 0 24 24" aria-hidden="true">
                <path d="M3 12a9 9 0 1 0 9-9"/>
                <path d="M12 7v5l3 2"/>
              </svg>
              <span class="mono">
                {#if planFiveHour !== null}5h {Math.round(planFiveHour)}%{/if}
                {#if planFiveHour !== null && planSevenDay !== null} · {/if}
                {#if planSevenDay !== null}7d {Math.round(planSevenDay)}%{/if}
              </span>
            </button>
            {#if planPopoverOpen}
              {@const u = quotaState.usage}
              {@const buckets = [
                { key: '5h', label: '5-hour limit', pct: u.five_hour?.utilization ?? null, resetsAt: u.five_hour?.resets_at ?? null },
                { key: '7d', label: 'Weekly · all models', pct: u.seven_day?.utilization ?? null, resetsAt: u.seven_day?.resets_at ?? null },
                { key: 'opus', label: 'Weekly · Opus', pct: u.seven_day_opus?.utilization ?? null, resetsAt: u.seven_day_opus?.resets_at ?? null },
                { key: 'sonnet', label: 'Weekly · Sonnet', pct: u.seven_day_sonnet?.utilization ?? null, resetsAt: u.seven_day_sonnet?.resets_at ?? null },
                { key: 'omelette', label: 'Weekly · Claude Design', pct: u.seven_day_omelette?.utilization ?? null, resetsAt: u.seven_day_omelette?.resets_at ?? null }
              ].filter((b) => b.pct !== null)}
              <!-- Backdrop catches outside clicks to close. Sits
                   below the popover (z-index) but above the page so
                   clicks on chat / sidebar dismiss instead of
                   passing through. -->
              <button
                type="button"
                class="plan-popover-backdrop"
                aria-label="Close plan usage"
                onclick={() => { planPopoverOpen = false; }}
              ></button>
              <div class="plan-popover" role="dialog" aria-label="Plan usage breakdown">
                <div class="plan-popover-head">
                  <span>Plan usage</span>
                  <button
                    type="button"
                    class="plan-refresh"
                    onclick={() => void refreshPlanUsage({ force: true })}
                    disabled={quotaState.loading}
                    aria-label="Refresh plan usage"
                    title={quotaState.loading ? 'Refreshing…' : 'Refresh now'}
                  >
                    <svg class="i i-sm" viewBox="0 0 24 24" class:plan-refresh--spinning={quotaState.loading}>
                      <path d="M1 4v6h6M23 20v-6h-6"/>
                      <path d="M20.49 9A9 9 0 0 0 5.64 5.64L1 10m22 4-4.64 4.36A9 9 0 0 1 3.51 15"/>
                    </svg>
                  </button>
                </div>
                {#each buckets as b (b.key)}
                  {@const pct = Math.max(0, Math.min(100, b.pct ?? 0))}
                  {@const resetsIn = formatResetsIn(b.resetsAt)}
                  <div class="plan-row">
                    <div class="plan-row-head">
                      <span class="plan-row-label">{b.label}</span>
                      <span class="plan-row-num mono">
                        {Math.round(pct)}%{resetsIn ? ` · resets ${resetsIn}` : ''}
                      </span>
                    </div>
                    <div class="plan-bar">
                      <div
                        class="plan-bar-fill"
                        style="width: {pct}%; background: {planBarColor(b.pct)};"
                      ></div>
                    </div>
                  </div>
                {/each}
                <div class="plan-popover-foot mono">
                  Source: <code>api.anthropic.com/api/oauth/usage</code> · same numbers as <code>claude /usage</code>
                </div>
              </div>
            {/if}
          </div>
        {:else if quotaState.error}
          <!-- Fetch failed (typically: not logged in via `claude
               login`, or a 429 from over-eager polling). Click to
               retry. Hover surfaces the underlying error string. -->
          <button
            type="button"
            class="plan-chip plan-chip--err"
            onclick={() => void refreshPlanUsage({ force: true })}
            disabled={quotaState.loading}
            title={`Plan-usage unavailable: ${quotaState.error}. Click to retry.`}
          >
            <svg class="i i-sm" viewBox="0 0 24 24" aria-hidden="true">
              <circle cx="12" cy="12" r="9"/>
              <path d="M12 8v4M12 16h0"/>
            </svg>
            <span class="mono">plan: —</span>
          </button>
        {/if}
      {/if}
    </div>
    {#if sessionsState.activeByInstance[instanceId] === activeSess.id && activeRepoInfo && !activeRepoInfo.missing}
      <div class="repo-info-bar" class:is-git={activeRepoInfo.is_git} class:not-git={!activeRepoInfo.is_git}>
        {#if activeRepoInfo.is_git}
          <span class="repo-info-chip" title={activeRepoInfo.root ?? ''}>
            <svg class="i i-sm" viewBox="0 0 24 24"><path d="M6 3v18M6 9a3 3 0 0 0 3 3h4a3 3 0 0 1 3 3v6M18 3a3 3 0 1 1 0 6 3 3 0 0 1 0-6z"/></svg>
            <span class="mono">{activeRepoInfo.current_branch ?? 'detached'}</span>
          </span>
          {#if activeRepoInfo.dirty_count > 0}
            <span class="repo-info-chip repo-info-dirty" title="{activeRepoInfo.dirty_count} modified file(s), {activeRepoInfo.untracked_count} untracked">
              <span class="repo-info-dot"></span>
              {activeRepoInfo.dirty_count} dirty
            </span>
          {:else}
            <span class="repo-info-chip repo-info-clean" title="Working tree clean">
              <svg class="i i-sm" viewBox="0 0 24 24"><path d="M20 6 9 17l-5-5"/></svg>
              clean
            </span>
          {/if}
          {#if activeRepoInfo.ahead > 0 || activeRepoInfo.behind > 0}
            <span class="repo-info-chip" title="ahead/behind upstream">
              {#if activeRepoInfo.ahead > 0}↑{activeRepoInfo.ahead}{/if}
              {#if activeRepoInfo.behind > 0}↓{activeRepoInfo.behind}{/if}
            </span>
          {/if}
          {#if activeRepoInfo.remote_url}
            <span class="repo-info-remote mono" title={activeRepoInfo.remote_url}>
              {shortRemote(activeRepoInfo.remote_url)}
            </span>
          {:else}
            <span class="repo-info-chip repo-info-noremote" title="No remote configured — PR creation will fail">
              no remote
            </span>
          {/if}
        {:else}
          <span class="repo-info-chip repo-info-notgit">
            <svg class="i i-sm" viewBox="0 0 24 24"><path d="M12 9v4M12 17h.01M10.29 3.86 1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z"/></svg>
            not a git repo
          </span>
        {/if}
      </div>
    {/if}
    {#if activeSess.worktreePath}
      <div class="wt-bar">
        <button
          class="wt-chip wt-chip--active"
          onclick={() => { focusLocalSession(activeSess.id); onToggleWorktreeMenu(); }}
          title={activeSess.worktreePath}
          disabled={worktreeBusy === 'removing'}
        >
          <span class="wt-dot"></span>
          <svg class="i i-sm" viewBox="0 0 24 24"><path d="M6 3v18M6 9a3 3 0 0 0 3 3h4a3 3 0 0 1 3 3v6M18 3a3 3 0 1 1 0 6 3 3 0 0 1 0-6z"/></svg>
          <span class="mono">{activeSess.worktreeBranch}</span>
          <span class="wt-sub">isolated</span>
          <svg class="i i-sm" viewBox="0 0 24 24"><path d="M6 9l6 6 6-6"/></svg>
        </button>
        {#if worktreeMenuOpen && sessionsState.activeByInstance[instanceId] === activeSess.id}
          <div class="wt-menu">
            <div class="wt-menu-header mono" title={activeSess.worktreePath}>{shortenFsPath(activeSess.worktreePath)}</div>
            <button class="wt-menu-item" onclick={() => { focusLocalSession(activeSess.id); onOpenWorktreeDiff(); }}>
              <svg class="i i-sm" viewBox="0 0 24 24"><path d="M12 3v18M6 8l-3 4 3 4M18 8l3 4-3 4"/></svg>
              View changes
            </button>
            <button class="wt-menu-item" onclick={() => { focusLocalSession(activeSess.id); onOpenWorktreeInEditor(); }}>
              <svg class="i i-sm" viewBox="0 0 24 24"><path d="M3 7v11a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2V9a2 2 0 0 0-2-2h-7L10 5H5a2 2 0 0 0-2 2z" /></svg>
              Open worktree in Editor
            </button>
            <button class="wt-menu-item" onclick={() => { focusLocalSession(activeSess.id); onCopyWorktreeBranch(); }}>
              <svg class="i i-sm" viewBox="0 0 24 24"><rect x="9" y="9" width="13" height="13" rx="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></svg>
              Copy branch name
            </button>
            <div class="wt-menu-sep"></div>
            <button class="wt-menu-item wt-menu-item--apply" onclick={() => { focusLocalSession(activeSess.id); onApplyWorktree(); }} disabled={worktreeBusy !== null}>
              <svg class="i i-sm" viewBox="0 0 24 24"><path d="M20 6 9 17l-5-5"/></svg>
              Apply to current branch (merge)
            </button>
            <button class="wt-menu-item wt-menu-item--danger" onclick={() => { focusLocalSession(activeSess.id); onRemoveWorktree(); }} disabled={worktreeBusy === 'removing'}>
              <svg class="i i-sm" viewBox="0 0 24 24"><path d="M3 6h18M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2M19 6l-1 14a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2L5 6"/></svg>
              {worktreeBusy === 'removing' ? 'Removing…' : 'Discard worktree & branch'}
            </button>
          </div>
        {/if}
      </div>
    {/if}
  {/if}

  <!-- Session tabs -->
  <div class="chat-tabs">
    <div class="chat-tabs-scroll">
      {#each kindSessions as s (s.id)}
        <button
          class="chat-tab"
          class:active={s.id === (activeSess?.id ?? null)}
          onclick={() => focusLocalSession(s.id)}
          title={s.title}
        >
          {#if s.mentions.length > 0}
            <span class="chat-tab-mark mono">{s.mentions.length}</span>
          {/if}
          <span class="chat-tab-title">{s.title}</span>
          {#if kindSessions.length > 1}
            <span
              class="chat-tab-close"
              role="button"
              tabindex="0"
              aria-label="Close chat"
              onclick={(e) => { e.stopPropagation(); onDeleteClaudeSession(s.id); }}
              onkeydown={(e) => { if (e.key === 'Enter') { e.stopPropagation(); onDeleteClaudeSession(s.id); } }}
            >
              <svg class="i i-sm" viewBox="0 0 24 24"><path d="M18 6 6 18M6 6l12 12"/></svg>
            </span>
          {/if}
        </button>
      {/each}
    </div>
    <button class="chat-new" onclick={() => onNewClaudeSession({ agentKind: kind, columnInstanceId: instanceId })} title="New chat" aria-label="New chat">
      <svg class="i i-sm" viewBox="0 0 24 24"><path d="M12 5v14M5 12h14"/></svg>
    </button>
  </div>

  {#if !activeSess}
    <div class="claude-drop" class:drag-over={dragOver}>
      <svg class="i" viewBox="0 0 24 24" style="width:44px; height:44px; color: var(--accent-bright); opacity: 0.6;"><path d="M12 2l2.09 6.26L20 10.27l-5 4.87L16.18 22 12 18.56 7.82 22 9 15.14l-5-4.87 5.91-2.01L12 2z"/></svg>
      <div class="claude-drop-title">Start a chat</div>
      <div class="claude-drop-sub">Click + above to create a chat. Or drop a ticket here to open a context-first session.</div>
    </div>
  {:else}
    {@const sess = activeSess}
    {@const imageMentions = sess.mentions.filter((m) => m.source === 'file' && !m.isDir && !!m.body && isImagePath(m.body))}
    <div class="claude-chat">

      <div class="chat-messages" bind:this={sessionsState.scrollEls[instanceId]}>
        {#if sess.messages.length === 0}
          <div class="chat-empty">
            <svg class="i" viewBox="0 0 24 24" style="width:28px; height:28px; color: var(--text-mute);"><path d="M12 2l2.09 6.26L20 10.27l-5 4.87L16.18 22 12 18.56 7.82 22 9 15.14l-5-4.87 5.91-2.01L12 2z"/></svg>
            <div class="chat-empty-title">Ask {brandLabel} anything</div>
            <div class="chat-empty-sub">
              Type a question below. Drop a ticket on this column to start a session with context.
            </div>
          </div>
        {:else}
          {#each sess.messages as msg, idx (idx)}
            <div class="chat-msg chat-msg--{msg.role}" class:chat-msg--editing={editingMsg && editingMsg.sessionId === sess.id && editingMsg.index === idx}>
              <div class="chat-msg-head">
                {#if msg.role === 'assistant'}
                  {@const brandMeta = kind === 'claude' ? claudeMeta : cursorMeta}
                  {#if brandMeta?.iconImg}
                    <span class="chat-avatar chat-avatar--brand">
                      <img src={brandMeta.iconImg} alt="" />
                    </span>
                  {:else}
                    <span class="chat-avatar chat-avatar--claude">{brandInitial}</span>
                  {/if}
                  <span class="chat-who">{brandLabel}</span>
                {:else if msg.role === 'user'}
                  {#if githubStatus.kind === 'connected'}
                    <img src={githubStatus.user.avatar_url} alt="" class="chat-avatar" />
                  {:else}
                    <span class="chat-avatar">NK</span>
                  {/if}
                  <span class="chat-who">You</span>
                {:else}
                  <span class="chat-avatar chat-avatar--system">•</span>
                  <span class="chat-who">System</span>
                {/if}
                <span class="chat-time mono">{relativeTime(msg.at, now)}</span>
                {#if msg.role === 'user' && !sess.sending}
                  <div class="chat-msg-actions">
                    <button
                      class="chat-msg-act"
                      onclick={() => { focusLocalSession(sess.id); onStartEditMessage(sess.id, idx, msg.content); }}
                      title="Edit & resend — everything after will be erased"
                      aria-label="Edit message"
                    >
                      <svg class="i i-sm" viewBox="0 0 24 24"><path d="M12 20h9M16.5 3.5a2.12 2.12 0 0 1 3 3L7 19l-4 1 1-4L16.5 3.5z"/></svg>
                    </button>
                    <button
                      class="chat-msg-act"
                      onclick={() => { focusLocalSession(sess.id); onResendMessage(sess.id, idx, msg.content); }}
                      title="Resend — everything after will be erased"
                      aria-label="Resend message"
                    >
                      <svg class="i i-sm" viewBox="0 0 24 24"><path d="M1 4v6h6M23 20v-6h-6"/><path d="M20.49 9A9 9 0 0 0 5.64 5.64L1 10m22 4-4.64 4.36A9 9 0 0 1 3.51 15"/></svg>
                    </button>
                  </div>
                {/if}
              </div>
              <div class="chat-msg-body">
                {#if msg.role === 'user' && msg.images && msg.images.length > 0}
                  <!-- Image attachments stamped on the user message at send time
                       so the transcript still shows what was sent after the
                       composer chip strip clears. Loaded via convertFileSrc
                       (Tauri asset:// protocol — scoped to $HOME). -->
                  <div class="chat-msg-images">
                    {#each msg.images as img (img.path)}
                      <img class="chat-msg-image" src={convertFileSrc(img.path)} alt={img.name} title={img.name} loading="lazy" />
                    {/each}
                  </div>
                {/if}
                {#if editingMsg && editingMsg.sessionId === sess.id && editingMsg.index === idx}
                  <textarea
                    class="chat-msg-edit"
                    value={editingMsg.draft}
                    oninput={(e) => onSetEditingMsgDraft((e.currentTarget as HTMLTextAreaElement).value)}
                    rows="3"
                    {@attach (node: HTMLTextAreaElement) => { node.focus(); node.setSelectionRange(node.value.length, node.value.length); }}
                    onkeydown={(e) => {
                      if (e.key === 'Escape') { e.preventDefault(); onCancelEditMessage(); }
                      if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) { e.preventDefault(); onCommitEditMessage(); }
                    }}
                  ></textarea>
                  <div class="chat-msg-edit-actions">
                    <button class="btn-tiny" onclick={onCancelEditMessage}>Cancel</button>
                    <button class="btn-tiny btn-tiny--primary" onclick={onCommitEditMessage} disabled={!editingMsg.draft.trim()}>Send ⌘↵</button>
                  </div>
                {:else if msg.role === 'system'}
                  <div class="chat-plain">{msg.content}</div>
                {:else}
                  {#if msg.role === 'assistant' && msg.thinking && msg.thinking.trim()}
                    {@const tkey = `${sess.id}:${idx}`}
                    {@const tOpen = expandedThinking.has(tkey)}
                    <button
                      class="thinking-pill"
                      onclick={() => toggleThinkingExpansion(tkey)}
                      aria-expanded={tOpen}
                      title={tOpen ? 'Collapse thinking' : 'Show thinking'}
                    >
                      <svg class="i i-sm thinking-chevron" class:thinking-chevron--open={tOpen} viewBox="0 0 24 24"><path d="m9 18 6-6-6-6"/></svg>
                      <span class="thinking-pill-label">Thinking</span>
                      <svg class="i i-sm thinking-check" viewBox="0 0 24 24"><path d="M20 6 9 17l-5-5"/></svg>
                    </button>
                    {#if tOpen}
                      <div class="thinking-body mono">{msg.thinking}</div>
                    {/if}
                  {/if}
                  {#if msg.role === 'assistant' && msg.events && msg.events.length > 0}
                    <!-- Preferred render path: walk the ordered events
                         array so text bubbles + tool-trace pills appear
                         in the order the agent produced them. Each
                         trace event is one pill; text events fall
                         straight into Markdown. -->
                    {#each msg.events as ev, evIdx (evIdx)}
                      {#if ev.kind === 'text'}
                        <Markdown source={ev.body} onOpenFile={onOpenMentionPath} />
                      {:else if ev.kind === 'edit'}
                        <!-- Inline diff card for an Edit / MultiEdit chunk.
                             Cursor-style "expand → red/green → Revert"
                             surface, lives next to the assistant text so
                             the user sees the change in context. -->
                        <EditDiffCard
                          sessionId={sess.id}
                          toolId={ev.toolId}
                          filePath={ev.filePath}
                          oldText={ev.oldText}
                          newText={ev.newText}
                          isCreate={ev.isCreate}
                          isDelete={ev.isDelete ?? false}
                          wholeFile={ev.wholeFile ?? false}
                          status={ev.status}
                          note={ev.note}
                        />
                      {:else}
                        {@const ckey = `${sess.id}:${idx}:trace:${evIdx}`}
                        {@const cOpen = expandedTrace.has(ckey)}
                        <button
                          class="thinking-pill"
                          onclick={() => toggleTraceExpansion(ckey)}
                          aria-expanded={cOpen}
                          title={cOpen ? 'Hide steps' : 'Show steps'}
                        >
                          <svg class="i i-sm thinking-chevron" class:thinking-chevron--open={cOpen} viewBox="0 0 24 24"><path d="m9 18 6-6-6-6"/></svg>
                          <span class="thinking-pill-label">{ev.segments.length} step{ev.segments.length === 1 ? '' : 's'}</span>
                          <svg class="i i-sm thinking-check" viewBox="0 0 24 24"><path d="M20 6 9 17l-5-5"/></svg>
                        </button>
                        {#if cOpen}
                          <div class="trace-body">
                            <Markdown source={ev.segments.join('\n\n')} onOpenFile={onOpenMentionPath} />
                          </div>
                        {/if}
                      {/if}
                    {/each}
                  {:else}
                    <!-- Legacy path: messages persisted before the
                         events array existed. Pill on top, body below
                         — same shape as before this refactor. -->
                    {#if msg.role === 'assistant' && msg.trace && msg.trace.trim()}
                      {@const ckey = `${sess.id}:${idx}:trace:legacy`}
                      {@const cOpen = expandedTrace.has(ckey)}
                      {@const stepCount = msg.trace.split('\n\n').filter((s) => s.trim()).length}
                      <button
                        class="thinking-pill"
                        onclick={() => toggleTraceExpansion(ckey)}
                        aria-expanded={cOpen}
                        title={cOpen ? 'Hide steps' : 'Show steps'}
                      >
                        <svg class="i i-sm thinking-chevron" class:thinking-chevron--open={cOpen} viewBox="0 0 24 24"><path d="m9 18 6-6-6-6"/></svg>
                        <span class="thinking-pill-label">{stepCount} step{stepCount === 1 ? '' : 's'}</span>
                        <svg class="i i-sm thinking-check" viewBox="0 0 24 24"><path d="M20 6 9 17l-5-5"/></svg>
                      </button>
                      {#if cOpen}
                        <div class="trace-body">
                          <Markdown source={msg.trace} onOpenFile={onOpenMentionPath} />
                        </div>
                      {/if}
                    {/if}
                    <Markdown source={msg.content} onOpenFile={onOpenMentionPath} />
                  {/if}
                {/if}
                {#if msg.role === 'assistant' && msg.usage}
                  {@const u = msg.usage}
                  {@const hit = cacheHitRate(u)}
                  {@const cost = costForUsage(u)}
                  <!-- Per-turn token / cache-hit / cost badge. Pulled from
                       the last `usage` snapshot of the turn (final sub-step
                       — its cache_read covers the whole conversation, so
                       it's the most informative single number). Cost is
                       a "what API would charge" estimate; on a Pro/Max
                       subscription it's directional, not literal. Cursor
                       turns omit the cost span (subscription credits,
                       not per-token billing — `costForUsage` returns 0
                       when the model isn't in the rate table). -->
                  <div class="usage-badge mono" title={`${formatTokens(u.inputTokens)} input · ${formatTokens(u.cacheCreationTokens)} cache write · ${formatTokens(u.cacheReadTokens)} cache read · ${formatTokens(u.outputTokens)} output${u.model ? ' · ' + u.model : ''}`}>
                    <span>↑ {formatTokens(u.inputTokens + u.cacheCreationTokens + u.cacheReadTokens)}</span>
                    <span>↓ {formatTokens(u.outputTokens)}</span>
                    {#if hit !== null}
                      <span class="usage-badge-cache">{Math.round(hit * 100)}% cache</span>
                    {/if}
                    {#if cost > 0}
                      <span>{formatCostUsd(cost)}</span>
                    {/if}
                  </div>
                {/if}
              </div>
            </div>
          {/each}
        {/if}
        {#if sess.sending}
          <div class="chat-typing">
            <span class="dot-pulse"></span><span class="dot-pulse"></span><span class="dot-pulse"></span>
            {#if thinkingStartedAt && sessionsState.activeByInstance[instanceId] === sess.id}
              <span class="thinking-time mono">
                {thinkingTick}s
              </span>
            {/if}
          </div>
        {/if}
      </div>

      {#if sess.actions.length > 0}
        <div class="action-cards">
          {#if sess.awaitingApproval && !sess.sending}
            <!-- Surface that the agent's turn is paused waiting on the
                 user's decision. After approval (or all-dismiss) the
                 page-level continuation auto-resumes the agent — no
                 need to type "now make the PR" by hand. -->
            <div class="awaiting-approval">
              <span class="awaiting-dot"></span>
              <span>Agent paused — approve or dismiss above to continue.</span>
            </div>
          {/if}
          {#each sess.actions as act (act.id)}
            <ClaudeActionCard
              action={act}
              onUpdate={(patch) => onUpdateAction(sess.id, act.id, patch)}
              onDismiss={() => onRemoveAction(sess.id, act.id)}
              onExecute={() => {
                focusLocalSession(sess.id);
                onExecuteAction(sess.id, act);
              }}
              onOpenPrInForgehold={(url) => onOpenPrInForgehold(url, act.kind === 'pr' ? act : null)}
              repoCwd={sess.worktreePath ?? sess.cwd ?? null}
            />
          {/each}
        </div>
      {/if}

      <!-- Bulk-action bar for un-decided diff cards. Surfaces only
           while there are still cards in `applied` status that the user
           hasn't flipped to `kept` or `reverted`. Rendered as a top
           strip of the composer block (same bg-1 + border-top as
           `.attach-row` / `.chat-input`) so the bar visually fuses with
           whatever is below — no floating pill, no air gap. Visual
           order follows time: bar = "the agent's last changes still
           need a verdict", attach-row = "what you'll send next". -->
      {#if pendingEdits.length > 0}
        <div class="pending-edits-bar" transition:slide={{ duration: 160, easing: cubicOut }}>
          <span class="pending-edits-count" aria-live="polite">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 9v4M12 17h.01"/><circle cx="12" cy="12" r="9"/></svg>
            {pendingEdits.length} pending change{pendingEdits.length === 1 ? '' : 's'}
          </span>
          <div class="pending-edits-actions">
            <button
              type="button"
              class="btn btn--ghost btn--small"
              onclick={handleRevertAllPending}
              disabled={bulkActionBusy}
              title="Revert every pending change in this session, newest first"
            >
              {bulkActionBusy ? 'Reverting…' : 'Revert all'}
            </button>
            <button
              type="button"
              class="btn btn--ghost btn--small"
              onclick={handleKeepAllPending}
              disabled={bulkActionBusy}
              title="Acknowledge every pending change without touching disk"
            >
              Keep all
            </button>
          </div>
        </div>
      {/if}

      <!-- Image attachments don't get an inline `@token` (the path can have
           spaces and the user can't see what's attached either way), so they
           render as thumbnail chips here. Non-image file mentions stay
           inline as `@token`. -->
      {#if imageMentions.length > 0}
        <div class="attach-row">
          {#each imageMentions as m (m.externalId)}
            <div class="attach-chip attach-chip--image" title={m.body ?? m.title}>
              <img class="attach-thumb" src={convertFileSrc(m.body!)} alt="" loading="lazy" />
              <span class="attach-name">{m.title}</span>
              <button
                type="button"
                class="attach-remove"
                onclick={() => removeMention(m.externalId)}
                aria-label="Remove attachment"
                title="Remove"
              >
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M18 6 6 18M6 6l12 12"/></svg>
              </button>
            </div>
          {/each}
        </div>
      {/if}

      <form class="chat-input" onsubmit={(e) => { e.preventDefault(); focusLocalSession(sess.id); onSendClaudeMessage(); }}>
        <button
          type="button"
          class="chat-attach"
          onclick={() => { focusLocalSession(sess.id); void pickFiles(); }}
          disabled={sess.sending}
          aria-label="Attach files or images"
          title="Attach files or images (⌘-click for multi-select)"
        >
          <svg class="i" viewBox="0 0 24 24"><path d="M12 5v14M5 12h14"/></svg>
        </button>
        <div class="chat-textarea-wrap">
          <!-- Backdrop: renders the same text as the textarea but with
               @mention tokens wrapped in styled spans. The textarea itself
               is transparent on top, contributing only the caret + focus
               ring. `aria-hidden` because screen readers already read the
               textarea's value. -->
          <div
            class="chat-textarea-backdrop"
            aria-hidden="true"
            bind:this={backdropEl}
          >{@html highlightedInput}</div>
          <textarea
            bind:this={textareaEl}
            class="chat-textarea"
            value={sess.input}
            oninput={(e) => {
              const el = e.currentTarget as HTMLTextAreaElement;
              onSetSessionInput(sess.id, el.value);
              // Backspacing an `@token` out of the textarea must
              // also unattach the corresponding mention, otherwise
              // the next send still bakes the file into the prompt
              // (`mentionsSnapshot` in +page.svelte) and the
              // placeholder stays "Ask about the attached items…"
              // forever. Image mentions are exempted inside —
              // they're managed via the chip strip above the
              // composer, not via inline tokens.
              pruneMentionsByInput(sess.id, el.value);
              syncMentionFromTextarea(el);
              syncBackdropScroll();
            }}
            onscroll={syncBackdropScroll}
            onkeyup={(e) => syncMentionFromTextarea(e.currentTarget as HTMLTextAreaElement)}
            onclick={(e) => syncMentionFromTextarea(e.currentTarget as HTMLTextAreaElement)}
            onblur={() => setTimeout(closeMentionPopover, 120)}
            placeholder={sess.mentions.length ? 'Ask about the attached items (use @IDs in your text)…' : `Ask ${brandLabel} anything…`}
            disabled={sess.sending}
            onfocus={() => focusLocalSession(sess.id)}
            onkeydown={(e) => onTextareaKeydown(e, sess)}
            onpaste={(e) => {
              // Pull image blobs out of the clipboard. If any landed,
              // intercept the paste so the image data doesn't get
              // round-tripped as garbled text into the textarea.
              const items = e.clipboardData?.items;
              if (!items) return;
              const blobs: { name: string; type: string; blob: Blob }[] = [];
              for (let i = 0; i < items.length; i++) {
                const it = items[i];
                if (it.kind === 'file' && it.type.startsWith('image/')) {
                  const f = it.getAsFile();
                  if (f) {
                    const ext = f.type.split('/')[1] || 'png';
                    blobs.push({
                      name: f.name || `pasted-image.${ext}`,
                      type: f.type,
                      blob: f
                    });
                  }
                }
              }
              if (blobs.length > 0) {
                e.preventDefault();
                focusLocalSession(sess.id);
                void onPasteImages(instanceId, kind, blobs);
              }
            }}
          ></textarea>
          {#if mentionQuery !== null && mentionCandidates.length > 0}
            <div class="mention-pop" role="listbox" aria-label="Mention suggestions">
              <div class="mention-pop-head mono">
                @{mentionQuery || '…'} · {mentionCandidates.length} match{mentionCandidates.length === 1 ? '' : 'es'}
              </div>
              {#each mentionCandidates as c, i (c.source + ':' + c.externalId)}
                <button
                  type="button"
                  class="mention-item"
                  class:active={i === mentionSelectedIdx}
                  onmousedown={(e) => { e.preventDefault(); applyMentionCandidate(c); }}
                  onmouseenter={() => (mentionSelectedIdx = i)}
                  role="option"
                  aria-selected={i === mentionSelectedIdx}
                >
                  <span class="mention-item-kind mention-item-kind--{c.source}">
                    {#if c.source === 'jira'}J
                    {:else if c.source === 'github'}GH
                    {:else if c.source === 'sentry'}Se
                    {:else}F
                    {/if}
                  </span>
                  <span class="mention-item-id mono">{c.externalId}</span>
                  <span class="mention-item-title">{c.title}</span>
                  <span class="mention-item-hint mono">{c.hint}</span>
                </button>
              {/each}
            </div>
          {/if}
        </div>
        {#if sess.sending}
          <button
            type="button"
            class="chat-send chat-stop"
            onclick={() => { focusLocalSession(sess.id); onStopClaude(); }}
            aria-label="Stop"
            title="Stop generation"
          >
            <svg class="i" viewBox="0 0 24 24" fill="currentColor" stroke="none"><rect x="6" y="6" width="12" height="12" rx="2"/></svg>
          </button>
        {:else}
          <button
            type="submit"
            class="chat-send"
            disabled={!sess.input.trim() && sess.mentions.length === 0}
            aria-label="Send"
          >
            <svg class="i" viewBox="0 0 24 24"><path d="M22 2 11 13"/><polygon points="22 2 15 22 11 13 2 9 22 2"/></svg>
          </button>
        {/if}
      </form>
    </div>
  {/if}
</section>

<style>
  /* Agent (Claude / Cursor) chat column. Uses generic .wb-column rules
     from +page.svelte; all chat-, claude-, cwd-, wt-, model-, agent-
     scoped rules live here. */

  .claude-col {
    flex: 1.3 1 420px;
    min-width: 400px;
    display: flex; flex-direction: column;
    background: var(--bg-1);
    transition: background 180ms, box-shadow 180ms;
  }
  .claude-col.drag-over {
    background: rgba(16, 185, 129, 0.05);
    box-shadow: inset 0 0 0 2px rgba(16, 185, 129, 0.4);
  }
  .claude-col .inbox-brand { border-bottom: 1px solid var(--border-neutral); }

  .inbox-brand {
    padding: 16px 20px 10px; display: flex; align-items: center; gap: 10px;
  }
  .brand-word { font-size: 14px; font-weight: 600; color: var(--text-0); letter-spacing: -0.01em; }
  .brand-sub { font-size: 11.5px; color: var(--text-2); margin-left: auto; }
  .source-mark {
    width: 22px; height: 22px; border-radius: 5px;
    display: inline-flex; align-items: center; justify-content: center;
    font-size: 10.5px; font-weight: 700; letter-spacing: -0.02em;
    background: var(--bg-2); color: var(--text-1);
    border: 1px solid var(--border-neutral-hi);
  }

  .claude-drop {
    flex: 1; margin: 16px;
    padding: 36px 20px;
    border: 1.5px dashed var(--border-neutral-hi);
    border-radius: 14px;
    display: flex; flex-direction: column;
    align-items: center; justify-content: center;
    text-align: center; gap: 10px;
    transition: all 220ms;
  }
  .claude-drop.drag-over {
    border-color: rgba(16, 185, 129, 0.55);
    background: radial-gradient(ellipse at center, rgba(16, 185, 129, 0.08), transparent 70%);
    transform: scale(1.01);
  }
  .claude-drop-title {
    font-size: 14px; font-weight: 600; color: var(--text-0);
    margin-top: 6px;
  }
  .claude-drop-sub { font-size: 12.5px; color: var(--text-2); max-width: 300px; line-height: 1.55; }

  .claude-chat {
    flex: 1; display: flex; flex-direction: column; min-height: 0;
  }

  .chat-messages {
    flex: 1; overflow-y: auto; padding: 16px 16px 8px;
    display: flex; flex-direction: column; gap: 14px;
  }
  .chat-msg-actions {
    display: inline-flex; gap: 2px; margin-left: auto;
    opacity: 0; transition: opacity 120ms;
  }
  .chat-msg:hover .chat-msg-actions { opacity: 1; }
  .chat-msg-act {
    width: 22px; height: 22px; border-radius: 4px;
    color: var(--text-2); background: transparent;
    display: inline-flex; align-items: center; justify-content: center;
    transition: all 120ms;
  }
  .chat-msg-act:hover { background: var(--bg-3); color: var(--accent-bright); }
  .chat-msg-act :global(svg) { width: 12px; height: 12px; }

  .chat-msg--editing {
    outline: 2px solid rgba(16, 185, 129, 0.35);
    outline-offset: -2px; border-radius: 8px;
  }
  .chat-msg-edit {
    width: 100%;
    padding: 10px 12px;
    background: var(--bg-0);
    border: 1px solid var(--border-hi2);
    border-radius: 8px;
    font: inherit;
    color: var(--text-0);
    font-size: 13px;
    resize: vertical;
    min-height: 72px;
  }
  .chat-msg-edit:focus { outline: none; border-color: var(--accent); }
  .chat-msg-edit-actions {
    margin-top: 6px;
    display: flex; gap: 6px; justify-content: flex-end;
  }
  :global(.btn-tiny) {
    padding: 5px 10px; border-radius: 6px; font-size: 11.5px; font-weight: 500;
    background: var(--bg-2); color: var(--text-1); border: 1px solid var(--border-neutral-hi);
    transition: all 120ms;
  }
  :global(.btn-tiny:hover:not(:disabled)) { background: var(--bg-3); color: var(--text-0); }
  :global(.btn-tiny--primary) {
    color: var(--accent-fg);
    background: linear-gradient(135deg, #34d399, #10b981);
    border-color: rgba(16, 185, 129, 0.5);
    font-weight: 600;
  }
  :global(.btn-tiny--primary:hover:not(:disabled)) {
    filter: brightness(1.06);
  }
  :global(.btn-tiny:disabled) { opacity: 0.5; cursor: not-allowed; }

  .chat-msg {
    display: flex; flex-direction: column; gap: 6px;
  }
  .chat-msg-head {
    display: flex; align-items: center; gap: 8px;
    font-size: 12px;
  }
  .chat-avatar {
    width: 22px; height: 22px; border-radius: 50%;
    display: inline-flex; align-items: center; justify-content: center;
    font-size: 10px; font-weight: 700;
    background: var(--bg-2); color: var(--text-1);
    border: 1px solid var(--border-neutral-hi);
    flex-shrink: 0;
    object-fit: cover;
  }
  .chat-avatar--claude {
    background: rgba(16, 185, 129, 0.14);
    color: var(--accent-bright);
    border-color: rgba(16, 185, 129, 0.3);
  }
  /* PNG/SVG brand mark variant — fills the circle with the actual logo
     (Anthropic A for Claude, hex prism for Cursor) instead of a letter. */
  .chat-avatar--brand {
    padding: 0;
    background: var(--bg-2);
    overflow: hidden;
  }
  .chat-avatar--brand img {
    width: 100%; height: 100%;
    object-fit: cover;
    display: block;
  }
  .chat-avatar--system {
    background: rgba(59, 130, 246, 0.12);
    color: var(--blue-bright);
    border-color: rgba(59, 130, 246, 0.24);
    font-size: 14px;
  }
  .chat-who { font-weight: 600; color: var(--text-1); }
  .chat-time { margin-left: auto; color: var(--text-mute); font-size: 10.5px; }

  .chat-msg-body {
    padding-left: 30px;
    font-size: 13px; line-height: 1.55; color: var(--text-0);
  }
  .chat-plain {
    color: var(--text-1);
    font-size: 12.5px;
  }
  .chat-msg--user .chat-msg-body {
    color: var(--text-0);
  }
  .chat-msg--system .chat-msg-body {
    color: var(--text-2); font-style: italic;
  }
  /* Image attachments stamped into the user-message bubble at send time. */
  .chat-msg-images {
    display: flex; flex-wrap: wrap; gap: 6px;
    margin-bottom: 6px;
  }
  .chat-msg-image {
    max-width: 220px; max-height: 160px;
    border-radius: 8px;
    border: 1px solid var(--border-neutral-hi);
    background: var(--bg-2);
    object-fit: contain;
    display: block;
  }

  .chat-typing {
    display: inline-flex; gap: 5px; align-items: center;
    padding-left: 30px;
  }
  .thinking-time {
    margin-left: 8px;
    color: var(--text-mute); font-size: 10.5px;
    font-variant-numeric: tabular-nums;
  }
  /* Collapsed "Thinking ✓" pill above an assistant message that emitted
     reasoning blocks. Click to expand the chain-of-thought below it.
     Muted style — secondary to the answer body. */
  .thinking-pill {
    display: inline-flex; align-items: center; gap: 6px;
    margin: 0 0 8px;
    padding: 4px 10px 4px 6px;
    background: var(--bg-1);
    border: 1px solid var(--border-neutral);
    border-radius: 999px;
    color: var(--text-2);
    font-size: 11.5px;
    font-weight: 500;
    cursor: pointer;
    transition: all 120ms;
  }
  .thinking-pill:hover {
    background: var(--bg-2);
    color: var(--text-0);
    border-color: var(--border-neutral-hi);
  }
  .thinking-chevron {
    transition: transform 140ms ease;
    width: 12px; height: 12px;
  }
  .thinking-chevron--open {
    transform: rotate(90deg);
  }
  .thinking-check {
    width: 12px; height: 12px;
    color: var(--accent-bright);
  }
  .thinking-body {
    margin: 0 0 10px;
    padding: 10px 12px;
    background: var(--bg-1);
    border: 1px solid var(--border-neutral);
    border-left: 3px solid var(--border-neutral-hi);
    border-radius: 6px;
    font-size: 11.5px;
    color: var(--text-2);
    white-space: pre-wrap;
    line-height: 1.55;
    max-height: 320px;
    overflow-y: auto;
  }
  /* Trace body uses Markdown rendering (inherits .prose styles), so no
     pre-wrap. Same chrome as thinking-body otherwise — muted, scrollable,
     left-bordered to read as "secondary detail". */
  .trace-body {
    margin: 0 0 10px;
    padding: 6px 12px;
    background: var(--bg-1);
    border: 1px solid var(--border-neutral);
    border-left: 3px solid var(--border-neutral-hi);
    border-radius: 6px;
    max-height: 320px;
    overflow-y: auto;
    font-size: 12px;
  }
  .trace-body :global(p) { margin: 4px 0; }
  .trace-body :global(em) { color: var(--text-1); font-style: normal; font-weight: 500; }
  .dot-pulse {
    width: 6px; height: 6px; border-radius: 50%;
    background: var(--accent-bright);
    animation: pulse 1.2s ease-in-out infinite;
  }
  .dot-pulse:nth-child(2) { animation-delay: 0.2s; }
  .dot-pulse:nth-child(3) { animation-delay: 0.4s; }
  @keyframes pulse {
    0%, 100% { opacity: 0.3; transform: scale(0.8); }
    50% { opacity: 1; transform: scale(1.1); }
  }

  .action-cards {
    display: flex; flex-direction: column; gap: 10px;
    padding: 0 16px 10px;
  }
  .awaiting-approval {
    display: flex; align-items: center; gap: 8px;
    padding: 6px 10px;
    background: var(--accent-soft);
    border: 1px solid rgba(232, 163, 58, 0.25);
    border-radius: 6px;
    font-size: 11.5px;
    color: var(--accent-bright);
  }
  .awaiting-dot {
    width: 6px; height: 6px; border-radius: 50%;
    background: var(--accent-bright);
    animation: pulse 1.4s ease-in-out infinite;
  }

  /* Bulk-action bar for un-decided diff cards. Visually a header strip
     for the composer block — same `bg-1` and `border-top` as
     `.attach-row` and `.chat-input` below, so the three stack into one
     continuous panel without gaps. NO side margin / border-radius: a
     floating pill would read as "loose card" when no attach-row is
     present and the bar would hang in mid-air above the composer.
     Full-width strip removes that ambiguity at the cost of being
     slightly less prominent — fine, it's a transient affordance. */
  .pending-edits-bar {
    display: flex; align-items: center; justify-content: space-between;
    gap: 10px;
    padding: 8px 14px;
    background: var(--bg-1);
    border-top: 1px solid var(--border-neutral);
    font-size: 12px;
  }
  .pending-edits-count {
    display: inline-flex; align-items: center; gap: 6px;
    color: var(--text-1);
    font-weight: 500;
  }
  .pending-edits-count svg {
    width: 14px; height: 14px;
    color: var(--accent-bright);
    flex-shrink: 0;
  }
  .pending-edits-actions {
    display: inline-flex; align-items: center; gap: 6px;
  }

  /* Attachment chips — shown above the composer whenever the active session
     has `mentions`. Image mentions get a live thumbnail via `convertFileSrc`;
     everything else falls back to a folder/file/source-tag icon. */
  .attach-row {
    display: flex; flex-wrap: wrap; gap: 6px;
    padding: 10px 14px 0;
    background: var(--bg-1);
    border-top: 1px solid var(--border-neutral);
  }
  .attach-chip {
    display: inline-flex; align-items: center; gap: 6px;
    padding: 4px 4px 4px 8px;
    background: var(--bg-2);
    border: 1px solid var(--border-neutral-hi);
    border-radius: 7px;
    max-width: 240px;
    transition: border-color 120ms;
  }
  .attach-chip:hover { border-color: var(--border-hi); }
  .attach-chip--image { padding-left: 4px; }
  .attach-thumb {
    width: 26px; height: 26px;
    border-radius: 4px;
    object-fit: cover;
    flex-shrink: 0;
    background: var(--bg-3);
  }
  .attach-icon {
    width: 20px; height: 20px;
    color: var(--text-2);
    flex-shrink: 0;
  }
  .attach-icon--ticket {
    display: inline-flex; align-items: center; justify-content: center;
    width: 20px; height: 20px;
    font-size: 9px; font-weight: 700;
    border-radius: 4px;
    background: var(--bg-3);
    border: 1px solid var(--border-neutral-hi);
    color: var(--text-1);
  }
  .attach-name {
    font-size: 11.5px; color: var(--text-1);
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
    max-width: 160px;
  }
  .attach-remove {
    width: 20px; height: 20px;
    display: inline-flex; align-items: center; justify-content: center;
    border-radius: 4px;
    color: var(--text-mute);
    flex-shrink: 0;
    background: none; border: none; cursor: pointer;
  }
  .attach-remove:hover { color: var(--error); background: var(--bg-3); }
  .attach-remove svg { width: 12px; height: 12px; }

  .chat-input {
    display: flex; align-items: flex-end; gap: 8px;
    padding: 12px 14px;
    border-top: 1px solid var(--border-neutral);
    background: var(--bg-1);
  }
  .chat-attach {
    width: 38px; height: 38px; border-radius: 8px;
    display: inline-flex; align-items: center; justify-content: center;
    color: var(--text-2);
    background: var(--bg-0);
    border: 1px dashed var(--border-neutral-hi);
    transition: all 120ms;
    flex-shrink: 0;
    cursor: pointer;
  }
  .chat-attach:hover:not(:disabled) {
    color: var(--accent-bright);
    border-color: var(--border-hi);
    background: var(--accent-soft);
  }
  .chat-attach:disabled { opacity: 0.4; cursor: not-allowed; }
  .chat-attach svg { width: 18px; height: 18px; }
  /* Composite composer: wrapper owns the border + background, the textarea
     is transparent on top, and a backdrop DIV underneath renders the same
     text with @tokens highlighted. Both children share the exact same
     padding / font / line-height so wrapping and caret positions line up.
     Any drift here shows as text ghosting, so keep them strictly mirrored. */
  .chat-textarea-wrap {
    flex: 1; position: relative; display: flex; min-width: 0;
    background: var(--bg-0);
    border: 1px solid var(--border-neutral-hi);
    border-radius: 8px;
    transition: border-color 120ms;
  }
  .chat-textarea-wrap:focus-within { border-color: var(--accent); }
  .chat-textarea-backdrop {
    position: absolute; inset: 0;
    padding: 10px 12px;
    font-family: 'Inter', -apple-system, BlinkMacSystemFont, system-ui, sans-serif;
    font-size: 13px;
    line-height: 1.5;
    letter-spacing: -0.005em;
    color: var(--text-0);
    white-space: pre-wrap;
    word-wrap: break-word;
    overflow: hidden;
    pointer-events: none;
    /* Use same bg-transparent surface so caret/selection on top read right. */
    background: transparent;
  }
  .chat-textarea-backdrop :global(.mention-hl) {
    /* No padding / border / margin — otherwise the span inflates glyph
       widths and wrapping desyncs from the textarea. Pure background +
       color. Rounded corners via border-radius only (doesn't affect
       layout). */
    color: var(--accent-bright);
    background: rgba(232, 163, 58, 0.18);
    border-radius: 3px;
    box-shadow: 0 0 0 1px rgba(232, 163, 58, 0.24);
    /* `box-shadow` draws the 1px "outline" OUTSIDE the box-model, so it
       doesn't shift adjacent glyphs — keeps wrap alignment intact. */
  }
  .chat-textarea {
    flex: 1;
    min-height: 40px; max-height: 140px;
    padding: 10px 12px;
    background: transparent;
    border: 0;
    border-radius: 8px;
    color: transparent;
    caret-color: var(--text-0);
    font-family: 'Inter', -apple-system, BlinkMacSystemFont, system-ui, sans-serif;
    font-size: 13px;
    line-height: 1.5;
    letter-spacing: -0.005em;
    resize: vertical;
    position: relative; z-index: 1;
  }
  .chat-textarea::selection { color: transparent; background: rgba(232, 163, 58, 0.35); }
  .chat-textarea:focus { outline: none; }
  .chat-textarea:disabled { opacity: 0.5; }

  .chat-send {
    width: 38px; height: 38px; border-radius: 8px;
    display: inline-flex; align-items: center; justify-content: center;
    color: var(--accent-bright);
    background: var(--accent-soft);
    border: 1px solid rgba(16, 185, 129, 0.3);
    transition: all 120ms;
    flex-shrink: 0;
  }
  .chat-send:hover:not(:disabled) {
    background: rgba(16, 185, 129, 0.18);
    border-color: rgba(16, 185, 129, 0.5);
  }
  .chat-send:disabled { opacity: 0.4; cursor: not-allowed; }

  .chat-stop {
    color: var(--error);
    background: rgba(214, 72, 44, 0.12);
    border-color: rgba(214, 72, 44, 0.3);
  }
  .chat-stop:hover {
    background: rgba(214, 72, 44, 0.22);
    border-color: rgba(214, 72, 44, 0.5);
  }

  /* Cwd bar */
  .cwd-bar {
    display: flex; align-items: center; gap: 6px;
    padding: 8px 14px;
    border-bottom: 1px solid var(--border-neutral);
    background: var(--bg-1);
  }
  .agent-select {
    padding: 5px 8px; padding-right: 22px;
    background: var(--bg-0);
    border: 1px solid var(--border-neutral-hi);
    border-radius: 6px;
    color: var(--text-1);
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.03em;
    text-transform: uppercase;
    cursor: pointer;
    appearance: none;
    -webkit-appearance: none;
    background-image: url("data:image/svg+xml;utf8,<svg xmlns='http://www.w3.org/2000/svg' width='8' height='8' viewBox='0 0 24 24' fill='none' stroke='%23888' stroke-width='2.5' stroke-linecap='round' stroke-linejoin='round'><polyline points='6 9 12 15 18 9'/></svg>");
    background-repeat: no-repeat;
    background-position: right 6px center;
  }
  .agent-select:hover { border-color: var(--border-hi); color: var(--text-0); }
  .agent-select:focus { outline: none; border-color: var(--border-hi2); }
  .cwd-chip {
    display: inline-flex; align-items: center; gap: 8px;
    padding: 6px 10px;
    background: var(--bg-0);
    border: 1px dashed var(--border-neutral-hi);
    border-radius: 7px;
    color: var(--text-2);
    font-size: 11.5px;
    transition: all 120ms;
    flex: 1; min-width: 0;
  }
  .cwd-chip:hover { border-color: var(--border-hi); color: var(--text-0); }
  .cwd-chip.has-cwd {
    border-style: solid;
    border-color: rgba(16, 185, 129, 0.25);
    background: var(--accent-soft);
    color: var(--accent-bright);
  }
  .cwd-chip.editor-linked {
    border-color: var(--border-hi);
    color: var(--text-1);
  }
  .cwd-chip.muted { opacity: 0.5; }

  .model-chip {
    display: inline-flex; align-items: center; gap: 6px;
    padding: 4px 4px 4px 8px;
    border: 1px solid var(--border-neutral-hi);
    border-radius: 7px;
    background: var(--bg-0);
    color: var(--text-2);
    flex-shrink: 0;
  }
  .usage-badge {
    display: inline-flex; align-items: center; gap: 8px;
    margin-top: 6px;
    padding: 3px 8px;
    border-radius: 5px;
    background: var(--bg-1);
    border: 1px solid var(--border-neutral);
    color: var(--text-mute);
    font-size: 10.5px;
    line-height: 1;
    width: fit-content;
  }
  .usage-badge-cache { color: var(--text-2); }
  .ctx-ring {
    display: inline-flex; align-items: center; gap: 6px;
    padding: 3px 8px 3px 4px;
    border: 1px solid var(--border-neutral-hi);
    border-radius: 7px;
    background: var(--bg-0);
    color: var(--text-2);
    font-size: 11px;
    flex-shrink: 0;
  }
  .ctx-ring svg { display: block; }
  .ctx-ring-track { stroke: var(--border-neutral); }
  .ctx-ring-fill { stroke: var(--text-2); transition: stroke-dasharray 0.2s ease; }
  .ctx-ring-fill--warn { stroke: #d18a4a; }
  .ctx-ring-fill--alert { stroke: #c75a4a; }
  .plan-chip-wrap { position: relative; display: inline-flex; flex-shrink: 0; }
  .plan-chip {
    display: inline-flex; align-items: center; gap: 5px;
    padding: 3px 8px 3px 6px;
    border: 1px solid var(--border-neutral-hi);
    border-radius: 7px;
    background: var(--bg-0);
    color: var(--text-2);
    font-size: 11px;
    flex-shrink: 0;
    white-space: nowrap;
    cursor: pointer;
    font-family: inherit;
  }
  .plan-chip:hover { border-color: var(--border-hi); color: var(--text-1); }
  .plan-chip svg { display: block; opacity: 0.7; }
  .plan-chip--warn { border-color: #d18a4a; color: #d18a4a; }
  .plan-chip--warn:hover { border-color: #d18a4a; color: #d18a4a; }
  .plan-chip--warn svg { opacity: 1; }
  .plan-chip--alert { border-color: #c75a4a; color: #c75a4a; }
  .plan-chip--alert:hover { border-color: #c75a4a; color: #c75a4a; }
  .plan-chip--alert svg { opacity: 1; }
  .plan-chip--err { color: var(--text-mute); border-style: dashed; }

  /* Click-outside backdrop. Transparent and unstyled — its only job
     is to absorb clicks anywhere outside the popover so the dialog
     dismisses without us wiring a window-level listener. */
  .plan-popover-backdrop {
    position: fixed; inset: 0;
    background: transparent;
    border: none;
    padding: 0;
    cursor: default;
    z-index: 90;
  }
  .plan-popover {
    position: absolute;
    top: calc(100% + 6px);
    right: 0;
    z-index: 100;
    min-width: 320px;
    padding: 12px 12px 10px;
    background: var(--bg-1);
    border: 1px solid var(--border-neutral-hi);
    border-radius: 10px;
    box-shadow: 0 12px 32px rgba(0, 0, 0, 0.45);
    color: var(--text-1);
    font-size: 12px;
    display: flex; flex-direction: column; gap: 10px;
  }
  .plan-popover-head {
    display: flex; align-items: center; justify-content: space-between;
    color: var(--text-mute);
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .plan-refresh {
    display: inline-flex; align-items: center; justify-content: center;
    width: 22px; height: 22px;
    padding: 0;
    border: none;
    background: transparent;
    color: var(--text-mute);
    border-radius: 5px;
    cursor: pointer;
  }
  .plan-refresh:hover:not(:disabled) { color: var(--text-1); background: var(--bg-0); }
  .plan-refresh:disabled { cursor: default; opacity: 0.5; }
  @keyframes plan-spin { from { transform: rotate(0deg); } to { transform: rotate(360deg); } }
  .plan-refresh--spinning { animation: plan-spin 0.9s linear infinite; }
  .plan-row { display: flex; flex-direction: column; gap: 4px; }
  .plan-row-head {
    display: flex; align-items: baseline; justify-content: space-between;
    gap: 12px;
  }
  .plan-row-label { color: var(--text-1); }
  .plan-row-num { color: var(--text-mute); font-size: 11px; white-space: nowrap; }
  .plan-bar {
    height: 4px;
    background: var(--border-neutral);
    border-radius: 2px;
    overflow: hidden;
  }
  .plan-bar-fill {
    height: 100%;
    border-radius: 2px;
    transition: width 0.25s ease, background 0.25s ease;
    min-width: 2px;
  }
  .plan-popover-foot {
    color: var(--text-mute);
    font-size: 10px;
    line-height: 1.45;
    padding-top: 6px;
    border-top: 1px dashed var(--border-neutral);
  }
  .plan-popover-foot code {
    font-size: 10px;
    color: var(--text-2);
  }
  .model-chip:hover { border-color: var(--border-hi); color: var(--text-1); }
  .model-chip:focus-within { border-color: var(--border-hi2); }
  .model-select { border: none; padding: 4px 20px 4px 0; background-color: transparent; }
  .model-select:hover { border: none; }
  .model-select:focus { border: none; }

  .repo-info-bar {
    display: flex; flex-wrap: wrap; align-items: center; gap: 5px;
    padding: 4px 14px 8px;
    background: var(--bg-1);
    font-size: 11px;
  }
  .repo-info-chip {
    display: inline-flex; align-items: center; gap: 4px;
    padding: 2px 7px;
    border-radius: 3px;
    background: var(--bg-2);
    color: var(--text-1);
    border: 1px solid var(--border-neutral);
  }
  .repo-info-clean { color: var(--success); border-color: rgba(217, 145, 60, 0.2); }
  .repo-info-dirty { color: var(--warning); border-color: rgba(229, 162, 42, 0.28); }
  .repo-info-dirty .repo-info-dot {
    width: 6px; height: 6px; border-radius: 50%;
    background: var(--warning);
  }
  .repo-info-notgit { color: var(--text-2); border-style: dashed; }
  .repo-info-noremote { color: var(--text-2); font-style: italic; }
  .repo-info-remote { color: var(--text-2); font-size: 10.5px; margin-left: 2px; opacity: 0.8; }

  .wt-chip {
    display: inline-flex; align-items: center; gap: 6px;
    padding: 5px 10px;
    border-radius: 7px;
    font-size: 11.5px;
    border: 1px dashed var(--border-hi);
    background: transparent;
    color: var(--text-1);
    transition: all 120ms;
    flex-shrink: 0;
  }
  .wt-chip:hover:not(:disabled) { color: var(--text-0); border-color: var(--accent); background: var(--accent-soft); }
  .wt-chip:disabled { opacity: 0.5; cursor: default; }
  .wt-chip--active {
    background: linear-gradient(135deg, rgba(238, 107, 31, 0.15), rgba(217, 145, 60, 0.08));
    border: 1px solid rgba(238, 107, 31, 0.4);
    color: var(--accent-bright);
    font-weight: 500;
  }
  .wt-chip--active:hover:not(:disabled) {
    border-color: var(--accent);
    background: linear-gradient(135deg, rgba(238, 107, 31, 0.22), rgba(217, 145, 60, 0.12));
  }
  .wt-dot {
    width: 7px; height: 7px; border-radius: 50%;
    background: var(--accent-bright);
    box-shadow: 0 0 8px var(--accent-glow);
    animation: wt-pulse 1.6s ease-in-out infinite;
  }
  @keyframes wt-pulse {
    0%, 100% { opacity: 0.55; transform: scale(0.9); }
    50% { opacity: 1; transform: scale(1.1); }
  }
  .wt-sub {
    font-size: 10px;
    color: var(--text-2);
    padding: 1px 6px;
    border-radius: 3px;
    background: var(--bg-2);
    margin-left: 2px;
    font-weight: 500;
    letter-spacing: 0.02em;
  }

  .wt-bar {
    position: relative;
    padding: 0 14px 10px;
    background: var(--bg-1);
  }
  .wt-menu {
    position: absolute; top: calc(100% - 6px); left: 14px; right: 14px;
    background: var(--bg-2);
    border: 1px solid var(--border-hi2);
    border-radius: 8px;
    box-shadow: 0 10px 30px rgba(0, 0, 0, 0.5);
    z-index: 20;
    padding: 4px;
    display: flex; flex-direction: column; gap: 1px;
  }
  .wt-menu-header {
    font-size: 10.5px; color: var(--text-2);
    padding: 8px 10px 6px;
    border-bottom: 1px solid var(--border-neutral);
    margin-bottom: 4px;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .wt-menu-item {
    display: flex; align-items: center; gap: 8px;
    padding: 7px 10px;
    border-radius: 5px;
    font-size: 12.5px; color: var(--text-1);
    text-align: left;
    transition: all 100ms;
  }
  .wt-menu-item:hover:not(:disabled) { background: var(--bg-3); color: var(--text-0); }
  .wt-menu-item:disabled { opacity: 0.5; cursor: default; }
  .wt-menu-item--danger { color: var(--error); }
  .wt-menu-item--danger:hover:not(:disabled) { background: rgba(214, 72, 44, 0.12); color: var(--error); }
  .wt-menu-item--apply { color: var(--accent-bright); font-weight: 500; }
  .wt-menu-item--apply:hover:not(:disabled) { background: var(--accent-soft); color: var(--accent); }
  .wt-menu-sep { height: 1px; background: var(--border-neutral); margin: 4px 0; }
  .cwd-label {
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
    font-size: 11px;
  }

  /* Chat tabs */
  .chat-tabs {
    display: flex;
    align-items: center; gap: 4px;
    border-bottom: 1px solid var(--border-neutral);
    padding: 8px 8px 8px 14px;
    background: var(--bg-1);
  }
  .chat-tabs-scroll {
    flex: 1;
    display: flex;
    align-items: center; gap: 2px;
    overflow-x: auto;
    scrollbar-width: none;
  }
  .chat-tabs-scroll::-webkit-scrollbar { display: none; }
  .chat-tab {
    display: inline-flex; align-items: center; gap: 6px;
    padding: 5px 9px;
    border-radius: 6px;
    font-size: 11.5px; color: var(--text-2);
    background: transparent;
    border: 1px solid transparent;
    transition: all 120ms;
    flex-shrink: 0;
    max-width: 160px;
  }
  .chat-tab:hover { color: var(--text-0); background: var(--bg-2); }
  .chat-tab.active {
    color: var(--text-0);
    background: var(--bg-2);
    border-color: var(--border-neutral-hi);
  }
  .chat-tab-mark {
    font-size: 9.5px; font-weight: 700;
    padding: 1px 5px;
    border-radius: 3px;
    background: rgba(16, 185, 129, 0.12);
    color: var(--accent-bright);
  }
  .chat-tab-title {
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .chat-tab-close {
    display: inline-flex; align-items: center; justify-content: center;
    width: 16px; height: 16px;
    border-radius: 3px;
    color: var(--text-mute);
    opacity: 0; transition: all 120ms;
  }
  .chat-tab:hover .chat-tab-close,
  .chat-tab.active .chat-tab-close { opacity: 1; }
  .chat-tab-close:hover { background: var(--bg-3); color: #fca5a5; }

  .chat-new {
    width: 26px; height: 26px;
    border-radius: 6px;
    color: var(--text-2);
    display: inline-flex; align-items: center; justify-content: center;
    transition: all 120ms;
    flex-shrink: 0;
  }
  .chat-new:hover { background: var(--bg-2); color: var(--accent-bright); }

  .chat-empty {
    flex: 1;
    display: flex; flex-direction: column; align-items: center; justify-content: center;
    gap: 10px; padding: 40px 20px;
    text-align: center;
  }
  .chat-empty-title {
    font-size: 13px; font-weight: 500; color: var(--text-1);
  }
  .chat-empty-sub {
    font-size: 12px; color: var(--text-2);
    max-width: 280px; line-height: 1.55;
  }

  /* @-autocomplete popover — absolute-positioned above the textarea so it
     floats over the chat transcript instead of pushing it down. Matches
     the pill-menu + dropdown styles so it feels like one family. */
  .mention-pop {
    position: absolute; left: 0; right: 0; bottom: calc(100% + 6px);
    background: var(--bg-2);
    border: 1px solid var(--border-hi);
    border-radius: 10px;
    box-shadow: 0 12px 32px rgba(0, 0, 0, 0.5);
    padding: 4px;
    display: flex; flex-direction: column; gap: 1px;
    z-index: 30;
    max-height: 260px; overflow-y: auto;
    animation: mention-fade 100ms ease-out;
  }
  @keyframes mention-fade {
    from { opacity: 0; transform: translateY(4px); }
    to { opacity: 1; transform: none; }
  }
  .mention-pop-head {
    font-size: 10px; font-weight: 600; letter-spacing: 0.05em;
    color: var(--text-mute); text-transform: uppercase;
    padding: 7px 10px 5px;
    border-bottom: 1px solid var(--border-neutral);
    margin-bottom: 3px;
  }
  .mention-item {
    display: flex; align-items: center; gap: 8px;
    padding: 6px 10px;
    border-radius: 6px;
    font-size: 12px; color: var(--text-1);
    text-align: left;
    cursor: pointer; border: none; background: transparent;
    width: 100%;
  }
  .mention-item:hover, .mention-item.active {
    background: var(--accent-soft); color: var(--accent-bright);
  }
  .mention-item-kind {
    width: 22px; height: 18px; border-radius: 4px;
    display: inline-flex; align-items: center; justify-content: center;
    font-size: 9px; font-weight: 700;
    flex-shrink: 0;
  }
  .mention-item-kind--jira { background: rgba(59, 130, 246, 0.18); color: var(--blue-bright); }
  .mention-item-kind--github { background: rgba(139, 92, 246, 0.18); color: #c7a8ff; }
  .mention-item-kind--sentry { background: rgba(247, 100, 87, 0.18); color: #f76457; }
  .mention-item-kind--file { background: var(--bg-3); color: var(--text-2); }
  .mention-item-id {
    font-size: 11px; color: var(--text-2); min-width: 56px;
    flex-shrink: 0;
  }
  .mention-item.active .mention-item-id { color: var(--accent-bright); }
  .mention-item-title {
    flex: 1; font-size: 12px; color: inherit;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .mention-item-hint {
    font-size: 10px; color: var(--text-mute);
    padding: 1px 6px; border-radius: 3px;
    background: var(--bg-1);
    flex-shrink: 0; max-width: 120px;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
</style>
