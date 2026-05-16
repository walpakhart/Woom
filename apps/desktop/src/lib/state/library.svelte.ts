/* Library state — pluggable source list. Each `LibrarySource` describes
 * where catalog entries come from; we fetch them in parallel at refresh
 * time and merge the results into a single `entries` array.
 *
 * Built-in sources (always shipped, can be disabled but not removed):
 *   - Anthropic plugins (the official `claude-plugins-official` marketplace)
 *   - Anthropic skills (`anthropics/skills` repo)
 *   - Woom defaults (`/library.json` bundled with the app)
 *
 * User-added sources persist in `localStorage` and can be either a
 * plugin marketplace (any GitHub repo carrying `.claude-plugin/marketplace.json`)
 * or a skill repo (any GitHub repo with `<root>/<slug>/SKILL.md` folders).
 *
 * "Installed" comes from the Tauri command `library_list_installed`
 * which scans `~/.claude/skills` + `~/.claude/plugins`. */

import { invoke } from '@tauri-apps/api/core';
import { loadVersioned, saveVersioned } from '$lib/state/persist';

export type EntryKind = 'skill' | 'plugin';

export type SourceKind = 'plugin-marketplace' | 'skill-repo' | 'bundled';

export interface LibrarySource {
  /** Stable id, used as the prefix for entry ids + as the filter key. */
  id: string;
  kind: SourceKind;
  /** Human-readable label surfaced in chips, filters, error toasts. */
  label: string;
  /** GitHub repo `owner/name` — required for plugin-marketplace + skill-repo
   *  unless an explicit URL overrides. */
  repo?: string;
  /** Subdirectory inside the repo that holds per-skill folders. Defaults
   *  to `skills` (matches `anthropics/skills`). */
  rootPath?: string;
  /** Explicit marketplace.json URL — overrides the repo-derived default
   *  (`https://raw.githubusercontent.com/<repo>/HEAD/.claude-plugin/marketplace.json`). */
  marketplaceUrl?: string;
  /** Bundled-source URL (only used for the Woom default). */
  bundledUrl?: string;
  /** Built-ins can't be removed — only toggled. */
  builtin: boolean;
  /** When false, the source is skipped at refresh time. */
  enabled: boolean;
}

export type CatalogSource =
  | { type: 'inline'; slug: string; content: string }
  | { type: 'git'; slug: string; url: string }
  | { type: 'plugin'; reference: string; marketplace?: string }
  | { type: 'anthropic-skill'; slug: string }
  | { type: 'anthropic-plugin'; name: string; homepage?: string }
  | { type: 'remote-skill'; slug: string; repo: string; root: string };

export interface CatalogEntry {
  id: string;
  kind: EntryKind;
  name: string;
  description: string;
  tags: string[];
  author: string;
  source: CatalogSource;
  note?: string;
  /** "Anthropic & Partners" / "Woom defaults" / source.label — surfaced
   *  as a badge so the user knows where the entry came from. */
  origin: string;
  /** ID of the `LibrarySource` that produced this entry — drives the
   *  per-source filter chip in Browse. */
  sourceId: string;
  /** Plugin-only — `category` from the marketplace manifest. */
  category?: string;
}

export interface InstalledSkill {
  slug: string;
  name: string;
  description: string;
  path: string;
}

export interface InstalledPlugin {
  /** Bare plugin name (part before `@` in the Claude CLI reference). */
  name: string;
  /** Marketplace identifier (part after `@`). Empty when the manifest
   *  entry didn't carry one — practically never. */
  marketplace: string;
  /** Resolved version recorded at install time. */
  version: string;
  path: string;
}

export interface InstalledList {
  skills: InstalledSkill[];
  plugins: InstalledPlugin[];
}

interface BundledCatalog {
  version: number;
  updated: string;
  entries: Array<Omit<CatalogEntry, 'origin' | 'sourceId'>>;
}

