// Modal registry — single $state object holding all dialog payloads. Replaces
// the 11 individually-bound modal vars that used to live in `+page.svelte`.
//
// Conventions:
//  - One field per modal kind. `null` = closed.
//  - Mutate via `openModal('x', payload)` / `closeModal('x')` (or
//    `patchModal('x', partial)` for in-flight state like `busy: true`).
//  - Modal components read `modalsState.x` directly; they don't take the
//    payload as a prop. This keeps each modal component prop-free and the
//    parent stays slim.

import type {
  ChangedFile,
  ClaudeStatus,
  CommitDetail,
  CommitEntry,
  ConnectionMeta,
  CursorStatus,
  JiraIssueType,
  JiraProject,
  JiraSprint,
  JiraUserSummary,
  RepoBranch,
  CommitEntry as RepoCommitEntry
} from '$lib/data';

// --- Per-modal payload types -------------------------------------------------

export type ReviewEvent = 'APPROVE' | 'REQUEST_CHANGES' | 'COMMENT';
export type MergeMethod = 'merge' | 'squash' | 'rebase';

export interface CommitModalState {
  commit: CommitEntry;
  detail: CommitDetail | null;
  loading: boolean;
  error: string | null;
  expanded: Set<string>;
}

export interface UserPickerModalState {
  query: string;
  results: JiraUserSummary[];
  loading: boolean;
  error: string | null;
}

export interface JiraConnectModalState {
  workspace: string;
  email: string;
  token: string;
  error: string | null;
  busy: boolean;
}

export interface SentryConnectModalState {
  /** Base host — defaults to https://sentry.io. Self-hosted users override. */
  host: string;
  /** Org slug — the URL handle (acme-co), not the display name. */
  organization_slug: string;
  /** Auth Token from <host>/settings/account/api/auth-tokens/. Needs
   *  org:read, project:read, event:read scopes. */
  token: string;
  error: string | null;
  busy: boolean;
}

export interface ClaudeStatusModalState {
  status: ClaudeStatus | null;
  loading: boolean;
}

export interface CursorStatusModalState {
  status: CursorStatus | null;
  loading: boolean;
}

export interface PatModalState {
  conn: ConnectionMeta;
  token: string;
  error: string | null;
  busy: boolean;
}

export interface CommentModalState {
  body: string;
  busy: boolean;
  error: string | null;
}

export interface ReviewModalState {
  event: ReviewEvent;
  body: string;
  busy: boolean;
  error: string | null;
}

export interface MergeModalState {
  method: MergeMethod;
  busy: boolean;
  error: string | null;
}

export interface ConfirmModalState {
  title: string;
  body: string;
  confirmText: string;
  danger?: boolean;
  busy: boolean;
  onConfirm: () => Promise<void>;
  /** Fires when the user dismisses the modal (Cancel button / ESC /
   *  backdrop click). Lets callers re-arm state that gated the
   *  modal's open path — e.g. the close-window guard resets its
   *  re-entry flag here so a second close-request can fire. */
  onCancel?: () => void | Promise<void>;
}

export interface JiraCreateModalState {
  projectKey: string;
  projects: JiraProject[];
  projectsLoading: boolean;
  issueTypes: JiraIssueType[];
  issueTypeName: string;
  summary: string;
  description: string;
  assigneeAccountId: string;
  /** Project-scoped assignable users; refetched when project changes. */
  assignees: JiraUserSummary[];
  assigneesLoading: boolean;
  sprints: JiraSprint[];
  sprintId: number | null;
  busy: boolean;
  error: string | null;
}

export interface GithubCreatePrModalState {
  repo: string;
  repos: { owner: string; name: string; full_name: string; default_branch?: string | null }[];
  reposLoading: boolean;
  branches: RepoBranch[];
  branchesLoading: boolean;
  base: string;
  head: string;
  title: string;
  body: string;
  draft: boolean;
  compare: {
    loading: boolean;
    error: string | null;
    total_commits: number;
    ahead_by: number;
    behind_by: number;
    additions: number;
    deletions: number;
    commits: RepoCommitEntry[];
    files: ChangedFile[];
  } | null;
  filesExpanded: boolean;
  busy: boolean;
  error: string | null;
}

export interface DwPlanSummary {
  workflowId: string;
  rationale: string;
  subagents: { id: string; prompt: string }[];
}

