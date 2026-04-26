// Workbench layout state — a list of column instances per named workbench,
// their order, widths, and the transient snap-flash highlight used during
// resize. Persists to localStorage so workbenches and their layouts survive
// reloads. Migrates cleanly from the v1 (singleton-per-kind) schema.

import { tick } from 'svelte';
import { connectionsState } from '$lib/state/connections.svelte';
import type { PanelInstance, PanelKind, Workbench } from '$lib/types';

export const DEFAULT_PANEL_ORDER: PanelKind[] = ['github', 'jira', 'sentry', 'claude', 'cursor', 'editor'];
export const DEFAULT_PANEL_WIDTHS: Record<PanelKind, number> = {
  github: 420,
  jira: 420,
  sentry: 440,
  claude: 520,
  cursor: 520,
  editor: 720
};

// px — how close to a snap point before we grab
export const SNAP_THRESHOLD = 18;

// ---- Storage keys ----
// v1: singleton-per-kind (pre-multi-instance).
const STORAGE_COLUMNS_V1 = 'forgehold:workbench:columns';
const STORAGE_ORDER_V1 = 'forgehold:workbench:order';
const STORAGE_WIDTHS_V1 = 'forgehold:workbench:widths';
// v2: flat list of instances for a single workbench (intermediate schema).
const STORAGE_LAYOUT_V2 = 'forgehold:layout:v2';
// v3: workbenches (list of named column presets).
const STORAGE_WORKBENCHES_V1 = 'forgehold:workbenches:v1';

function genInstanceId(): string {
  if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
    return crypto.randomUUID();
  }
  return 'xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx'.replace(/[xy]/g, (c) => {
    const r = (Math.random() * 16) | 0;
    const v = c === 'x' ? r : (r & 0x3) | 0x8;
    return v.toString(16);
  });
}

/** Human-readable handles for column instances. Drawn from a pool of famous
 *  art / artists / monuments so each column gets a unique, memorable tag
 *  ("Claude Code (Mona-Lisa)"). `pickInstanceName` avoids collisions within
 *  the active workbench; when the pool runs out it falls back to
 *  `<name>-2`, `<name>-3`, etc. */
const INSTANCE_NAME_POOL: readonly string[] = [
  // Paintings
  'Mona-Lisa', 'Starry-Night', 'Sunflowers', 'The-Scream', 'Girl-with-Pearl',
  'Las-Meninas', 'Night-Watch', 'American-Gothic', 'Nighthawks', 'Persistence',
  'Guernica', 'Water-Lilies', 'Kiss', 'Birth-of-Venus', 'Great-Wave',
  'Self-Portrait', 'Whistler-Mother', 'Arnolfini', 'Last-Supper', 'Creation',
  // Sculptures
  'David', 'Pieta', 'Thinker', 'Discobolus', 'Venus-de-Milo', 'Winged-Victory',
  'Laocoon', 'Moses', 'Burghers-of-Calais',
  // Artists / polymaths
  'Da-Vinci', 'Michelangelo', 'Raphael', 'Donatello', 'Caravaggio', 'Vermeer',
  'Rembrandt', 'Van-Gogh', 'Monet', 'Renoir', 'Degas', 'Cezanne', 'Gauguin',
  'Picasso', 'Dali', 'Matisse', 'Kandinsky', 'Klimt', 'Munch', 'Rodin',
  'Bernini', 'Giotto', 'Botticelli', 'El-Greco', 'Velazquez', 'Goya',
  'Turner', 'Degas', 'Warhol', 'Rothko', 'Pollock', 'Hokusai', 'Frida',
  // Monuments / wonders / landmarks
  'Parthenon', 'Colosseum', 'Pantheon', 'Taj-Mahal', 'Great-Wall', 'Stonehenge',
  'Pyramids', 'Notre-Dame', 'Sagrada-Familia', 'Kremlin', 'Petra', 'Alhambra',
  'Machu-Picchu', 'Angkor-Wat', 'Chichen-Itza', 'Acropolis', 'Forum',
  'Duomo', 'Hagia-Sophia', 'Eiffel-Tower'
];