/* Pre-curated source list shipped with the app. Each entry was verified
 * to (a) exist on GitHub at the listed path and (b) carry a real plugin
 * `marketplace.json` or `<root>/<slug>/SKILL.md` layout. Enabled-by-default
 * is reserved for low-risk official-ish feeds; community ones ship
 * disabled so the user opts in to the extra network calls + content
 * curation. Counts are approximate (as of late-2026 verification) and
 * exist only as a hint — the live fetch is the source of truth. */
const BUILTIN_SOURCES: LibrarySource[] = [
  /* ── Official Anthropic feeds ─────────────────────────────────── */
  {
    id: 'anthropic-plugins',
    kind: 'plugin-marketplace',
    label: 'Anthropic Official',
    repo: 'anthropics/claude-plugins-official',
    builtin: true,
    enabled: true
  },
  {
    id: 'anthropic-plugins-community',
    kind: 'plugin-marketplace',
    label: 'Anthropic Community',
    repo: 'anthropics/claude-plugins-community',
    builtin: true,
    enabled: true
  },
  {
    id: 'anthropic-skills',
    kind: 'skill-repo',
    label: 'Anthropic skills',
    repo: 'anthropics/skills',
    rootPath: 'skills',
    builtin: true,
    enabled: true
  },
  /* ── Woom-bundled inline skills ───────────────────────────────── */
  {
    id: 'woom-defaults',
    kind: 'bundled',
    label: 'Woom defaults',
    bundledUrl: '/library.json',
    builtin: true,
    enabled: true
  },
  /* ── Community plugin marketplaces (off by default) ──────────── */
  {
    id: 'wshobson-agents',
    kind: 'plugin-marketplace',
    label: 'wshobson · agents',
    repo: 'wshobson/agents',
    builtin: true,
    enabled: false
  },
  {
    id: 'ccplugins-awesome',
    kind: 'plugin-marketplace',
    label: 'ccplugins · awesome',
    repo: 'ccplugins/awesome-claude-code-plugins',
    builtin: true,
    enabled: false
  },
  {
    id: 'composio-awesome',
    kind: 'plugin-marketplace',
    label: 'Composio · awesome plugins',
    repo: 'ComposioHQ/awesome-claude-plugins',
    builtin: true,
    enabled: false
  },
  {
    id: 'payrequest-plugins',
    kind: 'plugin-marketplace',
    label: 'PayRequest · plugins',
    repo: 'PayRequest/claude-plugins',
    builtin: true,
    enabled: false
  },
  {
    id: 'dev-gom-marketplace',
    kind: 'plugin-marketplace',
    label: 'Dev-GOM · marketplace',
    repo: 'Dev-GOM/claude-code-marketplace',
    builtin: true,
    enabled: false
  },
  /* ── Community skill repos ───────────────────────────────────── */
  /* These three are ENABLED by default because the Anthropic skills
     repo alone yields only ~17 entries, and "Skills" is the headline
     filter — a near-empty Skills tab makes the Library feel stunted.
     All three were verified to follow the `<slug>/SKILL.md` shape at
     the repo root (no `skills/` parent), which the empty `rootPath`
     triggers. SKILL.md absence filters out housekeeping folders. */
  {
    id: 'glebis-skills',
    kind: 'skill-repo',
    label: 'glebis · skills',
    repo: 'glebis/claude-skills',
    rootPath: '',
    builtin: true,
    enabled: true
  },
  {
    id: 'daymade-skills',
    kind: 'skill-repo',
    label: 'daymade · skills',
    repo: 'daymade/claude-code-skills',
    rootPath: '',
    builtin: true,
    enabled: true
  },
  {
    id: 'alirezarezvani-skills',
    kind: 'skill-repo',
    label: 'alirezarezvani · skills',
    repo: 'alirezarezvani/claude-skills',
    rootPath: '',
    builtin: true,
    enabled: true
  }
];

const SOURCES_STORAGE_KEY = 'woom.library.sources';
const SOURCES_STORAGE_VERSION = 2;

