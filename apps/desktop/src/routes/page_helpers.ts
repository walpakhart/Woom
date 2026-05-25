// Pure helpers extracted from routes/+page.svelte in wave-1 phase-9
// refactor. Nothing here touches the reactive store, Tauri's invoke,
// or any module-level state — just small functions the route shell
// calls into. Lives next to `+page.svelte` because they're route-
// specific (not shared with components) but worth isolating for unit
// tests + readability.

/** Human-readable byte counter for cleanup-toast bodies. KB → MB → GB
 *  with one decimal on MB and two on GB. KB shown as integer so a 60
 *  KB cache doesn't read as `60.0 KB`. */
export function formatBytesShort(b: number): string {
  if (b < 1024 * 1024) return `${(b / 1024).toFixed(0)} KB`;
  if (b < 1024 * 1024 * 1024) return `${(b / 1024 / 1024).toFixed(1)} MB`;
  return `${(b / 1024 / 1024 / 1024).toFixed(2)} GB`;
}

/** Convert a Blob to a base64 string (without the `data:…;base64,`
 *  prefix). Used by the image-paste / drag-drop pipeline that needs to
 *  hand bytes to `fs_write_bytes` on the Rust side. */
export function blobToBase64(blob: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const r = new FileReader();
    r.onload = () => {
      const s = String(r.result ?? '');
      const i = s.indexOf(',');
      resolve(i >= 0 ? s.slice(i + 1) : s);
    };
    r.onerror = () => reject(r.error);
    r.readAsDataURL(blob);
  });
}

/** Default extension when the blob carries no source filename.
 *  Falls back to `png` for anything we don't explicitly know. */
export function guessExt(mime: string): string {
  if (mime.includes('jpeg')) return 'jpg';
  if (mime.includes('gif')) return 'gif';
  if (mime.includes('webp')) return 'webp';
  return 'png';
}

/** Pull image File blobs out of a DragEvent's dataTransfer.files.
 *  Used as a fallback for the Cmd+Shift+5 floating preview drag
 *  (which exposes Files but NO text/uri-list, so the OS-path branch
 *  misses it). Accepts anything whose MIME starts with `image/` OR
 *  whose extension matches the common bitmap formats. */
export function imageFilesFromEvent(
  e: DragEvent
): { name: string; type: string; blob: Blob }[] {
  const out: { name: string; type: string; blob: Blob }[] = [];
  const files = e.dataTransfer?.files;
  if (!files || files.length === 0) return out;
  for (let i = 0; i < files.length; i++) {
    const f = files[i];
    if (
      f &&
      (f.type.startsWith('image/') ||
        /\.(png|jpe?g|gif|webp|bmp|svg|heic|heif|avif)$/i.test(f.name))
    ) {
      out.push({ name: f.name, type: f.type || 'image/png', blob: f });
    }
  }
  return out;
}

/** Resolve the cwd the agent should adopt when the user picks a file
 *  vs a folder. Folders adopt as-is; files adopt their immediate
 *  parent dir. Returns null for paths with no parent (root-level
 *  files, malformed input) so the caller can skip the switch. */
export function deriveCwd(path: string, isDir: boolean): string | null {
  if (isDir) return path;
  const idx = path.lastIndexOf('/');
  return idx > 0 ? path.slice(0, idx) : null;
}

/** Group agent sessions by relative date for the soloAgent sidebar.
 *  Returns four buckets with non-empty contents only — Today /
 *  Yesterday / This week / Older. The "now" arg makes the result
 *  reactive when the parent ticks. Sessions with no messages yet
 *  bucket into "Older" so they don't pollute Today. */
export function groupAgentSessions<S extends { agentKind: string; messages: { at?: string }[] }>(
  sessions: readonly S[],
  kind: 'claude' | 'cursor',
  nowMs: number
): { label: string; items: S[] }[] {
  const items = sessions.filter((s) => s.agentKind === kind);
  const dayMs = 24 * 60 * 60 * 1000;
  const sessTime = (s: S): number => {
    const last = s.messages[s.messages.length - 1]?.at;
    return last ? new Date(last).getTime() : 0;
  };
  const sorted = [...items].sort((a, b) => sessTime(b) - sessTime(a));
  const today: S[] = [];
  const yesterday: S[] = [];
  const week: S[] = [];
  const older: S[] = [];
  for (const s of sorted) {
    const t = sessTime(s);
    if (t === 0) { older.push(s); continue; }
    const ageDays = Math.floor((nowMs - t) / dayMs);
    if (ageDays < 1) today.push(s);
    else if (ageDays < 2) yesterday.push(s);
    else if (ageDays < 7) week.push(s);
    else older.push(s);
  }
  return [
    { label: 'Today', items: today },
    { label: 'Yesterday', items: yesterday },
    { label: 'This week', items: week },
    { label: 'Older', items: older },
  ].filter((g) => g.items.length > 0);
}
