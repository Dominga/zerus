import { describe, expect, it } from "vitest";
import { closedCards, panelGeometry } from "./inspector-layout";
import { sessionGoal, sessionPlan, usageRows } from "../ui/sessions/inspector-data";
describe("approved session side panel", () => {
  it("uses 400px by default and keeps 360px of Activity before floating", () => {
    expect(panelGeometry(896, 400)).toEqual({ floating: false, width: 400 });
    expect(panelGeometry(650, 400)).toEqual({ floating: false, width: 274 });
    expect(panelGeometry(615, 400)).toEqual({ floating: true, width: 400 });
    expect(panelGeometry(1600, 1200).width).toBe(900);
    expect(panelGeometry(650, 100).width).toBe(240);
  });
  it("bounds remembered card IDs and refuses malformed settings", () => {
    expect(closedCards("null")).toEqual([]); expect(closedCards("bad")).toEqual([]);
    expect(closedCards(JSON.stringify(["plan-main", 3, "x".repeat(1025)]))).toEqual(["plan-main"]);
    expect(closedCards(JSON.stringify(Array.from({ length: 500 }, (_, i) => String(i))))).toHaveLength(128);
  });
  it("shows actual goals, bounded native plan and conversation usage only", () => {
    expect(sessionGoal({ goal: { objective: "Ship", completion_criterion: "Tests pass", status: "active" } }).objective).toBe("Ship");
    expect(sessionPlan({ task_lists: { main: { items: Array.from({ length: 200 }, () => ({ title: "Step", status: "completed" })) } } })).toHaveLength(100);
    expect(sessionPlan({ task_lists: { child: { items: [{ title: "Child" }] } } })).toEqual([]);
    expect(usageRows({ account_usage: { total: 999 } })).toEqual([]);
    expect(usageRows({ session_usage: { status: "ok", totals: { total: 123, output: 3 }, context: { used: 7, limit: 100 } } })).toEqual([["Total tokens", "123"], ["Output", "3"], ["Cost", "Not reported"], ["Latest context", "7 / 100 (7.0%)"]]);
  });
});
