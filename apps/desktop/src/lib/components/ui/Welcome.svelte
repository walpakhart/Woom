<script lang="ts">
  /* First-launch welcome flow (`docs/ROADMAP_1.0.md §1.5`).
   *
   * Three steps: theme → first source → first agent. The agent step
   * is genuinely diagnostic ("is `claude` / `cursor-agent` on PATH?")
   * — the user can keep going either way. Source step pops the
   * regular connect modal, so the actual auth UX stays in one place.
   *
   * Skippable at any time. Completion is persisted via
   * `markWelcomeCompleted` so re-launches go straight to the default
   * solo view. SettingsView exposes a "Show welcome again" button
   * (via `resetWelcome`) for users who want to revisit.
   */

  import Sigil from '$lib/components/ui/Sigil.svelte';
  import { focusTrap } from '$lib/actions/focusTrap';
  import { themeState, applyTheme, type ThemeName } from '$lib/state/theme.svelte';
  import { connectionsState } from '$lib/state/connections.svelte';
  import { markWelcomeCompleted } from '$lib/state/welcome.svelte';
  import type { ConnectionMeta } from '$lib/data';

  interface Props {
    /** Source catalogue (passed in to avoid re-importing the meta
     *  list — keeps the component decoupled from data ordering). */
    sources: ConnectionMeta[];
    onConnect: (id: string) => void;
    onClose: () => void;
  }

  let { sources, onConnect, onClose }: Props = $props();

  const STEP_COUNT = 3;
  let step = $state(0);

  /* Theme picker copies — kept terse so the welcome stays scannable. */
  const themeOptions: { name: ThemeName; label: string; sub: string; bg: string; fg: string; accent: string }[] = [
    { name: 'iconic', label: 'Iconic', sub: 'Sage + mint on cool noir', bg: '#0E1112', fg: '#EBEFEC', accent: '#B0DCC8' },
    { name: 'light',  label: 'Light',  sub: 'Sage + mint on cream',     bg: '#F1F5F2', fg: '#0E1B16', accent: '#2E5A4A' }
  ];

  function pickTheme(name: ThemeName) {
    themeState.name = name;
    applyTheme(name);
  }

  function next() {
    if (step < STEP_COUNT - 1) {
      step += 1;
    } else {
      finish();
    }
  }

  function back() {
    if (step > 0) step -= 1;
  }

  function finish() {
    markWelcomeCompleted();
    onClose();
  }

  function skip() {
    finish();
  }

  /* Filter the source list to only the implemented entries — Slack /
   * Linear / etc. are catalogue-only and would just frustrate a user
   * clicking on them during onboarding. */
  const implementedSources = $derived(sources.filter((s) => s.implemented));
</script>

<!-- The welcome flow always renders behind any modal (z lower than the
     connect modals it triggers), but in front of the solo so the
     user lands here on first run. Backdrop click does NOT dismiss to
     avoid accidental skips — explicit Skip / Done buttons. -->
