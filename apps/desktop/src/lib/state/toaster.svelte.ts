// Toaster — single source for transient user-facing notifications.
// Replaces ad-hoc `console.error()` + silent `catch {}` patterns scattered
// across components. Errors get a sticky toast (manual dismiss); info /
// success auto-dismiss after a few seconds.

export type ToastKind = 'info' | 'success' | 'warning' | 'error';

export interface ToastAction {
  label: string;
  onClick: () => void;
}

export interface Toast {
  id: string;
  kind: ToastKind;
  title: string;
  body?: string;
  actions?: ToastAction[];
  /** Remaining ms before auto-dismiss. `null` = sticky. */
  ttl: number | null;
  createdAt: number;
}

const MAX_STACK = 5;
const DEFAULT_TTL_MS: Record<ToastKind, number | null> = {
  info: 3500,
  success: 3000,
  warning: 6000,
  error: null
};

export const toasterState = $state<{ items: Toast[] }>({ items: [] });

let nextId = 1;
function genId(): string {
  return `t${Date.now().toString(36)}-${nextId++}`;
}

export function notify(opts: {
  kind?: ToastKind;
  title: string;
  body?: string;
  ttlMs?: number | null;
  actions?: ToastAction[];
}): string {
  const kind = opts.kind ?? 'info';
  const t: Toast = {
    id: genId(),
    kind,
    title: opts.title,
    body: opts.body,
    actions: opts.actions,
    ttl: opts.ttlMs === undefined ? DEFAULT_TTL_MS[kind] : opts.ttlMs,
    createdAt: Date.now()
  };
  // Drop oldest non-sticky if we're over the cap. Sticky errors stay.
  if (toasterState.items.length >= MAX_STACK) {
    const idx = toasterState.items.findIndex((x) => x.ttl !== null);
    if (idx >= 0) toasterState.items.splice(idx, 1);
    else toasterState.items.shift();
  }
  toasterState.items.push(t);
  return t.id;
}

/** Convenience for the most common pattern: a Tauri invoke() rejection.
 *  Accepts `unknown` because catch-bound vars are unknown in TS strict. */
export function notifyError(err: unknown, opts?: { title?: string; body?: string }): string {
  const fallback = opts?.title ?? 'Something went wrong';
  let title = fallback;
  let body = opts?.body;
  if (err instanceof Error) {
    title = opts?.title ?? (err.message || fallback);
    if (!body && err.message !== title) body = err.message;
  } else if (typeof err === 'string' && err.trim()) {
    title = opts?.title ?? err;
  } else if (err && typeof err === 'object' && 'message' in err && typeof err.message === 'string') {
    title = opts?.title ?? err.message;
  }
  return notify({ kind: 'error', title, body });
}

export function dismissToast(id: string) {
  const i = toasterState.items.findIndex((t) => t.id === id);
  if (i >= 0) toasterState.items.splice(i, 1);
}

export function clearToasts() {
  toasterState.items = [];
}
