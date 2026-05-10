// Layout state — instance lists keyed by kind. Each App view used to
// be a true singleton; for editor / canvas / terminal we now allow
// multiple instances (each with its own curated name from
// `instanceNames.ts`). Source apps (github / jira / sentry) and
// agent columns (claude / cursor) stay single-instance because their
// per-kind state stores assume that — multi-instance there is a
// future milestone.
//
// Schema:
//   - `instances`: per-kind list of `{ id, name }`. The default entry
//     per kind has id == `APP_INSTANCE_IDS[kind]` so legacy code that
//     looked up the singleton by that constant still works.
//   - `activeInstance`: which instance the rail's icon click jumps
//     to per kind.
//   - `active`: current selection inside each solo (which repo, which
//     canvas, which terminal cwd) — kept for back-compat with the old
//     persisted state.
//   - `links`: many-to-many links between apps (editor ↔ agent
//     sessions, canvas ↔ agent sessions, terminal ↔ agent sessions).
//
// Persisted to localStorage at `woom:layout:v1`.

import type { PanelKind } from '$lib/types';
import { pickInstanceName, nameToSlug } from './instanceNames';

export type AppKind = PanelKind;

/** Stable default instance ids — used everywhere a "column instance id"
 *  used to be needed (`inboxState.itemsByInstance`, `canvasState.byInstance`,
 *  session.columnInstanceId, etc.). Identity per kind, never changes;
 *  spawned secondary instances get ids like `editor:vermeer`. */
export const APP_INSTANCE_IDS: Record<AppKind, string> = {
  github: 'github-solo',
  jira: 'jira-solo',
  sentry: 'sentry-solo',
  claude: 'claude-solo',
  cursor: 'cursor-solo',
  editor: 'editor-solo',
  canvas: 'canvas-solo',
  terminal: 'terminal-solo'
};

/** Kinds that support spawning extra instances (multi-instance apps).
 *  Source apps + agents stay single-instance for now. */
export const MULTI_INSTANCE_KINDS: ReadonlySet<AppKind> = new Set<AppKind>([
  'editor', 'canvas', 'terminal'
]);

export interface AppInstance {
  /** Stable, slug-safe identifier. The default instance per kind
   *  uses the legacy `APP_INSTANCE_IDS[kind]` value; secondary
   *  instances use `<kind>:<slug>`. */
  id: string;
  /** Human-readable label shown in tooltips / link picker / rail
   *  popover. The default instance gets the kind's display name
   *  (e.g. "Editor"); secondary instances get a curated mark
   *  (e.g. "Vermeer"). */
  name: string;
  /** True for the always-present primary instance — these can't be
   *  removed via the rail menu. */
  primary?: boolean;
}

const DEFAULT_INSTANCE_NAMES: Record<AppKind, string> = {
  github: 'GitHub',
  jira: 'Jira',
  sentry: 'Sentry',
  claude: 'Claude',
  cursor: 'Cursor',
  editor: 'Editor',
  canvas: 'Canvas',
  terminal: 'Terminal'
};

function defaultInstances(): Record<AppKind, AppInstance[]> {
  const out: Record<string, AppInstance[]> = {};
  /* Multi-instance kinds get a curated mark from the pool out of the
     gate, so the primary editor reads as "Vermeer" instead of the bare
     "Editor" label. We keep a running `taken` so the three multi-
     instance kinds don't all collapse onto the same first-pool entry. */
  const taken: string[] = [];
  for (const k of Object.keys(APP_INSTANCE_IDS) as AppKind[]) {
    let name: string;
    if (MULTI_INSTANCE_KINDS.has(k)) {
      name = pickInstanceName(taken);
      taken.push(name);
    } else {
      name = DEFAULT_INSTANCE_NAMES[k];
    }
    out[k] = [{ id: APP_INSTANCE_IDS[k], name, primary: true }];
  }
  return out as Record<AppKind, AppInstance[]>;
}

