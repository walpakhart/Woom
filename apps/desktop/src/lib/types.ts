// Shared types used by solo-mode app components that live outside of
// `+page.svelte`. Kept here (not in `$lib/data.ts`) because these describe
// UI/runtime state — not the GitHub/Jira payload shapes that `data.ts`
// models.

/** One of the solo-mode kinds. There is exactly one app of each kind in
 *  the running shell; per-kind state slots are keyed by the matching
 *  `APP_INSTANCE_IDS[kind]` from `$lib/state/layout.svelte`. */
export type PanelKind = 'github' | 'jira' | 'sentry' | 'claude' | 'cursor' | 'editor' | 'canvas' | 'terminal';

/** Ordered chunk that makes up an assistant message's body. The stream
    parser appends events as they arrive, merging consecutive same-kind
    runs (a wall of `text` deltas collapses into one event; a sequence
    of tool_use calls between two text blocks groups into one `trace`
    event with multiple segments). The chat surface renders events in
    order — text → markdown bubble, trace → collapsed "✓ N steps" pill.
    Without this the prior architecture lost interleaving (all tool
    hints fell into one pill at the top + all text concatenated below). */
export type MessageEvent =
  | { kind: 'text'; body: string }
  | { kind: 'trace'; segments: string[] }
  /** Inline diff card for an Edit/Write/MultiEdit/Delete tool call. Renders
   *  as a collapsed file pill that expands to a unified diff with Keep /
   *  Revert (or Restore for deletions) buttons — same UX pattern as
   *  Cursor's chat. We intercept the tool call *after* the agent has
   *  already mutated the file (Claude/cursor-agent CLIs don't expose a
   *  pre-tool hook), so the visual state always starts at `applied`.
   *  The status flips to `reverted` if the user undoes the change, or
   *  `error` if the revert itself failed (e.g. the file moved).
   *
   *  Three flavours, distinguished by the `isCreate` / `isDelete` flags:
   *    - Default (both false): edit / write of an existing file.
   *      `oldText` is the prior contents, `newText` what's there now.
   *    - `isCreate=true`: Write created a brand-new file. `oldText` is
   *      empty and Revert deletes the file.
   *    - `isDelete=true`: file was deleted. `oldText` is the captured
   *      prior contents (from cursor's `prevContent` or `git show
   *      HEAD:`), `newText` is empty, and "Revert" is rendered as
   *      "Restore" — it re-creates the file. Mutually exclusive with
   *      `isCreate`. */
  | {
      kind: 'edit';
      toolId: string;
      filePath: string;
      oldText: string;
      newText: string;
      isCreate: boolean;
      /** True when the tool call removed the file. Mutually exclusive
       *  with `isCreate`. The card flips Revert → Restore (re-creates
       *  the file from `oldText`) and Reapply → Re-delete. Defaults to
       *  false for edit/write events. */
      isDelete?: boolean;
      /** True when the agent called `Write` (full-file overwrite) rather
       *  than `Edit` / `MultiEdit` (in-place substitution). Different
       *  revert semantics — Edit does `replace(newText, oldText)` once,
       *  Write does `fs::write(filePath, oldText)` (or `remove_file` if
       *  `isCreate`). The card UI also picks a different verb ("Wrote"
       *  vs "Edited"). Always true for `isDelete` events (the inverse
       *  of "delete this file" is a full rewrite). */
      wholeFile: boolean;
      /** Diff card's lifecycle state.
       *    - `loading` — Write/Delete event was just pushed; we're
       *      awaiting the async `git show HEAD:<file>` that fetches the
       *      pre-agent version (used as a fallback when the CLI didn't
       *      hand us `prevContent`). The card renders a placeholder so
       *      the user doesn't see a flash of "+N" / "−N" without
       *      context.
       *    - `applied` — change is on disk and the diff is fully
       *      populated. Default state for Edit/MultiEdit (no async
       *      fetch needed). The "pending changes" bar above the
       *      composer counts cards in this state.
       *    - `kept` — user explicitly approved the change ("Keep" on
       *      the card or "Keep all" on the bar). Disk is unchanged
       *      (still `newText`); the card swaps Revert/Keep for an
       *      "Unkeep" button that flips back to `applied`. Symmetrical
       *      to `reverted` so the user can always undo their decision.
       *    - `reverted` — user clicked Revert / Restore; the inverse
       *      write (or re-create) succeeded. Card shows "Reapply".
       *    - `error` — Revert / Restore (or the git fetch) failed.
       *      `note` carries the message. */
      status: 'loading' | 'applied' | 'kept' | 'reverted' | 'error';
      /** Optional explanation when `status === 'error'` — surfaced on the
       *  card so the user understands why Revert didn't apply. */
      note?: string;
    };