/** Pick a fresh instance name. Prefers unused pool entries in the active
 *  workbench; if all taken, appends an incrementing suffix. */
function pickInstanceName(): string {
  const used = new Set<string>();
  for (const wb of layoutState.workbenches) {
    for (const i of wb.instances) used.add(i.name);
  }
  const shuffled = [...INSTANCE_NAME_POOL].sort(() => Math.random() - 0.5);
  for (const n of shuffled) {
    if (!used.has(n)) return n;
  }
  // Pool exhausted — fall back to suffixing. Extremely rare.
  const base = INSTANCE_NAME_POOL[Math.floor(Math.random() * INSTANCE_NAME_POOL.length)];
  let k = 2;
  while (used.has(`${base}-${k}`)) k++;
  return `${base}-${k}`;
}

/** Module-level reactive singleton — imported by +page.svelte and any
 *  workbench column. Mutate via `layoutState.workbenches[i].instances.push(...)`,
 *  don't destructure. `instances` is a live getter pointing at the active
 *  workbench so existing single-workbench code keeps working. */
/** Snapshot kept when the user archives an instance — instance is
 *  detached from its workbench but its identity (and everything keyed
 *  off `instance.id` in inboxState / sessionsState) is preserved so
 *  unarchive restores it exactly where it was, with all its filters /
 *  items / chats intact. `originalIndex` is the position in
 *  `workbenches[wb].instances` at the moment of archive — restore
 *  inserts at that index, clamped to the current length. */
export type ArchivedInstance = {
  inst: PanelInstance;
  originalWorkbenchId: string;
  originalIndex: number;
  archivedAt: number;
};

export const layoutState = $state<{
  workbenches: Workbench[];
  activeWorkbenchId: string;
  snapFlashInstanceId: string | null;
  archivedInstances: ArchivedInstance[];
}>({
  workbenches: [
    {
      id: genInstanceId(),
      name: 'Main',
      instances: []
    }
  ],
  activeWorkbenchId: '',
  snapFlashInstanceId: null,
  archivedInstances: []
});
// Seed activeWorkbenchId after initialization (can't reference `workbenches` inside the literal).
layoutState.activeWorkbenchId = layoutState.workbenches[0].id;

/** Short convenience: the currently-active workbench's instance list. */
export function activeWorkbench(): Workbench {
  const wb = layoutState.workbenches.find((w) => w.id === layoutState.activeWorkbenchId);
  if (wb) return wb;
  // Defensive fallback — shouldn't happen, but don't crash the UI.
  return layoutState.workbenches[0];
}

export function activeInstances(): PanelInstance[] {
  return activeWorkbench().instances;
}

/** Look up an instance across all workbenches. Sessions hold
 *  `columnInstanceId`, so when workbenches switch we need to find where any
 *  given instance lives today (or return null if it was deleted). */
export function findInstanceAnywhere(id: string): { wb: Workbench; inst: PanelInstance } | null {
  for (const wb of layoutState.workbenches) {
    const inst = wb.instances.find((i) => i.id === id);
    if (inst) return { wb, inst };
  }
  return null;
}

export function persistPanelState() {
  try {
    localStorage.setItem(
      STORAGE_WORKBENCHES_V1,
      JSON.stringify({
        workbenches: layoutState.workbenches,
        activeId: layoutState.activeWorkbenchId,
        archived: layoutState.archivedInstances
      })
    );
  } catch {
    /* quota / SSR: ignore */
  }
}

/** Read localStorage and populate `layoutState`. Supports three schema
 *  generations (v1 singleton-per-kind, v2 flat instance list, v3 workbenches).
 *  First-time v1 users end up with a single "Main" workbench containing their
 *  existing columns. */
