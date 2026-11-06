import { expect, it, vi } from "vitest";
import { ACTIVITY_OVERSCAN, ACTIVITY_RESIZE_OVERSCAN, ACTIVITY_RESIZE_SETTLE_MS, ActivityHeights, activityMeasureWindow,
  activityOverscan, createActivityResize, syncActivityHeights } from "./activity-heights";

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

it("keeps estimates only for loaded rows across transcript updates (the cache bound)", () => {
  // #1155 review: the update path, not only retain() itself.
  const heights = new ActivityHeights();
  const page = (ids: string[], changed = "") => new Map(ids.map((id) => [id, id === changed ? `${id}*` : id]));
  let loaded = page(["a", "b", "c"]);
  syncActivityHeights(heights, new Map(), loaded);
  for (const id of ["a", "b", "c"]) heights.set(id, 800, 100);
  // An older page lands and the tail is trimmed; "b" changed.
  const next = page(["older", "a", "b"], "b");
  syncActivityHeights(heights, loaded, next); loaded = next;
  expect(heights.get("a", 800)).toBe(100);
  expect(heights.get("b", 800)).toBeUndefined();
  expect(heights.get("c", 800)).toBeUndefined();
  // Another target: nothing of the old transcript remains.
  heights.set("a", 800, 100);
  syncActivityHeights(heights, loaded, page(["x"]));
  for (const id of ["older", "a", "b", "c"]) expect(heights.get(id, 800)).toBeUndefined();
});

it("returns the scroll overscan to eight once widths stop changing", () => {
  vi.useFakeTimers();
  try {
    // The contract is 250 ms, not whatever the constant holds (#1176 review).
    expect(ACTIVITY_RESIZE_SETTLE_MS).toBe(250);
    let resizing = false, settled = 0;
    const widths = createActivityResize((value) => { resizing = value; }, () => { settled++; });
    expect(activityOverscan(resizing)).toBe(ACTIVITY_OVERSCAN);
    widths.resized();
    expect(activityOverscan(resizing)).toBe(ACTIVITY_RESIZE_OVERSCAN);
    vi.advanceTimersByTime(249);
    widths.resized(); // another width change restarts the wait
    vi.advanceTimersByTime(249);
    expect([activityOverscan(resizing), settled]).toEqual([ACTIVITY_RESIZE_OVERSCAN, 0]);
    vi.advanceTimersByTime(1);
    expect([activityOverscan(resizing), settled]).toEqual([8, 1]);
    // Disposal cancels a pending return without settling.
    widths.resized(); widths.dispose(); vi.advanceTimersByTime(250);
    expect([resizing, settled]).toEqual([true, 1]);
  } finally { vi.useRealTimers(); }
});

it("is wired into Activity's transcript updates, virtualizer and width observer", async () => {
  // Read from disk, as row-morph.test.ts does.
  const fs = (await import(/* @vite-ignore */ ["node", "fs"].join(":"))) as { readFileSync(path: URL, encoding: "utf8"): string };
  const activity = fs.readFileSync(new URL("./Activity.tsx", import.meta.url), "utf8");
  expect(activity).toContain("syncActivityHeights(heights, measuredContent, content);");
  expect(activity).toMatch(/const overscan = \(\) => activityOverscan\(resizing\(\)\);/);
  expect(activity).toMatch(/get overscan\(\) \{ return overscan\(\); \}/);
  expect(activity).toMatch(/const widths = createActivityResize\(setResizing, /);
  expect(activity).toContain("if (placed()) { widths.resized(); measureWindow(); }");
  expect(activity).toMatch(/onCleanup\(\(\) => \{ widths\.dispose\(\);/);
});
