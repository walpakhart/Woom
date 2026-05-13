/* Library state — combines Anthropic's official directories with our
 * own curated bundle. Three live sources fold into one `entries` list:
 *
 *   1. Anthropic plugins — pulled at runtime from
 *      anthropics/claude-plugins-official/.claude-plugin/marketplace.json
 *      (169 entries; install via `claude plugin install`).
 *   2. Anthropic skills — directory listing from anthropics/skills/skills
 *      (per-folder fetch of `SKILL.md` frontmatter for name/description).
 *   3. Woom defaults — the small bundle in `/library.json` (inline
 *      one-shot skills + any extras we ship).
 *
 * "Installed" comes from the Tauri command `library_list_installed`
 * which scans `~/.claude/skills` + `~/.claude/plugins`. */

import { invoke } from '@tauri-apps/api/core';

export type EntryKind = 'skill' | 'plugin';

export type CatalogSource =
  | { type: 'inline'; slug: string; content: string }
  | { type: 'git'; slug: string; url: string }
  | { type: 'plugin'; reference: string; marketplace?: string }
  | { type: 'anthropic-skill'; slug: string }
  | { type: 'anthropic-plugin'; name: string; homepage?: string };

export interface CatalogEntry {
  id: string;
  kind: EntryKind;
  name: string;
  description: string;
  tags: string[];
  author: string;
  source: CatalogSource;
  note?: string;
  /** "Anthropic & Partners" / "Woom defaults" — surfaced as a badge so
   *  the user knows where the entry came from. */
  origin: string;
  /** Plugin-only — `category` from the Anthropic manifest (design,
   *  development, security, …). Drives the kind filter chips. */
  category?: string;
}

export interface InstalledSkill {
  slug: string;
  name: string;
  description: string;
  path: string;
}

export interface InstalledPlugin {
  name: string;
  path: string;
}

export interface InstalledList {
  skills: InstalledSkill[];
  plugins: InstalledPlugin[];
}

interface BundledCatalog {
  version: number;
  updated: string;
  entries: CatalogEntry[];
}

const ANTHROPIC_PLUGINS_MANIFEST =
  'https://raw.githubusercontent.com/anthropics/claude-plugins-official/main/.claude-plugin/marketplace.json';
const ANTHROPIC_SKILLS_LISTING =
  'https://api.github.com/repos/anthropics/skills/contents/skills';
const ANTHROPIC_SKILL_RAW =
  'https://raw.githubusercontent.com/anthropics/skills/main/skills';

export const libraryState = $state<{
  entries: CatalogEntry[];
  installed: InstalledList;
  busy: Set<string>;
  catalogError: string | null;
  installedError: string | null;
  /** Sub-errors from each upstream — surfaced quietly so a Skill-feed
   *  outage doesn't blank the Plugins list and vice versa. */
  warnings: string[];
  query: string;
  kindFilter: EntryKind | null;
  /** Optional category narrowing (plugin-only). Null = all. */
  categoryFilter: string | null;
  /** True while a catalog refresh is in flight — disables the Refresh
   *  button so a slow GitHub fetch doesn't stack on itself. */
  loadingCatalog: boolean;
  loaded: boolean;
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
  loadingCatalog: false,
  loaded: false
});

/** Lightweight frontmatter parse — Claude skill SKILL.md only carries
 *  shallow top-level keys, so we don't need a real YAML lib. Strips
 *  surrounding quotes and trailing whitespace. */
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

async function fetchAnthropicPlugins(): Promise<CatalogEntry[]> {
  const res = await fetch(ANTHROPIC_PLUGINS_MANIFEST, { cache: 'no-cache' });
  if (!res.ok) throw new Error(`plugins manifest HTTP ${res.status}`);
  const body = (await res.json()) as {
    plugins?: Array<{
      name: string;
      description?: string;
      author?: { name?: string };
      category?: string;
      homepage?: string;
    }>;
  };
  const arr = body.plugins ?? [];
  return arr.map((p) => ({
    id: `anthropic-plugin:${p.name}`,
    kind: 'plugin' as const,
    name: p.name,
    description: p.description ?? '',
    tags: p.category ? [p.category] : [],
    author: p.author?.name ?? 'Unknown',
    source: { type: 'anthropic-plugin' as const, name: p.name, homepage: p.homepage },
    origin: 'Anthropic & Partners',
    category: p.category
  }));
}