/** After load, any instance missing a `name` gets one assigned. Runs once
 *  per restore so v1/v2 users (and early v3 writes) don't end up with blank
 *  labels in the column header. */
function ensureAllInstancesNamed() {
  const used = new Set<string>();
  for (const wb of layoutState.workbenches) {
    for (const i of wb.instances) if (i.name) used.add(i.name);
  }
  const pool = [...INSTANCE_NAME_POOL].sort(() => Math.random() - 0.5);
  let cursor = 0;
  const take = (): string => {
    while (cursor < pool.length && used.has(pool[cursor])) cursor++;
    if (cursor < pool.length) {
      const n = pool[cursor++];
      used.add(n);
      return n;
    }
    // Pool exhausted — suffix fallback.
    const base = INSTANCE_NAME_POOL[0];
    let k = 2;
    while (used.has(`${base}-${k}`)) k++;
    used.add(`${base}-${k}`);
    return `${base}-${k}`;
  };
  for (const wb of layoutState.workbenches) {
    for (const i of wb.instances) {
      if (!i.name) i.name = take();
    }
  }
}

export function restorePanelState() {
  try {
    // Preferred path: v3 workbenches.
    const w = localStorage.getItem(STORAGE_WORKBENCHES_V1);
    if (w) {
      const parsed = JSON.parse(w) as {
        workbenches?: Workbench[];
        activeId?: string;
        archived?: ArchivedInstance[];
      };
      if (Array.isArray(parsed.workbenches) && parsed.workbenches.length > 0) {
        const cleaned: Workbench[] = parsed.workbenches
          .map((wb) => ({
            id: typeof wb.id === 'string' && wb.id ? wb.id : genInstanceId(),
            name: typeof wb.name === 'string' && wb.name ? wb.name : 'Workbench',
            instances: Array.isArray(wb.instances)
              ? wb.instances
                  .filter(
                    (i): i is PanelInstance =>
                      !!i &&
                      typeof i.id === 'string' &&
                      typeof i.kind === 'string' &&
                      DEFAULT_PANEL_ORDER.includes(i.kind as PanelKind) &&
                      typeof i.width === 'number'
                  )
                  .map((i) => ({
                    id: i.id || genInstanceId(),
                    kind: i.kind,
                    width: Math.max(280, Math.min(2000, i.width)),
                    name: typeof (i as { name?: string }).name === 'string'
                      ? (i as { name: string }).name
                      : ''
                  }))
              : []
          }))
          .filter((wb) => wb.instances.length >= 0);
        if (cleaned.length > 0) {
          layoutState.workbenches = cleaned;
          const activeExists = cleaned.find((wb) => wb.id === parsed.activeId);
          layoutState.activeWorkbenchId = activeExists ? activeExists.id : cleaned[0].id;
          /* Restore archive list — same filtering as instances above so a
             corrupt entry doesn't poison the rest. */
          if (Array.isArray(parsed.archived)) {
            layoutState.archivedInstances = parsed.archived
              .filter(
                (a): a is ArchivedInstance =>
                  !!a &&
                  !!a.inst &&
                  typeof a.inst.id === 'string' &&
                  typeof a.inst.kind === 'string' &&
                  DEFAULT_PANEL_ORDER.includes(a.inst.kind as PanelKind) &&
                  typeof a.inst.width === 'number' &&
                  typeof a.originalWorkbenchId === 'string' &&
                  typeof a.originalIndex === 'number'
              )
              .map((a) => ({
                inst: {
                  id: a.inst.id,
                  kind: a.inst.kind,
                  width: Math.max(280, Math.min(2000, a.inst.width)),
                  name:
                    typeof (a.inst as { name?: string }).name === 'string'
                      ? (a.inst as { name: string }).name
                      : ''
                },
                originalWorkbenchId: a.originalWorkbenchId,
                originalIndex: Math.max(0, a.originalIndex),
                archivedAt: typeof a.archivedAt === 'number' ? a.archivedAt : 0
              }));
          }
          ensureAllInstancesNamed();
          return;
        }
      }
    }

    // v2 → v3: upgrade a flat instance list to a single "Main" workbench.
    const v2 = localStorage.getItem(STORAGE_LAYOUT_V2);
    if (v2) {
      const parsed = JSON.parse(v2) as { instances?: PanelInstance[] };
      if (Array.isArray(parsed.instances)) {
        const instances: PanelInstance[] = parsed.instances
          .filter(
            (i): i is PanelInstance =>
              !!i &&
              typeof i.id === 'string' &&
              typeof i.kind === 'string' &&
              DEFAULT_PANEL_ORDER.includes(i.kind as PanelKind) &&
              typeof i.width === 'number'
          )
          .map((i) => ({ id: i.id, kind: i.kind, width: i.width, name: '' }));
        layoutState.workbenches = [
          { id: layoutState.workbenches[0].id, name: 'Main', instances }
        ];
        layoutState.activeWorkbenchId = layoutState.workbenches[0].id;
        ensureAllInstancesNamed();
        return;
      }
    }

    // v1 → v3: stitch columns/order/widths together into instances.
    const c = localStorage.getItem(STORAGE_COLUMNS_V1);
    const o = localStorage.getItem(STORAGE_ORDER_V1);
    const wMap = localStorage.getItem(STORAGE_WIDTHS_V1);
    if (c || o || wMap) {
      const columns = safeParse<Partial<Record<PanelKind, boolean>>>(c) ?? {};
      const order = Array.isArray(safeParse<unknown>(o))
        ? (safeParse<PanelKind[]>(o) ?? []).filter((k) => DEFAULT_PANEL_ORDER.includes(k))
        : [];
      const widths = safeParse<Partial<Record<PanelKind, number>>>(wMap) ?? {};

      // Defaults for kinds the user never toggled off: github/jira/claude on,
      // cursor/sentry/editor off (added 2026-04-25 for sentry).
      const defaults: Record<PanelKind, boolean> = {
        github: true,
        jira: true,
        sentry: false,
        claude: true,
        cursor: false,
        editor: false
      };
      const resolved: Record<PanelKind, boolean> = { ...defaults };
      for (const k of DEFAULT_PANEL_ORDER) {
        if (typeof columns[k] === 'boolean') resolved[k] = columns[k] as boolean;
      }
      const instances: PanelInstance[] = [];
      const orderSet = order.length ? order : DEFAULT_PANEL_ORDER;
      for (const k of orderSet) {
        if (resolved[k]) {
          instances.push({
            id: genInstanceId(),
            kind: k,
            width:
              typeof widths[k] === 'number' && widths[k]! >= 240 && widths[k]! <= 2000
                ? (widths[k] as number)
                : DEFAULT_PANEL_WIDTHS[k],
            name: ''
          });
        }
      }
      layoutState.workbenches = [
        { id: layoutState.workbenches[0].id, name: 'Main', instances }
      ];
      layoutState.activeWorkbenchId = layoutState.workbenches[0].id;
      ensureAllInstancesNamed();
      return;
    }
  } catch {
    /* ignore */
  }
  // Default state (no storage): ensure the bootstrap "Main" workbench's
  // instances (none by default) are fine, and any later-added ones get names.
  ensureAllInstancesNamed();
}

