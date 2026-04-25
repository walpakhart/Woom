<script lang="ts">
  // Renders every modal. Each child reads its own state slice from
  // `$lib/state/modals.svelte` directly — this component just composes
  // them and threads the few callbacks that need parent context (browser
  // open, async submits, refresh).

  import CommitModal from './CommitModal.svelte';
  import UserPickerModal from './UserPickerModal.svelte';
  import JiraConnectModal from './JiraConnectModal.svelte';
  import SentryConnectModal from './SentryConnectModal.svelte';
  import ClaudeStatusModal from './ClaudeStatusModal.svelte';
  import CursorStatusModal from './CursorStatusModal.svelte';
  import PatModal from './PatModal.svelte';
  import CommentModal from './CommentModal.svelte';
  import ReviewModal from './ReviewModal.svelte';
  import MergeModal from './MergeModal.svelte';
  import ConfirmModal from './ConfirmModal.svelte';
  import JiraCreateModal from './JiraCreateModal.svelte';
  import GithubCreatePrModal from './GithubCreatePrModal.svelte';
  import type { JiraUserSummary } from '$lib/data';

  interface Props {
    now: number;
    openBrowser: (url: string) => void | Promise<void>;
    onUserPickerInput: (q: string) => void;
    selectJiraUser: (u: JiraUserSummary | null) => Promise<void> | void;
    selectAnyJiraUser: () => Promise<void> | void;
    submitJiraConnect: () => Promise<void> | void;
    jiraTokenUrl: () => string;
    submitSentryConnect: () => Promise<void> | void;
    sentryTokenUrl: () => string;
    refreshClaudeStatus: () => Promise<void> | void;
    claudeInstallUrl: () => string;
    refreshCursorStatus: () => Promise<void> | void;
    cursorInstallUrl: () => string;
    submitPat: () => Promise<void> | void;
    githubTokenUrl: () => string;
    submitComment: () => Promise<void> | void;
    submitReview: () => Promise<void> | void;
    submitMerge: () => Promise<void> | void;
    onJiraCreateProjectChange: (key: string) => Promise<void> | void;
    submitJiraCreate: () => Promise<void> | void;
    onGithubPrRepoChange: (full: string) => Promise<void> | void;
    onGithubPrBranchesChange: () => Promise<void> | void;
    submitGithubPr: () => Promise<void> | void;
  }

  let {
    now,
    openBrowser,
    onUserPickerInput,
    selectJiraUser,
    selectAnyJiraUser,
    submitJiraConnect,
    jiraTokenUrl,
    submitSentryConnect,
    sentryTokenUrl,
    refreshClaudeStatus,
    claudeInstallUrl,
    refreshCursorStatus,
    cursorInstallUrl,
    submitPat,
    githubTokenUrl,
    submitComment,
    submitReview,
    submitMerge,
    onJiraCreateProjectChange,
    submitJiraCreate,
    onGithubPrRepoChange,
    onGithubPrBranchesChange,
    submitGithubPr
  }: Props = $props();
</script>

<CommitModal {now} {openBrowser} />
<UserPickerModal {onUserPickerInput} {selectJiraUser} {selectAnyJiraUser} />
<JiraConnectModal {openBrowser} {jiraTokenUrl} onSubmit={submitJiraConnect} />
<SentryConnectModal {openBrowser} {sentryTokenUrl} onSubmit={submitSentryConnect} />
<ClaudeStatusModal {openBrowser} {claudeInstallUrl} onRefresh={refreshClaudeStatus} />
<CursorStatusModal {openBrowser} {cursorInstallUrl} onRefresh={refreshCursorStatus} />
<PatModal {openBrowser} {githubTokenUrl} onSubmit={submitPat} />
<CommentModal onSubmit={submitComment} />
<ReviewModal onSubmit={submitReview} />
<MergeModal onSubmit={submitMerge} />
<ConfirmModal />
<JiraCreateModal onProjectChange={onJiraCreateProjectChange} onSubmit={submitJiraCreate} />
<GithubCreatePrModal {now} onRepoChange={onGithubPrRepoChange} onBranchesChange={onGithubPrBranchesChange} onSubmit={submitGithubPr} />
