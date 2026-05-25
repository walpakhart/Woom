<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { notify, notifyError } from '$lib/state/toaster.svelte';
  import {
    hooksState,
    loadHookConfig,
    saveHookConfig,
    enabledHookCount,
    type HookConfig
  } from '$lib/state/hooks.svelte';

  // ---- Hooks editor ----------------------------------------------------

  let hooksDraft = $state('');
  let hooksDraftPristine = $state('');
  let hooksParseError = $state<string | null>(null);
  let hooksLoading = $state(false);
  const hookCount = $derived(enabledHookCount());
  const hooksPlaceholder = `{
  "hooks": {
    "UserPromptSubmit": [
      {
        "matcher": "*",
        "handler": { "type": "command", "command": "/path/to/script.sh" },
        "timeout_ms": 5000,
        "disabled": false
      }
    ],
    "Stop": [],
    "SessionStart": []
  }
}`;

  $effect(() => {
    if (hooksDraft.trim().length === 0) {
      hooksParseError = null;
      return;
    }
    try {
      const parsed = JSON.parse(hooksDraft);
      if (parsed && typeof parsed === 'object' && parsed.hooks && typeof parsed.hooks !== 'object') {
        hooksParseError = '`hooks` must be an object keyed by event name';
        return;
      }
      hooksParseError = null;
    } catch (e) {
      hooksParseError = `JSON parse error: ${(e as Error).message}`;
    }
  });

  async function resetHooksDraft(): Promise<void> {
    hooksDraft = hooksDraftPristine;
  }

  async function saveHooksDraft(): Promise<void> {
    if (hooksParseError) return;
    hooksLoading = true;
    try {
      const parsed = hooksDraft.trim() === ''
        ? ({ hooks: {} } as HookConfig)
        : (JSON.parse(hooksDraft) as HookConfig);
      await saveHookConfig(parsed);
      hooksDraftPristine = hooksDraft;
      notify({ kind: 'success', title: 'Hooks saved', ttlMs: 2200 });
    } catch (e) {
      notifyError(e, { title: 'Hooks save failed' });
    } finally {
      hooksLoading = false;
    }
  }

  $effect(() => {
    void (async () => {
      hooksLoading = true;
      try {
        const cfg = hooksState.loaded ? hooksState.config : await loadHookConfig();
        hooksDraftPristine = JSON.stringify(cfg, null, 2);
        if (hooksDraft.length === 0) hooksDraft = hooksDraftPristine;
      } finally {
        hooksLoading = false;
      }
    })();
  });

  // ---- MCP sidecar health ----------------------------------------------

  type SidecarHealth = { name: string; running: boolean; pid_count: number };
  let sidecarHealth = $state<SidecarHealth[] | null>(null);
  let sidecarHealthLoading = $state(false);

  async function refreshSidecarHealth() {
    sidecarHealthLoading = true;
    try {
      sidecarHealth = await invoke<SidecarHealth[]>('mcp_sidecar_health');
    } catch (e) {
      notifyError(e, { title: 'Probe failed' });
    } finally {
      sidecarHealthLoading = false;
    }
  }

  $effect(() => {
    if (sidecarHealth === null) void refreshSidecarHealth();
  });
</script>

<!-- Hooks -->
<div class="card">
  <header class="card-head">
    <h2 class="card-title">Hooks</h2>
    <p class="card-sub">
      Wire shell scripts into agent lifecycle events. Currently supported:
      <span class="mono">UserPromptSubmit</span>, <span class="mono">Stop</span>,
      <span class="mono">SessionStart</span>. Each handler reads JSON on stdin and may
      rewrite the prompt, attach context, or block the action via exit code 2.
    </p>
  </header>
  <div class="hooks-actions">
    <span class="card-sub">{hookCount} hook{hookCount === 1 ? '' : 's'} enabled</span>
    <button
      class="btn btn--ghost"
      onclick={() => void resetHooksDraft()}
      disabled={hooksLoading || hooksDraft === hooksDraftPristine}
      title="Revert unsaved edits"
    >Revert</button>
    <button
      class="btn"
      onclick={() => void saveHooksDraft()}
      disabled={hooksLoading || hooksDraft === hooksDraftPristine || !!hooksParseError}
    >{hooksLoading ? 'Saving…' : 'Save'}</button>
  </div>
  <textarea
    class="hooks-editor mono"
    bind:value={hooksDraft}
    placeholder={hooksPlaceholder}
    spellcheck="false"
    rows="12"
  ></textarea>
  {#if hooksParseError}
    <div class="hooks-error mono">{hooksParseError}</div>
  {/if}
  <p class="card-sub hooks-hint">
    Schema:
    <span class="mono">{`{ "hooks": { "<EventName>": [ { "matcher": "*", "handler": { "type": "command", "command": "/path/to/script" }, "timeout_ms": 5000, "disabled": false } ] } }`}</span>.
    Script stdin = event JSON. Stdout JSON keys honored:
    <span class="mono">updated_prompt</span> (UserPromptSubmit rewrite, deferred wiring),
    <span class="mono">additional_context</span>, <span class="mono">reason</span>.
    Exit 2 = block.
  </p>
</div>

<!-- MCP servers -->
<div class="card">
  <header class="card-head">
    <h2 class="card-title">MCP servers</h2>
    <p class="card-sub">
      Woom's bundled sidecars. Spawned by Claude / Cursor on first MCP handshake; "not running" means no agent has talked to that sidecar yet this launch.
    </p>
  </header>
  <div class="update-actions">
    <button
      class="btn btn--ghost"
      onclick={() => void refreshSidecarHealth()}
      disabled={sidecarHealthLoading}
    >
      {sidecarHealthLoading ? 'Checking…' : 'Refresh'}
    </button>
  </div>
  {#if sidecarHealth}
    <ul class="sidecar-list">
      {#each sidecarHealth as s (s.name)}
        <li class="sidecar-row" class:running={s.running}>
          <span class="sidecar-dot" aria-hidden="true"></span>
          <span class="sidecar-name mono">{s.name}</span>
          <span class="sidecar-status">
            {s.running ? `running · ${s.pid_count} pid${s.pid_count === 1 ? '' : 's'}` : 'not running'}
          </span>
        </li>
      {/each}
    </ul>
  {/if}
</div>
