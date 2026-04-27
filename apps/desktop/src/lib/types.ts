// Shared types used by workbench column components that live outside of
// `+page.svelte`. Kept here (not in `$lib/data.ts`) because these describe
// UI/runtime state — not the GitHub/Jira payload shapes that `data.ts`
// models. Re-exporting these from the columns (plus +page.svelte) keeps
// the import graph flat.

export type PanelKind = 'github' | 'jira' | 'sentry' | 'claude' | 'cursor' | 'editor';

/** One live instance of a column in a workbench. Editors and chat columns can
 *  have multiple instances side-by-side; github/jira are effectively singletons
 *  (see `addPanelInstance` in layout.svelte.ts).
 *
 *  `name` is a human-readable handle drawn from a pool of art / artist /
 *  monument names (Mona-Lisa, Da-Vinci, Parthenon, …). Shown in the column
 *  header and used when picking a target for linking ("Link to Claude
 *  (Mona-Lisa)"). Auto-generated on creation, unique within a workbench. */
export type PanelInstance = {
  id: string;
  kind: PanelKind;
  width: number;
  name: string;
};

/** Named preset of a column layout. The user can switch between many. */
export type Workbench = {
  id: string;
  name: string;
  instances: PanelInstance[];
};

/** Ordered chunk that makes up an assistant message's body. The stream
    parser appends events as they arrive, merging consecutive same-kind
    runs (a wall of `text` deltas collapses into one event; a sequence
    of tool_use calls between two text blocks groups into one `trace`
    event with multiple segments). The chat column renders events in
    order — text → markdown bubble, trace → collapsed "✓ N steps" pill.
    Without this the prior architecture lost interleaving (all tool
    hints fell into one pill at the top + all text concatenated below). */
export type MessageEvent =
  | { kind: 'text'; body: string }
  | { kind: 'trace'; segments: string[] }
  /** Inline diff card for an Edit/Write/MultiEdit tool call. Renders as a
   *  collapsed file pill that expands to a unified diff with Keep / Revert
   *  buttons — same UX pattern as Cursor's chat. We intercept the tool call
   *  *after* the agent has already mutated the file (Claude/cursor-agent
   *  CLIs don't expose a pre-tool hook), so the visual state always starts
   *  at `applied`. The status flips to `reverted` if the user undoes the
   *  change, or `error` if the revert itself failed (e.g. the file moved).
   *
   *  `oldText` is the literal `old_string` from the Edit (or empty for
   *  Write to a new file). `newText` is `new_string` for Edit / `content`
   *  for Write — what now lives in the file. `isCreate` distinguishes "this
   *  edited an existing file" from "this created the file from nothing"
   *  so revert can choose between rewriting and deleting. */
  | {
      kind: 'edit';
      toolId: string;
      filePath: string;
      oldText: string;
      newText: string;
      isCreate: boolean;
      /** True when the agent called `Write` (full-file overwrite) rather
       *  than `Edit` / `MultiEdit` (in-place substitution). Different
       *  revert semantics — Edit does `replace(newText, oldText)` once,
       *  Write does `fs::write(filePath, oldText)` (or `remove_file` if
       *  `isCreate`). The card UI also picks a different verb ("Wrote"
       *  vs "Edited"). */
      wholeFile: boolean;
      /** Diff card's lifecycle state.
       *    - `loading` — Write event was just pushed; we're awaiting the
       *      async `git show HEAD:<file>` that fetches the pre-agent
       *      version. The card renders a placeholder so the user
       *      doesn't see a flash of "+N" without context.
       *    - `applied` — change is on disk and the diff is fully
       *      populated. Default state for Edit/MultiEdit (no async
       *      fetch needed).
       *    - `reverted` — user clicked Revert; the inverse write
       *      succeeded.
       *    - `error` — Revert (or the git fetch) failed. `note` carries
       *      the message. */
      status: 'loading' | 'applied' | 'reverted' | 'error';
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
};

export type Mention = {
  source: 'github' | 'jira' | 'sentry' | 'file';
  externalId: string;
  title: string;
  body: string | null;
  isDir?: boolean;
};

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
    }
  | {
      id: string;
      kind: 'switch_cwd';
      path: string;
      reason: string;
      status: 'pending' | 'executing' | 'done' | 'error';
      result?: string;
    }
  | {
      id: string;
      kind: 'bash';
      command: string;
      reason: string;
      status: 'pending' | 'executing' | 'done' | 'error';
      result?: string;
      exitCode?: number;
    };

export type ClaudeSession = {
  id: string;
  title: string;
  mentions: Mention[];
  messages: ClaudeMessage[];
  input: string;
  sending: boolean;
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
  /** Which subset of MCP tools to expose to Claude this session. Each MCP
      tool schema costs ~150-300 tokens of system-prompt overhead, and we
      ship 60+ tools across Jira/GitHub/Sentry/Memory/App. Most chats only
      need a slice — restricting to a profile cuts startup token cost by
      40-60% and reduces process count (sidecars are spawned only for the
      servers in the profile).
        - 'coding' (new-session default): App nav + Memory only.
        - 'github' / 'jira' / 'sentry': single-source focus — that source
           full-access plus Memory + App nav. Other sources skipped.
        - 'triage': Read-only across all three sources (Jira + GitHub +
           Sentry) + Memory + App nav. For "what's the state of X" chats
           that don't edit anything.
        - 'all' (legacy / null backfill): every tool wired, like before
           profiles existed. */
  claudeToolProfile:
    | 'all'
    | 'coding'
    | 'github'
    | 'jira'
    | 'sentry'
    | 'triage'
    | null;
  /** Effective context size at the end of the most recent turn (input +
      cache_read + cache_creation from the last assistant API call).
      Drives the context-window % indicator chip. 0 = no turn yet. */
  lastContextSize: number;
  /** When true, the session's `cwd` tracks the Editor's open folder live —
      pick a new folder in the Editor and every linked chat follows. The
      link is broken the moment the user picks an explicit cwd on the
      session (via pickCwd / clearCwd / worktree). */
  linkedToEditor: boolean;
  /** Which Editor instance this session is linked to. When null and
      `linkedToEditor` is true, falls back to the first editor in the
      active workbench. Explicit id lets the user keep a stable target even
      when multiple Editor columns are open. */
  linkedToEditorInstanceId: string | null;
  /** Which column instance this session is attached to. Null means the session
      "floats" and will reattach to the first matching-kind column it finds. */
  columnInstanceId: string | null;
  /** One-shot recap to inject into the system prompt on the NEXT turn. Set
      whenever cwd changes — Claude / cursor-agent scope conversations by
      project, so a cwd switch starts a fresh CLI conversation that doesn't
      remember prior turns. Stuffing the last few UI-side messages back in
      keeps continuity for the user without permanently inflating prompts.
      Cleared after the next turn ships. */
  cwdSwitchRecap: string | null;
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

