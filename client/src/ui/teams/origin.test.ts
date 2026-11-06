import { describe, expect, it } from "vitest";
import { spawnedFrom } from "./planModes";

// Task #60: a follow-up shows the task it was spawned from, from the graph
// node or the task; none for a task without one.
describe("spawned from", () => {
  it("reads the graph's origin, else the task's, and ignores none", () => {
    expect(spawnedFrom({ origin: 3 })).toBe(3);
    expect(spawnedFrom({ origin: null }, { origin: 4 })).toBe(4);
    expect(spawnedFrom({}, {})).toBeUndefined();
    expect(spawnedFrom({ origin: 0 })).toBeUndefined();
  });
});