type StoredSource =
  /* Built-ins only persist the toggle — the rest comes from the constant
     above so a code update can bump labels / URLs without a migration. */
  | { id: string; builtin: true; enabled: boolean }
  | (LibrarySource & { builtin: false });

/* v1→v2: the first cut of the source list shipped these three skill
 * repos as `enabled: false`. After realising the Skills filter was
 * starved (~21 entries total), they flipped to `enabled: true` in the
 * BUILTIN constant. Existing localStorage snapshots from v1 still carry
 * the explicit `false`, which would otherwise win on merge. Dropping
 * those rows lets the new defaults take effect without nuking the
 * user's other toggle preferences. */
const V2_RESET_BUILTIN_IDS = new Set([
  'glebis-skills',
  'daymade-skills',
  'alirezarezvani-skills'
]);

function loadSources(): LibrarySource[] {
  const stored = loadVersioned<StoredSource[]>(SOURCES_STORAGE_KEY, {
    version: SOURCES_STORAGE_VERSION,
    migrations: {
      2: (data: unknown): StoredSource[] => {
        if (!Array.isArray(data)) return [];
        return (data as StoredSource[]).filter(
          (s) => !(s.builtin && V2_RESET_BUILTIN_IDS.has(s.id))
        );
      }
    }
  });
  if (!stored || !Array.isArray(stored)) return BUILTIN_SOURCES.map((s) => ({ ...s }));
  const overrides = new Map<string, StoredSource>();
  for (const s of stored) overrides.set(s.id, s);
  const merged: LibrarySource[] = BUILTIN_SOURCES.map((b) => {
    const o = overrides.get(b.id);
    if (o && o.builtin) return { ...b, enabled: o.enabled };
    return { ...b };
  });
  for (const s of stored) {
    if (!s.builtin && !merged.some((m) => m.id === s.id)) merged.push(s);
  }
  return merged;
}

function persistSources(sources: LibrarySource[]): void {
  const payload: StoredSource[] = sources.map((s) =>
    s.builtin ? { id: s.id, builtin: true, enabled: s.enabled } : { ...s, builtin: false }
  );
  saveVersioned(SOURCES_STORAGE_KEY, SOURCES_STORAGE_VERSION, payload);
}

export const libraryState = $state<{
  entries: CatalogEntry[];
  installed: InstalledList;
  busy: Set<string>;
  catalogError: string | null;
  installedError: string | null;
  warnings: string[];
  query: string;
  kindFilter: EntryKind | null;
  categoryFilter: string | null;
  /** Null = all sources; otherwise narrow to the given source.id. */
  sourceFilter: string | null;
  loadingCatalog: boolean;
  loaded: boolean;
  sources: LibrarySource[];
}>({
  entries: [],
  installed: { skills: [], plugins: [] },
  busy: new Set(),
  catalogError: null,
  installedError: null,
  warnings: [],
  query: '',
  kindFilter: null,
  categoryFilter: null,
  sourceFilter: null,
  loadingCatalog: false,
  loaded: false,
  sources: loadSources()
});

function parseFrontmatter(content: string): { name?: string; description?: string } {
  const lines = content.split('\n');
  if (lines[0]?.trim() !== '---') return {};
  const out: { name?: string; description?: string } = {};
  for (let i = 1; i < lines.length; i++) {
    const t = lines[i].trim();
    if (t === '---') break;
    if (t.startsWith('name:')) {
      out.name = t.slice(5).trim().replace(/^"|"$/g, '');
    } else if (t.startsWith('description:')) {
      out.description = t.slice(12).trim().replace(/^"|"$/g, '');
    }
  }
  return out;
}

function marketplaceUrlFor(s: LibrarySource): string {
  if (s.marketplaceUrl) return s.marketplaceUrl;
  if (!s.repo) throw new Error('marketplace source missing both repo and URL');
  return `https://raw.githubusercontent.com/${s.repo}/HEAD/.claude-plugin/marketplace.json`;
}

