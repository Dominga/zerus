import { describe, expect, it } from "vitest";
import type { TeamProfileView, TeamTestsView } from "../../types";
import type { PlacedNode } from "./plan";
import {
  boardColumns,
  formatSeconds,
  formatTokens,
  heatShare,
  layerRows,
  lossText,
  runTime,
  stageChips,
  testStatus,
  testsRows,
} from "./planModes";

function placed(id: number, status: string, layer = 0): PlacedNode {
  return {
    node: { id, title: `Task ${id}`, status, assignee: "dev", role: "Backend", phase: 1, layer, ready: false, in_cycle: false },
    task: undefined,
    x: 0,
    y: 0,
  };
}

const phases = [
  { nodes: [placed(1, "pending", 0), placed(2, "in_progress", 0), placed(3, "accepted", 1)] },
  { nodes: [placed(4, "in_test", 1), placed(5, "cancelled", 2)] },
];

const profile: TeamProfileView = {
  team: "t",
  elapsed_s: 120,
  worked_s: 90,
  tasks: 5,
  accepted: 3,
  tokens: 12000,
  tokens_partial: false,
  losses: [
    { kind: "rework", task: 2, seconds: 30, tokens: 300 },
    { kind: "deps", task: 4, seconds: 15, tokens: null },
  ],
  roles: [
    { key: "Backend", kind: "role", accepted: 2, attempts: 3, returned: 1, tokens_per_accepted: 4000 },
    { key: "gpt", kind: "model", accepted: 1, attempts: 1, returned: 0, tokens_per_accepted: null },
  ],
};

const tests: TeamTestsView = {
  team: "t",
  queue: [
    { id: "q1", suite: "test_a", asked_by: "dev", state: "running", task: 1, forced: false, started: 10, seconds: 2, reason: "" },
    { id: "q2", suite: "test_b", asked_by: "lead", state: "waiting", task: 2, forced: true, started: 0, seconds: 0, reason: "no runner" },
  ],
  runs: [
    { id: "r1", suite: "test_a", status: "passed", asked_by: "dev", task: 1, at: 10, duration_ms: 45000, recorded_ms: 0, runner: "box", excerpt: "", flags: [], reused_from: "", due: false, over_budget: false },
    { id: "r2", suite: "test_b", status: "timed_out", asked_by: "lead", task: 2, at: 9, duration_ms: 0, recorded_ms: 0, runner: "", excerpt: "", flags: [], reused_from: "", due: false, over_budget: true },
  ],
  next: 9,
};

describe("board mode", () => {
  it("groups tasks by state and hides cancelled unless shown", () => {
    const hidden = boardColumns(phases, false);
    expect(hidden.map((column) => column.key)).toEqual(["todo", "active", "review", "test", "done"]);
    expect(hidden.find((column) => column.key === "todo")!.nodes.map((node) => node.node.id)).toEqual([1]);
    expect(hidden.find((column) => column.key === "cancelled")).toBeUndefined();
    const shown = boardColumns(phases, true);
    expect(shown.find((column) => column.key === "cancelled")!.nodes.map((node) => node.node.id)).toEqual([5]);
  });
});

describe("layers mode", () => {
  it("puts every phase's tasks in their graph layer", () => {
    expect(layerRows(phases, false).map((row) => [row.layer, row.nodes.map((node) => node.node.id)])).toEqual([
      [0, [1, 2]],
      [1, [3, 4]],
    ]);
  });
});

describe("stages and heat", () => {
  it("keeps the newest run of each stage kind", () => {
    const stages = [
      { kind: "review", state: "passed", attempt: 1, member: "rev" },
      { kind: "test", state: "waiting", attempt: 1, member: "qa" },
      { kind: "review", state: "returned", attempt: 2, member: "rev" },
    ];
    expect(stageChips(stages)).toEqual([
      { kind: "review", state: "returned", attempt: 2 },
      { kind: "test", state: "waiting", attempt: 1 },
    ]);
  });

  it("tints only tasks with a recorded loss, by the top loss", () => {
    expect(heatShare(profile, 2)).toBe(1);
    expect(heatShare(profile, 4)).toBe(0.5);
    expect(heatShare(profile, 1)).toBeNull();
    expect(lossText(profile, 2)).toBe("rework 30s");
  });
});

describe("profile mode", () => {
  it("formats known and unknown values without turning unknown into zero", () => {
    expect(formatSeconds(90)).toBe("1m 30s");
    expect(formatSeconds(3900)).toBe("1h 05m");
    expect(formatTokens(null)).toBe("unknown");
    expect(formatTokens(12000)).toBe("12.0k");
    expect(profile.roles[1].tokens_per_accepted).toBeNull();
  });
});

describe("tests mode", () => {
  it("separates running and waiting requests and keeps run results", () => {
    const rows = testsRows(tests);
    expect(rows.running.map((item) => item.id)).toEqual(["q1"]);
    expect(rows.waiting.map((item) => item.id)).toEqual(["q2"]);
    expect(rows.runs).toHaveLength(2);
    expect(runTime(45000)).toBe("45s");
    expect(testStatus("timed_out")).toBe("timed-out");
    expect(testStatus("passed")).toBe("passed");
  });
});
