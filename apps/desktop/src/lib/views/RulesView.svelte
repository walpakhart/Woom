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
  .rules-view { overflow-y: auto; flex: 1; display: flex; flex-direction: column; }
  .rules-header { padding: 48px 56px 20px; text-align: center; }
  .view-title { font-size: 28px; font-weight: 600; letter-spacing: -0.025em; color: var(--text-0); margin-bottom: 10px; }
  .view-sub { font-size: 14px; color: var(--text-2); max-width: 520px; margin: 0 auto; line-height: 1.5; }
  .rules-header .view-sub code {
    background: var(--bg-2); padding: 1px 6px; border-radius: 4px;
    font-family: 'JetBrains Mono', ui-monospace, 'SF Mono', monospace;
    font-size: 12px; color: var(--text-1);
  }
  .rules-body {
    padding: 0 56px 48px; max-width: 980px; margin: 0 auto; width: 100%;
    flex: 1; display: flex; flex-direction: column; gap: 10px; min-height: 0;
  }
  .rules-textarea {
    flex: 1; min-height: 360px;
    background: var(--bg-1); border: 1px solid var(--border-neutral);
    border-radius: 10px; color: var(--text-0);
    padding: 18px 20px; font-size: 13px; line-height: 1.6;
    font-family: 'JetBrains Mono', ui-monospace, 'SF Mono', monospace;
    resize: none;
    transition: border-color 150ms;
  }
  .rules-textarea:focus { outline: none; border-color: var(--border-hi2); }
  .rules-textarea::placeholder { color: var(--text-mute); white-space: pre-line; }
  .rules-meta {
    font-size: 11px; color: var(--text-mute);
    text-align: right; letter-spacing: 0.02em;
  }
</style>
