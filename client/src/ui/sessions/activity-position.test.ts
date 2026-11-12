import { expect, it } from "vitest";
import { activityAnchorSeq, activityNearestRow, activityPositionCursor, createActivityPositions, type ActivityPosition } from "./activity-position";

const position = (seq: number): ActivityPosition => ({ following: false, seq, anchors: [{ key: "item-reading", offset: -25 }], expanded: {}, more: {} });

it("seeks the visible message rather than replaying all previously loaded pages", () => {
  const events = Array.from({ length: 1000 }, (_, index) => ({ seq: index + 1 }));
  expect(activityPositionCursor(events, [{ key: "item-s650", offset: -50 }])).toBe(649);
  expect(activityPositionCursor([{ seq: 42, activity_display_key: "retained" }], [{ key: "item-retained", offset: 0 }])).toBe(41);
  expect(activityPositionCursor([], [])).toBeUndefined();
});

it("bounds positions by recent visits and keeps session and child identities separate", () => {
  const cache = createActivityPositions(2);
  cache.set("parent", position(100)); cache.set("child", position(200));
  expect(cache.get("parent")?.seq).toBe(100);
  cache.set("other", position(300));
  expect(cache.get("child")).toBeUndefined();
  expect(cache.get("parent")?.anchors).toEqual([{ key: "item-reading", offset: -25 }]);
});

it("retains only bounded visible-row metadata, copied before the old view resets", () => {
  const cache = createActivityPositions();
  const saved = position(100);
  saved.expanded = { reading: true, offscreen: true };
  saved.more = { "reading#0": 200, "offscreen#0": 500 };
  cache.set("session", saved);
  saved.anchors[0].offset = 90; saved.expanded.reading = false; saved.more["reading#0"] = 300;
  expect(cache.get("session")).toMatchObject({ anchors: [{ offset: -25 }], expanded: { reading: true }, more: { "reading#0": 200 } });
  expect(cache.get("session")?.expanded.offscreen).toBeUndefined();
  expect(cache.get("session")?.more["offscreen#0"]).toBeUndefined();
});

it("finds the top saved row's journal seq and the nearest surviving row at or before it", () => {
  const events = [{ seq: 3 }, { seq: 4 }, { seq: 9, activity_display_key: "group" }, { seq: 10, activity_display_key: "group" }, { seq: 12 }, { activity_key: "local" }];
  expect(activityAnchorSeq(events, [{ key: "item-klocal", offset: 0 }, { key: "item-group", offset: 40 }])).toBe(9);
  expect(activityAnchorSeq(events, [{ key: "item-gone", offset: 0 }])).toBeUndefined();
  const rows = ["item-s3", "item-s4", "item-group", "item-s12", "item-klocal"];
  expect(activityNearestRow(events, rows, 11)).toBe("item-group");
  expect(activityNearestRow(events, rows, 9)).toBe("item-group");
  expect(activityNearestRow(events, rows, 8)).toBe("item-s4");
  expect(activityNearestRow(events, rows, 2)).toBeUndefined();
  expect(activityNearestRow(events, rows, undefined)).toBeUndefined();
  expect(activityNearestRow([], [], 50)).toBeUndefined();
});