function safeParse<T>(s: string | null): T | null {
  if (!s) return null;
  try {
    return JSON.parse(s) as T;
  } catch {
    return null;
  }
}

// ---- Instance-id-based handlers (primary API) ----

/** Callback invoked whenever an instance is removed from any workbench. Set by
 *  the sessions store at startup so sessions whose `columnInstanceId` pointed
 *  here are orphaned back to the floating pool. Avoids a circular import. */
let onInstanceRemoved: ((id: string) => void) | null = null;
export function registerInstanceRemovedHook(cb: (id: string) => void) {
  onInstanceRemoved = cb;
}

/** Remove the instance with `id` from whichever workbench it's in. Notifies
 *  the sessions store (via the registered hook) so its sessions float back. */
export function closePanelById(id: string) {
  for (const wb of layoutState.workbenches) {
    const idx = wb.instances.findIndex((i) => i.id === id);
    if (idx >= 0) {
      wb.instances = [...wb.instances.slice(0, idx), ...wb.instances.slice(idx + 1)];
      persistPanelState();
      onInstanceRemoved?.(id);
      return;
    }
  }
}

/** Archive an instance — detach it from its workbench but keep its
 *  identity + per-instance state (filters, items, chats) intact. The
 *  user can restore it later from the kind-pill menu and it'll go back
 *  to the same workbench / position it was at. Falls back to the
 *  current workbench if the original was deleted in the meantime.
 *
 *  Crucially we do NOT call `onInstanceRemoved` here — that hook drops
 *  the inboxState slots (and orphans sessions). Archive is the
 *  opposite intent. Only `closePanelById` fires the cleanup hook. */
