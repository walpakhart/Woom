<script lang="ts">
  /* LibraryApp — store-style surface for Claude skills (and, in time,
     plugins). Three modes:
       - browse:     curated catalog from /library.json with search +
                     kind filter; "Install" / "Installed" per row.
       - installed:  what's actually living under ~/.claude/skills/ and
                     ~/.claude/plugins/, with × to uninstall.
       - sources:    where the catalog comes from + how to add more
                     (v1 just shows the bundled URL and reload). */

  import {
    libraryState,
    ensureLibraryLoaded,
    loadCatalog,
    refreshInstalled,
    installEntry,
    uninstallSkill,
    uninstallPlugin,
    isInstalled,
    pluginCategories,
    type CatalogEntry,
    type EntryKind
  } from '$lib/state/library.svelte';
  import { notify } from '$lib/state/toaster.svelte';

  type Tab = 'browse' | 'installed' | 'sources';
  let tab = $state<Tab>('browse');

  ensureLibraryLoaded();

  const filtered = $derived.by((): CatalogEntry[] => {
    const q = libraryState.query.trim().toLowerCase();
    const cat = libraryState.categoryFilter;
    return libraryState.entries.filter((e) => {
      if (libraryState.kindFilter && e.kind !== libraryState.kindFilter) return false;
      if (cat && e.kind === 'plugin' && (e.category ?? 'uncategorized') !== cat) return false;
      if (!q) return true;
      const hay = [e.name, e.description, e.author, e.tags.join(' ')]
        .join(' ')
        .toLowerCase();
      return hay.includes(q);
    });
  });

  /** Surface plugin categories only when the user has narrowed to
   *  plugins (or shown All) — chips for "All" + 17+ categories
   *  cluttering Skills view would just be noise. */
  const showCategories = $derived(
    libraryState.kindFilter === null || libraryState.kindFilter === 'plugin'
  );
  const categories = $derived(pluginCategories());

  async function onInstall(e: CatalogEntry) {
    try {
      await installEntry(e);
      notify({
        kind: 'success',
        title: `Installed ${e.name}`,
        body: 'Start a new chat for Claude to pick it up.'
      });
    } catch (err) {
      notify({ kind: 'error', title: 'Install failed', body: String(err) });
    }
  }

  async function onUninstallSkill(slug: string, name: string) {
    if (!confirm(`Uninstall skill “${name}”? Files at ~/.claude/skills/${slug} will be deleted.`)) return;
    try {
      await uninstallSkill(slug);
      notify({ kind: 'success', title: `Uninstalled ${name}` });
    } catch (err) {
      notify({ kind: 'error', title: 'Uninstall failed', body: String(err) });
    }
  }

  async function onUninstallPlugin(name: string) {
    if (!confirm(`Uninstall plugin “${name}”?`)) return;
    try {
      await uninstallPlugin(name);
      notify({ kind: 'success', title: `Uninstalled ${name}` });
    } catch (err) {
      notify({ kind: 'error', title: 'Uninstall failed', body: String(err) });
    }
  }

  function setKind(k: EntryKind | null) {
    libraryState.kindFilter = k;
    if (k === 'skill') libraryState.categoryFilter = null;
  }
  function setCategory(c: string | null) {
    libraryState.categoryFilter = c;
  }
</script>