async function fetchPluginMarketplace(s: LibrarySource): Promise<CatalogEntry[]> {
  const url = marketplaceUrlFor(s);
  const res = await fetch(url, { cache: 'no-cache' });
  if (!res.ok) throw new Error(`HTTP ${res.status}`);
  const body = (await res.json()) as {
    name?: string;
    plugins?: Array<{
      name: string;
      description?: string;
      author?: { name?: string };
      category?: string;
      homepage?: string;
    }>;
  };
  const arr = body.plugins ?? [];
  /* The `marketplace.json` MAY carry a top-level `name` field that Claude
     CLI uses as the lookup key for `<plugin>@<marketplace>` references.
     Fall back to the repo's tail segment so partner marketplaces without
     an explicit name still work for the common GitHub layout. */
  const marketplaceName =
    body.name ??
    (s.repo ? s.repo.split('/').pop() : undefined) ??
    s.id;
  return arr.map((p) => ({
    id: `${s.id}:${p.name}`,
    kind: 'plugin' as const,
    name: p.name,
    description: p.description ?? '',
    tags: p.category ? [p.category] : [],
    author: p.author?.name ?? s.label,
    source:
      s.id === 'anthropic-plugins'
        ? { type: 'anthropic-plugin' as const, name: p.name, homepage: p.homepage }
        : {
            type: 'plugin' as const,
            reference: `${p.name}@${marketplaceName}`,
            marketplace: s.repo ?? url
          },
    origin: s.label,
    sourceId: s.id,
    category: p.category
  }));
}

async function fetchSkillRepo(s: LibrarySource): Promise<CatalogEntry[]> {
  if (!s.repo) throw new Error('skill-repo source missing repo');
  const rawRoot = (s.rootPath ?? 'skills').trim().replace(/^\/+|\/+$/g, '');
  /* Empty / "." root means "scan the repo's root directory" — useful for
     repos that store skill folders at top level (no `skills/` parent).
     `apiSuffix` and `rawSuffix` collapse to empty strings in that case so
     the URLs stay clean (no double slashes). */
  const isRoot = rawRoot === '' || rawRoot === '.';
  const apiSuffix = isRoot ? '' : `/${rawRoot}`;
  const rawSuffix = isRoot ? '' : `/${rawRoot}`;
  const listingUrl = `https://api.github.com/repos/${s.repo}/contents${apiSuffix}`;
  const res = await fetch(listingUrl, { cache: 'no-cache' });
  if (!res.ok) throw new Error(`HTTP ${res.status}`);
  const items = (await res.json()) as Array<{ name: string; type: string }>;
  /* Skip dot-prefixed dirs (`.github`, `.claude-plugin`) and well-known
     non-skill folders so root-scanning repos don't surface housekeeping
     directories as "skills". */
  const ignore = new Set(['scripts', 'demos', 'examples', 'references', 'docs', 'bin']);
  const dirs = items
    .filter((x) => x.type === 'dir' && !x.name.startsWith('.') && !ignore.has(x.name))
    .map((x) => x.name);
  const rawBase = `https://raw.githubusercontent.com/${s.repo}/HEAD${rawSuffix}`;
  const detailed = await Promise.all(
    dirs.map(async (slug) => {
      try {
        const r = await fetch(`${rawBase}/${slug}/SKILL.md`, { cache: 'no-cache' });
        if (!r.ok) return null;
        const md = await r.text();
        const fm = parseFrontmatter(md);
        return { slug, name: fm.name ?? slug, description: fm.description ?? '' };
      } catch {
        return null;
      }
    })
  );
  /* Filter out dirs that have no SKILL.md — they're not skills, just
     incidental subfolders (especially common when scanning the repo
     root). */
  const valid = detailed.filter(
    (x): x is { slug: string; name: string; description: string } => x !== null
  );
  const isAnthropicBuiltin = s.id === 'anthropic-skills';
  const storedRoot = isRoot ? '' : rawRoot;
  return valid.map((d) => ({
    id: `${s.id}:${d.slug}`,
    kind: 'skill' as const,
    name: d.name,
    description: d.description,
    tags: [],
    author: s.label,
    /* The Anthropic built-in keeps its dedicated source type so the
       existing cache + Tauri command flow stays untouched. Everyone else
       routes through `remote-skill`, which the backend services via the
       generic per-repo cache. */
    source: isAnthropicBuiltin
      ? { type: 'anthropic-skill' as const, slug: d.slug }
      : { type: 'remote-skill' as const, slug: d.slug, repo: s.repo!, root: storedRoot },
    origin: s.label,
    sourceId: s.id
  }));
}