export function archiveInstance(id: string) {
  for (const wb of layoutState.workbenches) {
    const idx = wb.instances.findIndex((i) => i.id === id);
    if (idx < 0) continue;
    const inst = wb.instances[idx];
    layoutState.archivedInstances = [
      ...layoutState.archivedInstances,
      { inst, originalWorkbenchId: wb.id, originalIndex: idx, archivedAt: Date.now() }
    ];
    wb.instances = [...wb.instances.slice(0, idx), ...wb.instances.slice(idx + 1)];
    persistPanelState();
    return;
  }
}

/** Restore an archived instance. Default target is the original
 *  workbench at the original index; if that workbench was deleted, we
 *  fall back to `fallbackWorkbenchId` (or the active workbench if
 *  none was given). Returns true on success, false only when the
 *  archive entry can't be found. */
export function unarchiveInstance(id: string, fallbackWorkbenchId?: string): boolean {
  const idx = layoutState.archivedInstances.findIndex((a) => a.inst.id === id);
  if (idx < 0) return false;
  const archived = layoutState.archivedInstances[idx];
  const target =
    layoutState.workbenches.find((w) => w.id === archived.originalWorkbenchId) ??
    layoutState.workbenches.find(
      (w) => w.id === (fallbackWorkbenchId ?? layoutState.activeWorkbenchId)
    ) ??
    layoutState.workbenches[0];
  if (!target) return false;
  const insertAt = Math.min(Math.max(0, archived.originalIndex), target.instances.length);
  target.instances = [
    ...target.instances.slice(0, insertAt),
    archived.inst,
    ...target.instances.slice(insertAt)
  ];
  layoutState.archivedInstances = [
    ...layoutState.archivedInstances.slice(0, idx),
    ...layoutState.archivedInstances.slice(idx + 1)
  ];
  persistPanelState();
  return true;
}

/** Archived instances of a given kind, newest-first, with the original
 *  workbench name resolved (or "Workbench gone" when the workbench was
 *  deleted between archive and now). Used by the pill menu to render
 *  greyed-out rows beside the live ones. */
export function listArchivedOfKind(
  kind: PanelKind
): { id: string; name: string; originalWorkbenchName: string; archivedAt: number }[] {
  return layoutState.archivedInstances
    .filter((a) => a.inst.kind === kind)
    .slice()
    .sort((a, b) => b.archivedAt - a.archivedAt)
    .map((a) => ({
      id: a.inst.id,
      name: a.inst.name,
      originalWorkbenchName:
        layoutState.workbenches.find((w) => w.id === a.originalWorkbenchId)?.name ??
        'Workbench gone',
      archivedAt: a.archivedAt
    }));
}

