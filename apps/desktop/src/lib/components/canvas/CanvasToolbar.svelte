<script lang="ts">
  // Compact tool strip on the left side of the canvas surface. Each
  // button activates a tool; the active tool is what `setTool` records
  // for the column instance. Tool kinds match `CanvasTool` in
  // canvas.svelte.ts; adding a new tool = update both.
  //
  // Keyboard shortcuts are listed in tooltips and handled by
  // CanvasSurface's window keydown listener — this component just
  // surfaces them visually.

  import type { CanvasTool } from '$lib/state/canvas.svelte';
  import type { LayoutAlgorithm } from '$lib/services/canvasLayout';

  interface Props {
    tool: CanvasTool;
    onSelect: (tool: CanvasTool) => void;
    onUndo: () => void;
    onRedo: () => void;
    canUndo: boolean;
    canRedo: boolean;
    onLayout: (algo: LayoutAlgorithm) => void;
  }

  let { tool, onSelect, onUndo, onRedo, canUndo, canRedo, onLayout }: Props = $props();

  type Btn = { tool: CanvasTool; key: string; label: string; icon: string };
  /* SVG path strings — single-color, currentColor stroke, 24×24 viewBox.
     Drawn with stroke-only so they re-tint via .cv-tool-btn color. */
  const buttons: Btn[] = [
    { tool: 'select',   key: 'V', label: 'Select',     icon: 'M4 4l16 6-7 2-2 7-7-15z' },
    { tool: 'rect',     key: 'R', label: 'Rectangle',  icon: 'M4 5h16v14H4z' },
    { tool: 'ellipse',  key: 'O', label: 'Ellipse',    icon: 'M12 5a8 7 0 1 0 0 14 8 7 0 1 0 0-14z' },
    { tool: 'line',     key: 'L', label: 'Line',       icon: 'M4 20 20 4' },
    { tool: 'arrow',    key: 'A', label: 'Arrow',      icon: 'M4 20 20 4M14 4h6v6' },
    { tool: 'text',     key: 'T', label: 'Text',       icon: 'M5 6h14M12 6v14M9 20h6' },
    { tool: 'sticky',   key: 'S', label: 'Sticky',     icon: 'M5 5h11l4 4v10H5zM16 5v4h4' },
    { tool: 'frame',    key: 'F', label: 'Frame',      icon: 'M4 8V4h4M16 4h4v4M20 16v4h-4M8 20H4v-4' },
    { tool: 'freehand', key: 'P', label: 'Pencil',     icon: 'M3 21l4-1L20 7l-3-3L4 17l-1 4zM14 7l3 3' },
    { tool: 'mermaid',  key: 'M', label: 'Mermaid diagram', icon: 'M4 18 9 8l3 6 3-4 5 8M4 18h16' },
    { tool: 'code',     key: 'C', label: 'Code block', icon: 'M9 8 4 12l5 4M15 8l5 4-5 4M14 6l-4 12' },
    { tool: 'image',    key: 'I', label: 'Image (paste, click to pick)', icon: 'M4 5h16v14H4zM4 17l5-5 4 4 3-3 4 4M9 10a1.5 1.5 0 1 0 0-3 1.5 1.5 0 0 0 0 3z' }
  ];
</script>

<!-- Stop pointerdown bubbling so a click on a toolbar button doesn't
     trigger the canvas surface's marquee gesture (which would
     setPointerCapture and prevent the button's `click` from firing).
     Same for wheel — scrolling inside the toolbar shouldn't pan/zoom
     the canvas underneath. -->
<div
  class="cv-toolbar"
  role="toolbar"
  aria-label="Canvas tools"
  tabindex="-1"
  onpointerdown={(e) => e.stopPropagation()}
  onwheel={(e) => e.stopPropagation()}
