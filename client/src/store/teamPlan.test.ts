import { expect, it } from "vitest";
import { createTeamPlanPreferences, PLAN_SECTIONS_MAX, PLAN_TEAMS_MAX, sectionCollapsed } from "./teamPlan";

it("keeps viewer choices per team across mounts without changing another team's defaults", () => {
  const store = createTeamPlanPreferences();
  store.update("a", { filter: "problems", zoom: 0.7, showCancelled: true, hideFinished: true });
  store.toggleSection("a", 3);
  expect(store.get("b")).toMatchObject({ filter: "all", zoom: 1, showCancelled: false, hideFinished: false });
  expect(sectionCollapsed(store.get("a"), 3)).toBe(true);
  store.update("b", { filter: "active" });
  expect(store.get("a")).toMatchObject({ filter: "problems", zoom: 0.7, showCancelled: true, hideFinished: true });
});

it("folds current and later sections and permits an individual exception", () => {
  const store = createTeamPlanPreferences();
  store.update("a", { allCollapsed: true, sections: [] });
  expect(sectionCollapsed(store.get("a"), 300)).toBe(true);
  store.toggleSection("a", 300);
  expect(sectionCollapsed(store.get("a"), 300)).toBe(false);
  expect(sectionCollapsed(store.get("a"), 301)).toBe(true);
  store.update("a", { allCollapsed: false, sections: [] });
  expect(sectionCollapsed(store.get("a"), 301)).toBe(false);
});

it("bounds viewer caches independently of pages and clamps finite zoom values", () => {
  const store = createTeamPlanPreferences();
  for (let i = 0; i <= PLAN_TEAMS_MAX; i++) store.update(String(i), { filter: "active" });
  expect(store.get("0").filter).toBe("all");
  expect(store.get(String(PLAN_TEAMS_MAX)).filter).toBe("active");
  for (let i = 0; i <= PLAN_SECTIONS_MAX; i++) store.toggleSection("a", i);
  expect(store.get("a").sections).toHaveLength(PLAN_SECTIONS_MAX);
  for (const [zoom, expected] of [[0, 0.4], [2, 1.6], [NaN, 1], [Infinity, 1], [0.75, 0.75]]) {
    store.update("a", { zoom }); expect(store.get("a").zoom).toBe(expected);
  }
});
