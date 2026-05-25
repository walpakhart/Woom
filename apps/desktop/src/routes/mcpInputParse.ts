// MCP tool-input parsers — extracted from the giant
// `handleAppNavigation` dispatcher in `+page.svelte` (wave-1 phase-9
// refactor). The agent CLI passes parameters as a free-form JSON
// object, and historically every consumer re-implemented the same
// "try a few alias spellings + drill into wrapper objects" dance.
// Centralising the helpers here means:
//
//   - The alias lists stay in one place (mirrored against the Rust
//     sidecar's `REPO_PATH_KEYS` / `INSTANCE_*_KEYS` constants —
//     both halves of the wire format MUST accept the same shapes).
//   - The dispatcher in `+page.svelte` shrinks from ~80 lines of
//     closure helpers to a few imports.
//   - The parsers are unit-testable in isolation (pure string-in
//     / object-in, string-out).

/** Trim+coerce a single key off the raw input map. Empty string when
 *  missing or non-string. */
export function str(input: Record<string, unknown>, key: string): string {
  return typeof input[key] === 'string' ? (input[key] as string).trim() : '';
}

/** Number coercion — passes raw numbers through, parses strings via
 *  `Number(...)`. Returns `NaN` when uncoercible; callers branch on
 *  `Number.isFinite`. */
export function num(input: Record<string, unknown>, key: string): number {
  const v = input[key];
  return typeof v === 'number' ? v : Number(v);
}

/** Return the first non-empty string value across the listed keys.
 *  Used by every dispatcher case that has to accept the multiple
 *  alias spellings the agent CLI ships ("repo_path" vs "repoPath"
 *  vs "folder" vs "path" …). */
export function pickFrom(
  obj: Record<string, unknown>,
  ...keys: string[]
): string {
  for (const k of keys) {
    const v = obj[k];
    if (typeof v === 'string' && v.trim()) return v.trim();
  }
  return '';
}

/** Cursor-agent has shipped the same field as a string, a single-
 *  element array, or even a wrapped object with an inner `path`/
 *  `value` key. We accept any of those shapes and return the first
 *  plausible non-empty string (or empty string when nothing resolves).
 *  Mirrors the sidecar's `coerce_to_string`. */
export function coerceString(v: unknown): string {
  if (typeof v === 'string') return v.trim();
  if (Array.isArray(v)) {
    for (const x of v) {
      const s = coerceString(x);
      if (s) return s;
    }
    return '';
  }
  if (v && typeof v === 'object') {
    const obj = v as Record<string, unknown>;
    for (const k of ['repo_path', 'path', 'folder', 'directory', 'dir', 'cwd', 'value', 'text', 'string']) {
      if (k in obj) {
        const s = coerceString(obj[k]);
        if (s) return s;
      }
    }
  }
  return '';
}

/** Alias-aware deep extractor — drills into the wrapper objects
 *  cursor-agent / claude have been known to nest payloads under
 *  (`args` / `arguments` / `params` / `input`). Walks up to depth 4
 *  to cover the `{"args":{"args":{...}}}` case we've seen in the
 *  wild. Returns empty string when nothing in the alias list
 *  resolves. */
export function pickDeep(
  obj: Record<string, unknown> | null | undefined,
  keys: string[],
  depth = 4
): string {
  if (!obj || typeof obj !== 'object' || depth === 0) return '';
  for (const k of keys) {
    if (k in obj) {
      const s = coerceString(obj[k]);
      if (s) return s;
    }
  }
  for (const wrap of ['args', 'arguments', 'params', 'parameters', 'input', 'data', 'payload']) {
    const inner = obj[wrap];
    if (inner && typeof inner === 'object' && !Array.isArray(inner)) {
      const s = pickDeep(inner as Record<string, unknown>, keys, depth - 1);
      if (s) return s;
    }
  }
  return '';
}

/* Canonical alias lists for the deep extractors — kept in sync with
 * the sidecar's `REPO_PATH_KEYS` / `INSTANCE_NAME_KEYS` /
 * `INSTANCE_ID_KEYS` so both halves of the round-trip recognise the
 * same payload shapes. */
export const REPO_PATH_KEYS_DEEP = [
  'repo_path', 'repoPath', 'path', 'folder', 'directory', 'dir',
  'cwd', 'repo', 'repository_path', 'folderPath', 'dirPath',
  'fullPath', 'absolutePath', 'target_path', 'target'
];

export const INSTANCE_NAME_KEYS_DEEP = [
  'instance_name', 'instanceName', 'name', 'column_name', 'columnName',
  'editor_name', 'agent_name', 'label'
];

export const INSTANCE_ID_KEYS_DEEP = [
  'instance_id', 'instanceId', 'id', 'column_id', 'columnId',
  'editor_id', 'agent_id', 'uuid'
];
