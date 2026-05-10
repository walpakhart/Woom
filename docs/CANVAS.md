# Woom — Canvas (Whiteboard) Specification

**Version:** 0.1 (draft)
**Last updated:** 2026-04-28
**Status:** spec only — no code yet. Targets a post-v0.1 milestone
(`M-canvas-*`). v0.1 ships without Canvas; this document pins the
shape of it before any pixel is committed.

> A second-class workbench surface where Claude / Cursor agents and the
> user share a 2D plane and put things on it: diagrams, sketches,
> live cards (Jira / PR / Sentry / files), code snippets, freehand
> notes. The agent **sees** the canvas as both a JSON scene graph
> (precise) and a screenshot (visual), and **acts** on it through
> MCP tools — placing, moving, sizing, connecting, laying out shapes
> with pixel precision. The user manipulates the same plane directly.

---

## 1. Vision & Non-Goals

### 1.1 Vision

Today the agent communicates through a chat column: text, code blocks,
diff cards. That is fine for "do this thing", but a category of work is
inherently spatial — system architecture, dependency graphs, sequence
of events across services, post-mortem timelines, sketch of a UI,
"here's how these tickets relate to that PR". The chat column is the
wrong shape for it.

Canvas adds a **drawable, infinite, persistent 2D plane per workbench**
that:

1. The user can draw on (cards, arrows, freehand, mermaid blocks, code).
2. The agent can read and edit programmatically through MCP tools, with
   exact `(x, y, w, h)` placement in canvas pixels.
3. Stays alive across sessions — every canvas has an id, a name, a
   thumbnail; you can keep many canvases side-by-side, switch, archive,
   resume.
4. Connects to the rest of Forge: drag a Jira ticket from `JiraColumn`
   onto a canvas → it lands as a **live card** that auto-refreshes from
   the API, like the inbox version. Drop a file from the editor's file
   tree → file card with click-to-open.

### 1.2 Goals (v1)

1. Agent can `read` the entire canvas in two ways: as JSON (full state),
   and as a PNG screenshot for visual reasoning.
2. Agent can `write` shapes, edges, groups, and trigger layouts —
   all coordinates in canvas pixels, sub-pixel resolution allowed.
3. Pixel-precise alignment helpers built in (snap-to-grid, snap-to-edge,
   smart guides, distribute, align), exposed both to user (UI) and agent
   (tool calls).
4. Multi-canvas: a library of canvases per workbench, create / open /
   rename / archive / duplicate / export.
5. Live integration with existing Forge objects: drag-from-other-columns
   produces live cards that re-render when the source updates.
6. Agent ↔ canvas link is per-session, set by drag (same UX as the
   editor link). A linked agent's MCP tools target that canvas.
7. Drawing surface feels native — 60 fps pan/zoom on Apple Silicon, no
   stutter at 2000 shapes.
8. Clean serialization (one JSON file per canvas) so canvases survive
   crash, can be diffed in git, and round-trip perfectly through the
   agent's tools.

### 1.3 Non-Goals (v1)

- **Real-time multi-user collaboration.** Canvas is single-player in v1
  exactly like the rest of Forge. Workspace sharing / CRDT is post-v0.3
  alongside the team layer.
- **Vector tools beyond what the agent needs.** No bezier handles, no
  pen tool with anchor editing, no boolean ops. We give: rectangles,
  ellipses, arrows, text, sticky notes, freehand strokes,
  rendered-from-source diagrams (Mermaid / DOT / PlantUML), and Forge
  live cards. That's the full v1 catalog.
- **Image editing.** Pasted images are placed and resized but not
  cropped, masked, or filtered.
- **Animation / motion.** Static frames only.
- **Plugin system** for new shape kinds. The catalog is closed in v1;
  we add kinds by code change, not by extension.
- **Slideshow / present mode.** Maybe later, but not v1.

---

## 2. Core Concepts

Three entities make up a canvas. Mirrors how the rest of Forge models
things — flat, plain, no inheritance.

### 2.1 Canvas

Top-level container. One Canvas = one drawable plane = one JSON file on
disk. A workbench can hold one Canvas column instance per Canvas (via
`PanelKind = 'canvas'`), and a column can switch between canvases via
its tab bar.

```ts
type Canvas = {
  id: string;                       // uuid
  name: string;                     // user-visible, free text
  createdAt: number;                // unix ms
  updatedAt: number;                // unix ms, bumped on autosave
  archivedAt: number | null;        // null = active
  thumbnail: string | null;         // 256×160 PNG dataURL, regenerated on save
  background: 'dot' | 'grid' | 'plain';
  gridSize: number;                 // canvas px between grid lines, default 8
  viewport: { x: number; y: number; zoom: number };  // last user view
  shapes: Shape[];                  // flat list, parent linkage by parentId
  edges: Edge[];                    // flat list
  /** Bumped by every applied operation (user or agent) so the canvas
   *  store can detect external changes and the agent can detect that
   *  its read-state is stale. */
  version: number;
};
```

### 2.2 Shape

The thing on the canvas. Common envelope, kind-specific `props`. Every
shape is rectangular in its bounding box (`x, y, w, h`) regardless of
visual form — even a freehand stroke and an ellipse have a bounding box;
that's what selection / snapping / layout reason about.

```ts
type Shape = {
  id: string;                       // uuid
  kind: ShapeKind;                  // discriminator
  x: number;                        // canvas px, top-left of bbox
  y: number;
  w: number;                        // canvas px, > 0
  h: number;                        // canvas px, > 0
  rot: number;                      // radians around bbox center, 0 = upright
  z: number;                        // monotonic, higher = on top
  parentId: string | null;          // group / frame parent, null = root
  locked: boolean;                  // user/agent edits ignored when true
  hidden: boolean;                  // drawn off-screen, still in serialization
  label: string | null;             // a11y label, surfaces on hover
  props: ShapeProps[Kind];          // kind-specific, see §5
  createdAt: number;
  createdBy: 'user' | 'agent';
  updatedAt: number;
};
```

### 2.3 Edge