<div class="welcome-backdrop" role="dialog" aria-modal="true" aria-labelledby="welcome-title" use:focusTrap>
  <section class="welcome-panel" aria-label="Welcome to Woom">
    <header class="welcome-head">
      <Sigil size={48} />
      <div class="welcome-head-text">
        <h2 id="welcome-title" class="welcome-title">Welcome to Woom</h2>
        <p class="welcome-sub">Step {step + 1} of {STEP_COUNT}</p>
      </div>
      <button class="welcome-skip" onclick={skip} aria-label="Skip onboarding">Skip</button>
    </header>

    <div class="welcome-progress" aria-hidden="true">
      <div class="welcome-progress-bar" style="width: {((step + 1) / STEP_COUNT) * 100}%"></div>
    </div>

    <div class="welcome-body">
      {#if step === 0}
        <h3 class="welcome-step-title">Pick a theme</h3>
        <p class="welcome-step-desc">You can change it later in Settings.</p>
        <div class="welcome-themes">
          {#each themeOptions as t (t.name)}
            <button
              class="welcome-theme"
              class:active={themeState.name === t.name}
              onclick={() => pickTheme(t.name)}
              aria-pressed={themeState.name === t.name}
              type="button"
            >
              <span class="welcome-theme-swatch" style="background: {t.bg}; color: {t.fg};">
                <span class="welcome-theme-accent" style="background: {t.accent};"></span>
              </span>
              <span class="welcome-theme-label">{t.label}</span>
              <span class="welcome-theme-sub">{t.sub}</span>
            </button>
          {/each}
        </div>
      {:else if step === 1}
        <h3 class="welcome-step-title">Connect a source</h3>
        <p class="welcome-step-desc">
          Tokens live in your macOS Keychain — Woom never stores plaintext credentials. You can connect more later.
        </p>
        <div class="welcome-sources">
          {#each implementedSources.filter((s) => s.kind === 'source') as conn (conn.id)}
            {@const connected = connectionsState[conn.id as 'github' | 'jira' | 'sentry']?.kind === 'connected'}
            <button
              class="welcome-source"
              class:connected
              onclick={() => onConnect(conn.id)}
              type="button"
            >
              <span class="welcome-source-name">{conn.name}</span>
              <span class="welcome-source-status">{connected ? 'connected' : 'connect'}</span>
            </button>
          {/each}
        </div>
      {:else}
        <h3 class="welcome-step-title">Pick an agent</h3>
        <p class="welcome-step-desc">
          Woom drives Claude Code or Cursor as a CLI subprocess. Install at least one to enable agent columns. We won't store any credentials — they auth to their own services.
        </p>
        <div class="welcome-sources">
          {#each implementedSources.filter((s) => s.kind === 'agent') as conn (conn.id)}
            {@const ready = (conn.id === 'claude' ? connectionsState.claude : connectionsState.cursor)?.ready ?? false}
            <button
              class="welcome-source"
              class:connected={ready}
              onclick={() => onConnect(conn.id)}
              type="button"
            >
              <span class="welcome-source-name">{conn.name}</span>
              <span class="welcome-source-status">{ready ? 'ready' : 'check status'}</span>
            </button>
          {/each}
        </div>
      {/if}
    </div>

    <footer class="welcome-foot">
      <button class="welcome-back" onclick={back} disabled={step === 0} type="button">Back</button>
      <button class="welcome-next" onclick={next} type="button">
        {step === STEP_COUNT - 1 ? 'Done' : 'Next'}
      </button>
    </footer>
  </section>
</div>

<style>
  .welcome-backdrop {
    position: fixed; inset: 0;
    background: var(--backdrop);
    backdrop-filter: blur(22px) saturate(1.1);
    -webkit-backdrop-filter: blur(22px) saturate(1.1);
    display: flex; align-items: center; justify-content: center;
    z-index: 900;
    padding: 32px;
  }
  .welcome-panel {
    width: 100%; max-width: 640px;
    background: linear-gradient(180deg, var(--bg-2), var(--bg-1));
    border: 1px solid var(--border-hi);
    border-radius: var(--r-modal, 16px);
    box-shadow: var(--shadow-3), 0 0 0 1px var(--border-accent-2);
    display: flex; flex-direction: column;
    overflow: hidden;
  }
  .welcome-head {
    display: flex; align-items: center; gap: 14px;
    padding: 22px 24px 14px;
  }
  .welcome-head-text { flex: 1; min-width: 0; }
  .welcome-title {
    margin: 0;
    font-family: 'Geist', 'Inter', -apple-system, system-ui, sans-serif;
    font-size: 28px; font-weight: 600;
    letter-spacing: -0.02em; line-height: 1.18;
    color: var(--text-0);
  }
  .welcome-sub {
    margin: 4px 0 0;
    font-family: 'JetBrains Mono', monospace;
    font-size: 10.5px; color: var(--text-mute);
    letter-spacing: 0.10em; text-transform: uppercase;
  }
  .welcome-skip {
    background: none; border: none; cursor: pointer;
    color: var(--text-2); font-size: 12px;
    padding: 4px 8px; border-radius: 6px;
  }
  .welcome-skip:hover { background: var(--bg-2); color: var(--text-0); }

  .welcome-progress {
    height: 3px; background: var(--bg-2);
    overflow: hidden;
  }
  .welcome-progress-bar {
    height: 100%; background: linear-gradient(90deg, var(--accent), var(--accent-bright));
    transition: width 220ms ease;
  }
  @media (prefers-reduced-motion: reduce) {
    .welcome-progress-bar { transition: none; }
  }

  .welcome-body {
    padding: 22px;
    display: flex; flex-direction: column; gap: 14px;
  }
  .welcome-step-title { margin: 0; font-size: 15px; color: var(--text-0); font-weight: 600; }
  .welcome-step-desc { margin: 0; font-size: 12.5px; color: var(--text-1); line-height: 1.55; }

  .welcome-themes {
    display: grid; grid-template-columns: repeat(3, 1fr); gap: 10px;
    margin-top: 4px;
  }
  .welcome-theme {
    background: var(--bg-0);
    border: 1px solid var(--border-neutral);
    border-radius: 10px;
    padding: 12px;
    display: flex; flex-direction: column; gap: 4px;
    cursor: pointer; text-align: left;
    transition: all 140ms ease;
  }
  .welcome-theme:hover { border-color: var(--border-hi2); }
  .welcome-theme.active { border-color: var(--accent); box-shadow: 0 0 0 1px var(--accent); }
  .welcome-theme-swatch {
    width: 100%; height: 56px; border-radius: 6px;
    display: flex; align-items: flex-end; justify-content: flex-end;
    padding: 6px;
  }
  .welcome-theme-accent { width: 14px; height: 14px; border-radius: 50%; }
  .welcome-theme-label { font-size: 13px; color: var(--text-0); font-weight: 600; }
  .welcome-theme-sub { font-size: 11px; color: var(--text-mute); }

  .welcome-sources { display: flex; flex-direction: column; gap: 6px; margin-top: 4px; }
  .welcome-source {
    display: flex; align-items: center; justify-content: space-between;
    padding: 10px 14px;
    background: var(--bg-0);
    border: 1px solid var(--border-neutral);
    border-radius: 8px;
    cursor: pointer;
    transition: all 140ms ease;
  }
  .welcome-source:hover { border-color: var(--border-hi2); background: var(--bg-2); }
  .welcome-source.connected {
    border-color: rgba(168, 217, 184, 0.40);
    background: rgba(168, 217, 184, 0.06);
  }
  .welcome-source-name { font-size: 13px; color: var(--text-0); font-weight: 500; }
  .welcome-source-status { font-size: 11px; color: var(--text-2); text-transform: uppercase; letter-spacing: 0.04em; }
  .welcome-source.connected .welcome-source-status { color: var(--accent-bright); }

  .welcome-foot {
    display: flex; justify-content: space-between; gap: 10px;
    padding: 14px 22px;
    border-top: 1px solid var(--border-neutral);
    background: var(--bg-0);
  }
  .welcome-back, .welcome-next {
    padding: 8px 16px; border-radius: 8px;
    font-size: 12.5px; font-weight: 500;
    cursor: pointer; border: none;
    transition: all 140ms ease;
  }
  .welcome-back {
    background: transparent;
    color: var(--text-2);
    border: 1px solid var(--border-neutral-hi);
  }
  .welcome-back:hover:not(:disabled) { background: var(--bg-2); color: var(--text-0); }
  .welcome-back:disabled { opacity: 0.4; cursor: not-allowed; }
  .welcome-next {
    background: linear-gradient(180deg, var(--accent-bright), var(--accent));
    color: var(--accent-fg);
    font-weight: 600;
    box-shadow:
      0 6px 18px var(--accent-glow),
      inset 0 1px 0 rgba(255, 255, 255, 0.20);
  }
  .welcome-next:hover { transform: translateY(-1px); }
</style>
