// Top-level view router. Mirrors the local `View` union in `+page.svelte`
// so any component can switch the view without prop drilling.
//
// `tab` is the inspector tab inside the GitHub focus pane. Both pieces
// are independent but live together because every consumer that flips
// one usually wants to read the other.

export type View =
  | 'home'
  | 'jiraApp'
  | 'githubApp'
  | 'sentryApp'
  | 'claudeApp'
  | 'cursorApp'
  | 'editorApp'
  | 'canvasApp'
  | 'terminalApp'
  | 'rules'
  | 'connections'
  | 'settings';
export type DetailTab = 'conversation' | 'commits' | 'files' | 'reviews' | 'checks';

export const viewState = $state<{ view: View; tab: DetailTab }>({
  view: 'claudeApp',
  tab: 'conversation'
});

export function setView(v: View) {
  viewState.view = v;
}

export function setTab(t: DetailTab) {
  viewState.tab = t;
}