/** Token-accounting snapshot from a single Claude API call. Each
 *  assistant turn produces one of these (sometimes several when the
 *  agent makes tool-using sub-steps); we stamp the LAST one onto the
 *  rendered message so the UI badge shows the full cost of the final
 *  reply (its `cache_read` reflects the entire prior conversation, so
 *  it's the most informative single number to surface). */
export type ClaudeUsage = {
  inputTokens: number;
  cacheCreationTokens: number;
  cacheReadTokens: number;
  outputTokens: number;
  /** Effective context size for this call: `input + cache_read +
   *  cache_creation`. The total bytes the model saw on this hop —
   *  drives the context-window % indicator. */
  contextSize: number;
  /** Model id reported by the CLI for this call. Used for $-cost
   *  calculation (different rates per model) and to label the badge
   *  if the user mid-session swapped models. */
  model: string | null;
  /** Whether the session was in Fast mode when this snapshot was
   *  stamped. The CLI doesn't report Fast in its `usage` block —
   *  `sessions.svelte.ts::updateLastAssistantUsage` copies the
   *  session's `fastMode` flag onto each snapshot at stamp time so
   *  `costForUsage` can pick the right RATE_TABLE row. */
  fastMode?: boolean;
};

export type ClaudeMessage = {
  role: 'system' | 'user' | 'assistant';
  content: string;
  at: string;
  /** Last `usage` snapshot stamped on this assistant turn. Used by the
   *  per-message badge. Only set on assistant messages once the turn
   *  has ended (we keep overwriting on each sub-step so the final
   *  value reflects the real cost of the full turn). */
  usage?: ClaudeUsage;
  /** When set, this assistant turn ended prematurely — Woom interrupted
   *  the CLI before it could finish. Sources:
   *  - `'quota'` — quota guard tripped during streaming (Phase 2).
   *  - `'user'` — user clicked Stop in the composer.
   *  - `'crash'` — main app crashed mid-stream; restart-time recovery
   *    flips a previously-running session into this state.
   *  ResumePill renders for sessions with `interrupted === 'quota'`
   *  on the LAST message; the field is per-message so multi-turn
   *  histories with one paused turn don't all show pills. */
  interrupted?: 'quota' | 'user' | 'crash';
  /** When set, this assistant message hosts a Dynamic Workflow (Phase 4).
   *  ChatThread renders `<DynamicWorkflowCard>` reading the workflow
   *  from `dwState.workflows` by this id. The text content carries the
   *  user's `/dw <prompt>` echo so the chat stays readable when the
   *  card is collapsed / off-screen. */
  dwWorkflowId?: string;
  /** Concatenated `thinking` content blocks the agent emitted before the
      final answer. Surfaced as a collapsed "Thinking ✓" pill in the UI
      that the user can expand to read. Only set on assistant messages
      from thinking-capable models (Claude with `*-thinking-*` model
      family, Cursor with reasoning models). Persisted alongside the
      session so a reload still shows the same pill. */
  thinking?: string;
  /** LEGACY — concatenated `formatToolUse` lines (one big string with
      `\n\n` separators). Kept so old persisted messages still render.
      New messages use `events` instead, which preserves interleaving
      between text and tool-use blocks. */
  trace?: string;
  /** Ordered text/trace events. When present, the renderer uses this
      instead of `content` + `trace` so the chat shows tool calls right
      where they happened in the conversation, not all jammed into one
      pill at the top. `content` is still maintained as a flat
      concatenation of every text-event body (used for search /
      back-compat / replaceLastAssistant). */
  events?: MessageEvent[];
  /** Image attachments the user sent on this turn. Stored alongside the
      message so the transcript still shows what was sent after the agent
      replies (the chip strip above the composer is cleared on send). Each
      entry holds the absolute path (rendered via `convertFileSrc`) and the
      basename for the alt text. Only set on `role: 'user'` messages. */
  images?: { path: string; name: string }[];
  /** When true, the chat thread DOES NOT render this message — it's
   *  invisible orchestration traffic that the agent's CLI transcript
   *  needs to see (so `--resume` history stays correct) but the user
   *  shouldn't have to scroll past. Set by SDD when phase prompts are
   *  injected: the giant spec/plan/phase template lives on the agent's
   *  side but stays out of the user's visible chat. Pure UX filter —
   *  search / export / hydrate all treat hidden + visible alike. */
  hidden?: boolean;
};

