<script lang="ts">
  /* SddAmendPanel — inline textarea where the user describes a change
     to the CURRENT spec / plan / phase. Extracted from SddCard in
     wave-13 split. On Cmd/Ctrl+↵ fires `onSend` (parent builds the
     amend prompt + dispatches); Esc cancels.

     Pure presentational — parent owns `amendMode` toggle + the
     `sendAmend` / `cancelAmend` callbacks. */
  interface Props {
    /** Workspace root path — shown in the hint copy as
     *  "Agent will edit `<basename>` in place". */
    workspaceRoot: string;
    /** Draft bound to the textarea. Parent reads on submit. */
    draft: string;
    onSend: () => void | Promise<void>;
    onCancel: () => void;
  }
  let { workspaceRoot, draft = $bindable(''), onSend, onCancel }: Props = $props();

  const rootName = $derived(workspaceRoot.split('/').pop() ?? workspaceRoot);
</script>

<div class="sdd-amend">
  <label class="sdd-amend-label">
    <span class="sdd-amend-hint">
      Describe the change. Agent will edit `{rootName}` in place — spec / plan / current phase — instead of starting over.
    </span>
    <textarea
      class="sdd-amend-area mono"
      bind:value={draft}
      placeholder="e.g. drop phase 4, retitle phase 2 to “Combat”, replace Unity with Godot, add an audio router task…"
      rows="4"
      spellcheck="false"
      {@attach (node: HTMLTextAreaElement) => node.focus()}
      onkeydown={(e) => {
        if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') { e.preventDefault(); void onSend(); }
        if (e.key === 'Escape') { e.preventDefault(); onCancel(); }
      }}
    ></textarea>
  </label>
</div>

<style>
  .sdd-amend {
    margin-top: 4px;
    padding: 6px 0 4px 12px;
    border-left: 2px solid color-mix(in srgb, var(--accent) 35%, transparent);
  }
  .sdd-amend-label { display: flex; flex-direction: column; gap: 4px; }
  .sdd-amend-hint {
    font-size: 11px;
    color: var(--text-mute);
    line-height: 1.45;
  }
  .sdd-amend-area {
    width: 100%;
    padding: 6px 8px;
    border: 1px solid var(--border-neutral-hi);
    border-radius: 4px;
    background: color-mix(in srgb, var(--bg-0) 70%, transparent);
    color: var(--text-0);
    font-family: 'JetBrains Mono', monospace;
    font-size: 12.5px;
    line-height: 1.55;
    resize: vertical;
    outline: 0;
  }
  .sdd-amend-area:focus { border-color: var(--accent); }
</style>
