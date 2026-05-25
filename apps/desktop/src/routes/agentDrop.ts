// Agent column drop + image-paste pipeline extracted from
// `+page.svelte` in wave-35.
//
// Covers:
//   - `attachBlobsToSession` — save in-memory image blobs under
//     $APPDATA/chat-attachments/ then attach the resulting paths via
//     `attachPathsToSession`.
//   - `pasteImagesIntoColumn` — clipboard-paste variant that picks
//     the right target session before delegating to attachBlobs.
//   - `onAgentDrop` — the meaty drag-drop handler the agent column
//     uses. Routes through 3 source kinds: in-app drag (file from
//     editor tree, ticket from inbox), OS Finder drop (text/uri-list),
//     in-memory image blob (Cmd+Shift+5 / cross-tab drag).
//
// Caller-supplied deps cover the route-local closures we can't
// reach from a TS module: `setJustDragged` flips the local debounce
// flag the dragend handler reads; `clearAgentDragState` empties the
// `agentDragCounts` Map + drops the highlighted-column id.

import { invoke } from '@tauri-apps/api/core';
import {
  attachPathsToSession,
  newClaudeSession,
  sessionsState,
  setActiveSessionInInstance,
  updateSession,
} from '$lib/state/sessions.svelte';
import { dragState, setDragPayload } from '$lib/state/drag.svelte';
import { externalId } from '$lib/data';
import type { ClaudeSession, Mention } from '$lib/types';
import {
  blobToBase64,
  deriveCwd,
  guessExt,
  imageFilesFromEvent,
} from './page_helpers';

export interface AgentDropDeps {
  setJustDragged(v: boolean): void;
  clearAgentDragState(): void;
}

let cachedAppDataDir: string | null = null;

/** Prime the cache from `onMount` — saves one round-trip when the
 *  user drops their first image since the path is already known.
 *  Call sites pass `await invoke<string>('app_data_dir')`. */
export function setCachedAppDataDir(dir: string): void {
  cachedAppDataDir = dir;
}

/** App-data path for chat image attachments (clipboard / Cmd+Shift+5
 *  floating preview / direct File blob drop). Resolved lazily once and
 *  cached — the OS path is stable for the install. Lives under
 *  `$APPDATA` which is in `assetProtocol.scope` so `convertFileSrc` can
 *  render thumbnails. */
export async function getAttachmentDir(): Promise<string> {
  if (!cachedAppDataDir) {
    cachedAppDataDir = await invoke<string>('app_data_dir');
  }
  return `${cachedAppDataDir}/chat-attachments`;
}

/** Save a list of in-memory image blobs to disk + attach them to a
 *  session. Used for Files drops (Cmd+Shift+5 floating preview, drag
 *  from another browser tab) and clipboard paste — anywhere we have
 *  bytes but no source path. Sanitises the filename and prefixes a
 *  timestamp so two screenshots from the same minute don't collide. */
export async function attachBlobsToSession(
  sessionId: string,
  blobs: { name: string; type: string; blob: Blob }[],
): Promise<number> {
  if (blobs.length === 0) return 0;
  const dir = await getAttachmentDir();
  const savedPaths: string[] = [];
  for (const item of blobs) {
    try {
      const b64 = await blobToBase64(item.blob);
      const safe = (item.name || `image.${guessExt(item.type)}`)
        .replace(/[/\\]+/g, '_')
        .replace(/\s+/g, ' ')
        .trim();
      const stamp = `${Date.now()}-${Math.random().toString(36).slice(2, 7)}`;
      const path = `${dir}/${stamp}-${safe}`;
      await invoke('fs_write_bytes', { path, base64: b64 });
      savedPaths.push(path);
    } catch (err) {
      console.warn('attach blob failed', err);
    }
  }
  if (savedPaths.length === 0) return 0;
  return attachPathsToSession(sessionId, savedPaths);
}

/** Cmd+V of one or more images in a chat composer. Routes through
 *  the same blob → on-disk → mention pipeline as drag-drop, so the
 *  resulting attachment chip strip + transcript thumbnail look
 *  identical. */
export async function pasteImagesIntoColumn(
  instanceId: string,
  kind: 'claude' | 'cursor',
  blobs: { name: string; type: string; blob: Blob }[],
): Promise<number> {
  if (blobs.length === 0) return 0;
  // Resolve target the same way `onAgentDrop` does: active session in this
  // column, then any session bound here, then a fresh one of this kind.
  // Prefer `activeIds[kind]` since that's what ChatThread renders;
  // `activeByInstance[instanceId]` only updates when the focused
  // session has an `agentInstanceId`, which leaves floating sessions
  // out of sync.
  const activeId =
    sessionsState.activeIds[kind] ?? sessionsState.activeByInstance[instanceId];
  let target = activeId ? sessionsState.list.find((s) => s.id === activeId) ?? null : null;
  if (!target) target = sessionsState.list.find((s) => s.agentInstanceId === instanceId) ?? null;
  if (!target) {
    const id = newClaudeSession({ agentKind: kind, agentInstanceId: instanceId });
    target = sessionsState.list.find((s) => s.id === id) ?? null;
  }
  if (!target) return 0;
  const n = await attachBlobsToSession(target.id, blobs);
  if (n > 0) setActiveSessionInInstance(instanceId, target.id);
  return n;
}

