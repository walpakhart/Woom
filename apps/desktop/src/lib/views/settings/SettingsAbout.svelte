<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { getVersion } from '@tauri-apps/api/app';
  import { marked } from 'marked';
  import { SESSIONS_STORAGE_KEY, RULES_STORAGE_KEY } from '$lib/state/sessions.svelte';
  import { resetWelcome, welcomeState } from '$lib/state/welcome.svelte';

  // ---- Docs viewer -----------------------------------------------------

  let docsList = $state<string[] | null>(null);
  let docsListError = $state<string | null>(null);
  let activeDoc = $state<string | null>(null);
  let activeDocBody = $state<string | null>(null);
  let activeDocLoading = $state(false);
  let activeDocError = $state<string | null>(null);

  async function loadDocsList() {
    if (docsList !== null) return;
    try {
      docsList = await invoke<string[]>('list_bundled_docs');
    } catch (e) {
      docsListError = typeof e === 'string' ? e : (e as Error).message ?? 'unknown error';
    }
  }

  async function openDoc(name: string) {
    activeDoc = name;
    activeDocBody = null;
    activeDocError = null;
    activeDocLoading = true;
    try {
      const md = await invoke<string>('read_bundled_doc', { name });
      activeDocBody = await Promise.resolve(marked.parse(md) as string | Promise<string>);
    } catch (e) {
      activeDocError = typeof e === 'string' ? e : (e as Error).message ?? 'unknown error';
    } finally {
      activeDocLoading = false;
    }
  }

  function closeDoc() {
    activeDoc = null;
    activeDocBody = null;
    activeDocError = null;
  }

  $effect(() => {
    void loadDocsList();
  });

  // ---- App version -----------------------------------------------------

  let appVersionLabel = $state('Woom');
  onMount(async () => {
    try {
      appVersionLabel = `Woom ${await getVersion()}`;
    } catch {
      /* leave fallback */
    }
  });
</script>

<!-- Documentation -->
<div class="card">
  <header class="card-head">
    <h2 class="card-title">Documentation</h2>
    <p class="card-sub">
      The full Woom spec set, bundled with the app. Each entry is rendered from <span class="mono">docs/*.md</span> in the repo. Pick one to read inline, or open the file in Finder.
    </p>
  </header>
  {#if docsListError}
    <div class="alert alert--error">{docsListError}</div>
  {:else if docsList === null}
    <div class="docs-loading">Loading…</div>
  {:else if docsList.length === 0}
    <div class="docs-empty">No bundled docs found.</div>
  {:else if activeDoc === null}
    <ul class="docs-list">
      {#each docsList as name (name)}
        <li>
          <button class="docs-link" onclick={() => void openDoc(name)}>
            <span class="mono">{name}.md</span>
          </button>
        </li>
      {/each}
    </ul>
  {:else}
    <div class="docs-active">
      <div class="docs-active-bar">
        <button class="btn btn--ghost" onclick={closeDoc}>← Back</button>
        <span class="docs-active-name mono">{activeDoc}.md</span>
      </div>
      {#if activeDocLoading}
        <div class="docs-loading">Loading…</div>
      {:else if activeDocError}
        <div class="alert alert--error">{activeDocError}</div>
      {:else if activeDocBody}
        <article class="docs-md">
          {@html activeDocBody}
        </article>
      {/if}
    </div>
  {/if}
</div>

<!-- Build / app info -->
<div class="card">
  <header class="card-head">
    <h2 class="card-title">App</h2>
  </header>
  <div class="grid">
    <div class="stat">
      <div class="stat-label">Build</div>
      <div class="stat-value mono">{appVersionLabel} · macOS</div>
    </div>
    <div class="stat">
      <div class="stat-label">Storage keys</div>
      <div class="stat-value mono">{SESSIONS_STORAGE_KEY}, {RULES_STORAGE_KEY}</div>
    </div>
  </div>
  <div class="update-actions">
    <button
      class="btn btn--ghost"
      onclick={() => resetWelcome()}
      disabled={!welcomeState.completed}
      title="Re-open the first-launch welcome flow"
    >
      {welcomeState.completed ? 'Show welcome flow again' : 'Welcome flow active'}
    </button>
  </div>
</div>
