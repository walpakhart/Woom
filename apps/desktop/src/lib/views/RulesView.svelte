<script lang="ts">
  import { sessionsState } from '$lib/state/sessions.svelte';

  const charCount = $derived(sessionsState.userRules.length);
  const lineCount = $derived(
    sessionsState.userRules.trim().split(/\n+/).filter(Boolean).length
  );
</script>

<section class="rules-view">
  <div class="rules-doc">
    <header class="rules-header">
      <h1 class="rules-title">Rules</h1>
      <span class="rules-count mono">{charCount} chars · {lineCount} lines</span>
    </header>
    <p class="rules-sub">
      Injected into every agent turn via <code>--append-system-prompt</code> — across all chats,
      workflows and docks. Saves automatically.
    </p>
    <textarea
      class="rules-sheet mono"
      bind:value={sessionsState.userRules}
      placeholder={`e.g.\n— Always respond in English, concise.\n— Prefer TypeScript over JavaScript.\n— Don't add comments unless I ask.\n— Conventional-commits style for commit messages.`}
      spellcheck="false"
    ></textarea>
  </div>
</section>

<style>
  /* Redesign v2 §2.7 — centred document (max 720), padding 46/64. */
  .rules-view {
    overflow-y: auto; flex: 1;
    display: flex; flex-direction: column;
    padding: 46px 64px 60px;
    background: var(--bg-0);
  }
  .rules-doc {
    max-width: 720px; margin: 0 auto; width: 100%;
    flex: 1; min-height: 0;
    display: flex; flex-direction: column;
  }
  .rules-header {
    display: flex; align-items: baseline; gap: 12px;
    margin-bottom: 6px;
  }
  .rules-title {
    font-size: 24px; font-weight: 600;
    letter-spacing: -0.015em;
    color: var(--text-0);
    margin: 0;
  }
  .rules-count {
    margin-left: auto;
    font-size: 11.5px; color: var(--text-mute);
    letter-spacing: 0.02em;
  }
  .rules-sub {
    font-size: 13px; color: var(--text-2);
    margin: 0 0 20px; line-height: 1.5;
  }
  .rules-sub code {
    background: var(--bg-2); padding: 1px 6px; border-radius: 4px;
    font-family: var(--font-mono);
    font-size: 12px; color: var(--text-1);
  }
  /* Rules sheet — bg-2 card, r12, engraved shadow-1; rows 13.5/1.9. */
  .rules-sheet {
    flex: 1; min-height: 360px;
    background: var(--bg-2); border: 1px solid var(--border);
    border-radius: 12px; color: var(--text-0);
    padding: 20px 24px; font-size: 13.5px; line-height: 1.9;
    font-family: var(--font-mono);
    resize: none;
    box-shadow: var(--shadow-1);
    transition: border-color 150ms, box-shadow 150ms;
  }
  .rules-sheet:focus {
    outline: none;
    border-color: var(--border-accent);
    box-shadow: var(--shadow-1), 0 0 0 3px var(--accent-soft);
  }
  .rules-sheet::placeholder { color: var(--text-mute); white-space: pre-line; }
</style>
