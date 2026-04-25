// Thin wrapper around the Tauri notification plugin. Centralizes permission
// handling and gives callers a no-op-on-failure surface so a denied permission
// or a missing entitlement never throws into the UI.
//
// macOS Notification Center delivery is best-effort: if the user toggled
// notifications off in System Settings, requestPermission() returns 'denied'
// and we silently skip. Toaster (in-app) still fires from the call site.

import {
  isPermissionGranted,
  requestPermission,
  sendNotification
} from '@tauri-apps/plugin-notification';

let granted: boolean | null = null;

/** Probe + request once. Cached for the session. */
async function ensurePermission(): Promise<boolean> {
  if (granted !== null) return granted;
  try {
    let ok = await isPermissionGranted();
    if (!ok) {
      const res = await requestPermission();
      ok = res === 'granted';
    }
    granted = ok;
  } catch {
    granted = false;
  }
  return granted;
}

export async function notifyNative(opts: { title: string; body?: string }): Promise<void> {
  try {
    if (!(await ensurePermission())) return;
    sendNotification({ title: opts.title, body: opts.body });
  } catch {
    // Swallow — in-app toaster already covered it.
  }
}

/** Fire-and-forget — don't await this from hot paths. */
export function notifyClaudeRunComplete(opts: {
  agentLabel: string;
  sessionTitle: string;
  ok: boolean;
  durationMs?: number;
}): void {
  const dur = opts.durationMs && opts.durationMs > 1000
    ? ` · ${Math.round(opts.durationMs / 1000)}s`
    : '';
  void notifyNative({
    title: opts.ok ? `${opts.agentLabel} finished` : `${opts.agentLabel} failed`,
    body: `${opts.sessionTitle}${dur}`
  });
}

/** Whether the document is currently focused. Skip native notifications when
 *  the user is already looking at the app — the in-chat update is enough. */
export function appHasFocus(): boolean {
  return typeof document !== 'undefined' && document.visibilityState === 'visible' && document.hasFocus();
}
