// Task 197: the plan graph as Qt's TaskView draws it in its Wide mode
// (tray/src/TeamWidgets.h layerColumns, placeWide, routeAll, orthoPath). One
// phase is one group: its layers are columns left to right, a layer with many
// tasks wraps into several columns, and every dependency inside the phase is
// an orthogonal route with rounded bends that runs through the gaps between
// columns and the corridors between rows, never across a card. Pure, so the
// geometry is tested without a webview.

/** Qt's prototype node and steps (spec 5.4). */
export const NODE_W = 196;
export const NODE_H = 54;
export const STEP_X = 236;
export const STEP_Y = 74;
export const CORNER = 7;
export const LEFT = 14;
export const PAD_TOP = 34;
export const UNDER = 26;
export const LANE = 6;
/** A layer wraps into columns of at most this many rows (owner, 2026-10-28). */
export const WRAP_ROWS = 6;
const GAP = STEP_X - NODE_W;

export interface Rect { x: number; y: number; w: number; h: number }
export interface Point { x: number; y: number }

/** One layer's place: its first column, how many columns and rows it takes. */
export interface LayerSpread { start: number; span: number; rows: number; count: number }

/**
 * Qt's layerColumns: a layer is not always one column. A layer with many
 * tasks wraps into columns of at most `rows` tasks: as tall as the tallest
 * other layer, at least WRAP_ROWS, and about the square root of twice its
 * size. A later layer moves right by the extra columns of the earlier ones.
 */
export function layerColumns(counts: ReadonlyMap<number, number>): Map<number, LayerSpread> {
  let tallest = 0;
  let second = 0;
  for (const count of counts.values()) {
    if (count > tallest) { second = tallest; tallest = count; } else second = Math.max(second, count);
  }
  const cap = Math.max(WRAP_ROWS, Math.max(second, Math.ceil(Math.sqrt(2 * tallest))));
  const spreads = new Map<number, LayerSpread>();
  let extra = 0;
  for (const [layer, count] of [...counts.entries()].sort(([a], [b]) => a - b)) {
    const span = Math.max(1, Math.ceil(count / cap));
    const rows = Math.max(1, Math.ceil(count / span));
    spreads.set(layer, { start: layer + extra, span, rows, count });
    extra += span - 1;
  }
  return spreads;
}

export interface LayoutTask { id: number; layer: number }
export interface LayoutEdge { from: number; to: number }

export interface LayerLabel { layer: number; count: number; x: number; w: number }

export interface PhaseLayout {
  /** Each task's card, in the phase body's own coordinates. */
  rects: Map<number, Rect>;
  /** The screen column of each task (a wrapped layer's columns count apart). */
  columns: Map<number, number>;
  /** The small "L2" labels and counts over each layer's columns. */
  labels: LayerLabel[];
  /** The body's size: the cards, the corridor above and the lane below. */
  width: number;
  height: number;
  /** The cards' top and bottom: the corridors above and below them. */
  top: number;
  bottom: number;
}

/**
 * Qt's placeWide for one phase: columns by layer, each column centered on the
 * tallest. Inside a layer a task other tasks need comes first (G3); a
 * dead-end sinks to the bottom of its column. `minWidth` keeps a short phase
 * as wide as its header.
 */