export interface DwPreflightModalState {
  plan: DwPlanSummary;
  estimateUsd: number;
  budgetCap: number;
  resolve: ((action: { kind: 'approve'; cap: number } | { kind: 'cancel' }) => void) | null;
}

export interface QuotaPauseModalState {
  /** 0-100 percent — current utilization that tripped the guard. */
  pct: number;
  /** Which bucket pushed past the threshold. */
  bucketLabel: '5H' | '7D';
  /** Unix-ms of the bucket's next reset. Modal renders live countdown. */
  resumeAt: number;
  /** Deferred resolver — the modal calls `resolve('wait' | 'cancel')`
   *  to close itself + signal the caller. The promise lives in
   *  `openQuotaPauseModal`'s closure; the state-side ref is non-
   *  serializable so a reload mid-pause just dismisses the modal. */
  resolve: ((action: 'wait' | 'cancel') => void) | null;
}

// --- Aggregate registry ------------------------------------------------------

export interface ModalsState {
  commit: CommitModalState | null;
  userPicker: UserPickerModalState | null;
  jiraConnect: JiraConnectModalState | null;
  sentryConnect: SentryConnectModalState | null;
  claudeStatus: ClaudeStatusModalState | null;
  cursorStatus: CursorStatusModalState | null;
  pat: PatModalState | null;
  comment: CommentModalState | null;
  review: ReviewModalState | null;
  merge: MergeModalState | null;
  confirm: ConfirmModalState | null;
  jiraCreate: JiraCreateModalState | null;
  githubCreatePr: GithubCreatePrModalState | null;
  quotaPause: QuotaPauseModalState | null;
  dwPreflight: DwPreflightModalState | null;
}

export const modalsState = $state<ModalsState>({
  commit: null,
  userPicker: null,
  jiraConnect: null,
  sentryConnect: null,
  claudeStatus: null,
  cursorStatus: null,
  pat: null,
  comment: null,
  review: null,
  merge: null,
  confirm: null,
  jiraCreate: null,
  githubCreatePr: null,
  quotaPause: null,
  dwPreflight: null
});

export type ModalKey = keyof ModalsState;

/** Open a modal with its initial state. Replaces any existing payload for
 *  that key. */
export function openModal<K extends ModalKey>(key: K, value: NonNullable<ModalsState[K]>): void {
  modalsState[key] = value;
}

/** Close a modal. */
export function closeModal<K extends ModalKey>(key: K): void {
  modalsState[key] = null;
}

/** Merge a partial update into an open modal. No-op when the modal is
 *  closed — useful inside async submit handlers that may race with a manual
 *  dismiss. */
/** Open the quota-pause modal + await the user's choice. Returns
 *  `'wait'` when the user asks to queue the send and resume on
 *  reset, `'cancel'` when they bail (we drop the input). The modal
 *  component is responsible for calling the resolver before
 *  closing itself; this helper just sets the state + holds the
 *  deferred. */
export function openQuotaPauseModal(args: {
  pct: number;
  bucketLabel: '5H' | '7D';
  resumeAt: number;
}): Promise<'wait' | 'cancel'> {
  return new Promise((resolve) => {
    modalsState.quotaPause = {
      pct: args.pct,
      bucketLabel: args.bucketLabel,
      resumeAt: args.resumeAt,
      resolve
    };
  });
}

/** Open the DW preflight modal + await user's approve / cancel
 *  decision (SDD `sdd-98a42f3bdb` Phase 4). On approve returns the
 *  user-chosen budget cap (may be raised above the $5 default).
 *  Cancel drops the workflow without firing fan-out. */
export function openDwPreflightModal(args: {
  plan: DwPlanSummary;
  estimateUsd: number;
}): Promise<{ kind: 'approve'; cap: number } | { kind: 'cancel' }> {
  return new Promise((resolve) => {
    modalsState.dwPreflight = {
      plan: args.plan,
      estimateUsd: args.estimateUsd,
      budgetCap: 5,
      resolve
    };
  });
}

export function patchModal<K extends ModalKey>(
  key: K,
  patch: Partial<NonNullable<ModalsState[K]>>
): void {
  const current = modalsState[key];
  if (!current) return;
  modalsState[key] = { ...current, ...patch } as ModalsState[K];
}
