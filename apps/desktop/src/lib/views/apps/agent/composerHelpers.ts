// Pure helpers + curated catalogues for the Composer model / effort
// pickers. Extracted from Composer.svelte in wave-1 phase-6 refactor.
// No Svelte state, no IPC — just data + small functions consumed by
// `$derived` blocks in the composer template.

/** Per-model context-window cap. Wrong here = the ring shows 100%
 *  on models that actually have 5× the headroom. Numbers tracked
 *  against Anthropic's published limits as of late-2025; falls
 *  through to the 200K Sonnet/Haiku default for unknown ids. */
export function modelContextLimit(model: string | null | undefined): number {
  if (!model) return 200_000;
  /* Opus 4.8 default tier dropped to 200K; the dedicated 1M variant
     carries an explicit `[1m]` suffix. Check the 1M-variant first
     because `startsWith('claude-opus-4-8')` matches both. */
  if (model.startsWith('claude-opus-4-8[1m]')) return 1_000_000;
  if (model.startsWith('claude-opus-4-8')) return 200_000;
  /* Opus 4.7 + earlier ship with a 1M-token window by default (the
     same extended-context tier Sonnet 4.5 has). */
  if (model.startsWith('claude-opus-4')) return 1_000_000;
  if (model.startsWith('claude-sonnet-4-6')) return 200_000;
  if (model.startsWith('claude-sonnet')) return 1_000_000;
  if (model.startsWith('claude-haiku')) return 200_000;
  /* Cursor models inherit the Anthropic limits when proxied. */
  if (model.includes('opus-4')) return 1_000_000;
  if (model.includes('sonnet-4-6')) return 200_000;
  return 200_000;
}

/** "47%" / "—" formatter for the quota-bar pill. */
export function fmtPct(b: { utilization: number | null } | null): string {
  if (!b || b.utilization == null) return '—';
  return `${Math.round(b.utilization)}%`;
}

/** Threshold-based CSS modifier for the same pill. Returns
 *  `cmp-q--err` ≥ 90%, `cmp-q--warn` ≥ 70%, empty string otherwise. */
export function pctClass(b: { utilization: number | null } | null): string {
  if (!b || b.utilization == null) return '';
  if (b.utilization >= 90) return 'cmp-q--err';
  if (b.utilization >= 70) return 'cmp-q--warn';
  return '';
}

/* Real Claude model ids only — Claude CLI rejects the run with
   "model does not exist" if we pass anything it can't resolve. */
export const claudeModels: { value: string; label: string }[] = [
  /* Opus 4.8 launched 2026-05-28 at $5/$25 per 1M (was $15/$75 on
     4.7). The 1M-context variant is a separate model id with the
     `[1m]` suffix — exposed as its own dropdown entry. Fast mode
     is handled separately via the FAST chip (it's a per-session
     toggle, not a model id). */
  { value: 'claude-opus-4-8', label: 'Opus 4.8' },
  { value: 'claude-opus-4-8[1m]', label: 'Opus 4.8 · 1M' },
  { value: 'claude-opus-4-7', label: 'Opus 4.7' },
  { value: 'claude-sonnet-4-6', label: 'Sonnet 4.6' },
  { value: 'claude-haiku-4-5-20251001', label: 'Haiku 4.5' },
];

/* Curated subset of Cursor's model catalogue (the CLI exposes ~100
   SKUs via `cursor-agent --list-models`, including every effort
   tier and "fast" variant). We surface the headline picks across
   vendors so the dropdown stays scannable; advanced effort tiers
   stay reachable through the CLI directly. Keep ids EXACTLY as
   `--list-models` reports — cursor-agent rejects the run with
   "model does not exist" if we pass an alias it doesn't know. */
export const cursorModels: { value: string; label: string }[] = [
  { value: 'auto', label: 'Auto' },
  { value: 'composer-2', label: 'Composer 2' },
  { value: 'claude-4.6-sonnet-medium', label: 'Sonnet 4.6' },
  { value: 'claude-4.6-sonnet-medium-thinking', label: 'Sonnet 4.6 Thinking' },
  { value: 'claude-opus-4-7-medium', label: 'Opus 4.7' },
  { value: 'claude-opus-4-7-high', label: 'Opus 4.7 High' },
  { value: 'claude-opus-4-7-thinking-medium', label: 'Opus 4.7 Thinking' },
  { value: 'gpt-5.5-medium', label: 'GPT 5.5' },
  { value: 'gpt-5.5-high', label: 'GPT 5.5 High' },
  { value: 'gpt-5.4-medium', label: 'GPT 5.4' },
  { value: 'gpt-5.3-codex', label: 'Codex 5.3' },
  { value: 'gpt-5.1', label: 'GPT 5.1' },
  { value: 'gemini-3.1-pro', label: 'Gemini 3.1 Pro' },
  { value: 'grok-4.3', label: 'Grok 4.3' },
];

export const claudeEffort: { value: string; label: string }[] = [
  { value: 'auto', label: 'Effort · auto' },
  { value: 'low', label: 'Effort · low' },
  { value: 'medium', label: 'Effort · medium' },
  { value: 'high', label: 'Effort · high' },
  { value: 'max', label: 'Effort · max' },
];

/** Trigger detector for the `/` slash-command + `@` mention pickers.
 *  Both pickers anchor to the most recent occurrence of their marker
 *  character (preceded by start-of-string or whitespace) and treat
 *  everything between marker and caret as a live query string. The
 *  query closes when a whitespace lands between marker and caret
 *  (so `/foo ` cancels the picker on the trailing space) or when the
 *  query crosses a newline. Returns `{ at, query }` on a live trigger,
 *  null otherwise. Pure — no DOM mutation, no reactive read. */
export function detectTriggerPosition(
  value: string,
  caret: number,
  marker: '/' | '@'
): { at: number; query: string } | null {
  let at = -1;
  for (let i = caret - 1; i >= 0; i--) {
    const c = value[i];
    if (c === marker) {
      if (i === 0 || /\s/.test(value[i - 1])) at = i;
      break;
    }
    if (/\s/.test(c)) break;
  }
  if (at < 0) return null;
  const query = value.slice(at + 1, caret);
  if (query.includes('\n')) return null;
  return { at, query };
}

/** Splice a token in place of the active trigger query. Used by
 *  `pickSlashCommand` / `pickSkill` / `pickMention` — they all
 *  share the same "replace `/<query>` (or `@<query>`) with
 *  `<insertion>` and place caret immediately after" pattern.
 *  Returns the rewritten string + the post-insertion caret offset
 *  so the caller can apply both to the textarea in one go. */
export function spliceTriggerInsertion(
  value: string,
  caret: number,
  triggerFrom: number,
  insertion: string
): { next: string; caretAfter: number } {
  const before = value.slice(0, triggerFrom);
  const after = value.slice(caret);
  const next = before + insertion + after;
  return { next, caretAfter: (before + insertion).length };
}