async function fetchBundled(s: LibrarySource): Promise<CatalogEntry[]> {
  const url = s.bundledUrl ?? '/library.json';
  const res = await fetch(url, { cache: 'no-cache' });
  if (!res.ok) return [];
  const body = (await res.json()) as BundledCatalog;
  return (body.entries ?? []).map((e) => ({
    ...e,
    origin: s.label,
    sourceId: s.id
  }));
}

async function fetchSource(s: LibrarySource): Promise<CatalogEntry[]> {
  if (s.kind === 'plugin-marketplace') return fetchPluginMarketplace(s);
  if (s.kind === 'skill-repo') return fetchSkillRepo(s);
  return fetchBundled(s);
}

export async function loadCatalog(): Promise<void> {
  if (libraryState.loadingCatalog) return;
  libraryState.loadingCatalog = true;
  libraryState.warnings = [];
  try {
    const active = libraryState.sources.filter((s) => s.enabled);
    const results = await Promise.allSettled(active.map((s) => fetchSource(s)));
    const merged: CatalogEntry[] = [];
    results.forEach((r, i) => {
      const s = active[i];
      if (r.status === 'fulfilled') merged.push(...r.value);
      else libraryState.warnings.push(`${s.label}: ${String(r.reason)}`);
    });
    /* Skills first, then plugins, alphabetical within each — matches
       how Claude.ai's Directory presents them. */
    merged.sort((a, b) => {
      if (a.kind !== b.kind) return a.kind === 'skill' ? -1 : 1;
      return a.name.localeCompare(b.name);
    });
    libraryState.entries = merged;
    libraryState.catalogError =
      merged.length === 0 && active.length === 0
        ? 'No sources enabled — turn one on in Sources.'
        : merged.length === 0
        ? 'No entries from any enabled source.'
        : null;
  } catch (e) {
    libraryState.catalogError = String(e);
  } finally {
    libraryState.loadingCatalog = false;
  }
}

export async function refreshInstalled(): Promise<void> {
  try {
    const list = await invoke<InstalledList>('library_list_installed');
    libraryState.installed = list;
    libraryState.installedError = null;
  } catch (e) {
    libraryState.installedError = String(e);
  }
}

export function slugForEntry(e: CatalogEntry): string | null {
  if (e.kind !== 'skill') return null;
  if (e.source.type === 'inline' || e.source.type === 'git') return e.source.slug;
  if (e.source.type === 'anthropic-skill') return e.source.slug;
  if (e.source.type === 'remote-skill') return e.source.slug;
  return null;
}

export function isInstalled(e: CatalogEntry): boolean {
  if (e.kind === 'skill') {
    const slug = slugForEntry(e);
    if (!slug) return false;
    return libraryState.installed.skills.some((s) => s.slug === slug);
  }
  if (e.source.type === 'plugin') {
    const [name, marketplace] = e.source.reference.split('@');
    /* Match on (name, marketplace) so the same plugin name in two
       different marketplaces doesn't show both as installed. */
    return libraryState.installed.plugins.some(
      (p) => p.name === name && (!marketplace || p.marketplace === marketplace)
    );
  }
  if (e.source.type === 'anthropic-plugin') {
    const name = e.source.name;
    return libraryState.installed.plugins.some(
      (p) => p.name === name && p.marketplace === 'claude-plugins-official'
    );
  }
  return false;
}