function defaultActiveInstance(): Record<AppKind, string> {
  const out: Record<string, string> = {};
  for (const k of Object.keys(APP_INSTANCE_IDS) as AppKind[]) {
    out[k] = APP_INSTANCE_IDS[k];
  }
  return out as Record<AppKind, string>;
}

const STORAGE_KEY = 'woom:layout:v1';

interface LayoutLinks {
  /** repoPath (editor cwd) → sessionIds it's linked to */
  editorToAgent: Record<string, string[]>;
  /** canvasId → sessionIds linked */
  canvasToAgent: Record<string, string[]>;
  /** cwd → sessionIds linked (terminal) */
  terminalToAgent: Record<string, string[]>;
}

export const layoutState = $state<{
  /** Per-kind list of open instances. Each kind always has at least
   *  the primary entry; multi-instance kinds may have more. */
  instances: Record<AppKind, AppInstance[]>;
  /** Which instance the rail jumps to when its icon is clicked. */
  activeInstance: Record<AppKind, string>;
  /** Per-kind "what is currently open". `active.editor.repoPath` drives
   *  EditorApp; `active.canvas.canvasId` drives CanvasApp; `active.terminal.cwd`
   *  drives TerminalApp. */
  active: {
    editor: { repoPath: string | null };
    canvas: { canvasId: string | null };
    terminal: { cwd: string | null };
  };
  links: LayoutLinks;
}>({
  instances: defaultInstances(),
  activeInstance: defaultActiveInstance(),
  active: {
    editor: { repoPath: null },
    canvas: { canvasId: null },
    terminal: { cwd: null }
  },
  links: {
    editorToAgent: {},
    canvasToAgent: {},
    terminalToAgent: {}
  }
});

export function persistPanelState() {
  try {
    localStorage.setItem(
      STORAGE_KEY,
      JSON.stringify({
        instances: layoutState.instances,
        activeInstance: layoutState.activeInstance,
        active: layoutState.active,
        links: layoutState.links
      })
    );
  } catch {
    /* SSR / quota: ignore */
  }
}

/** Load layout from localStorage. Reads `woom:layout:v1`; absent on
 *  fresh installs and on first launch after the rename — handled below
 *  by clearing any pre-Woom layout keys so they don't trip later
 *  schema bumps. */
export function restorePanelState() {
  try {
    const v1 = localStorage.getItem(STORAGE_KEY);
    if (v1) {
      const parsed = JSON.parse(v1) as Partial<typeof layoutState>;
      if (parsed && typeof parsed === 'object') {
        /* Multi-instance restore: merge whatever was persisted with
           the default-primary entry per kind so a missing/corrupt
           list still leaves the rail click-able. */
        if (parsed.instances && typeof parsed.instances === 'object') {
          const merged = defaultInstances();
          /* Track names already used across the whole restore so that
             when we promote a stale "Editor"/"Canvas"/"Terminal" name
             to a curated one, we don't collide with a curated mark
             already in use on a different kind. */
          const usedNames = new Set<string>();
          for (const [k, list] of Object.entries(merged) as [AppKind, AppInstance[]][]) {
            for (const i of list) usedNames.add(i.name);
          }
          for (const [k, raw] of Object.entries(parsed.instances) as [AppKind, unknown][]) {
            if (!merged[k] || !Array.isArray(raw)) continue;
            const seen = new Set<string>();
            const list: AppInstance[] = [];
            for (const item of raw) {
              if (!item || typeof item !== 'object') continue;
              const id = (item as { id?: unknown }).id;
              const rawName = (item as { name?: unknown }).name;
              if (typeof id !== 'string' || typeof rawName !== 'string') continue;
              if (seen.has(id)) continue;
              seen.add(id);
              const primary = id === APP_INSTANCE_IDS[k];
              /* One-shot migration: legacy primaries were named after
                 the kind ("Editor", "Canvas", "Terminal"). Promote
                 those to a curated mark so users get the v8 vibe
                 without losing their persisted instance ids. */
              let name: string = rawName;
              if (
                primary &&
                MULTI_INSTANCE_KINDS.has(k) &&
                rawName === DEFAULT_INSTANCE_NAMES[k]
              ) {
                name = pickInstanceName([...usedNames]);
              }
              usedNames.add(name);
              list.push({ id, name, primary });
            }
            /* Always keep the primary first, even if persisted blob
               dropped it (legacy data from before multi-instance). */
            if (!list.some((i) => i.primary)) {
              list.unshift(merged[k][0]);
            }
            merged[k] = list;
          }
          layoutState.instances = merged;
        }
        if (parsed.activeInstance && typeof parsed.activeInstance === 'object') {
          const next = defaultActiveInstance();
          for (const [k, raw] of Object.entries(parsed.activeInstance) as [AppKind, unknown][]) {
            if (typeof raw !== 'string') continue;
            /* Only honour ids that actually exist in our (now-merged)
               instance list, otherwise fall back to the primary. */
            const exists = layoutState.instances[k]?.some((i) => i.id === raw);
            if (exists) next[k] = raw;
          }
          layoutState.activeInstance = next;
        }
        if (parsed.active) {
          layoutState.active = {
            editor: { repoPath: parsed.active.editor?.repoPath ?? null },
            canvas: { canvasId: parsed.active.canvas?.canvasId ?? null },
            terminal: { cwd: parsed.active.terminal?.cwd ?? null }
          };
        }
        if (parsed.links) {
          layoutState.links = {
            editorToAgent: dictOfStringArrays(parsed.links.editorToAgent),
            canvasToAgent: dictOfStringArrays(parsed.links.canvasToAgent),
            terminalToAgent: dictOfStringArrays(parsed.links.terminalToAgent)
          };
        }
        return;
      }
    }
  } catch {
    /* SSR or corrupted blob — leave layoutState at defaults. */
  }
}

