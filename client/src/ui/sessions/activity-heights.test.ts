import { expect, it } from "vitest";
import { ActivityHeights, activityMeasureWindow } from "./activity-heights";

it("remeasures only the visible window and eight rows on each side, regardless of loaded count", () => {
  const range = { startIndex: 50, endIndex: 53 };
  expect(activityMeasureWindow(range, 100)).toEqual(Array.from({ length: 20 }, (_, i) => i + 42));
  expect(activityMeasureWindow(range, 10000)).toEqual(activityMeasureWindow(range, 100));
  expect(activityMeasureWindow({ startIndex: 0, endIndex: 1 }, 100)).toEqual(Array.from({ length: 10 }, (_, i) => i));
  expect(activityMeasureWindow({ startIndex: 98, endIndex: 99 }, 100)).toEqual(Array.from({ length: 10 }, (_, i) => i + 90));
  expect(activityMeasureWindow(null, 100)).toEqual([]);
  expect(activityMeasureWindow(range, 100, 2)).toEqual([48, 49, 50, 51, 52, 53, 54, 55]);
});

it("uses separate width estimates without retaining DOM or clearing other loaded rows", () => {
  const heights = new ActivityHeights();
  heights.set("a", 800, 1200); heights.set("b", 800, 300);
  heights.set("a", 600, 1900);
  expect(heights.get("a", 800)).toBe(1200);
  expect(heights.get("a", 600)).toBe(1900);
  expect(heights.get("b", 800)).toBe(300);
  expect(heights.get("b", 600)).toBeUndefined();
  expect(heights.get("a", 801)).toBe(1200); // bucket values are estimates; DOM measurement remains exact
});

it("evicts the least recently measured width after six buckets", () => {
  const heights = new ActivityHeights();
  for (let i = 0; i < 6; i++) heights.set("row", 400 + i * 64, i + 1);
  heights.set("row", 400, 10); // a measured width is active again
  heights.set("row", 900, 20);
  expect(heights.get("row", 464)).toBeUndefined();
  expect(heights.get("row", 400)).toBe(10);
  expect(heights.get("row", 900)).toBe(20);
});

it("forgets changed content/disclosures, removed rows and a previous target or font", () => {
  const heights = new ActivityHeights();
  for (const width of [600, 800]) for (const key of ["changed", "kept", "removed"]) heights.set(key, width, 100);
  heights.invalidate("changed"); heights.retain(new Set(["changed", "kept"]));
  for (const width of [600, 800]) {
    expect(heights.get("changed", width)).toBeUndefined();
    expect(heights.get("removed", width)).toBeUndefined();
    expect(heights.get("kept", width)).toBe(100);
  }
  heights.clear(); expect(heights.get("kept", 800)).toBeUndefined();
});

it("does not store zero or invalid geometry", () => {
  const heights = new ActivityHeights();
  for (const [width, height] of [[0, 100], [600, 0], [600, NaN], [Infinity, 100]]) heights.set("row", width, height);
  expect(heights.get("row", 600)).toBeUndefined();
});
