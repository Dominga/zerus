import { expect, it } from "vitest";
import { ancestors, chain, phasePlans } from "./plan";
import { canSetDeps } from "./planTasks";
import { orthoPath, placePhase, routeEdges } from "./planLayout";
import graphView from "../../../core/src/teams/fixtures/graph-view.json";
import type { TeamView } from "../../store/teams";

const cards = () => (graphView as unknown as TeamView).groups.flatMap((group) => group.cards);
it("walks the product page's dependency chain and leaves off-page IDs in it", () => {
  expect([...ancestors(cards(), 3)].sort()).toEqual([1, 2, 3, 4]);
  expect([...chain(cards(), 2)].sort()).toEqual([1, 2, 3]);
  const plan = phasePlans(cards())[0];
  expect(plan.routes.map((route) => [route.from, route.to])).toEqual([[1, 2], [2, 3]]);
  expect(plan.layout.rects.get(3)!.x).toBeGreaterThan(plan.layout.rects.get(2)!.x);
});
it("ends a malformed dependency cycle", () => {
  // An intentionally hand-written malformed overlay tests the walk's bound.
  const tasks = cards(); tasks[0] = { ...tasks[0], deps: [{ node: 3 }] };
  expect([...ancestors(tasks, 1)].sort()).toEqual([1, 2, 3, 4]);
});
it("refuses stages, pending decisions, decided and unknown states in the edit affordance", () => {
  const task = cards()[0];
  expect(canSetDeps(task, "running", false)).toBe(true);
  for (const overlay of [{ kind: "stage" as const }, { pending_decision: "accept" }, { state: "accepted" }, { state: "cancelled" }, { state: "future" }, { state: "in_progress" }, { state: "reported" }, { deps_editable: false }, { deps_editable: undefined }]) {
    expect(canSetDeps({ ...task, ...overlay }, "running", false)).toBe(false);
  }
});
it("wraps a wide layer and routes a long edge through a corridor", () => {
  // Synthetic geometry, with no store rows or task effects.
  const tasks = Array.from({ length: 18 }, (_, id) => ({ id, layer: id === 17 ? 2 : id === 16 ? 1 : 0 }));
  const layout = placePhase(tasks, [{ from: 0, to: 17 }]);
  expect(new Set([...layout.columns.values()]).size).toBeGreaterThan(3);
  const route = routeEdges(layout, [{ from: 0, to: 17 }])[0];
  expect(orthoPath(route.points)).toMatch(/^M /);
  expect(route.points.length).toBeGreaterThan(2);
});
