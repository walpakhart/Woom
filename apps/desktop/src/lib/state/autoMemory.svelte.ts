/* Auto-memory inject — pulls the user's durable preferences + feedback
 * entries from `memory_local` and stamps them into the agent's
 * system-prompt suffix at session start. Mirrors Claude Code's
 * MEMORY.md autoload (`docs/CLAUDE_PARITY.md §7.3`).
 *
 * Scope: `user` + `feedback` kinds (top 20 each, newest-first). The
 * `project` / `reference` / `note` kinds stay out of the prefix —
 * they're searchable via the existing `memory_search` MCP tool when
 * the agent needs them. Always-injecting everything would blow the
 * prompt-cache prefix.
 *
 * Cache: module-level. Refreshed on app boot and after the user
 * saves/edits a memory (Settings UI hooks this in). The cached
 * snapshot is read sync by `buildAgentAppContext`. */

import { invoke } from '@tauri-apps/api/core';

export interface MemoryHit {
  id: number;
  kind: string;
  content: string;
  tags: string;
  created_at: number;
}

const TOP_PER_KIND = 20;

let cachedBlock = '';
let lastLoadedAt = 0;

export function getCachedAutoMemoryBlock(): string {
  return cachedBlock;
}

export async function refreshAutoMemory(): Promise<void> {
  try {
    const [users, feedback] = await Promise.all([
      invoke<MemoryHit[]>('memory_list_local', { kind: 'user', limit: TOP_PER_KIND, offset: 0 }),
      invoke<MemoryHit[]>('memory_list_local', { kind: 'feedback', limit: TOP_PER_KIND, offset: 0 })
    ]);
    cachedBlock = formatBlock(users, feedback);
    lastLoadedAt = Date.now();
  } catch (e) {
    console.warn('auto-memory load failed', e);
    cachedBlock = '';
  }
}

function formatBlock(users: MemoryHit[], feedback: MemoryHit[]): string {
  if (users.length === 0 && feedback.length === 0) return '';
  const parts: string[] = [];
  parts.push('Long-term memory (auto-loaded from Woom\'s SQLite store):');
  if (users.length > 0) {
    parts.push('');
    parts.push('**About the user (kind=user):**');
    for (const h of users) {
      parts.push(`- ${oneLine(h.content)}`);
    }
  }
  if (feedback.length > 0) {
    parts.push('');
    parts.push('**Feedback / preferences (kind=feedback):**');
    for (const h of feedback) {
      parts.push(`- ${oneLine(h.content)}`);
    }
  }
  parts.push('');
  parts.push(
    '_For project facts / references / notes, call `mcp__memory__memory_search` ' +
    'with a focused query — they live in the same store but aren\'t auto-prefixed._'
  );
  return parts.join('\n');
}

/** Collapse multi-line content to a single line for the prefix
 *  digest. The full content is reachable via memory_get if the
 *  agent needs it. Cap at 280 chars so a wall-of-text memory
 *  doesn't dominate the prefix. */
function oneLine(content: string): string {
  const flat = content.replace(/\s+/g, ' ').trim();
  return flat.length > 280 ? flat.slice(0, 277) + '…' : flat;
}

/** Diagnostic — how stale is the cached block. Used by Settings
 *  view to show "auto-memory last refreshed: 3 minutes ago". */
export function autoMemoryLastLoadedAt(): number {
  return lastLoadedAt;
}
