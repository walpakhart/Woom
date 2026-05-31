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
    lastContextSize: 0,
    linkedToEditor: false,
    linkedToEditorInstanceId: null,
    linkedCanvasId: null,
    linkedTerminalInstanceId: null,
    agentInstanceId: null,
    cwdSwitchRecap: null,
    cwdUuids: {},
    awaitingApproval: false,
    pendingActionResults: [],
    pendingTurn: null
  };
}

describe('buildCwdSwitchRecap', () => {
  it('always returns a recap string (never null), even with no messages', () => {
    const r = buildCwdSwitchRecap(fakeSession([]), '/old', '/new', { resumed: false });
    expect(typeof r).toBe('string');
    expect(r).toContain('No prior exchanges to recap');
  });

  it('skips empty / whitespace-only messages → "no prior exchanges" recap', () => {
    const r = buildCwdSwitchRecap(
      fakeSession([
        { role: 'user', content: '', at: '2026-01-01' },
        { role: 'assistant', content: '   ', at: '2026-01-01' }
      ]),
      '/old',
      '/new',
      { resumed: false }
    );
    expect(r).toContain('No prior exchanges to recap');
  });

  it('uses the "fresh CLI session" framing when resumed: false', () => {
    const r = buildCwdSwitchRecap(
      fakeSession([{ role: 'user', content: 'Hello', at: '2026-01-01' }]),
      '/old',
      '/new',
      { resumed: false }
    );
    expect(r).toContain('project the session has not visited before');
    expect(r).toContain('- Previous cwd (just left): /old');
    expect(r).toContain('- Current cwd: /new');
    expect(r).toContain('User: Hello');
  });

  it('uses the "returning to project" framing when resumed: true', () => {
    const r = buildCwdSwitchRecap(
      fakeSession([{ role: 'user', content: 'Hi again', at: '2026-01-01' }]),
      '/old',
      '/new',
      { resumed: true }
    );
    expect(r).toContain("cwd just returned to a project you've worked in before");
    expect(r).toContain('- Current cwd: /new');
  });

  it('caps to the last 30 meaningful messages (+ first-message anchor)', () => {
    const messages: ClaudeSession['messages'] = [];
    for (let i = 0; i < 35; i++) {
      messages.push({ role: 'user', content: `msg ${i}`, at: '2026-01-01' });
    }
    const r = buildCwdSwitchRecap(fakeSession(messages), null, '/new', { resumed: false });
    // Last 30 kept (msg 5..34); msg 4 dropped; msg 0 re-included as the
    // ORIGINAL TASK north-star anchor.
    expect(r).toContain('msg 34');
    expect(r).toContain('msg 5');
    expect(r).not.toContain('msg 4');
    expect(r).toContain('msg 0');
  });

  it('truncates a long non-tail message at the 2500-char cap', () => {
    const long = 'x'.repeat(3000);
    const r = buildCwdSwitchRecap(
      fakeSession([
        { role: 'user', content: long, at: '2026-01-01' },
        { role: 'assistant', content: 'a', at: '2026-01-01' },
        { role: 'user', content: 'b', at: '2026-01-01' }
      ]),
      null,
      '/new',
      { resumed: false }
    );
    // Non-tail messages cap at 2500 → 2499 chars + ellipsis.
    expect(r).toContain('x'.repeat(2499) + '…');
    expect(r).not.toContain('x'.repeat(2501));
  });

  it('omits the cwd lines when oldCwd / newCwd are null', () => {
    const r = buildCwdSwitchRecap(
      fakeSession([{ role: 'user', content: 'hi', at: '2026-01-01' }]),
      null,
      null,
      { resumed: false }
    );
    expect(r).not.toContain('Previous cwd');
    expect(r).not.toContain('Current cwd:');
  });
});