A connector between two shapes. Lives in `canvas.edges` (not in `shapes`)
so layout algorithms can reason about graph structure cheaply. Anchors
point at one of nine canonical positions on a shape (`tl`, `tc`, `tr`,
`ml`, `mc`, `mr`, `bl`, `bc`, `br`) or at a free `(dx, dy)` offset
relative to the shape's bbox top-left.

```ts
type EdgeAnchor =
  | { shapeId: string; anchor: 'tl'|'tc'|'tr'|'ml'|'mc'|'mr'|'bl'|'bc'|'br' }
  | { shapeId: string; offset: { dx: number; dy: number } };

type Edge = {
  id: string;
  from: EdgeAnchor;
  to: EdgeAnchor;
  kind: 'arrow' | 'line' | 'dashed';   // visual style
  routing: 'straight' | 'orthogonal' | 'curved';
  label: string | null;                // mid-line label
  color: string | null;                // null = themed default
  thickness: 1 | 2 | 3;
  z: number;
};
```

Edges with a missing endpoint shape are kept (they may be re-resolved
when the shape is restored from undo) but rendered as a dashed ghost
with a warning glyph.

---

## 3. Multi-Canvas Library

A workbench owns **a library of canvases**, accessed from any Canvas
column instance. The user starts with zero canvases; clicking the
"+ canvas" button in the column header creates one and opens it.

### 3.1 Where canvases live

`localStorage` does not scale here (a 1500-shape canvas can be 2 MB JSON
+ inlined screenshots). We persist to disk:

```
~/Library/Application Support/Woom/
  canvases/
    <canvas-id>.json          # full Canvas record
    <canvas-id>.thumb.png     # 256×160 thumbnail
    <canvas-id>.assets/       # pasted images, agent-uploaded PNGs
      <asset-id>.png
```

`localStorage` keeps a tiny **library index** so the app boots without
reading every JSON file:

```ts
type CanvasIndexEntry = {
  id: string;
  name: string;
  updatedAt: number;
  archivedAt: number | null;
  shapeCount: number;
  thumbnailPath: string;      // resolved via convertFileSrc
  workbenchId: string;        // home workbench (where it was created)
};
type CanvasIndex = { entries: CanvasIndexEntry[] };
```

Persisted under `woom:canvas:index:v1`. On startup we read this
index and lazy-load full JSON only when a canvas is opened in a column.

### 3.2 Canvas column header

```
┌──────────────────────────────────────────────────────────────────┐
│ ▣ Canvas (Mona-Lisa)   ▾  [Architecture v3]  [Postmortem]  +     │
│                          ^^^^^^^^^^^^^^^^^^^                     │
│                          active tab — bold, accent underline     │
└──────────────────────────────────────────────────────────────────┘
```

- The pill `▣ Canvas (Mona-Lisa)` follows the existing instance-naming
  convention (see [layout.svelte.ts](../apps/desktop/src/lib/state/layout.svelte.ts)).
- A horizontal scrollable strip of **canvas tabs** lives next to it —
  one tab per canvas pinned to this column. Click switches; ⌘-click
  duplicates; right-click menu offers rename / archive / export /
  duplicate / detach (move tab to another column).
- `+` opens a new untitled canvas.
- Dropdown `▾` opens the **library overlay** — full grid of every
  canvas (active + archived) with thumbnails. Same affordance as
  raycast-style command palette: `⌘P` while focused on a Canvas column
  surfaces it.

### 3.3 Library overlay (full grid)

Triggered by the column header dropdown or `⌘P` while focus is on a
Canvas column.

```
╔════════════════════════════════════════════════════════════════════╗
║  Canvases                                  [+ New]   [⌕ search ]   ║
╠════════════════════════════════════════════════════════════════════╣
║  Active                                                            ║
║                                                                    ║
║  ┌──────┐  ┌──────┐  ┌──────┐  ┌──────┐                            ║
║  │ thmb │  │ thmb │  │ thmb │  │ thmb │     thumbnail tiles        ║
║  │      │  │      │  │      │  │      │     256×160                ║
║  └──────┘  └──────┘  └──────┘  └──────┘                            ║
║   Arch v3   Backup    Onboard   Post-                              ║
║   2h ago    1d ago    3d ago    mortem                             ║
║                                                                    ║
║  Archived                                                          ║
║  ┌──────┐  ┌──────┐                                                ║
║  │ thmb │  │ thmb │                                                ║
║  └──────┘  └──────┘                                                ║
╚════════════════════════════════════════════════════════════════════╝
```

Click a tile to open it in the current Canvas column. Hover shows
shape-count, last edited, agent activity badge.

### 3.4 Lifecycle

| Action       | Effect                                                                     |
|--------------|----------------------------------------------------------------------------|
| Create       | New uuid, blank canvas, "Untitled (Mona-Lisa)" name, opens in current col. |
| Open         | Load JSON from disk, hydrate, attach to column tab.                        |
| Rename       | Update `name`, persist index + JSON.                                       |
| Archive      | Set `archivedAt`, drop from active tabs, keep on disk.                     |
| Unarchive    | Clear `archivedAt`.                                                        |
| Duplicate    | Deep-clone JSON + assets, new uuid, open new tab.                          |
| Export       | Write JSON + thumbnail to a user-picked folder.                            |
| Import       | Pick a JSON file, validate schema, mint new uuid (avoid id collisions).    |
| Delete       | Hard-delete JSON + assets + index entry. Confirm modal, no undo.           |

Autosave is **debounced 1.5s** after the last mutation, plus an
immediate save on `blur` of the app window.

---

## 4. Coordinate System

The user said: "распологать как-то по пикселям внутренним". Here is what
"internal pixel" means and what guarantees it gives the agent.

### 4.1 Units

- **Canvas pixel** is the only unit. It is a logical pixel — independent
  of device DPI, independent of zoom level.
- All shape coordinates, sizes, edge offsets, freehand stroke points,
  layout outputs, and agent tool inputs / outputs use canvas pixels.
- Coordinates are 64-bit floats; sub-pixel resolution is allowed.
- `zoom` in `viewport` is a multiplier from canvas pixels to CSS pixels.
  At zoom = 1, one canvas px = one CSS px = one device pixel at 1x DPR
  (or 2 device pixels at 2x DPR — this happens at the renderer level,
  the data model never sees DPR).

