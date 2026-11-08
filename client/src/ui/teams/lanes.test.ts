import { describe, expect, it } from "vitest";
import type { TeamTaskView } from "../../types";
import { defaultLaneName, laneEditable, laneGroups, laneMoveUpdate, laneName, laneRenameUpdates, laneTitle, nextLane } from "./lanes";

function task(id: number, phase: number, lane: { n: number; label: string } | null = null, status = "pending"): TeamTaskView {
  return {
    id,
    title: `Task ${id}`,
    status,
    assignee: "dev",
    phase,
    deps: [],
    estimate_minutes: 0,
    attempts: 0,
    attempt_state: "",
    decided_by: "",
    pinned: false,
    output: [],
    waits: "",
    lane,
  };
}

describe("lane groups", () => {
  it("groups a phase by lane, numbered lanes ascending and no lane last", () => {
    const tasks = [
      task(1, 1),
      task(2, 1, { n: 2, label: "Second" }),
      task(3, 1, { n: 1, label: "First" }),
      task(4, 2, { n: 1, label: "Other phase" }),
    ];
    const groups = laneGroups(tasks, 1);
    expect(groups.map((group) => group.lane)).toEqual([1, 2, 0]);
    expect(groups[0].label).toBe("First");
    expect(groups[0].tasks.map((found) => found.id)).toEqual([3]);
    expect(groups[2].tasks.map((found) => found.id)).toEqual([1]);
  });

  it("drops cancelled tasks only when cancelled ones are hidden", () => {
    const tasks = [task(1, 1, null, "cancelled"), task(2, 1, { n: 1, label: "One" })];
    expect(laneGroups(tasks, 1).length).toBe(2);
    expect(laneGroups(tasks, 1, false).map((group) => group.lane)).toEqual([1]);
  });

  it("names lanes and their task counts without a separator dot", () => {
    expect(laneName(2, "Alpha")).toBe("Lane 2 — Alpha");
    expect(laneName(2)).toBe("Lane 2");
    expect(laneName(0)).toBe("No lane — by dependency");
    expect(laneTitle(2, "Alpha", 1)).toBe("Lane 2 — Alpha — 1 task");
    expect(laneTitle(0, "", 3)).toBe("No lane — by dependency — 3 tasks");
    expect(defaultLaneName(4)).toBe("Checkpoint 4");
  });

  it("starts a new lane one past the highest in the phase", () => {
    const tasks = [task(1, 1, { n: 1, label: "a" }), task(2, 1, { n: 3, label: "b" }), task(3, 2, { n: 9, label: "c" })];
    expect(nextLane(tasks, 1)).toBe(4);
    expect(nextLane(tasks, 2)).toBe(10);
    expect(nextLane([], 1)).toBe(1);
  });

  it("renames every task of the lane and moves one task out or in", () => {
    const tasks = [task(1, 1, { n: 1, label: "Old" }), task(2, 1, { n: 1, label: "Old" }), task(3, 1)];
    const renamed = laneRenameUpdates(tasks, 1, 1, "  New name  ");
    expect(renamed).toEqual([
      { id: 1, lane: { n: 1, label: "New name" } },
      { id: 2, lane: { n: 1, label: "New name" } },
    ]);
    expect(laneMoveUpdate(tasks[2], 2, "Later")).toEqual([{ id: 3, lane: { n: 2, label: "Later" } }]);
    expect(laneMoveUpdate(tasks[0], 0)).toEqual([{ id: 1, lane: null }]);
    expect(laneRenameUpdates(tasks, 1, 0, "x")).toEqual([]);
  });

  it("edits lanes in planning, a draft phase, or an accepted phase that has not started", () => {
    const tasks = [
      { phase: 1, status: "draft", attempts: 0 },
      { phase: 2, status: "pending", attempts: 1 },
      { phase: 3, status: "pending", attempts: 0 },
    ];
    const team = {
      status: "running",
      phases: [{ n: 1, state: "draft", status: "" }, { n: 2, state: "accepted", status: "" }, { n: 3, state: "accepted", status: "accepted" }],
      tasks,
    };
    expect(laneEditable(team, 1)).toBe(true);
    expect(laneEditable(team, 2)).toBe(false);
    expect(laneEditable(team, 3)).toBe(true);
    expect(laneEditable({ ...team, status: "planning" }, 2)).toBe(true);
    expect(laneEditable(team, 9)).toBe(false);
  });
});
