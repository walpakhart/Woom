/* Session export to Markdown + JSON
 * (`docs/ROADMAP_1.0.md §2.2.8`).
 *
 * Two outputs:
 *   - Markdown — human-readable transcript, suitable for paste into
 *     GitHub issues / docs / chat. User + assistant messages alternate
 *     under role headers.
 *   - JSON — full session snapshot, for support bundles or feeding
 *     back into an analysis tool. Includes everything ClaudeSession
 *     carries (mentions, actions, worktree state) sans transient
 *     `sending` / `awaitingApproval` flags that would mislead a
 *     reader looking at a stale dump.
 *
 * Both are returned as plain strings — the caller chooses to copy
 * to clipboard, save to disk, or hand to a different surface.
 */

import type { ClaudeSession, ClaudeMessage } from '$lib/types';

export function exportSessionMarkdown(session: ClaudeSession): string {
  const header: string[] = [];
  const title = session.title || `Session ${session.id.slice(0, 6)}`;
  header.push(`# ${title}`);
  header.push('');
  header.push(`- Agent: \`${session.agentKind}\``);
  if (session.claudeModel) header.push(`- Model: \`${session.claudeModel}\``);
  if (session.cursorModel) header.push(`- Model: \`${session.cursorModel}\``);
  if (session.cwd) header.push(`- Cwd: \`${session.cwd}\``);
  header.push(`- Session id: \`${session.id}\``);
  header.push(`- Messages: ${session.messages.length}`);
  header.push('');
  header.push('---');
  header.push('');

  const body: string[] = [];
  for (const msg of session.messages) {
    body.push(`## ${roleLabel(msg.role)} · ${msg.at}`);
    body.push('');
    body.push(formatBody(msg));
    if (msg.usage) {
      body.push('');
      body.push(formatUsageFootnote(msg));
    }
    body.push('');
  }
  return [...header, ...body].join('\n');
}

export function exportSessionJson(session: ClaudeSession): string {
  /* Whitelist serializable fields. Drop the live-only flags so a
   * dump pasted back doesn't fool the reader into thinking the
   * session was mid-send when exported. */
  const out = {
    id: session.id,
    title: session.title,
    agentKind: session.agentKind,
    cwd: session.cwd,
    claudeModel: session.claudeModel,
    cursorModel: session.cursorModel,
    claudeUuid: session.claudeUuid,
    agentInstanceId: session.agentInstanceId,
    linkedToEditor: session.linkedToEditor,
    linkedToEditorInstanceId: session.linkedToEditorInstanceId,
    linkedCanvasId: session.linkedCanvasId,
    worktreePath: session.worktreePath,
    worktreeBranch: session.worktreeBranch,
    worktreeRepo: session.worktreeRepo,
    mentions: session.mentions,
    actions: session.actions,
    messages: session.messages.map((m) => ({
      role: m.role,
      content: m.content,
      at: m.at,
      usage: m.usage,
      images: m.images
    })),
    exportedAt: new Date().toISOString()
  };
  return JSON.stringify(out, null, 2);
}

function roleLabel(role: ClaudeMessage['role']): string {
  switch (role) {
    case 'user':      return 'User';
    case 'assistant': return 'Assistant';
    case 'system':    return 'System';
  }
}

function formatBody(msg: ClaudeMessage): string {
  /* Multi-line content stays as-is — the markdown renderer downstream
   * will format code fences / lists inside the message body itself. */
  if (!msg.content) return '_(empty)_';
  return msg.content;
}

function formatUsageFootnote(msg: ClaudeMessage): string {
  const u = msg.usage;
  if (!u) return '';
  const parts: string[] = [];
  parts.push(`in:${u.inputTokens}`);
  parts.push(`out:${u.outputTokens}`);
  if (u.cacheReadTokens) parts.push(`cacheR:${u.cacheReadTokens}`);
  if (u.cacheCreationTokens) parts.push(`cacheW:${u.cacheCreationTokens}`);
  return `_usage — ${parts.join(' · ')}${u.model ? ` · ${u.model}` : ''}_`;
}
