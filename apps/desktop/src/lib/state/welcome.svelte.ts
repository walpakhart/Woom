/* First-launch welcome flow flag (`docs/ROADMAP_1.0.md §1.5`).
 *
 * Tiny piece of state — just "has the user dismissed the welcome
 * overlay yet?" — but it earns its own module so SettingsView can
 * expose a "Show welcome again" affordance without poking
 * localStorage directly. */

const STORAGE_KEY = 'forgehold:welcome-completed:v1';

export const welcomeState = $state<{ completed: boolean }>({
  completed: loadCompleted()
});

export function markWelcomeCompleted(): void {
  welcomeState.completed = true;
  try {
    localStorage.setItem(STORAGE_KEY, '1');
  } catch {
    /* SSR / quota — non-critical */
  }
}

export function resetWelcome(): void {
  welcomeState.completed = false;
  try {
    localStorage.removeItem(STORAGE_KEY);
  } catch { /* ignore */ }
}

function loadCompleted(): boolean {
  if (typeof localStorage === 'undefined') return true; /* SSR — never show */
  try {
    return localStorage.getItem(STORAGE_KEY) === '1';
  } catch {
    return true;
  }
}