export async function installEntry(e: CatalogEntry): Promise<void> {
  if (libraryState.busy.has(e.id)) return;
  libraryState.busy = new Set([...libraryState.busy, e.id]);
  try {
    if (e.source.type === 'inline') {
      await invoke('library_install_skill_inline', {
        slug: e.source.slug,
        content: e.source.content
      });
    } else if (e.source.type === 'git') {
      await invoke('library_install_skill_git', {
        url: e.source.url,
        slug: e.source.slug
      });
    } else if (e.source.type === 'anthropic-skill') {
      await invoke('library_install_anthropic_skill', { name: e.source.slug });
    } else if (e.source.type === 'remote-skill') {
      await invoke('library_install_skill_from_repo', {
        repo: e.source.repo,
        slug: e.source.slug,
        root: e.source.root
      });
    } else if (e.source.type === 'anthropic-plugin') {
      await invoke('library_plugin_install_anthropic', { name: e.source.name });
    } else if (e.source.type === 'plugin') {
      if (e.source.marketplace) {
        await invoke('library_plugin_marketplace_add', { url: e.source.marketplace });
      }
      await invoke('library_plugin_install', { reference: e.source.reference });
    }
    await refreshInstalled();
  } finally {
    const next = new Set(libraryState.busy);
    next.delete(e.id);
    libraryState.busy = next;
  }
}

export async function uninstallSkill(slug: string): Promise<void> {
  const key = `skill:${slug}`;
  if (libraryState.busy.has(key)) return;
  libraryState.busy = new Set([...libraryState.busy, key]);
  try {
    await invoke('library_uninstall_skill', { slug });
    await refreshInstalled();
  } finally {
    const next = new Set(libraryState.busy);
    next.delete(key);
    libraryState.busy = next;
  }
}

export async function uninstallPlugin(name: string, marketplace?: string): Promise<void> {
  /* Disambiguate against same-named plugins in other marketplaces by
     passing `<name>@<marketplace>` to `claude plugin uninstall`. The
     CLI accepts either form; the qualified one is safer. */
  const reference = marketplace ? `${name}@${marketplace}` : name;
  const key = `plugin:${reference}`;
  if (libraryState.busy.has(key)) return;
  libraryState.busy = new Set([...libraryState.busy, key]);
  try {
    await invoke('library_plugin_uninstall', { name: reference });
    await refreshInstalled();
  } finally {
    const next = new Set(libraryState.busy);
    next.delete(key);
    libraryState.busy = next;
  }
}

export function pluginCategories(): { name: string; count: number }[] {
  const counts = new Map<string, number>();
  for (const e of libraryState.entries) {
    if (e.kind !== 'plugin') continue;
    const c = e.category ?? 'uncategorized';
    counts.set(c, (counts.get(c) ?? 0) + 1);
  }
  return Array.from(counts.entries())
    .map(([name, count]) => ({ name, count }))
    .sort((a, b) => b.count - a.count || a.name.localeCompare(b.name));
}

/** Active sources sorted by entry count desc — drives the source filter
 *  row in Browse. */
export function sourceStats(): { source: LibrarySource; count: number }[] {
  const counts = new Map<string, number>();
  for (const e of libraryState.entries) {
    counts.set(e.sourceId, (counts.get(e.sourceId) ?? 0) + 1);
  }
  return libraryState.sources
    .filter((s) => s.enabled)
    .map((source) => ({ source, count: counts.get(source.id) ?? 0 }))
    .sort((a, b) => b.count - a.count || a.source.label.localeCompare(b.source.label));
}

export async function ensureLibraryLoaded(): Promise<void> {
  if (!libraryState.loaded) {
    await Promise.all([loadCatalog(), refreshInstalled()]);
    libraryState.loaded = true;
  } else {
    await refreshInstalled();
  }
}

