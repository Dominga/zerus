import { describe, expect, it } from "vitest";
import { NODE_H, NODE_W, STEP_X, STEP_Y, crossesCard, layerColumns, orthoPath, placePhase, routeEdges, type LayoutEdge, type LayoutTask } from "./planLayout";

// Task 197 (P1): one graph per phase, layers as columns, orthogonal routes with
// rounded bends that go around the cards (Qt TeamWidgets.h placeWide/routeAll).

const tasks = (layers: number[]): LayoutTask[] => layers.map((layer, index) => ({ id: index + 1, layer }));

describe("planLayout", () => {
  it("wraps a crowded layer into columns and moves later layers right (Qt layerColumns)", () => {
    const spreads = layerColumns(new Map([[0, 1], [1, 13], [2, 2]]));
    expect(spreads.get(0)).toEqual({ start: 0, span: 1, rows: 1, count: 1 });
    expect(spreads.get(1)).toEqual({ start: 1, span: 3, rows: 5, count: 13 });
    expect(spreads.get(2)?.start).toBe(4);
  });

  it("puts each layer in its own column, centered on the tallest", () => {
    const layout = placePhase(tasks([0, 1, 1, 2]), [{ from: 1, to: 2 }, { from: 1, to: 3 }, { from: 2, to: 4 }]);
    const x = (id: number) => layout.rects.get(id)!.x;
    expect([x(1), x(2), x(3), x(4)]).toEqual([14, 14 + STEP_X, 14 + STEP_X, 14 + 2 * STEP_X]);
    // Layer 0 has one card: centered between the two rows of layer 1.
    expect(layout.rects.get(1)!.y).toBe(layout.top + STEP_Y / 2);
    expect(layout.labels.map((label) => [label.layer, label.count])).toEqual([[0, 1], [1, 2], [2, 1]]);
    expect(layout.width).toBe(14 + 2 * STEP_X + NODE_W + 16);
    expect(layout.height).toBe(layout.top + STEP_Y + NODE_H + 26);
  });

  it("puts a task others need above a dead-end in its column (G3)", () => {
    const layout = placePhase(tasks([0, 0, 1]), [{ from: 2, to: 3 }]);
    expect(layout.rects.get(2)!.y).toBeLessThan(layout.rects.get(1)!.y);
  });

  it("routes a next-layer link through the gap with right angles", () => {
    const edges: LayoutEdge[] = [{ from: 1, to: 2 }];
    const layout = placePhase(tasks([0, 1]), edges);
    const [route] = routeEdges(layout, edges);
    expect(route.points.length).toBe(4);
    for (let i = 0; i + 1 < route.points.length; i++) {
      const a = route.points[i];
      const b = route.points[i + 1];
      expect(a.x === b.x || a.y === b.y).toBe(true);
    }
    expect(orthoPath(route.points)).toMatch(/^M [\d.]+ [\d.]+ L .* Q /);
  });

  it("never draws a long link across a card in between", () => {
    // 1 -> 4 skips layer 1, where 2 and 3 stand in its way.
    const all = tasks([0, 1, 1, 2, 1]);
    const edges: LayoutEdge[] = [{ from: 1, to: 4 }, { from: 1, to: 2 }, { from: 3, to: 4 }, { from: 5, to: 4 }];
    const layout = placePhase(all, edges);
    for (const route of routeEdges(layout, edges)) expect(crossesCard(route, layout.rects)).toBeNull();
  });

  it("sends a back link (a cycle) under every card", () => {
    const edges: LayoutEdge[] = [{ from: 2, to: 1 }];
    const layout = placePhase(tasks([0, 1]), edges);
    const [route] = routeEdges(layout, edges);
    expect(route.back).toBe(true);
    expect(Math.max(...route.points.map((point) => point.y))).toBeGreaterThan(layout.bottom);
    expect(crossesCard(route, layout.rects)).toBeNull();
  });

  it("spreads links through one gap into lanes", () => {
    const edges: LayoutEdge[] = [{ from: 1, to: 3 }, { from: 2, to: 3 }, { from: 1, to: 4 }];
    const layout = placePhase(tasks([0, 0, 1, 1]), edges);
    const xs = routeEdges(layout, edges).map((route) => route.points[1].x);
    expect(new Set(xs).size).toBe(3);
  });

  it("rounds a corner at most 7 px, never past half a leg", () => {
    const path = orthoPath([{ x: 0, y: 0 }, { x: 4, y: 0 }, { x: 4, y: 40 }]);
    expect(path).toBe("M 0 0 L 2 0 Q 4 0 4 2 L 4 40");
  });
});
