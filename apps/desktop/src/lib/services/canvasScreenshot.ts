// Canvas screenshot — rasterize a Canvas into a PNG dataURL.
//
// Used as the vision channel for linked agent sessions: we render the
// canvas to a PNG and attach it as an image input alongside the user's
// text message every turn. The agent gets BOTH the structured JSON
// inventory in the system prompt (precise — ids, coordinates) AND a
// pixel snapshot (qualitative — what does this LAYOUT actually look
// like, where are the empty regions, are the labels readable).
//
// We deliberately don't try to pixel-perfectly mirror the live canvas
// renderer:
//   - mermaid / DOT diagrams render to SVG live; we'd have to invoke
//     mermaid here too, which doubles bundle cost. Instead we render
//     a placeholder rect with the source's first line so the agent
//     knows "there's a mermaid here saying X" — it already has the
//     full source in the system prompt JSON.
//   - code blocks become a "[code lang]" placeholder for the same
//     reason.
//   - live cards (jira / PR / etc) become small tinted rects with the
//     lookup key — agent already has full state via the JSON.
//
// What WE DO render faithfully:
//   - Geometry primitives (rect, ellipse, line, arrow) — gives the
//     agent the actual spatial structure.
//   - Text and sticky bodies (truncated to bbox).
//   - Freehand strokes (the agent has no other way to "see" a
//     hand-drawn sketch).
//   - Embedded images (drawImage from the dataUrl).
//   - Edges with their routings + arrowheads.
//
// The result is "blueprint of the canvas + the bits that aren't in
// JSON". 50–150 KB PNG per turn for typical canvases.

import { invoke } from '@tauri-apps/api/core';
import type { Canvas, Shape, Edge } from '$lib/state/canvas.svelte';
import { getEdgeEndpoint } from '$lib/state/canvas.svelte';

export type ScreenshotOpts = {
  /** Target longest-side dimension in CSS px. Smaller = lower quality
   *  but cheaper tokens. Default 1280 — Anthropic's vision works fine
   *  at this resolution and tokens settle around ~3k per image. */
  maxDim?: number;
  /** Padding around the content AABB in canvas px. Default 40 — gives
   *  the agent some breathing room around the edges. */
  padding?: number;
};

/** Compute AABB of all shapes (or null when empty). */
function aabb(shapes: Shape[]): { x: number; y: number; w: number; h: number } | null {
  if (shapes.length === 0) return null;
  let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
  for (const s of shapes) {
    if (s.x < minX) minX = s.x;
    if (s.y < minY) minY = s.y;
    if (s.x + s.w > maxX) maxX = s.x + s.w;
    if (s.y + s.h > maxY) maxY = s.y + s.h;
  }
  return { x: minX, y: minY, w: maxX - minX, h: maxY - minY };
}

/** Pull the most descriptive text-ish field per shape kind for the
 *  rasterized label. Returns "" when nothing meaningful exists. */
function shapeText(s: Shape): string {
  const p = s.props as Record<string, unknown>;
  switch (s.kind) {
    case 'text':              return typeof p.text === 'string' ? p.text : '';
    case 'sticky':            return typeof p.markdown === 'string' ? p.markdown : '';
    case 'mermaid':
    case 'dot':
    case 'code': {
      const src = typeof p.source === 'string' ? p.source : '';
      return src.split('\n')[0]?.trim() ?? '';
    }
    case 'jira-card':         return typeof p.ticketKey === 'string' ? p.ticketKey : '';
    case 'github-pr-card':
    case 'github-issue-card': return `#${p.number ?? '?'}`;
    case 'sentry-event-card': return typeof p.shortId === 'string' ? p.shortId : '';
    case 'file-card':         return typeof p.relPath === 'string' ? p.relPath : '';
    case 'chat-message-card': {
      const snap = p.snapshot;
      if (snap && typeof snap === 'object') {
        const ex = (snap as Record<string, unknown>).excerpt;
        if (typeof ex === 'string') return ex.slice(0, 60);
      }
      return '';
    }
    default:                  return s.label ?? '';
  }
}

/** Per-kind tint used for the rasterized representation. Mirrors the
 *  live renderer's stripe colors so the agent sees the same visual
 *  hierarchy. */
