<script lang="ts">
  import { editorPrefs, setEditorPrefs } from '$lib/state/editorPrefs.svelte';
</script>

<!-- Editor preferences -->
<div class="card">
  <header class="card-head">
    <h2 class="card-title">Editor</h2>
    <p class="card-sub">
      Behaviour for the inline code editor (right-pane buffers + diff
      review surfaces).
    </p>
  </header>

  <label class="toggle-row">
    <input
      type="checkbox"
      checked={editorPrefs.autosave}
      onchange={(e) => setEditorPrefs({ autosave: (e.currentTarget as HTMLInputElement).checked })}
    />
    <span class="toggle-text">
      <span class="toggle-label">Autosave</span>
      <span class="toggle-sub">
        Write the buffer to disk 600 ms after the last keystroke.
        Disable to use the editor in a "modify freely, save with ⌘S"
        flow — keeps the CodeMirror undo stack intact across long edit
        sessions, and stops the change-bar diff from snapping under
        the caret on mid-word pauses. Manual ⌘S still works either
        way.
      </span>
    </span>
  </label>
</div>

<style>
  .toggle-row {
    display: flex;
    align-items: flex-start;
    gap: 12px;
    padding: 4px 0;
    cursor: pointer;
  }
  .toggle-row input {
    margin-top: 3px;
    accent-color: var(--accent);
  }
  .toggle-text {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .toggle-label {
    font-size: 13px;
    font-weight: 500;
    color: var(--text-0);
  }
  .toggle-sub {
    font-size: 12px;
    color: var(--text-mute);
    line-height: 1.5;
  }
</style>
