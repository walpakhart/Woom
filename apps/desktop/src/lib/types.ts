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

export type ClaudeMessage = {
  role: 'system' | 'user' | 'assistant';
  content: string;
  at: string;
  /** Concatenated `thinking` content blocks the agent emitted before the
      final answer. Surfaced as a collapsed "Thinking ✓" pill in the UI
      that the user can expand to read. Only set on assistant messages
      from thinking-capable models (Claude with `*-thinking-*` model
      family, Cursor with reasoning models). Persisted alongside the
      session so a reload still shows the same pill. */
  thinking?: string;
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

