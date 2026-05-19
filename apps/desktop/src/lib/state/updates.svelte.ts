/* Frontend mirror of the Rust-side updater state machine.
 *
 * Subscribes to the `update:state` Tauri event so the toast (Phase 4),
 * the Settings card, and the future release-notes pane all read from
 * one source of truth. Settings are loaded once on init + re-fetched
 * on demand after any command that mutates them.
 *
 * Phase reference: SDD workspace `sdd-2508eeb82e`, phase 3 task 7.
 */

import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

/** Mirrors `updater::UpdateState` in Rust. Tagged via serde so the
 *  discriminant lands in `.kind`. */
export type UpdateState =
  | { kind: 'idle' }
  | { kind: 'checking' }
  | { kind: 'up_to_date'; checked_at_ms: number }
  | { kind: 'available'; version: string; notes: string; pub_date: string | null; manifest_url: string }
  | { kind: 'snoozed'; version: string; until_ms: number }
  | { kind: 'skipped'; version: string }
  | { kind: 'downloading'; version: string; downloaded: number; total: number | null }
  | { kind: 'verifying'; version: string }
  | { kind: 'installing'; version: string }
  | { kind: 'installed_pending_quit'; version: string }
  | { kind: 'failed'; version: string | null; reason: string };

/** Mirrors `updater::UpdaterSettings`. All timestamps are Unix millis
 *  (Rust side stores them that way to dodge a chrono / time crate
 *  dependency). JS formats via `new Date(ms).toLocaleString()` where
 *  display is needed. */
export interface UpdaterSettings {
  auto_check_enabled: boolean;
  snooze_until_ms: number | null;
  skipped_version: string | null;
  last_checked_at_ms: number | null;
  last_known_version: string | null;
  pending_update_path: string | null;
  pending_update_version: string | null;
}

const defaultSettings: UpdaterSettings = {
  auto_check_enabled: true,
  snooze_until_ms: null,
  skipped_version: null,
  last_checked_at_ms: null,
  last_known_version: null,
  pending_update_path: null,
  pending_update_version: null,
};

/** Reactive store driving every updater UI surface. Lifecycle:
 *   - `initUpdatesStore()` runs once on mount in `+page.svelte`.
 *   - Rust emits `update:state` on every state transition; the
 *     listener below patches `phase`.
 *   - Command wrappers (`snooze`, `skipVersion`, etc.) refresh
 *     `settings` after the mutation lands so the Settings UI's
 *     toggle state reflects the new persisted value without a
 *     manual reload.
 */
export const updateState = $state<{
  phase: UpdateState;
  settings: UpdaterSettings;
  /** True once `initUpdatesStore` has wired the listener +
   *  hydrated `phase`/`settings`. Guards against the toast firing
   *  off a stale `idle` snapshot before the seed completes. */
  ready: boolean;
}>({
  phase: { kind: 'idle' },
  settings: { ...defaultSettings },
  ready: false,
});

let unlisten: (() => void) | null = null;

export async function initUpdatesStore(): Promise<void> {
  if (updateState.ready) return;
  try {
    updateState.settings = await invoke<UpdaterSettings>('updater_get_settings');
  } catch (e) {
    console.warn('updater_get_settings:', e);
  }
  try {
    updateState.phase = await invoke<UpdateState>('updater_get_state');
  } catch (e) {
    console.warn('updater_get_state:', e);
  }
  const off = await listen<UpdateState>('update:state', (e) => {
    updateState.phase = e.payload;
  });
  unlisten = off;
  updateState.ready = true;
}

export function teardownUpdatesStore(): void {
  if (unlisten) {
    unlisten();
    unlisten = null;
  }
  updateState.ready = false;
}

/* ─── Action helpers ───────────────────────────────────────────────
 * Thin wrappers around the Tauri commands. Each helper re-fetches
 * settings after a mutation so the Settings card's bindings reflect
 * the new persisted state immediately (Rust ALSO emits `update:state`
 * for snooze/skip, but settings live in a separate field).
 */

export async function checkNow(): Promise<void> {
  await invoke('updater_check_now');
  // Don't await refresh — `check_and_emit` already pushed the new
  // state via the event listener, and the settings write happened
  // inline. Refresh is best-effort.
  void refreshSettings();
}

export async function setAutoCheck(enabled: boolean): Promise<void> {
  await invoke('updater_set_auto_check', { enabled });
  updateState.settings = { ...updateState.settings, auto_check_enabled: enabled };
}

export async function snooze(hours: number): Promise<void> {
  if (hours <= 0) throw new Error('hours must be > 0');
  await invoke('updater_snooze', { hours });
  await refreshSettings();
}

export async function skipVersion(version: string): Promise<void> {
  await invoke('updater_skip_version', { version });
  await refreshSettings();
}

export async function clearSkip(): Promise<void> {
  await invoke('updater_clear_skip');
  await refreshSettings();
}

export async function installNow(): Promise<void> {
  await invoke('updater_install_now');
}

/** Install-on-quit (Phase 4 stub flow). Downloads the DMG into the
 *  pending-update slot + persists `pending_update_path`. Phase 5
 *  consumes the slot on `before-quit` via the swap script. Today the
 *  DMG just sits there until next launch's startup check clears it. */
export async function installOnQuit(): Promise<void> {
  await invoke('updater_install_on_quit');
  void refreshSettings();
}

async function refreshSettings(): Promise<void> {
  try {
    updateState.settings = await invoke<UpdaterSettings>('updater_get_settings');
  } catch (e) {
    console.warn('refreshSettings:', e);
  }
}

/* ─── Derived helpers ──────────────────────────────────────────────
 * Pure functions for components to consume. Kept out of the
 * `$derived` blocks because Svelte 5 reactivity can be invoked from
 * non-component callers (e.g. the toast trigger in `+page.svelte`).
 */

export function isAvailable(phase: UpdateState): boolean {
  return phase.kind === 'available';
}

export function availableVersion(phase: UpdateState): string | null {
  return phase.kind === 'available' ? phase.version : null;
}

export function isInProgress(phase: UpdateState): boolean {
  return (
    phase.kind === 'checking' ||
    phase.kind === 'downloading' ||
    phase.kind === 'verifying' ||
    phase.kind === 'installing'
  );
}

export function lastCheckedLabel(settings: UpdaterSettings): string {
  if (!settings.last_checked_at_ms) return 'never';
  const d = new Date(settings.last_checked_at_ms);
  return d.toLocaleString();
}
