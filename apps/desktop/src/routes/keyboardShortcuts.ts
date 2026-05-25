// Global keyboard-shortcut dispatcher extracted from `+page.svelte`
// in wave-40. Handles every ⌘/⇧/Esc binding documented in the
// cheatsheet. Built as a factory so the route file can pass setter
// callbacks for its reactive `$state` locals (paletteOpen,
// agentDashboardOpen, etc) — Svelte 5 `let` state can't be passed
// by reference.

import { openUrl } from '@tauri-apps/plugin-opener';
import { sessionsState } from '$lib/state/sessions.svelte';
import { inboxState, moveSelection, closeFocusItem } from '$lib/state/inbox.svelte';
import { closeModal, modalsState } from '$lib/state/modals.svelte';
import { toggleDensity } from '$lib/state/density.svelte';

export type View = string;

export interface KeyboardShortcutDeps {
  // View / paletteMode / digit-map
  getView(): View;
  setView(v: View): void;
  setPaletteMode(m: 'normal' | 'recents'): void;
  togglePaletteOpen(): void;
  setPaletteOpen(open: boolean): void;
  SOLO_BY_DIGIT: Record<string, View>;
  // Booleans the bindings flip
  toggleAgentDashboard(): void;
  toggleSearchInFiles(): void;
  toggleQuickOpen(): void;
  toggleSymbolPicker(): void;
  toggleCheatsheet(): void;
  setCheatsheet(open: boolean): void;
  toggleWelcome(): void;
  setWelcome(open: boolean): void;
  setSearchInFiles(open: boolean): void;
  setQuickOpen(open: boolean): void;
  setSymbolPicker(open: boolean): void;
  // Route-local readers / actions
  isSourceApp(): boolean;
  isWelcomeOpen(): boolean;
  isCheatsheetOpen(): boolean;
  isPaletteOpen(): boolean;
  isSearchInFilesOpen(): boolean;
  isQuickOpenOpen(): boolean;
  isSymbolPickerOpen(): boolean;
  navBack(): void;
  navForward(): void;
  stopAgentForKind(kind: 'claude' | 'cursor'): Promise<void>;
  // Modal derived snapshots (returns truthy object when open)
  getPatModal(): { busy: boolean } | null;
  getJiraModal(): { busy: boolean } | null;
  getClaudeModal(): { loading: boolean } | null;
  getCommentModal(): { busy: boolean } | null;
  getReviewModal(): { busy: boolean } | null;
  getMergeModal(): { busy: boolean } | null;
  getCommitModal(): unknown | null;
  getConfirmModal(): { busy: boolean } | null;
  getJiraCreateModal(): { busy: boolean } | null;
  getGithubCreatePrModal(): { busy: boolean } | null;
}

export function isTextInput(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  const tag = target.tagName;
  if (tag === 'INPUT' || tag === 'TEXTAREA') return true;
  if (target.isContentEditable) return true;
  return false;
}

export function focusedRowUrl(): string | null {
  if (inboxState.focusItem?.url) return inboxState.focusItem.url;
  if (inboxState.jiraFocusKey) {
    for (const list of Object.values(inboxState.jiraItemsByInstance)) {
      const hit = list.find((it) => it.key === inboxState.jiraFocusKey);
      if (hit) return hit.url;
    }
    const hitTab = inboxState.jiraTabItems.find((it) => it.key === inboxState.jiraFocusKey);
    if (hitTab) return hitTab.url;
  }
  if (inboxState.sentryFocusId) {
    for (const list of Object.values(inboxState.sentryItemsByInstance)) {
      const hit = list.find((it) => it.id === inboxState.sentryFocusId);
      if (hit?.permalink) return hit.permalink;
    }
    const hitTab = inboxState.sentryTabItems.find((it) => it.id === inboxState.sentryFocusId);
    if (hitTab?.permalink) return hitTab.permalink;
  }
  return null;
}