function dictOfStringArrays(v: unknown): Record<string, string[]> {
  if (!v || typeof v !== 'object') return {};
  const out: Record<string, string[]> = {};
  for (const [k, raw] of Object.entries(v as Record<string, unknown>)) {
    if (Array.isArray(raw)) {
      out[k] = raw.filter((x): x is string => typeof x === 'string');
    }
  }
  return out;
}

// ---- Convenience accessors --------------------------------------------

/** Reverse lookup: kind for an instance id. Handles both the legacy
 *  primary ids (`editor-solo`) and the new secondary form
 *  (`editor:vermeer`). Returns null for an unknown id (legacy id that
 *  survived in some persisted blob). */
export function kindForInstanceId(id: string): AppKind | null {
  for (const [kind, soloId] of Object.entries(APP_INSTANCE_IDS) as [AppKind, string][]) {
    if (soloId === id) return kind;
  }
  /* Secondary instance form: `<kind>:<slug>`. */
  const colon = id.indexOf(':');
  if (colon > 0) {
    const head = id.slice(0, colon);
    if (head in APP_INSTANCE_IDS) return head as AppKind;
  }
  return null;
}

/** Spawn a new instance of a multi-instance kind. Picks the next
 *  unused name from the curated pool — checking against EVERY open
 *  instance across all kinds so we never end up with two Vermeers
 *  on different rails. Builds a `<kind>:<slug>` id, appends it to
 *  `instances[kind]`, and immediately makes it active. No-op
 *  (and returns null) for kinds that don't support multi-instance. */
export function addInstance(kind: AppKind): AppInstance | null {
  if (!MULTI_INSTANCE_KINDS.has(kind)) return null;
  const taken: string[] = [];
  for (const list of Object.values(layoutState.instances)) {
    for (const inst of list) taken.push(inst.name);
  }
  const name = pickInstanceName(taken);
  const id = `${kind}:${nameToSlug(name)}`;
  /* Guard against the slim chance two clicks within the same tick
     produced the same id — bail rather than silently dedupe. */
  if (layoutState.instances[kind].some((i) => i.id === id)) return null;
  const inst: AppInstance = { id, name };
  layoutState.instances[kind] = [...layoutState.instances[kind], inst];
  layoutState.activeInstance[kind] = id;
  persistPanelState();
  return inst;
}

