<script lang="ts">
  /* Settings solo — redesign v2 §2.7 (screen 4q): a 264px nav column
   *  (section jump-links + scroll-spy active state) beside a max-720
   *  content column that scrolls the stacked section cards. Section
   *  content still lives in the co-located `Settings*.svelte`
   *  components; shared chrome CSS is in `./settings/chrome.css`. */
  import './settings/chrome.css';
  import SettingsStorage from './settings/SettingsStorage.svelte';
  import SettingsAppearance from './settings/SettingsAppearance.svelte';
  import SettingsEditor from './settings/SettingsEditor.svelte';
  import SettingsMemory from './settings/SettingsMemory.svelte';
  import SettingsUpdates from './settings/SettingsUpdates.svelte';
  import SettingsPrivacy from './settings/SettingsPrivacy.svelte';
  import SettingsLogs from './settings/SettingsLogs.svelte';
  import SettingsAgents from './settings/SettingsAgents.svelte';
  import SettingsAbout from './settings/SettingsAbout.svelte';

  const sections = [
    { id: 'set-storage', label: 'Storage' },
    { id: 'set-appearance', label: 'Appearance' },
    { id: 'set-editor', label: 'Editor' },
    { id: 'set-memory', label: 'Memory' },
    { id: 'set-updates', label: 'Updates', dot: true },
    { id: 'set-privacy', label: 'Privacy' },
    { id: 'set-logs', label: 'Logs' },
    { id: 'set-agents', label: 'Agents' },
    { id: 'set-about', label: 'About' }
  ];

  let activeId = $state('set-storage');
  let bodyEl = $state<HTMLElement | null>(null);

  /* Scroll-spy — the section nearest the top of the scroll container
     owns the active highlight. rootMargin pulls the trigger line up so
     a section activates as its heading reaches the upper third. */
  $effect(() => {
    if (!bodyEl) return;
    const obs = new IntersectionObserver(
      (entries) => {
        for (const e of entries) {
          if (e.isIntersecting) activeId = (e.target as HTMLElement).id;
        }
      },
      { root: bodyEl, rootMargin: '0px 0px -68% 0px', threshold: 0 }
    );
    for (const s of sections) {
      const el = document.getElementById(s.id);
      if (el) obs.observe(el);
    }
    return () => obs.disconnect();
  });

  function jump(id: string) {
    document.getElementById(id)?.scrollIntoView({ behavior: 'smooth', block: 'start' });
    activeId = id;
  }
</script>

<section class="settings-view">
  <nav class="settings-nav">
    <h1 class="settings-nav-title">Settings</h1>
    {#each sections as s (s.id)}
      <button class="settings-nav-item" class:active={activeId === s.id} onclick={() => jump(s.id)}>
        <span>{s.label}</span>
        {#if s.dot}<span class="settings-nav-dot" aria-hidden="true"></span>{/if}
      </button>
    {/each}
  </nav>

  <div class="settings-body" bind:this={bodyEl}>
    <div class="set-section" id="set-storage"><SettingsStorage /></div>
    <div class="set-section" id="set-appearance"><SettingsAppearance /></div>
    <div class="set-section" id="set-editor"><SettingsEditor /></div>
    <div class="set-section" id="set-memory"><SettingsMemory /></div>
    <div class="set-section" id="set-updates"><SettingsUpdates /></div>
    <div class="set-section" id="set-privacy"><SettingsPrivacy /></div>
    <div class="set-section" id="set-logs"><SettingsLogs /></div>
    <div class="set-section" id="set-agents"><SettingsAgents /></div>
    <div class="set-section" id="set-about"><SettingsAbout /></div>
  </div>
</section>
