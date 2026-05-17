/* Statusline — pipes the current session state to a user shell
 * command and renders stdout below the composer. Mirrors Claude
 * Code's `statusLine` settings (§7.1 of CLAUDE_PARITY.md).
 *
 * Config lives at `<app_data>/statusline.json`:
 *   { "command": "echo $WOOM_MODEL · $WOOM_CWD", "refreshInterval": 30 }
 *
 * The state payload is delivered on stdin as JSON. User scripts can
 * pipe through jq, Python, anything sh -c can spawn. Cost-of-call is
 * absorbed off the agent turn (we await it on turn-end), so a slow
 * script can't block sending. */

import { invoke } from '@tauri-apps/api/core';

export interface StatusLineConfig {
  command: string | null;
  args: string[];
  refresh_interval: number | null;
  timeout_ms: number;
  max_output_bytes: number;
}

export interface StatusLineResult {
  ok: boolean;
  stdout: string;
  stderr: string;
  exit_code: number | null;
  duration_ms: number;
}

export interface StatusLinePayload {
  model: { id: string | null; display_name: string | null };
  cwd: string | null;
  session_id: string;
  session_title: string;
  agent_kind: 'claude' | 'cursor';
  permission_mode: 'default' | 'plan' | string;
  /** Cumulative cost across the session, USD. */
  cost_usd: number;
  /** Last-turn token usage snapshot. */
  context_window: { used_percentage: number; size: number };
  worktree: { path: string | null; branch: string | null };
}

export const statuslineState = $state<{
  config: StatusLineConfig;
  configLoaded: boolean;
  lastResult: StatusLineResult | null;
  /** Wall-clock ms of last successful run — used for the refresh timer
   *  + the "last refreshed Ns ago" Settings hint. */
  lastRanAt: number;
}>({
  config: { command: null, args: [], refresh_interval: null, timeout_ms: 5000, max_output_bytes: 4000 },
  configLoaded: false,
  lastResult: null,
  lastRanAt: 0
});

export async function loadStatusLineConfig(): Promise<void> {
  try {
    const c = await invoke<StatusLineConfig>('statusline_load_config');
    statuslineState.config = c;
    statuslineState.configLoaded = true;
  } catch (e) {
    console.warn('statusline_load_config failed', e);
  }
}

export async function saveStatusLineConfig(config: StatusLineConfig): Promise<void> {
  await invoke<void>('statusline_save_config', { config });
  statuslineState.config = config;
}

/** Run the configured script. Returns the result; also stamps
 *  `lastResult` so reactive UI updates. Cheap to call repeatedly —
 *  Rust spawns sh -c each call (no daemon). */
export async function runStatusLine(payload: StatusLinePayload): Promise<StatusLineResult | null> {
  if (!statuslineState.config.command) return null;
  try {
    const r = await invoke<StatusLineResult>('statusline_run', { payload });
    statuslineState.lastResult = r;
    statuslineState.lastRanAt = Date.now();
    return r;
  } catch (e) {
    console.warn('statusline_run failed', e);
    return null;
  }
}

/** Module-level timer driving the optional periodic re-run. Restarts
 *  whenever the interval setting changes. */
let timerHandle: ReturnType<typeof setInterval> | null = null;

export function installStatusLineTimer(getPayload: () => StatusLinePayload | null): void {
  if (timerHandle !== null) clearInterval(timerHandle);
  const interval = statuslineState.config.refresh_interval ?? 0;
  if (!interval || interval < 5) return;
  timerHandle = setInterval(() => {
    const payload = getPayload();
    if (!payload) return;
    void runStatusLine(payload);
  }, interval * 1000);
}