### 4.2 Origin and bounds

- `(0, 0)` is the canvas origin. We render an infinite plane (no clamp).
- Positions can be negative.
- The canvas has a soft "starting region" at `[-2000, -2000, 4000, 4000]`
  used for fit-to-content when the canvas is empty.

### 4.3 Grid

- Default grid step is 8 canvas px (matches the design system's 4px /
  8px spacing scale from [UI.md §2.3](UI.md)).
- Rendered as dots (default) or lines (`background: 'grid'`).
- Major grid mark every 8 grid steps (64 px) with brighter dot.
- The grid step is per-canvas so a presentation canvas can use 16, a
  schematic 4.

### 4.4 Snapping

Three snap regimes, all default-on, each toggleable per-drag with
modifier keys:

1. **Grid snap** — round to nearest `gridSize` on translate / resize.
   Modifier: hold `⌘` to ignore grid.
2. **Edge snap** — when a shape's edge or center comes within 4 px of
   another shape's edge or center, lock to it. Smart guide line
   visualizes the alignment. Modifier: hold `⌥` to ignore edge snap.
3. **Distance snap** — when moving a shape between two others, equal
   spacing within 4 px tolerance produces a "12 ↔ 12" badge and locks.

These regimes are also exposed to the agent — every `move` / `resize`
tool takes an optional `snap: 'grid' | 'edge' | 'distance' | 'none' |
'all'` parameter. Default `'all'`.

### 4.5 What "красиво расставлять" means concretely

When the agent calls `canvas.add_shape({ x, y, w, h, … })` directly, it
gets exactly what it asked for. When it wants the canvas to do the
hard alignment work, it has three escalations:

1. **Snap on insert** — pass `snap: 'all'` and the shape is nudged onto
   grid + aligns to nearest neighbor's edge / center within tolerance.
   Cheap, local.
2. **Layout helper** — `canvas.arrange({ algorithm, … })`. Built-ins:
   `grid`, `row`, `column`, `dagre` (for DAGs over the edge graph),
   `force` (for unstructured graphs), `tree` (rooted at a shape),
   `radial`. Every algorithm rounds final positions to `gridSize`.
3. **Free-form math** — agent reads the scene graph, computes positions
   itself, writes them back. Useful when the layout is novel (a
   timeline aligned to event timestamps, a swimlane, etc).

The renderer also draws **alignment guides** (red 1px lines) during
agent batch insertion so the user sees the agent's reasoning visually.

---

## 5. Shape Catalog

`ShapeKind` is closed. Adding a kind = code change, schema migration,
serializer update, agent-tool entry. Keep it small.

```ts
type ShapeKind =
  // primitive geometry
  | 'rect' | 'ellipse' | 'arrow-shape' | 'line' | 'text' | 'sticky'
  // rich
  | 'mermaid' | 'dot' | 'plantuml' | 'code' | 'image' | 'freehand'
  // structural
  | 'frame' | 'group'
  // forge live cards (drop-zone produced)
  | 'jira-card' | 'github-pr-card' | 'github-issue-card'
  | 'sentry-event-card' | 'file-card' | 'chat-message-card';
```

Each kind defines its `props` shape. Below: the contract per kind.

### 5.1 Primitive geometry

#### `rect`
```ts
{ fill: string|null; stroke: string|null; strokeWidth: 1|2|3; radius: number; }
```
Default radius = 6 (chip), 10 (card), 14 (panel) per UI.md §2.3 — exposed
as named tokens to the agent (`radius: 'chip' | 'card' | 'panel' | <num>`).

#### `ellipse`
```ts
{ fill: string|null; stroke: string|null; strokeWidth: 1|2|3; }
```

#### `arrow-shape` (a free-standing arrow, not an Edge)
```ts
{ from: {x,y}; to: {x,y}; head: 'none'|'open'|'filled'; thickness: 1|2|3; }
```
Used when the user drags an arrow without snapping endpoints to shapes.
If both endpoints later attach to shapes, the renderer auto-promotes
this to an `Edge` (asks the user for confirmation on the first
auto-promote).

#### `line`
```ts
{ from: {x,y}; to: {x,y}; thickness: 1|2|3; dash: 'solid'|'dashed'|'dotted'; }
```

#### `text`
```ts
{ text: string; fontSize: number; fontWeight: 400|500|600|700;
  align: 'left'|'center'|'right'; color: string|null; }
```
Auto-wraps inside `(w, h)`. `\n` for hard breaks. Markdown is **not**
rendered here — use `sticky` for that.

#### `sticky`
```ts
{ markdown: string; tint: 'yellow'|'pink'|'blue'|'green'|'gray'|'forge';
  fontSize: number; }
```
Markdown body renders via the same [Markdown.svelte](../apps/desktop/src/lib/components/ui/Markdown.svelte)
the chat column uses. Sticky is the agent's default choice for
"explanatory text on the canvas".

### 5.2 Rich / rendered

#### `mermaid`
```ts
{ source: string; theme: 'forge-dark' | 'mermaid-default'; }
```
Renders the Mermaid SVG into the bbox. Updates re-render. Errors
display the raw source + a 1-line error badge. Theme `forge-dark` is
shipped with the app: warm-orange primary, bg-1 surface.