export type Mention = {
  /** Where the mention was sourced from. `chat` is added in v8 for
   *  "@-mention another chat session" — used by the inline mention
   *  picker so users can hand a session's context to another agent.
   *  `terminal` is added in v8 for "select-text-in-terminal → Apply
   *  to agent" — body holds the captured shell output so the agent
   *  reads the literal bytes instead of trying to resolve a path. */
  source: 'github' | 'jira' | 'sentry' | 'file' | 'chat' | 'terminal';
  externalId: string;
  title: string;
  body: string | null;
  isDir?: boolean;
  /** True when this mention represents a file/image dragged or pasted
   *  from OUTSIDE the app (Finder drop, Cmd+V image, screenshot). The
   *  composer surfaces these as removable chips in a dedicated strip.
   *  Mentions added via the @-picker or in-app editor-tree drag stay
   *  unflagged — those live as inline `@token` references in the
   *  prompt text and don't need a chip. */
  attached?: boolean;
};

/** `waitId` is set on cards created via the synchronous IPC path
 *  (sidecar's `propose_*` blocks the agent's MCP call until the card
 *  resolves). When present, the action executor calls
 *  `resolve_action_wait` with this id after running the action so
 *  the sidecar's MCP response carries the actual outcome — and the
 *  agent reacts in the SAME turn. Cards from the legacy fire-and-
 *  forget path (no IPC available) leave waitId undefined; their
 *  outcome rides the old `pendingActionResults` queue + manual
 *  next-turn drain. */
export type ClaudeAction =
  | {
      id: string;
      kind: 'commit';
      message: string;
      body: string;
      push: boolean;
      note: string;
      status: 'pending' | 'executing' | 'done' | 'error';
      result?: string;
      waitId?: string;
    }
  | {
      id: string;
      kind: 'pr';
      title: string;
      body: string;
      base: string;
      draft: boolean;
      note: string;
      status: 'pending' | 'executing' | 'done' | 'error';
      result?: string;
      waitId?: string;
    }
  | {
      id: string;
      kind: 'switch_cwd';
      path: string;
      reason: string;
      status: 'pending' | 'executing' | 'done' | 'error';
      result?: string;
      waitId?: string;
    }
  | {
      id: string;
      kind: 'bash';
      command: string;
      reason: string;
      status: 'pending' | 'executing' | 'done' | 'error';
      result?: string;
      exitCode?: number;
      waitId?: string;
    }
  | {
      id: string;
      kind: 'question';
      /** Shape of the question card:
       *   - `single`  — radio list, click auto-submits.
       *   - `multi`   — checkbox list + Submit button.
       *   - `text`    — free-form input only, no clickable options.
       *   - `confirm` — Yes / No buttons, no `options` needed.
       *  Defaults to `single` (or `multi` when the legacy
       *  `multi_select=true` flag was set). */
      questionKind: 'single' | 'multi' | 'text' | 'confirm';
      /** The literal question text — rendered as the card's header. */
      question: string;
      /** Short context blurb shown above the option list (optional). */
      header?: string;
      /** Clickable options. Empty for `questionKind=text|confirm`. */
      options: { label: string; description?: string }[];
      /** Legacy mirror of `questionKind === 'multi'` — kept for
       *  back-compat with older serialised sessions. New code should
       *  read `questionKind`. */
      multiSelect?: boolean;
      status: 'pending' | 'executing' | 'done' | 'error';
      /** Chosen label(s) — set on submit. Surfaces in the executed-card
       *  history so the user can see what they picked. */
      chosen?: string[];
      /** Free-form note the user wrote in the "Other" field. Honoured
       *  when present even if no option was clicked. */
      other?: string;
      result?: string;
      waitId?: string;
    };

