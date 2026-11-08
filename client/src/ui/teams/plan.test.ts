import { describe, expect, it } from "vitest";
import type { GraphEdge, GraphNode, PlanGraph, TaskView, TeamView } from "./types";
import {
  CARD_W,
  GAP_X,
  ROW_H,
  ancestors,
  buildRows,
  chain,
  descendants,
  edgePath,
  edgeState,
  externalDeps,
  phaseNeedsApproval,
  phasePlans,
  phaseStatus,
  rowEdges,
  rowOrigins,
} from "./plan";

it("archived phase totals survive a partially retained task window", () => {
  const archived = task(1, 1, [], "accepted");
  archived.archived = true;
  const team = { ...TEAM, tasks: [archived], phases: [{ ...TEAM.phases[0], n: 1, done: 80, total: 80, archived: true }] };
  const graph: PlanGraph = { team: team.id, nodes: [], edges: [], layers: 0 };
  const [phase] = phasePlans(team, graph);
  expect(phase.phase.total).toBe(80);
  expect(phase.phase.done).toBe(80);
  expect(phase.nodes.map((node) => node.node.id)).toEqual([1]);
});

function task(id: number, phase: number, deps: number[] = [], status = "pending"): TaskView {
  return {
    id,
    title: `Task ${id}`,
    status,
    assignee: id === 1 ? "lead" : "backend",
    phase,
    deps: deps.map((dep) => ({ task: dep, hard: true })),
    estimate_minutes: 0,
    attempts: 0,
    attempt_state: "",
    decided_by: "",
    pinned: false,
    output: [],
    waits: "",
  };
}

const TEAM: TeamView = {
  id: "team-1",
  name: "Orbit platform",
  status: "running",
  project: "support",
  folder: "/work",
  goal: "",
  plan_mode: "standard",
  lead: "lead",
  revision: 1,
  created: 0,
  updated: 0,
  waiting_for_lead: false,
  members: [
    { id: "lead", role: "Lead", agent: "claude", model: "claude-opus", effort: "high", account: "", computer: "", folder: "", state: "active", activity: "", phase: "", error: "", lead: true, queue: [], live: true, stuck: false, question: false, host_shared: false, minutes_without_progress: 0, session: null },
    { id: "backend", role: "Backend", agent: "deepseek", model: "deepseek-v4", effort: "high", account: "", computer: "", folder: "", state: "active", activity: "", phase: "", error: "", lead: false, queue: [], live: true, stuck: false, question: false, host_shared: false, minutes_without_progress: 0, session: null },
  ],
  tasks: [
    task(1, 1, [], "done"),
    task(2, 1, [1], "done"),
    task(3, 2, [2], "working"),
    task(4, 2, [3]),
    task(5, 2),
    task(6, 3, [4]),
  ],
  phases: [
    { n: 1, title: "Analysis", state: "approved", status: "done", done: 2, total: 2, barrier: false, auto: false, acceptance: "Signed off", paused: false, health: "" },
    { n: 2, title: "Build", state: "approved", status: "in_work", done: 1, total: 3, barrier: true, auto: false, acceptance: "", paused: false, health: "" },
    { n: 3, title: "Verify", state: "draft", status: "draft", done: 0, total: 1, barrier: false, auto: false, acceptance: "", paused: false, health: "" },
  ],
  ready: [],
  cycle: [],
  truncated: false,
  pending_phase: null,
  settings: {},
  proposals: [],
  attention: [],
  validation: { errors: [], warnings: [] },
  health: { critical_path: 0, parallelism: 0, warnings: [] },
  draft: null,
};

const LAYERS: Record<number, number> = { 1: 0, 2: 1, 3: 2, 5: 2, 4: 3, 6: 4 };

function node(task: TaskView): GraphNode {
  return {
    id: task.id,
    title: task.title,
    status: task.status,
    assignee: task.assignee,
    role: task.id === 1 ? "Lead" : "Backend",
    phase: task.phase,
    layer: LAYERS[task.id] ?? 0,
    ready: false,
    in_cycle: false,
  };
}

const GRAPH: PlanGraph = {
  team: "team-1",
  nodes: TEAM.tasks.map(node),
  edges: [
    { from: 1, to: 2, hard: true },
    { from: 2, to: 3, hard: true },
    { from: 3, to: 4, hard: true },
    { from: 4, to: 6, hard: true },
  ],
  layers: 5,
};