function tintFor(kind: string): { fill: string; stroke: string } {
  switch (kind) {
    case 'rect':                return { fill: 'rgba(255,255,255,0.04)', stroke: '#9094a0' };
    case 'ellipse':             return { fill: 'rgba(255,255,255,0.04)', stroke: '#9094a0' };
    case 'sticky':              return { fill: 'rgba(238, 107, 31, 0.18)', stroke: 'rgba(238, 107, 31, 0.55)' };
    case 'mermaid':
    case 'dot':                 return { fill: 'rgba(14, 165, 233, 0.08)', stroke: 'rgba(14, 165, 233, 0.55)' };
    case 'code':                return { fill: 'rgba(168, 85, 247, 0.08)', stroke: 'rgba(168, 85, 247, 0.55)' };
    case 'image':               return { fill: 'rgba(34, 197, 94, 0.08)', stroke: 'rgba(34, 197, 94, 0.55)' };
    case 'jira-card':           return { fill: 'rgba(38, 132, 255, 0.10)', stroke: '#2684FF' };
    case 'github-pr-card':
    case 'github-issue-card':   return { fill: 'rgba(139, 92, 246, 0.10)', stroke: '#8B5CF6' };
    case 'sentry-event-card':   return { fill: 'rgba(248, 143, 116, 0.10)', stroke: '#F88F74' };
    case 'file-card':           return { fill: 'rgba(232, 163, 58, 0.10)', stroke: '#E8A33A' };
    case 'chat-message-card':   return { fill: 'rgba(217, 119, 87, 0.10)', stroke: '#D97757' };
    case 'frame':               return { fill: 'transparent', stroke: 'rgba(255,255,255,0.18)' };
    case 'group':               return { fill: 'transparent', stroke: 'rgba(255,255,255,0.06)' };
    default:                    return { fill: 'rgba(255,255,255,0.03)', stroke: 'rgba(255,255,255,0.45)' };
  }
}

/** Wrap `text` to fit inside `maxWidth` at the given font, returning
 *  one line per row. Trims to `maxLines`. */
function wrapLines(
  ctx: CanvasRenderingContext2D,
  text: string,
  maxWidth: number,
  maxLines: number
): string[] {
  if (!text) return [];
  const words = text.replace(/\s+/g, ' ').trim().split(' ');
  const lines: string[] = [];
  let current = '';
  for (const w of words) {
    const candidate = current ? `${current} ${w}` : w;
    if (ctx.measureText(candidate).width <= maxWidth) {
      current = candidate;
    } else {
      if (current) lines.push(current);
      current = w;
      if (lines.length >= maxLines) break;
    }
  }
  if (current && lines.length < maxLines) lines.push(current);
  if (lines.length === maxLines && words.join(' ').length > lines.join(' ').length) {
    lines[lines.length - 1] = lines[lines.length - 1].replace(/.{0,3}$/, '…');
  }
  return lines;
}