export function placePhase(tasks: readonly LayoutTask[], edges: readonly LayoutEdge[], minWidth = 0): PhaseLayout {
  const needed = new Set(edges.map((edge) => edge.from));
  const ordered = [...tasks].sort((a, b) => a.layer - b.layer || Number(needed.has(b.id)) - Number(needed.has(a.id)) || a.id - b.id);
  const byLayer = new Map<number, LayoutTask[]>();
  for (const task of ordered) byLayer.set(task.layer, [...(byLayer.get(task.layer) ?? []), task]);
  const spreads = layerColumns(new Map([...byLayer.entries()].map(([layer, list]) => [layer, list.length])));
  let tallest = 1;
  let columns = 0;
  for (const spread of spreads.values()) {
    tallest = Math.max(tallest, spread.rows);
    columns = Math.max(columns, spread.start + spread.span);
  }
  // Layers renumbered to screen order: hgs layers may skip numbers.
  const rects = new Map<number, Rect>();
  const columnOf = new Map<number, number>();
  const labels: LayerLabel[] = [];
  let screenColumn = 0;
  const top = PAD_TOP;
  for (const [layer, list] of [...byLayer.entries()].sort(([a], [b]) => a - b)) {
    const spread = spreads.get(layer)!;
    list.forEach((task, index) => {
      const column = Math.floor(index / spread.rows);
      const row = index % spread.rows;
      const inColumn = Math.min(spread.rows, list.length - column * spread.rows);
      const y = top + row * STEP_Y + ((tallest - inColumn) * STEP_Y) / 2;
      rects.set(task.id, { x: LEFT + (screenColumn + column) * STEP_X, y, w: NODE_W, h: NODE_H });
      columnOf.set(task.id, screenColumn + column);
    });
    labels.push({ layer, count: list.length, x: Math.max(0, LEFT + screenColumn * STEP_X - GAP / 2), w: spread.span * STEP_X });
    screenColumn += spread.span;
  }
  const bottom = top + (tallest - 1) * STEP_Y + NODE_H;
  const width = Math.max(minWidth, LEFT + (Math.max(1, columns) - 1) * STEP_X + NODE_W + 16);
  return { rects, columns: columnOf, labels, width, height: bottom + UNDER, top, bottom };
}

export interface Route { from: number; to: number; points: Point[]; back: boolean }

/**
 * Qt's routeAll inside one phase. Next column: right into the gap, down or
 * up, into the task. Further: into the gap, along the free corridor (above,
 * below, or between rows) with the least deviation, into the gap before the
 * task, into it. Edges through one gap spread by LANE px; a back edge (a
 * cycle) runs under every card. Edges to other phases are not drawn (the
 * card names them, "↤#2").
 */
export function routeEdges(layout: PhaseLayout, edges: readonly LayoutEdge[]): Route[] {
  const uses = new Map<number, number>();
  const lanes = 2 * Math.floor((GAP / 2 - 2) / LANE) + 1;
  const lane = (gx: number) => {
    const key = Math.round(gx);
    const k = (uses.get(key) ?? 0) % lanes;
    uses.set(key, (uses.get(key) ?? 0) + 1);
    return gx + (k % 2 ? 1 : -1) * Math.ceil(k / 2) * LANE;
  };
  // Each column's cards (top, bottom), and the corridors between rows.
  const byColumn = new Map<number, [number, number][]>();
  const corridors: number[] = [];
  for (const [id, rect] of layout.rects) {
    const column = layout.columns.get(id) ?? 0;
    byColumn.set(column, [...(byColumn.get(column) ?? []), [rect.y, rect.y + rect.h]]);
    corridors.push(rect.y - (STEP_Y - NODE_H) / 2, rect.y + rect.h + (STEP_Y - NODE_H) / 2);
  }
  // The rows a long edge must not cross: the cards of the columns between
  // its ends, 6 px around each, merged and sorted.
  const blocked = (la: number, lb: number) => {
    const spans: [number, number][] = [];
    for (const [column, list] of byColumn) if (column > la && column < lb) for (const [t, b] of list) spans.push([t - 6, b + 6]);
    spans.sort((a, b) => a[0] - b[0]);
    const merged: [number, number][] = [];
    for (const span of spans) {
      const last = merged[merged.length - 1];
      if (last && span[0] < last[1]) last[1] = Math.max(last[1], span[1]);
      else merged.push([span[0], span[1]]);
    }
    return merged;
  };
  const free = (spans: [number, number][], y: number) => !spans.some(([t, b]) => y > t && y < b);
  const routes: Route[] = [];
  for (const edge of edges) {
    const a = layout.rects.get(edge.from);
    const b = layout.rects.get(edge.to);
    if (!a || !b || edge.from === edge.to) continue;
    const la = layout.columns.get(edge.from) ?? 0;
    const lb = layout.columns.get(edge.to) ?? 0;
    const x1 = a.x + a.w;
    const y1 = a.y + a.h / 2;
    const x2 = b.x;
    const y2 = b.y + b.h / 2;
    let points: Point[];
    let back = false;
    if (lb <= la) {
      back = true;
      const yb = layout.bottom + 12;
      points = [{ x: x1, y: y1 }, { x: x1 + 14, y: y1 }, { x: x1 + 14, y: yb }, { x: x2 - 14, y: yb }, { x: x2 - 14, y: y2 }, { x: x2 - 2, y: y2 }];
    } else if (lb === la + 1) {
      const gx = lane(a.x + NODE_W + GAP / 2);
      points = [{ x: x1, y: y1 }, { x: gx, y: y1 }, { x: gx, y: y2 }, { x: x2 - 2, y: y2 }];
    } else {
      const spans = blocked(la, lb);
      if (free(spans, y1) && Math.abs(y1 - y2) < 1) {
        points = [{ x: x1, y: y1 }, { x: x2 - 2, y: y2 }];
      } else {
        const candidates = [layout.top - 9, layout.bottom + 10, ...corridors].filter((y) => free(spans, y));
        candidates.sort((u, v) => Math.abs(u - y1) + Math.abs(u - y2) - (Math.abs(v - y1) + Math.abs(v - y2)));
        const cy = candidates[0] ?? layout.bottom + 10;
        const g1 = lane(a.x + NODE_W + GAP / 2);
        const g2 = lane(b.x - GAP / 2);
        points = [{ x: x1, y: y1 }, { x: g1, y: y1 }, { x: g1, y: cy }, { x: g2, y: cy }, { x: g2, y: y2 }, { x: x2 - 2, y: y2 }];
      }
    }
    routes.push({ from: edge.from, to: edge.to, points, back });
  }
  return routes;
}