<section class="lib">
  <div class="lib-head">
    <h1 class="view-title">Library</h1>
    <p class="view-sub">
      Skills and plugins for Claude, pulled live from
      <code>anthropics/skills</code> and the official
      <code>claude-plugins-official</code> marketplace. Both come live on the
      next chat.
    </p>
  </div>

  <div class="lib-tabs">
    <button class:active={tab === 'browse'} onclick={() => (tab = 'browse')}>Browse</button>
    <button class:active={tab === 'installed'} onclick={() => (tab = 'installed')}>
      Installed
      <span class="lib-tab-count mono">
        {libraryState.installed.skills.length + libraryState.installed.plugins.length}
      </span>
    </button>
    <button class:active={tab === 'sources'} onclick={() => (tab = 'sources')}>Sources</button>
    <div class="lib-tabs-spacer"></div>
    {#if tab === 'installed'}
      <button class="lib-refresh" onclick={() => refreshInstalled()} title="Rescan ~/.claude">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M3 12a9 9 0 0 1 15-6.7L21 8"/><path d="M21 3v5h-5"/><path d="M21 12a9 9 0 0 1-15 6.7L3 16"/><path d="M3 21v-5h5"/></svg>
        Refresh
      </button>
    {:else if tab === 'browse'}
      <button class="lib-refresh" disabled={libraryState.loadingCatalog} onclick={() => loadCatalog()} title="Re-fetch the Anthropic Directory">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M3 12a9 9 0 0 1 15-6.7L21 8"/><path d="M21 3v5h-5"/><path d="M21 12a9 9 0 0 1-15 6.7L3 16"/><path d="M3 21v-5h5"/></svg>
        {libraryState.loadingCatalog ? 'Loading…' : 'Refresh'}
      </button>
    {/if}
  </div>

  {#if tab === 'browse'}
    <div class="lib-toolbar">
      <input
        class="lib-search"
        type="search"
        placeholder="Search skills and plugins…"
        bind:value={libraryState.query}
        spellcheck="false"
      />
      <div class="lib-filters">
        <button class:active={libraryState.kindFilter === null} onclick={() => setKind(null)}>All</button>
        <button class:active={libraryState.kindFilter === 'skill'} onclick={() => setKind('skill')}>Skills</button>
        <button class:active={libraryState.kindFilter === 'plugin'} onclick={() => setKind('plugin')}>Plugins</button>
      </div>
    </div>

    {#if showCategories && categories.length > 0}
      <div class="lib-cats">
        <button class:active={libraryState.categoryFilter === null} onclick={() => setCategory(null)}>All categories</button>
        {#each categories as c (c.name)}
          <button class:active={libraryState.categoryFilter === c.name} onclick={() => setCategory(c.name)}>
            {c.name} <span class="mono lib-cat-count">{c.count}</span>
          </button>
        {/each}
      </div>
    {/if}

    {#if libraryState.catalogError}
      <div class="lib-error">Catalog failed to load: {libraryState.catalogError}</div>
    {/if}
    {#each libraryState.warnings as w}
      <div class="lib-warn">{w}</div>
    {/each}

    <div class="lib-grid">
      {#each filtered as e (e.id)}
        {@const installed = isInstalled(e)}
        {@const busy = libraryState.busy.has(e.id)}
        <article class="lib-card" class:installed>
          <header class="lib-card-head">
            <span class="lib-kind lib-kind--{e.kind}">{e.kind}</span>
            <h2 class="lib-card-name">{e.name}</h2>
          </header>
          <p class="lib-card-desc">{e.description}</p>
          {#if e.note}<p class="lib-card-note">{e.note}</p>{/if}
          <div class="lib-card-meta mono">
            <span class="lib-card-author">{e.author}</span>
            <span class="lib-card-origin">· {e.origin}</span>
            {#if e.tags.length}
              <span class="lib-card-tags">
                {#each e.tags as t (t)}<span class="lib-tag">{t}</span>{/each}
              </span>
            {/if}
          </div>
          <div class="lib-card-actions">
            {#if installed}
              <span class="lib-installed-pill">
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><polyline points="20 6 9 17 4 12"/></svg>
                Installed
              </span>
            {:else}
              <button class="lib-install" disabled={busy} onclick={() => onInstall(e)}>
                {busy ? 'Installing…' : 'Install'}
              </button>
            {/if}
          </div>
        </article>
      {/each}
      {#if filtered.length === 0 && !libraryState.catalogError}
        <div class="lib-empty">
          No entries match — try a different search or filter.
        </div>
      {/if}
    </div>

  {:else if tab === 'installed'}
    {#if libraryState.installedError}
      <div class="lib-error">Failed to read ~/.claude: {libraryState.installedError}</div>
    {/if}
    <h3 class="lib-section">
      Skills
      <span class="lib-section-count mono">{libraryState.installed.skills.length}</span>
    </h3>
    {#if libraryState.installed.skills.length === 0}
      <p class="lib-empty">No skills installed yet — pick one from Browse.</p>
    {:else}
      <ul class="lib-list">
        {#each libraryState.installed.skills as s (s.slug)}
          {@const busy = libraryState.busy.has(`skill:${s.slug}`)}
          <li class="lib-row">
            <div class="lib-row-main">
              <div class="lib-row-name">{s.name}</div>
              <div class="lib-row-desc">{s.description || '—'}</div>
              <div class="lib-row-path mono">{s.path}</div>
            </div>
            <button
              class="lib-row-x"
              disabled={busy}
              onclick={() => onUninstallSkill(s.slug, s.name)}
              title="Uninstall"
              aria-label="Uninstall {s.name}"
            >
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"><path d="M6 6l12 12M6 18 18 6"/></svg>
            </button>
          </li>
        {/each}
      </ul>
    {/if}

    <h3 class="lib-section">
      Plugins
      <span class="lib-section-count mono">{libraryState.installed.plugins.length}</span>
    </h3>
    {#if libraryState.installed.plugins.length === 0}
      <p class="lib-empty">No plugins installed yet.</p>
    {:else}
      <ul class="lib-list">
        {#each libraryState.installed.plugins as pl (pl.name)}
          {@const busy = libraryState.busy.has(`plugin:${pl.name}`)}
          <li class="lib-row">
            <div class="lib-row-main">
              <div class="lib-row-name">{pl.name}</div>
              <div class="lib-row-path mono">{pl.path}</div>
            </div>
            <button
              class="lib-row-x"
              disabled={busy}
              onclick={() => onUninstallPlugin(pl.name)}
              title="Uninstall"
              aria-label="Uninstall {pl.name}"
            >
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"><path d="M6 6l12 12M6 18 18 6"/></svg>
            </button>
          </li>
        {/each}
      </ul>
    {/if}

  {:else}
    <div class="lib-sources">
      <h3 class="lib-section">Catalog</h3>
      <p class="view-sub">
        The catalog is bundled with Woom at <code>static/library.json</code>.
        It lists skills you can install in one click. Anthropic doesn't run a
        centralized store — public skills/plugins live in git repos.
        Federation (point Woom at any catalog URL) is on the roadmap.
      </p>
      <button class="lib-install" onclick={() => ensureLibraryLoaded()}>Reload catalog</button>
    </div>
  {/if}
</section>

<style>
  .lib {
    overflow-y: auto; flex: 1;
    display: flex; flex-direction: column;
    padding: 30px 60px 60px;
    background: var(--bg-0);
  }
  .lib-head { max-width: 980px; margin: 0 auto 22px; width: 100%; }
  .view-title {
    font-family: 'Geist', 'Inter', -apple-system, system-ui, sans-serif;
    font-size: 38px; font-weight: 600;
    letter-spacing: -0.02em;
    color: var(--text-0);
    margin: 0 0 6px;
  }
  .view-sub { font-size: 14px; color: var(--text-2); margin: 0; line-height: 1.5; }
  .view-sub code {
    background: var(--bg-2); padding: 1px 6px; border-radius: 4px;
    font-family: 'JetBrains Mono', ui-monospace, 'SF Mono', monospace;
    font-size: 12px; color: var(--text-1);
  }

  .lib-tabs {
    max-width: 980px; margin: 0 auto 14px; width: 100%;
    display: flex; align-items: center; gap: 4px;
    border-bottom: 1px solid var(--border);
  }
  .lib-tabs > button {
    display: inline-flex; align-items: center; gap: 6px;
    padding: 9px 14px;
    background: transparent;
    border: 0;
    border-bottom: 2px solid transparent;
    color: var(--text-2);
    font-size: 13px; font-weight: 500;
    cursor: pointer;
    transition: color 120ms, border-color 120ms;
  }
  .lib-tabs > button:hover { color: var(--text-0); }
  .lib-tabs > button.active {
    color: var(--text-0);
    border-bottom-color: var(--accent-bright);
  }
  .lib-tab-count {
    font-size: 10.5px; color: var(--text-mute);
    padding: 1px 5px; border-radius: 4px;
    background: var(--bg-2);
  }
  .lib-tabs-spacer { flex: 1; }
  .lib-refresh {
    display: inline-flex; align-items: center; gap: 5px;
    padding: 5px 9px;
    background: transparent; border: 1px solid var(--border);
    border-radius: 6px;
    color: var(--text-2);
    font-size: 12px;
    cursor: pointer;
  }
  .lib-refresh svg { width: 12px; height: 12px; }
  .lib-refresh:hover { color: var(--text-0); border-color: var(--border-hi); }

  .lib-toolbar {
    max-width: 980px; margin: 0 auto 16px; width: 100%;
    display: flex; align-items: center; gap: 12px;
  }
  .lib-search {
    flex: 1;
    padding: 9px 12px;
    background: var(--bg-1); border: 1px solid var(--border);
    border-radius: 8px;
    color: var(--text-0);
    font-size: 13px;
  }
  .lib-search:focus {
    outline: none; border-color: var(--border-accent);
    box-shadow: 0 0 0 3px var(--accent-soft);
  }
  .lib-filters { display: inline-flex; gap: 2px; padding: 2px; background: var(--bg-1); border: 1px solid var(--border); border-radius: 8px; }
  .lib-filters button {
    padding: 6px 11px;
    border: 0; background: transparent;
    color: var(--text-2);
    font-size: 12px; font-weight: 500;
    border-radius: 6px;
    cursor: pointer;
  }
  .lib-filters button:hover { color: var(--text-0); }
  .lib-filters button.active {
    color: var(--text-0);
    background: var(--bg-3);
  }

  .lib-grid {
    max-width: 980px; margin: 0 auto; width: 100%;
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(280px, 1fr));
    gap: 14px;
  }
  .lib-card {
    background: var(--bg-1); border: 1px solid var(--border);
    border-radius: 12px;
    padding: 14px 16px;
    display: flex; flex-direction: column; gap: 8px;
    transition: border-color 140ms, transform 140ms;
  }
  .lib-card:hover { border-color: var(--border-hi); }
  .lib-card.installed { border-color: color-mix(in srgb, var(--accent) 30%, var(--border)); }
  .lib-card-head { display: flex; align-items: baseline; gap: 8px; }
  .lib-kind {
    font-size: 9.5px; font-weight: 600;
    text-transform: uppercase; letter-spacing: 0.08em;
    padding: 2px 6px; border-radius: 4px;
    background: var(--bg-3); color: var(--text-mute);
  }
  .lib-kind--skill { background: color-mix(in srgb, var(--src-claude) 16%, var(--bg-3)); color: var(--src-claude); }
  .lib-kind--plugin { background: color-mix(in srgb, var(--src-canvas) 14%, var(--bg-3)); color: var(--src-canvas); }
  .lib-card-name {
    flex: 1; min-width: 0;
    font-size: 15px; font-weight: 600;
    color: var(--text-0);
    margin: 0;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .lib-card-desc { font-size: 12.5px; color: var(--text-2); line-height: 1.5; margin: 0; }
  .lib-card-note {
    font-size: 11.5px; color: var(--text-mute);
    background: var(--bg-2); border-left: 2px solid var(--border-hi);
    padding: 6px 9px; border-radius: 4px;
    margin: 0; line-height: 1.45;
  }
  .lib-card-meta {
    display: flex; align-items: center; flex-wrap: wrap; gap: 8px;
    font-size: 10.5px; color: var(--text-mute);
  }
  .lib-card-tags { display: inline-flex; flex-wrap: wrap; gap: 4px; }
  .lib-tag {
    padding: 1px 5px; border-radius: 3px;
    background: var(--bg-2); color: var(--text-mute);
    font-size: 10px;
  }

  .lib-card-actions { display: flex; justify-content: flex-end; margin-top: auto; padding-top: 4px; }
  .lib-install {
    padding: 6px 14px;
    background: var(--accent-soft);
    border: 1px solid var(--border-accent-2);
    color: var(--accent-bright);
    border-radius: 7px;
    font-size: 12px; font-weight: 500;
    cursor: pointer;
    transition: background 120ms, border-color 120ms;
  }
  .lib-install:hover { background: color-mix(in srgb, var(--accent) 14%, transparent); }
  .lib-install:disabled { opacity: 0.6; cursor: progress; }
  .lib-installed-pill {
    display: inline-flex; align-items: center; gap: 5px;
    padding: 5px 10px;
    background: color-mix(in srgb, var(--accent) 10%, transparent);
    border: 1px solid color-mix(in srgb, var(--accent) 28%, transparent);
    color: var(--accent-bright);
    border-radius: 7px;
    font-size: 11.5px; font-weight: 500;
  }
  .lib-installed-pill svg { width: 11px; height: 11px; }

  .lib-empty {
    max-width: 980px; margin: 18px auto 0; width: 100%;
    padding: 24px;
    color: var(--text-mute);
    font-size: 13px;
    background: var(--bg-1);
    border: 1px dashed var(--border);
    border-radius: 10px;
    text-align: center;
  }
  .lib-error {
    max-width: 980px; margin: 0 auto 14px; width: 100%;
    padding: 10px 14px;
    background: color-mix(in srgb, var(--error) 8%, var(--bg-1));
    border: 1px solid color-mix(in srgb, var(--error) 30%, var(--border));
    border-radius: 8px;
    color: var(--text-0);
    font-size: 12.5px;
  }
  .lib-warn {
    max-width: 980px; margin: 0 auto 8px; width: 100%;
    padding: 6px 12px;
    background: var(--bg-1);
    border: 1px solid var(--border);
    border-left: 3px solid var(--warning);
    border-radius: 6px;
    color: var(--text-2);
    font-size: 11.5px;
  }

  /* Category filter chips — render below the Skill/Plugin toggle when
     plugins are in scope. Lots of categories (~17 from Anthropic) so we
     wrap to a second row. */
  .lib-cats {
    max-width: 980px; margin: 0 auto 14px; width: 100%;
    display: flex; flex-wrap: wrap; gap: 6px;
  }
  .lib-cats button {
    display: inline-flex; align-items: center; gap: 5px;
    padding: 4px 9px;
    background: var(--bg-1);
    border: 1px solid var(--border);
    color: var(--text-2);
    border-radius: 6px;
    font-size: 11.5px;
    cursor: pointer;
    text-transform: capitalize;
    transition: color 120ms, border-color 120ms, background 120ms;
  }
  .lib-cats button:hover { color: var(--text-0); border-color: var(--border-hi); }
  .lib-cats button.active {
    color: var(--accent-bright);
    background: var(--accent-soft);
    border-color: var(--border-accent-2);
  }
  .lib-cat-count {
    font-size: 9.5px; color: var(--text-mute);
    padding: 0 4px; border-radius: 3px;
    background: var(--bg-2);
  }
  .lib-cats button.active .lib-cat-count {
    color: var(--accent-bright);
    background: color-mix(in srgb, var(--accent) 12%, transparent);
  }
  .lib-card-origin { color: var(--text-mute); }

  .lib-section {
    max-width: 980px; margin: 18px auto 8px; width: 100%;
    font-size: 12px; font-weight: 600;
    letter-spacing: 0.05em; text-transform: uppercase;
    color: var(--text-mute);
    display: flex; align-items: center; gap: 8px;
  }
  .lib-section-count {
    font-size: 10.5px; color: var(--text-mute);
    padding: 1px 5px; border-radius: 4px;
    background: var(--bg-2);
  }

  .lib-list {
    max-width: 980px; margin: 0 auto; width: 100%;
    list-style: none; padding: 0;
    display: flex; flex-direction: column; gap: 8px;
  }
  .lib-row {
    display: flex; align-items: center; gap: 10px;
    padding: 12px 14px;
    background: var(--bg-1); border: 1px solid var(--border);
    border-radius: 10px;
  }
  .lib-row-main { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 2px; }
  .lib-row-name { color: var(--text-0); font-size: 13.5px; font-weight: 500; }
  .lib-row-desc { color: var(--text-2); font-size: 12px; }
  .lib-row-path {
    color: var(--text-mute); font-size: 10.5px;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .lib-row-x {
    width: 28px; height: 28px;
    display: grid; place-items: center;
    background: transparent; border: 1px solid var(--border);
    border-radius: 7px;
    color: var(--text-mute);
    cursor: pointer;
  }
  .lib-row-x svg { width: 13px; height: 13px; }
  .lib-row-x:hover {
    color: var(--error);
    border-color: color-mix(in srgb, var(--error) 50%, var(--border));
    background: color-mix(in srgb, var(--error) 8%, transparent);
  }
  .lib-row-x:disabled { opacity: 0.5; cursor: progress; }

  .lib-sources {
    max-width: 980px; margin: 0 auto; width: 100%;
    display: flex; flex-direction: column; gap: 12px;
  }
  .lib-sources .lib-section { margin-left: 0; margin-right: 0; }
  .lib-sources .lib-install { align-self: flex-start; }
</style>
