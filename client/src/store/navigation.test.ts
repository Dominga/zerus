import { describe, expect, it } from "vitest";
import { listedOrder, stepArea, stepAttention, stepSession, stepTab } from "./navigation";

// Task 242: Qt's SessionsWindowShortcuts stepSession, stepTab and stepFocus.
const card = (key: string) => ({ type: "card" as const, session: { key } });
const header = (project: string, collapsed: boolean) => ({ type: "header" as const, project, collapsed });

describe("window navigation (task 242)", () => {
  const rows = [header("a", false), card("a1"), card("a2"), header("b", true), header("c", false), card("c1"), { type: "child" }];
  const members = new Map([["b", [{ key: "b1" }, { key: "b2" }]]]);
  const order = listedOrder(rows, members, (key) => key !== "b2");

  it("lists the cards as shown and a collapsed project's listed sessions at its header", () => {
    expect(order).toEqual([
      { key: "a1", hidden: false }, { key: "a2", hidden: false }, { key: "b1", hidden: true }, { key: "c1", hidden: false },
    ]);
  });

  it("steps through the shown sessions and stops at the ends", () => {
    expect(stepSession(order, "a2", 1)).toBe("c1");
    expect(stepSession(order, "c1", -1)).toBe("a2");
    expect(stepSession(order, "c1", 1)).toBeUndefined();
    expect(stepSession(order, "a1", -1)).toBeUndefined();
    expect(stepSession(order, undefined, 1)).toBe("a1");
    expect(stepSession(order, undefined, -1)).toBe("c1");
    expect(stepSession(order, "gone", 1)).toBe("a1");
  });

  it("finds the next session that needs attention, in collapsed projects too, wrapping", () => {
    const needs = (key: string) => key === "b1" || key === "a1";
    expect(stepAttention(order, "a2", 1, needs)).toEqual({ key: "b1", hidden: true });
    expect(stepAttention(order, "b1", 1, needs)).toEqual({ key: "a1", hidden: false });
    expect(stepAttention(order, "a1", -1, needs)).toEqual({ key: "b1", hidden: true });
    expect(stepAttention(order, undefined, 1, needs)?.key).toBe("a1");
    expect(stepAttention(order, "a1", 1, (key) => key === "a1")?.key).toBe("a1");
    expect(stepAttention(order, "a1", 1, () => false)).toBeUndefined();
    expect(stepAttention([], undefined, 1, () => true)).toBeUndefined();
  });

  it("steps to the next shown tab, wrapping", () => {
    const tabs = [{ id: "activity", show: true }, { id: "terminal", show: false }, { id: "processes", show: true }, { id: "team", show: true }];
    expect(stepTab(tabs, "activity", 1)).toBe("processes");
    expect(stepTab(tabs, "team", 1)).toBe("activity");
    expect(stepTab(tabs, "activity", -1)).toBe("team");
    expect(stepTab([{ id: "activity", show: true }], "activity", 1)).toBeUndefined();
  });

  it("moves focus to the next area, wrapping, and starts at an end", () => {
    expect(stepArea(4, 1, 1)).toBe(2);
    expect(stepArea(4, 3, 1)).toBe(0);
    expect(stepArea(4, 0, -1)).toBe(3);
    expect(stepArea(4, -1, 1)).toBe(0);
    expect(stepArea(4, -1, -1)).toBe(3);
    expect(stepArea(0, -1, 1)).toBe(-1);
  });
});