>
  {#each buttons as b (b.tool)}
    <button
      class="cv-tool-btn"
      class:active={tool === b.tool}
      onclick={() => onSelect(b.tool)}
      title={`${b.label} (${b.key})`}
      aria-label={b.label}
      aria-pressed={tool === b.tool}
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">
        <path d={b.icon} />
      </svg>
    </button>
  {/each}

  <div class="cv-toolbar-divider"></div>

  <button
    class="cv-tool-btn"
    onclick={onUndo}
    disabled={!canUndo}
    title="Undo (⌘Z)"
    aria-label="Undo"
  >
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">
      <path d="M9 14 4 9l5-5"/><path d="M4 9h11a5 5 0 0 1 0 10h-3"/>
    </svg>
  </button>
  <button
    class="cv-tool-btn"
    onclick={onRedo}
    disabled={!canRedo}
    title="Redo (⇧⌘Z)"
    aria-label="Redo"
  >
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">
      <path d="M15 14l5-5-5-5"/><path d="M20 9H9a5 5 0 0 0 0 10h3"/>
    </svg>
  </button>

  <div class="cv-toolbar-divider"></div>

  <!-- Layout actions — applied to current selection if any, else to all
       root-level shapes. Each button is a one-shot; no active state since
       layouts aren't a "mode". -->
  <button
    class="cv-tool-btn"
    onclick={() => onLayout('grid')}
    title="Lay out as grid"
    aria-label="Grid layout"
  >
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">
      <rect x="4" y="4" width="6" height="6"/><rect x="14" y="4" width="6" height="6"/><rect x="4" y="14" width="6" height="6"/><rect x="14" y="14" width="6" height="6"/>
    </svg>
  </button>
  <button
    class="cv-tool-btn"
    onclick={() => onLayout('row')}
    title="Lay out in a row"
    aria-label="Row layout"
  >
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">
      <rect x="3" y="9" width="5" height="6"/><rect x="10" y="9" width="5" height="6"/><rect x="17" y="9" width="4" height="6"/>
    </svg>
  </button>
  <button
    class="cv-tool-btn"
    onclick={() => onLayout('column')}
    title="Lay out in a column"
    aria-label="Column layout"
  >
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">
      <rect x="9" y="3" width="6" height="5"/><rect x="9" y="10" width="6" height="5"/><rect x="9" y="17" width="6" height="4"/>
    </svg>
  </button>
  <button
    class="cv-tool-btn"
    onclick={() => onLayout('dagre')}
    title="DAG layout (Sugiyama)"
    aria-label="Dagre layout"
  >
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">
      <rect x="3" y="9" width="5" height="6"/><rect x="10" y="3" width="5" height="6"/><rect x="10" y="15" width="5" height="6"/><rect x="17" y="9" width="4" height="6"/><path d="M8 12h2M15 6h2M15 18h2"/>
    </svg>
  </button>
</div>

<style>
  .cv-toolbar {
    position: absolute;
    left: 10px;
    top: 10px;
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 4px;
    background: var(--bg-1);
    border: 1px solid var(--border-neutral);
    border-radius: 8px;
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.35);
    z-index: 4;
    backdrop-filter: blur(6px);
  }
  .cv-tool-btn {
    width: 30px;
    height: 30px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background: transparent;
    border: 1px solid transparent;
    border-radius: 6px;
    color: var(--text-1);
    cursor: pointer;
    transition: background 100ms, color 100ms, border-color 100ms;
  }
  .cv-tool-btn:hover:not(:disabled) {
    background: var(--bg-2);
    color: var(--text-0);
  }
  .cv-tool-btn.active {
    background: var(--accent-soft, rgba(232, 130, 100, 0.18));
    color: var(--accent-bright, var(--accent));
    border-color: var(--accent);
  }
  .cv-tool-btn:disabled {
    opacity: 0.35;
    cursor: not-allowed;
  }
  .cv-tool-btn svg {
    width: 16px;
    height: 16px;
  }
  .cv-toolbar-divider {
    height: 1px;
    margin: 4px 2px;
    background: var(--border-neutral);
  }
</style>