describe("dependency highlighting", () => {
  it("lights the whole chain, not an unrelated task", () => {
    expect([...chain(TEAM.tasks, 4)].sort()).toEqual([1, 2, 3, 4, 6]);
    expect([...chain(TEAM.tasks, 5)].sort()).toEqual([5]);
    expect([...ancestors(TEAM.tasks, 6)].sort()).toEqual([1, 2, 3, 4, 6]);
    expect([...descendants(TEAM.tasks, 2)].sort()).toEqual([2, 3, 4, 6]);
  });

  it("ends on a cycle instead of walking it forever", () => {
    const cyclic = [task(1, 1, [2]), task(2, 1, [1])];
    expect([...chain(cyclic, 1)].sort()).toEqual([1, 2]);
    expect([...chain(cyclic, 2)].sort()).toEqual([1, 2]);
  });

  it("lights an edge only when both ends are on the chain", () => {
    const edge: GraphEdge = { from: 1, to: 2, hard: true };
    expect(edgeState(edge, null)).toBe("");
    expect(edgeState(edge, new Set([1]))).toBe("dim");
    expect(edgeState(edge, new Set([1, 2]))).toBe("lit");
    expect(edgeState({ from: 1, to: 4, hard: true }, new Set([1, 2]))).toBe("dim");
  });
});

describe("phase layout", () => {
  it("groups tasks by phase and hgs layer, and keeps cross-phase deps external", () => {
    const phases = phasePlans(TEAM, GRAPH);
    expect(phases.map((phase) => phase.phase.n)).toEqual([1, 2, 3]);
    const build = phases[1];
    expect(build.layers.map((layer) => layer.layer)).toEqual([2, 3]);
    expect(build.layers[0].nodes.map((placed) => placed.node.id)).toEqual([3, 5]);
    expect(build.layers[1].nodes.map((placed) => placed.node.id)).toEqual([4]);
    expect(build.width).toBe(2 * CARD_W + GAP_X);
    expect(build.height).toBe(2 * ROW_H);
    expect(build.edges.map((edge) => `${edge.from}>${edge.to}`)).toEqual(["3>4"]);
    const byId = new Map(TEAM.tasks.map((item) => [item.id, item]));
    expect(externalDeps(2, byId.get(3)!, byId)).toEqual([2]);
    expect(externalDeps(2, byId.get(4)!, byId)).toEqual([]);
  });

  it("puts only the target layer's edges on a row", () => {
    const phases = phasePlans(TEAM, GRAPH);
    const rows = buildRows(phases, new Set());
    const lastBuild = rows.find((row) => row.key === "phase:2:layer:3")!;
    expect(rowEdges(phases[1], lastBuild).map((entry) => `${entry.edge.from}>${entry.edge.to}`)).toEqual(["3>4"]);
    expect(edgePath(phases[1].nodes[0], phases[1].nodes[1], lastBuild.top).startsWith("M ")).toBe(true);
  });

  it("draws a follow-up's origin apart from its dependencies", () => {
    const team = { ...TEAM, tasks: TEAM.tasks.map((row) => (row.id === 4 ? { ...row, origin: 5 } : row)) };
    const phases = phasePlans(team, GRAPH);
    const rows = buildRows(phases, new Set());
    const lastBuild = rows.find((row) => row.key === "phase:2:layer:3")!;
    expect(rowOrigins(phases[1], lastBuild).map((entry) => `${entry.origin}>${entry.to.node.id}`)).toEqual(["5>4"]);
    expect(rowEdges(phases[1], lastBuild).map((entry) => entry.to.node.id)).toEqual([4]);
  });

  it("virtualizes phases as headers and layer rows, not one row per task", () => {
    const phases = phasePlans(TEAM, GRAPH);
    const rows = buildRows(phases, new Set());
    expect(rows.map((row) => row.kind)).toEqual(["header", "layer", "layer", "header", "layer", "layer", "header", "layer"]);
    expect(rows.filter((row) => row.kind === "layer").length).toBeLessThan(TEAM.tasks.length);
    // A finished phase starts collapsed: its layers are not rows at all.
    const collapsed = buildRows(phases, new Set([1]));
    expect(collapsed.filter((row) => row.phase.phase.n === 1)).toHaveLength(1);
    expect(collapsed.find((row) => row.phase.phase.n === 1)!.end).toBe(true);
  });

  it("keeps a 5000-task plan in a few layer rows", () => {
    const tasks: TaskView[] = Array.from({ length: 5000 }, (_, index) => task(index + 1, 1));
    const team: TeamView = { ...TEAM, tasks, phases: [TEAM.phases[0]] };
    const graph: PlanGraph = {
      team: "team-1",
      nodes: tasks.map((item, index) => ({ ...node(item), id: item.id, layer: index % 5 })),
      edges: [],
      layers: 5,
    };
    const rows = buildRows(phasePlans(team, graph), new Set());
    expect(rows.filter((row) => row.kind === "layer")).toHaveLength(5);
    expect(rows.length).toBe(6);
  });

  it("names the phase approval state", () => {
    expect(phaseStatus(TEAM.phases[0])).toEqual({ kind: "done", label: "Done" });
    expect(phaseStatus(TEAM.phases[1])).toEqual({ kind: "running", label: "Running" });
    expect(phaseStatus(TEAM.phases[2])).toEqual({ kind: "draft", label: "Draft" });
    expect(phaseNeedsApproval(TEAM.phases[2])).toBe(true);
    expect(phaseNeedsApproval(TEAM.phases[1])).toBe(false);
  });
});
