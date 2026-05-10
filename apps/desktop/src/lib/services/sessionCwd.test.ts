// Tests for the cwd-switch recap builder. The function is the only
// part of sessionCwd.ts that's testable without spinning up the
// sessionsState reactive store — `applySessionCwd` mutates that
// store and would need a Svelte test environment. The recap builder
// is a pure data transform; covering it here means a regression in
// the wording / truncation rules surfaces immediately.

import { describe, expect, it } from 'vitest';

import { buildCwdSwitchRecap } from './sessionCwd';
import type { ClaudeSession } from '$lib/types';

/** Minimal valid `ClaudeSession` for tests — fields not relevant to
 *  `buildCwdSwitchRecap` get safe defaults. The function only reads
 *  `messages`, so everything else is busy-work. */
function fakeSession(messages: ClaudeSession['messages']): ClaudeSession {
  return {
    id: 'sess-1',
    title: 'Test',
    mentions: [],
    messages,
    input: '',
    sending: false,
    cwd: null,
    worktreePath: null,
    worktreeBranch: null,
    worktreeRepo: null,
    actions: [],
    claudeUuid: 'uuid-1',
    claudeResumable: false,
    agentKind: 'claude',
    cursorModel: null,
    claudeModel: null,
    claudeToolProfile: null,
    lastContextSize: 0,
    linkedToEditor: false,
    linkedToEditorInstanceId: null,
    linkedCanvasId: null,
    linkedTerminalInstanceId: null,
    agentInstanceId: null,
    cwdSwitchRecap: null,
    cwdUuids: {},
    awaitingApproval: false,
    pendingActionResults: []
  };
}

describe('buildCwdSwitchRecap', () => {
  it('returns null when there are no meaningful messages', () => {
    expect(buildCwdSwitchRecap(fakeSession([]), '/old', '/new', { resumed: false })).toBeNull();
  });

  it('skips empty / whitespace-only messages and returns null when nothing remains', () => {
    const r = buildCwdSwitchRecap(
      fakeSession([
        { role: 'user', content: '', at: '2026-01-01' },
        { role: 'assistant', content: '   ', at: '2026-01-01' }
      ]),
      '/old',
      '/new',
      { resumed: false }
    );
    expect(r).toBeNull();
  });

  it('uses the "fresh CLI session" framing when resumed: false', () => {
    const r = buildCwdSwitchRecap(
      fakeSession([{ role: 'user', content: 'Hello', at: '2026-01-01' }]),
      '/old',
      '/new',
      { resumed: false }
    );
    expect(r).toContain('cwd just changed mid-conversation');
    expect(r).toContain('- Previous cwd: /old');
    expect(r).toContain('- New cwd: /new');
    expect(r).toContain('User: Hello');
  });

  it('uses the "returning to project" framing when resumed: true', () => {
    const r = buildCwdSwitchRecap(
      fakeSession([{ role: 'user', content: 'Hi again', at: '2026-01-01' }]),
      '/old',
      '/new',
      { resumed: true }
    );
    expect(r).toContain("returning to a project you've been in before");
    expect(r).toContain('- Now back in: /new');
  });

  it('caps to the last 6 meaningful messages', () => {
    const messages: ClaudeSession['messages'] = [];
    for (let i = 0; i < 10; i++) {
      messages.push({ role: 'user', content: `msg ${i}`, at: '2026-01-01' });
    }
    const r = buildCwdSwitchRecap(fakeSession(messages), null, '/new', { resumed: false }) ?? '';
    // Earliest 4 should be dropped, latest 6 kept.
    expect(r).not.toContain('msg 0');
    expect(r).not.toContain('msg 3');
    expect(r).toContain('msg 4');
    expect(r).toContain('msg 9');
  });

  it('truncates individual messages over 800 chars with an ellipsis', () => {
    const long = 'x'.repeat(2000);
    const r = buildCwdSwitchRecap(
      fakeSession([{ role: 'user', content: long, at: '2026-01-01' }]),
      null,
      '/new',
      { resumed: false }
    ) ?? '';
    // Should contain the head (799 chars + "…") but not the tail.
    expect(r).toContain('x'.repeat(799) + '…');
    expect(r).not.toContain('x'.repeat(801));
  });

  it('omits the cwd lines when oldCwd / newCwd are null', () => {
    const r = buildCwdSwitchRecap(
      fakeSession([{ role: 'user', content: 'hi', at: '2026-01-01' }]),
      null,
      null,
      { resumed: false }
    ) ?? '';
    expect(r).not.toContain('Previous cwd:');
    expect(r).not.toContain('New cwd:');
  });
});
