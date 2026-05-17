/* User-defined hooks — mirrors `hooks.rs` config + outcome shape on the
 * frontend so the agent-lifecycle call sites (SessionStart, UserPrompt-
 * Submit, Stop) can `await runHook(...)` and react to blocking exits +
 * prompt rewrites.
 *
 * Config lives on disk at `<app_data>/hooks.json` and is read on
 * demand via `hooks_load_config`. The Settings UI edits it in-place
 * with a JSON textarea (cheap MVP — proper form follows).
 *
 * Phase 1 events: SessionStart / UserPromptSubmit / Stop. Pre/Post-
 * ToolUse not wired yet (requires owning tool dispatch — see
 * `docs/CLAUDE_PARITY.md §2.1.3`). */

import { invoke } from '@tauri-apps/api/core';

export type HookEventName = 'SessionStart' | 'UserPromptSubmit' | 'Stop';

export interface HookHandlerCommand {
  type: 'command';
  command: string;
  args?: string[];
}

export type HookHandler = HookHandlerCommand;

export interface HookEntry {
  matcher: string;
  handler: HookHandler;
  timeout_ms: number;
  disabled: boolean;
}

export interface HookConfig {
  hooks: Record<string, HookEntry[]>;
}

export interface PerHookResult {
  command: string;
  exit_code: number | null;
  duration_ms: number;
  stdout: string;
  stderr: string;
  error: string | null;
}

export interface HookOutcome {
  blocked: boolean;
  feedback: string[];
  updated_prompt: string | null;
  additional_context: string | null;
  per_hook: PerHookResult[];
}

/** Reactive cache of the loaded config. The Settings UI reads + writes
 *  this; lifecycle callers read it via `runHook` (which always uses
 *  the latest on-disk state via the Tauri command, but the cache is
 *  handy for showing "N hooks configured" in the UI without IPC). */
export const hooksState = $state<{
  loaded: boolean;
  config: HookConfig;
}>({
  loaded: false,
  config: { hooks: {} }
});

export async function loadHookConfig(): Promise<HookConfig> {
  try {
    const c = await invoke<HookConfig>('hooks_load_config');
    hooksState.config = c;
    hooksState.loaded = true;
    return c;
  } catch (e) {
    console.warn('hooks_load_config failed', e);
    return { hooks: {} };
  }
}

export async function saveHookConfig(config: HookConfig): Promise<void> {
  await invoke<void>('hooks_save_config', { config });
  hooksState.config = config;
}

/** Run hooks for an event. Caller awaits the outcome and acts on
 *  `blocked` / `updated_prompt` / `additional_context`. Failure to
 *  invoke (Tauri error) is logged and returns a no-op outcome — we
 *  never want a misconfigured hook to brick the agent flow. */
export async function runHook(
  event: HookEventName,
  payload: Record<string, unknown>,
  matchField?: string
): Promise<HookOutcome> {
  try {
    return await invoke<HookOutcome>('hooks_run', {
      event,
      payload,
      matchField: matchField ?? null
    });
  } catch (e) {
    console.warn(`hooks_run(${event}) failed`, e);
    return {
      blocked: false,
      feedback: [],
      updated_prompt: null,
      additional_context: null,
      per_hook: []
    };
  }
}

/** Convenience: count of enabled hooks across all events — used for
 *  the Settings card's "N hooks configured" subtitle. */
export function enabledHookCount(c: HookConfig = hooksState.config): number {
  let n = 0;
  for (const list of Object.values(c.hooks)) {
    for (const e of list) {
      if (!e.disabled) n += 1;
    }
  }
  return n;
}