function drawShape(
  ctx: CanvasRenderingContext2D,
  s: Shape,
  loadedImages: Map<string, HTMLImageElement>
) {
  ctx.save();
  if (s.rot !== 0) {
    ctx.translate(s.x + s.w / 2, s.y + s.h / 2);
    ctx.rotate(s.rot);
    ctx.translate(-(s.x + s.w / 2), -(s.y + s.h / 2));
  }
  const tint = tintFor(s.kind);
  switch (s.kind) {
    case 'rect': {
      const p = s.props as Record<string, unknown>;
      const radius = typeof p.radius === 'number' ? p.radius : 6;
      const sw = typeof p.strokeWidth === 'number' ? p.strokeWidth : 2;
      ctx.beginPath();
      ctx.fillStyle = tint.fill;
      ctx.strokeStyle = tint.stroke;
      ctx.lineWidth = sw;
      roundRect(ctx, s.x, s.y, s.w, s.h, radius);
      ctx.fill();
      ctx.stroke();
      break;
    }
    case 'ellipse': {
      const sw = ((s.props as Record<string, unknown>).strokeWidth as number | undefined) ?? 2;
      ctx.beginPath();
      ctx.fillStyle = tint.fill;
      ctx.strokeStyle = tint.stroke;
      ctx.lineWidth = sw;
      ctx.ellipse(s.x + s.w / 2, s.y + s.h / 2, Math.max(0, s.w / 2 - sw / 2), Math.max(0, s.h / 2 - sw / 2), 0, 0, Math.PI * 2);
      ctx.fill();
      ctx.stroke();
      break;
    }
    case 'line':
    case 'arrow-shape': {
      const p = s.props as Record<string, unknown>;
      const from = (p.from as { x: number; y: number } | undefined) ?? { x: 0, y: 0 };
      const to = (p.to as { x: number; y: number } | undefined) ?? { x: s.w, y: s.h };
      const ax = s.x + from.x, ay = s.y + from.y;
      const bx = s.x + to.x, by = s.y + to.y;
      const thickness = typeof p.thickness === 'number' ? p.thickness : 2;
      ctx.lineWidth = thickness;
      ctx.strokeStyle = '#cfd2da';
      ctx.lineCap = 'round';
      ctx.beginPath();
      ctx.moveTo(ax, ay);
      ctx.lineTo(bx, by);
      ctx.stroke();
      if (s.kind === 'arrow-shape') drawArrowhead(ctx, ax, ay, bx, by, '#cfd2da');
      break;
    }
    case 'freehand': {
      const p = s.props as Record<string, unknown>;
      const pts = (p.points as unknown[] | undefined) ?? [];
      if (pts.length < 2) break;
      ctx.strokeStyle = '#f4f0eb';
      ctx.lineWidth = (typeof p.thickness === 'number' ? p.thickness : 2) * 1.6;
      ctx.lineJoin = 'round';
      ctx.lineCap = 'round';
      ctx.beginPath();
      let started = false;
      for (const pt of pts) {
        if (!Array.isArray(pt) || pt.length < 2) continue;
        const [px, py] = pt as [number, number];
        const wx = s.x + px, wy = s.y + py;
        if (!started) { ctx.moveTo(wx, wy); started = true; }
        else ctx.lineTo(wx, wy);
      }
      ctx.stroke();
      break;
    }
    case 'text':
    case 'sticky':
    case 'mermaid':
    case 'dot':
    case 'code':
    case 'jira-card':
    case 'github-pr-card':
    case 'github-issue-card':
    case 'sentry-event-card':
    case 'file-card':
    case 'chat-message-card':
    case 'frame':
    case 'group': {
      const radius = s.kind === 'sticky' ? 8 : (s.kind === 'frame' ? 10 : 6);
      ctx.beginPath();
      ctx.fillStyle = tint.fill;
      ctx.strokeStyle = tint.stroke;
      ctx.lineWidth = 1.5;
      roundRect(ctx, s.x, s.y, s.w, s.h, radius);
      ctx.fill();
      ctx.stroke();
      /* Tag badge in the corner showing the kind, so the agent can
         tell a mermaid-placeholder apart from a code-placeholder
         from a live-card visually. */
      if (s.kind !== 'frame' && s.kind !== 'group') {
        ctx.font = `600 9px -apple-system, system-ui, sans-serif`;
        ctx.fillStyle = tint.stroke;
        ctx.textBaseline = 'top';
        const tag = s.kind.replace(/-card$/, '').replace(/-/g, ' ');
        ctx.fillText(`[${tag}]`, s.x + 6, s.y + 4);
      }
      /* Body text — single-line for text/cards, wrapped for sticky.
         For frame/group nothing — they're container glyphs. */
      const body = shapeText(s);
      if (body && s.kind !== 'frame' && s.kind !== 'group') {
        const fontSize = s.kind === 'text'
          ? (((s.props as Record<string, unknown>).fontSize as number | undefined) ?? 14)
          : (s.kind === 'sticky' ? 12 : 11);
        ctx.font = `${s.kind === 'text' ? 500 : 400} ${fontSize}px -apple-system, system-ui, sans-serif`;
        ctx.fillStyle = '#f4f0eb';
        ctx.textBaseline = 'top';
        const lineHeight = fontSize * 1.35;
        const padX = 8;
        const yStart = s.kind === 'text' ? s.y + 6 : s.y + 18;
        const maxLines = Math.max(1, Math.floor((s.h - (yStart - s.y) - 4) / lineHeight));
        const lines = wrapLines(ctx, body, s.w - padX * 2, Math.min(maxLines, s.kind === 'text' ? 6 : 4));
        for (let i = 0; i < lines.length; i++) {
          ctx.fillText(lines[i], s.x + padX, yStart + i * lineHeight);
        }
      }
      break;
    }
    case 'image': {
      /* Try to draw the actual image when we've loaded it; otherwise
         show the tinted placeholder. We pre-load images in the caller
         so this stays synchronous. */
      const dataUrl = ((s.props as Record<string, unknown>).dataUrl as string | undefined) ?? '';
      const img = dataUrl ? loadedImages.get(dataUrl) : null;
      if (img && img.naturalWidth > 0) {
        ctx.drawImage(img, s.x, s.y, s.w, s.h);
      } else {
        ctx.beginPath();
        ctx.fillStyle = tint.fill;
        ctx.strokeStyle = tint.stroke;
        ctx.lineWidth = 1.5;
        roundRect(ctx, s.x, s.y, s.w, s.h, 6);
        ctx.fill();
        ctx.stroke();
        ctx.font = '500 11px -apple-system, system-ui, sans-serif';
        ctx.fillStyle = tint.stroke;
        ctx.fillText('[image]', s.x + 8, s.y + 8);
      }
      break;
    }
  }
  ctx.restore();
}