/** Move the instance left (-1) or right (+1) within its workbench. */
export function movePanelById(id: string, direction: -1 | 1) {
  for (const wb of layoutState.workbenches) {
    const idx = wb.instances.findIndex((i) => i.id === id);
    if (idx < 0) continue;
    const next = idx + direction;
    if (next < 0 || next >= wb.instances.length) return;
    const copy = [...wb.instances];
    [copy[idx], copy[next]] = [copy[next], copy[idx]];
    wb.instances = copy;
    persistPanelState();
    return;
  }
}

/** Relocate an instance to a different workbench, preserving its identity
 *  (id, width, name) and any sessions bound to it. The instance is appended
 *  to the target workbench's instance list. If `targetWorkbenchId` already
 *  contains the instance, this is a no-op.
 *
 *  Returns true on success, false when the source/target can't be resolved
 *  (caller can surface a toast). Singleton-kind clashes (two `github`
 *  panels in one workbench) are rejected since the inbox only renders the
 *  first one anyway — caller should prompt before calling. */
export function moveInstanceToWorkbench(
  instanceId: string,
  targetWorkbenchId: string
): boolean {
  let source: Workbench | null = null;
  let inst: PanelInstance | null = null;
  for (const wb of layoutState.workbenches) {
    const found = wb.instances.find((i) => i.id === instanceId);
    if (found) {
      source = wb;
      inst = found;
      break;
    }
  }
  if (!source || !inst) return false;
  if (source.id === targetWorkbenchId) return true;
  const target = layoutState.workbenches.find((w) => w.id === targetWorkbenchId);
  if (!target) return false;
  /* Per-instance refactor removed the old "one of each kind per
     workbench" rule — every column owns its own filter / item slot
     keyed by `instance.id`, so two Jira columns side by side can
     legitimately browse different boards. No singleton check here. */
  source.instances = source.instances.filter((i) => i.id !== instanceId);
  target.instances = [...target.instances, inst];
  persistPanelState();
  return true;
}

/** Append a new instance of `kind` to the active workbench and return
 *  its id. Always creates a fresh instance now that columns are
 *  per-instance — multiple github / jira / sentry columns are valid. */
export function addPanelInstance(kind: PanelKind): string {
  const wb = activeWorkbench();
  const id = genInstanceId();
  wb.instances = [
    ...wb.instances,
    { id, kind, width: DEFAULT_PANEL_WIDTHS[kind], name: pickInstanceName() }
  ];
  persistPanelState();
  return id;
}

/** Find the first visible instance of `kind` in the active workbench, or null
 *  if none exist. */
export function firstInstanceOfKind(kind: PanelKind): PanelInstance | null {
  return activeInstances().find((i) => i.kind === kind) ?? null;
}

/** Predicate used by layout code (snap math) and consumers that want to
 *  know whether an instance is actually rendered. Reads live connection
 *  status from the connections store. */
export function isInstanceVisible(inst: PanelInstance): boolean {
  if (inst.kind === 'github') return connectionsState.github.kind === 'connected';
  if (inst.kind === 'jira') return connectionsState.jira.kind === 'connected';
  if (inst.kind === 'sentry') return connectionsState.sentry.kind === 'connected';
  if (inst.kind === 'claude') return connectionsState.claude?.ready ?? false;
  if (inst.kind === 'cursor') return connectionsState.cursor?.ready ?? false;
  if (inst.kind === 'editor') return true;
  return false;
}

/** Compute a "clean" width for `instance` if `desired` is within the threshold
 *  of any of: fill-the-viewport, match another visible column, or a clean
 *  viewport fraction (1/4, 1/3, 1/2, 2/3, 3/4). */
