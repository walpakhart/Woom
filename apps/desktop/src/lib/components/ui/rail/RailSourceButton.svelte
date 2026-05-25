<script lang="ts">
  /* Singleton rail button for source/agent solos (Jira / GitHub / Sentry /
     Claude / Cursor). Each solo has exactly one instance — different
     contract from RailAppButton's multi-instance editor/canvas/terminal.
     Optional badge (unread count) and optional drag-drop (Claude/Cursor
     only). */
  import type { Snippet } from 'svelte';

  type DragKind = 'claude' | 'cursor';

  interface Props {
    /** True when this solo is the currently active top-level view. */
    active: boolean;
    label: string;
    /** Tooltip text — read by parent's `data-tooltip` delegation. */
    tooltip: string;
    /** Brand tone + glow CSS variables. */
    tone: string;
    glow: string;
    /** `data-view` value the parent's CSS uses to scope drag-pulse /
     *  active-halo selectors. */
    view: string;
    /** Unread count. 0 = no badge. >99 displays "99+". */
    badge?: number;
    /** Ambient "thinking" pulse — applied when the active agent
     *  session for this kind is mid-turn. Mutually exclusive with
     *  the drop-target pulse; the latter wins when both apply. */
    busy?: boolean;
    /** When set, treats this button as a drop target — registers
     *  drag listeners and accepts the same payloads the column-
     *  level drop targets handle. Only Claude/Cursor opt in. */
    dragKind?: DragKind;
    /** Highlight while a payload hovers — parent owns the state
     *  because dragleave/drop need to be coordinated across the
     *  rail (one source button per kind, the other one needs to
     *  drop the highlight). */
    dropOver?: boolean;
    onclick: () => void;
    onDragEnter?: (kind: DragKind, e: DragEvent) => void;
    onDragOver?: (kind: DragKind, e: DragEvent) => void;
    onDragLeave?: () => void;
    onDrop?: (kind: DragKind, e: DragEvent) => void;
    icon: Snippet;
  }
  let p: Props = $props();

  function badgeLabel(n: number): string {
    return n > 99 ? '99+' : String(n);
  }
</script>

<button
  class="rail-btn"
  class:active={p.active}
  class:rail-dropping={p.dropOver}
  class:rail-busy={p.busy && !p.dropOver}
  style="--rail-tone: {p.tone}; --rail-glow: {p.glow};"
  data-tooltip={p.tooltip}
  data-view={p.view}
  onclick={p.onclick}
  ondragenter={p.dragKind && p.onDragEnter ? (e) => p.onDragEnter?.(p.dragKind!, e) : null}
  ondragover={p.dragKind && p.onDragOver ? (e) => p.onDragOver?.(p.dragKind!, e) : null}
  ondragleave={p.dragKind && p.onDragLeave ? p.onDragLeave : null}
  ondrop={p.dragKind && p.onDrop ? (e) => p.onDrop?.(p.dragKind!, e) : null}
  aria-label={p.label}
>
  {@render p.icon()}
  {#if (p.badge ?? 0) > 0 && !p.active}
    <span class="rail-badge" aria-label="{p.badge} new in {p.label}">{badgeLabel(p.badge!)}</span>
  {/if}
</button>