async function fetchAnthropicSkills(): Promise<CatalogEntry[]> {
  const res = await fetch(ANTHROPIC_SKILLS_LISTING, { cache: 'no-cache' });
  if (!res.ok) throw new Error(`skills listing HTTP ${res.status}`);
  const items = (await res.json()) as Array<{ name: string; type: string }>;
  const dirs = items.filter((x) => x.type === 'dir').map((x) => x.name);
  /* Parallel-fetch SKILL.md frontmatter so per-skill metadata is
     available without 17 round-trips serialised. Failures fall through
     to a name-only entry — better than nothing for the user to install. */
  const detailed = await Promise.all(
    dirs.map(async (slug) => {
      try {
        const r = await fetch(`${ANTHROPIC_SKILL_RAW}/${slug}/SKILL.md`, {
          cache: 'no-cache'
        });
        if (!r.ok) return { slug, name: slug, description: '' };
        const md = await r.text();
        const fm = parseFrontmatter(md);
        return {
          slug,
          name: fm.name ?? slug,
          description: fm.description ?? ''
        };
      } catch {
        return { slug, name: slug, description: '' };
      }
    })
  );
  return detailed.map((d) => ({
    id: `anthropic-skill:${d.slug}`,
    kind: 'skill' as const,
    name: d.name,
    description: d.description,
    tags: [],
    author: 'Anthropic',
    source: { type: 'anthropic-skill' as const, slug: d.slug },
    origin: 'Anthropic & Partners'
  }));
}

async function fetchBundled(): Promise<CatalogEntry[]> {
  try {
    const res = await fetch('/library.json', { cache: 'no-cache' });
    if (!res.ok) return [];
    const body = (await res.json()) as BundledCatalog;
    return (body.entries ?? []).map((e) => ({
      ...e,
      origin: e.author || 'Woom defaults'
    }));
  } catch {
    return [];
  }
}

export async function loadCatalog(): Promise<void> {
  if (libraryState.loadingCatalog) return;
  libraryState.loadingCatalog = true;
  libraryState.warnings = [];
  try {
    const [pluginsRes, skillsRes, bundledRes] = await Promise.allSettled([
      fetchAnthropicPlugins(),
      fetchAnthropicSkills(),
      fetchBundled()
    ]);
    const merged: CatalogEntry[] = [];
    if (pluginsRes.status === 'fulfilled') merged.push(...pluginsRes.value);
    else libraryState.warnings.push(`Plugins: ${pluginsRes.reason}`);
    if (skillsRes.status === 'fulfilled') merged.push(...skillsRes.value);
    else libraryState.warnings.push(`Skills: ${skillsRes.reason}`);
    if (bundledRes.status === 'fulfilled') merged.push(...bundledRes.value);
    /* Skills first, then plugins, alphabetical within each — matches
       how Claude.ai's Directory presents them. */
    merged.sort((a, b) => {
      if (a.kind !== b.kind) return a.kind === 'skill' ? -1 : 1;
      return a.name.localeCompare(b.name);
    });
    libraryState.entries = merged;
    libraryState.catalogError = merged.length === 0 ? 'No catalog entries' : null;
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

/** Local-disk slug an entry maps to, or null if the entry isn't a skill
 *  (plugins live under a different namespace and use a different
 *  installed-check below). */
export function slugForEntry(e: CatalogEntry): string | null {
  if (e.kind !== 'skill') return null;
  if (e.source.type === 'inline' || e.source.type === 'git') return e.source.slug;
  if (e.source.type === 'anthropic-skill') return e.source.slug;
  return null;
}

export function isInstalled(e: CatalogEntry): boolean {
  if (e.kind === 'skill') {
    const slug = slugForEntry(e);
    if (!slug) return false;
    return libraryState.installed.skills.some((s) => s.slug === slug);
  }
  if (e.source.type === 'plugin') {
    const ref = e.source.reference;
    return libraryState.installed.plugins.some((p) => p.name === ref);
  }
  if (e.source.type === 'anthropic-plugin') {
    const name = e.source.name;
    return libraryState.installed.plugins.some((p) => p.name === name);
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

export async function uninstallPlugin(name: string): Promise<void> {
  const key = `plugin:${name}`;
  if (libraryState.busy.has(key)) return;
  libraryState.busy = new Set([...libraryState.busy, key]);
  try {
    await invoke('library_plugin_uninstall', { name });
    await refreshInstalled();
  } finally {
    const next = new Set(libraryState.busy);
    next.delete(key);
    libraryState.busy = next;
  }
}

/** Unique plugin categories present in the live catalog. Used by the
 *  Browse toolbar to render the category filter chips. Sorted by
 *  popularity (count desc) so the most-relevant filters surface
 *  first. */
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

export async function ensureLibraryLoaded(): Promise<void> {
  if (!libraryState.loaded) {
    await Promise.all([loadCatalog(), refreshInstalled()]);
    libraryState.loaded = true;
  } else {
    await refreshInstalled();
  }
}