export type ClaudeSession = {
  id: string;
  title: string;
  mentions: Mention[];
  messages: ClaudeMessage[];
  input: string;
  sending: boolean;
  /** Messages typed while a turn was in flight. Each item is a fully-
   *  composed user prompt — `Send` while `sending=true` pushes the
   *  current input here, clears the visible composer, and the queue
   *  drains FIFO when the turn finishes. Optional / undefined =
   *  empty queue. */
  pendingQueue?: { text: string; mentions: Mention[] }[];
  cwd: string | null;
  worktreePath: string | null;
  worktreeBranch: string | null;
  worktreeRepo: string | null;
  actions: ClaudeAction[];
  claudeUuid: string;
  claudeResumable: boolean;
  agentKind: 'claude' | 'cursor';
  cursorModel: string | null;
  /** Model id passed to `claude --model`. Null = let the CLI pick (which on
      a Pro/Max subscription means Opus 4.7 — the default that burns the
      5h quota fastest). New sessions default to `claude-sonnet-4-6`; users
      opt in to Opus per-session via the model chip. */
  claudeModel: string | null;
  /** Effective context size at the end of the most recent turn (input +
      cache_read + cache_creation from the last assistant API call).
      Drives the context-window % indicator chip. 0 = no turn yet. */
  lastContextSize: number;
  /** When set, the session is bound to a Canvas in the workspace
      library. The `woom-app` sidecar's `mcp__app__canvas_*` tools
      target this canvas; a brief canvas summary is injected into the
      system prompt at every turn so the agent knows what's there. Null
      = no canvas link (default). The id is the **library** canvas id,
      not an app-instance id — multiple Canvas instances can pin the
      same canvas; the agent talks to the underlying record. */
  linkedCanvasId: string | null;
  /** When true, the session's `cwd` tracks the Editor's open folder live —
      pick a new folder in the Editor and every linked chat follows. The
      link is broken the moment the user picks an explicit cwd on the
      session (via pickCwd / clearCwd / worktree). */
  linkedToEditor: boolean;
  /** Which Editor instance this session is linked to. When null and
      `linkedToEditor` is true, falls back to the first editor in the
      active solo. Explicit id lets the user keep a stable target even
      when multiple Editor instances are open. */
  linkedToEditorInstanceId: string | null;
  /** Which Terminal instance this session is bound to. When set, the
   *  agent's MCP `terminal_run` / `terminal_write` tools default to
   *  this id (it's still listed by `terminal_list` so the agent can
   *  pick a different one if it wants). Auto-link convention: linking
   *  here also writes the linked-editor's repoPath into the terminal's
   *  cwd on next spawn — see `linkSessionToTerminal` in
   *  `sessions.svelte.ts`. */
  linkedTerminalInstanceId: string | null;
  /** Which agent-app instance (Claude or Cursor) this session lives in.
      Null means the session "floats" and will reattach to the first
      matching-kind app instance it encounters. */
  agentInstanceId: string | null;
  /** One-shot recap to inject into the system prompt on the NEXT turn. Set
      whenever cwd changes — Claude / cursor-agent scope conversations by
      project, so a cwd switch starts a fresh CLI conversation that doesn't
      remember prior turns. Stuffing the last few UI-side messages back in
      keeps continuity for the user without permanently inflating prompts.
      Cleared after the next turn ships. */
  cwdSwitchRecap: string | null;
  /** Permission mode for the next turn (Claude Code parity §4). `plan`
   *  tells the agent it may only read/inspect — no edits, no mutating
   *  bash. `default` is the normal mode. Cycle via Shift+Tab in the
   *  composer. The mode is appended to the system-prompt suffix so the
   *  agent gates its own tool choice; we don't (yet) own tool dispatch
   *  to enforce hard. Persisted per-session so a "plan mode" session
   *  stays in plan after a window close. */
  permissionMode?: 'default' | 'plan';
  /** Whether the RTK output-compression hook is active for this
   *  session. When false, the spawned `claude` CLI gets
   *  `WOOM_RTK_SESSION_DISABLED=1` in its environment so the
   *  woom-managed PreToolUse wrapper script passes Bash output
   *  through unchanged. Default `true` — RTK is opt-out, not opt-in.
   *  Persisted (see `sessions_serialize.ts`). */
  rtkEnabled?: boolean;
  /** Whether Claude Opus Fast mode is on for this session. Applies
   *  only when `claudeModel` is a Fast-capable variant
   *  (`claude-opus-4-8*`). Fast = 2.5× faster output at 2× cost via
   *  Anthropic's dedicated endpoint. Default `false` (standard
   *  pricing + speed). Persisted across reloads. */
  fastMode?: boolean;
  /** Quota guard (Phase 2). When the 5H/7D quota tripped during an
   *  in-flight turn OR the user explicitly chose «wait» in the
   *  pre-send modal, this flag is set; `resumeAt` carries the unix-ms
   *  of the relevant bucket's reset. ResumePill renders below the
   *  interrupted assistant message with a live countdown until
   *  `resumeAt`, then becomes an active Resume button. Cleared by
   *  `clearResumeState` on reset detection or user-initiated send. */
  awaitingResume?: boolean;
  /** Unix-ms when the quota bucket that paused this session resets.
   *  Drives the countdown shown on ResumePill + the modal. Null /
   *  undefined when `awaitingResume` is false. */
  resumeAt?: number;
  /** Why we paused. Mirrors the per-message `ClaudeMessage.interrupted`
   *  but lives on the session for quick guard-check reads. */
  interruptedReason?: 'quota' | 'user' | 'crash';
  /** Manual pin from the agent-view dashboard. Pinned sessions sort to
   *  the top of their group + survive in the dashboard's "Pinned"
   *  bucket regardless of their working status. */
  pinned?: boolean;
  /** Per-cwd CLI session ids. Key = cwd path (the actual string we passed
      as `--cwd`). Value = the claudeUuid that the CLI accepted for that
      project. Lets us *resume* an old conversation when the user moves
      back to a previously-visited cwd, instead of starting fresh every
      time. Populated as we leave each cwd (we stash the current uuid
      under the cwd we're leaving), consulted on entry to a cwd we have
      a record of. CLI-kind specific — cleared on switchAgentKind since
      a cursor-agent chat id can't resume in claude and vice versa. */
  cwdUuids: Record<string, string>;
  /** True when the agent's last turn ended with one or more pending
      action cards (commit / PR / bash / switch_cwd) that block the
      next step of its plan. Set in sendClaudeMessage's success path,
      cleared on action resolution → automatic follow-up turn. UI
      shows a "waiting for your approval" hint instead of the idle
      input prompt so the user knows the agent is paused on them. */
  awaitingApproval: boolean;
  /** Outcomes from action cards (commit / PR / bash / switch_cwd) that
      have run since the agent's last turn. Two consumers:
        1. UI: each outcome gets appended to `messages` as an action-
           result chip when the chat is quiescent (after streaming
           ends). Mid-stream appends would shift "last message"
           position and silently drop assistant deltas — see
           `flushActionResultsToUI`.
        2. Agent: drained on the NEXT runAgentRequest call (manual or
           auto-fired) and prepended to the prompt as a "since-last-
           turn outcomes" block. The CLI's `--resume` history doesn't
           include these (Woom-side annotations), so this is the
           only channel the agent has for learning whether its commit
           push succeeded, what stderr a bash card returned, etc.
      Persisted across app restarts so a result that arrived right
      before quit isn't lost when the user reopens. Items have a
      `flushedToUI` flag to track UI-side delivery independently from
      agent-side delivery. */
  pendingActionResults: PendingActionResult[];
  /** Crash-detection marker. Stamped at the start of every
      `runAgentRequest` call (user pressed Send → user message has been
      appended → CLI process is about to be invoked), cleared in the
      `finally` block when that turn reaches a terminal state (success,
      error, or normal stop). If the app dies mid-turn (force-quit, OS
      crash, hardware failure, agent process killed without the JS
      promise settling) this stays populated on disk. On the next boot
      `hydrateSession` reads it, sets `interrupted=true`, and clears
      the marker — the UI can then surface a "previous turn was
      interrupted, continue?" affordance. The user message index lets
      the recap reference the exact prompt that was in flight. Null
      whenever no turn is running. */
  pendingTurn: { startedAt: number; userMessageIndex: number } | null;
  /** Derived (NOT persisted): true when this session was hydrated from
      a disk record whose `pendingTurn` was non-null. Indicates the
      prior app process died mid-turn. Reset to false when the user
      either dismisses the recovery affordance or starts a new turn —
      caller is responsible for that lifecycle. */
  interrupted?: boolean;
};

