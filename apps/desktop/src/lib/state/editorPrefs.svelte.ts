/* Editor-pane preferences. Currently:
 *   - `autosave` — bool, defaults true (legacy behaviour).
 *     When false, the editor only persists on explicit Mod-S; the
 *     600 ms idle save timer is dormant. Added because the autosave
 *     was firing fast enough to (a) snap the change-bar diff under
 *     the caret on mid-word stops and (b) periodically reset the
 *     CodeMirror undo stack when the parent reacted to `onSaved` by
 *     re-running an effect that re-rendered the surrounding tree.
 *     Users who want lossless undo + manual save now have a knob.
 *
 * Persisted to localStorage under `woom:editor-prefs:v1` so the
 * choice survives reload. No SSR concerns — the editor only mounts
 * client-side.
 */

const KEY = 'woom:editor-prefs:v1';

export interface EditorPrefs {
  autosave: boolean;
}

const DEFAULT: EditorPrefs = { autosave: true };

function readPersisted(): EditorPrefs {
  try {
    const raw = typeof localStorage !== 'undefined' ? localStorage.getItem(KEY) : null;
    if (!raw) return { ...DEFAULT };
    const parsed = JSON.parse(raw) as Partial<EditorPrefs>;
    return {
      autosave: typeof parsed.autosave === 'boolean' ? parsed.autosave : DEFAULT.autosave,
    };
  } catch {
    return { ...DEFAULT };
  }
}

export const editorPrefs = $state<EditorPrefs>(readPersisted());

/** Update + persist. Use this instead of mutating the proxy
 *  directly so the localStorage write is always paired with the
 *  reactive update. */
export function setEditorPrefs(patch: Partial<EditorPrefs>): void {
  Object.assign(editorPrefs, patch);
  try {
    localStorage.setItem(KEY, JSON.stringify({ autosave: editorPrefs.autosave }));
  } catch {
    /* localStorage may throw in private/quota-exceeded; the in-
     * memory state still updates so the current session honours
     * the choice. */
  }
}