export function onAgentDrop(
  instanceId: string,
  kind: 'claude' | 'cursor',
  e: DragEvent,
  deps: AgentDropDeps,
) {
  e.preventDefault();

  // Pick (or create) the drop target: the active session in THIS column
  // instance. Falls back to any session bound to this instance, then a
  // floating session of this kind (adopted), then a fresh one.
  const pickTarget = (): ClaudeSession | null => {
    const activeId =
      sessionsState.activeIds[kind] ?? sessionsState.activeByInstance[instanceId];
    let t = activeId ? sessionsState.list.find((s) => s.id === activeId) ?? null : null;
    if (!t) t = sessionsState.list.find((s) => s.agentInstanceId === instanceId) ?? null;
    if (!t) {
      // Adopt a floating session of the same kind if one exists.
      t = sessionsState.list.find(
        (s) => s.agentKind === kind && s.agentInstanceId === null,
      ) ?? null;
      if (t) updateSession(t.id, { agentInstanceId: instanceId });
    }
    if (!t) {
      const id = newClaudeSession({ agentKind: kind, agentInstanceId: instanceId });
      t = sessionsState.list.find((s) => s.id === id) ?? null;
    }
    return t;
  };

  const settle = () => {
    deps.setJustDragged(true);
    setTimeout(() => deps.setJustDragged(false), 200);
  };

  // 1) Internal drag (file from Editor tree, or ticket from inbox).
  const internal = dragState.payload;
  let filePayload: { path: string; isDir: boolean; name: string } | null = null;
  if (internal && internal.source === 'file') {
    filePayload = { path: internal.path, isDir: internal.isDir, name: internal.name };
  } else {
    const raw = e.dataTransfer?.getData('application/x-woom-file');
    if (raw) {
      try {
        const p = JSON.parse(raw) as { path: string; isDir: boolean; name: string };
        if (p && typeof p.path === 'string') filePayload = p;
      } catch { /* malformed mime payload — ignore */ }
    }
  }
  if (filePayload) {
    const { path, isDir, name } = filePayload;
    const target = pickTarget();
    if (target) {
      const cwd = target.cwd ?? '';
      const rel = cwd && path.startsWith(cwd + '/') ? path.slice(cwd.length + 1) : path;
      const display = '@' + rel + (isDir ? '/' : '');
      const mention: Mention = {
        source: 'file',
        externalId: rel + (isDir ? '/' : ''),
        title: name,
        body: path,
        isDir,
      };
      const sep = target.input && !target.input.endsWith(' ') ? ' ' : '';
      updateSession(target.id, {
        input: target.input + sep + display + ' ',
        mentions: [...target.mentions, mention],
        cwd: target.cwd ?? deriveCwd(path, isDir),
      });
      setActiveSessionInInstance(instanceId, target.id);
    }
    deps.clearAgentDragState();
    setDragPayload(null);
    settle();
    return;
  }

  // 2) OS file drop from Finder / Downloads / other apps.
  const uriList = e.dataTransfer?.getData('text/uri-list') || '';
  if (uriList) {
    const paths = uriList
      .split(/\r?\n/)
      .map((l) => l.trim())
      .filter((l) => l && !l.startsWith('#') && l.startsWith('file://'))
      .map((u) => {
        try {
          return decodeURIComponent(u.replace(/^file:\/\//, ''));
        } catch {
          return '';
        }
      })
      .filter(Boolean);
    if (paths.length > 0) {
      const target = pickTarget();
      if (target) {
        const n = attachPathsToSession(target.id, paths);
        if (n > 0) setActiveSessionInInstance(instanceId, target.id);
      }
      deps.clearAgentDragState();
      settle();
      return;
    }
  }

  // 2.5) In-memory image File blobs (Cmd+Shift+5 floating preview / cross-tab).
  const imageBlobs = imageFilesFromEvent(e);
  if (imageBlobs.length > 0) {
    const target = pickTarget();
    if (target) {
      void attachBlobsToSession(target.id, imageBlobs).then((n) => {
        if (n > 0) setActiveSessionInInstance(instanceId, target.id);
      });
    }
    deps.clearAgentDragState();
    settle();
    return;
  }

  // 3) Ticket / Sentry drop from inbox.
  if (!internal || internal.source === 'file' || internal.source === 'chat-message') {
    deps.clearAgentDragState();
    return;
  }
  let mention: Mention;
  if (internal.source === 'github') {
    mention = {
      source: 'github',
      externalId: externalId(internal.item),
      title: internal.item.title,
      body: internal.item.body,
    };
  } else if (internal.source === 'jira') {
    mention = {
      source: 'jira',
      externalId: internal.item.key,
      title: internal.item.summary,
      body: internal.item.description,
    };
  } else {
    const issue = internal.item;
    const ref = issue.short_id || issue.id;
    const summary = [
      issue.metadata_type && issue.metadata_value
        ? `${issue.metadata_type}: ${issue.metadata_value}`
        : issue.title,
      issue.culprit ? `culprit: ${issue.culprit}` : null,
      `level: ${issue.level} · status: ${issue.status}`,
      `project: ${issue.project_slug} · last seen: ${issue.last_seen}`,
      issue.permalink ? `url: ${issue.permalink}` : null,
    ]
      .filter(Boolean)
      .join('\n');
    mention = {
      source: 'sentry',
      externalId: ref,
      title: issue.title,
      body: summary,
    };
  }

  const target = pickTarget();
  if (target) {
    const sep = target.input && !target.input.endsWith(' ') ? ' ' : '';
    updateSession(target.id, {
      input: target.input + sep + `@${mention.externalId} `,
      mentions: [...target.mentions, mention],
    });
    setActiveSessionInInstance(instanceId, target.id);
  }

  deps.clearAgentDragState();
  setDragPayload(null);
  settle();
}