/** Remove a non-primary instance. The primary one (created from
 *  `APP_INSTANCE_IDS`) is permanent and silently ignored. If the
 *  removed instance was active, fall back to the primary. */
export function removeInstance(kind: AppKind, id: string): void {
  if (!MULTI_INSTANCE_KINDS.has(kind)) return;
  const list = layoutState.instances[kind];
  const target = list.find((i) => i.id === id);
  if (!target || target.primary) return;
  layoutState.instances[kind] = list.filter((i) => i.id !== id);
  if (layoutState.activeInstance[kind] === id) {
    layoutState.activeInstance[kind] = APP_INSTANCE_IDS[kind];
  }
  /* Let dependent stores (sessions / inbox / canvas) clear their
     per-instance slots so a future add of a same-named instance
     doesn't inherit stale state. */
  notifyInstanceRemoved(id);
  persistPanelState();
}

/** Switch the rail's "what to show on click" pointer for a kind. */
export function setActiveInstance(kind: AppKind, id: string): void {
  if (!layoutState.instances[kind].some((i) => i.id === id)) return;
  if (layoutState.activeInstance[kind] === id) return;
  layoutState.activeInstance[kind] = id;
  persistPanelState();
}

/** Canonical iteration order for the eight kinds — used by the agent
 *  context builder, settings panels, and any bookkeeping that needs a
 *  stable kind list. */
export const DEFAULT_PANEL_ORDER: AppKind[] = [
  'github', 'jira', 'sentry', 'claude', 'cursor', 'editor', 'canvas', 'terminal'
];

// ---- Hook for sessions store -----------------------------------------

/** Sessions register a callback so that when a singleton's data is
 *  cleared (rare — only triggered by an explicit reset path) the per-
 *  instance slots in inbox / canvas / sessions stores can free up. */
let onInstanceRemoved: ((id: string) => void) | null = null;
export function registerInstanceRemovedHook(cb: (id: string) => void) {
  onInstanceRemoved = cb;
}
export function notifyInstanceRemoved(id: string) {
  onInstanceRemoved?.(id);
}

// ---- Linking helpers --------------------------------------------------

export function linkSessionToEditor(repoPath: string, sessionId: string) {
  if (!repoPath || !sessionId) return;
  const list = layoutState.links.editorToAgent[repoPath] ?? [];
  if (!list.includes(sessionId)) {
    layoutState.links.editorToAgent[repoPath] = [...list, sessionId];
    persistPanelState();
  }
}

export function unlinkSessionFromEditor(repoPath: string, sessionId: string) {
  const list = layoutState.links.editorToAgent[repoPath];
  if (!list) return;
  const next = list.filter((id) => id !== sessionId);
  if (next.length === 0) {
    delete layoutState.links.editorToAgent[repoPath];
  } else {
    layoutState.links.editorToAgent[repoPath] = next;
  }
  persistPanelState();
}

export function linkSessionToCanvas(canvasId: string, sessionId: string) {
  if (!canvasId || !sessionId) return;
  const list = layoutState.links.canvasToAgent[canvasId] ?? [];
  if (!list.includes(sessionId)) {
    layoutState.links.canvasToAgent[canvasId] = [...list, sessionId];
    persistPanelState();
  }
}

export function unlinkSessionFromCanvas(canvasId: string, sessionId: string) {
  const list = layoutState.links.canvasToAgent[canvasId];
  if (!list) return;
  const next = list.filter((id) => id !== sessionId);
  if (next.length === 0) {
    delete layoutState.links.canvasToAgent[canvasId];
  } else {
    layoutState.links.canvasToAgent[canvasId] = next;
  }
  persistPanelState();
}

/** Sessions linked to a given canvas. */
export function sessionsLinkedToCanvas(canvasId: string): string[] {
  return layoutState.links.canvasToAgent[canvasId] ?? [];
}

/** Sessions linked to an editor repo. */
export function sessionsLinkedToEditor(repoPath: string): string[] {
  return layoutState.links.editorToAgent[repoPath] ?? [];
}