/* ─── Source CRUD ───────────────────────────────────────────────── */

function commitSources(next: LibrarySource[]): void {
  libraryState.sources = next;
  persistSources(next);
}

export async function toggleSource(id: string, enabled: boolean): Promise<void> {
  const next = libraryState.sources.map((s) =>
    s.id === id ? { ...s, enabled } : s
  );
  commitSources(next);
  await loadCatalog();
}

export async function removeSource(id: string): Promise<void> {
  const s = libraryState.sources.find((x) => x.id === id);
  if (!s || s.builtin) return; /* built-ins are toggle-only */
  commitSources(libraryState.sources.filter((x) => x.id !== id));
  /* Drop entries from this source so the UI updates immediately. The
     next `loadCatalog` would do the same, but it'd hit the network
     first and leave stale entries on screen until it finished. */
  libraryState.entries = libraryState.entries.filter((e) => e.sourceId !== id);
  if (libraryState.sourceFilter === id) libraryState.sourceFilter = null;
}

/** Normalize a user-pasted GitHub identifier (`owner/repo`, full HTTPS
 *  URL, or `git@github.com:owner/repo`) into `owner/repo`. Returns null
 *  on something we can't confidently rewrite. */
export function normalizeRepo(input: string): string | null {
  const t = input.trim();
  if (!t) return null;
  /* git@github.com:owner/repo(.git) */
  const ssh = /^git@github\.com:([^/]+)\/([^/]+?)(?:\.git)?$/.exec(t);
  if (ssh) return `${ssh[1]}/${ssh[2]}`;
  /* https://github.com/owner/repo(.git)(/...) */
  const https = /^https?:\/\/github\.com\/([^/]+)\/([^/?#.]+)/.exec(t);
  if (https) return `${https[1]}/${https[2]}`;
  /* Bare `owner/repo` */
  if (/^[^/\s]+\/[^/\s]+$/.test(t)) return t;
  return null;
}

interface AddSourceInput {
  kind: SourceKind;
  label?: string;
  /** GitHub `owner/repo` or full URL — normalized internally. */
  repo?: string;
  rootPath?: string;
  /** Explicit marketplace.json URL (alternative to repo). */
  marketplaceUrl?: string;
}

/** Append a user-defined source. Returns the new id (or throws on
 *  invalid input). */
export async function addSource(input: AddSourceInput): Promise<string> {
  if (input.kind === 'bundled') throw new Error("can't add bundled sources");
  let repo: string | undefined;
  if (input.repo) {
    const norm = normalizeRepo(input.repo);
    if (!norm) throw new Error(`unrecognized repo: "${input.repo}"`);
    repo = norm;
  }
  if (input.kind === 'plugin-marketplace' && !repo && !input.marketplaceUrl) {
    throw new Error('plugin marketplace needs a GitHub repo or marketplace.json URL');
  }
  if (input.kind === 'skill-repo' && !repo) {
    throw new Error('skill repo needs a GitHub owner/repo');
  }
  const base = repo ?? input.marketplaceUrl ?? '';
  /* Stable id derived from repo / URL so adding the same source twice
     replaces rather than duplicates. */
  const id = base
    .replace(/^https?:\/\//, '')
    .replace(/[^A-Za-z0-9-_]+/g, '-')
    .replace(/^-+|-+$/g, '')
    .toLowerCase();
  if (!id) throw new Error('could not derive id');
  const label = input.label?.trim() || repo || input.marketplaceUrl || id;
  const next: LibrarySource = {
    id,
    kind: input.kind,
    label,
    repo,
    rootPath: input.rootPath?.trim() || undefined,
    marketplaceUrl: input.marketplaceUrl?.trim() || undefined,
    builtin: false,
    enabled: true
  };
  const filtered = libraryState.sources.filter((s) => s.id !== id);
  filtered.push(next);
  commitSources(filtered);
  await loadCatalog();
  return id;
}