/** The route as an SVG path with rounded corners (at most CORNER px, at most
 * half of either leg), as Qt's orthoPath. */
export function orthoPath(route: readonly Point[]): string {
  const points: Point[] = [];
  for (const point of route) {
    const last = points[points.length - 1];
    if (!last || last.x !== point.x || last.y !== point.y) points.push(point);
  }
  if (points.length === 0) return "";
  const n = (value: number) => Math.round(value * 10) / 10;
  let path = `M ${n(points[0].x)} ${n(points[0].y)}`;
  for (let i = 1; i + 1 < points.length; i++) {
    const a = points[i - 1];
    const b = points[i];
    const c = points[i + 1];
    const l1 = Math.hypot(b.x - a.x, b.y - a.y);
    const l2 = Math.hypot(c.x - b.x, c.y - b.y);
    const r = Math.min(CORNER, l1 / 2, l2 / 2);
    const p1 = { x: b.x - ((b.x - a.x) / (l1 || 1)) * r, y: b.y - ((b.y - a.y) / (l1 || 1)) * r };
    const p2 = { x: b.x + ((c.x - b.x) / (l2 || 1)) * r, y: b.y + ((c.y - b.y) / (l2 || 1)) * r };
    path += ` L ${n(p1.x)} ${n(p1.y)} Q ${n(b.x)} ${n(b.y)} ${n(p2.x)} ${n(p2.y)}`;
  }
  const last = points[points.length - 1];
  return `${path} L ${n(last.x)} ${n(last.y)}`;
}

/** Whether a straight or bent segment passes through a card other than its
 * ends: what the routes must never do (a test helper and a guard). */
export function crossesCard(route: Route, rects: ReadonlyMap<number, Rect>): number | null {
  for (let i = 0; i + 1 < route.points.length; i++) {
    const a = route.points[i];
    const b = route.points[i + 1];
    for (const [id, rect] of rects) {
      if (id === route.from || id === route.to) continue;
      const left = Math.min(a.x, b.x);
      const right = Math.max(a.x, b.x);
      const top = Math.min(a.y, b.y);
      const bottom = Math.max(a.y, b.y);
      if (right > rect.x + 1 && left < rect.x + rect.w - 1 && bottom > rect.y + 1 && top < rect.y + rect.h - 1) return id;
    }
  }
  return null;
}