function roundRect(
  ctx: CanvasRenderingContext2D,
  x: number, y: number, w: number, h: number, r: number
) {
  const radius = Math.max(0, Math.min(r, Math.min(w, h) / 2));
  ctx.moveTo(x + radius, y);
  ctx.lineTo(x + w - radius, y);
  ctx.arcTo(x + w, y, x + w, y + radius, radius);
  ctx.lineTo(x + w, y + h - radius);
  ctx.arcTo(x + w, y + h, x + w - radius, y + h, radius);
  ctx.lineTo(x + radius, y + h);
  ctx.arcTo(x, y + h, x, y + h - radius, radius);
  ctx.lineTo(x, y + radius);
  ctx.arcTo(x, y, x + radius, y, radius);
}

function drawArrowhead(
  ctx: CanvasRenderingContext2D,
  fromX: number, fromY: number, toX: number, toY: number,
  color: string
) {
  const angle = Math.atan2(toY - fromY, toX - fromX);
  const headLen = 9;
  ctx.fillStyle = color;
  ctx.beginPath();
  ctx.moveTo(toX, toY);
  ctx.lineTo(toX - headLen * Math.cos(angle - Math.PI / 7), toY - headLen * Math.sin(angle - Math.PI / 7));
  ctx.lineTo(toX - headLen * Math.cos(angle + Math.PI / 7), toY - headLen * Math.sin(angle + Math.PI / 7));
  ctx.closePath();
  ctx.fill();
}

function drawEdge(ctx: CanvasRenderingContext2D, canvas: Canvas, edge: Edge) {
  const a = getEdgeEndpoint(canvas, edge.from);
  const b = getEdgeEndpoint(canvas, edge.to);
  if (!a || !b) return;
  ctx.strokeStyle = '#a8acb4';
  ctx.lineWidth = edge.thickness;
  ctx.lineCap = 'round';
  ctx.setLineDash(edge.kind === 'dashed' ? [6, 4] : []);
  if (edge.routing === 'orthogonal') {
    /* Two-elbow routing matching the live renderer's heuristic. */
    const fromIsHoriz = 'anchor' in edge.from && (edge.from.anchor === 'ml' || edge.from.anchor === 'mr');
    if (fromIsHoriz) {
      const midX = (a.x + b.x) / 2;
      ctx.beginPath();
      ctx.moveTo(a.x, a.y);
      ctx.lineTo(midX, a.y);
      ctx.lineTo(midX, b.y);
      ctx.lineTo(b.x, b.y);
      ctx.stroke();
    } else {
      const midY = (a.y + b.y) / 2;
      ctx.beginPath();
      ctx.moveTo(a.x, a.y);
      ctx.lineTo(a.x, midY);
      ctx.lineTo(b.x, midY);
      ctx.lineTo(b.x, b.y);
      ctx.stroke();
    }
  } else if (edge.routing === 'curved') {
    const dx = b.x - a.x;
    const dy = b.y - a.y;
    const reach = Math.min(120, Math.max(40, Math.hypot(dx, dy) * 0.4));
    ctx.beginPath();
    ctx.moveTo(a.x, a.y);
    ctx.bezierCurveTo(a.x + reach, a.y, b.x - reach, b.y, b.x, b.y);
    ctx.stroke();
  } else {
    ctx.beginPath();
    ctx.moveTo(a.x, a.y);
    ctx.lineTo(b.x, b.y);
    ctx.stroke();
  }
  ctx.setLineDash([]);
  if (edge.kind === 'arrow') drawArrowhead(ctx, a.x, a.y, b.x, b.y, '#cfd2da');
  if (edge.label) {
    ctx.font = '500 10px -apple-system, system-ui, sans-serif';
    ctx.fillStyle = '#f4f0eb';
    ctx.strokeStyle = '#0a0806';
    ctx.lineWidth = 3;
    const mx = (a.x + b.x) / 2;
    const my = (a.y + b.y) / 2;
    ctx.strokeText(edge.label, mx, my);
    ctx.fillText(edge.label, mx, my);
  }
}

