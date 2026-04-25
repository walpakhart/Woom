// Top-level view router. Replaces a `let view = $state(...)` in
// `+page.svelte` so any component can switch the view without prop
// drilling (e.g. an empty-state CTA in TasksView jumping to Connections).
//
// Tab is the inspector tab inside the GitHub focus pane. Both pieces are
// independent but live together because every consumer that flips one
// usually wants to read the other.

export type View = 'workbench' | 'repositories' | 'tasks' | 'issues' | 'rules' | 'connections' | 'settings';
export type DetailTab = 'conversation' | 'commits' | 'files' | 'reviews' | 'checks';

export const viewState = $state<{ view: View; tab: DetailTab }>({
  view: 'workbench',
  tab: 'conversation'
});

export function setView(v: View) {
  viewState.view = v;
}

export function setTab(t: DetailTab) {
  viewState.tab = t;
}