export function snapWidth(instance: PanelInstance, desired: number, wb: HTMLElement): number {
  const MIN = 280;
  const MAX = 1600;
  const viewport = wb.clientWidth;

  const others = activeInstances().filter((i) => i.id !== instance.id && isInstanceVisible(i));

  // 1) "Fill" snap — pick the width that makes the sum of visible columns
  //    exactly equal the viewport (no horizontal scroll). The most
  //    satisfying snap because it's the "это красиво" moment.
  const otherSum = others.reduce((s, i) => s + i.width, 0);
  const fill = viewport - otherSum;
  if (fill >= MIN && fill <= MAX && Math.abs(desired - fill) < SNAP_THRESHOLD) {
    return fill;
  }

  // 2) Match-another-column — easy visual parity.
  for (const i of others) {
    if (Math.abs(desired - i.width) < SNAP_THRESHOLD) return i.width;
  }

  // 3) Clean fractions of the viewport.
  const fractions = [0.25, 1 / 3, 0.5, 2 / 3, 0.75];
  for (const f of fractions) {
    const target = Math.round(viewport * f);
    if (target >= MIN && target <= MAX && Math.abs(desired - target) < SNAP_THRESHOLD) {
      return target;
    }
  }

  // 4) Round to a 10px grid so everything ends up on clean values anyway.
  return Math.round(desired / 10) * 10;
}

let snapFlashTimer: ReturnType<typeof setTimeout> | null = null;
export function flashSnap(instanceId: string) {
  if (layoutState.snapFlashInstanceId === instanceId) return;
  layoutState.snapFlashInstanceId = instanceId;
  if (snapFlashTimer) clearTimeout(snapFlashTimer);
  snapFlashTimer = setTimeout(() => {
    layoutState.snapFlashInstanceId = null;
  }, 160);
}

/** Pointer-drag resize gesture for a column. Reads live `isInstanceVisible`
 *  for the "fill" snap point math. */
export function startResizeById(instanceId: string, ev: PointerEvent) {
  if (ev.button !== 0) return;
  const inst = activeInstances().find((i) => i.id === instanceId);
  if (!inst) return;
  const startX = ev.clientX;
  const startW = inst.width;
  const wb = document.querySelector('.wb-columns') as HTMLElement | null;
  if (!wb) return;

  let autoscrollRaf: number | null = null;
  let pointerX = ev.clientX;
  let latestDeltaX = 0;
  const startScrollLeft = wb.scrollLeft;

  function step() {
    const rect = wb!.getBoundingClientRect();
    const vw = window.innerWidth || document.documentElement.clientWidth;
    const effectiveRight = Math.min(rect.right, vw);
    const effectiveLeft = Math.max(rect.left, 0);
    const edge = 80;
    const maxStep = 30;
    let dx = 0;
    if (pointerX > effectiveRight - edge) {
      dx = Math.min(maxStep, Math.max(4, Math.round((pointerX - (effectiveRight - edge)) / 3)));
    } else if (pointerX < effectiveLeft + edge) {
      dx = -Math.min(maxStep, Math.max(4, Math.round(((effectiveLeft + edge) - pointerX) / 3)));
    }
    const before = wb!.scrollLeft;
    if (dx !== 0) {
      wb!.scrollLeft = Math.max(0, Math.min(wb!.scrollWidth - wb!.clientWidth, before + dx));
    }
    const drift = wb!.scrollLeft - startScrollLeft;
    const effective = latestDeltaX + drift;
    const rawNext = Math.max(280, Math.min(1600, startW + effective));
    // Re-find the instance each tick (could be reordered/deleted mid-drag).
    const live = activeInstances().find((i) => i.id === instanceId);
    if (!live) { autoscrollRaf = requestAnimationFrame(step); return; }
    const snapped = snapWidth(live, rawNext, wb!);
    if (snapped !== live.width) live.width = snapped;
    if (snapped !== rawNext) flashSnap(instanceId);
    autoscrollRaf = requestAnimationFrame(step);
  }

  const onMove = (e: PointerEvent) => {
    pointerX = e.clientX;
    latestDeltaX = e.clientX - startX;
    if (autoscrollRaf === null) autoscrollRaf = requestAnimationFrame(step);
  };
  const onUp = () => {
    if (autoscrollRaf !== null) cancelAnimationFrame(autoscrollRaf);
    autoscrollRaf = null;
    persistPanelState();
    window.removeEventListener('pointermove', onMove);
    window.removeEventListener('pointerup', onUp);
  };
  window.addEventListener('pointermove', onMove);
  window.addEventListener('pointerup', onUp);
  ev.preventDefault();
  autoscrollRaf = requestAnimationFrame(step);
}

