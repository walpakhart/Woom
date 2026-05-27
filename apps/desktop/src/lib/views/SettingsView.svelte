<script lang="ts">
  /* Settings solo — vertical scroll of section cards. The view itself
   *  is a thin shell: the section content moved to
   *  `lib/views/settings/Settings*.svelte` components co-located in a
   *  sibling folder. Each section owns its own state, helpers, and
   *  $effects. The shared chrome CSS lives in `./settings/chrome.css`
   *  as plain CSS (global by nature) so it reaches the child
   *  components without crossing Svelte's per-component CSS scoping. */
  import './settings/chrome.css';
  import SettingsStorage from './settings/SettingsStorage.svelte';
  import SettingsAppearance from './settings/SettingsAppearance.svelte';
  import SettingsEditor from './settings/SettingsEditor.svelte';
  import SettingsMemory from './settings/SettingsMemory.svelte';
  import SettingsUpdates from './settings/SettingsUpdates.svelte';
  import SettingsSdd from './settings/SettingsSdd.svelte';
  import SettingsPrivacy from './settings/SettingsPrivacy.svelte';
  import SettingsAgents from './settings/SettingsAgents.svelte';
  import SettingsAbout from './settings/SettingsAbout.svelte';
</script>

<section class="settings-view">
  <div class="settings-header">
    <h1 class="view-title">Settings</h1>
    <p class="view-sub">Storage usage, cleanup, and app diagnostics.</p>
  </div>

  <div class="settings-body">
    <SettingsStorage />
    <SettingsAppearance />
    <SettingsEditor />
    <SettingsMemory />
    <SettingsUpdates />
    <SettingsSdd />
    <SettingsPrivacy />
    <SettingsAgents />
    <SettingsAbout />
  </div>
</section>