/** Pre-load every embedded image's dataUrl into an Image element so
 *  the synchronous draw loop can use them without async waits. */
async function preloadImages(canvas: Canvas): Promise<Map<string, HTMLImageElement>> {
  const m = new Map<string, HTMLImageElement>();
  const promises: Promise<void>[] = [];
  for (const s of canvas.shapes) {
    if (s.kind !== 'image') continue;
    const dataUrl = (s.props as Record<string, unknown>).dataUrl as string | undefined;
    if (!dataUrl || m.has(dataUrl)) continue;
    promises.push(new Promise<void>((resolve) => {
      const img = new Image();
      img.onload = () => { m.set(dataUrl, img); resolve(); };
      img.onerror = () => { resolve(); /* fallback to placeholder */ };
      img.src = dataUrl;
    }));
  }
  await Promise.all(promises);
  return m;
}

/** Render `canvas` to a PNG dataURL. Returns null when there's
 *  literally nothing to show (zero shapes AND zero edges). */
export async function renderCanvasToDataUrl(
  canvas: Canvas,
  opts: ScreenshotOpts = {}
): Promise<string | null> {
  if (canvas.shapes.length === 0 && canvas.edges.length === 0) return null;
  const padding = opts.padding ?? 40;
  const maxDim = opts.maxDim ?? 1280;

  const bounds = aabb(canvas.shapes) ?? { x: 0, y: 0, w: 800, h: 600 };
  const w = bounds.w + padding * 2;
  const h = bounds.h + padding * 2;
  const scale = Math.min(maxDim / w, maxDim / h, 2);
  const outW = Math.round(w * scale);
  const outH = Math.round(h * scale);

  const off = document.createElement('canvas');
  off.width = outW;
  off.height = outH;
  const ctx = off.getContext('2d');
  if (!ctx) return null;

  ctx.fillStyle = '#0a0806'; /* matches --bg-0 (warm dark) */
  ctx.fillRect(0, 0, outW, outH);

  /* Shift origin so the canvas-coord (bounds.x - padding, bounds.y - padding)
     maps to (0, 0) on the bitmap, then scale once for the entire frame. */
  ctx.translate(-(bounds.x - padding) * scale, -(bounds.y - padding) * scale);
  ctx.scale(scale, scale);

  const loadedImages = await preloadImages(canvas);

  /* Edges first (under shapes) so connector lines don't draw over
     card bodies. Mirrors the live z-stack. */
  for (const e of canvas.edges) drawEdge(ctx, canvas, e);

  /* Sort shapes by z so overlapping shapes render in the order the
     user / agent placed them. */
  const ordered = canvas.shapes.slice().sort((a, b) => a.z - b.z);
  for (const s of ordered) drawShape(ctx, s, loadedImages);

  return off.toDataURL('image/png');
}

/** Save a freshly-rendered canvas screenshot to disk and return the
 *  path. Used by the agent-send pipeline to attach as an image input.
 *  Returns null on render failure / IO failure. The `dirOverride`
 *  arg is used by the unit-test harness; production code lets the
 *  caller resolve `app_data_dir` once and pass it in. */
export async function saveCanvasScreenshot(
  canvas: Canvas,
  dir: string,
  opts: ScreenshotOpts = {}
): Promise<string | null> {
  const dataUrl = await renderCanvasToDataUrl(canvas, opts);
  if (!dataUrl) return null;
  const i = dataUrl.indexOf(',');
  const b64 = i >= 0 ? dataUrl.slice(i + 1) : dataUrl;
  const stamp = `${Date.now()}-${Math.random().toString(36).slice(2, 7)}`;
  const path = `${dir}/canvas-${canvas.id.slice(0, 8)}-${stamp}.png`;
  try {
    await invoke('fs_write_bytes', { path, base64: b64 });
    return path;
  } catch (e) {
    console.warn('canvas screenshot write failed', e);
    return null;
  }
}