/** Flat list of all instances of `kind` across every workbench, tagged
 *  with the workbench they live in. Used by the workbench bar pills to
 *  show a count + a per-instance hover menu. */
export function listInstancesOfKind(
  kind: PanelKind
): { id: string; name: string; workbenchId: string; workbenchName: string }[] {
  const out: { id: string; name: string; workbenchId: string; workbenchName: string }[] = [];
  for (const wb of layoutState.workbenches) {
    for (const inst of wb.instances) {
      if (inst.kind === kind) {
        out.push({
          id: inst.id,
          name: inst.name,
          workbenchId: wb.id,
          workbenchName: wb.name
        });
      }
    }
  }
  return out;
}

/** Jump to `instanceId` in whichever workbench owns it. Switches the
 *  active workbench first if needed, then scrolls the column into view. */
export async function goToInstance(instanceId: string, workbenchId: string) {
  if (workbenchId !== layoutState.activeWorkbenchId) {
    setActiveWorkbench(workbenchId);
  }
  await scrollInstanceIntoView(instanceId);
}

/** Scroll-into-view the given instance in the current workbench. */
export async function scrollInstanceIntoView(instanceId: string) {
  await tick();
  const wb = document.querySelector('.wb-columns') as HTMLElement | null;
  if (!wb) return;
  const idx = activeInstances().findIndex((i) => i.id === instanceId);
  if (idx < 0) return;
  const cols = Array.from(wb.children) as HTMLElement[];
  const target = cols.find((c) => c.classList.contains('wb-column') && c.style.order === String(idx));
  target?.scrollIntoView({ behavior: 'smooth', inline: 'nearest', block: 'nearest' });
}

/** Scroll the first visible instance of `kind` (in the active workbench) into
 *  view. If none exists, create one. Used by pill clicks. */
export async function scrollKindIntoView(kind: PanelKind) {
  let inst = firstInstanceOfKind(kind);
  if (!inst) {
    const id = addPanelInstance(kind);
    await tick();
    inst = activeInstances().find((i) => i.id === id) ?? null;
  }
  if (inst) await scrollInstanceIntoView(inst.id);
}

// ---- Workbench-level operations ----

export function addWorkbench(name = 'Workbench'): string {
  const id = genInstanceId();
  layoutState.workbenches = [...layoutState.workbenches, { id, name, instances: [] }];
  persistPanelState();
  return id;
}

export function removeWorkbench(id: string): PanelInstance[] {
  const doomed = layoutState.workbenches.find((w) => w.id === id);
  if (!doomed) return [];
  const rest = layoutState.workbenches.filter((w) => w.id !== id);
  if (rest.length === 0) return []; // refuse to delete last — caller should check
  layoutState.workbenches = rest;
  if (layoutState.activeWorkbenchId === id) {
    layoutState.activeWorkbenchId = rest[0].id;
  }
  // Orphan every session that was pinned to an instance in the doomed workbench.
  for (const inst of doomed.instances) onInstanceRemoved?.(inst.id);
  persistPanelState();
  return doomed.instances;
}

export function renameWorkbench(id: string, name: string) {
  const wb = layoutState.workbenches.find((w) => w.id === id);
  if (!wb) return;
  wb.name = name.trim() || wb.name;
  persistPanelState();
}

export function setActiveWorkbench(id: string) {
  if (!layoutState.workbenches.find((w) => w.id === id)) return;
  layoutState.activeWorkbenchId = id;
  persistPanelState();
}
