import { expect, it } from "vitest";
import { activityPositionCursor, createActivityPositions, type ActivityPosition } from "./activity-position";

const position = (top: number): ActivityPosition => ({ following: false, top, anchors: [{ key: "item-reading", offset: -25 }], expanded: {}, more: {} });

it("seeks the visible message rather than replaying all previously loaded pages", () => {
  const events = Array.from({ length: 1000 }, (_, index) => ({ seq: index + 1 }));
  expect(activityPositionCursor(events, [{ key: "item-s650", offset: -50 }])).toBe(649);
  expect(activityPositionCursor([{ seq: 42, activity_display_key: "retained" }], [{ key: "item-retained", offset: 0 }])).toBe(41);
  expect(activityPositionCursor([], [])).toBeUndefined();
});

it("bounds positions by recent visits and keeps session and child identities separate", () => {
  const cache = createActivityPositions(2);
  cache.set("parent", position(100)); cache.set("child", position(200));
  expect(cache.get("parent")?.top).toBe(100);
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
