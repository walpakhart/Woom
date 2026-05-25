// Action-IPC payload converters — extracted from +page.svelte in
// wave-3 phase-9 split. Both functions are pure (input shape → output
// shape, no IO, no reactive state) so the dispatcher in +page.svelte
// shrinks by ~100 LoC and these become trivially unit-testable.

import type { ClaudeAction } from '$lib/types';

/** Wire shape of `propose_*` IPC requests fired by the sidecar
 *  ahead of the matching tool_use stream event. The dispatcher in
 *  +page.svelte matches incoming payloads against pending action
 *  cards (via `actionMatchesIpcParams`) and creates new ones from
 *  scratch (via `buildActionFromIpcRequest`) when no match is
 *  found. */
export type ActionRequestPayload = {
  session_id: string;
  wait_id: string;
  kind: 'bash' | 'commit' | 'pr' | 'switch_cwd' | 'question';
  params: Record<string, unknown>;
};

/** Compare an in-flight `ClaudeAction` against a fresh IPC payload —
 *  used by the dispatcher to skip re-creating a duplicate card when
 *  the stream parser has already emitted the same action via the
 *  tool_use event path. Match by kind + primary identity field
 *  (command / message / title / path). */
export function actionMatchesIpcParams(
  a: ClaudeAction,
  kind: ActionRequestPayload['kind'],
  params: Record<string, unknown>
): boolean {
  if (a.kind !== kind) return false;
  if (kind === 'bash')
    return a.kind === 'bash' && a.command === String(params.command ?? '');
  if (kind === 'commit')
    return a.kind === 'commit' && a.message === String(params.message ?? '');
  if (kind === 'pr')
    return a.kind === 'pr' && a.title === String(params.title ?? '');
  if (kind === 'switch_cwd')
    return a.kind === 'switch_cwd' && a.path === String(params.path ?? '');
  return false;
}

/** Build a `ClaudeAction` card from a raw IPC `ActionRequestPayload`.
 *  Used when the sidecar fires `action_request` BEFORE the matching
 *  `tool_use` event lands on the stream (e.g. async pre-approval
 *  flows). The stream parser later spots the matching `waitId` and
 *  skips its own duplicate creation. Returns null for unknown
 *  kinds — keeps the dispatcher fail-open on schema growth. */
export function buildActionFromIpcRequest(p: ActionRequestPayload): ClaudeAction | null {
  const id = (typeof crypto !== 'undefined' && crypto.randomUUID)
    ? crypto.randomUUID()
    : `act-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
  const px = p.params;
  if (p.kind === 'bash') {
    return {
      id,
      kind: 'bash',
      command: String(px.command ?? ''),
      reason: typeof px.reason === 'string' ? px.reason : '',
      status: 'pending',
      waitId: p.wait_id,
    };
  }
  if (p.kind === 'commit') {
    return {
      id,
      kind: 'commit',
      message: String(px.message ?? ''),
      body: typeof px.body === 'string' ? px.body : '',
      push: px.push !== false,
      note: typeof px.note === 'string' ? px.note : '',
      status: 'pending',
      waitId: p.wait_id,
    };
  }
  if (p.kind === 'pr') {
    return {
      id,
      kind: 'pr',
      title: String(px.title ?? ''),
      body: typeof px.body === 'string' ? px.body : '',
      base: typeof px.base === 'string' ? px.base : '',
      draft: px.draft === true,
      note: typeof px.note === 'string' ? px.note : '',
      status: 'pending',
      waitId: p.wait_id,
    };
  }
  if (p.kind === 'switch_cwd') {
    return {
      id,
      kind: 'switch_cwd',
      path: String(px.path ?? ''),
      reason: typeof px.reason === 'string' ? px.reason : '',
      status: 'pending',
      waitId: p.wait_id,
    };
  }
  if (p.kind === 'question') {
    const opts = Array.isArray(px.options) ? px.options : [];
    /* The sidecar normalises `kind` for us (single/multi/text/confirm),
     * but older serialised sessions may still carry only `multi_select`
     * — derive from that as a fallback so resumed turns keep working. */
    const rawKind = typeof px.kind === 'string' ? px.kind : '';
    const questionKind: 'single' | 'multi' | 'text' | 'confirm' =
      rawKind === 'multi'   ? 'multi'   :
      rawKind === 'text'    ? 'text'    :
      rawKind === 'confirm' ? 'confirm' :
      rawKind === 'single'  ? 'single'  :
      px.multi_select === true ? 'multi' : 'single';
    return {
      id,
      kind: 'question',
      questionKind,
      question: String(px.question ?? ''),
      header: typeof px.header === 'string' ? px.header : undefined,
      options: opts
        .map((o) => (typeof o === 'object' && o !== null
          ? {
              label: String((o as Record<string, unknown>).label ?? ''),
              description: typeof (o as Record<string, unknown>).description === 'string'
                ? String((o as Record<string, unknown>).description)
                : undefined,
            }
          : { label: String(o) }))
        .filter((o) => o.label.length > 0),
      multiSelect: questionKind === 'multi',
      status: 'pending',
      waitId: p.wait_id,
    };
  }
  return null;
}
