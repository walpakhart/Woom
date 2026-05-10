<script lang="ts">
  import { sessionsState } from '$lib/state/sessions.svelte';
</script>

<section class="rules-view">
  <div class="rules-header">
    <h1 class="view-title">Rules</h1>
    <p class="view-sub">
      Preferences and instructions Claude follows on every turn. Appended to the system prompt
      via <code>--append-system-prompt</code>. Saves automatically.
    </p>
  </div>
  <div class="rules-body">
    <textarea
      class="rules-textarea mono"
      bind:value={sessionsState.userRules}
      placeholder={`e.g.\n- Always respond in English, concise.\n- Prefer TypeScript over JavaScript.\n- Don't add comments unless I ask.\n- When proposing commits, use conventional-commits style.`}
      spellcheck="false"
    ></textarea>
    <div class="rules-meta mono">
      {sessionsState.userRules.length} chars · {sessionsState.userRules.trim().split(/\n+/).filter(Boolean).length} non-empty lines
    </div>
  </div>
</section>

<style>
  .rules-view {
    overflow-y: auto; flex: 1;
    display: flex; flex-direction: column;
    padding: 30px 60px 60px;
    background: var(--bg-0);
  }
  .rules-header { padding: 8px 0 28px; max-width: 880px; margin: 0 auto; width: 100%; }
  .view-title {
    font-family: 'Geist', 'Inter', -apple-system, system-ui, sans-serif;
    font-size: 38px; font-weight: 600;
    
    letter-spacing: -0.02em;
    color: var(--text-0);
    margin: 0 0 6px;
  }
  .view-sub { font-size: 14px; color: var(--text-2); margin: 0; line-height: 1.5; }
  .rules-header .view-sub code {
    background: var(--bg-2); padding: 1px 6px; border-radius: 4px;
    font-family: 'JetBrains Mono', ui-monospace, 'SF Mono', monospace;
    font-size: 12px; color: var(--text-1);
  }
  .rules-body {
    padding: 0; max-width: 880px; margin: 0 auto; width: 100%;
    flex: 1; display: flex; flex-direction: column; gap: 10px; min-height: 0;
  }
  .rules-textarea {
    flex: 1; min-height: 360px;
    background: var(--bg-1); border: 1px solid var(--border);
    border-radius: 14px; color: var(--text-0);
    padding: 18px 20px; font-size: 13px; line-height: 1.6;
    font-family: 'JetBrains Mono', ui-monospace, 'SF Mono', monospace;
    resize: none;
    box-shadow: var(--shadow-1);
    transition: border-color 150ms, box-shadow 150ms;
  }
  .rules-textarea:focus {
    border-color: var(--border-accent);
    box-shadow: var(--shadow-1), 0 0 0 3px var(--accent-soft);
  }
  .rules-textarea:focus { outline: none; }
  .rules-textarea::placeholder { color: var(--text-mute); white-space: pre-line; }
  .rules-meta {
    font-size: 11px; color: var(--text-mute);
    text-align: right; letter-spacing: 0.02em;
  }
</style>