export function makeAnyModalOpen(deps: KeyboardShortcutDeps): () => boolean {
  return () =>
    !!(
      deps.getPatModal() ||
      deps.getJiraModal() ||
      deps.getClaudeModal() ||
      modalsState.userPicker ||
      deps.getCommentModal() ||
      deps.getReviewModal() ||
      deps.getMergeModal() ||
      deps.getCommitModal() ||
      deps.getConfirmModal() ||
      deps.getJiraCreateModal() ||
      deps.getGithubCreatePrModal() ||
      inboxState.focusItem ||
      inboxState.jiraFocusKey ||
      inboxState.sentryFocusId ||
      deps.isPaletteOpen() ||
      deps.isSearchInFilesOpen() ||
      deps.isQuickOpenOpen() ||
      deps.isSymbolPickerOpen() ||
      deps.isCheatsheetOpen() ||
      deps.isWelcomeOpen()
    );
}

export function createOnKey(deps: KeyboardShortcutDeps): (e: KeyboardEvent) => void {
  const anyModalOpen = makeAnyModalOpen(deps);
  return (e: KeyboardEvent) => {
    if ((e.metaKey || e.ctrlKey) && e.key === 'k') {
      e.preventDefault();
      deps.setPaletteMode('normal');
      deps.togglePaletteOpen();
    } else if ((e.metaKey || e.ctrlKey) && e.shiftKey && (e.key === 'a' || e.key === 'A')) {
      e.preventDefault();
      deps.toggleAgentDashboard();
    } else if ((e.metaKey || e.ctrlKey) && e.shiftKey && (e.key === 'f' || e.key === 'F')) {
      e.preventDefault();
      deps.toggleSearchInFiles();
    } else if ((e.metaKey || e.ctrlKey) && (e.key === 'p' || e.key === 'P') && !e.shiftKey && !e.altKey) {
      e.preventDefault();
      deps.toggleQuickOpen();
    } else if (
      (e.metaKey || e.ctrlKey) && e.shiftKey &&
      (e.key === '?' || e.key === '/' || e.code === 'Slash')
    ) {
      e.preventDefault();
      deps.setCheatsheet(false);
      deps.toggleWelcome();
    } else if (
      (e.metaKey || e.ctrlKey) && e.shiftKey &&
      (e.key === 'v' || e.key === 'V' || e.code === 'KeyV') &&
      deps.getView() === 'editorApp'
    ) {
      e.preventDefault();
      window.dispatchEvent(new CustomEvent('woom:editor:toggle-md-preview'));
    } else if ((e.metaKey || e.ctrlKey) && e.shiftKey && (e.key === 'o' || e.key === 'O' || e.code === 'KeyO')) {
      e.preventDefault();
      deps.toggleSymbolPicker();
    } else if (
      (e.metaKey || e.ctrlKey) && e.shiftKey &&
      (e.key === 'b' || e.key === 'B' || e.code === 'KeyB') &&
      deps.getView() === 'editorApp'
    ) {
      e.preventDefault();
      window.dispatchEvent(new CustomEvent('woom:editor:new-branch'));
    } else if (
      (e.metaKey || e.ctrlKey) &&
      !e.shiftKey && !e.altKey &&
      (e.key === '[' || e.code === 'BracketLeft') &&
      !isTextInput(e.target)
    ) {
      e.preventDefault();
      deps.navBack();
    } else if (
      (e.metaKey || e.ctrlKey) &&
      !e.shiftKey && !e.altKey &&
      (e.key === ']' || e.code === 'BracketRight') &&
      !isTextInput(e.target)
    ) {
      e.preventDefault();
      deps.navForward();
    } else if (
      (e.metaKey || e.ctrlKey) &&
      !e.shiftKey && !e.altKey &&
      (e.key === '\\' || e.code === 'Backslash') &&
      !isTextInput(e.target)
    ) {
      e.preventDefault();
      toggleDensity();
    } else if (
      (e.metaKey || e.ctrlKey) &&
      !e.shiftKey && !e.altKey &&
      (e.key === '.' || e.code === 'Period')
    ) {
      const view = deps.getView();
      const kind: 'claude' | 'cursor' | null =
        view === 'claudeApp' ? 'claude'
        : view === 'cursorApp' ? 'cursor'
        : null;
      if (kind) {
        const activeId = sessionsState.activeIds[kind];
        const s = activeId ? sessionsState.list.find((x) => x.id === activeId) : null;
        if (s?.sending) {
          e.preventDefault();
          void deps.stopAgentForKind(kind);
        }
      }
    } else if ((e.metaKey || e.ctrlKey) && e.key === 'e' && !e.shiftKey && !e.altKey) {
      e.preventDefault();
      deps.setPaletteMode('recents');
      deps.setPaletteOpen(true);
    } else if (
      (e.metaKey || e.ctrlKey) &&
      !e.shiftKey && !e.altKey &&
      e.key >= '0' && e.key <= '8'
    ) {
      e.preventDefault();
      const targetView = deps.SOLO_BY_DIGIT[e.key];
      if (targetView) deps.setView(targetView);
    } else if (e.key === 'Escape') {
      if (deps.isWelcomeOpen()) {
        deps.setWelcome(false);
        return;
      }
      if (deps.isCheatsheetOpen()) {
        deps.setCheatsheet(false);
        return;
      }
      deps.setPaletteOpen(false);
      deps.setSearchInFiles(false);
      deps.setQuickOpen(false);
      deps.setSymbolPicker(false);
      const patModal = deps.getPatModal();
      if (patModal && !patModal.busy) closeModal('pat');
      const jiraModal = deps.getJiraModal();
      if (jiraModal && !jiraModal.busy) closeModal('jiraConnect');
      const claudeModal = deps.getClaudeModal();
      if (claudeModal && !claudeModal.loading) closeModal('claudeStatus');
      if (modalsState.cursorStatus && !modalsState.cursorStatus.loading) closeModal('cursorStatus');
      if (modalsState.userPicker) closeModal('userPicker');
      const commentModal = deps.getCommentModal();
      if (commentModal && !commentModal.busy) closeModal('comment');
      const reviewModal = deps.getReviewModal();
      if (reviewModal && !reviewModal.busy) closeModal('review');
      const mergeModal = deps.getMergeModal();
      if (mergeModal && !mergeModal.busy) closeModal('merge');
      if (deps.getCommitModal()) closeModal('commit');
      const confirmModal = deps.getConfirmModal();
      if (confirmModal && !confirmModal.busy) closeModal('confirm');
      const jiraCreateModal = deps.getJiraCreateModal();
      if (jiraCreateModal && !jiraCreateModal.busy) closeModal('jiraCreate');
      const githubCreatePrModal = deps.getGithubCreatePrModal();
      if (githubCreatePrModal && !githubCreatePrModal.busy) closeModal('githubCreatePr');
      if (inboxState.focusItem) closeFocusItem();
      if (inboxState.jiraFocusKey) inboxState.jiraFocusKey = null;
      if (inboxState.sentryFocusId) inboxState.sentryFocusId = null;
    } else if (e.key === '?' && !isTextInput(e.target) && !anyModalOpen()) {
      e.preventDefault();
      deps.toggleCheatsheet();
    } else if (e.key === 'j' && deps.isSourceApp() && !anyModalOpen()) {
      moveSelection(1);
    } else if (e.key === 'k' && deps.isSourceApp() && !(e.metaKey || e.ctrlKey) && !anyModalOpen()) {
      moveSelection(-1);
    } else if (e.key === 'o' && !isTextInput(e.target) && !anyModalOpen() && !(e.metaKey || e.ctrlKey)) {
      const targetUrl = focusedRowUrl();
      if (targetUrl) {
        e.preventDefault();
        void openUrl(targetUrl);
      }
    }
  };
}

export function mergeDisabled(): boolean {
  if (!inboxState.focusItem?.is_pull_request) return true;
  if (!inboxState.prDetail) return true;
  if (inboxState.prDetail.merged) return true;
  if (inboxState.prDetail.state !== 'open') return true;
  if (inboxState.prDetail.draft) return true;
  return inboxState.prDetail.mergeable === false;
}