#### `dot` (Graphviz DOT)
```ts
{ source: string; engine: 'dot'|'neato'|'fdp'|'circo'; }
```
Rendered via [@hpcc-js/wasm-graphviz](https://github.com/hpcc-systems/hpcc-js-wasm)
(same family as the existing CodeMirror lang autoload).

#### `plantuml`
PlantUML needs Java to render server-side, so v1 ships **read-only**:
the `source` is stored, and we render via a local-only WASM port if /
when one exists (kroki has none today). For v1 we degrade to "show the
source as text and the agent gets the source on read". Re-evaluate
post-v1.

#### `code`
```ts
{ source: string; language: string; theme: string;
  lineNumbers: boolean; highlight: number[]; }  // highlight = line numbers
```
Renders via the existing CodeMirror 6 stack ([codemirrorLang.ts](../apps/desktop/src/lib/components/editor/codemirrorLang.ts)).
Read-only by default; double-click to edit inline.

#### `image`
```ts
{ assetPath: string;          // path inside <canvas-id>.assets/
  intrinsicWidth: number;
  intrinsicHeight: number;
  alt: string|null; }
```
Source on disk; the bbox is independent of intrinsic size (free resize).
Agent can produce images via `canvas.upload_image(base64)` — see §10.5.

#### `freehand`
```ts
{ points: Array<[number, number, number]>;  // [x, y, pressure]
  color: string;
  thickness: 1|2|3;
  smoothing: number;          // 0..1, perfect-freehand parameter
}
```
Pressure curve via [perfect-freehand](https://github.com/steveruizok/perfect-freehand).
Bbox is computed from points on commit and on edit.

### 5.3 Structural

#### `frame`
A labeled rectangular region. Children whose `parentId` === frame's id
move with it. Frames clip nothing visually but are first-class for
selection (`⌥-click` selects through to the child, plain click selects
the frame).
```ts
{ title: string; tint: string|null; }
```

#### `group`
Like a frame but with no visual border / title. Pure logical grouping
for batch transforms.
```ts
{}
```

### 5.4 Forge live cards

These are **projections of Forge inbox objects** — same data the
[GithubColumn](../apps/desktop/src/lib/components/workbench/GithubColumn.svelte)
and [JiraColumn](../apps/desktop/src/lib/components/workbench/JiraColumn.svelte)
render, just placed on the canvas. They auto-refresh on the same poll
cadence as their home columns. Click navigates back to that column with
the object focused.

#### `jira-card`
```ts
{ ticketKey: string;          // "PROJ-1234"
  // rendered fields auto-pulled — title, status, assignee, etc.
}
```

#### `github-pr-card`
```ts
{ owner: string; repo: string; number: number; }
```

#### `github-issue-card`
```ts
{ owner: string; repo: string; number: number; }
```

#### `sentry-event-card`
```ts
{ projectSlug: string; eventId: string; }
```

#### `file-card`
A reference to a file or folder in the user's open repo. Used for
"this is the file my arrow is pointing at".
```ts
{ repoRoot: string; relPath: string; isDir: boolean; }
```
Click opens in an Editor column instance (linked or floating).

#### `chat-message-card`
A pinned chat message from a Claude / Cursor session. The card holds a
copy of the message content + a back-pointer to the session it came
from. Useful for "this is what the agent decided at step 3".
```ts
{ sessionId: string; messageIndex: number;
  // copy of message.content and usage badge for instant render
}
```

---

## 6. Edges & Connectors

§2.3 defines the schema. UI rules:

- Drag from the small "+" anchor that appears on a shape's edge mid-points
  on hover → drag to another shape → edge snaps to the target shape's
  nearest anchor.
- An edge attached to two shapes follows both as they move (positions
  recompute every frame).
- Routing:
  - `straight` — two-point line.
  - `orthogonal` — Manhattan; routed around bboxes via simple A* on a
    coarse grid (`gridSize * 4`). Cached per (from, to, layout-version).
  - `curved` — Catmull-Rom through computed waypoints.
- Default routing is `orthogonal` for `arrow`, `straight` for `line`,
  `curved` for `dashed`.
- Edge head styles match the global UI accents — never use raw black.

---

## 7. Layout Algorithms

Built-in, deterministic, exposed both as toolbar buttons and agent
tools. Every algorithm gets a **selection** (or "all root shapes" if
no selection) and produces new positions. Animations use the `gentle`
spring preset from [UI.md §2.5](UI.md).

| Algorithm  | Inputs                            | Best for                          |
|------------|-----------------------------------|-----------------------------------|
| `grid`     | cols, gap                         | dump-and-arrange a pile of cards  |
| `row`      | gap, align                        | linear horizontal sequence        |
| `column`   | gap, align                        | linear vertical sequence          |
| `dagre`    | direction (LR/TB), node-sep, rank-sep | DAGs (uses `edges` to compute) |
| `tree`     | rootId, direction, child-sep      | rooted tree                       |
| `force`    | iterations, repulsion, link-len   | unstructured graph                |
| `radial`   | rootId, ring-sep, start-angle     | hub-and-spoke                     |
| `align`    | axis, anchor                      | align selection on an axis        |
| `distribute` | axis                            | equal spacing along an axis       |

`dagre` and `force` use [d3-dag](https://github.com/erikbrinkman/d3-dag)
and [d3-force](https://github.com/d3/d3-force) (both small, both used
by tldraw too). The choice of d3 over a graph-only library keeps total
bundle delta under ~80 KB.

---

## 8. Viewport / Camera

```ts
type Viewport = { x: number; y: number; zoom: number };
```

`(x, y)` is the canvas pixel under the top-left of the visible area;
`zoom` is canvas-px-to-CSS-px multiplier (`0.1 .. 4.0`).

### 8.1 Pan / zoom

- Trackpad two-finger swipe → pan (no Shift required, native macOS
  behavior).
- Pinch → zoom around cursor.
- `⌘ + scroll` → zoom around cursor (mouse fallback).
- `Space + drag` → pan (Figma habit).
- `1` → 100% zoom centered on selection (or origin).
- `⇧ 1` → fit selection.
- `⇧ 2` → fit content.

### 8.2 Minimap

Bottom-right corner, 200×120 px, semi-transparent. Shows the full bbox
of all shapes plus the viewport rect. Click to teleport, drag the
viewport rect to pan. Toggle with `M`.

### 8.3 "Fly to" animations

When the agent calls `canvas.focus({ shapeId | rect })`, the viewport
animates with a `smooth` spring over ~350 ms. Helpful when the agent
adds shapes off-screen and wants to draw the user's eye.

---

## 9. Drag-and-Drop Integration

Canvas is a **drop target** for everything draggable in Forge. Drop
position becomes the new shape's center.

| Source                    | Drop produces                                     |
|---------------------------|---------------------------------------------------|
| Inbox / Jira ticket       | `jira-card` shape                                 |
| GitHub PR                 | `github-pr-card`                                  |
| GitHub issue              | `github-issue-card`                               |
| Sentry event              | `sentry-event-card`                               |
| File from editor tree     | `file-card`                                       |
| Chat message (long-press) | `chat-message-card`                               |
| Image (OS clipboard / Finder) | `image` (asset copied into `<canvas-id>.assets/`) |
| `.canvas.json` file       | imported as new canvas (asks before replacing)    |
| Plain text (OS)           | `sticky` shape                                    |
| Mermaid block             | `mermaid` shape                                   |

Reverse — Canvas as a drag **source**:

- Drag a shape **out** of the canvas onto a Claude / Cursor column →
  attaches as a `Mention` (the mention payload includes the shape's id,
  kind, label, and a small JSON snapshot of its props). The agent sees
  it the same way it sees any other mention.
- Drag a `jira-card` shape onto another column → behaves like the
  original ticket.

Drop indicators reuse the existing accent-glow drop-zone style from
[UI.md §4.3](UI.md).

---

## 10. Agent Integration

The crux. Three things to spec: (a) how the agent **reads** the canvas;
(b) how it **writes**; (c) how a session is **linked** to a canvas.

### 10.1 The link

A Claude / Cursor session can be linked to **at most one** canvas (per
session). Linking happens by:

- Drag the canvas-column instance pill onto the agent column's "link
  zone" (existing UX, mirrors how Editor link works today).
- Or open the agent's link menu and pick "Link canvas… → Mona-Lisa".

When linked, the canvas's MCP tools are exposed to that session
(see §10.4). Unlinking removes the tools on the next turn. This is per
session, not per workbench — two Claude columns can target two
different canvases in the same workbench.

Persistence: `ClaudeSession` gains:

```ts
linkedCanvasId: string | null;
linkedCanvasInstanceId: string | null;   // which Canvas column tab
```

Mirrors `linkedToEditor` / `linkedToEditorInstanceId` from [types.ts](../apps/desktop/src/lib/types.ts).

### 10.2 What the agent sees on every turn

When a session is linked to a canvas, we **prepend a compact summary**
of the canvas to the system prompt — much like cwd recap. The summary
is short (< 500 tokens) and changes between turns:

```
You are linked to canvas "Architecture v3" (id: abc123).
Bounds: [-1800,-900]..[2200,1400]. 47 shapes, 31 edges.
Last user edit: 2m ago. Current viewport: (-200,-100, zoom 0.8).
Use the canvas.* tools to read the full state and to draw.
```

The agent does **not** automatically receive the full state — it'd
balloon the prompt for every turn. It calls `canvas.get_state()` when
it actually needs to operate.

### 10.3 Two reading modes

#### a) JSON scene graph — `canvas.get_state(opts?)`

Returns the full canvas JSON, optionally filtered:

```ts
canvas.get_state({
  bbox?: [x1, y1, x2, y2];          // only shapes intersecting bbox
  kinds?: ShapeKind[];               // filter by kind
  ids?: string[];                    // exact ids
  include_edges?: boolean;           // default true
  include_props?: boolean;           // default true; false → skeleton only
  detail?: 'full'|'compact';         // compact strips long text/image data
}) -> Canvas (filtered)
```

Sub-pixel positions, exact w/h, all metadata. The agent uses this for
"where exactly is shape X" / "what's near coordinate (300, 120)".

#### b) Screenshot — `canvas.screenshot(opts?)`

Returns a base64 PNG via the existing tool-result image-input channel
(Claude / Cursor both support image inputs):

```ts
canvas.screenshot({
  view?: 'viewport'|'all'|{ rect: [x1,y1,x2,y2] };
  zoom?: number;                     // override
  max_dim?: number;                  // largest side, default 1600 px
  include_grid?: boolean;            // default false
  include_labels?: boolean;          // default true
  highlight_ids?: string[];          // boxes drawn around these
}) -> { mime: 'image/png'; data: string /* base64 */; w: number; h: number; }
```

Used when the JSON isn't enough — freehand strokes, "does this layout
look balanced", "is the arrow pointing at the right node visually". The
screenshot is rendered through the same renderer as the live canvas
(off-screen `<canvas>` or SVG rasterization) and returned synchronously.

#### c) Hybrid — typical agent recipe

```
1. canvas.get_state({ detail: 'compact' })       # cheap overview
2. (reason about what to add / move)
3. canvas.screenshot({ view: 'viewport' })        # eyeball it
4. canvas.add_shape(...) | canvas.move_shape(...)
5. canvas.screenshot({ highlight_ids: [...] })    # confirm
```

### 10.4 Writing — the MCP tool catalog

All tools live under the `canvas.*` namespace and are served by a new
sidecar **`woom-canvas`** following the existing MCP pattern (see
sidecars list under [src-tauri/sidecars/](../apps/desktop/src-tauri/sidecars/)).
The sidecar is a thin Rust shim that proxies to the running Woom
app via Tauri IPC — the canvas state is in the SvelteKit process, the
MCP server speaks JSON-RPC over stdio.

Catalog (v1):

| Tool                          | Purpose                                          |
|-------------------------------|--------------------------------------------------|
| `canvas.list`                 | Library: list canvases (id, name, updatedAt).    |
| `canvas.create`               | New canvas, returns id.                          |
| `canvas.open`                 | Switch the linked session to a different canvas. |
| `canvas.get_state`            | §10.3 (a).                                       |
| `canvas.screenshot`           | §10.3 (b).                                       |
| `canvas.add_shape`            | One shape at a time, returns id + final coords.  |
| `canvas.add_shapes`           | Batch insert, atomic; returns ids in order.      |
| `canvas.update_shape`         | Patch a shape (any subset of fields).            |
| `canvas.move_shape`           | Translate; supports `snap` parameter.            |
| `canvas.resize_shape`         | New `w`,`h`; supports `snap`.                    |
| `canvas.delete_shape`         | Single id or array.                              |
| `canvas.add_edge`             | Connect two shapes.                              |
| `canvas.update_edge`          | Patch.                                           |
| `canvas.delete_edge`          | Remove.                                          |
| `canvas.group`                | Wrap shapes in a `group` or `frame`.             |
| `canvas.ungroup`              | Inverse.                                         |
| `canvas.arrange`              | §7. Returns the new positions per id.            |
| `canvas.align`                | Align selection on axis.                         |
| `canvas.distribute`           | Equal spacing.                                   |
| `canvas.set_z`                | Z-order: `to-front`, `to-back`, `forward`, `backward`. |
| `canvas.set_viewport`         | Pan / zoom programmatically.                     |
| `canvas.focus`                | Animate viewport to show a shape / rect.         |
| `canvas.upload_image`         | Receive base64 PNG, store as asset, return assetPath. |
| `canvas.duplicate`            | Clone a shape / selection at an offset.          |
| `canvas.lock` / `canvas.unlock` | Toggle `locked` on shape ids.                  |
| `canvas.find`                 | Substring / regex search across labels + text + sticky markdown. Returns ids + bboxes. |
| `canvas.diff_since`           | Given a `version`, return ops applied since. Lets the agent re-sync without rereading the whole state. |

#### Tool result conventions

Every mutating tool returns:

```ts
{
  ok: true;
  version: number;            // canvas.version after the op
  applied: Op[];              // what really happened (after snapping etc)
}
```

or

```ts
{
  ok: false;
  error: { code: string; message: string; ids?: string[]; };
  version: number;            // unchanged
}
```

Errors include `'shape-not-found'`, `'invalid-bbox'`, `'locked'`,
`'canvas-not-linked'`, `'cycle-detected'` (for parentId).

#### Atomicity

`canvas.add_shapes`, `canvas.arrange`, and any tool that takes an
array of ids are **atomic**: either all applied, or none, with a
single `version` bump. Implemented as a transaction in the canvas
store; failures roll back in memory.

### 10.5 Worked examples

#### Example A — agent draws a high-level architecture from a Jira ticket

User drops PROJ-1234 onto the linked Canvas; agent sees the new
`jira-card`, reads the ticket body, then:

```
1. canvas.get_state({ ids: ['<jira-card-id>'], detail: 'compact' })
   → { shapes: [{ id, kind: 'jira-card', x: 200, y: 200, w: 280, h: 140, ... }] }

2. canvas.add_shapes([
     { kind: 'frame', x: 0, y: 0, w: 1000, h: 600,
       props: { title: 'Auth flow' } },
     { kind: 'rect',  x: 80,  y: 80,  w: 160, h: 80,
       props: { fill: '--bg-2', stroke: '--accent-claude' },
       label: 'Client' },
     { kind: 'rect',  x: 320, y: 80,  w: 160, h: 80, label: 'Auth API' },
     { kind: 'rect',  x: 320, y: 220, w: 160, h: 80, label: 'Sessions' },
     { kind: 'rect',  x: 560, y: 80,  w: 160, h: 80, label: 'OAuth provider' },
   ])
   → ok, ids: [F, A, B, C, D]

3. canvas.add_edge({ from: { shapeId: A, anchor: 'mr' },
                     to:   { shapeId: B, anchor: 'ml' }, kind: 'arrow' })
   ... (edges A→B, B→D, B→C)

4. canvas.arrange({ algorithm: 'dagre', direction: 'LR',
                    ids: [A, B, C, D] })
   → all four rect bboxes laid out cleanly inside the frame
```

#### Example B — agent labels a screenshot with annotations

Agent took a `canvas.screenshot()` of a UI mockup the user pasted; it
now adds callouts:

```
1. canvas.find({ kind: 'image', label_contains: 'mockup' })
   → { ids: [IMG], bboxes: [{ x: 100, y: 100, w: 800, h: 500 }] }

2. canvas.add_shapes([
     { kind: 'sticky', x: 940, y: 120, w: 200, h: 80,
       props: { markdown: '**1.** Header is too tight\n8px → 16px' } },
     { kind: 'sticky', x: 940, y: 220, w: 200, h: 80,
       props: { markdown: '**2.** Primary CTA color contrast' } },
   ])

3. canvas.add_edge({ from: { shapeId: STICKY1, anchor: 'ml' },
                     to:   { shapeId: IMG, offset: { dx: 200, dy: 40 } },
                     kind: 'arrow' })
```

### 10.6 Concurrency

User and agent share one canvas. Conflict policy is **last-writer-wins
on a per-shape basis**. Specifically:

- Every shape carries `updatedAt` and `updatedBy: 'user'|'agent'`.
- A user mutation while the agent has a pending op pipeline interrupts:
  the agent's next tool call gets `version` mismatch and an explicit
  error code `version-stale`. The agent must re-read state.
- We do **not** auto-merge. Two writers on the same shape produces a
  toast: "Agent moved Card A while you were editing it — agent edit
  applied".
- Animations (gentle fade-in) on agent-inserted shapes make the
  multi-actor reality visible.

### 10.7 Permissions

The agent has full power on the canvas it's linked to. There is **no
"approval" pre-flight** for canvas mutations (unlike commits / PRs),
because the cost of an unwanted shape is "press undo". `canvas.delete`
of more than 10 shapes in one call surfaces a one-click confirm toast,
similar to how the editor's bulk-delete does.

`canvas.delete_canvas` is **not** in the v1 catalog — only the user
deletes canvases.

---

## 11. Data Model & Persistence

### 11.1 On disk

```
canvases/<id>.json                     # Canvas record (no inlined assets)
canvases/<id>.thumb.png                # 256×160 PNG, regenerated on save
canvases/<id>.assets/<asset-id>.png    # binary assets referenced by Shape.props
canvases/<id>.history/                 # 30-step bounded undo log (one op-set per file)
```

`<id>.json` is the source of truth. The thumbnail is regenerated from
it on every save (debounced).

### 11.2 In memory

```ts
// $lib/state/canvas.svelte.ts
export const canvasState = $state<{
  index: CanvasIndex;
  open: Map<string, Canvas>;          // canvasId → loaded Canvas
  activeByInstance: Record<string, string|null>;  // columnInstanceId → canvasId
  selection: Record<string, string[]>;            // canvasId → shapeIds
  hoverId: Record<string, string|null>;
}>({ ... });
```

Same pattern as [layout.svelte.ts](../apps/desktop/src/lib/state/layout.svelte.ts) /
[sessions.svelte.ts](../apps/desktop/src/lib/state/sessions.svelte.ts).

### 11.3 Operation log (undo / redo)

Every mutation is an `Op`:

```ts
type Op =
  | { kind: 'add'; shape: Shape }
  | { kind: 'remove'; shapeId: string; backup: Shape }
  | { kind: 'patch'; shapeId: string; before: Partial<Shape>; after: Partial<Shape> }
  | { kind: 'edge-add' | 'edge-remove' | 'edge-patch'; … }
  | { kind: 'viewport'; before: Viewport; after: Viewport };
```

`undo` reverses; `redo` re-applies. Bounded history (50 ops in memory,
30 last persisted to `<id>.history/` for crash recovery — first 20
discarded). Agent ops and user ops share one log.

### 11.4 Schema migration

The Canvas JSON has a top-level `schemaVersion: number`. v1 ships
`schemaVersion: 1`. On load we run a migration chain. Backwards
compatibility through v1.x is required; v2 may introduce a one-shot
migration with a confirm modal.

---

## 12. Canvas Column UI

```
╭─ canvas column ─────────────────────────────────────╮
│ ▣ Canvas (Mona-Lisa) ▾ │ Arch v3 │ Postmortem │ + │  ← header (§3.2)
├─────────────────────────────────────────────────────┤
│ ▾ Tools                                             │  ← collapsible toolbar
│   ↻ ✋ ▢ ◯ → A T □ ✎ ⊞ ⌗                            │     (kinds + ops)
│   pan select rect ellipse arrow text sticky frame   │
│                                                     │
│   ▢ □ ▭ ▭                                           │  ← layout buttons
│   grid row col dag                                  │
├─────────────────────────────────────────────────────┤
│                                                     │
│                                                     │
│        DRAWING AREA                                 │
│                                                     │
│                                                     │
│                                                     │
├─────────────────────────────────────────────────────┤
│ minimap (bottom-right corner, overlay)              │
│ status bar: 47 shapes · zoom 80% · ⛓ Claude (Mona)  │
╰─────────────────────────────────────────────────────╯
```

- Toolbar collapses to a vertical icon strip when the column is < 360 px.
- Right-click on an empty area opens a context menu (paste, add sticky,
  add frame, paste image from clipboard, fit to content).
- Selection shows handles (8 + rotation) plus the smart-guide overlay
  during transforms.
- Bottom status bar always shows agent-link state — clicking the chip
  opens the link picker.

---

## 13. Keyboard

Aligned with the rest of Forge ([UI.md](UI.md) — keyboard-first principle).

| Key                   | Action                                  |
|-----------------------|-----------------------------------------|
| `V`                   | Select tool                             |
| `H`                   | Pan tool (or `Space` while held)        |
| `R`                   | Rectangle                               |
| `O`                   | Ellipse                                 |
| `A`                   | Arrow                                   |
| `T`                   | Text                                    |
| `S`                   | Sticky                                  |
| `F`                   | Frame                                   |
| `P`                   | Pencil (freehand)                       |
| `M`                   | Toggle minimap                          |
| `G`                   | Toggle grid                             |
| `⌘ Z` / `⇧⌘Z`         | Undo / Redo                             |
| `⌘ C` / `⌘ V` / `⌘ D` | Copy / Paste / Duplicate                |
| `⌘ A`                 | Select all                              |
| `⌫`                   | Delete selection                        |
| `⌘ G` / `⇧⌘G`         | Group / Ungroup                         |
| `[` / `]`             | Send back / bring forward               |
| `⇧[` / `⇧]`           | Send to back / bring to front           |
| `1`                   | 100% zoom on selection                  |
| `⇧1`                  | Fit selection                           |
| `⇧2`                  | Fit content                             |
| `⌘ P`                 | Library overlay                         |
| `⌘ K`                 | Command palette (canvas-aware actions)  |

---

## 14. Performance

Targets on a 2024 MacBook Air M3:

- **60 fps** pan / zoom up to 2 000 shapes.
- **< 16 ms** frame budget for incremental shape add (single insert).
- **< 250 ms** to render a `canvas.screenshot({ view: 'viewport' })` at
  default quality.
- **< 1 s** to load and hydrate a 5 000-shape canvas from disk.

Strategy:

- Render uses **Svelte components in absolute-positioned divs** for
  shapes that have HTML/CSS-friendly content (text, sticky, code
  blocks, live cards) and an **off-screen `<canvas>` 2D layer** for
  primitive geometry + freehand strokes. Pasted images go in DOM with
  `image-rendering: pixelated` only when zoom > 2.
- Quad-tree spatial index for hit-testing and viewport culling. Rebuilt
  incrementally on shape add/move.
- Pan/zoom uses CSS `transform: translate(...) scale(...)` on the
  containing layer (single composited transform).
- Mermaid / DOT / code blocks render inside `requestIdleCallback`
  during a batch insert so a 100-shape `canvas.add_shapes` doesn't
  block the main thread.
- Autosave is debounced + write-coalesced.

---

## 15. Tech Stack Decision

### 15.1 We do **not** use tldraw or Excalidraw

Both are great. Both bring tradeoffs we don't want:

- **tldraw 2** is React. We'd need a React island in Svelte (mountable,
  but adds React + ReactDOM + tldraw bundle ≈ 600 KB gz). The tldraw
  store API is excellent for an agent (programmatic shape API, stable
  uuids, undo built-in) — the temptation is real. We pass because the
  bundle cost violates Forge's existing budget and because the visual
  language doesn't quite match (tldraw's defaults are bright; theming
  works but is friction).
- **Excalidraw** has the wrong aesthetic — sketchy / hand-drawn — for
  Forge's clean dark UI. Theming to "non-sketchy" fights the library.

### 15.2 We build a Svelte-native renderer

Stack:

- **Svelte 5 runes** for state (matches the rest of `$lib/state/*.svelte.ts`).
- **HTML/CSS layer** for rich shapes (text, sticky, code, live cards).
- **Off-screen `<canvas>` 2D** for geometry primitives + freehand
  strokes. One layer behind, one layer above (so a sticky can be on
  top of a stroke and vice versa via z).
- **`perfect-freehand`** for stroke geometry.
- **`mermaid`** for `mermaid` shapes.
- **`@hpcc-js/wasm-graphviz`** for `dot`.
- **`d3-dag`, `d3-force`** for layouts.
- **CodeMirror 6** for `code` shapes — already in the project.
- **`@tauri-apps/api/fs`** for canvas file IO via the existing
  [fs.rs](../apps/desktop/src-tauri/src/fs.rs) IPC.

Total estimated bundle delta: **~250 KB gz** (mermaid is the heaviest
piece at ~90 KB; everything else is small). Within budget.

### 15.3 Sidecar

`woom-canvas` — a Rust binary (same shape as `woom-app`) that
exposes the §10.4 tool catalog over MCP stdio. The sidecar is a thin
proxy: every tool is forwarded as a JSON-RPC call to the running
Woom app via a localhost-loopback socket bound to a random port
written to a file in the app data dir on startup, with a token. The
app process owns the canvas state — the sidecar is stateless.

---

## 16. Implementation Plan

Suggested milestones, sized for one engineer.

### M-canvas-1 — Renderer skeleton (1 week)

- New `PanelKind = 'canvas'`, default width 720.
- New column component `CanvasColumn.svelte`.
- Empty canvas, pan + zoom + grid, viewport persistence.
- Add a `Canvas` to the layout state, single canvas per column for now.
- 60 fps verified on empty canvas.

### M-canvas-2 — Primitive shapes (1 week)

- `rect`, `ellipse`, `line`, `arrow-shape`, `text`, `sticky`.
- Selection, transform handles, snapping (grid + edge), undo/redo.
- Keyboard shortcuts.
- Shape autosave to disk (one file per canvas).

### M-canvas-3 — Rich shapes & assets (1 week)

- `mermaid`, `dot`, `code`, `image`, `freehand`.
- `<canvas-id>.assets/` directory wiring.
- Image paste from clipboard.

### M-canvas-4 — Edges & layouts (1 week)

- Anchored edges with three routings.
- `grid`, `row`, `column`, `dagre`, `tree` layouts (force / radial in v1.1).
- Smart guides during transform.

### M-canvas-5 — Multi-canvas library (3 days)

- Library index in localStorage.
- Tabs in column header.
- Library overlay (⌘P).
- Archive / duplicate / export / import.

### M-canvas-6 — Forge live cards (1 week)

- `jira-card`, `github-pr-card`, `github-issue-card`, `sentry-event-card`,
  `file-card`, `chat-message-card`.
- Reuses card components from existing columns; new wrappers that
  re-render on inbox state change.
- Drag-from-inbox-onto-canvas drop handling.

### M-canvas-7 — `woom-canvas` sidecar + MCP tools (1 week)

- Rust sidecar skeleton (mirror `woom-app` shape).
- Tool catalog from §10.4, with the IPC bridge.
- Per-session linking UI.
- Canvas summary in the system prompt for linked sessions.

### M-canvas-8 — Agent vision channel (3 days)

- `canvas.screenshot` implementation (off-screen render).
- Plug into the existing image-input path used by Cursor / Claude
  ([applyToAgent.ts](../apps/desktop/src/lib/services/applyToAgent.ts) /
  [agentContext.ts](../apps/desktop/src/lib/services/agentContext.ts)).
- Round-trip test: agent reads, draws, screenshots, refines.

### M-canvas-9 — Polish (1 week)

- Minimap, status bar, smart-guide visuals, alignment tool, ⌘K canvas-aware
  actions.
- Performance pass to hit the §14 targets.
- Crash-recovery test from `<id>.history/`.

**Total:** ~7–8 weeks of focused work for v1 of Canvas.

---

## 17. Out of Scope (for v1, revisit later)

- Real-time multi-user editing / CRDT — paired with the team layer (v0.3+).
- Plugin shapes / custom shape kinds.
- PlantUML rendering (no good local-only path today).
- 3D / parallax / animation between frames.
- Slideshow / present mode.
- Embedded video / GIF playback (static frame only in v1).
- Web embed in a shape (iframe security cost).
- Math / LaTeX shapes (KaTeX is small but the agent rarely needs it
  rendered — it can write Mermaid / images instead).
- Vector path / pen tool with bezier handles.
- Boolean ops on shapes (union / subtract / intersect).
- Voice notes attached to shapes.

---

## 18. Open Questions

1. **Library is per-workbench or global?** Currently the spec says
   per-workbench; an alternative is one global library and any workbench
   can open any canvas. Global feels simpler but mixes "this canvas is
   for the Auth workbench" context.
2. **Should an agent be able to create new canvases unprompted?** The
   spec exposes `canvas.create` to agents. This is convenient ("draw
   me the architecture" → agent makes a fresh canvas) but a chatty
   agent could litter the library. Alternative: only the linked canvas
   can be edited; new canvases require the user to create them first.
3. **Does drag-out from canvas to chat send a screenshot or a JSON
   snippet as the mention payload?** Probably both: `Mention.body` as
   compact JSON, plus an attached image (the existing image-input
   path). Confirm with usage.
4. **PlantUML in v1?** Excluded above. Could ship as text-only with no
   render until WASM exists. Decide before M-canvas-3.
5. **Agent vision: full PNG vs SVG vs ASCII tree?** PNG is most general
   but tokens-heavy. SVG would let the agent reason about vector
   structure but doubles the tool implementation. ASCII tree (auto
   generated from the scene graph) might be enough in many cases. v1
   ships PNG; SVG / ASCII as future affordances.
6. **Mobile / touch** — out of scope (macOS only) but a touch event
   model that *could* work later is worth keeping in mind during
   pointer-event plumbing.

---

## 19. Glossary

- **Canvas** — one drawable plane.
- **Shape** — anything on a canvas with a bounding box.
- **Edge** — connector between two shapes.
- **Frame** — labeled rectangular region; children move with it.
- **Live card** — shape that is a projection of a Forge object
  (Jira / PR / file / chat message), kept fresh from its source.
- **Library** — the set of canvases for a workbench; persisted on disk.
- **Agent link** — a `ClaudeSession` ↔ `Canvas` binding that exposes
  `canvas.*` tools to that session.
- **Snap regime** — the rule (`grid`, `edge`, `distance`) by which a
  position is clamped during a transform.
- **Op** — a single mutation in the undo/redo log.