/** One outcome from an action card. Lives on the session until both
 *  `flushedToUI` is true (chip has been appended to `messages`) AND
 *  the next agent turn has fired (drained for prompt prefix). */
export type PendingActionResult = {
  /** True for `done`-status outcomes, false for `error`. Drives the
      ✓/✗ marker the UI chip and agent prompt prefix display. */
  ok: boolean;
  /** The action kind — used to label outcomes in the agent's prompt
      block ("commit: …", "bash: …") so the agent doesn't have to
      infer kind from prose. */
  kind: 'commit' | 'pr' | 'bash' | 'switch_cwd';
  /** Multi-line summary as built by the executor. Includes commit
      message + sha + push diagnostics, or bash command + stdout/
      stderr + exit code, or PR title + URL, etc. Free-form prose
      because every kind has different relevant fields. */
  summary: string;
  /** ISO timestamp of when the action resolved. Used as the chat
      message's `at` when flushed to UI, so chips appear in
      chronological context. */
  at: string;
  /** True after the result has been appended to `messages` for the UI
      chip. The drain-for-agent step doesn't gate on this — agent
      delivery and UI display are tracked independently. */
  flushedToUI: boolean;
};

export interface RepoInfo {
  is_git: boolean;
  root: string | null;
  current_branch: string | null;
  remote_url: string | null;
  remote_name: string | null;
  dirty_count: number;
  untracked_count: number;
  ahead: number;
  behind: number;
  missing: boolean;
}

/* Dynamic Workflows (SDD `sdd-98a42f3bdb` Phase 4) — Anthropic's
 * research-preview feature replicated locally. Planner emits a JSON
 * plan with up to 20 subagents; each runs in an isolated git worktree;
 * verifier synthesises the final answer. State lives in
 * `state/dw.svelte.ts`; the Rust side mirrors these shapes via serde. */

export interface DwSubagent {
  id: string;
  prompt: string;
  cwdStrategy: 'inherit' | 'subpath';
  cwdSubpath?: string;
  expectedArtifacts: string[];
  status: 'queued' | 'streaming' | 'done' | 'failed' | 'cancelled';
  claudeUuid?: string;
  worktreePath?: string;
  result?: string;
  error?: string;
  tokensIn: number;
  tokensOut: number;
  costUsd: number;
}

export interface DynamicWorkflow {
  id: string;
  sessionId: string;
  userPrompt: string;
  status:
    | 'planning'
    | 'awaiting_approval'
    | 'running'
    | 'verifying'
    | 'done'
    | 'failed'
    | 'cancelled';
  planRationale?: string;
  subagents: DwSubagent[];
  verifierPrompt?: string;
  verifierResult?: string;
  finalAnswer?: string;
  budgetCapUsd: number;
  totalCostUsd: number;
  createdAt: number;
  startedAt?: number;
  completedAt?: number;
}

